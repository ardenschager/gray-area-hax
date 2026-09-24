//! Top bar (transport, tempo, MIDI, export), status bar and script console.

use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::{midi, App};
use eframe::egui;

impl App {
    pub(crate) fn transport_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("chromagrain").size(17.0).strong().color(ACCENT));
            ui.add_space(10.0);

            // Mode switch.
            for (m, label, tip) in [
                (crate::AppMode::Arrange, "arrange", "tracks on a timeline  (tab)"),
                (crate::AppMode::Perform, "perform", "loop pads you start and stop live  (tab)"),
            ] {
                let on = self.mode == m;
                let b = egui::Button::new(egui::RichText::new(label).strong().color(if on {
                    egui::Color32::BLACK
                } else {
                    theme::TEXT
                }))
                .fill(if on { ACCENT } else { theme::RAISED })
                .min_size(egui::vec2(74.0, 24.0));
                if ui.add(b).on_hover_text(tip).clicked() {
                    self.set_mode(m);
                }
            }
            ui.add_space(14.0);

            // Transport.
            if ui.button("⏮").on_hover_text("back to start  (enter)").clicked() {
                self.seek(if self.loop_on { self.loop_start } else { 0.0 });
            }
            let playing = self.playback.is_playing();
            let play = egui::Button::new(
                egui::RichText::new(if playing { "■  stop" } else { "▶  play" }).strong(),
            )
            .fill(if playing { ACCENT.linear_multiply(0.55) } else { theme::RAISED_HI })
            .min_size(egui::vec2(78.0, 24.0));
            if ui.add(play).on_hover_text("space").clicked() {
                self.toggle_play();
            }
            if self.mode == crate::AppMode::Arrange {
                let mut loop_on = self.loop_on;
                if ui.toggle_value(&mut loop_on, "⟲ loop").on_hover_text("loop a range of beats").changed() {
                    self.loop_on = loop_on;
                    self.playback.set_loop(self.loop_region());
                }
                if self.loop_on {
                    let mut changed = false;
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.loop_start).speed(0.25).range(0.0..=512.0).prefix("from "))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.loop_end).speed(0.25).range(0.0..=512.0).prefix("to "))
                        .changed();
                    if changed {
                        self.playback.set_loop(self.loop_region());
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(theme::beats_label(self.playhead_beats))
                    .monospace()
                    .size(14.0)
                    .color(theme::TEXT),
            )
            .on_hover_text("bar . beat . sixteenth");
            ui.add_space(8.0);

            // Tempo + snap.
            {
                let mut p = self.project.lock().unwrap();
                let mut bpm = p.bpm;
                let bpm_changed = ui
                    .add(egui::DragValue::new(&mut bpm).range(20.0..=300.0).speed(0.5).suffix(" bpm"))
                    .changed();
                if bpm_changed {
                    p.bpm = bpm;
                }
                let mut q = p.quantize;
                egui::ComboBox::from_id_salt("quantize")
                    .selected_text(format!("snap {}", quantize_label(q)))
                    .width(84.0)
                    .show_ui(ui, |ui| {
                        for div in [1.0, 0.5, 0.25, 0.125] {
                            ui.selectable_value(&mut q, div, quantize_label(div));
                        }
                    });
                p.quantize = q;
                drop(p);
                if bpm_changed {
                    self.mark();
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("export", |ui| {
                    ui.set_min_width(170.0);
                    if ui.button("render video (mp4)…").clicked() {
                        ui.close();
                        self.start_render(false);
                    }
                    if ui.button("render audio (wav)…").clicked() {
                        ui.close();
                        self.start_render(true);
                    }
                    theme::hint(ui, "offline, deterministic render of the whole arrangement");
                });
                ui.toggle_value(&mut self.show_script, "{ } script")
                    .on_hover_text("drive the whole engine with rhai scripts");
                let midi_label = if self.midi.connected.is_some() { "midi (on)" } else { "midi" };
                ui.menu_button(midi_label, |ui| self.midi_menu(ui));
                if let Some(b) = &self.busy {
                    ui.label(egui::RichText::new(b.clone()).color(TEXT_DIM));
                    ui.spinner();
                }
            });
        });
    }

    fn midi_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(260.0);
        theme::section(ui, "input");
        ui.horizontal(|ui| {
            let selected = self
                .midi
                .connected
                .clone()
                .or_else(|| self.midi.ports.get(self.midi_port).cloned())
                .unwrap_or_else(|| "no midi inputs".into());
            egui::ComboBox::from_id_salt("midi_port").selected_text(selected).width(190.0).show_ui(
                ui,
                |ui| {
                    for (i, name) in self.midi.ports.clone().iter().enumerate() {
                        if ui.selectable_label(self.midi_port == i, name).clicked() {
                            self.midi_port = i;
                            match self.midi.connect(i) {
                                Ok(()) => self.status = format!("midi: {name}"),
                                Err(e) => self.status = format!("midi failed: {e}"),
                            }
                        }
                    }
                },
            );
            if ui.small_button("⟳").on_hover_text("rescan").clicked() {
                self.midi.refresh();
            }
        });
        theme::hint(ui, "notes play the selected clip like a sampler (C4 = unity)");

        theme::section(ui, "pads");
        ui.horizontal(|ui| {
            for (label, note) in [("C", 60u8), ("D", 62), ("E", 64), ("G", 67), ("A", 69), ("C'", 72)] {
                if ui.button(label).clicked() {
                    self.trigger_note(note, 110);
                }
            }
        });

        theme::section(ui, "knob mappings");
        let names = self.target_names();
        let mut remove: Option<usize> = None;
        for (i, m) in self.cc_mappings.iter().enumerate() {
            ui.horizontal(|ui| {
                let cc = m.cc.map(|c| format!("CC {c}")).unwrap_or_else(|| "move a knob…".into());
                ui.label(egui::RichText::new(cc).monospace().color(ACCENT));
                ui.label("→");
                ui.label(describe_target(&m.target, &names));
                if ui.small_button("x").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.cc_mappings.remove(i);
        }
        if self.cc_mappings.is_empty() {
            theme::hint(ui, "none yet");
        }
        ui.horizontal(|ui| {
            let clip = self.selected_clip();
            ui.add_enabled_ui(clip.is_some(), |ui| {
                egui::ComboBox::from_id_salt("learn_cc")
                    .selected_text("learn: clip param")
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        if let Some((ti, ci)) = clip {
                            for p in chromagrain_core::auto::AUTOMATABLE {
                                if ui.selectable_label(false, p).clicked() {
                                    self.cc_mappings.push(midi::CcMapping {
                                        cc: None,
                                        target: midi::CcTarget::ClipParam {
                                            track: ti,
                                            clip: ci,
                                            param: p.to_string(),
                                        },
                                    });
                                    self.status = "twist a knob to finish mapping".into();
                                }
                            }
                        }
                    });
            });
            if ui.button("learn: track level").clicked() {
                self.cc_mappings.push(midi::CcMapping {
                    cc: None,
                    target: midi::CcTarget::TrackLevel { track: self.focus_track },
                });
                self.status = "twist a knob to finish mapping".into();
            }
        });
    }

    /// (track names, per-track clip labels) for readable mapping labels.
    fn target_names(&self) -> Vec<(String, Vec<String>)> {
        let p = self.project.lock().unwrap();
        p.tracks
            .iter()
            .map(|t| {
                let clips = t
                    .clips
                    .iter()
                    .map(|c| crate::ui_timeline::clip_title(&p, c))
                    .collect();
                (t.name.clone(), clips)
            })
            .collect()
    }

    pub(crate) fn status_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.status).size(11.5).color(TEXT_DIM));
            if !self.playback.available() {
                ui.label(
                    egui::RichText::new("· no audio device — rendering still works")
                        .size(11.5)
                        .color(egui::Color32::from_rgb(230, 190, 90)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(
                        "space play · del delete · ⌘D duplicate · esc deselect · ⌘scroll zoom · drop files anywhere",
                    )
                    .size(11.0)
                    .color(theme::TEXT_FAINT),
                );
            });
        });
    }

    pub(crate) fn script_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("script").strong());
            theme::hint(ui, "rhai — builds onto the current project");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("x").clicked() {
                    self.show_script = false;
                }
                if ui.button("▶ run").clicked() {
                    self.start_script();
                }
            });
        });
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical().id_salt("script_scroll").show(&mut cols[0], |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.script_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(10),
                );
            });
            egui::ScrollArea::vertical().id_salt("script_log").show(&mut cols[1], |ui| {
                if self.script_log.is_empty() {
                    theme::hint(ui, "output appears here");
                } else {
                    ui.label(egui::RichText::new(&self.script_log).monospace().color(TEXT_DIM));
                }
            });
        });
    }
}

fn describe_target(t: &midi::CcTarget, names: &[(String, Vec<String>)]) -> String {
    match t {
        midi::CcTarget::ClipParam { track, clip, param } => {
            let clip_name = names
                .get(*track)
                .and_then(|(_, c)| c.get(*clip))
                .cloned()
                .unwrap_or_else(|| "?".into());
            format!("{param} of {clip_name}")
        }
        midi::CcTarget::TrackLevel { track } => {
            let n = names.get(*track).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".into());
            format!("level of {n}")
        }
    }
}

pub fn quantize_label(q: f64) -> &'static str {
    if q >= 1.0 {
        "1/4"
    } else if q >= 0.5 {
        "1/8"
    } else if q >= 0.25 {
        "1/16"
    } else {
        "1/32"
    }
}
