//! `cargo xtask`: Dalica's gates and tools (`docs/development.md` §3).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod accept;
mod assets;
mod ci;
mod deps;
mod docs;
mod layers;
mod metadata;
mod names;
mod parity;
mod tasks;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  ci        THE gate: fmt, clippy, tests, layers, deps, assets, names, parity, tasks sync --check, accept (finished milestones)
  accept    cargo xtask accept M<n> [--headless]: a milestone's acceptance test and its manual checks
  assets    every asset has an ATTRIBUTION.md row; licence files exist; no forbidden names or formats
  docs      every `cargo xtask` command the docs name exists (AGENTS.md, development.md, task-board.md)
  deps      audit dependencies: licences, banned and *-sys crates, C builds, sources (docs/security.md §8)
  layers    check crate dependencies against the layer table (docs/architecture.md §2)
  names     the reference product's name appears in no file but AGENTS.md
  parity    docs/parity.md is well formed and its evidence exists; --write refreshes the summary
  tasks     the task board (docs/task-board.md); `cargo xtask tasks` for its commands";

/// Implemented commands (`cargo xtask docs` checks the docs against these and [`PLANNED`]).
pub const COMMANDS: &[&str] = &["ci", "accept", "assets", "docs", "deps", "layers", "names", "parity", "tasks"];

/// Commands the docs already describe, with where they are planned; running one says so.
pub const PLANNED: &[(&str, &str)] = &[
    ("bench", "with the first hot paths (M4's headless scrolling benchmark)"),
    ("perf", "with the performance budgets (docs/development.md §13), from M4"),
    ("corpus", "with the test corpora (M5, docs/development.md §11)"),
    ("test-corpus", "with the test corpora (M5, docs/development.md §11)"),
    ("scorecard", "after M1 (docs/development.md §13)"),
    ("snapshot", "with the headless snapshot scripts (M2)"),
    ("stats", "not scheduled yet"),
];

/// The workspace root: the parent of this crate's manifest directory, as cargo reports it when it
/// runs `xtask` (`cargo xtask …` sets `CARGO_MANIFEST_DIR` at run time). The compile-time value is
/// only a fallback: with a target dir shared by two checkouts, cargo may reuse a binary built in the
/// other checkout, and a baked-in root would make `ci` check the wrong tree.
pub fn workspace_root() -> PathBuf {
    root_from(std::env::var_os("CARGO_MANIFEST_DIR"), env!("CARGO_MANIFEST_DIR"))
}

fn root_from(at_run_time: Option<std::ffi::OsString>, compiled: &str) -> PathBuf {
    let here = at_run_time.filter(|v| !v.is_empty()).map_or_else(|| PathBuf::from(compiled), PathBuf::from);
    here.parent().map(Path::to_path_buf).unwrap_or(here)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.first().map(String::as_str) {
        Some("accept") => accept::run(&root, args.get(1..).unwrap_or(&[])),
        Some("assets") => assets::run(&root),
        Some("ci") => ci::run(&root),
        Some("parity") => parity::run(&root, args.iter().any(|a| a == "--write")),
        Some("deps") => deps::run(&root),
        Some("docs") => docs::run(&root),
        Some("names") => names::run(&root),
        Some("layers") => layers::run(&root),
        Some("tasks") => tasks::run(&root, args.get(1..).unwrap_or(&[])),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) if PLANNED.iter().any(|(c, _)| *c == other) => {
            let when = PLANNED.iter().find(|(c, _)| *c == other).map_or("", |(_, w)| *w);
            eprintln!("xtask: `cargo xtask {other}` does not exist yet; it is planned {when}");
            return ExitCode::from(2);
        }
        Some(other) => {
            eprintln!("xtask: unknown command `{other}`\n\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod root_tests {
    use super::*;

    #[test]
    fn the_run_time_manifest_dir_wins_over_the_compiled_one() {
        let compiled = "/checkout-a/xtask";
        assert_eq!(root_from(Some("/checkout-b/xtask".into()), compiled), PathBuf::from("/checkout-b"));
        assert_eq!(root_from(None, compiled), PathBuf::from("/checkout-a"));
        assert_eq!(root_from(Some("".into()), compiled), PathBuf::from("/checkout-a"));
    }
}
