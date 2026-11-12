//! Waveform peaks per source (for the Chop sheet), computed once per clip
//! and cached by the audio buffer's identity.

use chromagrain_core::timeline::Source;
use egui::{Color32, Painter, Rect, Stroke};
use std::collections::HashMap;
use std::sync::Arc;

/// Buckets per clip: enough for a full-width sheet on a 4K screen.
pub const BUCKETS: usize = 2048;

/// Min / max of the mono mix per bucket, -1..1.
pub struct Peaks {
    pub lo: Vec<f32>,
    pub hi: Vec<f32>,
    pub secs: f64,
}

impl Peaks {
    pub fn of(src: &Source) -> Option<Peaks> {
        let a = src.audio.as_deref()?;
        let n = a.samples.len();
        if n == 0 {
            return None;
        }
        let b = BUCKETS.min(n);
        let (mut lo, mut hi) = (vec![0.0f32; b], vec![0.0f32; b]);
        for k in 0..b {
            let (s, e) = (k * n / b, ((k + 1) * n / b).max(k * n / b + 1).min(n));
            let side = a.side.as_deref();
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for i in s..e {
                let v = match side {
                    Some(r) => 0.5 * (a.samples[i] + r.get(i).copied().unwrap_or(a.samples[i])),
                    None => a.samples[i],
                };
                mn = mn.min(v);
                mx = mx.max(v);
            }
            lo[k] = mn.clamp(-1.0, 1.0);
            hi[k] = mx.clamp(-1.0, 1.0);
        }
        Some(Peaks { lo, hi, secs: a.duration() })
    }

    /// Draw seconds `t0..t1` of the clip into `rect` as a centred,
    /// filled envelope.
    pub fn draw(&self, p: &Painter, rect: Rect, t0: f64, t1: f64, col: Color32) {
        let n = self.lo.len();
        if n == 0 || self.secs <= 0.0 || t1 <= t0 {
            return;
        }
        let cols = rect.width().max(1.0) as usize;
        let cy = rect.center().y;
        let half = rect.height() * 0.5;
        for x in 0..cols {
            let a = t0 + (t1 - t0) * x as f64 / cols as f64;
            let b = t0 + (t1 - t0) * (x + 1) as f64 / cols as f64;
            let (ia, ib) = (((a / self.secs) * n as f64) as usize, ((b / self.secs) * n as f64).ceil() as usize);
            let ia = ia.min(n - 1);
            let ib = ib.clamp(ia + 1, n);
            let mn = self.lo[ia..ib].iter().copied().fold(f32::MAX, f32::min);
            let mx = self.hi[ia..ib].iter().copied().fold(f32::MIN, f32::max);
            let px = rect.left() + x as f32 + 0.5;
            let (y0, y1) = (cy - mx * half, cy - mn * half);
            p.line_segment([egui::pos2(px, y0.min(cy - 0.5)), egui::pos2(px, y1.max(cy + 0.5))], Stroke::new(1.0, col));
        }
    }
}

#[derive(Default)]
pub struct WaveCache {
    map: HashMap<usize, (usize, Arc<Peaks>)>,
}

impl WaveCache {
    pub fn get(&mut self, sources: &[Source], id: usize) -> Option<Arc<Peaks>> {
        let src = sources.get(id)?;
        let ident = src.audio.as_ref().map(|a| Arc::as_ptr(a) as usize)?;
        if let Some((k, p)) = self.map.get(&id) {
            if *k == ident {
                return Some(p.clone());
            }
        }
        let p = Arc::new(Peaks::of(src)?);
        self.map.insert(id, (ident, p.clone()));
        Some(p)
    }
}
