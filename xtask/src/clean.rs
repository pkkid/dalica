//! `cargo xtask clean`: frees the disk space the build directories take (`docs/development.md` §9).
//!
//! Everything it removes is rebuildable cache: the cargo target dirs under the main checkout's
//! `target/` (one per agent, the lead's, the merge queue's, reviews', the main checkout's own
//! `target/debug` and `target/release`, old one-offs) and the stray `target/` dirs that runs with a
//! relative `CARGO_TARGET_DIR` leave in `apps/*`, `crates/*`, `tests/acceptance` and `xtask`.
//!
//! Safety (`AGENTS.md` §7: never `rm -rf` shared paths):
//! - only an immediate child of `<main>/target/` or one of the fixed stray paths is ever removed,
//!   and only when it holds a cargo profile (a directory with `.fingerprint/`);
//! - symlinks are never followed: a symlinked dir is refused, and the canonical path of the dir
//!   must be exactly the expected one under the canonical `<main>/target` (or `<main>` for strays);
//! - a dir in which cargo is building (its locks in a profile are held) is skipped, as is the dir
//!   the running xtask binary lives in, so `cargo xtask clean` can run from any checkout;
//! - removal is `std::fs::remove_dir_all` on the verified path while holding cargo's locks (on
//!   Windows, released just before, since a locked file cannot be removed there), never a shell.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::tasks::prune::{Held, human, is_real_dir, lock_profile};

const USAGE: &str = "usage: cargo xtask clean [--dry-run] [--keep <name>]… [--only <name>]… [--older-than <days>]

Removes the cargo build dirs of the main checkout (everything under target/, plus stray
apps/*/target, crates/*/target, tests/acceptance/target and xtask/target). All of it is cache.
A name is a dir under target/ (`agent-3`, `lead`, `review-*`) or a stray path (`apps/dalica/target`);
`*` matches any run of characters. Dirs cargo is building in, and the running xtask's own, are skipped.

  --dry-run           list and size only; remove nothing
  --keep <name>       never remove this dir (repeatable)
  --only <name>       remove only these dirs (repeatable)
  --older-than <days> only dirs whose newest file is older than this";

/// How deep below a build dir profiles are looked for (`agent-7-hook/target/debug` is depth 2; a
/// stray left by the M0 acceptance test run with a relative target dir,
/// `tests/acceptance/target/agent-1/accept-m0/tree/target/agent-1/accept-m0/target/debug`, depth 8).
const MAX_DEPTH: usize = 10;
/// Directories never descended into while looking for profiles.
const NO_DESCEND: [&str; 7] = ["incremental", "deps", "build", ".fingerprint", "examples", "src", ".git"];
/// Caps on the directories visited while looking for profiles, and the entries walked while sizing.
const MAX_DIRS: usize = 10_000;
const MAX_ENTRIES: usize = 20_000_000;
/// Parents whose children may hold a stray `target/` (`apps/dalica/target`).
const STRAY_PARENTS: [&str; 2] = ["apps", "crates"];
/// Fixed stray `target/` dirs.
const STRAY_FIXED: [&str; 2] = ["tests/acceptance/target", "xtask/target"];

#[derive(Debug, Default, Clone)]
pub struct Options {
    pub dry_run: bool,
    pub keep: Vec<String>,
    pub only: Vec<String>,
    pub older_than_days: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// Bytes removed (or, in a dry run, that would be).
    pub freed: u64,
    /// Dirs that could not be removed.
    pub errors: usize,
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let opts = parse(args)?;
    let main = main_checkout(root);
    let own = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
    println!("clean: build dirs of {}{}", main.display(), if opts.dry_run { " (dry run)" } else { "" });
    let report = clean(&main, &opts, own.as_deref(), SystemTime::now());
    for line in &report.lines {
        println!("clean: {line}");
    }
    println!("clean: {} {}", if opts.dry_run { "would free" } else { "freed" }, human(report.freed));
    if report.errors > 0 { Err(format!("clean: {} dir(s) could not be removed", report.errors)) } else { Ok(()) }
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("clean: {flag} needs a value\n\n{USAGE}"));
        match arg.as_str() {
            "--dry-run" | "-n" => opts.dry_run = true,
            "--keep" => opts.keep.push(value("--keep")?),
            "--only" => opts.only.push(value("--only")?),
            "--older-than" => {
                let v = value("--older-than")?;
                let days = v.parse::<u64>().map_err(|_| format!("clean: --older-than takes a whole number of days, not `{v}`"))?;
                opts.older_than_days = Some(days);
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("clean: unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    Ok(opts)
}

/// The main checkout (where `target/` lives): the parent of git's common dir, so a worktree finds
/// it too. Falls back to `root` outside git.
fn main_checkout(root: &Path) -> PathBuf {
    let out = Command::new("git").arg("-C").arg(root).args(["rev-parse", "--path-format=absolute", "--git-common-dir"]).output();
    match out {
        Ok(o) if o.status.success() => {
            let common = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
            common.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf)
        }
        _ => root.to_path_buf(),
    }
}

/// A dir that may be removed: `rel` is relative to the main checkout, with `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    /// `target/agent-3`, `apps/dalica/target`.
    rel: String,
    /// What `--keep`/`--only` match: `agent-3`, `apps/dalica/target`.
    name: String,
}

/// Every dir under `<main>/target/` and every stray `target/` that exists, sorted.
fn candidates(main: &Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    for name in child_names(&main.join("target")) {
        out.push(Candidate { rel: format!("target/{name}"), name });
    }
    for parent in STRAY_PARENTS {
        for child in child_names(&main.join(parent)) {
            let rel = format!("{parent}/{child}/target");
            if std::fs::symlink_metadata(main.join(&rel)).is_ok() {
                out.push(Candidate { name: rel.clone(), rel });
            }
        }
    }
    for rel in STRAY_FIXED {
        if std::fs::symlink_metadata(main.join(rel)).is_ok() {
            out.push(Candidate { rel: rel.to_string(), name: rel.to_string() });
        }
    }
    out
}

/// The names of the directories (and symlinks, so they can be reported) directly in `dir`, sorted.
fn child_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir() || m.file_type().is_symlink()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Whether `rel` is a path clean may remove: `target/<name>`, `apps|crates/<name>/target`, or a
/// fixed stray, each part a plain name.
fn allowed(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let plain = parts.iter().all(|p| {
        let mut c = Path::new(p).components();
        matches!((c.next(), c.next()), (Some(Component::Normal(_)), None)) && !p.contains('\\')
    });
    plain
        && match parts.as_slice() {
            ["target", _] => true,
            [parent, _, "target"] if STRAY_PARENTS.contains(parent) => true,
            _ => STRAY_FIXED.contains(&rel),
        }
}

/// Why a path is not removed by [`checked_path`].
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    /// It no longer exists (removed meanwhile): fine.
    Gone,
    /// Never removed, with why.
    Refused(String),
}

/// The canonical path of `<main>/<rel>`, verified: `rel` is [`allowed`], the dir is a real dir
/// (not a symlink), and its canonical path is exactly the expected one under the canonical
/// `<main>/target` (or `<main>` for strays), so no symlinked parent leads elsewhere.
fn checked_path(main: &Path, rel: &str) -> Result<PathBuf, Refusal> {
    if !allowed(rel) {
        return Err(Refusal::Refused(format!("{rel} is outside target/ and the stray list")));
    }
    let path = rel.split('/').fold(main.to_path_buf(), |p, part| p.join(part));
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Gone),
        Err(e) => return Err(Refusal::Refused(format!("cannot read: {e}"))),
    };
    if meta.file_type().is_symlink() {
        return Err(Refusal::Refused("a symlink (never followed)".into()));
    }
    if !meta.is_dir() {
        return Err(Refusal::Refused("not a directory".into()));
    }
    let canon = |p: &Path| p.canonicalize().map_err(|e| Refusal::Refused(format!("cannot resolve {}: {e}", p.display())));
    let actual = canon(&path)?;
    let expected = match rel.split_once('/') {
        Some(("target", name)) => canon(&main.join("target"))?.join(name),
        _ => rel.split('/').fold(canon(main)?, |p, part| p.join(part)),
    };
    if actual != expected {
        return Err(Refusal::Refused(format!("resolves to {}, outside {}", actual.display(), expected.display())));
    }
    Ok(actual)
}

/// `pattern` matches `name`; `*` matches any run of characters.
fn matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    let pattern = pattern.strip_prefix("target/").unwrap_or(pattern);
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else { return false };
    let Some(mut rest) = name.strip_prefix(first) else { return false };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else { return rest.is_empty() };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = rest.get(i + part.len()..).unwrap_or(""),
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The cargo profile dirs (holding `.fingerprint/`) at or below `dir`: bounded, symlinks not
/// followed, profiles not descended into.
fn find_profiles(dir: &Path, depth: usize, visited: &mut usize, found: &mut Vec<PathBuf>) {
    *visited += 1;
    if *visited > MAX_DIRS {
        return;
    }
    if is_real_dir(&dir.join(".fingerprint")) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<(String, PathBuf)> =
        entries.filter_map(Result::ok).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    children.sort();
    for (name, path) in children {
        if is_real_dir(&path) && !NO_DESCEND.contains(&name.as_str()) {
            find_profiles(&path, depth + 1, visited, found);
        }
    }
}

/// Total size of the files below `dir` and the newest modification time of anything in it
/// (symlinks counted as themselves, never followed; capped).
fn size_and_newest(dir: &Path) -> (u64, Option<SystemTime>) {
    let mut total: u64 = 0;
    let mut newest = std::fs::symlink_metadata(dir).and_then(|m| m.modified()).ok();
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.filter_map(Result::ok) {
            seen += 1;
            if seen > MAX_ENTRIES {
                return (total, newest);
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            if let Ok(m) = meta.modified() {
                newest = Some(newest.map_or(m, |n| n.max(m)));
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    (total, newest)
}

/// Cleans the build dirs of `main`. `own` is the canonical path of the running xtask binary (its
/// target dir is never removed); `now` is compared with `--older-than`.
pub fn clean(main: &Path, opts: &Options, own: Option<&Path>, now: SystemTime) -> Report {
    let mut report = Report::default();
    let all = candidates(main);
    if all.is_empty() {
        report.lines.push(format!("nothing to clean in {}", main.display()));
    }
    let width = all.iter().map(|c| c.rel.len()).max().unwrap_or(0);
    for cand in all {
        let rel = &cand.rel;
        let line = |size: Option<u64>, what: &str| {
            let size = size.map_or_else(String::new, human);
            format!("{rel:<width$} {size:>9}  {what}")
        };
        if !opts.only.is_empty() && !opts.only.iter().any(|p| matches(p, &cand.name)) {
            continue;
        }
        if opts.keep.iter().any(|p| matches(p, &cand.name)) {
            report.lines.push(line(None, "kept (--keep)"));
            continue;
        }
        let path = match checked_path(main, rel) {
            Ok(p) => p,
            Err(Refusal::Gone) => continue,
            Err(Refusal::Refused(why)) => {
                report.lines.push(line(None, &format!("skipped: {why}")));
                continue;
            }
        };
        let mut profiles = Vec::new();
        let mut visited = 0;
        find_profiles(&path, 0, &mut visited, &mut profiles);
        if visited > MAX_DIRS {
            // Some profile may not have been seen, so its lock could not be checked.
            report.lines.push(line(None, &format!("skipped: more than {MAX_DIRS} dirs to check for cargo's locks")));
            continue;
        }
        if profiles.is_empty() {
            report.lines.push(line(None, "skipped: not a cargo build dir"));
            continue;
        }
        let (bytes, newest) = size_and_newest(&path);
        if own.is_some_and(|exe| exe.starts_with(&path)) {
            report.lines.push(line(Some(bytes), "skipped: the running xtask lives here"));
            continue;
        }
        if let Some(days) = opts.older_than_days {
            let cutoff = now.checked_sub(Duration::from_secs(days.saturating_mul(86_400)));
            if let (Some(newest), Some(cutoff)) = (newest, cutoff)
                && newest >= cutoff
            {
                let age = now.duration_since(newest).map_or(0, |d| d.as_secs() / 86_400);
                report.lines.push(line(Some(bytes), &format!("skipped: used {age} day(s) ago (--older-than {days})")));
                continue;
            }
        }
        let mut held = Vec::new();
        let mut busy = None;
        for profile in &profiles {
            match lock_profile(profile, opts.dry_run) {
                Ok(h) => held.push(h),
                Err(why) => {
                    busy = Some(why);
                    break;
                }
            }
        }
        if let Some(why) = busy {
            drop(held);
            let what = if why.contains("building") { "in use, skipped (cargo is building there)".to_string() } else { format!("skipped: {why}") };
            report.lines.push(line(Some(bytes), &what));
            continue;
        }
        if opts.dry_run {
            report.lines.push(line(Some(bytes), "would remove"));
            report.freed = report.freed.saturating_add(bytes);
            continue;
        }
        match remove(&path, held) {
            Ok(()) => {
                report.lines.push(line(Some(bytes), "removed"));
                report.freed = report.freed.saturating_add(bytes);
            }
            Err(e) => {
                report.lines.push(line(Some(bytes), &format!("FAILED: {e}")));
                report.errors += 1;
            }
        }
    }
    report
}

/// Removes the verified dir `path` while holding cargo's locks of its profiles (`held`). A dir that
/// vanished meanwhile is fine.
fn remove(path: &Path, held: Vec<Held>) -> std::io::Result<()> {
    // Windows cannot remove a file someone holds a lock on: release just before.
    if cfg!(windows) {
        drop(held);
        return ok_if_gone(std::fs::remove_dir_all(path));
    }
    let result = ok_if_gone(std::fs::remove_dir_all(path));
    drop(held);
    result
}

fn ok_if_gone(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    /// A cargo target dir at `dir` with one `debug` profile holding `bytes` bytes.
    fn build_dir(dir: &Path, bytes: usize) {
        profile(&dir.join("debug"), bytes);
        std::fs::write(dir.join("CACHEDIR.TAG"), b"").unwrap();
    }

    /// A cargo profile dir holding `bytes` bytes of build output.
    fn profile(dir: &Path, bytes: usize) {
        std::fs::create_dir_all(dir.join(".fingerprint")).unwrap();
        std::fs::create_dir_all(dir.join("deps")).unwrap();
        std::fs::write(dir.join("deps").join("libx.rlib"), vec![1u8; bytes]).unwrap();
    }

    fn opts() -> Options {
        Options::default()
    }

    fn removed(report: &Report) -> Vec<String> {
        report
            .lines
            .iter()
            .filter(|l| l.ends_with("removed") || l.ends_with("would remove"))
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect()
    }

    /// A main checkout with build dirs of every kind, sizes in bytes.
    fn checkout() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let m = tmp.path();
        let t = m.join("target");
        build_dir(&t.join("agent-3"), 100);
        profile(&t.join("agent-3").join("accept-m0").join("target").join("debug"), 10);
        // The M0 acceptance test's copy of the tree, built with a relative target dir: a profile
        // eight levels down.
        let copy = m.join("tests").join("acceptance").join("target").join("agent-1").join("accept-m0").join("tree");
        std::fs::create_dir_all(copy.join("crates").join("core").join("src")).unwrap();
        profile(&copy.join("target").join("agent-1").join("accept-m0").join("target").join("debug"), 32_000);
        build_dir(&t.join("lead"), 200);
        build_dir(&t.join("website"), 400);
        build_dir(&t.join("review-m7"), 800);
        profile(&t.join("debug"), 1_000);
        profile(&t.join("release"), 2_000);
        // A cargo project with its own target dir (agent-7-hook): profile two levels down.
        std::fs::create_dir_all(t.join("agent-7-hook").join("src")).unwrap();
        profile(&t.join("agent-7-hook").join("target").join("debug"), 4_000);
        // Not a build dir: fingerprint test output.
        std::fs::create_dir_all(t.join("test-output").join("ui")).unwrap();
        std::fs::write(t.join("test-output").join("ui").join("diff.png"), b"png").unwrap();
        std::fs::write(t.join("CACHEDIR.TAG"), b"").unwrap();
        // Strays.
        build_dir(&m.join("apps").join("dalica").join("target"), 8_000);
        // Source dirs are never touched.
        std::fs::create_dir_all(m.join("apps").join("dalica-cli").join("src")).unwrap();
        std::fs::create_dir_all(m.join("crates").join("core").join("src")).unwrap();
        tmp
    }

    #[test]
    fn picks_every_build_dir_and_sizes_add_up() {
        let tmp = checkout();
        let m = tmp.path();
        let report = clean(m, &Options { dry_run: true, ..opts() }, None, SystemTime::now());
        assert_eq!(
            removed(&report),
            [
                "target/agent-3",
                "target/agent-7-hook",
                "target/debug",
                "target/lead",
                "target/release",
                "target/review-m7",
                "target/website",
                "apps/dalica/target",
                "tests/acceptance/target",
            ]
        );
        assert!(report.lines.iter().any(|l| l.starts_with("target/test-output") && l.ends_with("not a cargo build dir")), "{:?}", report.lines);
        assert_eq!(report.freed, 100 + 10 + 4_000 + 1_000 + 200 + 2_000 + 800 + 400 + 8_000 + 32_000);
        assert_eq!(report.errors, 0);

        // A dry run deletes nothing and creates no lock file.
        assert!(m.join("target").join("agent-3").join("debug").join("deps").join("libx.rlib").is_file());
        assert!(!m.join("target").join("agent-3").join("debug").join(".cargo-lock").exists());

        let real = clean(m, &opts(), None, SystemTime::now());
        assert_eq!(real.freed, report.freed);
        assert_eq!(real.errors, 0);
        assert_eq!(removed(&real), removed(&report));
        for gone in ["target/agent-3", "target/debug", "target/agent-7-hook", "apps/dalica/target", "tests/acceptance/target"] {
            assert!(!m.join(gone).exists(), "{gone}");
        }
        for kept in ["target/test-output/ui/diff.png", "target/CACHEDIR.TAG", "apps/dalica-cli/src", "crates/core/src"] {
            assert!(m.join(kept).exists(), "{kept}");
        }
        // Nothing left to free.
        assert_eq!(clean(m, &opts(), None, SystemTime::now()).freed, 0);
    }

    #[test]
    fn keep_and_only() {
        let tmp = checkout();
        let m = tmp.path();
        let o = Options { dry_run: true, keep: vec!["website".into(), "lead".into(), "review-*".into()], ..opts() };
        let report = clean(m, &o, None, SystemTime::now());
        let r = removed(&report);
        assert!(!r.iter().any(|d| d.contains("website") || d.contains("lead") || d.contains("review")), "{r:?}");
        assert!(r.contains(&"target/agent-3".to_string()));
        assert_eq!(report.lines.iter().filter(|l| l.ends_with("kept (--keep)")).count(), 3);

        let o =
            Options { keep: vec!["target/website".into()], only: vec!["agent-*".into(), "website".into(), "apps/dalica/target".into()], ..opts() };
        let report = clean(m, &o, None, SystemTime::now());
        assert_eq!(removed(&report), ["target/agent-3", "target/agent-7-hook", "apps/dalica/target"]);
        assert!(m.join("target").join("website").is_dir() && m.join("target").join("lead").is_dir());
        assert!(!m.join("target").join("agent-3").exists());
    }

    #[test]
    fn older_than_keeps_recently_used_dirs() {
        let tmp = checkout();
        let m = tmp.path();
        let now = SystemTime::now();
        let o = Options { dry_run: true, older_than_days: Some(3), ..opts() };
        // Everything was just written: nothing is older than 3 days.
        let report = clean(m, &o, None, now);
        assert!(removed(&report).is_empty(), "{:?}", report.lines);
        assert!(report.lines.iter().any(|l| l.contains("used 0 day(s) ago (--older-than 3)")));
        // Seen from 4 days on, all of it is.
        let later = now + Duration::from_secs(4 * 86_400);
        assert_eq!(removed(&clean(m, &o, None, later)).len(), 9);
    }

    #[test]
    fn a_dir_cargo_is_building_in_is_skipped() {
        let tmp = checkout();
        let m = tmp.path();
        let debug = m.join("target").join("agent-3").join("accept-m0").join("target").join("debug");
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(debug.join(".cargo-lock")).unwrap();
        lock.lock().unwrap();
        for dry_run in [true, false] {
            let report = clean(m, &Options { dry_run, only: vec!["agent-3".into()], ..opts() }, None, SystemTime::now());
            assert_eq!(report.lines.len(), 1);
            assert!(report.lines[0].ends_with("in use, skipped (cargo is building there)"), "{:?}", report.lines);
            assert_eq!(report.freed, 0);
            assert!(m.join("target").join("agent-3").join("debug").join("deps").is_dir(), "no part of it goes");
        }
        // See `prune.rs`: unlock before closing so a child spawned meanwhile keeps no copy.
        lock.unlock().unwrap();
        drop(lock);
        let report = clean(m, &Options { only: vec!["agent-3".into()], ..opts() }, None, SystemTime::now());
        assert_eq!(removed(&report), ["target/agent-3"]);
    }

    #[test]
    fn the_running_xtask_target_dir_is_kept() {
        let tmp = checkout();
        let m = tmp.path();
        let exe = m.join("target").join("lead").join("debug").join("xtask");
        std::fs::write(&exe, b"bin").unwrap();
        let own = exe.canonicalize().unwrap();
        let report = clean(m, &opts(), Some(&own), SystemTime::now());
        assert!(report.lines.iter().any(|l| l.starts_with("target/lead") && l.ends_with("the running xtask lives here")), "{:?}", report.lines);
        assert!(own.is_file());
        assert!(!removed(&report).contains(&"target/lead".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_refused_and_never_followed() {
        let tmp = checkout();
        let m = tmp.path();
        let elsewhere = tmp.path().join("elsewhere");
        build_dir(&elsewhere, 50);
        std::os::unix::fs::symlink(&elsewhere, m.join("target").join("agent-9")).unwrap();
        // A stray whose parent is a symlink leads outside the checkout.
        std::fs::create_dir_all(elsewhere.join("app").join("target")).unwrap();
        profile(&elsewhere.join("app").join("target").join("debug"), 5);
        std::os::unix::fs::symlink(elsewhere.join("app"), m.join("apps").join("linked")).unwrap();

        let report = clean(m, &opts(), None, SystemTime::now());
        assert!(report.lines.iter().any(|l| l.starts_with("target/agent-9") && l.ends_with("a symlink (never followed)")), "{:?}", report.lines);
        assert!(report.lines.iter().any(|l| l.starts_with("apps/linked/target") && l.contains("resolves to")), "{:?}", report.lines);
        assert!(elsewhere.join("debug").join("deps").join("libx.rlib").is_file());
        assert!(elsewhere.join("app").join("target").join("debug").join("deps").is_dir());
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn paths_outside_target_are_refused() {
        let tmp = checkout();
        let m = tmp.path();
        for rel in
            ["target", "target/..", "target/../apps", "apps/dalica", "crates/core/src", "target/a/b", "..", "/etc", "docs/target", "target/./x"]
        {
            assert!(matches!(checked_path(m, rel), Err(Refusal::Refused(_))), "{rel}");
        }
        assert_eq!(checked_path(m, "target/nope"), Err(Refusal::Gone));
        assert!(checked_path(m, "target/lead").is_ok());
        assert!(checked_path(m, "apps/dalica/target").is_ok());
        assert!(checked_path(m, "tests/acceptance/target").is_ok());
    }

    #[test]
    fn patterns_and_args() {
        assert!(matches("lead", "lead"));
        assert!(!matches("lead", "lead-merge"));
        assert!(matches("lead*", "lead-merge"));
        assert!(matches("target/lead", "lead"));
        assert!(matches("review-*", "review-m7"));
        assert!(matches("agent-*-fix*", "agent-5-fix-old"));
        assert!(!matches("agent-*-fix*", "agent-5"));
        assert!(matches("*", "anything"));
        assert!(matches("apps/*/target", "apps/dalica/target"));

        let args = |a: &[&str]| parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let o = args(&["--dry-run", "--keep", "website", "--keep", "lead", "--only", "agent-3", "--older-than", "7"]).unwrap();
        assert!(o.dry_run);
        assert_eq!(o.keep, ["website", "lead"]);
        assert_eq!(o.only, ["agent-3"]);
        assert_eq!(o.older_than_days, Some(7));
        assert!(args(&["--older-than", "x"]).is_err());
        assert!(args(&["--keep"]).is_err());
        assert!(args(&["--bogus"]).is_err());
    }
}
