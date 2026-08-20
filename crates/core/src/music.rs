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

/// The one place a grain's final pitch ratio is computed: raw semitones,
/// optionally pulled toward the key by `quantize_amount` (1 = hard snap,
/// 0.5 = halfway, 0 = free), then shifted by `detune_cents` AFTER the
/// snap — so you can sit subtly off-pitch of a perfectly quantized note.
pub fn effective_ratio(
    semitones: f32,
    key: Option<&Key>,
    base_hz: f32,
    quantize_amount: f32,
    detune_cents: f32,
) -> f32 {
    let raw = semitones_to_ratio(semitones);
    let amount = quantize_amount.clamp(0.0, 1.0);
    let snapped = match key {
        Some(k) if amount > 0.0 => {
            let q = k.quantize_ratio(base_hz, raw);
            let st_raw = ratio_to_semitones(raw);
            let st_q = ratio_to_semitones(q);
            semitones_to_ratio(st_raw + (st_q - st_raw) * amount)
        }
        _ => raw,
    };
    snapped * semitones_to_ratio(detune_cents.clamp(-100.0, 100.0) / 100.0)
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
    fn effective_ratio_partial_quantize_lands_halfway() {
        let key = Key::parse("C", "major").unwrap();
        let base = midi_to_hz(60.0); // source fundamental = C4
        // +0.5 semitones from C4 sits between C and C#; hard snap goes to C.
        let st = 0.5;
        let raw_st = st;
        let full = effective_ratio(st, Some(&key), base, 1.0, 0.0);
        let snapped_st = ratio_to_semitones(full);
        let half = effective_ratio(st, Some(&key), base, 0.5, 0.0);
        let half_st = ratio_to_semitones(half);
        // amount=0.5 lands halfway between raw and snapped in semitone space.
        assert!((half_st - (raw_st + snapped_st) / 2.0).abs() < 1e-3);
        // amount=0 is a no-op even with a key set.
        let free = effective_ratio(st, Some(&key), base, 0.0, 0.0);
        assert!((ratio_to_semitones(free) - raw_st).abs() < 1e-3);
    }

    #[test]
    fn effective_ratio_detune_applies_after_snap() {
        let key = Key::parse("C", "major").unwrap();
        let base = midi_to_hz(60.0);
        let snapped = effective_ratio(0.3, Some(&key), base, 1.0, 0.0);
        let detuned = effective_ratio(0.3, Some(&key), base, 1.0, 50.0);
        // +50 cents shifts the already-snapped ratio by exactly 2^(50/1200).
        let expect = snapped * 2.0_f32.powf(50.0 / 1200.0);
        assert!((detuned - expect).abs() < 1e-5);
        // Detune works with no key too.
        let d = effective_ratio(0.0, None, base, 1.0, -25.0);
        assert!((d - 2.0_f32.powf(-25.0 / 1200.0)).abs() < 1e-5);
        // Out-of-range detune clamps to +/-100 cents.
        let c = effective_ratio(0.0, None, base, 0.0, 400.0);
        assert!((c - 2.0_f32.powf(100.0 / 1200.0)).abs() < 1e-5);
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
