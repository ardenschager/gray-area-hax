//! Streaming performance playback: a producer thread runs the core
//! realtime engine (block-rendering the project with persistent effect
//! state), resamples to the device rate, and feeds a lock-free ring
//! buffer that the cpal callback drains. Edits mid-performance rebuild
//! the engine at the current playhead.

use chromagrain_core::realtime::RealtimeAudio;
use chromagrain_core::timeline::Project;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const BLOCK: usize = 1024;
/// Ring capacity in interleaved samples (~90 ms at 48k stereo).
const RING_CAPACITY: usize = 8192;

struct Shared {
    playing: AtomicBool,
    /// Frames consumed by the audio device since play started.
    consumed_frames: AtomicU64,
    /// Set to request the producer rebuild its engine from `new_project`.
    dirty: AtomicBool,
    new_project: Mutex<Option<Project>>,
    loop_region: Mutex<Option<(f64, f64)>>,
}

pub struct Playback {
    stream: Option<cpal::Stream>,
    consumer_slot: Arc<Mutex<Option<rtrb::Consumer<f32>>>>,
    shared: Arc<Shared>,
    device_rate: u32,
    device_channels: usize,
    producer_thread: Option<std::thread::JoinHandle<()>>,
    start_secs: f64,
    project_rate: u32,
    /// Wall-clock fallback so the visual performance still runs when no
    /// audio device exists.
    wall_start: Option<std::time::Instant>,
    pub last_error: Option<String>,
}

impl Playback {
    pub fn new() -> Playback {
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
            new_project: Mutex::new(None),
            loop_region: Mutex::new(None),
        });
        let mut pb = Playback {
            stream: None,
            consumer_slot: Arc::new(Mutex::new(None)),
            shared,
            device_rate: 48000,
            device_channels: 2,
            producer_thread: None,
            start_secs: 0.0,
            project_rate: 48000,
            wall_start: None,
            last_error: None,
        };
        if let Err(e) = pb.init_stream() {
            pb.last_error = Some(e);
        }
        pb
    }

    fn init_stream(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!("unsupported sample format {:?}", config.sample_format()));
        }
        self.device_rate = config.sample_rate().0;
        self.device_channels = config.channels() as usize;
        let channels = self.device_channels;
        let shared = self.shared.clone();
        let consumer_slot = self.consumer_slot.clone();

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    data.fill(0.0);
                    if !shared.playing.load(Ordering::Relaxed) {
                        return;
                    }
                    // try_lock keeps the callback non-blocking; a missed
                    // buffer swap just plays one quiet callback.
                    let Ok(mut guard) = consumer_slot.try_lock() else { return };
                    let Some(cons) = guard.as_mut() else { return };
                    let mut frames = 0u64;
                    for frame in data.chunks_mut(channels) {
                        let (Ok(l), Ok(r)) = (cons.pop(), cons.pop()) else { break };
                        match channels {
                            1 => frame[0] = 0.5 * (l + r),
                            _ => {
                                frame[0] = l;
                                frame[1] = r;
                            }
                        }
                        frames += 1;
                    }
                    shared.consumed_frames.fetch_add(frames, Ordering::Relaxed);
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
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Start performance playback from `start_beat`.
    pub fn play(&mut self, project: Project, start_beat: f64, loop_region: Option<(f64, f64)>) {
        self.stop();
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        *self.consumer_slot.lock().unwrap() = Some(consumer);
        self.shared.consumed_frames.store(0, Ordering::Relaxed);
        self.shared.dirty.store(false, Ordering::Relaxed);
        *self.shared.loop_region.lock().unwrap() = loop_region;
        self.start_secs = project.beats_to_secs(start_beat);
        self.project_rate = project.sample_rate;
        self.wall_start = self.stream.is_none().then(std::time::Instant::now);
        self.shared.playing.store(true, Ordering::Relaxed);

        let shared = self.shared.clone();
        let device_rate = self.device_rate;
        if self.stream.is_some() {
            self.producer_thread = Some(std::thread::spawn(move || {
                producer_loop(project, start_beat, loop_region, device_rate, producer, shared);
            }));
        }
    }

    pub fn stop(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.wall_start = None;
        if let Some(t) = self.producer_thread.take() {
            let _ = t.join();
        }
        *self.consumer_slot.lock().unwrap() = None;
    }

    /// Live-update the performing project (rebuilds the engine at the
    /// current playhead; effect tails reset).
    pub fn update_project(&self, project: Project) {
        if !self.is_playing() {
            return;
        }
        *self.shared.new_project.lock().unwrap() = Some(project);
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn set_loop(&self, loop_region: Option<(f64, f64)>) {
        *self.shared.loop_region.lock().unwrap() = loop_region;
        self.shared.dirty.store(true, Ordering::Relaxed);
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
        self.stop();
    }
}

fn producer_loop(
    project: Project,
    start_beat: f64,
    loop_region: Option<(f64, f64)>,
    device_rate: u32,
    mut producer: rtrb::Producer<f32>,
    shared: Arc<Shared>,
) {
    let mut engine = RealtimeAudio::new(project, start_beat, loop_region);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    // Linear resampler state (project rate -> device rate).
    let ratio = engine.sample_rate() as f64 / device_rate as f64;
    let mut frac = 0.0f64;
    let mut prev = (0.0f32, 0.0f32);
    let mut pending: Vec<f32> = Vec::new();

    while shared.playing.load(Ordering::Relaxed) {
        if shared.dirty.swap(false, Ordering::Relaxed) {
            let new_project = shared.new_project.lock().unwrap().take();
            let loop_now = *shared.loop_region.lock().unwrap();
            if let Some(p) = new_project {
                let beat = engine.playhead_beats();
                engine = RealtimeAudio::new(p, beat, loop_now);
            } else {
                engine.loop_region = loop_now;
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
