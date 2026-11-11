//! The studio UI inside the plugin: a [`Host`] backed by the plugin's
//! audio side (the `Instrument` on the host's audio thread), not by the
//! app's project engine.
//!
//! v1 scope (what the plugin exposes):
//! * **Play** — the clip grid and the four emitters. Emitter grains are
//!   computed in the editor with the same `grain_step` / `MacroMap` the
//!   instrument uses and sent straight to the audio thread as one-shots
//!   (`engine::Live` ring -> `Instrument::trigger`, next block); they sound through the instrument's SMEAR +
//!   LOOK. The four knobs ARE the plugin's GRAIN / PITCH / SMEAR / LOOK
//!   parameters (automatable; every emitter shares them in v1).
//! * **Sequence** — one scene of step lanes (slice / pitch / length /
//!   chance, polymeter) played by the instrument in sync with the host
//!   transport (`Instrument::swap_pattern`). Scenes, the song chain and
//!   the arrangement are the DAW's job inside a plugin.
//! * Not in v1: Perform (pads, output window), Rec takes, export, tempo /
//!   transport control (the host owns them), the classic editors.

use crate::engine::{Job, Live, Shared};
use crate::params::{CgParams, Root, Scale};
use chromagrain_core::fx::AvEffect;
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::render::{plan_project, ClipPlan};
use chromagrain_core::timeline::{Clip, Project, Track};
use chromagrain_ui::gpu::Scene;
use chromagrain_ui::{Caps, DeepTab, Host, Studio, View};
use nih_plug::prelude::{Enum, GuiContext, ParamSetter};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// One bar of 16 steps: what the Sequence lanes span.
const PATTERN_BEATS: f64 = 4.0;

pub struct PluginHost {
    pub project: Arc<Mutex<Project>>,
    pub shared: Arc<Shared>,
    pub params: Arc<CgParams>,
    /// Set inside a real editor (parameter edits go through the host);
    /// None offline (tests keep the edits locally).
    pub gui: Option<Arc<dyn GuiContext>>,
    macro_override: [Option<f32>; 4],
    key_override: Option<Key>,
    pub scene: Arc<Mutex<Option<Scene>>>,
    pub gpu: bool,
    pub live: Vec<(usize, u8, GrainEvent)>,
    clock: Instant,
    doc_rev: u64,
    sent_rev: u64,
    plan_rev: u64,
    plan: Arc<Vec<ClipPlan>>,
    seq_pid: Option<usize>,
    frame: u64,
}

impl PluginHost {
    pub fn new(shared: Arc<Shared>, params: Arc<CgParams>, gui: Option<Arc<dyn GuiContext>>) -> PluginHost {
        PluginHost {
            project: Arc::new(Mutex::new(Project::default())),
            shared,
            params,
            gui,
            macro_override: [None; 4],
            key_override: None,
            scene: Arc::new(Mutex::new(None)),
            gpu: true,
            live: Vec::new(),
            clock: Instant::now(),
            doc_rev: 1,
            sent_rev: 0,
            plan_rev: 0,
            plan: Arc::new(Vec::new()),
            seq_pid: None,
            frame: 0,
        }
    }

    /// Pull what the audio side and loader know into the document; push
    /// edited step lanes to the audio thread.
    pub fn sync(&mut self) {
        let srcs = self.shared.ui_sources.lock().map(|s| s.clone()).unwrap_or_default();
        {
            let mut p = self.project.lock().unwrap();
            let changed = p.sources.len() != srcs.len()
                || p.sources.iter().zip(&srcs).any(|(a, b)| {
                    a.video.as_ref().map(Arc::as_ptr) != b.video.as_ref().map(Arc::as_ptr)
                        || a.audio.as_ref().map(Arc::as_ptr) != b.audio.as_ref().map(Arc::as_ptr)
                });
            if changed {
                p.sources = srcs;
                self.doc_rev += 1;
            }
            let bpm = self.shared.bpm();
            if bpm > 0.0 && (p.bpm - bpm).abs() > 1e-9 {
                p.bpm = bpm;
                self.doc_rev += 1;
            }
        }
        if self.doc_rev != self.sent_rev {
            if let Some(pat) = self.seq_pid.and_then(|pid| self.project.lock().unwrap().patterns.get(pid).cloned()) {
                self.shared.send(Job::Pattern(Box::new(pat)));
            }
            self.sent_rev = self.doc_rev;
        }
        let now = self.clock.elapsed().as_secs_f64();
        self.live.retain(|(_, _, e)| e.end() + 0.5 > now);
    }

    /// Hand the GPU this frame's picture.
    pub fn publish(&mut self, studio: &Studio) {
        let doc = self.project.lock().unwrap().clone();
        if self.plan_rev != self.doc_rev {
            self.plan = Arc::new(plan_project(&doc));
            self.plan_rev = self.doc_rev;
        }
        self.frame += 1;
        let beat = self.playhead_beats().rem_euclid(PATTERN_BEATS);
        let scene = Scene {
            t: doc.beats_to_secs(beat),
            plan: self.plan.clone(),
            live: self.live.clone(),
            live_t: self.clock(),
            buses: studio.buses(),
            backdrop: studio.backdrop(self),
            parked: !self.playing(),
            frame: self.frame,
            project: doc,
        };
        *self.scene.lock().unwrap() = Some(scene);
    }

    fn with_setter(&self, f: impl FnOnce(&ParamSetter)) {
        if let Some(gui) = self.gui.as_ref() {
            f(&ParamSetter::new(gui.as_ref()));
        }
    }
}

fn root_of(pc: i32) -> Root {
    Root::from_index(pc.rem_euclid(12) as usize + 1)
}

fn scale_of(s: ScaleKind) -> Scale {
    match s {
        ScaleKind::Major => Scale::Major,
        ScaleKind::Minor => Scale::Minor,
        ScaleKind::HarmonicMinor => Scale::HarmonicMinor,
        ScaleKind::MajorPentatonic => Scale::MajorPentatonic,
        ScaleKind::MinorPentatonic => Scale::MinorPentatonic,
        ScaleKind::Blues => Scale::Blues,
        ScaleKind::Dorian => Scale::Dorian,
        ScaleKind::Phrygian => Scale::Phrygian,
        ScaleKind::Lydian => Scale::Lydian,
        ScaleKind::Mixolydian => Scale::Mixolydian,
        ScaleKind::WholeTone => Scale::WholeTone,
        ScaleKind::Chromatic => Scale::Chromatic,
    }
}

impl Host for PluginHost {
    fn project(&self) -> &Arc<Mutex<Project>> {
        &self.project
    }
    fn mark(&mut self) {
        self.doc_rev += 1;
    }
    fn playing(&self) -> bool {
        self.shared.playing()
    }
    fn toggle_play(&mut self) {}
    fn seek(&mut self, _beat: f64) {}
    fn playhead_beats(&self) -> f64 {
        self.shared.beat()
    }
    fn clock(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }
    fn fire(&mut self, bus: u8, source: usize, ev: GrainEvent) {
        self.shared.live(Live::Fire(source, ev.clone()));
        self.live.push((source, bus, ev));
    }
    fn set_bus(&mut self, _bus: u8, _chain: Vec<AvEffect>) {
        // The instrument's SMEAR + LOOK follow the macro parameters.
    }
    fn canvas(&self) -> Option<Arc<Mutex<Option<Scene>>>> {
        self.gpu.then(|| self.scene.clone())
    }
    fn canvas_size(&self) -> (u32, u32) {
        let p = self.project.lock().unwrap();
        (p.width, p.height)
    }
    fn caps(&self) -> Caps {
        Caps {
            views: vec![View::Play, View::Sequence],
            transport: false,
            rec: false,
            scenes: false,
            tempo: false,
            deep_tabs: vec![DeepTab::Links],
            export: false,
        }
    }
    fn sequence_changed(&mut self, layout: &[(usize, f64, f64)]) -> bool {
        // One looping scene: the picture of the lanes, and what the
        // instrument plays.
        let Some(&(pid, _, _)) = layout.first() else { return false };
        let mut p = self.project.lock().unwrap();
        let ti = match p.tracks.iter().position(|t| t.name == "sequence") {
            Some(t) => t,
            None => {
                p.tracks.push(Track { name: "sequence".into(), ..Default::default() });
                p.tracks.len() - 1
            }
        };
        p.tracks[ti].clips = vec![Clip::new_pattern(pid, 0.0, PATTERN_BEATS)];
        drop(p);
        self.seq_pid = Some(pid);
        self.doc_rev += 1;
        true
    }
    fn import_media(&mut self) {
        let pick = rfd::FileDialog::new()
            .add_filter("media", &["mp4", "mov", "m4v", "webm", "mkv", "wav", "mp3", "m4a", "aif", "aiff"])
            .pick_file();
        if let Some(path) = pick {
            self.shared.send(Job::Load(path));
        }
    }
    fn status(&mut self, s: String) {
        self.shared.set_status(s);
    }
    fn busy(&self) -> Option<String> {
        let s = self.shared.status();
        s.ends_with('…').then_some(s)
    }
    fn shared_macros(&self) -> Option<[f32; 4]> {
        let p = &self.params;
        let v = [p.grain.value(), p.pitch.value(), p.smear.value(), p.look.value()];
        Some(std::array::from_fn(|i| self.macro_override[i].unwrap_or(v[i])))
    }
    fn set_shared_macro(&mut self, i: usize, v: f32) {
        let params = self.params.clone();
        let param = match i {
            0 => &params.grain,
            1 => &params.pitch,
            2 => &params.smear,
            _ => &params.look,
        };
        if self.gui.is_some() {
            self.with_setter(|s| {
                s.begin_set_parameter(param);
                s.set_parameter(param, v);
                s.end_set_parameter(param);
            });
        } else {
            self.macro_override[i.min(3)] = Some(v);
        }
    }
    fn shared_key(&self) -> Option<Key> {
        self.key_override.or_else(|| self.params.key())
    }
    fn set_shared_key(&mut self, key: Key) {
        if self.gui.is_some() {
            let params = self.params.clone();
            self.with_setter(|s| {
                s.begin_set_parameter(&params.root);
                s.set_parameter(&params.root, root_of(key.root));
                s.end_set_parameter(&params.root);
                s.begin_set_parameter(&params.scale);
                s.set_parameter(&params.scale, scale_of(key.scale));
                s.end_set_parameter(&params.scale);
            });
        } else {
            self.key_override = Some(key);
        }
    }
}

/// The editor's per-frame driver: the studio against the plugin host.
/// The real editor window and the headless tests both run exactly this.
pub struct PluginUi {
    pub studio: Studio,
    pub host: PluginHost,
}

impl PluginUi {
    pub fn new(shared: Arc<Shared>, params: Arc<CgParams>, gui: Option<Arc<dyn GuiContext>>) -> PluginUi {
        PluginUi { studio: Studio::default(), host: PluginHost::new(shared, params, gui) }
    }

    pub fn frame(&mut self, ctx: &egui::Context) {
        self.host.sync();
        self.studio.update(&mut self.host, ctx);
        self.host.publish(&self.studio);
        self.studio.ui(&mut self.host, ctx);
        // Plugin editors redraw continuously (meters, the host's playhead).
        ctx.request_repaint();
    }
}
