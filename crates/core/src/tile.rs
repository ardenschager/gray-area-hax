//! Tiling: every sounding voice gets its own tile in an automatic grid.
//!
//! * [`Layout::Stack`] is the classic layered compositing.
//! * In [`Layout::Tile`], voices get tiles in a grid that grows with the
//!   number of voices: 1 → 2 (2×1) → 2×2 → 3×3 ([`grid_for`]).
//!
//! Slot assignment is stable: a voice keeps its slot for its whole life,
//! and new voices take the lowest free slot ([`TileAssigner`]). The grid
//! never shrinks under a live voice, so tiles don't jump.
//!
//! Voices are assigned when they *start* (by the code that schedules their
//! grains: Instrument, render plan, UI emitters). The slot rides on every
//! grain (`GrainEvent::tag.tile`), so the compositor only needs the grains
//! at frame time, and audio can use the slot too: [`layout_pan`] maps the
//! column to pan and the row to a subtle elevation (gain) cue.
//!
//! [`frame_grid`] + [`tile_rect`] are the pure functions the CPU compositor
//! and the GPU tile pass both use: normalized rects with a hairline gap.

use serde::{Deserialize, Serialize};

/// Most tiles at once (3×3). More voices share: the extra voice takes slot
/// `voice % MAX_TILES`.
pub const MAX_TILES: usize = 9;
/// Gap between tiles, as a fraction of the canvas' shorter side.
pub const TILE_GAP: f32 = 0.004;
/// Fractional interior-edge overlap in [`Layout::Split`] (`+6.5%`) for
/// feathered optical-screen seam blending.
pub const SPLIT_BLEED_OVERLAP: f32 = 0.065;

/// Output stage aspect ratio (`16:9`, `1:1`, `9:16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AspectRatio {
    #[default]
    Wide16x9,
    Square1x1,
    Vertical9x16,
}

impl AspectRatio {
    pub const ALL: [AspectRatio; 3] = [
        AspectRatio::Wide16x9,
        AspectRatio::Square1x1,
        AspectRatio::Vertical9x16,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AspectRatio::Wide16x9 => "16:9",
            AspectRatio::Square1x1 => "1:1",
            AspectRatio::Vertical9x16 => "9:16",
        }
    }

    pub fn parse(s: &str) -> Option<AspectRatio> {
        match s.trim().to_ascii_lowercase().as_str() {
            "16:9" | "16x9" | "wide" | "landscape" => Some(AspectRatio::Wide16x9),
            "1:1" | "1x1" | "square" => Some(AspectRatio::Square1x1),
            "9:16" | "9x16" | "vertical" | "portrait" => Some(AspectRatio::Vertical9x16),
            _ => None,
        }
    }

    pub fn ratio(self) -> f32 {
        match self {
            AspectRatio::Wide16x9 => 16.0 / 9.0,
            AspectRatio::Square1x1 => 1.0,
            AspectRatio::Vertical9x16 => 9.0 / 16.0,
        }
    }

    /// Full-HD export dimensions `(width, height)`.
    pub fn hd_dimensions(self) -> (u32, u32) {
        match self {
            AspectRatio::Wide16x9 => (1920, 1080),
            AspectRatio::Square1x1 => (1080, 1080),
            AspectRatio::Vertical9x16 => (1080, 1920),
        }
    }

    /// Even `(width, height)` scaled to a target long-edge pixel count.
    pub fn dimensions_for_long_edge(self, long_edge: u32) -> (u32, u32) {
        let le = (long_edge.max(16)) & !1;
        match self {
            AspectRatio::Wide16x9 => {
                let h = (((le as f32 * 9.0 / 16.0).round() as u32).max(16)) & !1;
                (le, h)
            }
            AspectRatio::Square1x1 => (le, le),
            AspectRatio::Vertical9x16 => {
                let w = (((le as f32 * 9.0 / 16.0).round() as u32).max(16)) & !1;
                (w, le)
            }
        }
    }
}

/// Compositing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
    /// Layered spatial grain compositing (default).
    #[default]
    Stack,
    /// Feathered Split + Bleed (`1 -> full`, `2 -> 2-up`, `3 -> triptych`,
    /// `4 -> 2x2`) with `+6.5%` screen-blended interior seams and sustained
    /// wash voices bleeding full-screen at `30%`.
    Split,
    /// Crisp architectural Grid (`1.5px` obsidian dividers).
    Tile,
    /// Full-canvas optical luma-key + screen multi-exposure melt.
    Bleed,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Stack, Layout::Split, Layout::Tile, Layout::Bleed];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Stack => "stack",
            Layout::Split => "split",
            Layout::Tile => "tile",
            Layout::Bleed => "bleed",
        }
    }

    /// Short stage pill label (`stack | split | grid | bleed`).
    pub fn label(self) -> &'static str {
        match self {
            Layout::Stack => "stack",
            Layout::Split => "split",
            Layout::Tile => "grid",
            Layout::Bleed => "bleed",
        }
    }

    /// True when discrete voices claim spatial panel slots (`Split` or `Tile`).
    pub fn uses_tiles(self) -> bool {
        matches!(self, Layout::Split | Layout::Tile)
    }

    pub fn parse(s: &str) -> Option<Layout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stack" => Some(Layout::Stack),
            "split" | "split+bleed" | "smart" => Some(Layout::Split),
            "tile" | "tiles" | "grid" => Some(Layout::Tile),
            "bleed" | "blend" | "fullbleed" => Some(Layout::Bleed),
            _ => None,
        }
    }
}

/// Architectural split grid `(cols, rows)` for [`Layout::Split`]:
/// `1 -> 1x1`, `2 -> 2x1` (`1x2` in portrait `9:16`), `3 -> 3x1` triptych
/// (`1x3` in portrait), `4 -> 2x2`, `5..=9 -> 3x3`.
pub fn split_grid_for(n: usize, aspect: f32) -> (u32, u32) {
    let portrait = aspect.is_finite() && aspect < 0.85;
    match n {
        0 | 1 => (1, 1),
        2 => if portrait { (1, 2) } else { (2, 1) },
        3 => if portrait { (1, 3) } else { (3, 1) },
        4 => (2, 2),
        _ => (3, 3),
    }
}

/// Split-grid `(cols, rows)` for one frame from active discrete voice slots.
pub fn frame_split_grid(slots: impl IntoIterator<Item = u8>, aspect: f32) -> (u32, u32) {
    let (mut n, mut hi) = (0usize, 0usize);
    let mut seen = [false; MAX_TILES];
    for s in slots {
        let s = (s as usize).min(MAX_TILES - 1);
        if !seen[s] {
            seen[s] = true;
            n += 1;
            hi = hi.max(s + 1);
        }
    }
    split_grid_for(n.max(hi), aspect)
}

/// *MODELL 5* (Hentschläger & Langheinrich, 1994) vertical portrait-column grid
/// on wide canvases (`aspect > 1.3`): `1 -> 1×1`, `2 -> 2×1`, `3 -> 3×1`
/// triptych monoliths, `4 -> 4×1` quad portrait strips (`1×N` in portrait `9:16`,
/// `3×3` for `5+`).
pub fn portrait_grid_for(n: usize, aspect: f32) -> (u32, u32) {
    let wide = !aspect.is_finite() || aspect > 1.3;
    let portrait = aspect.is_finite() && aspect < 0.85;
    match n {
        0 | 1 => (1, 1),
        2 => if portrait { (1, 2) } else { (2, 1) },
        3 => if wide { (3, 1) } else if portrait { (1, 3) } else { (2, 2) },
        4 => if wide { (4, 1) } else if portrait { (1, 4) } else { (2, 2) },
        _ => (3, 3),
    }
}

/// *MODELL 5* portrait-column grid `(cols, rows)` for one frame from active slots.
pub fn frame_portrait_grid(slots: impl IntoIterator<Item = u8>, aspect: f32) -> (u32, u32) {
    let (mut n, mut hi) = (0usize, 0usize);
    let mut seen = [false; MAX_TILES];
    for s in slots {
        let s = (s as usize).min(MAX_TILES - 1);
        if !seen[s] {
            seen[s] = true;
            n += 1;
            hi = hi.max(s + 1);
        }
    }
    portrait_grid_for(n.max(hi), aspect)
}

/// Feathered panel rect and interior-edge flags `[left, right, top, bottom]`
/// for `slot` in [`Layout::Split`]. Interior shared seams expand symmetrically
/// by [`SPLIT_BLEED_OVERLAP`] (`+6.5%`) for optical screen feathering.
pub fn split_rect(slot: usize, cols: u32, rows: u32) -> (TileRect, [bool; 4]) {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let slot = slot.min((cols * rows) as usize - 1) as u32;
    let (c, r) = (slot % cols, slot / cols);
    let base_w = 1.0 / cols as f32;
    let base_h = 1.0 / rows as f32;
    let base_x = c as f32 * base_w;
    let base_y = r as f32 * base_h;
    let fl = c > 0;
    let fr = c + 1 < cols;
    let ft = r > 0;
    let fb = r + 1 < rows;
    let ox = SPLIT_BLEED_OVERLAP * base_w;
    let oy = SPLIT_BLEED_OVERLAP * base_h;
    let x0 = if fl { (base_x - ox).max(0.0) } else { base_x };
    let x1 = if fr { (base_x + base_w + ox).min(1.0) } else { base_x + base_w };
    let y0 = if ft { (base_y - oy).max(0.0) } else { base_y };
    let y1 = if fb { (base_y + base_h + oy).min(1.0) } else { base_y + base_h };
    (TileRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, [fl, fr, ft, fb])
}

/// Grid (cols, rows) that holds `n` tiles: 1 → 1×1, 2 → 2×1, 3–4 → 2×2,
/// 5–9 → 3×3.
pub fn grid_for(n: usize) -> (u32, u32) {
    match n {
        0 | 1 => (1, 1),
        2 => (2, 1),
        3 | 4 => (2, 2),
        _ => (3, 3),
    }
}

/// A rect in normalized canvas coordinates (0..1, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TileRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl TileRect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// Rect of `slot` (row-major) in a `cols`×`rows` grid on a canvas of
/// `aspect` = width / height, with a [`TILE_GAP`] hairline between tiles
/// (none at the canvas edge).
pub fn tile_rect(slot: usize, cols: u32, rows: u32, aspect: f32) -> TileRect {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let slot = slot.min((cols * rows) as usize - 1) as u32;
    let (c, r) = (slot % cols, slot / cols);
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    // Gap in normalized units on each axis (same pixel width both ways).
    let (gx, gy) = if aspect >= 1.0 { (TILE_GAP / aspect, TILE_GAP) } else { (TILE_GAP, TILE_GAP * aspect) };
    let w = (1.0 - gx * (cols - 1) as f32) / cols as f32;
    let h = (1.0 - gy * (rows - 1) as f32) / rows as f32;
    TileRect { x: c as f32 * (w + gx), y: r as f32 * (h + gy), w, h }
}

/// Grid for one frame from the tile slots of the voices visible in it
/// (pure: the same grains give the same grid on CPU and GPU). Big enough
/// for the number of voices and the highest slot in use.
pub fn frame_grid(slots: impl IntoIterator<Item = u8>) -> (u32, u32) {
    let (mut n, mut hi) = (0usize, 0usize);
    let mut seen = [false; MAX_TILES];
    for s in slots {
        let s = (s as usize).min(MAX_TILES - 1);
        if !seen[s] {
            seen[s] = true;
            n += 1;
            hi = hi.max(s + 1);
        }
    }
    grid_for(n.max(hi))
}

/// Piecewise-constant tile grid over time from voice spans
/// `(slot, start, end)`: `(from, grid)` segments sorted by `from` (the
/// grid holds until the next segment). Pair with [`grid_in`]. Used where
/// every voice is known up front (offline render plans).
pub fn grid_timeline(spans: &[(u8, f64, f64)]) -> Vec<(f64, (u32, u32))> {
    let mut edges: Vec<(f64, i32, usize)> = Vec::with_capacity(spans.len() * 2);
    for &(slot, start, end) in spans {
        let slot = (slot as usize).min(MAX_TILES - 1);
        if end > start {
            edges.push((start, 1, slot));
            edges.push((end, -1, slot));
        }
    }
    // Ends before starts at the same instant (a voice at `t` is live iff
    // start <= t < end).
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut count = [0i32; MAX_TILES];
    let mut out: Vec<(f64, (u32, u32))> = vec![(f64::NEG_INFINITY, (1, 1))];
    let mut k = 0;
    while k < edges.len() {
        let t = edges[k].0;
        while k < edges.len() && edges[k].0 == t {
            count[edges[k].2] += edges[k].1;
            k += 1;
        }
        let grid = frame_grid((0..MAX_TILES).filter(|&i| count[i] > 0).map(|i| i as u8));
        if out.last().is_some_and(|l| l.1 != grid) {
            out.push((t, grid));
        }
    }
    out
}

/// The grid at `t` in a [`grid_timeline`].
pub fn grid_in(timeline: &[(f64, (u32, u32))], t: f64) -> (u32, u32) {
    let i = timeline.partition_point(|s| s.0 <= t);
    if i == 0 {
        (1, 1)
    } else {
        timeline[i - 1].1
    }
}

/// Pan (-1..1) and gain cue for a tile: column → pan across ±0.8, row →
/// a subtle elevation cue (top row full level, each row lower −1 dB).
pub fn layout_pan(slot: usize, cols: u32, rows: u32) -> (f32, f32) {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let slot = slot.min((cols * rows) as usize - 1) as u32;
    let (c, r) = (slot % cols, slot / cols);
    let pan = if cols > 1 { -0.8 + 1.6 * c as f32 / (cols - 1) as f32 } else { 0.0 };
    let gain = 10f32.powf(-(r as f32) / 20.0);
    (pan, gain)
}

/// Stable slot assignment for live voices. Allocation-free once built
/// (fixed table), so the audio thread can own one.
#[derive(Debug, Clone, Default)]
pub struct TileAssigner {
    /// Voice in each slot, when it started, and when it ends
    /// (`f64::INFINITY` while held).
    slots: [Option<(u64, f64, f64)>; MAX_TILES],
    /// Affinity key of each slot's latest voice (see [`TileAssigner::assign_keyed`]).
    keys: [Option<u64>; MAX_TILES],
}

impl TileAssigner {
    pub fn new() -> TileAssigner {
        TileAssigner::default()
    }

    /// Free the slots of voices that ended at or before `t`.
    pub fn expire(&mut self, t: f64) {
        for s in self.slots.iter_mut() {
            if s.is_some_and(|(_, _, end)| end <= t) {
                *s = None;
            }
        }
    }

    /// Slot for `voice` starting at `t`: its existing slot, else the
    /// lowest free one (after expiring ended voices), else shared
    /// (`voice % MAX_TILES`).
    pub fn assign(&mut self, voice: u64, t: f64) -> u8 {
        if let Some(i) = self.slot_of(voice) {
            return i as u8;
        }
        self.expire(t);
        match self.slots.iter().position(|s| s.is_none()) {
            Some(i) => {
                self.slots[i] = Some((voice, t, f64::INFINITY));
                i as u8
            }
            None => (voice % MAX_TILES as u64) as u8,
        }
    }

    /// Like [`TileAssigner::assign`], for voices with an affinity `key`
    /// (a track, a pad, a lane) that holds one tile: a new voice takes
    /// over the slot of a still-sounding voice with the same key (its tail
    /// shares the tile), else takes back a free slot its key used last,
    /// else the lowest free one. So a looping pad's next pass or a lane's
    /// next hit stays in its tile instead of trading places.
    pub fn assign_keyed(&mut self, voice: u64, key: u64, t: f64) -> u8 {
        if let Some(i) = self.slot_of(voice) {
            return i as u8;
        }
        self.expire(t);
        let free = |i: &usize| self.slots[*i].is_none();
        let i = (0..MAX_TILES)
            .find(|&i| self.keys[i] == Some(key))
            .filter(|&i| self.slots[i].is_some())
            .or_else(|| (0..MAX_TILES).filter(free).find(|&i| self.keys[i] == Some(key)))
            .or_else(|| (0..MAX_TILES).find(free));
        match i {
            Some(i) => {
                self.slots[i] = Some((voice, t, f64::INFINITY));
                self.keys[i] = Some(key);
                i as u8
            }
            None => (voice % MAX_TILES as u64) as u8,
        }
    }

    /// The voice ends (fully silent) at `t`: its slot frees from then on.
    pub fn release(&mut self, voice: u64, end: f64) {
        if let Some(i) = self.slot_of(voice) {
            if let Some(s) = self.slots[i].as_mut() {
                s.2 = end;
            }
        }
    }

    pub fn slot_of(&self, voice: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.is_some_and(|(v, _, _)| v == voice))
    }

    /// Grid covering every slot in use at `t` (voices with
    /// `start <= t < end`): the grid a grain starting at `t` is drawn in,
    /// for its layout pan.
    pub fn grid_at(&self, t: f64) -> (u32, u32) {
        frame_grid(
            self.slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_some_and(|(_, start, end)| start <= t && t < end))
                .map(|(i, _)| i as u8),
        )
    }

    pub fn clear(&mut self) {
        self.slots = [None; MAX_TILES];
        self.keys = [None; MAX_TILES];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids() {
        assert_eq!(grid_for(0), (1, 1));
        assert_eq!(grid_for(1), (1, 1));
        assert_eq!(grid_for(2), (2, 1));
        assert_eq!(grid_for(3), (2, 2));
        assert_eq!(grid_for(4), (2, 2));
        assert_eq!(grid_for(5), (3, 3));
        assert_eq!(grid_for(20), (3, 3));
        assert_eq!(Layout::parse(" Tiles"), Some(Layout::Tile));
        assert_eq!(Layout::parse(Layout::Stack.name()), Some(Layout::Stack));
        assert_eq!(Layout::parse("x"), None);
    }

    #[test]
    fn tile_rects_cover_the_canvas_with_hairline_gaps() {
        let aspect = 16.0 / 9.0;
        let one = tile_rect(0, 1, 1, aspect);
        assert_eq!(one, TileRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let (l, r) = (tile_rect(0, 2, 1, aspect), tile_rect(1, 2, 1, aspect));
        assert_eq!(l.x, 0.0);
        assert!((r.x + r.w - 1.0).abs() < 1e-6);
        let gap_px_x = (r.x - (l.x + l.w)) * aspect; // in units of height
        assert!((gap_px_x - TILE_GAP).abs() < 1e-6);
        let (a, c) = (tile_rect(0, 3, 3, aspect), tile_rect(8, 3, 3, aspect));
        assert!((c.y + c.h - 1.0).abs() < 1e-6 && (c.x + c.w - 1.0).abs() < 1e-6);
        let b = tile_rect(3, 3, 3, aspect);
        assert!((b.y - (a.y + a.h) - TILE_GAP).abs() < 1e-6);
        // Out-of-range slots clamp to the last tile.
        assert_eq!(tile_rect(42, 2, 2, aspect), tile_rect(3, 2, 2, aspect));
        assert_eq!(tile_rect(0, 2, 2, f32::NAN), tile_rect(0, 2, 2, 1.0));
    }

    #[test]
    fn frame_grid_counts_distinct_and_highest_slot() {
        assert_eq!(frame_grid([]), (1, 1));
        assert_eq!(frame_grid([0, 0, 0]), (1, 1));
        assert_eq!(frame_grid([0, 1]), (2, 1));
        // Slot 2 alone still needs a grid with 3 cells.
        assert_eq!(frame_grid([2]), (2, 2));
        assert_eq!(frame_grid([0, 1, 4]), (3, 3));
        assert_eq!(frame_grid([200]), (3, 3));
    }

    #[test]
    fn layout_pan_columns_and_rows() {
        assert_eq!(layout_pan(0, 1, 1), (0.0, 1.0));
        assert_eq!(layout_pan(0, 2, 1).0, -0.8);
        assert_eq!(layout_pan(1, 2, 1).0, 0.8);
        let (p, g) = layout_pan(4, 3, 3);
        assert!(p.abs() < 1e-6);
        assert!((g - 10f32.powf(-1.0 / 20.0)).abs() < 1e-6);
        assert!(layout_pan(8, 3, 3).1 < g);
    }

    #[test]
    fn assigner_is_stable_and_takes_the_lowest_free_slot() {
        let mut a = TileAssigner::new();
        assert_eq!(a.assign(10, 0.0), 0);
        assert_eq!(a.assign(11, 0.1), 1);
        assert_eq!(a.assign(10, 0.2), 0, "a voice keeps its slot");
        assert_eq!(a.grid_at(0.2), (2, 1));
        // Voice 10 ends at 1.0; until then its slot stays taken.
        a.release(10, 1.0);
        assert_eq!(a.assign(12, 0.5), 2);
        assert_eq!(a.grid_at(0.5), (2, 2));
        // After it ends the lowest free slot is reused; the grid never
        // shrinks while slot 2 is live.
        assert_eq!(a.assign(13, 1.5), 0);
        assert_eq!(a.grid_at(1.5), (2, 2));
        assert_eq!(a.slot_of(10), None);
        for v in 0..20u64 {
            a.assign(100 + v, 2.0);
        }
        assert_eq!(a.assign(999, 2.0), (999 % MAX_TILES as u64) as u8, "overflow shares");
        a.clear();
        assert_eq!(a.assign(5, 3.0), 0);
    }

    #[test]
    fn grid_at_counts_only_live_voices() {
        let mut a = TileAssigner::new();
        a.assign(1, 0.0);
        a.assign(2, 1.0);
        assert_eq!(a.grid_at(0.5), (1, 1), "voice 2 hasn't started");
        assert_eq!(a.grid_at(1.0), (2, 1));
        a.release(2, 2.0);
        assert_eq!(a.grid_at(2.0), (1, 1), "voice 2 is over");
    }

    #[test]
    fn grid_timeline_matches_live_spans() {
        let spans = [(0u8, 0.0, 4.0), (1, 1.0, 2.0), (2, 1.5, 3.0), (0, 4.0, 5.0)];
        let tl = grid_timeline(&spans);
        assert_eq!(grid_in(&tl, -1.0), (1, 1));
        assert_eq!(grid_in(&tl, 0.5), (1, 1));
        assert_eq!(grid_in(&tl, 1.2), (2, 1));
        assert_eq!(grid_in(&tl, 1.7), (2, 2));
        // Slot 2 still live: the grid doesn't shrink.
        assert_eq!(grid_in(&tl, 2.5), (2, 2));
        assert_eq!(grid_in(&tl, 3.0), (1, 1));
        // Back-to-back voices in one slot: no gap.
        assert_eq!(grid_in(&tl, 4.0), (1, 1));
        assert!(tl.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1));
        assert_eq!(grid_timeline(&[]), vec![(f64::NEG_INFINITY, (1, 1))]);
    }

    #[test]
    fn keyed_voices_take_back_their_slot() {
        let mut a = TileAssigner::new();
        // Track 3 starts first (slot 0), track 1 joins (slot 1).
        assert_eq!(a.assign_keyed(30, 3, 0.0), 0);
        a.release(30, 1.0);
        assert_eq!(a.assign_keyed(10, 1, 0.5), 1);
        a.release(10, 1.0);
        // Both loop at 1.0: each pass keeps its track's slot, even though
        // track 1's voice comes first and slot 0 is lower.
        assert_eq!(a.assign_keyed(11, 1, 1.0), 1);
        assert_eq!(a.assign_keyed(31, 3, 1.0), 0);
        // A new key takes the lowest free slot.
        assert_eq!(a.assign_keyed(50, 5, 1.0), 2);
        // An overlapping voice of the same key takes over its slot.
        assert_eq!(a.assign_keyed(51, 5, 1.2), 2);
        assert_eq!(a.slot_of(50), None);
        assert_eq!(a.grid_at(1.2), (2, 2));
    }
}
