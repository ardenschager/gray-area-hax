//! Video side: frames, HSV color filtering (the visual analog of the audio
//! frequency filters), and the visual grain compositor that consumes the
//! SAME grain events as the audio renderer.
//!
//! Correspondence between an audio grain and its visual grain:
//! * onset & duration — the patch appears and dies with the sound
//! * envelope        — patch opacity follows the audio amplitude envelope
//! * pitch_ratio     — the patch's video plays at the same rate, and its
//!                     hue rotates by `hue_per_semitone * semitones`
//! * pan             — horizontal placement on the canvas
//! * gain            — opacity weight
//! * source_pos      — the patch is sampled from the video at the same
//!                     moment the audio grain reads from the waveform
//! * reverse         — the patch's video runs backwards too

use crate::dsp::grain_env;
use crate::grain::GrainEvent;
use serde::{Deserialize, Serialize};

/// RGBA8 frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn black(width: u32, height: u32) -> Frame {
        let mut data = vec![0u8; (width * height * 4) as usize];
        for px in data.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Frame { width, height, data }
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    #[inline]
    pub fn put(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&px);
    }

    /// Mean luma of the frame (0..255), used by tests/analysis.
    pub fn mean_luma(&self) -> f32 {
        let mut sum = 0.0f64;
        for px in self.data.chunks_exact(4) {
            sum += 0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64;
        }
        (sum / (self.width as f64 * self.height as f64)) as f32
    }
}

/// A decoded (or procedural) video clip held in memory.
#[derive(Debug, Clone, Default)]
pub struct VideoClip {
    pub frames: Vec<Frame>,
    pub fps: f32,
}

impl VideoClip {
    pub fn duration(&self) -> f64 {
        if self.fps <= 0.0 {
            0.0
        } else {
            self.frames.len() as f64 / self.fps as f64
        }
    }

    pub fn frame_at(&self, t: f64) -> Option<&Frame> {
        if self.frames.is_empty() || self.fps <= 0.0 {
            return None;
        }
        let mut idx = (t * self.fps as f64).floor() as i64;
        idx = idx.rem_euclid(self.frames.len() as i64);
        self.frames.get(idx as usize)
    }

    /// Procedural test clip: hue sweeps over time, a bright moving bar gives
    /// spatial structure. Deterministic; used by tests and the demo project.
    pub fn test_pattern(width: u32, height: u32, fps: f32, secs: f32) -> VideoClip {
        let n = (fps * secs) as usize;
        let mut frames = Vec::with_capacity(n);
        for f in 0..n {
            let t = f as f32 / fps;
            let hue = (t * 40.0) % 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 0.8, 0.55);
            let mut frame = Frame::black(width, height);
            let bar_x = ((t * 0.25).fract() * width as f32) as u32;
            for y in 0..height {
                for x in 0..width {
                    let mut px = [r, g, b, 255];
                    let d = (x as i32 - bar_x as i32).abs() as f32;
                    if d < width as f32 * 0.08 {
                        let boost = 1.0 - d / (width as f32 * 0.08);
                        px[0] = (px[0] as f32 + (255.0 - px[0] as f32) * boost) as u8;
                        px[1] = (px[1] as f32 + (255.0 - px[1] as f32) * boost) as u8;
                        px[2] = (px[2] as f32 + (255.0 - px[2] as f32) * boost) as u8;
                    }
                    // Vertical gradient for vertical structure.
                    let vg = 0.6 + 0.4 * (y as f32 / height as f32);
                    px[0] = (px[0] as f32 * vg) as u8;
                    px[1] = (px[1] as f32 * vg) as u8;
                    px[2] = (px[2] as f32 * vg) as u8;
                    frame.put(x, y, px);
                }
            }
            frames.push(frame);
        }
        VideoClip { frames, fps }
    }
}

/// RGB (0..255) -> HSV (h in 0..360, s/v in 0..1).
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = if max <= 0.0 { 0.0 } else { delta / max };
    (h.rem_euclid(360.0), s, max)
}

/// HSV (h in 0..360, s/v in 0..1) -> RGB (0..255).
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorFilterMode {
    /// Keep only pixels inside the hue band (everything else -> transparent).
    Keep,
    /// Remove pixels inside the hue band.
    Remove,
}

/// The visual analog of a band-pass / notch filter: select or reject a hue
/// band. Applied per-pixel when sampling visual grains.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorFilter {
    /// Center hue in degrees, 0..360.
    pub hue_center: f32,
    /// Full width of the hue band in degrees.
    pub hue_width: f32,
    /// Minimum saturation for a pixel to be considered "colored".
    pub sat_min: f32,
    /// Minimum value/brightness for a pixel to be considered.
    pub val_min: f32,
    pub mode: ColorFilterMode,
    /// 0..1: fraction of the band edge used as a soft falloff.
    pub softness: f32,
}

impl ColorFilter {
    pub fn keep(hue_center: f32, hue_width: f32) -> ColorFilter {
        ColorFilter {
            hue_center,
            hue_width,
            sat_min: 0.1,
            val_min: 0.05,
            mode: ColorFilterMode::Keep,
            softness: 0.25,
        }
    }

    pub fn remove(hue_center: f32, hue_width: f32) -> ColorFilter {
        ColorFilter { mode: ColorFilterMode::Remove, ..ColorFilter::keep(hue_center, hue_width) }
    }

    /// Opacity multiplier (0..1) for a pixel under this filter.
    pub fn mask(&self, r: u8, g: u8, b: u8) -> f32 {
        let (h, s, v) = rgb_to_hsv(r, g, b);
        // Distance from center hue, wrapped.
        let mut d = (h - self.hue_center).rem_euclid(360.0);
        if d > 180.0 {
            d = 360.0 - d;
        }
        let half = (self.hue_width / 2.0).max(1e-3);
        let soft = (half * self.softness.clamp(0.0, 1.0)).max(1e-3);
        // 1 inside the band, 0 outside, linear ramp across the soft edge.
        let in_band = if d <= half - soft {
            1.0
        } else if d >= half + soft {
            0.0
        } else {
            1.0 - (d - (half - soft)) / (2.0 * soft)
        };
        // Low-sat / low-val pixels have no meaningful hue: treat as out-of-band.
        let colored = if s >= self.sat_min && v >= self.val_min { 1.0 } else { 0.0 };
        let band = in_band * colored;
        match self.mode {
            ColorFilterMode::Keep => band,
            ColorFilterMode::Remove => 1.0 - band,
        }
    }

    /// Apply destructively to a frame's alpha channel (for previews).
    pub fn apply(&self, frame: &mut Frame) {
        for px in frame.data.chunks_exact_mut(4) {
            let m = self.mask(px[0], px[1], px[2]);
            px[3] = (px[3] as f32 * m) as u8;
        }
    }
}

/// How grain events are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VisualGrainStyle {
    /// Patch size as a fraction of the canvas per second of grain duration.
    /// size_frac = clamp(duration * size_scale, min_size, max_size)
    pub size_scale: f32,
    pub min_size: f32,
    pub max_size: f32,
    /// Degrees of hue rotation per semitone of grain pitch.
    pub hue_per_semitone: f32,
    /// 0 = alpha-over blending, 1 = additive glow. In between blends both.
    pub additive: f32,
    /// Vertical scatter amount (0 = all grains centered vertically).
    pub scatter_y: f32,
}

impl Default for VisualGrainStyle {
    fn default() -> Self {
        VisualGrainStyle {
            size_scale: 2.2,
            min_size: 0.12,
            max_size: 0.65,
            hue_per_semitone: 12.0,
            additive: 0.35,
            scatter_y: 0.7,
        }
    }
}

fn hash01(x: u64) -> f32 {
    // splitmix-ish scramble to a stable [0,1) float per grain id.
    let mut z = x.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z = z ^ (z >> 31);
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Composite every grain active at output time `t` (timeline seconds) onto
/// `out`. Call once per output frame.
pub fn composite_grains_frame(
    clip: &VideoClip,
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    color_filter: Option<&ColorFilter>,
    out: &mut Frame,
    t: f64,
) {
    let ow = out.width as f32;
    let oh = out.height as f32;
    for ev in events {
        if t < ev.onset || t >= ev.end() {
            continue;
        }
        let phase = ((t - ev.onset) / ev.duration as f64) as f32;
        let alpha = grain_env(phase, ev.envelope) * ev.gain.min(1.5);
        if alpha <= 0.003 {
            continue;
        }

        // The visual grain plays its patch of video at the audio's rate.
        let local = (t - ev.onset) * ev.pitch_ratio as f64;
        let src_t = if ev.reverse {
            ev.source_pos + ev.duration as f64 * ev.pitch_ratio as f64 - local
        } else {
            ev.source_pos + local
        };
        let Some(src) = clip.frame_at(src_t) else { continue };

        // Size from duration; position from pan (x) + stable per-grain scatter (y).
        let size = (ev.duration * style.size_scale)
            .clamp(style.min_size, style.max_size);
        let pw = (size * ow).max(2.0) as i32;
        let ph = (size * oh).max(2.0) as i32;
        let cx = (0.5 + 0.45 * ev.pan) * ow;
        let jy = (hash01(ev.id) - 0.5) * style.scatter_y;
        let cy = (0.5 + jy) * oh;
        let x0 = cx as i32 - pw / 2;
        let y0 = cy as i32 - ph / 2;

        // Source patch: sample a window of the source frame centered where
        // the read head is horizontally (source_pos as fraction of clip).
        let src_frac = if clip.duration() > 0.0 {
            (ev.source_pos / clip.duration()).fract() as f32
        } else {
            0.0
        };
        let sw = src.width as f32;
        let sh = src.height as f32;
        let patch_w = sw * size.clamp(0.05, 1.0);
        let patch_h = sh * size.clamp(0.05, 1.0);
        let sx0 = (src_frac * (sw - patch_w)).clamp(0.0, (sw - patch_w).max(0.0));
        let sy0 = ((0.5 + jy * 0.5) * (sh - patch_h)).clamp(0.0, (sh - patch_h).max(0.0));

        let hue_shift = ev.semitones() * style.hue_per_semitone;
        let add = style.additive.clamp(0.0, 1.0);

        for dy in 0..ph {
            let y = y0 + dy;
            if y < 0 || y >= oh as i32 {
                continue;
            }
            let sy = (sy0 + (dy as f32 / ph as f32) * (patch_h - 1.0)) as u32;
            let sy = sy.min(src.height - 1);
            for dx in 0..pw {
                let x = x0 + dx;
                if x < 0 || x >= ow as i32 {
                    continue;
                }
                let sx = (sx0 + (dx as f32 / pw as f32) * (patch_w - 1.0)) as u32;
                let sx = sx.min(src.width - 1);
                let [mut r, mut g, mut b, _] = src.get(sx, sy);

                let mut a = alpha;
                if let Some(cf) = color_filter {
                    a *= cf.mask(r, g, b);
                    if a <= 0.003 {
                        continue;
                    }
                }

                if hue_shift.abs() > 0.5 {
                    let (h, s, v) = rgb_to_hsv(r, g, b);
                    let (nr, ng, nb) = hsv_to_rgb(h + hue_shift, s, v);
                    (r, g, b) = (nr, ng, nb);
                }

                let dst = out.get(x as u32, y as u32);
                let blend = |src_c: u8, dst_c: u8| -> u8 {
                    let s = src_c as f32;
                    let d = dst_c as f32;
                    let over = d + (s - d) * a;
                    let additive = (d + s * a).min(255.0);
                    (over * (1.0 - add) + additive * add) as u8
                };
                out.put(
                    x as u32,
                    y as u32,
                    [blend(r, dst[0]), blend(g, dst[1]), blend(b, dst[2]), 255],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grain::{schedule_grains, GrainSettings};

    #[test]
    fn hsv_roundtrip() {
        for (r, g, b) in [(255, 0, 0), (0, 255, 0), (12, 200, 130), (250, 250, 250)] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!((r as i32 - r2 as i32).abs() <= 2, "{r} vs {r2}");
            assert!((g as i32 - g2 as i32).abs() <= 2);
            assert!((b as i32 - b2 as i32).abs() <= 2);
        }
    }

    #[test]
    fn color_filter_keep_and_remove() {
        // Pure red pixel, keep-red filter passes it; remove-red kills it.
        let keep = ColorFilter::keep(0.0, 60.0);
        let remove = ColorFilter::remove(0.0, 60.0);
        assert!(keep.mask(255, 0, 0) > 0.9);
        assert!(remove.mask(255, 0, 0) < 0.1);
        // A green pixel is outside a red band.
        assert!(keep.mask(0, 255, 0) < 0.1);
        assert!(remove.mask(0, 255, 0) > 0.9);
        // Gray has no hue: treated as out-of-band.
        assert!(keep.mask(128, 128, 128) < 0.1);
        assert!(remove.mask(128, 128, 128) > 0.9);
    }

    #[test]
    fn hue_wraparound_in_filter() {
        // Band centered at 350 with width 40 covers 330..10, so hue 5 (reddish) is in.
        let f = ColorFilter::keep(350.0, 40.0);
        let (r, g, b) = hsv_to_rgb(5.0, 1.0, 1.0);
        assert!(f.mask(r, g, b) > 0.5);
        let (r, g, b) = hsv_to_rgb(180.0, 1.0, 1.0);
        assert!(f.mask(r, g, b) < 0.1);
    }

    #[test]
    fn compositor_draws_active_grains() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let settings = GrainSettings { density: 25.0, ..Default::default() };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let style = VisualGrainStyle::default();

        let mut active = Frame::black(96, 72);
        composite_grains_frame(&clip, &events, &style, None, &mut active, 0.5);
        let mut idle = Frame::black(96, 72);
        composite_grains_frame(&clip, &events, &style, None, &mut idle, 500.0);

        assert!(active.mean_luma() > 1.0, "grains should light up the frame");
        assert!(idle.mean_luma() < 0.5, "no grains active -> black frame");
    }

    #[test]
    fn compositor_respects_color_filter() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let settings = GrainSettings { density: 40.0, ..Default::default() };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let style = VisualGrainStyle { hue_per_semitone: 0.0, ..Default::default() };

        let mut unfiltered = Frame::black(96, 72);
        composite_grains_frame(&clip, &events, &style, None, &mut unfiltered, 0.4);

        // A keep-band that matches nothing (test pattern at t<9s has hue<360
        // sweeping slowly; pick a band far from early hues but sat_min high).
        let cf = ColorFilter {
            sat_min: 0.99,
            ..ColorFilter::keep(200.0, 2.0)
        };
        let mut filtered = Frame::black(96, 72);
        composite_grains_frame(&clip, &events, &style, Some(&cf), &mut filtered, 0.4);

        assert!(filtered.mean_luma() < unfiltered.mean_luma() * 0.2 + 1.0);
    }

    #[test]
    fn test_pattern_has_content() {
        let clip = VideoClip::test_pattern(32, 24, 10.0, 1.0);
        assert_eq!(clip.frames.len(), 10);
        assert!(clip.frames[0].mean_luma() > 10.0);
        assert!(clip.frame_at(0.05).is_some());
        // Wraps.
        assert!(clip.frame_at(100.0).is_some());
    }
}
