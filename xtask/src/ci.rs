//! `cargo xtask ci`: THE gate (`docs/development.md` §3). Runs every step even after a failure, so
//! one run reports everything, then prints a table and exits non-zero if any step failed. A green
//! run on a clean worktree records the commit for `cargo xtask tasks submit`.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use crate::tasks;

/// Test threads and build jobs: one per GiB of available memory, at most the CPUs or
/// `CARGO_BUILD_JOBS`, at least one. Several agents share one machine (`task-board.md` §7).
pub fn parallelism(available_gib: Option<u64>, cpus: usize, build_jobs: Option<usize>) -> usize {
    let cap = build_jobs.unwrap_or(cpus).max(1);
    let by_ram = available_gib.map_or(cap, |g| usize::try_from(g).unwrap_or(usize::MAX));
    cap.min(by_ram).max(1)
}

/// `MemAvailable` from `/proc/meminfo`, in GiB (Linux; `None` elsewhere).
pub fn available_gib(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / (1024 * 1024))
}

fn cargo(root: &Path, args: &[&str]) -> Result<(), String> {
    cargo_env(root, args, &[])
}

fn cargo_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

/// The acceptance tests' package. The workspace test step leaves it out: each milestone's test runs
/// once, in its `accept` step, and the rest of the package in the `test acceptance` step.
pub const ACCEPTANCE: &str = "dalica-acceptance";

/// The `[[test]]` targets of the acceptance manifest that no `accept` step runs, in its order: all
/// but `m<k>` and `m<k>_headless` for each milestone in `accepted` (`m<k>` runs the headless half's
/// steps first, so `m<k>_headless` would repeat them; it stays for `accept M<k> --headless`). The
/// skeleton checks and the acceptance test of a milestone still in progress keep running in `ci`.
pub fn acceptance_rest(manifest: &str, accepted: &[u32]) -> Vec<String> {
    let mut rest = Vec::new();
    let mut in_test = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_test = line == "[[test]]";
            continue;
        }
        let Some(value) = line.strip_prefix("name").map(str::trim_start).and_then(|l| l.strip_prefix('=')) else { continue };
        let name = value.trim().trim_matches('"');
        if in_test && !name.is_empty() && !accepted.iter().any(|m| name == format!("m{m}") || name == format!("m{m}_headless")) {
            rest.push(name.to_string());
        }
    }
    rest
}

/// Milestones whose every task block is ✅ (their acceptance checks keep running).
pub fn finished_milestones(roadmap: &tasks::roadmap::Roadmap) -> Vec<u32> {
    roadmap
        .milestones
        .iter()
        .map(|m| m.number)
        .filter(|n| {
            let mut tasks = roadmap.tasks.iter().filter(|t| t.milestone == *n).peekable();
            tasks.peek().is_some() && tasks.all(|t| t.done)
        })
        .collect()
}

pub fn run(root: &Path) -> Result<(), String> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let cpus = std::thread::available_parallelism().map(usize::from).unwrap_or(2);
    let build_jobs = std::env::var("CARGO_BUILD_JOBS").ok().and_then(|j| j.parse().ok());
    let n = parallelism(available_gib(&meminfo), cpus, build_jobs).to_string();
    let threads = format!("--test-threads={n}");

    let roadmap = tasks::load_roadmap(root);
    let accepted = roadmap.as_ref().map(finished_milestones).unwrap_or_default();
    // The crate suites the acceptance tests would re-run have run in the test step by the time
    // they start, so they skip them (`accept::SUITES_RAN`).
    let suites_ran = [(crate::accept::SUITES_RAN, "1")];

    type Step<'a> = (String, Box<dyn Fn() -> Result<(), String> + 'a>);
    let mut steps: Vec<Step> = vec![
        ("fmt".into(), Box::new(|| cargo(root, &["fmt", "--all", "--", "--check"]))),
        // Clippy still covers the acceptance package (all targets).
        ("clippy".into(), Box::new(|| cargo(root, &["clippy", "--workspace", "--all-targets", "-j", &n, "--", "-D", "warnings"]))),
        ("test".into(), Box::new(|| cargo(root, &["test", "--workspace", "--exclude", ACCEPTANCE, "-j", &n, "--", &threads]))),
        (
            "test acceptance".into(),
            Box::new(|| {
                let manifest = root.join("tests").join("acceptance").join("Cargo.toml");
                let manifest = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
                let rest = acceptance_rest(&manifest, &accepted);
                println!("ci: {ACCEPTANCE}: the library and {} (each finished milestone's test runs in its accept step)", rest.join(", "));
                let mut args = vec!["test", "-p", ACCEPTANCE, "-j", &n, "--lib"];
                for test in &rest {
                    args.extend(["--test", test.as_str()]);
                }
                args.extend(["--", &threads]);
                cargo_env(root, &args, &suites_ran)
            }),
        ),
        ("layers".into(), Box::new(|| crate::layers::run(root))),
        ("deps".into(), Box::new(|| crate::deps::run(root))),
        ("assets".into(), Box::new(|| crate::assets::run(root))),
        ("names".into(), Box::new(|| crate::names::run(root))),
        ("parity".into(), Box::new(|| crate::parity::run(root, false))),
        ("docs".into(), Box::new(|| crate::docs::run(root))),
        (
            "tasks sync --check".into(),
            Box::new(|| {
                tasks::load_roadmap(root)
                    .map(|r| println!("tasks: roadmap.md parses: {} milestones, {} task blocks", r.milestones.len(), r.tasks.len()))
            }),
        ),
    ];
    for &m in &accepted {
        steps.push((format!("accept M{m}"), Box::new(move || crate::accept::accept(root, m, false, true, true))));
    }

    let mut results = Vec::new();
    for (name, step) in &steps {
        println!("\n==> ci: {name}");
        let started = Instant::now();
        let result = step();
        if let Err(e) = &result {
            eprintln!("ci: {name}: {e}");
        }
        results.push((name.clone(), result.is_ok(), started.elapsed().as_secs_f64()));
    }

    println!("\nci summary (test threads / build jobs: {n})");
    for (name, ok, secs) in &results {
        println!("  {} {name:<20} {secs:>6.1} s", if *ok { "ok  " } else { "FAIL" });
    }
    let failed: Vec<&str> = results.iter().filter(|(_, ok, _)| !ok).map(|(n, _, _)| n.as_str()).collect();
    if !failed.is_empty() {
        return Err(format!("ci: failed: {}", failed.join(", ")));
    }
    match record_green(root) {
        Ok(Some(commit)) => println!("ci: green on {commit} (recorded for `tasks submit`)"),
        Ok(None) => println!("ci: green (worktree has uncommitted changes, so no commit is recorded for `tasks submit`)"),
        Err(e) => eprintln!("ci: green, but recording it failed: {e}"),
    }
    Ok(())
}

/// Writes `<git common dir>/dalica-ci/<commit>.ok` when the worktree is clean.
fn record_green(root: &Path) -> Result<Option<String>, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()) } else { Err(format!("git {} failed", args.join(" "))) }
    };
    if !git(&["status", "--porcelain"])?.is_empty() {
        return Ok(None);
    }
    let commit = git(&["rev-parse", "HEAD"])?;
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let marker = tasks::board::ci_marker(Path::new(&common), &commit);
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&marker, b"").map_err(|e| e.to_string())?;
    Ok(Some(commit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallelism_follows_ram_and_jobs() {
        assert_eq!(parallelism(Some(19), 24, None), 19);
        assert_eq!(parallelism(Some(19), 24, Some(3)), 3);
        assert_eq!(parallelism(Some(0), 24, None), 1);
        assert_eq!(parallelism(None, 8, None), 8);
        assert_eq!(available_gib("MemTotal: 32000000 kB\nMemAvailable:   20971520 kB\n"), Some(20));
        assert_eq!(available_gib("nothing"), None);
    }

    #[test]
    fn acceptance_rest_leaves_out_the_accepted_milestones() {
        let manifest = "[package]\nname = \"dalica-acceptance\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[[test]]\nname = \"m0\"\npath = \"m0.rs\"\n\n\
                        [[test]]\nname = \"m0_skeleton\"\n\n[[test]]\nname = \"m1\"\n\n[[test]]\nname = \"m1_headless\"\n\n[[test]]\nname=\"m12\"\n";
        assert_eq!(acceptance_rest(manifest, &[0, 1]), ["m0_skeleton", "m12"]);
        assert_eq!(acceptance_rest(manifest, &[0]), ["m0_skeleton", "m1", "m1_headless", "m12"]);
        assert_eq!(acceptance_rest(manifest, &[]), ["m0", "m0_skeleton", "m1", "m1_headless", "m12"]);
        assert_eq!(acceptance_rest(manifest, &[0, 1, 12]), ["m0_skeleton"]);
        // The real manifest: every finished milestone up to M4 has its test there.
        let real = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/acceptance/Cargo.toml")).unwrap();
        let rest = acceptance_rest(&real, &[0, 1, 2, 3, 4]);
        assert!(rest.iter().all(|t| !["m0", "m1", "m2", "m3", "m4"].contains(&t.as_str())), "{rest:?}");
        assert!(rest.contains(&"m0_skeleton".to_string()) && !rest.contains(&"m1_headless".to_string()), "{rest:?}");
    }

    #[test]
    fn finished_milestones_need_every_block_ticked() {
        let text = "| P | M0 | A | a | ⬜ |\n| | M1 | B | b | ⬜ |\n\n- ✅ **M0.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n\
                    - ✅ **M0.R Y**\n  - after: M0.1\n  - owner: maintainer\n  - files: —\n  - scope: s\n  - done when: d\n- ⬜ **M1.1 Z**\n  - after: M0.R\n  - files: —\n  - scope: s\n  - done when: d\n";
        let r = tasks::roadmap::parse(text).unwrap();
        assert_eq!(finished_milestones(&r), [0]);
    }
}
