//! Options ▸ Keyboard Shortcuts: the shortcut editor (`menus-and-shortcuts.md` §2.0). Its state is
//! the draft's `shortcuts` ([`ShortcutEditor`]): the search text, the Context filter, "Show
//! commands without a shortcut", the key cell recording (whose key presses `crate::keys` hands to
//! [`ShortcutEditor::captured`] instead of the keymap and the dialog), a conflict waiting for an
//! answer, and the changed bindings that Apply or OK run. A stub: it shows that state as text, and
//! [`apply`] refuses a draft that changes bindings, until M10.12 draws the editor and applies it
//! through `app.setShortcut` and `app.resetShortcuts`.

use dalica_engine::CmdError;
use egui::Ui;
use serde_json::{Value, json};

use crate::app::App;
use crate::state::ShortcutEditor;

/// Why a change cannot be applied yet.
pub const NOT_YET: &str = "The shortcut editor cannot apply changes yet.";

pub fn body(app: &mut App, ui: &mut Ui) {
    let Some(e) = app.state().options.as_ref().map(|o| o.shortcuts.clone()) else { return };
    super::super::text_lines(app, ui, &lines(&e));
}

/// The editor's state as text, one line each.
pub fn lines(e: &ShortcutEditor) -> Vec<String> {
    vec![
        format!("Search: {}", e.search),
        format!("Context: {}", as_json(&e.context)),
        format!("Show commands without a shortcut: {}", e.show_unbound),
        format!("Recording: {}", as_json(&e.recording)),
        format!("Conflict: {}", as_json(&e.conflict)),
        format!("Changed bindings: {}{}", e.changes.len(), if e.reset_all { " (Reset All)" } else { "" }),
    ]
}

/// Applies the editor's changes; nothing to do when the draft changes no binding. Refused
/// (`disabled`) while the editor cannot apply them, so a change is never silently dropped.
pub fn apply(_app: &mut App, e: &ShortcutEditor) -> Result<Value, CmdError> {
    if e.changed() { Err(CmdError::Disabled(NOT_YET.into())) } else { Ok(json!({})) }
}

/// `v` as its JSON text (`null` for none).
fn as_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}
