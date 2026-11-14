//! Perform mode: a 4×4 grid of looping pads, Koala-sampler style.
//!
//! Model (kept deliberately simple so everything else keeps working):
//! * pad *i* is track *i* of the project while performing — the
//!   arrangement's tracks are stashed in `App::other_tracks`;
//! * a pad's first clip is its loop. Its length snaps to 1…32 beats and
//!   the rest of the track is refilled with copies of it every time the
//!   project changes, so the engine just sees an ordinary looped project;
//! * a pad is ON when its track isn't muted. Pressing a pad queues a
//!   mute flip for the next beat/bar (or now), applied a little early so
//!   the audio engine — which runs ~90 ms ahead — flips right on the line;
//! * selecting a pad selects its loop clip, so the inspector edits it live.

use crate::theme::{self, ACCENT, LINE, RAISED, RAISED_HI, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::{pow2_beats, App, AppMode, BrowseDrag, LaunchQ, PlayMode, Sel, SourceDrag, PADS};
use chromagrain_core::timeline::{ClipKind, Source};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// Keyboard key for each pad, laid out like the grid (4 per row).
pub const PAD_KEYS: [(egui::Key, &str); PADS] = [
    (egui::Key::Num1, "1"),
    (egui::Key::Num2, "2"),
    (egui::Key::Num3, "3"),
    (egui::Key::Num4, "4"),
    (egui::Key::Q, "Q"),
    (egui::Key::W, "W"),
    (egui::Key::E, "E"),
    (egui::Key::R, "R"),
    (egui::Key::A, "A"),
    (egui::Key::S, "S"),
    (egui::Key::D, "D"),
    (egui::Key::F, "F"),
    (egui::Key::Z, "Z"),
    (egui::Key::X, "X"),
    (egui::Key::C, "C"),
    (egui::Key::V, "V"),
];

/// How far ahead of the heard playhead launches are applied (seconds):
/// roughly the audio ring buffer plus a frame.
const LAUNCH_LOOKAHEAD: f64 = 0.1;

struct PadView {
    has: bool,
    on: bool,
    title: String,
    color: Color32,
    mode: PlayMode,
    len: f64,
    source: Option<Source>,
}

pub fn len_label(beats: f64) -> String {
    if beats >= 4.0 {
        let bars = beats / 4.0;
        if bars == 1.0 { "1 bar".into() } else { format!("{bars:.0} bars") }
    } else if beats == 1.0 {
        "1 beat".into()
    } else {
        format!("{beats:.0} beats")
    }
}

impl App {
    // ------------------------------------------------------------ model

    /// Switch between the arrangement and the pads. Each mode keeps its
    /// own tracks and loop; media, patterns and master are shared.
    pub(crate) fn set_mode(&mut self, m: AppMode) {
        if m == self.mode {
            return;
        }
        self.pending.clear();
        self.prev_launch_t = None;
        let first_time = {
            let mut p = self.project.lock().unwrap();
            if self.mode == AppMode::Perform && p.tracks.len() > PADS {
                p.tracks.truncate(PADS);
            }
            std::mem::swap(&mut p.tracks, &mut self.other_tracks);
            let first = m == AppMode::Perform && p.tracks.is_empty();
            if first {
                // Seed the pads with the arrangement's non-pattern media clips so there's
                // something to play straight away while keeping any active SET sequence live.
                let seeds: Vec<_> = self
                    .other_tracks
                    .iter()
                    .filter(|t| t.name != "sequence")
                    .flat_map(|t| t.clips.iter().cloned())
                    .take(PADS)
                    .collect();
                for i in 0..PADS {
                    let ti = p.add_track(&format!("pad {}", i + 1));
                    p.tracks[ti].muted = true;
                    if let Some(mut c) = seeds.get(i).cloned() {
                        c.start_beat = 0.0;
                        if c.grains.sculpt.free_loop_secs.is_none() {
                            c.length_beats = pow2_beats(c.length_beats);
                        }
                        p.tracks[ti].clips.push(c);
                    }
                }
            }
            if m == AppMode::Perform {
                if p.tracks.len() > PADS {
                    p.tracks.truncate(PADS);
                }
                let seq_has_steps = p.patterns.iter().any(|pat| {
                    pat.rows.iter().any(|r| !r.muted && r.steps.iter().any(|s| s.on))
                });
                if seq_has_steps {
                    if let Some(seq_tr) = self
                        .other_tracks
                        .iter()
                        .find(|t| t.name == "sequence" && !t.muted && !t.clips.is_empty())
                        .cloned()
                    {
                        while p.tracks.len() < PADS {
                            let n = p.tracks.len();
                            let ti = p.add_track(&format!("pad {}", n + 1));
                            p.tracks[ti].muted = true;
                        }
                        p.tracks.push(seq_tr);
                    }
                }
            }
            first
        };
        let cur = (self.loop_on, self.loop_start, self.loop_end);
        (self.loop_on, self.loop_start, self.loop_end) = self.other_loop;
        self.other_loop = cur;
        self.mode = m;
        self.cc_mappings.clear();
        // Arrange and perform swap whole track lists: history can't span both.
        self.history.request_clear();
        self.sel = Sel::None;
        self.clip_drag = None;
        self.context_clip = None;
        self.gizmo = None;
        self.expanded_rows.clear();
        self.focus_track = 0;
        self.fill_rev = 0;
        self.perform_fill();
        self.playback.set_loop(self.loop_region());
        self.mark_now();
        if !self.playback.is_playing() {
            self.seek(if self.loop_on { self.loop_start } else { 0.0 });
        }
        self.status = match (m, first_time) {
            (AppMode::Perform, true) => "perform: your clips are on the pads — click one (or press its key) to start it".into(),
            (AppMode::Perform, false) => "perform mode".into(),
            (AppMode::Arrange, _) => "arrange mode — the pads are waiting in perform".into(),
        };
    }

    /// Per-frame perform bookkeeping (loop refill + quantized launches).
    pub(crate) fn perform_update(&mut self) {
        if self.mode != AppMode::Perform {
            return;
        }
        self.perform_fill();
        self.perform_tick();
    }

    /// Make every pad loop: beat-synced pads snap to a power-of-two bar/beat
    /// length and tile across `loop_len`, while free-running tape loop pads
    /// (`free_loop_secs: Some(s)`) stay as one unbroken clip across a `>= 64`-beat
    /// span (`>= 6` passes of the longest free loop) so `loop_pass` advances
    /// continuously for Basinski-style phase drift & disintegration.
    pub(crate) fn perform_fill(&mut self) {
        if self.mode != AppMode::Perform || self.fill_rev == self.project_rev {
            return;
        }
        let mut loop_len = 4.0f64;
        {
            let mut p = self.project.lock().unwrap();
            let bpm = p.bpm.max(20.0);
            while p.tracks.len() < PADS {
                let n = p.tracks.len();
                let ti = p.add_track(&format!("pad {}", n + 1));
                p.tracks[ti].muted = true;
            }
            for t in p.tracks.iter_mut().take(PADS) {
                t.clips.truncate(1);
                let track_free = t.sculpt.free_loop_secs;
                if let Some(c0) = t.clips.first_mut() {
                    c0.start_beat = 0.0;
                    if let Some(s) = c0.grains.sculpt.free_loop_secs.or(track_free) {
                        c0.grains.sculpt.free_loop_secs = Some(s);
                        let min_beats = (s.max(0.5) as f64 * 6.0 * bpm / 60.0).ceil();
                        let bar_aligned = ((min_beats / 4.0).ceil() * 4.0).max(64.0);
                        loop_len = loop_len.max(bar_aligned);
                    } else {
                        c0.length_beats = pow2_beats(c0.length_beats);
                        loop_len = loop_len.max(c0.length_beats);
                    }
                }
            }
            let base_seq_clips: Option<(Vec<chromagrain_core::timeline::Clip>, f64)> = self
                .other_tracks
                .iter()
                .find(|t| t.name == "sequence" && !t.clips.is_empty())
                .map(|t| {
                    let span = t
                        .clips
                        .iter()
                        .map(|c| c.start_beat + c.length_beats)
                        .fold(0.0f64, f64::max)
                        .max(4.0);
                    (t.clips.clone(), span)
                });
            if let Some((_, seq_span)) = &base_seq_clips {
                if p.tracks.get(PADS).is_some_and(|t| t.name == "sequence" && !t.muted) {
                    loop_len = loop_len.max(*seq_span);
                }
            }
            for t in p.tracks.iter_mut().take(PADS) {
                let is_free = t.sculpt.free_loop_secs.is_some()
                    || t.clips
                        .first()
                        .is_some_and(|c0| c0.grains.sculpt.free_loop_secs.is_some());
                if is_free {
                    if let Some(c0) = t.clips.first_mut() {
                        c0.start_beat = 0.0;
                        c0.length_beats = loop_len;
                    }
                } else if let Some(c0) = t.clips.first().cloned() {
                    let n = (loop_len / c0.length_beats).round() as usize;
                    for k in 1..n {
                        let mut c = c0.clone();
                        c.start_beat = k as f64 * c0.length_beats;
                        t.clips.push(c);
                    }
                }
            }
            if let (Some((base_clips, seq_span)), Some(seq_tr)) =
                (base_seq_clips, p.tracks.get_mut(PADS).filter(|t| t.name == "sequence"))
            {
                let reps = ((loop_len / seq_span.max(1.0)).ceil() as usize).max(1);
                let mut tiled = Vec::with_capacity(base_clips.len() * reps);
                for r in 0..reps {
                    let off = r as f64 * seq_span;
                    if off >= loop_len - 1e-6 {
                        break;
                    }
                    for mut c in base_clips.iter().cloned() {
                        if off + c.start_beat < loop_len - 1e-6 {
                            c.start_beat += off;
                            c.length_beats = c.length_beats.min(loop_len - c.start_beat);
                            tiled.push(c);
                        }
                    }
                }
                seq_tr.clips = tiled;
            }
        }
        self.fill_rev = self.project_rev;
        if !self.loop_on || self.loop_start != 0.0 || self.loop_end != loop_len {
            self.loop_on = true;
            self.loop_start = 0.0;
            self.loop_end = loop_len;
            self.playback.set_loop(self.loop_region());
        }
    }

    /// Fire queued pad changes when the (look-ahead) playhead crosses the
    /// next launch boundary — including the wrap back to the loop start.
    fn perform_tick(&mut self) {
        if self.pending.is_empty() {
            self.prev_launch_t = None;
            return;
        }
        if !self.playback.is_playing() || self.launch_q == LaunchQ::Now {
            self.apply_pending();
            return;
        }
        let bpm = self.project.lock().unwrap().bpm;
        let q = self.launch_q.beats();
        let mut t = self.playhead_beats + LAUNCH_LOOKAHEAD * bpm / 60.0;
        if self.loop_on && t >= self.loop_end {
            t -= (self.loop_end - self.loop_start).max(q);
        }
        if let Some(prev) = self.prev_launch_t {
            if t < prev || (t / q).floor() != (prev / q).floor() {
                self.apply_pending();
                return;
            }
        }
        self.prev_launch_t = Some(t);
    }

    fn apply_pending(&mut self) {
        {
            let mut p = self.project.lock().unwrap();
            for (pad, on) in self.pending.drain(..) {
                if let Some(t) = p.tracks.get_mut(pad) {
                    t.muted = !on;
                }
            }
        }
        self.prev_launch_t = None;
        self.mark_now();
    }

    /// Press a pad: start/stop it (quantized). Pressing a queued pad
    /// again cancels. With the transport stopped, starting a pad starts
    /// everything from the top.
    pub(crate) fn toggle_pad(&mut self, i: usize) {
        let slice_hit = {
            let p = self.project.lock().unwrap();
            p.tracks.get(i).filter(|t| t.name.contains(" · ")).and_then(|t| {
                let c = t.clips.first()?;
                let src = p.sources.get(c.source)?;
                let len = src.duration();
                let t0 = c.grains.position as f64 * len;
                let dur = c.grains.duration.max(0.08) as f64;
                let adsr = c.grains.adsr.unwrap_or(chromagrain_core::envelope::Adsr {
                    attack: 0.005,
                    decay: 0.08,
                    sustain: 0.9,
                    release: 0.14,
                    picture: chromagrain_core::envelope::PictureCut::Cut,
                    ..Default::default()
                });
                Some((c.source, t0, dur, adsr, p.layout.uses_tiles(), p.layout_pan))
            })
        };
        if let Some((source, t0, dur, adsr, is_tile, pan_by_tile)) = slice_hit {
            self.fire_pad_slice(i, source, t0, dur, adsr, is_tile, pan_by_tile);
            return;
        }
        let (has, on) = {
            let p = self.project.lock().unwrap();
            p.tracks.get(i).map(|t| (!t.clips.is_empty(), !t.muted)).unwrap_or((false, false))
        };
        if !has {
            self.focus_track = i;
            self.sel = Sel::None;
            self.status = format!("pad {} is empty — drag media onto it (or double-click a library card)", i + 1);
            return;
        }
        let target = match self.pending.iter().find(|(p, _)| *p == i) {
            Some(&(_, t)) => !t,
            None => !on,
        };
        self.pending.retain(|(p, _)| *p != i);
        if target == on {
            return; // cancelled a queued change
        }
        let playing = self.playback.is_playing();
        if playing && self.launch_q != LaunchQ::Now {
            self.pending.push((i, target));
            return;
        }
        if let Some(t) = self.project.lock().unwrap().tracks.get_mut(i) {
            t.muted = !target;
        }
        self.mark_now();
        if target && !playing {
            self.playhead_beats = 0.0;
            self.start_play();
        }
    }

    /// Fire a one-shot slice pad (`PadKind::Chop` from `Slices → pads`),
    /// shaped by the pad's ADSR envelope and claiming a tile in `Layout::Tile`.
    pub(crate) fn fire_pad_slice(
        &mut self,
        pad: usize,
        source: usize,
        t0: f64,
        dur: f64,
        adsr: chromagrain_core::envelope::Adsr,
        is_tile: bool,
        pan_by_tile: bool,
    ) {
        let now = self.app_clock.secs();
        self.live_id += 1;
        let voice = (0x50AD_0000_0000 | ((pad as u64) << 32)) | (self.live_id & 0xFFFF_FFFF);
        let hold_len = (dur - adsr.release_secs() * 0.5).clamp(0.18, dur.max(0.18));
        let off = now + hold_len;
        let end_t = off + adsr.release_secs();
        let slot = if is_tile {
            let s = self.pad_tiles.assign_keyed(voice, pad as u64 + 1, now);
            self.pad_tiles.release(voice, end_t);
            Some(s as u8)
        } else {
            None
        };
        let (sculpt, choke_group, pitch_st, pad_pan, pad_gain, base_hz) = {
            let p = self.project.lock().unwrap();
            let hz = p.sources.get(source).map(|s| s.base_hz).unwrap_or(0.0);
            match p.tracks.get(pad) {
                Some(t) => {
                    let c = t.clips.first();
                    let sc = c.filter(|x| x.grains.sculpt.is_active()).map(|x| x.grains.sculpt).unwrap_or(t.sculpt);
                    let ch = c.filter(|x| x.grains.choke_group > 0).map(|x| x.grains.choke_group).unwrap_or(t.choke_group);
                    let pt = c.map(|x| x.grains.pitch).unwrap_or(0.0);
                    let pn = c.map(|x| x.grains.pan).unwrap_or(0.0);
                    let gn = c.map(|x| x.grains.gain).unwrap_or(0.90);
                    (sc, ch, pt, pn, gn, hz)
                }
                None => (chromagrain_core::grain::VoiceSculpt::default(), 0, 0.0, 0.0, 0.90, hz),
            }
        };
        if (1..=4).contains(&choke_group) {
            let mut to_release = Vec::new();
            for (_, _, prev) in &mut self.live_events {
                if prev.tag.choke_group == choke_group && prev.onset < now - 0.002 {
                    prev.tag.choke_at = Some(now);
                    if prev.tag.voice != 0 {
                        to_release.push(prev.tag.voice);
                    }
                }
            }
            for v in to_release {
                self.playback.release_voice(v);
            }
        }
        let (cols, rows) = self.pad_tiles.grid_at(now);
        let (pan, gain_mul) = match (slot, pan_by_tile) {
            (Some(sl), true) => chromagrain_core::tile::layout_pan(sl as usize, cols, rows),
            _ => (pad_pan, 1.0),
        };
        let pitch_ratio = chromagrain_core::music::semitones_to_ratio(pitch_st + sculpt.pitch_semitones);
        let ev = chromagrain_core::grain::GrainEvent {
            onset: now,
            source_pos: t0,
            duration: (end_t - now).max(0.05) as f32,
            pitch_ratio,
            gain: pad_gain * gain_mul,
            pan,
            envelope: 0.01,
            env_skew: 0.0,
            reverse: false,
            id: voice,
            tag: chromagrain_core::grain::VoiceTag {
                voice,
                env: Some(chromagrain_core::envelope::VoiceEnv { adsr, on: now, off }),
                tile: slot,
                choke_group,
                zoom_punch: 0.08,
                ..Default::default()
            },
            sculpt,
            ..Default::default()
        };
        let key = self.studio.key;
        let (chord_evs, chord_n) = chromagrain_core::grain::expand_chord_grain_inline(&ev, sculpt.chord, base_hz, Some(&key));
        for cev in chord_evs.into_iter().take(chord_n) {
            chromagrain_ui::Host::fire(self, 0, source, cev);
        }
    }

    /// Load media onto a pad (replacing what was there). A pad that was
    /// playing keeps playing with the new sound.
    pub(crate) fn load_pad(&mut self, src: usize, pad: usize, mode: PlayMode) {
        let name = {
            let mut p = self.project.lock().unwrap();
            if src >= p.sources.len() || pad >= PADS {
                return;
            }
            while p.tracks.len() < PADS {
                let n = p.tracks.len();
                let ti = p.add_track(&format!("pad {}", n + 1));
                p.tracks[ti].muted = true;
            }
            let mut clip = crate::build_clip(&mut p, src, 0.0, mode);
            clip.length_beats = if mode == PlayMode::Snippet {
                // Round DOWN so the loop never runs past the media into silence.
                2f64.powf(clip.length_beats.clamp(1.0, 16.0).log2().floor())
            } else {
                pow2_beats(clip.length_beats)
            };
            p.tracks[pad].clips = vec![clip];
            p.sources[src].name.clone()
        };
        self.pending.retain(|(q, _)| *q != pad);
        self.select(Sel::Clip(pad, 0));
        self.status = format!("'{name}' is on pad {} — press {} to play it", pad + 1, PAD_KEYS[pad].1);
        self.mark_now();
    }

    pub(crate) fn clear_pad(&mut self, pad: usize) {
        if let Some(t) = self.project.lock().unwrap().tracks.get_mut(pad) {
            t.clips.clear();
            t.muted = true;
        }
        self.pending.retain(|(q, _)| *q != pad);
        self.cc_mappings.clear();
        self.sel = Sel::None;
        self.focus_track = pad;
        self.mark_now();
    }

    pub(crate) fn set_pad_len(&mut self, pad: usize, beats: f64) {
        if let Some(c) = self.project.lock().unwrap().tracks.get_mut(pad).and_then(|t| t.clips.first_mut()) {
            c.length_beats = beats;
        }
        self.mark_now();
    }

    pub(crate) fn stop_all(&mut self) {
        for t in &mut self.project.lock().unwrap().tracks {
            t.muted = true;
        }
        self.pending.clear();
        self.playback.stop();
        self.playhead_beats = 0.0;
        self.mark_now();
    }

    // ------------------------------------------------------------ view

    pub(crate) fn perform_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let playing = self.playback.is_playing();

        // ---- header: stop all · launch quantize · beat dots · loop
        ui.horizontal(|ui| {
            let any_on = self.project.lock().unwrap().tracks.iter().any(|t| !t.muted);
            let stop = egui::Button::new(egui::RichText::new("■  stop all").strong())
                .fill(if any_on || playing { ACCENT.linear_multiply(0.4) } else { RAISED_HI })
                .min_size(egui::vec2(96.0, 26.0));
            if ui.add(stop).on_hover_text("silence every pad and stop").clicked() {
                self.stop_all();
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("start pads on").color(TEXT_DIM));
            for q in [LaunchQ::Now, LaunchQ::Beat, LaunchQ::Bar] {
                let b = egui::Button::selectable(self.launch_q == q, q.label()).min_size(egui::vec2(0.0, 24.0));
                if ui.add(b).clicked() {
                    self.launch_q = q;
                }
            }
            ui.add_space(12.0);
            // Metronome dots: which beat of the bar we're on.
            let (r, _) = ui.allocate_exact_size(egui::vec2(4.0 * 16.0, 24.0), egui::Sense::hover());
            let beat = self.playhead_beats.floor() as i64 % 4;
            for k in 0..4 {
                let c = Pos2::new(r.left() + 8.0 + k as f32 * 16.0, r.center().y);
                let lit = playing && beat == k;
                let col = if lit { if k == 0 { ACCENT } else { TEXT } } else { LINE };
                ui.painter().circle_filled(c, if k == 0 { 5.0 } else { 4.0 }, col);
            }
            ui.add_space(8.0);
            let loop_len = self.loop_end - self.loop_start;
            ui.label(
                egui::RichText::new(format!("loop {}", len_label(loop_len))).color(TEXT_FAINT),
            )
            .on_hover_text("the longest pad sets the loop; shorter pads repeat inside it");
        });
        ui.add_space(6.0);

        // ---- gather pad info (short lock)
        let pads: Vec<PadView> = {
            let p = self.project.lock().unwrap();
            (0..PADS)
                .map(|i| {
                    let t = p.tracks.get(i);
                    let c = t.and_then(|t| t.clips.first());
                    PadView {
                        has: c.is_some(),
                        on: t.is_some_and(|t| !t.muted),
                        title: c.map(|c| crate::ui_timeline::clip_title(&p, c)).unwrap_or_default(),
                        color: c.map(crate::ui_timeline::clip_color).unwrap_or(LINE),
                        mode: c.map(|c| PlayMode::of(c.kind)).unwrap_or(PlayMode::Snippet),
                        len: c.map(|c| c.length_beats).unwrap_or(4.0),
                        source: c.and_then(|c| match c.kind {
                            ClipKind::Pattern(_) => None,
                            _ => p.sources.get(c.source).cloned(),
                        }),
                    }
                })
                .collect()
        };

        // ---- the grid
        let pattern_open = self.selected_clip().is_some_and(|(ti, _)| pads.get(ti).is_some_and(|p| p.mode == PlayMode::Pattern));
        let gap = 8.0;
        let w = ui.available_width();
        let reserve = if pattern_open { 230.0 } else { 26.0 };
        let pad_w = (w - 3.0 * gap) / 4.0;
        let pad_h = ((ui.available_height() - reserve - 3.0 * gap) / 4.0).clamp(56.0, 150.0);
        let (grid, _) = ui.allocate_exact_size(egui::vec2(w, 4.0 * pad_h + 3.0 * gap), egui::Sense::hover());
        let t_now = ui.input(|i| i.time);
        let mut click: Option<(usize, bool)> = None;
        let mut menu_action: Option<(usize, PadAction)> = None;
        let mut drop_src: Option<(usize, usize)> = None;
        let mut drop_file: Option<(usize, std::path::PathBuf)> = None;
        let mut needs_anim = false;

        for (i, pad) in pads.iter().enumerate() {
            let (row, col) = (i / 4, i % 4);
            let rect = Rect::from_min_size(
                grid.min + egui::vec2(col as f32 * (pad_w + gap), row as f32 * (pad_h + gap)),
                egui::vec2(pad_w, pad_h),
            );
            let resp = ui.interact(rect, ui.id().with(("pad", i)), egui::Sense::click());
            let queued = self.pending.iter().find(|(p, _)| *p == i).map(|&(_, on)| on);
            let selected = self.focus_track == i
                && matches!(self.sel, Sel::Clip(t, _) if t == i) | (self.sel == Sel::None && !pad.has);
            let dnd_hover = resp.dnd_hover_payload::<SourceDrag>().is_some()
                || resp.dnd_hover_payload::<BrowseDrag>().is_some();

            let tex = pad.source.as_ref().and_then(|s| self.thumbnail(&ctx, s).map(|t| (t, s.clone())));
            let painter = ui.painter_at(rect.expand(4.0));
            paint_pad(&painter, rect, i, pad, tex, queued, selected, dnd_hover, resp.hovered(), playing, t_now, self.playhead_beats);
            needs_anim |= queued.is_some();

            if resp.clicked() {
                click = Some((i, ui.input(|inp| inp.modifiers.shift)));
            }
            resp.context_menu(|ui| {
                ui.set_min_width(170.0);
                if pad.has {
                    ui.label(egui::RichText::new(format!("pad {} · {}", i + 1, pad.title)).color(TEXT_DIM));
                    ui.separator();
                    ui.horizontal(|ui| {
                        for m in [PlayMode::Snippet, PlayMode::Grains, PlayMode::Pattern] {
                            if ui.selectable_label(pad.mode == m, m.label()).clicked() {
                                menu_action = Some((i, PadAction::Mode(m)));
                                ui.close();
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for b in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
                            if ui.selectable_label(pad.len == b, len_label(b)).clicked() {
                                menu_action = Some((i, PadAction::Len(b)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("cut out people").on_hover_text("background removed around people; the pad switches to the cut-out").clicked() {
                        menu_action = Some((i, PadAction::CutOut("person")));
                        ui.close();
                    }
                    if ui.button("cut out subject").clicked() {
                        menu_action = Some((i, PadAction::CutOut("subject")));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("clear pad").clicked() {
                        menu_action = Some((i, PadAction::Clear));
                        ui.close();
                    }
                } else {
                    theme::hint(ui, "empty — drag media here");
                }
            });
            if let Some(p) = resp.dnd_release_payload::<SourceDrag>() {
                drop_src = Some((i, p.0));
            }
            if let Some(p) = resp.dnd_release_payload::<BrowseDrag>() {
                drop_file = Some((i, p.0.clone()));
            }
        }
        if needs_anim {
            ctx.request_repaint();
        }

        theme::hint(
            ui,
            "click a pad to start / stop it  ·  keys 1-4 Q-R A-F Z-V  ·  shift-click to edit without launching  ·  right-click: length, mode, cut out people  ·  shift-F fullscreen  ·  tab = arrange",
        );

        // ---- apply interactions (after drawing, lock-free)
        if let Some((i, shift)) = click {
            if pads[i].has {
                self.select(Sel::Clip(i, 0));
                if !shift {
                    self.toggle_pad(i);
                }
            } else {
                self.toggle_pad(i); // explains + focuses the empty pad
            }
        }
        match menu_action {
            Some((i, PadAction::Mode(m))) => {
                self.set_play_mode(i, 0, m);
                self.select(Sel::Clip(i, 0));
            }
            Some((i, PadAction::Len(b))) => self.set_pad_len(i, b),
            Some((i, PadAction::Clear)) => self.clear_pad(i),
            Some((i, PadAction::CutOut(what))) => self.cut_out_clip(i, 0, what),
            None => {}
        }
        if let Some((pad, src)) = drop_src {
            self.load_pad(src, pad, PlayMode::Snippet);
        }
        if let Some((pad, path)) = drop_file {
            self.import_path(path, Some(crate::Place { track: pad, new_track: false, beat: 0.0 }));
        }
    }
}

enum PadAction {
    Mode(PlayMode),
    Len(f64),
    CutOut(&'static str),
    Clear,
}

#[allow(clippy::too_many_arguments)]
fn paint_pad(
    painter: &egui::Painter,
    rect: Rect,
    i: usize,
    pad: &PadView,
    tex: Option<(egui::TextureId, Source)>,
    queued: Option<bool>,
    selected: bool,
    dnd_hover: bool,
    hovered: bool,
    playing: bool,
    t_now: f64,
    playhead: f64,
) {
    let r = 8.0;
    let live = pad.on && playing;
    painter.rect_filled(rect, r, if hovered { RAISED_HI } else { RAISED });

    if pad.has {
        // Picture: cover-fit thumbnail, bright when on.
        if let Some((id, src)) = tex {
            let (fw, fh) = src
                .video
                .as_ref()
                .and_then(|v| v.frames.first().map(|f| (f.width as f32, f.height as f32)))
                .unwrap_or((16.0, 9.0));
            let (ai, ap) = (fw / fh.max(1.0), rect.width() / rect.height());
            let uv = if ai > ap {
                let u = ap / ai;
                Rect::from_min_max(Pos2::new(0.5 - u / 2.0, 0.0), Pos2::new(0.5 + u / 2.0, 1.0))
            } else {
                let v = ai / ap;
                Rect::from_min_max(Pos2::new(0.0, 0.5 - v / 2.0), Pos2::new(1.0, 0.5 + v / 2.0))
            };
            let tint = if pad.on { Color32::WHITE } else { Color32::from_gray(70) };
            painter.add(egui::epaint::RectShape::filled(rect, r, tint).with_texture(id, uv));
        } else {
            let c = if pad.on { pad.color.linear_multiply(0.55) } else { pad.color.linear_multiply(0.14) };
            painter.rect_filled(rect, r, c);
        }
        // Legibility band at the bottom.
        let band = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 24.0), rect.max);
        painter.rect_filled(band, egui::CornerRadius { nw: 0, ne: 0, sw: r as u8, se: r as u8 }, Color32::from_black_alpha(170));
        let max_chars = ((rect.width() - 70.0) / 6.8).max(4.0) as usize;
        let mut title = pad.title.clone();
        if title.chars().count() > max_chars {
            title = title.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…";
        }
        painter.text(
            Pos2::new(band.left() + 10.0, band.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(12.5),
            if pad.on { Color32::WHITE } else { TEXT_DIM },
        );
        painter.text(
            Pos2::new(band.right() - 9.0, band.center().y),
            egui::Align2::RIGHT_CENTER,
            pad.mode.label(),
            egui::FontId::proportional(10.5),
            TEXT_FAINT,
        );
        // Length chip.
        let chip = len_label(pad.len);
        let g = painter.layout_no_wrap(chip, egui::FontId::proportional(10.5), TEXT);
        let cr = Rect::from_min_size(Pos2::new(rect.right() - g.size().x - 16.0, rect.top() + 7.0), g.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(cr, 4.0, Color32::from_black_alpha(150));
        painter.galley(cr.min + egui::vec2(5.0, 2.0), g, TEXT);
        // Loop progress.
        if live {
            let phase = ((playhead % pad.len) / pad.len) as f32;
            let bar = Rect::from_min_size(Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0), egui::vec2((rect.width() - 2.0) * phase, 3.0));
            painter.rect_filled(bar, 1.5, pad.color);
        }
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if dnd_hover { "drop to load" } else { "+ drop media" },
            egui::FontId::proportional(12.0),
            if dnd_hover { ACCENT } else { TEXT_FAINT },
        );
    }

    // Key badge.
    let kr = Rect::from_min_size(rect.min + egui::vec2(7.0, 7.0), egui::vec2(20.0, 18.0));
    painter.rect_filled(kr, 4.0, if pad.on { pad.color } else { Color32::from_black_alpha(150) });
    painter.text(
        kr.center(),
        egui::Align2::CENTER_CENTER,
        PAD_KEYS[i].1,
        egui::FontId::monospace(11.0),
        if pad.on { Color32::BLACK } else { TEXT_DIM },
    );

    // Outline: queued blinks, playing glows, empty is a faint line.
    let blink = (0.5 + 0.5 * (t_now * 7.0).sin()) as f32;
    let stroke = if dnd_hover {
        Stroke::new(2.0, ACCENT)
    } else if let Some(on) = queued {
        let c = if on { pad.color } else { Color32::from_gray(160) };
        Stroke::new(2.0, c.linear_multiply(0.35 + 0.65 * blink))
    } else if pad.on {
        Stroke::new(if live { 2.0 } else { 1.5 }, pad.color)
    } else {
        Stroke::new(1.0, LINE)
    };
    painter.rect_stroke(rect, r, stroke, egui::StrokeKind::Inside);
    if let Some(on) = queued {
        let label = if on { "▶ starts soon" } else { "■ stops soon" };
        let g = painter.layout_no_wrap(label.into(), egui::FontId::proportional(11.0), Color32::WHITE);
        let lr = Rect::from_center_size(rect.center() - egui::vec2(0.0, 6.0), g.size() + egui::vec2(12.0, 6.0));
        painter.rect_filled(lr, 5.0, Color32::from_black_alpha(190));
        painter.galley(lr.min + egui::vec2(6.0, 3.0), g, Color32::WHITE);
    }
    if selected {
        painter.rect_stroke(rect.expand(3.0), r + 2.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    }
}
