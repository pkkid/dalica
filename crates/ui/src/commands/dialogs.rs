//! Commands that open an in-app dialog on the host (`crate::dialogs`): their effect is the open
//! dialog, a view state, so they are UI commands. The dialog's own buttons (or
//! `ui.dialog.confirm`) then run whatever it changes, as one undo step.
//!
//! - `folders.manager` (Tools ▸ Folder Manager…): the Folder Manager (`organizing.md` §2).

use dalica_engine::CmdError;
use serde_json::Value;

use super::{UiCommand, no_params};
use crate::app::App;
use crate::dialogs::{self, Dialog, DialogKind};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "folders.manager",
    label: "Folder Manager…",
    menu: "Tools/Folder Manager…",
    keys: &[],
    params: "{}: open the Folder Manager (replies {dialog: \"folderManager\"}; answer it with ui.dialog.confirm / ui.dialog.cancel)",
    run: folder_manager,
}];

fn folder_manager(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    dialogs::open(app, Dialog::new(DialogKind::FolderManager))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn folders_manager_opens_the_folder_manager_once() {
        let mut app = App::new(None);
        assert_eq!(app.run("folders.manager", &json!({})).unwrap(), json!({ "dialog": "folderManager" }));
        assert!(matches!(app.run("folders.manager", &Value::Null), Err(CmdError::Disabled(_))), "already open");
        assert_eq!(app.state().dialogs.len(), 1);
        for bad in [json!({ "path": "/x" }), json!([1]), json!("x")] {
            assert!(matches!(app.run("folders.manager", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }
}
