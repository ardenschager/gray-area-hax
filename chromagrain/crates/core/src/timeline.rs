//! Project model: sources, tracks, clips on a beat-based timeline.

use crate::audio::AudioClip;
use crate::dsp::FilterSpec;
use crate::grain::GrainSettings;
use crate::music::Key;
use crate::video::{ColorFilter, VideoClip, VisualGrainStyle};

pub type SourceId = usize;

/// An imported piece of media (file, YouTube rip, or procedural source).
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub name: String,
    pub audio: Option<AudioClip>,
    pub video: Option<VideoClip>,
    /// Estimated fundamental (Hz) used for key quantization. 0 = unknown,
    /// in which case A440 relative quantization is used.
    pub base_hz: f32,
}

impl Source {
    /// Longest media duration available in this source, seconds.
    pub fn duration(&self) -> f64 {
        let a = self.audio.as_ref().map(|a| a.duration()).unwrap_or(0.0);
        let v = self.video.as_ref().map(|v| v.duration()).unwrap_or(0.0);
        a.max(v)
    }

    pub fn effective_base_hz(&self) -> f32 {
        if self.base_hz > 0.0 {
            self.base_hz
        } else {
            440.0
        }
    }
}

/// A granular clip placed on the timeline (positions in beats).
#[derive(Debug, Clone)]
pub struct Clip {
    pub source: SourceId,
    pub start_beat: f64,
    pub length_beats: f64,
    pub grains: GrainSettings,
    /// Quantize grain pitches onto this key.
    pub key: Option<Key>,
    /// Audio frequency filtering, applied to the source before granulation.
    pub audio_filters: Vec<FilterSpec>,
    /// Color (hue-band) filtering applied to visual grains.
    pub color_filter: Option<ColorFilter>,
    pub visual: VisualGrainStyle,
}

impl Clip {
    pub fn new(source: SourceId, start_beat: f64, length_beats: f64) -> Clip {
        Clip {
            source,
            start_beat,
            length_beats,
            grains: GrainSettings::default(),
            key: None,
            audio_filters: Vec::new(),
            color_filter: None,
            visual: VisualGrainStyle::default(),
        }
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }
}

#[derive(Debug, Clone, Default)]
pub struct Track {
    pub name: String,
    pub clips: Vec<Clip>,
    pub muted: bool,
}

/// A whole chromagrain project.
#[derive(Debug, Clone)]
pub struct Project {
    pub bpm: f64,
    pub sample_rate: u32,
    /// Output canvas.
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub sources: Vec<Source>,
    pub tracks: Vec<Track>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            bpm: 110.0,
            sample_rate: 48000,
            width: 640,
            height: 360,
            fps: 24.0,
            sources: Vec::new(),
            tracks: Vec::new(),
        }
    }
}

impl Project {
    pub fn beats_to_secs(&self, beats: f64) -> f64 {
        beats * 60.0 / self.bpm
    }

    pub fn secs_to_beats(&self, secs: f64) -> f64 {
        secs * self.bpm / 60.0
    }

    pub fn add_source(&mut self, source: Source) -> SourceId {
        self.sources.push(source);
        self.sources.len() - 1
    }

    pub fn add_track(&mut self, name: &str) -> usize {
        self.tracks.push(Track { name: name.to_string(), ..Default::default() });
        self.tracks.len() - 1
    }

    /// Last beat covered by any clip.
    pub fn end_beat(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.end_beat())
            .fold(0.0, f64::max)
    }

    /// A ready-to-play demo project built from procedural sources — used by
    /// the app on first launch and by integration tests.
    pub fn demo() -> Project {
        use crate::music::ScaleKind;
        let mut p = Project { bpm: 100.0, width: 480, height: 270, ..Default::default() };

        let tone = Source {
            name: "saw stack (A2)".into(),
            audio: Some(AudioClip::saw_stack(110.0, 4.0, p.sample_rate)),
            video: Some(VideoClip::test_pattern(240, 136, 12.0, 4.0)),
            base_hz: 110.0,
        };
        let sid = p.add_source(tone);

        let t0 = p.add_track("grains A");
        let mut c0 = Clip::new(sid, 0.0, 8.0);
        c0.key = Some(Key::new(9, ScaleKind::MinorPentatonic)); // A minor pent
        c0.grains = GrainSettings {
            density: 24.0,
            duration: 0.15,
            pitch_jitter: 12.0,
            spray: 0.4,
            scan_speed: 0.5,
            ..Default::default()
        };
        c0.audio_filters.push(FilterSpec {
            kind: crate::dsp::FilterKind::LowPass,
            freq: 2500.0,
            q: 0.9,
        });
        p.tracks[t0].clips.push(c0);

        let t1 = p.add_track("grains B");
        let mut c1 = Clip::new(sid, 4.0, 4.0);
        c1.key = Some(Key::new(9, ScaleKind::MinorPentatonic));
        c1.grains = GrainSettings {
            density: 40.0,
            duration: 0.06,
            pitch: 12.0,
            pitch_jitter: 7.0,
            spray: 0.8,
            gain: 0.5,
            reverse_prob: 0.3,
            seed: 77,
            ..Default::default()
        };
        c1.color_filter = Some(ColorFilter::keep(200.0, 160.0));
        p.tracks[t1].clips.push(c1);

        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_time_conversion() {
        let p = Project { bpm: 120.0, ..Default::default() };
        assert!((p.beats_to_secs(4.0) - 2.0).abs() < 1e-9);
        assert!((p.secs_to_beats(2.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn demo_project_is_well_formed() {
        let p = Project::demo();
        assert!(!p.sources.is_empty());
        assert!(!p.tracks.is_empty());
        assert!(p.end_beat() >= 8.0);
        for track in &p.tracks {
            for clip in &track.clips {
                assert!(clip.source < p.sources.len());
                assert!(p.sources[clip.source].duration() > 0.0);
            }
        }
    }
}
