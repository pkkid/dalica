//! Board logic shared by every `cargo xtask tasks` command, written against the [`GitHub`], [`Git`]
//! and [`Clock`] traits so tests run it on fakes (`docs/task-board.md` §4–§5).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::body;
use super::config::{LABEL_BLOCKED, LABEL_GATE, LABEL_STALE, LABEL_TASK, LABELS, LEAD, MAINTAINER, STALE_SECS};
use super::devlog;
use super::git::{Clock, Git};
use super::github::{ALREADY_IN_PROJECT, GitHub, Issue, IssueEdit, Item, NewIssue, Status, TextField};
use super::glob::lists_overlap;
use super::roadmap::{Owner, Roadmap, Task};

/// Reads of the project's item list while waiting for an auto-added item, and the pause between them.
const ITEM_WAIT_TRIES: usize = 6;
const ITEM_WAIT_SECS: u64 = 2;

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

    /// The lowest milestone with an open review gate the maintainer owns; tasks of later milestones
    /// wait for it. A gate the lead owns (a check-in) blocks nothing.
    fn open_gate_milestone(&self, snap: &Snapshot) -> Option<u32> {
        self.roadmap.tasks.iter().filter(|t| t.is_gate() && t.owner == Owner::Maintainer && !snap.done(t)).map(|t| t.milestone).min()
    }

    /// Where `roadmap.md`, the issues and `main` disagree; `sync` refuses to touch the board then.
    pub fn disagreements(&self, snap: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        for task in &self.roadmap.tasks {
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
        let mut report = SyncReport::default();
        let mut numbers = snap.issue_numbers(self.roadmap);
        let mut ordered_items: Vec<String> = Vec::new();

        for task in &self.roadmap.tasks {
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
        let status = snap.status(task);
        if status != Some(Status::Ready) {
            return Some(format!("{} is {}, not Ready", task.id, status.map_or("not on the board", Status::name)));
        }
        if let Some(dep) = task.after.iter().find(|d| !self.roadmap.task(d).is_some_and(|t| snap.done(t))) {
            return Some(format!("{} waits for {dep}", task.id));
        }
        if let Some(gate) = self.open_gate_milestone(snap)
            && task.milestone > gate
        {
            return Some(format!("{} is in M{} but the M{gate} review gate is open", task.id, task.milestone));
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

    /// `tasks submit`: refuses without a green `ci` on this commit and a dev-log entry; pushes, opens
    /// the pull request and sets In Review.
    pub fn submit(&self, id: &str, dir: &Path, body_extra: Option<&str>) -> Result<String, String> {
        let task = self.task(id)?;
        let snap = self.snapshot()?;
        let (issue, item) = self.require_status(&snap, task, &[Status::InProgress])?;
        let head = self.git.head(dir)?;
        if !ci_passed(&self.git.common_dir(), &head) {
            return Err(format!("cargo xtask ci has not passed on {head}; run it in this worktree first"));
        }
        let log = std::fs::read_to_string(devlog::path(&self.git.main_checkout())).unwrap_or_default();
        if devlog::entry_times(&log, id, 0).is_empty() {
            return Err(format!("no dev-log entry for {id}; add one with `cargo xtask tasks log {id} \"Landed: … Open: … Numbers: …\"`"));
        }
        let branch = if item.branch.is_empty() { task.branch() } else { item.branch.clone() };
        self.git.push(dir, &branch)?;
        let pr = match self.gh.find_pr(&branch)? {
            Some(pr) => pr,
            None => {
                let subjects = self.git.branch_subjects(&branch).unwrap_or_default();
                let body = pr_body(task, issue.number, &subjects, body_extra);
                self.gh.open_pr(&branch, &format!("{}: {}", task.id, task.plain_title()), &body)?
            }
        };
        self.gh.set_status(&item.id, Status::InReview)?;
        self.gh.comment(issue.number, &format!("submitted for review: {}", pr.url))?;
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

    /// `tasks merge` (lead only; the caller holds the merge lock).
    pub fn merge(&self, id: &str) -> Result<String, String> {
        let task = self.task(id)?;
        let snap = self.snapshot()?;
        let (_, item) = self.require_status(&snap, task, &[Status::InReview])?;
        let branch = if item.branch.is_empty() { task.branch() } else { item.branch.clone() };
        let pr = self.gh.find_pr(&branch)?.ok_or_else(|| format!("no open pull request for {branch}"))?;
        // The local gate decides the merge; GitHub's three-OS run checks `main` after it, and a red
        // `main` stops further merges until it is fixed (`task-board.md` §5).
        if let Some(run) = self.gh.main_ci_failure()? {
            return Err(format!("main is red on GitHub CI ({run}); fix it before merging anything else"));
        }
        let dir = self.git.rebase_for_merge(&branch)?;
        self.git.run_ci(&dir)?;
        self.git.push(&dir, &branch)?;
        let subject = format!("{}: {} (#{})", task.id, task.plain_title(), pr.number);
        self.gh.squash_merge(pr.number, &subject)?;
        Ok(subject)
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

    fn last_activity(&self, log: &str, task: &Task, branch: &str, offset: i64) -> Option<i64> {
        let logged = devlog::entry_times(log, &task.id, offset).into_iter().max();
        let pushed = if branch.is_empty() { None } else { self.git.last_activity(branch) };
        logged.max(pushed)
    }

    /// `tasks setup`: labels, milestones and project fields (idempotent).
    pub fn setup(&self) -> Result<Vec<String>, String> {
        let mut changes = Vec::new();
        let labels = self.gh.labels()?;
        for (name, color, description) in LABELS {
            if !labels.iter().any(|l| l == name) {
                self.gh.create_label(name, color, description)?;
                changes.push(format!("created label {name}"));
            }
        }
        let milestones = self.gh.milestones()?;
        for m in &self.roadmap.milestones {
            let prefix = format!("M{} ", m.number);
            if !milestones.iter().any(|x| x.title.starts_with(&prefix)) {
                self.gh.create_milestone(&m.title(), &m.goal)?;
                changes.push(format!("created milestone {}", m.title()));
            }
        }
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
                changes.push(format!("created project field {}", field.name()));
            }
        }
        Ok(changes)
    }
}

/// `cargo xtask ci` records each green commit here; `submit` checks it.
pub fn ci_marker(common_dir: &Path, commit: &str) -> PathBuf {
    common_dir.join("dalica-ci").join(format!("{commit}.ok"))
}

pub fn ci_passed(common_dir: &Path, commit: &str) -> bool {
    ci_marker(common_dir, commit).is_file()
}

fn pr_body(task: &Task, issue: u64, subjects: &[String], extra: Option<&str>) -> String {
    let commits: String = subjects.iter().map(|s| format!("- {s}\n")).collect();
    format!(
        "## What changed\n\n{commits}\n## Done when\n\n{}\n\nEvidence for each point, numbers and decisions are in the commit bodies and below.\n\n{}\n\nCloses #{issue}\n",
        task.done_when,
        extra.unwrap_or("").trim()
    )
}
