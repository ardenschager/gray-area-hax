//! Perform: 4×4 clip pads next to the live visual stage, shared Set / Cue
//! Arranger strip (standalone), 6 tactile momentary/latching VJ Punch-In FX
//! pills (`Stutter`, `Tape Stop`, `Comb`, `Dub`, `Shimmer`, `Wear`), launch
//! quantize, output routing, contextual object bar (`Open clip ›`), and
//! chromeless fullscreen.

use super::{ClipTarget, Studio, PUNCH_FX_A11Y, PUNCH_FX_LABELS};
use crate::atlas;
use crate::host::{Host, Pad, PadKind};
use crate::studio::play::short;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::envelope::Adsr;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind};

impl Studio {
    pub fn perform_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1100.0;
        let compact_h = area.height() < 560.0;
        let has_set_bar = host.caps().scenes;
        let set_h = if has_set_bar { if compact_h { 28.0 } else { 32.0 } } else { 0.0 };
        let set_gap = if has_set_bar { if compact_h { 6.0 } else { 8.0 } } else { 0.0 };
        let isol8_h = if compact_h {
            106.0
        } else if area.height() < 760.0 {
            126.0
        } else {
            148.0
        };
        let isol8_gap = if compact_h { 6.0 } else { 10.0 };
        let bar_h = if compact_h { 36.0 } else if narrow { 38.0 } else { 42.0 };
        let bot_gap = if compact_h { 6.0 } else if narrow { 8.0 } else { 12.0 };
        let fx_h = if compact_h { 30.0 } else { 34.0 };
        let fx_gap = if compact_h { 6.0 } else { 8.0 };
        let gap = if narrow { 12.0 } else { 18.0 };

        let pads_w = if narrow { area.width() * 0.42 } else { (area.width() * 0.38).min(640.0) };
        let stage_w = area.width() - pads_w - gap;
        let max_main_h = (area.height() - set_h - set_gap - isol8_gap - isol8_h - bot_gap - bar_h).max(190.0);
        let ideal_stage_h = (stage_w * 9.0 / 16.0).floor();
        let main_h = (ideal_stage_h + fx_gap + fx_h).min(max_main_h);
        let stage_h = (main_h - fx_gap - fx_h).max(150.0);

        let block_h = set_h + set_gap + main_h + isol8_gap + isol8_h + bot_gap + bar_h;
        let mut y = area.top() + ((area.height() - block_h) * 0.5).max(0.0);

        if has_set_bar {
            let set_rect = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(area.width(), set_h));
            self.set_arranger_bar(host, ui, set_rect, "perf-set");
            y = set_rect.bottom() + set_gap;
        }

        let stage = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(stage_w, stage_h));
        self.stage(host, ui, stage, 8.0);
        self.layout_switch(host, ui, stage);
        if let Some(sub) = host.perform().and_then(|p| p.output()) {
            let p = ui.painter();
            p.rect_filled(stage, r(8), Color32::from_black_alpha(190));
            draw_icon(p, Icon::Display2, stage.center() - egui::vec2(0.0, 26.0), 40.0, CREAM2);
            p.text(stage.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "Live on Display 2", font_medium(15.0), CREAM);
            p.text(stage.center() + egui::vec2(0.0, 32.0), Align2::CENTER_CENTER, sub, mono(11.0), CREAM3);
        }

        // 6 Tactile Backlit Amber VJ Punch-In FX Switches right under the stage.
        let fx_rect = Rect::from_min_size(egui::pos2(area.left(), stage.bottom() + fx_gap), egui::vec2(stage_w, fx_h));
        self.perform_punch_fx_bar(host, ui, fx_rect);

        let pads = Rect::from_min_size(egui::pos2(stage.right() + gap, y), egui::vec2(pads_w, main_h));
        self.pads(host, ui, pads);

        // Dedicated `ISOL8 · 5-BAND FREQUENCY MONITOR` hardware rack bay!
        let isol8_rect = Rect::from_min_size(
            egui::pos2(area.left(), y + main_h + isol8_gap),
            egui::vec2(area.width(), isol8_h),
        );
        self.isolator_rack_ui(host, ui, isol8_rect, "perf-isol8");

        let bar = Rect::from_min_size(
            egui::pos2(area.left(), isol8_rect.bottom() + bot_gap),
            egui::vec2(area.width(), bar_h),
        );
        self.perform_bar(host, ui, bar);
    }

    fn perform_punch_fx_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bar: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = 6.0;
        let pw = (bar.width() - g * 5.0) / 6.0;
        let t_now = ui.input(|i| i.time);
        for k in 0..6 {
            let rc = Rect::from_min_size(egui::pos2(bar.left() + k as f32 * (pw + g), bar.top()), egui::vec2(pw, bar.height()));
            let resp = hit(ui, rc, ui.id().with(("punch-fx", k)), Sense::click_and_drag(), PUNCH_FX_A11Y[k]);
            let down = resp.is_pointer_button_down_on();
            if down && self.punch_press_t[k].is_none() {
                self.punch_press_t[k] = Some(t_now);
                // Momentary hold (>0.28s) turns FX on immediately while held; quick tap latches/toggles on click.
            }
            if down {
                if let Some(t0) = self.punch_press_t[k] {
                    if t_now - t0 >= 0.28 && !self.punch_fx[k] {
                        self.set_punch_fx(host, k, true, &ctx);
                    }
                }
            } else if let Some(t0) = self.punch_press_t[k].take() {
                if resp.clicked() && t_now - t0 < 0.28 {
                    let next = !self.punch_fx[k];
                    self.set_punch_fx(host, k, next, &ctx);
                } else if t_now - t0 >= 0.28 {
                    self.set_punch_fx(host, k, false, &ctx);
                }
            } else if resp.clicked() {
                let next = !self.punch_fx[k];
                self.set_punch_fx(host, k, next, &ctx);
            }

            let on = self.punch_fx[k];
            paint_switch_cap(&p, rc, SwitchKind::Amber, on, resp.hovered(), down);
            let fsz = if pw < 76.0 { 10.0 } else { 11.0 };
            p.text(
                rc.center(),
                Align2::CENTER_CENTER,
                PUNCH_FX_LABELS[k],
                font_semibold(fsz),
                if on { ON_HI } else { Color32::from_rgb(0x24, 0x1c, 0x09) },
            );
        }
    }

    fn pads(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        monitor_well(&p, area, 7);
        let inner_area = area.shrink(4.5);
        let g = 5.0;
        let pw = (inner_area.width() - 3.0 * g) / 4.0;
        let ph = (inner_area.height() - 3.0 * g) / 4.0;
        let now = host.clock();
        let playing = host.playing();
        let beat = host.playhead_beats();
        let (pads, loop_start, pad_envs): (Vec<Pad>, f64, Vec<Adsr>) = match host.perform() {
            Some(perf) => {
                let ps = perf.pads();
                let ls = perf.loop_start();
                let envs = (0..16).map(|i| perf.pad_env(i).unwrap_or_default()).collect();
                (ps, ls, envs)
            }
            None => return,
        };
        if pads.get(self.sel_pad).is_none_or(|pd| pd.source.is_none()) {
            if let Some(first) = pads.iter().position(|pd| pd.on).or_else(|| pads.iter().position(|pd| pd.source.is_some())) {
                self.sel_pad = first;
            }
        }
        let shared = host.shared_macros();
        let mut toggle = None;
        let mut load = None;
        let mut clip = None;
        let pending = self.pending_slots(host);
        let t_ui = ui.input(|i| i.time);
        for (i, pad) in pads.into_iter().enumerate().take(16) {
            let rc = Rect::from_min_size(
                egui::pos2(
                    inner_area.left() + (i % 4) as f32 * (pw + g),
                    inner_area.top() + (i / 4) as f32 * (ph + g),
                ),
                egui::vec2(pw, ph),
            );
            let resp = hit(ui, rc, ui.id().with(("pad", i)), Sense::click(), &format!("pad {}", i + 1));
            let on = pad.on;
            let is_sel = i == self.sel_pad;
            if pad.source.is_none() {
                if let Some((pd, _)) = pending.iter().find(|(_, t)| *t == crate::host::AddTarget::Pad(i)) {
                    super::add::placeholder_tile(&p, rc, 6, pd, t_ui, true);
                    ctx.request_repaint();
                    continue;
                }
            }
            match pad.source {
                Some(src) => {
                    let (base_frame, pad_grain, pad_pitch_m, pad_pitch_st) = {
                        let pp = host.project().lock().unwrap();
                        let bf = pad
                            .name
                            .as_deref()
                            .and_then(|s| s.rsplit_once(" · "))
                            .and_then(|(_, tail)| tail.parse::<usize>().ok())
                            .and_then(|n| {
                                let so = pp.sources.get(src)?;
                                let sl = so.slices.get(n.checked_sub(1)?)?;
                                Some(atlas::frame_at((sl.start / so.duration().max(1e-6)) as f32))
                            })
                            .or_else(|| {
                                let c = pp.tracks.get(i)?.clips.first()?;
                                (c.grains.position > 0.001).then(|| atlas::frame_at(c.grains.position))
                            })
                            .unwrap_or(5);
                        let (gr, pm, pst) = pp
                            .tracks
                            .get(i)
                            .and_then(|t| t.clips.first())
                            .map(|c| {
                                let g0 = if matches!(pad.kind, PadKind::Cloud) { c.grains.texture.max(0.55) } else { c.grains.texture };
                                (g0, ((c.grains.pitch + 12.0) / 24.0).clamp(0.0, 1.0), c.grains.pitch)
                            })
                            .or_else(|| shared.map(|m| (m[0], m[1], (m[1] - 0.5) * 24.0)))
                            .unwrap_or((0.35, 0.5, 0.0));
                        (bf, gr, pm, pst)
                    };
                    if let Some((tex, a)) = self.atlas(host, &ctx, src) {
                        let frame = if on && playing { (base_frame + (now * 4.0) as usize) % atlas::FRAMES } else { base_frame };
                        let bright = if on { 0.90 } else if is_sel { 0.65 } else { 0.50 };
                        tile(&p, rc, 6, tex, a.uv_cover(frame, pw / ph), gray(bright));
                    } else {
                        p.rect_filled(rc, r(6), WELL2);
                    }
                    let top_scrim = Rect::from_min_size(rc.min, egui::vec2(pw, 22.0_f32.min(ph * 0.4)));
                    p.rect_filled(top_scrim, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_black_alpha(105));
                    let scrim = Rect::from_min_max(egui::pos2(rc.left(), (rc.bottom() - 26.0).max(rc.top())), rc.max);
                    p.rect_filled(scrim, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, Color32::from_black_alpha(145));

                    let env_w = (pw * 0.30).clamp(18.0, 34.0);
                    let env_r = Rect::from_min_size(egui::pos2(rc.left() + 7.0, rc.top() + 6.0), egui::vec2(env_w, 10.5));
                    mini_adsr(&p, env_r, &pad_envs[i], if on || is_sel { 0.92 } else { 0.68 });

                    if is_sel && ph >= 62.0 {
                        draw_on_object_ring(&p, rc, pad_grain, pad_pitch_m, pad_pitch_st, 0.0, true);
                    }

                    let room = ((pw - 32.0) / 6.2).max(5.0) as usize;
                    let raw_label = pad.name.clone().unwrap_or_else(|| self.source_name(host, src));
                    let label = super::strip_media_ext(&raw_label);
                    let cap = room.min(17);
                    let name = if let Some((head, tail)) = label.rsplit_once(" · ") {
                        if tail.len() + 3 < cap {
                            format!("{} · {tail}", short(head, cap - tail.len() - 3))
                        } else {
                            short(label, cap)
                        }
                    } else if let Some((head, tail)) = label.rsplit_once(" ~ ") {
                        if tail.len() + 3 < cap {
                            format!("{} ~ {tail}", short(head, cap - tail.len() - 3))
                        } else {
                            short(label, cap)
                        }
                    } else {
                        short(label, cap)
                    };
                    p.text(
                        egui::pos2(rc.left() + 8.0, rc.bottom() - 6.5),
                        Align2::LEFT_BOTTOM,
                        name,
                        font_medium(10.5),
                        if on || is_sel { CREAM } else { CREAM2 },
                    );
                    let icon = match pad.kind {
                        PadKind::Loop => Icon::Loop,
                        PadKind::Cloud => Icon::Cloud,
                        PadKind::Chop => Icon::Chop,
                    };
                    draw_icon(&p, icon, egui::pos2(rc.right() - 12.0, rc.bottom() - 12.0), 12.0, CREAM3);

                    if on {
                        p.rect_stroke(rc, r(6), Stroke::new(2.0, AMBER_MID), StrokeKind::Inside);
                        let prog = if let Some(free_s) = self.pad_sculpt[i].free_loop_secs {
                            ((now.rem_euclid(free_s.max(0.5) as f64)) / free_s.max(0.5) as f64) as f32
                        } else {
                            let len = pad.len_beats.max(1.0);
                            (((beat - loop_start).rem_euclid(len)) / len) as f32
                        };
                        let y = rc.bottom() - 2.5;
                        p.line_segment(
                            [egui::pos2(rc.left() + 6.0, y), egui::pos2(rc.left() + 6.0 + (rc.width() - 12.0) * prog, y)],
                            Stroke::new(2.0, AMBER_CORE),
                        );
                    } else if is_sel {
                        p.rect_stroke(rc, r(6), Stroke::new(1.5, CREAM2), StrokeKind::Inside);
                    } else {
                        p.rect_stroke(rc, r(6), hair(WELL_LINE), StrokeKind::Inside);
                    }

                    if let Some(target) = pad.queued {
                        if (t_ui * 3.0).fract() < 0.6 {
                            let tip = egui::pos2(rc.right() - 34.0, rc.top() + 13.5);
                            let pts = if target {
                                vec![tip + egui::vec2(-3.5, -4.5), tip + egui::vec2(4.5, 0.0), tip + egui::vec2(-3.5, 4.5)]
                            } else {
                                vec![tip + egui::vec2(-3.5, -3.5), tip + egui::vec2(3.5, -3.5), tip + egui::vec2(3.5, 3.5), tip + egui::vec2(-3.5, 3.5)]
                            };
                            p.add(egui::Shape::convex_polygon(pts, AMBER_MID, Stroke::NONE));
                        }
                        ctx.request_repaint();
                    }

                    let chev_vis = Rect::from_min_size(egui::pos2(rc.right() - 24.0, rc.top() + 4.0), egui::vec2(20.0, 20.0));
                    let chev_hit = Rect::from_center_size(chev_vis.center(), egui::vec2(25.0, 25.0));
                    let chev_resp = chev_button(ui, chev_vis, chev_hit, ui.id().with(("pad-clip", i)), &format!("clip pad {}", i + 1));

                    let h = hold(ui, &resp, 0.45);
                    if chev_resp.clicked() {
                        self.sel_pad = i;
                        clip = Some(i);
                    } else if h.fired {
                        self.sel_pad = i;
                        clip = Some(i);
                    } else if resp.clicked() && !h.held_before && !chev_resp.hovered() {
                        self.sel_pad = i;
                        toggle = Some(i);
                    }
                }
                None => {
                    p.rect_filled(rc, r(6), WELL2);
                    p.rect_stroke(rc, r(6), hair(WELL_LINE), StrokeKind::Inside);
                    draw_icon(&p, Icon::Plus, rc.center(), 15.0, CREAM3);
                    if resp.clicked() {
                        load = Some(i);
                    }
                }
            }
        }
        if let Some(i) = toggle {
            if let Some(perf) = host.perform() {
                perf.toggle_pad(i);
            }
            if !self.pad_captions[i].trim().is_empty() {
                self.active_caption = Some((self.pad_captions[i].trim().to_string(), now + 1.2));
            }
        }
        if let Some(i) = load {
            self.open_add(host, crate::host::AddTarget::Pad(i));
        }
        if let Some(i) = clip {
            self.open_clip(ClipTarget::Pad(i));
        }
        if playing {
            ctx.request_repaint();
        }
    }

    fn perform_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bar: Rect) {
        let p = ui.painter().clone();
        let narrow = bar.width() < 1100.0;
        let bh = bar.height();
        let cy = bar.center().y;
        let beat_i = (host.playhead_beats().max(0.0) % 4.0).floor() as usize;
        let playing = host.playing();
        let routing = host.caps().output;
        let (pads, quant, out_open) = match host.perform() {
            Some(perf) => (perf.pads(), perf.quantize(), perf.output().is_some()),
            None => return,
        };
        let mut set_q = None;
        let mut set_out = None;
        let mut do_stop = false;
        let mut do_fs = false;
        let mut open_pad_clip = None;

        let mut x = bar.left();
        if !narrow {
            let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Launch", 10.5);
            x = r.right() + 12.0;
        }
        let qw = if narrow { 164.0 } else { 172.0 };
        let q = Rect::from_min_size(egui::pos2(x, cy - bh * 0.5), egui::vec2(qw, bh));
        if let Some(i) = segmented(ui, q, ui.id().with("launch"), &["Now", "Beat", "Bar"], &[], quant, font(13.0), false) {
            set_q = Some(i);
        }
        x = q.right() + 14.0;
        for k in 0..4 {
            let on = playing && k == beat_i;
            jewel_led(&p, egui::pos2(x + k as f32 * 11.0, cy), 3.0, on, false);
        }
        x += 50.0;
        if routing {
            if !narrow {
                let r = caps(&p, egui::pos2(x, cy), Align2::LEFT_CENTER, "Output", 10.5);
                x = r.right() + 12.0;
            }
            let ow = if narrow { 176.0 } else { 184.0 };
            let o = Rect::from_min_size(egui::pos2(x, cy - bh * 0.5), egui::vec2(ow, bh));
            if let Some(i) = segmented(ui, o, ui.id().with("out"), &["This screen", "Display 2"], &[], out_open as usize, font(13.0), false) {
                set_out = Some(i);
            }
            x = o.right() + 18.0;
        }

        let fw = if narrow { 126.0 } else { 140.0 };
        let fr = Rect::from_min_size(egui::pos2(bar.right() - fw, cy - bh * 0.5), egui::vec2(fw, bh));
        let sr = Rect::from_min_size(egui::pos2(fr.left() - 10.0 - 102.0, cy - bh * 0.5), egui::vec2(102.0, bh));

        if !narrow && sr.left() - x >= 180.0 {
            let sp = self.sel_pad.min(15);
            if let Some(pd) = pads.get(sp).filter(|pd| pd.source.is_some()) {
                let src = pd.source.unwrap();
                let raw = pd.name.clone().unwrap_or_else(|| self.source_name(host, src));
                let clean = super::strip_media_ext(&raw);
                let label = format!("Pad {} · {}", sp + 1, short(clean, 16));
                let tr = p.text(egui::pos2(x, cy), Align2::LEFT_CENTER, label, font_medium(12.0), INK2);
                let pill_x = (tr.right() + 10.0).min(sr.left() - 104.0);
                if pill_x >= x + 36.0 {
                    let pill = Rect::from_min_size(egui::pos2(pill_x, cy - 14.0), egui::vec2(94.0, 28.0));
                    if open_clip_pill(ui, pill, ui.id().with("perf-open-clip"), "open clip").clicked() {
                        open_pad_clip = Some(sp);
                    }
                }
            }
        }

        if button(ui, sr, ui.id().with("stopall"), "Stop all", false, Some(Icon::Square), None).clicked() {
            do_stop = true;
        }
        if button(ui, fr, ui.id().with("fs"), "Fullscreen", true, Some(Icon::Fullscreen), Some("F")).clicked() {
            do_fs = true;
        }

        if let Some(perf) = host.perform() {
            if let Some(q) = set_q {
                perf.set_quantize(q);
            }
            if let Some(o) = set_out {
                if o == 1 {
                    perf.open_output();
                } else if out_open {
                    perf.close_output();
                }
            }
            if do_stop {
                perf.stop_all();
            }
        }
        if do_fs {
            host.toggle_fullscreen(ui.ctx());
        }
        if let Some(sp) = open_pad_clip {
            self.open_clip(ClipTarget::Pad(sp));
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
            let top = Rect::from_min_size(full.min + egui::vec2(20.0, 20.0), egui::vec2(full.width() - 40.0, 40.0));
            let beats = host.playhead_beats().max(0.0);
            let pos = format!("{:03}.{}", (beats / 4.0).floor() as u32 + 1, (beats % 4.0).floor() as u32 + 1);
            let pr = p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), CREAM);
            p.rect_filled(pr.expand2(egui::vec2(14.0, 9.0)), r(10), Color32::from_black_alpha(150));
            p.text(egui::pos2(top.left() + 14.0, top.center().y), Align2::LEFT_CENTER, &pos, mono(13.0), CREAM);
            let xr = Rect::from_min_size(egui::pos2(top.right() - 96.0, top.top()), egui::vec2(96.0, 40.0));
            p.rect_filled(xr, r(10), Color32::from_black_alpha(150));
            if hit(ui, xr, ui.id().with("fs-exit"), Sense::click(), "exit fullscreen").clicked() {
                host.toggle_fullscreen(ui.ctx());
            }
            runs(&p, xr.center(), Align2::CENTER_CENTER, &[("Exit  ", font_medium(13.0), CREAM), ("F", mono(10.0), CREAM3)]);
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
            let names_w = if names.is_empty() { 0.0 } else { p.layout_no_wrap(names.clone(), font(12.0), CREAM2).size().x + 22.0 };
            let info = Rect::from_min_max(bl - egui::vec2(0.0, 64.0), bl + egui::vec2(206.0 + names_w, 0.0));
            p.rect_filled(info, r(12), Color32::from_black_alpha(150));
            runs(
                &p,
                egui::pos2(info.left() + 16.0, info.center().y),
                Align2::LEFT_CENTER,
                &[(&format!("{bpm:.0}"), mono_medium(26.0), CREAM), (" bpm", font(12.0), CREAM3)],
            );
            let bi = (beats % 4.0).floor() as usize;
            for k in 0..4 {
                p.circle_filled(egui::pos2(info.left() + 140.0 + k as f32 * 14.0, info.center().y), 3.5, if k == bi && playing { AMBER_MID } else { CREAM3 });
            }
            if !names.is_empty() {
                p.text(egui::pos2(info.left() + 206.0, info.center().y), Align2::LEFT_CENTER, &names, font(12.0), CREAM2);
            }
            let (mw, mh, mg) = (52.0, 32.0, 5.0);
            let gsize = egui::vec2(4.0 * mw + 3.0 * mg, 4.0 * mh + 3.0 * mg);
            let grid = Rect::from_min_size(egui::pos2(full.right() - 28.0 - gsize.x, full.bottom() - 28.0 - gsize.y), gsize);
            p.rect_filled(grid.expand(8.0), r(12), Color32::from_black_alpha(150));
            let ctx = ui.ctx().clone();
            let mut toggle = None;
            for (i, pad) in pads.iter().enumerate().take(16) {
                let c = Rect::from_min_size(grid.min + egui::vec2((i % 4) as f32 * (mw + mg), (i / 4) as f32 * (mh + mg)), egui::vec2(mw, mh));
                match pad.source.and_then(|s| self.atlas(host, &ctx, s)) {
                    Some((tex, a)) => tile(&p, c, 5, tex, a.uv_cover(5, mw / mh), gray(if pad.on { 0.95 } else { 0.45 })),
                    None => {
                        p.rect_filled(c, r(5), WELL2);
                    }
                }
                if pad.on {
                    p.rect_stroke(c, r(5), Stroke::new(1.5, AMBER_MID), StrokeKind::Inside);
                } else if pad.queued.is_some() && (ui.input(|i| i.time) * 3.0).fract() < 0.6 {
                    p.rect_stroke(c, r(5), Stroke::new(1.0, CREAM2), StrokeKind::Inside);
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
