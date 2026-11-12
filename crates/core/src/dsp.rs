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
}
