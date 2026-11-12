//! ADSR: one envelope shapes a voice's sound AND its picture.
//!
//! [`Adsr::level`] is the gain envelope. The picture follows it through
//! [`PictureCut`]: `Cut` (default) is full opacity while the level is above
//! a small threshold, then a hard cut, with no fade, so a short release
//! cuts sound and picture together. `Fade` sets opacity to the level.
//!
//! A voice's envelope travels on every grain it makes as a [`VoiceEnv`]
//! (absolute note-on/off times) in `GrainEvent::tag`. The audio renderer
//! multiplies each output sample by the level at that sample's time and the
//! compositor multiplies the grain's opacity by the picture weight, so
//! offline renders and realtime playback agree sample for sample.

use serde::{Deserialize, Serialize};

/// How the picture follows the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PictureCut {
    /// Full opacity above `Adsr::cut_threshold`, then a hard cut (default).
    #[default]
    Cut,
    /// Opacity = level.
    Fade,
}

/// Attack / decay / release in seconds, sustain 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    #[serde(default)]
    pub picture: PictureCut,
    /// Level below which `Cut` hides the picture.
    #[serde(default = "default_threshold")]
    pub cut_threshold: f32,
}

fn default_threshold() -> f32 {
    0.05
}

impl Default for Adsr {
    /// Short attack, full sustain, short release, picture `Cut`.
    fn default() -> Self {
        Adsr { attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.03, picture: PictureCut::Cut, cut_threshold: 0.05 }
    }
}

impl Adsr {
    /// Gain at `since_on` seconds after note-on; `since_off` = seconds
    /// since note-off while releasing (None while held). The release
    /// starts from wherever the envelope was at note-off.
    pub fn level(&self, since_on: f64, since_off: Option<f64>) -> f32 {
        match since_off {
            None => self.held_level(since_on),
            Some(off) => {
                let off = off.max(0.0);
                let at_off = self.held_level((since_on - off).max(0.0));
                let r = self.release.max(0.0) as f64;
                if r <= 0.0 || off >= r {
                    0.0
                } else {
                    at_off * (1.0 - off / r) as f32
                }
            }
        }
    }

    fn held_level(&self, t: f64) -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        let (a, d) = (self.attack.max(0.0) as f64, self.decay.max(0.0) as f64);
        let s = self.sustain.clamp(0.0, 1.0);
        if t < a {
            return (t / a) as f32;
        }
        let t = t - a;
        if t < d {
            return 1.0 + (s - 1.0) * (t / d) as f32;
        }
        s
    }

    /// Picture weight (opacity multiplier) for a level.
    pub fn picture_weight(&self, level: f32) -> f32 {
        match self.picture {
            PictureCut::Cut => {
                if level > self.cut_threshold {
                    1.0
                } else {
                    0.0
                }
            }
            PictureCut::Fade => level.clamp(0.0, 1.0),
        }
    }

    /// Seconds the voice still sounds after note-off.
    pub fn release_secs(&self) -> f64 {
        self.release.max(0.0) as f64
    }
}

/// A voice's envelope in absolute time (seconds on the renderer's clock).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceEnv {
    pub adsr: Adsr,
    /// Note-on.
    pub on: f64,
    /// Note-off (`f64::INFINITY` while held).
    pub off: f64,
}

impl VoiceEnv {
    pub fn held(adsr: Adsr, on: f64) -> VoiceEnv {
        VoiceEnv { adsr, on, off: f64::INFINITY }
    }

    pub fn level_at(&self, t: f64) -> f32 {
        let since_off = (t >= self.off).then(|| t - self.off);
        self.adsr.level(t - self.on, since_off)
    }

    pub fn picture_at(&self, t: f64) -> f32 {
        self.adsr.picture_weight(self.level_at(t))
    }

    /// When the voice is silent for good (INFINITY while held).
    pub fn end(&self) -> f64 {
        self.off + self.adsr.release_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adsr() -> Adsr {
        Adsr { attack: 0.1, decay: 0.2, sustain: 0.5, release: 0.4, ..Adsr::default() }
    }

    #[test]
    fn level_points() {
        let a = adsr();
        assert_eq!(a.level(-0.1, None), 0.0);
        assert!((a.level(0.05, None) - 0.5).abs() < 1e-6);
        assert!((a.level(0.1, None) - 1.0).abs() < 1e-6);
        assert!((a.level(0.2, None) - 0.75).abs() < 1e-6);
        assert!((a.level(0.3, None) - 0.5).abs() < 1e-6);
        assert!((a.level(5.0, None) - 0.5).abs() < 1e-6);
        // Release from sustain: linear to 0 over `release`.
        assert!((a.level(5.0, Some(0.0)) - 0.5).abs() < 1e-6);
        assert!((a.level(5.2, Some(0.2)) - 0.25).abs() < 1e-6);
        assert!(a.level(5.4, Some(0.4)).abs() < 1e-6);
        assert_eq!(a.level(9.0, Some(4.0)), 0.0);
    }

    #[test]
    fn release_from_mid_attack_starts_where_the_envelope_was() {
        let a = adsr();
        // Note-off at 0.05 s (level 0.5, mid-attack).
        assert!((a.level(0.05, Some(0.0)) - 0.5).abs() < 1e-6);
        assert!((a.level(0.25, Some(0.2)) - 0.25).abs() < 1e-6);
        // Zero release cuts instantly.
        let z = Adsr { release: 0.0, ..a };
        assert_eq!(z.level(1.0, Some(0.0)), 0.0);
    }

    #[test]
    fn zero_attack_and_decay_are_instant() {
        let a = Adsr { attack: 0.0, decay: 0.0, sustain: 0.7, release: 0.1, ..Adsr::default() };
        assert!((a.level(0.0, None) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn picture_cut_vs_fade() {
        let cut = adsr();
        assert_eq!(cut.picture_weight(0.5), 1.0);
        assert_eq!(cut.picture_weight(0.06), 1.0);
        assert_eq!(cut.picture_weight(0.05), 0.0);
        assert_eq!(cut.picture_weight(0.0), 0.0);
        let fade = Adsr { picture: PictureCut::Fade, ..cut };
        assert!((fade.picture_weight(0.3) - 0.3).abs() < 1e-6);
        assert_eq!(fade.picture_weight(2.0), 1.0);
    }

    #[test]
    fn voice_env_absolute_times_and_end() {
        let mut e = VoiceEnv::held(adsr(), 10.0);
        assert_eq!(e.end(), f64::INFINITY);
        assert_eq!(e.level_at(9.0), 0.0);
        assert!((e.level_at(11.0) - 0.5).abs() < 1e-6);
        assert_eq!(e.picture_at(11.0), 1.0);
        e.off = 12.0;
        assert!((e.end() - 12.4).abs() < 1e-6);
        assert!((e.level_at(12.2) - 0.25).abs() < 1e-6);
        // Cut: the picture goes when the level drops below 0.05 (≈ 12.36 s).
        assert_eq!(e.picture_at(12.3), 1.0);
        assert_eq!(e.picture_at(12.38), 0.0);
        assert_eq!(e.level_at(12.5), 0.0);
    }

    #[test]
    fn serde_defaults_fill_picture_fields() {
        let a: Adsr = serde_json::from_str(r#"{"attack":0.1,"decay":0.2,"sustain":0.5,"release":0.4}"#).unwrap();
        assert_eq!(a.picture, PictureCut::Cut);
        assert_eq!(a.cut_threshold, 0.05);
        let back: Adsr = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, back);
    }
}
