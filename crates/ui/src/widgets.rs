//! Studio widgets, custom-painted: segmented switches, chips, knobs,
//! emitter rings, rounded footage tiles, icons, hold detection.

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

/// 10.5 px uppercase label, `ink-3`, +0.08 em tracking.
pub fn caps(painter: &Painter, pos: Pos2, align: Align2, text: &str, size: f32) -> Rect {
    text_spaced(painter, pos, align, &text.to_uppercase(), font_medium(size), INK3, size * 0.1)
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

/// Raised surface: g2 fill, 1 px hairline, 1 px top inner highlight.
pub fn raised(painter: &Painter, rect: Rect, radius: u8, fill: Color32) {
    painter.rect_filled(rect, r(radius), fill);
    painter.rect_stroke(rect, r(radius), hair(LINE), StrokeKind::Inside);
    let y = rect.top() + 0.5;
    let inset = radius as f32 * 0.7;
    painter.line_segment(
        [egui::pos2(rect.left() + inset, y), egui::pos2(rect.right() - inset, y)],
        Stroke::new(1.0, Color32::from_white_alpha(9)),
    );
}

/// An interactive rect with an accessibility label (so tests and screen
/// readers can find custom-painted controls).
pub fn hit(ui: &mut Ui, rect: Rect, id: egui::Id, sense: Sense, label: &str) -> Response {
    let resp = ui.interact(rect, id, sense);
    let l = label.to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &l));
    resp
}

/// Press-and-hold detection (touch-first: no right-click needed). True
/// once, when the pointer has been down on `resp` for `secs` without
/// dragging; `consumed` then suppresses the click on release.
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
        ui.data_mut(|d| d.insert_temp(id, (None::<f64>, done)));
    }
    out
}

/// Segmented switch with a sliding highlight thumb. Returns the newly
/// picked index when tapped.
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
    painter.rect_filled(rect, r(12), G1);
    painter.rect_stroke(rect, r(12), hair(LINE), StrokeKind::Inside);
    let n = labels.len().max(1);
    let inner = rect.shrink(4.0);
    let w = inner.width() / n as f32;
    let x = ui.ctx().animate_value_with_time(id.with("thumb"), selected as f32, 0.22);
    let thumb = Rect::from_min_size(egui::pos2(inner.left() + x * w, inner.top()), egui::vec2(w, inner.height()));
    if thumb_hi {
        painter.rect_filled(thumb, r(9), HI);
    } else {
        painter.rect_filled(thumb, r(9), G4);
        painter.rect_stroke(thumb, r(9), hair(LINE2), StrokeKind::Inside);
    }
    let mut picked = None;
    for (i, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(egui::pos2(inner.left() + i as f32 * w, inner.top()), egui::vec2(w, inner.height()));
        let resp = hit(ui, cell, id.with(i), Sense::click(), names.get(i).copied().unwrap_or(label));
        if resp.clicked() {
            picked = Some(i);
        }
        let on = i == selected;
        let col = if on {
            if thumb_hi { ON_HI } else { INK }
        } else if resp.hovered() {
            INK
        } else {
            INK2
        };
        let hint = hints.get(i).copied().unwrap_or("");
        if hint.is_empty() {
            painter.text(cell.center(), Align2::CENTER_CENTER, *label, font.clone(), col);
        } else {
            let hint_col = if on && thumb_hi { Color32::from_black_alpha(90) } else { INK4 };
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

/// A chip: rounded raised button with free-form content painted by `f`.
pub fn chip(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, ghost: bool, on: bool) -> Response {
    let resp = hit(ui, rect, id, Sense::click_and_drag(), label);
    let p = ui.painter();
    if on {
        p.rect_filled(rect, r(10), HI);
    } else if !ghost {
        raised(p, rect, 10, if resp.is_pointer_button_down_on() { G3 } else { G2 });
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        p.rect_filled(rect, r(10), G2);
    }
    resp
}

/// Primary / secondary button with an icon painter and a label.
pub fn button(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, primary: bool, icon: Option<Icon>, kbd: Option<&str>) -> Response {
    button_named(ui, rect, id, label, label, primary, icon, kbd)
}

/// [`button`] with an accessibility name other than its label.
#[allow(clippy::too_many_arguments)]
pub fn button_named(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, name: &str, primary: bool, icon: Option<Icon>, kbd: Option<&str>) -> Response {
    let resp = hit(ui, rect, id, Sense::click(), name);
    let p = ui.painter().clone();
    let pressed = resp.is_pointer_button_down_on();
    if primary {
        p.rect_filled(rect, r(10), if pressed { INK2 } else { HI });
    } else {
        raised(&p, rect, 10, if pressed { G3 } else { G2 });
    }
    let fg = if primary { ON_HI } else { INK };
    let font = font_medium(13.0);
    let galley = p.layout_no_wrap(label.to_string(), font.clone(), fg);
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let kbd_w = if kbd.is_some() { 18.0 } else { 0.0 };
    let total = icon_w + galley.size().x + kbd_w;
    let mut x = rect.center().x - total / 2.0;
    if let Some(ic) = icon {
        draw_icon(&p, ic, egui::pos2(x + 8.0, rect.center().y), 14.0, fg);
        x += icon_w;
    }
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley.clone(), fg);
    x += galley.size().x;
    if let Some(k) = kbd {
        p.text(
            egui::pos2(x + 12.0, rect.center().y),
            Align2::CENTER_CENTER,
            k,
            mono(9.0),
            if primary { Color32::from_black_alpha(110) } else { INK3 },
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
            // A spade: blade + shaft.
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

/// Numbered emitter ring. `selected` fills it; `active` adds ripples.
pub fn emitter_ring(p: &Painter, c: Pos2, rad: f32, n: usize, selected: bool, active: bool, t: f64) {
    emitter_ring_pitch(p, c, rad, n, selected, active, t, None);
}

/// Numbered emitter ring with an optional pitch dot on its outer circumference.
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
            let a = ((1.0 - ph) * 110.0) as u8;
            p.circle_stroke(c, rr, Stroke::new(1.2, Color32::from_white_alpha(a)));
        }
    }
    p.circle_filled(c, rad + 6.0, Color32::from_black_alpha(if selected { 68 } else { 46 }));
    let outer_r = if selected { rad + 4.5 } else { rad };
    if selected {
        p.circle_stroke(c, outer_r, Stroke::new(1.2, Color32::from_white_alpha(150)));
        p.circle_filled(c, rad, HI);
        p.text(c, Align2::CENTER_CENTER, n.to_string(), mono_medium(rad * 0.85), ON_HI);
    } else {
        p.circle_filled(c, rad, Color32::from_black_alpha(75));
        p.circle_stroke(c, rad, Stroke::new(1.5, HI));
        p.text(c, Align2::CENTER_CENTER, n.to_string(), mono(rad * 0.85), HI);
    }
    if let Some(pm) = pitch_macro {
        let ha = ((pm.clamp(0.0, 1.0) - 0.5) * 2.0) * 0.75 * PI;
        let dot = egui::pos2(c.x + ha.sin() * outer_r, c.y - ha.cos() * outer_r);
        p.circle_filled(dot, 3.3, HI);
        p.circle_stroke(dot, 3.3, Stroke::new(1.2, G0));
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

/// Knob: 270° arc + disc with a vertical gradient + indicator. `detents`
/// > 0 draws detent dots instead of the arc (the LOOK knob). Vertical drag
/// changes the value (0..1); double-tap resets to `default`.
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
    let w = (rad * 0.1).clamp(2.8, 5.0);
    let v = if detents > 1 {
        let k = ((*value * detents as f32) as usize).min(detents - 1);
        k as f32 / (detents - 1) as f32
    } else {
        *value
    };
    let bipolar = (default - 0.5).abs() < 1e-3;
    if detents > 1 {
        for k in 0..detents {
            let a = a0 + span * k as f32 / (detents - 1) as f32;
            let pos = egui::pos2(center.x + rad * a.cos(), center.y + rad * a.sin());
            let on = (k as f32 / (detents - 1) as f32 - v).abs() < 1e-3;
            p.circle_filled(pos, if on { 3.4 } else { 2.0 }, if on { HI } else { G5 });
        }
    } else {
        p.add(Shape::line(arc_points(center, rad, a0, a0 + span), Stroke::new(w, G3)));
        if bipolar {
            let mid = a0 + span * 0.5;
            let cur = a0 + span * v;
            if (v - 0.5).abs() > 0.005 {
                let ( lo, hi_a ) = (mid.min(cur), mid.max(cur));
                p.add(Shape::line(arc_points(center, rad, lo, hi_a), Stroke::new(w, HI)));
                for a in [lo, hi_a] {
                    p.circle_filled(egui::pos2(center.x + rad * a.cos(), center.y + rad * a.sin()), w / 2.0, HI);
                }
            }
            let (s, cth) = (mid.sin(), mid.cos());
            p.line_segment(
                [egui::pos2(center.x + (rad + 4.0) * cth, center.y + (rad + 4.0) * s), egui::pos2(center.x + (rad + 8.0) * cth, center.y + (rad + 8.0) * s)],
                Stroke::new(1.2, INK4),
            );
        } else if v > 0.001 {
            p.add(Shape::line(arc_points(center, rad, a0, a0 + span * v), Stroke::new(w, HI)));
            for a in [a0, a0 + span * v] {
                p.circle_filled(egui::pos2(center.x + rad * a.cos(), center.y + rad * a.sin()), w / 2.0, HI);
            }
        }
    }
    // Disc: vertical gradient g4 -> g2 via a mesh.
    let disc = rad * 0.72;
    let mut mesh = egui::Mesh::default();
    let n = 48;
    let top = G4;
    let bot = G1;
    let lerp = |t: f32| -> Color32 {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        Color32::from_rgb(f(top.r(), bot.r()), f(top.g(), bot.g()), f(top.b(), bot.b()))
    };
    mesh.colored_vertex(center, lerp(0.5));
    for i in 0..=n {
        let a = TAU * i as f32 / n as f32;
        let pt = egui::pos2(center.x + disc * a.cos(), center.y + disc * a.sin());
        mesh.colored_vertex(pt, lerp((pt.y - (center.y - disc)) / (2.0 * disc)));
    }
    for i in 0..n as u32 {
        mesh.add_triangle(0, i + 1, i + 2);
    }
    p.circle_filled(center, disc + 1.0, Color32::from_black_alpha(120));
    p.add(Shape::mesh(mesh));
    p.circle_stroke(center, disc, Stroke::new(1.0, Color32::from_white_alpha(10)));
    let a = a0 + span * v;
    let (s, cth) = (a.sin(), a.cos());
    p.line_segment(
        [egui::pos2(center.x + disc * 0.3 * cth, center.y + disc * 0.3 * s), egui::pos2(center.x + disc * 0.68 * cth, center.y + disc * 0.68 * s)],
        Stroke::new(2.4, HI),
    );
    resp
}

/// Paint the window background over the corners of `rect` (so square GPU
/// output looks rounded).
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

/// A 2 px slider row value (deep drawer). Returns true when changed.
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
    p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(2.0, LINE3));
    p.line_segment([egui::pos2(rect.left(), y), egui::pos2(x, y)], Stroke::new(2.0, HI));
    p.circle_filled(egui::pos2(x, y), 8.0, HI);
    changed
}

/// Compact mini ADSR polyline + cut-threshold dashed guide (drawn on emitter
/// cards, perform pads and sequence lane headers).
pub fn mini_adsr(p: &Painter, rect: Rect, adsr: &chromagrain_core::envelope::Adsr, alpha: f32) {
    if rect.width() < 14.0 || rect.height() < 6.0 {
        return;
    }
    let pts = crate::envelope::points(rect, adsr);
    let a = (alpha.clamp(0.0, 1.0) * 230.0) as u8;
    p.add(Shape::line(pts.to_vec(), Stroke::new(1.4, Color32::from_rgba_unmultiplied(243, 242, 238, a))));
    if adsr.picture == chromagrain_core::envelope::PictureCut::Cut {
        let ty = rect.bottom() - adsr.cut_threshold.clamp(0.0, 1.0) * rect.height();
        let x_end = pts[4].x.max(rect.left() + 8.0);
        let da = (alpha.clamp(0.0, 1.0) * 95.0) as u8;
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
        let center = egui::pos2(rect.center().x, rect.top() + rect.height() * if small { 0.48 } else { 0.48 });
        let ha = ((pitch_macro.clamp(0.0, 1.0) - 0.5) * 2.0) * 0.75 * PI;
        let pitch_pos = egui::pos2(center.x + ha.sin() * rr, center.y - ha.cos() * rr);
        Self { center, rr, r_max, r_min, pitch_pos }
    }
}

/// Draw the on-object grain ring + golden-angle cloud dots + pitch dot +
/// optional smear trail (`objSVG` in `design/v002`).
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
    p.circle_stroke(geo.center, geo.rr, Stroke::new(1.5, Color32::from_rgba_unmultiplied(243, 242, 238, 215)));
    let n = (1.0 + 15.0 * g).round() as usize;
    let dot_r = 3.4 + (1.4 - 3.4) * g;
    for k in 0..n {
        let a = k as f32 * 2.39996;
        let rad = geo.rr * 0.74 * ((k as f32 + 0.5) / n as f32).sqrt();
        let pt = egui::pos2(geo.center.x + a.cos() * rad, geo.center.y + a.sin() * rad);
        p.circle_filled(pt, if small { dot_r * 0.8 } else { dot_r }, Color32::from_rgba_unmultiplied(243, 242, 238, 178));
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
                let alpha = ((0.55 - off.abs() * 0.2) * 255.0) as u8;
                p.line_segment([egui::pos2(x0, yy), egui::pos2(x1, yy)], Stroke::new(1.5, Color32::from_rgba_unmultiplied(243, 242, 238, alpha)));
            }
        }
    }
    // 12 o'clock neutral tick + pitch dot on the ring circumference.
    p.line_segment(
        [egui::pos2(geo.center.x, geo.center.y - geo.rr - 4.5), egui::pos2(geo.center.x, geo.center.y - geo.rr + 2.5)],
        Stroke::new(1.4, Color32::from_white_alpha(90)),
    );
    let pr = if small { 4.6 } else { 6.2 };
    p.circle_filled(geo.pitch_pos, pr, HI);
    p.circle_stroke(geo.pitch_pos, pr, Stroke::new(1.8, G0));
    if !small && pitch_st.abs() >= 0.5 {
        let tag_y = (geo.pitch_pos.y - 12.0).max(rect.top() + 9.0);
        p.text(egui::pos2(geo.pitch_pos.x, tag_y), Align2::CENTER_CENTER, format!("{pitch_st:+.0}"), mono(10.0), HI);
    }
    geo
}

/// Primary white `Open clip ›` pill used in contextual object bars.
pub fn open_clip_pill(ui: &mut Ui, rect: Rect, id: egui::Id, a11y_label: &str) -> Response {
    let resp = hit(ui, rect, id, Sense::click(), a11y_label);
    let p = ui.painter();
    let rad = (rect.height() * 0.5).min(15.0) as u8;
    p.rect_filled(rect, r(rad), if resp.is_pointer_button_down_on() { INK2 } else { HI });
    let text_galley = p.layout_no_wrap("Open clip".into(), font_medium(11.5), ON_HI);
    let total_w = text_galley.size().x + 14.0;
    let x0 = rect.center().x - total_w * 0.5;
    p.galley(egui::pos2(x0, rect.center().y - text_galley.size().y * 0.5), text_galley.clone(), ON_HI);
    draw_icon(p, Icon::ChevronRight, egui::pos2(x0 + text_galley.size().x + 9.0, rect.center().y + 0.5), 11.0, ON_HI);
    resp
}

/// Small dark rounded `›` button on Perform pads and Sequence lane headers.
pub fn chev_button(ui: &mut Ui, vis_rect: Rect, hit_rect: Rect, id: egui::Id, a11y_label: &str) -> Response {
    let resp = hit(ui, hit_rect, id, Sense::click(), a11y_label);
    let p = ui.painter();
    let hovered = resp.hovered() || resp.is_pointer_button_down_on();
    p.rect_filled(vis_rect, r(6), Color32::from_black_alpha(if hovered { 205 } else { 155 }));
    p.rect_stroke(vis_rect, r(6), hair(if hovered { LINE3 } else { LINE2 }), StrokeKind::Inside);
    draw_icon(p, Icon::ChevronRight, vis_rect.center(), 12.0, if hovered { HI } else { INK2 });
    resp
}

/// Dual-amount pill for AV effects (`♪` audio + `▣` video bars). Click toggles;
/// horizontal drag adjusts both amounts in `0..1`. Returns true when changed.
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
    p.rect_filled(rect, r(8), if on { G3 } else if resp.hovered() { G2 } else { Color32::from_rgb(0x16, 0x16, 0x18) });
    p.rect_stroke(rect, r(8), hair(if on { LINE2 } else { LINE }), StrokeKind::Inside);
    p.text(
        egui::pos2(rect.left() + 9.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font(11.0),
        if on { INK } else { INK3 },
    );
    let bar_w = (rect.width() * 0.21).clamp(15.0, 36.0);
    let bx0 = rect.right() - 8.0 - bar_w;
    let by_a = rect.center().y - 3.0;
    let by_v = rect.center().y + 3.0;
    p.line_segment([egui::pos2(bx0, by_a), egui::pos2(bx0 + bar_w, by_a)], Stroke::new(2.0, G4));
    p.line_segment([egui::pos2(bx0, by_v), egui::pos2(bx0 + bar_w, by_v)], Stroke::new(2.0, G4));
    if *audio > 0.005 {
        p.line_segment([egui::pos2(bx0, by_a), egui::pos2(bx0 + bar_w * audio.clamp(0.0, 1.0), by_a)], Stroke::new(2.0, HI));
    }
    if *video > 0.005 {
        p.line_segment(
            [egui::pos2(bx0, by_v), egui::pos2(bx0 + bar_w * video.clamp(0.0, 1.0), by_v)],
            Stroke::new(2.0, Color32::from_white_alpha(150)),
        );
    }
    changed
}

/// Single-bar pill for AV correspondence links (`pitch → hue`, etc.). Click
/// toggles `0 <-> default_on` (unless `discrete`, which snaps `0` or `1`);
/// horizontal drag sets `0..1`. Returns true when changed.
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
    p.rect_filled(rect, r(8), if on { G2 } else { Color32::from_rgb(0x15, 0x15, 0x17) });
    p.rect_stroke(rect, r(8), hair(if on { LINE2 } else { LINE }), StrokeKind::Inside);
    let bar_w = (rect.width() * 0.20).clamp(16.0, 28.0);
    let bx0 = rect.right() - 8.0 - bar_w;
    let max_text_w = (bx0 - rect.left() - 13.0).max(20.0);
    let fg = if on { INK } else { INK3 };
    let full_g = p.layout_no_wrap(label.to_string(), font(10.5), fg);
    let shown = if full_g.size().x <= max_text_w {
        label.to_string()
    } else {
        let mut chars: Vec<char> = label.chars().collect();
        while chars.len() > 4 {
            chars.pop();
            let cand: String = chars.iter().collect::<String>() + "…";
            if p.layout_no_wrap(cand.clone(), font(10.5), fg).size().x <= max_text_w {
                break;
            }
        }
        chars.into_iter().collect::<String>() + "…"
    };
    p.text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        shown,
        font(10.5),
        fg,
    );
    let by = rect.center().y;
    p.line_segment([egui::pos2(bx0, by), egui::pos2(bx0 + bar_w, by)], Stroke::new(2.0, G4));
    if *value > 0.005 {
        p.line_segment([egui::pos2(bx0, by), egui::pos2(bx0 + bar_w * value.clamp(0.0, 1.0), by)], Stroke::new(2.0, HI));
    }
    changed
}
