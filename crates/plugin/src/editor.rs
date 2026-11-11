//! The plugin editor: the real studio UI (crates/ui), drawn with egui +
//! wgpu on the host's window.
//!
//! nih-plug's stock egui editor (nih_plug_egui / egui-baseview) is OpenGL
//! and pinned to an older egui, while our canvas is wgpu on egui 0.32. So
//! this is a small custom [`Editor`]: baseview only provides the child
//! window and its events (it is the windowing shim nih-plug's editors all
//! use); on that window we create a wgpu surface from the raw handle
//! (`chromagrain_ui::surface::SurfacePainter` — the same abstraction an
//! AUv3 will use on its `CAMetalLayer`), translate baseview input into
//! egui events, and run [`PluginUi`] every frame.
//!
//! The host may resize the editor (VST3 `IPlugView::onSize`, CLAP
//! `gui.set_size`; nih-plug is patched for it, see vendor/nih-plug/PATCHES.md)
//! down to the 960×600 minimum the studio layout holds; the size persists
//! with the session.

use crate::engine::Shared;
use crate::params::CgParams;
use crate::ui_host::PluginUi;
use baseview::{
    Event, EventStatus, MouseButton, MouseEvent, ScrollDelta, Size, Window, WindowEvent, WindowHandler, WindowOpenOptions,
    WindowScalePolicy,
};
use chromagrain_ui::surface::SurfacePainter;
use nih_plug::prelude::*;
use raw_window_handle_05 as rwh05;
use raw_window_handle_05::{HasRawDisplayHandle, HasRawWindowHandle};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Default editor size (logical points): the design's iPad-landscape
/// layout; it holds down to the 960×600 plugin minimum.
pub const DEFAULT_SIZE: (u32, u32) = (1180, 820);
/// Smallest editor the layout holds (the design's minimum).
pub const MIN_SIZE: (u32, u32) = (960, 600);
/// Largest (a 4K screen at 1×).
pub const MAX_SIZE: (u32, u32) = (3840, 2400);

/// The size the editor takes for a host's proposal.
pub fn constrain(width: u32, height: u32) -> (u32, u32) {
    (width.clamp(MIN_SIZE.0, MAX_SIZE.0), height.clamp(MIN_SIZE.1, MAX_SIZE.1))
}

pub fn create(params: Arc<CgParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    Some(Box::new(StudioEditor {
        params,
        shared,
        scale: Mutex::new(None),
        open: Arc::new(AtomicBool::new(false)),
        resize: Arc::new(Mutex::new(None)),
    }))
}

struct StudioEditor {
    params: Arc<CgParams>,
    shared: Arc<Shared>,
    /// Host-reported DPI scale (Windows/Linux; macOS scales natively).
    scale: Mutex<Option<f32>>,
    open: Arc<AtomicBool>,
    /// A host resize for the open window to apply on its next frame.
    resize: Arc<Mutex<Option<(u32, u32)>>>,
}

/// nih-plug's parent handle in baseview's raw-window-handle (0.5).
struct Parent(ParentWindowHandle);

unsafe impl HasRawWindowHandle for Parent {
    fn raw_window_handle(&self) -> rwh05::RawWindowHandle {
        match self.0 {
            ParentWindowHandle::X11Window(window) => {
                let mut h = rwh05::XcbWindowHandle::empty();
                h.window = window;
                rwh05::RawWindowHandle::Xcb(h)
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                let mut h = rwh05::AppKitWindowHandle::empty();
                h.ns_view = ns_view;
                rwh05::RawWindowHandle::AppKit(h)
            }
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let mut h = rwh05::Win32WindowHandle::empty();
                h.hwnd = hwnd;
                rwh05::RawWindowHandle::Win32(h)
            }
        }
    }
}

struct EditorHandle {
    _window: baseview::WindowHandle,
    open: Arc<AtomicBool>,
}

// The window handle is only touched from the GUI thread by the host.
unsafe impl Send for EditorHandle {}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Release);
    }
}

impl Editor for StudioEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let (w, h) = self.size();
        let scale = *self.scale.lock().unwrap();
        let params = self.params.clone();
        let shared = self.shared.clone();
        let resize = self.resize.clone();
        *resize.lock().unwrap() = None;
        let window = Window::open_parented(
            &Parent(parent),
            WindowOpenOptions {
                title: "chromagrain".into(),
                size: Size::new(w as f64, h as f64),
                scale: scale.map(|s| WindowScalePolicy::ScaleFactor(s as f64)).unwrap_or(WindowScalePolicy::SystemScaleFactor),
            },
            move |window| StudioWindow::new(window, params, shared, context, (w, h), scale, resize),
        );
        self.open.store(true, Ordering::Release);
        Box::new(EditorHandle { _window: window, open: self.open.clone() })
    }

    fn size(&self) -> (u32, u32) {
        let (w, h) = *self.params.editor_size.lock().unwrap();
        constrain(w, h)
    }

    fn can_resize(&self) -> bool {
        true
    }

    fn constrain_size(&self, width: u32, height: u32) -> (u32, u32) {
        constrain(width, height)
    }

    fn set_size(&self, width: u32, height: u32) -> bool {
        let size = constrain(width, height);
        *self.params.editor_size.lock().unwrap() = size;
        if self.open.load(Ordering::Acquire) {
            *self.resize.lock().unwrap() = Some(size);
        }
        true
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        if self.open.load(Ordering::Acquire) {
            return false;
        }
        *self.scale.lock().unwrap() = Some(factor);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}

/// Raw handles of a baseview window in raw-window-handle 0.6 (wgpu's).
fn handles(window: &Window) -> Option<(raw_window_handle::RawDisplayHandle, raw_window_handle::RawWindowHandle)> {
    use raw_window_handle as rwh;
    use std::ptr::NonNull;
    match (window.raw_display_handle(), window.raw_window_handle()) {
        (_, rwh05::RawWindowHandle::AppKit(h)) => Some((
            rwh::RawDisplayHandle::AppKit(rwh::AppKitDisplayHandle::new()),
            rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(NonNull::new(h.ns_view)?)),
        )),
        (_, rwh05::RawWindowHandle::Win32(h)) => {
            let mut wh = rwh::Win32WindowHandle::new(std::num::NonZeroIsize::new(h.hwnd as isize)?);
            wh.hinstance = std::num::NonZeroIsize::new(h.hinstance as isize);
            Some((rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()), rwh::RawWindowHandle::Win32(wh)))
        }
        (rwh05::RawDisplayHandle::Xlib(d), rwh05::RawWindowHandle::Xlib(h)) => Some((
            rwh::RawDisplayHandle::Xlib(rwh::XlibDisplayHandle::new(NonNull::new(d.display), d.screen)),
            rwh::RawWindowHandle::Xlib(rwh::XlibWindowHandle::new(h.window)),
        )),
        (rwh05::RawDisplayHandle::Xcb(d), rwh05::RawWindowHandle::Xcb(h)) => Some((
            rwh::RawDisplayHandle::Xcb(rwh::XcbDisplayHandle::new(NonNull::new(d.connection), d.screen)),
            rwh::RawWindowHandle::Xcb(rwh::XcbWindowHandle::new(std::num::NonZeroU32::new(h.window)?)),
        )),
        _ => None,
    }
}

struct StudioWindow {
    ui: PluginUi,
    ctx: egui::Context,
    painter: Option<SurfacePainter>,
    events: Vec<egui::Event>,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    logical: egui::Vec2,
    scale: f32,
    start: Instant,
    resize: Arc<Mutex<Option<(u32, u32)>>>,
    cursor: egui::CursorIcon,
}

impl StudioWindow {
    fn new(
        window: &mut Window,
        params: Arc<CgParams>,
        shared: Arc<Shared>,
        gui: Arc<dyn GuiContext>,
        size: (u32, u32),
        scale: Option<f32>,
        resize: Arc<Mutex<Option<(u32, u32)>>>,
    ) -> StudioWindow {
        let scale = scale.unwrap_or(1.0);
        let phys = [(size.0 as f32 * scale) as u32, (size.1 as f32 * scale) as u32];
        let painter = handles(window).and_then(|(d, w)| match unsafe { SurfacePainter::for_window(d, w, phys) } {
            Ok(p) => Some(p),
            Err(e) => {
                nih_log!("chromagrain editor: no GPU surface ({e}); the UI can't draw");
                None
            }
        });
        let ctx = egui::Context::default();
        let mut ui = PluginUi::new(shared, params, Some(gui));
        ui.host.gpu = painter.is_some();
        ui.studio.install_theme(&ctx);
        StudioWindow {
            ui,
            ctx,
            painter,
            events: Vec::new(),
            pointer: egui::Pos2::ZERO,
            modifiers: egui::Modifiers::default(),
            logical: egui::vec2(size.0 as f32, size.1 as f32),
            scale,
            start: Instant::now(),
            resize,
            cursor: egui::CursorIcon::Default,
        }
    }

    fn mods(m: &keyboard_types::Modifiers) -> egui::Modifiers {
        use keyboard_types::Modifiers as M;
        let mac = cfg!(target_os = "macos");
        egui::Modifiers {
            alt: m.contains(M::ALT),
            ctrl: m.contains(M::CONTROL),
            shift: m.contains(M::SHIFT),
            mac_cmd: mac && m.contains(M::META),
            command: if mac { m.contains(M::META) } else { m.contains(M::CONTROL) },
        }
    }
}

fn key_of(k: &keyboard_types::Key) -> Option<egui::Key> {
    use keyboard_types::Key as K;
    Some(match k {
        K::ArrowDown => egui::Key::ArrowDown,
        K::ArrowLeft => egui::Key::ArrowLeft,
        K::ArrowRight => egui::Key::ArrowRight,
        K::ArrowUp => egui::Key::ArrowUp,
        K::Escape => egui::Key::Escape,
        K::Tab => egui::Key::Tab,
        K::Backspace => egui::Key::Backspace,
        K::Enter => egui::Key::Enter,
        K::Delete => egui::Key::Delete,
        K::Home => egui::Key::Home,
        K::End => egui::Key::End,
        K::Character(s) if s == " " => egui::Key::Space,
        K::Character(s) => egui::Key::from_name(&s.to_uppercase())?,
        _ => return None,
    })
}

impl WindowHandler for StudioWindow {
    fn on_frame(&mut self, window: &mut Window) {
        // A host resize: the view (baseview), then our surface. macOS sends
        // no Resized event for a programmatic resize, so don't wait for one.
        if let Some((w, h)) = self.resize.lock().ok().and_then(|mut r| r.take()) {
            window.resize(Size::new(w as f64, h as f64));
            self.logical = egui::vec2(w as f32, h as f32);
            if let Some(p) = self.painter.as_mut() {
                p.resize([(w as f32 * self.scale).round() as u32, (h as f32 * self.scale).round() as u32]);
            }
        }
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.logical)),
            time: Some(self.start.elapsed().as_secs_f64()),
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.events),
            focused: true,
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.scale);
        let ui = &mut self.ui;
        let out = self.ctx.run(raw, |ctx| ui.frame(ctx));
        let cursor = out.platform_output.cursor_icon;
        if cursor != self.cursor {
            self.cursor = cursor;
            // baseview's macOS `set_mouse_cursor` is `todo!()` (it would
            // panic inside the host): cursors only on Windows / Linux.
            #[cfg(not(target_os = "macos"))]
            window.set_mouse_cursor(match cursor {
                egui::CursorIcon::None => baseview::MouseCursor::Hidden,
                egui::CursorIcon::PointingHand => baseview::MouseCursor::Hand,
                egui::CursorIcon::Grab => baseview::MouseCursor::Hand,
                egui::CursorIcon::Grabbing => baseview::MouseCursor::HandGrabbing,
                egui::CursorIcon::Text => baseview::MouseCursor::Text,
                egui::CursorIcon::ResizeVertical => baseview::MouseCursor::NsResize,
                _ => baseview::MouseCursor::Default,
            });
        }
        if let Some(p) = self.painter.as_mut() {
            if let Err(e) = p.present(&self.ctx, out) {
                nih_log!("chromagrain editor: present failed: {e}");
            }
        }
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        match event {
            Event::Mouse(m) => match m {
                MouseEvent::CursorMoved { position, modifiers } => {
                    self.modifiers = Self::mods(&modifiers);
                    self.pointer = egui::pos2(position.x as f32, position.y as f32);
                    self.events.push(egui::Event::PointerMoved(self.pointer));
                }
                MouseEvent::ButtonPressed { button, modifiers } | MouseEvent::ButtonReleased { button, modifiers } => {
                    let pressed = matches!(m, MouseEvent::ButtonPressed { .. });
                    let button = match button {
                        MouseButton::Left => egui::PointerButton::Primary,
                        MouseButton::Right => egui::PointerButton::Secondary,
                        MouseButton::Middle => egui::PointerButton::Middle,
                        _ => return EventStatus::Ignored,
                    };
                    self.modifiers = Self::mods(&modifiers);
                    self.events.push(egui::Event::PointerButton { pos: self.pointer, button, pressed, modifiers: self.modifiers });
                }
                MouseEvent::WheelScrolled { delta, modifiers } => {
                    let (unit, d) = match delta {
                        ScrollDelta::Lines { x, y } => (egui::MouseWheelUnit::Line, egui::vec2(x, y)),
                        ScrollDelta::Pixels { x, y } => (egui::MouseWheelUnit::Point, egui::vec2(x, y)),
                    };
                    self.events.push(egui::Event::MouseWheel { unit, delta: d, modifiers: Self::mods(&modifiers) });
                }
                MouseEvent::CursorLeft => self.events.push(egui::Event::PointerGone),
                _ => return EventStatus::Ignored,
            },
            Event::Keyboard(k) => {
                let pressed = k.state == keyboard_types::KeyState::Down;
                self.modifiers = Self::mods(&k.modifiers);
                if let Some(key) = key_of(&k.key) {
                    self.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: k.repeat, modifiers: self.modifiers });
                }
                if pressed && !self.modifiers.command {
                    if let keyboard_types::Key::Character(s) = &k.key {
                        self.events.push(egui::Event::Text(s.clone()));
                    }
                }
            }
            Event::Window(WindowEvent::Resized(info)) => {
                self.scale = info.scale() as f32;
                let l = info.logical_size();
                self.logical = egui::vec2(l.width as f32, l.height as f32);
                let p = info.physical_size();
                if let Some(painter) = self.painter.as_mut() {
                    painter.resize([p.width, p.height]);
                }
            }
            Event::Window(WindowEvent::WillClose) => {
                self.painter = None;
            }
            _ => return EventStatus::Ignored,
        }
        EventStatus::Captured
    }
}
