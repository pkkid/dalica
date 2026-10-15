//! The shortcut editor's query (`menus-and-shortcuts.md` §2.0): `keymap.list {context?, search?,
//! withoutKeys?}`, one row per command and context with its keys in effect and its defaults.
//! Rebinding stays `app.setShortcut` / `app.resetShortcuts` in `cmd/app.rs`.
//!
//! - Rows come from [`crate::keymap::Keymap::rows`]: every command the keymap knows (the
//!   registry's and the UI commands the app added) once per context it has keys in, by default or
//!   by the user (also when the user removed them all); with `withoutKeys`, the commands with no
//!   keys anywhere too, once each, with `context: null`.
//! - `context` keeps the rows of that context (and the keyless ones, which belong to none yet).
//! - `search` (ignoring case) matches a row's label, menu path or id as text, and its keys: a
//!   search that reads as a key (`ctrl+1`, `Ctrl 1`, `⌘1`; [`Key::parse_lenient`]) finds the rows
//!   with that key, or, with no modifier typed (`8`, `F4`), with that key under any modifiers;
//!   any other search is matched against the keys as written and as the screen shows them.
//! - UI commands are labelled by their id: the engine knows only their ids and keys.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::keymap::{Key, Platform, Row};
use crate::registry::{CommandSpec, Context, Effect, History, always, params};
use crate::session::Session;

/// The longest search text taken, in bytes.
pub const MAX_SEARCH_BYTES: usize = 256;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "keymap.list",
        label: "List Shortcuts",
        menu: "",
        keys: &[],
        params: r#"{"context"?:"global"|"library"|"editView"|…, "search"?:string (≤ 256 bytes), "withoutKeys"?:bool}: the shortcut editor's rows {rows: [{command, label, menu, context, keys, defaults, modified, editable}], count}: one per command and context it has keys in (context null: a command without keys, listed with withoutKeys); search matches labels, menu paths and keys ignoring case and spelling ("ctrl+1" finds every Ctrl+1 binding)"#,
        effect: Effect::Query,
        history: History::None,
        enabled: always,
        run: list,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct List {
    context: Option<Context>,
    search: Option<String>,
    #[serde(default)]
    without_keys: bool,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let List { context, search, without_keys } = params(p)?;
    let search = search.unwrap_or_default();
    if search.len() > MAX_SEARCH_BYTES {
        return Err(CmdError::Limit(format!("search is longer than {MAX_SEARCH_BYTES} bytes")));
    }
    let query = Query::new(&search);
    let rows: Vec<Value> = s
        .keymap()
        .rows(without_keys)
        .into_iter()
        .filter(|r| context.is_none_or(|c| r.context.is_none_or(|rc| rc == c)))
        .filter_map(|r| {
            let (label, menu) = s.spec(&r.command).map_or((r.command.as_str(), ""), |spec| (spec.label, spec.menu));
            query.matches(&r, label, menu).then(|| row_json(&r, label, menu))
        })
        .collect();
    Ok(json!({ "count": rows.len(), "rows": rows }))
}

fn row_json(r: &Row, label: &str, menu: &str) -> Value {
    let keys = |ks: &[Key]| ks.iter().map(Key::to_string).collect::<Vec<_>>();
    json!({
        "command": r.command,
        "label": label,
        "menu": menu,
        "context": r.context,
        "keys": keys(&r.keys),
        "defaults": keys(&r.defaults),
        "modified": r.modified(),
        "editable": r.editable,
    })
}

/// A search of the editor.
struct Query {
    /// Lowercase, trimmed; empty matches everything.
    text: String,
    /// The search read as a key, if it is one.
    key: Option<Key>,
}

impl Query {
    fn new(search: &str) -> Query {
        let text = search.trim().to_lowercase();
        let key = if text.is_empty() { None } else { Key::parse_lenient(&text).ok() };
        Query { text, key }
    }

    fn matches(&self, r: &Row, label: &str, menu: &str) -> bool {
        if self.text.is_empty() {
            return true;
        }
        let words = [label, menu, r.command.as_str()];
        if words.iter().any(|w| w.to_lowercase().contains(&self.text)) {
            return true;
        }
        match &self.key {
            Some(k) if k.ctrl || k.alt || k.shift => r.keys.contains(k),
            Some(k) => r.keys.iter().any(|x| x.code == k.code),
            None => r
                .keys
                .iter()
                .any(|x| x.to_string().to_lowercase().contains(&self.text) || x.pretty(Platform::current()).to_lowercase().contains(&self.text)),
        }
    }
}
