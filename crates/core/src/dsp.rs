//! DSP building blocks: biquad filters (RBJ cookbook), grain envelopes,
//! pitch detection, and a small deterministic PRNG.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

impl FilterKind {
    pub fn parse(name: &str) -> Option<FilterKind> {
        let n = name.to_ascii_lowercase().replace([' ', '-', '_'], "");
        Some(match n.as_str() {
            "lowpass" | "lp" | "low" => FilterKind::LowPass,
            "highpass" | "hp" | "high" => FilterKind::HighPass,
            "bandpass" | "bp" | "band" => FilterKind::BandPass,
            "notch" | "bandreject" | "br" => FilterKind::Notch,
            _ => return None,
        })
    }
}

/// A filter stage in a clip's audio chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterSpec {
    pub kind: FilterKind,
    pub freq: f32,
    pub q: f32,
}

/// A parametric peaking bell EQ band (`gain_db` in `-24..+24` dB).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub freq: f32,
    pub q: f32,
    pub gain_db: f32,
}

/// Direct form 1 biquad, RBJ cookbook coefficients.
#[derive(Debug, Clone)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    pub fn new(kind: FilterKind, sample_rate: f32, freq: f32, q: f32) -> Biquad {
        let freq = freq.clamp(10.0, sample_rate * 0.49);
        let q = q.max(0.05);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            FilterKind::LowPass => {
                let b1 = 1.0 - cos_w0;
                (b1 / 2.0, b1, b1 / 2.0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterKind::HighPass => {
                let b1 = -(1.0 + cos_w0);
                let b0 = (1.0 + cos_w0) / 2.0;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterKind::BandPass => {
                // Constant 0 dB peak gain.
                (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterKind::Notch => {
                (1.0, -2.0 * cos_w0, 1.0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
        };

        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn from_spec(spec: &FilterSpec, sample_rate: f32) -> Biquad {
        Biquad::new(spec.kind, sample_rate, spec.freq, spec.q)
    }

    /// RBJ cookbook parametric peaking bell EQ (`gain_db` in `-24..+24`).
    pub fn peaking(sample_rate: f32, freq: f32, q: f32, gain_db: f32) -> Biquad {
        let freq = freq.clamp(15.0, sample_rate * 0.49);
        let q = q.max(0.08);
        let gdb = gain_db.clamp(-24.0, 24.0);
        if gdb.abs() < 1e-4 {
            return Biquad {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
                x1: 0.0,
                x2: 0.0,
                y1: 0.0,
                y2: 0.0,
            };
        }
        let a = 10.0f32.powf(gdb / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// 2nd-order RBJ allpass filter (`|H(e^{jw})| == 1` everywhere; at `Q = FRAC_1_SQRT_2`,
    /// `AP(z) === LP_LR4(z) + HP_LR4(z)` identically for phase-coherent Linkwitz-Riley trees).
    pub fn allpass(sample_rate: f32, freq: f32, q: f32) -> Biquad {
        let freq = freq.clamp(10.0, sample_rate * 0.49);
        let q = q.max(0.05);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = 1.0 - alpha;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 + alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Update coefficients in-place while preserving `x1, x2, y1, y2` delay state
    /// so live crossover sweeps remain smooth and click-free.
    #[inline]
    pub fn copy_coeffs_from(&mut self, src: &Biquad) {
        self.b0 = src.b0;
        self.b1 = src.b1;
        self.b2 = src.b2;
        self.a1 = src.a1;
        self.a2 = src.a2;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn process_buffer(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }
}

/// Apply a chain of filters to a buffer, in order.
pub fn apply_filter_chain(samples: &mut [f32], chain: &[FilterSpec], sample_rate: f32) {
    for spec in chain {
        let mut bq = Biquad::from_spec(spec, sample_rate);
        bq.process_buffer(samples);
    }
}

/// Apply a parametric peaking EQ chain to a mono buffer in-place.
pub fn apply_eq_chain(samples: &mut [f32], bands: &[EqBand], sample_rate: f32) {
    for b in bands {
        let mut bq = Biquad::peaking(sample_rate, b.freq, b.q, b.gain_db);
        bq.process_buffer(samples);
    }
}

/// Tukey (tapered cosine) grain envelope.
///
/// `phase` in [0, 1]; `shape` in (0, 1]: fraction of the grain spent in
/// fade-in + fade-out. shape = 1 gives a full Hann window; small shapes give
/// a rectangular window with short ramps.
#[inline]
pub fn grain_env(phase: f32, shape: f32) -> f32 {
    grain_env_skewed(phase, shape, 0.0)
}

/// Skewed tukey grain envelope: `skew` in [-1, 1] shifts the fade budget
/// between attack and release. -1 = instant attack, all release (expodec
/// percussive shape); 0 = symmetric; +1 = all attack, instant release
/// (reverse-decay swell).
#[inline]
pub fn grain_env_skewed(phase: f32, shape: f32, skew: f32) -> f32 {
    let phase = phase.clamp(0.0, 1.0);
    let a = shape.clamp(0.01, 1.0);
    let skew = skew.clamp(-1.0, 1.0);
    let attack = (a * (1.0 + skew) / 2.0).max(0.002);
    let release = (a * (1.0 - skew) / 2.0).max(0.002);
    if phase < attack {
        0.5 * (1.0 + (std::f32::consts::PI * (phase / attack - 1.0)).cos())
    } else if phase > 1.0 - release {
        0.5 * (1.0 + (std::f32::consts::PI * ((phase - 1.0 + release) / release)).cos())
    } else {
        1.0
    }
}

fn detect_pitch_window(
    x: &[f32],
    win: usize,
    min_lag: usize,
    max_lag: usize,
    sr: f32,
) -> Option<(f32, f32)> {
    let energy: f32 = x[..win].iter().map(|s| s * s).sum();
    if energy < 1e-6 {
        return None;
    }

    let mut corrs = vec![0.0f32; max_lag + 1];
    let mut best_corr = 0.0f32;
    for lag in min_lag..=max_lag {
        let mut corr = 0.0f32;
        let mut e2 = 0.0f32;
        for i in 0..win {
            corr += x[i] * x[i + lag];
            e2 += x[i + lag] * x[i + lag];
        }
        let norm = (energy * e2).sqrt();
        if norm <= 0.0 {
            continue;
        }
        let c = corr / norm;
        corrs[lag] = c;
        if c > best_corr {
            best_corr = c;
        }
    }
    if best_corr < 0.5 {
        return None;
    }
    // Any multiple of the true period correlates as well as the period
    // itself; take the SMALLEST lag near the maximum to avoid octave errors.
    let threshold = best_corr - 0.02;
    let mut best_lag = (min_lag..=max_lag).find(|&l| corrs[l] >= threshold)?;
    // Climb to the local peak of that first near-max region.
    while best_lag + 1 <= max_lag && corrs[best_lag + 1] > corrs[best_lag] {
        best_lag += 1;
    }

    // Parabolic interpolation around the peak for sub-sample lag accuracy.
    let corr_at = |lag: usize| -> f32 {
        let mut c = 0.0;
        for i in 0..win {
            c += x[i] * x[i + lag];
        }
        c
    };
    let lag = if best_lag > min_lag && best_lag < max_lag {
        let c0 = corr_at(best_lag - 1);
        let c1 = corr_at(best_lag);
        let c2 = corr_at(best_lag + 1);
        let denom = c0 - 2.0 * c1 + c2;
        if denom.abs() > 1e-9 {
            best_lag as f32 + 0.5 * (c0 - c2) / denom
        } else {
            best_lag as f32
        }
    } else {
        best_lag as f32
    };

    Some((sr / lag, best_corr))
}

/// Estimate the fundamental frequency of a buffer via multi-window normalized
/// autocorrelation. Stationary tones preserve exact sub-cent frequency, while
/// melismatic / vibrato sources (`spread > 12 cents` across voiced windows)
/// stabilize onto their nearest A=440 chromatic semitone so `Key` pitch
/// quantization never injects a spurious quarter-tone offset from vibrato.
pub fn detect_pitch(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let sr = sample_rate as f32;
    let win = ((sr * 0.18) as usize).max(256);
    let max_lag = (sr / 50.0) as usize; // down to 50 Hz
    let min_lag = (sr / 1200.0).max(2.0) as usize; // up to 1200 Hz
    let needed = win + max_lag + 1;
    if samples.len() < needed || min_lag >= max_lag {
        return None;
    }
    let avail = samples.len() - needed;
    let n_wins = if avail < win / 2 {
        1
    } else {
        ((avail / (win / 2)) + 1).clamp(2, 13)
    };
    let mut midis: Vec<f32> = Vec::with_capacity(n_wins);
    for w_idx in 0..n_wins {
        let start = if n_wins == 1 {
            avail / 2
        } else {
            w_idx * avail / (n_wins - 1)
        };
        if let Some((hz, _corr)) =
            detect_pitch_window(&samples[start..start + needed], win, min_lag, max_lag, sr)
        {
            if (45.0..1250.0).contains(&hz) {
                midis.push(69.0 + 12.0 * (hz / 440.0).log2());
            }
        }
    }
    if midis.is_empty() {
        return None;
    }
    midis.sort_by(|a, b| a.total_cmp(b));
    let med = midis[midis.len() / 2];
    let spread = midis[midis.len() - 1] - midis[0];
    let eff_midi = if midis.len() >= 3 && spread > 0.12 {
        med.round()
    } else {
        med
    };
    Some(440.0 * 2.0f32.powf((eff_midi - 69.0) / 12.0))
}

/// McLeod Normalized Square Difference Function (NSDF) pitch & confidence
/// detector on the loudest `75 ms` window (`15 ms` hop, skipping the initial
/// `<= 12 ms` transient click), searching `60..=1100 Hz`.
///
/// Picks the first positive-lobe key maximum `>= 0.88 * max_peak` (preventing
/// sub-octave errors) with 3-point parabolic lag refinement. Returns
/// `(Some(f0_hz), confidence)` when `confidence >= 0.45`, else `(None, confidence)`.
pub fn detect_pitch_nsdf(samples: &[f32], sample_rate: u32) -> (Option<f32>, f32) {
    if sample_rate == 0 || samples.len() < 128 {
        return (None, 0.0);
    }
    let sr = sample_rate as f32;
    let dur_s = samples.len() as f32 / sr;
    let skip = ((0.012f32.min(dur_s * 0.10)) * sr) as usize;
    let body = if skip + 64 < samples.len() { &samples[skip..] } else { samples };

    let win_len = ((0.075 * sr) as usize).clamp(64, body.len());
    let hop_len = ((0.015 * sr) as usize).max(16);

    // Locate loudest 75 ms window.
    let mut best_start = 0usize;
    let mut best_e = 0.0f32;
    let mut st = 0usize;
    loop {
        let end = (st + win_len).min(body.len());
        if end <= st {
            break;
        }
        let e: f32 = body[st..end].iter().map(|v| v * v).sum::<f32>() / (end - st) as f32;
        if e > best_e {
            best_e = e;
            best_start = st;
        }
        if end >= body.len() {
            break;
        }
        st += hop_len;
    }
    if best_e < 1e-7 {
        return (None, 0.0);
    }
    let raw_win = &body[best_start..(best_start + win_len).min(body.len())];

    // Decimate 3:1 with 3-tap [0.25, 0.5, 0.25] smoothing when SR >= 36 kHz.
    let (dec_buf, eff_sr): (Vec<f32>, f32) = if sample_rate >= 36_000 && raw_win.len() >= 192 {
        let m = raw_win.len() / 3;
        let mut d = Vec::with_capacity(m);
        for k in 0..m {
            let j = k * 3;
            let a = raw_win[j];
            let b = raw_win.get(j + 1).copied().unwrap_or(a);
            let c = raw_win.get(j + 2).copied().unwrap_or(b);
            d.push(0.25 * a + 0.50 * b + 0.25 * c);
        }
        (d, sr / 3.0)
    } else {
        (raw_win.to_vec(), sr)
    };

    let w = dec_buf.len();
    let min_lag = (eff_sr / 1100.0).floor().max(2.0) as usize;
    let max_lag = ((eff_sr / 60.0).ceil() as usize).min(w.saturating_sub(4));
    if min_lag >= max_lag {
        return (None, 0.0);
    }

    // Zero-mean the analysis window so DC never inflates NSDF.
    let mean = dec_buf.iter().sum::<f32>() / w as f32;
    let x: Vec<f32> = dec_buf.iter().map(|v| v - mean).collect();

    let mut nsdf = vec![0.0f32; max_lag + 1];
    for tau in 1..=max_lag {
        let limit = w - tau;
        let mut acf = 0.0f32;
        let mut m_term = 0.0f32;
        for j in 0..limit {
            let a = x[j];
            let b = x[j + tau];
            acf += a * b;
            m_term += a * a + b * b;
        }
        if m_term > 1e-12 {
            nsdf[tau] = (2.0 * acf / m_term).clamp(-1.0, 1.0);
        }
    }

    // Collect key maxima inside positive lobes after the first zero-crossing.
    let mut first_neg = min_lag;
    for (tau, &val) in nsdf.iter().enumerate().take(max_lag + 1).skip(1) {
        if val <= 0.0 {
            first_neg = tau;
            break;
        }
    }
    let start_tau = min_lag.max(first_neg);
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    let mut in_pos = false;
    let mut lobe_tau = start_tau;
    let mut lobe_max = 0.0f32;
    for (tau, &v) in nsdf.iter().enumerate().take(max_lag + 1).skip(start_tau) {
        if v > 0.0 {
            if !in_pos {
                in_pos = true;
                lobe_tau = tau;
                lobe_max = v;
            } else if v > lobe_max {
                lobe_max = v;
                lobe_tau = tau;
            }
        } else if in_pos {
            peaks.push((lobe_tau, lobe_max));
            in_pos = false;
        }
    }
    if in_pos && lobe_max > 0.0 {
        peaks.push((lobe_tau, lobe_max));
    }
    if peaks.is_empty() {
        return (None, 0.0);
    }

    let max_peak = peaks.iter().fold(0.0f32, |m, p| m.max(p.1));
    if max_peak < 0.35 {
        return (None, max_peak.max(0.0));
    }
    let thresh = 0.88 * max_peak;
    let &(best_tau, conf) = peaks.iter().find(|p| p.1 >= thresh).unwrap_or(&peaks[0]);

    let refined_tau = if best_tau > 1 && best_tau < max_lag {
        let y0 = nsdf[best_tau - 1];
        let y1 = nsdf[best_tau];
        let y2 = nsdf[best_tau + 1];
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-7 {
            best_tau as f32 + (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5)
        } else {
            best_tau as f32
        }
    } else {
        best_tau as f32
    };

    let hz = eff_sr / refined_tau.max(1.0);
    let conf = conf.clamp(0.0, 1.0);
    if conf >= 0.45 && (55.0..=1150.0).contains(&hz) {
        (Some(hz), conf)
    } else {
        (None, conf)
    }
}

/// Asymmetric zero-crossing snapper around `idx` searching `[-back_ms, +fwd_ms]`
/// with a `0.85` pre-center distance bias so transient attacks are never shaved off.
/// Falls back to `argmin(|samples[k]|)` when DC offset prevents a sign change.
pub fn snap_zero_crossing(
    samples: &[f32],
    sample_rate: u32,
    idx: usize,
    back_ms: f32,
    fwd_ms: f32,
) -> usize {
    if samples.len() < 2 || sample_rate == 0 {
        return idx.min(samples.len().saturating_sub(1));
    }
    let n = samples.len();
    let idx = idx.min(n - 1);
    let sr = sample_rate as f32;
    let back = ((back_ms.max(0.0) * 0.001 * sr).round() as usize).max(1);
    let fwd = ((fwd_ms.max(0.0) * 0.001 * sr).round() as usize).max(1);
    let lo = idx.saturating_sub(back);
    let hi = (idx + fwd).min(n - 2);
    if lo > hi {
        return idx;
    }

    let mut best_zc: Option<(usize, f32)> = None;
    let mut min_abs_i = idx;
    let mut min_abs_v = samples[idx].abs();

    for k in lo..=hi {
        let s0 = samples[k];
        let s1 = samples[k + 1];
        let a0 = s0.abs();
        if a0 < min_abs_v {
            min_abs_v = a0;
            min_abs_i = k;
        }
        let crosses = (s0 <= 0.0 && s1 > 0.0) || (s0 >= 0.0 && s1 < 0.0);
        if crosses {
            let pick = if s0.abs() <= s1.abs() { k } else { k + 1 };
            let raw_dist = pick.abs_diff(idx) as f32;
            let weighted = if pick <= idx { raw_dist * 0.85 } else { raw_dist };
            if best_zc.is_none_or(|(_, best_w)| weighted < best_w) {
                best_zc = Some((pick, weighted));
            }
        }
    }
    best_zc.map(|(i, _)| i).unwrap_or(min_abs_i)
}

/// Split `samples` into 5 biquad bands (`sub <90`, `low 90-250`, `mid 250-2k`,
/// `high 2k-8k`, `air >8k`) rectified into `4 ms` (`250 Hz`) RMS envelope frames.
/// Returns `(frames, hop_samples)`.
pub fn analyze_5band_envelopes(samples: &[f32], sample_rate: u32) -> (Vec<[f32; 5]>, usize) {
    let sr = sample_rate.max(1) as f32;
    let hop = ((sample_rate as usize) / 250).max(1);
    if samples.is_empty() {
        return (Vec::new(), hop);
    }
    let nyq = sr * 0.48;
    let mut b_sub = Biquad::new(FilterKind::LowPass, sr, 90.0f32.min(nyq), 0.707);
    let mut b_low = Biquad::new(FilterKind::BandPass, sr, 150.0f32.min(nyq), 1.0);
    let mut b_mid = Biquad::new(FilterKind::BandPass, sr, 707.0f32.min(nyq), 0.65);
    let mut b_high = Biquad::new(FilterKind::BandPass, sr, 4000.0f32.min(nyq), 0.75);
    let mut b_air = if sr > 17_000.0 {
        Some(Biquad::new(FilterKind::HighPass, sr, 8000.0f32.min(nyq), 0.707))
    } else {
        None
    };

    let n_frames = samples.len().div_ceil(hop);
    let mut out = Vec::with_capacity(n_frames);
    for fi in 0..n_frames {
        let s0 = fi * hop;
        let s1 = ((fi + 1) * hop).min(samples.len());
        let inv = 1.0 / (s1 - s0).max(1) as f32;
        let mut acc = [0.0f32; 5];
        for &x in &samples[s0..s1] {
            let v0 = b_sub.process(x);
            let v1 = b_low.process(x);
            let v2 = b_mid.process(x);
            let v3 = b_high.process(x);
            let v4 = b_air.as_mut().map_or(0.0, |f| f.process(x));
            acc[0] += v0 * v0;
            acc[1] += v1 * v1;
            acc[2] += v2 * v2;
            acc[3] += v3 * v3;
            acc[4] += v4 * v4;
        }
        out.push([
            (acc[0] * inv).sqrt(),
            (acc[1] * inv).sqrt(),
            (acc[2] * inv).sqrt(),
            (acc[3] * inv).sqrt(),
            (acc[4] * inv).sqrt(),
        ]);
    }
    (out, hop)
}

/// Compute normalized 5-band distribution (`sub, low, mid, high, air` summing to ~1),
/// transient `punch` (`0..1`), and broadband `rms_db` (`-96..0`) for a slice buffer.
pub fn slice_band_stats(samples: &[f32], sample_rate: u32) -> ([f32; 5], f32, f32) {
    if samples.is_empty() || sample_rate == 0 {
        return ([0.0; 5], 0.0, -96.0);
    }
    let total_sq: f32 = samples.iter().map(|s| s * s).sum();
    let total_rms = (total_sq / samples.len() as f32).sqrt();
    if total_rms < 1e-6 {
        return ([0.0; 5], 0.0, -96.0);
    }
    let rms_db = (20.0 * total_rms.log10()).clamp(-96.0, 6.0);
    let att_n = ((0.025 * sample_rate as f32) as usize).clamp(1, samples.len());
    let att_rms = (samples[..att_n].iter().map(|s| s * s).sum::<f32>() / att_n as f32).sqrt();
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let punch = ((att_rms / total_rms.max(1e-5)) * 0.45 + (peak * 0.70).min(0.55)).clamp(0.0, 1.0);

    let (envs, _) = analyze_5band_envelopes(samples, sample_rate);
    let mut sum = [0.0f32; 5];
    for e in &envs {
        for b in 0..5 {
            sum[b] += e[b];
        }
    }
    let norm = (sum.iter().sum::<f32>()).max(1e-6);
    let bands = [sum[0] / norm, sum[1] / norm, sum[2] / norm, sum[3] / norm, sum[4] / norm];
    (bands, punch, rms_db)
}

/// 5-band log-compressed weighted spectral flux onset detector with `-30 ms`
/// speech/mid consonant pre-roll and asymmetric zero-crossing snap (`[-2.0ms, +1.8ms]`).
/// Returns non-overlapping `(start_sec, end_sec)` spans in chronological order.
pub fn detect_onsets_5band(
    samples: &[f32],
    sample_rate: u32,
    max_slices: usize,
) -> Vec<(f64, f64)> {
    let max_slices = max_slices.clamp(1, 128);
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let dur = samples.len() as f64 / sample_rate as f64;
    let (envs, hop) = analyze_5band_envelopes(samples, sample_rate);
    let n = envs.len();
    if n < 4 {
        return vec![(0.0, dur)];
    }

    let tot_env: Vec<f32> = envs
        .iter()
        .map(|b| (b[0] * b[0] + b[1] * b[1] + b[2] * b[2] + b[3] * b[3] + b[4] * b[4]).sqrt())
        .collect();
    let peak_tot = tot_env.iter().copied().fold(0.0f32, f32::max);
    if peak_tot < 1e-5 {
        return vec![(0.0, dur)];
    }
    let inv_peak = 1.0 / peak_tot;

    // Smooth sub/low/mid band envelopes across wave-period boundaries (~12ms on sub/low,
    // ~6ms on mid) so 4ms hops never alias against 50..250 Hz fundamental cycles!
    const ALPHA: [f32; 5] = [0.24, 0.28, 0.48, 0.78, 0.85];
    let mut smooth = envs.clone();
    let mut state = envs[0];
    for frame in &mut smooth {
        for b in 0..5 {
            state[b] += ALPHA[b] * (frame[b] - state[b]);
            frame[b] = state[b];
        }
    }

    // Band weights: [sub, low, mid, high, air], log compression k = 180.0
    const W: [f32; 5] = [0.70, 1.15, 1.35, 1.50, 1.10];
    const K: f32 = 180.0;
    let mut flux = vec![0.0f32; n];
    for i in 1..n {
        let mut f = 0.0f32;
        for b in 0..5 {
            let cur = (1.0 + K * smooth[i][b] * inv_peak).ln();
            let prev = (1.0 + K * smooth[i - 1][b] * inv_peak).ln();
            f += W[b] * (cur - prev).max(0.0);
        }
        flux[i] = f;
    }

    let mut sorted_flux = flux.clone();
    sorted_flux.sort_by(|a, b| a.total_cmp(b));
    let p70 = sorted_flux[(sorted_flux.len() * 70 / 100).min(sorted_flux.len() - 1)];
    let global_floor = (p70 * 0.85).max(0.32);

    // Adaptive local threshold over +-28 frames (+-112 ms) and min_gap = 20 frames (80 ms).
    let min_gap = 20usize;
    let mut cands: Vec<(usize, f32)> = Vec::new();
    if tot_env[0] >= peak_tot * 0.08 {
        cands.push((0, tot_env[0] * inv_peak + 0.2));
    }
    let mut scratch = Vec::with_capacity(57);
    for i in 1..n - 1 {
        if flux[i] < flux[i - 1] || flux[i] < flux[i + 1] {
            continue;
        }
        let lo = i.saturating_sub(28);
        let hi = (i + 28).min(n - 1);
        scratch.clear();
        scratch.extend_from_slice(&flux[lo..=hi]);
        scratch.sort_by(|a, b| a.total_cmp(b));
        let med = scratch[scratch.len() / 2];
        let mean = scratch.iter().sum::<f32>() / scratch.len() as f32;
        let thresh = global_floor.max(1.8 * med + 0.18).max(1.25 * mean + 0.12);
        let look_hi = (i + 10).min(n);
        let look_e = tot_env[i..look_hi].iter().copied().fold(0.0f32, f32::max) * inv_peak;
        if flux[i] >= thresh && look_e > 0.012 {
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
        return vec![(0.0, dur)];
    }
    if cands.len() > max_slices {
        cands.sort_by(|a, b| b.1.total_cmp(&a.1));
        cands.truncate(max_slices);
        cands.sort_by_key(|c| c.0);
    }

    let sr = sample_rate as usize;
    let preroll_samples = (0.030 * sample_rate as f32).round() as usize; // 30 ms consonant guard
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(cands.len());
    let mut prev_end_samp = 0usize;

    for (idx, &(f0, _)) in cands.iter().enumerate() {
        let next_f = cands.get(idx + 1).map(|c| c.0).unwrap_or(n);
        // Active slice end: advance while energy stays above 4% of peak (allowing brief
        // inter-syllable dips up to 60 ms = 15 frames so spoken phrases aren't chopped mid-word).
        let mut end_f = (f0 + 6).min(next_f);
        let mut quiet_run = 0usize;
        let mut scan = end_f;
        while scan < next_f {
            if tot_env[scan] >= peak_tot * 0.04 {
                quiet_run = 0;
                end_f = scan + 1;
            } else {
                quiet_run += 1;
                if quiet_run >= 15 {
                    break;
                }
            }
            scan += 1;
        }

        // Check whether this slice is mid/phrase-dominant (speech/vocal).
        let check_end = end_f.min(f0 + 75).max(f0 + 1).min(n);
        let mut b_sum = [0.0f32; 5];
        for fr in &envs[f0..check_end] {
            for b in 0..5 {
                b_sum[b] += fr[b];
            }
        }
        let b_tot = b_sum.iter().sum::<f32>().max(1e-6);
        let sub_share = b_sum[0] / b_tot;
        let vocal_share = (b_sum[2] + 0.55 * b_sum[3] + 0.35 * b_sum[1]) / b_tot;
        let dur_frames = end_f.saturating_sub(f0);
        // Spoken syllables/phrases carry vocal formant energy across low-mid..high
        // and often begin with a soft fricative/plosive consonant up to 30ms before
        // the vowel flux peak.
        let is_speech_like = vocal_share >= 0.42 && sub_share < 0.38 && dur_frames >= 12;

        let raw_start = (f0 * hop).min(samples.len().saturating_sub(1));
        let pulled_start = if is_speech_like && raw_start > prev_end_samp {
            // Pull -30ms earlier, advancing forward ONLY through true digital silence
            // (< 0.1% of peak) so soft opening consonants ('s', 'f', 't', 'k') are never clipped!
            let min_bound = raw_start.saturating_sub(preroll_samples).max(prev_end_samp);
            let min_f = min_bound / hop;
            let mut start_f = min_f;
            while start_f < f0 && tot_env[start_f] < peak_tot * 0.001 {
                start_f += 1;
            }
            (start_f * hop).max(min_bound).min(raw_start)
        } else {
            raw_start.max(prev_end_samp)
        };

        let start_samp = if is_speech_like {
            // Speech rule: never snap forward > +1.0ms so opening consonants stay intact!
            snap_zero_crossing(samples, sample_rate, pulled_start, 3.0, 1.0).max(prev_end_samp)
        } else {
            snap_zero_crossing(samples, sample_rate, pulled_start, 2.0, 1.8).max(prev_end_samp)
        };

        let raw_end = (end_f * hop).clamp((start_samp + sr / 80).min(samples.len()), samples.len());
        let max_end = (next_f * hop).min(samples.len());
        let end_samp = snap_zero_crossing(samples, sample_rate, raw_end.min(max_end), 2.0, 1.8)
            .clamp(start_samp.saturating_add(1), max_end.max(start_samp.saturating_add(1)))
            .min(samples.len());

        let s_sec = start_samp as f64 / sample_rate as f64;
        let e_sec = end_samp as f64 / sample_rate as f64;
        if e_sec - s_sec >= 0.01 {
            out.push((s_sec, e_sec));
            prev_end_samp = end_samp;
        }
    }
    if out.is_empty() {
        vec![(0.0, dur)]
    } else {
        out
    }
}

/// Spectral tilt (the up/down elevation cue): `tilt` +1 = bright/airy
/// (highs up, lows down), -1 = dark/heavy. One-pole split at 800 Hz,
/// streaming-safe.
#[derive(Debug, Clone, Default)]
pub struct TiltState {
    lp: [f32; 2],
}

pub fn apply_tilt(
    left: &mut [f32],
    right: &mut [f32],
    state: &mut TiltState,
    tilt: f32,
    sample_rate: u32,
) {
    let tilt = tilt.clamp(-1.0, 1.0);
    if tilt.abs() < 1e-4 {
        return;
    }
    let a = 1.0 - (-2.0 * std::f32::consts::PI * 800.0 / sample_rate as f32).exp();
    let k = tilt * 0.85;
    for (c, ch) in [left, right].into_iter().enumerate() {
        let mut lp = state.lp[c];
        for x in ch.iter_mut() {
            lp += a * (*x - lp);
            let hp = *x - lp;
            *x = lp * (1.0 - k) + hp * (1.0 + k);
        }
        state.lp[c] = lp;
    }
}

/// Small deterministic xorshift* PRNG so renders are reproducible.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15).max(1))
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    /// Uniform in [a, b).
    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next_f32()
    }
}

// ---------------------------------------------------------------------------
// ISOL8 — 5-Band 24 dB/Oct Linkwitz-Riley Studio Frequency Isolator
// ---------------------------------------------------------------------------

/// Which stereo component the 5-band isolator filters (`CH. MODE` on ISOL8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum IsolatorChannelMode {
    /// Filter both Left and Right channels identically (`STEREO`).
    #[default]
    Stereo,
    /// Filter only Mid (`0.5*(L+R)`), keeping Side phase-aligned via matching allpass (`MID`).
    Mid,
    /// Filter only Side (`0.5*(L-R)`), keeping Mid phase-aligned via matching allpass (`SIDE`).
    Side,
}

impl IsolatorChannelMode {
    pub fn label(self) -> &'static str {
        match self {
            IsolatorChannelMode::Stereo => "STEREO",
            IsolatorChannelMode::Mid => "MID",
            IsolatorChannelMode::Side => "SIDE",
        }
    }
}

/// Output channel monitor latch (`L | R | M | S` push-buttons on ISOL8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum IsolatorMonitor {
    /// Normal stereo output (no monitor button latched).
    #[default]
    Off,
    /// Isolate Left channel (`L`).
    Left,
    /// Isolate Right channel (`R`).
    Right,
    /// Isolate Mid mono sum (`M = 0.5*(L+R)`).
    Mid,
    /// Isolate Side difference (`S = 0.5*(L-R)`).
    Side,
}

/// 5-Band `24 dB/Oct` Linkwitz-Riley studio frequency isolator & monitor (`ISOL8`).
///
/// Splits audio into 5 phase-coherent bands (`LF`, `LMF`, `MF`, `HMF`, `HF`) across
/// 4 adjustable crossovers (`[200, 700, 2500, 8000] Hz` default). Using complementary
/// LR4 (`Q = 1/sqrt(2)` cascaded Butterworth) splits + 2nd-order Butterworth allpass
/// tree alignment, all 5 bands sum back with `0.00 dB` magnitude ripple across the entire
/// spectrum!
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BandIsolator {
    /// 4 crossover frequencies `[X1, X2, X3, X4]` in Hz (default `[200, 700, 2500, 8000]`).
    pub crossovers: [f32; 4],
    /// Per-band mute switches (`LF`, `LMF`, `MF`, `HMF`, `HF`).
    pub mute: [bool; 5],
    /// Per-band solo switches (`LF`, `LMF`, `MF`, `HMF`, `HF`). When any band is soloed,
    /// only soloed and non-muted bands pass.
    pub solo: [bool; 5],
    /// Filter channel mode (`STEREO`, `MID`, `SIDE`).
    pub ch_mode: IsolatorChannelMode,
    /// Output channel monitor (`Off`, `Left`, `Right`, `Mid`, `Side`).
    pub monitor: IsolatorMonitor,
    /// `FILTER BYPASS` switch: bypasses the 5-band mute/solo crossover.
    pub bypass: bool,
    /// Output trim in dB (`-24.0..+12.0`, default `0.0`).
    pub output_db: f32,
    /// `DIM` switch in the `ATTEN.` section.
    pub dim: bool,
    /// `DIM` attenuation in dB (`-48.0..0.0`, default `-18.0`).
    pub dim_db: f32,
}

impl Default for BandIsolator {
    fn default() -> Self {
        Self {
            crossovers: Self::DEFAULT_CROSSOVERS,
            mute: [false; 5],
            solo: [false; 5],
            ch_mode: IsolatorChannelMode::Stereo,
            monitor: IsolatorMonitor::Off,
            bypass: false,
            output_db: 0.0,
            dim: false,
            dim_db: -18.0,
        }
    }
}

impl BandIsolator {
    pub const BAND_NAMES: [&'static str; 5] = ["LF", "LMF", "MF", "HMF", "HF"];
    pub const DEFAULT_CROSSOVERS: [f32; 4] = [200.0, 700.0, 2500.0, 8000.0];

    /// Sanitized, strictly ordered crossover frequencies `[X1 < X2 < X3 < X4]` for `sample_rate`.
    #[inline]
    pub fn sanitized_crossovers(&self, sample_rate: f32) -> [f32; 4] {
        let nyq = (sample_rate.max(8000.0) * 0.46).min(19_500.0);
        let x1 = self.crossovers[0].clamp(25.0, (nyq * 0.25).min(1200.0));
        let x2 = self.crossovers[1].clamp((x1 * 1.25).max(80.0), (nyq * 0.50).min(4500.0));
        let x3 = self.crossovers[2].clamp((x2 * 1.25).max(250.0), (nyq * 0.78).min(12_000.0));
        let x4 = self.crossovers[3].clamp((x3 * 1.20).max(800.0), nyq);
        [x1, x2, x3, x4]
    }

    /// Per-band linear audibility (`1.0` = pass, `0.0` = cut) accounting for `bypass`, `solo`, and `mute`.
    #[inline]
    pub fn band_audibility(&self) -> [f32; 5] {
        if self.bypass {
            return [1.0; 5];
        }
        let any_solo = self.solo.iter().any(|&s| s);
        std::array::from_fn(|b| {
            let pass = if any_solo {
                self.solo[b] && !self.mute[b]
            } else {
                !self.mute[b]
            };
            if pass { 1.0 } else { 0.0 }
        })
    }

    /// True if any band is muted or soloed and `bypass` is off.
    #[inline]
    pub fn is_filter_active(&self) -> bool {
        !self.bypass && (self.mute.iter().any(|&m| m) || self.solo.iter().any(|&s| s))
    }

    /// True when the isolator alters audio or video away from unity passthrough.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.is_filter_active()
            || self.monitor != IsolatorMonitor::Off
            || self.dim
            || self.output_db.abs() > 1e-4
    }

    /// Combined linear output + dim gain.
    #[inline]
    pub fn target_output_gain(&self) -> f32 {
        let mut db = self.output_db.clamp(-48.0, 18.0);
        if self.dim {
            db += self.dim_db.clamp(-48.0, 0.0);
        }
        10.0f32.powf(db / 20.0)
    }
}

/// Stack-allocated per-channel 5-band LR4 (`24 dB/Oct`) phase-aligned filter tree.
#[derive(Debug, Clone)]
struct IsolatorChannel {
    lp1: [Biquad; 4],
    lp2: [Biquad; 4],
    hp1: [Biquad; 4],
    hp2: [Biquad; 4],
    /// Band 0 (`LF`) phase alignment through `X2, X3, X4`.
    ap_b0: [Biquad; 3],
    /// Band 1 (`LMF`) phase alignment through `X3, X4`.
    ap_b1: [Biquad; 2],
    /// Band 2 (`MF`) phase alignment through `X4`.
    ap_b2: Biquad,
}

impl IsolatorChannel {
    fn new(sr: f32, x: [f32; 4]) -> Self {
        let q = std::f32::consts::FRAC_1_SQRT_2;
        Self {
            lp1: std::array::from_fn(|k| Biquad::new(FilterKind::LowPass, sr, x[k], q)),
            lp2: std::array::from_fn(|k| Biquad::new(FilterKind::LowPass, sr, x[k], q)),
            hp1: std::array::from_fn(|k| Biquad::new(FilterKind::HighPass, sr, x[k], q)),
            hp2: std::array::from_fn(|k| Biquad::new(FilterKind::HighPass, sr, x[k], q)),
            ap_b0: [
                Biquad::allpass(sr, x[1], q),
                Biquad::allpass(sr, x[2], q),
                Biquad::allpass(sr, x[3], q),
            ],
            ap_b1: [
                Biquad::allpass(sr, x[2], q),
                Biquad::allpass(sr, x[3], q),
            ],
            ap_b2: Biquad::allpass(sr, x[3], q),
        }
    }

    fn update_coeffs(&mut self, sr: f32, x: [f32; 4]) {
        let q = std::f32::consts::FRAC_1_SQRT_2;
        for k in 0..4 {
            let lp = Biquad::new(FilterKind::LowPass, sr, x[k], q);
            let hp = Biquad::new(FilterKind::HighPass, sr, x[k], q);
            self.lp1[k].copy_coeffs_from(&lp);
            self.lp2[k].copy_coeffs_from(&lp);
            self.hp1[k].copy_coeffs_from(&hp);
            self.hp2[k].copy_coeffs_from(&hp);
        }
        let ap1 = Biquad::allpass(sr, x[1], q);
        let ap2 = Biquad::allpass(sr, x[2], q);
        let ap3 = Biquad::allpass(sr, x[3], q);
        self.ap_b0[0].copy_coeffs_from(&ap1);
        self.ap_b0[1].copy_coeffs_from(&ap2);
        self.ap_b0[2].copy_coeffs_from(&ap3);
        self.ap_b1[0].copy_coeffs_from(&ap2);
        self.ap_b1[1].copy_coeffs_from(&ap3);
        self.ap_b2.copy_coeffs_from(&ap3);
    }

    #[inline]
    fn split(&mut self, x: f32) -> [f32; 5] {
        // Stage 1 (X1): LF vs >=X1
        let b0_raw = self.lp2[0].process(self.lp1[0].process(x));
        let rem1 = self.hp2[0].process(self.hp1[0].process(x));
        // Stage 2 (X2): LMF vs >=X2
        let b1_raw = self.lp2[1].process(self.lp1[1].process(rem1));
        let rem2 = self.hp2[1].process(self.hp1[1].process(rem1));
        // Stage 3 (X3): MF vs >=X3
        let b2_raw = self.lp2[2].process(self.lp1[2].process(rem2));
        let rem3 = self.hp2[2].process(self.hp1[2].process(rem2));
        // Stage 4 (X4): HMF vs HF
        let b3 = self.lp2[3].process(self.lp1[3].process(rem3));
        let b4 = self.hp2[3].process(self.hp1[3].process(rem3));

        // Phase-align lower bands through subsequent crossover allpasses:
        let mut b0 = b0_raw;
        for ap in &mut self.ap_b0 {
            b0 = ap.process(b0);
        }
        let mut b1 = b1_raw;
        for ap in &mut self.ap_b1 {
            b1 = ap.process(b1);
        }
        let b2 = self.ap_b2.process(b2_raw);
        [b0, b1, b2, b3, b4]
    }
}

/// Allocation-free streaming state for [`BandIsolator`].
#[derive(Debug, Clone)]
pub struct IsolatorState {
    channels: [IsolatorChannel; 2],
    /// Companion 4-stage allpass (`AP(X1..X4)`) keeping the unfiltered Mid or Side
    /// channel 100% phase-aligned when `ch_mode` is `Mid` or `Side`.
    ref_ap: [Biquad; 4],
    cached_x: [f32; 4],
    cached_sr: u32,
    band_gains: [f32; 5],
    out_gain: f32,
    initialized: bool,
}

impl Default for IsolatorState {
    fn default() -> Self {
        Self::new(48_000)
    }
}

impl IsolatorState {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(8000);
        let sr_f = sr as f32;
        let x = BandIsolator::DEFAULT_CROSSOVERS;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        Self {
            channels: [IsolatorChannel::new(sr_f, x), IsolatorChannel::new(sr_f, x)],
            ref_ap: std::array::from_fn(|k| Biquad::allpass(sr_f, x[k], q)),
            cached_x: x,
            cached_sr: sr,
            band_gains: [1.0; 5],
            out_gain: 1.0,
            initialized: false,
        }
    }

    fn sync_params(&mut self, spec: &BandIsolator, sample_rate: u32) {
        let sr = sample_rate.max(8000);
        let sr_f = sr as f32;
        let x = spec.sanitized_crossovers(sr_f);
        if sr != self.cached_sr {
            let fresh = Self::new(sr);
            *self = fresh;
        }
        if (0..4).any(|k| (x[k] - self.cached_x[k]).abs() > 0.05) {
            self.channels[0].update_coeffs(sr_f, x);
            self.channels[1].update_coeffs(sr_f, x);
            let q = std::f32::consts::FRAC_1_SQRT_2;
            for k in 0..4 {
                let ap = Biquad::allpass(sr_f, x[k], q);
                self.ref_ap[k].copy_coeffs_from(&ap);
            }
            self.cached_x = x;
        }
    }

    /// Process a stereo buffer in-place (`0` heap allocations).
    pub fn process_block(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        spec: &BandIsolator,
        sample_rate: u32,
    ) {
        let n = left.len().min(right.len());
        if n == 0 {
            return;
        }
        self.sync_params(spec, sample_rate);
        let targets = spec.band_audibility();
        let target_out = spec.target_output_gain();
        if !self.initialized {
            self.band_gains = targets;
            self.out_gain = target_out;
            self.initialized = true;
        }
        // ~3 ms click-free smoothing for live MUTE / SOLO / BYPASS / DIM punches.
        let smooth_a = 1.0 - (-1.0 / (0.003 * sample_rate.max(8000) as f32)).exp();

        for i in 0..n {
            for b in 0..5 {
                self.band_gains[b] += smooth_a * (targets[b] - self.band_gains[b]);
            }
            self.out_gain += smooth_a * (target_out - self.out_gain);

            let in_l = left[i];
            let in_r = right[i];
            let (mut out_l, mut out_r) = match spec.ch_mode {
                IsolatorChannelMode::Stereo => {
                    let bl = self.channels[0].split(in_l);
                    let br = self.channels[1].split(in_r);
                    let mut sl = 0.0f32;
                    let mut sr_acc = 0.0f32;
                    for b in 0..5 {
                        let g = self.band_gains[b];
                        sl += bl[b] * g;
                        sr_acc += br[b] * g;
                    }
                    (sl, sr_acc)
                }
                IsolatorChannelMode::Mid => {
                    let mid = 0.5 * (in_l + in_r);
                    let side = 0.5 * (in_l - in_r);
                    let bm = self.channels[0].split(mid);
                    let mut m_out = 0.0f32;
                    for b in 0..5 {
                        m_out += bm[b] * self.band_gains[b];
                    }
                    let mut s_aligned = side;
                    for ap in &mut self.ref_ap {
                        s_aligned = ap.process(s_aligned);
                    }
                    (m_out + s_aligned, m_out - s_aligned)
                }
                IsolatorChannelMode::Side => {
                    let mid = 0.5 * (in_l + in_r);
                    let side = 0.5 * (in_l - in_r);
                    let bs = self.channels[1].split(side);
                    let mut s_out = 0.0f32;
                    for b in 0..5 {
                        s_out += bs[b] * self.band_gains[b];
                    }
                    let mut m_aligned = mid;
                    for ap in &mut self.ref_ap {
                        m_aligned = ap.process(m_aligned);
                    }
                    (m_aligned + s_out, m_aligned - s_out)
                }
            };

            match spec.monitor {
                IsolatorMonitor::Off => {}
                IsolatorMonitor::Left => {
                    out_r = out_l;
                }
                IsolatorMonitor::Right => {
                    out_l = out_r;
                }
                IsolatorMonitor::Mid => {
                    let m = 0.5 * (out_l + out_r);
                    out_l = m;
                    out_r = m;
                }
                IsolatorMonitor::Side => {
                    let s = 0.5 * (out_l - out_r);
                    out_l = s;
                    out_r = s;
                }
            }

            left[i] = out_l * self.out_gain;
            right[i] = out_r * self.out_gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, secs: f32, sr: u32) -> Vec<f32> {
        let n = (secs * sr as f32) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn lowpass_attenuates_high_freq() {
        let sr = 48000;
        let mut hi = sine(8000.0, 0.5, sr);
        let mut lo = sine(200.0, 0.5, sr);
        let mut f1 = Biquad::new(FilterKind::LowPass, sr as f32, 1000.0, 0.707);
        let mut f2 = Biquad::new(FilterKind::LowPass, sr as f32, 1000.0, 0.707);
        f1.process_buffer(&mut hi);
        f2.process_buffer(&mut lo);
        let hi_rms = rms(&hi[4800..]);
        let lo_rms = rms(&lo[4800..]);
        assert!(hi_rms < 0.05, "8k through 1k LP should be crushed, rms={hi_rms}");
        assert!(lo_rms > 0.6, "200Hz through 1k LP should pass, rms={lo_rms}");
    }

    #[test]
    fn bandpass_selects_band() {
        let sr = 48000;
        let mut inband = sine(1000.0, 0.5, sr);
        let mut below = sine(100.0, 0.5, sr);
        let mut above = sine(9000.0, 0.5, sr);
        for buf in [&mut inband, &mut below, &mut above] {
            let mut f = Biquad::new(FilterKind::BandPass, sr as f32, 1000.0, 2.0);
            f.process_buffer(buf);
        }
        assert!(rms(&inband[4800..]) > 0.5);
        assert!(rms(&below[4800..]) < 0.1);
        assert!(rms(&above[4800..]) < 0.1);
    }

    #[test]
    fn envelope_bounds_and_shape() {
        for shape in [0.1, 0.5, 1.0] {
            assert!(grain_env(0.0, shape) < 1e-3);
            assert!(grain_env(1.0, shape) < 1e-3);
            assert!((grain_env(0.5, shape) - 1.0).abs() < 1e-3 || shape == 1.0);
            for i in 0..=100 {
                let v = grain_env(i as f32 / 100.0, shape);
                assert!((0.0..=1.0001).contains(&v));
            }
        }
        // Full Hann peaks at 1 in the middle.
        assert!((grain_env(0.5, 1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn pitch_detection_sine() {
        let sr = 48000;
        for freq in [110.0, 220.0, 440.0, 587.33] {
            let s = sine(freq, 1.0, sr);
            let det = detect_pitch(&s, sr).expect("pitch detected");
            assert!(
                (det - freq).abs() / freq < 0.02,
                "expected ~{freq}, got {det}"
            );
        }
    }

    #[test]
    fn pitch_detection_rejects_silence() {
        let s = vec![0.0f32; 48000];
        assert!(detect_pitch(&s, 48000).is_none());
    }

    #[test]
    fn vibrato_pitch_detection_stabilizes_to_a440_semitone() {
        // Simulate a melismatic/vibrato vocal centered near G4 (391.995 Hz)
        // with asymmetric vibrato/portamento drifting ~35 cents flat on one half.
        let sr = 48000u32;
        let n = (1.5 * sr as f32) as usize;
        let mut ph = 0.0f32;
        let mut sig = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / sr as f32;
            let cents_mod = 28.0 * (2.0 * std::f32::consts::PI * 5.4 * t).sin()
                - if t > 0.75 { 24.0 } else { 0.0 };
            let hz = 391.9954 * 2.0f32.powf(cents_mod / 1200.0);
            ph = (ph + hz / sr as f32) % 1.0;
            sig.push((2.0 * std::f32::consts::PI * ph).sin() * 0.8);
        }
        let det = detect_pitch(&sig, sr).expect("detected vibrato pitch");
        let cents_err = 1200.0 * (det / 391.9954).log2();
        assert!(
            cents_err.abs() < 3.0,
            "vibrato pitch must snap to A=440 G4 (392 Hz), got {det:.2} Hz ({cents_err:.1}c)"
        );
    }

    #[test]
    fn peaking_eq_cuts_and_boosts_target_band() {
        let sr = 48000u32;
        let mut target = sine(320.0, 0.5, sr);
        let mut high = sine(4000.0, 0.5, sr);
        let band = [EqBand { freq: 320.0, q: 1.4, gain_db: -9.0 }];
        apply_eq_chain(&mut target, &band, sr as f32);
        apply_eq_chain(&mut high, &band, sr as f32);
        assert!(rms(&target[4000..]) < 0.28, "320 Hz cut by ~9 dB");
        assert!(rms(&high[4000..]) > 0.68, "4 kHz untouched");
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn band_isolator_reconstructs_flat_magnitude_and_isolates_bands_at_24db_oct() {
        let sr = 48_000u32;
        // 1. Flat magnitude reconstruction across 5 bands in Stereo, Mid, and Side modes!
        for mode in [
            IsolatorChannelMode::Stereo,
            IsolatorChannelMode::Mid,
            IsolatorChannelMode::Side,
        ] {
            let iso = BandIsolator { ch_mode: mode, ..Default::default() };
            for &freq in &[55.0f32, 200.0, 420.0, 700.0, 1400.0, 2500.0, 5200.0, 8000.0, 13_500.0] {
                let mut l = sine(freq, 0.30, sr);
                let mut r: Vec<f32> = l.iter().map(|&x| x * 0.65).collect();
                // Measure over an exact integer number of wave cycles after filter settling
                // so phase rotation across crossovers doesn't skew fractional-cycle window power!
                let settle = 4800usize;
                let period_samps = sr as f32 / freq;
                let n_cycles = ((l.len() - settle) as f32 / period_samps).floor().max(4.0);
                let win_len = (n_cycles * period_samps).round() as usize;
                let ref_rms_l = rms(&l[settle..settle + win_len]);
                let ref_rms_r = rms(&r[settle..settle + win_len]);
                let mut st = IsolatorState::new(sr);
                st.process_block(&mut l, &mut r, &iso, sr);
                let out_rms_l = rms(&l[settle..settle + win_len]);
                let out_rms_r = rms(&r[settle..settle + win_len]);
                let db_err_l = 20.0 * (out_rms_l / ref_rms_l).log10();
                let db_err_r = 20.0 * (out_rms_r / ref_rms_r).log10();
                assert!(
                    db_err_l.abs() < 0.02 && db_err_r.abs() < 0.02,
                    "LR4 5-band sum must be flat within 0.02 dB at {freq} Hz ({mode:?}): L={db_err_l:.4}dB R={db_err_r:.4}dB"
                );
            }
        }

        // 2. Soloing LF (<200 Hz) passes 60 Hz cleanly and rejects 1000 Hz by >26 dB (24 dB/Oct).
        let mut lf_solo = BandIsolator::default();
        lf_solo.solo[0] = true;
        let mut sub_l = sine(60.0, 0.25, sr);
        let mut sub_r = sub_l.clone();
        IsolatorState::new(sr).process_block(&mut sub_l, &mut sub_r, &lf_solo, sr);
        assert!(rms(&sub_l[4000..]) > 0.68, "LF SOLO passes 60 Hz sub cleanly");

        let mut mid_l = sine(1000.0, 0.25, sr);
        let mut mid_r = mid_l.clone();
        IsolatorState::new(sr).process_block(&mut mid_l, &mut mid_r, &lf_solo, sr);
        assert!(rms(&mid_l[4000..]) < 0.008, "LF SOLO rejects 1 kHz at 24 dB/oct: {}", rms(&mid_l[4000..]));

        // 3. LMF + MF + HMF SOLO (exact ISOL8 panel state: 200..8000 Hz vocal/radio band)
        //    plus Mute-overrides-Solo, Bypass, Dim (-18 dB), and Monitor M/S.
        let mut radio = BandIsolator::default();
        radio.solo = [false, true, true, true, false];
        let mut vox_l = sine(1200.0, 0.25, sr);
        let mut vox_r = vox_l.clone();
        IsolatorState::new(sr).process_block(&mut vox_l, &mut vox_r, &radio, sr);
        assert!(rms(&vox_l[4000..]) > 0.68, "LMF+MF+HMF SOLO passes 1.2 kHz");

        // Muting MF while MF is soloed cuts 1.2 kHz; DIM attenuates by ~18 dB (0.1259x).
        radio.dim = true;
        let mut dim_l = sine(1200.0, 0.25, sr);
        let mut dim_r = dim_l.clone();
        IsolatorState::new(sr).process_block(&mut dim_l, &mut dim_r, &radio, sr);
        let ratio = rms(&dim_l[4000..]) / rms(&vox_l[4000..]);
        assert!((ratio - 0.1259).abs() < 0.01, "DIM -18.0 dB scales by ~0.1259, got {ratio:.4}");

        // Monitor Mid vs Side on hard-left input (L = sine, R = 0 -> M = 0.5*L, S = 0.5*L).
        let mut mon_m = BandIsolator { monitor: IsolatorMonitor::Mid, ..Default::default() };
        let mut ml = sine(440.0, 0.2, sr);
        let mut mr = vec![0.0f32; ml.len()];
        IsolatorState::new(sr).process_block(&mut ml, &mut mr, &mon_m, sr);
        assert!((rms(&ml[3000..]) - 0.3535).abs() < 0.01 && (rms(&mr[3000..]) - 0.3535).abs() < 0.01);
        mon_m.monitor = IsolatorMonitor::Right;
        let mut rl = sine(440.0, 0.2, sr);
        let mut rr = vec![0.0f32; rl.len()];
        IsolatorState::new(sr).process_block(&mut rl, &mut rr, &mon_m, sr);
        assert!(rms(&rl[3000..]) < 1e-6 && rms(&rr[3000..]) < 1e-6, "Monitor R on L-only signal is silent");
    }
}
