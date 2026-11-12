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

/// Ways to add media (the Add sheet's tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddRoute {
    /// Pick files (a dialog, or paths injected by tests / the showcase).
    Files,
    /// Every video on this machine (desktop).
    Browse,
    /// A YouTube / yt-dlp URL (desktop).
    Link,
    /// A random, rarely seen video (desktop).
    Dig,
}

impl AddRoute {
    pub fn label(self) -> &'static str {
        match self {
            AddRoute::Files => "Files",
            AddRoute::Browse => "Browse",
            AddRoute::Link => "Link",
            AddRoute::Dig => "Dig",
        }
    }
}

/// Where added media should land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTarget {
    /// The next empty grid column.
    Any,
    Column(usize),
    Pad(usize),
    Lane(usize),
}

/// One thing in the Browse tab.
#[derive(Debug, Clone)]
pub struct BrowseItem {
    pub name: String,
    /// e.g. "0:42 · Movies".
    pub detail: String,
    pub thumb: Option<egui::TextureId>,
}

/// An add still under way (a placeholder tile until it lands).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: u64,
    pub target: AddTarget,
    /// "digging…", "downloading", "cutting", a file name.
    pub label: String,
    /// 0..1 when known.
    pub progress: Option<f32>,
    /// Failed: why (the tile shows it until dismissed or retried).
    pub failed: Option<String>,
}

/// Media that finished arriving: the source index and where it should go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Added {
    pub source: usize,
    pub target: AddTarget,
}

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
    /// Perform's output routing (a second window on another display).
    pub output: bool,
    /// Add-sheet tabs (empty: the "+" buttons call `import_media`).
    pub add: Vec<AddRoute>,
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
            output: true,
            add: vec![AddRoute::Files, AddRoute::Browse, AddRoute::Link, AddRoute::Dig],
        }
    }
}

/// One perform pad, as the UI draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub source: Option<usize>,
    /// Display name (None = the source's name).
    pub name: Option<String>,
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
    /// Load seconds `t0..t1` of a source (a slice) onto pad `i`.
    fn load_pad_slice(&mut self, source: usize, _t0: f64, _t1: f64, i: usize) {
        self.load_pad(source, i);
    }
    /// A pad's A/D/S/R (None: the default envelope).
    fn pad_env(&self, _i: usize) -> Option<chromagrain_core::envelope::Adsr> {
        None
    }
    fn set_pad_env(&mut self, _i: usize, _adsr: chromagrain_core::envelope::Adsr) {}
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
    /// The Add sheet's Files tab (default: `import_media`).
    fn add_files(&mut self, _target: AddTarget) {
        self.import_media();
    }
    /// The Browse tab's items for `query` (thumbnails may fill in later).
    fn browse(&mut self, _ctx: &egui::Context, _query: &str) -> Vec<BrowseItem> {
        Vec::new()
    }
    /// Add Browse item `index` (of the last `browse` list).
    fn browse_add(&mut self, _index: usize, _target: AddTarget) {}
    fn add_link(&mut self, _url: &str, _target: AddTarget) {}
    /// Dig `n` random, rarely seen videos (`cc_only`: Creative Commons).
    fn dig(&mut self, _n: usize, _cc_only: bool, _target: AddTarget) {}
    /// Adds under way (placeholder tiles).
    fn pending(&self) -> Vec<Pending> {
        Vec::new()
    }
    /// Forget a failed add.
    fn dismiss_pending(&mut self, _id: u64) {}
    /// Media that landed since the last call, with its target.
    fn take_added(&mut self) -> Vec<Added> {
        Vec::new()
    }
    /// Hear (and see) seconds `t0..t1` of a source once, now.
    fn audition(&mut self, source: usize, t0: f64, t1: f64) {
        let ev = GrainEvent {
            onset: self.clock(),
            source_pos: t0,
            duration: (t1 - t0).max(0.01) as f32,
            pitch_ratio: 1.0,
            gain: 0.9,
            pan: 0.0,
            envelope: 0.04,
            env_skew: 0.0,
            reverse: false,
            id: 0xA0D1_0000_0000 ^ (t0 * 1000.0) as u64,
            tag: Default::default(),
        };
        self.fire(0, source, ev);
    }
    /// Stop an audition (where the host can).
    fn stop_audition(&mut self) {}
    /// A source's slices changed in the document (the plugin forwards
    /// them to its instrument).
    fn slices_changed(&mut self, _source: usize) {}
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
    /// Macros owned by the host (a plugin's automatable GRAIN / PITCH /
    /// SMEAR / LOOK parameters): every emitter shows and edits these.
    fn shared_macros(&self) -> Option<[f32; 4]> {
        None
    }
    fn set_shared_macro(&mut self, _i: usize, _v: f32) {}
    /// The global key, when the host owns it (plugin key / scale params).
    fn shared_key(&self) -> Option<chromagrain_core::music::Key> {
        None
    }
    fn set_shared_key(&mut self, _key: chromagrain_core::music::Key) {}
    /// The canvas layout: `Some(false)` stack (layered), `Some(true)` tile
    /// (every sounding voice its own tile); None = no layout switch.
    fn tile_layout(&self) -> Option<bool> {
        None
    }
    fn set_tile_layout(&mut self, _tile: bool) {}
    /// Busy line (e.g. "bouncing the take…").
    fn busy(&self) -> Option<String> {
        None
    }
}
