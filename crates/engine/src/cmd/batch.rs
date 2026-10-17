//! `edit.apply {ids, op, params?}`, `edit.copyRecipe {id}`, `edit.pasteRecipe {ids}` and
//! `edit.undoAll {ids, confirm}` (`editing.md` §5.1 and §9): Batch Edit, Copy and Paste All Edits,
//! and Undo All Edits. Empty until M11.9.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
