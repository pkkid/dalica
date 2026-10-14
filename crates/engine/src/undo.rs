//! The library undo stack (`architecture.md` §4): one step per command, in memory, per session,
//! capped at 1 000 steps. A step holds the op that undoes it; undoing applies that op and turns its
//! inverse into the redo step. A step that changed the Folder Manager's rules (`folders.set`,
//! `folder.remove`) also holds the rules to put back ([`crate::folders::Modes`]); putting them back
//! gives the rules the redo step puts back in turn.
//!
//! A file operation (M9: a move, copy, rename or delete of real files) is a step of its own kind,
//! a [`FileStep`] (`fileops.rs`): undoing it runs the reverse operation as a file job, and the step
//! stays on its stack while that job runs. When the job ends, the items it reversed go to the other
//! stack as one step and those it could not stay where the step was
//! ([`UndoStack::file_step_ended`]).

use std::sync::Arc;

use dalica_library::Op;

use crate::fileops::{Batch, Direction};
use crate::jobs::JobId;

/// The most steps kept; the oldest are dropped first.
pub const CAP: usize = 1000;

/// One undoable step: the command's label ("Star / Unstar"), the op that reverses it, and the
/// folder rules to put back, if it changed any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub label: &'static str,
    pub op: Op,
    pub modes: Option<crate::folders::Modes>,
}

/// One file operation on the stack: its items, each with the way back and the library ops that
/// follow it (`fileops.rs`), and the job running its undo or redo, if one is.
#[derive(Clone)]
pub struct FileStep {
    pub(crate) id: u64,
    pub(crate) label: &'static str,
    pub(crate) batch: Arc<dyn Batch>,
    pub(crate) running: Option<JobId>,
}

impl std::fmt::Debug for FileStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileStep")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("items", &self.batch.len())
            .field("running", &self.running)
            .finish()
    }
}

impl FileStep {
    /// Why it cannot run in `dir` now: its undo or redo is running, or (undoing) an item has no way
    /// back ("Restore it from the Trash").
    pub(crate) fn blocked(&self, dir: Direction) -> Option<String> {
        if self.running.is_some() {
            let verb = if dir == Direction::Redo { "Redo" } else { "Undo" };
            return Some(format!("{verb} {} is still running.", self.label));
        }
        self.batch.blocked(dir)
    }
}

/// A step of either kind.
#[derive(Clone, Debug)]
pub(crate) enum Entry {
    Step(Step),
    Files(FileStep),
}

impl Entry {
    fn label(&self) -> &'static str {
        match self {
            Entry::Step(s) => s.label,
            Entry::Files(f) => f.label,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// Rises with every new step (and when the stack is cleared): an undo that ends after a newer
    /// step was recorded leaves no redo step.
    pushes: u64,
}

impl UndoStack {
    /// Records a new step; the redo history is dropped.
    pub fn push(&mut self, step: Step) {
        self.push_new(Entry::Step(step));
    }

    /// Records a new file step; the redo history is dropped.
    pub(crate) fn push_files(&mut self, step: FileStep) {
        self.push_new(Entry::Files(step));
    }

    fn push_new(&mut self, entry: Entry) {
        self.redo.clear();
        self.pushes = self.pushes.wrapping_add(1);
        self.push_entry(entry);
    }

    /// Records a step without touching the redo history (used by redo itself).
    pub(crate) fn push_undo(&mut self, step: Step) {
        self.push_entry(Entry::Step(step));
    }

    fn push_entry(&mut self, entry: Entry) {
        self.undo.push(entry);
        if self.undo.len() > CAP {
            self.undo.remove(0);
        }
    }

    pub(crate) fn push_redo(&mut self, step: Step) {
        self.redo.push(Entry::Step(step));
    }

    /// The step Undo would take (`redo`: the step Redo would take).
    pub(crate) fn last(&self, redo: bool) -> Option<&Entry> {
        if redo { self.redo.last() } else { self.undo.last() }
    }

    pub(crate) fn pop_undo(&mut self) -> Option<Entry> {
        self.undo.pop()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<Entry> {
        self.redo.pop()
    }

    /// How many new steps were recorded so far (see [`UndoStack::file_step_ended`]).
    pub(crate) fn pushes(&self) -> u64 {
        self.pushes
    }

    /// Marks file step `id` (on either stack) as running its undo or redo as `job`.
    pub(crate) fn set_running(&mut self, id: u64, job: Option<JobId>) {
        for e in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            if let Entry::Files(f) = e
                && f.id == id
            {
                f.running = job;
            }
        }
    }

    /// File step `id`'s undo (`undone`) or redo job ended. The items it did not reverse stay where
    /// the step was (`rest`; `None` when it reversed them all: the step leaves its stack), and those
    /// it did go to the other stack as `moved`: an undo's to the redo stack, unless a new step was
    /// recorded since it started (`since`, from [`UndoStack::pushes`]); a redo's to the undo stack.
    pub(crate) fn file_step_ended(&mut self, id: u64, undone: bool, rest: Option<Arc<dyn Batch>>, moved: Option<FileStep>, since: u64) {
        let from = if undone { &mut self.undo } else { &mut self.redo };
        if let Some(i) = from.iter().position(|e| matches!(e, Entry::Files(f) if f.id == id)) {
            match rest {
                Some(batch) => {
                    if let Some(Entry::Files(f)) = from.get_mut(i) {
                        f.batch = batch;
                        f.running = None;
                    }
                }
                None => {
                    from.remove(i);
                }
            }
        }
        let Some(moved) = moved else { return };
        if !undone {
            self.push_entry(Entry::Files(moved));
        } else if self.pushes == since {
            self.redo.push(Entry::Files(moved));
        }
    }

    /// The label of the step Undo would reverse ("Undo Star / Unstar").
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(Entry::label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(Entry::label)
    }

    /// Why Undo cannot run now although there is a step: a file step whose undo is still running,
    /// or one with an item that has no way back (the reason, e.g. "Restore it from the Trash").
    pub fn undo_blocked(&self) -> Option<String> {
        match self.undo.last()? {
            Entry::Files(f) => f.blocked(Direction::Undo),
            Entry::Step(_) => None,
        }
    }

    /// Why Redo cannot run now although there is a step (its redo is still running).
    pub fn redo_blocked(&self) -> Option<String> {
        match self.redo.last()? {
            Entry::Files(f) => f.blocked(Direction::Redo),
            Entry::Step(_) => None,
        }
    }

    /// The job running the top file step's undo, if one is.
    pub fn undo_running(&self) -> Option<JobId> {
        match self.undo.last()? {
            Entry::Files(f) => f.running,
            Entry::Step(_) => None,
        }
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// The steps Redo can take.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Forgets everything (after a rebuild, when ids change). A file step's undo still running
    /// then leaves no redo step.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pushes = self.pushes.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(n: u32) -> Step {
        Step { label: "Star / Unstar", op: Op::SetStars { stars: vec![(dalica_core::PhotoId::new(u64::from(n)), true)] }, modes: None }
    }

    #[test]
    fn the_stack_is_capped_and_a_new_step_drops_redo() {
        let mut s = UndoStack::default();
        for n in 0..(CAP as u32 + 5) {
            s.push(step(n));
        }
        assert_eq!(s.len(), CAP);
        let Some(Entry::Step(top)) = s.pop_undo() else { panic!("a step") };
        assert_eq!(top, step(CAP as u32 + 4));
        s.push_redo(top);
        assert_eq!(s.redo_label(), Some("Star / Unstar"));
        s.push(step(9999));
        assert_eq!(s.redo_label(), None);
        s.clear();
        assert!(s.is_empty() && s.undo_label().is_none());
        assert_eq!((s.undo_blocked(), s.redo_blocked(), s.undo_running()), (None, None, None));
    }
}
