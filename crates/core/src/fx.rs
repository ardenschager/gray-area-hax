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
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            EffectKind::Crush { .. } => "crush",
            EffectKind::Delay { .. } => "delay",
            EffectKind::Reverb { .. } => "reverb",
            EffectKind::Compress { .. } => "compress",
        }
    }
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
        })
        .collect()
}

/// Samples of latency the chain introduces (spectral compression is
/// windowed and cannot be zero-latency in a stream).
pub fn audio_chain_latency(chain: &[AvEffect]) -> usize {
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
            _ => debug_assert!(false, "chain/state mismatch — rebuild states"),
        }
    }
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

/// Freeverb-style reverb tank for one channel (4 combs + 2 allpasses).
pub struct ReverbChannel {
    combs: Vec<Comb>,
    aps: Vec<AllPass>,
}

impl ReverbChannel {
    fn new(sample_rate: u32, size: f32, damp: f32, offset: f32) -> ReverbChannel {
        let sr_scale = sample_rate as f32 / 44100.0;
        let feedback = 0.7 + 0.28 * size.clamp(0.0, 1.0);
        let damp = damp.clamp(0.0, 1.0) * 0.8;
        ReverbChannel {
            combs: [1116.0f32, 1188.0, 1277.0, 1356.0]
                .iter()
                .map(|t| Comb::new(((t + offset) * sr_scale) as usize, feedback, damp))
                .collect(),
            aps: [556.0f32, 441.0]
                .iter()
                .map(|t| AllPass::new(((t + offset) * sr_scale) as usize))
                .collect(),
        }
    }

    fn process(&mut self, samples: &mut [f32], mix: f32) {
        for s in samples.iter_mut() {
            let input = *s * 0.25;
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
            _ => VideoFxState::Stateless,
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
        }
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
}
