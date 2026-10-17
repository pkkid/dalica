//! The slideshow's commands (`viewing.md` §1): starting a show (View ▸ Slideshow and Ctrl+4,
//! `view.slideshow`; a section's ▶ and Folder ▸ View Slideshow, `section.slideshow`, join with
//! M11.19: the section header treats any UI command as live, so registering it before it plays
//! would offer a ▶ that does nothing) and, while one
//! plays, the keys of §1.4 in the Slideshow context: Esc exits, Space pauses and resumes, → and PgDn
//! next, ← and PgUp previous (a command has at most two keys per context, so ↓ and ↑ are left to
//! the user to bind), Home and End the first and last photo, + and − zoom.
//!
//! M11.2's stubs: the show is the view state's `slideshow` (set with `ui.set` until M11.19 starts
//! one from what M11.11 lists), and the commands that move through it change only that state:
//! next and previous stop at the ends, Pause toggles playing and shows the bar, zooming pauses,
//! Exit goes back to where the show started. Starting a show and the bar's settings (Transition,
//! Captions, Display time) wait for M11.19 and M11.21 and say so.

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{SLIDE_ZOOM_MAX, ShowFrom, Slideshow, View};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "view.slideshow",
        label: "Slideshow",
        menu: "View/Slideshow",
        keys: &[(Context::Library, "Ctrl+4"), (Context::EditView, "Ctrl+4")],
        params: "{ids?: [PhotoId], start?: PhotoId}: play a slideshow: the photos named, else the tray's two or more, else the focused photo's section from it (from the edit view: its folder or album from the photo shown)",
        run: start,
    },
    UiCommand {
        id: "slideshow.exit",
        label: "Exit Slideshow",
        menu: "",
        keys: &[(Context::Slideshow, "Esc")],
        params: "{}: end the slideshow and go back to where it started",
        run: exit,
    },
    UiCommand {
        id: "slideshow.next",
        label: "Next Photo",
        menu: "",
        keys: &[(Context::Slideshow, "Right"), (Context::Slideshow, "PageDown")],
        params: "{}: show the next photo",
        run: next,
    },
    UiCommand {
        id: "slideshow.previous",
        label: "Previous Photo",
        menu: "",
        keys: &[(Context::Slideshow, "Left"), (Context::Slideshow, "PageUp")],
        params: "{}: show the previous photo",
        run: previous,
    },
    UiCommand {
        id: "slideshow.first",
        label: "First Photo",
        menu: "",
        keys: &[(Context::Slideshow, "Home")],
        params: "{}: show the first photo",
        run: first,
    },
    UiCommand {
        id: "slideshow.last",
        label: "Last Photo",
        menu: "",
        keys: &[(Context::Slideshow, "End")],
        params: "{}: show the last photo",
        run: last,
    },
    UiCommand {
        id: "slideshow.pause",
        label: "Pause / Play",
        menu: "",
        keys: &[(Context::Slideshow, "Space")],
        params: "{on?: bool}: pause (true) or play (false); toggles without `on`; shows the control bar",
        run: pause,
    },
    UiCommand {
        id: "slideshow.zoomIn",
        label: "Zoom In",
        menu: "",
        keys: &[(Context::Slideshow, "Plus")],
        params: "{}: zoom the photo shown in (pauses the show)",
        run: zoom_in,
    },
    UiCommand {
        id: "slideshow.zoomOut",
        label: "Zoom Out",
        menu: "",
        keys: &[(Context::Slideshow, "-")],
        params: "{}: zoom the photo shown out, back to fitted",
        run: zoom_out,
    },
    UiCommand {
        id: "slideshow.transition",
        label: "Transition",
        menu: "",
        keys: &[],
        params: r#"{transition: "cut" | "panAndZoom" | "dissolve" | "wipe" | "circle" | "push" | "rectangle"}: the transition between photos (kept in the settings)"#,
        run: transition,
    },
    UiCommand {
        id: "slideshow.captions",
        label: "Captions",
        menu: "",
        keys: &[],
        params: "{on?: bool}: show captions in the slideshow (kept in the settings); toggles without `on`",
        run: captions,
    },
    UiCommand {
        id: "slideshow.displayTime",
        label: "Display Time",
        menu: "",
        keys: &[],
        params: "{seconds: 1 … 60}: how long each photo is shown (kept in the settings)",
        run: display_time,
    },
];

/// Why a show cannot start yet, and why the bar's settings cannot change yet.
pub const NOT_YET: &str = "Arrives with the slideshow.";
pub const BAR_NOT_YET: &str = "Arrives with the slideshow's control bar.";
/// Why the show's own commands cannot run.
pub const NOT_PLAYING: &str = "No slideshow is playing.";

/// The zoom step of + and −.
pub const ZOOM_STEP: f32 = 1.25;

pub const CHECKS: &[(&str, Check)] = &[
    ("view.slideshow", |_, _| Err(NOT_YET.into())),
    ("slideshow.exit", playing),
    ("slideshow.next", playing),
    ("slideshow.previous", playing),
    ("slideshow.first", playing),
    ("slideshow.last", playing),
    ("slideshow.pause", playing),
    ("slideshow.zoomIn", playing),
    ("slideshow.zoomOut", playing),
    ("slideshow.transition", |_, _| Err(BAR_NOT_YET.into())),
    ("slideshow.captions", |_, _| Err(BAR_NOT_YET.into())),
    ("slideshow.displayTime", |_, _| Err(BAR_NOT_YET.into())),
];

fn playing(app: &App, _: &Value) -> Result<(), String> {
    show(app).map(|_| ()).ok_or_else(|| NOT_PLAYING.into())
}

/// The slideshow playing, if one is.
pub fn show(app: &App) -> Option<&Slideshow> {
    app.state().slideshow.as_ref().filter(|_| app.state().view == View::Slideshow)
}

/// Runs `change` on the slideshow playing; `disabled` when none is.
fn with_show(app: &mut App, change: impl FnOnce(&mut Slideshow)) -> Result<Value, CmdError> {
    if show(app).is_none() {
        return Err(CmdError::Disabled(NOT_PLAYING.into()));
    }
    let s = app.state_mut().slideshow.as_mut().ok_or_else(|| CmdError::Disabled(NOT_PLAYING.into()))?;
    change(s);
    Ok(json!({ "index": s.index, "photo": s.photo(), "playing": s.playing }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // read by M11.19
struct Start {
    #[serde(default)]
    ids: Option<Vec<dalica_core::PhotoId>>,
    #[serde(default)]
    start: Option<dalica_core::PhotoId>,
}

fn start(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    let _: Start = parse_object(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn exit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let Some(s) = show(app).cloned() else { return Err(CmdError::Disabled(NOT_PLAYING.into())) };
    let state = app.state_mut();
    state.slideshow = None;
    state.view = match s.from {
        ShowFrom::Edit if state.edit.photo.is_some() => View::Edit,
        ShowFrom::Viewer if state.viewer.is_some() => View::Viewer,
        _ => View::Library,
    };
    Ok(json!({ "photo": s.photo() }))
}

fn next(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| s.index = (s.index + 1).min(s.photos.len().saturating_sub(1)))
}

fn previous(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| s.index = s.index.saturating_sub(1))
}

fn first(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| s.index = 0)
}

fn last(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| s.index = s.photos.len().saturating_sub(1))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    on: Option<bool>,
}

fn pause(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    with_show(app, |s| {
        s.playing = !p.on.unwrap_or(s.playing);
        s.bar = true;
    })
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| {
        s.zoom = (s.zoom * ZOOM_STEP).min(SLIDE_ZOOM_MAX);
        s.playing = false;
    })
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    with_show(app, |s| s.zoom = (s.zoom / ZOOM_STEP).max(1.0))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Transition {
    Cut,
    PanAndZoom,
    Dissolve,
    Wipe,
    Circle,
    Push,
    Rectangle,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetTransition {
    #[allow(dead_code)] // read by M11.21
    transition: Transition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayTime {
    seconds: u8,
}

fn transition(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    let _: SetTransition = parse(params)?;
    Err(CmdError::Disabled(BAR_NOT_YET.into()))
}

fn captions(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    let _: On = parse_object(params)?;
    Err(CmdError::Disabled(BAR_NOT_YET.into()))
}

fn display_time(_: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: DisplayTime = parse(params)?;
    if !(1..=60).contains(&p.seconds) {
        return Err(CmdError::BadParams(format!("`seconds` is 1 … 60, not {}", p.seconds)));
    }
    Err(CmdError::Disabled(BAR_NOT_YET.into()))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;

    fn playing_app(n: u64, from: ShowFrom) -> App {
        let mut app = App::new(None);
        let photos = (1..=n).map(PhotoId::new).collect();
        app.state_mut().slideshow = Some(Slideshow { photos, from, ..Slideshow::default() });
        app.state_mut().view = View::Slideshow;
        app
    }

    #[test]
    fn the_show_moves_within_its_photos_pauses_zooms_and_exits_where_it_started() {
        let mut app = App::new(None);
        assert!(matches!(app.run("slideshow.next", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_PLAYING));
        assert!(matches!(app.run("view.slideshow", &json!({})), Err(CmdError::Disabled(_))), "a stub until M11.19");
        let mut app = playing_app(3, ShowFrom::Library);
        let index = |app: &App| app.state().slideshow.as_ref().map(|s| s.index);
        app.run("slideshow.previous", &json!({})).unwrap();
        assert_eq!(index(&app), Some(0), "no photo before the first");
        app.run("slideshow.last", &json!({})).unwrap();
        app.run("slideshow.next", &json!({})).unwrap();
        assert_eq!(index(&app), Some(2), "none after the last");
        app.run("slideshow.first", &json!({})).unwrap();
        assert_eq!(app.run("slideshow.next", &json!({})).unwrap()["photo"], 2);
        app.run("slideshow.pause", &json!({})).unwrap();
        assert!(!app.state().slideshow.as_ref().unwrap().playing);
        app.run("slideshow.pause", &json!({ "on": false })).unwrap();
        app.run("slideshow.zoomIn", &json!({})).unwrap();
        let s = app.state().slideshow.clone().unwrap();
        assert!((s.zoom - ZOOM_STEP).abs() < 1e-6 && !s.playing, "zooming pauses");
        for _ in 0..3 {
            app.run("slideshow.zoomOut", &json!({})).unwrap();
        }
        assert_eq!(app.state().slideshow.as_ref().unwrap().zoom, 1.0);
        assert_eq!(app.run("slideshow.exit", &json!({})).unwrap(), json!({ "photo": 2 }));
        assert_eq!((app.state().view, app.state().slideshow.is_none()), (View::Library, true));
        // Started from the edit view with no photo there any more: back to the library.
        let mut app = playing_app(1, ShowFrom::Edit);
        app.run("slideshow.exit", &json!({})).unwrap();
        assert_eq!(app.state().view, View::Library);
        assert!(matches!(app.run("slideshow.displayTime", &json!({ "seconds": 61 })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("slideshow.transition", &json!({ "transition": "wipe" })), Err(CmdError::Disabled(_))));
    }
}
