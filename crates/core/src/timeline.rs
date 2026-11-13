//! Project model: sources, tracks, clips on a beat-based timeline.

use crate::audio::AudioClip;
use crate::dsp::FilterSpec;
use crate::fx::AvEffect;
use crate::grain::GrainSettings;
use crate::music::Key;
use crate::video::{AvLink, ColorFilter, VideoClip, VisualGrainStyle};

pub use crate::chop::{build_kit_across_sources, build_tape_loops_across_sources, KitPadSpec};
pub use crate::tile::AspectRatio;

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
    /// Chop points (Koala-style), source seconds; see [`crate::chop`].
    pub slices: Vec<crate::chop::Slice>,
    /// Where it came from (Dig / Link imports).
    pub provenance: Option<Provenance>,
}

/// Where an imported source came from (Dig / Link), shown on its card.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    pub title: String,
    pub uploader: String,
    /// YYYYMMDD.
    pub upload_date: String,
    pub views: Option<u64>,
    pub url: String,
    /// yt-dlp's license string ("Creative Commons Attribution license
    /// (reuse allowed)"), empty when unknown.
    pub license: String,
    /// The Dig query that found it (empty for Link).
    pub query: String,
    /// The window cut from the original video, seconds (start, length).
    pub window: (f64, f64),
}

impl Source {
    /// Rich transient slices (at most `max`) from audio + video frames
    /// (5-band flux + zero-crossing consonant pre-roll + NSDF pitch + `(cx, cy)` focus).
    pub fn auto_slices(&self, max: usize) -> Vec<crate::chop::Slice> {
        if self.audio.is_none() && self.video.is_none() {
            return Vec::new();
        }
        crate::chop::analyze_source_slices(self, max)
    }

    /// `n` equal slices over the whole source, enriched with spectral/pitch/focus metadata.
    pub fn equal_slices(&self, n: usize) -> Vec<crate::chop::Slice> {
        let mut sls = crate::chop::equal_slices(self.duration(), n);
        crate::chop::enrich_slices(&mut sls, self.audio.as_deref(), self.video.as_deref());
        sls
    }

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

    /// Detect the local fundamental pitch (Hz) around `pos_secs` in the audio
    /// clip (falling back to [`Self::effective_base_hz`] on unvoiced/percussive
    /// slices), locked to the parent source's chromatic tuning reference so
    /// `Key` quantization on multi-note vocal/instrument reels snaps each slice
    /// to pure in-tune scale intervals without quarter-tone drift.
    pub fn base_hz_at(&self, pos_secs: f64) -> f32 {
        let fallback = self.effective_base_hz();
        if let Some(audio) = &self.audio {
            if audio.sample_rate > 0 && audio.samples.len() >= 2048 {
                let sr = audio.sample_rate as usize;
                let win = (sr * 60 / 100).clamp(2048, audio.samples.len());
                let center = ((pos_secs.max(0.0) * sr as f64) as usize) % audio.samples.len();
                let start = center.saturating_sub(win / 3).min(audio.samples.len() - win);
                if let Some(hz) =
                    crate::dsp::detect_pitch(&audio.samples[start..start + win], audio.sample_rate)
                {
                    if (45.0..1250.0).contains(&hz) {
                        let ref_midi = 69.0 + 12.0 * (fallback / 440.0).log2();
                        let ref_frac = ref_midi - ref_midi.round();
                        let loc_midi = 69.0 + 12.0 * (hz / 440.0).log2();
                        let snapped_midi = (loc_midi - ref_frac).round() + ref_frac;
                        return 440.0 * 2.0f32.powf((snapped_midi - 69.0) / 12.0);
                    }
                }
            }
        }
        fallback
    }

    /// A new source holding `[start, start + len)` seconds of this one
    /// (audio and video alike, wrapping past the end like the read head).
    pub fn slice(&self, start: f64, len: f64) -> Source {
        let dur = self.duration().max(1e-6);
        let start = start.rem_euclid(dur);
        let len = len.clamp(0.01, dur);
        let audio = self.audio.as_ref().map(|a| {
            if a.samples.is_empty() {
                return a.clone();
            }
            let sr = a.sample_rate as f64;
            let n = a.samples.len().max(1);
            let i0 = (start * sr) as usize;
            let cnt = ((len * sr) as usize).max(1);
            let take = |v: &Vec<f32>| -> Vec<f32> { (0..cnt).map(|i| v[(i0 + i) % n]).collect() };
            std::sync::Arc::new(AudioClip {
                samples: take(&a.samples),
                sample_rate: a.sample_rate,
                side: a.side.as_ref().map(take),
            })
        });
        let video = self.video.as_ref().map(|v| {
            let n = v.frames.len().max(1);
            let i0 = (start * v.fps as f64) as usize;
            let cnt = ((len * v.fps as f64).round() as usize).max(1);
            std::sync::Arc::new(VideoClip {
                frames: (0..cnt).filter_map(|i| v.frames.get((i0 + i) % n).cloned()).collect(),
                fps: v.fps,
            })
        });
        let sliced_base = if len >= 0.55 {
            audio
                .as_ref()
                .and_then(|a| crate::dsp::detect_pitch(&a.samples, a.sample_rate))
                .unwrap_or_else(|| self.base_hz_at(start + len * 0.35))
        } else {
            self.base_hz_at(start + len * 0.35)
        };
        Source { name: self.name.clone(), audio, video, base_hz: sliced_base, ..Default::default() }
    }

    /// Combine `self`'s audio (and `base_hz`) with `video_src`'s video frames,
    /// producing a unified AV source whose grains trigger both together.
    pub fn pair(&self, video_src: &Source) -> Source {
        Source {
            name: format!("{} + {}", self.name, video_src.name),
            audio: self.audio.clone(),
            video: video_src.video.clone(),
            base_hz: self.base_hz,
            ..Default::default()
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

/// Placement of a clip's visuals on the canvas. x/y are offsets in
/// half-canvas units (-1..1, 0 = centered); rotation in degrees.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipTransform {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub rotation: f32,
}

impl Default for ClipTransform {
    fn default() -> Self {
        ClipTransform { x: 0.0, y: 0.0, scale: 1.0, rotation: 0.0 }
    }
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
    /// Parametric peaking bell EQ bands applied alongside `audio_filters`.
    pub eq: Vec<crate::dsp::EqBand>,
    /// Color (hue-band) filtering applied to visual grains.
    pub color_filter: Option<ColorFilter>,
    pub visual: VisualGrainStyle,
    /// The audio->visual correspondence dials (full correspondence default).
    pub link: AvLink,
    /// AV effect chain applied to this clip's audio AND its visual layer.
    pub effects: Vec<AvEffect>,
    /// Parameter automation lanes (grain params, evaluated per grain;
    /// lane beats are relative to the clip start).
    pub automation: Vec<crate::auto::AutomationLane>,
    /// Canvas placement of this clip's visuals — and, through the AV link,
    /// of its audio: x drives pan, scale drives distance loudness.
    pub transform: ClipTransform,
    /// Repeat: the clip's content is one pass this many beats long,
    /// repeated to fill `length_beats`. `None` = plays once, no repeat.
    pub loop_beats: Option<f64>,
    /// Keep repeating until the next clip on the same track (or well past
    /// the end of the song). `length_beats` is kept in sync by
    /// [`Project::resolve_forever`].
    pub loop_forever: bool,
    /// Silence between passes, beats (negative = passes overlap).
    pub loop_gap: f64,
    /// Which passes play, cycling (empty = all). A step sequencer of
    /// repeats: [on, on, off, on] skips every 3rd of 4.
    pub loop_mask: Vec<bool>,
    /// How far each repeat moves through the media, in passes: 0 = the
    /// same bit every time, 1 = carry on where the last pass ended,
    /// -1 = walk backwards, 0.5 = overlapping slices.
    pub loop_advance: f32,
    /// Pass and gap lengths snap to the beat grid when edited (off = free,
    /// e.g. the media's natural length).
    pub loop_sync: bool,
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
            eq: Vec::new(),
            color_filter: None,
            visual: VisualGrainStyle::default(),
            link: AvLink::default(),
            effects: Vec::new(),
            automation: Vec::new(),
            transform: ClipTransform::default(),
            loop_beats: None,
            loop_forever: false,
            loop_gap: 0.0,
            loop_mask: Vec::new(),
            loop_advance: 0.0,
            loop_sync: true,
        }
    }

    pub fn new_snippet(source: SourceId, start_beat: f64, length_beats: f64) -> Clip {
        let mut c = Clip::new(source, start_beat, length_beats);
        c.kind = ClipKind::Snippet;
        // Snippets default to short edge fades and opaque alpha-over rather
        // than a glowing grain cloud.
        c.grains.envelope = 0.05;
        c.grains.pan_spread = 0.0;
        c.visual.additive = 0.0;
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

    /// Length of one pass of the content (the whole clip when not looping).
    pub fn pass_beats(&self) -> f64 {
        match self.loop_beats {
            Some(l) if l > 1e-6 => l,
            _ => self.length_beats,
        }
    }

    /// Pass start to next pass start, beats (pass + gap).
    pub fn period_beats(&self) -> f64 {
        let pass = self.pass_beats();
        if self.loop_beats.is_none() {
            return pass.max(1e-6);
        }
        (pass + self.loop_gap).max(pass * 0.1).max(1e-3)
    }

    /// Whether pass `k` plays (the repeat mask cycles).
    pub fn pass_on(&self, k: usize) -> bool {
        self.loop_mask.is_empty() || self.loop_mask[k % self.loop_mask.len()]
    }

    /// Clip start of pass `k`, beats from the clip start.
    pub fn pass_offset(&self, k: usize) -> f64 {
        k as f64 * self.period_beats()
    }

    /// Whether the content repeats inside the clip.
    pub fn is_looping(&self) -> bool {
        self.loop_beats.is_some() || self.loop_forever
    }

    /// How many passes fit (fractional when the last pass is cut short).
    pub fn repeat_count(&self) -> f64 {
        if self.loop_beats.is_none() {
            return 1.0;
        }
        let pass = self.pass_beats();
        if self.length_beats <= pass {
            return self.length_beats / pass.max(1e-6);
        }
        1.0 + (self.length_beats - pass) / self.period_beats()
    }

    /// Total length for `n` whole passes (no trailing gap).
    pub fn length_for(&self, n: u32) -> f64 {
        let n = n.max(1) as f64;
        ((n - 1.0) * self.period_beats() + self.pass_beats()).max(self.pass_beats())
    }

    /// Turn repeating on with the current length as one pass, playing it
    /// `times` in total (`None` = forever).
    pub fn set_repeat(&mut self, times: Option<u32>) {
        let pass = self.pass_beats();
        self.loop_beats = Some(pass);
        match times {
            Some(n) => {
                self.loop_forever = false;
                self.length_beats = self.length_for(n);
            }
            None => self.loop_forever = true,
        }
    }

    /// Back to a single pass.
    pub fn clear_repeat(&mut self) {
        self.length_beats = self.pass_beats().min(self.length_beats);
        self.loop_beats = None;
        self.loop_forever = false;
        self.loop_gap = 0.0;
        self.loop_mask.clear();
        self.loop_advance = 0.0;
    }
}

/// How far a "forever" clip runs past the end of the song when nothing
/// follows it on its track.
pub const FOREVER_TAIL_BEATS: f64 = 64.0;

/// Sidechain envelope ducking from another track (`source_track` may sit
/// anywhere in the track list, independent of video z-order).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DuckSpec {
    pub source_track: usize,
    /// Maximum audio gain reduction `0..1` (`0.0` = visual-only ducking via
    /// `Track::duck_to_opacity`).
    pub amount: f32,
    /// Attack time in seconds (e.g. `0.008`).
    pub attack: f32,
    /// Release time in seconds (e.g. `0.18`).
    pub release: f32,
    /// 2-pole low-pass cutoff (Hz) on the sidechain detector input. Defaults
    /// to `165 Hz` so kicks/low-percussion carve deep sub/pad pocket while
    /// hi-hats and shakers pass without chattering the gain!
    #[serde(default = "default_duck_key_hz")]
    pub key_hz: f32,
}

pub fn default_duck_key_hz() -> f32 {
    165.0
}

/// Streaming envelope follower state for [`DuckSpec`], shared by offline and
/// realtime rendering.
#[derive(Debug, Clone, Copy, Default)]
pub struct DuckState {
    pub env: f32,
    pub lp1: [f32; 2],
    pub lp2: [f32; 2],
}

impl DuckState {
    /// Advance one sample of sidechain detection, returning `(audio_gain, shaped_env)`
    /// where `shaped_env` is `0..1` detector deflection (used for both audio
    /// gain reduction and visual ducking).
    #[inline]
    pub fn step_av(
        &mut self,
        sc_l: f32,
        sc_r: f32,
        spec: &DuckSpec,
        sample_rate: u32,
    ) -> (f32, f32) {
        let sr = sample_rate.max(1) as f32;
        let x = if spec.key_hz > 10.0 && spec.key_hz < sr * 0.42 {
            let a = 1.0 - (-2.0 * std::f32::consts::PI * spec.key_hz / sr).exp();
            for (c, inp) in [sc_l, sc_r].into_iter().enumerate() {
                self.lp1[c] += a * (inp - self.lp1[c]);
                self.lp2[c] += a * (self.lp1[c] - self.lp2[c]);
            }
            (self.lp2[0].abs().max(self.lp2[1].abs()) * 1.25).min(2.0)
        } else {
            sc_l.abs().max(sc_r.abs())
        };
        let att_c = (-1.0 / (spec.attack.max(0.0005) * sr)).exp();
        let rel_c = (-1.0 / (spec.release.max(0.005) * sr)).exp();
        if x > self.env {
            self.env = att_c * self.env + (1.0 - att_c) * x;
        } else {
            self.env = rel_c * self.env + (1.0 - rel_c) * x;
        }
        let drive = (self.env * 3.4).min(4.0);
        let shaped = drive / (1.0 + drive);
        let audio_g = (1.0 - spec.amount.clamp(0.0, 1.0) * shaped).clamp(0.0, 1.0);
        (audio_g, shaped)
    }

    #[inline]
    pub fn step(&mut self, sc_l: f32, sc_r: f32, spec: &DuckSpec, sample_rate: u32) -> f32 {
        self.step_av(sc_l, sc_r, spec, sample_rate).0
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
    /// Level automation over absolute timeline beats (empty = static
    /// `level`). Drives audio gain per sample and video opacity per frame.
    pub level_points: Vec<crate::auto::AutoPoint>,
    /// Optional sidechain ducking from another track.
    pub duck: Option<DuckSpec>,
    /// How strongly sidechain audio ducking also dims this track's visuals (`0..1`).
    pub duck_to_opacity: f32,
    /// Explicit track-onto-master blend mode (`0` = alpha-over, `1` = additive
    /// glow; `None` = inherit max active clip `additive`).
    pub additive: Option<f32>,
    /// Per-track analog tape / vinyl stage.
    pub tape: crate::audio::TapeSettings,
    /// Per-track parametric peaking EQ bands.
    pub eq: Vec<crate::dsp::EqBand>,
    /// Real-time audio-RMS-to-visual-opacity coupling (`0..1`): modulates the
    /// track's visual layer directly from its smoothed audio envelope so swells
    /// and hits visibly breathe on screen (`0` = static fader opacity only).
    pub rms_to_opacity: f32,
    /// Monophonic choke group (`0 = off`, `1..=4`).
    pub choke_group: u8,
    /// Optional track-wide voice sculptor / tape-loop preset.
    pub sculpt: crate::grain::VoiceSculpt,
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
            level_points: Vec::new(),
            duck: None,
            duck_to_opacity: 0.35,
            additive: None,
            tape: crate::audio::TapeSettings::default(),
            eq: Vec::new(),
            rms_to_opacity: 0.0,
            choke_group: 0,
            sculpt: crate::grain::VoiceSculpt::default(),
        }
    }
}

impl Track {
    /// Level at an absolute timeline beat (automation, or the fader).
    /// Allocation-free: this runs per audio sample.
    pub fn level_at(&self, beat: f64) -> f32 {
        let pts = &self.level_points;
        if pts.is_empty() {
            return self.level.max(0.0);
        }
        if beat <= pts[0].beat {
            return pts[0].value.max(0.0) as f32;
        }
        let last = &pts[pts.len() - 1];
        if beat >= last.beat {
            return last.value.max(0.0) as f32;
        }
        for w in pts.windows(2) {
            if beat >= w[0].beat && beat <= w[1].beat {
                let span = (w[1].beat - w[0].beat).max(1e-12);
                let f = (beat - w[0].beat) / span;
                return ((w[0].value + (w[1].value - w[0].value) * f).max(0.0)) as f32;
            }
        }
        last.value.max(0.0) as f32
    }

    fn opacity_of(&self, level: f32) -> f32 {
        (1.0 + (level.min(1.5) - 1.0) * self.level_to_opacity).clamp(0.0, 1.0)
    }

    /// Video opacity implied by the (static) level fader and its link dial.
    pub fn opacity(&self) -> f32 {
        self.opacity_of(self.level)
    }

    /// Video opacity at an absolute timeline beat.
    pub fn opacity_at(&self, beat: f64) -> f32 {
        self.opacity_of(self.level_at(beat))
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
    /// SuperCollider instrument parts; each renders into one library
    /// source (`part.source`), which clips and pads use like any media.
    pub synth_parts: Vec<crate::sc::SynthPart>,
    /// Master analog tape / vinyl stage applied before master limiting.
    pub master_tape: crate::audio::TapeSettings,
    /// Master output trim in dB (`-24..+24`, default `0.0`) driving the C1
    /// `-1.0 dBFS` soft limiter for consistent `-14 LUFS` web loudness.
    pub master_gain_db: f32,
    /// Whole-frame audio-onset-to-luminance bloom (`0..1`, default `0.0`):
    /// couples master broadband log-energy transients into subtle, non-strobing
    /// whole-frame light breathing (`corr(onset_flux, frame_diff) >= 0.30`).
    pub master_bloom: f32,
    /// Compositing: `Stack`, `Split` (`Split + Bleed`), `Tile` (`Grid`), or `Bleed`.
    pub layout: crate::tile::Layout,
    /// In tile/split mode, a voice's column drives its pan and its row a
    /// subtle elevation cue (on by default).
    pub layout_pan: bool,
    /// Stage aspect ratio (`16:9`, `1:1`, `9:16`).
    pub aspect: crate::tile::AspectRatio,
    /// Studio clarity master bus (`285 Hz` mud control, `HPF >= 240 Hz` on master
    /// delay/reverb tails, `56 Hz` warm sub shelf, `-1.4 dBTP` ceiling; `true` by default).
    pub studio_bus: bool,
    /// Render kinetic stage captions when triggered slices carry a non-empty `caption`.
    pub captions_on: bool,
    /// Prefer *MODELL 5* vertical portrait columns (`2×1` / `3×1` / `4×1`) on wide `16:9` canvases in `Layout::Tile`.
    pub portrait_columns: bool,
    /// Master 5-band `24 dB/Oct` Linkwitz-Riley frequency isolator (`ISOL8`).
    pub isolator: crate::dsp::BandIsolator,
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
            synth_parts: Vec::new(),
            master_tape: crate::audio::TapeSettings::default(),
            master_gain_db: 0.0,
            master_bloom: 0.0,
            layout: crate::tile::Layout::Stack,
            layout_pan: true,
            aspect: crate::tile::AspectRatio::Wide16x9,
            studio_bus: true,
            captions_on: true,
            portrait_columns: false,
            isolator: crate::dsp::BandIsolator::default(),
        }
    }
}

impl Project {
    /// Switch output aspect ratio (`16:9`, `1:1`, `9:16`), preserving the current long-edge resolution.
    pub fn set_aspect(&mut self, aspect: crate::tile::AspectRatio) {
        self.aspect = aspect;
        let long_edge = self.width.max(self.height).max(16);
        let (w, h) = aspect.dimensions_for_long_edge(long_edge);
        self.width = w;
        self.height = h;
    }

    /// Switch output aspect ratio to full-HD export dimensions (`1920x1080`, `1080x1080`, `1080x1920`).
    pub fn set_aspect_hd(&mut self, aspect: crate::tile::AspectRatio) {
        self.aspect = aspect;
        let (w, h) = aspect.hd_dimensions();
        self.width = w;
        self.height = h;
    }
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

    fn finite_end_beat(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter(|c| !c.loop_forever)
            .map(|c| c.end_beat())
            .fold(0.0, f64::max)
    }

    /// End of the song for export: the last finite clip's end (forever
    /// clips don't stretch it), or the last clip's end if all repeat forever.
    pub fn song_end_beat(&self) -> f64 {
        let finite = self.finite_end_beat();
        if finite > 0.0 {
            finite
        } else {
            self.end_beat()
        }
    }

    /// Stretch every forever-repeating clip up to the next clip on its
    /// track, or [`FOREVER_TAIL_BEATS`] past the end of the song (the end
    /// of every *other*, finite clip). Idempotent; returns true if any
    /// length changed.
    pub fn resolve_forever(&mut self) -> bool {
        let song_end = self.finite_end_beat();
        let mut changed = false;
        for t in &mut self.tracks {
            let starts: Vec<f64> = t.clips.iter().map(|c| c.start_beat).collect();
            for (ci, c) in t.clips.iter_mut().enumerate() {
                if !c.loop_forever {
                    continue;
                }
                let next = starts
                    .iter()
                    .enumerate()
                    .filter(|&(j, &s)| j != ci && s > c.start_beat + 1e-6)
                    .map(|(_, &s)| s)
                    .fold(f64::INFINITY, f64::min);
                let end = if next.is_finite() {
                    next
                } else {
                    song_end.max(c.start_beat) + FOREVER_TAIL_BEATS
                };
                let len = (end - c.start_beat).max(0.0625);
                if (len - c.length_beats).abs() > 1e-9 {
                    if c.loop_beats.is_none() {
                        c.loop_beats = Some(c.length_beats);
                    }
                    c.length_beats = len;
                    changed = true;
                }
            }
        }
        changed
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
            ..Default::default()
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
        // Automation: the cloud thickens over the clip.
        let mut dens = crate::auto::AutomationLane::new("density");
        dens.set_point(0.0, 8.0);
        dens.set_point(8.0, 40.0);
        c0.automation.push(dens);
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
            env_skew: -0.6, // percussive grains
            voices: 2,      // octave shimmer
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
        // Half-speed drift (granular stretch keeps the pitch), placed as a
        // tilted pane — its x offset also pans the audio left.
        c2.grains.speed = 0.5;
        c2.transform = ClipTransform { x: -0.25, y: -0.1, scale: 0.85, rotation: -7.0 };
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
        // A 5-step polymeter row: phases against the 16-step grid.
        pat.rows[r1].set_steps(5);
        pat.rows[r1].steps[0].on = true;
        pat.rows[r1].steps[3].on = true;
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
