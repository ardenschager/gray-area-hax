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
    }
}
