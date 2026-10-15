//! The CLI: `commands` prints the engine's registry, `run` executes scripts on a library (exit codes
//! 0 / 1 / 2 / 3), `inspect` summarises one, and it prints the workspace version (the app's own
//! `--version` line is `apps/dalica`'s unit tests: building the app here cost a task's gate a
//! minute).

use std::path::Path;
use std::process::Command;

fn workspace_version() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let manifest = std::fs::read_to_string(root).unwrap();
    let section = manifest.split("[workspace.package]").nth(1).unwrap();
    let line = section.lines().find(|l| l.trim_start().starts_with("version")).unwrap();
    line.split('"').nth(1).unwrap().to_string()
}

/// `dalica-cli` with its thumbnail cache in `cache`, never the user's cache folder.
fn dalica_cli(cache: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_dalica-cli"));
    c.env("DALICA_CACHE_DIR", cache);
    c
}

fn cli(args: &[&str]) -> (bool, String) {
    let cache = tempfile::tempdir().unwrap();
    let out = dalica_cli(cache.path()).args(args).output().unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn commands_prints_the_registry() {
    let (ok, out) = cli(&["commands"]);
    assert!(ok);
    let list: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    let star = list.iter().find(|c| c["id"] == "photo.star.toggle").unwrap();
    assert_eq!((star["label"].as_str(), star["shortcut"].as_str()), (Some("Star / Unstar"), Some("Ctrl+8")));
    for id in ["folders.add", "library.query", "library.rebuild", "photo.inspect", "photo.star.set", "edit.undo", "edit.redo", "app.setShortcut"] {
        assert!(list.iter().any(|c| c["id"] == id), "{id}");
    }
}

fn run(args: &[&str], stdin: &str) -> (Option<i32>, Vec<serde_json::Value>, String) {
    use std::io::Write;
    let cache = tempfile::tempdir().unwrap();
    let mut child = dalica_cli(cache.path())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let lines = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (out.status.code(), lines, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn run_prints_one_result_per_line_and_sets_the_exit_code() {
    let lib = tempfile::tempdir().unwrap();
    let dir = lib.path().to_str().unwrap();
    let (code, lines, _) = run(&["run", "--library", dir], "# a comment\n\n{\"command\": \"library.query\", \"params\": {}}\n");
    assert_eq!(code, Some(0));
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["ok"], true);
    assert_eq!(lines[0]["result"]["total"], 0);
    // A failed command: exit 1, the script goes on.
    let (code, lines, _) = run(&["run", "--library", dir, "-"], "{\"command\": \"edit.undo\"}\n{\"command\": \"library.query\"}\n");
    assert_eq!(code, Some(1));
    assert_eq!((lines[0]["ok"].as_bool(), lines[0]["code"].as_str(), lines[1]["ok"].as_bool()), (Some(false), Some("disabled"), Some(true)));
    // A bad line: exit 2 at that line.
    for bad in ["not json\n", "{\"params\": {}}\n", "{\"command\": \"x\", \"extra\": 1}\n"] {
        let (code, lines, err) =
            run(&["run", "--library", dir], &format!("{{\"command\": \"library.query\"}}\n{bad}{{\"command\": \"library.query\"}}\n"));
        assert_eq!(code, Some(2), "{bad}");
        assert_eq!(lines.len(), 1, "{bad}");
        assert!(err.contains("line 2"), "{err}");
    }
    // One command with --cmd.
    let (code, lines, _) = run(&["run", "--library", dir, "--cmd", "photo.inspect", "--params", "{\"ids\": [5]}"], "");
    assert_eq!((code, lines[0]["code"].as_str()), (Some(1), Some("not_found")));
    let (code, ..) = run(&["run", "--library", dir, "--cmd", "photo.inspect", "--params", "{oops"], "");
    assert_eq!(code, Some(2));
    let (code, ..) = run(&["run"], "");
    assert_eq!(code, Some(2));
}

#[test]
fn inspect_summarises_a_library() {
    let lib = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let out = dalica_cli(cache.path()).args(["inspect", "--library", lib.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((v["photos"].as_u64(), v["folders"].as_array().map(Vec::len)), (Some(0), Some(0)));
}

/// Every file `dalica-cli` made under `dir`.
fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// A run on a named library keeps its thumbnails in `$DALICA_CACHE_DIR/thumbs` and writes nothing
/// to the platform's cache folder (pointed at another temporary folder here).
#[test]
fn a_run_on_a_library_keeps_its_thumbnails_in_dalica_cache_dir() {
    let dir = tempfile::tempdir().unwrap();
    let (lib, cache, platform) = (dir.path().join("library"), dir.path().join("cache"), dir.path().join("platform"));
    let photos = dir.path().join("photos");
    for d in [&cache, &platform, &photos] {
        std::fs::create_dir_all(d).unwrap();
    }
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 400, 300), 80, None).unwrap();
    std::fs::write(photos.join("IMG_0001.jpg"), jpeg).unwrap();
    let add = serde_json::json!({ "path": photos }).to_string();
    let out = dalica_cli(&cache)
        .args(["run", "--library", lib.to_str().unwrap(), "--cmd", "folders.add", "--params", &add])
        .envs([("HOME", &platform), ("XDG_CACHE_HOME", &platform), ("LOCALAPPDATA", &platform)])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let made = files_under(&cache.join("thumbs"));
    let names: Vec<String> = made.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.starts_with("128-") && n.ends_with(".pack")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("256-") && n.ends_with(".pack")), "{names:?}");
    assert_eq!(files_under(&platform), Vec::<std::path::PathBuf>::new(), "nothing in the platform cache folder");
}

#[test]
fn cli_version_is_the_workspace_version() {
    let (ok, out) = cli(&["--version"]);
    assert!(ok);
    // `<version> (<commit>, <date>)`, or `<version> (dev build)` without the build variables.
    let out = out.trim();
    assert!(out.starts_with(&format!("dalica-cli {} (", workspace_version())) && out.ends_with(')'), "{out}");
}

#[test]
fn unknown_commands_fail_with_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli")).arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: dalica-cli"));
}
