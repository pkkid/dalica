//! `--dry-run`: forwards reads to the real [`GitHub`] and records writes instead of making them, so
//! the maintainer can see what `setup` or `sync` would change before approving it.

use std::sync::{Mutex, PoisonError};

use super::github::{GitHub, Issue, IssueEdit, Item, MilestoneInfo, NewIssue, ProjectFields, PullRequest, Status, TextField};

pub struct DryRun<'a> {
    pub inner: &'a dyn GitHub,
    pub writes: Mutex<Vec<String>>,
}

impl<'a> DryRun<'a> {
    pub fn new(inner: &'a dyn GitHub) -> DryRun<'a> {
        DryRun { inner, writes: Mutex::new(Vec::new()) }
    }

    fn record(&self, what: String) {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).push(what);
    }

    pub fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap_or_else(PoisonError::into_inner).clone()
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

    fn open_pr(&self, branch: &str, title: &str, _body: &str) -> Result<PullRequest, String> {
        self.record(format!("open pull request `{title}` from {branch}"));
        Ok(PullRequest { number: 0, url: "(dry run)".into() })
    }

    fn wait_for_checks(&self, _pr: u64) -> Result<(), String> {
        Ok(())
    }

    fn squash_merge(&self, pr: u64, subject: &str) -> Result<(), String> {
        self.record(format!("squash-merge #{pr} as `{subject}`"));
        Ok(())
    }
}
