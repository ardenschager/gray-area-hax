//! The one Deep drawer: everything that isn't playing. Opens from the top
//! bar, an emitter's `⋯` / long-press, or a lane header long-press. Its
//! Effects / Automation / Script / Library tabs host the classic editors,
//! so every existing feature stays reachable.

use crate::theme::*;
use crate::widgets::*;
use super::Studio;
use crate::host::Host;
use chromagrain_core::music::{Key, ScaleKind};
use egui::{self, Align2, Color32, Rect, Sense};

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
    pub const ALL: [DeepTab; 5] = [DeepTab::Links, DeepTab::Effects, DeepTab::Automation, DeepTab::Script, DeepTab::Library];
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

/// One drawer row: label, 2 px slider, mono value.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut egui::Ui, id: egui::Id, label: (&str, &str), value: &mut f32, lo: f32, hi: f32, shown: &str) -> bool {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 44.0), Sense::hover());
    let p = ui.painter().clone();
    let y = rect.center().y;
    runs(
        &p,
        egui::pos2(rect.left(), y),
        Align2::LEFT_CENTER,
        &[(label.0, font_medium(13.0), INK), (label.1, font(13.0), INK2)],
    );
    let sl = Rect::from_min_max(egui::pos2(rect.left() + w * 0.48, y - 1.0), egui::pos2(rect.right() - 62.0, y + 1.0));
    let changed = slider(ui, sl, id, &format!("{}{}", label.0, label.1), value, lo, hi);
    p.text(egui::pos2(rect.right(), y), Align2::RIGHT_CENTER, shown, mono(12.0), INK);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], hair(LINE));
    changed
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::hover());
    caps(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, text, 10.5);
    ui.add_space(2.0);
}

impl Studio {
    pub fn deep_drawer(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let open = self.deep.is_some();
        let t = ctx.animate_bool_with_time(egui::Id::new("cg-deep-open"), open, 0.24);
        if t <= 0.001 {
            return;
        }
        let Some((target, tab)) = self.deep.or(Some((DeepTarget::Global, DeepTab::Links))) else { return };
        let screen = ctx.screen_rect();
        let wide = matches!(tab, DeepTab::Automation | DeepTab::Library);
        let w = if wide { (screen.width() * 0.72).max(420.0) } else { 420.0f32 }.min(screen.width() * 0.94);
        // Scrim: tap to close.
        egui::Area::new(egui::Id::new("cg-deep-scrim")).fixed_pos(screen.min).order(egui::Order::Middle).show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha((140.0 * t) as u8));
            if resp.clicked() {
                self.deep = None;
            }
        });
        let x = screen.right() - w * t;
        egui::Area::new(egui::Id::new("cg-deep")).fixed_pos(egui::pos2(x, screen.top())).order(egui::Order::Foreground).show(ctx, |ui| {
            let rect = Rect::from_min_size(egui::pos2(x, screen.top()), egui::vec2(w, screen.height()));
            ui.painter().rect_filled(rect, 0.0, G1);
            ui.painter().line_segment([rect.left_top(), rect.left_bottom()], hair(LINE2));
            let inner = rect.shrink2(egui::vec2(18.0, 0.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            self.deep_contents(host, &mut child, target, tab, inner);
        });
    }

    fn deep_contents(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, target: DeepTarget, tab: DeepTab, inner: Rect) {
        let p = ui.painter().clone();
        let (title, sub) = match target {
            DeepTarget::Global => ("Deep".to_string(), "everything that isn't playing"),
            DeepTarget::Emitter(i) => (format!("Emitter {}", i + 1), "everything this brush can do"),
            DeepTarget::Lane(i) => (format!("Lane {}", i + 1), "length, clip and sound of this lane"),
        };
        p.text(egui::pos2(inner.left(), inner.top() + 30.0), Align2::LEFT_CENTER, title, font_semibold(18.0), INK);
        p.text(egui::pos2(inner.left(), inner.top() + 52.0), Align2::LEFT_CENTER, sub, font(12.0), INK3);
        let done = Rect::from_min_size(egui::pos2(inner.right() - 64.0, inner.top() + 22.0), egui::vec2(64.0, 34.0));
        if chip(ui, done, ui.id().with("deep-done"), "Done", true, false).clicked() {
            self.deep = None;
            return;
        }
        p.text(done.center(), Align2::CENTER_CENTER, "Done", font(13.0), INK2);
        let tabs = Rect::from_min_size(egui::pos2(inner.left(), inner.top() + 76.0), egui::vec2(inner.width(), 42.0));
        let mut avail = host.caps().deep_tabs;
        if !avail.contains(&DeepTab::Links) {
            avail.insert(0, DeepTab::Links);
        }
        // The global page's first tab holds song settings, not links.
        let labels: Vec<&str> = avail.iter().map(|t| if *t == DeepTab::Links && target == DeepTarget::Global { "Song" } else { t.label() }).collect();
        let cur = avail.iter().position(|t| *t == tab).unwrap_or(0);
        let tab = avail[cur];
        if let Some(i) = segmented(ui, tabs, ui.id().with("deep-tabs"), &labels, &[], cur, font(12.5), false) {
            self.deep = Some((target, avail[i]));
        }
        let body = Rect::from_min_max(egui::pos2(inner.left(), tabs.bottom() + 12.0), egui::pos2(inner.right(), inner.bottom() - 12.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt(("deep-scroll", cur)).auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.set_width(body.width() - 4.0);
            match tab {
                DeepTab::Links => self.deep_links(host, ui, target),
                other => {
                    if let (DeepTab::Effects, DeepTarget::Emitter(i)) = (other, target) {
                        section(ui, &format!("Emitter {} (from its knobs)", i + 1));
                        let map = chromagrain_core::instrument::MacroMap::resolve(&self.emitters[i].macros, Some(&self.key));
                        let fx = super::emitter_effects(&map, &self.key);
                        let names: Vec<&str> = fx.iter().map(|f| f.kind.name()).collect();
                        ui.label(egui::RichText::new(if names.is_empty() { "clean".into() } else { names.join(" → ") }).color(INK2));
                    }
                    let heading = match other {
                        DeepTab::Effects => "Master — sound and picture",
                        DeepTab::Automation => "Arrangement — clips, automation, takes",
                        DeepTab::Library => "Sources — video, YouTube, instruments",
                        _ => "",
                    };
                    if !heading.is_empty() {
                        section(ui, heading);
                    }
                    host.deep_tab(other, target, ui);
                }
            }
        });
    }

    fn deep_links(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, target: DeepTarget) {
        match target {
            DeepTarget::Emitter(i) => {
                let w = ui.available_width();
                let (er, _) = ui.allocate_exact_size(egui::vec2(w, 170.0), Sense::hover());
                let src = self.columns[self.emitters[i].col];
                self.envelope_panel(host, ui, er, super::clip::EnvTarget::Emitter(i), src);
                section(ui, &format!("Emitter {}", i + 1));
                let id = ui.id().with(("em", i));
                let e = &mut self.emitters[i];
                let c = &mut e.cloud;
                let v = c.density;
                row(ui, id.with("dens"), ("Density", ""), &mut c.density, 2.0, 80.0, &format!("{v:.0}/s"));
                let v = c.duration;
                row(ui, id.with("dur"), ("Grain length", ""), &mut c.duration, 0.01, 0.4, &format!("{:.0} ms", v * 1000.0));
                let v = c.spray;
                row(ui, id.with("spray"), ("Spray", ""), &mut c.spray, 0.0, 2.0, &format!("{v:.2}"));
                let v = c.pitch_jitter;
                row(ui, id.with("pj"), ("Pitch jitter", ""), &mut c.pitch_jitter, 0.0, 12.0, &format!("{v:.1}"));
                let v = c.reverse;
                row(ui, id.with("rev"), ("Reverse", ""), &mut c.reverse, 0.0, 1.0, &format!("{:.0}%", v * 100.0));
                let v = c.pan_spread;
                row(ui, id.with("pan"), ("Pan spread", ""), &mut c.pan_spread, 0.0, 1.0, &format!("{v:.2}"));
                section(ui, "Sound ↔ picture links");
                let l = &mut e.link;
                let v = l.pitch_to_hue;
                row(ui, id.with("l1"), ("pitch", " → hue"), &mut l.pitch_to_hue, 0.0, 30.0, &format!("{v:.1}"));
                let v = l.gain_to_opacity;
                row(ui, id.with("l2"), ("gain", " → opacity"), &mut l.gain_to_opacity, 0.0, 1.0, &format!("{v:.2}"));
                let v = l.pan_to_x;
                row(ui, id.with("l3"), ("pan", " → x position"), &mut l.pan_to_x, 0.0, 1.0, &format!("{v:.2}"));
                let v = l.pitch_to_rate;
                row(ui, id.with("l4"), ("position", " → frame rate"), &mut l.pitch_to_rate, 0.0, 1.0, &format!("{v:.2}"));
                let mut rv = if l.reverse_video { 1.0 } else { 0.0 };
                let shown = if l.reverse_video { "on" } else { "off" };
                if row(ui, id.with("l5"), ("reverse", " → reverse"), &mut rv, 0.0, 1.0, shown) {
                    l.reverse_video = rv >= 0.5;
                }
                let v = l.envelope_to_opacity;
                row(ui, id.with("l6"), ("envelope", " → fade"), &mut l.envelope_to_opacity, 0.0, 1.0, &format!("{v:.2}"));
            }
            DeepTarget::Lane(lane) => {
                section(ui, &format!("Lane {}", lane + 1));
                let (scene, _) = self.seq_current(host);
                let len = {
                    let pid = self.seq_pattern(scene);
                    let pp = host.project().lock().unwrap();
                    pid.and_then(|pid| pp.patterns.get(pid)).and_then(|pat| pat.rows.get(lane)).map(|r| r.steps.len()).unwrap_or(16)
                };
                let mut lv = len as f32;
                if row(ui, ui.id().with(("lane-len", lane)), ("Length", ""), &mut lv, 1.0, 16.0, &format!("{len} steps")) {
                    self.set_lane_len(host, lane, lv.round() as usize);
                }
                section(ui, "Clip");
                let names: Vec<String> = host.project().lock().unwrap().sources.iter().map(|s| s.name.clone()).collect();
                let cur = self.seq.lanes[lane];
                ui.horizontal_wrapped(|ui| {
                    for (si, n) in names.iter().enumerate() {
                        if ui.selectable_label(cur == Some(si), n).clicked() {
                            self.seq.lanes[lane] = Some(si);
                        }
                    }
                });
                section(ui, "Sound of every hit");
                if let Some(pid) = self.seq_pattern(scene) {
                    let mut g = {
                        let pp = host.project().lock().unwrap();
                        pp.patterns.get(pid).and_then(|pat| pat.rows.get(lane)).map(|r| r.grains.clone())
                    };
                    if let Some(gs) = g.as_mut() {
                        let mut changed = false;
                        let v = gs.duration;
                        changed |= row(ui, ui.id().with("ld"), ("Length", ""), &mut gs.duration, 0.02, 1.5, &format!("{:.0} ms", v * 1000.0));
                        let v = gs.gain;
                        changed |= row(ui, ui.id().with("lg"), ("Gain", ""), &mut gs.gain, 0.0, 2.0, &format!("{v:.2}"));
                        let v = gs.pan;
                        changed |= row(ui, ui.id().with("lp"), ("Pan", ""), &mut gs.pan, -1.0, 1.0, &format!("{v:+.2}"));
                        let v = gs.envelope;
                        changed |= row(ui, ui.id().with("le"), ("Envelope", ""), &mut gs.envelope, 0.05, 1.0, &format!("{v:.2}"));
                        let v = gs.texture;
                        changed |= row(ui, ui.id().with("lt"), ("Sample ↔ grains", ""), &mut gs.texture, 0.0, 1.0, &format!("{v:.2}"));
                        if changed {
                            let mut pp = host.project().lock().unwrap();
                            for pid in self.seq.scene_pats.iter().flatten() {
                                if let Some(r) = pp.patterns.get_mut(*pid).and_then(|pat| pat.rows.get_mut(lane)) {
                                    r.grains = gs.clone();
                                }
                            }
                            drop(pp);
                            host.mark();
                        }
                    }
                }
            }
            DeepTarget::Global => {
                section(ui, "Song");
                let mut bpm = host.project().lock().unwrap().bpm as f32;
                let shown = format!("{bpm:.0} bpm");
                if row(ui, ui.id().with("bpm"), ("Tempo", ""), &mut bpm, 40.0, 220.0, &shown) {
                    host.project().lock().unwrap().bpm = bpm.round() as f64;
                    host.mark();
                }
                section(ui, "Key");
                let key = self.key;
                let mut nk = key;
                ui.horizontal_wrapped(|ui| {
                    for pc in 0..12 {
                        if ui.selectable_label(key.root == pc, super::note_name(pc)).clicked() {
                            nk = Key::new(pc, key.scale);
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for s in [ScaleKind::Minor, ScaleKind::Major, ScaleKind::MinorPentatonic, ScaleKind::MajorPentatonic, ScaleKind::Dorian, ScaleKind::Phrygian, ScaleKind::Lydian, ScaleKind::Mixolydian, ScaleKind::Blues, ScaleKind::HarmonicMinor, ScaleKind::WholeTone, ScaleKind::Chromatic] {
                        if ui.selectable_label(key.scale == s, super::scale_label(s)).clicked() {
                            nk = Key::new(key.root, s);
                        }
                    }
                });
                if nk != key {
                    self.key = nk;
                    self.key_changed(host);
                }
            }
        }
    }
}
