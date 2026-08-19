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

/// Estimate the fundamental frequency of a buffer via normalized
/// autocorrelation. Returns None when the signal is too quiet or aperiodic.
pub fn detect_pitch(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let sr = sample_rate as f32;
    // Analyze up to ~0.75s from the middle of the buffer.
    let win = (sr * 0.25) as usize;
    let max_lag = (sr / 50.0) as usize; // down to 50 Hz
    let min_lag = (sr / 1200.0).max(2.0) as usize; // up to 1200 Hz
    if samples.len() < win + max_lag + 1 || min_lag >= max_lag {
        return None;
    }
    let start = (samples.len() - win - max_lag) / 2;
    let x = &samples[start..start + win + max_lag];

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
    let mut best_lag = match (min_lag..=max_lag).find(|&l| corrs[l] >= threshold) {
        Some(l) => l,
        None => return None,
    };
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

    Some(sr / lag)
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
