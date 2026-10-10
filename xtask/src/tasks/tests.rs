//! Board tests on the in-memory fakes (`docs/task-board.md` §9).

use std::path::Path;
use std::sync::{Arc, Barrier, Mutex};

use super::board::{Board, ci_marker};
use super::fake::{FakeClock, FakeGit, FakeGitHub};
use super::github::{GitHub, MergeState, MilestoneInfo, Status};
use super::lock::Lock;
use super::roadmap::{self, Roadmap};

const ROADMAP: &str = "\
| Phase | # | Milestone | One-line goal | Status |
|---|---|---|---|---|
| F | M0 | Skeleton | an empty workspace | ⬜ |
| | M1 | Star | star a photo | ⬜ |

- ⬜ **M0.1 Skeleton**
  - after: —
  - files: `Cargo.toml`, `crates/**`
  - scope: s
  - done when: d
- ⬜ **M0.2 Layers**
  - after: M0.1
  - files: `xtask/src/layers.rs`
  - scope: s
  - done when: d
- ⬜ **M0.3 Deps**
  - after: M0.1
  - files: `xtask/src/deps.rs`
  - scope: s
  - done when: d
- ⬜ **M0.4 Deps docs**
  - after: M0.1
  - files: `xtask/src/deps.rs`, `docs/roadmap.md`
  - scope: s
  - done when: d
- ⬜ **M0.R Review**
  - after: M0.2, M0.3
  - owner: maintainer
  - files: —
  - scope: s
  - done when: d
- ⬜ **M1.1 Core**
  - after: M0.1
  - files: `crates/core/**`
  - scope: s
  - done when: d
";

struct World {
    /// The roadmap text the world started from (`finish` ticks blocks in it).
    text: String,
    roadmap: Roadmap,
    gh: FakeGitHub,
    git: FakeGit,
    clock: FakeClock,
    _dir: tempfile::TempDir,
}

impl World {
    fn new(text: &str) -> World {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let roadmap = roadmap::parse(text).unwrap();
        let gh = FakeGitHub::with_setup();
        // `setup` has run: every roadmap milestone is on GitHub (sync needs them).
        gh.lock().milestones = roadmap
            .milestones
            .iter()
            .zip(1..)
            .map(|(m, number)| MilestoneInfo { number, title: m.title(), description: m.goal.clone(), open: true, issues: 0 })
            .collect();
        World { text: text.to_string(), roadmap, gh, git: FakeGit::new(dir.path()), clock: FakeClock { now: Mutex::new(1_000_000) }, _dir: dir }
    }

    fn board(&self) -> Board<'_> {
        Board { roadmap: &self.roadmap, gh: &self.gh, git: &self.git, clock: &self.clock }
    }

    /// Ticks a task ✅ in the roadmap (as a merged commit would) and records the merge on main.
    fn finish(&mut self, id: &str) {
        let text = self.text.replace(&format!("- ⬜ **{id} "), &format!("- ✅ **{id} "));
        let mut done: Vec<String> = self.roadmap.tasks.iter().filter(|t| t.done).map(|t| t.id.clone()).collect();
        done.push(id.to_string());
        let mut text = text;
        for d in &done {
            text = text.replace(&format!("- ⬜ **{d} "), &format!("- ✅ **{d} "));
        }
        self.roadmap = roadmap::parse(&text).unwrap();
        self.git.merged.lock().unwrap().insert(id.to_string());
    }

    fn issue_of(&self, id: &str) -> u64 {
        self.gh.lock().issues.iter().find(|i| i.title.starts_with(&format!("{id} "))).unwrap().number
    }

    fn status_of(&self, id: &str) -> Option<Status> {
        self.gh.item_status(self.issue_of(id))
    }

    fn next_ids(&self) -> Vec<String> {
        self.board().next(None).unwrap().into_iter().map(|t| t.id.clone()).collect()
    }
}

#[test]
fn sync_twice_makes_no_second_change() {
    let w = World::new(ROADMAP);
    let first = w.board().sync().unwrap();
    assert!(!first.changes.is_empty());
    assert_eq!(w.gh.lock().issues.len(), 6);
    assert_eq!(w.status_of("M0.1"), Some(Status::Ready));
    assert_eq!(w.status_of("M0.2"), Some(Status::Todo));
    let gate = w.gh.lock().issues.iter().find(|i| i.title == "M0.R Review").cloned().unwrap();
    assert_eq!(gate.labels, ["gate"]);
    assert_eq!(gate.assignees, ["pkkid"]);
    assert!(gate.body.contains("- **after:** M0.2 (#2), M0.3 (#3)"), "{}", gate.body);

    let before = w.gh.mutations();
    let second = w.board().sync().unwrap();
    assert!(second.changes.is_empty(), "{:?}", second.changes);
    assert_eq!(w.gh.mutations(), before);
}

#[test]
fn sync_of_the_real_roadmap_is_idempotent_and_ordered() {
    let text = std::fs::read_to_string(crate::workspace_root().join("docs/roadmap.md")).unwrap();
    let mut w = World::new(&text);
    // Merge evidence for every ✅ task, as `main` would have it.
    let done: Vec<String> = w.roadmap.tasks.iter().filter(|t| t.done).map(|t| t.id.clone()).collect();
    w.git.merged.lock().unwrap().extend(done);
    // A ✅ gate's issue was closed by the maintainer: sync with those gates still ⬜ to create
    // their issues, close them, then sync the real text.
    let gates: Vec<String> = w.roadmap.tasks.iter().filter(|t| t.done && t.is_gate()).map(|t| t.id.clone()).collect();
    let mut open = text.clone();
    for g in &gates {
        open = open.replace(&format!("- ✅ **{g} "), &format!("- ⬜ **{g} "));
    }
    w.roadmap = roadmap::parse(&open).unwrap();
    w.board().sync().unwrap();
    for g in &gates {
        w.gh.close_issue(w.issue_of(g), "approved").unwrap();
    }
    w.roadmap = roadmap::parse(&text).unwrap();
    w.board().sync().unwrap();
    // Shuffle the board, then sync puts it back in roadmap order.
    w.gh.lock().items.reverse();
    w.board().sync().unwrap();
    let order: Vec<u64> = w.gh.lock().items.iter().map(|i| i.issue).collect();
    let expected: Vec<u64> = w.roadmap.tasks.iter().map(|t| w.issue_of(&t.id)).collect();
    assert_eq!(order, expected);
    let before = w.gh.mutations();
    assert!(w.board().sync().unwrap().changes.is_empty());
    assert_eq!(w.gh.mutations(), before);
}

#[test]
fn sync_adopts_existing_issues_without_duplicates() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    // A human note below the generated section survives, and nothing is created twice.
    let n = w.issue_of("M0.2");
    let body = w.gh.lock().issues.iter().find(|i| i.number == n).unwrap().body.clone();
    w.gh.lock().issues.iter_mut().find(|i| i.number == n).unwrap().body = format!("{body}\nhuman note\n");
    let report = w.board().sync().unwrap();
    assert!(report.changes.is_empty(), "{:?}", report.changes);
    assert_eq!(w.gh.lock().issues.len(), 6);
}

#[test]
fn done_task_with_open_issue_fails_unless_merged() {
    let mut w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.finish("M0.1");
    // ✅ without a merge on main: sync refuses and changes nothing.
    w.git.merged.lock().unwrap().clear();
    let before = w.gh.mutations();
    let err = w.board().sync().unwrap_err();
    assert!(err.contains("M0.1 is ✅ in roadmap.md but its issue #1 is open"), "{err}");
    assert_eq!(w.gh.mutations(), before);
    // With the `M0.1:` commit on main, sync closes the issue and moves dependents to Ready.
    w.git.merged.lock().unwrap().insert("M0.1".into());
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Done));
    assert!(!w.gh.lock().issues[0].open);
    assert_eq!(w.status_of("M0.2"), Some(Status::Ready));
    assert!(w.board().sync().unwrap().changes.is_empty());
}

#[test]
fn closed_issue_without_tick_fails() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.gh.close_issue(w.issue_of("M0.2"), "x").unwrap();
    let err = w.board().sync().unwrap_err();
    assert!(err.contains("closed but the task is not ✅"), "{err}");
}

#[test]
fn next_respects_dependencies_overlap_and_gates() {
    let mut w = World::new(ROADMAP);
    w.board().sync().unwrap();
    assert_eq!(w.next_ids(), ["M0.1"]);

    // A stale board saying Ready never beats an unfinished dependency.
    let n = w.issue_of("M0.2");
    let item = w.gh.lock().items.iter().find(|i| i.issue == n).unwrap().id.clone();
    w.gh.set_status(&item, Status::Ready).unwrap();
    assert_eq!(w.next_ids(), ["M0.1"]);

    w.finish("M0.1");
    w.board().sync().unwrap();
    // M1.1 is Ready by dependencies but waits for the open M0.R gate.
    assert_eq!(w.status_of("M1.1"), Some(Status::Ready));
    assert_eq!(w.next_ids(), ["M0.2", "M0.3", "M0.4"]);

    // M0.3 and M0.4 share `xtask/src/deps.rs`; claiming one hides the other. `docs/roadmap.md` is shared.
    w.board().claim("M0.3", "agent-2").unwrap();
    assert_eq!(w.next_ids(), ["M0.2"]);
    let err = w.board().claim("M0.4", "agent-3").unwrap_err();
    assert!(err.contains("overlaps"), "{err}");
    assert!(w.board().claim("M0.R", "agent-3").unwrap_err().contains("maintainer"));

    // The gate closes → M1.1 becomes claimable.
    w.finish("M0.2");
    w.finish("M0.3");
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.R"), Some(Status::Ready));
    assert!(!w.next_ids().contains(&"M1.1".to_string()));
    w.gh.close_issue(w.issue_of("M0.R"), "approved").unwrap();
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.R"), Some(Status::Done));
    assert_eq!(w.next_ids(), ["M0.4", "M1.1"]);
}

#[test]
fn eight_threads_claiming_one_task_produce_one_winner() {
    let w = Arc::new(World::new(ROADMAP));
    w.board().sync().unwrap();
    let lock_path = w.git.common_dir_path().join("dalica-tasks.lock");
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (1..=8)
        .map(|n| {
            let (w, barrier, lock_path) = (Arc::clone(&w), Arc::clone(&barrier), lock_path.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let _lock = Lock::acquire(&lock_path).unwrap();
                w.board().claim("M0.1", &format!("agent-{n}")).is_ok()
            })
        })
        .collect();
    let winners = handles.into_iter().map(|h| h.join().unwrap()).filter(|won| *won).count();
    assert_eq!(winners, 1);
    assert_eq!(w.status_of("M0.1"), Some(Status::InProgress));
    assert_eq!(w.git.worktrees.load(std::sync::atomic::Ordering::SeqCst), 1);
    let item = w.gh.lock().items.iter().find(|i| i.issue == 1).cloned().unwrap();
    assert!(item.agent.starts_with("agent-") && item.branch == "task/M0.1-skeleton", "{item:?}");
}

#[test]
fn release_block_and_submit() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.board().claim("M0.1", "agent-2").unwrap();
    w.board().release("M0.1", "agent-2", "estimate was wrong").unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Ready));
    let item = w.gh.lock().items.iter().find(|i| i.issue == 1).cloned().unwrap();
    assert_eq!((item.agent.as_str(), item.branch.as_str()), ("", "task/M0.1-skeleton"));

    w.board().claim("M0.1", "agent-3").unwrap();
    w.board().block("M0.1", "agent-3", "which licence?").unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Blocked));
    assert!(w.gh.lock().issues[0].labels.contains(&"needs-maintainer".to_string()));
    // Still Blocked after a sync until the maintainer removes the label.
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Blocked));
    w.gh.lock().issues[0].labels.retain(|l| l != "needs-maintainer");
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Ready));

    w.board().claim("M0.1", "agent-3").unwrap();
    let dir = w.git.dir.clone();
    let err = w.board().submit("M0.1", &dir, None).unwrap_err();
    assert!(err.contains("ci has not passed"), "{err}");
    let marker = ci_marker(&w.git.common_dir_path(), "0123abcd");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();
    let err = w.board().submit("M0.1", &dir, None).unwrap_err();
    assert!(err.contains("no dev-log entry"), "{err}");
    std::fs::create_dir_all(dir.join("log")).unwrap();
    std::fs::write(dir.join("log/devlog.md"), "## 2026-10-09 14:02 · agent-3 · M0.1 Skeleton\nLanded: x\n").unwrap();
    let url = w.board().submit("M0.1", &dir, None).unwrap();
    assert!(url.contains("/pull/"));
    assert_eq!(w.status_of("M0.1"), Some(Status::InReview));
    // A red `main` on GitHub CI stops every merge until it is fixed.
    w.gh.lock().main_ci_failure = Some("M0.9: x https://example.test/run/1".into());
    let err = w.board().merge("M0.1").unwrap_err();
    assert!(err.contains("main is red"), "{err}");
    w.gh.lock().main_ci_failure = None;
    // In review, its files still block overlapping work; merge squashes with the task id.
    let subject = w.board().merge("M0.1").unwrap();
    assert_eq!(subject, "M0.1: Skeleton (#100)");
}

const REVIEWS: &str = "\
| Phase | # | Milestone | One-line goal | Status |
|---|---|---|---|---|
| F | M0 | Skeleton | an empty workspace | ⬜ |
| | M1 | Star | star a photo | ⬜ |

- ⬜ **M0.1 Before**
  - after: —
  - files: `crates/a/**`
  - scope: s
  - done when: d
- ⬜ **M0.A Checkpoint**
  - after: M0.1
  - owner: maintainer
  - files: `docs/status.md`
  - scope: s
  - done when: d
- ⬜ **M0.2 Middle**
  - after: —
  - files: `crates/b/**`
  - scope: s
  - done when: d
- ⬜ **M0.3 Lead plan**
  - after: —
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: s
  - done when: d
- ⬜ **M0.R Review**
  - after: M0.2
  - owner: maintainer
  - files: `docs/status.md`
  - scope: s
  - done when: d
- ⬜ **M1.1 Next**
  - after: —
  - files: `crates/c/**`
  - scope: s
  - done when: d
";

#[test]
fn an_open_review_holds_back_everything_listed_after_it() {
    let mut w = World::new(REVIEWS);
    w.board().sync().unwrap();
    // M0.2, M0.3 and M1.1 are Ready by dependencies, but all come after the open M0.A;
    // M0.1, listed before it, is not held back.
    assert_eq!(w.status_of("M0.2"), Some(Status::Ready));
    assert_eq!(w.status_of("M1.1"), Some(Status::Ready));
    assert_eq!(w.next_ids(), ["M0.1"]);
    let ids = |w: &World, agent| w.board().next(Some(agent)).unwrap().into_iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&w, "agent-1"), ["M0.1"]);
    // `claim` uses the same check as `next`, for the same milestone and a later one.
    for id in ["M0.2", "M1.1"] {
        let err = w.board().claim(id, "agent-2").unwrap_err();
        assert!(err.contains(&format!("{id} comes after M0.A")), "{err}");
    }
    assert!(w.board().claim("M0.3", "agent-1").unwrap_err().contains("comes after M0.A"));
    // Nobody claims the review itself, not even the lead.
    assert!(w.board().claim("M0.A", "agent-1").unwrap_err().contains("belongs to the maintainer"));

    // Its dependency done, the review is Ready, and still holds everything after it.
    w.finish("M0.1");
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.A"), Some(Status::Ready));
    assert!(w.next_ids().is_empty(), "{:?}", w.next_ids());

    // The maintainer closes M0.A: the tasks up to the next open review become claimable.
    w.gh.close_issue(w.issue_of("M0.A"), "approved").unwrap();
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.A"), Some(Status::Done));
    assert_eq!(w.next_ids(), ["M0.2", "M0.3"]);
    // `owner: lead` tasks are still the lead's alone.
    assert_eq!(ids(&w, "agent-3"), ["M0.2"]);
    assert_eq!(ids(&w, "agent-1"), ["M0.2", "M0.3"]);
    assert!(w.board().claim("M0.3", "agent-3").unwrap_err().contains("belongs to the lead"));
    w.board().claim("M0.3", "agent-1").unwrap();
    assert_eq!(w.status_of("M0.3"), Some(Status::InProgress));

    // M1.1 waits for M0.R next, even once M0.R is Ready; closing it frees M1.1.
    w.finish("M0.2");
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.R"), Some(Status::Ready));
    assert!(!w.next_ids().contains(&"M1.1".to_string()));
    assert!(w.board().claim("M1.1", "agent-2").unwrap_err().contains("comes after M0.R"));
    w.gh.close_issue(w.issue_of("M0.R"), "approved").unwrap();
    w.board().sync().unwrap();
    assert_eq!(w.next_ids(), ["M1.1"]);
}

#[test]
fn merge_and_submit_refuse_review_tasks() {
    let w = World::new(REVIEWS);
    w.board().sync().unwrap();
    // Even with the review's item forced to In Progress or In Review and a pull request open for
    // its branch, the board neither submits nor merges it.
    let n = w.issue_of("M0.A");
    let item = w.gh.lock().items.iter().find(|i| i.issue == n).unwrap().id.clone();
    w.gh.set_status(&item, Status::InProgress).unwrap();
    w.gh.open_pr("task/M0.A-checkpoint", "M0.A: check-in", "body").unwrap();
    let marker = ci_marker(&w.git.common_dir_path(), "0123abcd");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();
    let dir = w.git.dir.clone();
    std::fs::create_dir_all(dir.join("log")).unwrap();
    std::fs::write(dir.join("log/devlog.md"), "## 2026-10-10 12:00 · agent-1 · M0.A Checkpoint\nLanded: x\n").unwrap();
    let before = w.gh.mutations();
    let err = w.board().submit("M0.A", &dir, None).unwrap_err();
    assert!(err.contains("M0.A is a review task") && err.contains("never submits"), "{err}");
    w.gh.set_status(&item, Status::InReview).unwrap();
    let before_merge = w.gh.mutations();
    assert_eq!(before_merge, before + 1);
    for id in ["M0.A", "M0.R"] {
        let err = w.board().merge(id).unwrap_err();
        assert!(err.contains(&format!("{id} is a review task")) && err.contains("never merges"), "{err}");
    }
    assert_eq!(w.gh.mutations(), before_merge);
    assert!(w.git.calls.lock().unwrap().is_empty(), "nothing pushed, rebased or built");
    assert!(w.gh.lock().merged_prs.is_empty());
    assert!(w.gh.lock().issues.iter().all(|i| i.open));
}

#[test]
fn sync_never_closes_an_open_review_issue() {
    let mut w = World::new(REVIEWS);
    w.board().sync().unwrap();
    w.finish("M0.1");
    w.board().sync().unwrap();
    // The review is ✅ in roadmap.md and even has an `M0.A:` commit on main, but its issue is open:
    // sync refuses (the maintainer closes it) and writes nothing.
    w.finish("M0.A");
    let n = w.issue_of("M0.A");
    let before = w.gh.mutations();
    let err = w.board().sync().unwrap_err();
    assert!(err.contains(&format!("M0.A is ✅ in roadmap.md but its review issue #{n} is still open")), "{err}");
    assert_eq!(w.gh.mutations(), before);
    assert!(w.gh.lock().issues.iter().find(|i| i.number == n).unwrap().open);
    // Once the maintainer has closed it, sync marks it Done and closes nothing else.
    w.gh.close_issue(n, "approved").unwrap();
    w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.A"), Some(Status::Done));
    let open: Vec<u64> = w.gh.lock().issues.iter().filter(|i| i.open).map(|i| i.number).collect();
    assert_eq!(open.len(), 4, "only M0.1 and M0.A are closed: {open:?}");
}

#[test]
fn stale_claims_are_marked_on_a_virtual_clock() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.board().claim("M0.1", "agent-2").unwrap();
    w.git.activity.lock().unwrap().push(("task/M0.1-skeleton".into(), 1_000_000));
    *w.clock.now.lock().unwrap() = 1_000_000 + 2 * 3600;
    let out = w.board().status().unwrap();
    assert!(out.contains("M0.1") && !out.contains("STALE"), "{out}");
    assert!(!w.gh.lock().issues[0].labels.contains(&"stale".to_string()));

    *w.clock.now.lock().unwrap() = 1_000_000 + 3 * 3600 + 1;
    let out = w.board().status().unwrap();
    assert!(out.contains("STALE"), "{out}");
    assert!(w.gh.lock().issues[0].labels.contains(&"stale".to_string()));

    // A fresh push clears it.
    w.git.activity.lock().unwrap().push(("task/M0.1-skeleton".into(), 1_000_000 + 3 * 3600));
    w.board().status().unwrap();
    assert!(!w.gh.lock().issues[0].labels.contains(&"stale".to_string()));
}

#[test]
fn setup_is_idempotent() {
    let w = World::new(ROADMAP);
    w.gh.lock().fields.text_fields.clear();
    w.gh.lock().milestones.clear();
    let changes = w.board().setup().unwrap().changes;
    assert_eq!(changes.len(), 5 + 2 + 2, "{changes:?}");
    assert!(changes.contains(&"created label baseline-approved".to_string()), "{changes:?}");
    assert!(w.gh.lock().milestones.iter().any(|m| m.title == "M1 Star" && m.description == "star a photo"));
    let before = w.gh.mutations();
    let again = w.board().setup().unwrap();
    assert!(again.changes.is_empty() && again.notes.is_empty(), "{again:?}");
    assert_eq!(w.gh.mutations(), before);
    w.gh.lock().fields.status_options.retain(|o| o != "Blocked");
    assert!(w.board().setup().unwrap_err().contains("Blocked"));
}

#[test]
fn agent_environment() {
    let main = Path::new("/src/dalica");
    let out = super::env_lines("agent-3", main, Path::new("/tmp/scratch")).unwrap();
    // Paths are joined with the platform separator.
    let worktree = Path::new("/src").join("dalica.worktrees").join("agent-3");
    assert!(out.contains(&format!("export DALICA_WORKTREE='{}'", worktree.display())), "{out}");
    assert!(out.contains(&format!("export CARGO_TARGET_DIR='{}'", main.join("target").join("agent-3").display())), "{out}");
    assert!(out.contains("export CARGO_BUILD_JOBS=3"));
    assert!(out.contains("export DALICA_CONTROL_PORT=18030"));
    assert!(super::env_lines("agent-0", Path::new("/x"), Path::new("/y")).is_err());
    assert!(super::env_lines("bob", Path::new("/x"), Path::new("/y")).is_err());
}

impl FakeGit {
    fn common_dir_path(&self) -> std::path::PathBuf {
        use super::git::Git;
        self.common_dir()
    }
}

#[test]
fn sync_waits_for_an_auto_added_item_the_item_list_does_not_show_yet() {
    // Live failure (M1.R, 2026-10-10): GitHub refused to add #44 again ("Content already exists"),
    // yet the item list read straight after did not list it; a moment later it did.
    let w = World::new(ROADMAP);
    {
        let mut s = w.gh.lock();
        s.auto_add = true;
        s.item_list_lag = 2;
    }
    let report = w.board().sync().unwrap();
    assert_eq!(w.status_of("M0.1"), Some(Status::Ready), "{:?}", report.changes);
    let (items, issues) = {
        let s = w.gh.lock();
        (s.items.len(), s.issues.len())
    };
    assert_eq!(items, issues);
    // A lag longer than the wait still fails with a clear error, never a duplicate item.
    let w = World::new(ROADMAP);
    {
        let mut s = w.gh.lock();
        s.auto_add = true;
        s.item_list_lag = 100;
    }
    let e = w.board().sync().unwrap_err();
    assert!(e.contains("not in the project's item list after 6 reads"), "{e}");
}

#[test]
fn sync_adopts_an_issue_the_project_auto_added() {
    // The project's auto-add workflow puts each new issue on the board as it is created, so
    // adding it again fails; sync uses the item that is already there.
    let w = World::new(ROADMAP);
    w.gh.lock().auto_add = true;
    let report = w.board().sync().unwrap();
    assert!(!report.changes.iter().any(|c| c.contains("added to the project")), "{:?}", report.changes);
    let (items, issues) = {
        let s = w.gh.lock();
        (s.items.len(), s.issues.len())
    };
    assert_eq!(items, issues);
    assert_eq!(w.status_of("M0.1"), Some(Status::Ready));
    assert_eq!(w.status_of("M0.2"), Some(Status::Todo));
    let before = w.gh.mutations();
    assert!(w.board().sync().unwrap().changes.is_empty());
    assert_eq!(w.gh.mutations(), before);
}

impl World {
    /// Claims `id` for `agent`, records a green `ci` on the fake HEAD and writes `log` as the dev log.
    fn ready_to_submit(&self, id: &str, agent: &str, log: &str) -> std::path::PathBuf {
        self.board().claim(id, agent).unwrap();
        let marker = ci_marker(&self.git.common_dir_path(), "0123abcd");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let dir = self.git.dir.clone();
        std::fs::create_dir_all(dir.join("log")).unwrap();
        std::fs::write(dir.join("log/devlog.md"), log).unwrap();
        dir
    }

    fn pr_body(&self) -> String {
        self.gh.lock().pr_texts.last().map(|(_, b)| b.clone()).unwrap_or_default()
    }
}

const LOG: &str = "# Dev log\n\n## 2026-10-10 11:00 · agent-2 · M0.1 Skeleton\nLanded: an early try.\n\n\
                   ## 2026-10-10 12:00 · agent-2 · M0.1 Skeleton\nLanded: the workspace.\nOpen: nothing.\nNumbers: ci 76 s.\n\n\
                   ## 2026-10-10 12:05 · agent-3 · M0.2 Layers\nLanded: other work.\n";

#[test]
fn submit_without_a_body_file_uses_the_newest_dev_log_entry() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    w.board().submit("M0.1", &dir, None).unwrap();
    let body = w.pr_body();
    assert!(
        body.contains(
            "### From the dev log (2026-10-10 12:00 · agent-2 · M0.1 Skeleton)\n\nLanded: the workspace.\nOpen: nothing.\nNumbers: ci 76 s."
        ),
        "{body}"
    );
    assert!(!body.contains("an early try") && !body.contains("other work"), "{body}");
    assert!(body.ends_with("Closes #1\n"), "{body}");
    assert!(!body.contains("Fingerprint baselines"), "{body}");
}

#[test]
fn submit_with_a_body_file_uses_it_and_lists_new_baselines() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    *w.git.changed.lock().unwrap() = vec![
        "crates/ui/src/lib.rs".into(),
        "crates/ui/tests/fingerprints/library-small.json".into(),
        "tests/acceptance/fingerprints/m2-star.json".into(),
    ];
    w.board().submit("M0.1", &dir, Some("Evidence: the gallery test passes.\n")).unwrap();
    let body = w.pr_body();
    assert!(body.contains("Evidence: the gallery test passes."), "{body}");
    assert!(!body.contains("From the dev log"), "--body-file replaces the dev-log default: {body}");
    assert!(body.contains("## Fingerprint baselines"), "{body}");
    assert!(body.contains("- `crates/ui/tests/fingerprints/library-small.json`\n- `tests/acceptance/fingerprints/m2-star.json`\n"), "{body}");
    assert!(!body.contains("`crates/ui/src/lib.rs`"), "{body}");
    assert!(body.contains("`baseline-approved`"), "{body}");
}

#[test]
fn merge_waits_for_the_baseline_approved_label() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    *w.git.changed.lock().unwrap() = vec!["crates/ui-kit/tests/fingerprints/gallery.json".into()];
    w.board().submit("M0.1", &dir, None).unwrap();
    w.git.calls.lock().unwrap().clear();

    let before = w.gh.mutations();
    let err = w.board().merge("M0.1").unwrap_err();
    assert!(err.contains("PR #100 adds or changes fingerprint baselines (crates/ui-kit/tests/fingerprints/gallery.json)"), "{err}");
    assert!(err.contains("`baseline-approved` label"), "{err}");
    // Refused before ci, the push and the squash.
    assert_eq!(*w.git.calls.lock().unwrap(), ["rebase task/M0.1-skeleton"]);
    assert_eq!(w.gh.mutations(), before);
    assert!(w.gh.lock().merged_prs.is_empty());

    // Another label is not an approval; the maintainer's label is.
    w.gh.lock().prs[0].1.labels = vec!["task".into()];
    assert!(w.board().merge("M0.1").is_err());
    w.gh.lock().prs[0].1.labels.push("baseline-approved".into());
    assert_eq!(w.board().merge("M0.1").unwrap(), "M0.1: Skeleton (#100)");
    assert_eq!(w.gh.lock().merged_prs.len(), 1);
}

#[test]
fn merge_without_fingerprints_needs_no_label() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    *w.git.changed.lock().unwrap() = vec!["crates/scenes/src/fingerprint.rs".into()];
    w.board().submit("M0.1", &dir, None).unwrap();
    w.git.calls.lock().unwrap().clear();
    assert_eq!(w.board().merge("M0.1").unwrap(), "M0.1: Skeleton (#100)");
    assert_eq!(*w.git.calls.lock().unwrap(), ["rebase task/M0.1-skeleton", "ci", "push task/M0.1-skeleton"]);
}

#[test]
fn dry_runs_of_submit_and_merge_write_nothing() {
    use super::dryrun::{DryGit, DryRun, Recorder};
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    *w.git.changed.lock().unwrap() = vec!["crates/ui/tests/fingerprints/library-small.json".into()];
    let before = w.gh.mutations();

    let rec = Recorder::default();
    let (gh, git) = (DryRun::new(&w.gh, &rec), DryGit::new(&w.git, &rec));
    let dry = Board { roadmap: &w.roadmap, gh: &gh, git: &git, clock: &w.clock };
    assert_eq!(dry.submit("M0.1", &dir, None).unwrap(), "(dry run)");
    let writes = rec.writes();
    assert_eq!(writes.len(), 4, "{writes:#?}");
    assert_eq!(writes[0], "push task/M0.1-skeleton to origin");
    assert!(writes[1].starts_with("open pull request `M0.1: Skeleton` from task/M0.1-skeleton, body:"), "{}", writes[1]);
    assert!(writes[1].contains("| ### From the dev log (2026-10-10 12:00 · agent-2 · M0.1 Skeleton)"), "{}", writes[1]);
    assert!(writes[1].contains("| - `crates/ui/tests/fingerprints/library-small.json`"), "{}", writes[1]);
    assert!(writes[2].ends_with("Status = In Review"), "{}", writes[2]);
    assert!(writes[3].starts_with("comment on #1: submitted for review"), "{}", writes[3]);
    assert_eq!(w.gh.mutations(), before, "the board is untouched");
    assert!(w.git.calls.lock().unwrap().is_empty(), "nothing pushed");
    assert_eq!(w.status_of("M0.1"), Some(Status::InProgress));

    // Merge needs In Review; submit for real, then dry-run the merge.
    w.board().submit("M0.1", &dir, None).unwrap();
    w.git.calls.lock().unwrap().clear();
    let before = w.gh.mutations();
    let rec = Recorder::default();
    let (gh, git) = (DryRun::new(&w.gh, &rec), DryGit::new(&w.git, &rec));
    let dry = Board { roadmap: &w.roadmap, gh: &gh, git: &git, clock: &w.clock };
    let err = dry.merge("M0.1").unwrap_err();
    assert!(err.contains("baseline-approved"), "the dry run reports the missing approval: {err}");
    w.gh.lock().prs[0].1.labels.push("baseline-approved".into());
    assert_eq!(dry.merge("M0.1").unwrap(), "M0.1: Skeleton (#100)");
    assert_eq!(
        rec.writes(),
        [
            "rebase origin/task/M0.1-skeleton onto origin/main in the merge worktree",
            "rebase origin/task/M0.1-skeleton onto origin/main in the merge worktree",
            "run `cargo xtask ci` in the merge worktree",
            "push task/M0.1-skeleton to origin",
            "squash-merge #100 as `M0.1: Skeleton (#100)`",
        ]
    );
    assert_eq!(w.gh.mutations(), before);
    assert!(w.git.calls.lock().unwrap().is_empty());
    assert!(w.gh.lock().merged_prs.is_empty());
}

#[test]
fn prune_skips_agents_the_board_says_are_working() {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.board().claim("M0.1", "agent-2").unwrap();
    let target = w.git.dir.join("target");
    for agent in ["agent-2", "agent-3"] {
        let debug = target.join(agent).join("debug");
        std::fs::create_dir_all(debug.join(".fingerprint")).unwrap();
        std::fs::create_dir_all(debug.join("incremental").join("x")).unwrap();
        std::fs::write(debug.join("incremental").join("x").join("y.bin"), [0u8; 64]).unwrap();
    }
    let before = w.gh.mutations();
    let report = w.board().prune(&target, false).unwrap();
    assert_eq!(report.lines, ["agent-2: skipped, M0.1 is In Progress", "agent-3: removed target/agent-3/debug/incremental (64 B)"]);
    assert_eq!(report.freed, 64);
    assert!(target.join("agent-2").join("debug").join("incremental").is_dir());
    assert!(!target.join("agent-3").join("debug").join("incremental").exists());
    assert_eq!(w.gh.mutations(), before, "prune only reads the board");
}

/// Submits M0.1 for real and clears the git call log, ready for `merge`.
fn in_review() -> World {
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    let dir = w.ready_to_submit("M0.1", "agent-2", LOG);
    w.board().submit("M0.1", &dir, None).unwrap();
    w.git.calls.lock().unwrap().clear();
    w
}

const NOT_MERGEABLE_REPLY: &str = "gh pr failed: GraphQL: Pull Request is not mergeable (mergePullRequest)";

#[test]
fn merge_waits_until_github_finds_the_pushed_pr_mergeable() {
    // Live failure (M2.6, M2.9, M2.3, M2.8 on 2026-10-10): merge squashed straight after its push,
    // before GitHub had recomputed mergeability, and GitHub refused; a retry later always worked.
    let w = in_review();
    w.gh.lock().mergeable = vec![MergeState::Unknown, MergeState::Unknown, MergeState::Mergeable];
    let start = *w.clock.now.lock().unwrap();
    assert_eq!(w.board().merge("M0.1").unwrap(), "M0.1: Skeleton (#100)");
    assert_eq!(w.gh.lock().mergeable_reads, 3);
    assert_eq!(*w.clock.now.lock().unwrap() - start, 2 + 3, "read at once, then after each pause");
    assert_eq!(*w.git.calls.lock().unwrap(), ["rebase task/M0.1-skeleton", "ci", "push task/M0.1-skeleton"]);
    assert_eq!(w.gh.lock().merged_prs.len(), 1);
}

#[test]
fn merge_refuses_a_conflicting_pr_without_squashing() {
    let w = in_review();
    w.gh.lock().mergeable = vec![MergeState::Unknown, MergeState::Conflicting, MergeState::Mergeable];
    let before = w.gh.mutations();
    let err = w.board().merge("M0.1").unwrap_err();
    assert!(err.contains("PR #100 conflicts with main") && err.contains("nothing was merged"), "{err}");
    assert_eq!(w.gh.lock().mergeable_reads, 2, "a conflict is final, not polled");
    assert!(w.gh.lock().merged_prs.is_empty());
    assert_eq!(w.gh.mutations(), before);
}

#[test]
fn merge_gives_up_when_github_never_decides() {
    let w = in_review();
    w.gh.lock().mergeable = vec![MergeState::Unknown; 50];
    let start = *w.clock.now.lock().unwrap();
    let err = w.board().merge("M0.1").unwrap_err();
    assert!(err.contains("PR #100 is mergeable after 120 s") && err.contains("tasks merge M0.1` again"), "{err}");
    assert_eq!(w.gh.lock().mergeable_reads, 9, "bounded");
    assert_eq!(*w.clock.now.lock().unwrap() - start, 120);
    assert!(w.gh.lock().merged_prs.is_empty());
}

#[test]
fn merge_retries_the_squash_while_github_says_not_mergeable() {
    let w = in_review();
    w.gh.lock().squash_errors = vec![NOT_MERGEABLE_REPLY.into(), NOT_MERGEABLE_REPLY.into()];
    let start = *w.clock.now.lock().unwrap();
    assert_eq!(w.board().merge("M0.1").unwrap(), "M0.1: Skeleton (#100)");
    assert_eq!(w.gh.lock().merged_prs.len(), 1);
    assert_eq!(*w.clock.now.lock().unwrap() - start, 2 * 20);

    // Bounded: GitHub refusing every time is an error after three tries, and nothing is merged.
    let w = in_review();
    w.gh.lock().squash_errors = vec![NOT_MERGEABLE_REPLY.into(); 10];
    let err = w.board().merge("M0.1").unwrap_err();
    assert!(err.contains("refused to merge PR #100 after 3 tries") && err.contains("not mergeable"), "{err}");
    assert_eq!(w.gh.lock().squash_errors.len(), 7);
    assert!(w.gh.lock().merged_prs.is_empty());

    // Any other error is not retried.
    let w = in_review();
    w.gh.lock().squash_errors = vec!["gh pr failed: HTTP 401".into(), NOT_MERGEABLE_REPLY.into()];
    assert_eq!(w.board().merge("M0.1").unwrap_err(), "gh pr failed: HTTP 401");
    assert_eq!(w.gh.lock().squash_errors.len(), 1);
}

#[test]
fn the_sync_after_a_merge_reads_the_roadmap_as_merged() {
    // Live failure (every M2 merge, 2026-10-10): the post-merge sync read the main checkout's
    // roadmap.md, not pulled since the squash, and refused: "issue #N is closed but the task is not ✅".
    let mut w = in_review();
    // origin/main after the squash: the branch ticked M0.1 ✅, and its `M0.1:` commit is there.
    let ticked = w.text.replace("- ⬜ **M0.1 ", "- ✅ **M0.1 ");
    *w.git.origin_roadmap.lock().unwrap() = Some(ticked.clone());
    w.board().merge("M0.1").unwrap();
    w.git.merged.lock().unwrap().insert("M0.1".into());
    assert!(!w.gh.lock().issues[0].open, "GitHub closed the issue (`Closes #1`)");

    // The local roadmap still says ⬜: a plain sync refuses, as it did live.
    let err = w.board().sync().unwrap_err();
    assert!(err.contains("issue #1 (M0.1) is closed but the task is not ✅"), "{err}");
    // The post-merge sync fetches and reads origin/main's roadmap.md instead.
    w.git.calls.lock().unwrap().clear();
    let report = w.board().sync_merged().unwrap();
    assert_eq!(*w.git.calls.lock().unwrap(), ["fetch"]);
    assert!(report.changes.iter().any(|c| c.contains("M0.1: In Review -> Done")), "{:?}", report.changes);
    assert_eq!(w.status_of("M0.1"), Some(Status::Done));
    assert_eq!(w.status_of("M0.2"), Some(Status::Ready));
    // Once the checkout is pulled, a plain sync agrees and has nothing left to do.
    w.text = ticked;
    w.roadmap = roadmap::parse(&w.text).unwrap();
    assert!(w.board().sync().unwrap().changes.is_empty());
}

/// The board's milestones after the roadmap renamed and dropped some (2026-10-10).
fn renamed_milestones(w: &World) {
    let m = |number, title: &str, description: &str, open, issues| MilestoneInfo {
        number,
        title: title.into(),
        description: description.into(),
        open,
        issues,
    };
    let mut s = w.gh.lock();
    s.labels = super::config::LABELS.iter().map(|(n, _, _)| n.to_string()).collect();
    s.milestones = vec![
        m(1, "M0 Skeleton", "an empty workspace", true, 3),
        m(2, "M1 Folder tree", "the old M1 goal", true, 0),
        m(3, "M2 Old middle", "x", true, 0),
        m(4, "M3 Busy", "y", true, 2),
        m(5, "M4 Retired", "", false, 0),
    ];
}

#[test]
fn setup_renames_milestones_and_closes_empty_ones_the_roadmap_dropped() {
    // Live failure (2026-10-10): setup matched milestones by number only, said "nothing to change"
    // after the roadmap renamed them, and sync then failed to create issues in `M4 Library browsing`.
    let w = World::new(ROADMAP);
    renamed_milestones(&w);
    let report = w.board().setup().unwrap();
    assert_eq!(
        report.changes,
        ["renamed milestone `M1 Folder tree` to `M1 Star`", "closed milestone `M2 Old middle` (not in roadmap.md, no issues)"]
    );
    assert_eq!(
        report.notes,
        [
            "milestone `M3 Busy` is not in roadmap.md but holds 2 issue(s); left as it is (once they move, setup closes it)",
            "1 closed milestone(s) with no issues are not in roadmap.md; left as they are",
        ]
    );
    {
        let s = w.gh.lock();
        let titles: Vec<(&str, &str, bool)> = s.milestones.iter().map(|m| (m.title.as_str(), m.description.as_str(), m.open)).collect();
        assert_eq!(
            titles,
            [
                ("M0 Skeleton", "an empty workspace", true),
                ("M1 Star", "star a photo", true),
                ("M2 Old middle", "x", false),
                ("M3 Busy", "y", true),
                ("M4 Retired", "", false),
            ]
        );
    }
    let before = w.gh.mutations();
    assert!(w.board().setup().unwrap().changes.is_empty());
    assert_eq!(w.gh.mutations(), before);

    // A changed goal updates the description of a milestone whose title already matches.
    w.gh.lock().milestones[0].description = "an older goal".into();
    assert_eq!(w.board().setup().unwrap().changes, ["updated the description of milestone M0 Skeleton"]);
    assert_eq!(w.gh.lock().milestones[0].description, "an empty workspace");
}

#[test]
fn setup_dry_run_lists_the_milestone_writes_and_makes_none() {
    use super::dryrun::{DryGit, DryRun, Recorder};
    let w = World::new(ROADMAP);
    renamed_milestones(&w);
    let before = w.gh.mutations();
    let rec = Recorder::default();
    let (gh, git) = (DryRun::new(&w.gh, &rec), DryGit::new(&w.git, &rec));
    let dry = Board { roadmap: &w.roadmap, gh: &gh, git: &git, clock: &w.clock };
    dry.setup().unwrap();
    assert_eq!(rec.writes(), ["edit milestone 2: title `M1 Star`, description `star a photo`", "close milestone 3"]);
    assert_eq!(w.gh.mutations(), before);
    assert_eq!(w.gh.lock().milestones[1].title, "M1 Folder tree");
    assert!(w.gh.lock().milestones[2].open);
}

#[test]
fn sync_names_setup_when_a_milestone_is_missing() {
    let w = World::new(ROADMAP);
    w.gh.lock().milestones.retain(|m| m.title != "M1 Star");
    let err = w.board().sync().unwrap_err();
    assert!(err.contains("GitHub has no milestone `M1 Star`") && err.contains("run `cargo xtask tasks setup`"), "{err}");
    assert_eq!(w.gh.mutations(), 0, "refused before any write");
    // Issues already in their milestone need no read of the milestones at all.
    let w = World::new(ROADMAP);
    w.board().sync().unwrap();
    w.gh.lock().milestones.clear();
    assert!(w.board().sync().unwrap().changes.is_empty());
}
