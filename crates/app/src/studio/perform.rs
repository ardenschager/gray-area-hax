//! Perform: 4×4 clip pads (one global bank) next to the output, launch
//! quantize, output routing (this screen / Display 2), fullscreen.

use super::atlas;
use super::play::short;
use super::theme::*;
use super::widgets::*;
use crate::{App, LaunchQ, PADS};
use chromagrain_core::timeline::ClipKind;
use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

impl App {
    pub(crate) fn perform_view(&mut self, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1100.0;
        let bar_h = 44.0;
        let gap = if narrow { 14.0 } else { 20.0 };
        let pads_w = if narrow { area.width() * 0.42 } else { (area.width() * 0.37).min(640.0) };
        let stage_w = area.width() - pads_w - gap;
        let mut stage_h = stage_w * 9.0 / 16.0;
        let max_h = area.height() - bar_h - 22.0;
        if stage_h > max_h {
            stage_h = max_h;
        }
        let block_h = stage_h + 22.0 + bar_h;
        let top = area.top() + ((area.height() - block_h) / 2.0).max(0.0);
        let stage = Rect::from_min_size(egui::pos2(area.left(), top), egui::vec2(stage_w, stage_h));
        self.stage(ui, stage, 14.0);
        if self.output.open {
            let p = ui.painter();
            p.rect_filled(stage, r(14), Color32::from_black_alpha(170));
            draw_icon(p, Icon::Display2, stage.center() - egui::vec2(0.0, 26.0), 40.0, INK2);
            p.text(stage.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "Live on Display 2", font_medium(15.0), INK);
            let sub = match self.output.target {
                Some(d) => format!("{:.0}×{:.0} · no chrome", d.size[0], d.size[1]),
                None => "output window · no chrome".into(),
            };
            p.text(stage.center() + egui::vec2(0.0, 32.0), Align2::CENTER_CENTER, sub, mono(11.0), INK3);
        }
        let pads = Rect::from_min_size(egui::pos2(stage.right() + gap, top), egui::vec2(pads_w, stage_h));
        self.pads(ui, pads);
        let bar = Rect::from_min_size(egui::pos2(area.left(), stage.bottom() + 22.0), egui::vec2(area.width(), bar_h));
        self.perform_bar(ui, bar);
    }

    fn pads(&mut self, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = 6.0;
        let pw = (area.width() - 3.0 * g) / 4.0;
        let ph = (area.height() - 3.0 * g) / 4.0;
        let now = self.app_clock.elapsed().as_secs_f64();
        let playing = self.playback.is_playing();
        let (beat, loop_len) = (self.playhead_beats, (self.loop_end - self.loop_start).max(1.0));
        let pads: Vec<(Option<(usize, ClipKind, f64)>, bool)> = {
            let pp = self.project.lock().unwrap();
            (0..PADS)
                .map(|i| {
                    let t = pp.tracks.get(i);
                    let clip = t.and_then(|t| t.clips.first()).map(|c| (c.source, c.kind, c.length_beats));
                    let clip = clip.map(|(s, k, l)| {
                        let s = match k {
                            ClipKind::Pattern(pid) => pp.patterns.get(pid).and_then(|pat| pat.rows.first()).map(|r| r.source).unwrap_or(s),
                            _ => s,
                        };
                        (s, k, l)
                    });
                    (clip, t.is_some_and(|t| !t.muted))
                })
                .collect()
        };
        for (i, (clip, on)) in pads.into_iter().enumerate() {
            let rc = Rect::from_min_size(egui::pos2(area.left() + (i % 4) as f32 * (pw + g), area.top() + (i / 4) as f32 * (ph + g)), egui::vec2(pw, ph));
            let queued = self.pending.iter().find(|(pi, _)| *pi == i).map(|(_, t)| *t);
            let resp = hit(ui, rc, ui.id().with(("pad", i)), Sense::click(), &format!("pad {}", i + 1));
            match clip {
                Some((src, kind, len)) => {
                    if let Some((tex, a)) = self.atlas(&ctx, src) {
                        let frame = if on && playing { ((now * 4.0) as usize) % atlas::FRAMES } else { 5 };
                        tile(&p, rc, 10, tex, a.uv_cover(frame, pw / ph), gray(if on { 0.95 } else { 0.5 }));
                    } else {
                        p.rect_filled(rc, r(10), G2);
                    }
                    // Label + mode glyph on a soft bottom scrim.
                    let scrim = Rect::from_min_max(egui::pos2(rc.left(), rc.bottom() - 30.0), rc.max);
                    p.rect_filled(scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(70));
                    let name = short(&self.source_name(src), 16);
                    p.text(egui::pos2(rc.left() + 10.0, rc.bottom() - 9.0), Align2::LEFT_BOTTOM, name, font_medium(11.5), if on { INK } else { INK2 });
                    let icon = match kind {
                        ClipKind::Snippet => Icon::Loop,
                        ClipKind::Granular => Icon::Cloud,
                        ClipKind::Pattern(_) => Icon::Chop,
                    };
                    draw_icon(&p, icon, egui::pos2(rc.right() - 14.0, rc.bottom() - 15.0), 14.0, INK3);
                    if on {
                        p.rect_stroke(rc, r(10), Stroke::new(2.0, HI), StrokeKind::Inside);
                        // Bar progress.
                        let prog = (((beat - self.loop_start).rem_euclid(len.max(1.0))) / len.max(1.0)) as f32;
                        let _ = loop_len;
                        let y = rc.bottom() - 3.0;
                        p.line_segment([egui::pos2(rc.left() + 8.0, y), egui::pos2(rc.left() + 8.0 + (rc.width() - 16.0) * prog, y)], Stroke::new(2.0, HI));
                    } else {
                        p.rect_stroke(rc, r(10), hair(LINE), StrokeKind::Inside);
                    }
                    if let Some(target) = queued {
                        if (ui.input(|i| i.time) * 3.0).fract() < 0.6 {
                            let tip = egui::pos2(rc.right() - 12.0, rc.top() + 12.0);
                            let pts = if target {
                                vec![tip + egui::vec2(-4.0, -5.0), tip + egui::vec2(5.0, 0.0), tip + egui::vec2(-4.0, 5.0)]
                            } else {
                                vec![tip + egui::vec2(-4.0, -4.0), tip + egui::vec2(4.0, -4.0), tip + egui::vec2(4.0, 4.0), tip + egui::vec2(-4.0, 4.0)]
                            };
                            p.add(egui::Shape::convex_polygon(pts, HI, Stroke::NONE));
                        }
                        ctx.request_repaint();
                    }
                    if resp.clicked() {
                        self.toggle_pad(i);
                    }
                }
                None => {
                    p.rect_filled(rc, r(10), G1);
                    p.rect_stroke(rc, r(10), hair(LINE), StrokeKind::Inside);
                    draw_icon(&p, Icon::Plus, rc.center(), 16.0, INK4);
                    if resp.clicked() {
                        let src = self.studio.columns[self.studio.emitters[self.studio.sel_emitter].col];
                        if let Some(src) = src {
                            self.load_pad(src, i, crate::PlayMode::Snippet);
                        }
                    }
                }
            }
        }
        if playing {
            ctx.request_repaint();
        }
    }

    fn perform_bar(&mut self, ui: &mut egui::Ui, bar: Rect) {
        let p = ui.painter().clone();
        let narrow = bar.width() < 1100.0;
        let cy = bar.center().y;
        let mut x = bar.left();
        if !narrow {
            let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Launch", 10.5);
            x = r.right() + 12.0;
        }
        let qw = if narrow { 168.0 } else { 172.0 };
        let q = Rect::from_min_size(egui::pos2(x, cy - 22.0), egui::vec2(qw, 44.0));
        let qi = match self.launch_q {
            LaunchQ::Now => 0,
            LaunchQ::Beat => 1,
            LaunchQ::Bar => 2,
        };
        if let Some(i) = segmented(ui, q, ui.id().with("launch"), &["Now", "Beat", "Bar"], &[], qi, font(13.0), false) {
            self.launch_q = [LaunchQ::Now, LaunchQ::Beat, LaunchQ::Bar][i];
        }
        x = q.right() + 14.0;
        let beat_i = (self.playhead_beats.max(0.0) % 4.0).floor() as usize;
        for k in 0..4 {
            let on = self.playback.is_playing() && k == beat_i;
            p.circle_filled(egui::pos2(x + k as f32 * 10.0, cy), 2.6, if on { HI } else { G5 });
        }
        x += 52.0;
        if !narrow {
            let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Output", 10.5);
            x = r.right() + 12.0;
        }
        let ow = if narrow { 180.0 } else { 184.0 };
        let o = Rect::from_min_size(egui::pos2(x, cy - 22.0), egui::vec2(ow, 44.0));
        let oi = if self.output.open { 1 } else { 0 };
        if let Some(i) = segmented(ui, o, ui.id().with("out"), &["This screen", "Display 2"], &[], oi, font(13.0), false) {
            if i == 1 {
                self.open_output_window();
            } else if self.output.open {
                self.toggle_output();
            }
        }
        // Right: stop all, fullscreen.
        let fw = if narrow { 128.0 } else { 140.0 };
        let fr = Rect::from_min_size(egui::pos2(bar.right() - fw, cy - 22.0), egui::vec2(fw, 44.0));
        if button(ui, fr, ui.id().with("fs"), "Fullscreen", true, Some(Icon::Fullscreen), Some("F")).clicked() {
            self.toggle_fullscreen(ui.ctx());
        }
        let sr = Rect::from_min_size(egui::pos2(fr.left() - 10.0 - 104.0, cy - 22.0), egui::vec2(104.0, 44.0));
        if button(ui, sr, ui.id().with("stopall"), "Stop all", false, Some(Icon::Square), None).clicked() {
            self.stop_all();
        }
    }

    /// Fullscreen: just the output; a minimal HUD on pointer activity.
    pub(crate) fn perform_fullscreen_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let active = ctx.input(|i| i.pointer.is_moving() || i.pointer.any_pressed() || !i.events.is_empty());
        if active {
            self.studio.hud_until = now + 2.6;
        }
        let show_hud = now < self.studio.hud_until;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ctx, |ui| {
            let full = ui.max_rect();
            let (pw, ph) = (self.plan_project.width.max(1) as f32, self.plan_project.height.max(1) as f32);
            let rect = crate::displays::fit(full, pw / ph);
            if !self.gpu_failed.load(std::sync::atomic::Ordering::Relaxed) {
                ui.painter().add(crate::gpu::PreviewCallback::shape(self.scene.clone(), rect));
            }
            let resp = ui.interact(full, ui.id().with("fs-bg"), Sense::click());
            if resp.double_clicked() {
                self.toggle_fullscreen(ui.ctx());
            }
            if !show_hud {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            let p = ui.painter().clone();
            // Top: position + exit.
            let top = Rect::from_min_size(full.min + egui::vec2(20.0, 20.0), egui::vec2(full.width() - 40.0, 40.0));
            let beats = self.playhead_beats.max(0.0);
            let pos = format!("{:03}.{}", (beats / 4.0).floor() as u32 + 1, (beats % 4.0).floor() as u32 + 1);
            let pr = p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), INK);
            p.rect_filled(pr.expand2(egui::vec2(14.0, 9.0)), r(10), Color32::from_black_alpha(130));
            p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), INK);
            let xr = Rect::from_min_size(egui::pos2(top.right() - 96.0, top.top()), egui::vec2(96.0, 40.0));
            p.rect_filled(xr, r(10), Color32::from_black_alpha(130));
            if hit(ui, xr, ui.id().with("fs-exit"), Sense::click(), "exit fullscreen").clicked() {
                self.toggle_fullscreen(ui.ctx());
            }
            runs(&p, xr.center(), Align2::CENTER_CENTER, &[("Exit  ", font_medium(13.0), INK), ("F", mono(10.0), INK3)]);
            // Bottom: tempo, beat dots, and a mini pad grid.
            let bpm = self.project.lock().unwrap().bpm;
            let bl = egui::pos2(full.left() + 20.0, full.bottom() - 20.0);
            let info = Rect::from_min_max(bl - egui::vec2(0.0, 64.0), bl + egui::vec2(260.0, 0.0));
            p.rect_filled(info, r(12), Color32::from_black_alpha(130));
            runs(
                &p,
                egui::pos2(info.left() + 16.0, info.center().y),
                Align2::LEFT_CENTER,
                &[(&format!("{bpm:.0}"), mono_medium(26.0), INK), (" bpm", font(12.0), INK3)],
            );
            let bi = (beats % 4.0).floor() as usize;
            for k in 0..4 {
                p.circle_filled(egui::pos2(info.left() + 150.0 + k as f32 * 14.0, info.center().y), 3.5, if k == bi && self.playback.is_playing() { HI } else { G5 });
            }
            let mini = 30.0;
            let grid = Rect::from_min_size(egui::pos2(full.right() - 20.0 - 4.0 * (mini + 4.0), full.bottom() - 20.0 - 4.0 * (mini + 4.0)), egui::vec2(4.0 * (mini + 4.0), 4.0 * (mini + 4.0)));
            p.rect_filled(grid.expand(8.0), r(12), Color32::from_black_alpha(130));
            let states: Vec<(bool, bool)> = {
                let pp = self.project.lock().unwrap();
                (0..PADS).map(|i| pp.tracks.get(i).map(|t| (!t.clips.is_empty(), !t.muted)).unwrap_or((false, false))).collect()
            };
            for (i, (has, on)) in states.into_iter().enumerate() {
                let c = Rect::from_min_size(grid.min + egui::vec2((i % 4) as f32 * (mini + 4.0), (i / 4) as f32 * (mini + 4.0)), egui::vec2(mini, mini));
                p.rect_filled(c, r(6), if on { HI } else if has { G3 } else { G1 });
                if hit(ui, c, ui.id().with(("mini-pad", i)), Sense::click(), &format!("mini pad {}", i + 1)).clicked() && has {
                    self.toggle_pad(i);
                }
            }
        });
    }
}
