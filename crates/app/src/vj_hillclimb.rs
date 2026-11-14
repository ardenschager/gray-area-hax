//! Phase 3 VJ Hillclimb: 5 contrasting 60fps pure-YouTube sample pieces +
//! 1 headless 1080p 30fps `ISOL8` hardware studio VJ showcase film.
//! Zero AI arrangement, zero oscillators, zero external synth/drum WAVs.

use crate::showcase::{
    lab, record_with, write_wav, Aim, Caption as FilmCaption, Cue, Film, Gesture, Overlay,
    RecordOpts, Tour, FPS,
};
use crate::{App, AppOpts, Clock};
use chromagrain_core::auto::{AutoPoint, AutomationLane};
use chromagrain_core::chop::{self, Caption, Slice, SliceRole};
use chromagrain_core::dsp::{BandIsolator, EqBand, IsolatorChannelMode};
use chromagrain_core::envelope::{Adsr, PictureCut};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::{ChordVoicing, GrainSettings, Stamp, VoiceSculpt};
use chromagrain_core::media;
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::render::render_project_to_video;
use chromagrain_core::seq::{Step, StepPattern};
use chromagrain_core::tile::{AspectRatio, Layout};
use chromagrain_core::timeline::{
    Clip, DuckSpec, MasterPunch, Project, Provenance, PunchAction, Source,
};
use chromagrain_ui::YoutubeHit;
use eframe::egui;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};

fn hillclimb_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../outputs/261113_vj-hillclimb")
}

fn src_dir() -> PathBuf {
    hillclimb_root().join("src")
}

fn out_dir(version: &str) -> PathBuf {
    hillclimb_root().join(version).join("out")
}

fn version_tag() -> String {
    std::env::var("CHROMAGRAIN_VJ_VERSION").unwrap_or_else(|_| "v001".into())
}

/// Load a real YouTube `.mp4` from `outputs/261113_vj-hillclimb/src/` with `48 kHz` audio,
/// `24 fps` `640px` video, deterministic spectral/pitch slices, and full provenance.
fn load_yt(
    filename: &str,
    display_name: &str,
    uploader: &str,
    upload_date: &str,
    views: u64,
    url: &str,
    window: (f64, f64),
) -> Source {
    let p = src_dir().join(filename);
    let mut src = media::load_source(&p, 48_000)
        .unwrap_or_else(|e| panic!("failed to load real YouTube clip {}: {e}", p.display()));
    src.name = display_name.into();
    src.provenance = Some(Provenance {
        title: display_name.into(),
        uploader: uploader.into(),
        upload_date: upload_date.into(),
        views: Some(views),
        url: url.into(),
        license: String::new(),
        query: String::new(),
        window,
    });
    if src.slices.is_empty() {
        src.slices = src.auto_slices(20);
    }
    src
}

/// Append a precision-timed slice `[start..end]` with optional stage `caption` and `role`,
/// enriched directly from the source's real audio & video frames, returning its slice index.
fn push_slice(
    src: &mut Source,
    start: f64,
    end: f64,
    role: SliceRole,
    caption: &str,
    focus_override: Option<(f32, f32)>,
) -> u16 {
    let dur = src.duration().max(0.05);
    let s0 = start.clamp(0.0, (dur - 0.02).max(0.0));
    let s1 = end.clamp(s0 + 0.015, dur);
    let mut single = vec![Slice::new(s0, s1)];
    chop::enrich_slices(&mut single, src.audio.as_deref(), src.video.as_deref());
    let mut sl = single.remove(0);
    if role != SliceRole::Raw {
        sl.role_hint = role;
    }
    if !caption.is_empty() {
        sl.caption = Caption::new(caption);
    }
    if let Some(f) = focus_override {
        sl.focus = f;
    }
    src.slices.push(sl);
    (src.slices.len() - 1) as u16
}

// ============================================================================
// PIECE 1: `frontier-therapy_dialogue-collage` (9:16 1080x1920 @ 60fps, ~19.6s)
// The Avalanches — Frontier Psychiatrist x absurdist YouTube dialogue collage
// ============================================================================

fn build_piece1_frontier_therapy() -> Project {
    let mut p = Project {
        bpm: 98.0,
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Split,
        layout_pan: true,
        studio_bus: true,
        captions_on: true,
        master_gain_db: 1.5,
        master_bloom: 0.14,
        ..Default::default()
    };
    p.set_aspect_hd(AspectRatio::Vertical9x16);
    p.master_tape.warmth = 0.22;

    // 1. Real YouTube Sources (every visual panel shows clean moving human/instrument footage!)
    let mut s_foot = load_yt(
        "p3_house_footwork_Bldi_Ph-oDw.mp4",
        "House Footwork Stomp",
        "PRINCE DANCE TUTORIALS",
        "20260618",
        959,
        "https://www.youtube.com/watch?v=Bldi_Ph-oDw",
        (49.5, 28.0),
    );
    let sl_kick_a = push_slice(&mut s_foot, 19.32, 19.54, SliceRole::Kick, "", Some((0.68, 0.46)));
    let sl_kick_b = push_slice(&mut s_foot, 17.32, 17.56, SliceRole::Kick, "", Some((0.68, 0.46)));

    let s_funk_raw = load_yt(
        "p1_funk_drums_fnTaVyzWYjI.mp4",
        "Funk Groove Drums",
        "TIMTIM RECORDS",
        "20230217",
        265_399,
        "https://www.youtube.com/watch?v=fnTaVyzWYjI",
        (19.5, 28.0),
    );

    let s_room_raw = load_yt(
        "p1_room_drums_NqoBtAsByLI.mp4",
        "Drum Room Shootout",
        "Geoff Bosco",
        "20190115",
        926,
        "https://www.youtube.com/watch?v=NqoBtAsByLI",
        (7.5, 26.0),
    );

    let mut s_brass = load_yt(
        "p1_trumpet_fanfare_T_Kj30n-g9E.mp4",
        "Solo Trumpet Fanfare",
        "New York Philharmonic",
        "20220328",
        53_101,
        "https://www.youtube.com/watch?v=T_Kj30n-g9E",
        (6.0, 30.0),
    );
    s_brass.base_hz = 493.88; // B4
    let sl_brass_b4 = push_slice(&mut s_brass, 16.12, 16.56, SliceRole::Tonal, "", Some((0.50, 0.42)));
    let sl_brass_high = push_slice(&mut s_brass, 26.42, 26.92, SliceRole::Tonal, "", Some((0.50, 0.42)));

    let mut s_scratch = load_yt(
        "p1_turntable_scratch_rtqTmUVjsuY.mp4",
        "Basic Vinyl Scratches",
        "ScratchDJAcademy",
        "20140819",
        1_160_631,
        "https://www.youtube.com/watch?v=rtqTmUVjsuY",
        (150.5, 30.0),
    );
    let sl_scr_a = push_slice(&mut s_scratch, 5.86, 6.38, SliceRole::Phrase, "", Some((0.52, 0.52)));
    let sl_scr_b = push_slice(&mut s_scratch, 15.67, 16.12, SliceRole::Phrase, "", Some((0.52, 0.52)));

    let mut s_angry = load_yt(
        "p1_angry_boy_1950_vwVHhrj-doE.mp4",
        "Angry Boy (1950)",
        "OVR | Old Videos Restored",
        "20260531",
        79,
        "https://www.youtube.com/watch?v=vwVHhrj-doE",
        (296.0, 40.0),
    );
    // Word-exact VTT local timestamps (w0=296.0s):
    // [0.44] We [0.64] think [0.84] it [0.96] calls [1.28] for [1.40] professional [2.08] help.
    let sl_stut_help = push_slice(&mut s_angry, 0.44, 1.02, SliceRole::Phrase, "WE THINK—", Some((0.48, 0.40)));
    let sl_full_help = push_slice(
        &mut s_angry,
        0.44,
        2.95,
        SliceRole::Phrase,
        "CALLS FOR PROFESSIONAL HELP",
        Some((0.48, 0.40)),
    );
    let sl_pine_st = push_slice(
        &mut s_angry,
        3.82,
        6.95,
        SliceRole::Phrase,
        "GUIDANCE CLINIC ON PINE ST?",
        Some((0.52, 0.38)),
    );
    let sl_take_tommy = push_slice(
        &mut s_angry,
        18.40,
        20.68,
        SliceRole::Phrase,
        "PSYCHIATRIC SOCIAL WORKER",
        Some((0.50, 0.38)),
    );

    let mut s_mental = load_yt(
        "p1_mental_fit_1952_uC9B2F-l5V8.mp4",
        "Keeping Mentally Fit (1952)",
        "OVR | Old Videos Restored",
        "20251003",
        24,
        "https://www.youtube.com/watch?v=uC9B2F-l5V8",
        (2.0, 42.0),
    );
    // Word-exact VTT local timestamps (w0=2.0s):
    // [35.68] in [35.92] a [36.16] mental [36.56] institution. [38.00] My job...
    let sl_stut_mental = push_slice(
        &mut s_mental,
        36.10,
        36.55,
        SliceRole::Phrase,
        "MENTAL—",
        Some((0.50, 0.42)),
    );
    let sl_full_mental = push_slice(
        &mut s_mental,
        35.64,
        37.86,
        SliceRole::Phrase,
        "IN A MENTAL INSTITUTION",
        Some((0.50, 0.42)),
    );

    let mut s_crazy = load_yt(
        "p1_crazy_calls_tape_wewsuLjJpEo.mp4",
        "1980s Answering Machine Gag",
        "UncleCathode",
        "20070821",
        186_607,
        "https://www.youtube.com/watch?v=wewsuLjJpEo",
        (13.5, 45.0),
    );
    // Word-exact VTT local timestamps (w0=13.5s):
    // [10.98] one-way [11.46] ticket [11.86] to [12.34] the [12.66] answering [13.14] machine [13.62] zone.
    // [14.34] Nobody's [14.82] home. / [16.90] Nobody's [17.46] home.
    let sl_answering = push_slice(
        &mut s_crazy,
        10.95,
        14.18,
        SliceRole::Phrase,
        "ANSWERING MACHINE ZONE",
        Some((0.50, 0.48)),
    );
    let sl_nobody = push_slice(
        &mut s_crazy,
        16.85,
        18.85,
        SliceRole::Phrase,
        "NOBODY'S HOME!",
        Some((0.50, 0.48)),
    );

    let mut s_deviant = load_yt(
        "p1_deviant_behavior_ABCDPGzjalQ.mp4",
        "Changing Deviant Behavior",
        "Bite-Size Random",
        "20090104",
        783_311,
        "https://www.youtube.com/watch?v=ABCDPGzjalQ",
        (18.5, 42.0),
    );
    let sl_sloppy = push_slice(
        &mut s_deviant,
        26.34,
        29.38,
        SliceRole::Phrase,
        "IN A RUT WITH A BAD HABIT",
        Some((0.50, 0.42)),
    );

    let mut s_horse = load_yt(
        "p1_horse_whinny_Y4kY-Rx47VM.mp4",
        "Horse Whinny Calls",
        "wildLINK",
        "20241107",
        356_581,
        "https://www.youtube.com/watch?v=Y4kY-Rx47VM",
        (200.5, 34.0),
    );
    // Pure high-centroid horse whinnies (verified ZCR 1792-3392 Hz, zero narrator voice!):
    let sl_whinny_long = push_slice(
        &mut s_horse,
        26.15,
        27.45,
        SliceRole::Phrase,
        "[HORSE WHINNY!]",
        Some((0.46, 0.50)),
    );
    let sl_whinny_stab = push_slice(
        &mut s_horse,
        11.22,
        11.80,
        SliceRole::Phrase,
        "[NEIGH!]",
        Some((0.50, 0.48)),
    );

    // Pair static-card drum recordings with kinetic turntable & 1950s film visuals so 0 text cards appear!
    let mut s_funk = s_funk_raw.pair(&s_scratch);
    let sl_snare_a = push_slice(&mut s_funk, 8.31, 8.45, SliceRole::Snare, "", Some((0.52, 0.52)));
    let sl_snare_b = push_slice(&mut s_funk, 5.30, 5.43, SliceRole::Snare, "", Some((0.52, 0.52)));

    let mut s_room = s_room_raw.pair(&s_deviant);
    let sl_hat_closed = push_slice(&mut s_room, 24.08, 24.22, SliceRole::Hat, "", Some((0.50, 0.42)));
    let sl_hat_open = push_slice(&mut s_room, 6.10, 6.38, SliceRole::Hat, "", Some((0.50, 0.42)));

    let id_foot = p.add_source(s_foot);
    let id_funk = p.add_source(s_funk);
    let id_room = p.add_source(s_room);
    let id_brass = p.add_source(s_brass);
    let id_scratch = p.add_source(s_scratch);
    let id_angry = p.add_source(s_angry);
    let id_mental = p.add_source(s_mental);
    let id_crazy = p.add_source(s_crazy);
    let id_deviant = p.add_source(s_deviant);
    let id_horse = p.add_source(s_horse);

    let key = Key::new(4, ScaleKind::Minor); // E Minor

    // Track 0: Boom-Bap Pure-Footage Sub Kick (sidechain trigger)
    let mut pat_kick = StepPattern::new("boom-bap kick");
    pat_kick.set_grid(4.0, 4);
    pat_kick.swing = 0.58;
    let rk = pat_kick.add_row(id_foot);
    pat_kick.rows[rk].sculpt = VoiceSculpt {
        pitch_semitones: -13.0,
        pitch_env_semitones: 17.0,
        pitch_env_ms: 44.0,
        sub_exciter: 0.82,
        drive_db: 8.5,
        hpf_hz: 28.0,
        lpf_hz: 152.0,
        resonance_q: 0.90,
        ..VoiceSculpt::preset(Stamp::Kick)
    };
    pat_kick.rows[rk].choke_group = 2;
    pat_kick.rows[rk].grains.adsr = Some(Adsr {
        attack: 0.002,
        decay: 0.22,
        sustain: 0.0,
        release: 0.06,
        picture: PictureCut::Cut,
        cut_threshold: 0.08,
        ..Adsr::default()
    });
    for &(st, sl, g, ev_n, ev_off) in &[
        (0usize, sl_kick_a, 1.05f32, 1u8, 0u8),
        (3, sl_kick_b, 0.72, 2, 1),
        (7, sl_kick_b, 0.86, 1, 0),
        (10, sl_kick_a, 1.00, 1, 0),
        (14, sl_kick_b, 0.78, 2, 0),
    ] {
        pat_kick.rows[rk].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl),
            length: 1.0,
            every_n: ev_n,
            every_offset: ev_off,
            ..Step::default()
        };
    }
    let pid_kick = p.add_pattern(pat_kick);
    let t_kick = p.add_track("boom-bap kick");
    p.tracks[t_kick].level = 1.06;
    let mut ck1 = Clip::new_pattern(pid_kick, 0.0, 15.0);
    ck1.transform.scale = 1.85;
    let mut ck2 = Clip::new_pattern(pid_kick, 24.0, 8.0);
    ck2.transform.scale = 1.85;
    p.tracks[t_kick].clips.push(ck1);
    p.tracks[t_kick].clips.push(ck2);

    // Track 1: Crisp Funk Snare + Swung Room Hats + Vinyl Scratches
    let mut pat_top = StepPattern::new("snare + hats + scratch");
    pat_top.set_grid(4.0, 4);
    pat_top.swing = 0.58;
    let rs = pat_top.add_row(id_funk);
    pat_top.rows[rs].sculpt = VoiceSculpt {
        snare_snap: 0.88,
        drive_db: 8.5,
        hpf_hz: 180.0,
        lpf_hz: 8800.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Snare)
    };
    pat_top.rows[rs].grains.adsr = Some(Adsr {
        attack: 0.001,
        decay: 0.16,
        sustain: 0.0,
        release: 0.05,
        picture: PictureCut::Cut,
        cut_threshold: 0.08,
        ..Adsr::default()
    });
    for &(st, sl, g, ev_n, ev_off) in &[
        (4usize, sl_snare_a, 1.00f32, 1u8, 0u8),
        (9, sl_snare_b, 0.42, 2, 1),
        (12, sl_snare_a, 1.04, 1, 0),
        (15, sl_snare_b, 0.48, 2, 0),
    ] {
        pat_top.rows[rs].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl),
            every_n: ev_n,
            every_offset: ev_off,
            ..Step::default()
        };
    }

    let rh = pat_top.add_row(id_room);
    pat_top.rows[rh].sculpt = VoiceSculpt {
        pitch_semitones: 8.0,
        drive_db: 3.0,
        hpf_hz: 5600.0,
        lpf_hz: 18_000.0,
        ..VoiceSculpt::preset(Stamp::Hat)
    };
    pat_top.rows[rh].choke_group = 3;
    for i in (0..16).step_by(2) {
        let open = i == 6 || i == 14;
        pat_top.rows[rh].steps[i] = Step {
            on: true,
            gain: if i % 4 == 2 { 0.68 } else { 0.48 },
            slice_index: Some(if open { sl_hat_open } else { sl_hat_closed }),
            length: if open { 0.85 } else { 0.45 },
            ..Step::default()
        };
    }
    pat_top.rows[rh].steps[11] = Step {
        on: true,
        gain: 0.44,
        slice_index: Some(sl_hat_closed),
        length: 0.35,
        ratchet: 2,
        ratchet_pitch: 2.0,
        ..Step::default()
    };

    let rsc = pat_top.add_row(id_scratch);
    pat_top.rows[rsc].sculpt = VoiceSculpt {
        drive_db: 2.5,
        hpf_hz: 320.0,
        lpf_hz: 11_000.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Phrase)
    };
    pat_top.rows[rsc].steps[14] = Step {
        on: true,
        gain: 0.62,
        slice_index: Some(sl_scr_a),
        length: 0.72,
        every_n: 2,
        every_offset: 0,
        ratchet: 2,
        ratchet_pitch: 3.0,
        ..Step::default()
    };
    pat_top.rows[rsc].steps[13] = Step {
        on: true,
        gain: 0.60,
        slice_index: Some(sl_scr_b),
        length: 0.80,
        every_n: 2,
        every_offset: 1,
        ..Step::default()
    };

    let pid_top = p.add_pattern(pat_top);
    let t_top = p.add_track("funk kit + scratches");
    p.tracks[t_top].level = 0.90;
    p.tracks[t_top].level_points = vec![
        AutoPoint { beat: 0.0, value: 0.92 },
        AutoPoint { beat: 15.2, value: 0.92 },
        AutoPoint { beat: 16.0, value: 0.34 }, // Strip drums down for AM Radio Break dynamic contrast!
        AutoPoint { beat: 23.5, value: 0.34 },
        AutoPoint { beat: 24.0, value: 0.96 }, // Full-band Grid drop!
    ];
    p.tracks[t_top].clips.push(Clip::new_pattern(pid_top, 0.0, 15.2));
    p.tracks[t_top].clips.push(Clip::new_pattern(pid_top, 16.5, 15.5));

    // Track 2: Avalanches Resampled Brass Chord Stabs (sidechained to kick)
    let mut pat_brass = StepPattern::new("avalanches brass chords");
    pat_brass.set_grid(8.0, 4); // 2-bar (32-step) harmonic cycle
    pat_brass.swing = 0.58;
    let rb = pat_brass.add_row(id_brass);
    pat_brass.rows[rb].key = Some(key);
    pat_brass.rows[rb].grains.quantize_amount = 1.0;
    pat_brass.rows[rb].sculpt = VoiceSculpt {
        pitch_semitones: -12.0,
        comb_mix: 0.28,
        comb_feedback: 0.88,
        chord: ChordVoicing::Min9,
        shimmer: 0.18,
        hpf_hz: 235.0,
        lpf_hz: 9800.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Chord)
    };
    pat_brass.rows[rb].grains.adsr = Some(Adsr {
        attack: 0.008,
        decay: 0.38,
        sustain: 0.45,
        release: 0.18,
        picture: PictureCut::Fade,
        ..Adsr::default()
    });
    for &(st, pitch, sl, chd, len, gain) in &[
        (0usize, 0.0f32, sl_brass_b4, ChordVoicing::Min9, 1.35f32, 0.68f32),
        (6, 3.0, sl_brass_b4, ChordVoicing::Min7, 0.90, 0.56),
        (16, -2.0, sl_brass_b4, ChordVoicing::Maj7, 1.35, 0.66),
        (22, 5.0, sl_brass_high, ChordVoicing::Min9, 0.95, 0.54),
        (28, 0.0, sl_brass_b4, ChordVoicing::Sus4, 0.75, 0.52),
    ] {
        pat_brass.rows[rb].steps[st] = Step {
            on: true,
            pitch,
            gain,
            slice_index: Some(sl),
            chord: chd,
            length: len,
            ..Step::default()
        };
    }
    let pid_brass = p.add_pattern(pat_brass);
    let t_brass = p.add_track("brass chord stabs");
    p.tracks[t_brass].level = 0.62;
    p.tracks[t_brass].clips.push(Clip::new_pattern(pid_brass, 0.0, 15.0));
    p.tracks[t_brass].clips.push(Clip::new_pattern(pid_brass, 24.0, 8.0));
    p.tracks[t_brass].effects.push(AvEffect {
        kind: EffectKind::Reverb { size: 0.58, damp: 0.48, mix: 0.20 },
        audio: 1.0,
        video: 0.20,
    });

    // Track 3: `Choke Group 1` Absurdist Dialogue Call-and-Response (`0 st` natural speech + Captions!)
    // One 32-beat (128-sixteenth-step) non-looping theatrical dialogue pattern so every punchline
    // lands on the exact pocket beat with zero AI and real MPC Choke Group 1!
    let mut pat_vox = StepPattern::new("frontier therapy dialogue");
    pat_vox.set_grid(32.0, 4); // 128 sixteenth steps across the entire 32-beat collage
    pat_vox.swing = 0.54;

    let mut add_vox_row = |src_id: usize, gain: f32| -> usize {
        let r = pat_vox.add_row(src_id);
        pat_vox.rows[r].choke_group = 1;
        pat_vox.rows[r].grains.quantize_amount = 0.0; // Natural unwarped 0st formants!
        pat_vox.rows[r].grains.gain = gain;
        pat_vox.rows[r].grains.envelope = 0.02;
        pat_vox.rows[r].sculpt = VoiceSculpt {
            drive_db: 2.2,
            hpf_hz: 175.0,
            lpf_hz: 13_500.0,
            pocket_carve: true,
            ..VoiceSculpt::preset(Stamp::Phrase)
        };
        r
    };

    let rv_angry = add_vox_row(id_angry, 1.18);
    let rv_mental = add_vox_row(id_mental, 1.16);
    let rv_crazy = add_vox_row(id_crazy, 1.24);
    let rv_deviant = add_vox_row(id_deviant, 1.15);
    let rv_horse = add_vox_row(id_horse, 1.06);

    // Bar 1-2 (steps 0..32): Stutter "WE THINK—" -> Choked by "CALLS FOR PROFESSIONAL HELP" -> "GUIDANCE CLINIC ON PINE STREET?"
    for st in [0usize, 2, 4] {
        pat_vox.rows[rv_angry].steps[st] = Step {
            on: true,
            slice_index: Some(sl_stut_help),
            length: 0.95,
            gain: 0.95,
            ..Step::default()
        };
    }
    pat_vox.rows[rv_angry].steps[6] = Step {
        on: true,
        slice_index: Some(sl_full_help),
        length: 1.0,
        gain: 1.10,
        ..Step::default()
    };
    pat_vox.rows[rv_angry].steps[18] = Step {
        on: true,
        slice_index: Some(sl_pine_st),
        length: 0.92,
        gain: 1.08,
        ..Step::default()
    };

    // Bar 3-4 (steps 32..64): Stutter "MENTAL—" -> "A MENTAL INSTITUTION" -> HORSE WHINNY -> Tape Stop!
    for st in [32usize, 34] {
        pat_vox.rows[rv_mental].steps[st] = Step {
            on: true,
            slice_index: Some(sl_stut_mental),
            length: 0.95,
            gain: 0.96,
            ..Step::default()
        };
    }
    pat_vox.rows[rv_mental].steps[36] = Step {
        on: true,
        slice_index: Some(sl_full_mental),
        length: 1.0,
        gain: 1.12,
        ..Step::default()
    };
    pat_vox.rows[rv_horse].steps[50] = Step {
        on: true,
        slice_index: Some(sl_whinny_long),
        length: 1.0,
        gain: 1.02,
        ..Step::default()
    };

    // Bar 5-6 (steps 64..96 — ISOL8 AM Radio Break!): Answering machine + Deviant Behavior
    pat_vox.rows[rv_crazy].steps[64] = Step {
        on: true,
        slice_index: Some(sl_answering),
        length: 0.95,
        gain: 1.15,
        ..Step::default()
    };
    pat_vox.rows[rv_crazy].steps[78] = Step {
        on: true,
        slice_index: Some(sl_nobody),
        length: 1.0,
        gain: 1.20,
        ..Step::default()
    };
    pat_vox.rows[rv_deviant].steps[86] = Step {
        on: true,
        slice_index: Some(sl_sloppy),
        length: 0.90,
        gain: 1.10,
        ..Step::default()
    };

    // Bar 7-8 (steps 96..128 — Full-Band Grid Drop!): Rapid comedic cross-cutting!
    pat_vox.rows[rv_angry].steps[96] = Step {
        on: true,
        slice_index: Some(sl_take_tommy),
        length: 1.0,
        gain: 1.12,
        ..Step::default()
    };
    pat_vox.rows[rv_horse].steps[106] = Step {
        on: true,
        slice_index: Some(sl_whinny_stab),
        length: 1.0,
        gain: 0.95,
        ..Step::default()
    };
    pat_vox.rows[rv_angry].steps[110] = Step {
        on: true,
        slice_index: Some(sl_stut_help),
        length: 0.9,
        gain: 0.95,
        ..Step::default()
    };
    pat_vox.rows[rv_crazy].steps[112] = Step {
        on: true,
        slice_index: Some(sl_nobody),
        length: 1.0,
        gain: 1.18,
        ..Step::default()
    };
    pat_vox.rows[rv_horse].steps[122] = Step {
        on: true,
        slice_index: Some(sl_whinny_stab),
        length: 1.0,
        gain: 0.96,
        ..Step::default()
    };

    let pid_vox = p.add_pattern(pat_vox);
    let t_vox = p.add_track("dialogue call-and-response");
    p.tracks[t_vox].level = 1.22;
    p.tracks[t_vox].choke_group = 1;
    p.tracks[t_vox].eq.push(EqBand { freq: 2900.0, q: 1.0, gain_db: 2.8 }); // Clear speech presence
    p.tracks[t_vox].clips.push(Clip::new_pattern(pid_vox, 0.0, 32.0));
    // Duck brass chords under spoken dialogue so every punchline cuts through crystal-clearly!
    p.tracks[t_brass].duck = Some(DuckSpec {
        source_track: t_vox,
        amount: 0.62,
        attack: 0.006,
        release: 0.22,
        key_hz: 3500.0,
    });

    // Timed Master-Bus Performance Punches:
    // 1. Turntable Tape-Stop brake right after the Horse Whinny before the Radio Break!
    p.master_punches.push(MasterPunch {
        start_beat: 14.85,
        end_beat: 15.92,
        action: PunchAction::TapeStop { spin_down_beats: 0.72, spin_up_beats: 0.25 },
    });
    // 2. Signature `ISOL8` `LMF + MF + HMF SOLO` (`Mid` channel) Vintage Radio Break!
    let mut radio_iso = BandIsolator::default();
    radio_iso.solo = [false, true, true, true, false]; // LMF + MF + HMF SOLO!
    radio_iso.ch_mode = IsolatorChannelMode::Mid;
    radio_iso.output_db = 2.5;
    p.master_punches.push(MasterPunch {
        start_beat: 16.0,
        end_beat: 23.65,
        action: PunchAction::Isolator(radio_iso),
    });
    // 3. Climax dynamic stage switch (`Split` -> architectural `Grid`) on the Beat-24 Drop!
    p.master_punches.push(MasterPunch {
        start_beat: 24.0,
        end_beat: 32.0,
        action: PunchAction::Layout(Layout::Tile),
    });
    // 4. Tight closing Tape-Stop spin-down on the ending beat!
    p.master_punches.push(MasterPunch {
        start_beat: 31.25,
        end_beat: 32.0,
        action: PunchAction::TapeStop { spin_down_beats: 0.65, spin_up_beats: 0.10 },
    });

    p
}

// ============================================================================
// PIECE 2: `lemon-and-string_books-collage` (16:9 1920x1080 @ 60fps, ~18.6s)
// The Books intimate acoustic micro-collage + Karplus-Strong Pluck + polymeter
// ============================================================================

fn build_piece2_lemon_and_string() -> Project {
    let mut p = Project {
        bpm: 103.0,
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Split,
        layout_pan: true,
        studio_bus: true,
        captions_on: true,
        master_gain_db: 3.1,
        master_bloom: 0.10,
        ..Default::default()
    };
    p.set_aspect_hd(AspectRatio::Wide16x9);
    p.master_tape.warmth = 0.22;

    let s_wood_raw = load_yt(
        "p2_wooden_board_eup71wodKY8.mp4",
        "Chopping On Wooden Board",
        "Sound Effect Database",
        "20210929",
        490,
        "https://www.youtube.com/watch?v=eup71wodKY8",
        (4.0, 28.0),
    );

    let mut s_pages = load_yt(
        "p2_book_pages_WZhetc795TA.mp4",
        "Old Books & Page Turning",
        "latenightwhisperer",
        "20111210",
        6_807,
        "https://www.youtube.com/watch?v=WZhetc795TA",
        (180.0, 32.0),
    );
    let sl_page_turn = push_slice(&mut s_pages, 6.63, 6.88, SliceRole::Hat, "", Some((0.50, 0.56)));
    let sl_page_tap = push_slice(&mut s_pages, 11.65, 11.90, SliceRole::Snare, "", Some((0.50, 0.56)));
    let sl_page_riffle = push_slice(&mut s_pages, 30.65, 30.84, SliceRole::Hat, "", Some((0.50, 0.56)));

    // Pair wooden cutting-board foley audio with real page-turning hands video (zero static text card!)
    let mut s_wood = s_wood_raw.pair(&s_pages);
    let sl_wood_a = push_slice(&mut s_wood, 4.28, 4.38, SliceRole::Snare, "", Some((0.48, 0.54)));
    let sl_wood_b = push_slice(&mut s_wood, 7.48, 7.56, SliceRole::Snare, "", Some((0.52, 0.56)));
    let sl_wood_c = push_slice(&mut s_wood, 10.38, 10.46, SliceRole::Snare, "", Some((0.50, 0.52)));

    let mut s_fgtr = load_yt(
        "p2_fingerpick_guitar_EfZyjGF64wA.mp4",
        "Fingerpicking Riffs in C",
        "Jeff Segall",
        "20121012",
        77,
        "https://www.youtube.com/watch?v=EfZyjGF64wA",
        (2.5, 32.0),
    );
    s_fgtr.base_hz = 164.81; // E3
    let sl_fgtr_a = push_slice(&mut s_fgtr, 0.60, 1.08, SliceRole::Tonal, "", None);
    let sl_fgtr_b = push_slice(&mut s_fgtr, 29.28, 30.15, SliceRole::Tonal, "", None);

    let mut s_igtr = load_yt(
        "p2_img0238_guitar_oGJvKwLkcPs.mp4",
        "IMG 0238 Bedroom Guitar",
        "Ed Haare SemperPaternus",
        "20250726",
        1,
        "https://www.youtube.com/watch?v=oGJvKwLkcPs",
        (22.0, 28.0),
    );
    s_igtr.base_hz = 130.81; // C3
    let sl_igtr_thump = push_slice(&mut s_igtr, 11.35, 11.82, SliceRole::Kick, "", None);
    let sl_igtr_pluck = push_slice(&mut s_igtr, 16.32, 16.82, SliceRole::Tonal, "", None);

    let mut s_cello = load_yt(
        "p2_baroque_cello_I-lZe5ZMeZM.mp4",
        "Sarabande Baroque Cello",
        "Juliana Soltis",
        "20200930",
        166,
        "https://www.youtube.com/watch?v=I-lZe5ZMeZM",
        (156.5, 32.0),
    );
    s_cello.base_hz = 130.81; // C Major suite
    let sl_cello_low = push_slice(&mut s_cello, 6.25, 7.80, SliceRole::Tonal, "", None);
    let sl_cello_high = push_slice(&mut s_cello, 22.61, 24.55, SliceRole::Tonal, "", None);

    let mut s_jenny = load_yt(
        "p2_found_cassette_jenny_EA0X4C2shiw.mp4",
        "Found Cassette Jenny",
        "Audio Artifacts",
        "20260730",
        140,
        "https://www.youtube.com/watch?v=EA0X4C2shiw",
        (291.0, 42.0),
    );
    let sl_jenny_crisp = push_slice(
        &mut s_jenny,
        29.88,
        31.56,
        SliceRole::Phrase,
        "IT WAS NICE AND CRISP",
        None,
    );
    let sl_jenny_weather = push_slice(
        &mut s_jenny,
        24.00,
        25.85,
        SliceRole::Phrase,
        "THE WEATHER IS SO MISERABLE",
        None,
    );
    let sl_jenny_windy = push_slice(
        &mut s_jenny,
        40.60,
        41.96,
        SliceRole::Phrase,
        "COLD, RAINY AND WINDY",
        None,
    );

    let id_wood = p.add_source(s_wood);
    let id_pages = p.add_source(s_pages);
    let id_fgtr = p.add_source(s_fgtr);
    let id_igtr = p.add_source(s_igtr);
    let id_cello = p.add_source(s_cello);
    let id_jenny = p.add_source(s_jenny);

    let key = Key::new(0, ScaleKind::Major); // C Major

    // Track 0: Polymetric Acoustic Micro-Percussion (12-step Wood + 14-step Book Pages + Acoustic Body Pulse)
    let mut pat_perc = StepPattern::new("books polymetric foley");
    pat_perc.set_grid(4.0, 4);
    pat_perc.swing = 0.54;

    // Row 0: Warm woody guitar-body pulse (16 steps)
    let r_body = pat_perc.add_row(id_igtr);
    pat_perc.rows[r_body].sculpt = VoiceSculpt {
        pitch_semitones: -12.0,
        pitch_env_semitones: 11.0,
        pitch_env_ms: 36.0,
        sub_exciter: 0.52,
        drive_db: 4.5,
        hpf_hz: 32.0,
        lpf_hz: 165.0,
        ..VoiceSculpt::preset(Stamp::Kick)
    };
    pat_perc.rows[r_body].choke_group = 2;
    for &(st, g) in &[(0usize, 0.56f32), (6, 0.42), (10, 0.52)] {
        pat_perc.rows[r_body].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl_igtr_thump),
            length: 0.75,
            ..Step::default()
        };
    }

    // Row 1: 12-step Polymetric Wooden Cutting-Board Knocks!
    let r_wood = pat_perc.add_row(id_wood);
    pat_perc.rows[r_wood].set_steps(12);
    pat_perc.rows[r_wood].key = Some(key);
    pat_perc.rows[r_wood].sculpt = VoiceSculpt {
        snare_snap: 0.42,
        comb_mix: 0.28,
        comb_feedback: 0.86,
        hpf_hz: 210.0,
        lpf_hz: 9500.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    for &(st, sl, g, pt) in &[
        (0usize, sl_wood_a, 0.85f32, 0.0f32),
        (3, sl_wood_b, 0.65, 7.0),
        (5, sl_wood_c, 0.55, 12.0),
        (8, sl_wood_a, 0.88, 4.0),
        (10, sl_wood_b, 0.60, 9.0),
    ] {
        pat_perc.rows[r_wood].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: g,
            slice_index: Some(sl),
            length: 1.4,
            ..Step::default()
        };
    }

    // Row 2: 14-step Polymetric Book Page Turns & Paper Riffles!
    let r_pg = pat_perc.add_row(id_pages);
    pat_perc.rows[r_pg].set_steps(14);
    pat_perc.rows[r_pg].sculpt = VoiceSculpt {
        drive_db: 2.5,
        hpf_hz: 1650.0,
        lpf_hz: 16_500.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    for &(st, sl, g) in &[
        (2usize, sl_page_turn, 0.78f32),
        (6, sl_page_tap, 0.72),
        (9, sl_page_riffle, 0.76),
        (12, sl_page_turn, 0.64),
    ] {
        pat_perc.rows[r_pg].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl),
            length: 0.9,
            ..Step::default()
        };
    }

    let pid_perc = p.add_pattern(pat_perc);
    let t_perc = p.add_track("polymetric wood & pages");
    p.tracks[t_perc].level = 0.92;
    // Macro 3-second EBU R128 chamber dynamic arc (`LRA >= 3.8 LU`):
    // 0..8.5b: whisper solo intro + Jenny 1 -> 8.8..22.8b: full polymetric chamber bloom -> 23..32b: intimate cassette hush!
    p.tracks[t_perc].level_points = vec![
        AutoPoint { beat: 0.0, value: 0.14 },
        AutoPoint { beat: 8.2, value: 0.16 },
        AutoPoint { beat: 9.0, value: 1.00 },
        AutoPoint { beat: 22.6, value: 1.00 },
        AutoPoint { beat: 23.2, value: 0.16 },
        AutoPoint { beat: 32.0, value: 0.10 },
    ];
    p.tracks[t_perc].eq.push(EqBand { freq: 290.0, q: 1.4, gain_db: -5.5 });
    p.tracks[t_perc].eq.push(EqBand { freq: 5800.0, q: 0.9, gain_db: 2.8 });
    p.tracks[t_perc].clips.push(Clip::new_pattern(pid_perc, 0.0, 32.0));

    // Track 1: Interlocking Key-Locked Karplus-Strong `Pluck` Guitar Melodies (10-step vs 16-step!)
    let mut pat_gtr = StepPattern::new("karplus bedroom guitars");
    pat_gtr.set_grid(4.0, 4);
    pat_gtr.swing = 0.54;

    let r_fg = pat_gtr.add_row(id_fgtr);
    pat_gtr.rows[r_fg].set_steps(10); // 10-step melodic cycle phasing warmly over 4/4!
    pat_gtr.rows[r_fg].key = Some(key);
    pat_gtr.rows[r_fg].grains.quantize_amount = 1.0;
    pat_gtr.rows[r_fg].sculpt = VoiceSculpt {
        comb_mix: 0.66,
        comb_feedback: 0.935,
        hpf_hz: 165.0,
        lpf_hz: 9200.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Pluck)
    };
    for &(st, pt, sl, g) in &[
        (0usize, 0.0f32, sl_fgtr_a, 0.80f32),
        (2, 4.0, sl_fgtr_a, 0.70),
        (4, 7.0, sl_fgtr_b, 0.76),
        (7, 11.0, sl_fgtr_a, 0.72),
        (9, 7.0, sl_fgtr_a, 0.62),
    ] {
        pat_gtr.rows[r_fg].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: g,
            slice_index: Some(sl),
            length: 0.85,
            ..Step::default()
        };
    }

    let r_ig = pat_gtr.add_row(id_igtr); // 1-view YouTube bedroom guitar!
    pat_gtr.rows[r_ig].key = Some(key);
    pat_gtr.rows[r_ig].grains.quantize_amount = 1.0;
    pat_gtr.rows[r_ig].sculpt = VoiceSculpt {
        comb_mix: 0.58,
        comb_feedback: 0.92,
        chord: ChordVoicing::Off,
        hpf_hz: 195.0,
        lpf_hz: 8600.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Pluck)
    };
    for &(st, pt, chd, g) in &[
        (0usize, 0.0f32, ChordVoicing::Maj7, 0.66f32),
        (6, 5.0, ChordVoicing::Off, 0.62),
        (11, 7.0, ChordVoicing::Fifths, 0.60),
        (14, 9.0, ChordVoicing::Off, 0.56),
    ] {
        pat_gtr.rows[r_ig].steps[st] = Step {
            on: true,
            pitch: pt,
            chord: chd,
            gain: g,
            slice_index: Some(sl_igtr_pluck),
            length: 1.15,
            ..Step::default()
        };
    }

    let pid_gtr = p.add_pattern(pat_gtr);
    let t_gtr = p.add_track("karplus acoustic guitars");
    p.tracks[t_gtr].level = 0.86;
    p.tracks[t_gtr].level_points = vec![
        AutoPoint { beat: 0.0, value: 0.34 },
        AutoPoint { beat: 8.2, value: 0.34 },
        AutoPoint { beat: 9.0, value: 1.00 },
        AutoPoint { beat: 13.0, value: 0.58 },
        AutoPoint { beat: 16.2, value: 0.58 },
        AutoPoint { beat: 17.0, value: 1.05 },
        AutoPoint { beat: 22.6, value: 1.00 },
        AutoPoint { beat: 23.4, value: 0.32 },
        AutoPoint { beat: 32.0, value: 0.18 },
    ];
    // Carve 220-380 Hz bedroom guitar resonance so Cello + Jenny voice breathe with zero mud!
    p.tracks[t_gtr].eq.push(EqBand { freq: 295.0, q: 1.35, gain_db: -7.5 });
    p.tracks[t_gtr].eq.push(EqBand { freq: 5200.0, q: 0.95, gain_db: 3.4 });
    p.tracks[t_gtr].effects.push(AvEffect {
        kind: EffectKind::Delay {
            time: 60.0 / 103.0 * 0.75,
            feedback: 0.24,
            mix: 0.15,
            shift_x: 0.015,
            shift_y: 0.0,
        },
        audio: 1.0,
        video: 0.18,
    });
    p.tracks[t_gtr].clips.push(Clip::new_pattern(pid_gtr, 0.0, 32.0));

    // Track 2: Baroque Cello Lyrical Countermelody + Soft Split-Bleed Harmonic Wash
    let mut pat_cello = StepPattern::new("sarabande cello phrases");
    pat_cello.set_grid(8.0, 4); // 2-bar bowed phrases
    let rc = pat_cello.add_row(id_cello);
    pat_cello.rows[rc].key = Some(key);
    pat_cello.rows[rc].grains.quantize_amount = 1.0;
    pat_cello.rows[rc].grains.envelope = 0.18;
    pat_cello.rows[rc].sculpt = VoiceSculpt {
        shimmer: 0.22,
        wow_flutter: 0.12,
        hpf_hz: 145.0,
        lpf_hz: 7800.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    for &(st, pt, sl, g) in &[
        (0usize, 0.0f32, sl_cello_low, 0.78f32),
        (12, 4.0, sl_cello_high, 0.74),
        (20, -3.0, sl_cello_low, 0.76),
        (28, 2.0, sl_cello_high, 0.68),
    ] {
        pat_cello.rows[rc].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: g,
            slice_index: Some(sl),
            length: 1.05,
            ..Step::default()
        };
    }
    let pid_cello = p.add_pattern(pat_cello);
    let t_cello = p.add_track("baroque cello");
    p.tracks[t_cello].level = 0.84;
    p.tracks[t_cello].level_points = vec![
        AutoPoint { beat: 8.5, value: 0.0 },
        AutoPoint { beat: 9.2, value: 0.94 },
        AutoPoint { beat: 13.0, value: 0.45 },
        AutoPoint { beat: 16.2, value: 0.45 },
        AutoPoint { beat: 17.0, value: 1.00 },
        AutoPoint { beat: 22.6, value: 0.96 },
        AutoPoint { beat: 23.4, value: 0.22 },
        AutoPoint { beat: 32.0, value: 0.10 },
    ];
    p.tracks[t_cello].eq.push(EqBand { freq: 265.0, q: 1.3, gain_db: -8.5 });
    p.tracks[t_cello].eq.push(EqBand { freq: 4400.0, q: 1.0, gain_db: 2.8 });
    p.tracks[t_cello].effects.push(AvEffect {
        kind: EffectKind::Reverb { size: 0.64, damp: 0.46, mix: 0.22 },
        audio: 1.0,
        video: 0.25,
    });
    p.tracks[t_cello].clips.push(Clip::new_pattern(pid_cello, 8.5, 23.5));

    // Subtle full-bleed cello shimmer wash (`is_wash == true` so it bleeds softly at 30% in `Layout::Split`)
    let t_wash = p.add_track("cello harmonic bleed");
    p.tracks[t_wash].level = 0.20;
    let mut c_wash = Clip::new_snippet(id_cello, 8.5, 15.0);
    c_wash.key = Some(key);
    c_wash.grains.position = 22.61 / 32.0;
    c_wash.grains.gain = 0.42;
    c_wash.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(5.8),
        shimmer: 0.56,
        wow_flutter: 0.18,
        ebb_flow: 0.55,
        hpf_hz: 420.0,
        lpf_hz: 7200.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Wash)
    };
    p.tracks[t_wash].clips.push(c_wash);

    // Track 4: Found-Cassette "Jenny" Intimate Spoken Micro-Hooks (`0 st`, Choke Group 1, Captions)
    let mut pat_jenny = StepPattern::new("found cassette jenny");
    pat_jenny.set_grid(32.0, 4);
    let rj = pat_jenny.add_row(id_jenny);
    pat_jenny.rows[rj].choke_group = 1;
    pat_jenny.rows[rj].grains.quantize_amount = 0.0;
    pat_jenny.rows[rj].sculpt = VoiceSculpt {
        drive_db: 2.2,
        hpf_hz: 220.0,
        lpf_hz: 12_500.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Phrase)
    };
    pat_jenny.rows[rj].steps[20] = Step {
        on: true,
        slice_index: Some(sl_jenny_crisp),
        length: 1.0,
        gain: 0.72,
        ..Step::default()
    };
    pat_jenny.rows[rj].steps[52] = Step {
        on: true,
        slice_index: Some(sl_jenny_weather),
        length: 1.0,
        gain: 1.15,
        ..Step::default()
    };
    pat_jenny.rows[rj].steps[96] = Step {
        on: true,
        slice_index: Some(sl_jenny_windy),
        length: 1.0,
        gain: 0.76,
        ..Step::default()
    };
    let pid_jenny = p.add_pattern(pat_jenny);
    let t_jenny = p.add_track("found cassette voice");
    p.tracks[t_jenny].level = 0.96;
    p.tracks[t_jenny].choke_group = 1;
    p.tracks[t_jenny].eq.push(EqBand { freq: 290.0, q: 1.4, gain_db: -3.5 });
    p.tracks[t_jenny].eq.push(EqBand { freq: 3200.0, q: 1.0, gain_db: 3.2 });
    p.tracks[t_jenny].clips.push(Clip::new_pattern(pid_jenny, 0.0, 32.0));

    p
}

// ============================================================================
// PIECE 3: `kinetic-prism_ratchet-grid` (1:1 1080x1080 @ 60fps, ~17.14s)
// Aphex Twin x UKG kinetic ratchet + high-Q resonant sweeps + polar Kaleido
// ============================================================================

fn build_piece3_kinetic_prism() -> Project {
    let mut p = Project {
        bpm: 140.0,
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Tile, // Crisp 1:1 architectural Grid
        layout_pan: true,
        studio_bus: true,
        captions_on: false,
        master_gain_db: 3.8,
        master_bloom: 0.20,
        ..Default::default()
    };
    p.set_aspect_hd(AspectRatio::Square1x1);

    let mut s_foot = load_yt(
        "p3_house_footwork_Bldi_Ph-oDw.mp4",
        "House Dance Footwork",
        "PRINCE DANCE TUTORIALS",
        "20260618",
        959,
        "https://www.youtube.com/watch?v=Bldi_Ph-oDw",
        (49.5, 28.0),
    );
    let sl_sub_a = push_slice(&mut s_foot, 19.32, 19.52, SliceRole::Kick, "", Some((0.68, 0.46)));
    let sl_sub_b = push_slice(&mut s_foot, 20.32, 20.58, SliceRole::Kick, "", Some((0.68, 0.46)));

    let mut s_anvil = load_yt(
        "p3_anvil_rhythm_uQVeyOplSj0.mp4",
        "Blacksmith Anvil Rhythm",
        "windfarmer78",
        "20151014",
        99,
        "https://www.youtube.com/watch?v=uQVeyOplSj0",
        (2.0, 26.0),
    );
    let sl_anv_snare = push_slice(&mut s_anvil, 3.60, 3.84, SliceRole::Snare, "", Some((0.50, 0.52)));
    let sl_anv_hat = push_slice(&mut s_anvil, 12.47, 12.62, SliceRole::Hat, "", Some((0.50, 0.52)));
    let sl_anv_ping = push_slice(&mut s_anvil, 16.23, 16.48, SliceRole::Snare, "", Some((0.50, 0.52)));

    let mut s_pan = load_yt(
        "p3_handpan_slap_X3xMP7zyq3M.mp4",
        "Handpan Shoulder Slaps",
        "Rosie Bergonzi",
        "20210323",
        444,
        "https://www.youtube.com/watch?v=X3xMP7zyq3M",
        (295.5, 30.0),
    );
    s_pan.base_hz = 164.81;
    // Strictly inside 3.5..19.5s live performance window (zero Patreon end-credits!)
    let sl_pan_low = push_slice(&mut s_pan, 3.55, 4.08, SliceRole::Tonal, "", Some((0.54, 0.46)));
    let sl_pan_hi = push_slice(&mut s_pan, 13.45, 14.05, SliceRole::Tonal, "", Some((0.54, 0.46)));

    let mut s_crt = load_yt(
        "p3_crt_lissajous_p5Wi2hcncHg.mp4",
        "CRT Oscilloscope Lissajous",
        "RadioTrefoil",
        "20150125",
        17_733,
        "https://www.youtube.com/watch?v=p5Wi2hcncHg",
        (10.5, 30.0),
    );
    s_crt.base_hz = 440.0;
    let sl_crt_a = push_slice(&mut s_crt, 3.95, 4.65, SliceRole::Tonal, "", None);
    let sl_crt_b = push_slice(&mut s_crt, 23.10, 23.85, SliceRole::Tonal, "", None);

    let id_foot = p.add_source(s_foot);
    let id_anvil = p.add_source(s_anvil);
    let id_pan = p.add_source(s_pan);
    let id_crt = p.add_source(s_crt);

    let key = Key::new(6, ScaleKind::Minor); // F# Minor

    // Track 0: Swung UKG Sub Pocket (`swing = 0.60`)
    let mut pat_ukg = StepPattern::new("ukg sub kick");
    pat_ukg.set_grid(4.0, 4);
    pat_ukg.swing = 0.60;
    let rk = pat_ukg.add_row(id_foot);
    pat_ukg.rows[rk].choke_group = 2;
    pat_ukg.rows[rk].sculpt = VoiceSculpt {
        pitch_semitones: -14.0,
        pitch_env_semitones: 18.0,
        pitch_env_ms: 40.0,
        sub_exciter: 0.86,
        drive_db: 8.8,
        hpf_hz: 28.0,
        lpf_hz: 148.0,
        resonance_q: 0.95,
        ..VoiceSculpt::preset(Stamp::Kick)
    };
    pat_ukg.rows[rk].grains.adsr = Some(Adsr {
        attack: 0.002,
        decay: 0.20,
        sustain: 0.0,
        release: 0.05,
        picture: PictureCut::Cut,
        cut_threshold: 0.08,
        ..Adsr::default()
    });
    for &(st, sl, g, ev_n, ev_off) in &[
        (0usize, sl_sub_a, 1.06f32, 1u8, 0u8),
        (6, sl_sub_b, 0.92, 1, 0),
        (11, sl_sub_a, 0.96, 1, 0),
        (14, sl_sub_b, 0.78, 2, 1),
    ] {
        pat_ukg.rows[rk].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl),
            every_n: ev_n,
            every_offset: ev_off,
            ..Step::default()
        };
    }
    let pid_ukg = p.add_pattern(pat_ukg);
    let t_ukg = p.add_track("ukg footwork sub");
    p.tracks[t_ukg].level = 0.78;
    // 8-beat UKG club breakdown (`beats 14..22`, ~3.4s) before the Kaleido sub-drop (`LRA >= 3.2 LU`)!
    let mut c_ukg1 = Clip::new_pattern(pid_ukg, 0.0, 14.0);
    c_ukg1.transform.scale = 1.85;
    let mut c_ukg2 = Clip::new_pattern(pid_ukg, 22.0, 18.0);
    c_ukg2.transform.scale = 1.85;
    p.tracks[t_ukg].clips.push(c_ukg1);
    p.tracks[t_ukg].clips.push(c_ukg2);

    // Track 1: Anvil UKG Rim/Snare + Ratcheting Metallic Hats (`×3`, `×4`)
    let mut pat_anv = StepPattern::new("anvil ukg tops");
    pat_anv.set_grid(4.0, 4);
    pat_anv.swing = 0.60;
    let rs = pat_anv.add_row(id_anvil);
    pat_anv.rows[rs].sculpt = VoiceSculpt {
        snare_snap: 0.86,
        drive_db: 7.5,
        hpf_hz: 190.0,
        lpf_hz: 9600.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Snare)
    };
    pat_anv.rows[rs].grains.adsr = Some(Adsr {
        attack: 0.001,
        decay: 0.14,
        sustain: 0.0,
        release: 0.04,
        picture: PictureCut::Cut,
        cut_threshold: 0.08,
        ..Adsr::default()
    });
    pat_anv.rows[rs].steps[4] = Step {
        on: true,
        gain: 0.98,
        slice_index: Some(sl_anv_snare),
        ..Step::default()
    };
    pat_anv.rows[rs].steps[12] = Step {
        on: true,
        gain: 1.02,
        slice_index: Some(sl_anv_snare),
        ..Step::default()
    };
    pat_anv.rows[rs].steps[15] = Step {
        on: true,
        gain: 0.72,
        slice_index: Some(sl_anv_ping),
        ratchet: 4,
        ratchet_pitch: -2.0,
        every_n: 2,
        every_offset: 1,
        ..Step::default()
    };

    let rh = pat_anv.add_row(id_anvil);
    pat_anv.rows[rh].choke_group = 3;
    pat_anv.rows[rh].sculpt = VoiceSculpt {
        pitch_semitones: 9.0,
        drive_db: 3.5,
        hpf_hz: 5800.0,
        lpf_hz: 18_500.0,
        ..VoiceSculpt::preset(Stamp::Hat)
    };
    for &(st, g, rat, rp) in &[
        (2usize, 0.74f32, 1u8, 0.0f32),
        (5, 0.52, 1, 0.0),
        (7, 0.64, 3, 1.5),
        (10, 0.76, 1, 0.0),
        (13, 0.56, 1, 0.0),
        (14, 0.68, 3, -1.5),
    ] {
        pat_anv.rows[rh].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(sl_anv_hat),
            length: 0.55,
            ratchet: rat,
            ratchet_pitch: rp,
            ..Step::default()
        };
    }

    let pid_anv = p.add_pattern(pat_anv);
    let t_anv = p.add_track("anvil ukg percussion");
    p.tracks[t_anv].level = 1.15;
    p.tracks[t_anv].eq.push(EqBand { freq: 265.0, q: 1.3, gain_db: -5.0 });
    p.tracks[t_anv].eq.push(EqBand { freq: 1950.0, q: 0.9, gain_db: 4.8 });
    p.tracks[t_anv].clips.push(Clip::new_pattern(pid_anv, 0.0, 40.0));

    // Track 2: Aphex Bouncing-Ball Ratchet Handpan (`×3 / ×4 / ×6` + High-Q Resonant Sweeps!)
    // Build 6 steep resonant cutoff stages (`1350 -> 2400 -> 4200 -> 7200 -> 2600 -> 5800 Hz` @ `Q = 5.4`)
    // across the 40-beat timeline so crisp 800 Hz-4.5 kHz acid/IDM ratchets cut right through!
    let cutoffs = [
        (0.0, 6.0, 1350.0f32),
        (6.0, 6.0, 2400.0),
        (12.0, 8.0, 4200.0),
        (20.0, 8.0, 7200.0),
        (28.0, 8.0, 2600.0),
        (36.0, 4.0, 5800.0),
    ];
    let t_pan = p.add_track("aphex ratchet handpan");
    p.tracks[t_pan].level = 1.25;
    p.tracks[t_pan].eq.push(EqBand { freq: 275.0, q: 1.3, gain_db: -8.5 });
    p.tracks[t_pan].eq.push(EqBand { freq: 1650.0, q: 0.85, gain_db: 6.2 });
    p.tracks[t_pan].eq.push(EqBand { freq: 3400.0, q: 1.0, gain_db: 4.5 });
    p.tracks[t_pan].duck = Some(DuckSpec {
        source_track: t_ukg,
        amount: 0.45,
        attack: 0.005,
        release: 0.14,
        key_hz: 155.0,
    });
    for &(b0, blen, lpf) in &cutoffs {
        let mut pat_p = StepPattern::new("handpan ratchets");
        pat_p.set_grid(8.0, 4); // 32 steps
        pat_p.swing = 0.60;
        let rp = pat_p.add_row(id_pan);
        pat_p.rows[rp].key = Some(key);
        pat_p.rows[rp].grains.quantize_amount = 1.0;
        pat_p.rows[rp].grains.adsr = Some(Adsr {
            attack: 0.002,
            decay: 0.18,
            sustain: 0.18,
            release: 0.07,
            picture: PictureCut::Cut,
            cut_threshold: 0.08,
            ..Adsr::default()
        });
        pat_p.rows[rp].sculpt = VoiceSculpt {
            comb_mix: 0.54,
            comb_feedback: 0.92,
            hpf_hz: 320.0,
            lpf_hz: lpf,
            resonance_q: 5.4, // Singing high-Q acid/IDM resonant filter sweep!
            drive_db: 5.5,
            pocket_carve: true,
            ..VoiceSculpt::preset(Stamp::Pluck)
        };
        for &(st, pt, sl, rat, r_slope, g) in &[
            (0usize, 0.0f32, sl_pan_low, 1u8, 0.0f32, 0.85f32),
            (3, 3.0, sl_pan_low, 3, 2.0, 0.78),
            (8, 7.0, sl_pan_hi, 1, 0.0, 0.80),
            (11, 5.0, sl_pan_low, 4, -1.5, 0.82),
            (14, 10.0, sl_pan_hi, 6, 1.5, 0.86), // ×6 Aphex bouncing-ball roll + flipX!
            (18, 0.0, sl_pan_low, 1, 0.0, 0.82),
            (22, 7.0, sl_pan_hi, 3, 2.5, 0.78),
            (26, 12.0, sl_pan_hi, 4, -2.0, 0.84),
            (30, 3.0, sl_pan_low, 6, 1.0, 0.88),
        ] {
            pat_p.rows[rp].steps[st] = Step {
                on: true,
                pitch: pt,
                gain: g,
                slice_index: Some(sl),
                length: 0.85,
                ratchet: rat,
                ratchet_pitch: r_slope,
                ..Step::default()
            };
        }
        let pid = p.add_pattern(pat_p);
        p.tracks[t_pan].clips.push(Clip::new_pattern(pid, b0, blen));
    }

    // Track 3: CRT Oscilloscope Lissajous Phosphor Chords & Diving Drills
    let mut pat_crt = StepPattern::new("crt phosphor chords");
    pat_crt.set_grid(8.0, 4);
    pat_crt.swing = 0.60;
    let rc = pat_crt.add_row(id_crt);
    pat_crt.rows[rc].key = Some(key);
    pat_crt.rows[rc].grains.quantize_amount = 1.0;
    pat_crt.rows[rc].grains.adsr = Some(Adsr {
        attack: 0.005,
        decay: 0.28,
        sustain: 0.35,
        release: 0.12,
        picture: PictureCut::Cut,
        cut_threshold: 0.08,
        ..Adsr::default()
    });
    pat_crt.rows[rc].sculpt = VoiceSculpt {
        pitch_semitones: 0.0, // Crystalline 440-880 Hz mid register (zero 220 Hz mud!)
        chord: ChordVoicing::Min9,
        shimmer: 0.28,
        hpf_hz: 380.0,
        lpf_hz: 9600.0,
        resonance_q: 2.6,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Chord)
    };
    for &(st, pt, sl, chd, rat, r_slope, g) in &[
        (0usize, 0.0f32, sl_crt_a, ChordVoicing::Min9, 1u8, 0.0f32, 0.62f32),
        (6, 5.0, sl_crt_b, ChordVoicing::Fifths, 1, 0.0, 0.56),
        (15, 7.0, sl_crt_a, ChordVoicing::Off, 4, -2.0, 0.60),
        (16, -2.0, sl_crt_b, ChordVoicing::Maj7, 1, 0.0, 0.62),
        (24, 3.0, sl_crt_a, ChordVoicing::Sus4, 1, 0.0, 0.58),
        (29, 10.0, sl_crt_b, ChordVoicing::Off, 4, 1.5, 0.58),
    ] {
        pat_crt.rows[rc].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: g,
            slice_index: Some(sl),
            chord: chd,
            length: 0.90,
            ratchet: rat,
            ratchet_pitch: r_slope,
            ..Step::default()
        };
    }
    let pid_crt = p.add_pattern(pat_crt);
    let t_crt = p.add_track("crt oscilloscope chords");
    p.tracks[t_crt].level = 1.02;
    p.tracks[t_crt].eq.push(EqBand { freq: 270.0, q: 1.2, gain_db: -8.5 });
    p.tracks[t_crt].eq.push(EqBand { freq: 1850.0, q: 1.0, gain_db: 3.8 });
    p.tracks[t_crt].duck = Some(DuckSpec {
        source_track: t_ukg,
        amount: 0.48,
        attack: 0.005,
        release: 0.16,
        key_hz: 155.0,
    });
    p.tracks[t_crt].clips.push(Clip::new_pattern(pid_crt, 0.0, 40.0));
    // Bright full-canvas sunlit handpan + CRT dual prism bed across `beats 20..36` so the 6-fold
    // spinning polar `Kaleido` folds vibrant warm color + glowing neon vector geometry (high-passed
    // >= 520 Hz at whisper audio gain so zero 180-450 Hz mud enters the mix!).
    let t_prism = p.add_track("kaleido prism bed");
    p.tracks[t_prism].level = 0.85;
    let mut c_prism = Clip::new_snippet(id_pan, 20.0, 16.0);
    c_prism.key = Some(key);
    c_prism.grains.position = 5.5 / 30.0;
    c_prism.grains.pitch = 12.0;
    c_prism.grains.gain = 0.08;
    c_prism.link.gain_to_opacity = 0.0;
    c_prism.visual.additive = 0.35;
    c_prism.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(3.2),
        shimmer: 0.45,
        hpf_hz: 540.0,
        lpf_hz: 9500.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::TapeLoop)
    };
    p.tracks[t_prism].clips.push(c_prism);

    // Master Punches:
    // 1. Mid-track `ISOL8` Breakdown (`LMF + MF + HMF SOLO` resonant acid focus during `beats 15..20`!)
    let mut scoop_iso = BandIsolator::default();
    scoop_iso.solo = [false, true, true, true, false];
    p.master_punches.push(MasterPunch {
        start_beat: 15.0,
        end_beat: 20.0,
        action: PunchAction::Isolator(scoop_iso),
    });
    // 2. Switch to feathered `Split` + Spinning 6-fold Polar `Kaleido` AV Prism (`beats 20.0..36.0`) -> crisp `Grid` (`36..40`)!
    p.master_punches.push(MasterPunch {
        start_beat: 20.0,
        end_beat: 36.0,
        action: PunchAction::Layout(Layout::Split),
    });
    p.master_punches.push(MasterPunch {
        start_beat: 20.0,
        end_beat: 36.0,
        action: PunchAction::Effect(AvEffect {
            kind: EffectKind::Kaleido { folds: 6.0, spin: 0.38 },
            audio: 0.65,
            video: 0.98,
        }),
    });

    p
}

// ============================================================================
// PIECE 4: `oxide-ballroom_disintegration-loops` (16:9 1920x1080 @ 60fps, 25.6s)
// Basinski / The Caretaker asynchronous free_loop_secs + Disintegrate + HF MUTE
// ============================================================================

fn build_piece4_oxide_ballroom() -> Project {
    let mut p = Project {
        bpm: 75.0, // 32 beats = 25.60s
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Bleed, // Optical luma-key + screen multi-exposure memory melt
        layout_pan: false,
        studio_bus: true,
        captions_on: false,
        master_gain_db: 1.4,
        master_bloom: 0.08,
        ..Default::default()
    };
    p.set_aspect_hd(AspectRatio::Wide16x9);
    p.master_tape.warmth = 0.38;
    p.master_tape.wow = 0.26;
    p.master_tape.age = 0.16; // Clear golden opening exposure before progressive oxide decay!

    let mut s_hol = load_yt(
        "p4_victrola_holiday_0YQ-Ya-LYPU.mp4",
        "1940 Victrola Horn Phonograph",
        "Mitch Devine",
        "20110715",
        3_966,
        "https://www.youtube.com/watch?v=0YQ-Ya-LYPU",
        (31.5, 34.0),
    );
    s_hol.base_hz = 311.13; // Eb4

    // Watermark-free 1920 sunlit lawn waltz archival film (2,020 views, zero timecodes or archive logos!)
    let mut s_ball = load_yt(
        "p4_waltz_1920_HnBT-jPRNBU.mp4",
        "People Dance a Waltz on a Lawn in 1920",
        "Walter Nelson (Wallywood Pictures)",
        "20210510",
        2_020,
        "https://www.youtube.com/watch?v=HnBT-jPRNBU",
        (12.0, 34.0),
    );
    s_ball.base_hz = 207.65; // Ab3

    let mut s_pno_raw = load_yt(
        "p4_detuned_piano_wAhwj_kNZIA.mp4",
        "Antique Out-of-Tune Piano",
        "Stringless Piano",
        "20160322",
        9_271,
        "https://www.youtube.com/watch?v=wAhwj_kNZIA",
        (142.5, 34.0),
    );
    s_pno_raw.base_hz = 246.94;
    // Pair antique out-of-tune piano audio with clean 1920 waltz film so zero channel text appears!
    let s_pno = s_pno_raw.pair(&s_ball);

    let s_sup8 = load_yt(
        "p4_super8_projector_4rWesEobV38.mp4",
        "1960s Super8 Projector",
        "coolbluelights",
        "20081022",
        2_604,
        "https://www.youtube.com/watch?v=4rWesEobV38",
        (111.5, 28.0),
    );

    let mut s_jazz = load_yt(
        "p4_victrola_1930_jazz_9QNikxbXuYM.mp4",
        "1930 Credenza Victrola Jazz",
        "Bruce Victrolaman Young",
        "20090202",
        14_479,
        "https://www.youtube.com/watch?v=9QNikxbXuYM",
        (30.5, 34.0),
    );
    s_jazz.base_hz = 293.66;
    // Pair 1930 Victrola brass horn audio with Super 8mm sprockets & warm gate flicker!
    let s_jazz_sup8 = s_jazz.pair(&s_sup8);

    let id_hol = p.add_source(s_hol);
    let id_ball = p.add_source(s_ball);
    let id_pno = p.add_source(s_pno);
    let id_js8 = p.add_source(s_jazz_sup8);

    let key = Key::new(3, ScaleKind::Major); // Eb Major nostalgic ballroom key

    // Loop 1 (`3.4s` free-running period — ~7.5 passes): Victrola Ballroom Melody eroding into oxide flakes
    let t0 = p.add_track("victrola 3.4s loop");
    p.tracks[t0].level = 0.88;
    p.tracks[t0].level_points = vec![
        AutoPoint { beat: 0.0, value: 0.0 },
        AutoPoint { beat: 1.5, value: 0.90 },
        AutoPoint { beat: 28.5, value: 0.84 },
        AutoPoint { beat: 32.0, value: 0.12 },
    ];
    let mut c0 = Clip::new_snippet(id_hol, 0.0, 32.0);
    c0.key = Some(key);
    c0.grains.position = 3.68 / 34.0;
    c0.grains.pitch = -5.0; // Warm Caretaker slowed-turntable register
    c0.grains.varispeed = true;
    c0.grains.pan = -0.22;
    c0.grains.seed = 101;
    c0.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(3.4),
        wow_flutter: 0.58,
        disintegrate: 0.84,
        ebb_flow: 0.58,
        shimmer: 0.34,
        drive_db: 3.0,
        hpf_hz: 135.0,
        lpf_hz: 4900.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Disintegrate)
    };
    p.tracks[t0].clips.push(c0);

    // Loop 2 (`5.1s` free-running period — ~5.0 passes): 1920s Sunlit Lawn Waltz Couple
    let t1 = p.add_track("ballroom dancers 5.1s loop");
    p.tracks[t1].level = 0.90;
    p.tracks[t1].level_points = vec![
        AutoPoint { beat: 0.0, value: 0.85 },
        AutoPoint { beat: 2.5, value: 1.00 },
        AutoPoint { beat: 14.0, value: 0.96 },
        AutoPoint { beat: 26.0, value: 0.58 },
        AutoPoint { beat: 32.0, value: 0.12 },
    ];
    let mut c1 = Clip::new_snippet(id_ball, 0.0, 32.0);
    c1.key = Some(key);
    c1.grains.position = 5.5 / 34.0;
    c1.grains.pitch = -3.0;
    c1.grains.varispeed = true;
    c1.grains.pan = 0.24;
    c1.grains.seed = 202;
    c1.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(5.1),
        wow_flutter: 0.52,
        disintegrate: 0.82,
        ebb_flow: 0.72,
        shimmer: 0.44,
        drive_db: 2.5,
        hpf_hz: 140.0,
        lpf_hz: 4600.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Disintegrate)
    };
    c1.visual.additive = 0.22;
    c1.effects.push(AvEffect {
        kind: EffectKind::Tint { hue: 36.0, amount: 0.24 },
        audio: 0.0,
        video: 1.0,
    });
    p.tracks[t1].clips.push(c1);

    // Loop 3 (`7.3s` free-running period — ~3.5 passes): Out-of-tune Antique Upright Piano + `+12/+7st` Shimmer
    let t2 = p.add_track("detuned piano 7.3s loop");
    p.tracks[t2].level = 0.76;
    p.tracks[t2].level_points = vec![
        AutoPoint { beat: 1.5, value: 0.0 },
        AutoPoint { beat: 5.0, value: 0.78 },
        AutoPoint { beat: 28.0, value: 0.76 },
        AutoPoint { beat: 32.0, value: 0.14 },
    ];
    let mut c2 = Clip::new_snippet(id_pno, 1.5, 30.5);
    c2.key = Some(key);
    c2.grains.position = 11.20 / 34.0;
    c2.grains.pitch = -7.0; // Deep resonant parlor low-mids + sub warmth
    c2.grains.varispeed = true;
    c2.grains.pan = -0.10;
    c2.grains.seed = 303;
    c2.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(7.3),
        wow_flutter: 0.64,
        disintegrate: 0.74,
        ebb_flow: 0.68,
        shimmer: 0.50,
        drive_db: 2.2,
        hpf_hz: 75.0,
        lpf_hz: 4400.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Disintegrate)
    };
    p.tracks[t2].clips.push(c2);

    // Loop 4 (`4.3s` free-running period — ~5.8 passes): Super 8mm Gate + 1930s Horn Ghost
    let t3 = p.add_track("super8 memory 4.3s loop");
    p.tracks[t3].level = 0.58;
    p.tracks[t3].level_points = vec![
        AutoPoint { beat: 3.0, value: 0.0 },
        AutoPoint { beat: 7.0, value: 0.60 },
        AutoPoint { beat: 27.0, value: 0.55 },
        AutoPoint { beat: 32.0, value: 0.06 },
    ];
    let mut c3 = Clip::new_snippet(id_js8, 3.0, 29.0);
    c3.key = Some(key);
    c3.grains.position = 14.36 / 34.0;
    c3.grains.pitch = -5.0;
    c3.grains.varispeed = true;
    c3.grains.pan = 0.16;
    c3.grains.seed = 404;
    c3.grains.sculpt = VoiceSculpt {
        free_loop_secs: Some(4.3),
        wow_flutter: 0.56,
        disintegrate: 0.80,
        ebb_flow: 0.66,
        shimmer: 0.30,
        drive_db: 2.8,
        hpf_hz: 160.0,
        lpf_hz: 3800.0,
        pocket_carve: true,
        ..VoiceSculpt::preset(Stamp::Disintegrate)
    };
    p.tracks[t3].clips.push(c3);

    // Master Cathedral Hall Tail (HPF-cleaned by studio_bus)
    p.master_effects.push(AvEffect {
        kind: EffectKind::Reverb { size: 0.92, damp: 0.58, mix: 0.30 },
        audio: 1.0,
        video: 0.22,
    });

    // Progressive `ISOL8` Darkening Across Passes:
    // Stage 1 (`beats 15.5..23.5`): `HF MUTE` — gramophone horn air begins to vanish into tape dust
    let mut hf_mute = BandIsolator::default();
    hf_mute.mute = [false, false, false, false, true];
    p.master_punches.push(MasterPunch {
        start_beat: 15.5,
        end_beat: 23.5,
        action: PunchAction::Isolator(hf_mute),
    });
    // Stage 2 (`beats 23.5..32.0`): `HF + HMF MUTE` — deep Caretaker memory submersion + optical darkening
    let mut deep_mute = BandIsolator::default();
    deep_mute.mute = [false, false, false, true, true];
    p.master_punches.push(MasterPunch {
        start_beat: 23.5,
        end_beat: 32.0,
        action: PunchAction::Isolator(deep_mute),
    });

    p
}

// ============================================================================
// PIECE 5: `modell5-pulse_portrait-bursts` (16:9 1920x1080 @ 60fps, 16.5s)
// Granular-Synthesis — MODELL 5 staccato 3x1 Portrait Triptych (18–45 Hz buzz!)
// ============================================================================

fn build_piece5_modell5_pulse() -> Project {
    let mut p = Project {
        bpm: 120.0, // 33 beats = 16.50s (2 beats = 1.00s!)
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Tile, // Crisp architectural 3x1 Portrait Triptych with 1.5px obsidian dividers!
        portrait_columns: true,
        layout_pan: true,
        studio_bus: true,
        captions_on: false,
        master_gain_db: 1.1,
        master_bloom: 0.16,
        ..Default::default()
    };
    p.set_aspect_hd(AspectRatio::Wide16x9);

    let mut s_bari_raw = load_yt(
        "p5_closeup_baritone_Vpv8jFV_-ZI.mp4",
        "Baritone Close-Up",
        "The Song Company",
        "20200327",
        434,
        "https://www.youtube.com/watch?v=Vpv8jFV_-ZI",
        (72.0, 28.0),
    );
    s_bari_raw.base_hz = 110.0; // A2

    let s_opera_vis = load_yt(
        "p5_opera_closeup_XPCANtdso1Q.mp4",
        "Opera Close-Up Portrait",
        "Opera Parallèle",
        "20210324",
        293,
        "https://www.youtube.com/watch?v=XPCANtdso1Q",
        (12.0, 32.0),
    );
    // Pair deep 110 Hz baritone voice with front-facing close-up singer portrait (`3.5..5.4s`, `focus = (0.65, 0.23)`)
    let mut s_bari = s_bari_raw.pair(&s_opera_vis);
    let _ = push_slice(&mut s_bari, 0.0, 28.0, SliceRole::Tonal, "", Some((0.65, 0.23)));

    let mut s_drama = load_yt(
        "p5_closeup_drama_nbBruTwcxvs.mp4",
        "Emotional Close-Up",
        "Becca Fox",
        "20180812",
        170,
        "https://www.youtube.com/watch?v=nbBruTwcxvs",
        (156.0, 32.0),
    );
    s_drama.base_hz = 220.0; // A3
    let _ = push_slice(&mut s_drama, 6.0, 28.0, SliceRole::Phrase, "", Some((0.50, 0.36)));

    let mut s_sop = load_yt(
        "p5_closeup_soprano_xGz_lKfLFdg.mp4",
        "Operatic Close-Up",
        "Best Of Opera",
        "20261017",
        72,
        "https://www.youtube.com/watch?v=xGz_lKfLFdg",
        (275.5, 28.0),
    );
    s_sop.base_hz = 293.66; // D4
    // Lock strictly to tight bearded face close-up window (`14.85..15.68s`) & zoom past corner TV logo!
    let _ = push_slice(&mut s_sop, 0.0, 28.0, SliceRole::Tonal, "", Some((0.50, 0.50)));

    let id_bari = p.add_source(s_bari);
    let id_drama = p.add_source(s_drama);
    let id_sop = p.add_source(s_sop);

    let hard_adsr = Adsr {
        attack: 0.001,
        decay: 0.05,
        sustain: 1.0,
        release: 0.004,
        picture: PictureCut::Cut,
        cut_threshold: 0.05,
        ..Adsr::default()
    };

    // Helper to build one MODELL 5 synchronous audio-rate portrait monolith
    #[allow(clippy::too_many_arguments)]
    let make_monolith = |src_id: usize,
                         start_beat: f64,
                         len_beats: f64,
                         face_zoom: f32,
                         pos_pts: &[(f64, f64)],
                         dens_pts: &[(f64, f64)],
                         dur_pts: &[(f64, f64)],
                         pitch_pts: &[(f64, f64)],
                         sculpt: VoiceSculpt,
                         seed: u64|
     -> Clip {
        let mut c = Clip::new(src_id, start_beat, len_beats);
        c.transform.scale = face_zoom;
        c.grains = GrainSettings {
            density: 18.0,
            duration: 0.032,
            duration_jitter: 0.0, // Synchronous pulse train!
            position: pos_pts[0].1 as f32,
            spray: 0.004, // Micro-frozen vocal vowel/expression (never drifts across cuts)!
            scan_speed: 0.0,
            pitch: 0.0,
            quantize_amount: 0.0,
            gain: 0.98,
            envelope: 0.14,
            env_skew: -0.45,
            seed,
            adsr: Some(hard_adsr),
            sculpt,
            ..Default::default()
        };
        c.visual.flicker = 1.0; // Zero visual smear: hard frame shutter!
        c.visual.feather = 0.0;
        c.visual.additive = 0.0;

        let mut l_pos = AutomationLane::new("position");
        for &(b, v) in pos_pts {
            l_pos.set_point(b, v);
        }
        let mut l_dens = AutomationLane::new("density");
        for &(b, v) in dens_pts {
            l_dens.set_point(b, v);
        }
        let mut l_dur = AutomationLane::new("duration");
        for &(b, v) in dur_pts {
            l_dur.set_point(b, v);
        }
        let mut l_pt = AutomationLane::new("pitch");
        for &(b, v) in pitch_pts {
            l_pt.set_point(b, v);
        }
        c.automation = vec![l_pos, l_dens, l_dur, l_pt];
        c
    };

    // Monolith 0 (Left): Baritone (`sub_exciter` gives physical chest impact to every frame cut!)
    let t0 = p.add_track("monolith left · baritone");
    p.tracks[t0].level = 0.98;
    p.tracks[t0].clips.push(make_monolith(
        id_bari,
        0.0,
        32.8,
        2.35,
        &[
            (0.0, 3.65 / 28.0),
            (10.0, 4.15 / 28.0),
            (18.0, 4.55 / 28.0),
            (26.0, 3.85 / 28.0),
        ],
        &[
            (0.0, 12.0),   // Crisp authoritative machine shutter from beat 0!
            (4.0, 19.5),   // Threshold of pitch!
            (10.0, 27.5),  // A0 audio-rate buzz
            (16.0, 36.71), // D1 harmonic buzz
            (22.0, 27.5),  // 5:6:7.5 polyrhythmic beating
            (27.0, 41.20), // E1 climax buzz
            (31.5, 7.5),   // De-escalation back to discrete breath clicks
        ],
        &[
            (0.0, 0.048),
            (4.0, 0.032),
            (10.0, 0.025),
            (16.0, 0.020),
            (27.0, 0.018),
            (31.5, 0.046),
        ],
        &[(0.0, 0.0), (16.0, -2.0), (24.0, 0.0), (28.0, -5.0)],
        VoiceSculpt {
            sub_exciter: 0.48,
            drive_db: 4.6,
            hpf_hz: 36.0,
            lpf_hz: 12_500.0,
            pocket_carve: true,
            ..VoiceSculpt::default()
        },
        501,
    ));

    // Monolith 1 (Center): Dramatic Close-Up (`enters beat 1.5`, phases against Left!)
    let t1 = p.add_track("monolith center · drama");
    p.tracks[t1].level = 0.98;
    p.tracks[t1].clips.push(make_monolith(
        id_drama,
        1.5,
        31.3,
        1.42,
        &[
            (0.0, 9.92 / 32.0),
            (8.5, 15.42 / 32.0),
            (16.5, 20.72 / 32.0),
            (24.5, 21.10 / 32.0),
        ],
        &[
            (0.0, 13.5),
            (4.5, 22.0),   // Phasing against 19.5 Hz!
            (10.5, 33.0),  // C1
            (16.5, 43.65), // F1 audio-rate vocal buzz!
            (20.5, 33.0),  // 6-ratio beating
            (25.5, 44.0),  // A1 octave lock!
            (30.0, 6.5),
        ],
        &[
            (0.0, 0.042),
            (4.5, 0.028),
            (10.5, 0.022),
            (16.5, 0.017),
            (25.5, 0.017),
            (30.0, 0.044),
        ],
        &[(0.0, 0.0), (12.0, 3.0), (20.0, -2.0), (26.0, 5.0)],
        VoiceSculpt {
            drive_db: 4.2,
            hpf_hz: 130.0,
            lpf_hz: 13_500.0,
            resonance_q: 1.6,
            pocket_carve: true,
            ..VoiceSculpt::default()
        },
        502,
    ));

    // Monolith 2 (Right): Operatic Close-Up (`enters beat 3.0` -> locks full 3x1 Portrait Triptych!)
    let t2 = p.add_track("monolith right · opera");
    p.tracks[t2].level = 0.95;
    p.tracks[t2].clips.push(make_monolith(
        id_sop,
        3.0,
        29.8,
        1.65,
        &[
            (0.0, 14.84 / 28.0),
            (8.0, 15.05 / 28.0),
            (15.0, 15.25 / 28.0),
            (23.0, 14.95 / 28.0),
        ],
        &[
            (0.0, 14.0),
            (3.0, 24.0),
            (9.0, 36.71),
            (15.0, 45.0), // Peak 45 Hz audio-rate frame oscillation!
            (19.0, 41.25),
            (24.0, 38.5),
            (28.5, 6.0),
        ],
        &[
            (0.0, 0.034),
            (3.0, 0.026),
            (9.0, 0.019),
            (15.0, 0.015),
            (24.0, 0.018),
            (28.5, 0.044),
        ],
        &[(0.0, 0.0), (10.0, 5.0), (18.0, 7.0), (25.0, 0.0)],
        VoiceSculpt {
            drive_db: 3.2,
            hpf_hz: 165.0,
            lpf_hz: 14_000.0,
            pocket_carve: true,
            ..VoiceSculpt::default()
        },
        503,
    ));

    // Brief `ISOL8` `LF + MF + HMF SOLO` mid-installation stark vocal presence focus (`beats 19..23`)
    let mut stark_iso = BandIsolator::default();
    stark_iso.solo = [true, false, true, true, false];
    stark_iso.output_db = 1.5;
    p.master_punches.push(MasterPunch {
        start_beat: 19.0,
        end_beat: 23.0,
        action: PunchAction::Isolator(stark_iso),
    });

    // Subtle tight room reflection so the 3 monoliths inhabit a cavernous gallery space
    p.master_effects.push(AvEffect {
        kind: EffectKind::Reverb { size: 0.52, damp: 0.55, mix: 0.14 },
        audio: 1.0,
        video: 0.0,
    });

    p
}

// ============================================================================
// PIECE 6: `isol8-studio_vj-showcase` (16:9 1920x1080 @ 30fps, ~52.5s)
// Headless `egui_kittest` + `wgpu` tour of the anodized sand `ISOL8` console
// ============================================================================

fn prepare_real_yt_stub_dir() -> PathBuf {
    let stub_dir = hillclimb_root().join("work/yt-stub");
    let _ = std::fs::remove_dir_all(&stub_dir);
    std::fs::create_dir_all(&stub_dir).expect("create yt-stub dir");

    let items = [
        (
            "vwVHhrj-doE",
            "p1_angry_boy_1950_vwVHhrj-doE.mp4",
            "Angry Boy Part I (1950) - Child Guidance Clinic",
            "OVR | Old Videos Restored",
            "20260531",
            79u64,
            40.0f64,
        ),
        (
            "uC9B2F-l5V8",
            "p1_mental_fit_1952_uC9B2F-l5V8.mp4",
            "Mental Health: Keeping Mentally Fit (1952)",
            "OVR | Old Videos Restored",
            "20251003",
            24,
            42.0,
        ),
        (
            "EA0X4C2shiw",
            "p2_found_cassette_jenny_EA0X4C2shiw.mp4",
            "Found Cassette - Jenny",
            "Audio Artifacts",
            "20260730",
            140,
            42.0,
        ),
        (
            "I-lZe5ZMeZM",
            "p2_baroque_cello_I-lZe5ZMeZM.mp4",
            "Sarabande - Baroque Cello (BWV 1009)",
            "Juliana Soltis",
            "20200930",
            166,
            32.0,
        ),
    ];

    let mut search_lines = String::new();
    for (id, file, title, uploader, date, views, dur) in items {
        let src_mp4 = src_dir().join(file);
        let dst_mp4 = stub_dir.join(format!("{id}.mp4"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&src_mp4, &dst_mp4).expect("symlink real yt mp4");
        #[cfg(not(unix))]
        std::fs::copy(&src_mp4, &dst_mp4).expect("copy real yt mp4");

        let meta = format!(
            "{{\"id\":\"{id}\",\"title\":\"{title}\",\"uploader\":\"{uploader}\",\"upload_date\":\"{date}\",\"view_count\":{views},\"duration\":{dur:.1},\"license\":\"Standard YouTube License\",\"webpage_url\":\"https://www.youtube.com/watch?v={id}\"}}\n"
        );
        std::fs::write(stub_dir.join(format!("{id}.json")), &meta).expect("write id.json");
        search_lines.push_str(&meta);
    }
    std::fs::write(stub_dir.join("search.jsonl"), search_lines).expect("write search.jsonl");
    stub_dir
}

fn p6_after_yt_import(app: &mut App) {
    // Enrich the freshly fetched Angry Boy (1950) clip on Column 4 with our word-exact caption slice
    // so `Kit -> pads` harvests `"CALLS FOR PROFESSIONAL HELP"` onto a Perform phrase pad!
    let mut p = app.project.lock().unwrap();
    if let Some((idx, s)) = p.sources.iter_mut().enumerate().last() {
        s.name = "Angry Boy 1950".into();
        let sl_idx = push_slice(
            s,
            0.44,
            2.95,
            SliceRole::Phrase,
            "CALLS FOR PROFESSIONAL HELP",
            Some((0.48, 0.40)),
        ) as usize;
        drop(p);
        app.studio
            .slice_captions
            .insert((idx, sl_idx), "CALLS FOR PROFESSIONAL HELP".into());
        app.studio.emitters[0].col = 3;
        app.studio.emitters[0].row = 1.2;
        app.studio.sel_emitter = 0;
    }
}

fn p6_pause_for_audition(app: &mut App) {
    if app.playback.is_playing() {
        chromagrain_ui::Host::toggle_play(app);
    }
}

fn p6_fire_stamp_audition(app: &mut App, stamp: Stamp, dur_s: f32) {
    let now = app.app_clock.secs();
    app.live_id += 1;
    let voice = 0x57A0_0000_0000 | (app.live_id & 0xFFFF_FFFF);
    let (src_idx, t0, base_hz) = {
        let p = app.project.lock().unwrap();
        let idx = p.sources.len().saturating_sub(1);
        let hz = p.sources.get(idx).map(|s| s.effective_base_hz()).unwrap_or(220.0);
        let pos = match stamp {
            Stamp::Kick => 0.55,
            Stamp::Chord => 1.15,
            Stamp::Disintegrate => 0.44,
            _ => 0.44,
        };
        (idx, pos, hz)
    };
    let mut sculpt = VoiceSculpt::preset(stamp);
    if stamp == Stamp::Chord {
        sculpt.chord = ChordVoicing::Min9;
        sculpt.pitch_semitones = -5.0;
    } else if stamp == Stamp::Disintegrate {
        sculpt.wow_flutter = 0.62;
        sculpt.disintegrate = 0.85;
        sculpt.shimmer = 0.42;
    }
    let adsr = Adsr {
        attack: 0.004,
        decay: 0.16,
        sustain: 0.85,
        release: 0.22,
        picture: PictureCut::Fade,
        ..Adsr::default()
    };
    let off = now + (dur_s as f64 - 0.18).max(0.18);
    let ev = chromagrain_core::grain::GrainEvent {
        onset: now,
        source_pos: t0,
        duration: dur_s,
        pitch_ratio: chromagrain_core::music::semitones_to_ratio(sculpt.pitch_semitones),
        gain: 0.96,
        pan: 0.0,
        envelope: 0.02,
        id: voice,
        tag: chromagrain_core::grain::VoiceTag {
            voice,
            env: Some(chromagrain_core::envelope::VoiceEnv { adsr, on: now, off }),
            focus: (0.48, 0.40),
            oxide_dropout: if stamp == Stamp::Disintegrate { 0.65 } else { 0.0 },
            zoom_punch: if stamp == Stamp::Kick { 0.16 } else { 0.05 },
            ..Default::default()
        },
        sculpt,
        ..Default::default()
    };
    let key = app.studio.key;
    let (chord_evs, chord_n) =
        chromagrain_core::grain::expand_chord_grain_inline(&ev, sculpt.chord, base_hz, Some(&key));
    for cev in chord_evs.into_iter().take(chord_n) {
        chromagrain_ui::Host::fire(app, 0, src_idx, cev);
    }
}

fn p6_audition_kick(app: &mut App) {
    p6_fire_stamp_audition(app, Stamp::Kick, 0.95);
}

fn p6_audition_chord(app: &mut App) {
    p6_fire_stamp_audition(app, Stamp::Chord, 1.15);
}

fn p6_audition_disintegrate(app: &mut App) {
    p6_fire_stamp_audition(app, Stamp::Disintegrate, 1.85);
}

fn p6_prime_perform_groove(app: &mut App) {
    // Immediate pad launch (`LaunchQ::Now`), unmute looping Perform pads (15 & 16) so multi-clip
    // video & tape loops run continuously alongside the live `SET` sequence (`p.tracks[PADS]`),
    // and ensure Pad 11 carries our spoken hook caption!
    app.launch_q = crate::LaunchQ::Now;
    for i in 10..14 {
        if app.studio.pad_captions[i].is_empty() {
            app.studio.pad_captions[i] = "CALLS FOR PROFESSIONAL HELP".into();
            break;
        }
    }
    {
        let mut p = app.project.lock().unwrap();
        for loop_pad in [14usize, 15] {
            if let Some(tr) = p.tracks.get_mut(loop_pad) {
                if !tr.clips.is_empty() {
                    tr.muted = false;
                }
            }
        }
        if let Some(seq_tr) = p.tracks.get_mut(crate::PADS) {
            seq_tr.muted = false;
        }
    }
    app.mark_now();
    if !app.playback.is_playing() {
        chromagrain_ui::Host::toggle_play(app);
    }
}

fn harness_isol8_showcase(opts: &RecordOpts) -> Harness<'static, Film> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stub = manifest.join("../../tools/ytdlp-stub");
    let stub_dir = prepare_real_yt_stub_dir();
    std::env::set_var(chromagrain_core::dig::YTDLP_ENV, &stub);
    std::env::set_var("CHROMAGRAIN_YTDLP_STUB_DIR", &stub_dir);

    // Pre-decode 3 real YouTube clips for Columns 1-3 (zero oscillators, zero synthetic media!)
    let mut s0 = load_yt(
        "p3_house_footwork_Bldi_Ph-oDw.mp4",
        "Footwork Sub",
        "PRINCE DANCE",
        "20260618",
        959,
        "https://www.youtube.com/watch?v=Bldi_Ph-oDw",
        (49.5, 17.8),
    );
    // Trim off 17.8..28.0s YouTuber spoken outro ("smashing that like button" + SUBSCRIBE banner)
    // and crop frames to [0.45, 0.08, 0.50, 0.76] past "RECAP / YOUR ANGLE" & bottom captions!
    if let Some(a_arc) = s0.audio.as_mut() {
        let mut a = (**a_arc).clone();
        let keep = ((17.8 * a.sample_rate as f64) as usize).min(a.samples.len());
        a.samples.truncate(keep);
        if let Some(side) = a.side.as_mut() {
            side.truncate(keep);
        }
        *a_arc = std::sync::Arc::new(a);
    }
    if let Some(v_arc) = s0.video.as_mut() {
        let mut v = (**v_arc).clone();
        let keep_vf = ((17.8 * v.fps as f64) as usize).min(v.frames.len()).max(1);
        v.frames.truncate(keep_vf);
        for f in &mut v.frames {
            let x0 = ((f.width as f32 * 0.45) as u32).min(f.width.saturating_sub(2));
            let y0 = ((f.height as f32 * 0.08) as u32).min(f.height.saturating_sub(2));
            let cw = (((f.width as f32 * 0.50) as u32) & !1).clamp(2, f.width - x0);
            let ch = (((f.height as f32 * 0.76) as u32) & !1).clamp(2, f.height - y0);
            let mut cropped = chromagrain_core::video::Frame::black(cw, ch);
            for y in 0..ch {
                for x in 0..cw {
                    cropped.put(x, y, f.get(x0 + x, y0 + y));
                }
            }
            *f = cropped;
        }
        *v_arc = std::sync::Arc::new(v);
    }
    s0.slices = s0.auto_slices(16);
    let _ = push_slice(&mut s0, 17.28, 17.55, SliceRole::Kick, "", Some((0.50, 0.50)));

    let s1_raw = load_yt(
        "p1_funk_drums_fnTaVyzWYjI.mp4",
        "Funk Groove",
        "TIMTIM RECORDS",
        "20230217",
        265_399,
        "https://www.youtube.com/watch?v=fnTaVyzWYjI",
        (19.5, 28.0),
    );
    let s1_vis = load_yt(
        "p1_turntable_scratch_rtqTmUVjsuY.mp4",
        "Vinyl Scratches",
        "ScratchDJAcademy",
        "20140819",
        1_160_631,
        "https://www.youtube.com/watch?v=rtqTmUVjsuY",
        (150.5, 30.0),
    );
    // Pair Funk Groove snare audio with live turntable hands video so every stage panel moves!
    let mut s1 = s1_raw.pair(&s1_vis);
    let _ = push_slice(&mut s1, 8.31, 8.45, SliceRole::Snare, "", Some((0.52, 0.52)));

    let mut s2 = load_yt(
        "p1_trumpet_fanfare_T_Kj30n-g9E.mp4",
        "Trumpet Fanfare",
        "NY Philharmonic",
        "20220328",
        53_101,
        "https://www.youtube.com/watch?v=T_Kj30n-g9E",
        (6.0, 30.0),
    );
    s2.base_hz = 493.88;
    let _ = push_slice(&mut s2, 16.12, 16.60, SliceRole::Tonal, "", None);

    let yt_catalog = vec![
        YoutubeHit {
            title: "Angry Boy Part I (1950) – Child Guidance Clinic".into(),
            url: "https://www.youtube.com/watch?v=vwVHhrj-doE".into(),
            views: Some(79),
            duration_secs: Some(40.0),
            uploader: "OVR | Old Videos Restored".into(),
        },
        YoutubeHit {
            title: "Mental Health: Keeping Mentally Fit (1952)".into(),
            url: "https://www.youtube.com/watch?v=uC9B2F-l5V8".into(),
            views: Some(24),
            duration_secs: Some(42.0),
            uploader: "OVR | Old Videos Restored".into(),
        },
        YoutubeHit {
            title: "Found Cassette - \"Jenny\"".into(),
            url: "https://www.youtube.com/watch?v=EA0X4C2shiw".into(),
            views: Some(140),
            duration_secs: Some(42.0),
            uploader: "Audio Artifacts".into(),
        },
        YoutubeHit {
            title: "Sarabande – Baroque Cello (BWV 1009)".into(),
            url: "https://www.youtube.com/watch?v=I-lZe5ZMeZM".into(),
            views: Some(166),
            duration_secs: Some(32.0),
            uploader: "Juliana Soltis".into(),
        },
    ];

    Harness::builder()
        .with_size(egui::vec2(opts.size.0, opts.size.1))
        .with_pixels_per_point(opts.ppp)
        .with_step_dt(1.0 / FPS as f32)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = App::new(AppOpts { demo: false, classic: false, headless: true });
            app.playback = crate::playback::Playback::new_offline();
            app.playback.offline_latency = 1.0 / FPS;
            app.app_clock = Clock::Sim(0.0);
            app.adds.sim_clock = Some(0.0);
            app.adds.yt_mock = Some(yt_catalog.clone());
            app.studio.install_theme(&cc.egui_ctx);
            app.studio.key = Key::new(4, ScaleKind::Minor);
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.18);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }

            {
                let mut p = app.project.lock().unwrap();
                p.bpm = 100.0;
                p.width = 640;
                p.height = 360;
                p.layout = Layout::Split;
                p.sources.clear();
                p.tracks.clear();
                p.patterns.clear();
                p.add_source(s0.clone());
                p.add_source(s1.clone());
                p.add_source(s2.clone());

                // Build two named Sequence cues (`groove` and `drop`) on the real YouTube clips
                // so the Perform `SET` bar has live multi-scene cues ready to launch!
                let mut pat1 = StepPattern::new("groove");
                pat1.set_grid(4.0, 4);
                pat1.swing = 0.57;
                for li in 0..chromagrain_ui::studio::LANES {
                    let src = if li < 3 { li } else { usize::MAX };
                    let r = pat1.add_row(src);
                    pat1.rows[r].key = Some(Key::new(4, ScaleKind::Minor));
                }
                pat1.rows[0].sculpt = VoiceSculpt::preset(Stamp::Kick);
                for st in [0usize, 7, 10] {
                    pat1.rows[0].steps[st] = Step { on: true, gain: 0.95, ..Step::default() };
                }
                pat1.rows[1].sculpt = VoiceSculpt::preset(Stamp::Snare);
                for st in [4usize, 12] {
                    pat1.rows[1].steps[st] = Step { on: true, gain: 0.92, ..Step::default() };
                }
                pat1.rows[2].sculpt = VoiceSculpt::preset(Stamp::Chord);
                for &(st, pt) in &[(0usize, -12.0f32), (6, -9.0), (12, -5.0)] {
                    pat1.rows[2].steps[st] = Step { on: true, pitch: pt, gain: 0.65, ..Step::default() };
                }

                let mut pat2 = pat1.clone();
                pat2.name = "climax".into();
                pat2.rows[1].steps[15] = Step {
                    on: true,
                    gain: 0.78,
                    ratchet: 3,
                    ratchet_pitch: 2.0,
                    ..Step::default()
                };
                pat2.rows[2].steps[14] = Step {
                    on: true,
                    pitch: -7.0,
                    gain: 0.72,
                    chord: ChordVoicing::Min9,
                    ..Step::default()
                };

                let pid1 = p.add_pattern(pat1);
                let pid2 = p.add_pattern(pat2);
                let ti = p.add_track("sequence");
                let mut sc1 = Clip::new_pattern(pid1, 0.0, 16.0);
                sc1.visual.feather = 0.3;
                sc1.visual.additive = 0.3;
                let mut sc2 = Clip::new_pattern(pid2, 16.0, 16.0);
                sc2.visual.feather = 0.3;
                sc2.visual.additive = 0.3;
                p.tracks[ti].clips = vec![sc1, sc2];
                drop(p);

                app.loop_on = true;
                app.loop_start = 0.0;
                app.loop_end = 32.0;
                app.playback.set_loop(app.loop_region());
                app.studio.source_focus.insert(0, (0.68, 0.46));

                for c in 0..3 {
                    app.studio.columns[c] = Some(c);
                    app.studio.seq.lanes[c] = Some(c);
                }
                app.studio.lane_sculpt[0] =
                    chromagrain_ui::SculptParams::from_core(Stamp::Kick, VoiceSculpt::preset(Stamp::Kick), 2);
                app.studio.lane_sculpt[1] =
                    chromagrain_ui::SculptParams::from_core(Stamp::Snare, VoiceSculpt::preset(Stamp::Snare), 0);
                app.studio.lane_sculpt[2] =
                    chromagrain_ui::SculptParams::from_core(Stamp::Chord, VoiceSculpt::preset(Stamp::Chord), 0);
                app.studio.seq.scene_pats[0] = Some(pid1);
                app.studio.seq.scene_pats[1] = Some(pid2);
                app.studio.seq.scene_names[0] = "groove".into();
                app.studio.seq.scene_names[1] = "climax".into();
                app.studio.seq.chain = vec![(0, 4), (1, 4)];
                app.studio.emitters[0].col = 2;
                app.studio.emitters[0].row = 2.2;
                app.studio.emitters[1].col = 1;
                app.studio.emitters[1].row = 1.6;
                app.studio.rev += 1;
            }
            app.mark_now();
            Film { app, overlay: Overlay::default() }
        })
}

fn tour_isol8_showcase() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(FilmCaption { t0, t1, text });
    let em = |i: usize| Aim::Emitter(i);
    let rows = |i: usize, path: &[(f32, f32)]| -> Vec<Aim> {
        path.iter()
            .map(|(dc, dr)| Aim::Off(Box::new(em(i)), *dc, *dr))
            .collect()
    };

    // Act 1 (0.0..5.5s): Anodized sand console · multi-touch real YouTube footage -> start live groove!
    at(0.35, Touch(rows(0, &[(0.0, 0.0), (0.03, 0.8), (0.0, 1.5)]), 4.3));
    at(1.35, Touch(rows(1, &[(0.0, 0.0), (-0.03, 0.7), (0.0, 1.3)]), 3.3));
    say(2.0, 5.1, "touch real youtube clips · sound and split-screen picture");
    at(4.85, Tap(lab("play"))); // Keep live 3-lane YouTube groove rolling while crate-digging!

    // Act 2 (5.8..14.4s): Add -> YouTube `<500 views` crate digger over the live beat!
    at(5.8, Tap(lab("column 4")));
    say(6.1, 9.6, "add -> youtube · crate-dig under 500 views");
    at(6.5, Tap(lab("add Link")));
    at(7.2, Tap(lab("youtube search")));
    at(7.6, Paste("1950 guidance clinic"));
    at(8.1, Tap(lab("low views")));
    at(8.7, Tap(lab("search youtube")));
    say(10.4, 14.1, "79 views · trimmed & sliced with zero ai");
    at(10.5, Tap(lab("youtube result 1")));
    at(12.2, Custom(p6_after_yt_import));
    at(13.2, Tap(lab("close sheet")));

    // Act 3 (14.6..22.2s): Open Clip -> Audible `Sculpt & Tape` stamps (`Kick`, `Chord`, `Disintegrate`) -> `Kit -> pads`
    at(14.55, Custom(p6_pause_for_audition));
    at(14.8, Tap(lab("open clip")));
    say(15.1, 19.3, "sculpt & tape · kick, chord & disintegrate stamps");
    at(16.0, Tap(lab("sculpt Kick")));
    at(16.05, Custom(p6_audition_kick));
    at(17.4, Tap(lab("sculpt Chord")));
    at(17.45, Custom(p6_audition_chord));
    at(18.8, Tap(lab("sculpt Disintegrate")));
    at(18.85, Custom(p6_audition_disintegrate));
    say(20.1, 23.8, "kit -> pads · 16 pure-sample pads across every clip");
    at(21.4, Tap(lab("kit to pads")));
    at(22.0, Custom(p6_prime_perform_groove));

    // Act 4 (23.0..41.2s): Continuous Perform groove + looping video pads + `ISOL8` + `Kaleido` / `Stutter` / `Tape Stop`
    at(23.1, Tap(lab("pad 1")));
    at(24.0, Tap(lab("pad 7")));
    say(24.7, 28.4, "spoken hook pads · choke group 1 + stage captions");
    at(25.2, Tap(lab("pad 11")));
    at(27.8, Tap(lab("pad 7")));

    // Punch `ISOL8` 24 dB/Oct Linkwitz-Riley switches (`LMF + MF + HMF SOLO` -> `LF SOLO` -> full band)
    say(29.2, 33.4, "isol8 · 24 db/oct mid-band radio solo & deep sub solo");
    at(29.3, Tap(lab("isol8 solo lmf")));
    at(29.65, Tap(lab("isol8 solo mf")));
    at(30.0, Tap(lab("isol8 solo hmf")));
    at(30.4, Tap(lab("pad 11"))); // Spoken 1950 dialogue right through the mid-band radio solo!
    // Release mid solos & punch LF SOLO for deep sub check!
    at(31.7, Tap(lab("isol8 solo lmf")));
    at(32.0, Tap(lab("isol8 solo mf")));
    at(32.3, Tap(lab("isol8 solo hmf")));
    at(32.6, Tap(lab("isol8 solo lf")));
    // Release LF SOLO -> full spectrum slams back!
    at(33.7, Tap(lab("isol8 solo lf")));

    // Backlit Amber VJ Punch-In FX: Shimmer (6-fold spinning polar Kaleido!), Stutter, and varispeed Tape Stop!
    say(34.3, 38.8, "polar kaleido · beat-repeat stutter · varispeed tape stop");
    at(34.35, Tap(lab("punch shimmer")));
    at(34.9, Tap(lab("pad 7")));
    at(35.8, Hold(lab("punch stutter"), 0.85));
    at(37.15, Hold(lab("punch tape stop"), 1.05));
    at(38.75, Tap(lab("punch shimmer"))); // Disengage Kaleido cleanly

    // Act 5 (39.6..52.5s): Launch Cue 2 on `SET` bar -> `Grid` -> Fullscreen -> HUD strip-back -> End card
    say(39.7, 42.9, "set cues · launch the climax & cut to fullscreen");
    at(39.8, Tap(lab("chain 2")));
    at(40.7, Tap(lab("layout tile")));
    at(41.6, Tap(lab("pad 11")));
    at(42.8, Tap(lab("Fullscreen")));

    say(44.8, 48.0, "tap for the hud · trigger pads over the live projection");
    at(45.0, Tap(Aim::Screen(0.5, 0.44)));
    at(45.9, Tap(lab("mini pad 7")));
    at(47.0, Tap(lab("mini pad 11")));

    Tour {
        cues,
        captions: cap,
        title_sub: "isol8 av grain console  ·  pure youtube",
        end_sub: "dig  ·  sculpt  ·  isol8  ·  perform",
        title_until: 1.9,
        end_card: 48.8,
        len: 52.5,
    }
}

fn mux_showcase_hd(
    frames: &Path,
    wav: &Path,
    out: &Path,
    fade_from: f64,
    _len: f64,
) -> Result<(), String> {
    let comp = "acompressor=threshold=-25dB:ratio=2.2:attack=25:release=300:knee=6";
    // Clean fade-out finishing BEFORE `end_card` (`48.8s`) + hard mute after `48.78s` so the
    // closing `chromagrain` title card (`0:48.8..0:52.5`) has zero audio bleed!
    let fade_st = (fade_from - 1.25).max(1.0);
    let fade = format!(
        "afade=t=out:st={:.2}:d=1.05,volume=enable='gte(t,{:.2})':volume=0",
        fade_st,
        fade_st + 1.08
    );
    let af = format!(
        "{comp},{fade},loudnorm=I=-14.6:TP=-2.4:LRA=9,aresample=192000,alimiter=limit=0.7328:attack=1.5:release=35:level=false,aresample=48000"
    );
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            "30",
            "-i",
        ])
        .arg(frames.join("%05d.png"))
        .arg("-i")
        .arg(wav)
        .args([
            "-af",
            &af,
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-maxrate",
            "6500k",
            "-bufsize",
            "13000k",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "256k",
            "-movflags",
            "+faststart",
            "-shortest",
        ])
        .arg(out)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with {st}"))
    }
}

fn render_piece(slug: &str, variant: &str, version: &str, proj: &Project) -> PathBuf {
    let dir = out_dir(version);
    std::fs::create_dir_all(&dir).expect("create out dir");
    let out = dir.join(format!("261113_chromagrain_{slug}_{variant}_{version}.mp4"));
    assert!(
        !out.exists(),
        "{} already exists — never overwrite a version",
        out.display()
    );
    let t0 = std::time::Instant::now();
    let end_beat = proj.song_end_beat();
    render_project_to_video(proj, 0.0, end_beat, &out)
        .unwrap_or_else(|e| panic!("render {slug} failed: {e}"));
    eprintln!(
        "[vj-hillclimb] wrote {} ({}x{} @ {:.0}fps, {:.2}s) in {:.1}s",
        out.display(),
        proj.width,
        proj.height,
        proj.fps,
        proj.beats_to_secs(proj.song_end_beat()),
        t0.elapsed().as_secs_f64()
    );
    out
}

fn render_piece6_showcase(version: &str) -> PathBuf {
    let dir = out_dir(version);
    std::fs::create_dir_all(&dir).expect("create out dir");
    let out = dir.join(format!(
        "261113_chromagrain_isol8-studio_vj-showcase_{version}.mp4"
    ));
    assert!(
        !out.exists(),
        "{} already exists — never overwrite a version",
        out.display()
    );
    let work = hillclimb_root().join(version).join("work");
    let frames = work.join("frames");
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::create_dir_all(&frames).expect("create frames dir");

    let tour = tour_isol8_showcase();
    let t0 = std::time::Instant::now();
    let mut h = harness_isol8_showcase(&RecordOpts {
        size: (1600.0, 900.0),
        ppp: 1.2,
        frames_dir: Some(frames.clone()),
        max_secs: None,
    });
    for _ in 0..6 {
        h.input_mut().time = Some(0.0);
        h.step();
    }
    let rec = record_with(
        &tour,
        &RecordOpts {
            size: (1600.0, 900.0),
            ppp: 1.2,
            frames_dir: Some(frames.clone()),
            max_secs: None,
        },
        h,
        None,
    );
    eprintln!(
        "[vj-hillclimb] piece6 recorded {} frames ({}x{}) in {:.1}s",
        rec.frames,
        rec.px.0,
        rec.px.1,
        t0.elapsed().as_secs_f64()
    );
    let wav = work.join(format!(
        "261113_chromagrain_isol8-studio_vj-showcase_{version}.wav"
    ));
    write_wav(&wav, &rec.audio, rec.sample_rate);
    mux_showcase_hd(&frames, &wav, &out, tour.end_card + 0.2, tour.len).expect("mux piece 6");
    // Protect disk space: delete temporary PNG sequence & scratch wav immediately!
    std::fs::remove_dir_all(&frames).expect("remove temporary frames");
    let _ = std::fs::remove_file(&wav);
    eprintln!("[vj-hillclimb] wrote {}", out.display());
    out
}

// ============================================================================
// TESTS & RENDER ENTRYPOINTS
// ============================================================================

#[test]
fn isol8_showcase_tour_is_well_formed() {
    let tour = tour_isol8_showcase();
    let mut pointer: Vec<(f64, f64)> = tour
        .cues
        .iter()
        .filter(|c| {
            matches!(
                c.g,
                Gesture::Tap(_) | Gesture::Hold(..) | Gesture::Drag(..) | Gesture::Knob(..)
            )
        })
        .map(|c| {
            let d = match &c.g {
                Gesture::Tap(_) => 0.14,
                Gesture::Hold(_, s) | Gesture::Drag(_, s) => *s,
                Gesture::Knob(_, keys) => keys.last().map_or(0.0, |k| k.0),
                _ => 0.0,
            };
            (c.t, c.t + d)
        })
        .collect();
    pointer.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in pointer.windows(2) {
        assert!(
            w[0].1 + 1.0 / FPS < w[1].0,
            "pointer gestures overlap: {:.2}..{:.2} vs {:.2}",
            w[0].0,
            w[0].1,
            w[1].0
        );
    }
    let mut caps: Vec<(f64, f64)> = tour.captions.iter().map(|c| (c.t0, c.t1)).collect();
    caps.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in caps.windows(2) {
        assert!(
            w[0].1 + 0.7 <= w[1].0 + 1e-6,
            "captions crowd at {:.2}s",
            w[1].0
        );
    }
    assert!(tour.captions.iter().all(|c| c.text == c.text.to_lowercase()));
    assert!((48.0..=56.0).contains(&tour.len));
}

#[test]
#[ignore = "inspects real YouTube crate slices"]
fn vj_inspect_crate() {
    let dir = src_dir();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("src dir")
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mp4"))
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let audio = media::decode_audio(&p, 48_000).expect("decode audio");
        let base_hz =
            chromagrain_core::dsp::detect_pitch(&audio.samples, audio.sample_rate).unwrap_or(0.0);
        let mut src = Source {
            name: name.clone(),
            audio: Some(std::sync::Arc::new(audio)),
            video: None,
            base_hz,
            ..Default::default()
        };
        let slices: Vec<Slice> = src.auto_slices(20);
        src.slices = slices.clone();
        println!(
            "\n=== {} (dur={:.2}s, base_hz={:.1}Hz, slices={}) ===",
            name,
            src.duration(),
            base_hz,
            slices.len()
        );
        for (i, s) in slices.iter().enumerate() {
            println!(
                "  [{:2}] {:5.2}s..{:5.2}s (len={:4.2}s) role={:<7} rms={:5.1}dB punch={:.2} f0={:>5} conf={:.2}",
                i,
                s.start,
                s.end,
                s.len(),
                chop::SliceRole::name(s.role_hint),
                s.rms_db,
                s.punch,
                s.f0_hz.map_or("-".into(), |h| format!("{h:.0}")),
                s.pitch_confidence,
            );
        }
    }
}

#[test]
#[ignore = "renders piece 1 (frontier-therapy)"]
fn vj_render_p1() {
    let v = version_tag();
    render_piece("frontier-therapy", "dialogue-collage", &v, &build_piece1_frontier_therapy());
}

#[test]
#[ignore = "renders piece 2 (lemon-and-string)"]
fn vj_render_p2() {
    let v = version_tag();
    render_piece("lemon-and-string", "books-collage", &v, &build_piece2_lemon_and_string());
}

#[test]
#[ignore = "renders piece 3 (kinetic-prism)"]
fn vj_render_p3() {
    let v = version_tag();
    render_piece("kinetic-prism", "ratchet-grid", &v, &build_piece3_kinetic_prism());
}

#[test]
#[ignore = "renders piece 4 (oxide-ballroom)"]
fn vj_render_p4() {
    let v = version_tag();
    render_piece("oxide-ballroom", "disintegration-loops", &v, &build_piece4_oxide_ballroom());
}

#[test]
#[ignore = "renders piece 5 (modell5-pulse)"]
fn vj_render_p5() {
    let v = version_tag();
    render_piece("modell5-pulse", "portrait-bursts", &v, &build_piece5_modell5_pulse());
}

#[test]
#[ignore = "renders piece 6 (isol8-studio vj showcase)"]
fn vj_render_p6() {
    let v = version_tag();
    render_piece6_showcase(&v);
}

#[test]
#[ignore = "renders all 6 Phase 3 VJ deliverables"]
fn vj_render_all() {
    let v = version_tag();
    render_piece("frontier-therapy", "dialogue-collage", &v, &build_piece1_frontier_therapy());
    render_piece("lemon-and-string", "books-collage", &v, &build_piece2_lemon_and_string());
    render_piece("kinetic-prism", "ratchet-grid", &v, &build_piece3_kinetic_prism());
    render_piece("oxide-ballroom", "disintegration-loops", &v, &build_piece4_oxide_ballroom());
    render_piece("modell5-pulse", "portrait-bursts", &v, &build_piece5_modell5_pulse());
    render_piece6_showcase(&v);
}
