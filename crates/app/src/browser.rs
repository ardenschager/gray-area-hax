//! Browse: Google Drive and local folders on this machine as one sample library.
//!
//! * Google Drive mounts (`~/Library/CloudStorage/GoogleDrive-*`, `/Volumes/GoogleDrive*`)
//!   and local video folders (`Documents`, `Movies`, `Downloads`, `Desktop`, …)
//!   are discovered in the background and grouped into collections.
//! * Strictly 100% deterministic local & Drive discovery — zero AI or cloud API calls.
//! * Thumbnails are grabbed with ffmpeg by a small worker pool and cached
//!   on disk, so the second visit is instant.

use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

pub const THUMB_W: usize = 160;
pub const THUMB_H: usize = 90;
const VIDEO_EXTS: [&str; 7] = ["mp4", "mov", "webm", "mkv", "m4v", "avi", "gif"];
const SKIP_DIRS: [&str; 14] = [
    "node_modules", "target", "dist", "build", "Library", "venv", ".venv", "site-packages",
    "__pycache__", "dist-deploy", "Pods", "DerivedData", "Applications", "cache",
];

#[derive(Debug, Clone)]
pub struct BrowseClip {
    pub path: PathBuf,
    pub title: String,
    /// The reel / source film this clip was cut from (groups siblings).
    pub reel: String,
    pub desc: String,
    pub dur: f32,
    /// Lowercased haystack for word filtering.
    pub(crate) hay: String,
}

pub struct Collection {
    pub name: String,
    pub clips: Vec<BrowseClip>,
    pub is_drive: bool,
}

enum ScanMsg {
    Collection(Collection),
    Done,
}

enum Thumb {
    Pending,
    Ready(egui::TextureHandle),
    Failed,
}

struct ThumbQueue {
    stack: Mutex<Vec<PathBuf>>,
    cv: Condvar,
}

pub struct Browser {
    pub collections: Vec<Collection>,
    pub scanning: bool,
    scan_rx: Option<Receiver<ScanMsg>>,
    pub current: usize,
    pub query: String,
    /// Indices into the current collection, in display order.
    pub results: Vec<usize>,
    results_key: Option<(usize, String)>,
    pub selected: Option<PathBuf>,
    thumbs: HashMap<PathBuf, Thumb>,
    queue: Arc<ThumbQueue>,
    done_rx: Receiver<(PathBuf, Option<Vec<u8>>)>,
    started: bool,
}

impl Browser {
    pub fn new() -> Browser {
        let queue = Arc::new(ThumbQueue { stack: Mutex::new(Vec::new()), cv: Condvar::new() });
        let (done_tx, done_rx) = mpsc::channel();
        for _ in 0..4 {
            let q = queue.clone();
            let tx: Sender<_> = done_tx.clone();
            std::thread::spawn(move || thumb_worker(q, tx));
        }
        Browser {
            collections: Vec::new(),
            scanning: false,
            scan_rx: None,
            current: 0,
            query: String::new(),
            results: Vec::new(),
            results_key: None,
            selected: None,
            thumbs: HashMap::new(),
            queue,
            done_rx,
            started: false,
        }
    }

    /// Kick off discovery the first time the browser is opened.
    pub fn ensure_started(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.scanning = true;
        let (tx, rx) = mpsc::channel();
        self.scan_rx = Some(rx);
        std::thread::spawn(move || scan(tx));
    }

    pub fn total_clips(&self) -> usize {
        self.collections.iter().map(|c| c.clips.len()).sum()
    }

    pub fn current(&self) -> Option<&Collection> {
        self.collections.get(self.current)
    }

    /// Drain background results; call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.scan_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    ScanMsg::Collection(c) => {
                        let cur_name = self.current().map(|c| c.name.clone());
                        self.collections.push(c);
                        // Google Drive collections first, then biggest folders.
                        self.collections.sort_by(|a, b| {
                            b.is_drive.cmp(&a.is_drive).then(b.clips.len().cmp(&a.clips.len()))
                        });
                        if let Some(n) = cur_name {
                            self.current = self.collections.iter().position(|c| c.name == n).unwrap_or(0);
                        }
                        self.results_key = None;
                    }
                    ScanMsg::Done => self.scanning = false,
                }
            }
        }
        let mut got = false;
        while let Ok((path, px)) = self.done_rx.try_recv() {
            got = true;
            let t = match px {
                Some(px) if px.len() == THUMB_W * THUMB_H * 4 => {
                    let img = egui::ColorImage::from_rgba_unmultiplied([THUMB_W, THUMB_H], &px);
                    Thumb::Ready(ctx.load_texture(
                        format!("bt:{}", path.display()),
                        img,
                        egui::TextureOptions::LINEAR,
                    ))
                }
                _ => Thumb::Failed,
            };
            self.thumbs.insert(path, t);
        }
        if got || self.scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    pub fn refresh_results(&mut self) {
        let key = (self.current, self.query.clone());
        if self.results_key.as_ref() == Some(&key) {
            return;
        }
        self.results_key = Some(key);
        let Some(col) = self.collections.get(self.current) else {
            self.results.clear();
            return;
        };
        let words: Vec<String> =
            self.query.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
        self.results = col
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| words.iter().all(|w| c.hay.contains(w.as_str())))
            .map(|(i, _)| i)
            .collect();
    }

    /// Shuffle what's showing (Fisher–Yates, time-seeded xorshift).
    pub fn shuffle(&mut self) {
        self.refresh_results();
        let mut s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9)
            | 1;
        for i in (1..self.results.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            self.results.swap(i, (s % (i as u64 + 1)) as usize);
        }
    }

    /// Find a clip in the current collection by path.
    pub fn clip_by_path(&self, path: &Path) -> Option<&BrowseClip> {
        self.current()?.clips.iter().find(|c| c.path == path)
    }

    /// Texture for a clip thumbnail, queueing generation if needed.
    pub fn thumb(&mut self, path: &Path) -> Option<egui::TextureId> {
        match self.thumbs.get(path) {
            Some(Thumb::Ready(t)) => Some(t.id()),
            Some(Thumb::Pending) | Some(Thumb::Failed) => None,
            None => {
                self.thumbs.insert(path.to_path_buf(), Thumb::Pending);
                let mut st = self.queue.stack.lock().unwrap();
                st.push(path.to_path_buf());
                // Newest requests (what's on screen now) win; forget stale ones.
                if st.len() > 160 {
                    let excess = st.len() - 160;
                    let stale: Vec<PathBuf> = st.drain(..excess).collect();
                    drop(st);
                    for p in stale {
                        self.thumbs.remove(&p);
                    }
                } else {
                    drop(st);
                }
                self.queue.cv.notify_one();
                None
            }
        }
    }
}

// ------------------------------------------------------------ thumbnails

fn cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join("Library/Caches/chromagrain/thumbs")
}

fn cache_path(path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(m) = std::fs::metadata(path) {
        m.len().hash(&mut h);
    }
    cache_dir().join(format!("{:016x}.rgba", h.finish()))
}

fn thumb_worker(q: Arc<ThumbQueue>, tx: Sender<(PathBuf, Option<Vec<u8>>)>) {
    loop {
        let path = {
            let mut st = q.stack.lock().unwrap();
            loop {
                if let Some(p) = st.pop() {
                    break p;
                }
                st = q.cv.wait(st).unwrap();
            }
        };
        let px = make_thumb(&path);
        if tx.send((path, px)).is_err() {
            return;
        }
    }
}

fn make_thumb(path: &Path) -> Option<Vec<u8>> {
    let cached = cache_path(path);
    if let Ok(px) = std::fs::read(&cached) {
        if px.len() == THUMB_W * THUMB_H * 4 {
            return Some(px);
        }
    }
    // Grab a frame a little way in (skips black leaders), letterboxed.
    let vf = format!(
        "scale={THUMB_W}:{THUMB_H}:force_original_aspect_ratio=decrease,pad={THUMB_W}:{THUMB_H}:(ow-iw)/2:(oh-ih)/2"
    );
    let run = |seek: &str| {
        std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-ss", seek, "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .map(|o| o.stdout)
            .filter(|b| b.len() == THUMB_W * THUMB_H * 4)
    };
    let px = run("1.0").or_else(|| run("0"))?;
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = std::fs::write(&cached, &px);
    Some(px)
}

// ------------------------------------------------------------ discovery

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTS.iter().any(|v| e.eq_ignore_ascii_case(v)))
}

/// "tv_superman_full_clip_0084" -> "tv_superman_full_clip"
fn reel_of(stem: &str) -> String {
    match stem.rsplit_once(['_', '-', ' ']) {
        Some((head, tail)) if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head.to_string(),
        _ => stem.to_string(),
    }
}

fn prettify(s: &str) -> String {
    s.replace(['_', '-'], " ")
}

/// `CHROMAGRAIN_BROWSE_ROOT=/dir` scans only that folder (tests, the
/// showcase's fixture library) instead of the home folders.
pub const BROWSE_ROOT_ENV: &str = "CHROMAGRAIN_BROWSE_ROOT";

/// `CHROMAGRAIN_DRIVE_ROOT=/dir` adds a Google Drive fixture folder in tests.
pub const DRIVE_ROOT_ENV: &str = "CHROMAGRAIN_DRIVE_ROOT";

/// Discover mounted Google Drive roots on macOS (`~/Library/CloudStorage/GoogleDrive-*`
/// and `/Volumes/GoogleDrive*`).
pub fn drive_roots() -> Vec<PathBuf> {
    if let Some(override_root) = std::env::var_os(DRIVE_ROOT_ENV).map(PathBuf::from) {
        if override_root.exists() {
            return vec![override_root];
        }
    }
    let mut out = Vec::new();
    let cloud = home().join("Library/CloudStorage");
    if let Ok(rd) = std::fs::read_dir(&cloud) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("GoogleDrive") {
                let p = e.path();
                let my_drive = p.join("My Drive");
                if my_drive.is_dir() {
                    out.push(my_drive);
                } else if p.is_dir() {
                    out.push(p);
                }
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("GoogleDrive") || name == "Google Drive" {
                let p = e.path();
                if p.is_dir() && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn build_collection(name: String, mut files: Vec<PathBuf>, is_drive: bool) -> Collection {
    files.sort();
    let clips = files
        .into_iter()
        .map(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let reel = prettify(&reel_of(&stem));
            let title = prettify(&stem);
            let drive_tag = if is_drive { "drive google" } else { "local" };
            let hay = format!("{title} {reel} {name} {drive_tag}").to_lowercase();
            BrowseClip { path: p, title, reel, desc: String::new(), dur: 0.0, hay }
        })
        .collect();
    Collection { name, clips, is_drive }
}

fn scan(tx: Sender<ScanMsg>) {
    let home = home();
    let projects = home.join("Documents/Projects");
    let mut covered: HashSet<PathBuf> = HashSet::new();

    if let Some(root) = std::env::var_os(BROWSE_ROOT_ENV).map(PathBuf::from) {
        // Also scan explicit test Drive root if set alongside BROWSE_ROOT_ENV.
        if let Some(droot) = std::env::var_os(DRIVE_ROOT_ENV).map(PathBuf::from) {
            let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
            walk(&droot, 0, &covered, &mut by_dir, true);
            let mut dirs: Vec<(PathBuf, Vec<PathBuf>)> = by_dir.into_iter().collect();
            dirs.sort();
            for (dir, files) in dirs {
                let sub = dir.strip_prefix(&droot).ok().filter(|r| !r.as_os_str().is_empty());
                let name = match sub {
                    Some(r) => format!("Drive · {}", r.display()),
                    None => "Drive".into(),
                };
                let _ = tx.send(ScanMsg::Collection(build_collection(name, files, true)));
            }
            covered.insert(droot);
        }
        let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        walk(&root, 0, &covered, &mut by_dir, false);
        let mut dirs: Vec<(PathBuf, Vec<PathBuf>)> = by_dir.into_iter().collect();
        dirs.sort();
        for (dir, files) in dirs {
            let name = dir.strip_prefix(&root).ok().filter(|r| !r.as_os_str().is_empty()).unwrap_or(&dir).display().to_string();
            let _ = tx.send(ScanMsg::Collection(build_collection(name, files, false)));
        }
        let _ = tx.send(ScanMsg::Done);
        return;
    }

    // 1. Google Drive roots (`~/Library/CloudStorage/GoogleDrive-*`, `/Volumes/GoogleDrive*`).
    for droot in drive_roots() {
        let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        walk(&droot, 0, &covered, &mut by_dir, true);
        covered.insert(droot.clone());
        let mut dirs: Vec<(PathBuf, Vec<PathBuf>)> = by_dir.into_iter().collect();
        dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (dir, files) in dirs {
            let sub = dir.strip_prefix(&droot).ok().filter(|r| !r.as_os_str().is_empty());
            let name = match sub {
                Some(r) => format!("Drive · {}", r.display()),
                None => "Drive".into(),
            };
            let _ = tx.send(ScanMsg::Collection(build_collection(name, files, true)));
        }
    }

    // 2. Local folders under `$HOME` (`Sync` included for Cloudtop<->Mac synced clips).
    let roots = ["Documents", "Movies", "Downloads", "Desktop", "Pictures", "Screencaps"];
    let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for r in roots {
        walk(&home.join(r), 0, &covered, &mut by_dir, false);
    }
    let mut dirs: Vec<(PathBuf, Vec<PathBuf>)> = by_dir.into_iter().collect();
    dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (dir, files) in dirs {
        let rel = dir.strip_prefix(&projects).or_else(|_| dir.strip_prefix(&home)).unwrap_or(&dir);
        let name = rel.display().to_string();
        let _ = tx.send(ScanMsg::Collection(build_collection(name, files, false)));
    }
    let _ = tx.send(ScanMsg::Done);
}

fn walk(
    dir: &Path,
    depth: usize,
    covered: &HashSet<PathBuf>,
    out: &mut HashMap<PathBuf, Vec<PathBuf>>,
    allow_library: bool,
) {
    if depth > 7 || covered.contains(dir) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            let skip = SKIP_DIRS.iter().any(|s| name == *s && !(allow_library && *s == "Library"));
            if skip || name.ends_with(".app") || name.ends_with(".photoslibrary") {
                continue;
            }
            walk(&p, depth + 1, covered, out, allow_library);
        } else if ft.is_file() && is_video(&p) {
            out.entry(dir.to_path_buf()).or_default().push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reels_group_numbered_siblings() {
        assert_eq!(reel_of("tv_superman_full_clip_0084"), "tv_superman_full_clip");
        assert_eq!(reel_of("clip_0003"), "clip");
        assert_eq!(reel_of("sunset"), "sunset");
        assert_eq!(reel_of("take 12"), "take");
        assert_eq!(reel_of("0012"), "0012");
    }

    #[test]
    fn video_extensions() {
        assert!(is_video(Path::new("a/b.MP4")));
        assert!(is_video(Path::new("x.webm")));
        assert!(!is_video(Path::new("x.ts")));
        assert!(!is_video(Path::new("x")));
    }
}
