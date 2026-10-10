//! The generated section of a task issue's body. `sync` rewrites only the text between the markers,
//! so humans may add notes below it.

use std::collections::BTreeMap;

use super::config::{BLOB_URL, END, START};
use super::roadmap::Task;

/// The generated section for `task`; `issues` maps task ids to issue numbers (for `after`).
pub fn render(task: &Task, issues: &BTreeMap<String, u64>) -> String {
    let after = if task.after.is_empty() {
        "—".to_string()
    } else {
        task.after
            .iter()
            .map(|id| match issues.get(id) {
                Some(n) => format!("{id} (#{n})"),
                None => id.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let next = if task.is_gate() {
        format!(
            "A review task: the lead prepares it with `/milestone-review {}`; only the maintainer merges its pull request and closes this issue, and nothing listed after it in the roadmap starts until then.",
            task.id
        )
    } else {
        format!("Claim with `cargo xtask tasks claim {} --agent <name>`.", task.id)
    };
    let pulled = task.pulled.map_or_else(String::new, |k| format!("- **pulled:** M{k} (its run may take this task early)\n"));
    format!(
        "{START}\n**Task {id}** · [roadmap.md line {line}]({BLOB_URL}/docs/roadmap.md#L{line}) · managed by `cargo xtask tasks sync` (edit the roadmap, not this section)\n\n\
         - **after:** {after}\n- **owner:** {owner}\n{pulled}- **files:** {files}\n- **scope:** {scope}\n- **done when:** {done}\n\n\
         Workflow: [docs/task-board.md]({BLOB_URL}/docs/task-board.md). {next}\n{END}\n",
        id = task.id,
        line = task.line,
        owner = task.owner.as_str(),
        files = task.files_text,
        scope = task.scope,
        done = task.done_when,
    )
}

/// `body` with its generated section replaced by `section` (or `section` first if it has none).
pub fn replace_section(body: &str, section: &str) -> String {
    if let (Some(start), Some(end)) = (body.find(START), body.find(END))
        && start < end
    {
        let mut tail = body.get(end + END.len()..).unwrap_or("");
        tail = tail.strip_prefix('\n').unwrap_or(tail);
        return format!("{}{section}{tail}", body.get(..start).unwrap_or(""));
    }
    if body.trim().is_empty() { section.to_string() } else { format!("{section}\n{body}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::roadmap::Owner;

    fn task() -> Task {
        Task {
            id: "M0.5".into(),
            milestone: 0,
            title: "`xtask ci`".into(),
            done: false,
            after: vec!["M0.3".into(), "M0.4".into()],
            owner: Owner::Agent,
            pulled: None,
            files: vec!["xtask/src/ci.rs".into()],
            files_text: "`xtask/src/ci.rs`".into(),
            scope: "s".into(),
            done_when: "d".into(),
            line: 171,
        }
    }

    #[test]
    fn matches_the_bootstrap_format() {
        let issues = BTreeMap::from([("M0.3".to_string(), 4), ("M0.4".to_string(), 5)]);
        let body = render(&task(), &issues);
        assert_eq!(
            body,
            "<!-- dalica:task -->\n**Task M0.5** · [roadmap.md line 171](https://github.com/pkkid/dalica/blob/main/docs/roadmap.md#L171) · managed by `cargo xtask tasks sync` (edit the roadmap, not this section)\n\n- **after:** M0.3 (#4), M0.4 (#5)\n- **owner:** agent\n- **files:** `xtask/src/ci.rs`\n- **scope:** s\n- **done when:** d\n\nWorkflow: [docs/task-board.md](https://github.com/pkkid/dalica/blob/main/docs/task-board.md). Claim with `cargo xtask tasks claim M0.5 --agent <name>`.\n<!-- /dalica:task -->\n"
        );
    }

    #[test]
    fn a_pulled_task_says_which_run_may_take_it() {
        let pulled = Task { id: "M1.5".into(), milestone: 1, pulled: Some(0), ..task() };
        let body = render(&pulled, &BTreeMap::new());
        assert!(body.contains("- **owner:** agent\n- **pulled:** M0 (its run may take this task early)\n- **files:**"), "{body}");
    }

    #[test]
    fn a_review_says_who_merges_and_closes_it() {
        let review = Task { id: "M0.R".into(), owner: Owner::Maintainer, ..task() };
        let body = render(&review, &BTreeMap::new());
        assert!(body.contains("- **owner:** maintainer\n"), "{body}");
        assert!(
            body.contains(
                "A review task: the lead prepares it with `/milestone-review M0.R`; only the maintainer merges its pull request and closes this issue"
            ),
            "{body}"
        );
        assert!(!body.contains("tasks claim"), "{body}");
    }

    #[test]
    fn replace_keeps_human_notes() {
        let old = "<!-- dalica:task -->\nold\n<!-- /dalica:task -->\nnote from a human\n";
        assert_eq!(
            replace_section(old, "<!-- dalica:task -->\nnew\n<!-- /dalica:task -->\n"),
            "<!-- dalica:task -->\nnew\n<!-- /dalica:task -->\nnote from a human\n"
        );
        assert_eq!(replace_section("", "S\n"), "S\n");
        assert_eq!(replace_section("hand-written\n", "S\n"), "S\n\nhand-written\n");
    }
}
