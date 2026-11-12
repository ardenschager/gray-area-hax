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
//! * **Perform** (top-bar switch) — the timeline becomes a 4×4 grid of
//!   looping pads. Each pad is a track whose first clip loops forever;
//!   pads start/stop on the next beat or bar. Sources, patterns and the
//!   master bus are shared with the arrangement.
//! * **Output** (`O` / top-bar button) — a second window with only the
//!   picture, fullscreen on the second display (see `ui_output`).

mod browser;
mod demo;
mod displays;
mod history;
mod midi;
mod playback;
#[cfg(test)]
mod showcase;
mod studio_host;
#[cfg(test)]
mod studio_tests;
mod theme;
mod ui_browse;
mod ui_inspector;
mod ui_library;
mod ui_output;
mod ui_perform;
mod ui_synth;
mod ui_timeline;
mod ui_topbar;

pub(crate) use chromagrain_ui::gpu;
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
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    // `chromagrain clip.mp4 other.mov` — open with those files placed on
    // their own tracks at the start. `--perform` opens on the pads (the
    // files land on pads instead). Starts blank; `--demo` loads the demo.
    // `--output` opens the performance output window, `--play` starts the
    // transport (together: an unattended installation).
    let perform = args.iter().any(|a| a == "--perform");
    let demo = args.iter().any(|a| a == "--demo");
    let output = args.iter().any(|a| a == "--output");
    // The play-first studio UI is the default; `--classic` opens the
    // previous timeline editor (kept until the studio reaches parity).
    let classic = args.iter().any(|a| a == "--classic");
    let autoplay = args.iter().any(|a| a == "--play");
    let open: Vec<PathBuf> =
        args.iter().skip(1).map(PathBuf::from).filter(|p| p.is_file()).collect();
    eframe::run_native(
        "chromagrain",
        options,
        Box::new(move |cc| {
            if classic {
                theme::apply(&cc.egui_ctx);
            }
            // `--demo` opens Night Platform (assets/demo) in the studio; the
            // classic editor, or a missing folder / ffmpeg, gets the
            // built-in procedural demo instead.
            let night = (demo && !classic)
                .then(demo::DemoSpec::night_platform)
                .and_then(|spec| match spec.load_found(48000) {
                    Ok(sources) => Some((spec, sources)),
                    Err(e) => {
                        eprintln!("demo: {e} — opening the built-in demo instead");
                        None
                    }
                });
            let mut app = App::new(AppOpts { demo: demo && night.is_none(), classic, headless: false });
            if let Some((spec, sources)) = night {
                app.install_demo(&spec, sources);
            }
            if !classic {
                app.studio.install_theme(&cc.egui_ctx);
            }
            if let Some(rs) = cc.wgpu_render_state.as_ref() {
                gpu::install(rs);
            } else {
                eprintln!("gpu preview unavailable (no wgpu render state)");
                app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if perform {
                app.set_mode(AppMode::Perform);
            }
            for p in open {
                app.import_path(p, Some(Place { track: 0, new_track: true, beat: 0.0 }));
            }
            if output {
                app.toggle_output();
            }
            if autoplay {
                app.start_play();
            }
            Ok(Box::new(app))
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

/// Payload carried while dragging a file from the Browse tab (not yet
/// imported — it's decoded when dropped).
#[derive(Debug, Clone)]
pub struct BrowseDrag(pub PathBuf);

/// Where a file should land on the timeline once it has loaded.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub track: usize,
    /// Append a fresh track instead of using `track`.
    pub new_track: bool,
    pub beat: f64,
}

/// Which list the library panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibTab {
    Project,
    Browse,
}

/// Arrange = tracks on a timeline. Perform = looping pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Arrange,
    Perform,
}

/// When a pad you press actually starts/stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchQ {
    Now,
    Beat,
    Bar,
}

impl LaunchQ {
    pub fn label(self) -> &'static str {
        match self {
            LaunchQ::Now => "now",
            LaunchQ::Beat => "next beat",
            LaunchQ::Bar => "next bar",
        }
    }
    fn beats(self) -> f64 {
        match self {
            LaunchQ::Now => 0.0,
            LaunchQ::Beat => 1.0,
            LaunchQ::Bar => 4.0,
        }
    }
}

/// Pads in perform mode (a 4×4 grid). Pad i is track i.
pub const PADS: usize = 16;

/// Snap a loop length to 1, 2, 4 … 32 beats (nearest, in log terms).
pub fn pow2_beats(b: f64) -> f64 {
    let b = b.clamp(1.0, 32.0);
    2f64.powf(b.log2().round())
}

enum Job {
    Load(String, Option<Place>),
    Youtube(String),
}

pub(crate) enum WorkerMsg {
    Source(Result<Source, String>),
    /// A file finished loading: remember its path, maybe place it.
    Loaded(PathBuf, Option<Place>, Result<Source, String>),
    Script(Result<String, String>),
    Export(Result<String, String>),
    /// A bounced Rec take and the beat it was recorded at.
    Take(chromagrain_core::timeline::Source, f64),
}


/// In-progress clip drag on the timeline.
#[derive(Debug, Clone, Copy)]
pub struct ClipDrag {
    pub resize: bool,
    /// Dragging the repeat tab: the edge grows in whole passes.
    pub repeat: bool,
    pub ti: usize,
    pub ci: usize,
    /// Pointer beat minus clip start at grab time.
    pub grab: f64,
    /// Dragging the LEFT edge: trims the start (media start moves with it).
    pub trim_left: bool,
    /// At grab time: (start beat, length beats, media position 0..1).
    pub orig: (f64, f64, f32),
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

    gpu_failed: Arc<AtomicBool>,
    /// The scene every canvas presents this frame (published once per
    /// update; the GPU renders it once however many windows show it).
    scene: Arc<Mutex<Option<gpu::Scene>>>,
    /// App frame counter (`Scene::frame`).
    frame_no: u64,
    /// The performance output window (`O`).
    output: ui_output::OutputWin,
    /// Single-display output: go fullscreen in this window next frame.
    pending_fullscreen: bool,
    /// The previous timeline editor instead of the studio (`--classic`).
    classic: bool,
    /// The play-first studio UI state.
    studio: chromagrain_ui::Studio,

    busy: Option<String>,
    jobs: VecDeque<Job>,
    rx: mpsc::Receiver<WorkerMsg>,
    tx: mpsc::Sender<WorkerMsg>,
    status: String,

    midi: midi::Midi,
    midi_port: usize,
    cc_mappings: Vec<midi::CcMapping>,
    /// Live pad hits for the GPU preview, on the app clock.
    live_events: Vec<(usize, u8, chromagrain_core::grain::GrainEvent)>,
    live_id: u64,
    app_clock: Clock,
    /// `app_clock` seconds of the last engine sync (throttle).
    last_sync: f64,

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

    lib_tab: LibTab,
    browser: browser::Browser,
    /// File path -> source id, so a file is only ever decoded once.
    imported: HashMap<PathBuf, usize>,
    /// Paths queued/decoding right now (avoid double-queueing).
    loading: HashSet<PathBuf>,
    /// Preview fills the whole screen (F / esc / double-click).
    fullscreen: bool,
    gizmo: Option<ui_timeline::GizmoDrag>,

    mode: AppMode,
    /// The tracks of whichever mode is NOT showing (arrangement tracks
    /// while performing, pad tracks while arranging).
    other_tracks: Vec<chromagrain_core::timeline::Track>,
    /// Loop settings of the mode that is not showing.
    other_loop: (bool, f64, f64),
    /// Pads waiting for the next launch boundary: (pad, turn on?).
    pending: Vec<(usize, bool)>,
    launch_q: LaunchQ,
    /// project_rev the pad loops were last rebuilt for.
    fill_rev: u64,
    /// Look-ahead playhead last frame (to spot boundary crossings).
    prev_launch_t: Option<f64>,

    /// SuperCollider instrument parts: renders, auditions, editor state.
    synth: ui_synth::SynthState,
    /// A one-click cut-out in progress: (track, clip, original source) —
    /// the clip switches to the cut-out when it arrives.
    pending_cutout: Option<(usize, usize, usize)>,
    /// A per-clip paulstretch in progress: (track, clip, original source,
    /// factor) — the clip swaps to the stretched region when it arrives.
    pub(crate) pending_stretch: Option<(usize, usize, usize, f32)>,

    /// Undo / redo (whole-project snapshots, one step per gesture).
    history: history::History,
    /// Copied clip (+ its pattern, for pattern clips).
    clipboard: Option<(Clip, Option<StepPattern>)>,
    /// Put a marker on the OS clipboard next frame so ⌘V reaches us.
    os_copy: bool,
    /// Files dropped on the timeline that are still decoding: drawn as
    /// placeholders where they'll land (path, place, length in beats).
    drops: Vec<(PathBuf, Place, f64)>,
}

/// The app's monotonic clock (live grain onsets, the canvas, throttles):
/// wall time, or a simulated clock a frame-locked recorder advances.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Clock {
    Wall(std::time::Instant),
    #[cfg_attr(not(test), allow(dead_code))]
    Sim(f64),
}

impl Clock {
    pub(crate) fn secs(&self) -> f64 {
        match self {
            Clock::Wall(t0) => t0.elapsed().as_secs_f64(),
            Clock::Sim(t) => *t,
        }
    }
}

/// How to start the app.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppOpts {
    /// Open the built-in demo project instead of a blank canvas.
    pub demo: bool,
    /// The classic timeline editor instead of the studio.
    pub classic: bool,
    /// No audio device, no disk scan, no MIDI (tests / headless renders).
    pub headless: bool,
}

impl App {
    /// A blank canvas (`demo` = the built-in demo project instead).
    fn new(opts: AppOpts) -> App {
        let demo = opts.demo;
        let (tx, rx) = mpsc::channel();
        let project = if demo { Project::demo() } else { Project::default() };
        let plan = Arc::new(plan_project(&project));
        let mut browser = browser::Browser::new();
        if !opts.headless {
            browser.ensure_started();
        }
        App {
            plan_project: project.clone(),
            project: Arc::new(Mutex::new(project)),
            plan,
            project_rev: 1,
            plan_rev: 1,
            sel: if demo { Sel::Clip(0, 0) } else { Sel::None },
            focus_track: 0,
            last_source: 0,
            playback: if opts.headless { playback::Playback::new_headless() } else { playback::Playback::new() },
            loop_on: false,
            loop_start: 0.0,
            loop_end: 8.0,
            playhead_beats: 0.0,
            gpu_failed: Arc::new(AtomicBool::new(false)),
            scene: Arc::new(Mutex::new(None)),
            frame_no: 0,
            output: ui_output::OutputWin::default(),
            pending_fullscreen: false,
            classic: opts.classic,
            studio: chromagrain_ui::Studio::default(),
            busy: None,
            jobs: VecDeque::new(),
            rx,
            tx,
            status: if demo {
                "demo project loaded — press space to play".into()
            } else {
                "blank canvas — drop video or audio anywhere, or drag from Browse".into()
            },
            midi: if opts.headless { midi::Midi::none() } else { midi::Midi::new() },
            midi_port: 0,
            cc_mappings: Vec::new(),
            live_events: Vec::new(),
            live_id: 0,
            app_clock: Clock::Wall(std::time::Instant::now()),
            last_sync: 0.0,
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
            lib_tab: LibTab::Project,
            browser,
            imported: HashMap::new(),
            loading: HashSet::new(),
            fullscreen: false,
            gizmo: None,
            mode: AppMode::Arrange,
            other_tracks: Vec::new(),
            other_loop: (true, 0.0, 4.0),
            pending: Vec::new(),
            launch_q: LaunchQ::Bar,
            fill_rev: 0,
            prev_launch_t: None,
            synth: ui_synth::SynthState::new(),
            pending_cutout: None,
            pending_stretch: None,
            history: history::History::default(),
            clipboard: None,
            os_copy: false,
            drops: Vec::new(),
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
        self.last_sync = f64::NEG_INFINITY;
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
        let now = self.app_clock.secs();
        if now - self.last_sync < 0.08 {
            return; // try again next frame
        }
        self.last_sync = now;
        self.plan_rev = self.project_rev;
        let p = {
            let mut g = self.project.lock().unwrap();
            g.resolve_forever();
            g.clone()
        };
        self.plan = Arc::new(plan_project(&p));
        self.plan_project = p.clone();
        self.playback.set_project(p);
    }

    pub(crate) fn start_play(&mut self) {
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
        if self.mode == AppMode::Perform {
            // Target pad if it's empty, else the first empty pad.
            let pad = {
                let p = self.project.lock().unwrap();
                let empty = |i: usize| p.tracks.get(i).is_none_or(|t| t.clips.is_empty());
                if track < PADS && empty(track) {
                    track
                } else {
                    (0..PADS).find(|&i| empty(i)).unwrap_or(track.min(PADS - 1))
                }
            };
            self.load_pad(src, pad, mode);
            return;
        }
        let mut p = self.project.lock().unwrap();
        if src >= p.sources.len() {
            return;
        }
        if p.tracks.is_empty() {
            p.add_track("track 1");
        }
        let t = track.min(p.tracks.len() - 1);
        let clip = build_clip(&mut p, src, beat, mode);
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
                c.grains.texture = 0.0;
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

    /// Copy a clip to the app clipboard.
    fn copy_clip(&mut self, ti: usize, ci: usize) {
        let p = self.project.lock().unwrap();
        let Some(c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)).cloned() else { return };
        let pat = match c.kind {
            ClipKind::Pattern(pid) => p.patterns.get(pid).cloned(),
            _ => None,
        };
        drop(p);
        self.clipboard = Some((c, pat));
        self.os_copy = true;
        self.status = "copied — ⌘V pastes at the playhead".into();
    }

    /// Paste the clipboard clip at the playhead on the focused track, then
    /// move the playhead to its end so repeated pastes line up.
    fn paste_clip(&mut self) {
        let Some((mut c, pat)) = self.clipboard.clone() else {
            self.status = "nothing copied yet — select a clip and press ⌘C".into();
            return;
        };
        if self.mode == AppMode::Perform {
            self.status = "paste works on the arrangement (tab)".into();
            return;
        }
        let beat = self.snap(self.playhead_beats);
        let mut p = self.project.lock().unwrap();
        if p.tracks.is_empty() {
            p.add_track("track 1");
        }
        let ti = self.focus_track.min(p.tracks.len() - 1);
        if let Some(pat) = pat {
            c.kind = ClipKind::Pattern(p.add_pattern(pat));
        }
        if c.source >= p.sources.len() && !matches!(c.kind, ClipKind::Pattern(_)) {
            return;
        }
        c.start_beat = beat;
        let end = c.end_beat();
        p.tracks[ti].clips.push(c);
        let ci = p.tracks[ti].clips.len() - 1;
        drop(p);
        self.select(Sel::Clip(ti, ci));
        if !self.playback.is_playing() {
            self.playhead_beats = end;
        }
        self.status = "pasted".into();
        self.mark_now();
    }

    fn undo(&mut self) {
        self.history.flush(self.project_rev, &self.project);
        match self.history.undo() {
            Some(p) => {
                self.restore(p);
                self.status = "undo".into();
            }
            None => self.status = "nothing to undo".into(),
        }
    }

    fn redo(&mut self) {
        self.history.flush(self.project_rev, &self.project);
        match self.history.redo() {
            Some(p) => {
                self.restore(p);
                self.status = "redo".into();
            }
            None => self.status = "nothing to redo".into(),
        }
    }

    /// Install a project snapshot from the history and tidy app state that
    /// points into it.
    fn restore(&mut self, p: Project) {
        let n_src = p.sources.len();
        let n_tracks = p.tracks.len();
        let sel_ok = match self.sel {
            Sel::Clip(t, c) => p.tracks.get(t).is_some_and(|tr| c < tr.clips.len()),
            Sel::Track(t) => t < n_tracks,
            Sel::Source(i) => i < n_src,
            _ => true,
        };
        *self.project.lock().unwrap() = p;
        if !sel_ok {
            self.sel = Sel::None;
        }
        self.focus_track = self.focus_track.min(n_tracks.saturating_sub(1));
        self.last_source = self.last_source.min(n_src.saturating_sub(1));
        self.imported.retain(|_, i| *i < n_src);
        self.pending_cutout = None;
        self.pending_stretch = None;
        self.pending.clear();
        self.clip_drag = None;
        self.cc_mappings.clear();
        self.synth.reset_ids();
        self.mark_now();
        self.history.rebase(self.project_rev);
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
        let hidden_use = self.other_tracks.iter().flat_map(|t| &t.clips).any(|c| match c.kind {
            ClipKind::Pattern(pid) => p.patterns.get(pid).is_some_and(|pat| pat.rows.iter().any(|r| r.source == src)),
            _ => c.source == src,
        });
        if hidden_use {
            drop(p);
            let other = if self.mode == AppMode::Perform { "arrangement" } else { "perform pads" };
            self.status = format!("still used in the {other}");
            return;
        }
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
        for t in p.tracks.iter_mut().chain(self.other_tracks.iter_mut()) {
            for c in &mut t.clips {
                if !matches!(c.kind, ClipKind::Pattern(_)) && c.source > src {
                    c.source -= 1;
                }
            }
        }
        p.synth_parts.retain(|s| s.source != src);
        for s in &mut p.synth_parts {
            if s.source > src {
                s.source -= 1;
            }
        }
        let name = p.sources.remove(src).name;
        drop(p);
        self.synth.reset_ids();
        self.pending_cutout = None;
        self.pending_stretch = None;
        self.imported.retain(|_, id| *id != src);
        for id in self.imported.values_mut() {
            if *id > src {
                *id -= 1;
            }
        }
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
        vis.onset = self.app_clock.secs();
        self.live_events.push((source, 0, vis));
    }

    fn handle_midi(&mut self) {
        for msg in self.midi.poll() {
            match msg {
                midi::MidiMsg::NoteOn { note, velocity } => {
                    if self.mode == AppMode::Perform && (36..36 + PADS as u8).contains(&note) {
                        self.toggle_pad((note - 36) as usize);
                    } else {
                        self.trigger_note(note, velocity)
                    }
                }
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
        self.import_path(PathBuf::from(path), None);
    }

    /// Bring a file into the library (once) and optionally drop it on the
    /// timeline. Already-imported files are reused instantly.
    pub(crate) fn import_path(&mut self, path: PathBuf, place: Option<Place>) {
        if let Some(&id) = self.imported.get(&path) {
            match place {
                Some(pl) => self.place_at(id, pl),
                None => self.select(Sel::Source(id)),
            }
            return;
        }
        if !self.loading.insert(path.clone()) && place.is_none() {
            return; // already on its way
        }
        if let (Some(pl), AppMode::Arrange) = (place, self.mode) {
            let beats = self.browse_len_beats(&path);
            self.drops.push((path.clone(), pl, beats));
        }
        self.jobs.push_back(Job::Load(path.display().to_string(), place));
        self.pump_jobs();
    }

    /// How long a Browse file will be on the timeline, in beats (the same
    /// rule new snippet clips use), from its known duration.
    pub(crate) fn browse_len_beats(&self, path: &std::path::Path) -> f64 {
        let dur = self.browser.clip_by_path(path).map(|c| c.dur as f64).unwrap_or(0.0);
        self.media_len_beats(dur)
    }

    pub(crate) fn media_len_beats(&self, secs: f64) -> f64 {
        let p = self.project.lock().unwrap();
        if secs <= 0.0 {
            return 4.0;
        }
        let q = p.quantize.max(0.0625);
        ((p.secs_to_beats(secs) / q).round() * q).clamp(1.0, 32.0)
    }

    fn place_at(&mut self, src: usize, pl: Place) {
        self.last_source = src;
        if self.mode == AppMode::Perform {
            if pl.new_track {
                self.place_source(src, PADS, 0.0, PlayMode::Snippet); // first empty pad
            } else {
                self.load_pad(src, pl.track.min(PADS - 1), PlayMode::Snippet);
            }
            return;
        }
        let track = if pl.new_track {
            let mut p = self.project.lock().unwrap();
            let n = p.tracks.len();
            p.add_track(&format!("track {}", n + 1))
        } else {
            pl.track
        };
        self.place_source(src, track, pl.beat, PlayMode::Snippet);
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
            Job::Load(path, place) => {
                let pb = PathBuf::from(&path);
                // A duplicate queued before the first finished: reuse it.
                if let Some(&id) = self.imported.get(&pb) {
                    if let Some(pl) = place {
                        self.place_at(id, pl);
                    }
                    self.pump_jobs();
                    return;
                }
                let short = pb.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(path.clone());
                self.busy = Some(format!("loading {short}…"));
                std::thread::spawn(move || {
                    let res = media::load_source(&pb, sr);
                    let _ = tx.send(WorkerMsg::Loaded(pb, place, res));
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

    /// One click: cut `prompt` ("person", "subject", …) out of a clip's
    /// video; when it's done the clip plays the cut-out instead.
    pub(crate) fn cut_out_clip(&mut self, ti: usize, ci: usize, prompt: &str) {
        if let Some(b) = &self.busy {
            self.status = format!("busy — {b}");
            return;
        }
        let src = {
            let p = self.project.lock().unwrap();
            let Some(c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)) else { return };
            if matches!(c.kind, ClipKind::Pattern(_)) {
                return;
            }
            if p.sources.get(c.source).is_none_or(|s| s.video.is_none()) {
                drop(p);
                self.status = "this clip has no video to cut out of".into();
                return;
            }
            c.source
        };
        self.pending_cutout = Some((ti, ci, src));
        self.start_segment(src, prompt.to_string());
    }

    /// One click: paulstretch exactly the bit of media this clip plays
    /// (one pass) by `factor`; the clip then plays the wash, `factor`×
    /// longer (repeats and their count are kept).
    pub(crate) fn stretch_clip(&mut self, ti: usize, ci: usize, factor: f32) {
        if let Some(b) = &self.busy {
            self.status = format!("busy — {b}");
            return;
        }
        let (orig, region) = {
            let p = self.project.lock().unwrap();
            let Some(c) = p.tracks.get(ti).and_then(|t| t.clips.get(ci)) else { return };
            let Some(src) = p.sources.get(c.source) else { return };
            let rate = match c.kind {
                ClipKind::Snippet => c.grains.speed as f64,
                ClipKind::Granular => (c.grains.scan_speed.abs() as f64).max(0.1),
                ClipKind::Pattern(_) => return,
            };
            let dur = src.duration();
            if dur <= 0.0 {
                return;
            }
            let len = (p.beats_to_secs(c.pass_beats()) * rate).clamp(0.25, dur);
            (c.source, src.slice(c.grains.position as f64 * dur, len))
        };
        self.pending_stretch = Some((ti, ci, orig, factor));
        self.busy = Some(format!("paulstretching the clip ×{factor:.0}…"));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = Ok(chromagrain_core::stretch::paulstretch_source(&region, factor));
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
            let p = {
                let mut g = project.lock().unwrap();
                g.resolve_forever();
                g.clone()
            };
            let end = p.song_end_beat().max(1.0);
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
                    if let Some((ti, ci, orig, factor)) = self.pending_stretch.take() {
                        let mut p = self.project.lock().unwrap();
                        if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                            if c.source == orig {
                                let times = c.repeat_count().round().max(1.0) as u32;
                                let pass = (c.pass_beats() * factor as f64).min(64.0);
                                c.source = id;
                                c.grains.position = 0.0;
                                if c.loop_beats.is_some() {
                                    c.loop_beats = Some(pass);
                                    if !c.loop_forever {
                                        c.length_beats = c.length_for(times);
                                    }
                                } else {
                                    c.length_beats = pass;
                                }
                                drop(p);
                                self.select(Sel::Clip(ti, ci));
                                self.mark_now();
                                self.status = format!(
                                    "stretched ×{factor:.0} → '{name}' (original kept in the library)"
                                );
                                continue;
                            }
                        }
                    }
                    if let Some((ti, ci, orig)) = self.pending_cutout.take() {
                        // One-click cut-out: the clip that asked for it
                        // now plays the cut-out; the original stays in
                        // the library.
                        let mut p = self.project.lock().unwrap();
                        if let Some(c) = p.tracks.get_mut(ti).and_then(|t| t.clips.get_mut(ci)) {
                            if c.source == orig && !matches!(c.kind, ClipKind::Pattern(_)) {
                                c.source = id;
                                drop(p);
                                self.select(Sel::Clip(ti, ci));
                                self.mark_now();
                                self.status = format!("cut out → '{name}' (original kept in the library)");
                                continue;
                            }
                        }
                    }
                    self.select(Sel::Source(id));
                    self.mark_now();
                    self.status = if hz > 0.0 {
                        format!("added '{name}' (base pitch ≈ {hz:.1} Hz) — drag it onto a track")
                    } else {
                        format!("added '{name}' — drag it onto a track")
                    };
                }
                WorkerMsg::Source(Err(e)) => {
                    self.pending_cutout = None;
                    self.pending_stretch = None;
                    self.status = format!("couldn't add media: {e}");
                }
                WorkerMsg::Loaded(path, place, res) => {
                    self.loading.remove(&path);
                    if let Some(i) = self.drops.iter().position(|d| d.0 == path) {
                        self.drops.remove(i);
                    }
                    match res {
                        Ok(src) => {
                            let name = src.name.clone();
                            let id = self.project.lock().unwrap().add_source(src);
                            self.imported.insert(path, id);
                            match place {
                                Some(pl) => self.place_at(id, pl),
                                None => {
                                    self.select(Sel::Source(id));
                                    self.status = format!("added '{name}' — drag it onto a track");
                                }
                            }
                            self.mark_now();
                        }
                        Err(e) => self.status = format!("couldn't add {}: {e}", path.display()),
                    }
                }
                WorkerMsg::Script(Ok(log)) => {
                    self.script_log = if log.is_empty() { "(ok)".into() } else { log };
                    self.mark_now();
                    self.status = "script ok".into();
                }
                WorkerMsg::Script(Err(e)) => {
                    self.script_log = format!("ERROR: {e}");
                    self.status = "script error".into();
                }
                WorkerMsg::Take(src, beat) => self.studio_place_take(src, beat),
                WorkerMsg::Export(Ok(path)) => self.status = format!("rendered {path}"),
                WorkerMsg::Export(Err(e)) => self.status = format!("render failed: {e}"),
            }
        }
        self.pump_jobs();
    }

    /// Hand the GPU a consistent (project, plan) pair for this frame.
    fn publish_scene(&mut self) {
        self.frame_no += 1;
        let t = self.plan_project.beats_to_secs(self.playhead_beats);
        *self.scene.lock().unwrap() = Some(gpu::Scene {
            project: self.plan_project.clone(),
            plan: self.plan.clone(),
            t,
            live: self.live_events.clone(),
            live_t: self.app_clock.secs(),
            buses: if self.classic { Vec::new() } else { self.studio.buses() },
            backdrop: if self.classic { None } else { self.studio.backdrop(self) },
            parked: !self.playback.is_playing(),
            frame: self.frame_no,
        });
    }

    pub(crate) fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
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
        let (space, del, dup, esc, home, f) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.modifiers.command && i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Home) || i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::F) && !i.modifiers.command && (self.mode != AppMode::Perform || i.modifiers.shift),
            )
        });
        if space {
            self.toggle_play();
        }
        if f || (esc && self.fullscreen) {
            self.toggle_fullscreen(ctx);
            return;
        }
        if home {
            self.seek(if self.loop_on { self.loop_start } else { 0.0 });
        }
        if esc {
            self.sel = Sel::None;
        }
        let (undo, redo) = ctx.input(|i| {
            let z = i.modifiers.command && i.key_pressed(egui::Key::Z);
            (z && !i.modifiers.shift, (z && i.modifiers.shift) || (i.modifiers.command && i.key_pressed(egui::Key::Y)))
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::O) && !i.modifiers.any()) {
            self.toggle_output();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Tab) && !i.modifiers.any()) {
            let m = if self.mode == AppMode::Perform { AppMode::Arrange } else { AppMode::Perform };
            self.set_mode(m);
            return;
        }
        if self.mode == AppMode::Perform {
            let hit: Vec<usize> = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
                    return Vec::new();
                }
                ui_perform::PAD_KEYS
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| i.key_pressed(*k))
                    .map(|(n, _)| n)
                    .collect()
            });
            for pad in hit {
                self.toggle_pad(pad);
            }
            if del {
                if let Sel::Clip(ti, _) | Sel::Track(ti) = self.sel {
                    self.clear_pad(ti);
                }
            }
            return;
        }
        if let Some((ti, ci)) = self.selected_clip() {
            if del {
                self.delete_clip(ti, ci);
            } else if dup {
                self.duplicate_clip(ti, ci);
            }
        }
        let (copy, cut, paste) = ctx.input(|i| {
            let mut r = (false, false, false);
            for e in &i.events {
                match e {
                    egui::Event::Copy => r.0 = true,
                    egui::Event::Cut => r.1 = true,
                    egui::Event::Paste(_) => r.2 = true,
                    _ => {}
                }
            }
            // With an empty OS clipboard ⌘V arrives as a plain key.
            r.2 |= i.modifiers.command && i.key_pressed(egui::Key::V);
            r
        });
        if let Some((ti, ci)) = self.selected_clip() {
            if copy || cut {
                self.copy_clip(ti, ci);
            }
            if cut {
                self.delete_clip(ti, ci);
            }
        }
        if paste {
            self.paste_clip();
        }
    }
}

/// A fresh clip of `src` in the given play mode.
pub(crate) fn build_clip(p: &mut Project, src: usize, beat: f64, mode: PlayMode) -> Clip {
    let media_beats = p.secs_to_beats(p.sources[src].duration());
    let q = p.quantize.max(0.0625);
    match mode {
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
        let rev0 = self.project_rev;
        let mut bg = 0;
        self.handle_midi();
        self.poll_workers();
        let r = self.project_rev;
        self.synth_update(ctx);
        bg += self.project_rev - r;
        self.browser.poll(ctx);
        if self.classic {
            self.handle_shortcuts(ctx);
        } else {
            // Files dropped from the Finder go to the library.
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            for path in dropped {
                self.import_path(path, None);
            }
            self.with_studio(|st, app| st.update(app, ctx));
        }

        if self.playback.is_playing() {
            let p = self.project.lock().unwrap();
            self.playhead_beats = p.secs_to_beats(self.playback.playhead_secs(&p));
            drop(p);
            ctx.request_repaint();
        } else if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let r = self.project_rev;
        self.perform_update();
        bg += self.project_rev - r;
        let r = self.project_rev;
        self.sync_project();
        bg += self.project_rev - r;
        // History: one step per finished gesture; background jobs don't count.
        let user_changed = self.project_rev - rev0 > bg;
        let busy = ctx.input(|i| i.pointer.any_down()) || ctx.wants_keyboard_input();
        self.history.observe(self.project_rev, user_changed, busy, &self.project);
        if std::mem::take(&mut self.os_copy) {
            ctx.copy_text("chromagrain clip".into());
        }

        let now = self.app_clock.secs();
        self.live_events.retain(|(_, _, e)| e.end() + 0.5 > now);
        if !self.live_events.is_empty() {
            ctx.request_repaint();
        }
        if self.project_rev != self.plan_rev {
            ctx.request_repaint_after(std::time::Duration::from_millis(90));
        }
        self.publish_scene();
        if std::mem::take(&mut self.pending_fullscreen) && !self.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        self.output_ui(ctx);
        if !self.classic {
            self.with_studio(|st, app| st.ui(app, ctx));
            return;
        }

        if self.fullscreen {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(egui::Color32::BLACK))
                .show(ctx, |ui| self.preview_ui(ui));
            return;
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
            .default_width(if self.lib_tab == LibTab::Browse { 380.0 } else { 250.0 })
            .width_range(200.0..=640.0)
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
                // The instrument editor docks at the bottom (resizable) so
                // the preview and timeline shrink to make room for it.
                if let Some(pi) = self.selected_part() {
                    egui::TopBottomPanel::bottom("synth_editor")
                        .resizable(true)
                        .default_height(360.0)
                        .height_range(200.0..=640.0)
                        .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin { top: 8, ..Default::default() }))
                        .show_inside(ui, |ui| {
                            egui::ScrollArea::vertical().id_salt("synth_scroll").show(ui, |ui| {
                                self.synth_editor_ui(ui, pi);
                            });
                        });
                }
                self.preview_ui(ui);
                ui.add_space(8.0);
                if self.mode == AppMode::Perform {
                    self.perform_ui(ui);
                } else {
                    self.timeline_ui(ui);
                }
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

#[cfg(test)]
mod tests {
    use super::pow2_beats;

    #[test]
    fn loop_lengths_snap_to_powers_of_two() {
        assert_eq!(pow2_beats(0.3), 1.0);
        assert_eq!(pow2_beats(3.0), 4.0);
        assert_eq!(pow2_beats(5.0), 4.0);
        assert_eq!(pow2_beats(6.0), 8.0);
        assert_eq!(pow2_beats(100.0), 32.0);
    }
}
