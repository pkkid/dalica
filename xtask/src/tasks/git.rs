//! Local git and clock operations the board needs, as traits so tests can fake them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::devlog;
use super::roadmap::split_id;

pub trait Git: Send + Sync {
    /// Task ids with a commit `M1.4: …` on `main` or `origin/main` (merged work).
    fn merged_task_ids(&self) -> Result<BTreeSet<String>, String>;
    /// Unix time of the newest commit on `origin/<branch>` (or the local branch), if any.
    fn last_activity(&self, branch: &str) -> Option<i64>;
    /// Creates or reuses `agent`'s worktree and checks out `branch` there (new branches start at
    /// `origin/main`). Returns the worktree path.
    fn prepare_worktree(&self, agent: &str, branch: &str) -> Result<PathBuf, String>;
    /// The commit checked out in `dir`.
    fn head(&self, dir: &Path) -> Result<String, String>;
    /// Subjects of the commits on `branch` that are not on `origin/main`.
    fn branch_subjects(&self, branch: &str) -> Result<Vec<String>, String>;
    /// Paths (with `/`) that origin's `branch` adds, changes or deletes since it left `origin/main`.
    fn changed_paths(&self, branch: &str) -> Result<Vec<PathChange>, String>;
    /// The commit origin's `branch` was at when last fetched or pushed, if it is there.
    fn pushed_head(&self, branch: &str) -> Option<String>;
    /// Pushes what `dir` has checked out to origin's `branch`, unless origin's `branch` moved since
    /// the last fetch.
    fn push(&self, dir: &Path, branch: &str) -> Result<(), String>;
    /// Checks out origin's `branch` (detached) in the merge worktree, rebases it onto `origin/main`
    /// and returns that worktree. The local branch is not used: the worker may still have it
    /// checked out in its own worktree. A conflict only in `docs/parity.md`'s generated summary is
    /// resolved by regenerating the summary; any other conflict aborts the rebase (a send-back).
    fn rebase_for_merge(&self, branch: &str) -> Result<PathBuf, String>;
    /// Runs `cargo xtask ci` in `dir`.
    fn run_ci(&self, dir: &Path) -> Result<(), String>;
    /// Fetches origin and returns `docs/roadmap.md` as it is on `origin/main` (what a merge just
    /// landed), read from git's objects: no checkout, clean or dirty, is touched.
    fn merged_roadmap(&self) -> Result<String, String>;
    /// Appends `text` to the private dev log in the main checkout (`docs/task-board.md` §8).
    fn append_devlog(&self, text: &str) -> Result<(), String>;
    /// The main checkout (parent of the git common directory).
    fn main_checkout(&self) -> PathBuf;
    /// The git common directory, shared by every worktree (locks live here).
    fn common_dir(&self) -> PathBuf;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
    /// Local time offset from UTC in seconds (dev-log times are local).
    fn utc_offset(&self) -> i64;
    /// Waits `secs` seconds (the fake only advances its time).
    fn pause(&self, secs: u64);
}

/// How a branch touches a path (`git diff --name-status`, renames split into a delete and an add).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Changed,
    Deleted,
}

impl ChangeKind {
    pub fn name(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Changed => "changed",
            ChangeKind::Deleted => "deleted",
        }
    }
}

/// One path a branch adds, changes or deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathChange {
    pub kind: ChangeKind,
    pub path: String,
}

impl PathChange {
    pub fn new(kind: ChangeKind, path: &str) -> PathChange {
        PathChange { kind, path: path.to_string() }
    }
}

/// Parses `git diff --name-status -z` output: a status letter, then the path, each NUL-terminated.
/// `A` is an add, `D` a delete, anything else (`M`, `T`, …) a change.
pub fn parse_name_status(out: &str) -> Vec<PathChange> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let kind = match status.chars().next() {
            Some('A') => ChangeKind::Added,
            Some('D') => ChangeKind::Deleted,
            _ => ChangeKind::Changed,
        };
        changes.push(PathChange::new(kind, path));
    }
    changes
}

/// Parses a commit subject's task id: `M1.4: scenes (#12)` → `M1.4`.
pub fn subject_task_id(subject: &str) -> Option<&str> {
    let (id, _) = subject.split_once(':')?;
    split_id(id).map(|_| id)
}

pub struct RealGit {
    /// Any directory inside the repository (the current worktree).
    pub cwd: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    run_git(Command::new("git").current_dir(dir).args(args), args)
}

/// Like [`git`], for commands that may want an editor (`rebase --continue`): the editor is `true`,
/// so the message git proposes is kept and nothing waits for a terminal.
fn git_no_editor(dir: &Path, args: &[&str]) -> Result<String, String> {
    run_git(Command::new("git").current_dir(dir).env("GIT_EDITOR", "true").args(args), args)
}

fn run_git(cmd: &mut Command, args: &[&str]) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

impl RealGit {
    fn worktree_path(&self, name: &str) -> PathBuf {
        let main = self.main_checkout();
        let parent = main.parent().map(Path::to_path_buf).unwrap_or_else(|| main.clone());
        let repo = main.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dalica".into());
        parent.join(format!("{repo}.worktrees")).join(name)
    }

    fn fetch(&self) -> Result<(), String> {
        git(&self.cwd, &["fetch", "--quiet", "origin"]).map(|_| ())
    }

    fn ref_exists(&self, r: &str) -> bool {
        git(&self.cwd, &["rev-parse", "--verify", "--quiet", r]).is_ok()
    }

    /// Checks out `branch` in `dir`: the local branch if it exists, else the remote one, else a new
    /// branch at `origin/main`.
    fn checkout(&self, dir: &Path, branch: &str) -> Result<(), String> {
        if self.ref_exists(&format!("refs/heads/{branch}")) {
            git(dir, &["checkout", "--quiet", branch])?;
        } else if self.ref_exists(&format!("refs/remotes/origin/{branch}")) {
            git(dir, &["checkout", "--quiet", "-B", branch, &format!("origin/{branch}")])?;
        } else {
            git(dir, &["checkout", "--quiet", "-B", branch, "origin/main"])?;
        }
        Ok(())
    }

    /// `name`'s worktree, created (detached at `origin/main`) if missing; refused if it has
    /// uncommitted changes.
    fn clean_worktree(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.worktree_path(name);
        if path.join(".git").exists() {
            let dirty = git(&path, &["status", "--porcelain"])?;
            if !dirty.is_empty() {
                return Err(format!("worktree {} has uncommitted changes; commit or clean them first", path.display()));
            }
        } else {
            let p = path.to_string_lossy().to_string();
            git(&self.cwd, &["worktree", "add", "--quiet", "--detach", &p, "origin/main"])?;
        }
        Ok(path)
    }

    fn ensure_worktree(&self, name: &str, branch: &str) -> Result<PathBuf, String> {
        let path = self.clean_worktree(name)?;
        self.checkout(&path, branch)?;
        Ok(path)
    }
}

impl Git for RealGit {
    fn merged_task_ids(&self) -> Result<BTreeSet<String>, String> {
        let mut ids = BTreeSet::new();
        for r in ["refs/heads/main", "refs/remotes/origin/main"] {
            if !self.ref_exists(r) {
                continue;
            }
            for subject in git(&self.cwd, &["log", "--format=%s", r])?.lines() {
                if let Some(id) = subject_task_id(subject) {
                    ids.insert(id.to_string());
                }
            }
        }
        Ok(ids)
    }

    fn last_activity(&self, branch: &str) -> Option<i64> {
        [format!("refs/remotes/origin/{branch}"), format!("refs/heads/{branch}")]
            .iter()
            .filter_map(|r| git(&self.cwd, &["log", "-1", "--format=%ct", r]).ok())
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .max()
    }

    fn prepare_worktree(&self, agent: &str, branch: &str) -> Result<PathBuf, String> {
        self.fetch()?;
        self.ensure_worktree(agent, branch)
    }

    fn head(&self, dir: &Path) -> Result<String, String> {
        git(dir, &["rev-parse", "HEAD"])
    }

    fn branch_subjects(&self, branch: &str) -> Result<Vec<String>, String> {
        let range = format!("origin/main..{branch}");
        Ok(git(&self.cwd, &["log", "--reverse", "--format=%s", &range])?.lines().map(str::to_string).collect())
    }

    fn changed_paths(&self, branch: &str) -> Result<Vec<PathChange>, String> {
        let range = format!("origin/main...origin/{branch}");
        Ok(parse_name_status(&git(&self.cwd, &["diff", "--name-status", "-z", "--no-renames", &range])?))
    }

    fn pushed_head(&self, branch: &str) -> Option<String> {
        git(&self.cwd, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")]).ok()
    }

    fn push(&self, dir: &Path, branch: &str) -> Result<(), String> {
        let lease = format!("--force-with-lease=refs/heads/{branch}");
        git(dir, &["push", "--quiet", &lease, "origin", &format!("HEAD:refs/heads/{branch}")]).map(|_| ())
    }

    fn rebase_for_merge(&self, branch: &str) -> Result<PathBuf, String> {
        self.fetch()?;
        let remote = format!("origin/{branch}");
        if !self.ref_exists(&format!("refs/remotes/{remote}")) {
            return Err(format!("origin has no {branch}; `tasks submit` pushes it"));
        }
        let path = self.clean_worktree("merge")?;
        git(&path, &["checkout", "--quiet", "--detach", &remote])?;
        rebase_onto_main(&path, branch)?;
        Ok(path)
    }

    fn run_ci(&self, dir: &Path) -> Result<(), String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        // Its own target directory (never another checkout's: cargo could reuse its binaries).
        let target = ci_target_dir(&self.main_checkout(), dir);
        let status = Command::new(cargo)
            .current_dir(dir)
            .env("CARGO_TARGET_DIR", &target)
            .args(["xtask", "ci"])
            .status()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if status.success() { Ok(()) } else { Err(format!("cargo xtask ci failed in {}", dir.display())) }
    }

    fn merged_roadmap(&self) -> Result<String, String> {
        self.fetch()?;
        git(&self.cwd, &["show", "origin/main:docs/roadmap.md"])
    }

    fn append_devlog(&self, text: &str) -> Result<(), String> {
        devlog::append(&devlog::path(&self.main_checkout()), text)
    }

    fn main_checkout(&self) -> PathBuf {
        let common = self.common_dir();
        common.parent().map(Path::to_path_buf).unwrap_or(common)
    }

    fn common_dir(&self) -> PathBuf {
        match git(&self.cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
            Ok(p) => PathBuf::from(p),
            Err(_) => self.cwd.join(".git"),
        }
    }
}

/// The generated file whose conflicts `merge` resolves itself (`docs/task-board.md` §5).
const PARITY: &str = "docs/parity.md";

/// Rebase steps (one per conflicting commit) `rebase_onto_main` resolves before it gives up.
const MAX_REBASE_STEPS: usize = 200;

/// Rebases what `dir` has checked out onto `origin/main`. Each stop on a conflict only in the parity
/// summary is resolved by regenerating it ([`crate::parity::resolve_summary_conflicts`]) and the
/// rebase goes on; any other conflict aborts it and is the worker's to resolve (a send-back). When
/// a summary was regenerated, the summary of the result is refreshed once more (a later commit can
/// apply cleanly on top of a regenerated one and still leave it stale) and folded into the last
/// commit.
fn rebase_onto_main(dir: &Path, branch: &str) -> Result<(), String> {
    let mut step = git(dir, &["rebase", "--quiet", "origin/main"]);
    let mut regenerated = false;
    for _ in 0..MAX_REBASE_STEPS {
        let Err(e) = &step else { break };
        if let Err(why) = resolve_summary_stop(dir) {
            let _ = git(dir, &["rebase", "--abort"]);
            return Err(format!("{branch} does not rebase cleanly on origin/main ({why}); send it back to its worker ({e})"));
        }
        regenerated = true;
        // A commit whose only change was the summary may now change nothing: skip it.
        step = if git(dir, &["diff", "--cached", "--quiet"]).is_ok() {
            git_no_editor(dir, &["rebase", "--skip"])
        } else {
            git_no_editor(dir, &["rebase", "--continue"])
        };
    }
    if let Err(e) = step {
        let _ = git(dir, &["rebase", "--abort"]);
        return Err(format!("{branch} still conflicts after {MAX_REBASE_STEPS} rebase steps; send it back to its worker ({e})"));
    }
    if regenerated {
        let file = dir.join(PARITY);
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {PARITY} after the rebase: {e}"))?;
        let fresh = crate::parity::with_summary(&text).ok_or_else(|| format!("{PARITY}: the parity:summary markers are missing"))?;
        if fresh != text {
            std::fs::write(&file, fresh).map_err(|e| format!("cannot write {PARITY}: {e}"))?;
            git(dir, &["add", "--", PARITY])?;
            // The refresh may undo all the last commit did (a stale hand edit of the summary).
            git(dir, &["commit", "--quiet", "--amend", "--no-edit", "--allow-empty"])?;
        }
    }
    Ok(())
}

/// At a rebase stopped on conflicts: when the only conflicted path is `docs/parity.md` and every
/// conflict in it is inside the generated summary, writes the file with the summary regenerated and
/// stages it. Otherwise says what conflicts.
fn resolve_summary_stop(dir: &Path) -> Result<(), String> {
    let conflicted: Vec<String> =
        git(dir, &["diff", "--name-only", "--diff-filter=U", "-z"])?.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect();
    match conflicted.as_slice() {
        [] => return Err("the rebase stopped without a conflicted file".into()),
        [only] if only == PARITY => {}
        paths => return Err(format!("conflicts in {}", paths.join(", "))),
    }
    let file = dir.join(PARITY);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {PARITY}: {e}"))?;
    let resolved = crate::parity::resolve_summary_conflicts(&text)?;
    std::fs::write(&file, resolved).map_err(|e| format!("cannot write {PARITY}: {e}"))?;
    git(dir, &["add", "--", PARITY]).map(|_| ())
}

/// The target directory `run_ci` uses for the worktree `dir`: `<main checkout>/target/<name>`.
pub fn ci_target_dir(main: &Path, dir: &Path) -> PathBuf {
    let name = dir.file_name().map_or_else(|| "merge".into(), |n| n.to_string_lossy().into_owned());
    main.join("target").join(name)
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn pause(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn now(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }

    fn utc_offset(&self) -> i64 {
        // `date +%z` prints e.g. `-0400`; where it is missing (Windows), log times are UTC.
        let Ok(out) = Command::new("date").arg("+%z").output() else { return 0 };
        parse_offset(String::from_utf8_lossy(&out.stdout).trim()).unwrap_or(0)
    }
}

/// `-0400` → -14400.
pub fn parse_offset(s: &str) -> Option<i64> {
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, s.get(1..)?),
        b'-' => (-1, s.get(1..)?),
        _ => return None,
    };
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let h: i64 = digits.get(..2)?.parse().ok()?;
    let m: i64 = digits.get(2..)?.parse().ok()?;
    Some(sign * (h * 3600 + m * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_and_offsets() {
        assert_eq!(subject_task_id("M1.4: scenes (#12)"), Some("M1.4"));
        assert_eq!(subject_task_id("M0.R: review"), Some("M0.R"));
        assert_eq!(subject_task_id("Docs: roadmap"), None);
        assert_eq!(parse_offset("-0400"), Some(-14400));
        assert_eq!(parse_offset("+0530"), Some(19800));
        assert_eq!(parse_offset("EDT"), None);
        let main = Path::new("/w/dalica");
        assert_eq!(ci_target_dir(main, &Path::new("/w/dalica.worktrees").join("merge")), main.join("target").join("merge"));
    }

    #[test]
    fn name_status_kinds() {
        let out = "A\0crates/ui/tests/fingerprints/new.json\0M\0a.txt\0D\0old.json\0T\0link\0";
        assert_eq!(
            parse_name_status(out),
            [
                PathChange::new(ChangeKind::Added, "crates/ui/tests/fingerprints/new.json"),
                PathChange::new(ChangeKind::Changed, "a.txt"),
                PathChange::new(ChangeKind::Deleted, "old.json"),
                PathChange::new(ChangeKind::Changed, "link"),
            ]
        );
        assert!(parse_name_status("").is_empty());
        // A status without its path (truncated output) is dropped, not misread.
        assert_eq!(parse_name_status("M\0a.txt\0A"), [PathChange::new(ChangeKind::Changed, "a.txt")]);
    }

    /// `changed_paths` tells adds, changes and deletes apart on a real repository (what `merge`
    /// records for the review), and a rename counts as a delete and an add.
    #[test]
    fn changed_paths_report_the_kind_of_change() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, main) = (tmp.path().join("origin.git"), tmp.path().join("repo"));
        g(tmp.path(), &["init", "--quiet", "--bare", origin.to_str().unwrap()]);
        g(tmp.path(), &["clone", "--quiet", origin.to_str().unwrap(), main.to_str().unwrap()]);
        let fp = main.join("tests").join("fingerprints");
        std::fs::create_dir_all(&fp).unwrap();
        for name in ["kept.json", "gone.json", "moved.json"] {
            std::fs::write(fp.join(name), format!("{{\"name\": \"{name}\"}}\n")).unwrap();
        }
        g(&main, &["add", "."]);
        g(&main, &["commit", "--quiet", "-m", "M0.1: start"]);
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);
        g(&main, &["checkout", "--quiet", "-b", "task/M9.1-thing"]);
        std::fs::write(fp.join("kept.json"), "{\"name\": \"kept, new pixels\"}\n").unwrap();
        std::fs::write(fp.join("new.json"), "{}\n").unwrap();
        g(&main, &["rm", "--quiet", "tests/fingerprints/gone.json"]);
        g(&main, &["mv", "tests/fingerprints/moved.json", "tests/fingerprints/renamed.json"]);
        g(&main, &["add", "."]);
        g(&main, &["commit", "--quiet", "-m", "M9.1: thing"]);
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/task/M9.1-thing"]);
        g(&main, &["fetch", "--quiet", "origin"]);
        let git = RealGit { cwd: main.clone() };
        let mut changes = git.changed_paths("task/M9.1-thing").unwrap();
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            changes,
            [
                PathChange::new(ChangeKind::Deleted, "tests/fingerprints/gone.json"),
                PathChange::new(ChangeKind::Changed, "tests/fingerprints/kept.json"),
                PathChange::new(ChangeKind::Deleted, "tests/fingerprints/moved.json"),
                PathChange::new(ChangeKind::Added, "tests/fingerprints/new.json"),
                PathChange::new(ChangeKind::Added, "tests/fingerprints/renamed.json"),
            ]
        );
    }

    /// Runs git with a fixed identity in `dir`.
    fn g(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A live failure (M1.16, 2026-10-10): after `submit` the worker's worktree still had the
    /// branch checked out, so `merge` could not check it out in the merge worktree. Merge now works
    /// on origin's copy of the branch, whatever the worker's worktree holds.
    #[test]
    fn merge_rebases_a_branch_its_worker_still_has_checked_out() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, main) = (tmp.path().join("origin.git"), tmp.path().join("repo"));
        g(tmp.path(), &["init", "--quiet", "--bare", origin.to_str().unwrap()]);
        g(tmp.path(), &["clone", "--quiet", origin.to_str().unwrap(), main.to_str().unwrap()]);
        // RealGit's own commands (the rebase) need an identity and no signing in this repository.
        for (k, v) in [("user.name", "t"), ("user.email", "t@example.invalid"), ("commit.gpgsign", "false")] {
            g(&main, &["config", k, v]);
        }
        std::fs::write(main.join("a.txt"), "a\n").unwrap();
        g(&main, &["add", "."]);
        g(&main, &["commit", "--quiet", "-m", "M0.1: start"]);
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);
        g(&main, &["fetch", "--quiet", "origin"]);
        let git = RealGit { cwd: main.clone() };

        // The worker: a branch in its own worktree, committed and pushed (as `submit` does).
        let branch = "task/M9.1-thing";
        let worker = git.prepare_worktree("agent-9", branch).unwrap();
        std::fs::write(worker.join("b.txt"), "b\n").unwrap();
        g(&worker, &["add", "."]);
        g(&worker, &["commit", "--quiet", "-m", "M9.1: thing"]);
        git.push(&worker, branch).unwrap();
        assert_eq!(git.changed_paths(branch).unwrap(), [PathChange::new(ChangeKind::Added, "b.txt")]);

        // Meanwhile main moves on.
        std::fs::write(main.join("c.txt"), "c\n").unwrap();
        g(&main, &["add", "."]);
        g(&main, &["commit", "--quiet", "-m", "M9.0: other"]);
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);

        // The worker still has the branch checked out; the lead merges anyway.
        assert_eq!(g(&worker, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
        let dir = git.rebase_for_merge(branch).unwrap();
        git.push(&dir, branch).unwrap();
        g(&main, &["fetch", "--quiet", "origin"]);
        let log = g(&main, &["log", "--format=%s", &format!("origin/{branch}")]);
        assert_eq!(log.lines().collect::<Vec<_>>(), ["M9.1: thing", "M9.0: other", "M0.1: start"], "rebased onto the new main");
        // Only the branch's own files, not what main added meanwhile.
        assert_eq!(git.changed_paths(branch).unwrap(), [PathChange::new(ChangeKind::Added, "b.txt")]);
        assert!(git.rebase_for_merge("task/M9.2-missing").unwrap_err().contains("origin has no"));
    }

    /// Live failure (M2 merges, 2026-10-10): the sync after a merge read the main checkout's
    /// roadmap.md, which predates the squash. It now reads origin/main's, and never touches the
    /// checkout, even a dirty one.
    #[test]
    fn the_merged_roadmap_comes_from_origin_and_a_dirty_checkout_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, main, other) = (tmp.path().join("origin.git"), tmp.path().join("repo"), tmp.path().join("other"));
        g(tmp.path(), &["init", "--quiet", "--bare", origin.to_str().unwrap()]);
        g(tmp.path(), &["clone", "--quiet", origin.to_str().unwrap(), main.to_str().unwrap()]);
        std::fs::create_dir_all(main.join("docs")).unwrap();
        std::fs::write(main.join("docs").join("roadmap.md"), "- ⬜ **M9.1 Thing**\n").unwrap();
        g(&main, &["add", "."]);
        g(&main, &["commit", "--quiet", "-m", "M9.0: start"]);
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);

        // The squash-merge lands on origin's main from elsewhere (GitHub).
        g(tmp.path(), &["clone", "--quiet", origin.to_str().unwrap(), other.to_str().unwrap()]);
        std::fs::write(other.join("docs").join("roadmap.md"), "- ✅ **M9.1 Thing**\n").unwrap();
        g(&other, &["commit", "--quiet", "-am", "M9.1: thing (#1)"]);
        g(&other, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);

        // The lead's main checkout is behind and dirty.
        std::fs::write(main.join("docs").join("roadmap.md"), "- ⬜ **M9.1 Thing**\nlocal edit\n").unwrap();
        std::fs::write(main.join("scratch.txt"), "untracked\n").unwrap();
        let head = g(&main, &["rev-parse", "HEAD"]);
        let status = g(&main, &["status", "--porcelain"]);

        let git = RealGit { cwd: main.clone() };
        assert_eq!(git.merged_roadmap().unwrap(), "- ✅ **M9.1 Thing**");
        assert!(git.merged_task_ids().unwrap().contains("M9.1"), "the fetch brought the squash commit");
        assert_eq!(g(&main, &["rev-parse", "HEAD"]), head, "the checkout did not move");
        assert_eq!(g(&main, &["status", "--porcelain"]), status);
        assert_eq!(std::fs::read_to_string(main.join("docs").join("roadmap.md")).unwrap(), "- ⬜ **M9.1 Thing**\nlocal edit\n");
    }
}
