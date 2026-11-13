//! The unified calm `Clip` sheet (`D · Hybrid: "touch it · open it"`):
//! - Header: `< Play` / `< Sequence` / `< Perform` back pill, clip title +
//!   provenance, Owner Switcher (`Clip · Emitter N · Lane N · Pad N`),
//!   `Replace` and `Dig`.
//! - Top band: live clip stage preview (`stack | tile`) + 6 owner macros
//!   (`GRAIN`, `PITCH`, `SMEAR`, `LOOK`, `LEVEL`, `PAN`).
//! - Middle band: Koala-style `CHOP` card (16-frame filmstrip, waveform,
//!   Auto · 4 · 8 · 16 · Tap, edge grips, double-tap delete, `Slices → pads`).
//! - Bottom band: 5 parameter cards (`SHAPE · SOUND + PICTURE`, `GRAIN`,
//!   `PITCH`, `EFFECTS ♪ ▣`, `LINKS`) wired to the active owner.

use super::add::modal;
use super::deep::{get_chain_fx, set_chain_fx, sheet_back_button, sheet_card, FX_SLOTS};
use super::{Sheet, Studio, COLUMNS, EMITTERS, LANES};
use crate::atlas;
use crate::envelope::adsr_editor;
use crate::host::{AddRoute, AddTarget, Host, Pad};
use crate::studio::play::short;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::chop::{self, Edge, Slice};
use chromagrain_core::dsp::{FilterKind, FilterSpec};
use chromagrain_core::envelope::{Adsr, PictureCut};
use chromagrain_core::instrument::{MacroMap, LOOKS};
use chromagrain_core::video::AvLink;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// What the clip sheet edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTarget {
    /// A source on its own (a just-added card or library item).
    Source(usize),
    /// The clip in grid column N.
    Column(usize),
    /// Play emitter `0..4`.
    Emitter(usize),
    /// Sequence lane `0..8`.
    Lane(usize),
    /// Perform pad `0..16`.
    Pad(usize),
}

/// Height the chop panel is laid out for when used standalone.
pub const CHOP_PANEL_H: f32 = 208.0;

/// Whose envelope and voice parameters the parameter cards edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvTarget {
    Pad(usize),
    Lane(usize),
    Emitter(usize),
}

impl ClipTarget {
    /// Explicit owner target when `self` names an emitter, pad or lane.
    pub fn env_target(self) -> Option<EnvTarget> {
        match self {
            ClipTarget::Emitter(i) => Some(EnvTarget::Emitter(i)),
            ClipTarget::Pad(i) => Some(EnvTarget::Pad(i)),
            ClipTarget::Lane(l) => Some(EnvTarget::Lane(l)),
            _ => None,
        }
    }
}

/// Live tap-chop: the clip plays from the start; each tap drops a chop
/// point straight into the source's slices.
#[derive(Debug, Clone)]
pub struct TapChop {
    pub source: usize,
    pub t0: f64,
    pub len: f64,
}

#[derive(Default)]
pub struct ChopState {
    pub tap: Option<TapChop>,
    /// The slice last auditioned (highlighted) and when.
    pub playing: Option<(usize, usize, f64)>,
    /// An edge being dragged: (slice, end? false = start).
    pub drag: Option<(usize, bool)>,
}

/// Snapshot of the 6 macros + Grain/Pitch sub-params for the active owner.
#[derive(Clone, Copy)]
struct OwnerParams {
    grain: f32,
    pitch_m: f32,
    smear: f32,
    look_m: f32,
    level: f32,
    pan_m: f32,
    density: f32,
    duration: f32,
    spray: f32,
    reverse: f32,
    fine_cents: f32,
    octave: i32,
    wobble: f32,
    spread: f32,
}

impl Studio {
    pub fn clip_source(&self, host: &mut dyn Host, t: ClipTarget) -> Option<usize> {
        match t {
            ClipTarget::Source(s) => Some(s),
            ClipTarget::Column(c) => self.columns.get(c).copied().flatten(),
            ClipTarget::Emitter(i) => self.emitters.get(i).and_then(|e| self.columns.get(e.col).copied().flatten()),
            ClipTarget::Lane(l) => self.seq.lanes.get(l).copied().flatten(),
            ClipTarget::Pad(i) => host.perform().and_then(|p| p.pads().get(i).and_then(|pd| pd.source)),
        }
    }

    pub fn effective_owner(&self, target: ClipTarget, src: usize) -> EnvTarget {
        match target {
            ClipTarget::Emitter(i) => EnvTarget::Emitter(i.min(EMITTERS - 1)),
            ClipTarget::Pad(i) => EnvTarget::Pad(i),
            ClipTarget::Lane(l) => EnvTarget::Lane(l.min(LANES - 1)),
            ClipTarget::Column(c) => {
                let em = (0..EMITTERS).find(|&i| self.emitters[i].col == c).unwrap_or(self.sel_emitter);
                EnvTarget::Emitter(em.min(EMITTERS - 1))
            }
            ClipTarget::Source(_) => {
                let em = (0..EMITTERS)
                    .find(|&i| self.columns.get(self.emitters[i].col).copied().flatten() == Some(src))
                    .unwrap_or(self.sel_emitter);
                EnvTarget::Emitter(em.min(EMITTERS - 1))
            }
        }
    }

    fn add_target_of(target: ClipTarget, emitter_col: usize) -> AddTarget {
        match target {
            ClipTarget::Column(c) => AddTarget::Column(c),
            ClipTarget::Emitter(_) => AddTarget::Column(emitter_col.min(COLUMNS - 1)),
            ClipTarget::Lane(l) => AddTarget::Lane(l),
            ClipTarget::Pad(i) => AddTarget::Pad(i),
            ClipTarget::Source(_) => AddTarget::Any,
        }
    }

    pub fn open_clip(&mut self, t: ClipTarget) {
        match t {
            ClipTarget::Emitter(i) if i < EMITTERS => self.sel_emitter = i,
            ClipTarget::Lane(l) if l < LANES => self.sel_lane = l,
            ClipTarget::Pad(p) if p < 16 => self.sel_pad = p,
            _ => {}
        }
        self.chop = ChopState::default();
        self.sheet = Some(Sheet::Clip(t));
    }

    pub fn slices(&self, host: &dyn Host, src: usize) -> Vec<Slice> {
        host.project().lock().unwrap().sources.get(src).map(|s| s.slices.clone()).unwrap_or_default()
    }

    /// Replace a source's slices (normalized: sorted, no overlaps).
    pub fn set_slices(&mut self, host: &mut dyn Host, src: usize, v: Vec<Slice>) {
        {
            let mut p = host.project().lock().unwrap();
            let Some(s) = p.sources.get_mut(src) else { return };
            let mut v = v;
            chop::normalize(&mut v, s.duration());
            s.slices = v;
        }
        host.slices_changed(src);
        host.mark_now();
    }

    /// Slices 1..N onto pads 1..N (16 at most).
    pub fn slices_to_pads(&mut self, host: &mut dyn Host, src: usize) -> usize {
        let v = self.slices(host, src);
        let Some(perf) = host.perform() else { return 0 };
        for (i, s) in v.iter().take(16).enumerate() {
            perf.load_pad_slice(src, s.start, s.end, i);
        }
        v.len().min(16)
    }

    fn collect_owners(&self, host: &mut dyn Host, src: usize, cur: ClipTarget, wide: bool, clean_name: &str) -> Vec<(ClipTarget, String, String, bool)> {
        let mut out = Vec::new();
        let clip_target = (0..COLUMNS)
            .find(|&c| self.columns[c] == Some(src))
            .map(ClipTarget::Column)
            .unwrap_or(ClipTarget::Source(src));
        let clip_on = matches!(cur, ClipTarget::Column(_) | ClipTarget::Source(_));
        out.push((clip_target, "Clip".into(), "owner Clip".into(), clip_on));

        for i in 0..EMITTERS {
            let on_src = self.columns.get(self.emitters[i].col).copied().flatten() == Some(src);
            if on_src || cur == ClipTarget::Emitter(i) {
                let active = cur == ClipTarget::Emitter(i);
                let label = if wide && active {
                    format!("Emitter {} · {}", i + 1, short(clean_name, 10))
                } else if wide {
                    format!("Emitter {}", i + 1)
                } else {
                    format!("Em {}", i + 1)
                };
                out.push((ClipTarget::Emitter(i), label, format!("owner Emitter {}", i + 1), active));
            }
        }
        for l in 0..LANES {
            if self.seq.lanes[l] == Some(src) || cur == ClipTarget::Lane(l) {
                let active = cur == ClipTarget::Lane(l);
                let label = if wide { format!("Lane {}", l + 1) } else { format!("Ln {}", l + 1) };
                out.push((ClipTarget::Lane(l), label, format!("owner Lane {}", l + 1), active));
            }
        }
        let pads: Vec<Pad> = host.perform().map(|pf| pf.pads()).unwrap_or_default();
        let mut pad_count = 0usize;
        for (pi, pd) in pads.iter().enumerate().take(16) {
            let active = cur == ClipTarget::Pad(pi);
            if pd.source == Some(src) || active {
                if pad_count < 3 || active {
                    let label = if wide { format!("Pad {}", pi + 1) } else { format!("Pd {}", pi + 1) };
                    out.push((ClipTarget::Pad(pi), label, format!("owner Pad {}", pi + 1), active));
                    pad_count += 1;
                }
            }
        }
        out
    }

    pub fn clip_sheet(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let Some(Sheet::Clip(target)) = self.sheet else { return };
        let Some(src) = self.clip_source(host, target) else {
            self.sheet = None;
            return;
        };
        let screen = ctx.screen_rect();
        let size = egui::vec2((screen.width() - 36.0).min(1260.0), (screen.height() - 40.0).min(860.0));
        let caps_cfg = host.caps();
        let owner = self.effective_owner(target, src);
        let mut close = false;
        let mut switch_target: Option<ClipTarget> = None;
        let mut add_action: Option<(AddRoute, AddTarget)> = None;

        let scrim = modal(ctx, "cg-clip", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(20.0, 16.0));
            let id = egui::Id::new("cg-clip");
            let p = ui.painter().clone();
            let wide_sheet = inner.width() >= 1040.0;

            let raw_name = self.source_name(host, src);
            let clean_name = super::strip_media_ext(&raw_name).to_string();
            let (len, has_audio) = {
                let pp = host.project().lock().unwrap();
                pp.sources.get(src).map(|s| (s.duration(), s.audio.is_some())).unwrap_or((0.0, true))
            };
            let slices = self.slices(host, src);
            let prov = self.provenance_line(host, src);
            let sub = if !prov.is_empty() {
                prov
            } else {
                format!(
                    "{} · {}:{:02}",
                    match slices.len() {
                        0 => "no slices yet".to_string(),
                        1 => "1 slice".to_string(),
                        n => format!("{n} slices"),
                    },
                    (len / 60.0) as u32,
                    len as u32 % 60,
                )
            };

            // --- Header Row ---
            let (back_r, clicked_back) = sheet_back_button(ui, egui::pos2(inner.left(), inner.top() + 2.0), self.view, id);
            if clicked_back {
                close = true;
            }

            // Right-hand Replace / Dig buttons.
            let mut rx = inner.right();
            let em_col = match owner {
                EnvTarget::Emitter(i) => self.emitters[i].col,
                _ => self.emitters[self.sel_emitter].col,
            };
            let add_tgt = Self::add_target_of(target, em_col);
            if caps_cfg.add.contains(&AddRoute::Dig) {
                let dr = Rect::from_min_size(egui::pos2(rx - 54.0, inner.top() + 3.0), egui::vec2(54.0, 34.0));
                rx = dr.left() - 8.0;
                if button_named(ui, dr, id.with("dig"), "Dig", "clip dig", false, None, None).clicked() {
                    add_action = Some((AddRoute::Dig, add_tgt));
                }
            }
            if !caps_cfg.add.is_empty() {
                let rr = Rect::from_min_size(egui::pos2(rx - 74.0, inner.top() + 3.0), egui::vec2(74.0, 34.0));
                rx = rr.left() - 10.0;
                if button_named(ui, rr, id.with("replace"), "Replace", "clip replace", false, None, None).clicked() {
                    add_action = Some((caps_cfg.add[0], add_tgt));
                }
            }

            // Middle: Owner Switcher pills.
            let owners = self.collect_owners(host, src, target, wide_sheet, &clean_name);
            let pill_widths: Vec<f32> = owners
                .iter()
                .map(|(_, l, _, _)| p.layout_no_wrap(l.clone(), font_medium(11.5), INK).size().x + 20.0)
                .collect();
            let total_owners_w: f32 = pill_widths.iter().sum::<f32>() + 8.0;
            let title_x = back_r.right() + 14.0;
            let min_title_w = if wide_sheet { 210.0 } else { 130.0 };
            if rx - total_owners_w >= title_x + min_title_w {
                let box_r = Rect::from_min_size(egui::pos2(rx - total_owners_w, inner.top() + 3.0), egui::vec2(total_owners_w, 34.0));
                p.rect_filled(box_r, r(10), G0);
                p.rect_stroke(box_r, r(10), hair(LINE2), StrokeKind::Inside);
                let mut ox = box_r.left() + 4.0;
                for ((ot, label, a11y, active), pw) in owners.iter().zip(pill_widths.iter()) {
                    let pr = Rect::from_min_size(egui::pos2(ox, box_r.top() + 3.0), egui::vec2(*pw, 28.0));
                    let resp = hit(ui, pr, id.with(("owner", &a11y)), Sense::click(), a11y);
                    if *active {
                        p.rect_filled(pr, r(7), G4);
                        p.rect_stroke(pr, r(7), hair(LINE2), StrokeKind::Inside);
                    } else if resp.hovered() {
                        p.rect_filled(pr, r(7), G2);
                    }
                    p.text(
                        pr.center(),
                        Align2::CENTER_CENTER,
                        label,
                        font_medium(11.5),
                        if *active || resp.hovered() { INK } else { INK2 },
                    );
                    if resp.clicked() && !*active {
                        switch_target = Some(*ot);
                    }
                    ox += *pw;
                }
                rx = box_r.left() - 12.0;
            }

            let max_title_chars = ((rx - title_x) / 8.5).max(8.0) as usize;
            let max_sub_chars = ((rx - title_x) / 6.5).max(12.0) as usize;
            p.text(
                egui::pos2(title_x, inner.top() + 11.0),
                Align2::LEFT_CENTER,
                short(&clean_name, max_title_chars),
                font_semibold(17.5),
                INK,
            );
            p.text(
                egui::pos2(title_x, inner.top() + 30.0),
                Align2::LEFT_CENTER,
                short(&sub, max_sub_chars),
                mono(11.0),
                INK3,
            );

            // --- Body (Scrollable when viewport height < 860, e.g. 960×600) ---
            let body = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + 48.0), inner.max);
            let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
            egui::ScrollArea::vertical()
                .id_salt("cg-clip-scroll")
                .auto_shrink([false, false])
                .show(&mut body_ui, |ui| {
                    let compact_h = body.height() < 755.0;
                    let needs_scroll = body.height() < 696.0;
                    let avail_w = if needs_scroll {
                        (body.width() - 8.0).max(320.0)
                    } else {
                        body.width().max(320.0)
                    };
                    let wide_cards = avail_w >= 1040.0;
                    let gap = if compact_h { 10.0 } else { 12.0 };
                    let top_h = if compact_h { 174.0 } else { 200.0 };
                    let chop_card_h = if compact_h { 214.0 } else { 244.0 };
                    let cards_row1_h = if compact_h { 276.0 } else { 292.0 };
                    let cards_row2_h = if wide_cards { 0.0 } else { 208.0 };
                    let total_h = top_h + gap + chop_card_h + gap + cards_row1_h + if wide_cards { 0.0 } else { gap + cards_row2_h } + 2.0;

                    let (canvas, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), Sense::hover());
                    let cp = ui.painter().clone();

                    // 1. Top Band: Stage Preview + 6 Macros
                    let stage_w = (top_h * 16.0 / 9.0).floor().min(avail_w * 0.35);
                    let stage_r = Rect::from_min_size(canvas.min, egui::vec2(stage_w, top_h));
                    self.clip_stage_preview(host, ui, stage_r, src, len, owner);
                    let macros_r = Rect::from_min_max(
                        egui::pos2(stage_r.right() + gap, canvas.top()),
                        egui::pos2(canvas.right(), canvas.top() + top_h),
                    );
                    self.clip_macros_card(host, ui, macros_r, owner);

                    // 2. Middle Band: CHOP Card
                    let chop_card_r = Rect::from_min_size(
                        egui::pos2(canvas.left(), stage_r.bottom() + gap),
                        egui::vec2(avail_w, chop_card_h),
                    );
                    let chop_inner = sheet_card(&cp, chop_card_r, "Chop");
                    if !has_audio {
                        cp.text(
                            egui::pos2(chop_card_r.right() - 16.0, chop_card_r.top() + 16.0),
                            Align2::RIGHT_CENTER,
                            "PICTURE",
                            mono(9.5),
                            INK3,
                        );
                    }
                    self.chop_panel(host, ui, chop_inner, src);
                    // Note: `Slices → pads` inside `chop_panel` closes `self.sheet`.
                    if self.sheet.is_none() {
                        return;
                    }

                    // 3. Bottom Band: 5 Parameter Cards (`SHAPE`, `GRAIN`, `PITCH`, `EFFECTS`, `LINKS`)
                    let cards_top = chop_card_r.bottom() + gap;
                    if wide_cards {
                        let w_total = avail_w - gap * 4.0;
                        let w_shape = (w_total * 0.225).floor();
                        let w_grain = (w_total * 0.155).floor();
                        let w_pitch = (w_total * 0.155).floor();
                        let w_fx = (w_total * 0.23).floor();
                        let w_links = w_total - w_shape - w_grain - w_pitch - w_fx;

                        let r_shape = Rect::from_min_size(egui::pos2(canvas.left(), cards_top), egui::vec2(w_shape, cards_row1_h));
                        let r_grain = Rect::from_min_size(egui::pos2(r_shape.right() + gap, cards_top), egui::vec2(w_grain, cards_row1_h));
                        let r_pitch = Rect::from_min_size(egui::pos2(r_grain.right() + gap, cards_top), egui::vec2(w_pitch, cards_row1_h));
                        let r_fx = Rect::from_min_size(egui::pos2(r_pitch.right() + gap, cards_top), egui::vec2(w_fx, cards_row1_h));
                        let r_links = Rect::from_min_size(egui::pos2(r_fx.right() + gap, cards_top), egui::vec2(w_links, cards_row1_h));

                        self.clip_shape_card(host, ui, r_shape, owner, src);
                        self.clip_grain_card(host, ui, r_grain, owner);
                        self.clip_pitch_card(host, ui, r_pitch, owner);
                        self.clip_effects_card(host, ui, r_fx, owner);
                        self.clip_links_card(host, ui, r_links, owner);
                    } else {
                        let w1 = avail_w - gap * 2.0;
                        let w_shape = (w1 * 0.40).floor();
                        let w_grain = (w1 * 0.30).floor();
                        let w_pitch = w1 - w_shape - w_grain;

                        let r_shape = Rect::from_min_size(egui::pos2(canvas.left(), cards_top), egui::vec2(w_shape, cards_row1_h));
                        let r_grain = Rect::from_min_size(egui::pos2(r_shape.right() + gap, cards_top), egui::vec2(w_grain, cards_row1_h));
                        let r_pitch = Rect::from_min_size(egui::pos2(r_grain.right() + gap, cards_top), egui::vec2(w_pitch, cards_row1_h));

                        self.clip_shape_card(host, ui, r_shape, owner, src);
                        self.clip_grain_card(host, ui, r_grain, owner);
                        self.clip_pitch_card(host, ui, r_pitch, owner);

                        let r2_top = r_shape.bottom() + gap;
                        let w2 = ((avail_w - gap) * 0.5).floor();
                        let r_fx = Rect::from_min_size(egui::pos2(canvas.left(), r2_top), egui::vec2(w2, cards_row2_h));
                        let r_links = Rect::from_min_size(egui::pos2(r_fx.right() + gap, r2_top), egui::vec2(avail_w - w2 - gap, cards_row2_h));

                        self.clip_effects_card(host, ui, r_fx, owner);
                        self.clip_links_card(host, ui, r_links, owner);
                    }
                });
        });

        if let Some((route, tgt)) = add_action {
            self.add.tab = route;
            self.open_add(host, tgt);
        } else if let Some(new_tgt) = switch_target {
            match new_tgt {
                ClipTarget::Emitter(i) if i < EMITTERS => self.sel_emitter = i,
                ClipTarget::Lane(l) if l < LANES => self.sel_lane = l,
                ClipTarget::Pad(p) if p < 16 => self.sel_pad = p,
                _ => {}
            }
            self.sheet = Some(Sheet::Clip(new_tgt));
        } else if (close || scrim) && matches!(self.sheet, Some(Sheet::Clip(_))) {
            if self.chop.tap.is_some() {
                self.finish_tap(host);
            }
            host.stop_audition();
            self.sheet = None;
        }
    }

    fn clip_stage_preview(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, stage: Rect, src: usize, len: f64, owner: EnvTarget) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let now = host.clock();
        let norm_pos = if let Some(tc) = self.chop.tap.as_ref().filter(|tc| tc.source == src) {
            ((now - tc.t0) / tc.len.max(1e-6)).clamp(0.0, 0.999) as f32
        } else if let Some((_, sl_i, t0)) = self.chop.playing.filter(|(s, _, _)| *s == src) {
            let slices = self.slices(host, src);
            slices
                .get(sl_i)
                .map(|sl| {
                    let at = (sl.start + (now - t0)).min(sl.end);
                    (at / len.max(1e-6)).clamp(0.0, 0.999) as f32
                })
                .unwrap_or(0.25)
        } else {
            match owner {
                EnvTarget::Emitter(i) => self.emitters[i].pos(),
                _ => 0.25,
            }
        };
        if let Some((tex, a)) = self.atlas(host, &ctx, src) {
            tile(&p, stage, 12, tex, a.uv_cover(atlas::frame_at(norm_pos), stage.width() / stage.height()), gray(0.95));
        } else {
            p.rect_filled(stage, r(12), G0);
        }
        p.rect_stroke(stage, r(12), hair(LINE2), StrokeKind::Inside);
        self.layout_switch_named(
            host,
            ui,
            egui::pos2(stage.left() + 10.0, stage.top() + 10.0),
            "clip-layout",
            ("clip layout stack", "clip layout tile"),
        );
    }

    fn read_owner_params(&self, host: &dyn Host, owner: EnvTarget) -> OwnerParams {
        match owner {
            EnvTarget::Emitter(i) => {
                let e = &self.emitters[i];
                OwnerParams {
                    grain: e.macros[0],
                    pitch_m: e.macros[1],
                    smear: e.macros[2],
                    look_m: e.macros[3],
                    level: e.level,
                    pan_m: ((e.pan + 1.0) * 0.5).clamp(0.0, 1.0),
                    density: e.cloud.density,
                    duration: e.cloud.duration,
                    spray: e.cloud.spray,
                    reverse: e.cloud.reverse,
                    fine_cents: e.fine_cents,
                    octave: e.octave,
                    wobble: e.cloud.pitch_jitter,
                    spread: e.cloud.pan_spread,
                }
            }
            EnvTarget::Lane(l) => {
                let (scene, _) = self.seq_current(host);
                let pid = self.seq_pattern(scene);
                let pp = host.project().lock().unwrap();
                let gs = pid
                    .and_then(|id| pp.patterns.get(id))
                    .and_then(|pat| pat.rows.get(l))
                    .map(|r| r.grains.clone())
                    .unwrap_or_default();
                let look_idx = self.lane_fx[l]
                    .iter()
                    .find_map(|f| LOOKS.iter().position(|lk| *lk == Some(f.kind.name())))
                    .unwrap_or(0);
                let oct = (gs.pitch / 12.0).round().clamp(-2.0, 2.0) as i32;
                let rem_st = gs.pitch - oct as f32 * 12.0;
                OwnerParams {
                    grain: gs.texture.clamp(0.0, 1.0),
                    pitch_m: ((rem_st + 12.0) / 24.0).clamp(0.0, 1.0),
                    smear: (1.0 - gs.scan_speed).clamp(0.0, 1.0),
                    look_m: (look_idx as f32 + 0.5) / LOOKS.len() as f32,
                    level: (gs.gain / 1.0).clamp(0.0, 1.0),
                    pan_m: ((gs.pan + 1.0) * 0.5).clamp(0.0, 1.0),
                    density: gs.density.clamp(2.0, 120.0),
                    duration: gs.duration.clamp(0.01, 0.6),
                    spray: gs.spray.clamp(0.0, 1.0),
                    reverse: gs.reverse_prob.clamp(0.0, 1.0),
                    fine_cents: gs.detune_cents.clamp(-50.0, 50.0),
                    octave: oct,
                    wobble: gs.pitch_jitter.clamp(0.0, 12.0),
                    spread: gs.pan_spread.clamp(0.0, 1.0),
                }
            }
            EnvTarget::Pad(pi) => {
                let pp = host.project().lock().unwrap();
                if let Some((track, clip)) = pp.tracks.get(pi).and_then(|t| t.clips.first().map(|c| (t, c))) {
                    let gs = &clip.grains;
                    let look_idx = track
                        .effects
                        .iter()
                        .find_map(|f| LOOKS.iter().position(|lk| *lk == Some(f.kind.name())))
                        .unwrap_or(0);
                    let smear = track
                        .effects
                        .iter()
                        .find_map(|f| match f.kind {
                            chromagrain_core::fx::EffectKind::Reverb { mix, .. } => Some((mix / 0.6).clamp(0.0, 1.0)),
                            _ => None,
                        })
                        .unwrap_or(0.0);
                    let oct = (gs.pitch / 12.0).round().clamp(-2.0, 2.0) as i32;
                    let rem_st = gs.pitch - oct as f32 * 12.0;
                    OwnerParams {
                        grain: gs.texture.clamp(0.0, 1.0),
                        pitch_m: ((rem_st + 12.0) / 24.0).clamp(0.0, 1.0),
                        smear,
                        look_m: (look_idx as f32 + 0.5) / LOOKS.len() as f32,
                        level: (track.level / 1.25).clamp(0.0, 1.0),
                        pan_m: ((gs.pan + 1.0) * 0.5).clamp(0.0, 1.0),
                        density: gs.density.clamp(2.0, 120.0),
                        duration: gs.duration.clamp(0.01, 0.6),
                        spray: gs.spray.clamp(0.0, 1.0),
                        reverse: gs.reverse_prob.clamp(0.0, 1.0),
                        fine_cents: gs.detune_cents.clamp(-50.0, 50.0),
                        octave: oct,
                        wobble: gs.pitch_jitter.clamp(0.0, 12.0),
                        spread: gs.pan_spread.clamp(0.0, 1.0),
                    }
                } else {
                    drop(pp);
                    let e = &self.emitters[self.sel_emitter];
                    let m = host.shared_macros().unwrap_or(e.macros);
                    OwnerParams {
                        grain: m[0],
                        pitch_m: m[1],
                        smear: m[2],
                        look_m: m[3],
                        level: e.level,
                        pan_m: ((e.pan + 1.0) * 0.5).clamp(0.0, 1.0),
                        density: e.cloud.density,
                        duration: e.cloud.duration,
                        spray: e.cloud.spray,
                        reverse: e.cloud.reverse,
                        fine_cents: e.fine_cents,
                        octave: e.octave,
                        wobble: e.cloud.pitch_jitter,
                        spread: e.cloud.pan_spread,
                    }
                }
            }
        }
    }

    fn write_owner_params(&mut self, host: &mut dyn Host, owner: EnvTarget, op: OwnerParams) {
        let key = self.key;
        match owner {
            EnvTarget::Emitter(i) => {
                let e = &mut self.emitters[i];
                let prev = e.macros;
                e.macros = [op.grain, op.pitch_m, op.smear, op.look_m];
                e.level = op.level;
                e.pan = (op.pan_m * 2.0 - 1.0).clamp(-1.0, 1.0);
                e.cloud.density = op.density;
                e.cloud.duration = op.duration;
                e.cloud.spray = op.spray;
                e.cloud.reverse = op.reverse;
                e.fine_cents = op.fine_cents;
                e.octave = op.octave;
                e.cloud.pitch_jitter = op.wobble;
                e.cloud.pan_spread = op.spread;
                for k in 0..4 {
                    if (e.macros[k] - prev[k]).abs() > 1e-5 {
                        host.set_shared_macro(k, e.macros[k]);
                    }
                }
            }
            EnvTarget::Lane(l) => {
                let map = MacroMap::resolve(&[op.grain, op.pitch_m, op.smear, op.look_m], Some(&key));
                {
                    let mut pp = host.project().lock().unwrap();
                    for pid in self.seq.scene_pats.iter().flatten() {
                        if let Some(r) = pp.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(l)) {
                            r.grains.texture = op.grain;
                            r.grains.pitch = map.pitch_st + op.octave as f32 * 12.0;
                            r.grains.scan_speed = (1.0 - op.smear).clamp(0.05, 1.0);
                            r.grains.gain = op.level;
                            r.grains.pan = (op.pan_m * 2.0 - 1.0).clamp(-1.0, 1.0);
                            r.grains.density = op.density;
                            r.grains.duration = op.duration;
                            r.grains.spray = op.spray;
                            r.grains.reverse_prob = op.reverse;
                            r.grains.detune_cents = op.fine_cents;
                            r.grains.pitch_jitter = op.wobble;
                            r.grains.pan_spread = op.spread;
                        }
                    }
                }
                self.lane_fx[l] = super::emitter_effects(&map, &key);
                host.mark();
            }
            EnvTarget::Pad(pi) => {
                let map = MacroMap::resolve(&[op.grain, op.pitch_m, op.smear, op.look_m], Some(&key));
                let has_track = {
                    let mut pp = host.project().lock().unwrap();
                    if let Some(t) = pp.tracks.get_mut(pi).filter(|t| !t.clips.is_empty()) {
                        t.level = op.level * 1.25;
                        for c in &mut t.clips {
                            c.grains.texture = op.grain;
                            c.grains.pitch = map.pitch_st + op.octave as f32 * 12.0;
                            c.grains.pan = (op.pan_m * 2.0 - 1.0).clamp(-1.0, 1.0);
                            c.grains.density = op.density;
                            c.grains.duration = op.duration;
                            c.grains.spray = op.spray;
                            c.grains.reverse_prob = op.reverse;
                            c.grains.detune_cents = op.fine_cents;
                            c.grains.pitch_jitter = op.wobble;
                            c.grains.pan_spread = op.spread;
                        }
                        t.effects = super::emitter_effects(&map, &key);
                        true
                    } else {
                        false
                    }
                };
                if has_track {
                    host.mark();
                } else {
                    for (k, v) in [op.grain, op.pitch_m, op.smear, op.look_m].into_iter().enumerate() {
                        host.set_shared_macro(k, v);
                    }
                }
            }
        }
    }

    fn clip_macros_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        p.rect_filled(rect, r(12), Color32::from_rgb(0x10, 0x10, 0x12));
        p.rect_stroke(rect, r(12), hair(LINE), StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(16.0, 8.0));
        let col_w = inner.width() / 3.0;
        let row_h = inner.height() / 2.0;
        let rad = (row_h * 0.24).clamp(17.5, 22.5);
        let id = ui.id().with(("clip-macros", owner));

        let mut op = self.read_owner_params(host, owner);
        let before = (op.grain, op.pitch_m, op.smear, op.look_m, op.level, op.pan_m);

        let slots: [(&str, &str, f32, usize); 6] = [
            ("Grain", "clip macro Grain", 0.0, 0),
            ("Pitch", "clip macro Pitch", 0.5, 0),
            ("Smear", "clip macro Smear", 0.0, 0),
            ("Look", "clip macro Look", 0.0, LOOKS.len()),
            ("Level", "clip macro Level", 0.8, 0),
            ("Pan", "clip macro Pan", 0.5, 0),
        ];

        let key = self.key;
        for (k, &(name, a11y, def, detents)) in slots.iter().enumerate() {
            let cx = inner.left() + ((k % 3) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 3) as f32 * row_h + rad + 6.0;
            let c = egui::pos2(cx, cy);
            let val_ref = match k {
                0 => &mut op.grain,
                1 => &mut op.pitch_m,
                2 => &mut op.smear,
                3 => &mut op.look_m,
                4 => &mut op.level,
                _ => &mut op.pan_m,
            };
            knob(ui, c, rad, id.with(k), a11y, val_ref, def, detents);
            let map = MacroMap::resolve(&[op.grain, op.pitch_m, op.smear, op.look_m], Some(&key));
            let shown = match k {
                0 => format!("{:.0}", op.grain * 100.0),
                1 => format!("{:+.0} {}", map.pitch_st, super::note_name(key.root + map.pitch_st as i32)).replace("+0 ", "0 "),
                2 => format!("{:.0}", op.smear * 100.0),
                3 => super::look_name(map.look).to_string(),
                4 => format!("{:.0}%", op.level * 100.0),
                _ => {
                    let pan = op.pan_m * 2.0 - 1.0;
                    if pan.abs() < 0.03 {
                        "C".into()
                    } else if pan < 0.0 {
                        format!("L{:.0}", pan.abs() * 100.0)
                    } else {
                        format!("R{:.0}", pan * 100.0)
                    }
                }
            };
            caps(&p, egui::pos2(cx, cy + rad + 10.0), Align2::CENTER_CENTER, name, 9.5);
            p.text(egui::pos2(cx, cy + rad + 23.0), Align2::CENTER_CENTER, shown, mono(11.0), INK);
        }

        if (op.grain, op.pitch_m, op.smear, op.look_m, op.level, op.pan_m) != before {
            self.write_owner_params(host, owner, op);
        }
    }

    fn clip_shape_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget, src: usize) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Shape · sound + picture");
        let Some(mut adsr) = self.target_env(host, owner) else { return };

        let env_h = (inner.height() - 102.0).clamp(84.0, 140.0);
        let er = Rect::from_min_size(inner.min, egui::vec2(inner.width(), env_h));
        let frame = match owner {
            EnvTarget::Emitter(i) => atlas::frame_at(self.emitters[i].pos()),
            _ => 5,
        };
        let thumb = self
            .atlas(host, &ctx, src)
            .map(|(tex, _)| (tex, atlas::uv_cover(frame, er.width() / er.height().max(1.0))));
        let mut changed = adsr_editor(ui, er, ui.id().with(("env", owner)), &mut adsr, thumb);

        // Bottom row: `cut | fade` segmented toggle + `CUT AT` threshold knob.
        let bot = Rect::from_min_max(egui::pos2(inner.left(), er.bottom() + 10.0), inner.max);
        let seg_w = (bot.width() * 0.56).clamp(92.0, 128.0);
        let seg_r = Rect::from_min_size(egui::pos2(bot.left(), bot.center().y - 18.0), egui::vec2(seg_w, 36.0));
        let fade = adsr.picture == PictureCut::Fade;
        if let Some(pick) = segmented_named(
            ui,
            seg_r,
            ui.id().with(("cutfade", owner)),
            &["cut", "fade"],
            &["picture cut", "picture fade"],
            &[],
            usize::from(fade),
            font(12.0),
            false,
        ) {
            adsr.picture = if pick == 1 { PictureCut::Fade } else { PictureCut::Cut };
            changed = true;
        }

        let kc = egui::pos2(seg_r.right() + (bot.right() - seg_r.right()) * 0.5, bot.center().y - 10.0);
        let mut tv = ((adsr.cut_threshold - 0.01) / 0.49).clamp(0.0, 1.0);
        let before_tv = tv;
        knob(ui, kc, 18.0, ui.id().with(("cut-at", owner)), "cut threshold", &mut tv, (0.08 - 0.01) / 0.49, 0);
        if (tv - before_tv).abs() > 1e-4 {
            adsr.cut_threshold = 0.01 + tv * 0.49;
            changed = true;
        }
        caps(&p, egui::pos2(kc.x, kc.y + 28.0), Align2::CENTER_CENTER, "Cut at", 9.0);
        p.text(
            egui::pos2(kc.x, kc.y + 41.0),
            Align2::CENTER_CENTER,
            format!("{:.0}%", adsr.cut_threshold * 100.0),
            mono(10.5),
            INK,
        );

        if changed {
            self.set_target_env(host, owner, adsr);
        }
        if let EnvTarget::Pad(i) = owner {
            let released_in_env = ui.input(|inp| {
                inp.pointer.button_released(egui::PointerButton::Primary)
                    && inp.pointer.latest_pos().is_some_and(|pt| er.expand(24.0).contains(pt))
            });
            if released_in_env {
                if let Some(pf) = host.perform() {
                    pf.toggle_pad(i);
                }
            }
        }
    }

    fn clip_grain_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Grain");
        let id = ui.id().with(("clip-grain", owner));
        let mut op = self.read_owner_params(host, owner);
        let before = (op.density, op.duration, op.spray, op.reverse);

        let col_w = inner.width() * 0.5;
        let row_h = inner.height() * 0.5;
        let rad = 18.5;

        let mut n_dens = ((op.density - 2.0) / 118.0).clamp(0.0, 1.0);
        let mut n_dur = ((op.duration - 0.01) / 0.39).clamp(0.0, 1.0);
        let mut n_spray = op.spray.clamp(0.0, 1.0);
        let mut n_rev = op.reverse.clamp(0.0, 1.0);

        let items: [(&str, &str, f32); 4] = [
            ("Density", "grain density", (30.0 - 2.0) / 118.0),
            ("Length", "grain length", (0.08 - 0.01) / 0.39),
            ("Spray", "grain spray", 0.3),
            ("Reverse", "grain reverse", 0.15),
        ];

        for (k, &(_name, a11y, def)) in items.iter().enumerate() {
            let cx = inner.left() + ((k % 2) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 2) as f32 * row_h + 28.0;
            let c = egui::pos2(cx, cy);
            let vref = match k {
                0 => &mut n_dens,
                1 => &mut n_dur,
                2 => &mut n_spray,
                _ => &mut n_rev,
            };
            knob(ui, c, rad, id.with(k), a11y, vref, def, 0);
        }

        op.density = 2.0 + n_dens * 118.0;
        op.duration = 0.01 + n_dur * 0.39;
        op.spray = n_spray;
        op.reverse = n_rev;

        let texts = [
            format!("{:.0}/s", op.density),
            format!("{:.0}ms", op.duration * 1000.0),
            format!("{:.0}%", op.spray * 100.0),
            format!("{:.0}%", op.reverse * 100.0),
        ];
        for (k, (&(name, _, _), val_s)) in items.iter().zip(texts.iter()).enumerate() {
            let cx = inner.left() + ((k % 2) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 2) as f32 * row_h + 28.0;
            caps(&p, egui::pos2(cx, cy + rad + 11.0), Align2::CENTER_CENTER, name, 9.0);
            p.text(egui::pos2(cx, cy + rad + 24.0), Align2::CENTER_CENTER, val_s, mono(10.5), INK);
        }

        if (op.density, op.duration, op.spray, op.reverse) != before {
            self.write_owner_params(host, owner, op);
        }
    }

    fn clip_pitch_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Pitch");
        let id = ui.id().with(("clip-pitch", owner));
        let mut op = self.read_owner_params(host, owner);
        let before = (op.fine_cents, op.octave, op.wobble, op.spread);

        let col_w = inner.width() * 0.5;
        let row_h = inner.height() * 0.5;
        let rad = 18.5;

        let mut n_fine = ((op.fine_cents + 50.0) / 100.0).clamp(0.0, 1.0);
        let mut n_oct = ((op.octave + 2) as f32 + 0.5) / 5.0;
        let mut n_wob = (op.wobble / 12.0).clamp(0.0, 1.0);
        let mut n_spr = op.spread.clamp(0.0, 1.0);

        let items: [(&str, &str, f32, usize); 4] = [
            ("Fine", "pitch fine", 0.5, 0),
            ("Octave", "pitch octave", 0.5, 5),
            ("Wobble", "pitch wobble", 0.0, 0),
            ("Spread", "pitch spread", 0.8, 0),
        ];

        for (k, &(_name, a11y, def, det)) in items.iter().enumerate() {
            let cx = inner.left() + ((k % 2) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 2) as f32 * row_h + 28.0;
            let c = egui::pos2(cx, cy);
            let vref = match k {
                0 => &mut n_fine,
                1 => &mut n_oct,
                2 => &mut n_wob,
                _ => &mut n_spr,
            };
            knob(ui, c, rad, id.with(k), a11y, vref, def, det);
        }

        op.fine_cents = (n_fine * 100.0 - 50.0).round();
        op.octave = ((n_oct * 5.0).floor() as i32).clamp(0, 4) - 2;
        op.wobble = n_wob * 12.0;
        op.spread = n_spr;

        let texts = [
            format!("{:+.0}ct", op.fine_cents).replace("+0ct", "0ct"),
            format!("{:+}", op.octave).replace("+0", "0"),
            format!("{:.1}st", op.wobble),
            format!("{:.0}%", op.spread * 100.0),
        ];
        for (k, (&(name, _, _, _), val_s)) in items.iter().zip(texts.iter()).enumerate() {
            let cx = inner.left() + ((k % 2) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 2) as f32 * row_h + 28.0;
            caps(&p, egui::pos2(cx, cy + rad + 11.0), Align2::CENTER_CENTER, name, 9.0);
            p.text(egui::pos2(cx, cy + rad + 24.0), Align2::CENTER_CENTER, val_s, mono(10.5), INK);
        }

        if (op.fine_cents, op.octave, op.wobble, op.spread) != before {
            self.write_owner_params(host, owner, op);
        }
    }

    fn clip_effects_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Effects  ♪ ▣");
        let id = ui.id().with(("clip-fx", owner));
        let cols = 2usize;
        let rows = 4usize;
        let g = 6.0;
        let pw = (inner.width() - g) * 0.5;
        let ph = ((inner.height() - g * (rows as f32 - 1.0)) / rows as f32).clamp(28.0, 40.0);
        let key = self.key;

        match owner {
            EnvTarget::Emitter(em_i) => {
                for (idx, &(slot_key, label)) in FX_SLOTS.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let pr = Rect::from_min_size(
                        egui::pos2(inner.left() + c_i as f32 * (pw + g), inner.top() + r_i as f32 * (ph + g)),
                        egui::vec2(pw, ph),
                    );
                    let e = &mut self.emitters[em_i];
                    let map = MacroMap::resolve(&e.macros, Some(&key));
                    let (mut a, mut v) = match slot_key {
                        "stretch" => (e.stretch, e.stretch),
                        "lowpass" => (e.lowpass, e.lowpass * 0.7),
                        "highpass" => (e.highpass, e.highpass * 0.7),
                        _ => {
                            let (ca, cv) = get_chain_fx(&e.effects, slot_key, e.lowpass);
                            if ca + cv > 0.01 {
                                (ca, cv)
                            } else if slot_key == "delay" && map.echo_mix > 0.01 {
                                (map.echo_mix, map.echo_mix)
                            } else if slot_key == "reverb" && map.reverb_mix > 0.01 {
                                (map.reverb_mix, map.reverb_mix)
                            } else {
                                (0.0, 0.0)
                            }
                        }
                    };
                    if dual_pill(ui, pr, id.with(slot_key), label, &format!("fx {}", label.to_lowercase()), &mut a, &mut v) {
                        let e = &mut self.emitters[em_i];
                        match slot_key {
                            "stretch" => e.stretch = a.max(v),
                            "lowpass" => e.lowpass = a.max(v),
                            "highpass" => e.highpass = a.max(v),
                            _ => {
                                let mut lp = e.lowpass;
                                set_chain_fx(&mut e.effects, slot_key, key, a, v, &mut lp);
                                if (slot_key == "delay" || slot_key == "reverb") && a + v < 0.01 {
                                    e.macros[2] = 0.0;
                                    host.set_shared_macro(2, 0.0);
                                }
                            }
                        }
                    }
                }
            }
            EnvTarget::Pad(pi) => {
                let (mut chain, mut lp_age, mut stretch_amt, mut hp_amt, has_track) = {
                    let pp = host.project().lock().unwrap();
                    if let Some(t) = pp.tracks.get(pi) {
                        let st = t.clips.first().map(|c| (1.0 - c.grains.speed).clamp(0.0, 0.75) / 0.75).unwrap_or(0.0);
                        let hp = t
                            .clips
                            .first()
                            .and_then(|c| c.audio_filters.iter().find(|f| f.kind == FilterKind::HighPass))
                            .map(|f| ((f.freq / 20.0).ln() / (2400.0_f32 / 20.0).ln()).clamp(0.0, 1.0))
                            .unwrap_or(0.0);
                        (t.effects.clone(), t.tape.age, st, hp, true)
                    } else {
                        (Vec::new(), 0.0, 0.0, 0.0, false)
                    }
                };
                let mut changed = false;
                for (idx, &(slot_key, label)) in FX_SLOTS.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let pr = Rect::from_min_size(
                        egui::pos2(inner.left() + c_i as f32 * (pw + g), inner.top() + r_i as f32 * (ph + g)),
                        egui::vec2(pw, ph),
                    );
                    let (mut a, mut v) = match slot_key {
                        "stretch" => (stretch_amt, stretch_amt),
                        "highpass" => (hp_amt, hp_amt * 0.6),
                        _ => get_chain_fx(&chain, slot_key, lp_age),
                    };
                    if dual_pill(ui, pr, id.with(slot_key), label, &format!("fx {}", label.to_lowercase()), &mut a, &mut v) {
                        match slot_key {
                            "stretch" => stretch_amt = a.max(v),
                            "highpass" => hp_amt = a.max(v),
                            _ => set_chain_fx(&mut chain, slot_key, key, a, v, &mut lp_age),
                        }
                        changed = true;
                    }
                }
                if changed && has_track {
                    {
                        let mut pp = host.project().lock().unwrap();
                        if let Some(t) = pp.tracks.get_mut(pi) {
                            t.effects = chain;
                            t.tape.age = lp_age;
                            for c in &mut t.clips {
                                c.grains.speed = (1.0 - 0.75 * stretch_amt).clamp(0.25, 1.0);
                                c.audio_filters.retain(|f| f.kind != FilterKind::HighPass);
                                if hp_amt > 0.02 {
                                    let freq = 20.0 * (2400.0_f32 / 20.0).powf(hp_amt);
                                    c.audio_filters.push(FilterSpec { kind: FilterKind::HighPass, freq, q: 0.707 });
                                }
                            }
                        }
                    }
                    host.mark();
                }
            }
            EnvTarget::Lane(l) => {
                let mut chain = self.lane_fx[l].clone();
                let mut lp_age = {
                    let pp = host.project().lock().unwrap();
                    pp.tracks.iter().find(|t| t.name == "sequence").map(|t| t.tape.age).unwrap_or(0.0)
                };
                let mut changed = false;
                for (idx, &(slot_key, label)) in FX_SLOTS.iter().enumerate() {
                    let r_i = idx / cols;
                    let c_i = idx % cols;
                    let pr = Rect::from_min_size(
                        egui::pos2(inner.left() + c_i as f32 * (pw + g), inner.top() + r_i as f32 * (ph + g)),
                        egui::vec2(pw, ph),
                    );
                    let (mut a, mut v) = get_chain_fx(&chain, slot_key, lp_age);
                    if dual_pill(ui, pr, id.with(slot_key), label, &format!("fx {}", label.to_lowercase()), &mut a, &mut v) {
                        set_chain_fx(&mut chain, slot_key, key, a, v, &mut lp_age);
                        changed = true;
                    }
                }
                if changed {
                    self.lane_fx[l] = chain.clone();
                    {
                        let mut pp = host.project().lock().unwrap();
                        if let Some(t) = pp.tracks.iter_mut().find(|t| t.name == "sequence") {
                            t.effects = chain;
                            t.tape.age = lp_age;
                        }
                    }
                    host.mark();
                }
            }
        }
    }

    fn clip_links_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Links");
        let id = ui.id().with(("clip-links", owner));
        let cols = 2usize;
        let rows = 4usize;
        let g = 6.0;
        let pw = (inner.width() - g) * 0.5;
        let ph = ((inner.height() - g * (rows as f32 - 1.0)) / rows as f32).clamp(28.0, 40.0);

        let mut link: AvLink = match owner {
            EnvTarget::Emitter(i) => self.emitters[i].link,
            EnvTarget::Lane(l) => self.lane_links[l],
            EnvTarget::Pad(pi) => {
                let pp = host.project().lock().unwrap();
                pp.tracks
                    .get(pi)
                    .and_then(|t| t.clips.first().map(|c| c.link))
                    .unwrap_or(self.pad_links[pi.min(15)])
            }
        };
        let mut layout_pan = host.project().lock().unwrap().layout_pan;
        let before_link = link;
        let before_lp = layout_pan;

        let mut v_hue = (link.pitch_to_hue / 30.0).clamp(0.0, 1.0);
        let mut v_gain = link.gain_to_opacity.clamp(0.0, 1.0);
        let mut v_pan = link.pan_to_x.clamp(0.0, 1.0);
        let mut v_rate = link.pitch_to_rate.clamp(0.0, 1.0);
        let mut v_rev = if link.reverse_video { 1.0 } else { 0.0 };
        let mut v_env = link.envelope_to_opacity.clamp(0.0, 1.0);
        let mut v_lpan = if layout_pan { 1.0 } else { 0.0 };

        let specs: [(&str, f32, bool); 7] = [
            ("pitch → hue", 0.5, false),
            ("gain → opacity", 1.0, false),
            ("pan → x", 1.0, false),
            ("position → rate", 1.0, false),
            ("reverse → video", 1.0, true),
            ("envelope → fade", 1.0, false),
            ("layout → pan", 1.0, true),
        ];

        for (idx, &(label, def, disc)) in specs.iter().enumerate() {
            let r_i = idx / cols;
            let c_i = idx % cols;
            let pr = Rect::from_min_size(
                egui::pos2(inner.left() + c_i as f32 * (pw + g), inner.top() + r_i as f32 * (ph + g)),
                egui::vec2(pw, ph),
            );
            let vref = match idx {
                0 => &mut v_hue,
                1 => &mut v_gain,
                2 => &mut v_pan,
                3 => &mut v_rate,
                4 => &mut v_rev,
                5 => &mut v_env,
                _ => &mut v_lpan,
            };
            single_pill(ui, pr, id.with(idx), label, label, vref, def, disc);
        }

        link.pitch_to_hue = v_hue * 30.0;
        link.gain_to_opacity = v_gain;
        link.pan_to_x = v_pan;
        link.pitch_to_rate = v_rate;
        link.reverse_video = v_rev >= 0.5;
        link.envelope_to_opacity = v_env;
        layout_pan = v_lpan >= 0.5;

        if link != before_link {
            match owner {
                EnvTarget::Emitter(i) => self.emitters[i].link = link,
                EnvTarget::Lane(l) => {
                    self.lane_links[l] = link;
                    {
                        let mut pp = host.project().lock().unwrap();
                        if let Some(t) = pp.tracks.iter_mut().find(|t| t.name == "sequence") {
                            for c in &mut t.clips {
                                c.link = link;
                            }
                        }
                    }
                    host.mark();
                }
                EnvTarget::Pad(pi) => {
                    self.pad_links[pi.min(15)] = link;
                    {
                        let mut pp = host.project().lock().unwrap();
                        if let Some(t) = pp.tracks.get_mut(pi) {
                            for c in &mut t.clips {
                                c.link = link;
                            }
                        }
                    }
                    host.mark();
                }
            }
        }
        if layout_pan != before_lp {
            host.project().lock().unwrap().layout_pan = layout_pan;
            host.mark();
        }
    }

    /// The chop panel, self-contained: filmstrip + waveform with slices,
    /// then Auto / 4 / 8 / 16, Tap, Clear, `Slices → pads`.
    pub fn chop_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, src: usize) {
        let now = host.clock();
        let len = host.project().lock().unwrap().sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
        let slices = self.slices(host, src);
        let bar_h = 40.0;
        let film_h = (rect.height() * 0.20).clamp(32.0, 44.0);
        let film = Rect::from_min_size(rect.min, egui::vec2(rect.width(), film_h));
        let wave = Rect::from_min_max(
            egui::pos2(rect.left(), film.bottom() + 4.0),
            egui::pos2(rect.right(), rect.bottom() - bar_h - 10.0),
        );
        self.chop_area(host, ui, src, len, &slices, wave, film, now);
        let bar = Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - bar_h), egui::vec2(rect.width(), bar_h));
        self.chop_bar(host, ui, src, len, bar, now);
        if let Some(tc) = self.chop.tap.as_ref() {
            if now - tc.t0 >= tc.len {
                self.finish_tap(host);
            }
            ui.ctx().request_repaint();
        }
        if self.chop.playing.is_some_and(|(_, _, t)| now - t < 4.0) {
            ui.ctx().request_repaint();
        }
    }

    fn finish_tap(&mut self, host: &mut dyn Host) {
        if self.chop.tap.take().is_some() {
            host.stop_audition();
        }
    }

    /// Tap-chop: a chop point at the playhead.
    fn tap_chop(&mut self, host: &mut dyn Host, now: f64) {
        let Some(tc) = self.chop.tap.clone() else { return };
        let mut v = self.slices(host, tc.source);
        if chop::add_chop_point(&mut v, (now - tc.t0).clamp(0.0, tc.len), tc.len).is_some() {
            self.set_slices(host, tc.source, v);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn chop_area(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, src: usize, len: f64, slices: &[Slice], wave: Rect, film: Rect, now: f64) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        if len <= 0.0 {
            return;
        }
        let span_top = film.top().min(wave.top());
        let span_bot = film.bottom().max(wave.bottom());
        let x_of = |t: f64| wave.left() + (t / len) as f32 * wave.width();
        let t_of = |x: f32| ((x - wave.left()) / wave.width()).clamp(0.0, 1.0) as f64 * len;

        // 1. Filmstrip along the top.
        let frames = atlas::FRAMES;
        if let Some((tex, _)) = self.atlas(host, &ctx, src) {
            let fw = film.width() / frames as f32;
            for k in 0..frames {
                let fr = Rect::from_min_size(egui::pos2(film.left() + k as f32 * fw, film.top()), egui::vec2(fw + 0.5, film.height()));
                p.image(tex, fr, atlas::uv_cover(k, fw / film.height().max(1.0)), gray(0.86));
            }
        } else {
            p.rect_filled(film, 0.0, G2);
        }
        for (i, sl) in slices.iter().enumerate() {
            if i % 2 == 1 {
                let band = Rect::from_min_max(egui::pos2(x_of(sl.start), film.top()), egui::pos2(x_of(sl.end), film.bottom()));
                p.rect_filled(band, 0.0, Color32::from_black_alpha(65));
            }
        }
        mask_corners(&p, film, 8.0, Color32::from_rgb(0x10, 0x10, 0x12));

        // 2. Waveform + slice bands below.
        p.rect_filled(wave, r(9), G0);
        let playing = self.chop.playing.filter(|(s, _, t)| *s == src && now - t < 3.0).map(|(_, i, _)| i);
        for (i, sl) in slices.iter().enumerate() {
            let band = Rect::from_min_max(egui::pos2(x_of(sl.start), wave.top()), egui::pos2(x_of(sl.end), wave.bottom()));
            let fill = if playing == Some(i) { G4 } else if i % 2 == 0 { G1 } else { G2 };
            p.rect_filled(band, 0.0, fill);
            p.text(
                band.left_top() + egui::vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                (i + 1).to_string(),
                mono(9.5),
                if playing == Some(i) { INK } else { INK3 },
            );
        }
        let peaks = {
            let pp = host.project().lock().unwrap();
            self.waves.get(&pp.sources, src)
        };
        if let Some(pk) = peaks {
            pk.draw(&p, wave.shrink2(egui::vec2(0.0, 12.0)), 0.0, len, INK2);
        }

        // Edges (a shared boundary once: the left slice's end).
        let mut edges: Vec<(usize, bool, f64)> = Vec::new();
        for (i, sl) in slices.iter().enumerate() {
            let shared = i > 0 && (slices[i - 1].end - sl.start).abs() < 1e-9;
            if !shared {
                edges.push((i, false, sl.start));
            }
            edges.push((i, true, sl.end));
        }
        for (_, _, t) in &edges {
            let x = x_of(*t);
            p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(1.0, Color32::from_white_alpha(65)));
        }

        // Playhead (tap-chop / audition).
        if let Some(tc) = self.chop.tap.as_ref().filter(|tc| tc.source == src) {
            let x = x_of((now - tc.t0).clamp(0.0, len));
            p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(2.0, HI));
        } else if let Some((s, i, t)) = self.chop.playing {
            if s == src {
                if let Some(sl) = slices.get(i) {
                    let at = sl.start + (now - t);
                    if at < sl.end {
                        let x = x_of(at);
                        p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(1.5, HI));
                    }
                }
            }
        }
        mask_corners(&p, wave, 9.0, Color32::from_rgb(0x10, 0x10, 0x12));
        p.rect_stroke(wave, r(9), hair(LINE2), StrokeKind::Inside);

        // Edge drags first (grips straddling each edge near the bottom of wave).
        let mut new_slices: Option<Vec<Slice>> = None;
        let mut on_edge = false;
        for (k, (i, end, t)) in edges.iter().enumerate() {
            let x = x_of(*t);
            let grip = Rect::from_center_size(egui::pos2(x, wave.bottom() - 9.0), egui::vec2(5.0, 16.0));
            p.rect_filled(grip, r(2), INK2);
            let resp = hit(
                ui,
                Rect::from_center_size(grip.center(), egui::vec2(26.0, 32.0)),
                ui.id().with(("edge", src, k)),
                Sense::drag(),
                &format!("slice {} {}", i + 1, if *end { "end" } else { "start" }),
            );
            if resp.hovered() || resp.dragged() {
                on_edge = true;
            }
            if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                let mut v = slices.to_vec();
                chop::move_boundary(&mut v, *i, if *end { Edge::End } else { Edge::Start }, t_of(pos.x), len);
                new_slices = Some(v);
            }
        }

        // Taps on the wave / filmstrip: audition (tap mode: chop), double-tap deletes slice.
        let area = Rect::from_min_max(egui::pos2(wave.left(), span_top), egui::pos2(wave.right(), span_bot));
        let resp = hit(ui, area, ui.id().with(("chop-area", src)), Sense::click(), "chop wave");
        if !on_edge {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = t_of(pos.x);
                let under = chop::slice_at(slices, t);
                if self.chop.tap.as_ref().is_some_and(|tc| tc.source == src) {
                    if resp.clicked() {
                        self.tap_chop(host, now);
                    }
                } else if resp.double_clicked() {
                    if let Some(i) = under {
                        let mut v = slices.to_vec();
                        chop::delete_slice(&mut v, i);
                        new_slices = Some(v);
                        host.stop_audition();
                        self.chop.playing = None;
                    }
                } else if resp.clicked() {
                    match under {
                        Some(i) => {
                            let (a, b) = (slices[i].start, slices[i].end);
                            host.audition(src, a, b);
                            self.chop.playing = Some((src, i, now));
                        }
                        None => host.audition(src, t, (t + 1.0).min(len)),
                    }
                }
            }
        }
        if let Some(v) = new_slices {
            self.set_slices(host, src, v);
        }
    }

    fn chop_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, src: usize, len: f64, bar: Rect, now: f64) {
        let p = ui.painter().clone();
        // Slice by: Auto / 4 / 8 / 16.
        let seg = Rect::from_min_size(bar.min, egui::vec2(252.0, bar.height()));
        if let Some(i) = segmented_named(
            ui,
            seg,
            ui.id().with(("slice-by", src)),
            &["Auto", "4", "8", "16"],
            &["slice auto", "slice 4", "slice 8", "slice 16"],
            &[],
            usize::MAX,
            font(12.5),
            false,
        ) {
            let v = match i {
                0 => host.project().lock().unwrap().sources.get(src).map(|s| s.auto_slices(16)).unwrap_or_default(),
                1 => chop::equal_slices(len, 4),
                2 => chop::equal_slices(len, 8),
                _ => chop::equal_slices(len, 16),
            };
            if !v.is_empty() {
                self.set_slices(host, src, v);
            }
        }
        // Tap: play and drop chop points live.
        let tapping = self.chop.tap.as_ref().is_some_and(|t| t.source == src);
        let tb = Rect::from_min_size(egui::pos2(seg.right() + 10.0, bar.top()), egui::vec2(if tapping { 142.0 } else { 98.0 }, bar.height()));
        let label = if tapping { "Chop here" } else { "Tap" };
        if button_named(ui, tb, ui.id().with(("chop-tap", src)), label, "chop tap", tapping, Some(Icon::Chop), None).clicked() {
            if tapping {
                self.tap_chop(host, now);
            } else {
                self.set_slices(host, src, Vec::new());
                host.audition(src, 0.0, len);
                self.chop.tap = Some(TapChop { source: src, t0: now, len });
                self.chop.playing = None;
            }
        }
        let mut left_end = tb.right();
        if tapping {
            let sb = Rect::from_min_size(egui::pos2(tb.right() + 8.0, bar.top()), egui::vec2(78.0, bar.height()));
            left_end = sb.right();
            if button_named(ui, sb, ui.id().with(("chop-tap-done", src)), "Done", "chop tap done", false, None, None).clicked() {
                self.finish_tap(host);
            }
        }
        // Right: slice count, Clear, Slices → pads.
        let has_pads = host.caps().views.contains(&super::View::Perform) && host.perform().is_some();
        let n = self.slices(host, src).len();
        let mut rx = bar.right();
        if has_pads {
            let pb = Rect::from_min_size(egui::pos2(rx - 154.0, bar.top()), egui::vec2(154.0, bar.height()));
            rx = pb.left() - 8.0;
            let primary = n > 0;
            if button_named(ui, pb, ui.id().with(("to-pads", src)), "Slices → pads", "slices to pads", primary, Some(Icon::Grid), None).clicked()
                && n > 0
            {
                let k = self.slices_to_pads(host, src);
                host.status(format!("{k} slices on pads 1–{k}"));
                self.finish_tap(host);
                host.stop_audition();
                self.sheet = None;
                self.set_view(host, super::View::Perform);
            }
        }
        if n > 0 {
            let cb = Rect::from_min_size(egui::pos2(rx - 78.0, bar.top()), egui::vec2(78.0, bar.height()));
            rx = cb.left() - 12.0;
            if button_named(ui, cb, ui.id().with(("chop-clear", src)), "Clear", "clear slices", false, None, None).clicked() {
                self.set_slices(host, src, Vec::new());
            }
            if rx > left_end + 64.0 {
                p.text(
                    egui::pos2(rx, bar.center().y),
                    Align2::RIGHT_CENTER,
                    if n == 1 { "1 slice".to_string() } else { format!("{n} slices") },
                    mono(11.5),
                    INK3,
                );
            }
        }
    }

    /// Standalone envelope panel helper (retained for public API parity).
    pub fn envelope_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect, target: EnvTarget, thumb_src: Option<usize>) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        caps(&p, egui::pos2(area.left(), area.top() + 6.0), Align2::LEFT_CENTER, "Envelope — sound and picture", 10.5);
        let Some(mut adsr) = self.target_env(host, target) else { return };
        let side_w = 230.0f32.min(area.width() * 0.36);
        let er = Rect::from_min_size(egui::pos2(area.left(), area.top() + 22.0), egui::vec2(area.width() - side_w - 18.0, (area.height() - 26.0).clamp(80.0, 170.0)));
        let side = Rect::from_min_max(egui::pos2(er.right() + 18.0, er.top()), egui::pos2(area.right(), er.bottom()));
        let frame = match target {
            EnvTarget::Emitter(i) => atlas::frame_at(self.emitters[i].pos()),
            _ => 5,
        };
        let thumb = thumb_src.and_then(|s| self.atlas(host, &ctx, s)).map(|(tex, _)| (tex, atlas::uv_cover(frame, er.width() / er.height())));
        let mut changed = adsr_editor(ui, er, ui.id().with(("env", target)), &mut adsr, thumb);
        changed |= picture_switch(ui, side, ui.id().with(("cutfade", target)), &mut adsr);
        if changed {
            self.set_target_env(host, target, adsr);
        }
    }

    /// The envelope of a pad / lane / emitter.
    pub fn target_env(&self, host: &mut dyn Host, target: EnvTarget) -> Option<Adsr> {
        match target {
            EnvTarget::Emitter(i) => self.emitters.get(i).map(|e| e.adsr),
            EnvTarget::Pad(i) => host.perform().map(|p| p.pad_env(i).unwrap_or_default()),
            EnvTarget::Lane(l) => {
                let (scene, _) = self.seq_current(host);
                let pid = self.seq_pattern(scene)?;
                let pp = host.project().lock().unwrap();
                Some(pp.patterns.get(pid).and_then(|pat| pat.rows.get(l)).and_then(|r| r.grains.adsr).unwrap_or_default())
            }
        }
    }

    pub fn set_target_env(&mut self, host: &mut dyn Host, target: EnvTarget, adsr: Adsr) {
        match target {
            EnvTarget::Emitter(i) => {
                if let Some(e) = self.emitters.get_mut(i) {
                    e.adsr = adsr;
                }
            }
            EnvTarget::Pad(i) => {
                if let Some(p) = host.perform() {
                    p.set_pad_env(i, adsr);
                }
            }
            EnvTarget::Lane(l) => {
                {
                    let mut pp = host.project().lock().unwrap();
                    for pid in self.seq.scene_pats.iter().flatten() {
                        if let Some(r) = pp.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(l)) {
                            r.grains.adsr = Some(adsr);
                        }
                    }
                }
                host.mark();
            }
        }
    }
}

/// Picture: Cut | Fade, with a one-line note. Returns true when changed.
pub fn picture_switch(ui: &mut egui::Ui, side: Rect, id: egui::Id, adsr: &mut Adsr) -> bool {
    let p = ui.painter().clone();
    caps(&p, egui::pos2(side.left(), side.top() + 8.0), Align2::LEFT_CENTER, "Picture", 10.5);
    let seg = Rect::from_min_size(egui::pos2(side.left(), side.top() + 22.0), egui::vec2(side.width().min(220.0), 44.0));
    let fade = adsr.picture == PictureCut::Fade;
    let mut changed = false;
    if let Some(i) = segmented_named(ui, seg, id, &["Cut", "Fade"], &["picture cut", "picture fade"], &[], fade as usize, font(13.0), false) {
        adsr.picture = if i == 1 { PictureCut::Fade } else { PictureCut::Cut };
        changed = true;
    }
    let note = if adsr.picture == PictureCut::Fade {
        "the picture fades with the envelope"
    } else {
        "full picture, then a hard cut with the sound"
    };
    p.text(egui::pos2(side.left(), seg.bottom() + 16.0), Align2::LEFT_CENTER, note, font(11.5), INK3);
    changed
}
