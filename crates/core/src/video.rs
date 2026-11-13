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

use crate::dsp::grain_env_skewed;
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

    /// Fully transparent frame — the starting point for a clip's layer.
    pub fn transparent(width: u32, height: u32) -> Frame {
        Frame {
            width,
            height,
            data: vec![0u8; (width * height * 4) as usize],
        }
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

    /// Normalized `(cx, cy)` alpha centroid when this frame is a sparse
    /// segmented cutout (`0.5%..80%` opaque). Returns `None` for normal
    /// full-frame footage (`alpha == 255` everywhere) so full-frame sources
    /// keep their exact patch coordinates.
    pub fn alpha_focus(&self) -> Option<(f32, f32)> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let step_x = (self.width / 48).max(1);
        let step_y = (self.height / 36).max(1);
        let mut w_sum = 0.0f64;
        let mut x_sum = 0.0f64;
        let mut y_sum = 0.0f64;
        let mut samples = 0u64;
        let mut y = 0u32;
        while y < self.height {
            let mut x = 0u32;
            while x < self.width {
                let a = self.data[((y * self.width + x) * 4 + 3) as usize] as f64 / 255.0;
                w_sum += a;
                x_sum += a * (x as f64 + 0.5) / self.width as f64;
                y_sum += a * (y as f64 + 0.5) / self.height as f64;
                samples += 1;
                x += step_x;
            }
            y += step_y;
        }
        let frac = w_sum / samples.max(1) as f64;
        if !(0.005..0.80).contains(&frac) {
            return None;
        }
        Some(((x_sum / w_sum) as f32, (y_sum / w_sum) as f32))
    }

    /// Padded normalized `[x0, y0, w, h]` bounding box of opaque foreground
    /// cutout pixels when the frame is a segmented cutout (`0.2%..55%` opaque).
    /// Allows grain rendering to frame the entire segmented figure intact
    /// instead of guillotining limbs along a square sub-patch crop.
    pub fn alpha_bounds(&self) -> Option<[f32; 4]> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let step_x = (self.width / 64).max(1);
        let step_y = (self.height / 48).max(1);
        let mut x_min = self.width;
        let mut x_max = 0u32;
        let mut y_min = self.height;
        let mut y_max = 0u32;
        let mut opaque = 0u64;
        let mut total = 0u64;
        let mut y = 0u32;
        while y < self.height {
            let mut x = 0u32;
            while x < self.width {
                total += 1;
                let a = self.data[((y * self.width + x) * 4 + 3) as usize];
                if a > 32 {
                    opaque += 1;
                    x_min = x_min.min(x);
                    x_max = x_max.max(x);
                    y_min = y_min.min(y);
                    y_max = y_max.max(y);
                }
                x += step_x;
            }
            y += step_y;
        }
        let frac = opaque as f64 / total.max(1) as f64;
        if !(0.002..0.58).contains(&frac) || x_min > x_max || y_min > y_max {
            return None;
        }
        let fw = self.width as f32;
        let fh = self.height as f32;
        let mut bw = ((x_max - x_min + step_x) as f32 / fw) * 1.16;
        let mut bh = ((y_max - y_min + step_y) as f32 / fh) * 1.16;
        // Keep square-ish normalized aspect so the figure never stretches.
        let span = bw.max(bh).clamp(0.18, 1.0);
        bw = span;
        bh = span;
        let cx = ((x_min + x_max) as f32 * 0.5 + step_x as f32 * 0.5) / fw;
        let cy = ((y_min + y_max) as f32 * 0.5 + step_y as f32 * 0.5) / fh;
        let bx = (cx - bw * 0.5).clamp(0.0, (1.0 - bw).max(0.0));
        let by = (cy - bh * 0.5).clamp(0.0, (1.0 - bh).max(0.0));
        Some([bx, by, bw, bh])
    }
}

/// A decoded (or procedural) video clip held in memory.
#[derive(Debug, Clone, Default)]
pub struct VideoClip {
    pub frames: Vec<Frame>,
    pub fps: f32,
}

impl VideoClip {
    /// `len` seconds of frames from `start` (clamped to the clip).
    pub fn trimmed(&self, start: f64, len: f64) -> VideoClip {
        let fps = self.fps.max(1e-3) as f64;
        let n = self.frames.len();
        let a = ((start.max(0.0) * fps).round() as usize).min(n);
        let b = (((start.max(0.0) + len.max(0.0)) * fps).round() as usize).clamp(a, n);
        VideoClip { frames: self.frames[a..b].to_vec(), fps: self.fps }
    }

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

    /// Temporally blended frame at `t` seconds: linearly crossfades adjacent
    /// source frames so slow-motion snippets (`speed < 1`) glide smoothly
    /// instead of stepping at reduced frame rates.
    pub fn frame_blended_at(&self, t: f64) -> Option<std::borrow::Cow<'_, Frame>> {
        if self.frames.is_empty() || self.fps <= 0.0 {
            return None;
        }
        let pos = t * self.fps as f64;
        let fl = pos.floor();
        let frac = (pos - fl) as f32;
        let n = self.frames.len() as i64;
        let i0 = (fl as i64).rem_euclid(n) as usize;
        if frac <= 0.01 || n <= 1 {
            return Some(std::borrow::Cow::Borrowed(&self.frames[i0]));
        }
        let i1 = ((fl as i64 + 1).rem_euclid(n)) as usize;
        if frac >= 0.99 || i0 == i1 {
            return Some(std::borrow::Cow::Borrowed(&self.frames[i1]));
        }
        let f0 = &self.frames[i0];
        let f1 = &self.frames[i1];
        if f0.width != f1.width || f0.height != f1.height {
            return Some(std::borrow::Cow::Borrowed(f0));
        }
        let inv = 1.0 - frac;
        let data: Vec<u8> = f0
            .data
            .iter()
            .zip(&f1.data)
            .map(|(&a, &b)| (a as f32 * inv + b as f32 * frac + 0.5) as u8)
            .collect();
        Some(std::borrow::Cow::Owned(Frame {
            width: f0.width,
            height: f0.height,
            data,
        }))
    }

    /// Seamless loop-crossfaded frame at elapsed playback time `t` seconds:
    /// when `t` runs past a single clip duration `D`, overlaps the outgoing
    /// tail `[D - X .. D]` with the incoming head `[0 .. X]` via a raised-cosine
    /// RGB/alpha blend (`X = crossfade_sec.min(0.35 * D)`), keeping opaque clips
    /// at full `alpha = 255` with zero jump-cut or straight-alpha dip.
    pub fn frame_looped_at(
        &self,
        t: f64,
        crossfade_sec: f64,
    ) -> Option<std::borrow::Cow<'_, Frame>> {
        let dur = self.duration();
        if dur <= 0.0 || self.frames.is_empty() {
            return None;
        }
        let xfade = crossfade_sec.clamp(0.0, dur * 0.35);
        if t < 0.0 || xfade <= 1.0 / self.fps.max(1.0) as f64 || self.frames.len() < 4 || t < dur - xfade {
            return self.frame_blended_at(t);
        }
        let eff_period = (dur - xfade).max(1e-4);
        let u_total = t - (dur - xfade);
        let cycle_pos = u_total.rem_euclid(eff_period);
        if cycle_pos >= xfade {
            return self.frame_blended_at(cycle_pos);
        }
        let w = (0.5 * (1.0 - (std::f64::consts::PI * (cycle_pos / xfade)).cos())) as f32;
        if w <= 0.005 {
            return self.frame_blended_at(dur - xfade + cycle_pos);
        }
        if w >= 0.995 {
            return self.frame_blended_at(cycle_pos);
        }
        let out_f = self.frame_blended_at(dur - xfade + cycle_pos)?;
        let in_f = self.frame_blended_at(cycle_pos)?;
        if out_f.width != in_f.width || out_f.height != in_f.height {
            return Some(out_f);
        }
        let inv = 1.0 - w;
        let data: Vec<u8> = out_f
            .data
            .iter()
            .zip(&in_f.data)
            .map(|(&a, &b)| (a as f32 * inv + b as f32 * w + 0.5) as u8)
            .collect();
        Some(std::borrow::Cow::Owned(Frame {
            width: out_f.width,
            height: out_f.height,
            data,
        }))
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
        // Low-sat / low-val pixels have no meaningful hue: ramp smoothly
        // across `sat_min` and `val_min` so foam/mist edges never stipple
        // between binary 0 and 1.
        let s_ramp = if self.sat_min <= 1e-4 {
            1.0
        } else {
            let w = (0.06 * (0.25 + self.softness.clamp(0.0, 1.0))).min(self.sat_min);
            let t = ((s - (self.sat_min - w * 0.5)) / w.max(1e-4)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let v_ramp = if self.val_min <= 1e-4 {
            1.0
        } else {
            let w = (0.06 * (0.25 + self.softness.clamp(0.0, 1.0))).min(self.val_min);
            let t = ((v - (self.val_min - w * 0.5)) / w.max(1e-4)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let colored = s_ramp * v_ramp;
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

/// How grain events are drawn (pure style — the audio->visual mapping
/// itself lives in [`AvLink`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VisualGrainStyle {
    /// Patch size as a fraction of the canvas per second of grain duration.
    /// size_frac = clamp(duration * size_scale, min_size, max_size)
    pub size_scale: f32,
    pub min_size: f32,
    pub max_size: f32,
    /// 0 = alpha-over blending, 1 = additive glow. In between blends both.
    pub additive: f32,
    /// Vertical scatter amount (0 = all grains centered vertically).
    pub scatter_y: f32,
    /// Optional soft cosine border taper on visual grain patches (`0..1`).
    /// Defaults to `0.0` so crisp lo-fi grain blocks stay intact unless requested.
    #[serde(default)]
    pub feather: f32,
    /// Visual transient sharpness (`0..1`): `1.0` = raw audio envelope (hard
    /// percussive flashes), `0.0` = calm multi-frame bloom & afterglow.
    /// Defaults to `0.30` so 16th-note percussion glows instead of strobing.
    #[serde(default = "default_flicker")]
    pub flicker: f32,
    /// Optional nocturnal shadow/silhouette crush (`0..1`, default `0.0`):
    /// darkens foreground RGB into high-contrast silhouettes while keeping
    /// cutout alpha intact.
    #[serde(default)]
    pub shadow: f32,
}

fn default_flicker() -> f32 {
    0.30
}

impl Default for VisualGrainStyle {
    fn default() -> Self {
        VisualGrainStyle {
            size_scale: 2.2,
            min_size: 0.12,
            max_size: 0.65,
            additive: 0.35,
            scatter_y: 0.7,
            feather: 0.0,
            flicker: 0.30,
            shadow: 0.0,
        }
    }
}

impl VisualGrainStyle {
    /// Visual lifetime of a grain in seconds (extends very short percussive
    /// grains into a smooth multi-frame decay when `flicker < 1.0`).
    #[inline]
    pub fn visual_duration(&self, audio_duration: f32) -> f64 {
        let fl = self.flicker.clamp(0.0, 1.0);
        if fl >= 0.999 {
            return audio_duration.max(0.001) as f64;
        }
        let min_d = 0.22 * (1.0 - fl) + audio_duration * fl;
        audio_duration.max(min_d).max(0.001) as f64
    }

    /// Blend from full-frame (`t` = 0) to `cloud` (`t` = 1) — the picture
    /// side of the sample <-> grains morph.
    pub fn morph(additive: f32, cloud: &VisualGrainStyle, t: f32) -> VisualGrainStyle {
        let t = t.clamp(0.0, 1.0);
        let full = VisualGrainStyle::full_frame(additive);
        let lin = |a: f32, b: f32| a + (b - a) * t;
        let log = |a: f32, b: f32| (a.max(1e-3).ln() + (b.max(1e-3).ln() - a.max(1e-3).ln()) * t).exp();
        VisualGrainStyle {
            size_scale: log(full.size_scale, cloud.size_scale),
            min_size: lin(full.min_size, cloud.min_size),
            max_size: lin(full.max_size, cloud.max_size),
            additive: lin(additive, cloud.additive),
            scatter_y: lin(0.0, cloud.scatter_y),
            feather: lin(0.0, cloud.feather),
            flicker: lin(1.0, cloud.flicker),
            shadow: lin(0.0, cloud.shadow),
        }
    }

    /// Style for snippet clips: the patch is the whole frame.
    pub fn full_frame(additive: f32) -> VisualGrainStyle {
        VisualGrainStyle {
            size_scale: 1000.0,
            min_size: 1.0,
            max_size: 1.0,
            additive,
            scatter_y: 0.0,
            feather: 0.0,
            flicker: 1.0,
            shadow: 0.0,
        }
    }
}

/// The audio->visual correspondence, per clip. Full correspondence is the
/// default; every mapping is a dial (0 = unlinked, 1 = fully linked).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AvLink {
    /// Grain gain drives visual opacity (0 = opacity ignores gain).
    pub gain_to_opacity: f32,
    /// The audio amplitude envelope drives opacity over the grain's life
    /// (0 = constant opacity while active).
    pub envelope_to_opacity: f32,
    /// Degrees of hue rotation per semitone of grain pitch.
    pub pitch_to_hue: f32,
    /// The visual patch plays its video at the audio playback rate
    /// (0 = patch always plays at 1x).
    pub pitch_to_rate: f32,
    /// Audio pan drives horizontal placement (0 = always centered).
    pub pan_to_x: f32,
    /// Reversed audio grains also play their video backwards.
    pub reverse_video: bool,
    /// The clip transform's x position drives audio pan (0 = unlinked).
    pub x_to_pan: f32,
    /// The clip transform's y drives audio brightness — up = airy, down =
    /// heavy (the elevation cue of the panning model). 0 = unlinked.
    pub y_to_brightness: f32,
    /// The clip transform's scale drives loudness — the distance cue of
    /// the "3D" panning model (smaller = farther = quieter).
    pub scale_to_gain: f32,
}

impl Default for AvLink {
    fn default() -> Self {
        AvLink {
            gain_to_opacity: 1.0,
            envelope_to_opacity: 1.0,
            pitch_to_hue: 2.0,
            pitch_to_rate: 1.0,
            pan_to_x: 1.0,
            reverse_video: true,
            x_to_pan: 1.0,
            y_to_brightness: 1.0,
            scale_to_gain: 1.0,
        }
    }
}

impl AvLink {
    /// Set a link dial by name.
    pub fn set_param(&mut self, name: &str, value: f32) -> Result<(), String> {
        let key = name.to_ascii_lowercase().replace([' ', '-'], "_");
        match key.as_str() {
            "gain_to_opacity" => self.gain_to_opacity = value.clamp(0.0, 1.0),
            "envelope_to_opacity" => self.envelope_to_opacity = value.clamp(0.0, 1.0),
            "pitch_to_hue" | "hue_per_semitone" => self.pitch_to_hue = value,
            "pitch_to_rate" => self.pitch_to_rate = value.clamp(0.0, 1.0),
            "pan_to_x" => self.pan_to_x = value.clamp(0.0, 1.0),
            "reverse_video" => self.reverse_video = value > 0.5,
            "x_to_pan" => self.x_to_pan = value.clamp(0.0, 1.0),
            "y_to_brightness" => self.y_to_brightness = value.clamp(0.0, 1.0),
            "scale_to_gain" => self.scale_to_gain = value.clamp(0.0, 1.0),
            other => return Err(format!("unknown link parameter '{other}'")),
        }
        Ok(())
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

/// Optical carry window (`52 ms`) holding percussive panel freeze-frames across
/// micro-gaps between rapid cuts in `Tile` and `Split` layouts so 16th-note
/// collages never strobe black between hits.
pub const OPTICAL_CARRY_SECS: f64 = 0.052;

/// Geometry + shading for one visual grain at output time `t` — the ONE
/// place grain placement is computed. Both the CPU compositor and the GPU
/// preview consume these, so they cannot drift.
#[derive(Debug, Clone, Copy)]
pub struct GrainDraw {
    /// Destination rect, normalized 0..1 (x, y, w, h), y-down, already
    /// carrying the clip transform's translate/scale.
    pub dest: [f32; 4],
    /// Rotation about the rect center, radians (from the clip transform),
    /// applied in aspect-corrected space.
    pub rotation: f32,
    /// Source patch rect, normalized 0..1 in the source frame.
    pub patch: [f32; 4],
    /// Source video time to sample (seconds; wrap at source duration).
    pub src_time: f64,
    pub alpha: f32,
    pub hue_shift: f32,
    /// Drawn as a tile (layout `Tile`): hard-edged, opaque normal blend,
    /// no feather / shadow / additive.
    pub tiled: bool,
    /// Horizontal mirror (`true` on odd ratchet sub-hits for Aphex rolls).
    pub flip_x: bool,
    /// Basinski tape oxide dropout wear (`0..1`; `0` = pristine, `1` = deep dropout).
    pub oxide_dropout: f32,
    /// Sustained wash layer in `Layout::Split` (bleeds full-canvas at `30%` screen).
    pub is_wash: bool,
    /// Interior shared seam edges `[left, right, top, bottom]` feathered in `Layout::Split`.
    pub feather_edges: [bool; 4],
}

/// Cover-crop source patch `(x, y, w, h)` for a destination panel of aspect
/// `panel_ar` against a source of aspect `src_ar`, centered around normalized
/// subject `focus` `(cx, cy)` and zoomed by `zoom >= 1.0`.
#[inline]
pub fn focus_cover_patch(panel_ar: f32, src_ar: f32, focus: (f32, f32), zoom: f32) -> [f32; 4] {
    let panel_ar = panel_ar.max(1e-4);
    let src_ar = src_ar.max(1e-4);
    let z = zoom.max(1.0);
    let (base_w, base_h) = if panel_ar < src_ar {
        (panel_ar / src_ar, 1.0)
    } else {
        (1.0, src_ar / panel_ar)
    };
    let pw = (base_w / z).clamp(0.05, 1.0);
    let ph = (base_h / z).clamp(0.05, 1.0);
    let fx = if focus.0 <= 0.0 && focus.1 <= 0.0 { 0.5 } else { focus.0.clamp(0.0, 1.0) };
    let fy = if focus.0 <= 0.0 && focus.1 <= 0.0 { 0.5 } else { focus.1.clamp(0.0, 1.0) };
    let px = (fx - pw * 0.5).clamp(0.0, (1.0 - pw).max(0.0));
    let py = (fy - ph * 0.5).clamp(0.0, (1.0 - ph).max(0.0));
    [px, py, pw, ph]
}

/// Compute the draw list for all events active at `t`. `aspect` is the
/// canvas width/height ratio (rotation stays rigid, not skewed).
pub fn grain_draws(
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    source_duration: f64,
    t: f64,
    aspect: f32,
) -> Vec<GrainDraw> {
    grain_draws_opts(events, style, link, transform, source_duration, t, aspect, false)
}

/// [`grain_draws`] with a `parked` flag for the stopped preview: long events
/// (snippets, slow grains) ignore their fade-in/out so a playhead parked at
/// a clip's start shows the picture instead of black.
#[allow(clippy::too_many_arguments)]
pub fn grain_draws_opts(
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    source_duration: f64,
    t: f64,
    aspect: f32,
    parked: bool,
) -> Vec<GrainDraw> {
    grain_draws_layout(events, style, link, transform, source_duration, t, aspect, parked, None)
}

/// [`grain_draws_opts`] with a layout: `tile_grid` = `Some((cols, rows))`
/// for [`Layout::Tile`](crate::tile::Layout) (the frame's grid, from
/// [`crate::tile::frame_grid`] over every visible grain's `tag.tile`).
/// Tile-tagged grains then fill their tile rect ([`crate::tile::tile_rect`])
/// with a focus-centered cover crop of the source, at full opacity times the
/// voice's picture weight (hard cut with ADSR `Cut`); untagged grains and
/// `None` draw as before. In every mode a grain's opacity is multiplied by
/// its voice's picture weight (`tag.picture_at(t)`).
#[allow(clippy::too_many_arguments)]
pub fn grain_draws_layout(
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    source_duration: f64,
    t: f64,
    aspect: f32,
    parked: bool,
    tile_grid: Option<(u32, u32)>,
) -> Vec<GrainDraw> {
    let layout = if tile_grid.is_some() {
        crate::tile::Layout::Tile
    } else {
        crate::tile::Layout::Stack
    };
    grain_draws_stage(
        events,
        style,
        link,
        transform,
        source_duration,
        t,
        aspect,
        parked,
        layout,
        tile_grid,
    )
}

/// Stage-aware grain placement for all four [`crate::tile::Layout`] modes
/// (`Stack`, `Split`, `Tile`, `Bleed`), honoring `VoiceTag::{focus, zoom_punch,
/// ratchet_odd, oxide_dropout, is_wash}` and `52 ms` optical carry on short
/// percussive cuts.
#[allow(clippy::too_many_arguments)]
pub fn grain_draws_stage(
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    source_duration: f64,
    t: f64,
    aspect: f32,
    parked: bool,
    layout: crate::tile::Layout,
    panel_grid: Option<(u32, u32)>,
) -> Vec<GrainDraw> {
    let mut out = Vec::new();
    let rot = transform.rotation.to_radians();
    let (sin_r, cos_r) = rot.sin_cos();
    let tscale = transform.scale.max(0.01);
    let aspect = aspect.max(0.01);
    let fl = style.flicker.clamp(0.0, 1.0);
    for ev in events {
        let dur_f64 = ev.duration.max(0.001) as f64;
        // *MODELL 5* staccato micro-bursts (`<= 85 ms` or `hard_burst`) cut sharply
        // on their exact frame boundary; rhythmic drum/phrase stabs (`85..350 ms`)
        // hold `52 ms` optical carry across micro-gaps.
        let carry = if !parked
            && !ev.tag.hard_burst
            && ev.tag.env.is_none()
            && ev.tag.choke_at.is_none()
            && layout.uses_tiles()
            && ev.duration > 0.085
            && ev.duration < 0.35
        {
            OPTICAL_CARRY_SECS
        } else {
            0.0
        };
        let vis_dur = if parked || ev.tag.hard_burst {
            dur_f64
        } else if layout.uses_tiles() && ev.tag.tile.is_some() {
            dur_f64 + carry
        } else {
            style.visual_duration(ev.duration).max(dur_f64 + carry)
        };
        if t < ev.onset || t >= ev.onset + vis_dur {
            continue;
        }
        let picture = ev.tag.picture_at(t);
        if picture <= 0.0 {
            continue;
        }
        let dt = (t - ev.onset).max(0.0);
        let zoom = if ev.tag.zoom_punch > 1e-4 && !parked {
            1.0 + ev.tag.zoom_punch.clamp(0.0, 0.35) * ((-dt / 0.095).exp() as f32)
        } else {
            1.0
        };
        let oxide = ev.tag.oxide_dropout.clamp(0.0, 1.0);
        let oxide_alpha = 1.0 - 0.82 * oxide;

        // 1. Crisp architectural `Layout::Tile` or feathered `Layout::Split` panel.
        if let (Some((cols, rows)), Some(slot)) = (panel_grid, ev.tag.tile) {
            if layout == crate::tile::Layout::Tile || (layout == crate::tile::Layout::Split && !ev.tag.is_wash) {
                let carry_fade = if dt <= dur_f64 {
                    1.0
                } else if carry > 1e-6 {
                    let u = ((dt - dur_f64) / carry).clamp(0.0, 1.0) as f32;
                    0.5 * (1.0 + (std::f32::consts::PI * u).cos())
                } else {
                    continue;
                };
                if carry_fade <= 0.003 {
                    continue;
                }
                let visual_rate = 1.0 + (ev.pitch_ratio - 1.0) * link.pitch_to_rate;
                // Freeze-frame during optical carry after the audio slice finishes.
                let eff_dt = dt.min(dur_f64);
                let local = eff_dt * visual_rate as f64;
                let src_time = if ev.reverse && link.reverse_video {
                    ev.source_pos + dur_f64 * visual_rate as f64 - local
                } else {
                    ev.source_pos + local
                };
                let (r, feather_edges, tiled) = if layout == crate::tile::Layout::Split {
                    let (sr, fe) = crate::tile::split_rect(slot as usize, cols, rows);
                    (sr, fe, false)
                } else {
                    (crate::tile::tile_rect(slot as usize, cols, rows, aspect), [false; 4], true)
                };
                let tile_ar = r.w * aspect / r.h.max(1e-6);
                let patch = focus_cover_patch(tile_ar, aspect, ev.tag.focus, zoom);
                out.push(GrainDraw {
                    dest: [r.x, r.y, r.w, r.h],
                    rotation: 0.0,
                    patch,
                    src_time,
                    alpha: (picture * carry_fade * oxide_alpha).clamp(0.0, 1.0),
                    hue_shift: ev.semitones() * link.pitch_to_hue,
                    tiled,
                    flip_x: ev.tag.ratchet_odd,
                    oxide_dropout: oxide,
                    is_wash: false,
                    feather_edges,
                });
                continue;
            }
        }

        // 2. Full-canvas optical `Layout::Bleed` OR sustained wash layer in `Layout::Split`.
        if layout == crate::tile::Layout::Bleed
            || (layout == crate::tile::Layout::Split && ev.tag.is_wash)
        {
            let phase = (dt / vis_dur).clamp(0.0, 1.0) as f32;
            let taper = ev.envelope.max(0.18);
            let env = if parked && ev.duration >= 0.25 {
                1.0
            } else {
                grain_env_skewed(phase, taper, ev.env_skew * fl)
            };
            let env_part = 1.0 + (env - 1.0) * link.envelope_to_opacity;
            let gain_part = 1.0 + (ev.gain.min(1.5) - 1.0) * link.gain_to_opacity;
            let wash_scale = if layout == crate::tile::Layout::Split { 0.30 } else { 0.88 };
            let alpha = (env_part * gain_part * picture * oxide_alpha * wash_scale).clamp(0.0, 1.0);
            if alpha <= 0.003 {
                continue;
            }
            let visual_rate = 1.0 + (ev.pitch_ratio - 1.0) * link.pitch_to_rate;
            let local = dt.min(dur_f64) * visual_rate as f64;
            let src_time = if ev.reverse && link.reverse_video {
                ev.source_pos + dur_f64 * visual_rate as f64 - local
            } else {
                ev.source_pos + local
            };
            let patch = focus_cover_patch(aspect, aspect, ev.tag.focus, zoom);
            out.push(GrainDraw {
                dest: [0.0, 0.0, 1.0, 1.0],
                rotation: rot,
                patch,
                src_time,
                alpha,
                hue_shift: ev.semitones() * link.pitch_to_hue,
                tiled: false,
                flip_x: ev.tag.ratchet_odd,
                oxide_dropout: oxide,
                is_wash: true,
                feather_edges: [false; 4],
            });
            continue;
        }

        // 2b. *MODELL 5* Full-Frame Hard Micro-Burst (`PictureCut::Cut` + tight `spray <= 0.08` + micro-grain):
        // renders 100% full-frame (`size = 1.0`, `feather = 0.0`, sharp cut) even in `Stack` mode!
        if ev.tag.hard_burst && layout == crate::tile::Layout::Stack {
            let visual_rate = 1.0 + (ev.pitch_ratio - 1.0) * link.pitch_to_rate;
            let local = dt.min(dur_f64) * visual_rate as f64;
            let src_time = if ev.reverse && link.reverse_video {
                ev.source_pos + dur_f64 * visual_rate as f64 - local
            } else {
                ev.source_pos + local
            };
            let patch = focus_cover_patch(aspect, aspect, ev.tag.focus, zoom);
            out.push(GrainDraw {
                dest: [0.0, 0.0, 1.0, 1.0],
                rotation: 0.0,
                patch,
                src_time,
                alpha: (picture * oxide_alpha).clamp(0.0, 1.0),
                hue_shift: ev.semitones() * link.pitch_to_hue,
                tiled: true,
                flip_x: ev.tag.ratchet_odd,
                oxide_dropout: oxide,
                is_wash: false,
                feather_edges: [false; 4],
            });
            continue;
        }

        // 3. Classic `Layout::Stack` (and untagged grains).
        let phase = ((t - ev.onset) / vis_dur) as f32;
        let (vis_env_taper, vis_skew, flash_damp) = if fl < 0.999 && !parked {
            let taper = ev.envelope.max((1.0 - fl) * 0.85);
            let skew = ev.env_skew * fl;
            let damp = if vis_dur > ev.duration as f64 {
                ((ev.duration.max(0.005) as f64 / vis_dur).powf(0.35 * (1.0 - fl as f64))) as f32
            } else {
                1.0
            };
            (taper, skew, damp)
        } else if !parked && style.min_size >= 0.99 && ev.duration > 0.35 && ev.envelope > 0.005 {
            // Full-frame snippets get at least ~3.5 frames (0.14s) of smooth
            // entry/exit fade so hard cuts into bright footage never flash.
            let min_taper = (0.28 / vis_dur.max(0.28)) as f32;
            (ev.envelope.max(min_taper), ev.env_skew, 1.0)
        } else {
            (ev.envelope, ev.env_skew, 1.0)
        };
        let mut env = grain_env_skewed(phase, vis_env_taper, vis_skew) * flash_damp;
        if parked && ev.duration >= 0.25 {
            env = env.max(grain_env_skewed(0.5, ev.envelope, ev.env_skew));
        }
        let env_part = 1.0 + (env - 1.0) * link.envelope_to_opacity;
        let gain_part = 1.0 + (ev.gain.min(1.5) - 1.0) * link.gain_to_opacity;
        let alpha = (env_part * gain_part * picture * oxide_alpha).clamp(0.0, 1.0);
        if alpha <= 0.003 {
            continue;
        }
        let visual_rate = 1.0 + (ev.pitch_ratio - 1.0) * link.pitch_to_rate;
        let local = (t - ev.onset) * visual_rate as f64;
        let reverse = ev.reverse && link.reverse_video;
        let src_time = if reverse {
            ev.source_pos + ev.duration as f64 * visual_rate as f64 - local
        } else {
            ev.source_pos + local
        };

        let size = (ev.duration * style.size_scale).clamp(style.min_size, style.max_size);
        let cx = 0.5 + 0.45 * ev.pan * link.pan_to_x;
        let jy = (hash01(ev.id) - 0.5) * style.scatter_y;
        let cy = 0.5 + jy;

        // Clip transform: scale + rotate about the canvas center (in
        // aspect-corrected units), then translate. Half-canvas x/y units.
        let mut ox = (cx - 0.5) * tscale * aspect;
        let mut oy = (cy - 0.5) * tscale;
        (ox, oy) = (ox * cos_r - oy * sin_r, ox * sin_r + oy * cos_r);
        let cx = 0.5 + ox / aspect + transform.x * 0.5;
        let cy = 0.5 + oy + transform.y * 0.5;
        let size = size * tscale * zoom;

        let src_frac = if source_duration > 0.0 {
            (ev.source_pos / source_duration).fract() as f32
        } else {
            0.0
        };
        let patch_size = (size / zoom).clamp(0.05, 1.0);
        let has_custom_focus =
            (ev.tag.focus.0 - 0.5).abs() > 0.01 || (ev.tag.focus.1 - 0.5).abs() > 0.01;
        let (sx0, sy0) = if has_custom_focus && patch_size < 0.98 {
            (
                (ev.tag.focus.0 - patch_size * 0.5).clamp(0.0, (1.0 - patch_size).max(0.0)),
                (ev.tag.focus.1 - patch_size * 0.5).clamp(0.0, (1.0 - patch_size).max(0.0)),
            )
        } else {
            (
                (src_frac * (1.0 - patch_size)).clamp(0.0, (1.0 - patch_size).max(0.0)),
                ((0.5 + jy * 0.5) * (1.0 - patch_size))
                    .clamp(0.0, (1.0 - patch_size).max(0.0)),
            )
        };

        out.push(GrainDraw {
            dest: [cx - size / 2.0, cy - size / 2.0, size, size],
            rotation: rot,
            patch: [sx0, sy0, patch_size, patch_size],
            src_time,
            alpha,
            hue_shift: ev.semitones() * link.pitch_to_hue,
            tiled: false,
            flip_x: ev.tag.ratchet_odd,
            oxide_dropout: oxide,
            is_wash: ev.tag.is_wash,
            feather_edges: [false; 4],
        });
    }
    out
}

#[inline]
fn border_feather(qx: f32, qy: f32, feather: f32) -> f32 {
    let f = (feather.clamp(0.0, 1.0) * 0.45).max(0.0);
    if f <= 1e-4 {
        return 1.0;
    }
    let edge = |q: f32| -> f32 {
        let dist = q.min(1.0 - q).clamp(0.0, f);
        let u = dist / f;
        0.5 - 0.5 * (std::f32::consts::PI * u).cos()
    };
    edge(qx) * edge(qy)
}

#[inline]
fn split_edge_feather(qx: f32, qy: f32, edges: [bool; 4]) -> f32 {
    if !edges.iter().any(|&e| e) {
        return 1.0;
    }
    // Interior overlap zone spans ~11.5% of the expanded panel width (`2 * 0.065 / 1.13`).
    const ZONE: f32 = 0.13;
    let taper = |dist: f32| -> f32 {
        let u = (dist / ZONE).clamp(0.0, 1.0);
        0.5 - 0.5 * (std::f32::consts::PI * u).cos()
    };
    let mut w = 1.0f32;
    if edges[0] && qx < ZONE {
        w *= taper(qx);
    }
    if edges[1] && (1.0 - qx) < ZONE {
        w *= taper(1.0 - qx);
    }
    if edges[2] && qy < ZONE {
        w *= taper(qy);
    }
    if edges[3] && (1.0 - qy) < ZONE {
        w *= taper(1.0 - qy);
    }
    w
}

/// Composite every grain active at output time `t` (timeline seconds) onto
/// `out` — a transparent layer with straight-alpha accumulation. Geometry
/// comes from [`grain_draws`] (shared with the GPU preview); this function
/// rasterizes each draw, supporting the clip transform's rotation.
#[allow(clippy::too_many_arguments)]
pub fn composite_grains_frame(
    clip: &VideoClip,
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    color_filter: Option<&ColorFilter>,
    out: &mut Frame,
    t: f64,
) {
    composite_grains_frame_opts(clip, events, style, link, transform, color_filter, out, t, false, None);
}

/// [`composite_grains_frame`] with optional neighbor-frame temporal crossfade
/// (`blend_frames`, used for slow-motion snippets) and optional `hue_override`
/// (preserving pitch-decoupled hue on stretched snippets).
#[allow(clippy::too_many_arguments)]
pub fn composite_grains_frame_opts(
    clip: &VideoClip,
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    color_filter: Option<&ColorFilter>,
    out: &mut Frame,
    t: f64,
    blend_frames: bool,
    hue_override: Option<f32>,
) {
    composite_grains_frame_layout(clip, events, style, link, transform, color_filter, out, t, blend_frames, hue_override, None);
}

/// [`composite_grains_frame_opts`] with a tile grid (layout `Tile`; see
/// [`grain_draws_layout`]). Tiles are drawn hard-edged and opaque.
#[allow(clippy::too_many_arguments)]
pub fn composite_grains_frame_layout(
    clip: &VideoClip,
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    color_filter: Option<&ColorFilter>,
    out: &mut Frame,
    t: f64,
    blend_frames: bool,
    hue_override: Option<f32>,
    tile_grid: Option<(u32, u32)>,
) {
    let layout = if tile_grid.is_some() {
        crate::tile::Layout::Tile
    } else {
        crate::tile::Layout::Stack
    };
    composite_grains_frame_stage(
        clip,
        events,
        style,
        link,
        transform,
        color_filter,
        out,
        t,
        blend_frames,
        hue_override,
        layout,
        tile_grid,
    );
}

/// Stage-aware CPU grain rasterizer supporting all four [`crate::tile::Layout`]
/// modes (`Stack`, `Split` with feathered screen-blended seams & `30%` wash bleed,
/// `Tile` crisp architectural grid, and `Bleed` full-canvas multi-exposure melt),
/// plus `focus` subject crop, `ratchet_odd` horizontal mirror, and `oxide_dropout`
/// ferric wear.
#[allow(clippy::too_many_arguments)]
pub fn composite_grains_frame_stage(
    clip: &VideoClip,
    events: &[GrainEvent],
    style: &VisualGrainStyle,
    link: &AvLink,
    transform: &crate::timeline::ClipTransform,
    color_filter: Option<&ColorFilter>,
    out: &mut Frame,
    t: f64,
    blend_frames: bool,
    hue_override: Option<f32>,
    layout: crate::tile::Layout,
    panel_grid: Option<(u32, u32)>,
) {
    let ow = out.width as f32;
    let oh = out.height as f32;
    let aspect = ow / oh.max(1.0);
    let add = style.additive.clamp(0.0, 1.0);
    let feath = style.feather.clamp(0.0, 1.0);
    let shad = style.shadow.clamp(0.0, 1.0);
    let draws = grain_draws_stage(
        events,
        style,
        link,
        transform,
        clip.duration(),
        t,
        aspect,
        false,
        layout,
        panel_grid,
    );

    for d in draws {
        let is_split_panel =
            layout == crate::tile::Layout::Split && !d.is_wash && d.feather_edges.iter().any(|&e| e);
        let use_screen = layout == crate::tile::Layout::Bleed
            || (layout == crate::tile::Layout::Split && (d.is_wash || is_split_panel));
        let (add, feath, shad) = if d.tiled || layout == crate::tile::Layout::Split {
            (0.0, 0.0, 0.0)
        } else {
            (add, feath, shad)
        };
        let src_cow = if blend_frames {
            clip.frame_looped_at(d.src_time, 0.32)
        } else {
            clip.frame_at(d.src_time).map(std::borrow::Cow::Borrowed)
        };
        let Some(src) = src_cow else { continue };
        let alpha = d.alpha;
        let hue_shift = hue_override.unwrap_or(d.hue_shift);

        // If the source frame is a segmented cutout (`alpha_bounds` is Some),
        // sample the entire padded silhouette bounding box rather than cropping
        // a small square window that guillotines limbs along a rectangle edge.
        let mut patch = d.patch;
        if !d.tiled && layout == crate::tile::Layout::Stack && patch[2] < 0.98 {
            if let Some(bounds) = src.alpha_bounds() {
                patch = bounds;
            } else if let Some((fx, fy)) = src.alpha_focus() {
                let target_x = (fx - patch[2] * 0.5).clamp(0.0, (1.0 - patch[2]).max(0.0));
                let target_y = (fy - patch[3] * 0.5).clamp(0.0, (1.0 - patch[3]).max(0.0));
                patch[0] = patch[0] * 0.25 + target_x * 0.75;
                patch[1] = patch[1] * 0.25 + target_y * 0.75;
            }
        }

        // Dest rect in pixels.
        let rw = (d.dest[2] * ow / 2.0).max(1.0); // half extents
        let rh = (d.dest[3] * oh / 2.0).max(1.0);
        let cx = (d.dest[0] + d.dest[2] / 2.0) * ow;
        let cy = (d.dest[1] + d.dest[3] / 2.0) * oh;
        let (sin_r, cos_r) = d.rotation.sin_cos();

        // AABB of the (possibly rotated) rect.
        let ext_x = rw * cos_r.abs() + rh * sin_r.abs();
        let ext_y = rw * sin_r.abs() + rh * cos_r.abs();
        let x_min = ((cx - ext_x).floor() as i32).max(0);
        let x_max = ((cx + ext_x).ceil() as i32).min(ow as i32 - 1);
        let y_min = ((cy - ext_y).floor() as i32).max(0);
        let y_max = ((cy + ext_y).ceil() as i32).min(oh as i32 - 1);
        if x_min > x_max || y_min > y_max {
            continue;
        }

        let sw = src.width as f32;
        let sh = src.height as f32;
        for y in y_min..=y_max {
            for x in x_min..=x_max {
                // Inverse-rotate the pixel offset into rect space.
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let ux = dx * cos_r + dy * sin_r;
                let uy = -dx * sin_r + dy * cos_r;
                let qx = ux / rw * 0.5 + 0.5;
                let qy = uy / rh * 0.5 + 0.5;
                if !(0.0..1.0).contains(&qx) || !(0.0..1.0).contains(&qy) {
                    continue;
                }
                let sample_qx = if d.flip_x { 1.0 - qx } else { qx };
                let sx = ((patch[0] + sample_qx * patch[2]) * (sw - 1.0)) as u32;
                let sy = ((patch[1] + qy * patch[3]) * (sh - 1.0)) as u32;
                let [mut r, mut g, mut b, ta] =
                    src.get(sx.min(src.width - 1), sy.min(src.height - 1));

                // The source frame's own alpha participates: segmented
                // (masked) sources composite only where the object is.
                let seam_f = split_edge_feather(qx, qy, d.feather_edges);
                let mut a = alpha
                    * (ta as f32 / 255.0)
                    * border_feather(qx, qy, feath)
                    * seam_f;
                if a <= 0.003 {
                    continue;
                }
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
                // Basinski tape oxide chroma wear: desaturates toward warm sepia ferric oxide.
                if d.oxide_dropout > 0.015 {
                    let ox = d.oxide_dropout.clamp(0.0, 1.0);
                    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
                    let keep_c = 1.0 - 0.65 * ox;
                    r = (r as f32 * keep_c + luma * 1.08 * (1.0 - keep_c)).clamp(0.0, 255.0) as u8;
                    g = (g as f32 * keep_c + luma * 0.94 * (1.0 - keep_c)).clamp(0.0, 255.0) as u8;
                    b = (b as f32 * keep_c + luma * 0.76 * (1.0 - keep_c)).clamp(0.0, 255.0) as u8;
                }
                if shad > 0.002 {
                    let luma = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0;
                    let sx_c = sx.min(src.width - 1);
                    let sy_c = sy.min(src.height - 1);
                    let a_l = src.get(sx_c.saturating_sub(2), sy_c)[3];
                    let a_r = src.get((sx_c + 2).min(src.width - 1), sy_c)[3];
                    let a_u = src.get(sx_c, sy_c.saturating_sub(2))[3];
                    let a_d = src.get(sx_c, (sy_c + 2).min(src.height - 1))[3];
                    let min_nb = a_l.min(a_r).min(a_u).min(a_d);
                    let edge_grad = (ta.saturating_sub(min_nb)) as f32 / 255.0;
                    let ta_n = ta as f32 / 255.0;
                    let rim = (edge_grad * 1.4 + (ta_n * (1.0 - ta_n) * 2.8)).clamp(0.0, 1.0);

                    let keep = (1.0 - 0.72 * shad) * (0.58 + 0.42 * luma);
                    let floor_lift = 18.0 * shad * (0.35 + 0.65 * luma);
                    let rim_boost = 92.0 * shad * rim;
                    r = (r as f32 * keep + floor_lift + rim_boost * 1.08).clamp(0.0, 255.0) as u8;
                    g = (g as f32 * keep + floor_lift + rim_boost * 0.90).clamp(0.0, 255.0) as u8;
                    b = (b as f32 * keep + floor_lift + rim_boost * 0.68).clamp(0.0, 255.0) as u8;
                }

                let dst = out.get(x as u32, y as u32);
                let da = dst[3] as f32 / 255.0;
                let out_a = (a + da * (1.0 - a)).max(1e-6);
                let blend = |src_c: u8, dst_c: u8| -> u8 {
                    let s = src_c as f32;
                    let dd = dst_c as f32;
                    if use_screen && da > 0.01 {
                        let sn = (s * a) / 255.0;
                        let dn = (dd * da) / 255.0;
                        let scr = 1.0 - (1.0 - sn) * (1.0 - dn);
                        let over = (s * a + dd * da * (1.0 - a)) / (out_a * 255.0);
                        let w_scr = if seam_f < 0.999 || d.is_wash { 0.85 } else { 0.35 };
                        ((over * (1.0 - w_scr) + (scr / out_a).min(1.0) * w_scr) * 255.0)
                            .clamp(0.0, 255.0) as u8
                    } else {
                        let over = (s * a + dd * da * (1.0 - a)) / out_a;
                        let additive = ((dd * da + s * a) / out_a).min(255.0);
                        (over * (1.0 - add) + additive * add) as u8
                    }
                };
                out.put(
                    x as u32,
                    y as u32,
                    [
                        blend(r, dst[0]),
                        blend(g, dst[1]),
                        blend(b, dst[2]),
                        (out_a * 255.0) as u8,
                    ],
                );
            }
        }
    }
}

/// Blend a track layer onto the master stage frame according to `layout`:
/// - `Stack` / `Tile`: standard `blend_layer` (`additive`, `opacity`).
/// - `Split`: optical screen-blend along feathered interior panel seams and
///   sustained wash layers so shared boundaries glow like overlapping film gates
///   instead of dimming to gray.
/// - `Bleed`: luma-keyed optical screen + soft-exclusion highlight roll-off so
///   multiple overlapping video streams melt together without blowing out to white.
pub fn blend_stage_layer(
    dst: &mut Frame,
    layer: &Frame,
    layout: crate::tile::Layout,
    additive: f32,
    opacity: f32,
    is_wash_track: bool,
) {
    match layout {
        crate::tile::Layout::Stack | crate::tile::Layout::Tile => {
            blend_layer(dst, layer, additive, opacity);
        }
        crate::tile::Layout::Split => {
            let opacity = opacity.clamp(0.0, 1.0);
            if opacity <= 0.002 {
                return;
            }
            for (d, s) in dst.data.chunks_exact_mut(4).zip(layer.data.chunks_exact(4)) {
                let raw_a = s[3] as f32 / 255.0;
                let la = raw_a * opacity;
                if la <= 0.002 {
                    continue;
                }
                let dst_luma = (0.299 * d[0] as f32 + 0.587 * d[1] as f32 + 0.114 * d[2] as f32) / 255.0;
                // Where `dst` is already lit (interior feathered seam or wash bleed),
                // blend via optical screen so neither panel darkens at the seam.
                let seam_screen = if is_wash_track {
                    0.92
                } else if raw_a < 0.97 && dst_luma > 0.02 {
                    0.82
                } else {
                    additive.clamp(0.0, 1.0) * 0.35
                };
                for c in 0..3 {
                    let dn = d[c] as f32 / 255.0;
                    let sn = (s[c] as f32 / 255.0) * la;
                    let over = d[c] as f32 * (1.0 - la) + s[c] as f32 * la;
                    let screen = (1.0 - (1.0 - dn) * (1.0 - sn)) * 255.0;
                    d[c] = (over * (1.0 - seam_screen) + screen * seam_screen).clamp(0.0, 255.0) as u8;
                }
                d[3] = 255;
            }
        }
        crate::tile::Layout::Bleed => {
            let opacity = opacity.clamp(0.0, 1.0);
            if opacity <= 0.002 {
                return;
            }
            for (d, s) in dst.data.chunks_exact_mut(4).zip(layer.data.chunks_exact(4)) {
                let raw_a = s[3] as f32 / 255.0 * opacity;
                if raw_a <= 0.002 {
                    continue;
                }
                let src_val = s[0].max(s[1]).max(s[2]) as f32 / 255.0;
                let dst_val = d[0].max(d[1]).max(d[2]) as f32 / 255.0;
                // Smoothstep value key (`0.05..0.65`) so dark shadows stay translucent obsidian
                // while saturated/bright subjects project optically onto existing footage.
                let lk_t = ((src_val - 0.05) / 0.60).clamp(0.0, 1.0);
                let luma_key = 0.45 + 0.55 * (lk_t * lk_t * (3.0 - 2.0 * lk_t));
                let la = (raw_a * luma_key).clamp(0.0, 1.0);
                for c in 0..3 {
                    let dn = d[c] as f32 / 255.0;
                    let sn = (s[c] as f32 / 255.0) * la;
                    let screen = 1.0 - (1.0 - dn) * (1.0 - sn);
                    // Soft-exclusion roll-off when both layers are bright prevents flat white clip.
                    let hi = (dn * sn).max(0.0);
                    let melted = if dst_val > 0.02 {
                        (screen - 0.22 * hi * dst_val).clamp(0.0, 1.0)
                    } else {
                        dn * (1.0 - la) + (s[c] as f32 / 255.0) * la
                    };
                    d[c] = (melted * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                d[3] = 255;
            }
        }
    }
}

/// Built-in 5×7 uppercase pixel glyph rows (MSB = leftmost column of 5 bits)
/// for deterministic headless & cross-platform VJ stage captions (`captions_on`).
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        '?' => [0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000],
        '\'' => [0b00100, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        _ => [0; 7],
    }
}

/// Render a high-contrast kinetic VJ stage caption (`text`) onto `frame`.
/// `opacity` (`0..1`) fades the caption card cleanly with the active vocal grain.
pub fn draw_stage_caption(frame: &mut Frame, text: &str, opacity: f32) {
    let trimmed = text.trim();
    let op = opacity.clamp(0.0, 1.0);
    if trimmed.is_empty() || op <= 0.02 || frame.width < 16 || frame.height < 16 {
        return;
    }
    let chars: Vec<char> = trimmed.chars().take(32).collect();
    let scale = ((frame.height / 150).max(1)).min(6);
    let glyph_w = 5 * scale;
    let glyph_h = 7 * scale;
    let gap = scale;
    let total_w = chars.len() as u32 * (glyph_w + gap) - gap;
    if total_w + 4 >= frame.width {
        return;
    }
    let x0 = (frame.width - total_w) / 2;
    let y0 = ((frame.height as f32 * 0.83) as u32).min(frame.height.saturating_sub(glyph_h + 4 * scale));
    let pad_x = (2 * scale).min(x0);
    let pad_y = scale.min(y0);

    // Subtle smoked obsidian backing plate for legibility over bright footage.
    let bx0 = x0.saturating_sub(pad_x);
    let bx1 = (x0 + total_w + pad_x).min(frame.width);
    let by0 = y0.saturating_sub(pad_y);
    let by1 = (y0 + glyph_h + pad_y).min(frame.height);
    let plate_a = 0.58 * op;
    for y in by0..by1 {
        for x in bx0..bx1 {
            let mut px = frame.get(x, y);
            px[0] = (px[0] as f32 * (1.0 - plate_a) + 10.0 * plate_a) as u8;
            px[1] = (px[1] as f32 * (1.0 - plate_a) + 10.0 * plate_a) as u8;
            px[2] = (px[2] as f32 * (1.0 - plate_a) + 12.0 * plate_a) as u8;
            frame.put(x, y, px);
        }
    }

    // Crisp warm-cream studio lettering (`#F4F0E8`) with 95% max blend.
    let fg_a = 0.95 * op;
    for (ci, &ch) in chars.iter().enumerate() {
        let rows = glyph_5x7(ch);
        let gx0 = x0 + ci as u32 * (glyph_w + gap);
        for (ry, &bits) in rows.iter().enumerate() {
            if bits == 0 {
                continue;
            }
            for rx in 0..5u32 {
                if (bits >> (4 - rx)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px_x = gx0 + rx * scale + sx;
                            let px_y = y0 + ry as u32 * scale + sy;
                            if px_x < frame.width && px_y < frame.height {
                                let mut px = frame.get(px_x, px_y);
                                px[0] = (px[0] as f32 * (1.0 - fg_a) + 244.0 * fg_a) as u8;
                                px[1] = (px[1] as f32 * (1.0 - fg_a) + 240.0 * fg_a) as u8;
                                px[2] = (px[2] as f32 * (1.0 - fg_a) + 232.0 * fg_a) as u8;
                                frame.put(px_x, px_y, px);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Blend a (possibly effect-processed) layer onto an opaque master frame.
/// `additive` mixes between alpha-over and additive glow; `opacity` scales
/// the whole layer (track level -> opacity correspondence).
pub fn blend_layer(dst: &mut Frame, layer: &Frame, additive: f32, opacity: f32) {
    debug_assert_eq!(dst.width, layer.width);
    debug_assert_eq!(dst.height, layer.height);
    let add = additive.clamp(0.0, 1.0);
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.002 {
        return;
    }
    for (d, s) in dst.data.chunks_exact_mut(4).zip(layer.data.chunks_exact(4)) {
        let la = s[3] as f32 / 255.0 * opacity;
        if la <= 0.002 {
            continue;
        }
        for c in 0..3 {
            let over = d[c] as f32 * (1.0 - la) + s[c] as f32 * la;
            let additive_v = (d[c] as f32 + s[c] as f32 * la).min(255.0);
            d[c] = (over * (1.0 - add) + additive_v * add) as u8;
        }
        d[3] = 255;
    }
}

/// Blend one transparent layer OVER another transparent layer (straight
/// alpha), used to stack clip layers into a track layer.
pub fn blend_layer_over(dst: &mut Frame, src: &Frame, additive: f32) {
    debug_assert_eq!(dst.width, src.width);
    debug_assert_eq!(dst.height, src.height);
    let add = additive.clamp(0.0, 1.0);
    for (d, s) in dst.data.chunks_exact_mut(4).zip(src.data.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0;
        if sa <= 0.002 {
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let out_a = (sa + da * (1.0 - sa)).max(1e-6);
        for c in 0..3 {
            let over = (s[c] as f32 * sa + d[c] as f32 * da * (1.0 - sa)) / out_a;
            let additive_v = ((d[c] as f32 * da + s[c] as f32 * sa) / out_a).min(255.0);
            d[c] = (over * (1.0 - add) + additive_v * add) as u8;
        }
        d[3] = (out_a * 255.0) as u8;
    }
}

/// Visual counterpart of [`crate::audio::TapeSettings`], making `track_tape`
/// and `master_tape` genuine chromagrain AV processors:
/// - `wow`: subtle sub-Hz analog film-gate vertical weave
/// - `warmth` / `age`: warm amber highlight halation + lifted celluloid shadows
/// - `dust`: sparse deterministic film dust specks & faint celluloid hairlines
pub fn apply_video_tape(
    frame: &mut Frame,
    cfg: &crate::audio::TapeSettings,
    frame_idx: usize,
    fps: f32,
    seed: u64,
) {
    if !cfg.is_active() || frame.width == 0 || frame.height == 0 {
        return;
    }
    let wow = cfg.wow.clamp(0.0, 1.0);
    let warmth = cfg.warmth.clamp(0.0, 1.0);
    let age = cfg.age.clamp(0.0, 1.0);
    let dust = cfg.dust.clamp(0.0, 1.0);
    let t = frame_idx as f64 / fps.max(1.0) as f64;
    let (w, h) = (frame.width as usize, frame.height as usize);

    // 1. Subtle film-gate vertical weave synced to the audio wow frequencies.
    if wow > 0.02 {
        let weave = (0.7 * (2.0 * std::f64::consts::PI * 0.51 * t).sin()
            + 0.3 * (2.0 * std::f64::consts::PI * 0.19 * t).sin()) as f32
            * wow
            * (h as f32 * 0.006).max(1.0);
        let dy = weave.round() as isize;
        if dy != 0 {
            let orig = frame.data.clone();
            let row_bytes = w * 4;
            for y in 0..h {
                let sy = (y as isize - dy).clamp(0, h as isize - 1) as usize;
                frame.data[y * row_bytes..(y + 1) * row_bytes]
                    .copy_from_slice(&orig[sy * row_bytes..(sy + 1) * row_bytes]);
            }
        }
    }

    // 2. Warm tungsten/amber film tone (`warmth`) + lifted celluloid blacks (`age`).
    if warmth > 0.01 || age > 0.01 {
        let lift = age * 7.5;
        let r_gain = 1.0 + warmth * 0.085;
        let g_gain = 1.0 + warmth * 0.02;
        let b_gain = 1.0 - warmth * 0.075 - age * 0.04;
        for px in frame.data.chunks_exact_mut(4) {
            if px[3] == 0 {
                continue;
            }
            let a_frac = px[3] as f32 / 255.0;
            let lf = lift * a_frac;
            px[0] = (px[0] as f32 * r_gain + lf).clamp(0.0, 255.0) as u8;
            px[1] = (px[1] as f32 * g_gain + lf * 0.85).clamp(0.0, 255.0) as u8;
            px[2] = (px[2] as f32 * b_gain + lf * 0.70).clamp(0.0, 255.0) as u8;
        }
    }

    // 3. Sparse celluloid dust specks & occasional faint vertical hairline.
    if dust > 0.01 {
        let mut rng = crate::dsp::Rng::new(seed ^ (frame_idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let max_specks = ((dust * 5.0).ceil() as usize).min(6);
        for _ in 0..max_specks {
            if rng.next_f32() < dust * 0.55 {
                let cx = (rng.next_u64() as usize) % w;
                let cy = (rng.next_u64() as usize) % h;
                let rad = if rng.next_f32() < 0.25 { 1isize } else { 0isize };
                let bright = (155.0 + 75.0 * rng.next_f32()) as f32;
                let blend = (0.18 + 0.28 * dust).min(0.5);
                for oy in -rad..=rad {
                    for ox in -rad..=rad {
                        let x = (cx as isize + ox).clamp(0, w as isize - 1) as usize;
                        let y = (cy as isize + oy).clamp(0, h as isize - 1) as usize;
                        let idx = (y * w + x) * 4;
                        if frame.data[idx + 3] > 16 {
                            frame.data[idx] = (frame.data[idx] as f32 * (1.0 - blend) + bright * blend) as u8;
                            frame.data[idx + 1] = (frame.data[idx + 1] as f32 * (1.0 - blend) + bright * 0.94 * blend) as u8;
                            frame.data[idx + 2] = (frame.data[idx + 2] as f32 * (1.0 - blend) + bright * 0.82 * blend) as u8;
                        }
                    }
                }
            }
        }
        // Occasional faint vertical 16mm scratch line (~once every several frames)
        if rng.next_f32() < dust * 0.22 {
            let sx = (rng.next_u64() as usize) % w;
            let b_add = (18.0 + 24.0 * dust) as u8;
            for y in 0..h {
                let idx = (y * w + sx) * 4;
                if frame.data[idx + 3] > 32 {
                    frame.data[idx] = frame.data[idx].saturating_add(b_add);
                    frame.data[idx + 1] = frame.data[idx + 1].saturating_add((b_add as u16 * 9 / 10) as u8);
                    frame.data[idx + 2] = frame.data[idx + 2].saturating_add((b_add as u16 * 7 / 10) as u8);
                }
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

    fn render_layer_at(
        clip: &VideoClip,
        events: &[crate::grain::GrainEvent],
        style: &VisualGrainStyle,
        link: &AvLink,
        cf: Option<&ColorFilter>,
        t: f64,
    ) -> Frame {
        let mut layer = Frame::transparent(96, 72);
        composite_grains_frame(
            clip,
            events,
            style,
            link,
            &crate::timeline::ClipTransform::default(),
            cf,
            &mut layer,
            t,
        );
        let mut out = Frame::black(96, 72);
        blend_layer(&mut out, &layer, style.additive, 1.0);
        out
    }

    #[test]
    fn compositor_draws_active_grains() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let settings = GrainSettings { density: 25.0, ..Default::default() };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let style = VisualGrainStyle::default();
        let link = AvLink::default();

        let active = render_layer_at(&clip, &events, &style, &link, None, 0.5);
        let idle = render_layer_at(&clip, &events, &style, &link, None, 500.0);

        assert!(active.mean_luma() > 1.0, "grains should light up the frame");
        assert!(idle.mean_luma() < 0.5, "no grains active -> black frame");
    }

    #[test]
    fn compositor_respects_source_alpha() {
        // A source whose left half is transparent (as a segmented/masked
        // source would be) must only composite its right half.
        let mut clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let frames: Vec<Frame> = clip
            .frames
            .iter()
            .map(|f| {
                let mut nf = f.clone();
                for y in 0..nf.height {
                    for x in 0..nf.width {
                        let i = ((y * nf.width + x) * 4 + 3) as usize;
                        nf.data[i] = if x < nf.width / 2 { 0 } else { 255 };
                    }
                }
                nf
            })
            .collect();
        clip.frames = frames;

        // One full-frame, full-opacity grain so output geometry maps 1:1
        // onto source pixels.
        let settings = GrainSettings {
            density: 2.0,
            duration: 1.0,
            envelope: 1.0,
            ..Default::default()
        };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let style = VisualGrainStyle::full_frame(0.0);
        let link = AvLink { envelope_to_opacity: 0.0, pitch_to_hue: 0.0, ..Default::default() };

        let out = render_layer_at(&clip, &events, &style, &link, None, 0.25);
        let half_luma = |f: &Frame, left: bool| {
            let mut sum = 0.0f32;
            let mut n = 0u32;
            for y in 0..f.height {
                for x in 0..f.width {
                    let in_left = x < f.width / 2;
                    if in_left == left {
                        let i = ((y * f.width + x) * 4) as usize;
                        sum += f.data[i] as f32;
                        n += 1;
                    }
                }
            }
            sum / n.max(1) as f32
        };
        let left = half_luma(&out, true);
        let right = half_luma(&out, false);
        assert!(right > 5.0, "opaque half should draw (luma {right})");
        assert!(
            left < right * 0.1,
            "transparent half must stay dark: left {left} right {right}"
        );
    }

    #[test]
    fn compositor_respects_color_filter() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let settings = GrainSettings { density: 40.0, ..Default::default() };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let style = VisualGrainStyle::default();
        let link = AvLink { pitch_to_hue: 0.0, ..Default::default() };

        let unfiltered = render_layer_at(&clip, &events, &style, &link, None, 0.4);

        // A keep-band that matches nothing (test pattern at t<9s has hue<360
        // sweeping slowly; pick a band far from early hues but sat_min high).
        let cf = ColorFilter {
            sat_min: 0.99,
            ..ColorFilter::keep(200.0, 2.0)
        };
        let filtered = render_layer_at(&clip, &events, &style, &link, Some(&cf), 0.4);

        assert!(filtered.mean_luma() < unfiltered.mean_luma() * 0.2 + 1.0);
    }

    #[test]
    fn gain_to_opacity_link_is_configurable() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        // One quiet grain covering the eval time.
        let ev = crate::grain::GrainEvent {
            onset: 0.0,
            source_pos: 0.5,
            duration: 1.0,
            pitch_ratio: 1.0,
            gain: 0.25,
            pan: 0.0,
            envelope: 0.2,
            env_skew: 0.0,
            reverse: false,
            id: 0,
            ..Default::default()
        };
        let style = VisualGrainStyle { additive: 0.0, ..Default::default() };

        // Linked (default): low gain -> dim visual grain.
        let linked = render_layer_at(&clip, &[ev.clone()], &style, &AvLink::default(), None, 0.5);
        // Unlinked: opacity ignores gain -> brighter.
        let unlinked_link = AvLink { gain_to_opacity: 0.0, ..Default::default() };
        let unlinked = render_layer_at(&clip, &[ev], &style, &unlinked_link, None, 0.5);

        assert!(
            unlinked.mean_luma() > linked.mean_luma() * 2.0,
            "unlinked {} vs linked {}",
            unlinked.mean_luma(),
            linked.mean_luma()
        );
    }

    #[test]
    fn pitch_to_hue_link_is_configurable() {
        let clip = VideoClip::test_pattern(64, 48, 12.0, 2.0);
        let ev = crate::grain::GrainEvent {
            onset: 0.0,
            source_pos: 0.2,
            duration: 1.0,
            pitch_ratio: 2.0, // +12 semitones
            gain: 1.0,
            pan: 0.0,
            envelope: 0.2,
            env_skew: 0.0,
            reverse: false,
            id: 0,
            ..Default::default()
        };
        let style = VisualGrainStyle { additive: 0.0, ..Default::default() };
        let shifted = render_layer_at(
            &clip,
            &[ev.clone()],
            &style,
            &AvLink { pitch_to_hue: 15.0, pitch_to_rate: 0.0, ..Default::default() },
            None,
            0.5,
        );
        let unshifted = render_layer_at(
            &clip,
            &[ev],
            &style,
            &AvLink { pitch_to_hue: 0.0, pitch_to_rate: 0.0, ..Default::default() },
            None,
            0.5,
        );
        assert_ne!(shifted.data, unshifted.data, "hue link should change colors");
    }

    #[test]
    fn link_param_setter() {
        let mut link = AvLink::default();
        link.set_param("gain_to_opacity", 0.5).unwrap();
        link.set_param("pitch_to_hue", -20.0).unwrap();
        link.set_param("reverse_video", 0.0).unwrap();
        assert_eq!(link.gain_to_opacity, 0.5);
        assert_eq!(link.pitch_to_hue, -20.0);
        assert!(!link.reverse_video);
        assert!(link.set_param("bogus", 1.0).is_err());
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

    #[test]
    fn additive_glow_never_darkens_background() {
        let mut bg = Frame::black(16, 16);
        for px in bg.data.chunks_exact_mut(4) {
            px[0] = 120;
            px[1] = 120;
            px[2] = 120;
        }
        let mut src_f = Frame::black(16, 16);
        for px in src_f.data.chunks_exact_mut(4) {
            px[0] = 80;
            px[1] = 80;
            px[2] = 80;
        }
        let clip = VideoClip { frames: vec![src_f; 4], fps: 10.0 };
        let ev = crate::grain::GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 1.0,
            pitch_ratio: 1.0,
            gain: 0.35,
            pan: 0.0,
            envelope: 0.1,
            env_skew: 0.0,
            reverse: false,
            id: 0,
            ..Default::default()
        };
        let style = VisualGrainStyle { additive: 1.0, min_size: 1.0, max_size: 1.0, flicker: 1.0, ..Default::default() };
        let mut clip_layer = Frame::transparent(16, 16);
        composite_grains_frame(&clip, &[ev], &style, &AvLink::default(), &crate::timeline::ClipTransform::default(), None, &mut clip_layer, 0.5);
        let mut track_layer = Frame::transparent(16, 16);
        blend_layer_over(&mut track_layer, &clip_layer, 1.0);
        blend_layer(&mut bg, &track_layer, 1.0, 1.0);
        assert!(bg.mean_luma() > 135.0, "additive glow must brighten bg (120 -> {})", bg.mean_luma());
    }

    #[test]
    fn flicker_dial_smooths_percussive_strobe() {
        let ev = crate::grain::GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 0.05,
            pitch_ratio: 1.0,
            gain: 1.0,
            pan: 0.0,
            envelope: 0.1,
            env_skew: -0.95, // hard percussive audio transient
            reverse: false,
            id: 0,
            ..Default::default()
        };
        let tform = crate::timeline::ClipTransform::default();
        let link = AvLink::default();
        let raw_style = VisualGrainStyle { flicker: 1.0, ..Default::default() };
        let calm_style = VisualGrainStyle { flicker: 0.2, ..Default::default() };

        // At t = 2 ms, raw percussive envelope slams near 1.0 while calm
        // bloom ramps in gently; at t = 100 ms (after the 50 ms audio grain),
        // raw is gone while calm still glows softly.
        let raw_t0 = grain_draws(&[ev.clone()], &raw_style, &link, &tform, 1.0, 0.002, 1.0);
        let calm_t0 = grain_draws(&[ev.clone()], &calm_style, &link, &tform, 1.0, 0.002, 1.0);
        assert!(raw_t0[0].alpha > 0.7);
        assert!(calm_t0.is_empty() || calm_t0[0].alpha < 0.25);

        let raw_tail = grain_draws(&[ev.clone()], &raw_style, &link, &tform, 1.0, 0.10, 1.0);
        let calm_tail = grain_draws(&[ev], &calm_style, &link, &tform, 1.0, 0.10, 1.0);
        assert!(raw_tail.is_empty());
        assert!(!calm_tail.is_empty() && calm_tail[0].alpha > 0.05);
    }

    #[test]
    fn frame_blended_at_interpolates_frames() {
        let mut f0 = Frame::black(4, 4);
        let mut f1 = Frame::black(4, 4);
        for px in f0.data.chunks_exact_mut(4) { px[0] = 0; }
        for px in f1.data.chunks_exact_mut(4) { px[0] = 200; }
        let vc = VideoClip { frames: vec![f0, f1], fps: 10.0 };
        let mid = vc.frame_blended_at(0.05).unwrap();
        assert!((mid.get(0, 0)[0] as i32 - 100).abs() <= 2);
    }

    #[test]
    fn video_loop_crossfade_wraps_without_jump_or_alpha_dip() {
        // 1.0s clip (10 frames @ 10 fps) ramping luma from 20 -> 200.
        let mut frames = Vec::new();
        for i in 0..10 {
            let mut f = Frame::black(8, 8);
            let v = (20 + i * 20) as u8;
            for px in f.data.chunks_exact_mut(4) {
                px[0] = v;
                px[1] = v;
                px[2] = v;
                px[3] = 255;
            }
            frames.push(f);
        }
        let vc = VideoClip { frames, fps: 10.0 };
        let mut prev_luma = vc.frame_looped_at(0.0, 0.30).unwrap().mean_luma();
        let mut max_step = 0.0f32;
        for step in 1..=30 {
            let t = step as f64 * 0.1;
            let f = vc.frame_looped_at(t, 0.30).unwrap();
            assert_eq!(f.get(0, 0)[3], 255, "opaque loop crossfade keeps alpha=255");
            let l = f.mean_luma();
            max_step = max_step.max((l - prev_luma).abs());
            prev_luma = l;
        }
        // Without crossfade, wrap jump is 180 -> 20 = 160 luma in 1 frame;
        // with 0.30s cosine loop crossfade, max 1-frame step is <= 65.
        assert!(max_step < 70.0, "loop crossfade smoothed wrap jump: max_step={max_step}");
    }

    #[test]
    fn alpha_bounds_frames_entire_segmented_cutout() {
        let mut f = Frame::transparent(32, 32);
        for y in 8..24 {
            for x in 12..20 {
                f.put(x, y, [180, 160, 140, 255]);
            }
        }
        let b = f.alpha_bounds().expect("segmented cutout detected");
        assert!(b[0] <= 12.0 / 32.0 && b[0] + b[2] >= 20.0 / 32.0);
        assert!(b[1] <= 8.0 / 32.0 && b[1] + b[3] >= 24.0 / 32.0);
    }

    #[test]
    fn picture_follows_the_envelope_and_tiles_fill_their_rects() {
        let adsr = crate::envelope::Adsr { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.2, ..Default::default() };
        let env = crate::envelope::VoiceEnv { adsr, on: 0.0, off: 0.5 };
        let ev = |voice: u64, tile: Option<u8>| crate::grain::GrainEvent {
            onset: 0.0,
            source_pos: 0.5,
            duration: 2.0,
            pitch_ratio: 1.0,
            gain: 1.0,
            envelope: 0.01,
            id: voice,
            tag: crate::grain::VoiceTag { voice, env: Some(env), tile, ..Default::default() },
            ..Default::default()
        };
        let (style, link, tr) = (VisualGrainStyle::default(), AvLink::default(), crate::timeline::ClipTransform::default());
        let aspect = 16.0 / 9.0;
        // Stack: alpha *= picture weight (Cut: full, then a hard cut).
        let stack = |t: f64| grain_draws_layout(&[ev(1, None)], &style, &link, &tr, 4.0, t, aspect, false, None);
        let held = stack(0.3);
        assert!(!held.is_empty() && held.iter().all(|d| d.alpha > 0.0 && !d.tiled));
        assert!(stack(0.5 + 0.195).iter().all(|d| d.alpha == 0.0), "level < 0.05: cut");
        // Mid-release (level 0.5): Cut keeps full weight, Fade halves it.
        let a_cut = stack(0.6).iter().map(|d| d.alpha).fold(0.0f32, f32::max);
        let mut faded = ev(1, None);
        faded.tag.env.as_mut().unwrap().adsr.picture = crate::envelope::PictureCut::Fade;
        let a_fade = grain_draws_layout(&[faded], &style, &link, &tr, 4.0, 0.6, aspect, false, None).iter().map(|d| d.alpha).fold(0.0f32, f32::max);
        assert!(a_cut > 0.0 && (a_fade / a_cut - 0.5).abs() < 0.01, "cut {a_cut} vs fade {a_fade}");

        // Tiles: dest = tile_rect, full alpha, no rotation; nothing after
        // the release.
        let evs = [ev(1, Some(0)), ev(2, Some(1))];
        let draws = grain_draws_layout(&evs, &style, &link, &tr, 4.0, 0.3, aspect, false, Some((2, 1)));
        assert_eq!(draws.len(), 2);
        for (k, d) in draws.iter().enumerate() {
            let r = crate::tile::tile_rect(k, 2, 1, aspect);
            assert!(d.tiled);
            assert_eq!(d.dest, [r.x, r.y, r.w, r.h]);
            assert_eq!(d.alpha, 1.0);
            assert_eq!(d.rotation, 0.0);
            // Centered cover crop: a half-width tile shows the middle of the source.
            assert!((d.patch[0] + d.patch[2] * 0.5 - 0.5).abs() < 1e-6 && (d.patch[1] + d.patch[3] * 0.5 - 0.5).abs() < 1e-6);
        }
        let gone = grain_draws_layout(&evs, &style, &link, &tr, 4.0, 0.8, aspect, false, Some((2, 1)));
        assert!(gone.iter().all(|d| d.alpha == 0.0));

        // And the composited frame: left tile lit, right tile lit, gap dark.
        let clip = VideoClip::test_pattern(64, 36, 12.0, 4.0);
        let mut out = Frame::black(64, 36);
        composite_grains_frame_layout(&clip, &evs, &style, &link, &tr, None, &mut out, 0.3, false, None, Some((2, 1)));
        let col = |x: u32| (0..36u32).map(|y| out.data[((y * 64 + x) * 4) as usize] as u32 + out.data[((y * 64 + x) * 4 + 1) as usize] as u32).sum::<u32>();
        assert!(col(10) > 0 && col(54) > 0);
    }
}
