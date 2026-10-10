//! The library undo stack (`architecture.md` §4): one step per command, in memory, per session,
//! capped at 1 000 steps. A step holds the op that undoes it; undoing applies that op and turns its
//! inverse into the redo step.

use dalica_library::Op;

/// The most steps kept; the oldest are dropped first.
pub const CAP: usize = 1000;

/// One undoable step: the command's label ("Star / Unstar") and the op that reverses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub label: &'static str,
    pub op: Op,
}

#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl UndoStack {
    /// Records a new step; the redo history is dropped.
    pub fn push(&mut self, step: Step) {
        self.redo.clear();
        self.push_undo(step);
    }

    /// Records a step without touching the redo history (used by redo itself).
    pub(crate) fn push_undo(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > CAP {
            self.undo.remove(0);
        }
    }

    pub(crate) fn push_redo(&mut self, step: Step) {
        self.redo.push(step);
    }

    pub(crate) fn pop_undo(&mut self) -> Option<Step> {
        self.undo.pop()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<Step> {
        self.redo.pop()
    }

    /// The label of the step Undo would reverse ("Undo Star / Unstar").
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|s| s.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|s| s.label)
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// Forgets everything (after a rebuild, when ids change).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(n: u32) -> Step {
        Step { label: "Star / Unstar", op: Op::SetStars { stars: vec![(dalica_core::PhotoId::new(u64::from(n)), true)] } }
    }

    #[test]
    fn the_stack_is_capped_and_a_new_step_drops_redo() {
        let mut s = UndoStack::default();
        for n in 0..(CAP as u32 + 5) {
            s.push(step(n));
        }
        assert_eq!(s.len(), CAP);
        let top = s.pop_undo().unwrap();
        assert_eq!(top, step(CAP as u32 + 4));
        s.push_redo(top);
        assert_eq!(s.redo_label(), Some("Star / Unstar"));
        s.push(step(9999));
        assert_eq!(s.redo_label(), None);
        s.clear();
        assert!(s.is_empty() && s.undo_label().is_none());
    }
}
