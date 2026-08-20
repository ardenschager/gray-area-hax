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
}

impl Default for VisualGrainStyle {
    fn default() -> Self {
        VisualGrainStyle {
            size_scale: 2.2,
            min_size: 0.12,
            max_size: 0.65,
            additive: 0.35,
            scatter_y: 0.7,
        }
    }
}

impl VisualGrainStyle {
    /// Style for snippet clips: the patch is the whole frame.
    pub fn full_frame(additive: f32) -> VisualGrainStyle {
        VisualGrainStyle {
            size_scale: 1000.0,
            min_size: 1.0,
            max_size: 1.0,
            additive,
            scatter_y: 0.0,
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
            pitch_to_hue: 12.0,
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
    let mut out = Vec::new();
    let rot = transform.rotation.to_radians();
    let (sin_r, cos_r) = rot.sin_cos();
    let tscale = transform.scale.max(0.01);
    let aspect = aspect.max(0.01);
    for ev in events {
        if t < ev.onset || t >= ev.end() {
            continue;
        }
        let phase = ((t - ev.onset) / ev.duration as f64) as f32;
        let env_part = 1.0
            + (grain_env_skewed(phase, ev.envelope, ev.env_skew) - 1.0)
                * link.envelope_to_opacity;
        let gain_part = 1.0 + (ev.gain.min(1.5) - 1.0) * link.gain_to_opacity;
        let alpha = (env_part * gain_part).clamp(0.0, 1.0);
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
        let size = size * tscale;

        let src_frac = if source_duration > 0.0 {
            (ev.source_pos / source_duration).fract() as f32
        } else {
            0.0
        };
        let patch_size = size.clamp(0.05, 1.0);
        let sx0 = (src_frac * (1.0 - patch_size)).clamp(0.0, (1.0 - patch_size).max(0.0));
        let sy0 = ((0.5 + jy * 0.5) * (1.0 - patch_size))
            .clamp(0.0, (1.0 - patch_size).max(0.0));

        out.push(GrainDraw {
            dest: [cx - size / 2.0, cy - size / 2.0, size, size],
            rotation: rot,
            patch: [sx0, sy0, patch_size, patch_size],
            src_time,
            alpha,
            hue_shift: ev.semitones() * link.pitch_to_hue,
        });
    }
    out
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
    let ow = out.width as f32;
    let oh = out.height as f32;
    let aspect = ow / oh.max(1.0);
    let add = style.additive.clamp(0.0, 1.0);
    let draws = grain_draws(events, style, link, transform, clip.duration(), t, aspect);

    for d in draws {
        let Some(src) = clip.frame_at(d.src_time) else { continue };
        let alpha = d.alpha;
        let hue_shift = d.hue_shift;

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
                let sx = ((d.patch[0] + qx * d.patch[2]) * (sw - 1.0)) as u32;
                let sy = ((d.patch[1] + qy * d.patch[3]) * (sh - 1.0)) as u32;
                let [mut r, mut g, mut b, _] =
                    src.get(sx.min(src.width - 1), sy.min(src.height - 1));

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

                // Straight-alpha accumulation into the layer, with an
                // additive component for glow.
                let dst = out.get(x as u32, y as u32);
                let da = dst[3] as f32 / 255.0;
                let out_a = (a + da * (1.0 - a)).max(1e-6);
                let blend = |src_c: u8, dst_c: u8| -> u8 {
                    let s = src_c as f32;
                    let dd = dst_c as f32;
                    let over = (s * a + dd * da * (1.0 - a)) / out_a;
                    let additive = (dd * da + s * a).min(255.0);
                    (over * (1.0 - add) + additive * add) as u8
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
            let additive_v = (d[c] as f32 * da + s[c] as f32 * sa).min(255.0);
            d[c] = (over * (1.0 - add) + additive_v * add) as u8;
        }
        d[3] = (out_a * 255.0) as u8;
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
}
