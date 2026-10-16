//! Renaming photos (`organizing.md` §9): File ▸ Rename… and F2 open Rename Files
//! (`crate::dialogs::rename_files`) for the selection, whose Rename runs the engine's
//! `photo.rename {ids, name, addDate?, addSize?}` (M9.14). [`RENAME`] is the opener of
//! `photo.rename` (a [`super::Opener`]): a click, F2 or `ui.menu.invoke` opens the dialog, while
//! `engine.execute` runs the engine command with its input (`control-protocol.md` §5.3).

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::Value;

use super::{Opener, parse_object};
use crate::app::App;
use crate::dialogs::rename_files;
use crate::state::RenameDraft;

/// File ▸ Rename… (F2): opens Rename Files for the selection. F2 is bound in the Library only:
/// the edit view's key dispatch runs a command with the open photo and never opens an opener's
/// dialog, so F2 there would run `photo.rename` without a name.
pub const RENAME: Opener = Opener { id: "photo.rename", dialog: "renameFiles", keys: &[(Context::Library, "F2")], open, check };

/// Why there is nothing to rename.
pub const NOTHING: &str = "Select some photos first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Option<Vec<PhotoId>>,
}

/// The photos renamed: those named, else the selection.
fn renamed(app: &App, params: &Value) -> Result<Vec<PhotoId>, CmdError> {
    let p: Ids = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => super::albums::known_photos(app, ids)?,
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() { Err(CmdError::Disabled(NOTHING.into())) } else { Ok(ids) }
}

fn check(app: &App, params: &Value) -> Result<(), String> {
    renamed(app, params).map(|_| ()).map_err(|e| e.to_string())
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let ids = renamed(app, params)?;
    rename_files::open(app, RenameDraft { ids, ..RenameDraft::default() })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn rename_opens_rename_files_for_the_selection() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert!(matches!(open(&mut app, &json!({})), Err(CmdError::Disabled(_))), "nothing selected");
        app.run("select.set", &json!({ "ids": [ids[0], ids[2]] })).unwrap();
        assert_eq!(open(&mut app, &json!({})).unwrap(), json!({ "dialog": "renameFiles" }));
        assert_eq!(app.state().rename_files.as_ref().map(|d| d.ids.clone()), Some(vec![ids[0], ids[2]]));
        crate::dialogs::cancel(&mut app).unwrap();
        for bad in [json!({ "ids": 1 }), json!({ "name": "x" }), json!([])] {
            assert!(matches!(open(&mut app, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(open(&mut app, &json!({ "ids": [9999] })), Err(CmdError::NotFound(_))));
        assert!(app.state().dialogs.is_empty());
    }
}
