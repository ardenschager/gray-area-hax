//! Step sequencer (Koala / Nanoloop style): patterns of rows, each row a
//! sampler pad (source + grain preset + key), each step a beat-quantized
//! trigger. Patterns live in the project and are placed on the timeline as
//! pattern clips, looping to fill the clip. A triggered step is ONE grain
//! event, so every hit gets the full AV correspondence.

use crate::dsp::Rng;
use crate::grain::{GrainEvent, GrainSettings};
use crate::music::{semitones_to_ratio, Key};
use crate::timeline::{Source, SourceId};

/// One step of a row: on/off, semitone offset, gain multiplier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub on: bool,
    pub pitch: f32,
    pub gain: f32,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, pitch: 0.0, gain: 1.0 }
    }
}

/// A sampler pad row: a source, how its hits sound/look, and its steps.
#[derive(Debug, Clone)]
pub struct SeqRow {
    pub source: SourceId,
    /// Hit preset: duration, position, pitch, gain, pan(+spread), envelope,
    /// spray, pitch_jitter and seed all apply per hit.
    pub grains: GrainSettings,
    pub key: Option<Key>,
    pub steps: Vec<Step>,
}

impl SeqRow {
    pub fn new(source: SourceId, n_steps: usize) -> SeqRow {
        SeqRow {
            source,
            grains: GrainSettings {
                duration: 0.25,
                spray: 0.0,
                pan_spread: 0.2,
                envelope: 0.25,
                ..Default::default()
            },
            key: None,
            steps: vec![Step::default(); n_steps],
        }
    }
}

/// A step pattern: a grid of rows x steps covering `length_beats`.
#[derive(Debug, Clone)]
pub struct StepPattern {
    pub name: String,
    pub steps_per_beat: u32,
    pub length_beats: f64,
    pub rows: Vec<SeqRow>,
}

impl StepPattern {
    pub fn new(name: &str) -> StepPattern {
        StepPattern {
            name: name.to_string(),
            steps_per_beat: 4,
            length_beats: 4.0,
            rows: Vec::new(),
        }
    }

    pub fn n_steps(&self) -> usize {
        (self.length_beats * self.steps_per_beat as f64).round().max(1.0) as usize
    }

    pub fn add_row(&mut self, source: SourceId) -> usize {
        self.rows.push(SeqRow::new(source, self.n_steps()));
        self.rows.len() - 1
    }

    /// Change the grid, resizing all rows (existing steps preserved).
    pub fn set_grid(&mut self, length_beats: f64, steps_per_beat: u32) {
        self.length_beats = length_beats.clamp(0.25, 64.0);
        self.steps_per_beat = steps_per_beat.clamp(1, 16);
        let n = self.n_steps();
        for row in &mut self.rows {
            row.steps.resize(n, Step::default());
        }
    }
}

/// Expand a pattern into grain events for a clip window.
///
/// * `clip_start_secs`/`clip_len_secs`: the pattern clip on the timeline;
///   the pattern loops to fill it.
/// * `key_override`: the clip's key, taking precedence over row keys.
///
/// Returns events grouped by source (rows can use different sources).
pub fn pattern_events(
    pattern: &StepPattern,
    sources: &[Source],
    clip_start_secs: f64,
    clip_len_secs: f64,
    secs_per_beat: f64,
    key_override: Option<&Key>,
) -> Vec<(SourceId, Vec<GrainEvent>)> {
    let mut out: Vec<(SourceId, Vec<GrainEvent>)> = Vec::new();
    let pattern_secs = pattern.length_beats * secs_per_beat;
    let step_secs = secs_per_beat / pattern.steps_per_beat as f64;
    if pattern_secs <= 0.0 || clip_len_secs <= 0.0 {
        return out;
    }

    for (row_idx, row) in pattern.rows.iter().enumerate() {
        let Some(source) = sources.get(row.source) else { continue };
        let source_len = source.duration();
        if source_len <= 0.0 {
            continue;
        }
        let base_hz = source.effective_base_hz();
        let key = key_override.or(row.key.as_ref());
        let g = &row.grains;
        let mut rng = Rng::new(g.seed ^ ((row_idx as u64 + 1) * 0x9E37));
        let mut events = Vec::new();
        let mut id: u64 = (row_idx as u64) << 32;

        let mut loop_start = 0.0f64;
        while loop_start < clip_len_secs {
            for (step_idx, step) in row.steps.iter().enumerate() {
                let t = loop_start + step_idx as f64 * step_secs;
                if t >= clip_len_secs {
                    break;
                }
                // Consume jitter deterministically for EVERY step so
                // toggling one step doesn't reshuffle the others.
                let spray_j = rng.bipolar();
                let pitch_j = rng.bipolar();
                let pan_j = rng.bipolar();
                let rev_j = rng.next_f32();
                if !step.on {
                    continue;
                }

                let st = g.pitch + step.pitch + g.pitch_jitter * pitch_j;
                let mut ratio = semitones_to_ratio(st);
                if let Some(k) = key {
                    ratio = k.quantize_ratio(base_hz, ratio);
                }
                let mut pos =
                    g.position as f64 * source_len + g.spray as f64 * spray_j as f64;
                pos = pos.rem_euclid(source_len.max(1e-6));

                events.push(GrainEvent {
                    onset: clip_start_secs + t,
                    source_pos: pos,
                    duration: g.duration.clamp(0.01, 5.0),
                    pitch_ratio: ratio,
                    gain: g.gain * step.gain,
                    pan: (g.pan + g.pan_spread * pan_j).clamp(-1.0, 1.0),
                    envelope: g.envelope,
                    reverse: rev_j < g.reverse_prob,
                    id,
                });
                id += 1;
            }
            loop_start += pattern_secs;
        }
        if !events.is_empty() {
            out.push((row.source, events));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioClip;
    use crate::music::{hz_to_midi, ScaleKind};

    fn source_440() -> Vec<Source> {
        vec![Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(AudioClip::sine(440.0, 2.0, 48000))),
            video: None,
            base_hz: 440.0,
        }]
    }

    fn four_on_the_floor() -> StepPattern {
        let mut p = StepPattern::new("kick");
        let r = p.add_row(0);
        for i in (0..16).step_by(4) {
            p.rows[r].steps[i].on = true;
        }
        p
    }

    #[test]
    fn steps_land_on_the_beat_grid() {
        let p = four_on_the_floor();
        let sources = source_440();
        // 120 bpm -> 0.5s per beat; pattern = 4 beats = 2s.
        let parts = pattern_events(&p, &sources, 10.0, 2.0, 0.5, None);
        assert_eq!(parts.len(), 1);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 4);
        for (i, ev) in evs.iter().enumerate() {
            let expected = 10.0 + i as f64 * 0.5;
            assert!(
                (ev.onset - expected).abs() < 1e-9,
                "hit {i} at {} (expected {expected}) — beat quantized",
                ev.onset
            );
        }
    }

    #[test]
    fn pattern_loops_to_fill_clip() {
        let p = four_on_the_floor();
        let sources = source_440();
        // Clip of 8 beats = two pattern loops.
        let parts = pattern_events(&p, &sources, 0.0, 4.0, 0.5, None);
        assert_eq!(parts[0].1.len(), 8);
        // Second loop's first hit lands exactly one pattern later.
        assert!((parts[0].1[4].onset - 2.0).abs() < 1e-9);
        // Unique ids across loops.
        let mut ids: Vec<u64> = parts[0].1.iter().map(|e| e.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 8);
    }

    #[test]
    fn step_pitch_and_key_quantize() {
        let mut p = StepPattern::new("melody");
        let r = p.add_row(0);
        p.rows[r].steps[0].on = true;
        p.rows[r].steps[0].pitch = 3.5; // off-grid pitch
        p.rows[r].key = Some(Key::new(0, ScaleKind::Major));
        let sources = source_440();
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None);
        let ev = &parts[0].1[0];
        let midi = hz_to_midi(440.0 * ev.pitch_ratio);
        assert!((midi - midi.round()).abs() < 1e-3, "quantized to key: {midi}");

        // Clip key override wins.
        let whole = Key::new(0, ScaleKind::WholeTone);
        let parts2 = pattern_events(&p, &sources, 0.0, 2.0, 0.5, Some(&whole));
        let ev2 = &parts2[0].1[0];
        let midi2 = hz_to_midi(440.0 * ev2.pitch_ratio);
        let pc = (midi2.round() as i32).rem_euclid(12);
        assert!(whole.scale.intervals().contains(&pc), "override key applied");
    }

    #[test]
    fn toggling_one_step_keeps_others_stable() {
        let sources = source_440();
        let mut p = four_on_the_floor();
        p.rows[0].grains.pitch_jitter = 5.0;
        p.rows[0].grains.pan_spread = 0.8;
        let before = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None);
        p.rows[0].steps[2].on = true; // add an off-grid hit
        let after = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None);
        // The original four hits are unchanged (jitter is per-step-slot).
        let find = |evs: &Vec<GrainEvent>, onset: f64| -> GrainEvent {
            evs.iter().find(|e| (e.onset - onset).abs() < 1e-9).unwrap().clone()
        };
        for onset in [0.0, 0.5, 1.0, 1.5] {
            let a = find(&before[0].1, onset);
            let mut b = find(&after[0].1, onset);
            b.id = a.id; // ids may shift; compare musical content
            assert_eq!(a.pitch_ratio, b.pitch_ratio);
            assert_eq!(a.pan, b.pan);
            assert_eq!(a.source_pos, b.source_pos);
        }
    }

    #[test]
    fn grid_resize_preserves_steps() {
        let mut p = four_on_the_floor();
        p.set_grid(8.0, 4);
        assert_eq!(p.n_steps(), 32);
        assert!(p.rows[0].steps[0].on);
        assert!(p.rows[0].steps[4].on);
        assert_eq!(p.rows[0].steps.len(), 32);
    }
}
