//! The Add sheet: Files · Browse · Link · Dig, one modal reached from the
//! grid's "+", a pad's "+" and Sequence's "+". Adds that take time
//! (downloads, digs) stream in as placeholder tiles, here and where they
//! will land; what arrived shows as source cards (with provenance) that
//! offer Chop straight away.

use super::{ClipTarget, Sheet, Studio};
use crate::host::{AddRoute, AddTarget, Host, Pending};
use crate::theme::*;
use crate::widgets::*;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// Add-sheet state that outlives one frame.
pub struct AddState {
    pub tab: AddRoute,
    pub link: String,
    pub query: String,
    pub cc_only: bool,
    /// Sources that arrived while the sheet was open (newest last).
    pub recent: Vec<usize>,
}

impl Default for AddState {
    fn default() -> Self {
        AddState { tab: AddRoute::Files, link: String::new(), query: String::new(), cc_only: true, recent: Vec::new() }
    }
}

pub fn target_label(t: AddTarget) -> String {
    match t {
        AddTarget::Any => "to the grid".into(),
        AddTarget::Column(c) => format!("to column {}", c + 1),
        AddTarget::Pad(i) => format!("to pad {}", i + 1),
        AddTarget::Lane(l) => format!("to lane {}", l + 1),
    }
}

/// A tile still arriving: a slow sheen, what's happening, progress.
pub fn placeholder_tile(p: &egui::Painter, rect: Rect, radius: u8, pend: &Pending, t: f64, compact: bool) {
    p.rect_filled(rect, r(radius), G2);
    if pend.failed.is_none() {
        // A soft band sweeping left to right every 1.6 s.
        let phase = (t / 1.6).fract() as f32;
        let bw = rect.width() * 0.45;
        let x = rect.left() - bw + (rect.width() + bw) * phase;
        let band = Rect::from_min_max(egui::pos2(x.max(rect.left()), rect.top()), egui::pos2((x + bw).min(rect.right()), rect.bottom()));
        if band.width() > 0.0 {
            let mut mesh = egui::Mesh::default();
            let (c0, c1) = (Color32::from_white_alpha(0), Color32::from_white_alpha(14));
            let mid = x + bw * 0.5;
            for (a, b) in [(band.left(), mid.clamp(band.left(), band.right())), (mid.clamp(band.left(), band.right()), band.right())] {
                let i = mesh.vertices.len() as u32;
                let ca = if (a - mid).abs() < 0.5 { c1 } else { c0 };
                let cb = if (b - mid).abs() < 0.5 { c1 } else { c0 };
                mesh.colored_vertex(egui::pos2(a, band.top()), ca);
                mesh.colored_vertex(egui::pos2(b, band.top()), cb);
                mesh.colored_vertex(egui::pos2(b, band.bottom()), cb);
                mesh.colored_vertex(egui::pos2(a, band.bottom()), ca);
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            p.add(egui::Shape::mesh(mesh));
        }
    }
    p.rect_stroke(rect, r(radius), Stroke::new(1.0, if pend.failed.is_some() { REC.gamma_multiply(0.6) } else { LINE3 }), StrokeKind::Inside);
    let label = match &pend.failed {
        Some(e) => super::play::short(e, if compact { 14 } else { 40 }),
        None => super::play::short(&pend.label, if compact { 14 } else { 40 }),
    };
    let size = if compact { 10.0 } else { 11.5 };
    p.text(egui::pos2(rect.left() + 8.0, rect.bottom() - if compact { 10.0 } else { 14.0 }), Align2::LEFT_BOTTOM, label, font(size), if pend.failed.is_some() { REC } else { INK2 });
    if let (Some(f), None) = (pend.progress, &pend.failed) {
        let y = rect.bottom() - 4.0;
        let w = (rect.width() - 16.0).max(0.0);
        p.line_segment([egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.left() + 8.0 + w, y)], Stroke::new(2.0, G4));
        p.line_segment([egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.left() + 8.0 + w * f.clamp(0.0, 1.0), y)], Stroke::new(2.0, INK2));
    }
}

/// Scrim + centred sheet; returns the sheet rect and whether the scrim
/// was tapped.
pub fn modal(ctx: &egui::Context, id: &str, size: egui::Vec2, body: impl FnOnce(&mut egui::Ui, Rect)) -> bool {
    let screen = ctx.screen_rect();
    let mut closed = false;
    egui::Area::new(egui::Id::new((id, "scrim"))).fixed_pos(screen.min).order(egui::Order::Middle).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(150));
        closed = resp.clicked();
    });
    let sheet = Rect::from_center_size(screen.center(), size);
    egui::Area::new(egui::Id::new(id)).fixed_pos(sheet.min).order(egui::Order::Foreground).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(sheet.size(), Sense::click());
        let p = ui.painter().clone();
        p.add(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(160) }.as_shape(rect, r(18)));
        p.rect_filled(rect, r(18), G1);
        p.rect_stroke(rect, r(18), hair(LINE2), StrokeKind::Inside);
        body(ui, rect);
    });
    closed
}

/// The sheet's header: title, subtitle, Done. Returns true on Done.
pub fn sheet_header(ui: &mut egui::Ui, inner: Rect, title: &str, sub: &str, id: egui::Id) -> bool {
    let p = ui.painter().clone();
    p.text(egui::pos2(inner.left(), inner.top() + 16.0), Align2::LEFT_CENTER, title, font_semibold(18.0), INK);
    p.text(egui::pos2(inner.left(), inner.top() + 38.0), Align2::LEFT_CENTER, sub, font(12.0), INK3);
    let done = Rect::from_min_size(egui::pos2(inner.right() - 72.0, inner.top() + 4.0), egui::vec2(72.0, 40.0));
    let c = chip(ui, done, id.with("done"), "close sheet", true, false).clicked();
    p.text(done.center(), Align2::CENTER_CENTER, "Done", font(13.0), INK2);
    c
}

/// A text field in the studio's look. Returns (response, enter pressed).
pub fn field(ui: &mut egui::Ui, rect: Rect, id: egui::Id, text: &mut String, hint: &str, label: &str) -> (egui::Response, bool) {
    ui.painter().rect_filled(rect, r(10), G2);
    ui.painter().rect_stroke(rect, r(10), hair(LINE2), StrokeKind::Inside);
    let inner = rect.shrink2(egui::vec2(14.0, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(false)
            .font(font(14.0))
            .text_color(INK)
            .hint_text(egui::RichText::new(hint).color(INK3))
            .desired_width(inner.width()),
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    (resp, enter)
}

impl Studio {
    /// Open the Add sheet (or the host's file picker when it has no sheet).
    pub fn open_add(&mut self, host: &mut dyn Host, target: AddTarget) {
        let routes = host.caps().add;
        if routes.is_empty() {
            host.import_media();
            return;
        }
        if !routes.contains(&self.add.tab) {
            self.add.tab = routes[0];
        }
        self.add.recent.clear();
        self.sheet = Some(Sheet::Add(target));
    }

    /// Place what arrived where it was asked for.
    pub fn place_added(&mut self, host: &mut dyn Host) {
        for a in host.take_added() {
            match a.target {
                AddTarget::Any => {}
                AddTarget::Column(c) if c < super::COLUMNS => {
                    for col in self.columns.iter_mut() {
                        if *col == Some(a.source) {
                            *col = None;
                        }
                    }
                    self.columns[c] = Some(a.source);
                }
                AddTarget::Lane(l) if l < super::LANES => {
                    self.seq.lanes[l] = Some(a.source);
                    self.rev += 1;
                }
                AddTarget::Pad(i) => {
                    if let Some(perf) = host.perform() {
                        perf.load_pad(a.source, i);
                    }
                }
                _ => {}
            }
            if matches!(self.sheet, Some(Sheet::Add(_))) {
                self.add.recent.push(a.source);
            }
        }
    }

    /// Where pending adds show as placeholders: (pending, target slot),
    /// with `Any` mapped onto the next empty grid columns.
    pub fn pending_slots(&self, host: &dyn Host) -> Vec<(Pending, AddTarget)> {
        let mut free: Vec<usize> = (0..super::COLUMNS).filter(|c| self.columns[*c].is_none()).collect();
        let mut out = Vec::new();
        let pend = host.pending();
        for p in &pend {
            if let AddTarget::Column(c) = p.target {
                free.retain(|f| *f != c);
            }
        }
        for p in pend {
            let t = match p.target {
                AddTarget::Any => match free.first().copied() {
                    Some(c) => {
                        free.remove(0);
                        AddTarget::Column(c)
                    }
                    None => AddTarget::Any,
                },
                t => t,
            };
            out.push((p, t));
        }
        out
    }

    pub fn add_sheet(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let Some(Sheet::Add(target)) = self.sheet else { return };
        let screen = ctx.screen_rect();
        let size = egui::vec2((screen.width() - 40.0).min(780.0), (screen.height() - 48.0).min(620.0));
        let routes = host.caps().add;
        let mut close = false;
        let t = ctx.input(|i| i.time);
        let scrim = modal(ctx, "cg-add", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(22.0, 18.0));
            let id = egui::Id::new("cg-add");
            if sheet_header(ui, inner, "Add a clip", &target_label(target), id) {
                close = true;
            }
            let panel = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + 58.0), inner.max);
            self.add_panel(host, ui, panel, target, &routes, t);
        });
        if !host.pending().is_empty() {
            ctx.request_repaint();
        }
        if (close || scrim) && matches!(self.sheet, Some(Sheet::Add(_))) {
            self.sheet = None;
        }
    }

    /// The Add panel, self-contained (any host rect): route tabs, the
    /// route's body, then what's arriving / arrived.
    pub fn add_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, inner: Rect, target: AddTarget, routes: &[AddRoute], t: f64) {
        {
            let id = egui::Id::new("cg-add");
            let labels: Vec<&str> = routes.iter().map(|r| r.label()).collect();
            let names: Vec<String> = labels.iter().map(|l| format!("add {l}")).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let tabs = Rect::from_min_size(inner.min, egui::vec2(inner.width(), 44.0));
            let cur = routes.iter().position(|r| *r == self.add.tab).unwrap_or(0);
            if let Some(i) = segmented_named(ui, tabs, id.with("tabs"), &labels, &names, &[], cur, font_medium(13.0), false) {
                self.add.tab = routes[i];
            }
            // What's arriving / arrived (bottom strip).
            let pend: Vec<Pending> = host.pending();
            let strip_h = if pend.is_empty() && self.add.recent.is_empty() { 0.0 } else { 128.0 };
            let body = Rect::from_min_max(egui::pos2(inner.left(), tabs.bottom() + 16.0), egui::pos2(inner.right(), inner.bottom() - strip_h));
            match self.add.tab {
                AddRoute::Files => self.add_files_tab(host, ui, body, target),
                AddRoute::Browse => self.add_browse_tab(host, ui, body, target),
                AddRoute::Link => self.add_link_tab(host, ui, body, target),
                AddRoute::Dig => self.add_dig_tab(host, ui, body, target),
            }
            if strip_h > 0.0 {
                let strip = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - strip_h + 12.0), inner.max);
                if let Some(chop) = self.arrivals(host, ui, strip, &pend, t) {
                    self.open_clip(ClipTarget::Source(chop));
                }
            }
            if !pend.is_empty() {
                ui.ctx().request_repaint();
            }
        }
    }

    fn add_files_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();
        let zone = Rect::from_min_max(body.min, egui::pos2(body.right(), body.bottom().min(body.top() + 260.0)));
        p.rect_stroke(zone, r(14), Stroke::new(1.0, LINE3), StrokeKind::Inside);
        draw_icon(&p, Icon::Folder, zone.center() - egui::vec2(0.0, 52.0), 30.0, INK2);
        let b = Rect::from_center_size(zone.center() + egui::vec2(0.0, 4.0), egui::vec2(220.0, 48.0));
        if button_named(ui, b, ui.id().with("add-files"), "Choose files…", "choose files", true, None, None).clicked() {
            host.add_files(target);
        }
        p.text(zone.center() + egui::vec2(0.0, 54.0), Align2::CENTER_CENTER, "video or audio · or drop files anywhere", font(12.0), INK3);
    }

    fn add_browse_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let ctx = ui.ctx().clone();
        let search = Rect::from_min_size(body.min, egui::vec2(body.width(), 44.0));
        let mut q = self.add.query.clone();
        field(ui, search, egui::Id::new("cg-browse-q"), &mut q, "Filter by name…", "browse search");
        self.add.query = q;
        let items = host.browse(&ctx, &self.add.query);
        let grid = Rect::from_min_max(egui::pos2(body.left(), search.bottom() + 12.0), body.max);
        if items.is_empty() {
            ui.painter().text(grid.center(), Align2::CENTER_CENTER, "looking for videos on this machine…", font(13.0), INK3);
            return;
        }
        let cols = ((grid.width() / 170.0).floor() as usize).clamp(3, 6);
        let g = 10.0;
        let tw = (grid.width() - g * (cols as f32 - 1.0) - 6.0) / cols as f32;
        let th = tw * 9.0 / 16.0;
        let row_h = th + 30.0;
        let mut pick = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid));
        egui::ScrollArea::vertical().id_salt("cg-browse-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            let rows = items.len().div_ceil(cols);
            let (area, _) = ui.allocate_exact_size(egui::vec2(grid.width() - 6.0, rows as f32 * (row_h + g)), Sense::hover());
            let p = ui.painter().clone();
            for (i, it) in items.iter().enumerate() {
                let rc = Rect::from_min_size(area.min + egui::vec2((i % cols) as f32 * (tw + g), (i / cols) as f32 * (row_h + g)), egui::vec2(tw, th));
                if !ui.is_rect_visible(rc.expand(40.0)) {
                    continue;
                }
                match it.thumb {
                    Some(tex) => tile(&p, rc, 8, tex, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), gray(0.92)),
                    None => {
                        p.rect_filled(rc, r(8), G2);
                    }
                }
                p.rect_stroke(rc, r(8), hair(LINE2), StrokeKind::Inside);
                let resp = hit(ui, Rect::from_min_size(rc.min, egui::vec2(tw, row_h)), egui::Id::new(("cg-browse", i)), Sense::click(), &format!("browse item {}", i + 1));
                if resp.hovered() {
                    p.rect_stroke(rc, r(8), Stroke::new(1.5, INK2), StrokeKind::Inside);
                }
                let room = (tw / 6.6) as usize;
                p.text(egui::pos2(rc.left() + 2.0, rc.bottom() + 8.0), Align2::LEFT_TOP, super::play::short(&it.name, room.max(4)), font(12.0), INK);
                p.text(egui::pos2(rc.right() - 2.0, rc.bottom() + 8.0), Align2::RIGHT_TOP, super::play::short(&it.detail, 12), mono(10.5), INK3);
                if resp.clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            host.browse_add(i, target);
        }
    }

    fn add_link_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();
        let fw = body.width() - 132.0;
        let f = Rect::from_min_size(body.min + egui::vec2(0.0, 6.0), egui::vec2(fw, 48.0));
        let mut link = self.add.link.clone();
        let (_, enter) = field(ui, f, egui::Id::new("cg-link"), &mut link, "Paste a YouTube or any video link", "link url");
        self.add.link = link;
        let b = Rect::from_min_size(egui::pos2(f.right() + 12.0, f.top()), egui::vec2(120.0, 48.0));
        let go = button_named(ui, b, ui.id().with("link-fetch"), "Fetch", "fetch link", true, None, None).clicked();
        if (go || enter) && !self.add.link.trim().is_empty() {
            host.add_link(self.add.link.trim(), target);
            self.add.link.clear();
        }
        p.text(egui::pos2(body.left() + 2.0, f.bottom() + 22.0), Align2::LEFT_CENTER, "YouTube, Vimeo, archive.org — anything yt-dlp can fetch. Long videos are trimmed to a 20–60 s window.", font(12.0), INK3);
    }

    fn add_dig_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();
        let c = egui::pos2(body.center().x, body.top() + 96.0);
        let b = Rect::from_center_size(c, egui::vec2(200.0, 112.0));
        let resp = hit(ui, b, ui.id().with("dig"), Sense::click(), "dig");
        let h = hold(ui, &resp, 0.6);
        let down = resp.is_pointer_button_down_on();
        p.add(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) }.as_shape(b, r(22)));
        p.rect_filled(b, r(22), if down { INK } else { HI });
        draw_icon(&p, Icon::Dig, c - egui::vec2(0.0, 16.0), 30.0, ON_HI);
        p.text(c + egui::vec2(0.0, 22.0), Align2::CENTER_CENTER, "Dig", font_semibold(17.0), ON_HI);
        if down {
            // Hold progress around the button: dig 4 at the end.
            let t0 = ui.input(|i| i.pointer.press_start_time()).unwrap_or(0.0);
            let f = ((ui.input(|i| i.time) - t0) / 0.6).clamp(0.0, 1.0) as f32;
            p.line_segment([egui::pos2(b.left() + 18.0, b.bottom() + 10.0), egui::pos2(b.left() + 18.0 + (b.width() - 36.0) * f, b.bottom() + 10.0)], Stroke::new(2.0, INK2));
            ui.ctx().request_repaint();
        }
        if h.fired {
            host.dig(4, self.add.cc_only, target);
        } else if resp.clicked() && !h.held_before {
            host.dig(1, self.add.cc_only, target);
        }
        p.text(egui::pos2(c.x, b.bottom() + 34.0), Align2::CENTER_CENTER, "a random video almost nobody has seen · hold to dig 4", font(12.5), INK2);
        p.text(egui::pos2(c.x, b.bottom() + 54.0), Align2::CENTER_CENTER, "camera-default names (IMG_4821, MVI_0032…), under 500 views, a 20–60 s window", font(11.5), INK3);
        let cc = Rect::from_center_size(egui::pos2(c.x, b.bottom() + 92.0), egui::vec2(180.0, 40.0));
        if chip(ui, cc, ui.id().with("dig-cc"), "creative commons only", false, self.add.cc_only).clicked() {
            self.add.cc_only = !self.add.cc_only;
        }
        p.text(cc.center(), Align2::CENTER_CENTER, "Creative Commons only", font(12.0), if self.add.cc_only { ON_HI } else { INK2 });
    }

    /// Placeholders for adds under way, then cards for what arrived.
    /// Returns a source whose Chop was tapped.
    fn arrivals(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, strip: Rect, pend: &[Pending], t: f64) -> Option<usize> {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        caps(&p, egui::pos2(strip.left(), strip.top() + 6.0), Align2::LEFT_CENTER, if pend.is_empty() { "Added" } else { "Arriving" }, 10.5);
        let ch = strip.height() - 22.0;
        let cw = ch * 16.0 / 9.0 * 0.9;
        let mut x = strip.left();
        let y = strip.top() + 20.0;
        let mut dismiss = None;
        for pd in pend {
            if x + cw > strip.right() {
                break;
            }
            let rc = Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, ch));
            placeholder_tile(&p, rc, 10, pd, t, false);
            if pd.failed.is_some() && hit(ui, rc, ui.id().with(("pend", pd.id)), Sense::click(), "dismiss").clicked() {
                dismiss = Some(pd.id);
            }
            x += cw + 10.0;
        }
        if let Some(id) = dismiss {
            host.dismiss_pending(id);
        }
        let mut chop = None;
        let recent: Vec<usize> = self.add.recent.iter().rev().copied().collect();
        for src in recent {
            if x + cw > strip.right() {
                break;
            }
            let rc = Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, ch));
            if let Some((tex, a)) = self.atlas(host, &ctx, src) {
                tile(&p, rc, 10, tex, a.uv_cover(5, cw / ch), gray(0.9));
            } else {
                p.rect_filled(rc, r(10), G2);
            }
            let scrim = Rect::from_min_max(egui::pos2(rc.left(), rc.bottom() - 40.0), rc.max);
            p.rect_filled(scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(150));
            let name = self.source_name(host, src);
            p.text(egui::pos2(rc.left() + 9.0, rc.bottom() - 25.0), Align2::LEFT_CENTER, super::play::short(&name, 22), font_medium(11.5), INK);
            p.text(egui::pos2(rc.left() + 9.0, rc.bottom() - 10.0), Align2::LEFT_CENTER, super::play::short(&self.provenance_line(host, src), 30), font(10.0), INK3);
            let cb = Rect::from_min_size(egui::pos2(rc.right() - 66.0, rc.top() + 6.0), egui::vec2(60.0, 30.0));
            p.rect_filled(cb, r(8), Color32::from_black_alpha(160));
            draw_icon(&p, Icon::Chop, egui::pos2(cb.left() + 15.0, cb.center().y), 13.0, INK);
            p.text(egui::pos2(cb.left() + 26.0, cb.center().y), Align2::LEFT_CENTER, "Chop", font(11.5), INK);
            if hit(ui, cb.expand2(egui::vec2(4.0, 7.0)), ui.id().with(("chop-new", src)), Sense::click(), &format!("chop {name}")).clicked() {
                chop = Some(src);
            }
            x += cw + 10.0;
        }
        chop
    }
}
