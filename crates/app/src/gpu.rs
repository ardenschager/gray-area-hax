//! GPU preview pipeline (glow / OpenGL 3.3): renders the composition in
//! real time for performance playback. Grains are textured quads shaded
//! with hue-shift + HSV color filtering; effect chains run as fragment
//! passes (pixelate/posterize, ghost-history delay, persistence+blur
//! reverb, separable 8x8 DCT compression); tracks composite in z-order.
//!
//! The CPU renderer (render_project) stays the deterministic reference
//! used for export; this pipeline mirrors it closely enough to perform
//! with. Known deviations: GPU compress uses a DC-relative coefficient
//! threshold instead of exact top-K, and layer alpha under additive glow
//! is approximated by the premultiplied blend.

use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::render::ClipPlan;
use chromagrain_core::timeline::Project;
use chromagrain_core::video::{grain_draws, ColorFilterMode, VisualGrainStyle};
use eframe::glow::{self, HasContext};
use std::collections::HashMap;
use std::sync::Arc;

/// What the pipeline draws each frame (cheap to clone: media is Arc'd).
pub struct Scene {
    pub project: Project,
    pub plan: Arc<Vec<ClipPlan>>,
    pub t: f64,
}

const MAX_SRC_W: u32 = 256;
const MAX_SRC_LAYERS: usize = 192;
const MAX_HISTORY: usize = 40;

struct Target {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
}

struct GpuSource {
    tex: glow::Texture,
    layers: i32,
    duration: f64,
    /// Identity of the uploaded clip (Arc pointer) to detect replacement.
    ident: usize,
}

#[derive(Default)]
struct FxInst {
    history: Vec<glow::Texture>,
    head: usize,
    filled: usize,
    accum: Option<[Target; 2]>,
    accum_idx: usize,
}

pub struct GpuPreview {
    programs: HashMap<&'static str, glow::Program>,
    vao: glow::VertexArray,
    _vbo: glow::Buffer,
    size: (i32, i32),
    master: Option<Target>,
    track_t: Option<Target>,
    clip_t: Option<Target>,
    ping: Option<Target>,
    pong: Option<Target>,
    fping: Option<Target>,
    fpong: Option<Target>,
    sources: HashMap<usize, GpuSource>,
    fx: HashMap<u64, FxInst>,
}

const QUAD_VS: &str = r#"#version 330 core
layout(location=0) in vec2 a_pos;
uniform vec4 u_rect; // x,y,w,h in 0..1, y-down "screen" space
out vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 p = u_rect.xy + a_pos * u_rect.zw;   // 0..1 y-down
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}"#;

const HSV_HELPERS: &str = r#"
vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0/3.0, 2.0/3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0*d + e)), d / (q.x + e), q.x);
}
vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0/3.0, 1.0/3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}"#;

fn grain_fs() -> String {
    format!(
        r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2DArray u_src;
uniform float u_layer;
uniform vec4 u_patch;
uniform float u_alpha;
uniform float u_hue;      // degrees
uniform int u_cf_mode;    // 0 off, 1 keep, 2 remove
uniform vec4 u_cf;        // center_deg, half_width_deg, softness, sat_min
uniform float u_cf_valmin;
uniform float u_additive;
{HSV_HELPERS}
void main() {{
    vec2 suv = u_patch.xy + v_uv * u_patch.zw;
    vec3 rgb = texture(u_src, vec3(suv, u_layer)).rgb;
    float a = u_alpha;
    vec3 hsv = rgb2hsv(rgb);
    if (u_cf_mode != 0) {{
        float h = hsv.x * 360.0;
        float d = abs(mod(h - u_cf.x + 540.0, 360.0) - 180.0);
        float halfw = max(u_cf.y, 0.001);
        float soft = max(halfw * u_cf.z, 0.001);
        float inband = clamp(1.0 - (d - (halfw - soft)) / (2.0 * soft), 0.0, 1.0);
        float colored = (hsv.y >= u_cf.w && hsv.z >= u_cf_valmin) ? 1.0 : 0.0;
        float band = inband * colored;
        a *= (u_cf_mode == 1) ? band : (1.0 - band);
    }}
    if (abs(u_hue) > 0.5) {{
        hsv.x = fract(hsv.x + u_hue / 360.0 + 10.0);
        rgb = hsv2rgb(hsv);
    }}
    frag = vec4(rgb * a, a * (1.0 - u_additive));
}}"#
    )
}

const BLIT_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform float u_opacity;
uniform float u_additive;
uniform float u_alpha_mul; // extra alpha scale (delay feedback history)
void main() {
    vec4 c = texture(u_tex, v_uv);
    frag = vec4(c.rgb * u_opacity, c.a * u_opacity * u_alpha_mul * (1.0 - u_additive));
}"#;

const PRESENT_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
void main() {
    vec4 c = texture(u_tex, vec2(v_uv.x, 1.0 - v_uv.y));
    frag = vec4(c.rgb, 1.0);
}"#;

const CRUSH_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
uniform float u_block;
uniform float u_levels;
void main() {
    vec2 px = v_uv * u_res;
    vec2 buv = (floor(px / u_block) * u_block + 0.5) / u_res;
    vec4 c = texture(u_tex, buv);
    float l = max(u_levels - 1.0, 1.0);
    c.rgb = floor(c.rgb * l + 0.5) / l;
    frag = c;
}"#;

const DELAY_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform sampler2D u_ghost;
uniform vec2 u_shift;
uniform float u_mix;
void main() {
    vec4 cur = texture(u_tex, v_uv);
    vec2 guv = v_uv - u_shift;
    vec4 g = vec4(0.0);
    if (guv.x >= 0.0 && guv.x <= 1.0 && guv.y >= 0.0 && guv.y <= 1.0)
        g = texture(u_ghost, guv) * u_mix;
    // under where transparent + screen where covered (premultiplied)
    frag = cur + g * (1.0 - cur.a) + vec4(g.rgb * cur.a * (1.0 - cur.rgb), 0.0);
}"#;

const REVERB_ACCUM_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_acc;
uniform sampler2D u_cur;
uniform float u_decay;
void main() {
    frag = texture(u_acc, v_uv) * u_decay + texture(u_cur, v_uv) * (1.0 - u_decay);
}"#;

const REVERB_MIX_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform sampler2D u_acc;
uniform vec2 u_res;
uniform float u_radius;
uniform float u_mix;
void main() {
    vec4 cur = texture(u_tex, v_uv);
    vec4 sm = vec4(0.0);
    float wsum = 0.0;
    for (int i = -2; i <= 2; i++) {
        for (int j = -2; j <= 2; j++) {
            float w = 1.0 / (1.0 + float(i*i + j*j));
            sm += texture(u_acc, v_uv + vec2(float(i), float(j)) * u_radius / u_res) * w;
            wsum += w;
        }
    }
    sm = sm / wsum * u_mix;
    frag = cur + sm * (1.0 - cur.a) + vec4(sm.rgb * cur.a * (1.0 - cur.rgb), 0.0);
}"#;

// 8x8 DCT basis in-shader: b(u,x) = a(u) * cos((2x+1)u*pi/16)
const DCT_COMMON: &str = r#"
float dctb(int u, int x) {
    float a = (u == 0) ? 0.35355339 : 0.5;
    return a * cos((2.0 * float(x) + 1.0) * float(u) * 3.14159265 / 16.0);
}"#;

fn dct_row_fs() -> String {
    format!(
        r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
{DCT_COMMON}
void main() {{
    vec2 px = floor(v_uv * u_res);
    int u = int(mod(px.x, 8.0));
    float bx = px.x - float(u);
    vec4 s = vec4(0.0);
    for (int k = 0; k < 8; k++) {{
        vec2 uv = (vec2(bx + float(k), px.y) + 0.5) / u_res;
        s += (texture(u_tex, uv) - 0.5) * dctb(u, k);
    }}
    frag = s;
}}"#
    )
}

fn dct_col_fs() -> String {
    format!(
        r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
{DCT_COMMON}
void main() {{
    vec2 px = floor(v_uv * u_res);
    int v = int(mod(px.y, 8.0));
    float by = px.y - float(v);
    vec4 s = vec4(0.0);
    for (int k = 0; k < 8; k++) {{
        vec2 uv = (vec2(px.x, by + float(k)) + 0.5) / u_res;
        s += texture(u_tex, uv) * dctb(v, k);
    }}
    frag = s;
}}"#
    )
}

const QUANT_FS: &str = r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
uniform float u_crunch;   // (1-quality)^2
uniform float u_thresh;   // relative-to-DC zeroing threshold
void main() {
    vec2 px = floor(v_uv * u_res);
    float u = mod(px.x, 8.0);
    float v = mod(px.y, 8.0);
    vec4 c = texture(u_tex, v_uv);
    vec2 dcuv = (px - vec2(u, v) + 0.5) / u_res;
    vec4 dc = texture(u_tex, dcuv);
    float q = (1.0 + u_crunch * (8.0 + 16.0 * (u + v))) / 255.0;
    c = floor(c / q + 0.5) * q;
    if (u + v > 0.5) {
        vec4 keep = step(vec4(u_thresh) * (abs(dc) + 0.02), abs(c));
        c *= keep;
    }
    frag = c;
}"#;

fn idct_col_fs() -> String {
    format!(
        r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
{DCT_COMMON}
void main() {{
    vec2 px = floor(v_uv * u_res);
    int y = int(mod(px.y, 8.0));
    float by = px.y - float(y);
    vec4 s = vec4(0.0);
    for (int k = 0; k < 8; k++) {{
        vec2 uv = (vec2(px.x, by + float(k)) + 0.5) / u_res;
        s += texture(u_tex, uv) * dctb(k, y);
    }}
    frag = s;
}}"#
    )
}

fn idct_row_fs() -> String {
    format!(
        r#"#version 330 core
in vec2 v_uv; out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_res;
{DCT_COMMON}
void main() {{
    vec2 px = floor(v_uv * u_res);
    int x = int(mod(px.x, 8.0));
    float bx = px.x - float(x);
    vec4 s = vec4(0.0);
    for (int k = 0; k < 8; k++) {{
        vec2 uv = (vec2(bx + float(k), px.y) + 0.5) / u_res;
        s += texture(u_tex, uv) * dctb(k, x);
    }}
    frag = clamp(s + 0.5, 0.0, 1.0);
}}"#
    )
}

unsafe fn compile(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(format!("shader: {}", gl.get_shader_info_log(sh)));
            }
            gl.attach_shader(program, sh);
            shaders.push(sh);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(format!("link: {}", gl.get_program_info_log(program)));
        }
        for sh in shaders {
            gl.detach_shader(program, sh);
            gl.delete_shader(sh);
        }
        Ok(program)
    }
}

unsafe fn make_target(gl: &glow::Context, w: i32, h: i32, float: bool) -> Result<Target, String> {
    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        let (ifmt, ty) = if float {
            (glow::RGBA16F as i32, glow::FLOAT)
        } else {
            (glow::RGBA8 as i32, glow::UNSIGNED_BYTE)
        };
        gl.tex_image_2d(glow::TEXTURE_2D, 0, ifmt, w, h, 0, glow::RGBA, ty, glow::PixelUnpackData::Slice(None));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        let fbo = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
            return Err("framebuffer incomplete".into());
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        Ok(Target { fbo, tex })
    }
}

impl GpuPreview {
    pub fn new(gl: &glow::Context) -> Result<GpuPreview, String> {
        unsafe {
            let mut programs = HashMap::new();
            programs.insert("grain", compile(gl, QUAD_VS, &grain_fs())?);
            programs.insert("blit", compile(gl, QUAD_VS, BLIT_FS)?);
            programs.insert("present", compile(gl, QUAD_VS, PRESENT_FS)?);
            programs.insert("crush", compile(gl, QUAD_VS, CRUSH_FS)?);
            programs.insert("delay", compile(gl, QUAD_VS, DELAY_FS)?);
            programs.insert("racc", compile(gl, QUAD_VS, REVERB_ACCUM_FS)?);
            programs.insert("rmix", compile(gl, QUAD_VS, REVERB_MIX_FS)?);
            programs.insert("dct_row", compile(gl, QUAD_VS, &dct_row_fs())?);
            programs.insert("dct_col", compile(gl, QUAD_VS, &dct_col_fs())?);
            programs.insert("quant", compile(gl, QUAD_VS, QUANT_FS)?);
            programs.insert("idct_col", compile(gl, QUAD_VS, &idct_col_fs())?);
            programs.insert("idct_row", compile(gl, QUAD_VS, &idct_row_fs())?);

            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let verts: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0];
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck_cast(&verts),
                glow::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.bind_vertex_array(None);

            Ok(GpuPreview {
                programs,
                vao,
                _vbo: vbo,
                size: (0, 0),
                master: None,
                track_t: None,
                clip_t: None,
                ping: None,
                pong: None,
                fping: None,
                fpong: None,
                sources: HashMap::new(),
                fx: HashMap::new(),
            })
        }
    }

    fn ensure_targets(&mut self, gl: &glow::Context, w: i32, h: i32) -> Result<(), String> {
        if self.size == (w, h) && self.master.is_some() {
            return Ok(());
        }
        unsafe {
            self.size = (w, h);
            self.master = Some(make_target(gl, w, h, false)?);
            self.track_t = Some(make_target(gl, w, h, false)?);
            self.clip_t = Some(make_target(gl, w, h, false)?);
            self.ping = Some(make_target(gl, w, h, false)?);
            self.pong = Some(make_target(gl, w, h, false)?);
            self.fping = Some(make_target(gl, w, h, true).ok()).flatten();
            self.fpong = Some(make_target(gl, w, h, true).ok()).flatten();
            self.fx.clear(); // sizes changed; rebuild temporal state
        }
        Ok(())
    }

    fn ensure_source(&mut self, gl: &glow::Context, project: &Project, sid: usize) -> Option<()> {
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
                let sy = (y as f32 / scale) as u32;
                let sy = sy.min(f.height - 1);
                for x in 0..tw {
                    let sx = ((x as f32 / scale) as u32).min(f.width - 1);
                    let si = ((sy * f.width + sx) * 4) as usize;
                    let di = base + ((y * tw + x) * 4) as usize;
                    data[di..di + 4].copy_from_slice(&f.data[si..si + 4]);
                }
            }
        }
        unsafe {
            if let Some(old) = self.sources.remove(&sid) {
                gl.delete_texture(old.tex);
            }
            let tex = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                glow::RGBA8 as i32,
                tw as i32,
                th as i32,
                take as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&data)),
            );
            for (p, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, p, v);
            }
            self.sources.insert(
                sid,
                GpuSource { tex, layers: take as i32, duration: video.duration(), ident },
            );
        }
        Some(())
    }

    unsafe fn use_program(&self, gl: &glow::Context, name: &str) -> glow::Program {
        let p = self.programs[name];
        unsafe {
            gl.use_program(Some(p));
        }
        p
    }

    unsafe fn set_f(&self, gl: &glow::Context, p: glow::Program, name: &str, v: f32) {
        unsafe {
            let loc = gl.get_uniform_location(p, name);
            gl.uniform_1_f32(loc.as_ref(), v);
        }
    }
    unsafe fn set_i(&self, gl: &glow::Context, p: glow::Program, name: &str, v: i32) {
        unsafe {
            let loc = gl.get_uniform_location(p, name);
            gl.uniform_1_i32(loc.as_ref(), v);
        }
    }
    unsafe fn set_2f(&self, gl: &glow::Context, p: glow::Program, name: &str, a: f32, b: f32) {
        unsafe {
            let loc = gl.get_uniform_location(p, name);
            gl.uniform_2_f32(loc.as_ref(), a, b);
        }
    }
    unsafe fn set_4f(&self, gl: &glow::Context, p: glow::Program, name: &str, v: [f32; 4]) {
        unsafe {
            let loc = gl.get_uniform_location(p, name);
            gl.uniform_4_f32(loc.as_ref(), v[0], v[1], v[2], v[3]);
        }
    }

    unsafe fn draw_quad(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }

    unsafe fn bind_target(&self, gl: &glow::Context, t: &Target, clear: Option<[f32; 4]>) {
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
            gl.viewport(0, 0, self.size.0, self.size.1);
            if let Some(c) = clear {
                gl.clear_color(c[0], c[1], c[2], c[3]);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
        }
    }

    /// Process an effect chain over `cur` (a texture), returning the final
    /// texture. Uses ping/pong; temporal effects keep per-instance state.
    #[allow(clippy::too_many_arguments)]
    unsafe fn apply_chain(
        &mut self,
        gl: &glow::Context,
        chain: &[AvEffect],
        scope: u64,
        mut cur: glow::Texture,
        fps: f32,
    ) -> glow::Texture {
        unsafe {
            gl.disable(glow::BLEND);
            for (fi, fx) in chain.iter().enumerate() {
                if fx.video <= 0.001 {
                    // Delay history must still record for later enabling.
                    if let EffectKind::Delay { time, .. } = fx.kind {
                        let key = scope ^ ((fi as u64 + 1) << 40);
                        let frames = ((time * fps).round() as usize).clamp(1, MAX_HISTORY);
                        self.push_history(gl, key, frames, cur, 1.0);
                    }
                    continue;
                }
                let (w, h) = (self.size.0 as f32, self.size.1 as f32);
                match fx.kind {
                    EffectKind::Crush { downsample, bits } => {
                        let out = if cur == self.ping.as_ref().unwrap().tex {
                            self.pong.as_ref().unwrap()
                        } else {
                            self.ping.as_ref().unwrap()
                        };
                        self.bind_target(gl, out, None);
                        let p = self.use_program(gl, "crush");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(cur));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_2f(gl, p, "u_res", w, h);
                        let block = 1.0 + (downsample - 1.0) * fx.video;
                        let levels = 2.0f32.powf(8.0 + (bits.clamp(1.0, 8.0) - 8.0) * fx.video);
                        self.set_f(gl, p, "u_block", block.max(1.0));
                        self.set_f(gl, p, "u_levels", levels);
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        cur = out.tex;
                    }
                    EffectKind::Delay { time, feedback, mix, shift_x, shift_y } => {
                        let key = scope ^ ((fi as u64 + 1) << 40);
                        let frames = ((time * fps).round() as usize).clamp(1, MAX_HISTORY);
                        let ghost = self.history_tail(key, frames);
                        let out = if cur == self.ping.as_ref().unwrap().tex {
                            self.pong.as_ref().unwrap()
                        } else {
                            self.ping.as_ref().unwrap()
                        };
                        self.bind_target(gl, out, None);
                        let p = self.use_program(gl, "delay");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(cur));
                        gl.active_texture(glow::TEXTURE1);
                        gl.bind_texture(glow::TEXTURE_2D, Some(ghost.unwrap_or(cur)));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_i(gl, p, "u_ghost", 1);
                        self.set_2f(gl, p, "u_shift", shift_x, shift_y);
                        self.set_f(gl, p, "u_mix", if ghost.is_some() { mix * fx.video } else { 0.0 });
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        cur = out.tex;
                        self.push_history(gl, key, frames, cur, feedback);
                    }
                    EffectKind::Reverb { size, damp, mix } => {
                        let key = scope ^ ((fi as u64 + 1) << 40);
                        self.ensure_accum(gl, key);
                        let inst = self.fx.get(&key).unwrap();
                        let (a_read, a_write) = (inst.accum_idx, 1 - inst.accum_idx);
                        let read_tex = inst.accum.as_ref().unwrap()[a_read].tex;
                        // accum' = acc*decay + cur*(1-decay)
                        let write = &self.fx[&key].accum.as_ref().unwrap()[a_write];
                        let wf = write.fbo;
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(wf));
                        gl.viewport(0, 0, self.size.0, self.size.1);
                        let p = self.use_program(gl, "racc");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(read_tex));
                        gl.active_texture(glow::TEXTURE1);
                        gl.bind_texture(glow::TEXTURE_2D, Some(cur));
                        self.set_i(gl, p, "u_acc", 0);
                        self.set_i(gl, p, "u_cur", 1);
                        self.set_f(gl, p, "u_decay", 0.75 + 0.24 * size.clamp(0.0, 1.0));
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        let acc_tex = self.fx[&key].accum.as_ref().unwrap()[a_write].tex;
                        self.fx.get_mut(&key).unwrap().accum_idx = a_write;
                        // out = cur + smear
                        let out = if cur == self.ping.as_ref().unwrap().tex {
                            self.pong.as_ref().unwrap()
                        } else {
                            self.ping.as_ref().unwrap()
                        };
                        self.bind_target(gl, out, None);
                        let p = self.use_program(gl, "rmix");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(cur));
                        gl.active_texture(glow::TEXTURE1);
                        gl.bind_texture(glow::TEXTURE_2D, Some(acc_tex));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_i(gl, p, "u_acc", 1);
                        self.set_2f(gl, p, "u_res", w, h);
                        self.set_f(gl, p, "u_radius", 1.0 + damp * 5.0);
                        self.set_f(gl, p, "u_mix", mix * fx.video);
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        cur = out.tex;
                    }
                    EffectKind::Compress { quality } => {
                        let (Some(fp), Some(fq)) = (&self.fping, &self.fpong) else { continue };
                        let q = 1.0 + (quality - 1.0) * fx.video;
                        if q >= 0.999 {
                            continue;
                        }
                        let crunch = (1.0 - q) * (1.0 - q);
                        // row DCT: cur -> fping
                        for (name, src, dst) in [
                            ("dct_row", cur, fp),
                            ("dct_col", fp.tex, fq),
                        ] {
                            self.bind_target(gl, dst, None);
                            let p = self.use_program(gl, name);
                            gl.active_texture(glow::TEXTURE0);
                            gl.bind_texture(glow::TEXTURE_2D, Some(src));
                            self.set_i(gl, p, "u_tex", 0);
                            self.set_2f(gl, p, "u_res", w, h);
                            self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                            self.draw_quad(gl);
                        }
                        // quantize: fpong -> fping
                        self.bind_target(gl, fp, None);
                        let p = self.use_program(gl, "quant");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(fq.tex));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_2f(gl, p, "u_res", w, h);
                        self.set_f(gl, p, "u_crunch", crunch);
                        self.set_f(gl, p, "u_thresh", crunch * 0.35);
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        // inverse: fping -> fpong -> (rgba8 out)
                        self.bind_target(gl, fq, None);
                        let p = self.use_program(gl, "idct_col");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(fp.tex));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_2f(gl, p, "u_res", w, h);
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        let out = if cur == self.ping.as_ref().unwrap().tex {
                            self.pong.as_ref().unwrap()
                        } else {
                            self.ping.as_ref().unwrap()
                        };
                        self.bind_target(gl, out, None);
                        let p = self.use_program(gl, "idct_row");
                        gl.active_texture(glow::TEXTURE0);
                        gl.bind_texture(glow::TEXTURE_2D, Some(fq.tex));
                        self.set_i(gl, p, "u_tex", 0);
                        self.set_2f(gl, p, "u_res", w, h);
                        self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                        self.draw_quad(gl);
                        cur = out.tex;
                    }
                }
            }
            cur
        }
    }

    unsafe fn ensure_accum(&mut self, gl: &glow::Context, key: u64) {
        let inst = self.fx.entry(key).or_default();
        if inst.accum.is_none() {
            unsafe {
                let a = make_target(gl, self.size.0, self.size.1, false).ok();
                let b = make_target(gl, self.size.0, self.size.1, false).ok();
                if let (Some(a), Some(b)) = (a, b) {
                    for t in [&a, &b] {
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.fbo));
                        gl.clear_color(0.0, 0.0, 0.0, 0.0);
                        gl.clear(glow::COLOR_BUFFER_BIT);
                    }
                    inst.accum = Some([a, b]);
                }
            }
        }
    }

    /// The frame from `frames` steps ago, if the ring has filled.
    fn history_tail(&mut self, key: u64, frames: usize) -> Option<glow::Texture> {
        let inst = self.fx.get(&key)?;
        if inst.filled < frames || inst.history.len() < frames {
            return None;
        }
        let idx = (inst.head + inst.history.len() - frames) % inst.history.len();
        Some(inst.history[idx])
    }

    /// Record `cur` (scaled by feedback) into the ring for `key`.
    unsafe fn push_history(
        &mut self,
        gl: &glow::Context,
        key: u64,
        frames: usize,
        cur: glow::Texture,
        feedback: f32,
    ) {
        unsafe {
            let (w, h) = self.size;
            let need = frames.min(MAX_HISTORY);
            // Ensure ring textures exist.
            {
                let inst = self.fx.entry(key).or_default();
                while inst.history.len() < need {
                    let t = match make_target(gl, w, h, false) {
                        Ok(t) => t,
                        Err(_) => return,
                    };
                    // Keep only the texture; render into it via shared fbo.
                    inst.history.push(t.tex);
                    gl.delete_framebuffer(t.fbo);
                }
            }
            let (head, tex) = {
                let inst = self.fx.get(&key).unwrap();
                (inst.head % need, inst.history[inst.head % need])
            };
            // Render cur * feedback into the history slot.
            let fbo = gl.create_framebuffer().ok();
            let Some(fbo) = fbo else { return };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            gl.viewport(0, 0, w, h);
            gl.disable(glow::BLEND);
            let p = self.use_program(gl, "blit");
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(cur));
            self.set_i(gl, p, "u_tex", 0);
            self.set_f(gl, p, "u_opacity", feedback);
            self.set_f(gl, p, "u_additive", 0.0);
            self.set_f(gl, p, "u_alpha_mul", 1.0);
            self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
            self.draw_quad(gl);
            gl.delete_framebuffer(fbo);
            let inst = self.fx.get_mut(&key).unwrap();
            inst.head = (head + 1) % need;
            inst.filled = (inst.filled + 1).min(need + 1);
        }
    }

    /// Render the whole scene and present it into the egui viewport.
    pub fn paint(
        &mut self,
        gl: &glow::Context,
        info: &eframe::egui::PaintCallbackInfo,
        scene: &Scene,
    ) {
        let (w, h) = (scene.project.width as i32, scene.project.height as i32);
        if self.ensure_targets(gl, w.max(2), h.max(2)).is_err() {
            return;
        }
        unsafe {
            let prev_fbo = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);
            let scissor_was_on = gl.is_enabled(glow::SCISSOR_TEST);
            gl.disable(glow::SCISSOR_TEST);

            // Upload any sources we need.
            let needed: Vec<usize> = scene
                .plan
                .iter()
                .flat_map(|cp| cp.parts.iter().map(|(sid, _)| *sid))
                .collect();
            for sid in needed {
                self.ensure_source(gl, &scene.project, sid);
            }

            // Master starts opaque black.
            let master_fbo = self.master.as_ref().unwrap().fbo;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(master_fbo));
            gl.viewport(0, 0, self.size.0, self.size.1);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            let t = scene.t;
            let fps = scene.project.fps;
            for (ti, track) in scene.project.tracks.iter().enumerate() {
                if track.muted {
                    continue;
                }
                let plans: Vec<&ClipPlan> =
                    scene.plan.iter().filter(|p| p.track == ti).collect();
                if plans.is_empty() {
                    continue;
                }
                // Track layer.
                let track_fbo = self.track_t.as_ref().unwrap().fbo;
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(track_fbo));
                gl.viewport(0, 0, self.size.0, self.size.1);
                gl.clear_color(0.0, 0.0, 0.0, 0.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                let mut track_any = !track.effects.is_empty();

                for cp in &plans {
                    let clip = &track.clips[cp.clip_index];
                    let any_active = cp
                        .parts
                        .iter()
                        .any(|(_, evs)| evs.iter().any(|e| t >= e.onset && t < e.end()));
                    if !any_active && clip.effects.is_empty() {
                        continue;
                    }
                    // Clip layer.
                    let clip_target = self.clip_t.as_ref().unwrap();
                    self.bind_target(gl, clip_target, Some([0.0, 0.0, 0.0, 0.0]));
                    if any_active {
                        gl.enable(glow::BLEND);
                        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                        for (sid, evs) in &cp.parts {
                            let Some(gsrc) = self.sources.get(sid) else { continue };
                            let (gtex, layers, dur) =
                                (gsrc.tex, gsrc.layers, gsrc.duration.max(1e-6));
                            let style: VisualGrainStyle = cp.style;
                            let draws =
                                grain_draws(evs, &style, &clip.link, dur, t);
                            let p = self.use_program(gl, "grain");
                            gl.active_texture(glow::TEXTURE0);
                            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(gtex));
                            self.set_i(gl, p, "u_src", 0);
                            let (mode, cf, valmin) = match &clip.color_filter {
                                None => (0, [0.0f32; 4], 0.0),
                                Some(c) => (
                                    if c.mode == ColorFilterMode::Keep { 1 } else { 2 },
                                    [c.hue_center, c.hue_width / 2.0, c.softness, c.sat_min],
                                    c.val_min,
                                ),
                            };
                            self.set_i(gl, p, "u_cf_mode", mode);
                            self.set_4f(gl, p, "u_cf", cf);
                            self.set_f(gl, p, "u_cf_valmin", valmin);
                            self.set_f(gl, p, "u_additive", style.additive);
                            for d in draws {
                                let layer = ((d.src_time / dur).rem_euclid(1.0)
                                    * layers as f64)
                                    .min(layers as f64 - 1.0)
                                    as f32;
                                self.set_f(gl, p, "u_layer", layer.floor());
                                self.set_4f(gl, p, "u_patch", d.patch);
                                self.set_f(gl, p, "u_alpha", d.alpha);
                                self.set_f(gl, p, "u_hue", d.hue_shift);
                                self.set_4f(gl, p, "u_rect", d.dest);
                                self.draw_quad(gl);
                            }
                        }
                        gl.disable(glow::BLEND);
                    }
                    let clip_tex = self.clip_t.as_ref().unwrap().tex;
                    let final_tex = self.apply_chain(
                        gl,
                        &clip.effects,
                        ((ti as u64) << 20) | (cp.clip_index as u64 + 1),
                        clip_tex,
                        fps,
                    );
                    // Blend clip layer into track layer.
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(track_fbo));
                    gl.viewport(0, 0, self.size.0, self.size.1);
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                    let p = self.use_program(gl, "blit");
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(final_tex));
                    self.set_i(gl, p, "u_tex", 0);
                    self.set_f(gl, p, "u_opacity", 1.0);
                    self.set_f(gl, p, "u_additive", cp.style.additive * 0.5);
                    self.set_f(gl, p, "u_alpha_mul", 1.0);
                    self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                    self.draw_quad(gl);
                    gl.disable(glow::BLEND);
                    track_any = true;
                }
                if !track_any {
                    continue;
                }
                let track_tex = self.track_t.as_ref().unwrap().tex;
                let final_tex =
                    self.apply_chain(gl, &track.effects, (ti as u64 + 1) << 32, track_tex, fps);
                // Composite track onto master.
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(master_fbo));
                gl.viewport(0, 0, self.size.0, self.size.1);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                let p = self.use_program(gl, "blit");
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(final_tex));
                self.set_i(gl, p, "u_tex", 0);
                let beat = t * scene.project.bpm / 60.0;
                self.set_f(gl, p, "u_opacity", track.opacity_at(beat));
                self.set_f(gl, p, "u_additive", 0.0);
                self.set_f(gl, p, "u_alpha_mul", 1.0);
                self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
                self.draw_quad(gl);
                gl.disable(glow::BLEND);
            }

            // Master chain.
            let master_tex = self.master.as_ref().unwrap().tex;
            let final_tex = self.apply_chain(
                gl,
                &scene.project.master_effects,
                0xFFFF_0000_0000,
                master_tex,
                fps,
            );

            // Present into the egui viewport.
            gl.bind_framebuffer(
                glow::FRAMEBUFFER,
                if prev_fbo != 0 {
                    Some(glow::NativeFramebuffer(
                        std::num::NonZeroU32::new(prev_fbo as u32).unwrap(),
                    ))
                } else {
                    None
                },
            );
            let vp = info.viewport_in_pixels();
            gl.viewport(vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px);
            let clip = info.clip_rect_in_pixels();
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(clip.left_px, clip.from_bottom_px, clip.width_px, clip.height_px);
            gl.disable(glow::BLEND);
            let p = self.use_program(gl, "present");
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(final_tex));
            self.set_i(gl, p, "u_tex", 0);
            self.set_4f(gl, p, "u_rect", [0.0, 0.0, 1.0, 1.0]);
            self.draw_quad(gl);

            // Restore state egui expects.
            if !scissor_was_on {
                gl.disable(glow::SCISSOR_TEST);
            }
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE_MINUS_DST_ALPHA,
                glow::ONE,
            );
        }
    }
}

fn bytemuck_cast(v: &[f32; 12]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
