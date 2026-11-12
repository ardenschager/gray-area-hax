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
