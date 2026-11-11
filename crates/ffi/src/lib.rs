//! C ABI over [`Instrument`]: an opaque `CgInstrument*` plus one
//! `extern "C"` function per method. `include/chromagrain.h` is generated
//! from this file by cbindgen (see build.rs).
//!
//! Conventions: every function tolerates a NULL instrument (no-op /
//! zero result). Source ids are `int32_t`, -1 meaning "none". Keys are a
//! root pitch class 0..11 (or -1 for chromatic) plus a [`CgScale`].
//! Realtime safety is as documented on [`Instrument`]: process, notes,
//! macros, transport and grain snapshots don't allocate; construction,
//! sources and keys do.

#![allow(clippy::missing_safety_doc)]

use chromagrain_core::instrument::{Instrument, Macro, Transport};
use chromagrain_core::music::{Key, ScaleKind};
use std::ffi::{c_char, CStr};

/// Opaque instrument handle.
pub struct CgInstrument(Instrument);

/// The four performance macros (values 0..1).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgMacro {
    /// Sample (0) to grain cloud (1).
    Grain = 0,
    /// Transposition -12..+12 semitones, 0.5 = none, snapped to the key.
    Pitch = 1,
    /// Time smear: read head slows toward a freeze, echo + reverb swell.
    Smear = 2,
    /// Steps through the one-click looks.
    Look = 3,
}

/// Scales for [`cg_instrument_set_key`].
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgScale {
    Chromatic = 0,
    Major = 1,
    Minor = 2,
    HarmonicMinor = 3,
    MajorPentatonic = 4,
    MinorPentatonic = 5,
    Blues = 6,
    Dorian = 7,
    Phrygian = 8,
    Lydian = 9,
    Mixolydian = 10,
    WholeTone = 11,
}

const SCALES: [ScaleKind; 12] = [
    ScaleKind::Chromatic,
    ScaleKind::Major,
    ScaleKind::Minor,
    ScaleKind::HarmonicMinor,
    ScaleKind::MajorPentatonic,
    ScaleKind::MinorPentatonic,
    ScaleKind::Blues,
    ScaleKind::Dorian,
    ScaleKind::Phrygian,
    ScaleKind::Lydian,
    ScaleKind::Mixolydian,
    ScaleKind::WholeTone,
];

/// Host tempo + transport.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CgTransport {
    pub bpm: f64,
    pub playing: bool,
    /// Song position in beats (quarter notes) at the start of the next block.
    pub beat: f64,
}

/// One visual grain on screen (see [`cg_instrument_visible_grains`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CgGrain {
    pub source: i32,
    /// Instrument-clock seconds the grain starts.
    pub onset: f64,
    /// Seconds into the source media.
    pub source_pos: f64,
    pub duration: f32,
    pub pitch_ratio: f32,
    pub gain: f32,
    /// -1 (left) .. 1 (right); also the grain's horizontal drift.
    pub pan: f32,
    pub reverse: bool,
}

unsafe fn get<'a>(p: *const CgInstrument) -> Option<&'a Instrument> {
    p.as_ref().map(|c| &c.0)
}

unsafe fn get_mut<'a>(p: *mut CgInstrument) -> Option<&'a mut Instrument> {
    p.as_mut().map(|c| &mut c.0)
}

unsafe fn slice<'a, T>(p: *const T, len: usize) -> &'a [T] {
    if p.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p, len)
    }
}

/// A new instrument at `sample_rate`. Free with [`cg_instrument_free`].
#[no_mangle]
pub extern "C" fn cg_instrument_new(sample_rate: u32) -> *mut CgInstrument {
    Box::into_raw(Box::new(CgInstrument(Instrument::new(sample_rate))))
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_free(inst: *mut CgInstrument) {
    if !inst.is_null() {
        drop(Box::from_raw(inst));
    }
}

/// Change the output rate (re-activation). Silences everything.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_reset(inst: *mut CgInstrument, sample_rate: u32) {
    if let Some(i) = get_mut(inst) {
        i.reset(sample_rate);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_sample_rate(inst: *const CgInstrument) -> u32 {
    get(inst).map(|i| i.sample_rate()).unwrap_or(0)
}

/// Hand over decoded media. `pcm` is mono (`channels` = 1) or interleaved
/// stereo float samples (`pcm_len` floats in total); `rgba` is `rgba_len`
/// bytes of tightly packed `width` x `height` RGBA8 frames at `fps`. Either
/// may be NULL/empty. `name` is UTF-8 (may be NULL). `base_hz` is the
/// media's pitch for key snapping (0 = unknown). Returns the source id
/// (the first source becomes active), or -1. Allocates.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_add_source_pcm_rgba(
    inst: *mut CgInstrument,
    name: *const c_char,
    pcm: *const f32,
    pcm_len: usize,
    channels: u32,
    pcm_rate: u32,
    rgba: *const u8,
    rgba_len: usize,
    width: u32,
    height: u32,
    fps: f32,
    base_hz: f32,
) -> i32 {
    let Some(i) = get_mut(inst) else { return -1 };
    let name = if name.is_null() { String::from("source") } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
    let id = i.add_source_pcm_rgba(&name, slice(pcm, pcm_len), channels, pcm_rate, slice(rgba, rgba_len), width, height, fps, base_hz);
    i32::try_from(id).unwrap_or(-1)
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_source_count(inst: *const CgInstrument) -> usize {
    get(inst).map(|i| i.sources().len()).unwrap_or(0)
}

/// Seconds of media in source `id` (0 if unknown).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_source_duration(inst: *const CgInstrument, id: i32) -> f64 {
    get(inst).and_then(|i| usize::try_from(id).ok().and_then(|id| i.sources().get(id))).map(|s| s.duration()).unwrap_or(0.0)
}

/// Which source new notes play. Returns false for an unknown id.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_select_source(inst: *mut CgInstrument, id: i32) -> bool {
    match (get_mut(inst), usize::try_from(id)) {
        (Some(i), Ok(id)) => i.select_source(id),
        _ => false,
    }
}

/// The active source id, or -1.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_active_source(inst: *const CgInstrument) -> i32 {
    get(inst).and_then(|i| i.active_source()).and_then(|id| i32::try_from(id).ok()).unwrap_or(-1)
}

/// Set a macro (`m` is a [`CgMacro`]; unknown values are ignored).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_macro(inst: *mut CgInstrument, m: u32, value: f32) {
    if let (Some(i), Some(m)) = (get_mut(inst), Macro::from_index(m)) {
        i.set_macro(m, value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_macro_value(inst: *const CgInstrument, m: u32) -> f32 {
    match (get(inst), Macro::from_index(m)) {
        (Some(i), Some(m)) => i.macro_value(m),
        _ => 0.0,
    }
}

/// The global key: `root` 0..11 (C..B) with a [`CgScale`], or `root` < 0
/// for chromatic. Allocates (call off the audio thread).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_key(inst: *mut CgInstrument, root: i32, scale: u32) {
    if let Some(i) = get_mut(inst) {
        let key = (root >= 0).then(|| Key::new(root, SCALES.get(scale as usize).copied().unwrap_or(ScaleKind::Chromatic)));
        i.set_key(key);
    }
}

/// The key's root pitch class, or -1 when chromatic.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_key_root(inst: *const CgInstrument) -> i32 {
    get(inst).and_then(|i| i.key()).map(|k| k.root).unwrap_or(-1)
}

/// The key's [`CgScale`] (Chromatic when no key).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_key_scale(inst: *const CgInstrument) -> u32 {
    get(inst).and_then(|i| i.key()).and_then(|k| SCALES.iter().position(|s| *s == k.scale)).unwrap_or(0) as u32
}

/// Snap grain onsets to a beat grid (0.25 = sixteenths, 0 = free).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_sync(inst: *mut CgInstrument, div_beats: f32) {
    if let Some(i) = get_mut(inst) {
        i.set_sync(div_beats);
    }
}

/// Lock new notes' read head to the song position while the host plays.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_follow_transport(inst: *mut CgInstrument, on: bool) {
    if let Some(i) = get_mut(inst) {
        i.set_follow_transport(on);
    }
}

/// Where in the media new notes start, 0..1.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_position(inst: *mut CgInstrument, pos: f32) {
    if let Some(i) = get_mut(inst) {
        i.set_position(pos);
    }
}

/// Host transport for the next block.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_set_transport(inst: *mut CgInstrument, t: CgTransport) {
    if let Some(i) = get_mut(inst) {
        i.set_transport(Transport { bpm: t.bpm, playing: t.playing, beat: t.beat });
    }
}

/// Transport as of now (beat advanced through processed blocks).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_transport(inst: *const CgInstrument) -> CgTransport {
    let t = get(inst).map(|i| i.transport()).unwrap_or_default();
    CgTransport { bpm: t.bpm, playing: t.playing, beat: t.beat }
}

/// Instrument clock: seconds of audio processed.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_now_secs(inst: *const CgInstrument) -> f64 {
    get(inst).map(|i| i.now_secs()).unwrap_or(0.0)
}

/// Velocity 0..1 (0 acts as note-off). C4 (60) plays the media at its pitch.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_note_on(inst: *mut CgInstrument, note: u8, velocity: f32) {
    if let Some(i) = get_mut(inst) {
        i.note_on(note, velocity);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_note_off(inst: *mut CgInstrument, note: u8) {
    if let Some(i) = get_mut(inst) {
        i.note_off(note);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_all_notes_off(inst: *mut CgInstrument) {
    if let Some(i) = get_mut(inst) {
        i.all_notes_off();
    }
}

#[no_mangle]
pub unsafe extern "C" fn cg_instrument_held_note_count(inst: *const CgInstrument) -> usize {
    get(inst).map(|i| i.held_notes().count()).unwrap_or(0)
}

/// Samples of latency the current look adds (report to the host).
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_latency_samples(inst: *const CgInstrument) -> usize {
    get(inst).map(|i| i.latency_samples()).unwrap_or(0)
}

/// Fill `frames` samples of stereo audio into `left` and `right`
/// (overwritten). Realtime-safe for blocks up to 8192 frames.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_process(inst: *mut CgInstrument, left: *mut f32, right: *mut f32, frames: usize) {
    if left.is_null() || right.is_null() || frames == 0 {
        return;
    }
    let (l, r) = (std::slice::from_raw_parts_mut(left, frames), std::slice::from_raw_parts_mut(right, frames));
    match get_mut(inst) {
        Some(i) => i.process(l, r),
        None => {
            l.fill(0.0);
            r.fill(0.0);
        }
    }
}

/// The grains on screen at instrument time `t` (pass
/// [`cg_instrument_now_secs`] for "now"): writes up to `cap` into `out` and
/// returns how many there are in total. Realtime-safe.
#[no_mangle]
pub unsafe extern "C" fn cg_instrument_visible_grains(inst: *const CgInstrument, t: f64, out: *mut CgGrain, cap: usize) -> usize {
    let Some(i) = get(inst) else { return 0 };
    let mut n = 0;
    for (sid, e) in i.video_frame_events(t) {
        if n < cap && !out.is_null() {
            *out.add(n) = CgGrain {
                source: i32::try_from(sid).unwrap_or(-1),
                onset: e.onset,
                source_pos: e.source_pos,
                duration: e.duration,
                pitch_ratio: e.pitch_ratio,
                gain: e.gain,
                pan: e.pan,
                reverse: e.reverse,
            };
        }
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_side_round_trip() {
        unsafe {
            let i = cg_instrument_new(48_000);
            let pcm: Vec<f32> = (0..48_000).map(|k| (k as f32 * 0.03).sin() * 0.5).collect();
            let id = cg_instrument_add_source_pcm_rgba(i, std::ptr::null(), pcm.as_ptr(), pcm.len(), 1, 48_000, std::ptr::null(), 0, 0, 0, 0.0, 0.0);
            assert_eq!((id, cg_instrument_source_count(i), cg_instrument_active_source(i)), (0, 1, 0));
            cg_instrument_set_key(i, 9, CgScale::Minor as u32);
            assert_eq!((cg_instrument_key_root(i), cg_instrument_key_scale(i)), (9, CgScale::Minor as u32));
            cg_instrument_set_macro(i, CgMacro::Grain as u32, 0.5);
            assert_eq!(cg_instrument_macro_value(i, CgMacro::Grain as u32), 0.5);
            cg_instrument_note_on(i, 60, 1.0);
            let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
            cg_instrument_process(i, l.as_mut_ptr(), r.as_mut_ptr(), l.len());
            assert!(l.iter().any(|s| s.abs() > 0.01));
            let mut g = [CgGrain::default(); 4];
            assert!(cg_instrument_visible_grains(i, cg_instrument_now_secs(i), g.as_mut_ptr(), g.len()) > 0);
            cg_instrument_free(i);
            // NULL is tolerated everywhere.
            cg_instrument_note_on(std::ptr::null_mut(), 60, 1.0);
            assert_eq!(cg_instrument_source_count(std::ptr::null()), 0);
        }
    }
}
