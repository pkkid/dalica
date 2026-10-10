//! In-memory fakes of [`GitHub`], [`Git`] and [`Clock`] for the board tests. Every write bumps a
//! mutation counter, so tests can assert "no second change".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::git::{Clock, Git};
use super::github::{
    ALREADY_IN_PROJECT, GitHub, Issue, IssueEdit, Item, MergeState, MilestoneInfo, NewIssue, ProjectFields, PullRequest, Status, TextField,
};

#[derive(Default)]
pub struct State {
    pub issues: Vec<Issue>,
    pub items: Vec<Item>,
    pub comments: Vec<(u64, String)>,
    pub milestones: Vec<MilestoneInfo>,
    pub labels: Vec<String>,
    pub fields: ProjectFields,
    pub prs: Vec<(String, PullRequest)>,
    /// (title, body) of every pull request opened.
    pub pr_texts: Vec<(String, String)>,
    pub merged_prs: Vec<(u64, String)>,
    /// When set, the latest CI run on `main` failed (`main_ci_failure`).
    pub main_ci_failure: Option<String>,
    /// The project's auto-add workflow: a created issue lands on the board at once.
    pub auto_add: bool,
    /// Reads of the item list that still miss auto-added items (GitHub lists them a moment late).
    pub item_list_lag: usize,
    /// The next answers of `pr_mergeable`, oldest first; once used up, every pull request is
    /// mergeable.
    pub mergeable: Vec<MergeState>,
    /// How often `pr_mergeable` was asked.
    pub mergeable_reads: usize,
    /// Errors the next `squash_merge` calls fail with, oldest first.
    pub squash_errors: Vec<String>,
}

#[derive(Default)]
pub struct FakeGitHub {
    pub state: Mutex<State>,
    pub mutations: AtomicUsize,
}

impl FakeGitHub {
    pub fn with_setup() -> FakeGitHub {
        let fake = FakeGitHub::default();
        {
            let mut s = fake.lock();
            s.fields.status_options = Status::ALL.iter().map(|s| s.name().to_string()).collect();
            s.fields.text_fields = vec!["Agent".into(), "Branch".into()];
        }
        fake
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn mutations(&self) -> usize {
        self.mutations.load(Ordering::SeqCst)
    }

    fn bump(&self) {
        self.mutations.fetch_add(1, Ordering::SeqCst);
    }

    fn with_item(&self, item: &str, f: impl FnOnce(&mut Item)) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        let it = s.items.iter_mut().find(|i| i.id == item).ok_or_else(|| format!("no item {item}"))?;
        f(it);
        Ok(())
    }

    pub fn item_status(&self, issue: u64) -> Option<Status> {
        self.lock().items.iter().find(|i| i.issue == issue).and_then(|i| i.status)
    }
}

impl GitHub for FakeGitHub {
    fn issues(&self) -> Result<Vec<Issue>, String> {
        Ok(self.lock().issues.clone())
    }

    fn create_issue(&self, issue: &NewIssue) -> Result<u64, String> {
        let mut s = self.lock();
        missing_milestone(&s, issue.milestone.as_deref())?;
        self.bump();
        let number = s.issues.iter().map(|i| i.number).max().unwrap_or(0) + 1;
        s.issues.push(Issue {
            number,
            title: issue.title.clone(),
            body: issue.body.clone(),
            open: true,
            labels: issue.labels.clone(),
            assignees: issue.assignee.clone().into_iter().collect(),
            milestone: issue.milestone.clone(),
        });
        if s.auto_add {
            s.items.push(Item { id: format!("AUTO_{number}"), issue: number, status: None, agent: String::new(), branch: String::new() });
        }
        Ok(number)
    }

    fn edit_issue(&self, number: u64, edit: &IssueEdit) -> Result<(), String> {
        let mut s = self.lock();
        missing_milestone(&s, edit.milestone.as_deref())?;
        self.bump();
        let issue = s.issues.iter_mut().find(|i| i.number == number).ok_or_else(|| format!("no issue #{number}"))?;
        if let Some(t) = &edit.title {
            issue.title.clone_from(t);
        }
        if let Some(b) = &edit.body {
            issue.body.clone_from(b);
        }
        for l in &edit.add_labels {
            if !issue.labels.contains(l) {
                issue.labels.push(l.clone());
            }
        }
        issue.labels.retain(|l| !edit.remove_labels.contains(l));
        if let Some(m) = &edit.milestone {
            issue.milestone = Some(m.clone());
        }
        if let Some(a) = &edit.add_assignee {
            issue.assignees.push(a.clone());
        }
        Ok(())
    }

    fn comment(&self, number: u64, body: &str) -> Result<(), String> {
        self.bump();
        self.lock().comments.push((number, body.to_string()));
        Ok(())
    }

    fn close_issue(&self, number: u64, comment: &str) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        s.comments.push((number, comment.to_string()));
        let issue = s.issues.iter_mut().find(|i| i.number == number).ok_or_else(|| format!("no issue #{number}"))?;
        issue.open = false;
        Ok(())
    }

    fn items(&self) -> Result<Vec<Item>, String> {
        let mut s = self.lock();
        if s.item_list_lag > 0 {
            s.item_list_lag -= 1;
            return Ok(s.items.iter().filter(|i| !i.id.starts_with("AUTO_")).cloned().collect());
        }
        Ok(s.items.clone())
    }

    fn add_item(&self, issue: u64) -> Result<String, String> {
        self.bump();
        let mut s = self.lock();
        if s.items.iter().any(|i| i.issue == issue) {
            return Err(format!("GraphQL: {ALREADY_IN_PROJECT} (addProjectV2ItemById)"));
        }
        let id = format!("ITEM_{issue}");
        s.items.push(Item { id: id.clone(), issue, status: None, agent: String::new(), branch: String::new() });
        Ok(id)
    }

    fn set_status(&self, item: &str, status: Status) -> Result<(), String> {
        self.with_item(item, |i| i.status = Some(status))
    }

    fn set_field(&self, item: &str, field: TextField, value: &str) -> Result<(), String> {
        self.with_item(item, |i| match field {
            TextField::Agent => i.agent = value.to_string(),
            TextField::Branch => i.branch = value.to_string(),
        })
    }

    fn move_item(&self, item: &str, after: Option<&str>) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        let pos = s.items.iter().position(|i| i.id == item).ok_or_else(|| format!("no item {item}"))?;
        let moved = s.items.remove(pos);
        let at = match after {
            None => 0,
            Some(a) => s.items.iter().position(|i| i.id == a).map(|p| p + 1).ok_or_else(|| format!("no item {a}"))?,
        };
        s.items.insert(at, moved);
        Ok(())
    }

    fn milestones(&self) -> Result<Vec<MilestoneInfo>, String> {
        Ok(self.lock().milestones.clone())
    }

    fn create_milestone(&self, title: &str, description: &str) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        let number = s.milestones.iter().map(|m| m.number).max().unwrap_or(0) + 1;
        s.milestones.push(MilestoneInfo { number, title: title.into(), description: description.into(), open: true, issues: 0 });
        Ok(())
    }

    fn edit_milestone(&self, number: u64, title: &str, description: &str) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        let m = s.milestones.iter_mut().find(|m| m.number == number).ok_or_else(|| format!("no milestone {number}"))?;
        m.title = title.into();
        m.description = description.into();
        Ok(())
    }

    fn close_milestone(&self, number: u64) -> Result<(), String> {
        self.bump();
        let mut s = self.lock();
        let m = s.milestones.iter_mut().find(|m| m.number == number).ok_or_else(|| format!("no milestone {number}"))?;
        m.open = false;
        Ok(())
    }

    fn labels(&self) -> Result<Vec<String>, String> {
        Ok(self.lock().labels.clone())
    }

    fn create_label(&self, name: &str, _color: &str, _description: &str) -> Result<(), String> {
        self.bump();
        self.lock().labels.push(name.into());
        Ok(())
    }

    fn project_fields(&self) -> Result<ProjectFields, String> {
        Ok(self.lock().fields.clone())
    }

    fn create_text_field(&self, name: &str) -> Result<(), String> {
        self.bump();
        self.lock().fields.text_fields.push(name.into());
        Ok(())
    }

    fn find_pr(&self, branch: &str) -> Result<Option<PullRequest>, String> {
        Ok(self.lock().prs.iter().find(|(b, _)| b == branch).map(|(_, p)| p.clone()))
    }

    fn open_pr(&self, branch: &str, title: &str, body: &str) -> Result<PullRequest, String> {
        self.bump();
        let mut s = self.lock();
        s.pr_texts.push((title.into(), body.into()));
        let number = 100 + s.prs.len() as u64;
        let pr = PullRequest { number, url: format!("https://example.invalid/pull/{number}"), labels: Vec::new() };
        s.prs.push((branch.into(), pr.clone()));
        Ok(pr)
    }

    fn main_ci_failure(&self) -> Result<Option<String>, String> {
        Ok(self.lock().main_ci_failure.clone())
    }

    fn pr_mergeable(&self, _pr: u64, _head: &str) -> Result<MergeState, String> {
        let mut s = self.lock();
        s.mergeable_reads += 1;
        Ok(if s.mergeable.is_empty() { MergeState::Mergeable } else { s.mergeable.remove(0) })
    }

    /// Like GitHub, a merged pull request closes the issue its body names with `Closes #N`.
    fn squash_merge(&self, pr: u64, subject: &str) -> Result<(), String> {
        let mut s = self.lock();
        if !s.squash_errors.is_empty() {
            return Err(s.squash_errors.remove(0));
        }
        self.bump();
        s.merged_prs.push((pr, subject.into()));
        let index = s.prs.iter().position(|(_, p)| p.number == pr);
        let closes = index
            .and_then(|i| s.pr_texts.get(i))
            .and_then(|(_, body)| body.split("Closes #").nth(1))
            .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse::<u64>().ok());
        if let Some(issue) = closes.and_then(|n| s.issues.iter_mut().find(|i| i.number == n)) {
            issue.open = false;
        }
        Ok(())
    }
}

/// GitHub's refusal when an issue names a milestone that does not exist.
fn missing_milestone(s: &State, milestone: Option<&str>) -> Result<(), String> {
    match milestone {
        Some(m) if !s.milestones.iter().any(|x| x.title == m) => Err(format!("could not add to milestone '{m}': '{m}' not found")),
        _ => Ok(()),
    }
}

pub struct FakeGit {
    pub dir: PathBuf,
    pub merged: Mutex<BTreeSet<String>>,
    pub activity: Mutex<Vec<(String, i64)>>,
    pub worktrees: AtomicUsize,
    /// What `changed_paths` reports for every branch.
    pub changed: Mutex<Vec<String>>,
    /// `push`, `rebase_for_merge` and `run_ci` calls, in order.
    pub calls: Mutex<Vec<String>>,
    /// `docs/roadmap.md` on origin's `main` (`merged_roadmap`).
    pub origin_roadmap: Mutex<Option<String>>,
}

impl FakeGit {
    pub fn new(dir: &Path) -> FakeGit {
        FakeGit {
            dir: dir.to_path_buf(),
            merged: Mutex::default(),
            activity: Mutex::default(),
            worktrees: AtomicUsize::new(0),
            changed: Mutex::default(),
            calls: Mutex::default(),
            origin_roadmap: Mutex::default(),
        }
    }
}

impl Git for FakeGit {
    fn merged_task_ids(&self) -> Result<BTreeSet<String>, String> {
        Ok(self.merged.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }

    fn last_activity(&self, branch: &str) -> Option<i64> {
        self.activity.lock().unwrap_or_else(PoisonError::into_inner).iter().filter(|(b, _)| b == branch).map(|(_, t)| *t).max()
    }

    fn prepare_worktree(&self, agent: &str, _branch: &str) -> Result<PathBuf, String> {
        self.worktrees.fetch_add(1, Ordering::SeqCst);
        Ok(self.dir.join(agent))
    }

    fn head(&self, _dir: &Path) -> Result<String, String> {
        Ok("0123abcd".into())
    }

    fn branch_subjects(&self, _branch: &str) -> Result<Vec<String>, String> {
        Ok(vec![])
    }

    fn changed_paths(&self, _branch: &str) -> Result<Vec<String>, String> {
        Ok(self.changed.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }

    fn push(&self, _dir: &Path, branch: &str) -> Result<(), String> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push(format!("push {branch}"));
        Ok(())
    }

    fn rebase_for_merge(&self, branch: &str) -> Result<PathBuf, String> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push(format!("rebase {branch}"));
        Ok(self.dir.clone())
    }

    fn run_ci(&self, _dir: &Path) -> Result<(), String> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push("ci".into());
        Ok(())
    }

    fn merged_roadmap(&self) -> Result<String, String> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push("fetch".into());
        self.origin_roadmap.lock().unwrap_or_else(PoisonError::into_inner).clone().ok_or_else(|| "origin/main has no docs/roadmap.md".into())
    }

    fn main_checkout(&self) -> PathBuf {
        self.dir.clone()
    }

    fn common_dir(&self) -> PathBuf {
        self.dir.join(".git")
    }
}

pub struct FakeClock {
    pub now: Mutex<i64>,
}

impl Clock for FakeClock {
    fn pause(&self, secs: u64) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) += secs as i64;
    }

    fn now(&self) -> i64 {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn utc_offset(&self) -> i64 {
        0
    }
}
