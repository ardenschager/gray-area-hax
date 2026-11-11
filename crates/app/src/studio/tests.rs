//! Headless studio tests (egui_kittest + wgpu): snapshots of every view at
//! the design's three sizes, and touch interactions. Never opens a window.
//!
//! `CHROMAGRAIN_UI_SHOTS=/dir cargo test -p chromagrain studio` writes the
//! snapshots as PNGs; without it they render (catching panics / GPU
//! errors) but nothing is written.

use super::*;
use crate::{AppOpts, PADS};
use chromagrain_core::timeline::Source;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::sync::{Arc, OnceLock};

const DESIGN_CLIPS: [(&str, &str); 7] = [
    ("oregon", "horses"),
    ("betty", "beanstalk"),
    ("aurora", "aurora"),
    ("bees", "comb"),
    ("nyc", "concourse"),
    ("hummingbird", "hummingbird"),
    ("map", "rivers"),
];

/// The design's 7 archival clips (decoded with ffmpeg) when available,
/// else procedural stand-ins, so the tests run anywhere.
fn clips() -> &'static Vec<Source> {
    static CLIPS: OnceLock<Vec<Source>> = OnceLock::new();
    CLIPS.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/v001/media");
        let decoded: Option<Vec<Source>> = if chromagrain_core::media::ffmpeg_available() && dir.exists() {
            DESIGN_CLIPS
                .iter()
                .map(|(file, name)| {
                    let mut s = chromagrain_core::media::load_source(&dir.join(format!("{file}.mp4")), 48000).ok()?;
                    s.name = (*name).into();
                    Some(s)
                })
                .collect()
        } else {
            None
        };
        decoded.unwrap_or_else(|| {
            DESIGN_CLIPS
                .iter()
                .enumerate()
                .map(|(i, (_, name))| Source {
                    name: (*name).into(),
                    audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(110.0 * (1.0 + i as f32 * 0.25), 3.0, 48000))),
                    video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(160, 90, 12.0, 3.0))),
                    base_hz: 110.0,
                })
                .collect()
        })
    })
}

fn harness(size: (f32, f32), view: View, setup: impl FnOnce(&mut App) + 'static) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = App::new(AppOpts { demo: false, classic: false, headless: true });
            app.studio_install_theme(&cc.egui_ctx);
            if !crate::gpu::install(cc) {
                app.gpu_failed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            {
                let mut p = app.project.lock().unwrap();
                p.bpm = 112.0;
                for s in clips() {
                    p.add_source(s.clone());
                }
            }
            app.mark_now();
            app.studio_sync_columns();
            app.seq_ensure();
            app.studio_set_view(view);
            setup(&mut app);
            app
        })
}

/// The scripted state the design shots show.
fn showcase(app: &mut App) {
    let looks = |name: &str| {
        let i = chromagrain_core::instrument::LOOKS.iter().position(|l| *l == Some(name)).unwrap_or(0);
        (i as f32 + 0.5) / chromagrain_core::instrument::LOOKS.len() as f32
    };
    app.studio.emitters[1].macros[3] = looks("tint");
    app.studio.emitters[3].macros[3] = looks("crush");
    app.studio.emitters[0].held = true;
    // Sequence: the design's lanes.
    let steps: [(&[(usize, i32)], usize); 7] = [
        (&[(0, 0), (4, 0), (8, 0), (10, 0), (12, 0)], 16),
        (&[(2, 0), (5, 2), (9, 4), (11, -1)], 12),
        (&[(2, 0), (6, 0), (10, 0), (14, 0), (15, 3)], 16),
        (&[(0, 4), (3, 2), (5, 7)], 7),
        (&[(0, 0), (8, -2)], 16),
        (&[(1, 0), (4, 1), (6, 11), (9, 2)], 10),
        (&[(7, 0), (15, -2)], 16),
    ];
    let key = app.studio.key;
    for (lane, (hits, len)) in steps.iter().enumerate() {
        if *len != STEPS {
            app.set_lane_len(lane, *len);
        }
        for (s, deg) in hits.iter() {
            app.seq_toggle(0, lane, *s);
            let semis = super::sequence::degree_to_semis(&key, *deg);
            app.seq_edit_step(0, lane, *s, |x| x.pitch = semis);
        }
    }
    for s in 1..4 {
        app.seq_scene_tap(s);
    }
    app.studio.seq.queued = None;
    app.studio.seq.scene = 0;
    app.studio.seq.chain = vec![(0, 2), (1, 2), (2, 4), (3, 2), (2, 2)];
    app.studio.rev += 1;
    app.playhead_beats = 1.5;
}

fn shot(h: &mut Harness<'static, App>, name: &str) {
    let img = h.render().expect("render");
    if let Ok(dir) = std::env::var("CHROMAGRAIN_UI_SHOTS") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    }
}

fn push(h: &mut Harness<'static, App>, e: egui::Event) {
    h.input_mut().events.push(e);
}

const SIZES: [(f32, f32); 3] = [(1600.0, 1000.0), (1180.0, 820.0), (960.0, 600.0)];

#[test]
fn snapshots_every_view_at_every_size() {
    let version = std::env::var("CHROMAGRAIN_UI_VERSION").unwrap_or_else(|_| "v001".into());
    for size in SIZES {
        for (view, vname) in [(View::Play, "play"), (View::Sequence, "sequence"), (View::Perform, "perform")] {
            let mut h = harness(size, view, move |app| {
                showcase(app);
                if view == View::Perform {
                    for pad in [0usize, 4, 9] {
                        app.toggle_pad(pad);
                    }
                    app.playback.stop();
                    app.playhead_beats = 1.5;
                }
            });
            h.run_steps(8);
            shot(&mut h, &format!("261111_ui_{vname}_{}x{}_{version}", size.0, size.1));
        }
    }
    // The drawer and the step sheet, at the big size.
    let mut h = harness(SIZES[0], View::Play, |app| {
        showcase(app);
        app.studio.deep = Some((DeepTarget::Emitter(0), DeepTab::Links));
    });
    h.run_steps(20);
    shot(&mut h, &format!("261111_ui_deep-emitter_1600x1000_{version}"));
    let mut h = harness(SIZES[0], View::Sequence, |app| {
        showcase(app);
        app.studio.seq.hold = Some((1, 5));
    });
    h.run_steps(8);
    shot(&mut h, &format!("261111_ui_sequence-hold_1600x1000_{version}"));
}

#[test]
fn tapping_a_step_toggles_it() {
    let mut h = harness(SIZES[0], View::Sequence, |_| {});
    h.run_steps(3);
    assert!(!h.state().seq_step(0, 0, 2).unwrap().on);
    h.get_by_label("lane 1 step 3").click();
    h.run_steps(2);
    let st = h.state().seq_step(0, 0, 2).unwrap();
    assert!(st.on, "tap turns the step on");
    assert!(st.slice >= 0.0, "and picks the slice under it");
    h.get_by_label("lane 1 step 3").click();
    h.run_steps(2);
    assert!(!h.state().seq_step(0, 0, 2).unwrap().on, "tap again turns it off");
}

#[test]
fn holding_a_step_opens_its_sheet_and_dragging_sets_pitch() {
    let mut h = harness(SIZES[0], View::Sequence, |app| app.seq_toggle(0, 2, 4));
    h.run_steps(3);
    let rect = h.get_by_label("lane 3 step 5").rect();
    push(&mut h, egui::Event::PointerMoved(rect.center()));
    push(&mut h, egui::Event::PointerButton { pos: rect.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    for _ in 0..40 {
        h.step();
    }
    assert_eq!(h.state().studio.seq.hold, Some((2, 4)), "hold opens the step sheet");
    push(&mut h, egui::Event::PointerButton { pos: rect.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert!(h.state().seq_step(0, 2, 4).unwrap().on, "a hold is not a toggle");
    // Drag up on an active step: pitch goes up by key degrees.
    h.state_mut().studio.seq.hold = None;
    h.run_steps(1);
    let c = rect.center();
    push(&mut h, egui::Event::PointerMoved(c));
    push(&mut h, egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=6 {
        push(&mut h, egui::Event::PointerMoved(c - egui::vec2(0.0, k as f32 * 8.0)));
        h.step();
    }
    push(&mut h, egui::Event::PointerButton { pos: c - egui::vec2(0.0, 48.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    let st = h.state().seq_step(0, 2, 4).unwrap();
    assert!(st.pitch > 0.0, "dragging up raised the pitch ({})", st.pitch);
    let key = h.state().studio.key;
    let d = super::sequence::semis_to_degree(&key, st.pitch);
    assert!((super::sequence::degree_to_semis(&key, d) - st.pitch).abs() < 1e-4, "pitch stays on a key degree");
}

#[test]
fn tapping_the_grid_moves_the_selected_emitter_and_sprays() {
    let mut h = harness(SIZES[0], View::Play, |app| app.studio.sel_emitter = 2);
    h.run_steps(3);
    let before = (h.state().studio.emitters[2].col, h.state().studio.emitters[2].row);
    let grid = h.get_by_label("play grid").rect();
    let target = egui::pos2(grid.left() + grid.width() * (3.5 / 8.0), grid.top() + grid.height() * (6.5 / 8.0));
    push(&mut h, egui::Event::PointerMoved(target));
    push(&mut h, egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(6);
    {
        let e = &h.state().studio.emitters[2];
        assert_eq!(e.col, 3, "moved from {before:?}");
        assert!((e.row - 6.5).abs() < 0.3, "row {}", e.row);
        assert!(e.held, "spraying while held");
    }
    assert!(h.state().live_events.iter().any(|(_, bus, _)| *bus == 3), "emitter 3's grains are on bus 3");
    push(&mut h, egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert!(!h.state().studio.emitters[2].held, "release stops the spray");
    assert_eq!(h.state().studio.sel_emitter, 2);
}

#[test]
fn grabbing_an_emitter_ring_selects_it() {
    let mut h = harness(SIZES[0], View::Play, |app| app.studio.sel_emitter = 0);
    h.run_steps(3);
    let grid = h.get_by_label("play grid").rect();
    let (cw, ch) = ((grid.width() - 28.0) / 8.0, (grid.height() - 28.0) / 8.0);
    let e = &h.state().studio.emitters[3];
    let ring = egui::pos2(grid.left() + e.col as f32 * (cw + 4.0) + cw / 2.0, grid.top() + e.row * (ch + 4.0));
    h.get_by_label("play grid"); // exists
    push(&mut h, egui::Event::PointerMoved(ring));
    push(&mut h, egui::Event::PointerButton { pos: ring, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(2);
    push(&mut h, egui::Event::PointerButton { pos: ring, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert_eq!(h.state().studio.sel_emitter, 3);
}

#[test]
fn tapping_a_pad_launches_it_and_views_switch_by_tab_and_key() {
    let mut h = harness(SIZES[0], View::Play, |_| {});
    h.run_steps(3);
    h.get_by_label("Perform").click();
    h.run_steps(3);
    assert_eq!(h.state().studio.view, View::Perform);
    assert_eq!(h.state().mode, AppMode::Perform);
    let has_clip = h.state().project.lock().unwrap().tracks.first().is_some_and(|t| !t.clips.is_empty());
    assert!(has_clip, "pads are seeded from the grid's clips");
    h.get_by_label("pad 1").click();
    h.run_steps(2);
    let on = h.state().project.lock().unwrap().tracks[0].muted;
    assert!(!on, "pad 1 is playing");
    assert!(h.state().playback.is_playing(), "launching a pad starts the transport");
    assert_eq!(h.state().project.lock().unwrap().tracks.len(), PADS);
    // Keyboard: 2 is Sequence (only outside Perform, where digits are pads).
    h.get_by_label("Play").click();
    h.run_steps(2);
    h.key_press(egui::Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().studio.view, View::Sequence);
    assert_eq!(h.state().mode, AppMode::Arrange);
}

#[test]
fn knobs_drive_the_shared_macro_map_and_buses() {
    let mut h = harness(SIZES[0], View::Play, |app| {
        app.studio.emitters[0].macros[3] = 2.5 / chromagrain_core::instrument::LOOKS.len() as f32; // crush
        app.studio.emitters[0].macros[2] = 0.8;
    });
    h.run_steps(3);
    let buses = h.state().studio_buses();
    assert_eq!(buses.len(), EMITTERS);
    let names: Vec<&str> = buses[0].effects.iter().map(|f| f.kind.name()).collect();
    assert_eq!(names, ["delay", "reverb", "crush"], "smear echo + reverb, then the look");
    assert!(buses[1].effects.iter().all(|f| f.kind.name() != "crush"));
    // Drag the GRAIN knob up: texture follows.
    let k = h.get_by_label("Grain").rect();
    let before = h.state().studio.emitters[0].macros[0];
    push(&mut h, egui::Event::PointerMoved(k.center()));
    push(&mut h, egui::Event::PointerButton { pos: k.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for i in 1..=10 {
        push(&mut h, egui::Event::PointerMoved(k.center() - egui::vec2(0.0, i as f32 * 10.0)));
        h.step();
    }
    push(&mut h, egui::Event::PointerButton { pos: k.center() - egui::vec2(0.0, 100.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    assert!(h.state().studio.emitters[0].macros[0] > before + 0.2, "drag up turns GRAIN up");
}

#[test]
fn rec_bounces_played_grains_into_a_take() {
    let mut h = harness(SIZES[2], View::Play, |app| {
        app.studio_toggle_rec();
        app.studio.emitters[0].held = true;
    });
    h.run_steps(10);
    std::thread::sleep(std::time::Duration::from_millis(150));
    h.run_steps(4);
    h.state_mut().studio.emitters[0].held = false;
    let n0 = h.state().project.lock().unwrap().sources.len();
    h.state_mut().studio_toggle_rec();
    for _ in 0..200 {
        h.step();
        if h.state().project.lock().unwrap().sources.len() > n0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let p = h.state().project.lock().unwrap();
    assert_eq!(p.sources.len(), n0 + 1, "the take is in the library");
    let takes = p.tracks.iter().find(|t| t.name == "takes").expect("a takes track");
    assert_eq!(takes.clips.len(), 1);
    assert!(p.sources[n0].video.is_some() && p.sources[n0].audio.is_some());
}

#[test]
fn the_song_chain_becomes_the_arrangement() {
    let mut h = harness(SIZES[0], View::Sequence, showcase);
    h.run_steps(3);
    let app = h.state();
    let p = app.project.lock().unwrap();
    let seq = p.tracks.iter().find(|t| t.name == "sequence").expect("sequence track");
    let lens: Vec<f64> = seq.clips.iter().map(|c| c.length_beats).collect();
    assert_eq!(lens, [8.0, 8.0, 16.0, 8.0, 8.0], "1×2 2×2 3×4 4×2 3×2 bars");
    assert!(app.loop_on && (app.loop_end - 48.0).abs() < 1e-9, "the transport loops the song");
    assert!(seq.clips.windows(2).all(|w| (w[0].end_beat() - w[1].start_beat).abs() < 1e-9));
}
