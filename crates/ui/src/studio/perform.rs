//! Perform: true-proportion 2-column hardware console (`VIDEOGRAIN`):
//! - Left column (~52%): 100% unobstructed `#111314` Stage Monitor + dedicated
//!   hardware bezel strip (`grid · collage · split · stack` + `16:9 · 1:1 · 9:16` + `Fullscreen`),
//!   6 backlit amber Punch-In switches (`Stutter · Grid × · Cutout · Tape Stop · Comb · Crush`),
//!   and the 3-tier `ISOL8 · 5-BAND FREQUENCY ISOLATOR` rack bay.
//! - Right column (~48%): compact `SET` scene cue bar + 1-tap `Vocal Kit` & `Cutout`
//!   hardware actions, crisp `4×4` pads with bold role pills (`KICK`, `SNARE`, `VOX`,
//!   `CHORD`, `BASS`, `HOOK`, `LOOP`) + die-cut `✂` badges, and the Selected Pad
//!   4-knob hardware strip (`PITCH`, `GATE`, `SCULPT`, `LEVEL` + `Quant` + `Open clip ›`).

use super::{clean_clip_label, role_tag_for, ClipTarget, Studio, PUNCH_FX_A11Y, PUNCH_FX_LABELS};
use crate::atlas;
use crate::host::{Host, Pad};
use crate::studio::play::short;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::envelope::Adsr;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind};

impl Studio {
    pub fn perform_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let narrow = area.width() < 1120.0;
        let compact_h = area.height() < 620.0;
        let col_gap = if narrow { 10.0 } else { 16.0 };

        let left_w = ((area.width() - col_gap) * 0.52).floor();
        let right_w = area.width() - left_w - col_gap;
        let left_col = Rect::from_min_size(area.min, egui::vec2(left_w, area.height()));
        let right_col = Rect::from_min_size(egui::pos2(left_col.right() + col_gap, area.top()), egui::vec2(right_w, area.height()));

        // =====================================================================
        // Left Column (~52%): Stage + Bezel Strip + 6 Punch Switches + ISOL8
        // =====================================================================
        let fx_h = if compact_h { 28.0 } else { 34.0 };
        let fx_gap = if compact_h { 5.0 } else { 8.0 };
        let isol8_gap = if compact_h { 6.0 } else { 10.0 };
        let min_isol8_h = if compact_h { 244.0 } else { 360.0 };
        let ideal_stage_block_h = (left_w * 9.0 / 16.0 + 30.0).floor();
        let max_stage_block_h = (left_col.height() - fx_gap - fx_h - isol8_gap - min_isol8_h).max(176.0);
        let stage_block_h = ideal_stage_block_h.min(max_stage_block_h);

        let stage_block = Rect::from_min_size(left_col.min, egui::vec2(left_w, stage_block_h));
        let (stage_aperture, _) = self.stage_with_bezel(host, ui, stage_block, "perf-stage", true);
        if let Some(sub) = host.perform().and_then(|p| p.output()) {
            let p = ui.painter();
            p.rect_filled(stage_aperture, r(7), Color32::from_black_alpha(190));
            draw_icon(p, Icon::Display2, stage_aperture.center() - egui::vec2(0.0, 22.0), 36.0, CREAM2);
            p.text(stage_aperture.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "Live on Display 2", font_medium(14.5), CREAM);
            p.text(stage_aperture.center() + egui::vec2(0.0, 30.0), Align2::CENTER_CENTER, sub, mono(10.5), CREAM3);
        }

        let fx_rect = Rect::from_min_size(
            egui::pos2(left_col.left(), stage_block.bottom() + fx_gap),
            egui::vec2(left_w, fx_h),
        );
        self.perform_punch_fx_bar(host, ui, fx_rect);

        let isol8_rect = Rect::from_min_max(
            egui::pos2(left_col.left(), fx_rect.bottom() + isol8_gap),
            left_col.max,
        );
        self.isolator_rack_ui(host, ui, isol8_rect, "perf-isol8");

        // =====================================================================
        // Right Column (~48%): Top Cue/Action Bar + 4×4 Pads + Selected Pad Bay
        // =====================================================================
        let top_h = if compact_h { 28.0 } else { 32.0 };
        let top_gap = if compact_h { 6.0 } else { 8.0 };
        let strip_h = if compact_h { 96.0 } else if narrow { 112.0 } else { 126.0 };
        let strip_gap = if compact_h { 6.0 } else { 9.0 };

        let top_bar_r = Rect::from_min_size(right_col.min, egui::vec2(right_w, top_h));
        self.perform_right_top_bar(host, ui, top_bar_r);

        let pad_strip_r = Rect::from_min_max(
            egui::pos2(right_col.left(), right_col.bottom() - strip_h),
            right_col.max,
        );
        let pads_r = Rect::from_min_max(
            egui::pos2(right_col.left(), top_bar_r.bottom() + top_gap),
            egui::pos2(right_col.right(), pad_strip_r.top() - strip_gap),
        );
        self.pads(host, ui, pads_r);
        self.perform_pad_strip(host, ui, pad_strip_r);
    }

    fn perform_right_top_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bar: Rect) {
        let p = ui.painter().clone();
        let compact = bar.width() < 520.0;
        let h = bar.height();

        // Right-anchored 1-tap hardware actions: `Vocal Kit` (`"perform ukg kit"`) + `Cutout` (`"perform cutout"`).
        let cut_w = if compact { 72.0 } else { 88.0 };
        let kit_w = if compact { 82.0 } else { 98.0 };
        let cut_r = Rect::from_min_size(egui::pos2(bar.right() - cut_w, bar.top()), egui::vec2(cut_w, h));
        let kit_r = Rect::from_min_size(egui::pos2(cut_r.left() - 5.0 - kit_w, bar.top()), egui::vec2(kit_w, h));

        let sel_src = host
            .perform()
            .and_then(|pf| pf.pads().get(self.sel_pad).and_then(|pd| pd.source))
            .or_else(|| {
                host.perform()
                    .and_then(|pf| pf.pads().iter().find_map(|pd| pd.source))
            });
        let sel_has_alpha = sel_src.is_some_and(|s| self.source_has_alpha(host, s));

        let c_resp = hit(ui, cut_r, ui.id().with("perf-cutout"), Sense::click(), "perform cutout");
        paint_switch_cap(&p, cut_r, SwitchKind::PuttyMint, sel_has_alpha, c_resp.hovered(), c_resp.is_pointer_button_down_on());
        let c_fg = if sel_has_alpha { Color32::from_rgb(0x11, 0x2b, 0x1f) } else { INK };
        draw_icon(&p, Icon::Cutout, egui::pos2(cut_r.left() + if compact { 12.0 } else { 14.5 }, cut_r.center().y), 12.0, c_fg);
        p.text(
            egui::pos2(cut_r.left() + if compact { 22.0 } else { 26.0 }, cut_r.center().y),
            Align2::LEFT_CENTER,
            "Cutout",
            font_semibold(if compact { 10.0 } else { 11.0 }),
            c_fg,
        );
        if c_resp.clicked() {
            if let Some(src) = sel_src {
                self.trigger_cutout(host, src, "person");
            }
        }

        let k_resp = hit(ui, kit_r, ui.id().with("perf-ukg-kit"), Sense::click(), "perform ukg kit");
        paint_switch_cap(&p, kit_r, SwitchKind::Amber, false, k_resp.hovered(), k_resp.is_pointer_button_down_on());
        let k_fg = Color32::from_rgb(0x24, 0x1c, 0x09);
        draw_icon(&p, Icon::Grid, egui::pos2(kit_r.left() + if compact { 12.0 } else { 14.5 }, kit_r.center().y), 11.5, k_fg);
        p.text(
            egui::pos2(kit_r.left() + if compact { 22.0 } else { 26.0 }, kit_r.center().y),
            Align2::LEFT_CENTER,
            "Vocal Kit",
            font_semibold(if compact { 10.0 } else { 11.0 }),
            k_fg,
        );
        if k_resp.clicked() {
            self.trigger_ukg_kit(host);
        }

        let left_avail = kit_r.left() - bar.left() - 8.0;
        if host.caps().scenes && left_avail >= 150.0 {
            let set_rect = Rect::from_min_size(bar.min, egui::vec2(left_avail, h));
            self.set_arranger_bar(host, ui, set_rect, "perf-set");
        } else if left_avail >= 90.0 {
            caps(&p, egui::pos2(bar.left() + 2.0, bar.center().y), Align2::LEFT_CENTER, "4×4 PAD BANK", 9.5);
            let beat_i = (host.playhead_beats().max(0.0) % 4.0).floor() as usize;
            let playing = host.playing();
            for k in 0..4 {
                jewel_led(
                    &p,
                    egui::pos2(bar.left() + 104.0 + k as f32 * 11.0, bar.center().y),
                    2.8,
                    playing && k == beat_i,
                    false,
                );
            }
        }
    }

    fn perform_punch_fx_bar(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, bar: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let g = if bar.width() < 520.0 { 4.5 } else { 6.0 };
        let pw = (bar.width() - g * 5.0) / 6.0;
        let t_now = ui.input(|i| i.time);
        for k in 0..6 {
            let rc = Rect::from_min_size(egui::pos2(bar.left() + k as f32 * (pw + g), bar.top()), egui::vec2(pw, bar.height()));
            let resp = hit(ui, rc, ui.id().with(("punch-fx", k)), Sense::click_and_drag(), PUNCH_FX_A11Y[k]);
            let down = resp.is_pointer_button_down_on();
            if down && self.punch_press_t[k].is_none() {
                self.punch_press_t[k] = Some(t_now);
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
            let fsz = if pw < 80.0 { 9.5 } else { 11.0 };
            p.text(
                rc.center(),
                Align2::CENTER_CENTER,
                PUNCH_FX_LABELS[k],
                font_semibold(fsz),
                if on { ON_HI } else { Color32::from_rgb(0x24, 0x1c, 0x09) },
            );
            // Subtle engraved `[5..0]` keyboard legend in top-right corner of cap.
            if pw >= 62.0 {
                p.text(
                    egui::pos2(rc.right() - 5.5, rc.top() + 6.5),
                    Align2::RIGHT_CENTER,
                    super::PUNCH_FX_KEY_HINTS[k],
                    mono(7.8),
                    if on {
                        Color32::from_black_alpha(150)
                    } else {
                        Color32::from_rgb(0x3a, 0x2f, 0x14)
                    },
                );
            }
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
                    let (base_frame, has_alpha) = {
                        let pp = host.project().lock().unwrap();
                        let ha = pp
                            .sources
                            .get(src)
                            .and_then(|so| so.video.as_ref())
                            .is_some_and(|v| v.has_alpha());
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
                        (bf, ha)
                    };
                    if let Some((tex, a)) = self.atlas(host, &ctx, src) {
                        let frame = if on && playing { (base_frame + (now * 4.0) as usize) % atlas::FRAMES } else { base_frame };
                        let bright = if on { 0.94 } else if is_sel { 0.72 } else { 0.54 };
                        tile(&p, rc, 6, tex, a.uv_cover(frame, pw / ph), gray(bright));
                    } else {
                        p.rect_filled(rc, r(6), WELL2);
                    }
                    let top_scrim = Rect::from_min_size(rc.min, egui::vec2(pw, 24.0_f32.min(ph * 0.42)));
                    p.rect_filled(top_scrim, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_black_alpha(125));
                    let scrim = Rect::from_min_max(egui::pos2(rc.left(), (rc.bottom() - 25.0).max(rc.top())), rc.max);
                    p.rect_filled(scrim, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, Color32::from_black_alpha(155));

                    // Bold Role Tag Pill (`KICK`, `SNARE`, `VOX`, `CHORD`, `BASS`, `HOOK`, `LOOP`) + `✂` Cutout Badge!
                    let raw_label = pad.name.clone().unwrap_or_else(|| self.source_name(host, src));
                    let role = role_tag_for(self.pad_sculpt[i].stamp, pad.name.as_deref(), Some(pad.kind));
                    let role_r = role_tag_pill(&p, egui::pos2(rc.left() + 6.0, rc.top() + 5.0), role, on);
                    if has_alpha && role_r.right() + 21.0 < rc.right() - 26.0 {
                        cutout_badge(&p, egui::pos2(role_r.right() + 4.0, rc.top() + 5.0));
                    }

                    // Clean human-readable short title at bottom-left, mini ADSR at bottom-right.
                    let clean_title = clean_clip_label(&raw_label);
                    let show_adsr = pw >= 116.0;
                    let adsr_reserve = if show_adsr { 34.0 } else { 8.0 };
                    let room = ((pw - 14.0 - adsr_reserve) / 6.2).max(5.0) as usize;
                    p.text(
                        egui::pos2(rc.left() + 7.0, rc.bottom() - 6.0),
                        Align2::LEFT_BOTTOM,
                        short(&clean_title, room.min(18)),
                        font_medium(if pw < 112.0 { 9.5 } else { 10.5 }),
                        if on || is_sel { CREAM } else { CREAM2 },
                    );
                    if show_adsr {
                        let env_r = Rect::from_min_size(
                            egui::pos2(rc.right() - 32.0, rc.bottom() - 16.5),
                            egui::vec2(25.0, 9.5),
                        );
                        mini_adsr(&p, env_r, &pad_envs[i], if on || is_sel { 0.90 } else { 0.62 });
                    }

                    if on {
                        p.rect_stroke(rc, r(6), Stroke::new(2.2, AMBER_MID), StrokeKind::Inside);
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
                        p.rect_stroke(rc, r(6), Stroke::new(1.6, CREAM), StrokeKind::Inside);
                    } else {
                        p.rect_stroke(rc, r(6), hair(WELL_LINE), StrokeKind::Inside);
                    }

                    if let Some(target) = pad.queued {
                        if (t_ui * 3.0).fract() < 0.6 {
                            let tip = egui::pos2(rc.right() - 34.0, rc.top() + 13.0);
                            let pts = if target {
                                vec![tip + egui::vec2(-3.5, -4.5), tip + egui::vec2(4.5, 0.0), tip + egui::vec2(-3.5, 4.5)]
                            } else {
                                vec![tip + egui::vec2(-3.5, -3.5), tip + egui::vec2(3.5, -3.5), tip + egui::vec2(3.5, 3.5), tip + egui::vec2(-3.5, 3.5)]
                            };
                            p.add(egui::Shape::convex_polygon(pts, AMBER_MID, Stroke::NONE));
                        }
                        ctx.request_repaint();
                    }

                    let chev_vis = Rect::from_min_size(egui::pos2(rc.right() - 23.0, rc.top() + 4.0), egui::vec2(19.0, 19.0));
                    let chev_hit = Rect::from_center_size(chev_vis.center(), egui::vec2(24.0, 24.0));
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
                    draw_icon(&p, Icon::Plus, rc.center(), 14.0, CREAM3);
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

    /// Selected Pad Hardware Strip at the bottom of Perform's Right Column:
    /// Header (`PAD N · name` + `Now|Beat|Bar` + `Stop all` + `Open clip ›`) +
    /// 4 real `ISOL8`-style charcoal pointer knobs & silver LCDs (`PITCH`, `GATE`, `SCULPT`, `LEVEL`).
    fn perform_pad_strip(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, strip: Rect) {
        let p = ui.painter().clone();
        putty_panel(&p, strip, 7);
        let compact = strip.height() < 108.0 || strip.width() < 520.0;
        let inner = strip.shrink2(egui::vec2(if compact { 8.0 } else { 12.0 }, if compact { 5.0 } else { 8.0 }));

        let (pads, quant) = match host.perform() {
            Some(perf) => (perf.pads(), perf.quantize()),
            None => return,
        };
        let sp = self.sel_pad.min(15);
        let sel_pad_info = pads.get(sp).cloned();

        // --- Header row: `PAD N · title` + `Quant: Now|Beat|Bar` + `Stop` + `Open clip ›` ---
        let head_h = if compact { 23.0 } else { 26.0 };
        let head = Rect::from_min_size(inner.min, egui::vec2(inner.width(), head_h));
        let mut rx = head.right();

        let open_w = if compact { 82.0 } else { 92.0 };
        let open_r = Rect::from_min_size(egui::pos2(rx - open_w, head.top()), egui::vec2(open_w, head_h));
        rx = open_r.left() - 5.0;
        let mut open_pad_clip = None;
        if open_clip_pill(ui, open_r, ui.id().with("perf-open-clip"), "open clip").clicked() {
            open_pad_clip = Some(sp);
        }

        let stop_w = if compact { 52.0 } else { 68.0 };
        let stop_r = Rect::from_min_size(egui::pos2(rx - stop_w, head.top()), egui::vec2(stop_w, head_h));
        rx = stop_r.left() - 5.0;
        let mut do_stop = false;
        if button_named(
            ui,
            stop_r,
            ui.id().with("stopall"),
            if compact { "Stop" } else { "Stop all" },
            "Stop all",
            false,
            None,
            None,
        )
        .clicked()
        {
            do_stop = true;
        }

        let q_w = if compact { 104.0 } else { 126.0 };
        let mut set_q = None;
        if rx - q_w >= head.left() + 86.0 {
            let q_r = Rect::from_min_size(egui::pos2(rx - q_w, head.top()), egui::vec2(q_w, head_h));
            rx = q_r.left() - 6.0;
            if let Some(nq) = segmented(ui, q_r, ui.id().with("launch"), &["Now", "Beat", "Bar"], &[], quant, font_semibold(if compact { 9.5 } else { 10.5 }), false) {
                set_q = Some(nq);
            }
        }

        let title_str = match sel_pad_info.as_ref().and_then(|pd| pd.source.map(|s| (pd, s))) {
            Some((pd, src)) => {
                let raw = pd.name.clone().unwrap_or_else(|| self.source_name(host, src));
                let clean = clean_clip_label(&raw);
                let max_c = ((rx - head.left() - 54.0) / 6.2).max(5.0) as usize;
                format!("PAD {} · {}", sp + 1, short(&clean, max_c.min(18)))
            }
            None => format!("PAD {} · empty", sp + 1),
        };
        p.text(
            egui::pos2(head.left() + 2.0, head.center().y),
            Align2::LEFT_CENTER,
            title_str,
            font_semibold(if compact { 10.5 } else { 11.5 }),
            INK,
        );

        // --- Bottom row: 4 Charcoal Pointer Knobs + Silver LCDs (`PITCH`, `GATE`, `SCULPT`, `LEVEL`) ---
        let knobs_r = Rect::from_min_max(egui::pos2(inner.left(), head.bottom() + if compact { 3.0 } else { 6.0 }), inner.max);
        let owner = super::clip::EnvTarget::Pad(sp);
        let mut op = self.read_owner_params_pub(host, owner);
        let mut adsr = self.target_env(host, owner).unwrap_or_default();
        let mut sculpt = self.pad_sculpt[sp];
        let before = (op.pitch_m, adsr.release, sculpt.drive_db, op.level);

        let mut n_pitch = op.pitch_m.clamp(0.0, 1.0);
        let mut n_gate = ((adsr.release - 0.02) / 1.48).clamp(0.0, 1.0);
        let mut n_sculpt = (sculpt.drive_db / 18.0).clamp(0.0, 1.0);
        let mut n_level = op.level.clamp(0.0, 1.0);

        let kw = knobs_r.width() / 4.0;
        let lcd_h = if compact { 14.5 } else { 16.5 };
        let krad = ((knobs_r.height() - lcd_h - if compact { 12.0 } else { 16.0 }) * 0.45).clamp(11.5, 19.0);
        let ky = knobs_r.top() + krad + 1.5;
        let id = ui.id().with(("perf-pad-knobs", sp));

        let specs: [(&str, &str, f32); 4] = [
            ("PITCH", "pad pitch", 0.5),
            ("GATE", "pad gate", (0.14 - 0.02) / 1.48),
            ("SCULPT", "pad sculpt", 0.2),
            ("LEVEL", "pad level", 0.8),
        ];

        for (k, &(lbl, a11y, def)) in specs.iter().enumerate() {
            let cx = knobs_r.left() + (k as f32 + 0.5) * kw;
            let vref = match k {
                0 => &mut n_pitch,
                1 => &mut n_gate,
                2 => &mut n_sculpt,
                _ => &mut n_level,
            };
            knob(ui, egui::pos2(cx, ky), krad, id.with(k), a11y, vref, def, 0);
            if !compact {
                caps(&p, egui::pos2(cx, ky + krad + 6.5), Align2::CENTER_CENTER, lbl, 7.8);
            } else {
                p.text(egui::pos2(cx + krad * 0.88, ky - krad * 0.55), Align2::LEFT_CENTER, &lbl[..3.min(lbl.len())], font_semibold(7.5), INK3);
            }
        }

        op.pitch_m = n_pitch;
        adsr.release = 0.02 + n_gate * 1.48;
        sculpt.drive_db = n_sculpt * 18.0;
        op.level = n_level;

        let pitch_st = ((op.pitch_m - 0.5) * 24.0).round();
        let vals = [
            format!("{pitch_st:+.0} st").replace("+0 st", "0 st"),
            if adsr.release >= 1.0 {
                format!("{:.2}s", adsr.release)
            } else {
                format!("{:.0}ms", adsr.release * 1000.0)
            },
            format!("{:.1}dB", sculpt.drive_db),
            format!("{:.0}%", op.level * 100.0),
        ];
        for (k, vs) in vals.iter().enumerate() {
            let cx = knobs_r.left() + (k as f32 + 0.5) * kw;
            let lcd_w = (kw - if compact { 10.0 } else { 18.0 }).clamp(40.0, 68.0);
            let lcd_y = knobs_r.bottom() - lcd_h * 0.5 - 1.0;
            lcd_readout(&p, Rect::from_center_size(egui::pos2(cx, lcd_y), egui::vec2(lcd_w, lcd_h)), vs, if compact { 9.0 } else { 9.5 });
        }

        if (op.pitch_m, op.level) != (before.0, before.3) {
            self.write_owner_params_pub(host, owner, op);
        }
        if (adsr.release - before.1).abs() > 1e-4 {
            self.set_target_env(host, owner, adsr);
        }
        if (sculpt.drive_db - before.2).abs() > 1e-4 {
            self.set_owner_sculpt(host, owner, sculpt);
        }

        if let Some(perf) = host.perform() {
            if let Some(q) = set_q {
                perf.set_quantize(q);
            }
            if do_stop {
                perf.stop_all();
            }
        }
        if let Some(pad_i) = open_pad_clip {
            self.open_clip(ClipTarget::Pad(pad_i));
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
                .map(|s| clean_clip_label(&s))
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
