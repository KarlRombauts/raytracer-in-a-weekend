//! Undo/redo for the scene editor.
//!
//! The whole editable `Scene` is cheap to clone (mesh/BVH/texture data is
//! `Arc`-shared, so a clone copies only the small plain-data fields), so the
//! history is a plain stack of scene snapshots rather than a command log.
//!
//! A "transaction" coalesces a continuous interaction — dragging a slider, the
//! transform gizmo, or orbiting the camera produces a change every frame, but
//! should be a *single* undo step. The driver calls [`History::begin`] on every
//! frame the scene changed (recording the pre-edit baseline only on the first
//! such frame) and [`History::commit`] once the interaction settles (the
//! pointer is released), so the whole drag collapses to one entry.

/// Generic over the snapshot type so the transaction logic can be unit-tested
/// without constructing a full `Scene`. The editor uses `History<Scene>`.
pub struct History<T> {
    /// Pre-edit snapshots, oldest first; the last is the most recent baseline.
    undo: Vec<T>,
    /// Snapshots undone away, available to redo.
    redo: Vec<T>,
    /// Baseline of the in-progress transaction (the state to restore to if it
    /// is undone). `None` when no edit is in flight.
    pending: Option<T>,
    /// Maximum depth of the undo stack; the oldest entry is dropped past this.
    cap: usize,
}

impl<T: Clone> History<T> {
    pub fn new(cap: usize) -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            cap,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True while a transaction is open (an edit is in flight).
    pub fn in_transaction(&self) -> bool {
        self.pending.is_some()
    }

    /// Open a transaction, recording `before` as the state to restore to. Only
    /// the first call while a transaction is open takes effect, so repeated
    /// per-frame calls during a drag coalesce to one entry. Starting a new edit
    /// invalidates the redo stack.
    pub fn begin(&mut self, before: T) {
        if self.pending.is_none() {
            self.pending = Some(before);
            self.redo.clear();
        }
    }

    /// Close the open transaction, pushing its baseline onto the undo stack.
    /// No-op when no transaction is open. Drops the oldest entry past `cap`.
    pub fn commit(&mut self) {
        if let Some(baseline) = self.pending.take() {
            self.undo.push(baseline);
            if self.undo.len() > self.cap {
                self.undo.remove(0);
            }
        }
    }

    /// Undo: store `current` on the redo stack and return the state to restore
    /// to, or `None` when there is nothing to undo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let baseline = self.undo.pop()?;
        self.redo.push(current);
        Some(baseline)
    }

    /// Redo: store `current` on the undo stack and return the state to restore
    /// to, or `None` when there is nothing to redo.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    /// Drop all history (used when the whole scene is replaced).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_history_has_nothing_to_undo_or_redo() {
        let h: History<i32> = History::new(100);
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn a_committed_transaction_can_be_undone() {
        let mut h: History<i32> = History::new(100);
        h.begin(1); // scene was 1, now edited to 2
        h.commit();
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_returns_the_pre_edit_baseline_and_enables_redo() {
        let mut h: History<i32> = History::new(100);
        h.begin(1);
        h.commit(); // current is now 2
        assert_eq!(h.undo(2), Some(1));
        assert!(h.can_redo());
        assert!(!h.can_undo());
    }

    #[test]
    fn redo_returns_the_state_that_was_undone_away() {
        let mut h: History<i32> = History::new(100);
        h.begin(1);
        h.commit();
        let restored = h.undo(2).unwrap(); // back to 1, redo holds 2
        assert_eq!(restored, 1);
        assert_eq!(h.redo(restored), Some(2));
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_then_redo_walks_a_multi_step_history() {
        let mut h: History<i32> = History::new(100);
        h.begin(1);
        h.commit(); // 1 -> 2
        h.begin(2);
        h.commit(); // 2 -> 3, current is 3
        assert_eq!(h.undo(3), Some(2));
        assert_eq!(h.undo(2), Some(1));
        assert!(!h.can_undo());
        assert_eq!(h.redo(1), Some(2));
        assert_eq!(h.redo(2), Some(3));
        assert!(!h.can_redo());
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut h: History<i32> = History::new(100);
        h.begin(1);
        h.commit(); // 1 -> 2
        h.undo(2); // back to 1, redo holds 2
        assert!(h.can_redo());
        h.begin(1); // a fresh edit from 1
        h.commit();
        assert!(!h.can_redo());
    }

    #[test]
    fn repeated_begins_in_one_transaction_coalesce_to_one_entry() {
        let mut h: History<i32> = History::new(100);
        // Simulate a drag: the scene changes every frame, so begin() fires
        // repeatedly, but only the first baseline (10) should be recorded.
        h.begin(10);
        h.begin(11);
        h.begin(12);
        h.commit();
        assert_eq!(h.undo(13), Some(10));
        assert!(!h.can_undo());
    }

    #[test]
    fn commit_without_an_open_transaction_is_a_noop() {
        let mut h: History<i32> = History::new(100);
        h.commit();
        assert!(!h.can_undo());
    }

    #[test]
    fn the_undo_stack_is_capped_dropping_the_oldest() {
        let mut h: History<i32> = History::new(2);
        for v in [1, 2, 3] {
            h.begin(v);
            h.commit();
        }
        // cap=2 keeps the two most recent baselines: 2 then 3.
        assert_eq!(h.undo(4), Some(3));
        assert_eq!(h.undo(3), Some(2));
        assert!(!h.can_undo()); // 1 was dropped
    }

    #[test]
    fn clear_empties_both_stacks() {
        let mut h: History<i32> = History::new(100);
        h.begin(1);
        h.commit();
        h.undo(2);
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}
