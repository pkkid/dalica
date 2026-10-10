//! `cargo xtask parity [--write]`: `docs/parity.md` is well formed and its evidence exists.
//!
//! - Feature rows (`| Id | Feature | Tier | Status | Evidence / notes |`): ids unique and in a known
//!   area, tiers `P0 P1 P2 OOS —`, statuses `✅ 🟡 ⬜ 🚫` (🚫 only with `OOS`).
//! - Menu rows (`| Menu | Ids | Status |`): a known status.
//! - Evidence: every `cmd:<id>` is a registered command (from `dalica-cli commands`), every
//!   backticked path exists (bare `*.md` names are looked up in `docs/`).
//! - The summary between the `parity:summary` markers matches the rows; `--write` refreshes it.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

pub const AREAS: &[&str] = &["LIB", "ORG", "IMP", "PPL", "GEO", "SRCH", "FILE", "EDIT", "FX", "VIEW", "OUT", "DATA", "PREF", "SYS", "MENU", "KEY"];
pub const TIERS: &[&str] = &["P0", "P1", "P2", "OOS", "—"];
pub const STATUSES: [&str; 4] = ["✅", "🟡", "⬜", "🚫"];
const START: &str = "<!-- parity:summary -->";
const END: &str = "<!-- /parity:summary -->";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub section: String,
    pub id: String,
    pub tier: Option<String>,
    pub status: String,
    pub evidence: String,
}

/// Rows of every table under a `## X. …` section heading.
pub fn parse_rows(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut section = String::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(h) = line.strip_prefix("## ") {
            let is_area = h.split_once(". ").is_some_and(|(letter, _)| letter.len() == 1 && letter.chars().all(|c| c.is_ascii_uppercase()));
            section = if is_area { strip_parentheticals(h) } else { String::new() };
            continue;
        }
        if section.is_empty() || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        let first = cells.first().copied().unwrap_or("");
        if matches!(first, "Id" | "Menu") || first.starts_with("---") {
            continue;
        }
        let row = match cells.len() {
            5 => Row {
                line: i + 1,
                section: section.clone(),
                id: first.to_string(),
                tier: cells.get(2).map(|s| s.to_string()),
                status: cells.get(3).copied().unwrap_or("").to_string(),
                evidence: cells.get(4).copied().unwrap_or("").to_string(),
            },
            3 => Row {
                line: i + 1,
                section: section.clone(),
                id: format!("MENU-{first}"),
                tier: None,
                status: cells.get(2).copied().unwrap_or("").to_string(),
                evidence: String::new(),
            },
            n => Row {
                line: i + 1,
                section: section.clone(),
                id: first.to_string(),
                tier: None,
                status: format!("<{n} columns>"),
                evidence: String::new(),
            },
        };
        rows.push(row);
    }
    rows
}

/// `G. Preferences (PREF) and system (SYS)` → `G. Preferences and system`.
fn strip_parentheticals(heading: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in heading.chars() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn id_ok(id: &str) -> bool {
    let Some((area, rest)) = id.split_once('-') else { return false };
    AREAS.contains(&area)
        && !rest.is_empty()
        && rest
            .split(" / ")
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '.'))
}

/// Backticked items in the evidence that name files: they contain `/` or end in a source/doc
/// extension, do not start with `.`, and hold no wildcard. `path::test_name` cites a test.
fn cited_paths(evidence: &str) -> Vec<String> {
    evidence
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.starts_with("cmd:") && !s.starts_with('.') && !s.contains('*') && !s.contains(' '))
        .map(|s| s.split("::").next().unwrap_or(s).to_string())
        .filter(|s| s.contains('/') || [".rs", ".md", ".toml", ".yml", ".jsonl"].iter().any(|e| s.ends_with(e)))
        .collect()
}

fn cited_commands(evidence: &str) -> Vec<String> {
    evidence
        .split(|c: char| c == '`' || c.is_whitespace() || c == ',' || c == ';')
        .filter_map(|w| w.strip_prefix("cmd:"))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn check(root: &Path, text: &str, commands: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let rows = parse_rows(text);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for r in &rows {
        let at = format!("docs/parity.md:{}", r.line);
        if !seen.insert(r.id.as_str()) {
            out.push(format!("{at}: duplicate id `{}`", r.id));
        }
        if r.tier.is_some() && !id_ok(&r.id) {
            out.push(format!("{at}: id `{}` must be <AREA>-<NAME> with AREA one of {}", r.id, AREAS.join(" ")));
        }
        if !STATUSES.contains(&r.status.as_str()) {
            out.push(format!("{at}: unknown status `{}` (✅ 🟡 ⬜ 🚫)", r.status));
        }
        if let Some(tier) = &r.tier {
            if !TIERS.contains(&tier.as_str()) {
                out.push(format!("{at}: unknown tier `{tier}` (P0 P1 P2 OOS —)"));
            }
            if (r.status == "🚫") != (tier == "OOS") {
                out.push(format!("{at}: 🚫 goes with tier OOS and only with it"));
            }
        }
        for cmd in cited_commands(&r.evidence) {
            if !commands.contains(&cmd) {
                out.push(format!("{at}: `cmd:{cmd}` is not a registered command (dalica-cli commands)"));
            }
        }
        for path in cited_paths(&r.evidence) {
            let exists = root.join(&path).exists() || (!path.contains('/') && root.join("docs").join(&path).exists());
            if !exists {
                out.push(format!("{at}: cited path `{path}` does not exist"));
            }
        }
    }
    if rows.is_empty() {
        out.push("docs/parity.md: no rows found".into());
    }
    out
}

/// The summary table for the rows: one line per section and a total.
pub fn summary(text: &str) -> String {
    let rows = parse_rows(text);
    let mut sections: Vec<String> = Vec::new();
    for r in &rows {
        if !sections.contains(&r.section) {
            sections.push(r.section.clone());
        }
    }
    let count = |rows: &[&Row]| STATUSES.map(|s| rows.iter().filter(|r| r.status == s).count());
    let mut out = String::from("| Section | ✅ | 🟡 | ⬜ | 🚫 |\n|---|---:|---:|---:|---:|\n");
    for s in &sections {
        let in_section: Vec<&Row> = rows.iter().filter(|r| &r.section == s).collect();
        let [a, b, c, d] = count(&in_section);
        out.push_str(&format!("| {s} | {a} | {b} | {c} | {d} |\n"));
    }
    let all: Vec<&Row> = rows.iter().collect();
    let [a, b, c, d] = count(&all);
    out.push_str(&format!("| **All** | {a} | {b} | {c} | {d} |\n"));
    out
}

/// `text` with the summary block refreshed; `None` if the markers are missing.
pub fn with_summary(text: &str) -> Option<String> {
    let start = text.find(START)?;
    let end = text.find(END)?;
    if end < start {
        return None;
    }
    Some(format!("{}{START}\n{}{}", text.get(..start)?, summary(text), text.get(end..)?))
}

/// Ids of registered commands, from `dalica-cli commands` (a JSON array of objects with `id`, or of
/// strings). An empty registry before the CLI prints anything.
pub fn registered_commands(root: &Path) -> Result<BTreeSet<String>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args(["run", "--quiet", "-p", "dalica-cli", "--", "commands"])
        .output()
        .map_err(|e| format!("cannot run dalica-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("dalica-cli commands failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return Ok(BTreeSet::new());
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("dalica-cli commands printed invalid JSON: {e}"))?;
    Ok(v.as_array().into_iter().flatten().filter_map(|c| c.as_str().or_else(|| c.get("id").and_then(|i| i.as_str())).map(str::to_string)).collect())
}

pub fn run(root: &Path, write: bool) -> Result<(), String> {
    let path = root.join("docs").join("parity.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read docs/parity.md: {e}"))?;
    let commands = registered_commands(root)?;
    let mut problems = check(root, &text, &commands);
    let refreshed = with_summary(&text).ok_or("docs/parity.md: the parity:summary markers are missing")?;
    if refreshed != text {
        if write {
            std::fs::write(&path, &refreshed).map_err(|e| format!("cannot write docs/parity.md: {e}"))?;
            println!("parity: summary refreshed");
        } else {
            problems.push("docs/parity.md: the summary is stale; run `cargo xtask parity --write` and commit it".into());
        }
    }
    for p in &problems {
        eprintln!("parity: {p}");
    }
    if problems.is_empty() {
        println!("parity: ok ({} rows, {} registered commands)", parse_rows(&refreshed).len(), commands.len());
        Ok(())
    } else {
        Err(format!("parity: {} problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# P\n\n## Summary\n\n<!-- parity:summary -->\nold\n<!-- /parity:summary -->\n\n## A. Library view (LIB)\n\n\
| Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n\
| LIB-ONE | one | P0 | ⬜ | ref 02 §6 |\n| LIB-TWO | two | P1 | ✅ | `cmd:photo.star.toggle`, `docs/parity.md` |\n\
| LIB-OUT | out | OOS | 🚫 | dropped |\n\n## H. Menus (MENU)\n\n| Menu | Ids | Status |\n|---|---|---|\n| File | `app.quit` | ⬜ |\n";

    fn cmds(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn root() -> std::path::PathBuf {
        crate::workspace_root()
    }

    #[test]
    fn a_well_formed_doc_passes() {
        assert_eq!(check(&root(), DOC, &cmds(&["photo.star.toggle"])), Vec::<String>::new());
        let rows = parse_rows(DOC);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3].id, "MENU-File");
        assert_eq!(rows[0].section, "A. Library view");
    }

    #[test]
    fn rejects_a_duplicate_id() {
        let doc = DOC.replace("LIB-TWO", "LIB-ONE");
        let p = check(&root(), &doc, &cmds(&["photo.star.toggle"]));
        assert_eq!(p, ["docs/parity.md:14: duplicate id `LIB-ONE`"]);
    }

    #[test]
    fn rejects_an_unknown_status_and_tier() {
        let doc = DOC.replace("| P0 | ⬜ |", "| P7 | 🚧 |");
        let p = check(&root(), &doc, &cmds(&["photo.star.toggle"]));
        assert!(p.iter().any(|x| x.contains("unknown status `🚧`")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("unknown tier `P7`")), "{p:?}");
    }

    #[test]
    fn rejects_unregistered_commands_missing_paths_and_bad_ids() {
        let doc = DOC.replace("`docs/parity.md`", "`crates/nowhere/src/lib.rs`, `menus-and-shortcuts.md`").replace("LIB-ONE", "lib-one");
        let p = check(&root(), &doc, &cmds(&[]));
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p.iter().any(|x| x.contains("`cmd:photo.star.toggle` is not a registered command")));
        assert!(p.iter().any(|x| x.contains("`crates/nowhere/src/lib.rs` does not exist")));
        assert!(p.iter().any(|x| x.contains("id `lib-one`")));
    }

    #[test]
    fn write_twice_leaves_no_diff() {
        let once = with_summary(DOC).unwrap();
        assert_eq!(with_summary(&once).unwrap(), once);
        assert!(once.contains("| A. Library view | 1 | 0 | 1 | 1 |\n| H. Menus | 0 | 0 | 1 | 0 |\n| **All** | 1 | 0 | 2 | 1 |\n"), "{once}");
    }

    #[test]
    fn the_real_parity_doc_parses_cleanly() {
        let text = std::fs::read_to_string(root().join("docs/parity.md")).unwrap();
        let problems = check(&root(), &text, &cmds(&[]));
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(parse_rows(&text).len() > 100);
    }
}
