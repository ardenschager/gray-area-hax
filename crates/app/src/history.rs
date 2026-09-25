//! Undo / redo by whole-project snapshots.
//!
//! A project clone is cheap (media lives behind `Arc`s), so every committed
//! edit keeps a full copy. Edits are grouped by gesture: nothing is
//! committed while the mouse is held or a text field is being typed in, so
//! a slider drag or a clip drag is one undo step. Background changes (synth
//! renders swapping in, pads refilling) move the baseline without creating
//! steps of their own.

use chromagrain_core::timeline::Project;
use std::sync::Mutex;

const MAX_STEPS: usize = 200;

#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// The last committed state (what "undo" goes back from).
    baseline: Option<Project>,
    base_rev: u64,
    /// A user edit happened since the baseline.
    pending_user: bool,
    /// Drop all history at the next observe.
    clear_requested: bool,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending_user
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Call once per frame. `user_changed`: the project changed this frame
    /// because of the user (not a background job). `busy`: a gesture is in
    /// progress (pointer held / typing) — wait before committing.
    pub fn observe(&mut self, rev: u64, user_changed: bool, busy: bool, project: &Mutex<Project>) {
        if std::mem::take(&mut self.clear_requested) {
            self.clear(rev, project);
            return;
        }
        if self.baseline.is_none() {
            self.baseline = Some(project.lock().unwrap().clone());
            self.base_rev = rev;
            return;
        }
        if rev == self.base_rev {
            return;
        }
        if user_changed {
            self.pending_user = true;
        }
        if !busy {
            self.commit(rev, project);
        }
    }

    /// Commit whatever changed since the baseline (before undo/redo).
    pub fn flush(&mut self, rev: u64, project: &Mutex<Project>) {
        if rev != self.base_rev {
            self.commit(rev, project);
        }
    }

    fn commit(&mut self, rev: u64, project: &Mutex<Project>) {
        let cur = project.lock().unwrap().clone();
        if self.pending_user {
            if let Some(b) = self.baseline.take() {
                self.undo.push(b);
                if self.undo.len() > MAX_STEPS {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
        }
        self.baseline = Some(cur);
        self.base_rev = rev;
        self.pending_user = false;
    }

    /// Step back: returns the project to restore. Call [`Self::rebase`]
    /// with the new revision after installing it.
    pub fn undo(&mut self) -> Option<Project> {
        let prev = self.undo.pop()?;
        if let Some(cur) = self.baseline.replace(prev.clone()) {
            self.redo.push(cur);
        }
        Some(prev)
    }

    pub fn redo(&mut self) -> Option<Project> {
        let next = self.redo.pop()?;
        if let Some(cur) = self.baseline.replace(next.clone()) {
            self.undo.push(cur);
        }
        Some(next)
    }

    /// The restored project is now live at `rev`.
    pub fn rebase(&mut self, rev: u64) {
        self.base_rev = rev;
        self.pending_user = false;
    }

    /// Forget everything at the end of this frame.
    pub fn request_clear(&mut self) {
        self.clear_requested = true;
    }

    /// Forget everything (e.g. when switching between arrange and perform,
    /// which swap the whole track list).
    pub fn clear(&mut self, rev: u64, project: &Mutex<Project>) {
        self.undo.clear();
        self.redo.clear();
        self.baseline = Some(project.lock().unwrap().clone());
        self.base_rev = rev;
        self.pending_user = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n_tracks(m: &Mutex<Project>) -> usize {
        m.lock().unwrap().tracks.len()
    }

    #[test]
    fn gesture_is_one_step_and_background_is_none() {
        let m = Mutex::new(Project::default());
        let mut h = History::default();
        let mut rev = 1;
        h.observe(rev, false, false, &m);
        // A drag: several user changes while busy, then release.
        for _ in 0..3 {
            m.lock().unwrap().add_track("t");
            rev += 1;
            h.observe(rev, true, true, &m);
        }
        h.observe(rev, false, false, &m);
        // A background change (not undoable on its own).
        m.lock().unwrap().bpm = 90.0;
        rev += 1;
        h.observe(rev, false, false, &m);
        assert!(h.can_undo());
        let p = h.undo().unwrap();
        assert_eq!(p.tracks.len(), 0, "the whole drag undoes at once");
        *m.lock().unwrap() = p;
        rev += 1;
        h.rebase(rev);
        assert!(!h.can_undo());
        let p = h.redo().unwrap();
        assert_eq!(p.tracks.len(), 3);
        assert_eq!(p.bpm, 90.0, "redo returns to the latest state");
        *m.lock().unwrap() = p;
        rev += 1;
        h.rebase(rev);
        assert_eq!(n_tracks(&m), 3);
        // A new edit clears redo.
        h.undo().map(|p| *m.lock().unwrap() = p);
        rev += 1;
        h.rebase(rev);
        m.lock().unwrap().add_track("x");
        rev += 1;
        h.observe(rev, true, false, &m);
        assert!(!h.can_redo());
    }
}
