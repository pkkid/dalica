//! Save, Undo Save and Revert (`data-model.md` §6, `editing.md` §5.1): `photo.save`,
//! `photo.undoSave` and `photo.revert`, run as file jobs ([`crate::filejobs`]). Empty until M8.15.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
