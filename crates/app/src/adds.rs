//! Adds from the studio's Add sheet: files, Browse/Drive picks, YouTube search,
//! links and digs run as parallel background jobs. Each shows as a placeholder
//! tile (`Pending`) until its source lands; then it is added to the project and
//! reported (`Added`) with its target so the studio can place it.

use chromagrain_core::dig::{self, DigOptions, JobStatus};
use chromagrain_core::timeline::{Project, Source};
use chromagrain_ui::{AddTarget, Added, Pending, YoutubeHit};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

/// `CHROMAGRAIN_PICK=/a.mp4:/b.mov` answers the Files tab without a dialog
/// (tests, the showcase); paths are split like `$PATH`.
pub const PICK_ENV: &str = "CHROMAGRAIN_PICK";

pub enum AddMsg {
    Done(u64, Result<Source, String>, Option<PathBuf>),
}

struct Staged {
    id: u64,
    search_end: f64,
    done_at: f64,
    search_label: String,
    dl_label: String,
    res: Option<Result<Source, String>>,
    path: Option<PathBuf>,
}

pub struct Adds {
    next: u64,
    pub jobs: Vec<Pending>,
    tx: Sender<AddMsg>,
    rx: Receiver<AddMsg>,
    pub added: Vec<Added>,
    /// Dig / Link jobs (core `dig`), by placeholder id.
    jobs_net: Vec<(u64, dig::Job)>,
    /// Simulated clock for frame-locked showcase recording: jobs decode
    /// synchronously and pace their placeholder progress on film time.
    pub sim_clock: Option<f64>,
    staged: Vec<Staged>,
    sim_seen: Vec<String>,
    /// Active YouTube search results & background query state.
    pub yt_hits: Vec<YoutubeHit>,
    pub yt_searching: bool,
    pub yt_error: Option<String>,
    yt_gen: u64,
    yt_tx: Sender<(u64, Result<Vec<YoutubeHit>, String>)>,
    yt_rx: Receiver<(u64, Result<Vec<YoutubeHit>, String>)>,
    /// Optional deterministic YouTube catalog for headless tests/snapshots.
    pub yt_mock: Option<Vec<YoutubeHit>>,
    /// Remaining Dig jobs before auto-building a 16-pad kit (`Dig 4 → Kit`).
    pub dig_kit_remaining: usize,
    pub auto_kit_ready: bool,
}

impl Default for Adds {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let (yt_tx, yt_rx) = mpsc::channel();
        Adds {
            next: 1,
            jobs: Vec::new(),
            tx,
            rx,
            added: Vec::new(),
            jobs_net: Vec::new(),
            sim_clock: None,
            staged: Vec::new(),
            sim_seen: Vec::new(),
            yt_hits: Vec::new(),
            yt_searching: false,
            yt_error: None,
            yt_gen: 0,
            yt_tx,
            yt_rx,
            yt_mock: None,
            dig_kit_remaining: 0,
            auto_kit_ready: false,
        }
    }
}

/// Paths from `CHROMAGRAIN_PICK`, if set.
pub fn picked_from_env() -> Option<Vec<PathBuf>> {
    let v = std::env::var_os(PICK_ENV)?;
    Some(std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect())
}

fn run_yt_search_cli(query: &str, under_500_views: bool) -> Result<Vec<YoutubeHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let count = if under_500_views { 24 } else { 12 };
    let spec = format!("ytsearch{count}:{q}");
    let out = Command::new("yt-dlp")
        .args(["--dump-json", "--flat-playlist", "--no-warnings", "--ignore-errors", &spec])
        .output()
        .map_err(|e| format!("yt-dlp unavailable: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut hits = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let url = v
            .get("webpage_url")
            .or_else(|| v.get("url"))
            .and_then(|x| x.as_str())
            .map(|u| {
                if u.starts_with("http") {
                    u.to_string()
                } else {
                    format!("https://www.youtube.com/watch?v={u}")
                }
            })
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}"));
        let views = v.get("view_count").and_then(|x| x.as_u64());
        if under_500_views && views.is_some_and(|vw| vw >= 500) {
            continue;
        }
        let duration_secs = v.get("duration").and_then(|x| x.as_f64()).filter(|d| *d > 0.0);
        let uploader = v
            .get("channel")
            .or_else(|| v.get("uploader"))
            .and_then(|x| x.as_str())
            .unwrap_or("YouTube")
            .to_string();
        hits.push(YoutubeHit { title, url, views, duration_secs, uploader });
        if hits.len() >= 12 {
            break;
        }
    }
    Ok(hits)
}

impl Adds {
    /// A new placeholder; returns its id and a sender for the worker.
    pub fn begin(&mut self, target: AddTarget, label: &str) -> (u64, Sender<AddMsg>) {
        let id = self.next;
        self.next += 1;
        self.jobs.push(Pending { id, target, label: label.to_string(), progress: None, failed: None });
        (id, self.tx.clone())
    }

    /// Run a YouTube search query in the background (or from `yt_mock` in tests).
    pub fn search_youtube(&mut self, query: &str, under_500_views: bool) {
        let q = query.trim().to_string();
        if q.is_empty() {
            self.yt_hits.clear();
            self.yt_searching = false;
            self.yt_error = None;
            return;
        }
        self.yt_gen += 1;
        let gen = self.yt_gen;
        self.yt_error = None;
        if let Some(mock) = &self.yt_mock {
            let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
            self.yt_hits = mock
                .iter()
                .filter(|h| {
                    if under_500_views && h.views.is_some_and(|v| v >= 500) {
                        return false;
                    }
                    let hay = format!("{} {}", h.title, h.uploader).to_lowercase();
                    words.is_empty() || words.iter().any(|w| hay.contains(w.as_str())) || mock.len() <= 8
                })
                .cloned()
                .collect();
            self.yt_searching = false;
            return;
        }
        self.yt_searching = true;
        let tx = self.yt_tx.clone();
        std::thread::spawn(move || {
            let res = run_yt_search_cli(&q, under_500_views);
            let _ = tx.send((gen, res));
        });
    }

    /// Load a media file in the background.
    pub fn file(&mut self, path: PathBuf, target: AddTarget, sr: u32) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let label = format!("loading {name}");
        let (id, tx) = self.begin(target, &label);
        if let Some(t0) = self.sim_clock {
            let res = chromagrain_core::media::load_source(&path, sr);
            self.staged.push(Staged {
                id,
                search_end: t0 + 0.08,
                done_at: t0 + 0.32,
                search_label: label.clone(),
                dl_label: label,
                res: Some(res),
                path: Some(path),
            });
            return;
        }
        std::thread::spawn(move || {
            let res = chromagrain_core::media::load_source(&path, sr);
            let _ = tx.send(AddMsg::Done(id, res, Some(path)));
        });
    }

    /// Fetch a link (yt-dlp, a 20–60 s window, provenance kept).
    pub fn link(&mut self, url: String, target: AddTarget, sr: u32) {
        let (id, _) = self.begin(target, "fetching link");
        let job = dig::fetch_link(&url, DigOptions { sample_rate: sr, ..Default::default() });
        if let Some(t0) = self.sim_clock {
            let st = job.wait();
            let title = job.title().unwrap_or_else(|| "downloading".into());
            let res = match st {
                JobStatus::Done => job.take_source().ok_or_else(|| "no media".to_string()),
                JobStatus::Failed(e) => Err(e),
                _ => Err("failed".into()),
            };
            self.staged.push(Staged {
                id,
                search_end: t0 + 0.24,
                done_at: t0 + 0.88,
                search_label: "fetching link".into(),
                dl_label: title,
                res: Some(res),
                path: None,
            });
            return;
        }
        self.jobs_net.push((id, job));
    }

    /// Dig `n` random, rarely seen videos. The first lands on `target`,
    /// the rest on the next empty columns.
    pub fn dig(&mut self, n: usize, cc_only: bool, target: AddTarget, sr: u32) {
        let count = n.max(1);
        let jobs = dig::dig_many(count, DigOptions { sample_rate: sr, cc_only, ..Default::default() });
        if let Some(t0) = self.sim_clock {
            let mut extra_seed = 100u64;
            for (k, job) in jobs.into_iter().enumerate() {
                let (id, _) = self.begin(if k == 0 { target } else { AddTarget::Any }, "digging…");
                let st = job.wait();
                let mut res = match st {
                    JobStatus::Done => job.take_source().ok_or_else(|| "no media".to_string()),
                    JobStatus::Failed(e) => Err(e),
                    _ => Err("failed".into()),
                };
                while res.as_ref().is_ok_and(|s| self.sim_seen.contains(&s.name)) && extra_seed < 140 {
                    extra_seed += 1;
                    let retry = dig::dig(DigOptions { sample_rate: sr, cc_only, seed: Some(extra_seed * 7919), ..Default::default() });
                    if retry.wait() == JobStatus::Done {
                        if let Some(s) = retry.take_source() {
                            res = Ok(s);
                        }
                    }
                }
                if let Ok(s) = &res {
                    self.sim_seen.push(s.name.clone());
                }
                let query = res
                    .as_ref()
                    .ok()
                    .and_then(|s| s.provenance.as_ref())
                    .map(|p| p.query.clone())
                    .filter(|q| !q.is_empty())
                    .unwrap_or_else(|| "IMG_4821".into());
                let title = res.as_ref().ok().map(|s| s.name.clone()).unwrap_or_else(|| "downloading".into());
                let (search_end, done_at) = if count == 1 {
                    (t0 + 0.42, t0 + 1.05)
                } else {
                    (t0 + 0.50 + k as f64 * 0.22, t0 + 1.35 + k as f64 * 0.42)
                };
                self.staged.push(Staged {
                    id,
                    search_end,
                    done_at,
                    search_label: format!("digging “{query}”"),
                    dl_label: format!("digging · {title}"),
                    res: Some(res),
                    path: None,
                });
            }
            return;
        }
        for (k, job) in jobs.into_iter().enumerate() {
            let (id, _) = self.begin(if k == 0 { target } else { AddTarget::Any }, "digging…");
            self.jobs_net.push((id, job));
        }
    }

    /// Dig `n` unseen clips and auto-harvest a 16-pad collage kit when they land.
    pub fn dig_to_kit(&mut self, n: usize, cc_only: bool, target: AddTarget, sr: u32) {
        self.dig_kit_remaining = n.max(1);
        self.dig(n, cc_only, target, sr);
    }

    pub fn dismiss(&mut self, id: u64) {
        self.jobs.retain(|j| j.id != id || j.failed.is_none());
    }

    /// Drain worker messages: landed sources join the project (and are
    /// reported with their targets). Returns true when the project changed.
    pub fn poll(&mut self, project: &Mutex<Project>, imported: &mut HashMap<PathBuf, usize>) -> bool {
        let mut changed = false;
        while let Ok((gen, res)) = self.yt_rx.try_recv() {
            if gen == self.yt_gen {
                self.yt_searching = false;
                match res {
                    Ok(hits) => {
                        self.yt_hits = hits;
                        self.yt_error = None;
                    }
                    Err(e) => {
                        self.yt_hits.clear();
                        self.yt_error = Some(e);
                    }
                }
            }
        }
        if let Some(t) = self.sim_clock {
            for s in &mut self.staged {
                if t < s.search_end {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == s.id) {
                        j.label = s.search_label.clone();
                        j.progress = None;
                    }
                } else if t < s.done_at {
                    let f = ((t - s.search_end) / (s.done_at - s.search_end).max(1e-3)).clamp(0.0, 1.0) as f32;
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == s.id) {
                        j.label = s.dl_label.clone();
                        j.progress = Some(f);
                    }
                } else if let Some(res) = s.res.take() {
                    let _ = self.tx.send(AddMsg::Done(s.id, res, s.path.take()));
                }
            }
            self.staged.retain(|s| s.res.is_some());
        }
        // Dig / Link jobs: status -> placeholder; done -> a source.
        let mut finished = Vec::new();
        for (id, job) in &self.jobs_net {
            let st = job.status();
            let title = job.title();
            let (label, progress) = match &st {
                JobStatus::Queued => ("waiting…".to_string(), None),
                JobStatus::Searching { query, attempt } => (if *attempt > 1 { format!("digging “{query}” ({attempt})") } else { format!("digging “{query}”") }, None),
                JobStatus::Downloading(f) => (title.clone().unwrap_or_else(|| "downloading".into()), Some(*f)),
                JobStatus::Cutting => (title.clone().map(|t| format!("cutting {t}")).unwrap_or_else(|| "cutting a window".into()), Some(1.0)),
                JobStatus::Done => ("done".into(), Some(1.0)),
                JobStatus::Failed(e) => (e.clone(), None),
            };
            if let Some(j) = self.jobs.iter_mut().find(|j| j.id == *id) {
                j.label = label;
                j.progress = progress;
            }
            match st {
                JobStatus::Done => finished.push((*id, job.take_source().ok_or_else(|| "no media".to_string()))),
                JobStatus::Failed(e) => finished.push((*id, Err(e))),
                _ => {}
            }
        }
        for (id, res) in finished {
            self.jobs_net.retain(|(j, _)| *j != id);
            let _ = self.tx.send(AddMsg::Done(id, res, None));
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                AddMsg::Done(id, res, path) => {
                    let Some(k) = self.jobs.iter().position(|j| j.id == id) else { continue };
                    match res {
                        Ok(src) => {
                            let target = self.jobs.remove(k).target;
                            let reuse = path.as_ref().and_then(|p| imported.get(p).copied());
                            let source = match reuse {
                                Some(s) => s,
                                None => {
                                    let s = project.lock().unwrap().add_source(src);
                                    if let Some(p) = path {
                                        imported.insert(p, s);
                                    }
                                    s
                                }
                            };
                            self.added.push(Added { source, target });
                            if self.dig_kit_remaining > 0 {
                                self.dig_kit_remaining -= 1;
                                if self.dig_kit_remaining == 0 {
                                    self.auto_kit_ready = true;
                                }
                            }
                            changed = true;
                        }
                        Err(e) => {
                            let j = &mut self.jobs[k];
                            j.failed = Some(e.lines().next().unwrap_or("failed").to_string());
                            j.progress = None;
                            if self.dig_kit_remaining > 0 {
                                self.dig_kit_remaining -= 1;
                                if self.dig_kit_remaining == 0 {
                                    self.auto_kit_ready = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        changed
    }
}
