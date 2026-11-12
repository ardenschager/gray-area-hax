//! egui + wgpu without eframe: a painter that owns a wgpu device, an
//! egui-wgpu renderer (with the canvas pipeline installed) and either a
//! surface on a raw window handle or nothing (offscreen).
//!
//! This is the core abstraction for every embedded host:
//! * the VST3 / CLAP editor creates it on the host's parent window
//!   (NSView on macOS — wgpu puts a `CAMetalLayer` on it; HWND on
//!   Windows; X11 window on Linux);
//! * the AUv3 / iPad shell will create it on a UIView's `CAMetalLayer`;
//! * tests render offscreen and read the pixels back.
//!
//! The caller owns the egui `Context` and the input; this only turns a
//! `FullOutput` into pixels.

use crate::gpu::GpuPreview;
use egui_wgpu::{Renderer, ScreenDescriptor};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

pub struct SurfacePainter {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    renderer: Renderer,
    surface: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
    clear: wgpu::Color,
}

fn request(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        force_fallback_adapter: false,
    }))
    .map_err(|e| format!("no GPU adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("chromagrain ui"), ..Default::default() }))
        .map_err(|e| format!("no GPU device: {e}"))?;
    Ok((adapter, device, queue))
}

impl SurfacePainter {
    fn build(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat, surface: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>) -> SurfacePainter {
        let mut renderer = Renderer::new(&device, format, None, 1, false);
        renderer.callback_resources.insert(GpuPreview::new(&device, &queue, format));
        let g0 = crate::theme::G0;
        let clear = wgpu::Color { r: g0.r() as f64 / 255.0, g: g0.g() as f64 / 255.0, b: g0.b() as f64 / 255.0, a: 1.0 };
        SurfacePainter { device, queue, format, renderer, surface, clear }
    }

    /// No window: render into textures (tests, thumbnails).
    pub fn headless() -> Result<SurfacePainter, String> {
        let instance = wgpu::Instance::default();
        let (_, device, queue) = request(&instance, None)?;
        Ok(Self::build(device, queue, wgpu::TextureFormat::Rgba8Unorm, None))
    }

    /// On a native window / view (physical pixel size).
    ///
    /// # Safety
    /// The handles must stay valid for the painter's whole life (drop the
    /// painter before the window goes away).
    pub unsafe fn for_window(display: RawDisplayHandle, window: RawWindowHandle, size_px: [u32; 2]) -> Result<SurfacePainter, String> {
        let instance = wgpu::Instance::default();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: display, raw_window_handle: window })
        }
        .map_err(|e| format!("surface: {e}"))?;
        let (adapter, device, queue) = request(&instance, Some(&surface))?;
        let caps = surface.get_capabilities(&adapter);
        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats).map_err(|e| format!("{e}"))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size_px[0].max(1),
            height: size_px[1].max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        Ok(Self::build(device, queue, format, Some((surface, config))))
    }

    /// The window was resized (physical pixels).
    pub fn resize(&mut self, size_px: [u32; 2]) {
        if let Some((surface, config)) = self.surface.as_mut() {
            if config.width != size_px[0].max(1) || config.height != size_px[1].max(1) {
                config.width = size_px[0].max(1);
                config.height = size_px[1].max(1);
                surface.configure(&self.device, config);
            }
        }
    }

    /// Present one egui frame on the window surface.
    pub fn present(&mut self, ctx: &egui::Context, out: egui::FullOutput) -> Result<(), String> {
        let Some((surface, config)) = self.surface.as_ref() else { return Err("no surface".into()) };
        let size = [config.width, config.height];
        let frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                surface.configure(&self.device, config);
                return Ok(());
            }
            Err(e) => return Err(format!("{e}")),
        };
        let view = frame.texture.create_view(&Default::default());
        self.paint(ctx, out, &view, size);
        frame.present();
        Ok(())
    }

    /// Render one egui frame offscreen and read it back (RGBA8, rows).
    pub fn render_offscreen(&mut self, ctx: &egui::Context, out: egui::FullOutput, size_px: [u32; 2]) -> Vec<u8> {
        let (w, h) = (size_px[0].max(1), size_px[1].max(1));
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui offscreen"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.paint(ctx, out, &view, [w, h]);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::Wait);
        let data = buf.slice(..).get_mapped_range();
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            px.extend_from_slice(&data[y * row as usize..y * row as usize + w as usize * 4]);
        }
        px
    }

    fn paint(&mut self, ctx: &egui::Context, out: egui::FullOutput, view: &wgpu::TextureView, size_px: [u32; 2]) {
        let ppp = out.pixels_per_point;
        for (id, delta) in &out.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, delta);
        }
        let prims = ctx.tessellate(out.shapes, ppp);
        let sd = ScreenDescriptor { size_in_pixels: size_px, pixels_per_point: ppp };
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        let user = self.renderer.update_buffers(&self.device, &self.queue, &mut enc, &prims, &sd);
        {
            let mut rp = enc
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.renderer.render(&mut rp, &prims, &sd);
        }
        self.queue.submit(user.into_iter().chain(std::iter::once(enc.finish())));
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
