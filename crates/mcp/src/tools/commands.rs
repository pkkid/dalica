//! One tool per command (`mcp.md` §4.4): `cmd_` + the command id with `.` replaced by `_`, for every
//! entry `engine.commands` lists (the engine's commands, and the app's `UI_COMMANDS` in connect
//! mode; menu-catalog items whose command is not registered, `live: false`, are left out). The tool
//! runs its command as `command_run` does (`engine.execute`), its arguments being the command's
//! params, checked by the command itself. `--compact` leaves this group out; `command_run` still
//! reaches every command.
//!
//! The annotation hints are derived, so `CommandSpec` gains no field (`architecture.md` §3):
//!
//! | Effect | `readOnlyHint` | `destructiveHint` | `idempotentHint` |
//! |---|---|---|---|
//! | `query` | true | false | true |
//! | `library`, `files`, `view` | false | when it asks first: its params doc names `"confirm"`, or it is in [`ASKS_FIRST`] | false |
//! | `external`, or none given | false | true | false |
//!
//! `openWorldHint` is false everywhere (`mcp.md` §3).
//!
//! After LightCraft `crates/mcp/src/tools.rs` (`command_tools`, `command_tool_name`,
//! `annotations`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: the hints come
//! from the command's effect and its confirmation instead of a fixed list of tool names, the menu
//! path is a Dalica menu string, entries that are not live are left out, and a call finds its
//! command in the listing rather than turning `_` back into `.`.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

use super::{Tool, ToolResult};
use crate::backend::{Backend, NoProgress, Progress};

/// The prefix of a command's tool name.
pub(crate) const PREFIX: &str = "cmd_";

/// Commands the control channel asks first for (`needs_confirm`, `control-protocol.md` §5.5) whose
/// params doc does not name `"confirm"`: the `CONFIRM` lists of the headless backend and of the
/// app's control channel. `crates/mcp/tests/command_tools.rs` runs every command and fails when
/// one answering `needs_confirm` is not marked destructive, so a command added to those lists
/// without its params doc naming `"confirm"` is caught.
pub(crate) const ASKS_FIRST: &[&str] = &["library.rebuild"];

/// The most distinct tool names and titles kept (see [`intern`]); far over the registry's size.
const MAX_INTERNED: usize = 8192;

/// The tool name of command `id`: `photo.star.toggle` → `cmd_photo_star_toggle`.
pub(crate) fn tool_name(id: &str) -> String {
    format!("{PREFIX}{}", id.replace(['.', ' '], "_"))
}

/// This group's tools: one per live entry of `engine.commands`.
pub(crate) fn tools(b: &mut dyn Backend) -> Vec<Tool> {
    listing(b).iter().filter_map(tool).collect()
}

/// Runs `name` if it is one of this group's tools: its command through `engine.execute` with the
/// arguments as its params (a job it starts is waited for, reporting progress).
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    if !name.starts_with(PREFIX) {
        return None;
    }
    let id = listing(b).iter().filter_map(|c| c.get("id").and_then(Value::as_str)).find(|id| tool_name(id) == name).map(str::to_owned)?;
    Some(ToolResult::from(b.call("engine.execute", json!({ "command": id, "params": args }), progress)))
}

/// The live entries of `engine.commands` (none when the backend cannot list them: `tools/list`
/// still answers with the helpers).
fn listing(b: &mut dyn Backend) -> Vec<Value> {
    match b.call("engine.commands", json!({}), &mut NoProgress) {
        Ok(Value::Array(list)) => {
            list.into_iter().filter(|c| c.get("live") != Some(&Value::Bool(false)) && c.get("id").is_some_and(Value::is_string)).collect()
        }
        Ok(other) => {
            log::warn!("engine.commands answered something that is not a list: {other}");
            Vec::new()
        }
        Err(e) => {
            log::warn!("the commands cannot be listed: {e}");
            Vec::new()
        }
    }
}

/// `(readOnlyHint, destructiveHint, idempotentHint)` of an `engine.commands` entry.
fn hints(entry: &Value) -> (bool, bool, bool) {
    match entry.get("effect").and_then(Value::as_str) {
        Some("query") => (true, false, true),
        Some("library" | "files" | "view") => (false, asks_first(entry), false),
        _ => (false, true, false),
    }
}

/// Whether the command asks first in the app (`needs_confirm` until its params carry
/// `"confirm": true`).
fn asks_first(entry: &Value) -> bool {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
    entry.get("params").and_then(Value::as_str).is_some_and(|p| p.contains("\"confirm\"")) || ASKS_FIRST.contains(&id)
}

/// The tool of one `engine.commands` entry: the label as title; the description from the label,
/// the id, the menu path, the shortcut and the params doc; a loose argument object.
fn tool(entry: &Value) -> Option<Tool> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    let id = text("id")?;
    let label = text("label").unwrap_or(id);
    let mut description = format!("{label} (command `{id}`).");
    if let Some(menu) = text("menu") {
        description.push_str(&format!(" Menu: {}.", menu_path(menu)));
    }
    if let Some(shortcut) = text("shortcut") {
        description.push_str(&format!(" Shortcut: {shortcut}."));
    }
    if let Some(params) = text("params") {
        description.push_str(&format!(" Params: {params}"));
    }
    if ASKS_FIRST.contains(&id) {
        description.push_str(" Asks first in the app: needs \"confirm\": true.");
    }
    let (read_only, destructive, idempotent) = hints(entry);
    if read_only {
        description.push_str(" Changes nothing.");
    }
    Some(Tool {
        name: intern(&tool_name(id))?,
        title: intern(label)?,
        description,
        input: json!({ "type": "object", "description": "The command's params (its params doc)", "additionalProperties": true }),
        output: None,
        read_only,
        destructive,
        idempotent,
    })
}

/// A Dalica menu string (`Picture/Star / Unstar`) as `Picture ▸ Star / Unstar`: a `/` between two
/// words separates menus, one with a space beside it belongs to the label.
fn menu_path(menu: &str) -> String {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in menu.char_indices() {
        let before = menu.get(..i).and_then(|s| s.chars().last());
        let after = menu.get(i.saturating_add(1)..).and_then(|s| s.chars().next());
        if c == '/' && before.is_some_and(|b| b != ' ') && after.is_some_and(|a| a != ' ') {
            parts.push(menu.get(start..i).unwrap_or_default());
            start = i.saturating_add(1);
        }
    }
    parts.push(menu.get(start..).unwrap_or_default());
    parts.join(" ▸ ")
}

/// `text` as a `&'static str`, kept once for the process: a tool's name and title are static
/// (`Tool`), while the commands' names arrive as JSON (`engine.commands`, from the app in connect
/// mode). The set is the registry's names and labels, so it stays small; past [`MAX_INTERNED`]
/// distinct texts (an app answering ever new names) a command gets no tool, and `command_run`
/// still reaches it.
fn intern(text: &str) -> Option<&'static str> {
    static KEPT: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(s) = kept.get(text) {
        return Some(s);
    }
    if kept.len() >= MAX_INTERNED {
        log::warn!("more than {MAX_INTERNED} command tool names; `{text}` gets no tool (command_run still runs it)");
        return None;
    }
    let s: &'static str = Box::leak(text.to_owned().into_boxed_str());
    kept.insert(s);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases the registry does not have (a UI command's `view`, an entry without an effect)
    /// and the menu strings; the registry's own are `tests/command_tools.rs`'s.
    #[test]
    fn names_menus_and_hints() {
        assert_eq!(tool_name("photo.moveToNewFolder"), "cmd_photo_moveToNewFolder");
        assert_eq!(menu_path("Picture/Star / Unstar"), "Picture ▸ Star / Unstar");
        assert_eq!(menu_path("File/Export Pictures to Folder…"), "File ▸ Export Pictures to Folder…");
        assert_eq!(menu_path("Tools/Batch Edit/Rotate"), "Tools ▸ Batch Edit ▸ Rotate");
        assert_eq!(hints(&json!({ "id": "pane.tags", "effect": "view" })), (false, false, false));
        assert_eq!(hints(&json!({ "id": "a.b" })), (false, true, false), "no effect given: conservatively destructive");
        assert_eq!(intern("cmd_x").map(str::as_ptr), intern("cmd_x").map(str::as_ptr), "kept once");
    }
}
