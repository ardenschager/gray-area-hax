//! The Add sheet: YouTube (Search + Link) · Dig (`Dig 4 → Kit`) · Drive
//! (Google Drive + local folders) · Files. One calm modal reached from the
//! grid's "+", a pad's "+" and Sequence's "+". Adds that take time stream in
//! as placeholder tiles, here and where they will land; what arrived shows as
//! source cards (with provenance) that offer Chop straight away.

use super::{ClipTarget, Sheet, Studio};
use crate::host::{AddRoute, AddTarget, Host, Pending};
use crate::theme::*;
use crate::widgets::*;
use egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

const YT_SUGGESTIONS: [&str; 4] = [
    "1960s commercial",
    "cello practice",
    "answering machine",
    "cassette tape loop",
];

/// Add-sheet state that outlives one frame.
pub struct AddState {
    pub tab: AddRoute,
    pub link: String,
    pub yt_query: String,
    pub yt_low_views: bool,
    pub query: String,
    /// `0 = All`, `1 = Google Drive`, `2 = Local`.
    pub drive_scope: usize,
    pub cc_only: bool,
    /// Sources that arrived while the sheet was open (newest last).
    pub recent: Vec<usize>,
}

impl Default for AddState {
    fn default() -> Self {
        AddState {
            tab: AddRoute::Files,
            link: String::new(),
            yt_query: String::new(),
            yt_low_views: false,
            query: String::new(),
            drive_scope: 0,
            cc_only: true,
            recent: Vec::new(),
        }
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

/// Format view counts cleanly (`"142 views"`, `"3.4k views"`).
fn fmt_views(v: u64) -> String {
    if v == 1 {
        "1 view".into()
    } else if v < 1_000 {
        format!("{v} views")
    } else if v < 1_000_000 {
        format!("{:.1}k views", v as f64 / 1_000.0)
    } else {
        format!("{:.1}M views", v as f64 / 1_000_000.0)
    }
}

/// Format duration seconds cleanly (`"0:42"`, `"3:15"`).
fn fmt_dur(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A tile still arriving: a slow sheen, what's happening, progress.
pub fn placeholder_tile(p: &egui::Painter, rect: Rect, radius: u8, pend: &Pending, t: f64, compact: bool) {
    p.rect_filled(rect, r(radius), WELL2);
    if pend.failed.is_none() {
        let phase = (t / 1.6).fract() as f32;
        let bw = rect.width() * 0.45;
        let x = rect.left() - bw + (rect.width() + bw) * phase;
        let band = Rect::from_min_max(egui::pos2(x.max(rect.left()), rect.top()), egui::pos2((x + bw).min(rect.right()), rect.bottom()));
        if band.width() > 0.0 {
            let mut mesh = egui::Mesh::default();
            let (c0, c1) = (Color32::from_white_alpha(0), Color32::from_white_alpha(20));
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
    p.rect_stroke(rect, r(radius), Stroke::new(1.0, if pend.failed.is_some() { RUBY_MID } else { WELL_LINE2 }), StrokeKind::Inside);
    if pend.failed.is_none() {
        let icon = if pend.label.starts_with("digging") { Icon::Dig } else if pend.label.starts_with("loading") { Icon::Folder } else { Icon::Link };
        let c = egui::pos2(rect.center().x, rect.center().y.min(rect.top() + 44.0).max(rect.top() + rect.height() * 0.4));
        draw_icon(p, icon, c - egui::vec2(0.0, 8.0), if compact { 18.0 } else { 22.0 }, CREAM2);
        for k in 0..3 {
            let ph = ((t * 2.2 - k as f64 * 0.22).rem_euclid(1.0)) as f32;
            let a = (1.0 - (ph * 2.0 - 1.0).abs()).clamp(0.15, 1.0);
            p.circle_filled(c + egui::vec2((k as f32 - 1.0) * 8.0, 14.0), 2.2, Color32::from_rgba_unmultiplied(252, 227, 93, (a * 195.0) as u8));
        }
    }
    let label = match &pend.failed {
        Some(e) => super::play::short(e, if compact { 14 } else { 40 }),
        None => super::play::short(&pend.label, if compact { 14 } else { 40 }),
    };
    let size = if compact { 10.0 } else { 11.5 };
    p.text(egui::pos2(rect.left() + 8.0, rect.bottom() - if compact { 10.0 } else { 14.0 }), Align2::LEFT_BOTTOM, label, font(size), if pend.failed.is_some() { RUBY_MID } else { CREAM2 });
    if let (Some(f), None) = (pend.progress, &pend.failed) {
        let y = rect.bottom() - 4.0;
        let w = (rect.width() - 16.0).max(0.0);
        p.line_segment([egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.left() + 8.0 + w, y)], Stroke::new(2.0, WELL3));
        p.line_segment([egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.left() + 8.0 + w * f.clamp(0.0, 1.0), y)], Stroke::new(2.0, AMBER_MID));
    }
}

/// Scrim + centred hardware console sheet; returns whether the scrim was tapped.
pub fn modal(ctx: &egui::Context, id: &str, size: egui::Vec2, body: impl FnOnce(&mut egui::Ui, Rect)) -> bool {
    let screen = ctx.screen_rect();
    let mut closed = false;
    egui::Area::new(egui::Id::new((id, "scrim"))).fixed_pos(screen.min).order(egui::Order::Middle).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(155));
        closed = resp.clicked();
    });
    let sheet = Rect::from_center_size(screen.center(), size);
    egui::Area::new(egui::Id::new(id)).fixed_pos(sheet.min).order(egui::Order::Foreground).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(sheet.size(), Sense::click());
        let p = ui.painter().clone();
        p.add(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(170) }.as_shape(rect, r(10)));
        chassis_plate(&p, rect, CHASSIS_TOP, CHASSIS_BOT);
        p.rect_stroke(rect, r(10), Stroke::new(1.5, BEVEL_LO), StrokeKind::Inside);
        chassis_screws(&p, rect, 10.0);
        body(ui, rect);
    });
    closed
}

/// The sheet's header: title, subtitle, Done. Returns true on Done.
pub fn sheet_header(ui: &mut egui::Ui, inner: Rect, title: &str, sub: &str, id: egui::Id) -> bool {
    let p = ui.painter().clone();
    p.text(egui::pos2(inner.left(), inner.top() + 16.0), Align2::LEFT_CENTER, title.to_uppercase(), font_semibold(17.0), INK);
    p.text(egui::pos2(inner.left(), inner.top() + 37.0), Align2::LEFT_CENTER, sub, font(12.0), INK3);
    let done = Rect::from_min_size(egui::pos2(inner.right() - 74.0, inner.top() + 4.0), egui::vec2(74.0, 36.0));
    let c = button_named(ui, done, id.with("done"), "Done", "close sheet", false, None, None).clicked();
    c
}

/// A sunken LCD-style text input field in the hardware console's look.
pub fn field(ui: &mut egui::Ui, rect: Rect, id: egui::Id, text: &mut String, hint: &str, label: &str) -> (egui::Response, bool) {
    field_sized(ui, rect, id, text, hint, label, 13.0)
}

/// [`field`] with explicit font size.
pub fn field_sized(ui: &mut egui::Ui, rect: Rect, id: egui::Id, text: &mut String, hint: &str, label: &str, font_size: f32) -> (egui::Response, bool) {
    let p = ui.painter().clone();
    p.rect_filled(rect, r(4), LCD_BG);
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.top() + 0.8), egui::pos2(rect.right() - 2.0, rect.top() + 0.8)],
        Stroke::new(1.2, LCD_SHADOW),
    );
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.6), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.6)],
        Stroke::new(1.0, BEVEL_HI),
    );
    p.rect_stroke(rect, r(4), Stroke::new(1.0, Color32::from_rgb(0x66, 0x62, 0x54)), StrokeKind::Inside);
    let inner = rect.shrink2(egui::vec2(10.0, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(false)
            .font(font_medium(font_size))
            .text_color(LCD_INK)
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
                AddTarget::Any => {
                    if !self.columns.contains(&Some(a.source)) {
                        if let Some(slot) = self.columns.iter_mut().find(|c| c.is_none()) {
                            *slot = Some(a.source);
                        }
                    }
                }
                AddTarget::Column(c) if c < super::COLUMNS => {
                    for col in self.columns.iter_mut() {
                        if *col == Some(a.source) {
                            *col = None;
                        }
                    }
                    self.columns[c] = Some(a.source);
                    if self.sheet == Some(Sheet::Add(AddTarget::Column(c))) {
                        self.sheet = Some(Sheet::Add(AddTarget::Any));
                    }
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

    fn next_add_target(&self, host: &dyn Host, target: AddTarget) -> AddTarget {
        match target {
            AddTarget::Column(c)
                if c < super::COLUMNS
                    && (self.columns[c].is_some()
                        || host.pending().iter().any(|p| p.target == AddTarget::Column(c))) =>
            {
                AddTarget::Any
            }
            t => t,
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
        let size = egui::vec2((screen.width() - 40.0).min(820.0), (screen.height() - 44.0).min(650.0));
        let routes = host.caps().add;
        let mut close = false;
        let t = ctx.input(|i| i.time);
        let scrim = modal(ctx, "cg-add", size, |ui, rect| {
            let inner = rect.shrink2(egui::vec2(22.0, 16.0));
            let id = egui::Id::new("cg-add");
            if sheet_header(ui, inner, "Add a clip", &target_label(target), id) {
                close = true;
            }
            let panel = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + 54.0), inner.max);
            self.add_panel(host, ui, panel, target, &routes, t);
        });
        if !host.pending().is_empty() || host.youtube_state().0 {
            ctx.request_repaint();
        }
        if (close || scrim) && matches!(self.sheet, Some(Sheet::Add(_))) {
            self.sheet = None;
        }
    }

    /// The Add panel, self-contained (any host rect): route tabs, the
    /// route's body, then what's arriving / arrived.
    pub fn add_panel(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, inner: Rect, target: AddTarget, routes: &[AddRoute], t: f64) {
        let id = egui::Id::new("cg-add");
        let labels: Vec<&str> = routes.iter().map(|r| r.display_label()).collect();
        let a11y_owned: Vec<String> = routes.iter().map(|r| format!("add {}", r.label())).collect();
        let names: Vec<&str> = a11y_owned.iter().map(String::as_str).collect();
        let tabs = Rect::from_min_size(inner.min, egui::vec2(inner.width(), 42.0));
        let cur = routes.iter().position(|r| *r == self.add.tab).unwrap_or(0);
        if let Some(i) = segmented_named(ui, tabs, id.with("tabs"), &labels, &names, &[], cur, font_medium(13.0), false) {
            self.add.tab = routes[i];
        }
        // What's arriving / arrived (bottom strip).
        let pend: Vec<Pending> = host.pending();
        let strip_h = if pend.is_empty() && self.add.recent.is_empty() { 0.0 } else { 122.0 };
        let body = Rect::from_min_max(egui::pos2(inner.left(), tabs.bottom() + 14.0), egui::pos2(inner.right(), inner.bottom() - strip_h));
        let target = self.next_add_target(host, target);
        match self.add.tab {
            AddRoute::Files => self.add_files_tab(host, ui, body, target),
            AddRoute::Browse => self.add_browse_tab(host, ui, body, target),
            AddRoute::Link => self.add_youtube_tab(host, ui, body, target),
            AddRoute::Dig => self.add_dig_tab(host, ui, body, target),
        }
        if strip_h > 0.0 {
            let strip = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - strip_h + 10.0), inner.max);
            if let Some(chop) = self.arrivals(host, ui, strip, &pend, t) {
                self.open_clip(ClipTarget::Source(chop));
            }
        }
        if !pend.is_empty() {
            ui.ctx().request_repaint();
        }
    }

    fn add_files_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();
        let zone = Rect::from_min_max(body.min, egui::pos2(body.right(), body.bottom().min(body.top() + 248.0)));
        p.rect_stroke(zone, r(14), Stroke::new(1.0, LINE3), StrokeKind::Inside);
        draw_icon(&p, Icon::Folder, zone.center() - egui::vec2(0.0, 48.0), 28.0, INK2);
        let b = Rect::from_center_size(zone.center() + egui::vec2(0.0, 4.0), egui::vec2(220.0, 46.0));
        if button_named(ui, b, ui.id().with("add-files"), "Choose files…", "choose files", true, None, None).clicked() {
            host.add_files(target);
        }
        p.text(zone.center() + egui::vec2(0.0, 50.0), Align2::CENTER_CENTER, "video or audio · or drop files anywhere", font(12.0), INK3);
    }

    fn add_browse_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let ctx = ui.ctx().clone();
        let scope_w = 186.0;
        let search_w = (body.width() - scope_w - 10.0).max(160.0);
        let search = Rect::from_min_size(body.min, egui::vec2(search_w, 40.0));
        let mut q = self.add.query.clone();
        field(ui, search, egui::Id::new("cg-browse-q"), &mut q, "Filter Google Drive & local videos…", "browse search");
        self.add.query = q;

        let scope_r = Rect::from_min_size(egui::pos2(search.right() + 10.0, body.top()), egui::vec2(scope_w, 40.0));
        if let Some(ns) = segmented_named(
            ui,
            scope_r,
            egui::Id::new("cg-browse-scope"),
            &["All", "Drive", "Local"],
            &["browse all", "browse drive", "browse local"],
            &[],
            self.add.drive_scope.min(2),
            font(11.5),
            false,
        ) {
            self.add.drive_scope = ns;
        }

        let eff_query = match self.add.drive_scope {
            1 => format!("drive {}", self.add.query).trim().to_string(),
            2 => format!("local {}", self.add.query).trim().to_string(),
            _ => self.add.query.clone(),
        };
        let items = host.browse(&ctx, &eff_query);
        let grid = Rect::from_min_max(egui::pos2(body.left(), search.bottom() + 10.0), body.max);
        if items.is_empty() {
            let p = ui.painter().clone();
            let msg = if self.add.drive_scope == 1 && !host.browse_has_drive() {
                "no mounted Google Drive videos found under ~/Library/CloudStorage/GoogleDrive-*"
            } else {
                "scanning Google Drive and local video folders…"
            };
            p.text(grid.center() - egui::vec2(0.0, 18.0), Align2::CENTER_CENTER, msg, font(12.5), INK3);
            let pick_b = Rect::from_center_size(grid.center() + egui::vec2(0.0, 22.0), egui::vec2(174.0, 38.0));
            if button_named(ui, pick_b, ui.id().with("drive-pick-files"), "Browse files…", "drive browse files", false, Some(Icon::Folder), None).clicked() {
                host.add_files(target);
            }
            return;
        }
        let cols = ((grid.width() / 168.0).floor() as usize).clamp(3, 6);
        let g = 10.0;
        let tw = (grid.width() - g * (cols as f32 - 1.0) - 6.0) / cols as f32;
        let th = tw * 9.0 / 16.0;
        let row_h = th + 28.0;
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
                let room = (tw / 6.8) as usize;
                p.text(egui::pos2(rc.left() + 2.0, rc.bottom() + 6.0), Align2::LEFT_TOP, super::play::short(&it.name, room.max(4)), font(11.5), INK);
                p.text(egui::pos2(rc.right() - 2.0, rc.bottom() + 6.0), Align2::RIGHT_TOP, super::play::short(&it.detail, 12), mono(10.0), INK3);
                if resp.clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            host.browse_add(i, target);
        }
    }

    fn add_youtube_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();

        // 1. Top row: YouTube Search input + `<500 views` chip + `Search` button.
        let search_btn_w = 88.0;
        let low_w = 96.0;
        let gap = 8.0;
        let q_w = (body.width() - search_btn_w - low_w - gap * 2.0).max(160.0);
        let q_rect = Rect::from_min_size(body.min, egui::vec2(q_w, 40.0));
        let mut yt_q = self.add.yt_query.clone();
        let (_, q_enter) = field(
            ui,
            q_rect,
            egui::Id::new("cg-yt-q"),
            &mut yt_q,
            "Search YouTube — 1960s commercial, cello practice, answering machine…",
            "youtube search",
        );
        self.add.yt_query = yt_q;

        let low_rect = Rect::from_min_size(egui::pos2(q_rect.right() + gap, q_rect.top()), egui::vec2(low_w, 40.0));
        if chip(ui, low_rect, ui.id().with("yt-low-views"), "low views", false, self.add.yt_low_views).clicked() {
            self.add.yt_low_views = !self.add.yt_low_views;
            if !self.add.yt_query.trim().is_empty() {
                host.search_youtube(self.add.yt_query.trim(), self.add.yt_low_views);
            }
        }
        p.text(
            low_rect.center(),
            Align2::CENTER_CENTER,
            "<500 views",
            mono(11.0),
            if self.add.yt_low_views { ON_HI } else { INK2 },
        );

        let sb_rect = Rect::from_min_size(egui::pos2(low_rect.right() + gap, q_rect.top()), egui::vec2(search_btn_w, 40.0));
        let do_search = button_named(ui, sb_rect, ui.id().with("yt-search-go"), "Search", "search youtube", true, None, None).clicked();
        if (do_search || q_enter) && !self.add.yt_query.trim().is_empty() {
            host.search_youtube(self.add.yt_query.trim(), self.add.yt_low_views);
        }

        // 2. Bottom row: Direct Paste Link + Fetch (preserves `link url` and `fetch link` a11y!).
        let link_h = 40.0;
        let fetch_w = 96.0;
        let link_y = (body.bottom() - link_h).max(q_rect.bottom() + 48.0);
        let link_rect = Rect::from_min_size(egui::pos2(body.left(), link_y), egui::vec2(body.width() - fetch_w - gap, link_h));
        let mut link = self.add.link.clone();
        let (_, link_enter) = field(
            ui,
            link_rect,
            egui::Id::new("cg-link"),
            &mut link,
            "Or paste a YouTube, Vimeo, or archive.org link…",
            "link url",
        );
        self.add.link = link;
        let fb = Rect::from_min_size(egui::pos2(link_rect.right() + gap, link_rect.top()), egui::vec2(fetch_w, link_h));
        let go_link = button_named(ui, fb, ui.id().with("link-fetch"), "Fetch", "fetch link", !self.add.link.trim().is_empty(), None, None).clicked();
        if (go_link || link_enter) && !self.add.link.trim().is_empty() {
            host.add_link(self.add.link.trim(), target);
            self.add.link.clear();
        }

        // 3. Middle zone: Search results list or 1-tap starter search pills.
        let mid = Rect::from_min_max(
            egui::pos2(body.left(), q_rect.bottom() + 8.0),
            egui::pos2(body.right(), link_rect.top() - 8.0),
        );
        if mid.height() < 32.0 {
            return;
        }
        let (searching, err, hits) = host.youtube_state();
        if searching {
            putty_panel(&p, mid, 6);
            p.text(mid.center(), Align2::CENTER_CENTER, "searching YouTube…", font_medium(12.5), INK2);
            return;
        }
        if hits.is_empty() {
            putty_panel(&p, mid, 6);
            let mut sx = mid.left() + 12.0;
            let sy = mid.top() + 12.0;
            let mut picked_sug: Option<&str> = None;
            for sug in YT_SUGGESTIONS {
                let sw = p.layout_no_wrap(sug.into(), font_medium(11.5), INK).size().x + 22.0;
                if sx + sw > mid.right() - 12.0 {
                    break;
                }
                let sr = Rect::from_min_size(egui::pos2(sx, sy), egui::vec2(sw, 30.0));
                if chip(ui, sr, ui.id().with(("yt-sug", sug)), &format!("suggest {sug}"), false, false).clicked() {
                    picked_sug = Some(sug);
                }
                p.text(sr.center(), Align2::CENTER_CENTER, sug, font_medium(11.5), INK);
                sx += sw + 8.0;
            }
            if let Some(s) = picked_sug {
                self.add.yt_query = s.to_string();
                host.search_youtube(s, self.add.yt_low_views);
            }
            let note = err.unwrap_or_else(|| {
                "Click any search result to trim & import a 20–60 s window · zero AI, direct yt-dlp.".into()
            });
            if mid.height() >= 76.0 {
                p.text(
                    egui::pos2(mid.left() + 14.0, (sy + 48.0).min(mid.bottom() - 14.0)),
                    Align2::LEFT_CENTER,
                    super::play::short(&note, 96),
                    font(11.5),
                    INK3,
                );
            }
            return;
        }

        let mut picked_url: Option<String> = None;
        let row_h = 38.0;
        let row_g = 5.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(mid));
        egui::ScrollArea::vertical()
            .id_salt("cg-yt-results")
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let total_h = hits.len() as f32 * (row_h + row_g);
                let (area, _) = ui.allocate_exact_size(egui::vec2((mid.width() - 6.0).max(120.0), total_h), Sense::hover());
                let lp = ui.painter().clone();
                for (i, h) in hits.iter().enumerate() {
                    let rr = Rect::from_min_size(
                        egui::pos2(area.left(), area.top() + i as f32 * (row_h + row_g)),
                        egui::vec2(area.width(), row_h),
                    );
                    if !ui.is_rect_visible(rr.expand(20.0)) {
                        continue;
                    }
                    let resp = hit(ui, rr, egui::Id::new(("yt-hit", i)), Sense::click(), &format!("youtube result {}", i + 1));
                    let hov = resp.hovered() || resp.is_pointer_button_down_on();
                    paint_switch_cap(&lp, rr, SwitchKind::PuttyAmber, hov, hov, resp.is_pointer_button_down_on());

                    let mut meta_parts = Vec::new();
                    if !h.uploader.is_empty() {
                        meta_parts.push(super::play::short(&h.uploader, 18));
                    }
                    if let Some(v) = h.views {
                        meta_parts.push(fmt_views(v));
                    }
                    if let Some(d) = h.duration_secs {
                        meta_parts.push(fmt_dur(d));
                    }
                    let meta = meta_parts.join(" · ");
                    let meta_w = lp.layout_no_wrap(meta.clone(), mono(10.5), INK2).size().x;
                    let max_title_chars = ((rr.width() - meta_w - 48.0) / 7.0).max(12.0) as usize;

                    draw_icon(&lp, Icon::Play, egui::pos2(rr.left() + 15.0, rr.center().y), 11.5, if hov { ON_HI } else { INK2 });
                    lp.text(
                        egui::pos2(rr.left() + 28.0, rr.center().y),
                        Align2::LEFT_CENTER,
                        super::play::short(&h.title, max_title_chars),
                        font_semibold(12.0),
                        if hov { ON_HI } else { INK },
                    );
                    lp.text(
                        egui::pos2(rr.right() - 12.0, rr.center().y),
                        Align2::RIGHT_CENTER,
                        meta,
                        mono(10.5),
                        if hov { ON_HI } else { INK3 },
                    );
                    if resp.clicked() {
                        picked_url = Some(h.url.clone());
                    }
                }
            });
        if let Some(url) = picked_url {
            host.add_link(&url, target);
        }
    }

    fn add_dig_tab(&mut self, host: &mut dyn Host, ui: &mut egui::Ui, body: Rect, target: AddTarget) {
        let p = ui.painter().clone();
        let compact = body.height() < 305.0;
        let btn_h = if compact { 92.0 } else { 112.0 };
        let c = egui::pos2(body.center().x, body.top() + if compact { 54.0 } else { 86.0 });
        let b = Rect::from_center_size(c, egui::vec2(196.0, btn_h));
        let resp = hit(ui, b, ui.id().with("dig"), Sense::click(), "dig");
        let h = hold(ui, &resp, 0.6);
        let down = resp.is_pointer_button_down_on();
        paint_switch_cap(&p, b, SwitchKind::Amber, true, resp.hovered(), down);
        draw_icon(&p, Icon::Dig, c - egui::vec2(0.0, 14.0), if compact { 26.0 } else { 30.0 }, ON_HI);
        p.text(c + egui::vec2(0.0, 20.0), Align2::CENTER_CENTER, "DIG", font_semibold(17.0), ON_HI);
        if down {
            let t0 = ui.input(|i| i.pointer.press_start_time()).unwrap_or(0.0);
            let f = ((ui.input(|i| i.time) - t0) / 0.6).clamp(0.0, 1.0) as f32;
            p.line_segment([egui::pos2(b.left() + 18.0, b.bottom() + 8.0), egui::pos2(b.left() + 18.0 + (b.width() - 36.0) * f, b.bottom() + 8.0)], Stroke::new(2.0, INK2));
            ui.ctx().request_repaint();
        }
        if h.fired {
            host.dig(4, self.add.cc_only, target);
        } else if resp.clicked() && !h.held_before {
            host.dig(1, self.add.cc_only, target);
        }
        let l1_y = b.bottom() + if compact { 22.0 } else { 30.0 };
        let l2_y = l1_y + if compact { 18.0 } else { 20.0 };
        p.text(egui::pos2(c.x, l1_y), Align2::CENTER_CENTER, "a random video almost nobody has seen · hold to dig 4", font(12.5), INK2);
        p.text(egui::pos2(c.x, l2_y), Align2::CENTER_CENTER, "camera-default names (IMG_4821, MVI_0032…), under 500 views, a 20–60 s window", font(11.5), INK3);

        let row_y = l2_y + if compact { 32.0 } else { 42.0 };
        let kit_w = 148.0;
        let cc_w = 180.0;
        let total_w = kit_w + 10.0 + cc_w;
        let x0 = c.x - total_w * 0.5;

        let kit_r = Rect::from_min_size(egui::pos2(x0, row_y - 19.0), egui::vec2(kit_w, 38.0));
        if button_named(ui, kit_r, ui.id().with("dig-4-kit"), "Dig 4 → Kit", "dig 4 to kit", false, Some(Icon::Grid), None).clicked() {
            host.dig_to_kit(4, self.add.cc_only);
        }

        let cc = Rect::from_min_size(egui::pos2(kit_r.right() + 10.0, row_y - 19.0), egui::vec2(cc_w, 38.0));
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
            let scrim = Rect::from_min_max(egui::pos2(rc.left(), rc.bottom() - 42.0), rc.max);
            p.rect_filled(scrim, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(205));
            let name = self.source_name(host, src);
            p.text(egui::pos2(rc.left() + 9.0, rc.bottom() - 26.0), Align2::LEFT_CENTER, super::play::short(&name, 22), font_medium(11.5), CREAM);
            p.text(egui::pos2(rc.left() + 9.0, rc.bottom() - 10.0), Align2::LEFT_CENTER, super::play::short(&self.provenance_line(host, src), 32), font(10.0), CREAM2);
            let cb = Rect::from_min_size(egui::pos2(rc.right() - 66.0, rc.top() + 6.0), egui::vec2(60.0, 30.0));
            p.rect_filled(cb, r(6), Color32::from_black_alpha(190));
            draw_icon(&p, Icon::Chop, egui::pos2(cb.left() + 15.0, cb.center().y), 13.0, CREAM);
            p.text(egui::pos2(cb.left() + 26.0, cb.center().y), Align2::LEFT_CENTER, "Chop", font_medium(11.5), CREAM);
            if hit(ui, cb.expand2(egui::vec2(4.0, 7.0)), ui.id().with(("chop-new", src)), Sense::click(), &format!("chop {name}")).clicked() {
                chop = Some(src);
            }
            x += cw + 10.0;
        }
        chop
    }
}
