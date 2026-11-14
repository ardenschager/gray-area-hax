//! Always-on audio engine: a persistent producer thread runs the core
//! realtime engine and feeds a lock-free ring buffer that the cpal
//! callback drains. The transport can start/stop the arrangement while
//! the engine keeps running, so live MIDI pads sound at any time.

use chromagrain_core::grain::GrainEvent;
use chromagrain_core::realtime::RealtimeAudio;
use chromagrain_core::timeline::Project;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const BLOCK: usize = 1024;
/// Ring capacity in interleaved samples (~90 ms at 48k stereo).
const RING_CAPACITY: usize = 8192;

enum TransportCmd {
    Play { start_beat: f64, loop_region: Option<(f64, f64)> },
    Stop,
}

struct Shared {
    shutdown: AtomicBool,
    transport: AtomicBool,
    /// Frames consumed by the device while the transport ran.
    consumed_frames: AtomicU64,
    dirty: AtomicBool,
    new_project: Mutex<Option<Project>>,
    loop_region: Mutex<Option<(f64, f64)>>,
    transport_cmd: Mutex<Option<TransportCmd>>,
    /// Live hits waiting for the engine (onset filled engine-side):
    /// (bus, source, event).
    live_queue: Mutex<Vec<(u8, usize, GrainEvent)>>,
    /// Live voices to release now (emitter gestures ending).
    releases: Mutex<Vec<u64>>,
    /// Effect chains of the live buses (1-based), re-applied whenever the
    /// engine is rebuilt.
    live_buses: Mutex<Vec<Vec<chromagrain_core::fx::AvEffect>>>,
    buses_dirty: AtomicBool,
    /// Click-free live `ISOL8` isolator punch update (applied without swapping the engine).
    isolator_cmd: Mutex<Option<chromagrain_core::dsp::BandIsolator>>,
    /// Click-free live varispeed `Tape Stop` brake update (applied without swapping the engine).
    tape_stop_cmd: Mutex<Option<f32>>,
    /// One-shot preview sounds (instrument note auditions), mixed in after
    /// the engine at the project rate: (samples, read position).
    auditions: Mutex<Vec<(Arc<Vec<f32>>, usize)>>,
}

/// Send-able handle for firing audition sounds from worker threads.
#[derive(Clone)]
pub struct Auditioner(Arc<Shared>);

impl Auditioner {
    pub fn play(&self, samples: Arc<Vec<f32>>) {
        let mut a = self.0.auditions.lock().unwrap();
        if a.len() >= 6 {
            a.remove(0);
        }
        a.push((samples, 0));
    }
}

pub struct Playback {
    stream: Option<cpal::Stream>,
    shared: Arc<Shared>,
    device_rate: u32,
    producer_thread: Option<std::thread::JoinHandle<()>>,
    start_secs: f64,
    wall_start: Option<std::time::Instant>,
    pub last_error: Option<String>,
    /// Offline (rendering): no thread, no device — the caller pumps the
    /// engine block by block with [`Playback::render_offline`].
    offline: Option<Box<Driver>>,
    /// Offline: how late live hits sound after the frame that fired them
    /// (one frame, so their spacing within the frame survives).
    pub offline_latency: f64,
}

impl Playback {
    pub fn new() -> Playback {
        Playback::build(true, false)
    }

    /// No audio device at all (tests, headless renders): the engine still
    /// runs and the transport clock follows wall time.
    pub fn new_headless() -> Playback {
        Playback::build(false, false)
    }

    /// No device and no engine thread: time only moves when the caller
    /// renders (frame-locked recordings, offline bounces of a live set).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_offline() -> Playback {
        Playback::build(false, true)
    }

    fn build(open_device: bool, offline: bool) -> Playback {
        let shared = Arc::new(Shared {
            shutdown: AtomicBool::new(false),
            transport: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
            new_project: Mutex::new(None),
            loop_region: Mutex::new(None),
            transport_cmd: Mutex::new(None),
            live_queue: Mutex::new(Vec::new()),
            releases: Mutex::new(Vec::new()),
            live_buses: Mutex::new(Vec::new()),
            buses_dirty: AtomicBool::new(false),
            isolator_cmd: Mutex::new(None),
            tape_stop_cmd: Mutex::new(None),
            auditions: Mutex::new(Vec::new()),
        });
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        let mut pb = Playback {
            stream: None,
            shared: shared.clone(),
            device_rate: 48000,
            producer_thread: None,
            start_secs: 0.0,
            wall_start: None,
            last_error: None,
            offline: offline.then(|| Box::new(Driver::new())),
            offline_latency: 1.0 / 30.0,
        };
        if offline {
            return pb;
        }
        if open_device {
            if let Err(e) = pb.init_stream(consumer) {
                pb.last_error = Some(e);
            }
        }
        // The engine runs regardless of the device so pads/visuals work
        // headless too (audio simply has nowhere to go without a stream).
        let device_rate = pb.device_rate;
        pb.producer_thread = Some(std::thread::spawn(move || {
            producer_loop(device_rate, producer, shared);
        }));
        pb
    }

    fn init_stream(&mut self, consumer: rtrb::Consumer<f32>) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!("unsupported sample format {:?}", config.sample_format()));
        }
        self.device_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let shared = self.shared.clone();
        let mut consumer = consumer;

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    data.fill(0.0);
                    let mut frames = 0u64;
                    for frame in data.chunks_mut(channels) {
                        let (Ok(l), Ok(r)) = (consumer.pop(), consumer.pop()) else { break };
                        match channels {
                            1 => frame[0] = 0.5 * (l + r),
                            _ => {
                                frame[0] = l;
                                frame[1] = r;
                            }
                        }
                        frames += 1;
                    }
                    if shared.transport.load(Ordering::Relaxed) {
                        shared.consumed_frames.fetch_add(frames, Ordering::Relaxed);
                    }
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn available(&self) -> bool {
        self.stream.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.shared.transport.load(Ordering::Relaxed)
    }

    /// Start the arrangement transport from `start_beat`.
    pub fn play(&mut self, project: &Project, start_beat: f64, loop_region: Option<(f64, f64)>) {
        self.shared.consumed_frames.store(0, Ordering::Relaxed);
        *self.shared.loop_region.lock().unwrap() = loop_region;
        *self.shared.transport_cmd.lock().unwrap() =
            Some(TransportCmd::Play { start_beat, loop_region });
        self.start_secs = project.beats_to_secs(start_beat);
        self.wall_start = (self.stream.is_none() && self.offline.is_none()).then(std::time::Instant::now);
        self.shared.transport.store(true, Ordering::Relaxed);
    }

    pub fn stop(&mut self) {
        *self.shared.transport_cmd.lock().unwrap() = Some(TransportCmd::Stop);
        self.shared.transport.store(false, Ordering::Relaxed);
        self.wall_start = None;
    }

    /// Hand the engine a fresh project snapshot (edits, any time).
    pub fn set_project(&self, project: Project) {
        *self.shared.new_project.lock().unwrap() = Some(project);
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn set_loop(&self, loop_region: Option<(f64, f64)>) {
        *self.shared.loop_region.lock().unwrap() = loop_region;
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    /// Fire a live pad hit (MIDI note / on-screen pad). The engine stamps
    /// the onset when it picks the event up.
    pub fn trigger(&self, source: usize, mut event: GrainEvent) {
        event.onset = 0.0;
        self.shared.live_queue.lock().unwrap().push((0, source, event));
    }

    /// [`Playback::trigger`] through live effect bus `bus` (0 = dry).
    pub fn trigger_bus(&self, bus: u8, source: usize, mut event: GrainEvent) {
        env_relative(&mut event);
        event.onset = 0.0;
        self.shared.live_queue.lock().unwrap().push((bus, source, event));
    }

    /// [`Playback::trigger_bus`], sounding `delay` seconds after the engine
    /// picks it up (keeps grain spacing when hits arrive once per frame).
    pub fn trigger_bus_in(&self, bus: u8, source: usize, mut event: GrainEvent, delay: f64) {
        env_relative(&mut event);
        event.onset = delay.max(0.0);
        self.shared.live_queue.lock().unwrap().push((bus, source, event));
    }

    /// End a live voice now (its ADSR releases; queued grains learn it).
    pub fn release_voice(&self, voice: u64) {
        self.shared.releases.lock().unwrap().push(voice);
    }

    pub fn is_offline(&self) -> bool {
        self.offline.is_some()
    }

    /// Offline only: render the next `frames` samples (project rate),
    /// applying everything queued since the last call first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn render_offline(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0f32; frames];
        let mut r = vec![0.0f32; frames];
        if let Some(d) = self.offline.as_mut() {
            d.block(&self.shared, &mut l, &mut r);
        }
        (l, r)
    }

    /// Set the effect chain of live bus `bus` (1-based).
    pub fn set_bus(&self, bus: u8, chain: Vec<chromagrain_core::fx::AvEffect>) {
        if bus == 0 {
            return;
        }
        let mut b = self.shared.live_buses.lock().unwrap();
        while b.len() < bus as usize {
            b.push(Vec::new());
        }
        if b[bus as usize - 1] != chain {
            b[bus as usize - 1] = chain;
            self.shared.buses_dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Update the master 5-Band `ISOL8` Frequency Isolator live with 3 ms
    /// click-free smoothing (without rebuilding the audio engine).
    pub fn set_isolator(&self, iso: chromagrain_core::dsp::BandIsolator) {
        *self.shared.isolator_cmd.lock().unwrap() = Some(iso);
    }

    pub fn set_tape_stop(&self, amt: f32) {
        *self.shared.tape_stop_cmd.lock().unwrap() = Some(amt);
    }

    /// Handle for playing one-shot preview sounds (see [`Auditioner`]).
    pub fn auditioner(&self) -> Auditioner {
        Auditioner(self.shared.clone())
    }

    /// Current playhead in project seconds (loop-folded).
    pub fn playhead_secs(&self, project: &Project) -> f64 {
        if let Some(d) = &self.offline {
            if let Some(TransportCmd::Play { .. }) = &*self.shared.transport_cmd.lock().unwrap() {
                return self.start_secs;
            }
            return d.engine.playhead_secs();
        }
        let consumed = match &self.wall_start {
            Some(t0) => t0.elapsed().as_secs_f64(),
            None => {
                self.shared.consumed_frames.load(Ordering::Relaxed) as f64
                    / self.device_rate as f64
            }
        };
        let raw = self.start_secs + consumed;
        if let Some((ls, le)) = *self.shared.loop_region.lock().unwrap() {
            let (ls, le) = (project.beats_to_secs(ls), project.beats_to_secs(le));
            if le > ls && raw >= ls {
                return ls + (raw - ls) % (le - ls);
            }
        }
        raw
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.producer_thread.take() {
            let _ = t.join();
        }
    }
}

/// The engine plus everything it picks up from the UI side between blocks
/// (project swaps, transport commands, bus chains, live hits, auditions).
/// The device thread pumps it; an offline playback pumps it directly.
struct Driver {
    engine: RealtimeAudio,
    /// The project the engine was built from (to rebuild it at a seek).
    project: Project,
    /// The engine being replaced, still playing its arrangement (and
    /// effect tails) for a short crossfade: edits, seeks and stops never
    /// cut the sound. (samples of fade left)
    prev: Option<(RealtimeAudio, usize)>,
}

/// Engine swap crossfade (samples at the project rate).
const SWAP_XFADE: usize = 1024;

impl Driver {
    fn new() -> Driver {
        let mut engine = RealtimeAudio::new(Project::default(), 0.0, None);
        engine.transport = false;
        Driver { engine, project: Project::default(), prev: None }
    }

    /// Replace the engine with `next`, crossfading out of the current one.
    fn swap(&mut self, mut next: RealtimeAudio) {
        next.adopt_live(&mut self.engine);
        let old = std::mem::replace(&mut self.engine, next);
        // An already-fading engine keeps fading (if two swaps arrive in the
        // same block, e.g. set_project + Play, keep the real playing engine).
        if old.transport && self.prev.is_none() {
            self.prev = Some((old, SWAP_XFADE));
        }
    }

    /// Apply what the UI queued since the last block.
    fn apply(&mut self, shared: &Shared) {
        if shared.dirty.swap(false, Ordering::Relaxed) {
            let new_project = shared.new_project.lock().unwrap().take();
            let loop_now = *shared.loop_region.lock().unwrap();
            if let Some(p) = new_project {
                let beat = self.engine.playhead_beats();
                let transport = self.engine.transport;
                self.project = p.clone();
                let mut engine = RealtimeAudio::new(p, beat, loop_now);
                engine.transport = transport;
                self.swap(engine);
                shared.buses_dirty.store(true, Ordering::Relaxed);
            } else {
                self.engine.loop_region = loop_now;
            }
        }
        if let Some(cmd) = shared.transport_cmd.lock().unwrap().take() {
            match cmd {
                TransportCmd::Play { start_beat, loop_region } => {
                    if self.engine.transport {
                        // A jump while playing (scene queue, chain tap).
                        let mut engine = RealtimeAudio::new(self.project.clone(), start_beat, loop_region);
                        engine.transport = true;
                        self.swap(engine);
                    } else {
                        self.engine.seek_beats(start_beat);
                        self.engine.loop_region = loop_region;
                        self.engine.transport = true;
                    }
                }
                TransportCmd::Stop => {
                    let beat = self.engine.playhead_beats();
                    let mut engine = RealtimeAudio::new(self.project.clone(), beat, self.engine.loop_region);
                    engine.transport = false;
                    self.swap(engine);
                }
            }
        }
        if let Some(iso) = shared.isolator_cmd.lock().unwrap().take() {
            self.project.isolator = iso;
            self.engine.set_isolator(iso);
        }
        if let Some(ts) = shared.tape_stop_cmd.lock().unwrap().take() {
            self.project.tape_stop = ts;
            self.engine.set_tape_stop(ts);
        }
        if shared.buses_dirty.swap(false, Ordering::Relaxed) {
            for (i, chain) in shared.live_buses.lock().unwrap().iter().enumerate() {
                self.engine.set_live_bus(i as u8 + 1, chain.clone());
            }
        }
        let mut queue = shared.live_queue.lock().unwrap();
        for (bus, source, mut ev) in queue.drain(..) {
            // The queued onset is a delay from "now" (0 = immediately);
            // the envelope was made relative to it (`env_relative`).
            ev.onset = self.engine.live_now_secs() + ev.onset.max(0.0);
            if let Some(env) = ev.tag.env.as_mut() {
                env.on += ev.onset;
                env.off += ev.onset;
            }
            self.engine.trigger_live_bus(bus, source, ev);
        }
        drop(queue);
        let now = self.engine.live_now_secs();
        for voice in shared.releases.lock().unwrap().drain(..) {
            self.engine.release_voice(voice, now);
        }
    }

    /// One block at the project rate, auditions mixed in.
    fn block(&mut self, shared: &Shared, left: &mut [f32], right: &mut [f32]) {
        self.apply(shared);
        self.engine.next_block(left, right);
        let n = left.len();
        if let Some((old, rest)) = self.prev.as_mut() {
            let (mut ol, mut or) = (vec![0.0f32; n], vec![0.0f32; n]);
            old.next_block(&mut ol, &mut or);
            for i in 0..n {
                let k = rest.saturating_sub(i) as f32 / SWAP_XFADE as f32; // old's weight
                let g = 0.5 - 0.5 * (std::f32::consts::PI * k).cos();
                left[i] = left[i] * (1.0 - g) + ol[i] * g;
                right[i] = right[i] * (1.0 - g) + or[i] * g;
            }
            *rest = rest.saturating_sub(n);
            if *rest == 0 {
                self.prev = None;
            }
        }
        let mut aud = shared.auditions.lock().unwrap();
        for (buf, pos) in aud.iter_mut() {
            let k = n.min(buf.len().saturating_sub(*pos));
            for i in 0..k {
                let v = buf[*pos + i] * 0.7;
                left[i] += v;
                right[i] += v;
            }
            *pos += k;
        }
        aud.retain(|(buf, pos)| *pos < buf.len());
    }
}

fn producer_loop(device_rate: u32, mut producer: rtrb::Producer<f32>, shared: Arc<Shared>) {
    let mut driver = Driver::new();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut frac = 0.0f64;
    let mut prev = (0.0f32, 0.0f32);
    let mut pending: Vec<f32> = Vec::new();

    while !shared.shutdown.load(Ordering::Relaxed) {
        if pending.is_empty() {
            driver.block(&shared, &mut left, &mut right);
            let ratio = driver.engine.sample_rate() as f64 / device_rate as f64;
            // Resample to device rate, interleaved.
            let mut i = 0usize;
            while i < BLOCK {
                while frac < 1.0 {
                    let (l0, r0) = if i == 0 { prev } else { (left[i - 1], right[i - 1]) };
                    let f = frac as f32;
                    pending.push(l0 + (left[i] - l0) * f);
                    pending.push(r0 + (right[i] - r0) * f);
                    frac += ratio;
                }
                frac -= 1.0;
                i += 1;
            }
            prev = (left[BLOCK - 1], right[BLOCK - 1]);
        }

        // Push what fits; sleep briefly when the ring is full.
        let mut pushed = 0usize;
        for s in &pending {
            if producer.push(*s).is_err() {
                break;
            }
            pushed += 1;
        }
        pending.drain(..pushed);
        if !pending.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Live hits are timed on the app clock; the engine re-stamps onsets on
/// its own live clock. Make a voice envelope relative to the grain's onset
/// so the engine can re-anchor it (`env.on/off += onset` at pickup).
fn env_relative(ev: &mut GrainEvent) {
    if let Some(env) = ev.tag.env.as_mut() {
        env.on -= ev.onset;
        env.off -= ev.onset; // INFINITY stays INFINITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromagrain_core::audio::AudioClip;
    use chromagrain_core::envelope::{Adsr, VoiceEnv};
    use chromagrain_core::grain::VoiceTag;
    use chromagrain_core::timeline::Source;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// An emitter grain is timed on the app clock (here: 1000 s in) with a
    /// held envelope; the engine re-stamps it on its own live clock, so the
    /// envelope must be re-anchored — and a release cuts it.
    #[test]
    fn live_envelopes_follow_the_engine_clock_and_release() {
        let mut pb = Playback::new_offline();
        let mut p = Project::default();
        p.add_source(Source { name: "tone".into(), audio: Some(Arc::new(AudioClip::sine(220.0, 4.0, p.sample_rate))), base_hz: 220.0, ..Default::default() });
        pb.set_project(p);
        pb.render_offline(512);
        let app_t = 1000.0;
        let adsr = Adsr { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.02, ..Default::default() };
        let ev = GrainEvent {
            onset: app_t,
            source_pos: 0.5,
            duration: 2.0,
            pitch_ratio: 1.0,
            gain: 0.8,
            pan: 0.0,
            envelope: 0.01,
            env_skew: 0.0,
            reverse: false,
            id: 1,
            tag: VoiceTag {
                voice: 0xE000_0000_0001,
                env: Some(VoiceEnv::held(adsr, app_t)),
                tile: None,
                ..Default::default()
            },
            ..Default::default()
        };
        pb.trigger_bus(0, 0, ev);
        let (l, _) = pb.render_offline(4800);
        assert!(rms(&l[1000..]) > 0.05, "the held grain sounds (envelope re-anchored): {}", rms(&l[1000..]));
        pb.release_voice(0xE000_0000_0001);
        pb.render_offline(4800); // release (20 ms) happens here
        let (l, _) = pb.render_offline(4800);
        assert!(rms(&l) < 1e-3, "released: silent although the grain is 2 s long ({})", rms(&l));
    }
}
