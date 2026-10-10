//! `cargo xtask`: Dalica's gates and tools (`docs/development.md` §3).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod accept;
mod assets;
mod ci;
mod deps;
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
  deps      audit dependencies: licences, banned and *-sys crates, C builds, sources (docs/security.md §8)
  layers    check crate dependencies against the layer table (docs/architecture.md §2)
  names     the reference product's name appears in no file but AGENTS.md
  parity    docs/parity.md is well formed and its evidence exists; --write refreshes the summary
  tasks     the task board (docs/task-board.md); `cargo xtask tasks` for its commands";

/// The workspace root: the parent of this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().map(Path::to_path_buf).unwrap_or_else(|| here.to_path_buf())
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
        Some("names") => names::run(&root),
        Some("layers") => layers::run(&root),
        Some("tasks") => tasks::run(&root, args.get(1..).unwrap_or(&[])),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
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
