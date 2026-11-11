//! Studio look: neutral graphite + one off-white highlight; colour only
//! comes from the footage. Tokens mirror design/v001/NOTES.md.

use egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke};

pub const G0: Color32 = Color32::from_rgb(0x0b, 0x0b, 0x0c);
pub const G1: Color32 = Color32::from_rgb(0x12, 0x12, 0x13);
pub const G2: Color32 = Color32::from_rgb(0x18, 0x18, 0x1a);
pub const G3: Color32 = Color32::from_rgb(0x20, 0x20, 0x22);
pub const G4: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const G5: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3e);
pub const INK: Color32 = Color32::from_rgb(0xec, 0xeb, 0xe8);
pub const INK2: Color32 = Color32::from_rgb(0x9a, 0x98, 0x94);
pub const INK3: Color32 = Color32::from_rgb(0x5f, 0x5d, 0x5a);
pub const INK4: Color32 = Color32::from_rgb(0x3e, 0x3d, 0x3b);
/// The only highlight: selection, playhead, knob arc, active view.
pub const HI: Color32 = Color32::from_rgb(0xf3, 0xf2, 0xee);
/// Text on the highlight.
pub const ON_HI: Color32 = Color32::from_rgb(0x0b, 0x0b, 0x0c);
/// Only while recording.
pub const REC: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);

/// Hairlines: white at 6 / 10 / 16 %.
pub const LINE: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);
pub const LINE2: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
pub const LINE3: Color32 = Color32::from_rgba_premultiplied(41, 41, 41, 41);

pub fn r(px: u8) -> CornerRadius {
    CornerRadius::same(px)
}

pub fn hair(c: Color32) -> Stroke {
    Stroke::new(1.0, c)
}

/// Geist (UI) — regular / medium / semibold — and Geist Mono (numbers).
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn font_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("geist-medium".into()))
}
pub fn font_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("geist-semibold".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn mono_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mono-medium".into()))
}

/// Embed the fonts (OFL, assets/fonts/OFL.txt) and the studio visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let add = |fonts: &mut egui::FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
    };
    add(&mut fonts, "geist", include_bytes!("../../../assets/fonts/Geist-Regular.ttf"));
    add(&mut fonts, "geist-medium", include_bytes!("../../../assets/fonts/Geist-Medium.ttf"));
    add(&mut fonts, "geist-semibold", include_bytes!("../../../assets/fonts/Geist-SemiBold.ttf"));
    add(&mut fonts, "mono", include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf"));
    add(&mut fonts, "mono-medium", include_bytes!("../../../assets/fonts/GeistMono-Medium.ttf"));
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with_fallback = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, with_fallback("geist"));
    fonts.families.insert(FontFamily::Monospace, with_fallback("mono"));
    for name in ["geist-medium", "geist-semibold", "mono-medium"] {
        fonts.families.insert(FontFamily::Name(name.into()), with_fallback(name));
    }
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = G0;
    v.window_fill = G1;
    v.extreme_bg_color = G1;
    v.faint_bg_color = G2;
    v.override_text_color = Some(INK);
    v.window_stroke = hair(LINE2);
    v.window_corner_radius = r(14);
    v.menu_corner_radius = r(10);
    v.selection.bg_fill = G4;
    v.selection.stroke = Stroke::new(1.0, HI);
    v.hyperlink_color = HI;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r(8);
        w.fg_stroke = Stroke::new(1.0, INK2);
    }
    v.widgets.noninteractive.bg_fill = G1;
    v.widgets.noninteractive.bg_stroke = hair(LINE);
    v.widgets.inactive.bg_fill = G2;
    v.widgets.inactive.weak_bg_fill = G2;
    v.widgets.inactive.bg_stroke = hair(LINE);
    v.widgets.hovered.bg_fill = G3;
    v.widgets.hovered.weak_bg_fill = G3;
    v.widgets.hovered.bg_stroke = hair(LINE2);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    v.widgets.active.bg_fill = G4;
    v.widgets.active.weak_bg_fill = G4;
    v.widgets.active.bg_stroke = hair(LINE3);
    v.widgets.active.fg_stroke = Stroke::new(1.0, HI);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.text_styles.insert(egui::TextStyle::Body, font(13.0));
    style.text_styles.insert(egui::TextStyle::Button, font(13.0));
    style.text_styles.insert(egui::TextStyle::Small, font(11.0));
    style.text_styles.insert(egui::TextStyle::Monospace, mono(12.0));
    style.text_styles.insert(egui::TextStyle::Heading, font_semibold(17.0));
    ctx.set_style(style);
}

/// Multiply a colour's brightness (for dimming footage tints).
pub fn gray(v: f32) -> Color32 {
    let c = (v.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(c, c, c)
}
