//! Realtime audio engine for performance playback: renders the project in
//! consecutive small blocks with persistent effect state, so the audio a
//! cpal callback drains is (sample-exactly) the offline render, streamed.
//! Supports a beat-quantized loop region for live use.

use crate::audio::{render_grains_audio, StereoBuffer};
use crate::fx::{self, AudioFxState};
use crate::render::{plan_project, ClipPlan};
use crate::timeline::Project;

pub struct RealtimeAudio {
    project: Project,
    plan: Vec<ClipPlan>,
    /// Pre-filtered audio per plan part (clip audio_filters applied once).
    part_audio: Vec<Vec<Option<crate::audio::AudioClip>>>,
    clip_states: Vec<Vec<AudioFxState>>,
    track_states: Vec<Vec<AudioFxState>>,
    master_states: Vec<AudioFxState>,
    /// Absolute playhead in samples from beat 0.
    sample_pos: u64,
    /// Loop region in beats (start, end), if looping.
    pub loop_region: Option<(f64, f64)>,
    scratch_clip: StereoBuffer,
    scratch_track: StereoBuffer,
}

impl RealtimeAudio {
    pub fn new(project: Project, start_beat: f64, loop_region: Option<(f64, f64)>) -> Self {
        let plan = plan_project(&project);
        let sr = project.sample_rate;
        let part_audio = plan
            .iter()
            .map(|cp| {
                let clip = &project.tracks[cp.track].clips[cp.clip_index];
                cp.parts
                    .iter()
                    .map(|(sid, _)| {
                        let audio = project.sources.get(*sid).and_then(|s| s.audio.as_ref())?;
                        if clip.audio_filters.is_empty() {
                            None // use the source directly
                        } else {
                            Some(audio.filtered(&clip.audio_filters))
                        }
                    })
                    .collect()
            })
            .collect();
        let clip_states = plan
            .iter()
            .map(|cp| {
                fx::audio_chain_states(
                    &project.tracks[cp.track].clips[cp.clip_index].effects,
                    sr,
                )
            })
            .collect();
        let track_states = project
            .tracks
            .iter()
            .map(|t| fx::audio_chain_states(&t.effects, sr))
            .collect();
        let master_states = fx::audio_chain_states(&project.master_effects, sr);
        let sample_pos = (project.beats_to_secs(start_beat) * sr as f64).round() as u64;
        RealtimeAudio {
            project,
            plan,
            part_audio,
            clip_states,
            track_states,
            master_states,
            sample_pos,
            loop_region,
            scratch_clip: StereoBuffer { left: Vec::new(), right: Vec::new(), sample_rate: sr },
            scratch_track: StereoBuffer { left: Vec::new(), right: Vec::new(), sample_rate: sr },
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.project.sample_rate
    }

    pub fn playhead_secs(&self) -> f64 {
        self.sample_pos as f64 / self.project.sample_rate as f64
    }

    pub fn playhead_beats(&self) -> f64 {
        self.project.secs_to_beats(self.playhead_secs())
    }

    pub fn seek_beats(&mut self, beat: f64) {
        self.sample_pos = (self.project.beats_to_secs(beat.max(0.0))
            * self.project.sample_rate as f64)
            .round() as u64;
    }

    /// Fill one stereo block, advancing (and looping) the playhead.
    pub fn next_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.project.sample_rate as f64;
        let mut filled = 0usize;
        while filled < left.len() {
            let remaining = left.len() - filled;
            let n = match self.loop_region {
                Some((ls, le)) if le > ls => {
                    let end_sample =
                        (self.project.beats_to_secs(le) * sr).round() as u64;
                    if self.sample_pos >= end_sample {
                        self.seek_beats(ls);
                        continue;
                    }
                    remaining.min((end_sample - self.sample_pos) as usize)
                }
                _ => remaining,
            };
            self.render_segment_at(filled, n, left, right);
            filled += n;
            self.sample_pos += n as u64;
        }
    }

    /// Render `n` samples starting at self.sample_pos into out[at..at+n).
    fn render_segment_at(&mut self, at: usize, n: usize, left: &mut [f32], right: &mut [f32]) {
        let sr = self.project.sample_rate as f64;
        let t0 = self.sample_pos as f64 / sr;
        left[at..at + n].fill(0.0);
        right[at..at + n].fill(0.0);

        self.scratch_track.left.resize(n, 0.0);
        self.scratch_track.right.resize(n, 0.0);
        self.scratch_clip.left.resize(n, 0.0);
        self.scratch_clip.right.resize(n, 0.0);

        for (ti, track) in self.project.tracks.iter().enumerate() {
            if track.muted {
                continue;
            }
            self.scratch_track.left.fill(0.0);
            self.scratch_track.right.fill(0.0);
            let mut track_touched = !track.effects.is_empty();

            for (pi, cp) in self.plan.iter().enumerate() {
                if cp.track != ti {
                    continue;
                }
                let clip = &track.clips[cp.clip_index];
                let has_fx = !clip.effects.is_empty();
                let bus = if has_fx {
                    self.scratch_clip.left.fill(0.0);
                    self.scratch_clip.right.fill(0.0);
                    &mut self.scratch_clip
                } else {
                    &mut self.scratch_track
                };
                let mut any = has_fx; // fx chains must run every block
                for (part_i, (sid, evs)) in cp.parts.iter().enumerate() {
                    let audio = match &self.part_audio[pi][part_i] {
                        Some(filtered) => Some(filtered),
                        None => {
                            self.project.sources.get(*sid).and_then(|s| s.audio.as_deref())
                        }
                    };
                    let Some(audio) = audio else { continue };
                    render_grains_audio(audio, evs, bus, t0);
                    any = true;
                }
                if has_fx {
                    fx::process_audio_chain(
                        &mut self.scratch_clip.left,
                        &mut self.scratch_clip.right,
                        &clip.effects,
                        &mut self.clip_states[pi],
                    );
                    for i in 0..n {
                        self.scratch_track.left[i] += self.scratch_clip.left[i];
                        self.scratch_track.right[i] += self.scratch_clip.right[i];
                    }
                }
                track_touched |= any;
            }

            if !track_touched {
                continue;
            }
            fx::process_audio_chain(
                &mut self.scratch_track.left,
                &mut self.scratch_track.right,
                &track.effects,
                &mut self.track_states[ti],
            );
            let level = track.level.max(0.0);
            for i in 0..n {
                left[at + i] += self.scratch_track.left[i] * level;
                right[at + i] += self.scratch_track.right[i] * level;
            }
        }

        fx::process_audio_chain(
            &mut left[at..at + n],
            &mut right[at..at + n],
            &self.project.master_effects,
            &mut self.master_states,
        );
        // Same soft clip as the offline master.
        for s in left[at..at + n].iter_mut().chain(right[at..at + n].iter_mut()) {
            if s.abs() > 1.0 {
                *s = s.tanh();
            } else {
                *s = s.tanh() * 0.2 + *s * 0.8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{AvEffect, EffectKind};
    use crate::render::render_project;
    use crate::timeline::{Clip, Project, Source};
    use crate::video::VideoClip;

    fn perf_project() -> Project {
        let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 12.0, ..Default::default() };
        let sid = p.add_source(Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(110.0, 3.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(32, 24, 12.0, 2.0))),
            base_hz: 110.0,
        });
        // Granular clip with a clip-level delay.
        let t0 = p.add_track("grains");
        let mut c = Clip::new(sid, 0.0, 4.0);
        c.grains.density = 15.0;
        c.effects.push(AvEffect::new(EffectKind::Delay {
            time: 0.21,
            feedback: 0.4,
            mix: 0.5,
            shift_x: 0.0,
            shift_y: 0.0,
        }));
        p.tracks[t0].clips.push(c);
        // Snippet on a track with a track-level reverb + level fader.
        let t1 = p.add_track("bed");
        let mut s = Clip::new_snippet(sid, 1.0, 3.0);
        s.grains.gain = 0.5;
        p.tracks[t1].clips.push(s);
        p.tracks[t1].effects.push(AvEffect::new(EffectKind::Reverb {
            size: 0.6,
            damp: 0.3,
            mix: 0.4,
        }));
        p.tracks[t1].level = 0.8;
        // Pattern track.
        let mut pat = crate::seq::StepPattern::new("hits");
        let r = pat.add_row(sid);
        for i in (0..16).step_by(4) {
            pat.rows[r].steps[i].on = true;
        }
        let pid = p.add_pattern(pat);
        let t2 = p.add_track("seq");
        p.tracks[t2].clips.push(Clip::new_pattern(pid, 0.0, 4.0));
        // Master crush (streams with zero latency, unlike compress).
        p.master_effects.push(AvEffect::new(EffectKind::Crush {
            downsample: 3.0,
            bits: 8.0,
        }));
        p
    }

    #[test]
    fn realtime_stream_matches_offline_render() {
        let p = perf_project();
        let offline = render_project(&p, 0.0, 4.0);
        let total = offline.audio.len();

        let mut rt = RealtimeAudio::new(p, 0.0, None);
        let mut left = vec![0.0f32; total];
        let mut right = vec![0.0f32; total];
        let sizes = [512usize, 128, 2048, 733, 4096];
        let mut pos = 0;
        let mut k = 0;
        while pos < total {
            let b = sizes[k % sizes.len()].min(total - pos);
            let (l, r) = (&mut left[pos..pos + b], &mut right[pos..pos + b]);
            rt.next_block(l, r);
            pos += b;
            k += 1;
        }

        let mut max_diff = 0.0f32;
        for i in 0..total {
            max_diff = max_diff.max((left[i] - offline.audio.left[i]).abs());
            max_diff = max_diff.max((right[i] - offline.audio.right[i]).abs());
        }
        assert!(max_diff < 1e-5, "realtime == offline, max diff {max_diff}");
        let rms: f32 = (left.iter().map(|s| s * s).sum::<f32>() / total as f32).sqrt();
        assert!(rms > 0.01, "and it is not silence: {rms}");
    }

    #[test]
    fn loop_region_wraps_playhead() {
        let p = perf_project();
        let sr = p.sample_rate as usize;
        // Loop beats [0, 2) = 1 second at 120 bpm.
        let mut rt = RealtimeAudio::new(p, 0.0, Some((0.0, 2.0)));
        let mut l = vec![0.0f32; sr * 2 + 137];
        let mut r = vec![0.0f32; sr * 2 + 137];
        rt.next_block(&mut l, &mut r);
        // After 2.x loops the playhead sits inside the loop.
        let beats = rt.playhead_beats();
        assert!(
            (0.0..2.0).contains(&beats),
            "playhead wrapped into loop: {beats}"
        );
        // Audio in the second pass of the loop is present (still playing).
        let seg: f32 = l[sr..sr + 4800].iter().map(|s| s * s).sum();
        assert!(seg > 1e-6, "loop keeps sounding");
    }

    #[test]
    fn seek_moves_playhead() {
        let p = perf_project();
        let mut rt = RealtimeAudio::new(p, 0.0, None);
        rt.seek_beats(3.0);
        assert!((rt.playhead_beats() - 3.0).abs() < 1e-6);
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        rt.next_block(&mut l, &mut r);
        assert!(rt.playhead_beats() > 3.0);
    }
}
