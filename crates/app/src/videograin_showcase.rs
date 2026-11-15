//! Live `VIDEOGRAIN` (`v007`) Hardware Console Showcase (`1920x1080 @ 30fps`, `41s`) —
//! `261114_videograin_hardware-console_disco-ukg-showcase_v007.mp4`.
//!
//! `v006` review (#fb-701052396: "this sounds pretty bad") traced to the console wiring, not the
//! engine: lanes were bound to raw load order (a spoken Flowbee "da" hammered the vox-perc lane
//! on every off-16th, a Turbo Encabulator syllable was the kick, a 2-second Rhodes chunk was a
//! "hat", and the 1-click `Disco` preset put a spoken "duh" on the snare and tap shoes on the bass).
//! `v007` records the banger crate (#fb-701021625 / #fb-701041057) through the FIXED product path:
//! every groove preset / `Vocal Kit` / `Chord Wash` click binds each lane to the acoustically best
//! source + curated slice via `seq::assign_curated_kit_to_pattern` / `rebind_pattern_to_lane_kit`.
//!
//! - Strictly ZERO start title card (`title_until: 0.0`) and ZERO end card (`end_card: len + 100.0`).
//! - Load order is deliberately scrambled (speech FIRST) to prove roles come from sound, not order.
//!   1. `0:00–0:09.7`: `Disco` (`124 BPM`) drops on frame 0 — Rhodes `Min9` chords, fingerstyle bass,
//!      beatbox kick/snare, crisp hats, die-cut tap legs + Encabulator `"the government will buy it"`
//!      — while `+ Add -> YouTube` (`<500 views`) pulls `Soul Stutter Acapella` (`125` views).
//!   2. `0:09.7–0:15.5`: `Clip` -> `Cutout Person` + `Vocal Kit → pads` (soulful chop auditions on the bar).
//!   3. `0:15.5–0:24.7`: `Sequence` -> `2-Step` (`132 BPM`, `61%` swing) on the bar line -> `Build Set`
//!      -> hold `2 · drop`.
//!   4. `0:24.7–0:33.7`: `Perform` -> `Grid ×` + one-bar `ISOL8` mid-band radio break on `"It's not
//!      cheap"` -> slam back into the full drop on `"the government will buy it"`.
//!   5. `0:33.7–0:41`: `Chord Wash` over paulstretched Rhodes / bass chops -> `Fullscreen`.

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
use chromagrain_core::seq::{PatternPreset, StepPattern};
use chromagrain_core::tile::{AspectRatio, Layout};
use chromagrain_core::timeline::{Clip, Provenance, Source};
use chromagrain_ui::{ClipTarget, Host, PerformHost, StageAspect, StageMode, View, YoutubeHit};
use eframe::egui;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};

const SHOWCASE_LEN: f64 = 41.0;

/// `124 BPM` Disco bar (`4 beats`).
const BAR_DISCO: f64 = 4.0 * 60.0 / 124.0;
/// The `2-Step` click lands on the bar-10 downbeat (`40 beats @ 124 BPM`).
const T_2STEP: f64 = 10.0 * BAR_DISCO;
/// 2-step bar at the `2-Step` preset's own tempo (`132 BPM`).
const BAR_UKG: f64 = 4.0 * 60.0 / 132.0;
/// Pointer taps register on release (`0.14 s` after the press).
const TAP: f64 = 0.14;

/// Downbeat (seconds) of 2-step bar `n` counted from the `2-Step` click.
const fn ukg_bar(n: f64) -> f64 {
    T_2STEP + n * BAR_UKG
}

// Crate slot indices (deliberately scrambled load order: speech first, instruments last).
const S_ENCAB: usize = 0;
const S_TAP: usize = 1;
const S_BEATBOX: usize = 2;
const S_BASS: usize = 3;
const S_RHODES: usize = 4;
/// `Soul Stutter Acapella` arrives as source 5 via the live `+ Add -> YouTube` pull.
const S_SOUL: usize = 5;

pub(crate) fn videograin_root() -> PathBuf {
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

/// The user's chops on the banger crate (the same slices `disco-pump` / `bid-stutter` used).
fn curate_banger_slices(encab: &mut Source, tap: &mut Source, beatbox: &mut Source, bass: &mut Source, rhodes: &mut Source) {
    encab.slices.clear();
    push_curated_slice(encab, 25.24, 26.70, SliceRole::Phrase, (0.52, 0.38), "the government will buy it");
    push_curated_slice(encab, 23.45, 24.36, SliceRole::Phrase, (0.52, 0.38), "It's not cheap");

    tap.slices.clear();
    push_curated_slice(tap, 8.16, 8.22, SliceRole::Hat, (0.50, 0.62), "");
    push_curated_slice(tap, 19.88, 19.96, SliceRole::Snare, (0.50, 0.62), "");
    push_curated_slice(tap, 18.50, 18.59, SliceRole::Kick, (0.50, 0.62), "");

    beatbox.slices.clear();
    push_curated_slice(beatbox, 27.36, 27.46, SliceRole::Kick, (0.50, 0.42), "");
    push_curated_slice(beatbox, 23.65, 23.76, SliceRole::Snare, (0.50, 0.42), "");
    push_curated_slice(beatbox, 3.22, 3.29, SliceRole::Hat, (0.50, 0.42), "");
    push_curated_slice(beatbox, 15.46, 15.54, SliceRole::Hat, (0.50, 0.42), "");

    bass.slices.clear();
    push_curated_slice(bass, 3.46, 3.96, SliceRole::Tonal, (0.44, 0.54), "");
    push_curated_slice(bass, 4.31, 4.85, SliceRole::Tonal, (0.44, 0.54), "");

    rhodes.slices.clear();
    push_curated_slice(rhodes, 16.32, 16.92, SliceRole::Tonal, (0.38, 0.56), "");
    push_curated_slice(rhodes, 21.46, 22.06, SliceRole::Tonal, (0.38, 0.56), "");
}

fn curate_soul_slices(soul: &mut Source) {
    soul.slices.clear();
    push_curated_slice(soul, 25.90, 27.54, SliceRole::Phrase, (0.50, 0.36), "st-st-stuttering");
    push_curated_slice(soul, 29.50, 30.15, SliceRole::Tonal, (0.50, 0.30), "");
    push_curated_slice(soul, 32.60, 33.35, SliceRole::Tonal, (0.50, 0.30), "");
    push_curated_slice(soul, 11.35, 11.85, SliceRole::Tonal, (0.50, 0.30), "");
}

/// Fire an articulated vocal phrase onto both the offline audio bus and the real-time `wgpu`
/// `Collage` sticker stage (`bus = 0`), choked against any previous showcase phrase.
fn fire_console_sticker_phrase(
    app: &mut App,
    src_idx: usize,
    t0: f64,
    dur_s: f32,
    gain: f32,
    focus: (f32, f32),
    voice_id: u64,
) {
    let has_src = app.project.lock().unwrap().sources.get(src_idx).is_some();
    if !has_src {
        return;
    }
    for prev_id in 0x9900u64..0x9910u64 {
        if prev_id != voice_id {
            Host::release_voice(app, prev_id);
        }
    }
    let now = app.app_clock.secs();
    let mut sculpt = VoiceSculpt::preset(Stamp::Phrase);
    sculpt.hpf_hz = 200.0;
    sculpt.lpf_hz = 13_500.0;
    sculpt.drive_db = 1.8;
    sculpt.pocket_carve = true;
    let adsr = Adsr {
        attack: 0.006,
        decay: 0.12,
        sustain: 0.94,
        release: 0.10,
        picture: PictureCut::Cut,
        ..Adsr::default()
    };
    let off = now + (dur_s as f64 - 0.08).max(0.08);
    let ev = GrainEvent {
        onset: now,
        source_pos: t0,
        duration: dur_s,
        pitch_ratio: 1.0,
        gain,
        pan: 0.0,
        envelope: 0.02,
        id: voice_id,
        tag: VoiceTag {
            voice: voice_id,
            env: Some(VoiceEnv { adsr, on: now, off }),
            focus,
            hard_burst: true,
            choke_group: 1,
            ..Default::default()
        },
        sculpt,
        ..Default::default()
    };
    app.studio.source_focus.insert(src_idx, focus);
    Host::fire(app, 0, src_idx, ev);
}

/// A warm polyphonic video-to-chord swell (`Stamp::Wash`) with soft `PictureCut::Fade`.
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
    sculpt.comb_mix = if voicing == ChordVoicing::Off { 0.10 } else { 0.28 };
    sculpt.comb_feedback = 0.80;
    sculpt.shimmer = 0.18;
    sculpt.wow_flutter = 0.12;
    sculpt.lpf_hz = 3200.0;
    sculpt.hpf_hz = 150.0;
    let adsr = Adsr {
        attack: 0.55,
        decay: 0.35,
        sustain: 0.90,
        release: 1.40,
        picture: PictureCut::Fade,
        ..Adsr::default()
    };
    let off = now + (dur_s as f64 - 0.9).max(0.8);
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

fn mute_pad_tracks(app: &mut App) {
    let mut p = app.project.lock().unwrap();
    for ti in 0..PADS.min(p.tracks.len()) {
        p.tracks[ti].muted = true;
    }
    if let Some(seq_tr) = p.tracks.get_mut(PADS) {
        seq_tr.muted = false;
    }
}

/// Index of the most recently added source (the live YouTube pull).
fn last_source(app: &App) -> usize {
    app.project.lock().unwrap().sources.len().saturating_sub(1)
}

// --- Tour hooks ---

fn hook_after_soul_pull(app: &mut App) {
    let soul = last_source(app);
    app.studio.columns[5] = Some(soul);
    PerformHost::load_pad(app, 5, soul);
    app.studio.sel_pad = 5;
    app.studio.pad_captions[5] = "st-st-stuttering".into();
    app.studio.source_focus.insert(soul, (0.50, 0.34));
    mute_pad_tracks(app);
    app.mark_now();
}

fn hook_soul_chop_audition(app: &mut App) {
    // Bar 7 downbeat: the soul singer's "st-st-stuttering" rides the Disco pocket.
    let soul = last_source(app);
    fire_console_sticker_phrase(app, soul, 25.90, 1.62, 1.02, (0.50, 0.34), 0x9901);
}

fn hook_after_vocal_kit(app: &mut App) {
    app.launch_q = LaunchQ::Now;
    app.set_stage_mode(StageMode::Collage);
    mute_pad_tracks(app);
    app.mark_now();
}

fn hook_soul_chord_audition(app: &mut App) {
    // Bar 8: a soft `Min9` stab from the soul singer's held "hey" (from the fresh Vocal Kit).
    let soul = last_source(app);
    fire_console_chord_wash_swell(app, soul, 29.52, 1.60, 0.0, ChordVoicing::Min9, 0.24, (0.50, 0.32), 0x9A00);
}

fn hook_lock_drop_scene(app: &mut App) {
    app.studio.seq.scene = 1;
    app.studio.seq.hold_scene = true;
    app.studio.rev += 1;
    app.with_studio(|st, host| st.seq_build_arrangement(host));
    app.set_stage_mode(StageMode::Collage);
    app.mark_now();
}

fn hook_enter_perform(app: &mut App) {
    mute_pad_tracks(app);
    app.set_stage_mode(StageMode::Collage);
}

fn hook_isol8_radio_break(app: &mut App) {
    // One bar of telephone-band radio (MF + HMF solo, `700 Hz..8 kHz`) under "It's not cheap".
    let iso = chromagrain_core::dsp::BandIsolator {
        solo: [false, false, true, true, false],
        output_db: 2.0,
        ..chromagrain_core::dsp::BandIsolator::default()
    };
    app.set_isolator(iso);
    app.studio.isolator = iso;
    fire_console_sticker_phrase(app, S_ENCAB, 23.45, 0.94, 1.18, (0.52, 0.38), 0x9902);
}

fn hook_slam_back_drop(app: &mut App) {
    let iso = chromagrain_core::dsp::BandIsolator::default();
    app.set_isolator(iso);
    app.studio.isolator = iso;
    app.set_stage_mode(StageMode::Collage);
    fire_console_sticker_phrase(app, S_ENCAB, 25.24, 1.48, 0.90, (0.52, 0.38), 0x9903);
}

/// Paulstretch (`×8`) one curated chop into a held pad source and add it to the project.
fn add_stretched_pad(app: &mut App, src_idx: usize, t0: f64, len: f64) -> Option<usize> {
    let region = {
        let p = app.project.lock().unwrap();
        p.sources.get(src_idx)?.slice(t0, len)
    };
    let mut pad = chromagrain_core::stretch::paulstretch_source(&region, 8.0);
    pad.slices = chop::equal_slices(pad.duration(), 4);
    let idx = app.project.lock().unwrap().add_source(pad);
    Some(idx)
}

fn hook_wash_pre_swell(app: &mut App) {
    // Two beats before the drop-out the same Rhodes + bass chops, paulstretched `×8` into held
    // pads, start swelling UNDER the groove, so the wash crossfades in instead of hard-cutting.
    let a = add_stretched_pad(app, S_RHODES, 16.32, 0.60);
    let _b = add_stretched_pad(app, S_RHODES, 21.46, 0.60);
    let _c = add_stretched_pad(app, S_BASS, 3.46, 0.50);
    app.mark_now();
    if let Some(a) = a {
        fire_console_chord_wash_swell(app, a, 0.4, 4.6, 0.0, ChordVoicing::Off, 0.36, (0.38, 0.56), 0x9A10);
    }
}

fn set_seq_level(app: &mut App, level: f32) {
    if let Some(t) = app.project.lock().unwrap().tracks.get_mut(PADS) {
        t.level = level;
    }
    app.mark_now();
}
fn hook_groove_fade_1(app: &mut App) {
    set_seq_level(app, 0.72);
}
fn hook_groove_fade_2(app: &mut App) {
    set_seq_level(app, 0.50);
}
fn hook_groove_fade_3(app: &mut App) {
    set_seq_level(app, 0.32);
}
fn hook_groove_fade_4(app: &mut App) {
    set_seq_level(app, 0.18);
}

fn hook_enter_chord_wash(app: &mut App) {
    app.studio.punch_fx = [false; 6];
    app.studio.emitters[0].macros = [0.55, 0.50, 0.62, 0.0];
    app.studio.emitters[0].adsr.picture = PictureCut::Fade;
    {
        let mut p = app.project.lock().unwrap();
        for ti in 0..p.tracks.len() {
            p.tracks[ti].muted = true;
        }
        p.master_effects = vec![chromagrain_core::fx::AvEffect {
            kind: chromagrain_core::fx::EffectKind::Reverb { size: 0.80, damp: 0.45, mix: 0.26 },
            audio: 0.70,
            video: 0.30,
        }];
    }
    app.set_stage_mode(StageMode::Stack);
    app.mark_now();
}

/// Index of stretched pad `k` (`0` Rhodes chord A, `1` Rhodes chord B, `2` bass note).
fn stretched_pad(app: &App, k: usize) -> usize {
    let n = app.project.lock().unwrap().sources.len();
    n.saturating_sub(3) + k
}

fn hook_wash_swell_low(app: &mut App) {
    let bass = stretched_pad(app, 2);
    fire_console_chord_wash_swell(app, bass, 0.3, 4.4, 0.0, ChordVoicing::Octaves, 0.22, (0.44, 0.54), 0x9A20);
    let b = stretched_pad(app, 1);
    fire_console_chord_wash_swell(app, b, 0.4, 4.2, 0.0, ChordVoicing::Off, 0.32, (0.38, 0.56), 0x9A28);
}

/// One beatbox snare hit (`23.65 s` "pf") for the turnaround roll into the 2-step flip.
fn fire_beatbox_snare(app: &mut App, gain: f32, id: u64) {
    let now = app.app_clock.secs();
    let sculpt = VoiceSculpt::preset(Stamp::Snare);
    let ev = GrainEvent {
        onset: now,
        source_pos: 23.65,
        duration: 0.10,
        pitch_ratio: 1.0,
        gain,
        envelope: 0.02,
        id,
        tag: VoiceTag { voice: id, focus: (0.50, 0.42), hard_burst: true, ..Default::default() },
        sculpt,
        ..Default::default()
    };
    Host::fire(app, 0, S_BEATBOX, ev);
}

fn hook_roll_1(app: &mut App) {
    fire_beatbox_snare(app, 0.42, 0x9B01);
}
fn hook_roll_2(app: &mut App) {
    fire_beatbox_snare(app, 0.52, 0x9B02);
}
fn hook_roll_3(app: &mut App) {
    fire_beatbox_snare(app, 0.64, 0x9B03);
}
fn hook_roll_4(app: &mut App) {
    fire_beatbox_snare(app, 0.78, 0x9B04);
}

fn hook_flip_pickup_swell(app: &mut App) {
    // One-beat Rhodes pickup swell into the 2-step flip (no hard splice).
    fire_console_chord_wash_swell(app, S_RHODES, 16.34, 1.60, 0.0, ChordVoicing::Min7, 0.32, (0.38, 0.56), 0x9A40);
}

fn hook_wash_swell_resolve(app: &mut App) {
    let a = stretched_pad(app, 0);
    fire_console_chord_wash_swell(app, a, 1.2, 4.6, 0.0, ChordVoicing::Off, 0.32, (0.38, 0.56), 0x9A30);
}

pub fn tour_v007_videograin_showcase() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(FilmCaption { t0, t1, text });

    // Act 1 (0.0..9.7s): the Disco groove is already dropping on frame 0 -> pull a soul acapella.
    say(0.30, 2.40, "disco · rhodes, bass, beatbox & tap legs · every lane picked by ear");
    at(2.30, Tap(lab("top add")));
    say(3.10, 5.40, "+ add -> youtube · <500 views crate dig");
    at(2.90, Tap(lab("add Link")));
    at(3.40, Tap(lab("youtube search")));
    at(3.70, Paste("soul stutter acapella"));
    at(4.15, Tap(lab("low views")));
    at(4.60, Tap(lab("search youtube")));
    say(6.10, 8.40, "pulling a 125-view soul acapella into the live crate");
    at(5.40, Tap(lab("youtube result 1")));
    at(6.60, Custom(hook_after_soul_pull));
    at(7.20, Tap(lab("close sheet")));

    // Act 2 (8.0..15.5s): Clip -> Cutout Person -> Vocal Kit -> pads, auditioned on the bar.
    at(8.05, Tap(lab("open clip")));
    say(9.10, 11.40, "clip -> cutout person · apple vision die-cut sticker");
    at(9.00, Tap(lab("cutout person")));
    at(6.0 * BAR_DISCO, Custom(hook_soul_chop_audition));
    say(11.95, 15.20, "vocal kit -> pads · sequence lanes follow the same kit");
    at(12.70, Tap(lab("ukg vox kit")));
    at(12.70 + TAP + 0.05, Custom(hook_after_vocal_kit));
    at(7.0 * BAR_DISCO, Custom(hook_soul_chord_audition));

    // Act 3 (15.5..24.7s): Sequence -> 2-Step on the bar line -> Build Set -> hold the drop.
    at(15.55, Tap(lab("Sequence")));
    say(15.90, 18.90, "sequence · disco pumping live on the step matrix");
    say(19.50, 21.70, "2-step · 132 bpm, 61% swing · lanes re-picked by ear");
    at(T_2STEP - BAR_DISCO / 2.0, Custom(hook_flip_pickup_swell));
    // Beatbox snare roll on the last beat (16ths, rising) into the tempo flip.
    let s16 = BAR_DISCO / 16.0;
    at(T_2STEP - 4.0 * s16, Custom(hook_roll_1));
    at(T_2STEP - 3.0 * s16, Custom(hook_roll_2));
    at(T_2STEP - 2.0 * s16, Custom(hook_roll_3));
    at(T_2STEP - 1.0 * s16, Custom(hook_roll_4));
    at(T_2STEP - TAP - 0.04, Tap(lab("seed ukg 2step")));
    say(22.30, 24.50, "build set · intro, drop, break & climax");
    at(ukg_bar(1.0) + 0.40, Tap(lab("build set")));
    at(ukg_bar(2.0) - TAP - 0.30, Tap(lab("scene 2")));
    at(ukg_bar(2.0) - 0.10, Custom(hook_lock_drop_scene));

    // Act 4A (24.7..33.7s): Perform -> Grid × -> one-bar ISOL8 radio break -> slam back.
    at(ukg_bar(3.0) - 0.30, Tap(lab("Perform")));
    at(ukg_bar(3.0) - 0.10, Custom(hook_enter_perform));
    say(25.20, 28.10, "perform · 2-step drop rides · grid × punch");
    at(ukg_bar(4.0) - TAP, Tap(lab("punch grid")));
    say(28.60, 31.80, "isol8 mid-band radio break -> slam back into the drop");
    at(ukg_bar(5.0) - TAP - 0.02, Tap(lab("isol8 solo mf")));
    at(ukg_bar(5.0), Custom(hook_isol8_radio_break));
    at(ukg_bar(6.0) - TAP - 0.02, Tap(lab("punch grid")));
    at(ukg_bar(6.0), Custom(hook_slam_back_drop));

    // Act 4B (33.7..41.0s, 2-step bar 8): Chord Wash -> Fullscreen.
    say(ukg_bar(8.0) + 0.2, 36.80, "chord wash · the same clips melt into min9 & maj7");
    at(ukg_bar(7.5), Custom(hook_wash_pre_swell));
    // Intended groove fade over the last two beats (NOTE: live playback does not yet honor
    // mid-play track-level changes, so this is audibly a no-op; kept so v007 re-renders 1:1).
    at(ukg_bar(7.5) + BAR_UKG / 16.0, Custom(hook_groove_fade_1));
    at(ukg_bar(7.5) + 3.0 * BAR_UKG / 16.0, Custom(hook_groove_fade_2));
    at(ukg_bar(7.5) + 5.0 * BAR_UKG / 16.0, Custom(hook_groove_fade_3));
    at(ukg_bar(7.5) + 7.0 * BAR_UKG / 16.0, Custom(hook_groove_fade_4));
    at(ukg_bar(8.0) - TAP, Tap(lab("perform chord wash")));
    at(ukg_bar(8.0) + 0.02, Custom(hook_enter_chord_wash));
    at(35.60, Tap(lab("Fullscreen")));
    at(36.30, Custom(hook_wash_swell_low));
    say(37.40, 40.40, "fullscreen wash · soft optical dissolves");
    at(37.60, Custom(hook_wash_swell_resolve));

    cues.sort_by(|a, b| a.t.total_cmp(&b.t));
    cap.sort_by(|a, b| a.t0.total_cmp(&b.t0));
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

fn harness_v007_showcase(opts: &RecordOpts) -> Harness<'static, Film> {
    let encab_raw = load_crate_clip(
        "wA1_turbo_encabulator_Ac7G7xOG2Ag.mp4",
        "Turbo Encabulator",
        "Dave Rondot",
        412,
        "https://www.youtube.com/watch?v=Ac7G7xOG2Ag",
        (74.0, 29.0),
        83.5,
    );
    let mut encab = media::cutout_source(&encab_raw, "subject");
    encab.name = "Turbo Encabulator".into();
    let tap_raw = load_crate_clip(
        "wB6_tap_shuffle_heel_6uD9rTyeurk.mp4",
        "Tap Shuffle Step Heel",
        "Gregg Geoffroy",
        74,
        "https://www.youtube.com/watch?v=6uD9rTyeurk",
        (0.2, 34.0),
        138.6,
    );
    let mut tap = media::cutout_source(&tap_raw, "subject");
    tap.name = "Tap Shuffle Step Heel".into();
    let mut beatbox = load_crate_clip(
        "wB2_quarantine_beatbox_HL5rhH4m6Vc.mp4",
        "Quarantine Beatbox",
        "JAM ONE",
        397,
        "https://www.youtube.com/watch?v=HL5rhH4m6Vc",
        (5.5, 33.5),
        138.6,
    );
    let mut bass = load_crate_clip(
        "wB5_fingerstyle_bass_groove_KnvZ81XivtE.mp4",
        "Fingerstyle Bass Groove",
        "Dolman Bass",
        192,
        "https://www.youtube.com/watch?v=KnvZ81XivtE",
        (9.0, 30.0),
        104.1,
    );
    let mut rhodes = load_crate_clip(
        "wB4_solo_rhodes_improv_MxOzUVFZwCs.mp4",
        "Solo Rhodes Improv",
        "j_dublay",
        289,
        "https://www.youtube.com/watch?v=MxOzUVFZwCs",
        (1.0, 26.5),
        139.1,
    );
    curate_banger_slices(&mut encab, &mut tap, &mut beatbox, &mut bass, &mut rhodes);

    let mut soul = load_crate_clip(
        "wB1_soul_stutter_acapella_XUaVI90fs5w.mp4",
        "Soul Stutter Acapella",
        "MissChyna520",
        125,
        "https://www.youtube.com/watch?v=XUaVI90fs5w",
        (9.0, 36.0),
        326.5,
    );
    curate_soul_slices(&mut soul);

    let yt_catalog = vec![
        YoutubeHit {
            title: "Soul Stutter Acapella (kitchen take)".into(),
            url: "https://www.youtube.com/watch?v=XUaVI90fs5w".into(),
            views: Some(125),
            duration_secs: Some(36.0),
            uploader: "MissChyna520".into(),
        },
        YoutubeHit {
            title: "Solo Rhodes Improv in C# minor".into(),
            url: "https://www.youtube.com/watch?v=MxOzUVFZwCs".into(),
            views: Some(289),
            duration_secs: Some(26.5),
            uploader: "j_dublay".into(),
        },
        YoutubeHit {
            title: "Quarantine Beatbox Session".into(),
            url: "https://www.youtube.com/watch?v=HL5rhH4m6Vc".into(),
            views: Some(397),
            duration_secs: Some(33.5),
            uploader: "JAM ONE".into(),
        },
        YoutubeHit {
            title: "Acapella Soul Cover — Full Song".into(),
            url: "https://www.youtube.com/watch?v=0000000000a".into(),
            views: Some(18_420),
            duration_secs: Some(214.0),
            uploader: "vocal_vault".into(),
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
            app.adds.sim_sources = vec![soul.clone()];
            app.studio.install_theme(&cc.egui_ctx);
            // C# minor: the key the Rhodes improv and the soul acapella sit in.
            let key = Key::new(1, ScaleKind::Minor);
            app.studio.key = key;
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.16);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }

            {
                let mut p = app.project.lock().unwrap();
                p.bpm = PatternPreset::DiscoFilterPump.default_bpm() as f64;
                p.set_aspect_hd(AspectRatio::Wide16x9);
                p.layout = Layout::Collage;
                p.studio_bus = true;
                p.sources.clear();
                p.tracks.clear();
                p.patterns.clear();
                // Scrambled on purpose: speech first, drums, then the instruments.
                p.add_source(encab.clone());
                p.add_source(tap.clone());
                p.add_source(beatbox.clone());
                p.add_source(bass.clone());
                p.add_source(rhodes.clone());
                debug_assert_eq!(p.sources.len(), S_SOUL);

                // A 4-bar scene so the Disco `i -> VI -> iv -> v` chord roots all play.
                let mut pat0 = StepPattern::new("disco");
                pat0.set_grid(16.0, 4);
                let pid0 = p.add_pattern(pat0);
                let ti = p.add_track("sequence");
                p.tracks[ti].clips = vec![Clip::new_pattern(pid0, 0.0, 16.0)];
                drop(p);

                app.loop_on = true;
                app.loop_start = 0.0;
                app.loop_end = 16.0;
                app.playback.set_loop(app.loop_region());
                app.launch_q = LaunchQ::Now;
                Host::set_stage_aspect(&mut app, StageAspect::Wide16x9);

                for c in 0..S_SOUL {
                    app.studio.columns[c] = Some(c);
                    app.studio.seq.lanes[c] = Some(c);
                }
                app.studio.source_focus.insert(S_ENCAB, (0.52, 0.38));
                app.studio.source_focus.insert(S_TAP, (0.50, 0.62));
                app.studio.source_focus.insert(S_BEATBOX, (0.50, 0.42));
                app.studio.source_focus.insert(S_BASS, (0.44, 0.54));
                app.studio.source_focus.insert(S_RHODES, (0.38, 0.56));

                app.studio.seq.scene_pats[0] = Some(pid0);
                app.studio.seq.scene_names[0] = "disco".into();
                app.studio.seq.chain = vec![(0, 4)];

                // The product path: 1-click `Disco` binds every lane by ear, then the Vocal Kit pads.
                app.with_studio(|st, host| {
                    st.set_view(host, View::Perform);
                    st.apply_pattern_preset(host, PatternPreset::DiscoFilterPump);
                    st.trigger_ukg_kit(host);
                });
                app.set_stage_mode(StageMode::Collage);
                mute_pad_tracks(&mut app);
                app.studio.sel_pad = 0;
                app.mark_now();
                Host::toggle_play(&mut app);
            }

            let mut overlay = Overlay::default();
            overlay.no_band = true;
            Film { app, overlay }
        })
}

fn mux_v007_showcase_hd(frames: &Path, wav: &Path, out: &Path, len: f64) -> Result<(), String> {
    let comp = "acompressor=threshold=-22dB:ratio=1.8:attack=20:release=240:knee=6";
    let fade_st = (len - 1.4).max(1.0);
    let fade = format!("afade=t=in:st=0.00:d=0.03,afade=t=out:st={fade_st:.2}:d=1.35");
    let af = format!(
        "{comp},{fade},loudnorm=I=-13.4:TP=-2.2:LRA=9,aresample=192000,alimiter=limit=0.60:attack=1.5:release=40:level=false,aresample=48000"
    );
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-framerate", "30", "-i"])
        .arg(frames.join("%05d.png"))
        .arg("-i")
        .arg(wav)
        .args([
            "-af", &af, "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-maxrate", "7500k",
            "-bufsize", "15000k", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "256k", "-movflags",
            "+faststart", "-shortest",
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
fn v007_showcase_tour_has_zero_title_and_end_cards_and_bar_locked_flips() {
    let tour = tour_v007_videograin_showcase();
    assert!(tour.title_until <= 0.0, "ZERO start title card (title_until = {})", tour.title_until);
    assert!(tour.end_card >= tour.len + 50.0, "ZERO closing card (end_card = {}, len = {})", tour.end_card, tour.len);
    assert!((36.0..=42.0).contains(&tour.len), "showcase length ~36..42s, got {}", tour.len);
    for w in tour.cues.windows(2) {
        assert!(w[1].t >= w[0].t, "cues sorted: {} < {}", w[1].t, w[0].t);
    }
    // The 2-Step flip releases just before the bar-10 downbeat of the 124 BPM Disco groove.
    let flip = tour
        .cues
        .iter()
        .find(|c| matches!(&c.g, Gesture::Tap(crate::showcase::Aim::Label(l)) if l == "seed ukg 2step"))
        .expect("2-step tap");
    let release = flip.t + TAP;
    assert!((T_2STEP - 0.12..=T_2STEP).contains(&release), "2-step lands on the bar: {release:.3}");
    // Studio naming rule: nothing is ever called the f-word ("fi" + "nal").
    let banned = ["fi", "nal"].concat();
    assert!(tour.captions.iter().all(|c| !c.text.contains(&banned)), "captions avoid the banned word");
    let _ = ClipTarget::Pad(0);
}

#[test]
#[ignore = "renders the 1920x1080 @ 30fps live VIDEOGRAIN v007 disco/UKG console showcase"]
fn render_v007_videograin_console_showcase() {
    render_videograin_console_showcase_inner();
}

fn render_videograin_console_showcase_inner() {
    let version = std::env::var("VIDEOGRAIN_SHOWCASE_VERSION").unwrap_or_else(|_| "v007".into());
    let root = videograin_root();
    let out_dir = root.join(&version).join("out");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    let out = out_dir.join(format!("261114_videograin_hardware-console_disco-ukg-showcase_{version}.mp4"));
    if std::env::var_os("VIDEOGRAIN_OVERWRITE").is_none() {
        assert!(!out.exists(), "{} already exists — never overwrite a version", out.display());
    }

    let work = std::env::temp_dir().join(format!("videograin_showcase_{version}_{}", std::process::id()));
    let frames = work.join("frames");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&frames).expect("create temp frames dir");

    let tour = tour_v007_videograin_showcase();
    let opts = RecordOpts {
        size: (1600.0, 900.0),
        ppp: 1.2,
        frames_dir: Some(frames.clone()),
        max_secs: std::env::var("VIDEOGRAIN_MAX_SECS").ok().and_then(|s| s.parse::<f64>().ok()),
    };

    let t0 = std::time::Instant::now();
    let mut h = harness_v007_showcase(&opts);
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
    if let Ok(keep) = std::env::var("VIDEOGRAIN_KEEP_WAV") {
        let _ = std::fs::copy(&wav, keep);
    }
    let eff_len = opts.max_secs.unwrap_or(tour.len).min(tour.len);
    mux_v007_showcase_hd(&frames, &wav, &out, eff_len).expect("mux v007 showcase");

    let _ = std::fs::remove_dir_all(&work);
    eprintln!("[videograin-showcase] wrote {}", out.display());
}
