//! Parameter automation: piecewise-linear lanes that change values over
//! time. Clip lanes target grain parameters (evaluated per grain, so
//! clouds morph as they play); track lanes target the level fader
//! (evaluated per sample for audio, per frame for opacity).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Beats (relative to the clip start for clip lanes; absolute timeline
    /// beats for track lanes).
    pub beat: f64,
    pub value: f64,
}

/// A piecewise-linear automation curve for one named parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutomationLane {
    pub param: String,
    /// Sorted by beat.
    pub points: Vec<AutoPoint>,
}

impl AutomationLane {
    pub fn new(param: &str) -> AutomationLane {
        AutomationLane { param: param.to_string(), points: Vec::new() }
    }

    /// Insert or replace a point, keeping the lane sorted.
    pub fn set_point(&mut self, beat: f64, value: f64) {
        match self.points.iter_mut().find(|p| (p.beat - beat).abs() < 1e-9) {
            Some(p) => p.value = value,
            None => {
                self.points.push(AutoPoint { beat, value });
                self.points.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
            }
        }
    }

    /// Linear interpolation; clamps to the end values outside the range.
    /// None when the lane is empty.
    pub fn value_at(&self, beat: f64) -> Option<f64> {
        let pts = &self.points;
        if pts.is_empty() {
            return None;
        }
        if beat <= pts[0].beat {
            return Some(pts[0].value);
        }
        if beat >= pts[pts.len() - 1].beat {
            return Some(pts[pts.len() - 1].value);
        }
        for w in pts.windows(2) {
            if beat >= w[0].beat && beat <= w[1].beat {
                let span = (w[1].beat - w[0].beat).max(1e-12);
                let f = (beat - w[0].beat) / span;
                return Some(w[0].value + (w[1].value - w[0].value) * f);
            }
        }
        Some(pts[pts.len() - 1].value)
    }
}

/// Display/edit range for an automatable grain parameter (also the set of
/// names the UI offers). Returns None for unknown names.
pub fn param_range(name: &str) -> Option<(f32, f32)> {
    Some(match name {
        "density" => (0.5, 120.0),
        "duration" => (0.005, 1.0),
        "duration_jitter" => (0.0, 1.0),
        "position" => (0.0, 1.0),
        "spray" => (0.0, 3.0),
        "scan_speed" => (-2.0, 4.0),
        "pitch" => (-24.0, 24.0),
        "pitch_jitter" => (0.0, 24.0),
        "gain" => (0.0, 2.0),
        "pan" => (-1.0, 1.0),
        "pan_spread" => (0.0, 1.0),
        "envelope" => (0.01, 1.0),
        "reverse_prob" => (0.0, 1.0),
        _ => return None,
    })
}

/// The parameters offered by automation UIs, in menu order.
pub const AUTOMATABLE: [&str; 13] = [
    "density",
    "duration",
    "duration_jitter",
    "position",
    "spray",
    "scan_speed",
    "pitch",
    "pitch_jitter",
    "gain",
    "pan",
    "pan_spread",
    "envelope",
    "reverse_prob",
];

/// Apply every lane's value at `beat` onto a copy of `base`.
pub fn settings_at(
    base: &crate::grain::GrainSettings,
    lanes: &[AutomationLane],
    beat: f64,
) -> crate::grain::GrainSettings {
    let mut eff = base.clone();
    for lane in lanes {
        if let Some(v) = lane.value_at(beat) {
            let _ = eff.set_param(&lane.param, v);
        }
    }
    eff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_interpolates_and_clamps() {
        let mut lane = AutomationLane::new("gain");
        lane.set_point(4.0, 1.0);
        lane.set_point(0.0, 0.0);
        lane.set_point(8.0, 0.5);
        assert_eq!(lane.value_at(-1.0), Some(0.0));
        assert_eq!(lane.value_at(0.0), Some(0.0));
        assert!((lane.value_at(2.0).unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(lane.value_at(4.0), Some(1.0));
        assert!((lane.value_at(6.0).unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(lane.value_at(100.0), Some(0.5));
        // Replacing an existing point.
        lane.set_point(4.0, 2.0);
        assert_eq!(lane.value_at(4.0), Some(2.0));
        assert_eq!(lane.points.len(), 3);
    }

    #[test]
    fn empty_lane_is_none() {
        let lane = AutomationLane::new("gain");
        assert_eq!(lane.value_at(0.0), None);
    }

    #[test]
    fn settings_at_overrides() {
        let base = crate::grain::GrainSettings::default();
        let mut lane = AutomationLane::new("density");
        lane.set_point(0.0, 5.0);
        lane.set_point(4.0, 50.0);
        let eff = settings_at(&base, &[lane], 2.0);
        assert!((eff.density - 27.5).abs() < 1e-4);
        // Unknown params are ignored quietly.
        let bogus = AutomationLane { param: "nope".into(), points: vec![AutoPoint { beat: 0.0, value: 1.0 }] };
        let eff2 = settings_at(&base, &[bogus], 0.0);
        assert_eq!(eff2, base);
    }

    #[test]
    fn ranges_cover_all_automatable() {
        for p in AUTOMATABLE {
            assert!(param_range(p).is_some(), "{p} needs a range");
        }
        assert!(param_range("seed").is_none());
    }
}
