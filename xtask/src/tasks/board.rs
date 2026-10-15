//! Board logic shared by every `cargo xtask tasks` command, written against the [`GitHub`], [`Git`]
//! and [`Clock`] traits so tests run it on fakes (`docs/task-board.md` §4–§5).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::body;
use super::config::{LABEL_BLOCKED, LABEL_GATE, LABEL_STALE, LABEL_TASK, LABELS, LEAD, MAINTAINER, STALE_SECS, is_fingerprint};
use super::devlog;
use super::git::{ChangeKind, Clock, Git, PathChange};
use super::github::{ALREADY_IN_PROJECT, GitHub, Issue, IssueEdit, Item, MergeState, MilestoneInfo, NOT_MERGEABLE, NewIssue, Status, TextField};
use super::glob::lists_overlap;
use super::prune;
use super::roadmap::{self, Owner, Roadmap, Task};

/// Reads of the project's item list while waiting for an auto-added item, and the pause between them.
const ITEM_WAIT_TRIES: usize = 6;
const ITEM_WAIT_SECS: u64 = 2;

/// Pauses between reads of a pull request's mergeability after `merge` pushed it (120 s in all,
/// 9 reads). GitHub recomputes it after a push; a squash before that fails with "not mergeable".
const MERGEABLE_WAITS: [u64; 8] = [2, 3, 5, 10, 15, 25, 30, 30];
/// Squash attempts while GitHub still answers [`NOT_MERGEABLE`], and the pause between them.
const SQUASH_TRIES: usize = 3;
const SQUASH_RETRY_SECS: u64 = 20;

pub struct Board<'a> {
    pub roadmap: &'a Roadmap,
    pub gh: &'a dyn GitHub,
    pub git: &'a dyn Git,
    pub clock: &'a dyn Clock,
}

/// One read of everything the rules look at.
pub struct Snapshot {
    pub issues: Vec<Issue>,
    pub items: Vec<Item>,
    pub merged: BTreeSet<String>,
}

impl Snapshot {
    /// The issue of a task: its title is the task id, optionally followed by a space and more.
    pub fn issue(&self, task: &Task) -> Option<&Issue> {
        let prefix = format!("{} ", task.id);
        self.issues.iter().filter(|i| i.title == task.id || i.title.starts_with(&prefix)).min_by_key(|i| i.number)
    }

    pub fn item(&self, issue: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.issue == issue)
    }

    pub fn status(&self, task: &Task) -> Option<Status> {
        self.issue(task).and_then(|i| self.item(i.number)).and_then(|i| i.status)
    }

    /// ✅ in the roadmap, or a gate the maintainer has closed.
    pub fn done(&self, task: &Task) -> bool {
        task.done || (task.is_gate() && self.issue(task).is_some_and(|i| !i.open))
    }

    fn issue_numbers(&self, roadmap: &Roadmap) -> BTreeMap<String, u64> {
        roadmap.tasks.iter().filter_map(|t| self.issue(t).map(|i| (t.id.clone(), i.number))).collect()
    }
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub changes: Vec<String>,
    /// What sync saw but left alone: open issues whose task is no longer in `roadmap.md`.
    pub notes: Vec<String>,
}

/// What `setup` changed, and what it saw but left alone.
#[derive(Debug, Default)]
pub struct SetupReport {
    pub changes: Vec<String>,
    pub notes: Vec<String>,
}

/// The roadmap milestone number a GitHub milestone title starts with: `M4 Library browsing` → 4.
fn milestone_number(title: &str) -> Option<u32> {
    let rest = title.strip_prefix('M')?;
    let digits = rest.split(' ').next()?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub id: String,
    pub branch: String,
    pub worktree: PathBuf,
}

impl Board<'_> {
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot { issues: self.gh.issues()?, items: self.gh.items()?, merged: self.git.merged_task_ids()? })
    }

    fn task(&self, id: &str) -> Result<&Task, String> {
        self.roadmap.task(id).ok_or_else(|| format!("{id} is not a task in docs/roadmap.md"))
    }

    fn deps_done(&self, snap: &Snapshot, task: &Task) -> bool {
        task.after.iter().all(|d| self.roadmap.task(d).is_some_and(|t| snap.done(t)))
    }

    /// The first open review task the maintainer owns that is listed before `task` in `roadmap.md`.
    /// Every task after an open review waits until the maintainer closes its issue (or it is ✅),
    /// whatever its milestone: an open `M2.R` holds back every M3 task and everything after it.
    /// A task pulled forward into milestone k (`pulled: M<k>`) waits only for reviews before M<k>.
    fn open_gate_before<'r>(&'r self, snap: &Snapshot, task: &Task) -> Option<&'r Task> {
        self.roadmap
            .tasks
            .iter()
            .take_while(|t| t.id != task.id)
            .filter(|t| task.pulled.is_none_or(|k| t.milestone < k))
            .find(|t| t.is_gate() && t.owner == Owner::Maintainer && !snap.done(t))
    }

    /// The review that opens milestone `number`: the previous milestone's `M<n-1>.R` (`None` for the
    /// first milestone, which is always open).
    fn opening_review(&self, number: u32) -> Option<String> {
        let prev = self.roadmap.milestones.iter().map(|m| m.number).filter(|&n| n < number).max()?;
        Some(format!("M{prev}.R"))
    }

    /// A milestone is open once the maintainer has closed the previous milestone's review (or it
    /// is ✅); the first is always open (maintainer rule, 2026-10-10: no milestone's tasks on the
    /// board before the review of the one before it).
    pub fn milestone_open(&self, snap: &Snapshot, number: u32) -> bool {
        self.opening_review(number).is_none_or(|id| self.roadmap.task(&id).is_some_and(|r| snap.done(r)))
    }

    /// Whether `task` is on the board, i.e. has (or gets) an issue: its milestone is open, or it is
    /// pulled forward into an open milestone. Other blocks are planning text in `roadmap.md`.
    pub fn listed(&self, snap: &Snapshot, task: &Task) -> bool {
        self.milestone_open(snap, task.milestone) || task.pulled.is_some_and(|k| self.milestone_open(snap, k))
    }

    /// Review tasks are stops only the maintainer merges or closes; the board tooling never does.
    fn refuse_gate(task: &Task, what: &str) -> Result<(), String> {
        if task.is_gate() {
            return Err(format!(
                "{} is a review task: the board never {what} one; the maintainer merges its pull request and closes its issue",
                task.id
            ));
        }
        Ok(())
    }

    /// Where `roadmap.md`, the issues and `main` disagree; `sync` refuses to touch the board then.
    pub fn disagreements(&self, snap: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        for task in self.roadmap.tasks.iter().filter(|t| self.listed(snap, t)) {
            let Some(issue) = snap.issue(task) else {
                if task.done && !task.is_gate() && !snap.merged.contains(&task.id) {
                    out.push(format!("{} is ✅ in roadmap.md but no `{}:` commit is on main", task.id, task.id));
                }
                continue;
            };
            if task.is_gate() {
                if task.done && issue.open {
                    out.push(format!(
                        "{} is ✅ in roadmap.md but its review issue #{} is still open (the maintainer closes it)",
                        task.id, issue.number
                    ));
                }
            } else if task.done && issue.open && !snap.merged.contains(&task.id) {
                out.push(format!("{} is ✅ in roadmap.md but its issue #{} is open and no `{}:` commit is on main", task.id, issue.number, task.id));
            } else if !task.done && !issue.open {
                out.push(format!("issue #{} ({}) is closed but the task is not ✅ in roadmap.md", issue.number, task.id));
            }
        }
        out
    }

    /// The status `sync` wants for a task, given its current one.
    fn wanted_status(&self, snap: &Snapshot, task: &Task, issue: Option<&Issue>, current: Option<Status>) -> Status {
        if snap.done(task) {
            return Status::Done;
        }
        match current {
            Some(s @ (Status::InProgress | Status::InReview)) => s,
            Some(Status::Blocked) if issue.is_some_and(|i| i.has_label(LABEL_BLOCKED)) => Status::Blocked,
            _ if self.deps_done(snap, task) => Status::Ready,
            _ => Status::Todo,
        }
    }

    /// `tasks sync`: issues, items, statuses and board order made to match the roadmap.
    pub fn sync(&self) -> Result<SyncReport, String> {
        let mut snap = self.snapshot()?;
        let problems = self.disagreements(&snap);
        if !problems.is_empty() {
            return Err(format!("sync: roadmap.md, the issues and main disagree:\n  {}", problems.join("\n  ")));
        }
        self.check_milestones(&snap)?;
        let mut report = SyncReport { notes: self.orphan_notes(&snap), ..SyncReport::default() };
        report.notes.extend(self.unopened_notes(&snap));
        let listed: Vec<&Task> = self.roadmap.tasks.iter().filter(|t| self.listed(&snap, t)).collect();
        let mut numbers = snap.issue_numbers(self.roadmap);
        numbers.retain(|id, _| listed.iter().any(|t| &t.id == id));
        let mut ordered_items: Vec<String> = Vec::new();

        for task in listed {
            let section = body::render(task, &numbers);
            let milestone = self.roadmap.milestone(task.milestone).map(|m| m.title());
            let label = if task.is_gate() { LABEL_GATE } else { LABEL_TASK };
            let issue = match snap.issue(task).cloned() {
                Some(issue) => {
                    let mut edit = IssueEdit::default();
                    if issue.title != task.issue_title() {
                        edit.title = Some(task.issue_title());
                    }
                    let wanted = body::replace_section(&issue.body, &section);
                    if issue.body != wanted {
                        edit.body = Some(wanted);
                    }
                    if !issue.has_label(label) {
                        edit.add_labels.push(label.into());
                    }
                    if milestone.is_some() && issue.milestone != milestone {
                        edit.milestone.clone_from(&milestone);
                    }
                    if task.owner == Owner::Maintainer && !issue.assignees.iter().any(|a| a == MAINTAINER) {
                        edit.add_assignee = Some(MAINTAINER.into());
                    }
                    if !edit.is_empty() {
                        self.gh.edit_issue(issue.number, &edit)?;
                        report.changes.push(format!("#{} {}: updated issue", issue.number, task.id));
                    }
                    issue
                }
                None => {
                    let new = NewIssue {
                        title: task.issue_title(),
                        body: section.clone(),
                        labels: vec![label.into()],
                        milestone: milestone.clone(),
                        assignee: (task.owner == Owner::Maintainer).then(|| MAINTAINER.to_string()),
                    };
                    let number = self.gh.create_issue(&new)?;
                    report.changes.push(format!("#{number} {}: created issue", task.id));
                    numbers.insert(task.id.clone(), number);
                    let issue = Issue {
                        number,
                        title: new.title,
                        body: new.body,
                        open: true,
                        labels: new.labels,
                        assignees: new.assignee.into_iter().collect(),
                        milestone: new.milestone,
                    };
                    snap.issues.push(issue.clone());
                    issue
                }
            };

            let item = match snap.item(issue.number).cloned() {
                Some(item) => item,
                None => {
                    let item = match self.gh.add_item(issue.number) {
                        Ok(id) => {
                            report.changes.push(format!("#{} {}: added to the project", issue.number, task.id));
                            Item { id, issue: issue.number, status: None, agent: String::new(), branch: String::new() }
                        }
                        // The project's auto-add workflow put a new issue on the board when it was created;
                        // GitHub may list that item only a moment later.
                        Err(e) if e.contains(ALREADY_IN_PROJECT) => self.find_item(issue.number)?.ok_or_else(|| {
                            format!("{e}, but issue #{} is not in the project's item list after {ITEM_WAIT_TRIES} reads", issue.number)
                        })?,
                        Err(e) => return Err(e),
                    };
                    snap.items.push(item.clone());
                    item
                }
            };

            let wanted = self.wanted_status(&snap, task, Some(&issue), item.status);
            if item.status != Some(wanted) {
                self.gh.set_status(&item.id, wanted)?;
                report.changes.push(format!("#{} {}: {} -> {wanted}", issue.number, task.id, item.status.map_or("none", Status::name)));
            }
            // A review's issue is closed only by the maintainer, never here.
            if wanted == Status::Done && issue.open && !task.is_gate() {
                self.gh.close_issue(issue.number, &format!("Merged into main (`{}:` commit); closed by `cargo xtask tasks sync`.", task.id))?;
                report.changes.push(format!("#{} {}: closed (merged)", issue.number, task.id));
            }
            ordered_items.push(item.id);
        }

        // Board order = roadmap order. Items that are not tasks stay where they are, after ours.
        let mut current: Vec<String> = self.gh.items()?.into_iter().map(|i| i.id).filter(|id| ordered_items.contains(id)).collect();
        for (pos, id) in ordered_items.iter().enumerate() {
            if current.get(pos) != Some(id) {
                let after = pos.checked_sub(1).and_then(|p| ordered_items.get(p));
                self.gh.move_item(id, after.map(String::as_str))?;
                current.retain(|x| x != id);
                current.insert(pos.min(current.len()), id.clone());
                report.changes.push(format!("moved item {id} to position {}", pos + 1));
            }
        }
        Ok(report)
    }

    /// What sync leaves off the board: one line per milestone that is not open yet, and each open
    /// issue that already exists for one of its tasks (left as it is: never edited, moved or closed).
    fn unopened_notes(&self, snap: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.roadmap.milestones {
            let waiting: Vec<&Task> = self.roadmap.tasks.iter().filter(|t| t.milestone == m.number && !self.listed(snap, t)).collect();
            if waiting.is_empty() {
                continue;
            }
            let review = self.opening_review(m.number).unwrap_or_default();
            out.push(format!("M{} is not open yet: {} task block(s) get issues when the maintainer closes {review}", m.number, waiting.len()));
            for task in waiting {
                if let Some(issue) = snap
                    .issues
                    .iter()
                    .filter(|i| i.open && (i.title == task.id || i.title.starts_with(&format!("{} ", task.id))))
                    .min_by_key(|i| i.number)
                {
                    out.push(format!("issue #{} `{}` is open but M{} is not open yet; left as it is", issue.number, issue.title, m.number));
                }
            }
        }
        out
    }

    /// Open issues titled with a task id that `roadmap.md` no longer has (a dropped block, such as a
    /// removed mid-milestone review). Sync never edits, moves or closes them: a review's issue is
    /// the maintainer's to close, and a dropped task's issue is closed by whoever dropped it.
    fn orphan_notes(&self, snap: &Snapshot) -> Vec<String> {
        let mut issues: Vec<&Issue> = snap.issues.iter().filter(|i| i.open).collect();
        issues.sort_by_key(|i| i.number);
        issues
            .into_iter()
            .filter_map(|issue| {
                let id = issue.title.split(' ').next()?;
                let (_, n) = roadmap::split_id(id)?;
                if self.roadmap.task(id).is_some() {
                    return None;
                }
                let who = if n.chars().all(|c| c.is_ascii_uppercase()) {
                    "a review issue: only the maintainer closes it"
                } else {
                    "close it if the task was dropped on purpose"
                };
                Some(format!("issue #{} `{}` is open but {id} is not in roadmap.md any more; left as it is ({who})", issue.number, issue.title))
            })
            .collect()
    }

    /// The milestones `sync` would put issues in must exist on GitHub, checked before any write
    /// (GitHub is asked only when an issue is to be created or moved).
    fn check_milestones(&self, snap: &Snapshot) -> Result<(), String> {
        let wanted: BTreeSet<String> = self
            .roadmap
            .tasks
            .iter()
            .filter(|t| self.listed(snap, t))
            .filter_map(|t| {
                let title = self.roadmap.milestone(t.milestone)?.title();
                let placed = snap.issue(t).is_some_and(|i| i.milestone.as_deref() == Some(title.as_str()));
                (!placed).then_some(title)
            })
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let existing = self.gh.milestones()?;
        let missing: Vec<String> = wanted.into_iter().filter(|w| !existing.iter().any(|m| &m.title == w)).map(|m| format!("`{m}`")).collect();
        if missing.is_empty() {
            return Ok(());
        }
        Err(format!(
            "sync: GitHub has no milestone {}; run `cargo xtask tasks setup` (it creates and renames milestones to match roadmap.md), then sync again",
            missing.join(", ")
        ))
    }

    /// The `sync` that follows a merge, against `docs/roadmap.md` as merged on `origin/main`: the
    /// main checkout may not be pulled yet (its copy lacks the task's ✅), and it is never touched.
    pub fn sync_merged(&self) -> Result<SyncReport, String> {
        let text = self.git.merged_roadmap()?;
        let merged = roadmap::parse(&text).map_err(|e| format!("docs/roadmap.md on origin/main: {e}"))?;
        Board { roadmap: &merged, gh: self.gh, git: self.git, clock: self.clock }.sync()
    }

    /// Why `task` may not be claimed now (by `agent`, when known), or `None` if it may.
    fn refusal(&self, snap: &Snapshot, task: &Task, agent: Option<&str>) -> Option<String> {
        if task.owner == Owner::Maintainer {
            return Some(format!("{} belongs to the maintainer", task.id));
        }
        if task.owner == Owner::Lead && agent.is_some_and(|a| a != LEAD) {
            return Some(format!("{} belongs to the lead ({LEAD})", task.id));
        }
        if snap.done(task) {
            return Some(format!("{} is done", task.id));
        }
        if !self.listed(snap, task) {
            let review = self.opening_review(task.milestone).unwrap_or_default();
            return Some(format!("{} is in M{}, which is not open yet (it opens when the maintainer closes {review})", task.id, task.milestone));
        }
        let status = snap.status(task);
        if status != Some(Status::Ready) {
            return Some(format!("{} is {}, not Ready", task.id, status.map_or("not on the board", Status::name)));
        }
        if let Some(dep) = task.after.iter().find(|d| !self.roadmap.task(d).is_some_and(|t| snap.done(t))) {
            return Some(format!("{} waits for {dep}", task.id));
        }
        if let Some(gate) = self.open_gate_before(snap, task) {
            return Some(format!("{} comes after {}, a review the maintainer has not closed yet", task.id, gate.id));
        }
        for other in &self.roadmap.tasks {
            if other.id == task.id || !matches!(snap.status(other), Some(Status::InProgress | Status::InReview)) {
                continue;
            }
            if let Some((a, b)) = lists_overlap(&task.files, &other.files) {
                return Some(format!("{}'s `{a}` overlaps `{b}` of {}, which is in progress or in review", task.id, other.id));
            }
        }
        None
    }

    /// `tasks next`: the tasks this agent may claim, in roadmap order.
    pub fn next(&self, agent: Option<&str>) -> Result<Vec<&Task>, String> {
        let snap = self.snapshot()?;
        Ok(self.roadmap.tasks.iter().filter(|t| self.refusal(&snap, t, agent).is_none()).collect())
    }

    /// `tasks claim`: the caller holds the claim lock.
    pub fn claim(&self, id: &str, agent: &str) -> Result<Claim, String> {
        let task = self.task(id)?;
        let snap = self.snapshot()?;
        if let Some(why) = self.refusal(&snap, task, Some(agent)) {
            return Err(format!("cannot claim: {why}"));
        }
        let (issue, item) = self.issue_and_item(&snap, task)?;
        let branch = task.branch();
        self.gh.set_status(&item.id, Status::InProgress)?;
        self.gh.set_field(&item.id, TextField::Agent, agent)?;
        self.gh.set_field(&item.id, TextField::Branch, &branch)?;
        self.gh.comment(issue.number, &format!("claimed by {agent} on `{branch}`"))?;
        let worktree = self
            .git
            .prepare_worktree(agent, &branch)
            .map_err(|e| format!("claimed {id}, but the worktree failed: {e}\n(fix it, or `cargo xtask tasks release {id} --reason …`)"))?;
        Ok(Claim { id: id.to_string(), branch, worktree })
    }

    fn issue_and_item(&self, snap: &Snapshot, task: &Task) -> Result<(Issue, Item), String> {
        let issue = snap.issue(task).cloned().ok_or_else(|| format!("{} has no issue; run `cargo xtask tasks sync`", task.id))?;
        let item = snap.item(issue.number).cloned().ok_or_else(|| format!("issue #{} is not on the project; run sync", issue.number))?;
        Ok((issue, item))
    }

    fn require_status(&self, snap: &Snapshot, task: &Task, allowed: &[Status]) -> Result<(Issue, Item), String> {
        let (issue, item) = self.issue_and_item(snap, task)?;
        match item.status {
            Some(s) if allowed.contains(&s) => Ok((issue, item)),
            other => Err(format!("{} is {}", task.id, other.map_or("not on the board", Status::name))),
        }
    }

    /// `tasks release`: back to Ready; the branch is kept for the next claimer.
    pub fn release(&self, id: &str, by: &str, reason: &str) -> Result<(), String> {
        let task = self.task(id)?;
        let snap = self.snapshot()?;
        let (issue, item) = self.require_status(&snap, task, &[Status::InProgress, Status::Blocked])?;
        let status = if self.deps_done(&snap, task) { Status::Ready } else { Status::Todo };
        self.gh.set_status(&item.id, status)?;
        self.gh.set_field(&item.id, TextField::Agent, "")?;
        if issue.has_label(LABEL_STALE) {
            self.gh.edit_issue(issue.number, &IssueEdit { remove_labels: vec![LABEL_STALE.into()], ..IssueEdit::default() })?;
        }
        self.gh.comment(issue.number, &format!("released by {by}: {reason} (branch `{}` kept)", item.branch))
    }

    /// `tasks block`: Blocked, `needs-maintainer`, and the question as a comment.
    pub fn block(&self, id: &str, by: &str, question: &str) -> Result<(), String> {
        let task = self.task(id)?;
        let snap = self.snapshot()?;
        let (issue, item) = self.require_status(&snap, task, &[Status::InProgress])?;
        self.gh.set_status(&item.id, Status::Blocked)?;
        self.gh.edit_issue(issue.number, &IssueEdit { add_labels: vec![LABEL_BLOCKED.into()], ..IssueEdit::default() })?;
        self.gh.comment(issue.number, &format!("**Blocked ({by}), question for the maintainer:** {question}"))
    }

    /// `tasks submit`: refuses a review task, and refuses without a green `ci` on this commit and a
    /// dev-log entry; pushes, opens
    /// the pull request and sets In Review. The evidence is `body_extra` (`--body-file`) or, without
    /// it, the task's newest dev-log entry.
    ///
    /// A task already In Review is a send-back (`merge` could not rebase it): once its worker has
    /// rebased the branch, `submit` pushes it with lease and keeps the open pull request, its number
    /// and its body. The same commit as the one already pushed is refused (nothing was rebased).
    pub fn submit(&self, id: &str, dir: &Path, body_extra: Option<&str>) -> Result<String, String> {
        let task = self.task(id)?;
        Self::refuse_gate(task, "submits")?;
        let snap = self.snapshot()?;
        let (issue, item) = self.require_status(&snap, task, &[Status::InProgress, Status::InReview])?;
        let resubmit = item.status == Some(Status::InReview);
        let branch = if item.branch.is_empty() { task.branch() } else { item.branch.clone() };
        let head = self.git.head(dir)?;
        if resubmit && self.git.pushed_head(&branch).as_deref() == Some(head.as_str()) {
            return Err(format!(
                "{id} is In Review and origin already has {head} on {branch}; a send-back is submitted again after its branch is rebased on origin/main"
            ));
        }
        if !ci_passed(&self.git.common_dir(), &head) {
            return Err(format!("cargo xtask ci has not passed on {head}; run it in this worktree first"));
        }
        let log = std::fs::read_to_string(devlog::path(&self.git.main_checkout())).unwrap_or_default();
        if devlog::entry_times(&log, id, 0).is_empty() {
            return Err(format!("no dev-log entry for {id}; add one with `cargo xtask tasks log {id} \"Landed: … Open: … Numbers: …\"`"));
        }
        self.git.push(dir, &branch)?;
        let pr = match self.gh.find_pr(&branch)? {
            Some(pr) => pr,
            None => {
                let subjects = self.git.branch_subjects(&branch).unwrap_or_default();
                let evidence = match body_extra {
                    Some(extra) => extra.to_string(),
                    None => devlog_evidence(&log, id),
                };
                let baselines = fingerprints(self.git.changed_paths(&branch).unwrap_or_default());
                let body = pr_body(task, issue.number, &subjects, &evidence, &baselines);
                self.gh.open_pr(&branch, &format!("{}: {}", task.id, task.plain_title()), &body)?
            }
        };
        if resubmit {
            self.gh.comment(issue.number, &format!("submitted again after a rebase ({head}): {}", pr.url))?;
        } else {
            self.gh.set_status(&item.id, Status::InReview)?;
            self.gh.comment(issue.number, &format!("submitted for review: {}", pr.url))?;
        }
        Ok(pr.url)
    }

    /// The project item of `issue`, re-reading the item list a few times (it lags behind the
    /// project's auto-add workflow).
    fn find_item(&self, issue: u64) -> Result<Option<Item>, String> {
        for attempt in 0..ITEM_WAIT_TRIES {
            if attempt > 0 {
                self.clock.pause(ITEM_WAIT_SECS);
            }
            if let Some(item) = self.gh.items()?.into_iter().find(|i| i.issue == issue) {
                return Ok(Some(item));
            }
        }
        Ok(None)
    }

    /// `tasks merge` (lead only; the caller holds the merge lock). Never a review task: only the
    /// maintainer merges those.
    pub fn merge(&self, id: &str) -> Result<String, String> {
        let task = self.task(id)?;
        Self::refuse_gate(task, "merges")?;
        let snap = self.snapshot()?;
        let (_, item) = self.require_status(&snap, task, &[Status::InReview])?;
        let branch = if item.branch.is_empty() { task.branch() } else { item.branch.clone() };
        let pr = self.gh.find_pr(&branch)?.ok_or_else(|| format!("no open pull request for {branch}"))?;
        // The local merge gate decides the merge (`cargo xtask ci --merge`: everything still builds
        // after the rebase; the worker's gate ran the tests). GitHub runs CI on `main` only when
        // dispatched; a red run there stops further merges until it is fixed (`task-board.md` §5).
        if let Some(run) = self.gh.main_ci_failure()? {
            return Err(format!("main is red on GitHub CI ({run}); fix it before merging anything else"));
        }
        let dir = self.git.rebase_for_merge(&branch)?;
        // New, changed or deleted fingerprint baselines do not hold the merge: the maintainer
        // approves them in bulk at the next review, which lists them from the dev log (`task-board.md` §5).
        let baselines = fingerprints(self.git.changed_paths(&branch)?);
        self.git.run_ci(&dir)?;
        self.git.push(&dir, &branch)?;
        let head = self.git.head(&dir)?;
        self.wait_mergeable(id, pr.number, &head)?;
        let subject = format!("{}: {} (#{})", task.id, task.plain_title(), pr.number);
        self.squash(id, pr.number, &subject)?;
        if !baselines.is_empty() {
            let text = fingerprint_record(&task.id, pr.number, &baselines);
            let heading = format!("{} {}", task.id, task.plain_title());
            let entry = devlog::entry(self.clock.now(), self.clock.utc_offset(), LEAD, &heading, &text);
            self.git.append_devlog(&entry).map_err(|e| {
                format!(
                    "merged {subject}, but could not record its fingerprint baselines for the next review ({e}); add these lines to the dev log by hand, then run `cargo xtask tasks sync`:\n{text}"
                )
            })?;
        }
        Ok(subject)
    }

    /// Waits (bounded by [`MERGEABLE_WAITS`]) until GitHub says the pushed `head` of `pr` is mergeable.
    fn wait_mergeable(&self, id: &str, pr: u64, head: &str) -> Result<(), String> {
        let mut waited = 0u64;
        // The first read right away, then one after each pause.
        for pause in std::iter::once(0).chain(MERGEABLE_WAITS) {
            if pause > 0 {
                self.clock.pause(pause);
                waited = waited.saturating_add(pause);
            }
            match self.gh.pr_mergeable(pr, head)? {
                MergeState::Mergeable => return Ok(()),
                MergeState::Conflicting => {
                    return Err(format!(
                        "GitHub says PR #{pr} conflicts with main (main moved after the rebase?); nothing was merged: run `cargo xtask tasks merge {id}` again"
                    ));
                }
                MergeState::Unknown => {}
            }
        }
        Err(format!(
            "GitHub had not worked out whether PR #{pr} is mergeable after {waited} s; the branch is pushed, nothing was merged: run `cargo xtask tasks merge {id}` again"
        ))
    }

    /// Squash-merges `pr`, trying again (bounded) while GitHub still answers "not mergeable".
    fn squash(&self, id: &str, pr: u64, subject: &str) -> Result<(), String> {
        let not_mergeable = NOT_MERGEABLE.to_ascii_lowercase();
        let mut last = String::new();
        for attempt in 0..SQUASH_TRIES {
            if attempt > 0 {
                self.clock.pause(SQUASH_RETRY_SECS);
            }
            match self.gh.squash_merge(pr, subject) {
                Ok(()) => return Ok(()),
                Err(e) if e.to_ascii_lowercase().contains(&not_mergeable) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(format!(
            "GitHub still refused to merge PR #{pr} after {SQUASH_TRIES} tries ({last}); the branch is pushed, nothing was merged: run `cargo xtask tasks merge {id}` again"
        ))
    }

    /// `tasks status`: the board as text; marks idle claims `stale` (and unmarks active ones).
    pub fn status(&self) -> Result<String, String> {
        let snap = self.snapshot()?;
        let log = std::fs::read_to_string(devlog::path(&self.git.main_checkout())).unwrap_or_default();
        let now = self.clock.now();
        let offset = self.clock.utc_offset();
        let mut out = String::new();
        for milestone in &self.roadmap.milestones {
            let tasks: Vec<&Task> = self.roadmap.tasks.iter().filter(|t| t.milestone == milestone.number).collect();
            if tasks.is_empty() {
                continue;
            }
            let done = tasks.iter().filter(|t| snap.done(t)).count();
            out.push_str(&format!("{} ({done}/{} done)\n", milestone.title(), tasks.len()));
            for task in tasks {
                if !self.listed(&snap, task) {
                    out.push_str(&format!("  {:<6} {:<12} {}\n", task.id, "planned", task.plain_title()));
                    continue;
                }
                let issue = snap.issue(task);
                let item = issue.and_then(|i| snap.item(i.number));
                let status = item.and_then(|i| i.status).map_or("—", Status::name);
                let mut line = format!("  {:<6} {:<12} {}", task.id, status, task.plain_title());
                if let Some(item) = item.filter(|i| !i.agent.is_empty()) {
                    line.push_str(&format!("  [{} on {}]", item.agent, item.branch));
                }
                if let (Some(issue), Some(item)) = (issue, item)
                    && item.status == Some(Status::InProgress)
                {
                    let last = self.last_activity(&log, task, &item.branch, offset);
                    let stale = last.is_none_or(|t| now.saturating_sub(t) > STALE_SECS);
                    if stale {
                        line.push_str("  STALE");
                    }
                    if stale != issue.has_label(LABEL_STALE) {
                        let edit = if stale {
                            IssueEdit { add_labels: vec![LABEL_STALE.into()], ..IssueEdit::default() }
                        } else {
                            IssueEdit { remove_labels: vec![LABEL_STALE.into()], ..IssueEdit::default() }
                        };
                        self.gh.edit_issue(issue.number, &edit)?;
                    }
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// `tasks prune`: removes cargo's `incremental/` caches from the target directories under
    /// `target_root` whose agent holds no In Progress task (`dry_run`: only says what it would remove).
    pub fn prune(&self, target_root: &Path, dry_run: bool) -> Result<prune::Report, String> {
        let snap = self.snapshot()?;
        let mut busy = BTreeMap::new();
        for task in &self.roadmap.tasks {
            if let Some(item) = snap.issue(task).and_then(|i| snap.item(i.number))
                && item.status == Some(Status::InProgress)
                && !item.agent.is_empty()
            {
                busy.insert(item.agent.clone(), task.id.clone());
            }
        }
        Ok(prune::prune(target_root, &busy, dry_run))
    }

    fn last_activity(&self, log: &str, task: &Task, branch: &str, offset: i64) -> Option<i64> {
        let logged = devlog::entry_times(log, &task.id, offset).into_iter().max();
        let pushed = if branch.is_empty() { None } else { self.git.last_activity(branch) };
        logged.max(pushed)
    }

    /// `tasks setup`: labels, milestones and project fields (idempotent).
    pub fn setup(&self) -> Result<SetupReport, String> {
        let mut report = SetupReport::default();
        let labels = self.gh.labels()?;
        for (name, color, description) in LABELS {
            if !labels.iter().any(|l| l == name) {
                self.gh.create_label(name, color, description)?;
                report.changes.push(format!("created label {name}"));
            }
        }
        self.setup_milestones(&mut report)?;
        let fields = self.gh.project_fields()?;
        let missing: Vec<&str> = Status::ALL.iter().map(|s| s.name()).filter(|s| !fields.status_options.iter().any(|o| o == s)).collect();
        if !missing.is_empty() {
            // Replacing a single-select field's options through the API resets every item's value, so a
            // human adds them in the project's settings instead.
            return Err(format!("the project's Status field lacks {}; add them in the project settings and rerun", missing.join(", ")));
        }
        for field in [TextField::Agent, TextField::Branch] {
            if !fields.text_fields.iter().any(|f| f == field.name()) {
                self.gh.create_text_field(field.name())?;
                report.changes.push(format!("created project field {}", field.name()));
            }
        }
        Ok(report)
    }

    /// One GitHub milestone per roadmap milestone, titled and described as `roadmap.md` says. A
    /// GitHub milestone is matched by its exact title, else by its `M<n>` number (and renamed: the
    /// roadmap renamed its milestones once); one with no roadmap counterpart is closed when it has
    /// no issues, else reported. Nothing is deleted.
    fn setup_milestones(&self, report: &mut SetupReport) -> Result<(), String> {
        let github = self.gh.milestones()?;
        let mut used: BTreeSet<u64> = BTreeSet::new();
        let mut matched: Vec<(&roadmap::Milestone, Option<&MilestoneInfo>)> = Vec::new();
        for m in &self.roadmap.milestones {
            let exact = github.iter().find(|x| x.title == m.title());
            if let Some(x) = exact {
                used.insert(x.number);
            }
            matched.push((m, exact));
        }
        for (m, found) in &mut matched {
            if found.is_none() {
                *found = github.iter().filter(|x| !used.contains(&x.number) && milestone_number(&x.title) == Some(m.number)).min_by_key(|x| x.number);
                if let Some(x) = found {
                    used.insert(x.number);
                }
            }
        }
        for (m, found) in matched {
            let title = m.title();
            match found {
                None => {
                    self.gh.create_milestone(&title, &m.goal)?;
                    report.changes.push(format!("created milestone {title}"));
                }
                Some(x) if x.title != title => {
                    self.gh.edit_milestone(x.number, &title, &m.goal)?;
                    report.changes.push(format!("renamed milestone `{}` to `{title}`", x.title));
                }
                Some(x) if x.description != m.goal => {
                    self.gh.edit_milestone(x.number, &title, &m.goal)?;
                    report.changes.push(format!("updated the description of milestone {title}"));
                }
                Some(_) => {}
            }
        }
        let mut retired = 0usize;
        for x in github.iter().filter(|x| !used.contains(&x.number)) {
            if x.issues > 0 {
                report.notes.push(format!(
                    "milestone `{}` is not in roadmap.md but holds {} issue(s); left as it is (once they move, setup closes it)",
                    x.title, x.issues
                ));
            } else if x.open {
                self.gh.close_milestone(x.number)?;
                report.changes.push(format!("closed milestone `{}` (not in roadmap.md, no issues)", x.title));
            } else {
                retired += 1;
            }
        }
        if retired > 0 {
            report.notes.push(format!("{retired} closed milestone(s) with no issues are not in roadmap.md; left as they are"));
        }
        Ok(())
    }
}

/// `cargo xtask ci` records each green commit here; `submit` checks it.
pub fn ci_marker(common_dir: &Path, commit: &str) -> PathBuf {
    common_dir.join("dalica-ci").join(format!("{commit}.ok"))
}

pub fn ci_passed(common_dir: &Path, commit: &str) -> bool {
    ci_marker(common_dir, commit).is_file()
}

/// The fingerprint files among `changes`.
fn fingerprints(changes: Vec<PathChange>) -> Vec<PathChange> {
    changes.into_iter().filter(|c| is_fingerprint(&c.path)).collect()
}

/// The dev-log lines `merge` writes for the next review (`docs/task-board.md` §5), one per kind of
/// change: `FINGERPRINTS: M3.4 #120 added crates/ui/tests/fingerprints/a.json …`.
fn fingerprint_record(task_id: &str, pr: u64, baselines: &[PathChange]) -> String {
    let mut lines = Vec::new();
    for kind in [ChangeKind::Added, ChangeKind::Changed, ChangeKind::Deleted] {
        let paths: Vec<&str> = baselines.iter().filter(|b| b.kind == kind).map(|b| b.path.as_str()).collect();
        if !paths.is_empty() {
            lines.push(format!("FINGERPRINTS: {task_id} #{pr} {} {}", kind.name(), paths.join(" ")));
        }
    }
    lines.join("\n")
}

/// The PR evidence when `submit` has no `--body-file`: the task's newest dev-log entry.
fn devlog_evidence(log: &str, id: &str) -> String {
    match devlog::latest_entry(log, id) {
        Some((heading, text)) => format!("### From the dev log ({heading})\n\n{text}"),
        None => String::new(),
    }
}

fn pr_body(task: &Task, issue: u64, subjects: &[String], evidence: &str, baselines: &[PathChange]) -> String {
    let commits: String = subjects.iter().map(|s| format!("- {s}\n")).collect();
    let baselines = if baselines.is_empty() {
        String::new()
    } else {
        let list: String = baselines.iter().map(|b| format!("- `{}` ({})\n", b.path, b.kind.name())).collect();
        format!(
            "## Fingerprint baselines\n\nThis branch adds, changes or deletes these fingerprints. Their images are attached to this PR and the commit body says why; `cargo xtask tasks merge` merges them and records them in the dev log, and the maintainer approves them in bulk at the next review (the milestone's M<n>.R):\n\n{list}\n"
        )
    };
    format!(
        "## What changed\n\n{commits}\n## Done when\n\n{}\n\nEvidence for each point, numbers and decisions are in the commit bodies and below.\n\n{}\n\n{baselines}Closes #{issue}\n",
        task.done_when,
        evidence.trim()
    )
}
