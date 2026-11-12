//! GPU preview pipeline (wgpu: Metal on macOS/iOS, D3D12/Vulkan on
//! Windows/Linux, GL as a fallback): renders the composition in real time
//! for performance playback. Grains are textured quads shaded with
//! hue-shift + HSV color filtering; effect chains run as fragment passes
//! (pixelate/posterize, ghost-history delay, persistence+blur reverb,
//! separable 8x8 DCT compression, tint, tune, tile); tracks composite in
//! z-order.
//!
//! Structure (multi-viewport friendly): the whole scene renders ONCE per
//! app frame into an offscreen texture during the egui-wgpu `prepare`
//! step (guarded by `Scene::frame`, so temporal effects advance once even
//! when several windows show the picture); every viewport that shows the
//! canvas then just samples that texture in its `paint` step. egui-wgpu
//! shares one device and one callback-resource map across viewports, so
//! the editor window and the performance output window draw the same
//! texture.
//!
//! The CPU renderer (render_project) stays the deterministic reference
//! used for export; this pipeline mirrors it closely enough to perform
//! with. Known deviations: GPU compress uses a DC-relative coefficient
//! threshold instead of exact top-K, and layer alpha under additive glow
//! is approximated by the premultiplied blend.

use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::render::ClipPlan;
use chromagrain_core::timeline::Project;
use chromagrain_core::video::{grain_draws, grain_draws_opts, ColorFilterMode, VisualGrainStyle};


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
    pub live: Vec<(usize, u8, chromagrain_core::grain::GrainEvent)>,
    pub live_t: f64,
    pub buses: Vec<LiveBus>,
    /// Full-frame picture under everything: (source, media seconds).
    pub backdrop: Option<(usize, f64)>,
    /// Transport stopped: long events ignore fades so the still frame shows.
    pub parked: bool,
    /// App frame counter: the scene is rendered once per distinct value,
    /// however many viewports present it.
    pub frame: u64,
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

const MAX_SRC_W: u32 = 256;
const MAX_SRC_LAYERS: usize = 192;
const MAX_HISTORY: usize = 40;
/// Dynamic-offset stride for per-draw uniforms (WebGPU's guaranteed
/// minimum uniform offset alignment).
const UNI_STRIDE: u64 = 256;
/// Present uniforms ring (one slot per canvas shown per frame).
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
const FIXED_TARGETS: usize = 7;

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
    /// Pass-specific params (see each fragment entry point).
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

// Vertical convention: every texture stores rows y-down with v = 0 at the
// top (same as uploaded source frames), and wgpu's framebuffer row 0 is
// the top too, so every pass maps y-down 0..1 to NDC with one flip and
// full-screen passes sample `uv` directly.
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
    // Rect + rotation about its center in aspect-corrected space (the
    // grain transform; rot = 0 reduces to a plain rect).
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

// Grain: a patch of one source frame, color-filtered and hue-rotated,
// premultiplied; `additive` lowers alpha toward pure glow.
@fragment
fn fs_grain(i: VOut) -> @location(0) vec4f {
    let suv = u.patch_r.xy + i.uv * u.patch_r.zw;
    let texel = textureSampleLevel(tarr, samp, suv, i32(u.a.z + 0.5), 0.0);
    var rgb = texel.rgb;
    // Source alpha participates: segmented sources are transparent
    // outside the tracked object, matching the CPU compositor.
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
    // Feather (d.x): soft cosine border taper, as the CPU compositor.
    let f = clamp(u.d.x, 0.0, 1.0) * 0.45;
    if (f > 0.0001) {
        let q = min(i.uv, vec2f(1.0) - i.uv);
        let e = clamp(q / f, vec2f(0.0), vec2f(1.0));
        let w = vec2f(0.5) - 0.5 * cos(3.14159265 * e);
        a *= w.x * w.y;
    }
    return vec4f(rgb * a, a * (1.0 - u.b.w));
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
// hues[] degrees, d.w count, c.w amount.
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

// Tile (mirrors fx::tile_source_uv + fx::video_tile): d.x count,
// d.y mirror, d.z turn. Nearest texel so it matches the CPU export.
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
    // under where transparent + screen where covered (premultiplied)
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

// 8x8 DCT basis: b(u,x) = a(u) * cos((2x+1)u*pi/16)
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

// Quantize: d.x crunch = (1-quality)^2, d.y DC-relative zeroing threshold.
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
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Pipe {
    Grain,
    BlitBlend,
    Blit,
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
}

impl Pipe {
    const ALL: [Pipe; 15] = [
        Pipe::Grain,
        Pipe::BlitBlend,
        Pipe::Blit,
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
    ];

    fn entry(self) -> &'static str {
        match self {
            Pipe::Grain => "fs_grain",
            Pipe::BlitBlend | Pipe::Blit => "fs_blit",
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
        }
    }

    /// Premultiplied "over" blending (GL: ONE, ONE_MINUS_SRC_ALPHA).
    fn blends(self) -> bool {
        matches!(self, Pipe::Grain | Pipe::BlitBlend)
    }

    /// The DCT intermediates write float targets.
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Pipe::DctRow | Pipe::DctCol | Pipe::Quant | Pipe::IdctCol => RGBA16F,
            _ => RGBA8,
        }
    }
}

struct Target {
    /// Owns the texture behind `view` (read back directly in tests).
    #[cfg_attr(not(test), allow(dead_code))]
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

struct GpuSource {
    view: wgpu::TextureView,
    layers: i32,
    duration: f64,
    /// Identity of the uploaded clip (Arc pointer) to detect replacement.
    ident: usize,
}

#[derive(Default)]
struct FxInst {
    /// Slab indices of the ghost-history ring.
    history: Vec<usize>,
    head: usize,
    filled: usize,
    accum: Option<[usize; 2]>,
    accum_idx: usize,
}

struct DrawOp {
    pipe: Pipe,
    t0: Option<usize>,
    t1: Option<usize>,
    /// Source id whose frame array is bound.
    arr: Option<usize>,
    u: Uni,
}

struct PassOp {
    target: usize,
    clear: Option<wgpu::Color>,
    draws: Vec<DrawOp>,
}

/// Texture bind-group cache key: (t0, t1, arr), `u32::MAX` = dummy.
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
    final_tex: Option<usize>,
    last_frame: Option<u64>,
    /// Offscreen scene renders so far (one per app frame, however many
    /// windows present it).
    pub renders: u64,
}

fn make_tex(device: &wgpu::Device, w: u32, h: u32, layers: u32, format: wgpu::TextureFormat, target: bool) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    if target {
        // COPY_SRC: read-back (tests today; Syphon / frame grabs later).
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
    /// Build the pipeline for the egui render state (`target_format` is
    /// what the window surfaces use).
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
        let make_pipe = |entry: &str, format: wgpu::TextureFormat, blend: bool| {
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
                        blend: blend.then_some(wgpu::BlendState { color: premul, alpha: premul }),
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
        let pipelines = Pipe::ALL.iter().map(|&p| (p, make_pipe(p.entry(), p.format(), p.blends()))).collect();
        let present_pipe = make_pipe("fs_present", target_format, false);

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
            final_tex: None,
            last_frame: None,
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
        self.fx.clear(); // sizes changed; rebuild temporal state
        for i in 0..FIXED_TARGETS {
            self.new_target(i == FPING || i == FPONG);
        }
        self.final_tex = None;
    }

    fn ensure_source(&mut self, project: &Project, sid: usize) -> Option<()> {
        let video = project.sources.get(sid)?.video.as_ref()?;
        let ident = Arc::as_ptr(video) as usize;
        if self.sources.get(&sid).is_some_and(|s| s.ident == ident) {
            return Some(());
        }
        // (Re)upload, downscaled + frame-subsampled.
        let n_frames = video.frames.len().min(1_000_000);
        if n_frames == 0 {
            return None;
        }
        let take = n_frames.min(MAX_SRC_LAYERS);
        let (sw, sh) = (video.frames[0].width, video.frames[0].height);
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
        self.sources.insert(sid, GpuSource { view, layers: take as i32, duration: video.duration(), ident });
        // Bind groups holding the old view are stale.
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

    /// A full-target pass that overwrites every pixel (no blending).
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
    /// index holding the result. Temporal effects keep per-instance state.
    fn apply_chain(&mut self, chain: &[AvEffect], scope: u64, mut cur: usize, fps: f32) -> usize {
        for (fi, fx) in chain.iter().enumerate() {
            let key = scope ^ ((fi as u64 + 1) << 40);
            if fx.video <= 0.001 {
                // Delay history must still record for later enabling.
                if let EffectKind::Delay { time, .. } = fx.kind {
                    let frames = ((time * fps).round() as usize).clamp(1, MAX_HISTORY);
                    self.push_history(key, frames, cur, 1.0);
                }
                continue;
            }
            let out = Self::other(cur);
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
                    // accum' = acc*decay + cur*(1-decay)
                    let mut u = Uni::default();
                    u.d[0] = 0.75 + 0.24 * size.clamp(0.0, 1.0);
                    self.full(write, Pipe::Racc, read, Some(cur), u);
                    let inst = self.fx.get_mut(&key).unwrap();
                    inst.accum_idx = 1 - inst.accum_idx;
                    // out = cur + smear
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
            }
        }
        cur
    }

    fn ensure_accum(&mut self, key: u64) {
        if self.fx.get(&key).is_some_and(|i| i.accum.is_some()) {
            return;
        }
        let a = self.new_target(false);
        let b = self.new_target(false);
        // New targets start undefined: clear them.
        self.pass(a, Some([0.0; 4]));
        self.pass(b, Some([0.0; 4]));
        self.fx.entry(key).or_default().accum = Some([a, b]);
    }

    /// The frame from `frames` steps ago, if the ring has filled.
    fn history_tail(&self, key: u64, frames: usize) -> Option<usize> {
        let inst = self.fx.get(&key)?;
        if inst.filled < frames || inst.history.len() < frames {
            return None;
        }
        let idx = (inst.head + inst.history.len() - frames) % inst.history.len();
        Some(inst.history[idx])
    }

    /// Record `cur` (scaled by feedback) into the ring for `key`.
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

    /// Record the grain draws of one event list into the current pass.
    #[allow(clippy::too_many_arguments)]
    fn record_grains(
        &mut self,
        sid: usize,
        draws: Vec<chromagrain_core::video::GrainDraw>,
        color_filter: Option<&chromagrain_core::video::ColorFilter>,
        additive: f32,
        feather: f32,
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
        for d in draws {
            let layer = ((d.src_time / dur).rem_euclid(1.0) * layers as f64).min(layers as f64 - 1.0) as f32;
            let u = Uni {
                rect: d.dest,
                patch: d.patch,
                cf,
                a: [d.rotation, aspect, layer.floor(), d.alpha],
                b: [d.hue_shift, mode, valmin, additive],
                d: [feather, 0.0, 0.0, 0.0],
                ..Default::default()
            };
            self.draw(Pipe::Grain, None, None, Some(sid), u);
        }
    }

    /// Composite `tex` (premultiplied) over `target`.
    fn blend_onto(&mut self, target: usize, tex: usize, opacity: f32, additive: f32) {
        self.pass(target, None);
        let u = Uni { c: [opacity, 1.0, 0.0, 0.0], b: [0.0, 0.0, 0.0, additive], ..Default::default() };
        self.draw(Pipe::BlitBlend, Some(tex), None, None, u);
    }

    /// Render the whole scene offscreen; returns the commands to submit.
    pub fn render(&mut self, scene: &Scene) -> wgpu::CommandBuffer {
        let (w, h) = (scene.project.width.max(2), scene.project.height.max(2));
        self.ensure_targets(w, h);
        self.passes.clear();
        self.renders += 1;

        // Upload any sources we need.
        let needed: Vec<usize> =
            scene.plan.iter().flat_map(|cp| cp.parts.iter().map(|(sid, _)| *sid)).collect();
        for sid in needed {
            self.ensure_source(&scene.project, sid);
        }

        // Master starts opaque black (plus the backdrop picture, if any).
        self.pass(MASTER, Some([0.0, 0.0, 0.0, 1.0]));
        if let Some((sid, at)) = scene.backdrop {
            self.ensure_source(&scene.project, sid);
            if let Some(g) = self.sources.get(&sid) {
                let layer = ((at / g.duration.max(1e-6)).rem_euclid(1.0) * g.layers as f64)
                    .min(g.layers as f64 - 1.0)
                    .floor() as f32;
                let u = Uni { a: [0.0, 1.0, layer, 1.0], ..Default::default() };
                self.draw(Pipe::Grain, None, None, Some(sid), u);
            }
        }

        let t = scene.t;
        let fps = scene.project.fps;
        let aspect = w as f32 / h as f32;
        for (ti, track) in scene.project.tracks.iter().enumerate() {
            if track.muted {
                continue;
            }
            let plans: Vec<&ClipPlan> = scene.plan.iter().filter(|p| p.track == ti).collect();
            if plans.is_empty() {
                continue;
            }
            // Track layer.
            self.pass(TRACK, Some([0.0; 4]));
            let mut track_any = !track.effects.is_empty();
            for cp in &plans {
                let Some(clip) = track.clips.get(cp.clip_index) else { continue };
                let any_active =
                    cp.parts.iter().any(|(_, evs)| evs.iter().any(|e| t >= e.onset && t < e.end()));
                if !any_active && clip.effects.is_empty() {
                    continue;
                }
                // Clip layer.
                self.pass(CLIP, Some([0.0; 4]));
                if any_active {
                    for (sid, evs) in &cp.parts {
                        let Some(gsrc) = self.sources.get(sid) else { continue };
                        let dur = gsrc.duration.max(1e-6);
                        let style: VisualGrainStyle = cp.style;
                        let draws = grain_draws_opts(
                            evs,
                            &style,
                            &clip.link,
                            &clip.transform,
                            dur,
                            t,
                            aspect,
                            scene.parked,
                        );
                        self.record_grains(*sid, draws, clip.color_filter.as_ref(), style.additive, style.feather);
                    }
                }
                let final_tex = self.apply_chain(
                    &clip.effects,
                    ((ti as u64) << 20) | (cp.clip_index as u64 + 1),
                    CLIP,
                    fps,
                );
                // Blend clip layer into track layer.
                self.blend_onto(TRACK, final_tex, 1.0, cp.style.additive * 0.5);
                track_any = true;
            }
            if !track_any {
                continue;
            }
            let final_tex = self.apply_chain(&track.effects, (ti as u64 + 1) << 32, TRACK, fps);
            // Composite track onto master.
            let beat = t * scene.project.bpm / 60.0;
            self.blend_onto(MASTER, final_tex, track.opacity_at(beat), 0.0);
        }

        // Live hits on top, one layer per bus, full correspondence.
        let n_buses = scene.buses.len().min(250) as u8;
        for bus in 0..=n_buses {
            let (style, effects, link, add_in, add_out) = if bus == 0 {
                (VisualGrainStyle::default(), &[][..], chromagrain_core::video::AvLink::default(), 0.4, 0.3)
            } else {
                let b = &scene.buses[bus as usize - 1];
                (b.style, &b.effects[..], b.link, b.style.additive, b.style.additive * 0.5)
            };
            let any = scene.live.iter().any(|h| h.1 == bus);
            if !any && effects.is_empty() {
                continue; // fx buses keep running so trails fade out
            }
            self.pass(CLIP, Some([0.0; 4]));
            for (sid, _, ev) in scene.live.iter().filter(|h| h.1 == bus) {
                self.ensure_source(&scene.project, *sid);
                let Some(gsrc) = self.sources.get(sid) else { continue };
                let dur = gsrc.duration.max(1e-6);
                let draws = grain_draws(
                    std::slice::from_ref(ev),
                    &style,
                    &link,
                    &chromagrain_core::timeline::ClipTransform::default(),
                    dur,
                    scene.live_t,
                    aspect,
                );
                if !draws.is_empty() {
                    self.record_grains(*sid, draws, None, add_in, style.feather);
                }
            }
            let out = self.apply_chain(effects, 0xB000_0000_0000_0000 | bus as u64, CLIP, fps);
            self.blend_onto(MASTER, out, 1.0, add_out);
        }

        // Master chain.
        let final_tex = self.apply_chain(&scene.project.master_effects, 0xFFFF_0000_0000, MASTER, fps);
        self.final_tex = Some(final_tex);
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

    /// Upload this frame's uniforms and encode every recorded pass.
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

    /// Prepare a present of the canvas: `crop` is the visible uv window
    /// (x, y, w, h) of the canvas. Returns the present slot to paint with.
    fn prepare_present(&mut self, crop: [f32; 4]) -> Option<usize> {
        let tex = self.final_tex?;
        self.tex_bind_group((tex as u32, u32::MAX, u32::MAX));
        let slot = self.present_next;
        self.present_next = (self.present_next + 1) % PRESENT_SLOTS;
        let u = Uni { patch: crop, ..Default::default() };
        self.queue.write_buffer(&self.present_buf, slot as u64 * UNI_STRIDE, uni_bytes(&u));
        Some(slot)
    }

    fn present(&self, rp: &mut wgpu::RenderPass<'static>, slot: usize) {
        let Some(tex) = self.final_tex else { return };
        let Some(bg) = self.tex_bgs.get(&(tex as u32, u32::MAX, u32::MAX)) else { return };
        rp.set_pipeline(&self.present_pipe);
        rp.set_bind_group(0, &self.present_bg, &[(slot as u64 * UNI_STRIDE) as u32]);
        rp.set_bind_group(1, bg, &[]);
        rp.draw(0..6, 0..1);
    }
}

/// Install the pipeline into egui-wgpu's shared callback resources (once,
/// at startup).
pub fn install(rs: &egui_wgpu::RenderState) {
    let g = GpuPreview::new(&rs.device, &rs.queue, rs.target_format);
    rs.renderer.write().callback_resources.insert(g);
}

/// Paint callback: shows the current scene in `rect` (any viewport).
/// Parts of `rect` outside the window are cropped, not squeezed, so a
/// "cover" fullscreen keeps its aspect.
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
            if g.last_frame != Some(scene.frame) || g.final_tex.is_none() {
                out.push(g.render(scene));
                g.last_frame = Some(scene.frame);
            }
        }
        // Visible part of the rect, as a uv window into the canvas.
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
        });
        // Additive glow is off: under glow the GPU's premultiplied blend
        // is a documented approximation of the CPU's straight-alpha mix
        // (same as the GL pipeline it replaces); everything else must
        // match closely.
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
        };
        let cb = g.render(&scene);
        g.queue.submit([cb]);
        let tex = &g.targets[g.final_tex.unwrap()].tex;
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

    /// The multi-window path exactly as egui-wgpu drives it: for each
    /// "window" (an offscreen surface), `prepare` then `paint` inside a
    /// render pass whose viewport/scissor egui would set. Two windows of
    /// different sizes show the same frame: one offscreen render, both
    /// presents letterboxed in their rects, nothing drawn outside them;
    /// a rect larger than the window (cover fullscreen) is cropped, not
    /// squeezed.
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
        })));
        let aspect = p.width as f32 / p.height as f32;
        let r = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        // (window size px, pixels_per_point, canvas rect in points)
        let windows = [
            // editor: the preview is a panel inside a bigger window
            ((900u32, 600u32), 1.0f32, r(100.0, 80.0, 400.0, 240.0)),
            // performance output on a 4:3 projector: letterboxed
            ((1024, 768), 1.0, crate::fit(r(0.0, 0.0, 1024.0, 768.0), aspect)),
            // retina output window, ppp 2
            ((1280, 720), 2.0, crate::fit(r(0.0, 0.0, 640.0, 360.0), aspect)),
            // cover fullscreen: rect overflows the window and is cropped
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
        let canvas = read_tex(&device, &queue, &g.targets[g.final_tex.unwrap()].tex, p.width, p.height);
        let canvas_center = canvas.get(p.width / 2, p.height / 2);
        assert!(canvas.mean_luma() > 10.0);

        for ((w, h), ppp, rect, shot) in &shots {
            let px = |pt: egui::Pos2| ((pt.x * ppp) as u32, (pt.y * ppp) as u32);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(*w as f32 / ppp, *h as f32 / ppp));
            let vis = rect.intersect(screen);
            // The canvas centre lands at the rect centre (also when cropped).
            let (cx, cy) = px(rect.center());
            assert!(px_close(shot.get(cx, cy), canvas_center, 24), "{w}x{h}: centre {:?} vs canvas {canvas_center:?}", shot.get(cx, cy));
            // Inside the rect: picture. Outside: untouched.
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
        // Cover: the window shows the middle of the canvas at full height —
        // its left edge is the canvas column at the crop offset, not col 0.
        let ((w, h), _, rect, shot) = &shots[3];
        let crop_x0 = (0.0 - rect.min.x) / rect.width();
        let col = ((crop_x0 * p.width as f32) as u32).min(p.width - 1);
        let want = canvas.get(col, p.height / 2);
        assert!(px_close(shot.get(1, h / 2), want, 40), "cover crops: {:?} vs canvas col {col} {want:?} ({w}x{h})", shot.get(1, h / 2));

        // A new app frame renders again (temporal effects step once per frame).
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
        for name in ["none", "feather", "crush", "tint", "tune", "tile", "kaleido", "compress", "delay", "reverb"] {
            let fx = EffectKind::parse(name).map(AvEffect::new);
            let mut p = project(fx);
            if name == "feather" {
                // Soft-edged cloud grains taper the same on both renderers.
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
            assert!(gpu.mean_luma() > 10.0, "{name}: GPU frame is not black ({})", gpu.mean_luma());
            // Temporal effects (delay ghosts, reverb persistence) start
            // from history on the CPU; one GPU frame has none yet.
            let tol = match name {
                "delay" | "reverb" => 6.0,
                "compress" => 6.0, // GPU uses a DC-relative threshold
                _ => 2.0,
            };
            if std::env::var("CHROMAGRAIN_PARITY_DIR").is_ok() {
                continue; // dumping images: report every effect
            }
            assert!(d < tol, "{name}: mean |gpu - cpu| = {d:.2} (tolerance {tol}) — {report:?}");
        }
        eprintln!("gpu vs cpu mean abs diff (0..255): {report:?}");
    }
}
