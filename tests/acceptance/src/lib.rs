//! Helpers shared by the milestone acceptance tests (`tests/acceptance/m<n>.rs`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root (two levels above this crate's manifest).
pub fn workspace_root() -> PathBuf {
    // Cargo sets `CARGO_MANIFEST_DIR` when it runs a test; prefer that to the compile-time value, so a
    // test binary reused from another checkout (shared target dir) still checks this one.
    let here =
        std::env::var_os("CARGO_MANIFEST_DIR").filter(|v| !v.is_empty()).map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    here.ancestors().nth(2).map(Path::to_path_buf).unwrap_or(here)
}

/// A `cargo` command for the toolchain running the tests, started in the workspace root. It never
/// passes `DALICA_BLESS` down: blessing a milestone's own baselines must not re-bless the crate
/// suites and builds it runs.
pub fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(workspace_root()).env_remove("DALICA_BLESS");
    cmd
}

/// Set to `1` by `cargo xtask ci` on its `accept` steps, which run after its workspace test step
/// (`docs/development.md` §3): the crate suites an acceptance test would re-run (`cargo test -p …`)
/// already ran in that step, so the test skips them. A standalone `cargo xtask accept M<n>` clears
/// it, so one command still checks the whole *Done when*.
pub const SUITES_RAN: &str = "DALICA_SUITES_RAN";

/// Whether the crate suites already ran in this `cargo xtask ci` ([`SUITES_RAN`]); prints so when
/// they did, so the log says why they are missing.
pub fn suites_ran() -> bool {
    let ran = std::env::var_os(SUITES_RAN).is_some_and(|v| v == "1");
    if ran {
        println!("suites: run by the ci test step ({SUITES_RAN}=1)");
    }
    ran
}

/// A package's directory (relative to the workspace root) and manifest text, found by its name
/// among the workspace members (`crates/*`, `apps/*`, `xtask`, `tests/acceptance`).
fn member(name: &str) -> Option<(PathBuf, String)> {
    let root = workspace_root();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for group in ["crates", "apps"] {
        if let Ok(entries) = std::fs::read_dir(root.join(group)) {
            dirs.extend(entries.flatten().map(|e| e.path()));
        }
    }
    dirs.extend([root.join("xtask"), root.join("tests").join("acceptance")]);
    dirs.into_iter().find_map(|dir| {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        let named = manifest.lines().map(str::trim).find(|l| l.starts_with("name"))?.split('=').nth(1)?.trim().trim_matches('"').to_string();
        (named == name).then_some((dir, manifest))
    })
}

/// The `[[test]]` target names a manifest declares.
fn declared_tests(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_test = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_test = line == "[[test]]";
            continue;
        }
        if let Some(value) = line.strip_prefix("name").map(str::trim_start).and_then(|l| l.strip_prefix('='))
            && in_test
        {
            names.push(value.trim().trim_matches('"').to_string());
        }
    }
    names
}

/// Flags of `cargo test` that take a value.
const VALUE_FLAGS: [&str; 10] = ["-p", "--package", "--features", "-F", "--test", "--bench", "-j", "--jobs", "--target-dir", "--profile"];

/// The `cargo test` commands that run what `args` names, now that each crate's integration tests
/// are modules of one binary (`tests/all.rs`, `docs/development.md` §6): `--test <file>` becomes
/// the filter `<file>::` on `--test all`, while targets the manifest still declares on their own
/// (`separate`: self-exec, required features) keep their `--test`. A positional test-name filter
/// stays as it is (the module filters are then left out: libtest ORs its filters). Separate and
/// merged targets go in separate commands, so one's filters never hide the other's tests.
pub fn split_test_targets(args: &[&str], has_all: bool, separate: &[String]) -> Vec<Vec<String>> {
    let unchanged = vec![args.iter().map(|a| a.to_string()).collect()];
    if args.first() != Some(&"test") || !has_all {
        return unchanged;
    }
    let (before, tail) = match args.iter().position(|a| *a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };
    let mut base: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut modules: Vec<String> = Vec::new();
    let mut positional = false;
    let mut i = 0;
    while i < before.len() {
        let a = before[i];
        if a == "--test" {
            if let Some(t) = before.get(i + 1) {
                if *t == "all" || separate.iter().any(|s| s == t) {
                    kept.push((*t).to_string());
                } else {
                    modules.push((*t).to_string());
                }
            }
            i += 2;
            continue;
        }
        base.push(a.to_string());
        if VALUE_FLAGS.contains(&a) {
            if let Some(v) = before.get(i + 1) {
                base.push((*v).to_string());
            }
            i += 2;
            continue;
        }
        if i > 0 && !a.starts_with('-') {
            positional = true;
        }
        i += 1;
    }
    if modules.is_empty() {
        return unchanged;
    }
    let tail: Vec<String> = tail.iter().map(|a| a.to_string()).collect();
    let mut out = Vec::new();
    if !kept.is_empty() {
        let mut cmd = base.clone();
        for t in &kept {
            cmd.extend(["--test".to_string(), t.clone()]);
        }
        if !tail.is_empty() {
            cmd.push("--".into());
            cmd.extend(tail.iter().cloned());
        }
        out.push(cmd);
    }
    let mut cmd = base;
    cmd.extend(["--test".to_string(), "all".to_string(), "--".to_string()]);
    cmd.extend(tail);
    if !positional {
        cmd.extend(modules.iter().map(|m| format!("{m}::")));
    }
    out.push(cmd);
    out
}

/// [`split_test_targets`] for the package `args` name with `-p`, read from its manifest.
pub fn test_commands(args: &[&str]) -> Vec<Vec<String>> {
    let package = args.iter().position(|a| *a == "-p" || *a == "--package").and_then(|i| args.get(i + 1));
    match package.and_then(|p| member(p)) {
        Some((dir, manifest)) => split_test_targets(args, dir.join("tests").join("all.rs").is_file(), &declared_tests(&manifest)),
        None => vec![args.iter().map(|a| a.to_string()).collect()],
    }
}

/// Runs the `cargo` commands [`test_commands`] makes of `args` one after another, each prepared
/// by `prepare` (environment variables), and returns their output joined: the first failure's
/// status (later commands are not run), else the last success's.
pub fn cargo_split(args: &[&str], prepare: impl Fn(&mut Command)) -> std::io::Result<std::process::Output> {
    let mut joined: Option<std::process::Output> = None;
    for cmd_args in test_commands(args) {
        let mut cmd = cargo();
        cmd.args(&cmd_args);
        prepare(&mut cmd);
        let mut out = cmd.output()?;
        if let Some(before) = joined.take() {
            out.stdout = [before.stdout, out.stdout].concat();
            out.stderr = [before.stderr, out.stderr].concat();
        }
        let failed = !out.status.success();
        joined = Some(out);
        if failed {
            break;
        }
    }
    joined.ok_or_else(|| std::io::Error::other("no cargo command to run"))
}

/// The engine's integration-test binary (one per crate: `crates/engine/tests/all.rs`).
pub const ENGINE_TESTS: &str = "all";

/// `panic_hunt`: every registered command with hostile params, never a panic, at full strength
/// (`DALICA_FULL_TESTS=1`). It is an ordinary engine test, so the test step of `cargo xtask ci
/// --full` has run it by the time an `accept` step starts ([`SUITES_RAN`]): it runs once per review
/// gate. A standalone `cargo xtask accept M<n>` runs it here. `Err` holds the output of a failure.
pub fn panic_hunt() -> Result<(), String> {
    if suites_ran() {
        return Ok(());
    }
    let out = cargo()
        .args(["test", "-p", "dalica-engine", "--test", ENGINE_TESTS, "panic_hunt"])
        .env("DALICA_FULL_TESTS", "1")
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let passed: usize =
        stdout.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    if out.status.success() && passed > 0 {
        println!("panic_hunt: {passed} passed");
        Ok(())
    } else {
        Err(format!("panic_hunt failed or ran nothing:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr)))
    }
}

/// The production-code attributes every crate root must carry (`docs/standards/never-crash.md`).
pub const CRATE_ROOT_ATTRIBUTES: [&str; 2] = [
    "#![forbid(unsafe_code)]",
    "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(args: &[&str], separate: &[&str]) -> Vec<String> {
        let separate: Vec<String> = separate.iter().map(|s| s.to_string()).collect();
        split_test_targets(args, true, &separate).into_iter().map(|c| c.join(" ")).collect()
    }

    #[test]
    fn test_files_become_module_filters_on_the_one_binary() {
        assert_eq!(
            cmds(&["test", "-p", "dalica-ui", "--test", "layout", "--test", "menus"], &["all"]),
            ["test -p dalica-ui --test all -- layout:: menus::"]
        );
        // Separate targets keep their --test, in a command of their own.
        assert_eq!(
            cmds(&["test", "-p", "dalica-image", "--features", "heif", "--test", "heif", "--test", "decode", "--", "--nocapture"], &["all", "heif"]),
            [
                "test -p dalica-image --features heif --test heif -- --nocapture",
                "test -p dalica-image --features heif --test all -- --nocapture decode::"
            ]
        );
        // A test-name filter stays; the module filter would widen it.
        assert_eq!(
            cmds(&["test", "-p", "dalica-store", "--test", "sidecars", "a_failed_write"], &["all"]),
            ["test -p dalica-store a_failed_write --test all --"]
        );
        // Nothing to change without a merged binary, without --test, or for another command.
        assert_eq!(split_test_targets(&["test", "-p", "x", "--test", "a"], false, &[]), [vec!["test", "-p", "x", "--test", "a"]]);
        assert_eq!(cmds(&["test", "-p", "dalica-engine", "--lib"], &["all"]), ["test -p dalica-engine --lib"]);
        assert_eq!(cmds(&["bench", "-p", "dalica-ui", "--bench", "b"], &["all"]), ["bench -p dalica-ui --bench b"]);
    }
}
