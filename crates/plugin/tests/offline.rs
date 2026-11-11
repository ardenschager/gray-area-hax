//! Offline plugin tests: instantiate the plugin exactly as a host would
//! (Default + activation), hand it media through the loader thread, play
//! notes and render blocks — no host, no GUI.

use chromagrain_core::audio::AudioClip;
use chromagrain_core::instrument::Transport;
use chromagrain_core::timeline::Source;
use chromagrain_core::video::VideoClip;
use chromagrain_plugin::engine::{Ev, Job};
use chromagrain_plugin::Chromagrain;
use nih_plug::prelude::Params;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SR: u32 = 48_000;

fn sine_source() -> Source {
    Source {
        name: "sine".into(),
        audio: Some(Arc::new(AudioClip::sine(220.0, 3.0, SR))),
        video: Some(Arc::new(VideoClip::test_pattern(32, 18, 12.0, 3.0))),
        base_hz: 220.0,
    }
}

/// Wait (without advancing the audio clock) until the loader has handed
/// the audio thread `n` sources.
fn wait_for_sources(p: &mut Chromagrain, n: usize) {
    let t = Instant::now();
    while p.audio().inst.sources().len() < n {
        assert!(t.elapsed() < Duration::from_secs(10), "loader never delivered the media");
        std::thread::sleep(Duration::from_millis(5));
        p.render(&mut [&mut [][..], &mut [][..]], None, &mut || None);
    }
}

fn ready_plugin() -> Chromagrain {
    let mut p = Chromagrain::default();
    p.setup(SR);
    p.shared().send(Job::Decoded(Box::new(sine_source())));
    wait_for_sources(&mut p, 1);
    p.setup(SR); // clock back to 0 with the media in place
    p
}

/// A little phrase: (absolute sample, event).
fn phrase() -> Vec<(usize, Ev)> {
    vec![
        (100, Ev::On { timing: 0, note: 60, velocity: 0.9 }),
        (7_000, Ev::On { timing: 0, note: 67, velocity: 0.6 }),
        (20_011, Ev::Off { timing: 0, note: 60 }),
        (30_500, Ev::On { timing: 0, note: 72, velocity: 1.0 }),
        (41_000, Ev::Off { timing: 0, note: 67 }),
        (52_345, Ev::Off { timing: 0, note: 72 }),
    ]
}

/// Render `total` samples in `block`-sized host blocks, events placed at
/// their absolute sample positions.
fn play(p: &mut Chromagrain, total: usize, block: usize, transport: Option<Transport>) -> (Vec<f32>, Vec<f32>) {
    let events = phrase();
    let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
    let mut pos = 0;
    while pos < total {
        let n = block.min(total - pos);
        let mut evs = events.iter().filter(|(t, _)| *t >= pos && *t < pos + n).map(|(t, e)| {
            let timing = (*t - pos) as u32;
            match *e {
                Ev::On { note, velocity, .. } => Ev::On { timing, note, velocity },
                Ev::Off { note, .. } => Ev::Off { timing, note },
            }
        });
        let (lb, rb) = (&mut l[pos..pos + n], &mut r[pos..pos + n]);
        p.render(&mut [lb, rb], transport, &mut || evs.next());
        pos += n;
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn plays_notes_offline_finite_and_audible() {
    let mut p = ready_plugin();
    let t = Some(Transport { bpm: 120.0, playing: true, beat: 0.0 });
    let (l, r) = play(&mut p, SR as usize * 2, 512, t);
    assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() <= 1.0), "finite, within ±1");
    assert!(rms(&l[2_000..20_000]) > 0.02, "the held note sounds: {}", rms(&l[2_000..20_000]));
    assert!(rms(&l[..90]) == 0.0, "silent before the first note");
    assert!(rms(&l[SR as usize * 2 - 4_000..]) < 1e-4, "rings out after the last note-off");
    assert!(p.shared().visible_grains.load(Ordering::Relaxed) == 0);
    assert_eq!(p.shared().sources.load(Ordering::Relaxed), 1);
}

#[test]
fn output_is_independent_of_host_block_size() {
    let total = SR as usize * 3 / 2;
    let reference = play(&mut ready_plugin(), total, 64, None);
    assert!(rms(&reference.0) > 0.02, "the comparison is not of silence");
    for block in [1, 333, 1024, 4096, 8192] {
        let out = play(&mut ready_plugin(), total, block, None);
        let d = reference.0.iter().zip(&out.0).chain(reference.1.iter().zip(&out.1)).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(d < 1e-5, "block {block}: max diff {d}");
    }
}

/// nih-plug's `assert_process_allocs` makes the debug-build global
/// allocator abort on any allocation inside the fence; fence the exact
/// code `process` runs (event handling, sub-blocks, gain, media hand-off).
#[cfg(debug_assertions)]
#[test]
fn render_does_not_allocate() {
    let mut p = ready_plugin();
    play(&mut p, 4_096, 512, None); // warm up
    p.shared().send(Job::Decoded(Box::new(sine_source())));
    std::thread::sleep(Duration::from_millis(100)); // let the loader queue it
    let events = phrase();
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    let t = Some(Transport { bpm: 97.0, playing: true, beat: 1.5 });
    for k in 0..120usize {
        let mut evs = events.iter().filter(|(s, _)| *s / 512 == k).map(|(s, e)| match *e {
            Ev::On { note, velocity, .. } => Ev::On { timing: (*s % 512) as u32, note, velocity },
            Ev::Off { note, .. } => Ev::Off { timing: (*s % 512) as u32, note },
        });
        assert_no_alloc::assert_no_alloc(|| {
            p.render(&mut [&mut l[..], &mut r[..]], t, &mut || evs.next());
        });
    }
    assert_eq!(p.audio().inst.sources().len(), 2, "the second source arrived through the lock-free hand-off");
}

#[test]
fn loads_a_file_on_the_loader_thread_and_persists_its_path() {
    let dir = std::env::temp_dir().join(format!("chromagrain-plugin-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for i in 0..SR {
        w.write_sample(((i as f32 * 0.05).sin() * 12_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();

    let mut p = Chromagrain::default();
    p.setup(SR);
    p.shared().send(Job::Load(wav.clone()));
    wait_for_sources(&mut p, 1);
    let path = wav.to_string_lossy().into_owned();
    assert_eq!(*p.params_ref().sources.lock().unwrap(), vec![path.clone()]);
    assert!(p.shared().status().contains("tone.wav"));

    // The paths are plugin state: a fresh instance restoring it decodes
    // the media again when the host activates it.
    let state = p.params_ref().serialize_fields();
    assert!(state.contains_key("sources"));
    let mut q = Chromagrain::default();
    q.params_ref().deserialize_fields(&state);
    q.setup(SR);
    wait_for_sources(&mut q, 1);
    assert_eq!(q.shared().loaded.lock().unwrap().as_slice(), [path]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reports_latency_for_the_current_look() {
    let mut p = ready_plugin();
    assert_eq!(p.audio().latency_samples(), 0, "no look, no latency");
    assert_eq!(p.setup(SR), 0);
}
