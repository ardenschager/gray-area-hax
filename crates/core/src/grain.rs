//! The heart of chromagrain: a single grain scheduler whose events drive
//! BOTH the audio renderer and the visual compositor. One `GrainEvent` is
//! one audio grain *and* one visual grain — same onset, duration, envelope,
//! playback rate, pan and gain — so what you hear is literally what you see.

use crate::dsp::Rng;
use crate::music::{semitones_to_ratio, Key};
use serde::{Deserialize, Serialize};

/// User-facing granular parameters on a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrainSettings {
    /// Grains per second.
    pub density: f32,
    /// Grain length in seconds.
    pub duration: f32,
    /// Random variation of grain length (fraction of duration, 0..1).
    pub duration_jitter: f32,
    /// Normalized read position in the source (0..1).
    pub position: f32,
    /// Random spread of the read position, in seconds.
    pub spray: f32,
    /// How fast the read head scans through the source relative to
    /// timeline time (1.0 = realtime, 0.0 = frozen).
    pub scan_speed: f32,
    /// Pitch offset in semitones (also visual playback rate / hue shift).
    pub pitch: f32,
    /// Random pitch variation in semitones.
    pub pitch_jitter: f32,
    /// Linear gain applied per grain.
    pub gain: f32,
    /// Center pan -1..1 (audio position == visual x position).
    pub pan: f32,
    /// Random pan spread around the center, 0..1.
    pub pan_spread: f32,
    /// Envelope shape 0..1 (tukey taper fraction; 1 = hann).
    pub envelope: f32,
    /// Envelope skew -1..1: -1 percussive (instant attack, long tail),
    /// 0 symmetric, +1 reverse swell. Shapes opacity too.
    pub env_skew: f32,
    /// Probability a grain plays in reverse.
    pub reverse_prob: f32,
    /// Beat-sync: when > 0, grain onsets snap to this beat grid (e.g.
    /// 0.25 = 16ths) and `density` becomes the average trigger rate.
    /// 0 = free (classic asynchronous cloud).
    pub sync_div: f32,
    /// Harmony voices per grain (1..4). Voice v plays at
    /// `pitch + v * voice_interval` semitones (then key-quantized).
    pub voices: u32,
    /// Interval between harmony voices, semitones (12 = octaves, 7 = fifths).
    pub voice_interval: f32,
    /// Random detune per voice, semitones.
    pub voice_detune: f32,
    /// RNG seed; same seed -> identical grain cloud.
    pub seed: u64,
}

impl GrainSettings {
    /// Set a parameter by name (shared by scripting and automation).
    /// Returns Err for names that are not grain parameters.
    pub fn set_param(&mut self, key: &str, value: f64) -> Result<(), ()> {
        let x = value as f32;
        match key {
            "density" => self.density = x.clamp(0.1, 500.0),
            "duration" => self.duration = x.clamp(0.005, 5.0),
            "duration_jitter" => self.duration_jitter = x.clamp(0.0, 1.0),
            "position" => self.position = x.clamp(0.0, 1.0),
            "spray" => self.spray = x.max(0.0),
            "scan_speed" => self.scan_speed = x,
            "pitch" => self.pitch = x.clamp(-48.0, 48.0),
            "pitch_jitter" => self.pitch_jitter = x.clamp(0.0, 48.0),
            "gain" => self.gain = x.clamp(0.0, 4.0),
            "pan" => self.pan = x.clamp(-1.0, 1.0),
            "pan_spread" => self.pan_spread = x.clamp(0.0, 1.0),
            "envelope" => self.envelope = x.clamp(0.01, 1.0),
            "env_skew" => self.env_skew = x.clamp(-1.0, 1.0),
            "reverse_prob" => self.reverse_prob = x.clamp(0.0, 1.0),
            "sync_div" => self.sync_div = x.clamp(0.0, 4.0),
            "voices" => self.voices = (value as i64).clamp(1, 4) as u32,
            "voice_interval" => self.voice_interval = x.clamp(-24.0, 24.0),
            "voice_detune" => self.voice_detune = x.clamp(0.0, 2.0),
            "seed" => self.seed = value as u64,
            _ => return Err(()),
        }
        Ok(())
    }
}

impl Default for GrainSettings {
    fn default() -> Self {
        GrainSettings {
            density: 20.0,
            duration: 0.12,
            duration_jitter: 0.2,
            position: 0.0,
            spray: 0.05,
            scan_speed: 1.0,
            pitch: 0.0,
            pitch_jitter: 0.0,
            gain: 0.8,
            pan: 0.0,
            pan_spread: 0.6,
            envelope: 0.6,
            env_skew: 0.0,
            reverse_prob: 0.0,
            sync_div: 0.0,
            voices: 1,
            voice_interval: 12.0,
            voice_detune: 0.0,
            seed: 0xC0FFEE,
        }
    }
}

/// One scheduled grain, shared by the audio and video renderers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrainEvent {
    /// Absolute onset on the timeline, seconds.
    pub onset: f64,
    /// Read position in the source media, seconds.
    pub source_pos: f64,
    /// Grain length in seconds (output time).
    pub duration: f32,
    /// Playback rate; audio pitch ratio == visual playback rate.
    pub pitch_ratio: f32,
    /// Linear gain (audio) == brightness/opacity weight (video).
    pub gain: f32,
    /// -1..1; audio pan == horizontal placement of the visual grain.
    pub pan: f32,
    /// Envelope shape, forwarded from settings.
    pub envelope: f32,
    /// Envelope skew, forwarded from settings.
    pub env_skew: f32,
    /// Play the grain backwards (audio and video alike).
    pub reverse: bool,
    /// Stable per-grain id for deterministic visual scatter.
    pub id: u64,
}

impl GrainEvent {
    pub fn end(&self) -> f64 {
        self.onset + self.duration as f64
    }

    /// Pitch offset of this grain in semitones (drives visual hue shift).
    pub fn semitones(&self) -> f32 {
        crate::music::ratio_to_semitones(self.pitch_ratio)
    }
}

/// Schedule the grain cloud for a clip.
///
/// * `clip_start`/`clip_len`: seconds on the timeline.
/// * `source_len`: seconds of source media available.
/// * `base_hz` + `key`: when a key is given, every grain's pitch ratio is
///   quantized so `base_hz * ratio` lands on the key's scale.
pub fn schedule_grains(
    settings: &GrainSettings,
    clip_start: f64,
    clip_len: f64,
    source_len: f64,
    base_hz: f32,
    key: Option<&Key>,
) -> Vec<GrainEvent> {
    schedule_grains_auto(settings, &[], 1.0, clip_start, clip_len, source_len, base_hz, key)
}

/// Like [`schedule_grains`], with automation lanes evaluated at each
/// grain's onset (lane beats are relative to the clip start;
/// `secs_per_beat` converts grain time to beats). Density automation
/// changes the scheduling rate itself.
#[allow(clippy::too_many_arguments)]
pub fn schedule_grains_auto(
    settings: &GrainSettings,
    lanes: &[crate::auto::AutomationLane],
    secs_per_beat: f64,
    clip_start: f64,
    clip_len: f64,
    source_len: f64,
    base_hz: f32,
    key: Option<&Key>,
) -> Vec<GrainEvent> {
    let mut events = Vec::new();
    if clip_len <= 0.0 || source_len <= 0.0 || settings.density <= 0.0 {
        return events;
    }
    let mut rng = Rng::new(settings.seed ^ 0xA5A5_5A5A);

    /// One grain and its harmony voices at clip-time `t`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        t: f64,
        clip_start: f64,
        source_len: f64,
        base_hz: f32,
        key: Option<&Key>,
        s: &GrainSettings,
        rng: &mut Rng,
        events: &mut Vec<GrainEvent>,
        id: &mut u64,
    ) {
        let dur = (s.duration * (1.0 + s.duration_jitter * rng.bipolar())).clamp(0.005, 5.0);

        // Read head: base position + scan + spray.
        let mut pos = s.position as f64 * source_len
            + s.scan_speed as f64 * t
            + s.spray as f64 * rng.bipolar() as f64;
        pos = pos.rem_euclid(source_len.max(1e-6));

        let pitch_j = s.pitch_jitter * rng.bipolar();
        let reverse = rng.next_f32() < s.reverse_prob;

        let n_voices = s.voices.clamp(1, 4);
        // Keep the cloud's loudness roughly level as voices stack.
        let vgain = s.gain / (n_voices as f32).sqrt();
        for v in 0..n_voices {
            let detune = if v == 0 { 0.0 } else { s.voice_detune * rng.bipolar() };
            let st = s.pitch + v as f32 * s.voice_interval + pitch_j + detune;
            let mut ratio = semitones_to_ratio(st);
            if let Some(k) = key {
                ratio = k.quantize_ratio(base_hz, ratio);
            }
            let pan = (s.pan + s.pan_spread.clamp(0.0, 1.0) * rng.bipolar())
                .clamp(-1.0, 1.0);
            events.push(GrainEvent {
                onset: clip_start + t,
                source_pos: pos,
                duration: dur,
                pitch_ratio: ratio,
                gain: vgain,
                pan,
                envelope: s.envelope,
                env_skew: s.env_skew,
                reverse,
                id: *id,
            });
            *id += 1;
        }
    }

    let eff_at = |t: f64| -> GrainSettings {
        if lanes.is_empty() {
            settings.clone()
        } else {
            crate::auto::settings_at(settings, lanes, t / secs_per_beat.max(1e-9))
        }
    };

    let mut id: u64 = 0;
    if settings.sync_div > 0.0 {
        // Beat-synced: onsets live on the sync grid; density is the mean
        // trigger rate, realized as a per-slot probability.
        let slot_secs = (settings.sync_div as f64 * secs_per_beat).max(1e-3);
        let mut k = 0u64;
        loop {
            let t = k as f64 * slot_secs;
            if t >= clip_len {
                break;
            }
            let roll = rng.next_f32();
            let s = eff_at(t);
            let p = (s.density as f64 * slot_secs).min(1.0) as f32;
            if roll < p {
                emit(t, clip_start, source_len, base_hz, key, &s, &mut rng, &mut events, &mut id);
            }
            k += 1;
        }
    } else {
        // Free: classic asynchronous cloud with jittered inter-onset times.
        let mut t = 0.0f64;
        while t < clip_len {
            let s = eff_at(t);
            let mean_interval = 1.0 / s.density.max(0.01) as f64;
            // Jitter inter-onset interval +-50% for an organic cloud.
            let interval = mean_interval * (0.5 + rng.next_f32() as f64);
            emit(t, clip_start, source_len, base_hz, key, &s, &mut rng, &mut events, &mut id);
            t += interval;
        }
    }
    events
}

/// Build the grain a MIDI note plays: the sampler mapping. Note 60 (C4)
/// plays the source at the clip's base pitch; other notes transpose in
/// semitones. Velocity scales gain. No key quantization — MIDI notes are
/// explicit pitches.
pub fn grain_from_note(
    settings: &GrainSettings,
    note: u8,
    velocity: u8,
    source_len: f64,
    onset: f64,
    id: u64,
) -> GrainEvent {
    let st = settings.pitch + (note as f32 - 60.0);
    GrainEvent {
        onset,
        source_pos: (settings.position as f64 * source_len).min(source_len),
        duration: settings.duration.clamp(0.01, 5.0),
        pitch_ratio: semitones_to_ratio(st),
        gain: settings.gain * (velocity as f32 / 127.0),
        pan: settings.pan.clamp(-1.0, 1.0),
        envelope: settings.envelope,
        env_skew: settings.env_skew,
        reverse: settings.reverse_prob >= 0.5,
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::{hz_to_midi, Key};

    #[test]
    fn schedules_expected_grain_count() {
        let s = GrainSettings { density: 50.0, ..Default::default() };
        let ev = schedule_grains(&s, 0.0, 2.0, 10.0, 440.0, None);
        // ~50/s over 2s with +-50% interval jitter: expect roughly 100.
        assert!(ev.len() > 60 && ev.len() < 220, "got {}", ev.len());
        for e in &ev {
            assert!(e.onset >= 0.0 && e.onset < 2.0);
            assert!(e.source_pos >= 0.0 && e.source_pos < 10.0);
            assert!(e.duration > 0.0);
        }
    }

    #[test]
    fn deterministic_with_seed() {
        let s = GrainSettings::default();
        let a = schedule_grains(&s, 0.0, 1.0, 5.0, 440.0, None);
        let b = schedule_grains(&s, 0.0, 1.0, 5.0, 440.0, None);
        assert_eq!(a, b);
        let s2 = GrainSettings { seed: 999, ..Default::default() };
        let c = schedule_grains(&s2, 0.0, 1.0, 5.0, 440.0, None);
        assert_ne!(a, c);
    }

    #[test]
    fn key_quantization_applies_to_all_grains() {
        let key = Key::parse("C", "majorpentatonic").unwrap();
        let s = GrainSettings {
            pitch_jitter: 7.0,
            density: 40.0,
            ..Default::default()
        };
        let ev = schedule_grains(&s, 0.0, 1.0, 5.0, 261.626, Some(&key));
        assert!(!ev.is_empty());
        for e in &ev {
            let midi = hz_to_midi(261.626 * e.pitch_ratio);
            let pc = (midi.round() as i32).rem_euclid(12);
            let rel = (pc - key.root).rem_euclid(12);
            assert!(
                key.scale.intervals().contains(&rel),
                "grain pitch pc {pc} escaped the key"
            );
        }
    }

    #[test]
    fn sync_locks_onsets_to_the_grid() {
        let s = GrainSettings {
            density: 200.0, // saturates every slot
            sync_div: 0.25,
            duration_jitter: 0.0,
            ..Default::default()
        };
        // secs_per_beat = 0.5 -> grid of 0.125s.
        let ev = schedule_grains_auto(&s, &[], 0.5, 0.0, 2.0, 10.0, 440.0, None);
        assert!(!ev.is_empty());
        for e in &ev {
            let slots = e.onset / 0.125;
            assert!(
                (slots - slots.round()).abs() < 1e-9,
                "onset {} not on the 16th grid",
                e.onset
            );
        }
        // Full saturation: one grain per slot over 2s = 16 slots.
        assert_eq!(ev.len(), 16);

        // Low density -> sparse but still on-grid.
        let sparse = GrainSettings { density: 3.0, ..s };
        let ev2 = schedule_grains_auto(&sparse, &[], 0.5, 0.0, 4.0, 10.0, 440.0, None);
        assert!(ev2.len() < 32 && !ev2.is_empty(), "{} hits", ev2.len());
    }

    #[test]
    fn harmony_voices_stack_intervals() {
        let key = Key::parse("C", "major").unwrap();
        let s = GrainSettings {
            density: 10.0,
            voices: 3,
            voice_interval: 12.0,
            voice_detune: 0.0,
            pitch_jitter: 0.0,
            pan_spread: 0.0,
            ..Default::default()
        };
        let base_hz = 261.626; // C4
        let ev = schedule_grains(&s, 0.0, 1.0, 5.0, base_hz, Some(&key));
        // Events come in groups of 3 sharing an onset.
        assert_eq!(ev.len() % 3, 0);
        for g in ev.chunks(3) {
            assert_eq!(g[0].onset, g[1].onset);
            assert_eq!(g[1].onset, g[2].onset);
            // Octave stack: ratios 1, 2, 4.
            assert!((g[1].pitch_ratio / g[0].pitch_ratio - 2.0).abs() < 0.01);
            assert!((g[2].pitch_ratio / g[0].pitch_ratio - 4.0).abs() < 0.02);
            // Voice gain compensated.
            assert!((g[0].gain - s.gain / 3.0f32.sqrt()).abs() < 1e-6);
        }
        // Fifths stay in key when quantized.
        let s5 = GrainSettings { voice_interval: 7.0, ..s };
        let ev5 = schedule_grains(&s5, 0.0, 1.0, 5.0, base_hz, Some(&key));
        for e in &ev5 {
            let midi = hz_to_midi(base_hz * e.pitch_ratio);
            let pc = (midi.round() as i32).rem_euclid(12);
            assert!(key.scale.intervals().contains(&pc), "voice off-key: pc {pc}");
        }
    }

    #[test]
    fn env_skew_shapes_attack_vs_release() {
        use crate::dsp::grain_env_skewed;
        // Percussive: loud early, gone late.
        let perc_early = grain_env_skewed(0.05, 0.8, -1.0);
        let perc_late = grain_env_skewed(0.7, 0.8, -1.0);
        assert!(perc_early > 0.9, "instant attack: {perc_early}");
        assert!(perc_late < perc_early, "long tail decays: {perc_late}");
        // Swell: quiet early, loud late.
        let swell_early = grain_env_skewed(0.3, 0.8, 1.0);
        let swell_late = grain_env_skewed(0.95, 0.8, 1.0);
        assert!(swell_early < 0.9);
        assert!(swell_late > 0.9, "swell peaks late: {swell_late}");
        // Symmetric matches the classic env.
        for p in [0.1, 0.3, 0.5, 0.8] {
            assert!((grain_env_skewed(p, 0.6, 0.0) - crate::dsp::grain_env(p, 0.6)).abs() < 1e-6);
        }
    }

    #[test]
    fn note_grain_is_a_sampler_mapping() {
        let s = GrainSettings { gain: 1.0, pitch: 0.0, ..Default::default() };
        let c4 = grain_from_note(&s, 60, 127, 4.0, 0.0, 0);
        assert!((c4.pitch_ratio - 1.0).abs() < 1e-6, "C4 plays at unity");
        let c5 = grain_from_note(&s, 72, 127, 4.0, 0.0, 1);
        assert!((c5.pitch_ratio - 2.0).abs() < 1e-5, "octave up doubles rate");
        let soft = grain_from_note(&s, 60, 64, 4.0, 0.0, 2);
        assert!((soft.gain - 64.0 / 127.0).abs() < 1e-5, "velocity -> gain");
    }

    #[test]
    fn scan_advances_read_head() {
        let s = GrainSettings {
            spray: 0.0,
            scan_speed: 1.0,
            position: 0.0,
            duration_jitter: 0.0,
            ..Default::default()
        };
        let ev = schedule_grains(&s, 0.0, 2.0, 10.0, 440.0, None);
        // Later grains should read later in the source.
        let first = ev.first().unwrap();
        let last = ev.last().unwrap();
        assert!(last.source_pos > first.source_pos + 1.0);
    }
}
