//! What the studio UI needs from whatever it runs in — the standalone app
//! (project engine, audio device, perform pads, classic editors) or the
//! plugin (an `Instrument` on the host's audio thread). The UI itself only
//! knows this trait.

use crate::gpu::Scene;
use chromagrain_core::fx::AvEffect;
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::timeline::Project;
use std::sync::{Arc, Mutex};

static AUDITION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub use crate::studio::{DeepTab, DeepTarget, View};

/// Stage compositing mode (`stack | split | grid | bleed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StageMode {
    /// Spatial layered grain compositing (`Layout::Stack`).
    #[default]
    Stack,
    /// Full-canvas `Split + Bleed` (`Layout::Split`): feathered `+6.5%`
    /// screen-blended interior seams + sustained wash bleed across 100%.
    Split,
    /// Architectural `Grid` (`Layout::Tile`): crisp `1.5 px` obsidian dividers,
    /// low→high chord panels, `ratchet_odd` horizontal mirror.
    Grid,
    /// `Full Bleed` (`Layout::Bleed`): optical luma-key + screen melt across
    /// all active voices.
    Bleed,
}

impl StageMode {
    pub const ALL: [StageMode; 4] = [
        StageMode::Stack,
        StageMode::Split,
        StageMode::Grid,
        StageMode::Bleed,
    ];

    pub fn index(self) -> usize {
        match self {
            StageMode::Stack => 0,
            StageMode::Split => 1,
            StageMode::Grid => 2,
            StageMode::Bleed => 3,
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(StageMode::Stack)
    }

    pub fn label(self) -> &'static str {
        match self {
            StageMode::Stack => "stack",
            StageMode::Split => "split",
            StageMode::Grid => "grid",
            StageMode::Bleed => "bleed",
        }
    }
}

/// Canvas aspect ratio (`16:9 · 1:1 · 9:16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StageAspect {
    #[default]
    Wide16x9,
    Square1x1,
    Vertical9x16,
}

impl StageAspect {
    pub const ALL: [StageAspect; 3] = [
        StageAspect::Wide16x9,
        StageAspect::Square1x1,
        StageAspect::Vertical9x16,
    ];

    pub fn index(self) -> usize {
        match self {
            StageAspect::Wide16x9 => 0,
            StageAspect::Square1x1 => 1,
            StageAspect::Vertical9x16 => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(StageAspect::Wide16x9)
    }

    pub fn label(self) -> &'static str {
        match self {
            StageAspect::Wide16x9 => "16:9",
            StageAspect::Square1x1 => "1:1",
            StageAspect::Vertical9x16 => "9:16",
        }
    }

    pub fn dimensions(self) -> (u32, u32) {
        match self {
            StageAspect::Wide16x9 => (1920, 1080),
            StageAspect::Square1x1 => (1080, 1080),
            StageAspect::Vertical9x16 => (1080, 1920),
        }
    }

    pub fn from_dimensions(w: u32, h: u32) -> Self {
        let ar = w.max(1) as f32 / h.max(1) as f32;
        if ar < 0.78 {
            StageAspect::Vertical9x16
        } else if ar < 1.32 {
            StageAspect::Square1x1
        } else {
            StageAspect::Wide16x9
        }
    }
}

/// 1-tap musical stamps for the `Sculpt & tape` card (only populates the
/// continuous dials right below — never a black box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SculptStamp {
    #[default]
    Raw,
    Kick,
    Snare,
    Hat,
    Pluck,
    Chord,
    Phrase,
    TapeLoop,
    Disintegrate,
}

impl SculptStamp {
    pub const ALL: [SculptStamp; 9] = [
        SculptStamp::Raw,
        SculptStamp::Kick,
        SculptStamp::Snare,
        SculptStamp::Hat,
        SculptStamp::Pluck,
        SculptStamp::Chord,
        SculptStamp::Phrase,
        SculptStamp::TapeLoop,
        SculptStamp::Disintegrate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SculptStamp::Raw => "Raw",
            SculptStamp::Kick => "Kick",
            SculptStamp::Snare => "Snare",
            SculptStamp::Hat => "Hat",
            SculptStamp::Pluck => "Pluck",
            SculptStamp::Chord => "Chord",
            SculptStamp::Phrase => "Phrase",
            SculptStamp::TapeLoop => "Tape Loop",
            SculptStamp::Disintegrate => "Disintegrate",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            SculptStamp::TapeLoop => "Tape",
            SculptStamp::Disintegrate => "Wear",
            other => other.label(),
        }
    }
}

/// Chord voicing selector for `Sculpt & tape` and `Sequence` steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChordChoice {
    #[default]
    Off,
    Octaves,
    Fifths,
    Min7,
    Min9,
    Maj7,
    Sus4,
}

impl ChordChoice {
    pub const ALL: [ChordChoice; 7] = [
        ChordChoice::Off,
        ChordChoice::Octaves,
        ChordChoice::Fifths,
        ChordChoice::Min7,
        ChordChoice::Min9,
        ChordChoice::Maj7,
        ChordChoice::Sus4,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChordChoice::Off => "Off",
            ChordChoice::Octaves => "Oct",
            ChordChoice::Fifths => "5ths",
            ChordChoice::Min7 => "min7",
            ChordChoice::Min9 => "min9",
            ChordChoice::Maj7 => "maj7",
            ChordChoice::Sus4 => "sus4",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn step_by(self, delta: i32) -> Self {
        let n = Self::ALL.len() as i32;
        Self::ALL[(self.index() as i32 + delta).rem_euclid(n) as usize]
    }
}

/// Continuous `Sculpt & tape` voice parameters exposed in `Sheet::Clip`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SculptParams {
    pub stamp: SculptStamp,
    /// Exponential transient pitch drop (`-24..+24` semitones).
    pub pitch_drop_st: f32,
    /// Pitch envelope decay (`10..250` ms).
    pub pitch_drop_ms: f32,
    /// Sample-excited swept-SVF sub weight (`0..1`).
    pub sub: f32,
    /// 3-grain MPC flam + gated room snare snap (`0..1`).
    pub snap: f32,
    /// Pre-filter loudness-compensated `tanh` saturation (`0..18` dB).
    pub drive_db: f32,
    /// Scale-locked Karplus-Strong comb resonator mix (`0..1`).
    pub comb: f32,
    /// Karplus-Strong string feedback (`0.60..0.93`).
    pub comb_feedback: f32,
    /// Polyphonic resampled chord voicing.
    pub chord: ChordChoice,
    /// Monophonic choke group (`0 = Off`, `1..=4`).
    pub choke: u8,
    /// Unsynced free-running tape loop length in real seconds (`None = BPM Sync`).
    pub free_loop_secs: Option<f32>,
    /// Analog tape wow + flutter + Haas chorus (`0..1`).
    pub wow: f32,
    /// Basinski per-pass oxide disintegration & wear (`0..1`).
    pub wear: f32,
}

impl Default for SculptParams {
    fn default() -> Self {
        SculptParams {
            stamp: SculptStamp::Raw,
            pitch_drop_st: 0.0,
            pitch_drop_ms: 42.0,
            sub: 0.0,
            snap: 0.0,
            drive_db: 0.0,
            comb: 0.0,
            comb_feedback: 0.84,
            chord: ChordChoice::Off,
            choke: 0,
            free_loop_secs: None,
            wow: 0.0,
            wear: 0.0,
        }
    }
}

impl SculptParams {
    /// Populate the continuous dials from a 1-tap musical stamp.
    pub fn preset(stamp: SculptStamp) -> Self {
        let mut p = SculptParams { stamp, ..Default::default() };
        match stamp {
            SculptStamp::Raw => {}
            SculptStamp::Kick => {
                p.pitch_drop_st = 16.0;
                p.pitch_drop_ms = 42.0;
                p.sub = 0.72;
                p.drive_db = 7.5;
                p.choke = 2;
            }
            SculptStamp::Snare => {
                p.pitch_drop_st = 4.0;
                p.pitch_drop_ms = 28.0;
                p.snap = 0.78;
                p.drive_db = 8.5;
            }
            SculptStamp::Hat => {
                p.snap = 0.25;
                p.drive_db = 3.5;
                p.choke = 3;
            }
            SculptStamp::Pluck => {
                p.comb = 0.76;
                p.comb_feedback = 0.86;
                p.drive_db = 2.5;
            }
            SculptStamp::Chord => {
                p.comb = 0.48;
                p.comb_feedback = 0.84;
                p.chord = ChordChoice::Min9;
                p.wow = 0.14;
            }
            SculptStamp::Phrase => {
                p.drive_db = 2.0;
                p.choke = 1;
            }
            SculptStamp::TapeLoop => {
                p.free_loop_secs = Some(6.8);
                p.wow = 0.34;
                p.wear = 0.18;
            }
            SculptStamp::Disintegrate => {
                p.free_loop_secs = Some(8.4);
                p.wow = 0.48;
                p.wear = 0.76;
                p.drive_db = 3.0;
            }
        }
        p
    }
}

/// Ways to add media (the Add sheet's tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddRoute {
    /// Pick files (a dialog, or paths injected by tests / the showcase).
    Files,
    /// Google Drive + local video folders on this machine (desktop).
    Browse,
    /// YouTube Search + Paste URL (desktop).
    Link,
    /// Unseen camera-roll roulette (`<500` views, `Dig 4 → Kit`).
    Dig,
}

impl AddRoute {
    pub const YOUTUBE: AddRoute = AddRoute::Link;
    pub const DRIVE: AddRoute = AddRoute::Browse;

    /// Canonical route key (used in snapshot filenames and accessibility names).
    pub fn label(self) -> &'static str {
        match self {
            AddRoute::Files => "Files",
            AddRoute::Browse => "Browse",
            AddRoute::Link => "Link",
            AddRoute::Dig => "Dig",
        }
    }

    /// Visible tab title in the Add sheet.
    pub fn display_label(self) -> &'static str {
        match self {
            AddRoute::Link => "YouTube",
            AddRoute::Dig => "Dig",
            AddRoute::Browse => "Drive",
            AddRoute::Files => "Files",
        }
    }
}

/// One YouTube search result row in the Add sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct YoutubeHit {
    pub title: String,
    pub url: String,
    pub views: Option<u64>,
    pub duration_secs: Option<f64>,
    pub uploader: String,
}

/// Where added media should land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTarget {
    /// The next empty grid column.
    Any,
    Column(usize),
    Pad(usize),
    Lane(usize),
}

/// One thing in the Drive / Browse tab.
#[derive(Debug, Clone)]
pub struct BrowseItem {
    pub name: String,
    /// e.g. "0:42 · Drive" or "Movies".
    pub detail: String,
    pub thumb: Option<egui::TextureId>,
}

/// An add still under way (a placeholder tile until it lands).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: u64,
    pub target: AddTarget,
    /// "digging…", "downloading", "cutting", a file name.
    pub label: String,
    /// 0..1 when known.
    pub progress: Option<f32>,
    /// Failed: why (the tile shows it until dismissed or retried).
    pub failed: Option<String>,
}

/// Media that finished arriving: the source index and where it should go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Added {
    pub source: usize,
    pub target: AddTarget,
}

/// Which parts of the UI the host supports.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// Views the top bar offers.
    pub views: Vec<View>,
    /// The transport buttons control playback (false in a plugin: the DAW
    /// owns the transport).
    pub transport: bool,
    /// Rec → take bounces.
    pub rec: bool,
    /// Scenes + song chain in Sequence (false: one pattern, looping).
    pub scenes: bool,
    /// Tempo is editable (false in a plugin: host tempo).
    pub tempo: bool,
    /// Deep drawer tabs beyond Links.
    pub deep_tabs: Vec<DeepTab>,
    /// Export button.
    pub export: bool,
    /// Perform's output routing (a second window on another display).
    pub output: bool,
    /// Add-sheet tabs (empty: the "+" buttons call `import_media`).
    pub add: Vec<AddRoute>,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            views: View::ALL.to_vec(),
            transport: true,
            rec: true,
            scenes: true,
            tempo: true,
            deep_tabs: vec![DeepTab::Links, DeepTab::Effects, DeepTab::Automation, DeepTab::Script, DeepTab::Library],
            export: true,
            output: true,
            add: vec![AddRoute::Link, AddRoute::Dig, AddRoute::Browse, AddRoute::Files],
        }
    }
}

/// One perform pad, as the UI draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub source: Option<usize>,
    /// Display name (None = the source's name).
    pub name: Option<String>,
    pub kind: PadKind,
    pub len_beats: f64,
    pub on: bool,
    /// A launch waiting for the quantize boundary (true = start).
    pub queued: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Loop,
    Cloud,
    Chop,
}

/// Perform-view services (pads, launch quantize, output routing).
pub trait PerformHost {
    fn pads(&self) -> Vec<Pad>;
    fn toggle_pad(&mut self, i: usize);
    fn load_pad(&mut self, source: usize, i: usize);
    /// Load seconds `t0..t1` of a source (a slice) onto pad `i`.
    fn load_pad_slice(&mut self, source: usize, _t0: f64, _t1: f64, i: usize) {
        self.load_pad(source, i);
    }
    /// A pad's A/D/S/R (None: the default envelope).
    fn pad_env(&self, _i: usize) -> Option<chromagrain_core::envelope::Adsr> {
        None
    }
    fn set_pad_env(&mut self, _i: usize, _adsr: chromagrain_core::envelope::Adsr) {}
    /// 0 = now, 1 = beat, 2 = bar.
    fn quantize(&self) -> usize;
    fn set_quantize(&mut self, q: usize);
    fn stop_all(&mut self);
    fn loop_start(&self) -> f64;
    /// The output window: open?, and a sub-label for the "Live on Display
    /// 2" cover.
    fn output(&self) -> Option<String>;
    fn open_output(&mut self);
    fn close_output(&mut self);
}

pub trait Host {
    /// The document: sources, step patterns, tempo, tracks.
    fn project(&self) -> &Arc<Mutex<Project>>;
    /// Something in the document changed (throttled resync).
    fn mark(&mut self);
    /// A structural change (sync now).
    fn mark_now(&mut self) {
        self.mark();
    }

    fn playing(&self) -> bool;
    fn toggle_play(&mut self);
    fn seek(&mut self, beat: f64);
    fn playhead_beats(&self) -> f64;
    /// Monotonic seconds (live grain onsets are on this clock).
    fn clock(&self) -> f64;

    /// Play one live grain on effect bus `bus` (sound) and show it (picture).
    fn fire(&mut self, bus: u8, source: usize, ev: GrainEvent);
    /// The effect chain of live bus `bus` (1-based).
    fn set_bus(&mut self, bus: u8, chain: Vec<AvEffect>);

    /// The GPU canvas scene (None = no GPU: canvases draw a placeholder).
    fn canvas(&self) -> Option<Arc<Mutex<Option<Scene>>>>;
    /// Canvas pixel size (aspect for letterboxing).
    fn canvas_size(&self) -> (u32, u32);

    fn caps(&self) -> Caps {
        Caps::default()
    }

    /// The song chain changed: lay these (pattern id, start beat, length
    /// beats) out as the arrangement and loop over them. Return false to
    /// be asked again later (e.g. not possible right now).
    fn sequence_changed(&mut self, _layout: &[(usize, f64, f64)]) -> bool {
        true
    }

    /// Entering a view (the app swaps to its perform tracks).
    fn enter_view(&mut self, _v: View) {}

    fn perform(&mut self) -> Option<&mut dyn PerformHost> {
        None
    }

    /// Pick media to add (file dialog).
    fn import_media(&mut self) {}
    /// The Add sheet's Files tab (default: `import_media`).
    fn add_files(&mut self, _target: AddTarget) {
        self.import_media();
    }
    /// The Drive / Browse tab's items for `query` (thumbnails may fill in later).
    fn browse(&mut self, _ctx: &egui::Context, _query: &str) -> Vec<BrowseItem> {
        Vec::new()
    }
    /// Whether Google Drive folders were discovered in `browse`.
    fn browse_has_drive(&self) -> bool {
        false
    }
    /// Add Browse item `index` (of the last `browse` list).
    fn browse_add(&mut self, _index: usize, _target: AddTarget) {}
    fn add_link(&mut self, _url: &str, _target: AddTarget) {}
    /// Search YouTube (`yt-dlp` metadata query, optional `<500` low-view filter).
    fn search_youtube(&mut self, _query: &str, _low_views: bool) {}
    /// Current YouTube search state: `(searching, error, results)`.
    fn youtube_state(&self) -> (bool, Option<String>, Vec<YoutubeHit>) {
        (false, None, Vec::new())
    }
    /// Dig `n` random, rarely seen videos (`cc_only`: Creative Commons).
    fn dig(&mut self, _n: usize, _cc_only: bool, _target: AddTarget) {}
    /// Dig `n` unseen videos and automatically build a 16-pad kit once they land.
    fn dig_to_kit(&mut self, n: usize, cc_only: bool) {
        self.dig(n, cc_only, AddTarget::Any);
    }
    /// Build a 16-pad collage kit across all loaded sources onto Perform pads.
    fn build_kit_to_pads(&mut self) -> usize {
        0
    }
    /// Deal unsynced free-running tape loops across Perform pads.
    fn build_loops_to_pads(&mut self) -> usize {
        0
    }
    /// Adds under way (placeholder tiles).
    fn pending(&self) -> Vec<Pending> {
        Vec::new()
    }
    /// Forget a failed add.
    fn dismiss_pending(&mut self, _id: u64) {}
    /// Media that landed since the last call, with its target.
    fn take_added(&mut self) -> Vec<Added> {
        Vec::new()
    }
    /// Hear (and see) seconds `t0..t1` of a source once, now.
    fn audition(&mut self, source: usize, t0: f64, t1: f64) {
        self.stop_audition();
        let now = self.clock();
        let seq = AUDITION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let voice = 0xA0D1_7100_0000 | (seq & 0xFFFF_FFFF);
        let ev = GrainEvent {
            onset: now,
            source_pos: t0,
            duration: (t1 - t0).max(0.01) as f32,
            pitch_ratio: 1.0,
            gain: 0.9,
            pan: 0.0,
            envelope: 0.04,
            env_skew: 0.0,
            reverse: false,
            id: 0xA0D1_0000_0000 ^ (t0 * 1000.0) as u64,
            tag: chromagrain_core::grain::VoiceTag {
                voice,
                env: Some(chromagrain_core::envelope::VoiceEnv::held(
                    chromagrain_core::envelope::Adsr {
                        attack: 0.005,
                        decay: 0.01,
                        sustain: 1.0,
                        release: 0.03,
                        picture: chromagrain_core::envelope::PictureCut::Cut,
                        ..chromagrain_core::envelope::Adsr::default()
                    },
                    now,
                )),
                tile: None,
                ..Default::default()
            },
        };
        self.fire(0, source, ev);
    }
    /// Stop an audition (where the host can).
    fn stop_audition(&mut self) {
        let seq = AUDITION_SEQ.load(std::sync::atomic::Ordering::Relaxed);
        if seq > 0 {
            self.release_voice(0xA0D1_7100_0000 | (seq & 0xFFFF_FFFF));
        }
    }
    /// A live voice (an emitter gesture, `GrainEvent::tag.voice`) was
    /// released: grains already queued learn the note-off.
    fn release_voice(&mut self, _voice: u64) {}
    /// A source's slices changed in the document (the plugin forwards
    /// them to its instrument).
    fn slices_changed(&mut self, _source: usize) {}
    fn export(&mut self) {}
    fn fullscreen(&self) -> bool {
        false
    }
    fn toggle_fullscreen(&mut self, _ctx: &egui::Context) {}
    /// `O`: fullscreen output wherever it can't freeze.
    fn toggle_output(&mut self) {}
    fn status(&mut self, _s: String) {}
    /// Bounce recorded live grains (onsets from the take start) into a
    /// take of `len` seconds, recorded at `beat`.
    fn bounce_take(&mut self, _events: Vec<(usize, GrainEvent)>, _len: f64, _beat: f64) {}
    /// Deep drawer tabs other than Links (the app hosts its classic editors).
    fn deep_tab(&mut self, _tab: DeepTab, _target: DeepTarget, _ui: &mut egui::Ui) {}
    /// Macros owned by the host (a plugin's automatable GRAIN / PITCH /
    /// SMEAR / LOOK parameters): every emitter shows and edits these.
    fn shared_macros(&self) -> Option<[f32; 4]> {
        None
    }
    fn set_shared_macro(&mut self, _i: usize, _v: f32) {}
    /// The global key, when the host owns it (plugin key / scale params).
    fn shared_key(&self) -> Option<chromagrain_core::music::Key> {
        None
    }
    fn set_shared_key(&mut self, _key: chromagrain_core::music::Key) {}
    /// Stage compositing mode (`stack | split | grid | bleed`); None = no switch.
    fn stage_mode(&self) -> Option<StageMode> {
        self.tile_layout().map(|t| if t { StageMode::Grid } else { StageMode::Stack })
    }
    fn set_stage_mode(&mut self, mode: StageMode) {
        self.set_tile_layout(mode == StageMode::Grid);
    }
    /// Stage aspect ratio (`16:9 | 1:1 | 9:16`); None = host owns canvas size.
    fn stage_aspect(&self) -> Option<StageAspect> {
        None
    }
    fn set_stage_aspect(&mut self, _aspect: StageAspect) {}
    /// Legacy 2-way canvas layout helper (`Some(true)` when in `Grid` / `Tile`).
    fn tile_layout(&self) -> Option<bool> {
        None
    }
    fn set_tile_layout(&mut self, _tile: bool) {}
    /// Busy line (e.g. "bouncing the take…").
    fn busy(&self) -> Option<String> {
        None
    }
}
