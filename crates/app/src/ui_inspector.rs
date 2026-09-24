//! Inspector: edits the ONE selected thing — a source, a clip, a track or
//! the master bus. Every editable property lives here and only here.

use crate::theme::{self, param, ACCENT, TEXT, TEXT_DIM};
use crate::{App, PlayMode, Sel};
use chromagrain_core::dsp::{FilterKind, FilterSpec};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::music::{Key, ScaleKind, PITCH_CLASS_NAMES};
use chromagrain_core::video::{ColorFilter, ColorFilterMode};
use eframe::egui;

const SCALES: [ScaleKind; 12] = [
    ScaleKind::Chromatic,
    ScaleKind::Major,
    ScaleKind::Minor,
    ScaleKind::HarmonicMinor,
    ScaleKind::MajorPentatonic,
    ScaleKind::MinorPentatonic,
    ScaleKind::Blues,
    ScaleKind::Dorian,
    ScaleKind::Phrygian,
    ScaleKind::Lydian,
    ScaleKind::Mixolydian,
    ScaleKind::WholeTone,
];

fn title(ui: &mut egui::Ui, color: egui::Color32, kind: &str) {
    ui.horizontal(|ui| {
        theme::swatch(ui, color, 12.0);
        ui.label(egui::RichText::new(kind.to_uppercase()).size(10.5).color(TEXT_DIM));
    });
}

impl App {
    pub(crate) fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        match self.sel {
            Sel::None => self.empty_inspector(ui),
            Sel::Source(i) => self.source_inspector(ui, i),
            Sel::Clip(ti, ci) => self.clip_inspector(ui, ti, ci),
            Sel::Track(ti) => self.track_inspector(ui, ti),
            Sel::Master => self.master_inspector(ui),
        }
    }

    fn empty_inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.0);
        ui.label(egui::RichText::new("Nothing selected").heading().color(TEXT));
        ui.add_space(6.0);
        for line in [
            "1  + add media or a ♪ instrument in the Library (or drop files anywhere)",
            "2  drag it onto a track — it lands as a snippet",
            "3  click the clip: flip it to grains or pattern, shape it here",
            "4  space to play, drag the preview to place visuals",
            "5  cut out people / effects: the LOOK buttons at the top of any clip",
        ] {
            ui.label(egui::RichText::new(line).color(TEXT_DIM));
        }
    }

    // ------------------------------------------------------------ source

    fn source_inspector(&mut self, ui: &mut egui::Ui, i: usize) {
        let color = theme::source_color(i);
        let (users, info) = {
            let p = self.project.lock().unwrap();
            let Some(s) = p.sources.get(i) else {
                drop(p);
                self.sel = Sel::None;
                return;
            };
            let users: Vec<(usize, usize, String, &'static str)> = App::source_users(&p, i)
                .into_iter()
                .map(|(ti, ci)| {
                    let c = &p.tracks[ti].clips[ci];
                    (ti, ci, p.tracks[ti].name.clone(), PlayMode::of(c.kind).label())
                })
                .collect();
            (users, (s.duration(), s.audio.is_some(), s.video.is_some()))
        };
        title(ui, color, "media");
        let mut changed = false;
        {
            let mut p = self.project.lock().unwrap();
            let s = &mut p.sources[i];
            ui.add(
                egui::TextEdit::singleline(&mut s.name)
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            );
            ui.label(
                egui::RichText::new(format!(
                    "{:.2}s · {}{}",
                    info.0,
                    if info.2 { "video " } else { "" },
                    if info.1 { "audio" } else { "" }
                ))
                .color(TEXT_DIM),
            );
            theme::section(ui, "pitch");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("base pitch").color(TEXT_DIM));
                changed |= ui
                    .add(egui::DragValue::new(&mut s.base_hz).range(0.0..=4000.0).speed(0.5).suffix(" Hz"))
                    .on_hover_text("the note this media 'is' — anchors key quantize (0 = unknown, A440 relative)")
                    .changed();
                if s.base_hz > 0.0 {
                    let midi = 69.0 + 12.0 * (s.base_hz / 440.0).log2();
                    let n = midi.round() as i32;
                    ui.label(
                        egui::RichText::new(format!(
                            "≈ {}{}",
                            PITCH_CLASS_NAMES[n.rem_euclid(12) as usize],
                            n / 12 - 1
                        ))
                        .color(TEXT_DIM),
                    );
                }
            });
        }

        theme::section(ui, "use it");
        ui.horizontal(|ui| {
            for m in [PlayMode::Snippet, PlayMode::Grains, PlayMode::Pattern] {
                if ui
                    .button(format!("+ {}", m.label()))
                    .on_hover_text(format!("{} — at the playhead on the focused track", m.blurb()))
                    .clicked()
                {
                    let (t, b) = (self.focus_track, self.snap(self.playhead_beats));
                    self.place_source(i, t, b, m);
                }
            }
        });
        theme::hint(ui, "or drag the card from the library onto any track");

        theme::section(ui, "derive new media");
        ui.horizontal(|ui| {
            if ui
                .button("paulstretch")
                .on_hover_text("a spectral wash: time dissolves, pitch stays; video smears along")
                .clicked()
            {
                let f = self.stretch_factor;
                self.start_paulstretch(i, f);
            }
            ui.add(egui::DragValue::new(&mut self.stretch_factor).range(2.0..=64.0).speed(0.25).prefix("×"));
        });
        if info.2 {
            ui.horizontal(|ui| {
                if ui.button("cut out people").on_hover_text("new media: just the people, background removed").clicked() {
                    self.start_segment(i, "person".into());
                }
                if ui.button("cut out subject").on_hover_text("new media: whatever stands out in front").clicked() {
                    self.start_segment(i, "subject".into());
                }
            });
            ui.horizontal(|ui| {
                let go = ui
                    .button("cut out…")
                    .on_hover_text("anything else by name — on macOS words other than people pick the main subject; SAM 3 (tools/segment.py) handles any text prompt")
                    .clicked();
                ui.add(egui::TextEdit::singleline(&mut self.segment_prompt).hint_text("e.g. dancer, dog").desired_width(140.0));
                if go {
                    let p = self.segment_prompt.clone();
                    self.start_segment(i, p);
                }
            });
        }
        theme::hint(ui, "results appear as new cards in the library");

        theme::section(ui, &format!("used by {} clip{}", users.len(), if users.len() == 1 { "" } else { "s" }));
        if users.is_empty() {
            theme::hint(ui, "not on the timeline yet");
        }
        for (ti, ci, track, mode) in &users {
            if ui.selectable_label(false, format!("{mode} on {track}")).clicked() {
                self.select(Sel::Clip(*ti, *ci));
            }
        }

        ui.add_space(14.0);
        ui.add_enabled_ui(users.is_empty(), |ui| {
            if ui
                .button("remove from library")
                .on_disabled_hover_text("remove the clips that use it first")
                .clicked()
            {
                self.delete_source(i);
            }
        });
        if changed {
            self.mark();
        }
    }

    // ------------------------------------------------------------ clip

    fn clip_inspector(&mut self, ui: &mut egui::Ui, ti: usize, ci: usize) {
        let sources: Vec<String> = {
            let p = self.project.lock().unwrap();
            p.sources.iter().map(|s| s.name.clone()).collect()
        };
        let (mode, color, clip_title, track_name) = {
            let p = self.project.lock().unwrap();
            let Some(c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)) else {
                drop(p);
                self.sel = Sel::None;
                return;
            };
            (
                PlayMode::of(c.kind),
                crate::ui_timeline::clip_color(c),
                crate::ui_timeline::clip_title(&p, c),
                p.tracks[ti].name.clone(),
            )
        };

        title(ui, color, &format!("clip on {track_name}"));
        ui.label(egui::RichText::new(&clip_title).heading().color(TEXT));
        ui.add_space(4.0);

        // Play mode — the one switch between snippet / grains / pattern.
        let mut new_mode = mode;
        ui.horizontal(|ui| {
            for m in [PlayMode::Snippet, PlayMode::Grains, PlayMode::Pattern] {
                let b = egui::Button::selectable(new_mode == m, m.label()).min_size(egui::vec2(80.0, 24.0));
                if ui.add(b).on_hover_text(m.blurb()).clicked() {
                    new_mode = m;
                }
            }
        });
        theme::hint(ui, mode.blurb());
        if new_mode != mode {
            self.set_play_mode(ti, ci, new_mode);
            return;
        }

        let mut changed = false;
        let mut goto_source: Option<usize> = None;
        let mut cutout: Option<&'static str> = None;
        let mut p = self.project.lock().unwrap();
        let (has_video, is_part) = {
            let s = p.tracks[ti].clips[ci].source;
            let v = mode != PlayMode::Pattern && p.sources.get(s).is_some_and(|s| s.video.is_some());
            (v, mode != PlayMode::Pattern && crate::ui_synth::part_of(&p, s).is_some())
        };
        let cutting = self.pending_cutout.is_some_and(|(t, c, _)| t == ti && c == ci);
        let clip = &mut p.tracks[ti].clips[ci];

        if is_part {
            ui.add_space(2.0);
            ui.label(egui::RichText::new("♪ instrument — notes & knobs are in the editor under the timeline").color(theme::POLY).size(11.5));
        }

        // Look: the visual tricks, one click each, always in view.
        theme::section(ui, "look");
        if has_video && !is_part {
            ui.horizontal(|ui| {
                if cutting {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(egui::RichText::new("cutting out… (a few seconds per 10 s of video)").color(TEXT_DIM));
                } else {
                    if ui
                        .add(egui::Button::new("cut out people").min_size(egui::vec2(0.0, 26.0)))
                        .on_hover_text("remove the background around people — the clip switches to the cut-out (original stays in the library)")
                        .clicked()
                    {
                        cutout = Some("person");
                    }
                    if ui
                        .add(egui::Button::new("cut out subject").min_size(egui::vec2(0.0, 26.0)))
                        .on_hover_text("keep whatever stands out in front (objects, animals, people)")
                        .clicked()
                    {
                        cutout = Some("subject");
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            for (name, label) in [("tune", "tune to key"), ("crush", "crush"), ("delay", "echo trails"), ("reverb", "smear"), ("compress", "jpeg crunch"), ("tint", "tint")] {
                let Some(kind) = EffectKind::parse(name) else { continue };
                let on = clip.effects.iter().position(|f| f.kind.name() == kind.name());
                let b = egui::Button::selectable(on.is_some(), label).min_size(egui::vec2(0.0, 24.0));
                if ui.add(b).on_hover_text(effect_blurb(&kind)).clicked() {
                    match on {
                        Some(i) => {
                            clip.effects.remove(i);
                        }
                        None => clip.effects.push(AvEffect::new(kind)),
                    }
                    changed = true;
                }
            }
        });
        theme::hint(ui, "each works on sound and picture together · fine-tune under Effects below");

        theme::section(ui, "placement");
        ui.horizontal(|ui| {
            if mode != PlayMode::Pattern {
                theme::swatch(ui, theme::source_color(clip.source), 10.0);
                egui::ComboBox::from_id_salt("clip_src")
                    .selected_text(sources.get(clip.source).cloned().unwrap_or_default())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (si, name) in sources.iter().enumerate() {
                            changed |= ui.selectable_value(&mut clip.source, si, name).changed();
                        }
                    });
                if ui.small_button("↗").on_hover_text("show this media in the library").clicked() {
                    goto_source = Some(clip.source);
                }
            }
        });
        if self.mode == crate::AppMode::Perform {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("loop").color(TEXT_DIM));
                for b in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
                    let label = crate::ui_perform::len_label(b);
                    if ui.selectable_label(clip.length_beats == b, label).clicked() {
                        clip.length_beats = b;
                        changed = true;
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("start").color(TEXT_DIM));
                changed |= ui.add(egui::DragValue::new(&mut clip.start_beat).speed(0.1).range(0.0..=4096.0)).changed();
                ui.label(egui::RichText::new("length").color(TEXT_DIM));
                changed |= ui
                    .add(egui::DragValue::new(&mut clip.length_beats).speed(0.1).range(0.25..=256.0).suffix(" beats"))
                    .changed();
            });
        }

        changed |= picture_section(ui, &mut clip.transform);

        // Mode-specific sound shaping.
        let g = &mut clip.grains;
        match mode {
            PlayMode::Snippet => {
                theme::section(ui, "playback");
                changed |= param("start in media", 0.0, 1.0).tip("where in the media playback begins").show(ui, &mut g.position);
                changed |= param("speed", 0.25, 4.0).log().tip("tempo only — pitch stays").suffix("×").show(ui, &mut g.speed);
                changed |= param("pitch", -24.0, 24.0).tip("transpose — tempo stays").suffix(" st").show(ui, &mut g.pitch);
                changed |= param("fine tune", -100.0, 100.0).suffix(" ct").show(ui, &mut g.detune_cents);
                changed |= param("gain", 0.0, 2.0).tip("also drives opacity").show(ui, &mut g.gain);
                changed |= param("pan", -1.0, 1.0).show(ui, &mut g.pan);
                changed |= param("edge fades", 0.01, 1.0).show(ui, &mut g.envelope);
                let mut rev = g.reverse_prob >= 0.5;
                if ui.checkbox(&mut rev, "reverse (audio + video)").changed() {
                    g.reverse_prob = if rev { 1.0 } else { 0.0 };
                    changed = true;
                }
            }
            PlayMode::Grains => {
                theme::section(ui, "cloud");
                changed |= param("density", 0.5, 200.0).log().suffix("/s").tip("grains per second").show(ui, &mut g.density);
                changed |= param("grain length", 0.005, 1.5).log().suffix(" s").show(ui, &mut g.duration);
                changed |= param("length jitter", 0.0, 1.0).show(ui, &mut g.duration_jitter);
                changed |= param("position", 0.0, 1.0).tip("where in the media grains are read from").show(ui, &mut g.position);
                changed |= param("scan speed", -2.0, 4.0).tip("how fast the read position moves (0 = frozen)").show(ui, &mut g.scan_speed);
                changed |= param("spray", 0.0, 3.0).suffix(" s").tip("random read-position scatter").show(ui, &mut g.spray);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("sync to").color(TEXT_DIM));
                    for (label, v) in [("free", 0.0f32), ("1/4", 1.0), ("1/8", 0.5), ("1/16", 0.25), ("1/32", 0.125)] {
                        let sel = (g.sync_div - v).abs() < 1e-4 || (v == 0.0 && g.sync_div <= 0.0);
                        if ui.selectable_label(sel, label).clicked() {
                            g.sync_div = v;
                            changed = true;
                        }
                    }
                });
                theme::section(ui, "pitch");
                changed |= param("pitch", -24.0, 24.0).suffix(" st").show(ui, &mut g.pitch);
                changed |= param("random pitch", 0.0, 24.0).suffix(" st").show(ui, &mut g.pitch_jitter);
                changed |= param("fine tune", -100.0, 100.0).suffix(" ct").tip("applied after key snapping").show(ui, &mut g.detune_cents);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("voices").color(TEXT_DIM));
                    let mut v = g.voices as i64;
                    if ui.add(egui::DragValue::new(&mut v).range(1..=4)).changed() {
                        g.voices = v as u32;
                        changed = true;
                    }
                    if g.voices > 1 {
                        changed |= ui
                            .add(egui::DragValue::new(&mut g.voice_interval).speed(0.2).range(-24.0..=24.0).prefix("every ").suffix(" st"))
                            .on_hover_text("12 = octaves, 7 = fifths")
                            .changed();
                        changed |= ui
                            .add(egui::DragValue::new(&mut g.voice_detune).speed(0.01).range(0.0..=1.0).prefix("detune "))
                            .changed();
                    }
                });
                theme::section(ui, "shape & space");
                changed |= param("gain", 0.0, 2.0).tip("also drives opacity").show(ui, &mut g.gain);
                changed |= param("pan", -1.0, 1.0).show(ui, &mut g.pan);
                changed |= param("stereo spread", 0.0, 1.0).show(ui, &mut g.pan_spread);
                changed |= param("window", 0.01, 1.0).tip("grain envelope softness").show(ui, &mut g.envelope);
                changed |= param("attack ↔ swell", -1.0, 1.0).tip("-1 percussive · 0 symmetric · 1 reverse swell").show(ui, &mut g.env_skew);
                changed |= param("reverse chance", 0.0, 1.0).show(ui, &mut g.reverse_prob);
            }
            PlayMode::Pattern => {
                theme::section(ui, "pattern");
                theme::hint(ui, "steps and rows are edited in the grid under the timeline.");
                theme::hint(ui, "each row's sound is under its ... button.");
            }
        }

        // Key: one key drives both note snapping and frequency tuning.
        changed |= key_section(ui, clip);

        let n_fx = clip.effects.len();
        crate::theme::group(ui, &count_title("Effects", n_fx), n_fx > 0, |ui| {
            changed |= effects_chain_ui(ui, &mut clip.effects, "clip_fx", true);
        });

        let n_filters = clip.audio_filters.len() + clip.color_filter.is_some() as usize;
        crate::theme::group(ui, &count_title("Filters", n_filters), false, |ui| {
            changed |= filters_ui(ui, clip);
        });

        let n_lanes = clip.automation.len();
        crate::theme::group(ui, &count_title("Automation", n_lanes), n_lanes > 0, |ui| {
            let beats = clip.length_beats;
            changed |= automation_ui(ui, &mut clip.automation, beats);
        });

        crate::theme::group(ui, "Sound ↔ picture links", false, |ui| {
            theme::hint(ui, "1 = fully linked, 0 = independent");
            let l = &mut clip.link;
            changed |= param("gain → opacity", 0.0, 1.0).show(ui, &mut l.gain_to_opacity);
            changed |= param("env → opacity", 0.0, 1.0).show(ui, &mut l.envelope_to_opacity);
            changed |= param("pitch → hue", -30.0, 30.0).suffix("°/st").show(ui, &mut l.pitch_to_hue);
            changed |= param("pitch → rate", 0.0, 1.0).show(ui, &mut l.pitch_to_rate);
            changed |= param("pan → x", 0.0, 1.0).show(ui, &mut l.pan_to_x);
            changed |= param("x → pan", 0.0, 1.0).show(ui, &mut l.x_to_pan);
            changed |= param("y → brightness", 0.0, 1.0).show(ui, &mut l.y_to_brightness);
            changed |= param("scale → loudness", 0.0, 1.0).show(ui, &mut l.scale_to_gain);
            changed |= ui.checkbox(&mut l.reverse_video, "reverse video with audio").changed();
            theme::section(ui, "grain look");
            let v = &mut clip.visual;
            changed |= param("size", 0.2, 8.0).tip("patch size per second of grain").show(ui, &mut v.size_scale);
            changed |= param("glow", 0.0, 1.0).tip("additive blending").show(ui, &mut v.additive);
            changed |= param("y scatter", 0.0, 1.0).show(ui, &mut v.scatter_y);
        });
        drop(p);
        if let Some(what) = cutout {
            self.cut_out_clip(ti, ci, what);
        }

        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("audition").color(TEXT_DIM));
            for (label, note) in [("C", 60u8), ("D", 62), ("E", 64), ("G", 67), ("A", 69), ("C'", 72)] {
                if ui.small_button(label).clicked() {
                    self.trigger_note(note, 110);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("duplicate").on_hover_text("⌘D").clicked() {
                self.duplicate_clip(ti, ci);
            }
            if ui
                .button(egui::RichText::new("delete").color(egui::Color32::from_rgb(240, 120, 110)))
                .on_hover_text("del")
                .clicked()
            {
                self.delete_clip(ti, ci);
                return;
            }
        });

        if let Some(s) = goto_source {
            self.select(Sel::Source(s));
        }
        if changed {
            self.mark();
        }
    }

    // ------------------------------------------------------------ track

    fn track_inspector(&mut self, ui: &mut egui::Ui, ti: usize) {
        let n_tracks = self.project.lock().unwrap().tracks.len();
        if ti >= n_tracks {
            self.sel = Sel::None;
            return;
        }
        title(ui, ACCENT, &format!("track {} of {}", ti + 1, n_tracks));
        let mut changed = false;
        let mut p = self.project.lock().unwrap();
        let end_beats = p.end_beat();
        let n_clips = p.tracks[ti].clips.len();
        let track = &mut p.tracks[ti];
        ui.add(egui::TextEdit::singleline(&mut track.name).font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
        ui.label(egui::RichText::new(format!("{n_clips} clip{}", if n_clips == 1 { "" } else { "s" })).color(TEXT_DIM));

        theme::section(ui, "mix");
        changed |= ui.checkbox(&mut track.muted, "mute").changed();
        changed |= param("level", 0.0, 1.5).tip("audio gain AND video opacity").show(ui, &mut track.level);
        changed |= param("level → opacity", 0.0, 1.0).tip("how much the level fades the picture").show(ui, &mut track.level_to_opacity);
        let mut automated = !track.level_points.is_empty();
        if ui.checkbox(&mut automated, "draw a level curve").changed() {
            track.level_points = if automated {
                vec![chromagrain_core::auto::AutoPoint { beat: 0.0, value: track.level as f64 }]
            } else {
                Vec::new()
            };
            changed = true;
        }
        if !track.level_points.is_empty() {
            let mut lane = chromagrain_core::auto::AutomationLane {
                param: "level".into(),
                points: std::mem::take(&mut track.level_points),
            };
            changed |= lane_strip(ui, &mut lane, (0.0, 1.5), end_beats.max(8.0));
            track.level_points = lane.points;
        }

        theme::section(ui, "track effects");
        theme::hint(ui, "processes everything on this track, sound and picture");
        changed |= effects_chain_ui(ui, &mut track.effects, "track_fx", false);
        drop(p);

        theme::section(ui, "arrange");
        theme::hint(ui, "lower tracks draw on top of higher ones");
        ui.horizontal(|ui| {
            if ui.add_enabled(ti > 0, egui::Button::new("move up")).clicked() {
                self.move_track(ti, true);
            }
            if ui.add_enabled(ti + 1 < n_tracks, egui::Button::new("move down")).clicked() {
                self.move_track(ti, false);
            }
            if ui
                .button(egui::RichText::new("delete track").color(egui::Color32::from_rgb(240, 120, 110)))
                .on_hover_text("removes the track and its clips")
                .clicked()
            {
                self.delete_track(ti);
            }
        });
        if changed {
            self.mark();
        }
    }

    fn master_inspector(&mut self, ui: &mut egui::Ui) {
        title(ui, ACCENT, "master");
        ui.label(egui::RichText::new("Master bus").heading().color(TEXT));
        theme::hint(ui, "effects here process the final mix and the final picture");
        let mut effects = std::mem::take(&mut self.project.lock().unwrap().master_effects);
        theme::section(ui, "effects");
        let changed = effects_chain_ui(ui, &mut effects, "master_fx", false);
        self.project.lock().unwrap().master_effects = effects;
        if changed {
            self.mark();
        }
    }
}

fn count_title(name: &str, n: usize) -> String {
    if n > 0 {
        format!("{name}  ·  {n}")
    } else {
        name.to_string()
    }
}

fn scale_name(s: ScaleKind) -> String {
    let raw = format!("{s:?}");
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn filters_ui(ui: &mut egui::Ui, clip: &mut chromagrain_core::timeline::Clip) -> bool {
    let mut changed = false;
    theme::section(ui, "sound: frequency");
    let mut remove: Option<usize> = None;
    for (i, f) in clip.audio_filters.iter_mut().enumerate() {
        ui.push_id(("audio_filter_row", i), |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("fkind")
                    .selected_text(filter_name(f.kind))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for k in [FilterKind::LowPass, FilterKind::HighPass, FilterKind::BandPass, FilterKind::Notch] {
                            changed |= ui.selectable_value(&mut f.kind, k, filter_name(k)).changed();
                        }
                    });
                if ui.small_button("x").clicked() {
                    remove = Some(i);
                }
            });
            changed |= param("frequency", 30.0, 16000.0).log().suffix(" Hz").show(ui, &mut f.freq);
            changed |= param("resonance", 0.1, 8.0).show(ui, &mut f.q);
        });
    }
    if let Some(i) = remove {
        clip.audio_filters.remove(i);
        changed = true;
    }
    if ui.small_button("+ filter").clicked() {
        clip.audio_filters.push(FilterSpec { kind: FilterKind::LowPass, freq: 2000.0, q: 0.707 });
        changed = true;
    }

    theme::section(ui, "picture: hue band");
    let mut mode = match clip.color_filter {
        None => 0,
        Some(ColorFilter { mode: ColorFilterMode::Keep, .. }) => 1,
        Some(ColorFilter { mode: ColorFilterMode::Remove, .. }) => 2,
    };
    ui.horizontal(|ui| {
        for (v, label) in [(0, "off"), (1, "keep only"), (2, "remove")] {
            if ui.selectable_value(&mut mode, v, label).changed() {
                clip.color_filter = match v {
                    1 => Some(ColorFilter::keep(120.0, 90.0)),
                    2 => Some(ColorFilter::remove(120.0, 90.0)),
                    _ => None,
                };
                changed = true;
            }
        }
    });
    if let Some(cf) = &mut clip.color_filter {
        hue_bar(ui, cf.hue_center, cf.hue_width);
        changed |= param("hue", 0.0, 360.0).suffix("°").show(ui, &mut cf.hue_center);
        changed |= param("width", 5.0, 360.0).suffix("°").show(ui, &mut cf.hue_width);
        changed |= param("softness", 0.0, 1.0).show(ui, &mut cf.softness);
        changed |= param("min saturation", 0.0, 1.0).show(ui, &mut cf.sat_min);
    }
    changed
}

fn filter_name(k: FilterKind) -> &'static str {
    match k {
        FilterKind::LowPass => "low-pass",
        FilterKind::HighPass => "high-pass",
        FilterKind::BandPass => "band-pass",
        FilterKind::Notch => "notch",
    }
}

/// A rainbow strip with the selected hue band marked.
fn hue_bar(ui: &mut egui::Ui, center: f32, width: f32) {
    let w = ui.available_width().min(320.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 10.0), egui::Sense::hover());
    let n = 60;
    for i in 0..n {
        let h = i as f32 / n as f32 * 360.0;
        let (r, g, b) = chromagrain_core::video::hsv_to_rgb(h, 0.8, 0.9);
        let x0 = rect.min.x + i as f32 / n as f32 * w;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(egui::pos2(x0, rect.min.y), egui::vec2(w / n as f32 + 0.5, 10.0)),
            0.0,
            egui::Color32::from_rgb(r, g, b),
        );
    }
    let x = |h: f32| rect.min.x + (h.rem_euclid(360.0) / 360.0) * w;
    let (a, b) = (center - width / 2.0, center + width / 2.0);
    ui.painter().rect_stroke(
        egui::Rect::from_x_y_ranges(x(a).min(x(b))..=x(a).max(x(b)), rect.y_range()),
        2.0,
        egui::Stroke::new(1.5, egui::Color32::WHITE),
        egui::StrokeKind::Outside,
    );
}

/// Freehand automation strip: drag across it to draw the curve.
fn lane_strip(
    ui: &mut egui::Ui,
    lane: &mut chromagrain_core::auto::AutomationLane,
    range: (f32, f32),
    beats: f64,
) -> bool {
    let mut changed = false;
    let width = ui.available_width().clamp(120.0, 340.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 48.0), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG);
    let beats = beats.max(0.25);
    for b in 0..=(beats as usize) {
        let x = rect.min.x + (b as f64 / beats) as f32 * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(if b % 4 == 0 { 44 } else { 26 })),
        );
    }
    let (lo, hi) = range;
    let to_y = |v: f64| -> f32 {
        let t = ((v as f32 - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
        rect.max.y - t * rect.height()
    };
    if lane.points.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "drag to draw",
            egui::FontId::proportional(11.0),
            theme::TEXT_FAINT,
        );
    } else {
        let n = 96;
        let pts: Vec<egui::Pos2> = (0..=n)
            .map(|i| {
                let beat = i as f64 / n as f64 * beats;
                let v = lane.value_at(beat).unwrap_or(0.0);
                egui::pos2(rect.min.x + (i as f32 / n as f32) * rect.width(), to_y(v))
            })
            .collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, ACCENT)));
    }
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let fx = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
            let fy = 1.0 - ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
            let grid = beats / 32.0;
            let beat = (fx * beats / grid).round() * grid;
            lane.set_point(beat, (lo + (hi - lo) * fy) as f64);
            changed = true;
        }
    }
    changed
}

fn automation_ui(ui: &mut egui::Ui, automation: &mut Vec<chromagrain_core::auto::AutomationLane>, beats: f64) -> bool {
    use chromagrain_core::auto::{param_range, AutomationLane, AUTOMATABLE};
    let mut changed = false;
    let mut remove: Option<usize> = None;
    theme::hint(ui, "curves over the clip's length, evaluated per grain");
    for (i, lane) in automation.iter_mut().enumerate() {
        ui.push_id(("auto_lane", i), |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lane.param.replace('_', " ")).strong().size(12.0));
                if ui.small_button("x").clicked() {
                    remove = Some(i);
                }
                if ui.small_button("clear").clicked() {
                    lane.points.clear();
                    changed = true;
                }
            });
            let range = param_range(&lane.param).unwrap_or((0.0, 1.0));
            changed |= lane_strip(ui, lane, range, beats);
        });
    }
    if let Some(i) = remove {
        automation.remove(i);
        changed = true;
    }
    egui::ComboBox::from_id_salt("add_lane").selected_text("+ curve for…").width(140.0).show_ui(ui, |ui| {
        for p in AUTOMATABLE {
            if automation.iter().any(|l| l.param == p) {
                continue;
            }
            if ui.selectable_label(false, p.replace('_', " ")).clicked() {
                automation.push(AutomationLane::new(p));
                changed = true;
            }
        }
    });
    changed
}

/// Always-visible picture placement: sliders plus one-click presets.
fn picture_section(ui: &mut egui::Ui, tr: &mut chromagrain_core::timeline::ClipTransform) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        theme::section(ui, "picture");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("reset").on_hover_text("center, 100%, upright").clicked() {
                *tr = Default::default();
                changed = true;
            }
        });
    });
    theme::hint(ui, "or on the preview: drag = move · corners = size · top knob = rotate");
    changed |= param("x", -1.5, 1.5).tip("also pans the audio").show(ui, &mut tr.x);
    changed |= param("y", -1.5, 1.5).tip("also tilts brightness (up = airy)").show(ui, &mut tr.y);
    let mut pct = tr.scale * 100.0;
    if param("size", 10.0, 250.0).suffix("%").tip("also distance: smaller = quieter").show(ui, &mut pct) {
        tr.scale = pct / 100.0;
        changed = true;
    }
    changed |= param("rotate", -180.0, 180.0).suffix("°").show(ui, &mut tr.rotation);
    ui.horizontal_wrapped(|ui| {
        for (label, s) in [("50%", 0.5f32), ("100%", 1.0), ("fill 135%", 1.35)] {
            if ui.small_button(label).clicked() {
                tr.scale = s;
                changed = true;
            }
        }
        ui.separator();
        for (label, d) in [("-90°", -90.0f32), ("+90°", 90.0)] {
            if ui.small_button(label).clicked() {
                tr.rotation = wrap_deg(tr.rotation + d);
                changed = true;
            }
        }
        if ui.small_button("center").clicked() {
            tr.x = 0.0;
            tr.y = 0.0;
            changed = true;
        }
    });
    changed
}

fn wrap_deg(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// The clip's key, used both to snap notes and to tune the sound itself.
fn key_section(ui: &mut egui::Ui, clip: &mut chromagrain_core::timeline::Clip) -> bool {
    let mut changed = false;
    let tune_idx = clip.effects.iter().position(|e| matches!(e.kind, EffectKind::Tune { .. }));
    let (mut root, mut scale) = clip
        .key
        .map(|k| (k.root, k.scale))
        .or_else(|| {
            tune_idx.and_then(|i| match clip.effects[i].kind {
                EffectKind::Tune { root, scale, .. } => Some((root.round() as i32, scale)),
                _ => None,
            })
        })
        .unwrap_or((9, ScaleKind::MinorPentatonic));
    let mut snap = clip.key.is_some();
    let mut tune = tune_idx.is_some();
    let active = snap || tune;

    theme::section(ui, if active { "key  ♪" } else { "key" });
    let mut key_changed = false;
    ui.horizontal(|ui| {
        key_changed |= key_pickers(ui, "clip_key", &mut root, &mut scale);
        key_colors(ui, root, scale);
    });
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut snap, "snap notes")
            .on_hover_text("every grain / snippet / hit is re-pitched to the nearest note (needs the media's base pitch)")
            .changed()
        {
            key_changed = true;
        }
        if ui
            .checkbox(&mut tune, "tune the sound")
            .on_hover_text("resonators on every note of the key pull ALL frequencies — speech, noise, field audio — onto it. Picture hues snap to the key's colors too.")
            .changed()
        {
            match (tune, tune_idx) {
                (true, None) => clip.effects.insert(
                    0,
                    AvEffect::new(EffectKind::Tune { root: root as f32, scale, amount: 0.8, focus: 0.5 }),
                ),
                (false, Some(i)) => {
                    clip.effects.remove(i);
                }
                _ => {}
            }
            changed = true;
        }
    });
    if snap {
        changed |= param("snap strength", 0.0, 1.0).tip("1 = hard snap · 0 = free").show(ui, &mut clip.grains.quantize_amount);
    }
    if let Some(fx) = clip.effects.iter_mut().find(|e| matches!(e.kind, EffectKind::Tune { .. })) {
        if let EffectKind::Tune { amount, focus, .. } = &mut fx.kind {
            changed |= param("tune amount", 0.0, 1.0).tip("how much of the sound is pulled onto the key").show(ui, amount);
            changed |= param("focus", 0.0, 1.0).tip("narrow = pure ringing notes · wide = more of the original").show(ui, focus);
        }
        changed |= param("colors too", 0.0, 1.0).tip("how much picture hues snap to the key's colors").show(ui, &mut fx.video);
    }
    if key_changed {
        clip.key = snap.then_some(Key::new(root, scale));
        for fx in &mut clip.effects {
            if let EffectKind::Tune { root: r, scale: s, .. } = &mut fx.kind {
                *r = root as f32;
                *s = scale;
            }
        }
        changed = true;
    }
    changed
}

fn key_pickers(ui: &mut egui::Ui, salt: &str, root: &mut i32, scale: &mut ScaleKind) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt((salt, "root"))
        .selected_text(PITCH_CLASS_NAMES[root.rem_euclid(12) as usize])
        .width(46.0)
        .show_ui(ui, |ui| {
            for (i, name) in PITCH_CLASS_NAMES.iter().enumerate() {
                changed |= ui.selectable_value(root, i as i32, *name).changed();
            }
        });
    egui::ComboBox::from_id_salt((salt, "scale"))
        .selected_text(scale_name(*scale))
        .width(130.0)
        .show_ui(ui, |ui| {
            for s in SCALES {
                changed |= ui.selectable_value(scale, s, scale_name(s)).changed();
            }
        });
    changed
}

/// Twelve pitch-class swatches; the key's notes are lit (its palette).
fn key_colors(ui: &mut egui::Ui, root: i32, scale: ScaleKind) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0 * 7.0, 14.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for pc in 0..12 {
        let on = scale.intervals().contains(&(pc - root).rem_euclid(12));
        let hue = chromagrain_core::fx::pitch_class_hue(pc) / 360.0;
        let c: egui::Color32 = egui::ecolor::Hsva::new(hue, if on { 0.8 } else { 0.25 }, if on { 0.95 } else { 0.3 }, 1.0).into();
        let r = egui::Rect::from_min_size(rect.min + egui::vec2(pc as f32 * 7.0, if on { 0.0 } else { 4.0 }), egui::vec2(6.0, if on { 14.0 } else { 6.0 }));
        painter.rect_filled(r, 1.5, c);
    }
    resp.on_hover_text("the key's notes as colors (C = red, 30° per semitone)");
}

/// Editor for an AV effect chain. Returns true when anything changed.
/// `key_locked`: a clip chain, whose tune key is set in the Key section.
fn effects_chain_ui(ui: &mut egui::Ui, effects: &mut Vec<AvEffect>, salt: &str, key_locked: bool) -> bool {
    let mut changed = false;
    let mut remove: Option<usize> = None;
    for (i, fx) in effects.iter_mut().enumerate() {
        ui.push_id((salt, i), |ui| {
            egui::Frame::new()
                .fill(theme::RAISED)
                .corner_radius(5.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(fx.kind.name()).strong().color(ACCENT));
                        theme::hint(ui, effect_blurb(&fx.kind));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    match &mut fx.kind {
                        EffectKind::Crush { downsample, bits } => {
                            changed |= param("down-res", 1.0, 32.0).show(ui, downsample);
                            changed |= param("bits", 1.0, 16.0).show(ui, bits);
                        }
                        EffectKind::Delay { time, feedback, mix, shift_x, shift_y } => {
                            changed |= param("time", 0.02, 2.0).log().suffix(" s").show(ui, time);
                            changed |= param("feedback", 0.0, 0.95).show(ui, feedback);
                            changed |= param("mix", 0.0, 1.0).show(ui, mix);
                            changed |= param("drift x", -0.3, 0.3).show(ui, shift_x);
                            changed |= param("drift y", -0.3, 0.3).show(ui, shift_y);
                        }
                        EffectKind::Reverb { size, damp, mix } => {
                            changed |= param("size", 0.0, 1.0).show(ui, size);
                            changed |= param("damp / blur", 0.0, 1.0).show(ui, damp);
                            changed |= param("mix", 0.0, 1.0).show(ui, mix);
                        }
                        EffectKind::Compress { quality } => {
                            changed |= param("quality", 0.0, 1.0).show(ui, quality);
                        }
                        EffectKind::Tint { hue, amount } => {
                            hue_bar(ui, *hue, 20.0);
                            changed |= param("hue", 0.0, 330.0).tip("red = low rumble … violet = high air").show(ui, hue);
                            changed |= param("smush", 0.0, 1.0).show(ui, amount);
                        }
                        EffectKind::Tune { root, scale, amount, focus } => {
                            let mut r = root.round() as i32;
                            ui.horizontal(|ui| {
                                if key_locked {
                                    theme::hint(ui, "key: set in the key section above");
                                } else if key_pickers(ui, &format!("{salt}_tune{i}"), &mut r, scale) {
                                    *root = r as f32;
                                    changed = true;
                                }
                                key_colors(ui, r, *scale);
                            });
                            changed |= param("amount", 0.0, 1.0).show(ui, amount);
                            changed |= param("focus", 0.0, 1.0).tip("narrow = ringing notes · wide = more original").show(ui, focus);
                        }
                    }
                    changed |= param("on sound", 0.0, 1.0).show(ui, &mut fx.audio);
                    changed |= param("on picture", 0.0, 1.0).show(ui, &mut fx.video);
                });
        });
    }
    if let Some(i) = remove {
        effects.remove(i);
        changed = true;
    }
    ui.horizontal_wrapped(|ui| {
        for name in ["tune", "crush", "delay", "reverb", "compress", "tint"] {
            if ui.small_button(format!("+ {name}")).clicked() {
                effects.push(AvEffect::new(EffectKind::parse(name).unwrap()));
                changed = true;
            }
        }
    });
    changed
}

fn effect_blurb(k: &EffectKind) -> &'static str {
    match k {
        EffectKind::Crush { .. } => "bitcrush ↔ pixelate",
        EffectKind::Delay { .. } => "echo ↔ ghost frames",
        EffectKind::Reverb { .. } => "space ↔ smear",
        EffectKind::Compress { .. } => "codec crunch ↔ jpeg blocks",
        EffectKind::Tint { .. } => "resonance ↔ color",
        EffectKind::Tune { .. } => "frequencies → key ↔ hues → key colors",
    }
}

