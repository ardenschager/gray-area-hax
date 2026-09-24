//! Look & feel: palette, egui style, and the handful of small widgets every
//! panel shares (labelled parameter rows, color swatches, section labels).
//! Keeping these in one place is what makes the panels feel like one tool.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 89);
pub const BG: Color32 = Color32::from_rgb(12, 12, 14);
pub const PANEL: Color32 = Color32::from_rgb(19, 19, 22);
pub const RAISED: Color32 = Color32::from_rgb(27, 27, 31);
pub const RAISED_HI: Color32 = Color32::from_rgb(36, 36, 42);
pub const LINE: Color32 = Color32::from_rgb(38, 38, 44);
pub const TEXT: Color32 = Color32::from_gray(214);
pub const TEXT_DIM: Color32 = Color32::from_gray(128);
pub const TEXT_FAINT: Color32 = Color32::from_gray(84);
pub const POLY: Color32 = Color32::from_rgb(122, 196, 255);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.extreme_bg_color = BG;
    v.faint_bg_color = RAISED;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.slider_trailing_fill = true;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let r = CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = RAISED_HI;
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = Color32::from_rgb(52, 52, 60);
    w.hovered.weak_bg_fill = Color32::from_rgb(44, 44, 51);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(72));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.hovered.corner_radius = r;
    w.active.bg_fill = ACCENT.linear_multiply(0.55);
    w.active.weak_bg_fill = ACCENT.linear_multiply(0.45);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.corner_radius = r;
    w.open.corner_radius = r;
    w.open.weak_bg_fill = RAISED_HI;
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 6.0);
        s.spacing.button_padding = egui::vec2(9.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.slider_width = 150.0;
        s.spacing.menu_margin = egui::Margin::same(8);
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(17.0));
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    });
}

/// Every source gets a stable color. The same color is used for its card
/// in the library, its clips on the timeline and its rows in patterns, so
/// you can always see where a piece of media went.
pub fn source_color(i: usize) -> Color32 {
    let hue = (i as f32 * 137.508 + 14.0) % 360.0;
    let (r, g, b) = chromagrain_core::video::hsv_to_rgb(hue, 0.52, 0.92);
    Color32::from_rgb(r, g, b)
}

/// Pattern clips hold several sources, so they get a calmer shared tone.
pub fn pattern_color() -> Color32 {
    Color32::from_rgb(132, 190, 150)
}

/// Small uppercase, letter-spaced label that opens a group of controls.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    let spaced: String = title.to_uppercase().chars().flat_map(|c| [c, '\u{2009}']).collect();
    ui.label(egui::RichText::new(spaced).size(10.0).color(TEXT_DIM));
    ui.add_space(1.0);
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(TEXT_FAINT));
}

pub fn swatch(ui: &mut egui::Ui, color: Color32, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, color);
    resp
}

/// A labelled slider row: fixed-width dim label on the left, slider
/// filling the rest. Keeps every inspector column aligned.
pub struct Param<'a> {
    label: &'a str,
    tip: &'a str,
    lo: f32,
    hi: f32,
    log: bool,
    suffix: &'a str,
}

pub fn param<'a>(label: &'a str, lo: f32, hi: f32) -> Param<'a> {
    Param { label, tip: "", lo, hi, log: false, suffix: "" }
}

impl<'a> Param<'a> {
    pub fn log(mut self) -> Self {
        self.log = true;
        self
    }
    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = s;
        self
    }
    pub fn show(self, ui: &mut egui::Ui, v: &mut f32) -> bool {
        ui.horizontal(|ui| {
            let label_w = 96.0;
            let r = ui
                .allocate_ui_with_layout(
                    egui::vec2(label_w, 20.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(label_w);
                        ui.add(
                            egui::Label::new(egui::RichText::new(self.label).color(TEXT_DIM))
                                .truncate(),
                        )
                    },
                )
                .inner;
            if !self.tip.is_empty() {
                r.on_hover_text(self.tip);
            }
            ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
            let mut s = egui::Slider::new(v, self.lo..=self.hi).logarithmic(self.log);
            if !self.suffix.is_empty() {
                s = s.suffix(self.suffix);
            }
            ui.add(s).changed()
        })
        .inner
    }
}

/// Collapsible inspector group with consistent styling.
pub fn group<R>(
    ui: &mut egui::Ui,
    title: &str,
    open: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    ui.add_space(2.0);
    egui::CollapsingHeader::new(egui::RichText::new(title).size(12.5).color(TEXT))
        .default_open(open)
        .show(ui, add)
        .body_returned
}

pub fn beats_label(beats: f64) -> String {
    let bar = (beats / 4.0).floor() as i64 + 1;
    let beat = (beats % 4.0).floor() as i64 + 1;
    let sixteenth = ((beats.fract()) * 4.0).floor() as i64 + 1;
    format!("{bar}.{beat}.{sixteenth}")
}
