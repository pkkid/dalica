//! View commands that live in the engine because they change what the library shows or how it is
//! decoded (View menu). None yet.
//!
//! Seam: M5.16 adds `view.colorManagement` (View → Use Color Management) here; the module is
//! registered in [`crate::cmd::specs`] already so that work never touches `cmd/mod.rs`.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
