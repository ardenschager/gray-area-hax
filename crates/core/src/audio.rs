//! Audio side: clips, granular rendering into a stereo buffer.

use crate::dsp::{apply_filter_chain, grain_env, grain_env_skewed, FilterSpec};
use crate::grain::GrainEvent;
use serde::{Deserialize, Serialize};

/// Audio clip stored as mid (+ optional side). `samples` is always the
/// mono mid signal `(L+R)/2`, so analysis (pitch, peaks, scopes) stays mono;
/// stereo sources also carry `side = (L-R)/2`, which the grain renderer
/// uses to keep the original width (`L = mid+side`, `R = mid-side`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioClip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Vec<f32>>,
}

impl AudioClip {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> AudioClip {
        AudioClip { samples, sample_rate, side: None }
    }

    /// Build from left/right channels. Falls back to mono when the channels
    /// are (near) identical, so dual-mono files cost nothing extra.
    pub fn stereo(left: &[f32], right: &[f32], sample_rate: u32) -> AudioClip {
        let n = left.len().min(right.len());
        let mid: Vec<f32> = (0..n).map(|i| 0.5 * (left[i] + right[i])).collect();
        let side: Vec<f32> = (0..n).map(|i| 0.5 * (left[i] - right[i])).collect();
        let side_peak = side.iter().fold(0f32, |m, v| m.max(v.abs()));
        AudioClip {
            samples: mid,
            sample_rate,
            side: (side_peak > 1e-4).then_some(side),
        }
    }

    pub fn is_stereo(&self) -> bool {
        self.side.is_some()
    }

    /// Left and right channels (copies; mono clips return mid twice).
    pub fn left_right(&self) -> (Vec<f32>, Vec<f32>) {
        match &self.side {
            None => (self.samples.clone(), self.samples.clone()),
            Some(side) => self
                .samples
                .iter()
                .zip(side)
                .map(|(m, s)| (m + s, m - s))
                .unzip(),
        }
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f64 / self.sample_rate as f64
        }
    }

    /// Test/demo tone: sine with a soft attack/release.
    pub fn sine(freq: f32, secs: f32, sample_rate: u32) -> AudioClip {
        let n = (secs * sample_rate as f32) as usize;
        let samples = (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                let env = grain_env(i as f32 / n.max(1) as f32, 0.1);
                0.7 * env * (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();
        AudioClip::new(samples, sample_rate)
    }

    /// Test/demo source: a few detuned saws — richer for filtering demos.
    pub fn saw_stack(freq: f32, secs: f32, sample_rate: u32) -> AudioClip {
        let n = (secs * sample_rate as f32) as usize;
        let detunes = [1.0, 1.005, 0.995];
        let samples = (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                let mut v = 0.0;
                for d in detunes {
                    let phase = (freq * d * t).fract();
                    v += 2.0 * phase - 1.0;
                }
                let env = grain_env(i as f32 / n.max(1) as f32, 0.05);
                0.25 * env * v
            })
            .collect();
        AudioClip::new(samples, sample_rate)
    }

    /// Return a filtered copy of this clip.
    pub fn filtered(&self, chain: &[FilterSpec]) -> AudioClip {
        let mut out = self.clone();
        apply_filter_chain(&mut out.samples, chain, self.sample_rate as f32);
        // Linear filters: filtering mid and side == filtering L and R.
        if let Some(side) = out.side.as_mut() {
            apply_filter_chain(side, chain, self.sample_rate as f32);
        }
        out
    }

    /// 4-point Catmull-Rom (Hermite) interpolation — audibly cleaner than
    /// linear when grains are pitched, especially downward.
    #[inline]
    fn sample_hermite(&self, idx: f64) -> f32 {
        hermite(&self.samples, idx)
    }
}

/// 4-point Catmull-Rom (Hermite) interpolation of `x` at fractional `idx`.
#[inline]
fn hermite(x: &[f32], idx: f64) -> f32 {
    if idx < 0.0 {
        return 0.0;
    }
    let i = idx as usize;
    let n = x.len();
    if i + 1 >= n {
        return 0.0;
    }
    let frac = (idx - i as f64) as f32;
    let xm1 = if i > 0 { x[i - 1] } else { x[i] };
    let x0 = x[i];
    let x1 = x[i + 1];
    let x2 = if i + 2 < n { x[i + 2] } else { x1 };
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b = w + a;
    ((a * frac - b) * frac + c) * frac + x0
}

/// Interleaved-free stereo render target.
#[derive(Debug, Clone)]
pub struct StereoBuffer {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: u32,
}

impl StereoBuffer {
    pub fn new(len_secs: f64, sample_rate: u32) -> StereoBuffer {
        let n = (len_secs * sample_rate as f64).ceil() as usize;
        StereoBuffer {
            left: vec![0.0; n],
            right: vec![0.0; n],
            sample_rate,
        }
    }

    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn duration(&self) -> f64 {
        self.len() as f64 / self.sample_rate as f64
    }

    /// Soft-clip the mix so hot grain clouds don't wrap.
    pub fn soft_clip(&mut self) {
        for ch in [&mut self.left, &mut self.right] {
            for s in ch.iter_mut() {
                if s.abs() > 1.0 {
                    *s = s.tanh();
                } else {
                    // Gentle knee approaching 1.
                    *s = s.tanh() * 0.2 + *s * 0.8;
                }
            }
        }
    }

    pub fn peak(&self) -> f32 {
        self.left
            .iter()
            .chain(self.right.iter())
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }

    pub fn rms(&self) -> f32 {
        let n = (self.left.len() + self.right.len()).max(1);
        let sum: f32 = self
            .left
            .iter()
            .chain(self.right.iter())
            .map(|s| s * s)
            .sum();
        (sum / n as f32).sqrt()
    }

    pub fn interleaved(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.len() * 2);
        for i in 0..self.len() {
            out.push(self.left[i]);
            out.push(self.right[i]);
        }
        out
    }
}

/// Render a grain cloud from `clip` into `out`, offset so that timeline
/// second `t0` is sample 0 of the output buffer.
pub fn render_grains_audio(
    clip: &AudioClip,
    events: &[GrainEvent],
    out: &mut StereoBuffer,
    t0: f64,
) {
    render_grains_audio_spatial(clip, events, out, t0, 0.0, 1.0);
}

/// Like [`render_grains_audio`], with a clip-level spatial placement:
/// `pan_offset` shifts every grain's pan (the canvas-x -> pan link) and
/// `gain_mult` scales every grain (the scale -> distance-loudness link).
pub fn render_grains_audio_spatial(
    clip: &AudioClip,
    events: &[GrainEvent],
    out: &mut StereoBuffer,
    t0: f64,
    pan_offset: f32,
    gain_mult: f32,
) {
    let out_sr = out.sample_rate as f64;
    let src_sr = clip.sample_rate as f64;
    let out_len = out.len() as i64;
    let side = clip.side.as_deref();

    for ev in events {
        // Rounded independently so block-wise rendering at t0 = k/sr is
        // sample-exact against a single full render at t0 = 0.
        let start_sample = (ev.onset * out_sr).round() as i64 - (t0 * out_sr).round() as i64;
        let n = (ev.duration as f64 * out_sr) as i64;
        if n <= 0 || start_sample + n < 0 || start_sample >= out_len {
            continue;
        }
        // Equal-power pan (grain pan + clip placement offset).
        let pan = (ev.pan + pan_offset).clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        let g = ev.gain * gain_mult.max(0.0);
        let (gain_l, gain_r) = (angle.cos() * g, angle.sin() * g);

        let src_start = ev.source_pos * src_sr;
        let grain_src_len = ev.duration as f64 * ev.pitch_ratio as f64 * src_sr;

        for i in 0..n {
            let oi = start_sample + i;
            if oi < 0 || oi >= out_len {
                continue;
            }
            let phase = i as f32 / n as f32;
            let env = grain_env_skewed(phase, ev.envelope, ev.env_skew);
            let src_off = if ev.reverse {
                grain_src_len - i as f64 * ev.pitch_ratio as f64
            } else {
                i as f64 * ev.pitch_ratio as f64
            };
            let idx = src_start + src_off;
            let m = clip.sample_hermite(idx) * env;
            match side {
                None => {
                    out.left[oi as usize] += m * gain_l;
                    out.right[oi as usize] += m * gain_r;
                }
                Some(sd) => {
                    let s = hermite(sd, idx) * env;
                    out.left[oi as usize] += (m + s) * gain_l;
                    out.right[oi as usize] += (m - s) * gain_r;
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
    fn granular_render_produces_signal() {
        let clip = AudioClip::sine(440.0, 2.0, 48000);
        let settings = GrainSettings { density: 30.0, ..Default::default() };
        let events =
            schedule_grains(&settings, 0.0, 1.0, clip.duration(), 440.0, None);
        let mut out = StereoBuffer::new(1.5, 48000);
        render_grains_audio(&clip, &events, &mut out, 0.0);
        assert!(out.rms() > 0.01, "render should be audible, rms={}", out.rms());
        assert!(out.peak().is_finite());
    }

    #[test]
    fn empty_events_render_silence() {
        let clip = AudioClip::sine(440.0, 1.0, 48000);
        let mut out = StereoBuffer::new(1.0, 48000);
        render_grains_audio(&clip, &[], &mut out, 0.0);
        assert_eq!(out.rms(), 0.0);
    }

    #[test]
    fn pitch_ratio_transposes() {
        // Render a single long grain at ratio 2.0 from a 220 Hz source;
        // detected output pitch should be ~440.
        let sr = 48000;
        let clip = AudioClip::sine(220.0, 3.0, sr);
        let ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.5,
            duration: 1.0,
            pitch_ratio: 2.0,
            gain: 1.0,
            pan: 0.0,
            envelope: 0.2,
            env_skew: 0.0,
            reverse: false,
            id: 0,
        };
        let mut out = StereoBuffer::new(1.0, sr);
        render_grains_audio(&clip, &[ev], &mut out, 0.0);
        let mono: Vec<f32> = out
            .left
            .iter()
            .zip(&out.right)
            .map(|(l, r)| l + r)
            .collect();
        let pitch = crate::dsp::detect_pitch(&mono, sr).expect("pitch");
        assert!((pitch - 440.0).abs() < 15.0, "got {pitch}");
    }

    #[test]
    fn soft_clip_bounds_output() {
        let mut out = StereoBuffer::new(0.1, 48000);
        for s in out.left.iter_mut() {
            *s = 5.0;
        }
        out.soft_clip();
        assert!(out.peak() <= 1.0001);
    }

    #[test]
    fn stereo_source_keeps_its_width() {
        let sr = 48000;
        let n = sr as usize;
        let left: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let right = vec![0.0f32; n];
        let clip = AudioClip::stereo(&left, &right, sr);
        assert!(clip.is_stereo());
        let (l, r) = clip.left_right();
        assert!((l[100] - left[100]).abs() < 1e-6 && r[100].abs() < 1e-6);
        let ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.1,
            duration: 0.5,
            pitch_ratio: 1.0,
            gain: 1.0,
            pan: 0.0,
            envelope: 0.2,
            env_skew: 0.0,
            reverse: false,
            id: 0,
        };
        let mut out = StereoBuffer::new(0.6, sr);
        render_grains_audio(&clip, &[ev], &mut out, 0.0);
        let e = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        assert!(e(&out.left) > 0.1, "left carries the signal");
        assert!(e(&out.right) < 1e-6 * e(&out.left), "right stays silent");
        // Filtering keeps channels apart too.
        let f = clip.filtered(&[]);
        assert!(f.is_stereo());
    }

    #[test]
    fn dual_mono_collapses_to_mono() {
        let a: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        let clip = AudioClip::stereo(&a, &a, 48000);
        assert!(!clip.is_stereo());
        assert_eq!(clip.samples.len(), 1000);
    }
}
