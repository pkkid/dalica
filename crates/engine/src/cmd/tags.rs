//! Tags and quick tags: `photo.tags.add` / `.remove` / `.toggle`,
//! `tags.list`, `tags.rename`, `tags.quick.set` / `.apply` (`organizing.md` §5.2).
//!
//! Joined into the registry (`cmd/mod.rs`) by M6.5 and empty until M6.15 adds the commands, so the
//! tasks that fill the M6 areas never share a file.

use crate::registry::CommandSpec;

/// This area's commands (none yet).
pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
