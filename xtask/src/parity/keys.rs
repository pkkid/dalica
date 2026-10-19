//! Section I of `docs/parity.md`, generated (`cargo xtask parity --write`): one `KEY-<context>-<key>`
//! row per key documented in `menus-and-shortcuts.md` §2.1 (Global), §2.2 (Library) and `editing.md`
//! §12 (Edit view), from the bindings `dalica-cli commands` lists (the registry and `UI_COMMANDS`),
//! written between the `parity:keys` markers.
//!
//! - **Status:** ✅ when the keymap binds the key in its documented context (a view context's key
//!   may be bound in Global instead, which is active underneath it; a Global key may be bound in
//!   both the Library and the Edit view instead); for a Global key, 🟡 when it is bound in some view
//!   contexts only; ⬜ otherwise (the same key in another view context is another binding: Ctrl+1).
//! - **Command:** what the key is bound to; for an unbound key, what the other keys of its group are
//!   bound to (`Ctrl+8, Space`), else the command `menus-and-shortcuts.md` §1 gives the key.
//! - **Tier:** P0 when a P0 row outside this section cites one of its commands (`cmd:<id>` or the
//!   backticked id), P1 otherwise.
//! - A ⬜ key whose command is registered is a problem: the check fails. An unregistered command's
//!   row names the first unfinished task block citing it.
//! - Hold gestures, the wheel and clicks are not keymap bindings and get no row.

use std::collections::BTreeSet;

use crate::tasks::roadmap::Roadmap;

pub const START: &str = "<!-- parity:keys -->";
pub const END: &str = "<!-- /parity:keys -->";

/// The documented contexts: the keymap's name (as `dalica-cli commands` prints it), the row id's
/// part and the label.
pub const CONTEXTS: [(&str, &str, &str); 3] = [("global", "GLOBAL", "Global"), ("library", "LIBRARY", "Library"), ("editView", "EDIT", "Edit view")];

/// Where the documented keys are: the file, the heading that starts the table, the context, and
/// how a row names the place.
pub const SOURCES: [(&str, &str, &str, &str); 3] = [
    ("docs/menus-and-shortcuts.md", "### 2.1 ", "global", "menus-and-shortcuts.md §2.1"),
    ("docs/menus-and-shortcuts.md", "### 2.2 ", "library", "menus-and-shortcuts.md §2.2"),
    ("docs/editing.md", "## 12. ", "editView", "editing.md §12"),
];

const NAMED: [&str; 16] =
    ["Enter", "Esc", "Tab", "Space", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right", "Plus"];
const PUNCTUATION: [(char, &str); 11] = [
    (',', "COMMA"),
    ('.', "PERIOD"),
    ('/', "SLASH"),
    ('\\', "BACKSLASH"),
    (';', "SEMICOLON"),
    ('\'', "QUOTE"),
    ('[', "LBRACKET"),
    (']', "RBRACKET"),
    ('-', "MINUS"),
    ('=', "EQUALS"),
    ('`', "BACKTICK"),
];

/// A key as the keymap writes it (`Ctrl+Alt+Shift+<code>`, `Ctrl` for Cmd too), from the way the
/// docs or the registry write it; `None` for what is not a key (`Wheel`, `Ctrl+Alt` alone).
pub fn normalise(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    if text == "+" {
        return Some("Plus".into());
    }
    let mut parts: Vec<&str> = text.split('+').collect();
    if text.ends_with("++") {
        parts.truncate(parts.len().saturating_sub(2));
        parts.push("Plus");
    }
    let (code, mods) = parts.split_last()?;
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    for m in mods {
        let flag = match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "control" | "command" => &mut ctrl,
            "alt" | "option" | "opt" => &mut alt,
            "shift" => &mut shift,
            _ => return None,
        };
        if *flag {
            return None;
        }
        *flag = true;
    }
    let code = code_of(code.trim())?;
    let mut out = String::new();
    for (on, name) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(&code);
    Some(out)
}

fn code_of(code: &str) -> Option<String> {
    let mut chars = code.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let named = match c {
            '←' => Some("Left"),
            '→' => Some("Right"),
            '↑' => Some("Up"),
            '↓' => Some("Down"),
            '−' => Some("-"),
            _ => None,
        };
        if let Some(n) = named {
            return Some(n.into());
        }
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        if PUNCTUATION.iter().any(|(p, _)| *p == c) {
            return Some(c.to_string());
        }
        return None;
    }
    let lower = code.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=24).contains(n)) {
        return Some(format!("F{n}"));
    }
    let alias = match lower.as_str() {
        "escape" => "esc",
        "return" => "enter",
        "del" => "delete",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        other => other,
    };
    NAMED.iter().find(|n| n.eq_ignore_ascii_case(alias)).map(|n| (*n).to_string())
}

/// The row id's part for a normalised key: `Ctrl+Shift+S` → `CTRL-SHIFT-S`, `Ctrl+=` → `CTRL-EQUALS`.
pub fn key_id(key: &str) -> String {
    key.split('+')
        .map(|part| {
            let mut chars = part.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => PUNCTUATION.iter().find(|(p, _)| *p == c).map_or_else(|| part.to_ascii_uppercase(), |(_, n)| (*n).to_string()),
                _ => part.to_ascii_uppercase(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// One binding of the keymap: (command, context, key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub command: String,
    pub context: String,
    pub key: String,
}

/// The bindings `dalica-cli commands` lists: each entry's `keys` (`[{context, key}]`).
pub fn bindings(list: &[serde_json::Value]) -> Vec<Binding> {
    let mut out = Vec::new();
    for c in list {
        let Some(command) = c.get("id").and_then(|i| i.as_str()) else { continue };
        for k in c.get("keys").and_then(|k| k.as_array()).into_iter().flatten() {
            let context = k.get("context").and_then(|v| v.as_str());
            let key = k.get("key").and_then(|v| v.as_str()).and_then(normalise);
            if let (Some(context), Some(key)) = (context, key) {
                out.push(Binding { command: command.to_string(), context: context.to_string(), key });
            }
        }
    }
    out
}

/// A key the docs list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocKey {
    /// The keymap's name of its context (`global`, `library`, `editView`).
    pub context: &'static str,
    /// Normalised.
    pub key: String,
    /// As the docs write it.
    pub written: String,
    pub action: String,
    /// The keys of its group, itself included: keys written `Ctrl+8, Space` run one command.
    pub group: Vec<String>,
    /// Where it is documented (`menus-and-shortcuts.md §2.1`).
    pub source: &'static str,
}

/// The rows of the first table after the heading line that starts with `heading`, up to the next
/// heading: cells trimmed, the header and the separator left out.
fn table(text: &str, heading: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut inside = false;
    let mut header_seen = false;
    for line in text.lines() {
        if line.starts_with('#') {
            if inside {
                break;
            }
            inside = line.starts_with(heading);
            continue;
        }
        if !inside || !line.trim_start().starts_with('|') {
            continue;
        }
        let cells: Vec<String> = line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
        if !header_seen {
            header_seen = true;
            continue;
        }
        if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// `text` without its parenthetical parts.
fn strip_parens(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// The keys a range writes: `Ctrl+Alt+1 … Ctrl+Alt+0` → `Ctrl+Alt+1`, …, `Ctrl+Alt+9`, `Ctrl+Alt+0`.
fn range(part: &str) -> Vec<String> {
    const DIGITS: &str = "1234567890";
    let Some((a, b)) = part.split_once('…') else { return Vec::new() };
    let (a, b) = (a.trim(), b.trim());
    let (Some(first), Some(last)) = (a.chars().last(), b.chars().last()) else { return Vec::new() };
    let prefix = a.get(..a.len().saturating_sub(first.len_utf8())).unwrap_or("");
    if b.get(..b.len().saturating_sub(last.len_utf8())) != Some(prefix) {
        return Vec::new();
    }
    let (Some(from), Some(to)) = (DIGITS.find(first), DIGITS.find(last)) else { return Vec::new() };
    DIGITS.get(from..=to).unwrap_or("").chars().map(|d| format!("{prefix}{d}")).collect()
}

/// Keys that run one command, each `(normalised, as written)`.
type Group = Vec<(String, String)>;

/// The groups of keys one key cell writes, as `(normalised, as written)`, each with the index of its
/// ` / `-separated part and the number of parts: `A / B` are two groups, `A, B` one group of two
/// keys, `← → ↑ ↓` four groups, `X1 … X0` a range; `(also C/D)` adds C to the first group and D to
/// the second. Hold gestures and what is not a key are left out.
fn groups(cell: &str) -> (usize, Vec<(usize, Group)>) {
    let (cell, also) = match (cell.find("(also "), cell.rfind(')')) {
        (Some(at), Some(end)) if end > at => {
            let inner = cell.get(at + "(also ".len()..end).unwrap_or("");
            let rest = format!("{}{}", cell.get(..at).unwrap_or(""), cell.get(end + 1..).unwrap_or(""));
            (rest, inner.split('/').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        }
        _ => (cell.to_string(), Vec::new()),
    };
    let parts: Vec<&str> = cell.split(" / ").collect();
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if part.contains("(hold") {
            continue;
        }
        let part = strip_parens(part);
        let mut written: Vec<Vec<String>> = if part.contains('…') {
            range(&part).into_iter().map(|k| vec![k]).collect()
        } else if part.contains(", ") {
            vec![part.split(", ").map(|s| s.trim().to_string()).collect()]
        } else {
            part.split_whitespace().map(|s| vec![s.to_string()]).collect()
        };
        if also.len() == parts.len()
            && let ([group], Some(extra)) = (written.as_mut_slice(), also.get(i))
        {
            group.push(extra.clone());
        }
        for group in written {
            let keys: Vec<(String, String)> = group.into_iter().filter_map(|w| normalise(&w).map(|k| (k, w))).collect();
            if !keys.is_empty() {
                out.push((i, keys));
            }
        }
    }
    (parts.len(), out)
}

/// The action of each of a key cell's `parts` parts: `A / B` with `small / normal thumbnails` gives
/// `small thumbnails` and `normal thumbnails` (a one-word part takes the last part's other words);
/// the whole action when the counts differ.
fn split_action(action: &str, parts: usize) -> Vec<String> {
    let actions: Vec<&str> = action.split(" / ").collect();
    if actions.len() != parts || parts < 2 {
        return vec![action.to_string(); parts];
    }
    let tail: Vec<&str> = actions.last().map(|l| l.split_whitespace().skip(1).collect()).unwrap_or_default();
    let last = actions.len().saturating_sub(1);
    actions
        .iter()
        .enumerate()
        .map(
            |(i, a)| {
                if i < last && a.split_whitespace().count() == 1 && !tail.is_empty() { format!("{a} {}", tail.join(" ")) } else { (*a).to_string() }
            },
        )
        .collect()
}

/// Every key `menus-and-shortcuts.md` §2.1 and §2.2 and `editing.md` §12 document, in their order;
/// a key documented twice in one context counts once.
pub fn documented(menus: &str, editing: &str) -> Vec<DocKey> {
    let mut out: Vec<DocKey> = Vec::new();
    for (file, heading, context, source) in SOURCES {
        let text = if file.ends_with("editing.md") { editing } else { menus };
        for row in table(text, heading) {
            let (Some(cell), Some(action)) = (row.first(), row.get(1)) else { continue };
            let (parts, found) = groups(cell);
            let actions = split_action(action, parts);
            for (i, group) in found {
                let action = actions.get(i).map_or(action.as_str(), String::as_str);
                let keys: Vec<String> = group.iter().map(|(k, _)| k.clone()).collect();
                for (key, written) in &group {
                    if out.iter().any(|d| d.context == context && &d.key == key) {
                        continue;
                    }
                    out.push(DocKey { context, key: key.clone(), written: written.clone(), action: action.to_string(), group: keys.clone(), source });
                }
            }
        }
    }
    out
}

/// The default keys `menus-and-shortcuts.md` §1 gives commands, `(key, command)` in its order: the
/// Shortcut and Command id columns of its tables (`A / B` pairs with `` `a` / `b` ``; a parenthetical
/// or italic note after the key is left out) and `(F1, `help.contents`)` anywhere in a line.
pub fn menu_keys(menus: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut columns: Option<(usize, usize)> = None;
    let mut in_table = false;
    for line in menus.lines() {
        if line.starts_with("## ") {
            inside = line.starts_with("## 1.");
            continue;
        }
        if !inside {
            continue;
        }
        prose_keys(line, &mut out);
        if !line.trim_start().starts_with('|') {
            in_table = false;
            columns = None;
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if !in_table {
            in_table = true;
            let at = |name: &str| cells.iter().position(|c| *c == name);
            columns = at("Shortcut").zip(at("Command id"));
            continue;
        }
        let Some((k, c)) = columns else { continue };
        let (Some(keys), Some(ids)) = (cells.get(k), cells.get(c)) else { continue };
        let keys = keys.split(" (").next().unwrap_or("").split(" *(").next().unwrap_or("");
        let keys: Vec<String> = keys.split(" / ").filter_map(normalise).collect();
        let ids: Vec<&str> = ids.split('`').skip(1).step_by(2).collect();
        for (i, key) in keys.iter().enumerate() {
            let id = if ids.len() == keys.len() { ids.get(i) } else { ids.first() };
            if let Some(id) = id {
                out.push((key.clone(), (*id).to_string()));
            }
        }
    }
    out
}

/// `(KEY, `id`` in prose.
fn prose_keys(line: &str, out: &mut Vec<(String, String)>) {
    let mut rest = line;
    while let Some(at) = rest.find('(') {
        let after = rest.get(at + 1..).unwrap_or("");
        rest = after;
        let Some((key, tail)) = after.split_once(", `") else { continue };
        let (Some(key), Some(id)) = (normalise(key), tail.split('`').next()) else { continue };
        if !id.is_empty() && !id.contains(' ') {
            out.push((key, id.to_string()));
        }
    }
}

/// Commands that P0 rows outside section I cite: `cmd:<id>` and every backticked word.
pub fn p0_cited(rows: &[super::Row]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for r in rows.iter().filter(|r| r.tier.as_deref() == Some("P0") && !r.id.starts_with("KEY-")) {
        out.extend(super::cited_commands(&r.evidence));
        out.extend(r.evidence.split('`').skip(1).step_by(2).map(|w| w.strip_prefix("cmd:").unwrap_or(w).to_string()));
    }
    out
}

/// Whether `text` cites the command `id`: `` `id` `` alone, or with its params, `` `id {…}` ``.
fn cites(text: &str, id: &str) -> bool {
    text.contains(&format!("`{id}`")) || text.contains(&format!("`{id} {{"))
}

/// The first unfinished task block citing `id` (see [`cites`]), as `M11.13`.
fn arrives_with(id: &str, roadmap: Option<&Roadmap>) -> Option<String> {
    let mut tasks: Vec<_> =
        roadmap?.tasks.iter().filter(|t| !t.done && (cites(&t.title, id) || cites(&t.scope, id) || cites(&t.done_when, id))).collect();
    tasks.sort_by_key(|t| (t.milestone, t.line));
    tasks.first().map(|t| t.id.clone())
}

/// One generated row, and the problem it shows, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    pub id: String,
    pub feature: String,
    pub tier: &'static str,
    pub status: &'static str,
    pub evidence: String,
    pub problem: Option<String>,
}

/// What the rows are made from.
pub struct Inputs<'a> {
    pub documented: &'a [DocKey],
    pub bindings: &'a [Binding],
    pub registered: &'a BTreeSet<String>,
    pub menu_keys: &'a [(String, String)],
    pub p0_cited: &'a BTreeSet<String>,
    pub roadmap: Option<&'a Roadmap>,
}

fn label(context: &str) -> &str {
    CONTEXTS.iter().find(|(c, _, _)| *c == context).map_or(context, |(_, _, l)| *l)
}

/// The rows for the documented keys, in their order.
pub fn rows(inputs: &Inputs) -> Vec<KeyRow> {
    let bound = |key: &str, context: &str| -> BTreeSet<String> {
        inputs.bindings.iter().filter(|b| b.key == key && b.context == context).map(|b| b.command.clone()).collect()
    };
    // Every context of the keymap, the documented ones first.
    let mut contexts: Vec<String> = CONTEXTS.iter().map(|(c, _, _)| (*c).to_string()).collect();
    let mut others: Vec<String> = inputs.bindings.iter().map(|b| b.context.clone()).filter(|c| !contexts.contains(c)).collect();
    others.sort();
    others.dedup();
    contexts.extend(others);
    let cmds = |set: &BTreeSet<String>| set.iter().map(|c| format!("`cmd:{c}`")).collect::<Vec<_>>().join(", ");
    let at =
        |found: &[(String, BTreeSet<String>)]| found.iter().map(|(c, set)| format!("{} ({})", cmds(set), label(c))).collect::<Vec<_>>().join("; ");

    let mut out = Vec::new();
    for d in inputs.documented {
        let ctx_id = CONTEXTS.iter().find(|(c, _, _)| *c == d.context).map_or("OTHER", |(_, id, _)| *id);
        let id = format!("KEY-{ctx_id}-{}", key_id(&d.key));
        let feature = format!("{}: {} — {}", label(d.context), d.written, d.action);
        let here = bound(&d.key, d.context);
        let global = if d.context == "global" { BTreeSet::new() } else { bound(&d.key, "global") };
        let (library, edit) = (bound(&d.key, "library"), bound(&d.key, "editView"));
        let (status, found): (&'static str, Vec<(String, BTreeSet<String>)>) = if !here.is_empty() {
            ("✅", vec![(d.context.to_string(), here)])
        } else if !global.is_empty() {
            ("✅", vec![("global".to_string(), global)])
        } else if d.context == "global" && !library.is_empty() && !edit.is_empty() {
            ("✅", vec![("library".to_string(), library), ("editView".to_string(), edit)])
        } else if d.context == "global" {
            // A Global key bound in some view contexts only works there.
            let views: Vec<(String, BTreeSet<String>)> =
                contexts.iter().filter(|c| c.as_str() != "global").map(|c| (c.clone(), bound(&d.key, c))).filter(|(_, s)| !s.is_empty()).collect();
            (if views.is_empty() { "⬜" } else { "🟡" }, views)
        } else {
            ("⬜", Vec::new())
        };
        let mut commands: BTreeSet<String> = found.iter().flat_map(|(_, s)| s.iter().cloned()).collect();
        let (evidence, problem) = match status {
            "✅" => (at(&found), None),
            "🟡" => (format!("{}; not bound in {}", at(&found), label(d.context)), None),
            _ => {
                // The command the key is documented for: its group's, else §1's.
                let sibling = d.group.iter().filter(|k| **k != d.key).find_map(|k| {
                    let set: BTreeSet<String> = [d.context, "global"].iter().flat_map(|c| bound(k, c)).collect();
                    set.into_iter().next()
                });
                let command = sibling.or_else(|| inputs.menu_keys.iter().find(|(k, _)| *k == d.key).map(|(_, c)| c.clone()));
                match command {
                    Some(c) if inputs.registered.contains(&c) => {
                        commands.insert(c.clone());
                        let problem = format!(
                            "{id}: {} is documented in {} for `{c}`, which is registered, but the keymap does not bind it in {}",
                            d.written,
                            d.source,
                            label(d.context)
                        );
                        (format!("`cmd:{c}` is registered but the key is not bound in {}", label(d.context)), Some(problem))
                    }
                    Some(c) => {
                        commands.insert(c.clone());
                        let when = arrives_with(&c, inputs.roadmap).map_or_else(|| "no task yet".to_string(), |t| format!("arrives with {t}"));
                        (format!("`{c}` is not registered yet ({when})"), None)
                    }
                    None => ("no command yet".to_string(), None),
                }
            }
        };
        let tier = if commands.iter().any(|c| inputs.p0_cited.contains(c)) { "P0" } else { "P1" };
        out.push(KeyRow { id, feature, tier, status, evidence, problem });
    }
    out
}

/// The table between the markers.
pub fn block(rows: &[KeyRow]) -> String {
    let mut out = String::from("| Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n");
    for r in rows {
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", r.id, r.feature.replace('|', "\\|"), r.tier, r.status, r.evidence));
    }
    out
}

/// `text` with the generated table between the `parity:keys` markers; `None` if they are missing.
pub fn with_block(text: &str, block: &str) -> Option<String> {
    let start = text.find(START)?;
    let end = text.find(END)?;
    if end < start {
        return None;
    }
    Some(format!("{}{START}\n{block}{}", text.get(..start)?, text.get(end..)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_cited_bare_or_with_its_params() {
        assert!(cites("through `photo.locate` and more", "photo.locate"));
        assert!(cites("`photo.locate {id}` and `folder.locate {id}`", "photo.locate"));
        assert!(!cites("`photo.locateAll {id}`", "photo.locate"));
        assert!(!cites("photo.locate {id}", "photo.locate"));
    }

    const MENUS: &str = "# M\n\n## 1. Menu bar\n\n### 1.1 File\n\n| Item | Shortcut | Command id | Notes |\n|---|---|---|---|\n\
| Open File… | Ctrl+O | `library.openFile` | x |\n| Rename… | F2 | `photo.rename` | x |\n\
| Rotate | Ctrl+R / Ctrl+Shift+R | `photo.rotate.cw` / `photo.rotate.ccw` | x |\n\nHelp Contents (F1, `help.contents`) · more.\n\n\
## 2. Keyboard shortcuts by context\n\n### 2.1 Global\n\n| Key | Action |\n|---|---|\n| Ctrl+O / F2 | open file / rename |\n\
| Ctrl+S | save |\n| F1 | help |\n| F5 | refresh |\n\n### 2.2 Library\n\n| Key | Action |\n|---|---|\n| ← → | move |\n\
| Ctrl+8 | star |\n| Ctrl+Alt (hold, hover) | preview |\n| Ctrl+Alt+1 … Ctrl+Alt+3 | quick tags |\n| Ctrl+wheel | size |\n\n### 2.3 Edit view\n";
    const EDITING: &str = "# E\n\n## 12. Keyboard summary (edit view)\n\n| Key | Action |\n|---|---|\n| ← / → (also PgUp/PgDn) | previous / next |\n\
| Ctrl+8, Space | toggle star |\n| Wheel / Ctrl+= / Ctrl+− | zoom / zoom in / zoom out |\n| \\ (hold) | original |\n| Ctrl+Alt+1 | another meaning |\n";

    fn registry() -> Vec<serde_json::Value> {
        let k = |c: &str, key: &str| serde_json::json!({"context": c, "key": key});
        vec![
            serde_json::json!({"id": "photo.star.toggle", "keys": [k("library", "Ctrl+8"), k("editView", "Ctrl+8")]}),
            serde_json::json!({"id": "photo.save", "keys": [k("library", "Ctrl+S"), k("editView", "Ctrl+S")]}),
            serde_json::json!({"id": "photo.rename", "keys": [k("library", "F2")]}),
            serde_json::json!({"id": "select.left", "keys": [k("library", "Left")]}),
            serde_json::json!({"id": "select.right", "keys": [k("library", "Right")]}),
            serde_json::json!({"id": "edit.previous", "keys": [k("editView", "Left"), k("editView", "PageUp")]}),
            serde_json::json!({"id": "edit.next", "keys": [k("editView", "Right"), k("editView", "PageDown")]}),
            serde_json::json!({"id": "edit.zoom.in", "keys": [k("editView", "Ctrl+="), k("editView", "Ctrl+Plus")]}),
            serde_json::json!({"id": "edit.zoom.out", "keys": [k("editView", "Ctrl+-")]}),
            serde_json::json!({"id": "tags.quick.1", "keys": [k("library", "Ctrl+Alt+1")]}),
            serde_json::json!({"id": "tags.quick.2", "keys": [k("library", "Ctrl+Alt+2")]}),
            serde_json::json!({"id": "tags.quick.3", "keys": [k("library", "Ctrl+Alt+3")]}),
        ]
    }

    fn generate(list: &[serde_json::Value]) -> Vec<KeyRow> {
        let documented = documented(MENUS, EDITING);
        let bindings = bindings(list);
        let registered: BTreeSet<String> = list.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str()).map(str::to_string)).collect();
        let menu_keys = menu_keys(MENUS);
        let p0: BTreeSet<String> = ["photo.star.toggle".to_string()].into();
        rows(&Inputs { documented: &documented, bindings: &bindings, registered: &registered, menu_keys: &menu_keys, p0_cited: &p0, roadmap: None })
    }

    fn row<'a>(rows: &'a [KeyRow], id: &str) -> &'a KeyRow {
        rows.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no row {id} in {:#?}", rows.iter().map(|r| &r.id).collect::<Vec<_>>()))
    }

    #[test]
    fn keys_normalise_as_the_keymap_writes_them() {
        assert_eq!(normalise("Cmd+shift+s").as_deref(), Some("Ctrl+Shift+S"));
        assert_eq!(normalise("Shift+Alt+Ctrl+f4").as_deref(), Some("Ctrl+Alt+Shift+F4"));
        assert_eq!(normalise("PgDn").as_deref(), Some("PageDown"));
        assert_eq!(normalise("←").as_deref(), Some("Left"));
        assert_eq!(normalise("Ctrl+−").as_deref(), Some("Ctrl+-"));
        assert_eq!(normalise("+").as_deref(), Some("Plus"));
        assert_eq!(normalise("Ctrl++").as_deref(), Some("Ctrl+Plus"));
        assert_eq!(normalise("Ctrl+,").as_deref(), Some("Ctrl+,"));
        for bad in ["Wheel", "Ctrl+wheel", "Ctrl+Alt", "Ctrl+Ctrl+A", "", "Click-and-hold"] {
            assert_eq!(normalise(bad), None, "{bad}");
        }
        assert_eq!(key_id("Ctrl+Shift+S"), "CTRL-SHIFT-S");
        assert_eq!(key_id("Ctrl+="), "CTRL-EQUALS");
        assert_eq!(key_id("Ctrl+-"), "CTRL-MINUS");
        assert_eq!(key_id("\\"), "BACKSLASH");
    }

    #[test]
    fn the_documented_keys_are_read_from_the_tables() {
        let d = documented(MENUS, EDITING);
        let listed: Vec<String> = d.iter().map(|k| format!("{}:{}", k.context, k.key)).collect();
        assert_eq!(
            listed,
            [
                "global:Ctrl+O",
                "global:F2",
                "global:Ctrl+S",
                "global:F1",
                "global:F5",
                "library:Left",
                "library:Right",
                "library:Ctrl+8",
                "library:Ctrl+Alt+1",
                "library:Ctrl+Alt+2",
                "library:Ctrl+Alt+3",
                "editView:Left",
                "editView:PageUp",
                "editView:Right",
                "editView:PageDown",
                "editView:Ctrl+8",
                "editView:Space",
                "editView:Ctrl+=",
                "editView:Ctrl+-",
                "editView:Ctrl+Alt+1",
            ]
        );
        let space = d.iter().find(|k| k.key == "Space").unwrap();
        assert_eq!(space.group, ["Ctrl+8", "Space"]);
        assert_eq!(d.iter().find(|k| k.key == "PageUp").unwrap().group, ["Left", "PageUp"]);
        assert_eq!(
            menu_keys(MENUS),
            [
                ("Ctrl+O".to_string(), "library.openFile".to_string()),
                ("F2".to_string(), "photo.rename".to_string()),
                ("Ctrl+R".to_string(), "photo.rotate.cw".to_string()),
                ("Ctrl+Shift+R".to_string(), "photo.rotate.ccw".to_string()),
                ("F1".to_string(), "help.contents".to_string()),
            ]
        );
    }

    #[test]
    fn rows_say_how_each_documented_key_is_bound() {
        let mut list = registry();
        // Space is bound too, so the planted keymap has no gap.
        list[0]["keys"].as_array_mut().unwrap().push(serde_json::json!({"context": "editView", "key": "Space"}));
        let rows = generate(&list);
        assert!(rows.iter().all(|r| r.problem.is_none()), "{rows:#?}");
        // Bound in both view contexts: a Global key as documented.
        let save = row(&rows, "KEY-GLOBAL-CTRL-S");
        assert_eq!((save.status, save.tier), ("✅", "P1"));
        assert_eq!(save.evidence, "`cmd:photo.save` (Library); `cmd:photo.save` (Edit view)");
        // Bound in the Library only.
        let rename = row(&rows, "KEY-GLOBAL-F2");
        assert_eq!(rename.status, "🟡");
        assert_eq!(rename.evidence, "`cmd:photo.rename` (Library); not bound in Global");
        // A P0 row cites the star command.
        let star = row(&rows, "KEY-EDIT-SPACE");
        assert_eq!((star.status, star.tier, star.feature.as_str()), ("✅", "P0", "Edit view: Space — toggle star"));
        // Unregistered commands, and a key with no command.
        assert_eq!(row(&rows, "KEY-GLOBAL-CTRL-O").evidence, "`library.openFile` is not registered yet (no task yet)");
        assert_eq!(row(&rows, "KEY-GLOBAL-F1").status, "⬜");
        assert_eq!(row(&rows, "KEY-GLOBAL-F5").evidence, "no command yet");
        let zoom_out = row(&rows, "KEY-EDIT-CTRL-MINUS");
        assert_eq!((zoom_out.status, zoom_out.feature.as_str()), ("✅", "Edit view: Ctrl+− — zoom out"));
        // Bound in another view context only: another binding, not this one.
        let other = row(&rows, "KEY-EDIT-CTRL-ALT-1");
        assert_eq!((other.status, other.evidence.as_str()), ("⬜", "no command yet"));
        assert_eq!(row(&rows, "KEY-GLOBAL-CTRL-O").feature, "Global: Ctrl+O — open file");
        assert_eq!(row(&rows, "KEY-EDIT-CTRL-EQUALS").feature, "Edit view: Ctrl+= — zoom in");
        assert_eq!(split_action("small / normal thumbnails", 2), ["small thumbnails", "normal thumbnails"]);
        assert_eq!(split_action("new album / open file / import", 3), ["new album", "open file", "import"]);
        assert_eq!(split_action("zoom", 2), ["zoom", "zoom"]);
        assert_eq!(row(&rows, "KEY-LIBRARY-CTRL-ALT-3").evidence, "`cmd:tags.quick.3` (Library)");
    }

    #[test]
    fn a_documented_key_removed_from_the_keymap_fails() {
        // The planted keymap binds Ctrl+8 in the edit view but not Space, which the docs list with it.
        let rows = generate(&registry());
        let problems: Vec<&String> = rows.iter().filter_map(|r| r.problem.as_ref()).collect();
        assert_eq!(
            problems,
            [
                "KEY-EDIT-SPACE: Space is documented in editing.md §12 for `photo.star.toggle`, which is registered, but the keymap does not bind it in Edit view"
            ]
        );
        // Removing a key that has its own documented command fails the same way.
        let mut list = registry();
        list[2]["keys"] = serde_json::json!([]);
        let rows = generate(&list);
        let rename = row(&rows, "KEY-GLOBAL-F2");
        assert_eq!(rename.status, "⬜");
        assert!(rename.problem.as_deref().is_some_and(|p| p.contains("`photo.rename`")), "{rename:?}");
    }

    #[test]
    fn the_generated_rows_are_stable_between_two_runs() {
        let doc = format!("# P\n\n## I. Keyboard shortcuts (KEY)\n\nprose\n\n{START}\nold\n{END}\n\nafter\n");
        let once = with_block(&doc, &block(&generate(&registry()))).unwrap();
        let twice = with_block(&once, &block(&generate(&registry()))).unwrap();
        assert_eq!(once, twice);
        assert!(once.ends_with(&format!("{END}\n\nafter\n")) && !once.contains("old"));
        // The rows parse as feature rows of section I, with valid ids.
        let parsed = super::super::parse_rows(&once);
        assert_eq!(parsed.len(), generate(&registry()).len());
        assert!(parsed.iter().all(|r| r.section == "I. Keyboard shortcuts" && super::super::id_ok(&r.id)), "{parsed:#?}");
        assert_eq!(with_block("no markers", "x"), None);
    }
}
