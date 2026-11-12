//! Adds from the studio's Add sheet: files, Browse picks, links and digs
//! run as parallel background jobs. Each shows as a placeholder tile
//! (`Pending`) until its source lands; then it is added to the project and
//! reported (`Added`) with its target so the studio can place it.

use chromagrain_core::dig::{self, DigOptions, JobStatus};
use chromagrain_core::timeline::{Project, Source};
use chromagrain_ui::{AddTarget, Added, Pending};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

/// `CHROMAGRAIN_PICK=/a.mp4:/b.mov` answers the Files tab without a dialog
/// (tests, the showcase); paths are split like `$PATH`.
pub const PICK_ENV: &str = "CHROMAGRAIN_PICK";

pub enum AddMsg {
    Done(u64, Result<Source, String>, Option<PathBuf>),
}

pub struct Adds {
    next: u64,
    pub jobs: Vec<Pending>,
    tx: Sender<AddMsg>,
    rx: Receiver<AddMsg>,
    pub added: Vec<Added>,
    /// Dig / Link jobs (core `dig`), by placeholder id.
    jobs_net: Vec<(u64, dig::Job)>,
}

impl Default for Adds {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Adds { next: 1, jobs: Vec::new(), tx, rx, added: Vec::new(), jobs_net: Vec::new() }
    }
}

/// Paths from `CHROMAGRAIN_PICK`, if set.
pub fn picked_from_env() -> Option<Vec<PathBuf>> {
    let v = std::env::var_os(PICK_ENV)?;
    Some(std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect())
}

impl Adds {
    /// A new placeholder; returns its id and a sender for the worker.
    pub fn begin(&mut self, target: AddTarget, label: &str) -> (u64, Sender<AddMsg>) {
        let id = self.next;
        self.next += 1;
        self.jobs.push(Pending { id, target, label: label.to_string(), progress: None, failed: None });
        (id, self.tx.clone())
    }

    /// Load a media file in the background.
    pub fn file(&mut self, path: PathBuf, target: AddTarget, sr: u32) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (id, tx) = self.begin(target, &format!("loading {name}"));
        std::thread::spawn(move || {
            let res = chromagrain_core::media::load_source(&path, sr);
            let _ = tx.send(AddMsg::Done(id, res, Some(path)));
        });
    }

    /// Fetch a link (yt-dlp, a 20–60 s window, provenance kept).
    pub fn link(&mut self, url: String, target: AddTarget, sr: u32) {
        let (id, _) = self.begin(target, "fetching link");
        let job = dig::fetch_link(&url, DigOptions { sample_rate: sr, ..Default::default() });
        self.jobs_net.push((id, job));
    }

    /// Dig `n` random, rarely seen videos. The first lands on `target`,
    /// the rest on the next empty columns.
    pub fn dig(&mut self, n: usize, cc_only: bool, target: AddTarget, sr: u32) {
        let jobs = dig::dig_many(n.max(1), DigOptions { sample_rate: sr, cc_only, ..Default::default() });
        for (k, job) in jobs.into_iter().enumerate() {
            let (id, _) = self.begin(if k == 0 { target } else { AddTarget::Any }, "digging…");
            self.jobs_net.push((id, job));
        }
    }

    pub fn dismiss(&mut self, id: u64) {
        self.jobs.retain(|j| j.id != id || j.failed.is_none());
    }

    /// Drain worker messages: landed sources join the project (and are
    /// reported with their targets). Returns true when the project changed.
    pub fn poll(&mut self, project: &Mutex<Project>, imported: &mut HashMap<PathBuf, usize>) -> bool {
        let mut changed = false;
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
                            changed = true;
                        }
                        Err(e) => {
                            let j = &mut self.jobs[k];
                            j.failed = Some(e.lines().next().unwrap_or("failed").to_string());
                            j.progress = None;
                        }
                    }
                }
            }
        }
        changed
    }
}
