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
    /// Live pad hits waiting for the engine (onset filled engine-side).
    live_queue: Mutex<Vec<(usize, GrainEvent)>>,
}

pub struct Playback {
    stream: Option<cpal::Stream>,
    shared: Arc<Shared>,
    device_rate: u32,
    producer_thread: Option<std::thread::JoinHandle<()>>,
    start_secs: f64,
    wall_start: Option<std::time::Instant>,
    pub last_error: Option<String>,
}

impl Playback {
    pub fn new() -> Playback {
        let shared = Arc::new(Shared {
            shutdown: AtomicBool::new(false),
            transport: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
            new_project: Mutex::new(None),
            loop_region: Mutex::new(None),
            transport_cmd: Mutex::new(None),
            live_queue: Mutex::new(Vec::new()),
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
        };
        if let Err(e) = pb.init_stream(consumer) {
            pb.last_error = Some(e);
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
        self.wall_start = self.stream.is_none().then(std::time::Instant::now);
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
    pub fn trigger(&self, source: usize, event: GrainEvent) {
        self.shared.live_queue.lock().unwrap().push((source, event));
    }

    /// Current playhead in project seconds (loop-folded).
    pub fn playhead_secs(&self, project: &Project) -> f64 {
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

fn producer_loop(device_rate: u32, mut producer: rtrb::Producer<f32>, shared: Arc<Shared>) {
    let mut engine = RealtimeAudio::new(Project::default(), 0.0, None);
    engine.transport = false;
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut ratio = engine.sample_rate() as f64 / device_rate as f64;
    let mut frac = 0.0f64;
    let mut prev = (0.0f32, 0.0f32);
    let mut pending: Vec<f32> = Vec::new();

    while !shared.shutdown.load(Ordering::Relaxed) {
        if shared.dirty.swap(false, Ordering::Relaxed) {
            let new_project = shared.new_project.lock().unwrap().take();
            let loop_now = *shared.loop_region.lock().unwrap();
            if let Some(p) = new_project {
                let beat = engine.playhead_beats();
                let transport = engine.transport;
                engine = RealtimeAudio::new(p, beat, loop_now);
                engine.transport = transport;
                ratio = engine.sample_rate() as f64 / device_rate as f64;
            } else {
                engine.loop_region = loop_now;
            }
        }
        if let Some(cmd) = shared.transport_cmd.lock().unwrap().take() {
            match cmd {
                TransportCmd::Play { start_beat, loop_region } => {
                    engine.seek_beats(start_beat);
                    engine.loop_region = loop_region;
                    engine.transport = true;
                }
                TransportCmd::Stop => engine.transport = false,
            }
        }
        {
            let mut queue = shared.live_queue.lock().unwrap();
            for (source, mut ev) in queue.drain(..) {
                ev.onset = engine.live_now_secs();
                engine.trigger_live(source, ev);
            }
        }

        if pending.is_empty() {
            engine.next_block(&mut left, &mut right);
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
