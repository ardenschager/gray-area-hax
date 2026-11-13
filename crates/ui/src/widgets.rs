//! Tactile broadcast / studio hardware widgets (`ISOL8` industrial design):
//! anodized sand-olive chassis plates, machined corner screws, brushed putty
//! sub-panels, recessed bakelite monitor bezels, cylindrical charcoal pointer
//! knobs with sunken LCD readouts, and backlit translucent incandescent
//! amber / smoked ruby / mint-putty console switches.

use crate::theme::*;
use egui::{
    self, epaint::RectShape, text::LayoutJob, Align2, Color32, FontId, Painter, Pos2, Rect,
    Response, Sense, Shape, Stroke, StrokeKind, TextFormat, TextureId, Ui, Vec2,
};
use std::f32::consts::{PI, TAU};

/// Rounded footage tile: `uv` sub-rect of a texture, tinted.
pub fn tile(painter: &Painter, rect: Rect, radius: u8, tex: TextureId, uv: Rect, tint: Color32) {
    painter.add(Shape::Rect(RectShape::filled(rect, r(radius), tint).with_texture(tex, uv)));
}

/// Single-line text with explicit letter spacing; returns its rect.
pub fn text_spaced(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
) -> Rect {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// Silkscreened uppercase section header on sand/putty metal (`ISOL8`, `CH. MODE`, `OUTPUT`).
pub fn caps(painter: &Painter, pos: Pos2, align: Align2, text: &str, size: f32) -> Rect {
    text_spaced(painter, pos, align, &text.to_uppercase(), font_semibold(size), INK2, size * 0.08)
}

/// Text made of (string, font, colour) runs.
pub fn runs(painter: &Painter, pos: Pos2, align: Align2, parts: &[(&str, FontId, Color32)]) -> Rect {
    let mut job = LayoutJob::default();
    for (s, f, c) in parts {
        job.append(s, 0.0, TextFormat { font_id: f.clone(), color: *c, ..Default::default() });
    }
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, INK);
    rect
}

// ============================================================================
// Hardware Chassis, Sub-Panels, Monitor Bezels, Screws, LEDs & LCD Readouts
// ============================================================================

/// Paint vertical linear-gradient quad.
pub fn vgrad_rect(painter: &Painter, rect: Rect, top: Color32, bot: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bot);
    mesh.colored_vertex(rect.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Paint radial lamp / keycap gradient inside `rect` (center filament -> mid -> outer edge).
pub fn radial_lens(painter: &Painter, rect: Rect, core: Color32, mid: Color32, edge: Color32) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let c = rect.center();
    let inner = rect.shrink2(egui::vec2(rect.width() * 0.24, rect.height() * 0.24));
    let mut mesh = egui::Mesh::default();
    // 0: hot center
    mesh.colored_vertex(c, core);
    // 1..=4: mid ring
    let mid_pts = [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom()];
    for pt in mid_pts {
        mesh.colored_vertex(pt, mid);
    }
    // 5..=8: outer edge
    let out_pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    for pt in out_pts {
        mesh.colored_vertex(pt, edge);
    }
    for k in 0..4u32 {
        let nk = (k + 1) % 4;
        mesh.add_triangle(0, 1 + k, 1 + nk);
        mesh.add_triangle(1 + k, 5 + k, 5 + nk);
        mesh.add_triangle(1 + k, 5 + nk, 1 + nk);
    }
    painter.add(Shape::mesh(mesh));
}

/// Main warm anodized sand-olive metal faceplate with machined top/bottom bevels.
pub fn chassis_plate(painter: &Painter, rect: Rect, top: Color32, bot: Color32) {
    vgrad_rect(painter, rect, top, bot);
    painter.line_segment(
        [egui::pos2(rect.left(), rect.top() + 0.5), egui::pos2(rect.right(), rect.top() + 0.5)],
        Stroke::new(1.0, BEVEL_HI),
    );
    painter.line_segment(
        [egui::pos2(rect.left(), rect.bottom() - 0.5), egui::pos2(rect.right(), rect.bottom() - 0.5)],
        Stroke::new(1.0, BEVEL_LO),
    );
}

/// 4 machined slotted gunmetal rack/corner screws on outer faceplates & modals.
pub fn chassis_screws(painter: &Painter, rect: Rect, inset: f32) {
    if rect.width() < 120.0 || rect.height() < 80.0 {
        return;
    }
    let angles = [0.55f32, -0.42, 0.78, -0.25];
    for ((x, y), ang) in [
        (rect.left() + inset, rect.top() + inset),
        (rect.right() - inset, rect.top() + inset),
        (rect.left() + inset, rect.bottom() - inset),
        (rect.right() - inset, rect.bottom() - inset),
    ]
    .into_iter()
    .zip(angles)
    {
        let c = egui::pos2(x, y);
        painter.circle_filled(c + egui::vec2(0.0, 1.0), 4.8, Color32::from_rgba_unmultiplied(255, 250, 232, 75));
        painter.circle_filled(c, 4.5, Color32::from_rgb(0x52, 0x4d, 0x3d));
        painter.circle_filled(c, 3.6, Color32::from_rgb(0x6e, 0x68, 0x55));
        painter.circle_stroke(c, 4.2, Stroke::new(0.8, Color32::from_rgb(0x34, 0x30, 0x24)));
        let d = egui::vec2(ang.cos() * 2.7, ang.sin() * 2.7);
        painter.line_segment([c - d, c + d], Stroke::new(1.2, Color32::from_rgb(0x28, 0x25, 0x1c)));
    }
}

/// Lighter brushed putty inset sub-panel (like `ATTEN.` in `ISOL8`).
pub fn putty_panel(painter: &Painter, rect: Rect, radius: u8) {
    painter.rect_filled(rect.translate(egui::vec2(0.0, 1.0)), r(radius), Color32::from_rgba_unmultiplied(255, 250, 234, 65));
    painter.rect_filled(rect, r(radius), PANEL);
    painter.rect_stroke(rect, r(radius), Stroke::new(1.0, PANEL_LO), StrokeKind::Inside);
    let inset = (radius as f32 * 0.6).max(2.0);
    painter.line_segment(
        [egui::pos2(rect.left() + inset, rect.top() + 1.2), egui::pos2(rect.right() - inset, rect.top() + 1.2)],
        Stroke::new(1.0, PANEL_HI),
    );
}

/// Recessed dark bakelite / CRT monitor well for footage, waveforms, and step matrices.
pub fn monitor_well(painter: &Painter, rect: Rect, radius: u8) {
    painter.rect_filled(rect, r(radius), WELL);
    // Sunken top shadow + warm anodized bottom catchlight.
    let inset = (radius as f32 * 0.5).max(2.0);
    painter.line_segment(
        [egui::pos2(rect.left() + inset, rect.top() + 1.0), egui::pos2(rect.right() - inset, rect.top() + 1.0)],
        Stroke::new(1.8, Color32::from_black_alpha(180)),
    );
    painter.line_segment(
        [egui::pos2(rect.left() + inset, rect.bottom() + 0.8), egui::pos2(rect.right() - inset, rect.bottom() + 0.8)],
        Stroke::new(1.0, BEVEL_HI),
    );
    painter.rect_stroke(rect, r(radius), Stroke::new(1.4, Color32::from_rgb(0x0d, 0x0f, 0x10)), StrokeKind::Inside);
}

/// Draw the outer sunken dark bezel rim around a stage/preview after `mask_corners`.
pub fn monitor_bezel(painter: &Painter, rect: Rect, radius: u8) {
    let inset = (radius as f32 * 0.55).max(2.0);
    painter.line_segment(
        [egui::pos2(rect.left() + inset, rect.bottom() + 1.0), egui::pos2(rect.right() - inset, rect.bottom() + 1.0)],
        Stroke::new(1.0, BEVEL_HI),
    );
    painter.rect_stroke(rect, r(radius), Stroke::new(2.2, WELL), StrokeKind::Inside);
    painter.rect_stroke(rect.shrink(1.6), r(radius.saturating_sub(1)), Stroke::new(1.0, Color32::from_black_alpha(190)), StrokeKind::Inside);
}

/// Sunken brushed-silver LCD value box (`200`, `700`, `2500`, `8000`, `-18.0`).
pub fn lcd_readout(painter: &Painter, rect: Rect, text: &str, font_size: f32) {
    painter.rect_filled(rect, r(3), LCD_BG);
    painter.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.top() + 0.8), egui::pos2(rect.right() - 2.0, rect.top() + 0.8)],
        Stroke::new(1.2, LCD_SHADOW),
    );
    painter.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.6), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.6)],
        Stroke::new(1.0, BEVEL_HI),
    );
    painter.rect_stroke(rect, r(3), Stroke::new(1.0, Color32::from_rgb(0x66, 0x62, 0x54)), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, mono_medium(font_size), LCD_INK);
}

/// Sunken dark charcoal display slot (`STEREO`, `24 dB/Oct`).
pub fn sunken_display(painter: &Painter, rect: Rect, text: &str, font_size: f32) {
    painter.rect_filled(rect, r(3), SLOT_BG);
    painter.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.top() + 0.8), egui::pos2(rect.right() - 2.0, rect.top() + 0.8)],
        Stroke::new(1.2, Color32::from_black_alpha(190)),
    );
    painter.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.7), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.7)],
        Stroke::new(1.0, BEVEL_HI),
    );
    painter.rect_stroke(rect, r(3), Stroke::new(1.0, Color32::from_rgb(0x14, 0x13, 0x10)), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font_semibold(font_size), SLOT_INK);
}

/// Small round machined jewel indicator LED (green or ruby).
pub fn jewel_led(painter: &Painter, center: Pos2, rad: f32, on: bool, ruby: bool) {
    painter.circle_filled(center, rad + 1.3, Color32::from_rgb(0x3a, 0x36, 0x2a));
    let fill = match (ruby, on) {
        (true, true) => Color32::from_rgb(0xff, 0x58, 0x48),
        (true, false) => Color32::from_rgb(0x6e, 0x30, 0x2b),
        (false, true) => Color32::from_rgb(0x8c, 0xff, 0x5a),
        (false, false) => Color32::from_rgb(0x52, 0x63, 0x42),
    };
    if on {
        let glow = if ruby {
            Color32::from_rgba_unmultiplied(255, 88, 72, 70)
        } else {
            Color32::from_rgba_unmultiplied(140, 255, 90, 70)
        };
        painter.circle_filled(center, rad + 3.2, glow);
    }
    painter.circle_filled(center, rad, fill);
    painter.circle_filled(center - egui::vec2(rad * 0.28, rad * 0.28), rad * 0.30, Color32::from_white_alpha(if on { 190 } else { 85 }));
}

// ============================================================================
// Tactile Console Keycaps & Backlit Translucent Switches
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchKind {
    /// Smoked amber when off -> glowing incandescent amber-gold lamp when on (`SOLO`).
    Amber,
    /// Smoked ruby when off -> glowing incandescent ruby lamp when on (`MUTE` / `REC`).
    Ruby,
    /// Raised warm putty keycap when off -> glowing translucent mint-teal when on (`L R M S` / Views).
    PuttyMint,
    /// Raised warm putty keycap when off -> glowing incandescent amber when on.
    PuttyAmber,
    /// Matte dark charcoal rocker/push cap (`DIM`).
    DarkCap,
}

/// Paint a recessed hardware switch housing + cap (without text, so callers can
/// draw multi-line legends, icons, or custom typography).
pub fn paint_switch_cap(painter: &Painter, rect: Rect, kind: SwitchKind, on: bool, hovered: bool, pressed: bool) {
    // Soft directional shadow cast down-left onto the sand faceplate.
    painter.rect_filled(
        rect.translate(egui::vec2(-1.2, 2.0)),
        r(4),
        Color32::from_rgba_unmultiplied(26, 23, 15, 78),
    );
    // Outer black molded bezel housing (`#16191A`).
    painter.rect_filled(rect, r(4), WELL);
    painter.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.8), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.8)],
        Stroke::new(1.0, BEVEL_HI),
    );

    let pad = if rect.height() >= 38.0 { 2.8 } else { 2.0 };
    let mut cap = rect.shrink(pad);
    if pressed {
        cap = cap.translate(egui::vec2(0.0, 0.7));
    }
    if cap.width() <= 2.0 || cap.height() <= 2.0 {
        return;
    }

    match (kind, on) {
        (SwitchKind::Amber, true) | (SwitchKind::PuttyAmber, true) => {
            painter.rect_filled(cap, r(2), AMBER_EDGE);
            radial_lens(painter, cap.shrink(0.8), AMBER_CORE, AMBER_MID, AMBER_EDGE);
            let inner_rim = cap.shrink(2.2);
            if inner_rim.width() > 4.0 && inner_rim.height() > 4.0 {
                painter.rect_stroke(inner_rim, r(2), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 252, 210, 175)), StrokeKind::Inside);
            }
            painter.rect_stroke(cap, r(2), Stroke::new(1.0, Color32::from_rgb(0x9e, 0x76, 0x1e)), StrokeKind::Inside);
        }
        (SwitchKind::Amber, false) => {
            let mid = if hovered { Color32::from_rgb(0x8d, 0x78, 0x40) } else { AMBER_OFF };
            let core = if hovered { Color32::from_rgb(0x9c, 0x86, 0x49) } else { Color32::from_rgb(0x89, 0x75, 0x3f) };
            radial_lens(painter, cap, core, mid, AMBER_OFF_EDGE);
            let inner_rim = cap.shrink(2.2);
            if inner_rim.width() > 4.0 && inner_rim.height() > 4.0 {
                painter.rect_stroke(inner_rim, r(2), Stroke::new(1.0, Color32::from_rgb(0x98, 0x82, 0x48)), StrokeKind::Inside);
            }
            painter.rect_stroke(cap, r(2), Stroke::new(1.0, Color32::from_rgb(0x42, 0x36, 0x18)), StrokeKind::Inside);
        }
        (SwitchKind::Ruby, true) => {
            painter.rect_filled(cap, r(2), RUBY_EDGE);
            radial_lens(painter, cap.shrink(0.8), RUBY_CORE, RUBY_MID, RUBY_EDGE);
            let inner_rim = cap.shrink(2.2);
            if inner_rim.width() > 4.0 && inner_rim.height() > 4.0 {
                painter.rect_stroke(inner_rim, r(2), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 210, 198, 165)), StrokeKind::Inside);
            }
            painter.rect_stroke(cap, r(2), Stroke::new(1.0, Color32::from_rgb(0x7c, 0x1d, 0x16)), StrokeKind::Inside);
        }
        (SwitchKind::Ruby, false) => {
            let mid = if hovered { Color32::from_rgb(0x8c, 0x45, 0x3d) } else { RUBY_OFF };
            let core = if hovered { Color32::from_rgb(0x9a, 0x4e, 0x45) } else { Color32::from_rgb(0x86, 0x42, 0x3a) };
            radial_lens(painter, cap, core, mid, RUBY_OFF_EDGE);
            let inner_rim = cap.shrink(2.2);
            if inner_rim.width() > 4.0 && inner_rim.height() > 4.0 {
                painter.rect_stroke(inner_rim, r(2), Stroke::new(1.0, Color32::from_rgb(0x94, 0x4c, 0x44)), StrokeKind::Inside);
            }
            painter.rect_stroke(cap, r(2), Stroke::new(1.0, Color32::from_rgb(0x42, 0x1b, 0x17)), StrokeKind::Inside);
        }
        (SwitchKind::PuttyMint, true) => {
            painter.rect_filled(cap, r(2), MINT_EDGE);
            radial_lens(painter, cap.shrink(0.8), MINT_CORE, MINT_MID, MINT_EDGE);
            let inner_rim = cap.shrink(2.0);
            if inner_rim.width() > 4.0 && inner_rim.height() > 4.0 {
                painter.rect_stroke(inner_rim, r(2), Stroke::new(1.0, Color32::from_rgba_unmultiplied(215, 255, 236, 175)), StrokeKind::Inside);
            }
            painter.rect_stroke(cap, r(2), Stroke::new(1.0, Color32::from_rgb(0x28, 0x66, 0x4b)), StrokeKind::Inside);
        }
        (SwitchKind::PuttyMint, false) | (SwitchKind::PuttyAmber, false) => {
            let face = if pressed {
                Color32::from_rgb(0x9e, 0x99, 0x8c)
            } else if hovered {
                G3
            } else {
                PUTTY
            };
            painter.rect_filled(cap, r(3), PUTTY_LO);
            let top_cap = Rect::from_min_max(cap.min, egui::pos2(cap.right(), (cap.bottom() - 1.5).max(cap.top() + 2.0)));
            painter.rect_filled(top_cap, r(3), face);
            painter.line_segment(
                [egui::pos2(top_cap.left() + 2.0, top_cap.top() + 0.8), egui::pos2(top_cap.right() - 2.0, top_cap.top() + 0.8)],
                Stroke::new(1.0, PUTTY_HI),
            );
        }
        (SwitchKind::DarkCap, _) => {
            let face = if on {
                Color32::from_rgb(0x34, 0x37, 0x37)
            } else if hovered {
                Color32::from_rgb(0x2e, 0x31, 0x31)
            } else {
                Color32::from_rgb(0x25, 0x28, 0x28)
            };
            painter.rect_filled(cap, r(3), Color32::from_rgb(0x11, 0x13, 0x13));
            let top_cap = Rect::from_min_max(cap.min, egui::pos2(cap.right(), (cap.bottom() - 1.8).max(cap.top() + 2.0)));
            painter.rect_filled(top_cap, r(3), face);
            painter.line_segment(
                [egui::pos2(top_cap.left() + 2.0, top_cap.top() + 0.8), egui::pos2(top_cap.right() - 2.0, top_cap.top() + 0.8)],
                Stroke::new(1.0, Color32::from_rgb(0x4b, 0x4f, 0x4f)),
            );
        }
    }
}

/// Interactive hardware backlit push-button (supports 1-line or `\n` 2-line silkscreened legend).
pub fn hw_switch(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    a11y_label: &str,
    kind: SwitchKind,
    on: bool,
    font_size: f32,
) -> Response {
    let resp = hit(ui, rect, id, Sense::click(), a11y_label);
    let p = ui.painter();
    paint_switch_cap(p, rect, kind, on, resp.hovered(), resp.is_pointer_button_down_on());
    let fg = match (kind, on) {
        (SwitchKind::DarkCap, _) => CREAM,
        (SwitchKind::PuttyMint, true) => Color32::from_rgb(0x13, 0x2e, 0x21),
        (_, true) => ON_HI,
        (SwitchKind::Amber, false) => Color32::from_rgb(0x24, 0x1c, 0x09),
        (SwitchKind::Ruby, false) => Color32::from_rgb(0x24, 0x0f, 0x0c),
        _ => INK,
    };
    if let Some((l1, l2)) = label.split_once('\n') {
        let dy = (font_size * 0.58).max(5.5);
        p.text(rect.center() - egui::vec2(0.0, dy), Align2::CENTER_CENTER, l1, font_semibold(font_size), fg);
        p.text(rect.center() + egui::vec2(0.0, dy), Align2::CENTER_CENTER, l2, font_semibold(font_size), fg);
    } else {
        p.text(rect.center(), Align2::CENTER_CENTER, label, font_semibold(font_size), fg);
    }
    resp
}

/// Raised putty keycap surface (`D · Hybrid` compatible helper).
pub fn raised(painter: &Painter, rect: Rect, radius: u8, fill: Color32) {
    let rad = radius.min(6);
    painter.rect_filled(
        rect.translate(egui::vec2(-0.8, 1.5)),
        r(rad),
        Color32::from_rgba_unmultiplied(28, 24, 16, 62),
    );
    painter.rect_filled(rect, r(rad), WELL);
    let cap = rect.shrink(1.6);
    if cap.width() > 2.0 && cap.height() > 2.0 {
        painter.rect_filled(cap, r(rad.saturating_sub(1)), PUTTY_LO);
        let top = Rect::from_min_max(cap.min, egui::pos2(cap.right(), (cap.bottom() - 1.2).max(cap.top() + 1.0)));
        painter.rect_filled(top, r(rad.saturating_sub(1)), fill);
        let inset = (rad as f32 * 0.5).max(1.5);
        painter.line_segment(
            [egui::pos2(top.left() + inset, top.top() + 0.7), egui::pos2(top.right() - inset, top.top() + 0.7)],
            Stroke::new(1.0, PUTTY_HI),
        );
    }
}

/// An interactive rect with an accessibility label (so tests and screen
/// readers can find custom-painted controls).
pub fn hit(ui: &mut Ui, rect: Rect, id: egui::Id, sense: Sense, label: &str) -> Response {
    let resp = ui.interact(rect, id, sense);
    let l = label.to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &l));
    resp
}

/// Press-and-hold detection (touch-first: no right-click needed).
pub struct Hold {
    pub fired: bool,
    pub held_before: bool,
}

pub fn hold(ui: &Ui, resp: &Response, secs: f64) -> Hold {
    let id = resp.id.with("hold");
    let now = ui.input(|i| i.time);
    let down = resp.is_pointer_button_down_on();
    let moved = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()).map(|(a, b)| (a - b).length()).unwrap_or(0.0)) > 6.0;
    let (start, done): (Option<f64>, bool) = ui.data(|d| d.get_temp(id)).unwrap_or((None, false));
    let mut out = Hold { fired: false, held_before: done };
    if down && !moved {
        let t0 = start.unwrap_or(now);
        if !done && now - t0 >= secs {
            out.fired = true;
            ui.data_mut(|d| d.insert_temp(id, (Some(t0), true)));
        } else {
            ui.data_mut(|d| d.insert_temp(id, (Some(t0), done)));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    } else if !down {
        ui.data_mut(|d| d.insert_temp(id, (None::<f64>, false)));
    } else {
        ui.data_mut(|d| d.insert_temp(resp.id.with("hold"), (None::<f64>, done)));
    }
    out
}

/// Segmented hardware console bank: recessed black housing holding side-by-side
/// putty keycaps that illuminate in incandescent Amber (`thumb_hi = true`) or
/// glowing Mint-Teal (`thumb_hi = false`) when active!
#[allow(clippy::too_many_arguments)]
pub fn segmented(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    labels: &[&str],
    hints: &[&str],
    selected: usize,
    font: FontId,
    thumb_hi: bool,
) -> Option<usize> {
    segmented_named(ui, rect, id, labels, labels, hints, selected, font, thumb_hi)
}

/// [`segmented`] with separate accessibility names per segment.
#[allow(clippy::too_many_arguments)]
pub fn segmented_named(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    labels: &[&str],
    names: &[&str],
    hints: &[&str],
    selected: usize,
    font: FontId,
    thumb_hi: bool,
) -> Option<usize> {
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect.translate(egui::vec2(-1.0, 1.8)),
        r(5),
        Color32::from_rgba_unmultiplied(24, 21, 14, 70),
    );
    painter.rect_filled(rect, r(5), WELL);
    painter.line_segment(
        [egui::pos2(rect.left() + 3.0, rect.bottom() + 0.8), egui::pos2(rect.right() - 3.0, rect.bottom() + 0.8)],
        Stroke::new(1.0, BEVEL_HI),
    );

    let n = labels.len().max(1);
    let inner = rect.shrink(2.5);
    let gap = 2.0;
    let w = ((inner.width() - gap * (n.saturating_sub(1) as f32)) / n as f32).max(8.0);
    let mut picked = None;
    let switch_kind = if thumb_hi { SwitchKind::PuttyAmber } else { SwitchKind::PuttyMint };

    for (i, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(
            egui::pos2(inner.left() + i as f32 * (w + gap), inner.top()),
            egui::vec2(w, inner.height()),
        );
        let resp = hit(ui, cell, id.with(i), Sense::click(), names.get(i).copied().unwrap_or(label));
        if resp.clicked() {
            picked = Some(i);
        }
        let on = i == selected;
        let pressed = resp.is_pointer_button_down_on();
        let mut cap = cell;
        if pressed {
            cap = cap.translate(egui::vec2(0.0, 0.6));
        }
        match (switch_kind, on) {
            (SwitchKind::PuttyAmber, true) => {
                painter.rect_filled(cap, r(3), AMBER_EDGE);
                radial_lens(&painter, cap.shrink(0.6), AMBER_CORE, AMBER_MID, AMBER_EDGE);
                painter.rect_stroke(cap, r(3), Stroke::new(1.0, Color32::from_rgb(0x9e, 0x76, 0x1e)), StrokeKind::Inside);
            }
            (SwitchKind::PuttyMint, true) => {
                painter.rect_filled(cap, r(3), MINT_EDGE);
                radial_lens(&painter, cap.shrink(0.6), MINT_CORE, MINT_MID, MINT_EDGE);
                painter.rect_stroke(cap, r(3), Stroke::new(1.0, Color32::from_rgb(0x28, 0x66, 0x4b)), StrokeKind::Inside);
            }
            _ => {
                let face = if resp.hovered() { G3 } else { PUTTY };
                painter.rect_filled(cap, r(3), PUTTY_LO);
                let top_c = Rect::from_min_max(cap.min, egui::pos2(cap.right(), (cap.bottom() - 1.3).max(cap.top() + 1.0)));
                painter.rect_filled(top_c, r(3), face);
                painter.line_segment(
                    [egui::pos2(top_c.left() + 2.0, top_c.top() + 0.7), egui::pos2(top_c.right() - 2.0, top_c.top() + 0.7)],
                    Stroke::new(1.0, PUTTY_HI),
                );
            }
        }

        let col = if on {
            if thumb_hi { ON_HI } else { Color32::from_rgb(0x12, 0x2d, 0x20) }
        } else if resp.hovered() {
            INK
        } else {
            INK2
        };
        let hint = hints.get(i).copied().unwrap_or("");
        if hint.is_empty() {
            painter.text(cell.center(), Align2::CENTER_CENTER, *label, font.clone(), col);
        } else {
            let hint_col = if on { Color32::from_black_alpha(120) } else { INK4 };
            runs(
                &painter,
                cell.center(),
                Align2::CENTER_CENTER,
                &[(label, font.clone(), col), ("  ", font.clone(), col), (hint, mono(9.0), hint_col)],
            );
        }
    }
    picked
}

/// Hardware console keycap / backlit chip.
pub fn chip(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, ghost: bool, on: bool) -> Response {
    let resp = hit(ui, rect, id, Sense::click_and_drag(), label);
    let p = ui.painter();
    if on {
        paint_switch_cap(p, rect, SwitchKind::Amber, true, resp.hovered(), resp.is_pointer_button_down_on());
    } else if !ghost {
        paint_switch_cap(p, rect, SwitchKind::PuttyAmber, false, resp.hovered(), resp.is_pointer_button_down_on());
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        paint_switch_cap(p, rect, SwitchKind::PuttyAmber, false, true, resp.is_pointer_button_down_on());
    }
    resp
}

/// Primary / secondary hardware push-button with an icon painter and a label.
pub fn button(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, primary: bool, icon: Option<Icon>, kbd: Option<&str>) -> Response {
    button_named(ui, rect, id, label, label, primary, icon, kbd)
}

/// [`button`] with an accessibility name other than its label.
#[allow(clippy::too_many_arguments)]
pub fn button_named(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, name: &str, primary: bool, icon: Option<Icon>, kbd: Option<&str>) -> Response {
    let resp = hit(ui, rect, id, Sense::click(), name);
    let p = ui.painter().clone();
    let pressed = resp.is_pointer_button_down_on();
    paint_switch_cap(
        &p,
        rect,
        if primary { SwitchKind::Amber } else { SwitchKind::PuttyAmber },
        primary,
        resp.hovered(),
        pressed,
    );
    let fg = if primary { ON_HI } else { INK };
    let fnt = font_semibold(12.5);
    let galley = p.layout_no_wrap(label.to_string(), fnt, fg);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let kbd_w = if kbd.is_some() { 16.0 } else { 0.0 };
    let total = icon_w + galley.size().x + kbd_w;
    let mut x = rect.center().x - total / 2.0;
    if let Some(ic) = icon {
        draw_icon(&p, ic, egui::pos2(x + 7.5, rect.center().y), 13.5, fg);
        x += icon_w;
    }
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley.clone(), fg);
    x += galley.size().x;
    if let Some(k) = kbd {
        p.text(
            egui::pos2(x + 10.0, rect.center().y),
            Align2::CENTER_CENTER,
            k,
            mono(9.0),
            if primary { Color32::from_black_alpha(130) } else { INK3 },
        );
    }
    resp
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Icon {
    Play,
    Stop,
    Deep,
    Export,
    Fullscreen,
    More,
    Plus,
    Brand,
    Square,
    Loop,
    Cloud,
    Chop,
    Display2,
    Folder,
    Dig,
    Link,
    Grid,
    Tile,
    Stack,
    ChevronLeft,
    ChevronRight,
}

/// Line icons drawn as shapes (no icon font needed).
pub fn draw_icon(p: &Painter, icon: Icon, c: Pos2, size: f32, col: Color32) {
    let s = size / 24.0;
    let pt = |x: f32, y: f32| egui::pos2(c.x + (x - 12.0) * s, c.y + (y - 12.0) * s);
    let st = Stroke::new(1.6 * s.max(0.6), col);
    match icon {
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![pt(8.0, 5.5), pt(19.0, 12.0), pt(8.0, 18.5)], col, Stroke::NONE));
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_min_max(pt(7.0, 7.0), pt(17.0, 17.0)), r(2), col);
        }
        Icon::Square => {
            p.rect_stroke(Rect::from_min_max(pt(5.5, 5.5), pt(18.5, 18.5)), r(1), Stroke::new(1.5, col), StrokeKind::Middle);
        }
        Icon::Deep => {
            for (y, x1) in [(7.0, 20.0), (12.0, 14.0), (17.0, 10.0)] {
                p.line_segment([pt(4.0, y), pt(x1, y)], st);
            }
        }
        Icon::Export => {
            p.line_segment([pt(12.0, 4.0), pt(12.0, 15.0)], st);
            p.line_segment([pt(7.5, 9.0), pt(12.0, 4.0)], st);
            p.line_segment([pt(16.5, 9.0), pt(12.0, 4.0)], st);
            p.add(Shape::line(vec![pt(5.0, 15.0), pt(5.0, 19.0), pt(19.0, 19.0), pt(19.0, 15.0)], st));
        }
        Icon::Fullscreen => {
            for (a, b, cc) in [
                ((4.0, 9.0), (4.0, 4.0), (9.0, 4.0)),
                ((15.0, 4.0), (20.0, 4.0), (20.0, 9.0)),
                ((4.0, 15.0), (4.0, 20.0), (9.0, 20.0)),
                ((15.0, 20.0), (20.0, 20.0), (20.0, 15.0)),
            ] {
                p.add(Shape::line(vec![pt(a.0, a.1), pt(b.0, b.1), pt(cc.0, cc.1)], st));
            }
        }
        Icon::More => {
            for x in [6.0, 12.0, 18.0] {
                p.circle_filled(pt(x, 12.0), 1.7 * s, col);
            }
        }
        Icon::Plus => {
            p.line_segment([pt(12.0, 6.0), pt(12.0, 18.0)], st);
            p.line_segment([pt(6.0, 12.0), pt(18.0, 12.0)], st);
        }
        Icon::Brand => {
            for i in 0..3 {
                for j in 0..3 {
                    let o = pt(5.0 + i as f32 * 5.5, 5.0 + j as f32 * 5.5);
                    p.rect_filled(Rect::from_min_size(o, egui::vec2(3.6 * s, 3.6 * s)), r(1), col);
                }
            }
        }
        Icon::Loop => {
            p.circle_stroke(pt(8.5, 12.0), 3.5 * s, st);
            p.circle_stroke(pt(15.5, 12.0), 3.5 * s, st);
        }
        Icon::Cloud => {
            for (x, y) in [(8.0, 9.0), (15.0, 8.0), (11.0, 14.0), (17.0, 15.0), (7.0, 17.0)] {
                p.circle_filled(pt(x, y), 1.3 * s, col);
            }
        }
        Icon::Chop => {
            for x in [8.0, 12.0, 16.0] {
                p.line_segment([pt(x, 6.0), pt(x, 18.0)], st);
            }
        }
        Icon::Display2 => {
            p.rect_stroke(Rect::from_min_max(pt(2.0, 4.0), pt(15.0, 13.0)), r(1), st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(9.0, 10.0), pt(22.0, 19.0)), r(1), st, StrokeKind::Middle);
        }
        Icon::Folder => {
            p.add(Shape::closed_line(vec![pt(3.5, 7.0), pt(9.5, 7.0), pt(11.5, 9.0), pt(20.5, 9.0), pt(20.5, 18.5), pt(3.5, 18.5)], st));
        }
        Icon::Dig => {
            p.add(Shape::closed_line(vec![pt(8.0, 12.0), pt(16.0, 12.0), pt(16.0, 16.0), pt(12.0, 20.5), pt(8.0, 16.0)], st));
            p.line_segment([pt(12.0, 12.0), pt(12.0, 3.5)], st);
            p.line_segment([pt(9.5, 3.5), pt(14.5, 3.5)], st);
        }
        Icon::Link => {
            p.add(Shape::line(vec![pt(10.5, 8.0), pt(13.0, 5.5), pt(15.5, 5.5), pt(18.5, 8.5), pt(18.5, 11.0), pt(16.0, 13.5)], st));
            p.add(Shape::line(vec![pt(13.5, 16.0), pt(11.0, 18.5), pt(8.5, 18.5), pt(5.5, 15.5), pt(5.5, 13.0), pt(8.0, 10.5)], st));
            p.line_segment([pt(9.5, 14.5), pt(14.5, 9.5)], st);
        }
        Icon::Grid => {
            for (x, y) in [(4.0, 4.0), (13.0, 4.0), (4.0, 13.0), (13.0, 13.0)] {
                p.rect_stroke(Rect::from_min_size(pt(x, y), egui::vec2(7.0 * s, 7.0 * s)), r(1), st, StrokeKind::Middle);
            }
        }
        Icon::Tile => {
            p.rect_stroke(Rect::from_min_max(pt(3.5, 5.5), pt(11.2, 11.2)), r(1), st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(12.8, 5.5), pt(20.5, 11.2)), r(1), st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(3.5, 12.8), pt(11.2, 18.5)), r(1), st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(12.8, 12.8), pt(20.5, 18.5)), r(1), st, StrokeKind::Middle);
        }
        Icon::Stack => {
            p.rect_stroke(Rect::from_min_max(pt(7.0, 4.0), pt(20.0, 13.0)), r(1), st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(4.0, 8.0), pt(17.0, 17.0)), r(1), st, StrokeKind::Middle);
        }
        Icon::ChevronLeft => {
            p.add(Shape::line(vec![pt(14.5, 6.0), pt(8.5, 12.0), pt(14.5, 18.0)], st));
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![pt(9.5, 6.0), pt(15.5, 12.0), pt(9.5, 18.0)], st));
        }
    }
}

/// Numbered emitter ring over dark footage tiles (`selected` glows incandescent amber).
pub fn emitter_ring(p: &Painter, c: Pos2, rad: f32, n: usize, selected: bool, active: bool, t: f64) {
    emitter_ring_pitch(p, c, rad, n, selected, active, t, None);
}

#[allow(clippy::too_many_arguments)]
pub fn emitter_ring_pitch(
    p: &Painter,
    c: Pos2,
    rad: f32,
    n: usize,
    selected: bool,
    active: bool,
    t: f64,
    pitch_macro: Option<f32>,
) {
    if active {
        for k in 0..3 {
            let ph = ((t * 1.6 + k as f64 / 3.0).fract()) as f32;
            let rr = rad + 4.0 + ph * rad * 1.6;
            let a = ((1.0 - ph) * 140.0) as u8;
            p.circle_stroke(c, rr, Stroke::new(1.3, Color32::from_rgba_unmultiplied(252, 227, 93, a)));
        }
    }
    p.circle_filled(c, rad + 5.5, Color32::from_black_alpha(if selected { 125 } else { 95 }));
    let outer_r = if selected { rad + 4.0 } else { rad };
    if selected {
        p.circle_stroke(c, outer_r, Stroke::new(1.3, AMBER_CORE));
        p.circle_filled(c, rad, AMBER_MID);
        p.circle_filled(c, rad * 0.55, AMBER_CORE);
        p.text(c, Align2::CENTER_CENTER, n.to_string(), mono_medium(rad * 0.85), ON_HI);
    } else {
        p.circle_filled(c, rad, Color32::from_black_alpha(150));
        p.circle_stroke(c, rad, Stroke::new(1.6, CREAM));
        p.text(c, Align2::CENTER_CENTER, n.to_string(), mono(rad * 0.85), CREAM);
    }
    if let Some(pm) = pitch_macro {
        let ha = ((pm.clamp(0.0, 1.0) - 0.5) * 2.0) * 0.75 * PI;
        let dot = egui::pos2(c.x + ha.sin() * outer_r, c.y - ha.cos() * outer_r);
        p.circle_filled(dot, 3.5, if selected { AMBER_MID } else { CREAM });
        p.circle_stroke(dot, 3.5, Stroke::new(1.2, WELL));
    }
}

fn arc_points(c: Pos2, rad: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = ((a1 - a0).abs() / TAU * 96.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            egui::pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect()
}

/// Physical machined dark charcoal cylindrical pointer knob (`ISOL8` style):
/// directional drop shadow onto the sand chassis, silkscreened scale ticks,
/// dark tapered skirt `#181919`, matte top cap `#333434 -> #212222` with top
/// rim catchlight, and crisp white painted radial pointer line `#F4F2EC`.
#[allow(clippy::too_many_arguments)]
pub fn knob(ui: &mut Ui, center: Pos2, rad: f32, id: egui::Id, label: &str, value: &mut f32, default: f32, detents: usize) -> Response {
    let rect = Rect::from_center_size(center, egui::vec2(rad * 2.0 + 12.0, rad * 2.0 + 12.0));
    let resp = hit(ui, rect, id, Sense::click_and_drag(), label);
    if resp.dragged() {
        let d = -resp.drag_delta().y / 220.0 + resp.drag_delta().x / 600.0;
        *value = (*value + d).clamp(0.0, 1.0);
    } else if detents > 1 && resp.clicked() && !resp.double_clicked() {
        let k = ((*value * detents as f32) as usize).min(detents - 1);
        let next = (k + 1) % detents;
        *value = (next as f32 + 0.5) / detents as f32;
    }
    if resp.double_clicked() {
        *value = default;
    }
    let p = ui.painter();
    let a0 = PI * 0.75;
    let span = PI * 1.5;
    let v = if detents > 1 {
        let k = ((*value * detents as f32) as usize).min(detents - 1);
        k as f32 / (detents - 1) as f32
    } else {
        *value
    };

    // 1. Silkscreened calibration ticks around the sweep on the metal faceplate.
    let n_ticks = if detents > 1 { detents } else { 9 };
    for k in 0..n_ticks {
        let frac = k as f32 / (n_ticks.saturating_sub(1).max(1)) as f32;
        let a = a0 + span * frac;
        let (s, cth) = (a.sin(), a.cos());
        let major = k == 0 || k + 1 == n_ticks || (n_ticks % 2 == 1 && k == n_ticks / 2);
        let r0 = rad * 0.90;
        let r1 = rad * if major { 1.04 } else { 0.98 };
        p.line_segment(
            [egui::pos2(center.x + r0 * cth, center.y + r0 * s), egui::pos2(center.x + r1 * cth, center.y + r1 * s)],
            Stroke::new(if major { 1.3 } else { 1.0 }, INK3),
        );
    }

    // 2. Soft directional cast shadow (down-left, matching ISOL8 lighting).
    let skirt_r = rad * 0.84;
    p.circle_filled(
        center + egui::vec2(-2.6, 3.8),
        skirt_r + 1.2,
        Color32::from_rgba_unmultiplied(24, 21, 14, 72),
    );
    p.circle_filled(
        center + egui::vec2(-1.1, 1.8),
        skirt_r + 0.4,
        Color32::from_rgba_unmultiplied(18, 16, 10, 118),
    );

    // 3. Outer machined charcoal skirt.
    p.circle_filled(center, skirt_r, Color32::from_rgb(0x18, 0x19, 0x19));
    p.circle_stroke(center, skirt_r, Stroke::new(1.0, Color32::from_rgb(0x0c, 0x0d, 0x0d)));

    // 4. Matte charcoal top cap (vertical gradient mesh + top specular rim).
    let cap_r = rad * 0.70;
    let mut mesh = egui::Mesh::default();
    let segs = 40;
    let top_col = if resp.hovered() || resp.dragged() {
        Color32::from_rgb(0x3c, 0x3d, 0x3d)
    } else {
        Color32::from_rgb(0x34, 0x35, 0x35)
    };
    let bot_col = Color32::from_rgb(0x20, 0x21, 0x21);
    let lerp = |t: f32| -> Color32 {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)) as u8;
        Color32::from_rgb(f(top_col.r(), bot_col.r()), f(top_col.g(), bot_col.g()), f(top_col.b(), bot_col.b()))
    };
    mesh.colored_vertex(center, lerp(0.5));
    for i in 0..=segs {
        let a = TAU * i as f32 / segs as f32;
        let pt = egui::pos2(center.x + cap_r * a.cos(), center.y + cap_r * a.sin());
        mesh.colored_vertex(pt, lerp((pt.y - (center.y - cap_r)) / (2.0 * cap_r)));
    }
    for i in 0..segs as u32 {
        mesh.add_triangle(0, i + 1, i + 2);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(
        arc_points(center, cap_r, PI * 1.12, PI * 1.88),
        Stroke::new(1.1, Color32::from_rgb(0x52, 0x53, 0x51)),
    ));
    p.circle_stroke(center, cap_r, Stroke::new(0.8, Color32::from_black_alpha(120)));

    // 5. Crisp white painted radial pointer line extending to the cap rim.
    let a = a0 + span * v;
    let (s, cth) = (a.sin(), a.cos());
    let ptr_w = (rad * 0.125).clamp(2.0, 3.2);
    p.line_segment(
        [
            egui::pos2(center.x + cap_r * 0.22 * cth, center.y + cap_r * 0.22 * s),
            egui::pos2(center.x + cap_r * 0.94 * cth, center.y + cap_r * 0.94 * s),
        ],
        Stroke::new(ptr_w, Color32::from_rgb(0xf4, 0xf2, 0xec)),
    );
    resp
}

/// Paint `bg` over the corners of `rect` (so square GPU output looks rounded).
pub fn mask_corners(p: &Painter, rect: Rect, rad: f32, bg: Color32) {
    for (corner, a0) in [
        (rect.left_top(), PI),
        (rect.right_top(), PI * 1.5),
        (rect.right_bottom(), 0.0),
        (rect.left_bottom(), PI * 0.5),
    ] {
        let sx = if corner.x > rect.center().x { -1.0 } else { 1.0 };
        let sy = if corner.y > rect.center().y { -1.0 } else { 1.0 };
        let c = egui::pos2(corner.x + sx * rad, corner.y + sy * rad);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(corner, bg);
        let pts = arc_points(c, rad + 0.5, a0, a0 + PI * 0.5);
        for pt in &pts {
            mesh.colored_vertex(*pt, bg);
        }
        for i in 0..pts.len() as u32 - 1 {
            mesh.add_triangle(0, i + 1, i + 2);
        }
        p.add(Shape::mesh(mesh));
    }
}

/// Hardware fader slot (`slider`).
pub fn slider(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, value: &mut f32, lo: f32, hi: f32) -> bool {
    let resp = hit(ui, rect.expand2(Vec2::new(8.0, 10.0)), id, Sense::click_and_drag(), label);
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.is_pointer_button_down_on() || resp.dragged() {
            let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let nv = lo + (hi - lo) * t;
            if (nv - *value).abs() > 1e-6 {
                *value = nv;
                changed = true;
            }
        }
    }
    let p = ui.painter();
    let y = rect.center().y;
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let x = rect.left() + rect.width() * t;
    let track = Rect::from_min_max(egui::pos2(rect.left(), y - 2.0), egui::pos2(rect.right(), y + 2.0));
    p.rect_filled(track, r(2), WELL);
    let lit = Rect::from_min_max(egui::pos2(rect.left(), y - 1.5), egui::pos2(x, y + 1.5));
    p.rect_filled(lit, r(2), AMBER_MID);
    let cap = Rect::from_center_size(egui::pos2(x, y), egui::vec2(10.0, 16.0));
    p.rect_filled(cap, r(2), Color32::from_rgb(0x26, 0x28, 0x28));
    p.line_segment([egui::pos2(x, cap.top() + 2.0), egui::pos2(x, cap.bottom() - 2.0)], Stroke::new(1.5, CREAM));
    changed
}

/// Compact mini ADSR polyline over dark footage tiles / monitor wells.
pub fn mini_adsr(p: &Painter, rect: Rect, adsr: &chromagrain_core::envelope::Adsr, alpha: f32) {
    if rect.width() < 14.0 || rect.height() < 6.0 {
        return;
    }
    let pts = crate::envelope::points(rect, adsr);
    let a = (alpha.clamp(0.0, 1.0) * 240.0) as u8;
    p.add(Shape::line(pts.to_vec(), Stroke::new(1.5, Color32::from_rgba_unmultiplied(252, 227, 93, a))));
    if adsr.picture == chromagrain_core::envelope::PictureCut::Cut {
        let ty = rect.bottom() - adsr.cut_threshold.clamp(0.0, 1.0) * rect.height();
        let x_end = pts[4].x.max(rect.left() + 8.0);
        let da = (alpha.clamp(0.0, 1.0) * 115.0) as u8;
        let mut x = rect.left();
        while x < x_end {
            let x1 = (x + 2.0).min(x_end);
            p.line_segment([egui::pos2(x, ty), egui::pos2(x1, ty)], Stroke::new(1.0, Color32::from_white_alpha(da)));
            x += 4.5;
        }
    }
}

/// Computed geometry of an on-object grain ring (`D · Hybrid`).
#[derive(Debug, Clone, Copy)]
pub struct RingGeo {
    pub center: Pos2,
    pub rr: f32,
    pub r_max: f32,
    pub r_min: f32,
    pub pitch_pos: Pos2,
}

impl RingGeo {
    pub fn of(rect: Rect, grain: f32, pitch_macro: f32, small: bool) -> Self {
        let span = rect.width().min(rect.height());
        let r_max = span * if small { 0.31 } else { 0.34 };
        let r_min = (span * 0.11).max(7.0);
        let g = grain.clamp(0.0, 1.0);
        let rr = r_max + (r_min - r_max) * g;
        let center = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.48);
        let ha = ((pitch_macro.clamp(0.0, 1.0) - 0.5) * 2.0) * 0.75 * PI;
        let pitch_pos = egui::pos2(center.x + ha.sin() * rr, center.y - ha.cos() * rr);
        Self { center, rr, r_max, r_min, pitch_pos }
    }
}

/// Draw the on-object grain ring + golden-angle cloud dots + pitch dot +
/// optional smear trail over dark footage tiles.
#[allow(clippy::too_many_arguments)]
pub fn draw_on_object_ring(
    p: &Painter,
    rect: Rect,
    grain: f32,
    pitch_macro: f32,
    pitch_st: f32,
    smear: f32,
    small: bool,
) -> RingGeo {
    let geo = RingGeo::of(rect, grain, pitch_macro, small);
    if rect.width() < 30.0 || rect.height() < 30.0 {
        return geo;
    }
    let g = grain.clamp(0.0, 1.0);
    p.circle_stroke(geo.center, geo.rr, Stroke::new(1.5, Color32::from_rgba_unmultiplied(243, 240, 230, 225)));
    let n = (1.0 + 15.0 * g).round() as usize;
    let dot_r = 3.4 + (1.4 - 3.4) * g;
    for k in 0..n {
        let a = k as f32 * 2.39996;
        let rad = geo.rr * 0.74 * ((k as f32 + 0.5) / n as f32).sqrt();
        let pt = egui::pos2(geo.center.x + a.cos() * rad, geo.center.y + a.sin() * rad);
        p.circle_filled(pt, if small { dot_r * 0.8 } else { dot_r }, Color32::from_rgba_unmultiplied(252, 227, 93, 195));
    }
    if !small {
        let tl = smear.clamp(0.0, 1.0) * rect.width() * 0.25;
        if tl > 2.0 {
            for k in 0..3 {
                let off = k as f32 - 1.0;
                let yy = geo.center.y + off * geo.rr * 0.45;
                let dx = (geo.rr * geo.rr - (yy - geo.center.y).powi(2)).max(0.0).sqrt();
                let x0 = geo.center.x + dx + 4.0;
                let x1 = x0 + tl * (1.0 - off.abs() * 0.35);
                let alpha = ((0.60 - off.abs() * 0.2) * 255.0) as u8;
                p.line_segment([egui::pos2(x0, yy), egui::pos2(x1, yy)], Stroke::new(1.5, Color32::from_rgba_unmultiplied(252, 227, 93, alpha)));
            }
        }
    }
    p.line_segment(
        [egui::pos2(geo.center.x, geo.center.y - geo.rr - 4.5), egui::pos2(geo.center.x, geo.center.y - geo.rr + 2.5)],
        Stroke::new(1.4, Color32::from_white_alpha(120)),
    );
    let pr = if small { 4.8 } else { 6.2 };
    p.circle_filled(geo.pitch_pos, pr, AMBER_MID);
    p.circle_stroke(geo.pitch_pos, pr, Stroke::new(1.8, WELL));
    if !small && pitch_st.abs() >= 0.5 {
        let tag_y = (geo.pitch_pos.y - 12.0).max(rect.top() + 9.0);
        p.text(egui::pos2(geo.pitch_pos.x, tag_y), Align2::CENTER_CENTER, format!("{pitch_st:+.0}"), mono_medium(10.0), AMBER_CORE);
    }
    geo
}

/// Primary backlit amber `Open clip ›` switch used in contextual object bars.
pub fn open_clip_pill(ui: &mut Ui, rect: Rect, id: egui::Id, a11y_label: &str) -> Response {
    let resp = hit(ui, rect, id, Sense::click(), a11y_label);
    let p = ui.painter();
    paint_switch_cap(p, rect, SwitchKind::Amber, true, resp.hovered(), resp.is_pointer_button_down_on());
    let text_galley = p.layout_no_wrap("Open clip".into(), font_semibold(11.5), ON_HI);
    let total_w = text_galley.size().x + 14.0;
    let x0 = rect.center().x - total_w * 0.5;
    p.galley(egui::pos2(x0, rect.center().y - text_galley.size().y * 0.5), text_galley.clone(), ON_HI);
    draw_icon(p, Icon::ChevronRight, egui::pos2(x0 + text_galley.size().x + 8.5, rect.center().y + 0.5), 11.0, ON_HI);
    resp
}

/// Small dark rounded `›` button on Perform pads and Sequence lane headers.
pub fn chev_button(ui: &mut Ui, vis_rect: Rect, hit_rect: Rect, id: egui::Id, a11y_label: &str) -> Response {
    let resp = hit(ui, hit_rect, id, Sense::click(), a11y_label);
    let p = ui.painter();
    let hovered = resp.hovered() || resp.is_pointer_button_down_on();
    p.rect_filled(vis_rect, r(4), Color32::from_black_alpha(if hovered { 225 } else { 175 }));
    p.rect_stroke(vis_rect, r(4), hair(if hovered { AMBER_MID } else { WELL_LINE2 }), StrokeKind::Inside);
    draw_icon(p, Icon::ChevronRight, vis_rect.center(), 12.0, if hovered { AMBER_MID } else { CREAM2 });
    resp
}

/// Sunken console display switch for AV effects (`♪` amber audio + `▣` mint video LEDs).
pub fn dual_pill(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    a11y_label: &str,
    audio: &mut f32,
    video: &mut f32,
) -> bool {
    let resp = hit(ui, rect, id, Sense::click_and_drag(), a11y_label);
    let mut changed = false;
    let drag_id = id.with("drag-orig");
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(drag_id, (*audio, *video)));
    }
    if resp.dragged() {
        let (a0, v0) = ui.data(|d| d.get_temp::<(f32, f32)>(drag_id)).unwrap_or((*audio, *video));
        let delta = resp.drag_delta().x / rect.width().max(60.0);
        let na = (a0 + delta).clamp(0.0, 1.0);
        let nv = (v0 + delta).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(drag_id, (na, nv)));
        if (na - *audio).abs() > 1e-4 || (nv - *video).abs() > 1e-4 {
            *audio = na;
            *video = nv;
            changed = true;
        }
    } else if resp.clicked() {
        if *audio + *video > 0.02 {
            *audio = 0.0;
            *video = 0.0;
        } else {
            *audio = 0.55;
            *video = 0.55;
        }
        changed = true;
    }
    let on = *audio + *video > 0.02;
    let p = ui.painter();
    p.rect_filled(rect, r(4), if on { Color32::from_rgb(0x2e, 0x2b, 0x22) } else if resp.hovered() { Color32::from_rgb(0x2a, 0x28, 0x20) } else { SLOT_BG });
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.6), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.6)],
        Stroke::new(1.0, BEVEL_HI),
    );
    p.rect_stroke(rect, r(4), Stroke::new(1.0, if on { AMBER_EDGE } else { Color32::from_rgb(0x14, 0x13, 0x10) }), StrokeKind::Inside);
    p.text(
        egui::pos2(rect.left() + 9.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font_semibold(11.0),
        if on { CREAM } else { SLOT_INK },
    );
    let bar_w = (rect.width() * 0.21).clamp(15.0, 36.0);
    let bx0 = rect.right() - 8.0 - bar_w;
    let by_a = rect.center().y - 3.0;
    let by_v = rect.center().y + 3.0;
    p.line_segment([egui::pos2(bx0, by_a), egui::pos2(bx0 + bar_w, by_a)], Stroke::new(2.2, Color32::from_rgb(0x46, 0x42, 0x35)));
    p.line_segment([egui::pos2(bx0, by_v), egui::pos2(bx0 + bar_w, by_v)], Stroke::new(2.2, Color32::from_rgb(0x46, 0x42, 0x35)));
    if *audio > 0.005 {
        p.line_segment([egui::pos2(bx0, by_a), egui::pos2(bx0 + bar_w * audio.clamp(0.0, 1.0), by_a)], Stroke::new(2.2, AMBER_MID));
    }
    if *video > 0.005 {
        p.line_segment([egui::pos2(bx0, by_v), egui::pos2(bx0 + bar_w * video.clamp(0.0, 1.0), by_v)], Stroke::new(2.2, MINT_MID));
    }
    changed
}

/// Sunken console display switch for AV correspondence links (`pitch → hue`, etc.).
pub fn single_pill(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    a11y_label: &str,
    value: &mut f32,
    default_on: f32,
    discrete: bool,
) -> bool {
    let resp = hit(ui, rect, id, Sense::click_and_drag(), a11y_label);
    let mut changed = false;
    let drag_id = id.with("drag-orig");
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(drag_id, *value));
    }
    if resp.dragged() {
        let v0 = ui.data(|d| d.get_temp::<f32>(drag_id)).unwrap_or(*value);
        let delta = resp.drag_delta().x / rect.width().max(60.0);
        let mut nv = (v0 + delta).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(drag_id, nv));
        if discrete {
            nv = if nv >= 0.5 { 1.0 } else { 0.0 };
        }
        if (nv - *value).abs() > 1e-4 {
            *value = nv;
            changed = true;
        }
    } else if resp.clicked() {
        *value = if *value > 0.02 { 0.0 } else { default_on.clamp(0.0, 1.0) };
        changed = true;
    }
    let on = *value > 0.02;
    let p = ui.painter();
    p.rect_filled(rect, r(4), if on { Color32::from_rgb(0x2e, 0x2b, 0x22) } else { SLOT_BG });
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, rect.bottom() + 0.6), egui::pos2(rect.right() - 2.0, rect.bottom() + 0.6)],
        Stroke::new(1.0, BEVEL_HI),
    );
    p.rect_stroke(rect, r(4), Stroke::new(1.0, if on { AMBER_EDGE } else { Color32::from_rgb(0x14, 0x13, 0x10) }), StrokeKind::Inside);
    let bar_w = (rect.width() * 0.20).clamp(16.0, 28.0);
    let bx0 = rect.right() - 8.0 - bar_w;
    let max_text_w = (bx0 - rect.left() - 13.0).max(20.0);
    let fg = if on { CREAM } else { SLOT_INK };
    let full_g = p.layout_no_wrap(label.to_string(), font_medium(10.5), fg);
    let shown = if full_g.size().x <= max_text_w {
        label.to_string()
    } else {
        let mut chars: Vec<char> = label.chars().collect();
        while chars.len() > 4 {
            chars.pop();
            let cand: String = chars.iter().collect::<String>() + "…";
            if p.layout_no_wrap(cand.clone(), font_medium(10.5), fg).size().x <= max_text_w {
                break;
            }
        }
        chars.into_iter().collect::<String>() + "…"
    };
    p.text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        shown,
        font_medium(10.5),
        fg,
    );
    let by = rect.center().y;
    p.line_segment([egui::pos2(bx0, by), egui::pos2(bx0 + bar_w, by)], Stroke::new(2.2, Color32::from_rgb(0x46, 0x42, 0x35)));
    if *value > 0.005 {
        p.line_segment([egui::pos2(bx0, by), egui::pos2(bx0 + bar_w * value.clamp(0.0, 1.0), by)], Stroke::new(2.2, MINT_MID));
    }
    changed
}
