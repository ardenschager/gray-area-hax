//! Host-independent audio side of the plugin, plus the loader thread.
//!
//! Threads:
//! * **audio** owns [`AudioSide`] (the [`Instrument`]). It never allocates,
//!   frees or locks: new media and key changes arrive as values over a
//!   wait-free SPSC ring ([`ToAudio`]); whatever they replace goes back
//!   over a second ring ([`Trash`]) to be dropped elsewhere.
//! * **loader** (one per plugin instance) decodes files through
//!   `chromagrain_core::media` (so the active `MediaBackend`: ffmpeg on the
//!   desktop, AVFoundation on iOS), prepares key changes
//!   ([`KeyFx::prepare`]) when the key/scale params move, and frees trash.
//! * **GUI / host main thread** sends [`Job`]s to the loader, and
//!   time-critical, allocation-free messages ([`Live`]: emitter grains,
//!   pad launches) straight to the audio thread over a third SPSC ring, so
//!   a tap sounds in the next block the host renders.
//!
//! Everything here is plain Rust with no nih-plug process context, so the
//! tests drive exactly the code the host does.

use crate::params::CgParams;
use chromagrain_core::instrument::{GrainOrigin, Instrument, KeyFx, Macro, PadSlot, Transport, MAX_SOURCES, PADS};
use chromagrain_core::music::Key;
use chromagrain_core::grain::GrainEvent;
use chromagrain_core::seq::StepPattern;
use chromagrain_core::timeline::Source;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Longest sub-block handed to the instrument (it reserves 8192).
const MAX_SUB_BLOCK: usize = 4096;
const RING: usize = 64;
/// Editor messages per block, with lots of room (four emitters at a few
/// hundred grains/s, pads).
const LIVE_RING: usize = 1024;
/// Grains per editor frame, with room (16 pad loops + notes).
const GRAIN_RING: usize = 4096;

/// Loader -> audio.
pub enum ToAudio {
    /// Add (and select) a decoded source.
    Add(Source),
    /// Install key-dependent look state.
    Key(KeyFx),
    /// New step lanes from the editor's Sequence view.
    Pattern(Box<StepPattern>),
    /// The same media with new slices (the old one goes to the trash).
    Replace(usize, Source),
}

/// Editor -> audio, direct (no loader hop). Plain values: pushing and
/// popping never allocates.
#[derive(Debug, Clone)]
pub enum Live {
    /// A one-shot grain (an emitter on the Play grid), played at the
    /// start of the next block.
    Fire(usize, GrainEvent),
    /// Load / clear perform pad n.
    Pad(usize, Option<PadSlot>),
    /// Start / stop pad n on the next launch-grid line.
    TogglePad(usize),
    StopPads,
}

/// Audio -> loader: replaced things, to be dropped off the audio thread.
pub enum Trash {
    Source(Source),
    Key(KeyFx),
    Pattern(Box<StepPattern>),
}

/// Work for the loader thread.
pub enum Job {
    /// Decode a media file (and remember its path in the plugin state).
    Load(PathBuf),
    /// Already-decoded media (a host with its own decoder, or tests).
    Decoded(Box<Source>),
    /// Editor -> audio (via this thread: patterns allocate, and what they
    /// replace is freed here).
    Pattern(Box<StepPattern>),
    /// A source's chops changed in the editor.
    Slices(usize, Vec<chromagrain_core::chop::Slice>),
    Stop,
}

/// What the loader, the GUI and the plugin share. The audio thread only
/// touches the atomics.
pub struct Shared {
    jobs: mpsc::Sender<Job>,
    /// The editor's end of the [`Live`] ring. The lock only orders
    /// editor-side callers (the audio thread never takes it).
    live: Mutex<rtrb::Producer<Live>>,
    /// Output sample rate (decode target, key-state rate).
    pub sample_rate: AtomicU32,
    /// Paths decoded so far this session, in load order.
    pub loaded: Mutex<Vec<String>>,
    /// One line for the editor ("loading x…", errors).
    pub status: Mutex<String>,
    /// Grains on screen right now (written by the audio thread).
    pub visible_grains: AtomicUsize,
    /// Sources the instrument holds (written by the audio thread).
    pub sources: AtomicUsize,
    /// Decoded media, in load order, for the editor's thumbnails and
    /// canvas (Arc'd: no second copy of the frames).
    pub ui_sources: Mutex<Vec<Source>>,
    /// Host transport as the audio thread last saw it.
    pub playing: AtomicBool,
    pub beat_bits: AtomicU64,
    pub bpm_bits: AtomicU64,
    /// Instrument clock (seconds) at the end of the last block.
    pub clock_bits: AtomicU64,
    /// Pads as the audio thread last saw them: bit n = on, 16+n = a launch
    /// is queued, 32+n = the queued launch starts the pad.
    pub pad_bits: AtomicU64,
    /// Grid beat each pad's loop started on.
    pub pad_beats: [AtomicU64; PADS],
    /// The pads' contents as the editor set them (the audio thread has its
    /// own copy; this one is for drawing).
    pub pad_slots: Mutex<[Option<PadSlot>; PADS]>,
    /// Grains the instrument made from notes and pads (audio -> editor,
    /// for the canvas; one-shots and step lanes the editor draws itself):
    /// (source, grain, from a pad?).
    pub grains: Mutex<rtrb::Consumer<(usize, GrainEvent, bool)>>,
}

impl Shared {
    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Straight to the audio thread; false if the ring is full (the audio
    /// thread is not running) and the message was dropped.
    pub fn live(&self, msg: Live) -> bool {
        self.live.lock().map(|mut q| q.push(msg).is_ok()).unwrap_or(false)
    }

    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat_bits.load(Ordering::Relaxed))
    }
    pub fn bpm(&self) -> f64 {
        f64::from_bits(self.bpm_bits.load(Ordering::Relaxed))
    }
    pub fn clock(&self) -> f64 {
        f64::from_bits(self.clock_bits.load(Ordering::Relaxed))
    }
    /// (on, queued start/stop) for pad n.
    pub fn pad_state(&self, n: usize) -> (bool, Option<bool>) {
        let b = self.pad_bits.load(Ordering::Relaxed);
        let bit = |k: usize| b >> k & 1 == 1;
        (bit(n), bit(16 + n).then(|| bit(32 + n)))
    }
    pub fn pad_beat(&self, n: usize) -> f64 {
        self.pad_beats.get(n).map_or(0.0, |b| f64::from_bits(b.load(Ordering::Relaxed)))
    }

    /// Keep the editor's copy in step with the instrument (which recycles
    /// its newest slot when full).
    fn mirror_source(&self, src: &Source) {
        if let Ok(mut u) = self.ui_sources.lock() {
            if u.len() >= MAX_SOURCES {
                if let Some(last) = u.last_mut() {
                    *last = src.clone();
                }
            } else {
                u.push(src.clone());
            }
        }
    }

    pub fn set_status(&self, s: String) {
        if let Ok(mut st) = self.status.lock() {
            *st = s;
        }
    }
}

/// A note event, sample-timed within the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev {
    On { timing: u32, note: u8, velocity: f32 },
    Off { timing: u32, note: u8 },
}

impl Ev {
    fn timing(&self) -> u32 {
        match *self {
            Ev::On { timing, .. } | Ev::Off { timing, .. } => timing,
        }
    }
}

/// The audio-thread half.
pub struct AudioSide {
    pub inst: Instrument,
    rx: rtrb::Consumer<ToAudio>,
    live: rtrb::Consumer<Live>,
    trash: rtrb::Producer<Trash>,
    grains: rtrb::Producer<(usize, GrainEvent, bool)>,
    shared: Arc<Shared>,
}

/// Build the audio side, the shared state, and start the loader thread.
pub fn spawn(params: Arc<CgParams>, sample_rate: u32) -> (AudioSide, Arc<Shared>) {
    let (to_audio, rx) = rtrb::RingBuffer::new(RING);
    let (trash, trash_rx) = rtrb::RingBuffer::new(RING);
    let (live, live_rx) = rtrb::RingBuffer::new(LIVE_RING);
    let (grains, grains_rx) = rtrb::RingBuffer::new(GRAIN_RING);
    let (jobs, job_rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        jobs,
        live: Mutex::new(live),
        sample_rate: AtomicU32::new(sample_rate),
        loaded: Mutex::new(Vec::new()),
        status: Mutex::new(String::from("no video loaded")),
        visible_grains: AtomicUsize::new(0),
        sources: AtomicUsize::new(0),
        ui_sources: Mutex::new(Vec::new()),
        playing: AtomicBool::new(false),
        beat_bits: AtomicU64::new(0f64.to_bits()),
        bpm_bits: AtomicU64::new(120f64.to_bits()),
        clock_bits: AtomicU64::new(0f64.to_bits()),
        pad_bits: AtomicU64::new(0),
        pad_beats: std::array::from_fn(|_| AtomicU64::new(0f64.to_bits())),
        pad_slots: Mutex::new([None; PADS]),
        grains: Mutex::new(grains_rx),
    });
    let loader = Loader { params, shared: shared.clone(), jobs: job_rx, to_audio, trash: trash_rx, key: None, key_rate: sample_rate, pending: Vec::new() };
    std::thread::Builder::new()
        .name("chromagrain-loader".into())
        .spawn(move || loader.run())
        .expect("spawn loader thread");
    let mut inst = Instrument::new(sample_rate);
    inst.set_key(None);
    (AudioSide { inst, rx, live: live_rx, trash, grains, shared: shared.clone() }, shared)
}

impl AudioSide {
    /// (Re)start at `sample_rate` (plugin activation; not the audio thread).
    pub fn reset(&mut self, sample_rate: u32, params: &CgParams) {
        self.shared.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.inst.reset(sample_rate);
        self.inst.set_key(params.key());
        self.apply_params(params);
        // Restore the pads (a session load: the sources reload in the
        // same order, so the indices hold).
        let saved = params.pads.lock().map(|p| p.clone()).unwrap_or_default();
        let extra = params.pad_extra.lock().map(|p| p.clone()).unwrap_or_default();
        let mut slots = [None; PADS];
        for (pad, source, start, len) in saved {
            if pad < PADS {
                let (slice, adsr) = extra.iter().find(|e| e.0 == pad).map(|e| (e.1, e.2)).unwrap_or((None, None));
                slots[pad] = Some(PadSlot {
                    start,
                    slice: slice.map(|(a, b)| chromagrain_core::chop::Slice::new(a, b)),
                    adsr,
                    ..PadSlot::new(source, len)
                });
            }
        }
        for (i, s) in slots.iter().enumerate() {
            self.inst.set_pad(i, *s);
        }
        if let Ok(mut m) = self.shared.pad_slots.lock() {
            *m = slots;
        }
    }

    /// Macros, sync and follow from the params (cheap; every block).
    fn apply_params(&mut self, params: &CgParams) {
        let inst = &mut self.inst;
        inst.set_macro(Macro::Grain, params.grain.value());
        inst.set_macro(Macro::Pitch, params.pitch.value());
        inst.set_macro(Macro::Smear, params.smear.value());
        inst.set_macro(Macro::Look, params.look.value());
        inst.set_sync(params.sync.value().beats());
        inst.set_follow_transport(params.follow.value());
        inst.set_launch_quantize(params.launch.value().beats());
        inst.set_layout(params.layout.value().layout());
        inst.set_layout_pan(params.layout_pan.value());
    }

    /// Latency the instrument reports right now (the current LOOK).
    pub fn latency_samples(&self) -> u32 {
        self.inst.latency_samples() as u32
    }

    /// Render one host block: `out` is `[left, right]`. `transport` is the
    /// host's (None when it reports no tempo). Events must come in timing
    /// order. Allocation- and lock-free.
    pub fn render(&mut self, out: &mut [&mut [f32]], params: &CgParams, transport: Option<Transport>, next_event: &mut dyn FnMut() -> Option<Ev>) {
        // Transport and params first: queued pad launches aim at this
        // block's beat grid.
        if let Some(t) = transport {
            self.inst.set_transport(t);
        }
        self.apply_params(params);
        self.drain();
        let inst = &mut self.inst;

        let (l, r) = match out {
            [l, r, ..] => (l, r),
            [m] => {
                m.fill(0.0);
                return;
            }
            [] => return,
        };
        let n = l.len().min(r.len());
        let mut pos = 0;
        let mut next = next_event();
        while pos < n {
            while let Some(ev) = next {
                if ev.timing() as usize > pos {
                    break;
                }
                apply(inst, ev);
                next = next_event();
            }
            let until = next.map(|e| e.timing() as usize).unwrap_or(n);
            let end = until.min(n).min(pos + MAX_SUB_BLOCK).max(pos + 1);
            inst.process(&mut l[pos..end], &mut r[pos..end]);
            pos = end;
        }
        // Events stamped past the block (shouldn't happen) still count.
        while let Some(ev) = next {
            apply(inst, ev);
            next = next_event();
        }
        for i in 0..n {
            let g = params.gain.smoothed.next();
            l[i] *= g;
            r[i] *= g;
        }
        let now = inst.now_secs();
        self.shared.visible_grains.store(inst.video_frame_events(now).count(), Ordering::Relaxed);
        self.shared.sources.store(inst.sources().len(), Ordering::Relaxed);
        let t = inst.transport();
        self.shared.playing.store(t.playing, Ordering::Relaxed);
        self.shared.beat_bits.store(t.beat.to_bits(), Ordering::Relaxed);
        self.shared.bpm_bits.store(t.bpm.to_bits(), Ordering::Relaxed);
        self.shared.clock_bits.store(now.to_bits(), Ordering::Relaxed);
        let mut bits = 0u64;
        for (n, p) in inst.pads().iter().enumerate() {
            bits |= (p.on as u64) << n | (p.queued.is_some() as u64) << (16 + n) | ((p.queued == Some(true)) as u64) << (32 + n);
            self.shared.pad_beats[n].store(p.start_beat.to_bits(), Ordering::Relaxed);
        }
        self.shared.pad_bits.store(bits, Ordering::Relaxed);
        let ring = &mut self.grains;
        inst.drain_new_grains(|src, ev, origin| {
            if matches!(origin, GrainOrigin::Note | GrainOrigin::Pad(_)) {
                // Full = no editor reading: drop (the picture is optional).
                let _ = ring.push((src, ev.clone(), matches!(origin, GrainOrigin::Pad(_))));
            }
        });
    }

    /// Take everything the loader has sent.
    fn drain(&mut self) {
        while let Ok(msg) = self.rx.pop() {
            let old = match msg {
                ToAudio::Add(src) => {
                    let n = self.inst.sources().len();
                    if n < MAX_SOURCES {
                        let id = self.inst.add_source(src);
                        self.inst.select_source(id);
                        None
                    } else {
                        // Full: the newest slot is recycled.
                        let id = n - 1;
                        self.inst.select_source(id);
                        Some(Trash::Source(self.inst.swap_source(id, src).unwrap_or_else(|s| s)))
                    }
                }
                ToAudio::Key(fx) => Some(Trash::Key(self.inst.swap_key(fx))),
                ToAudio::Pattern(p) => {
                    let old = self.inst.swap_pattern(Some(*p));
                    old.map(|o| Trash::Pattern(Box::new(o)))
                }
                ToAudio::Replace(id, src) => Some(Trash::Source(self.inst.swap_source(id, src).unwrap_or_else(|s| s))),
            };
            if let Some(t) = old {
                // The trash ring only fills if the loader is stuck; then
                // freeing here is the lesser evil.
                let _ = self.trash.push(t);
            }
        }
        // After the loader's messages, so a grain on a just-added source
        // finds it.
        while let Ok(msg) = self.live.pop() {
            match msg {
                Live::Fire(src, mut ev) => {
                    // Re-anchor the editor's (relative) envelope: the grain
                    // starts now on the instrument clock.
                    let now = self.inst.now_secs();
                    if let Some(env) = ev.tag.env.as_mut() {
                        env.on += now;
                        env.off += now;
                    }
                    self.inst.trigger(src, ev)
                }
                Live::Pad(n, slot) => self.inst.set_pad(n, slot),
                Live::TogglePad(n) => self.inst.toggle_pad(n),
                Live::StopPads => self.inst.stop_pads(),
            }
        }
    }
}

fn apply(inst: &mut Instrument, ev: Ev) {
    match ev {
        Ev::On { note, velocity, .. } => inst.note_on(note, velocity),
        Ev::Off { note, .. } => inst.note_off(note),
    }
}

struct Loader {
    params: Arc<CgParams>,
    shared: Arc<Shared>,
    jobs: mpsc::Receiver<Job>,
    to_audio: rtrb::Producer<ToAudio>,
    trash: rtrb::Consumer<Trash>,
    /// Key (and rate) the audio side has, or will have once `pending` lands.
    key: Option<Key>,
    key_rate: u32,
    /// Messages waiting for room in the ring.
    pending: Vec<ToAudio>,
}

impl Loader {
    fn run(mut self) {
        loop {
            match self.jobs.recv_timeout(Duration::from_millis(15)) {
                Ok(Job::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Job::Load(path)) => self.load(path),
                Ok(Job::Decoded(src)) => {
                    let mut src = *src;
                    self.restore_slices(&mut src);
                    self.shared.mirror_source(&src);
                    self.pending.push(ToAudio::Add(src));
                }
                Ok(Job::Slices(id, v)) => self.slices(id, v),
                Ok(Job::Pattern(p)) => {
                    // Only the newest pattern matters.
                    self.pending.retain(|m| !matches!(m, ToAudio::Pattern(_)));
                    self.pending.push(ToAudio::Pattern(p));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.watch_key();
            while !self.pending.is_empty() && self.to_audio.slots() > 0 {
                let msg = self.pending.remove(0);
                let _ = self.to_audio.push(msg);
            }
            while let Ok(t) = self.trash.pop() {
                drop(t);
            }
        }
    }

    fn load(&mut self, path: PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.shared.set_status(format!("loading {name}…"));
        let sr = self.shared.sample_rate.load(Ordering::Relaxed);
        match chromagrain_core::media::load_source(&path, sr) {
            Ok(mut src) => {
                self.restore_slices(&mut src);
                let p = path.to_string_lossy().into_owned();
                if let Ok(mut l) = self.shared.loaded.lock() {
                    l.push(p.clone());
                }
                if let Ok(mut persisted) = self.params.sources.lock() {
                    if !persisted.contains(&p) {
                        persisted.push(p);
                    }
                }
                self.shared.set_status(format!("{name} ({:.1} s)", src.duration()));
                self.shared.mirror_source(&src);
                self.pending.push(ToAudio::Add(src));
            }
            Err(e) => self.shared.set_status(format!("{name}: {e}")),
        }
    }

    /// Index the next mirrored source gets.
    fn next_index(&self) -> usize {
        self.shared.ui_sources.lock().map(|u| u.len()).unwrap_or(0)
    }

    /// Chops saved with the session for the source about to be added.
    fn restore_slices(&self, src: &mut Source) {
        let k = self.next_index();
        let saved = self.params.slices.lock().ok().and_then(|s| s.iter().find(|e| e.0 == k).map(|e| e.1.clone()));
        if let Some(v) = saved {
            src.slices = v.into_iter().map(|(a, b)| chromagrain_core::chop::Slice::new(a, b)).collect();
            let len = src.duration();
            chromagrain_core::chop::normalize(&mut src.slices, len);
        }
    }

    /// New chops from the editor: swap in the same media with them, and
    /// keep them in the plugin state.
    fn slices(&mut self, id: usize, v: Vec<chromagrain_core::chop::Slice>) {
        let src = self.shared.ui_sources.lock().ok().and_then(|mut u| {
            let s = u.get_mut(id)?;
            s.slices = v.clone();
            Some(s.clone())
        });
        let Some(src) = src else { return };
        if let Ok(mut saved) = self.params.slices.lock() {
            saved.retain(|e| e.0 != id);
            if !v.is_empty() {
                saved.push((id, v.iter().map(|s| (s.start, s.end)).collect()));
            }
        }
        self.pending.retain(|m| !matches!(m, ToAudio::Replace(k, _) if *k == id));
        self.pending.push(ToAudio::Replace(id, src));
    }

    /// Prepare look state when the key/scale params or the rate change.
    fn watch_key(&mut self) {
        let want = self.params.key();
        let sr = self.shared.sample_rate.load(Ordering::Relaxed);
        if want != self.key || sr != self.key_rate {
            self.key = want;
            self.key_rate = sr;
            self.pending.push(ToAudio::Key(KeyFx::prepare(want, sr)));
        }
    }
}
