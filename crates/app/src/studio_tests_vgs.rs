//! Phase 2 VGS Studio UX & wgpu Stage tests + headless snapshot suite:
//! - Add Sheet: YouTube Search (`<500 views` chip + clickable rows + Paste Link),
//!   `Dig 4 → Kit`, and Google Drive / Browse (`All | Drive | Local`).
//! - Stage Modes (`stack | split | grid | bleed`) & Aspect (`16:9 · 1:1 · 9:16`),
//!   kinetic spoken-hook Caption bar, smart `(cx, cy)` crop focus.
//! - Clip Sheet: acoustic slice badges, `Kit → pads`, `Loops → pads`, and
//!   `SCULPT & TAPE` stamps/dials (`Raw..Disintegrate`, `Chord`, `Choke`, `Free Loop`, `Caption`).
//! - Sequence Step Hold-Sheet (`Ratchet ×1..×8` + pitch slope, `Offset`, `Chord`)
//!   and shared VJ Set / Cue Arranger Bar (`intro`, `drop`, `dialogue`, `wash`, repeats, `∞ hold`).
//! - Perform: 6 VJ Punch-In FX pills (`Stutter`, `Tape Stop`, `Comb`, `Dub`, `Shimmer`, `Wear`)
//!   and `60 fps` export toggle in `Sheet::Project`.

use crate::studio_tests::{harness, push, shot, showcase, SIZES};
use crate::App;
use chromagrain_core::chop;
use chromagrain_ui::studio::{ClipTarget, Sheet};
use chromagrain_ui::{
    AddRoute, AddTarget, ChordChoice, Host, SculptStamp, StageAspect, StageMode, View, YoutubeHit,
};
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn version() -> String {
    std::env::var("CHROMAGRAIN_UI_VERSION").unwrap_or_else(|_| "v001".into())
}

fn vgs_shot_name(what: &str, size: (f32, f32)) -> String {
    format!("261113_chromagrain_vgs_{what}_{}x{}_{}", size.0, size.1, version())
}

fn long_press(h: &mut Harness<'static, App>, label: &str) {
    let c = h.get_by_label(label).rect().center();
    push(h, egui::Event::PointerMoved(c));
    push(
        h,
        egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    );
    for _ in 0..40 {
        h.step();
    }
    push(
        h,
        egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    );
    h.run_steps(2);
}

fn drag(h: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2) {
    push(h, egui::Event::PointerMoved(from));
    push(
        h,
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    );
    h.step();
    for k in 1..=8 {
        push(h, egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
        h.step();
    }
    push(
        h,
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    );
    h.run_steps(2);
}

fn mock_youtube_catalog() -> Vec<YoutubeHit> {
    vec![
        YoutubeHit {
            title: "1964 Psychiatrist Clinic Educational Film Reel 2".into(),
            url: "https://www.youtube.com/watch?v=vgs_psych64".into(),
            views: Some(184),
            duration_secs: Some(142.0),
            uploader: "midcentury_archive".into(),
        },
        YoutubeHit {
            title: "cello practice — Bach sarabande bow study (room mic)".into(),
            url: "https://www.youtube.com/watch?v=vgs_cello09".into(),
            views: Some(73),
            duration_secs: Some(98.0),
            uploader: "clara_v_2011".into(),
        },
        YoutubeHit {
            title: "Panasonic microcassette answering machine tape 1993".into(),
            url: "https://www.youtube.com/watch?v=vgs_tape93".into(),
            views: Some(319),
            duration_secs: Some(64.0),
            uploader: "attic_tapes".into(),
        },
        YoutubeHit {
            title: "MVI_0419 Staten Island ferry foghorn dusk".into(),
            url: "https://www.youtube.com/watch?v=vgs_ferry08".into(),
            views: Some(42),
            duration_secs: Some(47.0),
            uploader: "user8831".into(),
        },
        YoutubeHit {
            title: "1960s kitchen appliance commercial compilation".into(),
            url: "https://www.youtube.com/watch?v=vgs_comm60s".into(),
            views: Some(18_420),
            duration_secs: Some(215.0),
            uploader: "retro_tv_vault".into(),
        },
    ]
}

#[test]
fn vgs_add_youtube_search_low_views_and_dig_to_kit() {
    let mut h = harness(SIZES[0], View::Play, |app| {
        app.adds.yt_mock = Some(mock_youtube_catalog());
        app.studio.add.tab = AddRoute::Link;
        app.studio.sheet = Some(Sheet::Add(AddTarget::Column(7)));
    });
    h.run_steps(3);

    // Type query in YouTube search field and click Search.
    h.get_by_label("youtube search").click();
    h.run_steps(1);
    push(&mut h, egui::Event::Text("commercial cello tape".into()));
    h.run_steps(2);
    h.get_by_label("search youtube").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.yt_hits.len(), 5, "all 5 mock hits match before <500 views filter");

    // Toggle `<500 views` chip -> filters out the 18.4k views item!
    h.get_by_label("low views").click();
    h.run_steps(2);
    assert!(h.state().studio.add.yt_low_views);
    assert_eq!(h.state().adds.yt_hits.len(), 4, "<500 views chip keeps only unseen uploads");

    // Click result 1 -> queues a clip window fetch job!
    let before_jobs = h.state().adds.jobs.len();
    h.get_by_label("youtube result 1").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.jobs.len(), before_jobs + 1, "clicking a YouTube result row queues a fetch");

    // Switch to Dig tab and click `Dig 4 → Kit`.
    h.get_by_label("add Dig").click();
    h.run_steps(2);
    let before_dig = h.state().adds.jobs.len();
    h.get_by_label("dig 4 to kit").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.jobs.len(), before_dig + 4, "Dig 4 -> Kit queues 4 unseen digs");
    assert_eq!(h.state().adds.dig_kit_remaining, 4);
}

#[test]
fn vgs_stage_modes_aspect_ratios_and_60fps_export_toggle() {
    let mut h = harness(SIZES[0], View::Play, |app| {
        showcase(app);
    });
    h.run_steps(3);

    // 4-way stage mode pill: `stack | split | grid | bleed`.
    assert_eq!(h.state().stage_mode(), Some(StageMode::Stack));
    h.get_by_label("layout split").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_mode(), Some(StageMode::Split));

    h.get_by_label("layout tile").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_mode(), Some(StageMode::Grid));

    h.get_by_label("layout bleed").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_mode(), Some(StageMode::Bleed));

    // Aspect ratio pill: `16:9 · 1:1 · 9:16`.
    assert_eq!(h.state().stage_aspect(), Some(StageAspect::Wide16x9));
    h.get_by_label("aspect 9:16").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_aspect(), Some(StageAspect::Vertical9x16));
    {
        let p = h.state().project.lock().unwrap();
        assert_eq!((p.width, p.height), (1080, 1920));
    }

    h.get_by_label("aspect 1:1").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_aspect(), Some(StageAspect::Square1x1));
    {
        let p = h.state().project.lock().unwrap();
        assert_eq!((p.width, p.height), (1080, 1080));
    }

    h.get_by_label("aspect 16:9").click();
    h.run_steps(2);
    assert_eq!(h.state().stage_aspect(), Some(StageAspect::Wide16x9));

    // MODELL 5 check: 4 voices on 16:9 widescreen split into 4x1 portrait columns,
    // while on 9:16 vertical they form a 2x2 quad, and tight spray + Cut emitter uses full_frame(0.0)!
    let (r0_wide, _) = chromagrain_ui::gpu::split_panel_rect(0, 4, 16.0 / 9.0);
    let (r3_wide, _) = chromagrain_ui::gpu::split_panel_rect(3, 4, 16.0 / 9.0);
    assert!(r0_wide[3] > 0.99 && r3_wide[0] > 0.70, "4 voices on 16:9 form 4x1 portrait columns");
    let (r3_tall, _) = chromagrain_ui::gpu::split_panel_rect(3, 4, 9.0 / 16.0);
    assert!(r3_tall[1] > 0.45, "4 voices on 9:16 form a 2x2 quad");
    {
        let app = h.state_mut();
        app.studio.emitters[0].adsr.picture = chromagrain_core::envelope::PictureCut::Cut;
        app.studio.emitters[0].cloud.spray = 0.04;
        let b0 = &app.studio.buses()[0];
        assert!(b0.style.min_size >= 0.99 && b0.style.feather == 0.0, "MODELL 5 tight Cut micro-bursts render 100% full-cell");
    }

    // Open `Project` sheet and toggle `60 fps` export!
    h.get_by_label("Project").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Project));
    h.get_by_label("fps 60").click();
    h.run_steps(2);
    assert!((h.state().project.lock().unwrap().fps - 60.0).abs() < 1e-3);
}

#[test]
fn vgs_clip_sheet_sculpt_stamps_kit_harvester_loops_caption_and_focus() {
    let mut h = harness(SIZES[0], View::Play, |app| {
        showcase(app);
        let mut p = app.project.lock().unwrap();
        let len = p.sources[0].duration();
        p.sources[0].slices = chop::equal_slices(len, 8);
    });
    h.run_steps(3);
    h.get_by_label("open clip").click();
    h.run_steps(3);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Emitter(0))));

    // 1. Click `Kick`, `Chord`, `Disintegrate` stamps -> verify continuous dials populate!
    h.get_by_label("sculpt Kick").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.emitters[0].sculpt.stamp, SculptStamp::Kick);
    assert!(h.state().studio.emitters[0].sculpt.sub > 0.6);
    assert!(h.state().studio.emitters[0].sculpt.pitch_drop_st > 10.0);

    h.get_by_label("sculpt Chord").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.emitters[0].sculpt.chord, ChordChoice::Min9);
    assert!(h.state().studio.emitters[0].sculpt.comb > 0.4);

    h.get_by_label("sculpt Disintegrate").click();
    h.run_steps(2);
    assert!(h.state().studio.emitters[0].sculpt.wear > 0.6);
    assert!(h.state().studio.emitters[0].sculpt.free_loop_secs.is_some());

    // 2. Type spoken-hook caption & drag smart focus crosshair!
    h.get_by_label("slice caption").click();
    h.run_steps(1);
    push(&mut h, egui::Event::Text("that boy needs therapy".into()));
    h.run_steps(2);
    assert_eq!(h.state().studio.emitters[0].caption, "that boy needs therapy");

    let fc = h.get_by_label("focus point").rect().center();
    drag(&mut h, fc, fc + egui::vec2(36.0, -18.0));
    let new_focus = h.state().studio.source_focus.get(&0).copied().unwrap_or((0.5, 0.5));
    assert!((new_focus.0 - 0.5).abs() > 0.03, "dragging focus crosshair shifts smart crop center: {new_focus:?}");

    // 3. `Kit → pads` harvests a 16-pad collage kit across all loaded clips and switches to Perform!
    h.get_by_label("kit to pads").click();
    h.run_steps(3);
    assert_eq!(h.state().studio.view, View::Perform);
    let pads = chromagrain_ui::PerformHost::pads(h.state());
    assert_eq!(pads.iter().filter(|pd| pd.source.is_some()).count(), 16);

    // 4. Re-open Clip sheet on Pad 1 and click `Loops → pads` for unsynced tape loops!
    h.get_by_label("clip pad 1").click();
    h.run_steps(3);
    h.get_by_label("loops to pads").click();
    h.run_steps(3);
    assert_eq!(h.state().studio.view, View::Perform);
    let p = h.state().project.lock().unwrap();
    assert!(
        p.tracks[0].name.contains("~ loop"),
        "Loops -> pads deals unsynced tape loops: {}",
        p.tracks[0].name
    );
}

#[test]
fn vgs_sequence_step_ratchet_chord_set_bar_and_perform_punch_fx() {
    let mut h = harness(SIZES[0], View::Sequence, |app| {
        showcase(app);
    });
    h.run_steps(4);

    // 1. Long-press step 1 on lane 1 -> step hold-sheet opens with `ratchet`, `offset`, `step chord`.
    long_press(&mut h, "lane 1 step 1");
    assert_eq!(h.state().studio.seq.hold, Some((0, 0)));

    h.get_by_label("ratchet").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.step_ratchet_ext(0, 0, 0).0, 2);
    h.get_by_label("ratchet").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.step_ratchet_ext(0, 0, 0).0, 3);

    h.get_by_label("step chord").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.step_ratchet_ext(0, 0, 0).2, ChordChoice::Octaves);

    let off_c = h.get_by_label("offset").rect().center();
    drag(&mut h, off_c, off_c - egui::vec2(0.0, 36.0));
    let step0 = h.state().studio.seq_step(h.state(), 0, 0, 0).unwrap();
    assert!(step0.offset > 0.02, "dragging offset cell adjusts microtiming pocket: {}", step0.offset);

    // Close step hold-sheet by clicking outside, then toggle `∞ hold` on Set Arranger Bar.
    h.state_mut().studio.seq.hold = None;
    h.run_steps(2);
    h.get_by_label("hold scene").click();
    h.run_steps(2);
    assert!(h.state().studio.seq.hold_scene);

    // 2. Switch to Perform -> test 6 VJ Punch-In FX pills (`Stutter`, `Tape Stop`, `Shimmer`, `Wear`).
    h.get_by_label("Perform").click();
    h.run_steps(3);
    h.get_by_label("punch stutter").click();
    h.get_by_label("punch tape stop").click();
    h.get_by_label("punch shimmer").click();
    h.run_steps(4);
    assert!(h.state().studio.punch_fx[0], "Stutter latched");
    assert!(h.state().studio.punch_fx[1], "Tape Stop latched");
    assert!(h.state().studio.punch_fx[4], "Shimmer latched");
    assert!(h.state().studio.tape_stop_amt > 0.05, "Tape Stop ramps active");
}

#[test]
fn vgs_ui_snapshots() {
    std::env::set_var(
        crate::browser::BROWSE_ROOT_ENV,
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/v001/media"),
    );
    std::env::set_var(
        crate::browser::DRIVE_ROOT_ENV,
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/v001/media"),
    );

    // 1. Play (1600×1000 & 960×600) with 4-mode stage pill + 16:9·1:1·9:16 aspect pill
    for size in [(1600.0f32, 1000.0f32), (960.0, 600.0)] {
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            app.set_stage_mode(StageMode::Split);
            app.studio.emitters[0].held = true;
            app.studio.emitters[1].held = true;
            app.studio.active_caption = Some(("frontier psychiatrist".into(), 999.0));
        });
        h.run_steps(10);
        shot(&mut h, &vgs_shot_name("play", size));
    }

    // 2. Add Sheet — YouTube Search (`<500 views` + populated search results + Paste Link)
    {
        let size = (1600.0, 1000.0);
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            app.adds.yt_mock = Some(mock_youtube_catalog());
            app.studio.add.tab = AddRoute::Link;
            app.studio.add.yt_query = "1960s psychiatrist cello answering machine".into();
            app.studio.add.yt_low_views = true;
            app.adds.search_youtube("1960s psychiatrist cello answering machine", true);
            app.studio.sheet = Some(Sheet::Add(AddTarget::Column(7)));
        });
        h.run_steps(8);
        shot(&mut h, &vgs_shot_name("add-youtube-search", size));
    }

    // 3. Add Sheet — Dig (`Dig 4 → Kit`) & Drive (`All | Drive | Local`)
    {
        let size = (1600.0, 1000.0);
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            app.studio.add.tab = AddRoute::Dig;
            app.studio.sheet = Some(Sheet::Add(AddTarget::Column(7)));
        });
        h.run_steps(6);
        shot(&mut h, &vgs_shot_name("add-dig-kit", size));

        h.get_by_label("add Browse").click();
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 10 && h.state().browse_last.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(40));
            h.run_steps(1);
        }
        for _ in 0..16 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.run_steps(1);
        }
        shot(&mut h, &vgs_shot_name("add-drive", size));
    }

    // 4. Clip Sheet — Acoustic slice badges, `Kit → pads`, `Loops → pads`, `SCULPT & TAPE`, `Caption`, `(cx, cy)` focus
    for size in [(1600.0f32, 1000.0f32), (1180.0, 820.0), (960.0, 600.0)] {
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            {
                let mut p = app.project.lock().unwrap();
                let len = p.sources[0].duration();
                p.sources[0].slices = chop::equal_slices(len, 8);
            }
            app.studio.source_focus.insert(0, (0.38, 0.44));
            app.studio.slice_captions.insert((0, 0), "that boy needs therapy".into());
            app.studio.emitters[0].sculpt = chromagrain_ui::SculptParams::preset(SculptStamp::Chord);
            app.studio.emitters[0].caption = "that boy needs therapy".into();
            app.studio.open_clip(ClipTarget::Emitter(0));
            app.studio.chop.selected_slice = Some((0, 0));
        });
        h.run_steps(8);
        shot(&mut h, &vgs_shot_name("clip-sculpt-tape", size));
    }

    // 5. Sequence — Named VJ Set Scenes (`intro`, `drop`, `dialogue`, `wash`) + Cue Bar + Open Step Hold-Sheet (`Ratchet`, `Offset`, `Chord`)
    {
        let size = (1600.0, 1000.0);
        let mut h = harness(size, View::Sequence, |app| {
            showcase(app);
            app.studio.seq.scene_names[0] = "intro".into();
            app.studio.seq.scene_names[1] = "drop".into();
            app.studio.seq.scene_names[2] = "dialogue".into();
            app.studio.seq.scene_names[3] = "wash".into();
            app.with_studio(|st, host| {
                st.seq_scene_tap(host, 2);
                st.seq_scene_tap(host, 3);
                st.seq_scene_tap(host, 0);
                st.seq.chain = vec![(0, 2), (1, 4), (2, 2), (3, 0)];
                st.rev += 1;
                st.set_step_ratchet_ext(host, 0, 0, 4, 4, 2.0, ChordChoice::Min9);
                st.seq_edit_step(host, 0, 0, 4, |s| {
                    s.on = true;
                    s.offset = 0.08;
                });
            });
        });
        h.run_steps(6);
        long_press(&mut h, "lane 1 step 5");
        h.run_steps(3);
        shot(&mut h, &vgs_shot_name("sequence-ratchet-set", size));
    }

    // 6. Perform — `Split + Bleed` in `9:16` vertical aspect + active pads + VJ Punch-In FX + ISOL8 Vocal Solo
    {
        let size = (1600.0, 1000.0);
        let mut h = harness(size, View::Perform, |app| {
            showcase(app);
            app.build_kit_to_pads();
            app.set_stage_mode(StageMode::Split);
            app.set_stage_aspect(StageAspect::Vertical9x16);
            app.studio.seq.scene_names[0] = "intro".into();
            app.studio.seq.scene_names[1] = "drop".into();
            app.studio.seq.scene_names[2] = "wash".into();
            app.studio.punch_fx[0] = true; // Stutter
            app.studio.punch_fx[4] = true; // Shimmer
            app.studio.active_caption = Some(("purely psychosomatic".into(), 999.0));
            let mut iso = chromagrain_core::dsp::BandIsolator::default();
            iso.solo = [false, true, true, true, false];
            iso.ch_mode = chromagrain_core::dsp::IsolatorChannelMode::Mid;
            iso.monitor = chromagrain_core::dsp::IsolatorMonitor::Mid;
            app.set_isolator(iso);
            app.studio.isolator = iso;
            for pad in [0usize, 1, 5] {
                app.toggle_pad(pad);
            }
            app.playback.stop();
            app.playhead_beats = 1.25;
        });
        h.run_steps(10);
        shot(&mut h, &vgs_shot_name("perform-split-bleed-9x16", size));
    }

    // 7. Perform — `Bleed` in `16:9` widescreen (Basinski / Caretaker tape loops + Wear + Shimmer + ISOL8 LF/LMF + DIM)
    for size in [(1600.0f32, 1000.0f32), (960.0, 600.0)] {
        let mut h = harness(size, View::Perform, move |app| {
            showcase(app);
            app.build_loops_to_pads();
            app.set_stage_mode(StageMode::Bleed);
            app.set_stage_aspect(StageAspect::Wide16x9);
            app.studio.seq.hold_scene = true;
            app.studio.punch_fx[4] = true; // Shimmer
            app.studio.punch_fx[5] = true; // Wear
            let mut iso = chromagrain_core::dsp::BandIsolator::default();
            if size.0 == 1600.0 {
                iso.solo = [false, true, true, true, true];
                iso.mute = [true, false, false, false, false];
            } else {
                iso.solo = [true, true, false, false, false];
                iso.mute = [false, false, false, false, true];
                iso.dim = true;
            }
            app.set_isolator(iso);
            app.studio.isolator = iso;
            for pad in [0usize, 1, 5] {
                app.toggle_pad(pad);
            }
            app.playback.stop();
            app.playhead_beats = 1.6;
        });
        h.run_steps(10);
        let tag = if size.0 == 1600.0 {
            vgs_shot_name("perform-bleed-16x9", size)
        } else {
            vgs_shot_name("perform-isol8", size)
        };
        shot(&mut h, &tag);
    }
}

#[test]
fn vgs_verifier_fixes_and_isol8_wiring() {
    use chromagrain_core::dsp::IsolatorMonitor;
    use chromagrain_core::render::plan_project;
    use chromagrain_ui::PerformHost;

    // (a) #72 item 3: Free-loop pads in Perform stay one unbroken Clip >= 64 beats
    // so `loop_pass` and `disintegrate` advance across passes instead of resetting!
    let mut h = harness((1600.0, 1000.0), View::Perform, |app| {
        showcase(app);
        app.build_loops_to_pads();
        app.toggle_pad(0);
        app.playback.stop();
    });
    h.run_steps(4);
    {
        assert!(h.state().loop_end >= 64.0, "free-loop horizon >= 64 beats");
        let p = h.state().project.lock().unwrap();
        assert_eq!(p.tracks[0].clips.len(), 1, "single unbroken clip for free-loop pad");
        assert!(p.tracks[0].clips[0].length_beats >= 64.0);
        let plan = plan_project(&p);
        let max_pass = plan
            .iter()
            .filter(|cp| cp.track == 0)
            .flat_map(|cp| cp.parts.iter().flat_map(|(_, evs)| evs.iter().map(|e| e.loop_pass)))
            .max()
            .unwrap_or(0);
        assert!(max_pass >= 4, "expected free-loop pass progression >= 4, got {max_pass}");
    }

    // (b) #72 item 4: Pad sculpt in Arrange mode routes to `other_tracks` (never mutating
    // the sequence track), Emitter sculpt applies pitch_semitones, and Kit -> pads syncs captions.
    let mut h2 = harness((1600.0, 1000.0), View::Play, |app| {
        showcase(app);
        {
            let mut p = app.project.lock().unwrap();
            let len = p.sources[0].duration();
            let mut sl = chop::equal_slices(len, 4);
            for s in &mut sl {
                s.caption = "psychosomatic hook".into();
            }
            p.sources[0].slices = sl;
            for src in p.sources.iter_mut().skip(1) {
                let l = src.duration();
                let mut v = chop::equal_slices(l, 4);
                for s in &mut v {
                    s.caption = "psychosomatic hook".into();
                }
                src.slices = v;
            }
        }
        app.build_kit_to_pads();
    });
    h2.run_steps(3);
    assert!(
        h2.state().studio.pad_captions.iter().any(|c| c == "psychosomatic hook"),
        "build_kit_to_pads syncs Slice::caption into studio.pad_captions"
    );
    {
        let app = h2.state_mut();
        let seq_sculpt_before = app.project.lock().unwrap().tracks[0].sculpt;
        let kick = chromagrain_ui::SculptParams::preset(SculptStamp::Kick);
        let mut core_kick = kick.to_core();
        core_kick.pitch_semitones = -12.0;
        app.set_pad_sculpt(0, core_kick, 1);
        assert_eq!(
            app.project.lock().unwrap().tracks[0].sculpt.pitch_semitones,
            seq_sculpt_before.pitch_semitones,
            "sequence track sculpt untouched while in Play/Arrange"
        );
        assert!(
            (app.other_tracks[0].sculpt.pitch_semitones - (-12.0)).abs() < 1e-4,
            "pad 0 sculpt persisted on other_tracks[0]"
        );
        // Emitter sculpt applies `pitch_semitones`:
        app.studio.emitters[0].sculpt = kick;
        app.studio.emitters[0].held = true;
    }
    h2.run_steps(4);
    assert!(
        h2.state().live_events.iter().any(|(_, _, ev)| ev.pitch_ratio < 0.65),
        "emitter Kick sculpt lowers grain pitch_ratio (< 0.65)"
    );

    // (c) #73: Clicking ISOL8 hardware switches in Perform updates Project::isolator live.
    h.get_by_label("isol8 solo lmf").click();
    h.get_by_label("isol8 solo mf").click();
    h.get_by_label("isol8 solo hmf").click();
    h.get_by_label("isol8 mon m").click();
    h.get_by_label("isol8 dim").click();
    h.run_steps(3);
    {
        let iso = h.state().project.lock().unwrap().isolator;
        assert_eq!(iso.solo, [false, true, true, true, false]);
        assert_eq!(iso.monitor, IsolatorMonitor::Mid);
        assert!(iso.dim);
    }
}
