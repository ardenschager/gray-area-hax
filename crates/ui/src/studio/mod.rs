//! The play-first studio UI (`D · Hybrid` + VGS Stage/Sculpt/Set upgrade):
//! three views — Play (Tempera grid + emitters), Sequence (nanoloop lanes +
//! VJ set cues + step ratchets/chords), Perform (pads + punch-in FX + output) —
//! calm modal sheets (`Add`, `Clip`, `Project`), and a minimal top bar.
//! Hosts (the standalone app, the plugin) implement [`crate::host::Host`].

pub mod add;
pub mod clip;
mod deep;
mod perform;
mod play;
pub mod sequence;

use crate::atlas;
use crate::host::{ChordChoice, Host, SculptParams, StageAspect, StageMode};
use crate::theme::{self, *};
use crate::widgets::*;
use chromagrain_core::dsp::Rng;
use chromagrain_core::envelope::{Adsr, VoiceEnv};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::{GrainEvent, VoiceTag};
use chromagrain_core::instrument::{grain_step, Cloud, MacroMap, LOOKS};
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::tile::{layout_pan, TileAssigner};
use chromagrain_core::video::{AvLink, VisualGrainStyle};
use egui::{self, Align2, Color32, Rect};
use std::collections::HashMap;

pub use clip::ClipTarget;
pub use deep::{DeepTab, DeepTarget};

/// A modal sheet over the studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Add(crate::host::AddTarget),
    Clip(ClipTarget),
    Project,
}

pub const COLUMNS: usize = 8;
pub const ROWS: usize = 8;
pub const LANES: usize = 8;
pub const STEPS: usize = 16;
pub const SCENES: usize = 12;
pub const EMITTERS: usize = 4;

/// Names of the 6 Perform VJ Punch-In FX switches (`v004` collage/rhythmic bank).
pub const PUNCH_FX_LABELS: [&str; 6] = [
    "Stutter",
    "Grid ×",
    "Cutout",
    "Tape Stop",
    "Comb",
    "Crush",
];

pub const PUNCH_FX_A11Y: [&str; 6] = [
    "punch stutter",
    "punch grid",
    "punch cutout",
    "punch tape stop",
    "punch comb",
    "punch crush",
];

pub const PUNCH_FX_KEYS: [egui::Key; 6] = [
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
    egui::Key::Num0,
];

pub const PUNCH_FX_KEY_HINTS: [&str; 6] = ["5", "6", "7", "8", "9", "0"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Play,
    Sequence,
    Perform,
}

impl View {
    pub const ALL: [View; 3] = [View::Play, View::Sequence, View::Perform];
    pub fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
}

/// A brush on the Play grid with its own grain and sculpt settings.
pub struct Emitter {
    pub col: usize,
    /// Vertical position in grid rows, 0..8 (which part of the clip).
    pub row: f32,
    /// GRAIN, PITCH, SMEAR, LOOK (0..1 each).
    pub macros: [f32; 4],
    /// Voice output level (0..1; 0.8 = unity).
    pub level: f32,
    /// Center stereo pan (-1..1).
    pub pan: f32,
    /// Fine detune in cents (-50..50).
    pub fine_cents: f32,
    /// Octave shift (-2..2).
    pub octave: i32,
    /// Additional per-emitter AV effects configured in the Clip sheet.
    pub effects: Vec<AvEffect>,
    /// Extra stretch / filter amounts (0..1) from the Clip sheet FX pills.
    pub stretch: f32,
    pub lowpass: f32,
    pub highpass: f32,
    pub sculpt: SculptParams,
    pub caption: String,
    pub focus: (f32, f32),
    pub cloud: Cloud,
    pub link: AvLink,
    /// Held by a finger / the pointer: spraying.
    pub held: bool,
    /// A/D/S/R over a gesture (press .. release), sound and picture.
    pub adsr: Adsr,
    /// The current gesture: voice id, note-on, note-off (None while held).
    gesture: Option<(u64, f64, Option<f64>)>,
    head: f64,
    next_t: f64,
    rng: Rng,
    k: u64,
    pub last_fire: f64,
}

impl Emitter {
    fn new(i: usize, col: usize, row: f32) -> Emitter {
        Emitter {
            col,
            row,
            macros: [0.18, 0.5, 0.25, 0.0],
            level: 0.8,
            pan: 0.0,
            fine_cents: 0.0,
            octave: 0,
            effects: Vec::new(),
            stretch: 0.0,
            lowpass: 0.0,
            highpass: 0.0,
            sculpt: SculptParams::default(),
            caption: String::new(),
            focus: (0.5, 0.5),
            cloud: Cloud::default(),
            link: AvLink::default(),
            held: false,
            adsr: Adsr {
                attack: 0.02,
                decay: 0.26,
                sustain: 0.78,
                release: [0.35, 0.85, 0.22, 0.10][i % 4],
                ..Adsr::default()
            },
            gesture: None,
            head: 0.0,
            next_t: 0.0,
            rng: Rng::new(0xE111 + i as u64 * 7919),
            k: 0,
            last_fire: -10.0,
        }
    }

    /// Media position 0..1 under the emitter.
    pub fn pos(&self) -> f32 {
        (self.row / ROWS as f32).clamp(0.0, 0.999)
    }

    pub fn look_name(&self) -> &'static str {
        look_name(MacroMap::resolve(&self.macros, None).look)
    }
}

pub fn look_name(i: usize) -> &'static str {
    match LOOKS.get(i).copied().flatten() {
        None => "clean",
        Some("compress") => "jpeg",
        Some(n) => n,
    }
}

pub struct Rec {
    pub t0: f64,
    pub beat0: f64,
    pub events: Vec<(usize, GrainEvent)>,
}

/// Sequence & VJ Set Arranger state: up to 12 named scenes, repeat counts
/// (`1..=16` bars or `0` = `∞` hold), and live `∞ hold` improv lock.
pub struct SeqState {
    pub scene_pats: [Option<usize>; SCENES],
    pub scene_names: [String; SCENES],
    pub scene: usize,
    pub queued: Option<usize>,
    /// Song / VJ set chain: `(scene_index, repeats_of_one_bar; 0 = ∞ hold)`.
    pub chain: Vec<(usize, u32)>,
    /// Live improv lock: holds the active scene endlessly until another is queued.
    pub hold_scene: bool,
    /// Scene whose name is currently being edited inline.
    pub editing_name: Option<usize>,
    /// Lane -> source (lane N = column N by default).
    pub lanes: [Option<usize>; LANES],
    /// Step whose hold sheet is open: (lane, step).
    pub hold: Option<(usize, usize)>,
    /// Per-(scene, lane, step) ratchet & chord metadata: `(ratchet 1..=8, slope_st, chord)`.
    pub step_ext: HashMap<(usize, usize, usize), (u8, f32, ChordChoice)>,
    /// Revision the arrangement was last rebuilt for.
    pub(crate) built_rev: u64,
}

pub struct Studio {
    pub view: View,
    pub columns: [Option<usize>; COLUMNS],
    pub emitters: [Emitter; EMITTERS],
    pub sel_emitter: usize,
    pub sel_lane: usize,
    pub sel_pad: usize,
    pub key: Key,
    pub seq: SeqState,
    pub deep: Option<(DeepTarget, DeepTab)>,
    pub rec: Option<Rec>,
    pub hud_until: f64,
    /// On-object gesture bubble: `(emitter_index, text, show_until_ui_time)`.
    pub gesture_hud: Option<(usize, String, f64)>,
    pub(crate) lane_links: [AvLink; LANES],
    pub(crate) lane_fx: [Vec<AvEffect>; LANES],
    pub lane_sculpt: [SculptParams; LANES],
    pub lane_captions: [String; LANES],
    pub(crate) pad_links: [AvLink; 16],
    pub pad_sculpt: [SculptParams; 16],
    pub pad_captions: [String; 16],
    /// Per-source smart crop focus `(cx, cy)` in `0.15..=0.85`.
    pub source_focus: HashMap<usize, (f32, f32)>,
    /// Per-(source, slice_index) user caption.
    pub slice_captions: HashMap<(usize, usize), String>,
    /// Active kinetic caption shown on stage: `(text, until_clock)`.
    pub active_caption: Option<(String, f64)>,
    /// Perform VJ Punch-In FX states (`Stutter`, `Grid ×`, `Cutout`, `Tape Stop`, `Comb`, `Crush`).
    pub punch_fx: [bool; 6],
    pub(crate) punch_press_t: [Option<f64>; 6],
    pub(crate) punch_key_held: [bool; 6],
    pub tape_stop_amt: f32,
    base_master_fx: Option<(Vec<AvEffect>, f32, f32)>,
    base_stage_mode: Option<StageMode>,
    /// Active pattern / global 16th-note swing (`0.50..=0.72`).
    pub swing: f32,
    /// Last applied zero-AI sequencer groove preset (`Ukg2Step..TapeDisintegration`).
    pub active_preset: Option<chromagrain_core::seq::PatternPreset>,
    /// Active lower-deck tab in `Sheet::Clip` (`0 = Sculpt & Tape`, `1 = Grain · FX · Links`).
    pub clip_tab: usize,
    /// Master 5-Band `ISOL8` Frequency Isolator (`LF · LMF · MF · HMF · HF`).
    pub isolator: chromagrain_core::dsp::BandIsolator,
    atlases: atlas::Atlases,
    grain_tex: Option<egui::TextureHandle>,
    /// Grid cells lit by sprays: (col, row) -> time.
    lit: Vec<((usize, usize), f64)>,
    pub(crate) bar_memo: Option<f64>,
    pub(crate) hold_rect: Option<Rect>,
    /// Fingers on the Play grid: (touch id, emitter).
    pub touches: Vec<(u64, usize)>,
    fonts_installed: bool,
    /// Fonts set via `set_fonts` apply from the next frame on.
    fonts_ready: bool,
    /// Bumped when the studio's sequence model changes.
    pub rev: u64,
    pub sheet: Option<Sheet>,
    pub add: add::AddState,
    pub chop: clip::ChopState,
    pub(crate) waves: crate::wave::WaveCache,
    /// Tile slots for emitter gestures and live one-shot pad hits (tile/split layout).
    pub tiles: TileAssigner,
    gestures: u64,
}

impl Default for Studio {
    fn default() -> Self {
        Studio {
            view: View::Play,
            columns: [None; COLUMNS],
            emitters: [
                Emitter::new(0, 0, 6.5),
                Emitter::new(1, 2, 5.5),
                Emitter::new(2, 5, 1.5),
                Emitter::new(3, 4, 6.4),
            ],
            sel_emitter: 0,
            sel_lane: 0,
            sel_pad: 0,
            key: Key::new(9, ScaleKind::Minor),
            seq: SeqState {
                scene_pats: [None; SCENES],
                scene_names: std::array::from_fn(|_| String::new()),
                scene: 0,
                queued: None,
                chain: vec![(0, 2)],
                hold_scene: false,
                editing_name: None,
                lanes: [None; LANES],
                hold: None,
                step_ext: HashMap::new(),
                built_rev: 0,
            },
            deep: None,
            rec: None,
            hud_until: 0.0,
            gesture_hud: None,
            lane_links: [AvLink::default(); LANES],
            lane_fx: std::array::from_fn(|_| Vec::new()),
            lane_sculpt: [SculptParams::default(); LANES],
            lane_captions: std::array::from_fn(|_| String::new()),
            pad_links: [AvLink::default(); 16],
            pad_sculpt: [SculptParams::default(); 16],
            pad_captions: std::array::from_fn(|_| String::new()),
            source_focus: HashMap::new(),
            slice_captions: HashMap::new(),
            active_caption: None,
            punch_fx: [false; 6],
            punch_press_t: [None; 6],
            punch_key_held: [false; 6],
            tape_stop_amt: 0.0,
            base_master_fx: None,
            base_stage_mode: None,
            swing: 0.50,
            active_preset: None,
            clip_tab: 0,
            isolator: chromagrain_core::dsp::BandIsolator::default(),
            atlases: atlas::Atlases::default(),
            grain_tex: None,
            lit: Vec::new(),
            bar_memo: None,
            hold_rect: None,
            touches: Vec::new(),
            fonts_installed: false,
            fonts_ready: false,
            rev: 1,
            sheet: None,
            add: add::AddState::default(),
            chop: clip::ChopState::default(),
            waves: crate::wave::WaveCache::default(),
            tiles: TileAssigner::new(),
            gestures: 0,
        }
    }
}

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

pub fn scale_label(s: ScaleKind) -> &'static str {
    match s {
        ScaleKind::Chromatic => "chromatic",
        ScaleKind::Major => "major",
        ScaleKind::Minor => "minor",
        ScaleKind::HarmonicMinor => "harm. minor",
        ScaleKind::MajorPentatonic => "major pent.",
        ScaleKind::MinorPentatonic => "minor pent.",
        ScaleKind::Blues => "blues",
        ScaleKind::Dorian => "dorian",
        ScaleKind::Phrygian => "phrygian",
        ScaleKind::Lydian => "lydian",
        ScaleKind::Mixolydian => "mixolydian",
        ScaleKind::WholeTone => "whole tone",
    }
}

/// Keyboard key for each perform pad, laid out like the grid.
pub const PAD_KEYS: [egui::Key; 16] = [
    egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4,
    egui::Key::Q, egui::Key::W, egui::Key::E, egui::Key::R,
    egui::Key::A, egui::Key::S, egui::Key::D, egui::Key::F,
    egui::Key::Z, egui::Key::X, egui::Key::C, egui::Key::V,
];

impl Studio {
    // ------------------------------------------------------------ model

    /// Fill empty grid columns with library sources (in order).
    pub fn sync_columns(&mut self, host: &dyn Host) {
        let n = host.project().lock().unwrap().sources.len();
        for c in self.columns.iter_mut() {
            if c.is_some_and(|s| s >= n) {
                *c = None;
            }
        }
        for s in 0..n {
            if self.columns.contains(&Some(s)) {
                continue;
            }
            if let Some(slot) = self.columns.iter_mut().take(COLUMNS - 1).find(|c| c.is_none()) {
                *slot = Some(s);
            }
        }
        for (i, lane) in self.seq.lanes.iter_mut().enumerate() {
            if lane.is_none_or(|s| s >= n) {
                *lane = self.columns[i];
            }
        }
    }

    pub fn route_deep(&mut self) {
        if let Some((target, _tab)) = self.deep.take() {
            match target {
                DeepTarget::Global => self.sheet = Some(Sheet::Project),
                DeepTarget::Emitter(i) => self.open_clip(ClipTarget::Emitter(i)),
                DeepTarget::Lane(l) => self.open_clip(ClipTarget::Lane(l)),
            }
        }
    }

    /// Per-frame engine work: emitters, buses, punch-in FX, rec, sequence arrangement.
    pub fn update(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        self.route_deep();
        if !self.fonts_installed {
            self.install_theme(ctx);
            ctx.request_repaint();
        } else {
            self.fonts_ready = true;
        }
        if let Some(m) = host.shared_macros() {
            for e in self.emitters.iter_mut() {
                e.macros = m;
            }
        }
        if let Some(k) = host.shared_key() {
            if k != self.key {
                self.key = k;
                self.key_changed(host);
            }
        }
        if let Some(iso) = host.isolator() {
            self.isolator = iso;
        }
        self.place_added(host);
        self.sync_columns(host);
        self.seq_ensure(host);
        self.seq_build_arrangement(host);
        self.seq_tick(host);
        self.sync_punch_fx(host, ctx);
        self.fire_emitters(host, ctx);
        let now = host.clock();
        self.lit.retain(|(_, t)| now - *t < 0.4);
        if self.active_caption.as_ref().is_some_and(|(_, exp)| now >= *exp) {
            self.active_caption = None;
        }
    }

    /// Apply Perform VJ Punch-In FX (`Stutter`, `Grid ×`, `Cutout`, `Tape Stop`, `Comb`, `Crush`)
    /// onto the stage mode, master effect chain & varispeed capstan while preserving base state.
    fn sync_punch_fx(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt.clamp(0.001, 0.1));
        // 3: Tape Stop — varispeed turntable brake via `host.set_tape_stop`.
        let prev_ts = self.tape_stop_amt;
        if self.punch_fx[3] {
            self.tape_stop_amt = (self.tape_stop_amt + dt * 1.65).clamp(0.0, 1.0);
            ctx.request_repaint();
        } else if self.tape_stop_amt > 0.0 {
            self.tape_stop_amt = (self.tape_stop_amt - dt * 3.2).max(0.0);
            ctx.request_repaint();
        }
        if (self.tape_stop_amt - prev_ts).abs() > 1e-4 {
            host.set_tape_stop(self.tape_stop_amt);
        }

        // 1 (`Grid ×`) & 2 (`Cutout`): momentary/latched stage mode override!
        if self.punch_fx[1] || self.punch_fx[2] {
            if self.base_stage_mode.is_none() {
                self.base_stage_mode = host.stage_mode();
            }
            let target_mode = if self.punch_fx[2] {
                StageMode::Collage
            } else {
                StageMode::Grid
            };
            if host.stage_mode() != Some(target_mode) {
                host.set_stage_mode(target_mode);
            }
        } else if let Some(prev_mode) = self.base_stage_mode.take() {
            if host.stage_mode() != Some(prev_mode) {
                host.set_stage_mode(prev_mode);
            }
        }

        let any_chain = self.punch_fx[0]
            || self.punch_fx[1]
            || self.punch_fx[4]
            || self.punch_fx[5];
        if !any_chain {
            if let Some((base_fx, base_age, base_wow)) = self.base_master_fx.take() {
                {
                    let mut p = host.project().lock().unwrap();
                    p.master_effects = base_fx;
                    p.master_tape.age = base_age;
                    p.master_tape.wow = base_wow;
                }
                host.mark_now();
            }
            return;
        }

        let bpm = host.project().lock().unwrap().bpm.max(40.0) as f32;
        if self.base_master_fx.is_none() {
            let p = host.project().lock().unwrap();
            self.base_master_fx = Some((p.master_effects.clone(), p.master_tape.age, p.master_tape.wow));
        }
        let Some((ref base_fx, base_age, base_wow)) = self.base_master_fx else { return };
        let mut chain = base_fx.clone();

        // 0: Stutter — 1/16 beat-repeat + crisp 2×2 non-mirrored tile cut.
        if self.punch_fx[0] {
            let sixteenth = (60.0 / bpm / 4.0).clamp(0.05, 0.35);
            chain.push(AvEffect {
                kind: EffectKind::Delay {
                    time: sixteenth,
                    feedback: 0.76,
                    mix: 0.78,
                    shift_x: 0.02,
                    shift_y: 0.0,
                },
                audio: 0.92,
                video: 0.85,
            });
            chain.push(AvEffect {
                kind: EffectKind::Tile { count: 2.0, mirror: 0.0, spin: 0.0 },
                audio: 0.85,
                video: 0.90,
            });
        }
        // 1: Grid × — crisp multi-cell 3×3 subdivision burst (zero mirror kaleidoscope).
        if self.punch_fx[1] {
            chain.push(AvEffect {
                kind: EffectKind::Tile { count: 3.0, mirror: 0.0, spin: 0.0 },
                audio: 0.65,
                video: 0.95,
            });
        }
        // 2: Cutout — handled above via `StageMode::Collage` die-cut sticker layering.
        // 3: Tape Stop — streamed directly via `host.set_tape_stop`.
        // 4: Comb — scale-locked metallic Karplus-Strong resonator.
        if self.punch_fx[4] {
            chain.push(AvEffect {
                kind: EffectKind::Tune {
                    root: self.key.root as f32,
                    scale: self.key.scale,
                    amount: 0.88,
                    focus: 0.82,
                },
                audio: 0.92,
                video: 0.80,
            });
        }
        // 5: Crush — crisp pixelated/bitcrushed print crunch, zero blur.
        if self.punch_fx[5] {
            chain.push(AvEffect {
                kind: EffectKind::Crush { bits: 5.0, downsample: 6.0 },
                audio: 0.80,
                video: 0.88,
            });
            chain.push(AvEffect {
                kind: EffectKind::Compress { quality: 0.36 },
                audio: 0.55,
                video: 0.65,
            });
        }

        let changed = {
            let mut p = host.project().lock().unwrap();
            let diff = p.master_effects != chain
                || (p.master_tape.age - base_age).abs() > 1e-3
                || (p.master_tape.wow - base_wow).abs() > 1e-3;
            if diff {
                p.master_effects = chain;
                p.master_tape.age = base_age;
                p.master_tape.wow = base_wow;
            }
            diff
        };
        if changed {
            host.mark_now();
        }
    }

    /// Toggle or momentarily engage Punch-In FX `idx` (`0..6`).
    pub fn set_punch_fx(&mut self, host: &mut dyn Host, idx: usize, on: bool, ctx: &egui::Context) {
        if idx < 6 && self.punch_fx[idx] != on {
            self.punch_fx[idx] = on;
            self.sync_punch_fx(host, ctx);
        }
    }

    /// The four emitter buses: grain style + SMEAR + LOOK + Clip-sheet FX, sound and picture.
    pub fn buses(&self) -> Vec<crate::gpu::LiveBus> {
        self.emitters
            .iter()
            .map(|e| {
                let map = MacroMap::resolve(&e.macros, Some(&self.key));
                let mut effects = emitter_effects(&map, &self.key);
                for extra in &e.effects {
                    if extra.audio > 0.005 || extra.video > 0.005 {
                        if let Some(existing) = effects.iter_mut().find(|f| f.kind.name() == extra.kind.name()) {
                            existing.audio = existing.audio.max(extra.audio);
                            existing.video = existing.video.max(extra.video);
                        } else {
                            effects.push(*extra);
                        }
                    }
                }
                let hard_burst = e.adsr.picture == chromagrain_core::envelope::PictureCut::Cut && e.cloud.spray <= 0.08;
                let style = if hard_burst {
                    VisualGrainStyle::full_frame(0.0)
                } else {
                    VisualGrainStyle::morph(0.0, &VisualGrainStyle { additive: 0.3, feather: 0.7, ..Default::default() }, map.texture)
                };
                crate::gpu::LiveBus {
                    style,
                    effects,
                    link: e.link,
                }
            })
            .collect()
    }

    /// What the canvas shows under the grains: in Play the selected
    /// emitter's tile of footage, playing; in Sequence the latest hit.
    pub fn backdrop(&self, host: &dyn Host) -> Option<(usize, f64)> {
        let now = host.clock();
        if let Some(tc) = self.chop.tap.as_ref() {
            if now - tc.t0 < tc.len {
                return Some((tc.source, (now - tc.t0).clamp(0.0, tc.len)));
            }
        }
        if let Some((src, i, t0)) = self.chop.playing {
            let slices = self.slices(host, src);
            if let Some(sl) = slices.get(i) {
                let at = sl.start + (now - t0);
                if at < sl.end {
                    return Some((src, at));
                }
            }
        }
        if self.view == View::Sequence {
            return self.seq_last_hit(host);
        }
        if self.view != View::Play {
            return None;
        }
        let e = &self.emitters[self.sel_emitter];
        let src = self.columns.get(e.col).copied().flatten()?;
        let len = host.project().lock().unwrap().sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
        if len <= 0.0 {
            return None;
        }
        let seg = len / ROWS as f64;
        let start = (e.row.floor() as f64 / ROWS as f64) * len;
        Some((src, start + now % seg))
    }

    fn fire_emitters(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let now = host.clock();
        let key = self.key;
        let sources: Vec<(f64, f32)> = host
            .project()
            .lock()
            .unwrap()
            .sources
            .iter()
            .map(|s| (s.duration(), s.base_hz))
            .collect();
        for (i, b) in self.buses().into_iter().enumerate() {
            host.set_bus(i as u8 + 1, b.effects);
        }
        let mut fired: Vec<(usize, u8, GrainEvent, (usize, usize), Option<String>)> = Vec::new();
        let mode = host.stage_mode().unwrap_or(StageMode::Stack);
        let assign_slot = matches!(mode, StageMode::Grid | StageMode::Split | StageMode::Collage);
        let pan_by_tile = matches!(mode, StageMode::Grid | StageMode::Collage) && host.project().lock().unwrap().layout_pan;
        let tape_spin_mul = (1.0 - 0.72 * self.tape_stop_amt).clamp(0.18, 1.0);
        self.tiles.expire(now);
        let mut released = Vec::new();
        for (i, e) in self.emitters.iter_mut().enumerate() {
            match (e.held, e.gesture) {
                (true, None) | (true, Some((_, _, Some(_)))) => {
                    self.gestures += 1;
                    let voice = 0xE000_0000_0000 | ((i as u64) << 32) | self.gestures;
                    e.gesture = Some((voice, now, None));
                    self.tiles.assign(voice, now);
                }
                (false, Some((v, on, None))) => {
                    e.gesture = Some((v, on, Some(now)));
                    self.tiles.release(v, now + e.adsr.release_secs());
                    released.push(v);
                }
                (false, Some((_, _, Some(off)))) if now >= off + e.adsr.release_secs() => e.gesture = None,
                _ => {}
            }
            let sounding = e.held || e.gesture.is_some_and(|(_, _, off)| off.is_some());
            let Some(src) = self.columns.get(e.col).copied().flatten() else { continue };
            let (len, src_hz) = sources.get(src).copied().unwrap_or((0.0, 0.0));
            if !sounding || len <= 0.0 {
                e.next_t = now;
                e.head = e.pos() as f64 * len;
                continue;
            }
            let (voice, on, off) = e.gesture.unwrap_or((0, now, None));
            let env = VoiceEnv { adsr: e.adsr, on, off: off.unwrap_or(f64::INFINITY) };
            let slot = if assign_slot { self.tiles.slot_of(voice).map(|s| s as u8) } else { None };
            let (cols, rows) = self.tiles.grid_at(now);
            let map = MacroMap::resolve(&e.macros, Some(&key));
            let mut n = 0;
            let base_note = (60 + e.octave * 12).clamp(24, 96) as u8;
            let vel = (e.level / 0.8 * 0.9).clamp(0.0, 1.2);
            let fine_ratio = if e.fine_cents.abs() > 0.01 {
                chromagrain_core::music::semitones_to_ratio(e.fine_cents / 100.0)
            } else {
                1.0
            };
            let stretch_mul = (1.0 - 0.75 * e.stretch.clamp(0.0, 1.0)).max(0.15) as f64;
            let cap = (!e.caption.trim().is_empty()).then(|| e.caption.trim().to_string());
            let core_sculpt = e.sculpt.to_core();
            let sculpt_pitch_ratio = if core_sculpt.pitch_semitones.abs() > 1e-4 {
                chromagrain_core::music::semitones_to_ratio(core_sculpt.pitch_semitones)
            } else {
                1.0
            };
            let core_chord = e.sculpt.chord.to_core();
            let hard_burst = e.adsr.picture == chromagrain_core::envelope::PictureCut::Cut && e.cloud.spray <= 0.08;
            let cap_inline = chromagrain_core::chop::Caption::new(&e.caption);
            while e.next_t <= now && n < 64 {
                let id = ((i as u64 + 1) << 48) | e.k;
                let (mut ev, hop) = grain_step(&map, &e.cloud, Some(&key), &mut e.rng, e.head, len, base_note, vel, id);
                ev.pitch_ratio *= fine_ratio * sculpt_pitch_ratio * tape_spin_mul;
                ev.onset = (e.next_t + ev.onset).max(now - 0.02);
                ev.sculpt = core_sculpt;
                ev.tag = VoiceTag {
                    voice,
                    env: Some(env),
                    tile: slot,
                    focus: e.focus,
                    choke_group: e.sculpt.choke,
                    caption: cap_inline,
                    hard_burst,
                    ..Default::default()
                };
                if let (Some(sl), true) = (slot, pan_by_tile) {
                    let (pan, gain) = layout_pan(sl as usize, cols, rows);
                    ev.pan = pan;
                    ev.gain *= gain;
                } else if e.pan.abs() > 1e-4 {
                    ev.pan = (ev.pan + e.pan).clamp(-1.0, 1.0);
                }
                if let Some(loop_s) = e.sculpt.free_loop_secs {
                    let span = (loop_s as f64).clamp(0.5, len);
                    let start = (e.pos() as f64 * len).min((len - span).max(0.0));
                    e.head = start + (e.head - start + hop * map.speed as f64 * stretch_mul).rem_euclid(span.max(0.1));
                } else {
                    e.head += hop * map.speed as f64 * stretch_mul;
                }
                e.next_t += hop;
                e.k += 1;
                n += 1;
                let cell_row = ((ev.source_pos / len) * ROWS as f64) as usize;
                let (chord_evs, chord_n) = chromagrain_core::grain::expand_chord_grain_inline(&ev, core_chord, src_hz, Some(&key));
                for (ci, cev) in chord_evs.into_iter().take(chord_n).enumerate() {
                    fired.push((
                        src,
                        i as u8 + 1,
                        cev,
                        (e.col, cell_row.min(ROWS - 1)),
                        if ci == 0 { cap.clone() } else { None },
                    ));
                }
                e.last_fire = now;
            }
            if e.next_t < now - 0.5 {
                e.next_t = now;
            }
        }
        for v in released {
            host.release_voice(v);
        }
        for (src, bus, ev, cell, cap) in fired {
            if let Some(c) = cap {
                self.active_caption = Some((c, now + ev.duration.max(0.45) as f64));
            }
            if let Some(rec) = self.rec.as_mut() {
                let mut r = ev.clone();
                r.onset -= rec.t0;
                if r.onset >= 0.0 {
                    rec.events.push((src, r));
                }
            }
            self.lit.push((cell, now));
            host.fire(bus, src, ev);
        }
        if self.emitters.iter().any(|e| e.held || e.gesture.is_some()) || self.rec.is_some() {
            ctx.request_repaint();
        }
    }

    pub fn toggle_rec(&mut self, host: &mut dyn Host) {
        if !host.caps().rec {
            return;
        }
        let now = host.clock();
        match self.rec.take() {
            None => {
                self.rec = Some(Rec { t0: now, beat0: host.playhead_beats(), events: Vec::new() });
                host.status("recording — what you play becomes a take".into());
            }
            Some(rec) => {
                if rec.events.is_empty() {
                    host.status("nothing played — no take".into());
                    return;
                }
                host.bounce_take(rec.events, now - rec.t0 + 0.5, rec.beat0);
            }
        }
    }

    // ------------------------------------------------------------ ui

    /// Fonts + visuals (call from the creation context so frame 1 has them).
    pub fn install_theme(&mut self, ctx: &egui::Context) {
        theme::install(ctx);
        self.fonts_installed = true;
    }

    /// The whole studio, every frame.
    pub fn ui(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        self.route_deep();
        if !self.fonts_ready {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(G0)).show(ctx, |_| {});
            return;
        }
        let caps = host.caps();
        if !caps.views.contains(&self.view) {
            self.view = caps.views.first().copied().unwrap_or(View::Play);
        }
        self.shortcuts(host, ctx);
        if host.fullscreen() {
            self.perform_fullscreen_ui(host, ctx);
            self.grain_overlay(ctx);
            return;
        }
        let w = ctx.screen_rect().width();
        let bar_h = if w < 1100.0 { 44.0 } else { 46.0 };
        egui::TopBottomPanel::top("studio_top")
            .exact_height(bar_h)
            .frame(egui::Frame::new().fill(CHASSIS_TOP))
            .show_separator_line(false)
            .show(ctx, |ui| {
                chassis_plate(ui.painter(), ui.max_rect(), CHASSIS_TOP, G0);
                self.top_bar(host, ui);
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(G0)).show(ctx, |ui| {
            let full = ui.max_rect();
            chassis_plate(ui.painter(), full, G0, CHASSIS_BOT);
            chassis_screws(ui.painter(), ctx.screen_rect(), 8.5);
            let pad = if full.width() < 1100.0 { 12.0 } else { 16.0 };
            let area = full.shrink(pad);
            match self.view {
                View::Play => self.play_view(host, ui, area),
                View::Sequence => self.sequence_view(host, ui, area),
                View::Perform => self.perform_view(host, ui, area),
            }
        });
        self.project_sheet(host, ctx);
        self.add_sheet(host, ctx);
        self.clip_sheet(host, ctx);
        self.grain_overlay(ctx);
    }

    pub fn set_view(&mut self, host: &mut dyn Host, v: View) {
        if v == self.view || !host.caps().views.contains(&v) {
            return;
        }
        self.seq.hold = None;
        self.seq.editing_name = None;
        for e in self.emitters.iter_mut() {
            e.held = false;
        }
        host.enter_view(v);
        self.view = v;
    }

    fn shortcuts(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if self.sheet.is_some() {
            let (esc, space) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Space)));
            if space && self.chop.tap.is_none() {
                host.toggle_play();
            }
            if esc {
                if self.chop.tap.is_some() {
                    self.chop.tap = None;
                    host.stop_audition();
                }
                self.sheet = None;
            }
            return;
        }
        let fullscreen = host.fullscreen();
        let (space, k1, k2, k3, tab, f, rk, esc, o) = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt;
            (
                i.key_pressed(egui::Key::Space),
                plain && i.key_pressed(egui::Key::Num1) && self.view != View::Perform,
                plain && i.key_pressed(egui::Key::Num2) && self.view != View::Perform,
                plain && i.key_pressed(egui::Key::Num3) && self.view != View::Perform,
                i.key_pressed(egui::Key::Tab) && !i.modifiers.any(),
                plain && i.key_pressed(egui::Key::F) && (self.view != View::Perform || i.modifiers.shift || fullscreen),
                plain && i.key_pressed(egui::Key::R) && self.view != View::Perform,
                i.key_pressed(egui::Key::Escape),
                plain && i.key_pressed(egui::Key::O),
            )
        });
        if space {
            host.toggle_play();
        }
        if k1 {
            self.set_view(host, View::Play);
        }
        if k2 {
            self.set_view(host, View::Sequence);
        }
        if k3 {
            self.set_view(host, View::Perform);
        }
        if tab {
            let views = host.caps().views;
            let i = views.iter().position(|v| *v == self.view).unwrap_or(0);
            let next = views[(i + 1) % views.len().max(1)];
            self.set_view(host, next);
        }
        if f {
            host.toggle_fullscreen(ctx);
        }
        if rk {
            self.toggle_rec(host);
        }
        if o {
            host.toggle_output();
        }
        if esc {
            if fullscreen {
                host.toggle_fullscreen(ctx);
            } else if self.sheet.is_some() {
                if self.chop.tap.is_some() {
                    self.chop.tap = None;
                    host.stop_audition();
                }
                self.sheet = None;
            } else if self.seq.hold.is_some() {
                self.seq.hold = None;
            } else if self.seq.editing_name.is_some() {
                self.seq.editing_name = None;
            } else {
                self.deep = None;
            }
        }
        // Number keys `[5, 6, 7, 8, 9, 0]`: momentary Punch-In FX while held (or single-frame tap toggle).
        let punch_states: [(bool, bool); 6] = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt;
            std::array::from_fn(|k| {
                let key = PUNCH_FX_KEYS[k];
                (plain && i.key_down(key), plain && i.key_pressed(key))
            })
        });
        for (k, (is_down, just_pressed)) in punch_states.into_iter().enumerate() {
            if is_down && !self.punch_key_held[k] {
                self.punch_key_held[k] = true;
                self.set_punch_fx(host, k, true, ctx);
            } else if !is_down && self.punch_key_held[k] {
                self.punch_key_held[k] = false;
                self.set_punch_fx(host, k, false, ctx);
            } else if just_pressed && !is_down {
                let next = !self.punch_fx[k];
                self.set_punch_fx(host, k, next, ctx);
            }
        }
        if self.view == View::Perform && !fullscreen {
            let hit: Vec<usize> = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt || i.modifiers.shift {
                    return Vec::new();
                }
                PAD_KEYS.iter().enumerate().filter(|(_, k)| i.key_pressed(**k)).map(|(n, _)| n).collect()
            });
            let pad_caps = self.pad_captions.clone();
            let now = host.clock();
            if let Some(perf) = host.perform() {
                for pad in hit {
                    perf.toggle_pad(pad);
                    if let Some(c) = pad_caps.get(pad).filter(|s| !s.trim().is_empty()) {
                        self.active_caption = Some((c.trim().to_string(), now + 1.2));
                    }
                }
            }
        }
    }

    pub fn active_swing(&self, host: &dyn Host) -> f32 {
        let (scene, _) = self.seq_current(host);
        if let Some(pid) = self.seq_pattern(scene) {
            if let Some(pat) = host.project().lock().unwrap().patterns.get(pid) {
                return pat.swing.clamp(0.50, 0.72);
            }
        }
        self.swing.clamp(0.50, 0.72)
    }

    pub fn set_active_swing(&mut self, host: &mut dyn Host, swing: f32) {
        let sw = swing.clamp(0.50, 0.72);
        self.swing = sw;
        {
            let mut p = host.project().lock().unwrap();
            for pid in self.seq.scene_pats.iter().flatten() {
                if let Some(pat) = p.patterns.get_mut(*pid) {
                    pat.swing = sw;
                }
            }
        }
        self.rev += 1;
        host.mark();
    }

    pub fn source_has_alpha(&self, host: &dyn Host, src: usize) -> bool {
        host.project()
            .lock()
            .unwrap()
            .sources
            .get(src)
            .and_then(|s| s.video.as_ref())
            .is_some_and(|v| v.has_alpha())
    }

    pub fn trigger_cutout(&mut self, host: &mut dyn Host, src: usize, mode: &str) -> bool {
        let ok = host.cutout_source(src, mode);
        if ok {
            host.set_stage_mode(StageMode::Collage);
            self.rev += 1;
            host.status(format!("{mode} cutout ready — collage sticker active"));
        }
        ok
    }

    pub fn trigger_ukg_kit(&mut self, host: &mut dyn Host) -> usize {
        let k = host.build_ukg_vox_kit_to_pads();
        if k > 0 {
            let (specs, any_alpha) = {
                let pp = host.project().lock().unwrap();
                let any_a = pp.sources.iter().any(|s| s.video.as_ref().is_some_and(|v| v.has_alpha()));
                let sp = chromagrain_core::chop::build_ukg_vox_kit_across_sources_opt(&pp.sources, Some(self.key));
                (sp, any_a)
            };
            for spc in &specs {
                let pad = spc.pad_index.min(15);
                self.pad_sculpt[pad] = crate::host::SculptParams::from_core(spc.stamp, spc.sculpt, spc.choke_group);
                if !spc.slice.caption.is_empty() {
                    self.pad_captions[pad] = spc.slice.caption.as_str().to_string();
                } else {
                    self.pad_captions[pad].clear();
                }
                let focus = spc.slice.focus;
                if !self.source_focus.contains_key(&spc.source)
                    && ((focus.0 - 0.5).abs() > 0.02 || (focus.1 - 0.5).abs() > 0.02)
                {
                    self.source_focus.insert(spc.source, focus);
                }
            }
            if any_alpha {
                host.set_stage_mode(StageMode::Collage);
            } else if matches!(host.stage_mode(), Some(StageMode::Stack | StageMode::Bleed)) {
                host.set_stage_mode(StageMode::Grid);
            }
            host.status(format!("harvested {k}-pad vocal percussion & chord kit"));
            // Keep Sequence on the exact same musical kit: re-bind every lane to its curated
            // role source + slice (steps untouched).
            self.sync_lanes_to_curated_kit(host);
        }
        k
    }

    pub fn trigger_chord_wash(&mut self, host: &mut dyn Host) -> usize {
        let k = host.build_chord_washes_to_pads();
        if k > 0 {
            let specs = {
                let pp = host.project().lock().unwrap();
                chromagrain_core::chop::build_chord_washes_across_sources_opt(&pp.sources, Some(self.key))
            };
            for spc in &specs {
                let pad = spc.pad_index.min(15);
                self.pad_sculpt[pad] = crate::host::SculptParams::from_core(spc.stamp, spc.sculpt, spc.choke_group);
                if !spc.slice.caption.is_empty() {
                    self.pad_captions[pad] = spc.slice.caption.as_str().to_string();
                } else {
                    self.pad_captions[pad].clear();
                }
                let focus = spc.slice.focus;
                if !self.source_focus.contains_key(&spc.source)
                    && ((focus.0 - 0.5).abs() > 0.02 || (focus.1 - 0.5).abs() > 0.02)
                {
                    self.source_focus.insert(spc.source, focus);
                }
            }
            // Ambient chord washes shine with soft optical crossfades (`Stack`).
            host.set_stage_mode(StageMode::Stack);
            host.status(format!("built {k}-pad video-to-chord ambient wash progression"));
        }
        k
    }

    fn top_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui) {
        let caps_cfg = host.caps();
        let rect = ui.max_rect();
        let small = rect.width() < 1180.0;
        let p = ui.painter().clone();
        let cy = rect.center().y;
        let bh = if small { 28.0 } else { 31.0 };

        // --- Centre: 3 physical interlock switches (`Play` · `Sequence` · `Perform`) ---
        let names: Vec<&str> = caps_cfg
            .views
            .iter()
            .map(|v| match v {
                View::Play => "Play",
                View::Sequence => "Sequence",
                View::Perform => "Perform",
            })
            .collect();
        let n = names.len().max(1) as f32;
        let sw = if small { 78.0 * n } else { 104.0 * n };
        let seg = Rect::from_center_size(egui::pos2(rect.center().x, cy), egui::vec2(sw, bh + 3.0));
        let hints: Vec<&str> = if small { Vec::new() } else { ["1", "2", "3"].into_iter().take(names.len()).collect() };
        let cur = caps_cfg.views.iter().position(|v| *v == self.view).unwrap_or(0);
        if let Some(i) = segmented(ui, seg, ui.id().with("views"), &names, &hints, cur, font_semibold(if small { 11.5 } else { 12.5 }), true) {
            self.set_view(host, caps_cfg.views[i]);
        }

        // --- Left: `VIDEOGRAIN` nameplate + Transport (`▶` / `■` / LCD) + `BPM` LCD + `SWING` LCD ---
        let mut x = rect.left() + if small { 12.0 } else { 18.0 };
        draw_icon(&p, Icon::Brand, egui::pos2(x + 7.0, cy), if small { 15.0 } else { 17.0 }, INK);
        x += if small { 19.0 } else { 22.0 };
        if !small {
            let r = text_spaced(
                &p,
                egui::pos2(x, cy - 4.5),
                Align2::LEFT_CENTER,
                "VIDEOGRAIN",
                font_semibold(13.0),
                INK,
                1.1,
            );
            p.text(
                egui::pos2(x, cy + 7.5),
                Align2::LEFT_CENTER,
                "AV SAMPLER",
                font_semibold(7.5),
                INK3,
            );
            x = r.right() + 14.0;
        } else {
            let r = text_spaced(
                &p,
                egui::pos2(x, cy),
                Align2::LEFT_CENTER,
                "VIDEOGRAIN",
                font_semibold(11.0),
                INK,
                0.6,
            );
            x = r.right() + 8.0;
        }

        let playing = host.playing();
        let play_w = if small { 30.0 } else { 36.0 };
        let b_play = Rect::from_min_size(egui::pos2(x, cy - bh * 0.5), egui::vec2(play_w, bh));
        if caps_cfg.transport {
            let resp = hit(ui, b_play, ui.id().with("play"), egui::Sense::click(), if playing { "stop" } else { "play" });
            paint_switch_cap(&p, b_play, SwitchKind::Amber, playing, resp.hovered(), resp.is_pointer_button_down_on());
            if resp.clicked() {
                host.toggle_play();
            }
        } else {
            paint_switch_cap(&p, b_play, SwitchKind::PuttyAmber, playing, false, false);
        }
        draw_icon(&p, Icon::Play, b_play.center(), if small { 13.0 } else { 14.5 }, if playing { ON_HI } else { INK });
        x = b_play.right() + 4.0;

        if caps_cfg.transport {
            let stop_w = if small { 28.0 } else { 32.0 };
            let b_stop = Rect::from_min_size(egui::pos2(x, cy - bh * 0.5), egui::vec2(stop_w, bh));
            let sresp = hit(ui, b_stop, ui.id().with("stop-btn"), egui::Sense::click(), "transport stop");
            paint_switch_cap(&p, b_stop, SwitchKind::PuttyAmber, false, sresp.hovered(), sresp.is_pointer_button_down_on());
            draw_icon(&p, Icon::Stop, b_stop.center(), if small { 12.0 } else { 13.5 }, INK);
            if sresp.clicked() {
                if host.playing() {
                    host.toggle_play();
                }
                host.seek(0.0);
                if let Some(perf) = host.perform() {
                    perf.stop_all();
                }
            }
            x = b_stop.right() + if small { 5.0 } else { 8.0 };
        }

        let beats = host.playhead_beats().max(0.0);
        let bar = (beats / 4.0).floor() as u32 + 1;
        let beat = (beats % 4.0).floor() as u32 + 1;
        let six = ((beats * 4.0) % 4.0).floor() as u32 + 1;
        let cnt_w = if small { 48.0 } else { 70.0 };
        let counter_r = Rect::from_min_size(egui::pos2(x, cy - 13.0), egui::vec2(cnt_w, 26.0));
        let cnt_txt = if small { format!("{bar:03}.{beat}") } else { format!("{bar:03}.{beat}.{six}") };
        sunken_display(&p, counter_r, &cnt_txt, if small { 10.0 } else { 11.0 });
        x = counter_r.right() + if small { 5.0 } else { 8.0 };

        // Draggable `BPM` silver LCD right in the top bar.
        let bpm = host.project().lock().unwrap().bpm;
        let bpm_w = if small { 52.0 } else { 74.0 };
        if x + bpm_w <= seg.left() - 6.0 {
            let tr = Rect::from_min_size(egui::pos2(x, cy - 13.0), egui::vec2(bpm_w, 26.0));
            if caps_cfg.tempo {
                let tresp = hit(ui, tr, ui.id().with("tempo"), egui::Sense::click_and_drag(), "tempo");
                if tresp.dragged() {
                    let nb = (bpm - tresp.drag_delta().y as f64 * 0.25).clamp(20.0, 300.0);
                    host.project().lock().unwrap().bpm = nb;
                    host.mark();
                } else if tresp.double_clicked() {
                    host.project().lock().unwrap().bpm = 134.0;
                    host.mark();
                }
            }
            let bpm_txt = if small { format!("{bpm:.0}") } else { format!("{bpm:.0} BPM") };
            lcd_readout(&p, tr, &bpm_txt, if small { 10.0 } else { 10.5 });
            x = tr.right() + if small { 4.0 } else { 6.0 };
        }

        // Draggable `SWING` silver LCD (`50%..72%`, `a11y: "swing"`).
        let cur_swing = self.active_swing(host);
        let swg_w = if small { 50.0 } else { 74.0 };
        if x + swg_w <= seg.left() - 4.0 {
            let sr = Rect::from_min_size(egui::pos2(x, cy - 13.0), egui::vec2(swg_w, 26.0));
            let sw_resp = hit(ui, sr, ui.id().with("top-swing"), egui::Sense::click_and_drag(), "swing");
            if sw_resp.dragged() {
                let nsw = (cur_swing - sw_resp.drag_delta().y * 0.0018).clamp(0.50, 0.72);
                self.set_active_swing(host, nsw);
            } else if sw_resp.double_clicked() {
                self.set_active_swing(host, 0.50);
            } else if sw_resp.clicked() {
                let presets = [0.50f32, 0.57, 0.61, 0.65];
                let next = presets.iter().copied().find(|&v| v > cur_swing + 0.015).unwrap_or(0.50);
                self.set_active_swing(host, next);
            }
            let sw_pct = (self.active_swing(host) * 100.0).round() as u32;
            let sw_txt = if small { format!("SW {sw_pct}%") } else { format!("SWG {sw_pct}%") };
            lcd_readout(&p, sr, &sw_txt, if small { 9.5 } else { 10.5 });
        }

        // --- Right: `+ Add` (`"top add"`), `Key` LCD, `Project`, `Export` ---
        let mut rx = rect.right() - if small { 12.0 } else { 18.0 };
        let cap_btn = |ui: &mut egui::Ui, rx: &mut f32, label: &str, a11y: &str, icon: Icon, id: &str, active: bool, amber: bool| -> egui::Response {
            let fnt = font_semibold(if small { 11.0 } else { 11.5 });
            let tw = ui.painter().layout_no_wrap(label.into(), fnt.clone(), INK).size().x;
            let w = if small && label != "Add" {
                20.0 + tw + 10.0
            } else {
                24.0 + tw + if small { 10.0 } else { 14.0 }
            };
            let r = Rect::from_min_size(egui::pos2(*rx - w, cy - bh * 0.5), egui::vec2(w, bh));
            *rx -= w + if small { 5.0 } else { 6.0 };
            let resp = hit(ui, r, ui.id().with(id), egui::Sense::click(), a11y);
            let p = ui.painter();
            let kind = if amber { SwitchKind::Amber } else { SwitchKind::PuttyAmber };
            paint_switch_cap(p, r, kind, active, resp.hovered(), resp.is_pointer_button_down_on());
            let fg = if active || amber { ON_HI } else { INK };
            let ix = r.left() + if small { 11.5 } else { 13.5 };
            draw_icon(p, icon, egui::pos2(ix, r.center().y), if small { 12.5 } else { 13.5 }, fg);
            p.text(egui::pos2(ix + if small { 9.5 } else { 11.0 }, r.center().y), Align2::LEFT_CENTER, label, fnt, fg);
            resp
        };
        if caps_cfg.export && cap_btn(ui, &mut rx, "Export", "Export", Icon::Export, "export", false, false).clicked() {
            host.export();
        }
        let proj_open = self.sheet == Some(Sheet::Project);
        if cap_btn(ui, &mut rx, "Project", "Project", Icon::Grid, "project", proj_open, false).clicked() {
            self.sheet = if proj_open { None } else { Some(Sheet::Project) };
        }

        // Key sunken slot: tap cycles the root, vertical drag the scale.
        let key = self.key;
        let kl = if small {
            match key.scale {
                ScaleKind::Minor => "MIN",
                ScaleKind::Major => "MAJ",
                ScaleKind::MinorPentatonic => "MPNT",
                ScaleKind::MajorPentatonic => "PNT",
                ScaleKind::Dorian => "DOR",
                ScaleKind::Blues => "BLU",
                _ => "SCL",
            }
        } else {
            match key.scale {
                ScaleKind::Minor => "MINOR",
                ScaleKind::Major => "MAJOR",
                ScaleKind::MinorPentatonic => "MIN PENT",
                ScaleKind::MajorPentatonic => "MAJ PENT",
                ScaleKind::Dorian => "DORIAN",
                ScaleKind::Blues => "BLUES",
                _ => "SCALE",
            }
        };
        let key_str = format!("{} {kl}", NOTE_NAMES[key.root as usize % 12]);
        let kw = (p.layout_no_wrap(key_str.clone(), font_semibold(10.0), SLOT_INK).size().x + if small { 14.0 } else { 20.0 }).max(52.0);
        if rx - kw >= seg.right() + 6.0 {
            let kr = Rect::from_min_size(egui::pos2(rx - kw, cy - 13.0), egui::vec2(kw, 26.0));
            let kresp = hit(ui, kr, ui.id().with("key"), egui::Sense::click_and_drag(), "global key");
            sunken_display(&p, kr, &key_str, if small { 9.5 } else { 10.0 });
            if kresp.clicked() {
                self.key = Key::new(key.root + 1, key.scale);
                self.key_changed(host);
            }
            if kresp.dragged() && kresp.drag_delta().y.abs() > 0.0 {
                let d = ui.data(|d| d.get_temp::<f32>(kresp.id).unwrap_or(0.0)) - kresp.drag_delta().y;
                if d.abs() > 18.0 {
                    let all = [ScaleKind::Minor, ScaleKind::Major, ScaleKind::MinorPentatonic, ScaleKind::MajorPentatonic, ScaleKind::Dorian, ScaleKind::Blues];
                    let i = all.iter().position(|s| *s == key.scale).unwrap_or(0) as i32 + d.signum() as i32;
                    self.key = Key::new(key.root, all[i.rem_euclid(all.len() as i32) as usize]);
                    self.key_changed(host);
                    ui.data_mut(|dd| dd.insert_temp(kresp.id, 0.0f32));
                } else {
                    ui.data_mut(|dd| dd.insert_temp(kresp.id, d));
                }
            }
            rx = kr.left() - if small { 5.0 } else { 6.0 };
        }

        // Prominent `+ Add` hardware button (`a11y: "top add"`).
        let add_open = matches!(self.sheet, Some(Sheet::Add(_)));
        if rx - 58.0 >= seg.right() + 4.0
            && cap_btn(ui, &mut rx, "Add", "top add", Icon::Plus, "top-add", add_open, true).clicked()
        {
            if add_open {
                self.sheet = None;
            } else {
                self.open_add(host, crate::host::AddTarget::Any);
            }
        }
    }

    pub fn key_changed(&mut self, host: &mut dyn Host) {
        let k = self.key;
        if host.shared_key() != Some(k) {
            host.set_shared_key(k);
        }
        let mut p = host.project().lock().unwrap();
        for pid in self.seq.scene_pats.iter().flatten() {
            if let Some(pat) = p.patterns.get_mut(*pid) {
                for r in &mut pat.rows {
                    r.key = Some(k);
                }
            }
        }
        drop(p);
        host.mark();
    }

    /// 2.8 % film grain over the whole window (one tiled noise texture).
    fn grain_overlay(&mut self, ctx: &egui::Context) {
        let tex = self.grain_tex.get_or_insert_with(|| {
            let n = 160usize;
            let mut rng = Rng::new(0x6A1A);
            let px = (0..n * n)
                .map(|_| {
                    let v = (rng.next_f32() * 255.0) as u8;
                    Color32::from_rgba_unmultiplied(v, v, v, 255)
                })
                .collect();
            ctx.load_texture(
                "cg-film-grain",
                egui::ColorImage { size: [n, n], pixels: px, source_size: egui::vec2(n as f32, n as f32) },
                egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST },
            )
        });
        let screen = ctx.screen_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("cg-grain")));
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(screen.width() / 160.0, screen.height() / 160.0));
        p.image(tex.id(), screen, uv, Color32::from_black_alpha(7));
    }

    pub fn atlas(&mut self, host: &dyn Host, ctx: &egui::Context, src: usize) -> Option<(egui::TextureId, atlas::AtlasView)> {
        let p = host.project().lock().unwrap();
        let a = self.atlases.get(ctx, &p.sources, src)?;
        Some((a.tex.id(), atlas::AtlasView::of(a)))
    }

    /// "uploader · 2014 · 212 views · CC" for dug / linked media.
    pub fn provenance_line(&self, host: &dyn Host, src: usize) -> String {
        let p = host.project().lock().unwrap();
        let Some(pv) = p.sources.get(src).and_then(|s| s.provenance.as_ref()) else { return String::new() };
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = pv.views {
            parts.push(if v == 1 { "1 view".into() } else { format!("{v} views") });
        }
        if pv.license.to_lowercase().contains("creative commons") {
            parts.push("CC".into());
        }
        if pv.upload_date.len() == 8 {
            parts.push(pv.upload_date[..4].to_string());
        }
        if !pv.uploader.is_empty() {
            parts.push(pv.uploader.clone());
        }
        parts.join(" · ")
    }

    pub fn source_name(&self, host: &dyn Host, src: usize) -> String {
        host.project().lock().unwrap().sources.get(src).map(|s| s.name.clone()).unwrap_or_default()
    }

    /// The live canvas (GPU), letterboxed/pillarboxed to `canvas_size` (`16:9 · 1:1 · 9:16`),
    /// recessed inside a dark broadcast bakelite bezel set into the sand-olive metal.
    pub fn stage(&mut self, host: &dyn Host, ui: &mut egui::Ui, rect: Rect, radius: f32) {
        let p = ui.painter().clone();
        let rad_u8 = (radius as u8).min(8);
        p.rect_filled(rect, r(rad_u8), Color32::from_rgb(0x0a, 0x0c, 0x0d));
        let inner = rect.shrink(2.5);
        let (cw, ch) = host.canvas_size();
        let canvas_ar = cw.max(1) as f32 / ch.max(1) as f32;
        let fitted = crate::fit(inner, canvas_ar);
        if let Some(scene) = host.canvas() {
            p.add(crate::gpu::PreviewCallback::shape(scene, fitted));
        }
        if (fitted.width() - inner.width()).abs() > 2.0 || (fitted.height() - inner.height()).abs() > 2.0 {
            p.rect_stroke(fitted, 0.0, hair(WELL_LINE), egui::StrokeKind::Inside);
        }
        // Kinetic spoken-hook caption bar (strictly only when a non-empty user/slice caption is active).
        let now = host.clock();
        if let Some((ref txt, exp)) = self.active_caption {
            if now < exp && !txt.trim().is_empty() && fitted.width() >= 140.0 && fitted.height() >= 80.0 {
                let upper = txt.trim().to_uppercase();
                let font_sz = (fitted.height() * 0.042).clamp(11.0, 16.0);
                let g = p.layout_no_wrap(play::short(&upper, 44), font_semibold(font_sz), CREAM);
                let bw = (g.size().x + 24.0).min(fitted.width() - 16.0);
                let bh = g.size().y + 10.0;
                let cy = fitted.top() + fitted.height() * 0.84;
                let box_r = Rect::from_center_size(egui::pos2(fitted.center().x, cy), egui::vec2(bw, bh));
                p.rect_filled(box_r, r(5), Color32::from_rgba_unmultiplied(14, 16, 17, 232));
                p.rect_stroke(box_r, r(5), hair(WELL_LINE2), egui::StrokeKind::Inside);
                let accent_r = Rect::from_min_size(egui::pos2(box_r.left() + 3.0, box_r.top() + 4.0), egui::vec2(2.5, bh - 8.0));
                p.rect_filled(accent_r, r(1), AMBER_MID);
                p.galley(egui::pos2(box_r.left() + 13.0, box_r.center().y - g.size().y * 0.5), g, CREAM);
            }
        }
        mask_corners(&p, rect, rad_u8 as f32, G0);
        monitor_bezel(&p, rect, rad_u8);
    }

    /// Clean, 100% unobstructed `#111314` Stage Monitor + dedicated Hardware Bezel Control Strip
    /// immediately below the CRT aperture (`grid · collage · split · stack` + `16:9 · 1:1 · 9:16` + `Fullscreen`).
    /// Returns `(stage_aperture_rect, bezel_strip_rect)`.
    pub fn stage_with_bezel(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        total_rect: Rect,
        id_salt: &str,
        show_fullscreen: bool,
    ) -> (Rect, Rect) {
        let bezel_h = if total_rect.width() < 420.0 { 25.0 } else { 28.0 };
        let gap = 4.0;
        let stage_h = (total_rect.height() - bezel_h - gap).max(72.0);
        let stage_r = Rect::from_min_size(total_rect.min, egui::vec2(total_rect.width(), stage_h));
        let bezel_r = Rect::from_min_size(
            egui::pos2(total_rect.left(), stage_r.bottom() + gap),
            egui::vec2(total_rect.width(), bezel_h),
        );
        self.stage(host, ui, stage_r, 7.0);
        self.stage_bezel_bar(host, ui, bezel_r, id_salt, show_fullscreen);
        (stage_r, bezel_r)
    }

    /// Dedicated recessed hardware bezel strip outside the video aperture:
    /// `grid · collage · split · stack` + `16:9 · 1:1 · 9:16` + optional `2nd` display + `Fullscreen`.
    pub fn stage_bezel_bar(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        bar: Rect,
        id_salt: &str,
        show_fullscreen: bool,
    ) {
        let p = ui.painter().clone();
        p.rect_filled(bar, r(4), SLOT_BG);
        p.line_segment(
            [egui::pos2(bar.left() + 2.0, bar.bottom() + 0.6), egui::pos2(bar.right() - 2.0, bar.bottom() + 0.6)],
            egui::Stroke::new(1.0, BEVEL_HI),
        );
        p.rect_stroke(bar, r(4), egui::Stroke::new(1.0, Color32::from_rgb(0x14, 0x13, 0x10)), egui::StrokeKind::Inside);

        let mut x = bar.left() + 3.0;
        let mut rx = bar.right() - 3.0;
        let inner_h = (bar.height() - 4.0).max(18.0);
        let top_y = bar.center().y - inner_h * 0.5;

        // Right: Fullscreen + optional Display 2 toggle.
        if show_fullscreen {
            let fs_a11y = if id_salt == "perf-stage" { "Fullscreen" } else { "stage fullscreen" };
            let wide_fs = bar.width() >= 470.0;
            let fs_w = if wide_fs { 86.0 } else { 26.0 };
            let fs_r = Rect::from_min_size(egui::pos2(rx - fs_w, top_y), egui::vec2(fs_w, inner_h));
            rx = fs_r.left() - 4.0;
            let fresp = hit(ui, fs_r, ui.id().with((id_salt, "fs")), egui::Sense::click(), fs_a11y);
            let f_hov = fresp.hovered() || fresp.is_pointer_button_down_on();
            if f_hov {
                p.rect_filled(fs_r, r(3), Color32::from_rgb(0x3a, 0x36, 0x2a));
            }
            if wide_fs {
                draw_icon(&p, Icon::Fullscreen, egui::pos2(fs_r.left() + 11.0, fs_r.center().y), 11.5, if f_hov { AMBER_MID } else { SLOT_INK });
                p.text(
                    egui::pos2(fs_r.left() + 21.0, fs_r.center().y),
                    Align2::LEFT_CENTER,
                    "Fullscreen",
                    font_semibold(9.5),
                    if f_hov { AMBER_MID } else { SLOT_INK },
                );
            } else {
                draw_icon(&p, Icon::Fullscreen, fs_r.center(), 12.0, if f_hov { AMBER_MID } else { SLOT_INK });
            }
            if fresp.clicked() {
                host.toggle_fullscreen(ui.ctx());
            }
        }

        if host.caps().output && bar.width() >= 540.0 {
            let out_open = host.perform().is_some_and(|pf| pf.output().is_some());
            let ow = 54.0;
            let out_r = Rect::from_min_size(egui::pos2(rx - ow, top_y), egui::vec2(ow, inner_h));
            rx = out_r.left() - 4.0;
            let oresp = hit(ui, out_r, ui.id().with((id_salt, "out2")), egui::Sense::click(), "Display 2");
            if out_open {
                radial_lens(&p, out_r, MINT_CORE, MINT_MID, MINT_EDGE);
            } else if oresp.hovered() {
                p.rect_filled(out_r, r(3), Color32::from_rgb(0x3a, 0x36, 0x2a));
            }
            p.text(
                out_r.center(),
                Align2::CENTER_CENTER,
                "Disp 2",
                font_semibold(9.5),
                if out_open { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { SLOT_INK },
            );
            if oresp.clicked() {
                if let Some(pf) = host.perform() {
                    if out_open {
                        pf.close_output();
                    } else {
                        pf.open_output();
                    }
                }
            }
        }

        // Left optional `STAGE` silkscreen when wide.
        if bar.width() >= 520.0 {
            let sr = text_spaced(
                &p,
                egui::pos2(x + 6.0, bar.center().y),
                Align2::LEFT_CENTER,
                "STAGE",
                font_semibold(8.5),
                Color32::from_rgb(0x8e, 0x87, 0x70),
                0.7,
            );
            x = sr.right() + 8.0;
        }

        // Primary 4 modes: `grid · collage · split · stack` (`"layout tile"`, `"layout collage"`, `"layout split"`, `"layout stack"`).
        let a11y_prefix = if id_salt == "clip-layout" { "clip layout" } else { "layout" };
        let a11y_owned = StageMode::ALL.map(|m| format!("{a11y_prefix} {}", m.a11y_suffix()));
        let a11y_refs = [
            a11y_owned[0].as_str(),
            a11y_owned[1].as_str(),
            a11y_owned[2].as_str(),
            a11y_owned[3].as_str(),
        ];
        let avail_for_modes = (rx - x).max(120.0);
        let mode_w = if avail_for_modes >= 292.0 {
            184.0
        } else if avail_for_modes >= 224.0 {
            156.0
        } else {
            avail_for_modes.min(168.0)
        };
        let used_r = self.stage_mode_bezel_cells(
            host,
            ui,
            Rect::from_min_size(egui::pos2(x, top_y), egui::vec2(mode_w, inner_h)),
            id_salt,
            a11y_refs,
        );
        x = used_r.right() + 6.0;

        // Aspect ratio bank: `16:9 · 1:1 · 9:16`.
        let Some(cur_aspect) = host.stage_aspect() else { return };
        let rem = rx - x;
        if rem >= 98.0 {
            let ar_w = rem.min(108.0);
            let ar_rect = Rect::from_min_size(egui::pos2(x, top_y), egui::vec2(ar_w, inner_h));
            self.aspect_bezel_cells(host, ui, ar_rect, cur_aspect, id_salt);
        } else if rem >= 36.0 {
            let cr = Rect::from_min_size(egui::pos2(rx - 38.0, top_y), egui::vec2(38.0, inner_h));
            p.rect_filled(cr, r(3), Color32::from_rgb(0x36, 0x33, 0x29));
            p.text(cr.center(), Align2::CENTER_CENTER, cur_aspect.label(), mono_medium(9.0), SLOT_INK);
            if hit(ui, cr, ui.id().with((id_salt, "aspect-cycle")), egui::Sense::click(), "aspect cycle").clicked() {
                host.set_stage_aspect(StageAspect::from_index((cur_aspect.index() + 1) % 3));
            }
        }
    }

    pub(crate) fn stage_mode_bezel_cells(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        r0: Rect,
        id_salt: &str,
        a11y: [&str; 4],
    ) -> Rect {
        let Some(cur) = host.stage_mode() else {
            return Rect::from_min_size(r0.min, egui::Vec2::ZERO);
        };
        let p = ui.painter().clone();
        let cell_w = r0.width() / 4.0;
        let fsz = if cell_w < 41.0 { 9.0 } else { 10.0 };
        for (k, m) in StageMode::ALL.into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(r0.left() + k as f32 * cell_w, r0.top()), egui::vec2(cell_w, r0.height())).shrink2(egui::vec2(1.0, 0.5));
            let on = cur == m || (cur == StageMode::Bleed && m == StageMode::Stack);
            let hr = Rect::from_min_max(egui::pos2(rc.left(), rc.top() - 2.0), egui::pos2(rc.right(), rc.bottom() + 2.0));
            let resp = hit(ui, hr, ui.id().with((id_salt, "sm", k)), egui::Sense::click(), a11y[k]);
            if on {
                radial_lens(&p, rc, AMBER_CORE, AMBER_MID, AMBER_EDGE);
            } else if resp.hovered() {
                p.rect_filled(rc, r(3), Color32::from_rgb(0x38, 0x35, 0x2b));
            }
            let col = if on { ON_HI } else { SLOT_INK };
            p.text(rc.center(), Align2::CENTER_CENTER, m.label(), font_semibold(fsz), col);
            if resp.clicked() && cur != m {
                host.set_stage_mode(m);
            }
        }
        r0
    }

    pub(crate) fn aspect_bezel_cells(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        r0: Rect,
        cur: StageAspect,
        id_salt: &str,
    ) {
        let p = ui.painter().clone();
        let cell_w = r0.width() / 3.0;
        let a11y = ["aspect 16:9", "aspect 1:1", "aspect 9:16"];
        for (k, ar) in StageAspect::ALL.into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(r0.left() + k as f32 * cell_w, r0.top()), egui::vec2(cell_w, r0.height())).shrink2(egui::vec2(1.0, 0.5));
            let on = cur == ar;
            let hr = Rect::from_min_max(egui::pos2(rc.left(), rc.top() - 2.0), egui::pos2(rc.right(), rc.bottom() + 2.0));
            let resp = hit(ui, hr, ui.id().with((id_salt, "ar", k)), egui::Sense::click(), a11y[k]);
            if on {
                radial_lens(&p, rc, MINT_CORE, MINT_MID, MINT_EDGE);
            } else if resp.hovered() {
                p.rect_filled(rc, r(3), Color32::from_rgb(0x38, 0x35, 0x2b));
            }
            let col = if on { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { SLOT_INK };
            p.text(rc.center(), Align2::CENTER_CENTER, ar.label(), mono_medium(9.0), col);
            if resp.clicked() && !on {
                host.set_stage_aspect(ar);
            }
        }
    }

    /// Signature `ISOL8 · 5-BAND FREQUENCY ISOLATOR` hardware rack unit!
    /// - When `rect.height() >= 185.0` (Perform's Left Console Column), renders in the
    ///   **authentic 3-tier vertical `ISOL8` hardware faceplate topology** (`clip-261113-175722.png`):
    ///   1. Top: `ISOL8 · 5-BAND FREQUENCY ISOLATOR` + `X1..X4` pointer knobs & `Hz` silver LCDs + red jewel `FILTER BYPASS`
    ///   2. Middle: `5×2` thick-bezel square backlit `LF..HF MUTE` (Ruby) & `SOLO` (Incandescent Amber) switches
    ///   3. Bottom: `CH. MODE` (`STEREO`) + `SLOPE` (`24 dB/Oct`) + `L R M S` Mint caps + jumbo `OUTPUT` knob + `ATTEN. DIM -18.0` putty plate
    /// - When `rect.height() < 185.0` (e.g. `Project` sheet), renders as a compact 1U horizontal rack strip.
    pub fn isolator_rack_ui(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, id_salt: &str) {
        use chromagrain_core::dsp::{IsolatorChannelMode, IsolatorMonitor};
        let p = ui.painter().clone();
        let id = ui.id().with(id_salt);
        chassis_plate(&p, rect, CHASSIS_TOP, CHASSIS_BOT);
        p.rect_stroke(rect, r(6), egui::Stroke::new(1.2, BEVEL_LO), egui::StrokeKind::Inside);
        if rect.height() >= 108.0 {
            chassis_screws(&p, rect, 6.5);
        }

        let mut iso = host.isolator().unwrap_or(self.isolator);
        let before = iso;

        let xover_ranges: [(f32, f32, f32, &str, &str); 4] = [
            (40.0, 500.0, 200.0, "X1", "isol8 x1"),
            (250.0, 2000.0, 700.0, "X2", "isol8 x2"),
            (1000.0, 6000.0, 2500.0, "X3", "isol8 x3"),
            (3500.0, 16000.0, 8000.0, "X4", "isol8 x4"),
        ];
        let bands: [(&str, &str, &str); 5] = [
            ("LF", "isol8 mute lf", "isol8 solo lf"),
            ("LMF", "isol8 mute lmf", "isol8 solo lmf"),
            ("MF", "isol8 mute mf", "isol8 solo mf"),
            ("HMF", "isol8 mute hmf", "isol8 solo hmf"),
            ("HF", "isol8 mute hf", "isol8 solo hf"),
        ];
        let mons = [
            ("L", "isol8 mon l", IsolatorMonitor::Left),
            ("R", "isol8 mon r", IsolatorMonitor::Right),
            ("M", "isol8 mon m", IsolatorMonitor::Mid),
            ("S", "isol8 mon s", IsolatorMonitor::Side),
        ];

        if rect.height() >= 185.0 {
            // =========================================================================
            // True 3-Tier `ISOL8` Hardware Proportions (`clip-261113-175722.png`)
            // =========================================================================
            let compact = rect.height() < 290.0 || rect.width() < 560.0;
            let inner = rect.shrink2(egui::vec2(if compact { 11.0 } else { 18.0 }, if compact { 8.0 } else { 13.0 }));
            let vgap = if compact { 6.0 } else { 11.0 };
            let top_h = (inner.height() * if compact { 0.30 } else { 0.29 }).clamp(64.0, 114.0);
            let bot_h = (inner.height() * if compact { 0.28 } else { 0.27 }).clamp(58.0, 106.0);
            let mid_h = (inner.height() - top_h - bot_h - vgap * 2.0).max(74.0);

            let top_r = Rect::from_min_size(inner.min, egui::vec2(inner.width(), top_h));
            let mid_r = Rect::from_min_size(egui::pos2(inner.left(), top_r.bottom() + vgap), egui::vec2(inner.width(), mid_h));
            let bot_r = Rect::from_min_max(egui::pos2(inner.left(), mid_r.bottom() + vgap), inner.max);

            // --- Tier 1: `ISOL8` Nameplate + `X1..X4` Knobs & Silver `Hz` LCDs + Red Jewel `BYPASS` ---
            let brand_w = if compact { 78.0 } else { 118.0 };
            let byp_w = if compact { 50.0 } else { 68.0 };
            text_spaced(
                &p,
                egui::pos2(top_r.left() + 2.0, top_r.top() + if compact { 10.0 } else { 13.0 }),
                Align2::LEFT_CENTER,
                "ISOL8",
                font_semibold(if compact { 14.0 } else { 18.5 }),
                INK,
                0.8,
            );
            let sub_lines: &[&str] = if compact {
                &["5-BAND", "ISOLATOR"]
            } else {
                &["5-BAND", "FREQUENCY", "ISOLATOR"]
            };
            for (li, line) in sub_lines.iter().enumerate() {
                p.text(
                    egui::pos2(top_r.left() + 2.5, top_r.top() + if compact { 24.0 } else { 31.0 } + li as f32 * if compact { 9.5 } else { 11.0 }),
                    Align2::LEFT_CENTER,
                    *line,
                    font_semibold(if compact { 7.5 } else { 8.5 }),
                    INK2,
                );
            }

            // Red Jewel `FILTER BYPASS` on top-right.
            let byp_x = top_r.right() - byp_w * 0.5;
            let led_y = top_r.top() + if compact { 7.0 } else { 10.0 };
            jewel_led(&p, egui::pos2(byp_x, led_y), if compact { 3.0 } else { 3.6 }, iso.bypass, true);
            let btn_y = led_y + if compact { 16.0 } else { 21.0 };
            let byp_btn = Rect::from_center_size(egui::pos2(byp_x, btn_y), egui::vec2(22.0, 22.0));
            let byp_resp = hit(ui, byp_btn.expand(4.0), id.with("bypass"), egui::Sense::click(), "isol8 bypass");
            let nut_r = if compact { 9.5 } else { 11.2 };
            p.circle_filled(byp_btn.center() + egui::vec2(-1.5, 2.2), nut_r + 0.5, Color32::from_black_alpha(85));
            p.circle_filled(byp_btn.center(), nut_r, Color32::from_rgb(0xd4, 0xcf, 0xc0));
            p.circle_stroke(byp_btn.center(), nut_r, egui::Stroke::new(1.1, Color32::from_rgb(0x4c, 0x48, 0x3b)));
            p.circle_stroke(byp_btn.center(), nut_r * 0.80, egui::Stroke::new(0.8, Color32::from_rgb(0xf2, 0xee, 0xe2)));
            p.circle_filled(
                byp_btn.center(),
                nut_r * 0.66,
                if iso.bypass { Color32::from_rgb(0xf0, 0x46, 0x38) } else { Color32::from_rgb(0xa8, 0x32, 0x28) },
            );
            p.circle_filled(byp_btn.center() + egui::vec2(-1.8, -1.8), 2.1, Color32::from_white_alpha(75));
            let byp_lbl_y = btn_y + nut_r + if compact { 6.5 } else { 8.5 };
            p.text(egui::pos2(byp_x, byp_lbl_y), Align2::CENTER_CENTER, "FILTER", font_semibold(if compact { 6.8 } else { 7.8 }), INK2);
            p.text(egui::pos2(byp_x, byp_lbl_y + if compact { 7.5 } else { 8.8 }), Align2::CENTER_CENTER, "BYPASS", font_semibold(if compact { 6.8 } else { 7.8 }), INK2);
            if byp_resp.clicked() {
                iso.bypass = !iso.bypass;
            }

            // 4 Crossover Pointer Knobs (`X1..X4`) + Sunken Silver `Hz` LCDs.
            let x_area = Rect::from_min_max(
                egui::pos2(top_r.left() + brand_w, top_r.top()),
                egui::pos2(top_r.right() - byp_w, top_r.bottom()),
            );
            let kw = x_area.width() / 4.0;
            let lcd_h = if compact { 15.5 } else { 19.0 };
            let krad = ((x_area.height() - lcd_h - if compact { 7.0 } else { 13.0 }) * 0.46).clamp(12.5, 22.5);
            let ky = x_area.top() + krad + 2.0;

            for (k, &(lo, hi, def_hz, tag, a11y)) in xover_ranges.iter().enumerate() {
                let cx = x_area.left() + (k as f32 + 0.5) * kw;
                let mut norm = ((iso.crossovers[k].clamp(lo, hi) - lo) / (hi - lo)).clamp(0.0, 1.0);
                let def_n = ((def_hz - lo) / (hi - lo)).clamp(0.0, 1.0);
                let prev_n = norm;
                knob(ui, egui::pos2(cx, ky), krad, id.with(("xover", k)), a11y, &mut norm, def_n, 0);
                if (norm - prev_n).abs() > 1e-5 {
                    let raw = lo + norm * (hi - lo);
                    let step = if raw >= 1000.0 { 50.0 } else { 10.0 };
                    iso.crossovers[k] = (raw / step).round() * step;
                }
                p.text(
                    egui::pos2(cx + krad * 0.84, ky - krad * 0.62),
                    Align2::LEFT_CENTER,
                    tag,
                    font_semibold(if compact { 8.0 } else { 9.5 }),
                    INK2,
                );
                let lcd_w = (kw - if compact { 8.0 } else { 14.0 }).clamp(38.0, 74.0);
                let lcd_r = Rect::from_center_size(egui::pos2(cx, x_area.bottom() - lcd_h * 0.5 - 1.0), egui::vec2(lcd_w, lcd_h));
                if !compact {
                    p.text(egui::pos2(lcd_r.right() - 1.0, lcd_r.top() - 5.5), Align2::RIGHT_CENTER, "Hz", font_semibold(7.5), INK3);
                }
                lcd_readout(&p, lcd_r, &format!("{:.0}", iso.crossovers[k]), if compact { 9.5 } else { 11.0 });
            }
            for k in 1..4 {
                if iso.crossovers[k] <= iso.crossovers[k - 1] + 40.0 {
                    iso.crossovers[k] = iso.crossovers[k - 1] + 40.0;
                }
            }

            // --- Tier 2: `5×2` Thick-Bezel Square Backlit `MUTE` (Ruby) & `SOLO` (Amber) Bank ---
            let bgap = if compact { 4.5 } else { 7.0 };
            let bw = (mid_r.width() - bgap * 4.0) / 5.0;
            let bh = (mid_r.height() - bgap) * 0.5;
            let sw_fsz = if bh < 40.0 || bw < 76.0 { 9.5 } else { 11.5 };
            for (bi, &(bname, mute_a11y, solo_a11y)) in bands.iter().enumerate() {
                let bx = mid_r.left() + bi as f32 * (bw + bgap);
                let r_mute = Rect::from_min_size(egui::pos2(bx, mid_r.top()), egui::vec2(bw, bh));
                let r_solo = Rect::from_min_size(egui::pos2(bx, mid_r.top() + bh + bgap), egui::vec2(bw, bh));
                if hw_switch(
                    ui,
                    r_mute,
                    id.with(("mute", bi)),
                    &format!("{bname}\nMUTE"),
                    mute_a11y,
                    SwitchKind::Ruby,
                    iso.mute[bi],
                    sw_fsz,
                )
                .clicked()
                {
                    iso.mute[bi] = !iso.mute[bi];
                }
                if hw_switch(
                    ui,
                    r_solo,
                    id.with(("solo", bi)),
                    &format!("{bname}\nSOLO"),
                    solo_a11y,
                    SwitchKind::Amber,
                    iso.solo[bi],
                    sw_fsz,
                )
                .clicked()
                {
                    iso.solo[bi] = !iso.solo[bi];
                }
            }

            // --- Tier 3: `CH. MODE / SLOPE` + `MONITOR L R M S` + Jumbo `OUTPUT` Knob + `ATTEN. DIM -18.0` ---
            let disp_w = (bot_r.width() * 0.24).clamp(84.0, 142.0);
            let atten_w = (bot_r.width() * 0.20).clamp(72.0, 114.0);
            let rem_mid = (bot_r.width() - disp_w - atten_w - 16.0).max(110.0);
            let mon_w = (rem_mid * 0.52).max(58.0);
            let out_w = (rem_mid - mon_w).max(52.0);

            // 3a. Left: `CH. MODE` & `SLOPE` sunken dark displays.
            let d_lbl_sz = if compact { 7.2 } else { 8.5 };
            let slot_h = ((bot_r.height() - if compact { 20.0 } else { 28.0 }) * 0.5).clamp(17.0, 24.0);
            caps(&p, egui::pos2(bot_r.left() + disp_w * 0.5, bot_r.top() + 5.0), Align2::CENTER_CENTER, "CH. MODE", d_lbl_sz);
            let ch_r = Rect::from_min_size(egui::pos2(bot_r.left(), bot_r.top() + if compact { 11.0 } else { 14.0 }), egui::vec2(disp_w, slot_h));
            sunken_display(&p, ch_r, iso.ch_mode.label(), if compact { 8.5 } else { 10.0 });
            if hit(ui, ch_r, id.with("ch-mode"), egui::Sense::click(), "isol8 ch mode").clicked() {
                iso.ch_mode = match iso.ch_mode {
                    IsolatorChannelMode::Stereo => IsolatorChannelMode::Mid,
                    IsolatorChannelMode::Mid => IsolatorChannelMode::Side,
                    IsolatorChannelMode::Side => IsolatorChannelMode::Stereo,
                };
            }
            let sl_lbl_y = ch_r.bottom() + if compact { 5.5 } else { 8.5 };
            caps(&p, egui::pos2(bot_r.left() + disp_w * 0.5, sl_lbl_y), Align2::CENTER_CENTER, "SLOPE", d_lbl_sz);
            let sl_r = Rect::from_min_size(egui::pos2(bot_r.left(), sl_lbl_y + if compact { 6.0 } else { 8.0 }), egui::vec2(disp_w, slot_h));
            sunken_display(&p, sl_r, "24 dB/Oct", if compact { 8.5 } else { 10.0 });

            // 3b. `MONITOR` `2×2` Putty/Mint switches (`L`, `R`, `M`, `S`).
            let mon_r = Rect::from_min_size(egui::pos2(bot_r.left() + disp_w + 8.0, bot_r.top()), egui::vec2(mon_w, bot_r.height()));
            caps(&p, egui::pos2(mon_r.center().x, mon_r.top() + 5.0), Align2::CENTER_CENTER, "MONITOR", d_lbl_sz);
            let m_top = mon_r.top() + if compact { 12.0 } else { 15.0 };
            let mg = if compact { 3.5 } else { 5.0 };
            let cell_s = ((mon_r.height() - (m_top - mon_r.top()) - mg) * 0.5)
                .min((mon_r.width() - mg) * 0.5)
                .clamp(19.0, 36.0);
            let mx0 = mon_r.center().x - (cell_s * 2.0 + mg) * 0.5;
            for (mi, &(mlbl, ma11y, mval)) in mons.iter().enumerate() {
                let mr = Rect::from_min_size(
                    egui::pos2(mx0 + (mi % 2) as f32 * (cell_s + mg), m_top + (mi / 2) as f32 * (cell_s + mg)),
                    egui::vec2(cell_s, cell_s),
                );
                let on = iso.monitor == mval;
                if hw_switch(ui, mr, id.with(("mon", mi)), mlbl, ma11y, SwitchKind::PuttyMint, on, if compact { 9.5 } else { 11.5 }).clicked() {
                    iso.monitor = if on { IsolatorMonitor::Off } else { mval };
                }
            }

            // 3c. Jumbo `OUTPUT` Charcoal Pointer Knob (`1.45×` crossover knob size, matching `ISOL8`).
            let out_r = Rect::from_min_size(egui::pos2(mon_r.right(), bot_r.top()), egui::vec2(out_w, bot_r.height()));
            caps(&p, egui::pos2(out_r.center().x, out_r.top() + 5.0), Align2::CENTER_CENTER, "OUTPUT", d_lbl_sz);
            let orad = ((out_r.height() - if compact { 14.0 } else { 18.0 }) * 0.45).clamp(18.0, 34.0);
            let oc = egui::pos2(out_r.center().x, out_r.top() + if compact { 12.0 } else { 15.0 } + orad);
            let mut n_out = ((iso.output_db.clamp(-24.0, 12.0) + 24.0) / 36.0).clamp(0.0, 1.0);
            let def_out = 24.0 / 36.0;
            let prev_out = n_out;
            knob(ui, oc, orad, id.with("output"), "isol8 output", &mut n_out, def_out, 0);
            if (n_out - prev_out).abs() > 1e-5 {
                iso.output_db = ((n_out * 36.0 - 24.0) * 2.0).round() * 0.5;
            }

            // 3d. `ATTEN.` Lighter Putty Sub-Plate (`jewel_led` + dark `DIM` cap + `-18.0` silver LCD).
            let att_outer = Rect::from_min_max(egui::pos2(bot_r.right() - atten_w, bot_r.top()), bot_r.max);
            caps(&p, egui::pos2(att_outer.center().x, att_outer.top() + 5.0), Align2::CENTER_CENTER, "ATTEN.", d_lbl_sz);
            let plate = Rect::from_min_max(egui::pos2(att_outer.left(), att_outer.top() + if compact { 12.0 } else { 15.0 }), att_outer.max);
            putty_panel(&p, plate, 5);
            jewel_led(&p, egui::pos2(plate.center().x, plate.top() + if compact { 6.5 } else { 8.5 }), if compact { 2.8 } else { 3.3 }, iso.dim, false);
            let lcd_a_h = if compact { 14.0 } else { 16.5 };
            let dim_top = plate.top() + if compact { 12.0 } else { 15.5 };
            let dim_bot = (plate.bottom() - lcd_a_h - if compact { 4.5 } else { 7.0 }).max(dim_top + 16.0);
            let dim_r = Rect::from_min_max(
                egui::pos2(plate.left() + 6.0, dim_top),
                egui::pos2(plate.right() - 6.0, dim_bot),
            );
            if hw_switch(ui, dim_r, id.with("dim"), "DIM", "isol8 dim", SwitchKind::DarkCap, iso.dim, if compact { 9.5 } else { 10.5 }).clicked() {
                iso.dim = !iso.dim;
            }
            let att_lcd = Rect::from_center_size(
                egui::pos2(plate.center().x, plate.bottom() - lcd_a_h * 0.5 - if compact { 2.5 } else { 4.0 }),
                egui::vec2((plate.width() - 12.0).max(40.0), lcd_a_h),
            );
            lcd_readout(&p, att_lcd, &format!("{:.1}", iso.dim_db), if compact { 9.0 } else { 10.0 });

            if iso != before {
                self.isolator = iso;
                host.set_isolator(iso);
            }
            return;
        }

        // =========================================================================
        // Compact 1U Horizontal Rack Strip (`Project` modal sheet)
        // =========================================================================
        let compact = rect.width() < 1040.0 || rect.height() < 126.0;
        let inner = rect.shrink2(egui::vec2(if compact { 12.0 } else { 16.0 }, if compact { 6.0 } else { 9.0 }));

        let left_w = (inner.width() * if compact { 0.33 } else { 0.32 }).clamp(242.0, 430.0);
        let right_w = (inner.width() * if compact { 0.27 } else { 0.32 }).clamp(210.0, 430.0);
        let gap = if compact { 8.0 } else { 14.0 };
        let center_w = (inner.width() - left_w - right_w - gap * 2.0).max(216.0);

        let left_r = Rect::from_min_size(inner.min, egui::vec2(left_w, inner.height()));
        let center_r = Rect::from_min_size(egui::pos2(left_r.right() + gap, inner.top()), egui::vec2(center_w, inner.height()));
        let right_r = Rect::from_min_max(egui::pos2(center_r.right() + gap, inner.top()), inner.max);

        let title_y = left_r.top() + if compact { 8.0 } else { 11.0 };
        let r_iso = text_spaced(
            &p,
            egui::pos2(left_r.left() + 2.0, title_y),
            Align2::LEFT_CENTER,
            "ISOL8",
            font_semibold(if compact { 13.0 } else { 15.5 }),
            INK,
            0.8,
        );
        if !compact {
            p.text(
                egui::pos2(r_iso.right() + 8.0, title_y + 0.5),
                Align2::LEFT_CENTER,
                "5-BAND FREQUENCY ISOLATOR",
                font_semibold(8.5),
                INK3,
            );
        }

        let byp_col_w = if compact { 32.0 } else { 44.0 };
        let byp_x = left_r.right() - byp_col_w * 0.5;
        let byp_led_y = left_r.top() + if compact { 12.0 } else { 18.0 };
        jewel_led(&p, egui::pos2(byp_x, byp_led_y), 3.6, iso.bypass, true);
        let byp_btn = Rect::from_center_size(egui::pos2(byp_x, byp_led_y + 19.0), egui::vec2(20.0, 20.0));
        let byp_resp = hit(ui, byp_btn.expand(4.0), id.with("bypass"), egui::Sense::click(), "isol8 bypass");
        p.circle_filled(byp_btn.center() + egui::vec2(-1.5, 2.0), 10.2, Color32::from_black_alpha(85));
        p.circle_filled(byp_btn.center(), 9.8, Color32::from_rgb(0xd4, 0xcf, 0xc0));
        p.circle_stroke(byp_btn.center(), 9.8, egui::Stroke::new(1.1, Color32::from_rgb(0x4c, 0x48, 0x3b)));
        p.circle_stroke(byp_btn.center(), 7.8, egui::Stroke::new(0.8, Color32::from_rgb(0xf2, 0xee, 0xe2)));
        p.circle_filled(
            byp_btn.center(),
            6.6,
            if iso.bypass { Color32::from_rgb(0xf0, 0x46, 0x38) } else { Color32::from_rgb(0xa8, 0x32, 0x28) },
        );
        p.circle_filled(byp_btn.center() + egui::vec2(-1.8, -1.8), 2.0, Color32::from_white_alpha(70));
        if !compact {
            p.text(egui::pos2(byp_x, byp_btn.bottom() + 7.5), Align2::CENTER_CENTER, "FILTER", font_semibold(7.5), INK2);
            p.text(egui::pos2(byp_x, byp_btn.bottom() + 15.5), Align2::CENTER_CENTER, "BYPASS", font_semibold(7.5), INK2);
        }
        if byp_resp.clicked() {
            iso.bypass = !iso.bypass;
        }

        let knobs_top = left_r.top() + if compact { 20.0 } else { 26.0 };
        let knobs_area = Rect::from_min_max(egui::pos2(left_r.left(), knobs_top), egui::pos2(left_r.right() - byp_col_w - 4.0, left_r.bottom()));
        let kw = knobs_area.width() / 4.0;
        let krad = ((knobs_area.height() - 28.0) * 0.42).clamp(12.5, 19.5);
        let ky = knobs_area.top() + krad + 1.0;

        for (k, &(lo, hi, def_hz, tag, a11y)) in xover_ranges.iter().enumerate() {
            let cx = knobs_area.left() + (k as f32 + 0.5) * kw;
            let mut norm = ((iso.crossovers[k].clamp(lo, hi) - lo) / (hi - lo)).clamp(0.0, 1.0);
            let def_n = ((def_hz - lo) / (hi - lo)).clamp(0.0, 1.0);
            let prev_n = norm;
            knob(ui, egui::pos2(cx, ky), krad, id.with(("xover", k)), a11y, &mut norm, def_n, 0);
            if (norm - prev_n).abs() > 1e-5 {
                let raw = lo + norm * (hi - lo);
                let step = if raw >= 1000.0 { 50.0 } else { 10.0 };
                iso.crossovers[k] = (raw / step).round() * step;
            }
            p.text(egui::pos2(cx + krad * 0.85, ky - krad * 0.65), Align2::LEFT_CENTER, tag, font_semibold(8.5), INK2);
            let lcd_w = (kw - 6.0).clamp(38.0, 60.0);
            let lcd_r = Rect::from_center_size(egui::pos2(cx, knobs_area.bottom() - 9.5), egui::vec2(lcd_w, 17.5));
            if !compact {
                p.text(egui::pos2(lcd_r.right() - 2.0, lcd_r.top() - 5.5), Align2::RIGHT_CENTER, "Hz", font_semibold(7.5), INK3);
            }
            lcd_readout(&p, lcd_r, &format!("{:.0}", iso.crossovers[k]), if compact { 9.5 } else { 10.5 });
        }
        for k in 1..4 {
            if iso.crossovers[k] <= iso.crossovers[k - 1] + 40.0 {
                iso.crossovers[k] = iso.crossovers[k - 1] + 40.0;
            }
        }

        let bgap = if compact { 4.0 } else { 6.0 };
        let bw = (center_r.width() - bgap * 4.0) / 5.0;
        let bh = (center_r.height() - bgap) * 0.5;
        let sw_fsz = if bw < 52.0 || bh < 42.0 { 9.5 } else { 11.0 };
        for (bi, &(bname, mute_a11y, solo_a11y)) in bands.iter().enumerate() {
            let bx = center_r.left() + bi as f32 * (bw + bgap);
            let r_mute = Rect::from_min_size(egui::pos2(bx, center_r.top()), egui::vec2(bw, bh));
            let r_solo = Rect::from_min_size(egui::pos2(bx, center_r.top() + bh + bgap), egui::vec2(bw, bh));
            if hw_switch(
                ui,
                r_mute,
                id.with(("mute", bi)),
                &format!("{bname}\nMUTE"),
                mute_a11y,
                SwitchKind::Ruby,
                iso.mute[bi],
                sw_fsz,
            )
            .clicked()
            {
                iso.mute[bi] = !iso.mute[bi];
            }
            if hw_switch(
                ui,
                r_solo,
                id.with(("solo", bi)),
                &format!("{bname}\nSOLO"),
                solo_a11y,
                SwitchKind::Amber,
                iso.solo[bi],
                sw_fsz,
            )
            .clicked()
            {
                iso.solo[bi] = !iso.solo[bi];
            }
        }

        let show_displays = right_r.width() >= 340.0 && !compact;
        let show_out_knob = right_r.width() >= 250.0;
        let mut rx = right_r.left();
        if show_displays {
            let disp_w = 78.0;
            caps(&p, egui::pos2(rx + disp_w * 0.5, right_r.top() + 8.0), Align2::CENTER_CENTER, "CH. MODE", 8.0);
            let ch_r = Rect::from_min_size(egui::pos2(rx, right_r.top() + 17.0), egui::vec2(disp_w, 24.0));
            sunken_display(&p, ch_r, iso.ch_mode.label(), 9.5);
            if hit(ui, ch_r, id.with("ch-mode"), egui::Sense::click(), "isol8 ch mode").clicked() {
                iso.ch_mode = match iso.ch_mode {
                    IsolatorChannelMode::Stereo => IsolatorChannelMode::Mid,
                    IsolatorChannelMode::Mid => IsolatorChannelMode::Side,
                    IsolatorChannelMode::Side => IsolatorChannelMode::Stereo,
                };
            }
            caps(&p, egui::pos2(rx + disp_w * 0.5, ch_r.bottom() + 12.0), Align2::CENTER_CENTER, "SLOPE", 8.0);
            let sl_r = Rect::from_min_size(egui::pos2(rx, ch_r.bottom() + 21.0), egui::vec2(disp_w, 24.0));
            sunken_display(&p, sl_r, "24 dB/Oct", 9.5);
            rx = ch_r.right() + 8.0;
        }

        let atten_w = if compact { 68.0 } else { 82.0 };
        let out_knob_w = if show_out_knob { if compact { 60.0 } else { 76.0 } } else { 0.0 };
        let mon_w = (right_r.right() - rx - atten_w - out_knob_w - if show_out_knob { 12.0 } else { 6.0 }).max(62.0);
        let mon_r = Rect::from_min_size(egui::pos2(rx, right_r.top()), egui::vec2(mon_w, right_r.height()));
        caps(&p, egui::pos2(mon_r.center().x, mon_r.top() + 7.0), Align2::CENTER_CENTER, "MONITOR", 8.0);
        let m_top = mon_r.top() + 16.0;
        let mg = 4.0;
        let mw = ((mon_r.width() - mg) * 0.5).min(36.0);
        let mh = ((mon_r.bottom() - m_top - mg) * 0.5).min(36.0);
        let mx0 = mon_r.center().x - (mw * 2.0 + mg) * 0.5;
        for (mi, &(mlbl, ma11y, mval)) in mons.iter().enumerate() {
            let mr = Rect::from_min_size(
                egui::pos2(mx0 + (mi % 2) as f32 * (mw + mg), m_top + (mi / 2) as f32 * (mh + mg)),
                egui::vec2(mw, mh),
            );
            let on = iso.monitor == mval;
            if hw_switch(ui, mr, id.with(("mon", mi)), mlbl, ma11y, SwitchKind::PuttyMint, on, 11.0).clicked() {
                iso.monitor = if on { IsolatorMonitor::Off } else { mval };
            }
        }

        if show_out_knob {
            let out_r = Rect::from_min_size(egui::pos2(mon_r.right() + 6.0, right_r.top()), egui::vec2(out_knob_w, right_r.height()));
            caps(&p, egui::pos2(out_r.center().x, out_r.top() + 7.0), Align2::CENTER_CENTER, "OUTPUT", 8.0);
            let orad = ((out_r.height() - 24.0) * 0.44).clamp(16.0, 26.0);
            let oc = egui::pos2(out_r.center().x, out_r.top() + 18.0 + orad);
            let mut n_out = ((iso.output_db.clamp(-24.0, 12.0) + 24.0) / 36.0).clamp(0.0, 1.0);
            let def_out = 24.0 / 36.0;
            let prev_out = n_out;
            knob(ui, oc, orad, id.with("output"), "isol8 output", &mut n_out, def_out, 0);
            if (n_out - prev_out).abs() > 1e-5 {
                iso.output_db = ((n_out * 36.0 - 24.0) * 2.0).round() * 0.5;
            }
        }

        let att_outer = Rect::from_min_max(egui::pos2(right_r.right() - atten_w, right_r.top()), right_r.max);
        caps(&p, egui::pos2(att_outer.center().x, att_outer.top() + 7.0), Align2::CENTER_CENTER, "ATTEN.", 8.0);
        let plate = Rect::from_min_max(egui::pos2(att_outer.left(), att_outer.top() + 15.0), att_outer.max);
        putty_panel(&p, plate, 5);
        jewel_led(&p, egui::pos2(plate.center().x, plate.top() + 9.0), 3.3, iso.dim, false);
        let dim_h = (plate.height() - 40.0).clamp(21.0, 29.0);
        let dim_r = Rect::from_center_size(
            egui::pos2(plate.center().x, plate.top() + 16.0 + dim_h * 0.5),
            egui::vec2((plate.width() - 10.0).max(44.0), dim_h),
        );
        if hw_switch(ui, dim_r, id.with("dim"), "DIM", "isol8 dim", SwitchKind::DarkCap, iso.dim, 10.5).clicked() {
            iso.dim = !iso.dim;
        }
        let att_lcd = Rect::from_center_size(
            egui::pos2(plate.center().x, plate.bottom() - 11.0),
            egui::vec2((plate.width() - 12.0).max(42.0), 16.5),
        );
        lcd_readout(&p, att_lcd, &format!("{:.1}", iso.dim_db), 9.5);

        if iso != before {
            self.isolator = iso;
            host.set_isolator(iso);
        }
    }
}

/// SMEAR's echo + reverb and the LOOK, as an AV effect chain.
pub fn emitter_effects(map: &MacroMap, key: &Key) -> Vec<AvEffect> {
    let mut out = Vec::new();
    if map.echo_mix > 0.001 || map.reverb_mix > 0.001 {
        out.push(AvEffect::new(EffectKind::Delay {
            time: 0.33,
            feedback: map.echo_feedback,
            mix: map.echo_mix,
            shift_x: 0.02,
            shift_y: 0.0,
        }));
        out.push(AvEffect::new(EffectKind::Reverb { size: 0.85, damp: 0.4, mix: map.reverb_mix }));
    }
    if let Some(name) = LOOKS.get(map.look).copied().flatten() {
        if let Some(mut kind) = EffectKind::parse(name) {
            if let EffectKind::Tune { root, scale, .. } = &mut kind {
                *root = key.root as f32;
                *scale = key.scale;
            }
            out.push(AvEffect::new(kind));
        }
    }
    out
}

pub fn note_name(pc: i32) -> &'static str {
    NOTE_NAMES[pc.rem_euclid(12) as usize]
}

/// Strip common media file extensions (`.mp4`, `.MOV`, etc.) from display titles.
pub fn strip_media_ext(s: &str) -> &str {
    for ext in [".mp4", ".mov", ".m4v", ".webm", ".mkv", ".wav", ".mp3", ".aif", ".aiff", ".flac"] {
        if s.len() > ext.len() && s.as_bytes()[s.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes()) {
            return &s[..s.len() - ext.len()];
        }
    }
    s
}

/// Clean human-readable clip title for pads, lanes, emitters, and headers:
/// strips `"person @ "` / `"subject @ "`, `" · Role"` / `" ~ loop"` suffixes,
/// crate prefixes (`wA1_`), trailing 11-char YouTube IDs (`_Ac7G7xOG2Ag`), and extensions.
pub fn clean_clip_label(raw: &str) -> String {
    let mut s = raw.trim();
    for prefix in ["person @ ", "subject @ ", "foreground @ "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim();
        }
    }
    if let Some((head, _)) = s.split_once(" · ") {
        s = head.trim();
    } else if let Some((head, _)) = s.split_once(" ~ ") {
        s = head.trim();
    }
    s = strip_media_ext(s);
    // Strip crate prefixes like `wA1_` or `wB12_`.
    let bytes = s.as_bytes();
    if bytes.len() > 4 && bytes[0] == b'w' && bytes[1].is_ascii_uppercase() {
        if let Some(us) = s[2..].find('_') {
            if s[2..2 + us].bytes().all(|b| b.is_ascii_digit()) {
                s = &s[2 + us + 1..];
            }
        }
    }
    // Strip trailing 11-char YouTube video ID (`_<11 id chars>`) when preceded by a word.
    if s.len() > 13 {
        let split_at = s.len() - 12;
        if s.as_bytes().get(split_at) == Some(&b'_') {
            let tail = &s[split_at + 1..];
            let looks_yt = tail.len() == 11
                && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && (tail.bytes().any(|b| b.is_ascii_uppercase()) || tail.bytes().any(|b| b.is_ascii_digit()));
            if looks_yt {
                s = &s[..split_at];
            }
        }
    }
    s.replace('_', " ")
}

/// Short uppercase role tag for a pad/lane (`KICK`, `SNARE`, `VOX`, `CHORD`, `BASS`, `HOOK`, `LOOP`, `CHOP`).
pub fn role_tag_for(stamp: crate::host::SculptStamp, raw_name: Option<&str>, kind: Option<crate::host::PadKind>) -> &'static str {
    use crate::host::{PadKind, SculptStamp};
    match stamp {
        SculptStamp::Kick => return "KICK",
        SculptStamp::Snare => return "SNARE",
        SculptStamp::VoxPerc | SculptStamp::Hat => return "VOX",
        SculptStamp::Chord => return "CHORD",
        SculptStamp::Pluck => return "BASS",
        SculptStamp::Phrase => return "HOOK",
        SculptStamp::Wash => return "WASH",
        SculptStamp::TapeLoop | SculptStamp::Disintegrate => return "LOOP",
        SculptStamp::Raw => {}
    }
    if let Some(nm) = raw_name {
        let low = nm.to_ascii_lowercase();
        if low.contains("kick") {
            return "KICK";
        } else if low.contains("snare") {
            return "SNARE";
        } else if low.contains("vox") || low.contains("hat") {
            return "VOX";
        } else if low.contains("chord") {
            return "CHORD";
        } else if low.contains("bass") || low.contains("sub") || low.contains("pluck") {
            return "BASS";
        } else if low.contains("phrase") || low.contains("hook") {
            return "HOOK";
        } else if low.contains("~ loop") || low.contains("tape") {
            return "LOOP";
        }
    }
    match kind {
        Some(PadKind::Chop) => "CHOP",
        Some(PadKind::Cloud) => "GRAIN",
        _ => "LOOP",
    }
}
