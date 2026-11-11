//! The plugin editor, headless: the real studio UI driven by the plugin's
//! audio side (no host, no window). Snapshots via egui_kittest + wgpu,
//! and an offline test that plays what the editor sets up.
//!
//! `CHROMAGRAIN_UI_SHOTS=/dir cargo test -p chromagrain-plugin --test editor`
//! writes the snapshots as PNGs.

use chromagrain_core::audio::AudioClip;
use chromagrain_core::instrument::Transport;
use chromagrain_core::timeline::Source;
use chromagrain_core::video::VideoClip;
use chromagrain_plugin::engine::{Ev, Job};
use chromagrain_plugin::ui_host::PluginUi;
use chromagrain_plugin::Chromagrain;
use chromagrain_ui::{PerformHost, View};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SR: u32 = 48_000;

const DESIGN_CLIPS: [(&str, &str); 7] = [
    ("oregon", "horses"),
    ("betty", "beanstalk"),
    ("aurora", "aurora"),
    ("bees", "comb"),
    ("nyc", "concourse"),
    ("hummingbird", "hummingbird"),
    ("map", "rivers"),
];

/// The design's clips when ffmpeg + design/v001/media are around, else
/// procedural stand-ins.
fn clips() -> Vec<Source> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../design/v001/media");
    if chromagrain_core::media::ffmpeg_available() && dir.exists() {
        let got: Option<Vec<Source>> = DESIGN_CLIPS
            .iter()
            .map(|(f, name)| {
                let mut s = chromagrain_core::media::load_source(&dir.join(format!("{f}.mp4")), SR).ok()?;
                s.name = (*name).into();
                Some(s)
            })
            .collect();
        if let Some(v) = got {
            return v;
        }
    }
    (0..7)
        .map(|i| Source {
            name: DESIGN_CLIPS[i].1.into(),
            audio: Some(Arc::new(AudioClip::saw_stack(110.0 * (1.0 + i as f32 * 0.25), 3.0, SR))),
            video: Some(Arc::new(VideoClip::test_pattern(160, 90, 12.0, 3.0))),
            base_hz: 110.0,
        })
        .collect()
}

fn sine() -> Source {
    Source {
        name: "sine".into(),
        audio: Some(Arc::new(AudioClip::sine(220.0, 3.0, SR))),
        video: Some(Arc::new(VideoClip::test_pattern(32, 18, 12.0, 3.0))),
        base_hz: 220.0,
    }
}

/// A plugin with `sources` loaded on both the audio side and the
/// editor's mirror.
fn plugin_with(sources: Vec<Source>) -> Chromagrain {
    let mut p = Chromagrain::default();
    p.setup(SR);
    let n = sources.len();
    for s in sources {
        p.shared().send(Job::Decoded(Box::new(s)));
    }
    let t = Instant::now();
    while p.audio().inst.sources().len() < n {
        assert!(t.elapsed() < Duration::from_secs(20), "loader never delivered the media");
        std::thread::sleep(Duration::from_millis(5));
        p.render(&mut [&mut [][..], &mut [][..]], None, &mut || None);
    }
    p.setup(SR);
    p
}

fn editor_ui(p: &Chromagrain) -> PluginUi {
    PluginUi::new(p.shared().clone(), p.params_ref().clone(), None)
}

/// Run editor frames headlessly (no pixels).
fn frames(ui: &mut PluginUi, ctx: &egui::Context, n: usize) {
    for _ in 0..n {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1180.0, 820.0))),
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| ui.frame(ctx));
    }
}

fn harness(size: (f32, f32), ui: PluginUi) -> Harness<'static, PluginUi> {
    let rs = egui_kittest::wgpu::create_render_state(egui_kittest::wgpu::default_wgpu_setup());
    chromagrain_ui::gpu::install(&rs);
    let renderer = egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs);
    Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(1.0)
        .renderer(renderer)
        .build_state(|ctx, ui: &mut PluginUi| ui.frame(ctx), ui)
}

fn shot(h: &mut Harness<'static, PluginUi>, name: &str) {
    let img = h.render().expect("render");
    if let Ok(dir) = std::env::var("CHROMAGRAIN_UI_SHOTS") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    }
}

/// Render `secs` with the host playing at 120 bpm from `beat0`, `events`
/// at absolute sample positions. Returns the left channel.
fn play(p: &mut Chromagrain, secs: f64, beat0: f64, events: &[(usize, Ev)]) -> Vec<f32> {
    let total = (secs * SR as f64) as usize;
    let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
    let block = 512;
    let mut pos = 0;
    while pos < total {
        let n = block.min(total - pos);
        let tr = Some(Transport { bpm: 120.0, playing: true, beat: beat0 + pos as f64 / SR as f64 * 2.0 });
        let mut evs = events.iter().filter(|(t, _)| *t >= pos && *t < pos + n).map(|(t, e)| match *e {
            Ev::On { note, velocity, .. } => Ev::On { timing: (*t - pos) as u32, note, velocity },
            Ev::Off { note, .. } => Ev::Off { timing: (*t - pos) as u32, note },
        });
        p.render(&mut [&mut l[pos..pos + n], &mut r[pos..pos + n]], tr, &mut || evs.next());
        pos += n;
    }
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The design's lanes, set through the editor.
fn lanes(ui: &mut PluginUi) {
    let hits: [&[usize]; 7] = [&[0, 4, 8, 10, 12], &[2, 5, 9, 11], &[2, 6, 10, 14, 15], &[0, 3, 5], &[0, 8], &[1, 4, 6, 9], &[7, 15]];
    for (lane, hs) in hits.iter().enumerate() {
        for s in hs.iter() {
            ui.studio.seq_toggle(&mut ui.host, 0, lane, *s);
        }
    }
    ui.studio.set_lane_len(&mut ui.host, 1, 12);
    ui.studio.set_lane_len(&mut ui.host, 3, 7);
}

#[test]
fn plugin_editor_snapshots() {
    let version = std::env::var("CHROMAGRAIN_UI_VERSION").unwrap_or_else(|_| "v001".into());
    let mut p = plugin_with(clips());
    for size in [(960.0f32, 600.0f32), (1180.0, 820.0)] {
        for (view, vname) in [(View::Play, "play"), (View::Sequence, "sequence")] {
            let mut h = harness(size, editor_ui(&p));
            h.run_steps(2); // theme, sources mirrored, scene 1 created
            {
                let ui = h.state_mut();
                lanes(ui);
                ui.studio.set_view(&mut ui.host, view);
                ui.studio.emitters[0].held = view == View::Play;
            }
            h.run_steps(8);
            let st = h.state();
            assert_eq!(st.studio.view, view);
            assert_eq!(st.host.project.lock().unwrap().sources.len(), 7, "the editor sees the plugin's media");
            shot(&mut h, &format!("261111_plugin_editor_{vname}_{}x{}_{version}", size.0, size.1));
        }
    }
    // Perform: the clips on pads 1–7, three of them playing.
    let mut ui = editor_ui(&p);
    frames(&mut ui, &egui::Context::default(), 2);
    for i in 0..7 {
        ui.host.load_pad(i, i);
    }
    for i in [0, 2, 5] {
        ui.host.toggle_pad(i);
    }
    play(&mut p, 1.2, 0.0, &[]);
    for size in [(960.0f32, 600.0f32), (1180.0, 820.0), (1600.0, 1000.0)] {
        for full in [false, true] {
            if full && size.0 != 1180.0 {
                continue;
            }
            play(&mut p, 0.1, 2.4, &[]);
            let mut h = harness(size, editor_ui(&p));
            h.run_steps(2);
            {
                let ui = h.state_mut();
                ui.studio.set_view(&mut ui.host, View::Perform);
                ui.host.fullscreen = full;
            }
            h.run_steps(6);
            let pads = h.state().host.pads();
            assert_eq!(pads.iter().filter(|p| p.source.is_some()).count(), 7);
            assert_eq!(pads.iter().filter(|p| p.on).count(), 3, "{pads:?}");
            let name = if full { "perform-fullscreen" } else { "perform" };
            shot(&mut h, &format!("261111_plugin_editor_{name}_{}x{}_{version}", size.0, size.1));
        }
    }
}

/// Pads from the editor and from MIDI, launched on the host's beat grid,
/// drawn on the canvas, and restored with the session.
#[test]
fn perform_pads_launch_on_the_host_beat_from_the_editor_and_midi() {
    let mut p = plugin_with(vec![sine()]);
    let mut ui = editor_ui(&p);
    let ctx = egui::Context::default();
    frames(&mut ui, &ctx, 2);
    ui.studio.set_view(&mut ui.host, View::Perform);
    assert_eq!(ui.studio.view, View::Perform);
    // 3 s of media at 120 bpm = 6 beats: a 4-beat loop.
    ui.host.load_pad(0, 0);
    assert_eq!(p.params_ref().pads.lock().unwrap().as_slice(), &[(0, 0, 0.0, 4.0)], "persisted");
    ui.host.toggle_pad(0);
    ui.host.toggle_pad(1); // empty: nothing
    // The host plays from beat 0.3: "Pad launch" = beat, so the loop
    // starts on beat 1, 0.35 s in.
    let l = play(&mut p, 1.0, 0.3, &[]);
    let at = (0.35 * SR as f64) as usize;
    assert_eq!(rms(&l[..at - 8]), 0.0, "silent until the beat");
    assert!(rms(&l[at + 2_400..]) > 0.05, "the loop plays from beat 1: {}", rms(&l[at + 2_400..]));
    frames(&mut ui, &ctx, 1);
    let pads = ui.host.pads();
    assert!(pads[0].on && pads[0].queued.is_none() && !pads[1].on);
    assert!(ui.host.live.iter().any(|(s, bus, _)| *s == 0 && *bus == chromagrain_plugin::ui_host::PAD_BUS), "the canvas draws the loop's grains");
    // MIDI note 36 (pad 1) stops it on the next beat: beat 2.3 -> 3.
    let l = play(&mut p, 1.0, 2.3, &[(0, Ev::On { timing: 0, note: 36, velocity: 1.0 })]);
    let at = (0.35 * SR as f64) as usize;
    assert!(rms(&l[..at - 8]) > 0.05, "still playing up to the beat");
    assert!(rms(&l[at + 6_000..]) < 1e-3, "stopped on beat 3");
    frames(&mut ui, &ctx, 1);
    assert!(!ui.host.pads()[0].on);
    // A restored session gets the pad back.
    let saved = p.params_ref().pads.lock().unwrap().clone();
    let mut q = plugin_with(vec![sine()]);
    *q.params_ref().pads.lock().unwrap() = saved;
    q.setup(SR);
    assert!(q.audio().inst.pad(0).slot.is_some());
    assert!(editor_ui(&q).host.pads()[0].source == Some(0));
}

/// Tapping a pad in the Perform view queues its launch.
#[test]
fn tapping_a_pad_in_the_editor_launches_it() {
    let mut p = plugin_with(vec![sine()]);
    let mut h = harness((1180.0, 820.0), editor_ui(&p));
    h.run_steps(2);
    {
        let ui = h.state_mut();
        ui.studio.set_view(&mut ui.host, View::Perform);
        ui.host.load_pad(0, 0);
    }
    h.run_steps(2);
    play(&mut p, 0.05, 0.0, &[]); // the slot reaches the audio thread
    h.get_by_label("pad 1").click();
    h.run_steps(2);
    play(&mut p, 0.05, 0.5, &[]);
    assert_eq!(p.audio().inst.pad(0).queued, Some(true), "queued for beat 1");
    play(&mut p, 0.4, 0.6, &[]);
    assert!(p.audio().inst.pad(0).on);
    h.run_steps(1);
    assert!(h.state().host.pads()[0].on, "and the editor shows it");
}

#[test]
fn plugin_offers_play_sequence_and_perform_with_the_host_owning_transport() {
    let p = plugin_with(vec![sine()]);
    let mut ui = editor_ui(&p);
    let ctx = egui::Context::default();
    frames(&mut ui, &ctx, 2);
    use chromagrain_ui::Host;
    let caps = ui.host.caps();
    assert_eq!(caps.views, vec![View::Play, View::Sequence, View::Perform]);
    assert!(!caps.output && !caps.rec && !caps.scenes, "no output window, takes or scenes in a DAW");
    // Toggling play in the UI does nothing: the DAW owns the transport.
    ui.host.toggle_play();
    assert!(!ui.host.playing());
    // The knobs are the plugin's macro parameters.
    ui.host.set_shared_macro(0, 0.9);
    frames(&mut ui, &ctx, 1);
    assert!(ui.studio.emitters.iter().all(|e| (e.macros[0] - 0.9).abs() < 1e-6), "every emitter shows the GRAIN param");
}

/// What the editor sets up, the plugin plays: step lanes in sync with the
/// host transport, and emitter grains as one-shots.
#[test]
fn offline_the_editor_drives_the_instrument() {
    let mut p = plugin_with(vec![sine()]);
    let mut ui = editor_ui(&p);
    let ctx = egui::Context::default();
    frames(&mut ui, &ctx, 2);
    // Lane 1: a hit on every beat, made through the editor.
    for s in [0, 4, 8, 12] {
        ui.studio.seq_toggle(&mut ui.host, 0, 0, s);
    }
    frames(&mut ui, &ctx, 1); // pushes the lanes to the audio thread
    let t = Instant::now();
    loop {
        p.render(&mut [&mut [][..], &mut [][..]], None, &mut || None);
        if p.audio().inst.pattern().is_some_and(|pat| pat.rows[0].steps[4].on) {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "lanes never reached the audio thread");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Host plays at 120 bpm from beat 0: hits at 0, 0.5, 1.0, 1.5 s.
    let total = SR as usize * 2;
    let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
    let block = 512;
    let mut pos = 0;
    while pos < total {
        let n = block.min(total - pos);
        let beat = pos as f64 / SR as f64 * 2.0;
        let tr = Some(Transport { bpm: 120.0, playing: true, beat });
        p.render(&mut [&mut l[pos..pos + n], &mut r[pos..pos + n]], tr, &mut || None);
        pos += n;
    }
    let rms = |a: usize, b: usize| (l[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt();
    let win = SR as usize / 20; // 50 ms
    for k in 0..4 {
        let at = k * SR as usize / 2;
        assert!(rms(at, at + win) > 0.02, "hit {k} sounds at {} s", at as f32 / SR as f32);
    }
    // The editor shows the host's playhead.
    frames(&mut ui, &ctx, 1);
    use chromagrain_ui::Host;
    assert!(ui.host.playing() && ui.host.playhead_beats() > 3.5);

    // Transport stopped: lanes are silent, but an emitter held on the grid
    // sounds (its grains go to the instrument as one-shots).
    let stop = Some(Transport { bpm: 120.0, playing: false, beat: 0.0 });
    let mut quiet = vec![0.0f32; SR as usize / 4];
    let mut q2 = quiet.clone();
    p.render(&mut [&mut quiet[..], &mut q2[..]], stop, &mut || None);
    let tail = &quiet[quiet.len() / 2..];
    assert!(tail.iter().map(|x| x * x).sum::<f32>() / (tail.len() as f32) < 1e-6, "stopped: lanes silent");
    ui.studio.emitters[0].held = true;
    for _ in 0..10 {
        frames(&mut ui, &ctx, 1);
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut out = vec![0.0f32; SR as usize / 2];
    let mut out_r = out.clone();
    let t = Instant::now();
    let mut heard = 0.0f32;
    while heard < 0.01 && t.elapsed() < Duration::from_secs(5) {
        frames(&mut ui, &ctx, 1);
        p.render(&mut [&mut out[..], &mut out_r[..]], stop, &mut || None);
        heard = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
    }
    assert!(heard > 0.01, "the held emitter sounds ({heard})");
}
