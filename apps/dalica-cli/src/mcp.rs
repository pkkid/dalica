//! `dalica-cli mcp [--demo] [--library DIR] [--compact] [FOLDERS…]` (`mcp.md` §1): the MCP server
//! on standard input and output, over a headless session. Standard output carries protocol
//! messages only; logs go to standard error (`DALICA_LOG=error|warn|info|debug`, default `warn`).
//!
//! - `--demo`: the demo library in a temporary folder, deleted on exit (not `run`'s persistent
//!   demo: an agent's changes never outlive its session).
//! - `--library DIR`: a library directory (exit code 3 when another Dalica process has it open).
//! - `FOLDERS…`: added as Scan Once, and read roots; with neither of the above, into an empty
//!   library deleted on exit.
//! - `--compact`: `tools/list` without the one-tool-per-command tools.
//!
//! The options of later tasks (`--connect`, M10.17; `--read-only`, the `--automation-*` roots and
//! `--no-client-roots`, M10.16) are refused by name until they exist.

use std::path::PathBuf;
use std::process::ExitCode;

use dalica_mcp::{Backend, Headless, OpenFailure, PROTOCOL_VERSION, Server, Source};

const USAGE: &str = "usage: dalica-cli mcp [--demo | --library DIR] [--compact] [FOLDERS…]

The MCP server (newline-delimited JSON-RPC 2.0) on standard input and output, over a headless
session: the demo library (--demo), a library directory (--library DIR), or an empty one; FOLDERS
are added as Scan Once. --compact lists only the helper tools. Logs go to standard error
(DALICA_LOG=error|warn|info|debug).";

/// Options of later tasks, refused by name until they are built.
const LATER: &[(&str, &str)] = &[
    ("--read-only", "M10.16"),
    ("--automation-read-root", "M10.16"),
    ("--automation-write-root", "M10.16"),
    ("--automation-allow-external", "M10.16"),
    ("--automation-allow-people", "M10.16"),
    ("--no-client-roots", "M10.16"),
    ("--control-token-file", "M10.17"),
];

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    source: Source,
    folders: Vec<PathBuf>,
    compact: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let (mut demo, mut library, mut compact, mut folders) = (false, None, false, Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => demo = true,
            "--compact" => compact = true,
            "--library" => match it.next() {
                Some(dir) => library = Some(PathBuf::from(dir)),
                None => return Err("--library needs a folder".into()),
            },
            "--connect" => return Err(dalica_mcp::CONNECT_NOT_BUILT.into()),
            other => {
                if let Some((_, task)) = LATER.iter().find(|(flag, _)| *flag == other) {
                    return Err(format!("{other} is not built yet: it arrives with {task}"));
                }
                if other.starts_with('-') {
                    return Err(format!("unknown option {other}"));
                }
                folders.push(PathBuf::from(other));
            }
        }
    }
    let source = match (demo, library) {
        (true, Some(_)) => return Err("--demo cannot be combined with --library".into()),
        (true, None) => Source::Demo,
        (false, Some(dir)) => Source::Library(dir),
        (false, None) => Source::Empty,
    };
    Ok(Options { source, folders, compact })
}

pub fn main(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("dalica-cli mcp: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    Stderr::install();
    let headless = match Headless::open(options.source, &options.folders) {
        Ok(h) => h,
        Err(OpenFailure::Locked(e) | OpenFailure::Failed(e)) => {
            eprintln!("dalica-cli mcp: {e}");
            return ExitCode::from(3);
        }
    };
    eprintln!("dalica-cli mcp: serving {} over standard input and output (MCP {PROTOCOL_VERSION})", headless.describe());
    let mut server = Server::new(Box::new(headless)).with_compact(options.compact);
    let served = server.serve(std::io::stdin(), std::io::stdout());
    // The session writes what is pending and a temporary library is removed.
    drop(server);
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dalica-cli mcp: {e}");
            ExitCode::from(1)
        }
    }
}

/// A logger that writes to standard error only: standard output is the protocol's.
struct Stderr;

static STDERR: Stderr = Stderr;

impl Stderr {
    /// Installs it at the level `DALICA_LOG` names (default `warn`).
    fn install() {
        let level = match std::env::var("DALICA_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
            "off" => log::LevelFilter::Off,
            "error" => log::LevelFilter::Error,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => log::LevelFilter::Warn,
        };
        if log::set_logger(&STDERR).is_ok() {
            log::set_max_level(level);
        }
    }
}

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("dalica-cli mcp: {} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    /// The sources, folders and `--compact`; the options of later tasks are refused by name.
    #[test]
    fn options_parse() {
        let o = parse(&strings(&["--demo", "--compact"])).unwrap();
        assert_eq!((o.source, o.compact), (Source::Demo, true));
        let o = parse(&strings(&["--library", "lib", "a", "b"])).unwrap();
        assert_eq!((o.source, o.folders), (Source::Library("lib".into()), vec![PathBuf::from("a"), PathBuf::from("b")]));
        assert_eq!(parse(&[]).unwrap().source, Source::Empty);
        assert!(parse(&strings(&["--demo", "--library", "x"])).is_err());
        assert!(parse(&strings(&["--library"])).is_err());
        assert!(parse(&strings(&["--connect"])).unwrap_err().contains("M10.17"));
        assert!(parse(&strings(&["--read-only"])).unwrap_err().contains("M10.16"));
        assert!(parse(&strings(&["--colour"])).unwrap_err().contains("unknown option"));
    }
}
