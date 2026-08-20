//! Media I/O: wav read/write, PNG frame dumps, ffmpeg-based decode/encode
//! of arbitrary media, and yt-dlp based YouTube fetching.
//!
//! ffmpeg / ffprobe / yt-dlp are invoked as external processes so the tool
//! has no heavyweight native codec dependencies. All are optional at
//! runtime: functions return descriptive errors when a tool is missing.

use crate::audio::{AudioClip, StereoBuffer};
use crate::timeline::Source;
use crate::video::{Frame, VideoClip};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub type MediaResult<T> = Result<T, String>;

/// Max decoded video size / length so a scraped video can't eat all memory.
pub const MAX_DECODE_WIDTH: u32 = 480;
pub const MAX_DECODE_SECONDS: f64 = 90.0;
pub const DECODE_FPS: f32 = 15.0;

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

pub fn ytdlp_available() -> bool {
    Command::new("yt-dlp")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
    // Mixdown to mono.
    let samples: Vec<f32> = raw
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(AudioClip::new(samples, spec.sample_rate))
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

// ------------------------------------------------------------- ffmpeg i/o

/// Decode any media file's audio track to a mono AudioClip via ffmpeg.
pub fn decode_audio(path: &Path, sample_rate: u32) -> MediaResult<AudioClip> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-t",
            &MAX_DECODE_SECONDS.to_string(),
            "-vn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "1",
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
    let samples: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if samples.is_empty() {
        return Err("no audio stream decoded".into());
    }
    Ok(AudioClip::new(samples, sample_rate))
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
    let (src_w, src_h) = probe_video_size(path)?;
    let scale = (MAX_DECODE_WIDTH as f32 / src_w as f32).min(1.0);
    // Even dimensions keep every downstream encoder happy.
    let w = ((src_w as f32 * scale) as u32).max(2) & !1;
    let h = ((src_h as f32 * scale) as u32).max(2) & !1;

    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-t",
            &MAX_DECODE_SECONDS.to_string(),
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
        if !ffmpeg_available() {
            return Err("ffmpeg is required to load non-wav media (install ffmpeg)".into());
        }
        let a = decode_audio(path, sample_rate).ok();
        let v = decode_video(path).ok();
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
    })
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

// ---------------------------------------------------------------- yt-dlp

/// Download a YouTube (or any yt-dlp-supported) URL into `dir`, returning
/// the downloaded file path. Capped at 480p to keep decode light.
pub fn fetch_youtube(url: &str, dir: &Path) -> MediaResult<PathBuf> {
    if !ytdlp_available() {
        return Err("yt-dlp is required for YouTube scraping (pip install yt-dlp)".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let template = dir.join("%(id)s.%(ext)s");
    let out = Command::new("yt-dlp")
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

/// Convenience: fetch a URL and load it as a source.
pub fn load_youtube_source(url: &str, cache_dir: &Path, sample_rate: u32) -> MediaResult<Source> {
    let file = fetch_youtube(url, cache_dir)?;
    let mut src = load_source(&file, sample_rate)?;
    src.name = format!("yt: {url}");
    Ok(src)
}

// --------------------------------------------------- SAM 3 segmentation
//
// Text-prompted object segmentation via a Python sidecar (tools/segment.py,
// SAM 3 under the hood). The sidecar receives a video file + prompt and
// writes one binary PGM mask per frame; we bake those masks into the alpha
// channel of a derived Source, so "the cat" becomes an ordinary source
// that granulates, sequences and composites everywhere — transparent
// outside the tracked object.

/// Feather radius for mask edges, as a fraction of the frame's smaller
/// dimension. Softens the cutout so composites don't look sticker-sharp.
const MASK_FEATHER_FRAC: f32 = 0.01;

/// The sidecar command: `CHROMAGRAIN_SEGMENT_CMD` (whitespace-split)
/// overrides; otherwise `python3 tools/segment.py` when the script exists
/// relative to the working directory.
pub fn segment_cmd() -> Option<Vec<String>> {
    if let Ok(cmd) = std::env::var("CHROMAGRAIN_SEGMENT_CMD") {
        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        if !parts.is_empty() {
            return Some(parts);
        }
    }
    let script = Path::new("tools/segment.py");
    if script.exists() {
        return Some(vec!["python3".into(), "tools/segment.py".into()]);
    }
    None
}

/// True when a segmenter is configured AND reports itself ready
/// (`--check` exits 0; for the default sidecar that means SAM 3 imports).
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

/// Run the segmenter sidecar on `video` with a text `prompt` and return
/// the same video with the tracked object's mask baked into alpha.
pub fn segment_video(
    video: &VideoClip,
    prompt: &str,
    work_dir: &Path,
) -> MediaResult<VideoClip> {
    let cmd = segment_cmd().ok_or("no segmenter configured (tools/segment.py missing and CHROMAGRAIN_SEGMENT_CMD unset)")?;
    if video.frames.is_empty() {
        return Err("source has no video".into());
    }
    std::fs::create_dir_all(work_dir).map_err(|e| e.to_string())?;
    let vid_path = work_dir.join("segment-input.mp4");
    let mask_dir = work_dir.join("masks");
    let _ = std::fs::remove_dir_all(&mask_dir);
    std::fs::create_dir_all(&mask_dir).map_err(|e| e.to_string())?;

    // Silent temp encode so the sidecar sees exactly our frames.
    let silence = StereoBuffer::new(video.duration().max(0.1), 44100);
    encode_video(&vid_path, &video.frames, video.fps, &silence)?;

    let out = Command::new(&cmd[0])
        .args(&cmd[1..])
        .arg(&vid_path)
        .arg(prompt)
        .arg(&mask_dir)
        .output()
        .map_err(|e| format!("segmenter failed to launch: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("segmenter failed: {}", err.trim()));
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&mask_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "pgm").unwrap_or(false))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err("segmenter produced no masks".into());
    }
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

    #[test]
    fn segment_pipeline_with_stub_sidecar() {
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
}
