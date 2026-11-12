//! Media I/O: wav read/write, PNG frame dumps, decode/encode of arbitrary
//! media, YouTube fetching and object segmentation.
//!
//! Everything platform-specific sits behind [`MediaBackend`]. The default
//! desktop backend ([`DesktopBackend`]) runs ffmpeg / ffprobe / yt-dlp /
//! the segmenter sidecar as external processes, so the tool has no
//! heavyweight native codec dependencies; all are optional at runtime and
//! functions return descriptive errors when a tool is missing. On iOS
//! (AUv3) processes can't be spawned: that code is compiled out and the
//! host installs its own backend (AVFoundation decoding) with
//! [`set_backend`]. The free functions below always go through
//! [`backend()`], so callers never care which one is active.

use crate::audio::{AudioClip, StereoBuffer};
use crate::timeline::Source;
use crate::video::{Frame, VideoClip};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub type MediaResult<T> = Result<T, String>;

/// Max decoded video size / length so a scraped video can't eat all memory.
pub const MAX_DECODE_WIDTH: u32 = 480;
pub const MAX_DECODE_SECONDS: f64 = 90.0;
pub const DECODE_FPS: f32 = 15.0;

// ------------------------------------------------------------ backend

/// Platform media services: decoding, encoding, fetching, segmentation.
/// Implementations must be callable from any (non-audio) thread.
/// Decoders should honour [`MAX_DECODE_WIDTH`], [`MAX_DECODE_SECONDS`] and
/// [`DECODE_FPS`].
pub trait MediaBackend: Send + Sync {
    /// Human-readable name, for diagnostics.
    fn name(&self) -> &str;
    /// Whether [`MediaBackend::decode_audio`]/[`MediaBackend::decode_video`]
    /// can run at all; the error says what to install.
    fn decoder_ready(&self) -> MediaResult<()> {
        Ok(())
    }
    /// The audio track of any media file, resampled to `sample_rate`
    /// (stereo when the media has distinct channels).
    fn decode_audio(&self, path: &Path, sample_rate: u32) -> MediaResult<AudioClip>;
    /// The video track as RGBA frames, downscaled and rate-limited.
    fn decode_video(&self, path: &Path) -> MediaResult<VideoClip>;
    /// Mux frames + audio into a video file.
    fn encode_video(&self, _path: &Path, _frames: &[Frame], _fps: f32, _audio: &StereoBuffer) -> MediaResult<()> {
        Err(format!("video export is not available with the {} media backend", self.name()))
    }
    /// Download a URL into `dir` and return the file.
    fn fetch_youtube(&self, _url: &str, _dir: &Path) -> MediaResult<PathBuf> {
        Err(format!("URL fetching is not available with the {} media backend", self.name()))
    }
    /// Segment `video_file` by `prompt`, writing one binary PGM mask per
    /// frame (255 = object) into `mask_dir`.
    fn segment_masks(&self, _video_file: &Path, _prompt: &str, _mask_dir: &Path) -> MediaResult<()> {
        Err(format!("segmentation is not available with the {} media backend", self.name()))
    }
    /// Search for videos (yt-dlp `ytsearchN:query`, metadata only).
    fn search_videos(&self, _query: &str, _n: u32) -> MediaResult<Vec<crate::dig::VideoHit>> {
        Err(format!("video search is not available with the {} media backend", self.name()))
    }
    /// Full metadata (license, upload date, ...) for one video URL.
    fn video_info(&self, _url: &str) -> MediaResult<crate::dig::VideoHit> {
        Err(format!("video lookup is not available with the {} media backend", self.name()))
    }
    /// Download `url` at ≤ `max_height` p into `dir`, reporting 0..1
    /// progress; returns the file.
    fn download_video(&self, _url: &str, _dir: &Path, _max_height: u32, _progress: &mut dyn FnMut(f32)) -> MediaResult<PathBuf> {
        Err(format!("video download is not available with the {} media backend", self.name()))
    }
    /// Decode `len` seconds of a file's audio from `start`.
    fn decode_audio_range(&self, path: &Path, sample_rate: u32, start: f64, len: f64) -> MediaResult<AudioClip> {
        let a = self.decode_audio(path, sample_rate)?;
        Ok(a.trimmed(start, len))
    }
    /// Decode `len` seconds of a file's video from `start`.
    fn decode_video_range(&self, path: &Path, start: f64, len: f64) -> MediaResult<VideoClip> {
        let v = self.decode_video(path)?;
        Ok(v.trimmed(start, len))
    }
    fn ffmpeg_available(&self) -> bool {
        false
    }
    fn ytdlp_available(&self) -> bool {
        false
    }
    fn segment_available(&self) -> bool {
        false
    }
    /// Stable identifier of the active segmenter configuration for mask cache keys.
    fn segment_cache_id(&self) -> String {
        self.name().to_string()
    }
}

/// A backend that can't decode anything (wav still loads, natively). The
/// default on iOS until the host installs a real one.
pub struct NoMediaBackend;

impl MediaBackend for NoMediaBackend {
    fn name(&self) -> &str {
        "none"
    }
    fn decoder_ready(&self) -> MediaResult<()> {
        Err("no media decoder installed (the host must call media::set_backend)".into())
    }
    fn decode_audio(&self, _path: &Path, _sample_rate: u32) -> MediaResult<AudioClip> {
        self.decoder_ready().and(Err("unreachable".into()))
    }
    fn decode_video(&self, _path: &Path) -> MediaResult<VideoClip> {
        self.decoder_ready().and(Err("unreachable".into()))
    }
}

static BACKEND: RwLock<Option<Arc<dyn MediaBackend>>> = RwLock::new(None);

/// Install the process-wide media backend (e.g. AVFoundation on iOS).
pub fn set_backend(backend: Arc<dyn MediaBackend>) {
    *BACKEND.write().unwrap_or_else(|e| e.into_inner()) = Some(backend);
}

/// The active media backend: the one installed with [`set_backend`], else
/// [`DesktopBackend`] (or [`NoMediaBackend`] on iOS).
pub fn backend() -> Arc<dyn MediaBackend> {
    if let Some(b) = BACKEND.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return b.clone();
    }
    #[cfg(not(target_os = "ios"))]
    return Arc::new(DesktopBackend);
    #[cfg(target_os = "ios")]
    return Arc::new(NoMediaBackend);
}

pub fn ffmpeg_available() -> bool {
    backend().ffmpeg_available()
}

pub fn ytdlp_available() -> bool {
    backend().ytdlp_available()
}

/// Decode any media file's audio track (see [`MediaBackend::decode_audio`]).
pub fn decode_audio(path: &Path, sample_rate: u32) -> MediaResult<AudioClip> {
    backend().decode_audio(path, sample_rate)
}

/// Decode a media file's video track (see [`MediaBackend::decode_video`]).
pub fn decode_video(path: &Path) -> MediaResult<VideoClip> {
    backend().decode_video(path)
}

/// Mux rendered frames + audio into a video file.
pub fn encode_video(path: &Path, frames: &[Frame], fps: f32, audio: &StereoBuffer) -> MediaResult<()> {
    backend().encode_video(path, frames, fps, audio)
}

/// Download a YouTube (or any yt-dlp-supported) URL into `dir`, returning
/// the downloaded file path. Capped at 480p to keep decode light.
pub fn fetch_youtube(url: &str, dir: &Path) -> MediaResult<PathBuf> {
    backend().fetch_youtube(url, dir)
}

/// True when a segmenter is configured AND reports itself ready.
pub fn segment_available() -> bool {
    backend().segment_available()
}

// ---------------------------------------------------------------- wav i/o

pub fn load_wav(path: &Path) -> MediaResult<AudioClip> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    if channels >= 2 {
        // Keep the first two channels as stereo (extra channels dropped).
        let (l, r): (Vec<f32>, Vec<f32>) =
            raw.chunks_exact(channels).map(|c| (c[0], c[1])).unzip();
        return Ok(AudioClip::stereo(&l, &r, spec.sample_rate));
    }
    Ok(AudioClip::new(raw, spec.sample_rate))
}

pub fn save_wav(path: &Path, buffer: &StereoBuffer) -> MediaResult<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: buffer.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for i in 0..buffer.len() {
        for s in [buffer.left[i], buffer.right[i]] {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v).map_err(|e| e.to_string())?;
        }
    }
    writer.finalize().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- png i/o

pub fn save_frame_png(path: &Path, frame: &Frame) -> MediaResult<()> {
    let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
        .ok_or("bad frame dimensions")?;
    img.save(path).map_err(|e| e.to_string())
}

/// Load any media file into a `Source`: audio and/or video tracks, with a
/// pitch estimate for key quantization.
pub fn load_source(path: &Path, sample_rate: u32) -> MediaResult<Source> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let (audio, video);
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        audio = Some(load_wav(path)?);
        video = None;
    } else {
        let b = backend();
        b.decoder_ready()?;
        let a = b.decode_audio(path, sample_rate).ok();
        let v = b.decode_video(path).ok();
        if a.is_none() && v.is_none() {
            return Err(format!("{name}: no decodable audio or video"));
        }
        audio = a;
        video = v;
    }

    let base_hz = audio
        .as_ref()
        .and_then(|a| crate::dsp::detect_pitch(&a.samples, a.sample_rate))
        .unwrap_or(0.0);

    Ok(Source {
        name,
        audio: audio.map(std::sync::Arc::new),
        video: video.map(std::sync::Arc::new),
        base_hz,
        ..Default::default()
    })
}

/// Load `len` seconds of a media file from `start` into a `Source` (Dig's
/// random window). WAV is read natively; everything else goes through the
/// backend's range decoders.
pub fn load_source_window(path: &Path, start: f64, len: f64, sample_rate: u32) -> MediaResult<Source> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string());
    let (audio, video);
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        audio = Some(load_wav(path)?.trimmed(start, len));
        video = None;
    } else {
        let b = backend();
        b.decoder_ready()?;
        let a = b.decode_audio_range(path, sample_rate, start, len).ok();
        let v = b.decode_video_range(path, start, len).ok();
        if a.is_none() && v.is_none() {
            return Err(format!("{name}: no decodable audio or video"));
        }
        audio = a;
        video = v;
    }
    let base_hz = audio.as_ref().and_then(|a| crate::dsp::detect_pitch(&a.samples, a.sample_rate)).unwrap_or(0.0);
    Ok(Source { name, audio: audio.map(std::sync::Arc::new), video: video.map(std::sync::Arc::new), base_hz, ..Default::default() })
}

/// Convenience: fetch a URL and load it as a source.
pub fn load_youtube_source(url: &str, cache_dir: &Path, sample_rate: u32) -> MediaResult<Source> {
    let file = fetch_youtube(url, cache_dir)?;
    let mut src = load_source(&file, sample_rate)?;
    src.name = format!("yt: {url}");
    Ok(src)
}

// --------------------------------------------------- segmentation
//
// Text-prompted object segmentation via a sidecar process. On macOS the
// default is tools/segment_vision.swift (Apple Vision: people or any
// salient foreground subject, on-device, nothing to install beyond the
// Xcode command-line tools); elsewhere / as an alternative, tools/segment.py
// (SAM 3, arbitrary text prompts, wants CUDA). The sidecar receives a
// video file + prompt and writes one PGM mask per frame; we bake those
// masks into the alpha channel of a derived Source, so "the cat" becomes
// an ordinary source that granulates, sequences and composites everywhere
// — transparent outside the tracked object.

/// Feather radius for mask edges, as a fraction of the frame's smaller
/// dimension. Softens the cutout so composites don't look sticker-sharp.
const MASK_FEATHER_FRAC: f32 = 0.01;

/// A grayscale mask frame from the sidecar (255 = object).
pub struct MaskFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Minimal binary PGM (P5) reader — the sidecar writes these so neither
/// side needs an image library for the mask hand-off.
pub fn read_pgm(path: &Path) -> MediaResult<MaskFrame> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
    // Header: "P5" <ws> width <ws> height <ws> maxval <single ws> data.
    // Comments (# ...) are legal between tokens.
    let mut pos = 0usize;
    let mut token = |bytes: &[u8]| -> MediaResult<String> {
        while pos < bytes.len() {
            let b = bytes[pos];
            if b == b'#' {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
            } else if b.is_ascii_whitespace() {
                pos += 1;
            } else {
                break;
            }
        }
        let start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start == pos {
            return Err("truncated pgm header".into());
        }
        Ok(String::from_utf8_lossy(&bytes[start..pos]).into_owned())
    };
    if token(&bytes)? != "P5" {
        return Err("not a binary PGM (P5)".into());
    }
    let w: u32 = token(&bytes)?.parse().map_err(|_| "bad pgm width")?;
    let h: u32 = token(&bytes)?.parse().map_err(|_| "bad pgm height")?;
    let maxval: u32 = token(&bytes)?.parse().map_err(|_| "bad pgm maxval")?;
    if maxval == 0 || maxval > 255 {
        return Err(format!("unsupported pgm maxval {maxval}"));
    }
    pos += 1; // single whitespace after maxval
    let n = (w * h) as usize;
    if bytes.len() < pos + n {
        return Err("pgm data truncated".into());
    }
    let mut data = bytes[pos..pos + n].to_vec();
    if maxval != 255 {
        for v in data.iter_mut() {
            *v = (*v as u32 * 255 / maxval) as u8;
        }
    }
    Ok(MaskFrame { width: w, height: h, data })
}

/// Separable box blur on a grayscale mask — cheap edge feathering.
fn feather(mask: &mut MaskFrame, radius: u32) {
    if radius == 0 {
        return;
    }
    let (w, h) = (mask.width as i32, mask.height as i32);
    let r = radius as i32;
    let mut tmp = vec![0u8; mask.data.len()];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0u32;
            let mut n = 0u32;
            for dx in -r..=r {
                let sx = (x + dx).clamp(0, w - 1);
                sum += mask.data[(y * w + sx) as usize] as u32;
                n += 1;
            }
            tmp[(y * w + x) as usize] = (sum / n) as u8;
        }
    }
    for x in 0..w {
        for y in 0..h {
            let mut sum = 0u32;
            let mut n = 0u32;
            for dy in -r..=r {
                let sy = (y + dy).clamp(0, h - 1);
                sum += tmp[(sy * w + x) as usize] as u32;
                n += 1;
            }
            mask.data[(y * w + x) as usize] = (sum / n) as u8;
        }
    }
}

/// Bake mask frames into a video's alpha channel (pure; tested directly).
/// Masks are index-mapped onto frames when counts differ and
/// nearest-sampled when dimensions differ.
pub fn apply_masks(video: &VideoClip, masks: &[MaskFrame]) -> VideoClip {
    if masks.is_empty() {
        return video.clone();
    }
    let frames = video
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mi = (i * masks.len() / video.frames.len().max(1)).min(masks.len() - 1);
            let m = &masks[mi];
            let mut nf = f.clone();
            for y in 0..nf.height {
                let my = (y * m.height / nf.height.max(1)).min(m.height - 1);
                for x in 0..nf.width {
                    let mx = (x * m.width / nf.width.max(1)).min(m.width - 1);
                    let a = m.data[(my * m.width + mx) as usize];
                    let di = ((y * nf.width + x) * 4 + 3) as usize;
                    nf.data[di] = a;
                }
            }
            nf
        })
        .collect();
    VideoClip { frames, fps: video.fps }
}

/// Fast 64-bit FNV-1a fingerprint over a `VideoClip` + prompt + sidecar cmd
/// so repeat script renders reuse cached segmentation masks instantly.
pub fn video_fingerprint(video: &VideoClip, extra: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mix = |h: &mut u64, bytes: &[u8]| {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    mix(&mut h, extra.as_bytes());
    mix(&mut h, &video.fps.to_bits().to_le_bytes());
    mix(&mut h, &(video.frames.len() as u64).to_le_bytes());
    let n = video.frames.len();
    for fi in [0, n / 4, n / 2, (n * 3) / 4, n.saturating_sub(1)] {
        if let Some(f) = video.frames.get(fi) {
            mix(&mut h, &f.width.to_le_bytes());
            mix(&mut h, &f.height.to_le_bytes());
            for b in f.data.iter().step_by(61) {
                mix(&mut h, &[*b]);
            }
        }
    }
    h
}

/// Run the segmenter sidecar on `video` with a text `prompt` and return
/// the same video with the tracked object's mask baked into alpha.
pub fn segment_video(
    video: &VideoClip,
    prompt: &str,
    work_dir: &Path,
) -> MediaResult<VideoClip> {
    if video.frames.is_empty() {
        return Err("source has no video".into());
    }
    let b = backend();
    std::fs::create_dir_all(work_dir).map_err(|e| e.to_string())?;
    let mask_dir = work_dir.join("masks");
    let key_path = work_dir.join("cache_key.txt");
    let cache_key = format!(
        "v2:{:016x}:{}:{}",
        video_fingerprint(video, prompt),
        b.segment_cache_id(),
        prompt
    );

    let cached_paths = || -> Option<Vec<PathBuf>> {
        let saved = std::fs::read_to_string(&key_path).ok()?;
        if saved.trim() != cache_key {
            return None;
        }
        let mut p: Vec<PathBuf> = std::fs::read_dir(&mask_dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "pgm").unwrap_or(false))
            .collect();
        p.sort();
        (p.len().abs_diff(video.frames.len()) <= 2 && !p.is_empty()).then_some(p)
    };

    let paths = if let Some(p) = cached_paths() {
        p
    } else {
        let vid_path = work_dir.join("segment-input.mp4");
        let _ = std::fs::remove_dir_all(&mask_dir);
        std::fs::create_dir_all(&mask_dir).map_err(|e| e.to_string())?;

        // Silent temp encode so the sidecar sees exactly our frames.
        let silence = StereoBuffer::new(video.duration().max(0.1), 44100);
        b.encode_video(&vid_path, &video.frames, video.fps, &silence)?;
        b.segment_masks(&vid_path, prompt, &mask_dir)?;

        let mut p: Vec<PathBuf> = std::fs::read_dir(&mask_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "pgm").unwrap_or(false))
            .collect();
        p.sort();
        if p.is_empty() {
            return Err("segmenter produced no masks".into());
        }
        let _ = std::fs::write(&key_path, &cache_key);
        p
    };
    let radius = ((video.frames[0].width.min(video.frames[0].height) as f32
        * MASK_FEATHER_FRAC) as u32)
        .max(1);
    let mut masks = Vec::with_capacity(paths.len());
    for p in &paths {
        let mut m = read_pgm(p)?;
        feather(&mut m, radius);
        masks.push(m);
    }
    Ok(apply_masks(video, &masks))
}

// ------------------------------------------------------ desktop backend
//
// ffmpeg / ffprobe / yt-dlp / swiftc / the segmenter sidecar as external
// processes. Compiled out on iOS, where an app can't spawn processes; the
// AUv3 host supplies an AVFoundation-backed MediaBackend instead.

/// The desktop [`MediaBackend`]: external processes (ffmpeg, ffprobe,
/// yt-dlp, the segmenter sidecar). Today's behaviour, unchanged.
#[cfg(not(target_os = "ios"))]
pub struct DesktopBackend;

#[cfg(not(target_os = "ios"))]
impl MediaBackend for DesktopBackend {
    fn name(&self) -> &str {
        "desktop (ffmpeg)"
    }
    fn ffmpeg_available(&self) -> bool {
        desktop::ffmpeg_available()
    }
    fn ytdlp_available(&self) -> bool {
        desktop::ytdlp_available()
    }
    fn decoder_ready(&self) -> MediaResult<()> {
        if desktop::ffmpeg_available() {
            Ok(())
        } else {
            Err("ffmpeg is required to load non-wav media (install ffmpeg)".into())
        }
    }
    fn decode_audio(&self, path: &Path, sample_rate: u32) -> MediaResult<AudioClip> {
        desktop::decode_audio(path, sample_rate)
    }
    fn decode_video(&self, path: &Path) -> MediaResult<VideoClip> {
        desktop::decode_video(path)
    }
    fn encode_video(&self, path: &Path, frames: &[Frame], fps: f32, audio: &StereoBuffer) -> MediaResult<()> {
        desktop::encode_video(path, frames, fps, audio)
    }
    fn fetch_youtube(&self, url: &str, dir: &Path) -> MediaResult<PathBuf> {
        desktop::fetch_youtube(url, dir)
    }
    fn search_videos(&self, query: &str, n: u32) -> MediaResult<Vec<crate::dig::VideoHit>> {
        desktop::search_videos(query, n)
    }
    fn video_info(&self, url: &str) -> MediaResult<crate::dig::VideoHit> {
        desktop::video_info(url)
    }
    fn download_video(&self, url: &str, dir: &Path, max_height: u32, progress: &mut dyn FnMut(f32)) -> MediaResult<PathBuf> {
        desktop::download_video(url, dir, max_height, progress)
    }
    fn decode_audio_range(&self, path: &Path, sample_rate: u32, start: f64, len: f64) -> MediaResult<AudioClip> {
        desktop::decode_audio_range(path, sample_rate, start, len)
    }
    fn decode_video_range(&self, path: &Path, start: f64, len: f64) -> MediaResult<VideoClip> {
        desktop::decode_video_range(path, start, len)
    }
    fn segment_available(&self) -> bool {
        desktop::segment_available()
    }
    fn segment_cache_id(&self) -> String {
        desktop::resolve_segment_cmd()
            .map(|c| c.join(" "))
            .unwrap_or_else(|_| self.name().to_string())
    }
    fn segment_masks(&self, video_file: &Path, prompt: &str, mask_dir: &Path) -> MediaResult<()> {
        desktop::segment_masks(video_file, prompt, mask_dir)
    }
}

#[cfg(not(target_os = "ios"))]
pub use desktop::{resolve_segment_cmd, segment_cmd};
#[cfg(target_os = "macos")]
pub use desktop::vision_sidecar;

#[cfg(not(target_os = "ios"))]
mod desktop {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    fn tool_exists(name: &str) -> bool {
        Command::new(name)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn ffmpeg_available() -> bool {
        tool_exists("ffmpeg")
    }

    /// The yt-dlp executable: [`crate::dig::YTDLP_ENV`] (a stub in tests /
    /// the showcase) or `yt-dlp` on PATH.
    pub fn ytdlp_bin() -> String {
        std::env::var(crate::dig::YTDLP_ENV).ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "yt-dlp".into())
    }

    pub fn ytdlp_available() -> bool {
        Command::new(ytdlp_bin())
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Decode any media file's audio track via ffmpeg (stereo when the source
    /// has distinct channels; mono sources come back as mono).
    pub fn decode_audio(path: &Path, sample_rate: u32) -> MediaResult<AudioClip> {
        decode_audio_range(path, sample_rate, 0.0, MAX_DECODE_SECONDS)
    }

    /// [`decode_audio`] of `len` seconds from `start` (fast input seek).
    pub fn decode_audio_range(path: &Path, sample_rate: u32, start: f64, len: f64) -> MediaResult<AudioClip> {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-ss", &format!("{:.3}", start.max(0.0)), "-i"])
            .arg(path)
            .args([
                "-t",
                &format!("{:.3}", len.clamp(0.0, MAX_DECODE_SECONDS)),
                "-vn",
                "-f",
                "f32le",
                "-acodec",
                "pcm_f32le",
                "-ac",
                "2",
                "-ar",
                &sample_rate.to_string(),
                "-",
            ])
            .output()
            .map_err(|e| format!("ffmpeg not runnable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ffmpeg audio decode failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let (l, r): (Vec<f32>, Vec<f32>) = out
            .stdout
            .chunks_exact(8)
            .map(|b| {
                (
                    f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                )
            })
            .unzip();
        if l.is_empty() {
            return Err("no audio stream decoded".into());
        }
        Ok(AudioClip::stereo(&l, &r, sample_rate))
    }

    /// Probe a media file's video stream dimensions with ffprobe.
    fn probe_video_size(path: &Path) -> MediaResult<(u32, u32)> {
        let out = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=s=x:p=0",
            ])
            .arg(path)
            .output()
            .map_err(|e| format!("ffprobe not runnable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ffprobe failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.trim().lines().next().unwrap_or("");
        let mut it = line.trim().split('x');
        let w: u32 = it.next().and_then(|v| v.parse().ok()).ok_or("no width")?;
        let h: u32 = it.next().and_then(|v| v.parse().ok()).ok_or("no height")?;
        Ok((w, h))
    }

    /// Decode a media file's video track to RGBA frames via ffmpeg (rawvideo
    /// pipe), downscaled and rate-limited to keep memory reasonable.
    pub fn decode_video(path: &Path) -> MediaResult<VideoClip> {
        decode_video_range(path, 0.0, MAX_DECODE_SECONDS)
    }

    /// [`decode_video`] of `len` seconds from `start` (fast input seek).
    pub fn decode_video_range(path: &Path, start: f64, len: f64) -> MediaResult<VideoClip> {
        let (src_w, src_h) = probe_video_size(path)?;
        let scale = (MAX_DECODE_WIDTH as f32 / src_w as f32).min(1.0);
        // Even dimensions keep every downstream encoder happy.
        let w = ((src_w as f32 * scale) as u32).max(2) & !1;
        let h = ((src_h as f32 * scale) as u32).max(2) & !1;

        let mut child = Command::new("ffmpeg")
            .args(["-v", "error", "-ss", &format!("{:.3}", start.max(0.0)), "-i"])
            .arg(path)
            .args([
                "-t",
                &format!("{:.3}", len.clamp(0.0, MAX_DECODE_SECONDS)),
                "-an",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{w}x{h}"),
                "-r",
                &DECODE_FPS.to_string(),
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg not runnable: {e}"))?;

        let mut stdout = child.stdout.take().ok_or("no stdout")?;
        let frame_bytes = (w * h * 4) as usize;
        let mut frames = Vec::new();
        let max_frames = (MAX_DECODE_SECONDS * DECODE_FPS as f64) as usize;
        let mut buf = vec![0u8; frame_bytes];
        loop {
            match read_exact_or_eof(&mut stdout, &mut buf) {
                Ok(true) => {
                    frames.push(Frame { width: w, height: h, data: buf.clone() });
                    if frames.len() >= max_frames {
                        break;
                    }
                }
                Ok(false) => break,
                Err(e) => return Err(format!("read frames: {e}")),
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        if frames.is_empty() {
            return Err("no video frames decoded".into());
        }
        Ok(VideoClip { frames, fps: DECODE_FPS })
    }

    fn read_exact_or_eof(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<bool> {
        let mut filled = 0;
        while filled < buf.len() {
            let n = r.read(&mut buf[filled..])?;
            if n == 0 {
                return Ok(false); // EOF (possibly mid-frame; drop partial)
            }
            filled += n;
        }
        Ok(true)
    }

    /// Mux rendered frames + audio into a video file via ffmpeg.
    pub fn encode_video(
        path: &Path,
        frames: &[Frame],
        fps: f32,
        audio: &StereoBuffer,
    ) -> MediaResult<()> {
        if frames.is_empty() {
            return Err("nothing to encode".into());
        }
        let (w, h) = (frames[0].width, frames[0].height);

        // Write audio to a temp wav next to the output.
        let wav_path: PathBuf = path.with_extension("chromagrain-tmp.wav");
        save_wav(&wav_path, audio)?;

        let mut child = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{w}x{h}"),
                "-r",
                &fps.to_string(),
                "-i",
                "-",
                "-i",
            ])
            .arg(&wav_path)
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "veryfast",
                "-c:a",
                "aac",
                "-b:a",
                "256k",
                "-shortest",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg not runnable: {e}"))?;

        {
            let mut stdin = child.stdin.take().ok_or("no stdin")?;
            for f in frames {
                if f.width != w || f.height != h {
                    return Err("inconsistent frame sizes".into());
                }
                stdin.write_all(&f.data).map_err(|e| e.to_string())?;
            }
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&wav_path);
        if !out.status.success() {
            return Err(format!(
                "ffmpeg encode failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    /// Download a YouTube (or any yt-dlp-supported) URL into `dir`, returning
    /// the downloaded file path. Capped at 480p to keep decode light.
    pub fn fetch_youtube(url: &str, dir: &Path) -> MediaResult<PathBuf> {
        if !ytdlp_available() {
            return Err("yt-dlp is required for YouTube scraping (pip install yt-dlp)".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let template = dir.join("%(id)s.%(ext)s");
        let out = Command::new(ytdlp_bin())
            .args([
                "-f",
                "bv*[height<=480]+ba/b[height<=480]/b",
                "--merge-output-format",
                "mp4",
                "--no-playlist",
                "--print",
                "after_move:filepath",
                "--no-simulate",
                "-o",
            ])
            .arg(&template)
            .arg(url)
            .output()
            .map_err(|e| format!("yt-dlp not runnable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "yt-dlp failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let file = stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .ok_or("yt-dlp reported no file")?
            .trim()
            .to_string();
        let p = PathBuf::from(file);
        if !p.exists() {
            return Err(format!("yt-dlp output missing: {p:?}"));
        }
        Ok(p)
    }

    fn hit_from_json(v: &serde_json::Value) -> crate::dig::VideoHit {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let id = s("id");
        let url = [s("webpage_url"), s("url")]
            .into_iter()
            .find(|u| u.starts_with("http"))
            .unwrap_or_else(|| if id.is_empty() { String::new() } else { format!("https://www.youtube.com/watch?v={id}") });
        let uploader = [s("uploader"), s("channel"), s("uploader_id")].into_iter().find(|u| !u.is_empty()).unwrap_or_default();
        let live = v.get("is_live").and_then(|x| x.as_bool()).unwrap_or(false)
            || matches!(v.get("live_status").and_then(|x| x.as_str()), Some("is_live" | "is_upcoming" | "post_live"));
        crate::dig::VideoHit {
            id,
            url,
            title: s("title"),
            uploader,
            upload_date: s("upload_date"),
            views: v.get("view_count").and_then(|x| x.as_u64()),
            duration: v.get("duration").and_then(|x| x.as_f64()),
            live,
            license: v.get("license").and_then(|x| x.as_str()).map(String::from),
        }
    }

    fn run_ytdlp(args: &[&str]) -> MediaResult<String> {
        let out = Command::new(ytdlp_bin())
            .args(args)
            .output()
            .map_err(|e| format!("yt-dlp not runnable ({}): {e}", ytdlp_bin()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("yt-dlp failed: {}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `yt-dlp --flat-playlist --dump-json "ytsearchN:query"`.
    pub fn search_videos(query: &str, n: u32) -> MediaResult<Vec<crate::dig::VideoHit>> {
        let q = format!("ytsearch{}:{query}", n.clamp(1, 100));
        let out = run_ytdlp(&["--flat-playlist", "--dump-json", "--no-warnings", "--ignore-errors", &q])?;
        Ok(out.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).map(|v| hit_from_json(&v)).collect())
    }

    /// `yt-dlp --dump-json --skip-download URL`.
    pub fn video_info(url: &str) -> MediaResult<crate::dig::VideoHit> {
        let out = run_ytdlp(&["--dump-json", "--skip-download", "--no-warnings", "--no-playlist", url])?;
        let line = out.lines().find(|l| l.trim_start().starts_with('{')).ok_or("yt-dlp returned no metadata")?;
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("yt-dlp metadata: {e}"))?;
        Ok(hit_from_json(&v))
    }

    /// Download at ≤ `max_height`p, streaming `[download]  42.0%` progress.
    pub fn download_video(url: &str, dir: &Path, max_height: u32, progress: &mut dyn FnMut(f32)) -> MediaResult<PathBuf> {
        use std::io::BufRead;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let template = dir.join("%(id)s.%(ext)s");
        let fmt = format!("bv*[height<={h}]+ba/b[height<={h}]/b", h = max_height.max(144));
        let mut child = Command::new(ytdlp_bin())
            .args(["-f", &fmt, "--merge-output-format", "mp4", "--no-playlist", "--newline", "--progress", "--no-warnings"])
            .args(["--print", "after_move:filepath", "--no-simulate", "-o"])
            .arg(&template)
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("yt-dlp not runnable ({}): {e}", ytdlp_bin()))?;
        // Progress can arrive on either stream; stderr is drained on a thread.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let t = std::thread::spawn(move || {
            for l in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        let parse = |l: &str| -> Option<f32> {
            let l = l.trim();
            if !l.starts_with("[download]") {
                return None;
            }
            let pct = l.split_whitespace().find(|w| w.ends_with('%'))?;
            pct.trim_end_matches('%').parse::<f32>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0))
        };
        let mut file = None;
        let mut errors = Vec::new();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            while let Ok(e) = rx.try_recv() {
                match parse(&e) {
                    Some(p) => progress(p),
                    None => errors.push(e),
                }
            }
            match parse(&l) {
                Some(p) => progress(p),
                None if !l.trim().is_empty() && !l.starts_with('[') => file = Some(l.trim().to_string()),
                None => {}
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let _ = t.join();
        while let Ok(e) = rx.try_recv() {
            if let Some(p) = parse(&e) {
                progress(p);
            } else {
                errors.push(e);
            }
        }
        if !status.success() {
            return Err(format!("yt-dlp download failed: {}", errors.iter().rev().find(|l| !l.trim().is_empty()).map(|s| s.trim()).unwrap_or("")));
        }
        let p = PathBuf::from(file.ok_or("yt-dlp reported no file")?);
        if !p.exists() {
            return Err(format!("yt-dlp output missing: {p:?}"));
        }
        progress(1.0);
        Ok(p)
    }

    /// The sidecar command, resolved in order:
    /// 1. `CHROMAGRAIN_SEGMENT_CMD` (whitespace-split) overrides everything;
    /// 2. on macOS, the Vision sidecar (compiled once from
    ///    tools/segment_vision.swift and cached — see [`vision_sidecar`]);
    /// 3. `python3 tools/segment.py` (SAM 3) when the script exists relative
    ///    to the working directory.
    pub fn segment_cmd() -> Option<Vec<String>> {
        resolve_segment_cmd().ok()
    }

    /// [`segment_cmd`] with the reason when nothing usable is found (e.g. the
    /// Vision sidecar's compile errors), for surfacing in the UI.
    pub fn resolve_segment_cmd() -> MediaResult<Vec<String>> {
        if let Ok(cmd) = std::env::var("CHROMAGRAIN_SEGMENT_CMD") {
            let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
            if !parts.is_empty() {
                return Ok(parts);
            }
        }
        #[allow(unused_mut)]
        let mut why = String::from("no segmenter configured (CHROMAGRAIN_SEGMENT_CMD unset");
        #[cfg(target_os = "macos")]
        match vision_sidecar() {
            Ok(bin) => return Ok(vec![bin.to_string_lossy().into_owned()]),
            Err(e) => why.push_str(&format!("; Vision sidecar unavailable: {e}")),
        }
        let script = Path::new("tools/segment.py");
        if script.exists() {
            return Ok(vec!["python3".into(), "tools/segment.py".into()]);
        }
        why.push_str("; tools/segment.py missing)");
        Err(why)
    }

    /// Locate tools/segment_vision.swift: next to this crate's workspace (baked
    /// in at compile time), else relative to the working directory.
    #[cfg(target_os = "macos")]
    fn vision_source() -> Option<PathBuf> {
        let baked = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/segment_vision.swift");
        let local = PathBuf::from("tools/segment_vision.swift");
        [baked, local].into_iter().find(|p| p.exists())
    }

    /// Path of the compiled Vision sidecar, building it with `swiftc -O` into
    /// ~/Library/Caches/chromagrain/ on first use (and whenever the .swift is
    /// newer than the cached binary). Errors carry swiftc's diagnostics.
    #[cfg(target_os = "macos")]
    pub fn vision_sidecar() -> MediaResult<PathBuf> {
        let src = vision_source().ok_or("tools/segment_vision.swift not found")?;
        let cache = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Caches/chromagrain"))
            .unwrap_or_else(|| std::env::temp_dir().join("chromagrain"));
        let bin = cache.join("segment_vision");
        let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let fresh = match (mtime(&bin), mtime(&src)) {
            (Some(b), Some(s)) => b >= s,
            _ => false,
        };
        if fresh {
            return Ok(bin);
        }
        std::fs::create_dir_all(&cache).map_err(|e| format!("create {cache:?}: {e}"))?;
        // Build to a temp name then rename, so a concurrent caller never runs
        // a half-written binary.
        let tmp = cache.join(format!("segment_vision.build-{}", std::process::id()));
        let out = Command::new("swiftc")
            .arg("-O")
            .arg(&src)
            .arg("-o")
            .arg(&tmp)
            .output()
            .map_err(|e| format!("swiftc not available (install Xcode command-line tools): {e}"))?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            let err = String::from_utf8_lossy(&out.stderr);
            let errs: Vec<&str> = err.lines().filter(|l| l.contains("error")).take(8).collect();
            return Err(format!(
                "compiling {} failed:\n{}",
                src.display(),
                if errs.is_empty() { err.trim().to_string() } else { errs.join("\n") }
            ));
        }
        std::fs::rename(&tmp, &bin).map_err(|e| format!("install {bin:?}: {e}"))?;
        Ok(bin)
    }

    /// True when a segmenter is configured AND reports itself ready
    /// (`--check` exits 0; for SAM 3 that means it imports, for Vision that the
    /// sidecar compiled and runs).
    pub fn segment_available() -> bool {
        let Some(cmd) = segment_cmd() else { return false };
        Command::new(&cmd[0])
            .args(&cmd[1..])
            .arg("--check")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Run the sidecar on `video_file`, writing one PGM mask per frame into
    /// `mask_dir`.
    pub fn segment_masks(video_file: &Path, prompt: &str, mask_dir: &Path) -> MediaResult<()> {
        let cmd = resolve_segment_cmd()?;
        let out = Command::new(&cmd[0])
            .args(&cmd[1..])
            .arg(video_file)
            .arg(prompt)
            .arg(mask_dir)
            .output()
            .map_err(|e| format!("segmenter failed to launch: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("segmenter failed: {}", err.trim()));
        }
        Ok(())
    }
}

/// Derive a segmented Source: same audio, video transparent outside the
/// prompted object. Purely visual — audio and base pitch pass through.
pub fn segment_source(src: &Source, prompt: &str, work_dir: &Path) -> MediaResult<Source> {
    let video = src.video.as_ref().ok_or("source has no video to segment")?;
    let masked = segment_video(video, prompt, work_dir)?;
    Ok(Source {
        name: format!("{prompt} @ {}", src.name),
        audio: src.audio.clone(),
        video: Some(std::sync::Arc::new(masked)),
        base_hz: src.base_hz,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip() {
        let dir = std::env::temp_dir().join("chromagrain-test-wav");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip.wav");

        let clip = AudioClip::sine(440.0, 0.5, 48000);
        let mut buf = StereoBuffer::new(0.5, 48000);
        for i in 0..buf.len().min(clip.samples.len()) {
            buf.left[i] = clip.samples[i];
            buf.right[i] = clip.samples[i];
        }
        save_wav(&path, &buf).unwrap();
        let loaded = load_wav(&path).unwrap();
        assert_eq!(loaded.sample_rate, 48000);
        assert!((loaded.duration() - 0.5).abs() < 0.01);
        // Content survives (mono mixdown of identical channels).
        let orig_rms = (clip.samples.iter().map(|s| s * s).sum::<f32>()
            / clip.samples.len() as f32)
            .sqrt();
        let load_rms = (loaded.samples.iter().map(|s| s * s).sum::<f32>()
            / loaded.samples.len() as f32)
            .sqrt();
        assert!((orig_rms - load_rms).abs() < 0.02);
        assert!(!loaded.is_stereo(), "identical channels load as mono");

        // Distinct channels stay stereo.
        let mut buf = StereoBuffer::new(0.2, 48000);
        for i in 0..buf.len() {
            buf.left[i] = 0.5 * (i as f32 * 0.03).sin();
        }
        let path = dir.join("stereo.wav");
        save_wav(&path, &buf).unwrap();
        let st = load_wav(&path).unwrap();
        assert!(st.is_stereo());
        let (l, r) = st.left_right();
        assert!(r.iter().all(|v| v.abs() < 1e-3));
        assert!(l.iter().map(|v| v.abs()).fold(0.0, f32::max) > 0.4);
    }

    #[test]
    fn pgm_roundtrip_and_masks_bake_into_alpha() {
        let dir = std::env::temp_dir().join("chromagrain-test-pgm");
        std::fs::create_dir_all(&dir).unwrap();
        // Write a P5 by hand: 4x2, left half black, right half white.
        let path = dir.join("m.pgm");
        let mut bytes = b"P5\n# comment\n4 2\n255\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255]);
        std::fs::write(&path, &bytes).unwrap();
        let m = read_pgm(&path).unwrap();
        assert_eq!((m.width, m.height), (4, 2));
        assert_eq!(m.data, vec![0, 0, 255, 255, 0, 0, 255, 255]);

        // Bake into a 8x4 clip: alpha should follow the mask, upsampled.
        let clip = crate::video::VideoClip::test_pattern(8, 4, 10.0, 0.3);
        let out = apply_masks(&clip, &[m]);
        assert_eq!(out.frames.len(), clip.frames.len());
        let f = &out.frames[0];
        let a = |x: u32, y: u32| f.data[((y * f.width + x) * 4 + 3) as usize];
        assert_eq!(a(0, 0), 0, "left half transparent");
        assert_eq!(a(3, 3), 0);
        assert_eq!(a(4, 0), 255, "right half opaque");
        assert_eq!(a(7, 3), 255);
        // RGB untouched.
        assert_eq!(f.data[0], clip.frames[0].data[0]);
    }

    /// Segmentation tests read/write CHROMAGRAIN_SEGMENT_CMD (process-global),
    /// so they must not run concurrently.
    static SEGMENT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// End-to-end: Apple Vision sidecar compiled from tools/ on the fly,
    /// run through segment_source on a real clip (or a synthetic one).
    /// Ignored by default (~10-20s incl. first swiftc build):
    /// `cargo test --release -p chromagrain-core -- --ignored vision`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn segment_with_vision_sidecar() {
        let _g = SEGMENT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg not available");
            return;
        }
        std::env::remove_var("CHROMAGRAIN_SEGMENT_CMD");
        let bin = vision_sidecar().expect("vision sidecar should compile");
        assert!(segment_cmd().unwrap()[0] == bin.to_string_lossy());
        assert!(segment_available(), "vision --check should pass");

        let clip = Path::new(concat!(
            "/Users/ardenschager/Documents/Projects/video-grid/public/clips/",
            "ff_home_movie_000448_1947_detroit_family_0002.mp4"
        ));
        let src = if clip.exists() {
            let mut s = load_source(clip, 44100).unwrap();
            // Keep the test quick: first ~2s of frames.
            let v = s.video.as_ref().unwrap();
            let frames = v.frames.iter().take(48).cloned().collect();
            s.video = Some(std::sync::Arc::new(VideoClip { frames, fps: v.fps }));
            s
        } else {
            eprintln!("real clip missing; using a synthetic pattern");
            Source {
                name: "pat".into(),
                audio: None,
                video: Some(std::sync::Arc::new(
                    crate::video::VideoClip::test_pattern(160, 120, 12.0, 1.0),
                )),
                base_hz: 220.0,
                ..Default::default()
            }
        };
        let n_in = src.video.as_ref().unwrap().frames.len();
        let dir = std::env::temp_dir().join("chromagrain-test-vision");
        for prompt in ["person", "subject"] {
            let work = dir.join(prompt);
            let out = segment_source(&src, prompt, &work).unwrap();
            let v = out.video.as_ref().unwrap();
            assert_eq!(v.frames.len(), n_in);
            // The sidecar wrote one mask per encoded frame (±1 for the
            // encoder's -shortest trim).
            let n_masks = std::fs::read_dir(work.join("masks")).unwrap().count();
            assert!(n_masks.abs_diff(n_in) <= 1, "{prompt}: {n_masks} masks for {n_in} frames");
            if clip.exists() {
                // Real footage with people: alpha must actually vary.
                let (mut lo, mut hi) = (255u8, 0u8);
                for f in &v.frames {
                    for a in f.data.iter().skip(3).step_by(4) {
                        lo = lo.min(*a);
                        hi = hi.max(*a);
                    }
                }
                assert!(lo < 20 && hi > 235, "{prompt}: alpha range {lo}..{hi}");
            }
        }
    }

    #[test]
    fn segment_pipeline_with_stub_sidecar() {
        let _g = SEGMENT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg not available");
            return;
        }
        let dir = std::env::temp_dir().join("chromagrain-test-segment");
        std::fs::create_dir_all(&dir).unwrap();
        // A stub segmenter honoring the sidecar contract: --check exits 0;
        // segmentation writes bottom-half-white masks regardless of input.
        let stub = dir.join("stub_segment.py");
        std::fs::write(
            &stub,
            r#"
import os, sys
if len(sys.argv) == 2 and sys.argv[1] == "--check":
    print("stub ok"); sys.exit(0)
video, prompt, out_dir = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out_dir, exist_ok=True)
w, h = 16, 12
rows = bytes([0]) * w * (h // 2) + bytes([255]) * w * (h - h // 2)
for i in range(4):
    with open(os.path.join(out_dir, f"mask_{i:05d}.pgm"), "wb") as f:
        f.write(f"P5\n{w} {h}\n255\n".encode())
        f.write(rows)
print("ok 4")
"#,
        )
        .unwrap();
        std::env::set_var(
            "CHROMAGRAIN_SEGMENT_CMD",
            format!("python3 {}", stub.display()),
        );
        assert!(segment_available(), "stub --check should pass");

        let src = Source {
            name: "pat".into(),
            audio: Some(std::sync::Arc::new(AudioClip::new(vec![0.1; 4410], 44100))),
            video: Some(std::sync::Arc::new(
                crate::video::VideoClip::test_pattern(32, 24, 10.0, 0.5),
            )),
            base_hz: 220.0,
            ..Default::default()
        };
        let out = segment_source(&src, "cat", &dir.join("work")).unwrap();
        std::env::remove_var("CHROMAGRAIN_SEGMENT_CMD");

        assert_eq!(out.name, "cat @ pat");
        assert_eq!(out.base_hz, 220.0);
        assert!(out.audio.is_some(), "audio passes through untouched");
        let v = out.video.as_ref().unwrap();
        assert_eq!(v.frames.len(), src.video.as_ref().unwrap().frames.len());
        let f = &v.frames[0];
        let a = |x: u32, y: u32| f.data[((y * f.width + x) * 4 + 3) as usize];
        // Top rows transparent, bottom rows opaque (feather softens the
        // boundary, so sample away from the midline).
        assert!(a(5, 1) < 30, "top should be transparent, got {}", a(5, 1));
        assert!(a(5, 22) > 225, "bottom should be opaque, got {}", a(5, 22));
        // The feathered edge actually grades.
        let mid = a(5, 12);
        assert!(mid > 30 && mid < 225, "edge should be feathered, got {mid}");
    }

    #[test]
    fn png_save() {
        let dir = std::env::temp_dir().join("chromagrain-test-png");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frame.png");
        let f = crate::video::VideoClip::test_pattern(32, 24, 10.0, 0.2).frames[0].clone();
        save_frame_png(&path, &f).unwrap();
        assert!(path.metadata().unwrap().len() > 100);
    }

    /// `CG_STEREO_FILE=/path/to/stereo.mp4 cargo test --release -p chromagrain-core stereo_media -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stereo_media_keeps_its_image() {
        let Ok(path) = std::env::var("CG_STEREO_FILE") else { return };
        let src = load_source(Path::new(&path), 48000).unwrap();
        let a = src.audio.expect("audio");
        let (l, r) = a.left_right();
        let e = |v: &[f32]| v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
        let side = a.side.as_deref().map(e).unwrap_or(0.0);
        eprintln!("stereo={} side/mid energy {:.3}", a.is_stereo(), side / e(&a.samples).max(1e-12));
        assert!(a.is_stereo());
        // Straight playback through the grain renderer keeps L != R.
        let ev = crate::grain::GrainEvent {
            onset: 0.0, source_pos: 0.0, duration: a.duration().min(5.0) as f32, pitch_ratio: 1.0,
            gain: 1.0, pan: 0.0, envelope: 0.05, env_skew: 0.0, reverse: false, id: 0,
            ..Default::default()
        };
        let mut out = StereoBuffer::new(ev.duration as f64, 48000);
        crate::audio::render_grains_audio(&a, &[ev], &mut out, 0.0);
        let d: f64 = out.left.iter().zip(&out.right).map(|(x, y)| ((x - y) as f64).powi(2)).sum();
        eprintln!("rendered L-R energy {:.3} (L {:.3}, R {:.3}); source L {:.3} R {:.3}", d, e(&out.left), e(&out.right), e(&l), e(&r));
        assert!(d > 1e-3);
    }
}
