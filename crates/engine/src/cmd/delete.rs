//! Deleting to the trash (`organizing.md` §9, `security.md` §5.7): `photo.delete` and
//! `folder.delete`, file operations (`fileops.rs`) whose undo restores from the trash where the
//! platform can. Empty until M9.15.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
