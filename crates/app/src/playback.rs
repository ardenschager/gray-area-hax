//! cpal-based playback of a bounced StereoBuffer.

use chromagrain_core::audio::StereoBuffer;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Shared {
    /// Interleaved stereo at the device sample rate.
    samples: Mutex<Arc<Vec<f32>>>,
    pos: AtomicUsize,
    playing: AtomicBool,
}

pub struct Playback {
    stream: Option<cpal::Stream>,
    shared: Arc<Shared>,
    device_rate: u32,
    pub last_error: Option<String>,
}

impl Playback {
    pub fn new() -> Playback {
        let shared = Arc::new(Shared::default());
        let mut pb = Playback {
            stream: None,
            shared,
            device_rate: 48000,
            last_error: None,
        };
        if let Err(e) = pb.init_stream() {
            pb.last_error = Some(e);
        }
        pb
    }

    fn init_stream(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device
            .default_output_config()
            .map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!("unsupported sample format {:?}", config.sample_format()));
        }
        let channels = config.channels() as usize;
        self.device_rate = config.sample_rate().0;
        let shared = self.shared.clone();

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    data.fill(0.0);
                    if !shared.playing.load(Ordering::Relaxed) {
                        return;
                    }
                    let buf = shared.samples.lock().unwrap().clone();
                    let mut pos = shared.pos.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        if pos + 1 >= buf.len() {
                            shared.playing.store(false, Ordering::Relaxed);
                            break;
                        }
                        let l = buf[pos];
                        let r = buf[pos + 1];
                        match channels {
                            1 => frame[0] = 0.5 * (l + r),
                            _ => {
                                frame[0] = l;
                                frame[1] = r;
                            }
                        }
                        pos += 2;
                    }
                    shared.pos.store(pos, Ordering::Relaxed);
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

    /// Load a bounce, resampling to the device rate if needed.
    pub fn set_buffer(&mut self, buffer: &StereoBuffer) {
        let ratio = buffer.sample_rate as f64 / self.device_rate as f64;
        let n_out = (buffer.len() as f64 / ratio) as usize;
        let mut interleaved = Vec::with_capacity(n_out * 2);
        for i in 0..n_out {
            let src = i as f64 * ratio;
            let i0 = src as usize;
            let frac = (src - i0 as f64) as f32;
            let i1 = (i0 + 1).min(buffer.len().saturating_sub(1));
            if i0 >= buffer.len() {
                break;
            }
            let l = buffer.left[i0] * (1.0 - frac) + buffer.left[i1] * frac;
            let r = buffer.right[i0] * (1.0 - frac) + buffer.right[i1] * frac;
            interleaved.push(l);
            interleaved.push(r);
        }
        *self.shared.samples.lock().unwrap() = Arc::new(interleaved);
        self.shared.pos.store(0, Ordering::Relaxed);
    }

    pub fn play_from(&self, secs: f64) {
        let pos = ((secs * self.device_rate as f64) as usize) * 2;
        self.shared.pos.store(pos, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Current playhead in seconds.
    pub fn position_secs(&self) -> f64 {
        self.shared.pos.load(Ordering::Relaxed) as f64 / 2.0 / self.device_rate as f64
    }
}
