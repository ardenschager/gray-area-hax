//! End-to-end: build a project, render it, bounce to mp4 with ffmpeg,
//! decode the mp4 back in and granulate THAT — the full sampling loop.

use chromagrain_core::media;
use chromagrain_core::render::render_project;
use chromagrain_core::script::run_script;
use chromagrain_core::timeline::Project;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn tmp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join("chromagrain-it").join(name);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn offline_render_full_demo() {
    let p = Project::demo();
    let out = render_project(&p, 0.0, 8.0);
    let secs = p.beats_to_secs(8.0);
    assert!((out.audio.duration() - secs).abs() < 0.1);
    assert_eq!(out.frames.len(), (secs * p.fps as f64).ceil() as usize);
    assert!(out.audio.rms() > 0.005);
    assert!(out.frames.iter().any(|f| f.mean_luma() > 1.0));
}

#[test]
fn mp4_roundtrip_and_regranulation() {
    if !media::ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = tmp_dir("roundtrip");
    let mp4 = dir.join("bounce.mp4");

    // 1. Render the demo project and encode to mp4.
    let p = Project::demo();
    let out = render_project(&p, 0.0, 4.0);
    media::encode_video(&mp4, &out.frames, out.fps, &out.audio).unwrap();
    assert!(mp4.metadata().unwrap().len() > 1000);

    // 2. Decode the mp4 back as a source (as if it were a scraped video).
    let src = media::load_source(&mp4, 48000).unwrap();
    let audio = src.audio.as_ref().expect("audio decoded");
    let video = src.video.as_ref().expect("video decoded");
    assert!(audio.duration() > 1.5, "audio {}s", audio.duration());
    assert!(video.duration() > 1.5, "video {}s", video.duration());
    assert!(video.frames[0].width > 0);

    // 3. Granulate the decoded media into a new render.
    let mut p2 = Project::default();
    let sid = p2.add_source(src);
    let tid = p2.add_track("regrain");
    let mut clip = chromagrain_core::timeline::Clip::new(sid, 0.0, 4.0);
    clip.grains.density = 25.0;
    p2.tracks[tid].clips.push(clip);
    let out2 = render_project(&p2, 0.0, 4.0);
    assert!(out2.audio.rms() > 0.001, "regranulated rms {}", out2.audio.rms());
    assert!(out2.frames.iter().any(|f| f.mean_luma() > 0.5));
}

#[test]
fn scripted_mp4_render() {
    if !media::ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = tmp_dir("scripted");
    let project = Arc::new(Mutex::new(Project::default()));
    let script = r#"
        let s = session(100.0);
        s.canvas(320, 180, 12.0);
        let src = s.demo_source();
        let t = s.track("g");
        let c = s.clip(t, src, 0.0, 4.0);
        s.key(c, "C", "major_pentatonic");
        s.set(c, "density", 25.0);
        s.set(c, "pitch_jitter", 10.0);
        s.set(c, "spray", 0.6);
        s.audio_filter(c, "bandpass", 900.0, 1.2);
        s.color_remove(c, 0.0, 40.0);
        s.render(0.0, 4.0, "scripted.mp4");
    "#;
    let log = run_script(project, &dir, script).unwrap();
    assert!(log.contains("rendered"), "log: {log}");
    let mp4 = dir.join("scripted.mp4");
    assert!(mp4.exists() && mp4.metadata().unwrap().len() > 1000);
}

#[test]
fn youtube_fetch_smoke() {
    // Network-dependent; only runs when explicitly requested.
    if std::env::var("CHROMAGRAIN_NET_TESTS").is_err() {
        eprintln!("skipping: set CHROMAGRAIN_NET_TESTS=1 to run yt-dlp test");
        return;
    }
    let dir = tmp_dir("yt");
    let src = media::load_youtube_source(
        "https://www.youtube.com/watch?v=jNQXAC9IVRw", // "Me at the zoo", 19s
        &dir,
        48000,
    )
    .unwrap();
    assert!(src.duration() > 5.0);
    let _ = Path::new("unused");
}
