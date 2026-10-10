//! M0.8: the CLI stubs. `commands` prints the (empty) registry and both binaries print the workspace
//! version.

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
fn commands_prints_an_empty_registry() {
    let (ok, out) = cli(&["commands"]);
    assert!(ok);
    assert_eq!(out.trim(), "[]");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), serde_json::json!([]));
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
