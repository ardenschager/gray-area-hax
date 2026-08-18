//! Musical pitch: notes, scales, keys, and quantization of arbitrary
//! frequencies / playback ratios onto a key.

use serde::{Deserialize, Serialize};

pub const A4_HZ: f32 = 440.0;
pub const A4_MIDI: f32 = 69.0;

/// Frequency (Hz) -> MIDI note number (fractional).
pub fn hz_to_midi(hz: f32) -> f32 {
    A4_MIDI + 12.0 * (hz / A4_HZ).log2()
}

/// MIDI note number (fractional) -> frequency (Hz).
pub fn midi_to_hz(midi: f32) -> f32 {
    A4_HZ * 2.0_f32.powf((midi - A4_MIDI) / 12.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleKind {
    Chromatic,
    Major,
    Minor,
    HarmonicMinor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    WholeTone,
}

impl ScaleKind {
    pub fn intervals(self) -> &'static [i32] {
        match self {
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::Minor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::WholeTone => &[0, 2, 4, 6, 8, 10],
        }
    }

    pub fn parse(name: &str) -> Option<ScaleKind> {
        let n = name.to_ascii_lowercase().replace([' ', '-', '_'], "");
        Some(match n.as_str() {
            "chromatic" => ScaleKind::Chromatic,
            "major" | "ionian" => ScaleKind::Major,
            "minor" | "aeolian" | "naturalminor" => ScaleKind::Minor,
            "harmonicminor" => ScaleKind::HarmonicMinor,
            "majorpentatonic" | "pentatonic" => ScaleKind::MajorPentatonic,
            "minorpentatonic" => ScaleKind::MinorPentatonic,
            "blues" => ScaleKind::Blues,
            "dorian" => ScaleKind::Dorian,
            "phrygian" => ScaleKind::Phrygian,
            "lydian" => ScaleKind::Lydian,
            "mixolydian" => ScaleKind::Mixolydian,
            "wholetone" => ScaleKind::WholeTone,
            _ => return None,
        })
    }
}

/// Parse a note name like "C", "F#", "Bb" to a pitch class (0 = C).
pub fn parse_pitch_class(name: &str) -> Option<i32> {
    let mut chars = name.trim().chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut acc: i32 = 0;
    for c in chars {
        match c {
            '#' | 's' => acc += 1,
            'b' | '♭' => acc -= 1,
            _ => return None,
        }
    }
    Some((base + acc).rem_euclid(12))
}

pub const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A musical key: root pitch class + scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class of the root, 0 = C .. 11 = B.
    pub root: i32,
    pub scale: ScaleKind,
}

impl Key {
    pub fn new(root: i32, scale: ScaleKind) -> Key {
        Key { root: root.rem_euclid(12), scale }
    }

    pub fn parse(root: &str, scale: &str) -> Option<Key> {
        Some(Key::new(parse_pitch_class(root)?, ScaleKind::parse(scale)?))
    }

    /// Snap a fractional MIDI note to the nearest note of this key
    /// (searching neighboring octaves so boundaries behave).
    pub fn quantize_midi(&self, midi: f32) -> f32 {
        let base_oct = (midi / 12.0).floor() as i32;
        let mut best = midi;
        let mut best_dist = f32::INFINITY;
        for oct in (base_oct - 1)..=(base_oct + 1) {
            for &iv in self.scale.intervals() {
                let cand = (oct * 12 + self.root + iv) as f32;
                let d = (cand - midi).abs();
                if d < best_dist {
                    best_dist = d;
                    best = cand;
                }
            }
        }
        best
    }

    /// Given a source whose fundamental is `base_hz`, adjust a desired
    /// playback `ratio` so the resulting pitch lands on this key.
    pub fn quantize_ratio(&self, base_hz: f32, ratio: f32) -> f32 {
        if base_hz <= 0.0 || ratio <= 0.0 {
            return ratio;
        }
        let target_midi = hz_to_midi(base_hz * ratio);
        let q = self.quantize_midi(target_midi);
        midi_to_hz(q) / base_hz
    }
}

/// Semitone offset -> playback ratio.
pub fn semitones_to_ratio(st: f32) -> f32 {
    2.0_f32.powf(st / 12.0)
}

/// Playback ratio -> semitone offset.
pub fn ratio_to_semitones(ratio: f32) -> f32 {
    12.0 * ratio.max(1e-6).log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_hz_roundtrip() {
        assert!((hz_to_midi(440.0) - 69.0).abs() < 1e-4);
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-2);
        assert!((midi_to_hz(hz_to_midi(123.4)) - 123.4).abs() < 1e-2);
    }

    #[test]
    fn parse_notes() {
        assert_eq!(parse_pitch_class("C"), Some(0));
        assert_eq!(parse_pitch_class("F#"), Some(6));
        assert_eq!(parse_pitch_class("Bb"), Some(10));
        assert_eq!(parse_pitch_class("Cb"), Some(11));
        assert_eq!(parse_pitch_class("x"), None);
    }

    #[test]
    fn quantize_in_scale_is_identity() {
        let key = Key::parse("C", "major").unwrap();
        // A4 = midi 69 is in C major.
        assert_eq!(key.quantize_midi(69.0), 69.0);
    }

    #[test]
    fn quantize_snaps_to_nearest_scale_note() {
        let key = Key::parse("C", "major").unwrap();
        // 70.4 (a sharp-ish A#) should snap to B (71), not A (69).
        assert_eq!(key.quantize_midi(70.4), 71.0);
        // 69.6 should also snap to... A# is not in scale; nearest of {69, 71} to 69.6 is 69.
        assert_eq!(key.quantize_midi(69.6), 69.0);
    }

    #[test]
    fn quantize_ratio_lands_on_key() {
        let key = Key::parse("D", "minorpentatonic").unwrap();
        let base = 440.0;
        for i in 0..40 {
            let ratio = 0.5 + i as f32 * 0.05;
            let q = key.quantize_ratio(base, ratio);
            let midi = hz_to_midi(base * q);
            let pc = (midi.round() as i32).rem_euclid(12);
            let rel = (pc - key.root).rem_euclid(12);
            assert!(
                key.scale.intervals().contains(&rel),
                "ratio {ratio} -> midi {midi} pc {pc} not in scale"
            );
            // Should be within a half step of round (i.e. exactly on a note).
            assert!((midi - midi.round()).abs() < 1e-3);
        }
    }
}
