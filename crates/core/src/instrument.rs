//! `Instrument`: chromagrain as a playable, plugin-shaped video sampler.
//!
//! This is the seam the play-first UI, the standalone app and the plugin
//! wrappers (nih-plug VST3/CLAP, a Swift AUv3 over a C ABI) all drive. It
//! is deliberately small and platform-free:
//!
//! * **No threads, no file IO, no ffmpeg, no locks.** Media arrives already
//!   decoded ([`Source`] / [`AudioClip`] / [`VideoClip`], or raw PCM + RGBA
//!   via [`Instrument::add_source_pcm_rgba`]); the host side decodes.
//! * **One audio call:** [`Instrument::process`] fills a stereo block.
//!   Notes, macros and transport are applied at block start; hosts that
//!   want sample-accurate events split the block at event offsets (as
//!   nih-plug does).
//! * **MIDI:** [`Instrument::note_on`] / [`Instrument::note_off`], velocity
//!   0..1. C4 (60) plays the media at its own pitch (sampler mapping).
//! * **Four macros** ([`Macro`]), each 0..1, mapped onto the existing
//!   engine parameters (see [`MacroMap`]):
//!   GRAIN = the sample ↔ cloud texture morph, PITCH = transposition snapped
//!   to the global key, SMEAR = read-head time smear + echo + reverb, LOOK =
//!   steps through the one-click AV looks.
//! * **Host tempo/transport:** [`Instrument::set_transport`] (bpm, playing,
//!   beat). Drives beat-synced grain onsets ([`Instrument::set_sync`]) and,
//!   optionally, a read head locked to the song position
//!   ([`Instrument::set_follow_transport`]).
//! * **Picture:** every audio grain is a visual grain. After each block,
//!   [`Instrument::frame`] / [`Instrument::fill_frame`] snapshot what is on
//!   screen as plain data ([`FrameState`]: grains + style + video effect
//!   chain) for a GPU renderer, and [`FrameRenderer`] rasterizes that on the
//!   CPU (thumbnails, headless, tests). The `(source, GrainEvent)` grain
//!   list is exactly what the app's GPU preview already draws for live pad
//!   hits.
//!
//! C ABI shape: every method takes scalars, `&[f32]` / `&[u8]` slices or
//! small `Copy` structs ([`Transport`], [`Macro`] as `u32`), so a cbindgen
//! layer is `Box<Instrument>` behind an opaque pointer plus one `extern "C"`
//! fn per method; [`FrameState::grains`] flattens to a `#[repr(C)]` array.
//!
//! Realtime notes: `process` does not allocate in steady state (grain and
//! scratch buffers are reserved up front and reused; the voice and grain
//! pools are capped). Adding/replacing sources and changing the key do
//! allocate and belong on the host's main thread (or a param-change queue).

use crate::audio::{render_grains_audio, AudioClip, StereoBuffer};
use crate::dsp::Rng;
use crate::fx::{self, AudioFxState, AvEffect, EffectKind, VideoFxState};
use crate::grain::GrainEvent;
use crate::music::{semitones_to_ratio, Key};
use crate::timeline::{ClipTransform, Source, SourceId};
use crate::video::{
    blend_layer, composite_grains_frame, AvLink, Frame, VideoClip, VisualGrainStyle,
};
use std::sync::Arc;

/// The four performance macros.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Macro {
    /// Sample ↔ cloud: 0 = the media plays straight, 1 = a grain cloud.
    Grain = 0,
    /// Transposition, -12..+12 semitones (0.5 = none), snapped to the key.
    Pitch = 1,
    /// Time smear: the read head slows toward a freeze while echo and
    /// reverb (sound AND picture) swell.
    Smear = 2,
    /// Steps through [`LOOKS`] (0 = no look).
    Look = 3,
}

impl Macro {
    pub const ALL: [Macro; 4] = [Macro::Grain, Macro::Pitch, Macro::Smear, Macro::Look];

    pub fn from_index(i: u32) -> Option<Macro> {
        Macro::ALL.get(i as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Macro::Grain => "grain",
            Macro::Pitch => "pitch",
            Macro::Smear => "smear",
            Macro::Look => "look",
        }
    }

    /// Value a fresh instrument starts with.
    pub fn default_value(self) -> f32 {
        match self {
            Macro::Pitch => 0.5,
            _ => 0.0,
        }
    }
}

/// The one-click looks the LOOK macro steps through, in order (parse
/// names for [`EffectKind::parse`]; `None` = no look). Echo/reverb are left
/// to SMEAR.
pub const LOOKS: [Option<&str>; 7] =
    [None, Some("tune"), Some("crush"), Some("compress"), Some("tint"), Some("tile"), Some("kaleido")];

/// Host tempo + transport, as plugin hosts report it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    pub bpm: f64,
    pub playing: bool,
    /// Song position in beats (quarter notes) at the start of the next block.
    pub beat: f64,
}

impl Default for Transport {
    fn default() -> Self {
        Transport { bpm: 120.0, playing: false, beat: 0.0 }
    }
}

/// Engine parameters the macros resolve to — a pure function of the macro
/// values and key, so UIs can display them and tests can pin them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacroMap {
    /// Sample ↔ cloud morph (`GrainSettings::texture`).
    pub texture: f32,
    /// Transposition in semitones (already snapped to the key).
    pub pitch_st: f32,
    /// Read-head speed: 1 = realtime, → 1/16 = nearly frozen.
    pub speed: f32,
    /// Echo feedback / wet, reverb wet (audio and video alike).
    pub echo_feedback: f32,
    pub echo_mix: f32,
    pub reverb_mix: f32,
    /// Index into [`LOOKS`].
    pub look: usize,
}

impl MacroMap {
    pub fn resolve(macros: &[f32; 4], key: Option<&Key>) -> MacroMap {
        let [g, p, s, l] = macros.map(|v| v.clamp(0.0, 1.0));
        let raw = (-12.0 + 24.0 * p).round() as i32;
        let pitch_st = match key {
            Some(k) => snap_interval(raw, k) as f32,
            None => raw as f32,
        };
        MacroMap {
            texture: g,
            pitch_st,
            speed: 2f32.powf(-4.0 * s),
            echo_feedback: 0.6 * s,
            echo_mix: 0.4 * s,
            reverb_mix: 0.6 * s,
            look: ((l * LOOKS.len() as f32) as usize).min(LOOKS.len() - 1),
        }
    }
}

/// The nearest transposition (in semitones) that keeps the key's root on a
/// scale tone — a diatonic shift. Ties go toward zero.
fn snap_interval(st: i32, key: &Key) -> i32 {
    let in_scale = |n: i32| key.scale.intervals().contains(&n.rem_euclid(12));
    (0..12)
        .flat_map(|d| [st - d, st + d])
        .find(|&n| in_scale(n) && (n - st).abs() <= 6)
        .unwrap_or(st)
}

// Cloud end of the GRAIN morph (mirrors the snippet texture morph in
// `render::textured_events`): long seamless windows -> these.
const CLOUD_DURATION: f32 = 0.08;
const CLOUD_DENSITY: f32 = 30.0;
const CLOUD_SPRAY: f32 = 0.3;
const CLOUD_PAN_SPREAD: f32 = 0.8;
const CLOUD_DUR_JITTER: f32 = 0.3;
const CLOUD_REVERSE: f32 = 0.15;
/// Scale-step pitch scatter of the cloud (only with a key, so it stays
/// in key).
const CLOUD_PITCH_JITTER: f32 = 7.0;
const SAMPLE_DURATION: f32 = 0.30;
const BASE_GAIN: f32 = 0.8;
/// Grains are scheduled this far ahead of the block end so onset jitter
/// never lands before the current block (keeps output independent of
/// block size). Must exceed the max onset jitter (0.5 × the longest hop).
const LOOKAHEAD: f64 = 0.1;
const MAX_VOICES: usize = 16;
const MAX_GRAINS: usize = 4096;
const ECHO_TIME: f32 = 0.33;

/// The cloud end of the GRAIN morph, overridable per emitter / voice
/// ("deep" settings). Defaults are the instrument's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cloud {
    /// Grain length at GRAIN = 1, seconds.
    pub duration: f32,
    /// Grains per second at GRAIN = 1.
    pub density: f32,
    /// Read-position scatter at GRAIN = 1, seconds.
    pub spray: f32,
    pub pan_spread: f32,
    /// Pitch scatter at GRAIN = 1 (scale steps when a key is set).
    pub pitch_jitter: f32,
    /// Probability of reversed grains (cloud half of the morph).
    pub reverse: f32,
}

impl Default for Cloud {
    fn default() -> Self {
        Cloud {
            duration: CLOUD_DURATION,
            density: CLOUD_DENSITY,
            spray: CLOUD_SPRAY,
            pan_spread: CLOUD_PAN_SPREAD,
            pitch_jitter: CLOUD_PITCH_JITTER,
            reverse: CLOUD_REVERSE,
        }
    }
}

/// One grain of a held note — THE grain definition shared by the
/// instrument, the app's emitters and the plugin. `head` is the read head
/// (source seconds); returns the event (onset 0: the caller places it) and
/// the hop to the next grain. The caller advances its head by
/// `hop * map.speed`.
#[allow(clippy::too_many_arguments)]
pub fn grain_step(
    map: &MacroMap,
    cloud: &Cloud,
    key: Option<&Key>,
    rng: &mut Rng,
    head: f64,
    src_len: f64,
    note: u8,
    velocity: f32,
    id: u64,
) -> (GrainEvent, f64) {
    let tex = map.texture;
    let dur = ((SAMPLE_DURATION.ln() + (cloud.duration.max(0.005).ln() - SAMPLE_DURATION.ln()) * tex).exp()
        * (1.0 + tex * CLOUD_DUR_JITTER * rng.bipolar()))
    .max(0.005) as f64;
    let cloud_hop = 1.0 / cloud.density.max(0.5) as f64;
    let hop = (dur / 2.0 + (cloud_hop - dur / 2.0) * tex as f64).max(0.002);
    let jitter = tex as f64 * hop * 0.5 * rng.bipolar() as f64;
    let pos = head + tex as f64 * cloud.spray as f64 * rng.bipolar() as f64;
    let base = note as f32 + map.pitch_st;
    let scatter = tex * cloud.pitch_jitter * rng.bipolar();
    let midi = match key {
        Some(k) if scatter.abs() > 1e-4 => base + k.quantize_midi(base + scatter) - k.quantize_midi(base),
        _ => base,
    };
    let pan = (tex * cloud.pan_spread * rng.bipolar()).clamp(-1.0, 1.0);
    let reverse = tex >= 0.5 && rng.next_f32() < cloud.reverse;
    let overlap = (dur / hop).max(0.25) as f32;
    let norm = (overlap * 0.5).powf(-(1.0 - 0.5 * tex));
    let ev = GrainEvent {
        onset: jitter, // relative: the caller adds the scheduled time
        source_pos: pos.rem_euclid(src_len.max(1e-6)),
        duration: dur as f32,
        pitch_ratio: semitones_to_ratio(midi - 60.0),
        gain: BASE_GAIN * velocity * norm,
        pan,
        envelope: 1.0,
        env_skew: 0.0,
        reverse,
        id,
    };
    (ev, hop)
}

struct Voice {
    note: u8,
    velocity: f32,
    source: SourceId,
    /// Read head, seconds into the source.
    head: f64,
    /// Instrument-clock time of the next grain.
    next_t: f64,
    start_t: f64,
    serial: u64,
    rng: Rng,
    k: u64,
}

#[derive(Clone)]
struct LiveGrain {
    source: SourceId,
    voice: u64,
    ev: GrainEvent,
}

/// What the picture is at one moment — plain data for a renderer.
#[derive(Debug, Clone, Default)]
pub struct FrameState {
    /// Instrument-clock seconds this snapshot describes (end of the last
    /// processed block). Grain onsets are on the same clock.
    pub time: f64,
    pub beat: f64,
    pub playing: bool,
    /// Grains to draw (some may start slightly after `time`; renderers
    /// interpolating forward in time can draw them as they begin).
    pub grains: Vec<(SourceId, GrainEvent)>,
    pub style: VisualGrainStyle,
    pub link: AvLink,
    /// Video effect chain to run over the grain layer, in order (SMEAR's
    /// echo + reverb, then the LOOK).
    pub effects: Vec<AvEffect>,
    /// Index into [`LOOKS`].
    pub look: usize,
}

/// The playable instrument. See the module docs.
pub struct Instrument {
    sample_rate: u32,
    sources: Vec<Source>,
    active: Option<SourceId>,
    macros: [f32; 4],
    key: Option<Key>,
    /// Beat grid for grain onsets (beats; 0 = free).
    sync_div: f32,
    follow_transport: bool,
    /// Normalized start position of new notes in the media (canvas x).
    position: f32,

    transport: Transport,
    /// Instrument-clock seconds at which `transport.beat` was reported.
    anchor_t: f64,
    /// Samples processed since construction (the instrument clock).
    clock: u64,

    voices: Vec<Voice>,
    serial: u64,
    grains: Vec<LiveGrain>,

    smear_chain: [AvEffect; 2],
    smear_states: Vec<AudioFxState>,
    looks: Vec<Option<AvEffect>>,
    look_states: Vec<Vec<AudioFxState>>,
    scratch: StereoBuffer,
}

impl Instrument {
    pub fn new(sample_rate: u32) -> Instrument {
        let sample_rate = sample_rate.max(1);
        let smear_chain = [
            AvEffect::new(EffectKind::Delay {
                time: ECHO_TIME,
                feedback: 0.0,
                mix: 0.0,
                shift_x: 0.02,
                shift_y: 0.0,
            }),
            AvEffect::new(EffectKind::Reverb { size: 0.85, damp: 0.4, mix: 0.0 }),
        ];
        let looks: Vec<Option<AvEffect>> =
            LOOKS.iter().map(|n| n.and_then(EffectKind::parse).map(AvEffect::new)).collect();
        let mut inst = Instrument {
            sample_rate,
            sources: Vec::new(),
            active: None,
            macros: Macro::ALL.map(Macro::default_value),
            key: None,
            sync_div: 0.0,
            follow_transport: false,
            position: 0.0,
            transport: Transport::default(),
            anchor_t: 0.0,
            clock: 0,
            voices: Vec::with_capacity(MAX_VOICES),
            serial: 0,
            grains: Vec::with_capacity(MAX_GRAINS),
            smear_states: Vec::new(),
            smear_chain,
            looks,
            look_states: Vec::new(),
            scratch: StereoBuffer { left: Vec::new(), right: Vec::new(), sample_rate },
        };
        inst.rebuild_fx();
        inst
    }

    /// Change the output rate (plugin re-activation). Silences everything.
    pub fn reset(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        self.scratch.sample_rate = self.sample_rate;
        self.voices.clear();
        self.grains.clear();
        self.clock = 0;
        self.anchor_t = 0.0;
        self.rebuild_fx();
    }

    fn rebuild_fx(&mut self) {
        let sr = self.sample_rate;
        self.smear_states = fx::audio_chain_states(&self.smear_chain, sr);
        if let Some(k) = self.key {
            for fx in self.looks.iter_mut().flatten() {
                if let EffectKind::Tune { root, scale, .. } = &mut fx.kind {
                    *root = k.root as f32;
                    *scale = k.scale;
                }
            }
        }
        self.look_states = self
            .looks
            .iter()
            .map(|l| l.as_ref().map(|fx| fx::audio_chain_states(std::slice::from_ref(fx), sr)).unwrap_or_default())
            .collect();
        // Room for the largest block a host is likely to send.
        self.scratch.left.reserve(8192);
        self.scratch.right.reserve(8192);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    // ------------------------------------------------------------ sources

    /// Hand over decoded media. The first source added becomes active.
    pub fn add_source(&mut self, source: Source) -> SourceId {
        self.sources.push(source);
        let id = self.sources.len() - 1;
        self.active.get_or_insert(id);
        id
    }

    /// Raw-data entry point (what a C ABI wraps): `pcm` is mono
    /// (`channels` = 1) or interleaved stereo; `rgba` is `n_frames`
    /// tightly packed `width × height` RGBA8 frames. Either may be empty.
    #[allow(clippy::too_many_arguments)]
    pub fn add_source_pcm_rgba(
        &mut self,
        name: &str,
        pcm: &[f32],
        channels: u32,
        pcm_rate: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
        fps: f32,
        base_hz: f32,
    ) -> SourceId {
        let audio = (!pcm.is_empty() && pcm_rate > 0).then(|| {
            Arc::new(if channels >= 2 {
                let n = pcm.len() / channels as usize;
                let ch = channels as usize;
                let l: Vec<f32> = (0..n).map(|i| pcm[i * ch]).collect();
                let r: Vec<f32> = (0..n).map(|i| pcm[i * ch + 1]).collect();
                AudioClip::stereo(&l, &r, pcm_rate)
            } else {
                AudioClip::new(pcm.to_vec(), pcm_rate)
            })
        });
        let frame_bytes = (width as usize) * (height as usize) * 4;
        let video = (frame_bytes > 0 && rgba.len() >= frame_bytes && fps > 0.0).then(|| {
            let frames = rgba
                .chunks_exact(frame_bytes)
                .map(|d| Frame { width, height, data: d.to_vec() })
                .collect();
            Arc::new(VideoClip { frames, fps })
        });
        self.add_source(Source { name: name.into(), audio, video, base_hz })
    }

    /// Swap the media behind an id (sounding grains keep their old audio
    /// until they end only if the id still resolves; ids are stable).
    pub fn replace_source(&mut self, id: SourceId, source: Source) -> bool {
        match self.sources.get_mut(id) {
            Some(s) => {
                *s = source;
                true
            }
            None => false,
        }
    }

    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Which source new notes play.
    pub fn select_source(&mut self, id: SourceId) -> bool {
        if id < self.sources.len() {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    pub fn active_source(&self) -> Option<SourceId> {
        self.active
    }

    // ------------------------------------------------------------ controls

    pub fn set_macro(&mut self, m: Macro, value: f32) {
        self.macros[m as usize] = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
    }

    pub fn macro_value(&self, m: Macro) -> f32 {
        self.macros[m as usize]
    }

    pub fn macro_map(&self) -> MacroMap {
        MacroMap::resolve(&self.macros, self.key.as_ref())
    }

    /// The one global key (None = chromatic). Allocates (rebuilds the
    /// tune look's resonators).
    pub fn set_key(&mut self, key: Option<Key>) {
        self.key = key;
        self.rebuild_fx();
    }

    pub fn key(&self) -> Option<Key> {
        self.key
    }

    /// Snap grain onsets to a beat grid (0.25 = sixteenths). 0 = free.
    pub fn set_sync(&mut self, div_beats: f32) {
        self.sync_div = div_beats.clamp(0.0, 4.0);
    }

    /// Lock the read head of new notes to the song position while the
    /// host plays, so scrubbing the DAW scrubs the picture.
    pub fn set_follow_transport(&mut self, on: bool) {
        self.follow_transport = on;
    }

    /// Where in the media new notes start, 0..1 (the canvas x axis).
    pub fn set_position(&mut self, pos: f32) {
        self.position = pos.clamp(0.0, 1.0);
    }

    /// Host transport for the next block.
    pub fn set_transport(&mut self, t: Transport) {
        self.transport = Transport { bpm: t.bpm.clamp(1.0, 999.0), ..t };
        self.anchor_t = self.now_secs();
    }

    /// Transport as of now (beat advanced through processed blocks).
    pub fn transport(&self) -> Transport {
        Transport { beat: self.beat_at(self.now_secs()), ..self.transport }
    }

    /// Instrument clock: seconds of audio processed.
    pub fn now_secs(&self) -> f64 {
        self.clock as f64 / self.sample_rate as f64
    }

    /// Song beat at instrument time `t`. Frozen while the host is stopped.
    fn beat_at(&self, t: f64) -> f64 {
        if self.transport.playing {
            self.transport.beat + (t - self.anchor_t) * self.transport.bpm / 60.0
        } else {
            self.transport.beat
        }
    }

    /// Beat on the grid clock at `t`: follows the song while playing,
    /// free-runs at the same tempo while stopped.
    fn grid_beat(&self, t: f64) -> f64 {
        self.transport.beat + (t - self.anchor_t) * self.transport.bpm / 60.0
    }

    fn grid_time(&self, beat: f64) -> f64 {
        self.anchor_t + (beat - self.transport.beat) * 60.0 / self.transport.bpm
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        let Some(source) = self.active else { return };
        let Some(src) = self.sources.get(source) else { return };
        let len = src.duration();
        if len <= 0.0 || velocity <= 0.0 {
            if velocity <= 0.0 {
                self.note_off(note); // MIDI convention
            }
            return;
        }
        self.note_off(note);
        if self.voices.len() >= MAX_VOICES {
            let oldest = self.voices.remove(0);
            self.cut_future(oldest.serial);
        }
        let now = self.now_secs();
        let mut head = self.position as f64 * len;
        if self.follow_transport && self.transport.playing {
            head += self.beat_at(now) * 60.0 / self.transport.bpm;
        }
        self.serial += 1;
        self.voices.push(Voice {
            note,
            velocity: velocity.min(1.0),
            source,
            head: head.rem_euclid(len),
            next_t: now,
            start_t: now,
            serial: self.serial,
            rng: Rng::new(0xC0FFEE ^ self.serial.wrapping_mul(0x9E37) ^ note as u64),
            k: 0,
        });
    }

    /// Stop a note: no new grains; sounding grains ring out (the release).
    pub fn note_off(&mut self, note: u8) {
        if let Some(i) = self.voices.iter().position(|v| v.note == note) {
            let v = self.voices.remove(i);
            self.cut_future(v.serial);
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in std::mem::take(&mut self.voices) {
            self.cut_future(v.serial);
        }
    }

    pub fn held_notes(&self) -> impl Iterator<Item = u8> + '_ {
        self.voices.iter().map(|v| v.note)
    }

    /// Drop a voice's look-ahead grains that have not started yet.
    fn cut_future(&mut self, serial: u64) {
        let now = self.now_secs();
        self.grains.retain(|g| !(g.voice == serial && g.ev.onset >= now));
    }

    /// Samples of latency the current look adds (report to the host).
    pub fn latency_samples(&self) -> usize {
        let look = self.macro_map().look;
        self.looks[look].as_ref().map(|fx| fx::audio_chain_latency(std::slice::from_ref(fx))).unwrap_or(0)
    }

    // ------------------------------------------------------------ audio

    /// Fill one stereo block. `left` and `right` must be the same length.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        left.fill(0.0);
        right.fill(0.0);
        if n == 0 {
            return;
        }
        let sr = self.sample_rate as f64;
        let t0 = self.now_secs();
        let t1 = t0 + n as f64 / sr;
        let map = self.macro_map();

        self.schedule(t1 + LOOKAHEAD, &map);

        // Grains -> scratch (exactly n samples, no realloc in steady state).
        self.scratch.left.clear();
        self.scratch.right.clear();
        self.scratch.left.resize(n, 0.0);
        self.scratch.right.resize(n, 0.0);
        for g in &self.grains {
            if g.ev.onset >= t1 || g.ev.end() <= t0 {
                continue;
            }
            let Some(audio) = self.sources.get(g.source).and_then(|s| s.audio.as_deref()) else {
                continue;
            };
            render_grains_audio(audio, std::slice::from_ref(&g.ev), &mut self.scratch, t0);
        }
        left.copy_from_slice(&self.scratch.left);
        right.copy_from_slice(&self.scratch.right);
        self.grains.retain(|g| g.ev.end() > t1);

        // SMEAR: echo + reverb, wet amounts live.
        if let EffectKind::Delay { feedback, mix, .. } = &mut self.smear_chain[0].kind {
            *feedback = map.echo_feedback;
            *mix = map.echo_mix;
        }
        if let EffectKind::Reverb { mix, .. } = &mut self.smear_chain[1].kind {
            *mix = map.reverb_mix;
        }
        fx::process_audio_chain(left, right, &self.smear_chain, &mut self.smear_states);
        // LOOK.
        if let Some(fx) = &self.looks[map.look] {
            fx::process_audio_chain(left, right, std::slice::from_ref(fx), &mut self.look_states[map.look]);
        }
        // Same soft clip as the master bus.
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s = if s.abs() > 1.0 { s.tanh() } else { s.tanh() * 0.2 + *s * 0.8 };
        }
        self.clock += n as u64;
    }

    /// Emit every held voice's grains with onset before `until`. A
    /// streaming version of the snippet texture morph: at GRAIN 0 a
    /// seamless 50%-overlap hann train (the media, straight); toward 1 the
    /// grains shrink, thin to the cloud density and scatter in onset,
    /// position, pan and (in key) pitch. Loudness stays level.
    fn schedule(&mut self, until: f64, map: &MacroMap) {
        let sync = (self.sync_div > 0.0).then_some(self.sync_div as f64);
        for vi in 0..self.voices.len() {
            let src_len = self.sources.get(self.voices[vi].source).map(|s| s.duration()).unwrap_or(0.0);
            if src_len <= 0.0 {
                continue;
            }
            while self.voices[vi].next_t < until {
                if self.grains.len() >= MAX_GRAINS {
                    return;
                }
                let key = self.key;
                let cloud = Cloud::default();
                let v = &mut self.voices[vi];
                let id = v.serial.wrapping_mul(1 << 20).wrapping_add(v.k);
                let (mut ev, hop) =
                    grain_step(map, &cloud, key.as_ref(), &mut v.rng, v.head, src_len, v.note, v.velocity, id);
                ev.onset = (v.next_t + ev.onset).max(v.start_t);
                let (source, serial) = (v.source, v.serial);
                v.head += hop * map.speed as f64;
                v.k += 1;
                let mut next = v.next_t + hop;
                if let Some(div) = sync {
                    // Next grid line at or after the free-running time.
                    let b = (self.grid_beat(next) / div - 1e-9).ceil() * div;
                    next = self.grid_time(b).max(self.voices[vi].next_t + 1e-4);
                }
                self.voices[vi].next_t = next;
                self.grains.push(LiveGrain { source, voice: serial, ev });
            }
        }
    }

    // ------------------------------------------------------------ picture

    /// Snapshot of the picture at the instrument's current time.
    pub fn frame(&self) -> FrameState {
        let mut f = FrameState::default();
        self.fill_frame(&mut f);
        f
    }

    /// [`Instrument::frame`] into an existing snapshot, reusing its
    /// buffers (no allocation once warmed up — safe on the audio thread).
    pub fn fill_frame(&self, out: &mut FrameState) {
        let map = self.macro_map();
        let now = self.now_secs();
        out.time = now;
        out.beat = self.beat_at(now);
        out.playing = self.transport.playing;
        out.grains.clear();
        out.grains.extend(self.grains.iter().map(|g| (g.source, g.ev.clone())));
        out.style = VisualGrainStyle::morph(0.35, &VisualGrainStyle::default(), map.texture);
        out.link = AvLink::default();
        out.effects.clear();
        let mut smear = self.smear_chain;
        if let EffectKind::Delay { feedback, mix, .. } = &mut smear[0].kind {
            *feedback = map.echo_feedback;
            *mix = map.echo_mix;
        }
        if let EffectKind::Reverb { mix, .. } = &mut smear[1].kind {
            *mix = map.reverb_mix;
        }
        out.effects.extend(smear);
        out.effects.extend(self.looks[map.look]);
        out.look = map.look;
    }

    /// Grains visible at instrument time `t` (a subset of the snapshot).
    pub fn video_frame_events(&self, t: f64) -> impl Iterator<Item = (SourceId, &GrainEvent)> + '_ {
        self.grains.iter().filter(move |g| t >= g.ev.onset && t < g.ev.end()).map(|g| (g.source, &g.ev))
    }
}

/// CPU rasterizer for [`FrameState`]s — the reference picture (thumbnails,
/// headless renders, tests). Holds the temporal video-effect state, so
/// feed it consecutive frames.
#[derive(Default)]
pub struct FrameRenderer {
    states: Vec<VideoFxState>,
    shape: Vec<&'static str>,
}

impl FrameRenderer {
    pub fn new() -> FrameRenderer {
        FrameRenderer::default()
    }

    /// Draw `frame` at time `t` (pass `frame.time` for "now") into `out`.
    pub fn render(&mut self, frame: &FrameState, sources: &[Source], t: f64, fps: f32, out: &mut Frame) {
        let shape: Vec<&'static str> = frame.effects.iter().map(|f| f.kind.name()).collect();
        if shape != self.shape {
            self.states = fx::video_chain_states(&frame.effects, fps);
            self.shape = shape;
        }
        let (w, h) = (out.width, out.height);
        let mut layer = Frame::transparent(w, h);
        for (sid, ev) in &frame.grains {
            let Some(video) = sources.get(*sid).and_then(|s| s.video.as_deref()) else { continue };
            composite_grains_frame(
                video,
                std::slice::from_ref(ev),
                &frame.style,
                &frame.link,
                &ClipTransform::default(),
                None,
                &mut layer,
                t,
            );
        }
        fx::apply_video_chain(&mut layer, &frame.effects, &mut self.states);
        *out = Frame::black(w, h);
        blend_layer(out, &layer, frame.style.additive, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::ScaleKind;

    const SR: u32 = 48000;

    fn sine_source(hz: f32, secs: f32) -> Source {
        Source {
            name: "sine".into(),
            audio: Some(Arc::new(AudioClip::sine(hz, secs, SR))),
            video: Some(Arc::new(VideoClip::test_pattern(32, 18, 12.0, secs))),
            base_hz: hz,
        }
    }

    fn inst_with_sine() -> Instrument {
        let mut i = Instrument::new(SR);
        i.add_source(sine_source(220.0, 3.0));
        i
    }

    /// Run `secs` of audio in `block`-sized chunks.
    fn run(i: &mut Instrument, secs: f64, block: usize) -> (Vec<f32>, Vec<f32>) {
        let total = (secs * SR as f64) as usize;
        let (mut l, mut r) = (vec![0.0; total], vec![0.0; total]);
        let mut pos = 0;
        while pos < total {
            let b = block.min(total - pos);
            i.process(&mut l[pos..pos + b], &mut r[pos..pos + b]);
            pos += b;
        }
        (l, r)
    }

    /// Every grain scheduled over `secs` (deduplicated by id).
    fn collect(i: &mut Instrument, secs: f64, block: usize) -> Vec<GrainEvent> {
        let mut all: Vec<GrainEvent> = Vec::new();
        let blocks = (secs * SR as f64 / block as f64).ceil() as usize;
        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        for _ in 0..blocks {
            i.process(&mut l, &mut r);
            for (_, e) in i.frame().grains {
                if !all.iter().any(|a| a.id == e.id) {
                    all.push(e);
                }
            }
        }
        all
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Zero-crossing pitch estimate.
    fn pitch_hz(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 * SR as f32 / x.len() as f32
    }

    #[test]
    fn silent_without_notes_or_sources() {
        let mut empty = Instrument::new(SR);
        empty.note_on(60, 1.0); // no source: ignored
        let (l, r) = run(&mut empty, 0.1, 256);
        assert_eq!(rms(&l) + rms(&r), 0.0);
        let mut i = inst_with_sine();
        let (l, _) = run(&mut i, 0.1, 256);
        assert_eq!(rms(&l), 0.0);
    }

    #[test]
    fn note_sounds_then_rings_out_after_note_off() {
        let mut i = inst_with_sine();
        i.note_on(60, 1.0);
        let (l, _) = run(&mut i, 0.5, 512);
        assert!(rms(&l[SR as usize / 10..]) > 0.05, "held note is audible");
        i.note_off(60);
        let (tail, _) = run(&mut i, 0.6, 512);
        assert!(rms(&tail[SR as usize * 4 / 10..]) < 1e-6, "silent after the release");
        assert_eq!(i.held_notes().count(), 0);
    }

    #[test]
    fn velocity_scales_level() {
        let level = |v: f32| {
            let mut i = inst_with_sine();
            i.note_on(60, v);
            let (l, _) = run(&mut i, 0.5, 512);
            rms(&l[SR as usize / 5..])
        };
        let (soft, loud) = (level(0.25), level(1.0));
        assert!(loud > soft * 2.5, "loud {loud} vs soft {soft}");
    }

    #[test]
    fn sampler_mapping_c4_is_unity_and_c5_is_an_octave_up() {
        for (note, want) in [(60u8, 220.0f32), (72, 440.0)] {
            let mut i = inst_with_sine();
            i.note_on(note, 1.0);
            let (l, _) = run(&mut i, 1.0, 512);
            let hz = pitch_hz(&l[SR as usize / 4..]);
            assert!((hz - want).abs() / want < 0.03, "note {note}: {hz} Hz, want {want}");
        }
    }

    #[test]
    fn output_is_independent_of_block_size() {
        let render = |block: usize| {
            let mut i = inst_with_sine();
            i.set_macro(Macro::Grain, 0.7);
            i.set_macro(Macro::Smear, 0.4);
            i.note_on(64, 0.9);
            run(&mut i, 0.8, block).0
        };
        let a = render(64);
        for b in [333usize, 1024, 4096] {
            let c = render(b);
            let d = a.iter().zip(&c).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
            assert!(d < 1e-5, "block {b}: max diff {d}");
        }
    }

    #[test]
    fn grain_macro_morphs_sample_to_cloud_at_level_loudness() {
        let mut levels = Vec::new();
        let mut counts = Vec::new();
        for g in [0.0f32, 0.5, 1.0] {
            let mut i = inst_with_sine();
            i.set_macro(Macro::Grain, g);
            i.note_on(60, 1.0);
            let (l, _) = run(&mut i, 1.0, 512);
            levels.push(rms(&l[SR as usize / 4..]));
            counts.push(i.frame().grains.len());
            let durs: Vec<f32> = i.frame().grains.iter().map(|(_, e)| e.duration).collect();
            let mean = durs.iter().sum::<f32>() / durs.len() as f32;
            if g == 0.0 {
                assert!((mean - SAMPLE_DURATION).abs() < 1e-3, "sample end: long windows");
            }
            if g == 1.0 {
                assert!(mean < 0.12, "cloud end: short grains ({mean})");
            }
        }
        assert!(counts[2] > counts[0], "the cloud has more grains in flight: {counts:?}");
        let db = |x: f32| 20.0 * (x / levels[0]).log10();
        for (k, l) in levels.iter().enumerate() {
            assert!(db(*l).abs() < 4.0, "grain step {k}: {:.1} dB", db(*l));
        }
    }

    #[test]
    fn pitch_macro_transposes_in_key() {
        let k = Key::new(0, ScaleKind::Major); // C major
        for step in 0..=24 {
            let p = step as f32 / 24.0;
            let mut m = [0.0, p, 0.0, 0.0];
            let free = MacroMap::resolve(&m, None).pitch_st;
            assert_eq!(free, (step - 12) as f32, "chromatic without a key");
            let snapped = MacroMap::resolve(&m, Some(&k)).pitch_st as i32;
            assert!(k.scale.intervals().contains(&snapped.rem_euclid(12)), "{snapped} is diatonic");
            assert!((snapped - (step - 12)).abs() <= 1);
            m[1] = 0.5;
            assert_eq!(MacroMap::resolve(&m, Some(&k)).pitch_st, 0.0, "centre = no shift");
        }
        // And it is audible: +12 on a 220 Hz sine at C4 -> 440 Hz.
        let mut i = inst_with_sine();
        i.set_key(Some(k));
        i.set_macro(Macro::Pitch, 1.0);
        i.note_on(60, 1.0);
        let (l, _) = run(&mut i, 1.0, 512);
        let hz = pitch_hz(&l[SR as usize / 4..]);
        assert!((hz - 440.0).abs() < 15.0, "{hz}");
    }

    #[test]
    fn cloud_pitch_scatter_stays_in_key() {
        let k = Key::new(9, ScaleKind::MinorPentatonic); // A minor pent.
        let mut i = inst_with_sine();
        i.set_key(Some(k));
        i.set_macro(Macro::Grain, 1.0);
        i.note_on(57, 1.0); // A3, in key
        let grains = collect(&mut i, 0.5, 512);
        assert!(grains.len() > 10);
        assert!(grains.iter().any(|e| e.semitones().abs() > 1.0), "the cloud does scatter");
        for e in &grains {
            let midi = (60.0 + e.semitones()).round() as i32; // ratio is vs C4
            assert!(k.scale.intervals().contains(&(midi - k.root).rem_euclid(12)), "{midi} in key");
        }
    }

    #[test]
    fn smear_slows_the_read_head_and_adds_a_tail() {
        let measure = |s: f32| {
            let mut i = inst_with_sine();
            i.set_macro(Macro::Smear, s);
            i.note_on(60, 1.0);
            run(&mut i, 0.6, 512);
            let positions: Vec<f64> = i.frame().grains.iter().map(|(_, e)| e.source_pos).collect();
            let advance = positions.last().unwrap() - positions.first().unwrap();
            i.note_off(60);
            let (tail, _) = run(&mut i, 0.8, 512);
            (advance, rms(&tail[SR as usize / 2..]))
        };
        let (adv0, tail0) = measure(0.0);
        let (adv1, tail1) = measure(1.0);
        assert!(adv1 < adv0 * 0.2, "read head nearly frozen: {adv1} vs {adv0}");
        assert!(tail0 < 1e-6 && tail1 > 1e-3, "smear rings on: {tail0} -> {tail1}");
    }

    #[test]
    fn look_macro_steps_through_every_look() {
        let mut seen = Vec::new();
        for step in 0..=100 {
            let m = [0.0, 0.5, 0.0, step as f32 / 100.0];
            let look = MacroMap::resolve(&m, None).look;
            if seen.last() != Some(&look) {
                seen.push(look);
            }
        }
        assert_eq!(seen, (0..LOOKS.len()).collect::<Vec<_>>());

        let mut i = inst_with_sine();
        assert_eq!(i.frame().effects.len(), 2, "no look: just the smear pair");
        i.set_macro(Macro::Look, 2.5 / LOOKS.len() as f32); // crush
        let f = i.frame();
        assert_eq!(f.effects.last().unwrap().kind.name(), "crush");
        // Crush is audible: plain vs crushed differ.
        let render = |look: f32| {
            let mut i = inst_with_sine();
            i.set_macro(Macro::Look, look);
            i.note_on(60, 1.0);
            run(&mut i, 0.3, 512).0
        };
        let (plain, crushed) = (render(0.0), render(2.5 / LOOKS.len() as f32));
        let d = plain.iter().zip(&crushed).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(d > 0.01, "crush look changes the sound ({d})");
        // The jpeg look reports its spectral latency to the host.
        i.set_macro(Macro::Look, 3.5 / LOOKS.len() as f32);
        assert!(i.latency_samples() > 0);
    }

    #[test]
    fn sync_puts_grain_onsets_on_the_host_beat_grid() {
        let mut i = inst_with_sine();
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 3.3 });
        i.set_sync(0.25);
        i.set_macro(Macro::Grain, 1.0);
        run(&mut i, 0.05, 128);
        i.note_on(60, 1.0);
        let grains = collect(&mut i, 0.5, 256);
        assert!(grains.len() > 3);
        for e in grains.iter().skip(1) {
            // Cloud jitter is ±hop/2 around its grid slot.
            let beat = 3.3 + e.onset * 2.0;
            let off = (beat / 0.25 - (beat / 0.25).round()).abs() * 0.25;
            assert!(off < 0.25 * 0.5 + 1e-6, "near grid: beat {beat}");
        }
        // With no cloud jitter (GRAIN 0), onsets are exactly on the grid.
        let mut i = inst_with_sine();
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 3.3 });
        i.set_sync(0.25);
        run(&mut i, 0.05, 128);
        i.note_on(60, 1.0);
        let grains = collect(&mut i, 1.0, 256);
        assert!(grains.len() > 3);
        for e in grains.iter().skip(1) {
            let q = (3.3 + e.onset * 2.0) / 0.25;
            assert!((q - q.round()).abs() < 1e-6, "on a 16th: {q}");
        }
        let played = i.now_secs();
        assert!((i.transport().beat - (3.3 + played * 2.0)).abs() < 1e-6, "beat advances while playing");
    }

    #[test]
    fn follow_transport_locks_the_read_head_to_song_position() {
        let head_at = |beat: f64| {
            let mut i = inst_with_sine();
            i.set_follow_transport(true);
            i.set_transport(Transport { bpm: 120.0, playing: true, beat });
            i.note_on(60, 1.0);
            run(&mut i, 0.01, 128);
            i.frame().grains[0].1.source_pos
        };
        // 2 beats at 120 bpm = 1 s further into the media.
        assert!((head_at(2.0) - head_at(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn frame_state_is_the_picture_of_the_sound() {
        let mut i = inst_with_sine();
        let mut r = FrameRenderer::new();
        let mut out = Frame::black(32, 18);
        r.render(&i.frame(), i.sources(), i.now_secs(), 24.0, &mut out);
        assert!(out.mean_luma() < 1.0, "idle = black");

        i.note_on(60, 1.0); // GRAIN 0: the whole picture
        run(&mut i, 0.3, 512);
        let f = i.frame();
        let visible: Vec<_> = i.video_frame_events(f.time).collect();
        assert!(!visible.is_empty());
        for (sid, e) in &visible {
            assert_eq!(*sid, 0);
            assert!(f.time >= e.onset && f.time < e.end());
            assert!((-1.0..=1.0).contains(&e.pan));
        }
        r.render(&f, i.sources(), f.time, 24.0, &mut out);
        assert!(out.mean_luma() > 20.0, "held note draws the video: {}", out.mean_luma());

        // Snapshots reuse their buffers.
        let mut snap = FrameState::default();
        i.fill_frame(&mut snap);
        let cap = snap.grains.capacity();
        i.fill_frame(&mut snap);
        assert_eq!(snap.grains.capacity(), cap);
    }

    #[test]
    fn raw_pcm_and_rgba_sources_and_voice_cap() {
        let mut i = Instrument::new(SR);
        let pcm: Vec<f32> = (0..SR).flat_map(|k| {
            let s = (k as f32 * 0.05).sin() * 0.5;
            [s, -s]
        }).collect();
        let rgba = vec![200u8; 8 * 4 * 4 * 3]; // 3 frames of 8×4
        let id = i.add_source_pcm_rgba("raw", &pcm, 2, SR, &rgba, 8, 4, 12.0, 0.0);
        assert_eq!(i.active_source(), Some(id));
        let s = &i.sources()[id];
        assert!(s.audio.as_ref().unwrap().is_stereo());
        assert_eq!(s.video.as_ref().unwrap().frames.len(), 3);
        for n in 0..40u8 {
            i.note_on(40 + n, 0.5);
        }
        assert_eq!(i.held_notes().count(), MAX_VOICES);
        let (l, r) = run(&mut i, 0.2, 512);
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() <= 1.0));
        i.all_notes_off();
        assert_eq!(i.held_notes().count(), 0);
    }
}
