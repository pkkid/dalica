//! The previous organizer's data (`data-model.md` §9; the state is `crate::legacy`):
//! `legacy.inspect {folders?}` (each folder's source and what it held) and `legacy.noticeShown`
//! (the first-run notice answered). Empty until M14.13.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
