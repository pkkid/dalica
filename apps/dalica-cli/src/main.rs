//! Dalica's headless command line (`docs/architecture.md` §3, §7): `commands` lists the command
//! registry as JSON, `run` executes a JSON-lines script on a library, `inspect` summarises one,
//! `snapshot` screenshots the app headlessly. `--demo` in place of `--library DIR` uses the demo
//! library (made on first use under the temp folder and kept); `--demo-fresh` a new one that is
//! removed on exit.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod inspect;
mod run;
mod snapshot;

use std::process::ExitCode;

const USAGE: &str = "usage: dalica-cli <command>

commands:
  commands                                   the command registry as JSON
  run --library DIR [SCRIPT | -]             run a JSON-lines script ({\"command\": id, \"params\": {...}} per line;
                                             stdin when no SCRIPT or `-`); one JSON result per line
  run --library DIR --cmd ID [--params JSON] run one command
  inspect --library DIR                      the library as JSON: folders, photos, stars, repairs
  snapshot --library DIR -o OUT.png [--size 1400x900] [--scale 1] [--script STEPS.jsonl]
                                             a headless screenshot of the app; script lines are
                                             {\"click\": id}, {\"key\": \"Ctrl+8\"}, {\"command\": id, \"params\": {...}},
                                             {\"settle\": true}
  --version                                  print the version

--demo may replace --library DIR in run, inspect and snapshot: the demo library of generated photos in
the temp folder, made on first use and kept between runs. --demo-fresh: a new demo library, removed on exit.

exit codes: 0 success, 1 a command failed, 2 a bad argument or script line, 3 the library could not be
opened (for example, another Dalica process has it open)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (args, fresh) = match demo_args(args, &dalica_engine::demo::default_dir()) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let code = dispatch(&args);
    // The command's session is gone: a fresh demo's folder can go too.
    if let Some(dir) = fresh
        && let Err(e) = dir.close()
    {
        eprintln!("dalica-cli: the fresh demo folder was not removed: {e}");
    }
    code
}

fn dispatch(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("dalica-cli {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h" | "help") => println!("{USAGE}"),
        Some("commands") => println!("{}", commands()),
        Some("run") => return run::main(args.get(1..).unwrap_or_default()),
        Some("inspect") => return inspect::main(args.get(1..).unwrap_or_default()),
        Some("snapshot") => return snapshot::main(args.get(1..).unwrap_or_default()),
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

/// A command's default keys on this platform, as `{context, key}`: keys for other platforms left
/// out, and no `mac:` / `win,linux:` qualifier ever shown.
fn shown_keys(keys: dalica_engine::keymap::DefaultKeys) -> Vec<serde_json::Value> {
    let here = dalica_engine::keymap::Platform::current();
    keys.iter()
        .filter_map(|(c, k)| match dalica_engine::keymap::parse_default(k, here) {
            Ok((key, true)) => Some(serde_json::json!({ "context": c, "key": key.to_string() })),
            _ => None,
        })
        .collect()
}

/// A command's first default key on this platform, without its qualifier.
fn first_key(keys: dalica_engine::keymap::DefaultKeys) -> Option<&'static str> {
    dalica_engine::keymap::first_default(keys, dalica_engine::keymap::Platform::current())
}

/// The registry as `engine.commands` lists it, UI-only commands included, without a library (so
/// `enabled` is not evaluated).
fn commands() -> String {
    let list: Vec<serde_json::Value> = dalica_engine::cmd::specs()
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "label": s.label, "menu": s.menu, "shortcut": first_key(s.keys), "keys": shown_keys(s.keys),
                "params": s.params, "effect": s.effect, "history": s.history, "ui": false, "live": true,
            })
        })
        .chain(dalica_ui::UI_COMMANDS.iter().map(|c| {
            serde_json::json!({
                "id": c.id, "label": c.label, "menu": c.menu, "shortcut": first_key(c.keys), "keys": shown_keys(c.keys),
                "params": c.params, "effect": "query", "history": "none", "ui": true, "live": true,
            })
        }))
        .collect();
    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into())
}

/// Replaces `--demo` (the persistent demo in `persistent`) or `--demo-fresh` (a new one) with
/// `--library <its library>`, after making and scanning it. A fresh demo's folder is returned, for
/// `main` to remove once the command is done. Neither flag: `args` as they are.
fn demo_args(mut args: Vec<String>, persistent: &std::path::Path) -> Result<(Vec<String>, Option<dalica_fs::TempDir>), ExitCode> {
    let demo = args.iter().position(|a| a == "--demo");
    let fresh = args.iter().position(|a| a == "--demo-fresh");
    let i = match (demo, fresh) {
        (None, None) => return Ok((args, None)),
        (Some(_), Some(_)) => return Err(usage_error("--demo and --demo-fresh cannot be combined")),
        _ if args.iter().any(|a| a == "--library") => return Err(usage_error("--demo cannot be combined with --library")),
        (Some(i), None) | (None, Some(i)) => i,
    };
    let temp = match fresh {
        Some(_) => match dalica_engine::demo::fresh_dir() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("dalica-cli: cannot make a fresh demo folder: {e}");
                return Err(ExitCode::from(3));
            }
        },
        None => None,
    };
    let dir = temp.as_ref().map_or(persistent, |t| t.path()).to_path_buf();
    let opened = dalica_engine::demo::open(&dir, std::sync::Arc::new(dalica_core::SystemClock));
    match opened.map(|mut s| s.flush()) {
        Ok(Ok(())) => {
            let library = dalica_engine::demo::library_dir(&dir).to_string_lossy().into_owned();
            args.splice(i..=i, ["--library".to_string(), library]);
            Ok((args, temp))
        }
        Ok(Err(e)) => {
            eprintln!("dalica-cli: the demo library was not saved: {e}");
            Err(ExitCode::from(3))
        }
        Err(e) => {
            eprintln!("dalica-cli: cannot open the demo library: {e}");
            Err(ExitCode::from(3))
        }
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("dalica-cli: {msg}\n\n{USAGE}");
    ExitCode::from(2)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    fn photos(library: &str) -> Vec<(u64, String)> {
        let mut s = open(std::path::Path::new(library)).unwrap();
        let q = s.execute("library.query", &serde_json::json!({})).unwrap();
        q["photos"].as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), p["name"].as_str().unwrap().to_string())).collect()
    }

    #[test]
    fn demo_fresh_is_a_new_library_removed_with_its_folder() {
        let unused = tempfile::tempdir().unwrap();
        let (args, fresh) = demo_args(strings(&["inspect", "--demo-fresh"]), unused.path()).unwrap();
        let fresh = fresh.unwrap();
        assert_eq!(args[..2], strings(&["inspect", "--library"]));
        assert_eq!(std::path::Path::new(&args[2]), dalica_engine::demo::library_dir(fresh.path()));
        let first = photos(&args[2]);
        assert_eq!(first.len(), dalica_engine::demo::PHOTOS);
        // A second fresh demo is another folder with the same ids for the same photos.
        let (again, other) = demo_args(strings(&["inspect", "--demo-fresh"]), unused.path()).unwrap();
        assert_ne!(again[2], args[2]);
        assert_eq!(photos(&again[2]), first);
        let path = fresh.path().to_path_buf();
        fresh.close().unwrap();
        drop(other);
        assert!(!path.exists());
        assert!(std::fs::read_dir(unused.path()).unwrap().next().is_none(), "the persistent demo was not touched");
    }

    /// No default key reaches the output with its platform qualifier (`win,linux:Ctrl+Y`); Redo
    /// shows this platform's key.
    #[test]
    fn commands_show_no_platform_qualifier() {
        let list: Vec<serde_json::Value> = serde_json::from_str(&commands()).unwrap();
        let mut shown = Vec::new();
        for c in &list {
            shown.extend(c["shortcut"].as_str().map(str::to_string));
            shown.extend(c["keys"].as_array().unwrap().iter().map(|k| k["key"].as_str().unwrap().to_string()));
        }
        assert!(shown.len() > 5, "{shown:?}");
        assert!(shown.iter().all(|k| !k.contains(':')), "{shown:?}");
        let redo = list.iter().find(|c| c["id"] == "edit.redo").unwrap();
        let want = if cfg!(target_os = "macos") { "Ctrl+Shift+Z" } else { "Ctrl+Y" };
        assert_eq!(redo["shortcut"], want);
        assert_eq!(redo["keys"], serde_json::json!([{ "context": "library", "key": want }]));
        let small = list.iter().find(|c| c["id"] == "view.thumbsSmall").unwrap();
        assert_eq!(small["shortcut"], "Ctrl+1");
    }

    #[test]
    fn demo_is_the_persistent_library() {
        let dir = tempfile::tempdir().unwrap();
        let (args, fresh) = demo_args(strings(&["run", "--demo", "-"]), dir.path()).unwrap();
        assert!(fresh.is_none());
        assert_eq!(args, strings(&["run", "--library", &dalica_engine::demo::library_dir(dir.path()).to_string_lossy(), "-"]));
        let (args, fresh) = demo_args(strings(&["inspect", "--library", "x"]), dir.path()).unwrap();
        assert_eq!((args, fresh.is_none()), (strings(&["inspect", "--library", "x"]), true));
    }

    #[test]
    fn demo_flags_do_not_combine() {
        let dir = tempfile::tempdir().unwrap();
        for bad in
            [&["inspect", "--demo", "--demo-fresh"][..], &["inspect", "--demo", "--library", "x"], &["inspect", "--library", "x", "--demo-fresh"]]
        {
            assert!(demo_args(strings(bad), dir.path()).is_err(), "{bad:?}");
        }
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
