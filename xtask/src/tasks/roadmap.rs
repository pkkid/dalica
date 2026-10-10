//! The roadmap task parser (`docs/task-board.md` §3).
//!
//! Milestones come from the Overview table (`| Phase | M0 | Skeleton and gates | goal | ⬜ |`); task
//! blocks are list items `- ⬜ **M0.1 Title**` followed by `  - key: value` lines, where a value may
//! continue on lines indented by four spaces. Any malformed block is an error with its line number.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    pub number: u32,
    pub name: String,
    pub goal: String,
}

impl Milestone {
    /// The GitHub milestone title, e.g. `M1 Star a photo, end to end`.
    pub fn title(&self) -> String {
        format!("M{} {}", self.number, self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Agent,
    /// Only the lead (`agent-1`) takes it: check-ins, board upkeep.
    Lead,
    Maintainer,
}

impl Owner {
    pub fn as_str(self) -> &'static str {
        match self {
            Owner::Agent => "agent",
            Owner::Lead => "lead",
            Owner::Maintainer => "maintainer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// `M1.4`, `M1.A`, `M1.R`.
    pub id: String,
    pub milestone: u32,
    /// The block title as written, e.g. ``"`scenes`"``.
    pub title: String,
    /// ✅ in the roadmap.
    pub done: bool,
    pub after: Vec<String>,
    pub owner: Owner,
    /// Paths or globs without backticks; empty for `—`.
    pub files: Vec<String>,
    /// The `files` value as written (backticks kept), `—` when empty.
    pub files_text: String,
    pub scope: String,
    pub done_when: String,
    /// 1-based line of the block's first line.
    pub line: usize,
}

impl Task {
    /// Review gates (`M1.A`, `M1.R`) end in a letter.
    pub fn is_gate(&self) -> bool {
        self.id.rsplit('.').next().is_some_and(|s| s.chars().all(|c| c.is_ascii_uppercase()))
    }

    /// The issue title: id plus the block title, e.g. ``M1.4 `scenes` ``.
    pub fn issue_title(&self) -> String {
        format!("{} {}", self.id, self.title)
    }

    /// The title without Markdown backticks, for logs and branch names.
    pub fn plain_title(&self) -> String {
        self.title.replace('`', "")
    }

    /// `task/M1.4-scenes`.
    pub fn branch(&self) -> String {
        let mut slug = String::new();
        for c in self.plain_title().chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug: String = slug.trim_end_matches('-').chars().take(40).collect();
        format!("task/{}-{}", self.id, slug.trim_end_matches('-'))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roadmap {
    pub milestones: Vec<Milestone>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "docs/roadmap.md:{}: {}", self.line, self.message)
    }
}

impl Roadmap {
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn milestone(&self, number: u32) -> Option<&Milestone> {
        self.milestones.iter().find(|m| m.number == number)
    }
}

/// Splits `M1.4` into (1, "4"); `None` if it is not a task id.
pub fn split_id(id: &str) -> Option<(u32, &str)> {
    let rest = id.strip_prefix('M')?;
    let (ms, n) = rest.split_once('.')?;
    if ms.is_empty() || !ms.chars().all(|c| c.is_ascii_digit()) || ms.len() > 3 {
        return None;
    }
    let numeric = !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit());
    let gate = n.len() == 1 && n.chars().all(|c| c.is_ascii_uppercase());
    if !(numeric || gate) {
        return None;
    }
    Some((ms.parse().ok()?, n))
}

pub fn parse(text: &str) -> Result<Roadmap, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let milestones = parse_milestones(&lines)?;
    let mut tasks: Vec<Task> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines.get(i).copied().unwrap_or("");
        if is_block_start(line) {
            let (task, next) = parse_block(&lines, i, &milestones, &tasks)?;
            tasks.push(task);
            i = next;
        } else {
            i += 1;
        }
    }
    if tasks.is_empty() {
        return Err(ParseError { line: 1, message: "no task blocks found".into() });
    }
    Ok(Roadmap { milestones, tasks })
}

fn parse_milestones(lines: &[&str]) -> Result<Vec<Milestone>, ParseError> {
    let mut out: Vec<Milestone> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        let [_, number, name, goal, ..] = cells.as_slice() else { continue };
        let Some(n) = number.strip_prefix('M').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) else { continue };
        let Ok(n) = n.parse::<u32>() else { continue };
        if out.iter().any(|m| m.number == n) {
            return Err(ParseError { line: i + 1, message: format!("milestone M{n} is listed twice in the Overview table") });
        }
        out.push(Milestone { number: n, name: name.to_string(), goal: goal.to_string() });
    }
    if out.is_empty() {
        return Err(ParseError { line: 1, message: "no milestones found in the Overview table".into() });
    }
    Ok(out)
}

/// A list item whose bold text starts with a task-like id: `- ⬜ **M0.1 …`, or a malformed one such
/// as `- 🚧 **M0.1 …` (reported by `parse_block`).
fn is_block_start(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("- ") else { return false };
    let Some((_, after)) = rest.split_once(' ') else { return false };
    after.starts_with("**M") && after.chars().nth(3).is_some_and(|c| c.is_ascii_digit())
}

fn parse_block(lines: &[&str], start: usize, milestones: &[Milestone], earlier: &[Task]) -> Result<(Task, usize), ParseError> {
    let lineno = start + 1;
    let err = |line: usize, message: String| ParseError { line, message };
    let header = lines.get(start).copied().unwrap_or("");
    let rest = header.strip_prefix("- ").unwrap_or(header);
    let (mark, bold) = rest.split_once(' ').unwrap_or(("", rest));
    let done = match mark {
        "⬜" => false,
        "✅" => true,
        other => return Err(err(lineno, format!("checkbox must be ⬜ or ✅, found `{other}`"))),
    };
    let inner = bold
        .strip_prefix("**")
        .and_then(|s| s.strip_suffix("**"))
        .ok_or_else(|| err(lineno, "the header must be `- ⬜ **<id> <title>**` with nothing after the closing `**`".into()))?;
    let (id, title) = inner.split_once(' ').ok_or_else(|| err(lineno, "the header needs an id and a title".into()))?;
    let (milestone, _) = split_id(id).ok_or_else(|| err(lineno, format!("`{id}` is not a task id (M<n>.<n>, M<n>.A, M<n>.R)")))?;
    if !milestones.iter().any(|m| m.number == milestone) {
        return Err(err(lineno, format!("{id}: milestone M{milestone} is not in the Overview table")));
    }
    if earlier.iter().any(|t| t.id == id) {
        return Err(err(lineno, format!("{id} is defined twice")));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err(err(lineno, format!("{id} has no title")));
    }

    // Fields.
    let mut fields: Vec<(String, String, usize)> = Vec::new();
    let mut i = start + 1;
    while let Some(line) = lines.get(i) {
        if let Some(field) = line.strip_prefix("  - ") {
            let (key, value) = field.split_once(':').ok_or_else(|| err(i + 1, format!("{id}: a field line must be `  - key: value`")))?;
            let key = key.trim().to_string();
            if fields.iter().any(|(k, _, _)| *k == key) {
                return Err(err(i + 1, format!("{id}: field `{key}` appears twice")));
            }
            fields.push((key, value.trim().to_string(), i + 1));
        } else if line.starts_with("    ") && !line.trim().is_empty() {
            let Some(last) = fields.last_mut() else {
                return Err(err(i + 1, format!("{id}: continuation line before any field")));
            };
            last.1.push(' ');
            last.1.push_str(line.trim());
        } else {
            break;
        }
        i += 1;
    }
    let get = |key: &str| fields.iter().find(|(k, _, _)| k == key).map(|(_, v, l)| (v.clone(), *l));
    for (key, _, line) in &fields {
        if !matches!(key.as_str(), "after" | "owner" | "files" | "scope" | "done when") {
            return Err(err(*line, format!("{id}: unknown field `{key}` (after, owner, files, scope, done when)")));
        }
    }
    let require = |key: &str| get(key).ok_or_else(|| err(lineno, format!("{id}: missing field `{key}`")));
    let (after_text, after_line) = require("after")?;
    let (files_text, files_line) = require("files")?;
    let (scope, scope_line) = require("scope")?;
    let (done_when, done_line) = require("done when")?;
    for (value, line, key) in [(&scope, scope_line, "scope"), (&done_when, done_line, "done when")] {
        if value.is_empty() {
            return Err(err(line, format!("{id}: `{key}` is empty")));
        }
    }
    let owner = match get("owner") {
        None => Owner::Agent,
        Some((v, _)) if v == "agent" => Owner::Agent,
        Some((v, _)) if v == "lead" => Owner::Lead,
        Some((v, _)) if v == "maintainer" => Owner::Maintainer,
        Some((v, l)) => return Err(err(l, format!("{id}: owner must be `agent` or `maintainer`, found `{v}`"))),
    };

    let mut after = Vec::new();
    if !is_none(&after_text) {
        let known: BTreeSet<&str> = earlier.iter().map(|t| t.id.as_str()).collect();
        for dep in after_text.split(',').map(str::trim) {
            if split_id(dep).is_none() {
                return Err(err(after_line, format!("{id}: `{dep}` in `after` is not a task id")));
            }
            if !known.contains(dep) {
                return Err(err(after_line, format!("{id}: `after` names {dep}, which is not defined earlier in the file")));
            }
            after.push(dep.to_string());
        }
    }

    let mut files = Vec::new();
    if !is_none(&files_text) {
        for item in files_text.split(", ").map(str::trim) {
            let path = item
                .strip_prefix('`')
                .and_then(|s| s.strip_suffix('`'))
                .filter(|p| !p.is_empty() && !p.contains('`'))
                .ok_or_else(|| err(files_line, format!("{id}: each entry in `files` must be one backticked path, found `{item}`")))?;
            files.push(path.to_string());
        }
    }

    let task = Task {
        id: id.to_string(),
        milestone,
        title: title.to_string(),
        done,
        after,
        owner,
        files,
        files_text: if is_none(&files_text) { "—".into() } else { files_text },
        scope,
        done_when,
        line: lineno,
    };
    Ok((task, i))
}

fn is_none(value: &str) -> bool {
    matches!(value.trim(), "—" | "-" | "")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "| Phase | # | Milestone | One-line goal | Status |\n|---|---|---|---|---|\n| F | M0 | Skeleton | an empty workspace | ⬜ |\n| | M1 | Star | star a photo | ⬜ |\n\n";

    fn doc(blocks: &str) -> String {
        format!("{HEAD}{blocks}")
    }

    #[test]
    fn every_block_of_the_real_roadmap_parses() {
        let text = std::fs::read_to_string(crate::workspace_root().join("docs/roadmap.md")).unwrap();
        let roadmap = parse(&text).unwrap();
        // 38 milestones combined into 16 (maintainer decision, 2026-10-10).
        assert_eq!(roadmap.milestones.len(), 16);
        // Check-ins after M1 are the lead's and block nothing.
        for id in ["M2.R", "M3.A", "M3.R"] {
            assert_eq!(roadmap.task(id).map(|t| t.owner), Some(Owner::Lead), "{id}");
        }
        let m0: Vec<_> = roadmap.tasks.iter().filter(|t| t.milestone == 0).map(|t| t.id.as_str()).collect();
        assert_eq!(m0, ["M0.1", "M0.2", "M0.10", "M0.3", "M0.4", "M0.5", "M0.6", "M0.7", "M0.8", "M0.9", "M0.R"]);
        assert_eq!(roadmap.tasks.iter().filter(|t| t.milestone == 1).count(), 22);
        let r = roadmap.task("M0.R").unwrap();
        assert!(r.is_gate() && r.owner == Owner::Maintainer);
        assert_eq!(roadmap.task("M0.5").unwrap().after, ["M0.3", "M0.4"]);
        assert_eq!(roadmap.task("M1.A").unwrap().files, Vec::<String>::new());
        assert_eq!(roadmap.milestone(1).unwrap().title(), "M1 Star a photo, end to end");
    }

    #[test]
    fn block_fields_and_continuations() {
        let text = doc(
            "- ⬜ **M0.1 Skeleton `ws`**\n  - after: —\n  - files: `Cargo.toml`, `crates/**`\n  - scope: one\n    two\n  - done when: it builds.\n\n- ✅ **M0.R Review**\n  - after: M0.1\n  - owner: maintainer\n  - files: —\n  - scope: s\n  - done when: d\n",
        );
        let r = parse(&text).unwrap();
        let t = &r.tasks[0];
        assert_eq!((t.id.as_str(), t.title.as_str(), t.line, t.done), ("M0.1", "Skeleton `ws`", 6, false));
        assert_eq!(t.files, ["Cargo.toml", "crates/**"]);
        assert_eq!(t.scope, "one two");
        assert_eq!(t.issue_title(), "M0.1 Skeleton `ws`");
        assert_eq!(t.branch(), "task/M0.1-skeleton-ws");
        assert!(r.tasks[1].done && r.tasks[1].is_gate());
    }

    #[test]
    fn malformed_blocks_fail_with_their_line_number() {
        let cases = [
            ("- 🚧 **M0.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n", 6, "checkbox"),
            ("- ⬜ **M0.1 X**\n  - after: —\n  - files: —\n  - done when: d\n", 6, "missing field `scope`"),
            ("- ⬜ **M0.1 X**\n  - after: M0.2\n  - files: —\n  - scope: s\n  - done when: d\n", 7, "not defined earlier"),
            ("- ⬜ **M0.1 X**\n  - after: —\n  - files: Cargo.toml\n  - scope: s\n  - done when: d\n", 8, "backticked"),
            ("- ⬜ **M0.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n  - colour: red\n", 11, "unknown field"),
            ("- ⬜ **M9.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n", 6, "not in the Overview"),
            ("- ⬜ **M0.1 X** extra\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n", 6, "header"),
            ("- ⬜ **M0.1 X**\n  - after: —\n  - owner: robot\n  - files: —\n  - scope: s\n  - done when: d\n", 8, "owner"),
        ];
        for (block, line, needle) in cases {
            let e = parse(&doc(block)).unwrap_err();
            assert_eq!(e.line, line, "{e}");
            assert!(e.message.contains(needle), "{e}");
            assert!(e.to_string().starts_with(&format!("docs/roadmap.md:{line}: ")));
        }
        let twice = "- ⬜ **M0.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n- ⬜ **M0.1 Y**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n";
        assert_eq!(parse(&doc(twice)).unwrap_err().line, 11);
    }

    #[test]
    fn ids() {
        assert_eq!(split_id("M1.4"), Some((1, "4")));
        assert_eq!(split_id("M12.R"), Some((12, "R")));
        assert_eq!(split_id("M1.AB"), None);
        assert_eq!(split_id("M.1"), None);
        assert_eq!(split_id("X1.1"), None);
    }
}
