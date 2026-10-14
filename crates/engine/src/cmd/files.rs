//! Moving and copying photos (`organizing.md` §9, `data-model.md` §8): `photo.moveTo`,
//! `photo.copyTo` and `photo.moveToNewFolder`, file operations (`fileops.rs`) that carry each
//! photo's sidecar entry, backup and record with it. Empty until M9.13.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
