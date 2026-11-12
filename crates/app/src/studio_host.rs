//! The standalone app as a studio host: the project engine, the audio
//! device, the perform pads, the output window and the classic editors.

use crate::{build_clip, App, AppMode, LaunchQ, PlayMode, Sel, WorkerMsg, PADS};
use chromagrain_core::fx::AvEffect;
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::timeline::{Clip, ClipKind, Project, Source, Track};
use chromagrain_core::video::VisualGrainStyle;
use chromagrain_ui::gpu::Scene;
use chromagrain_ui::{DeepTab, DeepTarget, Host, Pad, PadKind, PerformHost, Studio, View};
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

impl App {
    /// Run `f` with the studio and the app as its host (the studio is
    /// moved out for the call so both can be borrowed mutably).
    pub(crate) fn with_studio<R>(&mut self, f: impl FnOnce(&mut Studio, &mut App) -> R) -> R {
        let mut st = std::mem::take(&mut self.studio);
        let r = f(&mut st, self);
        self.studio = st;
        r
    }

    /// A bounced take: into the library and onto the "takes" track.
    pub(crate) fn studio_place_take(&mut self, src: Source, beat: f64) {
        let name = src.name.clone();
        let id = self.project.lock().unwrap().add_source(src);
        if self.mode == AppMode::Arrange {
            let mut p = self.project.lock().unwrap();
            let ti = match p.tracks.iter().position(|t| t.name == "takes") {
                Some(t) => t,
                None => p.add_track("takes"),
            };
            let clip = build_clip(&mut p, id, beat.max(0.0), PlayMode::Snippet);
            p.tracks[ti].clips.push(clip);
        }
        self.status = format!("'{name}' recorded — it's on the takes track and in the library");
        self.mark_now();
    }

    /// First visit to Perform: one pad per grid clip (loop), then clouds.
    fn studio_seed_pads(&mut self) {
        if self.mode != AppMode::Arrange || !self.other_tracks.is_empty() {
            return;
        }
        // The grid's columns: the first 7 library clips.
        let n = self.project.lock().unwrap().sources.len();
        let cols: Vec<usize> = (0..n.min(7)).collect();
        if cols.is_empty() {
            return;
        }
        let mut p = self.project.lock().unwrap();
        let mut tracks = Vec::new();
        for i in 0..PADS {
            let mut t = Track { name: format!("pad {}", i + 1), muted: true, ..Default::default() };
            if i < PADS - 1 {
                let src = cols[i % cols.len()];
                let mode = if i < cols.len() { PlayMode::Snippet } else { PlayMode::Grains };
                let mut c = build_clip(&mut p, src, 0.0, mode);
                c.length_beats = crate::pow2_beats(c.length_beats).min(8.0);
                t.clips.push(c);
            }
            tracks.push(t);
        }
        drop(p);
        self.other_tracks = tracks;
    }
}

impl Host for App {
    fn project(&self) -> &Arc<Mutex<Project>> {
        &self.project
    }
    fn mark(&mut self) {
        App::mark(self);
    }
    fn mark_now(&mut self) {
        App::mark_now(self);
    }
    fn playing(&self) -> bool {
        self.playback.is_playing()
    }
    fn toggle_play(&mut self) {
        App::toggle_play(self);
    }
    fn seek(&mut self, beat: f64) {
        App::seek(self, beat);
    }
    fn playhead_beats(&self) -> f64 {
        self.playhead_beats
    }
    fn clock(&self) -> f64 {
        self.app_clock.secs()
    }
    fn fire(&mut self, bus: u8, source: usize, ev: GrainEvent) {
        if self.playback.is_offline() {
            // Frame-locked: hits arrive once per frame with onsets spread
            // over the frame just past; replay that spacing one frame late.
            let delay = ev.onset - self.app_clock.secs() + self.playback.offline_latency;
            self.playback.trigger_bus_in(bus, source, ev.clone(), delay);
        } else {
            self.playback.trigger_bus(bus, source, ev.clone()); // the engine stamps its own onset
        }
        self.live_events.push((source, bus, ev));
    }
    fn set_bus(&mut self, bus: u8, chain: Vec<AvEffect>) {
        self.playback.set_bus(bus, chain);
    }
    fn canvas(&self) -> Option<Arc<Mutex<Option<Scene>>>> {
        (!self.gpu_failed.load(Ordering::Relaxed)).then(|| self.scene.clone())
    }
    fn canvas_size(&self) -> (u32, u32) {
        (self.plan_project.width, self.plan_project.height)
    }
    fn sequence_changed(&mut self, layout: &[(usize, f64, f64)]) -> bool {
        if self.mode != AppMode::Arrange {
            return false;
        }
        let total: f64 = layout.iter().map(|l| l.2).sum();
        {
            let mut p = self.project.lock().unwrap();
            let ti = match p.tracks.iter().position(|t| t.name == "sequence") {
                Some(t) => t,
                None => {
                    p.tracks.insert(0, Track { name: "sequence".into(), ..Default::default() });
                    0
                }
            };
            p.tracks[ti].clips = layout
                .iter()
                .map(|(pid, at, len)| {
                    let mut c = Clip::new_pattern(*pid, *at, *len);
                    // Hits glow in with soft edges instead of hard blocks.
                    c.visual.feather = 0.3;
                    c.visual.additive = 0.3;
                    c
                })
                .collect();
        }
        self.loop_on = true;
        self.loop_start = 0.0;
        self.loop_end = total;
        self.playback.set_loop(self.loop_region());
        App::mark_now(self);
        true
    }
    fn enter_view(&mut self, v: View) {
        if v == View::Perform {
            self.studio_seed_pads();
            self.set_mode(AppMode::Perform);
        } else if self.mode == AppMode::Perform {
            self.set_mode(AppMode::Arrange);
        }
    }
    fn perform(&mut self) -> Option<&mut dyn PerformHost> {
        Some(self)
    }
    fn import_media(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.import_path(path, None);
        }
    }
    fn export(&mut self) {
        self.start_render(false);
    }
    fn fullscreen(&self) -> bool {
        self.fullscreen
    }
    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        App::toggle_fullscreen(self, ctx);
    }
    fn toggle_output(&mut self) {
        App::toggle_output(self);
    }
    fn status(&mut self, s: String) {
        self.status = s;
    }
    fn bounce_take(&mut self, events: Vec<(usize, GrainEvent)>, len: f64, beat: f64) {
        let (sources, w, h, fps, sr) = {
            let p = self.project.lock().unwrap();
            (p.sources.clone(), p.width, p.height, p.fps, p.sample_rate)
        };
        let n = sources.len();
        let tx = self.tx.clone();
        self.busy = Some("bouncing the take…".into());
        std::thread::spawn(move || {
            let src = chromagrain_core::take::bounce(&format!("take {}", n + 1), &sources, &events, len, w, h, fps, sr, &VisualGrainStyle::default());
            let _ = tx.send(WorkerMsg::Take(src, beat));
        });
    }
    fn deep_tab(&mut self, tab: DeepTab, _target: DeepTarget, ui: &mut egui::Ui) {
        match tab {
            DeepTab::Links => {}
            DeepTab::Effects => {
                let sel = self.sel;
                self.sel = Sel::Master;
                self.inspector_ui(ui);
                if self.sel == Sel::Master {
                    self.sel = sel;
                }
            }
            DeepTab::Automation => {
                self.timeline_ui(ui);
                if self.selected_clip().is_some() {
                    self.inspector_ui(ui);
                }
            }
            DeepTab::Script => self.script_ui(ui),
            DeepTab::Library => self.library_ui(ui),
        }
    }
    fn busy(&self) -> Option<String> {
        self.busy.clone()
    }
}

impl PerformHost for App {
    fn pads(&self) -> Vec<Pad> {
        let p = self.project.lock().unwrap();
        (0..PADS)
            .map(|i| {
                let t = p.tracks.get(i);
                let clip = t.and_then(|t| t.clips.first());
                let (source, kind, len) = match clip {
                    Some(c) => {
                        let (s, k) = match c.kind {
                            ClipKind::Snippet => (Some(c.source), PadKind::Loop),
                            ClipKind::Granular => (Some(c.source), PadKind::Cloud),
                            ClipKind::Pattern(pid) => (p.patterns.get(pid).and_then(|pat| pat.rows.first()).map(|r| r.source), PadKind::Chop),
                        };
                        (s, k, c.length_beats)
                    }
                    None => (None, PadKind::Loop, 4.0),
                };
                Pad {
                    source,
                    // A named pad track (not the default "pad N") names the pad.
                    name: t.filter(|t| !t.name.starts_with("pad ") && !t.name.is_empty()).map(|t| t.name.clone()),
                    kind,
                    len_beats: len,
                    on: t.is_some_and(|t| !t.muted) && source.is_some(),
                    queued: self.pending.iter().find(|(pi, _)| *pi == i).map(|(_, on)| *on),
                }
            })
            .collect()
    }
    fn toggle_pad(&mut self, i: usize) {
        App::toggle_pad(self, i);
    }
    fn load_pad(&mut self, source: usize, i: usize) {
        App::load_pad(self, source, i, PlayMode::Snippet);
    }
    fn quantize(&self) -> usize {
        match self.launch_q {
            LaunchQ::Now => 0,
            LaunchQ::Beat => 1,
            LaunchQ::Bar => 2,
        }
    }
    fn set_quantize(&mut self, q: usize) {
        self.launch_q = [LaunchQ::Now, LaunchQ::Beat, LaunchQ::Bar][q.min(2)];
    }
    fn stop_all(&mut self) {
        App::stop_all(self);
    }
    fn loop_start(&self) -> f64 {
        self.loop_start
    }
    fn output(&self) -> Option<String> {
        self.output.open.then(|| match self.output.target {
            Some(d) => format!("{:.0}×{:.0} · no chrome", d.size[0], d.size[1]),
            None => "output window · no chrome".into(),
        })
    }
    fn open_output(&mut self) {
        self.open_output_window();
    }
    fn close_output(&mut self) {
        if self.output.open {
            App::toggle_output(self);
        }
    }
}
