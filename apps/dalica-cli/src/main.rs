//! Dalica's headless command line: `commands` lists the command registry as JSON; `run`, `inspect`
//! and `snapshot` arrive in M1 (`docs/architecture.md` §7).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

const USAGE: &str = "usage: dalica-cli <command>

commands:
  commands     the command registry as JSON
  --version    print the version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("dalica-cli {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h" | "help") => println!("{USAGE}"),
        // The engine registers no commands yet (M1.13 fills this from `Session`'s registry).
        Some("commands") => println!("[]"),
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
