//! The compact 4-handle A / D / S / R editor, drawn over the clip's own
//! thumbnail: the picture under the curve shows what the envelope does to
//! it (cut: full until the hard cut; fade: opacity follows the curve). No
//! numbers unless a handle is being dragged.

use crate::theme::*;
use crate::widgets::*;
use chromagrain_core::envelope::{Adsr, PictureCut};
use egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureId, Ui};

/// Longest attack / decay and release the editor offers (seconds).
pub const MAX_AD: f32 = 2.0;
pub const MAX_R: f32 = 4.0;

fn frac(v: f32, max: f32) -> f32 {
    (v / max).clamp(0.0, 1.0).sqrt()
}

fn unfrac(f: f32, max: f32) -> f32 {
    let f = f.clamp(0.0, 1.0);
    // Snap the very start to exactly 0 (an instant attack / hard cut).
    if f < 0.04 {
        0.0
    } else {
        f * f * max
    }
}

/// The curve's corner points in `rect` (bottom-left start, four quarter
/// stages: attack, decay, sustain hold, release).
pub fn points(rect: Rect, e: &Adsr) -> [Pos2; 5] {
    let q = rect.width() / 4.0;
    let y = |lvl: f32| rect.bottom() - lvl.clamp(0.0, 1.0) * rect.height();
    let p0 = egui::pos2(rect.left(), rect.bottom());
    let p1 = egui::pos2(rect.left() + q * frac(e.attack, MAX_AD), rect.top());
    let p2 = egui::pos2(p1.x + q * frac(e.decay, MAX_AD), y(e.sustain));
    let p3 = egui::pos2(rect.left() + 3.0 * q, y(e.sustain));
    let p4 = egui::pos2(p3.x + q * frac(e.release, MAX_R), rect.bottom());
    [p0, p1, p2, p3, p4]
}

/// Envelope level (0..1) at screen x along the curve.
fn level_at(pts: &[Pos2; 5], rect: Rect, x: f32) -> f32 {
    if x < pts[0].x || x > pts[4].x {
        return 0.0;
    }
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if x >= a.x && x <= b.x {
            let t = if b.x - a.x < 1e-3 { 1.0 } else { (x - a.x) / (b.x - a.x) };
            let y = a.y + (b.y - a.y) * t;
            return ((rect.bottom() - y) / rect.height()).clamp(0.0, 1.0);
        }
    }
    0.0
}

fn ms(v: f32) -> String {
    if v < 1.0 {
        format!("{:.0} ms", v * 1000.0)
    } else {
        format!("{v:.2} s")
    }
}

/// Draw + edit. `thumb` is the clip's frame (texture + uv); the picture
/// under the curve follows `env.picture`. Returns true when a value changed.
pub fn adsr_editor(ui: &mut Ui, rect: Rect, id: egui::Id, env: &mut Adsr, thumb: Option<(TextureId, Rect)>) -> bool {
    let fade = env.picture == PictureCut::Fade;
    let cut_level = env.cut_threshold;
    let p = ui.painter().clone();
    p.rect_filled(rect, r(10), G1);
    let inner = rect.shrink2(egui::vec2(14.0, 12.0));
    let pts = points(inner, env);
    // The picture: dim everywhere, lit where (and as much as) the envelope
    // lets it through.
    if let Some((tex, uv)) = thumb {
        tile(&p, rect, 10, tex, uv, gray(0.16));
        // Strips only inside `inner` (the curve's span), so the rounded
        // corners stay the dim tile.
        let strips = 64;
        let sw = inner.width() / strips as f32;
        let u_of = |x: f32| uv.left() + uv.width() * ((x - rect.left()) / rect.width());
        for k in 0..strips {
            let x0 = inner.left() + k as f32 * sw;
            let lvl = level_at(&pts, inner, x0 + sw * 0.5);
            let a = if fade { lvl } else if lvl >= cut_level { 1.0 } else { 0.0 };
            if a <= 0.0 {
                continue;
            }
            let sr = Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x0 + sw + 0.5, rect.bottom()));
            let suv = Rect::from_min_max(egui::pos2(u_of(sr.left()), uv.top()), egui::pos2(u_of(sr.right()), uv.bottom()));
            let v = (a * 0.86 * 255.0) as u8;
            p.image(tex, sr, suv, Color32::from_rgb(v, v, v));
        }
    }
    // The curve.
    p.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.6, HI)));
    p.line_segment([egui::pos2(inner.left(), inner.bottom()), egui::pos2(inner.right(), inner.bottom())], hair(LINE2));
    p.rect_stroke(rect, r(10), hair(LINE2), StrokeKind::Inside);

    // Handles: A (attack time), D (decay time + sustain), S (sustain), R (release time).
    let mut changed = false;
    let q = inner.width() / 4.0;
    let names = ["A", "D", "S", "R"];
    let mut shown: Option<(Pos2, String)> = None;
    for (k, name) in names.iter().enumerate() {
        let at = pts[k + 1];
        let resp = hit(ui, Rect::from_center_size(at, egui::vec2(44.0, 44.0)), id.with(("adsr", k)), Sense::click_and_drag(), &format!("adsr {name}"));
        let active = resp.dragged() || resp.is_pointer_button_down_on();
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            let lvl = ((inner.bottom() - pos.y) / inner.height()).clamp(0.0, 1.0);
            let before = *env;
            match k {
                0 => env.attack = unfrac((pos.x - inner.left()) / q, MAX_AD),
                1 => {
                    env.decay = unfrac((pos.x - pts[1].x) / q, MAX_AD);
                    env.sustain = lvl;
                }
                2 => env.sustain = lvl,
                _ => env.release = unfrac((pos.x - pts[3].x) / q, MAX_R),
            }
            changed |= *env != before;
        }
        let rad = if active { 7.0 } else { 5.5 };
        p.circle_filled(at, rad, if active { HI } else { G1 });
        p.circle_stroke(at, rad, Stroke::new(1.6, HI));
        if active {
            let v = match k {
                0 => format!("A {}", ms(env.attack)),
                1 => format!("D {}", ms(env.decay)),
                2 => format!("S {:.0}%", env.sustain * 100.0),
                _ => format!("R {}", ms(env.release)),
            };
            shown = Some((at, v));
        }
    }
    if let Some((at, v)) = shown {
        let g = p.layout_no_wrap(v, mono(11.0), INK);
        let tag = Rect::from_center_size(egui::pos2(at.x.clamp(rect.left() + 40.0, rect.right() - 40.0), (at.y - 22.0).max(rect.top() + 12.0)), g.size() + egui::vec2(14.0, 8.0));
        p.rect_filled(tag, r(6), Color32::from_black_alpha(190));
        p.galley(tag.center() - g.size() / 2.0, g, INK);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_round_trip_through_screen_space() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 100.0));
        let e = Adsr { attack: 0.5, decay: 0.32, sustain: 0.6, release: 1.0, ..Default::default() };
        let pts = points(rect, &e);
        let q = 100.0;
        assert!((unfrac(pts[1].x / q, MAX_AD) - e.attack).abs() < 1e-4);
        assert!((unfrac((pts[2].x - pts[1].x) / q, MAX_AD) - e.decay).abs() < 1e-4);
        assert!((unfrac((pts[4].x - pts[3].x) / q, MAX_R) - e.release).abs() < 1e-4);
        assert!((level_at(&pts, rect, pts[3].x - 1.0) - 0.6).abs() < 1e-3);
        assert_eq!(level_at(&pts, rect, 399.9), 0.0);
        assert_eq!(unfrac(0.01, MAX_AD), 0.0, "the start snaps to an instant edge");
    }
}
