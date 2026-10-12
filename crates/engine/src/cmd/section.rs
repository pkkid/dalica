//! Commands on a light-table section (a folder or an album as a whole: the Folder menu). None yet.
//!
//! Seam: M5.16 adds `section.refreshThumbs` (Folder → Refresh Thumbnails) here; the module is
//! registered in [`crate::cmd::specs`] already so that work never touches `cmd/mod.rs`.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
