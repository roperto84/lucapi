//! Snapshot-based undo/redo.
//!
//! Edits are coalesced into bursts: `mark_dirty` is called on every mutation,
//! and a snapshot is recorded once the burst is committed (on idle autosave or
//! right before an undo). Snapshots are whole documents, which keeps undo
//! correct across renames, deletions and imports without per-field bookkeeping.

pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    committed: T,
    dirty: bool,
    limit: usize,
}

impl<T: Clone> History<T> {
    pub fn new(initial: T) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            committed: initial,
            dirty: false,
            limit: 64,
        }
    }

    /// Drops all history and takes `current` as the new baseline (startup).
    pub fn reset(&mut self, current: T) {
        self.undo.clear();
        self.redo.clear();
        self.committed = current;
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Records the current document if it has uncommitted changes.
    pub fn commit(&mut self, current: &T) {
        if !self.dirty {
            return;
        }
        self.push_undo(self.committed.clone());
        self.committed = current.clone();
        self.dirty = false;
        self.redo.clear();
    }

    /// Returns the document to restore, if there is something to undo.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        self.commit(current);
        let previous = self.undo.pop()?;
        self.push_redo(self.committed.clone());
        self.committed = previous.clone();
        self.dirty = false;
        Some(previous)
    }

    /// Returns the document to restore, if there is something to redo.
    pub fn redo(&mut self) -> Option<T> {
        let next = self.redo.pop()?;
        self.push_undo(self.committed.clone());
        self.committed = next.clone();
        self.dirty = false;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        self.dirty || !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn push_undo(&mut self, snapshot: T) {
        self.undo.push(snapshot);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    fn push_redo(&mut self, snapshot: T) {
        self.redo.push(snapshot);
        if self.redo.len() > self.limit {
            self.redo.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_undo_without_changes() {
        let mut history = History::new("a");
        assert!(!history.can_undo());
        assert!(history.undo(&"a").is_none());
    }

    #[test]
    fn undo_restores_the_previous_burst() {
        let mut history = History::new("a");
        history.mark_dirty();
        history.commit(&"b");
        assert_eq!(history.undo(&"b"), Some("a"));
        assert!(!history.can_undo());
        assert_eq!(history.redo(), Some("b"));
    }

    #[test]
    fn undo_before_commit_commits_the_pending_burst_first() {
        let mut history = History::new("a");
        history.mark_dirty();
        assert_eq!(history.undo(&"b"), Some("a"));
    }

    #[test]
    fn new_edits_clear_the_redo_stack() {
        let mut history = History::new("a");
        history.mark_dirty();
        history.commit(&"b");
        assert_eq!(history.undo(&"b"), Some("a"));
        history.mark_dirty();
        history.commit(&"c");
        assert!(!history.can_redo());
    }

    #[test]
    fn several_bursts_are_undone_one_by_one() {
        let mut history = History::new(0);
        for value in 1..=3 {
            history.mark_dirty();
            history.commit(&value);
        }
        assert_eq!(history.undo(&3), Some(2));
        assert_eq!(history.undo(&2), Some(1));
        assert_eq!(history.undo(&1), Some(0));
        assert_eq!(history.redo(), Some(1));
        assert_eq!(history.redo(), Some(2));
        assert_eq!(history.redo(), Some(3));
        assert_eq!(history.redo(), None);
    }

    #[test]
    fn history_is_capped() {
        let mut history = History::new(0);
        for value in 1..=100 {
            history.mark_dirty();
            history.commit(&value);
        }
        let mut undone = 0;
        while history.undo(&100).is_some() {
            undone += 1;
        }
        assert_eq!(undone, 64);
    }
}
