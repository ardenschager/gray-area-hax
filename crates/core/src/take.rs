//! Takes: bounce what was played live (a list of grain events) into a new
//! source — sound AND picture — so a performance becomes media you can
//! place, granulate and sequence like any clip.

use crate::audio::{render_grains_audio, AudioClip, StereoBuffer};
use crate::grain::GrainEvent;
use crate::timeline::Source;
use crate::video::{blend_layer, composite_grains_frame, AvLink, Frame, VideoClip, VisualGrainStyle};
use std::sync::Arc;

/// Render `events` (onsets in seconds from the take start, each tagged
/// with its source id) into a `len`-second source at the given canvas
/// size, frame rate and sample rate. Every grain uses `style` and full
/// correspondence.
#[allow(clippy::too_many_arguments)]
pub fn bounce(
    name: &str,
    sources: &[Source],
    events: &[(usize, GrainEvent)],
    len: f64,
    width: u32,
    height: u32,
    fps: f32,
    sample_rate: u32,
    style: &VisualGrainStyle,
) -> Source {
    let len = len.max(0.05);
    let mut audio = StereoBuffer::new(len, sample_rate);
    for (sid, ev) in events {
        if let Some(a) = sources.get(*sid).and_then(|s| s.audio.as_deref()) {
            render_grains_audio(a, std::slice::from_ref(ev), &mut audio, 0.0);
        }
    }
    audio.soft_clip();
    let n_frames = (len * fps as f64).ceil().max(1.0) as usize;
    let link = AvLink::default();
    let frames = (0..n_frames)
        .map(|fi| {
            let t = fi as f64 / fps as f64;
            let mut layer = Frame::transparent(width, height);
            for (sid, ev) in events {
                if t < ev.onset || t >= ev.end() {
                    continue;
                }
                let Some(v) = sources.get(*sid).and_then(|s| s.video.as_deref()) else { continue };
                composite_grains_frame(
                    v,
                    std::slice::from_ref(ev),
                    style,
                    &link,
                    &crate::timeline::ClipTransform::default(),
                    None,
                    &mut layer,
                    t,
                );
            }
            let mut out = Frame::black(width, height);
            blend_layer(&mut out, &layer, style.additive, 1.0);
            out
        })
        .collect();
    let base_hz = events
        .first()
        .and_then(|(sid, _)| sources.get(*sid))
        .map(|s| s.base_hz)
        .unwrap_or(0.0);
    Source {
        name: name.into(),
        audio: Some(Arc::new(AudioClip::stereo(&audio.left, &audio.right, sample_rate))),
        video: Some(Arc::new(VideoClip { frames, fps })),
        base_hz,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_sounds_and_shows_where_it_was_played() {
        let src = Source {
            name: "s".into(),
            audio: Some(Arc::new(AudioClip::sine(220.0, 2.0, 48000))),
            video: Some(Arc::new(VideoClip::test_pattern(32, 18, 12.0, 2.0))),
            base_hz: 220.0,
            ..Default::default()
        };
        let hit = |onset: f64| GrainEvent {
            onset,
            source_pos: 0.5,
            duration: 0.3,
            pitch_ratio: 1.0,
            gain: 0.8,
            pan: 0.0,
            envelope: 0.5,
            env_skew: 0.0,
            reverse: false,
            id: onset as u64,
            ..Default::default()
        };
        let take = bounce(
            "take 1",
            &[src],
            &[(0, hit(0.1)), (0, hit(1.0))],
            1.5,
            32,
            18,
            12.0,
            48000,
            &VisualGrainStyle::full_frame(0.0),
        );
        assert!((take.duration() - 1.5).abs() < 0.1);
        let a = take.audio.as_ref().unwrap();
        let (l, _) = a.left_right();
        let rms = |r: std::ops::Range<usize>| (l[r.clone()].iter().map(|x| x * x).sum::<f32>() / r.len() as f32).sqrt();
        assert!(rms(9600..14400) > 0.05, "first hit sounds");
        assert!(rms(28800..43200) < 1e-4, "silence between hits");
        let v = take.video.as_ref().unwrap();
        assert!(v.frame_at(0.2).unwrap().mean_luma() > 5.0, "picture during a hit");
        assert!(v.frame_at(0.6).unwrap().mean_luma() < 1.0, "black between hits");
    }
}
