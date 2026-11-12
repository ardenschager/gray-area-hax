//! Perform: 4×4 clip pads (one global bank) next to the output, launch
//! quantize, output routing (this screen / Display 2), fullscreen.

use super::Studio;
use crate::atlas;
use crate::host::{Host, Pad, PadKind};
use crate::studio::play::short;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind};

impl Studio {
    pub fn perform_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
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
        self.stage(host, ui, stage, 14.0);
        if let Some(sub) = host.perform().and_then(|p| p.output()) {
            let p = ui.painter();
            p.rect_filled(stage, r(14), Color32::from_black_alpha(170));
            draw_icon(p, Icon::Display2, stage.center() - egui::vec2(0.0, 26.0), 40.0, INK2);
            p.text(stage.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "Live on Display 2", font_medium(15.0), INK);
            p.text(stage.center() + egui::vec2(0.0, 32.0), Align2::CENTER_CENTER, sub, mono(11.0), INK3);
        }
        let pads = Rect::from_min_size(egui::pos2(stage.right() + gap, top), egui::vec2(pads_w, stage_h));
        self.pads(host, ui, pads);
        let bar = Rect::from_min_size(egui::pos2(area.left(), stage.bottom() + 22.0), egui::vec2(area.width(), bar_h));
        self.perform_bar(host, ui, bar);
    }

    fn pads(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = 6.0;
        let pw = (area.width() - 3.0 * g) / 4.0;
        let ph = (area.height() - 3.0 * g) / 4.0;
        let now = host.clock();
        let playing = host.playing();
        let beat = host.playhead_beats();
        let (pads, loop_start): (Vec<Pad>, f64) = match host.perform() {
            Some(perf) => (perf.pads(), perf.loop_start()),
            None => return,
        };
        let mut toggle = None;
        let mut load = None;
        for (i, pad) in pads.into_iter().enumerate().take(16) {
            let rc = Rect::from_min_size(egui::pos2(area.left() + (i % 4) as f32 * (pw + g), area.top() + (i / 4) as f32 * (ph + g)), egui::vec2(pw, ph));
            let resp = hit(ui, rc, ui.id().with(("pad", i)), Sense::click(), &format!("pad {}", i + 1));
            let on = pad.on;
            match pad.source {
                Some(src) => {
                    if let Some((tex, a)) = self.atlas(host, &ctx, src) {
                        let frame = if on && playing { ((now * 4.0) as usize) % atlas::FRAMES } else { 5 };
                        tile(&p, rc, 10, tex, a.uv_cover(frame, pw / ph), gray(if on { 0.95 } else { 0.5 }));
                    } else {
                        p.rect_filled(rc, r(10), G2);
                    }
                    // Label + mode glyph on a soft bottom scrim.
                    let scrim = Rect::from_min_max(egui::pos2(rc.left(), rc.bottom() - 30.0), rc.max);
                    p.rect_filled(scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(70));
                    let room = ((pw - 40.0) / 6.6).max(3.0) as usize;
                    let label = pad.name.clone().unwrap_or_else(|| self.source_name(host, src));
                    let name = short(&label, room.min(18));
                    p.text(egui::pos2(rc.left() + 10.0, rc.bottom() - 9.0), Align2::LEFT_BOTTOM, name, font_medium(11.5), if on { INK } else { INK2 });
                    let icon = match pad.kind {
                        PadKind::Loop => Icon::Loop,
                        PadKind::Cloud => Icon::Cloud,
                        PadKind::Chop => Icon::Chop,
                    };
                    draw_icon(&p, icon, egui::pos2(rc.right() - 14.0, rc.bottom() - 15.0), 14.0, INK3);
                    if on {
                        p.rect_stroke(rc, r(10), Stroke::new(2.0, HI), StrokeKind::Inside);
                        // Bar progress.
                        let len = pad.len_beats.max(1.0);
                        let prog = (((beat - loop_start).rem_euclid(len)) / len) as f32;
                        let y = rc.bottom() - 3.0;
                        p.line_segment([egui::pos2(rc.left() + 8.0, y), egui::pos2(rc.left() + 8.0 + (rc.width() - 16.0) * prog, y)], Stroke::new(2.0, HI));
                    } else {
                        p.rect_stroke(rc, r(10), hair(LINE), StrokeKind::Inside);
                    }
                    if let Some(target) = pad.queued {
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
                        toggle = Some(i);
                    }
                }
                None => {
                    p.rect_filled(rc, r(10), G1);
                    p.rect_stroke(rc, r(10), hair(LINE), StrokeKind::Inside);
                    draw_icon(&p, Icon::Plus, rc.center(), 16.0, INK4);
                    if resp.clicked() {
                        if let Some(src) = self.columns[self.emitters[self.sel_emitter].col] {
                            load = Some((src, i));
                        }
                    }
                }
            }
        }
        if let Some(perf) = host.perform() {
            if let Some(i) = toggle {
                perf.toggle_pad(i);
            }
            if let Some((src, i)) = load {
                perf.load_pad(src, i);
            }
        }
        if playing {
            ctx.request_repaint();
        }
    }

    fn perform_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bar: Rect) {
        let p = ui.painter().clone();
        let narrow = bar.width() < 1100.0;
        let cy = bar.center().y;
        let beat_i = (host.playhead_beats().max(0.0) % 4.0).floor() as usize;
        let playing = host.playing();
        let routing = host.caps().output;
        let Some(perf) = host.perform() else { return };
        let mut x = bar.left();
        if !narrow {
            let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Launch", 10.5);
            x = r.right() + 12.0;
        }
        let qw = if narrow { 168.0 } else { 172.0 };
        let q = Rect::from_min_size(egui::pos2(x, cy - 22.0), egui::vec2(qw, 44.0));
        if let Some(i) = segmented(ui, q, ui.id().with("launch"), &["Now", "Beat", "Bar"], &[], perf.quantize(), font(13.0), false) {
            perf.set_quantize(i);
        }
        x = q.right() + 14.0;
        for k in 0..4 {
            let on = playing && k == beat_i;
            p.circle_filled(egui::pos2(x + k as f32 * 10.0, cy), 2.6, if on { HI } else { G5 });
        }
        x += 52.0;
        if routing {
            if !narrow {
                let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Output", 10.5);
                x = r.right() + 12.0;
            }
            let ow = if narrow { 180.0 } else { 184.0 };
            let o = Rect::from_min_size(egui::pos2(x, cy - 22.0), egui::vec2(ow, 44.0));
            let open = perf.output().is_some();
            if let Some(i) = segmented(ui, o, ui.id().with("out"), &["This screen", "Display 2"], &[], open as usize, font(13.0), false) {
                if i == 1 {
                    perf.open_output();
                } else if open {
                    perf.close_output();
                }
            }
        }
        // Right: stop all, fullscreen.
        let fw = if narrow { 128.0 } else { 140.0 };
        let fr = Rect::from_min_size(egui::pos2(bar.right() - fw, cy - 22.0), egui::vec2(fw, 44.0));
        let sr = Rect::from_min_size(egui::pos2(fr.left() - 10.0 - 104.0, cy - 22.0), egui::vec2(104.0, 44.0));
        if button(ui, sr, ui.id().with("stopall"), "Stop all", false, Some(Icon::Square), None).clicked() {
            perf.stop_all();
        }
        if button(ui, fr, ui.id().with("fs"), "Fullscreen", true, Some(Icon::Fullscreen), Some("F")).clicked() {
            host.toggle_fullscreen(ui.ctx());
        }
    }

    /// Fullscreen: just the output; a minimal HUD on pointer activity.
    pub fn perform_fullscreen_ui(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let active = ctx.input(|i| i.pointer.is_moving() || i.pointer.any_pressed() || !i.events.is_empty());
        if active {
            self.hud_until = now + 2.6;
        }
        let show_hud = now < self.hud_until;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ctx, |ui| {
            let full = ui.max_rect();
            let (pw, ph) = host.canvas_size();
            let rect = crate::fit(full, pw.max(1) as f32 / ph.max(1) as f32);
            if let Some(scene) = host.canvas() {
                ui.painter().add(crate::gpu::PreviewCallback::shape(scene, rect));
            }
            let resp = ui.interact(full, ui.id().with("fs-bg"), Sense::click());
            if resp.double_clicked() {
                host.toggle_fullscreen(ui.ctx());
            }
            if !show_hud {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            let p = ui.painter().clone();
            // Top: position + exit.
            let top = Rect::from_min_size(full.min + egui::vec2(20.0, 20.0), egui::vec2(full.width() - 40.0, 40.0));
            let beats = host.playhead_beats().max(0.0);
            let pos = format!("{:03}.{}", (beats / 4.0).floor() as u32 + 1, (beats % 4.0).floor() as u32 + 1);
            let pr = p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), INK);
            p.rect_filled(pr.expand2(egui::vec2(14.0, 9.0)), r(10), Color32::from_black_alpha(130));
            p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), INK);
            let xr = Rect::from_min_size(egui::pos2(top.right() - 96.0, top.top()), egui::vec2(96.0, 40.0));
            p.rect_filled(xr, r(10), Color32::from_black_alpha(130));
            if hit(ui, xr, ui.id().with("fs-exit"), Sense::click(), "exit fullscreen").clicked() {
                host.toggle_fullscreen(ui.ctx());
            }
            runs(&p, xr.center(), Align2::CENTER_CENTER, &[("Exit  ", font_medium(13.0), INK), ("F", mono(10.0), INK3)]);
            // Bottom left: tempo, beat dots, and what's playing.
            let bpm = host.project().lock().unwrap().bpm;
            let playing = host.playing();
            let pads: Vec<Pad> = host.perform().map(|perf| perf.pads()).unwrap_or_default();
            let on_names: Vec<String> = pads
                .iter()
                .filter(|pd| pd.on)
                .filter_map(|pd| pd.name.clone().or_else(|| pd.source.map(|s| self.source_name(host, s))))
                .collect();
            let names = match on_names.len() {
                0 => String::new(),
                n if n <= 4 => on_names.join(" · "),
                n => format!("{} · +{}", on_names[..3].join(" · "), n - 3),
            };
            let bl = egui::pos2(full.left() + 20.0, full.bottom() - 20.0);
            let names_w = if names.is_empty() { 0.0 } else { p.layout_no_wrap(names.clone(), font(12.0), INK2).size().x + 22.0 };
            let info = Rect::from_min_max(bl - egui::vec2(0.0, 64.0), bl + egui::vec2(206.0 + names_w, 0.0));
            p.rect_filled(info, r(12), Color32::from_black_alpha(130));
            runs(
                &p,
                egui::pos2(info.left() + 16.0, info.center().y),
                Align2::LEFT_CENTER,
                &[(&format!("{bpm:.0}"), mono_medium(26.0), INK), (" bpm", font(12.0), INK3)],
            );
            let bi = (beats % 4.0).floor() as usize;
            for k in 0..4 {
                p.circle_filled(egui::pos2(info.left() + 140.0 + k as f32 * 14.0, info.center().y), 3.5, if k == bi && playing { HI } else { G5 });
            }
            if !names.is_empty() {
                p.text(egui::pos2(info.left() + 206.0, info.center().y), Align2::LEFT_CENTER, &names, font(12.0), INK2);
            }
            // Bottom right: the pads, as small thumbnails.
            let (mw, mh, mg) = (52.0, 32.0, 5.0);
            let gsize = egui::vec2(4.0 * mw + 3.0 * mg, 4.0 * mh + 3.0 * mg);
            let grid = Rect::from_min_size(egui::pos2(full.right() - 28.0 - gsize.x, full.bottom() - 28.0 - gsize.y), gsize);
            p.rect_filled(grid.expand(8.0), r(12), Color32::from_black_alpha(130));
            let ctx = ui.ctx().clone();
            let mut toggle = None;
            for (i, pad) in pads.iter().enumerate().take(16) {
                let c = Rect::from_min_size(grid.min + egui::vec2((i % 4) as f32 * (mw + mg), (i / 4) as f32 * (mh + mg)), egui::vec2(mw, mh));
                match pad.source.and_then(|s| self.atlas(host, &ctx, s)) {
                    Some((tex, a)) => tile(&p, c, 5, tex, a.uv_cover(5, mw / mh), gray(if pad.on { 0.95 } else { 0.45 })),
                    None => {
                        p.rect_filled(c, r(5), G1);
                    }
                }
                if pad.on {
                    p.rect_stroke(c, r(5), Stroke::new(1.5, HI), StrokeKind::Inside);
                } else if pad.queued.is_some() && (ui.input(|i| i.time) * 3.0).fract() < 0.6 {
                    p.rect_stroke(c, r(5), Stroke::new(1.0, INK2), StrokeKind::Inside);
                }
                if hit(ui, c, ui.id().with(("mini-pad", i)), Sense::click(), &format!("mini pad {}", i + 1)).clicked() && pad.source.is_some() {
                    toggle = Some(i);
                }
            }
            if let (Some(i), Some(perf)) = (toggle, host.perform()) {
                perf.toggle_pad(i);
            }
        });
    }
}
