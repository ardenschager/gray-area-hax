//! Sequence: nanoloop-simple. One lane per clip, 16 steps; an active step
//! IS a thumbnail of the slice it triggers. Tap toggles, drag up/down sets
//! pitch (key degrees), hold opens the step sheet. Scenes 1–8 and a song
//! chain on the right (top strip when narrow).

use crate::atlas;
use crate::theme::*;
use crate::widgets::*;
use super::{DeepTab, DeepTarget, LANES, SCENES, STEPS};
use super::Studio;
use crate::host::Host;
use chromagrain_core::music::Key;
use chromagrain_core::seq::StepPattern;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// Semitones for key degree `d` (0 = root) of `key`'s scale.
pub fn degree_to_semis(key: &Key, d: i32) -> f32 {
    let iv = key.scale.intervals();
    let n = iv.len() as i32;
    (iv[d.rem_euclid(n) as usize] + 12 * d.div_euclid(n)) as f32
}

/// The nearest key degree for a semitone offset.
pub fn semis_to_degree(key: &Key, st: f32) -> i32 {
    (-28..=28).min_by(|a, b| (degree_to_semis(key, *a) - st).abs().total_cmp(&(degree_to_semis(key, *b) - st).abs())).unwrap_or(0)
}

const LENGTHS: [(f32, &str); 6] = [(0.5, "1/32"), (1.0, "1/16"), (2.0, "1/8"), (4.0, "1/4"), (8.0, "1/2"), (16.0, "1")];

impl Studio {
    // ------------------------------------------------------------ model

    /// Scene 1's pattern exists, and every scene pattern has one row per
    /// lane pointing at the lane's clip.
    pub fn seq_ensure(&mut self, host: &mut dyn Host) {
        let lanes = self.seq.lanes;
        if lanes.iter().all(|l| l.is_none()) {
            return;
        }
        let key = self.key;
        let bpm = host.project().lock().unwrap().bpm;
        let mut created = false;
        if self.seq.scene_pats[0].is_none() {
            let mut pat = StepPattern::new("scene 1");
            for lane in lanes.iter() {
                let r = pat.add_row(lane.unwrap_or(usize::MAX));
                pat.rows[r].key = Some(key);
                pat.rows[r].grains.duration = (60.0 / bpm as f32 / 4.0 * 1.5).clamp(0.05, 1.0);
            }
            let pid = host.project().lock().unwrap().add_pattern(pat);
            self.seq.scene_pats[0] = Some(pid);
            created = true;
        }
        let mut p = host.project().lock().unwrap();
        let mut changed = false;
        for pid in self.seq.scene_pats.iter().flatten() {
            let Some(pat) = p.patterns.get_mut(*pid) else { continue };
            while pat.rows.len() < LANES {
                pat.add_row(usize::MAX);
            }
            for (i, lane) in lanes.iter().enumerate() {
                let want = lane.unwrap_or(usize::MAX);
                if pat.rows[i].source != want {
                    pat.rows[i].source = want;
                    changed = true;
                }
            }
        }
        drop(p);
        if created || changed {
            self.rev += 1;
            host.mark();
        }
    }

    pub fn seq_pattern(&self, scene: usize) -> Option<usize> {
        self.seq.scene_pats.get(scene).copied().flatten()
    }

    /// Chain entries with their start beats.
    fn seq_chain_layout(&self) -> Vec<(usize, u32, f64)> {
        let mut b = 0.0;
        self.seq
            .chain
            .iter()
            .filter(|(s, _)| self.seq_pattern(*s).is_some())
            .map(|&(s, n)| {
                let at = b;
                b += 4.0 * n as f64;
                (s, n, at)
            })
            .collect()
    }

    /// The scene playing at the playhead (or being edited, when stopped).
    pub fn seq_current(&self, host: &dyn Host) -> (usize, Option<(usize, f64)>) {
        let lay = self.seq_chain_layout();
        let b = host.playhead_beats();
        for (i, (s, n, at)) in lay.iter().enumerate() {
            if b >= *at && b < at + 4.0 * *n as f64 {
                if host.playing() {
                    return (*s, Some((i, (b - at) / (4.0 * *n as f64))));
                }
                return (self.seq.scene, Some((i, (b - at) / (4.0 * *n as f64))));
            }
        }
        (self.seq.scene, None)
    }

    /// Lay the song chain out as pattern clips on the "sequence" track and
    /// loop over it.
    pub fn seq_build_arrangement(&mut self, host: &mut dyn Host) {
        if self.seq.built_rev == self.rev {
            return;
        }
        let lay: Vec<(usize, f64, f64)> = self
            .seq_chain_layout()
            .iter()
            .filter_map(|(s, n, at)| Some((self.seq.scene_pats[*s]?, *at, 4.0 * *n as f64)))
            .collect();
        if lay.is_empty() {
            return;
        }
        if host.sequence_changed(&lay) {
            self.seq.built_rev = self.rev;
        }
    }

    /// Queued scenes start on the next bar.
    pub fn seq_tick(&mut self, host: &mut dyn Host) {
        let Some(q) = self.seq.queued else { return };
        if !host.playing() {
            self.seq_jump(host, q);
            return;
        }
        let bar = (host.playhead_beats() / 4.0).floor();
        let prev = self.bar_memo.replace(bar);
        if prev.is_some_and(|pb| pb != bar) {
            self.seq_jump(host, q);
        }
    }

    fn seq_jump(&mut self, host: &mut dyn Host, scene: usize) {
        self.seq.queued = None;
        self.seq.scene = scene;
        self.bar_memo = None;
        if let Some((_, _, at)) = self.seq_chain_layout().into_iter().find(|(s, _, _)| *s == scene) {
            host.seek(at);
        }
    }

    /// The most recent hit at or before the playhead in the current scene
    /// (source, media seconds): the Sequence monitor holds it between hits.
    pub fn seq_last_hit(&self, host: &dyn Host) -> Option<(usize, f64)> {
        let (scene, at) = self.seq_current(host);
        let pid = self.seq_pattern(scene)?;
        let lay = self.seq_chain_layout();
        let start = at.and_then(|(i, _)| lay.get(i)).map(|e| e.2).unwrap_or(0.0);
        let slot = ((host.playhead_beats() - start).max(0.0) * 4.0).floor() as i64;
        let p = host.project().lock().unwrap();
        let pat = p.patterns.get(pid)?;
        let mut best: Option<(i64, usize, f32)> = None;
        for row in pat.rows.iter().filter(|r| !r.muted) {
            let Some(src) = p.sources.get(row.source) else { continue };
            let n = row.steps.len() as i64;
            if n == 0 || src.duration() <= 0.0 {
                continue;
            }
            // Look back up to one bar for this lane's latest hit.
            for back in 0..16i64 {
                let k = slot - back;
                let st = row.steps[k.rem_euclid(n) as usize];
                if st.on {
                    if best.is_none_or(|b| back < b.0) {
                        let pos = if st.slice >= 0.0 { st.slice } else { row.grains.position };
                        best = Some((back, row.source, pos));
                    }
                    break;
                }
            }
        }
        let (_, src, pos) = best?;
        let len = p.sources[src].duration();
        Some((src, pos as f64 * len))
    }

    pub fn seq_step(&self, host: &dyn Host, scene: usize, lane: usize, step: usize) -> Option<chromagrain_core::seq::Step> {
        let pid = self.seq_pattern(scene)?;
        let p = host.project().lock().unwrap();
        p.patterns.get(pid)?.rows.get(lane)?.steps.get(step).copied()
    }

    pub fn seq_edit_step(&mut self, host: &mut dyn Host, scene: usize, lane: usize, step: usize, f: impl FnOnce(&mut chromagrain_core::seq::Step)) {
        let Some(pid) = self.seq_pattern(scene) else { return };
        let mut p = host.project().lock().unwrap();
        if let Some(st) = p.patterns.get_mut(pid).and_then(|pat| pat.rows.get_mut(lane)).and_then(|r| r.steps.get_mut(step)) {
            f(st);
        }
        drop(p);
        host.mark();
    }

    /// Tap: toggle a step. Turning one on picks the slice under it.
    pub fn seq_toggle(&mut self, host: &mut dyn Host, scene: usize, lane: usize, step: usize) {
        self.seq_edit_step(host, scene, lane, step, |s| {
            s.on = !s.on;
            if s.on && s.slice < 0.0 {
                s.slice = (step as f32 + 0.5) / STEPS as f32;
            }
        });
    }

    fn lane_len(&self, host: &dyn Host, scene: usize, lane: usize) -> usize {
        let Some(pid) = self.seq_pattern(scene) else { return STEPS };
        let p = host.project().lock().unwrap();
        p.patterns.get(pid).and_then(|pat| pat.rows.get(lane)).map(|r| r.steps.len()).unwrap_or(STEPS)
    }

    pub fn lane_muted(&self, host: &dyn Host, scene: usize, lane: usize) -> bool {
        let Some(pid) = self.seq_pattern(scene) else { return false };
        let p = host.project().lock().unwrap();
        p.patterns.get(pid).and_then(|pat| pat.rows.get(lane)).is_some_and(|r| r.muted)
    }

    pub fn set_lane_len(&mut self, host: &mut dyn Host, lane: usize, n: usize) {
        let mut p = host.project().lock().unwrap();
        for pid in self.seq.scene_pats.iter().flatten() {
            if let Some(r) = p.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(lane)) {
                r.set_steps(n.clamp(1, 64));
            }
        }
        drop(p);
        host.mark();
    }

    fn toggle_lane_mute(&mut self, host: &mut dyn Host, lane: usize) {
        let mut p = host.project().lock().unwrap();
        for pid in self.seq.scene_pats.iter().flatten() {
            if let Some(r) = p.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(lane)) {
                r.muted = !r.muted;
            }
        }
        drop(p);
        host.mark();
    }

    /// Tap a scene: queue it; an empty scene copies the current one.
    pub fn seq_scene_tap(&mut self, host: &mut dyn Host, s: usize) {
        if self.seq_pattern(s).is_none() {
            let (cur, _) = self.seq_current(host);
            let Some(src) = self.seq_pattern(cur) else { return };
            let mut p = host.project().lock().unwrap();
            let mut pat = p.patterns[src].clone();
            pat.name = format!("scene {}", s + 1);
            let pid = p.add_pattern(pat);
            drop(p);
            self.seq.scene_pats[s] = Some(pid);
            self.seq.chain.push((s, 2));
            self.rev += 1;
        }
        self.seq.scene = s;
        self.seq.queued = Some(s);
    }

    // ------------------------------------------------------------ view

    pub fn sequence_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1100.0;
        let (lanes_rect, song) = if narrow {
            let strip_h = (area.height() * 0.17).clamp(70.0, 110.0);
            let song = Rect::from_min_size(area.min, egui::vec2(area.width(), strip_h));
            (Rect::from_min_max(egui::pos2(area.left(), song.bottom() + 16.0), area.max), song)
        } else {
            let rw = (area.width() * 0.225).clamp(260.0, 380.0);
            let song = Rect::from_min_max(egui::pos2(area.right() - rw, area.top()), area.max);
            (Rect::from_min_max(area.min, egui::pos2(song.left() - 24.0, area.bottom())), song)
        };
        self.seq_lanes(host, ui, lanes_rect);
        if narrow {
            self.song_strip(host, ui, song);
        } else {
            self.song_column(host, ui, song);
        }
        self.step_sheet(host, ui);
    }

    fn seq_lanes(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = 4.0;
        let lh = (area.height() - g * (LANES as f32 - 1.0)) / LANES as f32;
        let hw = (area.width() * 0.16).clamp(70.0, 200.0);
        let sx = area.left() + hw + if area.width() < 900.0 { 10.0 } else { 12.0 };
        let group = if area.width() < 900.0 { 4.0 } else { 8.0 };
        let sw = (area.right() - sx - g * 15.0 - group * 3.0) / STEPS as f32;
        let step_x = |s: usize| sx + s as f32 * (sw + g) + (s / 4) as f32 * group;
        let (scene, playing_at) = self.seq_current(host);
        let playing = host.playing();
        let key = self.key;
        let lay = self.seq_chain_layout();
        let clip_start = playing_at.and_then(|(i, _)| lay.get(i)).map(|e| e.2).unwrap_or(0.0);
        let slot = ((host.playhead_beats() - clip_start).max(0.0) * 4.0).floor() as usize;
        let mut first_empty = true;
        for lane in 0..LANES {
            let y = area.top() + lane as f32 * (lh + g);
            let header = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(hw, lh));
            let src = self.seq.lanes[lane];
            let Some(src) = src else {
                // Empty lane: the first one is the "drop a video" slot.
                p.rect_filled(header, r(8), G1);
                p.rect_stroke(header, r(8), hair(LINE), StrokeKind::Inside);
                if first_empty {
                    first_empty = false;
                    let resp = hit(ui, header, ui.id().with(("lane-add", lane)), Sense::click(), "add a lane");
                    draw_icon(&p, Icon::Plus, header.center(), 16.0, if resp.hovered() { INK2 } else { INK3 });
                    if resp.clicked() {
                        host.import_media();
                    }
                }
                for s in 0..STEPS {
                    let c = Rect::from_min_size(egui::pos2(step_x(s), y), egui::vec2(sw, lh));
                    p.rect_stroke(c, r(6), hair(LINE), StrokeKind::Inside);
                }
                continue;
            };
            let len = self.lane_len(host, scene, lane);
            let muted = self.lane_muted(host, scene, lane);
            let steps: Vec<chromagrain_core::seq::Step> = (0..len).filter_map(|s| self.seq_step(host, scene, lane, s)).collect();
            let silent = steps.iter().all(|s| !s.on);
            let at = self.atlas(host, &ctx, src);
            // Header: the clip itself.
            if let Some((tex, a)) = at {
                tile(&p, header, 8, tex, a.uv_cover(5, hw / lh), gray(if muted { 0.28 } else if silent { 0.5 } else { 0.86 }));
                p.rect_filled(Rect::from_min_size(header.min, egui::vec2(3.0, lh)), r(2), a.swatch);
            }
            p.rect_stroke(header, r(8), hair(LINE), StrokeKind::Inside);
            if len != STEPS {
                p.text(header.right_bottom() + egui::vec2(-8.0, -6.0), Align2::RIGHT_BOTTOM, len.to_string(), mono(10.0), INK2);
            }
            if muted {
                p.text(header.left_bottom() + egui::vec2(10.0, -6.0), Align2::LEFT_BOTTOM, "muted", font(10.0), INK2);
            }
            let hresp = hit(ui, header, ui.id().with(("lane-head", lane)), Sense::click(), &format!("lane {}", lane + 1));
            let h = hold(ui, &hresp, 0.45);
            if h.fired {
                self.deep = Some((DeepTarget::Lane(lane), DeepTab::Links));
            } else if hresp.clicked() && !h.held_before {
                self.toggle_lane_mute(host, lane);
            }
            let cur = if playing || host.playhead_beats() > 0.0 { Some(slot % len.max(1)) } else { None };
            for s in 0..STEPS {
                let c = Rect::from_min_size(egui::pos2(step_x(s), y), egui::vec2(sw, lh));
                if s >= len {
                    if s == len {
                        draw_loop_glyph(&p, egui::pos2(c.left() + 10.0, c.center().y), INK4);
                    }
                    continue;
                }
                let st = steps.get(s).copied().unwrap_or_default();
                let ghost = if muted { 0.35 } else { 1.0 };
                if st.on {
                    if let Some((tex, a)) = at {
                        let slice = if st.slice >= 0.0 { atlas::frame_at(st.slice) } else { s };
                        tile(&p, c, 6, tex, a.uv_cover(slice, sw / lh), gray(0.9 * ghost));
                    }
                    if st.pitch.abs() > 0.01 {
                        let d = semis_to_degree(&key, st.pitch);
                        let t = format!("{d:+}");
                        let lr = p.text(c.left_top() + egui::vec2(5.0, 4.0), Align2::LEFT_TOP, &t, mono(10.0), INK);
                        let _ = lr;
                    }
                    if st.chance < 0.999 {
                        p.text(c.right_top() + egui::vec2(-5.0, 4.0), Align2::RIGHT_TOP, format!("{:.0}%", st.chance * 100.0), mono(9.0), INK2);
                    }
                } else {
                    p.rect_filled(c, r(6), G1);
                    p.circle_filled(c.center(), 1.5, if s % 4 == 0 { INK3 } else { INK4 });
                }
                if cur == Some(s) {
                    p.rect_stroke(c, r(6), Stroke::new(1.5, HI), StrokeKind::Inside);
                }
                if self.seq.hold == Some((lane, s)) {
                    self.hold_rect = Some(c);
                    p.rect_stroke(c.expand(1.0), r(7), Stroke::new(2.0, HI), StrokeKind::Inside);
                }
                let resp = hit(ui, c, ui.id().with(("step", lane, s)), Sense::click_and_drag(), &format!("lane {} step {}", lane + 1, s + 1));
                let h = hold(ui, &resp, 0.45);
                if h.fired {
                    self.seq.hold = Some((lane, s));
                    if !st.on {
                        self.seq_toggle(host, scene, lane, s);
                    }
                } else if resp.clicked() && !h.held_before {
                    self.seq_toggle(host, scene, lane, s);
                } else if resp.dragged() && st.on {
                    let acc = ui.data(|d| d.get_temp::<f32>(resp.id).unwrap_or(0.0)) - resp.drag_delta().y;
                    let steps_moved = (acc / 14.0).trunc();
                    if steps_moved != 0.0 {
                        let d = semis_to_degree(&key, st.pitch) + steps_moved as i32;
                        let semis = degree_to_semis(&key, d.clamp(-14, 14));
                        self.seq_edit_step(host, scene, lane, s, |x| x.pitch = semis);
                        ui.data_mut(|dd| dd.insert_temp(resp.id, acc - steps_moved * 14.0));
                    } else {
                        ui.data_mut(|dd| dd.insert_temp(resp.id, acc));
                    }
                }
                if resp.drag_stopped() {
                    ui.data_mut(|dd| dd.remove::<f32>(resp.id));
                }
            }
        }
        if playing {
            ctx.request_repaint();
        }
    }

    fn scene_cell(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, s: usize, current: usize) {
        let p = ui.painter().clone();
        let exists = self.seq_pattern(s).is_some();
        let on = s == current;
        let resp = hit(ui, rect, ui.id().with(("scene", s)), Sense::click(), &format!("scene {}", s + 1));
        if on {
            p.rect_filled(rect, r(10), HI);
        } else if exists {
            raised(&p, rect, 10, if resp.hovered() { G3 } else { G2 });
        } else {
            p.rect_filled(rect, r(10), G1);
            p.rect_stroke(rect, r(10), hair(LINE), StrokeKind::Inside);
        }
        if self.seq.queued == Some(s) && !on {
            let t = ui.input(|i| i.time);
            if (t * 3.0).fract() < 0.5 {
                p.rect_stroke(rect, r(10), Stroke::new(1.5, HI), StrokeKind::Inside);
            }
            ui.ctx().request_repaint();
        }
        let col = if on { ON_HI } else if exists { INK } else { INK4 };
        let small = rect.height() < 50.0;
        p.text(rect.center() - egui::vec2(0.0, if small || !exists { 0.0 } else { 4.0 }), Align2::CENTER_CENTER, (s + 1).to_string(), mono(if small { 14.0 } else { 15.0 }), col);
        if exists && !small {
            // A tiny bar per lane: which lanes play in this scene.
            let pid = self.seq_pattern(s).unwrap();
            let pp = host.project().lock().unwrap();
            let rows: Vec<bool> = pp.patterns.get(pid).map(|pat| pat.rows.iter().map(|r| r.steps.iter().any(|s| s.on) && !r.muted).collect()).unwrap_or_default();
            drop(pp);
            let n = rows.len().max(1);
            let bw = ((rect.width() - 24.0) / n as f32 - 2.0).max(2.0);
            for (i, on_lane) in rows.iter().enumerate() {
                let x = rect.left() + 12.0 + i as f32 * (bw + 2.0);
                let c = if *on_lane { if on { Color32::from_black_alpha(140) } else { INK3 } } else if on { Color32::from_black_alpha(40) } else { G4 };
                p.rect_filled(Rect::from_min_size(egui::pos2(x, rect.bottom() - 10.0), egui::vec2(bw, 2.0)), r(1), c);
            }
        }
        if resp.clicked() {
            self.seq_scene_tap(host, s);
        }
    }

    fn chain_row(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        let lay = self.seq_chain_layout();
        let (_, at) = self.seq_current(host);
        let mut x = rect.left();
        let h = rect.height();
        for (i, (s, n, start)) in lay.iter().enumerate() {
            let label = format!("{} ×{}", s + 1, n);
            let w = p.layout_no_wrap(label.clone(), mono(12.0), INK).size().x + 16.0 + (*n as f32 - 1.0).min(3.0) * 5.0;
            if x + w > rect.right() - 16.0 {
                break;
            }
            let cr = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, h));
            let resp = hit(ui, cr, ui.id().with(("chain", i)), Sense::click(), &format!("chain {}", i + 1));
            let cur = at.map(|(ci, _)| ci) == Some(i);
            p.rect_filled(cr, r(8), if cur { G3 } else { G2 });
            if let (true, Some((_, prog))) = (cur, at) {
                let fill = Rect::from_min_size(cr.min, egui::vec2(cr.width() * prog as f32, h));
                p.rect_filled(fill, r(8), G4);
            }
            p.rect_stroke(cr, r(8), hair(if cur { LINE3 } else { LINE }), StrokeKind::Inside);
            runs(
                &p,
                egui::pos2(cr.left() + 11.0, cr.center().y),
                Align2::LEFT_CENTER,
                &[(&(s + 1).to_string(), mono_medium(12.0), if cur { INK } else { INK2 }), (&format!(" ×{n}"), mono(11.0), INK3)],
            );
            if resp.clicked() {
                host.seek(*start);
            }
            x = cr.right() + 5.0;
        }
        draw_loop_glyph(&p, egui::pos2(x + 8.0, rect.center().y), INK3);
    }

    fn song_column(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, col: Rect) {
        let stage = Rect::from_min_size(col.min, egui::vec2(col.width(), (col.width() * 9.0 / 16.0).floor()));
        self.stage(host, ui, stage, 12.0);
        if !host.caps().scenes {
            return; // one looping scene; the host arranges
        }
        let (cur, _) = self.seq_current(host);
        let g = 8.0;
        let sw = (col.width() - g * 3.0) / 4.0;
        let sh = 64.0;
        for s in 0..SCENES {
            let rc = Rect::from_min_size(egui::pos2(col.left() + (s % 4) as f32 * (sw + g), stage.bottom() + 14.0 + (s / 4) as f32 * (sh + g)), egui::vec2(sw, sh));
            self.scene_cell(host, ui, rc, s, cur);
        }
        let chain = Rect::from_min_size(egui::pos2(col.left(), stage.bottom() + 14.0 + 2.0 * (sh + g) + 8.0), egui::vec2(col.width(), 36.0));
        self.chain_row(host, ui, chain);
    }

    fn song_strip(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, strip: Rect) {
        let h = strip.height();
        let stage = Rect::from_min_size(strip.min, egui::vec2(h * 16.0 / 9.0, h));
        self.stage(host, ui, stage, 10.0);
        if !host.caps().scenes {
            return;
        }
        let (cur, _) = self.seq_current(host);
        let cell = (h * 0.47).clamp(36.0, 48.0);
        let y = strip.center().y - cell / 2.0;
        let mut x = stage.right() + 16.0;
        for s in 0..SCENES {
            let rc = Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell));
            self.scene_cell(host, ui, rc, s, cur);
            x += cell + 6.0;
        }
        let chain = Rect::from_min_max(egui::pos2(x + 10.0, strip.center().y - 18.0), egui::pos2(strip.right(), strip.center().y + 18.0));
        self.chain_row(host, ui, chain);
    }

    /// The step hold sheet: slice picker + pitch / length / chance.
    fn step_sheet(&mut self, host: &mut dyn Host, ui: &mut egui::Ui) {
        let Some((lane, s)) = self.seq.hold else { return };
        let Some(src) = self.seq.lanes[lane] else {
            self.seq.hold = None;
            return;
        };
        let (scene, _) = self.seq_current(host);
        let Some(st) = self.seq_step(host, scene, lane, s) else { return };
        let ctx = ui.ctx().clone();
        let id = egui::Id::new(("step-sheet", lane, s));
        let screen = ctx.screen_rect();
        let w = 360.0f32.min(screen.width() - 28.0);
        let hgt = 150.0;
        // Place under the step (above if no room).
        let step_rect = self.hold_rect.unwrap_or(Rect::from_center_size(screen.center(), egui::vec2(10.0, 10.0)));
        let mut pos = egui::pos2(step_rect.center().x - w / 2.0, step_rect.bottom() + 8.0);
        if pos.y + hgt > screen.bottom() - 10.0 {
            pos.y = step_rect.top() - hgt - 8.0;
        }
        pos.x = pos.x.clamp(screen.left() + 14.0, screen.right() - w - 14.0);
        let sheet = Rect::from_min_size(pos, egui::vec2(w, hgt));
        let at = self.atlas(host, &ctx, src);
        let mut close = false;
        egui::Area::new(id).fixed_pos(sheet.min).order(egui::Order::Foreground).show(&ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(sheet.size(), Sense::click());
            let p = ui.painter().clone();
            p.add(egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(140) }.as_shape(rect, r(14)));
            p.rect_filled(rect, r(14), G2);
            p.rect_stroke(rect, r(14), hair(LINE2), StrokeKind::Inside);
            let inner = rect.shrink(10.0);
            // Slices: 8×2.
            let cw = (inner.width() - 7.0 * 4.0) / 8.0;
            let chh = 26.0;
            let cur_slice = if st.slice >= 0.0 { atlas::frame_at(st.slice) } else { s };
            for i in 0..16 {
                let c = Rect::from_min_size(egui::pos2(inner.left() + (i % 8) as f32 * (cw + 4.0), inner.top() + (i / 8) as f32 * (chh + 4.0)), egui::vec2(cw, chh));
                if let Some((tex, a)) = at {
                    tile(&p, c, 4, tex, a.uv_cover(i, cw / chh), gray(if i == cur_slice { 1.0 } else { 0.6 }));
                }
                if i == cur_slice {
                    p.rect_stroke(c, r(4), Stroke::new(1.5, HI), StrokeKind::Inside);
                }
                if hit(ui, c, id.with(("slice", i)), Sense::click(), &format!("slice {}", i + 1)).clicked() {
                    self.seq_edit_step(host, scene, lane, s, |x| x.slice = (i as f32 + 0.5) / 16.0);
                }
            }
            // Params.
            let py = inner.top() + 2.0 * (chh + 4.0) + 6.0;
            let pw = (inner.width() - 2.0 * 8.0) / 3.0;
            let key = self.key;
            let degree = semis_to_degree(&key, st.pitch);
            let len_i = LENGTHS.iter().position(|(l, _)| (*l - st.length).abs() < 1e-3).unwrap_or(1);
            let vals = [
                (format!("{degree:+}").replace("+0", "0"), "pitch"),
                (LENGTHS[len_i].1.to_string(), "length"),
                (format!("{:.0}%", st.chance * 100.0), "chance"),
            ];
            for (k, (v, l)) in vals.iter().enumerate() {
                let c = Rect::from_min_max(egui::pos2(inner.left() + k as f32 * (pw + 8.0), py), egui::pos2(inner.left() + k as f32 * (pw + 8.0) + pw, inner.bottom()));
                p.rect_filled(c, r(10), G1);
                p.text(c.center() - egui::vec2(0.0, 7.0), Align2::CENTER_CENTER, v, mono(17.0), INK);
                p.text(c.center() + egui::vec2(0.0, 13.0), Align2::CENTER_CENTER, *l, font(10.5), INK3);
                let resp = hit(ui, c, id.with(("param", k)), Sense::click_and_drag(), l);
                if resp.dragged() {
                    let acc = ui.data(|d| d.get_temp::<f32>(resp.id).unwrap_or(0.0)) - resp.drag_delta().y;
                    let step = (acc / 12.0).trunc() as i32;
                    if step != 0 {
                        ui.data_mut(|d| d.insert_temp(resp.id, acc - step as f32 * 12.0));
                        match k {
                            0 => {
                                let semis = degree_to_semis(&key, (degree + step).clamp(-14, 14));
                                self.seq_edit_step(host, scene, lane, s, |x| x.pitch = semis);
                            }
                            1 => {
                                let ni = (len_i as i32 + step).clamp(0, LENGTHS.len() as i32 - 1) as usize;
                                self.seq_edit_step(host, scene, lane, s, |x| x.length = LENGTHS[ni].0);
                            }
                            _ => {
                                let c = (st.chance + step as f32 * 0.05).clamp(0.0, 1.0);
                                self.seq_edit_step(host, scene, lane, s, |x| x.chance = c);
                            }
                        }
                    } else {
                        ui.data_mut(|d| d.insert_temp(resp.id, acc));
                    }
                }
            }
        });
        // Tap outside closes.
        let pressed_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !sheet.contains(pt) && !step_rect.contains(pt)));
        if pressed_outside {
            close = true;
        }
        if close {
            self.seq.hold = None;
        }
    }
}

/// "↺": a lane that is shorter than 16 loops here.
pub fn draw_loop_glyph(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    let rad = 4.5;
    let pts: Vec<egui::Pos2> = (0..=20)
        .map(|i| {
            let a = std::f32::consts::PI * (0.35 + 1.55 * i as f32 / 20.0);
            egui::pos2(c.x + rad * a.cos(), c.y - rad * a.sin())
        })
        .collect();
    let end = *pts.last().unwrap();
    p.add(egui::Shape::line(pts, Stroke::new(1.2, col)));
    p.line_segment([end, end + egui::vec2(2.5, -1.0)], Stroke::new(1.2, col));
    p.line_segment([end, end + egui::vec2(0.5, -3.0)], Stroke::new(1.2, col));
}
