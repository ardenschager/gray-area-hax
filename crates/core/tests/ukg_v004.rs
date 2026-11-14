//! Wave 2 Pure-YouTube UKG + Vocal-Percussion + Gridded/Segmented Collage Suite (`v004`):
//! - Piece 1 (`1080x1920 @ 60fps` `9:16`, `134 BPM`, `61%` Swing):
//!   `261114_videograin_flowbee-zeon_ukg-sticker-collage_v004.mp4`
//!   World A (`wA1..wA7`) — Skipping 2-step UKG pocket, `Stamp::VoxPerc` off-16th
//!   vocal-consonant & switch percussion with multi-cell ratchets, Omnichord `C2`
//!   sub-bass + `Min7` garage chord stabs, spotlighted Flowbee + Omnibot Zeon hooks,
//!   Apple Vision die-cut `#F2EDE0` cream-bordered stickers over hairline grid, and
//!   zero-blur `ISOL8` Sobel zine-ink / Bayer halftone break.
//! - Piece 2 (`1080x1080 @ 60fps` `1:1`, `136 BPM`, `62%` Swing):
//!   `261114_videograin_bid-stutter_vox-percussion-grid_v004.mp4`
//!   World B (`wB1..wB7`) — Todd-Edwards / Joy-Orbison / Burial-meets-The-Books UKG:
//!   100% human mouth-drum & tap-shoe percussion, auctioneer/beatbox `VoxPerc` lattice
//!   with `x2`/`x3`/`x4` `cell_salt` ratchets, fingerstyle electric sub-bass, pitched
//!   soulful `E4` vocal `Min7` chords + Rhodes stabs, and rapid-fire `Layout::Tile`
//!   (`1x1 <-> 2x1 <-> 2x2 <-> 3x2 <-> 3x3`) + `Layout::Collage` Vision cutouts.
//!
//! Strictly zero oscillators, zero external drum WAVs, zero AI arrangement,
//! and zero kaleidoscope.

use chromagrain_core::audio::{AudioClip, StereoBuffer};
use chromagrain_core::chop::{self, Caption, Slice, SliceRole};
use chromagrain_core::dsp::{BandIsolator, Biquad, EqBand, FilterKind};
use chromagrain_core::envelope::{Adsr, PictureCut};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::grain::{ChordVoicing, Stamp, VoiceSculpt};
use chromagrain_core::media;
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::render::{plan_project, render_project_to_video};
use chromagrain_core::seq::{SeqRow, Step, StepPattern};
use chromagrain_core::tile::{AspectRatio, Layout};
use chromagrain_core::timeline::{
    Clip, ClipKind, ClipTransform, DuckSpec, MasterPunch, Project, Provenance, PunchAction, Source,
};
use chromagrain_core::video::VideoClip;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PIECE1_BEATS: f64 = 44.0; // 11 bars @ 134 BPM = 19.701s
const PIECE2_BEATS: f64 = 44.0; // 11 bars @ 136 BPM = 19.412s

fn crate_root() -> PathBuf {
    if let Ok(p) = std::env::var("CHROMAGRAIN_UKG_ROOT") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let canonical =
        PathBuf::from("/Users/ardenschager/Documents/Projects/gray-area-hax/outputs/261114_videograin");
    if canonical.join("src").exists() {
        return canonical;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../outputs/261114_videograin")
}

fn out_dir() -> PathBuf {
    let tag = std::env::var("CHROMAGRAIN_UKG_VERSION").unwrap_or_else(|_| "v004".into());
    crate_root().join(tag).join("out")
}

struct WorldASources {
    w_a1_grid: Source,
    w_a1_cut: Source,
    w_a2_grid: Source,
    w_a2_cut: Source,
    w_a3_grid: Source,
    w_a3_cut: Source,
    w_a4_grid: Source,
    w_a5_grid: Source,
    w_a5_cut: Source,
    w_a6_grid: Source,
    w_a7_grid: Source,
}

struct WorldBSources {
    w_b1_grid: Source,
    w_b1_cut: Source,
    w_b2_grid: Source,
    w_b3_grid: Source,
    w_b3_cut: Source,
    w_b4_grid: Source,
    w_b5_grid: Source,
    w_b6_grid: Source,
    w_b6_cut: Source,
    w_b7_grid: Source,
}

fn load_yt(
    filename: &str,
    display_name: &str,
    uploader: &str,
    views: u64,
    url: &str,
    window: (f64, f64),
    base_hz: f32,
) -> Source {
    let p = crate_root().join("src").join(filename);
    let mut src = media::load_source(&p, 48_000)
        .unwrap_or_else(|e| panic!("failed to load YouTube clip {}: {e}", p.display()));
    src.name = display_name.into();
    src.path = Some(p);
    if base_hz > 0.0 {
        src.base_hz = base_hz;
    }
    src.provenance = Some(Provenance {
        title: display_name.into(),
        uploader: uploader.into(),
        upload_date: String::new(),
        views: Some(views),
        url: url.into(),
        license: String::new(),
        query: String::new(),
        window,
    });
    src
}

fn stub_source(name: &str, dur_secs: f64, base_hz: f32, with_alpha: bool) -> Source {
    let sr = 48_000;
    let n = (dur_secs * sr as f64) as usize;
    let samples = vec![0.05f32; n.max(1024)];
    let mut vid = VideoClip::test_pattern(32, 24, 12.0, 1.0);
    if with_alpha {
        for f in &mut vid.frames {
            for y in 0..f.height {
                for x in 0..f.width {
                    let inside = x >= 8 && x < 24 && y >= 4 && y < 20;
                    f.data[((y * f.width + x) * 4 + 3) as usize] = if inside { 255 } else { 0 };
                }
            }
        }
    }
    Source {
        name: name.into(),
        audio: Some(Arc::new(AudioClip::new(samples, sr))),
        video: Some(Arc::new(vid)),
        base_hz,
        ..Default::default()
    }
}

fn load_world_a_real() -> WorldASources {
    let w_a1_grid = load_yt(
        "wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4",
        "Turbo Encabulator",
        "Dave Rondot",
        3_474_860,
        "https://www.youtube.com/watch?v=Ac7G7xOG2Ag",
        (74.0, 29.0),
        83.5,
    );
    let w_a1_cut = media::cutout_source(&w_a1_grid, "person");

    let w_a2_grid = load_yt(
        "wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4",
        "Pet Flowbee Groomer",
        "jlryancom",
        31_214,
        "https://www.youtube.com/watch?v=K-iaANk41Oc",
        (178.0, 35.0),
        130.8,
    );
    let w_a2_cut = media::cutout_source(&w_a2_grid, "person");

    let w_a3_grid = load_yt(
        "wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4",
        "Tomy Omnibot Zeon",
        "Rare Coins & Collectibles",
        558,
        "https://www.youtube.com/watch?v=E8d7kbbrvQU",
        (0.5, 35.0),
        191.2,
    );
    let w_a3_cut = media::cutout_source(&w_a3_grid, "subject");

    let w_a4_grid = load_yt(
        "wA4_useless_switch_box_OtgzPZQt4q4.mp4",
        "Useless Switch Box",
        "Danjel Nyberg",
        2_679,
        "https://www.youtube.com/watch?v=OtgzPZQt4q4",
        (4.0, 34.0),
        220.0,
    );

    let w_a5_grid = load_yt(
        "wA5_circuit_speak_math_N8-QAaJpc0Y.mp4",
        "Circuit Speak & Math",
        "Mike Weakley",
        936,
        "https://www.youtube.com/watch?v=N8-QAaJpc0Y",
        (2.0, 34.0),
        135.2,
    );
    let w_a5_cut = media::cutout_source(&w_a5_grid, "subject");

    let w_a6_grid = load_yt(
        "wA6_att_answering_machine_5O-f-sWv5Pk.mp4",
        "AT&T Answering Machine",
        "VACUUM TONES",
        793,
        "https://www.youtube.com/watch?v=5O-f-sWv5Pk",
        (5.0, 36.5),
        220.0,
    );

    let w_a7_grid = load_yt(
        "wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4",
        "Suzuki Omnichord OM-36",
        "Michael Dicken",
        712,
        "https://www.youtube.com/watch?v=j-z1lO9v24M",
        (18.0, 34.0),
        66.8,
    );

    WorldASources {
        w_a1_grid,
        w_a1_cut,
        w_a2_grid,
        w_a2_cut,
        w_a3_grid,
        w_a3_cut,
        w_a4_grid,
        w_a5_grid,
        w_a5_cut,
        w_a6_grid,
        w_a7_grid,
    }
}

fn load_world_a_stub() -> WorldASources {
    WorldASources {
        w_a1_grid: stub_source("wA1_grid", 29.0, 83.5, false),
        w_a1_cut: stub_source("wA1_cut", 29.0, 83.5, true),
        w_a2_grid: stub_source("wA2_grid", 35.0, 130.8, false),
        w_a2_cut: stub_source("wA2_cut", 35.0, 130.8, true),
        w_a3_grid: stub_source("wA3_grid", 35.0, 191.2, false),
        w_a3_cut: stub_source("wA3_cut", 35.0, 191.2, true),
        w_a4_grid: stub_source("wA4_grid", 34.0, 220.0, false),
        w_a5_grid: stub_source("wA5_grid", 34.0, 135.2, false),
        w_a5_cut: stub_source("wA5_cut", 34.0, 135.2, true),
        w_a6_grid: stub_source("wA6_grid", 36.5, 220.0, false),
        w_a7_grid: stub_source("wA7_grid", 34.0, 66.8, false),
    }
}

fn load_world_b_real() -> WorldBSources {
    let w_b1_grid = load_yt(
        "wB1_soul_stutter_acapella_XUaVI90fs5w.mp4",
        "Soul Stutter Acapella",
        "MissChyna520",
        125,
        "https://www.youtube.com/watch?v=XUaVI90fs5w",
        (9.0, 36.0),
        326.5,
    );
    let w_b1_cut = media::cutout_source(&w_b1_grid, "person");

    let w_b2_grid = load_yt(
        "wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4",
        "Quarantine Beatbox",
        "JAM ONE",
        397,
        "https://www.youtube.com/watch?v=HL5rhH4m6Vc",
        (5.5, 33.5),
        138.6,
    );

    let w_b3_grid = load_yt(
        "wB3_auctioneer_chant_0sRO8LVil8w.mp4",
        "Auctioneer Chant",
        "Schur Success Group",
        3_563,
        "https://www.youtube.com/watch?v=0sRO8LVil8w",
        (16.0, 34.0),
        138.6,
    );
    let w_b3_cut = media::cutout_source(&w_b3_grid, "person");

    let w_b4_grid = load_yt(
        "wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4",
        "Solo Rhodes Improv",
        "j_dublay",
        944,
        "https://www.youtube.com/watch?v=MxOzUVFZwCs",
        (1.0, 26.5),
        139.1,
    );

    let w_b5_grid = load_yt(
        "wB5_fingerstyle_bass_groove_KnvZ81XivtE.mp4",
        "Fingerstyle Bass Groove",
        "Dolman Bass",
        192,
        "https://www.youtube.com/watch?v=KnvZ81XivtE",
        (9.0, 30.0),
        104.1,
    );

    let w_b6_grid = load_yt(
        "wB6_tap_shuffle_heel_6uD9rTyeurk.mp4",
        "Tap Dance Shuffle Step Heel",
        "Gregg Geoffroy",
        74,
        "https://www.youtube.com/watch?v=6uD9rTyeurk",
        (0.2, 34.0),
        138.6,
    );
    let w_b6_cut = media::cutout_source(&w_b6_grid, "subject");

    let w_b7_grid = load_yt(
        "wB7_pinball_solenoids_5zJ7D1DNPFY.mp4",
        "Stern STARS Pinball Solenoids",
        "Pinballmania LLC",
        129,
        "https://www.youtube.com/watch?v=5zJ7D1DNPFY",
        (2.0, 22.5),
        640.0,
    );

    WorldBSources {
        w_b1_grid,
        w_b1_cut,
        w_b2_grid,
        w_b3_grid,
        w_b3_cut,
        w_b4_grid,
        w_b5_grid,
        w_b6_grid,
        w_b6_cut,
        w_b7_grid,
    }
}

fn load_world_b_stub() -> WorldBSources {
    WorldBSources {
        w_b1_grid: stub_source("wB1_grid", 36.0, 326.5, false),
        w_b1_cut: stub_source("wB1_cut", 36.0, 326.5, true),
        w_b2_grid: stub_source("wB2_grid", 33.5, 138.6, false),
        w_b3_grid: stub_source("wB3_grid", 34.0, 138.6, false),
        w_b3_cut: stub_source("wB3_cut", 34.0, 138.6, true),
        w_b4_grid: stub_source("wB4_grid", 26.5, 139.1, false),
        w_b5_grid: stub_source("wB5_grid", 30.0, 104.1, false),
        w_b6_grid: stub_source("wB6_grid", 34.0, 138.6, false),
        w_b6_cut: stub_source("wB6_cut", 34.0, 138.6, true),
        w_b7_grid: stub_source("wB7_grid", 22.5, 640.0, false),
    }
}

fn push_slice(src: &mut Source, start: f64, end: f64, role: SliceRole, focus: (f32, f32)) -> u16 {
    let dur = src.duration().max(0.05);
    let s0 = start.clamp(0.0, (dur - 0.02).max(0.0));
    let s1 = end.clamp(s0 + 0.015, dur);
    let mut single = vec![Slice::new(s0, s1)];
    chop::enrich_slices(&mut single, src.audio.as_deref(), src.video.as_deref());
    let mut sl = single.remove(0);
    if role != SliceRole::Raw {
        sl.role_hint = role;
    }
    sl.caption = Caption::default();
    sl.focus = focus;
    src.slices.push(sl);
    (src.slices.len() - 1) as u16
}

fn crisp_adsr(release: f32) -> Adsr {
    Adsr {
        attack: 0.002,
        decay: 0.04,
        sustain: 1.0,
        release,
        picture: PictureCut::Cut,
        cut_threshold: 0.05,
    }
}

fn configure_crisp_track(p: &mut Project, tid: usize, level: f32) {
    let tr = &mut p.tracks[tid];
    tr.level = level;
    tr.level_to_opacity = 0.0;
    tr.duck_to_opacity = 0.0;
    tr.rms_to_opacity = 0.0;
    tr.additive = Some(0.0);
}

#[allow(clippy::too_many_arguments)]
fn push_phrase_clip(
    p: &mut Project,
    track_id: usize,
    source_id: usize,
    slice_start: f64,
    slice_end: f64,
    start_beat: f64,
    max_beats: Option<f64>,
    gain: f32,
    drive_db: f32,
    pan: f32,
    xform: ClipTransform,
) {
    let src_dur = p.sources[source_id].duration().max(0.05);
    let raw_secs = (slice_end - slice_start).max(0.05);
    let nat_beats = p.secs_to_beats(raw_secs);
    let len_beats = max_beats.map_or(nat_beats, |mb| nat_beats.min(mb)).max(0.08);
    let mut c = Clip::new_snippet(source_id, start_beat, len_beats);
    c.grains.position = (slice_start / src_dur).clamp(0.0, 0.999) as f32;
    c.grains.gain = gain;
    c.grains.pan = pan;
    c.grains.envelope = 0.02;
    c.grains.adsr = Some(crisp_adsr(0.014));
    c.grains.sculpt = VoiceSculpt {
        stamp: Stamp::Phrase,
        drive_db,
        hpf_hz: 175.0,
        lpf_hz: 14_800.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    c.grains.choke_group = 1;
    c.collage_cutout = true;
    c.visual.additive = 0.0;
    c.visual.feather = 0.0;
    c.link.gain_to_opacity = 0.0;
    c.link.envelope_to_opacity = 0.0;
    c.link.pitch_to_hue = 0.0;
    c.transform = xform;
    p.tracks[track_id].clips.push(c);
}

/// Percussion / `VoxPerc` row (`adsr = None` so `VoiceTag::visual_hold` holds a crisp
/// 100%-opacity freeze-frame across the 16th step / multi-cell ratchet staircase with zero flash gaps!).
fn new_perc_row(source: usize, n_steps: usize, gain: f32, duration: f32, sculpt: VoiceSculpt) -> SeqRow {
    let mut r = SeqRow::new(source, n_steps);
    r.grains.gain = gain;
    r.grains.duration = duration;
    r.grains.spray = 0.0;
    r.grains.pan_spread = 0.10;
    r.grains.envelope = 0.03;
    r.grains.adsr = None;
    r.grains.sculpt = sculpt;
    r.sculpt = sculpt;
    r
}

/// Sustained harmonic / sub-bass row (`PictureCut::Cut` ADSR so sound & picture hard-cut together).
fn new_sustained_row(
    source: usize,
    n_steps: usize,
    gain: f32,
    duration: f32,
    sculpt: VoiceSculpt,
) -> SeqRow {
    let mut r = SeqRow::new(source, n_steps);
    r.grains.gain = gain;
    r.grains.duration = duration;
    r.grains.spray = 0.0;
    r.grains.pan_spread = 0.08;
    r.grains.envelope = 0.04;
    r.grains.adsr = Some(crisp_adsr(0.012));
    r.grains.sculpt = sculpt;
    r.sculpt = sculpt;
    r
}

// ============================================================================
// PIECE 1 — `261114_videograin_flowbee-zeon_ukg-sticker-collage_v004.mp4`
// (`1080x1920 @ 60fps` `9:16`, `134.0 BPM`, `61%` Swing, `44 beats` = `19.701s`)
// ============================================================================

fn build_piece1_flowbee_zeon(mut s: WorldASources) -> Project {
    let mut p = Project {
        bpm: 134.0,
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Collage,
        layout_pan: true,
        studio_bus: true,
        captions_on: false,
        master_gain_db: 2.5,
        master_bloom: 0.0,
        ..Project::new()
    };
    p.set_aspect_hd(AspectRatio::Vertical9x16);

    // Precision `0.01s` verified slices across all 7 World A YouTube clips:
    let sl_a1_kick = push_slice(&mut s.w_a1_grid, 8.12, 8.23, SliceRole::Kick, (0.52, 0.36));
    let sl_a1_tch = push_slice(&mut s.w_a1_grid, 23.62, 23.72, SliceRole::Snare, (0.52, 0.36));
    let sl_a1_vox_d = push_slice(&mut s.w_a1_grid, 18.92, 19.00, SliceRole::Hat, (0.52, 0.36));

    let sl_a2_pd = push_slice(&mut s.w_a2_grid, 18.11, 18.19, SliceRole::Hat, (0.52, 0.42));
    let sl_a2_b = push_slice(&mut s.w_a2_grid, 18.48, 18.56, SliceRole::Hat, (0.48, 0.46));

    let sl_a3_clack = push_slice(&mut s.w_a3_grid, 4.98, 5.08, SliceRole::Snare, (0.50, 0.48));
    let sl_a3_tr = push_slice(&mut s.w_a3_grid, 13.60, 13.69, SliceRole::Hat, (0.50, 0.42));
    let sl_a3_vowel = push_slice(&mut s.w_a3_grid, 26.05, 26.50, SliceRole::Tonal, (0.50, 0.44));

    let sl_a4_tog = push_slice(&mut s.w_a4_grid, 13.88, 13.94, SliceRole::Hat, (0.48, 0.48));
    let sl_a4_lid = push_slice(&mut s.w_a4_grid, 18.91, 18.99, SliceRole::Snare, (0.48, 0.48));
    let sl_a4_s = push_slice(&mut s.w_a4_grid, 31.92, 31.98, SliceRole::Hat, (0.48, 0.48));

    let sl_a5_zap = push_slice(&mut s.w_a5_grid, 4.50, 4.59, SliceRole::Snare, (0.46, 0.48));
    let sl_a5_tick = push_slice(&mut s.w_a5_grid, 20.30, 20.36, SliceRole::Hat, (0.46, 0.48));

    let sl_a6_btn = push_slice(&mut s.w_a6_grid, 32.89, 32.96, SliceRole::Hat, (0.46, 0.54));

    let mut s_a7_chord_src = s.w_a7_grid.clone();
    s_a7_chord_src.base_hz = 99.4; // G2 sustained Omnichord triad
    let sl_a7_sub = push_slice(&mut s.w_a7_grid, 18.39, 18.85, SliceRole::Tonal, (0.52, 0.54));
    let sl_a7_chd_g = push_slice(&mut s_a7_chord_src, 22.75, 23.35, SliceRole::Tonal, (0.54, 0.52));
    let sl_a7_chd_gs = push_slice(&mut s_a7_chord_src, 11.93, 12.55, SliceRole::Tonal, (0.48, 0.50));

    let _ = push_slice(&mut s.w_a1_cut, 18.88, 19.70, SliceRole::Phrase, (0.52, 0.38));
    let _ = push_slice(&mut s.w_a1_cut, 23.45, 24.36, SliceRole::Phrase, (0.52, 0.38));
    let _ = push_slice(&mut s.w_a2_cut, 12.88, 14.54, SliceRole::Phrase, (0.52, 0.42));
    let _ = push_slice(&mut s.w_a2_cut, 18.42, 20.08, SliceRole::Phrase, (0.52, 0.44));
    let _ = push_slice(&mut s.w_a3_cut, 15.54, 16.92, SliceRole::Phrase, (0.50, 0.46));
    let _ = push_slice(&mut s.w_a5_cut, 22.05, 22.65, SliceRole::Phrase, (0.48, 0.48));

    let id_a1_grid = p.add_source(s.w_a1_grid);
    let id_a1_cut = p.add_source(s.w_a1_cut);
    let id_a2_grid = p.add_source(s.w_a2_grid);
    let id_a2_cut = p.add_source(s.w_a2_cut);
    let id_a3_grid = p.add_source(s.w_a3_grid);
    let id_a3_cut = p.add_source(s.w_a3_cut);
    let id_a4_grid = p.add_source(s.w_a4_grid);
    let id_a5_grid = p.add_source(s.w_a5_grid);
    let id_a5_cut = p.add_source(s.w_a5_cut);
    let id_a6_grid = p.add_source(s.w_a6_grid);
    let id_a7_sub = p.add_source(s.w_a7_grid);
    let id_a7_chd = p.add_source(s_a7_chord_src);

    let key_cm = Key::new(0, ScaleKind::Minor);

    let t_kick = p.add_track("ukg_kick");
    let t_sub = p.add_track("omnichord_sub");
    let t_chords = p.add_track("garage_chords");
    let t_perc = p.add_track("vox_perc_grid");
    let t_hooks = p.add_track("absurd_sticker_hooks");

    configure_crisp_track(&mut p, t_kick, 1.06);
    configure_crisp_track(&mut p, t_sub, 0.92);
    configure_crisp_track(&mut p, t_chords, 0.92);
    configure_crisp_track(&mut p, t_perc, 1.12);
    configure_crisp_track(&mut p, t_hooks, 1.18);

    p.tracks[t_kick].eq.push(EqBand { freq: 255.0, gain_db: -4.5, q: 1.4 });
    p.tracks[t_sub].eq.push(EqBand { freq: 245.0, gain_db: -6.0, q: 1.2 });
    p.tracks[t_sub].duck = Some(DuckSpec {
        source_track: t_kick,
        amount: 0.62,
        attack: 0.004,
        release: 0.11,
        key_hz: 150.0,
    });
    p.tracks[t_chords].eq.push(EqBand { freq: 295.0, gain_db: -4.5, q: 1.3 });
    // Audio-only (`video = 0.0`) London 2-step dub chord echo — lush musical glue with zero visual blur!
    p.tracks[t_chords].effects.push(AvEffect {
        kind: EffectKind::Delay {
            time: 0.336, // dotted-8th @ 134 BPM
            feedback: 0.26,
            mix: 0.20,
            shift_x: 0.0,
            shift_y: 0.0,
        },
        audio: 1.0,
        video: 0.0,
    });
    p.tracks[t_chords].duck = Some(DuckSpec {
        source_track: t_hooks,
        amount: 0.54,
        attack: 0.008,
        release: 0.14,
        key_hz: 3800.0,
    });
    p.tracks[t_perc].eq.push(EqBand { freq: 300.0, gain_db: -3.5, q: 1.3 });
    p.tracks[t_perc].eq.push(EqBand { freq: 4200.0, gain_db: 2.5, q: 0.9 });
    p.tracks[t_perc].duck = Some(DuckSpec {
        source_track: t_hooks,
        amount: 0.32,
        attack: 0.006,
        release: 0.10,
        key_hz: 3800.0,
    });
    p.tracks[t_hooks].eq.push(EqBand { freq: 265.0, gain_db: -3.5, q: 1.3 });
    p.tracks[t_hooks].eq.push(EqBand { freq: 3100.0, gain_db: 2.5, q: 0.9 });
    p.tracks[t_hooks].choke_group = 1;

    // ---- 1. Skipping 2-Step UKG Kick Pattern (`61%` Swing, 2-bar = 32 steps, `loop_beats = None`!) ----
    let mut pat_kick = StepPattern::new("ukg_kick_2step");
    pat_kick.set_grid(8.0, 4);
    pat_kick.swing = 0.61;
    let kick_sculpt = VoiceSculpt {
        stamp: Stamp::Kick,
        pitch_semitones: -14.0,
        pitch_env_semitones: 17.0,
        pitch_env_ms: 40.0,
        sub_exciter: 0.82,
        drive_db: 7.8,
        hpf_hz: 30.0,
        lpf_hz: 150.0,
        resonance_q: 0.88,
        ..VoiceSculpt::default()
    };
    let mut r_kick = new_perc_row(id_a1_grid, 32, 1.02, 0.14, kick_sculpt);
    r_kick.choke_group = 2;
    for &(st, gn) in &[
        (0usize, 1.00f32),
        (10, 0.96),
        (16, 1.00),
        (23, 0.74),
        (26, 0.96),
        (30, 0.70),
    ] {
        r_kick.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl_a1_kick),
            length: 1.25,
            ..Default::default()
        };
    }
    pat_kick.rows.push(r_kick);
    let pid_kick = p.add_pattern(pat_kick);
    // Bar 1 (`0..4`) builds with kick on 1, Bar 2 (`4..8`) pauses low-end for Flowbee motor cue,
    // Bars 3–6 (`8..24`) full 2-step drop, Bar 7 (`24..28`) LF-cut radio break, Bar 8 (`28..30.65`) Bayer sub dub,
    // Bars 9–11 (`32..44`) full UKG re-drop!
    for &(b0, b1) in &[(0.0, 4.0), (8.0, 24.0), (28.0, 30.65), (32.0, 44.0)] {
        p.tracks[t_kick].clips.push(Clip::new_pattern(pid_kick, b0, b1 - b0));
    }

    // ---- 2. Omnichord `C2` (`66.8 Hz`) Syncopated UKG Sub-Bass (`61%` Swing) ----
    let mut pat_sub = StepPattern::new("omnichord_c2_sub");
    pat_sub.set_grid(8.0, 4);
    pat_sub.swing = 0.61;
    let sub_sculpt = VoiceSculpt {
        stamp: Stamp::Raw,
        sub_exciter: 0.34,
        drive_db: 4.5,
        hpf_hz: 34.0,
        lpf_hz: 175.0,
        resonance_q: 0.92,
        ..VoiceSculpt::default()
    };
    let mut r_sub = new_sustained_row(id_a7_sub, 32, 0.95, 0.26, sub_sculpt);
    r_sub.key = Some(key_cm);
    r_sub.choke_group = 4;
    let sub_notes: &[(usize, f32, f32, f32)] = &[
        (0, 0.0, 0.95, 0.68),   // C2
        (3, 3.0, 0.86, 0.50),   // Eb2 swung 16th
        (6, 0.0, 0.88, 0.56),   // C2 offbeat
        (10, -5.0, 0.94, 0.70), // G1 deep pocket
        (14, -2.0, 0.85, 0.48), // Bb1 pickup
        (16, 0.0, 0.95, 0.68),  // C2
        (19, 5.0, 0.88, 0.50),  // F2 garage bounce
        (22, 3.0, 0.86, 0.54),  // Eb2
        (26, -5.0, 0.94, 0.68), // G1
        (29, -2.0, 0.84, 0.44), // Bb1 swung 16th
        (31, 0.0, 0.80, 0.40),  // C2 turnaround
    ];
    for &(st, pt, gn, ln) in sub_notes {
        r_sub.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: ln,
            slice_index: Some(sl_a7_sub),
            ..Default::default()
        };
    }
    pat_sub.rows.push(r_sub);
    let pid_sub = p.add_pattern(pat_sub);
    for &(b0, b1) in &[(8.0, 24.0), (28.0, 30.65), (32.0, 44.0)] {
        p.tracks[t_sub].clips.push(Clip::new_pattern(pid_sub, b0, b1 - b0));
    }

    // ---- 3. Lush Syncopated `Min7` Omnichord + Zeon Robot Vowel Garage Stabs (`61%` Swing) ----
    let mut pat_chords = StepPattern::new("ukg_min7_stabs");
    pat_chords.set_grid(8.0, 4);
    pat_chords.swing = 0.61;
    let chord_sculpt = VoiceSculpt {
        stamp: Stamp::Chord,
        comb_mix: 0.26,
        comb_feedback: 0.88,
        chord: ChordVoicing::Min7,
        shimmer: 0.16,
        drive_db: 2.5,
        hpf_hz: 235.0,
        lpf_hz: 7600.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let mut r_omni_chd = new_sustained_row(id_a7_chd, 32, 0.86, 0.26, chord_sculpt);
    r_omni_chd.key = Some(key_cm);
    for &(st, pt, gn, ln, sl, vc) in &[
        (0usize, 5.0f32, 0.88f32, 0.72f32, sl_a7_chd_g, ChordVoicing::Min7),
        (6, 8.0, 0.84, 0.62, sl_a7_chd_gs, ChordVoicing::Min7),
        (10, 5.0, 0.78, 0.56, sl_a7_chd_g, ChordVoicing::Min7),
        (14, 3.0, 0.80, 0.56, sl_a7_chd_g, ChordVoicing::Sus4),
        (16, 5.0, 0.88, 0.72, sl_a7_chd_g, ChordVoicing::Min7),
        (22, 10.0, 0.82, 0.62, sl_a7_chd_gs, ChordVoicing::Min7),
        (27, 7.0, 0.78, 0.52, sl_a7_chd_g, ChordVoicing::Min7),
        (30, 3.0, 0.82, 0.56, sl_a7_chd_g, ChordVoicing::Sus4),
    ] {
        r_omni_chd.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: ln,
            slice_index: Some(sl),
            chord: vc,
            ..Default::default()
        };
    }
    let mut r_zeon_chd = new_sustained_row(id_a3_grid, 32, 0.72, 0.24, chord_sculpt);
    r_zeon_chd.key = Some(key_cm);
    for &(st, pt, gn) in &[
        (3usize, -7.0f32, 0.74f32),
        (11, -7.0, 0.78),
        (19, -4.0, 0.74),
        (25, -4.0, 0.76),
    ] {
        r_zeon_chd.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: 0.54,
            slice_index: Some(sl_a3_vowel),
            chord: ChordVoicing::Min7,
            ..Default::default()
        };
    }
    pat_chords.rows.push(r_omni_chd);
    pat_chords.rows.push(r_zeon_chd);
    let pid_chords = p.add_pattern(pat_chords);
    p.tracks[t_chords].clips.push(Clip::new_pattern(pid_chords, 0.0, PIECE1_BEATS));

    // ---- 4. Rim/Snare + Swung Off-16th `Stamp::VoxPerc` & Multi-Cell Gadget Grid (`61%` Swing) ----
    // Dense 16th interlocking across `wA1`, `wA2`, `wA3`, `wA4`, `wA5`, `wA6` (`adsr = None` -> `visual_hold = 0.14s`)
    // guarantees 100% continuous `2x1 / 2x2 / 2x3 / 3x3` hairline grid movement behind every sticker!
    let mut pat_perc = StepPattern::new("ukg_voxperc_gadget_grid");
    pat_perc.set_grid(8.0, 4);
    pat_perc.swing = 0.61;

    let snare_sculpt = VoiceSculpt {
        stamp: Stamp::Snare,
        snare_snap: 0.86,
        drive_db: 7.5,
        hpf_hz: 185.0,
        lpf_hz: 9600.0,
        resonance_q: 0.82,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let vox_sculpt = VoiceSculpt::preset(Stamp::VoxPerc);

    // Row A: Turbo Encabulator `"t-ch"` rim on `4, 12, 20, 28`
    let mut r_rim_a = new_perc_row(id_a1_grid, 32, 0.98, 0.11, snare_sculpt);
    for &st in &[4usize, 12, 20, 28] {
        r_rim_a.steps[st] = Step {
            on: true,
            gain: 0.98,
            slice_index: Some(sl_a1_tch),
            length: 1.2,
            ..Default::default()
        };
    }

    // Row B: Useless Switch Box lid snap + toggle click + `"s"` hat (`0, 2, 4, 6, 8, 10, 12, 14...`)
    let mut r_switch = new_perc_row(id_a4_grid, 32, 0.94, 0.05, vox_sculpt);
    for &(st, sl, gn, rt) in &[
        (0usize, sl_a4_tog, 0.72f32, 1u8),
        (2, sl_a4_s, 0.88, 1),
        (4, sl_a4_lid, 0.92, 1),
        (6, sl_a4_s, 0.90, 1),
        (8, sl_a4_tog, 0.74, 1),
        (10, sl_a4_s, 0.86, 1),
        (12, sl_a4_lid, 0.94, 1),
        (14, sl_a4_s, 0.90, 2), // x2 multi-cell ratchet hat!
        (16, sl_a4_tog, 0.72, 1),
        (18, sl_a4_s, 0.88, 1),
        (20, sl_a4_lid, 0.92, 1),
        (22, sl_a4_s, 0.90, 1),
        (24, sl_a4_tog, 0.74, 1),
        (26, sl_a4_s, 0.86, 1),
        (28, sl_a4_lid, 0.94, 1),
        (30, sl_a4_s, 0.90, 2),
    ] {
        r_switch.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 2.0 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row C: Swung Vocal-Snippet Percussion (`wA2` Flowbee `"p/d"` & `"b-"`)
    let mut r_vox_flow = new_perc_row(id_a2_grid, 32, 0.94, 0.048, vox_sculpt);
    r_vox_flow.choke_group = 3;
    for &(st, sl, gn, pt, rt) in &[
        (3usize, sl_a2_pd, 0.88f32, 1.5f32, 1u8),
        (7, sl_a2_b, 0.90, -1.0, 1),
        (9, sl_a2_pd, 0.84, 2.5, 1),
        (15, sl_a2_b, 0.86, 0.0, 2),
        (19, sl_a2_pd, 0.88, 2.0, 2), // x2 vocal syllable ratchet!
        (23, sl_a2_b, 0.86, -1.5, 1),
        (25, sl_a2_pd, 0.84, 1.0, 1),
        (31, sl_a2_pd, 0.92, 3.0, 3), // x3 turnaround multi-cell ratchet!
    ] {
        r_vox_flow.steps[st] = Step {
            on: true,
            gain: gn,
            pitch: pt,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 1.5 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row D: AT&T Red LED Answering Machine button clicks (`wA6`)
    let mut r_att = new_perc_row(id_a6_grid, 32, 0.90, 0.048, vox_sculpt);
    for &(st, gn, rt) in &[
        (1usize, 0.78f32, 1u8),
        (5, 0.76, 1),
        (11, 0.88, 1),
        (13, 0.76, 1),
        (17, 0.78, 1),
        (21, 0.76, 1),
        (27, 0.90, 2),
    ] {
        r_att.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl_a6_btn),
            ratchet: rt,
            ratchet_pitch: 2.0,
            ..Default::default()
        };
    }

    // Row E: Speak & Math circuit-bent keypad glitch ticks & zaps (`wA5`)
    let mut r_speak = new_perc_row(id_a5_grid, 32, 0.86, 0.048, vox_sculpt);
    for &(st, sl, gn, rt) in &[
        (2usize, sl_a5_tick, 0.78f32, 1u8),
        (7, sl_a5_tick, 0.80, 1),
        (11, sl_a5_tick, 0.86, 2),
        (14, sl_a5_zap, 0.78, 1),
        (18, sl_a5_tick, 0.78, 1),
        (24, sl_a5_tick, 0.80, 1),
        (29, sl_a5_tick, 0.88, 3), // x3 circuit-bent keypad roll!
    ] {
        r_speak.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 2.5 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row F: Omnibot `"tr"` & cassette clacks (`wA3`) + Encabulator `"d-"` (`wA1`)
    let mut r_bot_perc = new_perc_row(id_a3_grid, 32, 0.86, 0.05, vox_sculpt);
    for &(st, sl, gn) in &[
        (0usize, sl_a3_tr, 0.76f32),
        (4, sl_a3_clack, 0.78),
        (6, sl_a3_clack, 0.80),
        (9, sl_a3_tr, 0.82),
        (13, sl_a3_tr, 0.84),
        (16, sl_a3_tr, 0.76),
        (20, sl_a3_clack, 0.78),
        (22, sl_a3_clack, 0.80),
        (28, sl_a3_clack, 0.82),
    ] {
        r_bot_perc.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ..Default::default()
        };
    }

    let mut r_enc_d = new_perc_row(id_a1_grid, 32, 0.88, 0.048, vox_sculpt);
    for &(st, pt) in &[(7usize, 0.0f32), (15, 2.0), (23, -1.0), (31, 1.5)] {
        r_enc_d.steps[st] = Step {
            on: true,
            gain: 0.84,
            pitch: pt,
            slice_index: Some(sl_a1_vox_d),
            ..Default::default()
        };
    }

    pat_perc.rows.push(r_rim_a);
    pat_perc.rows.push(r_switch);
    pat_perc.rows.push(r_vox_flow);
    pat_perc.rows.push(r_att);
    pat_perc.rows.push(r_speak);
    pat_perc.rows.push(r_bot_perc);
    pat_perc.rows.push(r_enc_d);
    let pid_perc = p.add_pattern(pat_perc);
    p.tracks[t_perc].clips.push(Clip::new_pattern(pid_perc, 0.0, PIECE1_BEATS));

    // ---- 5. Spotlighted Absurd Deadpan Vocal Hooks (`choke_group = 1`, Vision Die-Cut Stickers!) ----
    let hero_center = ClipTransform { x: 0.0, y: 0.04, scale: 1.02, rotation: 0.0 };
    let hero_left = ClipTransform { x: -0.06, y: 0.02, scale: 1.00, rotation: 0.0 };
    let hero_right = ClipTransform { x: 0.06, y: 0.04, scale: 1.04, rotation: 0.0 };

    // Hook 1 (Bar 2 pre-drop `4.15..7.86`): "Now turn on the Flowbee motor" -> slams into Bar 3 UKG drop!
    push_phrase_clip(
        &mut p, t_hooks, id_a2_cut, 12.88, 14.54, 4.15, None, 1.28, 2.5, -0.05, hero_left,
    );

    // Hook 2 (Bar 4 `12.0..15.7`): "bouncing motion works best"
    push_phrase_clip(
        &mut p, t_hooks, id_a2_cut, 18.42, 20.08, 12.0, None, 1.36, 2.5, 0.04, hero_center,
    );

    // Hook 3 (Bar 6 `20.5..23.6` after Bar 5 rapid `Tile` grid break): Omnibot Zeon "My name is Zeon"
    push_phrase_clip(
        &mut p, t_hooks, id_a3_cut, 15.54, 16.92, 20.5, None, 1.20, 2.2, 0.0, hero_right,
    );

    // Hook 4 (Bar 7 `ISOL8` Sobel zine-ink entry `24.0..25.85`): Turbo Encabulator "dingle arm"
    push_phrase_clip(
        &mut p, t_hooks, id_a1_cut, 18.88, 19.70, 24.0, None, 1.22, 2.2, -0.06, hero_left,
    );

    // Hook 5 (Pre-redrop cue `30.65..31.99`): Speak & Math "Solve it"
    push_phrase_clip(
        &mut p, t_hooks, id_a5_cut, 22.05, 22.65, 30.65, None, 1.18, 2.2, 0.06, hero_right,
    );

    // Hook 6 (Bar 9 UKG Re-Drop `32.0 -> 32.5 -> 33.0..36.7`): Choked 2-step vocal stutter into "bouncing motion works best"!
    push_phrase_clip(
        &mut p, t_hooks, id_a2_cut, 18.42, 18.80, 32.0, Some(0.48), 1.32, 2.5, -0.10, hero_left,
    );
    push_phrase_clip(
        &mut p, t_hooks, id_a2_cut, 18.42, 18.80, 32.5, Some(0.48), 1.34, 2.5, 0.10, hero_right,
    );
    push_phrase_clip(
        &mut p, t_hooks, id_a2_cut, 18.42, 20.08, 33.0, None, 1.38, 2.5, 0.0, hero_center,
    );

    // Hook 7 (Bar 11 button tag `40.5..42.55`): Turbo Encabulator "It's not cheap"
    push_phrase_clip(
        &mut p, t_hooks, id_a1_cut, 23.45, 24.36, 40.5, None, 1.20, 2.2, 0.0, hero_center,
    );

    // ---- 6. Master Stage Choreography (`Collage <-> Tile` + 2-Bar Zero-Blur `ISOL8` Break) ----
    for &(b0, b1) in &[(0.0, 4.0), (16.0, 20.0), (37.0, 40.0)] {
        p.master_punches.push(MasterPunch {
            start_beat: b0,
            end_beat: b1,
            action: PunchAction::Layout(Layout::Tile),
        });
    }
    p.master_punches.push(MasterPunch {
        start_beat: 24.0,
        end_beat: 28.0,
        action: PunchAction::Isolator(BandIsolator {
            mute: [true, false, false, false, true],
            output_db: 2.2,
            ..BandIsolator::default()
        }),
    });
    p.master_punches.push(MasterPunch {
        start_beat: 28.0,
        end_beat: 30.65,
        action: PunchAction::Isolator(BandIsolator {
            mute: [false, false, true, true, true],
            output_db: 2.4,
            ..BandIsolator::default()
        }),
    });

    p
}

// ============================================================================
// PIECE 2 — `261114_videograin_bid-stutter_vox-percussion-grid_v004.mp4`
// (`1080x1080 @ 60fps` `1:1`, `136.0 BPM`, `62%` Swing, `44 beats` = `19.412s`)
// ============================================================================

fn build_piece2_bid_stutter(mut s: WorldBSources) -> Project {
    let mut p = Project {
        bpm: 136.0,
        sample_rate: 48_000,
        fps: 60.0,
        layout: Layout::Tile,
        layout_pan: true,
        studio_bus: true,
        captions_on: false,
        master_gain_db: 0.4,
        master_bloom: 0.0,
        ..Project::new()
    };
    p.set_aspect_hd(AspectRatio::Square1x1);

    let sl_b1_hey = push_slice(&mut s.w_b1_grid, 29.50, 30.15, SliceRole::Tonal, (0.50, 0.28));
    let sl_b1_hel = push_slice(&mut s.w_b1_grid, 32.60, 33.35, SliceRole::Tonal, (0.50, 0.28));
    let sl_b1_f4 = push_slice(&mut s.w_b1_grid, 11.35, 11.85, SliceRole::Tonal, (0.50, 0.28));

    let sl_b2_kick = push_slice(&mut s.w_b2_grid, 27.36, 27.44, SliceRole::Kick, (0.50, 0.42));
    let sl_b2_snare = push_slice(&mut s.w_b2_grid, 23.65, 23.74, SliceRole::Snare, (0.50, 0.42));
    let sl_b2_ts = push_slice(&mut s.w_b2_grid, 3.22, 3.29, SliceRole::Hat, (0.50, 0.42));
    let sl_b2_pf = push_slice(&mut s.w_b2_grid, 15.46, 15.54, SliceRole::Hat, (0.50, 0.42));

    let sl_b3_kt = push_slice(&mut s.w_b3_grid, 0.14, 0.23, SliceRole::Hat, (0.62, 0.42));
    let sl_b3_tw = push_slice(&mut s.w_b3_grid, 21.14, 21.22, SliceRole::Hat, (0.62, 0.42));

    let sl_b4_cs3 = push_slice(&mut s.w_b4_grid, 16.32, 16.92, SliceRole::Tonal, (0.38, 0.56));
    let sl_b4_a3 = push_slice(&mut s.w_b4_grid, 21.46, 22.06, SliceRole::Tonal, (0.38, 0.56));

    let sl_b5_gs2 = push_slice(&mut s.w_b5_grid, 3.46, 3.96, SliceRole::Tonal, (0.44, 0.54));
    let sl_b5_g2 = push_slice(&mut s.w_b5_grid, 4.31, 4.85, SliceRole::Tonal, (0.44, 0.54));

    let sl_b6_brush = push_slice(&mut s.w_b6_grid, 8.16, 8.22, SliceRole::Hat, (0.50, 0.62));
    let sl_b6_heel = push_slice(&mut s.w_b6_grid, 18.50, 18.59, SliceRole::Kick, (0.50, 0.62));
    let sl_b6_toe = push_slice(&mut s.w_b6_grid, 19.88, 19.96, SliceRole::Snare, (0.50, 0.62));

    let sl_b7_snap = push_slice(&mut s.w_b7_grid, 4.94, 5.00, SliceRole::Hat, (0.52, 0.48));
    let sl_b7_chime = push_slice(&mut s.w_b7_grid, 5.20, 5.70, SliceRole::Tonal, (0.52, 0.48));
    let sl_b7_knock = push_slice(&mut s.w_b7_grid, 8.46, 8.53, SliceRole::Snare, (0.52, 0.48));

    let _ = push_slice(&mut s.w_b1_cut, 25.90, 27.54, SliceRole::Phrase, (0.50, 0.36));
    let _ = push_slice(&mut s.w_b3_cut, 16.30, 17.26, SliceRole::Phrase, (0.62, 0.42));
    let _ = push_slice(&mut s.w_b3_cut, 17.56, 18.68, SliceRole::Phrase, (0.62, 0.42));
    let _ = push_slice(&mut s.w_b6_cut, 13.88, 14.92, SliceRole::Phrase, (0.50, 0.54));

    let id_b1_grid = p.add_source(s.w_b1_grid);
    let id_b1_cut = p.add_source(s.w_b1_cut);
    let id_b2_grid = p.add_source(s.w_b2_grid);
    let id_b3_grid = p.add_source(s.w_b3_grid);
    let id_b3_cut = p.add_source(s.w_b3_cut);
    let id_b4_grid = p.add_source(s.w_b4_grid);
    let id_b5_grid = p.add_source(s.w_b5_grid);
    let id_b6_grid = p.add_source(s.w_b6_grid);
    let id_b6_cut = p.add_source(s.w_b6_cut);
    let id_b7_grid = p.add_source(s.w_b7_grid);

    let key_csm = Key::new(1, ScaleKind::Minor);

    let t_kick = p.add_track("mouth_heel_kick");
    let t_bass = p.add_track("fingerstyle_ukg_sub");
    let t_chords = p.add_track("todd_vox_rhodes_chords");
    let t_perc = p.add_track("vox_tap_percussion_lattice");
    let t_hooks = p.add_track("collage_cutout_hooks");

    configure_crisp_track(&mut p, t_kick, 1.04);
    configure_crisp_track(&mut p, t_bass, 0.80);
    configure_crisp_track(&mut p, t_chords, 0.96);
    configure_crisp_track(&mut p, t_perc, 1.14);
    configure_crisp_track(&mut p, t_hooks, 1.16);

    p.tracks[t_kick].eq.push(EqBand { freq: 255.0, gain_db: -4.5, q: 1.4 });
    p.tracks[t_bass].eq.push(EqBand { freq: 240.0, gain_db: -6.5, q: 1.2 });
    p.tracks[t_bass].duck = Some(DuckSpec {
        source_track: t_kick,
        amount: 0.62,
        attack: 0.004,
        release: 0.11,
        key_hz: 155.0,
    });
    p.tracks[t_chords].eq.push(EqBand { freq: 290.0, gain_db: -5.0, q: 1.3 });
    p.tracks[t_chords].effects.push(AvEffect {
        kind: EffectKind::Delay {
            time: 0.331, // dotted-8th @ 136 BPM
            feedback: 0.24,
            mix: 0.18,
            shift_x: 0.0,
            shift_y: 0.0,
        },
        audio: 1.0,
        video: 0.0,
    });
    p.tracks[t_chords].duck = Some(DuckSpec {
        source_track: t_hooks,
        amount: 0.52,
        attack: 0.008,
        release: 0.14,
        key_hz: 3800.0,
    });
    p.tracks[t_perc].eq.push(EqBand { freq: 295.0, gain_db: -3.8, q: 1.3 });
    p.tracks[t_perc].eq.push(EqBand { freq: 4500.0, gain_db: 2.5, q: 0.9 });
    p.tracks[t_perc].duck = Some(DuckSpec {
        source_track: t_hooks,
        amount: 0.32,
        attack: 0.006,
        release: 0.10,
        key_hz: 3800.0,
    });
    p.tracks[t_hooks].eq.push(EqBand { freq: 255.0, gain_db: -3.5, q: 1.3 });
    p.tracks[t_hooks].eq.push(EqBand { freq: 3200.0, gain_db: 2.5, q: 0.9 });
    p.tracks[t_hooks].choke_group = 1;

    // ---- 1. 100% Human Mouth-Kick (`wB2`) + Hardwood Heel Thump (`wB6`) (`62%` Swing, `loop_beats = None`!) ----
    let mut pat_kick = StepPattern::new("beatbox_heel_2step_kick");
    pat_kick.set_grid(8.0, 4);
    pat_kick.swing = 0.62;
    let b_kick_sculpt = VoiceSculpt {
        stamp: Stamp::Kick,
        pitch_semitones: -10.0,
        pitch_env_semitones: 15.0,
        pitch_env_ms: 38.0,
        sub_exciter: 0.78,
        drive_db: 7.2,
        hpf_hz: 30.0,
        lpf_hz: 155.0,
        resonance_q: 0.88,
        ..VoiceSculpt::default()
    };
    let mut r_lip_kick = new_perc_row(id_b2_grid, 32, 1.00, 0.14, b_kick_sculpt);
    r_lip_kick.choke_group = 2;
    for &(st, gn) in &[
        (0usize, 1.00f32),
        (10, 0.96),
        (16, 1.00),
        (23, 0.74),
        (26, 0.96),
        (30, 0.68),
    ] {
        r_lip_kick.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl_b2_kick),
            length: 1.8,
            ..Default::default()
        };
    }
    let heel_sculpt = VoiceSculpt {
        stamp: Stamp::Kick,
        pitch_semitones: -8.0,
        pitch_env_semitones: 10.0,
        pitch_env_ms: 32.0,
        sub_exciter: 0.45,
        drive_db: 5.5,
        hpf_hz: 40.0,
        lpf_hz: 170.0,
        resonance_q: 0.82,
        ..VoiceSculpt::default()
    };
    let mut r_heel = new_perc_row(id_b6_grid, 32, 0.84, 0.12, heel_sculpt);
    for &(st, gn) in &[(0usize, 0.80f32), (7, 0.70), (16, 0.80), (26, 0.78)] {
        r_heel.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl_b6_heel),
            length: 1.5,
            ..Default::default()
        };
    }
    pat_kick.rows.push(r_lip_kick);
    pat_kick.rows.push(r_heel);
    let pid_kick = p.add_pattern(pat_kick);
    for &(b0, b1) in &[(4.0, 24.0), (28.0, 29.5), (32.0, 44.0)] {
        p.tracks[t_kick].clips.push(Clip::new_pattern(pid_kick, b0, b1 - b0));
    }

    // ---- 2. Warm Fingerstyle Electric Sub-Bass (`wB5`, `< 95 Hz` Joy-Orbison / UKG Pocket) ----
    let mut pat_bass = StepPattern::new("fingerstyle_ukg_sub");
    pat_bass.set_grid(8.0, 4);
    pat_bass.swing = 0.62;
    let bass_sculpt = VoiceSculpt {
        stamp: Stamp::Raw,
        sub_exciter: 0.34,
        drive_db: 4.2,
        hpf_hz: 34.0,
        lpf_hz: 185.0,
        resonance_q: 0.90,
        ..VoiceSculpt::default()
    };
    let mut r_bass = new_sustained_row(id_b5_grid, 32, 0.90, 0.25, bass_sculpt);
    r_bass.key = Some(key_csm);
    r_bass.choke_group = 4;
    for &(st, pt, gn, ln, sl) in &[
        (0usize, -7.0f32, 0.94f32, 0.58f32, sl_b5_gs2),
        (3, -4.0, 0.84, 0.46, sl_b5_g2),
        (6, -7.0, 0.88, 0.50, sl_b5_gs2),
        (10, -11.0, 0.95, 0.64, sl_b5_gs2),
        (14, -9.0, 0.86, 0.46, sl_b5_g2),
        (16, -7.0, 0.94, 0.58, sl_b5_gs2),
        (19, -2.0, 0.84, 0.44, sl_b5_g2),
        (22, -4.0, 0.86, 0.50, sl_b5_gs2),
        (26, -12.0, 0.95, 0.62, sl_b5_gs2),
        (29, -9.0, 0.85, 0.44, sl_b5_g2),
        (31, -7.0, 0.80, 0.38, sl_b5_gs2),
    ] {
        r_bass.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: ln,
            slice_index: Some(sl),
            ..Default::default()
        };
    }
    pat_bass.rows.push(r_bass);
    let pid_bass = p.add_pattern(pat_bass);
    for &(b0, b1) in &[(4.0, 24.0), (28.0, 29.5), (32.0, 44.0)] {
        p.tracks[t_bass].clips.push(Clip::new_pattern(pid_bass, b0, b1 - b0));
    }

    // ---- 3. Todd-Edwards Pitched-Vocal `Min7` Chords (`wB1`) + Rhodes Jazz-Garage Stabs (`wB4`) + Chime (`wB7`) ----
    let mut pat_chords = StepPattern::new("todd_edwards_vocal_rhodes_chords");
    pat_chords.set_grid(8.0, 4);
    pat_chords.swing = 0.62;
    let vox_chord_sculpt = VoiceSculpt {
        stamp: Stamp::Chord,
        comb_mix: 0.22,
        comb_feedback: 0.88,
        chord: ChordVoicing::Min7,
        shimmer: 0.18,
        drive_db: 2.4,
        hpf_hz: 245.0,
        lpf_hz: 9800.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let mut r_vox_chd = new_sustained_row(id_b1_grid, 32, 0.90, 0.22, vox_chord_sculpt);
    r_vox_chd.key = Some(key_csm);
    for &(st, pt, gn, ln, sl, vc) in &[
        (0usize, -3.0f32, 0.90f32, 0.56f32, sl_b1_hey, ChordVoicing::Min7),
        (3, 0.0, 0.82, 0.44, sl_b1_hel, ChordVoicing::Min7),
        (6, -3.0, 0.86, 0.52, sl_b1_hey, ChordVoicing::Min7),
        (11, -5.0, 0.80, 0.48, sl_b1_f4, ChordVoicing::Sus4),
        (14, -7.0, 0.84, 0.50, sl_b1_hel, ChordVoicing::Min7),
        (16, -3.0, 0.90, 0.56, sl_b1_hey, ChordVoicing::Min7),
        (19, 2.0, 0.80, 0.44, sl_b1_f4, ChordVoicing::Min7),
        (22, 0.0, 0.86, 0.52, sl_b1_hel, ChordVoicing::Min7),
        (27, -5.0, 0.82, 0.50, sl_b1_hey, ChordVoicing::Sus4),
        (30, -3.0, 0.84, 0.50, sl_b1_hel, ChordVoicing::Min7),
    ] {
        r_vox_chd.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: ln,
            slice_index: Some(sl),
            chord: vc,
            ..Default::default()
        };
    }

    let rhodes_sculpt = VoiceSculpt {
        stamp: Stamp::Phrase,
        drive_db: 2.2,
        hpf_hz: 225.0,
        lpf_hz: 8200.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let mut r_rhodes = new_sustained_row(id_b4_grid, 32, 0.82, 0.26, rhodes_sculpt);
    r_rhodes.key = Some(key_csm);
    for &(st, sl, gn, ln) in &[
        (0usize, sl_b4_cs3, 0.82f32, 0.72f32),
        (8, sl_b4_a3, 0.76, 0.68),
        (12, sl_b4_cs3, 0.74, 0.58),
        (16, sl_b4_cs3, 0.82, 0.72),
        (24, sl_b4_a3, 0.78, 0.68),
        (28, sl_b4_cs3, 0.74, 0.58),
    ] {
        r_rhodes.steps[st] = Step {
            on: true,
            gain: gn,
            length: ln,
            slice_index: Some(sl),
            ..Default::default()
        };
    }

    let chime_sculpt = VoiceSculpt {
        stamp: Stamp::Phrase,
        drive_db: 1.5,
        hpf_hz: 420.0,
        lpf_hz: 14_000.0,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let mut r_chime = new_sustained_row(id_b7_grid, 32, 0.76, 0.22, chime_sculpt);
    for &(st, pt, gn) in &[(6usize, 0.0f32, 0.70f32), (14, 0.0, 0.74), (22, -2.0, 0.70), (30, -2.0, 0.76)] {
        r_chime.steps[st] = Step {
            on: true,
            pitch: pt,
            gain: gn,
            length: 0.48,
            slice_index: Some(sl_b7_chime),
            ..Default::default()
        };
    }

    pat_chords.rows.push(r_vox_chd);
    pat_chords.rows.push(r_rhodes);
    pat_chords.rows.push(r_chime);
    let pid_chords = p.add_pattern(pat_chords);
    p.tracks[t_chords].clips.push(Clip::new_pattern(pid_chords, 0.0, PIECE2_BEATS));

    // ---- 4. Vocal-Snippet & Footwork Percussion Lattice (`wB2` + `wB3` + `wB6` + `wB7`, `x2/x3/x4` Ratchets!) ----
    let mut pat_perc = StepPattern::new("vox_tap_auction_percussion_lattice");
    pat_perc.set_grid(8.0, 4);
    pat_perc.swing = 0.62;

    let snare_sculpt = VoiceSculpt {
        stamp: Stamp::Snare,
        snare_snap: 0.84,
        drive_db: 6.8,
        hpf_hz: 185.0,
        lpf_hz: 10_500.0,
        resonance_q: 0.82,
        pocket_carve: true,
        ..VoiceSculpt::default()
    };
    let vox_sculpt = VoiceSculpt::preset(Stamp::VoxPerc);

    // Row A: Beatbox inward-`K` snare (`4, 12, 20, 28`)
    let mut r_k_snare = new_perc_row(id_b2_grid, 32, 0.98, 0.09, snare_sculpt);
    for &st in &[4usize, 12, 20, 28] {
        r_k_snare.steps[st] = Step {
            on: true,
            gain: 0.98,
            slice_index: Some(sl_b2_snare),
            length: 1.15,
            ..Default::default()
        };
    }

    // Row B: Beatbox `"ts"` / `"pf"` 2-step mouth-hats
    let mut r_mouth_hat = new_perc_row(id_b2_grid, 32, 0.92, 0.048, vox_sculpt);
    for &(st, sl, gn, rt) in &[
        (0usize, sl_b2_ts, 0.72f32, 1u8),
        (2, sl_b2_ts, 0.88, 1),
        (6, sl_b2_pf, 0.92, 1),
        (10, sl_b2_ts, 0.86, 1),
        (14, sl_b2_pf, 0.90, 2), // x2 mouth-hat ratchet!
        (18, sl_b2_ts, 0.88, 1),
        (22, sl_b2_pf, 0.92, 1),
        (26, sl_b2_ts, 0.86, 1),
        (30, sl_b2_pf, 0.90, 2),
    ] {
        r_mouth_hat.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 2.0 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row C: Auctioneer `"k/t"` & `"tw-"` micro-syllables (`x2` / `x3` / `x4` `cell_salt` ratchets!)
    let mut r_auc_perc = new_perc_row(id_b3_grid, 32, 0.92, 0.046, vox_sculpt);
    r_auc_perc.choke_group = 3;
    for &(st, sl, gn, pt, rt) in &[
        (3usize, sl_b3_kt, 0.86f32, 1.0f32, 1u8),
        (7, sl_b3_tw, 0.90, -1.0, 2),  // x2 syllable skip!
        (9, sl_b3_kt, 0.82, 2.0, 1),
        (15, sl_b3_tw, 0.88, 0.0, 3),  // x3 multi-cell ratchet!
        (19, sl_b3_kt, 0.86, 1.5, 1),
        (23, sl_b3_tw, 0.88, -1.5, 2),
        (25, sl_b3_kt, 0.84, 2.0, 1),
        (31, sl_b3_tw, 0.94, 1.0, 4),  // x4 blistering auctioneer `cell_salt` subdivision burst!
    ] {
        r_auc_perc.steps[st] = Step {
            on: true,
            gain: gn,
            pitch: pt,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 1.5 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row D: `74`-view Metal Tap-Shoe Brushes & Toe Cracks (`wB6`)
    let mut r_tap = new_perc_row(id_b6_grid, 32, 0.92, 0.048, vox_sculpt);
    for &(st, sl, gn, rt) in &[
        (1usize, sl_b6_brush, 0.82f32, 1u8),
        (5, sl_b6_toe, 0.86, 1),
        (11, sl_b6_brush, 0.90, 2),
        (13, sl_b6_toe, 0.84, 1),
        (17, sl_b6_brush, 0.82, 1),
        (21, sl_b6_toe, 0.86, 1),
        (27, sl_b6_brush, 0.92, 3), // x3 tap-shoe shuffle roll!
        (29, sl_b6_toe, 0.86, 1),
    ] {
        r_tap.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: if rt > 1 { 1.0 } else { 0.0 },
            ..Default::default()
        };
    }

    // Row E: 1978 Stern *STARS* Pinball Solenoid Snaps & Knocker (`wB7`)
    let mut r_pinball = new_perc_row(id_b7_grid, 32, 0.84, 0.048, vox_sculpt);
    for &(st, sl, gn, rt) in &[
        (4usize, sl_b7_knock, 0.74f32, 1u8),
        (8, sl_b7_snap, 0.82, 1),
        (12, sl_b7_knock, 0.76, 1),
        (16, sl_b7_snap, 0.80, 1),
        (24, sl_b7_snap, 0.84, 2),
        (28, sl_b7_knock, 0.78, 1),
    ] {
        r_pinball.steps[st] = Step {
            on: true,
            gain: gn,
            slice_index: Some(sl),
            ratchet: rt,
            ratchet_pitch: 2.0,
            ..Default::default()
        };
    }

    pat_perc.rows.push(r_k_snare);
    pat_perc.rows.push(r_mouth_hat);
    pat_perc.rows.push(r_auc_perc);
    pat_perc.rows.push(r_tap);
    pat_perc.rows.push(r_pinball);
    let pid_perc = p.add_pattern(pat_perc);
    p.tracks[t_perc].clips.push(Clip::new_pattern(pid_perc, 0.0, PIECE2_BEATS));

    // ---- 5. Infectious Vocal & Footwork Hooks (`choke_group = 1`, Vision Cutout Stickers!) ----
    let legs_center = ClipTransform { x: 0.0, y: 0.04, scale: 1.06, rotation: 0.0 };
    let auc_right = ClipTransform { x: 0.08, y: 0.04, scale: 1.02, rotation: 0.0 };
    let soul_left = ClipTransform { x: -0.06, y: 0.04, scale: 1.02, rotation: 0.0 };

    // Hook 1 (Bar 1 pre-drop callout `1.5..3.86`): Headless tap trouser legs `"Shuffle, step, heel"`!
    push_phrase_clip(
        &mut p, t_hooks, id_b6_cut, 13.88, 14.92, 1.5, None, 2.95, 3.0, 0.0, legs_center,
    );

    // Hook 2 (Bar 3–4 `10.0..12.54`): Auctioneer `"Would you bid a bid?"` (natural 2-step triplet!)
    push_phrase_clip(
        &mut p, t_hooks, id_b3_cut, 17.56, 18.68, 10.0, None, 1.55, 2.5, 0.06, auc_right,
    );

    // Hook 3 (Bar 5–6 `18.0..21.72`): Soul singer `"st-st-stuttering"`
    push_phrase_clip(
        &mut p, t_hooks, id_b1_cut, 25.90, 27.54, 18.0, None, 2.10, 2.6, -0.06, soul_left,
    );

    // Hook 4 (Bar 7–8 `ISOL8` break `24.0..26.18` -> `29.5..31.86`):
    // Auctioneer `"You able to bid?"` -> headless tap trouser legs `"Shuffle, step, heel"` into the re-drop!
    push_phrase_clip(
        &mut p, t_hooks, id_b3_cut, 16.30, 17.26, 24.0, None, 1.48, 2.5, 0.04, auc_right,
    );
    push_phrase_clip(
        &mut p, t_hooks, id_b6_cut, 13.88, 14.92, 29.5, None, 2.95, 3.0, 0.0, legs_center,
    );

    // Hook 5 (Bar 9–11 Climax Call-and-Response `34.0..36.54` -> `38.5..42.22`):
    // Auctioneer `"Would you bid a bid?"` answered immediately by Soul `"st-st-stuttering"`!
    push_phrase_clip(
        &mut p, t_hooks, id_b3_cut, 17.56, 18.68, 34.0, None, 1.58, 2.5, 0.06, auc_right,
    );
    push_phrase_clip(
        &mut p, t_hooks, id_b1_cut, 25.90, 27.54, 38.5, None, 2.12, 2.6, -0.04, soul_left,
    );

    // ---- 6. Stage Choreography: Rapid-Fire `Tile` / `Split` (`1x1 <-> 2x1 <-> 3x1 <-> 2x2 <-> 3x2`) <-> `Collage` Stickers ----
    // `Layout::Tile` opens Bar 1 (`0.0..1.5`, contiguous `2x2`), `Layout::Split` drives densely-remapped
    // `2x1 / 3x1 / 2x2 / 3x2 / 3x3` `2px` `#111314` grids across fast `VoxPerc` ratchets without sparse empty slots,
    // and `Layout::Collage` pops `#F2EDE0` cream-bordered Vision cutouts over the hairline grid!
    for &(b0, b1) in &[
        (4.0, 10.0),
        (13.0, 18.0),
        (22.0, 24.0),
        (27.0, 29.5),
        (32.0, 34.0),
        (36.8, 38.5),
        (42.5, 44.0),
    ] {
        p.master_punches.push(MasterPunch {
            start_beat: b0,
            end_beat: b1,
            action: PunchAction::Layout(Layout::Split),
        });
    }
    for &(b0, b1) in &[
        (1.5, 4.0),
        (10.0, 13.0),
        (18.0, 22.0),
        (24.0, 27.0),
        (29.5, 32.0),
        (34.0, 36.8),
        (38.5, 42.5),
    ] {
        p.master_punches.push(MasterPunch {
            start_beat: b0,
            end_beat: b1,
            action: PunchAction::Layout(Layout::Collage),
        });
    }

    p.master_punches.push(MasterPunch {
        start_beat: 24.0,
        end_beat: 28.0,
        action: PunchAction::Isolator(BandIsolator {
            mute: [true, false, false, false, false],
            output_db: 2.0,
            ..BandIsolator::default()
        }),
    });

    p
}

// ============================================================================
// Mix Telemetry Helper (`RMS`, `Peak`, `180-450 Hz` Mud Share)
// ============================================================================

fn analyze_mix_telemetry(audio: &StereoBuffer) -> (f32, f32, f32, f32) {
    let n = audio.len().max(1);
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    let sr = audio.sample_rate as f32;
    let mut bp_mud = Biquad::new(FilterKind::BandPass, sr, 300.0, 1.15);
    let mut lp_sub = Biquad::new(FilterKind::LowPass, sr, 95.0, 0.707);
    let mut mud_sq = 0.0f64;
    let mut sub_sq = 0.0f64;

    for i in 0..audio.len() {
        let l = audio.left[i];
        let r = audio.right[i];
        peak = peak.max(l.abs()).max(r.abs());
        let m = 0.5 * (l + r);
        sum_sq += (m * m) as f64;
        let mud = bp_mud.process(m);
        let sub = lp_sub.process(m);
        mud_sq += (mud * mud) as f64;
        sub_sq += (sub * sub) as f64;
    }
    let rms_db = 20.0 * ((sum_sq / n as f64).sqrt().max(1e-9) as f32).log10();
    let peak_db = 20.0 * peak.max(1e-9).log10();
    let mud_pct = (100.0 * mud_sq / sum_sq.max(1e-12)) as f32;
    let sub_pct = (100.0 * sub_sq / sum_sq.max(1e-12)) as f32;
    (rms_db, peak_db, mud_pct, sub_pct)
}

// ============================================================================
// Tests (1 Fast Non-Ignored Structural Gate + `#[ignore]` 60fps Full Renders)
// ============================================================================

#[test]
fn ukg_v004_compositions_structural_sanity() {
    let p1 = build_piece1_flowbee_zeon(load_world_a_stub());
    let p2 = build_piece2_bid_stutter(load_world_b_stub());

    let dur1 = p1.beats_to_secs(PIECE1_BEATS);
    let dur2 = p2.beats_to_secs(PIECE2_BEATS);
    assert!((18.0..=21.5).contains(&dur1), "Piece 1 duration {dur1:.2}s out of 18..21.5s");
    assert!((18.0..=21.5).contains(&dur2), "Piece 2 duration {dur2:.2}s out of 18..21.5s");
    assert_eq!((p1.width, p1.height, p1.fps as u32), (1080, 1920, 60));
    assert_eq!((p2.width, p2.height, p2.fps as u32), (1080, 1080, 60));
    assert_eq!(p1.layout, Layout::Collage);
    assert_eq!(p2.layout, Layout::Tile);

    for (label, prj) in [("Piece 1", &p1), ("Piece 2", &p2)] {
        assert!(prj.synth_parts.is_empty(), "{label}: must have 0 synth oscillators");
        assert!(prj.master_effects.is_empty(), "{label}: must have 0 master blur effects");
        assert!(!prj.master_tape.is_active(), "{label}: must have 0 master tape smear");
        for tr in &prj.tracks {
            assert_eq!(tr.duck_to_opacity, 0.0, "{label}: zero ghosted duck opacity");
            assert_eq!(tr.level_to_opacity, 0.0, "{label}: zero ghosted fader opacity");
            for fx in &tr.effects {
                assert!(
                    !matches!(fx.kind, EffectKind::Kaleido { .. }),
                    "{label}: Kaleido is banned"
                );
                assert_eq!(fx.video, 0.0, "{label}: track audio echo must keep video crisp");
            }
            for cl in &tr.clips {
                if matches!(cl.kind, ClipKind::Pattern(_)) {
                    assert!(
                        cl.loop_beats.is_none(),
                        "{label}: pattern clips loop natively without VoiceEnv pass aliasing"
                    );
                }
                if matches!(cl.kind, ClipKind::Pattern(_) | ClipKind::Snippet) {
                    assert_eq!(cl.visual.feather, 0.0, "{label}: zero border blur");
                    assert_eq!(cl.link.gain_to_opacity, 0.0);
                    assert_eq!(cl.link.envelope_to_opacity, 0.0);
                }
            }
        }
        for mp in &prj.master_punches {
            if let PunchAction::Effect(efx) = mp.action {
                assert!(
                    !matches!(efx.kind, EffectKind::Kaleido { .. }),
                    "{label}: Kaleido punch is banned"
                );
            }
        }
    }

    assert!(p1.patterns.iter().all(|pt| (pt.swing - 0.61).abs() < 1e-4));
    assert!(p2.patterns.iter().all(|pt| (pt.swing - 0.62).abs() < 1e-4));

    let plans1 = plan_project(&p1);
    let plans2 = plan_project(&p2);
    for (label, plans, min_ratchet_salt) in [("Piece 1", &plans1, 2u8), ("Piece 2", &plans2, 3u8)] {
        let all_evs: Vec<_> = plans.iter().flat_map(|cp| cp.parts.iter().flat_map(|(_, e)| e)).collect();
        assert!(
            all_evs.iter().any(|e| e.sculpt.stamp == Stamp::VoxPerc),
            "{label}: must schedule Stamp::VoxPerc vocal-percussion grains"
        );
        assert!(
            all_evs.iter().any(|e| e.tag.cell_salt >= min_ratchet_salt),
            "{label}: must subdivide grid cells via cell_salt >= {min_ratchet_salt}"
        );
        assert!(
            all_evs.iter().any(|e| e.tag.chord_harmony),
            "{label}: must expand Min7 UKG garage chord stabs into harmony voices"
        );
        assert!(
            all_evs.iter().any(|e| e.tag.collage_cutout && e.tag.choke_group == 1),
            "{label}: must spotlight choked segmented foreground hooks"
        );
        // Verify unmuted active picture across closing bars (> 16.0s):
        assert!(
            all_evs
                .iter()
                .any(|e| e.onset > 16.5 && e.sculpt.stamp == Stamp::VoxPerc && e.tag.picture_at(e.onset + 0.02) > 0.9),
            "{label}: VoxPerc grid must stay 100% live and visible through closing bars"
        );
    }
}

#[test]
#[ignore]
fn render_piece1_flowbee_zeon_ukg_sticker_collage_v004() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("create v004 out dir");
    let tag = std::env::var("CHROMAGRAIN_UKG_VERSION").unwrap_or_else(|_| "v004".into());
    let mp4 = dir.join(format!(
        "261114_videograin_flowbee-zeon_ukg-sticker-collage_{tag}.mp4"
    ));

    let p = build_piece1_flowbee_zeon(load_world_a_real());
    let (audio, n_frames) = render_project_to_video(&p, 0.0, PIECE1_BEATS, &mp4)
        .expect("render Piece 1 MP4");
    let (rms_db, peak_db, mud_pct, sub_pct) = analyze_mix_telemetry(&audio);
    eprintln!(
        "[Piece 1] wrote {} ({} frames @ 60fps, {:.2}s) | RMS {:.2} dBFS | Peak {:.2} dBFS | Mud(180-450Hz) {:.1}% | Sub(<95Hz) {:.1}%",
        mp4.display(),
        n_frames,
        audio.duration(),
        rms_db,
        peak_db,
        mud_pct,
        sub_pct,
    );
    assert!(mp4.exists() && mp4.metadata().unwrap().len() > 200_000);
    assert!(peak_db <= -1.38, "Piece 1 pre-AAC peak {peak_db:.2} dBFS exceeds StudioBus ceiling");
    assert!(mud_pct < 20.0, "Piece 1 mud {mud_pct:.1}% >= 20%");
}

#[test]
#[ignore]
fn render_piece2_bid_stutter_vox_percussion_grid_v004() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("create v004 out dir");
    let tag = std::env::var("CHROMAGRAIN_UKG_VERSION").unwrap_or_else(|_| "v004".into());
    let mp4 = dir.join(format!(
        "261114_videograin_bid-stutter_vox-percussion-grid_{tag}.mp4"
    ));

    let p = build_piece2_bid_stutter(load_world_b_real());
    let (audio, n_frames) = render_project_to_video(&p, 0.0, PIECE2_BEATS, &mp4)
        .expect("render Piece 2 MP4");
    let (rms_db, peak_db, mud_pct, sub_pct) = analyze_mix_telemetry(&audio);
    eprintln!(
        "[Piece 2] wrote {} ({} frames @ 60fps, {:.2}s) | RMS {:.2} dBFS | Peak {:.2} dBFS | Mud(180-450Hz) {:.1}% | Sub(<95Hz) {:.1}%",
        mp4.display(),
        n_frames,
        audio.duration(),
        rms_db,
        peak_db,
        mud_pct,
        sub_pct,
    );
    assert!(mp4.exists() && mp4.metadata().unwrap().len() > 200_000);
    assert!(peak_db <= -1.38, "Piece 2 pre-AAC peak {peak_db:.2} dBFS exceeds StudioBus ceiling");
    assert!(mud_pct < 20.0, "Piece 2 mud {mud_pct:.1}% >= 20%");
}

#[test]
#[ignore]
fn render_all_ukg_v004() {
    render_piece1_flowbee_zeon_ukg_sticker_collage_v004();
    render_piece2_bid_stutter_vox_percussion_grid_v004();
}
