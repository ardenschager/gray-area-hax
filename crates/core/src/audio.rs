//! Audio side: clips, granular rendering into a stereo buffer.

use crate::dsp::{apply_filter_chain, grain_env, FilterSpec};
use crate::grain::GrainEvent;
use serde::{Deserialize, Serialize};

/// Mono audio clip (sources are mixed down on load; grains re-spatialize).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioClip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioClip {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> AudioClip {
        AudioClip { samples, sample_rate }
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
        out
    }

    #[inline]
    fn sample_lerp(&self, idx: f64) -> f32 {
        if idx < 0.0 {
            return 0.0;
        }
        let i = idx as usize;
        if i + 1 >= self.samples.len() {
            return 0.0;
        }
        let frac = (idx - i as f64) as f32;
        self.samples[i] * (1.0 - frac) + self.samples[i + 1] * frac
    }
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
    let out_sr = out.sample_rate as f64;
    let src_sr = clip.sample_rate as f64;
    let out_len = out.len() as i64;

    for ev in events {
        let start_sample = ((ev.onset - t0) * out_sr).round() as i64;
        let n = (ev.duration as f64 * out_sr) as i64;
        if n <= 0 || start_sample + n < 0 || start_sample >= out_len {
            continue;
        }
        // Equal-power pan.
        let pan = ev.pan.clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        let (gain_l, gain_r) = (angle.cos() * ev.gain, angle.sin() * ev.gain);

        let src_start = ev.source_pos * src_sr;
        let grain_src_len = ev.duration as f64 * ev.pitch_ratio as f64 * src_sr;

        for i in 0..n {
            let oi = start_sample + i;
            if oi < 0 || oi >= out_len {
                continue;
            }
            let phase = i as f32 / n as f32;
            let env = grain_env(phase, ev.envelope);
            let src_off = if ev.reverse {
                grain_src_len - i as f64 * ev.pitch_ratio as f64
            } else {
                i as f64 * ev.pitch_ratio as f64
            };
            let s = clip.sample_lerp(src_start + src_off) * env;
            out.left[oi as usize] += s * gain_l;
            out.right[oi as usize] += s * gain_r;
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
}
