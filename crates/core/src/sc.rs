//! SuperCollider inside chromagrain.
//!
//! Instruments are real SuperCollider SynthDefs (`assets/synthdefs/*.scd`,
//! compiled by sclang to `.scsyndef` and embedded here) run by
//! [plyphon](https://github.com/nannou-org/plyphon), a pure-Rust scsynth.
//! No separate server, no audio routing: a [`SynthPart`] (an instrument,
//! its knob settings and a few bars of notes) is rendered straight into
//! an [`AudioClip`], which then behaves exactly like any other media —
//! snippets, grains, patterns, pads, effects, visuals and export all
//! apply, and the export is deterministic.
//!
//! Parts are rendered as seamless loops: whatever rings past the end
//! (release tails, reverb in the def) is folded back onto the start, so a
//! looping pad or pad-mode clip never clicks or drops its tail.

use crate::audio::AudioClip;
use plyphon::{engine, AddAction, ControllerBatchCommand, Options, ROOT_GROUP_ID};

/// One knob on an instrument.
#[derive(Debug, Clone, Copy)]
pub struct Knob {
    /// Control name in the SynthDef. Drum-kit knobs are `drum.control`.
    pub name: &'static str,
    pub label: &'static str,
    pub lo: f32,
    pub hi: f32,
    pub default: f32,
    pub log: bool,
    pub tip: &'static str,
}

const fn k(name: &'static str, label: &'static str, lo: f32, hi: f32, default: f32, log: bool, tip: &'static str) -> Knob {
    Knob { name, label, lo, hi, default, log, tip }
}

/// A playable instrument from the library.
pub struct Instrument {
    /// Id stored in parts ("cg_pad", or "kit" for the drum kit).
    pub id: &'static str,
    pub label: &'static str,
    pub blurb: &'static str,
    pub knobs: &'static [Knob],
    /// Output trim so every instrument sits at a similar loudness.
    pub trim: f32,
    /// Starter part: (length in beats, notes).
    pub starter: fn() -> (f64, Vec<Note>),
    /// Drum kit: pitch picks the drum (see [`KIT`]).
    pub kit: bool,
}

/// Drum kit mapping (General-MIDI-ish): pitch -> (def, freq Hz, label).
pub const KIT: &[(u8, &str, f32, &str)] = &[
    (36, "cg_kick", 50.0, "kick"),
    (38, "cg_snare", 190.0, "snare"),
    (42, "cg_hat", 8000.0, "hat"),
    (46, "cg_hat", 6500.0, "open hat"),
];

fn defs_bytes(def: &str) -> Option<&'static [u8]> {
    Some(match def {
        "cg_pad" => include_bytes!("../../../assets/synthdefs/cg_pad.scsyndef"),
        "cg_keys" => include_bytes!("../../../assets/synthdefs/cg_keys.scsyndef"),
        "cg_pluck" => include_bytes!("../../../assets/synthdefs/cg_pluck.scsyndef"),
        "cg_bass" => include_bytes!("../../../assets/synthdefs/cg_bass.scsyndef"),
        "cg_acid" => include_bytes!("../../../assets/synthdefs/cg_acid.scsyndef"),
        "cg_bell" => include_bytes!("../../../assets/synthdefs/cg_bell.scsyndef"),
        "cg_lead" => include_bytes!("../../../assets/synthdefs/cg_lead.scsyndef"),
        "cg_choir" => include_bytes!("../../../assets/synthdefs/cg_choir.scsyndef"),
        "cg_drone" => include_bytes!("../../../assets/synthdefs/cg_drone.scsyndef"),
        "cg_kick" => include_bytes!("../../../assets/synthdefs/cg_kick.scsyndef"),
        "cg_snare" => include_bytes!("../../../assets/synthdefs/cg_snare.scsyndef"),
        "cg_hat" => include_bytes!("../../../assets/synthdefs/cg_hat.scsyndef"),
        _ => return None,
    })
}

// ------------------------------------------------------------ starters
// Everything defaults to A minor, i–VI–III–VII (Am F C G), so parts made
// from different instruments fit together out of the box.

const CHORDS: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [55, 60, 64], [55, 59, 62]];
const ROOTS: [u8; 4] = [45, 41, 48, 43];

fn n(start: f64, len: f64, pitch: u8, vel: f32) -> Note {
    Note { start, len, pitch, vel }
}

fn starter_pad() -> (f64, Vec<Note>) {
    let mut v = Vec::new();
    for (bar, c) in CHORDS.iter().enumerate() {
        let t = bar as f64 * 4.0;
        for &p in c {
            v.push(n(t, 4.0, p, 0.7));
        }
        v.push(n(t, 4.0, ROOTS[bar] - 12, 0.55));
    }
    (16.0, v)
}

fn starter_keys() -> (f64, Vec<Note>) {
    let mut v = Vec::new();
    for (bar, c) in CHORDS.iter().enumerate() {
        let t = bar as f64 * 4.0;
        for (off, len, vel) in [(0.0, 1.4, 0.8), (1.5, 1.0, 0.6), (3.0, 0.9, 0.65)] {
            for &p in c {
                v.push(n(t + off, len, p, vel));
            }
        }
    }
    (16.0, v)
}

fn starter_pluck() -> (f64, Vec<Note>) {
    let shape = [0usize, 1, 2, 1, 2, 0, 1, 2];
    let mut v = Vec::new();
    for step in 0..64 {
        let bar = step / 16;
        let c = CHORDS[bar];
        let i = shape[step % shape.len()];
        let oct = if step % 8 >= 4 { 24 } else { 12 };
        let vel = if step % 4 == 0 { 0.85 } else { 0.55 };
        v.push(n(step as f64 * 0.25, 0.3, c[i] + oct - 12, vel));
    }
    (16.0, v)
}

fn starter_bass() -> (f64, Vec<Note>) {
    let mut v = Vec::new();
    for (bar, &r) in ROOTS.iter().enumerate() {
        let t = bar as f64 * 4.0;
        for (off, len, oct, vel) in [(0.0, 0.6, 0, 0.9), (0.75, 0.3, 0, 0.6), (1.5, 0.4, 12, 0.7), (2.0, 0.6, 0, 0.85), (2.75, 0.3, 0, 0.6), (3.5, 0.4, 7, 0.7)] {
            v.push(n(t + off, len, r - 12 + oct, vel));
        }
    }
    (16.0, v)
}

fn starter_acid() -> (f64, Vec<Note>) {
    let line: [Option<(u8, f32)>; 16] = [
        Some((33, 1.0)), Some((33, 0.6)), Some((45, 0.8)), None,
        Some((36, 0.7)), Some((33, 0.6)), Some((43, 1.0)), Some((33, 0.6)),
        Some((33, 0.7)), Some((45, 1.0)), None, Some((40, 0.7)),
        Some((33, 0.6)), Some((36, 0.9)), Some((31, 0.7)), Some((33, 0.6)),
    ];
    let v = line
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.map(|(p, vel)| n(i as f64 * 0.25, 0.2, p, vel)))
        .collect();
    (4.0, v)
}

fn starter_bell() -> (f64, Vec<Note>) {
    (16.0, vec![n(0.0, 2.0, 76, 0.7), n(3.0, 2.0, 72, 0.6), n(6.0, 2.0, 69, 0.55), n(8.0, 2.0, 71, 0.7), n(11.0, 2.0, 67, 0.6), n(14.0, 2.0, 64, 0.55)])
}

fn starter_lead() -> (f64, Vec<Note>) {
    (8.0, vec![
        n(0.0, 1.0, 69, 0.8), n(1.0, 0.5, 72, 0.7), n(1.5, 0.5, 71, 0.7), n(2.0, 1.0, 69, 0.75), n(3.0, 1.0, 67, 0.7),
        n(4.0, 1.5, 64, 0.8), n(5.5, 0.5, 67, 0.7), n(6.0, 2.0, 69, 0.8),
    ])
}

fn starter_choir() -> (f64, Vec<Note>) {
    let mut v = Vec::new();
    for (bar, c) in CHORDS.iter().enumerate() {
        for &p in c {
            v.push(n(bar as f64 * 4.0, 4.0, p + 12, 0.6));
        }
    }
    (16.0, v)
}

fn starter_drone() -> (f64, Vec<Note>) {
    (16.0, vec![n(0.0, 16.0, 33, 0.8), n(0.0, 16.0, 40, 0.5)])
}

fn starter_kit() -> (f64, Vec<Note>) {
    let mut v = Vec::new();
    for bar in 0..2 {
        let t = bar as f64 * 4.0;
        for b in [0.0, 1.0, 2.0, 3.0] {
            v.push(n(t + b, 0.2, 36, 1.0));
        }
        v.push(n(t + 1.0, 0.2, 38, 0.9));
        v.push(n(t + 3.0, 0.2, 38, 0.95));
        for s in 0..8 {
            let off = s as f64 * 0.5;
            let open = s == 7 && bar == 1;
            v.push(n(t + off, 0.2, if open { 46 } else { 42 }, if s % 2 == 1 { 0.75 } else { 0.45 }));
        }
    }
    v.push(n(7.75, 0.2, 38, 0.5));
    (8.0, v)
}

// ------------------------------------------------------------ catalog

pub static INSTRUMENTS: &[Instrument] = &[
    Instrument {
        id: "cg_pad",
        label: "warm pad",
        blurb: "seven detuned saws, slow filter bloom, chorus",
        knobs: &[
            k("cutoff", "brightness", 200.0, 12000.0, 1800.0, true, "filter cutoff"),
            k("detune", "detune", 0.0, 0.6, 0.18, false, "how far apart the saws drift (semitones)"),
            k("attack", "attack", 0.01, 4.0, 0.9, true, "fade-in per note (s)"),
            k("release", "release", 0.05, 6.0, 2.5, true, "fade-out per note (s)"),
            k("width", "width", 0.0, 1.0, 0.8, false, "stereo spread"),
            k("air", "air", 0.0, 1.0, 0.3, false, "breathy noise on top"),
        ],
        trim: 1.0,
        starter: starter_pad,
        kit: false,
    },
    Instrument {
        id: "cg_keys",
        label: "electric piano",
        blurb: "FM tine piano with tremolo",
        knobs: &[
            k("tone", "tone", 0.0, 1.0, 0.5, false, "how hard the tines are struck"),
            k("bell", "bell", 0.0, 1.0, 0.5, false, "metallic ping on the attack"),
            k("decay", "decay", 0.2, 6.0, 2.5, true, "how long notes ring (s)"),
            k("tremolo", "tremolo", 0.0, 1.0, 0.25, false, "wobble + auto-pan"),
        ],
        trim: 1.0,
        starter: starter_keys,
        kit: false,
    },
    Instrument {
        id: "cg_pluck",
        label: "pluck",
        blurb: "plucked string (Karplus-Strong) arpeggio",
        knobs: &[
            k("decay", "ring", 0.3, 8.0, 3.0, true, "how long the string rings (s)"),
            k("color", "color", 0.0, 1.0, 0.5, false, "dark nylon to bright steel"),
            k("body", "body", 0.0, 1.0, 0.4, false, "wooden body resonance"),
        ],
        trim: 1.0,
        starter: starter_pluck,
        kit: false,
    },
    Instrument {
        id: "cg_bass",
        label: "bass",
        blurb: "sub + saw through a Moog ladder",
        knobs: &[
            k("cutoff", "cutoff", 80.0, 5000.0, 700.0, true, "filter cutoff"),
            k("res", "resonance", 0.0, 3.5, 1.6, false, "filter emphasis"),
            k("drive", "drive", 0.0, 1.0, 0.4, false, "saturation"),
            k("sub", "sub", 0.0, 1.0, 0.6, false, "sine an octave down"),
            k("glide", "glide", 0.0, 0.3, 0.0, false, "slide between notes (s)"),
        ],
        trim: 1.0,
        starter: starter_bass,
        kit: false,
    },
    Instrument {
        id: "cg_acid",
        label: "acid",
        blurb: "303-style squelch line",
        knobs: &[
            k("cutoff", "cutoff", 80.0, 4000.0, 500.0, true, "base filter cutoff"),
            k("res", "resonance", 0.04, 0.8, 0.15, true, "lower = squelchier"),
            k("envmod", "env mod", 0.0, 1.0, 0.7, false, "how far each note opens the filter"),
            k("decay", "decay", 0.05, 1.5, 0.3, true, "filter envelope decay (s)"),
            k("wave", "saw / square", 0.0, 1.0, 0.0, false, ""),
        ],
        trim: 0.9,
        starter: starter_acid,
        kit: false,
    },
    Instrument {
        id: "cg_bell",
        label: "bell",
        blurb: "inharmonic FM bell, shimmering tail",
        knobs: &[
            k("ratio", "ratio", 0.5, 8.0, 3.5, false, "modulator ratio — changes the metal"),
            k("index", "strike", 0.0, 10.0, 3.0, false, "FM depth"),
            k("decay", "decay", 0.3, 12.0, 5.0, true, "ring time (s)"),
            k("shimmer", "shimmer", 0.0, 1.0, 0.4, false, "stereo chorus on the tail"),
        ],
        trim: 1.0,
        starter: starter_bell,
        kit: false,
    },
    Instrument {
        id: "cg_lead",
        label: "supersaw lead",
        blurb: "bright detuned saw lead with vibrato",
        knobs: &[
            k("cutoff", "brightness", 300.0, 14000.0, 4000.0, true, "filter cutoff"),
            k("detune", "detune", 0.0, 0.5, 0.12, false, "saw spread (semitones)"),
            k("vibrato", "vibrato", 0.0, 1.0, 0.15, false, "delayed vibrato depth"),
            k("release", "release", 0.02, 3.0, 0.4, true, "fade-out per note (s)"),
        ],
        trim: 0.9,
        starter: starter_lead,
        kit: false,
    },
    Instrument {
        id: "cg_choir",
        label: "choir",
        blurb: "vowel formants: a e i o u",
        knobs: &[
            k("vowel", "vowel", 0.0, 1.0, 0.2, false, "a → e → i → o → u"),
            k("breath", "breath", 0.0, 1.0, 0.3, false, "air in the voice"),
            k("attack", "attack", 0.01, 3.0, 0.6, true, "fade-in (s)"),
            k("release", "release", 0.05, 5.0, 1.5, true, "fade-out (s)"),
        ],
        trim: 1.0,
        starter: starter_choir,
        kit: false,
    },
    Instrument {
        id: "cg_drone",
        label: "drone",
        blurb: "slowly moving resonant bed",
        knobs: &[
            k("motion", "motion", 0.0, 1.0, 0.4, false, "how fast the harmonics wander"),
            k("color", "color", 0.0, 1.0, 0.5, false, "dark to bright"),
            k("attack", "attack", 0.1, 8.0, 3.0, true, "swell in (s)"),
            k("release", "release", 0.1, 8.0, 4.0, true, "fade out (s)"),
        ],
        trim: 1.0,
        starter: starter_drone,
        kit: false,
    },
    Instrument {
        id: "kit",
        label: "drum kit",
        blurb: "kick, snare, hats — synthesized",
        knobs: &[
            k("cg_kick.punch", "kick punch", 0.0, 1.0, 0.6, false, "pitch drop on the hit"),
            k("cg_kick.decay", "kick length", 0.1, 1.5, 0.45, true, "(s)"),
            k("cg_snare.tone", "snare body", 0.0, 1.0, 0.4, false, ""),
            k("cg_snare.snap", "snare snap", 0.0, 1.0, 0.7, false, "noise wires"),
            k("cg_snare.decay", "snare length", 0.05, 0.8, 0.22, true, "(s)"),
            k("cg_hat.decay", "hat length", 0.02, 0.6, 0.07, true, "(s) — open hats ring 4× longer"),
            k("cg_hat.tone", "hat tone", 0.0, 1.0, 0.6, false, ""),
        ],
        trim: 1.0,
        starter: starter_kit,
        kit: true,
    },
];

pub fn instrument(id: &str) -> Option<&'static Instrument> {
    INSTRUMENTS.iter().find(|i| i.id == id)
}

// ------------------------------------------------------------ parts

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Beats from the part start.
    pub start: f64,
    pub len: f64,
    /// MIDI note (60 = middle C).
    pub pitch: u8,
    /// 0..1
    pub vel: f32,
}

/// An instrument + its knob settings + a loop of notes. Rendered into the
/// library source `source` (same id forever, so clips follow edits).
#[derive(Debug, Clone, PartialEq)]
pub struct SynthPart {
    pub instrument: String,
    /// Knob values by control name.
    pub params: Vec<(String, f32)>,
    pub notes: Vec<Note>,
    pub length_beats: f64,
    /// Library source this part renders into.
    pub source: usize,
}

impl SynthPart {
    /// A part with the instrument's starter notes and default knobs.
    pub fn new(instrument_id: &str, source: usize) -> SynthPart {
        let inst = instrument(instrument_id).unwrap_or(&INSTRUMENTS[0]);
        let (length_beats, notes) = (inst.starter)();
        SynthPart {
            instrument: inst.id.to_string(),
            params: inst.knobs.iter().map(|k| (k.name.to_string(), k.default)).collect(),
            notes,
            length_beats,
            source,
        }
    }

    pub fn param(&self, name: &str) -> Option<f32> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    pub fn set_param(&mut self, name: &str, v: f32) {
        match self.params.iter_mut().find(|(n, _)| n == name) {
            Some(p) => p.1 = v,
            None => self.params.push((name.to_string(), v)),
        }
    }

    /// Fingerprint of everything that affects the sound at `bpm`.
    pub fn render_key(&self, bpm: f64, sample_rate: u32) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.instrument.hash(&mut h);
        for (n, v) in &self.params {
            n.hash(&mut h);
            v.to_bits().hash(&mut h);
        }
        for nt in &self.notes {
            nt.start.to_bits().hash(&mut h);
            nt.len.to_bits().hash(&mut h);
            nt.pitch.hash(&mut h);
            nt.vel.to_bits().hash(&mut h);
        }
        self.length_beats.to_bits().hash(&mut h);
        bpm.to_bits().hash(&mut h);
        sample_rate.hash(&mut h);
        h.finish()
    }

    /// Frequency of the lowest note (for key quantization of the render).
    pub fn base_hz(&self) -> f32 {
        let inst = instrument(&self.instrument);
        if inst.is_some_and(|i| i.kit) {
            return 0.0;
        }
        self.notes.iter().map(|n| n.pitch).min().map(midi_hz).unwrap_or(0.0)
    }
}

pub fn midi_hz(p: u8) -> f32 {
    440.0 * 2f32.powf((p as f32 - 69.0) / 12.0)
}

pub fn note_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[p as usize % 12], p as i32 / 12 - 1)
}

/// Seconds rendered past the loop end and folded back onto the start.
const TAIL_SECS: f64 = 6.0;

/// Render a part to stereo audio at `bpm` as a seamless loop.
pub fn render_part(part: &SynthPart, bpm: f64, sample_rate: u32) -> Result<AudioClip, String> {
    let inst = instrument(&part.instrument).ok_or_else(|| format!("unknown instrument '{}'", part.instrument))?;
    let sr = sample_rate as f64;
    let spb = 60.0 / bpm.max(1.0);
    let loop_n = ((part.length_beats.max(0.25) * spb) * sr).round() as usize;
    let (l, r) = render_raw(part, inst, bpm, sample_rate, loop_n + (TAIL_SECS * sr) as usize)?;
    // Fold the tail back onto the loop start: seamless looping.
    let fold = |out: &[f32]| {
        let mut v = out[..loop_n].to_vec();
        for (i, s) in out[loop_n..].iter().enumerate() {
            v[i % loop_n] += *s;
        }
        v
    };
    let (mut l, mut r) = (fold(&l), fold(&r));
    finish_level(&mut [&mut l, &mut r], inst.trim);
    Ok(AudioClip::stereo(&l, &r, sample_rate))
}

/// One note of a part's instrument (with its current knobs), for
/// auditioning while editing. Not looped; trailing silence trimmed.
pub fn render_note(part: &SynthPart, pitch: u8, vel: f32, bpm: f64, sample_rate: u32) -> Result<Vec<f32>, String> {
    let inst = instrument(&part.instrument).ok_or_else(|| format!("unknown instrument '{}'", part.instrument))?;
    let spb = 60.0 / bpm.max(1.0);
    let len = if inst.kit { 0.25 } else { 1.0 };
    let mut p = part.clone();
    p.notes = vec![Note { start: 0.0, len, pitch, vel }];
    p.length_beats = 64.0;
    let secs = len * spb + if inst.kit { 1.0 } else { 2.5 };
    let (l, r) = render_raw(&p, inst, bpm, sample_rate, (secs * sample_rate as f64) as usize)?;
    let mut out: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let last = out.iter().rposition(|v| v.abs() > 1e-4).unwrap_or(0);
    out.truncate((last + 1).max(64));
    let fade = (0.03 * sample_rate as f64) as usize;
    let n = out.len();
    for i in 0..fade.min(n) {
        out[n - 1 - i] *= i as f32 / fade as f32;
    }
    finish_level(&mut [&mut out], inst.trim * 0.8);
    Ok(out)
}

fn render_raw(part: &SynthPart, inst: &Instrument, bpm: f64, sample_rate: u32, total_n: usize) -> Result<(Vec<f32>, Vec<f32>), String> {
    let sr = sample_rate as f64;
    let spb = 60.0 / bpm.max(1.0);

    let (mut ctl, mut nrt, mut world) = engine(Options {
        sample_rate: sr,
        output_channels: 2,
        input_channels: 0,
        max_nodes: 4096,
        ..Options::default()
    });

    // Which defs this part needs.
    let mut defs: Vec<&str> = if inst.kit { KIT.iter().map(|d| d.1).collect() } else { vec![inst.id] };
    defs.dedup();
    let mut def_ids = std::collections::HashMap::new();
    let mut param_idx: std::collections::HashMap<&str, Vec<String>> = std::collections::HashMap::new();
    for d in &defs {
        if def_ids.contains_key(d) {
            continue;
        }
        let bytes = defs_bytes(d).ok_or_else(|| format!("missing synthdef {d}"))?;
        for (def, reblock, resample) in plyphon::synthdef::read::parse(bytes).map_err(|e| format!("{d}: {e:?}"))? {
            let name = def.name.clone();
            let names = def.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
            ctl.add_synthdef_rate(def, reblock, resample);
            let id = ctl.ensure_compiled(&name).map_err(|e| format!("{name}: {e:?}"))?;
            def_ids.insert(*d, id);
            param_idx.insert(*d, names);
        }
    }
    let idx = |d: &str, name: &str| param_idx.get(d).and_then(|v| v.iter().position(|p| p == name));

    // Timeline of commands in samples.
    enum Ev {
        On { note: usize },
        Off { note: usize },
    }
    let mut evs: Vec<(usize, u8, Ev)> = Vec::new();
    for (i, nt) in part.notes.iter().enumerate() {
        if nt.start >= part.length_beats || nt.len <= 0.0 {
            continue;
        }
        let on = (nt.start * spb * sr).round() as usize;
        let off = ((nt.start + nt.len) * spb * sr).round() as usize;
        evs.push((on, 1, Ev::On { note: i }));
        evs.push((off.max(on + 1), 0, Ev::Off { note: i }));
    }
    evs.sort_by_key(|e| (e.0, e.1)); // offs before ons at the same sample

    let mut node_of: Vec<i32> = vec![0; part.notes.len()];
    let mut next_id = 1000i32;
    let mut buf = vec![0f32; 64 * 2];
    let mut out_l = vec![0f32; total_n];
    let mut out_r = vec![0f32; total_n];
    let mut ei = 0usize;
    let mut pos = 0usize;
    let mut batch: Vec<ControllerBatchCommand> = Vec::new();
    while pos < total_n {
        // Fire everything due before the end of this 64-sample block.
        while ei < evs.len() && evs[ei].0 < pos + 64 {
            match evs[ei].2 {
                Ev::On { note } => {
                    let nt = part.notes[note];
                    let (def, freq, drum) = if inst.kit {
                        let d = KIT.iter().find(|d| d.0 == nt.pitch).or_else(|| KIT.iter().min_by_key(|d| (d.0 as i32 - nt.pitch as i32).abs()));
                        let d = d.unwrap();
                        (d.1, d.2, Some(d.3))
                    } else {
                        (inst.id, midi_hz(nt.pitch), None)
                    };
                    let Some(&def_id) = def_ids.get(def) else { ei += 1; continue };
                    let id = next_id;
                    next_id += 1;
                    node_of[note] = id;
                    batch.clear();
                    batch.push(ControllerBatchCommand::AddSynth { id, def_id, target: ROOT_GROUP_ID, action: AddAction::Tail });
                    let mut set = |name: &str, v: f32| {
                        if let Some(p) = idx(def, name) {
                            batch.push(ControllerBatchCommand::SetControl { node: id, param: p, value: v });
                        }
                    };
                    set("freq", freq);
                    set("amp", nt.vel.clamp(0.0, 1.0) * 0.5);
                    // A little kit image: hats right, open hat left.
                    match drum {
                        Some("hat") => set("pan", 0.3),
                        Some("open hat") => set("pan", -0.25),
                        Some("snare") => set("pan", 0.05),
                        _ => {}
                    }
                    for (name, v) in &part.params {
                        match name.split_once('.') {
                            Some((d, c)) if d == def => {
                                let v = if drum == Some("open hat") && c == "decay" { v * 4.0 } else { *v };
                                set(c, v)
                            }
                            Some(_) => {}
                            None => set(name, *v),
                        }
                    }
                    let _ = ctl.try_send_batch(&batch);
                }
                Ev::Off { note } => {
                    let id = node_of[note];
                    let def = if inst.kit { "" } else { inst.id };
                    if let Some(p) = idx(def, "gate") {
                        let _ = ctl.set_control(id, p, 0.0);
                    }
                }
            }
            ei += 1;
        }
        world.fill(&mut buf, 2);
        nrt.process();
        while nrt.poll().is_some() {}
        let n = 64.min(total_n - pos);
        for i in 0..n {
            out_l[pos + i] = buf[2 * i];
            out_r[pos + i] = buf[2 * i + 1];
        }
        pos += 64;
    }
    Ok((out_l, out_r))
}

/// Library media for a rendered part: the audio plus a glowing
/// oscilloscope trace of it on a transparent background, so the part has
/// a picture like any other clip without hiding the video underneath.
pub fn part_media(audio: AudioClip) -> (std::sync::Arc<AudioClip>, std::sync::Arc<crate::video::VideoClip>) {
    let video = crate::synth::scope_overlay(&audio, 240, 136, 12.0);
    (std::sync::Arc::new(audio), std::sync::Arc::new(video))
}

/// Display name for a part's library card.
pub fn part_name(part: &SynthPart) -> String {
    format!("♪ {}", instrument(&part.instrument).map(|i| i.label).unwrap_or("synth"))
}

/// Bring every render to a consistent loudness with a soft ceiling, so
/// switching instruments or knobs never jumps the level.
/// Channels share one gain so the stereo image is preserved.
fn finish_level(chans: &mut [&mut Vec<f32>], trim: f32) {
    let (mut sum, mut count, mut peak) = (0f64, 0usize, 0f32);
    for s in chans.iter() {
        for &v in s.iter() {
            if v.abs() > 1e-4 {
                sum += (v * v) as f64;
                count += 1;
            }
            peak = peak.max(v.abs());
        }
    }
    if count == 0 {
        return;
    }
    let rms = (sum / count as f64).sqrt() as f32;
    // Target ~ -16 dBFS RMS, but never push the peak past ~-1 dBFS.
    let mut g = 0.16 / rms.max(1e-6);
    g = g.min(0.89 / peak.max(1e-6) * 1.6);
    g *= trim;
    for s in chans.iter_mut() {
        for v in s.iter_mut() {
            let x = *v * g;
            // Soft knee above 0.8.
            *v = if x.abs() <= 0.8 { x } else { x.signum() * (0.8 + 0.2 * ((x.abs() - 0.8) / 0.2).tanh()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SC_WAV_DIR=/some/dir cargo test --release -p chromagrain-core sc::tests::dump -- --ignored`
    /// writes every instrument's starter loop (twice through) to WAV for listening.
    #[test]
    #[ignore]
    fn dump_wavs() {
        let dir = std::env::var("SC_WAV_DIR").unwrap_or_else(|_| "/tmp/cg_synth".into());
        std::fs::create_dir_all(&dir).unwrap();
        for inst in INSTRUMENTS {
            let a = render_part(&SynthPart::new(inst.id, 0), 100.0, 48000).unwrap();
            let spec = hound::WavSpec { channels: 2, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(format!("{dir}/{}.wav", inst.id), spec).unwrap();
            let (l, r) = a.left_right();
            for _ in 0..2 {
                for (x, y) in l.iter().zip(&r) {
                    w.write_sample((x.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
                    w.write_sample((y.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
                }
            }
            w.finalize().unwrap();
        }
    }

    /// plyphon once read audio-rate MulAdd operands once per 64-sample
    /// block (vendor/plyphon-unit/PATCHES.md): the output was a staircase
    /// with jumps at every block boundary. Guard against that coming back.
    #[test]
    fn no_block_rate_staircase() {
        for inst in INSTRUMENTS {
            // 120 bpm: a beat is 24000 samples, a whole number of blocks,
            // so the folded tail stays block-aligned.
            let a = render_part(&SynthPart::new(inst.id, 0), 120.0, 48000).unwrap();
            let (mut edge, mut ne, mut inner, mut ni) = (0f64, 0, 0f64, 0);
            for i in 1..a.samples.len() {
                let d = (a.samples[i] - a.samples[i - 1]).abs() as f64;
                if i % 64 == 0 { edge += d; ne += 1 } else { inner += d; ni += 1 }
            }
            let ratio = (edge / ne as f64) / (inner / ni as f64).max(1e-9);
            assert!(ratio < 1.6, "{}: block-edge steps {ratio:.1}x larger than elsewhere", inst.id);
        }
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_instrument_loads_and_sounds() {
        for inst in INSTRUMENTS {
            let part = SynthPart::new(inst.id, 0);
            let t = std::time::Instant::now();
            let a = render_part(&part, 100.0, 48000).unwrap_or_else(|e| panic!("{}: {e}", inst.id));
            let secs = part.length_beats * 0.6;
            assert!((a.duration() - secs).abs() < 0.01, "{}: loop length {}", inst.id, a.duration());
            let r = rms(&a.samples);
            let peak = a.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(r > 0.03, "{} is too quiet: rms {r}", inst.id);
            assert!(peak <= 1.0, "{} clips: {peak}", inst.id);
            assert!(a.samples.iter().all(|v| v.is_finite()), "{} has NaN", inst.id);
            eprintln!("{:>16}: rms {r:.3} peak {peak:.2} in {:?}", inst.label, t.elapsed());
        }
    }

    #[test]
    fn single_note_audition() {
        for id in ["cg_pluck", "kit"] {
            let part = SynthPart::new(id, 0);
            let v = render_note(&part, if id == "kit" { 38 } else { 60 }, 0.8, 120.0, 48000).unwrap();
            let peak = v.iter().fold(0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.1 && peak <= 1.0, "{id}: peak {peak}");
            assert!(v.len() < 48000 * 4, "{id}: not trimmed ({} samples)", v.len());
        }
    }

    #[test]
    fn knobs_change_the_sound() {
        let mut part = SynthPart::new("cg_acid", 0);
        let a = render_part(&part, 120.0, 48000).unwrap();
        part.set_param("cutoff", 3000.0);
        let b = render_part(&part, 120.0, 48000).unwrap();
        let diff: f32 = a.samples.iter().zip(&b.samples).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "cutoff knob does nothing");
        assert_ne!(SynthPart::new("cg_acid", 0).render_key(120.0, 48000), part.render_key(120.0, 48000));
    }

    #[test]
    fn loop_is_seamless() {
        // The drone rings the whole loop; with tail folding its end flows
        // into its start without a jump.
        let part = SynthPart::new("cg_pad", 0);
        let a = render_part(&part, 100.0, 48000).unwrap();
        let s = &a.samples;
        let jump = (s[s.len() - 1] - s[0]).abs();
        let typical = s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / s.len() as f32;
        assert!(jump < typical * 40.0 + 0.02, "loop seam jump {jump} vs typical step {typical}");
    }

    #[test]
    fn wide_instruments_render_in_stereo() {
        for id in ["cg_pad", "cg_lead", "cg_drone"] {
            let a = render_part(&SynthPart::new(id, 0), 100.0, 48000).unwrap();
            let (l, r) = a.left_right();
            // Normalized correlation < 1 means real width, not dual mono.
            let dot: f64 = l.iter().zip(&r).map(|(x, y)| (*x * *y) as f64).sum();
            let el: f64 = l.iter().map(|x| (*x * *x) as f64).sum();
            let er: f64 = r.iter().map(|x| (*x * *x) as f64).sum();
            let corr = dot / (el * er).sqrt().max(1e-12);
            eprintln!("{id}: L/R correlation {corr:.2}");
            assert!(a.is_stereo(), "{id} came back mono");
            assert!(corr < 0.98, "{id}: L/R nearly identical ({corr:.3})");
            assert!(l.iter().chain(&r).all(|v| v.abs() <= 1.0));
        }
    }
}
