//! Plugin parameters and persisted state.

use chromagrain_core::instrument::{Macro, MacroMap, LOOKS};
use chromagrain_core::music::{Key, ScaleKind};
use nih_plug::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    #[name = "off (chromatic)"]
    Off,
    C,
    #[name = "C#"]
    Cs,
    D,
    #[name = "D#"]
    Ds,
    E,
    F,
    #[name = "F#"]
    Fs,
    G,
    #[name = "G#"]
    Gs,
    A,
    #[name = "A#"]
    As,
    B,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Major,
    Minor,
    #[name = "harmonic minor"]
    HarmonicMinor,
    #[name = "major pentatonic"]
    MajorPentatonic,
    #[name = "minor pentatonic"]
    MinorPentatonic,
    Blues,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    #[name = "whole tone"]
    WholeTone,
    Chromatic,
}

impl Scale {
    pub fn kind(self) -> ScaleKind {
        match self {
            Scale::Major => ScaleKind::Major,
            Scale::Minor => ScaleKind::Minor,
            Scale::HarmonicMinor => ScaleKind::HarmonicMinor,
            Scale::MajorPentatonic => ScaleKind::MajorPentatonic,
            Scale::MinorPentatonic => ScaleKind::MinorPentatonic,
            Scale::Blues => ScaleKind::Blues,
            Scale::Dorian => ScaleKind::Dorian,
            Scale::Phrygian => ScaleKind::Phrygian,
            Scale::Lydian => ScaleKind::Lydian,
            Scale::Mixolydian => ScaleKind::Mixolydian,
            Scale::WholeTone => ScaleKind::WholeTone,
            Scale::Chromatic => ScaleKind::Chromatic,
        }
    }
}

/// Beat grid for grain onsets.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDiv {
    #[name = "free"]
    Free,
    #[name = "1 bar"]
    Bar,
    #[name = "1/2"]
    Half,
    #[name = "1/4"]
    Quarter,
    #[name = "1/8"]
    Eighth,
    #[name = "1/16"]
    Sixteenth,
    #[name = "1/32"]
    ThirtySecond,
}

impl SyncDiv {
    /// Grid size in beats (0 = free).
    pub fn beats(self) -> f32 {
        match self {
            SyncDiv::Free => 0.0,
            SyncDiv::Bar => 4.0,
            SyncDiv::Half => 2.0,
            SyncDiv::Quarter => 1.0,
            SyncDiv::Eighth => 0.5,
            SyncDiv::Sixteenth => 0.25,
            SyncDiv::ThirtySecond => 0.125,
        }
    }
}

/// Perform pads' launch grid.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Now,
    Beat,
    Bar,
}

impl Launch {
    pub fn beats(self) -> f64 {
        match self {
            Launch::Now => 0.0,
            Launch::Beat => 1.0,
            Launch::Bar => 4.0,
        }
    }
}

/// The canvas layout (see `chromagrain_core::tile`).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutParam {
    Stack,
    Tile,
}

impl LayoutParam {
    pub fn layout(self) -> chromagrain_core::tile::Layout {
        match self {
            LayoutParam::Stack => chromagrain_core::tile::Layout::Stack,
            LayoutParam::Tile => chromagrain_core::tile::Layout::Tile,
        }
    }
}

/// A source's chops in the plugin state: (source index in load order,
/// slices as (start, end) seconds).
pub type PersistedSlices = (usize, Vec<(f64, f64)>);
/// Per-pad extras: (pad, slice (start, end) seconds, envelope).
pub type PersistedPadExtra = (usize, Option<(f64, f64)>, Option<chromagrain_core::envelope::Adsr>);

/// One loaded perform pad in the plugin state: (pad, source index in
/// load order, start 0..1, length in beats).
pub type PersistedPad = (usize, usize, f32, f32);

#[derive(Params)]
pub struct CgParams {
    /// Editor window size (logical points), remembered with the session.
    #[persist = "editor-size"]
    pub editor_size: Arc<Mutex<(u32, u32)>>,
    /// Loaded media, as file paths (decoded again when the state loads).
    #[persist = "sources"]
    pub sources: Mutex<Vec<String>>,
    /// Loaded perform pads.
    #[persist = "pads"]
    pub pads: Mutex<Vec<PersistedPad>>,
    /// Slices / envelopes of those pads.
    #[persist = "pad-extra"]
    pub pad_extra: Mutex<Vec<PersistedPadExtra>>,
    /// Chops made in the editor.
    #[persist = "slices"]
    pub slices: Mutex<Vec<PersistedSlices>>,

    #[id = "grain"]
    pub grain: FloatParam,
    #[id = "pitch"]
    pub pitch: FloatParam,
    #[id = "smear"]
    pub smear: FloatParam,
    #[id = "look"]
    pub look: FloatParam,
    #[id = "root"]
    pub root: EnumParam<Root>,
    #[id = "scale"]
    pub scale: EnumParam<Scale>,
    #[id = "sync"]
    pub sync: EnumParam<SyncDiv>,
    #[id = "follow"]
    pub follow: BoolParam,
    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "launch"]
    pub launch: EnumParam<Launch>,
    #[id = "layout"]
    pub layout: EnumParam<LayoutParam>,
    /// Tile column -> pan, row -> level (tile layout).
    #[id = "layout-pan"]
    pub layout_pan: BoolParam,
}

fn macro_param(m: Macro, name: &str) -> FloatParam {
    FloatParam::new(name, m.default_value(), FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
        .with_unit("%")
}

impl Default for CgParams {
    fn default() -> Self {
        Self {
            editor_size: Arc::new(Mutex::new(crate::editor::DEFAULT_SIZE)),
            sources: Mutex::new(Vec::new()),
            pads: Mutex::new(Vec::new()),
            pad_extra: Mutex::new(Vec::new()),
            slices: Mutex::new(Vec::new()),
            grain: macro_param(Macro::Grain, "Grain"),
            pitch: FloatParam::new("Pitch", Macro::Pitch.default_value(), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let st = MacroMap::resolve(&[0.0, v, 0.0, 0.0], None).pitch_st;
                    format!("{st:+} st")
                }))
                .with_string_to_value(Arc::new(|s| {
                    let st: f32 = s.trim().trim_end_matches("st").trim().trim_start_matches('+').parse().ok()?;
                    Some(((st + 12.0) / 24.0).clamp(0.0, 1.0))
                })),
            smear: macro_param(Macro::Smear, "Smear"),
            look: FloatParam::new("Look", Macro::Look.default_value(), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let i = MacroMap::resolve(&[0.0, 0.5, 0.0, v], None).look;
                    LOOKS[i].unwrap_or("none").to_string()
                }))
                .with_string_to_value(Arc::new(|s| {
                    let s = s.trim();
                    let i = LOOKS.iter().position(|l| l.unwrap_or("none").eq_ignore_ascii_case(s))?;
                    Some((i as f32 + 0.5) / LOOKS.len() as f32)
                })),
            root: EnumParam::new("Key", Root::Off),
            scale: EnumParam::new("Scale", Scale::Minor),
            sync: EnumParam::new("Sync", SyncDiv::Free),
            follow: BoolParam::new("Follow transport", false),
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed { min: util::db_to_gain(-36.0), max: util::db_to_gain(12.0), factor: FloatRange::gain_skew_factor(-36.0, 12.0) },
            )
            .with_smoother(SmoothingStyle::Logarithmic(30.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            launch: EnumParam::new("Pad launch", Launch::Beat),
            layout: EnumParam::new("Layout", LayoutParam::Stack),
            layout_pan: BoolParam::new("Tile pan", true),
        }
    }
}

impl CgParams {
    /// The global key the key/scale params describe (None = chromatic).
    pub fn key(&self) -> Option<Key> {
        let root = self.root.value();
        (root != Root::Off).then(|| Key::new(root as i32 - 1, self.scale.value().kind()))
    }
}
