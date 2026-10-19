//! Dalica's headless command line (`docs/architecture.md` §3, §7): `commands` lists the command
//! registry as JSON, `run` executes a JSON-lines script on a library, `inspect` summarises one,
//! `snapshot` screenshots the app headlessly, `mcp` serves MCP on standard input and output
//! (`mcp.rs`). `--demo` in place of `--library DIR` uses the demo library (made on first use under
//! the temp folder and kept; `mcp`'s own demo is deleted on exit); `--demo-fresh` a new one that is
//! removed on exit.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod inspect;
mod mcp;
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
  mcp [--demo | --library DIR] [--compact] [FOLDERS…]
                                             the MCP server on standard input and output over a headless
                                             session (`mcp --help`); its --demo is deleted on exit
  --version                                  print the version, with its commit and build date

--demo may replace --library DIR in run, inspect and snapshot: the demo library of generated photos in
the temp folder, made on first use and kept between runs. --demo-fresh: a new demo library, removed on exit.

exit codes: 0 success, 1 a command failed, 2 a bad argument or script line, 3 the library could not be
opened (for example, another Dalica process has it open)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // A library named on the command line keeps its thumbnails in Dalica's cache folder; a demo
    // keeps a temporary cache that goes with its session.
    if !args.iter().any(|a| a == "--demo" || a == "--demo-fresh") {
        let exe = std::env::current_exe().ok();
        dalica_engine::thumbs::set_process_cache_dir(cache_dir(|k| std::env::var_os(k), exe.as_deref().and_then(std::path::Path::parent)));
    }
    // `mcp --demo` is a temporary demo of its own, deleted on exit, not the persistent one.
    if args.first().map(String::as_str) == Some("mcp") {
        return mcp::main(args.get(1..).unwrap_or_default());
    }
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
        Some("--version" | "-V") => print(&version_line()),
        Some("--help" | "-h" | "help") => print(USAGE),
        Some("commands") => print(&commands()),
        Some("run") => run::main(args.get(1..).unwrap_or_default()),
        Some("inspect") => inspect::main(args.get(1..).unwrap_or_default()),
        Some("snapshot") => snapshot::main(args.get(1..).unwrap_or_default()),
        Some(other) => {
            eprintln!("dalica-cli: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Writes `text` and a newline to `out` and flushes it: the one output path of every subcommand
/// (the standard print macros panic when standard output is closed). A closed reader (`BrokenPipe`, as
/// `dalica-cli commands | head` leaves it) is a quiet, successful end: `Err(0)` with nothing on
/// standard error. Any other write error is reported on standard error: `Err(1)`.
pub(crate) fn emit(out: &mut impl std::io::Write, text: &str) -> Result<(), ExitCode> {
    match writeln!(out, "{text}").and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Err(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("dalica-cli: cannot write the output: {e}");
            Err(ExitCode::from(1))
        }
    }
}

/// [`emit`] to standard output, then exit 0 (or the code `emit` gave).
pub(crate) fn print(text: &str) -> ExitCode {
    match emit(&mut std::io::stdout().lock(), text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// The thumbnail cache: `$DALICA_CACHE_DIR`, else `data/cache/thumbs` beside the executable when a
/// `portable` marker file is there (portable mode, as `apps/dalica/src/paths.rs` resolves it for the
/// app), else the platform cache folder.
fn cache_dir(var: impl Fn(&str) -> Option<std::ffi::OsString>, exe_dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let set = var(dalica_engine::thumbs::CACHE_DIR_VAR).filter(|v| !v.is_empty()).is_some_and(|v| std::path::Path::new(&v).is_absolute());
    match exe_dir.filter(|d| d.join("portable").is_file()) {
        Some(d) if !set => Some(d.join("data").join("cache").join("thumbs")),
        _ => dalica_engine::thumbs::default_cache_dir(var),
    }
}

/// What `--version` prints: the version with its commit and build date (`releasing.md`).
fn version_line() -> String {
    format!("dalica-cli {}", dalica_core::build::version_string())
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
/// `enabled` is not evaluated). An engine command with an opener (`dalica_ui::commands::OPENERS`:
/// Rename… opens its dialog) lists the opener's keys after its own, as the app's keymap binds them
/// (F2 for `photo.rename`).
fn commands() -> String {
    let list: Vec<serde_json::Value> = dalica_engine::cmd::specs()
        .iter()
        .map(|s| {
            let opener = dalica_ui::commands::opener(s.id).map_or(&[][..], |o| o.keys);
            let mut keys = shown_keys(s.keys);
            keys.extend(shown_keys(opener));
            serde_json::json!({
                "id": s.id, "label": s.label, "menu": s.menu, "shortcut": first_key(s.keys).or_else(|| first_key(opener)), "keys": keys,
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
        Ok(mut s) => {
            s.set_face_models_dir(dalica_engine::faces_scan::default_models_dir());
            Ok(s)
        }
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

    /// The openers' keys are listed with their engine commands: Rename… on F2 in the Library and
    /// the edit view (`menus-and-shortcuts.md` §2.1, M10.41), and every opener's keys somewhere.
    /// An opener may come before its engine command (the person menu's rows wait for
    /// `people.rename`, Clear Location… for `geo.clear`): it is not a command yet, so it is not
    /// listed, and it must have no keys, which would be bound in the app and listed nowhere.
    #[test]
    fn commands_list_the_openers_keys() {
        let list: Vec<serde_json::Value> = serde_json::from_str(&commands()).unwrap();
        let rename = list.iter().find(|c| c["id"] == "photo.rename").unwrap();
        assert_eq!(rename["shortcut"], "F2");
        assert_eq!(rename["keys"], serde_json::json!([{ "context": "library", "key": "F2" }, { "context": "editView", "key": "F2" }]));
        for o in dalica_ui::commands::OPENERS {
            let registered = dalica_engine::cmd::specs().iter().any(|s| s.id == o.id);
            let listed = list.iter().find(|c| c["id"] == o.id);
            assert_eq!(listed.is_some(), registered, "{} is listed exactly when its engine command is registered", o.id);
            let Some(c) = listed else {
                assert!(o.keys.is_empty(), "{} has keys but no engine command to list them with", o.id);
                continue;
            };
            for k in shown_keys(o.keys) {
                assert!(c["keys"].as_array().unwrap().contains(&k), "{}: {k}", o.id);
            }
        }
    }

    #[test]
    fn the_cache_follows_portable_mode() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Dalica");
        std::fs::create_dir_all(&exe).unwrap();
        let home = dir.path().join("home");
        let cache = dir.path().join("cache");
        let var = |k: &str| match k {
            "HOME" | "LOCALAPPDATA" => Some(home.clone().into_os_string()),
            _ => None,
        };
        // No marker: the platform cache folder, as before.
        assert_eq!(cache_dir(var, Some(&exe)), dalica_engine::thumbs::default_cache_dir(var));
        assert_eq!(cache_dir(var, None), dalica_engine::thumbs::default_cache_dir(var));
        std::fs::write(exe.join("portable"), b"").unwrap();
        assert_eq!(cache_dir(var, Some(&exe)), Some(exe.join("data").join("cache").join("thumbs")));
        assert_eq!(cache_dir(|_| None, Some(&exe)), Some(exe.join("data").join("cache").join("thumbs")));
        // $DALICA_CACHE_DIR still wins.
        let with_var = |k: &str| (k == dalica_engine::thumbs::CACHE_DIR_VAR).then(|| cache.clone().into_os_string());
        assert_eq!(cache_dir(with_var, Some(&exe)), Some(cache.join("thumbs")));
        assert!(!exe.join("data").exists(), "nothing is made until a thumbnail is written");
    }

    #[test]
    fn version_names_the_build() {
        let v = version_line();
        assert_eq!(v, format!("dalica-cli {}", dalica_core::build::version_string()));
        assert!(v.starts_with(&format!("dalica-cli {} (", env!("CARGO_PKG_VERSION"))), "{v}");
        assert_eq!(dispatch(&strings(&["--version"])), ExitCode::SUCCESS);
    }

    /// A writer whose every write fails with `kind`.
    struct Failing(std::io::ErrorKind);

    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A closed pipe ends quietly with exit 0; another write error is exit 1; a write is a line.
    #[test]
    fn emit_ends_quietly_on_a_closed_pipe() {
        assert_eq!(emit(&mut Failing(std::io::ErrorKind::BrokenPipe), "x"), Err(ExitCode::SUCCESS));
        assert_eq!(emit(&mut Failing(std::io::ErrorKind::PermissionDenied), "x"), Err(ExitCode::from(1)));
        let mut out = Vec::new();
        assert_eq!(emit(&mut out, "a"), Ok(()));
        assert_eq!(out, b"a\n");
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
