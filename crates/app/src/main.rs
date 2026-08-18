//! chromagrain — audiovisual granular sampling compositor / sequencer.
//!
//! GUI:       chromagrain
//! Headless:  chromagrain --script composition.rhai [base_dir]
//!            chromagrain --render-demo out.mp4

mod playback;

use chromagrain_core::dsp::{FilterKind, FilterSpec};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::media;
use chromagrain_core::music::{Key, ScaleKind, PITCH_CLASS_NAMES};
use chromagrain_core::render::{render_project, RenderOutput};
use chromagrain_core::script::run_script;
use chromagrain_core::timeline::{Clip, ClipKind, Project, Source};
use chromagrain_core::video::{ColorFilter, ColorFilterMode};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

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
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "chromagrain",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

enum WorkerMsg {
    Bounce(Result<RenderOutput, String>),
    Source(Result<Source, String>),
    Script(Result<String, String>),
    Export(Result<String, String>),
}

struct App {
    project: Arc<Mutex<Project>>,
    selected_track: usize,
    selected_clip: Option<(usize, usize)>,
    selected_source: usize,

    playback: playback::Playback,
    bounce: Option<Arc<RenderOutput>>,
    bounce_dirty: bool,
    pending_play: bool,
    busy: Option<String>,
    rx: mpsc::Receiver<WorkerMsg>,
    tx: mpsc::Sender<WorkerMsg>,

    preview_tex: Option<egui::TextureHandle>,
    preview_idx: usize,

    playhead_beats: f64,
    status: String,

    media_path: String,
    youtube_url: String,
    script_text: String,
    script_log: String,
}

impl App {
    fn new() -> App {
        let (tx, rx) = mpsc::channel();
        App {
            project: Arc::new(Mutex::new(Project::demo())),
            selected_track: 0,
            selected_clip: Some((0, 0)),
            selected_source: 0,
            playback: playback::Playback::new(),
            bounce: None,
            bounce_dirty: true,
            pending_play: false,
            busy: None,
            rx,
            tx,
            preview_tex: None,
            preview_idx: usize::MAX,
            playhead_beats: 0.0,
            status: "demo project loaded — press Play".into(),
            media_path: String::new(),
            youtube_url: String::new(),
            script_text: DEFAULT_SCRIPT.trim_start().into(),
            script_log: String::new(),
        }
    }

    fn start_bounce(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("bouncing…".into());
        let project = self.project.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let p = project.lock().unwrap().clone();
            let end = p.end_beat().max(1.0);
            let out = render_project(&p, 0.0, end);
            let _ = tx.send(WorkerMsg::Bounce(Ok(out)));
        });
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

    fn start_export(&mut self, wav_only: bool) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("exporting…".into());
        let project = self.project.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let p = project.lock().unwrap().clone();
            let end = p.end_beat().max(1.0);
            let out = render_project(&p, 0.0, end);
            let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let res = if wav_only || !media::ffmpeg_available() {
                let path = dir.join("chromagrain-export.wav");
                media::save_wav(&path, &out.audio).map(|_| path.display().to_string())
            } else {
                let path = dir.join("chromagrain-export.mp4");
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
                WorkerMsg::Bounce(Ok(out)) => {
                    self.playback.set_buffer(&out.audio);
                    self.bounce = Some(Arc::new(out));
                    self.bounce_dirty = false;
                    self.preview_idx = usize::MAX;
                    self.status = "bounce ready".into();
                    if self.pending_play {
                        self.pending_play = false;
                        let p = self.project.lock().unwrap();
                        let secs = p.beats_to_secs(self.playhead_beats);
                        drop(p);
                        self.playback.play_from(secs);
                    }
                }
                WorkerMsg::Bounce(Err(e)) => self.status = format!("bounce failed: {e}"),
                WorkerMsg::Source(Ok(src)) => {
                    let name = src.name.clone();
                    let hz = src.base_hz;
                    let mut p = self.project.lock().unwrap();
                    let id = p.add_source(src);
                    drop(p);
                    self.selected_source = id;
                    self.bounce_dirty = true;
                    self.status = if hz > 0.0 {
                        format!("loaded '{name}' (base pitch ≈ {hz:.1} Hz)")
                    } else {
                        format!("loaded '{name}'")
                    };
                }
                WorkerMsg::Source(Err(e)) => self.status = format!("load failed: {e}"),
                WorkerMsg::Script(Ok(log)) => {
                    self.script_log = if log.is_empty() { "(ok)".into() } else { log };
                    self.bounce_dirty = true;
                    self.status = "script ok".into();
                }
                WorkerMsg::Script(Err(e)) => {
                    self.script_log = format!("ERROR: {e}");
                    self.status = "script error".into();
                }
                WorkerMsg::Export(Ok(path)) => self.status = format!("exported {path}"),
                WorkerMsg::Export(Err(e)) => self.status = format!("export failed: {e}"),
            }
        }
    }

    fn transport_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("chromagrain");
            ui.separator();

            let playing = self.playback.is_playing();
            let play_label = if playing { "⏸ stop" } else { "▶ play" };
            if ui.button(play_label).clicked() {
                if playing {
                    self.playback.stop();
                } else if self.bounce.is_some() && !self.bounce_dirty {
                    let secs = {
                        let p = self.project.lock().unwrap();
                        p.beats_to_secs(self.playhead_beats)
                    };
                    self.playback.play_from(secs);
                } else {
                    self.pending_play = true;
                    self.start_bounce();
                }
            }
            if ui.button("⟳ bounce").clicked() {
                self.start_bounce();
            }
            if ui.button("export mp4").clicked() {
                self.start_export(false);
            }
            if ui.button("export wav").clicked() {
                self.start_export(true);
            }
            ui.separator();

            let mut bpm = self.project.lock().unwrap().bpm;
            if ui
                .add(egui::DragValue::new(&mut bpm).range(20.0..=300.0).suffix(" bpm"))
                .changed()
            {
                self.project.lock().unwrap().bpm = bpm;
                self.bounce_dirty = true;
            }

            if let Some(b) = &self.busy {
                ui.separator();
                ui.spinner();
                ui.label(b.clone());
            }
            if self.bounce_dirty && self.bounce.is_some() {
                ui.label(egui::RichText::new("(edited — re-bounce)").weak());
            }
            if !self.playback.available() {
                ui.label(
                    egui::RichText::new("no audio device — export still works")
                        .color(egui::Color32::YELLOW),
                );
            }
        });
    }

    fn sources_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("sources");
        let names: Vec<(usize, String, f64)> = {
            let p = self.project.lock().unwrap();
            p.sources
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.name.clone(), s.duration()))
                .collect()
        };
        for (i, name, secs) in names {
            let sel = self.selected_source == i;
            if ui
                .selectable_label(sel, format!("{name} ({secs:.1}s)"))
                .clicked()
            {
                self.selected_source = i;
            }
        }
        ui.separator();
        ui.label("load media file:");
        ui.text_edit_singleline(&mut self.media_path);
        if ui.button("load file").clicked() && !self.media_path.is_empty() {
            let p = self.media_path.clone();
            self.start_load(p);
        }
        ui.separator();
        ui.label("scrape youtube:");
        ui.text_edit_singleline(&mut self.youtube_url);
        if ui.button("fetch url").clicked() && !self.youtube_url.is_empty() {
            let u = self.youtube_url.clone();
            self.start_youtube(u);
        }
        ui.separator();
        if ui.button("+ demo source").clicked() {
            let mut p = self.project.lock().unwrap();
            let sr = p.sample_rate;
            let id = p.add_source(Source {
                name: "demo saw+pattern".into(),
                audio: Some(chromagrain_core::audio::AudioClip::saw_stack(110.0, 4.0, sr)),
                video: Some(chromagrain_core::video::VideoClip::test_pattern(
                    240, 136, 12.0, 4.0,
                )),
                base_hz: 110.0,
            });
            drop(p);
            self.selected_source = id;
            self.bounce_dirty = true;
        }
        ui.separator();
        ui.heading("tracks");
        let track_names: Vec<String> = {
            let p = self.project.lock().unwrap();
            p.tracks.iter().map(|t| t.name.clone()).collect()
        };
        for (i, name) in track_names.iter().enumerate() {
            if ui
                .selectable_label(self.selected_track == i, name)
                .clicked()
            {
                self.selected_track = i;
            }
        }
        if ui.button("+ track").clicked() {
            let mut p = self.project.lock().unwrap();
            let n = p.tracks.len();
            p.add_track(&format!("track {}", n + 1));
        }
        ui.horizontal(|ui| {
            if ui.button("+ grain clip").clicked() {
                self.add_clip(false);
            }
            if ui.button("+ snippet").clicked() {
                self.add_clip(true);
            }
        });
        ui.label(egui::RichText::new("clips land at the playhead").weak());
    }

    fn add_clip(&mut self, snippet: bool) {
        let mut p = self.project.lock().unwrap();
        if self.selected_source < p.sources.len() && !p.tracks.is_empty() {
            let t = self.selected_track.min(p.tracks.len() - 1);
            let clip = if snippet {
                Clip::new_snippet(self.selected_source, self.playhead_beats, 4.0)
            } else {
                Clip::new(self.selected_source, self.playhead_beats, 4.0)
            };
            p.tracks[t].clips.push(clip);
            let idx = p.tracks[t].clips.len() - 1;
            drop(p);
            self.selected_clip = Some((t, idx));
            self.bounce_dirty = true;
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let (frame_idx, total) = {
            let p = self.project.lock().unwrap();
            let secs = if self.playback.is_playing() {
                self.playhead_beats = p.secs_to_beats(self.playback.position_secs());
                self.playback.position_secs()
            } else {
                p.beats_to_secs(self.playhead_beats)
            };
            let fps = self.bounce.as_ref().map(|b| b.fps).unwrap_or(p.fps);
            (
                (secs * fps as f64) as usize,
                self.bounce.as_ref().map(|b| b.frames.len()).unwrap_or(0),
            )
        };

        if let Some(bounce) = &self.bounce {
            if total > 0 {
                let idx = frame_idx.min(total - 1);
                if idx != self.preview_idx {
                    let f = &bounce.frames[idx];
                    let img = egui::ColorImage::from_rgba_unmultiplied(
                        [f.width as usize, f.height as usize],
                        &f.data,
                    );
                    match &mut self.preview_tex {
                        Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                        None => {
                            self.preview_tex = Some(ui.ctx().load_texture(
                                "preview",
                                img,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                    }
                    self.preview_idx = idx;
                }
            }
        }
        let avail = ui.available_width().min(720.0);
        let size = egui::vec2(avail, avail * 9.0 / 16.0);
        match &self.preview_tex {
            Some(tex) => {
                ui.image((tex.id(), size));
            }
            None => {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4.0, egui::Color32::from_gray(18));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "bounce to preview",
                    egui::FontId::proportional(16.0),
                    egui::Color32::GRAY,
                );
            }
        }
    }

    fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        let (n_tracks, end_beat, bpm) = {
            let p = self.project.lock().unwrap();
            (p.tracks.len(), p.end_beat().max(8.0), p.bpm)
        };
        let beat_w = 42.0f32;
        let track_h = 46.0f32;
        let ruler_h = 18.0f32;
        let width = (end_beat as f32 + 4.0) * beat_w;
        let height = ruler_h + n_tracks.max(1) as f32 * track_h;

        egui::ScrollArea::horizontal().id_salt("timeline_scroll").show(ui, |ui| {
            let (resp, painter) = ui.allocate_painter(
                egui::vec2(width.max(ui.available_width()), height),
                egui::Sense::click_and_drag(),
            );
            let origin = resp.rect.min;
            painter.rect_filled(resp.rect, 0.0, egui::Color32::from_gray(24));

            // Beat grid.
            let total_beats = (width / beat_w) as usize;
            for b in 0..=total_beats {
                let x = origin.x + b as f32 * beat_w;
                let strong = b % 4 == 0;
                let color = if strong {
                    egui::Color32::from_gray(70)
                } else {
                    egui::Color32::from_gray(40)
                };
                painter.line_segment(
                    [
                        egui::pos2(x, origin.y),
                        egui::pos2(x, origin.y + height),
                    ],
                    egui::Stroke::new(1.0, color),
                );
                if strong {
                    painter.text(
                        egui::pos2(x + 3.0, origin.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{b}"),
                        egui::FontId::monospace(10.0),
                        egui::Color32::GRAY,
                    );
                }
            }

            // Clips.
            #[allow(clippy::type_complexity)]
            let clip_data: Vec<(usize, usize, f64, f64, bool, usize, bool)> = {
                let p = self.project.lock().unwrap();
                p.tracks
                    .iter()
                    .enumerate()
                    .flat_map(|(ti, t)| {
                        t.clips.iter().enumerate().map(move |(ci, c)| {
                            (
                                ti,
                                ci,
                                c.start_beat,
                                c.length_beats,
                                c.key.is_some(),
                                c.source,
                                c.kind == ClipKind::Snippet,
                            )
                        })
                    })
                    .collect()
            };
            for (ti, ci, start, len, has_key, source, is_snippet) in &clip_data {
                let x0 = origin.x + *start as f32 * beat_w;
                let y0 = origin.y + ruler_h + *ti as f32 * track_h + 3.0;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x0, y0),
                    egui::vec2(*len as f32 * beat_w, track_h - 6.0),
                );
                let selected = self.selected_clip == Some((*ti, *ci));
                let hue = (*source as f32 * 67.0) % 360.0;
                let (r, g, b) = chromagrain_core::video::hsv_to_rgb(
                    hue,
                    0.55,
                    if selected { 0.85 } else { 0.55 },
                );
                painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(r, g, b));
                if selected {
                    painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                }
                let base = if *is_snippet { "snippet" } else { "grains" };
                let label = if *has_key { format!("{base} ♪") } else { base.to_string() };
                painter.text(
                    rect.min + egui::vec2(5.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(12.0),
                    egui::Color32::BLACK,
                );
            }

            // Playhead.
            let secs_per_beat = 60.0 / bpm;
            let _ = secs_per_beat;
            let px = origin.x + self.playhead_beats as f32 * beat_w;
            painter.line_segment(
                [egui::pos2(px, origin.y), egui::pos2(px, origin.y + height)],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 70, 70)),
            );

            // Interaction: click ruler to seek, click clip to select,
            // drag clip to move.
            if let Some(pos) = resp.interact_pointer_pos() {
                let beat = ((pos.x - origin.x) / beat_w) as f64;
                let hit_clip = clip_data.iter().rev().find(|(ti, _, start, len, _, _, _)| {
                    let y0 = origin.y + ruler_h + *ti as f32 * track_h;
                    pos.y >= y0
                        && pos.y < y0 + track_h
                        && beat >= *start
                        && beat < start + len
                });
                if resp.drag_started() || resp.clicked() {
                    match hit_clip {
                        Some((ti, ci, _, _, _, _, _)) => {
                            self.selected_clip = Some((*ti, *ci));
                            self.selected_track = *ti;
                        }
                        None => {
                            self.playhead_beats =
                                (beat * 4.0).round().max(0.0) / 4.0;
                            self.selected_clip = None;
                        }
                    }
                } else if resp.dragged() {
                    if let Some((ti, ci)) = self.selected_clip {
                        let delta_beats = (resp.drag_delta().x / beat_w) as f64;
                        if delta_beats != 0.0 {
                            let mut p = self.project.lock().unwrap();
                            if let Some(c) = p
                                .tracks
                                .get_mut(ti)
                                .and_then(|t| t.clips.get_mut(ci))
                            {
                                c.start_beat = (c.start_beat + delta_beats).max(0.0);
                                self.bounce_dirty = true;
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    // Snap moved clip to a 16th grid.
                    if let Some((ti, ci)) = self.selected_clip {
                        let mut p = self.project.lock().unwrap();
                        if let Some(c) =
                            p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci))
                        {
                            c.start_beat = (c.start_beat * 4.0).round() / 4.0;
                        }
                    }
                }
            }
        });
    }

    /// Editor for an AV effect chain. Returns true when anything changed.
    fn effects_chain_ui(ui: &mut egui::Ui, effects: &mut Vec<AvEffect>, salt: &str) -> bool {
        let mut changed = false;
        let mut remove: Option<usize> = None;
        for (i, fx) in effects.iter_mut().enumerate() {
            ui.push_id((salt, i), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(fx.kind.name()).strong());
                    if ui.button("x").clicked() {
                        remove = Some(i);
                    }
                });
                match &mut fx.kind {
                    EffectKind::Crush { downsample, bits } => {
                        changed |= ui
                            .add(egui::Slider::new(downsample, 1.0..=32.0).text("downres"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(bits, 1.0..=16.0).text("bits"))
                            .changed();
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
                            .add(egui::Slider::new(shift_x, -0.3..=0.3).text("ghost drift x"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(shift_y, -0.3..=0.3).text("ghost drift y"))
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
                if ui.button(format!("+ {name}")).clicked() {
                    effects.push(AvEffect::new(EffectKind::parse(name).unwrap()));
                    changed = true;
                }
            }
        });
        changed
    }

    fn master_fx_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("master effects");
        let mut p = self.project.lock().unwrap();
        let mut effects = std::mem::take(&mut p.master_effects);
        drop(p);
        let changed = Self::effects_chain_ui(ui, &mut effects, "master_fx");
        self.project.lock().unwrap().master_effects = effects;
        if changed {
            self.bounce_dirty = true;
        }
    }

    fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        let Some((ti, ci)) = self.selected_clip else {
            ui.label("select a clip on the timeline");
            return;
        };
        let mut p = self.project.lock().unwrap();
        let n_sources = p.sources.len();
        let Some(clip) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) else {
            drop(p);
            self.selected_clip = None;
            return;
        };
        let mut changed = false;

        let kind_name = match clip.kind {
            ClipKind::Granular => "grain clip",
            ClipKind::Snippet => "snippet",
        };
        ui.heading(kind_name);
        ui.horizontal(|ui| {
            for (k, label) in [(ClipKind::Granular, "granular"), (ClipKind::Snippet, "snippet")]
            {
                changed |= ui.selectable_value(&mut clip.kind, k, label).changed();
            }
        });
        ui.horizontal(|ui| {
            ui.label("source");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.source)
                        .range(0..=n_sources.saturating_sub(1)),
                )
                .changed();
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

        ui.separator();
        ui.heading("grains  (audio <-> visual)");
        let g = &mut clip.grains;
        let slider = |ui: &mut egui::Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str, log: bool| -> bool {
            ui.add(egui::Slider::new(v, range).logarithmic(log).text(label))
                .changed()
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

        ui.separator();
        ui.heading("key quantize");
        let mut has_key = clip.key.is_some();
        if ui.checkbox(&mut has_key, "quantize grain pitches to key").changed() {
            clip.key = if has_key {
                Some(Key::new(9, ScaleKind::MinorPentatonic))
            } else {
                None
            };
            changed = true;
        }
        if let Some(key) = &mut clip.key {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("key_root")
                    .selected_text(PITCH_CLASS_NAMES[key.root.rem_euclid(12) as usize])
                    .show_ui(ui, |ui| {
                        for (i, name) in PITCH_CLASS_NAMES.iter().enumerate() {
                            changed |= ui
                                .selectable_value(&mut key.root, i as i32, *name)
                                .changed();
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

        ui.separator();
        ui.heading("audio frequency filters");
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
                changed |= ui
                    .add(egui::Slider::new(&mut f.q, 0.1..=8.0).text("Q"))
                    .changed();
                if ui.button("x").clicked() {
                    remove = Some(i);
                }
            });
            });
        }
        if let Some(i) = remove {
            clip.audio_filters.remove(i);
            changed = true;
        }
        if ui.button("+ filter").clicked() {
            clip.audio_filters.push(FilterSpec {
                kind: FilterKind::LowPass,
                freq: 2000.0,
                q: 0.707,
            });
            changed = true;
        }

        ui.separator();
        ui.heading("color filter (hue band)");
        let mut mode = match clip.color_filter {
            None => 0,
            Some(ColorFilter { mode: ColorFilterMode::Keep, .. }) => 1,
            Some(ColorFilter { mode: ColorFilterMode::Remove, .. }) => 2,
        };
        ui.horizontal(|ui| {
            for (v, label) in [(0, "off"), (1, "keep band"), (2, "remove band")] {
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

        ui.separator();
        ui.heading("effects chain  (audio + video)");
        changed |= Self::effects_chain_ui(ui, &mut clip.effects, "clip_fx");

        ui.separator();
        ui.heading("correspondence (AV link)");
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
            .add(
                egui::Slider::new(&mut l.pitch_to_hue, -30.0..=30.0)
                    .text("pitch -> hue (deg/st)"),
            )
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut l.pitch_to_rate, 0.0..=1.0).text("pitch -> video rate"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut l.pan_to_x, 0.0..=1.0).text("pan -> x position"))
            .changed();
        changed |= ui
            .checkbox(&mut l.reverse_video, "reversed audio reverses video")
            .changed();

        ui.separator();
        ui.heading("visual grain style");
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

        ui.separator();
        if ui.push_id("delete_clip", |ui| ui.button("delete clip")).inner.clicked() {
            p.tracks[ti].clips.remove(ci);
            drop(p);
            self.selected_clip = None;
            self.bounce_dirty = true;
            return;
        }

        if changed {
            self.bounce_dirty = true;
        }
    }

    fn script_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("script console (rhai)");
            if ui.button("▶ run").clicked() {
                self.start_script();
            }
        });
        egui::ScrollArea::vertical().id_salt("script_scroll").max_height(160.0).show(ui, |ui| {
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_workers();
        if self.playback.is_playing() || self.busy.is_some() {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("transport").show(ctx, |ui| {
            self.transport_ui(ui);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });
        egui::SidePanel::left("sources")
            .default_width(230.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.sources_ui(ui));
            });
        egui::SidePanel::right("inspector")
            .default_width(340.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.inspector_ui(ui);
                    ui.separator();
                    self.master_fx_ui(ui);
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.preview_ui(ui);
            ui.separator();
            self.timeline_ui(ui);
            ui.separator();
            self.script_ui(ui);
        });
    }
}

const DEFAULT_SCRIPT: &str = r#"
// chromagrain scripting — the whole engine is drivable from here.
let s = session(100.0);
let src = s.demo_source();          // or: s.load("clip.mp4") / s.youtube("https://…")
let t = s.track("scripted");
let c = s.clip(t, src, 8.0, 8.0);   // or s.snippet(t, src, 8.0, 8.0)
s.key(c, "A", "minor_pentatonic");
s.set(c, "density", 35.0);
s.set(c, "pitch_jitter", 12.0);
s.set(c, "spray", 0.5);
s.audio_filter(c, "bandpass", 1200.0, 1.5);
s.color_keep(c, 200.0, 140.0);
s.set(c, "gain_to_opacity", 1.0);   // AV correspondence dials
let d = s.effect(c, "delay");       // AV effect chain: crush/delay/reverb/compress
s.fx(c, d, "time", 0.3);
s.fx(c, d, "feedback", 0.55);
let m = s.master_effect("compress"); // master bus crunch
s.master_fx(m, "quality", 0.35);
print("clip + effects added — bounce to hear/see it");
"#;
