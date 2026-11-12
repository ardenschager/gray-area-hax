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
use serde::{Deserialize, Serialize};

/// Shortest slice, seconds.
pub const MIN_SLICE: f64 = 0.01;
/// MIDI note of slice 1 (C3); slice n is `SLICE_NOTE + n`.
pub const SLICE_NOTE: u8 = 48;

/// A span of source time, seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slice {
    pub start: f64,
    pub end: f64,
}

impl Slice {
    pub fn new(start: f64, end: f64) -> Slice {
        Slice { start: start.min(end), end: start.max(end) }
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
}

/// Which edge of a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// Sort, clip to `0..len`, drop too-short slices and resolve overlaps (a
/// later slice starts where the previous ended).
pub fn normalize(slices: &mut Vec<Slice>, len: f64) {
    let len = len.max(0.0);
    for s in slices.iter_mut() {
        *s = Slice::new(s.start.clamp(0.0, len), s.end.clamp(0.0, len));
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

/// Transient slices (at most `max`), from the audio's onsets.
pub fn auto_slices(audio: &AudioClip, max: usize) -> Vec<Slice> {
    let mut v: Vec<Slice> = audio.detect_slices(max).into_iter().map(|(a, b)| Slice::new(a as f64, b as f64)).collect();
    normalize(&mut v, audio.duration());
    v
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
