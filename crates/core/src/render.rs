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

use crate::audio::{render_grains_audio_spatial, StereoBuffer, StudioBusState, TapeState};
use crate::fx;
use crate::grain::{schedule_grains_auto, ChordVoicing, GrainEvent, VoiceSculpt};
use crate::seq::pattern_events;
use crate::timeline::{Clip, ClipKind, DuckState, Project, Source};
use crate::video::{
    apply_video_tape, blend_layer_over, blend_stage_layer, composite_grains_frame_stage,
    draw_stage_caption, Frame, VisualGrainStyle, OPTICAL_CARRY_SECS,
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
    /// Optional dedicated continuous video events for untextured stretched
    /// snippets, preventing 50%-overlap audio OLA grains from strobing
    /// straight-alpha video opacity at the grain hop rate.
    pub video_parts: Option<Vec<(usize, Vec<GrainEvent>)>>,
    pub style: VisualGrainStyle,
}

/// Find a small source-position offset (within `±8 ms`) around `nominal_pos`
/// whose waveform best correlates with `ref_pos` (where the previous OLA grain
/// currently sits), eliminating comb-filtering / phase warble on stretched
/// basslines, pads, and vocals (WSOLA).
fn wsola_align(audio: &crate::audio::AudioClip, ref_pos: f64, nominal_pos: f64) -> f64 {
    let sr = audio.sample_rate as usize;
    let n = audio.samples.len();
    if sr == 0 || n < 1024 {
        return nominal_pos;
    }
    let win = (sr / 180).clamp(64, 320); // ~5.5 ms correlation window
    let max_off = (sr / 125).clamp(32, 480) as isize; // ±8 ms search range
    let ref_i = ((ref_pos * sr as f64).round() as isize).rem_euclid(n as isize) as usize;
    let nom_i = ((nominal_pos * sr as f64).round() as isize).rem_euclid(n as isize);

    let mut best_off = 0isize;
    let mut best_score = f32::NEG_INFINITY;
    let mut d = -max_off;
    while d <= max_off {
        let cand_i = (nom_i + d).rem_euclid(n as isize) as usize;
        let mut dot = 0.0f32;
        let mut e_cand = 1e-6f32;
        let mut j = 0usize;
        while j < win {
            let a = audio.samples[(ref_i + j) % n];
            let b = audio.samples[(cand_i + j) % n];
            dot += a * b;
            e_cand += b * b;
            j += 2;
        }
        // Slight preference for smaller offsets when correlation ties.
        let penalty = 1.0 - 0.05 * (d.abs() as f32 / max_off as f32);
        let score = (dot / e_cand.sqrt()) * penalty;
        if score > best_score {
            best_score = score;
            best_off = d;
        }
        d += 2;
    }
    (nominal_pos + best_off as f64 / sr as f64).rem_euclid(n as f64 / sr as f64)
}

/// Events for a snippet clip. When `speed` matches the pitch ratio
/// (including the default speed=1 / pitch=0, or `varispeed = true`), the
/// source plays as ONE pristine grain. Otherwise the snippet becomes a
/// WSOLA-aligned granular time-stretch: overlapping 200 ms Hann grains whose
/// read head advances at `speed` while each grain resamples at the pitch ratio.
fn snippet_events(
    clip: &Clip,
    source: &Source,
    clip_t0: f64,
    clip_len: f64,
    secs_per_beat: f64,
    eff_sculpt: VoiceSculpt,
    eff_choke: u8,
) -> Vec<GrainEvent> {
    let g = &clip.grains;
    let source_len = source.duration();
    let start_pos = (g.position as f64 * source_len).min(source_len);
    let base_hz = if clip.key.is_some() && g.position > 0.02 {
        source.base_hz_at(start_pos)
    } else {
        source.effective_base_hz()
    };
    let quantized = |st: f32, g: &crate::grain::GrainSettings| -> f32 {
        crate::music::effective_ratio(
            st,
            clip.key.as_ref(),
            base_hz,
            g.quantize_amount,
            g.detune_cents,
        )
    };

    let base_ratio = quantized(g.pitch + eff_sculpt.pitch_semitones, g);
    let eff_speed0 = if g.varispeed { base_ratio } else { g.speed };

    // Asynchronous real-seconds Basinski tape loop (`free_loop_secs`) independent
    // of the BPM bar grid, with an `80ms` raised-cosine crossfade seam and
    // per-pass `disintegrate` / `ebb_flow` evolution.
    if let Some(free_secs) = eff_sculpt.free_loop_secs.filter(|s| *s > 0.05) {
        let period = (free_secs as f64).clamp(0.05, 120.0);
        let seam = 0.08_f64.min(period * 0.25);
        let rev = g.reverse_prob >= 0.5;
        let mut raw_events = Vec::new();
        let mut t = 0.0f64;
        let mut pass_k = 0u32;
        while t < clip_len {
            let rem = clip_len - t;
            let dur = if rem > period + seam { period + seam } else { rem.max(0.02) };
            let env_taper = ((2.0 * seam) / dur).clamp(0.02, 0.90) as f32;
            let id = g.seed.wrapping_add(pass_k as u64);
            let ebb = crate::grain::ebb_flow_gain_at(eff_sculpt.ebb_flow, pass_k, clip_t0 + t, g.seed);
            let ox = crate::grain::oxide_dropout_at(eff_sculpt.disintegrate, pass_k, 0.5, id);
            let comb_hz = if eff_sculpt.comb_mix > 1e-3 {
                crate::grain::quantize_comb_hz(base_hz.max(40.0) * base_ratio, clip.key.as_ref())
            } else {
                0.0
            };
            let mut ev = GrainEvent {
                onset: clip_t0 + t,
                source_pos: start_pos,
                duration: dur as f32,
                pitch_ratio: base_ratio,
                gain: g.gain * ebb,
                pan: g.pan.clamp(-1.0, 1.0),
                envelope: env_taper,
                env_skew: 0.0,
                reverse: rev,
                id,
                sculpt: eff_sculpt,
                comb_hz,
                loop_pass: pass_k,
                ..Default::default()
            };
            ev.tag.is_wash = true;
            ev.tag.choke_group = eff_choke;
            ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
            if eff_sculpt.chord != ChordVoicing::Off {
                crate::grain::expand_chord_grain(
                    &ev,
                    eff_sculpt.chord,
                    base_hz,
                    clip.key.as_ref(),
                    &mut raw_events,
                );
            } else {
                raw_events.push(ev);
            }
            t += period;
            pass_k += 1;
        }
        return raw_events;
    }

    let enrich_snippet_grains = |raw: Vec<GrainEvent>| -> Vec<GrainEvent> {
        if !eff_sculpt.is_active() && eff_choke == 0 {
            return raw;
        }
        let mut out = Vec::with_capacity(raw.len());
        for mut ev in raw {
            ev.sculpt = eff_sculpt;
            ev.tag.choke_group = eff_choke;
            ev.tag.is_wash = eff_sculpt.is_wash();
            if eff_sculpt.comb_mix > 1e-3 && ev.comb_hz <= 0.0 {
                ev.comb_hz = crate::grain::quantize_comb_hz(
                    base_hz.max(40.0) * ev.pitch_ratio,
                    clip.key.as_ref(),
                );
            }
            if eff_sculpt.disintegrate > 1e-3 {
                let ox = crate::grain::oxide_dropout_at(
                    eff_sculpt.disintegrate,
                    ev.loop_pass,
                    0.5,
                    ev.id ^ g.seed,
                );
                ev.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
            }
            if eff_sculpt.ebb_flow > 1e-3 {
                ev.gain *= crate::grain::ebb_flow_gain_at(eff_sculpt.ebb_flow, ev.loop_pass, ev.onset, g.seed);
            }
            if eff_sculpt.chord != ChordVoicing::Off {
                crate::grain::expand_chord_grain(
                    &ev,
                    eff_sculpt.chord,
                    base_hz,
                    clip.key.as_ref(),
                    &mut out,
                );
            } else {
                out.push(ev);
            }
        }
        out
    };

    if g.texture > 1e-3 {
        return enrich_snippet_grains(textured_events(
            clip,
            source_len,
            clip_t0,
            clip_len,
            secs_per_beat,
            eff_sculpt.pitch_semitones,
            &quantized,
        ));
    }
    if clip.automation.is_empty() && (eff_speed0 - base_ratio).abs() < 1e-3 {
        let rev = g.reverse_prob >= 0.5;
        let audio_len = source.audio.as_ref().map(|a| a.duration()).unwrap_or(source_len);
        let first_avail = if rev {
            audio_len
        } else {
            (audio_len - start_pos).max(0.05)
        };
        let first_out_dur = first_avail / base_ratio.max(0.05) as f64;
        // If the snippet fits inside the source media, emit a single pristine grain.
        if clip_len <= first_out_dur + 1e-4 || audio_len < 0.25 {
            return enrich_snippet_grains(vec![GrainEvent {
                onset: clip_t0,
                source_pos: start_pos,
                duration: clip_len as f32,
                pitch_ratio: base_ratio,
                gain: g.gain,
                pan: g.pan.clamp(-1.0, 1.0),
                envelope: g.envelope,
                env_skew: g.env_skew,
                reverse: rev,
                id: g.seed,
                ..Default::default()
            }]);
        }
        // Otherwise seamlessly crossfade-loop across `audio_len` using raised-cosine
        // Tukey overlaps (`cos^2` / complementary half-cosines sum to exact 1.0),
        // while `snippet_video_events` renders video as ONE unbroken pass!
        let full_cycle_dur = (audio_len / base_ratio.max(0.05) as f64).max(0.1);
        let xfade = 0.16_f64.min(full_cycle_dur * 0.25).min(first_out_dur * 0.4);
        let mut events = Vec::new();
        let mut t = 0.0f64;
        let mut pass_idx = 0u64;
        while t < clip_len {
            let src_p = if pass_idx == 0 { start_pos } else { 0.0 };
            let max_dur = if pass_idx == 0 { first_out_dur } else { full_cycle_dur };
            let dur = max_dur.min(clip_len - t);
            if dur <= 0.005 {
                break;
            }
            let env_frac = ((2.0 * xfade) / dur).clamp(g.envelope as f64, 0.95) as f32;
            events.push(GrainEvent {
                onset: clip_t0 + t,
                source_pos: src_p,
                duration: dur as f32,
                pitch_ratio: base_ratio,
                gain: g.gain,
                pan: g.pan.clamp(-1.0, 1.0),
                envelope: env_frac,
                env_skew: 0.0,
                reverse: rev,
                id: g.seed.wrapping_add(pass_idx),
                loop_pass: pass_idx as u32,
                ..Default::default()
            });
            if t + dur >= clip_len - 1e-5 {
                break;
            }
            t += (dur - xfade).max(0.02);
            pass_idx += 1;
        }
        return enrich_snippet_grains(events);
    }

    // WSOLA-aligned granular time-stretch train: 200 ms Hann grains at 50%
    // overlap sum to constant amplitude; correlation alignment prevents comb
    // warble while the read head integrates `speed` over time.
    const GRAIN: f64 = 0.20;
    const HOP: f64 = GRAIN / 2.0;
    let mut events = Vec::new();
    let mut head = g.position as f64 * source_len;
    let mut prev_pos = head;
    let mut prev_ratio = base_ratio as f64;
    let mut k = 0u64;
    loop {
        let t = k as f64 * HOP;
        if t >= clip_len {
            break;
        }
        let eff = if clip.automation.is_empty() {
            g.clone()
        } else {
            crate::auto::settings_at(g, &clip.automation, t / secs_per_beat.max(1e-9))
        };
        let ratio = quantized(eff.pitch + eff_sculpt.pitch_semitones, &eff);
        let speed = if eff.varispeed { ratio } else { eff.speed };
        let nom_pos = head.rem_euclid(source_len.max(1e-6));
        let rev = eff.reverse_prob >= 0.5;
        let aligned_pos = if k > 0 && !rev {
            if let Some(audio) = source.audio.as_deref() {
                let ref_pos = (prev_pos + HOP * prev_ratio).rem_euclid(source_len.max(1e-6));
                wsola_align(audio, ref_pos, nom_pos)
            } else {
                nom_pos
            }
        } else {
            nom_pos
        };
        prev_pos = aligned_pos;
        prev_ratio = ratio as f64;
        events.push(GrainEvent {
            onset: clip_t0 + t,
            source_pos: aligned_pos,
            duration: GRAIN.min(clip_len - t + HOP) as f32,
            pitch_ratio: ratio,
            gain: eff.gain,
            pan: eff.pan.clamp(-1.0, 1.0),
            envelope: 1.0, // full hann: overlapped windows sum to unity
            env_skew: 0.0,
            reverse: rev,
            id: g.seed.wrapping_add(k),
            ..Default::default()
        });
        head += HOP * speed.max(0.05) as f64;
        k += 1;
    }
    enrich_snippet_grains(events)
}

/// Continuous visual event(s) for an untextured stretched snippet (`texture == 0`
/// and `speed != base_ratio`): advances video smoothly at `speed` as ONE
/// unbroken pass instead of alpha-stacking 50%-overlap Hann audio grains (which
/// would ripple straight-alpha opacity between `1.0` and `0.75` at the hop rate).
fn snippet_video_events(
    clip: &Clip,
    source: &Source,
    clip_t0: f64,
    clip_len: f64,
    eff_sculpt: VoiceSculpt,
    eff_choke: u8,
) -> Option<Vec<GrainEvent>> {
    let g = &clip.grains;
    if g.texture > 1e-3
        || eff_sculpt.free_loop_secs.is_some()
        || eff_sculpt.chord != ChordVoicing::Off
        || eff_sculpt.disintegrate > 1e-3
    {
        return None;
    }
    let source_len = source.duration();
    let start_pos = (g.position as f64 * source_len).min(source_len);
    let bh = if clip.key.is_some() && g.position > 0.02 {
        source.base_hz_at(start_pos)
    } else {
        source.effective_base_hz()
    };
    let base_ratio = crate::music::effective_ratio(
        g.pitch + eff_sculpt.pitch_semitones,
        clip.key.as_ref(),
        bh,
        g.quantize_amount,
        g.detune_cents,
    );
    let eff_speed = if g.varispeed { base_ratio } else { g.speed };
    let audio_len = source.audio.as_ref().map(|a| a.duration()).unwrap_or(source_len);
    let video_len = source.video.as_ref().map(|v| v.duration()).unwrap_or(source_len);
    let first_avail = if g.reverse_prob >= 0.5 {
        audio_len
    } else {
        (audio_len - start_pos).max(0.05)
    };
    let wraps_audio = clip_len > first_avail / base_ratio.max(0.05) as f64 + 1e-4 && audio_len >= 0.25;
    let wraps_video = video_len >= 0.20
        && clip_len * eff_speed.max(0.05) as f64 > (video_len - start_pos.min(video_len)).max(0.05) + 1e-4;
    if clip.automation.is_empty() && (eff_speed - base_ratio).abs() < 1e-3 && !wraps_audio && !wraps_video {
        return None; // `parts` is already a single unbroken grain
    }
    let mut ev = GrainEvent {
        onset: clip_t0,
        source_pos: start_pos,
        duration: clip_len as f32,
        pitch_ratio: eff_speed.max(0.05),
        gain: g.gain,
        pan: g.pan.clamp(-1.0, 1.0),
        envelope: g.envelope,
        env_skew: g.env_skew,
        reverse: g.reverse_prob >= 0.5,
        id: g.seed,
        sculpt: eff_sculpt,
        ..Default::default()
    };
    ev.tag.choke_group = eff_choke;
    ev.tag.is_wash = eff_sculpt.is_wash();
    Some(vec![ev])
}

/// The sample <-> grains morph (`texture` > 0): a grain train whose read
/// head follows the media at `speed` like a snippet, but whose grains
/// shrink, scatter (onset, position, pitch, pan) and thin out toward the
/// clip's grain settings as `texture` -> 1. Loudness stays roughly level:
/// coherent overlap-add at low texture, power-normalized cloud at high.
fn textured_events(
    clip: &Clip,
    source_len: f64,
    clip_t0: f64,
    clip_len: f64,
    secs_per_beat: f64,
    sculpt_pitch: f32,
    quantized: &dyn Fn(f32, &crate::grain::GrainSettings) -> f32,
) -> Vec<GrainEvent> {
    let g = &clip.grains;
    let mut rng = crate::dsp::Rng::new(g.seed ^ 0x7E47_0E55);
    let mut events = Vec::new();
    let mut head = g.position as f64 * source_len;
    let mut t = 0.0f64;
    let mut k = 0u64;
    while t < clip_len {
        let eff = if clip.automation.is_empty() {
            g.clone()
        } else {
            crate::auto::settings_at(g, &clip.automation, t / secs_per_beat.max(1e-9))
        };
        let tex = eff.texture.clamp(0.0, 1.0);
        let grain_dur = eff.duration.clamp(0.01, 2.0);
        // Grain length: long, smooth windows -> the clip's grain size.
        let dur = (0.30f32.ln() + (grain_dur.ln() - 0.30f32.ln()) * tex).exp()
            * (1.0 + tex * eff.duration_jitter * rng.bipolar());
        let dur = dur.max(0.005) as f64;
        // Hop: 50% overlap (seamless) -> the clip's density.
        let cloud_hop = 1.0 / eff.density.max(0.5) as f64;
        let hop = (dur / 2.0 + (cloud_hop - dur / 2.0) * tex as f64).max(0.002);
        let jitter = tex as f64 * hop * 0.5 * rng.bipolar() as f64;
        let onset_t = (t + jitter).max(0.0);
        let pos = head + tex as f64 * eff.spray as f64 * rng.bipolar() as f64;
        let st = eff.pitch + sculpt_pitch + tex * eff.pitch_jitter * rng.bipolar();
        let pan = eff.pan + tex * eff.pan_spread.clamp(0.0, 1.0) * rng.bipolar();
        let reverse = if tex < 0.5 {
            eff.reverse_prob >= 0.5
        } else {
            rng.next_f32() < eff.reverse_prob
        };
        // Overlap factor 2 = unity OLA; normalize toward sqrt as grains
        // decorrelate so the cloud doesn't swell.
        let overlap = (dur / hop).max(0.25) as f32;
        let norm = (overlap * 0.5).powf(-(1.0 - 0.5 * tex));
        if onset_t < clip_len {
            events.push(GrainEvent {
                onset: clip_t0 + onset_t,
                source_pos: pos.rem_euclid(source_len.max(1e-6)),
                duration: dur.min(clip_len - onset_t + hop) as f32,
                pitch_ratio: quantized(st, &eff),
                gain: eff.gain * norm,
                pan: pan.clamp(-1.0, 1.0),
                envelope: 1.0,
                env_skew: tex * eff.env_skew,
                reverse,
                id: g.seed.wrapping_add(k),
                ..Default::default()
            });
        }
        head += hop * eff.speed.max(0.05) as f64;
        t += hop;
        k += 1;
    }
    events
}

/// How a repeating clip lays its passes out, seconds.
struct PassLayout {
    pass: f64,
    period: f64,
    /// Media seconds each pass moves the read head (0 = same bit).
    advance: f64,
    source_len: f64,
}

/// Repeat one pass of events (`pass` seconds from `t0`) every `period`
/// seconds until `total`, skipping masked-off passes and walking the read
/// head by `advance` per pass. Grains crossing the clip end are shortened
/// so nothing plays past the clip.
fn tile_events(
    pass_evs: &[GrainEvent],
    clip: &Clip,
    t0: f64,
    lay: &PassLayout,
    total: f64,
) -> Vec<GrainEvent> {
    let end = t0 + total;
    let n = (total / lay.period.max(1e-6)).ceil().max(1.0) as usize;
    let mut out = Vec::with_capacity(pass_evs.len() * n);
    for k in 0..n {
        if !clip.pass_on(k) {
            continue;
        }
        let off = k as f64 * lay.period;
        let shift = k as f64 * lay.advance;
        for e in pass_evs {
            // Events are planned for exactly one pass.
            if e.onset >= t0 + lay.pass {
                continue;
            }
            let mut e = e.clone();
            e.onset += off;
            if e.onset >= end {
                continue;
            }
            if shift != 0.0 && lay.source_len > 0.0 {
                e.source_pos = (e.source_pos + shift).rem_euclid(lay.source_len);
            }
            e.duration = e.duration.min((end - e.onset) as f32);
            e.loop_pass = e.loop_pass.saturating_add(k as u32);
            if k > 0 && e.sculpt.disintegrate > 1e-3 {
                let ox = crate::grain::oxide_dropout_at(
                    e.sculpt.disintegrate,
                    e.loop_pass,
                    0.5,
                    e.id ^ clip.grains.seed,
                );
                e.tag.oxide_dropout = (1.0 - ox).clamp(0.0, 1.0);
            }
            if k > 0 && e.sculpt.ebb_flow > 1e-3 {
                e.gain *= crate::grain::ebb_flow_gain_at(
                    e.sculpt.ebb_flow,
                    e.loop_pass,
                    e.onset,
                    e.id ^ clip.grains.seed,
                );
            }
            out.push(e);
        }
    }
    out
}

/// Apply MPC choke groups (`1..=4`) across all planned events on unmuted tracks:
/// triggering any voice in group `g > 0` at `t_cut` mutes earlier voices in group `g`
/// (`e.onset < t_cut - 0.002`) with a click-free `3 ms` raised-cosine release while
/// preserving simultaneous chord siblings (`|t_cut - e.onset| <= 2 ms`).
pub fn apply_choke_groups(plans: &mut [ClipPlan], project: &Project) {
    let mut triggers: [Vec<f64>; 4] = Default::default();
    for cp in plans.iter() {
        if project.tracks.get(cp.track).is_some_and(|t| t.muted) {
            continue;
        }
        for (_, evs) in &cp.parts {
            for e in evs {
                let g = e.tag.choke_group;
                if (1..=4).contains(&g) {
                    triggers[(g - 1) as usize].push(e.onset);
                }
            }
        }
    }
    if triggers.iter().all(|v| v.is_empty()) {
        return;
    }
    for list in &mut triggers {
        list.sort_by(|a, b| a.total_cmp(b));
        list.dedup_by(|a, b| (*a - *b).abs() <= 1e-4);
    }

    for cp in plans.iter_mut() {
        if project.tracks.get(cp.track).is_some_and(|t| t.muted) {
            continue;
        }
        for (_, evs) in cp.parts.iter_mut().chain(cp.video_parts.iter_mut().flatten()) {
            for e in evs.iter_mut() {
                let g = e.tag.choke_group;
                if !(1..=4).contains(&g) {
                    continue;
                }
                let list = &triggers[(g - 1) as usize];
                let min_cut = e.onset + 0.002;
                let idx = list.partition_point(|&t| t <= min_cut);
                if let Some(&t_cut) = list.get(idx) {
                    if t_cut < e.audio_end() {
                        e.tag.choke_at = Some(e.tag.choke_at.map_or(t_cut, |c| c.min(t_cut)));
                    }
                }
            }
        }
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
            let full_len = project.beats_to_secs(clip.length_beats);
            let eff_sculpt = if clip.grains.sculpt.is_active() {
                clip.grains.sculpt
            } else {
                track.sculpt
            };
            let eff_choke = if clip.grains.choke_group > 0 {
                clip.grains.choke_group.min(4)
            } else {
                track.choke_group.min(4)
            };
            // Free-running asynchronous tape loops plan over `full_len` directly;
            // beat-repeating clips plan one pass, then tile it (below).
            let has_free_loop =
                matches!(clip.kind, ClipKind::Snippet) && eff_sculpt.free_loop_secs.is_some();
            let clip_len = if has_free_loop {
                full_len
            } else {
                project.beats_to_secs(clip.pass_beats())
            };
            let (parts, video_parts, style) = match clip.kind {
                ClipKind::Granular => {
                    let Some(source) = project.sources.get(clip.source) else { continue };
                    if source.duration() <= 0.0 {
                        continue;
                    }
                    let mut g_cfg = clip.grains.clone();
                    g_cfg.sculpt = eff_sculpt;
                    g_cfg.choke_group = eff_choke;
                    let events = schedule_grains_auto(
                        &g_cfg,
                        &clip.automation,
                        secs_per_beat,
                        clip_t0,
                        clip_len,
                        source.duration(),
                        source.effective_base_hz(),
                        clip.key.as_ref(),
                    );
                    (vec![(clip.source, events)], None, clip.visual)
                }
                ClipKind::Snippet => {
                    let Some(source) = project.sources.get(clip.source) else { continue };
                    if source.duration() <= 0.0 {
                        continue;
                    }
                    let evs = snippet_events(
                        clip,
                        source,
                        clip_t0,
                        clip_len,
                        secs_per_beat,
                        eff_sculpt,
                        eff_choke,
                    );
                    let vevs = snippet_video_events(
                        clip,
                        source,
                        clip_t0,
                        clip_len,
                        eff_sculpt,
                        eff_choke,
                    )
                    .map(|v| vec![(clip.source, v)]);
                    (
                        vec![(clip.source, evs)],
                        vevs,
                        VisualGrainStyle::morph(
                            clip.visual.additive,
                            &clip.visual,
                            clip.grains.texture,
                        ),
                    )
                }
                ClipKind::Pattern(pid) => {
                    let Some(pattern) = project.patterns.get(pid) else { continue };
                    let mut parts = pattern_events(
                        pattern,
                        &project.sources,
                        clip_t0,
                        clip_len,
                        secs_per_beat,
                        clip.key.as_ref(),
                        &clip.automation,
                    );
                    if track.sculpt.is_active() || track.choke_group > 0 {
                        for (sid, evs) in &mut parts {
                            let bh = project
                                .sources
                                .get(*sid)
                                .map_or(440.0, |s| s.effective_base_hz());
                            for e in evs {
                                if !e.sculpt.is_active() && track.sculpt.is_active() {
                                    e.sculpt = track.sculpt;
                                    e.tag.is_wash = track.sculpt.is_wash();
                                    if track.sculpt.pitch_semitones.abs() > 1e-4 {
                                        e.pitch_ratio *= crate::music::semitones_to_ratio(
                                            track.sculpt.pitch_semitones,
                                        );
                                    }
                                    if track.sculpt.comb_mix > 1e-3 && e.comb_hz <= 0.0 {
                                        e.comb_hz = crate::grain::quantize_comb_hz(
                                            bh.max(40.0) * e.pitch_ratio,
                                            clip.key.as_ref(),
                                        );
                                    }
                                }
                                if e.tag.choke_group == 0 && track.choke_group > 0 {
                                    e.tag.choke_group = track.choke_group.min(4);
                                }
                            }
                        }
                    }
                    (parts, None, clip.visual)
                }
            };
            let tile_part_list = |plist: Vec<(usize, Vec<GrainEvent>)>| -> Vec<(usize, Vec<GrainEvent>)> {
                let period = project.beats_to_secs(clip.period_beats());
                let rate = match clip.kind {
                    ClipKind::Snippet => {
                        if clip.grains.varispeed {
                            let bh = project
                                .sources
                                .get(clip.source)
                                .map(|s| s.effective_base_hz())
                                .unwrap_or(440.0);
                            crate::music::effective_ratio(
                                clip.grains.pitch + eff_sculpt.pitch_semitones,
                                clip.key.as_ref(),
                                bh,
                                clip.grains.quantize_amount,
                                clip.grains.detune_cents,
                            ) as f64
                        } else {
                            clip.grains.speed as f64
                        }
                    }
                    ClipKind::Granular => clip.grains.scan_speed as f64,
                    ClipKind::Pattern(_) => 0.0,
                };
                plist
                    .into_iter()
                    .map(|(sid, evs)| {
                        let lay = PassLayout {
                            pass: clip_len,
                            period,
                            advance: clip.loop_advance as f64 * clip_len * rate,
                            source_len: project
                                .sources
                                .get(sid)
                                .map(|s| s.duration())
                                .unwrap_or(0.0),
                        };
                        (sid, tile_events(&evs, clip, clip_t0, &lay, full_len))
                    })
                    .collect()
            };
            let should_tile = clip.loop_beats.is_some() && !has_free_loop;
            let parts = if should_tile { tile_part_list(parts) } else { parts };
            let mut video_parts = if should_tile {
                video_parts.map(tile_part_list)
            } else {
                video_parts
            };
            let mut parts = parts;
            // A clip with an ADSR is one voice: on at its start, released so
            // the release ends with the clip (sound and picture cut together).
            if let (Some(adsr), false) = (clip.grains.adsr, matches!(clip.kind, ClipKind::Pattern(_))) {
                let on = clip_t0;
                let off = (clip_t0 + full_len - adsr.release_secs()).max(on);
                let env = crate::envelope::VoiceEnv { adsr, on, off };
                let voice = clip_voice(ti, ci);
                for (_, evs) in parts.iter_mut().chain(video_parts.iter_mut().flatten()) {
                    for e in evs.iter_mut() {
                        if e.tag.voice == 0 {
                            e.tag.voice = voice;
                        }
                        e.tag.env = Some(env);
                    }
                }
            }
            plans.push(ClipPlan { track: ti, clip_index: ci, parts, video_parts, style });
        }
    }

    apply_choke_groups(&mut plans, project);

    if project.layout.uses_tiles() {
        // Muted tracks aren't heard or drawn, so they get no tile (no
        // holes in the grid; their grains keep `tag.tile == None`).
        // In `Layout::Split`, sustained wash clips (`is_wash`) stay untiled
        // (`tile == None`) so they bleed full-canvas at `30%` screen.
        let muted = |cp: &ClipPlan| project.tracks.get(cp.track).is_some_and(|t| t.muted);
        let is_split = project.layout == crate::tile::Layout::Split;
        assign_tiles_where(&mut plans, project.layout_pan, |cp| {
            if muted(cp) {
                return false;
            }
            if is_split {
                let all_wash = cp
                    .parts
                    .iter()
                    .flat_map(|(_, evs)| evs.iter())
                    .all(|e| e.tag.is_wash)
                    && cp.parts.iter().any(|(_, evs)| !evs.is_empty());
                !all_wash
            } else {
                true
            }
        });
    }
    plans
}

/// Voice id of a whole clip (granular / snippet).
fn clip_voice(track: usize, clip: usize) -> u64 {
    0xC000_0000_0000_0000 | ((track as u64 & 0xFFFF) << 32) | (clip as u64 & 0xFFFF_FFFF)
}

/// Layout `Tile` / `Split`: give every voice in the plan a stable tile slot, in
/// voice-start order, preferring the slot its track used last (the same assignment the realtime engine replays,
/// since it plays this plan). Grains without a voice belong to their
/// clip's voice. With `layout_pan`, the slot's column sets the grain pan
/// (keeping a little of the grain's own spread) and its row a subtle gain
/// cue, from the grid at each grain's onset (the grid the picture shows).
pub fn assign_tiles(plans: &mut [ClipPlan], layout_pan: bool) {
    assign_tiles_where(plans, layout_pan, |_| true);
}

/// [`assign_tiles`] over only the plans `include` accepts (e.g. clips on
/// unmuted tracks); the others get no tile and don't count toward the grid.
pub fn assign_tiles_where(plans: &mut [ClipPlan], layout_pan: bool, include: impl Fn(&ClipPlan) -> bool) {
    use std::collections::BTreeMap;
    // voice -> (start, end, track, pitch_ratio)
    let mut spans: BTreeMap<u64, (f64, f64, usize, f32)> = BTreeMap::new();
    for cp in plans.iter_mut() {
        if !include(cp) {
            continue;
        }
        let fallback = clip_voice(cp.track, cp.clip_index);
        for (_, evs) in cp.parts.iter_mut().chain(cp.video_parts.iter_mut().flatten()) {
            for e in evs.iter_mut() {
                if is_wash_only_in_split(e) {
                    continue;
                }
                if e.tag.voice == 0 {
                    e.tag.voice = fallback;
                }
                let end = e.tag.env.map_or(e.end(), |v| v.end().min(e.end()));
                let s = spans.entry(e.tag.voice).or_insert((e.onset, end, cp.track, e.pitch_ratio));
                s.0 = s.0.min(e.onset);
                s.1 = s.1.max(end);
            }
        }
    }
    let mut order: Vec<(u64, f64, f64, usize, f32)> =
        spans.into_iter().map(|(v, (a, b, t, pr))| (v, a, b, t, pr)).collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut tiles = crate::tile::TileAssigner::new();
    let mut slot_of: BTreeMap<u64, u8> = BTreeMap::new();
    let mut spans: Vec<(u8, f64, f64)> = Vec::with_capacity(order.len());
    // One tile per track (a pad's loop passes) or per pattern lane (step
    // hits, `seq::hit_voice`): the next pass / hit keeps its tile.
    let key_of = |voice: u64, track: usize| {
        let chord_sub = if (voice >> 60) == 0xD { (voice & 0xF) + 1 } else { 0 };
        let lane = if voice >> 60 == 0xA { 1 + ((voice >> 52) & 0xFF) } else { 0 };
        ((track as u64) << 14) | (lane << 5) | chord_sub
    };
    // When the key's previous voice ends (continuations at a shared start
    // claim their slots before new keys pick the lowest free one; simultaneous
    // chord voices sort low -> high pitch left-to-right).
    let mut key_end: BTreeMap<u64, f64> = BTreeMap::new();
    let mut g = 0;
    while g < order.len() {
        let start = order[g].1;
        let mut h = g;
        while h < order.len() && order[h].1 <= start + 1e-9 {
            h += 1;
        }
        let group = &mut order[g..h];
        group.sort_by(|a, b| {
            let cont_a = key_end.get(&key_of(a.0, a.3)).is_some_and(|&e| e >= a.1 - 1e-9);
            let cont_b = key_end.get(&key_of(b.0, b.3)).is_some_and(|&e| e >= b.1 - 1e-9);
            (!cont_a)
                .cmp(&(!cont_b))
                .then_with(|| a.4.total_cmp(&b.4))
                .then_with(|| a.0.cmp(&b.0))
        });
        for &(voice, start, end, track, _) in group.iter() {
            let key = key_of(voice, track);
            let slot = tiles.assign_keyed(voice, key, start);
            tiles.release(voice, end);
            slot_of.insert(voice, slot);
            spans.push((slot, start, end));
            let e = key_end.entry(key).or_insert(end);
            *e = e.max(end);
        }
        g = h;
    }
    let grids = crate::tile::grid_timeline(&spans);
    for cp in plans.iter_mut() {
        if !include(cp) {
            continue;
        }
        for (_, evs) in cp.parts.iter_mut().chain(cp.video_parts.iter_mut().flatten()) {
            for e in evs.iter_mut() {
                if let Some(&slot) = slot_of.get(&e.tag.voice) {
                    e.tag.tile = Some(slot);
                    if layout_pan && !e.tag.is_wash {
                        let (cols, rows) = crate::tile::grid_in(&grids, e.onset);
                        let (pan, gain) = crate::tile::layout_pan(slot as usize, cols, rows);
                        e.pan = (pan + 0.2 * e.pan).clamp(-1.0, 1.0);
                        e.gain *= gain;
                    }
                }
            }
        }
    }
}

#[inline]
fn is_wash_only_in_split(_e: &GrainEvent) -> bool {
    false
}

/// Stage-aware `Layout::Tile` grid timeline: uses *MODELL 5* vertical portrait columns
/// (`2×1` / `3×1` / `4×1` on wide `aspect > 1.3`) when `portrait_columns` is set OR
/// when active grains on that frame carry `hard_burst`.
pub fn frame_tile_grids_stage(
    plans: &[ClipPlan],
    t0: f64,
    fps: f32,
    n_frames: usize,
    aspect: f32,
    portrait_columns: bool,
) -> Vec<(u32, u32)> {
    let slots = collect_frame_panel_slots(plans, t0, fps, n_frames, false);
    if aspect <= 1.3 {
        return slots.into_iter().map(crate::tile::frame_grid).collect();
    }
    let fps_f = fps.max(1e-3) as f64;
    let mut burst_frame = vec![portrait_columns; n_frames];
    if !portrait_columns {
        for cp in plans {
            for (_, evs) in &cp.parts {
                for e in evs.iter().filter(|e| e.tag.hard_burst) {
                    let f0 = ((e.onset - t0) * fps_f).floor().max(0.0) as usize;
                    let f1 = (((e.end() - t0) * fps_f).ceil().max(0.0) as usize).min(n_frames);
                    for bf in burst_frame.iter_mut().take(f1).skip(f0) {
                        *bf = true;
                    }
                }
            }
        }
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(fi, s)| {
            if burst_frame[fi] {
                crate::tile::frame_portrait_grid(s, aspect)
            } else {
                crate::tile::frame_grid(s)
            }
        })
        .collect()
}

/// Tile grid for each frame (layout `Tile`): from the tile slots of every
/// grain visible in it, across all clips (see [`crate::tile::frame_grid`]).
pub fn frame_tile_grids(plans: &[ClipPlan], t0: f64, fps: f32, n_frames: usize) -> Vec<(u32, u32)> {
    collect_frame_panel_slots(plans, t0, fps, n_frames, false)
        .into_iter()
        .map(crate::tile::frame_grid)
        .collect()
}

/// Architectural split grid `(cols, rows)` for each frame in [`crate::tile::Layout::Split`],
/// ignoring full-canvas wash layers (`is_wash == true`) and holding `52 ms` optical carry
/// on short percussive cuts.
pub fn frame_split_grids(
    plans: &[ClipPlan],
    t0: f64,
    fps: f32,
    n_frames: usize,
    aspect: f32,
) -> Vec<(u32, u32)> {
    collect_frame_panel_slots(plans, t0, fps, n_frames, true)
        .into_iter()
        .map(|s| crate::tile::frame_split_grid(s, aspect))
        .collect()
}

fn collect_frame_panel_slots(
    plans: &[ClipPlan],
    t0: f64,
    fps: f32,
    n_frames: usize,
    skip_wash: bool,
) -> Vec<Vec<u8>> {
    let mut slots: Vec<Vec<u8>> = vec![Vec::new(); n_frames];
    let fps = fps.max(1e-3) as f64;
    for cp in plans {
        let vparts = cp.video_parts.as_deref().unwrap_or(&cp.parts);
        for (_, evs) in vparts {
            for e in evs {
                if skip_wash && e.tag.is_wash {
                    continue;
                }
                let Some(slot) = e.tag.tile else { continue };
                let carry = if !e.tag.hard_burst
                    && e.tag.env.is_none()
                    && e.tag.choke_at.is_none()
                    && e.duration > 0.085
                    && e.duration < 0.35
                {
                    OPTICAL_CARRY_SECS
                } else {
                    0.0
                };
                let vis_end = e.end() + carry;
                let f0 = ((e.onset - t0) * fps).ceil().max(0.0) as usize;
                let f1 = (((vis_end - t0) * fps).ceil().max(0.0) as usize).min(n_frames);
                for (fi, list) in slots.iter_mut().enumerate().take(f1).skip(f0) {
                    let ft = t0 + fi as f64 / fps;
                    if ft >= e.onset && ft < vis_end && e.tag.picture_at(ft) > 0.0 && !list.contains(&slot) {
                        list.push(slot);
                    }
                }
            }
        }
    }
    slots
}

/// The clip transform's audio placement: x -> pan offset, scale ->
/// distance loudness, y -> spectral tilt (screen-up = brighter), each
/// through its AV-link dial.
pub fn clip_spatial(clip: &Clip) -> (f32, f32, f32) {
    let pan_offset = (clip.transform.x * clip.link.x_to_pan).clamp(-1.0, 1.0);
    let gain_mult =
        (1.0 + (clip.transform.scale.clamp(0.0, 1.5) - 1.0) * clip.link.scale_to_gain).max(0.0);
    // Screen y is down-positive; elevation up should brighten.
    let tilt = (-clip.transform.y * clip.link.y_to_brightness).clamp(-1.0, 1.0);
    (pan_offset, gain_mult, tilt)
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
    let (pan_offset, gain_mult, _) = clip_spatial(clip);
    if clip.audio_filters.is_empty() && clip.eq.is_empty() {
        render_grains_audio_spatial(audio_src, events, bus, t0, pan_offset, gain_mult);
    } else {
        let filtered = audio_src.filtered_and_eq(&clip.audio_filters, &clip.eq);
        render_grains_audio_spatial(&filtered, events, bus, t0, pan_offset, gain_mult);
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
    let aspect = project.width as f32 / project.height.max(1) as f32;
    let panel_grids: Option<Vec<(u32, u32)>> = match project.layout {
        crate::tile::Layout::Tile => Some(frame_tile_grids_stage(
            &plan,
            t0,
            project.fps,
            n_frames,
            aspect,
            project.portrait_columns,
        )),
        crate::tile::Layout::Split => Some(frame_split_grids(&plan, t0, project.fps, n_frames, aspect)),
        crate::tile::Layout::Stack | crate::tile::Layout::Bleed => None,
    };
    let mut frames: Vec<Frame> =
        (0..n_frames).map(|_| Frame::black(project.width, project.height)).collect();

    let n_tracks = project.tracks.len();
    let mut track_buses: Vec<Option<StereoBuffer>> = vec![None; n_tracks];

    // ---- Pass 1 (audio): clips -> clip fx -> track bus -> track fx -> tape -> level ----
    for (ti, track) in project.tracks.iter().enumerate() {
        if track.muted {
            continue;
        }
        let clip_plans: Vec<&ClipPlan> = plan.iter().filter(|p| p.track == ti).collect();
        if clip_plans.is_empty() && track.effects.is_empty() && !track.tape.is_active() && track.eq.is_empty() {
            continue;
        }

        let mut track_bus = StereoBuffer::new(len, project.sample_rate);
        for cp in &clip_plans {
            let clip = &track.clips[cp.clip_index];
            let (_, _, tilt) = clip_spatial(clip);
            let needs_bus = !clip.effects.is_empty() || tilt.abs() > 1e-4;
            if !needs_bus {
                for (sid, evs) in &cp.parts {
                    render_audio_part(project, clip, *sid, evs, &mut track_bus, t0);
                }
            } else {
                let mut clip_bus = StereoBuffer::new(len, project.sample_rate);
                for (sid, evs) in &cp.parts {
                    render_audio_part(project, clip, *sid, evs, &mut clip_bus, t0);
                }
                fx::apply_audio_chain(&mut clip_bus, &clip.effects);
                let mut tilt_state = crate::dsp::TiltState::default();
                crate::dsp::apply_tilt(
                    &mut clip_bus.left,
                    &mut clip_bus.right,
                    &mut tilt_state,
                    tilt,
                    project.sample_rate,
                );
                for i in 0..track_bus.len().min(clip_bus.len()) {
                    track_bus.left[i] += clip_bus.left[i];
                    track_bus.right[i] += clip_bus.right[i];
                }
            }
        }
        if !track.eq.is_empty() {
            crate::dsp::apply_eq_chain(&mut track_bus.left, &track.eq, project.sample_rate as f32);
            crate::dsp::apply_eq_chain(&mut track_bus.right, &track.eq, project.sample_rate as f32);
        }
        fx::apply_audio_chain(&mut track_bus, &track.effects);
        if track.tape.is_active() {
            let mut tape_state = TapeState::new(0x5452_4B00 ^ ti as u64);
            let start_samples = (t0 * project.sample_rate as f64).round() as u64;
            let _ = start_samples; // matched at t0 = 0 with RealtimeAudio
            tape_state.process_block(
                &mut track_bus.left,
                &mut track_bus.right,
                &track.tape,
                project.sample_rate,
            );
        }
        let sr = project.sample_rate as f64;
        let beats_per_sec = project.bpm / 60.0;
        for i in 0..track_bus.len() {
            let beat = (t0 + i as f64 / sr) * beats_per_sec;
            let level = track.level_at(beat);
            track_bus.left[i] *= level;
            track_bus.right[i] *= level;
        }
        track_buses[ti] = Some(track_bus);
    }

    // ---- Pass 2 (audio sum + z-independent sidechain & visual ducking) ----
    let mut track_duck_shaped: Vec<Vec<f32>> = vec![Vec::new(); n_tracks];
    for (ti, track) in project.tracks.iter().enumerate() {
        let Some(bus) = &track_buses[ti] else { continue };
        let n_samp = master_audio.len().min(bus.len());
        if let Some(spec) = &track.duck {
            if spec.source_track < n_tracks && spec.source_track != ti {
                let mut dstate = DuckState::default();
                let mut frame_shaped = vec![0.0f32; n_frames];
                let sr = project.sample_rate;
                let sc_opt = track_buses[spec.source_track].as_ref();
                for i in 0..n_samp {
                    let (sl, sr_c) = sc_opt
                        .map(|sc| (sc.left[i], sc.right[i]))
                        .unwrap_or((0.0, 0.0));
                    let (audio_g, shaped) = dstate.step_av(sl, sr_c, spec, sr);
                    master_audio.left[i] += bus.left[i] * audio_g;
                    master_audio.right[i] += bus.right[i] * audio_g;
                    let fi = ((i as f64 / sr as f64) * project.fps as f64) as usize;
                    if fi < n_frames {
                        frame_shaped[fi] = frame_shaped[fi].max(shaped);
                    }
                }
                for fi in 1..frame_shaped.len() {
                    // Smooth downward release across frames so kick-driven visual
                    // pulses decay organically over ~5 frames without 1-frame snap-back.
                    frame_shaped[fi] = frame_shaped[fi].max((frame_shaped[fi - 1] - 0.16).max(frame_shaped[fi - 1] * 0.78));
                }
                track_duck_shaped[ti] = frame_shaped;
                continue;
            }
        }
        for i in 0..n_samp {
            master_audio.left[i] += bus.left[i];
            master_audio.right[i] += bus.right[i];
        }
    }

    // ---- Pass 3 (video): clips -> clip fx -> track layer -> track fx -> z ----
    for (ti, track) in project.tracks.iter().enumerate() {
        if track.muted {
            continue;
        }
        let clip_plans: Vec<&ClipPlan> = plan.iter().filter(|p| p.track == ti).collect();
        if clip_plans.is_empty() {
            continue;
        }
        let track_has_video = clip_plans.iter().any(|cp| {
            let vparts = cp.video_parts.as_deref().unwrap_or(&cp.parts);
            vparts.iter().any(|(sid, evs)| {
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
        let fallback_add = clip_plans
            .iter()
            .map(|cp| cp.style.additive)
            .fold(0.0f32, f32::max);
        let track_rms_env: Vec<f32> = if track.rms_to_opacity > 1e-4 {
            if let Some(bus) = &track_buses[ti] {
                let mut out_env = vec![0.0f32; n_frames];
                let sr = project.sample_rate.max(1) as f32;
                let att_c = (-1.0 / (0.024 * sr)).exp();
                let rel_c = (-1.0 / (0.175 * sr)).exp();
                let mut env = 0.0f32;
                for i in 0..bus.len() {
                    let x = bus.left[i].abs().max(bus.right[i].abs());
                    if x > env {
                        env = att_c * env + (1.0 - att_c) * x;
                    } else {
                        env = rel_c * env + (1.0 - rel_c) * x;
                    }
                    let fi = ((i as f64 / sr as f64) * project.fps as f64) as usize;
                    if fi < n_frames {
                        let d = (env * 4.2).min(6.0);
                        out_env[fi] = out_env[fi].max(d / (1.0 + d));
                    }
                }
                out_env
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        for (fi, frame) in frames.iter_mut().enumerate() {
            let ft = t0 + fi as f64 / project.fps as f64;
            let mut track_layer: Option<Frame> = None;
            let mut active_add: Option<f32> = None;
            let mut track_is_wash = track.sculpt.is_wash();

            for (cpi, cp) in clip_plans.iter().enumerate() {
                let clip = &track.clips[cp.clip_index];
                let vparts = cp.video_parts.as_deref().unwrap_or(&cp.parts);
                let is_stretched_snippet = cp.video_parts.is_some();
                let carry_pad = if project.layout.uses_tiles() { OPTICAL_CARRY_SECS } else { 0.0 };
                let any_active = vparts.iter().any(|(_, evs)| {
                    evs.iter().any(|e| {
                        ft >= e.onset && ft < e.onset + cp.style.visual_duration(e.duration).max(e.duration as f64 + carry_pad)
                    })
                });
                if !any_active && clip.effects.is_empty() {
                    continue;
                }
                let mut clip_layer = Frame::transparent(project.width, project.height);
                if any_active {
                    active_add = Some(active_add.unwrap_or(0.0).max(cp.style.additive));
                    if vparts.iter().any(|(_, evs)| evs.iter().any(|e| e.tag.is_wash && ft >= e.onset && ft < e.end())) {
                        track_is_wash = true;
                    }
                    let hue_override = if is_stretched_snippet {
                        let bh = project
                            .sources
                            .get(clip.source)
                            .map(|s| s.effective_base_hz())
                            .unwrap_or(440.0);
                        let br = crate::music::effective_ratio(
                            clip.grains.pitch,
                            clip.key.as_ref(),
                            bh,
                            clip.grains.quantize_amount,
                            clip.grains.detune_cents,
                        );
                        Some(crate::music::ratio_to_semitones(br) * clip.link.pitch_to_hue)
                    } else {
                        None
                    };
                    let grid_fi = panel_grids.as_ref().map(|g| g[fi]);
                    for (sid, evs) in vparts {
                        if let Some(video) =
                            project.sources.get(*sid).and_then(|s| s.video.as_ref())
                        {
                            composite_grains_frame_stage(
                                video,
                                evs,
                                &cp.style,
                                &clip.link,
                                &clip.transform,
                                clip.color_filter.as_ref(),
                                &mut clip_layer,
                                ft,
                                is_stretched_snippet,
                                hue_override,
                                project.layout,
                                grid_fi,
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
            if track.tape.is_active() {
                apply_video_tape(&mut tl, &track.tape, fi, project.fps, 0x5452_4B00 ^ ti as u64);
            }
            let mut opacity =
                track.opacity_at((t0 + fi as f64 / project.fps as f64) * project.bpm / 60.0);
            if track.rms_to_opacity > 1e-4 {
                let r_amt = track.rms_to_opacity.clamp(0.0, 1.0);
                let env_v = track_rms_env.get(fi).copied().unwrap_or(0.0);
                opacity *= ((1.0 - r_amt) + r_amt * env_v).clamp(0.0, 1.0);
            }
            if let (Some(&shaped), Some(spec)) = (track_duck_shaped[ti].get(fi), &track.duck) {
                let raw_link = track.duck_to_opacity.clamp(-1.0, 1.0);
                let v_amt = if spec.amount <= 1e-4 {
                    raw_link
                } else {
                    spec.amount.clamp(0.0, 1.0) * raw_link
                };
                if v_amt >= 0.0 {
                    opacity *= (1.0 - v_amt * shaped).clamp(0.0, 1.0);
                } else {
                    let pulse = (-v_amt).clamp(0.0, 1.0);
                    opacity = (opacity * (1.0 - 0.55 * pulse) + pulse * shaped).clamp(0.0, 1.0);
                }
            }
            let track_add = track
                .additive
                .unwrap_or_else(|| active_add.unwrap_or(fallback_add));
            blend_stage_layer(frame, &tl, project.layout, track_add, opacity, track_is_wash);
        }
    }

    // ---- master chain ----
    if project.studio_bus {
        let mut studio_bus = StudioBusState::new(project.sample_rate);
        studio_bus.process_eq(&mut master_audio.left, &mut master_audio.right);
    }
    fx::apply_audio_chain_opts(&mut master_audio, &project.master_effects, project.studio_bus);
    if project.master_tape.is_active() {
        let mut master_tape = TapeState::new(0x4D41_5354);
        master_tape.process_block(
            &mut master_audio.left,
            &mut master_audio.right,
            &project.master_tape,
            project.sample_rate,
        );
    }
    if project.master_gain_db.abs() > 1e-4 {
        let g = 10.0f32.powf(project.master_gain_db.clamp(-24.0, 24.0) / 20.0);
        for s in &mut master_audio.left {
            *s *= g;
        }
        for s in &mut master_audio.right {
            *s *= g;
        }
    }
    if project.isolator.is_active() {
        let mut iso_state = crate::dsp::IsolatorState::new(project.sample_rate);
        iso_state.process_block(
            &mut master_audio.left,
            &mut master_audio.right,
            &project.isolator,
            project.sample_rate,
        );
    }
    let bloom_luma: Vec<f32> = if project.master_bloom > 1e-4 && n_frames > 0 && !master_audio.left.is_empty() {
        let b_amt = project.master_bloom.clamp(0.0, 1.0);
        let spf = ((project.sample_rate as f64 / project.fps.max(1.0) as f64).round() as usize).max(1);
        let mut log_e = vec![0.0f32; n_frames];
        for (fi, le) in log_e.iter_mut().enumerate() {
            let s0 = fi * spf;
            let s1 = ((fi + 1) * spf).min(master_audio.left.len());
            if s1 > s0 {
                let mut sum = 0.0f64;
                for i in s0..s1 {
                    let m = 0.5 * (master_audio.left[i] + master_audio.right[i]) as f64;
                    sum += m * m;
                }
                let rms = (sum / (s1 - s0) as f64).sqrt() as f32;
                *le = (rms + 1e-4).ln();
            } else {
                *le = 1e-4f32.ln();
            }
        }
        let mut out_b = vec![0.0f32; n_frames];
        let mut cur = 0.0f32;
        for fi in 0..n_frames {
            let flux = if fi > 0 { (log_e[fi] - log_e[fi - 1]).max(0.0) } else { 0.0 };
            let target = (flux * 6.8 * b_amt).min(6.8 * b_amt);
            // Fast onset rise on frame `fi` (driving onset<->frame_diff correlation)
            // + slew-limited smooth decay (`<= 1.40` luma/frame -> zero 1-frame reversals).
            cur = target.max((cur - 1.40 * b_amt).max(0.0));
            out_b[fi] = cur;
        }
        out_b
    } else {
        Vec::new()
    };
    let mut master_states = fx::video_chain_states(&project.master_effects, project.fps);
    for (fi, frame) in frames.iter_mut().enumerate() {
        let ft = t0 + fi as f64 / project.fps as f64;
        fx::apply_video_chain(frame, &project.master_effects, &mut master_states);
        if project.master_tape.is_active() {
            apply_video_tape(frame, &project.master_tape, fi, project.fps, 0x4D41_5354);
        }
        if project.isolator.is_active() {
            fx::apply_video_isolator(frame, &project.isolator, 1.0);
        }
        if let Some(&bl) = bloom_luma.get(fi) {
            if bl > 0.02 {
                let mult = 1.0 + bl * 0.036;
                let lift = bl * 1.10;
                for px in frame.data.chunks_exact_mut(4) {
                    px[0] = (px[0] as f32 * mult + lift).clamp(0.0, 255.0) as u8;
                    px[1] = (px[1] as f32 * mult + lift * 0.94).clamp(0.0, 255.0) as u8;
                    px[2] = (px[2] as f32 * mult + lift * 0.86).clamp(0.0, 255.0) as u8;
                }
            }
        }
        if project.captions_on {
            let mut best_cap: Option<(&str, f64, f32)> = None;
            for cp in &plan {
                if project.tracks.get(cp.track).is_some_and(|t| t.muted) {
                    continue;
                }
                for (_, evs) in &cp.parts {
                    for e in evs {
                        if !e.tag.caption.is_empty() && ft >= e.onset && ft < e.end() {
                            let pic = e.tag.picture_at(ft);
                            if pic > 0.05 && best_cap.is_none_or(|(_, prev_on, _)| e.onset >= prev_on) {
                                best_cap = Some((&e.tag.caption, e.onset, pic));
                            }
                        }
                    }
                }
            }
            if let Some((cap, _, pic)) = best_cap {
                draw_stage_caption(frame, cap, pic);
            }
        }
    }

    if project.studio_bus {
        StudioBusState::process_ceiling(&mut master_audio.left, &mut master_audio.right);
    } else {
        master_audio.soft_clip();
    }
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
        let mut p = Project {
            bpm: 120.0,
            width: 96,
            height: 54,
            fps: 12.0,
            studio_bus: false,
            ..Default::default()
        };
        let src = crate::timeline::Source {
            name: "src".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(220.0, 4.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(96, 54, 12.0, 4.0))),
            base_hz: 220.0,
            ..Default::default()
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
    fn repeating_clip_tiles_its_first_pass() {
        let mut p = snippet_project();
        // 1 beat pass (0.5 s at 120 bpm), played 4 times.
        let c = &mut p.tracks[0].clips[0];
        c.length_beats = 1.0;
        c.set_repeat(Some(4));
        assert_eq!(c.length_beats, 4.0);
        assert!((c.repeat_count() - 4.0).abs() < 1e-9);
        let plan = plan_project(&p);
        let evs = &plan[0].parts[0].1;
        assert_eq!(evs.len(), 4, "one snippet grain per pass");
        for (k, e) in evs.iter().enumerate() {
            assert!((e.onset - 0.5 * k as f64).abs() < 1e-9);
            assert!((e.source_pos - evs[0].source_pos).abs() < 1e-9, "every pass restarts the media");
        }
        // Audio: pass 2 is sample-identical to pass 1.
        let out = render_project(&p, 0.0, 2.0);
        let n = (0.5 * p.sample_rate as f64) as usize;
        let d: f32 = (100..n - 100).map(|i| (out.audio.left[i] - out.audio.left[i + n]).abs()).sum();
        assert!(d < 1e-3, "passes differ by {d}");
        // Off again: back to one pass.
        let c = &mut p.tracks[0].clips[0];
        c.clear_repeat();
        assert_eq!(c.length_beats, 1.0);
        assert!(!c.is_looping());
    }

    #[test]
    fn forever_runs_to_the_next_clip_or_past_the_song() {
        let mut p = snippet_project();
        let sid = 0;
        p.tracks[0].clips[0].length_beats = 2.0;
        p.tracks[0].clips[0].set_repeat(None);
        p.tracks[0].clips.push(Clip::new_snippet(sid, 10.0, 2.0));
        assert!(p.resolve_forever());
        assert_eq!(p.tracks[0].clips[0].length_beats, 10.0, "stops at the next clip");
        assert_eq!(p.tracks[0].clips[0].pass_beats(), 2.0);
        assert!(!p.resolve_forever(), "idempotent");
        // Nothing after it: runs FOREVER_TAIL_BEATS past the song's end.
        p.tracks[0].clips.pop();
        let t = p.add_track("other");
        p.tracks[t].clips.push(Clip::new_snippet(sid, 0.0, 16.0));
        p.resolve_forever();
        assert_eq!(p.tracks[0].clips[0].length_beats, 16.0 + crate::timeline::FOREVER_TAIL_BEATS);
    }

    #[test]
    fn repeat_gap_mask_and_advance_shape_the_passes() {
        let mut p = snippet_project();
        let c = &mut p.tracks[0].clips[0];
        c.length_beats = 1.0;
        c.set_repeat(Some(1));
        c.loop_gap = 1.0; // pass 1 beat, rest 1 beat
        c.loop_mask = vec![true, false, true];
        c.loop_advance = 1.0;
        let len = c.length_for(4);
        c.length_beats = len;
        assert_eq!(len, 7.0, "3 periods of 2 + one pass");
        assert!((c.repeat_count() - 4.0).abs() < 1e-9);
        let plan = plan_project(&p);
        let evs = &plan[0].parts[0].1;
        // Passes 0,1,2,3 at 0,1,2,3 s; mask drops pass 1.
        let onsets: Vec<f64> = evs.iter().map(|e| e.onset).collect();
        assert_eq!(onsets, vec![0.0, 2.0, 3.0]);
        // Advance 1: pass k starts k * 0.5 s further into the media.
        for e in evs {
            let k = (e.onset / 1.0).round();
            assert!((e.source_pos - k * 0.5).abs() < 1e-9, "pos {} at {}", e.source_pos, e.onset);
        }
        // Negative gap overlaps passes and still counts right.
        let c = &mut p.tracks[0].clips[0];
        c.loop_gap = -0.5;
        c.length_beats = c.length_for(3);
        assert!((c.length_beats - 2.0).abs() < 1e-9);
        assert!((c.repeat_count() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn texture_morphs_a_snippet_into_grains_at_level_loudness() {
        let render_tex = |tex: f32| {
            let mut p = snippet_project();
            p.tracks[0].clips[0].grains.texture = tex;
            let plan = plan_project(&p);
            let n = plan[0].parts[0].1.len();
            let out = render_project(&p, 0.5, 3.5);
            (n, out.audio.rms())
        };
        let (n0, r0) = render_tex(0.0);
        let (n1, r1) = render_tex(0.3);
        let (n2, r2) = render_tex(1.0);
        assert_eq!(n0, 1, "texture 0 is the pristine single grain");
        assert!(n1 > 5 && n2 > n1, "grain counts {n0} {n1} {n2}");
        for (r, name) in [(r1, "0.3"), (r2, "1.0")] {
            let db = 20.0 * (r / r0).log10();
            assert!(db.abs() < 4.0, "texture {name} loudness {db:.1} dB");
        }
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
            ..Default::default()
        });
        let blue = p.add_source(crate::timeline::Source {
            name: "blue".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(330.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(30, 30, 220))),
            base_hz: 330.0,
            ..Default::default()
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
    fn snippet_speed_changes_tempo_not_pitch() {
        // Half speed: the 220 Hz source still sounds at 220 Hz (granular
        // time-stretch), NOT chipmunked down to 110.
        let mut p = snippet_project();
        p.tracks[0].clips[0].grains.speed = 0.5;
        let out = render_project(&p, 0.0, 4.0);
        let mono: Vec<f32> =
            out.audio.left.iter().zip(&out.audio.right).map(|(l, r)| l + r).collect();
        let pitch = crate::dsp::detect_pitch(&mono, p.sample_rate).expect("pitch");
        assert!(
            (pitch - 220.0).abs() < 10.0,
            "half speed keeps pitch: got {pitch}"
        );
        // And the source is consumed at half rate: the read heads of late
        // events sit near t/2.
        let evs = &out.events[0];
        assert!(evs.len() > 4, "stretch train, not one grain");
        let late = evs.iter().find(|e| (e.onset - 1.5).abs() < 0.08).unwrap();
        assert!(
            (late.source_pos - 0.75).abs() < 0.08,
            "read head at half rate: {} at onset {}",
            late.source_pos,
            late.onset
        );

        // Separate pitch param: speed 1, pitch +12 -> octave up, full tempo.
        let mut p2 = snippet_project();
        p2.tracks[0].clips[0].grains.pitch = 12.0;
        let out2 = render_project(&p2, 0.0, 4.0);
        let mono2: Vec<f32> =
            out2.audio.left.iter().zip(&out2.audio.right).map(|(l, r)| l + r).collect();
        let pitch2 = crate::dsp::detect_pitch(&mono2, p2.sample_rate).expect("pitch");
        assert!((pitch2 - 440.0).abs() < 20.0, "pitch shift alone: {pitch2}");

        // Default stays the pristine single grain.
        let out3 = render_project(&snippet_project(), 0.0, 4.0);
        assert_eq!(out3.events[0].len(), 1, "speed==pitch ratio -> one grain");
    }

    fn bright_project() -> Project {
        let mut p = Project { bpm: 120.0, width: 64, height: 36, fps: 12.0, ..Default::default() };
        let mut f = crate::video::Frame::black(64, 36);
        for px in f.data.chunks_exact_mut(4) {
            px[0] = 230;
            px[1] = 230;
            px[2] = 230;
        }
        let sid = p.add_source(crate::timeline::Source {
            name: "bright".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(
                220.0, 4.0, p.sample_rate,
            ))),
            video: Some(std::sync::Arc::new(crate::video::VideoClip {
                frames: vec![f; 24],
                fps: 12.0,
            })),
            base_hz: 220.0,
            ..Default::default()
        });
        let t = p.add_track("t");
        p.tracks[t].clips.push(Clip::new_snippet(sid, 0.0, 4.0));
        p
    }

    fn half_lumas(frame: &Frame) -> (f32, f32) {
        // (left half, right half)
        let (w, h) = (frame.width, frame.height);
        let mut l = 0.0f64;
        let mut r = 0.0f64;
        for y in 0..h {
            for x in 0..w {
                let px = frame.get(x, y);
                let luma = 0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64;
                if x < w / 2 {
                    l += luma;
                } else {
                    r += luma;
                }
            }
        }
        let n = (w * h / 2) as f64;
        ((l / n) as f32, (r / n) as f32)
    }

    #[test]
    fn transform_positions_scales_and_rotates() {
        // Scale 0.5 + x offset right: bright content lands on the right.
        let mut p = bright_project();
        p.tracks[0].clips[0].transform.scale = 0.5;
        p.tracks[0].clips[0].transform.x = 0.5;
        let out = render_project(&p, 0.0, 4.0);
        let mid = &out.frames[out.frames.len() / 2];
        let (l, r) = half_lumas(mid);
        assert!(r > l * 2.0 + 5.0, "moved right: left {l} right {r}");

        // Scale shrinks coverage: full-scale frame is much brighter overall.
        let full = render_project(&bright_project(), 0.0, 4.0);
        let full_mid = &full.frames[full.frames.len() / 2];
        assert!(
            mid.mean_luma() < full_mid.mean_luma() * 0.55,
            "half scale covers ~quarter area: {} vs {}",
            mid.mean_luma(),
            full_mid.mean_luma()
        );

        // Rotation: make the top half bright, bottom dark; rotate 90 deg
        // -> the asymmetry flips to left/right.
        let mut pr = bright_project();
        {
            let src = &pr.sources[0];
            let mut frames = Vec::new();
            for _ in 0..24 {
                let mut f = crate::video::Frame::black(64, 36);
                for y in 0..18u32 {
                    for x in 0..64u32 {
                        f.put(x, y, [230, 230, 230, 255]);
                    }
                }
                frames.push(f);
            }
            let _ = src;
            pr.sources[0].video =
                Some(std::sync::Arc::new(crate::video::VideoClip { frames, fps: 12.0 }));
        }
        pr.tracks[0].clips[0].transform.scale = 0.6;
        pr.tracks[0].clips[0].transform.rotation = 90.0;
        let out_r = render_project(&pr, 0.0, 4.0);
        let mid_r = &out_r.frames[out_r.frames.len() / 2];
        let (rl, rr) = half_lumas(mid_r);
        assert!(
            (rl - rr).abs() > 4.0,
            "rotated top-bright becomes side-bright: {rl} vs {rr}"
        );
    }

    #[test]
    fn canvas_x_drives_pan_and_scale_drives_gain() {
        // x = +0.9: audio pans hard right.
        let mut p = bright_project();
        p.tracks[0].clips[0].transform.x = 0.9;
        let out = render_project(&p, 0.0, 4.0);
        let rms = |ch: &Vec<f32>| -> f32 {
            (ch.iter().map(|s| s * s).sum::<f32>() / ch.len() as f32).sqrt()
        };
        assert!(
            rms(&out.audio.right) > rms(&out.audio.left) * 2.0,
            "panned right: L {} R {}",
            rms(&out.audio.left),
            rms(&out.audio.right)
        );

        // Unlink x_to_pan: pan stays centered despite the offset.
        let mut pu = bright_project();
        pu.tracks[0].clips[0].transform.x = 0.9;
        pu.tracks[0].clips[0].link.x_to_pan = 0.0;
        let out_u = render_project(&pu, 0.0, 4.0);
        let (ul, ur) = (rms(&out_u.audio.left), rms(&out_u.audio.right));
        assert!((ul - ur).abs() < ul * 0.1, "unlinked stays centered");

        // Small scale = far away = quiet (the 3D distance cue).
        let mut ps = bright_project();
        ps.tracks[0].clips[0].transform.scale = 0.25;
        let out_s = render_project(&ps, 0.0, 4.0);
        assert!(
            out_s.audio.rms() < out.audio.rms() * 0.5,
            "scale ducks loudness: {} vs {}",
            out_s.audio.rms(),
            out.audio.rms()
        );
        // Unlink scale_to_gain: loudness restored.
        let mut psu = bright_project();
        psu.tracks[0].clips[0].transform.scale = 0.25;
        psu.tracks[0].clips[0].link.scale_to_gain = 0.0;
        let out_su = render_project(&psu, 0.0, 4.0);
        assert!(out_su.audio.rms() > out_s.audio.rms() * 1.8);
    }

    #[test]
    fn canvas_y_drives_brightness_tilt() {
        // A harmonically rich source; move it up vs down the canvas.
        let make = |y: f32, link: f32| -> f32 {
            let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 12.0, ..Default::default() };
            let sid = p.add_source(crate::timeline::Source {
                name: "saw".into(),
                audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(
                    110.0, 3.0, p.sample_rate,
                ))),
                video: None,
                base_hz: 110.0,
                ..Default::default()
            });
            let t = p.add_track("t");
            let mut c = Clip::new_snippet(sid, 0.0, 4.0);
            c.transform.y = y;
            c.link.y_to_brightness = link;
            p.tracks[t].clips.push(c);
            let out = render_project(&p, 0.0, 4.0);
            // High-frequency ratio: rms of first difference / rms.
            let sr = p.sample_rate as usize;
            let seg = &out.audio.left[sr / 2..sr * 3 / 2];
            let rms = (seg.iter().map(|x| x * x).sum::<f32>() / seg.len() as f32).sqrt();
            let d = (seg.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>()
                / (seg.len() - 1) as f32)
                .sqrt();
            d / rms.max(1e-9)
        };
        let up = make(-0.8, 1.0); // screen-up
        let mid = make(0.0, 1.0);
        let down = make(0.8, 1.0);
        assert!(up > mid * 1.15, "up is brighter: {up} vs {mid}");
        assert!(down < mid * 0.9, "down is darker: {down} vs {mid}");
        // Unlinked: y has no audible effect.
        let unlinked = make(0.8, 0.0);
        assert!((unlinked - mid).abs() < mid * 0.05, "unlinked: {unlinked} vs {mid}");
    }

    #[test]
    fn synth_sources_flow_through_the_whole_engine() {
        // An FM synth source sequenced by a pattern with a tint on the
        // clip chain: synths get sequencing + effects like any sample.
        let mut p = Project { bpm: 120.0, width: 48, height: 32, fps: 12.0, ..Default::default() };
        let fm = p.add_source(crate::synth::fm_source(220.0, 2.0, 1.0, 2.0, p.sample_rate));
        let mut pat = crate::seq::StepPattern::new("fm hits");
        let r = pat.add_row(fm);
        pat.rows[r].grains.duration = 0.3;
        for i in (0..16).step_by(4) {
            pat.rows[r].steps[i].on = true;
        }
        let pid = p.add_pattern(pat);
        let t = p.add_track("synth");
        let mut clip = Clip::new_pattern(pid, 0.0, 4.0);
        clip.effects.push(crate::fx::AvEffect::new(crate::fx::EffectKind::Tint {
            hue: 10.0,
            amount: 0.7,
        }));
        p.tracks[t].clips.push(clip);

        let out = render_project(&p, 0.0, 4.0);
        assert!(out.audio.rms() > 0.005, "sequenced synth sounds: {}", out.audio.rms());
        // The scope video flows through and the tint reddens it.
        let mid = &out.frames[7];
        let mut r_sum = 0u64;
        let mut b_sum = 0u64;
        for px in mid.data.chunks_exact(4) {
            r_sum += px[0] as u64;
            b_sum += px[2] as u64;
        }
        assert!(mid.mean_luma() > 0.5, "synth video visible");
        assert!(r_sum > b_sum, "tinted toward red: r {r_sum} b {b_sum}");
    }

    #[test]
    fn pattern_clip_renders_beat_quantized_hits() {
        let mut p = Project { bpm: 120.0, width: 64, height: 36, fps: 12.0, ..Default::default() };
        let sid = p.add_source(crate::timeline::Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(440.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(64, 36, 12.0, 2.0))),
            base_hz: 440.0,
            ..Default::default()
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

    #[test]
    fn stretched_snippet_video_has_zero_hop_strobe() {
        // Regression test for Strobe A: a uniform-brightness video snippet
        // played at speed = 0.65 (time-stretched) must maintain flat luma
        // across frames instead of rippling 1.0 <-> 0.75 at the OLA hop rate.
        let mut p = bright_project();
        p.fps = 24.0;
        p.tracks[0].clips[0].grains.speed = 0.65;
        p.tracks[0].clips[0].grains.envelope = 0.02;
        let out = render_project(&p, 0.0, 4.0);
        // Audio uses the multi-grain stretch train...
        assert!(out.events[0].len() > 8);
        // ...while interior video frames stay rock-steady (zero frame-to-frame
        // luma ripple).
        let interior = &out.frames[6..out.frames.len() - 6];
        for w in interior.windows(2) {
            let dl = (w[1].mean_luma() - w[0].mean_luma()).abs();
            assert!(dl < 0.5, "stretched snippet strobed by {dl} luma");
        }
    }

    #[test]
    fn varispeed_snippet_couples_speed_to_pitch_as_single_grain() {
        let mut p = snippet_project();
        p.tracks[0].clips[0].grains.pitch = -5.0;
        p.tracks[0].clips[0].grains.varispeed = true;
        let out = render_project(&p, 0.0, 4.0);
        assert_eq!(out.events[0].len(), 1, "varispeed plays as one unbroken grain");
    }

    #[test]
    fn sidechain_ducking_works_across_any_track_order() {
        // Track 0 (background pad) ducks from Track 1 (foreground kick),
        // verifying that sidechain order is independent of video z-order.
        let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 24.0, ..Default::default() };
        let pad_src = p.add_source(crate::timeline::Source {
            name: "pad".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(220.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(200, 200, 200))),
            base_hz: 220.0,
            ..Default::default()
        });
        let kick_src = p.add_source(crate::timeline::Source {
            name: "kick".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(60.0, 0.5, p.sample_rate))),
            video: None,
            base_hz: 60.0,
            ..Default::default()
        });
        let t_pad = p.add_track("pad");
        let mut c_pad = Clip::new_snippet(pad_src, 0.0, 4.0);
        c_pad.grains.envelope = 0.01;
        c_pad.grains.gain = 0.5;
        p.tracks[t_pad].clips.push(c_pad);

        let t_kick = p.add_track("kick");
        let mut c_kick = Clip::new_snippet(kick_src, 1.0, 0.5); // kick hits at beat 1 (t = 0.5s)
        c_kick.grains.gain = 1.0;
        p.tracks[t_kick].clips.push(c_kick);
        p.tracks[t_kick].level = 0.0; // mute kick in master? No, level=0 mutes sc too; mute kick output? Keep audible on R, pan pad L!
        p.tracks[t_kick].level = 1.0;
        p.tracks[t_pad].clips[0].grains.pan = -1.0;
        p.tracks[t_kick].clips[0].grains.pan = 1.0;

        let unducked = render_project(&p, 0.0, 4.0);
        p.tracks[t_pad].duck = Some(crate::timeline::DuckSpec {
            source_track: t_kick,
            amount: 0.85,
            attack: 0.005,
            release: 0.15,
            key_hz: 165.0,
        });
        p.tracks[t_pad].duck_to_opacity = 0.8;
        let ducked = render_project(&p, 0.0, 4.0);

        // Pad is hard-left, kick is hard-right: left channel at t=0.55..0.70s is pure pad.
        let sr = p.sample_rate as usize;
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let u_on = rms(&unducked.audio.left[sr / 2 + 1000..sr * 3 / 4]);
        let d_on = rms(&ducked.audio.left[sr / 2 + 1000..sr * 3 / 4]);
        assert!(d_on < u_on * 0.55, "pad ducked on kick hit: {d_on} vs {u_on}");

        // Video luma also dips during the kick at t ~ 0.6s (frame 14).
        assert!(ducked.frames[14].mean_luma() < unducked.frames[14].mean_luma() * 0.75);
    }

    #[test]
    fn long_snippet_crossfade_loops_without_seam_blackouts() {
        let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 24.0, ..Default::default() };
        let sid = p.add_source(crate::timeline::Source {
            name: "short".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(220.0, 1.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(200, 180, 140))),
            base_hz: 220.0,
            ..Default::default()
        });
        let t = p.add_track("bed");
        let mut c = Clip::new_snippet(sid, 0.0, 8.0);
        c.grains.envelope = 0.04;
        p.tracks[t].clips.push(c);

        let out = render_project(&p, 0.0, 8.0);
        let sr = p.sample_rate as usize;
        let late_rms = (out.audio.left[sr * 22 / 10..sr * 28 / 10]
            .iter()
            .map(|s| s * s)
            .sum::<f32>()
            / (sr * 6 / 10) as f32)
            .sqrt();
        assert!(late_rms > 0.15, "snippet looped seamlessly, late rms {late_rms}");
        for w in out.frames[8..out.frames.len() - 8].windows(2) {
            let dl = (w[1].mean_luma() - w[0].mean_luma()).abs();
            assert!(dl < 0.5, "seam blink of {dl} luma");
        }
    }

    #[test]
    fn low_band_sidechain_ignores_hihat_chatter() {
        let mut p = Project { bpm: 120.0, width: 16, height: 16, fps: 12.0, ..Default::default() };
        let sub_src = p.add_source(crate::timeline::Source {
            name: "sub".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(55.0, 2.0, p.sample_rate))),
            video: None,
            base_hz: 55.0,
            ..Default::default()
        });
        let hat_src = p.add_source(crate::timeline::Source {
            name: "hat".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(6000.0, 2.0, p.sample_rate))),
            video: None,
            base_hz: 6000.0,
            ..Default::default()
        });
        let t_sub = p.add_track("sub");
        let mut cs = Clip::new_snippet(sub_src, 0.0, 2.0);
        cs.grains.pan = -1.0;
        p.tracks[t_sub].clips.push(cs);

        let t_hat = p.add_track("hat");
        let mut ch = Clip::new_snippet(hat_src, 0.0, 2.0);
        ch.grains.pan = 1.0;
        ch.grains.gain = 1.0;
        p.tracks[t_hat].clips.push(ch);

        let base = render_project(&p, 0.0, 2.0);
        p.tracks[t_sub].duck = Some(crate::timeline::DuckSpec {
            source_track: t_hat,
            amount: 0.9,
            attack: 0.005,
            release: 0.15,
            key_hz: 165.0,
        });
        let with_hat = render_project(&p, 0.0, 2.0);
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let r0 = rms(&base.audio.left[4000..40000]);
        let r1 = rms(&with_hat.audio.left[4000..40000]);
        assert!(r1 > r0 * 0.92, "6 kHz hi-hat must not duck 165 Hz-keyed sub: {r1} vs {r0}");
    }

    #[test]
    fn master_av_bloom_correlates_audio_onsets_with_frame_changes() {
        // Regression test locking whole-frame audio-onset <-> frame-diff
        // correspondence (`corr >= 0.40`) with zero 1-frame strobe reversals.
        let mut p = Project { bpm: 120.0, width: 32, height: 24, fps: 24.0, master_bloom: 0.65, ..Default::default() };
        let bed_src = p.add_source(crate::timeline::Source {
            name: "bed".into(),
            audio: None,
            video: Some(std::sync::Arc::new(solid_video(45, 50, 60))),
            base_hz: 220.0,
            ..Default::default()
        });
        let hit_src = p.add_source(crate::timeline::Source {
            name: "hit".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(110.0, 0.25, p.sample_rate))),
            video: None,
            base_hz: 110.0,
            ..Default::default()
        });
        let t0 = p.add_track("bg");
        p.tracks[t0].clips.push(Clip::new_snippet(bed_src, 0.0, 6.0));

        let mut pat = crate::seq::StepPattern::new("pulse");
        let r = pat.add_row(hit_src);
        pat.rows[r].grains.duration = 0.12;
        pat.rows[r].grains.env_skew = -0.85;
        for st in [0, 6, 10, 14] {
            pat.rows[r].steps[st].on = true;
        }
        let pid = p.add_pattern(pat);
        let t1 = p.add_track("drums");
        p.tracks[t1].clips.push(Clip::new_pattern(pid, 0.0, 6.0));

        let out = render_project(&p, 0.0, 6.0);
        let nf = out.frames.len();
        let spf = p.sample_rate as usize / 24;
        let lumas: Vec<f32> = out.frames.iter().map(|f| f.mean_luma()).collect();
        let diffs: Vec<f32> = lumas.windows(2).map(|w| (w[1] - w[0]).abs()).collect();

        // Check zero >3-luma 1-frame strobe reversals.
        let signed: Vec<f32> = lumas.windows(2).map(|w| w[1] - w[0]).collect();
        let reversals = signed
            .windows(2)
            .filter(|w| w[0].abs() > 3.0 && w[1].abs() > 3.0 && w[0].signum() != w[1].signum())
            .count();
        assert_eq!(reversals, 0, "master_bloom must never introduce 1-frame strobe reversals");

        // Compute log-RMS onset flux and Pearson correlation with whole-frame |dY|.
        let log_rms: Vec<f32> = (0..nf)
            .map(|fi| {
                let s0 = fi * spf;
                let s1 = ((fi + 1) * spf).min(out.audio.left.len());
                let e = (out.audio.left[s0..s1].iter().map(|x| x * x).sum::<f32>() / (s1 - s0).max(1) as f32).sqrt();
                (e + 1e-4).ln()
            })
            .collect();
        let flux: Vec<f32> = log_rms.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
        let n = flux.len().min(diffs.len());
        let mf = flux[..n].iter().sum::<f32>() / n as f32;
        let md = diffs[..n].iter().sum::<f32>() / n as f32;
        let mut cov = 0.0f32;
        let mut vf = 0.0f32;
        let mut vd = 0.0f32;
        for i in 0..n {
            let df = flux[i] - mf;
            let dd = diffs[i] - md;
            cov += df * dd;
            vf += df * df;
            vd += dd * dd;
        }
        let corr = cov / (vf.sqrt() * vd.sqrt() + 1e-9);
        assert!(corr > 0.38, "expected whole-frame onset<->diff corr > 0.38, got {corr:.3}");
    }

    #[test]
    fn choke_group_cuts_prior_hat_in_3ms_while_preserving_simultaneous_chord_voices() {
        let mut p = snippet_project();
        p.studio_bus = false;
        let sid = 0;
        p.tracks.clear();

        let mut pat = crate::seq::StepPattern::new("hats_and_chords");
        // Row 0: Open hat on step 0 (long 0.45s), closed hat on step 2 (0.25s later) in choke_group 1.
        let r_hat = pat.add_row(sid);
        pat.rows[r_hat].choke_group = 1;
        pat.rows[r_hat].grains.duration = 0.45;
        pat.rows[r_hat].grains.envelope = 0.02;
        pat.rows[r_hat].steps[0] = crate::seq::Step { on: true, length: 1.0, ..Default::default() };
        pat.rows[r_hat].steps[2] = crate::seq::Step { on: true, length: 0.12, ..Default::default() };

        // Row 1: Min7 4-voice chord on step 0 in choke_group 2, choked later on step 8 (1.0s).
        let r_ch = pat.add_row(sid);
        pat.rows[r_ch].choke_group = 2;
        pat.rows[r_ch].grains.duration = 1.5;
        pat.rows[r_ch].steps[0] = crate::seq::Step {
            on: true,
            chord: crate::grain::ChordVoicing::Min7,
            ..Default::default()
        };
        pat.rows[r_ch].steps[8] = crate::seq::Step { on: true, length: 0.1, ..Default::default() };

        let pid = p.add_pattern(pat);
        let t_hat = p.add_track("seq");
        p.tracks[t_hat].clips.push(Clip::new_pattern(pid, 0.0, 4.0));

        let plans = plan_project(&p);
        let all_evs: Vec<&GrainEvent> = plans[0].parts.iter().flat_map(|(_, e)| e.iter()).collect();
        // All 4 simultaneous Min7 chord voices at t=0.0 must choke at t=1.0 (step 8), NEVER at t=0.0!
        let chord_t0: Vec<&GrainEvent> = all_evs
            .iter()
            .copied()
            .filter(|e| e.tag.choke_group == 2 && e.onset < 0.01)
            .collect();
        assert_eq!(chord_t0.len(), 4, "Min7 expanded to 4 polyphonic voices");
        for ce in &chord_t0 {
            assert_eq!(ce.tag.choke_at, Some(1.0), "sibling chord voices never self-choke");
            assert!(ce.sculpt.hpf_hz >= 210.0, "chord stack HPF >= 210 Hz");
        }
        // Open hat at t=0.0 is choked at step 2 (t = 0.25s) and silent after closed hat ends (~0.31s).
        let open_hat = all_evs
            .iter()
            .copied()
            .find(|e| e.tag.choke_group == 1 && e.onset < 0.01)
            .expect("open hat");
        assert_eq!(open_hat.tag.choke_at, Some(0.25));

        // Render solo hat row to verify silence after closed hat finishes and click-free 3ms ramp.
        p.patterns[pid].rows[r_ch].muted = true;
        let out = render_project(&p, 0.0, 2.0);
        let sr = p.sample_rate as usize;
        let post_closed = &out.audio.left[(0.34 * sr as f64) as usize..(0.44 * sr as f64) as usize];
        let post_rms = (post_closed.iter().map(|s| s * s).sum::<f32>() / post_closed.len() as f32).sqrt();
        assert!(post_rms < 1e-5, "open hat tail cleanly choked by closed hat: {post_rms}");
    }

    #[test]
    fn studio_bus_ceiling_and_realtime_parity_hold_with_sculptors_and_kaleido() {
        let mut p = Project {
            bpm: 124.0,
            width: 64,
            height: 36,
            fps: 12.0,
            ..Default::default()
        };
        assert!(p.studio_bus, "studio_bus defaults to true");
        let sid = p.add_source(crate::timeline::Source {
            name: " rich ".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(110.0, 3.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(64, 36, 12.0, 3.0))),
            base_hz: 110.0,
            ..Default::default()
        });
        let mut pat = crate::seq::StepPattern::new("vgs");
        let r0 = pat.add_row(sid);
        pat.rows[r0].sculpt = crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Kick);
        pat.rows[r0].steps[0].on = true;
        pat.rows[r0].steps[8].on = true;

        let r1 = pat.add_row(sid);
        pat.rows[r1].sculpt = crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Pluck);
        pat.rows[r1].steps[4] = crate::seq::Step {
            on: true,
            ratchet: 3,
            ratchet_pitch: 2.0,
            ..Default::default()
        };
        pat.rows[r1].steps[12] = crate::seq::Step {
            on: true,
            chord: crate::grain::ChordVoicing::Min7,
            ..Default::default()
        };
        let pid = p.add_pattern(pat);
        let t_seq = p.add_track("drums");
        p.tracks[t_seq].clips.push(Clip::new_pattern(pid, 0.0, 4.0));

        let t_loop = p.add_track("basinski");
        let mut loop_clip = Clip::new_snippet(sid, 0.0, 4.0);
        loop_clip.grains.gain = 1.4; // push into studio ceiling
        loop_clip.grains.sculpt = crate::grain::VoiceSculpt {
            free_loop_secs: Some(0.61),
            ..crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Disintegrate)
        };
        loop_clip.effects.push(AvEffect::new(EffectKind::Kaleido { folds: 6.0, spin: 0.35 }));
        p.tracks[t_loop].clips.push(loop_clip);

        p.master_effects.push(AvEffect::new(EffectKind::Reverb { size: 0.7, damp: 0.4, mix: 0.25 }));

        let offline = render_project(&p, 0.0, 4.0);
        assert!(
            offline.audio.peak() <= crate::audio::STUDIO_CEILING + 1e-4,
            "studio ceiling (-1.4 dBTP) respected: peak={}",
            offline.audio.peak()
        );

        let total = offline.audio.len();
        let mut rt = crate::realtime::RealtimeAudio::new(p, 0.0, None);
        let (mut left, mut right) = (vec![0.0f32; total], vec![0.0f32; total]);
        let blocks = [256usize, 64, 1024, 513, 2048];
        let (mut pos, mut bi) = (0, 0);
        while pos < total {
            let b = blocks[bi % blocks.len()].min(total - pos);
            rt.next_block(&mut left[pos..pos + b], &mut right[pos..pos + b]);
            pos += b;
            bi += 1;
        }
        let max_diff = left
            .iter()
            .zip(&offline.audio.left)
            .chain(right.iter().zip(&offline.audio.right))
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 1e-5, "RealtimeAudio == render_project with StudioBus + Sculpt + Kaleido: {max_diff}");
    }

    #[test]
    fn stage_layouts_split_tile_bleed_aspect_and_captions_render_on_cpu() {
        let mut p = two_track_project(true);
        p.set_aspect(crate::tile::AspectRatio::Vertical9x16);
        assert_eq!((p.width, p.height), (18, 32));
        p.set_aspect_hd(crate::tile::AspectRatio::Vertical9x16);
        assert_eq!((p.width, p.height), (1080, 1920));
        p.set_aspect_hd(crate::tile::AspectRatio::Square1x1);
        assert_eq!((p.width, p.height), (1080, 1080));
        // Use fast test resolution while preserving 16:9 aspect.
        p.width = 96;
        p.height = 54;
        p.fps = 20.0;

        // Add a third sustained wash track (`TapeLoop`) + caption on track 0.
        let green = p.add_source(crate::timeline::Source {
            name: "wash".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(165.0, 2.0, p.sample_rate))),
            video: Some(std::sync::Arc::new(solid_video(20, 180, 60))),
            base_hz: 165.0,
            ..Default::default()
        });
        let tw = p.add_track("wash");
        let mut wc = Clip::new_snippet(green, 0.0, 2.0);
        wc.grains.sculpt = crate::grain::VoiceSculpt::preset(crate::grain::Stamp::TapeLoop);
        p.tracks[tw].clips.push(wc);

        for layout in [
            crate::tile::Layout::Split,
            crate::tile::Layout::Tile,
            crate::tile::Layout::Bleed,
        ] {
            p.layout = layout;
            let out = render_project(&p, 0.0, 1.0);
            let mid = &out.frames[out.frames.len() / 2];
            assert!(mid.mean_luma() > 12.0, "{layout:?} lit the stage: {}", mid.mean_luma());
            if layout == crate::tile::Layout::Split {
                // Left panel carries red + green wash bleed; right panel carries blue + green wash bleed.
                let left_px = mid.get(16, 27);
                let right_px = mid.get(80, 27);
                assert!(left_px[0] > left_px[2] + 30, "Split left panel is red-dominant: {left_px:?}");
                assert!(right_px[2] > right_px[0] + 30, "Split right panel is blue-dominant: {right_px:?}");
                assert!(left_px[1] > 25 && right_px[1] > 25, "Wash bleeds across both Split panels");
            }
            if layout == crate::tile::Layout::Bleed {
                let center_px = mid.get(48, 27);
                assert!(
                    center_px[0] > 40 && center_px[1] > 40 && center_px[2] > 40 && center_px[0] < 252,
                    "Bleed melts all 3 layers without clipping flat white: {center_px:?}"
                );
            }
        }

        // Captions: attach a vocal slice with a Caption and compare captions_on true vs false.
        let mut cap_proj = snippet_project();
        cap_proj.width = 160;
        cap_proj.height = 90;
        let mut sl = crate::chop::Slice::new(0.0, 1.0);
        sl.caption = crate::chop::Caption::new("PURE SAMPLE");
        cap_proj.sources[0].slices = vec![sl];
        let mut pat = crate::seq::StepPattern::new("vox");
        let r = pat.add_row(0);
        pat.rows[r].steps[0] = crate::seq::Step {
            on: true,
            slice_index: Some(0),
            ..Default::default()
        };
        let pid = cap_proj.add_pattern(pat);
        cap_proj.tracks[0].clips.clear();
        cap_proj.tracks[0].clips.push(Clip::new_pattern(pid, 0.0, 2.0));
        cap_proj.captions_on = true;
        let with_cap = render_project(&cap_proj, 0.0, 1.0);
        cap_proj.captions_on = false;
        let no_cap = render_project(&cap_proj, 0.0, 1.0);
        assert_ne!(
            with_cap.frames[2].data, no_cap.frames[2].data,
            "captions_on draws kinetic stage typography"
        );
    }

    #[test]
    fn modell5_micro_bursts_render_full_cell_and_portrait_columns() {
        let mut p = Project {
            bpm: 120.0,
            width: 96,
            height: 54,
            fps: 30.0,
            layout: crate::tile::Layout::Tile,
            ..Default::default()
        };
        // Three side-by-side portrait voices at offset audio-rate micro-burst densities (18 / 19 / 20 Hz).
        for (k, (rgb, dens)) in [([230, 30, 30], 18.0f32), ([30, 220, 60], 19.0), ([40, 80, 240], 20.0)]
            .into_iter()
            .enumerate()
        {
            let sid = p.add_source(crate::timeline::Source {
                name: format!("m5_{k}"),
                audio: Some(std::sync::Arc::new(crate::audio::AudioClip::sine(180.0 + k as f32 * 25.0, 2.0, p.sample_rate))),
                video: Some(std::sync::Arc::new(solid_video(rgb[0], rgb[1], rgb[2]))),
                base_hz: 180.0,
                ..Default::default()
            });
            let tid = p.add_track(&format!("face_{k}"));
            let mut c = Clip::new(sid, 0.0, 2.0);
            c.grains.density = dens;
            c.grains.duration = 0.042; // 42 ms micro-grain
            c.grains.spray = 0.02; // tight <= 0.08
            c.grains.scan_speed = 0.06;
            c.grains.adsr = Some(crate::envelope::Adsr {
                attack: 0.001,
                decay: 0.0,
                sustain: 1.0,
                release: 0.02,
                picture: crate::envelope::PictureCut::Cut,
                ..Default::default()
            });
            p.tracks[tid].clips.push(c);
        }

        let plan = plan_project(&p);
        assert!(plan.iter().all(|cp| cp.parts[0].1.iter().all(|e| e.tag.hard_burst)));
        // On 16:9 (96x54), 3 simultaneous MODELL 5 voices form a 3x1 portrait triptych!
        let grids = frame_tile_grids_stage(&plan, 0.0, p.fps, 30, 96.0 / 54.0, p.portrait_columns);
        assert!(grids.iter().skip(2).any(|&g| g == (3, 1)), "3 voices -> 3x1 portrait columns: {grids:?}");

        // Stack mode with hard_burst ALSO renders full-frame (dest = [0, 0, 1, 1], tiled = true).
        let draws = crate::video::grain_draws_stage(
            &plan[0].parts[0].1,
            &crate::video::VisualGrainStyle::default(),
            &crate::video::AvLink::default(),
            &crate::timeline::ClipTransform::default(),
            2.0,
            plan[0].parts[0].1[0].onset + 0.01,
            16.0 / 9.0,
            false,
            crate::tile::Layout::Stack,
            None,
        );
        assert!(!draws.is_empty());
        assert_eq!(draws[0].dest, [0.0, 0.0, 1.0, 1.0], "MODELL 5 micro-burst fills full frame in Stack");
        assert!(draws[0].tiled, "hard-edged zero-feather cut");
    }

    #[test]
    fn snippet_sculpt_pitch_and_master_isolator_stream_sample_exact() {
        let mut p = Project {
            bpm: 120.0,
            sample_rate: 48_000,
            width: 64,
            height: 64,
            fps: 24.0,
            ..Default::default()
        };
        let sid = p.add_source(crate::timeline::Source {
            name: "s".into(),
            audio: Some(std::sync::Arc::new(crate::audio::AudioClip::saw_stack(220.0, 2.0, 48_000))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(64, 64, 24.0, 2.0))),
            base_hz: 220.0,
            ..Default::default()
        });
        let tid = p.add_track("hat_snip");
        let mut c = Clip::new_snippet(sid, 0.0, 1.0);
        c.grains.sculpt = crate::grain::VoiceSculpt::preset(crate::grain::Stamp::Hat); // +10 st
        p.tracks[tid].clips.push(c);

        // Snippet event honors Stamp::Hat +10st pitch transposition!
        let plan = plan_project(&p);
        let ratio = plan[0].parts[0].1[0].pitch_ratio;
        assert!(
            (ratio - crate::music::semitones_to_ratio(10.0)).abs() < 1e-4,
            "Snippet applies Stamp::Hat +10st: {ratio}"
        );

        // Engage master ISOL8 (LMF+MF+HMF SOLO + -3 dB output) and verify sample-exact
        // parity between offline `render_project` and chunked `RealtimeAudio`!
        p.isolator.solo = [false, true, true, true, false];
        p.isolator.output_db = -3.0;
        let offline = render_project(&p, 0.0, 1.0);

        let mut rt = crate::realtime::RealtimeAudio::new(p, 0.0, None);
        let n = offline.audio.len();
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        let mut pos = 0;
        while pos < n {
            let blk = 256.min(n - pos);
            rt.next_block(&mut l[pos..pos + blk], &mut r[pos..pos + blk]);
            pos += blk;
        }
        let max_diff = offline
            .audio
            .left
            .iter()
            .zip(&l)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 1e-6, "RealtimeAudio + ISOL8 matches render_project sample-exactly: {max_diff}");
    }
}
