//! Play: the Tempera model translated to video. An 8×8 grid where each
//! column is a clip laid out as 8 frame tiles; four emitters (brushes)
//! spray grains where they sit; the canvas shows the result; the brush
//! panel holds the selected emitter's four macros and Rec.

use crate::atlas;
use crate::theme::*;
use crate::widgets::*;
use super::{DeepTab, DeepTarget, EMITTERS, ROWS};
use super::Studio;
use crate::host::Host;
use chromagrain_core::instrument::MacroMap;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

const KNOBS: [(&str, usize, f32); 4] = [("Grain", 0, 0.0), ("Pitch", 1, 0.5), ("Smear", 2, 0.0), ("Look", 3, 0.0)];

impl Studio {
    pub fn play_view(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, area: Rect) {
        let small = area.width() < 1100.0;
        let gap = if small { 18.0 } else { 20.0 };
        let grid_w = ((area.width() - gap) / 2.0).floor();
        let grid = Rect::from_min_size(area.min, egui::vec2(grid_w, area.height()));
        let side = Rect::from_min_max(egui::pos2(grid.right() + gap, area.top()), area.max);
        self.play_grid(host, ui, grid);
        let stage_h = (side.width() * 9.0 / 16.0).min(side.height() * 0.5).floor();
        let stage = Rect::from_min_size(side.min, egui::vec2(side.width(), stage_h));
        self.stage(host, ui, stage, 14.0);
        let brush = Rect::from_min_max(egui::pos2(side.left(), stage.bottom() + gap), side.max);
        self.brush_panel(host, ui, brush);
        let e = &self.emitters[self.sel_emitter];
        let look = e.look_name();
        let tag_font = font(11.0);
        let tw = ui.painter().layout_no_wrap(look.into(), tag_font.clone(), INK2).size().x + 18.0;
        let tag = Rect::from_min_size(egui::pos2(stage.right() - tw - 12.0, stage.top() + 12.0), egui::vec2(tw, 22.0));
        ui.painter().rect_filled(tag, r(6), Color32::from_black_alpha(130));
        ui.painter().text(tag.center(), Align2::CENTER_CENTER, look, tag_font, INK2);
    }

    fn play_grid(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, grid: Rect) {
        let ctx = ui.ctx().clone();
        let g = 4.0;
        let cw = (grid.width() - g * 7.0) / 8.0;
        let ch = (grid.height() - g * (ROWS as f32 - 1.0)) / ROWS as f32;
        let cell = |c: usize, r: usize| {
            Rect::from_min_size(egui::pos2(grid.left() + c as f32 * (cw + g), grid.top() + r as f32 * (ch + g)), egui::vec2(cw, ch))
        };
        let p = ui.painter().clone();
        let now = host.clock();
        let sel_col = self.emitters[self.sel_emitter].col;
        for c in 0..super::COLUMNS {
            let src = self.columns[c];
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
                                host.import_media();
                            }
                        }
                    }
                }
            }
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
        let pick = |st: &super::Studio, pos: egui::Pos2, taken: &[usize]| -> usize {
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

        // Multi-touch: every finger plays its own emitter (egui also
        // turns the first finger into pointer events — those are ignored
        // while fingers are down).
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
                    let i = pick(&self, pos, &taken);
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
                let grabbing = ui.data(|d| d.get_temp::<Option<usize>>(resp.id)).flatten();
                let who = match grabbing {
                    Some(i) => i,
                    None => {
                        let i = pick(&self, pos, &[]);
                        ui.data_mut(|d| d.insert_temp(resp.id, Some(i)));
                        i
                    }
                };
                self.sel_emitter = who;
                let (c, row) = to_grid(pos);
                let e = &mut self.emitters[who];
                e.col = c;
                e.row = row;
                e.held = !resp.clicked() || resp.is_pointer_button_down_on();
            }
        }
        if !resp.is_pointer_button_down_on() || fingers {
            // Release: only the emitter this pointer was playing stops.
            if let Some(i) = ui.data(|d| d.get_temp::<Option<usize>>(resp.id)).flatten() {
                if !self.touches.iter().any(|(_, e)| *e == i) {
                    self.emitters[i].held = false;
                }
            }
            ui.data_mut(|d| d.insert_temp(resp.id, None::<usize>));
        }

        for i in 0..EMITTERS {
            let e = &self.emitters[i];
            let active = e.held || now - e.last_fire < 0.3;
            emitter_ring(&p, ring_pos(e.col, e.row), if grid.width() < 600.0 { 13.0 } else { 15.0 }, i + 1, i == self.sel_emitter, active, now);
        }
        if self.emitters.iter().any(|e| e.held) {
            ctx.request_repaint();
        }
    }

    fn brush_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let small = rect.height() < 320.0;
        p.rect_filled(rect, r(14), G1);
        p.rect_stroke(rect, r(14), hair(LINE), StrokeKind::Inside);
        let pad = if small { 10.0 } else { 14.0 };
        let inner = rect.shrink(pad);

        let th = if small { 48.0 } else { 64.0 };
        let fh = if small { 44.0 } else { 50.0 };
        let foot = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - fh), inner.max);
        // Knobs first, so the tiles below show what they just set.
        let ka = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + th), egui::pos2(inner.right(), foot.top()));
        let colw = ka.width() / 4.0;
        let label_h = if small { 34.0 } else { 46.0 };
        let rad = ((ka.height() - label_h - 16.0) / 2.0).min(colw * 0.3).clamp(22.0, 52.0);
        let cy = ka.top() + (ka.height() - label_h) / 2.0 + 4.0;
        let key = self.key;
        let sel = self.sel_emitter;
        for (k, (name, mi, default)) in KNOBS.iter().enumerate() {
            let c = egui::pos2(ka.left() + colw * (k as f32 + 0.5), cy);
            let mut v = self.emitters[sel].macros[*mi];
            let detents = if *mi == 3 { chromagrain_core::instrument::LOOKS.len() } else { 0 };
            let before = v;
            knob(ui, c, rad, ui.id().with(("knob", k)), name, &mut v, *default, detents);
            self.emitters[sel].macros[*mi] = v;
            if v != before {
                host.set_shared_macro(*mi, v);
            }
            let map = MacroMap::resolve(&self.emitters[sel].macros, Some(&key));
            let value = match mi {
                0 => format!("{:.0}", v * 100.0),
                1 => format!("{:+.0} {}", map.pitch_st, super::note_name(key.root + map.pitch_st as i32)).replace("+0 ", "0 "),
                2 => format!("{:.0}", v * 100.0),
                _ => super::look_name(map.look).to_string(),
            };
            let ly = c.y + rad + if small { 14.0 } else { 22.0 };
            caps(&p, egui::pos2(c.x, ly), Align2::CENTER_CENTER, name, 10.5);
            p.text(egui::pos2(c.x, ly + if small { 15.0 } else { 20.0 }), Align2::CENTER_CENTER, value, mono(13.0), INK);
        }
        // Emitter tiles.
        let tg = 8.0;
        let tw = (inner.width() - tg * 3.0) / 4.0;
        for i in 0..EMITTERS {
            let tr = Rect::from_min_size(egui::pos2(inner.left() + i as f32 * (tw + tg), inner.top()), egui::vec2(tw, th));
            let (col, pos, look) = {
                let e = &self.emitters[i];
                (e.col, e.pos(), e.look_name())
            };
            let src = self.columns[col];
            let sel = i == self.sel_emitter;
            if let Some((tex, a)) = src.and_then(|s| self.atlas(host, &ctx, s)) {
                tile(&p, tr, 10, tex, a.uv_cover(atlas::frame_at(pos), tw / th), gray(if sel { 0.8 } else { 0.5 }));
            } else {
                p.rect_filled(tr, r(10), G2);
            }
            let resp = hit(ui, tr, ui.id().with(("emitter-tile", i)), Sense::click(), &format!("emitter {}", i + 1));
            let h = hold(ui, &resp, 0.45);
            if h.fired {
                self.sel_emitter = i;
                self.deep = Some((DeepTarget::Emitter(i), DeepTab::Links));
            } else if resp.clicked() && !h.held_before {
                self.sel_emitter = i;
            }
            // A soft bottom scrim keeps the label readable on bright footage.
            let scrim = Rect::from_min_max(egui::pos2(tr.left(), tr.bottom() - 26.0), tr.max);
            p.rect_filled(scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(90));
            if sel {
                p.rect_stroke(tr, r(10), Stroke::new(2.0, HI), StrokeKind::Inside);
            } else {
                p.rect_stroke(tr, r(10), hair(LINE2), StrokeKind::Inside);
            }
            let rc = egui::pos2(tr.left() + 21.0, tr.center().y + if small { 0.0 } else { 6.0 });
            p.circle_filled(rc, 10.0, Color32::from_black_alpha(90));
            p.circle_stroke(rc, 10.0, Stroke::new(1.2, if sel { HI } else { INK2 }));
            p.text(rc, Align2::CENTER_CENTER, (i + 1).to_string(), mono(11.0), if sel { HI } else { INK2 });
            let label = if look == "clean" { src.map(|s| self.source_name(host, s)).unwrap_or_default() } else { look.to_string() };
            let room = ((tw - 48.0) / 6.2) as usize;
            if room >= 4 {
                p.text(egui::pos2(tr.right() - 10.0, tr.bottom() - 9.0), Align2::RIGHT_BOTTOM, short(&label, room.min(16)), font(11.0), INK2);
            }
        }

        // Foot: Rec + more.
        let more = Rect::from_min_max(egui::pos2(foot.right() - fh, foot.top()), foot.max);
        let recr = Rect::from_min_max(foot.min, egui::pos2(more.left() - 8.0, foot.bottom()));
        let recording = self.rec.is_some();
        if !host.caps().rec {
            // No takes here (a plugin): the foot loads clips instead.
            if chip(ui, recr, ui.id().with("load-clip"), "load a clip", false, false).clicked() {
                host.import_media();
            }
            draw_icon(&p, Icon::Plus, egui::pos2(recr.left() + 22.0, recr.center().y), 14.0, INK2);
            p.text(egui::pos2(recr.left() + 38.0, recr.center().y), Align2::LEFT_CENTER, "Load clip", font_medium(13.0), INK);
            if let Some(b) = host.busy() {
                p.text(egui::pos2(recr.right() - 16.0, recr.center().y), Align2::RIGHT_CENTER, short(&b, 32), font(11.0), INK3);
            }
        } else {
            let recording = self.rec.is_some();
            if chip(ui, recr, ui.id().with("rec-big"), "rec", false, false).clicked() {
                self.toggle_rec(host);
            }
            let rc = egui::pos2(recr.left() + 22.0, recr.center().y);
            if recording {
                p.rect_filled(recr, r(10), Color32::from_rgba_unmultiplied(229, 72, 77, 26));
                p.circle_filled(rc, 6.0, REC);
            } else {
                p.circle_stroke(rc, 6.0, Stroke::new(1.5, INK2));
            }
            p.text(egui::pos2(rc.x + 16.0, rc.y), Align2::LEFT_CENTER, if recording { "Stop" } else { "Rec" }, font_medium(13.0), INK);
            let secs = self.rec.as_ref().map(|r| host.clock() - r.t0).unwrap_or(0.0);
            p.text(
                egui::pos2(recr.right() - 16.0, rc.y),
                Align2::RIGHT_CENTER,
                format!("{}:{:02}", (secs / 60.0) as u32, secs as u32 % 60),
                mono(12.0),
                if recording { INK } else { INK3 },
            );
        }
        if chip(ui, more, ui.id().with("more"), "emitter settings", false, false).clicked() {
            self.deep = Some((DeepTarget::Emitter(self.sel_emitter), DeepTab::Links));
        }
        draw_icon(&p, Icon::More, more.center(), 18.0, INK2);
        if recording {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

    }
}

pub fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}
