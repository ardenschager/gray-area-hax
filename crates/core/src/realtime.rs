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

/// Samples over which a live bus crossfades from its old effect chain to
/// a new one (a LOOK change), so switching effects never clicks.
const BUS_XFADE: usize = 2048;

/// De-click ramp (samples) on the arrangement either side of a loop wrap:
/// sounds that run past the loop end fade out instead of being cut.
const WRAP_RAMP: u64 = 192;

/// One live effect bus: its chain + state, and the chain it is fading out
/// of (with the samples left in the fade).
struct LiveBus {
    chain: Vec<crate::fx::AvEffect>,
    states: Vec<AudioFxState>,
    old: Option<(Vec<crate::fx::AvEffect>, Vec<AudioFxState>, usize)>,
}

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
    live_buses: Vec<LiveBus>,
    /// Monotonic sample clock that never pauses (times live events).
    live_pos: u64,
    scratch_clip: StereoBuffer,
    /// Where the arrangement fades back in after the last loop wrap.
    wrap_fade_in: Option<u64>,
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
            wrap_fade_in: None,
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

    /// Carry the live side (sounding hits, the live clock, bus chains and
    /// their tails) over from the engine this one replaces, so an edit to
    /// the arrangement never cuts the grains being played.
    pub fn adopt_live(&mut self, old: &mut RealtimeAudio) {
        if old.project.sample_rate != self.project.sample_rate {
            return;
        }
        self.live = std::mem::take(&mut old.live);
        self.live_buses = std::mem::take(&mut old.live_buses);
        self.live_pos = old.live_pos;
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

    /// Release a live voice (its `GrainEvent::tag.voice`) at `off`, on the
    /// live clock ([`RealtimeAudio::live_now_secs`]): queued and sounding
    /// grains of the voice learn the note-off time, so their ADSR releases
    /// sample-exactly; grains that would start after the release are
    /// dropped. Grains without an envelope are left alone. Allocation-free.
    pub fn release_voice(&mut self, voice: u64, off: f64) {
        if voice == 0 {
            return;
        }
        let mut end = f64::NEG_INFINITY;
        for (_, _, e) in self.live.iter_mut() {
            if e.tag.voice == voice {
                if let Some(env) = e.tag.env.as_mut() {
                    env.off = env.off.min(off);
                    end = end.max(env.end());
                }
            }
        }
        if end.is_finite() {
            self.live.retain(|(_, _, e)| !(e.tag.voice == voice && e.tag.env.is_some() && e.onset >= end));
        }
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
            self.live_buses.push(LiveBus { chain: Vec::new(), states: Vec::new(), old: None });
        }
        let same = self.live_buses[i].chain.len() == chain.len()
            && self.live_buses[i].chain.iter().zip(&chain).all(|(a, b)| {
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
        let bus = &mut self.live_buses[i];
        if !same {
            // New shape: fresh state, and the old chain keeps running
            // underneath for a short crossfade.
            let states = fx::audio_chain_states(&chain, self.project.sample_rate);
            let old_chain = std::mem::replace(&mut bus.chain, chain);
            let old_states = std::mem::replace(&mut bus.states, states);
            bus.old = Some((old_chain, old_states, BUS_XFADE));
        } else {
            bus.chain = chain;
        }
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
                        self.wrap_fade_in = Some(self.sample_pos);
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

        self.declick_wrap(at, n, left, right);
        self.render_live_and_master(at, n, left, right);
    }

    /// Ramp the arrangement down into a loop wrap and back up after it.
    fn declick_wrap(&mut self, at: usize, n: usize, left: &mut [f32], right: &mut [f32]) {
        let Some((_, le)) = self.loop_region.filter(|(ls, le)| le > ls) else { return };
        let sr = self.project.sample_rate as f64;
        let end = (self.project.beats_to_secs(le) * sr).round() as u64;
        let ramp = |d: u64| 0.5 - 0.5 * (std::f32::consts::PI * d as f32 / WRAP_RAMP as f32).cos();
        for i in 0..n {
            let pos = self.sample_pos + i as u64;
            let mut g = 1.0f32;
            if pos < end && end - pos <= WRAP_RAMP {
                g *= ramp(end - pos);
            }
            if let Some(s) = self.wrap_fade_in {
                if pos >= s && pos - s < WRAP_RAMP {
                    g *= ramp(pos - s);
                }
            }
            if g < 1.0 {
                left[at + i] *= g;
                right[at + i] *= g;
            }
        }
        if self.wrap_fade_in.is_some_and(|s| self.sample_pos + n as u64 >= s + WRAP_RAMP) {
            self.wrap_fade_in = None;
        }
    }

    /// Mix live (MIDI) events on the monotonic clock, then run the master
    /// chain, master tape, and master conditioner (sample-exact with offline).
    fn render_live_and_master(&mut self, at: usize, n: usize, left: &mut [f32], right: &mut [f32]) {
        let sr = self.project.sample_rate as f64;
        let t_live = self.live_pos as f64 / sr;
        let n_buses = self.live_buses.len() as u8;
        for bus in 0..=n_buses {
            let has_fx = bus > 0 && {
                let b = &self.live_buses[bus as usize - 1];
                !b.chain.is_empty() || b.old.is_some()
            };
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
                let b = &mut self.live_buses[bus as usize - 1];
                let dry = b.old.as_ref().map(|_| (self.scratch_clip.left.clone(), self.scratch_clip.right.clone()));
                fx::process_audio_chain(&mut self.scratch_clip.left, &mut self.scratch_clip.right, &b.chain, &mut b.states);
                if let (Some((oc, os, left)), Some((mut ol, mut or))) = (b.old.as_mut(), dry) {
                    fx::process_audio_chain(&mut ol, &mut or, oc, os);
                    for i in 0..n {
                        let k = left.saturating_sub(i) as f32 / BUS_XFADE as f32; // old's weight
                        let g = 0.5 - 0.5 * (std::f32::consts::PI * k).cos();
                        self.scratch_clip.left[i] = self.scratch_clip.left[i] * (1.0 - g) + ol[i] * g;
                        self.scratch_clip.right[i] = self.scratch_clip.right[i] * (1.0 - g) + or[i] * g;
                    }
                    *left = left.saturating_sub(n);
                    if *left == 0 {
                        b.old = None;
                    }
                }
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
        if self.project.master_gain_db.abs() > 1e-4 {
            let g = 10.0f32.powf(self.project.master_gain_db.clamp(-24.0, 24.0) / 20.0);
            for s in &mut left[at..at + n] {
                *s *= g;
            }
            for s in &mut right[at..at + n] {
                *s *= g;
            }
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
            ..Default::default()
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
    fn changing_a_bus_effect_crossfades_instead_of_clicking() {
        // A long tone through a delay bus; mid-tone the bus switches to a
        // different effect shape (a LOOK change). The output must not jump.
        let mut p = perf_project();
        p.master_tape = crate::audio::TapeSettings::default();
        let mut rt = RealtimeAudio::new(p, 0.0, None);
        rt.transport = false;
        let delay = AvEffect::new(EffectKind::Delay { time: 0.1, feedback: 0.5, mix: 0.6, shift_x: 0.0, shift_y: 0.0 });
        rt.set_live_bus(1, vec![delay]);
        let settings = crate::grain::GrainSettings { gain: 1.0, duration: 2.0, envelope: 0.1, ..Default::default() };
        let ev = crate::grain::grain_from_note(&settings, 60, 120, 3.0, rt.live_now_secs(), 1);
        rt.trigger_live_bus(1, 0, ev);
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        let mut out = Vec::new();
        for k in 0..120 {
            if k == 60 {
                rt.set_live_bus(1, vec![AvEffect::new(EffectKind::Reverb { size: 0.8, damp: 0.4, mix: 0.5 })]);
            }
            rt.next_block(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        let jump = |a: &[f32]| a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let before = jump(&out[256 * 30..256 * 60]);
        let around = jump(&out[256 * 59..256 * 62]);
        assert!(before > 1e-3, "the tone sounds ({before})");
        assert!(around < before * 2.0, "no click at the switch: {around} vs {before}");
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

    // ---- v003: ADSR, slices, tiles ----

    /// Two overlapping granular clips (one with a clip ADSR) side by side
    /// in Tile layout, plus a pattern row with an ADSR playing chop slices.
    fn v003_project() -> Project {
        let mut p = Project { bpm: 120.0, width: 32, height: 18, fps: 12.0, ..Default::default() };
        p.layout = crate::tile::Layout::Tile;
        let mut src = Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(110.0, 3.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(32, 18, 12.0, 3.0))),
            base_hz: 110.0,
            ..Default::default()
        };
        src.slices = crate::chop::equal_slices(3.0, 8);
        let sid = p.add_source(src);
        let t0 = p.add_track("a");
        let mut a = Clip::new(sid, 0.0, 4.0);
        a.grains.density = 12.0;
        a.grains.adsr = Some(crate::envelope::Adsr { attack: 0.2, decay: 0.3, sustain: 0.6, release: 0.4, ..Default::default() });
        p.tracks[t0].clips.push(a);
        let t1 = p.add_track("b");
        let mut b = Clip::new(sid, 1.0, 2.0);
        b.grains.density = 10.0;
        p.tracks[t1].clips.push(b);
        let mut pat = crate::seq::StepPattern::new("chops");
        let r = pat.add_row(sid);
        for (k, i) in (0..16).step_by(4).enumerate() {
            pat.rows[r].steps[i].on = true;
            pat.rows[r].steps[i].slice_index = Some(k as u16 * 2);
        }
        pat.rows[r].grains.adsr = Some(crate::envelope::Adsr { attack: 0.005, decay: 0.05, sustain: 0.7, release: 0.08, ..Default::default() });
        let pid = p.add_pattern(pat);
        let t2 = p.add_track("seq");
        p.tracks[t2].clips.push(Clip::new_pattern(pid, 0.0, 4.0));
        p
    }

    #[test]
    fn v003_adsr_slices_tiles_stream_matches_offline() {
        let p = v003_project();
        let offline = render_project(&p, 0.0, 4.0);
        let total = offline.audio.len();
        let mut rt = RealtimeAudio::new(p, 0.0, None);
        let (mut left, mut right) = (vec![0.0f32; total], vec![0.0f32; total]);
        let sizes = [512usize, 64, 2048, 733, 4096];
        let (mut pos, mut k) = (0, 0);
        while pos < total {
            let b = sizes[k % sizes.len()].min(total - pos);
            rt.next_block(&mut left[pos..pos + b], &mut right[pos..pos + b]);
            pos += b;
            k += 1;
        }
        let mut max_diff = 0.0f32;
        for i in 0..total {
            max_diff = max_diff.max((left[i] - offline.audio.left[i]).abs());
            max_diff = max_diff.max((right[i] - offline.audio.right[i]).abs());
        }
        assert!(max_diff < 1e-5, "realtime == offline with ADSR + tiles + slices: {max_diff}");
        assert!(offline.audio.rms() > 0.01);

        // Every grain is tiled; the clips got different slots.
        let slots = |ci: usize| offline.events[ci].iter().filter_map(|e| e.tag.tile).collect::<std::collections::BTreeSet<_>>();
        assert!(offline.events.iter().flatten().all(|e| e.tag.tile.is_some()));
        let (sa, sb) = (slots(0), slots(1));
        assert_eq!(sa.len(), 1);
        assert_eq!(sb.len(), 1);
        assert_ne!(sa, sb, "overlapping clips get their own tiles");
        // The clip ADSR rides on clip a's grains.
        assert!(offline.events[0].iter().all(|e| e.tag.env.is_some_and(|v| v.on == 0.0 && (v.end() - 2.0).abs() < 1e-6)));
        // Slice steps start at their slice.
        let starts: Vec<f64> = offline.events[2].iter().map(|e| e.source_pos).collect();
        for s in &starts {
            assert!((s / 0.375 - (s / 0.375).round()).abs() < 1e-6, "on a slice boundary: {s}");
        }

        // The picture: while a and b overlap (1–2 s, after a's attack),
        // both halves are lit.
        let f = &offline.frames[(1.5 * 12.0) as usize];
        let half = |x0: u32, x1: u32| {
            let mut sum = 0.0;
            for y in 0..18u32 {
                for x in x0..x1 {
                    let k = ((y * 32 + x) * 4) as usize;
                    sum += f.data[k] as f32 + f.data[k + 1] as f32 + f.data[k + 2] as f32;
                }
            }
            sum / ((x1 - x0) * 18 * 3) as f32
        };
        assert!(half(0, 15) > 5.0 && half(17, 32) > 5.0, "{} {}", half(0, 15), half(17, 32));
        // After a's release (2 s) the picture is cut: by 2.5 s only the
        // pattern's short hits remain.
        let grids = crate::render::frame_tile_grids(&crate::render::plan_project(&v003_project()), 0.0, 12.0, 48);
        assert_eq!(grids.len(), 48);
        assert!(grids[18].0 >= 2, "two tiles mid-overlap: {:?}", grids[18]);
    }

    #[test]
    fn assign_tiles_gives_overlapping_voices_distinct_slots() {
        let ev = |onset: f64, voice: u64| GrainEvent { onset, duration: 1.0, tag: crate::grain::VoiceTag { voice, ..Default::default() }, ..Default::default() };
        let mut plans = vec![
            crate::render::ClipPlan { track: 0, clip_index: 0, parts: vec![(0, vec![ev(0.0, 1), ev(0.5, 1)])], video_parts: None, style: Default::default() },
            crate::render::ClipPlan { track: 1, clip_index: 0, parts: vec![(0, vec![ev(0.25, 2)])], video_parts: None, style: Default::default() },
            crate::render::ClipPlan { track: 2, clip_index: 0, parts: vec![(0, vec![ev(3.0, 3)])], video_parts: None, style: Default::default() },
        ];
        crate::render::assign_tiles(&mut plans, true);
        let tile = |p: usize, i: usize| plans[p].parts[0].1[i].tag.tile;
        assert_eq!(tile(0, 0), Some(0));
        assert_eq!(tile(0, 1), Some(0));
        assert_eq!(tile(1, 0), Some(1));
        assert_eq!(tile(2, 0), Some(0), "slot 0 is free again by 3 s");
        // Pans follow the grid at each grain's onset.
        assert_eq!(plans[0].parts[0].1[0].pan, 0.0, "alone: centered");
        assert!((plans[0].parts[0].1[1].pan + 0.8).abs() < 1e-6, "with voice 2: left");
        assert!((plans[1].parts[0].1[0].pan - 0.8).abs() < 1e-6);
        assert_eq!(plans[2].parts[0].1[0].pan, 0.0);
        let grids = crate::render::frame_tile_grids(&plans, 0.0, 4.0, 16);
        assert_eq!(grids[0], (1, 1));
        assert_eq!(grids[1], (2, 1));
        assert_eq!(grids[5], (1, 1), "1.25 s: voice 2 is over, voice 1 (0.5..1.5) remains");
    }

    #[test]
    fn release_voice_releases_live_grains() {
        let p = perf_project();
        let sr = p.sample_rate as usize;
        let mut rt = RealtimeAudio::new(p, 0.0, None);
        rt.transport = false;
        rt.project.master_tape.dust = 0.0;
        let adsr = crate::envelope::Adsr { attack: 0.001, decay: 0.0, sustain: 1.0, release: 0.05, ..Default::default() };
        let now = rt.live_now_secs();
        let voice = 0xE000_0000_0000_0042u64;
        for k in 0..8 {
            let mut ev = crate::grain::grain_from_note(&crate::grain::GrainSettings { gain: 1.0, duration: 0.3, ..Default::default() }, 60, 100, 3.0, now + k as f64 * 0.25, 10 + k);
            ev.tag = crate::grain::VoiceTag { voice, env: Some(crate::envelope::VoiceEnv::held(adsr, now)), tile: None };
            rt.trigger_live(0, ev);
        }
        let (mut l, mut r) = (vec![0.0f32; sr / 4], vec![0.0f32; sr / 4]);
        rt.next_block(&mut l, &mut r);
        let held: f32 = l.iter().map(|s| s * s).sum::<f32>();
        assert!(held > 1e-3, "sounds while held");
        let off = rt.live_now_secs();
        rt.release_voice(voice, off);
        rt.release_voice(0, off); // voice 0 = untagged: ignored
        let n_live = rt.live.len();
        assert!(n_live <= 2, "grains past the release are dropped: {n_live}");
        let (mut l2, mut r2) = (vec![0.0f32; sr / 2], vec![0.0f32; sr / 2]);
        rt.next_block(&mut l2, &mut r2);
        // Past the release (+ effect tails: perf_project's buses are dry for live grains).
        let tail = &l2[(0.06 * sr as f64) as usize..];
        let e: f32 = tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32;
        assert!(e.sqrt() < 1e-4, "silent after the release: {}", e.sqrt());
    }

    #[test]
    fn muted_tracks_get_no_tiles_and_pads_keep_their_tile() {
        // Six looping pad tracks (Perform: pad i = track i, ON = unmuted,
        // the loop refilled with copies), three playing, entering at
        // different times: they take slots 0, 1, 2 in a 2×2 grid, each
        // pad keeps its slot on every pass, and muted pads get no tile.
        let mut p = Project { bpm: 120.0, width: 32, height: 18, fps: 12.0, ..Default::default() };
        p.layout = crate::tile::Layout::Tile;
        let sid = p.add_source(Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(110.0, 3.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(32, 18, 12.0, 3.0))),
            base_hz: 110.0,
            ..Default::default()
        });
        let entry = [None, Some(2.0), None, Some(0.0), Some(1.0), None]; // beats
        for (k, e) in entry.iter().enumerate() {
            let t = p.add_track(&format!("pad{k}"));
            let from = e.unwrap_or(0.0);
            let mut b = from;
            while b < 8.0 {
                let mut c = Clip::new_snippet(sid, b, 1.0);
                c.grains.density = 10.0;
                p.tracks[t].clips.push(c);
                b += 1.0;
            }
            p.tracks[t].muted = e.is_none();
        }
        let plans = crate::render::plan_project(&p);
        let slots = |track: usize| {
            plans
                .iter()
                .filter(|cp| cp.track == track)
                .flat_map(|cp| cp.parts.iter().chain(cp.video_parts.iter().flatten()).flat_map(|(_, e)| e.iter().map(|e| e.tag.tile)))
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(slots(3), [Some(0)].into(), "first in: slot 0, every pass");
        assert_eq!(slots(4), [Some(1)].into());
        assert_eq!(slots(1), [Some(2)].into());
        for t in [0, 2, 5] {
            assert!(slots(t).iter().all(|s| s.is_none()), "muted track {t} has no tile");
        }
        let grids = crate::render::frame_tile_grids(&plans, 0.0, 12.0, 48);
        assert_eq!(grids[3], (1, 1), "0.25 s: pad 3 alone");
        assert_eq!(grids[9], (2, 1), "0.75 s: pads 3 + 4");
        for (f, g) in grids.iter().enumerate().skip(13) {
            assert_eq!(*g, (2, 2), "frame {f}: three pads → 2×2 (no 3×3 with holes)");
        }
        let out = render_project(&p, 0.0, 4.0);
        assert!(out.events.iter().flatten().all(|e| e.tag.tile.is_none_or(|s| s <= 2)));
        assert!(out.frames[18].mean_luma() > 1.0, "the tiles draw");
    }
}
