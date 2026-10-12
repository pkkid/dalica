//! The photo metadata query: `photo.metadata` (the Properties pane; `ui-layout.md` §7).
//!
//! Joined into the registry (`cmd/mod.rs`) by M6.5 and empty until M6.16 adds the commands, so the
//! tasks that fill the M6 areas never share a file.

use crate::registry::CommandSpec;

/// This area's commands (none yet).
pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
