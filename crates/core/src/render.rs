//! Offline renderer: walk the timeline, schedule one grain cloud per clip,
//! and feed the SAME events to the audio renderer and video compositor.

use crate::audio::{render_grains_audio, StereoBuffer};
use crate::grain::{schedule_grains, GrainEvent};
use crate::timeline::Project;
use crate::video::{composite_grains_frame, Frame};

pub struct RenderOutput {
    pub audio: StereoBuffer,
    pub frames: Vec<Frame>,
    pub fps: f32,
    /// All grain events rendered, per (track, clip) — useful for UIs.
    pub events: Vec<Vec<GrainEvent>>,
}

/// Render `[from_beat, to_beat)` of the project.
pub fn render_project(project: &Project, from_beat: f64, to_beat: f64) -> RenderOutput {
    let t0 = project.beats_to_secs(from_beat);
    let t1 = project.beats_to_secs(to_beat.max(from_beat + 0.001));
    let len = t1 - t0;

    let mut audio = StereoBuffer::new(len, project.sample_rate);
    let n_frames = (len * project.fps as f64).ceil() as usize;
    let mut frames: Vec<Frame> =
        (0..n_frames).map(|_| Frame::black(project.width, project.height)).collect();
    let mut all_events = Vec::new();

    for track in &project.tracks {
        if track.muted {
            continue;
        }
        for clip in &track.clips {
            let Some(source) = project.sources.get(clip.source) else { continue };
            let clip_t0 = project.beats_to_secs(clip.start_beat);
            let clip_len = project.beats_to_secs(clip.length_beats);
            // Skip clips fully outside the render window.
            if clip_t0 + clip_len <= t0 || clip_t0 >= t1 {
                continue;
            }
            let source_len = source.duration();
            if source_len <= 0.0 {
                continue;
            }

            let events = schedule_grains(
                &clip.grains,
                clip_t0,
                clip_len,
                source_len,
                source.effective_base_hz(),
                clip.key.as_ref(),
            );

            if let Some(audio_src) = &source.audio {
                let filtered;
                let src = if clip.audio_filters.is_empty() {
                    audio_src
                } else {
                    filtered = audio_src.filtered(&clip.audio_filters);
                    &filtered
                };
                render_grains_audio(src, &events, &mut audio, t0);
            }

            if let Some(video_src) = &source.video {
                for (fi, frame) in frames.iter_mut().enumerate() {
                    let ft = t0 + fi as f64 / project.fps as f64;
                    composite_grains_frame(
                        video_src,
                        &events,
                        &clip.visual,
                        clip.color_filter.as_ref(),
                        frame,
                        ft,
                    );
                }
            }

            all_events.push(events);
        }
    }

    audio.soft_clip();
    RenderOutput { audio, frames, fps: project.fps, events: all_events }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_renders_audio_and_video() {
        let p = Project::demo();
        let out = render_project(&p, 0.0, 8.0);

        assert!(out.audio.rms() > 0.005, "audio rms {}", out.audio.rms());
        assert!(out.audio.peak() <= 1.001, "soft clip holds");
        assert!(!out.frames.is_empty());

        // Somewhere in the middle, visual grains should be lighting pixels.
        let mid = &out.frames[out.frames.len() / 2];
        assert!(mid.mean_luma() > 0.5, "mid frame luma {}", mid.mean_luma());

        // Events were produced for both clips.
        assert!(out.events.len() >= 2);
        assert!(out.events.iter().all(|e| !e.is_empty()));
    }

    #[test]
    fn render_window_excludes_outside_clips() {
        let p = Project::demo();
        // Render a window after everything ends: silence and black.
        let out = render_project(&p, 100.0, 104.0);
        assert!(out.audio.rms() < 1e-6);
        assert!(out.frames.iter().all(|f| f.mean_luma() < 0.5));
    }
}
