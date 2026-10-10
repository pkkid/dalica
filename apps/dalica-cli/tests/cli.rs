//! The CLI: `commands` prints the engine's registry, `run` executes scripts on a library (exit codes
//! 0 / 1 / 2 / 3), `inspect` summarises one, and both binaries print the workspace version.

use std::path::Path;
use std::process::Command;

fn workspace_version() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let manifest = std::fs::read_to_string(root).unwrap();
    let section = manifest.split("[workspace.package]").nth(1).unwrap();
    let line = section.lines().find(|l| l.trim_start().starts_with("version")).unwrap();
    line.split('"').nth(1).unwrap().to_string()
}

fn cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli")).args(args).output().unwrap();
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_dalica-cli"))
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
    let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli")).args(["inspect", "--library", lib.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((v["photos"].as_u64(), v["folders"].as_array().map(Vec::len)), (Some(0), Some(0)));
}

#[test]
fn cli_version_is_the_workspace_version() {
    let (ok, out) = cli(&["--version"]);
    assert!(ok);
    assert_eq!(out.trim(), format!("dalica-cli {}", workspace_version()));
}

#[test]
fn app_version_is_the_workspace_version() {
    // `dalica` is another package's binary, so build and run it through cargo (cargo has released
    // the build lock by the time tests run).
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo).args(["run", "--quiet", "-p", "dalica", "--", "--version"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), format!("dalica {}", workspace_version()));
}

#[test]
fn unknown_commands_fail_with_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli")).arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: dalica-cli"));
}
