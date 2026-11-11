//! What the studio UI needs from whatever it runs in — the standalone app
//! (project engine, audio device, perform pads, classic editors) or the
//! plugin (an `Instrument` on the host's audio thread). The UI itself only
//! knows this trait.

use crate::gpu::Scene;
use chromagrain_core::fx::AvEffect;
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::timeline::Project;
use std::sync::{Arc, Mutex};

pub use crate::studio::{DeepTab, DeepTarget, View};

/// Which parts of the UI the host supports.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// Views the top bar offers.
    pub views: Vec<View>,
    /// The transport buttons control playback (false in a plugin: the DAW
    /// owns the transport).
    pub transport: bool,
    /// Rec → take bounces.
    pub rec: bool,
    /// Scenes + song chain in Sequence (false: one pattern, looping).
    pub scenes: bool,
    /// Tempo is editable (false in a plugin: host tempo).
    pub tempo: bool,
    /// Deep drawer tabs beyond Links.
    pub deep_tabs: Vec<DeepTab>,
    /// Export button.
    pub export: bool,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            views: View::ALL.to_vec(),
            transport: true,
            rec: true,
            scenes: true,
            tempo: true,
            deep_tabs: vec![DeepTab::Links, DeepTab::Effects, DeepTab::Automation, DeepTab::Script, DeepTab::Library],
            export: true,
        }
    }
}

/// One perform pad, as the UI draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pad {
    pub source: Option<usize>,
    pub kind: PadKind,
    pub len_beats: f64,
    pub on: bool,
    /// A launch waiting for the quantize boundary (true = start).
    pub queued: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Loop,
    Cloud,
    Chop,
}

/// Perform-view services (pads, launch quantize, output routing).
pub trait PerformHost {
    fn pads(&self) -> Vec<Pad>;
    fn toggle_pad(&mut self, i: usize);
    fn load_pad(&mut self, source: usize, i: usize);
    /// 0 = now, 1 = beat, 2 = bar.
    fn quantize(&self) -> usize;
    fn set_quantize(&mut self, q: usize);
    fn stop_all(&mut self);
    fn loop_start(&self) -> f64;
    /// The output window: open?, and a sub-label for the "Live on Display
    /// 2" cover.
    fn output(&self) -> Option<String>;
    fn open_output(&mut self);
    fn close_output(&mut self);
}

pub trait Host {
    /// The document: sources, step patterns, tempo, tracks.
    fn project(&self) -> &Arc<Mutex<Project>>;
    /// Something in the document changed (throttled resync).
    fn mark(&mut self);
    /// A structural change (sync now).
    fn mark_now(&mut self) {
        self.mark();
    }

    fn playing(&self) -> bool;
    fn toggle_play(&mut self);
    fn seek(&mut self, beat: f64);
    fn playhead_beats(&self) -> f64;
    /// Monotonic seconds (live grain onsets are on this clock).
    fn clock(&self) -> f64;

    /// Play one live grain on effect bus `bus` (sound) and show it (picture).
    fn fire(&mut self, bus: u8, source: usize, ev: GrainEvent);
    /// The effect chain of live bus `bus` (1-based).
    fn set_bus(&mut self, bus: u8, chain: Vec<AvEffect>);

    /// The GPU canvas scene (None = no GPU: canvases draw a placeholder).
    fn canvas(&self) -> Option<Arc<Mutex<Option<Scene>>>>;
    /// Canvas pixel size (aspect for letterboxing).
    fn canvas_size(&self) -> (u32, u32);

    fn caps(&self) -> Caps {
        Caps::default()
    }

    /// The song chain changed: lay these (pattern id, start beat, length
    /// beats) out as the arrangement and loop over them. Return false to
    /// be asked again later (e.g. not possible right now).
    fn sequence_changed(&mut self, _layout: &[(usize, f64, f64)]) -> bool {
        true
    }

    /// Entering a view (the app swaps to its perform tracks).
    fn enter_view(&mut self, _v: View) {}

    fn perform(&mut self) -> Option<&mut dyn PerformHost> {
        None
    }

    /// Pick media to add (file dialog).
    fn import_media(&mut self) {}
    fn export(&mut self) {}
    fn fullscreen(&self) -> bool {
        false
    }
    fn toggle_fullscreen(&mut self, _ctx: &egui::Context) {}
    /// `O`: fullscreen output wherever it can't freeze.
    fn toggle_output(&mut self) {}
    fn status(&mut self, _s: String) {}
    /// Bounce recorded live grains (onsets from the take start) into a
    /// take of `len` seconds, recorded at `beat`.
    fn bounce_take(&mut self, _events: Vec<(usize, GrainEvent)>, _len: f64, _beat: f64) {}
    /// Deep drawer tabs other than Links (the app hosts its classic editors).
    fn deep_tab(&mut self, _tab: DeepTab, _target: DeepTarget, _ui: &mut egui::Ui) {}
    /// Busy line (e.g. "bouncing the take…").
    fn busy(&self) -> Option<String> {
        None
    }
}
