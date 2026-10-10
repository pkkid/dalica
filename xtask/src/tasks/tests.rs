//! Board tests on the in-memory fakes (`docs/task-board.md` §9).

use std::path::Path;
use std::sync::{Arc, Barrier, Mutex};

use super::board::{Board, ci_marker};
use super::fake::{FakeClock, FakeGit, FakeGitHub};
use super::github::{GitHub, Status};
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
        World {
            roadmap: roadmap::parse(text).unwrap(),
            gh: FakeGitHub::with_setup(),
            git: FakeGit::new(dir.path()),
            clock: FakeClock { now: Mutex::new(1_000_000) },
            _dir: dir,
        }
    }

    fn board(&self) -> Board<'_> {
        Board { roadmap: &self.roadmap, gh: &self.gh, git: &self.git, clock: &self.clock }
    }

    /// Ticks a task ✅ in the roadmap (as a merged commit would) and records the merge on main.
    fn finish(&mut self, id: &str) {
        let text = ROADMAP.replace(&format!("- ⬜ **{id} "), &format!("- ✅ **{id} "));
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
        self.board().next().unwrap().into_iter().map(|t| t.id.clone()).collect()
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
    // In review, its files still block overlapping work; merge squashes with the task id.
    let subject = w.board().merge("M0.1").unwrap();
    assert_eq!(subject, "M0.1: Skeleton (#100)");
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
    let changes = w.board().setup().unwrap();
    assert_eq!(changes.len(), 4 + 2 + 2, "{changes:?}");
    assert!(w.gh.lock().milestones.iter().any(|m| m.title == "M1 Star" && m.description == "star a photo"));
    let before = w.gh.mutations();
    assert!(w.board().setup().unwrap().is_empty());
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
