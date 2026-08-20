//! Pure synth engines rendered as Sources — so synths get everything
//! samples get for free: granular clips, snippets, sequencer rows, MIDI
//! pads, key quantization, effect chains, transforms.
//!
//! Their video is generated FROM their own audio: an oscilloscope trace
//! whose hue follows the spectral centroid, using the SAME hue<->frequency
//! mapping as the Tint effect (low = red, high = violet). The visual IS
//! the signal's frequency content.

use crate::audio::AudioClip;
use crate::dsp::Rng;
use crate::timeline::Source;
use crate::video::{hsv_to_rgb, Frame, VideoClip};
use std::sync::Arc;

/// The shared synesthetic axis: hue (degrees) <-> frequency (Hz).
/// 0 deg (red) = 80 Hz rumble up to 330 deg (violet) ~ 8 kHz. The axis
/// stops short of 360 so high frequencies never wrap back to red.
pub const HUE_SPAN: f32 = 330.0;

pub fn hue_to_freq(hue: f32) -> f32 {
    80.0 * 2.0f32.powf(hue.clamp(0.0, HUE_SPAN) / HUE_SPAN * 6.64)
}

/// Inverse of [`hue_to_freq`].
pub fn freq_to_hue(freq: f32) -> f32 {
    ((freq.max(1.0) / 80.0).log2() / 6.64 * HUE_SPAN).clamp(0.0, HUE_SPAN)
}

/// Two-operator FM: carrier at `base_hz`, modulator at `base_hz * ratio`,
/// modulation `index` easing off over the tone for movement.
pub fn render_fm(base_hz: f32, ratio: f32, index: f32, secs: f32, sample_rate: u32) -> AudioClip {
    let n = (secs * sample_rate as f32) as usize;
    let sr = sample_rate as f32;
    let mut samples = Vec::with_capacity(n);
    let (mut phase_c, mut phase_m) = (0.0f32, 0.0f32);
    let w_c = 2.0 * std::f32::consts::PI * base_hz / sr;
    let w_m = 2.0 * std::f32::consts::PI * base_hz * ratio.max(0.01) / sr;
    for i in 0..n {
        let t = i as f32 / sr;
        // Index breathes: bright attack settling into the sustain.
        let idx = index * (0.35 + 0.65 * (-t * 2.0).exp());
        let env = crate::dsp::grain_env(i as f32 / n.max(1) as f32, 0.08);
        samples.push(0.7 * env * (phase_c + idx * phase_m.sin()).sin());
        phase_c += w_c;
        phase_m += w_m;
    }
    AudioClip::new(samples, sample_rate)
}

/// Colored noise: `color` 0 = white, 0.5 = pink-ish, 1 = brown.
pub fn render_noise(color: f32, secs: f32, sample_rate: u32) -> AudioClip {
    let n = (secs * sample_rate as f32) as usize;
    let color = color.clamp(0.0, 1.0);
    let mut rng = Rng::new(0xA0D10 ^ ((color * 1000.0) as u64));
    let mut samples = Vec::with_capacity(n);
    // Paul Kellet-style pink approximation + leaky-integrated brown.
    let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
    let mut brown = 0.0f32;
    for i in 0..n {
        let white = rng.bipolar();
        b0 = 0.99765 * b0 + white * 0.0990460;
        b1 = 0.96300 * b1 + white * 0.2965164;
        b2 = 0.57000 * b2 + white * 1.0526913;
        let pink = (b0 + b1 + b2 + white * 0.1848) * 0.25;
        brown = (brown + white * 0.02).clamp(-1.0, 1.0) * 0.997;
        let v = if color < 0.5 {
            let f = color * 2.0;
            white * (1.0 - f) * 0.5 + pink * f
        } else {
            let f = (color - 0.5) * 2.0;
            pink * (1.0 - f) + brown * f * 6.0
        };
        let env = crate::dsp::grain_env(i as f32 / n.max(1) as f32, 0.06);
        samples.push(v * env * 0.8);
    }
    AudioClip::new(samples, sample_rate)
}

/// Spectral centroid (Hz) of a window via the shared FFT.
fn centroid_hz(window: &[f32], sample_rate: f32) -> f32 {
    const N: usize = 512;
    let mut re = [0.0f32; N];
    let mut im = [0.0f32; N];
    // First N samples, Hann-windowed — bin k maps to k * sr / N exactly.
    for (i, r) in re.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos();
        *r = window.get(i).copied().unwrap_or(0.0) * w;
    }
    crate::fx::fft(&mut re, &mut im, false);
    let mut num = 0.0f32;
    let mut den = 0.0f32;
    for k in 1..N / 2 {
        let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
        num += mag * (k as f32 * sample_rate / N as f32);
        den += mag;
    }
    if den > 1e-6 {
        num / den
    } else {
        0.0
    }
}

/// Audio-reactive video: an oscilloscope trace of the actual waveform,
/// hue = spectral centroid (the hue<->freq axis), glow = loudness.
pub fn scope_video(audio: &AudioClip, width: u32, height: u32, fps: f32) -> VideoClip {
    let n_frames = ((audio.duration() * fps as f64).ceil() as usize).max(1);
    let sr = audio.sample_rate as f32;
    let win_len = (sr / fps).max(2.0) as usize;
    let mut frames = Vec::with_capacity(n_frames);

    for f in 0..n_frames {
        let start = (f * win_len).min(audio.samples.len().saturating_sub(1));
        let end = (start + win_len).min(audio.samples.len());
        let window = &audio.samples[start..end.max(start + 1)];

        let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
        let hue = freq_to_hue(centroid_hz(window, sr));
        let bright = (0.25 + rms * 2.2).min(1.0);
        let (br, bg, bb) = hsv_to_rgb(hue, 0.85, bright * 0.28);
        let (tr, tg, tb) = hsv_to_rgb(hue, 0.55, bright);

        let mut frame = Frame::black(width, height);
        // Dim spectral wash as the background.
        for px in frame.data.chunks_exact_mut(4) {
            px[0] = br;
            px[1] = bg;
            px[2] = bb;
        }
        // The trace.
        let half = height as f32 / 2.0;
        let glow = (height as f32 * 0.06).max(1.5);
        for x in 0..width {
            let idx = (x as usize * (window.len() - 1)) / width.max(1) as usize;
            let amp = window[idx.min(window.len() - 1)];
            let yc = half - amp * half * 0.8;
            let y0 = ((yc - glow * 3.0).floor() as i32).max(0);
            let y1 = ((yc + glow * 3.0).ceil() as i32).min(height as i32 - 1);
            for y in y0..=y1 {
                let d = (y as f32 - yc).abs() / glow;
                let a = (-d * d).exp();
                if a < 0.02 {
                    continue;
                }
                let px = frame.get(x, y as u32);
                frame.put(
                    x,
                    y as u32,
                    [
                        (px[0] as f32 + tr as f32 * a).min(255.0) as u8,
                        (px[1] as f32 + tg as f32 * a).min(255.0) as u8,
                        (px[2] as f32 + tb as f32 * a).min(255.0) as u8,
                        255,
                    ],
                );
            }
        }
        frames.push(frame);
    }
    VideoClip { frames, fps }
}

/// A complete FM synth source (audio + corresponding scope video).
pub fn fm_source(base_hz: f32, ratio: f32, index: f32, secs: f32, sample_rate: u32) -> Source {
    let audio = render_fm(base_hz, ratio, index, secs, sample_rate);
    let video = scope_video(&audio, 240, 136, 12.0);
    Source {
        name: format!("fm {base_hz:.0}Hz x{ratio:.2} i{index:.1}"),
        audio: Some(Arc::new(audio)),
        video: Some(Arc::new(video)),
        base_hz,
    }
}

/// A complete noise synth source (audio + corresponding scope video).
pub fn noise_source(color: f32, secs: f32, sample_rate: u32) -> Source {
    let audio = render_noise(color, secs, sample_rate);
    let video = scope_video(&audio, 240, 136, 12.0);
    Source {
        name: format!("noise c{color:.2}"),
        audio: Some(Arc::new(audio)),
        video: Some(Arc::new(video)),
        base_hz: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_freq_mapping_roundtrips() {
        for hz in [80.0, 440.0, 2000.0, 7000.0] {
            let hue = freq_to_hue(hz);
            assert!((hue_to_freq(hue) - hz).abs() / hz < 0.01);
        }
        // Red is low, far around the wheel is high.
        assert!(hue_to_freq(0.0) < 100.0);
        assert!(hue_to_freq(300.0) > 2000.0);
    }

    #[test]
    fn fm_holds_its_fundamental() {
        let sr = 48000;
        let clip = render_fm(220.0, 2.0, 1.2, 1.0, sr);
        let pitch = crate::dsp::detect_pitch(&clip.samples, sr).expect("pitch");
        assert!((pitch - 220.0).abs() < 8.0, "fm fundamental: {pitch}");
        // More index = more sidebands = brighter (higher centroid).
        let dull = render_fm(220.0, 2.0, 0.2, 1.0, sr);
        let mid = |c: &AudioClip| centroid_hz(&c.samples[8000..8000 + 2048], sr as f32);
        assert!(mid(&clip) > mid(&dull) * 1.2, "index brightens the spectrum");
    }

    #[test]
    fn noise_color_tilts_the_spectrum() {
        let sr = 48000;
        let white = render_noise(0.0, 1.0, sr);
        let brown = render_noise(1.0, 1.0, sr);
        // High-frequency content ~ mean squared sample-to-sample difference.
        let hf = |c: &AudioClip| -> f32 {
            let s = &c.samples[4800..43200];
            let rms: f32 = (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
            let d: f32 = s.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>()
                / (s.len() - 1) as f32;
            d.sqrt() / rms.max(1e-9)
        };
        assert!(
            hf(&white) > hf(&brown) * 3.0,
            "white {} vs brown {}",
            hf(&white),
            hf(&brown)
        );
    }

    #[test]
    fn scope_video_reflects_the_signal() {
        let sr = 48000;
        let low = render_fm(100.0, 1.0, 0.3, 0.5, sr);
        let noise = render_noise(0.0, 0.5, sr);
        let v_low = scope_video(&low, 64, 36, 12.0);
        let v_noise = scope_video(&noise, 64, 36, 12.0);
        assert!(!v_low.frames.is_empty() && !v_noise.frames.is_empty());
        let mid_l = &v_low.frames[v_low.frames.len() / 2];
        let mid_n = &v_noise.frames[v_noise.frames.len() / 2];
        assert!(mid_l.mean_luma() > 2.0, "scope draws something");
        // Different spectra -> different hues: compare mean red/blue balance.
        let tint = |f: &Frame| -> (f64, f64) {
            let (mut r, mut b) = (0.0f64, 0.0f64);
            for px in f.data.chunks_exact(4) {
                r += px[0] as f64;
                b += px[2] as f64;
            }
            (r, b)
        };
        let (lr, lb) = tint(mid_l);
        let (nr, nb) = tint(mid_n);
        // Low FM leans red (low centroid); white noise leans blue/violet.
        assert!(
            lr / lb.max(1.0) > nr / nb.max(1.0),
            "low tone redder than white noise: {} vs {}",
            lr / lb.max(1.0),
            nr / nb.max(1.0)
        );
    }

    #[test]
    fn synth_sources_are_complete() {
        let s = fm_source(110.0, 1.5, 1.0, 2.0, 48000);
        assert!(s.audio.is_some() && s.video.is_some());
        assert_eq!(s.base_hz, 110.0);
        assert!(s.duration() > 1.5);
        let n = noise_source(0.7, 1.0, 48000);
        assert!(n.audio.is_some() && n.video.is_some());
    }
}
