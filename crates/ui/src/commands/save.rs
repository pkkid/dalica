//! Save, Undo Save and Revert (`data-model.md` §6) are the engine's commands (`photo.save`,
//! `photo.undoSave`, `photo.revert`, with `photo.saveCopy`); the UI only says which photos they act
//! on, here: in the library the selection, like every command that takes `ids`
//! (`crate::keys::params_for`); in the edit view the photo it shows (File ▸ Save and Ctrl+S save
//! the open photo). Save and Revert ask first through the host (`crate::dialogs::save_explain`,
//! `crate::dialogs::revert`).
//!
//! The other ways in (M8.19): the section header's Save text button and disk button run
//! `photo.save` with the folder's photos that have unsaved edits
//! (`crate::panels::light_table::header`), and the edit panel's Undo reads "Undo Save" for a saved
//! photo without newer edits and runs `photo.undoSave` on it (`crate::panels::edit::history`).

use dalica_core::PhotoId;
use serde_json::{Value, json};

use crate::app::App;

/// The engine's commands that save, undo a save or revert photos.
pub const COMMANDS: &[&str] = &["photo.save", "photo.undoSave", "photo.revert", "photo.saveCopy"];

/// The photos `id` acts on when a click or a key runs it in the edit view: the open photo. `None`
/// for other commands and in the library.
pub fn open_photo_ids(app: &App, id: &str) -> Option<Value> {
    let photo: PhotoId = crate::commands::edit_view::open_photo(app)?;
    COMMANDS.contains(&id).then(|| json!([photo]))
}

/// Routes `id` run with `params`: in the edit view, one of [`COMMANDS`] run without `ids` acts on
/// the open photo. `None`: run it as it is.
pub fn route(app: &App, id: &str, params: &Value) -> Option<Value> {
    if params.get("ids").is_some() {
        return None;
    }
    let ids = open_photo_ids(app, id)?;
    let mut obj = match params {
        Value::Object(o) => o.clone(),
        Value::Null => serde_json::Map::new(),
        _ => return None,
    };
    obj.insert("ids".into(), ids);
    Some(Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::View;

    #[test]
    fn in_the_edit_view_the_save_commands_act_on_the_open_photo() {
        let mut app = App::new(None);
        assert_eq!(route(&app, "photo.save", &json!({})), None, "the library passes the selection");
        app.state_mut().view = View::Edit;
        app.state_mut().edit.photo = Some(PhotoId::new(9));
        for id in COMMANDS {
            assert_eq!(route(&app, id, &json!({ "confirm": true })), Some(json!({ "confirm": true, "ids": [9] })), "{id}");
            assert_eq!(route(&app, id, &Value::Null), Some(json!({ "ids": [9] })));
        }
        assert_eq!(route(&app, "photo.save", &json!({ "ids": [2] })), None, "ids named are kept");
        assert_eq!(route(&app, "photo.save", &json!([1])), None, "the command refuses what is not an object");
        assert_eq!(route(&app, "photo.star.toggle", &json!({})), None);
        assert_eq!(open_photo_ids(&app, "photo.revert"), Some(json!([9])));
    }
}
