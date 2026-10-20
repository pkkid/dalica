//! The OS integration in the window (`organizing.md` §9, `viewing.md` §3.3): Locate on Disk, Open
//! With ▸, Open in External Editor, Copy Path, the export notice's Show Folder, the viewer's Show
//! in File Manager and Open With ▸, Options ▸ General's External editor, and the default-viewer
//! question (M15.24 makes its Use Dalica act).
//!
//! **The engine's commands.** Locate on Disk, Open With, Open in External Editor, Copy Path and
//! the system clipboard are the engine's (M15.17: `photo.locate {id}`, `folder.locate {id}`,
//! `photo.openWithList {id}`, `photo.openWith {id, handler}`, `photo.openExternal {ids}`,
//! `photo.copyToSystem {ids, picture?}`, `photo.copyPath {ids}`), through its `Os` service; the
//! menu rows of `menus-and-shortcuts.md` §1 run them by those ids. A UI command never takes one of
//! them (a UI command of the same id would hide the engine's from `engine.execute`, M13.4's rule,
//! and an automation client must meet the engine's `external` check). What the UI adds is which
//! photo or folder a click or a key acts on ([`params_for`]: the focused photo, the open one in the
//! edit view and the viewer, the selection, the current folder). A row whose service the
//! operating system cannot give is greyed with the system's reason (the engine's `enabled` asks
//! `Os::available`).
//!
//! **Open With ▸** ([`open_with_rows`], [`sync_open_with`]): the submenu opening asks the operating
//! system for the applications that open the photo (`openWith.list`, which runs the engine's
//! `photo.openWithList`): "Loading…" in the frame it opens, then one row per application by its
//! name (`menuitem:photo.openWith/<n>`, running `photo.openWith {id, handler}`), or "No other
//! applications" when the system offers none; the submenu closing forgets them (`openWith.close`).
//! The photo menu's and the viewer's ⋯ menu's Open With ▸ are drawn from the same rows.
//!
//! **The UI's own commands**, for what has no engine command of its own:
//!
//! - **`openWith.list {id?}`**, **`openWith.close`**: above.
//! - **`toast.showFolder {path}`**: the export notice's Show Folder: the folder selected in the
//!   system's file manager (`creating.md` §2; the notice also brings it up in the library,
//!   `crate::notices::act`), through `folder.locate` for a folder of the library ([`reveal`]).
//! - **`viewer.showInFileManager`**: the photo viewer's ⋯ ▸ Show in File Manager: the photo shown,
//!   selected in the file manager (`photo.locate`, or the file itself for a photo the library does
//!   not hold).
//! - **`app.preferences.editor {path}`**: the program Browse… chose for Options ▸ General ▸
//!   External editor, into the dialog's draft (`files.externalEditor`; Apply saves it). Under
//!   automation it needs `--automation-allow-external`: the program is what a click on Open in
//!   External Editor starts.
//! - **`defaultViewer.ask`**: the default-viewer question offered once after the first run
//!   (`dialog:defaultViewer`, nothing chosen; M15.24 asks it and makes its Use Dalica act).
//!
//! **Only the person launches.** The commands that start a program (Locate on Disk, Open With,
//! Open in External Editor) are in `automation::NEVER_AUTOMATED`; Show Folder and Show in File
//! Manager ask [`crate::control::refuse`] with the engine command they stand for, so a control
//! request that clicks them is refused too.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use dalica_core::PhotoId;
use dalica_engine::cmd::os::{OS_WAIT, SLOW, missing};
use dalica_engine::options::find;
use dalica_engine::os::Service;
use dalica_engine::settings::MAX_PATH_BYTES;
use dalica_engine::{CmdError, Session};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::default_viewer;
use crate::menu_catalog::Mark;
use crate::menus::{ItemNode, Node};
use crate::state::{Handler, MAX_LOCATION, OpenWithMenu, View};
use crate::tl;

/// Why the default-viewer question's Use Dalica is greyed until M15.24 makes it act.
pub const LATER: &str = "Arrives with OS integration.";
/// Why a command on a photo cannot run: none is focused.
pub const NO_PHOTO: &str = "Select a photo first.";
/// Why the viewer's Show in File Manager cannot run: the viewer is not open.
pub const NO_VIEWER: &str = "The photo viewer is not open.";
/// Why the viewer's Open With ▸ is greyed for a photo the library does not hold.
pub const NOT_IN_LIBRARY: &str = "Open With lists applications for photos in your library.";
/// Why the External editor's Browse… cannot act: Options is closed.
pub const OPTIONS_CLOSED: &str = "Options is not open.";
/// Why automation cannot choose the external editor without the external permission.
pub const EDITOR_EXTERNAL: &str =
    "Changing the external editor chooses a program Dalica starts; automation changes it only when started with --automation-allow-external.";
/// The engine's commands of the OS integration (M15.17), all registered: a menu row of one is
/// greyed only for its `enabled`'s reason.
pub const AWAITED: &[&str] = &[];
/// The engine's command that lists a file's applications (M15.17).
pub const LIST_HANDLERS: &str = "photo.openWithList";
/// The engine's command an application row of Open With ▸ runs.
pub const OPEN_WITH: &str = "photo.openWith";
/// The submenu whose rows [`open_with_rows`] gives, by its label.
pub const OPEN_WITH_MENU: &str = "Open With";
/// The Options row of the external editor, and the command Browse…'s answer runs.
pub const EDITOR_KEY: &str = "files.externalEditor";
pub const EDITOR_COMMAND: &str = "app.preferences.editor";
/// The title of the program picker Browse… opens.
pub const EDITOR_TITLE: &str = "Choose the External Editor";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "openWith.list",
        label: "Open With",
        menu: "",
        keys: &[],
        params: "{id?: PhotoId}: open the Open With ▸ submenu for the photo (the focused one without `id`): its applications are asked of the system (photo.openWithList); replies {photo, handlers}",
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
        params: "{path: string}: show an exported folder (an absolute path) selected in the system's file manager; never run over automation. Replies {path}",
        run: show_folder,
    },
    UiCommand {
        id: "viewer.showInFileManager",
        label: "Show in File Manager",
        menu: "",
        keys: &[],
        params: "{}: show the photo the viewer shows, selected, in the system's file manager; never run over automation. Replies {path}",
        run: show_in_file_manager,
    },
    UiCommand {
        id: "app.preferences.editor",
        label: "Choose the External Editor",
        menu: "",
        keys: &[],
        params: "{path: string}: the program Options ▸ General ▸ External editor's Browse… chose, into the open dialog's draft (files.externalEditor); over automation only with --automation-allow-external",
        run: choose_editor,
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

/// Which of the OS integration's commands can run now, with the reason when one cannot.
pub const CHECKS: &[(&str, Check)] = &[
    ("openWith.list", list_ready),
    ("toast.showFolder", reveal_ready),
    ("viewer.showInFileManager", viewer_ready),
    ("app.preferences.editor", editor_ready),
];

/// Whether the system can show files in its file manager here; its reason when it cannot.
fn reveal_ready(app: &App, _: &Value) -> Result<(), String> {
    let session = app.session().ok_or_else(|| "The library is still opening.".to_string())?;
    session.os().available(Service::Reveal).map_err(|e| e.to_string())
}

fn viewer_ready(app: &App, params: &Value) -> Result<(), String> {
    if app.state().view != View::Viewer {
        return Err(NO_VIEWER.into());
    }
    reveal_ready(app, params)
}

fn list_ready(app: &App, _: &Value) -> Result<(), String> {
    let photo = focused(app).ok_or_else(|| NO_PHOTO.to_string())?;
    list_enabled(app, photo)
}

fn editor_ready(app: &App, _: &Value) -> Result<(), String> {
    if app.state().options.is_none() {
        return Err(OPTIONS_CLOSED.into());
    }
    Ok(())
}

/// Whether the engine can list the applications of `photo`: its `photo.openWithList` and the
/// system's Open With service.
fn list_enabled(app: &App, photo: PhotoId) -> Result<(), String> {
    let session = app.session().ok_or_else(|| "The library is still opening.".to_string())?;
    let spec = session.spec(LIST_HANDLERS).ok_or_else(|| "Not available yet.".to_string())?;
    (spec.enabled)(session, &json!({ "id": photo })).map_err(|d| d.0)
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

/// Why a menu row running the engine's command `id` is greyed while the engine lacks it: none
/// since the engine has every command of the OS integration ([`AWAITED`] is empty).
pub fn awaited(id: &str) -> Option<&'static str> {
    AWAITED.contains(&id).then_some(LATER)
}

// --- Open With ▸ --------------------------------------------------------------------------------

/// A greyed row of Open With ▸ that stands for its applications: "Loading…" while they are asked,
/// "No other applications", or why they cannot be asked.
fn placeholder(part: &str, label: String, reason: Option<String>) -> Node {
    Node::Item(ItemNode {
        id: "openWith.list",
        label,
        catalog_label: "",
        params: Value::Null,
        widget: format!("menuitem:openWith.list/{part}"),
        shortcut: None,
        live: true,
        enabled: false,
        reason,
        mark: Mark::None,
        checked: false,
        dialog: None,
    })
}

/// The rows of Open With ▸ for `photo` (none: a greyed row saying `none`, the viewer's photo the
/// library does not hold, or no photo focused): one per application the system offered, in its order (the default first), each running
/// `photo.openWith {id, handler}` (`menuitem:photo.openWith/<n>`); "Loading…" until they are
/// asked ([`sync_open_with`]); "No other applications" when there are none; a greyed row with the
/// reason when they cannot be asked (no photo, an operating system without Open With).
pub fn open_with_rows(app: &App, photo: Option<PhotoId>, none: &str) -> Vec<Node> {
    let Some(photo) = photo else { return vec![placeholder("none", tl!("No other applications"), Some(none.into()))] };
    if let Err(why) = list_enabled(app, photo) {
        return vec![placeholder("none", tl!("No other applications"), Some(why))];
    }
    let handlers = app.state().open_with.as_ref().filter(|m| m.photo == photo).and_then(|m| m.handlers.as_ref());
    match handlers {
        None => vec![placeholder("loading", tl!("Loading…"), Some(tl!("Asking the system which applications open this file.")))],
        Some(h) if h.is_empty() => {
            vec![placeholder("none", tl!("No other applications"), Some(tl!("The system offers no other application for this file.")))]
        }
        Some(h) => {
            let session = app.session();
            let spec = session.and_then(|s| s.spec(OPEN_WITH));
            h.iter()
                .enumerate()
                .map(|(n, handler)| {
                    let params = json!({ "id": photo, "handler": handler.id });
                    let ready = match (session, spec) {
                        (Some(s), Some(spec)) => (spec.enabled)(s, &params).map_err(|d| d.0),
                        _ => Err("Not available yet.".to_string()),
                    };
                    Node::Item(ItemNode {
                        id: OPEN_WITH,
                        label: handler.name.clone(),
                        catalog_label: "",
                        params,
                        widget: format!("menuitem:{OPEN_WITH}/{n}"),
                        shortcut: None,
                        live: spec.is_some(),
                        enabled: ready.is_ok(),
                        reason: ready.err(),
                        mark: Mark::None,
                        checked: false,
                        dialog: None,
                    })
                })
                .collect()
        }
    }
}

/// After a frame drew a menu with Open With ▸ for `photo`: the submenu `open` there asks the
/// system for the photo's applications once (`openWith.list`, so the next frame lists them), and
/// closed forgets them (`openWith.close`). Asks for a repaint when it changed something.
pub fn sync_open_with(app: &mut App, ctx: &egui::Context, photo: Option<PhotoId>, open: bool) {
    let asked = app.state().open_with.as_ref().map(|m| m.photo);
    match (open, photo) {
        (true, Some(p)) if asked != Some(p) && list_enabled(app, p).is_ok() => {
            let _ = app.run("openWith.list", &json!({ "id": p }));
            ctx.request_repaint();
        }
        (false, _) if asked.is_some() => {
            let _ = app.run("openWith.close", &json!({}));
            ctx.request_repaint();
        }
        _ => {}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    #[serde(default)]
    id: Option<u64>,
}

/// The handlers of a `photo.openWithList` reply that the view state can hold (an id and a name
/// within their lengths, no control characters), in order, at most `MAX_HANDLERS`.
fn handlers_of(reply: &Value) -> Vec<Handler> {
    let listed = reply.get("handlers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    listed
        .iter()
        .filter_map(|h| Some(Handler { id: h.get("id")?.as_str()?.to_string(), name: h.get("name")?.as_str()?.to_string() }))
        .filter(|h| OpenWithMenu { photo: PhotoId::new(1), handlers: Some(vec![h.clone()]) }.check().is_ok())
        .take(crate::state::MAX_HANDLERS)
        .collect()
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
    list_enabled(app, photo).map_err(CmdError::Disabled)?;
    // "Loading…" while the system is asked (the engine asks it off its owner thread and waits at
    // most `OS_WAIT`); a failure leaves the submenu with none, its reason on the status line.
    app.state_mut().open_with = Some(OpenWithMenu { photo, handlers: None });
    let reply = app.session_mut().map(|s| s.execute(LIST_HANDLERS, &json!({ "id": photo })));
    let (handlers, result) = match reply {
        Some(Ok(v)) => (handlers_of(&v), Ok(())),
        Some(Err(e)) => (Vec::new(), Err(e)),
        None => (Vec::new(), Err(CmdError::Disabled("The library is still opening.".into()))),
    };
    let shown: Vec<Value> = handlers.iter().map(|h| json!({ "id": h.id, "name": h.name })).collect();
    app.state_mut().open_with = Some(OpenWithMenu { photo, handlers: Some(handlers) });
    result.map(|()| json!({ "photo": photo, "handlers": shown }))
}

fn close(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().open_with = None;
    Ok(json!({}))
}

// --- Show Folder, Show in File Manager ----------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowFolder {
    path: PathBuf,
}

fn show_folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: ShowFolder = parse(params)?;
    if !p.path.is_absolute() || p.path.as_os_str().len() > MAX_LOCATION {
        return Err(CmdError::BadParams(format!("`path` is an absolute folder path of at most {MAX_LOCATION} bytes")));
    }
    crate::control::refuse("folder.locate", params, &[])?;
    reveal_ready(app, params).map_err(CmdError::Disabled)?;
    reveal(app, &p.path)
}

fn show_in_file_manager(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    viewer_ready(app, params).map_err(CmdError::Disabled)?;
    crate::control::refuse("photo.locate", params, &[])?;
    let (listing, i) = crate::panels::viewer::listing::current(app)?;
    let photo = listing.photos.get(i).ok_or_else(|| CmdError::Disabled(NO_VIEWER.into()))?;
    match photo.id {
        Some(id) => execute(app, "photo.locate", &json!({ "id": id })),
        None => reveal(app, &photo.path.clone()),
    }
}

/// Runs the engine's command `id` straight on the session (the reply is the engine's).
fn execute(app: &mut App, id: &str, params: &Value) -> Result<Value, CmdError> {
    match app.session_mut() {
        Some(s) => s.execute(id, params),
        None => Err(CmdError::Disabled("The library is still opening.".into())),
    }
}

/// Shows `path` selected in the system's file manager: a folder of the library through
/// `folder.locate` (its offline check and all); anything else, a file or folder the library does
/// not hold, through the session's `Os` on a helper thread (the UI thread makes no file or system
/// call), waited for at most `OS_WAIT`.
fn reveal(app: &mut App, path: &Path) -> Result<Value, CmdError> {
    let session = app.session().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    if let Some(folder) = session.library().folder_by_path(path) {
        return execute(app, "folder.locate", &json!({ "id": folder }));
    }
    reveal_outside(session, path)?;
    Ok(json!({ "path": path }))
}

fn reveal_outside(session: &Session, path: &Path) -> Result<(), CmdError> {
    let os = session.os();
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("dalica-reveal".into())
        .spawn(move || {
            let result = match dalica_fs::stat(&path) {
                Ok(Some(_)) | Err(dalica_fs::Error::Link(_)) => os.reveal(&path).map_err(|e| CmdError::Disabled(e.to_string())),
                Ok(None) => Err(CmdError::NotFound(missing(&path))),
                Err(e) => Err(CmdError::Disabled(format!("{}: {e}", path.display()))),
            };
            let _ = tx.send(result);
        })
        .map_err(|e| CmdError::Internal(format!("no thread for the system call: {e}")))?;
    match rx.recv_timeout(OS_WAIT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CmdError::Disabled(SLOW.into())),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(CmdError::Internal("the system call ended without an answer".into())),
    }
}

// --- the external editor ------------------------------------------------------------------------

/// The program picker Browse… opens for the External editor: any file (a program); its answer
/// runs [`EDITOR_COMMAND`].
pub fn editor_request() -> crate::picker::OpenRequest {
    crate::picker::OpenRequest { title: EDITOR_TITLE, filters: &[], command: EDITOR_COMMAND, params: json!({}) }
}

/// Whether the app may let automation choose a program it starts: always for the person's own
/// clicks; under a control request only with `--automation-allow-external`.
pub fn editor_change_allowed() -> bool {
    crate::control::automated().is_none_or(|c| c.allow_external)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Editor {
    path: String,
}

/// `app.preferences.editor {path}`: the program into the draft for `files.externalEditor`.
fn choose_editor(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Editor = parse_object(params)?;
    if p.path.len() > MAX_PATH_BYTES || p.path.chars().any(char::is_control) {
        return Err(CmdError::BadParams(format!("`path` is a program's path of at most {MAX_PATH_BYTES} bytes, without control characters")));
    }
    editor_ready(app, params).map_err(CmdError::Disabled)?;
    if !editor_change_allowed() {
        return Err(CmdError::Disabled(EDITOR_EXTERNAL.into()));
    }
    let spec = find(EDITOR_KEY).ok_or_else(|| CmdError::Internal(format!("{EDITOR_KEY} is not in the options catalogue")))?;
    let value = spec.check(&json!(p.path)).map_err(CmdError::BadParams)?;
    let row = crate::dialogs::options::general::rows(app)
        .into_iter()
        .find(|r| r.key == EDITOR_KEY)
        .ok_or_else(|| CmdError::Internal(format!("{EDITOR_KEY} is not on the General tab")))?;
    crate::dialogs::options::general::set(app, &row, value);
    Ok(json!({ "key": EDITOR_KEY, "path": p.path }))
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
    fn the_os_commands_say_what_they_act_on() {
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
        assert_eq!(awaited("photo.locate"), None, "the engine has them all");

        // The recording `Os` offers no applications: "No other applications".
        assert_eq!(app.run("openWith.list", &json!({})).unwrap(), json!({ "photo": ids[0], "handlers": [] }));
        assert_eq!(app.state().open_with, Some(OpenWithMenu { photo: ids[0], handlers: Some(vec![]) }));
        let rows = open_with_rows(&app, Some(ids[0]), NO_PHOTO);
        assert!(matches!(&rows[..], [Node::Item(i)] if i.widget == "menuitem:openWith.list/none" && !i.enabled), "{rows:?}");
        assert!(matches!(app.run("openWith.list", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("toast.showFolder", &json!({ "path": "relative" })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("viewer.showInFileManager", &json!({})), Err(CmdError::Disabled(m)) if m == NO_VIEWER));
        app.run("openWith.close", &json!({})).unwrap();
        assert_eq!(app.state().open_with, None);
        assert!(matches!(&open_with_rows(&app, Some(ids[0]), NO_PHOTO)[..], [Node::Item(i)] if i.widget == "menuitem:openWith.list/loading"));
        assert!(matches!(&open_with_rows(&app, None, NO_PHOTO)[..], [Node::Item(i)] if i.reason.as_deref() == Some(NO_PHOTO)));

        assert!(matches!(app.run("app.preferences.editor", &json!({ "path": "/usr/bin/gimp" })), Err(CmdError::Disabled(m)) if m == OPTIONS_CLOSED));
        assert!(matches!(app.run("app.preferences.editor", &json!({ "path": 1 })), Err(CmdError::BadParams(_))));

        assert_eq!(app.run("defaultViewer.ask", &json!({})).unwrap(), json!({ "dialog": "defaultViewer" }));
        assert_eq!(app.state().default_viewer, Some(crate::state::DefaultViewerDraft::default()), "nothing chosen");
    }

    #[test]
    fn handlers_the_view_state_cannot_hold_are_left_out() {
        let long = "x".repeat(crate::state::MAX_HANDLER_ID + 1);
        let reply = json!({ "handlers": [
            { "id": "gimp.desktop", "name": "GNU Image Manipulation Program" },
            { "id": long, "name": "Too long" },
            { "id": "bad", "name": "Bell\u{7}" },
            { "id": "no-name" },
        ] });
        assert_eq!(handlers_of(&reply), vec![Handler { id: "gimp.desktop".into(), name: "GNU Image Manipulation Program".into() }]);
        assert!(handlers_of(&json!({})).is_empty());
    }
}
