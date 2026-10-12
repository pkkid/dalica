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

/// A `cargo` command for the toolchain running the tests, started in the workspace root. It never
/// passes `DALICA_BLESS` down: blessing a milestone's own baselines must not re-bless the crate
/// suites and builds it runs.
pub fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(workspace_root()).env_remove("DALICA_BLESS");
    cmd
}

/// Set to `1` by `cargo xtask ci` on its `accept` steps, which run after its workspace test step
/// (`docs/development.md` §3): the crate suites an acceptance test would re-run (`cargo test -p …`)
/// already ran in that step, so the test skips them. A standalone `cargo xtask accept M<n>` clears
/// it, so one command still checks the whole *Done when*.
pub const SUITES_RAN: &str = "DALICA_SUITES_RAN";

/// Whether the crate suites already ran in this `cargo xtask ci` ([`SUITES_RAN`]); prints so when
/// they did, so the log says why they are missing.
pub fn suites_ran() -> bool {
    let ran = std::env::var_os(SUITES_RAN).is_some_and(|v| v == "1");
    if ran {
        println!("suites: run by the ci test step ({SUITES_RAN}=1)");
    }
    ran
}

/// The production-code attributes every crate root must carry (`docs/standards/never-crash.md`).
pub const CRATE_ROOT_ATTRIBUTES: [&str; 2] = [
    "#![forbid(unsafe_code)]",
    "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]",
];
