//! Audiovisual effects. Every effect has an AUDIO side and a VIDEO side
//! built on the same underlying idea, so the crunch you hear is the crunch
//! you see:
//!
//! | effect   | audio                          | video                          |
//! |----------|--------------------------------|--------------------------------|
//! | Crush    | sample-rate decimation + bit   | pixelation + color posterize   |
//! |          | depth reduction                |                                |
//! | Delay    | feedback delay line            | ghost frames at the same delay |
//! |          |                                | and feedback, optional drift   |
//! | Reverb   | Freeverb-style comb/allpass    | frame persistence + blur smear |
//! | Compress | FFT spectral quantization      | JPEG-style 8x8 DCT block       |
//! |          | (transform-domain crunch)      | quantization (same idea!)      |
//! | Tint     | resonant band at one frequency | every hue smushed to one hue   |
//! | Tune     | resonators on every note of a  | every hue snapped to the key's |
//! |          | key (frequencies → the key)    | colors (pitch class ↔ hue)     |
//! | Tile     | short multi-tap "tiling" of    | picture repeated as an N×N     |
//! |          | the sound (N-1 taps, mirror =  | grid (mirror = kaleidoscope    |
//! |          | taps ping-pong L/R)            | flips, turn = grid rotation)   |
//!
//! Correspondence is the default but configurable per effect: `audio` and
//! `video` are independent 0..1 amount dials. audio=1, video=1 means both
//! sides fully apply; video=0 makes an effect audio-only, and vice versa.

use crate::audio::StereoBuffer;
use crate::video::Frame;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EffectKind {
    /// Downres / bitcrush. `downsample`: hold factor (1..64); `bits`:
    /// audio bit depth == log2 of video color levels (1..16).
    Crush { downsample: f32, bits: f32 },
    /// Echo. `time` seconds, `feedback` 0..0.95, `mix` 0..1. Video ghosts
    /// drift by (`shift_x`, `shift_y`) fractions of the canvas per echo.
    Delay { time: f32, feedback: f32, mix: f32, shift_x: f32, shift_y: f32 },
    /// Smear. `size` 0..1 decay, `damp` 0..1 highs damping (video: blur),
    /// `mix` 0..1.
    Reverb { size: f32, damp: f32, mix: f32 },
    /// Codec crunch. `quality` 1 = transparent, 0 = destroyed.
    Compress { quality: f32 },
    /// Synesthetic tint: smush every hue toward `hue` (video) while a
    /// resonant bandpass smushes the spectrum toward the corresponding
    /// frequency (audio) — the shared hue<->freq axis in `synth`.
    /// `amount` 0 = off, 1 = fully monochrome / fully resonant.
    Tint { hue: f32, amount: f32 },
    /// Quantize frequencies to a key: a bank of resonators on every note
    /// of the key (audio) pulls any sound — speech, noise, field audio —
    /// onto those pitches; every hue snaps to the key's colors (video),
    /// pitch class ↔ hue wheel (C = red, 30° per semitone).
    /// `root` pitch class 0..11, `amount` 0..1 wet, `focus` 0..1 how
    /// narrow (and ringing) the resonators are.
    Tune { root: f32, scale: crate::music::ScaleKind, amount: f32, focus: f32 },
    /// Tiling. Video: the picture repeats as a `count`×`count` grid
    /// (1..8, fractional ok; 1 = passthrough); `mirror` >= 0.5 flips
    /// alternate tiles (seamless kaleidoscope); `spin` rotates the grid
    /// about the center, in turns (-0.5..0.5). Audio: `count` N adds N-1
    /// short decaying taps within ~40–120 ms (a "tiled" sound), mirror
    /// ping-pongs alternate taps L/R. count 1 = exact passthrough.
    Tile { count: f32, mirror: f32, spin: f32 },
}

impl EffectKind {
    pub fn parse(name: &str) -> Option<EffectKind> {
        let n = name.to_ascii_lowercase();
        Some(match n.as_str() {
            "crush" | "bitcrush" | "downres" => {
                EffectKind::Crush { downsample: 6.0, bits: 6.0 }
            }
            "delay" | "echo" => EffectKind::Delay {
                time: 0.3,
                feedback: 0.5,
                mix: 0.5,
                shift_x: 0.02,
                shift_y: 0.0,
            },
            "reverb" | "smear" => EffectKind::Reverb { size: 0.6, damp: 0.4, mix: 0.35 },
            "compress" | "compression" | "codec" => EffectKind::Compress { quality: 0.25 },
            "tint" | "smush" | "chroma" => EffectKind::Tint { hue: 8.0, amount: 0.6 },
            "tune" | "quantize" | "key" | "harmonize" => EffectKind::Tune {
                root: 9.0,
                scale: crate::music::ScaleKind::MinorPentatonic,
                amount: 0.8,
                focus: 0.5,
            },
            "tile" | "tiles" | "grid" => EffectKind::Tile { count: 2.0, mirror: 0.0, spin: 0.0 },
            "kaleido" | "kaleidoscope" => EffectKind::Tile { count: 3.0, mirror: 1.0, spin: 0.0 },
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            EffectKind::Crush { .. } => "crush",
            EffectKind::Delay { .. } => "delay",
            EffectKind::Reverb { .. } => "reverb",
            EffectKind::Compress { .. } => "compress",
            EffectKind::Tint { .. } => "tint",
            EffectKind::Tune { .. } => "tune",
            EffectKind::Tile { .. } => "tile",
        }
    }

    /// Canonical parse names of every effect kind, in UI order (each one
    /// round-trips through [`EffectKind::parse`] to its default).
    pub const KINDS: [&'static str; 7] = ["tune", "crush", "delay", "reverb", "compress", "tint", "tile"];

    /// One-line "audio ↔ video" description for UIs.
    pub fn blurb(&self) -> &'static str {
        match self {
            EffectKind::Crush { .. } => "bitcrush ↔ pixelate",
            EffectKind::Delay { .. } => "echo ↔ ghost frames",
            EffectKind::Reverb { .. } => "space ↔ smear",
            EffectKind::Compress { .. } => "codec crunch ↔ jpeg blocks",
            EffectKind::Tint { .. } => "resonance ↔ color",
            EffectKind::Tune { .. } => "frequencies → key ↔ hues → key colors",
            EffectKind::Tile { .. } => "multi-tap echoes ↔ tiled / kaleidoscope picture",
        }
    }
}

/// Hue (degrees) of a pitch class on the shared pitch-class ↔ hue wheel.
pub fn pitch_class_hue(pc: i32) -> f32 {
    pc.rem_euclid(12) as f32 * 30.0
}

/// The hues of every note in a key (for the video side of `Tune`).
pub fn key_hues(root: f32, scale: crate::music::ScaleKind) -> Vec<f32> {
    let r = root.round() as i32;
    scale.intervals().iter().map(|iv| pitch_class_hue(r + iv)).collect()
}

/// Frequencies of every note of a key between A1 (55 Hz) and A7.
pub fn key_freqs(root: f32, scale: crate::music::ScaleKind) -> Vec<f32> {
    let r = root.round() as i32;
    (33..=105)
        .filter(|m: &i32| scale.intervals().contains(&(m - r).rem_euclid(12)))
        .map(|m| crate::music::midi_to_hz(m as f32))
        .collect()
}

/// One effect in a chain: a kind plus the two correspondence dials.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AvEffect {
    pub kind: EffectKind,
    /// How strongly the audio side applies (0 = bypass audio).
    pub audio: f32,
    /// How strongly the video side applies (0 = bypass video).
    pub video: f32,
}

impl AvEffect {
    pub fn new(kind: EffectKind) -> AvEffect {
        AvEffect { kind, audio: 1.0, video: 1.0 }
    }

    /// Set a parameter by name. Returns Err for unknown names.
    pub fn set_param(&mut self, name: &str, value: f32) -> Result<(), String> {
        let key = name.to_ascii_lowercase().replace([' ', '-'], "_");
        match key.as_str() {
            "audio" => {
                self.audio = value.clamp(0.0, 1.0);
                return Ok(());
            }
            "video" => {
                self.video = value.clamp(0.0, 1.0);
                return Ok(());
            }
            _ => {}
        }
        match &mut self.kind {
            EffectKind::Crush { downsample, bits } => match key.as_str() {
                "downsample" | "downres" => *downsample = value.clamp(1.0, 64.0),
                "bits" => *bits = value.clamp(1.0, 16.0),
                _ => return Err(format!("crush has no parameter '{name}'")),
            },
            EffectKind::Delay { time, feedback, mix, shift_x, shift_y } => match key.as_str() {
                "time" => *time = value.clamp(0.01, 4.0),
                "feedback" => *feedback = value.clamp(0.0, 0.95),
                "mix" => *mix = value.clamp(0.0, 1.0),
                "shift_x" => *shift_x = value.clamp(-0.5, 0.5),
                "shift_y" => *shift_y = value.clamp(-0.5, 0.5),
                _ => return Err(format!("delay has no parameter '{name}'")),
            },
            EffectKind::Reverb { size, damp, mix } => match key.as_str() {
                "size" => *size = value.clamp(0.0, 1.0),
                "damp" => *damp = value.clamp(0.0, 1.0),
                "mix" => *mix = value.clamp(0.0, 1.0),
                _ => return Err(format!("reverb has no parameter '{name}'")),
            },
            EffectKind::Compress { quality } => match key.as_str() {
                "quality" => *quality = value.clamp(0.0, 1.0),
                _ => return Err(format!("compress has no parameter '{name}'")),
            },
            EffectKind::Tint { hue, amount } => match key.as_str() {
                "hue" => *hue = value.rem_euclid(360.0).min(crate::synth::HUE_SPAN),
                "amount" => *amount = value.clamp(0.0, 1.0),
                _ => return Err(format!("tint has no parameter '{name}'")),
            },
            EffectKind::Tune { root, amount, focus, .. } => match key.as_str() {
                "root" => *root = value.round().rem_euclid(12.0),
                "amount" | "mix" => *amount = value.clamp(0.0, 1.0),
                "focus" | "q" => *focus = value.clamp(0.0, 1.0),
                _ => return Err(format!("tune has no parameter '{name}'")),
            },
            EffectKind::Tile { count, mirror, spin } => match key.as_str() {
                "tiles" | "count" | "n" | "grid" => *count = value.clamp(1.0, 8.0),
                "mirror" | "kaleido" => *mirror = value.clamp(0.0, 1.0),
                "turn" | "spin" | "rotate" | "rotation" => *spin = value.clamp(-0.5, 0.5),
                _ => return Err(format!("tile has no parameter '{name}'")),
            },
        }
        Ok(())
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ======================================================================
// AUDIO SIDE — stateful, block-safe processors. The same chain code
// serves offline rendering (one big block) and realtime performance
// playback (a stream of small blocks): state persists across blocks.
// ======================================================================

/// Persistent state for one audio effect instance.
pub enum AudioFxState {
    Crush { hold: [f32; 2], phase: [f32; 2] },
    Delay { ring: [Vec<f32>; 2], pos: usize },
    Reverb { channels: [ReverbChannel; 2] },
    Compress { channels: [SpectralState; 2] },
    Tint { bq: [crate::dsp::Biquad; 2] },
    Tune { bank: Vec<[crate::dsp::Biquad; 2]>, env_in: [f32; 2], env_wet: [f32; 2], coef: f32 },
    /// Dry-input history for the multi-tap tiling (feed-forward, so the
    /// tap times can move freely with `count` without rebuilding).
    Tile { ring: [Vec<f32>; 2], pos: usize, sr: f32 },
}

/// Longest Tile span (seconds), used at count = 8. With the fractional
/// "next tap fading in", taps can reach ~1.05× this; the ring holds 1.5×.
const TILE_MAX_SPAN: f32 = 0.12;
/// Shortest Tile span (seconds), used at count = 2.
const TILE_MIN_SPAN: f32 = 0.04;
/// Wet level at count = 8 (scaled by (N-1)/7 below that).
const TILE_WET: f32 = 0.35;
/// Per-tap decay.
const TILE_DECAY: f32 = 0.75;

/// Tap layout for count `n`: (delay seconds, gain) per tap, gains
/// normalized to sum to 1. The last tap fades in with the fractional part
/// of `n`, so sweeping `count` never pops a tap in at full level.
fn tile_taps(n: f32) -> Vec<(f32, f32)> {
    let extra = (n.clamp(1.0, 8.0) - 1.0).max(0.0);
    if extra <= 1e-4 {
        return Vec::new();
    }
    let k_max = extra.ceil() as usize;
    let span = TILE_MIN_SPAN + (TILE_MAX_SPAN - TILE_MIN_SPAN) * ((extra - 1.0).max(0.0) / 6.0);
    let spacing = span / extra.max(1.0);
    let mut taps: Vec<(f32, f32)> = (1..=k_max)
        .map(|k| {
            let mut g = TILE_DECAY.powi(k as i32 - 1);
            if k == k_max {
                g *= extra - (k_max - 1) as f32;
            }
            (spacing * k as f32, g)
        })
        .collect();
    let sum: f32 = taps.iter().map(|t| t.1).sum::<f32>().max(1e-6);
    for t in taps.iter_mut() {
        t.1 /= sum;
    }
    taps
}

/// Build fresh states for a chain. Rebuild whenever the chain's shape or
/// its time-structuring params (delay time, reverb size) change.
pub fn audio_chain_states(chain: &[AvEffect], sample_rate: u32) -> Vec<AudioFxState> {
    chain
        .iter()
        .map(|fx| match fx.kind {
            EffectKind::Crush { .. } => {
                AudioFxState::Crush { hold: [0.0; 2], phase: [0.0; 2] }
            }
            EffectKind::Delay { time, .. } => {
                let d = ((time * sample_rate as f32) as usize).max(1);
                AudioFxState::Delay { ring: [vec![0.0; d], vec![0.0; d]], pos: 0 }
            }
            EffectKind::Reverb { size, damp, .. } => AudioFxState::Reverb {
                channels: [
                    ReverbChannel::new(sample_rate, size, damp, 0.0),
                    ReverbChannel::new(sample_rate, size, damp, 23.0),
                ],
            },
            EffectKind::Compress { .. } => AudioFxState::Compress {
                channels: [SpectralState::new(), SpectralState::new()],
            },
            EffectKind::Tint { hue, amount } => {
                let freq = crate::synth::hue_to_freq(hue);
                let q = 1.0 + amount * 7.0;
                let mk = || {
                    crate::dsp::Biquad::new(
                        crate::dsp::FilterKind::BandPass,
                        sample_rate as f32,
                        freq,
                        q,
                    )
                };
                AudioFxState::Tint { bq: [mk(), mk()] }
            }
            EffectKind::Tune { root, scale, focus, .. } => {
                let q = 6.0 + focus.clamp(0.0, 1.0) * 60.0;
                let sr = sample_rate as f32;
                let bank = key_freqs(root, scale)
                    .into_iter()
                    .filter(|f| *f < sr * 0.45)
                    .map(|f| {
                        let mk = || crate::dsp::Biquad::new(crate::dsp::FilterKind::BandPass, sr, f, q);
                        [mk(), mk()]
                    })
                    .collect();
                // ~60 ms level followers for the auto-gain.
                let coef = (-1.0 / (0.06 * sr)).exp();
                AudioFxState::Tune { bank, env_in: [0.0; 2], env_wet: [0.0; 2], coef }
            }
            EffectKind::Tile { .. } => {
                let sr = sample_rate as f32;
                let len = (TILE_MAX_SPAN * 1.5 * sr).ceil() as usize + 4;
                AudioFxState::Tile { ring: [vec![0.0; len], vec![0.0; len]], pos: 0, sr }
            }
        })
        .collect()
}

/// Samples of latency the chain introduces (spectral compression is
/// windowed and cannot be zero-latency in a stream).
pub fn audio_chain_latency(chain: &[AvEffect]) -> usize {
    // Only Compress is windowed; everything else (incl. Tint) is 0-latency.
    chain
        .iter()
        .filter(|fx| {
            matches!(fx.kind, EffectKind::Compress { quality } if quality < 0.999)
                && fx.audio > 0.001
        })
        .count()
        * SPECTRAL_N
}

/// Process one stereo block through the chain, streaming. Call repeatedly
/// with consecutive blocks and the same `states`.
pub fn process_audio_chain(
    left: &mut [f32],
    right: &mut [f32],
    chain: &[AvEffect],
    states: &mut [AudioFxState],
) {
    for (fx, state) in chain.iter().zip(states.iter_mut()) {
        if fx.audio <= 0.001 {
            continue;
        }
        match (fx.kind, state) {
            (EffectKind::Crush { downsample, bits }, AudioFxState::Crush { hold, phase }) => {
                let factor = lerp(1.0, downsample, fx.audio).max(1.0);
                let bits = lerp(16.0, bits, fx.audio);
                let levels = 2.0_f32.powf(bits.clamp(1.0, 16.0) - 1.0);
                for (c, ch) in [&mut *left, &mut *right].into_iter().enumerate() {
                    for s in ch.iter_mut() {
                        phase[c] += 1.0;
                        if phase[c] >= factor {
                            phase[c] -= factor;
                            hold[c] = (*s * levels).round() / levels;
                        }
                        *s = hold[c];
                    }
                }
            }
            (EffectKind::Delay { feedback, mix, .. }, AudioFxState::Delay { ring, pos }) => {
                let mix = mix * fx.audio;
                let len = ring[0].len();
                let mut p = *pos;
                for i in 0..left.len() {
                    for (c, s) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let wet = ring[c][p];
                        ring[c][p] = *s + wet * feedback;
                        *s += wet * mix;
                    }
                    p = (p + 1) % len;
                }
                *pos = p;
            }
            (EffectKind::Reverb { mix, .. }, AudioFxState::Reverb { channels }) => {
                let mix = mix * fx.audio;
                for (c, ch) in [&mut *left, &mut *right].into_iter().enumerate() {
                    channels[c].process(ch, mix);
                }
            }
            (EffectKind::Compress { quality }, AudioFxState::Compress { channels }) => {
                let q = lerp(1.0, quality, fx.audio);
                if q >= 0.999 {
                    continue;
                }
                channels[0].process(left, q);
                channels[1].process(right, q);
            }
            (EffectKind::Tint { amount, .. }, AudioFxState::Tint { bq }) => {
                // Morph toward the resonant band; makeup keeps loudness as
                // the band narrows.
                let mix = (amount * fx.audio).clamp(0.0, 1.0);
                let makeup = 1.0 + amount * 1.8;
                for (c, ch) in [&mut *left, &mut *right].into_iter().enumerate() {
                    for x in ch.iter_mut() {
                        let wet = bq[c].process(*x) * makeup;
                        *x = *x * (1.0 - mix) + wet * mix;
                    }
                }
            }
            (EffectKind::Tune { amount, .. }, AudioFxState::Tune { bank, env_in, env_wet, coef }) => {
                let mix = (amount * fx.audio).clamp(0.0, 1.0);
                let c = *coef;
                for (ch_i, ch) in [&mut *left, &mut *right].into_iter().enumerate() {
                    for x in ch.iter_mut() {
                        let dry = *x;
                        let mut wet = 0.0;
                        for bq in bank.iter_mut() {
                            wet += bq[ch_i].process(dry);
                        }
                        // Auto-gain: the narrow bank passes a sliver of the
                        // spectrum; bring it back up to the input's level.
                        env_in[ch_i] = env_in[ch_i] * c + dry * dry * (1.0 - c);
                        env_wet[ch_i] = env_wet[ch_i] * c + wet * wet * (1.0 - c);
                        let g = (env_in[ch_i] / env_wet[ch_i].max(1e-12)).sqrt().min(24.0);
                        let wet = (wet * g).clamp(-1.5, 1.5);
                        *x = dry * (1.0 - mix) + wet * mix;
                    }
                }
            }
            (EffectKind::Tile { count, mirror, .. }, AudioFxState::Tile { ring, pos, sr }) => {
                process_tile(left, right, ring, pos, *sr, count, mirror, fx.audio);
            }
            _ => debug_assert!(false, "chain/state mismatch — rebuild states"),
        }
    }
}

/// Multi-tap "tiling": each output sample adds N-1 decaying copies of the
/// recent dry input. Feed-forward (no feedback) so it can never run away,
/// and fractional (linear-interp) tap reads so sweeping `count` glides.
#[allow(clippy::too_many_arguments)]
fn process_tile(
    left: &mut [f32],
    right: &mut [f32],
    ring: &mut [Vec<f32>; 2],
    pos: &mut usize,
    sr: f32,
    count: f32,
    mirror: f32,
    amount: f32,
) {
    let len = ring[0].len();
    let taps = tile_taps(count);
    let extra = (count.clamp(1.0, 8.0) - 1.0).max(0.0);
    let wet = TILE_WET * (extra / 7.0) * amount.clamp(0.0, 1.0);
    let cross = mirror >= 0.5;
    // Precompute tap read offsets in samples (kept inside the ring).
    let taps: Vec<(f32, f32, bool)> = taps
        .iter()
        .enumerate()
        .map(|(k, &(d, g))| ((d * sr).clamp(1.0, (len - 2) as f32), g, cross && k % 2 == 0))
        .collect();
    let mut p = *pos;
    for i in 0..left.len() {
        let (dl, dr) = (left[i], right[i]);
        ring[0][p] = dl;
        ring[1][p] = dr;
        if wet > 0.0 && !taps.is_empty() {
            let mut acc = [0.0f32; 2];
            for &(d, g, swap) in &taps {
                let rd = p as f32 + len as f32 - d;
                let i0 = rd.floor();
                let f = rd - i0;
                let a = i0 as usize % len;
                let b = (a + 1) % len;
                for (c, out) in acc.iter_mut().enumerate() {
                    // Mirror: alternate taps come from the opposite side,
                    // so the echoes ping-pong L/R like flipped tiles.
                    let src = if swap { 1 - c } else { c };
                    let h = &ring[src];
                    *out += g * (h[a] + (h[b] - h[a]) * f);
                }
            }
            left[i] = dl + acc[0] * wet;
            right[i] = dr + acc[1] * wet;
        }
        p = (p + 1) % len;
    }
    *pos = p;
}

/// One-shot convenience: process a whole buffer, compensating for any
/// chain latency so offline renders stay time-aligned.
pub fn apply_audio_chain(buf: &mut StereoBuffer, chain: &[AvEffect]) {
    let mut states = audio_chain_states(chain, buf.sample_rate);
    let latency = audio_chain_latency(chain);
    let n = buf.len();
    if latency == 0 {
        let (l, r) = (&mut buf.left, &mut buf.right);
        process_audio_chain(l, r, chain, &mut states);
        return;
    }
    let mut l = buf.left.clone();
    let mut r = buf.right.clone();
    l.resize(n + latency + SPECTRAL_N, 0.0);
    r.resize(n + latency + SPECTRAL_N, 0.0);
    process_audio_chain(&mut l, &mut r, chain, &mut states);
    buf.left.copy_from_slice(&l[latency..latency + n]);
    buf.right.copy_from_slice(&r[latency..latency + n]);
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    feedback: f32,
    damp: f32,
    store: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32, damp: f32) -> Comb {
        Comb { buf: vec![0.0; len.max(1)], i: 0, feedback, damp, store: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.i];
        self.store = out * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.i] = x + self.store * self.feedback;
        self.i = (self.i + 1) % self.buf.len();
        out
    }
}

struct AllPass {
    buf: Vec<f32>,
    i: usize,
}

impl AllPass {
    fn new(len: usize) -> AllPass {
        AllPass { buf: vec![0.0; len.max(1)], i: 0 }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        let out = -x + b;
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        out
    }
}

/// Freeverb-style reverb tank for one channel (8 combs + 4 allpasses) with a
/// 160 Hz wet-input high-pass and natural HF damping floor so reverb tails
/// stay dense and velvety without low-end boom or narrow 4-comb ringing.
pub struct ReverbChannel {
    combs: Vec<Comb>,
    aps: Vec<AllPass>,
    hp_lp: f32,
    hp_a: f32,
}

impl ReverbChannel {
    fn new(sample_rate: u32, size: f32, damp: f32, offset: f32) -> ReverbChannel {
        let sr = sample_rate.max(1) as f32;
        let sr_scale = sr / 44100.0;
        let feedback = 0.68 + 0.24 * size.clamp(0.0, 1.0);
        let damp = 0.16 + damp.clamp(0.0, 1.0) * 0.68;
        let hp_a = 1.0 - (-2.0 * std::f32::consts::PI * 160.0 / sr).exp();
        ReverbChannel {
            combs: [1116.0f32, 1188.0, 1277.0, 1356.0, 1422.0, 1491.0, 1557.0, 1617.0]
                .iter()
                .map(|t| Comb::new(((t + offset) * sr_scale) as usize, feedback, damp))
                .collect(),
            aps: [556.0f32, 441.0, 341.0, 225.0]
                .iter()
                .map(|t| AllPass::new(((t + offset) * sr_scale) as usize))
                .collect(),
            hp_lp: 0.0,
            hp_a,
        }
    }

    fn process(&mut self, samples: &mut [f32], mix: f32) {
        for s in samples.iter_mut() {
            self.hp_lp += self.hp_a * (*s - self.hp_lp);
            let input = (*s - self.hp_lp) * 0.13;
            let mut wet: f32 = self.combs.iter_mut().map(|c| c.process(input)).sum();
            for ap in self.aps.iter_mut() {
                wet = ap.process(wet);
            }
            *s += wet * mix;
        }
    }
}

// ---------------------------------------------------------------- FFT

/// In-place iterative radix-2 FFT. `re`/`im` length must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0f32 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr0, vi0) = (re[i + k + len / 2], im[i + k + len / 2]);
                let vr = vr0 * cr - vi0 * ci;
                let vi = vr0 * ci + vi0 * cr;
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let inv = 1.0 / n as f32;
        for v in re.iter_mut() {
            *v *= inv;
        }
        for v in im.iter_mut() {
            *v *= inv;
        }
    }
}

pub const SPECTRAL_N: usize = 1024;
const SPECTRAL_HOP: usize = SPECTRAL_N / 2;

fn spectral_window() -> &'static [f32; SPECTRAL_N] {
    use std::sync::OnceLock;
    static W: OnceLock<[f32; SPECTRAL_N]> = OnceLock::new();
    W.get_or_init(|| {
        // Periodic Hann; hop = n/2 makes overlapping windows sum to 1.
        let mut w = [0.0f32; SPECTRAL_N];
        for (i, v) in w.iter_mut().enumerate() {
            *v = 0.5
                - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SPECTRAL_N as f32).cos();
        }
        w
    })
}

/// Streaming transform-domain audio crunch for one channel: FFT windows,
/// throw away / quantize weak coefficients (what lossy codecs do),
/// overlap-add back. Emits 1:1 samples with SPECTRAL_HOP latency.
pub struct SpectralState {
    /// Input samples not yet fully consumed by window starts.
    buf: Vec<f32>,
    /// OLA accumulator aligned with buf[0].
    acc: Vec<f32>,
    /// Next window start, relative to buf[0].
    next_window: usize,
    /// Output FIFO of finalized samples, primed with SPECTRAL_N zeros so
    /// the stream latency is exactly SPECTRAL_N for any block size.
    out: std::collections::VecDeque<f32>,
}

impl SpectralState {
    pub fn new() -> SpectralState {
        SpectralState {
            buf: Vec::new(),
            acc: Vec::new(),
            next_window: 0,
            out: std::iter::repeat(0.0).take(SPECTRAL_N).collect(),
        }
    }

    /// Process a block in place (output delayed by SPECTRAL_HOP samples).
    pub fn process(&mut self, samples: &mut [f32], quality: f32) {
        let quality = quality.clamp(0.0, 1.0);
        let n = SPECTRAL_N;
        let window = spectral_window();
        let keep = ((n / 2) as f32 * (0.02 + 0.98 * quality * quality)) as usize;
        let mag_levels = lerp(6.0, 256.0, quality);

        self.buf.extend_from_slice(samples);
        if self.acc.len() < self.buf.len() {
            self.acc.resize(self.buf.len(), 0.0);
        }

        while self.next_window + n <= self.buf.len() {
            let start = self.next_window;
            let mut re: Vec<f32> =
                (0..n).map(|i| self.buf[start + i] * window[i]).collect();
            let mut im = vec![0.0f32; n];
            fft(&mut re, &mut im, false);

            let mut mags: Vec<f32> = (0..n / 2)
                .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
                .collect();
            mags.sort_by(|a, b| b.partial_cmp(a).unwrap());
            let threshold =
                mags.get(keep.min(mags.len() - 1)).copied().unwrap_or(0.0);
            let max_mag = mags[0].max(1e-9);

            for i in 0..=n / 2 {
                let (r, ii) = (re[i], im[i]);
                let mag = (r * r + ii * ii).sqrt();
                let new_mag = if mag < threshold {
                    0.0
                } else {
                    // Quantize surviving magnitudes -> codec "birdies".
                    (mag / max_mag * mag_levels).round() / mag_levels * max_mag
                };
                let scale = if mag > 1e-9 { new_mag / mag } else { 0.0 };
                re[i] *= scale;
                im[i] *= scale;
                if i > 0 && i < n / 2 {
                    re[n - i] *= scale;
                    im[n - i] *= scale;
                }
            }

            fft(&mut re, &mut im, true);
            for i in 0..n {
                self.acc[start + i] += re[i];
            }
            self.next_window += SPECTRAL_HOP;
        }

        // Samples before the next window start are final: emit and drain.
        let final_n = self.next_window;
        self.out.extend(self.acc[..final_n].iter());
        self.buf.drain(..final_n);
        self.acc.drain(..final_n);
        self.next_window = 0;

        // Fill the block from the FIFO (never underruns: the FIFO holds
        // N zeros + all finalized samples, and a sample is finalized at
        // most N inputs after it arrived).
        for s in samples.iter_mut() {
            *s = self.out.pop_front().unwrap_or(0.0);
        }
    }
}

impl Default for SpectralState {
    fn default() -> Self {
        SpectralState::new()
    }
}

// ======================================================================
// VIDEO SIDE
// ======================================================================

/// Per-effect temporal state for video processing (delay ghosts, reverb
/// persistence). One state per effect instance per rendered clip/master.
pub enum VideoFxState {
    Stateless,
    Delay { ring: VecDeque<Frame>, delay_frames: usize },
    Reverb { acc: Vec<f32> },
}

/// Build the state vector for a chain.
pub fn video_chain_states(chain: &[AvEffect], fps: f32) -> Vec<VideoFxState> {
    chain
        .iter()
        .map(|fx| match fx.kind {
            EffectKind::Delay { time, .. } => VideoFxState::Delay {
                ring: VecDeque::new(),
                delay_frames: ((time * fps).round() as usize).max(1),
            },
            EffectKind::Reverb { .. } => VideoFxState::Reverb { acc: Vec::new() },
            EffectKind::Crush { .. }
            | EffectKind::Compress { .. }
            | EffectKind::Tint { .. }
            | EffectKind::Tune { .. }
            | EffectKind::Tile { .. } => VideoFxState::Stateless,
        })
        .collect()
}

/// Process one frame (or layer) through the whole chain. Call once per
/// output frame, in order.
pub fn apply_video_chain(
    frame: &mut Frame,
    chain: &[AvEffect],
    states: &mut [VideoFxState],
) {
    for (fx, state) in chain.iter().zip(states.iter_mut()) {
        if fx.video <= 0.001 {
            // Still record history so enabling mid-stream behaves.
            if let VideoFxState::Delay { ring, delay_frames } = state {
                ring.push_back(frame.clone());
                while ring.len() > *delay_frames {
                    ring.pop_front();
                }
            }
            continue;
        }
        match fx.kind {
            EffectKind::Crush { downsample, bits } => {
                let factor = lerp(1.0, downsample, fx.video).round() as u32;
                let levels = 2.0_f32.powf(lerp(8.0, bits.clamp(1.0, 8.0), fx.video));
                video_crush(frame, factor.max(1), levels);
            }
            EffectKind::Delay { feedback, mix, shift_x, shift_y, .. } => {
                if let VideoFxState::Delay { ring, delay_frames } = state {
                    video_delay(
                        frame,
                        ring,
                        *delay_frames,
                        feedback,
                        mix * fx.video,
                        shift_x,
                        shift_y,
                    );
                }
            }
            EffectKind::Reverb { size, damp, mix } => {
                if let VideoFxState::Reverb { acc } = state {
                    video_reverb(frame, acc, size, damp, mix * fx.video);
                }
            }
            EffectKind::Compress { quality } => {
                let q = lerp(1.0, quality, fx.video);
                video_compress(frame, q);
            }
            EffectKind::Tint { hue, amount } => {
                video_tint(frame, hue, amount * fx.video);
            }
            EffectKind::Tune { root, scale, amount, .. } => {
                video_tune(frame, &key_hues(root, scale), amount * fx.video);
            }
            EffectKind::Tile { count, mirror, spin } => {
                let (n, turn) = tile_video_params(count, spin, fx.video);
                video_tile(frame, n, mirror, turn);
            }
        }
    }
}

/// Effective (count, turn) for the Tile video side at picture dial
/// `video`: the dial glides count from 1 and turn from 0. Shared with the
/// GPU preview so both paths agree.
pub fn tile_video_params(count: f32, spin: f32, video: f32) -> (f32, f32) {
    let v = video.clamp(0.0, 1.0);
    (lerp(1.0, count.clamp(1.0, 8.0), v), spin.clamp(-0.5, 0.5) * v)
}

/// Where output uv (0..1, y-down, pixel centers) samples the source for
/// the Tile effect. `aspect` = width / height keeps rotation undistorted.
/// This is the exact math of the GPU `tile` shader.
pub fn tile_source_uv(u: f32, v: f32, count: f32, mirror: f32, turn: f32, aspect: f32) -> (f32, f32) {
    let (mut u, mut v) = (u, v);
    if turn.abs() > 1e-6 {
        let (s, c) = (turn * std::f32::consts::TAU).sin_cos();
        let dx = (u - 0.5) * aspect;
        let dy = v - 0.5;
        u = (dx * c - dy * s) / aspect + 0.5;
        v = dx * s + dy * c + 0.5;
    }
    let tile = |t: f32| -> f32 {
        let q = t * count;
        let i = q.floor();
        let f = q - i;
        if mirror >= 0.5 && (i as i64).rem_euclid(2) == 1 { 1.0 - f } else { f }
    };
    (tile(u), tile(v))
}

/// Repeat the picture as a `count`×`count` grid (nearest sampling, like
/// `video_crush`). Whole RGBA pixels are copied, so alpha cut-outs tile.
fn video_tile(frame: &mut Frame, count: f32, mirror: f32, turn: f32) {
    if (count - 1.0).abs() < 1e-4 && turn.abs() < 1e-6 {
        return;
    }
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let src = frame.data.clone();
    let aspect = w as f32 / h as f32;
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let (su, sv) = tile_source_uv(u, v, count, mirror, turn, aspect);
            let sx = ((su * w as f32) as usize).min(w - 1);
            let sy = ((sv * h as f32) as usize).min(h - 1);
            let (di, si) = ((y * w + x) * 4, (sy * w + sx) * 4);
            frame.data[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
}

/// Snap every pixel's hue to the nearest of `hues` (the key's colors);
/// `amount` blends from untouched to fully snapped.
fn video_tune(frame: &mut Frame, hues: &[f32], amount: f32) {
    use crate::video::{hsv_to_rgb, rgb_to_hsv};
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.002 || hues.is_empty() {
        return;
    }
    for px in frame.data.chunks_exact_mut(4) {
        let (h, s, v) = rgb_to_hsv(px[0], px[1], px[2]);
        let d = nearest_hue_delta(h, hues);
        let (r, g, b) = hsv_to_rgb((h + d * amount).rem_euclid(360.0), s, v);
        px[0] = r;
        px[1] = g;
        px[2] = b;
    }
}

/// Signed shortest hue move from `h` to the nearest of `hues`.
pub fn nearest_hue_delta(h: f32, hues: &[f32]) -> f32 {
    let mut best = 0.0f32;
    let mut best_abs = f32::INFINITY;
    for &t in hues {
        let mut d = (t - h).rem_euclid(360.0);
        if d > 180.0 {
            d -= 360.0;
        }
        if d.abs() < best_abs {
            best_abs = d.abs();
            best = d;
        }
    }
    best
}

/// Smush every pixel's hue toward `hue`; at high amounts even grey pixels
/// pick up the tint (that is what "make it all reddish" means).
fn video_tint(frame: &mut Frame, hue: f32, amount: f32) {
    use crate::video::{hsv_to_rgb, rgb_to_hsv};
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.002 {
        return;
    }
    for px in frame.data.chunks_exact_mut(4) {
        let (h, s, v) = rgb_to_hsv(px[0], px[1], px[2]);
        let mut d = (h - hue).rem_euclid(360.0);
        if d > 180.0 {
            d -= 360.0;
        }
        let nh = (hue + d * (1.0 - amount)).rem_euclid(360.0);
        // Greys gain saturation toward the tint as the smush deepens.
        let ns = s.max(amount * amount * 0.5 * v.min(1.0));
        let (r, g, b) = hsv_to_rgb(nh, ns, v);
        px[0] = r;
        px[1] = g;
        px[2] = b;
    }
}

/// Pixelate (nearest-block downres) + posterize color depth.
fn video_crush(frame: &mut Frame, factor: u32, levels: f32) {
    let (w, h) = (frame.width, frame.height);
    let posterize = |v: u8| -> u8 {
        let l = (levels - 1.0).max(1.0);
        ((v as f32 / 255.0 * l).round() / l * 255.0) as u8
    };
    if factor > 1 {
        for y in 0..h {
            let sy = (y / factor) * factor;
            for x in 0..w {
                let sx = (x / factor) * factor;
                let px = frame.get(sx, sy);
                frame.put(x, y, px);
            }
        }
    }
    if levels < 255.0 {
        for px in frame.data.chunks_exact_mut(4) {
            px[0] = posterize(px[0]);
            px[1] = posterize(px[1]);
            px[2] = posterize(px[2]);
        }
    }
}

/// Ghost frames: blend the frame from `delay_frames` ago (post-effect, so
/// echoes compound with `feedback`), drifting by (shift_x, shift_y).
fn video_delay(
    frame: &mut Frame,
    ring: &mut VecDeque<Frame>,
    delay_frames: usize,
    feedback: f32,
    mix: f32,
    shift_x: f32,
    shift_y: f32,
) {
    if ring.len() >= delay_frames {
        let ghost = ring[ring.len() - delay_frames].clone();
        let dx = (shift_x * frame.width as f32) as i32;
        let dy = (shift_y * frame.height as f32) as i32;
        let (w, h) = (frame.width as i32, frame.height as i32);
        for y in 0..h {
            let gy = y - dy;
            if gy < 0 || gy >= h {
                continue;
            }
            for x in 0..w {
                let gx = x - dx;
                if gx < 0 || gx >= w {
                    continue;
                }
                let g = ghost.get(gx as u32, gy as u32);
                let ga = g[3] as f32 / 255.0 * mix;
                if ga <= 0.004 {
                    continue;
                }
                let d = frame.get(x as u32, y as u32);
                let da = d[3] as f32 / 255.0;
                let out_a = (da + ga * (1.0 - da)).max(1e-6);
                let blend = |gc: u8, dc: u8| -> u8 {
                    // Ghost sits UNDER the current frame where it is
                    // transparent, and SCREENS over it where it is opaque
                    // (so master-bus ghosts glow instead of vanishing).
                    let under = (dc as f32 * da + gc as f32 * ga * (1.0 - da)) / out_a;
                    (under + gc as f32 * ga * da * (1.0 - under / 255.0)).min(255.0) as u8
                };
                frame.put(
                    x as u32,
                    y as u32,
                    [
                        blend(g[0], d[0]),
                        blend(g[1], d[1]),
                        blend(g[2], d[2]),
                        (out_a * 255.0) as u8,
                    ],
                );
            }
        }
    }
    // Store the processed frame so echoes-of-echoes decay by `feedback`.
    let mut stored = frame.clone();
    if feedback < 0.999 {
        for px in stored.data.chunks_exact_mut(4) {
            px[3] = (px[3] as f32 * feedback) as u8;
        }
    }
    ring.push_back(stored);
    while ring.len() > delay_frames {
        ring.pop_front();
    }
}

/// Persistence + blur: an accumulator remembers past frames with a decay
/// set by `size`, blurred by `damp`, mixed back under the live frame.
fn video_reverb(frame: &mut Frame, acc: &mut Vec<f32>, size: f32, damp: f32, mix: f32) {
    let n = frame.data.len();
    if acc.len() != n {
        *acc = vec![0.0; n];
    }
    let decay = 0.75 + 0.24 * size.clamp(0.0, 1.0);
    // Feed the live frame into the accumulator (premultiplied by alpha).
    for (i, px) in frame.data.chunks_exact(4).enumerate() {
        let a = px[3] as f32 / 255.0;
        let base = i * 4;
        for c in 0..3 {
            let v = px[c] as f32 * a;
            acc[base + c] = acc[base + c] * decay + v * (1.0 - decay);
        }
        acc[base + 3] = acc[base + 3] * decay + a * 255.0 * (1.0 - decay);
    }
    // Blur the accumulator (separable box) — the visual "damping".
    let radius = (damp.clamp(0.0, 1.0) * 6.0) as usize;
    if radius > 0 {
        box_blur(acc, frame.width as usize, frame.height as usize, radius);
    }
    // Composite the smear UNDER transparent regions and SCREEN it over
    // opaque ones (so master-bus smears glow over the mix).
    for (i, px) in frame.data.chunks_exact_mut(4).enumerate() {
        let base = i * 4;
        let sa = acc[base + 3] / 255.0 * mix;
        if sa <= 0.004 {
            continue;
        }
        let da = px[3] as f32 / 255.0;
        let out_a = (da + sa * (1.0 - da)).max(1e-6);
        for c in 0..3 {
            let smear_c = acc[base + c] / (acc[base + 3] / 255.0).max(1e-6);
            let under = (px[c] as f32 * da + smear_c * sa * (1.0 - da)) / out_a;
            px[c] = (under + smear_c * sa * da * (1.0 - under / 255.0)).min(255.0) as u8;
        }
        px[3] = (out_a * 255.0) as u8;
    }
}

/// Separable box blur over an RGBA f32 buffer.
fn box_blur(data: &mut [f32], w: usize, h: usize, radius: usize) {
    let mut tmp = data.to_vec();
    // Horizontal.
    for y in 0..h {
        for c in 0..4 {
            let mut sum = 0.0f32;
            let row = y * w;
            for x in 0..w.min(radius + 1) {
                sum += data[(row + x) * 4 + c];
            }
            let mut count = w.min(radius + 1) as f32;
            for x in 0..w {
                tmp[(row + x) * 4 + c] = sum / count;
                if x + radius + 1 < w {
                    sum += data[(row + x + radius + 1) * 4 + c];
                    count += 1.0;
                }
                if x >= radius {
                    sum -= data[(row + x - radius) * 4 + c];
                    count -= 1.0;
                }
            }
        }
    }
    // Vertical.
    for x in 0..w {
        for c in 0..4 {
            let mut sum = 0.0f32;
            for y in 0..h.min(radius + 1) {
                sum += tmp[(y * w + x) * 4 + c];
            }
            let mut count = h.min(radius + 1) as f32;
            for y in 0..h {
                data[(y * w + x) * 4 + c] = sum / count;
                if y + radius + 1 < h {
                    sum += tmp[((y + radius + 1) * w + x) * 4 + c];
                    count += 1.0;
                }
                if y >= radius {
                    sum -= tmp[((y - radius) * w + x) * 4 + c];
                    count -= 1.0;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- DCT

/// 8x8 DCT-II basis, precomputed on first use.
fn dct8_basis() -> &'static [[f32; 8]; 8] {
    use std::sync::OnceLock;
    static BASIS: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
    BASIS.get_or_init(|| {
        let mut b = [[0.0f32; 8]; 8];
        for (u, row) in b.iter_mut().enumerate() {
            let alpha = if u == 0 { (1.0f32 / 8.0).sqrt() } else { (2.0f32 / 8.0).sqrt() };
            for (x, v) in row.iter_mut().enumerate() {
                *v = alpha
                    * ((2.0 * x as f32 + 1.0) * u as f32 * std::f32::consts::PI / 16.0).cos();
            }
        }
        b
    })
}

/// JPEG-style crunch: per 8x8 block per channel, DCT -> quantize with a
/// frequency-weighted step (coarser for high frequencies) -> inverse DCT.
fn video_compress(frame: &mut Frame, quality: f32) {
    let quality = quality.clamp(0.0, 1.0);
    if quality >= 0.999 {
        return;
    }
    let basis = dct8_basis();
    let crunch = (1.0 - quality) * (1.0 - quality); // perceptual-ish ramp
    let (w, h) = (frame.width as usize, frame.height as usize);

    let mut block = [[0.0f32; 8]; 8];
    let mut coef = [[0.0f32; 8]; 8];
    for by in (0..h).step_by(8) {
        for bx in (0..w).step_by(8) {
            for c in 0..3 {
                // Load (clamp at edges).
                for y in 0..8 {
                    let sy = (by + y).min(h - 1);
                    for x in 0..8 {
                        let sx = (bx + x).min(w - 1);
                        block[y][x] = frame.data[(sy * w + sx) * 4 + c] as f32 - 128.0;
                    }
                }
                // Forward DCT: coef = B * block * B^T
                for (u, coef_row) in coef.iter_mut().enumerate() {
                    for (v, cv) in coef_row.iter_mut().enumerate() {
                        let mut s = 0.0f32;
                        for y in 0..8 {
                            for x in 0..8 {
                                s += basis[u][y] * block[y][x] * basis[v][x];
                            }
                        }
                        *cv = s;
                    }
                }
                // Keep only the strongest coefficients (like the audio
                // spectral crush keeps the strongest bins), then quantize
                // the survivors — harsher at higher spatial frequencies.
                let keep = (1.0 + 63.0 * quality * quality) as usize;
                if keep < 64 {
                    let mut mags: Vec<f32> = coef
                        .iter()
                        .flatten()
                        .map(|c| c.abs())
                        .collect();
                    mags.sort_by(|a, b| b.partial_cmp(a).unwrap());
                    let threshold = mags[keep.min(63)];
                    for coef_row in coef.iter_mut() {
                        for cv in coef_row.iter_mut() {
                            if cv.abs() <= threshold {
                                *cv = 0.0;
                            }
                        }
                    }
                }
                for (u, coef_row) in coef.iter_mut().enumerate() {
                    for (v, cv) in coef_row.iter_mut().enumerate() {
                        let q = 1.0 + crunch * (8.0 + 16.0 * (u + v) as f32);
                        *cv = (*cv / q).round() * q;
                    }
                }
                // Inverse DCT: block = B^T * coef * B
                for y in 0..8 {
                    let sy = by + y;
                    if sy >= h {
                        continue;
                    }
                    for x in 0..8 {
                        let sx = bx + x;
                        if sx >= w {
                            continue;
                        }
                        let mut s = 0.0f32;
                        for (u, coef_row) in coef.iter().enumerate() {
                            for (v, cv) in coef_row.iter().enumerate() {
                                s += basis[u][y] * *cv * basis[v][x];
                            }
                        }
                        frame.data[(sy * w + sx) * 4 + c] =
                            (s + 128.0).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::VideoClip;

    fn impulse_buffer(len_secs: f64, sr: u32) -> StereoBuffer {
        let mut b = StereoBuffer::new(len_secs, sr);
        b.left[0] = 1.0;
        b.right[0] = 1.0;
        b
    }

    /// Alpha-aware luma: what the frame contributes when composited onto
    /// black (mean_luma alone ignores the alpha channel).
    fn luma_on_black(f: &Frame) -> f32 {
        let mut black = Frame::black(f.width, f.height);
        crate::video::blend_layer(&mut black, f, 0.0, 1.0);
        black.mean_luma()
    }

    #[test]
    fn fft_roundtrip() {
        let n = 256;
        let orig: Vec<f32> = (0..n).map(|i| ((i * 7) % 13) as f32 / 13.0 - 0.5).collect();
        let mut re = orig.clone();
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im, false);
        fft(&mut re, &mut im, true);
        for (a, b) in orig.iter().zip(&re) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }

    #[test]
    fn crush_reduces_distinct_values() {
        let sr = 48000;
        let mut buf = StereoBuffer::new(0.2, sr);
        for (i, s) in buf.left.iter_mut().enumerate() {
            *s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin();
        }
        buf.right.copy_from_slice(&buf.left);
        let fx = AvEffect::new(EffectKind::Crush { downsample: 8.0, bits: 3.0 });
        apply_audio_chain(&mut buf, &[fx]);
        let mut vals: Vec<i32> = buf.left.iter().map(|s| (s * 1000.0) as i32).collect();
        vals.sort_unstable();
        vals.dedup();
        // 3 bits -> at most 2^3+1 quantization levels.
        assert!(vals.len() <= 9, "expected few levels, got {}", vals.len());
        // Zero-order hold: consecutive samples repeat.
        assert_eq!(buf.left[100], buf.left[101]);
    }

    #[test]
    fn delay_produces_decaying_echoes() {
        let sr = 48000;
        let mut buf = impulse_buffer(1.0, sr);
        let fx = AvEffect::new(EffectKind::Delay {
            time: 0.1,
            feedback: 0.5,
            mix: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        apply_audio_chain(&mut buf, &[fx]);
        let d = (0.1 * sr as f32) as usize;
        assert!((buf.left[d] - 1.0).abs() < 0.01, "first echo ~1.0: {}", buf.left[d]);
        assert!((buf.left[2 * d] - 0.5).abs() < 0.01, "second echo ~0.5: {}", buf.left[2 * d]);
        assert!((buf.left[3 * d] - 0.25).abs() < 0.01);
        // audio dial at 0 bypasses.
        let mut dry = impulse_buffer(1.0, sr);
        let mut off = fx;
        off.audio = 0.0;
        apply_audio_chain(&mut dry, &[off]);
        assert_eq!(dry.left[d], 0.0);
    }

    #[test]
    fn reverb_adds_tail() {
        let sr = 48000;
        let mut buf = impulse_buffer(2.0, sr);
        let fx = AvEffect::new(EffectKind::Reverb { size: 0.8, damp: 0.3, mix: 0.8 });
        apply_audio_chain(&mut buf, &[fx]);
        let tail: f32 = buf.left[sr as usize / 2..sr as usize]
            .iter()
            .map(|s| s * s)
            .sum();
        assert!(tail > 1e-6, "reverb should ring: tail energy {tail}");
        // Mix 0 leaves the signal dry.
        let mut dry = impulse_buffer(2.0, sr);
        let fx0 = AvEffect::new(EffectKind::Reverb { size: 0.8, damp: 0.3, mix: 0.0 });
        apply_audio_chain(&mut dry, &[fx0]);
        let tail0: f32 = dry.left[sr as usize / 2..sr as usize].iter().map(|s| s * s).sum();
        assert!(tail0 < 1e-12);
    }

    #[test]
    fn spectral_crush_degrades_but_preserves_energy() {
        let sr = 48000;
        let mut buf = StereoBuffer::new(1.0, sr);
        for (i, s) in buf.left.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            *s = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 0.2 * (2.0 * std::f32::consts::PI * 2917.0 * t).sin();
        }
        buf.right.copy_from_slice(&buf.left);
        let before = buf.rms();
        let orig = buf.left.clone();
        let fx = AvEffect::new(EffectKind::Compress { quality: 0.15 });
        apply_audio_chain(&mut buf, &[fx]);
        let after = buf.rms();
        assert!(after > before * 0.2, "energy mostly retained: {before} -> {after}");
        let diff: f32 = orig
            .iter()
            .zip(&buf.left)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / orig.len() as f32;
        assert!(diff > 1e-4, "low quality should audibly change the signal");
        // Quality 1.0 is a near-passthrough.
        let mut clean = StereoBuffer::new(1.0, sr);
        clean.left.copy_from_slice(&orig);
        clean.right.copy_from_slice(&orig);
        let fx1 = AvEffect::new(EffectKind::Compress { quality: 1.0 });
        apply_audio_chain(&mut clean, &[fx1]);
        let diff1: f32 = orig
            .iter()
            .zip(&clean.left)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / orig.len() as f32;
        assert!(diff1 < 1e-3, "quality 1 ~ transparent, diff {diff1}");
    }

    #[test]
    fn chunked_streaming_matches_one_shot() {
        // The same chain processed as one big block vs ragged small blocks
        // must produce identical output — this is what makes realtime
        // performance playback equal to the offline render.
        let sr = 48000;
        let len = sr as usize; // 1s
        let src: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f32 / sr as f32;
                0.4 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 3130.0 * t).sin()
            })
            .collect();
        let chain = [
            AvEffect::new(EffectKind::Crush { downsample: 5.0, bits: 6.0 }),
            AvEffect::new(EffectKind::Delay {
                time: 0.13,
                feedback: 0.4,
                mix: 0.6,
                shift_x: 0.0,
                shift_y: 0.0,
            }),
            AvEffect::new(EffectKind::Reverb { size: 0.6, damp: 0.3, mix: 0.4 }),
            AvEffect::new(EffectKind::Tint { hue: 40.0, amount: 0.5 }),
            AvEffect::new(EffectKind::Compress { quality: 0.3 }),
        ];

        // One shot.
        let mut one_l = src.clone();
        let mut one_r = src.clone();
        let mut st1 = audio_chain_states(&chain, sr);
        process_audio_chain(&mut one_l, &mut one_r, &chain, &mut st1);

        // Ragged blocks (including sizes around the spectral hop).
        let mut str_l = src.clone();
        let mut str_r = src.clone();
        let mut st2 = audio_chain_states(&chain, sr);
        let sizes = [64usize, 480, 1024, 333, 4096, 100, 2048];
        let mut pos = 0;
        let mut k = 0;
        while pos < len {
            let b = sizes[k % sizes.len()].min(len - pos);
            let (l, r) = (&mut str_l[pos..pos + b], &mut str_r[pos..pos + b]);
            process_audio_chain(l, r, &chain, &mut st2);
            pos += b;
            k += 1;
        }

        for i in 0..len {
            assert!(
                (one_l[i] - str_l[i]).abs() < 1e-5,
                "left diverged at {i}: {} vs {}",
                one_l[i],
                str_l[i]
            );
            assert!((one_r[i] - str_r[i]).abs() < 1e-5);
        }
        // And it's not silence.
        let rms: f32 =
            (str_l.iter().map(|s| s * s).sum::<f32>() / len as f32).sqrt();
        assert!(rms > 0.05, "rms {rms}");
    }

    #[test]
    fn offline_wrapper_compensates_compress_latency() {
        // apply_audio_chain with a Compress must keep the signal aligned:
        // an impulse at sample k stays near sample k, not k + latency.
        let sr = 48000;
        let mut buf = StereoBuffer::new(0.5, sr);
        let k = 12000usize;
        buf.left[k] = 1.0;
        buf.right[k] = 1.0;
        let fx = AvEffect::new(EffectKind::Compress { quality: 0.6 });
        apply_audio_chain(&mut buf, &[fx]);
        // Energy should be concentrated around k (windowed smear allowed).
        let around: f32 = buf.left[k.saturating_sub(SPECTRAL_N)..k + SPECTRAL_N]
            .iter()
            .map(|s| s * s)
            .sum();
        let total: f32 = buf.left.iter().map(|s| s * s).sum();
        assert!(total > 1e-6, "impulse survived");
        assert!(
            around / total > 0.8,
            "energy centered on the impulse: {}",
            around / total
        );
    }

    #[test]
    fn tint_smushes_hues_and_spectrum_together() {
        use crate::video::{hsv_to_rgb, rgb_to_hsv, VideoClip};
        // VIDEO: a colorful frame smushed toward red gets red-dominant.
        let clip = VideoClip::test_pattern(64, 48, 10.0, 0.4);
        let mut frame = clip.frames[2].clone();
        let fx = AvEffect::new(EffectKind::Tint { hue: 5.0, amount: 0.85 });
        let mut st = video_chain_states(&[fx], 10.0);
        apply_video_chain(&mut frame, &[fx], &mut st);
        let mut near = 0usize;
        let mut total = 0usize;
        for px in frame.data.chunks_exact(4) {
            let (h, s_, _) = rgb_to_hsv(px[0], px[1], px[2]);
            if s_ > 0.1 {
                total += 1;
                let mut d = (h - 5.0).rem_euclid(360.0);
                if d > 180.0 {
                    d = 360.0 - d;
                }
                if d < 40.0 {
                    near += 1;
                }
            }
        }
        assert!(total > 0 && near as f32 / total as f32 > 0.9,
            "hues smushed to red: {near}/{total}");
        // Greys pick up the tint at high amounts.
        let mut grey = Frame::black(8, 8);
        for px in grey.data.chunks_exact_mut(4) {
            px[0] = 128; px[1] = 128; px[2] = 128;
        }
        let mut st2 = video_chain_states(&[fx], 10.0);
        apply_video_chain(&mut grey, &[fx], &mut st2);
        let p0 = grey.get(4, 4);
        assert!(p0[0] > p0[2] + 10, "grey turned reddish: {p0:?}");

        // AUDIO: the corresponding band survives; far bands are crushed.
        // hue 5 -> ~85 Hz center. Compare a near-band 90 Hz sine with a
        // 4 kHz sine after full-amount tint.
        let sr = 48000;
        let center = crate::synth::hue_to_freq(5.0);
        assert!(center < 120.0, "red maps low: {center}");
        let tone = |freq: f32| -> f32 {
            let mut buf = StereoBuffer::new(0.5, sr);
            for (i, v) in buf.left.iter_mut().enumerate() {
                *v = (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin();
            }
            buf.right.copy_from_slice(&buf.left);
            let fx = AvEffect::new(EffectKind::Tint { hue: 5.0, amount: 1.0 });
            apply_audio_chain(&mut buf, &[fx]);
            let tail = &buf.left[sr as usize / 4..];
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let near_band = tone(center);
        let far_band = tone(4000.0);
        assert!(
            near_band > far_band * 4.0,
            "spectrum smushed toward red's frequency: {near_band} vs {far_band}"
        );
        // Same hue<->freq axis as the scope video: hsv sanity.
        let (r, _, _) = hsv_to_rgb(5.0, 1.0, 1.0);
        assert!(r > 200);
    }

    #[test]
    fn video_crush_pixelates_and_posterizes() {
        let clip = VideoClip::test_pattern(64, 48, 10.0, 0.2);
        let mut frame = clip.frames[0].clone();
        let fx = AvEffect::new(EffectKind::Crush { downsample: 8.0, bits: 2.0 });
        let mut states = video_chain_states(&[fx], 10.0);
        apply_video_chain(&mut frame, &[fx], &mut states);
        // Pixels within an 8x8 block are identical.
        assert_eq!(frame.get(0, 0), frame.get(7, 7));
        assert_eq!(frame.get(8, 8), frame.get(15, 15));
        // Posterized: few distinct channel values.
        let mut reds: Vec<u8> = frame.data.chunks_exact(4).map(|p| p[0]).collect();
        reds.sort_unstable();
        reds.dedup();
        assert!(reds.len() <= 4, "2 bits -> <=4 levels, got {}", reds.len());
    }

    #[test]
    fn video_delay_ghosts_past_frames() {
        let fx = AvEffect::new(EffectKind::Delay {
            time: 0.25, // 3 frames at 12 fps
            feedback: 0.6,
            mix: 0.8,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        let mut states = video_chain_states(&[fx], 12.0);
        let mut lumas = Vec::new();
        for i in 0..10 {
            // A single bright flash at frame 0, black after.
            let mut f = if i == 0 {
                let mut f = Frame::black(32, 24);
                for px in f.data.chunks_exact_mut(4) {
                    px[0] = 255;
                    px[1] = 255;
                    px[2] = 255;
                }
                f
            } else {
                Frame::transparent(32, 24)
            };
            apply_video_chain(&mut f, &[fx], &mut states);
            lumas.push(luma_on_black(&f));
        }
        // The flash echoes at the delay distance, decaying.
        assert!(lumas[0] > 200.0);
        assert!(lumas[1] < 5.0 && lumas[2] < 5.0, "no ghost before delay");
        assert!(lumas[3] > 20.0, "ghost at delay: {}", lumas[3]);
        assert!(lumas[6] > 2.0 && lumas[6] < lumas[3], "decayed 2nd echo: {}", lumas[6]);
    }

    #[test]
    fn video_reverb_smears_persistence() {
        let fx = AvEffect::new(EffectKind::Reverb { size: 0.8, damp: 0.5, mix: 0.9 });
        let mut states = video_chain_states(&[fx], 12.0);
        let mut lumas = Vec::new();
        for i in 0..12 {
            let mut f = if i < 3 {
                let mut f = Frame::black(32, 24);
                for px in f.data.chunks_exact_mut(4) {
                    px[1] = 255;
                }
                f
            } else {
                Frame::transparent(32, 24)
            };
            apply_video_chain(&mut f, &[fx], &mut states);
            lumas.push(luma_on_black(&f));
        }
        // After the source goes dark, the smear lingers and decays.
        assert!(lumas[3] > 5.0, "persistence right after: {}", lumas[3]);
        assert!(lumas[3] > lumas[6], "decaying: {:?}", lumas);
        assert!(lumas[6] > lumas[10], "still decaying: {:?}", lumas);
    }

    #[test]
    fn video_compress_is_blocky_at_low_quality() {
        let clip = VideoClip::test_pattern(64, 48, 10.0, 0.2);
        let orig = clip.frames[0].clone();

        // Quality 1 ~ identity.
        let mut hi = orig.clone();
        let fx1 = AvEffect::new(EffectKind::Compress { quality: 1.0 });
        let mut st1 = video_chain_states(&[fx1], 10.0);
        apply_video_chain(&mut hi, &[fx1], &mut st1);
        let diff_hi: f64 = orig
            .data
            .iter()
            .zip(&hi.data)
            .map(|(a, b)| (*a as f64 - *b as f64).abs())
            .sum::<f64>()
            / orig.data.len() as f64;
        assert!(diff_hi < 1.0, "quality 1 near-lossless, mean diff {diff_hi}");

        // Low quality visibly shifts even smooth imagery a bit...
        let mut lo = orig.clone();
        let fx0 = AvEffect::new(EffectKind::Compress { quality: 0.05 });
        let mut st0 = video_chain_states(&[fx0], 10.0);
        apply_video_chain(&mut lo, &[fx0], &mut st0);
        let mean: f64 = orig
            .data
            .iter()
            .zip(&lo.data)
            .map(|(a, b)| (*a as f64 - *b as f64).abs())
            .sum::<f64>()
            / orig.data.len() as f64;
        assert!(mean > 0.3, "low quality shifts pixels, mean diff {mean}");

        // ...and rings hard on a sharp edge (the classic JPEG artifact:
        // quantizing away high-frequency DCT terms => Gibbs ringing).
        // Edge deliberately mid-block (x=28) so blocks straddle the step.
        let mut step = Frame::black(64, 48);
        for y in 0..48u32 {
            for x in 28..64u32 {
                step.put(x, y, [255, 255, 255, 255]);
            }
        }
        let step_orig = step.clone();
        let mut st0b = video_chain_states(&[fx0], 10.0);
        apply_video_chain(&mut step, &[fx0], &mut st0b);
        let max = step_orig
            .data
            .iter()
            .zip(&step.data)
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        assert!(max > 20, "edge ringing expected, max diff {max}");
    }

    #[test]
    fn video_dial_zero_bypasses() {
        let clip = VideoClip::test_pattern(64, 48, 10.0, 0.2);
        let orig = clip.frames[0].clone();
        let mut f = orig.clone();
        let mut fx = AvEffect::new(EffectKind::Crush { downsample: 8.0, bits: 2.0 });
        fx.video = 0.0;
        let mut states = video_chain_states(&[fx], 10.0);
        apply_video_chain(&mut f, &[fx], &mut states);
        assert_eq!(orig.data, f.data);
    }

    #[test]
    fn effect_param_setter() {
        let mut fx = AvEffect::new(EffectKind::parse("delay").unwrap());
        fx.set_param("time", 0.7).unwrap();
        fx.set_param("feedback", 0.3).unwrap();
        fx.set_param("video", 0.5).unwrap();
        assert!(matches!(fx.kind, EffectKind::Delay { time, feedback, .. }
            if (time - 0.7).abs() < 1e-6 && (feedback - 0.3).abs() < 1e-6));
        assert_eq!(fx.video, 0.5);
        assert!(fx.set_param("nonsense", 1.0).is_err());
        assert!(EffectKind::parse("flanger").is_none());
    }

    fn goertzel(x: &[f32], sr: f32, f: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * f / sr;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &v in x {
            let s0 = v + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - c * s1 * s2
    }

    #[test]
    fn tune_pulls_noise_onto_the_key() {
        let sr = 48_000u32;
        let n = sr as usize;
        let mut seed = 12345u32;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let mut l: Vec<f32> = (0..n).map(|_| noise() * 0.3).collect();
        let mut r = l.clone();
        let chain = [AvEffect::new(EffectKind::parse("tune").unwrap())]; // A minor pentatonic
        let mut st = audio_chain_states(&chain, sr);
        process_audio_chain(&mut l, &mut r, &chain, &mut st);
        let tail = &l[n / 2..];
        assert!(tail.iter().all(|v| v.is_finite()));
        let on = goertzel(tail, sr as f32, 440.0); // A4, in key
        let off = goertzel(tail, sr as f32, 466.16); // A#4, not in key
        assert!(on > off * 8.0, "on {on} off {off}");
        // Auto-gain keeps it roughly as loud as the input.
        let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(rms > 0.05 && rms < 0.6, "rms {rms}");
    }

    #[test]
    fn tune_snaps_hues_to_key_colors() {
        let hues = key_hues(0.0, crate::music::ScaleKind::Major); // C major
        assert_eq!(hues, vec![0.0, 60.0, 120.0, 150.0, 210.0, 270.0, 330.0]);
        assert_eq!(nearest_hue_delta(100.0, &hues), 20.0);
        assert_eq!(nearest_hue_delta(350.0, &hues), 10.0);
        assert_eq!(key_freqs(9.0, crate::music::ScaleKind::MinorPentatonic)[0], 55.0);
    }

    /// 16x8 frame: left half opaque white, right half transparent black.
    fn half_frame() -> Frame {
        let mut f = Frame::black(16, 8);
        for y in 0..8 {
            for x in 0..16 {
                let px = if x < 8 { [255, 255, 255, 255] } else { [0, 0, 0, 0] };
                f.put(x, y, px);
            }
        }
        f
    }

    fn run_tile(f: &mut Frame, count: f32, mirror: f32, spin: f32) {
        let chain = [AvEffect::new(EffectKind::Tile { count, mirror, spin })];
        let mut st = video_chain_states(&chain, 30.0);
        apply_video_chain(f, &chain, &mut st);
    }

    #[test]
    fn tile_count_one_is_passthrough() {
        let orig = VideoClip::test_pattern(64, 48, 10.0, 0.2).frames[0].clone();
        for mirror in [0.0, 1.0] {
            let mut f = orig.clone();
            run_tile(&mut f, 1.0, mirror, 0.0);
            assert_eq!(orig.data, f.data);
        }
        // Audio: count 1 is bit-exact, mirror or not.
        let sr = 48_000u32;
        let src: Vec<f32> = (0..4800).map(|i| ((i as f32) * 0.37).sin() * 0.5).collect();
        for mirror in [0.0, 1.0] {
            let (mut l, mut r) = (src.clone(), src.iter().map(|v| -v).collect::<Vec<_>>());
            let r0 = r.clone();
            let chain = [AvEffect::new(EffectKind::Tile { count: 1.0, mirror, spin: 0.0 })];
            let mut st = audio_chain_states(&chain, sr);
            process_audio_chain(&mut l, &mut r, &chain, &mut st);
            assert_eq!(l, src);
            assert_eq!(r, r0);
        }
    }

    #[test]
    fn tile_two_repeats_columns_and_carries_alpha() {
        let mut f = half_frame();
        run_tile(&mut f, 2.0, 0.0, 0.0);
        for y in 0..8 {
            for x in 0..16 {
                let want = if (x / 4) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 0] };
                assert_eq!(f.get(x, y), want, "x {x} y {y}");
            }
        }
    }

    #[test]
    fn tile_mirror_flips_alternate_tiles() {
        // Horizontal gradient with a vertical alpha ramp.
        let mut src = Frame::black(16, 8);
        for y in 0..8 {
            for x in 0..16 {
                src.put(x, y, [(x * 16) as u8, 0, (y * 30) as u8, (y * 32 + 3) as u8]);
            }
        }
        let mut plain = src.clone();
        run_tile(&mut plain, 2.0, 0.0, 0.0);
        let mut mir = src.clone();
        run_tile(&mut mir, 2.0, 1.0, 0.0);
        for y in 0..8 {
            for k in 0..8 {
                // Top-left tile identical; the next tile is its mirror.
                if y < 4 {
                    assert_eq!(mir.get(k, y), plain.get(k, y));
                }
                assert_eq!(mir.get(8 + k, y), mir.get(7 - k, y), "k {k} y {y}");
            }
            // Alpha rides along with its pixel: output row y samples source
            // row 2r+1 (r = y in the first tile row, mirrored in the second).
            let r = if y < 4 { y } else { 7 - y };
            assert_eq!(mir.get(0, y)[3], src.get(1, 2 * r + 1)[3]);
            assert_eq!(mir.get(0, y), src.get(1, 2 * r + 1));
        }
        // Vertical tiles flip too (rows 4..8 mirror rows 0..4).
        for x in 0..16 {
            for y in 0..4 {
                assert_eq!(mir.get(x, 4 + y), mir.get(x, 3 - y));
            }
        }
        // Seamless: the mirrored grid has no hard jump at the tile seam.
        let seam = (mir.get(7, 0)[0] as i32 - mir.get(8, 0)[0] as i32).abs();
        assert_eq!(seam, 0);
    }

    #[test]
    fn tile_half_turn_rotates_180() {
        let src = VideoClip::test_pattern(32, 16, 10.0, 0.2).frames[0].clone();
        let mut f = src.clone();
        run_tile(&mut f, 1.0, 0.0, 0.5);
        for y in 0..16 {
            for x in 0..32 {
                assert_eq!(f.get(x, y), src.get(31 - x, 15 - y), "x {x} y {y}");
            }
        }
    }

    #[test]
    fn tile_audio_taps_and_mirror_pingpong() {
        let sr = 48_000u32;
        let n = sr as usize / 4;
        // Left-only impulse.
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        l[0] = 1.0;
        let chain = [AvEffect::new(EffectKind::Tile { count: 3.0, mirror: 0.0, spin: 0.0 })];
        let mut st = audio_chain_states(&chain, sr);
        process_audio_chain(&mut l, &mut r, &chain, &mut st);
        assert_eq!(l[0], 1.0);
        assert!(r.iter().all(|v| *v == 0.0), "no cross-feed without mirror");
        let taps: Vec<usize> = (1..n).filter(|&i| l[i].abs() > 1e-4).collect();
        assert!(!taps.is_empty());
        assert!(taps.iter().all(|&i| i < (0.13 * sr as f32) as usize), "taps within ~120 ms");
        let wet: f32 = l[1..].iter().sum();
        assert!(wet > 0.05 && wet < 0.2, "subtle wet {wet}"); // 0.35 * 2/7 = 0.1
        // Mirror: the first echo lands on the opposite side.
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        l[0] = 1.0;
        let chain = [AvEffect::new(EffectKind::Tile { count: 3.0, mirror: 1.0, spin: 0.0 })];
        let mut st = audio_chain_states(&chain, sr);
        process_audio_chain(&mut l, &mut r, &chain, &mut st);
        assert!(r.iter().map(|v| v.abs()).sum::<f32>() > 0.01);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        // Block-size independence (streaming state).
        let src: Vec<f32> = (0..n).map(|i| ((i as f32) * 0.05).sin()).collect();
        let (mut a, mut b) = (src.clone(), src.clone());
        let chain = [AvEffect::new(EffectKind::Tile { count: 5.5, mirror: 1.0, spin: 0.0 })];
        let mut st = audio_chain_states(&chain, sr);
        process_audio_chain(&mut a, &mut b, &chain, &mut st);
        let (mut c, mut d) = (src.clone(), src.clone());
        let mut st = audio_chain_states(&chain, sr);
        for (cc, dd) in c.chunks_mut(333).zip(d.chunks_mut(333)) {
            process_audio_chain(cc, dd, &chain, &mut st);
        }
        assert_eq!(a, c);
        assert_eq!(b, d);
    }

    #[test]
    fn tile_parse_and_params() {
        assert_eq!(
            EffectKind::parse("tile"),
            Some(EffectKind::Tile { count: 2.0, mirror: 0.0, spin: 0.0 })
        );
        assert_eq!(EffectKind::parse("grid"), EffectKind::parse("tiles"));
        assert_eq!(
            EffectKind::parse("kaleido"),
            Some(EffectKind::Tile { count: 3.0, mirror: 1.0, spin: 0.0 })
        );
        let mut fx = AvEffect::new(EffectKind::parse("tile").unwrap());
        fx.set_param("tiles", 12.0).unwrap();
        fx.set_param("mirror", 1.0).unwrap();
        fx.set_param("turn", 0.125).unwrap();
        assert_eq!(fx.kind, EffectKind::Tile { count: 8.0, mirror: 1.0, spin: 0.125 });
        assert!(fx.set_param("hue", 1.0).is_err());
        for name in EffectKind::KINDS {
            let k = EffectKind::parse(name).unwrap();
            assert_eq!(k.name(), name);
            assert!(!k.blurb().is_empty());
        }
    }
}

