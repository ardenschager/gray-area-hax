//! Offline renderer: walk the timeline, schedule one grain cloud (or one
//! snippet-grain) per clip, and feed the SAME events to the audio renderer
//! and video compositor. Each clip renders to its own audio buffer and
//! video layer so its AV effect chain applies to both sides, then
//! everything mixes down through the master chain.

use crate::audio::{render_grains_audio, StereoBuffer};
use crate::fx;
use crate::grain::{schedule_grains, GrainEvent};
use crate::music::semitones_to_ratio;
use crate::timeline::{Clip, ClipKind, Project, Source};
use crate::video::{blend_layer, composite_grains_frame, Frame, VisualGrainStyle};

pub struct RenderOutput {
    pub audio: StereoBuffer,
    pub frames: Vec<Frame>,
    pub fps: f32,
    /// All grain events rendered, per (track, clip) — useful for UIs.
    pub events: Vec<Vec<GrainEvent>>,
}

/// Build the single event a snippet clip plays: the source as ONE grain,
/// so all correspondence machinery applies.
fn snippet_event(clip: &Clip, source: &Source, clip_t0: f64, clip_len: f64) -> GrainEvent {
    let g = &clip.grains;
    let source_len = source.duration();
    let mut ratio = semitones_to_ratio(g.pitch);
    if let Some(key) = &clip.key {
        ratio = key.quantize_ratio(source.effective_base_hz(), ratio);
    }
    GrainEvent {
        onset: clip_t0,
        source_pos: (g.position as f64 * source_len).min(source_len),
        duration: clip_len as f32,
        pitch_ratio: ratio,
        gain: g.gain,
        pan: g.pan.clamp(-1.0, 1.0),
        envelope: g.envelope,
        reverse: g.reverse_prob >= 0.5,
        id: g.seed,
    }
}

/// Render `[from_beat, to_beat)` of the project.
pub fn render_project(project: &Project, from_beat: f64, to_beat: f64) -> RenderOutput {
    let t0 = project.beats_to_secs(from_beat);
    let t1 = project.beats_to_secs(to_beat.max(from_beat + 0.001));
    let len = t1 - t0;

    let mut master_audio = StereoBuffer::new(len, project.sample_rate);
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

            let events = match clip.kind {
                ClipKind::Granular => schedule_grains(
                    &clip.grains,
                    clip_t0,
                    clip_len,
                    source_len,
                    source.effective_base_hz(),
                    clip.key.as_ref(),
                ),
                ClipKind::Snippet => vec![snippet_event(clip, source, clip_t0, clip_len)],
            };

            // ---- audio: clip bus -> effects -> master mix ----
            if let Some(audio_src) = &source.audio {
                let filtered;
                let src = if clip.audio_filters.is_empty() {
                    audio_src
                } else {
                    filtered = audio_src.filtered(&clip.audio_filters);
                    &filtered
                };
                if clip.effects.is_empty() {
                    render_grains_audio(src, &events, &mut master_audio, t0);
                } else {
                    // Own full-length bus so delay/reverb tails ring out.
                    let mut clip_bus = StereoBuffer::new(len, project.sample_rate);
                    render_grains_audio(src, &events, &mut clip_bus, t0);
                    fx::apply_audio_chain(&mut clip_bus, &clip.effects);
                    for i in 0..master_audio.len().min(clip_bus.len()) {
                        master_audio.left[i] += clip_bus.left[i];
                        master_audio.right[i] += clip_bus.right[i];
                    }
                }
            }

            // ---- video: clip layer -> effects -> blend onto master ----
            if let Some(video_src) = &source.video {
                let style = match clip.kind {
                    ClipKind::Granular => clip.visual,
                    ClipKind::Snippet => VisualGrainStyle::full_frame(clip.visual.additive),
                };
                let has_fx = !clip.effects.is_empty();
                let mut states = fx::video_chain_states(&clip.effects, project.fps);
                for (fi, frame) in frames.iter_mut().enumerate() {
                    let ft = t0 + fi as f64 / project.fps as f64;
                    let any_active =
                        events.iter().any(|e| ft >= e.onset && ft < e.end());
                    // Without temporal effects, frames with no active grains
                    // can be skipped; with effects the chain must run every
                    // frame so ghosts/persistence keep evolving.
                    if !any_active && !has_fx {
                        continue;
                    }
                    let mut layer = Frame::transparent(project.width, project.height);
                    if any_active {
                        composite_grains_frame(
                            video_src,
                            &events,
                            &style,
                            &clip.link,
                            clip.color_filter.as_ref(),
                            &mut layer,
                            ft,
                        );
                    }
                    fx::apply_video_chain(&mut layer, &clip.effects, &mut states);
                    blend_layer(frame, &layer, style.additive);
                }
            }

            all_events.push(events);
        }
    }

    // ---- master chain ----
    fx::apply_audio_chain(&mut master_audio, &project.master_effects);
    let mut master_states = fx::video_chain_states(&project.master_effects, project.fps);
    for frame in &mut frames {
        fx::apply_video_chain(frame, &project.master_effects, &mut master_states);
    }

    master_audio.soft_clip();
    RenderOutput { audio: master_audio, frames, fps: project.fps, events: all_events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{AvEffect, EffectKind};
    use crate::timeline::Clip;

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

        // Events were produced for all three clips (two granular + snippet).
        assert!(out.events.len() >= 3);
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

    fn snippet_project() -> Project {
        let mut p = Project { bpm: 120.0, width: 96, height: 54, fps: 12.0, ..Default::default() };
        let src = crate::timeline::Source {
            name: "src".into(),
            audio: Some(crate::audio::AudioClip::sine(220.0, 4.0, p.sample_rate)),
            video: Some(crate::video::VideoClip::test_pattern(96, 54, 12.0, 4.0)),
            base_hz: 220.0,
        };
        let sid = p.add_source(src);
        let tid = p.add_track("snip");
        p.tracks[tid].clips.push(Clip::new_snippet(sid, 0.0, 4.0));
        p
    }

    #[test]
    fn snippet_plays_source_straight() {
        let p = snippet_project();
        let out = render_project(&p, 0.0, 4.0);
        // Audio: a 220 Hz sine should come straight through.
        let mono: Vec<f32> =
            out.audio.left.iter().zip(&out.audio.right).map(|(l, r)| l + r).collect();
        let pitch = crate::dsp::detect_pitch(&mono, p.sample_rate).expect("pitch");
        assert!((pitch - 220.0).abs() < 8.0, "snippet pitch {pitch}");
        // Video: mid-frame shows the source video full-frame (bright).
        let mid = &out.frames[out.frames.len() / 2];
        assert!(mid.mean_luma() > 20.0, "snippet fills frame: {}", mid.mean_luma());
    }

    #[test]
    fn snippet_gain_drives_both_loudness_and_opacity() {
        let mut quiet = snippet_project();
        quiet.tracks[0].clips[0].grains.gain = 0.3;
        let mut loud = snippet_project();
        loud.tracks[0].clips[0].grains.gain = 1.0;

        let out_q = render_project(&quiet, 0.0, 4.0);
        let out_l = render_project(&loud, 0.0, 4.0);
        // Audio corresponds...
        assert!(out_l.audio.rms() > out_q.audio.rms() * 2.0);
        // ...and so does the visual, via gain_to_opacity.
        let mq = out_q.frames[out_q.frames.len() / 2].mean_luma();
        let ml = out_l.frames[out_l.frames.len() / 2].mean_luma();
        assert!(ml > mq * 1.8, "loud {ml} vs quiet {mq}");

        // Unlink gain from opacity: visual brightness stops following gain.
        let mut unlinked = snippet_project();
        unlinked.tracks[0].clips[0].grains.gain = 0.3;
        unlinked.tracks[0].clips[0].link.gain_to_opacity = 0.0;
        let out_u = render_project(&unlinked, 0.0, 4.0);
        let mu = out_u.frames[out_u.frames.len() / 2].mean_luma();
        assert!(
            (mu - ml).abs() < ml * 0.2,
            "unlinked quiet clip should look as bright as loud one: {mu} vs {ml}"
        );
        // But audio still follows gain.
        assert!(out_u.audio.rms() < out_l.audio.rms() * 0.6);
    }

    #[test]
    fn clip_effect_chain_applies_to_both_domains() {
        let mut p = snippet_project();
        p.tracks[0].clips[0].effects.push(AvEffect::new(EffectKind::Crush {
            downsample: 10.0,
            bits: 3.0,
        }));
        let out = render_project(&p, 0.0, 4.0);
        let plain = render_project(&snippet_project(), 0.0, 4.0);

        // Audio: zero-order hold shows up as repeated samples.
        let mid = out.audio.len() / 2;
        assert_eq!(out.audio.left[mid], out.audio.left[mid + 1]);
        assert_ne!(plain.audio.left[mid], plain.audio.left[mid + 1]);

        // Video: pixels equal within crush blocks.
        let f = &out.frames[out.frames.len() / 2];
        assert_eq!(f.get(0, 0), f.get(5, 5));
        let pf = &plain.frames[plain.frames.len() / 2];
        assert_ne!(pf.get(0, 0), pf.get(5, 5));
    }

    #[test]
    fn delay_effect_rings_past_clip_end_in_both_domains() {
        let mut p = snippet_project();
        // Short 1-beat snippet with a big delay; render 4 beats.
        p.tracks[0].clips[0].length_beats = 1.0;
        p.tracks[0].clips[0].effects.push(AvEffect::new(EffectKind::Delay {
            time: 0.75,
            feedback: 0.6,
            mix: 0.9,
            shift_x: 0.05,
            shift_y: 0.0,
        }));
        let out = render_project(&p, 0.0, 4.0);
        let sr = p.sample_rate as usize;
        // Clip ends at 0.5s (120 bpm); first echo spans [0.75, 1.25].
        let seg = |a: usize, b: usize| -> f32 {
            (out.audio.left[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        };
        let gap = seg((0.55 * sr as f64) as usize, (0.7 * sr as f64) as usize);
        let echo = seg((0.9 * sr as f64) as usize, (1.2 * sr as f64) as usize);
        assert!(echo > gap * 2.0 + 1e-4, "audio echo {echo} vs gap {gap}");

        // Visual ghost appears after the clip's video is gone.
        let f_at = |t: f64| &out.frames[(t * p.fps as f64) as usize];
        let gap_luma = f_at(0.65).mean_luma();
        let echo_luma = f_at(1.0).mean_luma();
        assert!(
            echo_luma > gap_luma + 2.0,
            "visual echo {echo_luma} vs gap {gap_luma}"
        );
    }

    #[test]
    fn master_chain_smears_everything() {
        let mut p = snippet_project();
        p.tracks[0].clips[0].length_beats = 1.0; // ends at 0.5s
        p.master_effects.push(AvEffect::new(EffectKind::Reverb {
            size: 0.9,
            damp: 0.4,
            mix: 0.8,
        }));
        let out = render_project(&p, 0.0, 4.0);
        let sr = p.sample_rate as usize;
        let tail: f32 = out.audio.left[sr..sr + sr / 2].iter().map(|s| s * s).sum();
        assert!(tail > 1e-7, "master reverb tail {tail}");
        // Frames after the snippet still glow from persistence.
        let after = &out.frames[(0.8 * p.fps as f64) as usize];
        assert!(after.mean_luma() > 1.0, "visual smear {}", after.mean_luma());
    }
}
