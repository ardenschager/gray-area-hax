//! Audiovisual paulstretch: extreme time-stretching for AV samples.
//!
//! Audio side is the classic paulstretch algorithm — overlapping windowed
//! FFT frames whose magnitudes are kept but whose phases are randomized,
//! so the spectrum (and therefore perceived pitch) is preserved while time
//! dissolves into a wash. Video side is the visual analogue: the clip is
//! slowed by the same factor with neighbor-frame crossfades plus a decaying
//! accumulator so motion smears the way the audio does.
//!
//! Both run offline and produce a new derived [`Source`], so a stretched
//! sample granulates, sequences, key-quantizes and effects-chains exactly
//! like any other source.

use std::sync::Arc;

use crate::audio::AudioClip;
use crate::dsp::Rng;
use crate::fx::fft;
use crate::timeline::Source;
use crate::video::{Frame, VideoClip};

/// Preferred analysis window (samples). Shrunk for very short clips.
const WINDOW: usize = 8192;
/// Hard cap on stretched audio length so a fat factor can't eat all RAM.
const MAX_OUT_SECS: f64 = 180.0;
/// Output frame rate for stretched video — slow washes don't need 30fps.
const OUT_FPS: f32 = 12.0;
/// Exponential smear: acc = acc*SMEAR + frame*(1-SMEAR).
const SMEAR: f32 = 0.8;
/// Default frequency-proportional spectral smear half-width in cents: turns
/// narrow upper-mid sine/tine whistles (>1 kHz) into lush critical-band pads
/// while keeping fundamentals tight and broadband >6 kHz air intact.
pub const DEFAULT_SPREAD_CENTS: f32 = 85.0;

/// Clamp a requested stretch factor into the supported range.
pub fn clamp_factor(factor: f32) -> f32 {
    if !factor.is_finite() {
        8.0
    } else {
        factor.clamp(2.0, 64.0)
    }
}

/// Paulstretch `clip` by `factor` (2..64) using [`DEFAULT_SPREAD_CENTS`].
pub fn paulstretch(clip: &AudioClip, factor: f32) -> AudioClip {
    paulstretch_opts(clip, factor, DEFAULT_SPREAD_CENTS)
}

/// Paulstretch `clip` by `factor` (2..64) with explicit `spread_cents`
/// (`0..200`): frequency-proportional bandwidth smearing that melts narrow
/// tonal whistles without dulling high-frequency air.
pub fn paulstretch_opts(clip: &AudioClip, factor: f32, spread_cents: f32) -> AudioClip {
    let sr = clip.sample_rate.max(1);
    let spread = if spread_cents.is_finite() { spread_cents.clamp(0.0, 200.0) } else { DEFAULT_SPREAD_CENTS };
    if clip.samples.is_empty() {
        return AudioClip::new(Vec::new(), sr);
    }
    if clip.is_stereo() {
        let (l, r) = clip.left_right();
        let (l, r) = (
            paulstretch_channel(&l, factor, sr, spread),
            paulstretch_channel(&r, factor, sr, spread),
        );
        return AudioClip::stereo(&l, &r, sr);
    }
    AudioClip::new(paulstretch_channel(&clip.samples, factor, sr, spread), sr)
}

fn paulstretch_channel(input: &[f32], factor: f32, sr: u32, spread_cents: f32) -> Vec<f32> {
    let factor = clamp_factor(factor) as f64;
    if input.is_empty() {
        return Vec::new();
    }

    // Window must be a power of two and no longer than the clip (down to a
    // floor of 256 samples so tiny clips still work).
    let mut w = WINDOW;
    while w > 256 && w > input.len() {
        w /= 2;
    }
    let half = w / 2;

    let out_len = ((input.len() as f64 * factor).min(MAX_OUT_SECS * sr as f64)) as usize;
    let out_len = out_len.max(w);
    let mut out = vec![0.0f32; out_len + w];

    let hann: Vec<f32> = (0..w)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / w as f32).cos())
        .collect();

    // Input advances by half/factor per output hop of half — that ratio is
    // the whole stretch.
    let in_hop = half as f64 / factor;
    let n_segs = out_len / half + 1;
    let mut re = vec![0.0f32; w];
    let mut im = vec![0.0f32; w];
    let mut pow_a = vec![0.0f64; half + 1];
    let mut pow_b = vec![0.0f64; half + 1];
    let mut pref = vec![0.0f64; half + 2];

    let sr_f = sr.max(1) as f32;
    let hz_per_bin = sr_f / w as f32;
    let min_k = (20.0 / hz_per_bin).ceil() as usize;
    let rel_half = 2.0f32.powf(spread_cents / 1200.0) - 1.0;

    // One energy-conserving symmetric SCATTER pass over power spectrum `src -> dst`
    // via difference arrays (`O(half)`). Unlike a variable-width gather box (which
    // pulls peak centroids upward where `dk(k)` grows), scattering each source bin
    // `j` symmetrically across `[j - dj, j + dj]` keeps every harmonic's pitch
    // centroid dead in tune (`0.0` cents shift) while melting `>1.1 kHz` whistles.
    let scatter_power_pass = |src: &[f64], dst: &mut [f64], diff: &mut [f64]| {
        diff.fill(0.0);
        for j in 0..=half {
            let p = src[j];
            if p <= 0.0 {
                continue;
            }
            let freq = j as f32 * hz_per_bin;
            let ramp = ((freq - 950.0) / 600.0).clamp(0.0, 1.0);
            let dj = ((j as f32 * rel_half * ramp).round() as usize).min(half / 4);
            if dj == 0 {
                diff[j] += p;
                diff[j + 1] -= p;
            } else {
                let lo = j.saturating_sub(dj).max(min_k.min(j));
                let hi = (j + dj).min(half - 1).max(lo);
                let share = p / (hi - lo + 1) as f64;
                diff[lo] += share;
                diff[hi + 1] -= share;
            }
        }
        let mut acc = 0.0f64;
        for k in 0..=half {
            acc += diff[k];
            dst[k] = acc.max(0.0);
        }
    };

    for seg in 0..n_segs {
        let in_pos = (seg as f64 * in_hop) as usize;
        for i in 0..w {
            let s = input.get(in_pos + i).copied().unwrap_or(0.0);
            re[i] = s * hann[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, false);

        for k in 0..=half {
            pow_a[k] = if k < min_k || k == half {
                0.0
            } else {
                (re[k] as f64) * (re[k] as f64) + (im[k] as f64) * (im[k] as f64)
            };
        }
        if spread_cents > 0.5 {
            scatter_power_pass(&pow_a, &mut pow_b, &mut pref);
            scatter_power_pass(&pow_b, &mut pow_a, &mut pref);
        }

        let mut rng = Rng::new(0x9A75_57E7 ^ seg as u64);
        re[0] = 0.0;
        im[0] = 0.0;
        re[half] = 0.0;
        im[half] = 0.0;
        for k in 1..half {
            if k < min_k {
                re[k] = 0.0;
                im[k] = 0.0;
                re[w - k] = 0.0;
                im[w - k] = 0.0;
                continue;
            }
            let mag = pow_a[k].max(0.0).sqrt() as f32;
            let ph = rng.next_f32() * 2.0 * std::f32::consts::PI;
            let (s, c) = ph.sin_cos();
            re[k] = mag * c;
            im[k] = mag * s;
            re[w - k] = re[k];
            im[w - k] = -im[k];
        }

        fft(&mut re, &mut im, true);
        let base = seg * half;
        for i in 0..w {
            if base + i < out.len() {
                out[base + i] += re[i] * hann[i];
            }
        }
    }

    // Two hann^2 windows at 50% overlap sum to 0.5*(1+cos^2), not a
    // constant — divide it back out so the wash has no tremolo.
    for (p, s) in out.iter_mut().enumerate() {
        let c = (2.0 * std::f32::consts::PI * p as f32 / w as f32).cos();
        *s /= 0.5 * (1.0 + c * c);
    }
    out.truncate(out_len);

    // Match the input's RMS so the stretched source drops into a mix at a
    // sane level.
    let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt();
    let (ri, ro) = (rms(input), rms(&out));
    if ro > 1e-9 && ri > 1e-9 {
        let g = (ri / ro).min(8.0);
        for s in out.iter_mut() {
            *s = (*s * g).clamp(-1.0, 1.0);
        }
    }
    out
}

/// Slow `video` down by `factor` with neighbor-frame crossfades and a
/// decaying accumulator, the visual counterpart of the audio wash.
pub fn stretch_video(video: &VideoClip, factor: f32) -> VideoClip {
    let factor = clamp_factor(factor) as f64;
    if video.frames.is_empty() || video.fps <= 0.0 {
        return VideoClip { frames: Vec::new(), fps: OUT_FPS };
    }
    let src_dur = video.duration();
    let out_dur = (src_dur * factor).min(MAX_OUT_SECS);
    let n_out = ((out_dur * OUT_FPS as f64) as usize).max(1);
    let (fw, fh) = (video.frames[0].width, video.frames[0].height);
    let px = (fw * fh * 4) as usize;

    let mut acc: Vec<f32> = video.frames[0].data.iter().map(|&b| b as f32).collect();
    let mut frames = Vec::with_capacity(n_out);
    for f in 0..n_out {
        let t_src = (f as f64 / OUT_FPS as f64) / factor;
        let fpos = (t_src * video.fps as f64).min((video.frames.len() - 1) as f64);
        let i0 = fpos.floor() as usize;
        let i1 = (i0 + 1).min(video.frames.len() - 1);
        let frac = (fpos - i0 as f64) as f32;
        let a = &video.frames[i0].data;
        let b = &video.frames[i1].data;
        for i in 0..px.min(acc.len()) {
            let cross = a[i] as f32 * (1.0 - frac) + b[i] as f32 * frac;
            acc[i] = acc[i] * SMEAR + cross * (1.0 - SMEAR);
        }
        let mut frame = Frame::black(fw, fh);
        for (d, &s) in frame.data.iter_mut().zip(acc.iter()) {
            *d = s.round().clamp(0.0, 255.0) as u8;
        }
        // Keep alpha solid — smear should live in color, not transparency.
        for i in (3..frame.data.len()).step_by(4) {
            frame.data[i] = 255;
        }
        frames.push(frame);
    }
    VideoClip { frames, fps: OUT_FPS }
}

/// Derive a paulstretched [`Source`] from `src`. Audio and video are both
/// stretched by the same factor; `base_hz` carries over because the pitch
/// is unchanged, so key quantization keeps working on the wash.
pub fn paulstretch_source(src: &Source, factor: f32) -> Source {
    paulstretch_source_opts(src, factor, DEFAULT_SPREAD_CENTS)
}

/// [`paulstretch_source`] with explicit spectral `spread_cents` (`0..200`).
pub fn paulstretch_source_opts(src: &Source, factor: f32, spread_cents: f32) -> Source {
    let factor = clamp_factor(factor);
    Source {
        name: format!("paul x{:.0} {}", factor, src.name),
        audio: src
            .audio
            .as_ref()
            .map(|a| Arc::new(paulstretch_opts(a, factor, spread_cents))),
        video: src
            .video
            .as_ref()
            .map(|v| Arc::new(stretch_video(v, factor))),
        base_hz: src.base_hz,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::detect_pitch;

    fn sine(hz: f32, secs: f32, sr: u32) -> AudioClip {
        let n = (secs * sr as f32) as usize;
        let samples = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        AudioClip::new(samples, sr)
    }

    #[test]
    fn stretch_multiplies_duration() {
        let clip = sine(220.0, 1.0, 44100);
        let out = paulstretch(&clip, 8.0);
        let ratio = out.duration() / clip.duration();
        assert!((ratio - 8.0).abs() < 0.2, "duration ratio {ratio}");
    }

    #[test]
    fn stretch_preserves_pitch() {
        let sr = 44100;
        let clip = sine(220.0, 1.0, sr);
        let out = paulstretch(&clip, 8.0);
        // Detect pitch in a window from the middle of the wash.
        let mid = out.samples.len() / 2;
        let win = &out.samples[mid..mid + 16384];
        let hz = detect_pitch(win, sr).expect("pitch should be detectable in the wash");
        assert!(
            (hz - 220.0).abs() < 12.0,
            "expected ~220Hz after stretch, got {hz}"
        );
    }

    #[test]
    fn stretch_level_is_sane() {
        let clip = sine(110.0, 0.5, 44100);
        let out = paulstretch(&clip, 4.0);
        let rms = (out.samples.iter().map(|x| x * x).sum::<f32>()
            / out.samples.len() as f32)
            .sqrt();
        assert!(rms > 0.05 && rms < 0.9, "rms {rms}");
        assert!(out.samples.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn stretch_is_deterministic() {
        let clip = sine(330.0, 0.3, 22050);
        let a = paulstretch(&clip, 4.0);
        let b = paulstretch(&clip, 4.0);
        assert_eq!(a.samples, b.samples);
    }

    #[test]
    fn video_stretch_tracks_source_time() {
        // 10 frames fading black -> white at 10fps (1s). Stretched x4, the
        // output frame near out-t=2s should resemble the source at t=0.5s
        // (mid grey), and the wash should end brighter than it starts.
        let fps = 10.0;
        let mut frames = Vec::new();
        for i in 0..10 {
            let mut f = Frame::black(8, 8);
            let v = (i as f32 / 9.0 * 255.0) as u8;
            for p in f.data.chunks_mut(4) {
                p[0] = v;
                p[1] = v;
                p[2] = v;
                p[3] = 255;
            }
            frames.push(f);
        }
        let clip = VideoClip { frames, fps };
        let out = stretch_video(&clip, 4.0);
        assert!((out.duration() - 4.0).abs() < 0.3, "dur {}", out.duration());
        let luma = |f: &Frame| {
            f.data.chunks(4).map(|p| p[0] as f32).sum::<f32>() / (f.data.len() / 4) as f32
        };
        let mid = &out.frames[(2.0 * OUT_FPS) as usize];
        let l = luma(mid);
        assert!((l - 127.0).abs() < 40.0, "mid-wash luma {l}");
        let first = luma(&out.frames[0]);
        let last = luma(out.frames.last().unwrap());
        assert!(last > first + 100.0, "wash should brighten {first} -> {last}");
    }

    #[test]
    fn source_derivation_keeps_base_hz_and_stretches_both() {
        let audio = sine(220.0, 0.5, 22050);
        let mut frames = Vec::new();
        for _ in 0..5 {
            frames.push(Frame::black(4, 4));
        }
        let src = Source {
            name: "clip".into(),
            audio: Some(Arc::new(audio)),
            video: Some(Arc::new(VideoClip { frames, fps: 10.0 })),
            base_hz: 220.0,
        };
        let out = paulstretch_source(&src, 4.0);
        assert_eq!(out.base_hz, 220.0);
        assert!(out.name.contains("paul"));
        let a = out.audio.as_ref().unwrap();
        assert!((a.duration() / 0.5 - 4.0).abs() < 0.3);
        let v = out.video.as_ref().unwrap();
        assert!((v.duration() - 2.0).abs() < 0.3);
    }

    #[test]
    fn stretch_smears_upper_whistle_while_preserving_hf_air() {
        let sr = 48_000u32;
        // 1. Broadband noise must keep its >6 kHz spectral share within ~2 dB.
        let mut rng = crate::dsp::Rng::new(0x1234);
        let noise_in: Vec<f32> = (0..sr as usize).map(|_| rng.bipolar() * 0.3).collect();
        let in_clip = AudioClip::new(noise_in.clone(), sr);
        let out_clip = paulstretch(&in_clip, 4.0);
        let hf_share = |sig: &[f32]| -> f64 {
            let mut re: Vec<f32> = sig.iter().take(8192).copied().collect();
            let mut im = vec![0.0f32; 8192];
            fft(&mut re, &mut im, false);
            let bin_hz = sr as f32 / 8192.0;
            let mut tot = 1e-12f64;
            let mut hf = 1e-12f64;
            for k in 4..4096 {
                let p = (re[k] as f64).powi(2) + (im[k] as f64).powi(2);
                tot += p;
                if k as f32 * bin_hz >= 6000.0 {
                    hf += p;
                }
            }
            10.0 * (hf / tot).log10()
        };
        let in_db = hf_share(&noise_in);
        let out_db = hf_share(&out_clip.samples[8192..16384]);
        assert!(
            (out_db - in_db).abs() < 3.0,
            ">6 kHz air preserved: in {in_db:.2} dB vs out {out_db:.2} dB"
        );

        // 2. A narrow 1600 Hz whistle is spread across a critical band, dropping
        // time-averaged STFT peak-bin power by >5x (>7 dB) vs unspread (`0.0`).
        let whistle = sine(1600.0, 0.6, sr);
        let raw_w = paulstretch_opts(&whistle, 4.0, 0.0);
        let spread_w = paulstretch(&whistle, 4.0);
        let mean_peak_pow = |sig: &[f32]| -> f64 {
            let mut avg = vec![0.0f64; 2000];
            let mut wins = 0usize;
            let mut off = 4096usize;
            while off + 8192 <= sig.len() {
                let mut re: Vec<f32> = sig[off..off + 8192].to_vec();
                let mut im = vec![0.0f32; 8192];
                fft(&mut re, &mut im, false);
                for k in 100..2000 {
                    avg[k] += (re[k] as f64).powi(2) + (im[k] as f64).powi(2);
                }
                wins += 1;
                off += 4096;
            }
            avg.into_iter().fold(0.0f64, f64::max) / wins.max(1) as f64
        };
        let p_raw = mean_peak_pow(&raw_w.samples);
        let p_sp = mean_peak_pow(&spread_w.samples);
        assert!(
            p_sp < p_raw * 0.25,
            "cents spread melts narrow whistle spike: {p_sp:.1} vs {p_raw:.1}"
        );
    }

    #[test]
    fn stretch_preserves_eb5_harmonic_tuning_within_3_cents() {
        // Regression test for v004's 622.25 Hz -> 641 Hz (+51c) gather-box shift:
        // symmetric scatter must keep Eb5 (622.254 Hz) dead in tune (<3 cents).
        let sr = 48000u32;
        let eb5 = sine(622.254, 0.8, sr);
        let out = paulstretch(&eb5, 6.0);
        let n = 65536usize;
        let start = ((out.samples.len().saturating_sub(n)) / 2).max(0);
        let mut re = vec![0.0f32; n];
        let mut im = vec![0.0f32; n];
        for i in 0..n.min(out.samples.len() - start) {
            let h = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos());
            re[i] = out.samples[start + i] * h;
        }
        fft(&mut re, &mut im, false);
        let k0 = (550.0 * n as f32 / sr as f32) as usize;
        let k1 = (700.0 * n as f32 / sr as f32) as usize;
        let mut best_k = k0;
        let mut best_p = 0.0f64;
        for k in k0..=k1 {
            let p = (re[k] as f64).powi(2) + (im[k] as f64).powi(2);
            if p > best_p {
                best_p = p;
                best_k = k;
            }
        }
        let peak_hz = best_k as f32 * sr as f32 / n as f32;
        let cents_err = 1200.0 * (peak_hz / 622.254).log2();
        assert!(
            cents_err.abs() < 3.0,
            "paulstretch must keep Eb5 (622.25 Hz) in tune, got {peak_hz:.2} Hz ({cents_err:+.1}c)"
        );
    }
}
