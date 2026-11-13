//! v003 studio tests (headless, egui_kittest + wgpu): the Add sheet
//! (Files / Browse / Link / Dig, placeholders, source cards), the clip
//! sheet (chop, slices → pads, ADSR), the emitter envelope and the
//! `stack | tile` switch. Never opens a window.
//!
//! `CHROMAGRAIN_UI_SHOTS=/dir CHROMAGRAIN_UI_VERSION=v00N cargo test -p chromagrain v003`
//! writes the snapshots.

use crate::studio_tests::{clips, harness, push, shot, showcase, SIZES};
use crate::App;
use chromagrain_core::chop::{self, Slice};
use chromagrain_core::envelope::PictureCut;
use chromagrain_core::tile::Layout;
use chromagrain_core::timeline::Provenance;
use chromagrain_ui::studio::{ClipTarget, Sheet};
use chromagrain_ui::{AddRoute, AddTarget, DeepTab, DeepTarget, Pending, View};
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn version() -> String {
    std::env::var("CHROMAGRAIN_UI_VERSION").unwrap_or_else(|_| "v001".into())
}

fn name(what: &str, size: (f32, f32)) -> String {
    format!("261112_ui_{what}_{}x{}_{}", size.0, size.1, version())
}

/// Press, hold (the 0.45 s long-press), release.
fn long_press(h: &mut Harness<'static, App>, label: &str) {
    let c = h.get_by_label(label).rect().center();
    push(h, egui::Event::PointerMoved(c));
    push(h, egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    for _ in 0..40 {
        h.step();
    }
    push(h, egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

fn drag(h: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2) {
    push(h, egui::Event::PointerMoved(from));
    push(h, egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=8 {
        push(h, egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
        h.step();
    }
    push(h, egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

/// A dug find with provenance, as Dig delivers it.
fn dug(app: &mut App) -> usize {
    let mut s = clips()[2].clone();
    s.name = "IMG_4821".into();
    s.provenance = Some(Provenance {
        title: "IMG_4821".into(),
        uploader: "ruthless_owl".into(),
        upload_date: "20140302".into(),
        views: Some(212),
        url: "https://www.youtube.com/watch?v=xxxxxxxxxxx".into(),
        license: "Creative Commons Attribution license (reuse allowed)".into(),
        query: "IMG_4821".into(),
        window: (31.0, 42.0),
    });
    let id = app.project.lock().unwrap().add_source(s);
    app.mark_now();
    id
}

fn fake_pending(app: &mut App) {
    app.adds.jobs.push(Pending { id: 901, target: AddTarget::Column(7), label: "digging “MVI_0032”".into(), progress: None, failed: None });
    app.adds.jobs.push(Pending { id: 902, target: AddTarget::Any, label: "garden hose, 2009".into(), progress: Some(0.42), failed: None });
}

fn sliced(app: &mut App, src: usize, n: usize) {
    let mut p = app.project.lock().unwrap();
    let len = p.sources[src].duration();
    p.sources[src].slices = chop::equal_slices(len, n);
}

#[test]
fn v003_snapshots() {
    for size in SIZES {
        // Add sheet, every tab (Dig with things arriving and a dug card).
        // Browse shows the design's media folder (the fixture library).
        std::env::set_var(crate::browser::BROWSE_ROOT_ENV, std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/v001/media"));
        for route in [AddRoute::Files, AddRoute::Browse, AddRoute::Link, AddRoute::Dig] {
            let mut h = harness(size, View::Play, move |app| {
                showcase(app);
                if route == AddRoute::Dig {
                    let s = dug(app);
                    fake_pending(app);
                    app.studio.add.recent = vec![s];
                }
                if route == AddRoute::Link {
                    app.studio.add.link = "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into();
                }
                app.studio.add.tab = route;
                app.studio.sheet = Some(Sheet::Add(AddTarget::Column(7)));
            });
            h.run_steps(10);
            if route == AddRoute::Browse {
                // Scan + ffmpeg thumbnails arrive in the background.
                let t = std::time::Instant::now();
                while t.elapsed().as_secs() < 15 && h.state().browse_last.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    h.run_steps(1);
                }
                for _ in 0..40 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    h.run_steps(1);
                }
            }
            shot(&mut h, &name(&format!("add-{}", route.label().to_lowercase()), size));
        }
        // The grid with an add still arriving (placeholder column + tile).
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            fake_pending(app);
        });
        h.run_steps(8);
        shot(&mut h, &name("play-arriving", size));
        // Clip sheet: a column's clip chopped in 8, slice 3 auditioned.
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            sliced(app, 0, 8);
            app.studio.open_clip(ClipTarget::Column(0));
        });
        h.run_steps(6);
        shot(&mut h, &name("chop", size));
        // Pad sheet: chop + ADSR over the thumbnail (fade).
        let mut h = harness(size, View::Perform, |app| {
            showcase(app);
            sliced(app, 4, 6);
            app.with_studio(|st, h| {
                if let Some(perf) = chromagrain_ui::Host::perform(h) {
                    perf.set_pad_env(4, chromagrain_core::envelope::Adsr { attack: 0.08, decay: 0.3, sustain: 0.55, release: 0.6, picture: PictureCut::Fade, ..Default::default() });
                }
                st.open_clip(ClipTarget::Pad(4));
            });
        });
        h.run_steps(6);
        shot(&mut h, &name("pad-sheet", size));
        // Tile layout on the Play and Perform stages.
        for (view, what) in [(View::Play, "play-tile"), (View::Perform, "perform-tile")] {
            let mut h = harness(size, view, move |app| {
                showcase(app);
                app.project.lock().unwrap().layout = Layout::Tile;
                if view == View::Perform {
                    for pad in [0usize, 4, 9] {
                        app.toggle_pad(pad);
                    }
                    app.playback.stop();
                    app.playhead_beats = 1.5;
                }
            });
            h.run_steps(8);
            shot(&mut h, &name(what, size));
        }
    }
    // Emitter Deep page with its envelope, and a lane sheet (big size).
    let mut h = harness(SIZES[0], View::Play, |app| {
        showcase(app);
        app.studio.emitters[0].adsr = chromagrain_core::envelope::Adsr { attack: 0.02, decay: 0.2, sustain: 0.7, release: 0.35, ..Default::default() };
        app.studio.deep = Some((DeepTarget::Emitter(0), DeepTab::Links));
    });
    h.run_steps(20);
    shot(&mut h, &name("deep-emitter-adsr", SIZES[0]));
    let mut h = harness(SIZES[0], View::Sequence, |app| {
        showcase(app);
        sliced(app, 1, 4);
        app.studio.open_clip(ClipTarget::Lane(1));
    });
    h.run_steps(8);
    shot(&mut h, &name("lane-sheet", SIZES[0]));
}

#[test]
fn the_add_sheet_opens_from_grid_pad_and_lane_with_its_target() {
    let mut h = harness(SIZES[1], View::Play, |_| {});
    h.run_steps(3);
    h.get_by_label("column 8").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Add(AddTarget::Column(7))));
    for tab in ["add Files", "add Browse", "add Link", "add Dig"] {
        h.get_by_label(tab).click();
        h.run_steps(2);
    }
    assert_eq!(h.state().studio.add.tab, AddRoute::Dig);
    h.get_by_label("close sheet").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, None);

    let mut h = harness(SIZES[1], View::Perform, |_| {});
    h.run_steps(3);
    h.get_by_label("pad 16").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Add(AddTarget::Pad(15))));

    let mut h = harness(SIZES[1], View::Sequence, |_| {});
    h.run_steps(3);
    h.get_by_label("add a lane").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Add(AddTarget::Lane(7))));
}

#[test]
fn files_land_where_they_were_asked_for_and_offer_chop() {
    let path = std::env::temp_dir().join(format!("cg-v003-{}.wav", std::process::id()));
    let tone = chromagrain_core::audio::AudioClip::sine(330.0, 1.5, 48000);
    let buf = chromagrain_core::audio::StereoBuffer { left: tone.samples.clone(), right: tone.samples.clone(), sample_rate: 48000 };
    chromagrain_core::media::save_wav(&path, &buf).unwrap();
    let p2 = path.clone();
    let mut h = harness(SIZES[1], View::Play, move |app| {
        app.pick_override = Some(Box::new(move || vec![p2.clone()]));
    });
    h.run_steps(3);
    h.get_by_label("column 8").click();
    h.run_steps(2);
    h.get_by_label("add Files").click();
    h.run_steps(2);
    h.get_by_label("choose files").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.jobs.len(), 1, "a placeholder while it decodes");
    let t = std::time::Instant::now();
    while !h.state().adds.jobs.is_empty() {
        assert!(t.elapsed().as_secs() < 20, "the file never landed");
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
    let n = clips().len();
    assert_eq!(h.state().studio.columns[7], Some(n), "the new source is in column 8");
    assert_eq!(h.state().studio.add.recent, vec![n], "and on a card in the sheet");
    h.get_by_label(&format!("chop {}", h.state().project.lock().unwrap().sources[n].name)).click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Source(n))), "Chop opens the clip sheet");
    let _ = std::fs::remove_file(path);
}

#[test]
fn link_field_takes_typed_and_pasted_text_and_dig_queues_jobs() {
    let mut h = harness(SIZES[1], View::Play, |app| {
        app.studio.add.tab = AddRoute::Link;
        app.studio.sheet = Some(Sheet::Add(AddTarget::Any));
    });
    h.run_steps(3);
    h.get_by_label("link url").click();
    h.run_steps(1);
    push(&mut h, egui::Event::Text("https://youtu.be/".into()));
    push(&mut h, egui::Event::Paste("abc123".into()));
    h.run_steps(2);
    assert_eq!(h.state().studio.add.link, "https://youtu.be/abc123");
    h.get_by_label("fetch link").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.jobs.len(), 1, "fetching shows a placeholder");
    assert!(h.state().studio.add.link.is_empty());
    // Dig: tap = 1, hold = 4.
    h.get_by_label("add Dig").click();
    h.run_steps(2);
    h.get_by_label("dig").click();
    h.run_steps(2);
    assert_eq!(h.state().adds.jobs.len(), 2);
    long_press(&mut h, "dig");
    assert_eq!(h.state().adds.jobs.len(), 6, "hold digs 4");
}

#[test]
fn chop_sheet_slices_taps_drags_deletes_and_sends_to_pads() {
    let mut h = harness(SIZES[0], View::Play, |_| {});
    h.run_steps(3);
    long_press(&mut h, "column 1");
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Column(0))), "long-press a header opens the clip sheet");
    let slices = |h: &Harness<'static, App>| h.state().project.lock().unwrap().sources[0].slices.clone();
    h.get_by_label("slice 8").click();
    h.run_steps(2);
    assert_eq!(slices(&h).len(), 8);
    // Drag the boundary between slices 1 and 2 to the right.
    let before = slices(&h)[0].end;
    let grip = h.get_by_label("slice 1 end").rect().center();
    drag(&mut h, grip, grip + egui::vec2(40.0, 0.0));
    let after = slices(&h);
    assert!(after[0].end > before + 0.01 && (after[1].start - after[0].end).abs() < 1e-9, "the shared edge moved for both: {before} -> {}", after[0].end);
    // Double-tap slice 3: it merges into slice 2.
    let wave = h.get_by_label("chop wave").rect();
    let len = h.state().project.lock().unwrap().sources[0].duration();
    let s3 = after[2];
    let x = wave.left() + (((s3.start + s3.end) / 2.0) / len) as f32 * wave.width();
    let at = egui::pos2(x, wave.center().y);
    // Two taps in consecutive frames (kittest frames are 0.25 s apart,
    // inside egui's double-click window).
    for _ in 0..2 {
        push(&mut h, egui::Event::PointerMoved(at));
        push(&mut h, egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        push(&mut h, egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(2);
    assert_eq!(slices(&h).len(), 7, "double-tap deleted a slice");
    // Tap-chop: start, two chops while it plays, done.
    h.get_by_label("chop tap").click();
    h.run_steps(2);
    assert!(slices(&h).is_empty(), "tap-chop starts over");
    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        h.get_by_label("chop tap").click();
        h.run_steps(1);
    }
    h.get_by_label("chop tap done").click();
    h.run_steps(2);
    assert_eq!(slices(&h).len(), 2, "two taps: two slices (the head stays unsliced, like Koala)");
    // Slices -> pads.
    h.get_by_label("slices to pads").click();
    h.run_steps(2);
    let app = h.state();
    let p = app.project.lock().unwrap();
    let sl = p.sources[0].slices.clone();
    drop(p);
    assert_eq!(sl.len(), 2);
    // The app's pads are perform tracks: their first clips play slice 1 and 2.
    let tracks = if app.mode == crate::AppMode::Perform { app.project.lock().unwrap().tracks.clone() } else { app.other_tracks.clone() };
    let src_len = clips()[0].duration();
    for (i, s) in sl.iter().enumerate() {
        let c = tracks[i].clips.first().expect("a clip on the pad");
        assert_eq!(c.source, 0);
        assert!((c.grains.position as f64 * src_len - s.start).abs() < 1e-3, "pad {} starts at slice {}", i + 1, i + 1);
    }
}

#[test]
fn adsr_handles_shape_the_pad_envelope_and_picture_mode() {
    let mut h = harness(SIZES[0], View::Perform, |_| {});
    h.run_steps(3);
    long_press(&mut h, "pad 1");
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Pad(0))));
    let env = |h: &mut Harness<'static, App>| chromagrain_ui::Host::perform(h.state_mut()).and_then(|p| p.pad_env(0)).unwrap_or_default();
    let r0 = env(&mut h).release;
    let rh = h.get_by_label("adsr R").rect().center();
    drag(&mut h, rh, rh + egui::vec2(60.0, 0.0));
    let e = env(&mut h);
    assert!(e.release > r0 + 0.05, "dragging R right lengthens the release: {r0} -> {}", e.release);
    let sh = h.get_by_label("adsr S").rect().center();
    drag(&mut h, sh, sh + egui::vec2(0.0, 40.0));
    assert!(env(&mut h).sustain < 0.9, "dragging S down lowers the sustain");
    assert_eq!(env(&mut h).picture, PictureCut::Cut);
    h.get_by_label("picture fade").click();
    h.run_steps(2);
    assert_eq!(env(&mut h).picture, PictureCut::Fade);
}

#[test]
fn layout_switch_flips_stack_and_tile() {
    let mut h = harness(SIZES[1], View::Play, |_| {});
    h.run_steps(3);
    assert_eq!(h.state().project.lock().unwrap().layout, Layout::Stack);
    h.get_by_label("layout tile").click();
    h.run_steps(2);
    assert_eq!(h.state().project.lock().unwrap().layout, Layout::Tile);
    h.get_by_label("layout stack").click();
    h.run_steps(2);
    assert_eq!(h.state().project.lock().unwrap().layout, Layout::Stack);
}

#[test]
fn emitter_gestures_carry_their_envelope_and_tile() {
    let mut h = harness(SIZES[1], View::Play, |app| {
        app.project.lock().unwrap().layout = Layout::Tile;
        app.studio.emitters[0].adsr.release = 0.25;
        app.studio.emitters[0].held = true;
    });
    h.run_steps(4);
    std::thread::sleep(std::time::Duration::from_millis(80));
    h.run_steps(2);
    let held: Vec<_> = h.state().live_events.iter().filter(|(_, bus, _)| *bus == 1).map(|(_, _, e)| e.tag).collect();
    assert!(!held.is_empty(), "the held emitter sprays");
    let voice = held[0].voice;
    assert!(voice != 0 && held.iter().all(|t| t.voice == voice), "one gesture = one voice");
    assert!(held.iter().all(|t| t.env.is_some_and(|e| e.off.is_infinite())), "held: no note-off yet");
    assert!(held.iter().all(|t| t.tile == Some(0)), "the first gesture gets tile 0");
    // Release: grains keep coming through the release, with note-off set.
    h.state_mut().studio.emitters[0].held = false;
    h.run_steps(1);
    std::thread::sleep(std::time::Duration::from_millis(80));
    h.run_steps(2);
    let rel: Vec<_> = h.state().live_events.iter().filter(|(_, bus, e)| *bus == 1 && e.tag.env.is_some_and(|v| v.off.is_finite())).map(|(_, _, e)| e.tag).collect();
    assert!(!rel.is_empty(), "the release sounds (and shows) too");
    assert!(rel.iter().all(|t| t.voice == voice));
    let _ = Slice::new(0.0, 1.0);
}

#[test]
fn d_hybrid_open_clip_buttons_pad_chevrons_project_sheet_and_owner_tabs() {
    // 1. Play: `Open clip ›` in the object bar and `Clip ›` on emitter cards open `Sheet::Clip(ClipTarget::Emitter(_))`.
    let mut h = harness(SIZES[0], View::Play, |_| {});
    h.run_steps(3);
    assert_eq!(h.state().studio.sheet, None);
    h.get_by_label("open clip").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Emitter(0))));
    h.get_by_label("close sheet").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, None);

    h.get_by_label("clip emitter 2").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sel_emitter, 1);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Emitter(1))));

    // 2. Editing Grain / Links / Shape cards inside `Sheet::Clip` mutates engine parameters.
    let dens0 = h.state().studio.emitters[1].cloud.density;
    let dk = h.get_by_label("grain density").rect().center();
    drag(&mut h, dk, dk - egui::vec2(0.0, 70.0));
    assert!(
        h.state().studio.emitters[1].cloud.density > dens0 + 8.0,
        "grain density knob mutated emitter cloud: {dens0} -> {}",
        h.state().studio.emitters[1].cloud.density
    );

    let hue0 = h.state().studio.emitters[1].link.pitch_to_hue;
    let hk = h.get_by_label("pitch → hue").rect().center();
    drag(&mut h, hk, hk + egui::vec2(80.0, 0.0));
    assert!(
        h.state().studio.emitters[1].link.pitch_to_hue > hue0 + 2.0,
        "pitch → hue pill mutated AvLink: {hue0} -> {}",
        h.state().studio.emitters[1].link.pitch_to_hue
    );

    assert_eq!(h.state().studio.emitters[1].adsr.picture, PictureCut::Cut);
    h.get_by_label("picture fade").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.emitters[1].adsr.picture, PictureCut::Fade);

    // 3. Owner Switcher tabs inside `Sheet::Clip` switch cleanly between `Clip`, `Emitter`, `Lane`, and `Pad`.
    h.get_by_label("close sheet").click();
    h.run_steps(2);
    // Seed Perform pads so source 0 has Emitter 1, Lane 1, and Pad 1 owners.
    h.get_by_label("Perform").click();
    h.run_steps(2);
    h.get_by_label("Play").click();
    h.run_steps(2);
    h.get_by_label("emitter 1").click();
    h.run_steps(2);
    h.get_by_label("open clip").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Emitter(0))));

    h.get_by_label("owner Clip").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Column(0))));

    h.get_by_label("owner Lane 1").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Lane(0))));

    h.get_by_label("owner Pad 1").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Pad(0))));

    h.get_by_label("owner Emitter 1").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Emitter(0))));
    h.get_by_label("close sheet").click();
    h.run_steps(2);

    // 4. Perform: clicking `›` (`"clip pad 1"`) opens `ClipTarget::Pad(0)` WITHOUT toggling the pad.
    h.get_by_label("Perform").click();
    h.run_steps(2);
    let muted_before = h.state().project.lock().unwrap().tracks[0].muted;
    assert!(muted_before, "pad 1 starts stopped");
    h.get_by_label("clip pad 1").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Clip(ClipTarget::Pad(0))));
    let muted_after = h.state().project.lock().unwrap().tracks[0].muted;
    assert_eq!(muted_after, muted_before, "clicking `›` on a pad must not launch/toggle it");
    h.get_by_label("close sheet").click();
    h.run_steps(2);

    // 5. Top bar `Project` button opens `Sheet::Project`, and master FX pills mutate `project.master_effects`.
    h.get_by_label("Project").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, Some(Sheet::Project));
    assert!(h.state().project.lock().unwrap().master_effects.is_empty());
    h.get_by_label("master fx reverb").click();
    h.run_steps(2);
    assert_eq!(h.state().project.lock().unwrap().master_effects.len(), 1);
    h.get_by_label("close sheet").click();
    h.run_steps(2);
    assert_eq!(h.state().studio.sheet, None);
}

#[test]
fn ui_hybrid_snapshots() {
    let ver = version();
    let tag = |what: &str, size: (f32, f32)| format!("261112_chromagrain_ui-hybrid_{what}_{}x{}_{ver}", size.0, size.1);

    for size in [(1600.0f32, 1000.0f32), (960.0, 600.0)] {
        // 1. Play (with on-object emitter rings, pitch dots, ADSR curves, gesture bubble on Emitter 2)
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            app.studio.emitters[0].macros = [0.22, 0.5, 0.25, 0.0];
            app.studio.emitters[1].macros[0] = 0.48;
            app.studio.emitters[1].macros[1] = 0.66;
            app.studio.emitters[1].adsr.picture = PictureCut::Fade;
            app.studio.emitters[2].macros = [0.68, 0.66, 0.35, 0.0];
            app.studio.emitters[3].macros[0] = 0.42;
            app.studio.emitters[3].macros[1] = 0.36;
            app.studio.gesture_hud = Some((1, "Pitch  +4 C#".into(), 999.0));
        });
        h.run_steps(8);
        shot(&mut h, &tag("play", size));

        // 2. Clip sheet opened over Play for Emitter 1 · horses
        let mut h = harness(size, View::Play, |app| {
            showcase(app);
            app.with_studio(|st, host| st.set_view(host, View::Perform));
            app.with_studio(|st, host| st.set_view(host, View::Play));
            sliced(app, 0, 10);
            app.studio.emitters[0].adsr = chromagrain_core::envelope::Adsr {
                attack: 0.04,
                decay: 0.28,
                sustain: 0.68,
                release: 0.35,
                picture: PictureCut::Cut,
                cut_threshold: 0.08,
            };
            app.studio.open_clip(ClipTarget::Emitter(0));
        });
        h.run_steps(8);
        shot(&mut h, &tag("clip-sheet", size));

        // 3. Perform (Tile layout with chopped slices, ADSR curves + `›` on pads, selected pad ring)
        let mut h = harness(size, View::Perform, |app| {
            showcase(app);
            app.project.lock().unwrap().layout = Layout::Tile;
            sliced(app, 0, 10);
            app.with_studio(|st, host| {
                st.slices_to_pads(host, 0);
            });
            for pad in [0usize, 2, 5, 11] {
                app.toggle_pad(pad);
            }
            app.studio.sel_pad = 2;
            app.playback.stop();
            app.playhead_beats = 1.5;
        });
        h.run_steps(8);
        shot(&mut h, &tag("perform-tile", size));

        // 4. Project sheet over Sequence
        let mut h = harness(size, View::Sequence, |app| {
            showcase(app);
            app.project.lock().unwrap().master_effects.push(chromagrain_core::fx::AvEffect {
                kind: chromagrain_core::fx::EffectKind::Reverb { size: 0.6, damp: 0.4, mix: 0.35 },
                audio: 0.35,
                video: 0.25,
            });
            app.studio.sheet = Some(Sheet::Project);
        });
        h.run_steps(8);
        shot(&mut h, &tag("project-sheet", size));
    }

    // 5. Sequence at 1600×1000
    let mut h = harness((1600.0, 1000.0), View::Sequence, showcase);
    h.run_steps(8);
    shot(&mut h, &tag("sequence", (1600.0, 1000.0)));
}
