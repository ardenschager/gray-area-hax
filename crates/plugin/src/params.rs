//! Plugin parameters and persisted state.

use chromagrain_core::instrument::{Macro, MacroMap, LOOKS};
use chromagrain_core::music::{Key, ScaleKind};
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
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

#[derive(Params)]
pub struct CgParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
    /// Loaded media, as file paths (decoded again when the state loads).
    #[persist = "sources"]
    pub sources: Mutex<Vec<String>>,

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
            editor_state: EguiState::from_size(560, 420),
            sources: Mutex::new(Vec::new()),
            grain: macro_param(Macro::Grain, "Grain"),
            pitch: FloatParam::new("Pitch", Macro::Pitch.default_value(), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let st = MacroMap::resolve(&[0.0, v, 0.0, 0.0], None).pitch_st;
                    format!("{st:+} st")
                })),
            smear: macro_param(Macro::Smear, "Smear"),
            look: FloatParam::new("Look", Macro::Look.default_value(), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let i = MacroMap::resolve(&[0.0, 0.5, 0.0, v], None).look;
                    LOOKS[i].unwrap_or("none").to_string()
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
