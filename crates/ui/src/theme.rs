//! Studio hardware industrial design (`ISOL8` warm anodized sand-olive console):
//! brushed sand-olive chassis, lighter putty sub-panels, machined dark charcoal
//! pointer knobs with sunken LCD readouts, recessed backlit translucent
//! incandescent amber / smoked ruby / mint-putty push-buttons, and deep
//! bakelite monitor bezels framing the footage.

use egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke};

// --- Anodized Sand-Olive Chassis & Brushed Putty Surfaces ---
pub const CHASSIS_TOP: Color32 = Color32::from_rgb(0xb4, 0xac, 0x8c);
pub const G0: Color32 = Color32::from_rgb(0xa6, 0x9e, 0x7e);
pub const CHASSIS_BOT: Color32 = Color32::from_rgb(0x94, 0x8c, 0x6e);
pub const BEVEL_HI: Color32 = Color32::from_rgb(0xce, 0xc7, 0xac);
pub const BEVEL_LO: Color32 = Color32::from_rgb(0x5a, 0x54, 0x40);

pub const G1: Color32 = Color32::from_rgb(0xb8, 0xb1, 0x9a);
pub const G2: Color32 = Color32::from_rgb(0xad, 0xa8, 0x99);
pub const G3: Color32 = Color32::from_rgb(0xc3, 0xbe, 0xae);
pub const G4: Color32 = Color32::from_rgb(0x8c, 0x85, 0x6b);
pub const G5: Color32 = Color32::from_rgb(0x74, 0x6d, 0x56);

pub const PANEL: Color32 = G1;
pub const PANEL_HI: Color32 = Color32::from_rgb(0xd0, 0xca, 0xb5);
pub const PANEL_LO: Color32 = Color32::from_rgb(0x72, 0x6c, 0x56);

pub const PUTTY: Color32 = Color32::from_rgb(0xaf, 0xaa, 0x9d);
pub const PUTTY_HI: Color32 = Color32::from_rgb(0xcb, 0xc6, 0xb9);
pub const PUTTY_LO: Color32 = Color32::from_rgb(0x6d, 0x68, 0x5c);

// --- Silkscreened Ink on Sand / Putty Metal ---
pub const INK: Color32 = Color32::from_rgb(0x23, 0x1f, 0x16);
pub const INK2: Color32 = Color32::from_rgb(0x43, 0x3d, 0x2c);
pub const INK3: Color32 = Color32::from_rgb(0x5c, 0x55, 0x3f);
pub const INK4: Color32 = Color32::from_rgb(0x75, 0x6d, 0x55);

// --- Sunken Brushed-Metal LCD & Dark Display Slots ---
pub const LCD_BG: Color32 = Color32::from_rgb(0xa2, 0x9f, 0x97);
pub const LCD_SHADOW: Color32 = Color32::from_rgb(0x5c, 0x59, 0x50);
pub const LCD_INK: Color32 = Color32::from_rgb(0x1b, 0x19, 0x14);

pub const SLOT_BG: Color32 = Color32::from_rgb(0x27, 0x25, 0x1f);
pub const SLOT_INK: Color32 = Color32::from_rgb(0xc8, 0xc0, 0xa3);

// --- Recessed Dark Bakelite / Broadcast Monitor Wells (Footage & Oscilloscopes) ---
pub const WELL: Color32 = Color32::from_rgb(0x16, 0x19, 0x1a);
pub const WELL2: Color32 = Color32::from_rgb(0x20, 0x24, 0x25);
pub const WELL3: Color32 = Color32::from_rgb(0x2c, 0x31, 0x33);
pub const CREAM: Color32 = Color32::from_rgb(0xf3, 0xf0, 0xe6);
pub const CREAM2: Color32 = Color32::from_rgb(0xc6, 0xc0, 0xb0);
pub const CREAM3: Color32 = Color32::from_rgb(0x8c, 0x87, 0x7a);
pub const WELL_LINE: Color32 = Color32::from_rgba_premultiplied(28, 28, 28, 28);
pub const WELL_LINE2: Color32 = Color32::from_rgba_premultiplied(56, 56, 56, 56);

// --- Backlit Translucent Hardware Lamps (Amber, Ruby, Mint-Teal) ---
pub const AMBER_CORE: Color32 = Color32::from_rgb(0xff, 0xfd, 0xd4);
pub const AMBER_MID: Color32 = Color32::from_rgb(0xfc, 0xe3, 0x5d);
pub const AMBER_EDGE: Color32 = Color32::from_rgb(0xcb, 0x9d, 0x33);
pub const AMBER_OFF: Color32 = Color32::from_rgb(0x7d, 0x6a, 0x38);
pub const AMBER_OFF_EDGE: Color32 = Color32::from_rgb(0x55, 0x47, 0x22);

pub const RUBY_CORE: Color32 = Color32::from_rgb(0xff, 0xd6, 0xc9);
pub const RUBY_MID: Color32 = Color32::from_rgb(0xec, 0x56, 0x47);
pub const RUBY_EDGE: Color32 = Color32::from_rgb(0xa4, 0x2c, 0x22);
pub const RUBY_OFF: Color32 = Color32::from_rgb(0x7b, 0x3b, 0x34);
pub const RUBY_OFF_EDGE: Color32 = Color32::from_rgb(0x54, 0x24, 0x1f);

pub const MINT_CORE: Color32 = Color32::from_rgb(0xcc, 0xff, 0xe8);
pub const MINT_MID: Color32 = Color32::from_rgb(0x63, 0xd9, 0xa7);
pub const MINT_EDGE: Color32 = Color32::from_rgb(0x38, 0x84, 0x63);

/// Primary active lamp highlight (incandescent amber-gold).
pub const HI: Color32 = AMBER_MID;
/// Crisp dark silkscreen text on lit amber / mint caps.
pub const ON_HI: Color32 = Color32::from_rgb(0x1e, 0x17, 0x06);
/// Glowing ruby indicator (record / mute).
pub const REC: Color32 = RUBY_MID;

/// Etched grooves on anodized metal (premultiplied RGBA).
pub const LINE: Color32 = Color32::from_rgba_premultiplied(11, 10, 7, 52);
pub const LINE2: Color32 = Color32::from_rgba_premultiplied(15, 14, 9, 86);
pub const LINE3: Color32 = Color32::from_rgba_premultiplied(19, 17, 11, 128);

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

/// Embed the fonts (OFL, assets/fonts/OFL.txt) and the studio hardware visuals.
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
    *v = egui::Visuals::light();
    v.panel_fill = G0;
    v.window_fill = G1;
    v.extreme_bg_color = LCD_BG;
    v.faint_bg_color = G2;
    v.override_text_color = Some(INK);
    v.window_stroke = hair(LINE2);
    v.window_corner_radius = r(10);
    v.menu_corner_radius = r(6);
    v.selection.bg_fill = AMBER_MID;
    v.selection.stroke = Stroke::new(1.0, INK);
    v.hyperlink_color = INK;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r(5);
        w.fg_stroke = Stroke::new(1.0, INK2);
    }
    v.widgets.noninteractive.bg_fill = G1;
    v.widgets.noninteractive.bg_stroke = hair(LINE);
    v.widgets.inactive.bg_fill = PUTTY;
    v.widgets.inactive.weak_bg_fill = PUTTY;
    v.widgets.inactive.bg_stroke = hair(LINE2);
    v.widgets.hovered.bg_fill = G3;
    v.widgets.hovered.weak_bg_fill = G3;
    v.widgets.hovered.bg_stroke = hair(LINE3);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    v.widgets.active.bg_fill = AMBER_MID;
    v.widgets.active.weak_bg_fill = AMBER_MID;
    v.widgets.active.bg_stroke = hair(LINE3);
    v.widgets.active.fg_stroke = Stroke::new(1.0, ON_HI);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.text_styles.insert(egui::TextStyle::Body, font(13.0));
    style.text_styles.insert(egui::TextStyle::Button, font_medium(13.0));
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
