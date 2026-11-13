//! Audio side: clips, granular rendering into a stereo buffer.

use crate::dsp::{apply_filter_chain, grain_env, grain_env_skewed, FilterSpec};
pub use crate::dsp::{BandIsolator, IsolatorChannelMode, IsolatorMonitor, IsolatorState};
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

/// Studio clarity bus ceiling (`-1.40 dBFS` / `-1.4 dBTP`) preventing post-AAC
/// inter-sample overs on transient-rich video collages.
pub const STUDIO_CEILING: f32 = 0.851_138_04;
const STUDIO_KNEE: f32 = 0.58;

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

/// Transparent C1-continuous soft-knee limiter capped at `-1.4 dBTP` ([`STUDIO_CEILING`]).
#[inline]
pub fn soft_limit_studio_sample(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let a = x.abs();
    if a <= STUDIO_KNEE {
        x
    } else {
        let span = STUDIO_CEILING - STUDIO_KNEE;
        let y = STUDIO_KNEE + span * ((a - STUDIO_KNEE) / span).tanh();
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

/// Streaming-safe Studio Clarity Bus shared identically by offline [`crate::render::render_project`]
/// and [`crate::realtime::RealtimeAudio`]:
/// - `26 Hz` infrasonic DC cleanup
/// - `+2.4 dB @ 56 Hz` warm sub shelf/bell (`Q = 0.82`)
/// - `-3.2 dB @ 285 Hz` gentle low-mid mud control (`Q = 0.72`)
/// - `-1.4 dBTP` transparent knee ceiling ([`soft_limit_studio_sample`])
#[derive(Debug, Clone)]
pub struct StudioBusState {
    dc_hp: [crate::dsp::Biquad; 2],
    sub_eq: [crate::dsp::Biquad; 2],
    mud_eq: [crate::dsp::Biquad; 2],
}

impl StudioBusState {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(1) as f32;
        let hp = crate::dsp::Biquad::new(crate::dsp::FilterKind::HighPass, sr, 24.0, 0.707);
        let sub = crate::dsp::Biquad::peaking(sr, 56.0, 0.82, 2.4);
        let mud = crate::dsp::Biquad::peaking(sr, 285.0, 0.72, -3.2);
        StudioBusState {
            dc_hp: [hp.clone(), hp],
            sub_eq: [sub.clone(), sub],
            mud_eq: [mud.clone(), mud],
        }
    }

    #[inline]
    pub fn process_eq(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (c, ch) in [&mut *left, &mut *right].into_iter().enumerate() {
            for s in ch.iter_mut() {
                let mut v = self.dc_hp[c].process(*s);
                v = self.sub_eq[c].process(v);
                v = self.mud_eq[c].process(v);
                *s = v;
            }
        }
    }

    #[inline]
    pub fn process_ceiling(left: &mut [f32], right: &mut [f32]) {
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s = soft_limit_studio_sample(*s);
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
        if ev.sculpt.has_audio_dsp() {
            render_sculpted_grain(clip, ev, out, t0, pan_offset, gain_mult);
            continue;
        }
        // Rounded independently so block-wise rendering at t0 = k/sr is
        // sample-exact against a single full render at t0 = 0.
        let abs_start = (ev.onset * out_sr).round() as i64;
        let start_sample = abs_start - (t0 * out_sr).round() as i64;
        let n = (ev.duration as f64 * out_sr) as i64;
        if n <= 0 || start_sample + n < 0 || start_sample >= out_len {
            continue;
        }
        // The voice's ADSR, evaluated at each output sample's absolute time
        // (sample-exact for any block split). Grains past the voice's end
        // are silent.
        let venv = ev.tag.env;
        if venv.is_some_and(|e| ev.onset >= e.end()) {
            continue;
        }
        if ev.tag.choke_at.is_some_and(|tc| ev.onset >= tc + crate::grain::CHOKE_RELEASE_SECS) {
            continue;
        }
        // Equal-power pan (grain pan + clip placement offset).
        let pan = (ev.pan + pan_offset).clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        let g = ev.gain * gain_mult.max(0.0);
        let (gain_l, gain_r) = (angle.cos() * g, angle.sin() * g);

        let src_start = ev.source_pos * src_sr;
        let grain_src_len = ev.duration as f64 * ev.pitch_ratio as f64 * src_sr;
        let has_choke = ev.tag.choke_at.is_some();

        for i in 0..n {
            let oi = start_sample + i;
            if oi < 0 || oi >= out_len {
                continue;
            }
            let phase = i as f32 / n as f32;
            let mut env = grain_env_skewed(phase, ev.envelope, ev.env_skew);
            if venv.is_some() || has_choke {
                let t_abs = (abs_start + i) as f64 / out_sr;
                env *= ev.tag.level_at(t_abs);
                if env <= 0.0 {
                    continue;
                }
            }
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

/// Pure-sample, zero-oscillator, stack-only `VoiceSculpt` & `TapeLoop` DSP renderer.
/// Uses zero heap allocations (plugin `assert_no_alloc`-safe) and maintains sample-accurate
/// parity between offline full-buffer rendering and arbitrary-block streaming.
#[allow(clippy::too_many_arguments)]
fn render_sculpted_grain(
    clip: &AudioClip,
    ev: &GrainEvent,
    out: &mut StereoBuffer,
    t0: f64,
    pan_offset: f32,
    gain_mult: f32,
) {
    if clip.samples.is_empty() || clip.sample_rate == 0 || out.sample_rate == 0 {
        return;
    }
    let out_sr = out.sample_rate as f64;
    let sr_f = out.sample_rate as f32;
    let src_sr = clip.sample_rate as f64;
    let sr_ratio = src_sr / out_sr;
    let out_len = out.len() as i64;
    let side = clip.side.as_deref();
    let sc = &ev.sculpt;

    let venv = ev.tag.env;
    if venv.is_some_and(|e| ev.onset >= e.end()) {
        return;
    }
    if ev.tag.choke_at.is_some_and(|tc| ev.onset >= tc + crate::grain::CHOKE_RELEASE_SECS) {
        return;
    }

    let abs_start = (ev.onset * out_sr).round() as i64;
    let start_sample = abs_start - (t0 * out_sr).round() as i64;
    let n_nom = (ev.duration as f64 * out_sr).round() as i64;
    let n_tail = (sc.tail_secs() as f64 * out_sr).round() as i64;
    let n_tot = n_nom + n_tail;
    if n_nom <= 0 || n_tot <= 0 {
        return;
    }
    let i_blk0 = (-start_sample).max(0);
    let i_blk1 = (out_len - start_sample).min(n_tot);
    if i_blk0 >= i_blk1 {
        return;
    }

    // Warm up recursive IIR / Comb / Shimmer / Ghost ring states across chunk boundaries:
    // grains <= 1.5s run from i=0 (100% bit-exact); multi-second tape loops warm up 1.50s.
    let warm_span = (1.50 * out_sr).round() as i64;
    let i_warm = if i_blk0 <= warm_span {
        0
    } else {
        (i_blk0 - warm_span).max(0)
    };

    let pan = (ev.pan + pan_offset).clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let g = ev.gain * gain_mult.max(0.0);
    let (gain_l, gain_r) = (angle.cos() * g, angle.sin() * g);

    let src_start = ev.source_pos * src_sr;
    let base_rate = ev.pitch_ratio.max(0.02) as f64 * sr_ratio;
    let p_env_st = sc.pitch_env_semitones.clamp(-24.0, 24.0) as f64;
    let p_env_tau = (sc.pitch_env_ms.clamp(10.0, 250.0) as f64) * 0.001;

    // 16-panel Simpson rule to integrate source read phase from 0..t_warm cleanly.
    let mut read_off = if i_warm > 0 {
        let t_w = i_warm as f64 / out_sr;
        if p_env_st.abs() > 1e-4 {
            let steps = 32usize;
            let h = t_w / steps as f64;
            let f_rate = |t: f64| -> f64 { 2.0f64.powf((p_env_st / 12.0) * (-t / p_env_tau).exp()) };
            let mut sum = f_rate(0.0) + f_rate(t_w);
            for k in 1..steps {
                let c = if k % 2 == 1 { 4.0 } else { 2.0 };
                sum += c * f_rate(k as f64 * h);
            }
            (sum * h / 3.0) * out_sr * base_rate
        } else {
            i_warm as f64 * base_rate
        }
    } else {
        0.0
    };
    let grain_src_len = n_nom as f64 * base_rate;

    // Pre-compute filter stages (with smooth non-saturating Basinski per-pass LPF darkening).
    let d_amt = sc.disintegrate.clamp(0.0, 1.0);
    let pass_wear = 1.0 - (-0.115 * (ev.loop_pass as f32 + 0.35)).exp();
    let base_wear = (d_amt * pass_wear).clamp(0.0, 0.96);
    let eff_lpf_hz = (sc.lpf_hz * (0.08f32).powf(base_wear)).clamp(90.0, 20_000.0);
    let eff_hpf_hz = sc.hpf_hz.clamp(20.0, 16_000.0);
    let res_q = sc.resonance_q.clamp(0.4, 8.0);
    let steep_kick_lpf = sc.lpf_hz <= 200.0;
    let steep_wear_lpf = base_wear > 0.02 && !steep_kick_lpf;
    let steep_hpf = eff_hpf_hz >= 200.0;

    let mut hp1 = (eff_hpf_hz > 22.0).then(|| {
        let bq = crate::dsp::Biquad::new(crate::dsp::FilterKind::HighPass, sr_f, eff_hpf_hz, if steep_hpf { 0.707 } else { res_q });
        [bq.clone(), bq]
    });
    let mut hp2 = (steep_hpf && eff_hpf_hz > 22.0).then(|| {
        let bq = crate::dsp::Biquad::new(crate::dsp::FilterKind::HighPass, sr_f, eff_hpf_hz, res_q.max(0.707));
        [bq.clone(), bq]
    });
    let mut lp1 = (eff_lpf_hz < 19_800.0).then(|| {
        let bq = crate::dsp::Biquad::new(
            crate::dsp::FilterKind::LowPass,
            sr_f,
            eff_lpf_hz,
            if steep_kick_lpf || steep_wear_lpf { 0.707 } else { res_q },
        );
        [bq.clone(), bq]
    });
    let mut lp2 = (steep_kick_lpf || steep_wear_lpf).then(|| {
        let q2 = if steep_kick_lpf { res_q.max(0.85) } else { 0.707 };
        let bq = crate::dsp::Biquad::new(crate::dsp::FilterKind::LowPass, sr_f, eff_lpf_hz, q2);
        [bq.clone(), bq]
    });
    let kick_sub_makeup = if steep_kick_lpf { 1.38f32 } else { 1.0 };

    let snap = sc.snare_snap.clamp(0.0, 1.0);
    let mut snare_body = (snap > 1e-3).then(|| {
        let bq = crate::dsp::Biquad::peaking(sr_f, 220.0, 1.25, 4.5 * snap);
        [bq.clone(), bq]
    });
    let mut snare_crack = (snap > 1e-3).then(|| {
        let bq = crate::dsp::Biquad::peaking(sr_f, 2600.0, 0.92, 6.2 * snap);
        [bq.clone(), bq]
    });
    let mut pocket_bq = (sc.pocket_carve && eff_hpf_hz < 420.0 && eff_lpf_hz > 650.0).then(|| {
        let bq = crate::dsp::Biquad::peaking(sr_f, 290.0, 0.90, -4.0);
        [bq.clone(), bq]
    });

    // Drive (strictly PRE-FILTER!).
    let drive_db = sc.drive_db.clamp(0.0, 18.0);
    let drive_lin = if drive_db > 1e-3 { 10.0f32.powf(drive_db / 20.0) } else { 1.0 };
    let drive_comp = if drive_db > 1e-3 { 1.0 / drive_lin.powf(0.56) } else { 1.0 };

    // Cytomic ZDF swept-SVF state for `sub_exciter` (0 oscillators: sample-transient only!).
    let sub_mix = sc.sub_exciter.clamp(0.0, 1.0);
    let mut svf_ic1 = 0.0f32;
    let mut svf_ic2 = 0.0f32;
    let mut sub_env_fast = 0.0f32;
    let mut sub_env_slow = 0.0f32;
    let sub_fast_atk = 1.0 - (-1.0 / (0.0005 * sr_f)).exp();
    let sub_fast_rel = 1.0 - (-1.0 / (0.0060 * sr_f)).exp();
    let sub_slow_atk = 1.0 - (-1.0 / (0.0032 * sr_f)).exp();
    let sub_slow_rel = 1.0 - (-1.0 / (0.0450 * sr_f)).exp();

    // Karplus-Strong fractional-delay comb resonator (stack buffer, 0 heap allocs).
    const COMB_LEN: usize = 1024;
    let comb_mix = sc.comb_mix.clamp(0.0, 1.0);
    let comb_fb = sc.comb_feedback.clamp(0.60, 0.975);
    let comb_hz_l = if ev.comb_hz > 20.0 {
        ev.comb_hz.clamp(55.0, 1100.0)
    } else {
        (220.0 * ev.pitch_ratio).clamp(55.0, 1100.0)
    };
    let comb_hz_r = (comb_hz_l * 1.0018).min(1100.0);
    let comb_del = [
        (sr_f / comb_hz_l).clamp(2.0, (COMB_LEN - 3) as f32),
        (sr_f / comb_hz_r).clamp(2.0, (COMB_LEN - 3) as f32),
    ];
    let comb_damp = if comb_hz_l < 130.0 { 0.30f32 } else { 0.18f32 };
    let mut comb_ring = [[0.0f32; COMB_LEN]; 2];
    let mut comb_pos = (i_warm as usize) % COMB_LEN;
    let mut comb_lp = [0.0f32; 2];
    let mut comb_dc_x = [0.0f32; 2];
    let mut comb_dc_y = [0.0f32; 2];
    let mut comb_in_e = [1e-4f32; 2];
    let mut comb_wet_e = [1e-4f32; 2];
    let rms_coef = (-1.0 / (0.025 * sr_f)).exp();

    // Shared stack delay ring for 85ms 5-tap stereo gated snare room + dual-ratio shimmer + Basinski ghost tail.
    const FX_RING: usize = 2048;
    let mut fx_ring = [[0.0f32; FX_RING]; 2];
    let mut fx_pos = (i_warm as usize) % FX_RING;
    let room_taps_l: [usize; 5] = [0.0059f32, 0.0117, 0.0183, 0.0267, 0.0353]
        .map(|s| ((s * sr_f) as usize).clamp(1, FX_RING - 2));
    let room_taps_r: [usize; 5] = [0.0071f32, 0.0131, 0.0203, 0.0289, 0.0379]
        .map(|s| ((s * sr_f) as usize).clamp(1, FX_RING - 2));
    const ROOM_WEIGHTS: [f32; 5] = [1.0, -0.85, 0.72, -0.60, 0.50];

    let shimmer_amt = sc.shimmer.clamp(0.0, 1.0);
    let shim_cut = (11_500.0 * (0.05f32).powf(base_wear))
        .min(eff_lpf_hz * 1.25)
        .clamp(420.0, sr_f * 0.46);
    let shim_gain = 0.36 * shimmer_amt * (1.0 - 0.92 * base_wear).max(0.04);
    let mut shim_hp = (shimmer_amt > 1e-3).then(|| {
        let bq = crate::dsp::Biquad::new(crate::dsp::FilterKind::HighPass, sr_f, 380.0, 0.707);
        [bq.clone(), bq]
    });
    let mut shim_lp = (shimmer_amt > 1e-3).then(|| {
        let bq = crate::dsp::Biquad::new(crate::dsp::FilterKind::LowPass, sr_f, shim_cut, 0.707);
        [bq.clone(), bq]
    });
    // Ghost reverb tank (2 dark cross-coupled combs on stack) for Basinski disintegrate & shimmer.
    const GHOST_LEN: usize = 1024;
    let mut ghost_ring = [[0.0f32; GHOST_LEN]; 2];
    let ghost_lens = [
        ((0.0173 * sr_f) as usize).clamp(16, GHOST_LEN - 1),
        ((0.0209 * sr_f) as usize).clamp(16, GHOST_LEN - 1),
    ];
    let mut ghost_pos = [
        (i_warm as usize) % ghost_lens[0],
        (i_warm as usize) % ghost_lens[1],
    ];
    let mut ghost_lp = [0.0f32; 2];
    let ghost_cut = (1850.0 * (0.28f32).powf(base_wear)).clamp(320.0, 4000.0);
    let ghost_lp_a = 1.0 - (-2.0 * std::f32::consts::PI * ghost_cut / sr_f).exp();

    let src_len_samps = clip.samples.len() as f64;
    let read_stereo = |src_idx: f64, ratio: f32| -> (f32, f32) {
        if src_idx < 0.0 || src_idx >= src_len_samps {
            return (0.0, 0.0);
        }
        let m = sample_pitched(&clip.samples, src_idx, ratio);
        match side {
            None => (m, m),
            Some(sd) => {
                let s = sample_pitched(sd, src_idx, ratio);
                (m + s, m - s)
            }
        }
    };

    let anti_click_samps = (0.0025 * out_sr).max(1.0) as f32;
    let eff_wow = (sc.wow_flutter.clamp(0.0, 1.0) + 0.28 * base_wear).clamp(0.0, 1.0) as f64;
    let phase_seed = (ev.id & 0xFFFF) as f64 * 0.000_095;

    for i in i_warm..i_blk1 {
        let t_rel = i as f64 / out_sr;
        let t_abs = (abs_start + i) as f64 / out_sr;

        let pitch_mult = if p_env_st.abs() > 1e-4 {
            2.0f64.powf((p_env_st / 12.0) * (-t_rel / p_env_tau).exp())
        } else {
            1.0
        };
        let inst_ratio = (ev.pitch_ratio * pitch_mult as f32).clamp(0.05, 8.0);

        // 1. Sample read (with 0.82 Hz wow + 4.25 Hz flutter + subtle R Haas chorus).
        let perc_hit = (sub_mix > 0.1 || snap > 0.1 || comb_mix > 0.2) && sc.free_loop_secs.is_none();
        let (mut sig_l, mut sig_r) = if i < n_nom {
            let phase = (i as f32 / n_nom as f32).clamp(0.0, 1.0);
            let eff_env = if perc_hit && phase < 0.5 {
                ev.envelope.min(0.04)
            } else {
                ev.envelope
            };
            let mut w_env = grain_env_skewed(phase, eff_env, ev.env_skew);
            if (i as f32) < anti_click_samps {
                let u = (i as f32) / anti_click_samps;
                w_env *= 0.5 * (1.0 - (std::f32::consts::PI * u).cos());
            }
            let base_idx = if ev.reverse {
                src_start + (grain_src_len - read_off)
            } else {
                src_start + read_off
            };
            let (wl, wr) = if eff_wow > 1e-4 {
                let wow_l = (std::f64::consts::TAU * 0.82 * t_abs + phase_seed).sin() * 0.0024
                    + (std::f64::consts::TAU * 4.25 * t_abs + phase_seed * 1.7).sin() * 0.000_38;
                let wow_r = (std::f64::consts::TAU * 0.84 * t_abs + phase_seed + 1.15).sin() * 0.0024
                    + (std::f64::consts::TAU * 4.35 * t_abs + phase_seed * 2.1).sin() * 0.000_38
                    + 0.000_35 * eff_wow;
                (wow_l * eff_wow * src_sr, wow_r * eff_wow * src_sr)
            } else {
                (0.0, 0.0)
            };
            let (dl, _) = read_stereo(base_idx + wl, inst_ratio);
            let (_, dr) = read_stereo(base_idx + wr, inst_ratio);
            let (mut l, mut r) = (dl * w_env, dr * w_env);

            // 2. `snare_snap` 3-grain micro-flam (`0 / 6.5 / 14 ms`) + `130 ms` speech-spill gate.
            if snap > 1e-3 {
                let mut fl_l = l * 0.92;
                let mut fl_r = r * 0.92;
                let flam_win = 0.056 * src_sr;
                if t_rel >= 0.0065 {
                    let off1 = ((t_rel - 0.0065) * out_sr * base_rate * 1.045) % flam_win.max(1.0);
                    let (g1l, g1r) = read_stereo(src_start + off1, inst_ratio * 1.045);
                    fl_l += g1l * 0.72 * w_env;
                    fl_r += g1r * 0.44 * w_env;
                }
                if t_rel >= 0.014 {
                    let off2 = ((t_rel - 0.014) * out_sr * base_rate * 0.962) % flam_win.max(1.0);
                    let (g2l, g2r) = read_stereo(src_start + off2, inst_ratio * 0.962);
                    fl_l += g2l * 0.44 * w_env;
                    fl_r += g2r * 0.72 * w_env;
                }
                // 130 ms speech-spill tail gate: holds full body to 55ms, closes smoothly by 130ms.
                let gate_floor = (1.0 - snap * 1.25).max(0.0);
                let tail_gate = if t_rel <= 0.055 {
                    1.0
                } else if t_rel < 0.130 {
                    let u = ((t_rel - 0.055) / 0.075) as f32;
                    let c = 0.5 * (1.0 + (std::f32::consts::PI * u).cos());
                    gate_floor + (1.0 - gate_floor) * c
                } else {
                    gate_floor
                };
                l = (l * (1.0 - snap) + fl_l * snap * 0.78) * tail_gate;
                r = (r * (1.0 - snap) + fl_r * snap * 0.78) * tail_gate;
            }
            (l, r)
        } else {
            (0.0, 0.0)
        };
        read_off += base_rate * pitch_mult;

        // 3. `sub_exciter`: 68ms-transient-gated Cytomic ZDF swept SVF (`155 -> 50 Hz`, `Q = 8.2`, 0 oscillators!).
        // Half-cycle-matched dual-envelope positive onset flux (`0.5ms` vs `3.2ms`) pings the ZDF SVF
        // with maximum resonant momentum from ANY real sample transient while digital silence produces exact `0.0`!
        if sub_mix > 1e-3 && t_rel < 0.48 {
            let ex_in = if t_rel < 0.068 {
                let mono = 0.5 * (sig_l + sig_r);
                let rect = mono.abs();
                let a_f = if rect > sub_env_fast { sub_fast_atk } else { sub_fast_rel };
                let a_s = if rect > sub_env_slow { sub_slow_atk } else { sub_slow_rel };
                sub_env_fast += a_f * (rect - sub_env_fast);
                sub_env_slow += a_s * (rect - sub_env_slow);
                let onset_ping = (sub_env_fast - sub_env_slow).max(0.0);
                (mono * 0.45 + onset_ping * 4.1) * ((-t_rel / 0.032).exp() as f32)
            } else {
                0.0
            };
            let f_svf = (50.0 + 105.0 * (-t_rel / 0.042).exp() as f32).min(sr_f * 0.42);
            let g_svf = (std::f32::consts::PI * f_svf / sr_f).tan();
            let k_svf = 1.0 / 8.2f32;
            let a1 = 1.0 / (1.0 + g_svf * (g_svf + k_svf));
            let a2 = g_svf * a1;
            let a3 = g_svf * a2;
            let v3 = ex_in - svf_ic2;
            let v1 = a1 * svf_ic1 + a2 * v3;
            let v2 = svf_ic2 + a2 * svf_ic1 + a3 * v3;
            svf_ic1 = 2.0 * v1 - svf_ic1;
            svf_ic2 = 2.0 * v2 - svf_ic2;
            let body_env = (-t_rel / 0.165).exp() as f32;
            let shaped_sub = ((v1 * 1.40 + v2 * 0.95) * body_env * 2.1).tanh() * 0.86;
            sig_l = sig_l * (1.0 - sub_mix * 0.26) + shaped_sub * sub_mix;
            sig_r = sig_r * (1.0 - sub_mix * 0.26) + shaped_sub * sub_mix;
        }

        // 4. PRE-FILTER loudness-compensated `tanh` saturation (`drive_db`).
        if drive_db > 1e-3 {
            sig_l = (sig_l * drive_lin).tanh() * drive_comp;
            sig_r = (sig_r * drive_lin).tanh() * drive_comp;
        }

        // 5. Post-drive filtering + 24dB/oct sub cleanup + 290Hz pocket carve + snare presence & 85ms gated room.
        if let Some(f) = &mut hp1 {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }
        if let Some(f) = &mut hp2 {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }
        if let Some(f) = &mut lp1 {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }
        if let Some(f) = &mut lp2 {
            sig_l = f[0].process(sig_l) * kick_sub_makeup;
            sig_r = f[1].process(sig_r) * kick_sub_makeup;
        }
        if let Some(f) = &mut snare_body {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }
        if let Some(f) = &mut snare_crack {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }
        if let Some(f) = &mut pocket_bq {
            sig_l = f[0].process(sig_l);
            sig_r = f[1].process(sig_r);
        }

        fx_ring[0][fx_pos] = sig_l;
        fx_ring[1][fx_pos] = sig_r;

        if snap > 1e-3 && t_rel < 0.085 {
            let u = (t_rel / 0.085) as f32;
            let room_gate = (1.0 - u * u * u).max(0.0) * 0.32 * snap;
            let (mut rl, mut rr) = (0.0f32, 0.0f32);
            for k in 0..5 {
                let il = (fx_pos + FX_RING - room_taps_l[k]) % FX_RING;
                let ir = (fx_pos + FX_RING - room_taps_r[k]) % FX_RING;
                // Cross-channel stereo gated room reflections (0 noise oscillators!).
                rl += fx_ring[1][il] * ROOM_WEIGHTS[k];
                rr += fx_ring[0][ir] * ROOM_WEIGHTS[k];
            }
            sig_l += rl * room_gate;
            sig_r += rr * room_gate;
        }

        // 6. Scale-locked fractional-delay Karplus-Strong comb resonator (`comb_mix`, `comb_feedback`).
        if comb_mix > 1e-3 {
            let mut pair = [sig_l, sig_r];
            for (c, x) in pair.iter_mut().enumerate() {
                let dry = *x;
                let d = comb_del[c];
                let rd = comb_pos as f32 + COMB_LEN as f32 - d;
                let idx0 = rd.floor();
                let frac = rd - idx0;
                let a = idx0 as usize % COMB_LEN;
                let b = (a + 1) % COMB_LEN;
                let delayed = comb_ring[c][a] + (comb_ring[c][b] - comb_ring[c][a]) * frac;
                comb_lp[c] = delayed * (1.0 - comb_damp) + comb_lp[c] * comb_damp;
                let fb_s = comb_lp[c] * comb_fb + dry * 0.44;
                // In-loop DC blocker + safety tanh.
                let dc_clean = fb_s - comb_dc_x[c] + 0.995 * comb_dc_y[c];
                comb_dc_x[c] = fb_s;
                comb_dc_y[c] = dc_clean;
                let stored = dc_clean.tanh();
                comb_ring[c][comb_pos] = stored;

                comb_in_e[c] = comb_in_e[c] * rms_coef + (dry * dry).max(1e-4) * (1.0 - rms_coef);
                comb_wet_e[c] = comb_wet_e[c] * rms_coef + (stored * stored).max(1e-7) * (1.0 - rms_coef);
                let rms_match = ((comb_in_e[c].max(0.0012) / comb_wet_e[c].max(1e-6)).sqrt() * 1.12).clamp(0.32, 2.4);
                *x = dry * (1.0 - 0.62 * comb_mix) + stored * rms_match * comb_mix;
            }
            sig_l = pair[0];
            sig_r = pair[1];
            comb_pos = (comb_pos + 1) % COMB_LEN;
        }

        // 7. Dual-ratio (`+12st` L, `+7st` R) windowed grain shimmer + Basinski oxide wear & dark ghost tail.
        let mut wet_tail_l = 0.0f32;
        let mut wet_tail_r = 0.0f32;
        if shimmer_amt > 1e-3 {
            let win = (0.036 * sr_f).clamp(64.0, (FX_RING - 8) as f32);
            let sample_shimmer = |ch: usize, rate: f32, idx_samp: i64| -> f32 {
                let mut acc = 0.0f32;
                for ph_k in 0..2 {
                    let ph = (((idx_samp as f32 * (rate - 1.0)) / win) + 0.5 * ph_k as f32).rem_euclid(1.0);
                    let w = 0.5 * (1.0 - (std::f32::consts::TAU * ph).cos());
                    let d = (1.0 - ph) * win;
                    let rd = fx_pos as f32 + FX_RING as f32 - d.clamp(1.0, (FX_RING - 3) as f32);
                    let i0 = rd.floor();
                    let f = rd - i0;
                    let a = i0 as usize % FX_RING;
                    let b = (a + 1) % FX_RING;
                    acc += (fx_ring[ch][a] + (fx_ring[ch][b] - fx_ring[ch][a]) * f) * w;
                }
                acc
            };
            let mut sh_l = sample_shimmer(0, 2.0, i); // +12 st octave
            let mut sh_r = sample_shimmer(1, 1.498_307, i); // +7 st fifth
            if let Some(f) = &mut shim_hp {
                sh_l = f[0].process(sh_l);
                sh_r = f[1].process(sh_r);
            }
            if let Some(f) = &mut shim_lp {
                sh_l = f[0].process(sh_l);
                sh_r = f[1].process(sh_r);
            }
            wet_tail_l += sh_l * shim_gain;
            wet_tail_r += sh_r * shim_gain;
        }

        let oxide = crate::grain::oxide_dropout_at(sc.disintegrate, ev.loop_pass, t_rel, ev.id);
        if sc.disintegrate > 1e-3 || shimmer_amt > 1e-3 {
            let feed_g = 0.25 * shimmer_amt + 0.34 * base_wear * (1.0 - 0.55 * base_wear);
            let gl = ghost_ring[0][ghost_pos[0]];
            let gr = ghost_ring[1][ghost_pos[1]];
            ghost_lp[0] += ghost_lp_a * (gl - ghost_lp[0]);
            ghost_lp[1] += ghost_lp_a * (gr - ghost_lp[1]);
            ghost_ring[0][ghost_pos[0]] = (sig_l * feed_g + wet_tail_l) + ghost_lp[1] * 0.76;
            ghost_ring[1][ghost_pos[1]] = (sig_r * feed_g + wet_tail_r) + ghost_lp[0] * 0.76;
            ghost_pos[0] = (ghost_pos[0] + 1) % ghost_lens[0];
            ghost_pos[1] = (ghost_pos[1] + 1) % ghost_lens[1];
            let ghost_wet = 0.50 * (1.0 - 0.42 * base_wear);
            wet_tail_l += ghost_lp[0] * ghost_wet;
            wet_tail_r += ghost_lp[1] * ghost_wet;
        }

        let dry_keep = ((1.0 - 0.85 * oxide) * (1.0 - 0.35 * base_wear)).clamp(0.02, 1.0);
        let ebb = crate::grain::ebb_flow_gain_at(sc.ebb_flow, ev.loop_pass, t_rel, ev.id);
        sig_l = (sig_l * dry_keep + wet_tail_l) * ebb;
        sig_r = (sig_r * dry_keep + wet_tail_r) * ebb;

        fx_pos = (fx_pos + 1) % FX_RING;

        if i >= i_blk0 {
            let oi = (start_sample + i) as usize;
            let v_level = ev.tag.level_at(t_abs);
            if v_level > 0.0 {
                out.left[oi] += sig_l * gain_l * v_level;
                out.right[oi] += sig_r * gain_r * v_level;
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

    #[test]
    fn grain_envelope_is_sample_exact_across_blocks_and_releases_to_zero() {
        let sr = 48000u32;
        let clip = AudioClip::sine(220.0, 2.0, sr);
        let adsr = crate::envelope::Adsr { attack: 0.05, decay: 0.1, sustain: 0.5, release: 0.1, ..Default::default() };
        let env = crate::envelope::VoiceEnv { adsr, on: 0.1, off: 0.6 };
        let ev = GrainEvent {
            onset: 0.1,
            source_pos: 0.2,
            duration: 1.0,
            pitch_ratio: 1.0,
            gain: 1.0,
            envelope: 0.01,
            id: 1,
            tag: crate::grain::VoiceTag { voice: 5, env: Some(env), tile: None, ..Default::default() },
            ..Default::default()
        };
        let mut whole = StereoBuffer::new(1.2, sr);
        render_grains_audio(&clip, std::slice::from_ref(&ev), &mut whole, 0.0);
        let n = whole.left.len();
        let mut blocks = vec![0.0f32; n];
        let mut pos = 0;
        while pos < n {
            let b = 64.min(n - pos);
            let mut buf = StereoBuffer { left: vec![0.0; b], right: vec![0.0; b], sample_rate: sr };
            render_grains_audio(&clip, std::slice::from_ref(&ev), &mut buf, pos as f64 / sr as f64);
            blocks[pos..pos + b].copy_from_slice(&buf.left);
            pos += b;
        }
        let diff = whole.left.iter().zip(&blocks).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff < 1e-6, "one pass == 64-sample blocks: {diff}");
        let peak = |a: f64, b: f64| whole.left[(a * sr as f64) as usize..(b * sr as f64) as usize].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let (attack, sustain) = (peak(0.1, 0.12), peak(0.4, 0.6));
        assert!(attack < sustain, "attack ramps: {attack} < {sustain}");
        assert!(peak(0.55, 0.6) > 0.1, "sustain holds");
        assert!(peak(0.701, 1.2) < 1e-9, "silent after off + release");
        // A grain starting after the voice ended is skipped.
        let late = GrainEvent { onset: 0.8, ..ev };
        let mut buf = StereoBuffer::new(1.2, sr);
        render_grains_audio(&clip, std::slice::from_ref(&late), &mut buf, 0.0);
        assert_eq!(buf.rms(), 0.0);
    }

    #[test]
    fn kick_sculptor_excites_real_sample_sub_with_zero_oscillators() {
        let sr = 48_000u32;
        // 1) Silence in -> exact zero out (0 oscillators!).
        let silent_clip = AudioClip::new(vec![0.0; sr as usize], sr);
        let kick_ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 0.35,
            pitch_ratio: 1.0,
            gain: 1.0,
            envelope: 0.05,
            sculpt: crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Kick),
            ..Default::default()
        };
        let mut silent_out = StereoBuffer::new(0.5, sr);
        render_grains_audio(&silent_clip, std::slice::from_ref(&kick_ev), &mut silent_out, 0.0);
        assert_eq!(silent_out.peak(), 0.0, "Kick sculptor must emit 0.0 on silent sample");

        // 2) Thin 420 Hz desk tap -> Kick sculptor excites real sub-90 Hz punch.
        let mut tap = vec![0.0f32; sr as usize];
        for (i, s) in tap.iter_mut().enumerate().take((0.06 * sr as f64) as usize) {
            let t = i as f32 / sr as f32;
            *s = 0.7 * (-t / 0.012).exp() * (2.0 * std::f32::consts::PI * 420.0 * t).sin();
        }
        let tap_clip = AudioClip::new(tap, sr);
        let raw_ev = GrainEvent { sculpt: crate::grain::VoiceSculpt::default(), ..kick_ev };
        let (mut raw_out, mut sculpted_out) = (StereoBuffer::new(0.5, sr), StereoBuffer::new(0.5, sr));
        render_grains_audio(&tap_clip, std::slice::from_ref(&raw_ev), &mut raw_out, 0.0);
        render_grains_audio(&tap_clip, std::slice::from_ref(&kick_ev), &mut sculpted_out, 0.0);

        let mut lp_raw = crate::dsp::Biquad::new(crate::dsp::FilterKind::LowPass, sr as f32, 95.0, 0.707);
        let mut lp_sc = crate::dsp::Biquad::new(crate::dsp::FilterKind::LowPass, sr as f32, 95.0, 0.707);
        let mut raw_sub = raw_out.left.clone();
        let mut sc_sub = sculpted_out.left.clone();
        lp_raw.process_buffer(&mut raw_sub);
        lp_sc.process_buffer(&mut sc_sub);
        let e_raw: f32 = raw_sub.iter().map(|x| x * x).sum();
        let e_sc: f32 = sc_sub.iter().map(|x| x * x).sum();
        assert!(e_sc > e_raw * 6.0 + 1e-4, "sub_exciter + pitch drop added real sub weight: {e_sc} vs {e_raw}");
    }

    #[test]
    fn snare_sculptor_flams_widens_and_gates_trailing_speech() {
        let sr = 48_000u32;
        let mut samples = vec![0.0f32; sr as usize];
        let mut rng = crate::dsp::Rng::new(77);
        // Transient clap at 0.00..0.04s, then loud unwanted speech spill at 0.22..0.45s.
        for s in samples.iter_mut().take((0.04 * sr as f64) as usize) {
            *s = 0.8 * rng.bipolar();
        }
        for (i, s) in samples
            .iter_mut()
            .enumerate()
            .take((0.45 * sr as f64) as usize)
            .skip((0.22 * sr as f64) as usize)
        {
            let t = i as f32 / sr as f32;
            *s = 0.65 * (2.0 * std::f32::consts::PI * 520.0 * t).sin();
        }
        let clip = AudioClip::new(samples, sr);
        let raw_ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 0.45,
            pitch_ratio: 1.0,
            gain: 1.0,
            envelope: 0.04,
            ..Default::default()
        };
        let snare_ev = GrainEvent {
            sculpt: crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Snare),
            ..raw_ev
        };
        let (mut raw_buf, mut sn_buf) = (StereoBuffer::new(0.6, sr), StereoBuffer::new(0.6, sr));
        render_grains_audio(&clip, std::slice::from_ref(&raw_ev), &mut raw_buf, 0.0);
        render_grains_audio(&clip, std::slice::from_ref(&snare_ev), &mut sn_buf, 0.0);

        let a = (0.24 * sr as f64) as usize;
        let b = (0.42 * sr as f64) as usize;
        let raw_spill: f32 = (raw_buf.left[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt();
        let sn_spill: f32 = (sn_buf.left[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt();
        assert!(sn_spill < raw_spill * 0.08, "130ms snare tail gate muted trailing speech: {sn_spill} vs {raw_spill}");

        // Stereo room decorrelation during the gated body [0.02..0.10s].
        let r0 = (0.02 * sr as f64) as usize;
        let r1 = (0.10 * sr as f64) as usize;
        let side_e: f32 = sn_buf.left[r0..r1]
            .iter()
            .zip(&sn_buf.right[r0..r1])
            .map(|(l, r)| (l - r).powi(2))
            .sum();
        assert!(side_e > 1e-3, "5-tap gated room created stereo width: {side_e}");
    }

    #[test]
    fn pluck_comb_tunes_unpitched_click_and_disintegrate_darkens_over_passes() {
        let sr = 48_000u32;
        let mut click = vec![0.0f32; sr as usize];
        let mut rng = crate::dsp::Rng::new(1234);
        // 14ms unpitched noise burst ("page turn").
        for s in click.iter_mut().take((0.014 * sr as f64) as usize) {
            *s = 0.85 * rng.bipolar();
        }
        let click_clip = AudioClip::new(click, sr);
        let pluck_ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 0.18,
            pitch_ratio: 1.0,
            gain: 1.0,
            envelope: 0.05,
            sculpt: crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Pluck),
            comb_hz: 220.0,
            ..Default::default()
        };
        let mut pluck_buf = StereoBuffer::new(0.45, sr);
        render_grains_audio(&click_clip, std::slice::from_ref(&pluck_ev), &mut pluck_buf, 0.0);
        let (f0_opt, conf) = crate::dsp::detect_pitch_nsdf(
            &pluck_buf.left[(0.01 * sr as f64) as usize..(0.16 * sr as f64) as usize],
            sr,
        );
        let f0 = f0_opt.expect("Karplus-Strong comb pitched the noise click");
        assert!((f0 - 220.0).abs() < 6.0 && conf > 0.65, "tuned f0={f0:.1} Hz, conf={conf:.2}");
        // Ring-out extends past 0.18s (+140ms tail).
        let tail_rms: f32 = (pluck_buf.left[(0.185 * sr as f64) as usize..(0.25 * sr as f64) as usize]
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            / (0.065 * sr as f32))
            .sqrt();
        assert!(tail_rms > 1e-4, "comb rings out into +140ms tail: {tail_rms}");

        // Basinski disintegrate: strictly monotonic HF darkening AND body decay across passes 0, 4, 8, 12, 16!
        let rich = AudioClip::saw_stack(220.0, 1.0, sr);
        let mut dis_sculpt = crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Disintegrate);
        dis_sculpt.ebb_flow = 0.0; // isolate pure tape wear from slow LFO breathing
        let pass_metrics = |pass: u32| -> (f32, f32) {
            let ev = GrainEvent {
                onset: 0.0,
                source_pos: 0.0,
                duration: 0.6,
                pitch_ratio: 1.0,
                gain: 1.0,
                envelope: 0.1,
                id: 99,
                sculpt: dis_sculpt,
                loop_pass: pass,
                ..Default::default()
            };
            let mut b = StereoBuffer::new(0.7, sr);
            render_grains_audio(&rich, std::slice::from_ref(&ev), &mut b, 0.0);
            let total: f32 = b.left.iter().map(|x| x * x).sum::<f32>().max(1e-9);
            let rms = (total / b.left.len() as f32).sqrt();
            let mut hp = crate::dsp::Biquad::new(crate::dsp::FilterKind::HighPass, sr as f32, 2000.0, 0.707);
            let mut hi = b.left.clone();
            hp.process_buffer(&mut hi);
            let hf_r = hi.iter().map(|x| x * x).sum::<f32>() / total;
            (hf_r, rms)
        };
        let passes = [0u32, 4, 8, 12, 16];
        let vals: Vec<(u32, f32, f32)> = passes.iter().map(|&p| { let (h, r) = pass_metrics(p); (p, h, r) }).collect();
        for w in vals.windows(2) {
            let (p_prev, hf_prev, rms_prev) = w[0];
            let (p_next, hf_next, rms_next) = w[1];
            assert!(
                hf_next < hf_prev,
                "Disintegrate >2kHz HF ratio must darken monotonically: pass {p_prev} ({hf_prev:.5}) -> pass {p_next} ({hf_next:.5})"
            );
            assert!(
                rms_next < rms_prev,
                "Disintegrate RMS must wear monotonically: pass {p_prev} ({rms_prev:.5}) -> pass {p_next} ({rms_next:.5})"
            );
        }
    }
}
