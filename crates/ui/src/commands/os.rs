//! The OS integration in the window (`organizing.md` §9, `viewing.md` §3.3), stubs until M15.22 and
//! M15.24 make it act: Locate on Disk, Open With ▸, Open in External Editor, Copy Path, the export
//! notice's Show Folder, the viewer's Show in File Manager and the default-viewer question.
//!
//! **The engine's commands.** Locate on Disk, Open With, Open in External Editor, Copy Path and
//! the system clipboard are the engine's (M15.17: `photo.locate {id}`, `folder.locate {id}`,
//! `photo.openWithList {id}`, `photo.openWith {id, handler}`, `photo.openExternal {ids}`,
//! `photo.copyToSystem {ids, picture?}`, `photo.copyPath {ids}`), through its `Os` service; the
//! menu rows of `menus-and-shortcuts.md` §1 run them by those ids. A UI command never takes one of
//! them (a UI command of the same id would hide the engine's from `engine.execute`, M13.4's rule,
//! and an automation client must meet the engine's `external` check). What the UI adds is which
//! photo or folder a click or a key acts on ([`params_for`]: the focused photo, the open one in the
//! edit view and the viewer, the selection, the current folder) and the reason their rows are
//! greyed while the engine lacks them ([`LATER`], [`AWAITED`]).
//!
//! **The UI's own commands**, for what has no engine command of its own:
//!
//! - **`openWith.list {id?}`**: the Open With ▸ submenu opening asks the operating system for the
//!   applications that open the photo (the engine's `photo.openWithList`), "Loading…" until they
//!   come (`openWith`: `{photo, handlers: null}`), "No other applications" when none do;
//!   **`openWith.close`** forgets them when the submenu closes. Stub: [`LATER`] until the engine
//!   has `photo.openWithList`.
//! - **`toast.showFolder {path}`**: the export notice's Show Folder, the folder selected in the
//!   system's file manager (`creating.md` §2). Stub: [`LATER`] (the notice keeps bringing the
//!   folder up in the library until M15.22).
//! - **`viewer.showInFileManager`**: the photo viewer's ⋯ ▸ Show in File Manager, the photo shown
//!   selected in the file manager. Stub: [`LATER`].
//! - **`defaultViewer.ask`**: the default-viewer question offered once after the first run
//!   (`dialog:defaultViewer`, nothing chosen; M15.24 asks it and makes its Use Dalica act).

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::default_viewer;
use crate::state::{MAX_LOCATION, OpenWithMenu, View};

/// What the OS integration's stubs say, and why their menu rows are greyed, until it lands.
pub const LATER: &str = "Arrives with OS integration.";
/// Why a command on a photo cannot run: none is focused.
pub const NO_PHOTO: &str = "Select a photo first.";
/// Why the viewer's Show in File Manager cannot run: the viewer is not open.
pub const NO_VIEWER: &str = "The photo viewer is not open.";
/// The engine's commands of the OS integration (M15.17): their menu rows say [`LATER`] while the
/// engine lacks them.
pub const AWAITED: &[&str] =
    &["photo.locate", "folder.locate", "photo.openWithList", "photo.openWith", "photo.openExternal", "photo.copyToSystem", "photo.copyPath"];
/// The engine's command that lists a file's applications (M15.17).
pub const LIST_HANDLERS: &str = "photo.openWithList";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "openWith.list",
        label: "Open With",
        menu: "",
        keys: &[],
        params: "{id?: PhotoId}: open the Open With ▸ submenu for the photo (the focused one without `id`): its applications are asked of the system (photo.openWithList); replies {photo}",
        run: list,
    },
    UiCommand {
        id: "openWith.close",
        label: "Close Open With",
        menu: "",
        keys: &[],
        params: "{}: forget the Open With ▸ submenu's applications (the submenu closed)",
        run: close,
    },
    UiCommand {
        id: "toast.showFolder",
        label: "Show Folder",
        menu: "",
        keys: &[],
        params: "{path: string}: show an exported folder (an absolute path) in the system's file manager",
        run: show_folder,
    },
    UiCommand {
        id: "viewer.showInFileManager",
        label: "Show in File Manager",
        menu: "",
        keys: &[],
        params: "{}: show the photo the viewer shows, selected, in the system's file manager",
        run: show_in_file_manager,
    },
    UiCommand {
        id: "defaultViewer.ask",
        label: "Use Dalica as the Default Viewer",
        menu: "",
        keys: &[],
        params: "{}: ask which file types Dalica opens by default (dialog:defaultViewer, nothing chosen); replies {dialog}",
        run: ask_default_viewer,
    },
];

/// The OS integration's commands grey their rows and buttons while they wait for M15.22.
pub const CHECKS: &[(&str, Check)] = &[("openWith.list", list_ready), ("toast.showFolder", waits), ("viewer.showInFileManager", viewer_waits)];

fn waits(_: &App, _: &Value) -> Result<(), String> {
    Err(LATER.into())
}

fn viewer_waits(app: &App, _: &Value) -> Result<(), String> {
    if app.state().view != View::Viewer {
        return Err(NO_VIEWER.into());
    }
    Err(LATER.into())
}

fn list_ready(app: &App, _: &Value) -> Result<(), String> {
    focused(app).ok_or_else(|| NO_PHOTO.to_string())?;
    if app.session().and_then(|s| s.spec(LIST_HANDLERS)).is_none() {
        return Err(LATER.into());
    }
    Ok(())
}

/// The photo a command on one photo acts on: the edit view's open photo, else the focused photo of
/// the selection (its anchor, else its first). The viewer's photo is the viewer's own
/// (`viewer.showInFileManager`).
pub fn focused(app: &App) -> Option<PhotoId> {
    let state = app.state();
    if state.view == View::Edit {
        return state.edit.photo;
    }
    let sel = &state.selection;
    sel.anchor.filter(|a| sel.contains(*a)).or_else(|| sel.ids.first().copied())
}

/// The photos a command on photos acts on: the open photo in the edit view, else the selection.
fn photos(app: &App) -> Vec<PhotoId> {
    let state = app.state();
    match (state.view, state.edit.photo) {
        (View::Edit, Some(p)) => vec![p],
        _ => state.selection.ids.clone(),
    }
}

/// The parameters a click on a row of the OS integration (or its key, Ctrl+Enter, Ctrl+Shift+O)
/// passes to the engine's command `id`: `photo.locate`, `photo.openWithList` and `photo.openWith`
/// the focused photo (`{id}`), `folder.locate` the current folder (`{id}`), `photo.openExternal`,
/// `photo.copyToSystem` and `photo.copyPath` the photos acted on (`{ids}`). `None` for other ids.
pub fn params_for(app: &App, id: &str) -> Option<Value> {
    let mut p = Map::new();
    match id {
        "photo.locate" | "photo.openWithList" | "photo.openWith" => {
            if let Some(photo) = focused(app) {
                p.insert("id".into(), photo.get().into());
            }
        }
        "folder.locate" => {
            if let Some(folder) = crate::menus::current_folder(app) {
                p.insert("id".into(), folder.get().into());
            }
        }
        "photo.openExternal" | "photo.copyToSystem" | "photo.copyPath" => {
            p.insert("ids".into(), json!(photos(app)));
        }
        _ => return None,
    }
    Some(Value::Object(p))
}

/// Why a menu row running the engine's command `id` is greyed while the engine lacks it: [`LATER`]
/// for the OS integration's ([`AWAITED`]), `None` for any other.
pub fn awaited(id: &str) -> Option<&'static str> {
    AWAITED.contains(&id).then_some(LATER)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    #[serde(default)]
    id: Option<u64>,
}

fn list(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: List = parse_object(params)?;
    let photo = match p.id {
        Some(n) => {
            let id = PhotoId::new(n);
            if !app.session().is_some_and(|s| s.library().photo(id).is_some()) {
                return Err(CmdError::NotFound(format!("no photo {n}")));
            }
            id
        }
        None => focused(app).ok_or_else(|| CmdError::Disabled(NO_PHOTO.into()))?,
    };
    if app.session().and_then(|s| s.spec(LIST_HANDLERS)).is_none() {
        return Err(CmdError::Disabled(LATER.into()));
    }
    // "Loading…" until M15.22 runs the engine's `photo.openWithList {id}` on a worker and fills
    // the handlers.
    app.state_mut().open_with = Some(OpenWithMenu { photo, handlers: None });
    Ok(json!({ "photo": photo }))
}

fn close(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().open_with = None;
    Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowFolder {
    path: std::path::PathBuf,
}

fn show_folder(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: ShowFolder = parse(params)?;
    if !p.path.is_absolute() || p.path.as_os_str().len() > MAX_LOCATION {
        return Err(CmdError::BadParams(format!("`path` is an absolute folder path of at most {MAX_LOCATION} bytes")));
    }
    Err(CmdError::Disabled(LATER.into()))
}

fn show_in_file_manager(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    viewer_waits(app, params).map_err(CmdError::Disabled)?;
    Ok(json!({}))
}

fn ask_default_viewer(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    default_viewer::open(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_os_commands_say_what_they_act_on_and_wait() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert_eq!(params_for(&app, "photo.locate"), Some(json!({})), "nothing selected");
        app.run("select.set", &json!({ "ids": [ids[1].get(), ids[2].get()] })).unwrap();
        assert_eq!(params_for(&app, "photo.locate"), Some(json!({ "id": ids[2] })), "the focused photo");
        assert_eq!(params_for(&app, "photo.copyPath"), Some(json!({ "ids": [ids[1], ids[2]] })));
        assert!(params_for(&app, "folder.locate").is_some_and(|p| p["id"].is_u64()), "the current folder");
        assert_eq!(params_for(&app, "photo.star.toggle"), None);
        app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
        assert_eq!(params_for(&app, "photo.openExternal"), Some(json!({ "ids": [ids[0]] })), "the open photo");
        assert_eq!(params_for(&app, "photo.openWith"), Some(json!({ "id": ids[0] })));
        assert_eq!(awaited("photo.locate"), Some(LATER));
        assert_eq!(awaited("photo.star.toggle"), None);

        assert_eq!(app.run("openWith.list", &json!({})).unwrap(), json!({ "photo": ids[0] }), "the engine lists them");
        assert!(matches!(app.run("openWith.list", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("toast.showFolder", &json!({ "path": "relative" })), Err(CmdError::BadParams(_))));
        let abs = std::env::temp_dir();
        assert!(matches!(app.run("toast.showFolder", &json!({ "path": abs })), Err(CmdError::Disabled(m)) if m == LATER));
        assert!(matches!(app.run("viewer.showInFileManager", &json!({})), Err(CmdError::Disabled(m)) if m == NO_VIEWER));
        app.state_mut().open_with = Some(crate::state::OpenWithMenu { photo: ids[0], handlers: None });
        app.run("openWith.close", &json!({})).unwrap();
        assert_eq!(app.state().open_with, None);

        assert_eq!(app.run("defaultViewer.ask", &json!({})).unwrap(), json!({ "dialog": "defaultViewer" }));
        assert_eq!(app.state().default_viewer, Some(crate::state::DefaultViewerDraft::default()), "nothing chosen");
    }
}
