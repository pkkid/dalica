//! `menus-and-shortcuts.md` §1 read straight from the document (not from a transcription), and
//! compared with the menu bar a running app reports through `ui.menu.tree`.
//!
//! What the document gives, per menu: the rows of its table (§1.1–§1.7) or, for Help (§1.8), its
//! prose list; a separator is `—`; a row whose label ends in `▸` is a submenu whose rows are listed
//! inline (after the `▸`, or in the command-id or notes column: `A · B — C`). Each row's command id
//! is the backticked id of its row (alternatives `a` / `b` for one row; a label `X / Y` with two ids
//! is two rows). A label with an italic part (`Undo *<last action>*`) is matched as a prefix.

use serde_json::Value;

/// One row of a menu as the document describes it.
#[derive(Debug, Clone)]
pub enum Node {
    Sep,
    /// `ids` are the acceptable command ids (empty: the document names none for this row).
    Item {
        label: String,
        prefix: bool,
        ids: Vec<String>,
    },
    /// `children` is `None` where the document does not list the submenu's rows inline.
    Sub {
        label: String,
        children: Option<Vec<Node>>,
    },
}

/// A menu of the bar: its title and rows, top to bottom.
pub struct Menu {
    pub title: String,
    pub rows: Vec<Node>,
}

/// The backticked words of `s`, in order.
fn ticks(s: &str) -> Vec<String> {
    s.split('`').skip(1).step_by(2).map(str::to_string).collect()
}

/// `Label (notes, `id`)` → the item; `Sub ▸ A, B, C (`id`)` → a submenu of A, B, C.
fn inline_item(s: &str) -> Node {
    let s = s.trim().trim_end_matches('.');
    let (label, rest) = s.find(" (").map_or((s, ""), |i| (&s[..i], &s[i..]));
    let ids = ticks(rest);
    if let Some((sub, kids)) = label.split_once(" ▸ ") {
        let children = kids.split(", ").map(|k| Node::Item { label: k.trim().to_string(), prefix: false, ids: ids.clone() }).collect();
        return Node::Sub { label: sub.trim().to_string(), children: Some(children) };
    }
    Node::Item { label: label.trim().to_string(), prefix: false, ids }
}

/// `A · B (x) — C` → A, B, separator, C.
fn inline_list(s: &str) -> Vec<Node> {
    let mut out = Vec::new();
    for (i, group) in s.split(" — ").enumerate() {
        if i > 0 {
            out.push(Node::Sep);
        }
        out.extend(group.split(" · ").map(inline_item));
    }
    out
}

/// The cells of a table line `| a | b |`.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// The rows of the first table in `section`.
fn table_rows(section: &str) -> Vec<Node> {
    let mut lines = section.lines().skip_while(|l| !l.starts_with('|'));
    let header = cells(lines.next().unwrap_or_default());
    let id_col = header.iter().position(|c| c == "Command id").expect("a Command id column");
    let mut rows = Vec::new();
    for line in lines.take_while(|l| l.starts_with('|')).filter(|l| !l.starts_with("|---")) {
        let c = cells(line);
        let label = c[0].as_str();
        let id_cell = c.get(id_col).map_or("", String::as_str);
        let notes = c.get(id_col + 1).map_or("", String::as_str);
        if label == "—" {
            rows.push(Node::Sep);
        } else if let Some((sub, after)) = label.split_once('▸') {
            let after = after.trim();
            let children = if !after.is_empty() {
                // `Geotag ▸ Clear Location… · Copy Location · Paste Location`, ids `a` · `b` · `c`.
                let mut kids = inline_list(after);
                let ids: Vec<&str> = id_cell.split(" · ").collect();
                if ids.len() == kids.len() {
                    for (k, id) in kids.iter_mut().zip(ids) {
                        if let Node::Item { ids, .. } = k {
                            *ids = ticks(id);
                        }
                    }
                }
                Some(kids)
            } else if id_cell.contains(" · ") {
                Some(inline_list(id_cell))
            } else if notes.contains(" · ") {
                Some(inline_list(notes))
            } else {
                None
            };
            rows.push(Node::Sub { label: sub.trim().to_string(), children });
        } else {
            let (label, prefix) = label.split_once(" *").map_or((label, false), |(l, _)| (l, true));
            let ids = ticks(id_cell);
            let parts: Vec<&str> = label.split(" / ").collect();
            if parts.len() > 1 && parts.len() == ids.len() {
                // `Rotate Clockwise / Counterclockwise`: two rows, the second sharing the first word.
                let first_word = parts[0].split(' ').next().unwrap_or_default();
                for (i, (part, id)) in parts.iter().zip(&ids).enumerate() {
                    let label = if i == 0 { part.to_string() } else { format!("{first_word} {part}") };
                    rows.push(Node::Item { label, prefix, ids: vec![id.clone()] });
                }
            } else {
                rows.push(Node::Item { label: label.to_string(), prefix, ids });
            }
        }
    }
    rows
}

/// Every menu of §1, in the order of its first line, as the document gives it for Windows and
/// Linux.
pub fn from_doc(doc: &str) -> Vec<Menu> {
    let start = doc.find("## 1. Menu bar").expect("§1");
    let end = doc[start..].find("**Rows that share a command.**").map_or(doc.len(), |e| start + e);
    let s1 = &doc[start..end];
    let bar: Vec<String> = ticks(s1).first().expect("the menu bar line").split_whitespace().map(str::to_string).collect();
    let mut menus = Vec::new();
    for (n, section) in s1.split("\n### 1.").skip(1).enumerate() {
        let heading = section.lines().next().unwrap_or_default();
        let title = heading.split_once(' ').map_or("", |(_, t)| t).split(" / ").next().unwrap_or_default().to_string();
        assert_eq!(heading.split(' ').next(), Some(format!("{}", n + 1).as_str()), "§1.{} heading: {heading}", n + 1);
        let rows = if title == "Help" {
            let prose = section.lines().skip(1).collect::<Vec<_>>().join(" ");
            let prose = prose.split("The macOS application menu").next().unwrap_or_default();
            inline_list(prose.trim())
        } else {
            table_rows(section)
        };
        menus.push(Menu { title, rows });
    }
    let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, bar, "the §1.N headings follow the menu bar line");
    menus
}

/// The same for this platform's `ui.menu.tree`: on macOS Exit, Options… and About Dalica (each with
/// the separator before it) live in the application menu, which comes first (`design-system.md`
/// §11.1); that menu is checked by label in [`compare_bar`].
pub fn for_this_platform(mut menus: Vec<Menu>) -> Vec<Menu> {
    if cfg!(target_os = "macos") {
        for m in &mut menus {
            if let Some(Node::Item { label, .. }) = m.rows.last()
                && ["Exit", "Options…", "About Dalica"].contains(&label.as_str())
            {
                m.rows.pop();
                m.rows.pop();
            }
        }
    }
    menus
}

/// What was compared: menus, item rows (deep items included), submenus.
#[derive(Debug, Default)]
pub struct Counts {
    pub menus: usize,
    pub items: usize,
    pub submenus: usize,
}

/// Compares `want` with the rows of a live menu; pushes every difference onto `errors`.
fn compare(path: &str, want: &[Node], got: &Value, counts: &mut Counts, errors: &mut Vec<String>) {
    let Some(got) = got.as_array() else {
        errors.push(format!("{path}: no rows in {got}"));
        return;
    };
    let shown = |v: &Value| {
        if v["separator"] == true {
            "—".to_string()
        } else if v.get("children").is_some() {
            format!("{} ▸", v["label"].as_str().unwrap_or("?"))
        } else {
            format!("{} ({})", v["label"].as_str().unwrap_or("?"), v["id"].as_str().unwrap_or("?"))
        }
    };
    if want.len() != got.len() {
        errors.push(format!(
            "{path}: the document lists {} rows, the app shows {}: {:?} vs {:?}",
            want.len(),
            got.len(),
            want,
            got.iter().map(shown).collect::<Vec<_>>()
        ));
        return;
    }
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let at = format!("{path} row {}", i + 1);
        match w {
            Node::Sep => {
                if g["separator"] != true {
                    errors.push(format!("{at}: want a separator, the app shows {}", shown(g)));
                }
            }
            Node::Item { label, prefix, ids } => {
                let got_label = g["label"].as_str().unwrap_or_default();
                let got_id = g["id"].as_str().unwrap_or_default();
                let label_ok = if *prefix { got_label.starts_with(label.as_str()) } else { got_label == label };
                let id_ok = ids.is_empty() || ids.iter().any(|id| id == got_id);
                let widget_ok =
                    g["widget"].as_str().is_some_and(|w| w == format!("menuitem:{got_id}") || w.starts_with(&format!("menuitem:{got_id}/")));
                if !label_ok || !id_ok || !widget_ok || g.get("children").is_some() {
                    errors.push(format!("{at}: want `{label}` ({ids:?}), the app shows {} widget {}", shown(g), g["widget"]));
                }
                counts.items += 1;
            }
            Node::Sub { label, children } => {
                if g["label"] != label.as_str() || g.get("children").is_none() {
                    errors.push(format!("{at}: want the submenu `{label} ▸`, the app shows {}", shown(g)));
                    continue;
                }
                counts.submenus += 1;
                match children {
                    Some(kids) => compare(&format!("{path} ▸ {label}"), kids, &g["children"], counts, errors),
                    // Not listed inline (Batch Edit ▸): its rows are still items with ids.
                    None => {
                        let n = g["children"].as_array().map_or(0, |a| a.iter().filter(|r| r.get("id").is_some()).count());
                        if n == 0 {
                            errors.push(format!("{at}: `{label} ▸` is empty"));
                        }
                        counts.items += n;
                    }
                }
            }
        }
    }
}

/// Compares the document's menus with `ui.menu.tree`'s reply; returns what was compared or every
/// difference.
pub fn compare_bar(want: &[Menu], tree: &Value) -> Result<Counts, Vec<String>> {
    let mut errors = Vec::new();
    let mut counts = Counts::default();
    let mut got: Vec<Value> = tree.as_array().cloned().unwrap_or_default();
    if cfg!(target_os = "macos") {
        let app = if got.first().is_some_and(|m| m["label"] == "Dalica") { Some(got.remove(0)) } else { None };
        let labels: Vec<String> = app
            .iter()
            .flat_map(|m| m["children"].as_array().cloned().unwrap_or_default())
            .filter_map(|r| r["label"].as_str().map(str::to_string))
            .collect();
        for l in ["About Dalica", "Preferences…", "Hide Dalica", "Quit Dalica"] {
            if !labels.iter().any(|x| x == l) {
                errors.push(format!("macOS: the application menu has no `{l}`: {labels:?}"));
            }
        }
    }
    let titles: Vec<&str> = got.iter().map(|m| m["label"].as_str().unwrap_or("?")).collect();
    let want_titles: Vec<&str> = want.iter().map(|m| m.title.as_str()).collect();
    if titles != want_titles {
        errors.push(format!("the menu bar is {titles:?}, the document says {want_titles:?}"));
        return Err(errors);
    }
    for (m, g) in want.iter().zip(&got) {
        if g["widget"] != format!("menu:{}", m.title) {
            errors.push(format!("{}: widget {}", m.title, g["widget"]));
        }
        compare(&m.title, &m.rows, &g["children"], &mut counts, &mut errors);
        counts.menus += 1;
    }
    if errors.is_empty() { Ok(counts) } else { Err(errors) }
}
