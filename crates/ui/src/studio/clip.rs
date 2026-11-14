//! The unified calm `Clip` sheet (`D · Hybrid: "touch it · open it"` + VGS Sculpt/Tape):
//! - Header: `< Play` / `< Sequence` / `< Perform` back pill, clip title +
//!   provenance, Owner Switcher (`Clip · Emitter N · Lane N · Pad N`),
//!   `Replace` and `Dig`.
//! - Top band: live clip stage preview (`stack | split | grid | bleed` +
//!   draggable `(cx, cy)` focus crosshair) + 6 owner macros (`GRAIN`, `PITCH`,
//!   `SMEAR`, `LOOK`, `LEVEL`, `PAN`).
//! - Middle band: Koala-style `CHOP` card (16-frame filmstrip, waveform with
//!   subtle acoustic badges, `Auto · 4 · 8 · 16 · Tap`, edge grips, double-tap
//!   delete, `Loops → pads`, `Kit → pads`, `Slices → pads`).
//! - `SCULPT & TAPE` band: 1-tap stamps (`Raw · Kick · Snare · Hat · Pluck ·
//!   Chord · Phrase · Tape Loop · Disintegrate`) setting continuous dials
//!   (`Drop`, `Sub`, `Snap`, `Drive`, `Comb`, `Wow`, `Wear`) + `Chord`,
//!   `Choke`, `Sync | Free Loop (s)`, and optional `Caption` field.
//! - Bottom band: 5 parameter cards (`SHAPE · SOUND + PICTURE`, `GRAIN`,
//!   `PITCH`, `EFFECTS ♪ ▣`, `LINKS`) wired to the active owner.

use super::add::{field_sized, modal};
use super::deep::{get_chain_fx, set_chain_fx, sheet_back_button, sheet_card, FX_SLOTS};
use super::{Sheet, Studio, COLUMNS, EMITTERS, LANES};
use crate::atlas;
use crate::envelope::adsr_editor;
use crate::host::{AddRoute, AddTarget, Host, Pad, SculptParams, SculptStamp};
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
    Source(usize),
    Column(usize),
    Emitter(usize),
    Lane(usize),
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
    /// The slice last auditioned (highlighted) and when: `(source, slice_idx, clock)`.
    pub playing: Option<(usize, usize, f64)>,
    /// Last selected slice index on `source`.
    pub selected_slice: Option<(usize, usize)>,
    /// An edge being dragged: (slice, end? false = start).
    pub drag: Option<(usize, bool)>,
}

/// Snapshot of the 6 macros + Grain/Pitch sub-params for the active owner.
#[derive(Clone, Copy)]
pub(crate) struct OwnerParams {
    pub(crate) grain: f32,
    pub(crate) pitch_m: f32,
    pub(crate) smear: f32,
    pub(crate) look_m: f32,
    pub(crate) level: f32,
    pub(crate) pan_m: f32,
    pub(crate) density: f32,
    pub(crate) duration: f32,
    pub(crate) spray: f32,
    pub(crate) reverse: f32,
    pub(crate) fine_cents: f32,
    pub(crate) octave: i32,
    pub(crate) wobble: f32,
    pub(crate) spread: f32,
}

/// Compute a lightweight deterministic acoustic badge (`sub`, `snap`, `air`,
/// `C#3`, `phrase`) for a slice directly from its audio samples.
fn acoustic_slice_badge(src: &chromagrain_core::timeline::Source, sl: &Slice) -> &'static str {
    let dur = (sl.end - sl.start).max(0.0);
    let Some(audio) = src.audio.as_ref() else {
        return if dur >= 0.7 { "phrase" } else { "cut" };
    };
    let sr = audio.sample_rate.max(1) as f64;
    let i0 = ((sl.start * sr).floor() as usize).min(audio.samples.len());
    let i1 = ((sl.end * sr).ceil() as usize).clamp(i0, audio.samples.len());
    let seg = &audio.samples[i0..i1];
    if seg.len() < 64 {
        return "";
    }
    let step = (seg.len() / 2048).max(1);
    let mut sum_sq = 0.0f64;
    let mut zc = 0usize;
    let mut diff_sq = 0.0f64;
    let mut prev = seg[0];
    let mut count = 0usize;
    let mut k = 0usize;
    while k < seg.len() {
        let s = seg[k];
        sum_sq += (s as f64) * (s as f64);
        let d = s - prev;
        diff_sq += (d as f64) * (d as f64);
        if (prev <= 0.0 && s > 0.0) || (prev >= 0.0 && s < 0.0) {
            zc += 1;
        }
        prev = s;
        count += 1;
        k += step;
    }
    let rms = (sum_sq / count.max(1) as f64).sqrt();
    if rms < 1e-4 {
        return "";
    }
    let zcr = zc as f64 / (count.max(1) as f64 * step as f64 / sr);
    let hf_ratio = (diff_sq / sum_sq.max(1e-9)).sqrt();
    let atk_n = ((0.028 * sr) as usize).min(seg.len()).max(1);
    let atk_rms = (seg[..atk_n].iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / atk_n as f64).sqrt();
    let punch = atk_rms / rms.max(1e-6);

    if hf_ratio > 0.85 || zcr > 3200.0 {
        "air"
    } else if dur < 0.34 && zcr < 420.0 && punch > 1.05 {
        "sub"
    } else if dur < 0.38 && punch > 1.22 {
        "snap"
    } else if src.base_hz >= 55.0 && src.base_hz <= 1100.0 && dur >= 0.25 && hf_ratio < 0.45 {
        let midi = (69.0 + 12.0 * (src.base_hz / 440.0).log2()).round() as i32;
        const NOTES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
        match (NOTES[midi.rem_euclid(12) as usize], (midi / 12 - 1).clamp(1, 6)) {
            ("C", 2) => "C2", ("C", 3) => "C3", ("C", 4) => "C4",
            ("C#", 2) => "C#2", ("C#", 3) => "C#3", ("C#", 4) => "C#4",
            ("D", 2) => "D2", ("D", 3) => "D3", ("D", 4) => "D4",
            ("Eb", 3) => "Eb3", ("E", 3) => "E3", ("F", 2) => "F2", ("F", 3) => "F3",
            ("F#", 3) => "F#3", ("G", 2) => "G2", ("G", 3) => "G3",
            ("Ab", 3) => "Ab3", ("A", 2) => "A2", ("A", 3) => "A3",
            ("Bb", 3) => "Bb3", ("B", 3) => "B3",
            _ => "tone",
        }
    } else if dur >= 0.65 {
        "phrase"
    } else {
        "chop"
    }
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

    /// Slices 1..N onto pads 1..N (16 at most), carrying slice captions onto pads.
    pub fn slices_to_pads(&mut self, host: &mut dyn Host, src: usize) -> usize {
        let v = self.slices(host, src);
        for (i, _) in v.iter().take(16).enumerate() {
            if let Some(cap) = self.slice_captions.get(&(src, i)).cloned() {
                self.pad_captions[i] = cap;
            }
        }
        let Some(perf) = host.perform() else { return 0 };
        for (i, s) in v.iter().take(16).enumerate() {
            perf.load_pad_slice(src, s.start, s.end, i);
        }
        v.len().min(16)
    }

    pub fn owner_sculpt(&self, owner: EnvTarget) -> SculptParams {
        match owner {
            EnvTarget::Emitter(i) => self.emitters[i.min(EMITTERS - 1)].sculpt,
            EnvTarget::Lane(l) => self.lane_sculpt[l.min(LANES - 1)],
            EnvTarget::Pad(p) => self.pad_sculpt[p.min(15)],
        }
    }

    pub fn set_owner_sculpt(&mut self, host: &mut dyn Host, owner: EnvTarget, sp: SculptParams) {
        let core_sc = sp.to_core();
        match owner {
            EnvTarget::Emitter(i) => {
                let e = &mut self.emitters[i.min(EMITTERS - 1)];
                e.sculpt = sp;
                e.lowpass = sp.wear.clamp(0.0, 1.0) * 0.65;
            }
            EnvTarget::Lane(l) => {
                self.lane_sculpt[l.min(LANES - 1)] = sp;
                let mut pp = host.project().lock().unwrap();
                for pid in self.seq.scene_pats.iter().flatten() {
                    if let Some(r) = pp.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(l)) {
                        r.sculpt = core_sc;
                        r.choke_group = sp.choke.min(4);
                        r.grains.sculpt = core_sc;
                        r.grains.choke_group = sp.choke.min(4);
                    }
                }
                if let Some(t) = pp.tracks.iter_mut().find(|t| t.name == "sequence") {
                    t.tape.wow = sp.wow;
                    t.tape.age = sp.wear * 0.75;
                }
                drop(pp);
                self.rev += 1;
                host.mark();
            }
            EnvTarget::Pad(pi) => {
                let pi = pi.min(15);
                self.pad_sculpt[pi] = sp;
                if let Some(perf) = host.perform() {
                    perf.set_pad_sculpt(pi, core_sc, sp.choke.min(4));
                } else {
                    let mut pp = host.project().lock().unwrap();
                    let bpm = pp.bpm.max(20.0);
                    if let Some(t) = pp.tracks.get_mut(pi) {
                        t.sculpt = core_sc;
                        t.choke_group = sp.choke.min(4);
                        t.tape.wow = sp.wow;
                        t.tape.age = sp.wear * 0.8;
                        for c in &mut t.clips {
                            c.grains.sculpt = core_sc;
                            c.grains.choke_group = sp.choke.min(4);
                            if let Some(free_s) = sp.free_loop_secs {
                                let beats = (free_s.max(0.5) as f64 * bpm / 60.0).clamp(0.5, 64.0);
                                c.length_beats = beats;
                            }
                        }
                    }
                    drop(pp);
                    host.mark();
                }
            }
        }
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
        let tall_screen = screen.height() >= 760.0;
        let desired_h = if tall_screen { 744.0 } else { 576.0 };
        let size = egui::vec2(
            (screen.width() - 32.0).min(1320.0),
            (screen.height() - 24.0).min(desired_h),
        );
        let caps_cfg = host.caps();
        let owner = self.effective_owner(target, src);
        let mut close = false;
        let mut switch_target: Option<ClipTarget> = None;
        let mut add_action: Option<(AddRoute, AddTarget)> = None;

        let scrim = modal(ctx, "cg-clip", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(18.0, 11.0));
            let id = egui::Id::new("cg-clip");
            let p = ui.painter().clone();
            let wide_sheet = inner.width() >= 1040.0;

            let raw_name = self.source_name(host, src);
            let clean_name = super::clean_clip_label(&raw_name);
            let has_alpha = self.source_has_alpha(host, src);
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
                p.rect_filled(box_r, r(5), WELL);
                p.line_segment(
                    [egui::pos2(box_r.left() + 3.0, box_r.bottom() + 0.7), egui::pos2(box_r.right() - 3.0, box_r.bottom() + 0.7)],
                    Stroke::new(1.0, BEVEL_HI),
                );
                let mut ox = box_r.left() + 3.0;
                for ((ot, label, a11y, active), pw) in owners.iter().zip(pill_widths.iter()) {
                    let pr = Rect::from_min_size(egui::pos2(ox, box_r.top() + 2.5), egui::vec2(*pw - 2.0, 29.0));
                    let resp = hit(ui, pr, id.with(("owner", &a11y)), Sense::click(), a11y);
                    if *active {
                        radial_lens(&p, pr, MINT_CORE, MINT_MID, MINT_EDGE);
                        p.rect_stroke(pr, r(3), Stroke::new(1.0, Color32::from_rgb(0x28, 0x66, 0x4b)), StrokeKind::Inside);
                    } else {
                        let face = if resp.hovered() { G3 } else { PUTTY };
                        p.rect_filled(pr, r(3), face);
                        p.line_segment(
                            [egui::pos2(pr.left() + 2.0, pr.top() + 0.7), egui::pos2(pr.right() - 2.0, pr.top() + 0.7)],
                            Stroke::new(1.0, PUTTY_HI),
                        );
                    }
                    p.text(
                        pr.center(),
                        Align2::CENTER_CENTER,
                        label,
                        font_semibold(11.0),
                        if *active { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { INK },
                    );
                    if resp.clicked() && !*active {
                        switch_target = Some(*ot);
                    }
                    ox += *pw;
                }
                rx = box_r.left() - 12.0;
            }

            let badge_reserve = if has_alpha { 26.0 } else { 0.0 };
            let max_title_chars = ((rx - title_x - badge_reserve) / 8.5).max(8.0) as usize;
            let max_sub_chars = ((rx - title_x) / 6.5).max(12.0) as usize;
            let title_r = p.text(
                egui::pos2(title_x, inner.top() + 10.0),
                Align2::LEFT_CENTER,
                short(&clean_name, max_title_chars),
                font_semibold(16.5),
                INK,
            );
            if has_alpha && title_r.right() + 22.0 <= rx {
                cutout_badge(&p, egui::pos2(title_r.right() + 7.0, inner.top() + 2.5));
            }
            p.text(
                egui::pos2(title_x, inner.top() + 28.0),
                Align2::LEFT_CENTER,
                short(&sub, max_sub_chars),
                mono(10.5),
                INK3,
            );

            // --- Body: Clean 3-Row Hardware Sheet (zero scroll at 1600×1000 AND 960×600!) ---
            let body = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + 42.0), inner.max);
            let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
            egui::ScrollArea::vertical()
                .id_salt("cg-clip-scroll")
                .auto_shrink([false, false])
                .show(&mut body_ui, |ui| {
                    let tall = body.height() >= 620.0;
                    let gap = if tall { 11.0 } else { 6.0 };
                    let top_h = if tall { 196.0 } else { 152.0 };
                    let chop_card_h = if tall { 228.0 } else { 174.0 };
                    let deck_h = if tall { 230.0 } else { 170.0 };
                    let total_h = top_h + gap + chop_card_h + gap + deck_h;
                    let needs_scroll = total_h > body.height() + 2.0;
                    let avail_w = if needs_scroll {
                        (body.width() - 8.0).max(320.0)
                    } else {
                        body.width().max(320.0)
                    };

                    let (canvas, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), Sense::hover());
                    let cp = ui.painter().clone();

                    // 1. Row 1: Unobstructed Preview + Bezel | 6 Owner Macros | SHAPE · ADSR
                    let stage_w = (avail_w * 0.245).floor().clamp(212.0, 308.0);
                    let shape_w = (avail_w * 0.285).floor().clamp(238.0, 356.0);
                    let macros_w = (avail_w - stage_w - shape_w - gap * 2.0).max(260.0);

                    let stage_r = Rect::from_min_size(canvas.min, egui::vec2(stage_w, top_h));
                    self.clip_stage_preview(host, ui, stage_r, src, len, owner);

                    let macros_r = Rect::from_min_size(
                        egui::pos2(stage_r.right() + gap, canvas.top()),
                        egui::vec2(macros_w, top_h),
                    );
                    self.clip_macros_card(host, ui, macros_r, owner);

                    let shape_r = Rect::from_min_size(
                        egui::pos2(macros_r.right() + gap, canvas.top()),
                        egui::vec2(avail_w - stage_w - macros_w - gap * 2.0, top_h),
                    );
                    self.clip_shape_card(host, ui, shape_r, owner, src);

                    // 2. Row 2: `CHOP & CUTOUT` Bay (`Cutout Person`, `Cutout Subject`, `Vocal Kit → pads`, etc.)
                    let chop_card_r = Rect::from_min_size(
                        egui::pos2(canvas.left(), stage_r.bottom() + gap),
                        egui::vec2(avail_w, chop_card_h),
                    );
                    putty_panel(&cp, chop_card_r, 7);
                    caps(
                        &cp,
                        egui::pos2(chop_card_r.left() + 14.0, chop_card_r.top() + 15.0),
                        Align2::LEFT_CENTER,
                        "Chop & cutout",
                        9.0,
                    );
                    // Top-right header rail: `Stretch ×8` (`"paulstretch clip"`) + `Cutout Person` (`"cutout person"`) + `Cutout Subject` (`"cutout subject"`).
                    let hdr_y = chop_card_r.top() + 4.0;
                    let hdr_h = 24.0;
                    let mut hrx = chop_card_r.right() - 12.0;
                    let subj_w = if avail_w >= 960.0 { 128.0 } else { 114.0 };
                    let pers_w = if avail_w >= 960.0 { 124.0 } else { 110.0 };
                    let str_w = if avail_w >= 960.0 { 96.0 } else { 88.0 };
                    let subj_r = Rect::from_min_size(egui::pos2(hrx - subj_w, hdr_y), egui::vec2(subj_w, hdr_h));
                    hrx = subj_r.left() - 6.0;
                    let pers_r = Rect::from_min_size(egui::pos2(hrx - pers_w, hdr_y), egui::vec2(pers_w, hdr_h));
                    hrx = pers_r.left() - 6.0;
                    let str_r = Rect::from_min_size(egui::pos2(hrx - str_w, hdr_y), egui::vec2(str_w, hdr_h));
                    hrx = str_r.left() - 8.0;

                    let s_resp = hit(ui, str_r, id.with(("paulstretch-clip", src)), Sense::click(), "paulstretch clip");
                    paint_switch_cap(&cp, str_r, SwitchKind::Amber, false, s_resp.hovered(), s_resp.is_pointer_button_down_on());
                    cp.text(
                        str_r.center(),
                        Align2::CENTER_CENTER,
                        "Stretch ×8",
                        font_semibold(10.5),
                        Color32::from_rgb(0x24, 0x1c, 0x09),
                    );
                    if s_resp.clicked() {
                        if let Some(new_src) = host.paulstretch_source(src, 8.0) {
                            match target {
                                ClipTarget::Pad(pi) => {
                                    if let Some(perf) = host.perform() {
                                        perf.load_pad(pi, new_src);
                                    }
                                }
                                ClipTarget::Lane(l) => {
                                    let li = l.min(LANES - 1);
                                    self.seq.lanes[li] = Some(new_src);
                                    let mut pp = host.project().lock().unwrap();
                                    for pid in self.seq.scene_pats.iter().flatten() {
                                        if let Some(r) = pp.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(li)) {
                                            r.source = new_src;
                                        }
                                    }
                                    self.rev += 1;
                                }
                                ClipTarget::Column(c) => {
                                    self.columns[c.min(COLUMNS - 1)] = Some(new_src);
                                }
                                ClipTarget::Emitter(i) => {
                                    let col = self.emitters[i.min(EMITTERS - 1)].col.min(COLUMNS - 1);
                                    self.columns[col] = Some(new_src);
                                }
                                ClipTarget::Source(_) => {
                                    switch_target = Some(ClipTarget::Source(new_src));
                                }
                            }
                            let wash_sp = SculptParams::preset(SculptStamp::Wash);
                            self.set_owner_sculpt(host, owner, wash_sp);
                            let mut env = self.target_env(host, owner).unwrap_or_default();
                            env.attack = 0.55;
                            env.decay = 0.30;
                            env.sustain = 0.94;
                            env.release = 1.05;
                            env.picture = chromagrain_core::envelope::PictureCut::Fade;
                            self.set_target_env(host, owner, env);
                            host.set_stage_mode(crate::host::StageMode::Stack);
                            host.status("paulstretched ×8 into lush ambient chord wash".into());
                        }
                    }

                    for (btn_r, lbl, a11y, mode, key_id) in [
                        (pers_r, "Cutout Person", "cutout person", "person", "cutout-person"),
                        (subj_r, "Cutout Subject", "cutout subject", "subject", "cutout-subject"),
                    ] {
                        let resp = hit(ui, btn_r, id.with((key_id, src)), Sense::click(), a11y);
                        paint_switch_cap(
                            &cp,
                            btn_r,
                            SwitchKind::PuttyMint,
                            false,
                            resp.hovered(),
                            resp.is_pointer_button_down_on(),
                        );
                        draw_icon(&cp, Icon::Cutout, egui::pos2(btn_r.left() + 12.0, btn_r.center().y), 11.5, INK);
                        cp.text(
                            egui::pos2(btn_r.left() + 23.0, btn_r.center().y),
                            Align2::LEFT_CENTER,
                            lbl,
                            font_semibold(10.5),
                            INK,
                        );
                        if resp.clicked() {
                            self.trigger_cutout(host, src, mode);
                        }
                    }
                    if has_alpha && hrx > chop_card_r.left() + 210.0 {
                        cp.text(
                            egui::pos2(hrx, chop_card_r.top() + 15.0),
                            Align2::RIGHT_CENTER,
                            "ALPHA STICKER READY",
                            mono_medium(8.5),
                            Color32::from_rgb(0x1f, 0x5c, 0x42),
                        );
                    } else if !has_audio && hrx > chop_card_r.left() + 180.0 {
                        cp.text(
                            egui::pos2(hrx, chop_card_r.top() + 15.0),
                            Align2::RIGHT_CENTER,
                            "PICTURE",
                            mono(9.0),
                            INK3,
                        );
                    }

                    let chop_inner = Rect::from_min_max(
                        egui::pos2(chop_card_r.left() + 12.0, chop_card_r.top() + 31.0),
                        egui::pos2(chop_card_r.right() - 12.0, chop_card_r.bottom() - 8.0),
                    );
                    self.chop_panel(host, ui, chop_inner, src);
                    if self.sheet.is_none() {
                        return;
                    }

                    // 3. Row 3: Clean 2-Tab Lower Deck (`SCULPT & TAPE` | `GRAIN · FX · LINKS`)
                    let deck_r = Rect::from_min_size(
                        egui::pos2(canvas.left(), chop_card_r.bottom() + gap),
                        egui::vec2(avail_w, deck_h),
                    );
                    self.clip_lower_deck(host, ui, deck_r, owner, src);
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

    fn clip_stage_preview(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, total: Rect, src: usize, len: f64, owner: EnvTarget) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let now = host.clock();
        let bezel_h = 24.0;
        let stage = Rect::from_min_size(total.min, egui::vec2(total.width(), (total.height() - bezel_h - 4.0).max(72.0)));
        let bezel = Rect::from_min_size(egui::pos2(total.left(), stage.bottom() + 4.0), egui::vec2(total.width(), bezel_h));

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
        monitor_well(&p, stage, 7);
        if let Some((tex, a)) = self.atlas(host, &ctx, src) {
            tile(&p, stage.shrink(1.8), 5, tex, a.uv_cover(atlas::frame_at(norm_pos), stage.width() / stage.height()), gray(0.95));
        } else {
            p.rect_filled(stage.shrink(1.8), r(5), WELL2);
        }
        monitor_bezel(&p, stage, 7);

        // Dedicated bezel mode strip BELOW the video preview (`grid · collage · split · stack`) — 0% video obstruction!
        p.rect_filled(bezel, r(4), SLOT_BG);
        p.line_segment(
            [egui::pos2(bezel.left() + 2.0, bezel.bottom() + 0.6), egui::pos2(bezel.right() - 2.0, bezel.bottom() + 0.6)],
            Stroke::new(1.0, BEVEL_HI),
        );
        p.rect_stroke(bezel, r(4), Stroke::new(1.0, Color32::from_rgb(0x14, 0x13, 0x10)), StrokeKind::Inside);
        self.stage_mode_bezel_cells(
            host,
            ui,
            bezel.shrink2(egui::vec2(2.5, 2.0)),
            "clip-layout",
            ["clip layout tile", "clip layout collage", "clip layout split", "clip layout stack"],
        );

        // Draggable `(cx, cy)` smart crop focus crosshair across the entire unobstructed aperture!
        let mut focus = self
            .source_focus
            .get(&src)
            .copied()
            .unwrap_or_else(|| match owner {
                EnvTarget::Emitter(i) => self.emitters[i].focus,
                _ => (0.5, 0.5),
            });
        let fresp = hit(ui, stage, ui.id().with(("clip-focus", src)), Sense::click_and_drag(), "focus point");
        if fresp.double_clicked() {
            focus = (0.5, 0.5);
            self.source_focus.insert(src, focus);
            if let EnvTarget::Emitter(i) = owner {
                self.emitters[i].focus = focus;
            }
            host.mark();
        } else if fresp.dragged() {
            if let Some(pt) = fresp.interact_pointer_pos() {
                focus = (
                    ((pt.x - stage.left()) / stage.width().max(1.0)).clamp(0.15, 0.85),
                    ((pt.y - stage.top()) / stage.height().max(1.0)).clamp(0.15, 0.85),
                );
                self.source_focus.insert(src, focus);
                if let EnvTarget::Emitter(i) = owner {
                    self.emitters[i].focus = focus;
                }
                host.mark();
            }
        }
        let fp = egui::pos2(
            stage.left() + focus.0.clamp(0.15, 0.85) * stage.width(),
            stage.top() + focus.1.clamp(0.15, 0.85) * stage.height(),
        );
        let act = fresp.dragged() || fresp.hovered() || (focus.0 - 0.5).abs() > 0.01 || (focus.1 - 0.5).abs() > 0.01;
        let ring_col = Color32::from_white_alpha(if act { 215 } else { 125 });
        p.circle_stroke(fp, 7.0, Stroke::new(1.5, Color32::from_black_alpha(150)));
        p.circle_stroke(fp, 6.0, Stroke::new(1.2, ring_col));
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            p.line_segment(
                [fp + egui::vec2(dx * 8.0, dy * 8.0), fp + egui::vec2(dx * 11.5, dy * 11.5)],
                Stroke::new(1.2, ring_col),
            );
        }
        if act {
            p.text(
                egui::pos2(stage.right() - 8.0, stage.bottom() - 6.0),
                Align2::RIGHT_BOTTOM,
                format!("focus {:.0},{:.0}%", focus.0 * 100.0, focus.1 * 100.0),
                mono(9.0),
                Color32::from_white_alpha(175),
            );
        }
    }

    fn clip_lower_deck(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, deck: Rect, owner: EnvTarget, src: usize) {
        let p = ui.painter().clone();
        putty_panel(&p, deck, 7);

        // Top 2-Tab Physical Interlock Switch: `SCULPT & TAPE` | `GRAIN · FX · LINKS`
        let tab_bar = Rect::from_min_size(egui::pos2(deck.left() + 12.0, deck.top() + 4.0), egui::vec2(336.0_f32.min(deck.width() - 24.0), 25.0));
        let cur_tab = self.clip_tab.min(1);
        if let Some(picked) = segmented_named(
            ui,
            tab_bar,
            ui.id().with("clip-lower-tabs"),
            &["Sculpt & Tape", "Grain · FX · Links"],
            &["clip tab sculpt", "clip tab detail"],
            &[],
            cur_tab,
            font_semibold(11.0),
            true,
        ) {
            self.clip_tab = picked;
        }

        let inner = Rect::from_min_max(
            egui::pos2(deck.left() + 12.0, deck.top() + 33.0),
            egui::pos2(deck.right() - 12.0, deck.bottom() - 7.0),
        );
        if self.clip_tab == 0 {
            self.clip_sculpt_inner(host, ui, inner, owner, src);
        } else {
            let g = 10.0;
            let w_gp = (inner.width() * 0.38).floor();
            let w_fx = ((inner.width() - w_gp - g * 2.0) * 0.5).floor();
            let w_lk = inner.width() - w_gp - w_fx - g * 2.0;

            let r_gp = Rect::from_min_size(inner.min, egui::vec2(w_gp, inner.height()));
            let r_fx = Rect::from_min_size(egui::pos2(r_gp.right() + g, inner.top()), egui::vec2(w_fx, inner.height()));
            let r_lk = Rect::from_min_size(egui::pos2(r_fx.right() + g, inner.top()), egui::vec2(w_lk, inner.height()));

            self.clip_grain_pitch_bay(host, ui, r_gp, owner);
            self.clip_effects_bay(host, ui, r_fx, owner);
            self.clip_links_bay(host, ui, r_lk, owner);
        }
    }

    fn clip_sculpt_inner(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, inner: Rect, owner: EnvTarget, src: usize) {
        let p = ui.painter().clone();
        let id = ui.id().with(("clip-sculpt", owner));
        let mut sp = self.owner_sculpt(owner);
        let before = sp;

        // --- Row 1: 1-tap Musical Stamps (including `Vox Perc`) + Editable `Caption` field ---
        let row1_h = if inner.height() >= 150.0 { 29.0 } else { 25.0 };
        let compact_stamps = inner.width() < 980.0;
        let mut sx = inner.left();
        let sy = inner.top();
        for st in SculptStamp::ALL {
            let lbl = if st == SculptStamp::VoxPerc {
                "Vox Perc"
            } else if compact_stamps {
                st.short_label()
            } else {
                st.display_label()
            };
            let tw = p.layout_no_wrap(lbl.into(), font_semibold(10.5), INK).size().x;
            let pw = (tw + if compact_stamps { 13.0 } else { 17.0 }).max(34.0);
            let pr = Rect::from_min_size(egui::pos2(sx, sy), egui::vec2(pw, row1_h));
            let on = sp.stamp == st;
            let resp = hit(ui, pr, id.with(("stamp", st.label())), Sense::click(), &format!("sculpt {}", st.label()));
            paint_switch_cap(&p, pr, SwitchKind::Amber, on, resp.hovered(), resp.is_pointer_button_down_on());
            p.text(
                pr.center(),
                Align2::CENTER_CENTER,
                lbl,
                font_semibold(10.5),
                if on { ON_HI } else { Color32::from_rgb(0x24, 0x1c, 0x09) },
            );
            if resp.clicked() {
                sp = SculptParams::preset(st);
                if st == SculptStamp::Wash {
                    let mut env = self.target_env(host, owner).unwrap_or_default();
                    env.attack = 0.45;
                    env.decay = 0.30;
                    env.sustain = 0.92;
                    env.release = 0.95;
                    env.picture = chromagrain_core::envelope::PictureCut::Fade;
                    self.set_target_env(host, owner, env);
                }
            }
            sx = pr.right() + 4.0;
        }

        // Caption field on right of Row 1 (updates active slice + owner caption).
        let cap_left = (sx + 8.0).max(inner.right() - 260.0);
        if inner.right() - cap_left >= 110.0 {
            let cap_r = Rect::from_min_size(egui::pos2(cap_left, sy), egui::vec2(inner.right() - cap_left, row1_h));
            let sel_sl = self.chop.selected_slice.filter(|(s, _)| *s == src).map(|(_, i)| i);
            let mut cap_txt = if let Some(si) = sel_sl {
                self.slice_captions.get(&(src, si)).cloned().unwrap_or_default()
            } else {
                match owner {
                    EnvTarget::Emitter(i) => self.emitters[i].caption.clone(),
                    EnvTarget::Lane(l) => self.lane_captions[l].clone(),
                    EnvTarget::Pad(pi) => self.pad_captions[pi.min(15)].clone(),
                }
            };
            let prev_cap = cap_txt.clone();
            field_sized(
                ui,
                cap_r,
                id.with("caption"),
                &mut cap_txt,
                "Caption (spoken hook)…",
                "slice caption",
                11.0,
            );
            if cap_txt != prev_cap {
                if let Some(si) = sel_sl {
                    if cap_txt.trim().is_empty() {
                        self.slice_captions.remove(&(src, si));
                    } else {
                        self.slice_captions.insert((src, si), cap_txt.clone());
                    }
                    if let Some(so) = host.project().lock().unwrap().sources.get_mut(src) {
                        if let Some(sl) = so.slices.get_mut(si) {
                            sl.caption = chromagrain_core::chop::Caption::new(&cap_txt);
                        }
                    }
                    host.slices_changed(src);
                }
                match owner {
                    EnvTarget::Emitter(i) => self.emitters[i].caption = cap_txt,
                    EnvTarget::Lane(l) => self.lane_captions[l] = cap_txt,
                    EnvTarget::Pad(pi) => self.pad_captions[pi.min(15)] = cap_txt,
                }
                host.mark();
            }
        }

        // --- Row 2: 7 Continuous Dials + 3 Discrete/Drag Selectors (`Chord`, `Choke`, `Free Loop`) ---
        let row2 = Rect::from_min_max(egui::pos2(inner.left(), sy + row1_h + 5.0), inner.max);
        let right_w = (row2.width() * 0.34).clamp(242.0, 340.0);
        let knobs_w = (row2.width() - right_w - 12.0).max(280.0);
        let kw = knobs_w / 7.0;
        let compact_r2 = row2.height() < 118.0;
        let rad = (row2.height() * 0.23).clamp(13.0, 17.5);
        let ky = row2.top() + rad + if compact_r2 { 2.0 } else { 6.0 };

        let mut n_drop = ((sp.pitch_drop_st + 24.0) / 48.0).clamp(0.0, 1.0);
        let mut n_sub = sp.sub.clamp(0.0, 1.0);
        let mut n_snap = sp.snap.clamp(0.0, 1.0);
        let mut n_drive = (sp.drive_db / 18.0).clamp(0.0, 1.0);
        let mut n_comb = sp.comb.clamp(0.0, 1.0);
        let mut n_wow = sp.wow.clamp(0.0, 1.0);
        let mut n_wear = sp.wear.clamp(0.0, 1.0);

        let dials: [(&str, &str, f32); 7] = [
            ("Drop", "sculpt pitch drop", 0.5),
            ("Sub", "sculpt sub", 0.0),
            ("Snap", "sculpt snap", 0.0),
            ("Drive", "sculpt drive", 0.0),
            ("Comb", "sculpt comb", 0.0),
            ("Wow", "sculpt wow", 0.0),
            ("Wear", "sculpt wear", 0.0),
        ];

        for (k, &(name, a11y, def)) in dials.iter().enumerate() {
            let cx = row2.left() + (k as f32 + 0.5) * kw;
            let c = egui::pos2(cx, ky);
            let vref = match k {
                0 => &mut n_drop,
                1 => &mut n_sub,
                2 => &mut n_snap,
                3 => &mut n_drive,
                4 => &mut n_comb,
                5 => &mut n_wow,
                _ => &mut n_wear,
            };
            knob(ui, c, rad, id.with(("dial", k)), a11y, vref, def, 0);
            let _ = name;
        }

        sp.pitch_drop_st = (n_drop * 48.0 - 24.0).round();
        sp.sub = n_sub;
        sp.snap = n_snap;
        sp.drive_db = n_drive * 18.0;
        sp.comb = n_comb;
        sp.wow = n_wow;
        sp.wear = n_wear;

        let val_strs = [
            format!("{:+.0}st", sp.pitch_drop_st).replace("+0st", "0st"),
            format!("{:.0}%", sp.sub * 100.0),
            format!("{:.0}%", sp.snap * 100.0),
            format!("{:.1}dB", sp.drive_db),
            format!("{:.0}%", sp.comb * 100.0),
            format!("{:.0}%", sp.wow * 100.0),
            format!("{:.0}%", sp.wear * 100.0),
        ];
        for (k, (&(name, _, _), vs)) in dials.iter().zip(val_strs.iter()).enumerate() {
            let cx = row2.left() + (k as f32 + 0.5) * kw;
            caps(&p, egui::pos2(cx, ky + rad + 6.5), Align2::CENTER_CENTER, name, 8.0);
            let lcd_w = (kw - 6.0).clamp(38.0, 56.0);
            lcd_readout(&p, Rect::from_center_size(egui::pos2(cx, ky + rad + 19.0), egui::vec2(lcd_w, 15.0)), vs, 9.0);
        }

        // Right 3 selector switches: `Chord`, `Choke`, `Sync | Free Loop (s)`.
        let sel_area = Rect::from_min_max(egui::pos2(row2.right() - right_w, row2.top() + 2.0), egui::pos2(row2.right(), row2.bottom() - 2.0));
        let sg = 6.0;
        let sw = (sel_area.width() - sg * 2.0) / 3.0;
        let sh = sel_area.height().min(54.0);
        let sy2 = sel_area.center().y - sh * 0.5;

        // 1. Chord Voicing
        let r_chord = Rect::from_min_size(egui::pos2(sel_area.left(), sy2), egui::vec2(sw, sh));
        let c_resp = hit(ui, r_chord, id.with("chord"), Sense::click_and_drag(), "sculpt chord");
        let c_on = sp.chord != crate::host::ChordChoice::Off;
        paint_switch_cap(&p, r_chord, SwitchKind::PuttyMint, c_on, c_resp.hovered(), c_resp.is_pointer_button_down_on());
        let c_fg = if c_on { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { INK };
        p.text(r_chord.center() - egui::vec2(0.0, 6.5), Align2::CENTER_CENTER, sp.chord.label(), mono_medium(12.0), c_fg);
        p.text(r_chord.center() + egui::vec2(0.0, 9.5), Align2::CENTER_CENTER, "CHORD", font_semibold(8.0), c_fg);
        if c_resp.clicked() && !c_resp.dragged() {
            sp.chord = sp.chord.next();
        } else if c_resp.dragged() {
            let acc = ui.data(|d| d.get_temp::<f32>(c_resp.id).unwrap_or(0.0)) - c_resp.drag_delta().y;
            let stp = (acc / 14.0).trunc() as i32;
            if stp != 0 {
                ui.data_mut(|d| d.insert_temp(c_resp.id, acc - stp as f32 * 14.0));
                sp.chord = sp.chord.step_by(stp);
            } else {
                ui.data_mut(|d| d.insert_temp(c_resp.id, acc));
            }
        }

        // 2. Choke Group (`Off · 1 · 2 · 3 · 4`)
        let r_choke = Rect::from_min_size(egui::pos2(r_chord.right() + sg, sy2), egui::vec2(sw, sh));
        let k_resp = hit(ui, r_choke, id.with("choke"), Sense::click_and_drag(), "sculpt choke");
        let k_on = sp.choke > 0;
        paint_switch_cap(&p, r_choke, SwitchKind::PuttyMint, k_on, k_resp.hovered(), k_resp.is_pointer_button_down_on());
        let k_fg = if k_on { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { INK };
        let chk_lbl = if sp.choke == 0 { "Off".to_string() } else { format!("Grp {}", sp.choke) };
        p.text(r_choke.center() - egui::vec2(0.0, 6.5), Align2::CENTER_CENTER, chk_lbl, mono_medium(12.0), k_fg);
        p.text(r_choke.center() + egui::vec2(0.0, 9.5), Align2::CENTER_CENTER, "CHOKE", font_semibold(8.0), k_fg);
        if k_resp.clicked() && !k_resp.dragged() {
            sp.choke = (sp.choke + 1) % 5;
        }

        // 3. `Sync | Free Loop (s)`
        let r_loop = Rect::from_min_size(egui::pos2(r_choke.right() + sg, sy2), egui::vec2(sw, sh));
        let l_resp = hit(ui, r_loop, id.with("freeloop"), Sense::click_and_drag(), "sculpt free loop");
        let l_on = sp.free_loop_secs.is_some();
        paint_switch_cap(&p, r_loop, SwitchKind::PuttyAmber, l_on, l_resp.hovered(), l_resp.is_pointer_button_down_on());
        let loop_lbl = match sp.free_loop_secs {
            Some(s) => format!("{s:.1}s"),
            None => "Sync".into(),
        };
        let l_fg = if l_on { ON_HI } else { INK };
        p.text(r_loop.center() - egui::vec2(0.0, 6.5), Align2::CENTER_CENTER, loop_lbl, mono_medium(12.0), l_fg);
        p.text(r_loop.center() + egui::vec2(0.0, 9.5), Align2::CENTER_CENTER, "TAPE LOOP", font_semibold(8.0), l_fg);
        if l_resp.dragged() {
            let cur_s = sp.free_loop_secs.unwrap_or(6.8);
            let ds = -l_resp.drag_delta().y * 0.08 + l_resp.drag_delta().x * 0.06;
            sp.free_loop_secs = Some((cur_s + ds).clamp(1.5, 24.0));
        } else if l_resp.clicked() {
            sp.free_loop_secs = if sp.free_loop_secs.is_some() { None } else { Some(6.8) };
        }

        if sp != before {
            self.set_owner_sculpt(host, owner, sp);
        }
    }

    pub(crate) fn read_owner_params_pub(&self, host: &dyn Host, owner: EnvTarget) -> OwnerParams {
        self.read_owner_params(host, owner)
    }

    pub(crate) fn write_owner_params_pub(&mut self, host: &mut dyn Host, owner: EnvTarget, op: OwnerParams) {
        self.write_owner_params(host, owner, op);
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
                    level: gs.gain.clamp(0.0, 1.0),
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
        putty_panel(&p, rect, 7);
        let compact = rect.height() < 168.0;
        let inner = rect.shrink2(egui::vec2(if compact { 12.0 } else { 16.0 }, if compact { 4.0 } else { 6.0 }));
        let col_w = inner.width() / 3.0;
        let row_h = inner.height() / 2.0;
        let rad = (row_h * 0.215).clamp(12.5, 18.5);
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
            let cy = inner.top() + (k / 3) as f32 * row_h + rad + if compact { 2.0 } else { 4.0 };
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
            caps(&p, egui::pos2(cx, cy + rad + if compact { 6.0 } else { 7.5 }), Align2::CENTER_CENTER, name, if compact { 7.8 } else { 8.5 });
            let lcd_h = if compact { 14.5 } else { 16.0 };
            lcd_readout(
                &p,
                Rect::from_center_size(
                    egui::pos2(cx, cy + rad + if compact { 18.0 } else { 20.5 }),
                    egui::vec2((col_w - 22.0).clamp(44.0, 76.0), lcd_h),
                ),
                &shown,
                if compact { 9.0 } else { 9.5 },
            );
        }

        if (op.grain, op.pitch_m, op.smear, op.look_m, op.level, op.pan_m) != before {
            self.write_owner_params(host, owner, op);
        }
    }

    fn clip_shape_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, owner: EnvTarget, src: usize) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Shape · ADSR");
        let Some(mut adsr) = self.target_env(host, owner) else { return };

        let compact = inner.height() < 132.0;
        let bot_h = if compact { 30.0 } else { 36.0 };
        let env_h = (inner.height() - bot_h - 5.0).clamp(62.0, 148.0);
        let er = Rect::from_min_size(inner.min, egui::vec2(inner.width(), env_h));
        let frame = match owner {
            EnvTarget::Emitter(i) => atlas::frame_at(self.emitters[i].pos()),
            _ => 5,
        };
        let thumb = self
            .atlas(host, &ctx, src)
            .map(|(tex, _)| (tex, atlas::uv_cover(frame, er.width() / er.height().max(1.0))));
        let mut changed = adsr_editor(ui, er, ui.id().with(("env", owner)), &mut adsr, thumb);

        let bot = Rect::from_min_max(egui::pos2(inner.left(), er.bottom() + 4.0), inner.max);
        let seg_w = (bot.width() * 0.48).clamp(84.0, 118.0);
        let seg_h = (bot.height() - 2.0).clamp(24.0, 30.0);
        let seg_r = Rect::from_min_size(egui::pos2(bot.left(), bot.center().y - seg_h * 0.5), egui::vec2(seg_w, seg_h));
        let fade = adsr.picture == PictureCut::Fade;
        if let Some(pick) = segmented_named(
            ui,
            seg_r,
            ui.id().with(("cutfade", owner)),
            &["cut", "fade"],
            &["picture cut", "picture fade"],
            &[],
            usize::from(fade),
            font(11.0),
            false,
        ) {
            adsr.picture = if pick == 1 { PictureCut::Fade } else { PictureCut::Cut };
            changed = true;
        }

        let right_r = Rect::from_min_max(egui::pos2(seg_r.right() + 8.0, bot.top()), bot.max);
        let krad = if compact { 11.5 } else { 13.5 };
        let kc = egui::pos2(right_r.left() + krad + 4.0, right_r.center().y);
        let mut tv = ((adsr.cut_threshold - 0.01) / 0.49).clamp(0.0, 1.0);
        let before_tv = tv;
        knob(ui, kc, krad, ui.id().with(("cut-at", owner)), "cut threshold", &mut tv, (0.08 - 0.01) / 0.49, 0);
        if (tv - before_tv).abs() > 1e-4 {
            adsr.cut_threshold = 0.01 + tv * 0.49;
            changed = true;
        }
        let lx = kc.x + krad + 8.0;
        if right_r.right() - lx >= 34.0 {
            caps(&p, egui::pos2(lx, right_r.center().y - 7.0), Align2::LEFT_CENTER, "Cut at", 7.5);
            lcd_readout(
                &p,
                Rect::from_min_size(egui::pos2(lx, right_r.center().y - 0.5), egui::vec2((right_r.right() - lx).min(46.0), 14.5)),
                &format!("{:.0}%", adsr.cut_threshold * 100.0),
                8.8,
            );
        }

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

    fn clip_grain_pitch_bay(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bay: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        caps(&p, egui::pos2(bay.left(), bay.top() + 5.0), Align2::LEFT_CENTER, "Grain & pitch", 8.5);
        let inner = Rect::from_min_max(egui::pos2(bay.left(), bay.top() + 15.0), bay.max);
        let id = ui.id().with(("clip-gp", owner));
        let mut op = self.read_owner_params(host, owner);
        let before = (op.density, op.duration, op.spray, op.reverse, op.fine_cents, op.octave, op.wobble, op.spread);

        let col_w = inner.width() / 4.0;
        let row_h = inner.height() * 0.5;
        let compact = row_h < 76.0;
        let rad = (row_h * 0.215).clamp(11.5, 16.5);

        let mut n_dens = ((op.density - 2.0) / 118.0).clamp(0.0, 1.0);
        let mut n_dur = ((op.duration - 0.01) / 0.39).clamp(0.0, 1.0);
        let mut n_spray = op.spray.clamp(0.0, 1.0);
        let mut n_rev = op.reverse.clamp(0.0, 1.0);
        let mut n_fine = ((op.fine_cents + 50.0) / 100.0).clamp(0.0, 1.0);
        let mut n_oct = ((op.octave + 2) as f32 + 0.5) / 5.0;
        let mut n_wob = (op.wobble / 12.0).clamp(0.0, 1.0);
        let mut n_spr = op.spread.clamp(0.0, 1.0);

        let items: [(&str, &str, f32, usize); 8] = [
            ("Density", "grain density", (30.0 - 2.0) / 118.0, 0),
            ("Length", "grain length", (0.08 - 0.01) / 0.39, 0),
            ("Spray", "grain spray", 0.3, 0),
            ("Reverse", "grain reverse", 0.15, 0),
            ("Fine", "pitch fine", 0.5, 0),
            ("Octave", "pitch octave", 0.5, 5),
            ("Wobble", "pitch wobble", 0.0, 0),
            ("Spread", "pitch spread", 0.8, 0),
        ];

        for (k, &(_name, a11y, def, det)) in items.iter().enumerate() {
            let cx = inner.left() + ((k % 4) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 4) as f32 * row_h + rad + if compact { 1.5 } else { 4.0 };
            let vref = match k {
                0 => &mut n_dens,
                1 => &mut n_dur,
                2 => &mut n_spray,
                3 => &mut n_rev,
                4 => &mut n_fine,
                5 => &mut n_oct,
                6 => &mut n_wob,
                _ => &mut n_spr,
            };
            knob(ui, egui::pos2(cx, cy), rad, id.with(k), a11y, vref, def, det);
        }

        op.density = 2.0 + n_dens * 118.0;
        op.duration = 0.01 + n_dur * 0.39;
        op.spray = n_spray;
        op.reverse = n_rev;
        op.fine_cents = (n_fine * 100.0 - 50.0).round();
        op.octave = ((n_oct * 5.0).floor() as i32).clamp(0, 4) - 2;
        op.wobble = n_wob * 12.0;
        op.spread = n_spr;

        let texts = [
            format!("{:.0}/s", op.density),
            format!("{:.0}ms", op.duration * 1000.0),
            format!("{:.0}%", op.spray * 100.0),
            format!("{:.0}%", op.reverse * 100.0),
            format!("{:+.0}ct", op.fine_cents).replace("+0ct", "0ct"),
            format!("{:+}", op.octave).replace("+0", "0"),
            format!("{:.1}st", op.wobble),
            format!("{:.0}%", op.spread * 100.0),
        ];
        for (k, (&(name, _, _, _), val_s)) in items.iter().zip(texts.iter()).enumerate() {
            let cx = inner.left() + ((k % 4) as f32 + 0.5) * col_w;
            let cy = inner.top() + (k / 4) as f32 * row_h + rad + if compact { 1.5 } else { 4.0 };
            caps(&p, egui::pos2(cx, cy + rad + if compact { 5.5 } else { 7.0 }), Align2::CENTER_CENTER, name, 7.6);
            lcd_readout(
                &p,
                Rect::from_center_size(
                    egui::pos2(cx, cy + rad + if compact { 17.0 } else { 19.5 }),
                    egui::vec2((col_w - 8.0).clamp(38.0, 58.0), if compact { 14.0 } else { 15.0 }),
                ),
                val_s,
                8.8,
            );
        }

        if (op.density, op.duration, op.spray, op.reverse, op.fine_cents, op.octave, op.wobble, op.spread) != before {
            self.write_owner_params(host, owner, op);
        }
    }

    fn clip_effects_bay(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bay: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        caps(&p, egui::pos2(bay.left(), bay.top() + 5.0), Align2::LEFT_CENTER, "Effects  ♪ ▣", 8.5);
        let inner = Rect::from_min_max(egui::pos2(bay.left(), bay.top() + 15.0), bay.max);
        let id = ui.id().with(("clip-fx", owner));
        let cols = 2usize;
        let rows = 4usize;
        let g = if inner.height() < 125.0 { 4.0 } else { 6.0 };
        let pw = (inner.width() - g) * 0.5;
        let ph = ((inner.height() - g * (rows as f32 - 1.0)) / rows as f32).clamp(22.0, 38.0);
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
                let (mut chain, mut lp_age, mut stretch_amt, has_track) = {
                    let pp = host.project().lock().unwrap();
                    if let Some(t) = pp.tracks.get(pi) {
                        let st = t.clips.first().map(|c| (1.0 - c.grains.speed).clamp(0.0, 0.75) / 0.75).unwrap_or(0.0);
                        (t.effects.clone(), t.tape.age, st, true)
                    } else {
                        (Vec::new(), 0.0, 0.0, false)
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
                        _ => get_chain_fx(&chain, slot_key, lp_age),
                    };
                    if dual_pill(ui, pr, id.with(slot_key), label, &format!("fx {}", label.to_lowercase()), &mut a, &mut v) {
                        match slot_key {
                            "stretch" => stretch_amt = a.max(v),
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
                                c.audio_filters.retain(|f| f.kind != FilterKind::LowPass);
                                if lp_age > 0.02 {
                                    let freq = 18000.0 * (450.0_f32 / 18000.0).powf(lp_age);
                                    c.audio_filters.push(FilterSpec { kind: FilterKind::LowPass, freq, q: 0.707 });
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

    fn clip_links_bay(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bay: Rect, owner: EnvTarget) {
        let p = ui.painter().clone();
        caps(&p, egui::pos2(bay.left(), bay.top() + 5.0), Align2::LEFT_CENTER, "Links", 8.5);
        let inner = Rect::from_min_max(egui::pos2(bay.left(), bay.top() + 15.0), bay.max);
        let id = ui.id().with(("clip-links", owner));
        let cols = 2usize;
        let rows = 4usize;
        let g = if inner.height() < 125.0 { 4.0 } else { 6.0 };
        let pw = (inner.width() - g) * 0.5;
        let ph = ((inner.height() - g * (rows as f32 - 1.0)) / rows as f32).clamp(22.0, 38.0);

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

    /// The chop panel, self-contained: filmstrip + waveform with slices +
    /// acoustic badges, then `Auto / 4 / 8 / 16`, `Tap`, `Clear`, `Loops → pads`,
    /// `Kit → pads`, `Vocal Kit → pads`, `Slices → pads`.
    pub fn chop_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, src: usize) {
        let now = host.clock();
        let len = host.project().lock().unwrap().sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
        let slices = self.slices(host, src);
        let bar_h = if rect.height() < 150.0 { 30.0 } else { 35.0 };
        let film_h = (rect.height() * 0.21).clamp(24.0, 38.0);
        let film = Rect::from_min_size(rect.min, egui::vec2(rect.width(), film_h));
        let wave = Rect::from_min_max(
            egui::pos2(rect.left(), film.bottom() + 4.0),
            egui::pos2(rect.right(), rect.bottom() - bar_h - 6.0),
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
        mask_corners(&p, film, 6.0, PANEL);

        // 2. Waveform + slice bands + subtle unopinionated acoustic badges inside dark CRT well.
        p.rect_filled(wave, r(6), WELL);
        let playing = self.chop.playing.filter(|(s, _, t)| *s == src && now - t < 3.0).map(|(_, i, _)| i);
        let selected = self.chop.selected_slice.filter(|(s, _)| *s == src).map(|(_, i)| i);
        let badges: Vec<String> = {
            let pp = host.project().lock().unwrap();
            match pp.sources.get(src) {
                Some(so) => slices
                    .iter()
                    .map(|sl| {
                        let b = sl.badge();
                        if !b.is_empty() {
                            b
                        } else {
                            acoustic_slice_badge(so, sl).to_string()
                        }
                    })
                    .collect(),
                None => vec![String::new(); slices.len()],
            }
        };
        for (i, sl) in slices.iter().enumerate() {
            let band = Rect::from_min_max(egui::pos2(x_of(sl.start), wave.top()), egui::pos2(x_of(sl.end), wave.bottom()));
            let is_play = playing == Some(i);
            let is_sel = selected == Some(i);
            let fill = if is_play {
                Color32::from_rgb(0x3a, 0x35, 0x1e)
            } else if is_sel {
                WELL3
            } else if i % 2 == 0 {
                WELL
            } else {
                WELL2
            };
            p.rect_filled(band, 0.0, fill);
            let num_r = p.text(
                band.left_top() + egui::vec2(5.0, 4.0),
                Align2::LEFT_TOP,
                (i + 1).to_string(),
                mono(9.5),
                if is_play || is_sel { AMBER_MID } else { CREAM2 },
            );
            let badge = badges.get(i).map(String::as_str).unwrap_or("");
            if !badge.is_empty() && band.width() >= 42.0 {
                p.text(
                    egui::pos2(num_r.right() + 5.0, band.top() + 4.5),
                    Align2::LEFT_TOP,
                    badge,
                    mono_medium(8.5),
                    if is_play || is_sel { AMBER_CORE } else { MINT_MID },
                );
            }
            let cap_s = self
                .slice_captions
                .get(&(src, i))
                .map(|c| c.as_str())
                .filter(|c| !c.trim().is_empty())
                .or_else(|| (!sl.caption.is_empty()).then(|| sl.caption.as_str()));
            if let Some(cap) = cap_s {
                if band.width() >= 52.0 {
                    p.text(
                        band.left_bottom() + egui::vec2(5.0, -4.0),
                        Align2::LEFT_BOTTOM,
                        short(cap.trim(), ((band.width() - 10.0) / 5.5).max(3.0) as usize),
                        font_medium(9.0),
                        CREAM,
                    );
                }
            }
        }
        let peaks = {
            let pp = host.project().lock().unwrap();
            self.waves.get(&pp.sources, src)
        };
        if let Some(pk) = peaks {
            pk.draw(&p, wave.shrink2(egui::vec2(0.0, 10.0)), 0.0, len, Color32::from_rgb(0xe8, 0xd8, 0x98));
        }

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
            p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(1.0, Color32::from_white_alpha(85)));
        }

        if let Some(tc) = self.chop.tap.as_ref().filter(|tc| tc.source == src) {
            let x = x_of((now - tc.t0).clamp(0.0, len));
            p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(2.0, AMBER_MID));
        } else if let Some((s, i, t)) = self.chop.playing {
            if s == src {
                if let Some(sl) = slices.get(i) {
                    let at = sl.start + (now - t);
                    if at < sl.end {
                        let x = x_of(at);
                        p.line_segment([egui::pos2(x, span_top), egui::pos2(x, span_bot)], Stroke::new(1.8, AMBER_MID));
                    }
                }
            }
        }
        mask_corners(&p, wave, 6.0, PANEL);
        monitor_bezel(&p, wave, 6);

        let mut new_slices: Option<Vec<Slice>> = None;
        let mut on_edge = false;
        for (k, (i, end, t)) in edges.iter().enumerate() {
            let x = x_of(*t);
            let grip = Rect::from_center_size(egui::pos2(x, wave.bottom() - 8.0), egui::vec2(5.0, 14.0));
            p.rect_filled(grip, r(2), AMBER_MID);
            let resp = hit(
                ui,
                Rect::from_center_size(grip.center(), egui::vec2(26.0, 30.0)),
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
                        self.chop.selected_slice = None;
                    }
                } else if resp.clicked() {
                    match under {
                        Some(i) => {
                            let (a, b) = (slices[i].start, slices[i].end);
                            host.audition(src, a, b);
                            self.chop.playing = Some((src, i, now));
                            self.chop.selected_slice = Some((src, i));
                            if let Some(cap) = self.slice_captions.get(&(src, i)).filter(|c| !c.trim().is_empty()) {
                                self.active_caption = Some((cap.trim().to_string(), now + (b - a).max(0.8)));
                            }
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
        let compact = bar.width() < 980.0;
        let seg_w = if compact { 188.0 } else { 228.0 };
        let seg = Rect::from_min_size(bar.min, egui::vec2(seg_w, bar.height()));
        if let Some(i) = segmented_named(
            ui,
            seg,
            ui.id().with(("slice-by", src)),
            &["Auto", "4", "8", "16"],
            &["slice auto", "slice 4", "slice 8", "slice 16"],
            &[],
            usize::MAX,
            font(if compact { 11.5 } else { 12.0 }),
            false,
        ) {
            let v = match i {
                0 => host
                    .project()
                    .lock()
                    .unwrap()
                    .sources
                    .get(src)
                    .map(|s| chromagrain_core::chop::analyze_source_slices(s, 16))
                    .unwrap_or_default(),
                1 => chop::equal_slices(len, 4),
                2 => chop::equal_slices(len, 8),
                _ => chop::equal_slices(len, 16),
            };
            if !v.is_empty() {
                self.set_slices(host, src, v);
            }
        }

        let tapping = self.chop.tap.as_ref().is_some_and(|t| t.source == src);
        let tap_w = if tapping { 118.0 } else if compact { 72.0 } else { 88.0 };
        let tb = Rect::from_min_size(egui::pos2(seg.right() + 6.0, bar.top()), egui::vec2(tap_w, bar.height()));
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
            let sb = Rect::from_min_size(egui::pos2(tb.right() + 5.0, bar.top()), egui::vec2(62.0, bar.height()));
            left_end = sb.right();
            if button_named(ui, sb, ui.id().with(("chop-tap-done", src)), "Done", "chop tap done", false, None, None).clicked() {
                self.finish_tap(host);
            }
        }

        // Right: `Slices → pads` (single clip), `Vocal Kit → pads` (`"ukg vox kit"`),
        // `Kit → pads` (multi-clip harvester), `Loops → pads` (unsynced tape loops), and `Clear`.
        let has_pads = host.caps().views.contains(&super::View::Perform) && host.perform().is_some();
        let n = self.slices(host, src).len();
        let mut rx = bar.right();
        if has_pads {
            let sl_w = if compact { 108.0 } else { 132.0 };
            let pb = Rect::from_min_size(egui::pos2(rx - sl_w, bar.top()), egui::vec2(sl_w, bar.height()));
            rx = pb.left() - 4.5;
            if button_named(ui, pb, ui.id().with(("to-pads", src)), "Slices → pads", "slices to pads", n > 0, Some(Icon::Grid), None).clicked()
                && n > 0
            {
                let k = self.slices_to_pads(host, src);
                host.status(format!("{k} slices on pads 1–{k}"));
                self.finish_tap(host);
                host.stop_audition();
                self.sheet = None;
                self.set_view(host, super::View::Perform);
                return;
            }

            // `Vocal Kit → pads` (`a11y: "ukg vox kit"`): UKG vocal percussion & chord harvester!
            let vox_lbl = if compact { "Vocal Kit" } else { "Vocal Kit → pads" };
            let vox_w = if compact { 80.0 } else { 134.0 };
            if rx - vox_w > left_end + 6.0 {
                let vb = Rect::from_min_size(egui::pos2(rx - vox_w, bar.top()), egui::vec2(vox_w, bar.height()));
                rx = vb.left() - 4.5;
                if button_named(ui, vb, ui.id().with("ukg-vox-kit"), vox_lbl, "ukg vox kit", true, None, None).clicked() {
                    let k = self.trigger_ukg_kit(host);
                    if k > 0 {
                        self.finish_tap(host);
                        host.stop_audition();
                        self.sheet = None;
                        self.set_view(host, super::View::Perform);
                        return;
                    }
                }
            }

            // `Chords → pads` (`a11y: "chords to pads"`): 1-click ambient video-to-chord wash progression!
            let chd_lbl = if compact { "Chords" } else { "Chords → pads" };
            let chd_w = if compact { 70.0 } else { 126.0 };
            if rx - chd_w > left_end + 6.0 {
                let cb = Rect::from_min_size(egui::pos2(rx - chd_w, bar.top()), egui::vec2(chd_w, bar.height()));
                rx = cb.left() - 4.5;
                if button_named(ui, cb, ui.id().with("chords-to-pads"), chd_lbl, "chords to pads", false, None, None).clicked() {
                    let k = self.trigger_chord_wash(host);
                    if k > 0 {
                        self.finish_tap(host);
                        host.stop_audition();
                        self.sheet = None;
                        self.set_view(host, super::View::Perform);
                        return;
                    }
                }
            }

            // `Kit → pads` + `Loops → pads` (multi-clip harvester across standalone & plugin).
            let kit_w = if compact { 82.0 } else { 98.0 };
            if rx - kit_w > left_end + 6.0 {
                let kb = Rect::from_min_size(egui::pos2(rx - kit_w, bar.top()), egui::vec2(kit_w, bar.height()));
                rx = kb.left() - 4.5;
                if button_named(ui, kb, ui.id().with("kit-to-pads"), "Kit → pads", "kit to pads", false, None, None).clicked() {
                    let k = host.build_kit_to_pads();
                    if k > 0 {
                        let specs = {
                            let pp = host.project().lock().unwrap();
                            chromagrain_core::chop::build_kit_across_sources_opt(&pp.sources, Some(self.key))
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
                        host.status(format!("harvested {k}-pad kit across loaded clips"));
                        self.finish_tap(host);
                        host.stop_audition();
                        self.sheet = None;
                        self.set_view(host, super::View::Perform);
                        return;
                    }
                }
            }

            let lp_w = if compact { 100.0 } else { 114.0 };
            if rx - lp_w > left_end + 6.0 {
                let lb = Rect::from_min_size(egui::pos2(rx - lp_w, bar.top()), egui::vec2(lp_w, bar.height()));
                rx = lb.left() - 5.0;
                if button_named(ui, lb, ui.id().with("loops-to-pads"), "Loops → pads", "loops to pads", false, None, None).clicked() {
                    let k = host.build_loops_to_pads();
                    if k > 0 {
                        let specs = {
                            let pp = host.project().lock().unwrap();
                            chromagrain_core::chop::build_tape_loops_across_sources(&pp.sources, Some(self.key))
                        };
                        for spc in &specs {
                            let pad = spc.pad_index.min(15);
                            self.pad_sculpt[pad] = crate::host::SculptParams::from_core(spc.stamp, spc.sculpt, 0);
                            self.pad_captions[pad].clear();
                        }
                        host.status(format!("dealt {k} unsynced tape loops onto pads"));
                        self.finish_tap(host);
                        host.stop_audition();
                        self.sheet = None;
                        self.set_view(host, super::View::Perform);
                        return;
                    }
                }
            }
        }
        if n > 0 {
            let clr_w = if compact { 56.0 } else { 68.0 };
            if rx - clr_w >= left_end + 6.0 {
                let cb = Rect::from_min_size(egui::pos2(rx - clr_w, bar.top()), egui::vec2(clr_w, bar.height()));
                rx = cb.left() - 8.0;
                if button_named(ui, cb, ui.id().with(("chop-clear", src)), "Clear", "clear slices", false, None, None).clicked() {
                    self.set_slices(host, src, Vec::new());
                }
            }
            if rx > left_end + 52.0 {
                p.text(
                    egui::pos2(rx, bar.center().y),
                    Align2::RIGHT_CENTER,
                    if n == 1 { "1 slice".to_string() } else { format!("{n} slices") },
                    mono(10.5),
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
