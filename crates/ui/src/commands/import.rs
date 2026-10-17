//! The Import room's commands (`organizing.md` §3): `import.open` (File ▸ Import From…, Ctrl+M, the
//! toolbar's Import) opens the room as a tab and brings it to the front; in the room, X toggles
//! exclude on the selected items (`import.toggleExclude`) and Enter imports them
//! (`import.importSelected`), the keys of `menus-and-shortcuts.md` §2.4 in the Import context.
//!
//! M11.2's stubs: `import.open` opens an empty room; the other two wait for the import listing and
//! copying (M11.7, M11.8) and say so. M11.15 fills the room and its tray, M11.20 its destination
//! and the import itself; they run the engine's `import.*` commands on the room's selection.

use dalica_engine::{CmdError, Context};
use serde_json::Value;

use super::{Check, UiCommand, no_params};
use crate::app::App;
use crate::state::Room;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "import.open",
        label: "Import From…",
        menu: "File/Import From…",
        keys: &[(Context::Global, "Ctrl+M")],
        params: r#"{}: open the Import room, or bring it to the front; replies {room: "import"}"#,
        run: open,
    },
    UiCommand {
        id: "import.toggleExclude",
        label: "Exclude or Include",
        menu: "",
        keys: &[(Context::Import, "X")],
        params: "{}: exclude the Import room's selected items from the import, or include them again",
        run: toggle_exclude,
    },
    UiCommand {
        id: "import.importSelected",
        label: "Import Selected",
        menu: "",
        keys: &[(Context::Import, "Enter")],
        params: "{}: import the Import room's selected items to the destination drafted",
        run: import_selected,
    },
];

/// Why the room's actions cannot run yet.
pub const NOT_YET: &str = "Arrives with importing.";

pub const CHECKS: &[(&str, Check)] = &[("import.toggleExclude", not_yet), ("import.importSelected", not_yet)];

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Ok(super::rooms::open(app, Room::Import))
}

fn toggle_exclude(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn import_selected(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn import_open_opens_the_room_once_and_brings_it_to_the_front() {
        let mut app = App::new(None);
        assert_eq!(app.run("import.open", &json!({})).unwrap(), json!({ "room": "import" }));
        app.run("room.select", &json!({ "room": "library" })).unwrap();
        assert_eq!(app.run("import.open", &Value::Null).unwrap(), json!({ "room": "import" }));
        assert_eq!((app.state().rooms.as_slice(), app.state().room), (&[Room::Library, Room::Import][..], Room::Import));
        assert!(matches!(app.run("import.toggleExclude", &json!({})), Err(CmdError::Disabled(_))));
        assert!(matches!(app.run("import.importSelected", &json!({ "ids": [1] })), Err(CmdError::BadParams(_))));
    }
}
