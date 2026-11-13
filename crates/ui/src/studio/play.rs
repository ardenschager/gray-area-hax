//! Play: the Tempera model translated to video (`D · Hybrid: "touch it · open it"`).
//! An 8×8 grid where each column is a clip laid out as 8 frame tiles; four
//! emitters spray grains where they sit; the stage shows the result; the
//! brush panel holds the contextual object bar, the 2×2 grid of on-object
//! emitter cards (grain ring + cloud dots + pitch dot + smear trail + mini
//! ADSR + look chip + `Clip ›`), and the full-width Rec bar.

use super::{ClipTarget, Studio, COLUMNS, EMITTERS, ROWS};
use crate::atlas;
use crate::host::Host;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::instrument::{MacroMap, LOOKS};
use egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind};

impl Studio {
    pub fn play_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let small = area.width() < 1100.0;
        let gap = if small { 14.0 } else { 20.0 };
        let grid_w = ((area.width() - gap) / 2.0).floor();
        let grid = Rect::from_min_size(area.min, egui::vec2(grid_w, area.height()));
        let side = Rect::from_min_max(egui::pos2(grid.right() + gap, area.top()), area.max);
        self.play_grid(host, ui, grid);
        let stage_h = (side.width() * 9.0 / 16.0).min(side.height() * if small { 0.47 } else { 0.5 }).floor();
        let stage = Rect::from_min_size(side.min, egui::vec2(side.width(), stage_h));
        self.stage(host, ui, stage, 14.0);
        self.layout_switch(host, ui, stage);
        let brush = Rect::from_min_max(egui::pos2(side.left(), stage.bottom() + if small { 12.0 } else { 16.0 }), side.max);
        self.brush_panel(host, ui, brush, small);
    }

    fn play_grid(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let ctx = ui.ctx().clone();
        let g = 4.0;
        let head_h = 30.0;
        let grid = Rect::from_min_max(egui::pos2(area.left(), area.top() + head_h + 6.0), area.max);
        let cw = (grid.width() - g * 7.0) / 8.0;
        let ch = (grid.height() - g * (ROWS as f32 - 1.0)) / ROWS as f32;
        let cell = |c: usize, r: usize| {
            Rect::from_min_size(egui::pos2(grid.left() + c as f32 * (cw + g), grid.top() + r as f32 * (ch + g)), egui::vec2(cw, ch))
        };
        let p = ui.painter().clone();
        let now = host.clock();
        let sel_col = self.emitters[self.sel_emitter].col;
        let pending = self.pending_slots(host);
        let t_ui = ui.input(|i| i.time);
        let mut open_add = None;
        let mut open_clip = None;
        for c in 0..COLUMNS {
            let src = self.columns[c];
            let head = Rect::from_min_size(egui::pos2(area.left() + c as f32 * (cw + g), area.top()), egui::vec2(cw, head_h));
            let hresp = hit(ui, head, ui.id().with(("col-head", c)), Sense::click(), &format!("column {}", c + 1));
            let hovered = hresp.hovered();
            p.rect_filled(head, r(8), if src.is_some() && c == sel_col { G2 } else if hovered { G2 } else { G1 });
            p.rect_stroke(head, r(8), hair(if c == sel_col && src.is_some() { LINE2 } else { LINE }), StrokeKind::Inside);
            match src {
                Some(s) => {
                    let h = hold(ui, &hresp, 0.45);
                    let n_slices = self.slices(host, s).len();
                    let raw = self.source_name(host, s);
                    let clean = super::strip_media_ext(&raw);
                    let right_pad = if n_slices > 0 { 26.0 } else { 16.0 };
                    let room = ((cw - 10.0 - right_pad) / 6.2).max(3.0) as usize;
                    let col = if c == sel_col { INK } else { INK2 };
                    let name_galley = p.layout_no_wrap(short(clean, room), font(11.0), col);
                    let nx = (head.center().x - (name_galley.size().x + 8.0) * 0.5).max(head.left() + 6.0);
                    p.galley(egui::pos2(nx, head.center().y - name_galley.size().y * 0.5), name_galley.clone(), col);
                    draw_icon(
                        &p,
                        Icon::ChevronRight,
                        egui::pos2((nx + name_galley.size().x + 6.0).min(head.right() - 7.0), head.center().y + 0.5),
                        10.0,
                        INK3,
                    );
                    if n_slices > 0 {
                        draw_icon(&p, Icon::Chop, egui::pos2(head.right() - 8.0, head.center().y), 11.0, INK3);
                    }
                    if h.fired {
                        open_clip = Some(c);
                    } else if hresp.clicked() && !h.held_before {
                        let on_chev = hresp.interact_pointer_pos().is_some_and(|pt| pt.x >= head.right() - 20.0);
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
                    draw_icon(&p, Icon::Plus, head.center(), 13.0, if hovered { INK2 } else { INK3 });
                }
            }
            // An add still arriving for this column: one tall placeholder.
            if src.is_none() {
                if let Some((pd, _)) = pending.iter().find(|(_, t)| *t == crate::host::AddTarget::Column(c)) {
                    let col_rect = Rect::from_min_max(cell(c, 0).min, cell(c, ROWS - 1).max);
                    super::add::placeholder_tile(&p, col_rect, 6, pd, t_ui, true);
                    ctx.request_repaint();
                    continue;
                }
            }
            let at = src.and_then(|s| self.atlas(host, &ctx, s));
            for row in 0..ROWS {
                let rc = cell(c, row);
                match at {
                    Some((tex, a)) => {
                        let lit = self
                            .lit
                            .iter()
                            .filter(|(k, _)| *k == (c, row))
                            .map(|(_, t)| (1.0 - (now - t) / 0.4).max(0.0) as f32)
                            .fold(0.0f32, f32::max);
                        let base = if c == sel_col { 0.86 } else { 0.7 };
                        let v = base + (1.0 - base) * lit;
                        tile(&p, rc, 6, tex, a.uv_cover(row * 2 + 1, cw / ch), gray(v));
                        if lit > 0.0 {
                            p.rect_filled(rc, r(6), Color32::from_white_alpha((lit * 26.0) as u8));
                        }
                    }
                    None => {
                        p.rect_filled(rc, r(6), G1);
                        p.rect_stroke(rc, r(6), hair(LINE), StrokeKind::Inside);
                        if row == 0 {
                            let resp = hit(ui, rc, ui.id().with(("add-col", c)), Sense::click(), "add a clip");
                            draw_icon(&p, Icon::Plus, rc.center(), 16.0, if resp.hovered() { INK2 } else { INK3 });
                            if resp.clicked() {
                                open_add = Some(c);
                            }
                        }
                    }
                }
            }
        }

        if let Some(c) = open_add {
            self.open_add(host, crate::host::AddTarget::Column(c));
        }
        if let Some(c) = open_clip {
            self.open_clip(ClipTarget::Column(c));
        }

        // Touch / pointer: press = the selected emitter jumps there and
        // sprays while held; grab a ring to play that emitter instead.
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
                if grid.width() < 600.0 { 13.0 } else { 15.0 },
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
        let small = rect.height() < 320.0;
        p.rect_filled(rect, r(14), G1);
        p.rect_stroke(rect, r(14), hair(LINE), StrokeKind::Inside);
        let pad = if small { 8.0 } else { 12.0 };
        let inner = rect.shrink(pad);
        let t_ui = ui.input(|i| i.time);

        // 1. Contextual Object Bar at top.
        let bar_h = if small { 28.0 } else { 32.0 };
        let obj_bar = Rect::from_min_size(inner.min, egui::vec2(inner.width(), bar_h));
        let sel = self.sel_emitter.min(EMITTERS - 1);
        let sel_col = self.emitters[sel].col;
        let sel_src = self.columns.get(sel_col).copied().flatten();
        let raw_sel_name = sel_src.map(|s| self.source_name(host, s)).unwrap_or_else(|| "empty".into());
        let sel_clip_name = super::strip_media_ext(&raw_sel_name);
        let title = format!("Emitter {} · {}", sel + 1, short(sel_clip_name, 18));
        if !narrow_window && inner.width() >= 500.0 {
            runs(
                &p,
                egui::pos2(obj_bar.left() + 4.0, obj_bar.center().y),
                Align2::LEFT_CENTER,
                &[
                    (&title, font_medium(12.5), INK),
                    ("   touch to play & shape  ·  tap › to open clip", font(11.5), INK3),
                ],
            );
        } else {
            p.text(
                egui::pos2(obj_bar.left() + 4.0, obj_bar.center().y),
                Align2::LEFT_CENTER,
                &title,
                font_medium(12.5),
                INK,
            );
        }
        let open_pill_r = Rect::from_min_size(
            egui::pos2(obj_bar.right() - 96.0, obj_bar.center().y - 14.0),
            egui::vec2(96.0, 28.0),
        );
        let mut open_clip_em: Option<usize> = None;
        if open_clip_pill(ui, open_pill_r, ui.id().with("play-open-clip"), "open clip").clicked() {
            open_clip_em = Some(sel);
        }

        // 2. Footer Row (Rec / Load clip) at bottom.
        let fh = if small { 34.0 } else { 44.0 };
        let foot = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - fh), inner.max);

        // 3. 2×2 Grid of On-Object Emitter Cards in middle.
        let vgap = if small { 6.0 } else { 10.0 };
        let cards_rect = Rect::from_min_max(
            egui::pos2(inner.left(), obj_bar.bottom() + vgap),
            egui::pos2(inner.right(), foot.top() - vgap),
        );
        let cg = if small { 6.0 } else { 10.0 };
        let cw = ((cards_rect.width() - cg) * 0.5).max(40.0);
        let ch = ((cards_rect.height() - cg) * 0.5).max(36.0);
        let key = self.key;
        let now = host.clock();
        let sheet_open = self.sheet.is_some();

        for i in 0..EMITTERS {
            let card = Rect::from_min_size(
                egui::pos2(
                    cards_rect.left() + (i % 2) as f32 * (cw + cg),
                    cards_rect.top() + (i / 2) as f32 * (ch + cg),
                ),
                egui::vec2(cw, ch),
            );
            let is_sel = i == self.sel_emitter;
            let (col, pos, macros, adsr, level, held, last_fire) = {
                let e = &self.emitters[i];
                (e.col, e.pos(), e.macros, e.adsr, e.level, e.held, e.last_fire)
            };
            let src = self.columns.get(col).copied().flatten();
            let raw_name = src.map(|s| self.source_name(host, s)).unwrap_or_default();
            let clip_name = super::strip_media_ext(&raw_name);

            // Card footage backdrop + contrast scrims.
            if let Some((tex, a)) = src.and_then(|s| self.atlas(host, &ctx, s)) {
                let bright = if is_sel { 0.62 } else { 0.44 };
                tile(&p, card, 12, tex, a.uv_cover(atlas::frame_at(pos), cw / ch), gray(bright));
            } else {
                p.rect_filled(card, r(12), G2);
            }
            let top_scrim = Rect::from_min_size(card.min, egui::vec2(cw, 28.0_f32.min(ch * 0.45)));
            p.rect_filled(top_scrim, CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, Color32::from_black_alpha(88));
            let bot_scrim = Rect::from_min_max(egui::pos2(card.left(), (card.bottom() - 28.0).max(card.top())), card.max);
            p.rect_filled(bot_scrim, CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }, Color32::from_black_alpha(115));

            // On-object grain ring + cloud dots + smear trail + pitch dot.
            let map = MacroMap::resolve(&macros, Some(&key));
            let geo = draw_on_object_ring(&p, card, macros[0], macros[1], map.pitch_st, macros[2], small);

            // Bottom-left mini ADSR curve.
            let adsr_h = if small { 12.0 } else { 17.0 };
            let adsr_w = (cw * 0.33).clamp(34.0, 112.0);
            let adsr_r = Rect::from_min_size(
                egui::pos2(card.left() + 10.0, card.bottom() - adsr_h - if small { 6.0 } else { 9.0 }),
                egui::vec2(adsr_w, adsr_h),
            );
            mini_adsr(&p, adsr_r, &adsr, if is_sel { 0.95 } else { 0.72 });

            // Right-edge level bar.
            let bar_top = card.top() + if small { 28.0 } else { 34.0 };
            let bar_bot = card.bottom() - if small { 26.0 } else { 34.0 };
            if bar_bot > bar_top + 6.0 {
                let max_bh = bar_bot - bar_top;
                let bh = (max_bh * level.clamp(0.0, 1.0)).max(3.0);
                let lb = Rect::from_min_size(egui::pos2(card.right() - 5.5, bar_bot - bh), egui::vec2(2.0, bh));
                p.rect_filled(lb, r(1), Color32::from_white_alpha(125));
            }

            // Top-left numbered badge + clip name.
            let badge_c = egui::pos2(card.left() + 17.0, card.top() + if small { 15.0 } else { 17.0 });
            let badge_r = if small { 8.5 } else { 9.5 };
            if is_sel {
                p.circle_filled(badge_c, badge_r, HI);
                p.text(badge_c, Align2::CENTER_CENTER, (i + 1).to_string(), mono_medium(10.5), ON_HI);
            } else {
                p.circle_filled(badge_c, badge_r, Color32::from_black_alpha(120));
                p.circle_stroke(badge_c, badge_r, Stroke::new(1.2, INK2));
                p.text(badge_c, Align2::CENTER_CENTER, (i + 1).to_string(), mono(10.5), INK);
            }
            let max_name_chars = ((cw - 92.0) / 6.2).max(4.0) as usize;
            p.text(
                egui::pos2(badge_c.x + badge_r + 6.0, badge_c.y),
                Align2::LEFT_CENTER,
                short(clip_name, max_name_chars.min(18)),
                font_medium(if small { 11.0 } else { 11.5 }),
                if is_sel { INK } else { INK2 },
            );

            // Card border (selection + active halo).
            if held || now - last_fire < 0.25 {
                p.rect_stroke(card.shrink(1.0), r(11), Stroke::new(1.0, Color32::from_white_alpha(95)), StrokeKind::Inside);
            }
            if is_sel {
                p.rect_stroke(card, r(12), Stroke::new(2.0, HI), StrokeKind::Inside);
            } else {
                p.rect_stroke(card, r(12), hair(LINE2), StrokeKind::Inside);
            }

            // --- Interactive zones on Card `i` ---
            let base_resp = hit(
                ui,
                card,
                ui.id().with(("emitter-tile", i)),
                Sense::click_and_drag(),
                &format!("emitter {}", i + 1),
            );

            // Pitch dot zone (kept disjoint from `geo.center` so `"Grain"` owns `geo.center`).
            let mut pitch_c = geo.pitch_pos;
            let to_pitch = pitch_c - geo.center;
            if to_pitch.length() < 26.0 {
                let dir = if to_pitch.length() > 1e-3 { to_pitch.normalized() } else { egui::vec2(0.0, -1.0) };
                pitch_c = geo.center + dir * 26.0;
            }
            let pitch_label = if is_sel { "Pitch".to_string() } else { format!("emitter {} pitch", i + 1) };
            let pitch_resp = hit(
                ui,
                Rect::from_center_size(pitch_c, egui::vec2(24.0, 24.0)),
                ui.id().with(("em-pitch", i)),
                Sense::click_and_drag(),
                &pitch_label,
            );

            // Smear trail zone to the right of the ring.
            let smear_x = (geo.center.x + geo.rr.max(18.0) + 18.0).min(card.right() - 62.0).max(geo.center.x + 24.0);
            let smear_label = if is_sel { "Smear".to_string() } else { format!("emitter {} smear", i + 1) };
            let smear_resp = hit(
                ui,
                Rect::from_center_size(egui::pos2(smear_x, geo.center.y), egui::vec2(26.0, 26.0)),
                ui.id().with(("em-smear", i)),
                Sense::click_and_drag(),
                &smear_label,
            );

            // Central Grain ring drag zone (`"Grain"` on selected card).
            let grain_label = if is_sel { "Grain".to_string() } else { format!("emitter {} grain", i + 1) };
            let grain_resp = hit(
                ui,
                Rect::from_center_size(geo.center, egui::vec2(34.0, 34.0)),
                ui.id().with(("em-grain", i)),
                Sense::click_and_drag(),
                &grain_label,
            );

            // Top-right `Look` chip.
            let look_str = super::look_name(map.look);
            let look_w = (p.layout_no_wrap(look_str.into(), font(11.0), INK).size().x + 16.0).max(44.0);
            let look_vis = Rect::from_min_size(
                egui::pos2(card.right() - look_w - 7.0, card.top() + 6.0),
                egui::vec2(look_w, if small { 20.0 } else { 22.0 }),
            );
            let look_a11y = if is_sel { "Look".to_string() } else { format!("emitter {} look", i + 1) };
            let look_resp = hit(
                ui,
                look_vis.expand2(egui::vec2(2.0, 3.0)),
                ui.id().with(("em-look", i)),
                Sense::click_and_drag(),
                &look_a11y,
            );
            p.rect_filled(
                look_vis,
                r(7),
                Color32::from_black_alpha(if look_resp.hovered() { 195 } else { 150 }),
            );
            p.rect_stroke(look_vis, r(7), hair(LINE2), StrokeKind::Inside);
            p.text(
                look_vis.center(),
                Align2::CENTER_CENTER,
                look_str,
                mono(10.5),
                if map.look > 0 { INK } else { INK2 },
            );

            // Bottom-right `Clip ›` dark pill (`"emitter settings"` on selected card).
            let clip_vis = Rect::from_min_size(
                egui::pos2(card.right() - 54.0, card.bottom() - if small { 24.0 } else { 28.0 }),
                egui::vec2(47.0, if small { 19.0 } else { 22.0 }),
            );
            let clip_a11y = if is_sel {
                "emitter settings".to_string()
            } else {
                format!("clip emitter {}", i + 1)
            };
            let clip_resp = hit(
                ui,
                clip_vis.expand2(egui::vec2(3.0, 3.0)),
                ui.id().with(("em-clip", i)),
                Sense::click(),
                &clip_a11y,
            );
            let clip_hov = clip_resp.hovered() || clip_resp.is_pointer_button_down_on();
            p.rect_filled(clip_vis, r(8), Color32::from_black_alpha(if clip_hov { 215 } else { 175 }));
            p.rect_stroke(clip_vis, r(8), hair(if clip_hov { LINE3 } else { LINE2 }), StrokeKind::Inside);
            p.text(
                egui::pos2(clip_vis.left() + 9.0, clip_vis.center().y),
                Align2::LEFT_CENTER,
                "Clip",
                font_medium(11.0),
                if clip_hov { HI } else { INK },
            );
            draw_icon(
                &p,
                Icon::ChevronRight,
                egui::pos2(clip_vis.right() - 10.0, clip_vis.center().y + 0.5),
                10.5,
                if clip_hov { HI } else { INK2 },
            );

            // Apply interactions.
            if clip_resp.clicked() {
                self.sel_emitter = i;
                open_clip_em = Some(i);
            } else if look_resp.dragged() {
                self.sel_emitter = i;
                let nv = (self.emitters[i].macros[3] - look_resp.drag_delta().y / 190.0).clamp(0.0, 0.999);
                self.emitters[i].macros[3] = nv;
                host.set_shared_macro(3, nv);
                let nm = MacroMap::resolve(&self.emitters[i].macros, Some(&key));
                self.gesture_hud = Some((i, format!("Look  {}", super::look_name(nm.look)), t_ui + 0.65));
            } else if look_resp.clicked() {
                self.sel_emitter = i;
                let next_idx = (map.look + 1) % LOOKS.len();
                let nv = (next_idx as f32 + 0.5) / LOOKS.len() as f32;
                self.emitters[i].macros[3] = nv;
                host.set_shared_macro(3, nv);
                self.gesture_hud = Some((i, format!("Look  {}", super::look_name(next_idx)), t_ui + 0.65));
            } else if grain_resp.dragged() {
                self.sel_emitter = i;
                let nv = (self.emitters[i].macros[0] - grain_resp.drag_delta().y / 120.0).clamp(0.0, 1.0);
                self.emitters[i].macros[0] = nv;
                host.set_shared_macro(0, nv);
                self.gesture_hud = Some((i, format!("Grain  {:.0}", nv * 100.0), t_ui + 0.65));
            } else if pitch_resp.dragged() {
                self.sel_emitter = i;
                let d = -pitch_resp.drag_delta().y / 130.0 + pitch_resp.drag_delta().x / 180.0;
                let nv = (self.emitters[i].macros[1] + d).clamp(0.0, 1.0);
                self.emitters[i].macros[1] = nv;
                host.set_shared_macro(1, nv);
                let nm = MacroMap::resolve(&self.emitters[i].macros, Some(&key));
                let pst = format!("{:+.0} {}", nm.pitch_st, super::note_name(key.root + nm.pitch_st as i32)).replace("+0 ", "0 ");
                self.gesture_hud = Some((i, format!("Pitch  {pst}"), t_ui + 0.65));
            } else if smear_resp.dragged() {
                self.sel_emitter = i;
                let d = -smear_resp.drag_delta().y / 120.0 + smear_resp.drag_delta().x / 110.0;
                let nv = (self.emitters[i].macros[2] + d).clamp(0.0, 1.0);
                self.emitters[i].macros[2] = nv;
                host.set_shared_macro(2, nv);
                self.gesture_hud = Some((i, format!("Smear  {:.0}", nv * 100.0), t_ui + 0.65));
            } else if base_resp.dragged() {
                self.sel_emitter = i;
                let d = base_resp.drag_delta();
                if d.y.abs() >= d.x.abs() {
                    let nv = (self.emitters[i].macros[2] - d.y / 140.0).clamp(0.0, 1.0);
                    self.emitters[i].macros[2] = nv;
                    host.set_shared_macro(2, nv);
                    self.gesture_hud = Some((i, format!("Smear  {:.0}", nv * 100.0), t_ui + 0.65));
                } else {
                    let nl = (self.emitters[i].level + d.x / 180.0).clamp(0.0, 1.0);
                    self.emitters[i].level = nl;
                    self.gesture_hud = Some((i, format!("Level  {:.0}%", nl * 100.0), t_ui + 0.65));
                }
            } else {
                // Hold or tap on any non-button part of the card.
                let hb = hold(ui, &base_resp, 0.45);
                let hg = hold(ui, &grain_resp, 0.45);
                let hp = hold(ui, &pitch_resp, 0.45);
                let hs = hold(ui, &smear_resp, 0.45);
                if hb.fired || hg.fired || hp.fired || hs.fired {
                    self.sel_emitter = i;
                    open_clip_em = Some(i);
                } else if (base_resp.clicked() && !hb.held_before)
                    || (grain_resp.clicked() && !hg.held_before)
                    || (pitch_resp.clicked() && !hp.held_before)
                    || (smear_resp.clicked() && !hs.held_before)
                {
                    self.sel_emitter = i;
                }
            }

            // Wheel over card: Shift+scroll = Grain, plain scroll = Pitch.
            if !sheet_open {
                let ptr_in_card = ui.input(|inp| inp.pointer.hover_pos().is_some_and(|pt| card.contains(pt)));
                if ptr_in_card {
                    let (scroll_y, shift) = ui.input(|inp| (inp.smooth_scroll_delta.y, inp.modifiers.shift));
                    if scroll_y.abs() > 0.5 {
                        self.sel_emitter = i;
                        if shift {
                            let nv = (self.emitters[i].macros[0] + scroll_y.signum() * 0.04).clamp(0.0, 1.0);
                            self.emitters[i].macros[0] = nv;
                            host.set_shared_macro(0, nv);
                            self.gesture_hud = Some((i, format!("Grain  {:.0}", nv * 100.0), t_ui + 0.65));
                        } else {
                            let nv = (self.emitters[i].macros[1] + scroll_y.signum() * (1.0 / 24.0)).clamp(0.0, 1.0);
                            self.emitters[i].macros[1] = nv;
                            host.set_shared_macro(1, nv);
                            let nm = MacroMap::resolve(&self.emitters[i].macros, Some(&key));
                            let pst = format!("{:+.0} {}", nm.pitch_st, super::note_name(key.root + nm.pitch_st as i32)).replace("+0 ", "0 ");
                            self.gesture_hud = Some((i, format!("Pitch  {pst}"), t_ui + 0.65));
                        }
                    }
                }
            }

            // Floating dark HUD pill while shaping.
            if let Some((hud_i, ref txt, until)) = self.gesture_hud {
                if hud_i == i && t_ui < until {
                    let g = p.layout_no_wrap(txt.clone(), mono_medium(12.0), HI);
                    let pill = Rect::from_center_size(card.center(), g.size() + egui::vec2(20.0, 12.0));
                    p.rect_filled(pill, r(8), Color32::from_black_alpha(215));
                    p.rect_stroke(pill, r(8), hair(LINE3), StrokeKind::Inside);
                    p.galley(pill.center() - g.size() * 0.5, g, HI);
                    ctx.request_repaint_after(std::time::Duration::from_millis(80));
                }
            }
        }

        if let Some(em_i) = open_clip_em {
            self.open_clip(ClipTarget::Emitter(em_i));
        }

        // Full-width Footer: Rec (standalone) or Load clip (plugin).
        let recording = self.rec.is_some();
        if !host.caps().rec {
            if chip(ui, foot, ui.id().with("load-clip"), "load a clip", false, false).clicked() {
                host.import_media();
            }
            draw_icon(&p, Icon::Plus, egui::pos2(foot.left() + 20.0, foot.center().y), 14.0, INK2);
            p.text(egui::pos2(foot.left() + 36.0, foot.center().y), Align2::LEFT_CENTER, "Load clip", font_medium(12.5), INK);
            if let Some(b) = host.busy() {
                p.text(egui::pos2(foot.right() - 14.0, foot.center().y), Align2::RIGHT_CENTER, short(&b, 32), font(11.0), INK3);
            }
        } else {
            if chip(ui, foot, ui.id().with("rec-big"), "rec", false, false).clicked() {
                self.toggle_rec(host);
            }
            let rc = egui::pos2(foot.left() + 20.0, foot.center().y);
            if recording {
                p.rect_filled(foot, r(10), Color32::from_rgba_unmultiplied(229, 72, 77, 26));
                p.circle_filled(rc, 5.5, REC);
            } else {
                p.circle_stroke(rc, 5.5, Stroke::new(1.5, INK2));
            }
            p.text(
                egui::pos2(rc.x + 15.0, rc.y),
                Align2::LEFT_CENTER,
                if recording { "Stop" } else { "Rec" },
                font_medium(12.5),
                INK,
            );
            let secs = self.rec.as_ref().map(|r| host.clock() - r.t0).unwrap_or(0.0);
            p.text(
                egui::pos2(foot.right() - 16.0, rc.y),
                Align2::RIGHT_CENTER,
                format!("{}:{:02}", (secs / 60.0) as u32, secs as u32 % 60),
                mono(11.5),
                if recording { INK } else { INK3 },
            );
        }
        if recording {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
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
