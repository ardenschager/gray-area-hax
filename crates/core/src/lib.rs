//! chromagrain-core: an audiovisual granular sampling engine.
//!
//! One grain scheduler emits [`grain::GrainEvent`]s consumed by BOTH the
//! audio renderer ([`audio::render_grains_audio`]) and the visual
//! compositor ([`video::composite_grains_frame`]) — audio and visual
//! grains are the same object, so the two modalities always correspond:
//!
//! | grain field  | audio                   | video                        |
//! |--------------|-------------------------|------------------------------|
//! | onset/dur    | when it sounds          | when it appears              |
//! | envelope     | amplitude window        | opacity window               |
//! | pitch_ratio  | resampling ratio        | patch playback rate + hue    |
//! | pan          | stereo position         | horizontal position          |
//! | source_pos   | waveform read position  | video read position          |
//! | reverse      | backwards audio         | backwards video              |
//!
//! Other first-class citizens:
//! * [`music::Key::quantize_ratio`] — snap grain pitches to keys/scales
//! * [`dsp::Biquad`] — filter out audio frequency bands
//! * [`video::ColorFilter`] — filter out hue bands (the visual analog)
//! * [`timeline::Project`] — beat-based multitrack sequencing
//! * [`script`] — rhai scripting over the whole engine
//! * [`media`] — ffmpeg decode/encode + yt-dlp YouTube scraping

pub mod audio;
pub mod auto;
pub mod dsp;
pub mod fx;
pub mod grain;
pub mod media;
pub mod music;
pub mod realtime;
pub mod render;
pub mod script;
pub mod seq;
pub mod timeline;
pub mod video;
