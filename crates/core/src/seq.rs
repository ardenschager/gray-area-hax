//! Step sequencer (Koala / Nanoloop style): patterns of rows, each row a
//! sampler pad (source + grain preset + key), each step a beat-quantized
//! trigger. Patterns live in the project and are placed on the timeline as
//! pattern clips, looping to fill the clip. A triggered step is ONE grain
//! event, so every hit gets the full AV correspondence.

use crate::dsp::Rng;
use crate::grain::{ChordVoicing, GrainEvent, GrainSettings, VoiceSculpt};
use crate::music::Key;
use crate::timeline::{Source, SourceId};
use serde::{Deserialize, Serialize};

/// One step of a row: on/off, semitone offset, gain multiplier, plus the
/// chop-sequencing extras: which slice of the source it plays, how long,
/// how likely it is to fire, Aphex-style ratcheting, and chord voicings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub on: bool,
    pub pitch: f32,
    pub gain: f32,
    /// Read position 0..1 in the source for this hit (the slice it
    /// triggers); negative = the row's own position.
    pub slice: f32,
    /// Hit length as a multiple of the row's grain duration (1 = as set).
    pub length: f32,
    /// Probability the step fires, 0..1.
    pub chance: f32,
    /// Micro-timing nudge in fractions of a step (-0.5..0.5; positive = late,
    /// negative = early) for unquantized / drunken pocket.
    pub offset: f32,
    /// Pass-modulo period (`1` = every pass, `2` = every 2nd pass, etc.).
    pub every_n: u8,
    /// Which 0-based pass within `0..every_n` triggers this step.
    pub every_offset: u8,
    /// Play this slice of the row's source (index into `Source::slices`):
    /// position = slice start, length = the slice (`length` scales it).
    /// Takes precedence over `slice` when the source has that slice.
    pub slice_index: Option<u16>,
    /// Sub-step ratchet count (`1..=8`, default `1`): subdivides the step
    /// into `ratchet` rapid sub-hits. Odd sub-hits set `VoiceTag::ratchet_odd`
    /// so the stage compositor mirrors `flipX` on Aphex rolls.
    pub ratchet: u8,
    /// Semitone ramp per ratchet sub-hit (`0.0` default; `+1.0` / `-2.0` for
    /// rising or diving Aphex drill rolls).
    pub ratchet_pitch: f32,
    /// Optional per-step polyphonic chord override (`ChordVoicing::Off` uses
    /// the row/grain sculptor's chord setting).
    pub chord: ChordVoicing,
}

impl Default for Step {
    fn default() -> Self {
        Step {
            on: false,
            pitch: 0.0,
            gain: 1.0,
            slice: -1.0,
            length: 1.0,
            chance: 1.0,
            offset: 0.0,
            every_n: 1,
            every_offset: 0,
            slice_index: None,
            ratchet: 1,
            ratchet_pitch: 0.0,
            chord: ChordVoicing::Off,
        }
    }
}

/// How a row makes sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RowMode {
    /// Beat-quantized step triggers. The row cycles its OWN step count at
    /// the pattern's step rate — a 12-step row against a 16-step pattern
    /// drifts in and out of phase (polymeter).
    #[default]
    Steps,
    /// Not sequenced: the row loops a start..stop segment of its source,
    /// repeating seamlessly for the whole clip (or asynchronously in real
    /// seconds when `sculpt.free_loop_secs` is set).
    Loop,
}

/// A sampler pad row: a source, how its hits sound/look, and its steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// Silenced (the lane header's tap-to-mute).
    pub muted: bool,
    /// Optional per-row swing override (0.25..0.75; None = use pattern.swing).
    pub swing: Option<f32>,
    /// Deterministic timing humanize jitter in seconds (0 = tight).
    pub humanize: f32,
    /// MPC choke group (`0 = off`, `1..=4`). Triggering a hit in group `g > 0`
    /// mutes prior voices in group `g` with a click-free `3 ms` cosine release.
    pub choke_group: u8,
    /// Optional per-row continuous voice sculptor / tape-loop physics.
    pub sculpt: VoiceSculpt,
    /// Composite this row's hits as `Layout::Collage` foreground cutouts / zine photo-cards.
    #[serde(default)]
    pub collage_cutout: bool,
}

impl Default for SeqRow {
    fn default() -> Self {
        SeqRow::new(0, 16)
    }
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
            muted: false,
            swing: None,
            humanize: 0.0,
            choke_group: 0,
            sculpt: VoiceSculpt::default(),
            collage_cutout: false,
        }
    }

    /// Set this row's own step count (polymeter against the pattern grid).
    pub fn set_steps(&mut self, n: usize) {
        self.steps.resize(n.clamp(1, 128), Step::default());
    }
}

/// A step pattern: a grid of rows x steps covering `length_beats`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StepPattern {
    pub name: String,
    pub steps_per_beat: u32,
    pub length_beats: f64,
    /// Shuffle / swing ratio: 0.5 = straight, 0.58..0.64 = classic MPC/UKG
    /// shuffle delaying odd steps by `(swing - 0.5) * 2 * step_secs`.
    pub swing: f32,
    pub rows: Vec<SeqRow>,
}

impl Default for StepPattern {
    fn default() -> Self {
        StepPattern::new("pattern")
    }
}

impl StepPattern {
    pub fn new(name: &str) -> StepPattern {
        StepPattern {
            name: name.to_string(),
            steps_per_beat: 4,
            length_beats: 4.0,
            swing: 0.5,
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

    /// Seed an authentic skipping **134 BPM 2-step UKG groove** across this pattern's
    /// rows: sets `self.swing = 0.61` (`61%` UKG shuffle) and programs each row based
    /// on `row.sculpt.stamp` (or row index when `Stamp::Raw`):
    /// - `Kick`: skipping 2-step (`0, 10` + ghost `7` every 2nd bar)
    /// - `Snare`: crisp rim/snare (`4, 12` + turnaround ghost `15` every 4th bar)
    /// - `VoxPerc` / `Hat`: swung off-16th vocal-percussion syllables (`2, 3, 7, 9, 11, 14, 15`
    ///   with slice locks & `x2` ratchet)
    /// - `Chord`: syncopated `Min7` / `Sus4` garage vocal-organ stabs (`0, 6, 14`)
    /// - `Pluck`: deep UKG syncopated sub-bass pocket (`0, 3, 10, 13`)
    /// - `Phrase`: choked (`choke_group = 1`) absurd spoken vocal hook on bar turnarounds!
    ///
    /// If `self.rows` is currently empty and `sources` is non-empty, curates a 6-row UKG
    /// vocal kit across `sources` automatically.
    pub fn seed_ukg_2step(&mut self, sources: &[Source]) {
        use crate::grain::Stamp;
        if self.steps_per_beat != 4 || self.length_beats < 4.0 {
            self.set_grid(self.length_beats.max(4.0), 4);
        }
        self.swing = 0.61;

        if self.rows.is_empty() && !sources.is_empty() {
            let curated = crate::chop::build_ukg_vox_kit_across_sources(sources);
            let n_st = self.n_steps().max(16);
            // Pick 1 representative pad per core UKG lane: Kick, Snare, VoxPerc, Chord, Pluck, Phrase.
            for target_stamp in [
                Stamp::Kick,
                Stamp::Snare,
                Stamp::VoxPerc,
                Stamp::Chord,
                Stamp::Pluck,
                Stamp::Phrase,
            ] {
                if let Some(spec) = curated.iter().find(|p| p.stamp == target_stamp) {
                    let src_len = sources.get(spec.source).map_or(1.0, |s| s.duration());
                    let mut row = spec.to_seq_row(n_st, src_len);
                    if target_stamp == Stamp::Phrase
                        && sources
                            .get(spec.source)
                            .and_then(|s| s.video.as_deref())
                            .is_some_and(|v| v.has_alpha())
                    {
                        row.collage_cutout = true;
                    }
                    self.rows.push(row);
                }
            }
        }

        let n_st = self.n_steps().max(16);
        for (ri, row) in self.rows.iter_mut().enumerate() {
            row.mode = RowMode::Steps;
            if row.steps.len() < 16 {
                row.steps.resize(n_st, Step::default());
            } else {
                for s in &mut row.steps {
                    *s = Step::default();
                }
            }
            let raw_stamp = if row.sculpt.stamp != Stamp::Raw {
                row.sculpt.stamp
            } else {
                row.grains.sculpt.stamp
            };
            let role = if raw_stamp != Stamp::Raw {
                raw_stamp
            } else {
                match ri {
                    0 => Stamp::Kick,
                    1 => Stamp::Snare,
                    2 => Stamp::VoxPerc,
                    3 => Stamp::Chord,
                    4 => Stamp::Pluck,
                    _ => Stamp::Phrase,
                }
            };
            if !row.sculpt.is_active() {
                row.sculpt = VoiceSculpt::preset(role);
                row.grains.sculpt = row.sculpt;
            }
            if row.grains.adsr.is_none() {
                row.grains.adsr = Some(crate::envelope::Adsr {
                    attack: 0.002,
                    decay: 0.08,
                    sustain: 1.0,
                    release: 0.04,
                    picture: crate::envelope::PictureCut::Cut,
                    cut_threshold: 0.05,
                });
            }
            let n_slices = sources.get(row.source).map_or(0, |s| s.slices.len());

            match role {
                Stamp::Kick => {
                    row.choke_group = 2;
                    row.grains.duration = row.grains.duration.min(0.20);
                    row.steps[0] = Step { on: true, gain: 1.0, ..Default::default() };
                    row.steps[10] = Step { on: true, gain: 0.96, ..Default::default() };
                    row.steps[7] = Step {
                        on: true,
                        gain: 0.68,
                        every_n: 2,
                        every_offset: 1,
                        ..Default::default()
                    };
                }
                Stamp::Snare => {
                    row.grains.duration = row.grains.duration.min(0.18);
                    row.steps[4] = Step { on: true, gain: 0.95, ..Default::default() };
                    row.steps[12] = Step { on: true, gain: 0.98, ..Default::default() };
                    row.steps[15] = Step {
                        on: true,
                        gain: 0.50,
                        pitch: 1.5,
                        every_n: 4,
                        every_offset: 3,
                        ..Default::default()
                    };
                }
                Stamp::VoxPerc | Stamp::Hat => {
                    row.sculpt = VoiceSculpt::preset(Stamp::VoxPerc);
                    row.grains.sculpt = row.sculpt;
                    row.grains.duration = 0.042;
                    row.choke_group = 3;
                    let vox_steps: [(usize, f32, f32, u8); 7] = [
                        (2, 0.84, 0.0, 1),
                        (3, 0.64, 2.0, 1),
                        (7, 0.76, -1.0, 1),
                        (9, 0.68, 1.5, 1),
                        (11, 0.86, 0.0, 1),
                        (14, 0.80, 2.5, 2),
                        (15, 0.66, -1.5, 1),
                    ];
                    let norm_fallback = [0.11f32, 0.24, 0.37, 0.51, 0.63, 0.76, 0.88];
                    for (k, &(st_i, gain, pitch, ratch)) in vox_steps.iter().enumerate() {
                        if st_i < row.steps.len() {
                            row.steps[st_i] = Step {
                                on: true,
                                gain,
                                pitch,
                                length: 0.85,
                                slice: if n_slices == 0 { norm_fallback[k] } else { -1.0 },
                                slice_index: (n_slices > 0).then_some((k % n_slices) as u16),
                                ratchet: ratch,
                                ratchet_pitch: if ratch > 1 { 2.0 } else { 0.0 },
                                ..Default::default()
                            };
                        }
                    }
                }
                Stamp::Chord => {
                    row.choke_group = 4;
                    if row.sculpt.chord == ChordVoicing::Off {
                        row.sculpt.chord = ChordVoicing::Min7;
                    }
                    row.grains.duration = row.grains.duration.clamp(0.22, 0.42);
                    row.steps[0] = Step {
                        on: true,
                        gain: 0.86,
                        length: 0.90,
                        chord: ChordVoicing::Min7,
                        ..Default::default()
                    };
                    row.steps[6] = Step {
                        on: true,
                        gain: 0.82,
                        pitch: 3.0,
                        length: 0.75,
                        chord: ChordVoicing::Min7,
                        ..Default::default()
                    };
                    row.steps[14] = Step {
                        on: true,
                        gain: 0.78,
                        pitch: -2.0,
                        length: 0.70,
                        chord: ChordVoicing::Sus4,
                        ..Default::default()
                    };
                }
                Stamp::Pluck => {
                    if row.sculpt.sub_exciter < 0.2 {
                        row.sculpt.pitch_semitones = -12.0;
                        row.sculpt.sub_exciter = 0.75;
                        row.sculpt.lpf_hz = 260.0;
                    }
                    row.grains.duration = row.grains.duration.clamp(0.18, 0.34);
                    for (st_i, pt, gn) in [(0usize, 0.0f32, 0.92f32), (3, 3.0, 0.80), (10, 0.0, 0.90), (13, -2.0, 0.84)] {
                        if st_i < row.steps.len() {
                            row.steps[st_i] = Step {
                                on: true,
                                pitch: pt,
                                gain: gn,
                                ..Default::default()
                            };
                        }
                    }
                }
                Stamp::Phrase | Stamp::Raw | Stamp::Wash | Stamp::TapeLoop | Stamp::Disintegrate => {
                    row.sculpt = VoiceSculpt::preset(Stamp::Phrase);
                    row.grains.sculpt = row.sculpt;
                    row.choke_group = 1;
                    row.grains.duration = row.grains.duration.clamp(0.65, 1.45);
                    if sources
                        .get(row.source)
                        .and_then(|s| s.video.as_deref())
                        .is_some_and(|v| v.has_alpha())
                    {
                        row.collage_cutout = true;
                    }
                    let hook_slice = sources.get(row.source).and_then(|src| {
                        src.slices
                            .iter()
                            .enumerate()
                            .filter(|(_, sl)| sl.len() >= 0.45)
                            .max_by(|(_, a), (_, b)| a.bands[2].total_cmp(&b.bands[2]))
                            .map(|(idx, _)| idx as u16)
                    });
                    // Choked stutter teaser on step 10 + full spoken hook on bar turnaround (step 12)!
                    if 10 < row.steps.len() {
                        row.steps[10] = Step {
                            on: true,
                            gain: 0.88,
                            length: 0.22,
                            slice_index: hook_slice,
                            every_n: 2,
                            every_offset: 1,
                            ..Default::default()
                        };
                    }
                    if 12 < row.steps.len() {
                        row.steps[12] = Step {
                            on: true,
                            gain: 0.98,
                            length: 1.0,
                            slice_index: hook_slice,
                            every_n: 2,
                            every_offset: 1,
                            ..Default::default()
                        };
                    }
                }
            }
        }
    }
}

/// Output length of a step hit: a chopped slice plays its whole slice (at
/// the hit's pitch ratio) times `step.length`; otherwise the row's grain
/// duration times `step.length`.
pub fn hit_duration(g: &GrainSettings, step: &Step, chop: Option<crate::chop::Slice>, ratio: f32) -> f32 {
    let len = step.length.max(0.05);
    match chop {
        Some(sl) => ((sl.len() / ratio.max(0.01) as f64) as f32 * len).clamp(0.01, 30.0),
        None => (g.duration * len).clamp(0.01, 5.0),
    }
}

/// Voice id of one step hit.
pub fn hit_voice(row: usize, slot: u64, clip_start: f64) -> u64 {
    let c = (clip_start * 1000.0).round() as u64;
    0xA000_0000_0000_0000 | ((row as u64 & 0xFF) << 52) | ((c & 0xF_FFFF) << 32) | (slot & 0xFFFF_FFFF)
}

/// A row ADSR makes each hit a voice: gate = the hit length, then the
/// release (the grain grows by it); the grain's own window goes flat so
/// the envelope shapes sound and picture.
pub fn apply_hit_adsr(ev: &mut GrainEvent, g: &GrainSettings, voice: u64) {
    if let Some(adsr) = g.adsr {
        let off = ev.onset + ev.duration as f64;
        ev.duration += adsr.release;
        ev.envelope = 0.01;
        ev.env_skew = 0.0;
        ev.tag.voice = voice;
        ev.tag.env = Some(crate::envelope::VoiceEnv { adsr, on: ev.onset, off });
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
        if row.muted {
            continue;
        }
        let source_len = source.duration();
        if source_len <= 0.0 || row.steps.is_empty() {
            continue;
        }
        let base_hz = source.effective_base_hz();
        let key = key_override.or(row.key.as_ref());
        let mut rng = Rng::new(row.grains.seed ^ ((row_idx as u64 + 1) * 0x9E37));
        // Chance and humanize rolls use dedicated streams so they never shift
        // the musical spray/pitch/pan jitter of existing steps.
        let mut chance_rng = Rng::new(row.grains.seed ^ ((row_idx as u64 + 1) * 0xC4A5_CE00));
        let mut hum_rng = Rng::new(row.grains.seed ^ ((row_idx as u64 + 1) * 0x4041_5E00));
        let eff_swing = row.swing.unwrap_or(pattern.swing).clamp(0.25, 0.75) as f64;
        let mut events = Vec::new();
        let mut id: u64 = (row_idx as u64) << 32;

        let eff_at = |t: f64| -> GrainSettings {
            if lanes.is_empty() {
                row.grains.clone()
            } else {
                crate::auto::settings_at(&row.grains, lanes, t / secs_per_beat.max(1e-9))
            }
        };

        let eff_sculpt_for = |g: &GrainSettings| -> VoiceSculpt {
            if row.sculpt.is_active() { row.sculpt } else { g.sculpt }
        };
        let eff_choke_for = |g: &GrainSettings| -> u8 {
            if row.choke_group > 0 {
                row.choke_group.min(4)
            } else {
                g.choke_group.min(4)
            }
        };

        match row.mode {
            RowMode::Steps => {
                let row_collage_fg = row.collage_cutout
                    || (matches!(row.sculpt.stamp, crate::grain::Stamp::Phrase)
                        && source.video.as_deref().is_some_and(|v| v.has_alpha()));
                let mut prev_hit_t: Option<f64> = None;
                let mut prev_slice: Option<u16> = None;
                let mut prev_salt: u8 = 0;
                // Slot k cycles this row's own step count — rows shorter or
                // longer than the pattern grid phase against it (polymeter).
                let mut slot = 0usize;
                loop {
                    let grid_t = slot as f64 * step_secs;
                    if grid_t >= clip_len_secs {
                        break;
                    }
                    let pass_idx = slot / row.steps.len();
                    let step_idx = slot % row.steps.len();
                    let step = row.steps[step_idx];
                    // Consume jitter deterministically for EVERY slot so
                    // toggling one step doesn't reshuffle the others.
                    let spray_j = rng.bipolar();
                    let pitch_j = rng.bipolar();
                    let pan_j = rng.bipolar();
                    let rev_j = rng.next_f32();
                    let roll = chance_rng.next_f32();
                    let hum_j = hum_rng.bipolar();
                    let is_odd = slot % 2 == 1;
                    slot += 1;
                    let every = step.every_n.max(1) as usize;
                    let pass_ok = every == 1 || (pass_idx % every) == (step.every_offset as usize % every);
                    if !step.on || !pass_ok || roll >= step.chance {
                        continue;
                    }
                    let swing_shift = if is_odd { (eff_swing - 0.5) * 2.0 * step_secs } else { 0.0 };
                    let nudge = step.offset.clamp(-0.5, 0.5) as f64 * step_secs;
                    let hum = row.humanize.clamp(0.0, 0.1) as f64 * hum_j as f64;
                    let t = (grid_t + swing_shift + nudge + hum)
                        .clamp(0.0, (clip_len_secs - 1e-6).max(0.0));
                    let g = eff_at(t);

                    let chop = step.slice_index.and_then(|i| source.slices.get(i as usize).copied());
                    let at = if step.slice >= 0.0 { step.slice.min(1.0) } else { g.position };
                    let mut pos = match chop {
                        Some(sl) => sl.start,
                        None => at as f64 * source_len + g.spray as f64 * spray_j as f64,
                    };
                    pos = pos.rem_euclid(source_len.max(1e-6));
                    let step_base_hz = if key.is_some() && (step.slice >= 0.0 || g.position > 0.02) {
                        source.base_hz_at(pos)
                    } else {
                        base_hz
                    };

                    let mut sculpt = eff_sculpt_for(&g);
                    if step.chord != ChordVoicing::Off {
                        sculpt.chord = step.chord;
                    }
                    let choke_group = eff_choke_for(&g);
                    let focus = chop.map_or((0.5, 0.5), |sl| sl.focus);
                    let caption = chop.map_or_else(crate::chop::Caption::default, |sl| sl.caption);
                    let zoom_punch = if sculpt.sub_exciter > 0.2 || sculpt.snare_snap > 0.2 {
                        0.08
                    } else if chop.is_some_and(|sl| sl.role_hint == crate::chop::SliceRole::Phrase) {
                        0.05
                    } else {
                        0.0
                    };

                    let r_count = step.ratchet.clamp(1, 8);
                    let sub_step_secs = step_secs / r_count as f64;
                    let rapid_chop = prev_hit_t.is_some_and(|pt| (t - pt) < 0.22);
                    let step_base_salt: u8 = if sculpt.stamp == crate::grain::Stamp::VoxPerc {
                        let mut s = step
                            .slice_index
                            .map_or((step_idx & 1) as u8, |si| (si & 3) as u8);
                        if rapid_chop && s == prev_salt {
                            s = (prev_salt + 1) & 3;
                        }
                        s
                    } else if rapid_chop
                        && step.slice_index.is_some()
                        && step.slice_index != prev_slice
                    {
                        let mut s = (step.slice_index.unwrap_or(0) & 3) as u8;
                        if s == prev_salt {
                            s = (prev_salt + 1) & 3;
                        }
                        s
                    } else {
                        0
                    };
                    prev_hit_t = Some(t);
                    prev_slice = step.slice_index;
                    prev_salt = step_base_salt;

                    for sub in 0..r_count {
                        let sub_t = t + sub as f64 * sub_step_secs;
                        if sub_t >= clip_len_secs {
                            break;
                        }
                        let st = g.pitch
                            + sculpt.pitch_semitones
                            + step.pitch
                            + g.pitch_jitter * pitch_j
                            + sub as f32 * step.ratchet_pitch;
                        let ratio = crate::music::effective_ratio(
                            st,
                            key,
                            step_base_hz,
                            g.quantize_amount,
                            g.detune_cents,
                        );
                        let mut dur = hit_duration(&g, &step, chop, ratio);
                        if r_count > 1 {
                            dur = dur.min((sub_step_secs * 0.92).max(0.01) as f32);
                        }
                        if sculpt.stamp == crate::grain::Stamp::VoxPerc {
                            dur = dur.clamp(0.028, 0.052);
                        }
                        let sub_gain = if r_count > 1 {
                            0.88f32.powi(sub as i32)
                        } else {
                            1.0
                        };
                        let comb_hz = if sculpt.comb_mix > 1e-3 {
                            crate::grain::quantize_comb_hz(step_base_hz.max(40.0) * ratio, key)
                        } else {
                            0.0
                        };
                        let voice_slot = if r_count == 1 {
                            slot as u64
                        } else {
                            (slot as u64).wrapping_mul(8).wrapping_add(sub as u64)
                        };
                        let cell_salt = if r_count > 1 { sub as u8 } else { step_base_salt };
                        let is_short_perc = sculpt.stamp == crate::grain::Stamp::VoxPerc
                            || matches!(
                                sculpt.stamp,
                                crate::grain::Stamp::Kick
                                    | crate::grain::Stamp::Snare
                                    | crate::grain::Stamp::Hat
                            )
                            || sculpt.sub_exciter > 0.1
                            || sculpt.snare_snap > 0.1;
                        let visual_hold = if r_count > 1 {
                            (((r_count - sub) as f64 * sub_step_secs * 0.96)
                                .max(dur.clamp(0.065, 0.14) as f64)) as f32
                        } else if is_short_perc && dur <= 0.14 {
                            dur.clamp(0.065, 0.14)
                        } else {
                            0.0
                        };
                        let hit_env = if is_short_perc || sculpt.comb_mix > 0.2 {
                            g.envelope.min(0.04)
                        } else {
                            g.envelope
                        };
                        let mut ev = GrainEvent {
                            onset: clip_start_secs + sub_t,
                            source_pos: pos,
                            duration: dur,
                            pitch_ratio: ratio,
                            gain: g.gain * step.gain * sub_gain,
                            pan: (g.pan + g.pan_spread * pan_j).clamp(-1.0, 1.0),
                            envelope: hit_env,
                            env_skew: g.env_skew,
                            reverse: rev_j < g.reverse_prob,
                            id,
                            sculpt,
                            comb_hz,
                            loop_pass: pass_idx as u32,
                            ..Default::default()
                        };
                        let h_voice = hit_voice(row_idx, voice_slot, clip_start_secs);
                        ev.tag.voice = h_voice;
                        apply_hit_adsr(&mut ev, &g, h_voice);
                        ev.tag.voice = h_voice;
                        ev.tag.cell_salt = cell_salt;
                        ev.tag.visual_hold = visual_hold;
                        ev.tag.collage_cutout = row_collage_fg;
                        ev.tag.focus = focus;
                        ev.tag.caption = caption;
                        ev.tag.ratchet_odd = sub % 2 == 1;
                        ev.tag.zoom_punch = zoom_punch;
                        ev.tag.choke_group = choke_group;
                        ev.tag.is_wash = sculpt.is_wash();
                        if sculpt.disintegrate > 1e-3 {
                            let ox = crate::grain::oxide_dropout_at(
                                sculpt.disintegrate,
                                pass_idx as u32,
                                0.5,
                                id ^ row.grains.seed,
                            );
                            ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
                        }
                        if sculpt.ebb_flow > 1e-3 {
                            ev.gain *= crate::grain::ebb_flow_gain_at(
                                sculpt.ebb_flow,
                                pass_idx as u32,
                                ev.onset,
                                id ^ row.grains.seed,
                            );
                        }
                        if sculpt.chord != ChordVoicing::Off {
                            let before_len = events.len();
                            crate::grain::expand_chord_grain(
                                &ev,
                                sculpt.chord,
                                step_base_hz,
                                key,
                                &mut events,
                            );
                            id += (events.len() - before_len) as u64;
                        } else {
                            events.push(ev);
                            id += 1;
                        }
                    }
                }
            }
            RowMode::Loop => {
                // Not sequenced: repeat the start..stop source segment
                // seamlessly for the whole clip, or asynchronously in real
                // seconds when `sculpt.free_loop_secs` is set.
                let s0 = row.loop_start.clamp(0.0, 1.0) as f64;
                let s1 = row.loop_end.clamp(0.0, 1.0) as f64;
                let seg_src = ((s1 - s0) * source_len).max(0.02);
                let base_sculpt = eff_sculpt_for(&row.grains);
                if let Some(free_secs) = base_sculpt.free_loop_secs.filter(|s| *s > 0.05) {
                    let period = (free_secs as f64).clamp(0.05, 120.0);
                    let seam = 0.08_f64.min(period * 0.25);
                    let mut t = 0.0f64;
                    let mut pass_k = 0u32;
                    while t < clip_len_secs {
                        let pan_j = rng.bipolar();
                        let g = eff_at(t);
                        let sculpt = eff_sculpt_for(&g);
                        let choke_group = eff_choke_for(&g);
                        let ratio = crate::music::effective_ratio(
                            g.pitch + sculpt.pitch_semitones,
                            key,
                            base_hz,
                            g.quantize_amount,
                            g.detune_cents,
                        );
                        let rem = clip_len_secs - t;
                        let dur_out = if rem > period + seam {
                            period + seam
                        } else {
                            rem.max(0.02)
                        };
                        let env_taper = ((2.0 * seam) / dur_out).clamp(0.02, 0.90) as f32;
                        let ebb = crate::grain::ebb_flow_gain_at(
                            sculpt.ebb_flow,
                            pass_k,
                            clip_start_secs + t,
                            row.grains.seed ^ (row_idx as u64),
                        );
                        let ox = crate::grain::oxide_dropout_at(
                            sculpt.disintegrate,
                            pass_k,
                            0.5,
                            id ^ row.grains.seed,
                        );
                        let comb_hz = if sculpt.comb_mix > 1e-3 {
                            crate::grain::quantize_comb_hz(base_hz.max(40.0) * ratio, key)
                        } else {
                            0.0
                        };
                        let mut ev = GrainEvent {
                            onset: clip_start_secs + t,
                            source_pos: (s0 * source_len).min(source_len),
                            duration: dur_out as f32,
                            pitch_ratio: ratio,
                            gain: g.gain * ebb,
                            pan: (g.pan + g.pan_spread * pan_j).clamp(-1.0, 1.0),
                            envelope: env_taper,
                            env_skew: 0.0,
                            reverse: g.reverse_prob >= 0.5,
                            id,
                            sculpt,
                            comb_hz,
                            loop_pass: pass_k,
                            ..Default::default()
                        };
                        ev.tag.is_wash = true;
                        ev.tag.choke_group = choke_group;
                        ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
                        if sculpt.chord != ChordVoicing::Off {
                            let before_len = events.len();
                            crate::grain::expand_chord_grain(
                                &ev,
                                sculpt.chord,
                                base_hz,
                                key,
                                &mut events,
                            );
                            id += (events.len() - before_len) as u64;
                        } else {
                            events.push(ev);
                            id += 1;
                        }
                        t += period;
                        pass_k += 1;
                    }
                } else {
                    let mut t = 0.0f64;
                    let mut pass_k = 0u32;
                    while t < clip_len_secs {
                        let pan_j = rng.bipolar();
                        let g = eff_at(t);
                        let sculpt = eff_sculpt_for(&g);
                        let choke_group = eff_choke_for(&g);
                        let ratio = crate::music::effective_ratio(
                            g.pitch + sculpt.pitch_semitones,
                            key,
                            base_hz,
                            g.quantize_amount,
                            g.detune_cents,
                        );
                        // Playing the segment at `ratio` takes seg/ratio output
                        // seconds; repetitions tile back to back.
                        let dur_out = (seg_src / ratio.max(0.01) as f64).clamp(0.02, 60.0);
                        let ebb = if sculpt.ebb_flow > 1e-3 {
                            crate::grain::ebb_flow_gain_at(
                                sculpt.ebb_flow,
                                pass_k,
                                clip_start_secs + t,
                                row.grains.seed ^ (row_idx as u64),
                            )
                        } else {
                            1.0
                        };
                        let ox = crate::grain::oxide_dropout_at(
                            sculpt.disintegrate,
                            pass_k,
                            0.5,
                            id ^ row.grains.seed,
                        );
                        let comb_hz = if sculpt.comb_mix > 1e-3 {
                            crate::grain::quantize_comb_hz(base_hz.max(40.0) * ratio, key)
                        } else {
                            0.0
                        };
                        let mut ev = GrainEvent {
                            onset: clip_start_secs + t,
                            source_pos: (s0 * source_len).min(source_len),
                            duration: dur_out as f32,
                            pitch_ratio: ratio,
                            gain: g.gain * ebb,
                            pan: (g.pan + g.pan_spread * pan_j).clamp(-1.0, 1.0),
                            envelope: g.envelope,
                            env_skew: g.env_skew,
                            reverse: g.reverse_prob >= 0.5,
                            id,
                            sculpt,
                            comb_hz,
                            loop_pass: pass_k,
                            ..Default::default()
                        };
                        ev.tag.choke_group = choke_group;
                        ev.tag.is_wash = sculpt.is_wash();
                        ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
                        if sculpt.chord != ChordVoicing::Off {
                            let before_len = events.len();
                            crate::grain::expand_chord_grain(
                                &ev,
                                sculpt.chord,
                                base_hz,
                                key,
                                &mut events,
                            );
                            id += (events.len() - before_len) as u64;
                        } else {
                            events.push(ev);
                            id += 1;
                        }
                        t += dur_out;
                        pass_k += 1;
                    }
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
            ..Default::default()
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
    fn steps_play_their_slice_length_and_chance() {
        let mut p = four_on_the_floor();
        p.rows[0].steps[4].slice = 0.5;
        p.rows[0].steps[4].length = 2.0;
        p.rows[0].steps[8].chance = 0.0;
        let sources = source_440();
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 3, "the chance-0 step never fires");
        let d = p.rows[0].grains.duration;
        assert!((evs[1].source_pos - 1.0).abs() < 1e-6, "slice 0.5 of a 2 s source");
        assert!((evs[1].duration - 2.0 * d).abs() < 1e-6);
        assert!((evs[0].duration - d).abs() < 1e-6);
        // Chance doesn't reshuffle the other steps' jitter.
        let base = pattern_events(&four_on_the_floor(), &sources, 0.0, 2.0, 0.5, None, &[]);
        assert_eq!(base[0].1[0].pan, evs[0].pan);
        assert_eq!(base[0].1[3].pan, evs[2].pan);
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

    #[test]
    fn step_every_fires_only_on_matching_passes() {
        let mut p = StepPattern::new("cond");
        let r = p.add_row(0); // 16 steps = 4 beats = 2.0 s @ 120 BPM
        p.rows[r].steps[0].on = true;
        p.rows[r].steps[0].every_n = 2;
        p.rows[r].steps[0].every_offset = 1; // fire on pass 1, 3, ... (skip pass 0, 2)
        let sources = source_440();
        let parts = pattern_events(&p, &sources, 0.0, 8.0, 0.5, None, &[]); // 4 passes
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 2);
        assert!((evs[0].onset - 2.0).abs() < 1e-6);
        assert!((evs[1].onset - 6.0).abs() < 1e-6);
    }

    #[test]
    fn steps_play_chop_slices_by_index() {
        let mut sources = source_440();
        sources[0].slices = crate::chop::equal_slices(2.0, 4); // 0.5 s each
        let mut p = four_on_the_floor();
        p.rows[0].steps[4].slice_index = Some(2);
        p.rows[0].steps[8].slice_index = Some(9); // missing: falls back
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 4);
        assert!((evs[1].source_pos - 1.0).abs() < 1e-6, "slice 2 starts at 1.0 s");
        let r = evs[1].pitch_ratio as f64;
        assert!((evs[1].duration as f64 - 0.5 / r).abs() < 1e-4, "plays the slice length: {}", evs[1].duration);
        let d = p.rows[0].grains.duration;
        assert!((evs[2].duration - d).abs() < 1e-6, "no slice: the row grain length");
        assert!(evs.iter().all(|e| e.tag.env.is_none()), "no ADSR configured");
    }

    #[test]
    fn row_adsr_gates_each_hit() {
        let mut p = four_on_the_floor();
        let adsr = crate::envelope::Adsr { attack: 0.01, decay: 0.0, sustain: 1.0, release: 0.2, ..Default::default() };
        p.rows[0].grains.adsr = Some(adsr);
        let sources = source_440();
        let plain = pattern_events(&four_on_the_floor(), &sources, 0.0, 2.0, 0.5, None, &[]);
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let (a, b) = (&plain[0].1, &parts[0].1);
        assert_eq!(a.len(), b.len());
        let mut voices = std::collections::BTreeSet::new();
        for (x, y) in a.iter().zip(b) {
            let env = y.tag.env.expect("each hit carries its envelope");
            assert_eq!(env.on, y.onset);
            assert!((env.off - (x.onset + x.duration as f64)).abs() < 1e-9, "gate = the hit length");
            assert!((y.duration - (x.duration + 0.2)).abs() < 1e-6, "the grain grows by the release");
            assert!(y.tag.voice != 0);
            voices.insert(y.tag.voice);
        }
        assert_eq!(voices.len(), b.len(), "one voice per hit");

        let mut ev = GrainEvent { onset: 1.0, duration: 0.1, envelope: 0.5, ..Default::default() };
        apply_hit_adsr(&mut ev, &p.rows[0].grains, 7);
        assert_eq!((ev.envelope, ev.tag.voice), (0.01, 7));
        assert!((ev.tag.env.unwrap().off - 1.1).abs() < 1e-6);
        assert_ne!(hit_voice(0, 1, 0.0), hit_voice(1, 1, 0.0));
        assert_ne!(hit_voice(0, 1, 0.0), hit_voice(0, 1, 2.0));
        let step = Step { length: 2.0, ..Step::default() };
        assert!((hit_duration(&p.rows[0].grains, &step, Some(crate::chop::Slice::new(0.0, 1.0)), 2.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ratchet_subdivides_step_ramps_pitch_and_marks_odd_flips() {
        let mut p = StepPattern::new("aphex");
        let r = p.add_row(0);
        p.rows[r].steps[0] = Step {
            on: true,
            ratchet: 4,
            ratchet_pitch: 2.0,
            ..Step::default()
        };
        let sources = source_440();
        // 120 BPM -> 0.5s/beat -> 0.125s per 16th step -> 0.03125s per ratchet sub-hit.
        let parts = pattern_events(&p, &sources, 0.0, 2.0, 0.5, None, &[]);
        let evs = &parts[0].1;
        assert_eq!(evs.len(), 4);
        for (sub, ev) in evs.iter().enumerate() {
            assert!((ev.onset - sub as f64 * 0.03125).abs() < 1e-9);
            assert_eq!(ev.tag.ratchet_odd, sub % 2 == 1);
            if sub > 0 {
                assert!(ev.pitch_ratio > evs[sub - 1].pitch_ratio, "ratchet_pitch ramps up");
            }
        }
    }

    #[test]
    fn free_loop_secs_loops_asynchronously_in_real_seconds_without_seam_clicks() {
        let mut p = StepPattern::new("basinski");
        let r = p.add_row(0);
        p.rows[r].mode = RowMode::Loop;
        p.rows[r].sculpt = VoiceSculpt {
            free_loop_secs: Some(0.47),
            disintegrate: 0.65,
            ebb_flow: 0.30,
            ..VoiceSculpt::preset(crate::grain::Stamp::TapeLoop)
        };
        let sources = source_440();
        // Compare 90 BPM vs 150 BPM over 2.0 real seconds: onsets stay locked to 0.47s real seconds!
        let parts_90 = pattern_events(&p, &sources, 0.0, 2.0, 60.0 / 90.0, None, &[]);
        let parts_150 = pattern_events(&p, &sources, 0.0, 2.0, 60.0 / 150.0, None, &[]);
        let evs = &parts_90[0].1;
        assert_eq!(evs.len(), parts_150[0].1.len());
        for (k, (a, b)) in evs.iter().zip(&parts_150[0].1).enumerate() {
            assert!((a.onset - k as f64 * 0.47f32 as f64).abs() < 1e-6);
            assert!((a.onset - b.onset).abs() < 1e-9, "BPM-independent real-seconds loop");
            assert_eq!(a.loop_pass, k as u32);
        }
        // Render across the 0.47s seam and verify smooth continuity (no seam jump/click).
        let sr = 48_000u32;
        let mut buf = crate::audio::StereoBuffer::new(1.2, sr);
        crate::audio::render_grains_audio(sources[0].audio.as_deref().unwrap(), evs, &mut buf, 0.0);
        let s_a = (0.44 * sr as f64) as usize;
        let s_b = (0.58 * sr as f64) as usize;
        let max_jump = buf.left[s_a..s_b]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_jump < 0.12, "80ms crossfaded seam is click-free: max_jump={max_jump}");
    }

    #[test]
    fn step_pattern_kick_and_hat_apply_sculpt_pitch_and_studio_weight_sub() {
        let sr = 48_000u32;
        // 0.75-peak 380 Hz exponentially decaying desk tap (`tau = 25ms`).
        let mut tap = vec![0.0f32; (0.5 * sr as f64) as usize];
        for (i, s) in tap.iter_mut().enumerate() {
            let t = i as f64 / sr as f64;
            *s = 0.75 * (std::f64::consts::TAU * 380.0 * t).sin() as f32 * (-t / 0.025).exp() as f32;
        }
        let sources = vec![Source {
            name: "desk_tap".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::new(tap, sr))),
            base_hz: 380.0,
            ..Default::default()
        }];
        let mut pat = StepPattern::new("tap_kit");
        let r_kick = pat.add_row(0);
        pat.rows[r_kick].sculpt = VoiceSculpt::preset(crate::grain::Stamp::Kick);
        pat.rows[r_kick].steps[0].on = true;

        let r_hat = pat.add_row(0);
        pat.rows[r_hat].sculpt = VoiceSculpt::preset(crate::grain::Stamp::Hat);
        pat.rows[r_hat].steps[4].on = true;

        let parts = pattern_events(&pat, &sources, 0.0, 1.0, 0.5, None, &[]);
        let kick_ev = &parts[0].1[0];
        let hat_ev = &parts[1].1[0];
        assert!(
            (kick_ev.pitch_ratio - crate::music::semitones_to_ratio(-14.0)).abs() < 1e-4,
            "Stamp::Kick -14st applied in StepPattern: {}",
            kick_ev.pitch_ratio
        );
        assert!(
            (hat_ev.pitch_ratio - crate::music::semitones_to_ratio(10.0)).abs() < 1e-4,
            "Stamp::Hat +10st applied in StepPattern: {}",
            hat_ev.pitch_ratio
        );

        let mut buf = crate::audio::StereoBuffer::new(0.45, sr);
        crate::audio::render_grains_audio(
            sources[0].audio.as_deref().unwrap(),
            std::slice::from_ref(kick_ev),
            &mut buf,
            0.0,
        );
        let peak = buf.left.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        assert!(
            (0.24..=0.72).contains(&peak),
            "StepPattern Stamp::Kick on 0.75-peak 380Hz tap hits healthy drum level (-12..-3 dBFS), got peak={peak:.4}"
        );
        let mut lp = crate::dsp::Biquad::new(crate::dsp::FilterKind::LowPass, sr as f32, 95.0, 0.707);
        let mut sub_only = buf.left[..(0.25 * sr as f64) as usize].to_vec();
        lp.process_buffer(&mut sub_only);
        let sub_rms = (sub_only.iter().map(|x| x * x).sum::<f32>() / sub_only.len() as f32).sqrt();
        assert!(sub_rms >= 0.04, "Kick sub_exciter delivers solid <95Hz RMS >= 0.04, got {sub_rms:.4}");
    }

    #[test]
    fn voxperc_stamp_and_seed_ukg_2step_groove_and_cell_salt() {
        let sr = 48_000u32;
        // 1. Stamp::VoxPerc gates any long spoken syllable into a tight 28..52 ms UKG percussive tick
        //    with >= 220 Hz HPF so zero low-mid mud leaks through.
        let mut syllable = vec![0.0f32; sr as usize];
        for (i, s) in syllable.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            *s = 0.55 * (2.0 * std::f32::consts::PI * 110.0 * t).sin()
                + 0.55 * (2.0 * std::f32::consts::PI * 3400.0 * t).sin();
        }
        let clip = AudioClip::new(syllable, sr);
        let mut buf = crate::audio::StereoBuffer::new(0.25, sr);
        let sculpt = VoiceSculpt::preset(crate::grain::Stamp::VoxPerc);
        assert_eq!(sculpt.pitch_semitones, 2.0);
        assert!(sculpt.hpf_hz >= 220.0);
        let mut ev = GrainEvent {
            onset: 0.0,
            source_pos: 0.0,
            duration: 0.20,
            pitch_ratio: crate::music::semitones_to_ratio(sculpt.pitch_semitones),
            gain: 0.9,
            envelope: 0.08,
            ..Default::default()
        };
        ev.sculpt = sculpt;
        crate::audio::render_grains_audio(&clip, std::slice::from_ref(&ev), &mut buf, 0.0);
        let tick_end = (0.056 * sr as f64) as usize;
        let tail_peak = buf.left[tick_end..]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f32, f32::max);
        assert!(tail_peak < 1e-4, "Stamp::VoxPerc hard-gates cleanly within <=52ms: tail_peak={tail_peak}");
        let tick_peak = buf.left[..tick_end].iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        assert!(tick_peak > 0.15, "Stamp::VoxPerc delivers crisp consonant presence: {tick_peak}");

        // 2. StepPattern::seed_ukg_2step programs 61% shuffle + authentic 2-step UKG drum/voxperc/chord/hook rows.
        let mut src = Source {
            name: "vox".into(),
            audio: Some(std::sync::Arc::new(clip)),
            ..Default::default()
        };
        src.slices = vec![
            crate::chop::Slice::new(0.0, 0.04),
            crate::chop::Slice::new(0.08, 0.12),
            crate::chop::Slice::new(0.16, 0.20),
            crate::chop::Slice::new(0.24, 0.88),
        ];
        let sources = vec![src];
        let mut pat = StepPattern::new("ukg");
        for st in [
            crate::grain::Stamp::Kick,
            crate::grain::Stamp::Snare,
            crate::grain::Stamp::VoxPerc,
            crate::grain::Stamp::Chord,
            crate::grain::Stamp::Phrase,
        ] {
            let ri = pat.add_row(0);
            pat.rows[ri].sculpt = VoiceSculpt::preset(st);
        }
        pat.seed_ukg_2step(&sources);
        assert!((pat.swing - 0.61).abs() < 1e-5, "61% UKG shuffle");
        assert!(pat.rows[0].steps[0].on && pat.rows[0].steps[10].on && !pat.rows[0].steps[8].on, "2-step kick skips beat 3");
        assert!(pat.rows[1].steps[4].on && pat.rows[1].steps[12].on, "backbeat rim/snare on 4 & 12");
        assert!(pat.rows[2].steps[2].on && pat.rows[2].steps[7].on && pat.rows[2].steps[15].on, "off-16th VoxPerc shuffle");
        assert!(pat.rows[3].steps[0].on && pat.rows[3].steps[6].on && pat.rows[3].steps[14].on, "syncopated Min7 chord stabs");
        assert_eq!(pat.rows[4].choke_group, 1, "turnaround vocal hook is choked");

        // 3. Multi-cell transient subdivision (`cell_salt`) on ratchets & rapid VoxPerc steps!
        pat.rows[2].steps[2].ratchet = 4;
        let parts = pattern_events(&pat, &sources, 0.0, 8.0, 60.0 / 134.0, None, &[]);
        let vox_evs: Vec<&GrainEvent> = parts
            .iter()
            .flat_map(|(_, evs)| evs.iter())
            .filter(|ev| ev.sculpt.stamp == crate::grain::Stamp::VoxPerc && !ev.tag.chord_harmony)
            .collect();
        let ratchet_salts: std::collections::BTreeSet<u8> = vox_evs
            .iter()
            .take(4)
            .map(|e| e.tag.cell_salt)
            .collect();
        assert_eq!(ratchet_salts.len(), 4, "x4 ratchet assigns 4 distinct cell_salt values: {ratchet_salts:?}");
        for e in &vox_evs {
            assert!((0.065..=0.141).contains(&e.tag.visual_hold), "VoxPerc holds crisp visual frame: {}", e.tag.visual_hold);
        }
    }
}
