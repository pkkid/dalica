//! The photo viewer's commands (`viewing.md` §3.2), in the Photo viewer context: ← and J the
//! previous photo of the folder, → and K the next (stopping at the ends), + and PgUp zoom in, − and
//! PgDn zoom out, 1 toggles 100 %, Ctrl+R and Ctrl+Shift+R rotate, 8 and Ctrl+8 star, Delete moves
//! the photo to the trash (asking first), Space switches the background, E opens the photo in
//! Dalica, Esc closes; and `viewer.settings` (Tools ▸ Configure Photo Viewer…, the ⋯ menu's
//! Settings…) opens its Settings dialog.
//!
//! The viewer is the view state's `viewer`: the photo shown is `viewer.index`, its place in the
//! engine's listing of the folder (`crate::panels::viewer::listing`). Stepping shows another photo
//! fitted. The changes act on the photo shown through the engine's viewer commands, by its path:
//! `viewer.star` and `viewer.rotate` (the first change to a photo outside the library adds its
//! folder in the same undo step). Delete asks the library's own question (`photo.delete`, "Move the
//! selected picture to the trash?" [Move to Trash] [Cancel], unless "Delete from disk without
//! asking" is on); a photo of a folder outside the library has its folder added first
//! (`viewer.adopt`, an undo step of its own), as `viewer.delete` does. Open in Dalica runs
//! `Session::open_in_dalica` on the photo shown, whichever it is, and shows it in the edit view (the
//! window then becomes the main window). Close goes through `Session::close_viewer` (the app then
//! quits). While another Dalica holds the library the changes and Open in Dalica are disabled with
//! "Dalica is already open".
//!
//! The star, the rotations and Delete are UI commands of their own (`viewer.toggleStar`,
//! `viewer.rotateRight`, `viewer.rotateLeft`, `viewer.trash`) so their viewer keys join the keymap
//! without replacing the library's keys of `photo.star.toggle`, `photo.rotate.*` and
//! `photo.delete`.

use dalica_engine::{CmdError, Context};
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params};
use crate::app::App;
use crate::commands::zoom as maths;
use crate::dialogs::{self, viewer_settings};
use crate::panels::viewer::{listing, photo};
use crate::state::{Backdrop, View, ViewerState, Zoom};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "viewer.next",
        label: "Next Photo",
        menu: "",
        keys: &[(Context::PhotoViewer, "Right"), (Context::PhotoViewer, "K")],
        params: "{}: show the next photo of the folder",
        run: next,
    },
    UiCommand {
        id: "viewer.previous",
        label: "Previous Photo",
        menu: "",
        keys: &[(Context::PhotoViewer, "Left"), (Context::PhotoViewer, "J")],
        params: "{}: show the previous photo of the folder",
        run: previous,
    },
    UiCommand {
        id: "viewer.zoomIn",
        label: "Zoom In",
        menu: "",
        keys: &[(Context::PhotoViewer, "Plus"), (Context::PhotoViewer, "PageUp")],
        params: "{}: zoom in",
        run: zoom_in,
    },
    UiCommand {
        id: "viewer.zoomOut",
        label: "Zoom Out",
        menu: "",
        keys: &[(Context::PhotoViewer, "-"), (Context::PhotoViewer, "PageDown")],
        params: "{}: zoom out, down to fitted",
        run: zoom_out,
    },
    UiCommand {
        id: "viewer.actualSize",
        label: "Actual Size",
        menu: "",
        keys: &[(Context::PhotoViewer, "1")],
        params: "{}: show the photo at 100 %, or fitted again",
        run: actual_size,
    },
    UiCommand {
        id: "viewer.toggleStar",
        label: "Star / Unstar",
        menu: "",
        keys: &[(Context::PhotoViewer, "Ctrl+8"), (Context::PhotoViewer, "8")],
        params: "{}: star or unstar the photo shown (viewer.star)",
        run: toggle_star,
    },
    UiCommand {
        id: "viewer.rotateRight",
        label: "Rotate Clockwise",
        menu: "",
        keys: &[(Context::PhotoViewer, "Ctrl+R")],
        params: "{}: turn the photo shown a quarter turn clockwise (viewer.rotate)",
        run: rotate_right,
    },
    UiCommand {
        id: "viewer.rotateLeft",
        label: "Rotate Counterclockwise",
        menu: "",
        keys: &[(Context::PhotoViewer, "Ctrl+Shift+R")],
        params: "{}: turn the photo shown a quarter turn counterclockwise (viewer.rotate)",
        run: rotate_left,
    },
    UiCommand {
        id: "viewer.trash",
        label: "Delete from Disk…",
        menu: "",
        keys: &[(Context::PhotoViewer, "Delete")],
        params: r#"{}: move the photo shown to the trash, asking first (replies {dialog: "deletePhotos"} when it asks)"#,
        run: trash,
    },
    UiCommand {
        id: "viewer.background",
        label: "Translucent Background",
        menu: "",
        keys: &[(Context::PhotoViewer, "Space")],
        params: "{}: switch the background between translucent and opaque",
        run: background,
    },
    UiCommand {
        id: "viewer.openInDalica",
        label: "Open in Dalica",
        menu: "",
        keys: &[(Context::PhotoViewer, "E")],
        params: "{}: open the photo shown in Dalica's edit view (the viewer closes into the main window)",
        run: open_in_dalica,
    },
    UiCommand { id: "viewer.close", label: "Close", menu: "", keys: &[(Context::PhotoViewer, "Esc")], params: "{}: close the viewer", run: close },
    UiCommand {
        id: "viewer.settings",
        label: "Configure Photo Viewer…",
        menu: "Tools/Configure Photo Viewer…",
        keys: &[],
        params: r#"{}: open the photo viewer's Settings (replies {dialog: "viewerSettings"})"#,
        run: settings,
    },
];

/// Why the viewer's commands cannot run outside it.
pub const NOT_OPEN: &str = "The photo viewer is not open.";
/// Why stepping stops at the ends of the folder.
pub const AT_LAST: &str = "This is the last photo of the folder.";
pub const AT_FIRST: &str = "This is the first photo of the folder.";
/// The zoom step of + and −.
pub const ZOOM_STEP: f32 = 1.25;

pub const CHECKS: &[(&str, Check)] = &[
    ("viewer.next", open),
    ("viewer.previous", open),
    ("viewer.openInDalica", change),
    ("viewer.toggleStar", change),
    ("viewer.rotateRight", change),
    ("viewer.rotateLeft", change),
    ("viewer.trash", change),
    ("viewer.zoomIn", open),
    ("viewer.zoomOut", open),
    ("viewer.actualSize", open),
    ("viewer.background", open),
    ("viewer.close", open),
];

fn open(app: &App, _: &Value) -> Result<(), String> {
    viewer(app).map(|_| ()).ok_or_else(|| NOT_OPEN.into())
}

/// The changes and Open in Dalica: the viewer is open, and no other Dalica holds the library.
fn change(app: &App, params: &Value) -> Result<(), String> {
    open(app, params)?;
    match app.session().and_then(|s| s.viewer().read_only_reason()) {
        Some(why) => Err(why.to_string()),
        None => Ok(()),
    }
}

/// The viewer's state while it is shown.
pub fn viewer(app: &App) -> Option<&ViewerState> {
    app.state().viewer.as_ref().filter(|_| app.state().view == View::Viewer)
}

/// Runs `change` on the viewer shown; `disabled` when it is not.
fn with_viewer(app: &mut App, change: impl FnOnce(&mut ViewerState)) -> Result<Value, CmdError> {
    if viewer(app).is_none() {
        return Err(CmdError::Disabled(NOT_OPEN.into()));
    }
    let v = app.state_mut().viewer.as_mut().ok_or_else(|| CmdError::Disabled(NOT_OPEN.into()))?;
    change(v);
    serde_json::to_value(*v).map_err(|e| CmdError::Internal(e.to_string()))
}

/// Shows the photo at place `index` of the folder, fitted.
pub fn go_to(app: &mut App, index: usize) {
    if let Some(v) = app.state_mut().viewer.as_mut()
        && v.index != index
    {
        v.index = index;
        v.zoom = Zoom::Fit;
    }
}

/// The listing and the photo shown, for a command that needs them.
fn shown(app: &mut App) -> Result<(listing::Listing, usize), CmdError> {
    if viewer(app).is_none() {
        return Err(CmdError::Disabled(NOT_OPEN.into()));
    }
    listing::current(app)
}

/// Steps to the next (`forward`) or the previous photo of the folder, stopping at its ends.
fn step(app: &mut App, forward: bool) -> Result<Value, CmdError> {
    let (l, i) = shown(app)?;
    let to = if forward { i.checked_add(1).filter(|n| *n < l.photos.len()) } else { i.checked_sub(1) };
    let to = to.ok_or_else(|| CmdError::Disabled(if forward { AT_LAST } else { AT_FIRST }.into()))?;
    go_to(app, to);
    let name = l.photos.get(to).map(|p| p.name.clone()).unwrap_or_default();
    Ok(json!({ "index": to, "name": name }))
}

fn next(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    step(app, true)
}

fn previous(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    step(app, false)
}

/// The photo shown and its path, for a change: refused with the read-only reason.
fn changing(app: &mut App) -> Result<listing::Photo, CmdError> {
    change(app, &Value::Null).map_err(CmdError::Disabled)?;
    let (l, i) = shown(app)?;
    l.photos.get(i).cloned().ok_or_else(|| CmdError::Disabled(listing::EMPTY.into()))
}

fn toggle_star(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = changing(app)?;
    app.run("viewer.star", &json!({ "path": p.path }))
}

fn rotate(app: &mut App, params: &Value, direction: &str) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = changing(app)?;
    app.run("viewer.rotate", &json!({ "path": p.path, "direction": direction }))
}

fn rotate_right(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    rotate(app, params, "cw")
}

fn rotate_left(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    rotate(app, params, "ccw")
}

fn trash(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = changing(app)?;
    let id = match p.id {
        Some(id) => id,
        None => {
            let reply = app.run("viewer.adopt", &json!({ "path": p.path }))?;
            serde_json::from_value(reply["id"].clone()).map_err(|e| CmdError::Internal(format!("viewer.adopt replied no photo id: {e}")))?
        }
    };
    app.run("photo.delete", &json!({ "ids": [id] }))
}

fn open_in_dalica(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = changing(app)?;
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    let id = session.open_in_dalica(&p.path)?;
    app.state_mut().viewer = None;
    app.run("view.edit", &json!({ "id": id }))
}

fn settings(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    dialogs::open(app, viewer_settings::dialog())
}

/// The fitted scale of the photo shown (1 before the viewer has drawn it).
fn fit() -> f32 {
    photo::fitted().unwrap_or(1.0)
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let fit = fit();
    with_viewer(app, |v| v.zoom = maths::step(maths::effective(v.zoom, fit), ZOOM_STEP, fit))
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let fit = fit();
    with_viewer(app, |v| v.zoom = maths::step(maths::effective(v.zoom, fit), 1.0 / ZOOM_STEP, fit))
}

fn actual_size(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_viewer(app, |v| v.zoom = if v.zoom == Zoom::Scale(1.0) { Zoom::Fit } else { Zoom::Scale(1.0) })
}

fn background(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_viewer(app, |v| {
        v.backdrop = match v.backdrop {
            Backdrop::Translucent => Backdrop::Opaque,
            Backdrop::Opaque => Backdrop::Translucent,
        }
    })
}

fn close(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    open(app, params).map_err(CmdError::Disabled)?;
    if let Some(s) = app.session_mut() {
        s.close_viewer();
    }
    let state = app.state_mut();
    state.viewer = None;
    state.view = View::Library;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewer_zooms_switches_its_background_and_closes() {
        let mut app = App::new(None);
        assert!(matches!(app.run("viewer.background", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_OPEN));
        app.state_mut().viewer = Some(ViewerState::default());
        app.state_mut().view = View::Viewer;
        assert_eq!(app.run("viewer.background", &json!({})).unwrap()["backdrop"], "opaque");
        assert_eq!(app.run("viewer.actualSize", &json!({})).unwrap()["zoom"], 1.0);
        assert_eq!(app.run("viewer.actualSize", &json!({})).unwrap()["zoom"], "fit");
        // Before the viewer has drawn a photo its fitted scale counts as 1.
        assert_eq!(app.run("viewer.zoomIn", &json!({})).unwrap()["zoom"], 1.25);
        assert_eq!(app.run("viewer.zoomOut", &json!({})).unwrap()["zoom"], "fit");
        // No library: nothing to step through.
        assert!(matches!(app.run("viewer.next", &json!({})), Err(CmdError::Disabled(m)) if m == listing::READING));
        app.run("viewer.close", &json!({})).unwrap();
        assert_eq!((app.state().view, app.state().viewer), (View::Library, None));
    }

    /// `dalica --view`: a session with an open viewer starts on the viewer's view, and Close (Esc)
    /// closes the engine's viewer, which the app then quits on.
    #[test]
    fn a_session_with_an_open_viewer_starts_on_it_and_close_ends_it_in_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = dalica_engine::Session::open(&dir.path().join("library"), std::sync::Arc::new(dalica_core::SystemClock)).unwrap();
        s.open_viewer(&dir.path().join("photo.jpg")).unwrap();
        let mut app = App::new(Some(s));
        assert_eq!(app.state().view, View::Viewer);
        assert!(app.state().viewer.is_some());
        app.run("viewer.close", &json!({})).unwrap();
        assert_eq!(app.session().and_then(|s| s.viewer().closed()), Some(dalica_engine::viewer::Closed::Quit));
        // Without an open viewer the app starts on the library.
        let other = tempfile::tempdir().unwrap();
        let s = dalica_engine::Session::open(other.path(), std::sync::Arc::new(dalica_core::SystemClock)).unwrap();
        assert_eq!(App::new(Some(s)).state().view, View::Library);
    }
}
