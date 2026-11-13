//! The UX showcase film: the real studio UI, driven by a scripted tour of
//! touches and rendered offscreen (egui_kittest + wgpu) at a fixed 30 fps,
//! while the same audio engine is stepped offline exactly one frame of
//! samples per rendered frame. Never opens a window or an audio device.
//!
//! ```text
//! CHROMAGRAIN_SHOWCASE=v001 cargo test -p chromagrain --release showcase_film -- --ignored --nocapture
//! ```
//!
//! writes `outputs/261112_ux-showcase/<v>/work/frames/*.png` + the wav and
//! muxes `outputs/261112_ux-showcase/<v>/261112_chromagrain_ux-showcase_<v>.mp4`
//! (refuses to overwrite an existing version). The frames are deleted after
//! a successful mux; `CHROMAGRAIN_SHOWCASE_KEEP_FRAMES=1` keeps them. The tour itself is
//! [`tour`]: a list of gestures and captions on a timeline.

use crate::demo::DemoSpec;
use crate::{App, AppOpts, Clock};
use chromagrain_core::timeline::Source;
use chromagrain_ui::theme::{font, font_medium, font_semibold, G0, INK, INK2, INK3};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Stroke};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub const FPS: f64 = 30.0;
/// Height of the caption band under the UI (points).
pub const BAND: f32 = 58.0;

// ------------------------------------------------------------------ tour

/// Where a finger goes; resolved against the live UI when the gesture
/// starts (widget rects come from the accessibility tree).
#[derive(Debug, Clone)]
pub enum Aim {
    /// Centre of the widget with this label.
    Label(String),
    /// Fractions of the screen.
    Screen(f32, f32),
    /// Where emitter `i`'s ring is right now (grab it); with `Off`, the
    /// offset is in grid columns / rows.
    Emitter(usize),
    /// Another aim, moved by points.
    Off(Box<Aim>, f32, f32),
}

pub fn lab(s: &str) -> Aim {
    Aim::Label(s.into())
}


impl Aim {
    pub fn off(self, dx: f32, dy: f32) -> Aim {
        Aim::Off(Box::new(self), dx, dy)
    }
}

#[derive(Clone)]
pub enum Gesture {
    /// Press and release (pointer).
    Tap(Aim),
    /// Press, stay, release after `secs` (pointer).
    Hold(Aim, f64),
    /// Press at the first aim, glide through the rest, release (pointer).
    Drag(Vec<Aim>, f64),
    /// The same with a touch (several can overlap each other and the
    /// pointer: multi-touch).
    Touch(Vec<Aim>, f64),
    Key(egui::Key),
    /// Paste text into the focused field.
    Paste(&'static str),
    /// State / musical setup hook at this timestamp (no finger ring).
    Custom(fn(&mut App)),
    /// Turn a knob: press on it and drag vertically by `dy` points at
    /// each (seconds, dy) key. The ring stays on the knob.
    Knob(&'static str, Vec<(f64, f32)>),
}

pub struct Cue {
    pub t: f64,
    pub g: Gesture,
}

pub struct Caption {
    pub t0: f64,
    pub t1: f64,
    pub text: &'static str,
}

pub struct Tour {
    pub cues: Vec<Cue>,
    pub captions: Vec<Caption>,
    pub title_sub: &'static str,
    pub end_sub: &'static str,
    /// Title card fades out by this time; the end card fades in from
    /// `end_card`; the film ends at `len`.
    pub title_until: f64,
    pub end_card: f64,
    pub len: f64,
}

impl Gesture {
    fn secs(&self) -> f64 {
        match self {
            Gesture::Tap(_) => 0.14,
            Gesture::Hold(_, s) | Gesture::Drag(_, s) | Gesture::Touch(_, s) => *s,
            Gesture::Key(_) | Gesture::Paste(_) | Gesture::Custom(_) => 0.0,
            Gesture::Knob(_, keys) => keys.last().map_or(0.0, |k| k.0),
        }
    }
    fn aims(&self) -> Vec<Aim> {
        match self {
            Gesture::Tap(a) | Gesture::Hold(a, _) => vec![a.clone()],
            Gesture::Drag(v, _) | Gesture::Touch(v, _) => v.clone(),
            Gesture::Knob(l, _) => vec![lab(l)],
            _ => Vec::new(),
        }
    }
    fn is_touch(&self) -> bool {
        matches!(self, Gesture::Touch(..))
    }
}

/// The film's scene setup: the Night Platform demo, with a longer first
/// scene so the Sequence edits are heard before the next scene arrives.
pub fn setup(app: &mut App) {
    app.studio.seq.chain = vec![(0, 12), (1, 4), (2, 4)];
    app.studio.rev += 1;
}

/// The tour: Play → Sequence → Perform → Deep, about 80 s.
pub fn tour() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(Caption { t0, t1, text });
    let em = |i: usize| Aim::Emitter(i);
    let rows = |i: usize, path: &[(f32, f32)]| -> Vec<Aim> { path.iter().map(|(dc, dr)| Aim::Off(Box::new(em(i)), *dc, *dr)).collect() };

    // ---- Play: touch the picture. The first finger lands under the title.
    at(0.3, Touch(rows(0, &[(0.0, 0.0), (0.04, 0.6), (0.0, 1.8)]), 5.4));
    say(2.2, 5.8, "touch a clip to play it");
    say(6.6, 10.8, "two fingers, two emitters");
    at(6.8, Touch(rows(0, &[(0.0, 0.0), (0.03, 0.5), (0.0, 1.0)]), 4.2));
    at(7.2, Touch(rows(1, &[(0.0, 0.0), (-0.04, 0.8), (0.0, 1.8)]), 3.8));
    // One finger keeps playing while the other turns the knobs.
    at(11.4, Touch(rows(0, &[(0.0, 0.0), (0.0, 0.5)]), 15.6));
    say(11.8, 13.9, "grain · sample to cloud");
    at(12.0, Knob("Grain", vec![(0.0, 0.0), (1.5, -118.0)]));
    say(14.6, 16.5, "pitch · always in key");
    at(14.8, Knob("Pitch", vec![(0.0, 0.0), (1.1, -48.0)]));
    say(17.2, 19.1, "smear · space and trails");
    at(17.4, Knob("Smear", vec![(0.0, 0.0), (1.2, -80.0)]));
    // LOOK: slip quickly past crush and jpeg, settle on tint, then kaleido.
    say(19.8, 22.4, "look · picture and sound together");
    at(20.0, Knob("Look", vec![(0.0, 0.0), (0.25, -26.0), (0.42, -132.0), (1.3, -140.0), (1.9, -205.0)]));
    // Deep: the emitter's own page, one tap away.
    at(23.0, Tap(lab("emitter settings")));
    say(23.4, 26.8, "deep · every sound ↔ picture link is a dial");
    at(24.4, Drag(vec![lab("pitch → hue"), lab("pitch → hue").off(150.0, 0.0)], 1.4));
    at(26.6, Tap(lab("Done")));
    // Rec: what you play becomes a take, and plays back hands-free.
    say(27.8, 31.2, "rec turns a phrase into a take");
    at(28.0, Tap(lab("rec")));
    at(28.6, Touch(rows(1, &[(0.0, 0.0), (0.06, 0.7), (0.0, 1.4)]), 2.5));
    at(31.25, Tap(lab("rec")));
    say(31.9, 35.0, "hands off · it plays back");
    at(31.75, Tap(lab("play")));

    // ---- Sequence: tap, drag, hold; scenes and the chain.
    let s = 35.8;
    at(s, Tap(lab("Sequence")));
    say(s + 0.4, s + 2.8, "one lane per clip");
    say(s + 3.5, s + 7.0, "tap a step");
    at(s + 3.6, Tap(lab("lane 6 step 9")));
    at(s + 4.2, Tap(lab("lane 6 step 13")));
    at(s + 4.8, Tap(lab("lane 7 step 7")));
    at(s + 5.5, Tap(lab("lane 7 step 15")));
    say(s + 7.7, s + 10.6, "drag up · pitch");
    at(s + 7.8, Drag(vec![lab("lane 6 step 9"), lab("lane 6 step 9").off(0.0, -50.0)], 1.0));
    at(s + 9.2, Drag(vec![lab("lane 7 step 15"), lab("lane 7 step 15").off(0.0, -34.0)], 0.8));
    say(s + 11.3, s + 15.2, "hold a step · slice, length, chance");
    at(s + 11.4, Hold(lab("lane 5 step 5"), 0.7));
    at(s + 12.7, Tap(lab("slice 12")));
    at(s + 13.6, Drag(vec![lab("chance"), lab("chance").off(0.0, 62.0)], 0.8));
    at(s + 15.0, Key(egui::Key::Escape));
    say(s + 15.9, s + 18.6, "queue a scene · it lands on the bar");
    at(s + 16.0, Tap(lab("scene 3")));
    say(s + 19.3, s + 22.2, "an empty scene extends the song");
    at(s + 19.6, Tap(lab("scene 4")));

    // ---- Perform: layers on the bar, then the picture fills the screen.
    let p = s + 23.0;
    at(p, Tap(lab("Perform")));
    say(p + 0.6, p + 5.6, "pads launch on the bar");
    at(p + 1.2, Tap(lab("pad 5")));
    at(p + 2.9, Tap(lab("pad 6")));
    at(p + 4.6, Tap(lab("pad 7")));
    say(p + 6.6, p + 9.6, "fullscreen · just the picture");
    at(p + 6.8, Tap(lab("Fullscreen")));
    say(p + 10.4, p + 13.0, "tap for the hud · launch from it");
    at(p + 10.6, Tap(Aim::Screen(0.5, 0.42)));
    at(p + 11.8, Tap(lab("mini pad 9")));
    say(p + 13.7, p + 16.6, "tap again · it stops on the bar");
    at(p + 13.8, Tap(lab("mini pad 5")));
    at(p + 14.4, Tap(lab("mini pad 6")));
    at(p + 15.0, Tap(lab("mini pad 7")));
    Tour {
        cues,
        captions: cap,
        title_sub: "a granular video sampler",
        end_sub: "play  ·  sequence  ·  perform",
        title_until: 2.0,
        end_card: p + 19.6,
        len: p + 24.2,
    }
}

fn film2_after_file_add(app: &mut App) {
    if let Some(s) = app.project.lock().unwrap().sources.get_mut(0) {
        s.name = "fountain keys".into();
        s.base_hz = 174.614;
    }
    let e = &mut app.studio.emitters[0];
    e.col = 0;
    e.row = 1.6;
    e.held = true;
    e.macros = [0.22, 0.5, 0.18, 0.0];
    e.adsr.sustain = 0.55;
    e.adsr.release = 0.45;
}

fn film2_after_browse_add(app: &mut App) {
    if let Some(s) = app.project.lock().unwrap().sources.get_mut(1) {
        s.name = "vocal bench".into();
        s.base_hz = 391.995;
    }
    chromagrain_ui::Host::audition(app, 1, 0.35, 2.55);
}

fn film2_after_link_add(app: &mut App) {
    chromagrain_ui::Host::audition(app, 2, 0.0, 1.85);
}

fn film2_after_dig1_add(app: &mut App) {
    chromagrain_ui::Host::audition(app, 3, 8.2, 10.6);
}

fn film2_before_close_add(app: &mut App) {
    chromagrain_ui::Host::stop_audition(app);
}

fn film2_release_intro_emitter(app: &mut App) {
    app.studio.emitters[0].held = false;
    app.studio.emitters[1].col = 3;
    app.studio.emitters[1].row = 5.6;
    app.studio.emitters[1].macros = [0.42, 0.5, 0.28, 0.0];
}

fn film2_start_chop_bed(app: &mut App) {
    let e = &mut app.studio.emitters[0];
    e.col = 0;
    e.row = 1.4;
    e.held = true;
    e.adsr.sustain = 0.26;
}

fn film2_after_slices_to_pads(app: &mut App) {
    app.studio.emitters[0].held = false;
    app.launch_q = crate::LaunchQ::Now;
    let mut p = app.project.lock().unwrap();
    let n_src = p.sources.len();
    // Pads 1–4 hold Playing Vinyl · 1..4 (one-shot slice pads, 0.62s each).
    for i in 0..4 {
        if let Some(t) = p.tracks.get_mut(i) {
            t.muted = true;
            for c in &mut t.clips {
                c.grains.duration = 0.62;
            }
        }
    }
    let campfire_src = p
        .sources
        .iter()
        .position(|s| s.name.to_lowercase().contains("campfire"))
        .unwrap_or(n_src.saturating_sub(1));
    // Deduplicated labels & matched thumbnails on Pads 5–8.
    let labels = [
        (4usize, 0usize, "fountain keys"),
        (5usize, 1usize, "vocal bench"),
        (6usize, 3usize, "rhodes & piano"),
        (7usize, campfire_src, "campfire"),
    ];
    for (pad_i, src_i, name) in labels {
        if src_i < n_src && pad_i < p.tracks.len() {
            let mut c = crate::build_clip(&mut p, src_i, 0.0, crate::PlayMode::Snippet);
            c.length_beats = 4.0;
            c.grains.gain = 0.48;
            p.tracks[pad_i].name = name.into();
            p.tracks[pad_i].clips = vec![c];
            p.tracks[pad_i].muted = true;
        }
    }
    // Rows 3–4 (Pads 9–15): clean cloud pad names without extensions or " cloud" suffix.
    for i in 8..15 {
        let src_i = (i - 8) % n_src.max(1);
        if src_i < n_src && i < p.tracks.len() {
            let clean = chromagrain_ui::studio::strip_media_ext(&p.sources[src_i].name).to_string();
            let mut c = crate::build_clip(&mut p, src_i, 0.0, crate::PlayMode::Grains);
            c.length_beats = 4.0;
            p.tracks[i].name = clean;
            p.tracks[i].clips = vec![c];
            p.tracks[i].muted = true;
        }
    }
}

fn film2_open_pad1_sheet(app: &mut App) {
    {
        let mut p = app.project.lock().unwrap();
        if let Some(t) = p.tracks.get_mut(0) {
            for c in &mut t.clips {
                c.grains.duration = 0.92;
            }
        }
    }
    app.toggle_pad(0);
}

fn film2_widen_slice_dur(app: &mut App) {
    app.pad_tiles = chromagrain_core::tile::TileAssigner::new();
    let mut p = app.project.lock().unwrap();
    for i in 0..4 {
        if let Some(t) = p.tracks.get_mut(i) {
            for c in &mut t.clips {
                c.grains.duration = 1.65;
                if let Some(adsr) = c.grains.adsr.as_mut() {
                    adsr.decay = 0.12;
                    adsr.sustain = 0.92;
                    adsr.release = 0.14;
                    adsr.picture = chromagrain_core::envelope::PictureCut::Cut;
                }
            }
        }
    }
}

/// Start `Pad 5` (`fountain keys`, `src = 0`) held in `slot 0` (Left tile, panned Left `-0.80`)
/// so that tapping `Pad 1` (`Playing Vinyl · 1`) and `Pad 2` (`Playing Vinyl · 2`) naturally
/// claims `slot 1` (Right tile, panned Right `+0.80`) via `fire_pad_slice` and cuts out on release.
fn film2_hold_left_pad5(app: &mut App) {
    let now = app.app_clock.secs();
    for (_, _, ev) in &app.live_events {
        if ev.tag.voice != 0 {
            app.playback.release_voice(ev.tag.voice);
        }
    }
    app.live_events.clear();
    app.pad_tiles = chromagrain_core::tile::TileAssigner::new();
    {
        let mut p = app.project.lock().unwrap();
        if let Some(t) = p.tracks.get_mut(4) {
            t.muted = true;
        }
        for i in 0..2 {
            if let Some(t) = p.tracks.get_mut(i) {
                for c in &mut t.clips {
                    c.grains.duration = 0.82;
                    if let Some(adsr) = c.grains.adsr.as_mut() {
                        adsr.decay = 0.08;
                        adsr.sustain = 0.92;
                        adsr.release = 0.08;
                        adsr.picture = chromagrain_core::envelope::PictureCut::Cut;
                    }
                }
            }
        }
    }
    let v_left = (0x50AD_0000_0000 | (4u64 << 32)) | 901;
    let _ = app.pad_tiles.assign_keyed(v_left, 5, now);
    app.pad_tiles.release(v_left, now + 3.95);
    let adsr = chromagrain_core::envelope::Adsr {
        attack: 0.01,
        decay: 0.10,
        sustain: 0.92,
        release: 0.12,
        picture: chromagrain_core::envelope::PictureCut::Cut,
        ..Default::default()
    };
    let env = Some(chromagrain_core::envelope::VoiceEnv { adsr, on: now, off: now + 3.83 });
    chromagrain_ui::Host::fire(
        app,
        0,
        0,
        chromagrain_core::grain::GrainEvent {
            onset: now,
            source_pos: 0.15,
            duration: 3.95,
            pitch_ratio: 1.0,
            gain: 0.38,
            pan: -0.80,
            envelope: 0.02,
            env_skew: 0.0,
            reverse: false,
            id: v_left,
            tag: chromagrain_core::grain::VoiceTag { voice: v_left, env, tile: Some(0) },
        },
    );
}

fn film2_setup_sequence(app: &mut App) {
    use chromagrain_core::envelope::{Adsr, PictureCut};
    use chromagrain_core::fx::{AvEffect, EffectKind};
    use chromagrain_core::music::{Key, ScaleKind};
    let key = Key { root: 5, scale: ScaleKind::Minor };
    app.studio.key = key;
    let cut_adsr = |rel: f32| {
        Some(Adsr {
            attack: 0.005,
            decay: 0.06,
            sustain: 0.92,
            release: rel,
            picture: PictureCut::Cut,
            ..Default::default()
        })
    };
    {
        let mut p = app.project.lock().unwrap();
        p.bpm = 124.0;
        p.layout = chromagrain_core::tile::Layout::Tile;
        if p.master_effects.is_empty() {
            p.master_effects.push(AvEffect {
                video: 0.0,
                ..AvEffect::new(EffectKind::Reverb { size: 0.48, damp: 0.48, mix: 0.14 })
            });
        }
        // Syncopated 4-lane pocket in F minor so `fountain keys` (steps 1, 9) and
        // `rhodes` (off-beats 4, 12) call-and-response without harmonic collision,
        // while `Playing Vinyl` kick (1, 7, 11) and snare (5, 13) lock the groove:
        if let Some(pid) = app.studio.seq.scene_pats[0] {
            if let Some(pat) = p.patterns.get_mut(pid) {
                pat.swing = 0.04;
                // Row 0 (slot 0): fountain keys — steps 1, 9 (0-indexed 0, 8) + pickup 15 (0-indexed 14)
                if let Some(r) = pat.rows.get_mut(0) {
                    r.key = Some(key);
                    r.grains.duration = 0.42;
                    r.grains.gain = 0.56;
                    r.grains.envelope = 0.04;
                    r.grains.adsr = cut_adsr(0.05);
                    for &(s, sl) in &[(0usize, 0.05f32), (8, 0.40), (14, 0.66)] {
                        r.steps[s].on = true;
                        r.steps[s].slice = sl;
                    }
                }
                // Row 1 (slot 1): vocal bench — syncopated steps 3, 11 (0-indexed 2, 10); steps 7 & 15 tapped live!
                if let Some(r) = pat.rows.get_mut(1) {
                    r.key = Some(key);
                    r.grains.duration = 0.36;
                    r.grains.gain = 0.62;
                    r.grains.pan = -0.22;
                    r.grains.envelope = 0.04;
                    r.grains.adsr = cut_adsr(0.05);
                    r.grains.quantize_amount = 1.0;
                    for &(s, sl, pt) in &[(2usize, 0.04f32, 0.0f32), (10, 0.28, 0.0)] {
                        r.steps[s].on = true;
                        r.steps[s].slice = sl;
                        r.steps[s].pitch = pt;
                    }
                }
                // Row 2 (slot 2): Playing Vinyl — kick (steps 1, 7) and snare (steps 5, 13) tapped live!
                if let Some(r) = pat.rows.get_mut(2) {
                    r.grains.duration = 0.36;
                    r.grains.gain = 0.88;
                    r.grains.envelope = 0.03;
                    r.grains.adsr = cut_adsr(0.05);
                    for (idx, st) in r.steps.iter_mut().enumerate() {
                        st.length = 0.42;
                        st.slice_index = Some(if idx == 4 || idx == 12 { 1 } else { 0 });
                    }
                }
                // Row 3 (slot 3): rhodes and piano (in F minor) — off-beat stabs on steps 4, 12 (0-indexed 3, 11)
                if let Some(r) = pat.rows.get_mut(3) {
                    r.key = Some(key);
                    r.grains.duration = 0.36;
                    r.grains.gain = 0.58;
                    r.grains.pan = 0.24;
                    r.grains.envelope = 0.04;
                    r.grains.adsr = cut_adsr(0.05);
                    for &(s, sl) in &[(3usize, 0.74f32), (11, 0.84)] {
                        r.steps[s].on = true;
                        r.steps[s].slice = sl;
                    }
                }
            }
        }
    }
    if app.playback.is_playing() {
        app.toggle_play();
    }
    app.seek(0.0);
    app.studio.seq.chain = vec![(0, 4)];
    app.studio.rev += 1;
    app.mark_now();
}

fn film2_setup_perform_climax(app: &mut App) {
    // Prepare the Perform pads BEFORE switching from Sequence -> Perform so there
    // is zero audio gap at the view transition, and give each pattern pad its own
    // distinctive thumbnail source (`c.source`).
    app.launch_q = crate::LaunchQ::Now;
    {
        let mut p = app.project.lock().unwrap();
        let n_src = p.sources.len();
        let campfire_src = p
            .sources
            .iter()
            .position(|s| s.name.to_lowercase().contains("campfire"))
            .unwrap_or(n_src.saturating_sub(1));
        p.layout = chromagrain_core::tile::Layout::Tile;
        if let Some(base_pid) = app.studio.seq.scene_pats[1].or(app.studio.seq.scene_pats[0]) {
            let mut base_pat = p.patterns[base_pid].clone();
            // Syncopate across 16 steps so tiles dance across subdivisions (1×1, 2×1, 2×2)
            // with zero harmonic clashes and zero orphan 3-tile gaps:
            // - Steps 0, 8 (beats 1, 3): all 4 rows fire together -> full 2×2 (4 tiles!)
            // - Steps 3, 11 (off-beats 4, 12): rows 1 & 3 (vocal + rhodes) fire -> 2×1 (2 tiles!)
            // - Steps 4, 12 (backbeats 5, 13): rows 0 & 2 (keys + vinyl snare) fire -> 2×1 (2 tiles!)
            // - Steps 6, 14 (off-beats 7, 15): rows 0, 1, 2, 3 fire -> 2×2 (4 tiles!)
            if let Some(r0) = base_pat.rows.get_mut(0) {
                for s in &mut r0.steps {
                    s.on = false;
                }
                for &(s, sl) in &[(0usize, 0.05f32), (4, 0.22), (6, 0.32), (8, 0.40), (12, 0.56), (14, 0.66)] {
                    r0.steps[s].on = true;
                    r0.steps[s].slice = sl;
                }
            }
            if let Some(r1) = base_pat.rows.get_mut(1) {
                for s in &mut r1.steps {
                    s.on = false;
                }
                for &(s, sl, pt) in &[
                    (0usize, 0.04f32, 0.0f32),
                    (3, 0.14, 3.0),
                    (6, 0.22, 0.0),
                    (8, 0.28, 0.0),
                    (11, 0.34, -2.0),
                    (14, 0.40, 0.0),
                ] {
                    r1.steps[s].on = true;
                    r1.steps[s].slice = sl;
                    r1.steps[s].pitch = pt;
                }
            }
            if let Some(r2) = base_pat.rows.get_mut(2) {
                for s in &mut r2.steps {
                    s.on = false;
                }
                for &(s, idx) in &[(0usize, 0u16), (4, 1), (6, 2), (8, 0), (12, 3), (14, 2)] {
                    r2.steps[s].on = true;
                    r2.steps[s].slice_index = Some(idx);
                    r2.steps[s].length = 0.42;
                }
            }
            if let Some(r3) = base_pat.rows.get_mut(3) {
                for s in &mut r3.steps {
                    s.on = false;
                }
                for &(s, sl) in &[(0usize, 0.74f32), (3, 0.78), (6, 0.82), (8, 0.86), (11, 0.76), (14, 0.88)] {
                    r3.steps[s].on = true;
                    r3.steps[s].slice = sl;
                }
            }
            let groups: &[(usize, &str, &[usize], usize, bool)] = &[
                (4, "keys + vocal", &[0, 1], 0, true),
                (5, "vinyl + rhodes", &[2, 3], 3, true),
                (6, "campfire air", &[0], campfire_src, false),
                (7, "full groove", &[0, 1, 2, 3], 1, false),
            ];
            let mut updates = Vec::new();
            for &(pad_i, name, rows, thumb_src, on) in groups {
                let mut pat = base_pat.clone();
                pat.name = name.into();
                for (ri, row) in pat.rows.iter_mut().enumerate() {
                    if !rows.contains(&ri) {
                        row.muted = true;
                    }
                }
                let pid = p.add_pattern(pat);
                let mut c = chromagrain_core::timeline::Clip::new_pattern(pid, 0.0, 4.0);
                c.source = thumb_src;
                c.visual.feather = 0.0;
                c.visual.additive = 0.0;
                updates.push((pad_i, name.to_string(), c, !on));
            }
            let target_tracks = if app.mode == crate::AppMode::Perform {
                &mut p.tracks
            } else {
                &mut app.other_tracks
            };
            for (pad_i, name, c, muted) in updates {
                if pad_i < target_tracks.len() {
                    target_tracks[pad_i].name = name;
                    target_tracks[pad_i].clips = vec![c];
                    target_tracks[pad_i].muted = muted;
                }
            }
        }
    }
    app.fill_rev = 0;
    if !app.playback.is_playing() {
        app.toggle_play();
    }
    app.mark_now();
}

/// Film 2 (`spec_v003_dig-chop-tile-adsr.md`): start empty, add via Files /
/// Browse / Link / Dig (1 + 4), chop Koala-style (`Tap`, drag edge, audition,
/// `Slices -> pads` straight into Perform), finger-drum one-shot slices, shape
/// pad ADSR (`Fade` vs `Cut` live), flip `stack | tile` with isolated Left/Right
/// tile panning, sequence a 2-scene track, and perform in Fullscreen (~82 s).
pub fn tour_film2() -> Tour {
    use Gesture::*;
    let mut cues = Vec::new();
    let mut cap = Vec::new();
    let mut at = |t: f64, g: Gesture| cues.push(Cue { t, g });
    let mut say = |t0: f64, t1: f64, text: &'static str| cap.push(Caption { t0, t1, text });
    let em = |i: usize| Aim::Emitter(i);
    let rows = |i: usize, path: &[(f32, f32)]| -> Vec<Aim> { path.iter().map(|(dc, dr)| Aim::Off(Box::new(em(i)), *dc, *dr)).collect() };

    // ---- Act 1: Start empty (no phantom chords!), add clips every way (Files · Browse · Link · Dig · Hold to dig 4).
    at(2.1, Tap(lab("column 1").off(0.0, 48.0)));
    say(2.2, 5.0, "start empty · add a file");
    at(3.0, Tap(lab("choose files")));
    at(3.6, Custom(film2_after_file_add));

    at(5.2, Tap(lab("add Browse")));
    say(5.7, 8.4, "browse your library");
    at(5.8, Tap(lab("browse search")));
    at(6.1, Paste("vocal"));
    at(6.5, Tap(lab("browse item 1")));
    at(7.1, Custom(film2_after_browse_add));

    at(8.6, Tap(lab("add Link")));
    say(9.1, 12.2, "paste any link · yt-dlp fetches it");
    at(9.1, Tap(lab("link url")));
    at(9.45, Paste("https://youtu.be/CA9rIqpteD8"));
    at(9.9, Tap(lab("fetch link")));
    at(11.05, Custom(film2_after_link_add));

    at(12.4, Tap(lab("add Dig")));
    say(12.9, 16.0, "dig · a creative commons video almost nobody has seen");
    at(13.2, Tap(lab("dig")));
    at(14.55, Custom(film2_after_dig1_add));

    say(16.7, 20.4, "hold to dig 4 · placeholders fill the grid");
    at(16.2, Hold(lab("dig"), 0.68));
    at(17.5, Custom(film2_before_close_add));
    at(17.6, Tap(lab("close sheet")));
    at(20.6, Custom(film2_release_intro_emitter));

    // ---- Act 2: Play the newly dug material on the grid (warm chords + Rhodes + vocal).
    say(21.1, 25.8, "touch the grid · play what you found");
    at(20.8, Touch(rows(0, &[(0.0, 0.0), (0.04, 0.8), (0.0, 1.8)]), 4.9));
    at(21.5, Touch(rows(1, &[(0.0, 0.0), (0.0, 0.5), (-2.0, -3.8)]), 4.1));

    // ---- Act 3: Koala-style Chop -> `Slices -> pads` opens Perform directly.
    at(26.35, Custom(film2_start_chop_bed));
    at(26.5, Hold(lab("column 3"), 0.65));
    say(26.8, 31.0, "hold a clip to chop · tap as it plays");
    at(27.4, Tap(lab("chop tap")));
    at(28.1, Tap(lab("chop tap")));
    at(28.8, Tap(lab("chop tap")));
    at(29.5, Tap(lab("chop tap")));
    at(30.2, Tap(lab("chop tap")));
    at(30.75, Tap(lab("chop tap done")));

    say(31.7, 34.4, "drag an edge · tap a slice to audition");
    at(31.6, Drag(vec![lab("slice 1 end"), lab("slice 1 end").off(24.0, 0.0)], 0.7));
    at(32.6, Tap(lab("chop wave").off(-200.0, 0.0)));
    at(33.5, Tap(lab("chop wave").off(-10.0, 0.0)));

    say(35.1, 38.5, "slices → pads · one slice per pad");
    at(35.0, Tap(lab("slices to pads")));
    at(35.3, Custom(film2_after_slices_to_pads));

    // ---- Act 4: In Perform — finger-drum one-shot slices, shape ADSR (Fade vs Cut live), flip stack -> tile & L/R pan.
    at(35.9, Tap(lab("pad 1")));
    at(36.6, Tap(lab("pad 2")));
    at(37.3, Tap(lab("pad 3")));
    at(38.0, Tap(lab("pad 4")));

    say(39.3, 43.1, "hold a pad · shape attack, decay, sustain, release");
    at(39.0, Hold(lab("pad 1"), 0.55));
    at(39.65, Custom(film2_open_pad1_sheet));
    at(40.25, Drag(vec![lab("adsr R"), lab("adsr R").off(48.0, 0.0)], 0.7));
    at(41.45, Drag(vec![lab("adsr D"), lab("adsr D").off(64.0, 92.0)], 0.7));

    say(43.8, 48.0, "cut · full picture until the sound ends, or fade with it");
    at(43.9, Tap(lab("picture fade")));
    at(45.5, Tap(lab("picture cut")));
    at(47.0, Tap(lab("close sheet")));
    at(47.55, Tap(lab("pad 1")));

    say(48.7, 52.0, "tile · every sounding voice claims a tile");
    at(48.8, Tap(lab("layout tile")));
    at(49.2, Custom(film2_widen_slice_dur));
    at(49.4, Tap(lab("pad 1")));
    at(49.8, Tap(lab("pad 2")));
    at(50.2, Tap(lab("pad 3")));
    at(50.6, Tap(lab("pad 4")));

    say(52.7, 56.8, "left tile, left ear · stereo follows the grid");
    at(52.5, Tap(lab("pad 5")));
    at(52.75, Custom(film2_hold_left_pad5));
    at(53.55, Tap(lab("pad 1")));
    at(55.05, Tap(lab("pad 2")));

    // ---- Act 5: Sequence a 2-scene track from the chops (driving multi-tile cuts on every beat).
    at(57.4, Tap(lab("Sequence")));
    at(57.7, Custom(film2_setup_sequence));
    say(57.8, 61.4, "sequence what you dug · tap steps to build a groove");
    at(58.0, Tap(lab("play")));
    at(58.5, Tap(lab("lane 3 step 1")));
    at(59.1, Tap(lab("lane 3 step 5")));
    at(59.7, Tap(lab("lane 3 step 7")));
    at(60.3, Tap(lab("lane 3 step 13")));
    at(60.9, Tap(lab("lane 2 step 7")));

    say(62.1, 65.4, "hold a step · pick one of the clip's own slices");
    at(62.2, Hold(lab("lane 3 step 7"), 0.65));
    at(63.4, Tap(lab("slice 3")));
    at(64.5, Key(egui::Key::Escape));

    say(66.1, 69.2, "an empty scene copies and extends the song");
    at(66.3, Tap(lab("scene 2")));
    at(67.2, Tap(lab("lane 4 step 11")));
    at(67.9, Tap(lab("lane 2 step 15")));

    // ---- Act 6: Perform & Fullscreen climax (rhythmic sharp cuts across tiles + HUD strip-back).
    at(69.65, Custom(film2_setup_perform_climax));
    at(69.7, Tap(lab("Perform")));
    say(70.2, 73.4, "fullscreen · sharp cuts across every tile");
    at(70.6, Tap(lab("Fullscreen")));

    say(74.1, 77.0, "tap for the hud · launch or strip back on the bar");
    at(74.2, Tap(Aim::Screen(0.5, 0.42)));
    at(75.1, Tap(lab("mini pad 6")));

    Tour {
        cues,
        captions: cap,
        title_sub: "dig  ·  chop  ·  play",
        end_sub: "dig  ·  chop  ·  tile  ·  perform",
        title_until: 2.0,
        end_card: 77.6,
        len: 82.0,
    }
}

// --------------------------------------------------------------- overlay

/// A finger as the overlay draws it.
#[derive(Debug, Clone, Copy)]
struct Finger {
    pos: Pos2,
    /// Seconds since it touched down.
    age: f64,
    /// Seconds since it lifted (None = still down).
    up: Option<f64>,
    /// Turning a knob: drawn with a small "drag up" chevron.
    knob: bool,
}

/// What the film paints over the UI this frame.
#[derive(Default)]
pub struct Overlay {
    fingers: Vec<(Finger, Vec<Pos2>)>,
    caption: Option<(&'static str, f32)>,
    /// No caption band (fullscreen): captions float over the picture.
    floating: bool,
    /// Title card / end card opacity.
    title: f32,
    end: f32,
    title_sub: &'static str,
    end_sub: &'static str,
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Overlay {
    fn paint(&self, ctx: &egui::Context) {
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("showcase")));
        let screen = ctx.screen_rect();
        for (f, trail) in &self.fingers {
            let press = smooth(f.age / 0.16) as f32;
            let lift = f.up.map(|u| smooth(u / 0.42) as f32).unwrap_or(0.0);
            let a = press * (1.0 - lift);
            let rad = 30.0 - 6.0 * press + 20.0 * lift;
            if trail.len() > 1 {
                for (i, w) in trail.windows(2).enumerate() {
                    let k = (i + 1) as f32 / trail.len() as f32;
                    p.line_segment([w[0], w[1]], Stroke::new(2.0 * k, Color32::from_white_alpha((34.0 * k * a) as u8)));
                }
            }
            p.circle_filled(f.pos, rad, Color32::from_white_alpha((30.0 * a) as u8));
            p.circle_stroke(f.pos, rad, Stroke::new(1.5, Color32::from_white_alpha((150.0 * a) as u8)));
            if f.knob {
                let c = f.pos - egui::vec2(0.0, rad + 12.0);
                let st = Stroke::new(1.5, Color32::from_white_alpha((120.0 * a) as u8));
                p.line_segment([c + egui::vec2(-6.0, 4.0), c], st);
                p.line_segment([c, c + egui::vec2(6.0, 4.0)], st);
            }
        }
        if let Some((text, a)) = self.caption {
            // In the caption band under the UI: quiet grey, no box.
            // Same place and type everywhere; over the fullscreen picture
            // a soft shadow keeps it legible.
            let c = egui::pos2(screen.center().x, screen.bottom() - BAND / 2.0 + 4.0 * (1.0 - a));
            if self.floating {
                for (dx, dy) in [(0.0, 1.5), (1.0, 1.0), (-1.0, 1.0), (0.0, 0.0)] {
                    p.text(c + egui::vec2(dx, dy), Align2::CENTER_CENTER, text, font(16.0), Color32::from_black_alpha((150.0 * a) as u8));
                }
                p.text(c, Align2::CENTER_CENTER, text, font(16.0), INK.gamma_multiply(0.82 * a));
            } else {
                p.text(c, Align2::CENTER_CENTER, text, font(16.0), INK2.gamma_multiply(a));
            }
        }
        let t_sub = if self.title_sub.is_empty() { "a granular video sampler" } else { self.title_sub };
        let e_sub = if self.end_sub.is_empty() { "play  ·  sequence  ·  perform" } else { self.end_sub };
        for (a, lines) in [(self.title, ["chromagrain", t_sub]), (self.end, ["chromagrain", e_sub])] {
            if a <= 0.0 {
                continue;
            }
            p.rect_filled(screen, 0.0, G0.gamma_multiply(a));
            let c = screen.center();
            p.text(c - egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, lines[0], font_semibold(46.0), INK.gamma_multiply(a));
            p.text(c + egui::vec2(0.0, 34.0), Align2::CENTER_CENTER, lines[1], font_medium(16.0), INK3.gamma_multiply(a));
        }
    }
}

/// The app inside the film: the real `App`, plus the overlay on top.
pub struct Film {
    pub app: App,
    overlay: Overlay,
}

impl eframe::App for Film {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // The caption band claims the bottom before the studio lays out,
        // so captions never cover the interface.
        // (Fullscreen output goes edge to edge: no band, captions float.)
        if !self.app.fullscreen {
            egui::TopBottomPanel::bottom("film-band")
                .exact_height(BAND)
                .frame(egui::Frame::new().fill(G0))
                .show_separator_line(false)
                .show(ctx, |_| {});
        }
        eframe::App::update(&mut self.app, ctx, frame);
        self.overlay.paint(ctx);
    }
}

// -------------------------------------------------------------- recorder

pub struct RecordOpts {
    /// Logical size and scale (output pixels = size × ppp).
    pub size: (f32, f32),
    pub ppp: f32,
    /// Where PNG frames go (None = render but don't write).
    pub frames_dir: Option<PathBuf>,
    /// Stop after this many seconds (smoke tests).
    pub max_secs: Option<f64>,
}

pub struct Recording {
    pub frames: usize,
    /// Interleaved stereo at `sample_rate`, exactly `frames` frames long.
    pub audio: Vec<f32>,
    pub sample_rate: u32,
    pub px: (u32, u32),
}

/// The Night Platform clips (decoded once per process), or stand-ins.
fn demo_sources(spec: &DemoSpec) -> &'static Vec<Source> {
    static S: OnceLock<Vec<Source>> = OnceLock::new();
    S.get_or_init(|| {
        spec.load_found(48000).unwrap_or_else(|e| {
            eprintln!("showcase: {e} — procedural stand-ins");
            spec.clips
                .iter()
                .enumerate()
                .map(|(i, c)| Source {
                    name: c.name.into(),
                    audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(110.0 * (1.0 + i as f32 * 0.2), 4.0, 48000))),
                    video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(160, 90, 12.0, 4.0))),
                    base_hz: c.base_hz,
                    ..Default::default()
                })
                .collect()
        })
    })
}

fn harness(opts: &RecordOpts) -> Harness<'static, Film> {
    let spec = DemoSpec::night_platform();
    let sources = demo_sources(&spec).clone();
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
            app.studio.install_theme(&cc.egui_ctx);
            app.studio.key = chromagrain_core::music::Key { root: 5, scale: chromagrain_core::music::ScaleKind::Minor };
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.2);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }
            app.install_demo(&spec, sources);
            setup(&mut app);
            Film { app, overlay: Overlay::default() }
        })
}

/// A widget's rect in points (the accessibility tree is in pixels).
fn rect_of(h: &Harness<'static, Film>, label: &str) -> Rect {
    let r = h.get_by_label(label).rect();
    let ppp = h.ctx.pixels_per_point();
    Rect::from_min_max((r.min.to_vec2() / ppp).to_pos2(), (r.max.to_vec2() / ppp).to_pos2())
}

/// The Play grid's rect and cell size (as `play.rs` lays it out).
fn grid_geom(h: &Harness<'static, Film>) -> (Rect, f32, f32) {
    let g = rect_of(h, "play grid");
    (g, (g.width() - 4.0 * 7.0) / 8.0, (g.height() - 4.0 * 7.0) / 8.0)
}

fn resolve(h: &Harness<'static, Film>, aim: &Aim) -> Pos2 {
    match aim {
        Aim::Label(l) => rect_of(h, l).center(),
        Aim::Screen(fx, fy) => {
            let s = h.ctx.screen_rect();
            egui::pos2(s.left() + s.width() * fx, s.top() + s.height() * fy)
        }
        Aim::Emitter(i) => {
            let (g, cw, ch) = grid_geom(h);
            let e = &h.state().app.studio.emitters[*i];
            let y = (g.top() + e.row * (ch + 4.0)).clamp(g.top() + 16.0, g.bottom() - 16.0);
            egui::pos2(g.left() + e.col as f32 * (cw + 4.0) + cw / 2.0, y)
        }
        Aim::Off(a, dx, dy) if matches!(**a, Aim::Emitter(_)) => {
            let (_, cw, ch) = grid_geom(h);
            resolve(h, a) + egui::vec2(dx * (cw + 4.0), dy * (ch + 4.0))
        }
        Aim::Off(a, dx, dy) => resolve(h, a) + egui::vec2(*dx, *dy),
    }
}

/// A knob gesture's drag offset at `dt` seconds: smooth between keys.
fn knob_dy(keys: &[(f64, f32)], dt: f64) -> f32 {
    let Some(last) = keys.last() else { return 0.0 };
    if dt >= last.0 {
        return last.1;
    }
    for w in keys.windows(2) {
        if dt >= w[0].0 && dt < w[1].0 {
            let k = smooth((dt - w[0].0) / (w[1].0 - w[0].0).max(1e-6)) as f32;
            return w[0].1 + (w[1].1 - w[0].1) * k;
        }
    }
    keys[0].1
}

/// Point along a polyline at eased progress `t` (0..1), by arc length.
fn along(pts: &[Pos2], t: f64) -> Pos2 {
    if pts.len() == 1 {
        return pts[0];
    }
    let t = smooth(t) as f32;
    let lens: Vec<f32> = pts.windows(2).map(|w| (w[1] - w[0]).length()).collect();
    let total: f32 = lens.iter().sum::<f32>().max(1e-3);
    let mut d = t * total;
    for (i, l) in lens.iter().enumerate() {
        if d <= *l || i == lens.len() - 1 {
            let k = if *l > 0.0 { (d / l).clamp(0.0, 1.0) } else { 1.0 };
            return pts[i] + (pts[i + 1] - pts[i]) * k;
        }
        d -= l;
    }
    *pts.last().unwrap()
}

struct Live {
    cue: usize,
    /// Knobs: the ring stays here while the pointer drags.
    anchor: Option<Pos2>,
    pts: Vec<Pos2>,
    touch_id: u64,
    trail: Vec<Pos2>,
    up_at: Option<f64>,
    last: Pos2,
}

fn harness_film2(opts: &RecordOpts) -> Harness<'static, Film> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let demo_dir = manifest.join("../../assets/demo");
    let stub = manifest.join("../../tools/ytdlp-stub");
    let stub_dir = manifest.join("../../assets/dig-cc");
    let _ = std::fs::remove_file(stub_dir.join(".calls"));
    std::env::set_var(chromagrain_core::dig::YTDLP_ENV, &stub);
    std::env::set_var("CHROMAGRAIN_YTDLP_STUB_DIR", &stub_dir);
    std::env::set_var(chromagrain_core::dig::DIG_SEED_ENV, "42");
    std::env::set_var(crate::browser::BROWSE_ROOT_ENV, &demo_dir);
    let pick_file = demo_dir.join("night_chords.mp4");
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
            let p_file = pick_file.clone();
            app.pick_override = Some(Box::new(move || vec![p_file.clone()]));
            app.studio.install_theme(&cc.egui_ctx);
            app.studio.key = chromagrain_core::music::Key { root: 5, scale: chromagrain_core::music::ScaleKind::Minor };
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.2);
            match cc.wgpu_render_state.as_ref() {
                Some(rs) => crate::gpu::install(rs),
                None => app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed),
            }
            {
                let mut p = app.project.lock().unwrap();
                p.bpm = 124.0;
                p.width = 480;
                p.height = 270;
            }
            app.browser.ensure_started();
            Film { app, overlay: Overlay::default() }
        })
}

/// Render the tour: every frame to PNG (if asked) and the audio, in sync.
pub fn record(tour: &Tour, opts: &RecordOpts) -> Recording {
    let mut h = harness(opts);
    for _ in 0..6 {
        h.input_mut().time = Some(0.0);
        h.step();
    }
    record_with(tour, opts, h, None)
}

/// Render Film 2 (starting from an empty studio, with pre-warmed Browse thumbnails).
pub fn record_film2(tour: &Tour, opts: &RecordOpts) -> Recording {
    let mut h = harness_film2(opts);
    // Warm up fonts + pre-decode Browse thumbnails so the Browse tab is populated immediately.
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs() < 8 && h.state().app.browser.collections.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(30));
        h.input_mut().time = Some(0.0);
        h.step();
    }
    let paths: Vec<PathBuf> = h
        .state()
        .app
        .browser
        .collections
        .iter()
        .flat_map(|c| c.clips.iter().map(|k| k.path.clone()))
        .collect();
    for p in &paths {
        let _ = h.state_mut().app.browser.thumb(p);
    }
    for _ in 0..24 {
        std::thread::sleep(std::time::Duration::from_millis(30));
        h.input_mut().time = Some(0.0);
        h.step();
    }
    record_with(tour, opts, h, None)
}

fn record_with(tour: &Tour, opts: &RecordOpts, mut h: Harness<'static, Film>, intro_bed: Option<&Source>) -> Recording {
    let sr = h.state().app.project.lock().unwrap().sample_rate;
    let spf = (sr as f64 / FPS).round() as usize;
    let len = opts.max_secs.map_or(tour.len, |m| m.min(tour.len));
    let n = (len * FPS).round() as usize;
    let mut audio = Vec::with_capacity(n * spf * 2);
    let mut live: Vec<Live> = Vec::new();
    let mut started = vec![false; tour.cues.len()];
    let mut next_touch = 10u64;
    let mut px = (0, 0);
    // PNGs are written on worker threads.
    let (tx, rx) = std::sync::mpsc::sync_channel::<(PathBuf, image::RgbaImage)>(8);
    let rx = Arc::new(std::sync::Mutex::new(rx));
    let writers: Vec<_> = (0..4)
        .map(|_| {
            let rx = rx.clone();
            std::thread::spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                let Ok((path, img)) = job else { break };
                let f = std::io::BufWriter::new(std::fs::File::create(&path).expect("frame file"));
                let enc = image::codecs::png::PngEncoder::new_with_quality(
                    f,
                    image::codecs::png::CompressionType::Fast,
                    image::codecs::png::FilterType::Adaptive,
                );
                img.write_with_encoder(enc).expect("png");
            })
        })
        .collect();
    if let Some(d) = &opts.frames_dir {
        std::fs::create_dir_all(d).unwrap();
    }

    for k in 0..n {
        let t = k as f64 / FPS;
        h.state_mut().app.app_clock = Clock::Sim(t);
        if h.state().app.adds.sim_clock.is_some() {
            h.state_mut().app.adds.sim_clock = Some(t);
        }
        // Start cues whose time has come.
        for (ci, cue) in tour.cues.iter().enumerate() {
            if started[ci] || cue.t > t + 1e-9 {
                continue;
            }
            started[ci] = true;
            match &cue.g {
                Gesture::Key(key) => {
                    h.input_mut().events.push(egui::Event::Key { key: *key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
                    h.input_mut().events.push(egui::Event::Key { key: *key, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
                }
                Gesture::Paste(text) => {
                    h.input_mut().events.push(egui::Event::Paste((*text).into()));
                }
                Gesture::Custom(f) => {
                    f(&mut h.state_mut().app);
                }
                g => {
                    let pts: Vec<Pos2> = g.aims().iter().map(|a| resolve(&h, a)).collect();
                    let p0 = pts[0];
                    let touch_id = if g.is_touch() {
                        next_touch += 1;
                        h.input_mut().events.push(touch(next_touch, egui::TouchPhase::Start, p0));
                        next_touch
                    } else {
                        h.input_mut().events.push(egui::Event::PointerMoved(p0));
                        h.input_mut().events.push(egui::Event::PointerButton { pos: p0, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
                        0
                    };
                    let anchor = matches!(g, Gesture::Knob(..)).then_some(p0);
                    live.push(Live { cue: ci, anchor, pts, touch_id, trail: vec![p0], up_at: None, last: p0 });
                }
            }
        }
        // Move / release live fingers.
        for l in live.iter_mut().filter(|l| l.up_at.is_none()) {
            let cue = &tour.cues[l.cue];
            let dur = cue.g.secs().max(1e-3);
            let prog = (t - cue.t) / dur;
            let pos = match &cue.g {
                Gesture::Knob(_, keys) => l.pts[0] + egui::vec2(0.0, knob_dy(keys, t - cue.t)),
                _ => along(&l.pts, prog.min(1.0)),
            };
            if prog >= 1.0 {
                l.up_at = Some(t);
                if l.touch_id > 0 {
                    h.input_mut().events.push(touch(l.touch_id, egui::TouchPhase::End, pos));
                } else {
                    h.input_mut().events.push(egui::Event::PointerMoved(pos));
                    h.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
                    h.input_mut().events.push(egui::Event::PointerGone);
                }
            } else if (pos - l.last).length() > 0.01 {
                if l.touch_id > 0 {
                    h.input_mut().events.push(touch(l.touch_id, egui::TouchPhase::Move, pos));
                } else {
                    h.input_mut().events.push(egui::Event::PointerMoved(pos));
                }
            }
            l.last = pos;
            l.trail.push(pos);
            if l.trail.len() > 8 {
                l.trail.remove(0);
            }
        }
        live.retain(|l| l.up_at.is_none_or(|u| t - u < 0.5));
        // Overlay for this frame.
        let fingers = live
            .iter()
            .map(|l| match l.anchor {
                Some(a) => (Finger { pos: a, age: t - tour.cues[l.cue].t, up: l.up_at.map(|u| t - u), knob: true }, Vec::new()),
                None => (Finger { pos: l.last, age: t - tour.cues[l.cue].t, up: l.up_at.map(|u| t - u), knob: false }, l.trail.clone()),
            })
            .collect();
        let caption = tour.captions.iter().find(|c| t >= c.t0 - 0.35 && t < c.t1 + 0.35).map(|c| {
            let a = smooth((t - c.t0 + 0.35) / 0.35).min(smooth((c.t1 + 0.35 - t) / 0.35));
            (c.text, a as f32)
        });
        let title = 1.0 - smooth((t - (tour.title_until - 0.9)) / 0.9);
        let end = smooth((t - tour.end_card) / 1.0);
        let floating = h.state().app.fullscreen;
        h.state_mut().overlay = Overlay {
            fingers,
            caption,
            floating,
            title: title as f32,
            end: end as f32,
            title_sub: tour.title_sub,
            end_sub: tour.end_sub,
        };
        h.input_mut().time = Some(t);
        h.step();
        // Background jobs (a Rec take bouncing) finish before time moves on,
        // so the film is the same every run.
        let mut waited = 0;
        while h.state().app.busy.is_some() && waited < 3000 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.state_mut().app.poll_workers();
            waited += 1;
        }
        let img = h.render().expect("render");
        px = (img.width(), img.height());
        if let Some(d) = &opts.frames_dir {
            tx.send((d.join(format!("{k:05}.png")), img)).unwrap();
        }
        let (mut l, mut r) = h.state_mut().app.playback.render_offline(spf);
        if let Some(bed) = intro_bed.and_then(|s| s.audio.as_ref()).filter(|_| t < 4.2) {
            let ns = bed.samples.len().max(1);
            for i in 0..spf {
                let st = t + i as f64 / sr as f64;
                let env = (smooth(st / 0.35) * (1.0 - smooth((st - 2.9) / 1.2))) as f32 * 0.24;
                let idx = (k * spf + i) % ns;
                let idx_r = (idx + 720) % ns;
                l[i] += bed.samples[idx] * env;
                r[i] += bed.samples[idx_r] * env;
            }
        }
        for i in 0..spf {
            audio.push(l[i]);
            audio.push(r[i]);
        }
        if k % 150 == 0 {
            eprintln!("showcase: {:5.1}s / {len:.0}s", t);
        }
    }
    drop(tx);
    for w in writers {
        w.join().unwrap();
    }
    Recording { frames: n, audio, sample_rate: sr, px }
}

fn touch(id: u64, phase: egui::TouchPhase, pos: Pos2) -> egui::Event {
    egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos, force: None }
}

/// Write interleaved stereo f32 as a 16-bit wav.
fn write_wav(path: &Path, audio: &[f32], sr: u32) {
    use std::io::Write;
    let data_len = (audio.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sr.to_le_bytes()).unwrap();
    f.write_all(&(sr * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for s in audio {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}

/// A gentle glue compressor: the quiet Play opening and the full Perform
/// set sit closer together without pumping.
const COMP: &str = "acompressor=threshold=-27dB:ratio=2.5:attack=30:release=350:knee=8";

/// Mux frames + audio: H.264 + AAC, loudness-normalised (two-pass
/// loudnorm) with a soft fade at the end.
fn mux(frames: &Path, wav: &Path, out: &Path, fade_from: f64, len: f64) -> Result<(), String> {
    // Sound fades with the end card and is silent before the last frame.
    let fade = format!("afade=t=out:st={:.2}:d={:.2}", fade_from, (len - fade_from - 0.4).max(0.5));
    let pass1 = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(wav)
        .args(["-af", &format!("{COMP},{fade},loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json"), "-f", "null", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    let log = String::from_utf8_lossy(&pass1.stderr);
    let get = |k: &str| -> Option<String> {
        let i = log.rfind(&format!("\"{k}\""))?;
        let rest = &log[i..];
        let v = rest.split(':').nth(1)?.split('"').nth(1)?;
        Some(v.to_string())
    };
    let norm = match (get("input_i"), get("input_tp"), get("input_lra"), get("input_thresh"), get("target_offset")) {
        (Some(i), Some(tp), Some(lra), Some(th), Some(off)) => format!(
            "loudnorm=I=-16:TP=-1.5:LRA=11:measured_I={i}:measured_TP={tp}:measured_LRA={lra}:measured_thresh={th}:offset={off}:linear=true"
        ),
        _ => "loudnorm=I=-16:TP=-1.5:LRA=11".into(),
    };
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-framerate", "30", "-i"])
        .arg(frames.join("%05d.png"))
        .arg("-i")
        .arg(wav)
        .args([
            "-af",
            &format!("{COMP},{fade},{norm},aresample=48000"),
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "20",
            "-maxrate",
            "2000k",
            "-bufsize",
            "4000k",
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

// ----------------------------------------------------------------- tests

fn check_tour(tour: &Tour) {
    // One pointer at a time; touches may overlap.
    let mut pointer: Vec<(f64, f64)> = tour
        .cues
        .iter()
        .filter(|c| matches!(c.g, Gesture::Tap(_) | Gesture::Hold(..) | Gesture::Drag(..) | Gesture::Knob(..)))
        .map(|c| (c.t, c.t + c.g.secs()))
        .collect();
    pointer.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in pointer.windows(2) {
        assert!(w[0].1 + 1.0 / FPS < w[1].0, "pointer gestures overlap at {:.2}s", w[1].0);
    }
    let mut caps: Vec<(f64, f64)> = tour.captions.iter().map(|c| (c.t0, c.t1)).collect();
    caps.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in caps.windows(2) {
        assert!(w[0].1 + 0.7 <= w[1].0 + 1e-6, "captions overlap / crowd at {:.2}s", w[1].0);
    }
    assert!(tour.captions.iter().all(|c| c.text == c.text.to_lowercase()), "captions are lowercase");
    assert!(tour.cues.iter().all(|c| c.t + c.g.secs() < tour.end_card));
    assert!((60.0..=90.0).contains(&tour.len));
}

#[test]
fn the_tour_is_well_formed() {
    check_tour(&tour());
}

#[test]
fn the_second_tour_is_well_formed() {
    check_tour(&tour_film2());
}

#[test]
fn the_recorder_renders_frames_and_audio_in_lockstep() {
    // A short, small run: frames and samples stay locked, the first touch
    // plays grains that are heard, and the views are reachable.
    let tour = tour();
    let rec = record(&tour, &RecordOpts { size: (960.0, 600.0), ppp: 1.0, frames_dir: None, max_secs: Some(4.5) });
    assert_eq!(rec.frames, 135);
    assert_eq!(rec.audio.len(), rec.frames * (rec.sample_rate as usize / FPS as usize) * 2, "one frame of samples per frame");
    assert_eq!(rec.px, (960, 600));
    let rms = |a: &[f32]| (a.iter().map(|x| x * x).sum::<f32>() / a.len().max(1) as f32).sqrt();
    let spf2 = rec.sample_rate as usize / FPS as usize * 2;
    let before = rms(&rec.audio[..spf2 * 8]);
    let after = rms(&rec.audio[spf2 * 30..]);
    assert!(after > 1e-3 && after > before * 4.0, "the first touch is heard ({before} → {after})");
}

#[test]
#[ignore = "renders the full film (minutes); see the module docs"]
fn showcase_film() {
    let version = std::env::var("CHROMAGRAIN_SHOWCASE").unwrap_or_else(|_| "v001".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../outputs/261112_ux-showcase").join(&version);
    let out = root.join(format!("261112_chromagrain_ux-showcase_{version}.mp4"));
    assert!(!out.exists(), "{} exists — versions are never overwritten", out.display());
    let frames = root.join("work/frames");
    let _ = std::fs::remove_dir_all(&frames);
    let tour = tour();
    let t0 = std::time::Instant::now();
    let rec = record(&tour, &RecordOpts { size: (1600.0, 900.0), ppp: 1.2, frames_dir: Some(frames.clone()), max_secs: None });
    eprintln!("showcase: {} frames ({}×{}) in {:.0}s", rec.frames, rec.px.0, rec.px.1, t0.elapsed().as_secs_f64());
    let wav = root.join(format!("work/261112_chromagrain_ux-showcase_{version}.wav"));
    write_wav(&wav, &rec.audio, rec.sample_rate);
    mux(&frames, &wav, &out, tour.end_card + 0.2, tour.len).expect("mux");
    eprintln!("showcase: wrote {}", out.display());
    // The PNG frames are several GB per version: gone once muxed, unless
    // CHROMAGRAIN_SHOWCASE_KEEP_FRAMES=1.
    if std::env::var("CHROMAGRAIN_SHOWCASE_KEEP_FRAMES").is_ok_and(|v| v == "1") {
        eprintln!("showcase: kept frames in {}", frames.display());
    } else {
        std::fs::remove_dir_all(&frames).expect("remove frames");
    }
}

#[test]
#[ignore = "renders the second UX showcase film (dig, chop, tile, adsr); see module docs"]
fn showcase_film2() {
    let version = std::env::var("CHROMAGRAIN_SHOWCASE").unwrap_or_else(|_| "v001".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../outputs/261112_ux-showcase/film2").join(&version);
    let out = root.join(format!("261112_chromagrain_ux-showcase-film2_{version}.mp4"));
    assert!(!out.exists(), "{} exists — versions are never overwritten", out.display());
    let frames = root.join("work/frames");
    let _ = std::fs::remove_dir_all(&frames);
    let tour = tour_film2();
    let t0 = std::time::Instant::now();
    let rec = record_film2(&tour, &RecordOpts { size: (1600.0, 900.0), ppp: 1.2, frames_dir: Some(frames.clone()), max_secs: None });
    eprintln!("showcase film2: {} frames ({}×{}) in {:.0}s", rec.frames, rec.px.0, rec.px.1, t0.elapsed().as_secs_f64());
    let wav = root.join(format!("work/261112_chromagrain_ux-showcase-film2_{version}.wav"));
    write_wav(&wav, &rec.audio, rec.sample_rate);
    mux(&frames, &wav, &out, tour.end_card + 0.2, tour.len).expect("mux");
    eprintln!("showcase film2: wrote {}", out.display());
    if std::env::var("CHROMAGRAIN_SHOWCASE_KEEP_FRAMES").is_ok_and(|v| v == "1") {
        eprintln!("showcase film2: kept frames in {}", frames.display());
    } else {
        std::fs::remove_dir_all(&frames).expect("remove frames");
    }
}
