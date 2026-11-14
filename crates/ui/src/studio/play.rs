//! Play: 8 clip columns (with calm single-slot empty tape bays) on the left +
//! unobstructed `#111314` Stage Monitor with dedicated bezel strip and 4
//! `ISOL8`-style Hardware Emitter Channel Modules (`Grain`, `Pitch`, `Smear`,
//! `Level` pointer knobs + sunken silver LCDs + mini ADSR + `Clip ›`) on the right.

use super::{clean_clip_label, ClipTarget, Studio, COLUMNS, EMITTERS, ROWS};
use crate::atlas;
use crate::host::Host;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::instrument::{MacroMap, LOOKS};
use egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind};

impl Studio {
    pub fn play_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let small = area.width() < 1120.0;
        let gap = if small { 10.0 } else { 16.0 };
        let grid_w = ((area.width() - gap) * 0.48).floor();
        let grid = Rect::from_min_size(area.min, egui::vec2(grid_w, area.height()));
        let side = Rect::from_min_max(egui::pos2(grid.right() + gap, area.top()), area.max);
        self.play_grid(host, ui, grid);

        let ideal_stage_h = (side.width() * 9.0 / 16.0 + 30.0).floor();
        let max_stage_h = (side.height() * if small { 0.46 } else { 0.48 }).floor();
        let stage_block = Rect::from_min_size(side.min, egui::vec2(side.width(), ideal_stage_h.min(max_stage_h)));
        self.stage_with_bezel(host, ui, stage_block, "play-stage", true);

        let brush = Rect::from_min_max(
            egui::pos2(side.left(), stage_block.bottom() + if small { 8.0 } else { 12.0 }),
            side.max,
        );
        self.brush_panel(host, ui, brush, small);
    }

    fn play_grid(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let g = 4.0;
        let head_h = if area.height() < 560.0 { 26.0 } else { 30.0 };
        let grid = Rect::from_min_max(egui::pos2(area.left(), area.top() + head_h + 5.0), area.max);
        let cw = (grid.width() - g * 7.0) / 8.0;
        let ch = (grid.height() - g * (ROWS as f32 - 1.0)) / ROWS as f32;
        let cell = |c: usize, r: usize| {
            Rect::from_min_size(egui::pos2(grid.left() + c as f32 * (cw + g), grid.top() + r as f32 * (ch + g)), egui::vec2(cw, ch))
        };
        let p = ui.painter().clone();
        monitor_well(&p, grid.expand(3.5), 7);
        let now = host.clock();
        let sel_col = self.emitters[self.sel_emitter].col;
        let pending = self.pending_slots(host);
        let t_ui = ui.input(|i| i.time);
        let mut open_add = None;
        let mut open_clip = None;
        let mut first_empty_col = true;

        for c in 0..COLUMNS {
            let src = self.columns[c];
            let head = Rect::from_min_size(egui::pos2(area.left() + c as f32 * (cw + g), area.top()), egui::vec2(cw, head_h));
            let hresp = hit(ui, head, ui.id().with(("col-head", c)), Sense::click(), &format!("column {}", c + 1));
            let hovered = hresp.hovered();
            let col_lit = src.is_some() && c == sel_col;
            paint_switch_cap(&p, head, SwitchKind::PuttyAmber, col_lit, hovered, hresp.is_pointer_button_down_on());
            match src {
                Some(s) => {
                    let h = hold(ui, &hresp, 0.45);
                    let has_alpha = self.source_has_alpha(host, s);
                    let raw = self.source_name(host, s);
                    let clean = clean_clip_label(&raw);
                    let right_pad = if has_alpha { 22.0 } else { 14.0 };
                    let room = ((cw - 8.0 - right_pad) / 6.0).max(3.0) as usize;
                    let col = if col_lit { ON_HI } else { INK };
                    let name_galley = p.layout_no_wrap(short(&clean, room), font_medium(if cw < 64.0 { 9.8 } else { 10.8 }), col);
                    let nx = head.left() + 5.0;
                    p.galley(egui::pos2(nx, head.center().y - name_galley.size().y * 0.5), name_galley, col);
                    if has_alpha && cw >= 56.0 {
                        draw_icon(&p, Icon::Cutout, egui::pos2(head.right() - 16.5, head.center().y), 9.5, if col_lit { ON_HI } else { INK2 });
                    }
                    draw_icon(
                        &p,
                        Icon::ChevronRight,
                        egui::pos2(head.right() - 6.5, head.center().y + 0.5),
                        9.5,
                        if col_lit { ON_HI } else { INK3 },
                    );
                    if h.fired {
                        open_clip = Some(c);
                    } else if hresp.clicked() && !h.held_before {
                        let on_chev = hresp.interact_pointer_pos().is_some_and(|pt| pt.x >= head.right() - 18.0);
                        if on_chev {
                            open_clip = Some(c);
                        } else {
                            self.emitters[self.sel_emitter].col = c;
                        }
                    }
                }
                None => {
                    if hresp.clicked() {
                        open_add = Some(c);
                    }
                    draw_icon(&p, Icon::Plus, head.center(), 12.5, if hovered { INK } else { INK3 });
                }
            }

            let col_rect = Rect::from_min_max(cell(c, 0).min, cell(c, ROWS - 1).max);
            if src.is_none() {
                if let Some((pd, _)) = pending.iter().find(|(_, t)| *t == crate::host::AddTarget::Column(c)) {
                    super::add::placeholder_tile(&p, col_rect, 5, pd, t_ui, true);
                    ctx.request_repaint();
                    continue;
                }
                // Calm single recessed brushed-slate tape slot (zero 64 noisy empty boxes!).
                p.rect_filled(col_rect, r(5), Color32::from_rgb(0x1d, 0x20, 0x21));
                p.rect_stroke(col_rect, r(5), hair(WELL_LINE), StrokeKind::Inside);
                let rail_x = col_rect.center().x;
                p.line_segment(
                    [egui::pos2(rail_x, col_rect.top() + 14.0), egui::pos2(rail_x, col_rect.center().y - 18.0)],
                    Stroke::new(1.0, Color32::from_white_alpha(16)),
                );
                p.line_segment(
                    [egui::pos2(rail_x, col_rect.center().y + 18.0), egui::pos2(rail_x, col_rect.bottom() - 14.0)],
                    Stroke::new(1.0, Color32::from_white_alpha(16)),
                );
                let plus_box = Rect::from_center_size(col_rect.center(), egui::vec2((cw - 10.0).clamp(22.0, 34.0), 28.0));
                p.rect_filled(plus_box, r(4), Color32::from_rgb(0x26, 0x2a, 0x2b));
                p.rect_stroke(plus_box, r(4), hair(WELL_LINE2), StrokeKind::Inside);
                draw_icon(&p, Icon::Plus, plus_box.center(), 14.0, CREAM3);
                if first_empty_col {
                    first_empty_col = false;
                    let slot_resp = hit(ui, plus_box, ui.id().with(("add-col", c)), Sense::click(), "add a clip");
                    if slot_resp.clicked() {
                        open_add = Some(c);
                    }
                }
                continue;
            }

            let at = src.and_then(|s| self.atlas(host, &ctx, s));
            for row in 0..ROWS {
                let rc = cell(c, row);
                if let Some((tex, a)) = at {
                    let lit = self
                        .lit
                        .iter()
                        .filter(|(k, _)| *k == (c, row))
                        .map(|(_, t)| (1.0 - (now - t) / 0.4).max(0.0) as f32)
                        .fold(0.0f32, f32::max);
                    let base = if c == sel_col { 0.88 } else { 0.72 };
                    let v = base + (1.0 - base) * lit;
                    tile(&p, rc, 4, tex, a.uv_cover(row * 2 + 1, cw / ch), gray(v));
                    if lit > 0.0 {
                        p.rect_filled(rc, r(4), Color32::from_rgba_unmultiplied(252, 227, 93, (lit * 42.0) as u8));
                    }
                } else {
                    p.rect_filled(rc, r(4), WELL2);
                }
            }
        }

        if let Some(c) = open_add {
            self.open_add(host, crate::host::AddTarget::Column(c));
        }
        if let Some(c) = open_clip {
            self.open_clip(ClipTarget::Column(c));
        }

        // Touch / pointer: press = the selected emitter jumps there and sprays while held.
        let resp = hit(ui, grid, ui.id().with("play-grid"), Sense::click_and_drag(), "play grid");
        let to_grid = |pos: egui::Pos2| -> (usize, f32) {
            let c = (((pos.x - grid.left()) / (cw + g)).floor() as i32).clamp(0, 7) as usize;
            let row = ((pos.y - grid.top()) / (ch + g)).clamp(0.0, ROWS as f32 - 0.01);
            (c, row)
        };
        let center_of = |col: usize, row: f32| egui::pos2(grid.left() + col as f32 * (cw + g) + cw / 2.0, grid.top() + row * (ch + g));
        let ring_pos = |col: usize, row: f32| {
            let c = center_of(col, row);
            egui::pos2(c.x, c.y.clamp(grid.top() + 16.0, grid.bottom() - 16.0))
        };
        let pick = |st: &Studio, pos: egui::Pos2, taken: &[usize]| -> usize {
            let near = (0..EMITTERS)
                .filter(|i| !taken.contains(i))
                .map(|i| (i, (ring_pos(st.emitters[i].col, st.emitters[i].row) - pos).length()))
                .filter(|(_, d)| *d < 24.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            near.or_else(|| (!taken.contains(&st.sel_emitter)).then_some(st.sel_emitter))
                .or_else(|| (0..EMITTERS).find(|i| !taken.contains(i) && !st.emitters[*i].held))
                .unwrap_or(st.sel_emitter)
        };

        let touches: Vec<(u64, egui::TouchPhase, egui::Pos2)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Touch { id, phase, pos, .. } => Some((id.0, *phase, *pos)),
                    _ => None,
                })
                .collect()
        });
        for (id, phase, pos) in touches {
            let slot = self.touches.iter().position(|(t, _)| *t == id);
            match phase {
                egui::TouchPhase::Start if grid.contains(pos) && slot.is_none() => {
                    let taken: Vec<usize> = self.touches.iter().map(|(_, e)| *e).collect();
                    let i = pick(self, pos, &taken);
                    self.touches.push((id, i));
                    self.sel_emitter = i;
                    let (c, row) = to_grid(pos);
                    let e = &mut self.emitters[i];
                    (e.col, e.row, e.held) = (c, row, true);
                }
                egui::TouchPhase::Move => {
                    if let Some(k) = slot {
                        let i = self.touches[k].1;
                        let (c, row) = to_grid(pos);
                        let e = &mut self.emitters[i];
                        (e.col, e.row) = (c, row);
                    }
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                    if let Some(k) = slot {
                        let (_, i) = self.touches.remove(k);
                        self.emitters[i].held = false;
                    }
                }
                _ => {}
            }
        }

        let fingers = !self.touches.is_empty();
        let down = !fingers && (resp.is_pointer_button_down_on() || resp.clicked());
        if down {
            if let Some(pos) = resp.interact_pointer_pos() {
                let (c, row) = to_grid(pos);
                if self.columns[c].is_none() && resp.clicked() {
                    self.open_add(host, crate::host::AddTarget::Column(c));
                } else {
                    let grabbing = ui.data(|d| d.get_temp::<Option<usize>>(resp.id)).flatten();
                    let who = match grabbing {
                        Some(i) => i,
                        None => {
                            let i = pick(self, pos, &[]);
                            ui.data_mut(|d| d.insert_temp(resp.id, Some(i)));
                            i
                        }
                    };
                    self.sel_emitter = who;
                    let e = &mut self.emitters[who];
                    e.col = c;
                    e.row = row;
                    e.held = !resp.clicked() || resp.is_pointer_button_down_on();
                }
            }
        }
        if !resp.is_pointer_button_down_on() || fingers {
            if let Some(i) = ui.data(|d| d.get_temp::<Option<usize>>(resp.id)).flatten() {
                if !self.touches.iter().any(|(_, e)| *e == i) {
                    self.emitters[i].held = false;
                }
            }
            ui.data_mut(|d| d.insert_temp(resp.id, None::<usize>));
        }

        for i in 0..EMITTERS {
            let e = &self.emitters[i];
            if self.columns.get(e.col).copied().flatten().is_none() {
                continue;
            }
            let active = e.held || now - e.last_fire < 0.3;
            emitter_ring_pitch(
                &p,
                ring_pos(e.col, e.row),
                if grid.width() < 600.0 { 12.5 } else { 15.0 },
                i + 1,
                i == self.sel_emitter,
                active,
                now,
                Some(e.macros[1]),
            );
        }
        if self.emitters.iter().any(|e| e.held) {
            ctx.request_repaint();
        }
    }

    fn brush_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect, narrow_window: bool) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let small = rect.height() < 320.0 || narrow_window;
        putty_panel(&p, rect, 8);
        let pad = if small { 7.0 } else { 11.0 };
        let inner = rect.shrink(pad);

        // 1. Top Contextual Header + `REC TAKE` / `Load clip` + `Open clip ›`
        let bar_h = if small { 26.0 } else { 30.0 };
        let obj_bar = Rect::from_min_size(inner.min, egui::vec2(inner.width(), bar_h));
        let sel = self.sel_emitter.min(EMITTERS - 1);
        let sel_col = self.emitters[sel].col;
        let sel_src = self.columns.get(sel_col).copied().flatten();
        let raw_sel_name = sel_src.map(|s| self.source_name(host, s)).unwrap_or_else(|| "empty".into());
        let sel_clip_name = clean_clip_label(&raw_sel_name);

        let mut rx = obj_bar.right();
        let open_w = if small { 84.0 } else { 94.0 };
        let open_pill_r = Rect::from_min_size(egui::pos2(rx - open_w, obj_bar.top()), egui::vec2(open_w, bar_h));
        rx = open_pill_r.left() - 6.0;
        let mut open_clip_em: Option<usize> = None;
        if open_clip_pill(ui, open_pill_r, ui.id().with("play-open-clip"), "open clip").clicked() {
            open_clip_em = Some(sel);
        }

        // Compact hardware `REC TAKE` (standalone) or `Load clip` (plugin) right in the emitter bay header!
        let recording = self.rec.is_some();
        if !host.caps().rec {
            let lw = if small { 82.0 } else { 96.0 };
            let lr = Rect::from_min_size(egui::pos2(rx - lw, obj_bar.top()), egui::vec2(lw, bar_h));
            rx = lr.left() - 8.0;
            if button_named(ui, lr, ui.id().with("load-clip"), "Load clip", "load a clip", false, Some(Icon::Plus), None).clicked() {
                host.import_media();
            }
        } else {
            let rw = if small { 92.0 } else { 118.0 };
            let rr = Rect::from_min_size(egui::pos2(rx - rw, obj_bar.top()), egui::vec2(rw, bar_h));
            rx = rr.left() - 8.0;
            let fresp = hit(ui, rr, ui.id().with("rec-big"), Sense::click(), "rec");
            paint_switch_cap(&p, rr, SwitchKind::Ruby, recording, fresp.hovered(), fresp.is_pointer_button_down_on());
            if fresp.clicked() {
                self.toggle_rec(host);
            }
            let fg = Color32::from_rgb(0x24, 0x0b, 0x09);
            let dot_c = egui::pos2(rr.left() + 12.0, rr.center().y);
            p.circle_filled(dot_c, 4.2, fg);
            let secs = self.rec.as_ref().map(|r| host.clock() - r.t0).unwrap_or(0.0);
            let lbl = if recording {
                format!("STOP {}:{:02}", (secs / 60.0) as u32, secs as u32 % 60)
            } else if small {
                "REC TAKE".into()
            } else {
                "REC TAKE".into()
            };
            p.text(egui::pos2(dot_c.x + 10.0, dot_c.y), Align2::LEFT_CENTER, lbl, font_semibold(if small { 9.5 } else { 10.5 }), fg);
            if recording {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }

        let max_t_chars = ((rx - obj_bar.left() - 84.0) / 6.5).max(5.0) as usize;
        let title = format!("EMITTER {} · {}", sel + 1, short(&sel_clip_name, max_t_chars.min(20)));
        p.text(
            egui::pos2(obj_bar.left() + 2.0, obj_bar.center().y),
            Align2::LEFT_CENTER,
            &title,
            font_semibold(if small { 11.0 } else { 12.0 }),
            INK,
        );

        // 2. 2×2 Grid of `ISOL8`-Style Hardware Emitter Channel Cards (`Grain`, `Pitch`, `Smear`, `Level` pointer knobs + silver LCDs).
        let vgap = if small { 6.0 } else { 9.0 };
        let cards_rect = Rect::from_min_max(egui::pos2(inner.left(), obj_bar.bottom() + vgap), inner.max);
        let cg = if small { 6.0 } else { 10.0 };
        let cw = ((cards_rect.width() - cg) * 0.5).max(80.0);
        let ch = ((cards_rect.height() - cg) * 0.5).max(68.0);
        let key = self.key;
        let now = host.clock();

        for i in 0..EMITTERS {
            let card = Rect::from_min_size(
                egui::pos2(
                    cards_rect.left() + (i % 2) as f32 * (cw + cg),
                    cards_rect.top() + (i / 2) as f32 * (ch + cg),
                ),
                egui::vec2(cw, ch),
            );
            let is_sel = i == self.sel_emitter;
            let (col, pos, mut macros, adsr, mut level, held, last_fire) = {
                let e = &self.emitters[i];
                (e.col, e.pos(), e.macros, e.adsr, e.level, e.held, e.last_fire)
            };
            let src = self.columns.get(col).copied().flatten();
            let raw_name = src.map(|s| self.source_name(host, s)).unwrap_or_default();
            let clip_name = clean_clip_label(&raw_name);
            let has_alpha = src.is_some_and(|s| self.source_has_alpha(host, s));

            // Base card faceplate (`chassis_plate` warm anodized metal + dark CRT header strip).
            chassis_plate(&p, card, CHASSIS_TOP, G0);
            p.rect_stroke(
                card,
                r(7),
                Stroke::new(if is_sel { 2.0 } else { 1.0 }, if is_sel { AMBER_MID } else { PANEL_LO }),
                StrokeKind::Inside,
            );

            // Base card hit zone (`"emitter 1"`..`"emitter 4"`): placed BEFORE child controls so knobs/buttons win pointer priority!
            let base_resp = hit(
                ui,
                card,
                ui.id().with(("emitter-tile", i)),
                Sense::click(),
                &format!("emitter {}", i + 1),
            );
            let hb = hold(ui, &base_resp, 0.45);
            if hb.fired {
                self.sel_emitter = i;
                open_clip_em = Some(i);
            } else if base_resp.clicked() && !hb.held_before {
                self.sel_emitter = i;
            }

            // Top dark CRT monitor strip (`26..32 px`) with footage preview, badge, title, ADSR, Look, and `Clip ›`.
            let strip_h = if ch < 126.0 { 25.0 } else { 31.0 };
            let top_strip = Rect::from_min_size(card.min + egui::vec2(2.0, 2.0), egui::vec2(cw - 4.0, strip_h));
            p.rect_filled(top_strip, CornerRadius { nw: 5, ne: 5, sw: 2, se: 2 }, WELL);
            if let Some((tex, a)) = src.and_then(|s| self.atlas(host, &ctx, s)) {
                tile(&p, top_strip.shrink(0.5), 4, tex, a.uv_cover(atlas::frame_at(pos), top_strip.width() / strip_h), gray(if is_sel { 0.48 } else { 0.34 }));
            }
            p.rect_filled(top_strip, CornerRadius { nw: 5, ne: 5, sw: 2, se: 2 }, Color32::from_black_alpha(125));

            // Numbered Emitter Badge `1..4`.
            let badge_c = egui::pos2(top_strip.left() + 13.0, top_strip.center().y);
            let badge_r = if ch < 126.0 { 7.5 } else { 8.5 };
            if is_sel {
                p.circle_filled(badge_c, badge_r, AMBER_MID);
                p.text(badge_c, Align2::CENTER_CENTER, (i + 1).to_string(), mono_medium(9.5), ON_HI);
            } else {
                p.circle_filled(badge_c, badge_r, Color32::from_black_alpha(165));
                p.circle_stroke(badge_c, badge_r, Stroke::new(1.1, CREAM2));
                p.text(badge_c, Align2::CENTER_CENTER, (i + 1).to_string(), mono(9.5), CREAM);
            }
            if held || now - last_fire < 0.25 {
                p.circle_stroke(badge_c, badge_r + 2.2, Stroke::new(1.2, AMBER_CORE));
            }

            // Top-right `Clip ›` button (`"clip emitter 1"`..`"clip emitter 4"`) + `Look` chip.
            let clip_w = if cw < 245.0 { 42.0 } else { 48.0 };
            let btn_h = (strip_h - 6.0).max(18.0);
            let clip_vis = Rect::from_min_size(
                egui::pos2(top_strip.right() - clip_w - 4.0, top_strip.center().y - btn_h * 0.5),
                egui::vec2(clip_w, btn_h),
            );
            let clip_resp = hit(
                ui,
                clip_vis.expand2(egui::vec2(2.0, 2.0)),
                ui.id().with(("em-clip", i)),
                Sense::click(),
                &format!("clip emitter {}", i + 1),
            );
            let clip_hov = clip_resp.hovered() || clip_resp.is_pointer_button_down_on();
            p.rect_filled(clip_vis, r(4), Color32::from_black_alpha(if clip_hov { 235 } else { 195 }));
            p.rect_stroke(clip_vis, r(4), hair(if clip_hov { AMBER_MID } else { WELL_LINE2 }), StrokeKind::Inside);
            p.text(
                egui::pos2(clip_vis.left() + 6.5, clip_vis.center().y),
                Align2::LEFT_CENTER,
                "Clip",
                font_semibold(if cw < 245.0 { 9.5 } else { 10.2 }),
                if clip_hov { AMBER_MID } else { CREAM },
            );
            draw_icon(
                &p,
                Icon::ChevronRight,
                egui::pos2(clip_vis.right() - 7.5, clip_vis.center().y + 0.5),
                9.5,
                if clip_hov { AMBER_MID } else { CREAM2 },
            );
            if clip_resp.clicked() {
                self.sel_emitter = i;
                open_clip_em = Some(i);
            }

            let map = MacroMap::resolve(&macros, Some(&key));
            let look_str = super::look_name(map.look);
            let mut right_edge = clip_vis.left() - 4.0;
            if cw >= 236.0 {
                let look_w = (p.layout_no_wrap(look_str.into(), mono(9.5), CREAM).size().x + 12.0).max(38.0);
                let look_vis = Rect::from_min_size(
                    egui::pos2(right_edge - look_w, top_strip.center().y - btn_h * 0.5),
                    egui::vec2(look_w, btn_h),
                );
                right_edge = look_vis.left() - 5.0;
                let look_a11y = if is_sel { "Look".to_string() } else { format!("emitter {} look", i + 1) };
                let look_resp = hit(ui, look_vis, ui.id().with(("em-look", i)), Sense::click_and_drag(), &look_a11y);
                p.rect_filled(look_vis, r(4), Color32::from_black_alpha(if look_resp.hovered() { 225 } else { 180 }));
                p.rect_stroke(look_vis, r(4), hair(WELL_LINE2), StrokeKind::Inside);
                p.text(
                    look_vis.center(),
                    Align2::CENTER_CENTER,
                    look_str,
                    mono_medium(9.2),
                    if map.look > 0 { AMBER_MID } else { CREAM2 },
                );
                if look_resp.dragged() {
                    self.sel_emitter = i;
                    let nv = (macros[3] - look_resp.drag_delta().y / 190.0).clamp(0.0, 0.999);
                    macros[3] = nv;
                } else if look_resp.clicked() {
                    self.sel_emitter = i;
                    let next_idx = (map.look + 1) % LOOKS.len();
                    macros[3] = (next_idx as f32 + 0.5) / LOOKS.len() as f32;
                }
            }

            // Mini ADSR + Cutout badge + clean clip title in top strip.
            if cw >= 290.0 {
                let env_r = Rect::from_min_size(egui::pos2(right_edge - 28.0, top_strip.center().y - 5.0), egui::vec2(26.0, 10.0));
                mini_adsr(&p, env_r, &adsr, if is_sel { 0.92 } else { 0.68 });
                right_edge = env_r.left() - 5.0;
            }
            let mut tx = badge_c.x + badge_r + 6.0;
            if has_alpha && tx + 18.0 < right_edge - 24.0 {
                let cb = cutout_badge(&p, egui::pos2(tx, top_strip.center().y - 7.5));
                tx = cb.right() + 4.0;
            }
            let max_chars = ((right_edge - tx) / 5.8).max(3.0) as usize;
            p.text(
                egui::pos2(tx, top_strip.center().y),
                Align2::LEFT_CENTER,
                short(&clip_name, max_chars.min(16)),
                font_medium(if cw < 245.0 { 9.8 } else { 10.8 }),
                if is_sel { CREAM } else { CREAM2 },
            );

            // --- Bottom Hardware Deck: 4 Real Charcoal Pointer Knobs + Silver LCDs (`Grain`, `Pitch`, `Smear`, `Level`) ---
            let deck = Rect::from_min_max(egui::pos2(card.left() + 4.0, top_strip.bottom() + 2.0), egui::pos2(card.right() - 4.0, card.bottom() - 3.0));
            let kw = deck.width() / 4.0;
            let compact_deck = deck.height() < 86.0 || kw < 64.0;
            let lcd_h = if compact_deck { 13.5 } else { 17.0 };
            let lbl_gap = if compact_deck { 9.5 } else { 14.0 };
            let krad = ((deck.height() - lcd_h - lbl_gap - 6.0) * 0.44).clamp(11.0, 22.0);
            let unit_h = krad * 2.0 + lbl_gap + lcd_h + if compact_deck { 2.0 } else { 5.0 };
            let ky = (deck.center().y - unit_h * 0.5 + krad).max(deck.top() + krad + 1.5);

            let prev_macros = macros;
            let prev_level = level;
            let k_names: [(&str, String, f32); 4] = [
                ("GRAIN", if is_sel { "Grain".into() } else { format!("emitter {} grain", i + 1) }, 0.18),
                ("PITCH", if is_sel { "Pitch".into() } else { format!("emitter {} pitch", i + 1) }, 0.50),
                ("SMEAR", if is_sel { "Smear".into() } else { format!("emitter {} smear", i + 1) }, 0.25),
                ("LEVEL", if is_sel { "Level".into() } else { format!("emitter {} level", i + 1) }, 0.80),
            ];

            for (k, (lbl, a11y, def)) in k_names.iter().enumerate() {
                let cx = deck.left() + (k as f32 + 0.5) * kw;
                let vref = match k {
                    0 => &mut macros[0],
                    1 => &mut macros[1],
                    2 => &mut macros[2],
                    _ => &mut level,
                };
                let kresp = knob(ui, egui::pos2(cx, ky), krad, ui.id().with(("em-knob", i, k)), a11y, vref, *def, 0);
                if kresp.dragged() || kresp.clicked() {
                    self.sel_emitter = i;
                }
                caps(
                    &p,
                    egui::pos2(cx, ky + krad + if compact_deck { 4.8 } else { 7.2 }),
                    Align2::CENTER_CENTER,
                    lbl,
                    if compact_deck { 7.0 } else { 8.0 },
                );
            }

            let map_after = MacroMap::resolve(&macros, Some(&key));
            let pitch_txt = format!("{:+.0} {}", map_after.pitch_st, super::note_name(key.root + map_after.pitch_st as i32)).replace("+0 ", "0 ");
            let lcd_vals = [
                format!("{:.0}", macros[0] * 100.0),
                pitch_txt,
                format!("{:.0}", macros[2] * 100.0),
                format!("{:.0}%", level * 100.0),
            ];
            for (k, vs) in lcd_vals.iter().enumerate() {
                let cx = deck.left() + (k as f32 + 0.5) * kw;
                let lcd_w = (kw - if compact_deck { 5.0 } else { 12.0 }).clamp(36.0, 68.0);
                let lcd_y = (ky + krad + lbl_gap + lcd_h * 0.5 + if compact_deck { 0.5 } else { 2.0 })
                    .min(deck.bottom() - lcd_h * 0.5 - 1.0);
                lcd_readout(&p, Rect::from_center_size(egui::pos2(cx, lcd_y), egui::vec2(lcd_w, lcd_h)), vs, if compact_deck { 8.5 } else { 9.5 });
            }

            if macros != prev_macros || (level - prev_level).abs() > 1e-5 {
                self.emitters[i].macros = macros;
                self.emitters[i].level = level;
                for m_i in 0..4 {
                    if (macros[m_i] - prev_macros[m_i]).abs() > 1e-5 {
                        host.set_shared_macro(m_i, macros[m_i]);
                    }
                }
            }
        }

        if let Some(em_i) = open_clip_em {
            self.open_clip(ClipTarget::Emitter(em_i));
        }
    }
}

pub fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
