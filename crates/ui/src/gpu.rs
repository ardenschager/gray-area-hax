//! GPU preview pipeline (wgpu: Metal on macOS/iOS, D3D12/Vulkan on
//! Windows/Linux, GL as a fallback): renders the composition in real time
//! for performance playback across four stage modes:
//! - `Stack` (`Layout::Stack`): spatial layered grain quads
//! - `Split` (`Layout::Split`, `Split + Bleed`): full-canvas `1-up / 2-up /
//!   3-up / 2×2` splits (`1×2` on `9:16`, `2×1` on `16:9`/`1:1`) with `+6.5%`
//!   feathered screen-blended interior seams + sustained wash bleed across 100%
//! - `Grid` (`Layout::Tile`): crisp architectural panels with `1.5 px`
//!   obsidian dividers, low→high chord ordering, and `ratchet_odd` `flipX`
//! - `Bleed` (`Layout::Bleed`): full-canvas optical luma-key + screen /
//!   soft-exclusion multi-exposure melt across active voices
//!
//! Includes smart `(cx, cy)` subject crop, `52 ms` optical carry, transient
//! `zoom_punch`, Basinski `oxide_dropout` wear, and polar `Kaleido` (`fs_kaleido`).

use crate::host::StageMode;
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::render::ClipPlan;
use chromagrain_core::timeline::{ClipKind, Project};
use chromagrain_core::video::{grain_draws_layout, ColorFilterMode, GrainDraw, VisualGrainStyle};

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// What the pipeline draws each frame (cheap to clone: media is Arc'd).
pub struct Scene {
    pub project: Project,
    pub plan: Arc<Vec<ClipPlan>>,
    pub t: f64,
    /// Live hits (MIDI pads, emitters) timed on the app clock:
    /// (source, bus, event). Bus 0 = the plain pad layer; bus b > 0 uses
    /// `buses[b - 1]` (its own grain style + effect chain).
    pub live: Vec<(usize, u8, GrainEvent)>,
    pub live_t: f64,
    pub buses: Vec<LiveBus>,
    /// Full-frame picture under everything: (source, media seconds).
    pub backdrop: Option<(usize, f64)>,
    /// Transport stopped: long events ignore fades so the still frame shows.
    pub parked: bool,
    /// App frame counter: the scene is rendered once per distinct value,
    /// however many viewports present it.
    pub frame: u64,
    /// Optional stage mode override (`Stack | Split | Grid | Bleed`).
    pub mode_override: Option<StageMode>,
    /// Per-source smart crop focus `(cx, cy)` overrides from the Clip sheet.
    pub focus_overrides: HashMap<usize, (f32, f32)>,
    /// Live Perform `Tape Stop` amount (`0..1`).
    pub tape_stop: f32,
}

/// A live layer's look: how its grains are drawn and what effects run
/// over it (e.g. one per emitter: GRAIN's style, SMEAR + LOOK).
#[derive(Clone, Debug, Default)]
pub struct LiveBus {
    pub style: VisualGrainStyle,
    pub effects: Vec<AvEffect>,
    /// Sound↔picture dials for this layer.
    pub link: chromagrain_core::video::AvLink,
}

pub fn effective_stage_mode(scene: &Scene) -> StageMode {
    if let Some(m) = scene.mode_override {
        return m;
    }
    match scene.project.layout.name() {
        "split" => StageMode::Split,
        "tile" | "grid" => StageMode::Grid,
        "bleed" | "blend" => StageMode::Bleed,
        _ => StageMode::Stack,
    }
}

/// The frame's tile grid (`Layout::Tile` / `StageMode::Grid`), as the CPU
/// compositor computes it (`render::frame_tile_grids` / `tile::frame_grid`).
pub fn tile_grid(scene: &Scene) -> Option<(u32, u32)> {
    if effective_stage_mode(scene) != StageMode::Grid {
        return None;
    }
    let mut slots: Vec<u8> = Vec::new();
    let mut see = |e: &GrainEvent, t: f64| {
        if let Some(slot) = e.tag.tile {
            if t >= e.onset && t < e.end() && e.tag.picture_at(t) > 0.0 && !slots.contains(&slot) {
                slots.push(slot);
            }
        }
    };
    for cp in scene.plan.iter() {
        if scene.project.tracks.get(cp.track).is_some_and(|tr| tr.muted) {
            continue;
        }
        for (_, evs) in cp.video_parts.as_deref().unwrap_or(&cp.parts) {
            for e in evs {
                see(e, scene.t);
            }
        }
    }
    for (_, _, e) in &scene.live {
        see(e, scene.live_t);
    }
    Some(chromagrain_core::tile::frame_grid(slots))
}

/// Cover-crop normalized `[x, y, w, h]` patch around `focus = (cx, cy)` with
/// transient `zoom_punch` (`0..0.35`), preserving `src_aspect` inside `cell_aspect`.
pub fn cover_patch_focus(src_aspect: f32, cell_aspect: f32, focus: (f32, f32), zoom_punch: f32) -> [f32; 4] {
    let sa = src_aspect.max(0.05);
    let ca = cell_aspect.max(0.05);
    let (mut pw, mut ph) = if ca < sa { (ca / sa, 1.0) } else { (1.0, sa / ca) };
    let z = (1.0 + zoom_punch.clamp(0.0, 0.35)).max(1.0);
    pw = (pw / z).clamp(0.05, 1.0);
    ph = (ph / z).clamp(0.05, 1.0);
    let cx = focus.0.clamp(0.0, 1.0);
    let cy = focus.1.clamp(0.0, 1.0);
    let px = (cx - pw * 0.5).clamp(0.0, (1.0 - pw).max(0.0));
    let py = (cy - ph * 0.5).clamp(0.0, (1.0 - ph).max(0.0));
    [px, py, pw, ph]
}

/// Compute `Split + Bleed` panel rect `[x, y, w, h]` and interior-edge feather
/// bitmask (`bit 0 = left`, `bit 1 = right`, `bit 2 = top`, `bit 3 = bottom`)
/// for panel `idx` out of `count` foreground voices on `canvas_aspect`.
/// On widescreen (`16:9`, `canvas_aspect > 1.3`), `2`, `3`, and `4` voices lay
/// out as side-by-side portrait monolith columns (`2×1`, `3×1`, `4×1`, *MODELL 5*).
pub fn split_panel_rect(idx: usize, count: usize, canvas_aspect: f32) -> ([f32; 4], u32) {
    let n = count.max(1);
    let i = idx.min(n - 1);
    let portrait = canvas_aspect < 0.85;
    let wide = canvas_aspect > 1.3;
    let (cols, rows): (usize, usize) = match n {
        1 => (1, 1),
        2 => if portrait { (1, 2) } else { (2, 1) },
        3 => if portrait { (1, 3) } else { (3, 1) },
        4 => if wide { (4, 1) } else { (2, 2) },
        5 | 6 => if portrait { (2, 3) } else { (3, 2) },
        _ => (3, 3),
    };
    let c = i % cols;
    let r = i / cols;
    let cw = 1.0 / cols as f32;
    let ch = 1.0 / rows as f32;
    let overlap_x = 0.065 * cw;
    let overlap_y = 0.065 * ch;

    let mut x0 = c as f32 * cw;
    let mut x1 = (c + 1) as f32 * cw;
    let mut y0 = r as f32 * ch;
    let mut y1 = (r + 1) as f32 * ch;
    let mut mask = 0u32;

    if c > 0 {
        x0 = (x0 - overlap_x).max(0.0);
        mask |= 1;
    }
    if c + 1 < cols {
        x1 = (x1 + overlap_x).min(1.0);
        mask |= 2;
    }
    if r > 0 {
        y0 = (y0 - overlap_y).max(0.0);
        mask |= 4;
    }
    if r + 1 < rows {
        y1 = (y1 + overlap_y).min(1.0);
        mask |= 8;
    }
    ([x0, y0, (x1 - x0).max(1e-3), (y1 - y0).max(1e-3)], mask)
}

const MAX_SRC_W: u32 = 256;
const MAX_SRC_LAYERS: usize = 192;
const MAX_HISTORY: usize = 40;
const UNI_STRIDE: u64 = 256;
const PRESENT_SLOTS: usize = 16;
const RGBA8: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const RGBA16F: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// Slab indices of the fixed render targets.
const MASTER: usize = 0;
const TRACK: usize = 1;
const CLIP: usize = 2;
const PING: usize = 3;
const PONG: usize = 4;
const FPING: usize = 5;
const FPONG: usize = 6;
const CARRY: usize = 7;
const FISO: usize = 8;
const FIXED_TARGETS: usize = 9;

/// Per-draw uniforms: one struct shared by every pass (unused fields are
/// ignored). Mirrors `struct U` in the WGSL below.
#[repr(C)]
#[derive(Clone, Copy)]
struct Uni {
    /// Destination rect x,y,w,h in 0..1, y-down.
    rect: [f32; 4],
    /// Source patch (grains) / source uv crop (present).
    patch: [f32; 4],
    /// Color filter: center_deg, half_width_deg, softness, sat_min.
    cf: [f32; 4],
    /// rot (rad), aspect, layer, alpha.
    a: [f32; 4],
    /// hue (deg), cf_mode, cf_valmin, additive.
    b: [f32; 4],
    /// opacity, alpha_mul, mix, amount.
    c: [f32; 4],
    /// res.xy, shift.xy.
    res: [f32; 4],
    /// Pass-specific params (for `fs_grain`: feather, flip_x, interior_mask, oxide_wear).
    d: [f32; 4],
    hues: [f32; 12],
}

impl Default for Uni {
    fn default() -> Self {
        Uni {
            rect: [0.0, 0.0, 1.0, 1.0],
            patch: [0.0, 0.0, 1.0, 1.0],
            cf: [0.0; 4],
            a: [0.0, 1.0, 0.0, 1.0],
            b: [0.0; 4],
            c: [1.0, 1.0, 0.0, 0.0],
            res: [0.0; 4],
            d: [0.0; 4],
            hues: [0.0; 12],
        }
    }
}

const UNI_SIZE: u64 = std::mem::size_of::<Uni>() as u64;

fn uni_bytes(u: &Uni) -> &[u8] {
    // SAFETY: Uni is repr(C), all f32, no padding.
    unsafe { std::slice::from_raw_parts(u as *const Uni as *const u8, std::mem::size_of::<Uni>()) }
}

const SHADER: &str = r#"
struct U {
    rect: vec4f,
    patch_r: vec4f,
    cf: vec4f,
    a: vec4f,
    b: vec4f,
    c: vec4f,
    res: vec4f,
    d: vec4f,
    hues: array<vec4f, 3>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var tarr: texture_2d_array<f32>;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    var corners = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
        vec2f(0.0, 0.0), vec2f(1.0, 1.0), vec2f(0.0, 1.0));
    let a_pos = corners[vi];
    let c = u.rect.xy + 0.5 * u.rect.zw;
    var off = (a_pos - 0.5) * u.rect.zw;
    off.x *= u.a.y;
    let cs = cos(u.a.x);
    let sn = sin(u.a.x);
    off = vec2f(off.x * cs - off.y * sn, off.x * sn + off.y * cs);
    off.x /= u.a.y;
    let p = c + off;
    var o: VOut;
    o.pos = vec4f(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    o.uv = a_pos;
    return o;
}

fn gmod(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }

fn rgb2hsv(c: vec3f) -> vec3f {
    let K = vec4f(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4f(c.bg, K.wz), vec4f(c.gb, K.xy), step(c.b, c.g));
    let q = mix(vec4f(p.xyw, c.r), vec4f(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3f(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv2rgb(c: vec3f) -> vec3f {
    let K = vec4f(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, vec3f(0.0), vec3f(1.0)), c.y);
}

fn tex0(uv: vec2f) -> vec4f { return textureSampleLevel(t0, samp, uv, 0.0); }
fn tex1(uv: vec2f) -> vec4f { return textureSampleLevel(t1, samp, uv, 0.0); }

// Grain / Split / Grid quad:
// - d.x = border feather (0..1)
// - d.y = flip_x (1.0 on odd ratchet sub-hits)
// - d.z = Split interior-seam feather bitmask (bit0=L, bit1=R, bit2=T, bit3=B)
// - d.w = Basinski oxide_dropout visual wear (0..1)
@fragment
fn fs_grain(i: VOut) -> @location(0) vec4f {
    let ux = select(i.uv.x, 1.0 - i.uv.x, u.d.y >= 0.5);
    let suv = u.patch_r.xy + vec2f(ux, i.uv.y) * u.patch_r.zw;
    let texel = textureSampleLevel(tarr, samp, suv, i32(u.a.z + 0.5), 0.0);
    var rgb = texel.rgb;
    var a = u.a.w * texel.a;
    var hsv = rgb2hsv(rgb);
    let mode = i32(u.b.y + 0.5);
    if (mode != 0) {
        let h = hsv.x * 360.0;
        let d = abs(gmod(h - u.cf.x + 540.0, 360.0) - 180.0);
        let halfw = max(u.cf.y, 0.001);
        let soft = max(halfw * u.cf.z, 0.001);
        let inband = clamp(1.0 - (d - (halfw - soft)) / (2.0 * soft), 0.0, 1.0);
        let colored = select(0.0, 1.0, hsv.y >= u.cf.w && hsv.z >= u.b.z);
        let band = inband * colored;
        a *= select(1.0 - band, band, mode == 1);
    }
    if (abs(u.b.x) > 0.5) {
        hsv.x = fract(hsv.x + u.b.x / 360.0 + 10.0);
        rgb = hsv2rgb(hsv);
    }
    // Basinski ferric oxide dropout & warm saturation decay.
    let oxide = clamp(u.d.w, 0.0, 1.0);
    if (oxide > 0.01) {
        let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
        let ferric = vec3f(luma * 1.06, luma * 0.96, luma * 0.82);
        rgb = clamp(mix(rgb, ferric, oxide * 0.48), vec3f(0.0), vec3f(1.0));
        let scan_line = floor(i.uv.y * 92.0 + u.a.z * 3.17);
        let h_noise = fract(sin(scan_line * 12.9898 + u.a.z * 0.73) * 43758.5453);
        let drop = smoothstep(1.0 - oxide * 0.34, 1.0, h_noise);
        a *= (1.0 - 0.72 * drop);
    }
    // Grain cosine border feather (d.x).
    let f = clamp(u.d.x, 0.0, 1.0) * 0.45;
    if (f > 0.0001) {
        let q = min(i.uv, vec2f(1.0) - i.uv);
        let e = clamp(q / f, vec2f(0.0), vec2f(1.0));
        let w = vec2f(0.5) - 0.5 * cos(3.14159265 * e);
        a *= w.x * w.y;
    }
    // Split+Bleed interior-seam feather ONLY on shared inner borders (d.z bitmask).
    let emask = u32(u.d.z + 0.5);
    if (emask != 0u) {
        let fw = 0.22;
        if ((emask & 1u) != 0u) { a *= smoothstep(0.0, fw, i.uv.x); }
        if ((emask & 2u) != 0u) { a *= smoothstep(0.0, fw, 1.0 - i.uv.x); }
        if ((emask & 4u) != 0u) { a *= smoothstep(0.0, fw, i.uv.y); }
        if ((emask & 8u) != 0u) { a *= smoothstep(0.0, fw, 1.0 - i.uv.y); }
    }
    return vec4f(rgb * a, a * (1.0 - u.b.w));
}

// Full Bleed optical multi-exposure melt: t0 = base canvas, t1 = incoming layer, c.x = weight.
@fragment
fn fs_bleed(i: VOut) -> @location(0) vec4f {
    let base = tex0(i.uv);
    let inc = tex1(i.uv);
    let va = clamp(inc.a * u.c.x, 0.0, 1.0);
    if (va <= 0.002) {
        return base;
    }
    let v_rgb = clamp(inc.rgb / max(inc.a, 1e-4), vec3f(0.0), vec3f(1.0));
    let b_rgb = clamp(base.rgb, vec3f(0.0), vec3f(1.0));
    let b_luma = dot(b_rgb, vec3f(0.2126, 0.7152, 0.0722));
    if (b_luma < 0.025) {
        return vec4f(mix(b_rgb, v_rgb, va), 1.0);
    }
    let v_luma = dot(v_rgb, vec3f(0.2126, 0.7152, 0.0722));
    let screen_rgb = vec3f(1.0) - (vec3f(1.0) - b_rgb) * (vec3f(1.0) - 0.92 * v_rgb);
    let excl_rgb = b_rgb + v_rgb - 1.55 * b_rgb * v_rgb;
    let optical_melt = mix(mix(v_rgb, excl_rgb, 0.22), screen_rgb, 0.58);
    let luma_key = smoothstep(0.08, 0.78, v_luma + 0.26 * (1.0 - b_luma));
    let w = clamp(va * mix(0.52, 1.0, luma_key), 0.0, 1.0);
    return vec4f(clamp(mix(b_rgb, optical_melt, w), vec3f(0.0), vec3f(1.0)), 1.0);
}

// Blit: c.x opacity, c.y extra alpha scale, b.w additive.
@fragment
fn fs_blit(i: VOut) -> @location(0) vec4f {
    let c = tex0(i.uv);
    return vec4f(c.rgb * u.c.x, c.a * u.c.x * u.c.y * (1.0 - u.b.w));
}

// Present: the composited canvas, opaque, through a uv crop (patch_r).
@fragment
fn fs_present(i: VOut) -> @location(0) vec4f {
    let c = tex0(u.patch_r.xy + i.uv * u.patch_r.zw);
    return vec4f(c.rgb, 1.0);
}

// True Polar Kaleidoscope (`EffectKind::Kaleido` — 1:1 with `fx::kaleido_source_uv`):
// d.x = folds (2..12), d.y = angle_rad, d.z = cx, d.w = cy, c.w = video wet (0..1).
fn mirror_wrap1(t: f32) -> f32 {
    let m = gmod(t, 2.0);
    if (m > 1.0) { return 2.0 - m; }
    return m;
}

@fragment
fn fs_kaleido(i: VOut) -> @location(0) vec4f {
    let px0 = min(vec2i(i.uv * u.res.xy), vec2i(u.res.xy) - 1);
    let orig = textureLoad(t0, px0, 0);
    let w = clamp(u.c.w, 0.0, 1.0);
    if (w <= 0.001) { return orig; }
    let aspect = max(u.res.x / max(u.res.y, 1.0), 1e-4);
    let center = vec2f(u.d.z, u.d.w);
    let dx = (i.uv.x - center.x) * aspect;
    let dy = i.uv.y - center.y;
    let r = sqrt(dx * dx + dy * dy);
    let folds = clamp(u.d.x, 2.0, 12.0);
    let wedge = 3.14159265359 / folds;
    let theta = atan2(dy, dx) - u.d.y;
    let q = gmod(theta, 6.28318530718) / wedge;
    let fold_idx = i32(floor(q));
    let f = q - f32(fold_idx);
    let odd = ((fold_idx % 2) + 2) % 2;
    let folded = select(f, 1.0 - f, odd == 1);
    let src_theta = folded * wedge + u.d.y * 0.35;
    let raw_u = center.x + (r * cos(src_theta)) / aspect;
    let raw_v = center.y + r * sin(src_theta);
    let suv = clamp(vec2f(mirror_wrap1(raw_u), mirror_wrap1(raw_v)), vec2f(0.0), vec2f(1.0));
    let spx = min(vec2i(suv * u.res.xy), vec2i(u.res.xy) - 1);
    let kal = textureLoad(t0, spx, 0);
    return mix(orig, kal, w);
}

// Live Perform `Tape Stop`: VHS vertical sync roll + tracking shear (d.x = amt 0..1, d.y = phase).
@fragment
fn fs_tapestop(i: VOut) -> @location(0) vec4f {
    let amt = clamp(u.d.x, 0.0, 1.0);
    if (amt <= 0.002) { return tex0(i.uv); }
    let roll = fract(i.uv.y + amt * amt * 0.38 + sin(u.d.y * 3.0) * 0.02 * amt);
    let shear = sin(roll * 42.0 + u.d.y * 9.0) * 0.022 * amt;
    let uv = clamp(vec2f(i.uv.x + shear, roll), vec2f(0.001), vec2f(0.999));
    var c = tex0(uv);
    let bar = smoothstep(0.92, 0.99, abs(roll - 0.5) * 2.0) * amt;
    c = vec4f(mix(c.rgb * (1.0 - 0.22 * amt), vec3f(0.85, 0.84, 0.80), bar * 0.35), c.a);
    return c;
}

// Tint: d.x target hue (deg), c.w amount.
@fragment
fn fs_tint(i: VOut) -> @location(0) vec4f {
    let c = tex0(i.uv);
    let a = max(c.a, 1e-5);
    let hsv = rgb2hsv(c.rgb / a);
    let h = hsv.x * 360.0;
    let d = gmod(h - u.d.x + 540.0, 360.0) - 180.0;
    let amount = u.c.w;
    let nh = gmod(u.d.x + d * (1.0 - amount) + 360.0, 360.0);
    let ns = max(hsv.y, amount * amount * 0.5 * min(hsv.z, 1.0));
    return vec4f(hsv2rgb(vec3f(nh / 360.0, ns, hsv.z)) * a, c.a);
}

// Tune (mirrors fx::video_tune): snap hues to the key colors.
@fragment
fn fs_tune(i: VOut) -> @location(0) vec4f {
    let c = tex0(i.uv);
    let a = max(c.a, 1e-5);
    let hsv = rgb2hsv(c.rgb / a);
    let h = hsv.x * 360.0;
    var best = 0.0;
    var best_abs = 1e9;
    let nh_count = i32(u.d.w + 0.5);
    for (var k = 0; k < 12; k++) {
        if (k >= nh_count) { break; }
        let d = gmod(u.hues[k / 4][k % 4] - h + 540.0, 360.0) - 180.0;
        if (abs(d) < best_abs) { best_abs = abs(d); best = d; }
    }
    let nh = gmod(h + best * u.c.w + 360.0, 360.0);
    return vec4f(hsv2rgb(vec3f(nh / 360.0, hsv.y, hsv.z)) * a, c.a);
}

// Crush: d.x block size (px), d.y color levels.
@fragment
fn fs_crush(i: VOut) -> @location(0) vec4f {
    let px = i.uv * u.res.xy;
    let buv = (floor(px / u.d.x) * u.d.x + 0.5) / u.res.xy;
    var c = tex0(buv);
    let l = max(u.d.y - 1.0, 1.0);
    c = vec4f(floor(c.rgb * l + 0.5) / l, c.a);
    return c;
}

fn tile1(t: f32) -> f32 {
    let q = t * u.d.x;
    let i = floor(q);
    var f = q - i;
    if (u.d.y >= 0.5 && gmod(i, 2.0) >= 0.5) { f = 1.0 - f; }
    return f;
}

@fragment
fn fs_tile(i: VOut) -> @location(0) vec4f {
    var uv = i.uv;
    if (abs(u.d.z) > 1e-6) {
        let aspect = u.res.x / u.res.y;
        let ang = u.d.z * 6.283185307;
        let s = sin(ang);
        let c = cos(ang);
        let d = vec2f((uv.x - 0.5) * aspect, uv.y - 0.5);
        uv = vec2f((d.x * c - d.y * s) / aspect + 0.5, d.x * s + d.y * c + 0.5);
    }
    let suv = vec2f(tile1(uv.x), tile1(uv.y));
    let px = min(vec2i(suv * u.res.xy), vec2i(u.res.xy) - 1);
    return textureLoad(t0, px, 0);
}

// Delay: t1 ghost, res.zw shift, c.z mix.
@fragment
fn fs_delay(i: VOut) -> @location(0) vec4f {
    let cur = tex0(i.uv);
    let guv = i.uv - u.res.zw;
    var g = tex1(guv) * u.c.z;
    if (guv.x < 0.0 || guv.x > 1.0 || guv.y < 0.0 || guv.y > 1.0) { g = vec4f(0.0); }
    return cur + g * (1.0 - cur.a) + vec4f(g.rgb * cur.a * (1.0 - cur.rgb), 0.0);
}

// Reverb accumulate: t0 accum, t1 current, d.x decay.
@fragment
fn fs_racc(i: VOut) -> @location(0) vec4f {
    return tex0(i.uv) * u.d.x + tex1(i.uv) * (1.0 - u.d.x);
}

// Reverb mix: t0 current, t1 accum, d.x blur radius, c.z mix.
@fragment
fn fs_rmix(i: VOut) -> @location(0) vec4f {
    let cur = tex0(i.uv);
    var sm = vec4f(0.0);
    var wsum = 0.0;
    for (var x = -2; x <= 2; x++) {
        for (var y = -2; y <= 2; y++) {
            let w = 1.0 / (1.0 + f32(x * x + y * y));
            sm += tex1(i.uv + vec2f(f32(x), f32(y)) * u.d.x / u.res.xy) * w;
            wsum += w;
        }
    }
    sm = sm / wsum * u.c.z;
    return cur + sm * (1.0 - cur.a) + vec4f(sm.rgb * cur.a * (1.0 - cur.rgb), 0.0);
}

fn dctb(uu: i32, x: i32) -> f32 {
    let a = select(0.5, 0.35355339, uu == 0);
    return a * cos((2.0 * f32(x) + 1.0) * f32(uu) * 3.14159265 / 16.0);
}

@fragment
fn fs_dct_row(i: VOut) -> @location(0) vec4f {
    let px = floor(i.uv * u.res.xy);
    let uu = i32(gmod(px.x, 8.0));
    let bx = px.x - f32(uu);
    var s = vec4f(0.0);
    for (var k = 0; k < 8; k++) {
        let uv = (vec2f(bx + f32(k), px.y) + 0.5) / u.res.xy;
        s += (tex0(uv) - 0.5) * dctb(uu, k);
    }
    return s;
}

@fragment
fn fs_dct_col(i: VOut) -> @location(0) vec4f {
    let px = floor(i.uv * u.res.xy);
    let v = i32(gmod(px.y, 8.0));
    let by = px.y - f32(v);
    var s = vec4f(0.0);
    for (var k = 0; k < 8; k++) {
        let uv = (vec2f(px.x, by + f32(k)) + 0.5) / u.res.xy;
        s += tex0(uv) * dctb(v, k);
    }
    return s;
}

@fragment
fn fs_quant(i: VOut) -> @location(0) vec4f {
    let px = floor(i.uv * u.res.xy);
    let uu = gmod(px.x, 8.0);
    let v = gmod(px.y, 8.0);
    var c = tex0(i.uv);
    let dc = tex0((px - vec2f(uu, v) + 0.5) / u.res.xy);
    let q = (1.0 + u.d.x * (8.0 + 16.0 * (uu + v))) / 255.0;
    c = floor(c / q + 0.5) * q;
    if (uu + v > 0.5) {
        let keep = step(vec4f(u.d.y) * (abs(dc) + vec4f(0.02)), abs(c));
        c *= keep;
    }
    return c;
}

@fragment
fn fs_idct_col(i: VOut) -> @location(0) vec4f {
    let px = floor(i.uv * u.res.xy);
    let y = i32(gmod(px.y, 8.0));
    let by = px.y - f32(y);
    var s = vec4f(0.0);
    for (var k = 0; k < 8; k++) {
        let uv = (vec2f(px.x, by + f32(k)) + 0.5) / u.res.xy;
        s += tex0(uv) * dctb(k, y);
    }
    return s;
}

@fragment
fn fs_idct_row(i: VOut) -> @location(0) vec4f {
    let px = floor(i.uv * u.res.xy);
    let x = i32(gmod(px.x, 8.0));
    let bx = px.x - f32(x);
    var s = vec4f(0.0);
    for (var k = 0; k < 8; k++) {
        let uv = (vec2f(bx + f32(k), px.y) + 0.5) / u.res.xy;
        s += tex0(uv) * dctb(k, x);
    }
    return clamp(s + 0.5, vec4f(0.0), vec4f(1.0));
}

@fragment
fn fs_iso_blur_h(i: VOut) -> @location(0) vec4f {
    let w = i32(u.res.x);
    let h = i32(u.res.y);
    let px = clamp(vec2i(floor(i.uv * u.res.xy)), vec2i(0), vec2i(w - 1, h - 1));
    let rad = i32(round(u.d.x));
    let x0 = max(px.x - rad, 0);
    let x1 = min(px.x + rad, w - 1);
    let inv = 1.0 / f32(x1 - x0 + 1);
    var acc = vec3f(0.0);
    for (var xx = x0; xx <= x1; xx++) {
        acc += textureLoad(t0, vec2i(xx, px.y), 0).rgb;
    }
    return vec4f(acc * inv, 1.0);
}

@fragment
fn fs_iso_blur_v(i: VOut) -> @location(0) vec4f {
    let w = i32(u.res.x);
    let h = i32(u.res.y);
    let px = clamp(vec2i(floor(i.uv * u.res.xy)), vec2i(0), vec2i(w - 1, h - 1));
    let rad = i32(round(u.d.x));
    let y0 = max(px.y - rad, 0);
    let y1 = min(px.y + rad, h - 1);
    let inv = 1.0 / f32(y1 - y0 + 1);
    var acc = vec3f(0.0);
    for (var yy = y0; yy <= y1; yy++) {
        acc += textureLoad(t0, vec2i(px.x, yy), 0).rgb;
    }
    return vec4f(acc * inv, 1.0);
}

@fragment
fn fs_iso_combine(i: VOut) -> @location(0) vec4f {
    let w = i32(u.res.x);
    let h = i32(u.res.y);
    let px = clamp(vec2i(floor(i.uv * u.res.xy)), vec2i(0), vec2i(w - 1, h - 1));
    let bm = textureLoad(t0, px, 0).rgb;
    if (u.b.y > 0.5 && u.cf.w < 0.05) {
        return vec4f(bm, 1.0);
    }
    let bc = textureLoad(t1, px, 0).rgb;
    let y_luma = clamp(0.2126 * bc.r + 0.7152 * bc.g + 0.0722 * bc.b, 0.0, 1.0);
    let inv_y = 1.0 - y_luma;
    let c0 = 0.35 + 0.65 * inv_y * inv_y * inv_y;
    let c1 = 0.65 * 3.0 * y_luma * inv_y * inv_y;
    let c2 = 0.65 * 3.0 * y_luma * y_luma * inv_y;
    let c3 = 0.65 * y_luma * y_luma * y_luma;
    let w_bc = u.cf.x * c0 + u.cf.y * c1 + u.cf.z * c2 + u.cf.w * (c3 - 1.0);
    let w_bm = u.cf.w - u.b.x;
    return vec4f(w_bc * bc + w_bm * bm, 1.0);
}

@fragment
fn fs_isolator(i: VOut) -> @location(0) vec4f {
    let w = i32(u.res.x);
    let h = i32(u.res.y);
    let px = clamp(vec2i(floor(i.uv * u.res.xy)), vec2i(0), vec2i(w - 1, h - 1));
    let orig4 = textureLoad(t0, px, 0);
    if (orig4.a <= 0.0) {
        return orig4;
    }
    let orig = orig4.rgb;
    let uv = (vec2f(px) + 0.5) / u.res.xy;
    let dx = (uv.x - 0.5) * 1.15;
    let dy = uv.y - 0.5;
    let dist = sqrt(dx * dx + dy * dy);
    let t_rad = clamp((0.70 - dist) / (0.70 - 0.16), 0.0, 1.0);
    let w_mid = t_rad * t_rad * (3.0 - 2.0 * t_rad);
    let w_side = 1.0 - w_mid;

    var rgb = orig;
    if (u.b.z > 0.5) {
        let p1 = textureLoad(t1, px, 0).rgb;
        var band_sum = p1 + u.b.x * orig;
        if (u.b.y > 0.5 && u.cf.w < 0.05) {
            band_sum = min(abs(orig - p1) * (2.4 * u.b.x), vec3f(1.0));
        }
        let ch_m = i32(round(u.d.x));
        if (ch_m == 1) {
            rgb = band_sum * w_mid + orig * w_side;
        } else if (ch_m == 2) {
            rgb = orig * w_mid + band_sum * w_side;
        } else {
            rgb = band_sum;
        }
    }

    let mon_m = i32(round(u.d.y));
    var mon_mask = 1.0;
    if (mon_m == 1) {
        mon_mask = 0.18 + 0.82 * clamp((0.54 - uv.x) / 0.08, 0.0, 1.0);
    } else if (mon_m == 2) {
        mon_mask = 0.18 + 0.82 * clamp((uv.x - 0.46) / 0.08, 0.0, 1.0);
    } else if (mon_m == 3) {
        mon_mask = 0.16 + 0.84 * w_mid;
    } else if (mon_m == 4) {
        mon_mask = 0.14 + 0.86 * w_side;
    }
    let mon_blend = 1.0 + (mon_mask - 1.0) * u.b.w;
    let out_rgb = clamp(rgb * (mon_blend * u.d.z), vec3f(0.0), vec3f(orig4.a));
    return vec4f(out_rgb, orig4.a);
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BlendKind {
    None,
    PremulOver,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Pipe {
    Grain,
    GrainScreen,
    BlitBlend,
    BlitScreen,
    Blit,
    Bleed,
    Kaleido,
    TapeStop,
    Crush,
    Tint,
    Tune,
    Tile,
    Delay,
    Racc,
    Rmix,
    DctRow,
    DctCol,
    Quant,
    IdctCol,
    IdctRow,
    IsoBlurH,
    IsoBlurV,
    IsoCombine,
    Isolator,
}

impl Pipe {
    const ALL: [Pipe; 24] = [
        Pipe::Grain,
        Pipe::GrainScreen,
        Pipe::BlitBlend,
        Pipe::BlitScreen,
        Pipe::Blit,
        Pipe::Bleed,
        Pipe::Kaleido,
        Pipe::TapeStop,
        Pipe::Crush,
        Pipe::Tint,
        Pipe::Tune,
        Pipe::Tile,
        Pipe::Delay,
        Pipe::Racc,
        Pipe::Rmix,
        Pipe::DctRow,
        Pipe::DctCol,
        Pipe::Quant,
        Pipe::IdctCol,
        Pipe::IdctRow,
        Pipe::IsoBlurH,
        Pipe::IsoBlurV,
        Pipe::IsoCombine,
        Pipe::Isolator,
    ];

    fn entry(self) -> &'static str {
        match self {
            Pipe::Grain | Pipe::GrainScreen => "fs_grain",
            Pipe::BlitBlend | Pipe::BlitScreen | Pipe::Blit => "fs_blit",
            Pipe::Bleed => "fs_bleed",
            Pipe::Kaleido => "fs_kaleido",
            Pipe::TapeStop => "fs_tapestop",
            Pipe::Crush => "fs_crush",
            Pipe::Tint => "fs_tint",
            Pipe::Tune => "fs_tune",
            Pipe::Tile => "fs_tile",
            Pipe::Delay => "fs_delay",
            Pipe::Racc => "fs_racc",
            Pipe::Rmix => "fs_rmix",
            Pipe::DctRow => "fs_dct_row",
            Pipe::DctCol => "fs_dct_col",
            Pipe::Quant => "fs_quant",
            Pipe::IdctCol => "fs_idct_col",
            Pipe::IdctRow => "fs_idct_row",
            Pipe::IsoBlurH => "fs_iso_blur_h",
            Pipe::IsoBlurV => "fs_iso_blur_v",
            Pipe::IsoCombine => "fs_iso_combine",
            Pipe::Isolator => "fs_isolator",
        }
    }

    fn blend(self) -> BlendKind {
        match self {
            Pipe::Grain | Pipe::BlitBlend => BlendKind::PremulOver,
            Pipe::GrainScreen | Pipe::BlitScreen => BlendKind::Screen,
            _ => BlendKind::None,
        }
    }

    fn format(self) -> wgpu::TextureFormat {
        match self {
            Pipe::DctRow
            | Pipe::DctCol
            | Pipe::Quant
            | Pipe::IdctCol
            | Pipe::IsoBlurH
            | Pipe::IsoBlurV
            | Pipe::IsoCombine => RGBA16F,
            _ => RGBA8,
        }
    }
}

struct Target {
    #[cfg_attr(not(test), allow(dead_code))]
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

struct GpuSource {
    view: wgpu::TextureView,
    layers: i32,
    duration: f64,
    src_aspect: f32,
    ident: usize,
}

#[derive(Default)]
struct FxInst {
    history: Vec<usize>,
    head: usize,
    filled: usize,
    accum: Option<[usize; 2]>,
    accum_idx: usize,
    kaleido_angle: f32,
}

struct DrawOp {
    pipe: Pipe,
    t0: Option<usize>,
    t1: Option<usize>,
    arr: Option<usize>,
    u: Uni,
}

struct PassOp {
    target: usize,
    clear: Option<wgpu::Color>,
    draws: Vec<DrawOp>,
}

type TexKey = (u32, u32, u32);

fn key_part(x: Option<usize>) -> u32 {
    x.map(|v| v as u32).unwrap_or(u32::MAX)
}

pub struct GpuPreview {
    device: wgpu::Device,
    queue: wgpu::Queue,
    tex_layout: wgpu::BindGroupLayout,
    uni_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<Pipe, wgpu::RenderPipeline>,
    present_pipe: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    dummy_2d: wgpu::TextureView,
    dummy_arr: wgpu::TextureView,
    uni_buf: wgpu::Buffer,
    uni_slots: u64,
    uni_bg: wgpu::BindGroup,
    uni_staging: Vec<u8>,
    present_buf: wgpu::Buffer,
    present_bg: wgpu::BindGroup,
    present_next: usize,
    tex_bgs: HashMap<TexKey, wgpu::BindGroup>,
    size: (u32, u32),
    targets: Vec<Target>,
    sources: HashMap<usize, GpuSource>,
    fx: HashMap<u64, FxInst>,
    passes: Vec<PassOp>,
    /// Slab index of the last rendered canvas (what `present` samples).
    canvas_tex: Option<usize>,
    last_frame: Option<u64>,
    /// `52 ms` optical carry timestamp (holds last active frame across rapid cuts).
    carry_active_t: Option<f64>,
    pub renders: u64,
}

fn make_tex(device: &wgpu::Device, w: u32, h: u32, layers: u32, format: wgpu::TextureFormat, target: bool) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    if target {
        usage |= wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("chromagrain"),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: layers.max(1) },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn array_view(tex: &wgpu::Texture) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

impl GpuPreview {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, target_format: wgpu::TextureFormat) -> GpuPreview {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chromagrain preview"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uni_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chromagrain uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNI_SIZE),
                },
                count: None,
            }],
        });
        let tex_entry = |binding: u32, dim: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chromagrain textures"),
            entries: &[
                tex_entry(0, wgpu::TextureViewDimension::D2),
                tex_entry(1, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(3, wgpu::TextureViewDimension::D2Array),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chromagrain"),
            bind_group_layouts: &[&uni_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let premul = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let screen_rgb = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        let make_pipe = |entry: &str, format: wgpu::TextureFormat, blend: BlendKind| {
            let blend_state = match blend {
                BlendKind::None => None,
                BlendKind::PremulOver => Some(wgpu::BlendState { color: premul, alpha: premul }),
                BlendKind::Screen => Some(wgpu::BlendState { color: screen_rgb, alpha: premul }),
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: blend_state,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipelines = Pipe::ALL.iter().map(|&p| (p, make_pipe(p.entry(), p.format(), p.blend()))).collect();
        let present_pipe = make_pipe("fs_present", target_format, BlendKind::None);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("chromagrain linear clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy_2d = make_tex(device, 1, 1, 1, RGBA8, false).create_view(&Default::default());
        let dummy_arr = array_view(&make_tex(device, 1, 1, 1, RGBA8, false));

        let (uni_buf, uni_bg) = Self::make_uni(device, &uni_layout, 64);
        let (present_buf, present_bg) = Self::make_uni(device, &uni_layout, PRESENT_SLOTS as u64);
        GpuPreview {
            device: device.clone(),
            queue: queue.clone(),
            tex_layout,
            uni_layout,
            pipelines,
            present_pipe,
            sampler,
            dummy_2d,
            dummy_arr,
            uni_buf,
            uni_slots: 64,
            uni_bg,
            uni_staging: Vec::new(),
            present_buf,
            present_bg,
            present_next: 0,
            tex_bgs: HashMap::new(),
            size: (0, 0),
            targets: Vec::new(),
            sources: HashMap::new(),
            fx: HashMap::new(),
            passes: Vec::new(),
            canvas_tex: None,
            last_frame: None,
            carry_active_t: None,
            renders: 0,
        }
    }

    fn make_uni(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, slots: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chromagrain uniforms"),
            size: slots * UNI_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chromagrain uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(UNI_SIZE),
                }),
            }],
        });
        (buf, bg)
    }

    fn new_target(&mut self, float: bool) -> usize {
        let tex = make_tex(&self.device, self.size.0, self.size.1, 1, if float { RGBA16F } else { RGBA8 }, true);
        let view = tex.create_view(&Default::default());
        self.targets.push(Target { tex, view });
        self.targets.len() - 1
    }

    fn ensure_targets(&mut self, w: u32, h: u32) {
        if self.size == (w, h) && !self.targets.is_empty() {
            return;
        }
        self.size = (w, h);
        self.targets.clear();
        self.tex_bgs.clear();
        self.fx.clear();
        for i in 0..FIXED_TARGETS {
            self.new_target(i == FPING || i == FPONG || i == FISO);
        }
        self.canvas_tex = None;
        self.carry_active_t = None;
    }

    fn ensure_source(&mut self, project: &Project, sid: usize) -> Option<()> {
        let video = project.sources.get(sid)?.video.as_ref()?;
        let ident = Arc::as_ptr(video) as usize;
        if self.sources.get(&sid).is_some_and(|s| s.ident == ident) {
            return Some(());
        }
        let n_frames = video.frames.len().min(1_000_000);
        if n_frames == 0 {
            return None;
        }
        let take = n_frames.min(MAX_SRC_LAYERS);
        let (sw, sh) = (video.frames[0].width, video.frames[0].height);
        let src_aspect = sw.max(1) as f32 / sh.max(1) as f32;
        let scale = (MAX_SRC_W as f32 / sw as f32).min(1.0);
        let (tw, th) = (((sw as f32 * scale) as u32).max(2), ((sh as f32 * scale) as u32).max(2));
        let mut data = vec![0u8; (tw * th * 4) as usize * take];
        for l in 0..take {
            let fi = l * n_frames / take;
            let f = &video.frames[fi];
            let base = (tw * th * 4) as usize * l;
            for y in 0..th {
                let sy = ((y as f32 / scale) as u32).min(f.height - 1);
                for x in 0..tw {
                    let sx = ((x as f32 / scale) as u32).min(f.width - 1);
                    let si = ((sy * f.width + sx) * 4) as usize;
                    let di = base + ((y * tw + x) * 4) as usize;
                    data[di..di + 4].copy_from_slice(&f.data[si..si + 4]);
                }
            }
        }
        let tex = make_tex(&self.device, tw, th, take as u32, RGBA8, false);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tw * 4), rows_per_image: Some(th) },
            wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: take as u32 },
        );
        let view = array_view(&tex);
        self.sources.insert(
            sid,
            GpuSource {
                view,
                layers: take as i32,
                duration: video.duration(),
                src_aspect,
                ident,
            },
        );
        self.tex_bgs.retain(|k, _| k.2 != sid as u32);
        Some(())
    }

    // ------------------------------------------------------------ recording

    fn pass(&mut self, target: usize, clear: Option<[f64; 4]>) {
        let clear = clear.map(|c| wgpu::Color { r: c[0], g: c[1], b: c[2], a: c[3] });
        self.passes.push(PassOp { target, clear, draws: Vec::new() });
    }

    fn draw(&mut self, pipe: Pipe, t0: Option<usize>, t1: Option<usize>, arr: Option<usize>, u: Uni) {
        if let Some(p) = self.passes.last_mut() {
            p.draws.push(DrawOp { pipe, t0, t1, arr, u });
        }
    }

    fn full(&mut self, target: usize, pipe: Pipe, t0: usize, t1: Option<usize>, u: Uni) {
        self.pass(target, Some([0.0; 4]));
        self.draw(pipe, Some(t0), t1, None, u);
    }

    fn res_uni(&self) -> Uni {
        Uni { res: [self.size.0 as f32, self.size.1 as f32, 0.0, 0.0], ..Default::default() }
    }

    fn other(cur: usize) -> usize {
        if cur == PING {
            PONG
        } else {
            PING
        }
    }

    /// Record an effect chain over `cur` (slab index), returning the slab
    /// index holding the result.
    fn apply_chain(&mut self, chain: &[AvEffect], scope: u64, mut cur: usize, fps: f32, _t_sec: f64) -> usize {
        for (fi, fx) in chain.iter().enumerate() {
            let key = scope ^ ((fi as u64 + 1) << 40);
            if fx.video <= 0.001 {
                if let EffectKind::Delay { time, .. } = fx.kind {
                    let frames = ((time * fps).round() as usize).clamp(1, MAX_HISTORY);
                    self.push_history(key, frames, cur, 1.0);
                } else if let EffectKind::Kaleido { spin, .. } = fx.kind {
                    let inst = self.fx.entry(key).or_default();
                    inst.kaleido_angle = (inst.kaleido_angle
                        + spin.clamp(-1.0, 1.0) * std::f32::consts::TAU / fps.max(1.0))
                    .rem_euclid(std::f32::consts::TAU);
                }
                continue;
            }
            let out = Self::other(cur);
            #[allow(unreachable_patterns)]
            match fx.kind {
                EffectKind::Tint { hue, amount } => {
                    let mut u = self.res_uni();
                    u.d[0] = hue;
                    u.c[3] = (amount * fx.video).clamp(0.0, 1.0);
                    self.full(out, Pipe::Tint, cur, None, u);
                    cur = out;
                }
                EffectKind::Tune { root, scale, amount, .. } => {
                    let mut u = self.res_uni();
                    let hues = chromagrain_core::fx::key_hues(root, scale);
                    for (a, h) in u.hues.iter_mut().zip(&hues) {
                        *a = *h;
                    }
                    u.d[3] = hues.len().min(12) as f32;
                    u.c[3] = (amount * fx.video).clamp(0.0, 1.0);
                    self.full(out, Pipe::Tune, cur, None, u);
                    cur = out;
                }
                EffectKind::Tile { count, mirror, spin } => {
                    let (n, turn) = chromagrain_core::fx::tile_video_params(count, spin, fx.video);
                    if (n - 1.0).abs() < 1e-4 && turn.abs() < 1e-6 {
                        continue;
                    }
                    let mut u = self.res_uni();
                    u.d = [n, mirror, turn, 0.0];
                    self.full(out, Pipe::Tile, cur, None, u);
                    cur = out;
                }
                EffectKind::Crush { downsample, bits } => {
                    let mut u = self.res_uni();
                    let block = 1.0 + (downsample - 1.0) * fx.video;
                    let levels = 2.0f32.powf(8.0 + (bits.clamp(1.0, 8.0) - 8.0) * fx.video);
                    u.d = [block.max(1.0), levels, 0.0, 0.0];
                    self.full(out, Pipe::Crush, cur, None, u);
                    cur = out;
                }
                EffectKind::Delay { time, feedback, mix, shift_x, shift_y } => {
                    let frames = ((time * fps).round() as usize).clamp(1, MAX_HISTORY);
                    let ghost = self.history_tail(key, frames);
                    let mut u = self.res_uni();
                    u.res[2] = shift_x;
                    u.res[3] = shift_y;
                    u.c[2] = if ghost.is_some() { mix * fx.video } else { 0.0 };
                    self.full(out, Pipe::Delay, cur, Some(ghost.unwrap_or(cur)), u);
                    cur = out;
                    self.push_history(key, frames, cur, feedback);
                }
                EffectKind::Reverb { size, damp, mix } => {
                    self.ensure_accum(key);
                    let inst = &self.fx[&key];
                    let Some(acc) = inst.accum else { continue };
                    let (read, write) = (acc[inst.accum_idx], acc[1 - inst.accum_idx]);
                    let mut u = Uni::default();
                    u.d[0] = 0.75 + 0.24 * size.clamp(0.0, 1.0);
                    self.full(write, Pipe::Racc, read, Some(cur), u);
                    let inst = self.fx.get_mut(&key).unwrap();
                    inst.accum_idx = 1 - inst.accum_idx;
                    let mut u = self.res_uni();
                    u.d[0] = 1.0 + damp * 5.0;
                    u.c[2] = mix * fx.video;
                    self.full(out, Pipe::Rmix, cur, Some(write), u);
                    cur = out;
                }
                EffectKind::Compress { quality } => {
                    let q = 1.0 + (quality - 1.0) * fx.video;
                    if q >= 0.999 {
                        continue;
                    }
                    let crunch = (1.0 - q) * (1.0 - q);
                    let u = self.res_uni();
                    self.full(FPING, Pipe::DctRow, cur, None, u);
                    self.full(FPONG, Pipe::DctCol, FPING, None, u);
                    let mut uq = u;
                    uq.d = [crunch, crunch * 0.35, 0.0, 0.0];
                    self.full(FPING, Pipe::Quant, FPONG, None, uq);
                    self.full(FPONG, Pipe::IdctCol, FPING, None, u);
                    self.full(out, Pipe::IdctRow, FPONG, None, u);
                    cur = out;
                }
                EffectKind::Kaleido { folds, spin } => {
                    let angle = {
                        let inst = self.fx.entry(key).or_default();
                        let cur_ang = inst.kaleido_angle;
                        inst.kaleido_angle = (inst.kaleido_angle
                            + spin.clamp(-1.0, 1.0) * std::f32::consts::TAU / fps.max(1.0))
                        .rem_euclid(std::f32::consts::TAU);
                        cur_ang
                    };
                    let mut u = self.res_uni();
                    u.d = [folds.clamp(2.0, 12.0), angle, 0.5, 0.5];
                    u.c[3] = fx.video.clamp(0.0, 1.0);
                    self.full(out, Pipe::Kaleido, cur, None, u);
                    cur = out;
                }
                EffectKind::Isolator(iso) => {
                    cur = self.apply_isolator(&iso, fx.video, cur);
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        cur
    }

    fn apply_isolator(&mut self, iso: &chromagrain_core::dsp::BandIsolator, amount: f32, cur: usize) -> usize {
        let amt = amount.clamp(0.0, 1.0);
        if amt <= 1e-4 || !iso.is_active() {
            return cur;
        }
        let (g, ch_mode, mon_mode, eff_exp) = chromagrain_core::fx::isolator_video_uniforms(iso, amt);
        let filter_on = g.iter().any(|&v| (v - 1.0).abs() > 1e-4);
        let only_high_solo = filter_on && g[0] < 0.05 && g[1] < 0.05 && g[2] < 0.05 && (g[3] > 0.5 || g[4] > 0.5);

        if filter_on {
            let min_dim = self.size.0.min(self.size.1).max(1) as usize;
            let r_med = (min_dim / 42).clamp(2, 10) as f32;
            let r_coarse = (min_dim / 12).clamp(6, 28) as f32;

            let mut um = self.res_uni();
            um.d[0] = r_med;
            self.full(FPING, Pipe::IsoBlurH, cur, None, um);
            self.full(FPONG, Pipe::IsoBlurV, FPING, None, um);

            let mut uc = self.res_uni();
            uc.d[0] = r_coarse;
            self.full(FPING, Pipe::IsoBlurH, cur, None, uc);
            self.full(FISO, Pipe::IsoBlurV, FPING, None, uc);

            let mut ucomb = self.res_uni();
            ucomb.cf = [g[0], g[1], g[2], g[3]];
            ucomb.b = [g[4], if only_high_solo { 1.0 } else { 0.0 }, 0.0, 0.0];
            self.full(FPING, Pipe::IsoCombine, FPONG, Some(FISO), ucomb);
        }

        let out = Self::other(cur);
        let mut u = self.res_uni();
        u.cf = [g[0], g[1], g[2], g[3]];
        u.b = [
            g[4],
            if only_high_solo { 1.0 } else { 0.0 },
            if filter_on { 1.0 } else { 0.0 },
            amt,
        ];
        u.d = [ch_mode as f32, mon_mode as f32, eff_exp, 0.0];
        self.full(out, Pipe::Isolator, cur, if filter_on { Some(FPING) } else { None }, u);
        out
    }

    fn ensure_accum(&mut self, key: u64) {
        if self.fx.get(&key).is_some_and(|i| i.accum.is_some()) {
            return;
        }
        let a = self.new_target(false);
        let b = self.new_target(false);
        self.pass(a, Some([0.0; 4]));
        self.pass(b, Some([0.0; 4]));
        self.fx.entry(key).or_default().accum = Some([a, b]);
    }

    fn history_tail(&self, key: u64, frames: usize) -> Option<usize> {
        let inst = self.fx.get(&key)?;
        if inst.filled < frames || inst.history.len() < frames {
            return None;
        }
        let idx = (inst.head + inst.history.len() - frames) % inst.history.len();
        Some(inst.history[idx])
    }

    fn push_history(&mut self, key: u64, frames: usize, cur: usize, feedback: f32) {
        let need = frames.min(MAX_HISTORY);
        while self.fx.entry(key).or_default().history.len() < need {
            let t = self.new_target(false);
            self.fx.get_mut(&key).unwrap().history.push(t);
        }
        let slot = {
            let inst = &self.fx[&key];
            inst.history[inst.head % need]
        };
        let u = Uni { c: [feedback, 1.0, 0.0, 0.0], ..Default::default() };
        self.full(slot, Pipe::Blit, cur, None, u);
        let inst = self.fx.get_mut(&key).unwrap();
        inst.head = (inst.head % need + 1) % need;
        inst.filled = (inst.filled + 1).min(need + 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn record_grains_ext(
        &mut self,
        sid: usize,
        draws: Vec<(GrainDraw, bool, u32, f32)>,
        color_filter: Option<&chromagrain_core::video::ColorFilter>,
        additive: f32,
        feather: f32,
        screen_blend: bool,
    ) {
        let Some(gsrc) = self.sources.get(&sid) else { return };
        let (layers, dur) = (gsrc.layers, gsrc.duration.max(1e-6));
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let (mode, cf, valmin) = match color_filter {
            None => (0.0, [0.0f32; 4], 0.0),
            Some(c) => (
                if c.mode == ColorFilterMode::Keep { 1.0 } else { 2.0 },
                [c.hue_center, c.hue_width / 2.0, c.softness, c.sat_min],
                c.val_min,
            ),
        };
        let pipe = if screen_blend { Pipe::GrainScreen } else { Pipe::Grain };
        for (d, flip_x, edge_mask, oxide) in draws {
            let layer = ((d.src_time / dur).rem_euclid(1.0) * layers as f64).min(layers as f64 - 1.0) as f32;
            let (eff_add, eff_feather) = if d.tiled || edge_mask != 0 {
                (0.0, 0.0)
            } else {
                (additive, feather)
            };
            let u = Uni {
                rect: d.dest,
                patch: d.patch,
                cf,
                a: [d.rotation, aspect, layer.floor(), d.alpha],
                b: [d.hue_shift, mode, valmin, eff_add],
                d: [eff_feather, if flip_x { 1.0 } else { 0.0 }, edge_mask as f32, oxide],
                ..Default::default()
            };
            self.draw(pipe, None, None, Some(sid), u);
        }
    }

    fn blend_onto(&mut self, target: usize, tex: usize, opacity: f32, additive: f32) {
        self.pass(target, None);
        let u = Uni { c: [opacity, 1.0, 0.0, 0.0], b: [0.0, 0.0, 0.0, additive], ..Default::default() };
        self.draw(Pipe::BlitBlend, Some(tex), None, None, u);
    }

    fn screen_onto(&mut self, target: usize, tex: usize, opacity: f32) {
        self.pass(target, None);
        let u = Uni { c: [opacity, 1.0, 0.0, 0.0], ..Default::default() };
        self.draw(Pipe::BlitScreen, Some(tex), None, None, u);
    }

    fn bleed_onto(&mut self, target: usize, tex: usize, weight: f32) {
        let tmp = Self::other(tex);
        let u = Uni { c: [weight.clamp(0.0, 1.0), 1.0, 0.0, 0.0], ..Default::default() };
        self.full(tmp, Pipe::Bleed, target, Some(tex), u);
        self.full(target, Pipe::Blit, tmp, None, Uni::default());
    }

    /// Render `Split + Bleed` (`StageMode::Split`) across active timeline & live voices:
    /// - Foreground discrete voices (`1..9`) get full-canvas `1-up / 2-up / 3-up / 2×2`
    ///   panels (`1×2` in `9:16`, `2×1` in `16:9`/`1:1`) sorted by `(slot, pitch_ratio)`
    ///   with `+6.5%` screen-blended feathered interior seams!
    /// - Sustained `Granular` / wash voices bleed across `100%` of the frame at `~30%`!
    fn render_split_stage(&mut self, scene: &Scene, aspect: f32, fps: f32) -> bool {
        #[derive(Clone)]
        struct SplitVoice {
            sid: usize,
            src_time: f64,
            alpha: f32,
            hue_shift: f32,
            pitch_ratio: f32,
            zoom_punch: f32,
            oxide: f32,
            flip_x: bool,
            tag_focus: (f32, f32),
            slot_key: u32,
            effects: Vec<AvEffect>,
            fx_scope: u64,
        }

        let mut fg_voices: Vec<SplitVoice> = Vec::new();
        let mut wash_voices: Vec<SplitVoice> = Vec::new();
        let t = scene.t;

        for (ti, track) in scene.project.tracks.iter().enumerate() {
            if track.muted {
                continue;
            }
            for cp in scene.plan.iter().filter(|p| p.track == ti) {
                let Some(clip) = track.clips.get(cp.clip_index) else { continue };
                let hard_burst = clip
                    .grains
                    .adsr
                    .is_some_and(|a| a.picture == chromagrain_core::envelope::PictureCut::Cut)
                    && clip.grains.spray <= 0.08;
                let is_wash = !hard_burst && (matches!(clip.kind, ClipKind::Granular) || clip.grains.texture >= 0.55);
                let oxide = track.tape.age.clamp(0.0, 1.0);
                for (sid, evs) in cp.video_parts.as_deref().unwrap_or(&cp.parts) {
                    // Pick the strongest active event for this `(track, clip, sid, pitch)` voice.
                    let mut best: Option<&GrainEvent> = None;
                    for ev in evs {
                        let vis_end = ev.onset + cp.style.visual_duration(ev.duration);
                        if t >= ev.onset && t < vis_end && ev.tag.picture_at(t) > 0.01 {
                            if best.is_none_or(|b| ev.onset > b.onset) {
                                best = Some(ev);
                            }
                        }
                    }
                    let Some(ev) = best else { continue };
                    let dt = (t - ev.onset).max(0.0);
                    let rate = 1.0 + (ev.pitch_ratio - 1.0) * clip.link.pitch_to_rate;
                    let src_time = if ev.reverse && clip.link.reverse_video {
                        ev.source_pos + ev.duration as f64 * rate as f64 - dt * rate as f64
                    } else {
                        ev.source_pos + dt * rate as f64
                    };
                    let punch = if dt < 0.09 {
                        ((1.0 - dt as f32 / 0.09).powi(2)) * 0.085
                    } else {
                        0.0
                    }
                    .max(ev.tag.zoom_punch);
                    let mut fx = clip.effects.clone();
                    fx.extend(track.effects.iter().cloned());
                    let sv = SplitVoice {
                        sid: *sid,
                        src_time,
                        alpha: (ev.tag.picture_at(t) * track.opacity_at(t * scene.project.bpm / 60.0)).clamp(0.0, 1.0),
                        hue_shift: ev.semitones() * clip.link.pitch_to_hue,
                        pitch_ratio: ev.pitch_ratio,
                        zoom_punch: punch,
                        oxide: oxide.max(ev.tag.oxide_dropout),
                        flip_x: ev.tag.ratchet_odd,
                        tag_focus: ev.tag.focus,
                        slot_key: ev.tag.tile.map(|x| x as u32).unwrap_or(ti as u32),
                        effects: fx,
                        fx_scope: ((ti as u64 + 1) << 28) | (*sid as u64 + 1),
                    };
                    if is_wash || ev.tag.is_wash {
                        wash_voices.push(sv);
                    } else {
                        fg_voices.push(sv);
                    }
                }
            }
        }

        // Live voices (pads & emitters).
        let mut by_voice: HashMap<(u8, u64), (usize, &GrainEvent)> = HashMap::new();
        for (sid, bus, ev) in &scene.live {
            if scene.live_t >= ev.onset && scene.live_t < ev.end() && ev.tag.picture_at(scene.live_t) > 0.01 {
                let vk = (
                    *bus,
                    if ev.tag.voice != 0 { ev.tag.voice } else { (ev.pitch_ratio * 1000.0) as u64 },
                );
                let replace = by_voice.get(&vk).is_none_or(|(_, prev)| ev.onset >= prev.onset);
                if replace {
                    by_voice.insert(vk, (*sid, ev));
                }
            }
        }
        for ((bus, _), (sid, ev)) in by_voice {
            self.ensure_source(&scene.project, sid);
            let (link, fx, is_wash) = if bus == 0 || bus as usize > scene.buses.len() {
                (chromagrain_core::video::AvLink::default(), Vec::new(), false)
            } else {
                let b = &scene.buses[bus as usize - 1];
                (b.link, b.effects.clone(), b.style.min_size < 0.85 && b.style.additive > 0.22 && fg_voices.len() >= 2)
            };
            let dt = (scene.live_t - ev.onset).max(0.0);
            let rate = 1.0 + (ev.pitch_ratio - 1.0) * link.pitch_to_rate;
            let src_time = if ev.reverse && link.reverse_video {
                ev.source_pos + ev.duration as f64 * rate as f64 - dt * rate as f64
            } else {
                ev.source_pos + dt * rate as f64
            };
            let punch = if dt < 0.09 {
                ((1.0 - dt as f32 / 0.09).powi(2)) * 0.09
            } else {
                0.0
            }
            .max(ev.tag.zoom_punch);
            let sv = SplitVoice {
                sid,
                src_time,
                alpha: ev.tag.picture_at(scene.live_t).clamp(0.0, 1.0),
                hue_shift: ev.semitones() * link.pitch_to_hue,
                pitch_ratio: ev.pitch_ratio,
                zoom_punch: punch,
                oxide: ev.tag.oxide_dropout,
                flip_x: ev.tag.ratchet_odd,
                tag_focus: ev.tag.focus,
                slot_key: ev.tag.tile.map(|x| x as u32).unwrap_or(100 + bus as u32),
                effects: fx,
                fx_scope: 0xB500_0000_0000 | (bus as u64) << 16 | (sid as u64),
            };
            if is_wash || ev.tag.is_wash {
                wash_voices.push(sv);
            } else {
                fg_voices.push(sv);
            }
        }

        // If only wash voices are active, promote the primary wash to fill the stage cleanly!
        if fg_voices.is_empty() && !wash_voices.is_empty() {
            fg_voices.push(wash_voices.remove(0));
        }
        if fg_voices.is_empty() {
            return false;
        }

        // Sort foreground voices by `(slot_key, pitch_ratio)` so polyphonic chords
        // lay out low→high left-to-right!
        fg_voices.sort_by(|a, b| {
            a.slot_key
                .cmp(&b.slot_key)
                .then_with(|| a.pitch_ratio.total_cmp(&b.pitch_ratio))
        });
        fg_voices.truncate(9);

        let count = fg_voices.len();
        for (idx, v) in fg_voices.iter().enumerate() {
            let Some(gsrc) = self.sources.get(&v.sid) else { continue };
            let src_ar = gsrc.src_aspect;
            let (dest, edge_mask) = split_panel_rect(idx, count, aspect);
            let cell_ar = (dest[2] * aspect) / dest[3].max(1e-4);
            let focus = scene.focus_overrides.get(&v.sid).copied().unwrap_or(v.tag_focus);
            let patch = cover_patch_focus(src_ar, cell_ar, focus, v.zoom_punch);
            let gd = GrainDraw {
                dest,
                rotation: 0.0,
                patch,
                src_time: v.src_time,
                alpha: v.alpha,
                hue_shift: v.hue_shift,
                tiled: false,
                flip_x: v.flip_x,
                oxide_dropout: v.oxide,
                is_wash: false,
                feather_edges: [
                    (edge_mask & 1) != 0,
                    (edge_mask & 2) != 0,
                    (edge_mask & 4) != 0,
                    (edge_mask & 8) != 0,
                ],
            };
            if v.effects.is_empty() {
                self.pass(MASTER, None);
                self.record_grains_ext(v.sid, vec![(gd, v.flip_x, edge_mask, v.oxide)], None, 0.0, 0.0, idx > 0);
            } else {
                self.pass(CLIP, Some([0.0; 4]));
                self.record_grains_ext(v.sid, vec![(gd, v.flip_x, edge_mask, v.oxide)], None, 0.0, 0.0, false);
                let fx_out = self.apply_chain(&v.effects, v.fx_scope, CLIP, fps, scene.t);
                if idx == 0 {
                    self.blend_onto(MASTER, fx_out, 1.0, 0.0);
                } else {
                    self.screen_onto(MASTER, fx_out, 1.0);
                }
            }
        }

        // Sustained Wash / TapeLoop voices bleed across 100% of the canvas at ~30%!
        for wv in wash_voices.iter().take(3) {
            let Some(gsrc) = self.sources.get(&wv.sid) else { continue };
            let src_ar = gsrc.src_aspect;
            let focus = scene.focus_overrides.get(&wv.sid).copied().unwrap_or(wv.tag_focus);
            let patch = cover_patch_focus(src_ar, aspect, focus, 0.0);
            let gd = GrainDraw {
                dest: [0.0, 0.0, 1.0, 1.0],
                rotation: 0.0,
                patch,
                src_time: wv.src_time,
                alpha: (0.26 + 0.14 * wv.alpha).clamp(0.24, 0.40),
                hue_shift: wv.hue_shift,
                tiled: false,
                flip_x: wv.flip_x,
                oxide_dropout: wv.oxide,
                is_wash: true,
                feather_edges: [false; 4],
            };
            self.pass(CLIP, Some([0.0; 4]));
            self.record_grains_ext(wv.sid, vec![(gd, wv.flip_x, 0, wv.oxide)], None, 0.0, 0.0, false);
            let fx_out = self.apply_chain(&wv.effects, wv.fx_scope ^ 0x7777, CLIP, fps, scene.t);
            self.bleed_onto(MASTER, fx_out, 1.0);
        }

        true
    }

    /// Render the whole scene offscreen; returns the commands to submit.
    pub fn render(&mut self, scene: &Scene) -> wgpu::CommandBuffer {
        let (w, h) = (scene.project.width.max(2), scene.project.height.max(2));
        self.ensure_targets(w, h);
        self.passes.clear();
        self.renders += 1;

        let needed: Vec<usize> = scene
            .plan
            .iter()
            .flat_map(|cp| cp.parts.iter().map(|(sid, _)| *sid))
            .chain(scene.live.iter().map(|(sid, _, _)| *sid))
            .collect();
        for sid in needed {
            self.ensure_source(&scene.project, sid);
        }

        let stage_mode = effective_stage_mode(scene);
        let clear_col = match stage_mode {
            StageMode::Split | StageMode::Bleed => [0.039, 0.039, 0.043, 1.0],
            StageMode::Stack | StageMode::Grid => [0.0, 0.0, 0.0, 1.0],
        };
        self.pass(MASTER, Some(clear_col));
        let mut any_drawn = false;

        if let Some((sid, at)) = scene.backdrop {
            self.ensure_source(&scene.project, sid);
            if let Some(g) = self.sources.get(&sid) {
                let layer = ((at / g.duration.max(1e-6)).rem_euclid(1.0) * g.layers as f64)
                    .min(g.layers as f64 - 1.0)
                    .floor() as f32;
                let focus = scene.focus_overrides.get(&sid).copied().unwrap_or((0.5, 0.5));
                let patch = cover_patch_focus(g.src_aspect, w as f32 / h as f32, focus, 0.0);
                let u = Uni { patch, a: [0.0, 1.0, layer, 1.0], ..Default::default() };
                self.draw(Pipe::Grain, None, None, Some(sid), u);
                any_drawn = true;
            }
        }

        let t = scene.t;
        let fps = scene.project.fps;
        let aspect = w as f32 / h as f32;

        if stage_mode == StageMode::Split {
            any_drawn |= self.render_split_stage(scene, aspect, fps);
        } else {
            let tgrid = tile_grid(scene);
            let is_bleed = stage_mode == StageMode::Bleed;

            for (ti, track) in scene.project.tracks.iter().enumerate() {
                if track.muted {
                    continue;
                }
                let plans: Vec<&ClipPlan> = scene.plan.iter().filter(|p| p.track == ti).collect();
                if plans.is_empty() {
                    continue;
                }
                self.pass(TRACK, Some([0.0; 4]));
                let mut track_any = !track.effects.is_empty();
                let oxide = track.tape.age.clamp(0.0, 1.0);

                for cp in &plans {
                    let Some(clip) = track.clips.get(cp.clip_index) else { continue };
                    let any_active = cp
                        .parts
                        .iter()
                        .any(|(_, evs)| evs.iter().any(|e| t >= e.onset && t < e.onset + cp.style.visual_duration(e.duration)));
                    if !any_active && clip.effects.is_empty() {
                        continue;
                    }
                    self.pass(CLIP, Some([0.0; 4]));
                    if any_active {
                        for (sid, evs) in &cp.parts {
                            let Some(gsrc) = self.sources.get(sid) else { continue };
                            let dur = gsrc.duration.max(1e-6);
                            let src_ar = gsrc.src_aspect;
                            let style: VisualGrainStyle = if is_bleed {
                                VisualGrainStyle::full_frame(0.0)
                            } else {
                                cp.style
                            };
                            let raw_draws = grain_draws_layout(
                                evs,
                                &style,
                                &clip.link,
                                &clip.transform,
                                dur,
                                t,
                                aspect,
                                scene.parked,
                                tgrid,
                            );
                            let focus_opt = scene.focus_overrides.get(sid).copied();
                            let ext_draws: Vec<(GrainDraw, bool, u32, f32)> = raw_draws
                                .into_iter()
                                .map(|mut gd| {
                                    if gd.tiled {
                                        if focus_opt.is_some() || (src_ar - aspect).abs() > 1e-3 {
                                            let cell_ar = (gd.dest[2] * aspect) / gd.dest[3].max(1e-4);
                                            gd.patch = cover_patch_focus(src_ar, cell_ar, focus_opt.unwrap_or((0.5, 0.5)), 0.0);
                                        }
                                    } else if is_bleed {
                                        gd.dest = [0.0, 0.0, 1.0, 1.0];
                                        gd.patch = cover_patch_focus(src_ar, aspect, focus_opt.unwrap_or((0.5, 0.5)), 0.0);
                                    } else if let Some(fc) = focus_opt {
                                        if gd.dest[2] >= 0.95 && gd.dest[3] >= 0.95 {
                                            gd.patch = cover_patch_focus(src_ar, aspect, fc, 0.0);
                                        }
                                    }
                                    let flip = gd.flip_x;
                                    let ox = gd.oxide_dropout.max(oxide);
                                    (gd, flip, 0u32, ox)
                                })
                                .collect();
                            if !ext_draws.is_empty() {
                                any_drawn = true;
                                self.record_grains_ext(*sid, ext_draws, clip.color_filter.as_ref(), style.additive, style.feather, false);
                            }
                        }
                    }
                    let clip_out = self.apply_chain(
                        &clip.effects,
                        ((ti as u64) << 20) | (cp.clip_index as u64 + 1),
                        CLIP,
                        fps,
                        t,
                    );
                    self.blend_onto(TRACK, clip_out, 1.0, if is_bleed { 0.0 } else { cp.style.additive * 0.5 });
                    track_any = true;
                }
                if !track_any {
                    continue;
                }
                let track_out = self.apply_chain(&track.effects, (ti as u64 + 1) << 32, TRACK, fps, t);
                let beat = t * scene.project.bpm / 60.0;
                let opac = track.opacity_at(beat);
                if is_bleed {
                    self.bleed_onto(MASTER, track_out, opac);
                } else {
                    self.blend_onto(MASTER, track_out, opac, 0.0);
                }
            }

            // Live hits on top, one layer per bus.
            let n_buses = scene.buses.len().min(250) as u8;
            for bus in 0..=n_buses {
                let (style, effects, link, add_in, add_out) = if bus == 0 {
                    (
                        VisualGrainStyle::morph(0.0, &VisualGrainStyle::default(), 0.0),
                        &[][..],
                        chromagrain_core::video::AvLink::default(),
                        0.0,
                        0.0,
                    )
                } else {
                    let b = &scene.buses[bus as usize - 1];
                    (b.style, &b.effects[..], b.link, b.style.additive, b.style.additive * 0.5)
                };
                let any = scene.live.iter().any(|h| h.1 == bus);
                if !any && effects.is_empty() {
                    continue;
                }
                self.pass(CLIP, Some([0.0; 4]));
                for (sid, _, ev) in scene.live.iter().filter(|h| h.1 == bus) {
                    self.ensure_source(&scene.project, *sid);
                    let Some(gsrc) = self.sources.get(sid) else { continue };
                    let dur = gsrc.duration.max(1e-6);
                    let src_ar = gsrc.src_aspect;
                    let eff_style = if is_bleed { VisualGrainStyle::full_frame(0.0) } else { style };
                    let raw_draws = grain_draws_layout(
                        std::slice::from_ref(ev),
                        &eff_style,
                        &link,
                        &chromagrain_core::timeline::ClipTransform::default(),
                        dur,
                        scene.live_t,
                        aspect,
                        false,
                        tgrid,
                    );
                    let focus_opt = scene.focus_overrides.get(sid).copied();
                    let dt = (scene.live_t - ev.onset).max(0.0);
                    let punch = if dt < 0.085 && (tgrid.is_some() || is_bleed) {
                        ((1.0 - dt as f32 / 0.085).powi(2)) * 0.08
                    } else {
                        0.0
                    };
                    let ext_draws: Vec<(GrainDraw, bool, u32, f32)> = raw_draws
                        .into_iter()
                        .map(|mut gd| {
                            if gd.tiled {
                                if focus_opt.is_some() || punch > 0.0 || (src_ar - aspect).abs() > 1e-3 {
                                    let cell_ar = (gd.dest[2] * aspect) / gd.dest[3].max(1e-4);
                                    gd.patch = cover_patch_focus(src_ar, cell_ar, focus_opt.unwrap_or((0.5, 0.5)), punch);
                                }
                            } else if is_bleed {
                                gd.dest = [0.0, 0.0, 1.0, 1.0];
                                gd.patch = cover_patch_focus(src_ar, aspect, focus_opt.unwrap_or((0.5, 0.5)), punch);
                            }
                            let flip = gd.flip_x;
                            let ox = gd.oxide_dropout;
                            (gd, flip, 0u32, ox)
                        })
                        .collect();
                    if !ext_draws.is_empty() {
                        any_drawn = true;
                        self.record_grains_ext(*sid, ext_draws, None, if is_bleed { 0.0 } else { add_in }, eff_style.feather, false);
                    }
                }
                let out = self.apply_chain(effects, 0xB000_0000_0000_0000 | bus as u64, CLIP, fps, scene.live_t);
                if is_bleed {
                    self.bleed_onto(MASTER, out, 1.0);
                } else {
                    self.blend_onto(MASTER, out, 1.0, add_out);
                }
            }
        }

        // `52 ms` Optical Carry: prevent single-frame black strobing on rapid 16th cuts.
        let clock_now = scene.t.max(scene.live_t);
        if any_drawn && !scene.parked {
            self.full(CARRY, Pipe::Blit, MASTER, None, Uni::default());
            self.carry_active_t = Some(clock_now);
        } else if !any_drawn && !scene.parked && scene.backdrop.is_none() {
            if let Some(last_t) = self.carry_active_t {
                let gap = clock_now - last_t;
                if gap > 0.0 && gap < 0.135 {
                    let hold_w = if gap <= 0.052 {
                        1.0
                    } else {
                        let u = ((gap - 0.052) / (0.135 - 0.052)).clamp(0.0, 1.0) as f32;
                        0.5 * (1.0 + (std::f32::consts::PI * u).cos())
                    };
                    self.blend_onto(MASTER, CARRY, hold_w, 0.0);
                }
            }
        }

        // Master effect chain + master 5-Band `ISOL8` isolator + optional live `Tape Stop` VHS roll.
        let mut out_tex = self.apply_chain(&scene.project.master_effects, 0xFFFF_0000_0000, MASTER, fps, t);
        if scene.project.isolator.is_active() {
            out_tex = self.apply_isolator(&scene.project.isolator, 1.0, out_tex);
        }
        if scene.tape_stop > 0.005 {
            let next = Self::other(out_tex);
            let mut u = self.res_uni();
            u.d = [scene.tape_stop, clock_now as f32, 0.0, 0.0];
            self.full(next, Pipe::TapeStop, out_tex, None, u);
            out_tex = next;
        }
        self.canvas_tex = Some(out_tex);
        self.execute()
    }

    fn tex_bind_group(&mut self, key: TexKey) {
        if self.tex_bgs.contains_key(&key) {
            return;
        }
        let view2d = |k: u32| -> &wgpu::TextureView {
            if k == u32::MAX {
                &self.dummy_2d
            } else {
                &self.targets[k as usize].view
            }
        };
        let arr = if key.2 == u32::MAX {
            &self.dummy_arr
        } else {
            self.sources.get(&(key.2 as usize)).map(|s| &s.view).unwrap_or(&self.dummy_arr)
        };
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chromagrain textures"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view2d(key.0)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view2d(key.1)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(arr) },
            ],
        });
        self.tex_bgs.insert(key, bg);
    }

    fn execute(&mut self) -> wgpu::CommandBuffer {
        let passes = std::mem::take(&mut self.passes);
        let n: u64 = passes.iter().map(|p| p.draws.len() as u64).sum::<u64>().max(1);
        if n > self.uni_slots {
            let slots = n.next_power_of_two();
            let (buf, bg) = Self::make_uni(&self.device, &self.uni_layout, slots);
            self.uni_buf = buf;
            self.uni_bg = bg;
            self.uni_slots = slots;
        }
        self.uni_staging.clear();
        self.uni_staging.resize((n * UNI_STRIDE) as usize, 0);
        let mut slot = 0usize;
        for p in &passes {
            for d in &p.draws {
                let at = slot * UNI_STRIDE as usize;
                self.uni_staging[at..at + UNI_SIZE as usize].copy_from_slice(uni_bytes(&d.u));
                slot += 1;
            }
        }
        self.queue.write_buffer(&self.uni_buf, 0, &self.uni_staging);
        for p in &passes {
            for d in &p.draws {
                self.tex_bind_group((key_part(d.t0), key_part(d.t1), key_part(d.arr)));
            }
        }

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("chromagrain preview"),
        });
        let mut slot = 0u64;
        for p in &passes {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets[p.target].view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match p.clear {
                            Some(c) => wgpu::LoadOp::Clear(c),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for d in &p.draws {
                rp.set_pipeline(&self.pipelines[&d.pipe]);
                rp.set_bind_group(0, &self.uni_bg, &[(slot * UNI_STRIDE) as u32]);
                rp.set_bind_group(1, &self.tex_bgs[&(key_part(d.t0), key_part(d.t1), key_part(d.arr))], &[]);
                rp.draw(0..6, 0..1);
                slot += 1;
            }
        }
        enc.finish()
    }

    fn prepare_present(&mut self, crop: [f32; 4]) -> Option<usize> {
        let tex = self.canvas_tex?;
        self.tex_bind_group((tex as u32, u32::MAX, u32::MAX));
        let slot = self.present_next;
        self.present_next = (self.present_next + 1) % PRESENT_SLOTS;
        let u = Uni { patch: crop, ..Default::default() };
        self.queue.write_buffer(&self.present_buf, slot as u64 * UNI_STRIDE, uni_bytes(&u));
        Some(slot)
    }

    fn present(&self, rp: &mut wgpu::RenderPass<'static>, slot: usize) {
        let Some(tex) = self.canvas_tex else { return };
        let Some(bg) = self.tex_bgs.get(&(tex as u32, u32::MAX, u32::MAX)) else { return };
        rp.set_pipeline(&self.present_pipe);
        rp.set_bind_group(0, &self.present_bg, &[(slot as u64 * UNI_STRIDE) as u32]);
        rp.set_bind_group(1, bg, &[]);
        rp.draw(0..6, 0..1);
    }
}

pub fn install(rs: &egui_wgpu::RenderState) {
    let g = GpuPreview::new(&rs.device, &rs.queue, rs.target_format);
    rs.renderer.write().callback_resources.insert(g);
}

pub struct PreviewCallback {
    pub scene: Arc<Mutex<Option<Scene>>>,
    pub rect: egui::Rect,
    slot: AtomicUsize,
}

impl PreviewCallback {
    pub fn shape(scene: Arc<Mutex<Option<Scene>>>, rect: egui::Rect) -> egui::Shape {
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(
            rect,
            PreviewCallback { scene, rect, slot: AtomicUsize::new(usize::MAX) },
        ))
    }
}

impl egui_wgpu::CallbackTrait for PreviewCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(g) = resources.get_mut::<GpuPreview>() else { return Vec::new() };
        let mut out = Vec::new();
        if let Some(scene) = self.scene.lock().unwrap().as_ref() {
            if g.last_frame != Some(scene.frame) || g.canvas_tex.is_none() {
                out.push(g.render(scene));
                g.last_frame = Some(scene.frame);
            }
        }
        let ppp = screen.pixels_per_point.max(1e-3);
        let screen_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen.size_in_pixels[0] as f32 / ppp, screen.size_in_pixels[1] as f32 / ppp),
        );
        let r = self.rect;
        let vis = r.intersect(screen_rect);
        let (rw, rh) = (r.width().max(1e-3), r.height().max(1e-3));
        let crop = [
            (vis.min.x - r.min.x) / rw,
            (vis.min.y - r.min.y) / rh,
            vis.width().max(0.0) / rw,
            vis.height().max(0.0) / rh,
        ];
        let slot = g.prepare_present(crop).unwrap_or(usize::MAX);
        self.slot.store(slot, Ordering::Relaxed);
        out
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let slot = self.slot.load(Ordering::Relaxed);
        if slot == usize::MAX {
            return;
        }
        if let Some(g) = resources.get::<GpuPreview>() {
            g.present(render_pass, slot);
        }
    }
}


#[cfg(test)]
mod tests {
    //! GPU ↔ CPU parity: render one frame on a headless wgpu device and
    //! compare it with the reference CPU renderer. Skips (passes) when no
    //! adapter is available (e.g. a GPU-less CI box).
    use super::*;
    use chromagrain_core::render::{plan_project, render_project};
    use chromagrain_core::timeline::{Clip, Source};
    use chromagrain_core::video::{Frame, VideoClip};

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    fn project(master: Option<AvEffect>) -> Project {
        let mut p = Project { bpm: 120.0, width: 160, height: 96, fps: 24.0, ..Default::default() };
        let sid = p.add_source(Source {
            name: "pattern".into(),
            audio: None,
            video: Some(Arc::new(VideoClip::test_pattern(160, 96, 12.0, 2.0))),
            base_hz: 0.0,
            ..Default::default()
        });
        let t = p.add_track("bed");
        let mut bed = Clip::new_snippet(sid, 0.0, 4.0);
        bed.grains.gain = 1.0;
        bed.visual.additive = 0.0;
        p.tracks[t].clips.push(bed);
        let t = p.add_track("cloud");
        let mut c = Clip::new(sid, 0.0, 4.0);
        c.grains.density = 30.0;
        c.grains.duration = 0.3;
        c.grains.pitch_jitter = 5.0;
        c.visual.additive = 0.0;
        p.tracks[t].clips.push(c);
        p.master_effects.extend(master);
        p
    }

    /// GPU frame at `t` seconds, read back as RGBA8.
    fn gpu_frame(g: &mut GpuPreview, p: &Project, t: f64, frame: u64) -> Frame {
        let scene = Scene {
            project: p.clone(),
            plan: Arc::new(plan_project(p)),
            t,
            live: Vec::new(),
            live_t: 0.0,
            buses: Vec::new(),
            backdrop: None,
            parked: false,
            frame,
            mode_override: None,
            focus_overrides: HashMap::new(),
            tape_stop: 0.0,
        };
        let cb = g.render(&scene);
        g.queue.submit([cb]);
        let tex = &g.targets[g.canvas_tex.unwrap()].tex;
        read_tex(&g.device, &g.queue, tex, p.width, p.height)
    }

    /// Read an RGBA8 texture back to the CPU.
    fn read_tex(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, w: u32, h: u32) -> Frame {
        let row = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::Wait).unwrap();
        let data = buf.slice(..).get_mapped_range();
        let mut out = Frame::black(w, h);
        for y in 0..h as usize {
            let src = &data[y * row as usize..y * row as usize + w as usize * 4];
            out.data[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(src);
        }
        out
    }

    fn px_close(a: [u8; 4], b: [u8; 4], tol: i32) -> bool {
        (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= tol)
    }

    #[test]
    fn every_window_presents_one_shared_render() {
        use egui_wgpu::CallbackTrait;
        let Some((device, queue)) = device() else {
            eprintln!("no wgpu adapter: skipping multi-window present test");
            return;
        };
        let mut res = egui_wgpu::CallbackResources::default();
        res.insert(GpuPreview::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm));
        let p = project(None);
        let scene = Arc::new(Mutex::new(Some(Scene {
            project: p.clone(),
            plan: Arc::new(plan_project(&p)),
            t: 1.25,
            live: Vec::new(),
            live_t: 0.0,
            buses: Vec::new(),
            backdrop: None,
            parked: false,
            frame: 7,
            mode_override: None,
            focus_overrides: HashMap::new(),
            tape_stop: 0.0,
        })));
        let aspect = p.width as f32 / p.height as f32;
        let r = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        let windows = [
            ((900u32, 600u32), 1.0f32, r(100.0, 80.0, 400.0, 240.0)),
            ((1024, 768), 1.0, crate::fit(r(0.0, 0.0, 1024.0, 768.0), aspect)),
            ((1280, 720), 2.0, crate::fit(r(0.0, 0.0, 640.0, 360.0), aspect)),
            ((600, 600), 1.0, egui::Rect::from_center_size(egui::pos2(300.0, 300.0), egui::vec2(600.0 * aspect, 600.0))),
        ];
        let marker = [255u8, 0, 255, 255];
        let mut shots = Vec::new();
        for ((w, h), ppp, rect) in windows {
            let surface = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fake window"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = surface.create_view(&Default::default());
            let cb = PreviewCallback { scene: scene.clone(), rect, slot: AtomicUsize::new(usize::MAX) };
            let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: ppp };
            let mut enc = device.create_command_encoder(&Default::default());
            let mut cmds = cb.prepare(&device, &queue, &sd, &mut enc, &mut res);
            {
                let mut rp = enc
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime();
                let info = egui::PaintCallbackInfo {
                    viewport: rect,
                    clip_rect: rect,
                    pixels_per_point: ppp,
                    screen_size_px: [w, h],
                };
                let vp = info.viewport_in_pixels();
                let clip = info.clip_rect_in_pixels();
                rp.set_viewport(vp.left_px as f32, vp.top_px as f32, vp.width_px as f32, vp.height_px as f32, 0.0, 1.0);
                rp.set_scissor_rect(clip.left_px as u32, clip.top_px as u32, clip.width_px as u32, clip.height_px as u32);
                cb.paint(info, &mut rp, &res);
            }
            cmds.push(enc.finish());
            queue.submit(cmds);
            shots.push(((w, h), ppp, rect, read_tex(&device, &queue, &surface, w, h)));
        }
        let g = res.get::<GpuPreview>().unwrap();
        assert_eq!(g.renders, 1, "four windows, one frame: rendered once");
        let canvas = read_tex(&device, &queue, &g.targets[g.canvas_tex.unwrap()].tex, p.width, p.height);
        let canvas_center = canvas.get(p.width / 2, p.height / 2);
        assert!(canvas.mean_luma() > 10.0);

        for ((w, h), ppp, rect, shot) in &shots {
            let px = |pt: egui::Pos2| ((pt.x * ppp) as u32, (pt.y * ppp) as u32);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(*w as f32 / ppp, *h as f32 / ppp));
            let vis = rect.intersect(screen);
            let (cx, cy) = px(rect.center());
            assert!(px_close(shot.get(cx, cy), canvas_center, 24), "{w}x{h}: centre {:?} vs canvas {canvas_center:?}", shot.get(cx, cy));
            let inside = px(vis.shrink(3.0).left_top());
            assert_ne!(shot.get(inside.0, inside.1), marker, "{w}x{h}: picture fills its rect");
            if vis.min.x > 3.0 {
                let (ox, oy) = px(egui::pos2(vis.min.x - 3.0, vis.center().y));
                assert_eq!(shot.get(ox, oy), marker, "{w}x{h}: nothing drawn outside the rect");
            }
            if vis.min.y > 3.0 {
                let (ox, oy) = px(egui::pos2(vis.center().x, vis.min.y - 3.0));
                assert_eq!(shot.get(ox, oy), marker, "{w}x{h}: letterbox bars stay clear");
            }
        }
        let ((w, h), _, rect, shot) = &shots[3];
        let crop_x0 = (0.0 - rect.min.x) / rect.width();
        let col = ((crop_x0 * p.width as f32) as u32).min(p.width - 1);
        let want = canvas.get(col, p.height / 2);
        assert!(px_close(shot.get(1, h / 2), want, 40), "cover crops: {:?} vs canvas col {col} {want:?} ({w}x{h})", shot.get(1, h / 2));

        scene.lock().unwrap().as_mut().unwrap().frame = 8;
        let cb = PreviewCallback { scene: scene.clone(), rect: shots[0].2, slot: AtomicUsize::new(usize::MAX) };
        let mut enc = device.create_command_encoder(&Default::default());
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [900, 600], pixels_per_point: 1.0 };
        let cmds = cb.prepare(&device, &queue, &sd, &mut enc, &mut res);
        queue.submit(cmds);
        assert_eq!(res.get::<GpuPreview>().unwrap().renders, 2);
    }

    fn mean_abs_diff(a: &Frame, b: &Frame) -> f32 {
        let mut sum = 0u64;
        let mut n = 0u64;
        for (pa, pb) in a.data.chunks_exact(4).zip(b.data.chunks_exact(4)) {
            for c in 0..3 {
                sum += (pa[c] as i32 - pb[c] as i32).unsigned_abs() as u64;
                n += 1;
            }
        }
        sum as f32 / n as f32
    }

    #[test]
    fn gpu_matches_the_cpu_reference_for_every_effect() {
        let Some((device, queue)) = device() else {
            eprintln!("no wgpu adapter: skipping GPU parity test");
            return;
        };
        let mut g = GpuPreview::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let t = 1.25;
        let beat = t * 2.0; // 120 bpm
        let mut frame = 0;
        let mut report = Vec::new();
        for name in [
            "none",
            "feather",
            "crush",
            "tint",
            "tune",
            "tile",
            "kaleido",
            "isolator-lf",
            "isolator-vocal",
            "isolator-hf",
            "compress",
            "delay",
            "reverb",
        ] {
            let fx = match name {
                "isolator-lf" => {
                    let mut iso = chromagrain_core::dsp::BandIsolator::default();
                    iso.solo = [true, false, false, false, false];
                    iso.monitor = chromagrain_core::dsp::IsolatorMonitor::Left;
                    Some(AvEffect::new(EffectKind::Isolator(iso)))
                }
                "isolator-vocal" => {
                    let mut iso = chromagrain_core::dsp::BandIsolator::default();
                    iso.solo = [false, true, true, true, false];
                    iso.ch_mode = chromagrain_core::dsp::IsolatorChannelMode::Mid;
                    Some(AvEffect::new(EffectKind::Isolator(iso)))
                }
                "isolator-hf" => {
                    let mut iso = chromagrain_core::dsp::BandIsolator::default();
                    iso.solo = [false, false, false, false, true];
                    Some(AvEffect::new(EffectKind::Isolator(iso)))
                }
                _ => EffectKind::parse(name).map(AvEffect::new),
            };
            let mut p = project(fx);
            if name == "feather" {
                p.tracks[1].clips[0].visual.feather = 0.6;
            }
            frame += 1;
            let gpu = gpu_frame(&mut g, &p, t, frame);
            let cpu = render_project(&p, beat, beat + 0.01).frames.remove(0);
            let d = mean_abs_diff(&gpu, &cpu);
            if let Ok(dir) = std::env::var("CHROMAGRAIN_PARITY_DIR") {
                let dir = std::path::Path::new(&dir);
                let _ = chromagrain_core::media::save_frame_png(&dir.join(format!("parity_{name}_gpu.png")), &gpu);
                let _ = chromagrain_core::media::save_frame_png(&dir.join(format!("parity_{name}_cpu.png")), &cpu);
            }
            report.push(format!("{name}: {d:.2}"));
            let min_luma = if name == "isolator-hf" { 1.5 } else { 10.0 };
            assert!(gpu.mean_luma() > min_luma, "{name}: GPU frame is not black ({})", gpu.mean_luma());
            let tol = match name {
                "delay" | "reverb" => 6.0,
                "compress" => 6.0,
                _ => 2.0,
            };
            if std::env::var("CHROMAGRAIN_PARITY_DIR").is_ok() {
                continue;
            }
            assert!(d < tol, "{name}: mean |gpu - cpu| = {d:.2} (tolerance {tol}) — {report:?}");
        }
        eprintln!("gpu vs cpu mean abs diff (0..255): {report:?}");
    }

    #[test]
    fn gpu_matches_the_cpu_for_tiles_and_envelopes() {
        use chromagrain_core::envelope::{Adsr, PictureCut};
        let Some((device, queue)) = device() else {
            eprintln!("no wgpu adapter: skipping GPU tile parity test");
            return;
        };
        let mut g = GpuPreview::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut report = Vec::new();
        for (k, (name, t)) in [("tile2", 1.25), ("tile3", 1.25), ("adsr-fade", 0.4), ("adsr-cut", 0.4)].into_iter().enumerate() {
            let mut p = project(None);
            match name {
                "tile2" => p.layout = chromagrain_core::tile::Layout::Tile,
                "tile3" => {
                    p.layout = chromagrain_core::tile::Layout::Tile;
                    let ti = p.add_track("third");
                    let mut c = Clip::new_snippet(0, 0.5, 3.0);
                    c.visual.additive = 0.0;
                    c.grains.position = 0.1;
                    p.tracks[ti].clips.push(c);
                }
                _ => {
                    let picture = if name == "adsr-fade" { PictureCut::Fade } else { PictureCut::Cut };
                    let env = Adsr { attack: 0.6, decay: 0.2, sustain: 0.5, release: 0.3, picture, ..Default::default() };
                    p.tracks[0].clips[0].grains.adsr = Some(env);
                    p.tracks[1].clips[0].grains.adsr = Some(env);
                }
            }
            let gpu = gpu_frame(&mut g, &p, t, 100 + k as u64);
            let beat = t * 2.0;
            let cpu = render_project(&p, beat, beat + 0.01).frames.remove(0);
            let d = mean_abs_diff(&gpu, &cpu);
            if let Ok(dir) = std::env::var("CHROMAGRAIN_PARITY_DIR") {
                let dir = std::path::Path::new(&dir);
                let _ = chromagrain_core::media::save_frame_png(&dir.join(format!("parity_{name}_gpu.png")), &gpu);
                let _ = chromagrain_core::media::save_frame_png(&dir.join(format!("parity_{name}_cpu.png")), &cpu);
            }
            report.push(format!("{name}: {d:.2}"));
            if name.starts_with("tile") {
                let mut stacked = p.clone();
                stacked.layout = chromagrain_core::tile::Layout::Stack;
                let stack = render_project(&stacked, beat, beat + 0.01).frames.remove(0);
                assert!(mean_abs_diff(&cpu, &stack) > 4.0, "{name}: tile looks like stack");
                let (w, h) = (p.width as usize, p.height as usize);
                let half = |f: &Frame, right: bool| {
                    let xs = if right { w / 2..w } else { 0..w / 2 };
                    let mut sum = 0u64;
                    for y in 0..h {
                        for x in xs.clone() {
                            sum += f.data[(y * w + x) * 4] as u64;
                        }
                    }
                    sum / (h * w / 2) as u64
                };
                assert!(half(&gpu, false) > 20 && half(&gpu, true) > 20, "{name}: a tile is missing on the GPU");
            }
            if std::env::var("CHROMAGRAIN_PARITY_DIR").is_err() {
                assert!(d < 3.0, "{name}: mean |gpu - cpu| = {d:.2} — {report:?}");
            }
        }
        eprintln!("gpu vs cpu (tiles, envelopes): {report:?}");
    }

    #[test]
    fn gpu_renders_split_bleed_carry_and_tapestop() {
        let Some((device, queue)) = device() else {
            eprintln!("no wgpu adapter: skipping Split/Bleed GPU test");
            return;
        };
        let mut g = GpuPreview::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut p = project(None);
        let sid2 = p.add_source(Source {
            name: "second".into(),
            audio: None,
            video: Some(Arc::new(VideoClip::test_pattern(160, 96, 12.0, 2.0))),
            base_hz: 0.0,
            ..Default::default()
        });
        let t2 = p.add_track("lead2");
        let mut c2 = Clip::new_snippet(sid2, 0.0, 4.0);
        c2.grains.position = 0.35;
        p.tracks[t2].clips.push(c2);
        let plan = Arc::new(plan_project(&p));

        let mut render_mode = |mode: StageMode, t: f64, frame: u64, tape_stop: f32| -> Frame {
            let mut focus_overrides = HashMap::new();
            focus_overrides.insert(0, (0.35, 0.5));
            focus_overrides.insert(sid2, (0.65, 0.5));
            let scene = Scene {
                project: p.clone(),
                plan: plan.clone(),
                t,
                live: Vec::new(),
                live_t: 0.0,
                buses: Vec::new(),
                backdrop: None,
                parked: false,
                frame,
                mode_override: Some(mode),
                focus_overrides,
                tape_stop,
            };
            let cb = g.render(&scene);
            g.queue.submit([cb]);
            let tex = &g.targets[g.canvas_tex.unwrap()].tex;
            read_tex(&g.device, &g.queue, tex, p.width, p.height)
        };

        let stack = render_mode(StageMode::Stack, 1.0, 1, 0.0);
        let split = render_mode(StageMode::Split, 1.0, 2, 0.0);
        let bleed = render_mode(StageMode::Bleed, 1.0, 3, 0.0);
        assert!(split.mean_luma() > 15.0, "Split stage must illuminate canvas");
        assert!(bleed.mean_luma() > 15.0, "Bleed stage must illuminate canvas");
        assert!(mean_abs_diff(&split, &stack) > 3.0, "Split must differ visually from Stack");
        assert!(mean_abs_diff(&bleed, &split) > 3.0, "Bleed must differ visually from Split");

        // 52ms optical carry: jumping 30ms past all clips keeps the stage lit instead of flashing black!
        let carry = render_mode(StageMode::Split, 2.02, 4, 0.0);
        // Reset carry at t = 1.99, then step to 2.02 (past the 4-beat = 2.0s arrangement end)
        let _ = render_mode(StageMode::Split, 1.99, 5, 0.0);
        let carried = render_mode(StageMode::Split, 2.02, 6, 0.0);
        let _ = carry;
        assert!(carried.mean_luma() > 12.0, "52ms optical carry holds frame across micro-gap");

        // Tape Stop distorts/desaturates active output
        let stopped = render_mode(StageMode::Split, 1.0, 7, 0.85);
        assert!(mean_abs_diff(&stopped, &split) > 2.0, "Tape stop VHS roll alters active output");
    }
}
