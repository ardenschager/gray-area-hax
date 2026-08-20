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
use crate::fx::{AvEffect, EffectKind};
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

/// Shared grain-parameter setter (clips and sequencer rows). Returns
/// false when the key is not a grain parameter.
fn set_grain_param(g: &mut crate::grain::GrainSettings, key: &str, value: f64) -> bool {
    g.set_param(key, value).is_ok()
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
            audio: Some(std::sync::Arc::new(AudioClip::saw_stack(110.0, 4.0, sr))),
            video: Some(std::sync::Arc::new(VideoClip::test_pattern(240, 136, 12.0, 4.0))),
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
                audio: Some(std::sync::Arc::new(AudioClip::sine(freq as f32, secs as f32, sr))),
                video: None,
                base_hz: freq as f32,
            };
            p.add_source(src) as i64
        },
    );

    engine.register_fn(
        "fm_source",
        |s: &mut Session, base_hz: f64, ratio: f64, index: f64, secs: f64| -> i64 {
            let mut p = s.project.lock().unwrap();
            let sr = p.sample_rate;
            let src = crate::synth::fm_source(
                (base_hz as f32).clamp(20.0, 4000.0),
                (ratio as f32).clamp(0.01, 16.0),
                (index as f32).clamp(0.0, 10.0),
                (secs as f32).clamp(0.1, 30.0),
                sr,
            );
            p.add_source(src) as i64
        },
    );

    engine.register_fn(
        "noise_source",
        |s: &mut Session, color: f64, secs: f64| -> i64 {
            let mut p = s.project.lock().unwrap();
            let sr = p.sample_rate;
            let src = crate::synth::noise_source(
                (color as f32).clamp(0.0, 1.0),
                (secs as f32).clamp(0.1, 30.0),
                sr,
            );
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

    fn push_clip(
        s: &mut Session,
        track: i64,
        source: i64,
        clip: Clip,
    ) -> ScriptResult<ClipRef> {
        let mut p = s.project.lock().unwrap();
        if source as usize >= p.sources.len() {
            return Err(rt_err(format!("no source {source}")));
        }
        let t = p
            .tracks
            .get_mut(track as usize)
            .ok_or_else(|| rt_err(format!("no track {track}")))?;
        t.clips.push(clip);
        Ok(ClipRef { track, index: (t.clips.len() - 1) as i64 })
    }

    engine.register_fn(
        "clip",
        |s: &mut Session,
         track: i64,
         source: i64,
         start_beat: f64,
         length_beats: f64|
         -> ScriptResult<ClipRef> {
            let c = Clip::new(source as usize, start_beat, length_beats.max(0.001));
            push_clip(s, track, source, c)
        },
    );

    // A snippet: the source played straight as ONE long grain, keeping the
    // full AV correspondence (gain->opacity, pitch->rate/hue, pan->x).
    engine.register_fn(
        "snippet",
        |s: &mut Session,
         track: i64,
         source: i64,
         start_beat: f64,
         length_beats: f64|
         -> ScriptResult<ClipRef> {
            let c = Clip::new_snippet(source as usize, start_beat, length_beats.max(0.001));
            push_clip(s, track, source, c)
        },
    );

    // Property-style setter for grain + visual parameters.
    engine.register_fn(
        "set",
        |s: &mut Session, c: ClipRef, prop: &str, value: f64| -> ScriptResult<()> {
            let key = prop.to_ascii_lowercase().replace([' ', '-'], "_");
            s.with_clip(c, |clip| -> Result<(), String> {
                if set_grain_param(&mut clip.grains, &key, value) {
                    return Ok(());
                }
                let v = &mut clip.visual;
                let x = value as f32;
                match key.as_str() {
                    "size_scale" => v.size_scale = x.max(0.0),
                    "min_size" => v.min_size = x.clamp(0.01, 1.0),
                    "max_size" => v.max_size = x.clamp(0.01, 1.0),
                    "additive" => v.additive = x.clamp(0.0, 1.0),
                    "scatter_y" => v.scatter_y = x.clamp(0.0, 1.0),
                    // Correspondence dials (gain_to_opacity, pitch_to_hue,
                    // pitch_to_rate, pan_to_x, envelope_to_opacity,
                    // reverse_video, hue_per_semitone alias).
                    other => {
                        return clip
                            .link
                            .set_param(other, x)
                            .map_err(|_| format!("unknown parameter '{other}'"))
                    }
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

    // ---------------------------------------------------- tracks (mixer)
    engine.register_fn("track_level", |s: &mut Session, track: i64, level: f64| {
        if let Some(t) = s.project.lock().unwrap().tracks.get_mut(track as usize) {
            t.level = (level as f32).clamp(0.0, 2.0);
        }
    });
    engine.register_fn(
        "track_opacity_link",
        |s: &mut Session, track: i64, v: f64| {
            if let Some(t) = s.project.lock().unwrap().tracks.get_mut(track as usize) {
                t.level_to_opacity = (v as f32).clamp(0.0, 1.0);
            }
        },
    );
    engine.register_fn("track_mute", |s: &mut Session, track: i64, mute: bool| {
        if let Some(t) = s.project.lock().unwrap().tracks.get_mut(track as usize) {
            t.muted = mute;
        }
    });
    engine.register_fn(
        "track_effect",
        |s: &mut Session, track: i64, kind: &str| -> ScriptResult<i64> {
            let kind = EffectKind::parse(kind)
                .ok_or_else(|| rt_err(format!("unknown effect '{kind}'")))?;
            let mut p = s.project.lock().unwrap();
            let t = p
                .tracks
                .get_mut(track as usize)
                .ok_or_else(|| rt_err(format!("no track {track}")))?;
            t.effects.push(AvEffect::new(kind));
            Ok((t.effects.len() - 1) as i64)
        },
    );
    engine.register_fn(
        "track_fx",
        |s: &mut Session, track: i64, idx: i64, param: &str, value: f64| -> ScriptResult<()> {
            let mut p = s.project.lock().unwrap();
            let fx = p
                .tracks
                .get_mut(track as usize)
                .and_then(|t| t.effects.get_mut(idx as usize))
                .ok_or_else(|| rt_err(format!("no effect {idx} on track {track}")))?;
            fx.set_param(param, value as f32).map_err(rt_err)
        },
    );

    // ------------------------------------------------- step sequencer
    engine.register_fn("quantize", |s: &mut Session, div: f64| {
        s.project.lock().unwrap().quantize = div.clamp(0.0625, 4.0);
    });
    engine.register_fn("pattern", |s: &mut Session, name: &str| -> i64 {
        s.project
            .lock()
            .unwrap()
            .add_pattern(crate::seq::StepPattern::new(name)) as i64
    });
    engine.register_fn(
        "pattern_grid",
        |s: &mut Session, pid: i64, length_beats: f64, steps_per_beat: i64| -> ScriptResult<()> {
            let mut p = s.project.lock().unwrap();
            let pat = p
                .patterns
                .get_mut(pid as usize)
                .ok_or_else(|| rt_err(format!("no pattern {pid}")))?;
            pat.set_grid(length_beats, steps_per_beat as u32);
            Ok(())
        },
    );
    engine.register_fn(
        "row",
        |s: &mut Session, pid: i64, source: i64| -> ScriptResult<i64> {
            let mut p = s.project.lock().unwrap();
            if source as usize >= p.sources.len() {
                return Err(rt_err(format!("no source {source}")));
            }
            let pat = p
                .patterns
                .get_mut(pid as usize)
                .ok_or_else(|| rt_err(format!("no pattern {pid}")))?;
            Ok(pat.add_row(source as usize) as i64)
        },
    );
    fn with_row<T>(
        s: &mut Session,
        pid: i64,
        row: i64,
        f: impl FnOnce(&mut crate::seq::SeqRow) -> T,
    ) -> ScriptResult<T> {
        let mut p = s.project.lock().unwrap();
        let r = p
            .patterns
            .get_mut(pid as usize)
            .and_then(|pat| pat.rows.get_mut(row as usize))
            .ok_or_else(|| rt_err(format!("no pattern {pid} row {row}")))?;
        Ok(f(r))
    }
    engine.register_fn(
        "row_set",
        |s: &mut Session, pid: i64, row: i64, prop: &str, value: f64| -> ScriptResult<()> {
            let key = prop.to_ascii_lowercase().replace([' ', '-'], "_");
            with_row(s, pid, row, |r| set_grain_param(&mut r.grains, &key, value))?
                .then_some(())
                .ok_or_else(|| rt_err(format!("unknown row parameter '{prop}'")))
        },
    );
    engine.register_fn(
        "row_key",
        |s: &mut Session, pid: i64, row: i64, root: &str, scale: &str| -> ScriptResult<()> {
            let key = Key::parse(root, scale)
                .ok_or_else(|| rt_err(format!("bad key: {root} {scale}")))?;
            with_row(s, pid, row, |r| r.key = Some(key))
        },
    );
    engine.register_fn(
        "step",
        |s: &mut Session, pid: i64, row: i64, idx: i64, on: bool| -> ScriptResult<()> {
            with_row(s, pid, row, |r| -> Result<(), String> {
                let st = r
                    .steps
                    .get_mut(idx as usize)
                    .ok_or_else(|| format!("no step {idx}"))?;
                st.on = on;
                Ok(())
            })?
            .map_err(rt_err)
        },
    );
    engine.register_fn(
        "step_pitch",
        |s: &mut Session, pid: i64, row: i64, idx: i64, semitones: f64| -> ScriptResult<()> {
            with_row(s, pid, row, |r| -> Result<(), String> {
                let st = r
                    .steps
                    .get_mut(idx as usize)
                    .ok_or_else(|| format!("no step {idx}"))?;
                st.pitch = (semitones as f32).clamp(-48.0, 48.0);
                Ok(())
            })?
            .map_err(rt_err)
        },
    );
    engine.register_fn(
        "step_gain",
        |s: &mut Session, pid: i64, row: i64, idx: i64, gain: f64| -> ScriptResult<()> {
            with_row(s, pid, row, |r| -> Result<(), String> {
                let st = r
                    .steps
                    .get_mut(idx as usize)
                    .ok_or_else(|| format!("no step {idx}"))?;
                st.gain = (gain as f32).clamp(0.0, 2.0);
                Ok(())
            })?
            .map_err(rt_err)
        },
    );
    engine.register_fn(
        "pattern_clip",
        |s: &mut Session,
         track: i64,
         pattern: i64,
         start_beat: f64,
         length_beats: f64|
         -> ScriptResult<ClipRef> {
            let mut p = s.project.lock().unwrap();
            if pattern as usize >= p.patterns.len() {
                return Err(rt_err(format!("no pattern {pattern}")));
            }
            let t = p
                .tracks
                .get_mut(track as usize)
                .ok_or_else(|| rt_err(format!("no track {track}")))?;
            t.clips.push(Clip::new_pattern(
                pattern as usize,
                start_beat,
                length_beats.max(0.001),
            ));
            Ok(ClipRef { track, index: (t.clips.len() - 1) as i64 })
        },
    );

    // ---------------------------------------------------- canvas placement
    engine.register_fn(
        "transform",
        |s: &mut Session, c: ClipRef, x: f64, y: f64, scale: f64, rotation: f64| -> ScriptResult<()> {
            s.with_clip(c, |clip| {
                clip.transform = crate::timeline::ClipTransform {
                    x: (x as f32).clamp(-2.0, 2.0),
                    y: (y as f32).clamp(-2.0, 2.0),
                    scale: (scale as f32).clamp(0.05, 4.0),
                    rotation: rotation as f32,
                };
            })
        },
    );

    // --------------------------------------------------------- automation
    engine.register_fn(
        "automate",
        |s: &mut Session, c: ClipRef, param: &str, beat: f64, value: f64| -> ScriptResult<()> {
            let key = param.to_ascii_lowercase().replace([' ', '-'], "_");
            if crate::auto::param_range(&key).is_none() {
                return Err(rt_err(format!("'{param}' is not automatable")));
            }
            s.with_clip(c, |clip| {
                match clip.automation.iter_mut().find(|l| l.param == key) {
                    Some(lane) => lane.set_point(beat, value),
                    None => {
                        let mut lane = crate::auto::AutomationLane::new(&key);
                        lane.set_point(beat, value);
                        clip.automation.push(lane);
                    }
                }
            })
        },
    );
    engine.register_fn(
        "clear_automation",
        |s: &mut Session, c: ClipRef, param: &str| -> ScriptResult<()> {
            let key = param.to_ascii_lowercase().replace([' ', '-'], "_");
            s.with_clip(c, |clip| clip.automation.retain(|l| l.param != key))
        },
    );
    engine.register_fn(
        "track_level_point",
        |s: &mut Session, track: i64, beat: f64, value: f64| -> ScriptResult<()> {
            let mut p = s.project.lock().unwrap();
            let t = p
                .tracks
                .get_mut(track as usize)
                .ok_or_else(|| rt_err(format!("no track {track}")))?;
            let mut lane = crate::auto::AutomationLane {
                param: String::new(),
                points: std::mem::take(&mut t.level_points),
            };
            lane.set_point(beat, value.clamp(0.0, 2.0));
            t.level_points = lane.points;
            Ok(())
        },
    );
    engine.register_fn("clear_track_level", |s: &mut Session, track: i64| {
        if let Some(t) = s.project.lock().unwrap().tracks.get_mut(track as usize) {
            t.level_points.clear();
        }
    });

    // -------------------------------------------- row modes / polymeter
    engine.register_fn(
        "row_steps",
        |s: &mut Session, pid: i64, row: i64, n: i64| -> ScriptResult<()> {
            with_row(s, pid, row, |r| r.set_steps(n.max(1) as usize))
        },
    );
    engine.register_fn(
        "row_loop",
        |s: &mut Session, pid: i64, row: i64, start: f64, end: f64| -> ScriptResult<()> {
            with_row(s, pid, row, |r| {
                r.mode = crate::seq::RowMode::Loop;
                r.loop_start = (start as f32).clamp(0.0, 1.0);
                r.loop_end = (end as f32).clamp(0.0, 1.0).max(r.loop_start + 0.01);
            })
        },
    );
    engine.register_fn(
        "row_steps_mode",
        |s: &mut Session, pid: i64, row: i64| -> ScriptResult<()> {
            with_row(s, pid, row, |r| r.mode = crate::seq::RowMode::Steps)
        },
    );

    // ------------------------------------------------------------ effects
    // Chains: s.effect(c, "crush") -> index; s.fx(c, idx, "bits", 4.0);
    // master: s.master_effect("reverb") -> idx; s.master_fx(idx, "mix", 0.4).
    engine.register_fn(
        "effect",
        |s: &mut Session, c: ClipRef, kind: &str| -> ScriptResult<i64> {
            let kind = EffectKind::parse(kind)
                .ok_or_else(|| rt_err(format!("unknown effect '{kind}'")))?;
            s.with_clip(c, |clip| {
                clip.effects.push(AvEffect::new(kind));
                (clip.effects.len() - 1) as i64
            })
        },
    );

    engine.register_fn(
        "fx",
        |s: &mut Session, c: ClipRef, idx: i64, param: &str, value: f64| -> ScriptResult<()> {
            s.with_clip(c, |clip| -> Result<(), String> {
                let fx = clip
                    .effects
                    .get_mut(idx as usize)
                    .ok_or_else(|| format!("no effect {idx} on clip"))?;
                fx.set_param(param, value as f32)
            })?
            .map_err(rt_err)
        },
    );

    engine.register_fn("clear_effects", |s: &mut Session, c: ClipRef| -> ScriptResult<()> {
        s.with_clip(c, |clip| clip.effects.clear())
    });

    engine.register_fn(
        "master_effect",
        |s: &mut Session, kind: &str| -> ScriptResult<i64> {
            let kind = EffectKind::parse(kind)
                .ok_or_else(|| rt_err(format!("unknown effect '{kind}'")))?;
            let mut p = s.project.lock().unwrap();
            p.master_effects.push(AvEffect::new(kind));
            Ok((p.master_effects.len() - 1) as i64)
        },
    );

    engine.register_fn(
        "master_fx",
        |s: &mut Session, idx: i64, param: &str, value: f64| -> ScriptResult<()> {
            let mut p = s.project.lock().unwrap();
            let fx = p
                .master_effects
                .get_mut(idx as usize)
                .ok_or_else(|| rt_err(format!("no master effect {idx}")))?;
            fx.set_param(param, value as f32).map_err(rt_err)
        },
    );

    engine.register_fn("clear_master_effects", |s: &mut Session| {
        s.project.lock().unwrap().master_effects.clear();
    });

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
    fn script_snippets_effects_and_links() {
        let project = fresh();
        let script = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.snippet(t, src, 0.0, 4.0);
            s.set(c, "gain", 0.5);
            s.set(c, "pan", -0.5);
            s.set(c, "gain_to_opacity", 0.3);
            s.set(c, "pitch_to_hue", -18.0);
            let cr = s.effect(c, "crush");
            s.fx(c, cr, "bits", 4.0);
            s.fx(c, cr, "video", 0.7);
            let dl = s.effect(c, "delay");
            s.fx(c, dl, "time", 0.4);
            let mr = s.master_effect("reverb");
            s.master_fx(mr, "mix", 0.5);
        "#;
        run_script(project.clone(), Path::new("/tmp"), script).unwrap();

        let p = project.lock().unwrap();
        let clip = &p.tracks[0].clips[0];
        assert_eq!(clip.kind, crate::timeline::ClipKind::Snippet);
        assert_eq!(clip.grains.gain, 0.5);
        assert_eq!(clip.grains.pan, -0.5);
        assert_eq!(clip.link.gain_to_opacity, 0.3);
        assert_eq!(clip.link.pitch_to_hue, -18.0);
        assert_eq!(clip.effects.len(), 2);
        assert!(matches!(
            clip.effects[0].kind,
            crate::fx::EffectKind::Crush { bits, .. } if bits == 4.0
        ));
        assert_eq!(clip.effects[0].video, 0.7);
        assert!(matches!(
            clip.effects[1].kind,
            crate::fx::EffectKind::Delay { time, .. } if (time - 0.4).abs() < 1e-6
        ));
        assert_eq!(p.master_effects.len(), 1);
        assert!(matches!(
            p.master_effects[0].kind,
            crate::fx::EffectKind::Reverb { mix, .. } if mix == 0.5
        ));

        // Bad effect index and unknown effect error cleanly.
        drop(p);
        let bad = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 4.0);
            s.fx(c, 5, "time", 0.4);
        "#;
        let err = run_script(fresh(), Path::new("/tmp"), bad).unwrap_err();
        assert!(err.contains("no effect"), "err: {err}");
    }

    #[test]
    fn script_tracks_and_patterns() {
        let project = fresh();
        let script = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("seq");
            s.track_level(t, 0.7);
            s.track_opacity_link(t, 0.5);
            let d = s.track_effect(t, "delay");
            s.track_fx(t, d, "time", 0.25);
            s.quantize(0.5);

            let p = s.pattern("hits");
            s.pattern_grid(p, 4.0, 4);
            let r = s.row(p, src);
            s.row_set(p, r, "duration", 0.3);
            s.row_key(p, r, "A", "minor_pentatonic");
            s.step(p, r, 0, true);
            s.step(p, r, 8, true);
            s.step_pitch(p, r, 8, 12.0);
            s.step_gain(p, r, 8, 0.5);
            let c = s.pattern_clip(t, p, 0.0, 8.0);
        "#;
        run_script(project.clone(), Path::new("/tmp"), script).unwrap();

        let p = project.lock().unwrap();
        assert_eq!(p.quantize, 0.5);
        let t = &p.tracks[0];
        assert_eq!(t.level, 0.7);
        assert_eq!(t.level_to_opacity, 0.5);
        assert_eq!(t.effects.len(), 1);
        let pat = &p.patterns[0];
        assert_eq!(pat.rows.len(), 1);
        assert!(pat.rows[0].steps[0].on && pat.rows[0].steps[8].on);
        assert_eq!(pat.rows[0].steps[8].pitch, 12.0);
        assert_eq!(pat.rows[0].steps[8].gain, 0.5);
        assert!(pat.rows[0].key.is_some());
        assert!(matches!(
            t.clips[0].kind,
            crate::timeline::ClipKind::Pattern(0)
        ));
    }

    #[test]
    fn script_automation_and_row_modes() {
        let project = fresh();
        let script = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 8.0);
            s.automate(c, "density", 0.0, 5.0);
            s.automate(c, "density", 8.0, 60.0);
            s.automate(c, "gain", 4.0, 0.5);
            s.track_level_point(t, 0.0, 1.0);
            s.track_level_point(t, 8.0, 0.2);

            let p = s.pattern("poly");
            let r = s.row(p, src);
            s.row_steps(p, r, 5);
            s.step(p, r, 0, true);
            let l = s.row(p, src);
            s.row_loop(p, l, 0.25, 0.75);
        "#;
        run_script(project.clone(), Path::new("/tmp"), script).unwrap();

        let p = project.lock().unwrap();
        let clip = &p.tracks[0].clips[0];
        assert_eq!(clip.automation.len(), 2);
        let dens = clip.automation.iter().find(|l| l.param == "density").unwrap();
        assert_eq!(dens.points.len(), 2);
        assert!((dens.value_at(4.0).unwrap() - 32.5).abs() < 1e-9);
        assert_eq!(p.tracks[0].level_points.len(), 2);
        assert!((p.tracks[0].level_at(4.0) - 0.6).abs() < 1e-6);

        let pat = &p.patterns[0];
        assert_eq!(pat.rows[0].steps.len(), 5);
        assert_eq!(pat.rows[1].mode, crate::seq::RowMode::Loop);
        assert!((pat.rows[1].loop_start - 0.25).abs() < 1e-6);

        // Bad param errors cleanly.
        drop(p);
        let bad = r#"
            let s = session(120.0);
            let src = s.demo_source();
            let t = s.track("g");
            let c = s.clip(t, src, 0.0, 8.0);
            s.automate(c, "seed", 0.0, 5.0);
        "#;
        let err = run_script(fresh(), Path::new("/tmp"), bad).unwrap_err();
        assert!(err.contains("not automatable"), "err: {err}");
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
