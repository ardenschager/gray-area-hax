//! SuperCollider instruments in the app.
//!
//! An instrument part (instrument + knobs + a loop of notes, see
//! `chromagrain_core::sc`) renders into ONE library source. Everything
//! about the part is edited in ONE place: the instrument editor that opens
//! under the timeline / pads whenever the part — or any clip or pad
//! playing it — is selected. Edits re-render in the background (debounced)
//! and the new audio swaps into the same source, so every clip and pad
//! using it follows.

use crate::theme::{self, ACCENT, PANEL, RAISED, TEXT, TEXT_DIM};
use crate::{App, AppMode, PlayMode, Sel};
use chromagrain_core::audio::AudioClip;
use chromagrain_core::sc::{self, Note, SynthPart, INSTRUMENTS, KIT};
use chromagrain_core::timeline::{ClipKind, Project, Source};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Quiet period after the last edit before re-rendering.
const DEBOUNCE: Duration = Duration::from_millis(120);

struct Rendered {
    gen: u64,
    src: usize,
    key: u64,
    result: Result<AudioClip, String>,
}

#[derive(Clone, Copy)]
enum RollDrag {
    /// Moving note `idx`; `grab` = pointer beat minus note start.
    Move { idx: usize, grab: f64 },
    Resize { idx: usize },
}

pub struct SynthState {
    tx: mpsc::Sender<Rendered>,
    rx: mpsc::Receiver<Rendered>,
    /// Bumped whenever source ids shift, so late renders are dropped.
    gen: u64,
    /// source -> (last seen render key, when it last changed).
    seen: HashMap<usize, (u64, Instant)>,
    rendered: HashMap<usize, u64>,
    in_flight: HashMap<usize, u64>,
    /// (sound fingerprint, pitch) -> rendered audition note.
    notes: Arc<Mutex<HashMap<(u64, u8), Arc<Vec<f32>>>>>,
    drag: Option<RollDrag>,
    /// Pitch last auditioned during a drag (re-audition on change only).
    drag_pitch: u8,
    /// Lowest visible piano-roll pitch, per source.
    roll_low: HashMap<usize, u8>,
    /// Length of the last note drawn/resized — the next one copies it.
    last_len: f64,
}

impl SynthState {
    pub fn new() -> SynthState {
        let (tx, rx) = mpsc::channel();
        SynthState {
            tx,
            rx,
            gen: 0,
            seen: HashMap::new(),
            rendered: HashMap::new(),
            in_flight: HashMap::new(),
            notes: Arc::new(Mutex::new(HashMap::new())),
            drag: None,
            drag_pitch: 0,
            roll_low: HashMap::new(),
            last_len: 1.0,
        }
    }

    /// Source ids shifted (a source was deleted): forget per-source state.
    pub fn reset_ids(&mut self) {
        self.gen += 1;
        self.seen.clear();
        self.rendered.clear();
        self.in_flight.clear();
        self.roll_low.clear();
        self.drag = None;
    }
}

/// Index of the part rendering into `src`.
pub fn part_of(p: &Project, src: usize) -> Option<usize> {
    p.synth_parts.iter().position(|s| s.source == src)
}

/// Fingerprint of an instrument's sound (instrument + knobs, not notes).
fn sound_key(part: &SynthPart, sr: u32) -> u64 {
    let mut p = part.clone();
    p.notes.clear();
    p.length_beats = 0.0;
    p.render_key(0.0, sr)
}

fn in_key(key: Option<chromagrain_core::music::Key>, pitch: u8) -> bool {
    match key {
        Some(k) => k.scale.intervals().contains(&((pitch as i32 - k.root).rem_euclid(12))),
        None => ![1, 3, 6, 8, 10].contains(&(pitch % 12)),
    }
}

impl App {
    // ------------------------------------------------------------ parts

    /// Add an instrument from the library menu: a fresh part with starter
    /// notes, placed right away (new track at the playhead, or the first
    /// empty pad) and selected so its editor is open.
    pub(crate) fn add_instrument(&mut self, id: &str) {
        let (src, beats) = {
            let mut p = self.project.lock().unwrap();
            let sr = p.sample_rate;
            let mut part = SynthPart::new(id, p.sources.len());
            let secs = p.beats_to_secs(part.length_beats);
            let silent = AudioClip::new(vec![0.0; (secs * sr as f64) as usize], sr);
            let (audio, video) = sc::part_media(silent);
            let src = p.add_source(Source {
                name: sc::part_name(&part),
                audio: Some(audio),
                video: Some(video),
                base_hz: part.base_hz(),
                ..Default::default()
            });
            part.source = src;
            let beats = part.length_beats;
            p.synth_parts.push(part);
            (src, beats)
        };
        self.last_source = src;
        if self.mode == AppMode::Perform {
            self.place_source(src, crate::PADS, 0.0, PlayMode::Snippet);
        } else {
            let t = {
                let mut p = self.project.lock().unwrap();
                let n = p.tracks.len();
                p.add_track(&format!("track {}", n + 1))
            };
            let b = self.snap(self.playhead_beats);
            self.place_source(src, t, b, PlayMode::Snippet);
            // A part's clip covers its whole loop.
            if let Some((ti, ci)) = self.selected_clip() {
                if let Some(c) = self.project.lock().unwrap().tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                    c.length_beats = beats;
                }
            }
        }
        let label = sc::instrument(id).map(|i| i.label).unwrap_or(id);
        self.status = format!("added {label} — edit notes and knobs below, space to play");
        self.mark_now();
    }

    /// Per frame: collect finished renders and start new ones for parts
    /// whose sound changed (after a short quiet period).
    pub(crate) fn synth_update(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.synth.rx.try_recv() {
            if r.gen != self.synth.gen {
                continue;
            }
            self.synth.in_flight.remove(&r.src);
            self.synth.rendered.insert(r.src, r.key);
            match r.result {
                Ok(audio) => {
                    let (a, v) = sc::part_media(audio);
                    {
                        let mut p = self.project.lock().unwrap();
                        let hz = part_of(&p, r.src).map(|pi| p.synth_parts[pi].base_hz());
                        if let (Some(s), Some(hz)) = (p.sources.get_mut(r.src), hz) {
                            s.audio = Some(a);
                            s.video = Some(v);
                            s.base_hz = hz;
                        }
                    }
                    self.thumbs.remove(&r.src);
                    self.peaks.remove(&r.src);
                    self.mark_now();
                }
                Err(e) => self.status = format!("instrument render failed: {e}"),
            }
        }

        let (bpm, sr, parts) = {
            let p = self.project.lock().unwrap();
            (p.bpm, p.sample_rate, p.synth_parts.clone())
        };
        let now = Instant::now();
        let mut waiting = false;
        for part in parts {
            let key = part.render_key(bpm, sr);
            let seen = self.synth.seen.entry(part.source).or_insert((key, now - DEBOUNCE));
            if seen.0 != key {
                *seen = (key, now);
            }
            let changed_at = seen.1;
            if self.synth.rendered.get(&part.source) == Some(&key) {
                continue;
            }
            waiting = true;
            if self.synth.in_flight.contains_key(&part.source) || changed_at.elapsed() < DEBOUNCE {
                continue;
            }
            self.synth.in_flight.insert(part.source, key);
            let tx = self.synth.tx.clone();
            let gen = self.synth.gen;
            std::thread::spawn(move || {
                let result = sc::render_part(&part, bpm, sr);
                let _ = tx.send(Rendered { gen, src: part.source, key, result });
            });
        }
        if waiting {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    /// Play one note of `part` right now (cached per sound + pitch).
    fn audition(&self, part: &SynthPart, pitch: u8, vel: f32) {
        let (bpm, sr) = {
            let p = self.project.lock().unwrap();
            (p.bpm, p.sample_rate)
        };
        let key = (sound_key(part, sr), pitch);
        let out = self.playback.auditioner();
        if let Some(buf) = self.synth.notes.lock().unwrap().get(&key).cloned() {
            out.play(buf);
            return;
        }
        let cache = self.synth.notes.clone();
        let part = part.clone();
        std::thread::spawn(move || {
            if let Ok(v) = sc::render_note(&part, pitch, vel.max(0.6), bpm, sr) {
                let buf = Arc::new(v);
                out.play(buf.clone());
                let mut c = cache.lock().unwrap();
                if c.len() > 256 {
                    c.clear();
                }
                c.insert(key, buf);
            }
        });
    }

    /// The part the current selection is "about", if any.
    pub(crate) fn selected_part(&self) -> Option<usize> {
        let p = self.project.lock().unwrap();
        let src = match self.sel {
            Sel::Source(s) => s,
            Sel::Clip(ti, ci) => {
                let c = p.tracks.get(ti)?.clips.get(ci)?;
                if matches!(c.kind, ClipKind::Pattern(_)) {
                    return None;
                }
                c.source
            }
            _ => return None,
        };
        part_of(&p, src)
    }

    // ------------------------------------------------------------ editor

    pub(crate) fn synth_editor_ui(&mut self, ui: &mut egui::Ui, pi: usize) {
        let (mut part, bpm, sr, quant, clip_ctx, key) = {
            let p = self.project.lock().unwrap();
            let Some(part) = p.synth_parts.get(pi).cloned() else { return };
            let clip = self.selected_clip().and_then(|(ti, ci)| p.tracks.get(ti).and_then(|t| t.clips.get(ci)).cloned());
            let key = clip.as_ref().and_then(|c| c.key);
            (part, p.bpm, p.sample_rate, p.quantize.max(0.125), clip, key)
        };
        let before = part.clone();
        let Some(inst) = sc::instrument(&part.instrument) else { return };
        let mut auditions: Vec<(u8, f32)> = Vec::new();
        let mut switch_to: Option<&'static str> = None;
        let rendering = self.synth.in_flight.contains_key(&part.source)
            || self.synth.rendered.get(&part.source) != Some(&part.render_key(bpm, sr));

        egui::Frame::new().fill(PANEL).corner_radius(6.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // ---- header: instrument, loop length, sound actions
            ui.horizontal_wrapped(|ui| {
                theme::swatch(ui, theme::source_color(part.source), 12.0);
                egui::ComboBox::from_id_salt(("instrument", part.source))
                    .selected_text(egui::RichText::new(format!("♪ {}", inst.label)).strong().color(TEXT))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for i in INSTRUMENTS {
                            if ui.selectable_label(i.id == inst.id, i.label).on_hover_text(i.blurb).clicked() {
                                switch_to = Some(i.id);
                            }
                        }
                    });
                theme::hint(ui, inst.blurb);
                ui.add_space(10.0);
                ui.label(egui::RichText::new("loop").color(TEXT_DIM));
                for bars in [1.0, 2.0, 4.0, 8.0] {
                    let beats = bars * 4.0;
                    let label = if bars == 1.0 { "1 bar".to_string() } else { format!("{bars:.0} bars") };
                    if ui.selectable_label(part.length_beats == beats, label).clicked() {
                        stretch_loop(&mut part, beats);
                    }
                }
                ui.add_space(10.0);
                if ui.button("shuffle sound").on_hover_text("randomize the knobs").clicked() {
                    for k in inst.knobs {
                        let t: f32 = fastrand_f32(&part, k.name);
                        part.set_param(k.name, from_unit(k, 0.15 + 0.7 * t));
                    }
                }
                if ui.button("reset").on_hover_text("knobs back to the preset").clicked() {
                    for k in inst.knobs {
                        part.set_param(k.name, k.default);
                    }
                }
                if ui.button("starter notes").on_hover_text("replace the notes with the instrument's example").clicked() {
                    let (len, notes) = (inst.starter)();
                    part.notes = notes;
                    part.length_beats = len;
                }
                if ui.button("clear").on_hover_text("remove all notes").clicked() {
                    part.notes.clear();
                }
                if rendering {
                    ui.add(egui::Spinner::new().size(12.0));
                }
            });
            ui.add_space(6.0);

            // ---- knobs
            ui.horizontal_wrapped(|ui| {
                for k in inst.knobs {
                    let mut v = part.param(k.name).unwrap_or(k.default);
                    if knob(ui, k, &mut v) {
                        part.set_param(k.name, v);
                    }
                }
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    theme::hint(ui, "drag knobs up/down · double-click resets");
                    theme::hint(ui, "the sound re-renders as you go");
                });
            });
            ui.add_space(6.0);

            // ---- piano roll
            let playhead = clip_ctx.as_ref().filter(|_| self.playback.is_playing()).map(|c| {
                let t = (self.playhead_beats - c.start_beat).max(0.0);
                t % part.length_beats.max(0.25)
            });
            self.piano_roll(ui, &mut part, inst.kit, quant, key, playhead, &mut auditions);
            theme::hint(
                ui,
                "click empty space to add a note (drag to draw it longer) · drag notes to move · drag a note's right edge to resize · right-click deletes · scroll on a note = velocity · click the keys to hear them",
            );
        });

        if let Some(id) = switch_to {
            if id != part.instrument {
                let was_kit = inst.kit;
                let fresh = SynthPart::new(id, part.source);
                part.instrument = fresh.instrument.clone();
                part.params = fresh.params.clone();
                let now_kit = sc::instrument(id).is_some_and(|i| i.kit);
                if was_kit != now_kit || part.notes.is_empty() {
                    part.notes = fresh.notes;
                    part.length_beats = fresh.length_beats;
                }
                let mut p = self.project.lock().unwrap();
                if let Some(s) = p.sources.get_mut(part.source) {
                    s.name = sc::part_name(&part);
                }
            }
        }
        for (pitch, vel) in auditions {
            self.audition(&part, pitch, vel);
        }
        if part != before {
            let mut p = self.project.lock().unwrap();
            if let Some(slot) = p.synth_parts.get_mut(pi) {
                *slot = part;
            }
            drop(p);
            // A part's clips keep covering its loop when the loop changes.
            if clip_ctx.is_some_and(|c| c.length_beats == before.length_beats) {
                if let Some((ti, ci)) = self.selected_clip() {
                    let mut p = self.project.lock().unwrap();
                    let len = p.synth_parts.get(pi).map(|s| s.length_beats).unwrap_or(4.0);
                    if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                        if c.length_beats != len {
                            c.length_beats = len;
                            drop(p);
                            self.mark_now();
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn piano_roll(
        &mut self,
        ui: &mut egui::Ui,
        part: &mut SynthPart,
        kit: bool,
        quant: f64,
        key: Option<chromagrain_core::music::Key>,
        playhead: Option<f64>,
        auditions: &mut Vec<(u8, f32)>,
    ) {
        const GUTTER: f32 = 58.0;
        let rows: Vec<u8> = if kit {
            KIT.iter().rev().map(|d| d.0).collect()
        } else {
            let low = *self.synth.roll_low.entry(part.source).or_insert_with(|| {
                let (lo, hi) = part.notes.iter().fold((127u8, 0u8), |(a, b), n| (a.min(n.pitch), b.max(n.pitch)));
                let centre = if lo <= hi { (lo as i32 + hi as i32) / 2 } else { 60 };
                (centre - 12).clamp(12, 96) as u8
            });
            (low..low + 25).rev().collect()
        };
        let row_h = if kit { 26.0 } else { 9.0 };
        let width = ui.available_width();
        let height = row_h * rows.len() as f32;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let grid = Rect::from_min_max(Pos2::new(rect.left() + GUTTER, rect.top()), rect.max);
        let len = part.length_beats.max(0.25);
        let beat_w = grid.width() as f64 / len;
        let x_of = |b: f64| grid.left() + (b * beat_w) as f32;
        let beat_of = |x: f32| ((x - grid.left()) as f64 / beat_w).clamp(0.0, len);
        let row_of = |y: f32| (((y - rect.top()) / row_h).floor() as usize).min(rows.len() - 1);
        let y_of = |ri: usize| rect.top() + ri as f32 * row_h;

        // Rows.
        painter.rect_filled(rect, 4.0, Color32::from_rgb(16, 16, 19));
        for (ri, &pitch) in rows.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(grid.left(), y_of(ri)), Vec2::new(grid.width(), row_h));
            let fill = if kit {
                if ri % 2 == 0 { Color32::from_rgb(24, 24, 28) } else { Color32::from_rgb(20, 20, 24) }
            } else if in_key(key, pitch) {
                Color32::from_rgb(26, 26, 31)
            } else {
                Color32::from_rgb(17, 17, 20)
            };
            painter.rect_filled(r, 0.0, fill);
            if !kit && pitch % 12 == 0 {
                painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, theme::LINE));
            }
            // Gutter: piano keys or drum names.
            let g = Rect::from_min_size(Pos2::new(rect.left(), y_of(ri)), Vec2::new(GUTTER - 4.0, row_h));
            let black = !kit && [1, 3, 6, 8, 10].contains(&(pitch % 12));
            let key_fill = if kit {
                theme::RAISED_HI
            } else if black {
                Color32::from_gray(38)
            } else {
                Color32::from_gray(196)
            };
            painter.rect_filled(g.shrink2(Vec2::new(0.0, 0.5)), 2.0, key_fill);
            let label = if kit {
                KIT.iter().find(|d| d.0 == pitch).map(|d| d.3.to_string())
            } else {
                (pitch % 12 == 0).then(|| sc::note_name(pitch))
            };
            if let Some(l) = label {
                painter.text(
                    g.left_center() + Vec2::new(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    l,
                    egui::FontId::proportional(if kit { 11.5 } else { 9.0 }),
                    if kit { TEXT } else { Color32::from_gray(30) },
                );
            }
        }
        // Beat / bar lines.
        let mut b = 0.0;
        while b <= len + 1e-9 {
            let x = x_of(b);
            let (w, c) = if (b % 4.0).abs() < 1e-9 {
                (1.0, Color32::from_gray(70))
            } else if (b % 1.0).abs() < 1e-9 {
                (1.0, Color32::from_gray(42))
            } else {
                (0.5, Color32::from_gray(28))
            };
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(w, c));
            b += quant;
        }

        // Notes.
        let color = theme::source_color(part.source);
        let note_rect = |n: &Note| -> Option<Rect> {
            let ri = rows.iter().position(|&p| p == n.pitch)?;
            let x0 = x_of(n.start);
            let x1 = x_of((n.start + n.len).min(len)).max(x0 + 3.0);
            Some(Rect::from_min_max(Pos2::new(x0, y_of(ri) + 0.5), Pos2::new(x1, y_of(ri) + row_h - 0.5)))
        };
        let pointer = resp.hover_pos().or_else(|| ui.input(|i| i.pointer.interact_pos()));
        let hit = |part: &SynthPart, pos: Pos2| -> Option<(usize, bool)> {
            part.notes.iter().enumerate().rev().find_map(|(i, n)| {
                let r = note_rect(n)?;
                r.expand2(Vec2::new(2.0, 0.0)).contains(pos).then(|| (i, pos.x > r.right() - 6.0))
            })
        };
        for n in &part.notes {
            let Some(r) = note_rect(n) else { continue };
            let a = 0.45 + 0.55 * n.vel.clamp(0.0, 1.0);
            let c = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (a * 255.0) as u8);
            painter.rect_filled(r, 2.0, c);
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, color.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        }
        if let Some(t) = playhead {
            let x = x_of(t);
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.5, ACCENT));
        }

        // Cursor feedback.
        if let Some(pos) = resp.hover_pos() {
            if grid.contains(pos) {
                match hit(part, pos) {
                    Some((_, true)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                    Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                    None => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
                }
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }

        // ---- interaction
        let (pressed, down, secondary) =
            ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.secondary_clicked()));
        let snap = |b: f64| (b / quant).floor() * quant;
        if pressed && resp.hovered() {
            if let Some(pos) = pointer {
                if pos.x < grid.left() {
                    // Piano key: just listen.
                    auditions.push((rows[row_of(pos.y)], 0.8));
                } else if let Some((idx, edge)) = hit(part, pos) {
                    let n = part.notes[idx];
                    self.synth.drag = Some(if edge {
                        RollDrag::Resize { idx }
                    } else {
                        RollDrag::Move { idx, grab: beat_of(pos.x) - n.start }
                    });
                    self.synth.drag_pitch = n.pitch;
                    auditions.push((n.pitch, n.vel));
                } else {
                    let pitch = rows[row_of(pos.y)];
                    let start = snap(beat_of(pos.x)).min(len - quant);
                    let l = if kit { quant } else { self.synth.last_len.clamp(quant, len - start) };
                    part.notes.push(Note { start, len: l, pitch, vel: 0.8 });
                    self.synth.drag = Some(RollDrag::Resize { idx: part.notes.len() - 1 });
                    self.synth.drag_pitch = pitch;
                    auditions.push((pitch, 0.8));
                }
            }
        }
        if !down {
            self.synth.drag = None;
        }
        if let (Some(drag), Some(pos)) = (self.synth.drag, pointer) {
            match drag {
                RollDrag::Move { idx, grab } => {
                    if let Some(n) = part.notes.get_mut(idx) {
                        n.start = snap(beat_of(pos.x) - grab + quant * 0.5).clamp(0.0, (len - n.len.min(len)).max(0.0));
                        let pitch = rows[row_of(pos.y.clamp(rect.top(), rect.bottom() - 1.0))];
                        if pitch != n.pitch {
                            n.pitch = pitch;
                            if pitch != self.synth.drag_pitch {
                                self.synth.drag_pitch = pitch;
                                auditions.push((pitch, n.vel));
                            }
                        }
                    }
                }
                RollDrag::Resize { idx } => {
                    if let Some(n) = part.notes.get_mut(idx) {
                        let end = ((beat_of(pos.x) / quant).ceil() * quant).clamp(n.start + quant, len);
                        n.len = end - n.start;
                        if !kit {
                            self.synth.last_len = n.len;
                        }
                    }
                }
            }
        }
        if secondary {
            if let Some(pos) = resp.hover_pos() {
                if let Some((idx, _)) = hit(part, pos) {
                    part.notes.remove(idx);
                    self.synth.drag = None;
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pos) = resp.hover_pos() {
                // The first click of a double-click on empty space made a
                // note; a double-click on a note removes it.
                if let Some((idx, _)) = hit(part, pos) {
                    part.notes.remove(idx);
                    self.synth.drag = None;
                }
            }
        }
        // Scroll: velocity on a note, otherwise pitch range.
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                if let Some((idx, _)) = pointer.and_then(|p| hit(part, p)) {
                    let n = &mut part.notes[idx];
                    n.vel = (n.vel + dy * 0.004).clamp(0.05, 1.0);
                } else if !kit {
                    let low = self.synth.roll_low.entry(part.source).or_insert(48);
                    let step = (dy / 12.0).round() as i32;
                    *low = (*low as i32 + step).clamp(12, 100) as u8;
                }
            }
        }
        // Keep the part tidy: sorted, nothing hanging off the loop.
        if self.synth.drag.is_none() {
            part.notes.retain(|n| n.start < len);
            part.notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        }
    }
}

/// Change the loop length: repeat the notes to fill a longer loop, drop
/// the ones past a shorter one.
fn stretch_loop(part: &mut SynthPart, beats: f64) {
    let old = part.length_beats.max(0.25);
    if beats > old {
        let base: Vec<Note> = part.notes.clone();
        let mut off = old;
        while off < beats - 1e-9 {
            for n in &base {
                if n.start + off < beats {
                    part.notes.push(Note { start: n.start + off, ..*n });
                }
            }
            off += old;
        }
    } else {
        part.notes.retain(|n| n.start < beats);
        for n in &mut part.notes {
            n.len = n.len.min(beats - n.start);
        }
    }
    part.length_beats = beats;
}

fn to_unit(k: &sc::Knob, v: f32) -> f32 {
    if k.log && k.lo > 0.0 {
        ((v.max(k.lo) / k.lo).ln() / (k.hi / k.lo).ln()).clamp(0.0, 1.0)
    } else {
        ((v - k.lo) / (k.hi - k.lo)).clamp(0.0, 1.0)
    }
}

fn from_unit(k: &sc::Knob, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if k.log && k.lo > 0.0 {
        k.lo * (k.hi / k.lo).powf(t)
    } else {
        k.lo + (k.hi - k.lo) * t
    }
}

/// Deterministic-looking but fresh randomness without a new dependency.
fn fastrand_f32(part: &SynthPart, salt: &str) -> f32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    salt.hash(&mut h);
    part.render_key(0.0, 0).hash(&mut h);
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).hash(&mut h);
    (h.finish() % 10_000) as f32 / 10_000.0
}

fn fmt_value(k: &sc::Knob, v: f32) -> String {
    if k.hi >= 1000.0 {
        if v >= 1000.0 { format!("{:.1}k", v / 1000.0) } else { format!("{v:.0}") }
    } else if k.hi > 20.0 {
        format!("{v:.0}")
    } else if k.hi > 2.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// A rotary knob: drag up/down (shift = fine), double-click resets.
fn knob(ui: &mut egui::Ui, k: &sc::Knob, v: &mut f32) -> bool {
    let size = Vec2::new(64.0, 66.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let mut changed = false;
    let mut t = to_unit(k, *v);
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let d = resp.drag_delta();
        t = (t + (-d.y + d.x) * if fine { 0.0015 } else { 0.006 }).clamp(0.0, 1.0);
        *v = from_unit(k, t);
        changed = true;
    }
    if resp.double_clicked() {
        *v = k.default;
        t = to_unit(k, *v);
        changed = true;
    }
    let active = resp.dragged() || resp.hovered();
    let painter = ui.painter_at(rect.expand(2.0));
    let c = Pos2::new(rect.center().x, rect.top() + 24.0);
    let r = 18.0;
    let (a0, a1) = (std::f32::consts::PI * 0.75, std::f32::consts::PI * 2.25);
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        let n = 32;
        (0..=n).map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + Vec2::new(a.cos(), a.sin()) * r
        }).collect()
    };
    painter.circle_filled(c, r - 3.0, if active { theme::RAISED_HI } else { RAISED });
    painter.add(egui::Shape::line(arc(a0, a1), Stroke::new(3.0, Color32::from_gray(44))));
    if t > 0.001 {
        painter.add(egui::Shape::line(arc(a0, a0 + (a1 - a0) * t), Stroke::new(3.0, ACCENT)));
    }
    let a = a0 + (a1 - a0) * t;
    painter.line_segment([c + Vec2::new(a.cos(), a.sin()) * 5.0, c + Vec2::new(a.cos(), a.sin()) * (r - 5.0)], Stroke::new(2.0, TEXT));
    let text = if active { fmt_value(k, *v) } else { k.label.to_string() };
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(11.0),
        if active { TEXT } else { TEXT_DIM },
    );
    let tip = if k.tip.is_empty() { k.label.to_string() } else { format!("{} — {}", k.label, k.tip) };
    resp.on_hover_text(tip);
    changed
}
