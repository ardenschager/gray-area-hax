//! Center column: canvas preview, the timeline (track headers, clips,
//! master bus) and — when a pattern clip is selected — its step grid.

use crate::theme::{self, ACCENT, BG, LINE, PANEL, RAISED, RAISED_HI, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::ui_library::draw_peaks;
use crate::{App, BrowseDrag, ClipDrag, PlayMode, Sel, SourceDrag};
use chromagrain_core::seq::{RowMode, StepPattern};
use chromagrain_core::timeline::{Clip, ClipKind, Project};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const HEADER_W: f32 = 158.0;
const RULER_H: f32 = 22.0;
const TRACK_H: f32 = 50.0;

pub fn clip_title(p: &Project, c: &Clip) -> String {
    match c.kind {
        ClipKind::Pattern(pid) => {
            p.patterns.get(pid).map(|x| x.name.clone()).unwrap_or_else(|| "pattern".into())
        }
        _ => p.sources.get(c.source).map(|s| s.name.clone()).unwrap_or_else(|| "?".into()),
    }
}

pub fn clip_color(c: &Clip) -> Color32 {
    match c.kind {
        ClipKind::Pattern(_) => theme::pattern_color(),
        _ => theme::source_color(c.source),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    Move,
    Scale,
    Rotate,
}

/// An in-progress drag on the preview (values at grab time).
#[derive(Debug, Clone, Copy)]
pub struct GizmoDrag {
    pub mode: GizmoMode,
    pub start: Pos2,
    pub tr: chromagrain_core::timeline::ClipTransform,
}

/// On-screen geometry of a clip's picture box (mirrors
/// `video::grain_draws`: scale + rotate about the canvas center, then
/// translate by half-canvas units; positive rotation is clockwise).
struct Gizmo {
    center: Pos2,
    corners: [Pos2; 4],
    knob: Pos2,
    top_mid: Pos2,
}

impl Gizmo {
    fn new(canvas: Rect, tr: &chromagrain_core::timeline::ClipTransform) -> Gizmo {
        let (w, h) = (canvas.width(), canvas.height());
        let center = canvas.center() + egui::vec2(tr.x * 0.5 * w, tr.y * 0.5 * h);
        let (sn, cs) = tr.rotation.to_radians().sin_cos();
        let rot = |v: egui::Vec2| egui::vec2(v.x * cs - v.y * sn, v.x * sn + v.y * cs);
        let hx = 0.5 * w * tr.scale;
        let hy = 0.5 * h * tr.scale;
        let corners = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)].map(|(x, y)| center + rot(egui::vec2(x, y)));
        Gizmo {
            center,
            corners,
            knob: center + rot(egui::vec2(0.0, -hy + 22.0)),
            top_mid: center + rot(egui::vec2(0.0, -hy)),
        }
    }

    fn hit(&self, p: Pos2) -> GizmoMode {
        if (p - self.knob).length() < 12.0 {
            GizmoMode::Rotate
        } else if self.corners.iter().any(|c| (p - *c).length() < 12.0) {
            GizmoMode::Scale
        } else {
            GizmoMode::Move
        }
    }

    fn paint(&self, painter: &egui::Painter, active: Option<GizmoMode>) {
        let line = Stroke::new(1.0, Color32::from_white_alpha(140));
        let hot = Stroke::new(1.5, ACCENT);
        let box_stroke = if active == Some(GizmoMode::Move) { hot } else { line };
        for i in 0..4 {
            painter.line_segment([self.corners[i], self.corners[(i + 1) % 4]], box_stroke);
        }
        let scale_hot = active == Some(GizmoMode::Scale);
        for c in self.corners {
            painter.rect_filled(Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 2.0, if scale_hot { ACCENT } else { Color32::WHITE });
        }
        let rot_hot = active == Some(GizmoMode::Rotate);
        painter.line_segment([self.top_mid, self.knob], if rot_hot { hot } else { line });
        painter.circle(self.knob, 6.0, if rot_hot { ACCENT } else { Color32::WHITE }, Stroke::new(1.0, Color32::BLACK));
        painter.circle_filled(self.center, 2.5, Color32::from_white_alpha(160));
    }
}

/// Everything needed to draw one clip, copied out of the project lock.
struct ClipView {
    ti: usize,
    ci: usize,
    start: f64,
    len: f64,
    color: Color32,
    title: String,
    mode: PlayMode,
    has_key: bool,
    fx: usize,
    source: Option<chromagrain_core::timeline::Source>,
    position: f32,
    speed: f32,
    density: f32,
    duration: f32,
    seed: u64,
    pattern: Option<StepPattern>,
    /// One pass of the content when the clip repeats (== len otherwise).
    pass: f64,
    forever: bool,
}

impl App {
    // ------------------------------------------------------------ preview

    pub(crate) fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let (pw, ph) = {
            let p = self.project.lock().unwrap();
            (p.width.max(1) as f32, p.height.max(1) as f32)
        };
        let avail_w = ui.available_width();
        let max_h = if self.fullscreen {
            ui.available_height()
        } else {
            (ui.available_height() * 0.42).clamp(150.0, 520.0)
        };
        let mut w = avail_w;
        let mut h = w * ph / pw;
        if h > max_h {
            h = max_h;
            w = h * pw / ph;
        }
        let (full, resp) = ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::click_and_drag());
        let rect = Rect::from_center_size(full.center(), egui::vec2(w, h));
        ui.painter().rect_filled(rect.expand(1.0), 4.0, LINE);

        if self.gpu_failed.load(Ordering::Relaxed) {
            ui.painter().rect_filled(rect, 3.0, Color32::from_gray(14));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "GPU preview unavailable — rendering still works",
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
            return;
        }

        // Publish a consistent (project, plan) pair for the paint callback.
        {
            let t = self.plan_project.beats_to_secs(self.playhead_beats);
            *self.scene.lock().unwrap() = Some(crate::gpu::Scene {
                project: self.plan_project.clone(),
                plan: self.plan.clone(),
                t,
                live: self.live_events.clone(),
                live_t: self.app_clock.elapsed().as_secs_f64(),
                parked: !self.playback.is_playing(),
            });
        }
        let gpu_slot = self.gpu.clone();
        let scene = self.scene.clone();
        let failed = self.gpu_failed.clone();
        let cb = eframe::egui_glow::CallbackFn::new(move |info, painter| {
            let mut slot = gpu_slot.lock().unwrap();
            if slot.is_none() && !failed.load(Ordering::Relaxed) {
                match crate::gpu::GpuPreview::new(painter.gl()) {
                    Ok(g) => *slot = Some(g),
                    Err(e) => {
                        eprintln!("gpu preview init failed: {e}");
                        failed.store(true, Ordering::Relaxed);
                    }
                }
            }
            if let Some(g) = slot.as_mut() {
                if let Some(scene) = scene.lock().unwrap().as_ref() {
                    g.paint(painter.gl(), &info, scene);
                }
            }
        });
        ui.painter().add(egui::PaintCallback { rect, callback: Arc::new(cb) });

        // Fullscreen toggle, top-right of the canvas.
        let label = if self.fullscreen {
            "exit fullscreen  esc"
        } else if self.mode == crate::AppMode::Perform {
            "fullscreen  shift-F"
        } else {
            "fullscreen  F"
        };
        let btn = Rect::from_min_size(rect.right_top() + egui::vec2(-128.0, 6.0), egui::vec2(122.0, 20.0));
        let b = ui.interact(btn, ui.id().with("fullscreen_btn"), egui::Sense::click());
        ui.painter().rect_filled(btn, 4.0, Color32::from_black_alpha(if b.hovered() { 210 } else { 140 }));
        ui.painter().text(
            btn.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            if b.hovered() { TEXT } else { TEXT_DIM },
        );
        if b.on_hover_text("double-click the picture also toggles").clicked() {
            self.toggle_fullscreen(ui.ctx());
        }
        if resp.double_clicked() {
            self.toggle_fullscreen(ui.ctx());
        }

        // Direct manipulation of the selected clip's picture: drag to move,
        // corner handles to size, the top knob to rotate (shift = 15° steps),
        // scroll to size, shift+scroll to rotate. (Placement is audible:
        // x pans, y tilts brightness, size is distance.)
        let Some((ti, ci)) = self.selected_clip() else {
            self.gizmo = None;
            return;
        };
        let Some(tr0) = self.project.lock().unwrap().tracks.get(ti).and_then(|t| t.clips.get(ci)).map(|c| c.transform)
        else {
            return;
        };
        let g = Gizmo::new(rect, &tr0);
        let painter = ui.painter_at(full.expand(4.0));
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hit = pointer.map(|p| g.hit(p)).unwrap_or(GizmoMode::Move);
        g.paint(&painter, self.gizmo.map(|d| d.mode).or(pointer.filter(|p| full.contains(*p)).map(|_| hit)));

        let hovered = pointer.is_some_and(|p| full.contains(p));
        if hovered && self.gizmo.is_none() {
            ui.ctx().set_cursor_icon(match hit {
                GizmoMode::Move => egui::CursorIcon::Move,
                GizmoMode::Scale => egui::CursorIcon::ResizeNwSe,
                GizmoMode::Rotate => egui::CursorIcon::Alias,
            });
        }
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.gizmo = Some(GizmoDrag { mode: g.hit(p), start: p, tr: tr0 });
            }
        }
        if !resp.dragged() && !resp.drag_started() {
            self.gizmo = None;
        }
        let mut tr = tr0;
        if let (Some(d), Some(p)) = (self.gizmo, resp.interact_pointer_pos()) {
            let g0 = Gizmo::new(rect, &d.tr);
            match d.mode {
                GizmoMode::Move => {
                    tr.x = (d.tr.x + (p.x - d.start.x) / w * 2.0).clamp(-1.5, 1.5);
                    tr.y = (d.tr.y + (p.y - d.start.y) / h * 2.0).clamp(-1.5, 1.5);
                }
                GizmoMode::Scale => {
                    let r0 = (d.start - g0.center).length().max(4.0);
                    let r1 = (p - g0.center).length();
                    tr.scale = (d.tr.scale * r1 / r0).clamp(0.1, 2.5);
                }
                GizmoMode::Rotate => {
                    let a0 = (d.start - g0.center).angle();
                    let a1 = (p - g0.center).angle();
                    let mut deg = d.tr.rotation + (a1 - a0).to_degrees();
                    if ui.input(|i| i.modifiers.shift) {
                        deg = (deg / 15.0).round() * 15.0;
                    }
                    tr.rotation = (deg + 180.0).rem_euclid(360.0) - 180.0;
                }
            }
        }
        if hovered {
            let (dx, dy, shift) = ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y, i.modifiers.shift));
            if shift || ui.input(|i| i.modifiers.alt) {
                let d = dx + dy;
                if d != 0.0 {
                    tr.rotation = ((tr.rotation + d * 0.15) + 180.0).rem_euclid(360.0) - 180.0;
                }
            } else if dy != 0.0 {
                tr.scale = (tr.scale * (1.0 + dy * 0.002)).clamp(0.1, 2.5);
            }
            let tip_pos = rect.left_bottom() + egui::vec2(8.0, -8.0);
            ui.painter().text(
                tip_pos,
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "drag = move · corners = size · knob = rotate (shift snaps) · scroll = size    {:.0}%  {:.0}°",
                    tr.scale * 100.0,
                    tr.rotation
                ),
                egui::FontId::proportional(11.0),
                Color32::from_white_alpha(170),
            );
        }
        if tr != tr0 {
            let mut p = self.project.lock().unwrap();
            if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                c.transform = tr;
            }
            drop(p);
            self.mark();
        }
    }

    // ------------------------------------------------------------ timeline

    pub(crate) fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        // Rough height the pattern grid will need below us (0 if none).
        let editor_h = self.selected_clip().map_or(0.0, |(ti, ci)| {
            let p = self.project.lock().unwrap();
            match p.tracks.get(ti).and_then(|t| t.clips.get(ci)).map(|c| c.kind) {
                Some(ClipKind::Pattern(pid)) => {
                    let rows = p.patterns.get(pid).map_or(0, |x| x.rows.len());
                    let open = (0..rows).filter(|r| self.expanded_rows.contains(&(pid, *r))).count();
                    90.0 + rows as f32 * 34.0 + open as f32 * 200.0
                }
                _ => 0.0,
            }
        });
        let n_tracks = self.project.lock().unwrap().tracks.len();
        let content_h = RULER_H + (n_tracks + 1) as f32 * TRACK_H;
        let budget = ui.available_height() - 30.0;
        let max_h = if editor_h > 0.0 {
            (budget - editor_h).max(budget * 0.4).max(140.0)
        } else {
            budget.max(140.0)
        };

        egui::Frame::new().fill(PANEL).corner_radius(6.0).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tl_v")
                .max_height(max_h.min(content_h + 2.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        self.track_headers(ui, n_tracks);
                        egui::ScrollArea::horizontal().id_salt("tl_h").show(ui, |ui| {
                            self.lanes(ui, n_tracks);
                        });
                    });
                });
        });
        ui.horizontal(|ui| {
            if ui.small_button("+ track").clicked() {
                self.add_track();
            }
            theme::hint(
                ui,
                "drag clips to move (across tracks too) · drag the right edge to resize · right-click for more",
            );
        });
    }

    fn track_headers(&mut self, ui: &mut egui::Ui, n_tracks: usize) {
        let info: Vec<(String, bool, Vec<&'static str>)> = {
            let p = self.project.lock().unwrap();
            p.tracks
                .iter()
                .map(|t| (t.name.clone(), t.muted, t.effects.iter().map(|e| e.kind.name()).collect()))
                .collect()
        };
        let master_fx: Vec<&'static str> = {
            let p = self.project.lock().unwrap();
            p.master_effects.iter().map(|e| e.kind.name()).collect()
        };
        let height = RULER_H + (n_tracks + 1) as f32 * TRACK_H;
        let (area, _) = ui.allocate_exact_size(egui::vec2(HEADER_W, height), egui::Sense::hover());
        let painter = ui.painter_at(area);
        painter.rect_filled(
            Rect::from_min_size(area.min, egui::vec2(HEADER_W, RULER_H)),
            egui::CornerRadius { nw: 6, ne: 0, sw: 0, se: 0 },
            RAISED,
        );
        painter.text(
            area.min + egui::vec2(10.0, RULER_H / 2.0),
            egui::Align2::LEFT_CENTER,
            "TRACKS",
            egui::FontId::proportional(9.5),
            TEXT_FAINT,
        );

        let mut toggle_mute: Option<usize> = None;
        for (i, (name, muted, fx)) in info.iter().enumerate() {
            let row = Rect::from_min_size(
                area.min + egui::vec2(0.0, RULER_H + i as f32 * TRACK_H),
                egui::vec2(HEADER_W, TRACK_H),
            );
            let selected = self.sel == Sel::Track(i);
            let resp = ui.interact(row, ui.id().with(("track_hdr", i)), egui::Sense::click());
            let fill = if selected {
                RAISED_HI
            } else if resp.hovered() {
                Color32::from_rgb(30, 30, 35)
            } else if self.focus_track == i {
                Color32::from_rgb(25, 25, 29)
            } else {
                PANEL
            };
            painter.rect_filled(row.shrink2(egui::vec2(0.0, 0.5)), 0.0, fill);
            painter.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, LINE));
            if selected || self.focus_track == i {
                painter.rect_filled(
                    Rect::from_min_size(row.min, egui::vec2(3.0, TRACK_H)),
                    0.0,
                    if selected { ACCENT } else { ACCENT.linear_multiply(0.35) },
                );
            }
            painter.text(
                row.min + egui::vec2(12.0, 11.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::proportional(13.0),
                if *muted { TEXT_FAINT } else { TEXT },
            );
            let sub = if fx.is_empty() { format!("#{}", i + 1) } else { format!("#{} · {}", i + 1, fx.join(" · ")) };
            painter.text(
                row.min + egui::vec2(12.0, 29.0),
                egui::Align2::LEFT_TOP,
                sub,
                egui::FontId::proportional(10.0),
                TEXT_FAINT,
            );
            // Mute pill.
            let m_rect = Rect::from_center_size(
                egui::pos2(row.max.x - 20.0, row.center().y),
                egui::vec2(22.0, 18.0),
            );
            let m_resp = ui
                .interact(m_rect, ui.id().with(("mute", i)), egui::Sense::click())
                .on_hover_text(if *muted { "unmute" } else { "mute" });
            painter.rect_filled(
                m_rect,
                4.0,
                if *muted {
                    Color32::from_rgb(230, 180, 70)
                } else if m_resp.hovered() {
                    RAISED_HI
                } else {
                    RAISED
                },
            );
            painter.text(
                m_rect.center(),
                egui::Align2::CENTER_CENTER,
                "M",
                egui::FontId::proportional(10.5),
                if *muted { Color32::BLACK } else { TEXT_DIM },
            );
            if m_resp.clicked() {
                toggle_mute = Some(i);
            } else if resp.clicked() {
                self.select(Sel::Track(i));
            }
        }
        if let Some(i) = toggle_mute {
            let mut p = self.project.lock().unwrap();
            if let Some(t) = p.tracks.get_mut(i) {
                t.muted = !t.muted;
            }
            drop(p);
            self.mark_now();
        }

        // Master bus row.
        let row = Rect::from_min_size(
            area.min + egui::vec2(0.0, RULER_H + n_tracks as f32 * TRACK_H),
            egui::vec2(HEADER_W, TRACK_H),
        );
        let resp = ui.interact(row, ui.id().with("master_hdr"), egui::Sense::click());
        let selected = self.sel == Sel::Master;
        painter.rect_filled(
            row,
            egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 0 },
            if selected {
                RAISED_HI
            } else if resp.hovered() {
                Color32::from_rgb(30, 30, 35)
            } else {
                RAISED
            },
        );
        if selected {
            painter.rect_filled(Rect::from_min_size(row.min, egui::vec2(3.0, TRACK_H)), 0.0, ACCENT);
        }
        painter.text(
            row.min + egui::vec2(12.0, 11.0),
            egui::Align2::LEFT_TOP,
            "master",
            egui::FontId::proportional(13.0),
            TEXT,
        );
        painter.text(
            row.min + egui::vec2(12.0, 29.0),
            egui::Align2::LEFT_TOP,
            if master_fx.is_empty() { "no effects".to_string() } else { master_fx.join(" · ") },
            egui::FontId::proportional(10.0),
            TEXT_FAINT,
        );
        if resp.clicked() {
            self.select(Sel::Master);
        }
    }

    fn lanes(&mut self, ui: &mut egui::Ui, n_tracks: usize) {
        let (views, end_beat, q, bpm) = {
            let p = self.project.lock().unwrap();
            let views: Vec<ClipView> = p
                .tracks
                .iter()
                .enumerate()
                .flat_map(|(ti, t)| {
                    let p = &p;
                    t.clips.iter().enumerate().map(move |(ci, c)| ClipView {
                        ti,
                        ci,
                        start: c.start_beat,
                        len: c.length_beats,
                        color: clip_color(c),
                        title: clip_title(p, c),
                        mode: PlayMode::of(c.kind),
                        has_key: c.key.is_some(),
                        fx: c.effects.len(),
                        source: match c.kind {
                            ClipKind::Pattern(_) => None,
                            _ => p.sources.get(c.source).cloned(),
                        },
                        position: c.grains.position,
                        speed: c.grains.speed,
                        density: c.grains.density,
                        duration: c.grains.duration,
                        seed: c.grains.seed,
                        pattern: match c.kind {
                            ClipKind::Pattern(pid) => p.patterns.get(pid).cloned(),
                            _ => None,
                        },
                        pass: c.pass_beats(),
                        forever: c.loop_forever,
                    })
                })
                .collect();
            (views, p.end_beat().max(16.0), p.quantize.max(0.0625), p.bpm)
        };
        let beat_w = self.beat_w;
        let height = RULER_H + (n_tracks + 1) as f32 * TRACK_H;
        let width = ((end_beat + 16.0) as f32 * beat_w).max(ui.available_width());
        let (resp, painter) =
            ui.allocate_painter(egui::vec2(width, height), egui::Sense::click_and_drag());
        let origin = resp.rect.min;
        let lanes_top = origin.y + RULER_H;

        // Zoom (pinch / ⌘-scroll).
        if resp.hovered() {
            let z = ui.input(|i| i.zoom_delta());
            if z != 1.0 {
                self.beat_w = (self.beat_w * z).clamp(10.0, 220.0);
            }
        }

        let beat_at = |x: f32| ((x - origin.x) / beat_w) as f64;
        let x_at = |b: f64| origin.x + b as f32 * beat_w;
        let row_at = |y: f32| ((y - lanes_top) / TRACK_H).floor() as i64;

        // Background rows.
        painter.rect_filled(resp.rect, 0.0, BG);
        for i in 0..=n_tracks {
            let r = Rect::from_min_size(
                egui::pos2(origin.x, lanes_top + i as f32 * TRACK_H),
                egui::vec2(width, TRACK_H),
            );
            let fill = if i == n_tracks {
                Color32::from_rgb(17, 17, 20)
            } else if i == self.focus_track {
                Color32::from_rgb(19, 19, 23)
            } else {
                BG
            };
            painter.rect_filled(r, 0.0, fill);
            painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, Color32::from_gray(26)));
        }

        // Grid + ruler.
        painter.rect_filled(
            Rect::from_min_size(origin, egui::vec2(width, RULER_H)),
            0.0,
            RAISED,
        );
        let total_beats = (width / beat_w) as usize + 1;
        let show_sub = beat_w * q as f32 >= 9.0 && q < 1.0;
        for b in 0..=total_beats {
            let x = x_at(b as f64);
            let bar = b % 4 == 0;
            painter.line_segment(
                [egui::pos2(x, if bar { origin.y } else { origin.y + RULER_H * 0.6 }), egui::pos2(x, origin.y + height)],
                Stroke::new(1.0, Color32::from_gray(if bar { 50 } else { 30 })),
            );
            if show_sub {
                let n = (1.0 / q).round() as usize;
                for s in 1..n {
                    let xs = x + s as f32 * q as f32 * beat_w;
                    painter.line_segment(
                        [egui::pos2(xs, lanes_top), egui::pos2(xs, origin.y + height)],
                        Stroke::new(1.0, Color32::from_gray(21)),
                    );
                }
            }
            if bar && (beat_w >= 18.0 || b % 16 == 0) {
                painter.text(
                    egui::pos2(x + 4.0, origin.y + 3.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", b / 4 + 1),
                    egui::FontId::monospace(10.0),
                    TEXT_DIM,
                );
            }
        }

        // Loop region.
        if self.loop_on {
            let r = Rect::from_x_y_ranges(x_at(self.loop_start)..=x_at(self.loop_end), origin.y..=origin.y + height);
            painter.rect_filled(r, 0.0, ACCENT.linear_multiply(0.05));
            painter.rect_filled(
                Rect::from_x_y_ranges(r.x_range(), origin.y..=origin.y + RULER_H),
                0.0,
                ACCENT.linear_multiply(0.35),
            );
        }

        // Clips.
        let secs_per_beat = 60.0 / bpm;
        let clip_rect = |v: &ClipView| {
            Rect::from_min_size(
                egui::pos2(x_at(v.start), lanes_top + v.ti as f32 * TRACK_H + 3.0),
                egui::vec2((v.len as f32 * beat_w).max(6.0), TRACK_H - 6.0),
            )
        };
        for v in &views {
            let rect = clip_rect(v);
            let selected = self.sel == Sel::Clip(v.ti, v.ci);
            let peaks = v.source.as_ref().and_then(|s| self.peaks_for(s));
            draw_clip(&painter, rect, v, selected, peaks.as_deref().map(|p| p.as_slice()), secs_per_beat, beat_w);
        }

        // Files still decoding: a placeholder where each will land.
        if !self.drops.is_empty() {
            let t = ui.input(|i| i.time) as f32;
            for (path, pl, len) in &self.drops {
                let row = if pl.new_track { n_tracks } else { pl.track.min(n_tracks) };
                let r = Rect::from_min_size(
                    egui::pos2(x_at(pl.beat), lanes_top + row as f32 * TRACK_H + 3.0),
                    egui::vec2(*len as f32 * beat_w, TRACK_H - 6.0),
                );
                let pulse = 0.10 + 0.06 * (t * 4.0).sin();
                painter.rect_filled(r, 5.0, ACCENT.linear_multiply(pulse));
                painter.rect_stroke(r, 5.0, Stroke::new(1.0, ACCENT.linear_multiply(0.7)), egui::StrokeKind::Inside);
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                painter.with_clip_rect(r).text(
                    r.min + egui::vec2(6.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    format!("loading {name}…"),
                    egui::FontId::proportional(10.5),
                    ACCENT,
                );
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }

        // Playhead.
        let px = x_at(self.playhead_beats);
        painter.line_segment([egui::pos2(px, origin.y), egui::pos2(px, origin.y + height)], Stroke::new(1.5, ACCENT));
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(px - 5.0, origin.y),
                egui::pos2(px + 5.0, origin.y),
                egui::pos2(px, origin.y + 7.0),
            ],
            ACCENT,
            Stroke::NONE,
        ));

        // ---------------------------------------------------- interaction
        let hit = |pos: Pos2| -> Option<(usize, usize, bool)> {
            views.iter().rev().find_map(|v| {
                let r = clip_rect(v);
                r.contains(pos).then(|| (v.ti, v.ci, pos.x > r.max.x - 7.0))
            })
        };
        // The little repeat tab at a clip's top-right corner.
        let hit_repeat = |pos: Pos2| -> Option<(usize, usize)> {
            views.iter().rev().find_map(|v| repeat_tab(clip_rect(v)).expand(2.0).contains(pos).then_some((v.ti, v.ci)))
        };
        let shift = ui.input(|i| i.modifiers.shift);
        let snap = |b: f64| if shift { b.max(0.0) } else { ((b / q).round() * q).max(0.0) };

        if let Some(pos) = resp.hover_pos() {
            if self.clip_drag.is_none() && !self.scrubbing {
                if hit_repeat(pos).is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                    let tip = painter.layout_no_wrap("drag → to repeat".into(), egui::FontId::proportional(11.0), Color32::BLACK);
                    let r = Rect::from_min_size(pos + egui::vec2(10.0, -22.0), tip.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(r, 4.0, ACCENT);
                    painter.galley(r.min + egui::vec2(5.0, 2.0), tip, Color32::BLACK);
                } else {
                match hit(pos) {
                    Some((_, _, true)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                    Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                    None => {}
                }
                }
            }
        }

        if resp.drag_started() {
            let origin_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
            if origin_pos.y < lanes_top {
                self.scrubbing = true;
            } else if let Some((ti, ci)) = hit_repeat(origin_pos) {
                self.select(Sel::Clip(ti, ci));
                self.clip_drag = Some(ClipDrag { resize: true, repeat: true, ti, ci, grab: 0.0 });
            } else if let Some((ti, ci, edge)) = hit(origin_pos) {
                self.select(Sel::Clip(ti, ci));
                let start = views.iter().find(|v| v.ti == ti && v.ci == ci).map(|v| v.start).unwrap_or(0.0);
                self.clip_drag = Some(ClipDrag { resize: edge, repeat: false, ti, ci, grab: beat_at(origin_pos.x) - start });
            } else {
                self.scrubbing = true;
            }
        }
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if self.scrubbing {
                    self.playhead_beats = snap(beat_at(pos.x));
                } else if let Some(mut d) = self.clip_drag {
                    ui.ctx().set_cursor_icon(if d.resize {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::Grabbing
                    });
                    let mut p = self.project.lock().unwrap();
                    let n = p.tracks.len();
                    if d.resize {
                        if let Some(c) = p.tracks.get_mut(d.ti).and_then(|t| t.clips.get_mut(d.ci)) {
                            if d.repeat && c.loop_beats.is_none() {
                                c.loop_beats = Some(c.length_beats);
                            }
                            if c.is_looping() {
                                // Repeating clips grow in whole passes (shift = free).
                                c.loop_forever = false;
                                let pass = c.loop_beats.unwrap_or(c.length_beats).max(q);
                                let raw = beat_at(pos.x) - c.start_beat;
                                c.length_beats = if shift {
                                    raw.max(q)
                                } else {
                                    (raw / pass).round().max(1.0) * pass
                                };
                            } else {
                                c.length_beats = (snap(beat_at(pos.x)) - c.start_beat).max(q);
                            }
                        }
                    } else {
                        let new_start = snap(beat_at(pos.x) - d.grab);
                        let target = row_at(pos.y).clamp(0, n as i64 - 1) as usize;
                        if target != d.ti && d.ci < p.tracks[d.ti].clips.len() {
                            let c = p.tracks[d.ti].clips.remove(d.ci);
                            p.tracks[target].clips.push(c);
                            d.ti = target;
                            d.ci = p.tracks[target].clips.len() - 1;
                            self.sel = Sel::Clip(d.ti, d.ci);
                            self.focus_track = d.ti;
                        }
                        if let Some(c) = p.tracks.get_mut(d.ti).and_then(|t| t.clips.get_mut(d.ci)) {
                            c.start_beat = new_start;
                        }
                    }
                    drop(p);
                    self.clip_drag = Some(d);
                    self.mark();
                }
            }
        }
        if resp.drag_stopped() {
            if self.scrubbing && self.playback.is_playing() {
                self.start_play();
            }
            if self.clip_drag.is_some() {
                self.cc_mappings.clear();
                self.mark_now();
            }
            self.scrubbing = false;
            self.clip_drag = None;
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.y >= lanes_top {
                    if let Some((ti, ci, _)) = hit(pos) {
                        self.select(Sel::Clip(ti, ci));
                    } else {
                        let r = row_at(pos.y);
                        if r >= 0 && (r as usize) < n_tracks {
                            self.focus_track = r as usize;
                        }
                        self.seek(snap(beat_at(pos.x)));
                    }
                } else {
                    self.seek(snap(beat_at(pos.x)));
                }
            }
        }
        if resp.secondary_clicked() {
            self.context_clip = resp.interact_pointer_pos().and_then(|p| hit(p)).map(|(t, c, _)| (t, c));
            if let Some((t, c)) = self.context_clip {
                self.select(Sel::Clip(t, c));
            }
        }
        if self.context_clip.is_some() {
            resp.context_menu(|ui| {
                let Some((ti, ci)) = self.context_clip else { return };
                ui.set_min_width(160.0);
                theme::section(ui, "play as");
                for m in [PlayMode::Snippet, PlayMode::Grains, PlayMode::Pattern] {
                    if ui.button(m.label()).clicked() {
                        self.set_play_mode(ti, ci, m);
                        ui.close();
                    }
                }
                theme::section(ui, "repeat");
                ui.horizontal(|ui| {
                    let mut p = self.project.lock().unwrap();
                    let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) else { return };
                    let mut changed = false;
                    if ui.small_button("off").clicked() {
                        c.clear_repeat();
                        changed = true;
                    }
                    for n in [2u32, 4, 8] {
                        if ui.small_button(format!("×{n}")).clicked() {
                            c.set_repeat(Some(n));
                            changed = true;
                        }
                    }
                    if ui.small_button("forever").clicked() {
                        c.set_repeat(None);
                        changed = true;
                    }
                    drop(p);
                    if changed {
                        self.mark_now();
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("copy   ⌘C").clicked() {
                    self.copy_clip(ti, ci);
                    ui.close();
                }
                if ui.button("cut   ⌘X").clicked() {
                    self.copy_clip(ti, ci);
                    self.delete_clip(ti, ci);
                    ui.close();
                }
                if ui.button("duplicate   ⌘D").clicked() {
                    self.duplicate_clip(ti, ci);
                    ui.close();
                }
                if ui.button("delete   del").clicked() {
                    self.delete_clip(ti, ci);
                    ui.close();
                }
            });
        }

        // Drop media from the library (or straight from Browse).
        let dragging_media = egui::DragAndDrop::has_payload_of_type::<SourceDrag>(ui.ctx())
            || egui::DragAndDrop::has_payload_of_type::<BrowseDrag>(ui.ctx());
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if resp.rect.contains(pos) && dragging_media {
                let row = row_at(pos.y).max(0) as usize;
                let beat = snap(beat_at(pos.x));
                let new_track = row >= n_tracks;
                let y = lanes_top + row.min(n_tracks) as f32 * TRACK_H + 3.0;
                // Ghost the real length the clip will have.
                let len = if let Some(d) = egui::DragAndDrop::payload::<SourceDrag>(ui.ctx()) {
                    let secs = self.project.lock().unwrap().sources.get(d.0).map(|s| s.duration()).unwrap_or(0.0);
                    self.media_len_beats(secs)
                } else if let Some(d) = egui::DragAndDrop::payload::<BrowseDrag>(ui.ctx()) {
                    self.browse_len_beats(&d.0)
                } else {
                    4.0
                };
                let ghost = Rect::from_min_size(egui::pos2(x_at(beat), y), egui::vec2(len as f32 * beat_w, TRACK_H - 6.0));
                painter.rect_filled(ghost, 4.0, ACCENT.linear_multiply(0.12));
                painter.rect_stroke(ghost, 4.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                painter.text(
                    ghost.min + egui::vec2(6.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    if new_track { "drop → new track" } else { "drop here" },
                    egui::FontId::proportional(11.0),
                    ACCENT,
                );
            }
        }
        if let Some(payload) = resp.dnd_release_payload::<SourceDrag>() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                let row = row_at(pos.y).max(0) as usize;
                let beat = snap(beat_at(pos.x));
                let track = if row >= n_tracks {
                    let mut p = self.project.lock().unwrap();
                    let n = p.tracks.len();
                    p.add_track(&format!("track {}", n + 1))
                } else {
                    row
                };
                self.place_source(payload.0, track, beat, PlayMode::Snippet);
            }
        }
        if let Some(payload) = resp.dnd_release_payload::<BrowseDrag>() {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                let row = row_at(pos.y).max(0) as usize;
                let beat = snap(beat_at(pos.x));
                let place = crate::Place { track: row, new_track: row >= n_tracks, beat };
                self.import_path(payload.0.clone(), Some(place));
            }
        }
    }

    // ------------------------------------------------------------ pattern

    pub(crate) fn pattern_editor_ui(&mut self, ui: &mut egui::Ui, ti: usize, ci: usize, pid: usize) {
        let sources: Vec<String> = {
            let p = self.project.lock().unwrap();
            p.sources.iter().map(|s| s.name.clone()).collect()
        };
        let (shared, clip_start) = {
            let p = self.project.lock().unwrap();
            let n = p
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .filter(|c| c.kind == ClipKind::Pattern(pid))
                .count();
            let start = p.tracks.get(ti).and_then(|t| t.clips.get(ci)).map(|c| c.start_beat).unwrap_or(0.0);
            (n, start)
        };
        let playing = self.playback.is_playing();
        let playhead = self.playhead_beats;
        let pointer_down = ui.input(|i| i.pointer.primary_down());
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        if !pointer_down {
            self.step_paint = None;
        }

        let mut changed = false;
        let mut make_unique = false;
        let frame = egui::Frame::new()
            .fill(PANEL)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut p = self.project.lock().unwrap();
                let n_sources = p.sources.len();
                let Some(pat) = p.patterns.get_mut(pid) else { return };

                ui.horizontal(|ui| {
                    theme::swatch(ui, theme::pattern_color(), 12.0);
                    ui.add(egui::TextEdit::singleline(&mut pat.name).desired_width(140.0).font(egui::TextStyle::Body));
                    ui.add_space(8.0);
                    let mut beats = pat.length_beats;
                    let mut spb = pat.steps_per_beat as i64;
                    let a = ui
                        .add(egui::DragValue::new(&mut beats).speed(0.1).range(1.0..=16.0).suffix(" beats"))
                        .on_hover_text("pattern length (it loops to fill the clip)")
                        .changed();
                    let b = ui
                        .add(egui::DragValue::new(&mut spb).speed(0.1).range(1..=8).suffix(" steps/beat"))
                        .changed();
                    if a || b {
                        pat.set_grid(beats, spb as u32);
                        changed = true;
                    }
                    if shared > 1 {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(format!("shared by {shared} clips")).color(theme::POLY).size(11.0));
                        if ui.small_button("make unique").on_hover_text("give this clip its own copy").clicked() {
                            make_unique = true;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        theme::hint(ui, "click/drag steps · drop media here for a new row");
                    });
                });
                ui.add_space(4.0);

                let grid_n = pat.n_steps();
                let spb = pat.steps_per_beat as usize;
                let beat_in = playhead - clip_start;
                let global_step = (beat_in * spb as f64).floor();
                let mut remove_row: Option<usize> = None;
                for (ri, row) in pat.rows.iter_mut().enumerate() {
                    let color = theme::source_color(row.source);
                    ui.push_id(("seqrow", pid, ri), |ui| {
                        ui.horizontal(|ui| {
                            theme::swatch(ui, color, 10.0);
                            let name = sources.get(row.source).cloned().unwrap_or_else(|| "?".into());
                            egui::ComboBox::from_id_salt("row_src")
                                .selected_text(egui::RichText::new(name).size(12.0))
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    for (si, sname) in sources.iter().enumerate() {
                                        changed |= ui.selectable_value(&mut row.source, si, sname).changed();
                                    }
                                });
                            let mut is_loop = row.mode == RowMode::Loop;
                            if ui
                                .toggle_value(&mut is_loop, "loop")
                                .on_hover_text("instead of steps: loop a slice of the media for the whole clip")
                                .changed()
                            {
                                row.mode = if is_loop { RowMode::Loop } else { RowMode::Steps };
                                changed = true;
                            }
                            match row.mode {
                                RowMode::Steps => {
                                    let mut count = row.steps.len();
                                    let poly = count != grid_n;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut count)
                                                .speed(0.1)
                                                .range(1..=64)
                                                .suffix(if poly { " ↻" } else { "" }),
                                        )
                                        .on_hover_text("steps in this row — differ from the grid for polymeter")
                                        .changed()
                                    {
                                        row.set_steps(count);
                                        changed = true;
                                    }
                                    let n = row.steps.len();
                                    let current = (playing && beat_in >= 0.0).then(|| (global_step as usize) % n);
                                    for si in 0..n {
                                        if si % spb == 0 && si > 0 {
                                            ui.add_space(4.0);
                                        }
                                        let (rect, resp) =
                                            ui.allocate_exact_size(egui::vec2(18.0, 24.0), egui::Sense::click());
                                        let on = row.steps[si].on;
                                        let base = if (si / spb) % 2 == 0 { Color32::from_gray(40) } else { Color32::from_gray(32) };
                                        let fill = if on {
                                            if poly { theme::POLY } else { color }
                                        } else if resp.hovered() {
                                            Color32::from_gray(58)
                                        } else {
                                            base
                                        };
                                        ui.painter().rect_filled(rect.shrink(1.5), 3.0, fill);
                                        if current == Some(si) {
                                            ui.painter().rect_stroke(
                                                rect.shrink(0.5),
                                                3.0,
                                                Stroke::new(1.5, Color32::WHITE),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                        // Click toggles; press-and-drag paints.
                                        if resp.is_pointer_button_down_on() && self.step_paint.is_none() {
                                            self.step_paint = Some(!on);
                                        }
                                        if let (Some(v), Some(pp)) = (self.step_paint, pointer_pos) {
                                            if pointer_down && rect.contains(pp) && row.steps[si].on != v {
                                                row.steps[si].on = v;
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                                RowMode::Loop => {
                                    let mut s0 = row.loop_start;
                                    let mut s1 = row.loop_end;
                                    ui.spacing_mut().slider_width = 140.0;
                                    let a = ui.add(egui::Slider::new(&mut s0, 0.0..=1.0).text("start")).changed();
                                    let b = ui.add(egui::Slider::new(&mut s1, 0.0..=1.0).text("end")).changed();
                                    if a || b {
                                        row.loop_start = s0.min(s1 - 0.01).clamp(0.0, 0.99);
                                        row.loop_end = s1.max(s0 + 0.01).clamp(0.01, 1.0);
                                        changed = true;
                                    }
                                }
                            }
                            ui.add_space(6.0);
                            let key = (pid, ri);
                            let mut open = self.expanded_rows.contains(&key);
                            if ui.toggle_value(&mut open, "...").on_hover_text("how this row's hits sound").changed() {
                                if open {
                                    self.expanded_rows.insert(key);
                                } else {
                                    self.expanded_rows.remove(&key);
                                }
                            }
                            if ui.small_button("x").on_hover_text("remove row").clicked() {
                                remove_row = Some(ri);
                            }
                        });
                        if self.expanded_rows.contains(&(pid, ri)) {
                            ui.indent("row_detail", |ui| {
                                ui.set_max_width(420.0);
                                let g = &mut row.grains;
                                changed |= theme::param("pitch", -24.0, 24.0).suffix(" st").show(ui, &mut g.pitch);
                                changed |= theme::param("gain", 0.0, 2.0).show(ui, &mut g.gain);
                                changed |= theme::param("hit length", 0.01, 2.0).log().suffix(" s").show(ui, &mut g.duration);
                                changed |= theme::param("start in media", 0.0, 1.0).show(ui, &mut g.position);
                                changed |= theme::param("pan", -1.0, 1.0).show(ui, &mut g.pan);
                                changed |= theme::param("fade", 0.01, 1.0).show(ui, &mut g.envelope);
                                changed |= theme::param("random pitch", 0.0, 24.0).show(ui, &mut g.pitch_jitter);
                            });
                        }
                    });
                }
                if let Some(ri) = remove_row {
                    pat.rows.remove(ri);
                    changed = true;
                }
                if ui.small_button("+ row").on_hover_text("adds a row with the last media you touched").clicked()
                    && n_sources > 0
                {
                    pat.add_row(self.last_source.min(n_sources - 1));
                    changed = true;
                }
            });

        if let Some(payload) = frame.response.dnd_release_payload::<SourceDrag>() {
            let mut p = self.project.lock().unwrap();
            if let Some(pat) = p.patterns.get_mut(pid) {
                pat.add_row(payload.0);
            }
            changed = true;
        }
        if make_unique {
            let mut p = self.project.lock().unwrap();
            if let Some(pat) = p.patterns.get(pid).cloned() {
                let mut pat = pat;
                pat.name = format!("{} (own)", pat.name);
                let new_pid = p.add_pattern(pat);
                if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                    c.kind = ClipKind::Pattern(new_pid);
                }
            }
            changed = true;
        }
        if changed {
            self.mark_now();
        }
    }
}

fn draw_clip(
    painter: &egui::Painter,
    rect: Rect,
    v: &ClipView,
    selected: bool,
    peaks: Option<&[f32]>,
    secs_per_beat: f64,
    beat_w: f32,
) {
    let c = v.color;
    let body_fill = Color32::from_rgb(
        (c.r() as f32 * 0.22) as u8 + 10,
        (c.g() as f32 * 0.22) as u8 + 10,
        (c.b() as f32 * 0.22) as u8 + 12,
    );
    painter.rect_filled(rect, 5.0, body_fill);
    let head = Rect::from_min_size(rect.min, egui::vec2(rect.width(), 15.0));
    painter.rect_filled(head, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, if selected { c } else { c.linear_multiply(0.8) });

    let body = Rect::from_min_max(egui::pos2(rect.min.x + 3.0, head.max.y + 2.0), rect.max - egui::vec2(3.0, 3.0));
    let clip_painter = painter.with_clip_rect(rect);
    // Content is drawn once per pass; repeats are slightly dimmer with a
    // divider, so the grid shows exactly what plays.
    let looping = v.pass + 1e-6 < v.len;
    let pass = if looping { v.pass.max(1e-3) } else { v.len };
    let n_pass = ((v.len / pass).ceil() as usize).clamp(1, 512);
    for k in 0..n_pass {
        let x0 = rect.min.x + (k as f64 * pass) as f32 * beat_w;
        if x0 > rect.max.x {
            break;
        }
        let pass_rect = Rect::from_min_max(egui::pos2(x0, rect.min.y), egui::pos2((x0 + pass as f32 * beat_w).min(rect.max.x), rect.max.y));
        let pbody = Rect::from_min_max(egui::pos2(x0 + 3.0, body.min.y), egui::pos2(x0 + pass as f32 * beat_w - 3.0, body.max.y));
        let pp = clip_painter.with_clip_rect(pass_rect.intersect(rect));
        let tint = if k == 0 { 1.0 } else { 0.6 };
        draw_content(&pp, pbody, x0, pass, v, peaks, secs_per_beat, beat_w, tint);
        if k > 0 {
            painter.line_segment(
                [egui::pos2(x0, rect.min.y + 1.0), egui::pos2(x0, rect.max.y - 1.0)],
                Stroke::new(1.0, Color32::from_black_alpha(160)),
            );
        }
    }

    // Header label.
    let mut label = format!("{} · {}", v.mode.label(), v.title);
    if v.has_key {
        label.push_str("  ♪");
    }
    if v.fx > 0 {
        label.push_str(&format!("  fx{}", v.fx));
    }
    if v.forever {
        label.push_str("  · forever");
    } else if looping {
        let n = v.len / pass;
        if (n - n.round()).abs() < 1e-3 {
            label.push_str(&format!("  ×{}", n.round() as u32));
        } else {
            label.push_str(&format!("  ×{n:.1}"));
        }
    }
    clip_painter.text(
        head.min + egui::vec2(6.0, 1.5),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(10.5),
        Color32::from_gray(12),
    );
    if selected {
        painter.rect_stroke(rect, 5.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    }
    // Resize grip.
    let grip = Rect::from_min_max(egui::pos2(rect.max.x - 4.0, rect.min.y + 18.0), egui::pos2(rect.max.x - 2.0, rect.max.y - 5.0));
    painter.rect_filled(grip, 1.0, Color32::from_white_alpha(if selected { 90 } else { 30 }));
    // Repeat tab (drag right to repeat): a tiny loop arrow.
    if rect.width() > 26.0 {
        let tab = repeat_tab(rect);
        painter.rect_filled(tab, 3.0, if looping || v.forever { Color32::from_gray(18) } else { Color32::from_black_alpha(90) });
        let ctr = tab.center();
        let col = if looping || v.forever { c } else { Color32::from_gray(200) };
        let pts: Vec<Pos2> = (0..=10)
            .map(|i| {
                let a = 0.35 + i as f32 / 10.0 * 5.0;
                ctr + egui::vec2(a.cos(), a.sin()) * 3.6
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
        let tip = ctr + egui::vec2(3.6, 0.0);
        painter.add(egui::Shape::convex_polygon(
            vec![tip + egui::vec2(-2.2, -1.2), tip + egui::vec2(2.2, -1.2), tip + egui::vec2(0.0, 1.8)],
            col,
            Stroke::NONE,
        ));
    }
}

/// Where the repeat tab sits on a clip rect (top-right, in the header).
fn repeat_tab(rect: Rect) -> Rect {
    Rect::from_min_size(egui::pos2(rect.max.x - 16.0, rect.min.y + 1.5), egui::vec2(13.0, 12.0))
}

#[allow(clippy::too_many_arguments)]
fn draw_content(
    clip_painter: &egui::Painter,
    body: Rect,
    origin_x: f32,
    len: f64,
    v: &ClipView,
    peaks: Option<&[f32]>,
    secs_per_beat: f64,
    beat_w: f32,
    tint: f32,
) {
    let c = v.color.linear_multiply(tint);
    match v.mode {
        PlayMode::Snippet => {
            if let (Some(peaks), Some(src)) = (peaks, &v.source) {
                let dur = src.duration().max(1e-6) as f32;
                let from = v.position;
                let span = (len * secs_per_beat) as f32 * v.speed.max(0.01) / dur;
                draw_peaks(&clip_painter, body, peaks, from, from + span, c.linear_multiply(0.75));
            }
        }
        PlayMode::Grains => {
            // A deterministic sprinkle: denser clouds, more dots; longer
            // grains, bigger dots.
            let secs = (len * secs_per_beat) as f32;
            let n = ((v.density * secs) as usize).clamp(4, 500);
            let r = (v.duration * 12.0).clamp(1.0, 4.5);
            let mut s = v.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1;
            for _ in 0..n {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                let fx = (s & 0xFFFF) as f32 / 65535.0;
                let fy = ((s >> 16) & 0xFFFF) as f32 / 65535.0;
                let pt = egui::pos2(body.min.x + fx * body.width(), body.min.y + r + fy * (body.height() - 2.0 * r).max(1.0));
                clip_painter.circle_filled(pt, r, c.linear_multiply(0.55));
            }
        }
        PlayMode::Pattern => {
            if let Some(pat) = &v.pattern {
                let spb = pat.steps_per_beat.max(1) as f64;
                let rows = pat.rows.len().clamp(1, 4);
                let rh = body.height() / rows as f32;
                let total_steps = (len * spb).ceil() as usize;
                let step_w = beat_w / spb as f32;
                for (ri, row) in pat.rows.iter().take(4).enumerate() {
                    let y = body.min.y + ri as f32 * rh;
                    let col = theme::source_color(row.source).linear_multiply(tint);
                    match row.mode {
                        RowMode::Loop => {
                            clip_painter.rect_filled(
                                Rect::from_min_size(egui::pos2(body.min.x, y + rh * 0.3), egui::vec2(body.width(), rh * 0.4)),
                                2.0,
                                col.linear_multiply(0.5),
                            );
                        }
                        RowMode::Steps => {
                            let n = row.steps.len().max(1);
                            for k in 0..total_steps.min(2048) {
                                if row.steps[k % n].on {
                                    let x = origin_x + k as f32 * step_w;
                                    clip_painter.rect_filled(
                                        Rect::from_min_size(egui::pos2(x + 1.0, y + 1.0), egui::vec2((step_w - 2.0).max(1.5), (rh - 2.0).max(2.0))),
                                        1.5,
                                        col.linear_multiply(0.85),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

