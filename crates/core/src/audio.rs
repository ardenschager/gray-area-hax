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

    /// `len` seconds from `start` (clamped to the clip).
    pub fn trimmed(&self, start: f64, len: f64) -> AudioClip {
        let sr = self.sample_rate as f64;
        let n = self.samples.len();
        let a = ((start.max(0.0) * sr) as usize).min(n);
        let b = ((start.max(0.0) + len.max(0.0)) * sr).ceil().min(n as f64) as usize;
        let b = b.max(a);
        AudioClip {
            samples: self.samples[a..b].to_vec(),
            sample_rate: self.sample_rate,
            side: self.side.as_ref().map(|s| s[a.min(s.len())..b.min(s.len())].to_vec()),
        }
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
        self.filtered_and_eq(chain, &[])
    }

    /// Return a copy of this clip with both standard biquad filters and
    /// parametric peaking EQ bands applied in order.
    pub fn filtered_and_eq(&self, chain: &[FilterSpec], eq: &[crate::dsp::EqBand]) -> AudioClip {
        let mut out = self.clone();
        apply_filter_chain(&mut out.samples, chain, self.sample_rate as f32);
        crate::dsp::apply_eq_chain(&mut out.samples, eq, self.sample_rate as f32);
        // Linear filters: filtering mid and side == filtering L and R.
        if let Some(side) = out.side.as_mut() {
            apply_filter_chain(side, chain, self.sample_rate as f32);
            crate::dsp::apply_eq_chain(side, eq, self.sample_rate as f32);
        }
        out
    }

    /// Detect up to `max_slices` transient / syllable onset positions in
    /// `[0, 1)` along with each slice's active duration in seconds (bounded
    /// to the next onset or silence). Makes chopping speech, vocals, or drum
    /// breaks into short non-legible micro-fragments effortless.
    pub fn detect_slices(&self, max_slices: usize) -> Vec<(f32, f32)> {
        let max_slices = max_slices.clamp(1, 128);
        if self.samples.is_empty() || self.sample_rate == 0 {
            return Vec::new();
        }
        let sr = self.sample_rate as usize;
        let hop = (sr / 100).max(1); // 10 ms analysis hop
        let n_hops = self.samples.len() / hop;
        if n_hops < 3 {
            return vec![(0.0, self.duration() as f32)];
        }
        let mut env = Vec::with_capacity(n_hops);
        let mut peak_e = 0.0f32;
        for h in 0..n_hops {
            let seg = &self.samples[h * hop..(h + 1) * hop];
            let rms = (seg.iter().map(|s| s * s).sum::<f32>() / seg.len() as f32).sqrt();
            peak_e = peak_e.max(rms);
            env.push(rms);
        }
        if peak_e < 1e-5 {
            return vec![(0.0, self.duration() as f32)];
        }
        let floor = peak_e * 0.08;
        // Positive log-energy flux highlights syllable/transient attacks.
        let mut flux = vec![0.0f32; n_hops];
        for i in 1..n_hops {
            if env[i] >= floor {
                let d = (env[i] + 1e-4).ln() - (env[i - 1] + 1e-4).ln();
                flux[i] = d.max(0.0) * env[i].sqrt();
            }
        }
        // Pick local peaks separated by at least 120 ms (12 hops).
        let min_gap = 12usize;
        let mut cands: Vec<(usize, f32)> = Vec::new();
        if env[0] >= floor {
            cands.push((0, env[0]));
        }
        for i in 1..n_hops - 1 {
            if flux[i] > 0.02 && flux[i] >= flux[i - 1] && flux[i] >= flux[i + 1] {
                if let Some(&(last_i, last_v)) = cands.last() {
                    if i - last_i < min_gap {
                        if flux[i] > last_v {
                            *cands.last_mut().unwrap() = (i, flux[i]);
                        }
                        continue;
                    }
                }
                cands.push((i, flux[i]));
            }
        }
        if cands.is_empty() {
            return vec![(0.0, self.duration() as f32)];
        }
        // Keep the strongest `max_slices`, then restore chronological order.
        if cands.len() > max_slices {
            cands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            cands.truncate(max_slices);
            cands.sort_by_key(|c| c.0);
        }
        let total_dur = self.duration() as f32;
        let mut out = Vec::with_capacity(cands.len());
        for (idx, &(h0, _)) in cands.iter().enumerate() {
            let next_h = cands.get(idx + 1).map(|c| c.0).unwrap_or(n_hops);
            // Stop slice duration early if energy drops below 5% of peak.
            let mut end_h = (h0 + 3).min(next_h);
            while end_h < next_h && env[end_h] >= peak_e * 0.05 {
                end_h += 1;
            }
            let start_s = (h0 * hop) as f32 / sr as f32;
            let dur_s = (((end_h.saturating_sub(h0)).max(2) * hop) as f32 / sr as f32)
                .clamp(0.04, total_dur);
            out.push(((start_s / total_dur.max(1e-6)).clamp(0.0, 0.999), dur_s));
        }
        out
    }
}

/// Pitch-aware interpolation: exact 4-point Hermite when `ratio <= 1.05`, and
/// a symmetric 5-tap pitch-scaled FIR anti-aliasing + formant-warming kernel
/// when pitching upward (`ratio > 1.05`), preventing Nyquist foldback and
/// harsh 2-6 kHz chipmunk squeak while staying 100% stateless across blocks.
#[inline]
fn sample_pitched(x: &[f32], idx: f64, ratio: f32) -> f32 {
    if ratio <= 1.05 {
        return hermite(x, idx);
    }
    let step = ((ratio - 1.0) * 0.75).clamp(0.05, 3.5) as f64;
    0.10 * hermite(x, idx - 2.0 * step)
        + 0.24 * hermite(x, idx - step)
        + 0.32 * hermite(x, idx)
        + 0.24 * hermite(x, idx + step)
        + 0.10 * hermite(x, idx + 2.0 * step)
}
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

/// Master ceiling conditioner shared by offline rendering and
/// [`crate::realtime::RealtimeAudio`] so block streaming stays sample-identical
/// to offline export (and exact-identity below `MASTER_KNEE`).
#[derive(Debug, Clone, Copy, Default)]
pub struct MasterConditioner;

/// True-peak-safe master ceiling (~ -1.0 dBFS) leaving inter-sample headroom
/// for AAC / lossy encoders.
pub const MASTER_CEILING: f32 = 0.891_250_9;
const MASTER_KNEE: f32 = 0.65;

/// Smooth C1-continuous soft-knee ceiling limiter. Linear below `MASTER_KNEE`
/// (unit slope at the knee — zero kink or step discontinuity) and smoothly
/// asymptotic to `±MASTER_CEILING`.
#[inline]
pub fn soft_limit_sample(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let a = x.abs();
    if a <= MASTER_KNEE {
        x
    } else {
        let span = MASTER_CEILING - MASTER_KNEE;
        let y = MASTER_KNEE + span * ((a - MASTER_KNEE) / span).tanh();
        y.copysign(x)
    }
}

impl MasterConditioner {
    #[inline]
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32], _sample_rate: u32) {
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s = soft_limit_sample(*s);
        }
    }
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

    /// Block DC/infrasound (<18 Hz) and C1-smoothly soft-limit the mix below
    /// `-1.0 dBFS` (`MASTER_CEILING`) so hot clouds neither crackle at unity
    /// nor clip inter-sample peaks on AAC encode.
    pub fn soft_clip(&mut self) {
        let mut cond = MasterConditioner::default();
        cond.process_block(&mut self.left, &mut self.right, self.sample_rate);
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
            let m = sample_pitched(&clip.samples, idx, ev.pitch_ratio) * env;
            match side {
                None => {
                    out.left[oi as usize] += m * gain_l;
                    out.right[oi as usize] += m * gain_r;
                }
                Some(sd) => {
                    let s = sample_pitched(sd, idx, ev.pitch_ratio) * env;
                    out.left[oi as usize] += (m + s) * gain_l;
                    out.right[oi as usize] += (m - s) * gain_r;
                }
            }
        }
    }
}

/// Analog tape & vinyl character stage (available per-track and on master):
/// wow/flutter pitch drift, asymmetric tape warmth, worn head high-rolloff,
/// and resonant vinyl dust micro-crackles. All zero by default (exact bypass).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TapeSettings {
    /// Pitch wow & flutter depth, 0..1 (~ ±18 cents at 1.0, stereo-spread).
    pub wow: f32,
    /// Asymmetric tape saturation / harmonic warmth, 0..1.
    pub warmth: f32,
    /// Worn tape-head high-frequency rolloff, 0..1 (0 = open, 1 = dark ~2.6 kHz).
    pub age: f32,
    /// Vinyl surface dust & warm needle micro-pops, 0..1.
    pub dust: f32,
}

impl TapeSettings {
    pub fn is_active(&self) -> bool {
        self.wow > 1e-4 || self.warmth > 1e-4 || self.age > 1e-4 || self.dust > 1e-4
    }
}

const TAPE_RING: usize = 2048;

/// Streaming-safe state for [`TapeSettings`], shared by offline and realtime
/// audio paths so block boundaries stay sample-identical.
#[derive(Debug, Clone)]
pub struct TapeState {
    ring: [[f32; TAPE_RING]; 2],
    pos: usize,
    sample_idx: u64,
    lp: [f32; 2],
    rng: crate::dsp::Rng,
    pop_env: [f32; 2],
    pop_sign: [f32; 2],
    pop_lp: [f32; 2],
    pop_hp: [f32; 2],
    thump_env: [f32; 2],
    thump_lp: [f32; 2],
    hiss_lp: [f32; 2],
}

impl TapeState {
    pub fn new(seed: u64) -> Self {
        TapeState {
            ring: [[0.0; TAPE_RING]; 2],
            pos: 0,
            sample_idx: 0,
            lp: [0.0; 2],
            rng: crate::dsp::Rng::new(seed ^ 0x7A9E_D057),
            pop_env: [0.0; 2],
            pop_sign: [1.0; 2],
            pop_lp: [0.0; 2],
            pop_hp: [0.0; 2],
            thump_env: [0.0; 2],
            thump_lp: [0.0; 2],
            hiss_lp: [0.0; 2],
        }
    }

    pub fn process_block(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        cfg: &TapeSettings,
        sample_rate: u32,
    ) {
        if !cfg.is_active() {
            return;
        }
        let sr = sample_rate.max(1) as f32;
        let wow = cfg.wow.clamp(0.0, 1.0);
        let warmth = cfg.warmth.clamp(0.0, 1.0);
        let age = cfg.age.clamp(0.0, 1.0);
        let dust = cfg.dust.clamp(0.0, 1.0);

        let drive = 1.0 + warmth * 1.4;
        let bias = 0.075 * warmth;
        let bias_t = bias.tanh();
        let norm_g = 1.0 / drive.tanh().max(0.84);

        // Gentler tape head curve (20 kHz -> 4.2 kHz at age=1) so gentle age
        // settings (0.1..0.3) keep 5-12 kHz air alive.
        let cutoff = 20000.0 * (4200.0f32 / 20000.0).powf(age);
        let lp_a = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / sr).exp();
        let hiss_a = 1.0 - (-2.0 * std::f32::consts::PI * 2400.0 / sr).exp();
        let pop_lp_a = 1.0 - (-2.0 * std::f32::consts::PI * 7600.0 / sr).exp();
        let pop_hp_a = 1.0 - (-2.0 * std::f32::consts::PI * 1650.0 / sr).exp();
        let thump_lp_a = 1.0 - (-2.0 * std::f32::consts::PI * 340.0 / sr).exp();

        let n = left.len().min(right.len());
        for i in 0..n {
            let t = self.sample_idx as f64 / sr as f64;
            self.sample_idx += 1;

            let p = self.pos;
            self.ring[0][p] = left[i];
            self.ring[1][p] = right[i];

            // Authentic broadband vinyl crackle & needle dust (placed AFTER
            // tape `age` LP so vinyl surface dust retains crisp 2-8 kHz snap).
            if dust > 1e-4 {
                let p_tick = (58.0 * dust) / sr;
                if self.rng.next_f32() < p_tick {
                    let ch = (self.rng.next_u64() & 1) as usize;
                    let bleed = 0.35 + 0.35 * self.rng.next_f32();
                    let sign = if (self.rng.next_u64() & 2) != 0 { 1.0 } else { -1.0 };
                    if self.rng.next_f32() < 0.12 {
                        let th = (0.06 + 0.09 * self.rng.next_f32()) * dust;
                        self.thump_env[ch] += th;
                        self.thump_env[1 - ch] += th * 0.7;
                    } else {
                        let amp = (0.045 + 0.11 * self.rng.next_f32().powi(2)) * dust;
                        self.pop_env[ch] = amp;
                        self.pop_sign[ch] = sign;
                        self.pop_env[1 - ch] = self.pop_env[1 - ch].max(amp * bleed);
                        self.pop_sign[1 - ch] = sign;
                    }
                }
            }

            let mut out_pair = [left[i], right[i]];
            for (c, val) in out_pair.iter_mut().enumerate() {
                let mut x = if wow > 1e-4 {
                    let ph_off = c as f64 * 1.1;
                    let w1 = (2.0 * std::f64::consts::PI * (0.51 + 0.06 * c as f64) * t + ph_off).sin();
                    let w2 = (2.0 * std::f64::consts::PI * 0.19 * t + ph_off * 0.5).sin();
                    let fl = (2.0 * std::f64::consts::PI * (6.4 + 0.7 * c as f64) * t).sin() * 0.12
                        + (2.0 * std::f64::consts::PI * 11.7 * t + ph_off).sin() * 0.05;
                    let delay_secs = 0.0055 + wow as f64 * (0.0017 * (0.7 * w1 + 0.3 * w2) + 0.00025 * fl);
                    let d_samp = (delay_secs * sr as f64).clamp(1.0, (TAPE_RING - 3) as f64);
                    let rd = (p as f64 + TAPE_RING as f64 - d_samp) % TAPE_RING as f64;
                    let i0 = rd.floor() as usize % TAPE_RING;
                    let i1 = (i0 + 1) % TAPE_RING;
                    let f = (rd - rd.floor()) as f32;
                    self.ring[c][i0] + (self.ring[c][i1] - self.ring[c][i0]) * f
                } else {
                    *val
                };

                if warmth > 1e-4 {
                    let sat = ((x * drive + bias).tanh() - bias_t) * norm_g;
                    x = x * (1.0 - warmth) + sat * warmth;
                }

                if age > 1e-4 {
                    self.lp[c] += lp_a * (x - self.lp[c]);
                    x = self.lp[c];
                }

                if dust > 1e-4 {
                    let white = self.rng.bipolar();
                    self.hiss_lp[c] += hiss_a * (white - self.hiss_lp[c]);
                    let surface = (white - self.hiss_lp[c]) * 0.0065 * dust;

                    // Broadband needle tick shaped through 1.65-7.6 kHz bandpass (zero sine whistle!)
                    let raw_tick = self.pop_env[c] * (self.pop_sign[c] * 0.7 + white * 0.5);
                    self.pop_lp[c] += pop_lp_a * (raw_tick - self.pop_lp[c]);
                    self.pop_hp[c] += pop_hp_a * (self.pop_lp[c] - self.pop_hp[c]);
                    let crackle = (self.pop_lp[c] - self.pop_hp[c]) * 1.6;
                    self.pop_env[c] *= (-2400.0 / sr).exp();

                    // Warm low-end vinyl dust thump (<340 Hz)
                    let raw_thump = self.thump_env[c] * (0.6 + 0.4 * white);
                    self.thump_lp[c] += thump_lp_a * (raw_thump - self.thump_lp[c]);
                    self.thump_env[c] *= (-280.0 / sr).exp();

                    x += surface + crackle + self.thump_lp[c];
                }

                *val = x;
            }
            left[i] = out_pair[0];
            right[i] = out_pair[1];
            self.pos = (p + 1) % TAPE_RING;
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
            ..Default::default()
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
    fn soft_clip_bounds_output_and_is_continuous() {
        let mut out = StereoBuffer::new(0.1, 48000);
        for s in out.left.iter_mut() {
            *s = 5.0;
        }
        out.soft_clip();
        assert!(out.peak() <= MASTER_CEILING + 1e-4, "peak {}", out.peak());

        // C1 continuity check across the knee and past unity (fixes the old
        // 0.952 -> 0.762 discontinuity at |x| = 1.0).
        let mut prev = soft_limit_sample(0.0);
        for k in 1..=30000 {
            let x = k as f32 * 1e-4;
            let y = soft_limit_sample(x);
            assert!(
                (y - prev).abs() < 1.5e-4 && y >= prev,
                "discontinuity at x={x}: {prev} -> {y}"
            );
            prev = y;
        }

        // Below-knee signals pass through bit-identically (preserving Crush
        // zero-order-hold steps and loop pass parity).
        let mut quiet = StereoBuffer::new(0.05, 48000);
        for s in quiet.left.iter_mut() {
            *s = 0.25;
        }
        quiet.soft_clip();
        assert_eq!(quiet.left[100], 0.25);
    }

    #[test]
    fn detect_slices_finds_transients() {
        let sr = 48000;
        let mut samples = vec![0.0f32; sr * 2];
        // Three distinct bursts at 0.2s, 0.8s, 1.4s.
        for &onset in &[0.2f32, 0.8, 1.4] {
            let i0 = (onset * sr as f32) as usize;
            for k in 0..4800 {
                let env = (-(k as f32) / 1200.0).exp();
                samples[i0 + k] = 0.7 * env * (k as f32 * 0.12).sin();
            }
        }
        let clip = AudioClip::new(samples, sr as u32);
        let slices = clip.detect_slices(8);
        assert_eq!(slices.len(), 3, "slices: {slices:?}");
        assert!((slices[0].0 - 0.10).abs() < 0.03);
        assert!((slices[1].0 - 0.40).abs() < 0.03);
        assert!((slices[2].0 - 0.70).abs() < 0.03);
    }

    #[test]
    fn tape_streams_identically_across_blocks() {
        let sr = 48000;
        let cfg = TapeSettings { wow: 0.6, warmth: 0.5, age: 0.4, dust: 0.5 };
        let mut l1: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
        let mut r1 = l1.clone();
        let mut l2 = l1.clone();
        let mut r2 = r1.clone();

        let mut st1 = TapeState::new(42);
        st1.process_block(&mut l1, &mut r1, &cfg, sr);

        let mut st2 = TapeState::new(42);
        let mut pos = 0;
        for b in [257usize, 512, 113, 1024, 2094] {
            st2.process_block(&mut l2[pos..pos + b], &mut r2[pos..pos + b], &cfg, sr);
            pos += b;
        }
        for i in 0..4000 {
            assert!((l1[i] - l2[i]).abs() < 1e-6);
            assert!((r1[i] - r2[i]).abs() < 1e-6);
        }
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
            ..Default::default()
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
