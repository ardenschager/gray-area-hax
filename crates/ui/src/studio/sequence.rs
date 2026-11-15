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
            pat.swing = self.swing.clamp(0.50, 0.72);
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
        // Under `∞ hold` the arrangement loops the held scene alone, so "current" IS the held
        // scene (presets / kit syncs applied while holding must edit what is actually playing).
        if self.seq.hold_scene {
            let idx = spans.iter().position(|(s, _, _, _)| *s == self.seq.scene);
            return (
                self.seq.scene,
                idx.map(|i| (i, (b.max(0.0) / spans[i].3.max(1e-6)).fract())),
            );
        }
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
            // Live improv `∞ hold`: loop the held scene's WHOLE pattern (a 4-bar groove keeps
            // its full chord progression) until released or switched.
            let s = self.seq.scene.min(SCENES - 1);
            let pid = self.seq.scene_pats[s].or_else(|| self.seq.scene_pats.iter().copied().flatten().next());
            let len = pid
                .and_then(|pid| host.project().lock().unwrap().patterns.get(pid).map(|pat| pat.length_beats))
                .unwrap_or(4.0)
                .clamp(4.0, 64.0);
            pid.into_iter().map(|pid| (pid, 0.0, len)).collect()
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

        // Right-anchored live tools: `Build Set` (`"build set"`), `∞ hold`, `dup`, `+ cue`.
        let mut rx = rect.right();
        let build_w = if rect.width() < 340.0 {
            58.0
        } else if rect.width() < 520.0 {
            64.0
        } else {
            72.0
        };
        let build_r = Rect::from_min_size(egui::pos2(rx - build_w, rect.top()), egui::vec2(build_w, h));
        rx = build_r.left() - 4.0;
        let b_resp = hit(ui, build_r, ui.id().with((id_salt, "build-set")), Sense::click(), "build set");
        let set_built = self.seq.chain.len() >= 4
            && self.seq.scene_names[0] == "intro"
            && self.seq.scene_names[3] == "climax";
        paint_switch_cap(&p, build_r, SwitchKind::Amber, set_built, b_resp.hovered(), b_resp.is_pointer_button_down_on());
        p.text(
            build_r.center(),
            Align2::CENTER_CENTER,
            "Build Set",
            font_semibold(if build_w < 62.0 { 9.2 } else { 10.2 }),
            if set_built { ON_HI } else { Color32::from_rgb(0x24, 0x1c, 0x09) },
        );
        if b_resp.clicked() {
            self.build_four_section_set(host);
            return;
        }

        let hold_w = if rect.width() < 420.0 { 32.0 } else if rect.width() < 540.0 { 44.0 } else { 54.0 };
        let hold_r = Rect::from_min_size(egui::pos2(rx - hold_w, rect.top()), egui::vec2(hold_w, h));
        rx = hold_r.left() - 4.0;
        if chip(ui, hold_r, ui.id().with((id_salt, "hold-scene")), "hold scene", false, self.seq.hold_scene).clicked() {
            self.seq.hold_scene = !self.seq.hold_scene;
            self.rev += 1;
            host.mark();
        }
        p.text(
            hold_r.center(),
            Align2::CENTER_CENTER,
            if hold_w < 42.0 { "∞" } else { "∞ hold" },
            mono_medium(10.0),
            if self.seq.hold_scene { ON_HI } else { INK2 },
        );

        let mut dup_r_opt = None;
        if rect.width() >= 470.0 {
            let dup_r = Rect::from_min_size(egui::pos2(rx - 38.0, rect.top()), egui::vec2(38.0, h));
            rx = dup_r.left() - 4.0;
            dup_r_opt = Some(dup_r);
        }
        let add_w = if rect.width() < 420.0 { 30.0 } else { 46.0 };
        let add_r = Rect::from_min_size(egui::pos2(rx - add_w, rect.top()), egui::vec2(add_w, h));
        rx = add_r.left() - 5.0;

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

    /// Seed the active scene pattern with a skipping 2-Step UKG groove (`61%` swing)
    /// via [`StepPattern::apply_preset`].
    pub fn seed_ukg_2step_groove(&mut self, host: &mut dyn Host) {
        self.apply_pattern_preset(host, chromagrain_core::seq::PatternPreset::Ukg2Step);
    }

    /// Apply any of the 7 zero-AI [`chromagrain_core::seq::PatternPreset`] grooves across the
    /// active scene's rows, syncing lanes, sculpts, ratchets, chords, and `SWING`.
    pub fn apply_pattern_preset(&mut self, host: &mut dyn Host, preset: chromagrain_core::seq::PatternPreset) {
        self.seq_ensure(host);
        let (scene, _) = self.seq_current(host);
        let Some(pid) = self.seq_pattern(scene) else { return };
        let key = self.key;
        let can_tempo = host.caps().tempo;
        let mut any_alpha = false;
        {
            let mut p = host.project().lock().unwrap();
            for s in p.sources.iter_mut() {
                if s.slices.is_empty() {
                    s.slices = chromagrain_core::chop::analyze_source_slices(s, 32);
                }
                if s.video.as_ref().is_some_and(|v| v.has_alpha()) {
                    any_alpha = true;
                }
            }
            if can_tempo {
                p.bpm = preset.default_bpm() as f64;
            }
            let sources = p.sources.clone();
            let Some(pat) = p.patterns.get_mut(pid) else { return };
            // Smart acoustic role assignment: every lane (`Kick · Snare · Hat · VoxPerc · Chord ·
            // Bass · Hook A · Hook B`) is bound to the acoustically best loaded source and its
            // exact curated slice — never to raw load order.
            let _ = chromagrain_core::seq::assign_curated_kit_to_pattern(pat, &sources, key, preset);
            self.swing = pat.swing.clamp(0.50, 0.72);
            self.active_preset = Some(preset);

            // Sync UI lanes, lane sculpts, and step ratchet/chord extensions from seeded rows.
            self.seq.step_ext.retain(|&(sc, _, _), _| sc != scene);
            for i in 0..LANES {
                if let Some(row) = pat.rows.get(i) {
                    self.seq.lanes[i] = (row.source < sources.len()).then_some(row.source);
                    self.lane_sculpt[i] = crate::host::SculptParams::from_core(
                        row.sculpt.stamp,
                        row.sculpt,
                        row.choke_group,
                    );
                    for (st_i, st) in row.steps.iter().enumerate() {
                        let ch = ChordChoice::from_core(st.chord);
                        if st.ratchet > 1 || st.ratchet_pitch.abs() > 1e-3 || ch != ChordChoice::Off {
                            self.seq.step_ext.insert((scene, i, st_i), (st.ratchet.max(1), st.ratchet_pitch, ch));
                        }
                    }
                } else {
                    let fallback = self.columns[i].filter(|&s| s < sources.len());
                    self.seq.lanes[i] = fallback;
                    let r_idx = pat.add_row(fallback.unwrap_or(usize::MAX));
                    pat.rows[r_idx].key = Some(key);
                }
            }
        }
        if preset == chromagrain_core::seq::PatternPreset::AmbientWash {
            host.set_stage_mode(crate::host::StageMode::Stack);
        } else if any_alpha {
            host.set_stage_mode(crate::host::StageMode::Collage);
        } else if matches!(
            host.stage_mode(),
            Some(crate::host::StageMode::Stack | crate::host::StageMode::Bleed)
        ) {
            host.set_stage_mode(crate::host::StageMode::Grid);
        }
        self.rev += 1;
        self.seq_build_arrangement(host);
        host.mark_now();
        host.status(format!(
            "groove preset: {} · {:.0}% swing",
            preset.label(),
            self.swing * 100.0
        ));
    }

    /// Re-bind every scene's lanes to the curated 8-lane role kit across all loaded sources
    /// WITHOUT touching programmed steps (called after `Vocal Kit → pads`, so Perform pads and
    /// Sequence lanes always play the same musical kit). Returns `false` when there is no
    /// programmed pattern to re-bind.
    pub fn sync_lanes_to_curated_kit(&mut self, host: &mut dyn Host) -> bool {
        let key = self.key;
        let (scene, _) = self.seq_current(host);
        let pids: Vec<usize> = self.seq.scene_pats.iter().copied().flatten().collect();
        let Some(main_pid) = self.seq_pattern(scene).or_else(|| pids.first().copied()) else {
            return false;
        };
        {
            let mut p = host.project().lock().unwrap();
            let has_hits = p
                .patterns
                .get(main_pid)
                .is_some_and(|pat| pat.rows.iter().any(|r| r.steps.iter().any(|s| s.on)));
            if !has_hits {
                return false;
            }
            let sources = p.sources.clone();
            let picks = chromagrain_core::chop::build_lane_kit_across_sources_opt(&sources, Some(key));
            if picks.is_empty() {
                return false;
            }
            let mut lanes_now: Vec<Option<usize>> = Vec::new();
            for &pid in &pids {
                if let Some(pat) = p.patterns.get_mut(pid) {
                    let l = chromagrain_core::seq::rebind_pattern_to_lane_picks(pat, &sources, &picks);
                    if pid == main_pid {
                        lanes_now = l;
                        for i in 0..LANES {
                            if let Some(row) = pat.rows.get(i) {
                                self.lane_sculpt[i] =
                                    crate::host::SculptParams::from_core(row.sculpt.stamp, row.sculpt, row.choke_group);
                            }
                        }
                    }
                }
            }
            for i in 0..LANES {
                if let Some(l) = lanes_now.get(i) {
                    self.seq.lanes[i] = *l;
                }
            }
        }
        self.rev += 1;
        self.seq_build_arrangement(host);
        host.mark_now();
        true
    }

    /// 1-click 4-Section Song Expander (`Intro · Drop · Break · Climax`):
    /// expands the active pattern via [`StepPattern::expand_four_sections`] into Scenes `1..4`
    /// and configures the song chain `[1×2, 2×4, 3×2, 4×4]`.
    pub fn build_four_section_set(&mut self, host: &mut dyn Host) {
        self.seq_ensure(host);
        let (cur_scene, _) = self.seq_current(host);
        let has_hits = self.seq_pattern(cur_scene).is_some_and(|pid| {
            host.project()
                .lock()
                .unwrap()
                .patterns
                .get(pid)
                .is_some_and(|pat| pat.rows.iter().any(|r| r.steps.iter().any(|s| s.on)))
        });
        if !has_hits {
            self.apply_pattern_preset(
                host,
                self.active_preset.unwrap_or(chromagrain_core::seq::PatternPreset::Ukg2Step),
            );
        }
        let Some(src_pid) = self.seq_pattern(cur_scene) else { return };
        let key = self.key;
        {
            let mut p = host.project().lock().unwrap();
            let Some(base_pat) = p.patterns.get(src_pid).cloned() else { return };
            let sections = base_pat.expand_four_sections();
            let names = ["intro", "drop", "break", "climax"];
            let sources_len = p.sources.len();

            for (sec_i, mut sec_pat) in sections.into_iter().enumerate() {
                sec_pat.name = names[sec_i].into();
                while sec_pat.rows.len() < LANES {
                    let li = sec_pat.rows.len();
                    let fb = self.seq.lanes[li].or(self.columns[li]).filter(|&s| s < sources_len);
                    let r_idx = sec_pat.add_row(fb.unwrap_or(usize::MAX));
                    sec_pat.rows[r_idx].key = Some(key);
                }
                self.seq.step_ext.retain(|&(sc, _, _), _| sc != sec_i);
                for (li, row) in sec_pat.rows.iter().take(LANES).enumerate() {
                    if sec_i == 1 {
                        self.seq.lanes[li] = (row.source < sources_len).then_some(row.source);
                        self.lane_sculpt[li] = crate::host::SculptParams::from_core(
                            row.sculpt.stamp,
                            row.sculpt,
                            row.choke_group,
                        );
                    }
                    for (st_i, st) in row.steps.iter().enumerate() {
                        let ch = ChordChoice::from_core(st.chord);
                        if st.ratchet > 1 || st.ratchet_pitch.abs() > 1e-3 || ch != ChordChoice::Off {
                            self.seq.step_ext.insert((sec_i, li, st_i), (st.ratchet.max(1), st.ratchet_pitch, ch));
                        }
                    }
                }
                if let Some(existing_pid) = self.seq.scene_pats[sec_i] {
                    if let Some(slot) = p.patterns.get_mut(existing_pid) {
                        *slot = sec_pat;
                    }
                } else {
                    let new_pid = p.add_pattern(sec_pat);
                    self.seq.scene_pats[sec_i] = Some(new_pid);
                }
                self.seq.scene_names[sec_i] = names[sec_i].into();
            }
        }
        self.seq.chain = vec![(0, 2), (1, 4), (2, 2), (3, 4)];
        self.seq.scene = 0;
        self.seq.queued = None;
        self.seq.hold_scene = false;
        self.rev += 1;
        self.seq_build_arrangement(host);
        host.mark_now();
        host.status("built 4-section set · 1 intro ×2 · 2 drop ×4 · 3 break ×2 · 4 climax ×4".into());
    }

    /// Clear all active steps on the current scene pattern.
    pub fn clear_active_pattern(&mut self, host: &mut dyn Host) {
        let (scene, _) = self.seq_current(host);
        let Some(pid) = self.seq_pattern(scene) else { return };
        {
            let mut p = host.project().lock().unwrap();
            if let Some(pat) = p.patterns.get_mut(pid) {
                for row in &mut pat.rows {
                    for st in &mut row.steps {
                        *st = chromagrain_core::seq::Step::default();
                    }
                }
            }
        }
        self.seq.step_ext.retain(|&(sc, _, _), _| sc != scene);
        self.rev += 1;
        host.mark();
    }

    // ------------------------------------------------------------ view

    pub fn sequence_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1120.0;
        let col_gap = if narrow { 10.0 } else { 16.0 };
        let left_w = (area.width() * if narrow { 0.34 } else { 0.35 }).clamp(306.0, 540.0).floor();
        let left_col = Rect::from_min_size(area.min, egui::vec2(left_w, area.height()));
        let lanes_rect = Rect::from_min_max(egui::pos2(left_col.right() + col_gap, area.top()), area.max);

        self.sequence_left_column(host, ui, left_col);
        self.seq_lanes(host, ui, lanes_rect);
        self.step_sheet(host, ui);
    }

    fn sequence_left_column(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, col: Rect) {
        let compact = col.height() < 620.0 || col.width() < 360.0;
        let vgap = if compact { 6.0 } else { 10.0 };

        // 1. Live Stage Monitor + Dedicated Bezel Control Strip (`grid · collage · split · stack`).
        let ideal_stage_h = (col.width() * 9.0 / 16.0 + 30.0).floor();
        let max_stage_h = (col.height() * if compact { 0.42 } else { 0.46 }).max(158.0);
        let stage_block = Rect::from_min_size(col.min, egui::vec2(col.width(), ideal_stage_h.min(max_stage_h)));
        self.stage_with_bezel(host, ui, stage_block, "seq-stage", true);
        let mut y = stage_block.bottom() + vgap;

        // 2. Groove & Pocket Hardware Bay (7-Preset Groove Bank + `Clear` + `SWING` pointer knob + `61%` silver LCD).
        let groove_h = if compact { 82.0 } else { 102.0 };
        let groove_r = Rect::from_min_size(egui::pos2(col.left(), y), egui::vec2(col.width(), groove_h));
        self.sequence_groove_bar(host, ui, groove_r, compact);
        y = groove_r.bottom() + vgap;

        // 3. Bottom Selected Lane Strip (`Lane N · title` + `Open clip ›`).
        let lane_bar_h = if compact { 28.0 } else { 32.0 };
        let lane_bar_top = col.bottom() - lane_bar_h;

        // 4. Compact `SET · SCENES` Bay (standalone).
        if host.caps().scenes && lane_bar_top - y >= 92.0 {
            let max_set_h = if compact { 210.0 } else { 252.0 };
            let set_h = (lane_bar_top - vgap - y).min(max_set_h);
            let set_box = Rect::from_min_size(egui::pos2(col.left(), y), egui::vec2(col.width(), set_h));
            let p = ui.painter().clone();
            putty_panel(&p, set_box, 7);
            let s_in = set_box.shrink2(egui::vec2(if compact { 7.0 } else { 10.0 }, if compact { 5.0 } else { 8.0 }));
            let (cur, _) = self.seq_current(host);
            let mut sy = s_in.top();
            if !compact && s_in.height() >= 180.0 {
                caps(&p, egui::pos2(s_in.left(), sy + 5.0), Align2::LEFT_CENTER, "Set · scenes", 8.5);
                sy += 16.0;
            }

            if s_in.height() >= 132.0 {
                let name_h = if compact { 24.0 } else { 27.0 };
                let name_bar = Rect::from_min_size(egui::pos2(s_in.left(), sy), egui::vec2(s_in.width(), name_h));
                let mut nm = self.seq.scene_names[cur.min(SCENES - 1)].clone();
                let (_, enter) = super::add::field_sized(
                    ui,
                    name_bar,
                    egui::Id::new(("seq-scn-name", cur)),
                    &mut nm,
                    &format!("Scene {} — intro, drop, break…", cur + 1),
                    "scene name",
                    if compact { 10.5 } else { 11.5 },
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
                sy = name_bar.bottom() + if compact { 5.0 } else { 7.0 };
            }

            let chain_h = if compact { 26.0 } else { 30.0 };
            let chain_r = Rect::from_min_size(egui::pos2(s_in.left(), s_in.bottom() - chain_h), egui::vec2(s_in.width(), chain_h));
            let grid_avail = (chain_r.top() - sy - if compact { 4.0 } else { 6.0 }).max(48.0);

            // `6×2` compact hardware scene matrix (`1..12`).
            let sg = if compact { 3.5 } else { 5.0 };
            let cols_n = 6usize;
            let sw = (s_in.width() - sg * (cols_n as f32 - 1.0)) / cols_n as f32;
            let sh = ((grid_avail - sg) * 0.5).clamp(22.0, 52.0);
            for s in 0..SCENES {
                let rc = Rect::from_min_size(
                    egui::pos2(s_in.left() + (s % cols_n) as f32 * (sw + sg), sy + (s / cols_n) as f32 * (sh + sg)),
                    egui::vec2(sw, sh),
                );
                self.scene_cell(host, ui, rc, s, cur);
            }

            self.set_arranger_bar(host, ui, chain_r, "seq-set");
        }

        // Selected Lane Strip at bottom of Left Column.
        if self.seq.lanes.get(self.sel_lane).copied().flatten().is_none() {
            if let Some(first) = self.seq.lanes.iter().position(|l| l.is_some()) {
                self.sel_lane = first;
            }
        }
        let l = self.sel_lane.min(LANES - 1);
        if let Some(src) = self.seq.lanes[l] {
            let ob = Rect::from_min_size(egui::pos2(col.left(), lane_bar_top), egui::vec2(col.width(), lane_bar_h));
            let p = ui.painter();
            putty_panel(p, ob, 6);
            let raw = self.source_name(host, src);
            let clean = super::clean_clip_label(&raw);
            let pill_w = if compact { 82.0 } else { 90.0 };
            let room = ((ob.width() - pill_w - 24.0) / 6.2).max(5.0) as usize;
            p.text(
                egui::pos2(ob.left() + 10.0, ob.center().y),
                Align2::LEFT_CENTER,
                format!("LANE {} · {}", l + 1, super::play::short(&clean, room.min(18))),
                font_semibold(if compact { 10.0 } else { 11.0 }),
                INK,
            );
            let pill = Rect::from_min_size(
                egui::pos2(ob.right() - pill_w - 4.0, ob.center().y - (lane_bar_h - 6.0) * 0.5),
                egui::vec2(pill_w, lane_bar_h - 6.0),
            );
            if open_clip_pill(ui, pill, ui.id().with("seq-open-clip"), "open clip").clicked() {
                self.open_clip(super::ClipTarget::Lane(l));
            }
        }
    }

    fn sequence_groove_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, compact: bool) {
        use chromagrain_core::seq::PatternPreset;
        let p = ui.painter().clone();
        putty_panel(&p, card, 7);
        let inner = card.shrink2(egui::vec2(if compact { 7.0 } else { 10.0 }, if compact { 4.5 } else { 6.5 }));
        let id = ui.id().with("seq-groove");

        // Right side: `SWING` charcoal pointer knob + sunken silver LCD (`61%`).
        let swing_zone_w = if compact { 88.0 } else { 126.0 };
        let swing_r = Rect::from_min_max(egui::pos2(inner.right() - swing_zone_w, inner.top()), inner.max);
        let cur_swing = self.active_swing(host);
        let mut sw_norm = ((cur_swing - 0.50) / 0.22).clamp(0.0, 1.0);
        let prev_norm = sw_norm;
        let krad = (swing_r.height() * 0.26).clamp(11.5, 17.5);
        let kc = egui::pos2(swing_r.center().x, swing_r.top() + krad + if compact { 2.0 } else { 4.0 });
        knob(ui, kc, krad, id.with("swing-knob"), "seq swing", &mut sw_norm, (0.61 - 0.50) / 0.22, 0);
        if (sw_norm - prev_norm).abs() > 1e-4 {
            self.set_active_swing(host, 0.50 + sw_norm * 0.22);
        }
        caps(
            &p,
            egui::pos2(kc.x, kc.y + krad + if compact { 5.5 } else { 7.0 }),
            Align2::CENTER_CENTER,
            "SWING",
            if compact { 7.2 } else { 8.0 },
        );
        let sw_pct = (self.active_swing(host) * 100.0).round() as u32;
        lcd_readout(
            &p,
            Rect::from_center_size(
                egui::pos2(kc.x, (kc.y + krad + if compact { 17.0 } else { 20.0 }).min(swing_r.bottom() - 8.5)),
                egui::vec2((swing_r.width() - 8.0).clamp(44.0, 68.0), if compact { 15.0 } else { 17.0 }),
            ),
            &format!("{sw_pct}%"),
            if compact { 9.2 } else { 10.2 },
        );

        // Left side: `GROOVE · PRESETS` header (+ compact `Clear` pill) + symmetrical `4 × 2` 8-Preset Bank!
        let left_r = Rect::from_min_max(inner.min, egui::pos2(swing_r.left() - if compact { 6.0 } else { 10.0 }, inner.bottom()));
        let hdr_h = if compact { 14.5 } else { 16.5 };
        let clr_w = if compact { 36.0 } else { 44.0 };
        let clr_r = Rect::from_min_size(
            egui::pos2(left_r.right() - clr_w, left_r.top() - 1.0),
            egui::vec2(clr_w, hdr_h),
        );
        let c_resp = hit(ui, clr_r, id.with("clear-pat"), Sense::click(), "clear pattern");
        paint_switch_cap(&p, clr_r, SwitchKind::PuttyAmber, false, c_resp.hovered(), c_resp.is_pointer_button_down_on());
        p.text(
            clr_r.center(),
            Align2::CENTER_CENTER,
            "Clear",
            font_semibold(if compact { 8.4 } else { 9.2 }),
            INK,
        );
        if c_resp.clicked() {
            self.active_preset = None;
            self.clear_active_pattern(host);
        }

        caps(
            &p,
            egui::pos2(left_r.left() + 1.0, left_r.top() + hdr_h * 0.45),
            Align2::LEFT_CENTER,
            if compact { "GROOVE" } else { "GROOVE · PRESETS" },
            if compact { 7.6 } else { 8.5 },
        );
        let grid_top = left_r.top() + hdr_h + if compact { 1.5 } else { 2.5 };
        let grid_h = (left_r.bottom() - grid_top).max(38.0);
        let gx = if compact { 3.0 } else { 4.5 };
        let gy = if compact { 3.0 } else { 4.5 };
        let cw = (left_r.width() - gx * 3.0) / 4.0;
        let ch = (grid_h - gy) * 0.5;
        let narrow_caps = cw < 64.0;

        let presets: [(PatternPreset, &str, &str, &str); 8] = [
            (PatternPreset::Ukg2Step, "2-Step", "2-Step", "seed ukg 2step"),
            (PatternPreset::DiscoFilterPump, "Disco", "Disco", "preset disco"),
            (PatternPreset::BoomBapDialogue, "Boom-Bap", "Boom", "preset boombap"),
            (PatternPreset::KineticRatchet, "Ratchet", "Ratch", "preset ratchet"),
            (PatternPreset::DillaDrunk, "Dilla", "Dilla", "preset dilla"),
            (PatternPreset::Footwork332, "Footwork", "Footwk", "preset footwork"),
            (PatternPreset::TapeDisintegration, "Tape", "Tape", "preset tape"),
            (PatternPreset::AmbientWash, "Wash", "Wash", "preset wash"),
        ];

        for (idx, &(preset, full_lbl, short_lbl, a11y)) in presets.iter().enumerate() {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let rc = Rect::from_min_size(
                egui::pos2(left_r.left() + col_i as f32 * (cw + gx), grid_top + row_i as f32 * (ch + gy)),
                egui::vec2(cw, ch),
            );
            let on = self.active_preset == Some(preset);
            let kind = if preset == PatternPreset::AmbientWash {
                SwitchKind::PuttyMint
            } else {
                SwitchKind::Amber
            };
            let resp = hit(ui, rc, id.with(("preset", idx)), Sense::click(), a11y);
            paint_switch_cap(&p, rc, kind, on, resp.hovered(), resp.is_pointer_button_down_on());
            let fg = if preset == PatternPreset::AmbientWash {
                if on { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { INK }
            } else if on {
                ON_HI
            } else {
                Color32::from_rgb(0x24, 0x1c, 0x09)
            };
            p.text(
                rc.center(),
                Align2::CENTER_CENTER,
                if narrow_caps { short_lbl } else { full_lbl },
                font_semibold(if narrow_caps { 9.0 } else { 10.2 }),
                fg,
            );
            if resp.clicked() {
                self.apply_pattern_preset(host, preset);
            }
        }
    }

    fn seq_lanes(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = if area.height() < 560.0 { 3.5 } else { 4.5 };
        let lh = (area.height() - g * (LANES as f32 - 1.0)) / LANES as f32;
        let hw = (area.width() * 0.145).clamp(78.0, 148.0);
        let sx = area.left() + hw + if area.width() < 760.0 { 6.0 } else { 10.0 };
        let group = if area.width() < 760.0 { 4.0 } else { 7.0 };
        let step_gap = if area.width() < 760.0 { 2.5 } else { 3.5 };
        let sw = (area.right() - sx - step_gap * 15.0 - group * 3.0) / STEPS as f32;
        let step_x = |s: usize| sx + s as f32 * (sw + step_gap) + (s / 4) as f32 * group;
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
                // Calm single recessed slate slot across empty lane (no 16 noisy empty boxes!).
                let empty_row = Rect::from_min_max(header.min, egui::pos2(area.right(), header.bottom()));
                p.rect_filled(empty_row, r(6), Color32::from_rgba_unmultiplied(36, 36, 30, 68));
                p.rect_stroke(empty_row, r(6), hair(LINE2), StrokeKind::Inside);
                monitor_well(&p, header, 6);
                if first_empty {
                    first_empty = false;
                    let resp = hit(ui, header, ui.id().with(("lane-add", lane)), Sense::click(), "add a lane");
                    draw_icon(&p, Icon::Plus, header.center(), 15.0, if resp.hovered() { CREAM } else { CREAM3 });
                    if resp.clicked() {
                        self.open_add(host, crate::host::AddTarget::Lane(lane));
                    }
                }
                continue;
            };
            let len = self.lane_len(host, scene, lane);
            let muted = self.lane_muted(host, scene, lane);
            let steps: Vec<chromagrain_core::seq::Step> = (0..len).filter_map(|s| self.seq_step(host, scene, lane, s)).collect();
            let silent = steps.iter().all(|s| !s.on);
            let at = self.atlas(host, &ctx, src);
            let (chop_starts, has_alpha) = {
                let pp = host.project().lock().unwrap();
                let ha = pp.sources.get(src).and_then(|so| so.video.as_ref()).is_some_and(|v| v.has_alpha());
                let cs: Vec<f32> = pp
                    .sources
                    .get(src)
                    .map(|so| so.slices.iter().map(|sl| (sl.start / so.duration().max(1e-6)) as f32).collect())
                    .unwrap_or_default();
                (cs, ha)
            };
            let is_sel = lane == self.sel_lane;
            let lane_adsr = self.target_env(host, super::clip::EnvTarget::Lane(lane)).unwrap_or_default();
            monitor_well(&p, header, 6);
            if let Some((tex, a)) = at {
                tile(&p, header.shrink(1.2), 5, tex, a.uv_cover(5, hw / lh), gray(if muted { 0.28 } else if silent { 0.50 } else { 0.80 }));
                p.rect_filled(Rect::from_min_size(header.min + egui::vec2(1.0, 1.0), egui::vec2(3.0, lh - 2.0)), r(2), a.swatch);
            }
            let top_scrim = Rect::from_min_size(header.min, egui::vec2(hw, 20.0_f32.min(lh * 0.45)));
            p.rect_filled(top_scrim, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_black_alpha(120));
            let bot_scrim = Rect::from_min_max(egui::pos2(header.left(), (header.bottom() - 22.0).max(header.top())), header.max);
            p.rect_filled(bot_scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, Color32::from_black_alpha(145));

            let stamp = self.lane_sculpt[lane].stamp;
            let mut top_x = header.left() + 7.0;
            if stamp != crate::host::SculptStamp::Raw && hw >= 88.0 {
                let rtag = super::role_tag_for(stamp, None, None);
                let rr = role_tag_pill(&p, egui::pos2(top_x, header.top() + 4.0), rtag, !muted && !silent);
                top_x = rr.right() + 3.0;
            }
            if has_alpha && top_x + 18.0 <= header.right() - 24.0 {
                cutout_badge(&p, egui::pos2(top_x, header.top() + 4.0));
            } else if stamp == crate::host::SculptStamp::Raw && hw >= 76.0 {
                let raw_name = self.source_name(host, src);
                let clean = super::clean_clip_label(&raw_name);
                let room = ((hw - 34.0) / 6.0).max(4.0) as usize;
                p.text(
                    egui::pos2(header.left() + 8.0, header.top() + 5.5),
                    Align2::LEFT_TOP,
                    super::play::short(&clean, room.min(15)),
                    font_medium(9.8),
                    if muted { CREAM3 } else if is_sel { CREAM } else { CREAM2 },
                );
            }

            if !muted {
                if stamp != crate::host::SculptStamp::Raw && lh >= 48.0 {
                    let raw_name = self.source_name(host, src);
                    let clean = super::clean_clip_label(&raw_name);
                    let room = ((hw - 16.0) / 5.8).max(4.0) as usize;
                    p.text(
                        egui::pos2(header.left() + 7.0, header.bottom() - 5.0),
                        Align2::LEFT_BOTTOM,
                        super::play::short(&clean, room.min(16)),
                        font_medium(9.5),
                        if is_sel { CREAM } else { CREAM2 },
                    );
                } else {
                    let env_w = (hw * 0.36).clamp(18.0, 44.0);
                    let env_r = Rect::from_min_size(egui::pos2(header.left() + 8.0, header.bottom() - 15.5), egui::vec2(env_w, 9.5));
                    mini_adsr(&p, env_r, &lane_adsr, if is_sel { 0.92 } else { 0.68 });
                }
            } else {
                p.text(header.left_bottom() + egui::vec2(8.0, -5.0), Align2::LEFT_BOTTOM, "MUTED", font_semibold(9.0), RUBY_MID);
            }
            if len != STEPS {
                p.text(header.right_bottom() + egui::vec2(-6.0, -5.0), Align2::RIGHT_BOTTOM, len.to_string(), mono(9.5), CREAM2);
            }
            if is_sel {
                p.rect_stroke(header, r(6), Stroke::new(1.8, AMBER_MID), StrokeKind::Inside);
            }
            let hresp = hit(ui, header, ui.id().with(("lane-head", lane)), Sense::click(), &format!("lane {}", lane + 1));
            let chev_vis = Rect::from_min_size(egui::pos2(header.right() - 22.0, header.top() + 3.5), egui::vec2(18.5, 18.5));
            let chev_hit = Rect::from_center_size(chev_vis.center(), egui::vec2(23.0, 23.0));
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
                        draw_loop_glyph(&p, egui::pos2(c.left() + 8.0, c.center().y), INK3);
                    }
                    continue;
                }
                let st = steps.get(s).copied().unwrap_or_default();
                let (ext_r, _, ext_c) = self.step_ratchet_ext(scene, lane, s);
                let ratchet = ext_r.max(st.ratchet.max(1));
                let step_chord = if ext_c != ChordChoice::Off { ext_c } else { ChordChoice::from_core(st.chord) };
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
                        p.text(c.left_top() + egui::vec2(4.0, 3.0), Align2::LEFT_TOP, format!("{d:+}"), mono_medium(9.5), CREAM);
                    }
                    if st.chance < 0.999 {
                        p.text(c.right_top() + egui::vec2(-4.0, 3.0), Align2::RIGHT_TOP, format!("{:.0}%", st.chance * 100.0), mono(8.5), CREAM2);
                    }
                    if ratchet > 1 && sw >= 22.0 {
                        let tag_r = Rect::from_min_size(egui::pos2(c.left() + 2.5, c.bottom() - 13.5), egui::vec2(17.5, 11.0));
                        p.rect_filled(tag_r, r(3), Color32::from_black_alpha(210));
                        p.text(tag_r.center(), Align2::CENTER_CENTER, format!("×{ratchet}"), mono_medium(8.2), AMBER_MID);
                    }
                    if step_chord != ChordChoice::Off && sw >= 28.0 {
                        let ch_r = Rect::from_min_size(egui::pos2(c.right() - 25.0, c.bottom() - 13.5), egui::vec2(22.5, 11.0));
                        p.rect_filled(ch_r, r(3), Color32::from_black_alpha(195));
                        p.text(ch_r.center(), Align2::CENTER_CENTER, step_chord.label(), mono_medium(7.8), MINT_MID);
                    }
                    p.rect_stroke(c, r(5), Stroke::new(1.2, AMBER_EDGE), StrokeKind::Inside);
                } else {
                    monitor_well(&p, c, 5);
                    p.circle_filled(c.center(), if s % 4 == 0 { 2.0 } else { 1.3 }, if s % 4 == 0 { CREAM3 } else { Color32::from_white_alpha(45) });
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
