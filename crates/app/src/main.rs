//! chromagrain — audiovisual granular sampling compositor / sequencer /
//! performance tool.
//!
//! GUI:       chromagrain
//! Headless:  chromagrain --script composition.rhai [base_dir]
//!            chromagrain --render-demo out.mp4
//!
//! UI model (one place per thing):
//! * **Library** (left) — every piece of media: files, YouTube scrapes,
//!   synths, and derived artifacts (paulstretch washes, segment cutouts).
//!   Drag a card onto a track to use it.
//! * **Timeline** (center) — tracks, clips and the master bus. A clip is a
//!   clip: whether it plays as a *snippet*, a *grain cloud* or a *pattern*
//!   is a mode you flip in the inspector, not a different kind of object.
//!   A selected pattern clip opens its step grid right under the timeline.
//! * **Inspector** (right) — edits whatever is selected (source, clip,
//!   track or master). Nothing is editable in two places.

mod gpu;
mod midi;
mod playback;
mod theme;
mod ui_inspector;
mod ui_library;
mod ui_timeline;
mod ui_topbar;

use chromagrain_core::media;
use chromagrain_core::render::{plan_project, render_project, ClipPlan};
use chromagrain_core::script::run_script;
use chromagrain_core::seq::StepPattern;
use chromagrain_core::timeline::{Clip, ClipKind, Project, Source};
use eframe::egui;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "chromagrain",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// The one thing the inspector is editing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sel {
    None,
    Source(usize),
    Track(usize),
    Clip(usize, usize),
    Master,
}

/// How a clip plays its media. Flippable in place from the inspector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    Snippet,
    Grains,
    Pattern,
}

impl PlayMode {
    pub fn of(kind: ClipKind) -> PlayMode {
        match kind {
            ClipKind::Snippet => PlayMode::Snippet,
            ClipKind::Granular => PlayMode::Grains,
            ClipKind::Pattern(_) => PlayMode::Pattern,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Snippet => "snippet",
            PlayMode::Grains => "grains",
            PlayMode::Pattern => "pattern",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            PlayMode::Snippet => "plays the media straight",
            PlayMode::Grains => "a cloud of tiny pieces of the media",
            PlayMode::Pattern => "step-sequenced hits (grid below the timeline)",
        }
    }
}

/// Payload carried while dragging a library card.
#[derive(Debug, Clone, Copy)]
pub struct SourceDrag(pub usize);

enum Job {
    Load(String),
    Youtube(String),
}

enum WorkerMsg {
    Source(Result<Source, String>),
    Script(Result<String, String>),
    Export(Result<String, String>),
}

/// In-progress clip drag on the timeline.
#[derive(Debug, Clone, Copy)]
pub struct ClipDrag {
    pub resize: bool,
    pub ti: usize,
    pub ci: usize,
    /// Pointer beat minus clip start at grab time.
    pub grab: f64,
}

pub struct App {
    project: Arc<Mutex<Project>>,
    /// The plan and the exact project snapshot it was built from. The GPU
    /// preview always gets a matching pair, so a stale plan can never point
    /// at a clip that no longer exists.
    plan: Arc<Vec<ClipPlan>>,
    plan_project: Project,
    project_rev: u64,
    plan_rev: u64,

    sel: Sel,
    /// Track new clips land on (last touched track).
    focus_track: usize,
    /// Last source touched — what pads play when no clip is selected.
    last_source: usize,

    playback: playback::Playback,
    loop_on: bool,
    loop_start: f64,
    loop_end: f64,
    playhead_beats: f64,

    gpu: Arc<Mutex<Option<gpu::GpuPreview>>>,
    gpu_failed: Arc<AtomicBool>,
    scene: Arc<Mutex<Option<gpu::Scene>>>,

    busy: Option<String>,
    jobs: VecDeque<Job>,
    rx: mpsc::Receiver<WorkerMsg>,
    tx: mpsc::Sender<WorkerMsg>,
    status: String,

    midi: midi::Midi,
    midi_port: usize,
    cc_mappings: Vec<midi::CcMapping>,
    /// Live pad hits for the GPU preview, on the app clock.
    live_events: Vec<(usize, chromagrain_core::grain::GrainEvent)>,
    live_id: u64,
    app_clock: std::time::Instant,
    last_sync: std::time::Instant,

    youtube_url: String,
    stretch_factor: f32,
    segment_prompt: String,
    script_text: String,
    script_log: String,
    show_script: bool,

    beat_w: f32,
    clip_drag: Option<ClipDrag>,
    scrubbing: bool,
    context_clip: Option<(usize, usize)>,
    expanded_rows: HashSet<(usize, usize)>,
    /// Step value being painted by a press-and-drag in the pattern grid.
    step_paint: Option<bool>,
    thumbs: HashMap<usize, egui::TextureHandle>,
    peaks: HashMap<usize, Arc<Vec<f32>>>,
}

impl App {
    fn new() -> App {
        let (tx, rx) = mpsc::channel();
        let project = Project::demo();
        let plan = Arc::new(plan_project(&project));
        App {
            plan_project: project.clone(),
            project: Arc::new(Mutex::new(project)),
            plan,
            project_rev: 1,
            plan_rev: 1,
            sel: Sel::Clip(0, 0),
            focus_track: 0,
            last_source: 0,
            playback: playback::Playback::new(),
            loop_on: false,
            loop_start: 0.0,
            loop_end: 8.0,
            playhead_beats: 0.0,
            gpu: Arc::new(Mutex::new(None)),
            gpu_failed: Arc::new(AtomicBool::new(false)),
            scene: Arc::new(Mutex::new(None)),
            busy: None,
            jobs: VecDeque::new(),
            rx,
            tx,
            status: "demo project loaded — press space to play".into(),
            midi: midi::Midi::new(),
            midi_port: 0,
            cc_mappings: Vec::new(),
            live_events: Vec::new(),
            live_id: 0,
            app_clock: std::time::Instant::now(),
            last_sync: std::time::Instant::now(),
            youtube_url: String::new(),
            stretch_factor: 8.0,
            segment_prompt: String::new(),
            script_text: DEFAULT_SCRIPT.trim_start().into(),
            script_log: String::new(),
            show_script: false,
            beat_w: 44.0,
            clip_drag: None,
            scrubbing: false,
            context_clip: None,
            expanded_rows: HashSet::new(),
            step_paint: None,
            thumbs: HashMap::new(),
            peaks: HashMap::new(),
        }
    }

    /// Something in the project changed.
    fn mark(&mut self) {
        self.project_rev += 1;
    }

    /// Something structural changed (clips/tracks/sources added, removed or
    /// moved): skip the throttle so the engine picks it up this frame.
    fn mark_now(&mut self) {
        self.project_rev += 1;
        self.last_sync = std::time::Instant::now() - std::time::Duration::from_secs(1);
    }

    fn loop_region(&self) -> Option<(f64, f64)> {
        (self.loop_on && self.loop_end > self.loop_start).then_some((self.loop_start, self.loop_end))
    }

    /// Replans events + live-updates the always-on engine. Throttled so CC
    /// and slider streams don't rebuild per message.
    fn sync_project(&mut self) {
        if self.project_rev == self.plan_rev {
            return;
        }
        if self.last_sync.elapsed() < std::time::Duration::from_millis(80) {
            return; // try again next frame
        }
        self.last_sync = std::time::Instant::now();
        self.plan_rev = self.project_rev;
        let p = self.project.lock().unwrap().clone();
        self.plan = Arc::new(plan_project(&p));
        self.plan_project = p.clone();
        self.playback.set_project(p);
    }

    fn start_play(&mut self) {
        let p = self.project.lock().unwrap().clone();
        self.playback.play(&p, self.playhead_beats, self.loop_region());
        self.playback.set_project(p);
    }

    fn toggle_play(&mut self) {
        if self.playback.is_playing() {
            self.playback.stop();
        } else {
            self.start_play();
        }
    }

    fn seek(&mut self, beat: f64) {
        self.playhead_beats = beat.max(0.0);
        if self.playback.is_playing() {
            self.start_play();
        }
    }

    fn select(&mut self, sel: Sel) {
        self.sel = sel;
        match sel {
            Sel::Clip(ti, ci) => {
                self.focus_track = ti;
                let p = self.project.lock().unwrap();
                if let Some(c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)) {
                    if !matches!(c.kind, ClipKind::Pattern(_)) {
                        self.last_source = c.source;
                    }
                }
            }
            Sel::Track(ti) => self.focus_track = ti,
            Sel::Source(i) => self.last_source = i,
            _ => {}
        }
    }

    fn selected_clip(&self) -> Option<(usize, usize)> {
        match self.sel {
            Sel::Clip(t, c) => Some((t, c)),
            _ => None,
        }
    }

    // ------------------------------------------------------------ clip ops

    fn quantize(&self) -> f64 {
        self.project.lock().unwrap().quantize.max(0.0625)
    }

    fn snap(&self, beat: f64) -> f64 {
        let q = self.quantize();
        ((beat / q).round() * q).max(0.0)
    }

    /// Put a source on a track as a clip in the given mode.
    fn place_source(&mut self, src: usize, track: usize, beat: f64, mode: PlayMode) {
        let mut p = self.project.lock().unwrap();
        if src >= p.sources.len() {
            return;
        }
        if p.tracks.is_empty() {
            p.add_track("track 1");
        }
        let t = track.min(p.tracks.len() - 1);
        let secs = p.sources[src].duration();
        let media_beats = p.secs_to_beats(secs);
        let q = p.quantize.max(0.0625);
        let clip = match mode {
            PlayMode::Snippet => {
                let len = ((media_beats / q).round() * q).clamp(1.0, 32.0);
                Clip::new_snippet(src, beat, len)
            }
            PlayMode::Grains => Clip::new(src, beat, 4.0),
            PlayMode::Pattern => {
                let pat = new_pattern_for(&p.sources[src].name, src);
                let pid = p.add_pattern(pat);
                Clip::new_pattern(pid, beat, 4.0)
            }
        };
        p.tracks[t].clips.push(clip);
        let ci = p.tracks[t].clips.len() - 1;
        let name = p.sources[src].name.clone();
        drop(p);
        self.select(Sel::Clip(t, ci));
        self.status = format!("placed '{name}' as {}", mode.label());
        self.mark_now();
    }

    /// Flip a clip between snippet / grains / pattern in place.
    fn set_play_mode(&mut self, ti: usize, ci: usize, mode: PlayMode) {
        let mut p = self.project.lock().unwrap();
        let Some(clip) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)).cloned() else { return };
        if PlayMode::of(clip.kind) == mode {
            return;
        }
        // Which media is this clip "about"?
        let src = match clip.kind {
            ClipKind::Pattern(pid) => {
                p.patterns.get(pid).and_then(|pat| pat.rows.first()).map(|r| r.source).unwrap_or(0)
            }
            _ => clip.source,
        };
        let src_name = p.sources.get(src).map(|s| s.name.clone()).unwrap_or_default();
        let c = &mut p.tracks[ti].clips[ci];
        match mode {
            PlayMode::Snippet => {
                let fresh = Clip::new_snippet(src, 0.0, 1.0);
                c.kind = ClipKind::Snippet;
                c.source = src;
                c.grains.envelope = fresh.grains.envelope;
                c.grains.pan_spread = fresh.grains.pan_spread;
            }
            PlayMode::Grains => {
                let fresh = Clip::new(src, 0.0, 1.0);
                c.kind = ClipKind::Granular;
                c.source = src;
                c.grains.envelope = fresh.grains.envelope;
                c.grains.pan_spread = fresh.grains.pan_spread;
            }
            PlayMode::Pattern => {
                let pat = new_pattern_for(&src_name, src);
                let pid = p.add_pattern(pat);
                p.tracks[ti].clips[ci].kind = ClipKind::Pattern(pid);
            }
        }
        drop(p);
        self.mark_now();
    }

    fn duplicate_clip(&mut self, ti: usize, ci: usize) {
        let mut p = self.project.lock().unwrap();
        let Some(mut c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)).cloned() else { return };
        c.start_beat = c.end_beat();
        // Duplicated pattern clips get their own copy of the pattern, so
        // editing one never silently edits the other.
        if let ClipKind::Pattern(pid) = c.kind {
            if let Some(pat) = p.patterns.get(pid).cloned() {
                let mut pat = pat;
                pat.name = format!("{} copy", pat.name);
                c.kind = ClipKind::Pattern(p.add_pattern(pat));
            }
        }
        p.tracks[ti].clips.push(c);
        let new_ci = p.tracks[ti].clips.len() - 1;
        drop(p);
        self.select(Sel::Clip(ti, new_ci));
        self.mark_now();
    }

    fn delete_clip(&mut self, ti: usize, ci: usize) {
        let mut p = self.project.lock().unwrap();
        if let Some(t) = p.tracks.get_mut(ti) {
            if ci < t.clips.len() {
                t.clips.remove(ci);
            }
        }
        drop(p);
        self.cc_mappings.retain(|m| !matches!(m.target, midi::CcTarget::ClipParam { track, clip, .. } if track == ti && clip == ci));
        self.sel = Sel::Track(ti);
        self.clip_drag = None;
        self.mark_now();
    }

    fn add_track(&mut self) {
        let mut p = self.project.lock().unwrap();
        let n = p.tracks.len();
        let ti = p.add_track(&format!("track {}", n + 1));
        drop(p);
        self.select(Sel::Track(ti));
        self.mark_now();
    }

    fn delete_track(&mut self, ti: usize) {
        let mut p = self.project.lock().unwrap();
        if ti < p.tracks.len() {
            p.tracks.remove(ti);
        }
        drop(p);
        self.cc_mappings.clear();
        self.sel = Sel::None;
        self.focus_track = 0;
        self.mark_now();
    }

    fn move_track(&mut self, ti: usize, up: bool) {
        let mut p = self.project.lock().unwrap();
        let n = p.tracks.len();
        let to = if up { ti.checked_sub(1) } else { (ti + 1 < n).then_some(ti + 1) };
        let Some(to) = to else { return };
        p.tracks.swap(ti, to);
        drop(p);
        self.cc_mappings.clear();
        self.select(Sel::Track(to));
        self.mark_now();
    }

    /// Clips that use a source (directly, or through a pattern row).
    fn source_users(p: &Project, src: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (ti, t) in p.tracks.iter().enumerate() {
            for (ci, c) in t.clips.iter().enumerate() {
                let uses = match c.kind {
                    ClipKind::Pattern(pid) => {
                        p.patterns.get(pid).is_some_and(|pat| pat.rows.iter().any(|r| r.source == src))
                    }
                    _ => c.source == src,
                };
                if uses {
                    out.push((ti, ci));
                }
            }
        }
        out
    }

    /// Remove an unused source, shifting every later source id down.
    fn delete_source(&mut self, src: usize) {
        let mut p = self.project.lock().unwrap();
        if src >= p.sources.len() || !Self::source_users(&p, src).is_empty() {
            return;
        }
        // Rows in patterns no clip uses can still point at it; retarget.
        for pat in &mut p.patterns {
            pat.rows.retain(|r| r.source != src);
            for r in &mut pat.rows {
                if r.source > src {
                    r.source -= 1;
                }
            }
        }
        for t in &mut p.tracks {
            for c in &mut t.clips {
                if !matches!(c.kind, ClipKind::Pattern(_)) && c.source > src {
                    c.source -= 1;
                }
            }
        }
        let name = p.sources.remove(src).name;
        drop(p);
        self.thumbs.clear();
        self.peaks.clear();
        self.sel = Sel::None;
        self.last_source = 0;
        self.status = format!("removed '{name}'");
        self.mark_now();
    }

    // ------------------------------------------------------------ pads/midi

    /// Fire a pad: play the selected clip's grain preset (or the last
    /// source touched) at `note` (sampler mapping, C4 = unity).
    fn trigger_note(&mut self, note: u8, velocity: u8) {
        let (source, settings, source_len) = {
            let p = self.project.lock().unwrap();
            let from_clip = self.selected_clip().and_then(|(ti, ci)| {
                p.tracks.get(ti).and_then(|t| t.clips.get(ci)).and_then(|c| match c.kind {
                    ClipKind::Pattern(_) => None,
                    _ => Some((c.source, c.grains.clone())),
                })
            });
            let (s, g) = from_clip.unwrap_or_else(|| {
                (
                    self.last_source.min(p.sources.len().saturating_sub(1)),
                    chromagrain_core::grain::GrainSettings { duration: 0.4, ..Default::default() },
                )
            });
            let len = p.sources.get(s).map(|s| s.duration()).unwrap_or(0.0);
            (s, g, len)
        };
        if source_len <= 0.0 {
            return;
        }
        self.live_id += 1;
        let ev = chromagrain_core::grain::grain_from_note(
            &settings,
            note,
            velocity,
            source_len,
            0.0, // engine stamps its own onset
            self.live_id,
        );
        self.playback.trigger(source, ev.clone());
        let mut vis = ev;
        vis.onset = self.app_clock.elapsed().as_secs_f64();
        self.live_events.push((source, vis));
    }

    fn handle_midi(&mut self) {
        for msg in self.midi.poll() {
            match msg {
                midi::MidiMsg::NoteOn { note, velocity } => self.trigger_note(note, velocity),
                midi::MidiMsg::NoteOff { .. } => {}
                midi::MidiMsg::Cc { cc, value } => {
                    if let Some(m) = self.cc_mappings.iter_mut().find(|m| m.cc.is_none()) {
                        m.cc = Some(cc);
                        self.status = format!("mapped CC {cc} → {}", m.target.label());
                    }
                    let mut changed = false;
                    {
                        let mut p = self.project.lock().unwrap();
                        for m in &self.cc_mappings {
                            if m.cc == Some(cc) {
                                changed |= midi::apply_cc(&mut p, &m.target, value);
                            }
                        }
                    }
                    if changed {
                        self.mark();
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ workers

    fn queue_load(&mut self, path: String) {
        self.jobs.push_back(Job::Load(path));
        self.pump_jobs();
    }

    fn queue_youtube(&mut self, url: String) {
        self.jobs.push_back(Job::Youtube(url));
        self.pump_jobs();
    }

    fn pump_jobs(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let Some(job) = self.jobs.pop_front() else { return };
        let sr = self.project.lock().unwrap().sample_rate;
        let tx = self.tx.clone();
        match job {
            Job::Load(path) => {
                let short = std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(path.clone());
                self.busy = Some(format!("loading {short}…"));
                std::thread::spawn(move || {
                    let res = media::load_source(std::path::Path::new(&path), sr);
                    let _ = tx.send(WorkerMsg::Source(res));
                });
            }
            Job::Youtube(url) => {
                self.busy = Some("fetching from YouTube…".into());
                let cache = std::env::temp_dir().join("chromagrain-yt");
                std::thread::spawn(move || {
                    let res = media::load_youtube_source(&url, &cache, sr);
                    let _ = tx.send(WorkerMsg::Source(res));
                });
            }
        }
    }

    fn start_paulstretch(&mut self, src_idx: usize, factor: f32) {
        if self.busy.is_some() {
            return;
        }
        let Some(source) = self.project.lock().unwrap().sources.get(src_idx).cloned() else {
            return;
        };
        self.busy = Some(format!("paulstretching {} ×{:.0}…", source.name, factor));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = Ok(chromagrain_core::stretch::paulstretch_source(&source, factor));
            let _ = tx.send(WorkerMsg::Source(res));
        });
    }

    fn start_segment(&mut self, src_idx: usize, prompt: String) {
        if self.busy.is_some() || prompt.trim().is_empty() {
            return;
        }
        let Some(source) = self.project.lock().unwrap().sources.get(src_idx).cloned() else {
            return;
        };
        self.busy = Some(format!("cutting '{prompt}' out of {}…", source.name));
        let tx = self.tx.clone();
        let work = std::env::temp_dir().join("chromagrain-segment");
        std::thread::spawn(move || {
            let res = media::segment_source(&source, prompt.trim(), &work);
            let _ = tx.send(WorkerMsg::Source(res));
        });
    }

    fn add_synth(&mut self, which: &str) {
        let mut p = self.project.lock().unwrap();
        let sr = p.sample_rate;
        let src = match which {
            "fm" => chromagrain_core::synth::fm_source(110.0, 2.0, 1.5, 4.0, sr),
            "noise" => chromagrain_core::synth::noise_source(0.5, 4.0, sr),
            _ => Source {
                name: "test pattern (saw)".into(),
                audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(110.0, 4.0, sr))),
                video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(
                    240, 136, 12.0, 4.0,
                ))),
                base_hz: 110.0,
            },
        };
        let name = src.name.clone();
        let id = p.add_source(src);
        drop(p);
        self.select(Sel::Source(id));
        self.status = format!("added '{name}' to the library");
        self.mark_now();
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
        let ext = if wav_only || !media::ffmpeg_available() { "wav" } else { "mp4" };
        let default_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let Some(path) = rfd::FileDialog::new()
            .set_directory(&default_dir)
            .set_file_name(format!("chromagrain-render.{ext}"))
            .add_filter(ext, &[ext])
            .save_file()
        else {
            return;
        };
        self.busy = Some(format!("rendering {ext}…"));
        let project = self.project.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let p = project.lock().unwrap().clone();
            let end = p.end_beat().max(1.0);
            let out = render_project(&p, 0.0, end);
            let res = if ext == "wav" {
                media::save_wav(&path, &out.audio).map(|_| path.display().to_string())
            } else {
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
                    self.select(Sel::Source(id));
                    self.mark_now();
                    self.status = if hz > 0.0 {
                        format!("added '{name}' (base pitch ≈ {hz:.1} Hz) — drag it onto a track")
                    } else {
                        format!("added '{name}' — drag it onto a track")
                    };
                }
                WorkerMsg::Source(Err(e)) => self.status = format!("couldn't add media: {e}"),
                WorkerMsg::Script(Ok(log)) => {
                    self.script_log = if log.is_empty() { "(ok)".into() } else { log };
                    self.mark_now();
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
        self.pump_jobs();
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Files dropped from the Finder/Explorer go straight to the library.
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            self.queue_load(path.display().to_string());
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let (space, del, dup, esc, home) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.modifiers.command && i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Home) || i.key_pressed(egui::Key::Enter),
            )
        });
        if space {
            self.toggle_play();
        }
        if home {
            self.seek(if self.loop_on { self.loop_start } else { 0.0 });
        }
        if esc {
            self.sel = Sel::None;
        }
        if let Some((ti, ci)) = self.selected_clip() {
            if del {
                self.delete_clip(ti, ci);
            } else if dup {
                self.duplicate_clip(ti, ci);
            }
        }
    }
}

/// A fresh pattern playing `src` on every beat — a sensible starting point.
fn new_pattern_for(name: &str, src: usize) -> StepPattern {
    let mut pat = StepPattern::new(name);
    let r = pat.add_row(src);
    let spb = pat.steps_per_beat as usize;
    for i in (0..pat.rows[r].steps.len()).step_by(spb.max(1)) {
        pat.rows[r].steps[i].on = true;
    }
    pat
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_midi();
        self.poll_workers();
        self.handle_shortcuts(ctx);
        self.sync_project();

        let now = self.app_clock.elapsed().as_secs_f64();
        self.live_events.retain(|(_, e)| e.end() + 0.5 > now);
        if !self.live_events.is_empty() {
            ctx.request_repaint();
        }
        if self.project_rev != self.plan_rev {
            ctx.request_repaint_after(std::time::Duration::from_millis(90));
        }
        if self.playback.is_playing() {
            let p = self.project.lock().unwrap();
            self.playhead_beats = p.secs_to_beats(self.playback.playhead_secs(&p));
            drop(p);
            ctx.request_repaint();
        } else if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("transport")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ctx, |ui| self.transport_ui(ui));
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ctx, |ui| self.status_ui(ui));
        if self.show_script {
            egui::TopBottomPanel::bottom("script")
                .resizable(true)
                .default_height(220.0)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(10)))
                .show(ctx, |ui| self.script_ui(ui));
        }
        egui::SidePanel::left("library")
            .default_width(250.0)
            .width_range(200.0..=380.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| self.library_ui(ui));
        egui::SidePanel::right("inspector")
            .default_width(350.0)
            .width_range(300.0..=520.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("inspector_scroll").show(ui, |ui| {
                    self.inspector_ui(ui);
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| {
                self.preview_ui(ui);
                ui.add_space(8.0);
                self.timeline_ui(ui);
                if let Some((ti, ci)) = self.selected_clip() {
                    let pid = {
                        let p = self.project.lock().unwrap();
                        p.tracks.get(ti).and_then(|t| t.clips.get(ci)).and_then(|c| match c.kind {
                            ClipKind::Pattern(pid) => Some(pid),
                            _ => None,
                        })
                    };
                    if let Some(pid) = pid {
                        ui.add_space(8.0);
                        self.pattern_editor_ui(ui, ti, ci, pid);
                    }
                }
            });

        self.paint_drag_ghost(ctx);
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
