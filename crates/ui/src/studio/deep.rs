//! The calm `Project` modal sheet (`D · Hybrid`), replacing the old right-hand
//! `Deep` sidebar drawer. Includes `30 fps | 60 fps` export rate, `Time & key`,
//! `Set · scenes` (12 named scenes + cue chain), `Library`, `Master effects ♪ ▣`
//! (including `Kaleido`), and `Script`.

use super::add::modal;
use super::{ClipTarget, Sheet, Studio, View, EMITTERS, LANES, SCENES};
use crate::host::{AddRoute, AddTarget, Host, StageMode};
use crate::studio::play::short;
use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::music::{Key, ScaleKind};
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepTarget {
    Global,
    Emitter(usize),
    Lane(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepTab {
    Links,
    Effects,
    Automation,
    Script,
    Library,
}

impl DeepTab {
    pub const ALL: [DeepTab; 5] = [
        DeepTab::Links,
        DeepTab::Effects,
        DeepTab::Automation,
        DeepTab::Script,
        DeepTab::Library,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DeepTab::Links => "Links",
            DeepTab::Effects => "Effects",
            DeepTab::Automation => "Automation",
            DeepTab::Script => "Script",
            DeepTab::Library => "Library",
        }
    }
}

pub const FX_SLOTS: [(&str, &str); 8] = [
    ("crush", "Crush"),
    ("delay", "Echo"),
    ("reverb", "Reverb"),
    ("kaleido", "Kaleido"),
    ("compress", "Codec"),
    ("tint", "Tint"),
    ("stretch", "Stretch"),
    ("lowpass", "Filter"),
];

pub fn view_back_name(v: View) -> &'static str {
    match v {
        View::Play => "Play",
        View::Sequence => "Sequence",
        View::Perform => "Perform",
    }
}

/// `< Play` / `< Sequence` / `< Perform` back pill (accessibility name `"close sheet"`).
pub fn sheet_back_button(ui: &mut egui::Ui, pos: egui::Pos2, view: View, id: egui::Id) -> (Rect, bool) {
    let vname = view_back_name(view);
    let p = ui.painter().clone();
    let tw = p.layout_no_wrap(vname.into(), font_medium(12.5), INK).size().x;
    let w = (tw + 34.0).max(76.0);
    let rect = Rect::from_min_size(pos, egui::vec2(w, 36.0));
    let resp = hit(ui, rect, id.with("back"), Sense::click(), "close sheet");
    let hov = resp.hovered() || resp.is_pointer_button_down_on();
    raised(&p, rect, 10, if hov { G3 } else { G2 });
    draw_icon(&p, Icon::ChevronLeft, egui::pos2(rect.left() + 14.0, rect.center().y), 13.0, if hov { HI } else { INK2 });
    p.text(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        vname,
        font_medium(12.5),
        if hov { HI } else { INK },
    );
    (rect, resp.clicked())
}

/// Paint a calm dark section card inside a sheet and return its content rect.
pub fn sheet_card(p: &egui::Painter, rect: Rect, title: &str) -> Rect {
    p.rect_filled(rect, r(12), Color32::from_rgb(0x10, 0x10, 0x12));
    p.rect_stroke(rect, r(12), hair(LINE), StrokeKind::Inside);
    caps(p, egui::pos2(rect.left() + 14.0, rect.top() + 14.0), Align2::LEFT_CENTER, title, 10.0);
    Rect::from_min_max(
        egui::pos2(rect.left() + 14.0, rect.top() + 27.0),
        egui::pos2(rect.right() - 14.0, rect.bottom() - 10.0),
    )
}

pub fn fx_kind_matches(kind: &EffectKind, slot_key: &str) -> bool {
    match slot_key {
        "crush" => matches!(kind, EffectKind::Crush { .. }),
        "delay" => matches!(kind, EffectKind::Delay { .. }),
        "reverb" => matches!(kind, EffectKind::Reverb { .. }),
        "compress" => matches!(kind, EffectKind::Compress { .. }),
        "tint" => matches!(kind, EffectKind::Tint { .. }),
        "kaleido" => kind.name() == "kaleido" || matches!(kind, EffectKind::Tile { mirror, .. } if *mirror >= 0.5),
        "stretch" => matches!(kind, EffectKind::Tile { mirror, .. } if *mirror < 0.5),
        "highpass" | "tune" => matches!(kind, EffectKind::Tune { .. }),
        _ => false,
    }
}

pub fn fx_default_kind(slot_key: &str, key: Key) -> Option<EffectKind> {
    match slot_key {
        "crush" => EffectKind::parse("crush"),
        "delay" => EffectKind::parse("delay"),
        "reverb" => EffectKind::parse("reverb"),
        "compress" => EffectKind::parse("compress"),
        "tint" => EffectKind::parse("tint"),
        "kaleido" => EffectKind::parse("kaleido"),
        "stretch" => EffectKind::parse("tile"),
        "highpass" | "tune" => Some(EffectKind::Tune {
            root: key.root as f32,
            scale: key.scale,
            amount: 0.8,
            focus: 0.5,
        }),
        _ => None,
    }
}

pub fn get_chain_fx(chain: &[AvEffect], slot_key: &str, lowpass_age: f32) -> (f32, f32) {
    if slot_key == "lowpass" {
        let v = lowpass_age.clamp(0.0, 1.0);
        return (v, v * 0.7);
    }
    chain
        .iter()
        .find(|e| fx_kind_matches(&e.kind, slot_key))
        .map(|e| (e.audio.clamp(0.0, 1.0), e.video.clamp(0.0, 1.0)))
        .unwrap_or((0.0, 0.0))
}

pub fn set_chain_fx(chain: &mut Vec<AvEffect>, slot_key: &str, key: Key, audio: f32, video: f32, lowpass_age: &mut f32) {
    let a = audio.clamp(0.0, 1.0);
    let v = video.clamp(0.0, 1.0);
    if slot_key == "lowpass" {
        *lowpass_age = a.max(v);
        return;
    }
    if a + v < 0.01 {
        chain.retain(|e| !fx_kind_matches(&e.kind, slot_key));
        return;
    }
    if let Some(existing) = chain.iter_mut().find(|e| fx_kind_matches(&e.kind, slot_key)) {
        existing.audio = a;
        existing.video = v;
    } else if let Some(kind) = fx_default_kind(slot_key, key) {
        chain.push(AvEffect { kind, audio: a, video: v });
    }
}

impl Studio {
    /// First active owner of `src`, falling back to `ClipTarget::Source(src)`.
    pub fn preferred_clip_target(&mut self, host: &mut dyn Host, src: usize) -> ClipTarget {
        if let Some(em) = (0..EMITTERS).find(|&i| self.columns.get(self.emitters[i].col).copied().flatten() == Some(src)) {
            return ClipTarget::Emitter(em);
        }
        if let Some(col) = (0..super::COLUMNS).find(|&c| self.columns[c] == Some(src)) {
            return ClipTarget::Column(col);
        }
        if let Some(l) = (0..LANES).find(|&l| self.seq.lanes[l] == Some(src)) {
            return ClipTarget::Lane(l);
        }
        if let Some(perf) = host.perform() {
            if let Some(pi) = perf.pads().iter().position(|pd| pd.source == Some(src)) {
                return ClipTarget::Pad(pi);
            }
        }
        ClipTarget::Source(src)
    }

    pub fn project_sheet(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        if self.sheet != Some(Sheet::Project) {
            return;
        }
        let screen = ctx.screen_rect();
        let size = egui::vec2((screen.width() - 36.0).min(1220.0), (screen.height() - 40.0).min(780.0));
        let caps_cfg = host.caps();
        let mut close = false;
        let mut next_sheet: Option<Sheet> = None;
        let mut open_add_route: Option<AddRoute> = None;

        let scrim = modal(ctx, "cg-project", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(20.0, 16.0));
            let id = egui::Id::new("cg-project");
            let p = ui.painter().clone();

            // --- Header ---
            let (back_r, clicked_back) = sheet_back_button(ui, egui::pos2(inner.left(), inner.top() + 2.0), self.view, id);
            if clicked_back {
                close = true;
            }
            let (n_clips, cur_fps) = {
                let pp = host.project().lock().unwrap();
                (pp.sources.len(), pp.fps)
            };
            let n_scenes = self.seq.scene_pats.iter().filter(|s| s.is_some()).count().max(1);
            let sub = if caps_cfg.scenes {
                format!("{n_clips} clips · {n_scenes} scenes · {:.0} fps", cur_fps)
            } else {
                format!("{n_clips} clips")
            };
            let tx = back_r.right() + 14.0;
            p.text(egui::pos2(tx, inner.top() + 11.0), Align2::LEFT_CENTER, "Project", font_semibold(18.0), INK);
            p.text(egui::pos2(tx, inner.top() + 30.0), Align2::LEFT_CENTER, &sub, mono(11.5), INK3);

            if caps_cfg.export {
                let exp_r = Rect::from_min_size(egui::pos2(inner.right() - 96.0, inner.top() + 2.0), egui::vec2(96.0, 36.0));
                if button_named(ui, exp_r, id.with("export"), "Export", "project export", true, Some(Icon::Export), None).clicked() {
                    host.export();
                }
                // `30 fps | 60 fps` export rate selector right beside Export!
                let fps_r = Rect::from_min_size(egui::pos2(exp_r.left() - 118.0, inner.top() + 2.0), egui::vec2(110.0, 36.0));
                let is_60 = cur_fps >= 50.0;
                if let Some(pick) = segmented_named(
                    ui,
                    fps_r,
                    id.with("export-fps"),
                    &["30 fps", "60 fps"],
                    &["fps 30", "fps 60"],
                    &[],
                    usize::from(is_60),
                    mono(11.0),
                    false,
                ) {
                    host.project().lock().unwrap().fps = if pick == 1 { 60.0 } else { 30.0 };
                    host.mark();
                }
            }

            // --- Scrollable Grid of Cards ---
            let body = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + 50.0), inner.max);
            let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
            egui::ScrollArea::vertical()
                .id_salt("cg-project-scroll")
                .auto_shrink([false, false])
                .show(&mut body_ui, |ui| {
                    let avail_w = (body.width() - 4.0).max(320.0);
                    let wide = avail_w >= 960.0;
                    let gap = 12.0;

                    let n_sources = host.project().lock().unwrap().sources.len();
                    let has_script = caps_cfg.deep_tabs.contains(&DeepTab::Script);
                    let has_dig = caps_cfg.add.contains(&AddRoute::Dig);
                    let total_lib_tiles = n_sources + 1 + usize::from(has_dig);

                    if wide {
                        let left_w = (avail_w * 0.40).floor();
                        let right_w = avail_w - left_w - gap;
                        let row1_h = 196.0;
                        let lib_cols = ((avail_w * 0.63 - 28.0) / 106.0).floor().max(4.0) as usize;
                        let lib_rows = total_lib_tiles.div_ceil(lib_cols).max(1);
                        let lib_h = 44.0 + lib_rows as f32 * 84.0;
                        let script_h = if has_script { 216.0 } else { 0.0 };
                        let fx_h = 374.0;
                        let row2_h = (lib_h + if has_script { gap + script_h } else { 0.0 }).max(fx_h);
                        let total_h = row1_h + gap + row2_h + 4.0;

                        let (canvas, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), Sense::hover());
                        if caps_cfg.scenes {
                            let c_time = Rect::from_min_size(canvas.min, egui::vec2(left_w, row1_h));
                            let c_song = Rect::from_min_size(egui::pos2(c_time.right() + gap, canvas.top()), egui::vec2(right_w, row1_h));
                            self.project_time_card(host, ui, c_time, &caps_cfg);
                            self.project_song_card(host, ui, c_song);
                        } else {
                            let c_time = Rect::from_min_size(canvas.min, egui::vec2(avail_w, row1_h));
                            self.project_time_card(host, ui, c_time, &caps_cfg);
                        }

                        let r2_top = canvas.top() + row1_h + gap;
                        let r2_left_w = (avail_w * 0.63).floor();
                        let r2_right_w = avail_w - r2_left_w - gap;
                        let c_lib = Rect::from_min_size(egui::pos2(canvas.left(), r2_top), egui::vec2(r2_left_w, lib_h));
                        self.project_library_card(host, ui, c_lib, has_dig, &mut next_sheet, &mut open_add_route);

                        if has_script {
                            let c_script = Rect::from_min_size(
                                egui::pos2(canvas.left(), c_lib.bottom() + gap),
                                egui::vec2(r2_left_w, script_h),
                            );
                            self.project_script_card(host, ui, c_script);
                        }

                        let c_fx = Rect::from_min_size(
                            egui::pos2(c_lib.right() + gap, r2_top),
                            egui::vec2(r2_right_w, row2_h.max(fx_h)),
                        );
                        self.project_fx_card(host, ui, c_fx, 1);
                    } else {
                        let time_h = 192.0;
                        let song_h = if caps_cfg.scenes { 176.0 } else { 0.0 };
                        let lib_cols = ((avail_w - 28.0) / 112.0).floor().max(4.0) as usize;
                        let lib_rows = total_lib_tiles.div_ceil(lib_cols).max(1);
                        let lib_h = 44.0 + lib_rows as f32 * 84.0;
                        let fx_h = 204.0;
                        let script_h = if has_script { 216.0 } else { 0.0 };
                        let mut total_h = time_h + gap + lib_h + gap + fx_h + 6.0;
                        if caps_cfg.scenes {
                            total_h += song_h + gap;
                        }
                        if has_script {
                            total_h += script_h + gap;
                        }
                        let (canvas, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), Sense::hover());
                        let mut y = canvas.top();

                        let c_time = Rect::from_min_size(egui::pos2(canvas.left(), y), egui::vec2(avail_w, time_h));
                        self.project_time_card(host, ui, c_time, &caps_cfg);
                        y = c_time.bottom() + gap;

                        if caps_cfg.scenes {
                            let c_song = Rect::from_min_size(egui::pos2(canvas.left(), y), egui::vec2(avail_w, song_h));
                            self.project_song_card(host, ui, c_song);
                            y = c_song.bottom() + gap;
                        }

                        let c_lib = Rect::from_min_size(egui::pos2(canvas.left(), y), egui::vec2(avail_w, lib_h));
                        self.project_library_card(host, ui, c_lib, has_dig, &mut next_sheet, &mut open_add_route);
                        y = c_lib.bottom() + gap;

                        let c_fx = Rect::from_min_size(egui::pos2(canvas.left(), y), egui::vec2(avail_w, fx_h));
                        self.project_fx_card(host, ui, c_fx, 2);
                        y = c_fx.bottom() + gap;

                        if has_script {
                            let c_script = Rect::from_min_size(egui::pos2(canvas.left(), y), egui::vec2(avail_w, script_h));
                            self.project_script_card(host, ui, c_script);
                        }
                    }
                });
        });

        if let Some(route) = open_add_route {
            self.add.tab = route;
            self.open_add(host, AddTarget::Any);
        } else if let Some(sh) = next_sheet {
            if let Sheet::Clip(ct) = sh {
                self.open_clip(ct);
            } else {
                self.sheet = Some(sh);
            }
        } else if (close || scrim) && self.sheet == Some(Sheet::Project) {
            self.sheet = None;
        }
    }

    fn project_time_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, caps_cfg: &crate::host::Caps) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Time & key");
        let id = ui.id().with("proj-time");

        let top_h = (inner.height() - 42.0).max(86.0);
        let top_r = Rect::from_min_size(inner.min, egui::vec2(inner.width().min(420.0), top_h));
        let col_w = top_r.width() / 4.0;
        let ky = top_r.top() + 34.0;
        let krad = 24.0;

        // 1. Tempo knob.
        let mut bpm = host.project().lock().unwrap().bpm as f32;
        let tc = egui::pos2(top_r.left() + col_w * 0.5, ky);
        if caps_cfg.tempo {
            let mut tv = ((bpm - 60.0) / 140.0).clamp(0.0, 1.0);
            let before = tv;
            knob(ui, tc, krad, id.with("bpm"), "project tempo", &mut tv, (112.0 - 60.0) / 140.0, 0);
            if (tv - before).abs() > 1e-5 {
                bpm = (60.0 + tv * 140.0).round();
                host.project().lock().unwrap().bpm = bpm as f64;
                host.mark();
            }
        } else {
            p.circle_filled(tc, krad * 0.78, G2);
            p.circle_stroke(tc, krad * 0.78, hair(LINE2));
            p.text(tc, Align2::CENTER_CENTER, format!("{bpm:.0}"), mono_medium(13.0), INK);
        }
        caps(&p, egui::pos2(tc.x, ky + krad + 14.0), Align2::CENTER_CENTER, "Tempo", 9.5);
        p.text(egui::pos2(tc.x, ky + krad + 28.0), Align2::CENTER_CENTER, format!("{bpm:.0}"), mono(11.5), INK);

        // 2. Key tile.
        let kc = egui::pos2(top_r.left() + col_w * 1.5, ky + 8.0);
        let kr = Rect::from_center_size(kc, egui::vec2((col_w - 10.0).clamp(56.0, 74.0), 66.0));
        let kresp = hit(ui, kr, id.with("key"), Sense::click_and_drag(), "project key");
        raised(&p, kr, 10, if kresp.hovered() { G3 } else { G2 });
        let key = self.key;
        p.text(kc - egui::vec2(0.0, 8.0), Align2::CENTER_CENTER, super::note_name(key.root), font_semibold(15.0), INK);
        p.text(kc + egui::vec2(0.0, 11.0), Align2::CENTER_CENTER, super::scale_label(key.scale), font(10.5), INK3);
        let scales = [
            ScaleKind::Minor,
            ScaleKind::Major,
            ScaleKind::MinorPentatonic,
            ScaleKind::MajorPentatonic,
            ScaleKind::Dorian,
            ScaleKind::Blues,
        ];
        if kresp.clicked() {
            let shift = ui.input(|inp| inp.modifiers.shift);
            if shift {
                let si = scales.iter().position(|s| *s == key.scale).unwrap_or(0);
                self.key = Key::new(key.root, scales[(si + 1) % scales.len()]);
            } else {
                self.key = Key::new((key.root + 1) % 12, key.scale);
            }
            self.key_changed(host);
        } else if kresp.dragged() && kresp.drag_delta().y.abs() > 0.0 {
            let d = ui.data(|dd| dd.get_temp::<f32>(kresp.id).unwrap_or(0.0)) - kresp.drag_delta().y;
            if d.abs() > 16.0 {
                let si = scales.iter().position(|s| *s == key.scale).unwrap_or(0) as i32 + d.signum() as i32;
                self.key = Key::new(key.root, scales[si.rem_euclid(scales.len() as i32) as usize]);
                self.key_changed(host);
                ui.data_mut(|dd| dd.insert_temp(kresp.id, 0.0f32));
            } else {
                ui.data_mut(|dd| dd.insert_temp(kresp.id, d));
            }
        }

        // 3. Swing knob.
        let (scene, _) = self.seq_current(host);
        let pid = self.seq_pattern(scene);
        let mut swing_pct = {
            let pp = host.project().lock().unwrap();
            pid.and_then(|i| pp.patterns.get(i))
                .map(|pat| ((pat.swing - 0.5) * 2.0).clamp(0.0, 0.5) * 2.0)
                .unwrap_or(0.0)
        };
        let sc = egui::pos2(top_r.left() + col_w * 2.5, ky);
        let before_sw = swing_pct;
        knob(ui, sc, krad, id.with("swing"), "project swing", &mut swing_pct, 0.0, 0);
        if (swing_pct - before_sw).abs() > 1e-4 {
            let new_swing = 0.5 + swing_pct * 0.25;
            let mut pp = host.project().lock().unwrap();
            for sp in self.seq.scene_pats.iter().flatten() {
                if let Some(pat) = pp.patterns.get_mut(*sp) {
                    pat.swing = new_swing;
                }
            }
            drop(pp);
            host.mark();
        }
        caps(&p, egui::pos2(sc.x, ky + krad + 14.0), Align2::CENTER_CENTER, "Swing", 9.5);
        p.text(
            egui::pos2(sc.x, ky + krad + 28.0),
            Align2::CENTER_CENTER,
            format!("{:.0}%", swing_pct * 50.0),
            mono(11.5),
            INK,
        );

        // 4. Master Level knob.
        let mut lvl = {
            let db = host.project().lock().unwrap().master_gain_db;
            ((db + 24.0) / 30.0).clamp(0.0, 1.0)
        };
        let lc = egui::pos2(top_r.left() + col_w * 3.5, ky);
        let before_lv = lvl;
        knob(ui, lc, krad, id.with("level"), "project level", &mut lvl, 0.8, 0);
        if (lvl - before_lv).abs() > 1e-4 {
            host.project().lock().unwrap().master_gain_db = -24.0 + lvl * 30.0;
            host.mark();
        }
        caps(&p, egui::pos2(lc.x, ky + krad + 14.0), Align2::CENTER_CENTER, "Level", 9.5);
        p.text(
            egui::pos2(lc.x, ky + krad + 28.0),
            Align2::CENTER_CENTER,
            format!("{:.0}%", lvl * 100.0),
            mono(11.5),
            INK,
        );

        // Bottom row: `stack | split | grid | bleed` + `now | beat | bar`.
        let bot_y = inner.bottom() - 34.0;
        let mut bx = inner.left();
        if let Some(cur_mode) = host.stage_mode() {
            let lay_w = 186.0f32.min((inner.width() * 0.54).max(150.0));
            let lay_r = Rect::from_min_size(egui::pos2(bx, bot_y), egui::vec2(lay_w, 34.0));
            if let Some(pick) = segmented_named(
                ui,
                lay_r,
                id.with("layout"),
                &["stack", "split", "grid", "bleed"],
                &["project layout stack", "project layout split", "project layout tile", "project layout bleed"],
                &[],
                cur_mode.index(),
                font(11.5),
                false,
            ) {
                host.set_stage_mode(StageMode::from_index(pick));
            }
            bx = lay_r.right() + 8.0;
        }
        let cur_q = host.perform().map(|pf| pf.quantize());
        if let Some(q) = cur_q {
            let q_w = (inner.right() - bx).min(156.0);
            if q_w >= 108.0 {
                let qr = Rect::from_min_size(egui::pos2(bx, bot_y), egui::vec2(q_w, 34.0));
                if let Some(nq) = segmented_named(
                    ui,
                    qr,
                    id.with("quant"),
                    &["now", "beat", "bar"],
                    &["project launch now", "project launch beat", "project launch bar"],
                    &[],
                    q,
                    font(11.5),
                    false,
                ) {
                    if let Some(pf) = host.perform() {
                        pf.set_quantize(nq);
                    }
                }
            }
        }
    }

    fn project_song_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Set · scenes");
        let id = ui.id().with("proj-song");
        let (cur_scene, _) = self.seq_current(host);

        // Top row: Set / Cue Arranger Bar.
        let set_r = Rect::from_min_size(inner.min, egui::vec2(inner.width(), 36.0));
        self.set_arranger_bar(host, ui, set_r, "proj-set");

        // Bottom 2 rows: Scenes 1..12 (`6×2`).
        let grid_top = set_r.bottom() + 10.0;
        let grid_h = (inner.bottom() - grid_top).max(68.0);
        let sg = 6.0;
        let cols = 6usize;
        let rows = 2usize;
        let sw = (inner.width() - sg * (cols as f32 - 1.0)) / cols as f32;
        let sh = (grid_h - sg) / rows as f32;
        for s in 0..SCENES {
            let sr = Rect::from_min_size(
                egui::pos2(inner.left() + (s % cols) as f32 * (sw + sg), grid_top + (s / cols) as f32 * (sh + sg)),
                egui::vec2(sw, sh),
            );
            let exists = self.seq_pattern(s).is_some();
            let on = s == cur_scene;
            let resp = hit(ui, sr, id.with(("proj-scn", s)), Sense::click(), &format!("project scene {}", s + 1));
            if on {
                p.rect_filled(sr, r(8), HI);
            } else if exists {
                raised(&p, sr, 8, if resp.hovered() { G3 } else { G2 });
            } else {
                p.rect_filled(sr, r(8), G1);
                p.rect_stroke(sr, r(8), hair(LINE), StrokeKind::Inside);
            }
            let fg = if on { ON_HI } else if exists { INK } else { INK4 };
            let lbl = self.scene_display_label(s, 7);
            p.text(sr.center(), Align2::CENTER_CENTER, lbl, mono(11.5), fg);
            if resp.clicked() {
                self.seq_scene_tap(host, s);
            }
        }
    }

    fn project_library_card(
        &mut self,
        host: &mut dyn Host,
        ui: &mut egui::Ui,
        card: Rect,
        has_dig: bool,
        next_sheet: &mut Option<Sheet>,
        open_add_route: &mut Option<AddRoute>,
    ) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Library");
        let id = ui.id().with("proj-lib");
        let names: Vec<String> = host.project().lock().unwrap().sources.iter().map(|s| s.name.clone()).collect();
        let cols = (inner.width() / 106.0).floor().max(4.0) as usize;
        let g = 8.0;
        let tw = (inner.width() - g * (cols as f32 - 1.0)) / cols as f32;
        let th = (tw * 9.0 / 16.0).min(56.0);
        let row_step = th + 24.0;
        let mut picked_src = None;

        for (i, raw_name) in names.iter().enumerate() {
            let pos = egui::pos2(
                inner.left() + (i % cols) as f32 * (tw + g),
                inner.top() + (i / cols) as f32 * row_step,
            );
            let thumb_r = Rect::from_min_size(pos, egui::vec2(tw, th));
            let hit_r = Rect::from_min_size(pos, egui::vec2(tw, th + 20.0));
            if let Some((tex, a)) = self.atlas(host, &ctx, i) {
                tile(&p, thumb_r, 8, tex, a.uv_cover(5, tw / th), gray(0.9));
            } else {
                p.rect_filled(thumb_r, r(8), G2);
            }
            let clean = super::strip_media_ext(raw_name);
            let resp = hit(ui, hit_r, id.with(("src", i)), Sense::click(), &format!("library {clean}"));
            p.rect_stroke(thumb_r, r(8), if resp.hovered() { Stroke::new(1.5, HI) } else { hair(LINE2) }, StrokeKind::Inside);
            let room = (tw / 6.2).max(4.0) as usize;
            p.text(
                egui::pos2(thumb_r.left() + 2.0, thumb_r.bottom() + 4.0),
                Align2::LEFT_TOP,
                short(clean, room),
                font(11.0),
                if resp.hovered() { HI } else { INK },
            );
            if resp.clicked() {
                picked_src = Some(i);
            }
        }
        if let Some(src) = picked_src {
            let ct = self.preferred_clip_target(host, src);
            *next_sheet = Some(Sheet::Clip(ct));
        }

        let k_add = names.len();
        let add_pos = egui::pos2(
            inner.left() + (k_add % cols) as f32 * (tw + g),
            inner.top() + (k_add / cols) as f32 * row_step,
        );
        let add_r = Rect::from_min_size(add_pos, egui::vec2(tw, th));
        let add_resp = hit(ui, add_r, id.with("lib-add"), Sense::click(), "library add");
        p.rect_filled(add_r, r(8), if add_resp.hovered() { G3 } else { G2 });
        p.rect_stroke(add_r, r(8), hair(LINE2), StrokeKind::Inside);
        p.text(add_r.center() - egui::vec2(0.0, 7.0), Align2::CENTER_CENTER, "+", font_semibold(16.0), INK);
        p.text(add_r.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "add", font(10.5), INK3);
        if add_resp.clicked() {
            *open_add_route = Some(AddRoute::Link);
        }

        if has_dig {
            let k_dig = k_add + 1;
            let dig_pos = egui::pos2(
                inner.left() + (k_dig % cols) as f32 * (tw + g),
                inner.top() + (k_dig / cols) as f32 * row_step,
            );
            let dig_r = Rect::from_min_size(dig_pos, egui::vec2(tw, th));
            let dig_resp = hit(ui, dig_r, id.with("lib-dig"), Sense::click(), "library dig");
            p.rect_filled(dig_r, r(8), if dig_resp.hovered() { G3 } else { G2 });
            p.rect_stroke(dig_r, r(8), hair(LINE2), StrokeKind::Inside);
            p.text(dig_r.center() - egui::vec2(0.0, 7.0), Align2::CENTER_CENTER, "Dig", font_semibold(13.5), INK);
            p.text(dig_r.center() + egui::vec2(0.0, 10.0), Align2::CENTER_CENTER, "unseen", font(10.5), INK3);
            if dig_resp.clicked() {
                *open_add_route = Some(AddRoute::Dig);
            }
        }
    }

    fn project_fx_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect, cols: usize) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Master effects  ♪ ▣");
        let id = ui.id().with("proj-fx");
        let cols = cols.max(1);
        let rows = FX_SLOTS.len().div_ceil(cols);
        let g = 6.0;
        let pw = (inner.width() - g * (cols as f32 - 1.0)) / cols as f32;
        let ph = ((inner.height() - g * (rows as f32 - 1.0)) / rows as f32).clamp(30.0, 38.0);
        let key = self.key;

        let (mut chain, mut lp_age) = {
            let pp = host.project().lock().unwrap();
            (pp.master_effects.clone(), pp.master_tape.age)
        };
        let mut any_changed = false;
        for (idx, &(slot_key, label)) in FX_SLOTS.iter().enumerate() {
            let r_i = idx / cols;
            let c_i = idx % cols;
            let pr = Rect::from_min_size(
                egui::pos2(inner.left() + c_i as f32 * (pw + g), inner.top() + r_i as f32 * (ph + g)),
                egui::vec2(pw, ph),
            );
            let (mut a, mut v) = get_chain_fx(&chain, slot_key, lp_age);
            if dual_pill(
                ui,
                pr,
                id.with(("mfx", slot_key)),
                label,
                &format!("master fx {}", label.to_lowercase()),
                &mut a,
                &mut v,
            ) {
                set_chain_fx(&mut chain, slot_key, key, a, v, &mut lp_age);
                any_changed = true;
            }
        }
        if any_changed {
            {
                let mut pp = host.project().lock().unwrap();
                pp.master_effects = chain;
                pp.master_tape.age = lp_age;
            }
            host.mark();
        }
    }

    fn project_script_card(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, card: Rect) {
        let p = ui.painter().clone();
        let inner = sheet_card(&p, card, "Script");
        let code_box = inner.shrink2(egui::vec2(2.0, 2.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(code_box));
        host.deep_tab(DeepTab::Script, DeepTarget::Global, &mut child);
    }
}
