//! The Options dialog's commands (`menus-and-shortcuts.md` §5, §1.9; `design-system.md` §11.3),
//! over the catalogue in [`crate::options`], after PhotoCraft's `prefs.get` / `prefs.set`:
//!
//! - `options.list {tab?}` (query): the catalogue's rows, in the dialog's order, with the values in
//!   effect and whether each can be changed in this build ([`crate::options::OptionSpec::to_json`]).
//! - `options.set {values}`: `{key: value}`, every key and value checked against the catalogue
//!   before anything changes (an unknown key, an option whose feature is still to come, a wrong
//!   type or a value out of range is `bad_params` naming the option; nothing is stored). The new
//!   document is written to `settings.json` once, atomically, on the I/O worker, and only once it
//!   is on disk does the session take it: a failed write is `disabled` with the reason (a
//!   read-only file is refused as one, even where the system would let a rename replace it) and
//!   the old values stay. No undo step: the dialog's Cancel and Apply are the way back.
//!
//! What a change does: a File Types or small-picture threshold change starts one scan of the
//! watched folders ([`Session::rescan_for_file_types`]); the metadata write options act from the
//! next write and the export defaults from the next `export.defaults` (both read the settings each
//! time); options that apply at the next launch (Use system title bar) are listed in the result and
//! never applied to the running window; the rest are read by the UI every frame. The result:
//! `{changed: [key], restart: [key], rescan: bool, job: id | null}` (`job` is the scan started;
//! `rescan` without a job while a file job runs, after which it starts).

use std::path::Path;
use std::sync::mpsc;

use dalica_fs::Expect;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::CmdError;
use crate::options::{Applies, OptionSpec, Tab, catalogue, find};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::{Msg, Session};
use crate::settings::FILE;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "options.list",
            label: "List Options",
            menu: "",
            keys: &[],
            params: r#"{"tab"?:"general"|"fileTypes"|"slideshow"|"network"|"people"|"viewer"|"export"}: the options of one tab of Options, or of all of them, each {key, tab, group, label, hint, type, default, value, applies, available, reason, platforms, min?, max?, step?, unit?, choices?, maxBytes?}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "options.set",
            label: "Set Options",
            menu: "",
            keys: &[],
            params: r#"{"values":{key:value, …}}: keys and values as options.list gives them; all checked before anything changes; one write of settings.json; no undo step"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: set,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    tab: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    values: Map<String, Value>,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let List { tab } = params(p)?;
    let tab = match tab {
        None => None,
        Some(id) => Some(Tab::from_id(&id).ok_or_else(|| {
            let ids: Vec<String> = Tab::ALL.iter().map(|t| format!("\"{}\"", t.id())).collect();
            CmdError::BadParams(format!("tab must be one of {}", ids.join(", ")))
        })?),
    };
    let rows: Vec<Value> = catalogue().iter().filter(|o| tab.is_none_or(|t| o.tab == t)).map(|o| o.to_json(s.settings())).collect();
    Ok(json!({ "options": rows }))
}

/// The longest part of an unknown key an error message repeats.
const KEY_ECHO: usize = 64;

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Set { values } = params(p)?;
    // Every value is checked, and the whole document made, before anything changes.
    let mut next = s.settings().clone();
    for (key, value) in &values {
        let option = find(key).ok_or_else(|| CmdError::BadParams(format!("no option `{}`: options.list lists them", echo(key))))?;
        if let Some(reason) = option.unavailable {
            return Err(CmdError::BadParams(format!("{key} cannot be changed in this build: {reason}")));
        }
        option.store(&mut next, value).map_err(CmdError::BadParams)?;
    }
    let changed: Vec<&OptionSpec> = catalogue().iter().filter(|o| values.contains_key(o.key) && (o.read)(&next) != (o.read)(s.settings())).collect();
    // A stored value read as its default (one of the wrong type) is still written over.
    if next != *s.settings() {
        let bytes = next.print();
        write_settings(s, bytes).map_err(|why| CmdError::Disabled(format!("The options could not be saved: {why}. Nothing was changed.")))?;
        // On disk: the session takes the document (its own write of the same bytes follows).
        s.change_settings(|st| *st = next);
    }
    let rescan = changed.iter().any(|o| o.applies == Applies::Rescan);
    let job = if rescan { s.rescan_for_file_types() } else { None };
    let keys = |applies: Option<Applies>| -> Vec<&str> { changed.iter().filter(|o| applies.is_none_or(|a| o.applies == a)).map(|o| o.key).collect() };
    Ok(json!({
        "changed": keys(None),
        "restart": keys(Some(Applies::Restart)),
        "rescan": rescan,
        "job": job,
    }))
}

/// At most [`KEY_ECHO`] bytes of `key`, cut at a character boundary.
fn echo(key: &str) -> &str {
    let mut end = key.len().min(KEY_ECHO);
    while !key.is_char_boundary(end) {
        end -= 1;
    }
    key.get(..end).unwrap_or_default()
}

/// Writes `bytes` as the library's `settings.json` on the I/O worker, in order with the writes
/// sent before, and waits for the outcome.
fn write_settings(s: &mut Session, bytes: Vec<u8>) -> Result<(), String> {
    let path = s.dir().join(FILE);
    let (tx, rx) = mpsc::channel();
    s.send(Msg::Run(Box::new(move || {
        let _ = tx.send(replace(&path, &bytes));
    })));
    rx.recv().unwrap_or_else(|_| Err("the I/O worker stopped".into()))
}

/// Replaces `path` with `bytes` atomically, over the version there now; a file marked read-only is
/// left alone.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let expect = match dalica_fs::stat(path).map_err(|e| e.to_string())? {
        Some(st) if !st.writable => return Err(format!("{FILE} is read-only")),
        Some(st) => Expect::Unchanged(st.identity),
        None => Expect::Absent,
    };
    dalica_fs::replace(path, bytes, expect).map(|_| ()).map_err(|e| e.to_string())
}
