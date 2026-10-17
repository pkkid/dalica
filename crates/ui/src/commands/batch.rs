//! Batch editing in the window (`editing.md` §9, `menus-and-shortcuts.md` §1.2 and §1.5): Picture ▸
//! Batch Edit ▸ on the tray's photos (`edit.apply`), Edit ▸ Copy All Edits and Paste All Edits
//! (`edit.copyRecipe`, `edit.pasteRecipe`) and Picture ▸ Undo All Edits… (`edit.undoAll`), all
//! engine commands; the UI's part is what they act on and Undo All Edits' question.
//!
//! M11.2's ground: Undo All Edits… is an [opener](super::Opener): its menu item opens the question
//! "Remove all edits from %d pictures?" (`dialog:undoAllEdits`, `crate::dialogs::undo_all`) for the
//! tray's photos, whose default button runs `edit.undoAll`. It waits for M11.18 (and the engine's
//! command) and says so; the menu rows of the engine commands go live as the engine registers them.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::Value;

use super::{Opener, UiCommand, parse_object};
use crate::app::App;

/// No UI command of its own yet: Batch Edit's rows run engine commands.
pub const COMMANDS: &[UiCommand] = &[];

/// Why Undo All Edits… cannot open yet.
pub const NOT_YET: &str = "Arrives with Undo All Edits.";

/// Picture ▸ Undo All Edits…: its question for the tray's photos.
pub const UNDO_ALL: Opener = Opener { id: "edit.undoAll", dialog: "undoAllEdits", keys: &[], open: open_undo_all, check: |_, _| Err(NOT_YET.into()) };

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    #[serde(default)]
    #[allow(dead_code)] // read by M11.18
    ids: Option<Vec<dalica_core::PhotoId>>,
}

fn open_undo_all(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    let _: Ids = parse_object(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}
