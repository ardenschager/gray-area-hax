//! Realtime audio engine for performance playback: renders the project in
//! consecutive small blocks with persistent effect state, so the audio a
//! cpal callback drains is (sample-exactly) the offline render, streamed.
//! Supports a beat-quantized loop region for live use.

use crate::audio::{
    render_grains_audio, render_grains_audio_spatial, MasterConditioner, StereoBuffer, TapeState,
};
use crate::fx::{self, AudioFxState};
use crate::grain::GrainEvent;
use crate::render::{plan_project, ClipPlan};
use crate::timeline::{DuckState, Project};

pub struct RealtimeAudio {
    project: Project,
    plan: Vec<ClipPlan>,
    /// Pre-filtered audio per plan part (clip audio_filters applied once).
    part_audio: Vec<Vec<Option<crate::audio::AudioClip>>>,
    clip_states: Vec<Vec<AudioFxState>>,
    tilt_states: Vec<crate::dsp::TiltState>,
    track_eq_states: Vec<Vec<[crate::dsp::Biquad; 2]>>,
    track_states: Vec<Vec<AudioFxState>>,
    track_tape_states: Vec<TapeState>,
    duck_states: Vec<DuckState>,
    track_buses: Vec<StereoBuffer>,
    track_active: Vec<bool>,
    master_states: Vec<AudioFxState>,
    master_tape_state: TapeState,
    master_cond: MasterConditioner,
    /// Absolute playhead in samples from beat 0.
    sample_pos: u64,
    /// Loop region in beats (start, end), if looping.
    pub loop_region: Option<(f64, f64)>,
    /// When false, the arrangement is paused: the playhead freezes and
    /// plan events are silent, but live (MIDI) events keep sounding.
    pub transport: bool,
    /// One-shot live events (MIDI pads, emitters) on the monotonic live
    /// clock: (bus, source, event). Bus 0 is dry; bus b > 0 runs through
    /// `live_buses[b - 1]` (e.g. one per emitter: its smear + look).
    live: Vec<(u8, usize, GrainEvent)>,
    live_buses: Vec<(Vec<crate::fx::AvEffect>, Vec<AudioFxState>)>,
    /// Monotonic sample clock that never pauses (times live events).
    live_pos: u64,
    scratch_clip: StereoBuffer,
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
                        if clip.audio_filters.is_empty() && clip.eq.is_empty() {
                            None // use the source directly
                        } else {
                            Some(audio.filtered_and_eq(&clip.audio_filters, &clip.eq))
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
        let track_eq_states = project
            .tracks
            .iter()
            .map(|t| {
                t.eq.iter()
                    .map(|b| {
                        let bq = crate::dsp::Biquad::peaking(sr as f32, b.freq, b.q, b.gain_db);
                        [bq.clone(), bq]
                    })
                    .collect()
            })
            .collect();
        let track_states = project
            .tracks
            .iter()
            .map(|t| fx::audio_chain_states(&t.effects, sr))
            .collect();
        let n_tracks = project.tracks.len();
        let track_tape_states = (0..n_tracks)
            .map(|ti| TapeState::new(0x5452_4B00 ^ ti as u64))
            .collect();
        let duck_states = vec![DuckState::default(); n_tracks];
        let track_buses = (0..n_tracks)
            .map(|_| StereoBuffer { left: Vec::new(), right: Vec::new(), sample_rate: sr })
            .collect();
        let track_active = vec![false; n_tracks];
        let master_states = fx::audio_chain_states(&project.master_effects, sr);
        let master_tape_state = TapeState::new(0x4D41_5354);
        let master_cond = MasterConditioner::default();
        let tilt_states = plan.iter().map(|_| crate::dsp::TiltState::default()).collect();
        let sample_pos = (project.beats_to_secs(start_beat) * sr as f64).round() as u64;
        RealtimeAudio {
            project,
            plan,
            part_audio,
            clip_states,
            tilt_states,
            track_eq_states,
            track_states,
            track_tape_states,
            duck_states,
            track_buses,
            track_active,
            master_states,
            master_tape_state,
            master_cond,
            sample_pos,
            loop_region,
            transport: true,
            live: Vec::new(),
            live_buses: Vec::new(),
            live_pos: 0,
            scratch_clip: StereoBuffer { left: Vec::new(), right: Vec::new(), sample_rate: sr },
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

    /// Seconds on the monotonic live clock ("now" for trigger_live).
    pub fn live_now_secs(&self) -> f64 {
        self.live_pos as f64 / self.project.sample_rate as f64
    }

    /// Fire a one-shot live grain (a MIDI pad hit). The event's `onset`
    /// is on the live clock; pass `live_now_secs()` for "immediately".
    pub fn trigger_live(&mut self, source: usize, event: GrainEvent) {
        self.live.push((0, source, event));
    }

    /// [`RealtimeAudio::trigger_live`] on effect bus `bus` (0 = dry).
    pub fn trigger_live_bus(&mut self, bus: u8, source: usize, event: GrainEvent) {
        self.live.push((bus, source, event));
    }

    /// Set the effect chain of live bus `bus` (1-based). State is kept
    /// while the chain's shape (effect kinds) stays the same, so wet
    /// amounts can be swept without clicks; a new shape rebuilds it.
    pub fn set_live_bus(&mut self, bus: u8, chain: Vec<crate::fx::AvEffect>) {
        if bus == 0 {
            return;
        }
        let i = bus as usize - 1;
        while self.live_buses.len() <= i {
            self.live_buses.push((Vec::new(), Vec::new()));
        }
        let same = self.live_buses[i].0.len() == chain.len()
            && self.live_buses[i].0.iter().zip(&chain).all(|(a, b)| {
                std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
                    && match (a.kind, b.kind) {
                        // Time-structuring params live in the state.
                        (crate::fx::EffectKind::Delay { time: x, .. }, crate::fx::EffectKind::Delay { time: y, .. }) => x == y,
                        (crate::fx::EffectKind::Reverb { size: x, damp: dx, .. }, crate::fx::EffectKind::Reverb { size: y, damp: dy, .. }) => x == y && dx == dy,
                        (crate::fx::EffectKind::Tune { root: x, scale: sx, focus: fx, .. }, crate::fx::EffectKind::Tune { root: y, scale: sy, focus: fy, .. }) => x == y && sx == sy && fx == fy,
                        (crate::fx::EffectKind::Tint { hue: x, amount: ax }, crate::fx::EffectKind::Tint { hue: y, amount: ay }) => x == y && ax == ay,
                        _ => true,
                    }
            });
        if !same {
            self.live_buses[i].1 = fx::audio_chain_states(&chain, self.project.sample_rate);
        }
        self.live_buses[i].0 = chain;
    }

    /// Fill one stereo block, advancing (and looping) the playhead.
    pub fn next_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.transport {
            // Arrangement paused: only the live bus + master chain run.
            let n = left.len();
            left.fill(0.0);
            right.fill(0.0);
            self.render_live_and_master(0, n, left, right);
            return;
        }
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

        self.scratch_clip.left.resize(n, 0.0);
        self.scratch_clip.right.resize(n, 0.0);
        let n_tracks = self.project.tracks.len();
        self.track_active.fill(false);

        for (ti, track) in self.project.tracks.iter().enumerate() {
            if track.muted {
                continue;
            }
            let tbus = &mut self.track_buses[ti];
            tbus.left.resize(n, 0.0);
            tbus.right.resize(n, 0.0);
            tbus.left.fill(0.0);
            tbus.right.fill(0.0);
            let mut track_touched =
                !track.effects.is_empty() || track.tape.is_active() || !track.eq.is_empty();

            for (pi, cp) in self.plan.iter().enumerate() {
                if cp.track != ti {
                    continue;
                }
                let clip = &track.clips[cp.clip_index];
                let (_, _, tilt) = crate::render::clip_spatial(clip);
                let has_fx = !clip.effects.is_empty() || tilt.abs() > 1e-4;
                let bus = if has_fx {
                    self.scratch_clip.left.fill(0.0);
                    self.scratch_clip.right.fill(0.0);
                    &mut self.scratch_clip
                } else {
                    &mut *tbus
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
                    let (pan_offset, gain_mult, _) = crate::render::clip_spatial(clip);
                    render_grains_audio_spatial(audio, evs, bus, t0, pan_offset, gain_mult);
                    any = true;
                }
                if has_fx {
                    fx::process_audio_chain(
                        &mut self.scratch_clip.left,
                        &mut self.scratch_clip.right,
                        &clip.effects,
                        &mut self.clip_states[pi],
                    );
                    crate::dsp::apply_tilt(
                        &mut self.scratch_clip.left,
                        &mut self.scratch_clip.right,
                        &mut self.tilt_states[pi],
                        tilt,
                        self.project.sample_rate,
                    );
                    for i in 0..n {
                        tbus.left[i] += self.scratch_clip.left[i];
                        tbus.right[i] += self.scratch_clip.right[i];
                    }
                }
                track_touched |= any;
            }

            if !track_touched {
                continue;
            }
            for [bql, bqr] in &mut self.track_eq_states[ti] {
                bql.process_buffer(&mut tbus.left);
                bqr.process_buffer(&mut tbus.right);
            }
            fx::process_audio_chain(
                &mut tbus.left,
                &mut tbus.right,
                &track.effects,
                &mut self.track_states[ti],
            );
            if track.tape.is_active() {
                self.track_tape_states[ti].process_block(
                    &mut tbus.left,
                    &mut tbus.right,
                    &track.tape,
                    self.project.sample_rate,
                );
            }
            // Same per-sample level-automation expression as the offline
            // renderer, so streamed output stays sample-exact.
            let beats_per_sec = self.project.bpm / 60.0;
            for i in 0..n {
                let beat = (t0 + i as f64 / sr) * beats_per_sec;
                let level = track.level_at(beat);
                tbus.left[i] *= level;
                tbus.right[i] *= level;
            }
            self.track_active[ti] = true;
        }

        // Second pass: z-independent sidechain ducking + master sum.
        let sr_u32 = self.project.sample_rate;
        for (ti, track) in self.project.tracks.iter().enumerate() {
            if !self.track_active[ti] {
                continue;
            }
            if let Some(spec) = &track.duck {
                if spec.source_track < n_tracks && spec.source_track != ti {
                    let sc_active = self.track_active[spec.source_track];
                    for i in 0..n {
                        let (sl, sr_c) = if sc_active {
                            (
                                self.track_buses[spec.source_track].left[i],
                                self.track_buses[spec.source_track].right[i],
                            )
                        } else {
                            (0.0, 0.0)
                        };
                        let g = self.duck_states[ti].step(sl, sr_c, spec, sr_u32);
                        left[at + i] += self.track_buses[ti].left[i] * g;
                        right[at + i] += self.track_buses[ti].right[i] * g;
                    }
                    continue;
                }
            }
            for i in 0..n {
                left[at + i] += self.track_buses[ti].left[i];
                right[at + i] += self.track_buses[ti].right[i];
            }
        }

        self.render_live_and_master(at, n, left, right);
    }

    /// Mix live (MIDI) events on the monotonic clock, then run the master
    /// chain, master tape, and master conditioner (sample-exact with offline).
    fn render_live_and_master(&mut self, at: usize, n: usize, left: &mut [f32], right: &mut [f32]) {
        let sr = self.project.sample_rate as f64;
        let t_live = self.live_pos as f64 / sr;
        let n_buses = self.live_buses.len() as u8;
        for bus in 0..=n_buses {
            let has_fx = bus > 0 && !self.live_buses[bus as usize - 1].0.is_empty();
            let any = self.live.iter().any(|(b, _, _)| *b == bus);
            if !any && !has_fx {
                continue; // fx buses keep running so their tails ring out
            }
            self.scratch_clip.left.resize(n, 0.0);
            self.scratch_clip.right.resize(n, 0.0);
            self.scratch_clip.left.fill(0.0);
            self.scratch_clip.right.fill(0.0);
            for (b, sid, ev) in &self.live {
                if *b != bus {
                    continue;
                }
                let Some(audio) = self.project.sources.get(*sid).and_then(|s| s.audio.as_deref()) else {
                    continue;
                };
                render_grains_audio(audio, std::slice::from_ref(ev), &mut self.scratch_clip, t_live);
            }
            if has_fx {
                let (chain, states) = &mut self.live_buses[bus as usize - 1];
                fx::process_audio_chain(&mut self.scratch_clip.left, &mut self.scratch_clip.right, chain, states);
            }
            for i in 0..n {
                left[at + i] += self.scratch_clip.left[i];
                right[at + i] += self.scratch_clip.right[i];
            }
        }
        // Prune finished hits.
        let done_before = t_live + n as f64 / sr;
        self.live.retain(|(_, _, ev)| ev.end() > done_before);
        self.live_pos += n as u64;

        fx::process_audio_chain(
            &mut left[at..at + n],
            &mut right[at..at + n],
            &self.project.master_effects,
            &mut self.master_states,
        );
        if self.project.master_tape.is_active() {
            self.master_tape_state.process_block(
                &mut left[at..at + n],
                &mut right[at..at + n],
                &self.project.master_tape,
                self.project.sample_rate,
            );
        }
        self.master_cond.process_block(
            &mut left[at..at + n],
            &mut right[at..at + n],
            self.project.sample_rate,
        );
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
        // Spatial placement: pan right, up-tilt brightness, tint chain.
        s.transform.x = 0.4;
        s.transform.y = -0.5;
        s.effects.push(AvEffect::new(EffectKind::Tint { hue: 60.0, amount: 0.5 }));
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
        // Track-level automation on the bed (fade in), locking realtime
        // parity of the per-sample level curve.
        p.tracks[t1].level_points = vec![
            crate::auto::AutoPoint { beat: 0.0, value: 0.2 },
            crate::auto::AutoPoint { beat: 4.0, value: 1.0 },
        ];
        // Master crush (streams with zero latency, unlike compress).
        p.master_effects.push(AvEffect::new(EffectKind::Crush {
            downsample: 3.0,
            bits: 8.0,
        }));
        // Sidechain ducking (bed ducks from seq, testing reverse z-order) +
        // analog tape/vinyl stages on track and master.
        p.tracks[t1].duck = Some(crate::timeline::DuckSpec {
            source_track: t2,
            amount: 0.6,
            attack: 0.008,
            release: 0.16,
            key_hz: 165.0,
        });
        p.tracks[t1].tape = crate::audio::TapeSettings {
            wow: 0.3,
            warmth: 0.35,
            age: 0.25,
            dust: 0.2,
        };
        p.tracks[t1].eq.push(crate::dsp::EqBand { freq: 320.0, q: 1.2, gain_db: -3.5 });
        p.master_tape = crate::audio::TapeSettings {
            wow: 0.15,
            warmth: 0.2,
            age: 0.1,
            dust: 0.1,
        };
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
    fn live_pads_sound_with_transport_stopped() {
        let p = perf_project();
        let sr = p.sample_rate as usize;
        let mut rt = RealtimeAudio::new(p, 0.0, None);
        rt.transport = false;

        // Silence before any pad (disable master vinyl dust so silence is exact).
        rt.project.master_tape.dust = 0.0;
        let mut l = vec![0.0f32; sr / 4];
        let mut r = vec![0.0f32; sr / 4];
        rt.next_block(&mut l, &mut r);
        let rms0: f32 = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
        assert!(rms0 < 1e-6, "paused transport is silent: {rms0}");

        // A MIDI pad: C5 on the demo source.
        let settings = crate::grain::GrainSettings { gain: 1.0, ..Default::default() };
        let ev = crate::grain::grain_from_note(&settings, 72, 100, 3.0, rt.live_now_secs(), 1);
        rt.trigger_live(0, ev);
        rt.next_block(&mut l, &mut r);
        let rms1: f32 = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
        assert!(rms1 > 1e-4, "pad sounds while stopped: {rms1}");

        // Pads also mix during transport playback.
        rt.transport = true;
        let ev2 = crate::grain::grain_from_note(&settings, 60, 100, 3.0, rt.live_now_secs(), 2);
        rt.trigger_live(0, ev2);
        rt.next_block(&mut l, &mut r);
        let rms2: f32 = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
        assert!(rms2 > 1e-4);
        // Playhead advanced only while transport ran.
        assert!(rt.playhead_secs() > 0.0);
        assert!((rt.playhead_secs() - 0.25).abs() < 1e-6, "paused blocks froze it");
    }

    #[test]
    fn live_buses_run_their_own_effects() {
        let p = perf_project();
        let sr = p.sample_rate as usize;
        let settings = crate::grain::GrainSettings { gain: 1.0, duration: 0.2, ..Default::default() };
        let tail = |bus: u8| {
            let mut p_clean = p.clone();
            p_clean.master_tape = crate::audio::TapeSettings::default();
            let mut rt = RealtimeAudio::new(p_clean, 0.0, None);
            rt.transport = false;
            rt.set_live_bus(1, vec![AvEffect::new(EffectKind::Delay {
                time: 0.25,
                feedback: 0.5,
                mix: 0.8,
                shift_x: 0.0,
                shift_y: 0.0,
            })]);
            let ev = crate::grain::grain_from_note(&settings, 60, 120, 3.0, rt.live_now_secs(), 1);
            rt.trigger_live_bus(bus, 0, ev);
            let mut l = vec![0.0f32; sr];
            let mut r = vec![0.0f32; sr];
            rt.next_block(&mut l, &mut r);
            // Energy well after the 0.2 s grain ended: only an echo.
            (l[sr * 2 / 5..sr * 3 / 5].iter().map(|s| s * s).sum::<f32>()).sqrt()
        };
        assert!(tail(0) < 1e-3, "dry bus: no echo ({})", tail(0));
        assert!(tail(1) > 0.05, "bus 1 echoes ({})", tail(1));
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
