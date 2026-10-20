//! `cargo xtask ci`: the gates (`docs/development.md` §3). Runs every step even after a failure, so
//! one run reports everything, then prints a table and exits non-zero if any step failed. A green
//! run on a clean worktree records the commit for `cargo xtask tasks submit`.
//!
//! Three strengths, so nothing runs twice and the full list runs once, at the milestone review:
//!
//! - **`cargo xtask ci`** (a task's gate, run by its worker): fmt, clippy over the whole workspace
//!   (every crate and every test still compiles), the static checks, and the tests of the
//!   packages the branch changed and of the workspace packages that depend on one of them
//!   **directly** (a normal, dev or build dependency), in quick mode; transitive dependents are not
//!   added, and a break there surfaces at the review. `dalica-acceptance` and `xtask` are never added
//!   as dependents (they run when their own files change). A change to the root manifest, the lock
//!   file or the toolchain tests every package; a change only to docs tests none.
//! - **`cargo xtask ci --merge`** (`tasks merge`, after the rebase): fmt, clippy and the static
//!   checks, no tests: the worker's gate already ran them on the same changes, and tasks never
//!   share files.
//! - **`cargo xtask ci --full`** (the review gate, `M<n>.R`): every test of every package at full
//!   strength (`DALICA_FULL_TESTS=1`), the heif decoder's tests and the engine's HEIC Save, the
//!   parity check against the registered commands (and the `KEY-*` rows against the keymap), and
//!   the acceptance test of every finished milestone, and the RustSec advisory check
//!   (`crate::advisories`, which fetches the advisory database unless `DALICA_ADVISORY_DB` names a
//!   checkout). With `--platform` (the Windows and macOS runs of the review) it
//!   leaves out what is the same on every system: the format and static checks, the heif tests and
//!   the packages in [`PURE`].
//!
//! `--full` also takes `--tests-only` or `--accept-only`, which split it into two parts that run
//! side by side as separate GitHub jobs (each within its time limit; Windows took 65 minutes for
//! the whole at M13.R): the acceptance part is the `test acceptance` step and every `accept M<k>`;
//! the tests part is everything else. Together they run exactly what `--full` runs.
//!
//! `stats --check` (the workflow floor, `crate::stats`) runs in the review gate and in a task gate
//! that tests `dalica-mcp`, as a changed package or as a dependent.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Instant;

use crate::tasks;

/// Set to `1` on every test the review gate runs: tests at full strength (every fault point, full
/// proptest case counts, full-size scale tests). Unset, tests run quick.
pub const FULL_TESTS: &str = "DALICA_FULL_TESTS";

/// Cores the gates leave free for the rest of the desktop; `DALICA_RESERVE_CORES` overrides it.
pub const RESERVE_CORES: usize = 4;

/// Test threads and build jobs: one per GiB of available memory, at most the CPUs less the
/// reserved cores, at most `CARGO_BUILD_JOBS`, at least one. Several agents share one machine
/// (`task-board.md` §7).
pub fn parallelism(available_gib: Option<u64>, cpus: usize, reserve: usize, build_jobs: Option<usize>) -> usize {
    let free = cpus.saturating_sub(reserve).max(1);
    let cap = build_jobs.map_or(free, |j| j.min(free)).max(1);
    let by_ram = available_gib.map_or(cap, |g| usize::try_from(g).unwrap_or(usize::MAX));
    cap.min(by_ram).max(1)
}

/// Lowers this process's scheduling priority (nice 10), so the gate and every cargo, rustc and test
/// process it starts yield to the desktop. Best effort: where `renice` is missing (Windows) the gate
/// runs at normal priority.
fn lower_priority() {
    if cfg!(unix) {
        let pid = std::process::id().to_string();
        let _ =
            Command::new("renice").args(["-n", "10", "-p", &pid]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// `MemAvailable` from `/proc/meminfo`, in GiB (Linux; `None` elsewhere).
pub fn available_gib(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / (1024 * 1024))
}

/// The gate's share of the CPUs, set once by [`run`].
static RAYON_THREADS: OnceLock<String> = OnceLock::new();

/// Holds the rayon pool of the code under test to the gate's share: rayon otherwise starts one
/// thread per CPU in every test process, whatever `--test-threads` says. Benches are left alone;
/// their numbers are measured on the whole machine.
pub fn limit_threads(cmd: &mut Command) {
    if let Some(n) = RAYON_THREADS.get() {
        cmd.env("RAYON_NUM_THREADS", n);
    }
}

fn cargo_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(args);
    limit_threads(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

/// The acceptance tests' package. The workspace test step leaves it out: each milestone's test runs
/// once, in its `accept` step, and the rest of the package in the `test acceptance` step.
pub const ACCEPTANCE: &str = "dalica-acceptance";

/// Packages whose tests give the same answer on every system: pure computation over bytes and
/// numbers, with every file and OS call going through crates outside this list. The review's
/// Windows and macOS runs leave them out (`--platform`); Linux runs them.
pub const PURE: &[&str] =
    &["dalica-core", "dalica-library", "dalica-scenes", "dalica-edit", "dalica-image", "dalica-meta", "dalica-sidecar", "xtask"];

/// Which gate `ci` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// A task's gate: the changed packages' tests, quick.
    Task,
    /// `tasks merge` after its rebase: compile and static checks, no tests.
    Merge,
    /// The review gate: everything, full strength; `platform` leaves out what no system changes;
    /// `part` is all of it or one of its two halves.
    Full { platform: bool, part: Part },
}

/// Which part of the review gate runs (`--tests-only`, `--accept-only`; both, by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    All,
    /// Everything but the acceptance steps.
    Tests,
    /// The acceptance steps: `test acceptance` and each finished milestone's `accept M<k>`.
    Accept,
}

impl Part {
    fn tests(self) -> bool {
        self != Part::Accept
    }

    fn accept(self) -> bool {
        self != Part::Tests
    }
}

impl Gate {
    pub fn parse(args: &[String]) -> Result<Gate, String> {
        const USAGE: &str = "usage: cargo xtask ci [--merge | --full [--platform] [--tests-only | --accept-only]]";
        let has = |f: &str| args.iter().any(|a| a == f);
        if let Some(a) = args.iter().find(|a| !["--full", "--platform", "--merge", "--tests-only", "--accept-only"].contains(&a.as_str())) {
            return Err(format!("ci: unknown argument `{a}` ({USAGE})"));
        }
        let part = match (has("--tests-only"), has("--accept-only")) {
            (false, false) => Part::All,
            (true, false) => Part::Tests,
            (false, true) => Part::Accept,
            (true, true) => return Err(format!("ci: --tests-only and --accept-only are the two halves; give one or neither ({USAGE})")),
        };
        match (has("--full"), has("--platform"), has("--merge")) {
            (true, platform, false) => Ok(Gate::Full { platform, part }),
            (true, _, true) => Err("ci: --merge and --full are separate gates".into()),
            _ if part != Part::All => Err("ci: --tests-only and --accept-only go with --full".into()),
            (false, false, false) => Ok(Gate::Task),
            (false, false, true) => Ok(Gate::Merge),
            (false, true, _) => Err("ci: --platform goes with --full".into()),
        }
    }
}

/// What a task's gate tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every package (the build itself changed, or the changes could not be read).
    All(String),
    /// The packages the branch changed (possibly none: only docs changed) and the members that
    /// depend on one of them directly, not already changed themselves.
    Some { changed: BTreeSet<String>, dependents: BTreeSet<String> },
}

/// A workspace member: its directory (relative, `/`-separated), its package name and the names of
/// the packages it depends on (normal, dev and build dependencies, target-specific ones included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub dir: String,
    pub name: String,
    pub deps: BTreeSet<String>,
}

/// Members never added as a dependent of a changed package: the acceptance package dev-depends on
/// most crates, and its tests run at the review and when its own files change; `xtask` tests the
/// gates, not the crates. Both still run as changed packages.
pub const NEVER_DEPENDENT: &[&str] = &[ACCEPTANCE, "xtask"];

/// Files outside every package that change how everything builds or runs: any change to one tests
/// every package.
const EVERYTHING: &[&str] = &["Cargo.toml", "Cargo.lock", "clippy.toml", "rust-toolchain", "rust-toolchain.toml", "deny.toml"];

/// Paths outside every package that no test reads as its input: docs, workflows, notes.
fn is_prose(path: &str) -> bool {
    path.ends_with(".md") || [".github/", ".claude/", "docs/", "notes/", "log/", "website/"].iter().any(|p| path.starts_with(p))
}

/// The packages a branch's `changed` files belong to, and the members that depend on one of them
/// directly. A file belongs to the member whose directory is its longest prefix. Dependents of
/// dependents are not added, nor are the [`NEVER_DEPENDENT`] members.
pub fn select(changed: &[String], members: &[Member]) -> Selection {
    let mut picked = BTreeSet::new();
    for path in changed {
        let path = path.replace('\\', "/");
        if EVERYTHING.contains(&path.as_str()) || path.starts_with(".cargo/") {
            return Selection::All(format!("{path} changed"));
        }
        let owner = members.iter().filter(|m| path.starts_with(&format!("{}/", m.dir))).max_by_key(|m| m.dir.len());
        match owner {
            Some(m) if !path.ends_with(".md") => {
                picked.insert(m.name.clone());
            }
            Some(_) => {}
            None if is_prose(&path) => {}
            None => return Selection::All(format!("{path} is outside every package")),
        }
    }
    let dependents = members
        .iter()
        .filter(|m| !picked.contains(&m.name) && !NEVER_DEPENDENT.contains(&m.name.as_str()))
        .filter(|m| m.deps.iter().any(|d| picked.contains(d)))
        .map(|m| m.name.clone())
        .collect();
    Selection::Some { changed: picked, dependents }
}

/// The `name` of a manifest's `[package]` table.
pub fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        let Some(value) = line.strip_prefix("name").map(str::trim_start).and_then(|l| l.strip_prefix('=')) else { continue };
        if in_package {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The package names a manifest depends on: the keys of every `[dependencies]`,
/// `[dev-dependencies]` and `[build-dependencies]` table, target-specific ones
/// (`[target.'cfg(unix)'.dependencies]`) and the `[dependencies.<name>]` form included; a renamed
/// dependency (`package = "…"`) counts under its package name.
pub fn dependency_names(manifest: &str) -> BTreeSet<String> {
    const TABLES: &[&str] = &["dependencies", "dev-dependencies", "dev_dependencies", "build-dependencies", "build_dependencies"];
    let unquote = |s: &str| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    // The quoted `package = "…"` value of an inline table or a dependency table's line.
    let package_of = |text: &str| -> Option<String> {
        text.match_indices("package").find_map(|(at, word)| {
            let rest = text.get(at + word.len()..)?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
            Some(rest.get(..rest.find('"')?)?.to_string())
        })
    };
    let mut names = BTreeSet::new();
    // In a dependency list (`[dependencies]`), or in one dependency's own table
    // (`[dependencies.x]`, holding its key name until a `package` line renames it).
    let mut in_list = false;
    let mut table_dep: Option<String> = None;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            if let Some(dep) = table_dep.take() {
                names.insert(dep);
            }
            let header = line.trim_start_matches('[').trim_end_matches(']').trim();
            // Leave out `target.'cfg(…)'.` or `target.<triple>.` so the rest is the table's own path.
            let path = match header.strip_prefix("target.") {
                Some(rest) => match rest.rfind(['\'', '"']) {
                    Some(i) => rest.get(i + 1..).unwrap_or_default().trim_start_matches('.'),
                    None => rest.split_once('.').map_or("", |(_, r)| r),
                },
                None => header,
            };
            let (table, dep) = path.split_once('.').map_or((path, None), |(t, d)| (t, Some(d)));
            in_list = TABLES.contains(&table) && dep.is_none();
            table_dep = dep.filter(|_| TABLES.contains(&table)).map(unquote);
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(dep) = table_dep.as_mut() {
            if let Some(p) = package_of(line).filter(|_| line.starts_with("package")) {
                *dep = p;
            }
            continue;
        }
        if in_list {
            let Some((key, value)) = line.split_once('=') else { continue };
            // `name.workspace = true` and `name = { … }` both start with the dependency's key.
            let key = key.split('.').next().map(unquote).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            names.insert(package_of(value).unwrap_or(key));
        }
    }
    if let Some(dep) = table_dep {
        names.insert(dep);
    }
    names
}

/// The workspace members (`crates/*`, `apps/*`, `xtask`, `tests/acceptance`): directory, name and
/// dependencies.
fn members(root: &Path) -> Vec<Member> {
    let mut dirs = Vec::new();
    for group in ["crates", "apps"] {
        if let Ok(entries) = std::fs::read_dir(root.join(group)) {
            for e in entries.flatten() {
                dirs.push(format!("{group}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    dirs.extend(["xtask".to_string(), "tests/acceptance".to_string()]);
    dirs.into_iter()
        .filter_map(|dir| {
            let manifest = std::fs::read_to_string(root.join(&dir).join("Cargo.toml")).ok()?;
            let name = package_name(&manifest)?;
            Some(Member { dir, name, deps: dependency_names(&manifest) })
        })
        .collect()
}

/// The files this branch changed against `origin/main`: committed, staged, unstaged and untracked.
fn changed_files(root: &Path) -> Result<Vec<String>, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
        }
    };
    let base = git(&["merge-base", "HEAD", "origin/main"])?;
    let mut files: BTreeSet<String> = git(&["diff", "--name-only", base.trim()])?.lines().map(str::to_string).collect();
    files.extend(git(&["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_string));
    Ok(files.into_iter().collect())
}

/// The `[[test]]` targets of the acceptance manifest that no `accept` step runs, in its order: all
/// but `m<k>` and `m<k>_headless` for each milestone in `accepted` (`m<k>` runs the headless half's
/// steps first, so `m<k>_headless` would repeat them; it stays for `accept M<k> --headless`). The
/// skeleton checks and the acceptance test of a milestone still in progress run in this step.
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

/// Milestones whose every task block is ✅ (their acceptance checks run in the review gate).
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

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let gate = Gate::parse(args)?;
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let cpus = std::thread::available_parallelism().map(usize::from).unwrap_or(2);
    let build_jobs = std::env::var("CARGO_BUILD_JOBS").ok().and_then(|j| j.parse().ok());
    let reserve = std::env::var("DALICA_RESERVE_CORES").ok().and_then(|r| r.parse().ok()).unwrap_or(RESERVE_CORES);
    let n = parallelism(available_gib(&meminfo), cpus, reserve, build_jobs).to_string();
    let threads = format!("--test-threads={n}");
    lower_priority();
    let _ = RAYON_THREADS.set(n.clone());

    let roadmap = tasks::load_roadmap(root);
    let accepted = roadmap.as_ref().map(finished_milestones).unwrap_or_default();
    let full = matches!(gate, Gate::Full { .. });
    let platform = matches!(gate, Gate::Full { platform: true, .. });
    let part = match gate {
        Gate::Full { part, .. } => part,
        Gate::Task | Gate::Merge => Part::All,
    };
    // Full strength in the review gate; quick otherwise (the default when the variable is unset).
    let strength: &[(&str, &str)] = if full { &[(FULL_TESTS, "1")] } else { &[] };
    // The crate suites the acceptance tests would re-run have run in the test step by the time
    // they start, so they skip them (`accept::SUITES_RAN`).
    let suites_ran: Vec<(&str, &str)> = [(crate::accept::SUITES_RAN, "1")].into_iter().chain(strength.iter().copied()).collect();

    // What the test step covers.
    let selection = match gate {
        Gate::Task => match changed_files(root) {
            Ok(files) => select(&files, &members(root)),
            Err(e) => Selection::All(format!("the changes could not be read ({e})")),
        },
        Gate::Merge => Selection::Some { changed: BTreeSet::new(), dependents: BTreeSet::new() },
        Gate::Full { .. } => Selection::All("the review gate".into()),
    };
    let (test_all, picked) = match &selection {
        Selection::All(why) => {
            println!("ci: testing every package: {why}");
            (true, BTreeSet::new())
        }
        Selection::Some { changed, .. } if changed.is_empty() => {
            println!("ci: no package to test ({})", if gate == Gate::Merge { "the merge gate" } else { "only docs changed" });
            (false, BTreeSet::new())
        }
        Selection::Some { changed, dependents } => {
            let list = |s: &BTreeSet<String>| if s.is_empty() { "none".to_string() } else { s.iter().cloned().collect::<Vec<_>>().join(", ") };
            println!("ci: testing the changed packages: {}", list(changed));
            println!("ci: and their direct dependents: {}", list(dependents));
            (false, changed.union(dependents).cloned().collect())
        }
    };
    let test_acceptance = (test_all || picked.contains(ACCEPTANCE)) && part.accept();
    let crate_tests: Option<Vec<String>> = if !part.tests() {
        None
    } else if test_all {
        let mut args: Vec<String> = ["test", "--no-fail-fast", "--workspace", "--exclude", ACCEPTANCE].iter().map(|s| s.to_string()).collect();
        if platform {
            for p in PURE {
                args.extend(["--exclude".to_string(), (*p).to_string()]);
            }
        }
        Some(args)
    } else {
        let packages: Vec<&String> = picked.iter().filter(|p| p.as_str() != ACCEPTANCE).collect();
        (!packages.is_empty()).then(|| {
            let mut args = vec!["test".to_string(), "--no-fail-fast".to_string()];
            for p in packages {
                args.extend(["-p".to_string(), p.clone()]);
            }
            args
        })
    };

    type Step<'a> = (String, Box<dyn Fn() -> Result<(), String> + 'a>);
    let mut steps: Vec<Step> = Vec::new();
    if !platform && part.tests() {
        steps.push(("fmt".into(), Box::new(|| cargo_env(root, &["fmt", "--all", "--", "--check"], &[]))));
    }
    // Clippy covers every package and every target, so a change that breaks another crate's code or
    // tests is caught by every gate, also where that crate's tests do not run.
    if part.tests() {
        steps.push((
            "clippy".into(),
            Box::new(|| cargo_env(root, &["clippy", "--workspace", "--all-targets", "-j", &n, "--", "-D", "warnings"], &[])),
        ));
    }
    if let Some(args) = &crate_tests {
        let n = &n;
        let threads = &threads;
        steps.push((
            "test".into(),
            Box::new(move || {
                let mut all: Vec<&str> = args.iter().map(String::as_str).collect();
                all.extend(["-j", n, "--", threads]);
                cargo_env(root, &all, strength)
            }),
        ));
    }
    // The workflow pass rate holds its floor wherever the workflows' package is tested.
    if (full || test_all || picked.contains(crate::stats::PACKAGE)) && part.tests() {
        steps.push(("stats --check".into(), Box::new(move || crate::stats::check(root, strength))));
    }
    if full && !platform && part.tests() {
        // The optional HEIF decoder (`docs/formats.md` §3) is off by default: its own tests, once,
        // and the engine's Save of a HEIC photo, which only a build with the decoder can make
        // (`crates/engine/tests/save_formats.rs`). Both run even when the first fails.
        steps.push((
            "test heif".into(),
            Box::new(|| {
                let image = cargo_env(
                    root,
                    &["test", "--no-fail-fast", "-p", "dalica-image", "--features", "heif", "--test", "heif", "-j", &n, "--", &threads],
                    strength,
                );
                let save = cargo_env(
                    root,
                    &[
                        "test",
                        "--no-fail-fast",
                        "-p",
                        "dalica-engine",
                        "--features",
                        "heif",
                        "--test",
                        "all",
                        "-j",
                        &n,
                        "--",
                        "save_formats::",
                        &threads,
                    ],
                    strength,
                );
                image.and(save)
            }),
        ));
    }
    if test_acceptance {
        let accepted = &accepted;
        let suites_ran = &suites_ran;
        let n = &n;
        let threads = &threads;
        steps.push((
            "test acceptance".into(),
            Box::new(move || {
                let manifest = root.join("tests").join("acceptance").join("Cargo.toml");
                let manifest = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
                let rest = acceptance_rest(&manifest, accepted);
                println!("ci: {ACCEPTANCE}: the library and {} (each finished milestone's test runs in the review gate)", rest.join(", "));
                let mut args = vec!["test", "--no-fail-fast", "-p", ACCEPTANCE, "-j", n, "--lib"];
                for test in &rest {
                    args.extend(["--test", test.as_str()]);
                }
                args.extend(["--", threads]);
                cargo_env(root, &args, suites_ran)
            }),
        ));
    }
    if !platform && part.tests() {
        steps.push(("layers".into(), Box::new(|| crate::layers::run(root))));
        steps.push(("deps".into(), Box::new(|| crate::deps::run(root))));
        // The RustSec advisories against the shipped crates (`security.md` §8.4): once per review,
        // in the Linux run only; it fetches the advisory database unless `DALICA_ADVISORY_DB`
        // names a checkout.
        if full {
            steps.push(("advisories".into(), Box::new(|| crate::advisories::gate(root))));
        }
        steps.push(("assets".into(), Box::new(|| crate::assets::run(root))));
        steps.push(("names".into(), Box::new(|| crate::names::run(root))));
        steps.push(("test-layout".into(), Box::new(|| crate::test_layout::run(root))));
        // Cited `cmd:` ids are checked against the registry (a second build of the CLI) only in
        // the review gate; the rows and their cited paths in every gate.
        steps.push(("parity".into(), Box::new(move || crate::parity::run(root, false, full))));
        steps.push(("docs".into(), Box::new(|| crate::docs::run(root))));
        // The catalogs against the key set (`localization.md`): a fault, or a key a
        // `complete_menus` language lacks, fails; coverage and drafts are printed.
        steps.push(("i18n".into(), Box::new(|| crate::i18n::run(root, &[]))));
        steps.push(("contributors --check".into(), Box::new(|| crate::contributors::check(root))));
        steps.push((
            "tasks sync --check".into(),
            Box::new(|| {
                tasks::load_roadmap(root)
                    .map(|r| println!("tasks: roadmap.md parses: {} milestones, {} task blocks", r.milestones.len(), r.tasks.len()))
            }),
        ));
    }
    if full && part.accept() {
        for &m in &accepted {
            steps.push((format!("accept M{m}"), Box::new(move || crate::accept::accept_with(root, m, false, true, true, true))));
        }
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

    let label = match gate {
        Gate::Task => "task gate",
        Gate::Merge => "merge gate",
        Gate::Full { platform: false, part: Part::All } => "review gate",
        Gate::Full { platform: true, part: Part::All } => "review gate, platform part",
        Gate::Full { platform: false, part: Part::Tests } => "review gate, tests",
        Gate::Full { platform: true, part: Part::Tests } => "review gate, platform part, tests",
        Gate::Full { platform: false, part: Part::Accept } => "review gate, acceptance",
        Gate::Full { platform: true, part: Part::Accept } => "review gate, platform part, acceptance",
    };
    println!("\nci summary: {label} (test threads / build jobs: {n})");
    for (name, ok, secs) in &results {
        println!("  {} {name:<20} {:>6.1} min", if *ok { "ok  " } else { "FAIL" }, secs / 60.0);
    }
    let total: f64 = results.iter().map(|(_, _, s)| s).sum();
    println!("  total                     {:>6.1} min", total / 60.0);
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
    fn parallelism_follows_ram_jobs_and_reserve() {
        assert_eq!(parallelism(Some(19), 24, 4, None), 19);
        assert_eq!(parallelism(Some(32), 24, 4, None), 20);
        assert_eq!(parallelism(Some(19), 24, 4, Some(3)), 3);
        assert_eq!(parallelism(Some(32), 24, 4, Some(24)), 20);
        assert_eq!(parallelism(Some(0), 24, 4, None), 1);
        assert_eq!(parallelism(None, 8, 4, None), 4);
        assert_eq!(parallelism(None, 2, 4, None), 1);
        assert_eq!(parallelism(None, 8, 0, None), 8);
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

    #[test]
    fn the_gate_comes_from_the_flags() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(Gate::parse(&a(&[])), Ok(Gate::Task));
        assert_eq!(Gate::parse(&a(&["--merge"])), Ok(Gate::Merge));
        assert_eq!(Gate::parse(&a(&["--full"])), Ok(Gate::Full { platform: false, part: Part::All }));
        assert_eq!(Gate::parse(&a(&["--full", "--platform"])), Ok(Gate::Full { platform: true, part: Part::All }));
        assert_eq!(Gate::parse(&a(&["--full", "--tests-only"])), Ok(Gate::Full { platform: false, part: Part::Tests }));
        assert_eq!(Gate::parse(&a(&["--full", "--platform", "--accept-only"])), Ok(Gate::Full { platform: true, part: Part::Accept }));
        assert!(Gate::parse(&a(&["--full", "--tests-only", "--accept-only"])).is_err(), "the halves are exclusive");
        assert!(Gate::parse(&a(&["--accept-only"])).is_err(), "a half goes with --full");
        assert!(Gate::parse(&a(&["--merge", "--tests-only"])).is_err());
        assert!(Gate::parse(&a(&["--platform"])).is_err());
        assert!(Gate::parse(&a(&["--full", "--merge"])).is_err());
        assert!(Gate::parse(&a(&["--fast"])).is_err());
    }

    /// Synthetic members: directory, name and dependencies.
    fn graph(members: &[(&str, &str, &[&str])]) -> Vec<Member> {
        members
            .iter()
            .map(|(dir, name, deps)| Member { dir: dir.to_string(), name: name.to_string(), deps: deps.iter().map(|d| d.to_string()).collect() })
            .collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_task_tests_the_packages_it_changed() {
        let members = graph(&[
            ("crates/ui", "dalica-ui", &[]),
            ("crates/ui-kit", "dalica-ui-kit", &[]),
            ("crates/image", "dalica-image", &[]),
            ("tests/acceptance", ACCEPTANCE, &[]),
            ("xtask", "xtask", &[]),
        ]);
        let pick = |files: &[&str]| select(&files.iter().map(|f| f.to_string()).collect::<Vec<_>>(), &members);
        let some = |names: &[&str]| Selection::Some { changed: set(names), dependents: BTreeSet::new() };
        // The longest directory wins: ui-kit is not ui.
        assert_eq!(pick(&["crates/ui-kit/src/lib.rs", "crates/ui/tests/layout.rs"]), some(&["dalica-ui", "dalica-ui-kit"]));
        // Docs alone test nothing; a crate's README is docs too.
        assert_eq!(pick(&["docs/roadmap.md", "AGENTS.md", ".github/workflows/ci.yml", "crates/image/README.md"]), some(&[]));
        // The fuzz targets live under their crate.
        assert_eq!(pick(&["crates/image/fuzz/fuzz_targets/decode.rs"]), some(&["dalica-image"]));
        assert_eq!(pick(&["tests/acceptance/m9.rs", "xtask/src/ci.rs"]), some(&[ACCEPTANCE, "xtask"]));
        // The build itself, or a file no package owns, tests everything.
        assert!(matches!(pick(&["crates/ui/src/lib.rs", "Cargo.lock"]), Selection::All(_)));
        assert!(matches!(pick(&["Cargo.toml"]), Selection::All(_)));
        assert!(matches!(pick(&["assets/icons/star.svg"]), Selection::All(_)));
        // Windows separators read the same.
        assert_eq!(pick(&["crates\\ui\\src\\lib.rs"]), some(&["dalica-ui"]));
    }

    #[test]
    fn a_task_also_tests_the_direct_dependents_of_what_it_changed() {
        // core → library → engine → ui; the acceptance package and xtask depend on everything.
        let members = graph(&[
            ("crates/core", "dalica-core", &[]),
            ("crates/library", "dalica-library", &["dalica-core"]),
            ("crates/engine", "dalica-engine", &["dalica-core", "dalica-library", "dalica-engine"]),
            ("crates/ui", "dalica-ui", &["dalica-engine", "serde"]),
            ("tests/acceptance", ACCEPTANCE, &["dalica-core", "dalica-library", "dalica-engine", "dalica-ui"]),
            ("xtask", "xtask", &["dalica-library", "serde_json"]),
        ]);
        let pick = |files: &[&str]| select(&files.iter().map(|f| f.to_string()).collect::<Vec<_>>(), &members);
        let some = |changed: &[&str], dependents: &[&str]| Selection::Some { changed: set(changed), dependents: set(dependents) };
        // A library change: it and its direct dependent, not the dependent's dependents, never the
        // acceptance package or xtask.
        assert_eq!(pick(&["crates/library/src/lib.rs"]), some(&["dalica-library"], &["dalica-engine"]));
        assert_eq!(pick(&["crates/core/src/lib.rs"]), some(&["dalica-core"], &["dalica-engine", "dalica-library"]));
        // A dependent that changed itself is listed once, as changed; a package's dependency on
        // itself adds nothing.
        assert_eq!(pick(&["crates/library/src/lib.rs", "crates/engine/src/lib.rs"]), some(&["dalica-engine", "dalica-library"], &["dalica-ui"]));
        // Nothing depends on the top of the graph.
        assert_eq!(pick(&["crates/ui/src/lib.rs"]), some(&["dalica-ui"], &[]));
        // The acceptance package and xtask still run as changed packages.
        assert_eq!(pick(&["tests/acceptance/m13.rs", "xtask/src/ci.rs"]), some(&[ACCEPTANCE, "xtask"], &[]));
        // Docs alone still test nothing; the build files still test everything.
        assert_eq!(pick(&["docs/roadmap.md", "crates/library/README.md"]), some(&[], &[]));
        assert!(matches!(pick(&["crates/library/src/lib.rs", "Cargo.toml"]), Selection::All(_)));
        assert!(matches!(pick(&["crates/library/src/lib.rs", "Cargo.lock"]), Selection::All(_)));
    }

    #[test]
    fn dependencies_come_from_every_dependency_table() {
        let manifest = "[package]\nname = \"dalica-x\"\n\n[features]\nheif = [\"dalica-image/heif\"]\n\n[dependencies]\n\
                        # a comment = here\ndalica-core.workspace = true\nlog = { workspace = true }\n\
                        alias = { path = \"../package-dir\", package = \"dalica-fs\" }\nfeatures-list = { version = \"1\", features = [\n  \"a\",\n] }\n\n\
                        [dev-dependencies]\ndalica-scenes.workspace = true\n\n[build-dependencies]\n\"dalica-meta\" = \"1\"\n\n\
                        [target.'cfg(unix)'.dependencies]\nrustix.workspace = true\n\n[target.x86_64-pc-windows-msvc.dev-dependencies]\nwinapi = \"1\"\n\n\
                        [dependencies.renamed]\nversion = \"1\"\npackage = \"dalica-store\"\n\n[target.\"cfg(windows)\".dependencies.dalica-edit]\npath = \"../edit\"\n\n\
                        [[test]]\nname = \"all\"\npath = \"tests/all.rs\"\n\n[lints]\nworkspace = true\n";
        assert_eq!(
            dependency_names(manifest),
            set(&[
                "dalica-core",
                "dalica-edit",
                "dalica-fs",
                "dalica-meta",
                "dalica-scenes",
                "dalica-store",
                "features-list",
                "log",
                "rustix",
                "winapi"
            ])
        );
        assert!(dependency_names("[package]\nname = \"x\"\n").is_empty());
    }

    #[test]
    fn the_engine_s_direct_dependents_in_the_real_workspace() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let Selection::Some { changed, dependents } = select(&["crates/engine/src/lib.rs".to_string()], &members(&root)) else {
            panic!("a change in crates/engine tests some packages");
        };
        assert_eq!(changed, set(&["dalica-engine"]));
        for d in ["dalica-ui", "dalica-mcp", "dalica", "dalica-cli"] {
            assert!(dependents.contains(d), "{d} is not a dependent of dalica-engine: {dependents:?}");
        }
        assert!(!dependents.contains(ACCEPTANCE) && !dependents.contains("xtask"), "{dependents:?}");
        // Transitive only: nothing below the engine, and the UI kit does not depend on it.
        assert!(!dependents.contains("dalica-core") && !dependents.contains("dalica-ui-kit"), "{dependents:?}");
    }

    #[test]
    fn every_member_has_a_package_name() {
        assert_eq!(package_name("[package]\nname = \"dalica-ui\"\n[dependencies]\nname = \"x\"\n").as_deref(), Some("dalica-ui"));
        assert_eq!(package_name("[workspace]\nmembers = []\n"), None);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let m = members(&root);
        assert!(m.iter().any(|m| m.dir == "crates/ui" && m.name == "dalica-ui"), "{m:?}");
        assert!(m.iter().any(|m| m.dir == "tests/acceptance" && m.name == ACCEPTANCE), "{m:?}");
        // Every PURE package exists.
        for p in PURE {
            assert!(m.iter().any(|m| m.name == *p), "{p} is not a workspace member");
        }
    }
}
