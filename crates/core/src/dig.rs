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

    fn set(&self, status: JobStatus) {
        self.with(|s| s.status = status);
    }

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

fn cancelled(job: &Job) -> MediaResult<()> {
    if job.is_cancelled() {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

/// Fill in fields the flat search left out from a full lookup.
fn merge(flat: &VideoHit, full: VideoHit) -> VideoHit {
    let pick = |a: &String, b: String| if b.is_empty() { a.clone() } else { b };
    VideoHit {
        id: pick(&flat.id, full.id),
        url: pick(&flat.url, full.url),
        title: pick(&flat.title, full.title),
        uploader: pick(&flat.uploader, full.uploader),
        upload_date: pick(&flat.upload_date, full.upload_date),
        views: full.views.or(flat.views),
        duration: full.duration.or(flat.duration),
        live: full.live || flat.live,
        license: full.license.or_else(|| flat.license.clone()),
    }
}

fn source_name(hit: &VideoHit, path: &std::path::Path) -> String {
    let t = hit.title.trim();
    if t.is_empty() {
        path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "dig".into())
    } else {
        t.chars().take(80).collect()
    }
}

fn run_dig(job: &Job, opts: &DigOptions) -> MediaResult<Source> {
    let backend = crate::media::backend();
    let mut rng = crate::dsp::Rng::new(opts.seed.unwrap_or_else(seed_now) | 1);
    let mut last_err = String::from("nothing passed the filter");
    for attempt in 1..=opts.attempts.max(1) {
        cancelled(job)?;
        let query = random_query(&mut rng);
        job.set(JobStatus::Searching { query: query.clone(), attempt });
        let hits = match backend.search_videos(&query, opts.results) {
            Ok(h) => h,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        let mut candidates: Vec<VideoHit> = hits.into_iter().filter(|h| opts.accepts(h)).collect();
        // Fisher-Yates with the job's rng: a random pick, then fallbacks.
        for i in (1..candidates.len()).rev() {
            let j = (rng.next_f32() * (i + 1) as f32) as usize % (i + 1);
            candidates.swap(i, j);
        }
        for hit in candidates.into_iter().take(3) {
            cancelled(job)?;
            let hit = if opts.cc_only || hit.upload_date.is_empty() || hit.license.is_none() {
                match backend.video_info(&hit.url) {
                    Ok(full) => merge(&hit, full),
                    Err(e) if opts.cc_only => {
                        last_err = e;
                        continue;
                    }
                    Err(_) => hit,
                }
            } else {
                hit
            };
            if !opts.accepts(&hit) || (opts.cc_only && !hit.is_creative_commons()) {
                continue;
            }
            job.set_title(&hit.title);
            job.set(JobStatus::Downloading(0.0));
            let path = match backend.download_video(&hit.url, &opts.cache_dir, 480, &mut |p| job.set(JobStatus::Downloading(p))) {
                Ok(p) => p,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            cancelled(job)?;
            job.set(JobStatus::Cutting);
            let dur = hit.duration.unwrap_or(opts.window.1).max(0.0);
            let (w0, w1) = (opts.window.0.min(opts.window.1).max(0.1), opts.window.0.max(opts.window.1).max(0.1));
            let len = (w0 + (w1 - w0) * rng.next_f32() as f64).min(dur.max(0.1));
            let start = (dur - len).max(0.0) * rng.next_f32() as f64;
            match crate::media::load_source_window(&path, start, len, opts.sample_rate) {
                Ok(mut src) => {
                    src.name = source_name(&hit, &path);
                    src.provenance = Some(provenance_of(&hit, &query, (start, len)));
                    return Ok(src);
                }
                Err(e) => last_err = e,
            }
        }
    }
    Err(format!("dig: gave up after {} queries ({last_err})", opts.attempts.max(1)))
}

fn run_link(job: &Job, url: &str, opts: &DigOptions) -> MediaResult<Source> {
    let backend = crate::media::backend();
    job.set(JobStatus::Searching { query: url.to_string(), attempt: 1 });
    let hit = match backend.video_info(url) {
        Ok(h) => h,
        Err(_) => VideoHit { url: url.to_string(), ..Default::default() },
    };
    if !hit.title.is_empty() {
        job.set_title(&hit.title);
    }
    cancelled(job)?;
    job.set(JobStatus::Downloading(0.0));
    let path = backend.download_video(url, &opts.cache_dir, 480, &mut |p| job.set(JobStatus::Downloading(p)))?;
    cancelled(job)?;
    job.set(JobStatus::Cutting);
    let mut src = crate::media::load_source(&path, opts.sample_rate)?;
    src.name = source_name(&hit, &path);
    let len = src.duration();
    src.provenance = Some(provenance_of(&VideoHit { url: if hit.url.is_empty() { url.to_string() } else { hit.url.clone() }, ..hit }, "", (0.0, len)));
    Ok(src)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    /// The stub is selected through process-wide env vars: one dig test at a time.
    static ENV: Mutex<()> = Mutex::new(());

    fn stub() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ytdlp-stub")
    }

    fn write_wav(path: &Path, secs: u32) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..secs * 8000 {
            w.write_sample(((i as f32 * 0.07).sin() * 9000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn flat(id: &str, views: u64, dur: f64, live: bool) -> String {
        format!(r#"{{"id":"{id}","url":"https://www.youtube.com/watch?v={id}","title":"flat {id}","channel":"ch {id}","view_count":{views},"duration":{dur},"live_status":"{}"}}"#,
            if live { "is_live" } else { "not_live" })
    }

    fn info(id: &str, views: u64, dur: f64, license: &str) -> String {
        format!(r#"{{"id":"{id}","webpage_url":"https://www.youtube.com/watch?v={id}","title":"IMG_{id} at the lake","uploader":"someone {id}","upload_date":"20140612","view_count":{views},"duration":{dur},"license":"{license}"}}"#)
    }

    /// A finds folder: `hits` for search, plus metadata + media per good id.
    fn finds(name: &str, hits: &[String], good: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cg-dig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("search.jsonl"), hits.join("\n") + "\n").unwrap();
        for (id, lic) in good {
            std::fs::write(dir.join(format!("{id}.json")), info(id, 42, 70.0, lic)).unwrap();
            write_wav(&dir.join(format!("{id}.wav")), 70);
        }
        dir
    }

    fn with_stub<R>(dir: &Path, f: impl FnOnce() -> R) -> R {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(YTDLP_ENV, stub());
        std::env::set_var("CHROMAGRAIN_YTDLP_STUB_DIR", dir);
        let r = f();
        std::env::remove_var(YTDLP_ENV);
        std::env::remove_var("CHROMAGRAIN_YTDLP_STUB_DIR");
        r
    }

    fn opts(dir: &Path, seed: u64) -> DigOptions {
        DigOptions { seed: Some(seed), sample_rate: 8000, cache_dir: dir.join("cache"), attempts: 4, ..Default::default() }
    }

    #[test]
    fn queries_look_like_camera_files_or_words() {
        let mut rng = crate::dsp::Rng::new(7);
        let qs: Vec<String> = (0..200).map(|_| random_query(&mut rng)).collect();
        let camera = qs.iter().filter(|q| CAMERA_PATTERNS.iter().any(|p| q.starts_with(p))).count();
        assert!(camera > 120 && camera < 200, "mostly camera filenames: {camera}/200");
        assert!(qs.iter().any(|q| WORDS.contains(&q.as_str())), "sometimes a word");
        for q in qs.iter().filter(|q| q.starts_with("VID_")) {
            assert_eq!(q.len(), 12, "VID_YYYYMMDD: {q}");
        }
    }

    #[test]
    fn match_filter() {
        let o = DigOptions::default();
        let hit = |views, dur, live| VideoHit { views: Some(views), duration: Some(dur), live, ..Default::default() };
        assert!(o.accepts(&hit(10, 90.0, false)));
        assert!(!o.accepts(&hit(10_000, 90.0, false)), "too popular");
        assert!(!o.accepts(&hit(10, 4.0, false)), "too short");
        assert!(!o.accepts(&hit(10, 3600.0, false)), "too long");
        assert!(!o.accepts(&hit(10, 90.0, true)), "live");
        assert!(!o.accepts(&VideoHit { duration: None, ..hit(10, 90.0, false) }), "unknown duration");
        let cc = DigOptions { cc_only: true, ..o };
        let lic = |l: &str| VideoHit { license: Some(l.into()), ..hit(10, 90.0, false) };
        assert!(cc.accepts(&lic("Creative Commons Attribution license (reuse allowed)")));
        assert!(!cc.accepts(&lic("Standard YouTube License")));
    }

    #[test]
    fn dig_picks_an_unseen_video_and_cuts_a_window() {
        let hits = [flat("popular", 90_000, 70.0, false), flat("livecam", 3, 70.0, true), flat("short1", 2, 3.0, false), flat("good1", 42, 70.0, false)];
        let dir = finds("pick", &hits, &[("good1", "")]);
        let src = with_stub(&dir, || {
            let job = dig(opts(&dir, 11));
            assert_eq!(job.wait(), JobStatus::Done);
            assert_eq!(job.progress(), 1.0);
            assert_eq!(job.title().as_deref(), Some("IMG_good1 at the lake"));
            job.take_source().expect("source")
        });
        let p = src.provenance.clone().expect("provenance");
        assert_eq!(p.url, "https://www.youtube.com/watch?v=good1");
        assert_eq!((p.views, p.uploader.as_str(), p.upload_date.as_str()), (Some(42), "someone good1", "20140612"));
        assert!(!p.query.is_empty());
        let (start, len) = p.window;
        assert!((20.0..=60.0).contains(&len) && start >= 0.0 && start + len <= 70.0 + 1e-9, "window {start}+{len}");
        assert!((src.duration() - len).abs() < 0.01, "source is the window: {} vs {len}", src.duration());
        assert_eq!(src.name, "IMG_good1 at the lake");
        assert!(dir.join("cache/good1.wav").exists(), "downloaded into the cache dir");
    }

    #[test]
    fn dig_retries_when_searches_come_back_empty() {
        let dir = finds("retry", &[flat("good1", 42, 70.0, false)], &[("good1", "")]);
        std::fs::write(dir.join("empty_searches"), "2").unwrap();
        let st = with_stub(&dir, || dig(opts(&dir, 3)).wait());
        assert_eq!(st, JobStatus::Done);
        assert_eq!(std::fs::read_to_string(dir.join(".calls")).unwrap().trim(), "3", "two empty searches, then a hit");
    }

    #[test]
    fn dig_gives_up_when_nothing_passes() {
        let dir = finds("none", &[flat("popular", 90_000, 70.0, false)], &[]);
        let st = with_stub(&dir, || dig(DigOptions { attempts: 2, ..opts(&dir, 5) }).wait());
        match st {
            JobStatus::Failed(e) => assert!(e.contains("gave up after 2"), "{e}"),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[test]
    fn cc_only_keeps_creative_commons_finds() {
        let hits = [flat("plain", 10, 70.0, false), flat("ccfind", 12, 70.0, false)];
        let dir = finds("cc", &hits, &[("plain", "Standard YouTube License"), ("ccfind", "Creative Commons Attribution license (reuse allowed)")]);
        for seed in 1..5 {
            let src = with_stub(&dir, || {
                let job = dig(DigOptions { cc_only: true, ..opts(&dir, seed) });
                assert_eq!(job.wait(), JobStatus::Done);
                job.take_source().unwrap()
            });
            let p = src.provenance.unwrap();
            assert!(p.url.ends_with("ccfind"), "seed {seed}: {}", p.url);
            assert!(p.license.contains("Creative Commons"));
        }
    }

    #[test]
    fn dig_many_runs_jobs_with_distinct_seeds() {
        let hits = [flat("good1", 42, 70.0, false), flat("good2", 7, 70.0, false)];
        let dir = finds("many", &hits, &[("good1", ""), ("good2", "")]);
        let windows: Vec<(f64, f64)> = with_stub(&dir, || {
            let jobs = dig_many(3, opts(&dir, 99));
            jobs.iter().map(|j| {
                assert_eq!(j.wait(), JobStatus::Done);
                j.take_source().unwrap().provenance.unwrap().window
            }).collect()
        });
        assert_eq!(windows.len(), 3);
        assert!(windows[0] != windows[1] || windows[1] != windows[2], "different random windows: {windows:?}");
    }

    #[test]
    fn link_downloads_and_loads_with_provenance() {
        let dir = finds("link", &[], &[("good1", "")]);
        let src = with_stub(&dir, || {
            let job = fetch_link("https://www.youtube.com/watch?v=good1", opts(&dir, 1));
            assert_eq!(job.wait(), JobStatus::Done);
            job.take_source().unwrap()
        });
        let p = src.provenance.clone().unwrap();
        assert_eq!((p.title.as_str(), p.query.as_str()), ("IMG_good1 at the lake", ""));
        assert!((p.window.1 - 70.0).abs() < 0.01 && (src.duration() - 70.0).abs() < 0.01);
    }

    #[test]
    fn link_failure_is_reported() {
        let dir = finds("linkfail", &[], &[]);
        let st = with_stub(&dir, || fetch_link("https://www.youtube.com/watch?v=missing", opts(&dir, 1)).wait());
        assert!(matches!(st, JobStatus::Failed(ref e) if e.contains("no media")), "{st:?}");
    }

    /// Real network dig (yt-dlp + ffmpeg on PATH):
    /// `cargo test -p chromagrain-core dig::tests::real_dig -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_dig() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let job = dig(DigOptions { attempts: 8, ..Default::default() });
        let st = job.wait();
        eprintln!("{st:?}");
        let src = job.take_source().expect("a real find");
        eprintln!("{} {:.1}s {:?}", src.name, src.duration(), src.provenance);
        assert!(src.duration() >= 1.0);
    }
}
