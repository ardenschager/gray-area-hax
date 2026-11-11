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
use chromagrain_plugin::engine::Job;
use chromagrain_plugin::ui_host::PluginUi;
use chromagrain_plugin::Chromagrain;
use chromagrain_ui::View;
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
    let p = plugin_with(clips());
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
}

#[test]
fn plugin_offers_play_and_sequence_with_the_host_owning_transport() {
    let p = plugin_with(vec![sine()]);
    let mut ui = editor_ui(&p);
    let ctx = egui::Context::default();
    frames(&mut ui, &ctx, 2);
    // Perform isn't a plugin view in v1: asking for it is a no-op.
    ui.studio.set_view(&mut ui.host, View::Perform);
    assert_eq!(ui.studio.view, View::Play);
    // Toggling play in the UI does nothing: the DAW owns the transport.
    use chromagrain_ui::Host;
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
