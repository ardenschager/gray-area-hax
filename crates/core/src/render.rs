//! Offline renderer: walk the timeline, schedule events per clip (grain
//! clouds, snippets, or step-pattern hits), and feed the SAME events to
//! the audio renderer and video compositor.
//!
//! Signal flow, both domains:
//!   clip (events -> clip fx chain)
//!     -> track (stack clips -> track fx chain -> level / opacity)
//!       -> master (sum / composite in track order -> master fx chain)
//!
//! Track order is the video z-order: later tracks composite on top.

use crate::audio::{render_grains_audio, StereoBuffer};
use crate::fx;
use crate::grain::{schedule_grains_auto, GrainEvent};
use crate::music::semitones_to_ratio;
use crate::seq::pattern_events;
use crate::timeline::{Clip, ClipKind, Project, Source};
use crate::video::{
    blend_layer, blend_layer_over, composite_grains_frame, Frame, VisualGrainStyle,
};

pub struct RenderOutput {
    pub audio: StereoBuffer,
    pub frames: Vec<Frame>,
    pub fps: f32,
    /// All grain events rendered, per clip (flattened across sources).
    pub events: Vec<Vec<GrainEvent>>,
}

/// The scheduled events for one clip: (source, events) groups, since a
/// pattern clip's rows can pull from different sources.
pub struct ClipPlan {
    pub track: usize,
    pub clip_index: usize,
    pub parts: Vec<(usize, Vec<GrainEvent>)>,
    pub style: VisualGrainStyle,
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

/// Schedule every clip in the project (absolute timeline seconds). Shared
/// by the offline renderer, the realtime engine, and live previews.
pub fn plan_project(project: &Project) -> Vec<ClipPlan> {
    let mut plans = Vec::new();
    let secs_per_beat = 60.0 / project.bpm;
    for (ti, track) in project.tracks.iter().enumerate() {
        for (ci, clip) in track.clips.iter().enumerate() {
            let clip_t0 = project.beats_to_secs(clip.start_beat);
            let clip_len = project.beats_to_secs(clip.length_beats);
            let (parts, style) = match clip.kind {
                ClipKind::Granular => {
                    let Some(source) = project.sources.get(clip.source) else { continue };
                    if source.duration() <= 0.0 {
                        continue;
                    }
                    let events = schedule_grains_auto(
                        &clip.grains,
                        &clip.automation,
                        secs_per_beat,
                        clip_t0,
                        clip_len,
                        source.duration(),
                        source.effective_base_hz(),
                        clip.key.as_ref(),
                    );
                    (vec![(clip.source, events)], clip.visual)
                }
                ClipKind::Snippet => {
                    let Some(source) = project.sources.get(clip.source) else { continue };
                    if source.duration() <= 0.0 {
                        continue;
                    }
                    let ev = snippet_event(clip, source, clip_t0, clip_len);
                    (
                        vec![(clip.source, vec![ev])],
                        VisualGrainStyle::full_frame(clip.visual.additive),
                    )
                }
                ClipKind::Pattern(pid) => {
                    let Some(pattern) = project.patterns.get(pid) else { continue };
                    let parts = pattern_events(
                        pattern,
                        &project.sources,
                        clip_t0,
                        clip_len,
                        secs_per_beat,
                        clip.key.as_ref(),
                        &clip.automation,
                    );
                    (parts, clip.visual)
                }
            };
            plans.push(ClipPlan { track: ti, clip_index: ci, parts, style });
        }
    }
    plans
}

fn render_audio_part(
    project: &Project,
    clip: &Clip,
    source_id: usize,
    events: &[GrainEvent],
    bus: &mut StereoBuffer,
    t0: f64,
) {
    let Some(source) = project.sources.get(source_id) else { return };
    let Some(audio_src) = &source.audio else { return };
    if clip.audio_filters.is_empty() {
        render_grains_audio(audio_src, events, bus, t0);
    } else {
        let filtered = audio_src.filtered(&clip.audio_filters);
        render_grains_audio(&filtered, events, bus, t0);
    }
}

/// Render `[from_beat, to_beat)` of the project.
pub fn render_project(project: &Project, from_beat: f64, to_beat: f64) -> RenderOutput {
    let t0 = project.beats_to_secs(from_beat);
    let t1 = project.beats_to_secs(to_beat.max(from_beat + 0.001));
    let len = t1 - t0;

    let plan = plan_project(project);
    let mut master_audio = StereoBuffer::new(len, project.sample_rate);
    let n_frames = (len * project.fps as f64).ceil() as usize;
    let mut frames: Vec<Frame> =
        (0..n_frames).map(|_| Frame::black(project.width, project.height)).collect();

    for (ti, track) in project.tracks.iter().enumerate() {
        if track.muted {
            continue;
        }
        let clip_plans: Vec<&ClipPlan> = plan.iter().filter(|p| p.track == ti).collect();
        if clip_plans.is_empty() {
            continue;
        }

        // ---- audio: clips -> clip fx -> track bus -> track fx -> level ----
        let mut track_bus = StereoBuffer::new(len, project.sample_rate);
        for cp in &clip_plans {
            let clip = &track.clips[cp.clip_index];
            if clip.effects.is_empty() {
                for (sid, evs) in &cp.parts {
                    render_audio_part(project, clip, *sid, evs, &mut track_bus, t0);
                }
            } else {
                let mut clip_bus = StereoBuffer::new(len, project.sample_rate);
                for (sid, evs) in &cp.parts {
                    render_audio_part(project, clip, *sid, evs, &mut clip_bus, t0);
                }
                fx::apply_audio_chain(&mut clip_bus, &clip.effects);
                for i in 0..track_bus.len().min(clip_bus.len()) {
                    track_bus.left[i] += clip_bus.left[i];
                    track_bus.right[i] += clip_bus.right[i];
                }
            }
        }
        fx::apply_audio_chain(&mut track_bus, &track.effects);
        let sr = project.sample_rate as f64;
        let beats_per_sec = project.bpm / 60.0;
        for i in 0..master_audio.len().min(track_bus.len()) {
            let beat = (t0 + i as f64 / sr) * beats_per_sec;
            let level = track.level_at(beat);
            master_audio.left[i] += track_bus.left[i] * level;
            master_audio.right[i] += track_bus.right[i] * level;
        }

        // ---- video: clips -> clip fx -> track layer -> track fx -> z ----
        let track_has_video = clip_plans.iter().any(|cp| {
            cp.parts.iter().any(|(sid, evs)| {
                !evs.is_empty()
                    && project.sources.get(*sid).is_some_and(|s| s.video.is_some())
            })
        });
        let track_has_fx = !track.effects.is_empty();
        if !track_has_video && !track_has_fx {
            continue;
        }
        let mut clip_vstates: Vec<Vec<fx::VideoFxState>> = clip_plans
            .iter()
            .map(|cp| fx::video_chain_states(&track.clips[cp.clip_index].effects, project.fps))
            .collect();
        let mut track_vstates = fx::video_chain_states(&track.effects, project.fps);

        for (fi, frame) in frames.iter_mut().enumerate() {
            let ft = t0 + fi as f64 / project.fps as f64;
            let mut track_layer: Option<Frame> = None;

            for (cpi, cp) in clip_plans.iter().enumerate() {
                let clip = &track.clips[cp.clip_index];
                let any_active = cp
                    .parts
                    .iter()
                    .any(|(_, evs)| evs.iter().any(|e| ft >= e.onset && ft < e.end()));
                if !any_active && clip.effects.is_empty() {
                    continue;
                }
                let mut clip_layer = Frame::transparent(project.width, project.height);
                if any_active {
                    for (sid, evs) in &cp.parts {
                        if let Some(video) =
                            project.sources.get(*sid).and_then(|s| s.video.as_ref())
                        {
                            composite_grains_frame(
                                video,
                                evs,
                                &cp.style,
                                &clip.link,
                                clip.color_filter.as_ref(),
                                &mut clip_layer,
                                ft,
                            );
                        }
                    }
                }
                fx::apply_video_chain(&mut clip_layer, &clip.effects, &mut clip_vstates[cpi]);
                let tl = track_layer
                    .get_or_insert_with(|| Frame::transparent(project.width, project.height));
                blend_layer_over(tl, &clip_layer, cp.style.additive);
            }

            if track_layer.is_none() && !track_has_fx {
                continue;
            }
            let mut tl = track_layer
                .unwrap_or_else(|| Frame::transparent(project.width, project.height));
            fx::apply_video_chain(&mut tl, &track.effects, &mut track_vstates);
            let opacity = track.opacity_at((t0 + fi as f64 / project.fps as f64)
                * project.bpm / 60.0);
            blend_layer(frame, &tl, 0.0, opacity);
        }
    }

    // ---- master chain ----
    fx::apply_audio_chain(&mut master_audio, &project.master_effects);
    let mut master_states = fx::video_chain_states(&project.master_effects, project.fps);
    for frame in &mut frames {
        fx::apply_video_chain(frame, &project.master_effects, &mut master_states);
    }

    master_audio.soft_clip();
    let events = plan.into_iter().map(|p| p.parts.into_iter().flat_map(|(_, e)| e).collect()).collect();
    RenderOutput { audio: master_audio, frames, fps: project.fps, events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{AvEffect, EffectKind};
    use crate::timeline::Clip;
    use crate::video::VideoClip;

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

        // Events for all four clips (two granular + snippet + pattern).
        assert!(out.events.len() >= 4);
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
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(220.0, 4.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(96, 54, 12.0, 4.0))),
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
    fn track_effect_chain_applies_to_whole_track() {
        // The clip has NO effects; the TRACK has the delay. Echoes must
        // still ring past the clip end in both domains.
        let mut p = snippet_project();
        p.tracks[0].clips[0].length_beats = 1.0;
        p.tracks[0].effects.push(AvEffect::new(EffectKind::Delay {
            time: 0.75,
            feedback: 0.6,
            mix: 0.9,
            shift_x: 0.05,
            shift_y: 0.0,
        }));
        let out = render_project(&p, 0.0, 4.0);
        let sr = p.sample_rate as usize;
        let seg = |a: usize, b: usize| -> f32 {
            (out.audio.left[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        };
        let gap = seg((0.55 * sr as f64) as usize, (0.7 * sr as f64) as usize);
        let echo = seg((0.9 * sr as f64) as usize, (1.2 * sr as f64) as usize);
        assert!(echo > gap * 2.0 + 1e-4, "track audio echo {echo} vs gap {gap}");

        let f_at = |t: f64| &out.frames[(t * p.fps as f64) as usize];
        assert!(
            f_at(1.0).mean_luma() > f_at(0.65).mean_luma() + 2.0,
            "track visual echo"
        );
    }

    fn solid_video(r: u8, g: u8, b: u8) -> VideoClip {
        let mut f = Frame::black(32, 24);
        for px in f.data.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
        VideoClip { frames: vec![f; 12], fps: 12.0 }
    }

    fn two_track_project(order_red_first: bool) -> Project {
        let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 12.0, ..Default::default() };
        let red = p.add_source(crate::timeline::Source {
            name: "red".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(220.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(220, 30, 30))),
            base_hz: 220.0,
        });
        let blue = p.add_source(crate::timeline::Source {
            name: "blue".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(330.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(30, 30, 220))),
            base_hz: 330.0,
        });
        let order = if order_red_first { [red, blue] } else { [blue, red] };
        for sid in order {
            let t = p.add_track("t");
            let mut c = Clip::new_snippet(sid, 0.0, 2.0);
            c.grains.envelope = 0.01;
            p.tracks[t].clips.push(c);
        }
        p
    }

    #[test]
    fn track_order_is_video_z_order() {
        // Later track on top: with [red, blue], blue wins the pixel.
        let out1 = render_project(&two_track_project(true), 0.0, 2.0);
        let mid1 = &out1.frames[out1.frames.len() / 2];
        let px1 = mid1.get(16, 12);
        assert!(px1[2] > px1[0] + 50, "blue on top: {px1:?}");

        let out2 = render_project(&two_track_project(false), 0.0, 2.0);
        let mid2 = &out2.frames[out2.frames.len() / 2];
        let px2 = mid2.get(16, 12);
        assert!(px2[0] > px2[2] + 50, "red on top after reorder: {px2:?}");
    }

    #[test]
    fn track_level_drives_gain_and_opacity() {
        let mut full = snippet_project();
        full.tracks[0].level = 1.0;
        let mut dim = snippet_project();
        dim.tracks[0].level = 0.35;

        let out_f = render_project(&full, 0.0, 4.0);
        let out_d = render_project(&dim, 0.0, 4.0);
        assert!(out_d.audio.rms() < out_f.audio.rms() * 0.55, "level ducks audio");
        let lf = out_f.frames[out_f.frames.len() / 2].mean_luma();
        let ld = out_d.frames[out_d.frames.len() / 2].mean_luma();
        assert!(ld < lf * 0.6, "level ducks video too: {ld} vs {lf}");

        // Unlink: opacity stops following the fader.
        let mut unlinked = snippet_project();
        unlinked.tracks[0].level = 0.35;
        unlinked.tracks[0].level_to_opacity = 0.0;
        let out_u = render_project(&unlinked, 0.0, 4.0);
        let lu = out_u.frames[out_u.frames.len() / 2].mean_luma();
        assert!((lu - lf).abs() < lf * 0.15, "unlinked stays bright: {lu} vs {lf}");
        assert!(out_u.audio.rms() < out_f.audio.rms() * 0.55, "audio still ducked");
    }

    #[test]
    fn grain_automation_morphs_the_cloud() {
        // Density ramps 4 -> 60 over the clip: far more grains land in the
        // second half. Gain rides the opposite way, so audio fades even as
        // grains multiply.
        let mut p = snippet_project();
        let clip = &mut p.tracks[0].clips[0];
        clip.kind = crate::timeline::ClipKind::Granular;
        clip.grains.density = 10.0;
        let mut dens = crate::auto::AutomationLane::new("density");
        dens.set_point(0.0, 4.0);
        dens.set_point(8.0, 60.0);
        let mut gain = crate::auto::AutomationLane::new("gain");
        gain.set_point(0.0, 1.2);
        gain.set_point(8.0, 0.05);
        clip.automation = vec![dens, gain];
        clip.length_beats = 8.0;

        let out = render_project(&p, 0.0, 8.0);
        let ev = &out.events[0];
        let mid = 2.0; // seconds (8 beats at 120 bpm = 4s)
        let first: Vec<_> = ev.iter().filter(|e| e.onset < mid).collect();
        let second: Vec<_> = ev.iter().filter(|e| e.onset >= mid).collect();
        // A linear 4->60 ramp puts ~2.7x the grains in the second half.
        assert!(
            second.len() > first.len() * 2,
            "density ramp: {} then {}",
            first.len(),
            second.len()
        );
        let avg = |v: &Vec<&crate::grain::GrainEvent>| -> f32 {
            v.iter().map(|e| e.gain).sum::<f32>() / v.len().max(1) as f32
        };
        assert!(
            avg(&first) > avg(&second) * 2.0,
            "gain fade: {} -> {}",
            avg(&first),
            avg(&second)
        );
    }

    #[test]
    fn track_level_automation_fades_audio_and_video() {
        let mut p = snippet_project();
        p.tracks[0].level_points = vec![
            crate::auto::AutoPoint { beat: 0.0, value: 1.0 },
            crate::auto::AutoPoint { beat: 8.0, value: 0.0 },
        ];
        p.tracks[0].clips[0].length_beats = 8.0;
        let out = render_project(&p, 0.0, 8.0);
        let sr = p.sample_rate as usize;
        let seg = |a: usize, b: usize| -> f32 {
            (out.audio.left[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        };
        // 8 beats at 120bpm = 4s; early loud, late quiet.
        let early = seg(sr / 2, sr);
        let late = seg(3 * sr, 3 * sr + sr / 2);
        assert!(late < early * 0.4, "audio fades: {early} -> {late}");
        // Video opacity follows the same curve.
        let f_early = out.frames[(0.75 * p.fps as f64) as usize].mean_luma();
        let f_late = out.frames[(3.25 * p.fps as f64) as usize].mean_luma();
        assert!(f_late < f_early * 0.5, "video fades: {f_early} -> {f_late}");
    }

    #[test]
    fn pattern_clip_renders_beat_quantized_hits() {
        let mut p = Project { bpm: 120.0, width: 64, height: 36, fps: 12.0, ..Default::default() };
        let sid = p.add_source(crate::timeline::Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(440.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(64, 36, 12.0, 2.0))),
            base_hz: 440.0,
        });
        let mut pat = crate::seq::StepPattern::new("p");
        let r = pat.add_row(sid);
        pat.rows[r].grains.duration = 0.15;
        for i in (0..16).step_by(4) {
            pat.rows[r].steps[i].on = true;
        }
        let pid = p.add_pattern(pat);
        let t = p.add_track("seq");
        p.tracks[t].clips.push(Clip::new_pattern(pid, 0.0, 8.0));

        let out = render_project(&p, 0.0, 8.0);
        let sr = p.sample_rate as usize;
        // Hits at 0, 0.5, 1.0 ... 3.5s (looped); gaps between.
        let seg = |a: f64, b: f64| -> f32 {
            let (a, b) = ((a * sr as f64) as usize, (b * sr as f64) as usize);
            (out.audio.left[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        };
        for k in 0..4 {
            let on = seg(k as f64 * 0.5 + 0.01, k as f64 * 0.5 + 0.12);
            let off = seg(k as f64 * 0.5 + 0.3, k as f64 * 0.5 + 0.45);
            assert!(on > off * 3.0 + 1e-4, "hit {k}: on {on} off {off}");
        }
        // Video hits flash at the same quantized moments. Frame 7 is
        // t=0.583s — mid-envelope of the hit at 0.5s; frame 4 (t=0.33s)
        // is in the gap.
        let f_on = out.frames[7].mean_luma();
        let f_off = out.frames[4].mean_luma();
        assert!(f_on > f_off + 1.0, "visual hit {f_on} vs gap {f_off}");
    }
}
