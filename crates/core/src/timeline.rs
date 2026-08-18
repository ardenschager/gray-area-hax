//! Project model: sources, tracks, clips on a beat-based timeline.

use crate::audio::AudioClip;
use crate::dsp::FilterSpec;
use crate::fx::AvEffect;
use crate::grain::GrainSettings;
use crate::music::Key;
use crate::video::{AvLink, ColorFilter, VideoClip, VisualGrainStyle};

pub type SourceId = usize;

/// An imported piece of media (file, YouTube rip, or procedural source).
/// Media is Arc'd so cloning a Project (performance snapshots, workers)
/// costs refcounts, not sample/frame copies.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub name: String,
    pub audio: Option<std::sync::Arc<AudioClip>>,
    pub video: Option<std::sync::Arc<VideoClip>>,
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

/// What a clip does with its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    /// A cloud of grains (audio + visual) from the source.
    Granular,
    /// The source played straight, as ONE long grain — so the same AV
    /// correspondence machinery (gain->opacity, pitch->rate/hue, pan->x)
    /// applies to plain snippets too.
    Snippet,
    /// A step-sequencer pattern (index into `Project::patterns`), looping
    /// to fill the clip. Each triggered step is one grain event.
    Pattern(usize),
}

/// A clip placed on the timeline (positions in beats).
#[derive(Debug, Clone)]
pub struct Clip {
    pub kind: ClipKind,
    pub source: SourceId,
    pub start_beat: f64,
    pub length_beats: f64,
    /// Granular params. Snippets reuse a subset: position (source offset),
    /// pitch, gain, pan, envelope (edge fades), reverse_prob (>=0.5 =
    /// reversed).
    pub grains: GrainSettings,
    /// Quantize grain pitches onto this key.
    pub key: Option<Key>,
    /// Audio frequency filtering, applied to the source before granulation.
    pub audio_filters: Vec<FilterSpec>,
    /// Color (hue-band) filtering applied to visual grains.
    pub color_filter: Option<ColorFilter>,
    pub visual: VisualGrainStyle,
    /// The audio->visual correspondence dials (full correspondence default).
    pub link: AvLink,
    /// AV effect chain applied to this clip's audio AND its visual layer.
    pub effects: Vec<AvEffect>,
}

impl Clip {
    pub fn new(source: SourceId, start_beat: f64, length_beats: f64) -> Clip {
        Clip {
            kind: ClipKind::Granular,
            source,
            start_beat,
            length_beats,
            grains: GrainSettings::default(),
            key: None,
            audio_filters: Vec::new(),
            color_filter: None,
            visual: VisualGrainStyle::default(),
            link: AvLink::default(),
            effects: Vec::new(),
        }
    }

    pub fn new_snippet(source: SourceId, start_beat: f64, length_beats: f64) -> Clip {
        let mut c = Clip::new(source, start_beat, length_beats);
        c.kind = ClipKind::Snippet;
        // Snippets default to short edge fades rather than a grain window.
        c.grains.envelope = 0.05;
        c.grains.pan_spread = 0.0;
        c
    }

    pub fn new_pattern(pattern: usize, start_beat: f64, length_beats: f64) -> Clip {
        let mut c = Clip::new(0, start_beat, length_beats);
        c.kind = ClipKind::Pattern(pattern);
        c
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }
}

/// A track: clips, a level fader, and its own AV effect chain. Track order
/// is also the video compositing order — later tracks render ON TOP.
#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub clips: Vec<Clip>,
    pub muted: bool,
    /// Track AV effect chain (e.g. "this whole track has a lot of delay").
    pub effects: Vec<AvEffect>,
    /// Level fader: audio gain, and (via `level_to_opacity`) video opacity.
    pub level: f32,
    /// How strongly the level drives video opacity (correspondence dial).
    pub level_to_opacity: f32,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            name: String::new(),
            clips: Vec::new(),
            muted: false,
            effects: Vec::new(),
            level: 1.0,
            level_to_opacity: 1.0,
        }
    }
}

impl Track {
    /// Video opacity implied by the level fader and its link dial.
    pub fn opacity(&self) -> f32 {
        (1.0 + (self.level.min(1.5) - 1.0) * self.level_to_opacity).clamp(0.0, 1.0)
    }
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
    /// Step-sequencer patterns, referenced by pattern clips.
    pub patterns: Vec<crate::seq::StepPattern>,
    /// Master AV effect chain applied to the final mix and final frames.
    pub master_effects: Vec<AvEffect>,
    /// UI snap grid in beats (0.25 = sixteenths at x/4).
    pub quantize: f64,
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
            patterns: Vec::new(),
            master_effects: Vec::new(),
            quantize: 0.25,
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

    pub fn add_pattern(&mut self, pattern: crate::seq::StepPattern) -> usize {
        self.patterns.push(pattern);
        self.patterns.len() - 1
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
            audio: Some(std::sync::Arc::new(AudioClip::saw_stack(110.0, 4.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(240, 136, 12.0, 4.0))),
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
        c1.effects.push(AvEffect::new(
            crate::fx::EffectKind::Crush { downsample: 5.0, bits: 5.0 },
        ));
        p.tracks[t1].clips.push(c1);

        // A quiet snippet bed underneath: the source played straight, an
        // octave down, dimmed — gain drives opacity via the AV link.
        let t2 = p.add_track("bed (snippet)");
        let mut c2 = Clip::new_snippet(sid, 0.0, 8.0);
        c2.grains.pitch = -12.0;
        c2.grains.gain = 0.35;
        p.tracks[t2].clips.push(c2);
        // Snippet bed sits at the bottom of the stack: move it first.
        p.tracks.rotate_right(1);

        // A step pattern: low hits on the beat, answered up an octave.
        let mut pat = crate::seq::StepPattern::new("hits");
        let r0 = pat.add_row(sid);
        pat.rows[r0].grains.pitch = -12.0;
        pat.rows[r0].grains.duration = 0.3;
        pat.rows[r0].key = Some(Key::new(9, ScaleKind::MinorPentatonic));
        for i in (0..16).step_by(4) {
            pat.rows[r0].steps[i].on = true;
        }
        let r1 = pat.add_row(sid);
        pat.rows[r1].grains.pitch = 12.0;
        pat.rows[r1].grains.duration = 0.12;
        pat.rows[r1].grains.gain = 0.5;
        pat.rows[r1].key = Some(Key::new(9, ScaleKind::MinorPentatonic));
        for i in [6usize, 10, 14] {
            pat.rows[r1].steps[i].on = true;
        }
        let pid = p.add_pattern(pat);
        let t3 = p.add_track("seq");
        p.tracks[t3].clips.push(Clip::new_pattern(pid, 0.0, 8.0));
        // The whole seq track echoes: a track-level AV delay.
        p.tracks[t3].effects.push(crate::fx::AvEffect::new(
            crate::fx::EffectKind::Delay {
                time: 0.45,
                feedback: 0.45,
                mix: 0.5,
                shift_x: 0.04,
                shift_y: 0.02,
            },
        ));

        // Gentle master smear ties it together.
        p.master_effects.push(AvEffect {
            kind: crate::fx::EffectKind::Reverb { size: 0.5, damp: 0.4, mix: 0.2 },
            audio: 1.0,
            video: 1.0,
        });

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
