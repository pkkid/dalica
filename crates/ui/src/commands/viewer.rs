//! The photo viewer's commands (`viewing.md` §3.2), in the Photo viewer context: ← and J the
//! previous photo of the folder, → and K the next, + and PgUp zoom in, − and PgDn zoom out, 1 toggles
//! 100 %, Space the background, E opens the photo in Dalica, Esc closes; and `viewer.settings`
//! (Tools ▸ Configure Photo Viewer…, the ⋯ menu's Settings…) opens its Settings dialog.
//!
//! M11.2's stubs: the viewer is the view state's `viewer`, and the commands that only change it
//! work on it (zoom, 100 %, the background, Close). Stepping through the folder, Open in Dalica and
//! the Settings dialog wait for the viewer itself (M11.12, M11.17) and say so.

use dalica_engine::{CmdError, Context};
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params};
use crate::app::App;
use crate::state::{Backdrop, View, ViewerState, ZOOM_MAX, ZOOM_MIN, Zoom};

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
        params: "{}: open the photo in Dalica's edit view",
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

/// Why a command waits for the viewer itself.
pub const NOT_YET: &str = "Arrives with the photo viewer.";
/// Why the viewer's commands cannot run outside it.
pub const NOT_OPEN: &str = "The photo viewer is not open.";
/// The zoom step of + and −.
pub const ZOOM_STEP: f32 = 1.25;

pub const CHECKS: &[(&str, Check)] = &[
    ("viewer.next", |_, _| Err(NOT_YET.into())),
    ("viewer.previous", |_, _| Err(NOT_YET.into())),
    ("viewer.openInDalica", |_, _| Err(NOT_YET.into())),
    ("viewer.settings", |_, _| Err(NOT_YET.into())),
    ("viewer.zoomIn", open),
    ("viewer.zoomOut", open),
    ("viewer.actualSize", open),
    ("viewer.background", open),
    ("viewer.close", open),
];

fn open(app: &App, _: &Value) -> Result<(), String> {
    viewer(app).map(|_| ()).ok_or_else(|| NOT_OPEN.into())
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

fn next(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn previous(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn open_in_dalica(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn settings(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

/// A scale `factor` times `zoom` (fitted counts as 1, the stub's stand-in until the viewer knows
/// the fitted scale), within the zoom's range; a scale at or below 1 after zooming out is fitted.
fn scaled(zoom: Zoom, factor: f32) -> Zoom {
    let from = match zoom {
        Zoom::Fit => 1.0,
        Zoom::Scale(v) => v,
    };
    let to = from * factor;
    if factor < 1.0 && to <= 1.0 { Zoom::Fit } else { Zoom::Scale(to.clamp(ZOOM_MIN, ZOOM_MAX)) }
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_viewer(app, |v| v.zoom = scaled(v.zoom, ZOOM_STEP))
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_viewer(app, |v| v.zoom = scaled(v.zoom, 1.0 / ZOOM_STEP))
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
        assert_eq!(app.run("viewer.zoomIn", &json!({})).unwrap()["zoom"], 1.25);
        assert_eq!(app.run("viewer.zoomOut", &json!({})).unwrap()["zoom"], "fit");
        assert!(matches!(app.run("viewer.next", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_YET));
        app.run("viewer.close", &json!({})).unwrap();
        assert_eq!((app.state().view, app.state().viewer), (View::Library, None));
    }
}
