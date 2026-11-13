//! The play-first studio UI (design v001): three views — Play (Tempera
//! grid + emitters), Sequence (nanoloop lanes + scenes), Perform (pads +
//! output) — one Deep drawer, and a tiny top bar. Hosts (the standalone
//! app, the plugin) implement [`crate::host::Host`].
//!
//! Engine mapping:
//! * Emitters fire live grains through per-emitter effect buses
//!   (playback + GPU), using the core `instrument::grain_step` /
//!   `MacroMap` — the same macro definition the plugin uses.
//! * Sequence lanes are rows of one `StepPattern` per scene; the song
//!   chain lays scene pattern clips out on a "sequence" arrangement track.
//! * Perform pads are the existing perform-mode pad tracks.
//! * Rec captures the emitters' grains and bounces them (core `take`) into
//!   a new source placed on a "takes" track.

pub mod add;
pub mod clip;
mod deep;
mod perform;
mod play;
pub mod sequence;

use crate::atlas;
use crate::host::Host;
use crate::theme;
use chromagrain_core::dsp::Rng;
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::envelope::{Adsr, VoiceEnv};
use chromagrain_core::grain::VoiceTag;
use chromagrain_core::instrument::{grain_step, Cloud, MacroMap, LOOKS};
use chromagrain_core::tile::{layout_pan, TileAssigner};
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::video::{AvLink, VisualGrainStyle};
use egui::{self, Align2, Color32, Rect};
use crate::theme::*;
use crate::widgets::*;

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
pub const SCENES: usize = 8;
pub const EMITTERS: usize = 4;

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

/// A brush on the Play grid with its own grain settings.
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

/// Sequence state: one pattern per scene; lane i is row i of each.
pub struct SeqState {
    pub scene_pats: [Option<usize>; SCENES],
    pub scene: usize,
    pub queued: Option<usize>,
    /// Song chain: (scene, repeats of one bar).
    pub chain: Vec<(usize, u32)>,
    /// Lane -> source (lane N = column N by default).
    pub lanes: [Option<usize>; LANES],
    /// Step whose hold sheet is open: (lane, step).
    pub hold: Option<(usize, usize)>,
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
    pub(crate) pad_links: [AvLink; 16],
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
    /// Tile slots for emitter gestures and live one-shot pad hits (tile layout).
    pub tiles: TileAssigner,
    gestures: u64,
}

impl Default for Studio {
    fn default() -> Self {
        Studio {
            view: View::Play,
            columns: [None; COLUMNS],
            emitters: [Emitter::new(0, 0, 6.5), Emitter::new(1, 2, 5.5), Emitter::new(2, 5, 1.5), Emitter::new(3, 4, 6.4)],
            sel_emitter: 0,
            sel_lane: 0,
            sel_pad: 0,
            key: Key::new(9, ScaleKind::Minor),
            seq: SeqState {
                scene_pats: [None; SCENES],
                scene: 0,
                queued: None,
                chain: vec![(0, 2)],
                lanes: [None; LANES],
                hold: None,
                built_rev: 0,
            },
            deep: None,
            rec: None,
            hud_until: 0.0,
            gesture_hud: None,
            lane_links: [AvLink::default(); LANES],
            lane_fx: std::array::from_fn(|_| Vec::new()),
            pad_links: [AvLink::default(); 16],
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

    /// Per-frame engine work: emitters, buses, rec, sequence arrangement.
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
        self.place_added(host);
        self.sync_columns(host);
        self.seq_ensure(host);
        self.seq_build_arrangement(host);
        self.seq_tick(host);
        self.fire_emitters(host, ctx);
        let now = host.clock();
        self.lit.retain(|(_, t)| now - *t < 0.4);
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
                crate::gpu::LiveBus {
                    // Sample end: the footage itself (over, not glow); the
                    // cloud end glows like classic grain clouds.
                    // grain end: soft-edged patches that glow, never hard blocks.
                    style: VisualGrainStyle::morph(0.0, &VisualGrainStyle { additive: 0.3, feather: 0.7, ..Default::default() }, map.texture),
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
        let sources: Vec<f64> = host.project().lock().unwrap().sources.iter().map(|s| s.duration()).collect();
        // Bus effects follow the knobs.
        for (i, b) in self.buses().into_iter().enumerate() {
            host.set_bus(i as u8 + 1, b.effects);
        }
        let mut fired: Vec<(usize, u8, GrainEvent, (usize, usize))> = Vec::new();
        let tile = host.tile_layout() == Some(true);
        let pan_by_tile = tile && host.project().lock().unwrap().layout_pan;
        self.tiles.expire(now);
        let mut released = Vec::new();
        for (i, e) in self.emitters.iter_mut().enumerate() {
            // Gestures: press = note-on, release = note-off; grains keep
            // coming through the release so sound and picture end together.
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
            let len = sources.get(src).copied().unwrap_or(0.0);
            if !sounding || len <= 0.0 {
                e.next_t = now;
                e.head = e.pos() as f64 * len;
                continue;
            }
            let (voice, on, off) = e.gesture.unwrap_or((0, now, None));
            let env = VoiceEnv { adsr: e.adsr, on, off: off.unwrap_or(f64::INFINITY) };
            let slot = if tile { self.tiles.slot_of(voice).map(|s| s as u8) } else { None };
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
            while e.next_t <= now && n < 64 {
                let id = ((i as u64 + 1) << 48) | e.k;
                let (mut ev, hop) = grain_step(&map, &e.cloud, Some(&key), &mut e.rng, e.head, len, base_note, vel, id);
                ev.pitch_ratio *= fine_ratio;
                ev.onset = (e.next_t + ev.onset).max(now - 0.02);
                ev.tag = VoiceTag { voice, env: Some(env), tile: slot };
                if let (Some(sl), true) = (slot, pan_by_tile) {
                    let (pan, gain) = layout_pan(sl as usize, cols, rows);
                    ev.pan = pan;
                    ev.gain *= gain;
                } else if e.pan.abs() > 1e-4 {
                    ev.pan = (ev.pan + e.pan).clamp(-1.0, 1.0);
                }
                e.head += hop * map.speed as f64 * stretch_mul;
                e.next_t += hop;
                e.k += 1;
                n += 1;
                let cell_row = ((ev.source_pos / len) * ROWS as f64) as usize;
                fired.push((src, i as u8 + 1, ev, (e.col, cell_row.min(ROWS - 1))));
                e.last_fire = now;
            }
            if e.next_t < now - 0.5 {
                e.next_t = now;
            }
        }
        for v in released {
            host.release_voice(v);
        }
        for (src, bus, ev, cell) in fired {
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
            .frame(egui::Frame::new().fill(G0))
            .show_separator_line(false)
            .show(ctx, |ui| self.top_bar(host, ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(G0)).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment([full.left_top(), full.right_top()], hair(LINE));
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
            // Sheets are modal: only Esc (close) and Space (transport).
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
            if let Some(perf) = host.perform() {
                for pad in hit {
                    perf.toggle_pad(pad);
                }
            }
        }
    }

    fn top_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui) {
        let caps = host.caps();
        let rect = ui.max_rect();
        let small = rect.width() < 1100.0;
        let p = ui.painter().clone();
        let cy = rect.center().y;
        let mut x = rect.left() + if small { 14.0 } else { 18.0 };
        draw_icon(&p, Icon::Brand, egui::pos2(x + 8.0, cy), 18.0, INK);
        x += 26.0;
        if !small {
            let r = p.text(egui::pos2(x, cy), Align2::LEFT_CENTER, "chromagrain", font_semibold(14.0), INK);
            x = r.right() + 22.0;
        } else {
            x += 4.0;
        }
        // Transport (a plugin shows the host's state, read-only).
        let playing = host.playing();
        let b = Rect::from_min_size(egui::pos2(x, cy - 19.0), egui::vec2(44.0, 38.0));
        if caps.transport {
            if chip(ui, b, ui.id().with("play"), if playing { "stop" } else { "play" }, false, playing).clicked() {
                host.toggle_play();
            }
        } else if playing {
            p.rect_filled(b, r(10), HI);
        } else {
            raised(&p, b, 10, G2);
        }
        draw_icon(&p, if playing { Icon::Stop } else { Icon::Play }, b.center(), 16.0, if playing { ON_HI } else { INK });
        x = b.right() + 6.0;
        if caps.rec {
            let b = Rect::from_min_size(egui::pos2(x, cy - 19.0), egui::vec2(44.0, 38.0));
            let recording = self.rec.is_some();
            if chip(ui, b, ui.id().with("rec"), "record take", false, false).clicked() {
                self.toggle_rec(host);
            }
            if recording {
                p.circle_filled(b.center(), 6.0, REC);
            } else {
                p.circle_stroke(b.center(), 6.0, egui::Stroke::new(1.5, INK2));
            }
            x = b.right() + 16.0;
        } else {
            x += 10.0;
        }
        let beats = host.playhead_beats().max(0.0);
        let bar = (beats / 4.0).floor() as u32 + 1;
        let beat = (beats % 4.0).floor() as u32 + 1;
        let six = ((beats * 4.0) % 4.0).floor() as u32 + 1;
        runs(
            &p,
            egui::pos2(x, cy),
            Align2::LEFT_CENTER,
            &[
                (&format!("{bar:03}"), mono_medium(13.0), INK),
                (" ", mono(13.0), INK3),
                (&beat.to_string(), mono(13.0), INK2),
                (" ", mono(13.0), INK3),
                (&six.to_string(), mono(13.0), INK3),
            ],
        );

        // View switch (centre).
        let names: Vec<&str> = caps
            .views
            .iter()
            .map(|v| match v {
                View::Play => "Play",
                View::Sequence => "Sequence",
                View::Perform => "Perform",
            })
            .collect();
        let n = names.len().max(1) as f32;
        let sw = if small { 94.0 * n } else { 113.0 * n };
        let seg = Rect::from_center_size(egui::pos2(rect.center().x, cy), egui::vec2(sw, 42.0));
        let hints: Vec<&str> = if small { Vec::new() } else { ["1", "2", "3"].into_iter().take(names.len()).collect() };
        let cur = caps.views.iter().position(|v| *v == self.view).unwrap_or(0);
        if let Some(i) = segmented(ui, seg, ui.id().with("views"), &names, &hints, cur, font_medium(13.0), true) {
            self.set_view(host, caps.views[i]);
        }

        // Right: tempo, key, Project, Export.
        let mut rx = rect.right() - if small { 14.0 } else { 18.0 };
        let ghost = |ui: &mut egui::Ui, rx: &mut f32, label: &str, icon: Icon, id: &str, active: bool| -> egui::Response {
            let w = if small { 40.0 } else { 26.0 + ui.painter().layout_no_wrap(label.into(), font(13.0), INK).size().x + 14.0 };
            let r = Rect::from_min_size(egui::pos2(*rx - w, cy - 18.0), egui::vec2(w, 36.0));
            *rx -= w + 6.0;
            let resp = chip(ui, r, ui.id().with(id), label, !active, false);
            let p = ui.painter();
            let fg = if active { INK } else { INK2 };
            if small {
                draw_icon(p, icon, r.center(), 16.0, fg);
            } else {
                draw_icon(p, icon, egui::pos2(r.left() + 16.0, r.center().y), 15.0, fg);
                p.text(egui::pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0), fg);
            }
            resp
        };
        if caps.export && ghost(ui, &mut rx, "Export", Icon::Export, "export", false).clicked() {
            host.export();
        }
        let proj_open = self.sheet == Some(Sheet::Project);
        if ghost(ui, &mut rx, "Project", Icon::Grid, "project", proj_open).clicked() {
            self.sheet = if proj_open { None } else { Some(Sheet::Project) };
        }
        rx -= 6.0;
        p.line_segment([egui::pos2(rx, cy - 11.0), egui::pos2(rx, cy + 11.0)], hair(LINE2));
        rx -= 12.0;
        // Key chip: tap cycles the root, vertical drag the scale.
        let key = self.key;
        let kl = scale_label(key.scale);
        let kw = 24.0 + p.layout_no_wrap(kl.into(), font(11.0), INK).size().x + 18.0;
        let kr = Rect::from_min_size(egui::pos2(rx - kw, cy - 18.0), egui::vec2(kw, 36.0));
        let kresp = chip(ui, kr, ui.id().with("key"), "global key", false, false);
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
        runs(
            &p,
            egui::pos2(kr.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            &[(NOTE_NAMES[key.root as usize % 12], mono_medium(13.0), INK), ("  ", font(11.0), INK3), (kl, font(11.0), INK3)],
        );
        rx = kr.left() - 6.0;
        // Tempo chip: vertical drag (read-only when the host owns tempo).
        let bpm = host.project().lock().unwrap().bpm;
        let tr = Rect::from_min_size(egui::pos2(rx - 80.0, cy - 18.0), egui::vec2(80.0, 36.0));
        if caps.tempo {
            let tresp = chip(ui, tr, ui.id().with("tempo"), "tempo", false, false);
            if tresp.dragged() {
                let nb = (bpm - tresp.drag_delta().y as f64 * 0.25).clamp(20.0, 300.0);
                host.project().lock().unwrap().bpm = nb;
                host.mark();
            }
        } else {
            raised(&p, tr, 10, G1);
        }
        runs(
            &p,
            egui::pos2(tr.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            &[(&format!("{:.0}", bpm), mono_medium(13.0), INK), ("  ", font(11.0), INK3), ("bpm", font(11.0), INK3)],
        );
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
        p.image(tex.id(), screen, uv, Color32::from_white_alpha(7));
    }

    pub fn atlas(&mut self, host: &dyn Host, ctx: &egui::Context, src: usize) -> Option<(egui::TextureId, atlas::AtlasView)> {
        let p = host.project().lock().unwrap();
        let a = self.atlases.get(ctx, &p.sources, src)?;
        Some((a.tex.id(), atlas::AtlasView::of(a)))
    }

    /// "uploader · 2014-03-02 · 212 views · CC" for dug / linked media.
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

    /// The live canvas (GPU), rounded.
    pub fn stage(&mut self, host: &dyn Host, ui: &mut egui::Ui, rect: Rect, radius: f32) {
        let p = ui.painter().clone();
        p.rect_filled(rect, r(radius as u8), G1);
        if let Some(scene) = host.canvas() {
            p.add(crate::gpu::PreviewCallback::shape(scene, rect));
        }
        mask_corners(&p, rect, radius, G0);
        p.rect_stroke(rect, r(radius as u8), hair(LINE), egui::StrokeKind::Inside);
    }

    /// The small `stack | tile` switch in a stage's top-left corner.
    pub fn layout_switch(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, stage: Rect) {
        self.layout_switch_named(
            host,
            ui,
            egui::pos2(stage.left() + 10.0, stage.top() + 10.0),
            "layout",
            ("layout stack", "layout tile"),
        );
    }

    /// `stack | tile` switch at `pos` with explicit id salt and accessibility names.
    pub fn layout_switch_named(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        pos: egui::Pos2,
        id_salt: &str,
        names: (&str, &str),
    ) {
        let Some(tile) = host.tile_layout() else { return };
        let r0 = Rect::from_min_size(pos, egui::vec2(118.0, 34.0));
        let p = ui.painter().clone();
        p.rect_filled(r0, r(10), Color32::from_black_alpha(165));
        p.rect_stroke(r0, r(10), hair(LINE2), egui::StrokeKind::Inside);
        let half = r0.width() / 2.0;
        let a11y = [names.0, names.1];
        for (k, name) in ["stack", "tile"].into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(r0.left() + k as f32 * half, r0.top()), egui::vec2(half, r0.height())).shrink(3.0);
            let on = tile == (k == 1);
            if on {
                p.rect_filled(rc, r(7), HI);
            }
            let col = if on { ON_HI } else { INK2 };
            p.text(rc.center(), Align2::CENTER_CENTER, name, font_medium(11.5), col);
            let hr = Rect::from_min_max(egui::pos2(rc.left(), rc.top() - 4.0), egui::pos2(rc.right(), rc.bottom() + 4.0));
            if hit(ui, hr, ui.id().with((id_salt, k)), egui::Sense::click(), a11y[k]).clicked() && !on {
                host.set_tile_layout(k == 1);
            }
        }
    }
}

/// SMEAR's echo + reverb and the LOOK, as an AV effect chain.
pub fn emitter_effects(map: &MacroMap, key: &Key) -> Vec<AvEffect> {
    let mut out = Vec::new();
    if map.echo_mix > 0.001 || map.reverb_mix > 0.001 {
        out.push(AvEffect::new(EffectKind::Delay { time: 0.33, feedback: map.echo_feedback, mix: map.echo_mix, shift_x: 0.02, shift_y: 0.0 }));
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
