//! Live `VIDEOGRAIN` (`v005`) Hardware Console Showcase (`1920x1080 @ 30fps`, `38.5s`):
//! - Strictly ZERO start title card (`title_until: 0.0`) and ZERO end card (`end_card: len + 100.0`).
//! - Opens on Frame `0` directly inside the live `VIDEOGRAIN` hardware console (`Perform`, `collage` mode)
//!   with real Wave 1 crate videos (`Turbo Encabulator`, `Pet Flowbee Groomer` cutout, `Suzuki Omnichord`,
//!   `Solo Rhodes`, `Tap Shuffle`, plus `Tomy Omnibot Zeon` pulled live via `+ Add -> YouTube`).
//! - Demonstrates the complete 4-act human workflow with continuous synced audio + `wgpu` GPU stage:
//!   1. `0:00–0:08`: Live `Perform` (`collage`) -> `+ Add -> YouTube` (`<500 views` + instant pull bar) -> pulls `Tomy Omnibot Zeon`
//!   2. `0:08–0:16`: `Clip` sheet -> `✂ Cutout Subject` (live Apple Vision cutout) + `Vox Perc` + `Vocal Kit → pads`
//!   3. `0:16–0:25`: `Sequence` -> `Disco` & `2-Step` in the 7-Preset Groove Bank + 1-click `Build Set` (`1 intro · 2 drop · 3 break · 4 climax`)
//!   4. `0:25–0:38.5`: `Perform` -> vocal hook pads + `[5..0]` Punch switches (`Grid ×`, `Stutter`, `Cutout`, `Tape Stop`) + `ISOL8` mid-band zine solo -> `Fullscreen` die-cut sticker + packed grid climax!

use crate::showcase::{
    lab, record_with, Caption as FilmCaption, Cue, Film, Gesture, Overlay, RecordOpts, Tour,
    write_wav, FPS,
};
use crate::{App, AppOpts, Clock, LaunchQ, PADS};
use chromagrain_core::chop::{self, Caption, Slice, SliceRole};
use chromagrain_core::envelope::{Adsr, PictureCut, VoiceEnv};
use chromagrain_core::grain::{ChordVoicing, GrainEvent, Stamp, VoiceSculpt, VoiceTag};
use chromagrain_core::media;
use chromagrain_core::music::{semitones_to_ratio, Key, ScaleKind};
use chromagrain_core::seq::{PatternPreset, Step, StepPattern};
use chromagrain_core::tile::{AspectRatio, Layout};
use chromagrain_core::timeline::{Clip, Provenance, Source};
use chromagrain_ui::{ClipTarget, Host, PerformHost, StageAspect, StageMode, View, YoutubeHit};
use eframe::egui;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};

const SHOWCASE_LEN: f64 = 41.6;

fn videograin_root() -> PathBuf {
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

fn load_crate_clip(
    filename: &str,
    title: &str,
    uploader: &str,
    views: u64,
    url: &str,
    window: (f64, f64),
    base_hz: f32,
) -> Source {
    let path = videograin_root().join("src").join(filename);
    let mut s = media::load_source(&path, 48_000)
        .unwrap_or_else(|e| panic!("failed to load crate video {}: {e}", path.display()));
    s.name = title.into();
    s.path = Some(path);
    if base_hz > 0.0 {
        s.base_hz = base_hz;
    }
    s.provenance = Some(Provenance {
        title: title.into(),
        uploader: uploader.into(),
        upload_date: "19840912".into(),
        views: Some(views),
        url: url.into(),
        license: String::new(),
        query: String::new(),
        window,
    });
    s
}

fn push_curated_slice(
    s: &mut Source,
    start: f64,
    end: f64,
    role: SliceRole,
    focus: (f32, f32),
    text: &str,
) -> u16 {
    let dur = s.duration().max(0.1);
    let st = start.clamp(0.0, (dur - 0.05).max(0.0));
    let en = end.clamp(st + 0.04, dur);
    let mut single = vec![Slice::new(st, en)];
    chop::enrich_slices(&mut single, s.audio.as_deref(), s.video.as_deref());
    let mut sl = single.remove(0);
    if role != SliceRole::Raw {
        sl.role_hint = role;
    }
    sl.focus = focus;
    if !text.is_empty() {
        sl.caption = Caption::new(text);
    }
    s.slices.push(sl);
    (s.slices.len() - 1) as u16
}

/// Build a punchy, richly layered 2-step UKG pattern across our loaded crate sources
/// so the live console groove sounds full and authentic right from Frame `0`.
fn populate_console_ukg_pattern(pat: &mut StepPattern, key: Key, n_sources: usize, has_zeon: bool) {
    pat.set_grid(4.0, 4);
    pat.swing = 0.61;
    pat.rows.clear();
    for li in 0..chromagrain_ui::studio::LANES {
        let src = match li {
            0 => 0,                                    // Turbo Encabulator Kick
            1 => 3.min(n_sources.saturating_sub(1)),   // Tap Shuffle Snare/Rim
            2 => 1.min(n_sources.saturating_sub(1)),   // Omnichord Sub
            3 => 1.min(n_sources.saturating_sub(1)),   // Omnichord Min7 Chords
            4 => 4.min(n_sources.saturating_sub(1)),   // Flowbee VoxPerc
            5 => {
                if has_zeon && n_sources > 5 {
                    5 // Omnibot Zeon VoxPerc
                } else {
                    2.min(n_sources.saturating_sub(1)) // Solo Rhodes stabs
                }
            }
            6 => 2.min(n_sources.saturating_sub(1)),   // Solo Rhodes answer chord
            _ => usize::MAX,
        };
        let r = pat.add_row(src);
        pat.rows[r].grains.position = 0.0;
        if matches!(li, 2 | 3 | 6) {
            pat.rows[r].key = Some(key);
        }
    }

    // Row 0: Skipping 2-Step Kick (steps 0, 7, 10 — zero four-on-the-floor beat 4/8!)
    let sc_kick = VoiceSculpt::preset(Stamp::Kick);
    pat.rows[0].sculpt = sc_kick;
    pat.rows[0].grains.sculpt = sc_kick;
    pat.rows[0].grains.gain = 0.96;
    pat.rows[0].grains.duration = 0.13;
    for &(st, g) in &[(0usize, 0.98f32), (7, 0.84), (10, 0.94)] {
        pat.rows[0].steps[st] = Step {
            on: true,
            gain: g,
            slice_index: Some(0),
            ..Step::default()
        };
    }

    // Row 1: Crisp 2 & 4 Tap-Shoe Snare + ghost 15 ratchet
    let sc_snare = VoiceSculpt::preset(Stamp::Snare);
    pat.rows[1].sculpt = sc_snare;
    pat.rows[1].grains.sculpt = sc_snare;
    pat.rows[1].grains.gain = 0.90;
    pat.rows[1].grains.duration = 0.11;
    for &(st, g, rat) in &[(4usize, 0.95f32, 1u8), (12, 0.96, 1), (15, 0.62, 2)] {
        pat.rows[1].steps[st] = Step {
            on: true,
            gain: g,
            ratchet: rat,
            ratchet_pitch: if rat > 1 { 2.0 } else { 0.0 },
            slice_index: Some(0),
            ..Step::default()
        };
    }

    // Row 2: Omnichord Warm Sub-Bass (off-beat garage pocket)
    let mut sc_sub = VoiceSculpt::preset(Stamp::Pluck);
    sc_sub.lpf_hz = 260.0;
    sc_sub.hpf_hz = 32.0;
    sc_sub.drive_db = 3.5;
    pat.rows[2].sculpt = sc_sub;
    pat.rows[2].grains.sculpt = sc_sub;
    pat.rows[2].grains.gain = 0.86;
    pat.rows[2].grains.duration = 0.24;
    for &(st, pt, g) in &[(0usize, -12.0f32, 0.88f32), (3, -12.0, 0.74), (10, -7.0, 0.86), (14, -9.0, 0.76)] {
        pat.rows[2].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: g,
            slice_index: Some(0),
            ..Step::default()
        };
    }

    // Row 3: Omnichord Syncopated `Min7` UKG Chords (keeps background grid visually lit across beats!)
    let mut sc_chd = VoiceSculpt::preset(Stamp::Chord);
    sc_chd.chord = ChordVoicing::Min7;
    sc_chd.hpf_hz = 210.0;
    sc_chd.pocket_carve = true;
    pat.rows[3].sculpt = sc_chd;
    pat.rows[3].grains.sculpt = sc_chd;
    pat.rows[3].grains.gain = 0.74;
    pat.rows[3].grains.duration = 0.30;
    for &(st, pt, ch) in &[
        (2usize, 0.0f32, ChordVoicing::Min7),
        (6, 3.0, ChordVoicing::Min7),
        (11, -2.0, ChordVoicing::Min9),
        (14, 0.0, ChordVoicing::Sus4),
    ] {
        pat.rows[3].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: 0.74,
            chord: ch,
            slice_index: Some(1),
            ..Step::default()
        };
    }

    // Row 4: Flowbee / Tap Off-16th Vocal Percussion (`Stamp::VoxPerc`)
    let sc_vp = VoiceSculpt::preset(Stamp::VoxPerc);
    pat.rows[4].sculpt = sc_vp;
    pat.rows[4].grains.sculpt = sc_vp;
    pat.rows[4].grains.gain = 0.78;
    pat.rows[4].grains.duration = 0.075;
    pat.rows[4].collage_cutout = false;
    for &(st, pt, rat) in &[(1usize, 0.0f32, 1u8), (5, 3.0, 2), (9, -2.0, 1), (13, 5.0, 2)] {
        pat.rows[4].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: 0.76,
            ratchet: rat,
            ratchet_pitch: 2.0,
            slice_index: Some(0),
            ..Step::default()
        };
    }

    // Row 5: Omnibot Servo / Rhodes Shimmer Hats
    pat.rows[5].sculpt = sc_vp;
    pat.rows[5].grains.sculpt = sc_vp;
    pat.rows[5].grains.gain = 0.72;
    pat.rows[5].grains.duration = 0.08;
    for &(st, pt) in &[(2usize, 4.0f32), (6, 0.0), (8, 2.0), (14, 7.0)] {
        pat.rows[5].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: 0.70,
            slice_index: Some(0),
            ..Step::default()
        };
    }

    // Row 6: Rhodes Warm Answer Stab (fills beats 4..8 & 12..16 so stage grid never drops to 0 cells!)
    let mut sc_rh = VoiceSculpt::preset(Stamp::Chord);
    sc_rh.hpf_hz = 230.0;
    sc_rh.pocket_carve = true;
    pat.rows[6].sculpt = sc_rh;
    pat.rows[6].grains.sculpt = sc_rh;
    pat.rows[6].grains.gain = 0.68;
    pat.rows[6].grains.duration = 0.34;
    for &(st, pt) in &[(4usize, 0.0f32), (8, -5.0), (12, -2.0)] {
        pat.rows[6].steps[st] = Step {
            on: true,
            pitch: pt,
            gain: 0.68,
            chord: ChordVoicing::Min7,
            slice_index: Some(0),
            ..Step::default()
        };
    }
}

fn sync_studio_from_pattern(app: &mut App, scene_idx: usize, pat: &StepPattern) {
    let n_sources = app.project.lock().unwrap().sources.len();
    app.studio.swing = pat.swing;
    app.studio.seq.step_ext.retain(|&(sc, _, _), _| sc != scene_idx);
    for li in 0..chromagrain_ui::studio::LANES {
        if let Some(row) = pat.rows.get(li) {
            app.studio.seq.lanes[li] = (row.source < n_sources).then_some(row.source);
            app.studio.lane_sculpt[li] = chromagrain_ui::SculptParams::from_core(
                row.sculpt.stamp,
                row.sculpt,
                row.choke_group,
            );
            for (st_i, st) in row.steps.iter().enumerate() {
                let ch = chromagrain_ui::ChordChoice::from_core(st.chord);
                if st.ratchet > 1 || st.ratchet_pitch.abs() > 1e-3 || ch != chromagrain_ui::ChordChoice::Off {
                    app.studio
                        .seq
                        .step_ext
                        .insert((scene_idx, li, st_i), (st.ratchet.max(1), st.ratchet_pitch, ch));
                }
            }
        }
    }
}

/// Fire an articulated vocal phrase or `VoxPerc` chop onto both the offline audio bus and
/// the real-time `wgpu` `Collage` sticker stage (`bus = 0`).
fn fire_console_sticker_phrase(
    app: &mut App,
    src_idx: usize,
    t0: f64,
    dur_s: f32,
    gain: f32,
    stamp: Stamp,
    focus: (f32, f32),
    voice_id: u64,
) {
    let has_src = app.project.lock().unwrap().sources.get(src_idx).is_some();
    if !has_src {
        return;
    }
    // Release any previous choke-group-1 sticker phrase cleanly so hooks never talk over each other!
    for prev_id in 0x9900u64..0x9910u64 {
        if prev_id != voice_id {
            Host::release_voice(app, prev_id);
        }
    }
    let now = app.app_clock.secs();
    let mut sculpt = VoiceSculpt::preset(stamp);
    if stamp == Stamp::Phrase {
        sculpt.hpf_hz = 185.0;
        sculpt.lpf_hz = 14_600.0;
        sculpt.drive_db = 2.6;
        sculpt.pocket_carve = true;
    }
    let adsr = Adsr {
        attack: 0.005,
        decay: 0.12,
        sustain: 0.94,
        release: 0.08,
        picture: PictureCut::Cut,
        ..Adsr::default()
    };
    let off = now + (dur_s as f64 - 0.06).max(0.08);
    let ev = GrainEvent {
        onset: now,
        source_pos: t0,
        duration: dur_s,
        pitch_ratio: semitones_to_ratio(sculpt.pitch_semitones),
        gain,
        pan: 0.0,
        envelope: 0.02,
        id: voice_id,
        tag: VoiceTag {
            voice: voice_id,
            env: Some(VoiceEnv { adsr, on: now, off }),
            focus,
            hard_burst: true,
            ..Default::default()
        },
        sculpt,
        ..Default::default()
    };
    app.studio.source_focus.insert(src_idx, focus);
    Host::fire(app, 0, src_idx, ev);
}

// --- Tour Hooks ---

/// Fire a lush polyphonic `Stamp::Wash` video-to-chord swell (`Min9` / `Maj7` / `Sus4` / `Fifths`)
/// with scale-locked `comb` resonance, `+12st` shimmer, and soft `PictureCut::Fade` optical crossfade!
fn fire_console_chord_wash_swell(
    app: &mut App,
    src_idx: usize,
    t0: f64,
    dur_s: f32,
    semitones: f32,
    voicing: ChordVoicing,
    gain: f32,
    focus: (f32, f32),
    voice_base: u64,
) {
    let base_hz = {
        let p = app.project.lock().unwrap();
        let Some(s) = p.sources.get(src_idx) else { return };
        s.effective_base_hz()
    };
    let now = app.app_clock.secs();
    let mut sculpt = VoiceSculpt::preset(Stamp::Wash);
    sculpt.chord = voicing;
    sculpt.comb_mix = 0.54;
    sculpt.comb_feedback = 0.89;
    sculpt.shimmer = 0.30;
    sculpt.wow_flutter = 0.24;
    let adsr = Adsr {
        attack: 0.62,
        decay: 0.35,
        sustain: 0.92,
        release: 1.35,
        picture: PictureCut::Fade,
        ..Adsr::default()
    };
    let off = now + (dur_s as f64 - 0.85).max(0.80);
    let key = app.studio.key;
    let comb_hz = chromagrain_core::grain::quantize_comb_hz(
        base_hz.max(55.0) * semitones_to_ratio(semitones),
        Some(&key),
    );
    let ev = GrainEvent {
        onset: now,
        source_pos: t0,
        duration: dur_s,
        pitch_ratio: semitones_to_ratio(semitones),
        gain,
        pan: 0.0,
        envelope: 0.82,
        id: voice_base,
        comb_hz,
        tag: VoiceTag {
            voice: voice_base,
            env: Some(VoiceEnv { adsr, on: now, off }),
            focus,
            is_wash: true,
            ..Default::default()
        },
        sculpt,
        ..Default::default()
    };
    app.studio.source_focus.insert(src_idx, focus);
    let (chord_evs, chord_n) =
        chromagrain_core::grain::expand_chord_grain_inline(&ev, voicing, base_hz, Some(&key));
    for (ci, mut cev) in chord_evs.into_iter().take(chord_n).enumerate() {
        cev.id = voice_base + ci as u64;
        cev.tag.voice = voice_base + ci as u64;
        Host::fire(app, 1, src_idx, cev);
    }
}

fn hook_opening_flowbee(app: &mut App) {
    // Full-body Rick Hunt grooming dog (`18.42..20.00s` — zero disembodied switch hand!)
    fire_console_sticker_phrase(app, 4, 18.42, 1.56, 1.12, Stamp::Phrase, (0.52, 0.44), 0x9901);
}

fn hook_after_zeon_pull(app: &mut App) {
    let n_src = app.project.lock().unwrap().sources.len();
    let zeon_idx = n_src.saturating_sub(1);
    app.studio.columns[5] = Some(zeon_idx);
    PerformHost::load_pad(app, 5, zeon_idx);
    app.studio.sel_pad = 5;
    app.studio.pad_captions[5] = "My name is Zeon".into();
    app.studio.source_focus.insert(zeon_idx, (0.50, 0.46));
    app.mark_now();
}

fn hook_audition_zeon_voxperc(app: &mut App) {
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_sticker_phrase(app, zeon_idx, 4.98, 0.22, 1.05, Stamp::VoxPerc, (0.50, 0.46), 0x9902);
}

fn hook_audition_zeon_phrase(app: &mut App) {
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_sticker_phrase(app, zeon_idx, 15.54, 1.35, 1.15, Stamp::Phrase, (0.50, 0.46), 0x9903);
}

fn hook_curate_pads_after_kit(app: &mut App) {
    let n_src = app.project.lock().unwrap().sources.len();
    let zeon_idx = n_src.saturating_sub(1);
    app.launch_q = LaunchQ::Now;
    app.set_stage_mode(StageMode::Collage);
    // Keep pad 9 = Zeon sticker, pad 10 = Flowbee sticker, pad 11 = Tap Shuffle vocal!
    PerformHost::load_pad(app, 8, zeon_idx);
    app.studio.pad_sculpt[8] = chromagrain_ui::SculptParams::preset(chromagrain_ui::SculptStamp::Phrase);
    app.studio.pad_captions[8] = "My name is Zeon".into();

    PerformHost::load_pad(app, 9, 4);
    app.studio.pad_sculpt[9] = chromagrain_ui::SculptParams::preset(chromagrain_ui::SculptStamp::Phrase);
    app.studio.pad_captions[9] = "bouncing motion works".into();

    PerformHost::load_pad(app, 10, 3);
    app.studio.pad_sculpt[10] = chromagrain_ui::SculptParams::preset(chromagrain_ui::SculptStamp::VoxPerc);
    app.studio.pad_captions[10] = "Shuffle step heel".into();

    // Mute continuous pad loops so the crisp 2-step sequence + triggered hooks stay super clean!
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
        if let Some(seq_tr) = p.tracks.get_mut(PADS) {
            seq_tr.muted = false;
        }
    }
    app.mark_now();
    if !app.playback.is_playing() {
        Host::toggle_play(app);
    }
}

fn hook_enrich_2step_before_build_set(app: &mut App) {
    let key = app.studio.key;
    let n_src = app.project.lock().unwrap().sources.len();
    if let Some(pid) = app.studio.seq.scene_pats[0] {
        let pat_clone = {
            let mut p = app.project.lock().unwrap();
            p.bpm = 134.0;
            if let Some(pat) = p.patterns.get_mut(pid) {
                populate_console_ukg_pattern(pat, key, n_src, true);
                Some(pat.clone())
            } else {
                None
            }
        };
        if let Some(pat) = pat_clone {
            sync_studio_from_pattern(app, 0, &pat);
            app.studio.rev += 1;
            app.with_studio(|st, host| st.seq_build_arrangement(host));
            app.mark_now();
        }
    }
}

fn hook_lock_drop_scene(app: &mut App) {
    // Jump cleanly onto `2 · drop` (bar 2 of the 4-section set = beat 8.0) and hold it for Perform!
    app.studio.seq.scene = 1;
    app.studio.seq.hold_scene = true;
    app.studio.rev += 1;
    app.with_studio(|st, host| st.seq_build_arrangement(host));
    app.set_stage_mode(StageMode::Collage);
    app.mark_now();
}

fn hook_perform_zeon_hook(app: &mut App) {
    // Keep Perform pad tracks muted as one-shot hook triggers so loop tracks don't muddy the mix
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
    }
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_sticker_phrase(app, zeon_idx, 15.54, 1.36, 1.16, Stamp::Phrase, (0.50, 0.46), 0x9904);
}

fn hook_perform_flowbee_hook(app: &mut App) {
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
    }
    fire_console_sticker_phrase(app, 4, 18.42, 1.56, 1.14, Stamp::Phrase, (0.52, 0.44), 0x9905);
}

fn hook_perform_shuffle_hook(app: &mut App) {
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
    }
    fire_console_sticker_phrase(app, 3, 0.62, 1.15, 1.08, Stamp::Phrase, (0.50, 0.56), 0x9906);
}

fn hook_release_isol8_for_drop(app: &mut App) {
    let iso = chromagrain_core::dsp::BandIsolator::default();
    app.set_isolator(iso);
    app.studio.isolator = iso;
}

fn hook_perform_world_domination_drop(app: &mut App) {
    hook_release_isol8_for_drop(app);
    app.set_stage_mode(StageMode::Collage);
    app.studio.punch_fx = [false; 6];
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_sticker_phrase(app, zeon_idx, 29.96, 1.34, 1.18, Stamp::Phrase, (0.50, 0.46), 0x9907);
}

fn hook_enter_ambient_chord_wash(app: &mut App) {
    hook_release_isol8_for_drop(app);
    app.studio.punch_fx = [false; 6];
    // Configure Emitter 1's live bus (`bus = 1`) for warm cathedral reverb + feathered optical crossfades!
    app.studio.emitters[0].macros = [0.68, 0.50, 0.76, 0.0];
    app.studio.emitters[0].adsr.picture = PictureCut::Fade;
    app.with_studio(|st, host| {
        st.apply_pattern_preset(host, PatternPreset::AmbientWash);
        st.seq.hold_scene = true;
        st.rev += 1;
        st.seq_build_arrangement(host);
    });
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
        // Soften the background sequence track slightly so our live polyphonic video-to-chord swells soar!
        if let Some(seq_tr) = p.tracks.get_mut(PADS) {
            seq_tr.muted = false;
            seq_tr.level = 0.58;
        }
        p.master_effects = vec![chromagrain_core::fx::AvEffect {
            kind: chromagrain_core::fx::EffectKind::Reverb {
                size: 0.86,
                damp: 0.28,
                mix: 0.42,
            },
            audio: 0.85,
            video: 0.36,
        }];
    }
    app.set_stage_mode(StageMode::Stack);
    app.mark_now();
    // Swell 1 (`i min9` cathedral chord across Omnichord + Omnibot Zeon spoken vowel!)
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_chord_wash_swell(app, 1, 22.75, 3.10, 0.0, ChordVoicing::Min9, 0.56, (0.52, 0.52), 0x9A10);
    fire_console_chord_wash_swell(app, zeon_idx, 26.05, 2.80, -12.0, ChordVoicing::Fifths, 0.38, (0.50, 0.44), 0x9A20);
}

fn hook_wash_swell_maj7(app: &mut App) {
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..PADS.min(p.tracks.len()) {
            p.tracks[ti].muted = true;
        }
    }
    // Swell 2 (`VI maj7` luminous Rhodes + Omnichord crossfade!)
    fire_console_chord_wash_swell(app, 2, 4.20, 3.20, -4.0, ChordVoicing::Maj7, 0.58, (0.50, 0.48), 0x9A30);
    fire_console_chord_wash_swell(app, 1, 11.93, 2.90, 8.0, ChordVoicing::Fifths, 0.36, (0.48, 0.50), 0x9A40);
}

fn hook_wash_swell_resolve(app: &mut App) {
    // Swell 3 (`iv min9 -> i min9` warm fullscreen resolve across Omnichord + Rhodes + Zeon!)
    let zeon_idx = app.project.lock().unwrap().sources.len().saturating_sub(1);
    fire_console_chord_wash_swell(app, 1, 18.39, 3.45, 0.0, ChordVoicing::Min9, 0.58, (0.52, 0.54), 0x9A50);
    fire_console_chord_wash_swell(app, 2, 11.40, 3.35, -7.0, ChordVoicing::Sus4, 0.46, (0.52, 0.48), 0x9A60);
    fire_console_chord_wash_swell(app, zeon_idx, 26.08, 3.10, -12.0, ChordVoicing::Octaves, 0.34, (0.50, 0.45), 0x9A70);
}

pub fn tour_v005_videograin_showcase() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(FilmCaption { t0, t1, text });

    // Act 1 (0.0..8.0s): Opens live at Frame 0 inside Perform (`collage` mode) -> `+ Add -> YouTube` pull!
    at(0.12, Custom(hook_opening_flowbee));
    say(0.35, 2.15, "live ukg 2-step console · segmented collage & packed video grid");
    at(2.25, Tap(lab("top add")));
    say(2.55, 5.20, "+ add -> youtube · <500 views crate dig & 18s window pull");
    at(2.85, Tap(lab("add Link")));
    at(3.40, Tap(lab("youtube search")));
    at(3.70, Paste("1984 tomy omnibot zeon"));
    at(4.15, Tap(lab("low views")));
    at(4.60, Tap(lab("search youtube")));
    say(5.30, 7.65, "pulling 1984 omnibot zeon straight into the live crate");
    at(5.35, Tap(lab("youtube result 1")));
    at(6.55, Custom(hook_after_zeon_pull));
    at(7.15, Tap(lab("close sheet")));

    // Act 2 (8.0..16.0s): Open Clip -> `Cutout Subject` (Apple Vision) + `Vox Perc` + `Vocal Kit -> pads`!
    at(8.10, Tap(lab("open clip")));
    say(8.45, 11.45, "clip -> cutout subject · live apple vision foreground mask");
    at(9.15, Tap(lab("cutout subject")));
    at(11.35, Tap(lab("sculpt VoxPerc")));
    at(11.40, Custom(hook_audition_zeon_voxperc));
    say(11.70, 15.55, "vox perc & phrase stamps -> 1-click vocal kit to 16 pads");
    at(12.75, Tap(lab("sculpt Phrase")));
    at(12.80, Custom(hook_audition_zeon_phrase));
    at(14.70, Tap(lab("ukg vox kit")));
    at(15.25, Custom(hook_curate_pads_after_kit));

    // Act 3 (16.0..24.6s): Sequence -> 8-Preset Groove Bank (`Disco` -> `2-Step`) + 1-click `Build Set`!
    at(16.05, Tap(lab("Sequence")));
    say(16.45, 20.85, "sequence · 8-preset groove bank flips disco & 2-step live");
    at(17.10, Tap(lab("preset disco")));
    at(19.75, Tap(lab("seed ukg 2step")));
    at(20.05, Custom(hook_enrich_2step_before_build_set));
    say(21.45, 24.50, "1-click build set · intro, drop, break & climax arranged");
    at(22.05, Tap(lab("build set")));
    at(23.55, Tap(lab("scene 2")));
    at(24.00, Custom(hook_lock_drop_scene));

    // Act 4A (24.8..33.4s): Perform UKG Drop -> Die-cut stickers + Punch switches + ISOL8 zine solo!
    at(24.80, Tap(lab("Perform")));
    say(25.15, 28.30, "perform · die-cut robot & flowbee vocal hooks over 2-step");
    at(25.30, Tap(lab("pad 9")));
    at(25.35, Custom(hook_perform_zeon_hook));
    at(26.90, Tap(lab("pad 10")));
    at(26.95, Custom(hook_perform_flowbee_hook));

    say(28.50, 33.10, "punch grid & stutter · isol8 mid-band zine solo -> full drop");
    at(28.60, Tap(lab("punch grid")));
    at(29.20, Hold(lab("punch stutter"), 0.68));
    at(30.05, Tap(lab("punch grid")));

    // ISOL8 mid-band radio/zine ink solo (`LMF + MF + HMF SOLO`) -> full-spectrum drop!
    at(30.25, Tap(lab("isol8 solo lmf")));
    at(30.50, Tap(lab("isol8 solo mf")));
    at(30.75, Tap(lab("isol8 solo hmf")));
    at(30.90, Tap(lab("pad 11")));
    at(30.95, Custom(hook_perform_shuffle_hook));
    at(31.85, Tap(lab("isol8 solo lmf")));
    at(32.05, Tap(lab("isol8 solo mf")));
    at(32.25, Tap(lab("isol8 solo hmf")));
    at(32.32, Custom(hook_perform_world_domination_drop));

    // Act 4B (33.5..41.6s — 8.1s Lush Video-to-Chord Ambient Wash Closing into Fullscreen!):
    // Same YouTube videos effortlessly transform into warm `Min9 -> Maj7` cathedral chord washes!
    say(33.55, 36.20, "1-tap chord wash · same youtube clips melt into min9 & maj7 chords");
    at(33.55, Tap(lab("perform chord wash")));
    at(33.68, Custom(hook_enter_ambient_chord_wash));
    at(35.30, Tap(lab("pad 2")));
    at(35.35, Custom(hook_wash_swell_maj7));

    say(36.55, 41.05, "fullscreen ambient wash · soft optical dissolves & scale-locked shimmer");
    at(36.35, Tap(lab("Fullscreen")));
    at(38.15, Custom(hook_wash_swell_resolve));

    Tour {
        cues,
        captions: cap,
        title_sub: "",
        end_sub: "",
        // STRICT (#fb-675917599): ZERO start card (`0.0`) and ZERO closing card (`len + 100.0`)!
        title_until: 0.0,
        end_card: SHOWCASE_LEN + 100.0,
        len: SHOWCASE_LEN,
    }
}

fn harness_v005_showcase(opts: &RecordOpts) -> Harness<'static, Film> {
    // Load 5 real Wave 1 crate videos + 1 queued `Tomy Omnibot Zeon` for the live YouTube pull!
    let mut s0_encab = load_crate_clip(
        "wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4",
        "Turbo Encabulator",
        "Dave Rondot",
        412,
        "https://www.youtube.com/watch?v=Ac7G7xOG2Ag",
        (74.0, 29.0),
        83.5,
    );
    s0_encab.slices.clear();
    let _ = push_curated_slice(&mut s0_encab, 8.12, 8.24, SliceRole::Kick, (0.52, 0.36), "");
    let _ = push_curated_slice(&mut s0_encab, 23.62, 23.73, SliceRole::Snare, (0.52, 0.36), "");
    let _ = push_curated_slice(&mut s0_encab, 18.92, 19.01, SliceRole::Hat, (0.52, 0.36), "");

    let mut s1_omni = load_crate_clip(
        "wA7_suzuki_omnichord_chords_j-z1lO9v24M.mp4",
        "Suzuki Omnichord OM-36",
        "Michael Dicken",
        318,
        "https://www.youtube.com/watch?v=j-z1lO9v24M",
        (18.0, 34.0),
        99.4,
    );
    s1_omni.slices.clear();
    let _ = push_curated_slice(&mut s1_omni, 18.39, 18.85, SliceRole::Tonal, (0.52, 0.54), "");
    let _ = push_curated_slice(&mut s1_omni, 22.75, 23.35, SliceRole::Tonal, (0.54, 0.52), "");
    let _ = push_curated_slice(&mut s1_omni, 11.93, 12.55, SliceRole::Tonal, (0.48, 0.50), "");

    let mut s2_rhodes = load_crate_clip(
        "wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4",
        "Solo Rhodes Improv",
        "j_dublay",
        289,
        "https://www.youtube.com/watch?v=MxOzUVFZwCs",
        (1.0, 26.5),
        139.1,
    );
    s2_rhodes.slices = s2_rhodes.auto_slices(12);

    let mut s3_tap = load_crate_clip(
        "wB6_tap_shuffle_heel_6uD9rTyeurk.mp4",
        "Tap Shuffle Step Heel",
        "Gregg Geoffroy",
        74,
        "https://www.youtube.com/watch?v=6uD9rTyeurk",
        (0.2, 34.0),
        138.6,
    );
    s3_tap.slices.clear();
    let _ = push_curated_slice(&mut s3_tap, 2.12, 2.24, SliceRole::Snare, (0.50, 0.58), "");
    let _ = push_curated_slice(&mut s3_tap, 5.42, 5.52, SliceRole::Hat, (0.50, 0.58), "");
    let _ = push_curated_slice(
        &mut s3_tap,
        0.60,
        1.85,
        SliceRole::Phrase,
        (0.50, 0.56),
        "Shuffle, step, heel!",
    );

    let s4_flowbee_raw = load_crate_clip(
        "wA2_pet_flowbee_groomer_K-iaANk41Oc.mp4",
        "Pet Flowbee Groomer",
        "jlryancom",
        241,
        "https://www.youtube.com/watch?v=K-iaANk41Oc",
        (178.0, 35.0),
        130.8,
    );
    let mut s4_flowbee = media::cutout_source(&s4_flowbee_raw, "person");
    s4_flowbee.name = "Pet Flowbee Groomer".into();
    s4_flowbee.slices.clear();
    let _ = push_curated_slice(&mut s4_flowbee, 18.11, 18.20, SliceRole::Hat, (0.52, 0.42), "");
    let _ = push_curated_slice(
        &mut s4_flowbee,
        12.88,
        14.54,
        SliceRole::Phrase,
        (0.52, 0.38),
        "Now turn on the Flowbee motor",
    );
    let _ = push_curated_slice(
        &mut s4_flowbee,
        18.42,
        20.08,
        SliceRole::Phrase,
        (0.52, 0.44),
        "Bouncing motion works best",
    );

    let mut s5_zeon = load_crate_clip(
        "wA3_omnibot_zeon_robot_E8d7kbbrvQU.mp4",
        "Tomy Omnibot Zeon",
        "Rare Coins & Collectibles",
        184,
        "https://www.youtube.com/watch?v=E8d7kbbrvQU",
        (0.5, 35.0),
        191.2,
    );
    s5_zeon.slices.clear();
    let _ = push_curated_slice(&mut s5_zeon, 4.98, 5.08, SliceRole::Snare, (0.50, 0.48), "");
    let _ = push_curated_slice(&mut s5_zeon, 13.60, 13.69, SliceRole::Hat, (0.50, 0.44), "");
    let _ = push_curated_slice(
        &mut s5_zeon,
        15.54,
        16.92,
        SliceRole::Phrase,
        (0.50, 0.46),
        "My name is Zeon",
    );
    let _ = push_curated_slice(
        &mut s5_zeon,
        29.96,
        31.34,
        SliceRole::Phrase,
        (0.50, 0.46),
        "for world domination",
    );

    let yt_catalog = vec![
        YoutubeHit {
            title: "1984 Tomy Omnibot Zeon Robot Demo (VHS)".into(),
            url: "https://www.youtube.com/watch?v=E8d7kbbrvQU".into(),
            views: Some(184),
            duration_secs: Some(35.0),
            uploader: "Rare Coins & Collectibles".into(),
        },
        YoutubeHit {
            title: "Suzuki Omnichord OM-36 Chord Strums".into(),
            url: "https://www.youtube.com/watch?v=j-z1lO9v24M".into(),
            views: Some(318),
            duration_secs: Some(34.0),
            uploader: "Michael Dicken".into(),
        },
        YoutubeHit {
            title: "Tap Dance Lesson — Shuffle Step Heel".into(),
            url: "https://www.youtube.com/watch?v=6uD9rTyeurk".into(),
            views: Some(74),
            duration_secs: Some(34.0),
            uploader: "Gregg Geoffroy".into(),
        },
        YoutubeHit {
            title: "1980s Kitchen Vacuum Groomer Infomercial".into(),
            url: "https://www.youtube.com/watch?v=K-iaANk41Oc".into(),
            views: Some(14_820),
            duration_secs: Some(92.0),
            uploader: "retro_tv_vault".into(),
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
            app.adds.sim_sources = vec![s5_zeon.clone()];
            app.studio.install_theme(&cc.egui_ctx);
            let key = Key::new(0, ScaleKind::Minor);
            app.studio.key = key;
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.16);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }

            {
                let mut p = app.project.lock().unwrap();
                p.bpm = 134.0;
                p.set_aspect_hd(AspectRatio::Wide16x9);
                p.layout = Layout::Collage;
                p.studio_bus = true;
                p.sources.clear();
                p.tracks.clear();
                p.patterns.clear();
                p.add_source(s0_encab.clone());
                p.add_source(s1_omni.clone());
                p.add_source(s2_rhodes.clone());
                p.add_source(s3_tap.clone());
                p.add_source(s4_flowbee.clone());

                let mut pat0 = StepPattern::new("2step");
                populate_console_ukg_pattern(&mut pat0, key, 5, false);
                let pid0 = p.add_pattern(pat0.clone());
                let ti = p.add_track("sequence");
                p.tracks[ti].clips = vec![Clip::new_pattern(pid0, 0.0, 16.0)];
                drop(p);

                app.loop_on = true;
                app.loop_start = 0.0;
                app.loop_end = 16.0;
                app.playback.set_loop(app.loop_region());
                app.launch_q = LaunchQ::Now;
                Host::set_stage_aspect(&mut app, StageAspect::Wide16x9);

                for c in 0..5 {
                    app.studio.columns[c] = Some(c);
                }
                app.studio.source_focus.insert(0, (0.52, 0.36));
                app.studio.source_focus.insert(1, (0.52, 0.52));
                app.studio.source_focus.insert(2, (0.50, 0.48));
                app.studio.source_focus.insert(3, (0.50, 0.56));
                app.studio.source_focus.insert(4, (0.52, 0.40));

                app.studio.seq.scene_pats[0] = Some(pid0);
                app.studio.seq.scene_names[0] = "2step".into();
                app.studio.seq.chain = vec![(0, 4)];
                app.studio.active_preset = Some(PatternPreset::Ukg2Step);
                sync_studio_from_pattern(&mut app, 0, &pat0);

                // Switch straight into `Perform` + `Collage` + build initial 16-pad UKG kit + start transport!
                app.with_studio(|st, host| {
                    st.set_view(host, View::Perform);
                    st.trigger_ukg_kit(host);
                });
                app.set_stage_mode(StageMode::Collage);
                {
                    let mut p = app.project.lock().unwrap();
                    for pad_ti in 0..PADS.min(p.tracks.len()) {
                        p.tracks[pad_ti].muted = true;
                    }
                    if let Some(seq_tr) = p.tracks.get_mut(PADS) {
                        seq_tr.muted = false;
                        seq_tr.clips = vec![Clip::new_pattern(pid0, 0.0, 16.0)];
                    }
                }
                app.studio.sel_pad = 8;
                app.studio.pad_captions[8] = "Turn on Flowbee".into();
                app.mark_now();
                Host::toggle_play(&mut app);
            }

            let mut overlay = Overlay::default();
            overlay.no_band = true;
            Film { app, overlay }
        })
}

fn mux_v005_showcase_hd(frames: &Path, wav: &Path, out: &Path, len: f64) -> Result<(), String> {
    let comp = "acompressor=threshold=-23dB:ratio=2.1:attack=18:release=220:knee=5";
    let fade_st = (len - 0.64).max(1.0);
    let fade = format!("afade=t=in:st=0.00:d=0.04,afade=t=out:st={fade_st:.2}:d=0.60");
    let af = format!(
        "{comp},{fade},loudnorm=I=-14.7:TP=-2.4:LRA=8,aresample=192000,alimiter=limit=0.7328:attack=1.5:release=35:level=false,aresample=48000"
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
            "7500k",
            "-bufsize",
            "15000k",
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

#[test]
fn v005_showcase_tour_has_zero_title_and_end_cards() {
    let tour = tour_v005_videograin_showcase();
    assert!(
        tour.title_until <= 0.0,
        "v005 showcase must have ZERO start title card (title_until = {})",
        tour.title_until
    );
    assert!(
        tour.end_card >= tour.len + 50.0,
        "v005 showcase must have ZERO closing title card (end_card = {}, len = {})",
        tour.end_card,
        tour.len
    );
    assert!(
        (36.0..=42.0).contains(&tour.len),
        "v005 showcase length must be ~36..42s, got {}",
        tour.len
    );
    for w in tour.cues.windows(2) {
        assert!(w[1].t >= w[0].t, "showcase cues must be sorted monotonically: {} < {}", w[1].t, w[0].t);
    }
    // Verify ClipTarget import stays used cleanly without warnings.
    let _ = ClipTarget::Pad(0);
}

#[test]
#[ignore = "renders full 1920x1080 @ 30fps live VIDEOGRAIN UKG + Ambient Chord Wash console showcase"]
fn render_v005_videograin_console_showcase() {
    render_videograin_console_showcase_inner();
}

#[test]
#[ignore = "renders full 1920x1080 @ 30fps live VIDEOGRAIN v006 UKG + Ambient Chord Wash console showcase"]
fn render_v006_videograin_console_showcase() {
    render_videograin_console_showcase_inner();
}

fn render_videograin_console_showcase_inner() {
    let version = std::env::var("VIDEOGRAIN_SHOWCASE_VERSION").unwrap_or_else(|_| "v006".into());
    let root = videograin_root();
    let out_dir = root.join(&version).join("out");
    std::fs::create_dir_all(&out_dir).expect("create v005/out");
    let out = out_dir.join(format!(
        "261114_videograin_hardware-console_ukg-showcase_{version}.mp4"
    ));
    if std::env::var_os("VIDEOGRAIN_OVERWRITE").is_none() {
        assert!(
            !out.exists(),
            "{} already exists — never overwrite a version",
            out.display()
        );
    }

    let work = std::env::temp_dir().join(format!("videograin_showcase_{version}_{}", std::process::id()));
    let frames = work.join("frames");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&frames).expect("create temp frames dir");

    let tour = tour_v005_videograin_showcase();
    let opts = RecordOpts {
        size: (1600.0, 900.0),
        ppp: 1.2,
        frames_dir: Some(frames.clone()),
        max_secs: std::env::var("VIDEOGRAIN_MAX_SECS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok()),
    };

    let t0 = std::time::Instant::now();
    let mut h = harness_v005_showcase(&opts);
    for _ in 0..4 {
        h.input_mut().time = Some(0.0);
        h.step();
    }
    let rec = record_with(&tour, &opts, h, None);
    eprintln!(
        "[videograin-showcase] recorded {} frames ({}x{}) in {:.1}s",
        rec.frames,
        rec.px.0,
        rec.px.1,
        t0.elapsed().as_secs_f64()
    );
    assert_eq!(rec.px, (1920, 1080), "showcase must render at 1920x1080");

    let wav = work.join("showcase_audio.wav");
    write_wav(&wav, &rec.audio, rec.sample_rate);
    let eff_len = opts.max_secs.unwrap_or(tour.len).min(tour.len);
    mux_v005_showcase_hd(&frames, &wav, &out, eff_len).expect("mux v005 showcase");

    // Clean up temporary PNG frames & scratch WAV immediately!
    let _ = std::fs::remove_dir_all(&work);
    eprintln!("[videograin-showcase] wrote {}", out.display());
}
