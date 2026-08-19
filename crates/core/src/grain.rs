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
    /// Probability a grain plays in reverse.
    pub reverse_prob: f32,
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
            "reverse_prob" => self.reverse_prob = x.clamp(0.0, 1.0),
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
            reverse_prob: 0.0,
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

    let mut t = 0.0f64; // time within clip
    let mut id: u64 = 0;
    while t < clip_len {
        let s = if lanes.is_empty() {
            settings.clone()
        } else {
            crate::auto::settings_at(settings, lanes, t / secs_per_beat.max(1e-9))
        };
        let mean_interval = 1.0 / s.density.max(0.01) as f64;
        // Jitter inter-onset interval +-50% for an organic cloud.
        let interval = mean_interval * (0.5 + rng.next_f32() as f64);

        let dur = (s.duration * (1.0 + s.duration_jitter * rng.bipolar())).clamp(0.005, 5.0);

        // Read head: base position + scan + spray.
        let mut pos = s.position as f64 * source_len
            + s.scan_speed as f64 * t
            + s.spray as f64 * rng.bipolar() as f64;
        // Wrap into the source.
        pos = pos.rem_euclid(source_len.max(1e-6));

        let st = s.pitch + s.pitch_jitter * rng.bipolar();
        let mut ratio = semitones_to_ratio(st);
        if let Some(k) = key {
            ratio = k.quantize_ratio(base_hz, ratio);
        }

        let pan =
            (s.pan + s.pan_spread.clamp(0.0, 1.0) * rng.bipolar()).clamp(-1.0, 1.0);
        let reverse = rng.next_f32() < s.reverse_prob;

        events.push(GrainEvent {
            onset: clip_start + t,
            source_pos: pos,
            duration: dur,
            pitch_ratio: ratio,
            gain: s.gain,
            pan,
            envelope: s.envelope,
            reverse,
            id,
        });
        id += 1;
        t += interval;
    }
    events
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
