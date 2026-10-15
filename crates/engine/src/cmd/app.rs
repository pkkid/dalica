//! Application commands: shortcuts (`menus-and-shortcuts.md` §1.9, §2.0). The shortcut editor and
//! agents both rebind keys through them; the overrides are saved in `settings.json`. A command has
//! up to two keys per context (slots 0 and 1); a key another command already has where both can be
//! active is refused with the editor's sentence ("Ctrl+1 is already Small Thumbnails in
//! Library."), unless `replace`, which takes it from that command.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::keymap::{KEYS_PER_CONTEXT, Key, SetError, context_label, is_hold, overlapping};
use crate::registry::{CommandSpec, Context, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "app.setShortcut",
            label: "Set Shortcut",
            menu: "",
            keys: &[],
            params: r#"{"command":id, "context":"global"|"library"|"editView"|…, "key":"Ctrl+8"|null, "slot"?:0|1, "replace"?:bool}: without slot the key replaces the command's keys in that context (null removes them); with slot only that key is set or removed (null), the other kept. Keys as written or read on screen ("ctrl-8", "⇧⌘S"). A key another command has in a context active at the same time is refused, naming it ("Ctrl+1 is already Small Thumbnails in Library."), unless replace, which takes it from that command. Returns {command, context, keys, took: [{command, context}]}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: set_shortcut,
        },
        CommandSpec {
            id: "app.resetShortcuts",
            label: "Reset Shortcuts",
            menu: "",
            keys: &[],
            params: r#"{"command"?:id}: drop the user's bindings for one command, or for all"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: reset_shortcuts,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    command: String,
    context: Context,
    /// Required: a key, or `null` to remove the command's keys in this context.
    key: Value,
    slot: Option<usize>,
    #[serde(default)]
    replace: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reset {
    command: Option<String>,
}

fn set_shortcut(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Set { command, context, key, slot, replace } = params(p)?;
    // The engine's commands and the UI commands the app added (`Session::add_ui_keys`).
    if !s.keymap().knows(&command) {
        return Err(CmdError::NotFound(format!("no command `{command}`")));
    }
    if is_hold(&command) {
        return Err(CmdError::Disabled(format!("{} is held, not pressed; its key cannot be changed", label(s, &command))));
    }
    let key = match key {
        Value::Null => None,
        Value::String(k) => Some(Key::parse_lenient(&k).map_err(CmdError::BadParams)?),
        other => return Err(CmdError::BadParams(format!("key must be a string or null, not {other}"))),
    };
    // Who loses the key when `replace` takes it.
    let took: Vec<Value> = match (&key, replace) {
        (Some(k), true) => s
            .keymap()
            .bindings()
            .into_iter()
            .filter(|b| &b.key == k && b.command != command && overlapping(b.context, context))
            .map(|b| json!({ "command": b.command, "context": b.context }))
            .collect(),
        _ => Vec::new(),
    };
    // Refused before anything changes, so a refusal writes nothing.
    if let Some(n) = slot.filter(|n| *n >= KEYS_PER_CONTEXT) {
        return Err(CmdError::BadParams(SetError::Slot(n).to_string()));
    }
    if let Some(k) = &key
        && !replace
        && let Some(b) = s.keymap().clash(&command, context, k)
    {
        return Err(CmdError::BadParams(format!("{} is already {} in {}.", b.key, label(s, &b.command), context_label(b.context))));
    }
    s.change_keymap(|m| m.set(&command, context, slot, key, replace)).map_err(|e| CmdError::BadParams(e.to_string()))?;
    let keys: Vec<String> = s.keymap().keys_for(&command, context).iter().map(Key::to_string).collect();
    Ok(json!({ "command": command, "context": context, "keys": keys, "took": took }))
}

/// The label a command goes by: its spec's, or its id for a UI command (the engine knows only the
/// ids of those).
pub(crate) fn label<'a>(s: &'a Session, command: &'a str) -> &'a str {
    s.spec(command).map_or(command, |spec| spec.label)
}

fn reset_shortcuts(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Reset { command } = params(p)?;
    if let Some(c) = &command
        && !s.keymap().knows(c)
    {
        return Err(CmdError::NotFound(format!("no command `{c}`")));
    }
    s.change_keymap(|m| m.reset(command.as_deref()));
    Ok(json!({ "reset": command.unwrap_or_else(|| "all".into()) }))
}
