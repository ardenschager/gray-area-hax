//! Chop (Koala-style): slices on a [`Source`](crate::timeline::Source).
//!
//! A slice is a span of source seconds. Sources carry `slices` (persisted
//! with the source); this module makes and edits them:
//! * [`auto_slices`]: transients, via [`AudioClip::detect_slices`];
//! * [`equal_slices`]: n equal parts;
//! * [`add_chop_point`] / [`slices_from_points`]: tap-chop, live;
//! * [`move_boundary`] / [`set_slice_edges`] / [`delete_slice`]: edit.
//!
//! Slices always come back sorted, non-overlapping, inside `0..len` and at
//! least [`MIN_SLICE`] long.
//!
//! Mapping out: [`slice_pad`] makes a Perform pad from a slice (the plugin
//! pads too); MIDI notes from [`SLICE_NOTE`] (C3) up play slices
//! ([`slice_for_note`]); step-sequencer steps pick one with `Step::slice_index`.

use crate::audio::AudioClip;
use crate::grain::{ChordVoicing, Stamp, VoiceSculpt};
use crate::music::Key;
use crate::timeline::{Source, SourceId};
use crate::video::VideoClip;
use serde::{Deserialize, Serialize};

/// Shortest slice, seconds.
pub const MIN_SLICE: f64 = 0.01;
/// MIDI note of slice 1 (C3); slice n is `SLICE_NOTE + n`.
pub const SLICE_NOTE: u8 = 48;

fn default_rms_db() -> f32 {
    -96.0
}

fn default_focus() -> (f32, f32) {
    (0.5, 0.5)
}

/// Compact inline UTF-8 caption (`Copy`, up to 47 bytes) for optional VJ stage
/// text overlays on spoken hooks. Serializes as a plain JSON string (`""` default)
/// while keeping [`Slice`], [`crate::grain::VoiceTag`], and [`crate::instrument::PadSlot`]
/// `Copy` and realtime allocation-free.
#[derive(Clone, Copy)]
pub struct Caption {
    buf: [u8; 47],
    len: u8,
}

impl Caption {
    pub const MAX_BYTES: usize = 47;

    pub const fn empty() -> Caption {
        Caption { buf: [0u8; 47], len: 0 }
    }

    pub fn new(s: &str) -> Caption {
        let mut c = Caption::empty();
        c.set(s);
        c
    }

    pub fn set(&mut self, s: &str) {
        let trimmed = s.trim();
        let mut n = trimmed.len().min(Self::MAX_BYTES);
        while n > 0 && !trimmed.is_char_boundary(n) {
            n -= 1;
        }
        self.buf = [0u8; 47];
        self.buf[..n].copy_from_slice(&trimmed.as_bytes()[..n]);
        self.len = n as u8;
    }

    pub fn clear(&mut self) {
        self.buf = [0u8; 47];
        self.len = 0;
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        let n = (self.len as usize).min(Self::MAX_BYTES);
        std::str::from_utf8(&self.buf[..n]).unwrap_or("")
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for Caption {
    fn default() -> Self {
        Caption::empty()
    }
}

impl std::ops::Deref for Caption {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Caption {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for Caption {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Caption {}

impl PartialEq<&str> for Caption {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<str> for Caption {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<String> for Caption {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl std::fmt::Debug for Caption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl std::fmt::Display for Caption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Caption {
    fn from(s: &str) -> Self {
        Caption::new(s)
    }
}

impl From<String> for Caption {
    fn from(s: String) -> Self {
        Caption::new(&s)
    }
}

impl From<&String> for Caption {
    fn from(s: &String) -> Self {
        Caption::new(s)
    }
}

impl From<Caption> for String {
    fn from(c: Caption) -> Self {
        c.as_str().to_string()
    }
}

impl Serialize for Caption {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Caption {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Ok(Caption::new(&s))
    }
}

/// Unopinionated acoustic role hint computed deterministically from a slice's
/// 5-band envelope, transient punch, and McLeod NSDF pitch confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SliceRole {
    #[default]
    Raw,
    Kick,
    Snare,
    Hat,
    Tonal,
    Phrase,
    Texture,
}

impl SliceRole {
    pub const ALL: [SliceRole; 7] = [
        SliceRole::Raw,
        SliceRole::Kick,
        SliceRole::Snare,
        SliceRole::Hat,
        SliceRole::Tonal,
        SliceRole::Phrase,
        SliceRole::Texture,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SliceRole::Raw => "raw",
            SliceRole::Kick => "kick",
            SliceRole::Snare => "snare",
            SliceRole::Hat => "hat",
            SliceRole::Tonal => "tonal",
            SliceRole::Phrase => "phrase",
            SliceRole::Texture => "texture",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            SliceRole::Raw => "",
            SliceRole::Kick => "sub",
            SliceRole::Snare => "snap",
            SliceRole::Hat => "air",
            SliceRole::Tonal => "tone",
            SliceRole::Phrase => "phrase",
            SliceRole::Texture => "wash",
        }
    }

    pub fn parse(s: &str) -> Option<SliceRole> {
        match s.trim().to_ascii_lowercase().as_str() {
            "raw" | "clean" => Some(SliceRole::Raw),
            "kick" | "sub" => Some(SliceRole::Kick),
            "snare" | "snap" | "clap" => Some(SliceRole::Snare),
            "hat" | "air" => Some(SliceRole::Hat),
            "tonal" | "tone" | "note" => Some(SliceRole::Tonal),
            "phrase" | "vox" | "speech" => Some(SliceRole::Phrase),
            "texture" | "wash" | "pad" => Some(SliceRole::Texture),
            _ => None,
        }
    }
}

/// A span of source time, seconds, enriched with deterministic spectral,
/// pitch, and visual-motion focus metadata.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slice {
    pub start: f64,
    pub end: f64,
    /// Normalized 5-band energy distribution: `[sub <90, low 90-250, mid 250-2k, high 2k-8k, air >8k]`.
    #[serde(default)]
    pub bands: [f32; 5],
    /// Transient attack-to-RMS punch metric (`0..1`).
    #[serde(default)]
    pub punch: f32,
    /// Broadband RMS loudness in dBFS (`-96..0`).
    #[serde(default = "default_rms_db")]
    pub rms_db: f32,
    /// Detected fundamental frequency in Hz (`60..1100 Hz` via McLeod NSDF).
    #[serde(default)]
    pub f0_hz: Option<f32>,
    /// McLeod NSDF pitch confidence (`0..1`).
    #[serde(default)]
    pub pitch_confidence: f32,
    /// Normalized motion/subject center-of-mass `(cx, cy)` clamped to `0.15..0.85` (`(0.5, 0.5)` default).
    #[serde(default = "default_focus")]
    pub focus: (f32, f32),
    /// Acoustic role hint (`Raw` when unanalyzed).
    #[serde(default)]
    pub role_hint: SliceRole,
    /// Optional user-editable lyric/spoken-hook caption for stage display (empty by default).
    #[serde(default)]
    pub caption: Caption,
}

impl Slice {
    pub fn new(start: f64, end: f64) -> Slice {
        Slice {
            start: start.min(end),
            end: start.max(end),
            bands: [0.0; 5],
            punch: 0.0,
            rms_db: default_rms_db(),
            f0_hz: None,
            pitch_confidence: 0.0,
            focus: default_focus(),
            role_hint: SliceRole::Raw,
            caption: Caption::empty(),
        }
    }

    pub fn len(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start && t < self.end
    }

    /// Musical note name (e.g. `"C#3"`) when `f0_hz` is detected with confidence `>= 0.48`.
    pub fn note_name(&self) -> Option<String> {
        let hz = self.f0_hz.filter(|&h| h > 20.0 && self.pitch_confidence >= 0.48)?;
        let midi = crate::music::hz_to_midi(hz).round() as i32;
        let pc = midi.rem_euclid(12) as usize;
        let oct = midi.div_euclid(12) - 1;
        Some(format!("{}{oct}", crate::music::PITCH_CLASS_NAMES[pc]))
    }

    /// Subtle unopinionated Chopper badge (`"C#3"`, `"sub"`, `"snap"`, `"air"`, `"phrase"`, `"wash"`).
    pub fn badge(&self) -> String {
        if self.role_hint == SliceRole::Tonal {
            if let Some(n) = self.note_name() {
                return n;
            }
        }
        if self.pitch_confidence >= 0.62 && matches!(self.role_hint, SliceRole::Raw | SliceRole::Texture) {
            if let Some(n) = self.note_name() {
                return n;
            }
        }
        self.role_hint.badge().to_string()
    }
}

/// Which edge of a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// Sort, clip to `0..len`, drop too-short slices and resolve overlaps (a
/// later slice starts where the previous ended), preserving slice metadata.
pub fn normalize(slices: &mut Vec<Slice>, len: f64) {
    let len = len.max(0.0);
    for s in slices.iter_mut() {
        let a = s.start.clamp(0.0, len);
        let b = s.end.clamp(0.0, len);
        s.start = a.min(b);
        s.end = a.max(b);
    }
    slices.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut prev_end = 0.0f64;
    slices.retain_mut(|s| {
        s.start = s.start.max(prev_end);
        if s.len() < MIN_SLICE {
            return false;
        }
        prev_end = s.end;
        true
    });
}

/// Compute frame-to-frame luminance-diff motion center-of-mass `(cx, cy)` across
/// `video` frames in `[start, end]`, clamped to `0.15..=0.85` (`(0.5, 0.5)` fallback).
pub fn compute_video_focus(video: &VideoClip, start: f64, end: f64) -> (f32, f32) {
    if video.frames.is_empty() || video.fps <= 0.0 {
        return (0.5, 0.5);
    }
    let n = video.frames.len();
    let f0 = ((start.max(0.0) * video.fps as f64).floor() as usize).min(n - 1);
    let f1 = ((end.max(start) * video.fps as f64).ceil() as usize).clamp(f0 + 1, n);

    // Check sparse segmented cutout focus first on the start frame.
    if let Some((ax, ay)) = video.frames[f0].alpha_focus() {
        return (ax.clamp(0.15, 0.85), ay.clamp(0.15, 0.85));
    }

    if f1 - f0 >= 2 {
        let mut best_motion = 0.0f64;
        let mut best_xy = (0.5f32, 0.5f32);
        for fi in (f0 + 1)..f1 {
            let prev = &video.frames[fi - 1];
            let cur = &video.frames[fi];
            if prev.width != cur.width || prev.height != cur.height || cur.width == 0 || cur.height == 0 {
                continue;
            }
            let step_x = (cur.width / 64).max(1);
            let step_y = (cur.height / 36).max(1);
            let mut w_sum = 0.0f64;
            let mut x_sum = 0.0f64;
            let mut y_sum = 0.0f64;
            let mut samples = 0u64;
            let mut y = 0u32;
            while y < cur.height {
                let mut x = 0u32;
                while x < cur.width {
                    let idx = ((y * cur.width + x) * 4) as usize;
                    let l0 = 0.299 * prev.data[idx] as f64
                        + 0.587 * prev.data[idx + 1] as f64
                        + 0.114 * prev.data[idx + 2] as f64;
                    let l1 = 0.299 * cur.data[idx] as f64
                        + 0.587 * cur.data[idx + 1] as f64
                        + 0.114 * cur.data[idx + 2] as f64;
                    let diff = (l1 - l0).abs() / 255.0;
                    if diff > 0.025 {
                        w_sum += diff;
                        x_sum += diff * (x as f64 + 0.5) / cur.width as f64;
                        y_sum += diff * (y as f64 + 0.5) / cur.height as f64;
                    }
                    samples += 1;
                    x += step_x;
                }
                y += step_y;
            }
            let mean_motion = w_sum / samples.max(1) as f64;
            if mean_motion > best_motion && w_sum > 1e-5 {
                best_motion = mean_motion;
                best_xy = ((x_sum / w_sum) as f32, (y_sum / w_sum) as f32);
            }
        }
        if best_motion > 0.004 {
            return (best_xy.0.clamp(0.15, 0.85), best_xy.1.clamp(0.15, 0.85));
        }
    }
    (0.5, 0.5)
}

/// Deterministic acoustic role classifier from 5-band weights, punch, pitch confidence, and duration.
pub fn classify_slice_role(
    bands: [f32; 5],
    punch: f32,
    rms_db: f32,
    pitch_confidence: f32,
    dur_secs: f64,
) -> SliceRole {
    if rms_db <= -65.0 {
        return SliceRole::Raw;
    }
    let low_wt = bands[0] + bands[1] * 0.90;
    let crack_wt = bands[2] * 0.70 + bands[3] * 1.10;
    let air_wt = bands[3] * 0.60 + bands[4] * 1.45;

    if dur_secs <= 0.22 && air_wt >= 0.46 && air_wt > low_wt {
        return SliceRole::Hat;
    }
    if dur_secs <= 0.36 && punch >= 0.40 && low_wt >= 0.40 && pitch_confidence < 0.72 {
        return SliceRole::Kick;
    }
    if dur_secs <= 0.38 && punch >= 0.44 && crack_wt >= 0.40 && pitch_confidence < 0.68 {
        return SliceRole::Snare;
    }
    if dur_secs >= 0.20 && pitch_confidence >= 0.58 && (bands[2] < 0.52 || pitch_confidence >= 0.75) {
        return SliceRole::Tonal;
    }
    if dur_secs >= 0.32 && bands[2] >= 0.33 && (0.24..=0.86).contains(&pitch_confidence) {
        return SliceRole::Phrase;
    }
    if dur_secs >= 0.55 && punch < 0.46 {
        return SliceRole::Texture;
    }
    if air_wt >= 0.50 {
        SliceRole::Hat
    } else if low_wt >= 0.45 {
        SliceRole::Kick
    } else if punch >= 0.50 {
        SliceRole::Snare
    } else if pitch_confidence >= 0.50 {
        SliceRole::Tonal
    } else if bands[2] >= 0.36 {
        SliceRole::Phrase
    } else {
        SliceRole::Texture
    }
}

/// Populate `bands`, `punch`, `rms_db`, `f0_hz`, `pitch_confidence`, `focus`, and `role_hint` on `slice`.
pub fn analyze_slice_features(
    slice: &mut Slice,
    audio: Option<&AudioClip>,
    video: Option<&VideoClip>,
) {
    if let Some(a) = audio {
        if a.sample_rate > 0 && !a.samples.is_empty() {
            let sr = a.sample_rate as f64;
            let i0 = ((slice.start.max(0.0) * sr).round() as usize).min(a.samples.len());
            let i1 = ((slice.end.max(slice.start) * sr).round() as usize).clamp(i0, a.samples.len());
            if i1 > i0 {
                let seg = &a.samples[i0..i1];
                let (bands, punch, rms_db) = crate::dsp::slice_band_stats(seg, a.sample_rate);
                let (f0_hz, conf) = crate::dsp::detect_pitch_nsdf(seg, a.sample_rate);
                slice.bands = bands;
                slice.punch = punch;
                slice.rms_db = rms_db;
                slice.f0_hz = f0_hz;
                slice.pitch_confidence = conf;
                slice.role_hint = classify_slice_role(bands, punch, rms_db, conf, slice.len());
            }
        }
    }
    if let Some(v) = video {
        slice.focus = compute_video_focus(v, slice.start, slice.end);
    }
}

/// Enrich a list of slices in-place with spectral, pitch, role, and video motion-focus metadata.
pub fn enrich_slices(slices: &mut [Slice], audio: Option<&AudioClip>, video: Option<&VideoClip>) {
    for sl in slices.iter_mut() {
        analyze_slice_features(sl, audio, video);
    }
}

/// Transient slices (at most `max`), using the 5-band weighted log-spectral flux
/// onset detector, `-30 ms` consonant pre-roll, asymmetric zero-crossing snap,
/// and McLeod NSDF pitch + spectral role analysis.
pub fn auto_slices(audio: &AudioClip, max: usize) -> Vec<Slice> {
    let spans = crate::dsp::detect_onsets_5band(&audio.samples, audio.sample_rate, max);
    let mut v: Vec<Slice> = spans.into_iter().map(|(a, b)| Slice::new(a, b)).collect();
    normalize(&mut v, audio.duration());
    enrich_slices(&mut v, Some(audio), None);
    v
}

/// Analyze a full [`Source`] (both audio and video frames) into up to `max` rich slices
/// with 5-band spectral features, zero-crossing consonant protection, NSDF pitch, and `(cx, cy)` focus.
pub fn analyze_source_slices(source: &Source, max: usize) -> Vec<Slice> {
    let Some(audio) = source.audio.as_deref() else {
        let mut eq = equal_slices(source.duration(), max.clamp(1, 16));
        enrich_slices(&mut eq, None, source.video.as_deref());
        return eq;
    };
    let spans = crate::dsp::detect_onsets_5band(&audio.samples, audio.sample_rate, max);
    let mut v: Vec<Slice> = spans.into_iter().map(|(a, b)| Slice::new(a, b)).collect();
    normalize(&mut v, source.duration());
    enrich_slices(&mut v, Some(audio), source.video.as_deref());
    v
}

/// Compute semitone shift (`-6.0..=6.0`) snapping `f0_hz` onto the nearest note of `key`.
pub fn compute_key_quantize_semitones(f0_hz: f32, key: Key) -> f32 {
    if !f0_hz.is_finite() || f0_hz <= 20.0 {
        return 0.0;
    }
    let midi = crate::music::hz_to_midi(f0_hz);
    let snapped = key.quantize_midi(midi);
    let mut diff = snapped - midi;
    while diff > 6.0 {
        diff -= 12.0;
    }
    while diff < -6.0 {
        diff += 12.0;
    }
    diff.clamp(-6.0, 6.0)
}

/// One curated pad specification produced by [`build_kit_across_sources`] or
/// [`build_tape_loops_across_sources`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KitPadSpec {
    pub pad_index: usize,
    pub source: SourceId,
    pub slice: Slice,
    pub stamp: Stamp,
    pub sculpt: VoiceSculpt,
    pub pitch_semitones: f32,
    pub gain: f32,
    pub pan: f32,
    pub choke_group: u8,
    pub one_shot: bool,
    pub adsr: Option<crate::envelope::Adsr>,
    pub label: String,
}

impl KitPadSpec {
    /// Convert into an `Instrument` [`PadSlot`](crate::instrument::PadSlot) ready for live/plugin play.
    pub fn to_pad_slot(&self, src_len: f64) -> crate::instrument::PadSlot {
        let mut sculpt = self.sculpt;
        sculpt.pitch_semitones += self.pitch_semitones;
        let start = if src_len > 0.0 {
            (self.slice.start / src_len).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let mut slot = if self.one_shot {
            crate::instrument::PadSlot::from_slice(self.source, self.slice, src_len)
        } else {
            let beats = (self.slice.len() * 2.0).clamp(1.0, 32.0) as f32;
            crate::instrument::PadSlot {
                start,
                ..crate::instrument::PadSlot::new(self.source, beats)
            }
        };
        slot.gain = self.gain;
        slot.adsr = self.adsr;
        slot.sculpt = sculpt;
        slot.choke_group = self.choke_group;
        slot
    }
}

fn default_chord_for_scale(scale: crate::music::ScaleKind) -> ChordVoicing {
    use crate::music::ScaleKind::*;
    match scale {
        Major | Lydian | Mixolydian => ChordVoicing::Maj7,
        Minor | HarmonicMinor | Dorian | Phrygian | Blues => ChordVoicing::Min7,
        MajorPentatonic | MinorPentatonic => ChordVoicing::Fifths,
        Chromatic | WholeTone => ChordVoicing::Sus4,
    }
}

/// Deterministically harvest up to 16 MPC-ready pads across `1..N` loaded [`Source`]s:
/// - Pads `0..2`: `Kick` (`Stamp::Kick`, `choke_group = 2`)
/// - Pads `2..4`: `Snare` (`Stamp::Snare`)
/// - Pads `4..6`: `Hat` (`Stamp::Hat`, `choke_group = 3`, stereo bounce)
/// - Pads `6..10`: Key-tuned `Pluck` & `Chord` (`Karplus-Strong` + scale chord voicings)
/// - Pads `10..14`: `Phrase` hooks — including a paired **`Stutter + Full Phrase`** (`choke_group = 1`)
/// - Pads `14..16`: `Wash` / `TapeLoop` beds
pub fn build_kit_across_sources(sources: &[Source], key: Key) -> Vec<KitPadSpec> {
    build_kit_across_sources_opt(sources, Some(key))
}

/// Like [`build_kit_across_sources`], accepting `Option<Key>`.
pub fn build_kit_across_sources_opt(sources: &[Source], key: Option<Key>) -> Vec<KitPadSpec> {
    #[derive(Clone)]
    struct Cand {
        sid: SourceId,
        src_dur: f64,
        slice: Slice,
        used: bool,
    }
    let mut pool: Vec<Cand> = Vec::new();
    for (sid, src) in sources.iter().enumerate() {
        let src_dur = src.duration();
        if src_dur < MIN_SLICE {
            continue;
        }
        let mut sls = if !src.slices.is_empty() {
            let mut v = src.slices.clone();
            if v.iter().all(|s| s.rms_db <= -95.0 && s.role_hint == SliceRole::Raw) {
                enrich_slices(&mut v, src.audio.as_deref(), src.video.as_deref());
            }
            v
        } else {
            analyze_source_slices(src, 24)
        };
        if sls.len() < 4 && src_dur >= 0.4 {
            // Subdivide long steady clips so even 1 smooth clip can yield a multi-pad kit.
            let mut extra = equal_slices(src_dur, 8);
            enrich_slices(&mut extra, src.audio.as_deref(), src.video.as_deref());
            for es in extra {
                if !sls.iter().any(|x| (x.start - es.start).abs() < 0.04) {
                    sls.push(es);
                }
            }
        }
        for slice in sls {
            if slice.rms_db > -68.0 || src.audio.is_none() {
                pool.push(Cand { sid, src_dur, slice, used: false });
            }
        }
    }
    if pool.is_empty() {
        return Vec::new();
    }

    let n_sources = sources.len().max(1);
    let mut src_counts = vec![0usize; n_sources];
    let pick_best = |pool: &mut [Cand], src_counts: &mut [usize], score_fn: &dyn Fn(&Slice) -> f32| -> Option<(SourceId, Slice)> {
        let mut best_i = None;
        let mut best_s = f32::NEG_INFINITY;
        for (i, c) in pool.iter().enumerate() {
            if c.used {
                continue;
            }
            let base = score_fn(&c.slice);
            if base <= -1e6 {
                continue;
            }
            // Diversity bonus so multi-clip kits pull across all loaded videos.
            let div_bonus = 1.0 / (1.0 + 0.35 * src_counts[c.sid] as f32);
            let s = base * div_bonus;
            if s > best_s {
                best_s = s;
                best_i = Some(i);
            }
        }
        let idx = best_i?;
        pool[idx].used = true;
        src_counts[pool[idx].sid] += 1;
        Some((pool[idx].sid, pool[idx].slice))
    };

    let mut out: Vec<KitPadSpec> = Vec::with_capacity(16);

    // 1. Kicks (2 pads): punchy low/sub thuds or plosives (clamped to tight <= 0.28s window).
    for k in 0..2 {
        if let Some((sid, mut sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
            let dur_fit = if s.len() <= 0.45 { 1.15 } else { 0.75 };
            let low = s.bands[0] * 1.2 + s.bands[1] * 0.9 + 0.12;
            let unpitched = 1.0 - 0.35 * s.pitch_confidence;
            (s.punch + 0.25) * low * dur_fit * unpitched
        }) {
            sl.end = sl.end.min(sl.start + 0.28);
            sl.role_hint = SliceRole::Kick;
            out.push(KitPadSpec {
                pad_index: out.len(),
                source: sid,
                slice: sl,
                stamp: Stamp::Kick,
                sculpt: VoiceSculpt::preset(Stamp::Kick),
                pitch_semitones: if k == 0 { 0.0 } else { -2.0 },
                gain: 0.95,
                pan: 0.0,
                choke_group: 2,
                one_shot: true,
                adsr: None,
                label: format!("Kick {}", k + 1),
            });
        }
    }

    // 2. Snares (2 pads): mid-high transient claps/knocks/consonants.
    for k in 0..2 {
        if let Some((sid, mut sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
            let dur_fit = if s.len() <= 0.40 { 1.2 } else { 0.7 };
            let crack = s.bands[2] * 0.75 + s.bands[3] * 1.15 + 0.10;
            (s.punch + 0.20) * crack * dur_fit
        }) {
            sl.end = sl.end.min(sl.start + 0.26);
            sl.role_hint = SliceRole::Snare;
            out.push(KitPadSpec {
                pad_index: out.len(),
                source: sid,
                slice: sl,
                stamp: Stamp::Snare,
                sculpt: VoiceSculpt::preset(Stamp::Snare),
                pitch_semitones: if k == 0 { 0.0 } else { 1.5 },
                gain: 0.90,
                pan: 0.0,
                choke_group: 0,
                one_shot: true,
                adsr: None,
                label: format!("Snare {}", k + 1),
            });
        }
    }

    // 3. Hats (2 pads): closed + open sibilant/air ticks in Choke Group 3.
    for k in 0..2 {
        if let Some((sid, mut sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
            let air = s.bands[3] * 0.65 + s.bands[4] * 1.45 + 0.08;
            let short = if s.len() <= 0.25 { 1.25 } else { 0.8 };
            air * short * (0.5 + 0.5 * s.punch)
        }) {
            let max_d = if k == 0 { 0.085 } else { 0.18 };
            sl.end = sl.end.min(sl.start + max_d);
            sl.role_hint = SliceRole::Hat;
            out.push(KitPadSpec {
                pad_index: out.len(),
                source: sid,
                slice: sl,
                stamp: Stamp::Hat,
                sculpt: VoiceSculpt::preset(Stamp::Hat),
                pitch_semitones: if k == 0 { 0.0 } else { -2.0 },
                gain: 0.78,
                pan: if k == 0 { -0.22 } else { 0.22 },
                choke_group: 3,
                one_shot: true,
                adsr: None,
                label: if k == 0 { "Hat Cl".into() } else { "Hat Op".into() },
            });
        }
    }

    // 4. Key-tuned Tonal / Pluck / Chord (up to 4 pads).
    let chord_voicing = key
        .map(|k| default_chord_for_scale(k.scale))
        .unwrap_or(ChordVoicing::Min7);
    for k in 0..4 {
        if let Some((sid, mut sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
            let dur_fit = if (0.20..=1.80).contains(&s.len()) { 1.25 } else { 0.8 };
            (0.25 + s.pitch_confidence) * dur_fit * (1.0 + (s.rms_db + 48.0).max(0.0) / 48.0)
        }) {
            sl.end = sl.end.min(sl.start + 1.50);
            sl.role_hint = SliceRole::Tonal;
            let tune_st = match (key, sl.f0_hz) {
                (Some(ky), Some(hz)) => compute_key_quantize_semitones(hz, ky),
                _ => 0.0,
            };
            let is_chord = k >= 2;
            let stamp = if is_chord { Stamp::Chord } else { Stamp::Pluck };
            let mut sculpt = VoiceSculpt::preset(stamp);
            if is_chord {
                sculpt.chord = if k == 2 { chord_voicing } else { ChordVoicing::Fifths };
            }
            let degree_shift = match k {
                0 => 0.0,
                1 => 7.0,
                2 => 0.0,
                _ => 5.0,
            };
            out.push(KitPadSpec {
                pad_index: out.len(),
                source: sid,
                slice: sl,
                stamp,
                sculpt,
                pitch_semitones: tune_st + degree_shift,
                gain: if is_chord { 0.82 } else { 0.88 },
                pan: if k % 2 == 0 { -0.15 } else { 0.15 },
                choke_group: if is_chord { 4 } else { 0 },
                one_shot: true,
                adsr: None,
                label: if is_chord { format!("Chord {}", k - 1) } else { format!("Pluck {}", k + 1) },
            });
        }
    }

    // 5. Spoken Phrase Hooks (up to 4 pads) — including a paired `Stutter + Full Phrase` in Choke Group 1!
    if let Some((sid, full_sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
        if s.len() < 0.35 {
            return -1e9;
        }
        let vox_fit = if (0.45..=2.40).contains(&s.len()) { 1.4 } else { 0.9 };
        let mid_dom = s.bands[2] * 1.4 + s.bands[3] * 0.4;
        mid_dom * vox_fit * (0.4 + 0.6 * (1.0 - (s.pitch_confidence - 0.55).abs()))
    }) {
        let mut stut = full_sl;
        stut.end = (full_sl.start + 0.14).min(full_sl.end);
        stut.role_hint = SliceRole::Phrase;
        let mut full = full_sl;
        full.end = full.end.min(full.start + 2.20);
        full.role_hint = SliceRole::Phrase;
        // Paired Stutter -> Full Deadpan Phrase (both Choke Group 1, 0 st natural voice!).
        out.push(KitPadSpec {
            pad_index: out.len(),
            source: sid,
            slice: stut,
            stamp: Stamp::Phrase,
            sculpt: VoiceSculpt::preset(Stamp::Phrase),
            pitch_semitones: 0.0,
            gain: 0.92,
            pan: 0.0,
            choke_group: 1,
            one_shot: true,
            adsr: None,
            label: "Stutter".into(),
        });
        out.push(KitPadSpec {
            pad_index: out.len(),
            source: sid,
            slice: full,
            stamp: Stamp::Phrase,
            sculpt: VoiceSculpt::preset(Stamp::Phrase),
            pitch_semitones: 0.0,
            gain: 0.96,
            pan: 0.0,
            choke_group: 1,
            one_shot: true,
            adsr: None,
            label: "Phrase 1".into(),
        });
    }
    while out.len() < 14 {
        let Some((sid, mut sl)) = pick_best(&mut pool, &mut src_counts, &|s| {
            s.bands[2] * 1.2 + (s.len().min(2.0) as f32) * 0.35
        }) else {
            break;
        };
        sl.end = sl.end.min(sl.start + 2.0);
        sl.role_hint = SliceRole::Phrase;
        out.push(KitPadSpec {
            pad_index: out.len(),
            source: sid,
            slice: sl,
            stamp: Stamp::Phrase,
            sculpt: VoiceSculpt::preset(Stamp::Phrase),
            pitch_semitones: 0.0,
            gain: 0.92,
            pan: 0.0,
            choke_group: 1,
            one_shot: true,
            adsr: None,
            label: format!("Phrase {}", out.len() - 9),
        });
    }

    // 6. Texture / TapeLoop pads (pads 14..16): penalize late-clip YouTuber outro segments
    // (last 20% of >=8s clips) and syllabic 1-3.5 kHz speech chatter in favor of sustained harmonics!
    let loop_lengths = [5.3f32, 7.9];
    let mut tex_idx = 0usize;
    while out.len() < 16 {
        let mut best_i = None;
        let mut best_s = f32::NEG_INFINITY;
        for (i, c) in pool.iter().enumerate() {
            if c.used {
                continue;
            }
            let s = &c.slice;
            let outro_guard = if c.src_dur >= 8.0 && s.end > 0.80 * c.src_dur {
                0.08
            } else {
                1.0
            };
            let speech_guard = if s.role_hint == SliceRole::Phrase
                || (s.bands[2] > 0.36 && s.pitch_confidence < 0.62)
            {
                0.12
            } else {
                1.0
            };
            let tonal_bonus = 0.65 + 0.95 * s.pitch_confidence;
            let base = ((s.len().min(4.0) as f32) * (1.15 - 0.5 * s.punch) + 0.1)
                * tonal_bonus
                * outro_guard
                * speech_guard;
            let div_bonus = 1.0 / (1.0 + 0.35 * src_counts[c.sid] as f32);
            let sc = base * div_bonus;
            if sc > best_s {
                best_s = sc;
                best_i = Some(i);
            }
        }
        let Some(idx) = best_i else { break };
        pool[idx].used = true;
        src_counts[pool[idx].sid] += 1;
        let (sid, mut sl) = (pool[idx].sid, pool[idx].slice);
        sl.role_hint = SliceRole::Texture;
        let is_tape = tex_idx == 1;
        let stamp = if is_tape { Stamp::TapeLoop } else { Stamp::Wash };
        let mut sculpt = VoiceSculpt::preset(stamp);
        if is_tape {
            sculpt.free_loop_secs = Some(loop_lengths[tex_idx % loop_lengths.len()]);
        }
        let tune_st = match (key, sl.f0_hz) {
            (Some(ky), Some(hz)) if sl.pitch_confidence >= 0.55 => compute_key_quantize_semitones(hz, ky),
            _ => 0.0,
        };
        out.push(KitPadSpec {
            pad_index: out.len(),
            source: sid,
            slice: sl,
            stamp,
            sculpt,
            pitch_semitones: tune_st,
            gain: 0.72,
            pan: if tex_idx == 0 { -0.25 } else { 0.25 },
            choke_group: 0,
            one_shot: !is_tape,
            adsr: None,
            label: if is_tape { "Tape Loop".into() } else { "Wash".into() },
        });
        tex_idx += 1;
    }

    for (i, spec) in out.iter_mut().enumerate() {
        spec.pad_index = i;
    }
    out
}

/// Curate unsynced asynchronous `3.5s..14.0s` Basinski / Caretaker tape loops across
/// `1..N` loaded [`Source`]s (`Loops -> pads`), assigning incommensurable `free_loop_secs`
/// periods so simultaneously launched pads phase-drift and disintegrate organically.
pub fn build_tape_loops_across_sources(sources: &[Source], key: Option<Key>) -> Vec<KitPadSpec> {
    const INCOMMENSURABLE_SECS: [f32; 16] = [
        4.7, 6.1, 7.9, 9.3, 11.2, 13.4, 5.3, 8.6, 10.4, 12.7, 4.1, 6.8, 9.9, 11.8, 7.3, 13.9,
    ];
    #[derive(Clone)]
    struct LoopCand {
        sid: SourceId,
        slice: Slice,
        score: f32,
    }
    let mut cands: Vec<LoopCand> = Vec::new();
    for (sid, src) in sources.iter().enumerate() {
        let dur = src.duration();
        if dur < 0.25 {
            continue;
        }
        // Scan sliding 3.5..12s windows (or full clip when shorter than 4s).
        let win = dur.clamp(0.25, 8.5);
        let step = (win * 0.55).max(1.5);
        let mut t = 0.0;
        while t < dur {
            let s0 = t;
            let s1 = (t + win).min(dur);
            if s1 - s0 >= (0.25f64).min(dur) {
                let mut sl = Slice::new(s0, s1);
                analyze_slice_features(&mut sl, src.audio.as_deref(), src.video.as_deref());
                sl.role_hint = SliceRole::Texture;
                let warmth = sl.bands[0] * 0.5 + sl.bands[1] * 1.1 + sl.bands[2] * 0.65;
                let calm = 1.2 - 0.55 * sl.punch;
                let tonal = 0.7 + 0.85 * sl.pitch_confidence;
                let outro_guard = if dur >= 8.0 && s1 > 0.80 * dur { 0.10 } else { 1.0 };
                let speech_guard = if sl.bands[2] > 0.38 && sl.pitch_confidence < 0.60 { 0.15 } else { 1.0 };
                let audible = if sl.rms_db > -62.0 || src.audio.is_none() { 1.0 } else { 0.05 };
                cands.push(LoopCand {
                    sid,
                    slice: sl,
                    score: warmth * calm * tonal * outro_guard * speech_guard * audible * (sl.len().min(12.0) as f32),
                });
            }
            if s1 >= dur - 1e-3 {
                break;
            }
            t += step;
        }
    }
    cands.sort_by(|a, b| b.score.total_cmp(&a.score));

    // Round-robin across sources first so every clip contributes loops.
    let mut out: Vec<KitPadSpec> = Vec::with_capacity(16);
    let mut used = vec![false; cands.len()];
    for pass in 0..16 {
        if out.len() >= 16 {
            break;
        }
        let target_sid = pass % sources.len().max(1);
        let pick = cands
            .iter()
            .enumerate()
            .find(|(i, c)| !used[*i] && c.sid == target_sid)
            .or_else(|| cands.iter().enumerate().find(|(i, _)| !used[*i]));
        let Some((ci, cand)) = pick else { break };
        used[ci] = true;

        let idx = out.len();
        let disintegrate_mode = idx % 2 == 1;
        let stamp = if disintegrate_mode { Stamp::Disintegrate } else { Stamp::TapeLoop };
        let mut sculpt = VoiceSculpt::preset(stamp);
        let loop_s = INCOMMENSURABLE_SECS[idx % INCOMMENSURABLE_SECS.len()]
            .clamp(3.5, (cand.slice.len() as f32 * 1.6).clamp(3.5, 14.0));
        sculpt.free_loop_secs = Some(loop_s);

        let tune_st = match (key, cand.slice.f0_hz) {
            (Some(ky), Some(hz)) if cand.slice.pitch_confidence >= 0.50 => {
                compute_key_quantize_semitones(hz, ky)
            }
            _ => 0.0,
        };
        let pan = ((idx as f32 % 4.0) / 3.0 - 0.5) * 0.55;
        out.push(KitPadSpec {
            pad_index: idx,
            source: cand.sid,
            slice: cand.slice,
            stamp,
            sculpt,
            pitch_semitones: if idx % 4 == 3 { tune_st - 12.0 } else { tune_st },
            gain: 0.76,
            pan,
            choke_group: 0,
            one_shot: false,
            adsr: Some(crate::envelope::Adsr {
                attack: 0.18,
                decay: 0.2,
                sustain: 1.0,
                release: 0.45,
                picture: crate::envelope::PictureCut::Fade,
                cut_threshold: 0.02,
            }),
            label: format!("Loop {:.1}s", loop_s),
        });
    }
    out
}

/// `n` equal slices covering `0..len`.
pub fn equal_slices(len: f64, n: usize) -> Vec<Slice> {
    let n = n.clamp(1, 128);
    if len <= 0.0 {
        return Vec::new();
    }
    let step = len / n as f64;
    (0..n).map(|i| Slice::new(i as f64 * step, if i + 1 == n { len } else { (i + 1) as f64 * step })).collect()
}

/// Slices between consecutive chop points (tap-chop): points are sorted
/// and deduplicated; the first slice starts at the first point (taps before
/// the first chop leave the head unsliced, like Koala).
pub fn slices_from_points(points: &[f64], len: f64) -> Vec<Slice> {
    let mut p: Vec<f64> = points.iter().copied().filter(|t| t.is_finite() && *t >= 0.0 && *t < len).collect();
    p.sort_by(f64::total_cmp);
    p.dedup_by(|a, b| (*a - *b).abs() < MIN_SLICE);
    let mut v: Vec<Slice> = p.windows(2).map(|w| Slice::new(w[0], w[1])).collect();
    if let Some(&last) = p.last() {
        v.push(Slice::new(last, len));
    }
    normalize(&mut v, len);
    v
}

/// Tap-chop: drop a chop point at `t`. Splits the slice containing `t`;
/// with no slices yet, makes `[t, len)`; between slices, starts a new slice
/// at `t` running to the next slice (or the end). Returns the index of the
/// slice that now starts at `t`, if any.
pub fn add_chop_point(slices: &mut Vec<Slice>, t: f64, len: f64) -> Option<usize> {
    if !t.is_finite() || t < 0.0 || t >= len {
        return None;
    }
    if let Some(i) = slices.iter().position(|s| s.contains(t)) {
        let s = slices[i];
        if t - s.start < MIN_SLICE || s.end - t < MIN_SLICE {
            return None;
        }
        slices[i].end = t;
        slices.insert(i + 1, Slice::new(t, s.end));
        return Some(i + 1);
    }
    let next = slices.iter().find(|s| s.start > t).map(|s| s.start).unwrap_or(len);
    slices.push(Slice::new(t, next));
    normalize(slices, len);
    slices.iter().position(|s| (s.start - t).abs() < 1e-9)
}

/// Drag one edge of slice `i` to `t`. A shared boundary (this slice's end
/// == the next one's start) moves for both, so contiguous chops stay
/// contiguous. Clamped so no slice gets shorter than [`MIN_SLICE`].
pub fn move_boundary(slices: &mut [Slice], i: usize, edge: Edge, t: f64, len: f64) {
    let n = slices.len();
    if i >= n {
        return;
    }
    match edge {
        Edge::Start => {
            let shared = i > 0 && (slices[i - 1].end - slices[i].start).abs() < 1e-9;
            let lo = if i > 0 { slices[i - 1].start + MIN_SLICE } else { 0.0 };
            let lo = if shared { lo } else if i > 0 { slices[i - 1].end } else { 0.0 };
            let t = t.clamp(lo, slices[i].end - MIN_SLICE);
            if shared {
                slices[i - 1].end = t;
            }
            slices[i].start = t;
        }
        Edge::End => {
            let shared = i + 1 < n && (slices[i + 1].start - slices[i].end).abs() < 1e-9;
            let hi = if i + 1 < n { if shared { slices[i + 1].end - MIN_SLICE } else { slices[i + 1].start } } else { len };
            let t = t.clamp(slices[i].start + MIN_SLICE, hi);
            if shared {
                slices[i + 1].start = t;
            }
            slices[i].end = t;
        }
    }
}

/// Set both edges of slice `i` (clamped between its neighbours).
pub fn set_slice_edges(slices: &mut [Slice], i: usize, start: f64, end: f64, len: f64) {
    move_boundary(slices, i, Edge::Start, start, len);
    move_boundary(slices, i, Edge::End, end, len);
}

/// Delete slice `i` (double-tap). Its chop point goes away: the previous
/// contiguous slice grows over it; a first (or detached) slice is removed.
pub fn delete_slice(slices: &mut Vec<Slice>, i: usize) {
    if i >= slices.len() {
        return;
    }
    let s = slices.remove(i);
    if i > 0 && (slices[i - 1].end - s.start).abs() < 1e-9 {
        slices[i - 1].end = s.end;
    }
}

/// Slice index for a MIDI note (C3 = slice 0), if `n_slices` covers it.
pub fn slice_for_note(note: u8, n_slices: usize) -> Option<usize> {
    let i = note.checked_sub(SLICE_NOTE)? as usize;
    (i < n_slices).then_some(i)
}

/// Index of the slice containing source time `t`.
pub fn slice_at(slices: &[Slice], t: f64) -> Option<usize> {
    slices.iter().position(|s| s.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: f64, b: f64) -> Slice {
        Slice::new(a, b)
    }

    #[test]
    fn slice_basics() {
        let x = s(2.0, 1.0);
        assert_eq!((x.start, x.end), (1.0, 2.0));
        assert_eq!(x.len(), 1.0);
        assert!(x.contains(1.0) && !x.contains(2.0));
        assert!(s(1.0, 1.0).is_empty());
    }

    #[test]
    fn equal_slices_cover_exactly() {
        let v = equal_slices(10.0, 4);
        assert_eq!(v.len(), 4);
        assert_eq!(v[0], s(0.0, 2.5));
        assert_eq!(v[3].end, 10.0);
        for w in v.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        assert!(equal_slices(0.0, 4).is_empty());
        assert_eq!(equal_slices(1.0, 0).len(), 1);
        assert_eq!(equal_slices(1.0, 1000).len(), 128);
    }

    #[test]
    fn normalize_sorts_clips_and_resolves_overlaps() {
        let mut v = vec![s(3.0, 12.0), s(-1.0, 2.0), s(1.5, 4.0), s(5.0, 5.001)];
        normalize(&mut v, 10.0);
        assert_eq!(v, vec![s(0.0, 2.0), s(2.0, 4.0), s(4.0, 10.0)]);
    }

    #[test]
    fn slices_from_points_tap_chop() {
        let v = slices_from_points(&[3.0, 1.0, 1.001, f64::NAN, 12.0, 6.0], 10.0);
        assert_eq!(v, vec![s(1.0, 3.0), s(3.0, 6.0), s(6.0, 10.0)]);
        assert!(slices_from_points(&[], 10.0).is_empty());
    }

    #[test]
    fn add_chop_point_splits_and_starts() {
        let mut v = Vec::new();
        assert_eq!(add_chop_point(&mut v, 2.0, 10.0), Some(0));
        assert_eq!(v, vec![s(2.0, 10.0)]);
        assert_eq!(add_chop_point(&mut v, 5.0, 10.0), Some(1));
        assert_eq!(v, vec![s(2.0, 5.0), s(5.0, 10.0)]);
        // Before the first slice: a new head slice up to the next one.
        assert_eq!(add_chop_point(&mut v, 1.0, 10.0), Some(0));
        assert_eq!(v, vec![s(1.0, 2.0), s(2.0, 5.0), s(5.0, 10.0)]);
        // Too close to an existing edge, or out of range: ignored.
        assert_eq!(add_chop_point(&mut v, 5.001, 10.0), None);
        assert_eq!(add_chop_point(&mut v, 10.0, 10.0), None);
        assert_eq!(add_chop_point(&mut v, -1.0, 10.0), None);
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn move_boundary_moves_shared_edges_together() {
        let mut v = equal_slices(9.0, 3);
        move_boundary(&mut v, 1, Edge::Start, 2.0, 9.0);
        assert_eq!(v[0].end, 2.0);
        assert_eq!(v[1].start, 2.0);
        move_boundary(&mut v, 1, Edge::End, 7.0, 9.0);
        assert_eq!(v[1].end, 7.0);
        assert_eq!(v[2].start, 7.0);
        // Clamped: no slice shorter than MIN_SLICE, nothing crosses.
        move_boundary(&mut v, 1, Edge::Start, -5.0, 9.0);
        assert!((v[0].len() - MIN_SLICE).abs() < 1e-9);
        move_boundary(&mut v, 2, Edge::End, 50.0, 9.0);
        assert_eq!(v[2].end, 9.0);
        // Detached edges stop at the neighbour.
        let mut d = vec![s(0.0, 1.0), s(2.0, 3.0)];
        move_boundary(&mut d, 1, Edge::Start, 0.5, 9.0);
        assert_eq!(d[1].start, 1.0);
        assert_eq!(d[0].end, 1.0);
        set_slice_edges(&mut d, 1, 1.5, 2.5, 9.0);
        assert_eq!(d[1], s(1.5, 2.5));
        move_boundary(&mut d, 7, Edge::End, 1.0, 9.0);
    }

    #[test]
    fn delete_slice_merges_into_previous() {
        let mut v = equal_slices(9.0, 3);
        delete_slice(&mut v, 1);
        assert_eq!(v, vec![s(0.0, 6.0), s(6.0, 9.0)]);
        delete_slice(&mut v, 0);
        assert_eq!(v, vec![s(6.0, 9.0)]);
        let mut d = vec![s(0.0, 1.0), s(2.0, 3.0)];
        delete_slice(&mut d, 1);
        assert_eq!(d, vec![s(0.0, 1.0)]);
        delete_slice(&mut d, 5);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn notes_and_lookup() {
        assert_eq!(slice_for_note(47, 4), None);
        assert_eq!(slice_for_note(48, 4), Some(0));
        assert_eq!(slice_for_note(51, 4), Some(3));
        assert_eq!(slice_for_note(52, 4), None);
        let v = equal_slices(4.0, 4);
        assert_eq!(slice_at(&v, 2.5), Some(2));
        assert_eq!(slice_at(&v, 4.0), None);
    }

    #[test]
    fn auto_slices_find_transients() {
        let sr = 22050u32;
        let mut samples = vec![0.0f32; sr as usize * 2];
        for hit in [0.25f64, 0.75, 1.25] {
            let at = (hit * sr as f64) as usize;
            for i in 0..2000 {
                samples[at + i] = (1.0 - i as f32 / 2000.0) * if i % 2 == 0 { 0.8 } else { -0.8 };
            }
        }
        let audio = AudioClip { sample_rate: sr, samples, side: None };
        let v = auto_slices(&audio, 8);
        assert!(!v.is_empty());
        for w in v.windows(2) {
            assert!(w[0].end <= w[1].start + 1e-9);
        }
        assert!(v.iter().all(|x| x.len() >= MIN_SLICE && x.end <= audio.duration() + 1e-9));
    }

    #[test]
    fn slice_serde_round_trip() {
        let v = vec![s(0.5, 1.25)];
        let back: Vec<Slice> = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(v, back);
        // Backwards compatibility: legacy `{start, end}` JSON deserializes with rich defaults.
        let legacy: Slice = serde_json::from_str(r#"{"start":0.25,"end":0.75}"#).unwrap();
        assert_eq!((legacy.start, legacy.end), (0.25, 0.75));
        assert_eq!(legacy.focus, (0.5, 0.5));
        assert_eq!(legacy.role_hint, SliceRole::Raw);
        assert!(legacy.caption.is_empty());
    }

    #[test]
    fn onset_5band_preserves_opening_consonant_and_snaps_zero_crossing() {
        let sr = 48_000u32;
        let mut samples = vec![0.0f32; sr as usize * 2];
        // Spoken-word model: 28ms soft sibilant/consonant lead-in at 0.472..0.500s
        // followed by a strong 220 Hz + 770 Hz vowel body at 0.500..0.850s.
        let c0 = (0.472 * sr as f64) as usize;
        let v0 = (0.500 * sr as f64) as usize;
        let v1 = (0.850 * sr as f64) as usize;
        for i in c0..v0 {
            let env = (i - c0) as f32 / (v0 - c0) as f32;
            let t = i as f32 / sr as f32;
            samples[i] = 0.14 * env * ((2.0 * std::f32::consts::PI * 1450.0 * t).sin()
                + 0.7 * (2.0 * std::f32::consts::PI * 2900.0 * t).sin());
        }
        for i in v0..v1 {
            let decay = 1.0 - 0.5 * ((i - v0) as f32 / (v1 - v0) as f32);
            let t = i as f32 / sr as f32;
            samples[i] = 0.75 * decay * ((2.0 * std::f32::consts::PI * 220.0 * t).sin()
                + 0.65 * (2.0 * std::f32::consts::PI * 660.0 * t).sin()
                + 0.35 * (2.0 * std::f32::consts::PI * 1100.0 * t).sin());
        }
        let audio = std::sync::Arc::new(AudioClip::new(samples, sr));
        let src = crate::timeline::Source {
            name: "speech".into(),
            audio: Some(audio.clone()),
            ..Default::default()
        };
        let slices = analyze_source_slices(&src, 8);
        let hit = slices
            .iter()
            .find(|sl| sl.start > 0.20 && sl.start <= 0.505 && sl.end >= 0.70)
            .unwrap_or_else(|| panic!("expected spoken phrase slice in {slices:?}"));
        assert!(
            hit.start <= 0.490,
            "consonant pre-roll preserved opening onset before vowel peak (start={:.4}s)",
            hit.start
        );
        let idx = ((hit.start * sr as f64).round() as usize).min(audio.samples.len() - 1);
        assert!(
            audio.samples[idx].abs() < 0.05,
            "snapped to asymmetric zero crossing: sample={}",
            audio.samples[idx]
        );
    }

    #[test]
    fn nsdf_pitch_and_video_motion_focus_enrich_slices() {
        let sr = 48_000u32;
        let audio = AudioClip::sine(329.63, 1.0, sr); // E4
        // Video clip where motion happens exclusively in the right third (x ~ 0.78).
        let mut frames = Vec::new();
        for fi in 0..12 {
            let mut f = crate::video::Frame::black(64, 36);
            let x_box = 48 + (fi % 3) * 2;
            for y in 10..26 {
                for x in x_box..(x_box + 8).min(64) {
                    f.put(x as u32, y, [240, 220, 180, 255]);
                }
            }
            frames.push(f);
        }
        let video = crate::video::VideoClip { frames, fps: 12.0 };
        let mut slices = vec![Slice::new(0.0, 1.0)];
        enrich_slices(&mut slices, Some(&audio), Some(&video));
        let sl = slices[0];
        let f0 = sl.f0_hz.expect("detected harmonic f0");
        assert!((f0 - 329.63).abs() < 5.0, "NSDF f0={f0}");
        assert!(sl.pitch_confidence > 0.85, "high pitch confidence={}", sl.pitch_confidence);
        assert!(sl.focus.0 > 0.65 && sl.focus.0 <= 0.85, "right-third motion focus={:?}", sl.focus);
    }

    #[test]
    fn build_kit_and_tape_loops_curate_across_multiple_sources() {
        let sr = 48_000u32;
        // Source 0: low percussive thud (0..0.35s) + crisp high metallic tick (0.6..0.8s).
        let mut s0 = vec![0.0f32; sr as usize * 2];
        for i in 0..(0.22 * sr as f64) as usize {
            let t = i as f32 / sr as f32;
            let env = (-t / 0.055).exp();
            let f = 52.0 + 90.0 * (-t / 0.02).exp();
            s0[i] = 0.9 * env * (2.0 * std::f32::consts::PI * f * t).sin();
        }
        let h0 = (0.60 * sr as f64) as usize;
        let mut rng = crate::dsp::Rng::new(99);
        for i in 0..(0.09 * sr as f64) as usize {
            let env = (-(i as f32 / sr as f32) / 0.018).exp();
            // High-frequency alternating metallic burst.
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            s0[h0 + i] = 0.65 * env * sign * (0.5 + 0.5 * rng.next_f32());
        }
        let mut src0 = crate::timeline::Source {
            name: "perc_clip".into(),
            audio: Some(std::sync::Arc::new(AudioClip::new(s0, sr))),
            ..Default::default()
        };
        src0.slices = vec![Slice::new(0.0, 0.32), Slice::new(0.60, 0.78)];
        enrich_slices(&mut src0.slices, src0.audio.as_deref(), None);

        // Source 1: mid-band snare crack (0..0.25s) + sustained D4 tonal note (0.5..5.5s).
        let mut s1 = vec![0.0f32; sr as usize * 6];
        for i in 0..(0.16 * sr as f64) as usize {
            let t = i as f32 / sr as f32;
            let env = (-t / 0.035).exp();
            s1[i] = 0.8 * env * ((2.0 * std::f32::consts::PI * 240.0 * t).sin() + 0.8 * rng.bipolar());
        }
        for i in (0.50 * sr as f64) as usize..(5.50 * sr as f64) as usize {
            let t = i as f32 / sr as f32;
            s1[i] = 0.55 * (2.0 * std::f32::consts::PI * 293.66 * t).sin(); // D4
        }
        let mut src1 = crate::timeline::Source {
            name: "tonal_clip".into(),
            audio: Some(std::sync::Arc::new(AudioClip::new(s1, sr))),
            base_hz: 293.66,
            ..Default::default()
        };
        src1.slices = vec![Slice::new(0.0, 0.25), Slice::new(0.50, 1.40)];
        enrich_slices(&mut src1.slices, src1.audio.as_deref(), None);

        // Source 2: spoken phrase slice (0.1..0.95s) + warm ambient bed (1.2..7.5s).
        let mut s2 = vec![0.0f32; sr as usize * 8];
        for i in (0.10 * sr as f64) as usize..(0.95 * sr as f64) as usize {
            let t = i as f32 / sr as f32;
            let syll = (2.0 * std::f32::consts::PI * 4.5 * t).sin().abs();
            s2[i] = 0.65 * syll * ((2.0 * std::f32::consts::PI * 520.0 * t).sin()
                + 0.6 * (2.0 * std::f32::consts::PI * 1150.0 * t).sin());
        }
        for i in (1.20 * sr as f64) as usize..(7.50 * sr as f64) as usize {
            let t = i as f32 / sr as f32;
            s2[i] = 0.45 * ((2.0 * std::f32::consts::PI * 261.63 * t).sin()
                + 0.5 * (2.0 * std::f32::consts::PI * 392.0 * t).sin());
        }
        let mut src2 = crate::timeline::Source {
            name: "vocal_ambient".into(),
            audio: Some(std::sync::Arc::new(AudioClip::new(s2, sr))),
            ..Default::default()
        };
        let mut phrase_sl = Slice::new(0.10, 0.95);
        phrase_sl.caption = Caption::new("FRONTIER PSYCHIATRIST");
        src2.slices = vec![phrase_sl, Slice::new(1.20, 4.80)];
        enrich_slices(&mut src2.slices, src2.audio.as_deref(), None);

        let sources = vec![src0, src1, src2];
        let key = crate::music::Key::new(0, crate::music::ScaleKind::Minor);
        let kit = build_kit_across_sources(&sources, key);
        assert!(kit.len() >= 8 && kit.len() <= 16, "kit len={}", kit.len());
        let used_sources: std::collections::BTreeSet<usize> = kit.iter().map(|p| p.source).collect();
        assert_eq!(used_sources.len(), 3, "kit pulls across all 3 clips: {used_sources:?}");
        assert!(kit.iter().any(|p| p.stamp == crate::grain::Stamp::Kick));
        assert!(kit.iter().any(|p| p.stamp == crate::grain::Stamp::Snare));
        assert!(kit.iter().any(|p| p.stamp == crate::grain::Stamp::Hat && p.choke_group == 3));
        assert!(kit.iter().any(|p| p.label.contains("Stutter")), "paired vocal stutter pad");
        assert!(kit.iter().any(|p| p.slice.caption.as_str() == "FRONTIER PSYCHIATRIST"));

        let loops = build_tape_loops_across_sources(&sources, Some(key));
        assert!(!loops.is_empty(), "found rich ambient tape-loop spans");
        for lp in &loops {
            let secs = lp.sculpt.free_loop_secs.expect("unsynced real-seconds loop");
            assert!((3.5..=14.5).contains(&secs), "free_loop_secs={secs}");
        }
    }
}
