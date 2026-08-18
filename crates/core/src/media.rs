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
    fn png_save() {
        let dir = std::env::temp_dir().join("chromagrain-test-png");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frame.png");
        let f = crate::video::VideoClip::test_pattern(32, 24, 10.0, 0.2).frames[0].clone();
        save_frame_png(&path, &f).unwrap();
        assert!(path.metadata().unwrap().len() > 100);
    }
}
