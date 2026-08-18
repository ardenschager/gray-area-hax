//! chromagrain — audiovisual granular sampling compositor / sequencer /
//! performance tool.
//!
//! GUI:       chromagrain
//! Headless:  chromagrain --script composition.rhai [base_dir]
//!            chromagrain --render-demo out.mp4

mod gpu;
mod playback;

use chromagrain_core::dsp::{FilterKind, FilterSpec};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::media;
use chromagrain_core::music::{Key, ScaleKind, PITCH_CLASS_NAMES};
use chromagrain_core::render::{plan_project, render_project, ClipPlan};
use chromagrain_core::script::run_script;
use chromagrain_core::seq::StepPattern;
use chromagrain_core::timeline::{Clip, ClipKind, Project, Source};
use chromagrain_core::video::{ColorFilter, ColorFilterMode};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--script" {
        let script = std::fs::read_to_string(&args[2]).unwrap_or_else(|e| {
            eprintln!("cannot read {}: {e}", args[2]);
            std::process::exit(1);
        });
        let base = args
            .get(3)
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap());
        let project = Arc::new(Mutex::new(Project::default()));
        match run_script(project, &base, &script) {
            Ok(log) => {
                print!("{log}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("script error: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.len() >= 3 && args[1] == "--render-demo" {
        let p = Project::demo();
        let out = render_project(&p, 0.0, p.end_beat());
        let path = PathBuf::from(&args[2]);
        let res = if path.extension().is_some_and(|e| e == "wav") {
            media::save_wav(&path, &out.audio)
        } else {
            media::encode_video(&path, &out.frames, out.fps, &out.audio)
        };
        match res {
            Ok(()) => {
                println!(
                    "rendered demo: {} ({} frames, {:.1}s audio)",
                    path.display(),
                    out.frames.len(),
                    out.audio.duration()
                );
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("render failed: {e}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1360.0, 880.0]),
        ..Default::default()
    };
    eframe::run_native(
        "chromagrain",
        options,
        Box::new(|cc| {
            style(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// Minimal, tasteful: near-black surfaces, one warm accent, quiet chrome.
fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    let bg = egui::Color32::from_rgb(16, 16, 18);
    let panel = egui::Color32::from_rgb(22, 22, 25);
    let accent = egui::Color32::from_rgb(255, 122, 89);
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = bg;
    v.faint_bg_color = egui::Color32::from_rgb(28, 28, 32);
    v.widgets.noninteractive.bg_fill = panel;
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(34, 34, 38);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 46, 52);
    v.widgets.active.bg_fill = accent.linear_multiply(0.4);
    v.selection.bg_fill = accent.linear_multiply(0.35);
    v.selection.stroke = egui::Stroke::new(1.0, accent);
    v.hyperlink_color = accent;
    v.widgets.noninteractive.fg_stroke.color = egui::Color32::from_gray(190);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        s.spacing.slider_width = 150.0;
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    let spaced: String = title
        .to_uppercase()
        .chars()
        .flat_map(|c| [c, '\u{2009}'])
        .collect();
    ui.label(
        egui::RichText::new(spaced)
            .size(10.5)
            .color(egui::Color32::from_gray(140)),
    );
    ui.separator();
}

enum WorkerMsg {
    Source(Result<Source, String>),
    Script(Result<String, String>),
    Export(Result<String, String>),
}

struct App {
    project: Arc<Mutex<Project>>,
    plan: Arc<Vec<ClipPlan>>,
    project_rev: u64,
    plan_rev: u64,

    selected_track: usize,
    selected_clip: Option<(usize, usize)>,
    selected_source: usize,
    selected_pattern: usize,

    playback: playback::Playback,
    loop_on: bool,
    loop_start: f64,
    loop_end: f64,
    playhead_beats: f64,

    gpu: Arc<Mutex<Option<gpu::GpuPreview>>>,
    gpu_failed: Arc<AtomicBool>,
    scene: Arc<Mutex<Option<gpu::Scene>>>,

    busy: Option<String>,
    rx: mpsc::Receiver<WorkerMsg>,
    tx: mpsc::Sender<WorkerMsg>,
    status: String,

    media_path: String,
    youtube_url: String,
    script_text: String,
    script_log: String,
    show_script: bool,
}

impl App {
    fn new() -> App {
        let (tx, rx) = mpsc::channel();
        let project = Project::demo();
        let plan = Arc::new(plan_project(&project));
        App {
            project: Arc::new(Mutex::new(project)),
            plan,
            project_rev: 1,
            plan_rev: 1,
            selected_track: 0,
            selected_clip: Some((0, 0)),
            selected_source: 0,
            selected_pattern: 0,
            playback: playback::Playback::new(),
            loop_on: false,
            loop_start: 0.0,
            loop_end: 8.0,
            playhead_beats: 0.0,
            gpu: Arc::new(Mutex::new(None)),
            gpu_failed: Arc::new(AtomicBool::new(false)),
            scene: Arc::new(Mutex::new(None)),
            busy: None,
            rx,
            tx,
            status: "demo project — press play".into(),
            media_path: String::new(),
            youtube_url: String::new(),
            script_text: DEFAULT_SCRIPT.trim_start().into(),
            script_log: String::new(),
            show_script: false,
        }
    }

    fn loop_region(&self) -> Option<(f64, f64)> {
        (self.loop_on && self.loop_end > self.loop_start)
            .then_some((self.loop_start, self.loop_end))
    }

    /// Call after any project edit: replans events + live-updates playback.
    fn sync_project(&mut self) {
        if self.project_rev == self.plan_rev {
            return;
        }
        self.plan_rev = self.project_rev;
        let p = self.project.lock().unwrap().clone();
        self.plan = Arc::new(plan_project(&p));
        if self.playback.is_playing() {
            self.playback.update_project(p);
        }
    }

    fn start_play(&mut self) {
        let p = self.project.lock().unwrap().clone();
        self.playback.play(p, self.playhead_beats, self.loop_region());
    }

    fn start_load(&mut self, path: String) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(format!("loading {path}…"));
        let sr = self.project.lock().unwrap().sample_rate;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = media::load_source(std::path::Path::new(&path), sr);
            let _ = tx.send(WorkerMsg::Source(res));
        });
    }

    fn start_youtube(&mut self, url: String) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("fetching from YouTube…".into());
        let sr = self.project.lock().unwrap().sample_rate;
        let tx = self.tx.clone();
        let cache = std::env::temp_dir().join("chromagrain-yt");
        std::thread::spawn(move || {
            let res = media::load_youtube_source(&url, &cache, sr);
            let _ = tx.send(WorkerMsg::Source(res));
        });
    }

    fn start_script(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("running script…".into());
        let project = self.project.clone();
        let script = self.script_text.clone();
        let tx = self.tx.clone();
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        std::thread::spawn(move || {
            let res = run_script(project, &base, &script);
            let _ = tx.send(WorkerMsg::Script(res));
        });
    }

    /// Render the arrangement to disk with the reference CPU renderer.
    fn start_render(&mut self, wav_only: bool) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("rendering…".into());
        let project = self.project.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let p = project.lock().unwrap().clone();
            let end = p.end_beat().max(1.0);
            let out = render_project(&p, 0.0, end);
            let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let res = if wav_only || !media::ffmpeg_available() {
                let path = dir.join("chromagrain-render.wav");
                media::save_wav(&path, &out.audio).map(|_| path.display().to_string())
            } else {
                let path = dir.join("chromagrain-render.mp4");
                media::encode_video(&path, &out.frames, out.fps, &out.audio)
                    .map(|_| path.display().to_string())
            };
            let _ = tx.send(WorkerMsg::Export(res));
        });
    }

    fn poll_workers(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = None;
            match msg {
                WorkerMsg::Source(Ok(src)) => {
                    let name = src.name.clone();
                    let hz = src.base_hz;
                    let id = self.project.lock().unwrap().add_source(src);
                    self.selected_source = id;
                    self.project_rev += 1;
                    self.status = if hz > 0.0 {
                        format!("loaded '{name}' (base pitch ≈ {hz:.1} Hz)")
                    } else {
                        format!("loaded '{name}'")
                    };
                }
                WorkerMsg::Source(Err(e)) => self.status = format!("load failed: {e}"),
                WorkerMsg::Script(Ok(log)) => {
                    self.script_log = if log.is_empty() { "(ok)".into() } else { log };
                    self.project_rev += 1;
                    self.status = "script ok".into();
                }
                WorkerMsg::Script(Err(e)) => {
                    self.script_log = format!("ERROR: {e}");
                    self.status = "script error".into();
                }
                WorkerMsg::Export(Ok(path)) => self.status = format!("rendered {path}"),
                WorkerMsg::Export(Err(e)) => self.status = format!("render failed: {e}"),
            }
        }
    }

    // ------------------------------------------------------------ panels

    fn transport_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("chromagrain")
                    .size(16.0)
                    .color(egui::Color32::from_rgb(255, 122, 89)),
            );
            ui.add_space(8.0);

            let playing = self.playback.is_playing();
            if ui
                .button(if playing { "■ stop" } else { "▶ play" })
                .on_hover_text("performance playback — live, no bounce")
                .clicked()
            {
                if playing {
                    self.playback.stop();
                } else {
                    self.start_play();
                }
            }
            let mut loop_on = self.loop_on;
            if ui.toggle_value(&mut loop_on, "loop").changed() {
                self.loop_on = loop_on;
                self.playback.set_loop(self.loop_region());
            }
            if self.loop_on {
                let mut changed = false;
                changed |= ui
                    .add(egui::DragValue::new(&mut self.loop_start).speed(1.0).range(0.0..=512.0))
                    .changed();
                ui.label("→");
                changed |= ui
                    .add(egui::DragValue::new(&mut self.loop_end).speed(1.0).range(0.0..=512.0))
                    .changed();
                if changed {
                    self.playback.set_loop(self.loop_region());
                }
            }
            ui.add_space(8.0);

            {
                let mut p = self.project.lock().unwrap();
                let mut bpm = p.bpm;
                if ui
                    .add(egui::DragValue::new(&mut bpm).range(20.0..=300.0).suffix(" bpm"))
                    .changed()
                {
                    p.bpm = bpm;
                    drop(p);
                    self.project_rev += 1;
                } else {
                    let mut q = p.quantize;
                    egui::ComboBox::from_id_salt("quantize")
                        .selected_text(quantize_label(q))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for div in [1.0, 0.5, 0.25, 0.125] {
                                if ui.selectable_value(&mut q, div, quantize_label(div)).changed() {
                                    p.quantize = q;
                                }
                            }
                        });
                }
            }
            ui.add_space(8.0);

            if ui.button("render mp4").clicked() {
                self.start_render(false);
            }
            if ui.button("render wav").clicked() {
                self.start_render(true);
            }

            if let Some(b) = &self.busy {
                ui.spinner();
                ui.label(b.clone());
            }
            if !self.playback.available() {
                ui.label(
                    egui::RichText::new("no audio device — render still works")
                        .color(egui::Color32::YELLOW),
                );
            }
        });
    }

    fn sources_ui(&mut self, ui: &mut egui::Ui) {
        section(ui, "sources");
        let names: Vec<(usize, String, f64)> = {
            let p = self.project.lock().unwrap();
            p.sources
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.name.clone(), s.duration()))
                .collect()
        };
        for (i, name, secs) in names {
            if ui
                .selectable_label(self.selected_source == i, format!("{name} ({secs:.1}s)"))
                .clicked()
            {
                self.selected_source = i;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("+ demo").clicked() {
                let mut p = self.project.lock().unwrap();
                let sr = p.sample_rate;
                let id = p.add_source(Source {
                    name: "demo saw+pattern".into(),
                    audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(
                        110.0, 4.0, sr,
                    ))),
                    video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(
                        240, 136, 12.0, 4.0,
                    ))),
                    base_hz: 110.0,
                });
                drop(p);
                self.selected_source = id;
                self.project_rev += 1;
            }
        });
        ui.text_edit_singleline(&mut self.media_path);
        if ui.button("load file").clicked() && !self.media_path.is_empty() {
            let p = self.media_path.clone();
            self.start_load(p);
        }
        ui.text_edit_singleline(&mut self.youtube_url);
        if ui.button("scrape youtube").clicked() && !self.youtube_url.is_empty() {
            let u = self.youtube_url.clone();
            self.start_youtube(u);
        }

        section(ui, "tracks");
        let track_info: Vec<(String, bool)> = {
            let p = self.project.lock().unwrap();
            p.tracks.iter().map(|t| (t.name.clone(), t.muted)).collect()
        };
        let n_tracks = track_info.len();
        for (i, (name, muted)) in track_info.iter().enumerate() {
            ui.horizontal(|ui| {
                let label = if *muted { format!("{name} (m)") } else { name.clone() };
                if ui.selectable_label(self.selected_track == i, label).clicked() {
                    self.selected_track = i;
                }
                // Z-order: later tracks composite on top.
                if ui.small_button("↑").clicked() && i > 0 {
                    self.project.lock().unwrap().tracks.swap(i, i - 1);
                    self.selected_track = i - 1;
                    self.project_rev += 1;
                }
                if ui.small_button("↓").clicked() && i + 1 < n_tracks {
                    self.project.lock().unwrap().tracks.swap(i, i + 1);
                    self.selected_track = i + 1;
                    self.project_rev += 1;
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ track").clicked() {
                let mut p = self.project.lock().unwrap();
                let n = p.tracks.len();
                p.add_track(&format!("track {}", n + 1));
                drop(p);
                self.project_rev += 1;
            }
        });
        ui.label(egui::RichText::new("order = video stacking (top row renders first)").weak().size(10.0));

        section(ui, "patterns");
        let pattern_names: Vec<String> = {
            let p = self.project.lock().unwrap();
            p.patterns.iter().map(|pat| pat.name.clone()).collect()
        };
        for (i, name) in pattern_names.iter().enumerate() {
            if ui
                .selectable_label(self.selected_pattern == i, name)
                .clicked()
            {
                self.selected_pattern = i;
            }
        }
        if ui.button("+ pattern").clicked() {
            let mut p = self.project.lock().unwrap();
            let n = p.patterns.len();
            let mut pat = StepPattern::new(&format!("pattern {}", n + 1));
            if self.selected_source < p.sources.len() {
                pat.add_row(self.selected_source);
            }
            self.selected_pattern = p.add_pattern(pat);
            drop(p);
            self.project_rev += 1;
        }

        section(ui, "add clip at playhead");
        ui.horizontal_wrapped(|ui| {
            if ui.button("grains").clicked() {
                self.add_clip(AddKind::Granular);
            }
            if ui.button("snippet").clicked() {
                self.add_clip(AddKind::Snippet);
            }
            if ui.button("pattern").clicked() {
                self.add_clip(AddKind::Pattern);
            }
        });
    }

    fn add_clip(&mut self, kind: AddKind) {
        let mut p = self.project.lock().unwrap();
        if p.tracks.is_empty() {
            return;
        }
        let t = self.selected_track.min(p.tracks.len() - 1);
        let start = self.playhead_beats;
        let clip = match kind {
            AddKind::Granular if self.selected_source < p.sources.len() => {
                Clip::new(self.selected_source, start, 4.0)
            }
            AddKind::Snippet if self.selected_source < p.sources.len() => {
                Clip::new_snippet(self.selected_source, start, 4.0)
            }
            AddKind::Pattern if self.selected_pattern < p.patterns.len() => {
                Clip::new_pattern(self.selected_pattern, start, 4.0)
            }
            _ => return,
        };
        p.tracks[t].clips.push(clip);
        let idx = p.tracks[t].clips.len() - 1;
        drop(p);
        self.selected_clip = Some((t, idx));
        self.project_rev += 1;
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width().min(760.0);
        let size = egui::vec2(avail, avail * 9.0 / 16.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

        if self.gpu_failed.load(Ordering::Relaxed) {
            ui.painter().rect_filled(rect, 4.0, egui::Color32::from_gray(14));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "GPU preview unavailable — render still works",
                egui::FontId::proportional(14.0),
                egui::Color32::GRAY,
            );
            return;
        }

        // Publish the scene for the paint callback.
        {
            let p = self.project.lock().unwrap();
            let t = p.beats_to_secs(self.playhead_beats);
            *self.scene.lock().unwrap() = Some(gpu::Scene {
                project: p.clone(),
                plan: self.plan.clone(),
                t,
            });
        }
        let gpu_slot = self.gpu.clone();
        let scene = self.scene.clone();
        let failed = self.gpu_failed.clone();
        let cb = eframe::egui_glow::CallbackFn::new(move |info, painter| {
            let mut slot = gpu_slot.lock().unwrap();
            if slot.is_none() && !failed.load(Ordering::Relaxed) {
                match gpu::GpuPreview::new(painter.gl()) {
                    Ok(g) => *slot = Some(g),
                    Err(e) => {
                        eprintln!("gpu preview init failed: {e}");
                        failed.store(true, Ordering::Relaxed);
                    }
                }
            }
            if let Some(g) = slot.as_mut() {
                if let Some(scene) = scene.lock().unwrap().as_ref() {
                    g.paint(painter.gl(), &info, scene);
                }
            }
        });
        ui.painter().add(egui::PaintCallback { rect, callback: Arc::new(cb) });
    }

    fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        let (n_tracks, end_beat, quantize) = {
            let p = self.project.lock().unwrap();
            (p.tracks.len(), p.end_beat().max(8.0), p.quantize.max(0.0625))
        };
        let beat_w = 42.0f32;
        let track_h = 40.0f32;
        let ruler_h = 16.0f32;
        let width = (end_beat as f32 + 4.0) * beat_w;
        let height = ruler_h + n_tracks.max(1) as f32 * track_h;

        egui::ScrollArea::horizontal().id_salt("timeline_scroll").show(ui, |ui| {
            let (resp, painter) = ui.allocate_painter(
                egui::vec2(width.max(ui.available_width()), height),
                egui::Sense::click_and_drag(),
            );
            let origin = resp.rect.min;
            painter.rect_filled(resp.rect, 0.0, egui::Color32::from_rgb(13, 13, 15));

            let total_beats = (width / beat_w) as usize;
            for b in 0..=total_beats {
                let x = origin.x + b as f32 * beat_w;
                let strong = b % 4 == 0;
                painter.line_segment(
                    [egui::pos2(x, origin.y), egui::pos2(x, origin.y + height)],
                    egui::Stroke::new(
                        1.0,
                        if strong {
                            egui::Color32::from_gray(58)
                        } else {
                            egui::Color32::from_gray(30)
                        },
                    ),
                );
                if strong {
                    painter.text(
                        egui::pos2(x + 3.0, origin.y + 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{b}"),
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_gray(120),
                    );
                }
            }

            #[allow(clippy::type_complexity)]
            let clip_data: Vec<(usize, usize, f64, f64, bool, usize, &'static str)> = {
                let p = self.project.lock().unwrap();
                p.tracks
                    .iter()
                    .enumerate()
                    .flat_map(|(ti, t)| {
                        t.clips.iter().enumerate().map(move |(ci, c)| {
                            let (label, hue_seed) = match c.kind {
                                ClipKind::Granular => ("grains", c.source),
                                ClipKind::Snippet => ("snippet", c.source),
                                ClipKind::Pattern(p) => ("pattern", p + 7),
                            };
                            (ti, ci, c.start_beat, c.length_beats, c.key.is_some(), hue_seed, label)
                        })
                    })
                    .collect()
            };
            for (ti, ci, start, len, has_key, hue_seed, label) in &clip_data {
                let x0 = origin.x + *start as f32 * beat_w;
                let y0 = origin.y + ruler_h + *ti as f32 * track_h + 3.0;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x0, y0),
                    egui::vec2(*len as f32 * beat_w, track_h - 6.0),
                );
                let selected = self.selected_clip == Some((*ti, *ci));
                let hue = (*hue_seed as f32 * 67.0 + 12.0) % 360.0;
                let (r, g, b) = chromagrain_core::video::hsv_to_rgb(
                    hue,
                    0.45,
                    if selected { 0.8 } else { 0.42 },
                );
                painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                if selected {
                    painter.rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                }
                let txt = if *has_key { format!("{label} ♪") } else { label.to_string() };
                painter.text(
                    rect.min + egui::vec2(5.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    txt,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(15),
                );
            }

            // Loop region shading.
            if self.loop_on {
                let lx0 = origin.x + self.loop_start as f32 * beat_w;
                let lx1 = origin.x + self.loop_end as f32 * beat_w;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(lx0, origin.y),
                        egui::pos2(lx1, origin.y + ruler_h),
                    ),
                    0.0,
                    egui::Color32::from_rgb(255, 122, 89).linear_multiply(0.25),
                );
            }

            // Playhead.
            let px = origin.x + self.playhead_beats as f32 * beat_w;
            painter.line_segment(
                [egui::pos2(px, origin.y), egui::pos2(px, origin.y + height)],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 122, 89)),
            );

            if let Some(pos) = resp.interact_pointer_pos() {
                let beat = ((pos.x - origin.x) / beat_w) as f64;
                let hit_clip = clip_data.iter().rev().find(|(ti, _, start, len, _, _, _)| {
                    let y0 = origin.y + ruler_h + *ti as f32 * track_h;
                    pos.y >= y0 && pos.y < y0 + track_h && beat >= *start && beat < start + len
                });
                if resp.drag_started() || resp.clicked() {
                    match hit_clip {
                        Some((ti, ci, ..)) => {
                            self.selected_clip = Some((*ti, *ci));
                            self.selected_track = *ti;
                        }
                        None => {
                            let q = quantize;
                            self.playhead_beats = ((beat / q).round() * q).max(0.0);
                            self.selected_clip = None;
                            if self.playback.is_playing() {
                                self.start_play();
                            }
                        }
                    }
                } else if resp.dragged() {
                    if let Some((ti, ci)) = self.selected_clip {
                        let delta = (resp.drag_delta().x / beat_w) as f64;
                        if delta != 0.0 {
                            let mut p = self.project.lock().unwrap();
                            if let Some(c) =
                                p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci))
                            {
                                c.start_beat = (c.start_beat + delta).max(0.0);
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    if let Some((ti, ci)) = self.selected_clip {
                        let mut p = self.project.lock().unwrap();
                        if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci))
                        {
                            let q = quantize;
                            c.start_beat = (c.start_beat / q).round() * q;
                        }
                        drop(p);
                        self.project_rev += 1;
                    }
                }
            }
        });
    }

    fn sequencer_ui(&mut self, ui: &mut egui::Ui) {
        // Edit the pattern of the selected pattern clip, or the selected pattern.
        let pid = match self.selected_clip.and_then(|(ti, ci)| {
            let p = self.project.lock().unwrap();
            p.tracks.get(ti).and_then(|t| t.clips.get(ci)).and_then(|c| match c.kind {
                ClipKind::Pattern(pid) => Some(pid),
                _ => None,
            })
        }) {
            Some(pid) => pid,
            None => self.selected_pattern,
        };

        let mut p = self.project.lock().unwrap();
        let n_sources = p.sources.len();
        let Some(pat) = p.patterns.get_mut(pid) else {
            drop(p);
            ui.label(egui::RichText::new("no pattern — create one in the left panel").weak());
            return;
        };
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&pat.name).strong());
            ui.add_space(8.0);
            let mut beats = pat.length_beats;
            let mut spb = pat.steps_per_beat as i64;
            let a = ui
                .add(egui::DragValue::new(&mut beats).speed(1.0).range(1.0..=16.0).suffix(" beats"))
                .changed();
            let b = ui
                .add(egui::DragValue::new(&mut spb).speed(1.0).range(1..=8).suffix(" /beat"))
                .changed();
            if a || b {
                pat.set_grid(beats, spb as u32);
                changed = true;
            }
        });

        let n = pat.n_steps();
        let spb = pat.steps_per_beat as usize;
        let mut remove_row: Option<usize> = None;
        for (ri, row) in pat.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.push_id(("seqrow", ri), |ui| {
                    if ui.small_button("x").clicked() {
                        remove_row = Some(ri);
                    }
                    ui.label(egui::RichText::new(format!("src")).weak().size(10.0));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut row.source)
                                .range(0..=n_sources.saturating_sub(1)),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut row.grains.pitch)
                                .speed(1.0)
                                .range(-24.0..=24.0)
                                .suffix(" st"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut row.grains.gain)
                                .speed(0.05)
                                .range(0.0..=2.0)
                                .suffix(" g"),
                        )
                        .changed();
                    // The step grid.
                    for si in 0..n {
                        if si % spb == 0 && si > 0 {
                            ui.add_space(3.0);
                        }
                        let on = row.steps.get(si).map(|s| s.on).unwrap_or(false);
                        let (rect, resp) = ui
                            .allocate_exact_size(egui::vec2(16.0, 22.0), egui::Sense::click());
                        let accent = egui::Color32::from_rgb(255, 122, 89);
                        let color = if on {
                            accent
                        } else if (si / spb) % 2 == 0 {
                            egui::Color32::from_gray(45)
                        } else {
                            egui::Color32::from_gray(36)
                        };
                        ui.painter().rect_filled(rect.shrink(1.0), 2.0, color);
                        if resp.clicked() {
                            if let Some(s) = row.steps.get_mut(si) {
                                s.on = !s.on;
                                changed = true;
                            }
                        }
                    }
                });
            });
        }
        if let Some(ri) = remove_row {
            pat.rows.remove(ri);
            changed = true;
        }
        let sel_src = self.selected_source.min(n_sources.saturating_sub(1));
        if ui.small_button("+ row (selected source)").clicked() && n_sources > 0 {
            pat.add_row(sel_src);
            changed = true;
        }
        drop(p);
        if changed {
            self.project_rev += 1;
        }
    }

    /// Editor for an AV effect chain. Returns true when anything changed.
    fn effects_chain_ui(ui: &mut egui::Ui, effects: &mut Vec<AvEffect>, salt: &str) -> bool {
        let mut changed = false;
        let mut remove: Option<usize> = None;
        for (i, fx) in effects.iter_mut().enumerate() {
            ui.push_id((salt, i), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(fx.kind.name()).strong());
                    if ui.small_button("x").clicked() {
                        remove = Some(i);
                    }
                });
                match &mut fx.kind {
                    EffectKind::Crush { downsample, bits } => {
                        changed |= ui
                            .add(egui::Slider::new(downsample, 1.0..=32.0).text("downres"))
                            .changed();
                        changed |=
                            ui.add(egui::Slider::new(bits, 1.0..=16.0).text("bits")).changed();
                    }
                    EffectKind::Delay { time, feedback, mix, shift_x, shift_y } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(time, 0.02..=2.0)
                                    .logarithmic(true)
                                    .text("time (s)"),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(feedback, 0.0..=0.95).text("feedback"))
                            .changed();
                        changed |=
                            ui.add(egui::Slider::new(mix, 0.0..=1.0).text("mix")).changed();
                        changed |= ui
                            .add(egui::Slider::new(shift_x, -0.3..=0.3).text("drift x"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(shift_y, -0.3..=0.3).text("drift y"))
                            .changed();
                    }
                    EffectKind::Reverb { size, damp, mix } => {
                        changed |=
                            ui.add(egui::Slider::new(size, 0.0..=1.0).text("size")).changed();
                        changed |= ui
                            .add(egui::Slider::new(damp, 0.0..=1.0).text("damp / blur"))
                            .changed();
                        changed |=
                            ui.add(egui::Slider::new(mix, 0.0..=1.0).text("mix")).changed();
                    }
                    EffectKind::Compress { quality } => {
                        changed |= ui
                            .add(egui::Slider::new(quality, 0.0..=1.0).text("quality"))
                            .changed();
                    }
                }
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(egui::Slider::new(&mut fx.audio, 0.0..=1.0).text("audio"))
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut fx.video, 0.0..=1.0).text("video"))
                        .changed();
                });
                ui.separator();
            });
        }
        if let Some(i) = remove {
            effects.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            for name in ["crush", "delay", "reverb", "compress"] {
                if ui.small_button(format!("+{name}")).clicked() {
                    effects.push(AvEffect::new(EffectKind::parse(name).unwrap()));
                    changed = true;
                }
            }
        });
        changed
    }

    fn track_ui(&mut self, ui: &mut egui::Ui) {
        let ti = self.selected_track;
        let mut p = self.project.lock().unwrap();
        let Some(track) = p.tracks.get_mut(ti) else { return };
        let mut changed = false;

        section(ui, &format!("track: {}", track.name));
        changed |= ui.checkbox(&mut track.muted, "mute").changed();
        changed |= ui
            .add(egui::Slider::new(&mut track.level, 0.0..=1.5).text("level (gain + opacity)"))
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut track.level_to_opacity, 0.0..=1.0)
                    .text("level -> opacity link"),
            )
            .changed();
        ui.label(egui::RichText::new("track effects").weak().size(10.0));
        changed |= Self::effects_chain_ui(ui, &mut track.effects, "track_fx");
        drop(p);
        if changed {
            self.project_rev += 1;
        }
    }

    fn master_ui(&mut self, ui: &mut egui::Ui) {
        section(ui, "master effects");
        let mut p = self.project.lock().unwrap();
        let mut effects = std::mem::take(&mut p.master_effects);
        drop(p);
        let changed = Self::effects_chain_ui(ui, &mut effects, "master_fx");
        self.project.lock().unwrap().master_effects = effects;
        if changed {
            self.project_rev += 1;
        }
    }

    fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        let Some((ti, ci)) = self.selected_clip else {
            ui.label(egui::RichText::new("select a clip on the timeline").weak());
            return;
        };
        let mut p = self.project.lock().unwrap();
        let n_sources = p.sources.len();
        let n_patterns = p.patterns.len();
        let Some(clip) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) else {
            drop(p);
            self.selected_clip = None;
            return;
        };
        let mut changed = false;

        let kind_name = match clip.kind {
            ClipKind::Granular => "grain clip",
            ClipKind::Snippet => "snippet",
            ClipKind::Pattern(_) => "pattern clip",
        };
        section(ui, kind_name);
        ui.horizontal(|ui| {
            match &mut clip.kind {
                ClipKind::Pattern(pid) => {
                    ui.label("pattern");
                    changed |= ui
                        .add(egui::DragValue::new(pid).range(0..=n_patterns.saturating_sub(1)))
                        .changed();
                }
                _ => {
                    ui.label("source");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut clip.source)
                                .range(0..=n_sources.saturating_sub(1)),
                        )
                        .changed();
                }
            }
            ui.label("start");
            changed |= ui
                .add(egui::DragValue::new(&mut clip.start_beat).speed(0.25))
                .changed();
            ui.label("len");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.length_beats)
                        .speed(0.25)
                        .range(0.25..=256.0),
                )
                .changed();
        });
        let is_pattern = matches!(clip.kind, ClipKind::Pattern(_));

        if !is_pattern {
            section(ui, "grains (audio <-> visual)");
            let g = &mut clip.grains;
            let slider = |ui: &mut egui::Ui,
                          v: &mut f32,
                          range: std::ops::RangeInclusive<f32>,
                          label: &str,
                          log: bool|
             -> bool {
                ui.add(egui::Slider::new(v, range).logarithmic(log).text(label)).changed()
            };
            changed |= slider(ui, &mut g.density, 0.5..=200.0, "density (grains/s)", true);
            changed |= slider(ui, &mut g.duration, 0.005..=1.5, "duration (s)", true);
            changed |= slider(ui, &mut g.duration_jitter, 0.0..=1.0, "duration jitter", false);
            changed |= slider(ui, &mut g.position, 0.0..=1.0, "position", false);
            changed |= slider(ui, &mut g.spray, 0.0..=3.0, "spray (s)", false);
            changed |= slider(ui, &mut g.scan_speed, -2.0..=4.0, "scan speed", false);
            changed |= slider(ui, &mut g.pitch, -24.0..=24.0, "pitch (semitones)", false);
            changed |= slider(ui, &mut g.pitch_jitter, 0.0..=24.0, "pitch jitter", false);
            changed |= slider(ui, &mut g.gain, 0.0..=2.0, "gain", false);
            changed |= slider(ui, &mut g.pan, -1.0..=1.0, "pan / x position", false);
            changed |= slider(ui, &mut g.pan_spread, 0.0..=1.0, "pan/x spread", false);
            changed |= slider(ui, &mut g.envelope, 0.01..=1.0, "envelope shape", false);
            changed |= slider(ui, &mut g.reverse_prob, 0.0..=1.0, "reverse prob", false);
        }

        section(ui, "key quantize");
        let mut has_key = clip.key.is_some();
        if ui.checkbox(&mut has_key, "quantize pitches to key").changed() {
            clip.key = has_key.then_some(Key::new(9, ScaleKind::MinorPentatonic));
            changed = true;
        }
        if let Some(key) = &mut clip.key {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("key_root")
                    .selected_text(PITCH_CLASS_NAMES[key.root.rem_euclid(12) as usize])
                    .show_ui(ui, |ui| {
                        for (i, name) in PITCH_CLASS_NAMES.iter().enumerate() {
                            changed |=
                                ui.selectable_value(&mut key.root, i as i32, *name).changed();
                        }
                    });
                egui::ComboBox::from_id_salt("key_scale")
                    .selected_text(format!("{:?}", key.scale))
                    .show_ui(ui, |ui| {
                        for s in [
                            ScaleKind::Chromatic,
                            ScaleKind::Major,
                            ScaleKind::Minor,
                            ScaleKind::HarmonicMinor,
                            ScaleKind::MajorPentatonic,
                            ScaleKind::MinorPentatonic,
                            ScaleKind::Blues,
                            ScaleKind::Dorian,
                            ScaleKind::Phrygian,
                            ScaleKind::Lydian,
                            ScaleKind::Mixolydian,
                            ScaleKind::WholeTone,
                        ] {
                            changed |= ui
                                .selectable_value(&mut key.scale, s, format!("{s:?}"))
                                .changed();
                        }
                    });
            });
        }

        section(ui, "audio frequency filters");
        let mut remove: Option<usize> = None;
        for (i, f) in clip.audio_filters.iter_mut().enumerate() {
            ui.push_id(("audio_filter_row", i), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("fkind", i))
                        .selected_text(format!("{:?}", f.kind))
                        .show_ui(ui, |ui| {
                            for k in [
                                FilterKind::LowPass,
                                FilterKind::HighPass,
                                FilterKind::BandPass,
                                FilterKind::Notch,
                            ] {
                                changed |= ui
                                    .selectable_value(&mut f.kind, k, format!("{k:?}"))
                                    .changed();
                            }
                        });
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut f.freq, 30.0..=16000.0)
                                .logarithmic(true)
                                .text("Hz"),
                        )
                        .changed();
                    changed |=
                        ui.add(egui::Slider::new(&mut f.q, 0.1..=8.0).text("Q")).changed();
                    if ui.small_button("x").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            clip.audio_filters.remove(i);
            changed = true;
        }
        if ui.small_button("+ filter").clicked() {
            clip.audio_filters.push(FilterSpec {
                kind: FilterKind::LowPass,
                freq: 2000.0,
                q: 0.707,
            });
            changed = true;
        }

        section(ui, "color filter (hue band)");
        let mut mode = match clip.color_filter {
            None => 0,
            Some(ColorFilter { mode: ColorFilterMode::Keep, .. }) => 1,
            Some(ColorFilter { mode: ColorFilterMode::Remove, .. }) => 2,
        };
        ui.horizontal(|ui| {
            for (v, label) in [(0, "off"), (1, "keep"), (2, "remove")] {
                if ui.selectable_value(&mut mode, v, label).changed() {
                    clip.color_filter = match v {
                        1 => Some(ColorFilter::keep(120.0, 90.0)),
                        2 => Some(ColorFilter::remove(120.0, 90.0)),
                        _ => None,
                    };
                    changed = true;
                }
            }
        });
        if let Some(cf) = &mut clip.color_filter {
            changed |= ui
                .add(egui::Slider::new(&mut cf.hue_center, 0.0..=360.0).text("hue °"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut cf.hue_width, 5.0..=360.0).text("width °"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut cf.softness, 0.0..=1.0).text("softness"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut cf.sat_min, 0.0..=1.0).text("min sat"))
                .changed();
        }

        section(ui, "effects chain (audio + video)");
        changed |= Self::effects_chain_ui(ui, &mut clip.effects, "clip_fx");

        section(ui, "correspondence (AV link)");
        let l = &mut clip.link;
        changed |= ui
            .add(egui::Slider::new(&mut l.gain_to_opacity, 0.0..=1.0).text("gain -> opacity"))
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut l.envelope_to_opacity, 0.0..=1.0)
                    .text("envelope -> opacity"),
            )
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut l.pitch_to_hue, -30.0..=30.0).text("pitch -> hue °/st"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut l.pitch_to_rate, 0.0..=1.0).text("pitch -> video rate"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut l.pan_to_x, 0.0..=1.0).text("pan -> x position"))
            .changed();
        changed |= ui.checkbox(&mut l.reverse_video, "reverse video with audio").changed();

        section(ui, "visual grain style");
        let v = &mut clip.visual;
        changed |= ui
            .add(egui::Slider::new(&mut v.size_scale, 0.2..=8.0).text("size per duration"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut v.additive, 0.0..=1.0).text("additive glow"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut v.scatter_y, 0.0..=1.0).text("y scatter"))
            .changed();

        ui.add_space(6.0);
        if ui.push_id("delete_clip", |ui| ui.button("delete clip")).inner.clicked() {
            p.tracks[ti].clips.remove(ci);
            drop(p);
            self.selected_clip = None;
            self.project_rev += 1;
            return;
        }
        drop(p);
        if changed {
            self.project_rev += 1;
        }
    }

    fn script_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.show_script, "script console");
            if self.show_script && ui.button("▶ run").clicked() {
                self.start_script();
            }
        });
        if !self.show_script {
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("script_scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.script_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(8),
                );
            });
        if !self.script_log.is_empty() {
            ui.label(egui::RichText::new(&self.script_log).monospace().weak());
        }
    }
}

enum AddKind {
    Granular,
    Snippet,
    Pattern,
}

fn quantize_label(q: f64) -> &'static str {
    if q >= 1.0 {
        "1/4"
    } else if q >= 0.5 {
        "1/8"
    } else if q >= 0.25 {
        "1/16"
    } else {
        "1/32"
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_workers();
        self.sync_project();
        if self.playback.is_playing() {
            let p = self.project.lock().unwrap();
            self.playhead_beats = p.secs_to_beats(self.playback.playhead_secs(&p));
            drop(p);
            ctx.request_repaint();
        } else if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("transport").show(ctx, |ui| {
            self.transport_ui(ui);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(egui::RichText::new(&self.status).weak());
        });
        egui::SidePanel::left("sources").default_width(220.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("left_scroll").show(ui, |ui| {
                self.sources_ui(ui)
            });
        });
        egui::SidePanel::right("inspector").default_width(330.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("right_scroll").show(ui, |ui| {
                self.inspector_ui(ui);
                self.track_ui(ui);
                self.master_ui(ui);
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.preview_ui(ui);
            ui.add_space(4.0);
            self.timeline_ui(ui);
            ui.add_space(4.0);
            section(ui, "sequencer");
            self.sequencer_ui(ui);
            ui.add_space(4.0);
            self.script_ui(ui);
        });
    }
}

const DEFAULT_SCRIPT: &str = r#"
// chromagrain scripting — the whole engine is drivable from here.
let s = session(100.0);
let src = s.demo_source();          // or s.load("clip.mp4") / s.youtube("https://…")
let t = s.track("scripted");
let c = s.clip(t, src, 8.0, 8.0);   // or s.snippet(...) / s.pattern_clip(...)
s.key(c, "A", "minor_pentatonic");
s.set(c, "density", 35.0);
s.set(c, "pitch_jitter", 12.0);
let d = s.effect(c, "delay");       // AV effects: crush/delay/reverb/compress
s.fx(c, d, "time", 0.3);
let p = s.pattern("hits");          // step sequencer
let r = s.row(p, src);
s.step(p, r, 0, true); s.step(p, r, 6, true); s.step(p, r, 10, true);
s.pattern_clip(t, p, 16.0, 8.0);
print("scripted content added");
"#;
