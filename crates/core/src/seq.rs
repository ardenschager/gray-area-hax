//! Step sequencer (Koala / Nanoloop style): patterns of rows, each row a
//! sampler pad (source + grain preset + key), each step a beat-quantized
//! trigger. Patterns live in the project and are placed on the timeline as
//! pattern clips, looping to fill the clip. A triggered step is ONE grain
//! event, so every hit gets the full AV correspondence.

use crate::dsp::Rng;
use crate::grain::{GrainEvent, GrainSettings};
use crate::music::Key;
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

/// How a row makes sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMode {
    /// Beat-quantized step triggers. The row cycles its OWN step count at
    /// the pattern's step rate — a 12-step row against a 16-step pattern
    /// drifts in and out of phase (polymeter).
    Steps,
    /// Not sequenced: the row loops a start..stop segment of its source,
    /// repeating seamlessly for the whole clip.
    Loop,
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
    pub mode: RowMode,
    /// Loop-mode segment, normalized 0..1 of the source.
    pub loop_start: f32,
    pub loop_end: f32,
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
            mode: RowMode::Steps,
            loop_start: 0.0,
            loop_end: 1.0,
        }
    }

    /// Set this row's own step count (polymeter against the pattern grid).
    pub fn set_steps(&mut self, n: usize) {
        self.steps.resize(n.clamp(1, 128), Step::default());
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

    /// Change the grid. Rows still matching the old default step count
    /// follow along; rows with a custom (polymeter) count keep theirs.
    pub fn set_grid(&mut self, length_beats: f64, steps_per_beat: u32) {
        let old_n = self.n_steps();
        self.length_beats = length_beats.clamp(0.25, 64.0);
        self.steps_per_beat = steps_per_beat.clamp(1, 16);
        let n = self.n_steps();
        for row in &mut self.rows {
            if row.steps.len() == old_n {
                row.steps.resize(n, Step::default());
            }
        }
    }
}

/// Expand a pattern into grain events for a clip window.
///
/// * `clip_start_secs`/`clip_len_secs`: the pattern clip on the timeline.
///   Step rows cycle their OWN step count at the pattern's step rate
///   (polymeter); loop rows repeat their source segment continuously.
/// * `key_override`: the clip's key, taking precedence over row keys.
/// * `lanes`: the clip's automation lanes, overriding row grain params at
///   each hit's onset (lane beats relative to clip start).
///
/// Returns events grouped by source (rows can use different sources).
pub fn pattern_events(
    pattern: &StepPattern,
    sources: &[Source],
    clip_start_secs: f64,
    clip_len_secs: f64,
    secs_per_beat: f64,
    key_override: Option<&Key>,
    lanes: &[crate::auto::AutomationLane],
) -> Vec<(SourceId, Vec<GrainEvent>)> {
    let mut out: Vec<(SourceId, Vec<GrainEvent>)> = Vec::new();
    let step_secs = secs_per_beat / pattern.steps_per_beat as f64;
    if step_secs <= 0.0 || clip_len_secs <= 0.0 {
        return out;
    }

    for (row_idx, row) in pattern.rows.iter().enumerate() {
        let Some(source) = sources.get(row.source) else { continue };
        let source_len = source.duration();
        if source_len <= 0.0 || row.steps.is_empty() {
            continue;
        }
        let base_hz = source.effective_base_hz();
        let key = key_override.or(row.key.as_ref());
        let mut rng = Rng::new(row.grains.seed ^ ((row_idx as u64 + 1) * 0x9E37));
        let mut events = Vec::new();
        let mut id: u64 = (row_idx as u64) << 32;

        let eff_at = |t: f64| -> GrainSettings {
            if lanes.is_empty() {
                row.grains.clone()
            } else {
                crate::auto::settings_at(&row.grains, lanes, t / secs_per_beat.max(1e-9))
            }
        };

        match row.mode {
            RowMode::Steps => {
                // Slot k cycles this row's own step count — rows shorter or
                // longer than the pattern grid phase against it (polymeter).
                let mut slot = 0usize;
                loop {
                    let t = slot as f64 * step_secs;
                    if t >= clip_len_secs {
                        break;
                    }
                    let step = row.steps[slot % row.steps.len()];
                    // Consume jitter deterministically for EVERY slot so
                    // toggling one step doesn't reshuffle the others.
                    let spray_j = rng.bipolar();
                    let pitch_j = rng.bipolar();
                    let pan_j = rng.bipolar();
                    let rev_j = rng.next_f32();
                    slot += 1;
                    if !step.on {
                        continue;
                    }
                    let g = eff_at(t);

                    let st = g.pitch + step.pitch + g.pitch_jitter * pitch_j;
                    let ratio = crate::music::effective_ratio(
                        st,
                        key,
                        base_hz,
                        g.quantize_amount,
                        g.detune_cents,
                    );
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
                        env_skew: g.env_skew,
                        reverse: rev_j < g.reverse_prob,
                        id,
                    });
                    id += 1;
                }
            }
            RowMode::Loop => {
                // Not sequenced: repeat the start..stop source segment
                // seamlessly for the whole clip.
                let s0 = row.loop_start.clamp(0.0, 1.0) as f64;
                let s1 = row.loop_end.clamp(0.0, 1.0) as f64;
                let seg_src = ((s1 - s0) * source_len).max(0.02);
                let mut t = 0.0f64;
                while t < clip_len_secs {
                    let pan_j = rng.bipolar();
                    let g = eff_at(t);
                    let ratio = crate::music::effective_ratio(
                        g.pitch,
                        key,
                        base_hz,
                        g.quantize_amount,
                        g.detune_cents,
                    );
                    // Playing the segment at `ratio` takes seg/ratio output
                    // seconds; repetitions tile back to back.
                    let dur_out = (seg_src / ratio.max(0.01) as f64).clamp(0.02, 60.0);
                    events.push(GrainEvent {
                        onset: clip_start_secs + t,
                        source_pos: (s0 * source_len).min(source_len),
                        duration: dur_out as f32,
                        pitch_ratio: ratio,
                        gain: g.gain,
                        pan: (g.pan + g.pan_spread * pan_j).clamp(-1.0, 1.0),
                        envelope: g.envelope,
                        env_skew: g.env_skew,
                        reverse: g.reverse_prob >= 0.5,
                        id,
                    });
                    id += 1;
                    t += dur_out;
                }
            }
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
        let parts = pattern_events(&p, &sources, 10.0, 2.0, 0.5, None, &[]);
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
        let parts = pattern_events(&p, &sources, 0.0, 4.0, 0.5, None, &[]);
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
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let ev = &parts[0].1[0];
        let midi = hz_to_midi(440.0 * ev.pitch_ratio);
        assert!((midi - midi.round()).abs() < 1e-3, "quantized to key: {midi}");

        // Clip key override wins.
        let whole = Key::new(0, ScaleKind::WholeTone);
        let parts2 = pattern_events(&p, &sources, 0.0, 2.0, 0.5, Some(&whole), &[]);
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
        let before = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        p.rows[0].steps[2].on = true; // add an off-grid hit
        let after = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
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
    fn polymeter_row_cycles_its_own_length() {
        let mut p = StepPattern::new("poly");
        let r = p.add_row(0);
        // A 3-step row against the 16-step grid: hit on its step 0 only.
        p.rows[r].set_steps(3);
        p.rows[r].steps[0].on = true;
        let sources = source_440();
        // 8 beats at 0.5 s/beat = 32 slots; hits on slots 0,3,6,...,30.
        let parts = pattern_events(&p, &sources, 0.0, 4.0, 0.5, None, &[]);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 11, "ceil(32/3) hits, got {}", evs.len());
        for (k, ev) in evs.iter().enumerate() {
            let expected = (k * 3) as f64 * 0.125;
            assert!(
                (ev.onset - expected).abs() < 1e-9,
                "hit {k} at {} expected {expected} — drifts across the grid",
                ev.onset
            );
        }
    }

    #[test]
    fn loop_mode_repeats_source_segment() {
        let mut p = StepPattern::new("loops");
        let r = p.add_row(0);
        p.rows[r].mode = RowMode::Loop;
        p.rows[r].loop_start = 0.25;
        p.rows[r].loop_end = 0.5; // quarter of a 2s source = 0.5s segment
        let sources = source_440();
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let evs = &parts[0].1;
        // 2s clip / 0.5s segment = 4 seamless repetitions.
        assert_eq!(evs.len(), 4);
        for (k, ev) in evs.iter().enumerate() {
            assert!((ev.onset - k as f64 * 0.5).abs() < 1e-9, "tiled back-to-back");
            assert!((ev.source_pos - 0.5).abs() < 1e-9, "starts at loop_start");
            assert!((ev.duration - 0.5).abs() < 1e-6);
        }
        // Pitching up shortens each repetition (plays faster), so more fit.
        p.rows[r].grains.pitch = 12.0;
        let parts2 = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        assert_eq!(parts2[0].1.len(), 8, "octave up = half-length repeats");
    }

    #[test]
    fn clip_automation_overrides_row_params() {
        let p = four_on_the_floor();
        let sources = source_440();
        let mut lane = crate::auto::AutomationLane::new("gain");
        lane.set_point(0.0, 1.0);
        lane.set_point(4.0, 0.0); // fade out over the 4-beat clip
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[lane]);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 4);
        assert!(evs[0].gain > evs[3].gain + 0.5, "gain fades: {:?}",
            evs.iter().map(|e| e.gain).collect::<Vec<_>>());
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
