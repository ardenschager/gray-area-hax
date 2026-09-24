//! Browse tab: every video on this Mac as a thumbnail grid. Drag a tile
//! onto a track (it's imported on drop), double-click to drop it at the
//! playhead. Collections with a semantic corpus can be searched by meaning.

use crate::browser::BrowseClip;
use crate::theme::{self, ACCENT, BG, RAISED, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::{App, BrowseDrag, Place};
use eframe::egui::{self, Color32, Rect};

const GAP: f32 = 6.0;

impl App {
    fn playhead_place(&self) -> Place {
        Place { track: self.focus_track, new_track: false, beat: self.snap(self.playhead_beats) }
    }

    pub(crate) fn browse_ui(&mut self, ui: &mut egui::Ui) {
        self.browser.ensure_started();
        let cols: Vec<(String, usize, bool)> = self
            .browser
            .collections
            .iter()
            .map(|c| (c.name.clone(), c.clips.len(), c.semantic.is_some()))
            .collect();
        if cols.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                if self.browser.scanning {
                    ui.spinner();
                    theme::hint(ui, "looking for videos on this Mac…");
                } else {
                    theme::hint(ui, "no videos found under ~/Documents, ~/Movies, ~/Downloads…");
                }
            });
            return;
        }

        // Collection picker.
        let cur = self.browser.current.min(cols.len() - 1);
        let mut pick = cur;
        egui::ComboBox::from_id_salt("browse_collection")
            .width(ui.available_width())
            .height(460.0)
            .selected_text(format!("{}   {}", cols[cur].0, cols[cur].1))
            .show_ui(ui, |ui| {
                for (i, (name, n, _)) in cols.iter().enumerate() {
                    ui.selectable_value(&mut pick, i, format!("{name}   {n}"));
                }
            });
        if pick != cur {
            self.browser.current = pick;
        }
        let semantic = cols[pick].2;

        // Search.
        ui.add_space(4.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.browser.query)
                .hint_text(if semantic { "describe a moment, press enter…" } else { "filter by name…" })
                .desired_width(f32::INFINITY),
        );
        if semantic && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.browser.search_by_meaning();
        }
        self.browser.refresh_results();
        let n = self.browser.results.len();
        ui.horizontal(|ui| {
            if self.browser.semantic_busy {
                ui.spinner();
                theme::hint(ui, "searching by meaning…");
            } else if self.browser.is_semantic_view() {
                theme::hint(ui, &format!("top {n} by meaning"));
            } else if semantic && !self.browser.query.is_empty() {
                theme::hint(ui, &format!("{n} word matches · enter = by meaning"));
            } else {
                theme::hint(ui, &format!("{n} clips"));
            }
            if self.browser.scanning {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("shuffle").on_hover_text("shuffle what's showing").clicked() {
                    self.browser.shuffle();
                }
            });
        });
        if let Some(e) = self.browser.last_error.clone() {
            ui.colored_label(Color32::from_rgb(230, 120, 100), e);
        }
        ui.add_space(4.0);

        // Grid.
        let width = ui.available_width();
        let ncols = ((width + GAP) / 132.0).floor().clamp(1.0, 5.0) as usize;
        let tw = (width - GAP * (ncols as f32 - 1.0)) / ncols as f32;
        let th = tw * 9.0 / 16.0;
        let rows = n.div_ceil(ncols);
        let detail_h = if self.browser.selected.is_some() { 132.0 } else { 0.0 };
        let results = self.browser.results.clone();
        egui::ScrollArea::vertical()
            .id_salt(("browse_scroll", pick))
            .auto_shrink([false, false])
            .max_height((ui.available_height() - detail_h).max(120.0))
            .show_rows(ui, th + 20.0, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for c in 0..ncols {
                            let Some(&ci) = results.get(row * ncols + c) else { break };
                            let Some(clip) = self.browser.current().and_then(|col| col.clips.get(ci)).cloned()
                            else {
                                break;
                            };
                            self.browse_tile(ui, &clip, tw, th);
                        }
                    });
                }
            });

        self.browse_detail(ui);
    }

    fn browse_tile(&mut self, ui: &mut egui::Ui, clip: &BrowseClip, tw: f32, th: f32) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(tw, th + 20.0), egui::Sense::click_and_drag());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let selected = self.browser.selected.as_deref() == Some(clip.path.as_path());
        let in_project = self.imported.contains_key(&clip.path);
        let loading = self.loading.contains(&clip.path);
        let painter = ui.painter_at(rect.expand(2.0));
        let img = Rect::from_min_size(rect.min, egui::vec2(tw, th));

        painter.rect_filled(img, 5.0, BG);
        if let Some(tex) = self.browser.thumb(&clip.path) {
            painter.image(tex, img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(img, 5.0, RAISED);
        }
        if resp.hovered() {
            painter.rect_filled(img, 5.0, Color32::from_white_alpha(16));
        }
        if selected {
            painter.rect_stroke(img, 5.0, egui::Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
        }

        let small = egui::FontId::proportional(10.0);
        let badge = |text: String, anchor: egui::Pos2, align: egui::Align2, bg: Color32, fg: Color32| {
            let g = painter.layout_no_wrap(text, small.clone(), fg);
            let r = align.anchor_size(anchor, g.size() + egui::vec2(8.0, 3.0));
            painter.rect_filled(r, 3.0, bg);
            painter.galley(r.min + egui::vec2(4.0, 1.5), g, fg);
        };
        if clip.dur > 0.0 {
            badge(
                format!("{:.0}s", clip.dur),
                img.right_bottom() - egui::vec2(4.0, 4.0),
                egui::Align2::RIGHT_BOTTOM,
                Color32::from_black_alpha(180),
                TEXT,
            );
        }
        if loading {
            badge("loading…".into(), img.left_top() + egui::vec2(4.0, 4.0), egui::Align2::LEFT_TOP, ACCENT, Color32::BLACK);
        } else if in_project {
            badge("in project".into(), img.left_top() + egui::vec2(4.0, 4.0), egui::Align2::LEFT_TOP, ACCENT, Color32::BLACK);
        }

        let max_chars = ((tw / 6.0) as usize).max(4);
        let title = truncate(&clip.title, max_chars);
        painter.text(
            egui::pos2(rect.min.x + 1.0, img.max.y + 4.0),
            egui::Align2::LEFT_TOP,
            title,
            egui::FontId::proportional(11.0),
            if selected { TEXT } else { TEXT_DIM },
        );

        if resp.hovered() && !resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), BrowseDrag(clip.path.clone()));
            self.browser.selected = Some(clip.path.clone());
        }
        if resp.clicked() {
            self.browser.selected = Some(clip.path.clone());
        }
        if resp.double_clicked() {
            let place = self.playhead_place();
            self.import_path(clip.path.clone(), Some(place));
        }
    }

    /// Details + actions for the selected tile, pinned under the grid.
    fn browse_detail(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.browser.selected.clone() else { return };
        let Some(clip) = self.browser.clip_by_path(&path).cloned() else { return };
        ui.add_space(6.0);
        egui::Frame::new().fill(RAISED).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(truncate(&clip.title, 80)).color(TEXT).strong());
            let mut meta = clip.reel.clone();
            if clip.dur > 0.0 {
                meta.push_str(&format!(" · {:.1}s", clip.dur));
            }
            ui.label(egui::RichText::new(truncate(&meta, 80)).color(TEXT_DIM).small());
            let desc: String = clip.desc.lines().skip(1).collect::<Vec<_>>().join(" ");
            if !desc.trim().is_empty() {
                ui.label(egui::RichText::new(truncate(desc.trim(), 170)).color(TEXT_FAINT).small());
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("drop at playhead").on_hover_text("same as double-clicking the tile").clicked() {
                    let place = self.playhead_place();
                    self.import_path(path.clone(), Some(place));
                }
                if ui.button("add to library").clicked() {
                    self.import_path(path.clone(), None);
                }
                if ui.small_button("reveal").on_hover_text(path.display().to_string()).clicked() {
                    let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                }
            });
        });
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
