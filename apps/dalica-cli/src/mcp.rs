//! `dalica-cli mcp [--demo] [--library DIR] [--compact] [--read-only] [--automation-…] [FOLDERS…]`
//! or `dalica-cli mcp --connect [HOST:PORT] [--control-token-file PATH] [--compact]` (`mcp.md`
//! §1): the MCP server on standard input and output, over a headless session or a running app.
//! Standard output carries protocol messages only; logs go to standard error
//! (`DALICA_LOG=error|warn|info|debug`, default `warn`).
//!
//! - `--demo`: the demo library in a temporary folder, deleted on exit (not `run`'s persistent
//!   demo: an agent's changes never outlive its session).
//! - `--library DIR`: a library directory (exit code 3 when another Dalica process has it open).
//! - `FOLDERS…`: added as Scan Once, and read roots; with neither of the above, into an empty
//!   library deleted on exit.
//! - `--compact`: `tools/list` without the one-tool-per-command tools.
//! - `--read-only`: the session writes nothing to disk; file commands and exports answer
//!   `read_only`.
//! - `--automation-read-root DIR`, `--automation-write-root DIR` (each repeatable, made absolute
//!   against the working folder), `--automation-allow-external`, `--automation-allow-people`: what
//!   automation may touch (`mcp.md` §5). Flags only: no environment variable widens the roots of an
//!   MCP server.
//! - `--no-client-roots`: never ask the client for its roots.
//! - `--connect [HOST:PORT]` (default `127.0.0.1:7990`, loopback only) and `--control-token-file
//!   PATH`: connect mode, the [`dalica_mcp::Connect`] backend over the app's control channel. It
//!   starts even when the app is not running (the first call connects). The app's own policy
//!   applies: the roots flags, `--automation-allow-external` and the client's roots are ignored
//!   (`mcp.md` §5), and the headless session's options (`--demo`, `--library`, `FOLDERS`,
//!   `--read-only`) are refused with it. `--automation-allow-people` is this process's own say
//!   ([`Connect::allow_people`]): people's names reach the client only when it and the app's own
//!   flag both allow them. (Wired here by M10.16: M10.17 built connect mode while this file was
//!   M10.16's.)

use std::path::PathBuf;
use std::process::ExitCode;

use dalica_engine::automation::Policy;
use dalica_mcp::{Backend, Client, Connect, Headless, OpenFailure, PROTOCOL_VERSION, Server, Source, connect};

const USAGE: &str = "usage: dalica-cli mcp [--demo | --library DIR] [--compact] [--read-only]
       [--automation-read-root DIR]… [--automation-write-root DIR]… [--automation-allow-external]
       [--automation-allow-people] [--no-client-roots] [FOLDERS…]
   or: dalica-cli mcp --connect [HOST:PORT] [--control-token-file PATH] [--compact]

The MCP server (newline-delimited JSON-RPC 2.0) on standard input and output, over a headless
session: the demo library (--demo), a library directory (--library DIR), or an empty one; FOLDERS
are added as Scan Once. --compact lists only the helper tools. --read-only writes nothing to disk.
The --automation-* options say what automation may read, write and reach; the client's MCP roots
are read roots unless --no-client-roots. --connect drives a running `dalica --control PORT`
instead (default 127.0.0.1:7990), under the app's own policy (people's names reach the client
only when both the app and this command have --automation-allow-people). Logs go to standard error
(DALICA_LOG=error|warn|info|debug).";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    source: Source,
    folders: Vec<PathBuf>,
    compact: bool,
    read_only: bool,
    policy: Policy,
    client_roots: bool,
    /// `--connect`: the running app instead of a headless session.
    connect: Option<connect::Options>,
}

/// `dir` made absolute against the working folder (a root is compared as an absolute path).
fn absolute(flag: &str, dir: Option<&String>) -> Result<PathBuf, String> {
    let dir = dir.filter(|d| !d.is_empty()).ok_or_else(|| format!("{flag} needs a folder"))?;
    std::path::absolute(dir).map_err(|e| format!("{flag} {dir}: {e}"))
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut args = args.to_vec();
    let connect = connect::Options::take(&mut args)?;
    let (mut demo, mut library, mut compact, mut folders) = (false, None, false, Vec::new());
    let (mut read_only, mut policy, mut client_roots) = (false, Policy::default(), true);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => demo = true,
            "--compact" => compact = true,
            "--read-only" => read_only = true,
            "--no-client-roots" => client_roots = false,
            "--automation-allow-external" => policy.set_allow_external(true),
            "--automation-allow-people" => policy.set_allow_people(true),
            "--automation-read-root" => {
                policy.add_read(absolute(a, it.next())?);
            }
            "--automation-write-root" => {
                policy.add_write(absolute(a, it.next())?);
            }
            "--library" => match it.next() {
                Some(dir) => library = Some(PathBuf::from(dir)),
                None => return Err("--library needs a folder".into()),
            },
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown option {other}"));
                }
                folders.push(PathBuf::from(other));
            }
        }
    }
    if connect.is_some() && (demo || library.is_some() || !folders.is_empty() || read_only) {
        return Err("--connect drives the running app; --demo, --library, FOLDERS and --read-only are for a headless session".into());
    }
    let source = match (demo, library) {
        (true, Some(_)) => return Err("--demo cannot be combined with --library".into()),
        (true, None) => Source::Demo,
        (false, Some(dir)) => Source::Library(dir),
        (false, None) => Source::Empty,
    };
    Ok(Options { source, folders, compact, read_only, policy, client_roots, connect })
}

pub fn main(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return crate::print(USAGE);
    }
    let options = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("dalica-cli mcp: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    Stderr::install();
    let backend: Box<dyn Backend> = match &options.connect {
        // Connect mode: the app's policy applies; the client's roots are not asked for.
        Some(c) => match Client::from_options(c) {
            Ok(client) => Box::new(connect_backend(client, &options.policy)),
            Err(e) => {
                eprintln!("dalica-cli mcp: {e}");
                return ExitCode::from(2);
            }
        },
        None => match Headless::open_with(options.source, &options.folders, options.policy, options.read_only) {
            Ok(h) => Box::new(h),
            Err(OpenFailure::Locked(e) | OpenFailure::Failed(e)) => {
                eprintln!("dalica-cli mcp: {e}");
                return ExitCode::from(3);
            }
        },
    };
    eprintln!("dalica-cli mcp: serving {} over standard input and output (MCP {PROTOCOL_VERSION})", backend.describe());
    let client_roots = options.client_roots && options.connect.is_none();
    let mut server = Server::new(backend).with_compact(options.compact).with_client_roots(client_roots);
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

/// Connect mode over `client`: only `policy`'s people permission is its own (the app's policy
/// decides the rest).
fn connect_backend(client: Client, policy: &Policy) -> Connect {
    Connect::new(client).allow_people(policy.allow_people())
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

    /// The sources, folders, `--compact`, `--read-only`, the roots and permissions; the options of
    /// later tasks are refused by name.
    #[test]
    fn options_parse() {
        let o = parse(&strings(&["--demo", "--compact"])).unwrap();
        assert_eq!((o.source, o.compact), (Source::Demo, true));
        let o = parse(&strings(&["--library", "lib", "a", "b"])).unwrap();
        assert_eq!((o.source, o.folders), (Source::Library("lib".into()), vec![PathBuf::from("a"), PathBuf::from("b")]));
        assert_eq!(parse(&[]).unwrap().source, Source::Empty);
        assert!(parse(&strings(&["--demo", "--library", "x"])).is_err());
        assert!(parse(&strings(&["--library"])).is_err());
        let o = parse(&strings(&["--connect", "--compact", "--automation-read-root", "x"])).unwrap();
        assert_eq!(o.connect.map(|c| c.addr.to_string()), Some(dalica_mcp::DEFAULT_ADDR.to_string()), "the root flag is ignored, not refused");
        assert_eq!(parse(&strings(&["--connect", "127.0.0.1:18040"])).unwrap().connect.map(|c| c.addr.port()), Some(18040));
        assert!(parse(&strings(&["--connect", "--demo"])).unwrap_err().contains("headless session"));
        assert!(parse(&strings(&["--connect", "--read-only"])).unwrap_err().contains("headless session"));
        assert!(parse(&strings(&["--connect", "10.0.0.5:7990"])).is_err(), "loopback only");
        assert!(parse(&strings(&["--control-token-file", "t"])).unwrap_err().contains("give --connect too"));
        let o = parse(&strings(&["--read-only", "--no-client-roots", "--automation-read-root", "in", "--automation-write-root", "out"])).unwrap();
        assert!(o.read_only && !o.client_roots);
        let cwd = std::env::current_dir().unwrap();
        assert_eq!((o.policy.read_roots(), o.policy.write_roots()), (&[cwd.join("in")][..], vec![cwd.join("out")]), "made absolute");
        assert!(!o.policy.allow_external() && !o.policy.allow_people());
        let o = parse(&strings(&["--automation-allow-external", "--automation-allow-people"])).unwrap();
        assert!(o.policy.allow_external() && o.policy.allow_people() && o.client_roots && !o.read_only);
        assert!(parse(&strings(&["--automation-read-root"])).unwrap_err().contains("needs a folder"));
        assert!(parse(&strings(&["--colour"])).unwrap_err().contains("unknown option"));
    }

    /// `--connect --automation-allow-people` builds a backend that lets the app's people data
    /// through; without the flag it withholds it.
    #[test]
    fn connect_takes_its_people_permission() {
        for (args, allow) in [(&["--connect", "--automation-allow-people"][..], true), (&["--connect"], false)] {
            let o = parse(&strings(args)).unwrap();
            let client = Client::from_options(o.connect.as_ref().unwrap()).unwrap();
            assert_eq!(connect_backend(client, &o.policy).allows_people(), allow, "{args:?}");
        }
    }
}
