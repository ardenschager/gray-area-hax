//! The play-first studio UI (design v001): three views — Play (Tempera
//! grid + emitters), Sequence (nanoloop lanes + scenes), Perform (pads +
//! output) — one Deep drawer, and a tiny top bar. The classic editor
//! stays behind `--classic`.
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

mod atlas;
mod deep;
mod perform;
mod play;
mod sequence;
pub mod theme;
#[cfg(test)]
mod tests;
mod widgets;

use crate::{App, AppMode};
use chromagrain_core::dsp::Rng;
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::instrument::{grain_step, Cloud, MacroMap, LOOKS};
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::video::{AvLink, VisualGrainStyle};
use eframe::egui::{self, Align2, Color32, Rect};
use theme::*;
use widgets::*;

pub use deep::{DeepTab, DeepTarget};

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
    fn index(self) -> usize {
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
    pub cloud: Cloud,
    pub link: AvLink,
    /// Held by a finger / the pointer: spraying.
    pub held: bool,
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
            cloud: Cloud::default(),
            link: AvLink::default(),
            held: false,
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
    pub key: Key,
    pub seq: SeqState,
    pub deep: Option<(DeepTarget, DeepTab)>,
    pub rec: Option<Rec>,
    pub hud_until: f64,
    atlases: atlas::Atlases,
    grain_tex: Option<egui::TextureHandle>,
    /// Grid cells lit by sprays: (col, row) -> time.
    lit: Vec<((usize, usize), f64)>,
    pub(crate) seeded_pads: bool,
    pub(crate) bar_memo: Option<f64>,
    pub(crate) hold_rect: Option<Rect>,
    /// Fingers on the Play grid: (touch id, emitter).
    pub(crate) touches: Vec<(u64, usize)>,
    fonts_installed: bool,
    /// Fonts set via `set_fonts` apply from the next frame on.
    fonts_ready: bool,
    /// Bumped when the studio's sequence model changes.
    pub(crate) rev: u64,
}

impl Default for Studio {
    fn default() -> Self {
        Studio {
            view: View::Play,
            columns: [None; COLUMNS],
            emitters: [Emitter::new(0, 0, 6.5), Emitter::new(1, 2, 5.5), Emitter::new(2, 5, 1.5), Emitter::new(3, 4, 6.4)],
            sel_emitter: 0,
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
            atlases: atlas::Atlases::default(),
            grain_tex: None,
            lit: Vec::new(),
            seeded_pads: false,
            bar_memo: None,
            hold_rect: None,
            touches: Vec::new(),
            fonts_installed: false,
            fonts_ready: false,
            rev: 1,
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

impl App {
    // ------------------------------------------------------------ model

    /// Fill empty grid columns with library sources (in order).
    fn studio_sync_columns(&mut self) {
        let n = self.project.lock().unwrap().sources.len();
        let st = &mut self.studio;
        for c in st.columns.iter_mut() {
            if c.is_some_and(|s| s >= n) {
                *c = None;
            }
        }
        for s in 0..n {
            if st.columns.contains(&Some(s)) {
                continue;
            }
            // Derived sources (cut-outs, takes, stretches) don't take a column.
            if let Some(slot) = st.columns.iter_mut().take(COLUMNS - 1).find(|c| c.is_none()) {
                *slot = Some(s);
            }
        }
        for (i, lane) in st.seq.lanes.iter_mut().enumerate() {
            if lane.is_none_or(|s| s >= n) {
                *lane = st.columns[i];
            }
        }
    }

    /// Per-frame engine work: emitters, buses, rec, sequence arrangement.
    pub(crate) fn studio_update(&mut self, ctx: &egui::Context) {
        if !self.studio.fonts_installed {
            self.studio_install_theme(ctx);
            ctx.request_repaint();
        } else {
            self.studio.fonts_ready = true;
        }
        self.studio_sync_columns();
        self.seq_ensure();
        self.seq_build_arrangement();
        self.seq_tick();
        self.studio_fire_emitters(ctx);
        let now = self.app_clock.elapsed().as_secs_f64();
        self.studio.lit.retain(|(_, t)| now - *t < 0.4);
    }

    /// The four emitter buses: grain style + SMEAR + LOOK, sound and picture.
    pub(crate) fn studio_buses(&self) -> Vec<crate::gpu::LiveBus> {
        self.studio
            .emitters
            .iter()
            .map(|e| {
                let map = MacroMap::resolve(&e.macros, Some(&self.studio.key));
                crate::gpu::LiveBus {
                    // Sample end: the footage itself (over, not glow); the
                    // cloud end glows like classic grain clouds.
                    style: VisualGrainStyle::morph(0.0, &VisualGrainStyle { additive: 0.3, ..Default::default() }, map.texture),
                    effects: emitter_effects(&map, &self.studio.key),
                    link: e.link,
                }
            })
            .collect()
    }

    /// What the canvas shows under the grains in the Play view: the
    /// selected emitter's tile of footage, playing.
    pub(crate) fn studio_backdrop(&self) -> Option<(usize, f64)> {
        if self.classic {
            return None;
        }
        if self.studio.view == View::Sequence {
            return self.seq_last_hit();
        }
        if self.studio.view != View::Play {
            return None;
        }
        let e = &self.studio.emitters[self.studio.sel_emitter];
        let src = self.studio.columns.get(e.col).copied().flatten()?;
        let len = self.plan_project.sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
        if len <= 0.0 {
            return None;
        }
        let seg = len / ROWS as f64;
        let start = (e.row.floor() as f64 / ROWS as f64) * len;
        Some((src, start + self.app_clock.elapsed().as_secs_f64() % seg))
    }

    fn studio_fire_emitters(&mut self, ctx: &egui::Context) {
        let now = self.app_clock.elapsed().as_secs_f64();
        let key = self.studio.key;
        let sources: Vec<f64> = self.project.lock().unwrap().sources.iter().map(|s| s.duration()).collect();
        // Bus effects follow the knobs.
        let buses = self.studio_buses();
        for (i, b) in buses.iter().enumerate() {
            self.playback.set_bus(i as u8 + 1, b.effects.clone());
        }
        let mut fired: Vec<(usize, u8, GrainEvent, (usize, usize))> = Vec::new();
        for (i, e) in self.studio.emitters.iter_mut().enumerate() {
            let Some(src) = self.studio.columns.get(e.col).copied().flatten() else { continue };
            let len = sources.get(src).copied().unwrap_or(0.0);
            if !e.held || len <= 0.0 {
                e.next_t = now;
                e.head = e.pos() as f64 * len;
                continue;
            }
            let map = MacroMap::resolve(&e.macros, Some(&key));
            let mut n = 0;
            while e.next_t <= now && n < 64 {
                let id = ((i as u64 + 1) << 48) | e.k;
                let (mut ev, hop) = grain_step(&map, &e.cloud, Some(&key), &mut e.rng, e.head, len, 60, 0.9, id);
                ev.onset = (e.next_t + ev.onset).max(now - 0.02);
                e.head += hop * map.speed as f64;
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
        for (src, bus, ev, cell) in fired {
            let mut audio_ev = ev.clone();
            audio_ev.onset = 0.0; // the engine stamps its own onset
            self.playback.trigger_bus(bus, src, audio_ev);
            if let Some(rec) = self.studio.rec.as_mut() {
                let mut r = ev.clone();
                r.onset -= rec.t0;
                if r.onset >= 0.0 {
                    rec.events.push((src, r));
                }
            }
            self.studio.lit.push((cell, now));
            self.live_events.push((src, bus, ev));
        }
        if self.studio.emitters.iter().any(|e| e.held) || self.studio.rec.is_some() {
            ctx.request_repaint();
        }
    }

    pub(crate) fn studio_toggle_rec(&mut self) {
        let now = self.app_clock.elapsed().as_secs_f64();
        match self.studio.rec.take() {
            None => {
                self.studio.rec = Some(Rec { t0: now, beat0: self.playhead_beats, events: Vec::new() });
                self.status = "recording — what you play becomes a take".into();
            }
            Some(rec) => {
                let len = now - rec.t0 + 0.5;
                if rec.events.is_empty() {
                    self.status = "nothing played — no take".into();
                    return;
                }
                let (sources, w, h, fps, sr) = {
                    let p = self.project.lock().unwrap();
                    (p.sources.clone(), p.width, p.height, p.fps, p.sample_rate)
                };
                let n = sources.len();
                let tx = self.tx.clone();
                let beat = rec.beat0;
                self.busy = Some("bouncing the take…".into());
                std::thread::spawn(move || {
                    let src = chromagrain_core::take::bounce(
                        &format!("take {}", n + 1),
                        &sources,
                        &rec.events,
                        len,
                        w,
                        h,
                        fps,
                        sr,
                        &VisualGrainStyle::default(),
                    );
                    let _ = tx.send(crate::WorkerMsg::Take(src, beat));
                });
            }
        }
    }

    /// A bounced take: into the library and onto the "takes" track.
    pub(crate) fn studio_place_take(&mut self, src: chromagrain_core::timeline::Source, beat: f64) {
        let name = src.name.clone();
        let target = if self.mode == AppMode::Arrange { None } else { Some(()) };
        let id = self.project.lock().unwrap().add_source(src);
        if target.is_none() {
            let mut p = self.project.lock().unwrap();
            let ti = match p.tracks.iter().position(|t| t.name == "takes") {
                Some(t) => t,
                None => p.add_track("takes"),
            };
            let clip = crate::build_clip(&mut p, id, beat.max(0.0), crate::PlayMode::Snippet);
            p.tracks[ti].clips.push(clip);
        }
        self.status = format!("'{name}' recorded — it's on the takes track and in the library");
        self.mark_now();
    }

    // ------------------------------------------------------------ ui

    /// Fonts + visuals (call from the creation context so frame 1 has them).
    pub(crate) fn studio_install_theme(&mut self, ctx: &egui::Context) {
        theme::install(ctx);
        self.studio.fonts_installed = true;
    }

    pub(crate) fn studio_ui(&mut self, ctx: &egui::Context) {
        if !self.studio.fonts_ready {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(G0)).show(ctx, |_| {});
            return;
        }
        self.studio_shortcuts(ctx);
        if self.fullscreen {
            self.perform_fullscreen_ui(ctx);
            self.grain_overlay(ctx);
            return;
        }
        let w = ctx.screen_rect().width();
        let bar_h = if w < 1100.0 { 52.0 } else { 56.0 };
        egui::TopBottomPanel::top("studio_top")
            .exact_height(bar_h)
            .frame(egui::Frame::new().fill(G0))
            .show_separator_line(false)
            .show(ctx, |ui| self.top_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(G0)).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment([full.left_top(), full.right_top()], hair(LINE));
            let pad = if full.width() < 1100.0 { 14.0 } else { 20.0 };
            let area = full.shrink(pad);
            match self.studio.view {
                View::Play => self.play_view(ui, area),
                View::Sequence => self.sequence_view(ui, area),
                View::Perform => self.perform_view(ui, area),
            }
        });
        self.deep_drawer(ctx);
        self.grain_overlay(ctx);
    }

    pub(crate) fn studio_set_view(&mut self, v: View) {
        if v == self.studio.view {
            return;
        }
        self.studio.seq.hold = None;
        for e in self.studio.emitters.iter_mut() {
            e.held = false;
        }
        if v == View::Perform {
            self.studio_seed_pads();
            self.set_mode(AppMode::Perform);
        } else if self.mode == AppMode::Perform {
            self.set_mode(AppMode::Arrange);
        }
        self.studio.view = v;
    }

    /// First visit to Perform: one pad per clip (loop), then their clouds.
    fn studio_seed_pads(&mut self) {
        if self.studio.seeded_pads || self.mode != AppMode::Arrange || !self.other_tracks.is_empty() {
            self.studio.seeded_pads = true;
            return;
        }
        self.studio.seeded_pads = true;
        let cols: Vec<usize> = self.studio.columns.iter().flatten().copied().collect();
        if cols.is_empty() {
            return;
        }
        let mut p = self.project.lock().unwrap();
        let mut tracks = Vec::new();
        for i in 0..crate::PADS {
            let mut t = chromagrain_core::timeline::Track { name: format!("pad {}", i + 1), muted: true, ..Default::default() };
            if i < crate::PADS - 1 {
                let src = cols[i % cols.len()];
                let mode = match i / cols.len() {
                    0 => crate::PlayMode::Snippet,
                    _ => crate::PlayMode::Grains,
                };
                let mut c = crate::build_clip(&mut p, src, 0.0, mode);
                c.length_beats = crate::pow2_beats(c.length_beats).min(8.0);
                t.clips.push(c);
            }
            tracks.push(t);
        }
        drop(p);
        self.other_tracks = tracks;
    }

    fn studio_shortcuts(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            self.import_path(path, None);
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let (space, k1, k2, k3, tab, f, rk, esc, o) = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt;
            (
                i.key_pressed(egui::Key::Space),
                plain && i.key_pressed(egui::Key::Num1) && self.studio.view != View::Perform,
                plain && i.key_pressed(egui::Key::Num2) && self.studio.view != View::Perform,
                plain && i.key_pressed(egui::Key::Num3) && self.studio.view != View::Perform,
                i.key_pressed(egui::Key::Tab) && !i.modifiers.any(),
                plain && i.key_pressed(egui::Key::F) && (self.studio.view != View::Perform || i.modifiers.shift || self.fullscreen),
                plain && i.key_pressed(egui::Key::R) && self.studio.view != View::Perform,
                i.key_pressed(egui::Key::Escape),
                plain && i.key_pressed(egui::Key::O),
            )
        });
        if space {
            self.toggle_play();
        }
        if k1 {
            self.studio_set_view(View::Play);
        }
        if k2 {
            self.studio_set_view(View::Sequence);
        }
        if k3 {
            self.studio_set_view(View::Perform);
        }
        if tab {
            let next = View::ALL[(self.studio.view.index() + 1) % 3];
            self.studio_set_view(next);
        }
        if f {
            self.toggle_fullscreen(ctx);
        }
        if rk {
            self.studio_toggle_rec();
        }
        if o {
            self.toggle_output();
        }
        if esc {
            if self.fullscreen {
                self.toggle_fullscreen(ctx);
            } else if self.studio.seq.hold.is_some() {
                self.studio.seq.hold = None;
            } else {
                self.studio.deep = None;
            }
        }
        if self.studio.view == View::Perform && !self.fullscreen {
            let hit: Vec<usize> = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt || i.modifiers.shift {
                    return Vec::new();
                }
                crate::ui_perform::PAD_KEYS.iter().enumerate().filter(|(_, (k, _))| i.key_pressed(*k)).map(|(n, _)| n).collect()
            });
            for pad in hit {
                self.toggle_pad(pad);
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
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
        // Transport.
        let playing = self.playback.is_playing();
        let b = Rect::from_min_size(egui::pos2(x, cy - 19.0), egui::vec2(44.0, 38.0));
        if chip(ui, b, ui.id().with("play"), if playing { "stop" } else { "play" }, false, playing).clicked() {
            self.toggle_play();
        }
        draw_icon(&p, if playing { Icon::Stop } else { Icon::Play }, b.center(), 16.0, if playing { ON_HI } else { INK });
        x = b.right() + 6.0;
        let b = Rect::from_min_size(egui::pos2(x, cy - 19.0), egui::vec2(44.0, 38.0));
        let recording = self.studio.rec.is_some();
        if chip(ui, b, ui.id().with("rec"), "record take", false, false).clicked() {
            self.studio_toggle_rec();
        }
        if recording {
            p.circle_filled(b.center(), 6.0, REC);
        } else {
            p.circle_stroke(b.center(), 6.0, egui::Stroke::new(1.5, INK2));
        }
        x = b.right() + 16.0;
        let beats = self.playhead_beats.max(0.0);
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
        let sw = if small { 280.0 } else { 340.0 };
        let seg = Rect::from_center_size(egui::pos2(rect.center().x, cy), egui::vec2(sw, 42.0));
        let hints: &[&str] = if small { &[] } else { &["1", "2", "3"] };
        if let Some(i) = segmented(ui, seg, ui.id().with("views"), &["Play", "Sequence", "Perform"], hints, self.studio.view.index(), font_medium(13.0), true) {
            self.studio_set_view(View::ALL[i]);
        }

        // Right: tempo, key, Deep, Export.
        let mut rx = rect.right() - if small { 14.0 } else { 18.0 };
        let ghost = |ui: &mut egui::Ui, rx: &mut f32, label: &str, icon: Icon, id: &str| -> egui::Response {
            let w = if small { 40.0 } else { 26.0 + ui.painter().layout_no_wrap(label.into(), font(13.0), INK).size().x + 14.0 };
            let r = Rect::from_min_size(egui::pos2(*rx - w, cy - 18.0), egui::vec2(w, 36.0));
            *rx -= w + 6.0;
            let resp = chip(ui, r, ui.id().with(id), label, true, false);
            let p = ui.painter();
            if small {
                draw_icon(p, icon, r.center(), 16.0, INK2);
            } else {
                draw_icon(p, icon, egui::pos2(r.left() + 16.0, r.center().y), 15.0, INK2);
                p.text(egui::pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0), INK2);
            }
            resp
        };
        if ghost(ui, &mut rx, "Export", Icon::Export, "export").clicked() {
            self.start_render(false);
        }
        if ghost(ui, &mut rx, "Deep", Icon::Deep, "deep").clicked() {
            self.studio.deep = match self.studio.deep {
                Some(_) => None,
                None => Some((DeepTarget::Global, DeepTab::Links)),
            };
        }
        rx -= 6.0;
        p.line_segment([egui::pos2(rx, cy - 11.0), egui::pos2(rx, cy + 11.0)], hair(LINE2));
        rx -= 12.0;
        // Key chip: tap cycles the root, hold-free.
        let key = self.studio.key;
        let kl = scale_label(key.scale);
        let kw = 24.0 + p.layout_no_wrap(kl.into(), font(11.0), INK).size().x + 18.0;
        let kr = Rect::from_min_size(egui::pos2(rx - kw, cy - 18.0), egui::vec2(kw, 36.0));
        let kresp = chip(ui, kr, ui.id().with("key"), "global key", false, false);
        if kresp.clicked() {
            self.studio.key = Key::new(key.root + 1, key.scale);
            self.studio_key_changed();
        }
        if kresp.dragged() && kresp.drag_delta().y.abs() > 0.0 {
            let d = ui.data(|d| d.get_temp::<f32>(kresp.id).unwrap_or(0.0)) - kresp.drag_delta().y;
            if d.abs() > 18.0 {
                let all = [ScaleKind::Minor, ScaleKind::Major, ScaleKind::MinorPentatonic, ScaleKind::MajorPentatonic, ScaleKind::Dorian, ScaleKind::Blues];
                let i = all.iter().position(|s| *s == key.scale).unwrap_or(0) as i32 + d.signum() as i32;
                self.studio.key = Key::new(key.root, all[i.rem_euclid(all.len() as i32) as usize]);
                self.studio_key_changed();
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
        // Tempo chip: vertical drag.
        let bpm = self.project.lock().unwrap().bpm;
        let tr = Rect::from_min_size(egui::pos2(rx - 80.0, cy - 18.0), egui::vec2(80.0, 36.0));
        let tresp = chip(ui, tr, ui.id().with("tempo"), "tempo", false, false);
        if tresp.dragged() {
            let nb = (bpm - tresp.drag_delta().y as f64 * 0.25).clamp(20.0, 300.0);
            self.project.lock().unwrap().bpm = nb;
            self.mark();
        }
        runs(
            &p,
            egui::pos2(tr.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            &[(&format!("{:.0}", bpm), mono_medium(13.0), INK), ("  ", font(11.0), INK3), ("bpm", font(11.0), INK3)],
        );
    }

    fn studio_key_changed(&mut self) {
        let k = self.studio.key;
        let mut p = self.project.lock().unwrap();
        for pid in self.studio.seq.scene_pats.iter().flatten() {
            if let Some(pat) = p.patterns.get_mut(*pid) {
                for r in &mut pat.rows {
                    r.key = Some(k);
                }
            }
        }
        drop(p);
        self.mark();
    }

    /// 2.8 % film grain over the whole window (one tiled noise texture).
    fn grain_overlay(&mut self, ctx: &egui::Context) {
        let tex = self.studio.grain_tex.get_or_insert_with(|| {
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

    pub(crate) fn atlas(&mut self, ctx: &egui::Context, src: usize) -> Option<(egui::TextureId, atlas::AtlasView)> {
        let sources = &self.plan_project.sources;
        let a = self.studio.atlases.get(ctx, sources, src)?;
        Some((a.tex.id(), atlas::AtlasView::of(a)))
    }

    pub(crate) fn source_name(&self, src: usize) -> String {
        self.plan_project.sources.get(src).map(|s| s.name.clone()).unwrap_or_default()
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
