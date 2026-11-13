//! Sequence: nanoloop-simple lanes + VJ Set / Cue Arranger + Step Hold-Sheet
//! (`Slice`, `Pitch`, `Length`, `Chance`, `Ratchet`, `Offset`, `Chord`).
//! Up to 12 named scenes (`intro`, `drop`, `dialogue`, `wash`, `outro`),
//! repeat counts (`×1..×16` or `∞` hold for live improv), `+ cue`, and `dup`.

use super::{Studio, LANES, SCENES, STEPS};
use crate::atlas;
use crate::host::{ChordChoice, Host};
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::music::Key;
use chromagrain_core::seq::StepPattern;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

const DEFAULT_SECTION_NAMES: [&str; 6] = ["intro", "groove", "dialogue", "drop", "wash", "outro"];

/// Semitones for key degree `d` (0 = root) of `key`'s scale.
pub fn degree_to_semis(key: &Key, d: i32) -> f32 {
    let iv = key.scale.intervals();
    let n = iv.len() as i32;
    (iv[d.rem_euclid(n) as usize] + 12 * d.div_euclid(n)) as f32
}

/// The nearest key degree for a semitone offset.
pub fn semis_to_degree(key: &Key, st: f32) -> i32 {
    (-28..=28)
        .min_by(|a, b| (degree_to_semis(key, *a) - st).abs().total_cmp(&(degree_to_semis(key, *b) - st).abs()))
        .unwrap_or(0)
}

const LENGTHS: [(f32, &str); 6] = [
    (0.5, "1/32"),
    (1.0, "1/16"),
    (2.0, "1/8"),
    (4.0, "1/4"),
    (8.0, "1/2"),
    (16.0, "1"),
];

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
        for (si, pid_opt) in self.seq.scene_pats.iter().enumerate() {
            let Some(pid) = pid_opt else { continue };
            let Some(pat) = p.patterns.get_mut(*pid) else { continue };
            if !self.seq.scene_names[si].is_empty() && pat.name != self.seq.scene_names[si] {
                pat.name = self.seq.scene_names[si].clone();
            }
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

    /// Scene label (`"1"` when unnamed, `"1 · intro"` when named).
    pub fn scene_display_label(&self, s: usize, max_name: usize) -> String {
        let nm = self.seq.scene_names.get(s).map(|x| x.trim()).unwrap_or("");
        if nm.is_empty() || max_name == 0 {
            (s + 1).to_string()
        } else {
            format!("{} · {}", s + 1, super::play::short(nm, max_name))
        }
    }

    /// Chain entries `(scene, raw_reps, start_beat, len_beats)`.
    /// `raw_reps == 0` means `∞` hold (allocated 4 bars in timeline, held live).
    pub(crate) fn seq_chain_spans(&self) -> Vec<(usize, u32, f64, f64)> {
        let mut b = 0.0;
        self.seq
            .chain
            .iter()
            .filter(|(s, _)| self.seq_pattern(*s).is_some())
            .map(|&(s, n)| {
                let at = b;
                let bars = if n == 0 { 4 } else { n.clamp(1, 16) };
                let len = 4.0 * bars as f64;
                b += len;
                (s, n, at, len)
            })
            .collect()
    }

    /// Chain entries `(scene, eff_bars, start_beat)`.
    fn seq_chain_layout(&self) -> Vec<(usize, u32, f64)> {
        self.seq_chain_spans()
            .into_iter()
            .map(|(s, n, at, _)| (s, if n == 0 { 4 } else { n.clamp(1, 16) }, at))
            .collect()
    }

    /// The scene playing at the playhead (or being edited, when stopped).
    pub fn seq_current(&self, host: &dyn Host) -> (usize, Option<(usize, f64)>) {
        let spans = self.seq_chain_spans();
        let b = host.playhead_beats();
        for (i, (s, _, at, len)) in spans.iter().enumerate() {
            if b >= *at && b < *at + *len {
                if host.playing() {
                    return (*s, Some((i, (b - *at) / len.max(1e-6))));
                }
                return (self.seq.scene, Some((i, (b - *at) / len.max(1e-6))));
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
        let lay: Vec<(usize, f64, f64)> = if self.seq.hold_scene {
            // Live improv `∞ hold`: loop the held scene cleanly until released or switched.
            let s = self.seq.scene.min(SCENES - 1);
            self.seq.scene_pats[s]
                .or_else(|| self.seq.scene_pats.iter().copied().flatten().next())
                .into_iter()
                .map(|pid| (pid, 0.0, 4.0))
                .collect()
        } else {
            self.seq_chain_spans()
                .iter()
                .filter_map(|(s, _, at, len)| Some((self.seq.scene_pats[*s]?, *at, *len)))
                .collect()
        };
        if lay.is_empty() {
            return;
        }
        if host.sequence_changed(&lay) {
            self.seq.built_rev = self.rev;
        }
    }

    /// Queued scenes start on the next bar; `∞` cues hold on bar wrap.
    pub fn seq_tick(&mut self, host: &mut dyn Host) {
        if !host.playing() {
            if let Some(q) = self.seq.queued {
                self.seq_jump(host, q);
            }
            return;
        }
        let bar = (host.playhead_beats() / 4.0).floor();
        let prev = self.bar_memo.replace(bar);
        let crossed_bar = prev.is_some_and(|pb| pb != bar);
        if let Some(q) = self.seq.queued {
            if crossed_bar {
                self.seq_jump(host, q);
            }
            return;
        }
        if crossed_bar && !self.seq.hold_scene {
            // Check if the previous bar was inside an `∞` (`raw_reps == 0`) cue that just wrapped.
            if let Some(pb) = prev {
                let prev_beat = pb * 4.0 + 0.1;
                let spans = self.seq_chain_spans();
                if let Some(&(_, raw_reps, at, len)) = spans.iter().find(|&&(_, _, a, l)| prev_beat >= a && prev_beat < a + l) {
                    if raw_reps == 0 && host.playhead_beats() >= at + len - 0.05 {
                        host.seek(at);
                    }
                }
            }
        }
    }

    fn seq_jump(&mut self, host: &mut dyn Host, scene: usize) {
        self.seq.queued = None;
        self.seq.scene = scene;
        self.bar_memo = None;
        if self.seq.hold_scene {
            self.rev += 1;
            host.seek(0.0);
        } else if let Some((_, _, at)) = self.seq_chain_layout().into_iter().find(|(s, _, _)| *s == scene) {
            host.seek(at);
        }
    }

    /// Jump to or queue a specific chain cue index.
    pub fn seq_cue_tap(&mut self, host: &mut dyn Host, cue_idx: usize) {
        let spans = self.seq_chain_spans();
        let Some(&(s, _, at, _)) = spans.get(cue_idx) else { return };
        self.seq.scene = s;
        if self.seq.hold_scene {
            if host.playing() {
                self.seq.queued = Some(s);
            } else {
                self.rev += 1;
                host.seek(0.0);
            }
        } else if host.playing() {
            self.seq.queued = Some(s);
        } else {
            host.seek(at);
        }
    }

    /// Duplicate the active scene into the next empty scene slot (`0..SCENES`)
    /// and append it as a cue.
    pub fn seq_duplicate_scene(&mut self, host: &mut dyn Host) -> Option<usize> {
        let (cur, _) = self.seq_current(host);
        let src_pid = self.seq_pattern(cur)?;
        let dst = (0..SCENES).find(|&s| self.seq.scene_pats[s].is_none())?;
        let mut p = host.project().lock().unwrap();
        let mut pat = p.patterns.get(src_pid)?.clone();
        let default_nm = DEFAULT_SECTION_NAMES.get(dst).copied().unwrap_or("");
        if self.seq.scene_names[dst].is_empty() && !default_nm.is_empty() {
            self.seq.scene_names[dst] = default_nm.to_string();
        }
        pat.name = if self.seq.scene_names[dst].is_empty() {
            format!("scene {}", dst + 1)
        } else {
            self.seq.scene_names[dst].clone()
        };
        let pid = p.add_pattern(pat);
        drop(p);
        self.seq.scene_pats[dst] = Some(pid);
        // Copy step ratchet/chord extensions from `cur` to `dst`.
        let copied_ext: Vec<_> = self
            .seq
            .step_ext
            .iter()
            .filter(|((sc, _, _), _)| *sc == cur)
            .map(|(&(_, l, st), &v)| ((dst, l, st), v))
            .collect();
        for (k, v) in copied_ext {
            self.seq.step_ext.insert(k, v);
        }
        self.seq.chain.push((dst, 2));
        self.seq.scene = dst;
        self.rev += 1;
        host.mark();
        Some(dst)
    }

    /// Append a new cue to the VJ set chain.
    pub fn seq_add_cue(&mut self, host: &mut dyn Host) {
        let existing: Vec<usize> = (0..SCENES).filter(|&s| self.seq_pattern(s).is_some()).collect();
        if existing.is_empty() {
            return;
        }
        let next_s = if let Some(empty) = (0..SCENES).find(|&s| self.seq_pattern(s).is_none() && s < 6) {
            self.seq_scene_tap(host, empty);
            self.seq.queued = None;
            return;
        } else {
            let last = self.seq.chain.last().map(|(s, _)| *s).unwrap_or(0);
            let pos = existing.iter().position(|&x| x == last).unwrap_or(0);
            existing[(pos + 1) % existing.len()]
        };
        self.seq.chain.push((next_s, 2));
        self.rev += 1;
        host.mark();
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

    pub fn seq_edit_step(
        &mut self,
        host: &mut dyn Host,
        scene: usize,
        lane: usize,
        step: usize,
        f: impl FnOnce(&mut chromagrain_core::seq::Step),
    ) {
        let Some(pid) = self.seq_pattern(scene) else { return };
        let mut p = host.project().lock().unwrap();
        if let Some(st) = p
            .patterns
            .get_mut(pid)
            .and_then(|pat| pat.rows.get_mut(lane))
            .and_then(|r| r.steps.get_mut(step))
        {
            f(st);
        }
        drop(p);
        host.mark();
    }

    pub fn step_ratchet_ext(&self, scene: usize, lane: usize, step: usize) -> (u8, f32, ChordChoice) {
        self.seq.step_ext.get(&(scene, lane, step)).copied().unwrap_or((1, 0.0, ChordChoice::Off))
    }

    pub fn set_step_ratchet_ext(
        &mut self,
        host: &mut dyn Host,
        scene: usize,
        lane: usize,
        step: usize,
        ratchet: u8,
        slope_st: f32,
        chord: ChordChoice,
    ) {
        let r = ratchet.clamp(1, 8);
        let sl = slope_st.clamp(-12.0, 12.0);
        if r == 1 && sl.abs() < 1e-3 && chord == ChordChoice::Off {
            self.seq.step_ext.remove(&(scene, lane, step));
        } else {
            self.seq.step_ext.insert((scene, lane, step), (r, sl, chord));
        }
        let core_chord = chord.to_core();
        self.seq_edit_step(host, scene, lane, step, |st| {
            st.ratchet = r;
            st.ratchet_pitch = sl;
            st.chord = core_chord;
        });
        self.rev += 1;
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
        if s >= SCENES {
            return;
        }
        if self.seq_pattern(s).is_none() {
            let (cur, _) = self.seq_current(host);
            let Some(src) = self.seq_pattern(cur) else { return };
            let mut p = host.project().lock().unwrap();
            let mut pat = p.patterns[src].clone();
            pat.name = if self.seq.scene_names[s].is_empty() {
                format!("scene {}", s + 1)
            } else {
                self.seq.scene_names[s].clone()
            };
            let pid = p.add_pattern(pat);
            drop(p);
            self.seq.scene_pats[s] = Some(pid);
            self.seq.chain.push((s, 2));
            self.rev += 1;
        }
        self.seq.scene = s;
        self.seq.queued = Some(s);
        if self.seq.hold_scene && !host.playing() {
            self.rev += 1;
        }
    }

    // ------------------------------------------------------------ shared Set / Cue Bar

    /// Compact VJ Set / Cue Arranger Bar shared across `Sequence` and `Perform`.
    /// Supports click-to-queue, vertical drag / click on active cue to set
    /// repeats (`×1..×16` or `∞` hold), `+ cue`, `dup`, and `∞ hold` live lock!
    pub fn set_arranger_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, id_salt: &str) {
        let p = ui.painter().clone();
        let spans = self.seq_chain_spans();
        let (cur_scene, at) = self.seq_current(host);
        let h = rect.height();
        let cy = rect.center().y;

        // Right-anchored live tools: `∞ hold`, `dup`, `+ cue`.
        let mut rx = rect.right();
        let hold_w = if rect.width() < 520.0 { 44.0 } else { 58.0 };
        let hold_r = Rect::from_min_size(egui::pos2(rx - hold_w, rect.top()), egui::vec2(hold_w, h));
        rx = hold_r.left() - 5.0;
        if chip(ui, hold_r, ui.id().with((id_salt, "hold-scene")), "hold scene", false, self.seq.hold_scene).clicked() {
            self.seq.hold_scene = !self.seq.hold_scene;
            self.rev += 1;
            host.mark();
        }
        p.text(
            hold_r.center(),
            Align2::CENTER_CENTER,
            if hold_w < 50.0 { "∞" } else { "∞ hold" },
            mono_medium(10.5),
            if self.seq.hold_scene { ON_HI } else { INK2 },
        );

        let mut dup_r_opt = None;
        if rect.width() >= 420.0 {
            let dup_r = Rect::from_min_size(egui::pos2(rx - 42.0, rect.top()), egui::vec2(42.0, h));
            rx = dup_r.left() - 5.0;
            dup_r_opt = Some(dup_r);
        }
        let add_w = if rect.width() < 380.0 { 36.0 } else { 52.0 };
        let add_r = Rect::from_min_size(egui::pos2(rx - add_w, rect.top()), egui::vec2(add_w, h));
        rx = add_r.left() - 6.0;

        if let Some(dup_r) = dup_r_opt {
            if chip(ui, dup_r, ui.id().with((id_salt, "dup-scene")), "duplicate scene", false, false).clicked() {
                self.seq_duplicate_scene(host);
            }
            p.text(dup_r.center(), Align2::CENTER_CENTER, "dup", font_medium(11.0), INK2);
        }
        if chip(ui, add_r, ui.id().with((id_salt, "add-cue")), "add cue", false, false).clicked() {
            self.seq_add_cue(host);
        }
        p.text(
            add_r.center(),
            Align2::CENTER_CENTER,
            if add_w < 42.0 { "+" } else { "+ cue" },
            font_medium(11.0),
            INK,
        );

        // Left: optional `SET` label + scrollable/fitted cue pills.
        let mut x = rect.left();
        if rect.width() >= 680.0 {
            let lr = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Set", 10.0);
            x = lr.right() + 10.0;
        }

        let mut seek_to: Option<(usize, f64)> = None;
        let mut step_reps: Option<(usize, i32)> = None;
        let mut cycle_reps: Option<usize> = None;

        for (i, &(s, raw_n, start, _)) in spans.iter().enumerate() {
            let sn = self.seq.scene_names[s].trim();
            let head = if sn.is_empty() || rect.width() < 460.0 {
                (s + 1).to_string()
            } else {
                format!("{} {}", s + 1, super::play::short(sn, 7))
            };
            let rep_str = if raw_n == 0 { "∞".to_string() } else { format!("×{raw_n}") };
            let text_w = p.layout_no_wrap(format!("{head} {rep_str}"), mono(11.0), INK).size().x;
            let w = (text_w + 18.0).max(44.0);
            if x + w > rx {
                break;
            }
            let cr = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, h));
            let resp = hit(
                ui,
                cr,
                ui.id().with((id_salt, "chain", i)),
                Sense::click_and_drag(),
                &format!("chain {}", i + 1),
            );
            let cur = if self.seq.hold_scene {
                s == cur_scene && i == spans.iter().position(|x| x.0 == cur_scene).unwrap_or(0)
            } else {
                at.map(|(ci, _)| ci) == Some(i)
            };
            let queued = self.seq.queued == Some(s) && !cur;
            paint_switch_cap(&p, cr, SwitchKind::PuttyAmber, cur, resp.hovered(), resp.is_pointer_button_down_on());
            if let (true, Some((_, prog))) = (cur && !self.seq.hold_scene, at) {
                let fill = Rect::from_min_size(
                    egui::pos2(cr.left() + 3.0, cr.bottom() - 3.5),
                    egui::vec2((cr.width() - 6.0) * prog.clamp(0.0, 1.0) as f32, 2.0),
                );
                p.rect_filled(fill, r(1), ON_HI);
            }
            if queued && (ui.input(|inp| inp.time) * 3.0).fract() < 0.55 {
                p.rect_stroke(cr, r(4), Stroke::new(1.8, AMBER_MID), StrokeKind::Inside);
                ui.ctx().request_repaint();
            }
            runs(
                &p,
                cr.center(),
                Align2::CENTER_CENTER,
                &[
                    (&head, font_semibold(11.0), if cur { ON_HI } else { INK }),
                    (" ", mono(10.0), INK3),
                    (&rep_str, mono(10.0), if cur { ON_HI } else { INK3 }),
                ],
            );

            if resp.dragged() {
                let acc = ui.data(|d| d.get_temp::<f32>(resp.id).unwrap_or(0.0)) - resp.drag_delta().y;
                let delta = (acc / 14.0).trunc() as i32;
                if delta != 0 {
                    ui.data_mut(|d| d.insert_temp(resp.id, acc - delta as f32 * 14.0));
                    step_reps = Some((i, delta));
                } else {
                    ui.data_mut(|d| d.insert_temp(resp.id, acc));
                }
            } else if resp.double_clicked() {
                cycle_reps = Some(i);
            } else if resp.clicked() {
                seek_to = Some((i, start));
            }
            x = cr.right() + 5.0;
        }

        if let Some((idx, delta)) = step_reps {
            if let Some((_, reps)) = self.seq.chain.get_mut(idx) {
                // Sequence: 0 (∞), 1..=16.
                let cur_v = *reps as i32;
                *reps = (cur_v + delta).clamp(0, 16) as u32;
                self.rev += 1;
                host.mark();
            }
        } else if let Some(idx) = cycle_reps {
            if let Some((_, reps)) = self.seq.chain.get_mut(idx) {
                *reps = match *reps {
                    1 => 2,
                    2 => 4,
                    4 => 8,
                    8 => 16,
                    16 => 0,
                    _ => 1,
                };
                self.rev += 1;
                host.mark();
            }
        } else if let Some((idx, start)) = seek_to {
            if host.playing() {
                self.seq_cue_tap(host, idx);
            } else {
                if let Some(&(s, _, _, _)) = spans.get(idx) {
                    self.seq.scene = s;
                }
                host.seek(start);
            }
        }
    }

    // ------------------------------------------------------------ view

    pub fn sequence_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1100.0;
        let (lanes_rect, song) = if narrow {
            let strip_h = (area.height() * 0.19).clamp(78.0, 108.0);
            let song = Rect::from_min_size(area.min, egui::vec2(area.width(), strip_h));
            (Rect::from_min_max(egui::pos2(area.left(), song.bottom() + 12.0), area.max), song)
        } else {
            let rw = (area.width() * 0.24).clamp(272.0, 384.0);
            let song = Rect::from_min_max(egui::pos2(area.right() - rw, area.top()), area.max);
            (Rect::from_min_max(area.min, egui::pos2(song.left() - 20.0, area.bottom())), song)
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
        let pending = self.pending_slots(host);
        let t_ui = ui.input(|i| i.time);
        for lane in 0..LANES {
            let y = area.top() + lane as f32 * (lh + g);
            let header = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(hw, lh));
            let src = self.seq.lanes[lane];
            let Some(src) = src else {
                if let Some((pd, _)) = pending.iter().find(|(_, t)| *t == crate::host::AddTarget::Lane(lane)) {
                    super::add::placeholder_tile(&p, header, 6, pd, t_ui, true);
                    ctx.request_repaint();
                    continue;
                }
                monitor_well(&p, header, 6);
                if first_empty {
                    first_empty = false;
                    let resp = hit(ui, header, ui.id().with(("lane-add", lane)), Sense::click(), "add a lane");
                    draw_icon(&p, Icon::Plus, header.center(), 16.0, if resp.hovered() { CREAM } else { CREAM3 });
                    if resp.clicked() {
                        self.open_add(host, crate::host::AddTarget::Lane(lane));
                    }
                }
                for s in 0..STEPS {
                    let c = Rect::from_min_size(egui::pos2(step_x(s), y), egui::vec2(sw, lh));
                    p.rect_filled(c, r(4), Color32::from_rgba_unmultiplied(24, 27, 28, 85));
                    p.rect_stroke(c, r(4), hair(LINE2), StrokeKind::Inside);
                }
                continue;
            };
            let len = self.lane_len(host, scene, lane);
            let muted = self.lane_muted(host, scene, lane);
            let steps: Vec<chromagrain_core::seq::Step> = (0..len).filter_map(|s| self.seq_step(host, scene, lane, s)).collect();
            let silent = steps.iter().all(|s| !s.on);
            let at = self.atlas(host, &ctx, src);
            let chop_starts: Vec<f32> = {
                let pp = host.project().lock().unwrap();
                pp.sources
                    .get(src)
                    .map(|so| so.slices.iter().map(|sl| (sl.start / so.duration().max(1e-6)) as f32).collect())
                    .unwrap_or_default()
            };
            let is_sel = lane == self.sel_lane;
            let lane_adsr = self.target_env(host, super::clip::EnvTarget::Lane(lane)).unwrap_or_default();
            monitor_well(&p, header, 6);
            if let Some((tex, a)) = at {
                tile(&p, header.shrink(1.2), 5, tex, a.uv_cover(5, hw / lh), gray(if muted { 0.28 } else if silent { 0.50 } else { 0.80 }));
                p.rect_filled(Rect::from_min_size(header.min + egui::vec2(1.0, 1.0), egui::vec2(3.0, lh - 2.0)), r(2), a.swatch);
            }
            let top_scrim = Rect::from_min_size(header.min, egui::vec2(hw, 20.0_f32.min(lh * 0.45)));
            p.rect_filled(top_scrim, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_black_alpha(110));
            let bot_scrim = Rect::from_min_max(egui::pos2(header.left(), (header.bottom() - 22.0).max(header.top())), header.max);
            p.rect_filled(bot_scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, Color32::from_black_alpha(135));
            if lh >= 42.0 && hw >= 88.0 {
                let raw_name = self.source_name(host, src);
                let clean = super::strip_media_ext(&raw_name);
                let room = ((hw - 36.0) / 6.0).max(4.0) as usize;
                p.text(
                    egui::pos2(header.left() + 9.0, header.top() + 6.5),
                    Align2::LEFT_TOP,
                    super::play::short(clean, room.min(16)),
                    font_medium(10.5),
                    if muted { CREAM3 } else if is_sel { CREAM } else { CREAM2 },
                );
            }
            if !muted {
                let env_w = (hw * 0.36).clamp(18.0, 48.0);
                let env_r = Rect::from_min_size(egui::pos2(header.left() + 9.0, header.bottom() - 16.5), egui::vec2(env_w, 10.5));
                mini_adsr(&p, env_r, &lane_adsr, if is_sel { 0.92 } else { 0.68 });
            } else {
                p.text(header.left_bottom() + egui::vec2(10.0, -5.0), Align2::LEFT_BOTTOM, "MUTED", font_semibold(9.5), RUBY_MID);
            }
            if len != STEPS {
                p.text(header.right_bottom() + egui::vec2(-8.0, -5.0), Align2::RIGHT_BOTTOM, len.to_string(), mono(10.0), CREAM2);
            }
            if is_sel {
                p.rect_stroke(header, r(6), Stroke::new(1.8, AMBER_MID), StrokeKind::Inside);
            }
            let hresp = hit(ui, header, ui.id().with(("lane-head", lane)), Sense::click(), &format!("lane {}", lane + 1));
            let chev_vis = Rect::from_min_size(egui::pos2(header.right() - 24.0, header.top() + 4.0), egui::vec2(20.0, 20.0));
            let chev_hit = Rect::from_center_size(chev_vis.center(), egui::vec2(24.0, 24.0));
            let chev_resp = chev_button(ui, chev_vis, chev_hit, ui.id().with(("lane-clip", lane)), &format!("clip lane {}", lane + 1));
            let h = hold(ui, &hresp, 0.45);
            if chev_resp.clicked() {
                self.sel_lane = lane;
                self.open_clip(super::ClipTarget::Lane(lane));
            } else if h.fired {
                self.sel_lane = lane;
                self.open_clip(super::ClipTarget::Lane(lane));
            } else if hresp.clicked() && !h.held_before && !chev_resp.hovered() {
                self.sel_lane = lane;
                self.toggle_lane_mute(host, lane);
            }
            let cur = if playing || host.playhead_beats() > 0.0 { Some(slot % len.max(1)) } else { None };
            for s in 0..STEPS {
                let c = Rect::from_min_size(egui::pos2(step_x(s), y), egui::vec2(sw, lh));
                if s >= len {
                    if s == len {
                        draw_loop_glyph(&p, egui::pos2(c.left() + 10.0, c.center().y), INK3);
                    }
                    continue;
                }
                let st = steps.get(s).copied().unwrap_or_default();
                let (ratchet, _, step_chord) = self.step_ratchet_ext(scene, lane, s);
                let ghost = if muted { 0.35 } else { 1.0 };
                if st.on {
                    monitor_well(&p, c, 5);
                    if let Some((tex, a)) = at {
                        let slice = match st.slice_index.and_then(|k| chop_starts.get(k as usize)) {
                            Some(pos) => atlas::frame_at(*pos),
                            None if st.slice >= 0.0 => atlas::frame_at(st.slice),
                            None => s,
                        };
                        tile(&p, c.shrink(1.2), 4, tex, a.uv_cover(slice, sw / lh), gray(0.92 * ghost));
                    }
                    if st.pitch.abs() > 0.01 {
                        let d = semis_to_degree(&key, st.pitch);
                        p.text(c.left_top() + egui::vec2(5.0, 4.0), Align2::LEFT_TOP, format!("{d:+}"), mono_medium(10.0), CREAM);
                    }
                    if st.chance < 0.999 {
                        p.text(c.right_top() + egui::vec2(-5.0, 4.0), Align2::RIGHT_TOP, format!("{:.0}%", st.chance * 100.0), mono(9.0), CREAM2);
                    }
                    if ratchet > 1 && sw >= 24.0 {
                        let tag_r = Rect::from_min_size(egui::pos2(c.left() + 3.0, c.bottom() - 14.0), egui::vec2(18.0, 11.0));
                        p.rect_filled(tag_r, r(3), Color32::from_black_alpha(200));
                        p.text(tag_r.center(), Align2::CENTER_CENTER, format!("×{ratchet}"), mono_medium(8.5), AMBER_MID);
                    }
                    if step_chord != ChordChoice::Off && sw >= 34.0 {
                        p.text(c.right_bottom() + egui::vec2(-4.0, -3.0), Align2::RIGHT_BOTTOM, step_chord.label(), mono_medium(8.0), MINT_MID);
                    }
                    p.rect_stroke(c, r(5), Stroke::new(1.2, AMBER_EDGE), StrokeKind::Inside);
                } else {
                    monitor_well(&p, c, 5);
                    p.circle_filled(c.center(), if s % 4 == 0 { 2.0 } else { 1.4 }, if s % 4 == 0 { CREAM3 } else { Color32::from_white_alpha(45) });
                }
                if cur == Some(s) {
                    p.rect_stroke(c, r(5), Stroke::new(2.0, AMBER_CORE), StrokeKind::Inside);
                }
                if self.seq.hold == Some((lane, s)) {
                    self.hold_rect = Some(c);
                    p.rect_stroke(c.expand(1.0), r(6), Stroke::new(2.0, AMBER_MID), StrokeKind::Inside);
                }
                let resp = hit(ui, c, ui.id().with(("step", lane, s)), Sense::click_and_drag(), &format!("lane {} step {}", lane + 1, s + 1));
                let h = hold(ui, &resp, 0.45);
                if h.fired {
                    self.sel_lane = lane;
                    self.seq.hold = Some((lane, s));
                    if !st.on {
                        self.seq_toggle(host, scene, lane, s);
                    }
                } else if resp.clicked() && !h.held_before {
                    self.sel_lane = lane;
                    self.seq_toggle(host, scene, lane, s);
                } else if resp.dragged() && st.on {
                    self.sel_lane = lane;
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
            paint_switch_cap(&p, rect, SwitchKind::Amber, true, resp.hovered(), resp.is_pointer_button_down_on());
        } else if exists {
            paint_switch_cap(&p, rect, SwitchKind::PuttyAmber, false, resp.hovered(), resp.is_pointer_button_down_on());
        } else {
            p.rect_filled(rect, r(4), LCD_BG);
            p.rect_stroke(rect, r(4), hair(LINE2), StrokeKind::Inside);
        }
        if self.seq.queued == Some(s) && !on {
            let t = ui.input(|i| i.time);
            if (t * 3.0).fract() < 0.5 {
                p.rect_stroke(rect, r(9), Stroke::new(1.5, HI), StrokeKind::Inside);
            }
            ui.ctx().request_repaint();
        }
        let col = if on { ON_HI } else if exists { INK } else { INK4 };
        let small = rect.height() < 42.0;
        let sn = self.seq.scene_names[s].trim();
        if !small && !sn.is_empty() && rect.width() >= 52.0 {
            p.text(
                rect.center() - egui::vec2(0.0, 7.0),
                Align2::CENTER_CENTER,
                (s + 1).to_string(),
                mono_medium(12.5),
                col,
            );
            p.text(
                rect.center() + egui::vec2(0.0, 6.5),
                Align2::CENTER_CENTER,
                super::play::short(sn, ((rect.width() - 10.0) / 5.5).max(3.0) as usize),
                font(9.5),
                if on { Color32::from_black_alpha(170) } else { INK3 },
            );
        } else {
            p.text(
                rect.center() - egui::vec2(0.0, if small || !exists { 0.0 } else { 3.0 }),
                Align2::CENTER_CENTER,
                (s + 1).to_string(),
                mono(if small { 12.5 } else { 14.0 }),
                col,
            );
        }
        if exists && !small && sn.is_empty() {
            let pid = self.seq_pattern(s).unwrap();
            let pp = host.project().lock().unwrap();
            let rows: Vec<bool> = pp
                .patterns
                .get(pid)
                .map(|pat| pat.rows.iter().map(|r| r.steps.iter().any(|s| s.on) && !r.muted).collect())
                .unwrap_or_default();
            drop(pp);
            let n = rows.len().max(1);
            let bw = ((rect.width() - 18.0) / n as f32 - 2.0).max(2.0);
            for (i, on_lane) in rows.iter().enumerate() {
                let x = rect.left() + 9.0 + i as f32 * (bw + 2.0);
                let c = if *on_lane {
                    if on { Color32::from_black_alpha(140) } else { INK3 }
                } else if on {
                    Color32::from_black_alpha(40)
                } else {
                    G4
                };
                p.rect_filled(Rect::from_min_size(egui::pos2(x, rect.bottom() - 7.0), egui::vec2(bw, 2.0)), r(1), c);
            }
        }
        if resp.double_clicked() {
            self.seq.scene = s;
            self.seq.editing_name = Some(s);
        } else if resp.clicked() {
            self.seq_scene_tap(host, s);
        }
    }

    fn song_column(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, col: Rect) {
        let stage_h = (col.width() * 9.0 / 16.0).floor().min(col.height() * 0.36);
        let stage = Rect::from_min_size(col.min, egui::vec2(col.width(), stage_h));
        self.stage(host, ui, stage, 12.0);
        self.layout_switch(host, ui, stage);
        let mut y_after = stage.bottom() + 12.0;

        if host.caps().scenes {
            let (cur, _) = self.seq_current(host);

            // Scene header + inline editable scene name (`intro`, `drop`, `dialogue`, `wash`, `outro`).
            let name_bar = Rect::from_min_size(egui::pos2(col.left(), y_after), egui::vec2(col.width(), 30.0));
            let mut nm = self.seq.scene_names[cur.min(SCENES - 1)].clone();
            let (_, enter) = super::add::field_sized(
                ui,
                name_bar,
                egui::Id::new(("seq-scn-name", cur)),
                &mut nm,
                &format!("Scene {} name — intro, drop, dialogue, wash…", cur + 1),
                "scene name",
                12.0,
            );
            if nm != self.seq.scene_names[cur.min(SCENES - 1)] {
                self.seq.scene_names[cur.min(SCENES - 1)] = nm.clone();
                if let Some(pid) = self.seq_pattern(cur) {
                    if let Some(pat) = host.project().lock().unwrap().patterns.get_mut(pid) {
                        pat.name = if nm.trim().is_empty() { format!("scene {}", cur + 1) } else { nm.trim().to_string() };
                    }
                }
                host.mark();
            }
            if enter {
                self.seq.editing_name = None;
            }
            y_after = name_bar.bottom() + 8.0;

            // 4×3 grid of Scenes 1..12.
            let g = 6.0;
            let sw = (col.width() - g * 3.0) / 4.0;
            let avail_for_grid = (col.bottom() - y_after - 88.0).max(114.0);
            let sh = ((avail_for_grid - g * 2.0) / 3.0).clamp(38.0, 52.0);
            for s in 0..SCENES {
                let rc = Rect::from_min_size(
                    egui::pos2(col.left() + (s % 4) as f32 * (sw + g), y_after + (s / 4) as f32 * (sh + g)),
                    egui::vec2(sw, sh),
                );
                self.scene_cell(host, ui, rc, s, cur);
            }
            y_after += 3.0 * sh + 2.0 * g + 10.0;

            // Set / Cue Arranger Bar.
            let chain = Rect::from_min_size(egui::pos2(col.left(), y_after), egui::vec2(col.width(), 34.0));
            self.set_arranger_bar(host, ui, chain, "seq-set");
            y_after = chain.bottom() + 10.0;
        }

        if col.bottom() - y_after >= 32.0 {
            if self.seq.lanes.get(self.sel_lane).copied().flatten().is_none() {
                if let Some(first) = self.seq.lanes.iter().position(|l| l.is_some()) {
                    self.sel_lane = first;
                }
            }
            let l = self.sel_lane.min(LANES - 1);
            if let Some(src) = self.seq.lanes[l] {
                let ob = Rect::from_min_size(egui::pos2(col.left(), y_after), egui::vec2(col.width(), 32.0));
                let p = ui.painter();
                p.rect_filled(ob, r(10), G1);
                p.rect_stroke(ob, r(10), hair(LINE), StrokeKind::Inside);
                let raw = self.source_name(host, src);
                let clean = super::strip_media_ext(&raw);
                let room = ((ob.width() - 122.0) / 6.2).max(5.0) as usize;
                p.text(
                    egui::pos2(ob.left() + 12.0, ob.center().y),
                    Align2::LEFT_CENTER,
                    format!("Lane {} · {}", l + 1, super::play::short(clean, room.min(18))),
                    font_medium(12.0),
                    INK2,
                );
                let pill = Rect::from_min_size(egui::pos2(ob.right() - 100.0, ob.center().y - 13.0), egui::vec2(92.0, 26.0));
                if open_clip_pill(ui, pill, ui.id().with("seq-open-clip"), "open clip").clicked() {
                    self.open_clip(super::ClipTarget::Lane(l));
                }
            }
        }
    }

    fn song_strip(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, strip: Rect) {
        let h = strip.height();
        let stage = Rect::from_min_size(strip.min, egui::vec2((h * 16.0 / 9.0).floor(), h));
        self.stage(host, ui, stage, 10.0);
        self.layout_switch(host, ui, stage);
        if !host.caps().scenes {
            return;
        }
        let right = Rect::from_min_max(egui::pos2(stage.right() + 12.0, strip.top()), strip.max);
        let top_h = ((h - 6.0) * 0.5).max(32.0);
        let (cur, _) = self.seq_current(host);

        // Top row of narrow strip: Scenes 1..12.
        let sg = 4.0;
        let sw = ((right.width() - sg * (SCENES as f32 - 1.0)) / SCENES as f32).max(28.0);
        for s in 0..SCENES {
            let x = right.left() + s as f32 * (sw + sg);
            if x + sw > right.right() + 1.0 {
                break;
            }
            let rc = Rect::from_min_size(egui::pos2(x, right.top()), egui::vec2(sw, top_h));
            self.scene_cell(host, ui, rc, s, cur);
        }

        // Bottom row of narrow strip: Set / Cue Arranger Bar.
        let bot = Rect::from_min_max(egui::pos2(right.left(), right.top() + top_h + 6.0), right.max);
        self.set_arranger_bar(host, ui, bot, "seq-strip-set");
    }

    /// Step hold-sheet: `Slice` (8×2) + Row 1 (`Pitch`, `Length`, `Chance`) +
    /// Row 2 (`Ratchet` ×1..×8 + slope, `Offset` drunk pocket, `Chord`).
    fn step_sheet(&mut self, host: &mut dyn Host, ui: &mut egui::Ui) {
        let Some((lane, s)) = self.seq.hold else { return };
        let Some(src) = self.seq.lanes[lane] else {
            self.seq.hold = None;
            return;
        };
        let (scene, _) = self.seq_current(host);
        let Some(st) = self.seq_step(host, scene, lane, s) else { return };
        let (ratchet, ratchet_slope, step_chord) = self.step_ratchet_ext(scene, lane, s);

        let ctx = ui.ctx().clone();
        let id = egui::Id::new(("step-sheet", lane, s));
        let screen = ctx.screen_rect();
        let w = 420.0f32.min(screen.width() - 28.0);
        let hgt = 208.0;
        let step_rect = self.hold_rect.unwrap_or(Rect::from_center_size(screen.center(), egui::vec2(10.0, 10.0)));
        let mut pos = egui::pos2(step_rect.center().x - w / 2.0, step_rect.bottom() + 8.0);
        if pos.y + hgt > screen.bottom() - 10.0 {
            pos.y = (step_rect.top() - hgt - 8.0).max(screen.top() + 10.0);
        }
        pos.x = pos.x.clamp(screen.left() + 14.0, screen.right() - w - 14.0);
        let sheet = Rect::from_min_size(pos, egui::vec2(w, hgt));
        let at = self.atlas(host, &ctx, src);
        let mut close = false;

        egui::Area::new(id).fixed_pos(sheet.min).order(egui::Order::Foreground).show(&ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(sheet.size(), Sense::click());
            let p = ui.painter().clone();
            p.add(egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(130) }.as_shape(rect, r(8)));
            chassis_plate(&p, rect, CHASSIS_TOP, CHASSIS_BOT);
            p.rect_stroke(rect, r(8), Stroke::new(1.2, BEVEL_LO), StrokeKind::Inside);
            chassis_screws(&p, rect, 6.5);
            let inner = rect.shrink(11.0);

            // 1. Slice grid (8×2).
            let cw = (inner.width() - 7.0 * 4.0) / 8.0;
            let chh = 26.0;
            let (chops, src_len) = {
                let pp = host.project().lock().unwrap();
                pp.sources.get(src).map(|so| (so.slices.clone(), so.duration())).unwrap_or_default()
            };
            let n = if chops.is_empty() { 16 } else { chops.len().min(16) };
            let cur_slice = match st.slice_index {
                Some(k) if !chops.is_empty() => k as usize,
                _ if chops.is_empty() && st.slice >= 0.0 => atlas::frame_at(st.slice),
                _ if chops.is_empty() => s,
                _ => usize::MAX,
            };
            for i in 0..n {
                let c = Rect::from_min_size(
                    egui::pos2(inner.left() + (i % 8) as f32 * (cw + 4.0), inner.top() + (i / 8) as f32 * (chh + 4.0)),
                    egui::vec2(cw, chh),
                );
                let frame = if chops.is_empty() { i } else { atlas::frame_at((chops[i].start / src_len.max(1e-6)) as f32) };
                p.rect_filled(c, r(4), WELL);
                if let Some((tex, a)) = at {
                    tile(&p, c.shrink(0.8), 3, tex, a.uv_cover(frame, cw / chh), gray(if i == cur_slice { 1.0 } else { 0.6 }));
                }
                if !chops.is_empty() {
                    p.text(c.left_top() + egui::vec2(4.0, 2.0), Align2::LEFT_TOP, (i + 1).to_string(), mono(9.0), CREAM);
                }
                if i == cur_slice {
                    p.rect_stroke(c, r(4), Stroke::new(1.6, AMBER_MID), StrokeKind::Inside);
                }
                if hit(ui, c, id.with(("slice", i)), Sense::click(), &format!("slice {}", i + 1)).clicked() {
                    if chops.is_empty() {
                        self.seq_edit_step(host, scene, lane, s, |x| {
                            x.slice = (i as f32 + 0.5) / 16.0;
                            x.slice_index = None;
                        });
                    } else {
                        self.seq_edit_step(host, scene, lane, s, |x| x.slice_index = Some(i as u16));
                    }
                }
            }

            // 2. Two rows of 3 parameter cells (`Pitch`, `Length`, `Chance` | `Ratchet`, `Offset`, `Chord`).
            let py = inner.top() + 2.0 * (chh + 4.0) + 6.0;
            let row_gap = 6.0;
            let cell_h = ((inner.bottom() - py - row_gap) * 0.5).max(52.0);
            let pw = (inner.width() - 2.0 * 6.0) / 3.0;
            let key = self.key;
            let degree = semis_to_degree(&key, st.pitch);
            let len_i = LENGTHS.iter().position(|(l, _)| (*l - st.length).abs() < 1e-3).unwrap_or(1);
            let ratch_txt = if ratchet_slope.abs() >= 0.5 {
                format!("×{ratchet} {ratchet_slope:+.0}")
            } else {
                format!("×{ratchet}")
            };
            let off_pct = (st.offset * 100.0).round() as i32;
            let off_txt = if off_pct == 0 { "0%".into() } else { format!("{off_pct:+}%") };

            let cells: [(String, &str, &str); 6] = [
                (format!("{degree:+}").replace("+0", "0"), "pitch", "pitch"),
                (LENGTHS[len_i].1.to_string(), "length", "length"),
                (format!("{:.0}%", st.chance * 100.0), "chance", "chance"),
                (ratch_txt, "ratchet", "ratchet"),
                (off_txt, "offset", "offset"),
                (step_chord.label().to_string(), "chord", "step chord"),
            ];

            for (k, (val_s, title, a11y)) in cells.iter().enumerate() {
                let row_i = k / 3;
                let col_i = k % 3;
                let c = Rect::from_min_size(
                    egui::pos2(inner.left() + col_i as f32 * (pw + 6.0), py + row_i as f32 * (cell_h + row_gap)),
                    egui::vec2(pw, cell_h),
                );
                putty_panel(&p, c, 5);
                let lcd_r = Rect::from_center_size(c.center() - egui::vec2(0.0, 7.5), egui::vec2((pw - 14.0).max(40.0), 21.0));
                lcd_readout(&p, lcd_r, val_s, 12.0);
                caps(&p, c.center() + egui::vec2(0.0, 12.5), Align2::CENTER_CENTER, title, 8.5);
                let resp = hit(ui, c, id.with(("param", k)), Sense::click_and_drag(), a11y);

                if k == 3 && resp.clicked() && !resp.dragged() {
                    let next_r = if ratchet >= 8 { 1 } else if ratchet == 1 { 2 } else if ratchet == 2 { 3 } else if ratchet == 3 { 4 } else if ratchet == 4 { 6 } else { 8 };
                    self.set_step_ratchet_ext(host, scene, lane, s, next_r, ratchet_slope, step_chord);
                } else if k == 5 && resp.clicked() && !resp.dragged() {
                    self.set_step_ratchet_ext(host, scene, lane, s, ratchet, ratchet_slope, step_chord.next());
                } else if resp.dragged() {
                    let d = resp.drag_delta();
                    if k == 3 && d.x.abs() > d.y.abs() * 1.2 {
                        let acc_x = ui.data(|dd| dd.get_temp::<f32>(resp.id.with("x")).unwrap_or(0.0)) + d.x;
                        let st_step = (acc_x / 14.0).trunc();
                        if st_step != 0.0 {
                            ui.data_mut(|dd| dd.insert_temp(resp.id.with("x"), acc_x - st_step * 14.0));
                            self.set_step_ratchet_ext(host, scene, lane, s, ratchet, (ratchet_slope + st_step).clamp(-6.0, 6.0), step_chord);
                        } else {
                            ui.data_mut(|dd| dd.insert_temp(resp.id.with("x"), acc_x));
                        }
                    } else {
                        let acc = ui.data(|dd| dd.get_temp::<f32>(resp.id).unwrap_or(0.0)) - d.y;
                        let step = (acc / 12.0).trunc() as i32;
                        if step != 0 {
                            ui.data_mut(|dd| dd.insert_temp(resp.id, acc - step as f32 * 12.0));
                            match k {
                                0 => {
                                    let semis = degree_to_semis(&key, (degree + step).clamp(-14, 14));
                                    self.seq_edit_step(host, scene, lane, s, |x| x.pitch = semis);
                                }
                                1 => {
                                    let ni = (len_i as i32 + step).clamp(0, LENGTHS.len() as i32 - 1) as usize;
                                    self.seq_edit_step(host, scene, lane, s, |x| x.length = LENGTHS[ni].0);
                                }
                                2 => {
                                    let nc = (st.chance + step as f32 * 0.05).clamp(0.0, 1.0);
                                    self.seq_edit_step(host, scene, lane, s, |x| x.chance = nc);
                                }
                                3 => {
                                    let nr = (ratchet as i32 + step).clamp(1, 8) as u8;
                                    self.set_step_ratchet_ext(host, scene, lane, s, nr, ratchet_slope, step_chord);
                                }
                                4 => {
                                    let no = (st.offset + step as f32 * 0.03).clamp(-0.45, 0.45);
                                    self.seq_edit_step(host, scene, lane, s, |x| x.offset = no);
                                }
                                _ => {
                                    self.set_step_ratchet_ext(host, scene, lane, s, ratchet, ratchet_slope, step_chord.step_by(step));
                                }
                            }
                        } else {
                            ui.data_mut(|dd| dd.insert_temp(resp.id, acc));
                        }
                    }
                }
            }
        });

        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|pt| !sheet.contains(pt) && !step_rect.contains(pt))
        });
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
