//! Library: every piece of media in the project in one list — imported
//! files, YouTube scrapes, synths and derived artifacts alike. Cards are
//! dragged onto the timeline (or into a pattern) to be used.

use crate::theme::{self, ACCENT, RAISED, RAISED_HI, TEXT, TEXT_DIM};
use crate::{App, BrowseDrag, LibTab, PlayMode, Sel, SourceDrag};
use chromagrain_core::timeline::Source;
use eframe::egui;
use std::sync::Arc;

impl App {
    pub(crate) fn library_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let n_src = self.project.lock().unwrap().sources.len();
            let tab = |ui: &mut egui::Ui, cur: &mut LibTab, t: LibTab, label: String| {
                let on = *cur == t;
                let text = egui::RichText::new(label).size(15.0).color(if on { TEXT } else { TEXT_DIM });
                let r = ui.add(egui::Button::new(text).frame(false));
                if on {
                    let y = r.rect.bottom() + 1.0;
                    ui.painter().line_segment(
                        [egui::pos2(r.rect.left(), y), egui::pos2(r.rect.right(), y)],
                        egui::Stroke::new(2.0, ACCENT),
                    );
                }
                if r.clicked() {
                    *cur = t;
                }
            };
            tab(ui, &mut self.lib_tab, LibTab::Project, format!("Project · {n_src}"));
            let total = self.browser.total_clips();
            let browse = if total > 0 { format!("Browse · {total}") } else { "Browse".into() };
            tab(ui, &mut self.lib_tab, LibTab::Browse, browse);
            if self.lib_tab == LibTab::Project {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(egui::RichText::new("+ add").strong(), |ui| self.add_menu(ui));
                });
            }
        });
        ui.add_space(6.0);
        if self.lib_tab == LibTab::Browse {
            theme::hint(ui, "videos on this Mac · drag onto a track · double-click = at playhead");
            ui.add_space(4.0);
            self.browse_ui(ui);
            return;
        }
        theme::hint(
            ui,
            if self.mode == crate::AppMode::Perform {
                "drag media onto a pad · double-click loads the next empty pad"
            } else {
                "drag media onto a track · drop files anywhere"
            },
        );
        ui.add_space(4.0);

        let items: Vec<(usize, Source, usize)> = {
            let p = self.project.lock().unwrap();
            p.sources
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.clone(), App::source_users(&p, i).len()))
                .collect()
        };
        let highlight = self.highlighted_source();

        egui::ScrollArea::vertical().id_salt("library_scroll").show(ui, |ui| {
            if items.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    theme::hint(ui, "nothing here yet —");
                    theme::hint(ui, "+ add a file, a YouTube link or a synth");
                });
            }
            for (i, src, uses) in items {
                self.source_card(ui, i, &src, uses, highlight == Some(i));
            }
        });
    }

    /// The source the current selection is "about" (so clips light up
    /// their media in the library).
    fn highlighted_source(&self) -> Option<usize> {
        match self.sel {
            Sel::Source(i) => Some(i),
            Sel::Clip(ti, ci) => {
                let p = self.project.lock().unwrap();
                let c = p.tracks.get(ti)?.clips.get(ci)?;
                match c.kind {
                    chromagrain_core::timeline::ClipKind::Pattern(_) => None,
                    _ => Some(c.source),
                }
            }
            _ => None,
        }
    }

    fn add_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(240.0);
        theme::section(ui, "media");
        if ui.button("open files…").clicked() {
            ui.close();
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter(
                    "media",
                    &["mp4", "mov", "webm", "mkv", "avi", "m4v", "gif", "wav", "mp3", "flac", "ogg", "m4a", "aiff"],
                )
                .pick_files()
            {
                for p in paths {
                    self.queue_load(p.display().to_string());
                }
            }
        }
        theme::section(ui, "youtube");
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.youtube_url)
                    .hint_text("https://youtube.com/…")
                    .desired_width(160.0),
            );
            let go = ui.button("fetch").clicked()
                || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go && !self.youtube_url.trim().is_empty() {
                let u = self.youtube_url.trim().to_string();
                self.youtube_url.clear();
                self.queue_youtube(u);
                ui.close();
            }
        });
        theme::hint(ui, "first 90s, up to 480p (needs yt-dlp)");
        theme::section(ui, "synths");
        ui.horizontal(|ui| {
            if ui.button("FM tone").clicked() {
                self.add_synth("fm");
                ui.close();
            }
            if ui.button("noise").clicked() {
                self.add_synth("noise");
                ui.close();
            }
            if ui.button("test pattern").clicked() {
                self.add_synth("demo");
                ui.close();
            }
        });
    }

    fn source_card(&mut self, ui: &mut egui::Ui, i: usize, src: &Source, uses: usize, selected: bool) {
        let color = theme::source_color(i);
        let width = ui.available_width();
        let height = 52.0;
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect.expand(1.0));
        let fill = if selected {
            RAISED_HI
        } else if resp.hovered() {
            egui::Color32::from_rgb(32, 32, 37)
        } else {
            RAISED
        };
        painter.rect_filled(rect, 6.0, fill);
        if selected {
            painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
        }
        // Color tab.
        let tab = egui::Rect::from_min_size(rect.min, egui::vec2(4.0, height));
        painter.rect_filled(tab, egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, color);

        // Thumbnail (video frame) or waveform.
        let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 7.0), egui::vec2(66.0, 38.0));
        painter.rect_filled(thumb, 4.0, theme::BG);
        if let Some(tex) = self.thumbnail(ui.ctx(), src) {
            painter.image(
                tex,
                thumb,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else if let Some(peaks) = self.peaks_for(src) {
            draw_peaks(&painter, thumb.shrink(3.0), &peaks, 0.0, 1.0, color);
        }

        // Text.
        let tx = thumb.max.x + 9.0;
        let name_galley = painter.layout(
            src.name.clone(),
            egui::FontId::proportional(13.0),
            TEXT,
            rect.max.x - tx - 8.0,
        );
        let clipped_name = if name_galley.rows.len() > 1 {
            let mut s: String = src.name.chars().take(26).collect();
            s.push('…');
            s
        } else {
            src.name.clone()
        };
        painter.text(
            egui::pos2(tx, rect.min.y + 9.0),
            egui::Align2::LEFT_TOP,
            clipped_name,
            egui::FontId::proportional(13.0),
            TEXT,
        );
        let mut kinds = Vec::new();
        if src.video.is_some() {
            kinds.push("video");
        }
        if src.audio.is_some() {
            kinds.push("audio");
        }
        let mut meta = format!("{:.1}s · {}", src.duration(), kinds.join("+"));
        if src.base_hz > 0.0 {
            meta.push_str(&format!(" · {:.0}Hz", src.base_hz));
        }
        painter.text(
            egui::pos2(tx, rect.min.y + 28.0),
            egui::Align2::LEFT_TOP,
            meta,
            egui::FontId::proportional(10.5),
            TEXT_DIM,
        );
        if uses > 0 {
            painter.text(
                egui::pos2(rect.max.x - 8.0, rect.min.y + 9.0),
                egui::Align2::RIGHT_TOP,
                format!("×{uses}"),
                egui::FontId::monospace(10.5),
                color,
            );
        }

        if resp.hovered() && !resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), SourceDrag(i));
        }
        if resp.clicked() {
            self.select(Sel::Source(i));
        }
        if resp.double_clicked() {
            let t = self.focus_track;
            let b = self.playhead_beats;
            self.place_source(i, t, self.snap(b), PlayMode::Snippet);
        }
        resp.on_hover_text("click to inspect · drag onto a track · double-click to drop at the playhead");
        ui.add_space(2.0);
    }

    /// Ghost label that follows the pointer while a card is dragged.
    pub(crate) fn paint_drag_ghost(&self, ctx: &egui::Context) {
        let (name, color) = if let Some(payload) = egui::DragAndDrop::payload::<SourceDrag>(ctx) {
            let p = self.project.lock().unwrap();
            (p.sources.get(payload.0).map(|s| s.name.clone()).unwrap_or_default(), theme::source_color(payload.0))
        } else if let Some(payload) = egui::DragAndDrop::payload::<BrowseDrag>(ctx) {
            let stem = payload.0.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            (stem, ACCENT)
        } else {
            return;
        };
        let Some(pos) = ctx.pointer_latest_pos() else { return };
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dnd_ghost")));
        let galley = painter.layout_no_wrap(name, egui::FontId::proportional(12.5), egui::Color32::BLACK);
        let rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 8.0), galley.size() + egui::vec2(16.0, 8.0));
        painter.rect_filled(rect, 5.0, color);
        painter.galley(rect.min + egui::vec2(8.0, 4.0), galley, egui::Color32::BLACK);
    }

    /// Cached thumbnail texture for a source's middle video frame.
    pub(crate) fn thumbnail(&mut self, ctx: &egui::Context, src: &Source) -> Option<egui::TextureId> {
        let video = src.video.as_ref()?;
        let key = Arc::as_ptr(video) as usize;
        if let Some(t) = self.thumbs.get(&key) {
            return Some(t.id());
        }
        let frame = video.frames.get(video.frames.len() / 2)?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        // Downscale to keep texture memory tiny.
        let (tw, th) = (192usize, (192.0 * frame.height as f32 / frame.width as f32).max(1.0) as usize);
        let mut px = Vec::with_capacity(tw * th * 4);
        for y in 0..th {
            for x in 0..tw {
                let sx = x * frame.width as usize / tw;
                let sy = y * frame.height as usize / th;
                let o = (sy * frame.width as usize + sx) * 4;
                px.extend_from_slice(&frame.data[o..o + 4]);
            }
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([tw, th], &px);
        let tex = ctx.load_texture(format!("thumb{key}"), img, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.thumbs.insert(key, tex);
        Some(id)
    }

    /// Cached peak envelope (512 buckets) of a source's audio.
    pub(crate) fn peaks_for(&mut self, src: &Source) -> Option<Arc<Vec<f32>>> {
        let audio = src.audio.as_ref()?;
        let key = Arc::as_ptr(audio) as usize;
        if let Some(p) = self.peaks.get(&key) {
            return Some(p.clone());
        }
        let n = 512;
        let s = &audio.samples;
        if s.is_empty() {
            return None;
        }
        let mut peaks = vec![0.0f32; n];
        for (b, pk) in peaks.iter_mut().enumerate() {
            let a = b * s.len() / n;
            let e = ((b + 1) * s.len() / n).max(a + 1).min(s.len());
            *pk = s[a..e].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        }
        let max = peaks.iter().cloned().fold(1e-6, f32::max);
        for p in &mut peaks {
            *p /= max;
        }
        let arc = Arc::new(peaks);
        self.peaks.insert(key, arc.clone());
        Some(arc)
    }
}

/// Draw a mirrored peak envelope for the normalized range [from, to).
pub fn draw_peaks(
    painter: &egui::Painter,
    rect: egui::Rect,
    peaks: &[f32],
    from: f32,
    to: f32,
    color: egui::Color32,
) {
    let cols = (rect.width() / 2.0).max(1.0) as usize;
    let mid = rect.center().y;
    let half = rect.height() * 0.5;
    for c in 0..cols {
        let f = from + (to - from) * (c as f32 / cols as f32);
        if !(0.0..1.0).contains(&f) {
            continue;
        }
        let v = peaks[((f * peaks.len() as f32) as usize).min(peaks.len() - 1)];
        let x = rect.min.x + c as f32 * 2.0 + 1.0;
        painter.line_segment(
            [egui::pos2(x, mid - v * half), egui::pos2(x, mid + v * half)],
            egui::Stroke::new(1.0, color),
        );
    }
}

