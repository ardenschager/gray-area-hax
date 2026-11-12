//! The UX showcase film: the real studio UI, driven by a scripted tour of
//! touches and rendered offscreen (egui_kittest + wgpu) at a fixed 30 fps,
//! while the same audio engine is stepped offline exactly one frame of
//! samples per rendered frame. Never opens a window or an audio device.
//!
//! ```text
//! CHROMAGRAIN_SHOWCASE=v001 cargo test -p chromagrain --release showcase_film -- --ignored --nocapture
//! ```
//!
//! writes `outputs/261112_ux-showcase/<v>/work/frames/*.png` + the wav and
//! muxes `outputs/261112_ux-showcase/<v>/261112_chromagrain_ux-showcase_<v>.mp4`
//! (refuses to overwrite an existing version). The tour itself is
//! [`tour`]: a list of gestures and captions on a timeline.

use crate::demo::DemoSpec;
use crate::{App, AppOpts, Clock};
use chromagrain_core::timeline::Source;
use chromagrain_ui::theme::{font, font_medium, font_semibold, G0, INK, INK2, INK3};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Stroke};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub const FPS: f64 = 30.0;
/// Height of the caption band under the UI (points).
pub const BAND: f32 = 58.0;

// ------------------------------------------------------------------ tour

/// Where a finger goes; resolved against the live UI when the gesture
/// starts (widget rects come from the accessibility tree).
#[derive(Debug, Clone)]
pub enum Aim {
    /// Centre of the widget with this label.
    Label(String),
    /// Fractions of the screen.
    Screen(f32, f32),
    /// Where emitter `i`'s ring is right now (grab it); with `Off`, the
    /// offset is in grid columns / rows.
    Emitter(usize),
    /// Another aim, moved by points.
    Off(Box<Aim>, f32, f32),
}

pub fn lab(s: &str) -> Aim {
    Aim::Label(s.into())
}


impl Aim {
    pub fn off(self, dx: f32, dy: f32) -> Aim {
        Aim::Off(Box::new(self), dx, dy)
    }
}

#[derive(Clone)]
pub enum Gesture {
    /// Press and release (pointer).
    Tap(Aim),
    /// Press, stay, release after `secs` (pointer).
    Hold(Aim, f64),
    /// Press at the first aim, glide through the rest, release (pointer).
    Drag(Vec<Aim>, f64),
    /// The same with a touch (several can overlap each other and the
    /// pointer: multi-touch).
    Touch(Vec<Aim>, f64),
    Key(egui::Key),
    /// Turn a knob: press on it and drag vertically by `dy` points at
    /// each (seconds, dy) key. The ring stays on the knob.
    Knob(&'static str, Vec<(f64, f32)>),
}

pub struct Cue {
    pub t: f64,
    pub g: Gesture,
}

pub struct Caption {
    pub t0: f64,
    pub t1: f64,
    pub text: &'static str,
}

pub struct Tour {
    pub cues: Vec<Cue>,
    pub captions: Vec<Caption>,
    /// Title card fades out by this time; the end card fades in from
    /// `end_card`; the film ends at `len`.
    pub title_until: f64,
    pub end_card: f64,
    pub len: f64,
}

impl Gesture {
    fn secs(&self) -> f64 {
        match self {
            Gesture::Tap(_) => 0.14,
            Gesture::Hold(_, s) | Gesture::Drag(_, s) | Gesture::Touch(_, s) => *s,
            Gesture::Key(_) => 0.0,
            Gesture::Knob(_, keys) => keys.last().map_or(0.0, |k| k.0),
        }
    }
    fn aims(&self) -> Vec<Aim> {
        match self {
            Gesture::Tap(a) | Gesture::Hold(a, _) => vec![a.clone()],
            Gesture::Drag(v, _) | Gesture::Touch(v, _) => v.clone(),
            Gesture::Knob(l, _) => vec![lab(l)],
            _ => Vec::new(),
        }
    }
    fn is_touch(&self) -> bool {
        matches!(self, Gesture::Touch(..))
    }
}

/// The film's scene setup: the Night Platform demo, with a longer first
/// scene so the Sequence edits are heard before the next scene arrives.
pub fn setup(app: &mut App) {
    app.studio.seq.chain = vec![(0, 12), (1, 4), (2, 4)];
    app.studio.rev += 1;
}

/// The tour: Play → Sequence → Perform → Deep, about 80 s.
pub fn tour() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(Caption { t0, t1, text });
    let em = |i: usize| Aim::Emitter(i);
    let rows = |i: usize, path: &[(f32, f32)]| -> Vec<Aim> { path.iter().map(|(dc, dr)| Aim::Off(Box::new(em(i)), *dc, *dr)).collect() };

    // ---- Play: touch the picture. The first finger lands under the title.
    at(0.3, Touch(rows(0, &[(0.0, 0.0), (0.04, 0.6), (0.0, 1.8)]), 5.4));
    say(2.2, 5.8, "touch a clip to play it");
    say(6.6, 10.8, "two fingers, two emitters");
    at(6.8, Touch(rows(0, &[(0.0, 0.0), (0.03, 0.5), (0.0, 1.0)]), 4.2));
    at(7.2, Touch(rows(1, &[(0.0, 0.0), (-0.04, 0.8), (0.0, 1.8)]), 3.8));
    // One finger keeps playing while the other turns the knobs.
    at(11.4, Touch(rows(0, &[(0.0, 0.0), (0.0, 0.5)]), 15.6));
    say(11.8, 13.9, "grain · sample to cloud");
    at(12.0, Knob("Grain", vec![(0.0, 0.0), (1.5, -118.0)]));
    say(14.6, 16.5, "pitch · always in key");
    at(14.8, Knob("Pitch", vec![(0.0, 0.0), (1.1, -48.0)]));
    say(17.2, 19.1, "smear · space and trails");
    at(17.4, Knob("Smear", vec![(0.0, 0.0), (1.2, -80.0)]));
    // LOOK: slip quickly past crush and jpeg, settle on tint, then kaleido.
    say(19.8, 22.4, "look · picture and sound together");
    at(20.0, Knob("Look", vec![(0.0, 0.0), (0.25, -26.0), (0.42, -132.0), (1.3, -140.0), (1.9, -205.0)]));
    // Deep: the emitter's own page, one tap away.
    at(23.0, Tap(lab("emitter settings")));
    say(23.4, 26.8, "deep · every sound ↔ picture link is a dial");
    at(24.4, Drag(vec![lab("pitch → hue"), lab("pitch → hue").off(150.0, 0.0)], 1.4));
    at(26.6, Tap(lab("Done")));
    // Rec: what you play becomes a take, and plays back hands-free.
    say(27.8, 31.2, "rec turns a phrase into a take");
    at(28.0, Tap(lab("rec")));
    at(28.6, Touch(rows(1, &[(0.0, 0.0), (0.06, 0.7), (0.0, 1.4)]), 2.2));
    at(31.2, Tap(lab("rec")));
    say(32.0, 35.0, "hands off · it plays back");
    at(32.2, Tap(lab("play")));

    // ---- Sequence: tap, drag, hold; scenes and the chain.
    let s = 35.8;
    at(s, Tap(lab("Sequence")));
    say(s + 0.4, s + 2.8, "one lane per clip");
    say(s + 3.5, s + 7.0, "tap a step");
    at(s + 3.6, Tap(lab("lane 6 step 9")));
    at(s + 4.2, Tap(lab("lane 6 step 13")));
    at(s + 4.8, Tap(lab("lane 7 step 7")));
    at(s + 5.5, Tap(lab("lane 7 step 15")));
    say(s + 7.7, s + 10.6, "drag up · pitch");
    at(s + 7.8, Drag(vec![lab("lane 6 step 9"), lab("lane 6 step 9").off(0.0, -50.0)], 1.0));
    at(s + 9.2, Drag(vec![lab("lane 7 step 15"), lab("lane 7 step 15").off(0.0, -34.0)], 0.8));
    say(s + 11.3, s + 15.2, "hold a step · slice, length, chance");
    at(s + 11.4, Hold(lab("lane 5 step 5"), 0.7));
    at(s + 12.7, Tap(lab("slice 12")));
    at(s + 13.6, Drag(vec![lab("chance"), lab("chance").off(0.0, 62.0)], 0.8));
    at(s + 15.0, Key(egui::Key::Escape));
    say(s + 15.9, s + 18.6, "queue a scene · it lands on the bar");
    at(s + 16.0, Tap(lab("scene 3")));
    say(s + 19.3, s + 22.2, "an empty scene extends the song");
    at(s + 19.6, Tap(lab("scene 4")));

    // ---- Perform: layers on the bar, then the picture fills the screen.
    let p = s + 23.0;
    at(p, Tap(lab("Perform")));
    say(p + 0.6, p + 5.6, "pads launch on the bar");
    at(p + 1.2, Tap(lab("pad 5")));
    at(p + 2.9, Tap(lab("pad 6")));
    at(p + 4.6, Tap(lab("pad 7")));
    say(p + 6.6, p + 9.6, "fullscreen · tap for the hud");
    at(p + 6.8, Tap(lab("Fullscreen")));
    at(p + 7.8, Tap(Aim::Screen(0.5, 0.42)));
    at(p + 13.6, Tap(Aim::Screen(0.46, 0.4)));
    say(p + 14.0, p + 16.8, "launch from the hud");
    at(p + 14.6, Tap(lab("mini pad 11")));
    Tour { cues, captions: cap, title_until: 2.0, end_card: p + 18.4, len: p + 23.2 }
}

// --------------------------------------------------------------- overlay

/// A finger as the overlay draws it.
#[derive(Debug, Clone, Copy)]
struct Finger {
    pos: Pos2,
    /// Seconds since it touched down.
    age: f64,
    /// Seconds since it lifted (None = still down).
    up: Option<f64>,
    /// Turning a knob: drawn with a small "drag up" chevron.
    knob: bool,
}

/// What the film paints over the UI this frame.
#[derive(Default)]
pub struct Overlay {
    fingers: Vec<(Finger, Vec<Pos2>)>,
    caption: Option<(&'static str, f32)>,
    /// No caption band (fullscreen): captions float over the picture.
    floating: bool,
    /// Title card / end card opacity.
    title: f32,
    end: f32,
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Overlay {
    fn paint(&self, ctx: &egui::Context) {
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("showcase")));
        let screen = ctx.screen_rect();
        for (f, trail) in &self.fingers {
            let press = smooth(f.age / 0.16) as f32;
            let lift = f.up.map(|u| smooth(u / 0.42) as f32).unwrap_or(0.0);
            let a = press * (1.0 - lift);
            let rad = 30.0 - 6.0 * press + 20.0 * lift;
            if trail.len() > 1 {
                for (i, w) in trail.windows(2).enumerate() {
                    let k = (i + 1) as f32 / trail.len() as f32;
                    p.line_segment([w[0], w[1]], Stroke::new(2.0 * k, Color32::from_white_alpha((34.0 * k * a) as u8)));
                }
            }
            p.circle_filled(f.pos, rad, Color32::from_white_alpha((30.0 * a) as u8));
            p.circle_stroke(f.pos, rad, Stroke::new(1.5, Color32::from_white_alpha((150.0 * a) as u8)));
            p.circle_filled(f.pos, 3.5, Color32::from_white_alpha((120.0 * a * (1.0 - lift)) as u8));
            if f.knob {
                let c = f.pos - egui::vec2(0.0, rad + 12.0);
                let st = Stroke::new(1.5, Color32::from_white_alpha((120.0 * a) as u8));
                p.line_segment([c + egui::vec2(-6.0, 4.0), c], st);
                p.line_segment([c, c + egui::vec2(6.0, 4.0)], st);
            }
        }
        if let Some((text, a)) = self.caption {
            // In the caption band under the UI: quiet grey, no box.
            if self.floating {
                let g = p.layout_no_wrap(text.into(), font(16.0), INK);
                let size = g.size() + egui::vec2(36.0, 20.0);
                let c = egui::pos2(screen.center().x, screen.bottom() - 34.0 - size.y / 2.0 + 4.0 * (1.0 - a));
                let rr = egui::CornerRadius::same((size.y / 2.0) as u8);
                p.rect_filled(Rect::from_center_size(c, size), rr, Color32::from_black_alpha((120.0 * a) as u8));
                p.text(c, Align2::CENTER_CENTER, text, font(16.0), INK.gamma_multiply(0.8 * a));
            } else {
                let c = egui::pos2(screen.center().x, screen.bottom() - BAND / 2.0 + 4.0 * (1.0 - a));
                p.text(c, Align2::CENTER_CENTER, text, font(16.0), INK2.gamma_multiply(a));
            }
        }
        for (a, lines) in [(self.title, ["chromagrain", "a granular video sampler"]), (self.end, ["chromagrain", "play  ·  sequence  ·  perform"])] {
            if a <= 0.0 {
                continue;
            }
            p.rect_filled(screen, 0.0, G0.gamma_multiply(a));
            let c = screen.center();
            p.text(c - egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, lines[0], font_semibold(46.0), INK.gamma_multiply(a));
            p.text(c + egui::vec2(0.0, 34.0), Align2::CENTER_CENTER, lines[1], font_medium(16.0), INK3.gamma_multiply(a));
        }
    }
}

/// The app inside the film: the real `App`, plus the overlay on top.
pub struct Film {
    pub app: App,
    overlay: Overlay,
}

impl eframe::App for Film {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // The caption band claims the bottom before the studio lays out,
        // so captions never cover the interface.
        // (Fullscreen output goes edge to edge: no band, captions float.)
        if !self.app.fullscreen {
            egui::TopBottomPanel::bottom("film-band")
                .exact_height(BAND)
                .frame(egui::Frame::new().fill(G0))
                .show_separator_line(false)
                .show(ctx, |_| {});
        }
        eframe::App::update(&mut self.app, ctx, frame);
        self.overlay.paint(ctx);
    }
}

// -------------------------------------------------------------- recorder

pub struct RecordOpts {
    /// Logical size and scale (output pixels = size × ppp).
    pub size: (f32, f32),
    pub ppp: f32,
    /// Where PNG frames go (None = render but don't write).
    pub frames_dir: Option<PathBuf>,
    /// Stop after this many seconds (smoke tests).
    pub max_secs: Option<f64>,
}

pub struct Recording {
    pub frames: usize,
    /// Interleaved stereo at `sample_rate`, exactly `frames` frames long.
    pub audio: Vec<f32>,
    pub sample_rate: u32,
    pub px: (u32, u32),
}

/// The Night Platform clips (decoded once per process), or stand-ins.
fn demo_sources(spec: &DemoSpec) -> &'static Vec<Source> {
    static S: OnceLock<Vec<Source>> = OnceLock::new();
    S.get_or_init(|| {
        spec.load_found(48000).unwrap_or_else(|e| {
            eprintln!("showcase: {e} — procedural stand-ins");
            spec.clips
                .iter()
                .enumerate()
                .map(|(i, c)| Source {
                    name: c.name.into(),
                    audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(110.0 * (1.0 + i as f32 * 0.2), 4.0, 48000))),
                    video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(160, 90, 12.0, 4.0))),
                    base_hz: c.base_hz,
                })
                .collect()
        })
    })
}

fn harness(opts: &RecordOpts) -> Harness<'static, Film> {
    let spec = DemoSpec::night_platform();
    let sources = demo_sources(&spec).clone();
    Harness::builder()
        .with_size(egui::vec2(opts.size.0, opts.size.1))
        .with_pixels_per_point(opts.ppp)
        .with_step_dt(1.0 / FPS as f32)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = App::new(AppOpts { demo: false, classic: false, headless: true });
            app.playback = crate::playback::Playback::new_offline();
            app.playback.offline_latency = 1.0 / FPS;
            app.app_clock = Clock::Sim(0.0);
            app.studio.install_theme(&cc.egui_ctx);
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.2);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }
            app.install_demo(&spec, sources);
            setup(&mut app);
            Film { app, overlay: Overlay::default() }
        })
}

/// A widget's rect in points (the accessibility tree is in pixels).
fn rect_of(h: &Harness<'static, Film>, label: &str) -> Rect {
    let r = h.get_by_label(label).rect();
    let ppp = h.ctx.pixels_per_point();
    Rect::from_min_max((r.min.to_vec2() / ppp).to_pos2(), (r.max.to_vec2() / ppp).to_pos2())
}

/// The Play grid's rect and cell size (as `play.rs` lays it out).
fn grid_geom(h: &Harness<'static, Film>) -> (Rect, f32, f32) {
    let g = rect_of(h, "play grid");
    (g, (g.width() - 4.0 * 7.0) / 8.0, (g.height() - 4.0 * 7.0) / 8.0)
}

fn resolve(h: &Harness<'static, Film>, aim: &Aim) -> Pos2 {
    match aim {
        Aim::Label(l) => rect_of(h, l).center(),
        Aim::Screen(fx, fy) => {
            let s = h.ctx.screen_rect();
            egui::pos2(s.left() + s.width() * fx, s.top() + s.height() * fy)
        }
        Aim::Emitter(i) => {
            let (g, cw, ch) = grid_geom(h);
            let e = &h.state().app.studio.emitters[*i];
            let y = (g.top() + e.row * (ch + 4.0)).clamp(g.top() + 16.0, g.bottom() - 16.0);
            egui::pos2(g.left() + e.col as f32 * (cw + 4.0) + cw / 2.0, y)
        }
        Aim::Off(a, dx, dy) if matches!(**a, Aim::Emitter(_)) => {
            let (_, cw, ch) = grid_geom(h);
            resolve(h, a) + egui::vec2(dx * (cw + 4.0), dy * (ch + 4.0))
        }
        Aim::Off(a, dx, dy) => resolve(h, a) + egui::vec2(*dx, *dy),
    }
}

/// A knob gesture's drag offset at `dt` seconds: smooth between keys.
fn knob_dy(keys: &[(f64, f32)], dt: f64) -> f32 {
    let Some(last) = keys.last() else { return 0.0 };
    if dt >= last.0 {
        return last.1;
    }
    for w in keys.windows(2) {
        if dt >= w[0].0 && dt < w[1].0 {
            let k = smooth((dt - w[0].0) / (w[1].0 - w[0].0).max(1e-6)) as f32;
            return w[0].1 + (w[1].1 - w[0].1) * k;
        }
    }
    keys[0].1
}

/// Point along a polyline at eased progress `t` (0..1), by arc length.
fn along(pts: &[Pos2], t: f64) -> Pos2 {
    if pts.len() == 1 {
        return pts[0];
    }
    let t = smooth(t) as f32;
    let lens: Vec<f32> = pts.windows(2).map(|w| (w[1] - w[0]).length()).collect();
    let total: f32 = lens.iter().sum::<f32>().max(1e-3);
    let mut d = t * total;
    for (i, l) in lens.iter().enumerate() {
        if d <= *l || i == lens.len() - 1 {
            let k = if *l > 0.0 { (d / l).clamp(0.0, 1.0) } else { 1.0 };
            return pts[i] + (pts[i + 1] - pts[i]) * k;
        }
        d -= l;
    }
    *pts.last().unwrap()
}

struct Live {
    cue: usize,
    /// Knobs: the ring stays here while the pointer drags.
    anchor: Option<Pos2>,
    pts: Vec<Pos2>,
    touch_id: u64,
    trail: Vec<Pos2>,
    up_at: Option<f64>,
    last: Pos2,
}

/// Render the tour: every frame to PNG (if asked) and the audio, in sync.
pub fn record(tour: &Tour, opts: &RecordOpts) -> Recording {
    let mut h = harness(opts);
    // Warm up at t = 0: fonts, atlases, the first engine sync.
    for _ in 0..6 {
        h.input_mut().time = Some(0.0);
        h.step();
    }
    let sr = h.state().app.project.lock().unwrap().sample_rate;
    let spf = (sr as f64 / FPS).round() as usize;
    let len = opts.max_secs.map_or(tour.len, |m| m.min(tour.len));
    let n = (len * FPS).round() as usize;
    let mut audio = Vec::with_capacity(n * spf * 2);
    let mut live: Vec<Live> = Vec::new();
    let mut started = vec![false; tour.cues.len()];
    let mut next_touch = 10u64;
    let mut px = (0, 0);
    // PNGs are written on worker threads.
    let (tx, rx) = std::sync::mpsc::sync_channel::<(PathBuf, image::RgbaImage)>(8);
    let rx = Arc::new(std::sync::Mutex::new(rx));
    let writers: Vec<_> = (0..4)
        .map(|_| {
            let rx = rx.clone();
            std::thread::spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                let Ok((path, img)) = job else { break };
                let f = std::io::BufWriter::new(std::fs::File::create(&path).expect("frame file"));
                let enc = image::codecs::png::PngEncoder::new_with_quality(
                    f,
                    image::codecs::png::CompressionType::Fast,
                    image::codecs::png::FilterType::Adaptive,
                );
                img.write_with_encoder(enc).expect("png");
            })
        })
        .collect();
    if let Some(d) = &opts.frames_dir {
        std::fs::create_dir_all(d).unwrap();
    }

    for k in 0..n {
        let t = k as f64 / FPS;
        h.state_mut().app.app_clock = Clock::Sim(t);
        // Start cues whose time has come.
        for (ci, cue) in tour.cues.iter().enumerate() {
            if started[ci] || cue.t > t + 1e-9 {
                continue;
            }
            started[ci] = true;
            match &cue.g {
                Gesture::Key(key) => {
                    h.input_mut().events.push(egui::Event::Key { key: *key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
                    h.input_mut().events.push(egui::Event::Key { key: *key, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
                }
                g => {
                    let pts: Vec<Pos2> = g.aims().iter().map(|a| resolve(&h, a)).collect();
                    let p0 = pts[0];
                    let touch_id = if g.is_touch() {
                        next_touch += 1;
                        h.input_mut().events.push(touch(next_touch, egui::TouchPhase::Start, p0));
                        next_touch
                    } else {
                        h.input_mut().events.push(egui::Event::PointerMoved(p0));
                        h.input_mut().events.push(egui::Event::PointerButton { pos: p0, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
                        0
                    };
                    let anchor = matches!(g, Gesture::Knob(..)).then_some(p0);
                    live.push(Live { cue: ci, anchor, pts, touch_id, trail: vec![p0], up_at: None, last: p0 });
                }
            }
        }
        // Move / release live fingers.
        for l in live.iter_mut().filter(|l| l.up_at.is_none()) {
            let cue = &tour.cues[l.cue];
            let dur = cue.g.secs().max(1e-3);
            let prog = (t - cue.t) / dur;
            let pos = match &cue.g {
                Gesture::Knob(_, keys) => l.pts[0] + egui::vec2(0.0, knob_dy(keys, t - cue.t)),
                _ => along(&l.pts, prog.min(1.0)),
            };
            if prog >= 1.0 {
                l.up_at = Some(t);
                if l.touch_id > 0 {
                    h.input_mut().events.push(touch(l.touch_id, egui::TouchPhase::End, pos));
                } else {
                    h.input_mut().events.push(egui::Event::PointerMoved(pos));
                    h.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
                    h.input_mut().events.push(egui::Event::PointerGone);
                }
            } else if (pos - l.last).length() > 0.01 {
                if l.touch_id > 0 {
                    h.input_mut().events.push(touch(l.touch_id, egui::TouchPhase::Move, pos));
                } else {
                    h.input_mut().events.push(egui::Event::PointerMoved(pos));
                }
            }
            l.last = pos;
            l.trail.push(pos);
            if l.trail.len() > 8 {
                l.trail.remove(0);
            }
        }
        live.retain(|l| l.up_at.is_none_or(|u| t - u < 0.5));
        // Overlay for this frame.
        let fingers = live
            .iter()
            .map(|l| match l.anchor {
                Some(a) => (Finger { pos: a, age: t - tour.cues[l.cue].t, up: l.up_at.map(|u| t - u), knob: true }, Vec::new()),
                None => (Finger { pos: l.last, age: t - tour.cues[l.cue].t, up: l.up_at.map(|u| t - u), knob: false }, l.trail.clone()),
            })
            .collect();
        let caption = tour.captions.iter().find(|c| t >= c.t0 - 0.35 && t < c.t1 + 0.35).map(|c| {
            let a = smooth((t - c.t0 + 0.35) / 0.35).min(smooth((c.t1 + 0.35 - t) / 0.35));
            (c.text, a as f32)
        });
        let title = 1.0 - smooth((t - (tour.title_until - 0.9)) / 0.9);
        let end = smooth((t - tour.end_card) / 1.0);
        let floating = h.state().app.fullscreen;
        h.state_mut().overlay = Overlay { fingers, caption, floating, title: title as f32, end: end as f32 };
        h.input_mut().time = Some(t);
        h.step();
        // Background jobs (a Rec take bouncing) finish before time moves on,
        // so the film is the same every run.
        let mut waited = 0;
        while h.state().app.busy.is_some() && waited < 3000 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.state_mut().app.poll_workers();
            waited += 1;
        }
        let img = h.render().expect("render");
        px = (img.width(), img.height());
        if let Some(d) = &opts.frames_dir {
            tx.send((d.join(format!("{k:05}.png")), img)).unwrap();
        }
        let (l, r) = h.state_mut().app.playback.render_offline(spf);
        for i in 0..spf {
            audio.push(l[i]);
            audio.push(r[i]);
        }
        if k % 150 == 0 {
            eprintln!("showcase: {:5.1}s / {len:.0}s", t);
        }
    }
    drop(tx);
    for w in writers {
        w.join().unwrap();
    }
    Recording { frames: n, audio, sample_rate: sr, px }
}

fn touch(id: u64, phase: egui::TouchPhase, pos: Pos2) -> egui::Event {
    egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos, force: None }
}

/// Write interleaved stereo f32 as a 16-bit wav.
fn write_wav(path: &Path, audio: &[f32], sr: u32) {
    use std::io::Write;
    let data_len = (audio.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sr.to_le_bytes()).unwrap();
    f.write_all(&(sr * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for s in audio {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}

/// A gentle glue compressor: the quiet Play opening and the full Perform
/// set sit closer together without pumping.
const COMP: &str = "acompressor=threshold=-24dB:ratio=2:attack=30:release=300:knee=6";

/// Mux frames + audio: H.264 + AAC, loudness-normalised (two-pass
/// loudnorm) with a soft fade at the end.
fn mux(frames: &Path, wav: &Path, out: &Path, fade_from: f64, len: f64) -> Result<(), String> {
    // Sound fades with the end card and is silent before the last frame.
    let fade = format!("afade=t=out:st={:.2}:d={:.2}", fade_from, (len - fade_from - 0.4).max(0.5));
    let pass1 = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(wav)
        .args(["-af", &format!("{COMP},{fade},loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json"), "-f", "null", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    let log = String::from_utf8_lossy(&pass1.stderr);
    let get = |k: &str| -> Option<String> {
        let i = log.rfind(&format!("\"{k}\""))?;
        let rest = &log[i..];
        let v = rest.split(':').nth(1)?.split('"').nth(1)?;
        Some(v.to_string())
    };
    let norm = match (get("input_i"), get("input_tp"), get("input_lra"), get("input_thresh"), get("target_offset")) {
        (Some(i), Some(tp), Some(lra), Some(th), Some(off)) => format!(
            "loudnorm=I=-16:TP=-1.5:LRA=11:measured_I={i}:measured_TP={tp}:measured_LRA={lra}:measured_thresh={th}:offset={off}:linear=true"
        ),
        _ => "loudnorm=I=-16:TP=-1.5:LRA=11".into(),
    };
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-framerate", "30", "-i"])
        .arg(frames.join("%05d.png"))
        .arg("-i")
        .arg(wav)
        .args([
            "-af",
            &format!("{COMP},{fade},{norm},aresample=48000"),
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "19",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "256k",
            "-movflags",
            "+faststart",
            "-shortest",
        ])
        .arg(out)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with {st}"))
    }
}

// ----------------------------------------------------------------- tests

#[test]
fn the_tour_is_well_formed() {
    let tour = tour();
    // One pointer at a time; touches may overlap.
    let mut pointer: Vec<(f64, f64)> = tour.cues.iter().filter(|c| matches!(c.g, Gesture::Tap(_) | Gesture::Hold(..) | Gesture::Drag(..) | Gesture::Knob(..))).map(|c| (c.t, c.t + c.g.secs())).collect();
    pointer.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in pointer.windows(2) {
        assert!(w[0].1 + 1.0 / FPS < w[1].0, "pointer gestures overlap at {:.2}s", w[1].0);
    }
    let mut caps: Vec<(f64, f64)> = tour.captions.iter().map(|c| (c.t0, c.t1)).collect();
    caps.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in caps.windows(2) {
        assert!(w[0].1 + 0.7 <= w[1].0 + 1e-6, "captions overlap / crowd at {:.2}s", w[1].0);
    }
    assert!(tour.captions.iter().all(|c| c.text == c.text.to_lowercase()), "captions are lowercase");
    assert!(tour.cues.iter().all(|c| c.t + c.g.secs() < tour.end_card));
    assert!((60.0..=90.0).contains(&tour.len));
}

#[test]
fn the_recorder_renders_frames_and_audio_in_lockstep() {
    // A short, small run: frames and samples stay locked, the first touch
    // plays grains that are heard, and the views are reachable.
    let tour = tour();
    let rec = record(&tour, &RecordOpts { size: (960.0, 600.0), ppp: 1.0, frames_dir: None, max_secs: Some(4.5) });
    assert_eq!(rec.frames, 135);
    assert_eq!(rec.audio.len(), rec.frames * (rec.sample_rate as usize / FPS as usize) * 2, "one frame of samples per frame");
    assert_eq!(rec.px, (960, 600));
    let rms = |a: &[f32]| (a.iter().map(|x| x * x).sum::<f32>() / a.len().max(1) as f32).sqrt();
    let spf2 = rec.sample_rate as usize / FPS as usize * 2;
    let before = rms(&rec.audio[..spf2 * 8]);
    let after = rms(&rec.audio[spf2 * 30..]);
    assert!(after > 1e-3 && after > before * 4.0, "the first touch is heard ({before} → {after})");
}

#[test]
#[ignore = "renders the full film (minutes); see the module docs"]
fn showcase_film() {
    let version = std::env::var("CHROMAGRAIN_SHOWCASE").unwrap_or_else(|_| "v001".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../outputs/261112_ux-showcase").join(&version);
    let out = root.join(format!("261112_chromagrain_ux-showcase_{version}.mp4"));
    assert!(!out.exists(), "{} exists — versions are never overwritten", out.display());
    let frames = root.join("work/frames");
    let _ = std::fs::remove_dir_all(&frames);
    let tour = tour();
    let t0 = std::time::Instant::now();
    let rec = record(&tour, &RecordOpts { size: (1600.0, 900.0), ppp: 1.2, frames_dir: Some(frames.clone()), max_secs: None });
    eprintln!("showcase: {} frames ({}×{}) in {:.0}s", rec.frames, rec.px.0, rec.px.1, t0.elapsed().as_secs_f64());
    let wav = root.join(format!("work/261112_chromagrain_ux-showcase_{version}.wav"));
    write_wav(&wav, &rec.audio, rec.sample_rate);
    mux(&frames, &wav, &out, tour.end_card + 0.2, tour.len).expect("mux");
    eprintln!("showcase: wrote {}", out.display());
}
