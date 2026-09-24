//! Browse: every video on this machine as one big sample library.
//!
//! * Folders of videos under your home (Projects, Movies, Downloads, …)
//!   are discovered in the background and grouped into collections.
//! * Projects that ship a semantic corpus (`public/corpus.json` +
//!   `public/vectors.bin`, e.g. video-grid) become searchable *by meaning*:
//!   the query is embedded with the same model the corpus was built with
//!   (read from corpus.json) and ranked by cosine similarity.
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
    hay: String,
}

pub struct Collection {
    pub name: String,
    pub clips: Vec<BrowseClip>,
    pub semantic: Option<Semantic>,
}

pub struct Semantic {
    pub model: String,
    pub dim: usize,
    /// Row-major, one normalized vector per clip (same order as `clips`).
    pub vectors: Vec<f32>,
    pub project_dir: PathBuf,
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
    results_key: Option<(usize, String, bool)>,
    /// Semantic ranking for (collection, query), best first.
    ranked: Option<(usize, String, Vec<usize>)>,
    pub semantic_busy: bool,
    sem_rx: Option<Receiver<(usize, String, Result<Vec<f32>, String>)>>,
    pub last_error: Option<String>,
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
            ranked: None,
            semantic_busy: false,
            sem_rx: None,
            last_error: None,
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
                        // Semantic corpora first, then biggest folders.
                        self.collections.sort_by(|a, b| {
                            b.semantic.is_some().cmp(&a.semantic.is_some()).then(b.clips.len().cmp(&a.clips.len()))
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
        if let Some(rx) = &self.sem_rx {
            if let Ok((ci, q, res)) = rx.try_recv() {
                self.semantic_busy = false;
                match res {
                    Ok(qv) => {
                        if let Some(sem) = self.collections.get(ci).and_then(|c| c.semantic.as_ref()) {
                            let mut scored: Vec<(usize, f32)> = sem
                                .vectors
                                .chunks_exact(sem.dim)
                                .enumerate()
                                .map(|(i, v)| (i, v.iter().zip(&qv).map(|(a, b)| a * b).sum()))
                                .collect();
                            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
                            let top = scored.into_iter().take(300).map(|(i, _)| i).collect();
                            self.ranked = Some((ci, q, top));
                            self.results_key = None;
                            self.last_error = None;
                        }
                    }
                    Err(e) => self.last_error = Some(e),
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
        if got || self.scanning || self.semantic_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    /// Is the current query ranked by meaning (vs. word filtered)?
    pub fn is_semantic_view(&self) -> bool {
        matches!(&self.ranked, Some((ci, q, _)) if *ci == self.current && *q == self.query)
    }

    pub fn refresh_results(&mut self) {
        let sem = self.is_semantic_view();
        let key = (self.current, self.query.clone(), sem);
        if self.results_key.as_ref() == Some(&key) {
            return;
        }
        self.results_key = Some(key);
        let Some(col) = self.collections.get(self.current) else {
            self.results.clear();
            return;
        };
        if sem {
            self.results = self.ranked.as_ref().map(|r| r.2.clone()).unwrap_or_default();
            return;
        }
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

    /// Embed the query with the corpus' own model and rank by meaning.
    pub fn search_by_meaning(&mut self) {
        let Some(col) = self.collections.get(self.current) else { return };
        let Some(sem) = &col.semantic else { return };
        if self.query.trim().is_empty() || self.semantic_busy {
            return;
        }
        let Some(key) = api_key(&sem.project_dir) else {
            self.last_error = Some("no GEMINI_API_KEY found — word search still works".into());
            return;
        };
        self.semantic_busy = true;
        let (tx, rx) = mpsc::channel();
        self.sem_rx = Some(rx);
        let (model, dim, q, ci) = (sem.model.clone(), sem.dim, self.query.clone(), self.current);
        std::thread::spawn(move || {
            let res = embed_query(&key, &model, dim, &q);
            let _ = tx.send((ci, q, res));
        });
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

fn scan(tx: Sender<ScanMsg>) {
    let home = home();
    let projects = home.join("Documents/Projects");
    let mut covered: HashSet<PathBuf> = HashSet::new();

    // 1. Semantic corpora (video-grid style).
    if let Ok(rd) = std::fs::read_dir(&projects) {
        for e in rd.flatten() {
            let dir = e.path();
            if let Some(c) = load_corpus(&dir) {
                covered.insert(dir.join("public/clips"));
                let _ = tx.send(ScanMsg::Collection(c));
            }
        }
    }

    // 2. Every other folder that has videos in it.
    let roots = ["Documents", "Movies", "Downloads", "Desktop", "Pictures", "Screencaps"];
    let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for r in roots {
        walk(&home.join(r), 0, &covered, &mut by_dir);
    }
    let mut dirs: Vec<(PathBuf, Vec<PathBuf>)> = by_dir.into_iter().collect();
    dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (dir, mut files) in dirs {
        files.sort();
        let rel = dir.strip_prefix(&projects).or_else(|_| dir.strip_prefix(&home)).unwrap_or(&dir);
        let name = rel.display().to_string();
        let clips = files
            .into_iter()
            .map(|p| {
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let reel = prettify(&reel_of(&stem));
                let title = prettify(&stem);
                let hay = format!("{title} {reel} {name}").to_lowercase();
                BrowseClip { path: p, title, reel, desc: String::new(), dur: 0.0, hay }
            })
            .collect();
        let _ = tx.send(ScanMsg::Collection(Collection { name, clips, semantic: None }));
    }
    let _ = tx.send(ScanMsg::Done);
}

fn walk(dir: &Path, depth: usize, covered: &HashSet<PathBuf>, out: &mut HashMap<PathBuf, Vec<PathBuf>>) {
    if depth > 9 || covered.contains(dir) {
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
            if SKIP_DIRS.iter().any(|s| name == *s) || name.ends_with(".app") || name.ends_with(".photoslibrary") {
                continue;
            }
            walk(&p, depth + 1, covered, out);
        } else if ft.is_file() && is_video(&p) {
            out.entry(dir.to_path_buf()).or_default().push(p);
        }
    }
}

/// A project with `public/corpus.json` + `public/vectors.bin` (+ optional
/// `data/corpus.json` with titles/descriptions).
fn load_corpus(dir: &Path) -> Option<Collection> {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("public/corpus.json")).ok()?).ok()?;
    let dim = meta.get("dim")?.as_u64()? as usize;
    let model = meta.get("model")?.as_str()?.to_string();
    let moments = meta.get("moments")?.as_array()?;
    let bytes = std::fs::read(dir.join("public/vectors.bin")).ok()?;
    if bytes.len() != moments.len() * dim * 4 {
        return None;
    }
    let all: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();

    // Rich text, if present.
    let mut info: HashMap<String, (String, String)> = HashMap::new();
    if let Some(v) = std::fs::read(dir.join("data/corpus.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        if let Some(ms) = v.get("moments").and_then(|m| m.as_array()) {
            for m in ms {
                if let Some(id) = m.get("id").and_then(|x| x.as_str()) {
                    let title = m.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let text = m.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    info.insert(id.to_string(), (title, text));
                }
            }
        }
    }

    let clips_dir = dir.join("public/clips");
    let mut clips = Vec::with_capacity(moments.len());
    let mut vectors = Vec::with_capacity(all.len());
    for (i, m) in moments.iter().enumerate() {
        let Some(id) = m.get("id").and_then(|x| x.as_str()) else { continue };
        let path = clips_dir.join(format!("{id}.mp4"));
        if !path.exists() {
            continue;
        }
        let src = m.get("src").and_then(|x| x.as_str()).unwrap_or("");
        let (title, desc) = info.get(id).cloned().unwrap_or_default();
        let reel = if title.is_empty() { prettify(src) } else { title.clone() };
        let first_line = desc.lines().next().unwrap_or("").to_string();
        let title = if first_line.is_empty() { prettify(id) } else { first_line };
        let hay = format!("{title} {reel} {desc} {id}").to_lowercase();
        clips.push(BrowseClip {
            path,
            title,
            reel,
            desc,
            dur: m.get("d").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
            hay,
        });
        vectors.extend_from_slice(&all[i * dim..(i + 1) * dim]);
    }
    if clips.is_empty() {
        return None;
    }
    let project = dir.file_name()?.to_string_lossy().to_string();
    Some(Collection {
        name: format!("{project} · search by meaning"),
        clips,
        semantic: Some(Semantic { model, dim, vectors, project_dir: dir.to_path_buf() }),
    })
}

// ------------------------------------------------------------ embeddings

fn api_key(project_dir: &Path) -> Option<String> {
    let clean = |s: &str| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    for var in ["GEMINI_API_KEY", "GEMINI_API_KEYS"] {
        if let Ok(v) = std::env::var(var) {
            if let Some(k) = v.split(',').map(clean).find(|k| !k.is_empty()) {
                return Some(k);
            }
        }
    }
    // Same fallbacks the corpus' own tooling uses.
    for f in [project_dir.join(".env"), home().join("Documents/Projects/vibecut/.env")] {
        if let Ok(env) = std::fs::read_to_string(f) {
            for line in env.lines() {
                for var in ["GEMINI_API_KEY=", "GEMINI_API_KEYS="] {
                    if let Some(v) = line.trim().strip_prefix(var) {
                        if let Some(k) = v.split(',').map(clean).find(|k| !k.is_empty()) {
                            return Some(k);
                        }
                    }
                }
            }
        }
    }
    None
}

fn embed_query(key: &str, model: &str, dim: usize, text: &str) -> Result<Vec<f32>, String> {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:embedContent");
    let body = serde_json::json!({
        "model": format!("models/{model}"),
        "content": { "parts": [ { "text": text } ] },
        "taskType": "RETRIEVAL_QUERY",
        "outputDimensionality": dim,
    });
    let resp: serde_json::Value = ureq::post(&url)
        .set("x-goog-api-key", key)
        .timeout(std::time::Duration::from_secs(20))
        .send_json(body)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                format!("embedding failed ({code}): {}", r.into_string().unwrap_or_default().chars().take(160).collect::<String>())
            }
            other => format!("embedding failed: {other}"),
        })?
        .into_json()
        .map_err(|e| e.to_string())?;
    let vals = resp
        .get("embedding")
        .and_then(|e| e.get("values"))
        .and_then(|v| v.as_array())
        .ok_or("embedding response had no values")?;
    let mut v: Vec<f32> = vals.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
    if v.len() != dim {
        return Err(format!("embedding has {} dims, corpus has {dim}", v.len()));
    }
    // Truncated embeddings aren't unit length; normalize so dot = cosine.
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    for x in &mut v {
        *x /= n;
    }
    Ok(v)
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
