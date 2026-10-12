//! Rotation and flip commands: `photo.rotate.cw` / `photo.rotate.ccw`, `edit.flipH` / `edit.flipV`
//! (`organizing.md` §9, `data-model.md` §3.1).
//!
//! Joined into the registry (`cmd/mod.rs`) by M6.5 and empty until M6.14 adds the commands, so the
//! tasks that fill the M6 areas never share a file.

use crate::registry::CommandSpec;

/// This area's commands (none yet).
pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
