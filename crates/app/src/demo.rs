//! Demo projects: a small builder that loads a handful of clips and lays
//! them out across the whole studio — the Play grid's columns, the
//! Sequence lanes + scenes + song chain, and the Perform pads — at a tempo
//! and key, so opening the app immediately looks and sounds like a piece.
//!
//! `chromagrain --demo` opens [`DemoSpec::night_platform`]: the clips of
//! "Night Platform" (`examples/burial.rhai`, `assets/demo/`) at 133 bpm in
//! F minor. Without the assets (or ffmpeg) it falls back to the procedural
//! `Project::demo()`.

use crate::{build_clip, App, PlayMode, PADS};
use chromagrain_core::fx::{AvEffect, EffectKind};
use chromagrain_core::music::{Key, ScaleKind};
use chromagrain_core::seq::StepPattern;
use chromagrain_core::timeline::{Clip, Source, Track};
use chromagrain_ui::studio::{COLUMNS, EMITTERS, LANES, SCENES};
use std::path::{Path, PathBuf};

/// One clip of a demo: a file in the demo folder, its display name, and
/// its fundamental (0 = detect on load).
#[derive(Debug, Clone)]
pub struct DemoClip {
    pub file: &'static str,
    pub name: &'static str,
    pub base_hz: f32,
}

/// How a lane's hits sound (the row's hit preset).
#[derive(Debug, Clone, Copy)]
pub struct LaneSpec {
    /// Index into [`DemoSpec::clips`].
    pub clip: usize,
    /// Hit length (seconds), level, stereo, envelope (0..1) and its skew
    /// (-1 percussive … 1 swell).
    pub duration: f32,
    pub gain: f32,
    pub pan: f32,
    pub pan_spread: f32,
    pub envelope: f32,
    pub env_skew: f32,
    /// Snap hits to the key (1) or leave them as sampled (0, drums).
    pub in_key: bool,
}

impl LaneSpec {
    /// A drum lane: short, percussive, untouched by the key.
    pub const fn drum(clip: usize, duration: f32, gain: f32, pan: f32) -> LaneSpec {
        LaneSpec { clip, duration, gain, pan, pan_spread: 0.2, envelope: 0.08, env_skew: -0.85, in_key: false }
    }
    /// A tonal lane: softer edges, snapped to the key.
    pub const fn tonal(clip: usize, duration: f32, gain: f32, pan: f32, envelope: f32) -> LaneSpec {
        LaneSpec { clip, duration, gain, pan, pan_spread: 0.15, envelope, env_skew: 0.0, in_key: true }
    }
}

/// One step hit: which slice of the clip (0..1, negative = the step's own
/// position), pitch in semitones, length multiplier, level.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub lane: usize,
    pub step: usize,
    pub slice: f32,
    pub pitch: f32,
    pub length: f32,
    pub gain: f32,
    /// Plays on every `every.0`-th pass of the bar, at pass `every.1`.
    pub every: (u8, u8),
}

pub const fn hit(lane: usize, step: usize) -> Hit {
    Hit { lane, step, slice: -1.0, pitch: 0.0, length: 1.0, gain: 1.0, every: (1, 0) }
}

impl Hit {
    pub const fn slice(mut self, s: f32) -> Hit {
        self.slice = s;
        self
    }
    pub const fn pitch(mut self, st: f32) -> Hit {
        self.pitch = st;
        self
    }
    pub const fn len(mut self, l: f32) -> Hit {
        self.length = l;
        self
    }
    pub const fn gain(mut self, g: f32) -> Hit {
        self.gain = g;
        self
    }
    pub const fn every(mut self, n: u8, at: u8) -> Hit {
        self.every = (n, at);
        self
    }
}

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub hits: Vec<Hit>,
}

/// What a Perform pad holds.
#[derive(Debug, Clone)]
pub enum PadSpec {
    /// The clip straight, looping every `beats`.
    Loop { clip: usize, beats: f64 },
    /// A soft grain cloud of the clip (`position` 0..1), in key, under a
    /// name (None = the clip's).
    Cloud { clip: usize, position: f32, name: Option<&'static str> },
    /// Some lanes of a scene (in this order; the first one is the pad's
    /// picture), looping, under a name.
    Steps { name: &'static str, scene: usize, lanes: Vec<usize> },
    Empty,
}

#[derive(Debug, Clone)]
pub struct DemoSpec {
    pub name: &'static str,
    /// Folder of the clips, relative to the repo / working directory.
    pub dir: &'static str,
    pub bpm: f64,
    pub key: Key,
    pub clips: Vec<DemoClip>,
    /// Play grid columns, left to right (clip indices; at most 7 — the
    /// 8th column stays the "add a clip" slot).
    pub grid: Vec<usize>,
    /// Sequence lanes, top to bottom (at most 7, same reason).
    pub lanes: Vec<LaneSpec>,
    pub swing: f32,
    pub scenes: Vec<SceneSpec>,
    /// Song chain: (scene, bars).
    pub chain: Vec<(usize, u32)>,
    pub pads: Vec<PadSpec>,
    /// Pads already playing when Perform opens (a bed to play over).
    pub pads_on: Vec<usize>,
    /// Emitters: (grid column, row 0..8, GRAIN/PITCH/SMEAR/LOOK).
    pub emitters: [(usize, f32, [f32; 4]); EMITTERS],
    /// Master reverb mix (a little room glues the layers).
    pub master_room: f32,
    /// Canvas size (the clips are small; a larger canvas keeps grain
    /// edges and feathering crisp on a big output).
    pub canvas: (u32, u32),
}

impl DemoSpec {
    /// "Night Platform": rain on glass, fountain chords, two wordless
    /// voices on a subway platform, a wet highway sub, a 1941 tunnel —
    /// 2-step at 133 bpm in F minor.
    pub fn night_platform() -> DemoSpec {
        // Clips: the first seven are the Play grid.
        const KEYS: usize = 0;
        const BENCH: usize = 1;
        const PLATFORM: usize = 2;
        const RAIN: usize = 3;
        const HIGHWAY: usize = 4;
        const TUNNEL: usize = 5;
        const GLITTER: usize = 6;
        const KICK: usize = 7;
        const RIM: usize = 8;
        let clips = vec![
            DemoClip { file: "night_chords.mp4", name: "fountain keys", base_hz: 174.614 },
            DemoClip { file: "vocal_bench.mp4", name: "bench", base_hz: 391.995 },
            DemoClip { file: "vocal_street.mp4", name: "platform", base_hz: 466.164 },
            DemoClip { file: "rain_window.mp4", name: "rain", base_hz: 0.0 },
            DemoClip { file: "sub_highway.mp4", name: "highway", base_hz: 87.307 },
            DemoClip { file: "subway_tunnel.mp4", name: "tunnel", base_hz: 0.0 },
            DemoClip { file: "hat_glitter.mp4", name: "glitter", base_hz: 0.0 },
            DemoClip { file: "kick_tunnel.mp4", name: "kick", base_hz: 0.0 },
            DemoClip { file: "rim_train.mp4", name: "rim", base_hz: 0.0 },
        ];
        // Lanes: kick, rim, hats, sub, keys, two voices.
        let (l_kick, l_rim, l_hat, l_sub, l_keys, l_bench, l_plat) = (0, 1, 2, 3, 4, 5, 6);
        let lanes = vec![
            LaneSpec::drum(KICK, 0.30, 1.0, 0.0),
            LaneSpec::drum(RIM, 0.30, 0.7, -0.18),
            LaneSpec { pan_spread: 0.7, ..LaneSpec::drum(GLITTER, 0.09, 0.3, 0.0) },
            LaneSpec { in_key: false, envelope: 0.16, ..LaneSpec::tonal(HIGHWAY, 1.15, 0.78, 0.0, 0.16) },
            LaneSpec { in_key: false, ..LaneSpec::tonal(KEYS, 1.9, 0.32, 0.0, 0.42) },
            LaneSpec::tonal(BENCH, 0.46, 0.6, -0.14, 0.32),
            LaneSpec::tonal(PLATFORM, 0.44, 0.55, 0.18, 0.34),
        ];
        let hats = |gain: f32| [2usize, 6, 10, 14].map(|s| hit(l_hat, s).gain(gain));
        // 1 · rain: keys, a ghost rim, one sigh.
        let mut s1 = vec![
            hit(l_keys, 0).slice(0.02).gain(1.5),
            hit(l_keys, 8).slice(0.30).gain(1.2),
            hit(l_rim, 12).gain(0.6),
        ];
        s1.push(hit(l_bench, 3).slice(0.03).pitch(3.0));
        // 2 · groove: 2-step kick, rims on 2 and 4, offbeat hats, the sub.
        let mut s2 = vec![
            hit(l_kick, 0),
            hit(l_kick, 10),
            hit(l_rim, 4),
            hit(l_rim, 12),
            hit(l_sub, 0).slice(0.02),
            hit(l_sub, 10).slice(0.02).pitch(-2.0).len(0.6),
            hit(l_keys, 0).slice(0.02),
            hit(l_keys, 8).slice(0.30).gain(0.8),
        ];
        s2.extend(hats(0.8));
        s2.push(hit(l_hat, 15).gain(0.4));
        // 3 · platform: the voices answer each other over the groove.
        // Phrases alternate bar by bar (pass conditions), so the voices
        // never loop the same chop twice in a row.
        let mut s3 = s2.clone();
        s3.retain(|h| h.lane != l_keys);
        s3.extend([
            hit(l_keys, 0).slice(0.60).gain(0.7),
            hit(l_keys, 8).slice(0.80).gain(0.55),
            hit(l_bench, 3).slice(0.03).pitch(3.0).every(2, 0),
            hit(l_bench, 6).slice(0.14).pitch(-2.0).gain(0.8).every(2, 1),
            hit(l_bench, 13).slice(0.35).pitch(-5.0).len(1.2).gain(0.7).every(4, 3),
            hit(l_plat, 11).slice(0.35).pitch(-2.0).len(1.15).every(2, 0),
            hit(l_plat, 9).slice(0.03).pitch(-4.0).len(1.1).gain(0.8).every(4, 1),
            hit(l_kick, 7).gain(0.55),
        ]);
        // The pad bank, one row per role: beds, step layers, clouds, whole
        // scenes. Every pad has its own name and picture.
        let steps = |name, scene, lanes: &[usize]| PadSpec::Steps { name, scene, lanes: lanes.to_vec() };
        let pads = vec![
            PadSpec::Loop { clip: KEYS, beats: 16.0 },
            PadSpec::Loop { clip: RAIN, beats: 16.0 },
            PadSpec::Loop { clip: TUNNEL, beats: 16.0 },
            PadSpec::Loop { clip: HIGHWAY, beats: 8.0 },
            steps("2-step", 1, &[l_kick, l_rim, l_hat]),
            steps("sub", 1, &[l_sub]),
            steps("voices", 2, &[l_bench, l_plat]),
            steps("chords", 2, &[l_keys]),
            PadSpec::Cloud { clip: BENCH, position: 0.03, name: None },
            PadSpec::Cloud { clip: PLATFORM, position: 0.35, name: None },
            PadSpec::Cloud { clip: GLITTER, position: 0.3, name: None },
            PadSpec::Cloud { clip: TUNNEL, position: 0.5, name: Some("tunnel haze") },
            steps("drop", 2, &[l_bench, l_kick, l_rim, l_hat, l_sub, l_keys, l_plat]),
            steps("intro", 0, &[l_keys, l_rim, l_hat, l_bench]),
            steps("ghost rim", 0, &[l_rim, l_hat]),
            PadSpec::Empty,
        ];
        DemoSpec {
            name: "Night Platform",
            dir: "assets/demo",
            bpm: 133.0,
            key: Key::new(5, ScaleKind::Minor),
            clips,
            grid: vec![KEYS, BENCH, PLATFORM, RAIN, HIGHWAY, TUNNEL, GLITTER],
            lanes,
            swing: 0.60,
            scenes: vec![SceneSpec { hits: s1 }, SceneSpec { hits: s2 }, SceneSpec { hits: s3 }],
            chain: vec![(0, 4), (1, 4), (2, 4)],
            pads,
            pads_on: vec![0],
            emitters: [
                (0, 2.6, [0.22, 0.5, 0.3, 0.0]),
                (1, 0.6, [0.35, 0.5, 0.45, 0.0]),
                (3, 4.4, [0.6, 0.5, 0.2, 0.0]),
                (2, 3.0, [0.3, 0.5, 0.4, 0.0]),
            ],
            master_room: 0.09,
            canvas: (960, 540),
        }
    }

    /// The demo folder: `$CHROMAGRAIN_DEMO_DIR`, the working directory,
    /// next to the executable (and its parents), or the source tree.
    pub fn find_dir(&self) -> Option<PathBuf> {
        let mut cands: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("CHROMAGRAIN_DEMO_DIR") {
            cands.push(PathBuf::from(d));
        }
        if let Ok(cwd) = std::env::current_dir() {
            cands.push(cwd.join(self.dir));
        }
        if let Ok(exe) = std::env::current_exe() {
            for a in exe.ancestors().skip(1).take(5) {
                cands.push(a.join(self.dir));
            }
        }
        cands.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(self.dir));
        let first = self.clips.first()?.file;
        cands.into_iter().find(|d| d.join(first).is_file())
    }

    /// Decode every clip (ffmpeg) from `dir`.
    pub fn load(&self, dir: &Path, sample_rate: u32) -> Result<Vec<Source>, String> {
        self.clips
            .iter()
            .map(|c| {
                let mut s = chromagrain_core::media::load_source(&dir.join(c.file), sample_rate).map_err(|e| format!("{}: {e}", c.file))?;
                s.name = c.name.into();
                if c.base_hz > 0.0 {
                    s.base_hz = c.base_hz;
                }
                Ok(s)
            })
            .collect()
    }

    /// Find and decode the clips, if the folder and ffmpeg are there.
    pub fn load_found(&self, sample_rate: u32) -> Result<Vec<Source>, String> {
        if !chromagrain_core::media::ffmpeg_available() {
            return Err("ffmpeg not found".into());
        }
        let dir = self.find_dir().ok_or_else(|| format!("{} not found", self.dir))?;
        self.load(&dir, sample_rate)
    }
}

impl App {
    /// Replace the document with a demo: `sources` are the spec's clips
    /// (in order), laid out on the grid, lanes, scenes and pads.
    pub(crate) fn install_demo(&mut self, spec: &DemoSpec, sources: Vec<Source>) {
        assert_eq!(sources.len(), spec.clips.len(), "one source per demo clip");
        let key = spec.key;
        let mut scene_pats = [None; SCENES];
        let mut pads = Vec::new();
        {
            let mut p = self.project.lock().unwrap();
            *p = chromagrain_core::timeline::Project { bpm: spec.bpm, width: spec.canvas.0, height: spec.canvas.1, ..Default::default() };
            for s in sources {
                p.add_source(s);
            }
            if spec.master_room > 0.0 {
                p.master_effects.push(AvEffect { video: 0.0, ..AvEffect::new(EffectKind::Reverb { size: 0.56, damp: 0.44, mix: spec.master_room }) });
            }
            // One pattern per scene; lane i is row i, as the studio expects.
            for (si, scene) in spec.scenes.iter().enumerate().take(SCENES) {
                let mut pat = StepPattern::new(&format!("scene {}", si + 1));
                pat.swing = spec.swing;
                for lane in 0..LANES {
                    let r = match spec.lanes.get(lane) {
                        Some(l) => {
                            let r = pat.add_row(l.clip);
                            let g = &mut pat.rows[r].grains;
                            g.duration = l.duration;
                            g.gain = l.gain;
                            g.pan = l.pan;
                            g.pan_spread = l.pan_spread;
                            g.envelope = l.envelope;
                            g.env_skew = l.env_skew;
                            g.quantize_amount = if l.in_key { 1.0 } else { 0.0 };
                            r
                        }
                        None => pat.add_row(usize::MAX),
                    };
                    pat.rows[r].key = Some(key);
                }
                for h in &scene.hits {
                    if let Some(st) = pat.rows.get_mut(h.lane).and_then(|r| r.steps.get_mut(h.step)) {
                        st.on = true;
                        // No slice: the row's own position (drums: the hit's start).
                        st.slice = h.slice;
                        st.every_n = h.every.0.max(1);
                        st.every_offset = h.every.1;
                        st.pitch = h.pitch;
                        st.length = h.length;
                        st.gain = h.gain;
                    }
                }
                scene_pats[si] = Some(p.add_pattern(pat));
            }
            // Pads: one track each, muted unless playing at the start.
            for (i, pad) in spec.pads.iter().chain(std::iter::repeat(&PadSpec::Empty)).take(PADS).enumerate() {
                let mut t = Track { name: format!("pad {}", i + 1), muted: !spec.pads_on.contains(&i), ..Default::default() };
                match pad.clone() {
                    PadSpec::Loop { clip, beats } => {
                        let mut c = build_clip(&mut p, clip, 0.0, PlayMode::Snippet);
                        c.length_beats = beats;
                        c.grains.gain = 0.4;
                        t.clips.push(c);
                    }
                    PadSpec::Cloud { clip, position, name } => {
                        if let Some(n) = name {
                            t.name = n.into();
                        }
                        let mut c = Clip::new(clip, 0.0, 8.0);
                        c.key = Some(key);
                        let g = &mut c.grains;
                        g.position = position;
                        g.density = 7.0;
                        g.duration = 0.55;
                        g.spray = 0.35;
                        g.scan_speed = 0.25;
                        g.gain = 0.34;
                        g.envelope = 0.5;
                        g.pan_spread = 0.5;
                        g.quantize_amount = 1.0;
                        // Soft, glowing patches rather than hard blocks.
                        c.visual.feather = 0.55;
                        c.visual.additive = 0.45;
                        c.visual.max_size = 0.55;
                        t.clips.push(c);
                    }
                    PadSpec::Steps { name, scene, lanes } => {
                        if let Some(pid) = scene_pats.get(scene).copied().flatten() {
                            let mut pat = p.patterns[pid].clone();
                            pat.name = name.into();
                            pat.rows = lanes.iter().filter_map(|l| pat.rows.get(*l).cloned()).collect();
                            t.name = name.into();
                            let own = p.add_pattern(pat);
                            let mut c = Clip::new_pattern(own, 0.0, 4.0);
                            c.visual.feather = 0.3;
                            c.visual.additive = 0.35;
                            t.clips.push(c);
                        }
                    }
                    PadSpec::Empty => {}
                }
                pads.push(t);
            }
        }
        // The studio: grid, lanes, scenes, chain, emitters, key.
        let st = &mut self.studio;
        st.key = key;
        st.columns = [None; COLUMNS];
        for (c, clip) in spec.grid.iter().take(COLUMNS - 1).enumerate() {
            st.columns[c] = Some(*clip);
        }
        st.seq.lanes = [None; LANES];
        for (i, l) in spec.lanes.iter().take(LANES - 1).enumerate() {
            st.seq.lanes[i] = Some(l.clip);
        }
        st.seq.scene_pats = scene_pats;
        st.seq.scene = 0;
        st.seq.queued = None;
        st.seq.chain = spec.chain.clone();
        for (e, (col, row, macros)) in st.emitters.iter_mut().zip(spec.emitters) {
            e.col = col;
            e.row = row;
            e.macros = macros;
        }
        st.sel_emitter = 0;
        st.rev += 1;
        self.other_tracks = pads;
        self.status = format!("{} — press space to play", spec.name);
        self.mark_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppOpts;
    use chromagrain_core::timeline::ClipKind;
    use std::sync::Arc;

    fn stand_ins(spec: &DemoSpec) -> Vec<Source> {
        spec.clips
            .iter()
            .enumerate()
            .map(|(i, c)| Source {
                name: c.name.into(),
                audio: Some(Arc::new(chromagrain_core::audio::AudioClip::saw_stack(110.0 + i as f32 * 20.0, 4.0, 48000))),
                video: Some(Arc::new(chromagrain_core::video::VideoClip::test_pattern(64, 36, 12.0, 4.0))),
                base_hz: c.base_hz,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn night_platform_is_well_formed() {
        let spec = DemoSpec::night_platform();
        assert_eq!(spec.bpm, 133.0);
        assert_eq!(spec.key, Key::new(5, ScaleKind::Minor), "F minor");
        assert!(spec.grid.len() < COLUMNS && spec.lanes.len() < LANES, "the last column / lane stays free");
        let n = spec.clips.len();
        assert!(spec.grid.iter().all(|c| *c < n) && spec.lanes.iter().all(|l| l.clip < n));
        for s in &spec.scenes {
            assert!(s.hits.iter().all(|h| h.lane < spec.lanes.len() && h.step < chromagrain_ui::studio::STEPS), "hits land on real lanes and steps");
        }
        assert!(spec.chain.iter().all(|(s, n)| *s < spec.scenes.len() && *n > 0));
        assert_eq!(spec.pads.len(), PADS);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(spec.dir);
        if dir.exists() {
            for c in &spec.clips {
                assert!(dir.join(c.file).is_file(), "{} is in {}", c.file, spec.dir);
            }
        }
    }

    #[test]
    fn installing_a_demo_fills_grid_lanes_scenes_and_pads() {
        let spec = DemoSpec::night_platform();
        let mut app = App::new(AppOpts { demo: false, classic: false, headless: true });
        app.install_demo(&spec, stand_ins(&spec));
        {
            let p = app.project.lock().unwrap();
            assert_eq!(p.bpm, 133.0);
            assert_eq!(p.sources.len(), spec.clips.len());
            assert_eq!(p.sources[0].name, "fountain keys");
            assert_eq!(p.sources[1].base_hz, 391.995, "known fundamentals are kept");
        }
        let st = &app.studio;
        assert_eq!(st.key, spec.key);
        assert_eq!(st.columns[..7].iter().flatten().count(), 7, "seven clips on the grid");
        assert_eq!(st.columns[7], None, "the add slot stays");
        assert_eq!(st.seq.lanes[0], Some(7), "lane 1 is the kick");
        assert_eq!(st.seq.lanes[7], None);
        assert_eq!(st.seq.chain, spec.chain);
        let pid = st.seq.scene_pats[1].expect("scene 2");
        {
            let p = app.project.lock().unwrap();
            let pat = &p.patterns[pid];
            assert_eq!(pat.rows.len(), LANES);
            assert!(pat.rows[0].steps[0].on && pat.rows[0].steps[10].on, "2-step kick");
            assert_eq!(pat.rows[0].grains.quantize_amount, 0.0, "drums stay as sampled");
            assert_eq!(pat.rows[5].grains.quantize_amount, 1.0, "voices snap to the key");
            assert!(pat.rows.iter().all(|r| r.key == Some(spec.key)));
        }
        assert_eq!(app.other_tracks.len(), PADS, "the pad bank waits for Perform");
        assert!(!app.other_tracks[0].muted, "a bed is playing");
        assert!(app.other_tracks[4].muted);
        let names: Vec<&str> = app.other_tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(&names[4..8], ["2-step", "sub", "voices", "chords"], "step pads are named");
        {
            let p = app.project.lock().unwrap();
            let ClipKind::Pattern(pid) = app.other_tracks[4].clips[0].kind else { panic!("2-step is a pattern pad") };
            let srcs: Vec<usize> = p.patterns[pid].rows.iter().map(|r| r.source).collect();
            assert_eq!(srcs, [7, 8, 6], "kick, rim, hats");
        }
        assert!(app.other_tracks[15].clips.is_empty(), "the last pad is the add slot");
    }

    #[test]
    fn the_demo_plays_from_the_studio() {
        // The installed demo survives the studio's first frames (sync,
        // seq_ensure, the arrangement) and turns into a looping song.
        let spec = DemoSpec::night_platform();
        let mut app = App::new(AppOpts { demo: false, classic: false, headless: true });
        app.install_demo(&spec, stand_ins(&spec));
        app.with_studio(|st, h| {
            st.sync_columns(h);
            st.seq_ensure(h);
            st.seq_build_arrangement(h);
        });
        assert_eq!(app.studio.columns[7], None, "kick and rim stay off the grid");
        let p = app.project.lock().unwrap();
        let seq = p.tracks.iter().find(|t| t.name == "sequence").expect("sequence track");
        assert_eq!(seq.clips.len(), 3);
        drop(p);
        assert!(app.loop_on && (app.loop_end - 48.0).abs() < 1e-9, "4 + 4 + 4 bars, looping");
    }

    #[test]
    fn night_platform_decodes_when_ffmpeg_is_here() {
        let spec = DemoSpec::night_platform();
        let Ok(sources) = spec.load_found(48000) else { return };
        assert_eq!(sources.len(), spec.clips.len());
        assert!(sources.iter().all(|s| s.audio.is_some() && s.video.is_some()));
        assert!(sources.iter().all(|s| s.duration() > 0.5));
    }
}

