//! Send-backs and parity-summary conflicts on a temporary git repository (M5.22, found at M4.R):
//! the board runs on real git (a bare origin, a main checkout, worker and merge worktrees) and the
//! in-memory GitHub fake. Only `cargo xtask ci` is stubbed; nothing touches the real repository or
//! the real board.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use super::board::{Board, ci_marker};
use super::fake::{FakeClock, FakeGitHub};
use super::git::{Git, PathChange, RealGit};
use super::github::{MilestoneInfo, Status};
use super::roadmap::{self, Roadmap};

const ROADMAP: &str = "\
| Phase | # | Milestone | One-line goal | Status |
|---|---|---|---|---|
| F | M9 | Thing | a thing | ⬜ |

- ⬜ **M9.1 Thing**
  - after: —
  - files: `a.txt`, `docs/parity.md`
  - scope: s
  - done when: d
";

/// Five rows in two sections; rows are three lines apart so edits to different rows never touch.
fn parity_doc(statuses: [&str; 5]) -> String {
    let [a, b, c, d, e] = statuses;
    let text = format!(
        "# Parity\n\n## Summary\n\n<!-- parity:summary -->\n<!-- /parity:summary -->\n\n## A. Library view (LIB)\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
         | LIB-ONE | one | P0 | {a} | note |\n\n\
         Between one and two.\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
         | LIB-TWO | two | P0 | {b} | note |\n\n\
         ## B. Organizing (ORG)\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
         | ORG-ONE | one | P1 | {c} | note |\n\n\
         Between one and two.\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
         | ORG-TWO | two | P1 | {d} | note |\n\n\
         Between two and three.\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
         | ORG-THREE | three | P1 | {e} | note |\n"
    );
    crate::parity::with_summary(&text).unwrap()
}

/// [`RealGit`], except that `cargo xtask ci` is only recorded.
struct TempGit {
    real: RealGit,
    ci_runs: Mutex<Vec<PathBuf>>,
}

impl Git for TempGit {
    fn merged_task_ids(&self) -> Result<BTreeSet<String>, String> {
        self.real.merged_task_ids()
    }
    fn last_activity(&self, branch: &str) -> Option<i64> {
        self.real.last_activity(branch)
    }
    fn prepare_worktree(&self, agent: &str, branch: &str) -> Result<PathBuf, String> {
        self.real.prepare_worktree(agent, branch)
    }
    fn head(&self, dir: &Path) -> Result<String, String> {
        self.real.head(dir)
    }
    fn branch_subjects(&self, branch: &str) -> Result<Vec<String>, String> {
        self.real.branch_subjects(branch)
    }
    fn changed_paths(&self, branch: &str) -> Result<Vec<PathChange>, String> {
        self.real.changed_paths(branch)
    }
    fn pushed_head(&self, branch: &str) -> Option<String> {
        self.real.pushed_head(branch)
    }
    fn push(&self, dir: &Path, branch: &str) -> Result<(), String> {
        self.real.push(dir, branch)
    }
    fn rebase_for_merge(&self, branch: &str) -> Result<PathBuf, String> {
        self.real.rebase_for_merge(branch)
    }
    fn run_ci(&self, dir: &Path) -> Result<(), String> {
        self.ci_runs.lock().unwrap().push(dir.to_path_buf());
        Ok(())
    }
    fn merged_roadmap(&self) -> Result<String, String> {
        self.real.merged_roadmap()
    }
    fn append_devlog(&self, text: &str) -> Result<(), String> {
        self.real.append_devlog(text)
    }
    fn main_checkout(&self) -> PathBuf {
        self.real.main_checkout()
    }
    fn common_dir(&self) -> PathBuf {
        self.real.common_dir()
    }
}

/// Runs git in `dir`; panics with its stderr on failure.
fn g(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).env("GIT_EDITOR", "true").args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Runs git in `dir` and says whether it succeeded.
fn g_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git").current_dir(dir).env("GIT_EDITOR", "true").args(args).output().unwrap().status.success()
}

fn commit_all(dir: &Path, subject: &str) {
    g(dir, &["add", "-A"]);
    g(dir, &["commit", "--quiet", "-m", subject]);
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

struct Repo {
    tmp: tempfile::TempDir,
    main: PathBuf,
    roadmap: Roadmap,
    gh: FakeGitHub,
    git: TempGit,
    clock: FakeClock,
}

impl Repo {
    /// A bare origin whose `main` holds `a.txt` and `docs/parity.md`, a main checkout with a fixed
    /// identity (the rebases and commits `RealGit` makes need one), and a synced board with M9.1
    /// Ready.
    fn new() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, main) = (tmp.path().join("origin.git"), tmp.path().join("repo"));
        g(tmp.path(), &["init", "--quiet", "--bare", "--initial-branch=main", origin.to_str().unwrap()]);
        g(tmp.path(), &["clone", "--quiet", origin.to_str().unwrap(), main.to_str().unwrap()]);
        for (k, v) in [("user.name", "t"), ("user.email", "t@example.invalid"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
            g(&main, &["config", k, v]);
        }
        write(&main, "a.txt", "one\ntwo\nthree\n");
        write(&main, "docs/parity.md", &parity_doc(["⬜"; 5]));
        commit_all(&main, "M9.0: start");
        g(&main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);
        g(&main, &["fetch", "--quiet", "origin"]);

        let roadmap = roadmap::parse(ROADMAP).unwrap();
        let gh = FakeGitHub::with_setup();
        gh.lock().milestones = roadmap
            .milestones
            .iter()
            .zip(1..)
            .map(|(m, number)| MilestoneInfo { number, title: m.title(), description: m.goal.clone(), open: true, issues: 0 })
            .collect();
        let git = TempGit { real: RealGit { cwd: main.clone() }, ci_runs: Mutex::default() };
        let repo = Repo { tmp, main, roadmap, gh, git, clock: FakeClock { now: Mutex::new(1_000_000) } };
        repo.board().sync().unwrap();
        repo
    }

    fn board(&self) -> Board<'_> {
        Board { roadmap: &self.roadmap, gh: &self.gh, git: &self.git, clock: &self.clock }
    }

    /// Records a green `ci` on what `dir` has checked out, as `cargo xtask ci` does.
    fn ci_green(&self, dir: &Path) {
        let marker = ci_marker(&self.git.common_dir(), &self.git.head(dir).unwrap());
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(marker, "").unwrap();
    }

    /// Claims M9.1 for agent-2, lets `work` change its worktree, commits, records ci and a dev-log
    /// entry, and submits. Returns the worker's worktree.
    fn submit_work(&self, work: impl FnOnce(&Path)) -> PathBuf {
        let worker = self.board().claim("M9.1", "agent-2").unwrap().worktree;
        // Next to the temporary main checkout (compared by its end: macOS's temp dir is a symlink).
        assert!(worker.ends_with(Path::new("repo.worktrees").join("agent-2")), "{}", worker.display());
        work(&worker);
        commit_all(&worker, "M9.1: thing");
        self.ci_green(&worker);
        write(&self.main, "log/devlog.md", "## 2026-10-11 10:00 · agent-2 · M9.1 Thing\nLanded: the thing.\n");
        let url = self.board().submit("M9.1", &worker, None).unwrap();
        assert_eq!(url, "https://example.invalid/pull/100");
        assert_eq!(self.status(), Some(Status::InReview));
        worker
    }

    /// Another task lands on origin's `main` meanwhile.
    fn main_moves(&self, path: &str, text: &str) {
        g(&self.main, &["checkout", "--quiet", "--detach", "origin/main"]);
        write(&self.main, path, text);
        // Not `add -A`: the main checkout holds the untracked dev log.
        g(&self.main, &["add", "-u"]);
        g(&self.main, &["add", "--", path]);
        g(&self.main, &["commit", "--quiet", "-m", "M9.0: other work (#99)"]);
        g(&self.main, &["push", "--quiet", "origin", "HEAD:refs/heads/main"]);
        g(&self.main, &["fetch", "--quiet", "origin"]);
    }

    fn status(&self) -> Option<Status> {
        self.gh.item_status(1)
    }

    /// `path` as origin's `branch` has it.
    fn on_origin(&self, branch: &str, path: &str) -> String {
        g(&self.main, &["fetch", "--quiet", "origin"]);
        g(&self.main, &["show", &format!("origin/{branch}:{path}")])
    }

    /// The merge worktree is clean and not in the middle of a rebase.
    fn merge_worktree_is_clean(&self) {
        let merge = self.tmp.path().join("repo.worktrees").join("merge");
        assert_eq!(g(&merge, &["status", "--porcelain"]), "");
        for state in ["rebase-merge", "rebase-apply"] {
            let path = PathBuf::from(g(&merge, &["rev-parse", "--path-format=absolute", "--git-path", state]));
            assert!(!path.exists(), "no rebase in progress: {}", path.display());
        }
    }
}

const BRANCH: &str = "task/M9.1-thing";

/// The M4 run's friction: `merge` sent a branch back (it did not rebase cleanly) and `submit`
/// refused the rebased branch because the task was In Review; the lead rebuilt eight branches by
/// hand. Now the worker rebases, submits again, and the same pull request carries on to the merge.
#[test]
fn a_rebased_in_review_branch_is_submitted_again_and_keeps_its_pull_request() {
    let repo = Repo::new();
    let worker = repo.submit_work(|w| write(w, "a.txt", "one\nTWO (the task)\nthree\n"));
    let first_head = repo.git.head(&worker).unwrap();
    repo.main_moves("a.txt", "one\nTWO (main)\nthree\n");

    // The lead's merge cannot rebase it: a send-back, nothing pushed, nothing merged.
    let err = repo.board().merge("M9.1").unwrap_err();
    assert!(err.contains("does not rebase cleanly") && err.contains("conflicts in a.txt") && err.contains("send it back"), "{err}");
    assert!(repo.gh.lock().merged_prs.is_empty());
    assert!(repo.git.ci_runs.lock().unwrap().is_empty());
    repo.merge_worktree_is_clean();
    assert_eq!(repo.git.pushed_head(BRANCH).as_deref(), Some(first_head.as_str()));

    // Submitting the same commit again is refused: nothing was rebased.
    let err = repo.board().submit("M9.1", &worker, None).unwrap_err();
    assert!(err.contains("is In Review") && err.contains("rebased"), "{err}");

    // The worker rebases on origin/main and resolves the conflict; without a green ci on the new
    // commit, submit still refuses.
    g(&worker, &["fetch", "--quiet", "origin"]);
    assert!(!g_ok(&worker, &["rebase", "--quiet", "origin/main"]));
    write(&worker, "a.txt", "one\nTWO (main, then the task)\nthree\n");
    g(&worker, &["add", "a.txt"]);
    g(&worker, &["rebase", "--continue"]);
    let rebased = repo.git.head(&worker).unwrap();
    assert_ne!(rebased, first_head);
    let err = repo.board().submit("M9.1", &worker, None).unwrap_err();
    assert!(err.contains("ci has not passed"), "{err}");
    repo.ci_green(&worker);

    // Submitted again: pushed with lease over the old head, the same pull request, still In Review.
    let mutations = repo.gh.mutations();
    let url = repo.board().submit("M9.1", &worker, None).unwrap();
    assert_eq!(url, "https://example.invalid/pull/100", "the pull request keeps its number");
    assert_eq!(repo.gh.lock().prs.len(), 1, "no second pull request");
    assert_eq!(repo.gh.lock().pr_texts.len(), 1, "its body is kept");
    assert_eq!(repo.status(), Some(Status::InReview));
    assert_eq!(repo.git.pushed_head(BRANCH).as_deref(), Some(rebased.as_str()));
    assert_eq!(repo.on_origin(BRANCH, "a.txt"), "one\nTWO (main, then the task)\nthree");
    assert_eq!(repo.gh.mutations(), mutations + 1, "one write: the comment");
    let comment = repo.gh.lock().comments.last().cloned().unwrap();
    assert_eq!(comment, (1, format!("submitted again after a rebase ({rebased}): https://example.invalid/pull/100")));

    // The lead merges it now.
    assert_eq!(repo.board().merge("M9.1").unwrap(), "M9.1: Thing (#100)");
    assert_eq!(repo.gh.lock().merged_prs, [(100, "M9.1: Thing (#100)".to_string())]);
    assert_eq!(repo.git.ci_runs.lock().unwrap().len(), 1);
}

/// A lease protects the re-submit: when origin's branch moved since the worker last fetched, the
/// push is refused and the board is left as it was.
#[test]
fn a_resubmit_never_overwrites_a_branch_that_moved_on_origin() {
    let repo = Repo::new();
    let worker = repo.submit_work(|w| write(w, "a.txt", "one\ntwo\nthree\nfour\n"));
    // Someone else pushes to the branch from another clone; the worker's view of origin is stale.
    let other = repo.tmp.path().join("other");
    g(repo.tmp.path(), &["clone", "--quiet", "--branch", BRANCH, repo.tmp.path().join("origin.git").to_str().unwrap(), other.to_str().unwrap()]);
    for (k, v) in [("user.name", "o"), ("user.email", "o@example.invalid"), ("commit.gpgsign", "false")] {
        g(&other, &["config", k, v]);
    }
    write(&other, "b.txt", "b\n");
    commit_all(&other, "M9.1: someone else");
    g(&other, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{BRANCH}")]);
    let theirs = g(&other, &["rev-parse", "HEAD"]);

    g(&worker, &["commit", "--quiet", "--amend", "-m", "M9.1: thing, reworded"]);
    repo.ci_green(&worker);
    let mutations = repo.gh.mutations();
    let err = repo.board().submit("M9.1", &worker, None).unwrap_err();
    assert!(err.contains("push"), "{err}");
    assert_eq!(repo.gh.mutations(), mutations, "nothing on the board changed");
    g(&repo.main, &["fetch", "--quiet", "origin"]);
    assert_eq!(g(&repo.main, &["rev-parse", &format!("origin/{BRANCH}")]), theirs, "their commit is still there");
}

/// `docs/parity.md`'s summary is generated: when the branch and `main` each changed a different
/// row, only the summary conflicts, and `merge` regenerates it instead of sending the branch back.
#[test]
fn a_conflict_only_in_the_parity_summary_merges_after_regenerating_it() {
    let repo = Repo::new();
    repo.submit_work(|w| write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"])));
    repo.main_moves("docs/parity.md", &parity_doc(["⬜", "⬜", "🟡", "⬜", "⬜"]));

    assert_eq!(repo.board().merge("M9.1").unwrap(), "M9.1: Thing (#100)");
    let merged = repo.on_origin(BRANCH, "docs/parity.md");
    let expected = parity_doc(["✅", "⬜", "🟡", "⬜", "⬜"]);
    assert_eq!(format!("{merged}\n"), expected, "both rows, and a summary that counts them");
    assert!(expected.contains("| A. Library view | 1 | 0 | 1 | 0 |\n| B. Organizing | 0 | 1 | 2 | 0 |\n| **All** | 1 | 1 | 3 | 0 |\n"), "{expected}");
    let ci = repo.git.ci_runs.lock().unwrap().clone();
    assert_eq!(ci.len(), 1, "ci ran on the regenerated result");
    assert_eq!(std::fs::read_to_string(ci[0].join("docs/parity.md")).unwrap(), expected);
    assert_eq!(repo.gh.lock().merged_prs.len(), 1);
    repo.merge_worktree_is_clean();
    // The branch's history: its own commit on top of main's, nothing else.
    let log = g(&repo.main, &["log", "--format=%s", &format!("origin/{BRANCH}")]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["M9.1: thing", "M9.0: other work (#99)", "M9.0: start"]);
}

/// Several commits that each conflict in the summary are each resolved, and commits left empty by
/// the regeneration (they only changed the summary) are dropped.
#[test]
fn every_summary_conflict_of_a_long_branch_is_resolved() {
    let repo = Repo::new();
    repo.submit_work(|w| {
        write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]));
        commit_all(w, "M9.1: first row");
        write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "🟡"]));
        commit_all(w, "M9.1: third org row");
        // A commit that only touches the summary (a stale hand edit), then one that undoes it.
        let doc = parity_doc(["✅", "⬜", "⬜", "⬜", "🟡"]).replace("| **All** | 1 | 1 | 3 | 0 |", "| **All** | 7 | 7 | 7 | 7 |");
        write(w, "docs/parity.md", &doc);
        commit_all(w, "M9.1: summary by hand");
        write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "🟡"]));
    });
    repo.main_moves("docs/parity.md", &parity_doc(["⬜", "✅", "⬜", "🟡", "⬜"]));

    assert_eq!(repo.board().merge("M9.1").unwrap(), "M9.1: Thing (#100)");
    let merged = format!("{}\n", repo.on_origin(BRANCH, "docs/parity.md"));
    assert_eq!(merged, parity_doc(["✅", "✅", "⬜", "🟡", "🟡"]));
    let log = g(&repo.main, &["log", "--format=%s", &format!("origin/main..origin/{BRANCH}")]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["M9.1: third org row", "M9.1: first row"], "the summary-only commits are gone");
    repo.merge_worktree_is_clean();
}

/// A later commit can apply cleanly on top of a regenerated summary and still leave it stale (here
/// a hand edit of a line the regeneration did not change): the result is refreshed once more, so
/// `ci`'s parity check passes.
#[test]
fn the_summary_is_fresh_after_the_rebase_even_when_a_later_commit_applied_cleanly() {
    let repo = Repo::new();
    repo.submit_work(|w| {
        write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]));
        commit_all(w, "M9.1: first row");
        let stale = parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]).replace("| B. Organizing | 0 | 0 | 3 | 0 |", "| B. Organizing | 5 | 5 | 5 | 5 |");
        assert_ne!(stale, parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]));
        write(w, "docs/parity.md", &stale);
    });
    // Main's own stale hand edit of the total: it conflicts with the branch's first commit.
    let main_doc = parity_doc(["⬜"; 5]).replace("| **All** | 0 | 0 | 5 | 0 |", "| **All** | 7 | 7 | 7 | 7 |");
    assert_ne!(main_doc, parity_doc(["⬜"; 5]));
    repo.main_moves("docs/parity.md", &main_doc);

    assert_eq!(repo.board().merge("M9.1").unwrap(), "M9.1: Thing (#100)");
    let merged = format!("{}\n", repo.on_origin(BRANCH, "docs/parity.md"));
    assert_eq!(merged, parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]));
    assert_eq!(crate::parity::with_summary(&merged).unwrap(), merged, "`parity --write` would change nothing");
    repo.merge_worktree_is_clean();
}

/// When the branch and `main` change the same row, the conflict is the worker's: `merge` refuses,
/// the rebase is aborted, and nothing is pushed or merged.
#[test]
fn a_parity_row_conflict_is_sent_back() {
    let repo = Repo::new();
    let worker = repo.submit_work(|w| write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"])));
    let pushed = repo.git.pushed_head(BRANCH);
    assert_eq!(pushed, Some(repo.git.head(&worker).unwrap()));
    repo.main_moves("docs/parity.md", &parity_doc(["🟡", "⬜", "⬜", "⬜", "⬜"]));

    let err = repo.board().merge("M9.1").unwrap_err();
    assert!(err.contains("both sides changed row LIB-ONE") && err.contains("send it back"), "{err}");
    assert!(repo.git.ci_runs.lock().unwrap().is_empty());
    assert!(repo.gh.lock().merged_prs.is_empty());
    assert_eq!(repo.git.pushed_head(BRANCH), pushed, "nothing pushed");
    assert_eq!(repo.status(), Some(Status::InReview), "still In Review: the worker submits it again");
    repo.merge_worktree_is_clean();

    // A conflict in the parity file and another file is refused too, even when the parity part is
    // only the summary.
    let repo = Repo::new();
    repo.submit_work(|w| {
        write(w, "docs/parity.md", &parity_doc(["✅", "⬜", "⬜", "⬜", "⬜"]));
        write(w, "a.txt", "one\nTWO (the task)\nthree\n");
    });
    g(&repo.main, &["checkout", "--quiet", "--detach", "origin/main"]);
    write(&repo.main, "a.txt", "one\nTWO (main)\nthree\n");
    repo.main_moves("docs/parity.md", &parity_doc(["⬜", "⬜", "🟡", "⬜", "⬜"]));
    let err = repo.board().merge("M9.1").unwrap_err();
    assert!(err.contains("conflicts in a.txt, docs/parity.md"), "{err}");
    repo.merge_worktree_is_clean();
}

/// One table whose rows are neighbours (as in the real `docs/parity.md`): edits to two different
/// rows touch adjacent lines, so git's own rebase conflicts on them.
fn neighbours_doc(rows: &[(&str, &str)]) -> String {
    let mut text = String::from(
        "# Parity\n\n## Summary\n\n<!-- parity:summary -->\n<!-- /parity:summary -->\n\n## A. Library view (LIB)\n\n\
         | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n",
    );
    for (id, status) in rows {
        text.push_str(&format!("| {id} | feature | P0 | {status} | note |\n"));
    }
    crate::parity::with_summary(&text).unwrap()
}

const NEIGHBOURS: &[(&str, &str)] = &[("LIB-ONE", "⬜"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜")];

/// The M5 run's friction (M6.40): two tasks that each edit their own row of the same table conflict
/// as adjacent lines. `merge` now merges them by row id, keeps both, and regenerates the summary.
#[test]
fn neighbouring_parity_row_edits_merge_with_both_rows_kept() {
    let repo = Repo::new();
    repo.main_moves("docs/parity.md", &neighbours_doc(NEIGHBOURS));
    // The branch edits the first row and adds one after the last; main edits the last.
    let branch_doc = neighbours_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜"), ("LIB-NEW", "🟡")]);
    repo.submit_work(|w| write(w, "docs/parity.md", &branch_doc));
    let main_doc = neighbours_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "⬜"), ("LIB-THREE", "🟡")]);
    repo.main_moves("docs/parity.md", &main_doc);

    // Git alone cannot merge the two: the edits touch neighbouring lines.
    let tmp = repo.tmp.path();
    for (name, text) in [("base", neighbours_doc(NEIGHBOURS)), ("ours", main_doc.clone()), ("theirs", branch_doc.clone())] {
        write(tmp, name, &text);
    }
    assert!(!g_ok(tmp, &["merge-file", "-p", "ours", "base", "theirs"]), "the edits conflict as lines");

    assert_eq!(repo.board().merge("M9.1").unwrap(), "M9.1: Thing (#100)");
    let merged = format!("{}\n", repo.on_origin(BRANCH, "docs/parity.md"));
    let expected = neighbours_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "⬜"), ("LIB-THREE", "🟡"), ("LIB-NEW", "🟡")]);
    assert_eq!(merged, expected, "both sides' rows kept");
    assert!(expected.contains("| A. Library view | 1 | 2 | 1 | 0 |\n| **All** | 1 | 2 | 1 | 0 |\n"), "the summary counts both: {expected}");
    assert_eq!(crate::parity::with_summary(&merged).unwrap(), merged, "`parity --write` would change nothing");
    let ci = repo.git.ci_runs.lock().unwrap().clone();
    assert_eq!(ci.len(), 1, "ci ran on the merged result");
    assert_eq!(std::fs::read_to_string(ci[0].join("docs/parity.md")).unwrap(), expected);
    assert_eq!(repo.gh.lock().merged_prs.len(), 1);
    repo.merge_worktree_is_clean();
    let log = g(&repo.main, &["log", "--format=%s", &format!("origin/main..origin/{BRANCH}")]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["M9.1: thing"]);
}

/// Two edits of the same row stay the worker's to resolve, even among neighbours: `merge` refuses,
/// names the row, aborts the rebase and pushes nothing.
#[test]
fn an_edit_of_the_same_parity_row_by_both_sides_is_refused() {
    let repo = Repo::new();
    repo.main_moves("docs/parity.md", &neighbours_doc(NEIGHBOURS));
    repo.submit_work(|w| write(w, "docs/parity.md", &neighbours_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "✅"), ("LIB-THREE", "⬜")])));
    let pushed = repo.git.pushed_head(BRANCH);
    repo.main_moves("docs/parity.md", &neighbours_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]));

    let err = repo.board().merge("M9.1").unwrap_err();
    assert!(err.contains("both sides changed row LIB-TWO") && err.contains("send it back"), "{err}");
    assert!(repo.git.ci_runs.lock().unwrap().is_empty());
    assert!(repo.gh.lock().merged_prs.is_empty());
    assert_eq!(repo.git.pushed_head(BRANCH), pushed, "nothing pushed");
    assert_eq!(repo.status(), Some(Status::InReview));
    repo.merge_worktree_is_clean();
}
