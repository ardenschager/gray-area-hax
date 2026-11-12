//! The clip sheet (long-press a grid column header, a lane header or a
//! pad; or Chop on a just-added card): Koala-style chopping on a full-width
//! waveform + filmstrip, slices → pads, and for pads / lanes the A/D/S/R
//! envelope over the clip's own thumbnail with the picture's cut / fade.

use super::add::{modal, sheet_header};
use super::{Sheet, Studio};
use crate::atlas;
use crate::envelope::adsr_editor;
use crate::host::Host;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::chop::{self, Edge, Slice};
use chromagrain_core::envelope::{Adsr, PictureCut};
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// What the clip sheet edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTarget {
    /// A source on its own (a just-added card).
    Source(usize),
    /// The clip in grid column N.
    Column(usize),
    Lane(usize),
    Pad(usize),
}

/// Height the chop panel is laid out for (waveform, filmstrip, bar).
pub const CHOP_PANEL_H: f32 = 296.0;

/// Whose envelope the envelope panel edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvTarget {
    Pad(usize),
    Lane(usize),
    Emitter(usize),
}

impl ClipTarget {
    /// The envelope this clip target carries (pads and lanes).
    pub fn env_target(self) -> Option<EnvTarget> {
        match self {
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

impl Studio {
    pub fn clip_source(&self, host: &mut dyn Host, t: ClipTarget) -> Option<usize> {
        match t {
            ClipTarget::Source(s) => Some(s),
            ClipTarget::Column(c) => self.columns.get(c).copied().flatten(),
            ClipTarget::Lane(l) => self.seq.lanes.get(l).copied().flatten(),
            ClipTarget::Pad(i) => host.perform().and_then(|p| p.pads().get(i).and_then(|pd| pd.source)),
        }
    }

    pub fn open_clip(&mut self, t: ClipTarget) {
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

    pub fn clip_sheet(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let Some(Sheet::Clip(target)) = self.sheet else { return };
        let Some(src) = self.clip_source(host, target) else {
            self.sheet = None;
            return;
        };
        let env_target = target.env_target().is_some();
        let screen = ctx.screen_rect();
        let size = egui::vec2((screen.width() - 40.0).min(980.0), (screen.height() - 48.0).min(if env_target { 640.0 } else { 470.0 }));
        let now = host.clock();
        let mut close = false;
        let scrim = modal(ctx, "cg-clip", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(22.0, 18.0));
            let id = egui::Id::new("cg-clip");
            let name = self.source_name(host, src);
            let len = host.project().lock().unwrap().sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
            let slices = self.slices(host, src);
            let _ = now;
            let prov = self.provenance_line(host, src);
            let sub = format!(
                "{} · {}:{:02}{}",
                match slices.len() {
                    0 => "no slices yet".to_string(),
                    1 => "1 slice".to_string(),
                    n => format!("{n} slices"),
                },
                (len / 60.0) as u32,
                len as u32 % 60,
                if prov.is_empty() { String::new() } else { format!(" · {prov}") }
            );
            if sheet_header(ui, inner, &name, &sub, id) {
                close = true;
            }
            let chop = Rect::from_min_size(egui::pos2(inner.left(), inner.top() + 60.0), egui::vec2(inner.width(), CHOP_PANEL_H));
            self.chop_panel(host, ui, chop, src);
            if let Some(et) = target.env_target() {
                let env = Rect::from_min_max(egui::pos2(inner.left(), chop.bottom() + 22.0), inner.max);
                self.envelope_panel(host, ui, env, et, Some(src));
                if let EnvTarget::Lane(l) = et {
                    let more = Rect::from_min_size(egui::pos2(env.right() - 200.0, env.bottom() - 40.0), egui::vec2(200.0, 40.0));
                    if button(ui, more, ui.id().with(("lane-more", l)), "Lane settings", false, Some(Icon::Deep), None).clicked() {
                        self.sheet = None;
                        self.deep = Some((super::DeepTarget::Lane(l), super::DeepTab::Links));
                    }
                }
            }
        });
        if (close || scrim) && matches!(self.sheet, Some(Sheet::Clip(_))) {
            if self.chop.tap.is_some() {
                self.finish_tap(host);
            }
            host.stop_audition();
            self.sheet = None;
        }
    }

    /// The chop panel, self-contained (any host rect, `CHOP_PANEL_H` tall
    /// looks best): waveform + filmstrip with slices, then auto / 4 / 8 /
    /// 16, Tap, Clear, Slices → pads.
    pub fn chop_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, src: usize) {
        let now = host.clock();
        let len = host.project().lock().unwrap().sources.get(src).map(|s| s.duration()).unwrap_or(0.0);
        let slices = self.slices(host, src);
        let bar_h = 44.0;
        let film_h = (rect.height() * 0.24).clamp(36.0, 62.0);
        let wave = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - bar_h - 16.0 - film_h - 6.0));
        let film = Rect::from_min_size(egui::pos2(rect.left(), wave.bottom() + 6.0), egui::vec2(rect.width(), film_h));
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
        let x_of = |t: f64| wave.left() + (t / len) as f32 * wave.width();
        let t_of = |x: f32| ((x - wave.left()) / wave.width()).clamp(0.0, 1.0) as f64 * len;
        p.rect_filled(wave, r(10), G0);
        // Bands: alternating subtle fills; the auditioned slice brighter.
        let playing = self.chop.playing.filter(|(s, _, t)| *s == src && now - t < 3.0).map(|(_, i, _)| i);
        for (i, sl) in slices.iter().enumerate() {
            let band = Rect::from_min_max(egui::pos2(x_of(sl.start), wave.top()), egui::pos2(x_of(sl.end), wave.bottom()));
            let fill = if playing == Some(i) { G4 } else if i % 2 == 0 { G2 } else { G3 };
            p.rect_filled(band, 0.0, fill);
            p.text(band.left_top() + egui::vec2(6.0, 5.0), Align2::LEFT_TOP, (i + 1).to_string(), mono(10.0), if playing == Some(i) { INK } else { INK3 });
        }
        let peaks = {
            let pp = host.project().lock().unwrap();
            self.waves.get(&pp.sources, src)
        };
        if let Some(pk) = peaks {
            pk.draw(&p, wave.shrink2(egui::vec2(0.0, 16.0)), 0.0, len, INK2);
        }
        // Filmstrip of the same span.
        let frames = atlas::FRAMES;
        if let Some((tex, _)) = self.atlas(host, &ctx, src) {
            let fw = film.width() / frames as f32;
            for k in 0..frames {
                let fr = Rect::from_min_size(egui::pos2(film.left() + k as f32 * fw, film.top()), egui::vec2(fw + 0.5, film.height()));
                p.image(tex, fr, atlas::uv_cover(k, fw / film.height()), gray(0.85));
            }
        } else {
            p.rect_filled(film, 0.0, G2);
        }
        for (i, sl) in slices.iter().enumerate() {
            if i % 2 == 1 {
                let band = Rect::from_min_max(egui::pos2(x_of(sl.start), film.top()), egui::pos2(x_of(sl.end), film.bottom()));
                p.rect_filled(band, 0.0, Color32::from_black_alpha(70));
            }
        }
        mask_corners(&p, film, 8.0, G1);
        // Edges: hairlines, with grips under the waveform.
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
            p.line_segment([egui::pos2(x, wave.top()), egui::pos2(x, film.bottom())], Stroke::new(1.0, Color32::from_white_alpha(40)));
        }
        // The playhead (tap-chop / audition).
        if let Some(tc) = self.chop.tap.as_ref().filter(|tc| tc.source == src) {
            let x = x_of((now - tc.t0).clamp(0.0, len));
            p.line_segment([egui::pos2(x, wave.top()), egui::pos2(x, film.bottom())], Stroke::new(2.0, HI));
        } else if let Some((s, i, t)) = self.chop.playing {
            if s == src {
                if let Some(sl) = slices.get(i) {
                    let at = sl.start + (now - t);
                    if at < sl.end {
                        let x = x_of(at);
                        p.line_segment([egui::pos2(x, wave.top()), egui::pos2(x, film.bottom())], Stroke::new(1.5, HI));
                    }
                }
            }
        }
        mask_corners(&p, wave, 10.0, G1);
        p.rect_stroke(wave, r(10), hair(LINE2), StrokeKind::Inside);

        // Edge drags first (44 px grips straddling each edge under the wave).
        let mut new_slices: Option<Vec<Slice>> = None;
        let mut on_edge = false;
        for (k, (i, end, t)) in edges.iter().enumerate() {
            let x = x_of(*t);
            let grip = Rect::from_center_size(egui::pos2(x, wave.bottom() - 10.0), egui::vec2(6.0, 20.0));
            p.rect_filled(grip, r(3), INK2);
            let resp = hit(ui, Rect::from_center_size(grip.center(), egui::vec2(28.0, 44.0)), ui.id().with(("edge", src, k)), Sense::drag(), &format!("slice {} {}", i + 1, if *end { "end" } else { "start" }));
            if resp.hovered() || resp.dragged() {
                on_edge = true;
            }
            if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                let mut v = slices.to_vec();
                chop::move_boundary(&mut v, *i, if *end { Edge::End } else { Edge::Start }, t_of(pos.x), len);
                new_slices = Some(v);
            }
        }
        // Taps on the wave / filmstrip: audition (tap mode: chop), double
        // tap deletes the slice.
        let area = Rect::from_min_max(wave.min, film.max);
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
        // Slice by: auto / 4 / 8 / 16.
        let seg = Rect::from_min_size(bar.min, egui::vec2(268.0, bar.height()));
        if let Some(i) = segmented_named(ui, seg, ui.id().with(("slice-by", src)), &["Auto", "4", "8", "16"], &["slice auto", "slice 4", "slice 8", "slice 16"], &[], usize::MAX, font(13.0), false) {
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
        let tb = Rect::from_min_size(egui::pos2(seg.right() + 10.0, bar.top()), egui::vec2(if tapping { 150.0 } else { 104.0 }, bar.height()));
        let label = if tapping { "Chop here" } else { "Tap" };
        if button_named(ui, tb, ui.id().with(("chop-tap", src)), label, "chop tap", tapping, Some(Icon::Chop), None).clicked() {
            if tapping {
                self.tap_chop(host, now);
            } else {
                // Start over: the clip plays from the top; every tap chops.
                self.set_slices(host, src, Vec::new());
                host.audition(src, 0.0, len);
                self.chop.tap = Some(TapChop { source: src, t0: now, len });
                self.chop.playing = None;
            }
        }
        if tapping {
            let sb = Rect::from_min_size(egui::pos2(tb.right() + 8.0, bar.top()), egui::vec2(80.0, bar.height()));
            if button_named(ui, sb, ui.id().with(("chop-tap-done", src)), "Done", "chop tap done", false, None, None).clicked() {
                self.finish_tap(host);
            }
        }
        // Right: clear, slices → pads.
        let has_pads = host.caps().views.contains(&super::View::Perform) && host.perform().is_some();
        let n = self.slices(host, src).len();
        let mut rx = bar.right();
        if has_pads {
            let pb = Rect::from_min_size(egui::pos2(rx - 160.0, bar.top()), egui::vec2(160.0, bar.height()));
            rx = pb.left() - 8.0;
            let primary = n > 0;
            if button_named(ui, pb, ui.id().with(("to-pads", src)), "Slices → pads", "slices to pads", primary, Some(Icon::Grid), None).clicked() && n > 0 {
                let k = self.slices_to_pads(host, src);
                host.status(format!("{k} slices on pads 1–{k}"));
            }
        }
        if n > 0 {
            let cb = Rect::from_min_size(egui::pos2(rx - 84.0, bar.top()), egui::vec2(84.0, bar.height()));
            if button_named(ui, cb, ui.id().with(("chop-clear", src)), "Clear", "clear slices", false, None, None).clicked() {
                self.set_slices(host, src, Vec::new());
            }
        }
        let _ = p;
    }

    /// The envelope panel, self-contained: the 4-handle A/D/S/R editor over
    /// the clip's thumbnail (`thumb_src`) and the picture's Cut | Fade.
    pub fn envelope_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect, target: EnvTarget, thumb_src: Option<usize>) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        caps(&p, egui::pos2(area.left(), area.top() + 6.0), Align2::LEFT_CENTER, "Envelope — sound and picture", 10.5);
        let Some(mut adsr) = self.target_env(host, target) else { return };
        let side_w = 230.0f32.min(area.width() * 0.36);
        let ew = area.width() - side_w - 18.0;
        let er = Rect::from_min_size(egui::pos2(area.left(), area.top() + 22.0), egui::vec2(ew, (area.height() - 26.0).clamp(80.0, 170.0)));
        let frame = match target {
            EnvTarget::Emitter(i) => atlas::frame_at(self.emitters[i].pos()),
            _ => 5,
        };
        let thumb = thumb_src.and_then(|s| self.atlas(host, &ctx, s)).map(|(tex, _)| (tex, atlas::uv_cover(frame, er.width() / er.height())));
        let mut changed = adsr_editor(ui, er, ui.id().with(("env", target)), &mut adsr, thumb);
        let side = Rect::from_min_max(egui::pos2(er.right() + 18.0, er.top()), egui::pos2(area.right(), er.bottom()));
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
    let note = if adsr.picture == PictureCut::Fade { "the picture fades with the envelope" } else { "full picture, then a hard cut with the sound" };
    p.text(egui::pos2(side.left(), seg.bottom() + 16.0), Align2::LEFT_CENTER, note, font(11.5), INK3);
    changed
}
