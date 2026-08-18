//! Rhai scripting: the whole engine is drivable from scripts, both from the
//! app's script console and headless via `chromagrain --script`.
//!
//! ```rhai
//! let s = session(100.0);                    // bpm
//! let src = s.demo_source();                 // or s.load("clip.mp4")
//! let t = s.track("grains");                 //    s.youtube("https://…")
//! let c = s.clip(t, src, 0.0, 8.0);
//! s.key(c, "A", "minor_pentatonic");
//! s.set(c, "density", 30.0);
//! s.set(c, "pitch_jitter", 12.0);
//! s.audio_filter(c, "lowpass", 2000.0, 0.8);
//! s.color_keep(c, 200.0, 120.0);
//! s.render(0.0, 8.0, "out.mp4");
//! ```

use crate::audio::AudioClip;
use crate::dsp::{FilterKind, FilterSpec};
use crate::media;
use crate::music::Key;
use crate::render::render_project;
use crate::timeline::{Clip, Project, Source};
use crate::video::{ColorFilter, VideoClip};
use rhai::{Engine, EvalAltResult, Scope};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

/// Handle to a project shared between the script engine and a host app.
#[derive(Clone)]
pub struct Session {
    pub project: Arc<Mutex<Project>>,
    pub base_dir: PathBuf,
    pub cache_dir: PathBuf,
}

/// Handle to a clip placed by a script.
#[derive(Debug, Clone, Copy)]
pub struct ClipRef {
    pub track: i64,
    pub index: i64,
}

impl Session {
    pub fn new(project: Arc<Mutex<Project>>, base_dir: PathBuf) -> Session {
        let cache_dir = base_dir.join(".chromagrain-cache");
        Session { project, base_dir, cache_dir }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_dir.join(p)
        }
    }

    fn with_clip<T>(
        &mut self,
        c: ClipRef,
        f: impl FnOnce(&mut Clip) -> T,
    ) -> ScriptResult<T> {
        let mut p = self.project.lock().unwrap();
        let track = p
            .tracks
            .get_mut(c.track as usize)
            .ok_or_else(|| Box::<EvalAltResult>::from(format!("no track {}", c.track)))?;
        let clip = track
            .clips
            .get_mut(c.index as usize)
            .ok_or_else(|| Box::<EvalAltResult>::from(format!("no clip {}", c.index)))?;
        Ok(f(clip))
    }
}

fn rt_err(msg: impl Into<String>) -> Box<EvalAltResult> {
    Box::<EvalAltResult>::from(msg.into())
}

/// Build a rhai engine with the chromagrain API registered. `log` collects
/// script `print` output.
pub fn build_engine(session: Session, log: Arc<Mutex<String>>) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_expr_depths(128, 128);

    {
        let log = log.clone();
        engine.on_print(move |s| {
            let mut l = log.lock().unwrap();
            l.push_str(s);
            l.push('\n');
        });
    }

    engine
        .register_type_with_name::<Session>("Session")
        .register_type_with_name::<ClipRef>("Clip");

    let make_session = move |bpm: f64| -> Session {
        let s = session.clone();
        s.project.lock().unwrap().bpm = bpm.clamp(20.0, 300.0);
        s
    };
    engine.register_fn("session", make_session.clone());
    engine.register_fn("session", move || make_session(110.0));

    // ------------------------------------------------------------ project
    engine.register_fn("bpm", |s: &mut Session, bpm: f64| {
        s.project.lock().unwrap().bpm = bpm.clamp(20.0, 300.0);
    });
    engine.register_fn("canvas", |s: &mut Session, w: i64, h: i64, fps: f64| {
        let mut p = s.project.lock().unwrap();
        p.width = (w.clamp(16, 4096) as u32) & !1;
        p.height = (h.clamp(16, 4096) as u32) & !1;
        p.fps = fps.clamp(1.0, 60.0) as f32;
    });

    // ------------------------------------------------------------ sources
    engine.register_fn("demo_source", |s: &mut Session| -> i64 {
        let mut p = s.project.lock().unwrap();
        let sr = p.sample_rate;
        let src = Source {
            name: "demo saw+pattern".into(),
            audio: Some(AudioClip::saw_stack(110.0, 4.0, sr)),
            video: Some(VideoClip::test_pattern(240, 136, 12.0, 4.0)),
            base_hz: 110.0,
        };
        p.add_source(src) as i64
    });

    engine.register_fn(
        "sine_source",
        |s: &mut Session, freq: f64, secs: f64| -> i64 {
            let mut p = s.project.lock().unwrap();
            let sr = p.sample_rate;
            let src = Source {
                name: format!("sine {freq:.0}Hz"),
                audio: Some(AudioClip::sine(freq as f32, secs as f32, sr)),
                video: None,
                base_hz: freq as f32,
            };
            p.add_source(src) as i64
        },
    );

    engine.register_fn(
        "load",
        |s: &mut Session, path: &str| -> ScriptResult<i64> {
            let full = s.resolve(path);
            let sr = s.project.lock().unwrap().sample_rate;
            let src = media::load_source(&full, sr).map_err(rt_err)?;
            Ok(s.project.lock().unwrap().add_source(src) as i64)
        },
    );

    engine.register_fn(
        "youtube",
        |s: &mut Session, url: &str| -> ScriptResult<i64> {
            let sr = s.project.lock().unwrap().sample_rate;
            let src =
                media::load_youtube_source(url, &s.cache_dir, sr).map_err(rt_err)?;
            Ok(s.project.lock().unwrap().add_source(src) as i64)
        },
    );

    engine.register_fn("source_secs", |s: &mut Session, src: i64| -> f64 {
        s.project
            .lock()
            .unwrap()
            .sources
            .get(src as usize)
            .map(|s| s.duration())
            .unwrap_or(0.0)
    });

    engine.register_fn("base_hz", |s: &mut Session, src: i64| -> f64 {
        s.project
            .lock()
            .unwrap()
            .sources
            .get(src as usize)
            .map(|s| s.effective_base_hz() as f64)
            .unwrap_or(0.0)
    });

    engine.register_fn("set_base_hz", |s: &mut Session, src: i64, hz: f64| {
        if let Some(source) = s.project.lock().unwrap().sources.get_mut(src as usize) {
            source.base_hz = hz.max(0.0) as f32;
        }
    });

    // ------------------------------------------------------ tracks & clips
    engine.register_fn("track", |s: &mut Session, name: &str| -> i64 {
        s.project.lock().unwrap().add_track(name) as i64
    });

    engine.register_fn(
        "clip",
        |s: &mut Session,
         track: i64,
         source: i64,
         start_beat: f64,
         length_beats: f64|
         -> ScriptResult<ClipRef> {
            let mut p = s.project.lock().unwrap();
            if source as usize >= p.sources.len() {
                return Err(rt_err(format!("no source {source}")));
            }
            let t = p
                .tracks
                .get_mut(track as usize)
                .ok_or_else(|| rt_err(format!("no track {track}")))?;
            t.clips.push(Clip::new(source as usize, start_beat, length_beats.max(0.001)));
            Ok(ClipRef { track, index: (t.clips.len() - 1) as i64 })
        },
    );

    // Property-style setter for grain + visual parameters.
    engine.register_fn(
        "set",
        |s: &mut Session, c: ClipRef, prop: &str, value: f64| -> ScriptResult<()> {
            let key = prop.to_ascii_lowercase().replace([' ', '-'], "_");
            s.with_clip(c, |clip| -> Result<(), String> {
                let g = &mut clip.grains;
                let v = &mut clip.visual;
                let x = value as f32;
                match key.as_str() {
                    "density" => g.density = x.clamp(0.1, 500.0),
                    "duration" => g.duration = x.clamp(0.005, 5.0),
                    "duration_jitter" => g.duration_jitter = x.clamp(0.0, 1.0),
                    "position" => g.position = x.clamp(0.0, 1.0),
                    "spray" => g.spray = x.max(0.0),
                    "scan_speed" => g.scan_speed = x,
                    "pitch" => g.pitch = x.clamp(-48.0, 48.0),
                    "pitch_jitter" => g.pitch_jitter = x.clamp(0.0, 48.0),
                    "gain" => g.gain = x.clamp(0.0, 4.0),
                    "pan_spread" => g.pan_spread = x.clamp(0.0, 1.0),
                    "envelope" => g.envelope = x.clamp(0.01, 1.0),
                    "reverse_prob" => g.reverse_prob = x.clamp(0.0, 1.0),
                    "seed" => g.seed = value as u64,
                    "size_scale" => v.size_scale = x.max(0.0),
                    "min_size" => v.min_size = x.clamp(0.01, 1.0),
                    "max_size" => v.max_size = x.clamp(0.01, 1.0),
                    "hue_per_semitone" => v.hue_per_semitone = x,
                    "additive" => v.additive = x.clamp(0.0, 1.0),
                    "scatter_y" => v.scatter_y = x.clamp(0.0, 1.0),
                    other => return Err(format!("unknown parameter '{other}'")),
                }
                Ok(())
            })?
            .map_err(rt_err)
        },
    );

    engine.register_fn(
        "key",
        |s: &mut Session, c: ClipRef, root: &str, scale: &str| -> ScriptResult<()> {
            let key = Key::parse(root, scale)
                .ok_or_else(|| rt_err(format!("bad key: {root} {scale}")))?;
            s.with_clip(c, |clip| clip.key = Some(key))
        },
    );

    engine.register_fn("no_key", |s: &mut Session, c: ClipRef| -> ScriptResult<()> {
        s.with_clip(c, |clip| clip.key = None)
    });

    engine.register_fn(
        "audio_filter",
        |s: &mut Session, c: ClipRef, kind: &str, freq: f64, q: f64| -> ScriptResult<()> {
            let kind = FilterKind::parse(kind)
                .ok_or_else(|| rt_err(format!("bad filter kind: {kind}")))?;
            s.with_clip(c, |clip| {
                clip.audio_filters.push(FilterSpec {
                    kind,
                    freq: freq as f32,
                    q: q as f32,
                })
            })
        },
    );

    engine.register_fn(
        "clear_audio_filters",
        |s: &mut Session, c: ClipRef| -> ScriptResult<()> {
            s.with_clip(c, |clip| clip.audio_filters.clear())
        },
    );

    engine.register_fn(
        "color_keep",
        |s: &mut Session, c: ClipRef, hue: f64, width: f64| -> ScriptResult<()> {
            s.with_clip(c, |clip| {
                clip.color_filter = Some(ColorFilter::keep(hue as f32, width as f32))
            })
        },
    );

    engine.register_fn(
        "color_remove",
        |s: &mut Session, c: ClipRef, hue: f64, width: f64| -> ScriptResult<()> {
            s.with_clip(c, |clip| {
                clip.color_filter = Some(ColorFilter::remove(hue as f32, width as f32))
            })
        },
    );

    engine.register_fn(
        "no_color_filter",
        |s: &mut Session, c: ClipRef| -> ScriptResult<()> {
            s.with_clip(c, |clip| clip.color_filter = None)
        },
    );

    // ------------------------------------------------------------- render
    {
        let log_r = log.clone();
        engine.register_fn(
            "render",
            move |s: &mut Session, from_beat: f64, to_beat: f64, out: &str| -> ScriptResult<()> {
                let path = s.resolve(out);
                let output = {
                    let p = s.project.lock().unwrap();
                    render_project(&p, from_beat, to_beat)
                };
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                match ext.as_str() {
                    "wav" => media::save_wav(&path, &output.audio).map_err(rt_err)?,
                    "png" => {
                        let mid = &output.frames[output.frames.len() / 2];
                        media::save_frame_png(&path, mid).map_err(rt_err)?;
                    }
                    _ => {
                        media::encode_video(&path, &output.frames, output.fps, &output.audio)
                            .map_err(rt_err)?;
                    }
                }
                let mut l = log_r.lock().unwrap();
                l.push_str(&format!(
                    "rendered {:.1}s ({} frames, rms {:.3}) -> {}\n",
                    output.audio.duration(),
                    output.frames.len(),
                    output.audio.rms(),
                    path.display()
                ));
                Ok(())
            },
        );
    }

    engine.register_fn("ffmpeg_available", |_: &mut Session| -> bool {
        media::ffmpeg_available()
    });
    engine.register_fn("ytdlp_available", |_: &mut Session| -> bool {
        media::ytdlp_available()
    });

    engine
}

/// Run a script against a shared project. Returns collected print/log output.
pub fn run_script(
    project: Arc<Mutex<Project>>,
    base_dir: &Path,
    script: &str,
) -> Result<String, String> {
    let log = Arc::new(Mutex::new(String::new()));
    let session = Session::new(project, base_dir.to_path_buf());
    let engine = build_engine(session, log.clone());
    let mut scope = Scope::new();
    engine
        .run_with_scope(&mut scope, script)
        .map_err(|e| e.to_string())?;
    let out = log.lock().unwrap().clone();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Arc<Mutex<Project>> {
        Arc::new(Mutex::new(Project::default()))
    }

    #[test]
    fn script_builds_project() {
        let project = fresh();
        let script = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 4.0);
            s.key(c, "A", "minor_pentatonic");
            s.set(c, "density", 30.0);
            s.set(c, "pitch_jitter", 12.0);
            s.audio_filter(c, "lowpass", 2000.0, 0.8);
            s.color_keep(c, 200.0, 120.0);
            print("secs=" + s.source_secs(src));
        "#;
        let out = run_script(project.clone(), Path::new("/tmp"), script).unwrap();
        assert!(out.contains("secs="), "log: {out}");

        let p = project.lock().unwrap();
        assert_eq!(p.bpm, 120.0);
        assert_eq!(p.sources.len(), 1);
        assert_eq!(p.tracks.len(), 1);
        let clip = &p.tracks[0].clips[0];
        assert!(clip.key.is_some());
        assert_eq!(clip.grains.density, 30.0);
        assert_eq!(clip.grains.pitch_jitter, 12.0);
        assert_eq!(clip.audio_filters.len(), 1);
        assert!(clip.color_filter.is_some());
    }

    #[test]
    fn script_errors_are_reported() {
        let project = fresh();
        let bad = r#"
            let s = session(120.0);
            let t = s.track("g");
            let c = s.clip(t, 99, 0.0, 4.0);
        "#;
        let err = run_script(project, Path::new("/tmp"), bad).unwrap_err();
        assert!(err.contains("no source"), "err: {err}");

        let project = fresh();
        let bad_prop = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 4.0);
            s.set(c, "densitty", 30.0);
        "#;
        let err = run_script(project, Path::new("/tmp"), bad_prop).unwrap_err();
        assert!(err.contains("unknown parameter"), "err: {err}");
    }

    #[test]
    fn script_renders_wav() {
        let dir = std::env::temp_dir().join("chromagrain-script-test");
        std::fs::create_dir_all(&dir).unwrap();
        let project = fresh();
        let script = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 2.0);
            s.set(c, "density", 40.0);
            s.render(0.0, 2.0, "script-out.wav");
        "#;
        let out = run_script(project, &dir, script).unwrap();
        assert!(out.contains("rendered"), "log: {out}");
        let wav = dir.join("script-out.wav");
        assert!(wav.exists());
        let clip = media::load_wav(&wav).unwrap();
        assert!(clip.duration() > 0.9);
    }
}
