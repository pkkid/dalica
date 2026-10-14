//! `tasks prune`: removes cargo's `incremental/` caches from idle agents' target directories
//! (`docs/task-board.md` §5, §7). Long-lived per-agent target dirs grow by several GB a day; the
//! incremental cache is most of that growth and cargo rebuilds it on demand.
//!
//! Safety: only `target/agent-<n>` and `target/merge` are looked at; an agent holding an In Progress
//! task is skipped; symlinks are never followed; only a directory named `incremental` next to a
//! `.fingerprint` directory (cargo's profile layout) is removed; and it is removed while holding
//! cargo's own build locks, so a cargo that is building there makes prune skip it, and a cargo that
//! starts meanwhile waits.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use super::agent_number;
use super::config::MERGE_TARGET;

/// The lock files cargo holds in a profile directory while it builds (cargo 1.98 holds all three).
const CARGO_LOCKS: [&str; 3] = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"];
/// How deep below an agent's target dir profiles are looked for:
/// `accept-m0/target/debug/incremental` is depth 4.
const MAX_DEPTH: usize = 4;
/// Directories never descended into while looking for profiles (big, and never contain one).
const NO_DESCEND: [&str; 5] = ["incremental", "deps", "build", ".fingerprint", "examples"];
/// Caps on the directories visited while looking, and the entries counted while sizing.
const MAX_DIRS: usize = 10_000;
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// Bytes removed (or, in a dry run, that would be).
    pub freed: u64,
    /// Removals that failed.
    pub errors: usize,
}

/// Prunes every idle target dir under `target_root`. `busy` maps agent names to the task they hold
/// In Progress.
pub fn prune(target_root: &Path, busy: &BTreeMap<String, String>, dry_run: bool) -> Report {
    let mut report = Report::default();
    let mut names: Vec<String> = match std::fs::read_dir(target_root) {
        Ok(entries) => entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(e) => {
            report.lines.push(format!("nothing to prune: cannot read {}: {e}", target_root.display()));
            return report;
        }
    };
    names.retain(|n| n == MERGE_TARGET || agent_number(n).is_ok());
    names.sort_by_key(|n| (agent_number(n).unwrap_or(u32::MAX), n.clone()));
    for name in names {
        let dir = target_root.join(&name);
        if !is_real_dir(&dir) {
            report.lines.push(format!("{name}: skipped, not a directory (symlinks are never followed)"));
            continue;
        }
        if let Some(task) = busy.get(&name) {
            report.lines.push(format!("{name}: skipped, {task} is In Progress"));
            continue;
        }
        let mut found = Vec::new();
        let mut visited = 0;
        find_incremental(&dir, 1, &mut visited, &mut found);
        if found.is_empty() {
            report.lines.push(format!("{name}: nothing to prune"));
        }
        for inc in found {
            let shown = shown_path(target_root, &inc);
            let Some(profile) = inc.parent() else { continue };
            let _locks = match lock_profile(profile, dry_run) {
                Ok(locks) => locks,
                Err(why) => {
                    report.lines.push(format!("{name}: skipped {shown}, {why}"));
                    continue;
                }
            };
            let bytes = dir_size(&inc);
            if dry_run {
                report.lines.push(format!("{name}: would remove {shown} ({})", human(bytes)));
                report.freed = report.freed.saturating_add(bytes);
            } else {
                match std::fs::remove_dir_all(&inc) {
                    Ok(()) => {
                        report.lines.push(format!("{name}: removed {shown} ({})", human(bytes)));
                        report.freed = report.freed.saturating_add(bytes);
                    }
                    Err(e) => {
                        report.lines.push(format!("{name}: cannot remove {shown}: {e}"));
                        report.errors += 1;
                    }
                }
            }
        }
    }
    report
}

/// A directory, and not a symlink to one.
pub(crate) fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Cargo's `incremental/` directories below `dir`: each one sits in a profile directory next to
/// `.fingerprint/`. Bounded in depth and in directories visited; symlinks are not followed.
fn find_incremental(dir: &Path, depth: usize, visited: &mut usize, found: &mut Vec<PathBuf>) {
    *visited += 1;
    if *visited > MAX_DIRS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<(String, PathBuf)> =
        entries.filter_map(Result::ok).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    children.sort();
    for (name, path) in children {
        if !is_real_dir(&path) {
            continue;
        }
        if name == "incremental" {
            if is_real_dir(&dir.join(".fingerprint")) {
                found.push(path);
            }
        } else if depth < MAX_DEPTH && !NO_DESCEND.contains(&name.as_str()) {
            find_incremental(&path, depth + 1, visited, found);
        }
    }
}

/// Cargo's locks of one profile, held by prune. Dropping it unlocks each file before closing it.
/// Closing alone is not enough: the lock belongs to the open file, and a child process spawned
/// meanwhile (by another thread) holds a copy of every open file until its exec closes them, so
/// the lock could outlive prune for a moment and make a cargo starting then wait.
pub(crate) struct Held(Vec<File>);

impl Drop for Held {
    fn drop(&mut self) {
        for file in &self.0 {
            let _ = file.unlock();
        }
    }
}

/// Takes cargo's locks of `profile` without waiting. Err says why not (cargo is building there),
/// and has released the locks already taken. A dry run opens only lock files that exist,
/// read-only, and creates nothing.
pub(crate) fn lock_profile(profile: &Path, dry_run: bool) -> Result<Held, String> {
    let mut held = Held(Vec::new());
    for name in CARGO_LOCKS {
        let path = profile.join(name);
        let file = if dry_run {
            if !path.is_file() {
                continue;
            }
            OpenOptions::new().read(true).open(&path)
        } else {
            OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)
        };
        let file = file.map_err(|e| format!("cannot open {name}: {e}"))?;
        match file.try_lock() {
            Ok(()) => held.0.push(file),
            Err(TryLockError::WouldBlock) => return Err("cargo is building there".into()),
            Err(TryLockError::Error(e)) => return Err(format!("cannot lock {name}: {e}")),
        }
    }
    Ok(held)
}

/// Total size of the files below `dir` (symlinks counted as themselves, never followed; capped).
fn dir_size(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.filter_map(Result::ok) {
            seen += 1;
            if seen > MAX_ENTRIES {
                return total;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

/// `path` below `target_root`, as `target/agent-3/debug/incremental` (always with `/`).
fn shown_path(target_root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(target_root).unwrap_or(path);
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    format!("target/{}", parts.join("/"))
}

/// `1234567` → `1.2 MB` (powers of 1000, like `du --si`).
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS.get(unit).copied().unwrap_or("TB"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cargo profile directory with an incremental cache of `bytes` bytes.
    fn profile(dir: &Path, bytes: usize) {
        std::fs::create_dir_all(dir.join(".fingerprint")).unwrap();
        std::fs::create_dir_all(dir.join("deps")).unwrap();
        std::fs::create_dir_all(dir.join("incremental").join("xtask-1abc").join("s-xyz")).unwrap();
        std::fs::write(dir.join("incremental").join("xtask-1abc").join("s-xyz").join("query-cache.bin"), vec![7u8; bytes]).unwrap();
        std::fs::write(dir.join("deps").join("libkeep.rlib"), b"keep").unwrap();
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(999), "999 B");
        assert_eq!(human(1_000), "1.0 KB");
        assert_eq!(human(1_234_567), "1.2 MB");
        assert_eq!(human(9_900_000_000), "9.9 GB");
        assert_eq!(human(u64::MAX), "18446744.1 TB");
    }

    #[test]
    fn fingerprint_paths() {
        use crate::tasks::config::is_fingerprint;
        assert!(is_fingerprint("crates/ui/tests/fingerprints/library-small.json"));
        assert!(is_fingerprint("tests/acceptance/fingerprints/m2-star.json"));
        assert!(!is_fingerprint("crates/scenes/src/fingerprint.rs"));
        assert!(!is_fingerprint("crates/ui/tests/fingerprints/README.md"));
        assert!(!is_fingerprint("fingerprints.json"));
    }

    #[test]
    fn prunes_only_idle_agents_incremental_caches() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        profile(&root.join("agent-2").join("debug"), 10); // busy: In Progress
        profile(&root.join("agent-3").join("debug"), 1_000);
        profile(&root.join("agent-3").join("accept-m0").join("target").join("debug"), 500);
        // Named `incremental` but not in a cargo profile: kept.
        std::fs::create_dir_all(root.join("agent-3").join("tmp").join("incremental")).unwrap();
        profile(&root.join("merge").join("debug"), 20);
        profile(&root.join("debug"), 30); // the main checkout's own dir: not an agent's
        profile(&root.join("m1a-review").join("debug"), 40);
        let busy = BTreeMap::from([("agent-2".to_string(), "M2.1".to_string())]);

        // A dry run removes nothing and creates nothing.
        let dry = prune(root, &busy, true);
        assert_eq!(
            dry.lines,
            [
                "agent-2: skipped, M2.1 is In Progress",
                "agent-3: would remove target/agent-3/accept-m0/target/debug/incremental (500 B)",
                "agent-3: would remove target/agent-3/debug/incremental (1.0 KB)",
                "merge: would remove target/merge/debug/incremental (20 B)",
            ]
        );
        assert_eq!(dry.freed, 1_520);
        assert!(root.join("agent-3").join("debug").join("incremental").is_dir());
        assert!(!root.join("agent-3").join("debug").join(".cargo-lock").exists(), "a dry run creates no lock file");

        let report = prune(root, &busy, false);
        assert_eq!(report.freed, 1_520);
        assert_eq!(report.errors, 0);
        assert!(report.lines.iter().all(|l| !l.contains("would")), "{:?}", report.lines);
        for gone in [
            root.join("agent-3").join("debug"),
            root.join("agent-3").join("accept-m0").join("target").join("debug"),
            root.join("merge").join("debug"),
        ] {
            assert!(!gone.join("incremental").exists(), "{}", gone.display());
            assert!(gone.join("deps").join("libkeep.rlib").is_file(), "only incremental/ goes");
        }
        for kept in [root.join("agent-2").join("debug"), root.join("debug"), root.join("m1a-review").join("debug")] {
            assert!(kept.join("incremental").is_dir(), "{}", kept.display());
        }
        assert!(root.join("agent-3").join("tmp").join("incremental").is_dir());

        // Nothing left: a second run says so and frees nothing.
        let again = prune(root, &busy, false);
        assert_eq!(again.freed, 0);
        assert!(again.lines.contains(&"agent-3: nothing to prune".to_string()), "{:?}", again.lines);
    }

    #[test]
    fn a_cargo_building_there_is_never_disturbed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let debug = root.join("agent-4").join("debug");
        profile(&debug, 100);
        // Cargo holds its build lock while it compiles.
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(debug.join(".cargo-build-lock")).unwrap();
        lock.lock().unwrap();
        for dry_run in [true, false] {
            let report = prune(root, &BTreeMap::new(), dry_run);
            assert_eq!(report.lines, ["agent-4: skipped target/agent-4/debug/incremental, cargo is building there"]);
            assert_eq!(report.freed, 0);
            assert!(debug.join("incremental").is_dir());
        }
        // Once cargo is done, it goes. Unlock before closing, as cargo's lock release does in
        // effect: other tests in this binary spawn git meanwhile, and a child spawned while `lock`
        // is open holds a copy of it until its exec closes it, which keeps a merely closed lock
        // held for a moment (it failed this way on macOS CI).
        lock.unlock().unwrap();
        drop(lock);
        let report = prune(root, &BTreeMap::new(), false);
        assert_eq!(report.lines, ["agent-4: removed target/agent-4/debug/incremental (100 B)"]);
        assert!(!debug.join("incremental").exists());
    }

    #[test]
    fn a_copy_of_a_lock_file_left_open_elsewhere_keeps_no_lock() {
        // A child process spawned while prune holds cargo's locks holds a copy of each lock file
        // until its exec closes it; `try_clone` makes the same kind of copy.
        let tmp = tempfile::tempdir().unwrap();
        let debug = tmp.path().join("debug");
        profile(&debug, 1);
        let held = lock_profile(&debug, false).unwrap();
        assert_eq!(held.0.len(), CARGO_LOCKS.len());
        let copies: Vec<File> = held.0.iter().map(|f| f.try_clone().unwrap()).collect();
        drop(held);
        // Cargo starting now gets every lock at once, although the copies are still open.
        let cargo = lock_profile(&debug, false).unwrap();
        assert_eq!(cargo.0.len(), CARGO_LOCKS.len());
        drop(copies);

        // Locks taken before a refusal are released too: a cargo holding `.cargo-build-lock`
        // makes prune give up after taking `.cargo-lock`, and must not leave that one locked.
        let build = OpenOptions::new().read(true).write(true).open(debug.join(".cargo-build-lock")).unwrap();
        drop(cargo);
        build.lock().unwrap();
        assert_eq!(lock_profile(&debug, false).err().as_deref(), Some("cargo is building there"));
        let first = OpenOptions::new().read(true).write(true).open(debug.join(".cargo-lock")).unwrap();
        assert!(first.try_lock().is_ok(), "the refused prune released .cargo-lock");
    }

    #[test]
    fn missing_root_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let report = prune(&tmp.path().join("no-target"), &BTreeMap::new(), false);
        assert!(report.lines.first().is_some_and(|l| l.starts_with("nothing to prune: cannot read")), "{:?}", report.lines);
        assert_eq!(report.freed, 0);

        #[cfg(unix)]
        {
            // A symlinked target dir is never followed: what it points at is kept.
            let root = tmp.path().join("target");
            let elsewhere = tmp.path().join("elsewhere");
            profile(&elsewhere.join("debug"), 10);
            std::fs::create_dir_all(&root).unwrap();
            std::os::unix::fs::symlink(&elsewhere, root.join("agent-5")).unwrap();
            // A symlinked profile inside a real target dir is not followed either.
            std::fs::create_dir_all(root.join("agent-6")).unwrap();
            std::os::unix::fs::symlink(elsewhere.join("debug"), root.join("agent-6").join("debug")).unwrap();
            let report = prune(&root, &BTreeMap::new(), false);
            assert_eq!(report.lines, ["agent-5: skipped, not a directory (symlinks are never followed)", "agent-6: nothing to prune"]);
            assert!(elsewhere.join("debug").join("incremental").is_dir());
        }
    }
}
