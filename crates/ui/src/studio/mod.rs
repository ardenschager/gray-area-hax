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

/// Names of the 6 Perform VJ Punch-In FX pills.
pub const PUNCH_FX_LABELS: [&str; 6] = [
    "Stutter",
    "Tape Stop",
    "Comb",
    "Dub",
    "Shimmer",
    "Wear",
];

pub const PUNCH_FX_A11Y: [&str; 6] = [
    "punch stutter",
    "punch tape stop",
    "punch comb",
    "punch dub",
    "punch shimmer",
    "punch wear",
];

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
    /// Perform VJ Punch-In FX states (`Stutter`, `Tape Stop`, `Comb`, `Dub`, `Shimmer`, `Wear`).
    pub punch_fx: [bool; 6],
    pub(crate) punch_press_t: [Option<f64>; 6],
    pub tape_stop_amt: f32,
    base_master_fx: Option<(Vec<AvEffect>, f32, f32)>,
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
            tape_stop_amt: 0.0,
            base_master_fx: None,
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

    /// Apply Perform VJ Punch-In FX (`Stutter`, `Tape Stop`, `Comb`, `Dub`, `Shimmer`, `Wear`)
    /// onto the master effect chain & tape processor while preserving the user's base FX.
    fn sync_punch_fx(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt.clamp(0.001, 0.1));
        let prev_ts = self.tape_stop_amt;
        if self.punch_fx[1] {
            self.tape_stop_amt = (self.tape_stop_amt + dt * 1.65).clamp(0.0, 1.0);
            ctx.request_repaint();
        } else if self.tape_stop_amt > 0.0 {
            self.tape_stop_amt = (self.tape_stop_amt - dt * 3.2).max(0.0);
            ctx.request_repaint();
        }
        if (self.tape_stop_amt - prev_ts).abs() > 1e-4 {
            host.set_tape_stop(self.tape_stop_amt);
        }

        let any_chain = self.punch_fx[0]
            || self.punch_fx[2]
            || self.punch_fx[3]
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
        let mut age = base_age;
        let mut wow = base_wow;

        // 0: Stutter — beat-locked 16th echo + 2×2 / 4×4 mirrored tile cut.
        if self.punch_fx[0] {
            let sixteenth = (60.0 / bpm / 4.0).clamp(0.05, 0.35);
            chain.push(AvEffect {
                kind: EffectKind::Delay {
                    time: sixteenth,
                    feedback: 0.76,
                    mix: 0.78,
                    shift_x: 0.035,
                    shift_y: 0.0,
                },
                audio: 0.92,
                video: 0.85,
            });
            chain.push(AvEffect {
                kind: EffectKind::Tile { count: 2.0, mirror: 1.0, spin: 0.0 },
                audio: 0.85,
                video: 0.90,
            });
        }
        // 1: Tape Stop — streamed directly via `host.set_tape_stop` (true varispeed capstan brake + wgpu VHS roll, zero engine swaps!).
        // 2: Comb — scale-locked Karplus/Tune resonator wash.
        if self.punch_fx[2] {
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
        // 3: Dub — dotted-8th ping-pong echo + frame ghosting.
        if self.punch_fx[3] {
            let dotted_8th = (60.0 / bpm * 0.75).clamp(0.12, 0.9);
            chain.push(AvEffect {
                kind: EffectKind::Delay {
                    time: dotted_8th,
                    feedback: 0.68,
                    mix: 0.62,
                    shift_x: 0.04,
                    shift_y: -0.015,
                },
                audio: 0.88,
                video: 0.82,
            });
        }
        // 4: Shimmer — lush octave/prism kaleidoscope + cathedral tail.
        if self.punch_fx[4] {
            if let Some(k) = EffectKind::parse("kaleido") {
                chain.push(AvEffect { kind: k, audio: 0.72, video: 0.78 });
            }
            chain.push(AvEffect {
                kind: EffectKind::Reverb { size: 0.94, damp: 0.22, mix: 0.62 },
                audio: 0.88,
                video: 0.80,
            });
        }
        // 5: Wear — Basinski oxide disintegration + warm crush/codec decay.
        if self.punch_fx[5] {
            age = age.max(0.82);
            wow = wow.max(0.58);
            chain.push(AvEffect {
                kind: EffectKind::Compress { quality: 0.32 },
                audio: 0.68,
                video: 0.72,
            });
            chain.push(AvEffect {
                kind: EffectKind::Reverb { size: 0.88, damp: 0.65, mix: 0.42 },
                audio: 0.65,
                video: 0.55,
            });
        }

        let changed = {
            let mut p = host.project().lock().unwrap();
            let diff = p.master_effects != chain
                || (p.master_tape.age - age).abs() > 1e-3
                || (p.master_tape.wow - wow).abs() > 1e-3;
            if diff {
                p.master_effects = chain;
                p.master_tape.age = age;
                p.master_tape.wow = wow;
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
        let assign_slot = matches!(mode, StageMode::Grid | StageMode::Split);
        let pan_by_tile = mode == StageMode::Grid && host.project().lock().unwrap().layout_pan;
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
        let bar_h = if w < 1100.0 { 52.0 } else { 56.0 };
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
            let pad = if full.width() < 1100.0 { 14.0 } else { 20.0 };
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

    fn top_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui) {
        let caps_cfg = host.caps();
        let rect = ui.max_rect();
        let small = rect.width() < 1100.0;
        let p = ui.painter().clone();
        let cy = rect.center().y;
        let mut x = rect.left() + if small { 16.0 } else { 22.0 };
        draw_icon(&p, Icon::Brand, egui::pos2(x + 8.0, cy), 18.0, INK);
        x += 24.0;
        if !small {
            let r = text_spaced(
                &p,
                egui::pos2(x, cy - 5.0),
                Align2::LEFT_CENTER,
                "CHROMAGRAIN",
                font_semibold(13.0),
                INK,
                1.0,
            );
            p.text(
                egui::pos2(x, cy + 8.0),
                Align2::LEFT_CENTER,
                "AV GRAIN CONSOLE",
                font_medium(8.0),
                INK3,
            );
            x = r.right() + 18.0;
        } else {
            x += 4.0;
        }
        let playing = host.playing();
        let b = Rect::from_min_size(egui::pos2(x, cy - 18.0), egui::vec2(44.0, 36.0));
        if caps_cfg.transport {
            let resp = hit(ui, b, ui.id().with("play"), egui::Sense::click(), if playing { "stop" } else { "play" });
            paint_switch_cap(&p, b, SwitchKind::Amber, playing, resp.hovered(), resp.is_pointer_button_down_on());
            if resp.clicked() {
                host.toggle_play();
            }
        } else {
            paint_switch_cap(&p, b, SwitchKind::PuttyAmber, playing, false, false);
        }
        draw_icon(&p, if playing { Icon::Stop } else { Icon::Play }, b.center(), 15.5, if playing { ON_HI } else { INK });
        x = b.right() + 6.0;
        if caps_cfg.rec {
            let b = Rect::from_min_size(egui::pos2(x, cy - 18.0), egui::vec2(44.0, 36.0));
            let recording = self.rec.is_some();
            let resp = hit(ui, b, ui.id().with("rec"), egui::Sense::click(), "record take");
            paint_switch_cap(&p, b, SwitchKind::Ruby, recording, resp.hovered(), resp.is_pointer_button_down_on());
            if resp.clicked() {
                self.toggle_rec(host);
            }
            if recording {
                p.circle_filled(b.center(), 5.5, Color32::from_rgb(0x24, 0x09, 0x07));
            } else {
                p.circle_filled(b.center(), 5.0, Color32::from_rgb(0x28, 0x10, 0x0d));
            }
            x = b.right() + 10.0;
        } else {
            x += 8.0;
        }
        let beats = host.playhead_beats().max(0.0);
        let bar = (beats / 4.0).floor() as u32 + 1;
        let beat = (beats % 4.0).floor() as u32 + 1;
        let six = ((beats * 4.0) % 4.0).floor() as u32 + 1;
        let counter_r = Rect::from_min_size(egui::pos2(x, cy - 14.0), egui::vec2(if small { 68.0 } else { 78.0 }, 28.0));
        sunken_display(&p, counter_r, &format!("{bar:03}.{beat}.{six}"), if small { 11.0 } else { 11.5 });

        // View switch (centre — recessed console keycap bank).
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
        let sw = if small { 92.0 * n } else { 112.0 * n };
        let seg = Rect::from_center_size(egui::pos2(rect.center().x, cy), egui::vec2(sw, 38.0));
        let hints: Vec<&str> = if small { Vec::new() } else { ["1", "2", "3"].into_iter().take(names.len()).collect() };
        let cur = caps_cfg.views.iter().position(|v| *v == self.view).unwrap_or(0);
        if let Some(i) = segmented(ui, seg, ui.id().with("views"), &names, &hints, cur, font_semibold(12.5), true) {
            self.set_view(host, caps_cfg.views[i]);
        }

        // Right: tempo LCD, key cap, Project, Export.
        let mut rx = rect.right() - if small { 16.0 } else { 22.0 };
        let cap_btn = |ui: &mut egui::Ui, rx: &mut f32, label: &str, icon: Icon, id: &str, active: bool| -> egui::Response {
            let w = if small { 40.0 } else { 26.0 + ui.painter().layout_no_wrap(label.into(), font_semibold(12.0), INK).size().x + 14.0 };
            let r = Rect::from_min_size(egui::pos2(*rx - w, cy - 18.0), egui::vec2(w, 36.0));
            *rx -= w + 6.0;
            let resp = hit(ui, r, ui.id().with(id), egui::Sense::click(), label);
            let p = ui.painter();
            paint_switch_cap(p, r, SwitchKind::PuttyAmber, active, resp.hovered(), resp.is_pointer_button_down_on());
            let fg = if active { ON_HI } else { INK };
            if small {
                draw_icon(p, icon, r.center(), 15.5, fg);
            } else {
                draw_icon(p, icon, egui::pos2(r.left() + 14.5, r.center().y), 14.0, fg);
                p.text(egui::pos2(r.left() + 26.5, r.center().y), Align2::LEFT_CENTER, label, font_semibold(12.0), fg);
            }
            resp
        };
        if caps_cfg.export && cap_btn(ui, &mut rx, "Export", Icon::Export, "export", false).clicked() {
            host.export();
        }
        let proj_open = self.sheet == Some(Sheet::Project);
        if cap_btn(ui, &mut rx, "Project", Icon::Grid, "project", proj_open).clicked() {
            self.sheet = if proj_open { None } else { Some(Sheet::Project) };
        }
        rx -= 4.0;
        // Key sunken slot: tap cycles the root, vertical drag the scale.
        let key = self.key;
        let kl = scale_label(key.scale);
        let kw = 24.0 + p.layout_no_wrap(kl.into(), font_medium(10.5), INK).size().x + 18.0;
        let kr = Rect::from_min_size(egui::pos2(rx - kw, cy - 15.0), egui::vec2(kw, 30.0));
        let kresp = hit(ui, kr, ui.id().with("key"), egui::Sense::click_and_drag(), "global key");
        sunken_display(&p, kr, &format!("{} {}", NOTE_NAMES[key.root as usize % 12], kl.to_uppercase()), 10.5);
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
        rx = kr.left() - 6.0;
        // Tempo sunken brushed-LCD readout: vertical drag adjusts BPM.
        let bpm = host.project().lock().unwrap().bpm;
        let tr = Rect::from_min_size(egui::pos2(rx - 76.0, cy - 15.0), egui::vec2(76.0, 30.0));
        if caps_cfg.tempo {
            let tresp = hit(ui, tr, ui.id().with("tempo"), egui::Sense::click_and_drag(), "tempo");
            if tresp.dragged() {
                let nb = (bpm - tresp.drag_delta().y as f64 * 0.25).clamp(20.0, 300.0);
                host.project().lock().unwrap().bpm = nb;
                host.mark();
            }
        }
        lcd_readout(&p, tr, &format!("{bpm:.0} BPM"), 11.0);
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

    /// Stage overlay pills in top-left corner: `stack | split | grid | bleed` +
    /// compact `16:9 · 1:1 · 9:16` aspect switch (standalone).
    pub fn layout_switch(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, stage: Rect) {
        let pad = if stage.width() < 180.0 { 6.0 } else { 10.0 };
        let used_r = self.stage_mode_pill(
            host,
            ui,
            egui::pos2(stage.left() + pad, stage.top() + pad),
            stage.width() - pad * 2.0,
            "layout",
            ["layout stack", "layout split", "layout tile", "layout bleed"],
        );
        let Some(cur_aspect) = host.stage_aspect() else { return };
        let rem = stage.right() - pad - used_r.right();
        if rem >= 112.0 {
            let ar_rect = Rect::from_min_size(egui::pos2(used_r.right() + 6.0, used_r.top()), egui::vec2(106.0, used_r.height()));
            self.aspect_pill_bar(host, ui, ar_rect, cur_aspect, "aspect");
        } else if rem >= 46.0 {
            let cr = Rect::from_min_size(egui::pos2(stage.right() - pad - 42.0, used_r.top()), egui::vec2(42.0, used_r.height()));
            let p = ui.painter().clone();
            p.rect_filled(cr, r(5), Color32::from_black_alpha(205));
            p.rect_stroke(cr, r(5), hair(WELL_LINE2), egui::StrokeKind::Inside);
            p.text(cr.center(), Align2::CENTER_CENTER, cur_aspect.label(), mono_medium(10.0), CREAM);
            if hit(ui, cr.expand2(egui::vec2(2.0, 3.0)), ui.id().with("aspect-cycle"), egui::Sense::click(), "aspect cycle").clicked() {
                host.set_stage_aspect(StageAspect::from_index((cur_aspect.index() + 1) % 3));
            }
        }
    }

    /// Backwards-compatible named stage mode pill (e.g. inside `Sheet::Clip`).
    pub fn layout_switch_named(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        pos: egui::Pos2,
        id_salt: &str,
        names: (&str, &str),
    ) {
        let prefix = names.0.strip_suffix(" stack").unwrap_or("layout");
        let split_a11y = format!("{prefix} split");
        let bleed_a11y = format!("{prefix} bleed");
        self.stage_mode_pill(
            host,
            ui,
            pos,
            240.0,
            id_salt,
            [names.0, split_a11y.as_str(), names.1, bleed_a11y.as_str()],
        );
    }

    pub(crate) fn stage_mode_pill(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        pos: egui::Pos2,
        avail_w: f32,
        id_salt: &str,
        a11y: [&str; 4],
    ) -> Rect {
        let Some(cur) = host.stage_mode() else {
            return Rect::from_min_size(pos, egui::Vec2::ZERO);
        };
        let tiny = avail_w < 164.0;
        let w = if tiny { avail_w.clamp(124.0, 144.0) } else { 164.0 };
        let h = if tiny { 25.0 } else { 28.0 };
        let r0 = Rect::from_min_size(pos, egui::vec2(w, h));
        let p = ui.painter().clone();
        p.rect_filled(r0, r(5), Color32::from_black_alpha(205));
        p.rect_stroke(r0, r(5), hair(WELL_LINE2), egui::StrokeKind::Inside);
        let cell_w = r0.width() / 4.0;
        let fsz = if tiny { 9.5 } else { 10.5 };
        for (k, m) in StageMode::ALL.into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(r0.left() + k as f32 * cell_w, r0.top()), egui::vec2(cell_w, r0.height())).shrink(2.2);
            let on = cur == m;
            if on {
                radial_lens(&p, rc, AMBER_CORE, AMBER_MID, AMBER_EDGE);
            }
            let col = if on { ON_HI } else { CREAM2 };
            p.text(rc.center(), Align2::CENTER_CENTER, m.label(), font_semibold(fsz), col);
            let hr = Rect::from_min_max(egui::pos2(rc.left(), rc.top() - 3.0), egui::pos2(rc.right(), rc.bottom() + 3.0));
            if hit(ui, hr, ui.id().with((id_salt, k)), egui::Sense::click(), a11y[k]).clicked() && !on {
                host.set_stage_mode(m);
            }
        }
        r0
    }

    pub(crate) fn aspect_pill_bar(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        r0: Rect,
        cur: StageAspect,
        id_salt: &str,
    ) {
        let p = ui.painter().clone();
        p.rect_filled(r0, r(5), Color32::from_black_alpha(205));
        p.rect_stroke(r0, r(5), hair(WELL_LINE2), egui::StrokeKind::Inside);
        let cell_w = r0.width() / 3.0;
        let a11y = ["aspect 16:9", "aspect 1:1", "aspect 9:16"];
        for (k, ar) in StageAspect::ALL.into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(r0.left() + k as f32 * cell_w, r0.top()), egui::vec2(cell_w, r0.height())).shrink(2.2);
            let on = cur == ar;
            if on {
                radial_lens(&p, rc, MINT_CORE, MINT_MID, MINT_EDGE);
            }
            let col = if on { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { CREAM2 };
            p.text(rc.center(), Align2::CENTER_CENTER, ar.label(), mono_medium(9.5), col);
            let hr = Rect::from_min_max(egui::pos2(rc.left(), rc.top() - 3.0), egui::pos2(rc.right(), rc.bottom() + 3.0));
            if hit(ui, hr, ui.id().with((id_salt, k)), egui::Sense::click(), a11y[k]).clicked() && !on {
                host.set_stage_aspect(ar);
            }
        }
    }

    /// Signature `ISOL8 · 5-BAND FREQUENCY MONITOR` hardware rack unit:
    /// `X1..X4` (`200 · 700 · 2500 · 8000 Hz`) pointer knobs + sunken LCDs,
    /// `5×2` backlit `LF · LMF · MF · HMF · HF` Ruby `MUTE` / Glowing Amber `SOLO`
    /// switch matrix, `L · R · M · S` Mint/Putty monitor switches, `STEREO` &
    /// `24 dB/Oct` sunken slots, `ATTEN. DIM -18.0` putty plate, and `BYPASS`!
    pub fn isolator_rack_ui(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, id_salt: &str) {
        let p = ui.painter().clone();
        let id = ui.id().with(id_salt);
        chassis_plate(&p, rect, CHASSIS_TOP, CHASSIS_BOT);
        p.rect_stroke(rect, r(6), egui::Stroke::new(1.2, BEVEL_LO), egui::StrokeKind::Inside);
        if rect.height() >= 108.0 {
            chassis_screws(&p, rect, 6.5);
        }

        let mut iso = host.isolator().unwrap_or(self.isolator);
        let before = iso;
        let compact = rect.width() < 1040.0 || rect.height() < 126.0;
        let inner = rect.shrink2(egui::vec2(if compact { 12.0 } else { 16.0 }, if compact { 6.0 } else { 9.0 }));

        // Three horizontal sections:
        // [Left: ISOL8 nameplate + X1..X4 knobs & LCDs] | [Center: 5×2 LF..HF MUTE/SOLO] | [Right: CH.MODE + L/R/M/S + OUTPUT + ATTEN. DIM]
        let left_w = (inner.width() * if compact { 0.33 } else { 0.32 }).clamp(242.0, 430.0);
        let right_w = (inner.width() * if compact { 0.27 } else { 0.32 }).clamp(210.0, 430.0);
        let gap = if compact { 8.0 } else { 14.0 };
        let center_w = (inner.width() - left_w - right_w - gap * 2.0).max(216.0);

        let left_r = Rect::from_min_size(inner.min, egui::vec2(left_w, inner.height()));
        let center_r = Rect::from_min_size(egui::pos2(left_r.right() + gap, inner.top()), egui::vec2(center_w, inner.height()));
        let right_r = Rect::from_min_max(egui::pos2(center_r.right() + gap, inner.top()), inner.max);

        // --- 1. Left: Nameplate + 4 Crossover Knobs (`X1..X4`) + Sunken LCDs ---
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
                "5-BAND FREQUENCY MONITOR",
                font_semibold(8.5),
                INK3,
            );
        }

        // Filter Bypass round red plunger + chrome collar + ruby LED + `FILTER BYPASS` silkscreen.
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

        let xover_ranges: [(f32, f32, f32, &str, &str); 4] = [
            (40.0, 500.0, 200.0, "X1", "isol8 x1"),
            (250.0, 2000.0, 700.0, "X2", "isol8 x2"),
            (1000.0, 6000.0, 2500.0, "X3", "isol8 x3"),
            (3500.0, 16000.0, 8000.0, "X4", "isol8 x4"),
        ];
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
        // Keep crossovers strictly monotonic.
        for k in 1..4 {
            if iso.crossovers[k] <= iso.crossovers[k - 1] + 40.0 {
                iso.crossovers[k] = iso.crossovers[k - 1] + 40.0;
            }
        }

        // --- 2. Center HERO: `5×2` Backlit Switch Bank (`LF · LMF · MF · HMF · HF`) ---
        let bands: [(&str, &str, &str); 5] = [
            ("LF", "isol8 mute lf", "isol8 solo lf"),
            ("LMF", "isol8 mute lmf", "isol8 solo lmf"),
            ("MF", "isol8 mute mf", "isol8 solo mf"),
            ("HMF", "isol8 mute hmf", "isol8 solo hmf"),
            ("HF", "isol8 mute hf", "isol8 solo hf"),
        ];
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

        // --- 3. Right: `CH. MODE` / `SLOPE` + `L R M S` Putty-Mint Keycaps + `OUTPUT` Knob + `ATTEN. DIM -18.0` ---
        use chromagrain_core::dsp::{IsolatorChannelMode, IsolatorMonitor};
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
        let mons = [
            ("L", "isol8 mon l", IsolatorMonitor::Left),
            ("R", "isol8 mon r", IsolatorMonitor::Right),
            ("M", "isol8 mon m", IsolatorMonitor::Mid),
            ("S", "isol8 mon s", IsolatorMonitor::Side),
        ];
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

        // Physical large `OUTPUT` pointer knob (`-24..+12 dB`, default `0.0 dB`).
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

        // `ATTEN.` lighter putty inset plate (`jewel_led` + dark `DIM` cap + `-18.0` LCD).
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
        let att_txt = format!("{:.1}", iso.dim_db);
        lcd_readout(&p, att_lcd, &att_txt, 9.5);

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
