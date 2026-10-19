//! The Collage room's commands (`creating.md` §3), stubs until M14.18 fills them: the room's view
//! state acts now, what changes the collage checks its parameters and then refuses with
//! [`LATER`] (the collage itself is the engine's, M14.16–M14.17).
//!
//! The ids are the UI's own, never an engine command's: the engine owns `create.collage`
//! (Create ▸ Picture Collage…, the bottom bar's and the section headers' Collage) and `collage.*`
//! (`collage.set`, `collage.clips.add`, `collage.create`, `collage.saveDraft`, `collage.reset`,
//! `collage.close`, …), and a UI command of the same id would hide the engine's from
//! `engine.execute`. When the engine's `create.collage` (or `collage.edit`) succeeds the room opens
//! ([`ran`], joined to `App::run` with M14.18); until then [`OPEN`] opens the stub room.
//!
//! - **`collage.open {ids?, section?}`**: opens the Collage room (its tab last on the strip) or
//!   brings it to the front, the Settings tab shown; the photos are checked (`not_found`) and wait
//!   for the engine's collage to become its clips.
//! - **`collage.tab {tab}`**: the left panel's Settings or Clips tab.
//! - **`collage.style {style}`**, **`collage.settings {…}`**: the style dropdown and the options
//!   under it (Picture borders, Grid spacing, Background, Page format and orientation, Draw
//!   shadows, Show captions), through the engine's `collage.set`.
//! - **`collage.selectClips {ids}`** (the clips selected in the Clips tab), **`collage.addClips`**
//!   (+: the selected clips onto the page) and **`collage.removeClips`** (−: out of the Clips).
//! - **`collage.getMore`** shows the Library tab with the tray's **Back to Collage**
//!   (`collage.backToCollage`, `button:tray.backToCollage`), which brings the room back (and with
//!   M14.18 adds the selection, or the held tray, to the Clips).
//! - **`collage.createCollage`**, **`collage.saveAsDraft`**, **`collage.resetSettings`**: Create
//!   Collage, Save Draft and Reset, through the engine's `collage.create`, `collage.saveDraft` and
//!   `collage.reset`.
//! - **`collage.closeRoom {discard?}`**: Close, as the tab's × (`room.close {room: "collage"}`):
//!   with unsaved changes ([`unsaved`]) it asks [Save Draft] [Discard] [Cancel]
//!   (`dialog:collageClose`) unless `discard` is true.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, Dialog, DialogKind};
use crate::state::{CollageClose, CollageRoom, CollageTab, MAX_CLIPS, Room, Section, is_colour, is_ratio};

/// What the room's stub commands say until M14.18 makes them act.
pub const LATER: &str = "Arrives with the Collage room.";
/// Why a room command cannot run: the room is not open.
pub const NOT_OPEN: &str = "Open a collage first.";
/// The stub room's opener.
pub const OPEN: &str = "collage.open";
/// The styles, as `collage.style` takes them (`creating.md` §3).
pub const STYLES: [&str; 6] = ["pile", "mosaic", "frameMosaic", "grid", "contactSheet", "multipleExposure"];
/// Current Display: the page format of the screen's size.
pub const DISPLAY: &str = "display";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: OPEN,
        label: "Open the Collage Room",
        menu: "",
        keys: &[],
        params: "{ids?: [PhotoId], section?: Section}: open the Collage room (or bring it to the front) on the Settings tab; replies {room}",
        run: open,
    },
    UiCommand {
        id: "collage.tab",
        label: "Show Collage Tab",
        menu: "",
        keys: &[],
        params: r#"{tab: "settings" | "clips"}: show the Collage room's Settings or Clips tab; replies {tab}"#,
        run: tab,
    },
    UiCommand {
        id: "collage.style",
        label: "Collage Style",
        menu: "",
        keys: &[],
        params: r#"{style: "pile" | "mosaic" | "frameMosaic" | "grid" | "contactSheet" | "multipleExposure"}: the collage's style"#,
        run: style,
    },
    UiCommand {
        id: "collage.settings",
        label: "Collage Settings",
        menu: "",
        keys: &[],
        params: r##"{borders?: "none" | "white" | "instant", spacing?: 0 … 1, background?: "#rrggbb", usePicture?: bool, format?: "W:H" | "display", orientation?: "landscape" | "portrait", shadows?: bool, captions?: bool}: the collage's settings (at least one)"##,
        run: settings,
    },
    UiCommand {
        id: "collage.selectClips",
        label: "Select Clips",
        menu: "",
        keys: &[],
        params: "{ids: [PhotoId]}: select these clips in the Clips tab ([] selects none); replies {clips}",
        run: select_clips,
    },
    UiCommand {
        id: "collage.addClips",
        label: "Add to Collage",
        menu: "",
        keys: &[],
        params: "{}: put the selected clips on the page (+)",
        run: add_clips,
    },
    UiCommand {
        id: "collage.removeClips",
        label: "Remove from Clips",
        menu: "",
        keys: &[],
        params: "{}: take the selected clips out of the Clips (−)",
        run: remove_clips,
    },
    UiCommand {
        id: "collage.getMore",
        label: "Get More",
        menu: "",
        keys: &[],
        params: "{}: show the Library tab to choose more pictures; the tray offers Back to Collage; replies {room}",
        run: get_more,
    },
    UiCommand {
        id: "collage.backToCollage",
        label: "Back to Collage",
        menu: "",
        keys: &[],
        params: "{}: bring the Collage room back after Get More; replies {room}",
        run: back_to_collage,
    },
    UiCommand {
        id: "collage.createCollage",
        label: "Create Collage",
        menu: "",
        keys: &[],
        params: "{}: render the collage into the Collages folder and open it in the edit view",
        run: create_collage,
    },
    UiCommand {
        id: "collage.saveAsDraft",
        label: "Save Draft",
        menu: "",
        keys: &[],
        params: "{}: save the collage as a draft beside the collages",
        run: save_as_draft,
    },
    UiCommand {
        id: "collage.resetSettings",
        label: "Reset",
        menu: "",
        keys: &[],
        params: "{}: the collage's settings back to their defaults",
        run: reset_settings,
    },
    UiCommand {
        id: "collage.closeRoom",
        label: "Close",
        menu: "",
        keys: &[],
        params: "{discard?: bool}: close the Collage room; with unsaved changes it asks Save Draft, Discard or Cancel first unless `discard` is true; replies {room} (the room now in front), or {dialog} when it asks",
        run: close_room,
    },
];

/// The room's commands grey their buttons and menu rows while the room is not open, and those
/// that wait for M14.18 with [`LATER`] while it is.
pub const CHECKS: &[(&str, Check)] = &[
    ("collage.style", waits),
    ("collage.settings", waits),
    ("collage.selectClips", room_open),
    ("collage.addClips", waits),
    ("collage.removeClips", waits),
    ("collage.getMore", room_open),
    ("collage.backToCollage", room_open),
    ("collage.createCollage", waits),
    ("collage.saveAsDraft", waits),
    ("collage.resetSettings", waits),
    ("collage.closeRoom", room_open),
];

/// [`NOT_OPEN`] while the room is not open, else [`LATER`].
fn waits(app: &App, params: &Value) -> Result<(), String> {
    room_open(app, params)?;
    Err(LATER.into())
}

/// Whether the Collage room is open; [`NOT_OPEN`] when it is not.
pub fn room_open(app: &App, _: &Value) -> Result<(), String> {
    if app.state().rooms.contains(&Room::Collage) { Ok(()) } else { Err(NOT_OPEN.into()) }
}

fn need_room(app: &App) -> Result<(), CmdError> {
    room_open(app, &Value::Null).map_err(CmdError::Disabled)
}

/// Whether the open collage has changes not saved as a collage or a draft. The engine's collage
/// says so with M14.16 (`collage.inspect`); until then there is nothing to save.
pub fn unsaved(_app: &App) -> bool {
    false
}

/// After command `id` ran with `result`: the engine's `create.collage` and `collage.edit` open
/// the room on success (M14.18 joins this to `App::run`, as `save_as::ran` is).
pub fn ran(app: &mut App, id: &str, result: &Result<Value, CmdError>) {
    if matches!(id, "create.collage" | "collage.edit") && result.is_ok() {
        show_room(app);
    }
}

/// Opens the Collage room if it is not open (its tab goes last) and brings it to the front on its
/// Settings tab. Replies `{room}`.
pub fn show_room(app: &mut App) -> Value {
    let reply = super::rooms::open(app, Room::Collage);
    let c = &mut app.state_mut().collage;
    c.get_more = false;
    c.tab = CollageTab::Settings;
    reply
}

/// Closes the Collage room, unless it has unsaved changes and `discard` is false: then the close
/// question asks first (`dialog:collageClose`, what closes noted as `closing`). Closing drops the
/// room's view state (its tab kept) and puts the Library in front if the room was. Replies `{room}`
/// (the room now in front), or `{dialog}` when it asks.
pub fn close(app: &mut App, discard: bool, closing: CollageClose) -> Result<Value, CmdError> {
    need_room(app)?;
    if unsaved(app) && !discard {
        let reply = dialogs::open(app, Dialog::new(DialogKind::CollageClose))?;
        app.state_mut().collage.close = Some(closing);
        return Ok(reply);
    }
    let state = app.state_mut();
    state.rooms.retain(|r| *r != Room::Collage);
    if state.room == Room::Collage {
        state.room = Room::Library;
    }
    state.collage = CollageRoom { tab: state.collage.tab, ..CollageRoom::default() };
    Ok(json!({ "room": state.room.name() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    section: Option<Section>,
}

/// Refuses more than [`MAX_CLIPS`] photos (`bad_params`) and photos the library does not have
/// (`not_found`).
fn known_photos(app: &App, ids: &[PhotoId]) -> Result<(), CmdError> {
    if ids.len() > MAX_CLIPS {
        return Err(CmdError::BadParams(format!("a collage holds at most {MAX_CLIPS} pictures")));
    }
    let library = app.session().map(dalica_engine::Session::library);
    match ids.iter().find(|id| !library.is_some_and(|l| l.photo(**id).is_some())) {
        Some(id) => Err(CmdError::NotFound(format!("no photo {id}"))),
        None => Ok(()),
    }
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Open = parse_object(params)?;
    known_photos(app, p.ids.as_deref().unwrap_or_default())?;
    if let Some(Section::Folder(f)) = p.section
        && !app.session().is_some_and(|s| s.library().folder(f).is_some())
    {
        return Err(CmdError::NotFound(format!("no folder {f}")));
    }
    Ok(show_room(app))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tab {
    tab: CollageTab,
}

fn tab(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Tab = parse(params)?;
    app.state_mut().collage.tab = p.tab;
    Ok(json!({ "tab": p.tab }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Style {
    style: String,
}

fn style(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Style = parse(params)?;
    if !STYLES.contains(&p.style.as_str()) {
        return Err(CmdError::BadParams(format!("`style` is one of {}", STYLES.join(", "))));
    }
    need_room(app)?;
    Err(CmdError::Disabled(LATER.into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Borders {
    None,
    White,
    Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Orientation {
    Landscape,
    Portrait,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Settings {
    #[serde(default)]
    borders: Option<Borders>,
    #[serde(default)]
    spacing: Option<f32>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    use_picture: Option<bool>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    orientation: Option<Orientation>,
    #[serde(default)]
    shadows: Option<bool>,
    #[serde(default)]
    captions: Option<bool>,
}

fn settings(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Settings = parse(params)?;
    let any = p.borders.is_some()
        || p.spacing.is_some()
        || p.background.is_some()
        || p.use_picture.is_some()
        || p.format.is_some()
        || p.orientation.is_some()
        || p.shadows.is_some()
        || p.captions.is_some();
    if !any {
        return Err(CmdError::BadParams("name at least one setting".into()));
    }
    if p.spacing.is_some_and(|s| !(s.is_finite() && (0.0..=1.0).contains(&s))) {
        return Err(CmdError::BadParams("`spacing` is 0 (none) … 1 (the most)".into()));
    }
    if p.background.as_deref().is_some_and(|c| !is_colour(c)) {
        return Err(CmdError::BadParams("`background` is a colour \"#rrggbb\"".into()));
    }
    if p.format.as_deref().is_some_and(|f| f != DISPLAY && (f == "manual" || f == "current" || !is_ratio(f))) {
        return Err(CmdError::BadParams("`format` is a ratio \"W:H\" or \"display\"".into()));
    }
    need_room(app)?;
    Err(CmdError::Disabled(LATER.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clips {
    ids: Vec<PhotoId>,
}

fn select_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let mut p: Clips = parse(params)?;
    known_photos(app, &p.ids)?;
    need_room(app)?;
    let mut seen = std::collections::HashSet::new();
    p.ids.retain(|id| seen.insert(*id));
    app.state_mut().collage.clips = p.ids.clone();
    Ok(json!({ "clips": p.ids }))
}

/// A command of the room that waits for M14.18: no parameters, the room open, then [`LATER`].
fn stub(app: &App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need_room(app)?;
    Err(CmdError::Disabled(LATER.into()))
}

fn add_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    stub(app, params)
}

fn remove_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    stub(app, params)
}

fn create_collage(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    stub(app, params)
}

fn save_as_draft(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    stub(app, params)
}

fn reset_settings(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    stub(app, params)
}

fn get_more(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need_room(app)?;
    let state = app.state_mut();
    state.room = Room::Library;
    state.collage.get_more = true;
    Ok(json!({ "room": Room::Library.name() }))
}

fn back_to_collage(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need_room(app)?;
    let state = app.state_mut();
    state.room = Room::Collage;
    state.collage.get_more = false;
    Ok(json!({ "room": Room::Collage.name() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseRoom {
    #[serde(default)]
    discard: bool,
}

fn close_room(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: CloseRoom = parse_object(params)?;
    close(app, p.discard, CollageClose::Tab)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_room_opens_shows_its_tabs_and_closes() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("collage.getMore", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_OPEN));
        assert!(matches!(app.run(OPEN, &json!({ "ids": [999_999] })), Err(CmdError::NotFound(_))));
        app.run("collage.tab", &json!({ "tab": "clips" })).unwrap();
        assert_eq!(app.run(OPEN, &json!({ "ids": ids })).unwrap(), json!({ "room": "collage" }));
        let state = app.state();
        assert_eq!(
            (state.rooms.as_slice(), state.room, state.collage.tab),
            (&[Room::Library, Room::Collage][..], Room::Collage, CollageTab::Settings)
        );
        app.run("collage.selectClips", &json!({ "ids": [ids[1], ids[1], ids[0]] })).unwrap();
        assert_eq!(app.state().collage.clips, [ids[1], ids[0]]);
        // Get More shows the Library tab; Back to Collage the room again.
        app.run("collage.getMore", &json!({})).unwrap();
        assert_eq!((app.state().room, app.state().collage.get_more), (Room::Library, true));
        app.run("collage.backToCollage", &json!({})).unwrap();
        assert_eq!((app.state().room, app.state().collage.get_more), (Room::Collage, false));
        // The collage's own commands check their parameters, then wait.
        assert!(matches!(app.run("collage.style", &json!({ "style": "spiral" })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("collage.style", &json!({ "style": "grid" })), Err(CmdError::Disabled(m)) if m == LATER));
        for bad in [json!({}), json!({ "spacing": 2 }), json!({ "background": "red" }), json!({ "format": "manual" })] {
            assert!(matches!(app.run("collage.settings", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(app.run("collage.settings", &json!({ "format": "display", "shadows": false })), Err(CmdError::Disabled(_))));
        assert!(matches!(app.run("collage.createCollage", &json!({})), Err(CmdError::Disabled(m)) if m == LATER));
        // Nothing unsaved: Close closes at once and drops the room's view state.
        assert_eq!(app.run("collage.closeRoom", &json!({})).unwrap(), json!({ "room": "library" }));
        assert_eq!((app.state().rooms.as_slice(), &app.state().collage.clips), (&[Room::Library][..], &Vec::<PhotoId>::new()));
    }

    #[test]
    fn the_engines_collage_opens_the_room() {
        let mut app = App::new(None);
        ran(&mut app, "create.collage", &Err(CmdError::Disabled("no".into())));
        assert_eq!(app.state().rooms, [Room::Library]);
        ran(&mut app, "photo.star.toggle", &Ok(json!({})));
        assert_eq!(app.state().rooms, [Room::Library]);
        ran(&mut app, "create.collage", &Ok(json!({})));
        assert_eq!((app.state().rooms.as_slice(), app.state().room), (&[Room::Library, Room::Collage][..], Room::Collage));
    }
}
