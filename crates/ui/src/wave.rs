//! Waveform (or picture-energy) peaks per source for the Chop sheet,
//! computed once per clip and cached by buffer identity.

use chromagrain_core::timeline::Source;
use egui::{Color32, Painter, Rect, Stroke};
use std::collections::HashMap;
use std::sync::Arc;

/// Buckets per clip: enough for a full-width sheet on a 4K screen.
pub const BUCKETS: usize = 2048;

/// Min / max of the mono mix per bucket (-1..1), or picture energy (0..1)
/// when a source has video frames without an audio track.
pub struct Peaks {
    pub lo: Vec<f32>,
    pub hi: Vec<f32>,
    pub secs: f64,
    pub picture_only: bool,
}

impl Peaks {
    pub fn of(src: &Source) -> Option<Peaks> {
        if let Some(a) = src.audio.as_deref().filter(|a| !a.samples.is_empty()) {
            let n = a.samples.len();
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
            let peak = lo.iter().chain(&hi).map(|v| v.abs()).fold(0.0f32, f32::max);
            if peak > 1e-4 {
                let scale = (0.92 / peak).min(14.0);
                for v in &mut lo {
                    *v = (*v * scale).clamp(-1.0, 1.0);
                }
                for v in &mut hi {
                    *v = (*v * scale).clamp(-1.0, 1.0);
                }
            }
            return Some(Peaks { lo, hi, secs: a.duration(), picture_only: false });
        }

        // Silent video clip: compute a crisp picture-energy curve from frame luma + motion.
        let v = src.video.as_deref().filter(|v| !v.frames.is_empty() && v.duration() > 0.0)?;
        let nf = v.frames.len();
        let mut raw = Vec::with_capacity(nf);
        let mut prev_luma = None::<f32>;
        for f in &v.frames {
            let luma = f.mean_luma() / 255.0;
            let diff = prev_luma.map(|pl| (luma - pl).abs() * 3.5).unwrap_or(0.0);
            prev_luma = Some(luma);
            raw.push((luma * 0.65 + diff).clamp(0.08, 1.0));
        }
        let min_e = raw.iter().copied().fold(f32::MAX, f32::min);
        let max_e = raw.iter().copied().fold(f32::MIN, f32::max);
        let span = (max_e - min_e).max(0.08);
        let b = 192;
        let mut hi = vec![0.0f32; b];
        for k in 0..b {
            let pos = k as f32 / (b - 1).max(1) as f32 * (nf - 1) as f32;
            let i0 = (pos.floor() as usize).min(nf - 1);
            let i1 = (i0 + 1).min(nf - 1);
            let frac = pos - i0 as f32;
            let val = raw[i0] * (1.0 - frac) + raw[i1] * frac;
            hi[k] = (0.24 + 0.66 * ((val - min_e) / span)).clamp(0.18, 0.92);
        }
        Some(Peaks {
            lo: vec![0.0; b],
            hi,
            secs: v.duration(),
            picture_only: true,
        })
    }

    /// Draw seconds `t0..t1` of the clip into `rect`.
    pub fn draw(&self, p: &Painter, rect: Rect, t0: f64, t1: f64, col: Color32) {
        let n = self.hi.len();
        if n == 0 || self.secs <= 0.0 || t1 <= t0 {
            return;
        }
        if self.picture_only {
            let bars = (rect.width() / 4.5).floor().max(24.0) as usize;
            let step = rect.width() / bars as f32;
            let bar_w = (step - 1.5).max(1.5);
            let bar_col = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 105);
            for k in 0..bars {
                let a = t0 + (t1 - t0) * k as f64 / bars as f64;
                let idx = (((a / self.secs) * n as f64) as usize).min(n - 1);
                let h = (self.hi[idx] * rect.height() * 0.85).max(2.0);
                let x = rect.left() + k as f32 * step;
                p.rect_filled(
                    Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + bar_w, rect.bottom())),
                    0.0,
                    bar_col,
                );
            }
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
        let ident = src
            .audio
            .as_ref()
            .map(|a| Arc::as_ptr(a) as usize)
            .or_else(|| src.video.as_ref().map(|v| Arc::as_ptr(v) as usize ^ 0x5649_4400))?;
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
