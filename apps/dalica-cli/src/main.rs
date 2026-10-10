//! Dalica's headless command line (`docs/architecture.md` §3, §7): `commands` lists the command
//! registry as JSON, `run` executes a JSON-lines script on a library, `inspect` summarises one.
//! `snapshot` arrives with the headless UI (M1.17).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod inspect;
mod run;

use std::process::ExitCode;

const USAGE: &str = "usage: dalica-cli <command>

commands:
  commands                                   the command registry as JSON
  run --library DIR [SCRIPT | -]             run a JSON-lines script ({\"command\": id, \"params\": {...}} per line;
                                             stdin when no SCRIPT or `-`); one JSON result per line
  run --library DIR --cmd ID [--params JSON] run one command
  inspect --library DIR                      the library as JSON: folders, photos, stars, repairs
  --version                                  print the version

exit codes: 0 success, 1 a command failed, 2 a bad argument or script line, 3 the library could not be
opened (for example, another Dalica process has it open)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("dalica-cli {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h" | "help") => println!("{USAGE}"),
        Some("commands") => println!("{}", commands()),
        Some("run") => return run::main(args.get(1..).unwrap_or_default()),
        Some("inspect") => return inspect::main(args.get(1..).unwrap_or_default()),
        Some(other) => {
            eprintln!("dalica-cli: unknown command `{other}`\n\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// The registry as `engine.commands` lists it, without a library (so `enabled` is not evaluated).
fn commands() -> String {
    let list: Vec<serde_json::Value> = dalica_engine::cmd::specs()
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "label": s.label, "menu": s.menu, "shortcut": s.keys.first().map(|(_, k)| *k),
                "keys": s.keys.iter().map(|(c, k)| serde_json::json!({ "context": c, "key": k })).collect::<Vec<_>>(),
                "params": s.params, "effect": s.effect, "history": s.history, "ui": false, "live": true,
            })
        })
        .collect();
    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into())
}

/// The value after `--library`.
pub(crate) fn library_arg(args: &[String]) -> Option<std::path::PathBuf> {
    args.iter().position(|a| a == "--library").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from)
}

/// Opens the library, or prints why not and gives exit code 3.
pub(crate) fn open(dir: &std::path::Path) -> Result<dalica_engine::Session, ExitCode> {
    match dalica_engine::Session::open(dir, std::sync::Arc::new(dalica_core::SystemClock)) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("dalica-cli: {e}");
            Err(ExitCode::from(3))
        }
    }
}
