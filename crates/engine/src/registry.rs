//! The command registry (`architecture.md` §3): every command is a [`CommandSpec`], and
//! [`crate::Session::execute`] is the only way library state changes.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::CmdError;
use crate::session::Session;

/// Where a key binding applies (`menus-and-shortcuts.md` §2.0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Context {
    Global,
    Library,
    EditView,
    Slideshow,
    PhotoViewer,
    Import,
    Collage,
    TextInput,
}

/// What a command touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    /// Reads only.
    Query,
    /// Changes the library (and through it, sidecars).
    Library,
    /// Writes to photo folders beyond sidecars.
    Files,
    /// Reaches outside Dalica.
    External,
}

/// Which undo history a command's step goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum History {
    /// One step on the library undo stack.
    Library,
    /// A step of the photo's own history (M7).
    Photo,
    /// No undo step (queries, view changes, undo itself).
    None,
}

/// Why a command is disabled right now: the text the status line shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disabled(pub String);

/// One command.
#[derive(Clone, Copy)]
pub struct CommandSpec {
    /// Stable id, e.g. `photo.star.toggle`.
    pub id: &'static str,
    /// English label (also the i18n key).
    pub label: &'static str,
    /// Menu path (`Picture/Star / Unstar`); empty if not in a menu.
    pub menu: &'static str,
    /// Default key bindings with their contexts; `Ctrl` is the primary modifier.
    pub keys: &'static [(Context, &'static str)],
    /// Agent-readable parameter doc.
    pub params: &'static str,
    pub effect: Effect,
    pub history: History,
    pub enabled: fn(&Session, &Value) -> Result<(), Disabled>,
    pub run: fn(&mut Session, &Value) -> Result<Value, CmdError>,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).finish_non_exhaustive()
    }
}

/// Always enabled.
pub fn always(_: &Session, _: &Value) -> Result<(), Disabled> {
    Ok(())
}

/// Parses a command's parameters — a JSON object; `null` counts as `{}` — into its typed struct; any problem is
/// [`CmdError::BadParams`] naming the field.
pub fn params<T: DeserializeOwned>(value: &Value) -> Result<T, CmdError> {
    let value = match value {
        Value::Null => Value::Object(serde_json::Map::new()),
        Value::Object(_) => value.clone(),
        other => return Err(CmdError::BadParams(format!("parameters must be a JSON object, not {other}"))),
    };
    serde_json::from_value(value).map_err(|e| CmdError::BadParams(e.to_string()))
}

/// What `engine.commands` lists for one command (`control-protocol.md` §6.2).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: &'static str,
    /// The first default binding, if any.
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    pub ui: bool,
    pub checked: Option<bool>,
    pub live: bool,
    pub effect: Effect,
    pub history: History,
}
