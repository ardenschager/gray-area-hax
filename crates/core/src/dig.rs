//! Dig and Link: import random, rarely-seen videos (astronaut.io /
//! petittube style) or any yt-dlp URL, as background jobs.
//!
//! [`dig`] searches with a camera-default filename query (`IMG_1234`,
//! `MVI_0042`, `VID_20190612`, ...) or a random word ([`random_query`]),
//! keeps hits that pass the match filter ([`DigOptions`]: low view count,
//! 10 s–10 min, not live, optionally Creative Commons only), picks one at
//! random, downloads it at ≤480p, cuts a random 20–60 s window and returns
//! it as a [`Source`] with [`Provenance`]. Empty results retry with a new
//! query. [`fetch_link`] does the download + load for a given URL.
//!
//! Jobs run on their own thread; poll [`Job::status`] (or [`Job::progress`])
//! from the UI and [`Job::take_source`] when done. Platform work goes
//! through [`MediaBackend`](crate::media::MediaBackend) (desktop: yt-dlp
//! and ffmpeg); set [`YTDLP_ENV`] to a stub yt-dlp for tests and the
//! showcase, so no network is needed.

use crate::media::MediaResult;
use crate::timeline::{Provenance, Source};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Env var naming the yt-dlp executable to run (a stub in tests / the
/// showcase). Unset = `yt-dlp` on PATH.
pub const YTDLP_ENV: &str = "CHROMAGRAIN_YTDLP";
/// Env var seeding Dig's random choices (queries, pick, window) for
/// deterministic runs.
pub const DIG_SEED_ENV: &str = "CHROMAGRAIN_DIG_SEED";

/// One search hit, as yt-dlp reports it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoHit {
    pub id: String,
    pub url: String,
    pub title: String,
    pub uploader: String,
    /// YYYYMMDD, when known.
    pub upload_date: String,
    pub views: Option<u64>,
    pub duration: Option<f64>,
    pub live: bool,
    /// e.g. "Creative Commons Attribution license (reuse allowed)".
    pub license: Option<String>,
}

impl VideoHit {
    pub fn is_creative_commons(&self) -> bool {
        self.license.as_deref().is_some_and(|l| l.to_ascii_lowercase().contains("creative commons"))
    }
}

/// Dig's match filter and window.
#[derive(Debug, Clone, PartialEq)]
pub struct DigOptions {
    /// Results per search (`ytsearchN`).
    pub results: u32,
    /// Keep hits with fewer views than this.
    pub max_views: u64,
    pub min_secs: f64,
    pub max_secs: f64,
    /// Random window cut from the download, seconds (min, max).
    pub window: (f64, f64),
    /// Only Creative Commons licensed videos.
    pub cc_only: bool,
    /// Queries tried before giving up.
    pub attempts: u32,
    /// Seed for queries / pick / window (None = from the clock or
    /// [`DIG_SEED_ENV`]).
    pub seed: Option<u64>,
    pub sample_rate: u32,
    /// Where downloads go.
    pub cache_dir: PathBuf,
}

impl Default for DigOptions {
    fn default() -> Self {
        DigOptions {
            results: 20,
            max_views: 500,
            min_secs: 10.0,
            max_secs: 600.0,
            window: (20.0, 60.0),
            cc_only: false,
            attempts: 6,
            seed: None,
            sample_rate: 48_000,
            cache_dir: std::env::temp_dir().join("chromagrain-dig"),
        }
    }
}

impl DigOptions {
    /// Does a hit pass the match filter?
    pub fn accepts(&self, h: &VideoHit) -> bool {
        !h.live
            && h.views.is_none_or(|v| v < self.max_views)
            && h.duration.is_some_and(|d| d >= self.min_secs && d <= self.max_secs)
            && (!self.cc_only || h.license.is_none() || h.is_creative_commons())
    }
}

/// Where a job is.
#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Queued,
    /// Searching with `query` (attempt 1..).
    Searching { query: String, attempt: u32 },
    /// Downloading, 0..1.
    Downloading(f32),
    /// Decoding + cutting the window.
    Cutting,
    /// Finished: the source is waiting in [`Job::take_source`] (or taken).
    Done,
    Failed(String),
}

impl JobStatus {
    pub fn is_finished(&self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed(_))
    }
}

#[derive(Debug)]
struct JobState {
    status: JobStatus,
    source: Option<Source>,
    /// Title of the chosen video, as soon as it's known (placeholder card).
    title: Option<String>,
    cancelled: bool,
}

/// A background Dig / Link job. Cheap to clone; all clones see the same job.
#[derive(Debug, Clone)]
pub struct Job {
    state: Arc<Mutex<JobState>>,
}

impl Job {
    fn new() -> Job {
        Job { state: Arc::new(Mutex::new(JobState { status: JobStatus::Queued, source: None, title: None, cancelled: false })) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut JobState) -> R) -> R {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }

    pub fn status(&self) -> JobStatus {
        self.with(|s| s.status.clone())
    }

    /// Overall progress 0..1 (search 0–0.1, download 0.1–0.9, cut 0.9–1).
    pub fn progress(&self) -> f32 {
        match self.status() {
            JobStatus::Queued => 0.0,
            JobStatus::Searching { .. } => 0.05,
            JobStatus::Downloading(p) => 0.1 + 0.8 * p.clamp(0.0, 1.0),
            JobStatus::Cutting => 0.95,
            JobStatus::Done | JobStatus::Failed(_) => 1.0,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.status().is_finished()
    }

    /// The chosen video's title, once known (before it finishes).
    pub fn title(&self) -> Option<String> {
        self.with(|s| s.title.clone())
    }

    /// The finished source (once; later calls return None).
    pub fn take_source(&self) -> Option<Source> {
        self.with(|s| s.source.take())
    }

    /// Ask the job to stop at its next step.
    pub fn cancel(&self) {
        self.with(|s| s.cancelled = true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.with(|s| s.cancelled)
    }

    /// Block until finished (tests / headless tools).
    pub fn wait(&self) -> JobStatus {
        loop {
            let st = self.status();
            if st.is_finished() {
                return st;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[allow(dead_code)] // used by the dig/link workers (next commits)
    fn set(&self, status: JobStatus) {
        self.with(|s| s.status = status);
    }

    #[allow(dead_code)]
    fn set_title(&self, t: &str) {
        self.with(|s| s.title = Some(t.to_string()));
    }

    fn finish(&self, r: MediaResult<Source>) {
        self.with(|s| match r {
            Ok(src) => {
                s.source = Some(src);
                s.status = JobStatus::Done;
            }
            Err(e) => s.status = JobStatus::Failed(e),
        });
    }
}

/// Start a Dig job.
pub fn dig(opts: DigOptions) -> Job {
    let job = Job::new();
    let j = job.clone();
    std::thread::spawn(move || {
        let r = run_dig(&j, &opts);
        j.finish(r);
    });
    job
}

/// `n` Dig jobs at once ("dig 4"), with distinct seeds.
pub fn dig_many(n: usize, opts: DigOptions) -> Vec<Job> {
    let base = opts.seed.unwrap_or_else(seed_now);
    (0..n as u64).map(|i| dig(DigOptions { seed: Some(base.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15))), ..opts.clone() })).collect()
}

/// Start a Link job: download `url` (≤480p) and load it (whole, up to the
/// decode cap), with provenance.
pub fn fetch_link(url: &str, opts: DigOptions) -> Job {
    let job = Job::new();
    let (j, url) = (job.clone(), url.to_string());
    std::thread::spawn(move || {
        let r = run_link(&j, &url, &opts);
        j.finish(r);
    });
    job
}

fn seed_now() -> u64 {
    if let Some(s) = std::env::var(DIG_SEED_ENV).ok().and_then(|v| v.parse().ok()) {
        return s;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// Camera-default filename prefixes Dig searches for.
pub const CAMERA_PATTERNS: &[&str] = &["IMG_", "MVI_", "DSC_", "MOV_", "VID_", "DSCN", "DSCF", "GOPR", "P100", "SAM_", "100_"];
/// Plain words, for variety.
pub const WORDS: &[&str] = &[
    "untitled", "test", "my video", "birthday", "garden", "rain", "train", "beach", "kitchen", "dog", "cat", "street", "snow",
    "concert", "practice", "window", "river", "parade", "market", "bus",
];

/// A random query: mostly camera filenames (`IMG_4821`, `VID_20170913`),
/// sometimes a plain word.
pub fn random_query(rng: &mut crate::dsp::Rng) -> String {
    let r = rng.next_f32();
    if r < 0.2 {
        return WORDS[(rng.next_f32() * WORDS.len() as f32) as usize % WORDS.len()].to_string();
    }
    let p = CAMERA_PATTERNS[(rng.next_f32() * CAMERA_PATTERNS.len() as f32) as usize % CAMERA_PATTERNS.len()];
    if p == "VID_" {
        let y = 2009 + (rng.next_f32() * 15.0) as u32;
        let m = 1 + (rng.next_f32() * 12.0) as u32 % 12;
        let d = 1 + (rng.next_f32() * 28.0) as u32 % 28;
        format!("VID_{y}{m:02}{d:02}")
    } else {
        format!("{p}{:04}", (rng.next_f32() * 10_000.0) as u32 % 10_000)
    }
}

fn run_dig(_job: &Job, _opts: &DigOptions) -> MediaResult<Source> {
    Err("dig: not available yet".into())
}

fn run_link(_job: &Job, _url: &str, _opts: &DigOptions) -> MediaResult<Source> {
    Err("link: not available yet".into())
}

/// Provenance for a hit (`window` = the cut, seconds into the video).
pub fn provenance_of(hit: &VideoHit, query: &str, window: (f64, f64)) -> Provenance {
    Provenance {
        title: hit.title.clone(),
        uploader: hit.uploader.clone(),
        upload_date: hit.upload_date.clone(),
        views: hit.views,
        url: hit.url.clone(),
        license: hit.license.clone().unwrap_or_default(),
        query: query.to_string(),
        window,
    }
}
