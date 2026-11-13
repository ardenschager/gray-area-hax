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
//! * **Pads:** a 4×4 bank of loops ([`PadSlot`]: a stretch of a source,
//!   N beats long) launched quantized to the host's beat grid
//!   ([`Instrument::toggle_pad`], [`Instrument::set_launch_quantize`]).
//!   MIDI notes 36–51 ([`PAD_NOTE`]..) launch pads 1–16 when that pad is
//!   loaded (as the app's Perform view does); otherwise they play notes.
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
    AvLink, Frame, VideoClip, VisualGrainStyle,
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
/// Sources the instrument holds without reallocating: [`Instrument::add_source`]
/// is allocation-free (safe on the audio thread) below this count.
pub const MAX_SOURCES: usize = 16;
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
        ..Default::default()
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
    /// MIDI note the grains are pitched from (60 = unity: slices, pads).
    pitch_note: u8,
    /// Envelope (sound + picture) and the note-off time once releasing.
    adsr: Option<crate::envelope::Adsr>,
    off: Option<f64>,
    /// A slice: the voice stops (releases) when its head reaches this.
    slice_end: Option<f64>,
    /// Pad one-shots ignore note-off.
    one_shot: bool,
    origin: GrainOrigin,
    gain: f32,
    /// Tile slot + its (pan, gain) cue (layout `Tile` / `Split`).
    tile: Option<u8>,
    choke_group: u8,
    focus: (f32, f32),
    caption: crate::chop::Caption,
}

impl Voice {
    fn env(&self) -> Option<crate::envelope::VoiceEnv> {
        self.adsr.map(|adsr| crate::envelope::VoiceEnv { adsr, on: self.start_t, off: self.off.unwrap_or(f64::INFINITY) })
    }

    /// When a releasing voice is silent for good.
    fn release_end(&self) -> Option<f64> {
        self.off.map(|o| o + self.adsr.map_or(0.0, |a| a.release_secs()))
    }
}

/// Tag a grain with its voice: id, envelope, and (layout `Tile` / `Split`)
/// its tile slot with the column→pan / row→gain cue of the grid at the grain's
/// onset (so a voice's pan follows its tile as the grid grows and shrinks).
fn tag_grain(
    ev: &mut GrainEvent,
    voice: u64,
    env: Option<crate::envelope::VoiceEnv>,
    tile: Option<u8>,
    layout_pan: bool,
    tiles: &crate::tile::TileAssigner,
) {
    ev.tag.voice = voice;
    ev.tag.env = env;
    if let Some(slot) = tile {
        ev.tag.tile = Some(slot);
        if layout_pan && !ev.tag.is_wash {
            let (cols, rows) = tiles.grid_at(ev.onset);
            let (pan, gain) = crate::tile::layout_pan(slot as usize, cols, rows);
            ev.pan = (pan + 0.2 * ev.pan).clamp(-1.0, 1.0);
            ev.gain *= gain;
        }
    }
}

/// Assign a tile to a voice starting at `t` (layout `Tile` or `Split`).
fn start_tile(tiles: &mut crate::tile::TileAssigner, layout: crate::tile::Layout, voice: u64, t: f64) -> Option<u8> {
    layout.uses_tiles().then(|| tiles.assign(voice, t))
}

#[derive(Clone)]
struct LiveGrain {
    source: SourceId,
    voice: u64,
    ev: GrainEvent,
    origin: GrainOrigin,
    /// Handed out by [`Instrument::drain_new_grains`] already.
    seen: bool,
}

/// What made a grain (for UIs that already draw some grains themselves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrainOrigin {
    /// A held MIDI note.
    Note,
    /// Step lanes ([`Instrument::swap_pattern`]).
    Step,
    /// A one-shot from [`Instrument::trigger`].
    Trigger,
    /// Perform pad `n` (0-based).
    Pad(usize),
}

/// Pads in the bank (4×4).
pub const PADS: usize = 16;
/// MIDI note of pad 1 (C1); pads 1–16 are notes 36–51.
pub const PAD_NOTE: u8 = 36;
/// Grain length of the pad loops' seamless 50%-overlap Hann train.
const PAD_GRAIN: f64 = 0.08;

/// What a pad plays: a loop `len_beats` long (it follows the host tempo, or
/// loops asynchronously in real seconds when `sculpt.free_loop_secs` is set)
/// starting at `start` (0..1) in a source, or a chopped one-shot `slice`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PadSlot {
    pub source: SourceId,
    pub start: f32,
    pub len_beats: f32,
    pub gain: f32,
    /// A chopped slice (source seconds): the pad is a one-shot that plays
    /// this slice when hit (Koala pads) instead of looping `len_beats`.
    pub slice: Option<crate::chop::Slice>,
    /// Envelope of the pad's voice (sound + picture). None = classic.
    pub adsr: Option<crate::envelope::Adsr>,
    /// Continuous voice sculptor & tape-loop physics for this pad.
    pub sculpt: crate::grain::VoiceSculpt,
    /// MPC choke group (`0 = off`, `1..=4`).
    pub choke_group: u8,
}

impl Default for PadSlot {
    fn default() -> Self {
        PadSlot::new(0, 1.0)
    }
}

impl PadSlot {
    pub fn new(source: SourceId, len_beats: f32) -> PadSlot {
        PadSlot {
            source,
            start: 0.0,
            len_beats,
            gain: 1.0,
            slice: None,
            adsr: None,
            sculpt: crate::grain::VoiceSculpt::default(),
            choke_group: 0,
        }
    }

    /// A one-shot pad playing `slice` of `source` (whose media is
    /// `src_len` seconds long).
    pub fn from_slice(source: SourceId, slice: crate::chop::Slice, src_len: f64) -> PadSlot {
        let start = if src_len > 0.0 { (slice.start / src_len).clamp(0.0, 1.0) as f32 } else { 0.0 };
        PadSlot { start, slice: Some(slice), ..PadSlot::new(source, 1.0) }
    }
}

/// A pad as UIs show it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PadState {
    pub slot: Option<PadSlot>,
    pub on: bool,
    /// A launch waiting for its grid line: `Some(true)` = will start.
    pub queued: Option<bool>,
    /// Grid beat the loop (re)started on (its bar progress is
    /// `(beat - start_beat) mod len_beats`).
    pub start_beat: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Pad {
    slot: Option<PadSlot>,
    on: bool,
    /// (start?, grid beat it lands on).
    queued: Option<(bool, f64)>,
    start_beat: f64,
    next_t: f64,
    velocity: f32,
    serial: u64,
    k: u64,
    /// Envelope on / off times of the current loop (off = INFINITY while on).
    env_on: f64,
    env_off: f64,
    tile: Option<u8>,
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
    /// Compositing layout; in `Tile`, grains carry `tag.tile`.
    pub layout: crate::tile::Layout,
}

/// Key-dependent look state (the tune look's resonators), built off the
/// audio thread and installed with [`Instrument::swap_key`].
pub struct KeyFx {
    key: Option<Key>,
    sample_rate: u32,
    looks: Vec<Option<AvEffect>>,
    look_states: Vec<Vec<AudioFxState>>,
}

impl KeyFx {
    /// Build the look chain for `key` at `sample_rate`. Allocates.
    pub fn prepare(key: Option<Key>, sample_rate: u32) -> KeyFx {
        let sample_rate = sample_rate.max(1);
        let mut looks: Vec<Option<AvEffect>> =
            LOOKS.iter().map(|n| n.and_then(EffectKind::parse).map(AvEffect::new)).collect();
        if let Some(k) = key {
            for fx in looks.iter_mut().flatten() {
                if let EffectKind::Tune { root, scale, .. } = &mut fx.kind {
                    *root = k.root as f32;
                    *scale = k.scale;
                }
            }
        }
        let look_states = looks
            .iter()
            .map(|l| l.as_ref().map(|fx| fx::audio_chain_states(std::slice::from_ref(fx), sample_rate)).unwrap_or_default())
            .collect();
        KeyFx { key, sample_rate, looks, look_states }
    }

    pub fn key(&self) -> Option<Key> {
        self.key
    }
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
    /// Step lanes played in sync with the host transport.
    pattern: Option<crate::seq::StepPattern>,
    pat_rng: Rng,
    pads: [Pad; PADS],
    /// Launch grid in beats (0 = now, 1 = beat, 4 = bar).
    launch_q: f64,
    /// Envelope for held notes / slices (None = classic grain ring-out).
    note_adsr: Option<crate::envelope::Adsr>,
    layout: crate::tile::Layout,
    layout_pan: bool,
    tiles: crate::tile::TileAssigner,
    isolator: crate::dsp::BandIsolator,
    isolator_state: crate::dsp::IsolatorState,
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
        let looks = Vec::new();
        let mut inst = Instrument {
            sample_rate,
            sources: Vec::with_capacity(MAX_SOURCES),
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
            pattern: None,
            pat_rng: Rng::new(0x5E9_0E9C),
            pads: [Pad::default(); PADS],
            launch_q: 1.0,
            note_adsr: None,
            layout: crate::tile::Layout::Stack,
            layout_pan: true,
            tiles: crate::tile::TileAssigner::new(),
            isolator: crate::dsp::BandIsolator::default(),
            isolator_state: crate::dsp::IsolatorState::new(sample_rate),
        };
        inst.rebuild_fx();
        inst
    }

    /// Change the output rate (plugin re-activation). Silences everything.
    pub fn reset(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        self.scratch.sample_rate = self.sample_rate;
        self.isolator_state = crate::dsp::IsolatorState::new(self.sample_rate);
        self.voices.clear();
        self.grains.clear();
        for p in &mut self.pads {
            (p.on, p.queued) = (false, None);
        }
        self.clock = 0;
        self.anchor_t = 0.0;
        self.rebuild_fx();
    }

    fn rebuild_fx(&mut self) {
        let sr = self.sample_rate;
        self.smear_states = fx::audio_chain_states(&self.smear_chain, sr);
        let KeyFx { looks, look_states, .. } = KeyFx::prepare(self.key, sr);
        self.looks = looks;
        self.look_states = look_states;
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

    /// Raw-data entry point (what a C ABI wraps): see [`source_from_pcm_rgba`].
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
        self.add_source(source_from_pcm_rgba(name, pcm, channels, pcm_rate, rgba, width, height, fps, base_hz))
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

    /// [`Instrument::replace_source`] that hands back the old media instead
    /// of dropping it, so an audio thread can pass it elsewhere to be freed.
    /// Returns `Err(source)` for an unknown id.
    pub fn swap_source(&mut self, id: SourceId, source: Source) -> Result<Source, Source> {
        match self.sources.get_mut(id) {
            Some(s) => Ok(std::mem::replace(s, source)),
            None => Err(source),
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
    /// tune look's resonators); on an audio thread use
    /// [`KeyFx::prepare`] elsewhere + [`Instrument::swap_key`].
    pub fn set_key(&mut self, key: Option<Key>) {
        self.key = key;
        self.rebuild_fx();
    }

    /// Change the key without allocating: install look state built off the
    /// audio thread by [`KeyFx::prepare`]. Returns the replaced state (to
    /// be dropped elsewhere), or `fx` itself if it was prepared for a
    /// different sample rate (nothing changes then).
    pub fn swap_key(&mut self, mut fx: KeyFx) -> KeyFx {
        if fx.sample_rate != self.sample_rate {
            return fx;
        }
        std::mem::swap(&mut self.looks, &mut fx.looks);
        std::mem::swap(&mut self.look_states, &mut fx.look_states);
        let old = self.key;
        self.key = fx.key;
        fx.key = old;
        fx.sample_rate = self.sample_rate;
        fx
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

    /// MIDI note on. Order: an assigned pad (36–51) plays its pad; else,
    /// when the active source has slices, notes from C3 (48) up play slice
    /// `note - 48` (at unity pitch, stopping at the slice end); else the
    /// note plays the media pitched (C4 = unity).
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        if let Some(i) = pad_of_note(note) {
            if let Some(slot) = self.pads[i].slot {
                if velocity > 0.0 {
                    if slot.slice.is_some() {
                        self.hit_slice_pad(i, velocity.min(1.0));
                    } else {
                        self.toggle_pad_vel(i, velocity.min(1.0));
                    }
                }
                return;
            }
        }
        let Some(source) = self.active else { return };
        let Some(src) = self.sources.get(source) else { return };
        let len = src.duration();
        if len <= 0.0 || velocity <= 0.0 {
            if velocity <= 0.0 {
                self.note_off(note); // MIDI convention
            }
            return;
        }
        let slice = crate::chop::slice_for_note(note, src.slices.len()).map(|i| src.slices[i]);
        self.note_off(note);
        let now = self.now_secs();
        let (head, pitch_note, slice_end) = match slice {
            Some(sl) => (sl.start, 60, Some(sl.end)),
            None => {
                let mut head = self.position as f64 * len;
                if self.follow_transport && self.transport.playing {
                    head += self.beat_at(now) * 60.0 / self.transport.bpm;
                }
                (head.rem_euclid(len), note, None)
            }
        };
        let adsr = self.note_adsr;
        let focus = slice.map_or((0.5, 0.5), |sl| sl.focus);
        let caption = slice.map_or_else(crate::chop::Caption::default, |sl| sl.caption);
        self.start_voice(
            note,
            velocity.min(1.0),
            source,
            head,
            pitch_note,
            slice_end,
            adsr,
            false,
            GrainOrigin::Note,
            1.0,
            0,
            focus,
            caption,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn start_voice(
        &mut self,
        note: u8,
        velocity: f32,
        source: SourceId,
        head: f64,
        pitch_note: u8,
        slice_end: Option<f64>,
        adsr: Option<crate::envelope::Adsr>,
        one_shot: bool,
        origin: GrainOrigin,
        gain: f32,
        choke_group: u8,
        focus: (f32, f32),
        caption: crate::chop::Caption,
    ) {
        if self.voices.len() >= MAX_VOICES {
            let oldest = self.voices.remove(0);
            self.cut_future(oldest.serial);
            self.tiles.release(oldest.serial, self.now_secs());
        }
        let now = self.now_secs();
        self.serial += 1;
        let tile = start_tile(&mut self.tiles, self.layout, self.serial, now);
        self.voices.push(Voice {
            note,
            velocity,
            source,
            head,
            next_t: now,
            start_t: now,
            serial: self.serial,
            rng: Rng::new(0xC0FFEE ^ self.serial.wrapping_mul(0x9E37) ^ note as u64),
            k: 0,
            pitch_note,
            adsr,
            off: None,
            slice_end,
            one_shot,
            origin,
            gain,
            tile,
            choke_group,
            focus,
            caption,
        });
    }

    /// Mute active voices/pads/grains in MPC choke group `group` (`1..=4`) at
    /// `t_cut` with a click-free `3 ms` cosine release, never choking sibling
    /// chord voices triggered at the same instant (`|onset - t_cut| <= 2 ms`).
    /// Allocation-free.
    fn choke_group_at(&mut self, group: u8, t_cut: f64) {
        if group == 0 {
            return;
        }
        let cut_end = t_cut + crate::grain::CHOKE_RELEASE_SECS;
        for g in self.grains.iter_mut() {
            if g.ev.tag.choke_group == group && g.ev.onset < t_cut - 0.002 {
                g.ev.tag.choke_at = Some(g.ev.tag.choke_at.map_or(t_cut, |c| c.min(t_cut)));
            }
        }
        self.grains.retain(|g| !(g.ev.tag.choke_group == group && g.ev.onset >= t_cut + 0.002));

        let mut vi = self.voices.len();
        while vi > 0 {
            vi -= 1;
            if vi < self.voices.len()
                && self.voices[vi].choke_group == group
                && self.voices[vi].start_t < t_cut - 0.002
            {
                let v = self.voices.remove(vi);
                self.tiles.release(v.serial, cut_end);
            }
        }

        for i in 0..PADS {
            let p = &mut self.pads[i];
            if p.on && p.slot.is_some_and(|s| s.choke_group == group) && p.env_on < t_cut - 0.002 {
                p.on = false;
                p.env_off = t_cut;
                self.tiles.release(p.serial, cut_end);
            }
        }
    }

    /// A slice pad: a one-shot of its slice, now (not launch-quantized).
    fn hit_slice_pad(&mut self, i: usize, velocity: f32) {
        let Some(slot) = self.pads.get(i).and_then(|p| p.slot) else { return };
        let Some(sl) = slot.slice else { return };
        let Some(src) = self.sources.get(slot.source) else { return };
        let src_len = src.duration();
        if src_len <= 0.0 {
            return;
        }
        let base_hz = src.effective_base_hz();
        let now = self.now_secs();
        if slot.choke_group > 0 {
            self.choke_group_at(slot.choke_group, now);
        }
        // Retrigger: the previous hit of this pad releases.
        if let Some(vi) = self.voices.iter().position(|v| v.origin == GrainOrigin::Pad(i) && v.off.is_none()) {
            self.release_voice_at(vi, now);
        }
        for g in self.grains.iter_mut() {
            if g.origin == GrainOrigin::Pad(i) && g.ev.onset < now - 0.002 {
                g.ev.tag.choke_at = Some(g.ev.tag.choke_at.map_or(now, |c| c.min(now)));
            }
        }
        let adsr = slot.adsr.or(self.note_adsr);
        // Sculpted one-shot pads (Kick / Snare / Hat / Pluck / Chord / Phrase)
        // fire unified sculpted GrainEvent(s) directly at GRAIN = 0.
        if slot.sculpt.is_active() && self.macro_value(Macro::Grain) < 0.05 {
            if self.grains.len() >= MAX_GRAINS {
                return;
            }
            let map = self.macro_map();
            let ratio = semitones_to_ratio(map.pitch_st + slot.sculpt.pitch_semitones);
            let dur = ((sl.len() / ratio.max(0.01) as f64) as f32).clamp(0.015, 30.0);
            let comb_hz = if slot.sculpt.comb_mix > 1e-3 {
                crate::grain::quantize_comb_hz(base_hz.max(40.0) * ratio, self.key.as_ref())
            } else {
                0.0
            };
            self.serial += 1;
            let serial = self.serial;
            let mut ev = GrainEvent {
                onset: now,
                source_pos: sl.start.rem_euclid(src_len.max(1e-6)),
                duration: dur,
                pitch_ratio: ratio,
                gain: slot.gain * velocity,
                pan: 0.0,
                envelope: 0.08,
                env_skew: 0.0,
                reverse: false,
                id: (5u64 << 60) | ((i as u64) << 48) | (serial & 0xFFFF_FFFF_FFFF),
                sculpt: slot.sculpt,
                comb_hz,
                loop_pass: 0,
                ..Default::default()
            };
            if let Some(a) = adsr {
                let off = ev.onset + ev.duration as f64;
                ev.duration += a.release;
                ev.envelope = 0.01;
                ev.tag.env = Some(crate::envelope::VoiceEnv { adsr: a, on: ev.onset, off });
            }
            ev.tag.voice = serial;
            ev.tag.focus = sl.focus;
            ev.tag.caption = sl.caption;
            ev.tag.choke_group = slot.choke_group;
            ev.tag.is_wash = slot.sculpt.is_wash();
            ev.tag.zoom_punch = if slot.sculpt.sub_exciter > 0.2 || slot.sculpt.snare_snap > 0.2 {
                0.08
            } else {
                0.03
            };
            let (chord_evs, n_evs) = crate::grain::expand_chord_grain_inline(
                &ev,
                slot.sculpt.chord,
                base_hz,
                self.key.as_ref(),
            );
            for (ci, cev) in chord_evs.iter().copied().enumerate().take(n_evs) {
                if self.grains.len() >= MAX_GRAINS {
                    break;
                }
                let mut cev = cev;
                let v_id = serial.wrapping_add(ci as u64 * 0x1000_0000);
                cev.tag.voice = v_id;
                if let Some(tile) = start_tile(&mut self.tiles, self.layout, v_id, now) {
                    self.tiles.release(v_id, cev.end());
                    let env = cev.tag.env;
                    tag_grain(&mut cev, v_id, env, Some(tile), self.layout_pan, &self.tiles);
                }
                self.grains.push(LiveGrain {
                    source: slot.source,
                    voice: v_id,
                    ev: cev,
                    origin: GrainOrigin::Pad(i),
                    seen: false,
                });
            }
            return;
        }
        let note = PAD_NOTE + i as u8;
        let pitch_note = (60.0 + slot.sculpt.pitch_semitones).round().clamp(12.0, 115.0) as u8;
        self.start_voice(
            note,
            velocity,
            slot.source,
            sl.start,
            pitch_note,
            Some(sl.end),
            adsr,
            true,
            GrainOrigin::Pad(i),
            slot.gain,
            slot.choke_group,
            sl.focus,
            sl.caption,
        );
    }

    /// Note-off for voice `vi` at time `t`: with an envelope it releases
    /// (its grains learn the off time, it keeps sounding through the
    /// release); without one it stops making grains and rings out.
    fn release_voice_at(&mut self, vi: usize, t: f64) {
        let v = &mut self.voices[vi];
        if v.adsr.is_some() {
            if v.off.is_some() {
                return;
            }
            v.off = Some(t);
            let serial = v.serial;
            let end = v.release_end().unwrap_or(t);
            for g in self.grains.iter_mut().filter(|g| g.voice == serial) {
                if let Some(e) = g.ev.tag.env.as_mut() {
                    e.off = e.off.min(t);
                }
            }
            self.grains.retain(|g| !(g.voice == serial && g.ev.onset >= end));
            self.tiles.release(serial, end);
        } else {
            let v = self.voices.remove(vi);
            let end = self.grains.iter().filter(|g| g.voice == v.serial && g.ev.onset < t).map(|g| g.ev.end()).fold(t, f64::max);
            self.grains.retain(|g| !(g.voice == v.serial && g.ev.onset >= t));
            self.tiles.release(v.serial, end);
        }
    }

    /// Stop a note: no new grains; sounding grains ring out (the release),
    /// or, with an ADSR, the envelope releases.
    pub fn note_off(&mut self, note: u8) {
        if let Some(vi) = self.voices.iter().position(|v| v.note == note && !v.one_shot && v.off.is_none()) {
            self.release_voice_at(vi, self.now_secs());
        }
    }

    /// Envelope for held notes and slices (sound + picture); None = the
    /// classic grain ring-out on note-off.
    pub fn set_note_adsr(&mut self, adsr: Option<crate::envelope::Adsr>) {
        self.note_adsr = adsr;
    }

    pub fn note_adsr(&self) -> Option<crate::envelope::Adsr> {
        self.note_adsr
    }

    /// Compositing layout (see [`crate::tile`]). In `Tile` or `Split` every
    /// voice gets a panel slot (`GrainEvent::tag.tile`), and with `layout_pan`
    /// its column drives pan.
    pub fn set_layout(&mut self, layout: crate::tile::Layout) {
        self.layout = layout;
    }

    pub fn layout(&self) -> crate::tile::Layout {
        self.layout
    }

    pub fn set_layout_pan(&mut self, on: bool) {
        self.layout_pan = on;
    }

    pub fn layout_pan(&self) -> bool {
        self.layout_pan
    }

    /// Set (or clear) the slices of source `id`. Allocates; call off the
    /// audio thread (or swap the source).
    pub fn set_slices(&mut self, id: SourceId, slices: Vec<crate::chop::Slice>) -> bool {
        match self.sources.get_mut(id) {
            Some(s) => {
                s.slices = slices;
                let len = s.duration();
                crate::chop::normalize(&mut s.slices, len);
                true
            }
            None => false,
        }
    }

    /// Fill pads `first..` with one-shot pads, one per slice of `source`
    /// ("slices → pads"). Returns how many pads were set.
    pub fn slices_to_pads(&mut self, source: SourceId, first: usize) -> usize {
        let Some(src) = self.sources.get(source) else { return 0 };
        let len = src.duration();
        let slices: [Option<crate::chop::Slice>; PADS] = std::array::from_fn(|k| src.slices.get(k).copied());
        let mut n = 0;
        for (k, sl) in slices.iter().enumerate() {
            let (Some(sl), pad) = (sl, first + k) else { break };
            if pad >= PADS {
                break;
            }
            self.set_pad(pad, Some(PadSlot::from_slice(source, *sl, len)));
            n += 1;
        }
        n
    }

    /// Populate pads `0..PADS` from multi-source [`crate::chop::KitPadSpec`]s
    /// curated by [`crate::chop::build_kit_across_sources`] or
    /// [`crate::chop::build_tape_loops_across_sources`]. Returns pads loaded.
    pub fn kit_to_pads(&mut self, specs: &[crate::chop::KitPadSpec]) -> usize {
        let mut n = 0;
        for pad in 0..PADS {
            if let Some(spec) = specs.get(pad) {
                let Some(src_len) = self.sources.get(spec.source).map(|s| s.duration()) else {
                    self.set_pad(pad, None);
                    continue;
                };
                self.set_pad(pad, Some(spec.to_pad_slot(src_len)));
                n += 1;
            } else {
                self.set_pad(pad, None);
            }
        }
        n
    }

    /// Stop every note (allocation- and free-free, so safe on the audio thread).
    pub fn all_notes_off(&mut self) {
        let now = self.now_secs();
        let mut vi = self.voices.len();
        while vi > 0 {
            vi -= 1;
            if vi < self.voices.len() && self.voices[vi].off.is_none() {
                self.release_voice_at(vi, now);
            }
        }
    }

    /// Notes held down (not releasing, not pad one-shots).
    pub fn held_notes(&self) -> impl Iterator<Item = u8> + '_ {
        self.voices.iter().filter(|v| v.off.is_none() && !v.one_shot && v.origin == GrainOrigin::Note).map(|v| v.note)
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

    /// Play a one-shot grain now (an emitter on the editor's grid, a pad).
    /// It runs through SMEAR and LOOK like note grains. Allocation-free;
    /// dropped when the grain pool is full.
    pub fn trigger(&mut self, source: SourceId, mut ev: GrainEvent) {
        if self.grains.len() >= MAX_GRAINS || self.sources.get(source).is_none() {
            return;
        }
        let now = self.now_secs();
        ev.onset = now;
        if ev.tag.choke_group > 0 {
            self.choke_group_at(ev.tag.choke_group, now);
        }
        if ev.sculpt.chord != crate::grain::ChordVoicing::Off {
            let base_hz = self.sources.get(source).map_or(440.0, |s| s.effective_base_hz());
            let (chord_evs, n_evs) = crate::grain::expand_chord_grain_inline(
                &ev,
                ev.sculpt.chord,
                base_hz,
                self.key.as_ref(),
            );
            for cev in chord_evs.iter().copied().take(n_evs) {
                if self.grains.len() >= MAX_GRAINS {
                    break;
                }
                self.grains.push(LiveGrain { source, voice: 0, ev: cev, origin: GrainOrigin::Trigger, seen: false });
            }
        } else {
            self.grains.push(LiveGrain { source, voice: 0, ev, origin: GrainOrigin::Trigger, seen: false });
        }
    }

    /// Install step lanes (a `StepPattern`: rows = lanes, steps with
    /// slice / pitch / length / chance) that play in sync with the host
    /// transport while it runs. Swaps, so the caller frees the old one off
    /// the audio thread.
    pub fn swap_pattern(&mut self, pattern: Option<crate::seq::StepPattern>) -> Option<crate::seq::StepPattern> {
        std::mem::replace(&mut self.pattern, pattern)
    }

    pub fn pattern(&self) -> Option<&crate::seq::StepPattern> {
        self.pattern.as_ref()
    }

    /// Fire the pattern's steps whose boundary falls in `[t0, t1)`.
    fn sequence(&mut self, t0: f64, t1: f64) {
        if !self.transport.playing {
            return;
        }
        let Some(spb) = self.pattern.as_ref().filter(|p| !p.rows.is_empty()).map(|p| p.steps_per_beat.max(1) as f64) else {
            return;
        };
        let step_secs = (60.0 / self.transport.bpm) / spb;
        let (b0, b1) = (self.beat_at(t0), self.beat_at(t1));
        let n_rows = self.pattern.as_ref().map_or(0, |p| p.rows.len());
        let mut k = (b0 * spb - 1e-9).ceil() as i64;
        while (k as f64) / spb < b1 {
            let t = self.grid_time(k as f64 / spb).clamp(t0, t1);
            for ri in 0..n_rows {
                let (source, step, g, row_key, row_sculpt, row_choke) = {
                    let Some(pat) = self.pattern.as_ref() else { break };
                    let row = &pat.rows[ri];
                    if row.muted || row.steps.is_empty() {
                        continue;
                    }
                    let st = row.steps[k.rem_euclid(row.steps.len() as i64) as usize];
                    (row.source, st, row.grains.clone(), row.key, row.sculpt, row.choke_group)
                };
                let Some(src) = self.sources.get(source) else { continue };
                let len = src.duration();
                if len <= 0.0 {
                    continue;
                }
                let base_hz = src.effective_base_hz();
                let chop = step.slice_index.and_then(|i| src.slices.get(i as usize).copied());
                let roll = self.pat_rng.next_f32();
                let jit = self.pat_rng.bipolar();
                if !step.on || roll >= step.chance || self.grains.len() >= MAX_GRAINS {
                    continue;
                }
                let key = row_key.or(self.key);
                let mut sculpt = if row_sculpt.is_active() { row_sculpt } else { g.sculpt };
                if step.chord != crate::grain::ChordVoicing::Off {
                    sculpt.chord = step.chord;
                }
                let choke_group = if row_choke > 0 { row_choke.min(4) } else { g.choke_group.min(4) };
                let at = if step.slice >= 0.0 { step.slice.min(1.0) } else { g.position };
                let pos = chop.map_or(at as f64 * len, |sl| sl.start).rem_euclid(len.max(1e-6));
                let r_count = step.ratchet.clamp(1, 8);
                let sub_step_secs = step_secs / r_count as f64;

                for sub in 0..r_count {
                    if self.grains.len() >= MAX_GRAINS {
                        break;
                    }
                    let sub_t = t + sub as f64 * sub_step_secs;
                    if choke_group > 0 {
                        self.choke_group_at(choke_group, sub_t);
                    }
                    let st = g.pitch
                        + sculpt.pitch_semitones
                        + step.pitch
                        + g.pitch_jitter * jit
                        + sub as f32 * step.ratchet_pitch;
                    let ratio = crate::music::effective_ratio(st, key.as_ref(), base_hz, g.quantize_amount, g.detune_cents);
                    let mut dur = crate::seq::hit_duration(&g, &step, chop, ratio);
                    if r_count > 1 {
                        dur = dur.min((sub_step_secs * 0.92).max(0.01) as f32);
                    }
                    let sub_gain = if r_count > 1 { 0.88f32.powi(sub as i32) } else { 1.0 };
                    let comb_hz = if sculpt.comb_mix > 1e-3 {
                        crate::grain::quantize_comb_hz(base_hz.max(40.0) * ratio, key.as_ref())
                    } else {
                        0.0
                    };
                    let slot_u = if r_count == 1 { k as u64 } else { (k as u64).wrapping_mul(8).wrapping_add(sub as u64) };
                    let hit_env = if sculpt.sub_exciter > 0.1
                        || sculpt.snare_snap > 0.1
                        || sculpt.comb_mix > 0.2
                    {
                        g.envelope.min(0.04)
                    } else {
                        g.envelope
                    };
                    let mut ev = GrainEvent {
                        onset: sub_t,
                        source_pos: pos,
                        duration: dur,
                        pitch_ratio: ratio,
                        gain: g.gain * step.gain * sub_gain,
                        pan: g.pan.clamp(-1.0, 1.0),
                        envelope: hit_env,
                        env_skew: g.env_skew,
                        reverse: g.reverse_prob >= 0.5,
                        id: (1u64 << 62) | ((ri as u64) << 40) | (slot_u & 0xFF_FFFF_FFFF),
                        sculpt,
                        comb_hz,
                        loop_pass: 0,
                        ..Default::default()
                    };
                    let voice = crate::seq::hit_voice(ri, slot_u, 0.0);
                    crate::seq::apply_hit_adsr(&mut ev, &g, voice);
                    ev.tag.voice = voice;
                    ev.tag.focus = chop.map_or((0.5, 0.5), |sl| sl.focus);
                    ev.tag.caption = chop.map_or_else(crate::chop::Caption::default, |sl| sl.caption);
                    ev.tag.ratchet_odd = sub % 2 == 1;
                    ev.tag.choke_group = choke_group;
                    ev.tag.is_wash = sculpt.is_wash();

                    let (chord_evs, n_evs) = crate::grain::expand_chord_grain_inline(
                        &ev,
                        sculpt.chord,
                        base_hz,
                        key.as_ref(),
                    );
                    for (ci, mut cev) in chord_evs.iter().copied().enumerate().take(n_evs) {
                        if self.grains.len() >= MAX_GRAINS {
                            break;
                        }
                        let cv = voice.wrapping_add(ci as u64 * 0x1000_0000);
                        cev.tag.voice = cv;
                        if let Some(tile) = start_tile(&mut self.tiles, self.layout, cv, sub_t) {
                            self.tiles.release(cv, cev.end());
                            let env = cev.tag.env;
                            tag_grain(&mut cev, cv, env, Some(tile), self.layout_pan, &self.tiles);
                        }
                        self.grains.push(LiveGrain { source, voice: 0, ev: cev, origin: GrainOrigin::Step, seen: false });
                    }
                }
            }
            k += 1;
        }
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

        self.launch(t0, t1);
        self.schedule(t1 + LOOKAHEAD, &map);
        self.schedule_pads(t1 + LOOKAHEAD);
        self.sequence(t0, t1);

        // Grains -> scratch (exactly n samples, no realloc in steady state).
        self.scratch.left.clear();
        self.scratch.right.clear();
        self.scratch.left.resize(n, 0.0);
        self.scratch.right.resize(n, 0.0);
        for g in &self.grains {
            if g.ev.onset >= t1 || g.ev.audio_end() <= t0 {
                continue;
            }
            let Some(audio) = self.sources.get(g.source).and_then(|s| s.audio.as_deref()) else {
                continue;
            };
            render_grains_audio(audio, std::slice::from_ref(&g.ev), &mut self.scratch, t0);
        }
        left.copy_from_slice(&self.scratch.left);
        right.copy_from_slice(&self.scratch.right);
        self.grains.retain(|g| g.ev.audio_end() > t1);

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
        // ISOL8 5-band frequency isolator (allocation-free).
        if self.isolator.is_active() {
            self.isolator_state
                .process_block(left, right, &self.isolator, self.sample_rate);
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
        let layout_pan = self.layout_pan;
        let mut vi = 0;
        while vi < self.voices.len() {
            let src_len = self.sources.get(self.voices[vi].source).map(|s| s.duration()).unwrap_or(0.0);
            if src_len <= 0.0 {
                vi += 1;
                continue;
            }
            let mut done = false;
            while self.voices[vi].next_t < until {
                if self.grains.len() >= MAX_GRAINS {
                    return;
                }
                // A releasing voice stops making grains once silent; a slice
                // voice releases (or stops) when its head reaches the end.
                if let Some(end) = self.voices[vi].release_end() {
                    if self.voices[vi].next_t >= end {
                        done = true;
                        break;
                    }
                }
                if let Some(slice_end) = self.voices[vi].slice_end {
                    if self.voices[vi].head >= slice_end - 1e-6 && self.voices[vi].off.is_none() {
                        let t = self.voices[vi].next_t;
                        if self.voices[vi].adsr.is_some() {
                            self.voices[vi].off = Some(t);
                            let end = self.voices[vi].release_end().unwrap_or(t);
                            self.tiles.release(self.voices[vi].serial, end);
                            continue;
                        }
                        let serial = self.voices[vi].serial;
                        let end = self.grains.iter().filter(|g| g.voice == serial).map(|g| g.ev.end()).fold(t, f64::max);
                        self.tiles.release(serial, end);
                        done = true;
                        break;
                    }
                }
                let key = self.key;
                let cloud = Cloud::default();
                let v = &mut self.voices[vi];
                let id = v.serial.wrapping_mul(1 << 20).wrapping_add(v.k);
                let (mut ev, hop) =
                    grain_step(map, &cloud, key.as_ref(), &mut v.rng, v.head, src_len, v.pitch_note, v.velocity, id);
                ev.onset = (v.next_t + ev.onset).max(v.start_t);
                ev.gain *= v.gain;
                if let Some(slice_end) = v.slice_end {
                    // Don't read past the slice into the next one.
                    let room = ((slice_end - ev.source_pos) / ev.pitch_ratio.max(0.01) as f64).max(0.005);
                    ev.duration = ev.duration.min(room as f32);
                }
                ev.tag.choke_group = v.choke_group;
                ev.tag.focus = v.focus;
                ev.tag.caption = v.caption;
                ev.tag.hard_burst = v.adsr.is_some_and(|a| a.picture == crate::envelope::PictureCut::Cut)
                    && cloud.spray * map.texture <= 0.08
                    && ev.duration <= 0.095;
                tag_grain(&mut ev, v.serial, v.env(), v.tile, layout_pan, &self.tiles);
                let (source, serial, origin) = (v.source, v.serial, v.origin);
                v.head += hop * map.speed as f64;
                v.k += 1;
                let mut next = v.next_t + hop;
                if let Some(div) = sync {
                    // Next grid line at or after the free-running time.
                    let b = (self.grid_beat(next) / div - 1e-9).ceil() * div;
                    next = self.grid_time(b).max(self.voices[vi].next_t + 1e-4);
                }
                self.voices[vi].next_t = next;
                self.grains.push(LiveGrain { source, voice: serial, ev, origin, seen: false });
            }
            if done {
                self.voices.remove(vi);
            } else {
                vi += 1;
            }
        }
    }

    // ------------------------------------------------------------ pads

    /// Load (or clear) a pad. A pad that was playing keeps playing with the
    /// new loop. Allocation-free.
    pub fn set_pad(&mut self, i: usize, slot: Option<PadSlot>) {
        let Some(p) = self.pads.get_mut(i) else { return };
        let slot = slot.map(|s| PadSlot { start: s.start.clamp(0.0, 1.0), len_beats: s.len_beats.clamp(0.25, 64.0), gain: s.gain.clamp(0.0, 2.0), ..s });
        p.slot = slot;
        if slot.is_none() {
            let was_on = std::mem::replace(&mut p.on, false);
            p.queued = None;
            if was_on {
                let serial = p.serial;
                self.cut_future(serial);
            }
        }
    }

    pub fn pad(&self, i: usize) -> PadState {
        self.pads.get(i).map_or(PadState::default(), |p| PadState {
            slot: p.slot,
            on: p.on,
            queued: p.queued.map(|q| q.0),
            start_beat: p.start_beat,
        })
    }

    pub fn pads(&self) -> [PadState; PADS] {
        std::array::from_fn(|i| self.pad(i))
    }

    /// Launch grid in beats: 0 = now, 1 = next beat, 4 = next bar.
    pub fn set_launch_quantize(&mut self, beats: f64) {
        self.launch_q = if beats.is_finite() { beats.clamp(0.0, 16.0) } else { 0.0 };
    }

    pub fn launch_quantize(&self) -> f64 {
        self.launch_q
    }

    /// Set the master 5-band `24 dB/Oct` `ISOL8` frequency isolator live (`0` heap allocations).
    pub fn set_isolator(&mut self, isolator: crate::dsp::BandIsolator) {
        self.isolator = isolator;
    }

    pub fn isolator(&self) -> crate::dsp::BandIsolator {
        self.isolator
    }

    /// Start / stop a pad on the next launch-grid line of the host's beat
    /// grid (free-running at the host tempo while it is stopped). Pressing
    /// a pad whose launch is still queued cancels it. Empty pads ignore it.
    pub fn toggle_pad(&mut self, i: usize) {
        if self.pads.get(i).and_then(|p| p.slot).is_some_and(|s| s.slice.is_some()) {
            self.hit_slice_pad(i, 1.0);
        } else {
            self.toggle_pad_vel(i, 1.0);
        }
    }

    fn toggle_pad_vel(&mut self, i: usize, velocity: f32) {
        let now = self.now_secs();
        let beat = self.grid_beat(now);
        let q = self.launch_q;
        let Some(p) = self.pads.get_mut(i) else { return };
        if p.slot.is_none() {
            return;
        }
        if p.queued.take().is_some() {
            return;
        }
        let start = !p.on;
        if start {
            p.velocity = velocity;
        }
        let at = if q > 0.0 { (beat / q - 1e-9).ceil() * q } else { beat };
        p.queued = Some((start, at));
    }

    /// Stop every pad now.
    pub fn stop_pads(&mut self) {
        for i in 0..PADS {
            let p = &mut self.pads[i];
            p.queued = None;
            if p.on {
                p.on = false;
                let serial = p.serial;
                self.cut_future(serial);
            }
        }
    }

    /// Apply the queued launches whose grid line falls before `t1`.
    fn launch(&mut self, t0: f64, t1: f64) {
        for i in 0..PADS {
            let Some((start, mut at)) = self.pads[i].queued else { continue };
            // The host jumped back (a loop, a locate): re-aim at the next
            // line from here.
            let b0 = self.grid_beat(t0);
            if at > b0 + self.launch_q + 1e-6 {
                at = if self.launch_q > 0.0 { (b0 / self.launch_q - 1e-9).ceil() * self.launch_q } else { b0 };
                self.pads[i].queued = Some((start, at));
            }
            let t = self.grid_time(at).max(t0);
            if t >= t1 {
                continue;
            }
            let choke_group = self.pads[i].slot.map_or(0, |s| s.choke_group);
            if start && choke_group > 0 {
                self.choke_group_at(choke_group, t);
            }
            self.serial += 1;
            let serial = self.serial;
            let p = &mut self.pads[i];
            p.queued = None;
            let release = p.slot.and_then(|s| s.adsr).map_or(0.0, |a| a.release_secs());
            if start {
                let old = p.serial;
                (p.on, p.start_beat, p.next_t, p.serial, p.k) = (true, at, t, serial, 0);
                (p.env_on, p.env_off) = (t, f64::INFINITY);
                // A retrigger drops the old loop's pending grains.
                self.grains.retain(|g| !(g.voice == old && g.origin == GrainOrigin::Pad(i) && g.ev.onset >= t));
                self.tiles.release(old, t);
                let is_wash = p.slot.is_some_and(|s| s.sculpt.is_wash());
                self.pads[i].tile = if self.layout == crate::tile::Layout::Split && is_wash {
                    None
                } else {
                    start_tile(&mut self.tiles, self.layout, serial, t)
                };
            } else {
                p.on = false;
                p.env_off = t;
                let old = p.serial;
                // With an envelope the loop releases: its grains learn the
                // off time and the pad keeps playing through the release.
                for g in self.grains.iter_mut().filter(|g| g.voice == old && g.origin == GrainOrigin::Pad(i)) {
                    if let Some(e) = g.ev.tag.env.as_mut() {
                        e.off = e.off.min(t);
                    }
                }
                self.grains.retain(|g| !(g.voice == old && g.origin == GrainOrigin::Pad(i) && g.ev.onset >= t + release));
                self.tiles.release(old, t + release);
            }
        }
    }

    /// The playing pads' grains up to `until`: either an asynchronous
    /// real-seconds Basinski tape loop (`slot.sculpt.free_loop_secs` with `80ms`
    /// raised-cosine seam and per-pass `disintegrate` / `ebb_flow`), or a
    /// seamless 50%-overlap Hann train phase-locked to the host beat grid.
    fn schedule_pads(&mut self, until: f64) {
        let hop = PAD_GRAIN / 2.0;
        for i in 0..PADS {
            let p = self.pads[i];
            let Some(slot) = p.slot else { continue };
            if slot.slice.is_some() {
                continue; // slice pads are one-shot voices
            }
            let env = slot.adsr.map(|adsr| crate::envelope::VoiceEnv { adsr, on: p.env_on, off: if p.on { f64::INFINITY } else { p.env_off } });
            let releasing_until = env.filter(|_| !p.on).map(|e| e.end());
            if !p.on && releasing_until.is_none_or(|end| p.next_t >= end) {
                continue;
            }
            let Some((len, base_hz)) = self
                .sources
                .get(slot.source)
                .and_then(|s| (s.duration() > 0.0).then_some((s.duration(), s.effective_base_hz())))
            else {
                continue;
            };
            let pad_ratio = semitones_to_ratio(slot.sculpt.pitch_semitones);
            let pad_comb_hz = if slot.sculpt.comb_mix > 1e-3 {
                crate::grain::quantize_comb_hz(base_hz.max(40.0) * pad_ratio, self.key.as_ref())
            } else {
                0.0
            };
            let mut next_t = p.next_t;
            let mut k = p.k;
            // Never look ahead past a queued stop.
            let until = match releasing_until {
                Some(end) => until.min(end),
                None => p.queued.filter(|q| !q.0).map_or(until, |q| until.min(self.grid_time(q.1) + slot.adsr.map_or(0.0, |a| a.release_secs()))),
            };
            if let Some(free_secs) = slot.sculpt.free_loop_secs.filter(|s| *s > 0.05) {
                let period = (free_secs as f64).clamp(0.05, 120.0);
                let seam = 0.08_f64.min(period * 0.25);
                let dur_out = period + seam;
                let env_taper = ((2.0 * seam) / dur_out).clamp(0.02, 0.90) as f32;
                while next_t < until {
                    if self.grains.len() >= MAX_GRAINS {
                        break;
                    }
                    let pass_k = k as u32;
                    let id = (3u64 << 61) | ((i as u64) << 48) | (k & 0xFFFF_FFFF_FFFF);
                    let ebb = crate::grain::ebb_flow_gain_at(slot.sculpt.ebb_flow, pass_k, next_t, id);
                    let ox = crate::grain::oxide_dropout_at(slot.sculpt.disintegrate, pass_k, 0.5, id);
                    let mut ev = GrainEvent {
                        onset: next_t,
                        source_pos: (slot.start as f64 * len).rem_euclid(len),
                        duration: dur_out as f32,
                        pitch_ratio: pad_ratio,
                        gain: slot.gain * p.velocity * ebb,
                        pan: 0.0,
                        envelope: env_taper,
                        env_skew: 0.0,
                        reverse: false,
                        id,
                        sculpt: slot.sculpt,
                        comb_hz: pad_comb_hz,
                        loop_pass: pass_k,
                        ..Default::default()
                    };
                    ev.tag.choke_group = slot.choke_group;
                    ev.tag.is_wash = true;
                    ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
                    tag_grain(&mut ev, p.serial, env, p.tile, self.layout_pan, &self.tiles);
                    self.grains.push(LiveGrain { source: slot.source, voice: p.serial, ev, origin: GrainOrigin::Pad(i), seen: false });
                    next_t += period;
                    k += 1;
                }
            } else {
                let spb = 60.0 / self.transport.bpm;
                let loop_secs = (slot.len_beats as f64 * spb).min(len);
                while next_t < until {
                    if self.grains.len() >= MAX_GRAINS {
                        break;
                    }
                    let phase = ((self.grid_beat(next_t) - p.start_beat) * spb).rem_euclid(loop_secs.max(1e-3));
                    let pass_k = if loop_secs > 1e-3 {
                        (((self.grid_beat(next_t) - p.start_beat) * spb) / loop_secs).max(0.0) as u32
                    } else {
                        0
                    };
                    let id = (3u64 << 61) | ((i as u64) << 48) | (k & 0xFFFF_FFFF_FFFF);
                    let ebb = if slot.sculpt.ebb_flow > 1e-3 {
                        crate::grain::ebb_flow_gain_at(slot.sculpt.ebb_flow, pass_k, next_t, id)
                    } else {
                        1.0
                    };
                    let ox = crate::grain::oxide_dropout_at(slot.sculpt.disintegrate, pass_k, 0.5, id);
                    let mut ev = GrainEvent {
                        onset: next_t,
                        source_pos: (slot.start as f64 * len + phase).rem_euclid(len),
                        duration: PAD_GRAIN as f32,
                        pitch_ratio: pad_ratio,
                        gain: slot.gain * p.velocity * ebb,
                        pan: 0.0,
                        envelope: 1.0,
                        env_skew: 0.0,
                        reverse: false,
                        id,
                        sculpt: slot.sculpt,
                        comb_hz: pad_comb_hz,
                        loop_pass: pass_k,
                        ..Default::default()
                    };
                    ev.tag.choke_group = slot.choke_group;
                    ev.tag.is_wash = slot.sculpt.is_wash();
                    ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
                    tag_grain(&mut ev, p.serial, env, p.tile, self.layout_pan, &self.tiles);
                    self.grains.push(LiveGrain { source: slot.source, voice: p.serial, ev, origin: GrainOrigin::Pad(i), seen: false });
                    next_t += hop;
                    k += 1;
                }
            }
            (self.pads[i].next_t, self.pads[i].k) = (next_t, k);
        }
    }

    /// Grains made since the last call (notes, pads, steps, triggers), in
    /// creation order, onsets on the instrument clock ([`Instrument::now_secs`];
    /// may lie up to ~0.1 s ahead). For a UI on another thread that draws
    /// the picture itself. Allocation-free.
    pub fn drain_new_grains(&mut self, mut f: impl FnMut(SourceId, &GrainEvent, GrainOrigin)) {
        for g in self.grains.iter_mut().filter(|g| !g.seen) {
            g.seen = true;
            f(g.source, &g.ev, g.origin);
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
        if out.effects.capacity() < 4 {
            out.effects.reserve(4);
        }
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
        if self.isolator.is_active() {
            out.effects.push(AvEffect::new(EffectKind::Isolator(self.isolator)));
        }
        out.look = map.look;
        out.layout = self.layout;
    }

    /// Grains visible at instrument time `t` (a subset of the snapshot).
    pub fn video_frame_events(&self, t: f64) -> impl Iterator<Item = (SourceId, &GrainEvent)> + '_ {
        self.grains.iter().filter(move |g| t >= g.ev.onset && t < g.ev.end()).map(|g| (g.source, &g.ev))
    }
}

fn pad_of_note(note: u8) -> Option<usize> {
    (PAD_NOTE..PAD_NOTE + PADS as u8).contains(&note).then(|| (note - PAD_NOTE) as usize)
}

/// Build a [`Source`] from raw data (what hosts with their own decoders,
/// e.g. AVFoundation, hand over): `pcm` is mono (`channels` = 1) or
/// interleaved stereo; `rgba` is tightly packed `width × height` RGBA8
/// frames at `fps`. Either may be empty. Allocates.
#[allow(clippy::too_many_arguments)]
pub fn source_from_pcm_rgba(
    name: &str,
    pcm: &[f32],
    channels: u32,
    pcm_rate: u32,
    rgba: &[u8],
    width: u32,
    height: u32,
    fps: f32,
    base_hz: f32,
) -> Source {
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
    Source { name: name.into(), audio, video, base_hz, ..Default::default() }
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
        let aspect = w as f32 / h.max(1) as f32;
        let grid = match frame.layout {
            crate::tile::Layout::Tile => {
                let any_burst = frame
                    .grains
                    .iter()
                    .any(|(_, e)| e.tag.hard_burst && t >= e.onset && t < e.end());
                let active_slots = frame
                    .grains
                    .iter()
                    .filter(|(_, e)| {
                        let carry = if !e.tag.hard_burst && e.tag.env.is_none() && e.duration > 0.085 && e.duration < 0.35 {
                            crate::video::OPTICAL_CARRY_SECS
                        } else {
                            0.0
                        };
                        t >= e.onset && t < e.end() + carry && e.tag.picture_at(t) > 0.0
                    })
                    .filter_map(|(_, e)| e.tag.tile);
                Some(if any_burst && aspect > 1.3 {
                    crate::tile::frame_portrait_grid(active_slots, aspect)
                } else {
                    crate::tile::frame_grid(active_slots)
                })
            }
            crate::tile::Layout::Split => Some(crate::tile::frame_split_grid(
                frame
                    .grains
                    .iter()
                    .filter(|(_, e)| !e.tag.is_wash && t >= e.onset && t < e.end() + crate::video::OPTICAL_CARRY_SECS && e.tag.picture_at(t) > 0.0)
                    .filter_map(|(_, e)| e.tag.tile),
                aspect,
            )),
            _ => None,
        };
        for (sid, ev) in &frame.grains {
            let Some(video) = sources.get(*sid).and_then(|s| s.video.as_deref()) else { continue };
            crate::video::composite_grains_frame_stage(
                video,
                std::slice::from_ref(ev),
                &frame.style,
                &frame.link,
                &ClipTransform::default(),
                None,
                &mut layer,
                t,
                false,
                None,
                frame.layout,
                grid,
            );
        }
        fx::apply_video_chain(&mut layer, &frame.effects, &mut self.states);
        *out = Frame::black(w, h);
        crate::video::blend_stage_layer(out, &layer, frame.layout, frame.style.additive, 1.0, false);
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
            ..Default::default()
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
    fn step_lanes_play_in_sync_with_the_host_transport() {
        let mut i = inst_with_sine();
        let mut pat = crate::seq::StepPattern::new("lanes");
        let r = pat.add_row(0);
        for k in (0..16).step_by(4) {
            pat.rows[r].steps[k].on = true;
        }
        pat.rows[r].grains.duration = 0.1;
        assert!(i.swap_pattern(Some(pat)).is_none());
        // Stopped: nothing.
        let (l, _) = run(&mut i, 0.5, 256);
        assert_eq!(rms(&l), 0.0, "transport stopped: silent");
        // 120 bpm from beat 0: hits every 0.5 s.
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 0.0 });
        let grains = collect(&mut i, 2.0, 256);
        let onsets: Vec<f64> = grains.iter().map(|e| e.onset - 0.5).collect();
        assert_eq!(onsets.len(), 4, "{onsets:?}");
        for (n, o) in onsets.iter().enumerate() {
            assert!((o - n as f64 * 0.5).abs() < 1e-6, "hit {n} at {o}");
        }
        // The host jumps to beat 2 (a loop): the next hit is on beat 2.
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 1.9 });
        let t = i.now_secs();
        let g = collect(&mut i, 0.2, 128);
        assert!(g.iter().any(|e| (e.onset - (t + 0.05)).abs() < 1e-6), "beat 2 lands 0.05 s after 1.9");
        let old = i.swap_pattern(None);
        assert!(old.is_some());
    }

    #[test]
    fn triggered_grains_sound_through_the_macros() {
        let mut i = inst_with_sine();
        let ev = GrainEvent {
            onset: 123.0, // ignored: plays now
            source_pos: 0.5,
            duration: 0.2,
            pitch_ratio: 1.0,
            gain: 0.8,
            pan: 0.0,
            envelope: 0.5,
            env_skew: 0.0,
            reverse: false,
            id: 9,
            ..Default::default()
        };
        i.trigger(0, ev.clone());
        i.trigger(7, ev); // unknown source: ignored
        let (l, _) = run(&mut i, 0.3, 256);
        assert!(rms(&l[..9600]) > 0.01);
        assert_eq!(i.frame().grains.len(), 0, "finished");
    }

    /// Grains made while running `secs`, with their origins (each once).
    fn drained(i: &mut Instrument, secs: f64, block: usize) -> (Vec<(GrainEvent, GrainOrigin)>, Vec<f32>) {
        let blocks = (secs * SR as f64 / block as f64).ceil() as usize;
        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        let (mut all, mut audio) = (Vec::new(), Vec::new());
        for _ in 0..blocks {
            i.process(&mut l, &mut r);
            audio.extend_from_slice(&l);
            i.drain_new_grains(|_, e, o| all.push((e.clone(), o)));
        }
        (all, audio)
    }

    #[test]
    fn pads_launch_and_stop_on_the_host_beat_grid() {
        let mut i = inst_with_sine();
        i.set_pad(0, Some(PadSlot::new(0, 2.0)));
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 0.3 });
        assert_eq!(i.launch_quantize(), 1.0, "launches on the beat by default");
        i.toggle_pad(0);
        assert_eq!(i.pad(0).queued, Some(true));
        assert!(!i.pad(0).on);
        // Beat 1 is 0.35 s away at 120 bpm.
        let (g, audio) = drained(&mut i, 1.0, 256);
        let first = g.iter().map(|(e, _)| e.onset).fold(f64::MAX, f64::min);
        assert!((first - 0.35).abs() < 1e-9, "the loop starts on beat 1: {first}");
        assert!(g.iter().all(|(_, o)| *o == GrainOrigin::Pad(0)));
        let at = (0.35 * SR as f64) as usize;
        assert_eq!(rms(&audio[..at - 8]), 0.0, "silent until the beat");
        assert!(rms(&audio[at + 2_400..at + 12_000]) > 0.05, "a steady loop after it: {}", rms(&audio[at + 2_400..at + 12_000]));
        let st = i.pad(0);
        assert!(st.on && st.queued.is_none() && st.start_beat == 1.0, "{st:?}");
        // Stop on the bar: no grain starts at or after beat 4 (1.85 s from
        // the start); the sound rings out right after.
        i.set_launch_quantize(4.0);
        i.toggle_pad(0);
        assert_eq!(i.pad(0).queued, Some(false));
        let (g, audio) = drained(&mut i, 1.5, 256);
        assert!(g.iter().all(|(e, _)| e.onset < 1.85 - 1e-9), "nothing after the bar line");
        assert!(g.iter().any(|(e, _)| e.onset > 1.85 - 0.05), "but right up to it");
        let rel = ((1.85 - 1.0) * SR as f64) as usize;
        assert!(rms(&audio[..rel - 8]) > 0.05);
        assert!(rms(&audio[rel + (0.1 * SR as f64) as usize..]) < 1e-3, "rings out after the bar");
        assert!(!i.pad(0).on);
    }

    #[test]
    fn pad_loops_follow_the_host_tempo() {
        let mut i = inst_with_sine();
        i.set_launch_quantize(0.0);
        i.set_pad(3, Some(PadSlot { start: 0.25, ..PadSlot::new(0, 1.0) }));
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 0.0 });
        i.toggle_pad(3);
        let (g, _) = drained(&mut i, 1.2, 128);
        let len = 3.0; // the sine source's seconds
        let pos_at = |t: f64| g.iter().find(|(e, _)| (e.onset - t).abs() < 1e-6).map(|(e, _)| e.source_pos).unwrap();
        let start = 0.25 * len;
        assert!((pos_at(0.0) - start).abs() < 1e-6, "starts at the slot's start");
        assert!((pos_at(0.2) - (start + 0.2)).abs() < 1e-6, "reads straight through");
        assert!((pos_at(0.52) - (start + 0.02)).abs() < 1e-6, "1 beat = 0.5 s at 120 bpm: loops back");
        // Half the tempo: the same beat loop is twice as long.
        i.set_transport(Transport { bpm: 60.0, playing: true, beat: 4.0 });
        let t = i.now_secs();
        let (g, _) = drained(&mut i, 1.5, 128);
        let after: Vec<_> = g.iter().filter(|(e, _)| e.onset >= t + 0.2).collect();
        let e = after.iter().find(|(e, _)| (e.onset - (t + 0.6)).abs() < 1e-6).unwrap();
        assert!((e.0.source_pos - (start + 0.6)).abs() < 1e-6, "60 bpm: still inside the 1 s loop at 0.6 s");
    }

    #[test]
    fn midi_notes_36_to_51_launch_loaded_pads() {
        let mut i = inst_with_sine();
        i.set_launch_quantize(0.0);
        i.set_pad(2, Some(PadSlot::new(0, 4.0))); // note 38
        i.note_on(PAD_NOTE + 2, 0.5);
        assert_eq!(i.held_notes().count(), 0, "a loaded pad's note launches the pad");
        let (g, _) = drained(&mut i, 0.1, 256);
        assert!(i.pad(2).on);
        assert!(g.iter().all(|(e, o)| *o == GrainOrigin::Pad(2) && (e.gain - 0.5).abs() < 1e-6), "velocity = loop gain");
        i.note_off(PAD_NOTE + 2); // pads toggle; note-off is ignored
        drained(&mut i, 0.05, 256);
        assert!(i.pad(2).on);
        i.note_on(PAD_NOTE + 2, 1.0);
        drained(&mut i, 0.05, 256);
        assert!(!i.pad(2).on, "pressed again: stops");
        // An empty pad's note (and anything outside 36–51) plays the media.
        i.note_on(PAD_NOTE + 1, 1.0);
        i.note_on(60, 1.0);
        assert_eq!(i.held_notes().collect::<Vec<_>>(), vec![PAD_NOTE + 1, 60]);
        let (g, _) = drained(&mut i, 0.1, 256);
        assert!(!g.is_empty() && g.iter().all(|(_, o)| *o == GrainOrigin::Note));
    }

    #[test]
    fn queued_presses_cancel_and_stop_pads_silences() {
        let mut i = inst_with_sine();
        for p in 0..PADS {
            i.set_pad(p, Some(PadSlot::new(0, 1.0)));
        }
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 0.5 });
        i.toggle_pad(0);
        i.toggle_pad(0);
        assert_eq!(i.pad(0).queued, None, "a second press before the line cancels");
        i.toggle_pad(5); // empty pads aside, the queue survives a host jump
        i.set_transport(Transport { bpm: 120.0, playing: true, beat: 0.9 });
        drained(&mut i, 0.1, 256);
        assert!(i.pad(5).on);
        i.set_launch_quantize(0.0);
        for p in 0..PADS {
            if !i.pad(p).on {
                i.toggle_pad(p);
            }
        }
        drained(&mut i, 0.05, 256);
        assert!(i.pads().iter().all(|p| p.on), "16 loops at once");
        i.stop_pads();
        let (g, audio) = drained(&mut i, 0.3, 256);
        assert!(g.is_empty(), "no new grains after stop");
        assert!(rms(&audio[(0.12 * SR as f64) as usize..]) < 1e-3);
        // Clearing a playing pad stops it; triggered grains survive.
        i.toggle_pad(1);
        drained(&mut i, 0.05, 256);
        i.trigger(0, GrainEvent { onset: 0.0, source_pos: 0.1, duration: 0.5, pitch_ratio: 1.0, gain: 0.5, pan: 0.0, envelope: 1.0, env_skew: 0.0, reverse: false, id: 1, ..Default::default() });
        i.set_pad(1, None);
        assert!(!i.pad(1).on && i.pad(1).slot.is_none());
        let (g, audio) = drained(&mut i, 0.2, 256);
        assert!(g.iter().all(|(_, o)| *o == GrainOrigin::Trigger));
        assert!(rms(&audio) > 0.01, "the one-shot still sounds");
    }

    #[test]
    fn new_grains_are_reported_once_with_their_origin() {
        let mut i = inst_with_sine();
        i.note_on(60, 1.0);
        i.trigger(0, GrainEvent { onset: 0.0, source_pos: 0.1, duration: 0.05, pitch_ratio: 1.0, gain: 0.5, pan: 0.0, envelope: 1.0, env_skew: 0.0, reverse: false, id: 1, ..Default::default() });
        let (g, _) = drained(&mut i, 1.0, 256);
        let ids: std::collections::HashSet<u64> = g.iter().map(|(e, _)| e.id).collect();
        assert_eq!(ids.len(), g.len(), "each grain once");
        assert_eq!(g.iter().filter(|(_, o)| *o == GrainOrigin::Trigger).count(), 1);
        assert!(g.iter().filter(|(_, o)| *o == GrainOrigin::Note).count() > 2);
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
    fn swap_key_matches_set_key_and_swap_source_returns_the_old_media() {
        let k = Key::new(2, ScaleKind::Dorian);
        let render = |mut i: Instrument| {
            i.set_macro(Macro::Look, 1.5 / LOOKS.len() as f32); // tune
            i.note_on(62, 1.0);
            run(&mut i, 0.4, 512).0
        };
        let mut a = inst_with_sine();
        a.set_key(Some(k));
        let mut b = inst_with_sine();
        let old = b.swap_key(KeyFx::prepare(Some(k), SR));
        assert_eq!(old.key(), None);
        assert_eq!(b.key(), Some(k));
        assert_eq!(render(a), render(b));
        // Wrong rate: refused, nothing changes.
        let mut c = inst_with_sine();
        let back = c.swap_key(KeyFx::prepare(Some(k), 44100));
        assert_eq!((back.key(), c.key()), (Some(k), None));

        let mut i = inst_with_sine();
        let old = i.swap_source(0, sine_source(440.0, 1.0)).ok().unwrap();
        assert_eq!(old.base_hz, 220.0);
        assert_eq!(i.sources()[0].base_hz, 440.0);
        assert!(i.swap_source(5, sine_source(1.0, 1.0)).is_err());
        assert!(i.sources().len() < MAX_SOURCES && i.sources.capacity() >= MAX_SOURCES);
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

    // ---- v003: ADSR, slices, tiles ----

    use crate::chop::{equal_slices, Slice};
    use crate::envelope::Adsr;
    use crate::tile::Layout;

    fn run_n(i: &mut Instrument, samples: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; samples], vec![0.0; samples]);
        let mut pos = 0;
        while pos < samples {
            let b = block.min(samples - pos);
            i.process(&mut l[pos..pos + b], &mut r[pos..pos + b]);
            pos += b;
        }
        (l, r)
    }

    fn quick_adsr() -> Adsr {
        Adsr { attack: 0.002, decay: 0.0, sustain: 1.0, release: 0.02, ..Adsr::default() }
    }

    fn sliced() -> Instrument {
        let mut i = inst_with_sine();
        assert!(i.set_slices(0, equal_slices(3.0, 4)));
        assert_eq!(i.sources()[0].slices.len(), 4);
        i
    }

    #[test]
    fn note_adsr_release_silences_after_note_off() {
        let tail = |adsr: Option<Adsr>| {
            let mut i = inst_with_sine();
            i.set_macro(Macro::Smear, 0.0);
            i.set_note_adsr(adsr);
            assert_eq!(i.note_adsr(), adsr);
            i.note_on(60, 1.0);
            run(&mut i, 0.3, 256);
            i.note_off(60);
            assert_eq!(i.held_notes().count(), 0);
            let (l, _) = run(&mut i, 0.3, 256);
            // Past the release (+ the pipeline's latency).
            let skip = ((0.02 + 0.01) * SR as f64) as usize + i.latency_samples();
            rms(&l[skip..])
        };
        let classic = tail(None);
        let enveloped = tail(Some(quick_adsr()));
        assert!(classic > 0.01, "classic grains ring out: {classic}");
        assert!(enveloped < classic * 0.05, "ADSR release cuts: {enveloped} vs {classic}");
    }

    #[test]
    fn notes_from_c3_play_slices() {
        let mut i = sliced();
        i.note_on(49, 1.0); // slice 1 = 0.75..1.5 s
        let grains = collect(&mut i, 1.5, 256);
        assert!(!grains.is_empty());
        for e in &grains {
            assert!(e.source_pos >= 0.75 - 1e-9 && e.source_pos < 1.5, "inside slice 1: {}", e.source_pos);
            assert!(e.source_pos + (e.duration * e.pitch_ratio) as f64 <= 1.5 + 1e-6, "clamped to the slice end");
            assert!((e.pitch_ratio - 1.0).abs() < 1e-6, "slices play at unity");
        }
        // The voice stops at the slice end (0.75 s of media).
        let first = grains.iter().map(|e| e.onset).fold(f64::INFINITY, f64::min);
        let last = grains.iter().map(|e| e.onset).fold(0.0, f64::max);
        assert!(last - first < 0.75 + 1e-6, "{first}..{last}");
        // Past the slices: the melodic path.
        let mut m = sliced();
        m.note_on(60, 1.0);
        let g = collect(&mut m, 0.2, 256);
        assert!(!g.is_empty() && g.iter().all(|e| e.source_pos < 0.75 + 0.3));
    }

    #[test]
    fn slices_to_pads_make_one_shot_pads() {
        let mut i = sliced();
        assert_eq!(i.slices_to_pads(0, 0), 4);
        assert_eq!(i.slices_to_pads(9, 0), 0);
        let p = i.pad(2);
        assert_eq!(p.slot.and_then(|s| s.slice), Some(Slice::new(1.5, 2.25)));
        i.note_on(PAD_NOTE + 2, 1.0);
        i.note_off(PAD_NOTE + 2); // one-shot: note-off doesn't stop it
        assert_eq!(i.held_notes().count(), 0);
        let grains = collect(&mut i, 1.0, 256);
        assert!(!grains.is_empty(), "the pad hit plays");
        assert!(grains.iter().all(|e| e.source_pos >= 1.5 - 1e-9 && e.source_pos < 2.25));
        // toggle_pad on a slice pad is also a one-shot.
        let mut j = sliced();
        j.slices_to_pads(0, 4);
        j.toggle_pad(4);
        let g = collect(&mut j, 0.3, 256);
        assert!(!g.is_empty() && g.iter().all(|e| e.source_pos < 0.75));
        assert!(!j.pad(4).on, "a slice pad doesn't latch");
    }

    #[test]
    fn tile_layout_gives_voices_slots_and_pans() {
        let mut i = inst_with_sine();
        i.set_layout(Layout::Tile);
        assert!(i.layout_pan());
        i.note_on(60, 1.0);
        i.note_on(67, 1.0);
        let grains = collect(&mut i, 0.4, 256);
        let pans = |slot: u8| grains.iter().filter(|e| e.tag.tile == Some(slot)).map(|e| e.pan).collect::<Vec<_>>();
        let (a, b) = (pans(0), pans(1));
        assert!(!a.is_empty() && !b.is_empty());
        assert!(a.iter().all(|p| *p < -0.5), "slot 0 = left: {a:?}");
        assert!(b.iter().all(|p| *p > 0.5), "slot 1 = right: {b:?}");
        assert!(grains.iter().all(|e| e.tag.voice != 0));

        // The picture: left and right halves both lit.
        let f = i.frame();
        assert_eq!(f.layout, Layout::Tile);
        let mut out = Frame::black(32, 18);
        FrameRenderer::new().render(&f, i.sources(), f.time, 24.0, &mut out);
        let half = |x0: u32, x1: u32| {
            let mut sum = 0.0;
            for y in 0..18u32 {
                for x in x0..x1 {
                    let k = ((y * 32 + x) * 4) as usize;
                    sum += out.data[k] as f32 + out.data[k + 1] as f32 + out.data[k + 2] as f32;
                }
            }
            sum / ((x1 - x0) * 18 * 3) as f32
        };
        assert!(half(0, 15) > 10.0 && half(17, 32) > 10.0, "{} {}", half(0, 15), half(17, 32));

        // Without layout pan, pans stay near center.
        let mut c = inst_with_sine();
        c.set_layout(Layout::Tile);
        c.set_layout_pan(false);
        c.note_on(60, 1.0);
        c.note_on(67, 1.0);
        let g = collect(&mut c, 0.3, 256);
        assert!(g.iter().all(|e| e.pan.abs() < 0.5));
        // Stack: no tiles.
        let mut s = inst_with_sine();
        s.note_on(60, 1.0);
        assert!(collect(&mut s, 0.2, 256).iter().all(|e| e.tag.tile.is_none()));
    }

    #[test]
    fn adsr_tiles_and_slices_are_block_size_independent() {
        let render = |block: usize| {
            let mut i = sliced();
            i.set_layout(Layout::Tile);
            i.set_note_adsr(Some(Adsr { attack: 0.01, decay: 0.05, sustain: 0.6, release: 0.1, ..Adsr::default() }));
            i.slices_to_pads(0, 0);
            let mut l = Vec::new();
            let mut push = |i: &mut Instrument, n: usize| l.extend(run_n(i, n, block).0);
            i.note_on(60, 0.9);
            push(&mut i, 4096);
            i.note_on(50, 0.7);
            push(&mut i, 4096);
            i.note_on(PAD_NOTE + 1, 1.0);
            push(&mut i, 2048);
            i.note_off(60);
            push(&mut i, 8192);
            i.note_off(50);
            push(&mut i, 8192);
            l
        };
        let a = render(64);
        let b = render(512);
        let c = render(2048);
        assert!(rms(&a) > 0.01);
        let diff = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
        assert!(diff(&a, &b) < 1e-5, "64 vs 512: {}", diff(&a, &b));
        assert!(diff(&a, &c) < 1e-5, "64 vs 2048: {}", diff(&a, &c));
    }

    #[test]
    fn a_voice_pan_follows_its_tile_as_the_grid_grows() {
        let mut i = inst_with_sine();
        i.set_layout(Layout::Tile);
        i.note_on(60, 1.0);
        let alone = collect(&mut i, 0.3, 256);
        assert!(alone.iter().all(|e| e.tag.tile == Some(0) && e.pan.abs() <= 0.2 + 1e-6), "a lone voice is centered");
        i.note_on(67, 1.0);
        run(&mut i, 0.1, 256); // past the lookahead
        let t = i.now_secs();
        let both = collect(&mut i, 0.3, 256);
        let first: Vec<f32> = both.iter().filter(|e| e.tag.tile == Some(0) && e.onset >= t).map(|e| e.pan).collect();
        assert!(!first.is_empty() && first.iter().all(|p| *p < -0.5), "slot 0 moves left: {first:?}");
    }
}
