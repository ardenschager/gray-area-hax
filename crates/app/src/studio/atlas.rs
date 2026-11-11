//! Per-clip frame atlases: 16 evenly spaced frames (4×4 grid of 240×135)
//! in one texture, so grid cells, step thumbnails, emitter tiles and pads
//! are all UV sub-rects of the footage.

use chromagrain_core::timeline::Source;
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions};
use std::collections::HashMap;

pub const FRAMES: usize = 16;
const COLS: usize = 4;
const FW: usize = 240;
const FH: usize = 135;

pub struct Atlas {
    pub tex: TextureHandle,
    /// Average colour of the footage (the 3 px lane-header edge).
    pub swatch: Color32,
    ident: usize,
}

/// Borrow-free handle for UV math + the swatch.
#[derive(Clone, Copy)]
pub struct AtlasView {
    pub swatch: Color32,
}

impl AtlasView {
    pub fn of(a: &Atlas) -> AtlasView {
        AtlasView { swatch: a.swatch }
    }
    pub fn uv_cover(&self, i: usize, aspect: f32) -> Rect {
        uv_cover(i, aspect)
    }
}

/// UV of frame `i` (0..16).
pub fn uv(i: usize) -> Rect {
    let i = i.min(FRAMES - 1);
    let (cx, cy) = ((i % COLS) as f32, (i / COLS) as f32);
    let (w, h) = (1.0 / COLS as f32, 1.0 / (FRAMES / COLS) as f32);
    Rect::from_min_size(egui::pos2(cx * w, cy * h), egui::vec2(w, h))
}

/// UV of frame `i`, centre-cropped to fill a rect of `aspect` (w/h).
pub fn uv_cover(i: usize, aspect: f32) -> Rect {
    let full = uv(i);
    let src_aspect = FW as f32 / FH as f32;
    if aspect > src_aspect {
        let h = full.height() * src_aspect / aspect;
        Rect::from_center_size(full.center(), egui::vec2(full.width(), h))
    } else {
        let w = full.width() * aspect / src_aspect;
        Rect::from_center_size(full.center(), egui::vec2(w, full.height()))
    }
}

/// The frame nearest media position `pos` (0..1).
pub fn frame_at(pos: f32) -> usize {
    ((pos.rem_euclid(1.0)) * FRAMES as f32) as usize % FRAMES
}

#[derive(Default)]
pub struct Atlases {
    map: HashMap<usize, Atlas>,
}

impl Atlases {
    pub fn get(&mut self, ctx: &egui::Context, sources: &[Source], id: usize) -> Option<&Atlas> {
        let src = sources.get(id)?;
        let ident = src
            .video
            .as_ref()
            .map(|v| std::sync::Arc::as_ptr(v) as usize)
            .or_else(|| src.audio.as_ref().map(|a| std::sync::Arc::as_ptr(a) as usize))
            .unwrap_or(0);
        if self.map.get(&id).is_none_or(|a| a.ident != ident) {
            let (img, swatch) = build(src);
            let tex = ctx.load_texture(format!("cg-atlas-{id}-{ident}"), img, TextureOptions::LINEAR);
            self.map.insert(id, Atlas { tex, swatch, ident });
        }
        self.map.get(&id)
    }
}

fn build(src: &Source) -> (ColorImage, Color32) {
    let (w, h) = (FW * COLS, FH * (FRAMES / COLS));
    let mut px = vec![Color32::from_rgb(18, 18, 19); w * h];
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    if let Some(v) = src.video.as_ref().filter(|v| !v.frames.is_empty()) {
        for i in 0..FRAMES {
            let f = &v.frames[((i as f32 + 0.5) / FRAMES as f32 * v.frames.len() as f32) as usize % v.frames.len()];
            let (ox, oy) = ((i % COLS) * FW, (i / COLS) * FH);
            // Cover-crop the frame into 240×135, 2×2 supersampled.
            let (sw, sh) = (f.width as f32, f.height as f32);
            let scale = (sw / FW as f32).min(sh / FH as f32);
            let (cx, cy) = ((sw - FW as f32 * scale) / 2.0, (sh - FH as f32 * scale) / 2.0);
            for y in 0..FH {
                for x in 0..FW {
                    let mut acc = [0u32; 4];
                    for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                        let sx = ((cx + (x as f32 + dx) * scale) as u32).min(f.width - 1);
                        let sy = ((cy + (y as f32 + dy) * scale) as u32).min(f.height - 1);
                        let p = f.get(sx, sy);
                        for c in 0..4 {
                            acc[c] += p[c] as u32;
                        }
                    }
                    let a = acc[3] / 4;
                    // Transparent (cut-out) pixels sit on the panel grey.
                    let mix = |c: u32| ((c / 4) * a / 255 + 18 * (255 - a) / 255) as u8;
                    let c = Color32::from_rgb(mix(acc[0]), mix(acc[1]), mix(acc[2]));
                    px[(oy + y) * w + ox + x] = c;
                    if x % 8 == 0 && y % 8 == 0 {
                        sum[0] += c.r() as u64;
                        sum[1] += c.g() as u64;
                        sum[2] += c.b() as u64;
                        n += 1;
                    }
                }
            }
        }
    } else if let Some(a) = src.audio.as_ref() {
        // Audio-only: a waveform picture per slice.
        let samples = &a.samples;
        for i in 0..FRAMES {
            let (ox, oy) = ((i % COLS) * FW, (i / COLS) * FH);
            let seg = samples.len() / FRAMES;
            for x in 0..FW {
                let s0 = i * seg + x * seg / FW;
                let s1 = (s0 + seg / FW).min(samples.len());
                let peak = samples.get(s0..s1).map(|s| s.iter().fold(0.0f32, |m, v| m.max(v.abs()))).unwrap_or(0.0);
                let hh = (peak.min(1.0) * FH as f32 * 0.45) as usize;
                for y in (FH / 2).saturating_sub(hh)..(FH / 2 + hh).min(FH) {
                    px[(oy + y) * w + ox + x] = Color32::from_rgb(120, 118, 114);
                }
            }
        }
        sum = [60, 60, 60];
        n = 1;
    }
    let swatch = if n > 0 {
        Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8)
    } else {
        Color32::from_gray(60)
    };
    (ColorImage { size: [w, h], pixels: px, source_size: egui::vec2(w as f32, h as f32) }, swatch)
}
