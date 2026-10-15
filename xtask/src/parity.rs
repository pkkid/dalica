//! `cargo xtask parity [--write]`: `docs/parity.md` is well formed and its evidence exists.
//!
//! - Feature rows (`| Id | Feature | Tier | Status | Evidence / notes |`): ids unique and in a known
//!   area, tiers `P0 P1 P2 OOS —`, statuses `✅ 🟡 ⬜ 🚫` (🚫 only with `OOS`).
//! - Menu rows (`| Menu | Ids | Status |`): a known status.
//! - Evidence: every `cmd:<id>` is a registered command (from `dalica-cli commands`), every
//!   backticked path exists (bare `*.md` names are looked up in `docs/`).
//! - The summary between the `parity:summary` markers matches the rows; `--write` refreshes it.
//! - With the registry (`cargo xtask parity`, the review gate): section I's `KEY-*` rows between the
//!   `parity:keys` markers match the keymap ([`keys`]); `--write` regenerates them, and a documented
//!   key whose registered command no context binds fails.
//! - `--p0` lists the P0 rows that are not ✅ and fails while one remains ([`p0`]).
//!
//! `tasks merge` also uses this module to resolve rebase conflicts in the file: the summary is
//! regenerated, and edits to different rows by the two sides are merged row by row.

mod keys;
mod p0;

use std::collections::{BTreeMap, BTreeSet};
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

/// Resolves the merge conflicts in `text` (a conflicted `docs/parity.md`) when every conflict lies
/// inside the generated summary: keeps one side of each conflict, then regenerates the summary
/// from the rows, so the side kept does not matter (`tasks merge`, `docs/task-board.md` §5). A
/// conflict outside the summary (in the rows) is the worker's to resolve and is an error, as is a
/// conflict that touches the summary markers themselves or text without any conflict.
pub fn resolve_summary_conflicts(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut in_summary = false;
    // Inside a conflict: whether the lines still belong to the first side (the one kept).
    let mut conflict: Option<bool> = None;
    let mut conflicts = 0usize;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let bare = line.trim_end_matches(['\n', '\r']);
        let at = format!("docs/parity.md:{}", i.saturating_add(1));
        let marker = bare.trim() == START || bare.trim() == END;
        if let Some(first_side) = conflict.as_mut() {
            if marker {
                return Err(format!("{at}: a conflict touches the summary markers"));
            }
            if bare.starts_with(">>>>>>>") {
                conflict = None;
            } else if bare.starts_with("|||||||") || bare.starts_with("=======") {
                *first_side = false;
            } else if *first_side {
                out.push_str(line);
            }
            continue;
        }
        if bare.starts_with("<<<<<<<") {
            if !in_summary {
                return Err(format!("{at}: a conflict outside the generated summary (in the rows); the worker resolves it"));
            }
            conflict = Some(true);
            conflicts = conflicts.saturating_add(1);
            continue;
        }
        if bare.trim() == START {
            in_summary = true;
        } else if bare.trim() == END {
            in_summary = false;
        }
        out.push_str(line);
    }
    if conflict.is_some() {
        return Err("docs/parity.md: a conflict is not closed".into());
    }
    if conflicts == 0 {
        return Err("docs/parity.md: no conflict markers in the text (a conflict of another kind)".into());
    }
    with_summary(&out).ok_or_else(|| "docs/parity.md: the parity:summary markers are missing".into())
}

/// One version of `docs/parity.md` for [`merge_rows`].
struct Version {
    /// Its lines, without line ends.
    lines: Vec<String>,
    /// Row id → (line index, section).
    rows: BTreeMap<String, (usize, String)>,
    /// Line index → row id.
    by_line: BTreeMap<usize, String>,
    /// Row ids in file order.
    order: Vec<String>,
    crlf: bool,
    final_eol: bool,
}

impl Version {
    fn parse(text: &str, name: &str) -> Result<Version, String> {
        let (mut rows, mut by_line, mut order) = (BTreeMap::new(), BTreeMap::new(), Vec::new());
        for r in parse_rows(text) {
            let index = r.line.saturating_sub(1);
            if rows.insert(r.id.clone(), (index, r.section)).is_some() {
                return Err(format!("docs/parity.md ({name}): row {} appears twice", r.id));
            }
            by_line.insert(index, r.id.clone());
            order.push(r.id);
        }
        Ok(Version {
            lines: text.lines().map(str::to_string).collect(),
            rows,
            by_line,
            order,
            crlf: text.contains("\r\n"),
            final_eol: text.ends_with('\n'),
        })
    }

    /// Row `id` as (section, line).
    fn row(&self, id: &str) -> Option<(&str, &str)> {
        let (index, section) = self.rows.get(id)?;
        Some((section.as_str(), self.lines.get(*index)?.as_str()))
    }

    /// The lines outside the rows and outside the generated summary.
    fn skeleton(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut in_summary = false;
        for (i, line) in self.lines.iter().enumerate() {
            if line.trim() == END {
                in_summary = false;
            }
            if !in_summary && !self.by_line.contains_key(&i) {
                out.push(line.as_str());
            }
            if line.trim() == START {
                in_summary = true;
            }
        }
        out
    }

    /// The ids of the rows `other` also has, in this version's order.
    fn common_order<'a>(&'a self, other: &Version) -> Vec<&'a str> {
        self.order.iter().filter(|id| other.rows.contains_key(*id)).map(String::as_str).collect()
    }

    /// Where row `id` of this version goes in `out` (the merged lines, each with its row id): after
    /// the nearest row above it in the same table that `out` has, else before the nearest one below.
    fn place(&self, id: &str, out: &[(Option<&str>, &str)]) -> Option<usize> {
        let (index, _) = self.rows.get(id)?;
        let pos = |neighbour: &str| out.iter().position(|(r, _)| *r == Some(neighbour));
        let mut i = *index;
        while let Some(prev) = i.checked_sub(1) {
            let Some(neighbour) = self.by_line.get(&prev) else { break };
            if let Some(p) = pos(neighbour) {
                return p.checked_add(1);
            }
            i = prev;
        }
        let mut i = *index;
        loop {
            i = i.checked_add(1)?;
            if let Some(p) = pos(self.by_line.get(&i)?) {
                return Some(p);
            }
        }
    }
}

/// Merges two versions of `docs/parity.md` that both changed `base`, row by row (`tasks merge`,
/// `docs/task-board.md` §5): rows are matched by id, and a row added, changed or deleted on one
/// side only takes that side's version. The result follows the side that changed text outside the
/// rows (or `ours` when neither did), and the summary is regenerated from the merged rows.
///
/// Refused, so the worker resolves it: a row both sides changed differently, both sides changing
/// text outside the rows, the side whose rows are merged in reordering rows or moving one to
/// another section, a new row with no neighbour to place it by, and a duplicate id.
pub fn merge_rows(base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    let (b, o, t) = (Version::parse(base, "base")?, Version::parse(ours, "ours")?, Version::parse(theirs, "theirs")?);
    let ids: BTreeSet<&str> = b.rows.keys().chain(o.rows.keys()).chain(t.rows.keys()).map(String::as_str).collect();
    let mut merged: BTreeMap<&str, Option<(&str, &str)>> = BTreeMap::new();
    for id in ids {
        let (rb, ro, rt) = (b.row(id), o.row(id), t.row(id));
        for side in [ro, rt] {
            if let (Some((was, _)), Some((now, _))) = (rb, side)
                && was != now
            {
                return Err(format!("docs/parity.md: row {id} moved from section `{was}` to `{now}`; the worker resolves it"));
            }
        }
        let pick = if ro == rt {
            ro
        } else if ro == rb {
            rt
        } else if rt == rb {
            ro
        } else {
            return Err(format!("docs/parity.md: both sides changed row {id}; the worker resolves it"));
        };
        merged.insert(id, pick);
    }

    // `x` is the template, `y` the side whose row changes are merged into it.
    let clean = |v: &Version| v.skeleton() == b.skeleton() && v.common_order(&b) == b.common_order(v);
    let (x, y) = if clean(&t) {
        (&o, &t)
    } else if clean(&o) {
        (&t, &o)
    } else {
        return Err("docs/parity.md: both sides changed text outside the rows (or reordered rows); the worker resolves it".into());
    };
    let mut out: Vec<(Option<&str>, &str)> = Vec::with_capacity(x.lines.len().saturating_add(y.rows.len()));
    for (i, line) in x.lines.iter().enumerate() {
        match x.by_line.get(&i) {
            Some(id) => {
                if let Some(Some((_, text))) = merged.get(id.as_str()) {
                    out.push((Some(id.as_str()), text));
                }
            }
            None => out.push((None, line.as_str())),
        }
    }
    for id in &y.order {
        let Some(Some((_, text))) = merged.get(id.as_str()).copied() else { continue };
        if x.rows.contains_key(id) {
            continue;
        }
        let at =
            y.place(id, &out).ok_or_else(|| format!("docs/parity.md: no neighbouring row to place the new row {id} by; the worker resolves it"))?;
        out.insert(at, (Some(id.as_str()), text));
    }

    let eol = if x.crlf { "\r\n" } else { "\n" };
    let mut text = out.iter().map(|(_, line)| *line).collect::<Vec<_>>().join(eol);
    if x.final_eol {
        text.push_str(eol);
    }
    let text = with_summary(&text).ok_or("docs/parity.md: the parity:summary markers are missing")?;

    // Every row ends up as decided above, in its section: anything else is a send-back.
    let result = Version::parse(&text, "merged")?;
    let want: BTreeMap<&str, (&str, &str)> = merged.iter().filter_map(|(id, row)| row.map(|r| (*id, r))).collect();
    let got: BTreeMap<&str, (&str, &str)> = result.order.iter().filter_map(|id| result.row(id).map(|r| (id.as_str(), r))).collect();
    if got != want {
        return Err("docs/parity.md: the merged rows do not match both sides' changes; the worker resolves it".into());
    }
    Ok(text)
}

/// The registry as `dalica-cli commands` prints it: one JSON object per command (`id`, `keys`, …).
/// Empty before the CLI prints anything.
pub fn registry_list(root: &Path) -> Result<Vec<serde_json::Value>, String> {
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
        return Ok(Vec::new());
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("dalica-cli commands printed invalid JSON: {e}"))?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

/// Ids of the commands in a registry listing (objects with `id`, or strings).
pub fn command_ids(list: &[serde_json::Value]) -> BTreeSet<String> {
    list.iter().filter_map(|c| c.as_str().or_else(|| c.get("id").and_then(|i| i.as_str())).map(str::to_string)).collect()
}

/// `text` with section I's `KEY-*` rows regenerated from the registry `list`, and the problems the
/// rows show (a documented key whose registered command no context binds). `menus` and `editing`
/// are `menus-and-shortcuts.md` and `editing.md`.
pub fn with_key_rows(
    text: &str,
    list: &[serde_json::Value],
    menus: &str,
    editing: &str,
    roadmap: Option<&crate::tasks::roadmap::Roadmap>,
) -> Result<(String, Vec<String>), String> {
    let documented = keys::documented(menus, editing);
    let bindings = keys::bindings(list);
    let registered = command_ids(list);
    let menu_keys = keys::menu_keys(menus);
    let p0_cited = keys::p0_cited(&parse_rows(text));
    let rows = keys::rows(&keys::Inputs {
        documented: &documented,
        bindings: &bindings,
        registered: &registered,
        menu_keys: &menu_keys,
        p0_cited: &p0_cited,
        roadmap,
    });
    let problems = rows.iter().filter_map(|r| r.problem.as_ref()).map(|p| format!("docs/parity.md: {p}")).collect();
    let text = keys::with_block(text, &keys::block(&rows)).ok_or("docs/parity.md: the parity:keys markers are missing")?;
    Ok((text, problems))
}

/// `cargo xtask parity [--write] [--p0]`.
pub fn main(root: &Path, args: &[String]) -> Result<(), String> {
    if let Some(a) = args.iter().find(|a| !["--write", "--p0"].contains(&a.as_str())) {
        return Err(format!("parity: unknown argument `{a}` (usage: cargo xtask parity [--write] [--p0])"));
    }
    let has = |f: &str| args.iter().any(|a| a == f);
    match (has("--p0"), has("--write")) {
        (true, true) => Err("parity: --p0 reads the rows as they are; run --write first".into()),
        (true, false) => p0::run(root),
        (false, write) => run(root, write, true),
    }
}

/// Checks `docs/parity.md` (with `write`, refreshes its generated parts). With `registry`, every
/// cited `cmd:` id is checked against the registered commands and the `KEY-*` rows against the
/// keymap, which builds and runs `dalica-cli` (the review gate); without it, the rows and their
/// cited paths only (a task's gate).
pub fn run(root: &Path, write: bool, registry: bool) -> Result<(), String> {
    let path = root.join("docs").join("parity.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read docs/parity.md: {e}"))?;
    let mut problems = Vec::new();
    let (commands, generated) = if registry {
        let list = registry_list(root)?;
        let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("cannot read {p}: {e}"));
        let roadmap = crate::tasks::load_roadmap(root).ok();
        let (generated, key_problems) =
            with_key_rows(&text, &list, &read("docs/menus-and-shortcuts.md")?, &read("docs/editing.md")?, roadmap.as_ref())?;
        problems.extend(key_problems);
        (command_ids(&list), generated)
    } else {
        (parse_rows(&text).iter().flat_map(|r| cited_commands(&r.evidence)).collect(), text.clone())
    };
    problems.extend(check(root, &generated, &commands));
    let refreshed = with_summary(&generated).ok_or("docs/parity.md: the parity:summary markers are missing")?;
    if refreshed != text {
        if write {
            std::fs::write(&path, &refreshed).map_err(|e| format!("cannot write docs/parity.md: {e}"))?;
            println!("parity: {} refreshed", if generated != text { "KEY rows and summary" } else { "summary" });
        } else if generated != text {
            problems.push("docs/parity.md: the KEY rows are stale; run `cargo xtask parity --write` and commit them".into());
        } else {
            problems.push("docs/parity.md: the summary is stale; run `cargo xtask parity --write` and commit it".into());
        }
    }
    for p in &problems {
        eprintln!("parity: {p}");
    }
    if problems.is_empty() {
        if registry {
            println!("parity: ok ({} rows, {} registered commands)", parse_rows(&refreshed).len(), commands.len());
        } else {
            println!("parity: ok ({} rows; cited commands and KEY rows are checked in the review gate)", parse_rows(&refreshed).len());
        }
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

    /// `DOC` with its summary regenerated and a conflict in it (both sides of a rebase).
    fn summary_conflict(style_base: bool) -> String {
        let doc = with_summary(DOC).unwrap();
        let line = "| A. Library view | 1 | 0 | 1 | 1 |\n";
        let base = if style_base { "||||||| base\n| A. Library view | 0 | 0 | 2 | 1 |\n" } else { "" };
        doc.replace(
            line,
            &format!(
                "<<<<<<< HEAD\n| A. Library view | 2 | 0 | 0 | 1 |\n{base}=======\n| A. Library view | 9 | 9 | 9 | 9 |\n>>>>>>> abc (M9.1: x)\n"
            ),
        )
    }

    #[test]
    fn a_conflict_only_in_the_summary_is_resolved_by_regenerating_it() {
        for style_base in [false, true] {
            let resolved = resolve_summary_conflicts(&summary_conflict(style_base)).unwrap();
            assert_eq!(resolved, with_summary(DOC).unwrap(), "the rows decide, not either side");
            assert!(!resolved.contains("<<<<<<<") && !resolved.contains(">>>>>>>"));
        }
        // CRLF line ends are conflicts too.
        assert!(resolve_summary_conflicts(&summary_conflict(false).replace('\n', "\r\n")).is_ok());
    }

    #[test]
    fn a_conflict_in_the_rows_is_refused() {
        let doc = with_summary(DOC).unwrap();
        let row = "| LIB-ONE | one | P0 | ⬜ | ref 02 §6 |\n";
        let rows = doc.replace(row, "<<<<<<< HEAD\n| LIB-ONE | one | P0 | ✅ | a |\n=======\n| LIB-ONE | one | P0 | 🟡 | b |\n>>>>>>> abc\n");
        let err = resolve_summary_conflicts(&rows).unwrap_err();
        assert!(err.contains("outside the generated summary") && err.contains("docs/parity.md:"), "{err}");
        // A row conflict next to a summary conflict is still refused.
        let both = summary_conflict(false).replace(row, &format!("<<<<<<< HEAD\n{row}=======\nx\n>>>>>>> abc\n"));
        assert!(resolve_summary_conflicts(&both).unwrap_err().contains("in the rows"));
        // A conflict that swallows a marker, one left open, and no conflict at all.
        let swallowed = doc.replace(&format!("{END}\n"), &format!("<<<<<<< HEAD\n{END}\n=======\n>>>>>>> abc\n"));
        assert!(resolve_summary_conflicts(&swallowed).unwrap_err().contains("markers"), "{swallowed}");
        let open = summary_conflict(false).replace(">>>>>>> abc (M9.1: x)\n", "");
        assert!(resolve_summary_conflicts(&open).is_err());
        assert!(resolve_summary_conflicts(&doc).unwrap_err().contains("no conflict markers"));
    }

    /// Three neighbouring rows in one table and a menu table, with the summary generated.
    fn rows_doc(rows: &[(&str, &str)]) -> String {
        let mut text = String::from(
            "# P\n\n<!-- parity:summary -->\n<!-- /parity:summary -->\n\n## A. Library view (LIB)\n\n\
             | Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n",
        );
        for (id, status) in rows {
            text.push_str(&format!("| {id} | f | P0 | {status} | note |\n"));
        }
        text.push_str("\n## H. Menus (MENU)\n\n| Menu | Ids | Status |\n|---|---|---|\n| File | `app.quit` | ⬜ |\n");
        with_summary(&text).unwrap()
    }

    const BASE: &[(&str, &str)] = &[("LIB-ONE", "⬜"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜")];

    #[test]
    fn neighbouring_row_edits_merge_with_both_rows_kept() {
        let ours = rows_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜")]);
        let theirs = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]);
        let merged = merge_rows(&rows_doc(BASE), &ours, &theirs).unwrap();
        assert_eq!(merged, rows_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]));
        assert!(merged.contains("| A. Library view | 1 | 1 | 1 | 0 |\n"), "the summary counts both: {merged}");
        // Symmetric.
        assert_eq!(merge_rows(&rows_doc(BASE), &theirs, &ours).unwrap(), merged);
    }

    #[test]
    fn added_and_deleted_rows_merge_next_to_edits() {
        // Ours adds a row after LIB-ONE and edits LIB-THREE; theirs deletes LIB-TWO and adds a first
        // and a last row.
        let ours = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-NEW", "🟡"), ("LIB-TWO", "⬜"), ("LIB-THREE", "✅")]);
        let theirs = rows_doc(&[("LIB-ZERO", "⬜"), ("LIB-ONE", "⬜"), ("LIB-THREE", "⬜"), ("LIB-FOUR", "⬜")]);
        let want = rows_doc(&[("LIB-ZERO", "⬜"), ("LIB-ONE", "⬜"), ("LIB-NEW", "🟡"), ("LIB-THREE", "✅"), ("LIB-FOUR", "⬜")]);
        assert_eq!(merge_rows(&rows_doc(BASE), &ours, &theirs).unwrap(), want);
        assert_eq!(merge_rows(&rows_doc(BASE), &theirs, &ours).unwrap(), want);
        // Both sides adding the same row, or deleting the same one, is one change.
        let both = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-THREE", "⬜"), ("LIB-FOUR", "⬜")]);
        assert_eq!(merge_rows(&rows_doc(BASE), &both, &both).unwrap(), both);
    }

    #[test]
    fn text_outside_the_rows_follows_the_side_that_changed_it() {
        let ours = rows_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜")]).replace("# P\n", "# Parity, renamed\n");
        let theirs = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]);
        let want = rows_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]).replace("# P\n", "# Parity, renamed\n");
        assert_eq!(merge_rows(&rows_doc(BASE), &ours, &theirs).unwrap(), want);
        assert_eq!(merge_rows(&rows_doc(BASE), &theirs, &ours).unwrap(), want);
        // A stale summary on either side does not matter: it is regenerated.
        let stale = theirs.replace("| **All** | 0 | 1 | 3 | 0 |", "| **All** | 9 | 9 | 9 | 9 |");
        assert_ne!(stale, theirs);
        assert_eq!(merge_rows(&rows_doc(BASE), &ours, &stale).unwrap(), want);
    }

    #[test]
    fn an_edit_of_the_same_row_by_both_sides_is_refused() {
        let base = rows_doc(BASE);
        let ours = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "✅"), ("LIB-THREE", "⬜")]);
        let theirs = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]);
        let err = merge_rows(&base, &ours, &theirs).unwrap_err();
        assert!(err.contains("both sides changed row LIB-TWO"), "{err}");
        // Deleted on one side, changed on the other; added by both with different text.
        let deleted = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-THREE", "⬜")]);
        assert!(merge_rows(&base, &deleted, &theirs).unwrap_err().contains("row LIB-TWO"));
        let added = |s| rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜"), ("LIB-FOUR", s)]);
        assert!(merge_rows(&base, &added("✅"), &added("🟡")).unwrap_err().contains("row LIB-FOUR"));
    }

    #[test]
    fn what_cannot_be_merged_by_row_is_refused() {
        let base = rows_doc(BASE);
        let edit = rows_doc(&[("LIB-ONE", "✅"), ("LIB-TWO", "⬜"), ("LIB-THREE", "⬜")]);
        let retitled = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "🟡"), ("LIB-THREE", "⬜")]).replace("# P\n", "# Q\n");
        // Both sides change text outside the rows.
        let err = merge_rows(&base, &edit.replace("# P\n", "# R\n"), &retitled).unwrap_err();
        assert!(err.contains("outside the rows"), "{err}");
        // Rows reordered by both sides (one alone would be the template).
        let swapped = rows_doc(&[("LIB-TWO", "⬜"), ("LIB-ONE", "⬜"), ("LIB-THREE", "⬜")]);
        assert!(merge_rows(&base, &swapped, &retitled).unwrap_err().contains("reordered"));
        // A row moved to another section.
        let moved = rows_doc(&[("LIB-ONE", "⬜"), ("LIB-TWO", "⬜")]).replace("| File |", "| LIB-THREE | f | P0 | ⬜ | note |\n| File |");
        assert!(merge_rows(&base, &edit, &moved).unwrap_err().contains("moved from section"));
        // A duplicate id.
        let twice = edit.replace("| LIB-THREE |", "| LIB-ONE |");
        assert!(merge_rows(&base, &twice, &edit).unwrap_err().contains("appears twice"));
        // A new row in a table that had none: no neighbour to place it by.
        let empty_table = "\n## B. Organizing (ORG)\n\n| Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n";
        let with = |doc: &str, rows: &str| with_summary(&format!("{doc}{empty_table}{rows}")).unwrap();
        let err = merge_rows(&with(&base, ""), &with(&edit, ""), &with(&base, "| ORG-NEW | f | P1 | ⬜ | n |\n")).unwrap_err();
        assert!(err.contains("no neighbouring row") && err.contains("ORG-NEW"), "{err}");
        // Missing markers.
        let bare = base.replace(START, "").replace(END, "");
        let one = bare.replace("| LIB-ONE | f | P0 | ⬜ |", "| LIB-ONE | f | P0 | ✅ |");
        let two = bare.replace("| LIB-TWO | f | P0 | ⬜ |", "| LIB-TWO | f | P0 | 🟡 |");
        assert!(merge_rows(&bare, &one, &two).unwrap_err().contains("markers"));
    }

    #[test]
    fn the_real_parity_doc_parses_cleanly() {
        let text = std::fs::read_to_string(root().join("docs/parity.md")).unwrap();
        // Cited commands are checked against the real registry by `cargo xtask parity` (a ci step);
        // here every cited id counts as registered, so only the document's own shape is tested.
        let cited: Vec<String> = parse_rows(&text).iter().flat_map(|r| cited_commands(&r.evidence)).collect();
        let problems = check(&root(), &text, &cited.into_iter().collect());
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(parse_rows(&text).len() > 100);
    }

    /// The real document: two neighbouring rows, each edited by one side, merge with both kept.
    #[test]
    fn neighbouring_rows_of_the_real_parity_doc_merge() {
        let text = with_summary(&std::fs::read_to_string(root().join("docs/parity.md")).unwrap().replace("\r\n", "\n")).unwrap();
        let rows = parse_rows(&text);
        let (first, second) =
            rows.windows(2).find_map(|w| (w[1].line == w[0].line + 1 && w[0].tier.is_some()).then(|| (w[0].clone(), w[1].clone()))).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let edit = |doc: &str, row: &Row, note: &str| {
            let line = lines[row.line - 1];
            let edited = format!("{} {note} |", line.trim_end().trim_end_matches('|').trim_end());
            doc.replacen(&format!("{line}\n"), &format!("{edited}\n"), 1)
        };
        let ours = edit(&text, &first, "(ours)");
        let theirs = edit(&text, &second, "(theirs)");
        assert_ne!(ours, text);
        assert_ne!(theirs, text);
        assert_eq!(merge_rows(&text, &ours, &theirs).unwrap(), edit(&ours, &second, "(theirs)"));
    }
}
