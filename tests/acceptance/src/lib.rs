//! Helpers shared by the milestone acceptance tests (`tests/acceptance/m<n>.rs`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root (two levels above this crate's manifest).
pub fn workspace_root() -> PathBuf {
    // Cargo sets `CARGO_MANIFEST_DIR` when it runs a test; prefer that to the compile-time value, so a
    // test binary reused from another checkout (shared target dir) still checks this one.
    let here =
        std::env::var_os("CARGO_MANIFEST_DIR").filter(|v| !v.is_empty()).map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    here.ancestors().nth(2).map(Path::to_path_buf).unwrap_or(here)
}

/// A `cargo` command for the toolchain running the tests, started in the workspace root.
pub fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(workspace_root());
    cmd
}

/// The production-code attributes every crate root must carry (`docs/standards/never-crash.md`).
pub const CRATE_ROOT_ATTRIBUTES: [&str; 2] = [
    "#![forbid(unsafe_code)]",
    "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]",
];
