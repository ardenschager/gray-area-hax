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

/// Compositing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
    /// Layered compositing (default).
    #[default]
    Stack,
    /// One tile per sounding voice.
    Tile,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Stack => "stack",
            Layout::Tile => "tile",
        }
    }

    pub fn parse(s: &str) -> Option<Layout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stack" => Some(Layout::Stack),
            "tile" | "tiles" | "grid" => Some(Layout::Tile),
            _ => None,
        }
    }
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
    /// Voice in each slot, and when it ends (`f64::INFINITY` while held).
    slots: [Option<(u64, f64)>; MAX_TILES],
}

impl TileAssigner {
    pub fn new() -> TileAssigner {
        TileAssigner::default()
    }

    /// Free the slots of voices that ended at or before `t`.
    pub fn expire(&mut self, t: f64) {
        for s in self.slots.iter_mut() {
            if s.is_some_and(|(_, end)| end <= t) {
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
                self.slots[i] = Some((voice, f64::INFINITY));
                i as u8
            }
            None => (voice % MAX_TILES as u64) as u8,
        }
    }

    /// The voice ends (fully silent) at `t`: its slot frees from then on.
    pub fn release(&mut self, voice: u64, end: f64) {
        if let Some(i) = self.slot_of(voice) {
            if let Some(s) = self.slots[i].as_mut() {
                s.1 = end;
            }
        }
    }

    pub fn slot_of(&self, voice: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.is_some_and(|(v, _)| v == voice))
    }

    /// Grid covering every slot in use at `t` (for pan at voice start).
    pub fn grid_at(&self, t: f64) -> (u32, u32) {
        frame_grid(self.slots.iter().enumerate().filter(|(_, s)| s.is_some_and(|(_, end)| end > t)).map(|(i, _)| i as u8))
    }

    pub fn clear(&mut self) {
        self.slots = [None; MAX_TILES];
    }
}
