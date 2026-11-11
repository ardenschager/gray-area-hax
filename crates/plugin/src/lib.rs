//! chromagrain as a VST3 / CLAP instrument (nih-plug).
//!
//! MIDI notes in, stereo audio out: a thin wrapper over
//! `chromagrain_core::instrument::Instrument`. Host tempo/transport feeds
//! `set_transport`; the GRAIN / PITCH / SMEAR / LOOK macros, key, scale,
//! sync and gain are automatable parameters; loaded media persists in the
//! plugin state as file paths and is decoded again on a background thread
//! (see [`engine`]) and handed to the audio thread lock-free. `process`
//! never allocates or locks (enforced in debug builds by nih-plug's
//! `assert_process_allocs`).

pub mod editor;
pub mod engine;
pub mod params;

use chromagrain_core::instrument::Transport;
use engine::{AudioSide, Ev, Job, Shared};
use nih_plug::prelude::*;
use params::CgParams;
use std::num::NonZeroU32;
use std::sync::Arc;

pub struct Chromagrain {
    params: Arc<CgParams>,
    audio: AudioSide,
    shared: Arc<Shared>,
    reported_latency: u32,
}

impl Default for Chromagrain {
    fn default() -> Self {
        let params = Arc::new(CgParams::default());
        let (audio, shared) = engine::spawn(params.clone(), 48_000);
        Self { params, audio, shared, reported_latency: 0 }
    }
}

impl Drop for Chromagrain {
    fn drop(&mut self) {
        self.shared.send(Job::Stop);
    }
}

impl Chromagrain {
    /// Activation at `sample_rate` (what `initialize` does): resets the
    /// instrument and queues decodes for persisted media not loaded yet
    /// (a restored session). Returns the latency to report.
    pub fn setup(&mut self, sample_rate: u32) -> u32 {
        self.audio.reset(sample_rate, self.params.key());
        // Hosts' wrappers also do this; offline use (tests) relies on it.
        self.params.gain.smoothed.reset(self.params.gain.value());
        let persisted = self.params.sources.lock().map(|s| s.clone()).unwrap_or_default();
        let loaded = self.shared.loaded.lock().map(|s| s.clone()).unwrap_or_default();
        for p in persisted.into_iter().filter(|p| !loaded.contains(p)) {
            self.shared.send(Job::Load(p.into()));
        }
        self.reported_latency = self.audio.latency_samples();
        self.reported_latency
    }

    /// One block, host-independent (what `process` does after reading the
    /// host's events and transport).
    pub fn render(&mut self, out: &mut [&mut [f32]], transport: Option<Transport>, next_event: &mut dyn FnMut() -> Option<Ev>) {
        self.audio.render(out, &self.params, transport, next_event);
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub fn params_ref(&self) -> &Arc<CgParams> {
        &self.params
    }

    pub fn audio(&self) -> &AudioSide {
        &self.audio
    }
}

impl Plugin for Chromagrain {
    const NAME: &'static str = "chromagrain";
    const VENDOR: &'static str = "Creative Lab";
    const URL: &'static str = "https://github.com/ardenschager/gray-area-hax";
    const EMAIL: &'static str = "ardenschager@google.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] =
        &[AudioIOLayout { main_input_channels: None, main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, buffer_config: &BufferConfig, context: &mut impl InitContext<Self>) -> bool {
        let latency = self.setup(buffer_config.sample_rate.round().max(1.0) as u32);
        context.set_latency_samples(latency);
        true
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let t = context.transport();
        let transport = t.tempo.map(|bpm| Transport { bpm, playing: t.playing, beat: t.pos_beats().unwrap_or(0.0) });
        let mut next = || loop {
            match context.next_event()? {
                NoteEvent::NoteOn { timing, note, velocity, .. } => return Some(Ev::On { timing, note, velocity }),
                NoteEvent::NoteOff { timing, note, .. } | NoteEvent::Choke { timing, note, .. } => return Some(Ev::Off { timing, note }),
                _ => continue,
            }
        };
        self.audio.render(buffer.as_slice(), &self.params, transport, &mut next);
        let latency = self.audio.latency_samples();
        if latency != self.reported_latency {
            self.reported_latency = latency;
            context.set_latency_samples(latency);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Chromagrain {
    const CLAP_ID: &'static str = "com.creativelab.chromagrain";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Play video like a sampler: every grain of sound is a grain of picture");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Sampler, ClapFeature::Stereo];
}

impl Vst3Plugin for Chromagrain {
    const VST3_CLASS_ID: [u8; 16] = *b"ChromagrainVideo";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Sampler];
}

nih_export_clap!(Chromagrain);
nih_export_vst3!(Chromagrain);
