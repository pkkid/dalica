//! `--dry-run`: forwards reads to the real [`GitHub`] and [`Git`] and records writes instead of
//! making them, so the maintainer or the lead can see what `setup`, `sync`, `submit`, `merge` or
//! `prune` would change before anything does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::git::{Git, PathChange};
use super::github::{GitHub, Issue, IssueEdit, Item, MergeState, MilestoneInfo, NewIssue, ProjectFields, PullRequest, Status, TextField};

/// The writes a dry run would have made, in order.
#[derive(Default)]
pub struct Recorder {
    writes: Mutex<Vec<String>>,
}

impl Recorder {
    fn record(&self, what: String) {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).push(what);
    }

    pub fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

pub struct DryRun<'a> {
    pub inner: &'a dyn GitHub,
    rec: &'a Recorder,
}

impl<'a> DryRun<'a> {
    pub fn new(inner: &'a dyn GitHub, rec: &'a Recorder) -> DryRun<'a> {
        DryRun { inner, rec }
    }

    fn record(&self, what: String) {
        self.rec.record(what);
    }
}

/// Local git with the writes (worktrees, rebases, `ci` runs, pushes) recorded instead of made.
pub struct DryGit<'a> {
    pub inner: &'a dyn Git,
    rec: &'a Recorder,
}

impl<'a> DryGit<'a> {
    pub fn new(inner: &'a dyn Git, rec: &'a Recorder) -> DryGit<'a> {
        DryGit { inner, rec }
    }
}

impl Git for DryGit<'_> {
    fn merged_task_ids(&self) -> Result<BTreeSet<String>, String> {
        self.inner.merged_task_ids()
    }

    fn last_activity(&self, branch: &str) -> Option<i64> {
        self.inner.last_activity(branch)
    }

    fn prepare_worktree(&self, agent: &str, branch: &str) -> Result<PathBuf, String> {
        self.rec.record(format!("check out {branch} in {agent}'s worktree"));
        Ok(self.inner.main_checkout())
    }

    fn head(&self, dir: &Path) -> Result<String, String> {
        self.inner.head(dir)
    }

    fn branch_subjects(&self, branch: &str) -> Result<Vec<String>, String> {
        self.inner.branch_subjects(branch)
    }

    fn changed_paths(&self, branch: &str) -> Result<Vec<PathChange>, String> {
        self.inner.changed_paths(branch)
    }

    fn pushed_head(&self, branch: &str) -> Option<String> {
        self.inner.pushed_head(branch)
    }

    fn push(&self, _dir: &Path, branch: &str) -> Result<(), String> {
        self.rec.record(format!("push {branch} to origin"));
        Ok(())
    }

    fn rebase_for_merge(&self, branch: &str) -> Result<PathBuf, String> {
        self.rec.record(format!("rebase origin/{branch} onto origin/main in the merge worktree"));
        Ok(self.inner.main_checkout())
    }

    fn run_ci(&self, _dir: &Path) -> Result<(), String> {
        self.rec.record("run `cargo xtask ci --merge` in the merge worktree".into());
        Ok(())
    }

    fn merged_roadmap(&self) -> Result<String, String> {
        self.inner.merged_roadmap()
    }

    fn append_devlog(&self, text: &str) -> Result<(), String> {
        self.rec.record(format!("append to the dev log:\n{}", text.trim()));
        Ok(())
    }

    fn main_checkout(&self) -> PathBuf {
        self.inner.main_checkout()
    }

    fn common_dir(&self) -> PathBuf {
        self.inner.common_dir()
    }
}

impl GitHub for DryRun<'_> {
    fn issues(&self) -> Result<Vec<Issue>, String> {
        self.inner.issues()
    }

    fn create_issue(&self, issue: &NewIssue) -> Result<u64, String> {
        self.record(format!("create issue `{}` (labels {:?}, milestone {:?})", issue.title, issue.labels, issue.milestone));
        Ok(0)
    }

    fn edit_issue(&self, number: u64, edit: &IssueEdit) -> Result<(), String> {
        let mut parts = Vec::new();
        if let Some(t) = &edit.title {
            parts.push(format!("title -> `{t}`"));
        }
        if edit.body.is_some() {
            parts.push("body".to_string());
        }
        if !edit.add_labels.is_empty() {
            parts.push(format!("+labels {:?}", edit.add_labels));
        }
        if !edit.remove_labels.is_empty() {
            parts.push(format!("-labels {:?}", edit.remove_labels));
        }
        if let Some(m) = &edit.milestone {
            parts.push(format!("milestone -> {m}"));
        }
        if let Some(a) = &edit.add_assignee {
            parts.push(format!("+assignee {a}"));
        }
        self.record(format!("edit issue #{number}: {}", parts.join(", ")));
        Ok(())
    }

    fn comment(&self, number: u64, body: &str) -> Result<(), String> {
        self.record(format!("comment on #{number}: {body}"));
        Ok(())
    }

    fn close_issue(&self, number: u64, comment: &str) -> Result<(), String> {
        self.record(format!("close issue #{number}: {comment}"));
        Ok(())
    }

    fn items(&self) -> Result<Vec<Item>, String> {
        self.inner.items()
    }

    fn add_item(&self, issue: u64) -> Result<String, String> {
        self.record(format!("add issue #{issue} to the project"));
        Ok(format!("NEW_ITEM_{issue}"))
    }

    fn set_status(&self, item: &str, status: Status) -> Result<(), String> {
        self.record(format!("set item {item} Status = {status}"));
        Ok(())
    }

    fn set_field(&self, item: &str, field: TextField, value: &str) -> Result<(), String> {
        self.record(format!("set item {item} {} = `{value}`", field.name()));
        Ok(())
    }

    fn move_item(&self, item: &str, after: Option<&str>) -> Result<(), String> {
        self.record(format!("move item {item} after {}", after.unwrap_or("(top)")));
        Ok(())
    }

    fn milestones(&self) -> Result<Vec<MilestoneInfo>, String> {
        self.inner.milestones()
    }

    fn create_milestone(&self, title: &str, _description: &str) -> Result<(), String> {
        self.record(format!("create milestone `{title}`"));
        Ok(())
    }

    fn edit_milestone(&self, number: u64, title: &str, description: &str) -> Result<(), String> {
        self.record(format!("edit milestone {number}: title `{title}`, description `{description}`"));
        Ok(())
    }

    fn close_milestone(&self, number: u64) -> Result<(), String> {
        self.record(format!("close milestone {number}"));
        Ok(())
    }

    fn labels(&self) -> Result<Vec<String>, String> {
        self.inner.labels()
    }

    fn create_label(&self, name: &str, _color: &str, _description: &str) -> Result<(), String> {
        self.record(format!("create label `{name}`"));
        Ok(())
    }

    fn project_fields(&self) -> Result<ProjectFields, String> {
        self.inner.project_fields()
    }

    fn create_text_field(&self, name: &str) -> Result<(), String> {
        self.record(format!("create project field `{name}`"));
        Ok(())
    }

    fn find_pr(&self, branch: &str) -> Result<Option<PullRequest>, String> {
        self.inner.find_pr(branch)
    }

    fn open_pr(&self, branch: &str, title: &str, body: &str) -> Result<PullRequest, String> {
        let body: String = body.lines().map(|l| format!("\n      | {l}")).collect();
        self.record(format!("open pull request `{title}` from {branch}, body:{body}"));
        Ok(PullRequest { number: 0, url: "(dry run)".into(), labels: Vec::new() })
    }

    fn main_ci_failure(&self) -> Result<Option<String>, String> {
        self.inner.main_ci_failure()
    }

    /// Nothing was pushed, so GitHub's answer would describe the branch before the rebase; the dry
    /// run assumes the wait would end in a mergeable pull request.
    fn pr_mergeable(&self, _pr: u64, _head: &str) -> Result<MergeState, String> {
        Ok(MergeState::Mergeable)
    }

    fn squash_merge(&self, pr: u64, subject: &str) -> Result<(), String> {
        self.record(format!("squash-merge #{pr} as `{subject}`"));
        Ok(())
    }
}
