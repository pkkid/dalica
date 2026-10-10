//! Application commands: shortcuts (`menus-and-shortcuts.md` §1.9, §2.0). The shortcut editor and
//! agents both rebind keys through them; the overrides are saved in `settings.json`.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::keymap::Key;
use crate::registry::{CommandSpec, Context, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "app.setShortcut",
            label: "Set Shortcut",
            menu: "",
            keys: &[],
            params: r#"{"command":id, "context":"global"|"library"|"editView"|…, "key":"Ctrl+8"|null, "replace"?:bool}: null removes the command's keys in that context; a clash is refused unless replace"#,
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
    #[serde(default)]
    replace: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reset {
    command: Option<String>,
}

fn set_shortcut(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Set { command, context, key, replace } = params(p)?;
    if s.spec(&command).is_none() {
        return Err(CmdError::NotFound(format!("no command `{command}`")));
    }
    let key = match key {
        Value::Null => None,
        Value::String(k) => Some(Key::parse(&k).map_err(CmdError::BadParams)?),
        other => return Err(CmdError::BadParams(format!("key must be a string or null, not {other}"))),
    };
    s.change_keymap(|m| m.set(&command, context, key.clone(), replace)).map_err(CmdError::BadParams)?;
    let keys: Vec<String> = s.keymap().keys_for(&command, context).iter().map(Key::to_string).collect();
    Ok(json!({ "command": command, "context": context, "keys": keys }))
}

fn reset_shortcuts(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Reset { command } = params(p)?;
    if let Some(c) = &command
        && s.spec(c).is_none()
    {
        return Err(CmdError::NotFound(format!("no command `{c}`")));
    }
    s.change_keymap(|m| m.reset(command.as_deref()));
    Ok(json!({ "reset": command.unwrap_or_else(|| "all".into()) }))
}
