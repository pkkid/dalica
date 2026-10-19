//! The Collage room's commands (`creating.md` §3). The room's view state is here; the collage
//! itself is the engine's collage session (M14.16: `create.collage`, `collage.*`), which these
//! commands drive, and Create Collage and Save Draft are the engine's `collage.create` and
//! `collage.saveDraft` (M14.17), followed as jobs ([`Following`], [`follow`]).
//!
//! The ids are the UI's own, never an engine command's: the engine owns `create.collage`
//! (Create ▸ Picture Collage…, the bottom bar's and the section headers' Collage) and `collage.*`
//! (`collage.set`, `collage.clips.add`, `collage.create`, `collage.saveDraft`, `collage.reset`,
//! `collage.close`, …), and a UI command of the same id would hide the engine's from
//! `engine.execute`. When the engine's `create.collage` (or `collage.edit`) succeeds the room opens
//! ([`ran`], joined to `App::run`); when its `collage.close` succeeds the room closes.
//!
//! - **`collage.open {ids?, section?}`**: opens the Collage room (its tab last on the strip) or
//!   brings it to the front, the Settings tab shown; with photos (or a section) the engine's
//!   `create.collage` makes them the collage's clips (or adds them to the open one's).
//! - **`collage.tab {tab}`**: the left panel's Settings or Clips tab.
//! - **`collage.style {style}`**, **`collage.settings {…}`**: the style dropdown and the options
//!   under it (Picture borders, Grid spacing, Background, Page format and orientation, Draw
//!   shadows, Show captions), each one step of the room's history through the engine's
//!   `collage.set` (Use picture through `collage.background`). `spacing` is 0 (none) … 1 (the
//!   most, 5 % of the page's short side); `format` is a page format's name as `collage.inspect`
//!   lists it, a ratio `"W:H"` (either way round) or `"display"` (Current Display, at the window's
//!   display size).
//! - **`collage.selectClips {ids}`** (the clips selected in the Clips tab), **`collage.addClips`**
//!   (+: the selected clips onto the page, `collage.place`) and **`collage.removeClips`** (−: out
//!   of the Clips, `collage.clips.remove`).
//! - **`collage.getMore`** shows the Library tab with the tray's **Back to Collage**
//!   (`collage.backToCollage`, `button:tray.backToCollage`), which brings the room back and adds
//!   the tray's photos (the selection and the held photos) to the Clips (`collage.clips.add`).
//! - **`collage.createCollage`**, **`collage.saveAsDraft`**, **`collage.resetSettings`**: Create
//!   Collage, Save Draft and Reset, through the engine's `collage.create`, `collage.saveDraft` and
//!   `collage.reset`. Create's and Save Draft's job is followed: its progress and Cancel show in the
//!   Settings tab, and when Create's ends the new photo opens in the edit view (the job's result's
//!   `id`). Greyed "Not available yet." while the engine lacks them.
//! - **`collage.closeRoom {discard?}`**: Close, as the tab's × (`room.close {room: "collage"}`):
//!   with unsaved changes ([`unsaved`]) it asks [Save Draft] [Discard] [Cancel]
//!   (`dialog:collageClose`) unless `discard` is true. Closing the window asks the same
//!   ([`ask_before_window_closes`]); its Save Draft closes once the draft is written.

use dalica_core::PhotoId;
use dalica_engine::settings::GRID_SPACING_RANGE;
use dalica_engine::{CmdError, JobId, JobState};
use dalica_ui_kit::widgets::ToastKind;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, Dialog, DialogKind};
use crate::state::{CollageClose, CollageRoom, CollageTab, MAX_CLIPS, Room, Section, is_colour};

/// Why a room command cannot run: the room is not open.
pub const NOT_OPEN: &str = "Open a collage first.";
/// Why a command that changes the collage cannot run: the room shows no collage.
pub const NO_COLLAGE: &str = dalica_engine::cmd::collage::NOT_OPEN;
/// Why + and − cannot run: no clip is selected.
pub const NO_CLIPS: &str = "Select pictures in the Clips tab first.";
/// Why Create Collage and Save Draft cannot run: the engine does not make collages yet.
pub const NOT_YET: &str = "Not available yet.";
/// Why Create Collage and Save Draft cannot run: a collage is being written.
pub const BUSY: &str = "A collage is being made.";
/// Why Use picture cannot run: no picture is on the page.
pub const NO_PICTURE: &str = "Put a picture on the page first.";
/// The room's opener (also with photos).
pub const OPEN: &str = "collage.open";
/// The styles, as `collage.style` takes them (`creating.md` §3).
pub const STYLES: [&str; 6] = ["pile", "mosaic", "frameMosaic", "grid", "contactSheet", "multipleExposure"];
/// Current Display: the page format of the screen's size.
pub const DISPLAY: &str = "display";
/// The engine's commands that write a collage (M14.17).
pub const CREATE: &str = "collage.create";
pub const SAVE_DRAFT: &str = "collage.saveDraft";
/// The longest format name `collage.settings` takes.
const MAX_FORMAT: usize = 256;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: OPEN,
        label: "Open the Collage Room",
        menu: "",
        keys: &[],
        params: "{ids?: [PhotoId], section?: Section}: open the Collage room (or bring it to the front) on the Settings tab; the photos (or the section's) become the collage's clips through create.collage (added to an open collage's); replies {room}",
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
        params: r#"{style: "pile" | "mosaic" | "frameMosaic" | "grid" | "contactSheet" | "multipleExposure"}: the collage's style (collage.set; one room step); replies the engine's {changed, unsaved, undo, redo}"#,
        run: style,
    },
    UiCommand {
        id: "collage.settings",
        label: "Collage Settings",
        menu: "",
        keys: &[],
        params: r##"{borders?: "none" | "white" | "instant", spacing?: 0 … 1, background?: "#rrggbb", usePicture?: bool, format?: string (a page format's name as collage.inspect lists it, "W:H", or "display"), orientation?: "landscape" | "portrait", shadows?: bool, captions?: bool}: the collage's settings (at least one; collage.set, and Use picture collage.background); replies the engine's {changed, unsaved, undo, redo}"##,
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
        params: "{}: put the selected clips on the page (+, collage.place)",
        run: add_clips,
    },
    UiCommand {
        id: "collage.removeClips",
        label: "Remove from Clips",
        menu: "",
        keys: &[],
        params: "{}: take the selected clips out of the Clips (−, collage.clips.remove)",
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
        params: "{}: bring the Collage room back after Get More, the tray's photos added to the Clips (collage.clips.add); replies {room, added}",
        run: back_to_collage,
    },
    UiCommand {
        id: "collage.createCollage",
        label: "Create Collage",
        menu: "",
        keys: &[],
        params: "{}: render the collage into the Collages folder (collage.create, a job) and open it in the edit view when it ends; replies the engine's reply",
        run: create_collage,
    },
    UiCommand {
        id: "collage.saveAsDraft",
        label: "Save Draft",
        menu: "",
        keys: &[],
        params: "{}: save the collage as a draft beside the collages (collage.saveDraft, a job); replies the engine's reply",
        run: save_as_draft,
    },
    UiCommand {
        id: "collage.resetSettings",
        label: "Reset",
        menu: "",
        keys: &[],
        params: "{}: the collage's settings back to their defaults (collage.reset)",
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

/// The room's commands grey their buttons while they cannot run.
pub const CHECKS: &[(&str, Check)] = &[
    ("collage.style", collage_open),
    ("collage.settings", collage_open),
    ("collage.selectClips", room_open),
    ("collage.addClips", clips_selected),
    ("collage.removeClips", clips_selected),
    ("collage.getMore", room_open),
    ("collage.backToCollage", room_open),
    ("collage.createCollage", can_create),
    ("collage.saveAsDraft", can_save_draft),
    ("collage.resetSettings", collage_open),
    ("collage.closeRoom", room_open),
];

/// Whether the Collage room is open; [`NOT_OPEN`] when it is not.
pub fn room_open(app: &App, _: &Value) -> Result<(), String> {
    if app.state().rooms.contains(&Room::Collage) { Ok(()) } else { Err(NOT_OPEN.into()) }
}

/// The room open and showing the engine's collage; [`NO_COLLAGE`] when it shows none.
pub fn collage_open(app: &App, params: &Value) -> Result<(), String> {
    room_open(app, params)?;
    if app.session().is_some_and(|s| s.collage().is_open()) { Ok(()) } else { Err(NO_COLLAGE.into()) }
}

fn clips_selected(app: &App, params: &Value) -> Result<(), String> {
    collage_open(app, params)?;
    if app.state().collage.clips.is_empty() { Err(NO_CLIPS.into()) } else { Ok(()) }
}

/// Create Collage and Save Draft: the collage open, the engine's `command` registered and no
/// collage being written.
fn can_write(app: &App, params: &Value, command: &str) -> Result<(), String> {
    collage_open(app, params)?;
    if app.session().and_then(|s| s.spec(command)).is_none() {
        return Err(NOT_YET.into());
    }
    if running(app).is_some() {
        return Err(BUSY.into());
    }
    Ok(())
}

fn can_create(app: &App, params: &Value) -> Result<(), String> {
    can_write(app, params, CREATE)
}

fn can_save_draft(app: &App, params: &Value) -> Result<(), String> {
    can_write(app, params, SAVE_DRAFT)
}

fn need(check: Check, app: &App) -> Result<(), CmdError> {
    check(app, &Value::Null).map_err(CmdError::Disabled)
}

/// Whether the open collage has changes not saved as a collage or a draft (the engine's
/// collage session says so).
pub fn unsaved(app: &App) -> bool {
    app.session().is_some_and(|s| s.collage().is_changed())
}

// --- following Create Collage and Save Draft -----------------------------------------------------

/// What a followed job does when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Create Collage: the new photo opens in the edit view.
    Create,
    /// Save Draft; then the room (or the window) closes, when the close question asked for it.
    Draft { then_close: Option<CollageClose> },
}

/// What the room follows from frame to frame ([`App`] keeps one).
#[derive(Debug, Default)]
pub struct Following {
    /// The `collage` job Create Collage or Save Draft started, and what its end does.
    job: Option<(JobId, Purpose)>,
    /// The display's size in pixels as the window last gave it (Current Display).
    display: Option<[u32; 2]>,
    /// Create Collage's job, done, whose photo opens in the edit view once the library holds it
    /// (a new Collages folder is scanned after the job).
    open_made: Option<JobId>,
}

impl Following {
    /// The job followed, if any.
    pub fn job(&self) -> Option<(JobId, Purpose)> {
        self.job
    }

    /// The display's size the window last gave.
    pub fn display(&self) -> Option<[u32; 2]> {
        self.display
    }
}

/// The display's size in pixels (the panel tells it every frame the room is drawn); it goes to the
/// engine's collage when it changes.
pub fn set_display(app: &mut App, display: [u32; 2]) {
    if display[0] == 0 || display[1] == 0 {
        return;
    }
    app.collage.display = Some(display);
    let Some(session) = app.session_mut() else { return };
    let known = session.collage().display().map(|(w, h)| [w, h]);
    if session.collage().is_open() && known != Some(display) {
        let _ = session.execute("collage.set", &json!({ "display": display }));
    }
}

/// The collage job running now (Create Collage or Save Draft): the one followed, else the
/// engine's own (started over automation).
pub fn running(app: &App) -> Option<dalica_engine::JobEntry> {
    let session = app.session()?;
    let job = app.collage.job.map(|(j, _)| j).or_else(|| session.collage_output().job())?;
    session.job(job).filter(|e| !e.state.is_finished())
}

/// The job a reply names (`{job}`), else the engine's collage job.
fn job_of(app: &App, reply: &Value) -> Option<JobId> {
    reply.get("job").and_then(Value::as_u64).or_else(|| app.session().and_then(|s| s.collage_output().job()))
}

/// The new photo a finished Create names: `photo` (or `id`) in its result.
fn new_photo(result: &Value) -> Option<PhotoId> {
    ["photo", "id"].iter().find_map(|k| result.get(*k).and_then(Value::as_u64)).filter(|id| *id > 0).map(PhotoId::new)
}

/// Shows photo `id` in the edit view, the Library tab in front (Create Collage's new photo).
fn open_in_edit_view(app: &mut App, id: PhotoId) {
    app.state_mut().room = Room::Library;
    app.state_mut().collage.get_more = false;
    if let Err(e) = app.run("view.edit", &json!({ "id": id })) {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// Create Collage's photo, once the library holds it: the engine's last collage made by `job`
/// (`Session::collage_made`). `Err` when another collage was made since.
fn made_photo(app: &App, job: JobId) -> Result<Option<PhotoId>, ()> {
    let Some(session) = app.session() else { return Err(()) };
    match session.collage_made() {
        Some(made) if made.job == job => Ok(made.photo.filter(|id| session.library().photo(*id).is_some())),
        Some(_) => Err(()),
        None => Ok(None),
    }
}

/// Each frame: when the followed job has ended, does what it was for. Create Collage's opens its
/// photo in the edit view (the Library tab in front); Save Draft's closes the room (and the
/// window) when the close question asked for it. A failure is a toast and the status line; a
/// cancelled job does nothing more.
pub fn follow(app: &mut App) {
    if let Some(job) = app.collage.open_made {
        match made_photo(app, job) {
            Ok(Some(id)) => {
                app.collage.open_made = None;
                open_in_edit_view(app, id);
            }
            Ok(None) => {}
            Err(()) => app.collage.open_made = None,
        }
    }
    let Some((job, purpose)) = app.collage.job else { return };
    let Some(entry) = app.session().and_then(|s| s.job(job)) else {
        app.collage.job = None;
        return;
    };
    if !entry.state.is_finished() {
        return;
    }
    app.collage.job = None;
    match entry.state {
        JobState::Done => done(app, job, purpose, &entry.result.unwrap_or(Value::Null)),
        JobState::Failed => {
            let why = entry.error.unwrap_or_else(|| "The collage could not be written.".into());
            app.state_mut().status = why.clone();
            app.toast(&why, ToastKind::Caution);
        }
        _ => {}
    }
}

fn done(app: &mut App, job: JobId, purpose: Purpose, result: &Value) {
    let wrote = result.get("path").is_some_and(|p| !p.is_null()) || new_photo(result).is_some();
    match purpose {
        Purpose::Create => match new_photo(result) {
            Some(id) => open_in_edit_view(app, id),
            // The photo joins the library when its folder is scanned: opened then ([`follow`]).
            None if wrote => app.collage.open_made = Some(job),
            None => {}
        },
        Purpose::Draft { .. } if !wrote => {}
        Purpose::Draft { then_close: Some(closing) } => {
            if let Err(e) = close(app, true, closing) {
                app.toast(&e.to_string(), ToastKind::Caution);
            }
        }
        Purpose::Draft { then_close: None } => {}
    }
}

/// Runs the engine's `command` (Create Collage or Save Draft) and follows its job for `purpose`;
/// a reply without a job (written at once) does what the job's end would.
fn write(app: &mut App, command: &str, purpose: Purpose) -> Result<Value, CmdError> {
    need(if command == CREATE { can_create } else { can_save_draft }, app)?;
    let params = match (command, app.collage.display) {
        (CREATE, Some(d)) => json!({ "display": d }),
        _ => json!({}),
    };
    let reply = match app.session_mut() {
        Some(s) => s.execute(command, &params)?,
        None => return Err(CmdError::Disabled(NOT_YET.into())),
    };
    match job_of(app, &reply).filter(|j| app.session().and_then(|s| s.job(*j)).is_some()) {
        Some(job) => app.collage.job = Some((job, purpose)),
        None => done(app, 0, purpose, &reply),
    }
    Ok(reply)
}

/// Save Draft, then (once it is written) closes what `then_close` names: the close question's
/// Save Draft.
pub fn save_draft(app: &mut App, then_close: Option<CollageClose>) -> Result<Value, CmdError> {
    write(app, SAVE_DRAFT, Purpose::Draft { then_close })
}

// --- opening and closing -------------------------------------------------------------------------

/// After command `id` ran with `result`: the engine's `create.collage` and `collage.edit` open
/// the room on success; its `collage.close` closes it.
pub fn ran(app: &mut App, id: &str, result: &Result<Value, CmdError>) {
    if result.is_err() {
        return;
    }
    match id {
        "create.collage" | "collage.edit" => {
            show_room(app);
        }
        "collage.close" => {
            drop_room(app);
        }
        _ => {}
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

/// Takes the room's tab away and drops its view state (its tab kept); the Library comes to the
/// front if the room was. Replies `{room}` (the room now in front).
fn drop_room(app: &mut App) -> Value {
    let state = app.state_mut();
    state.rooms.retain(|r| *r != Room::Collage);
    if state.room == Room::Collage {
        state.room = Room::Library;
    }
    state.collage = CollageRoom { tab: state.collage.tab, ..CollageRoom::default() };
    json!({ "room": state.room.name() })
}

/// Closes the Collage room, unless it has unsaved changes and `discard` is false: then the close
/// question asks first (`dialog:collageClose`, what closes noted as `closing`). Closing ends the
/// engine's collage (its changes discarded), drops the room's view state (its tab kept) and puts
/// the Library in front if the room was; closing the window then asks it to close. Replies
/// `{room}` (the room now in front), or `{dialog}` when it asks.
pub fn close(app: &mut App, discard: bool, closing: CollageClose) -> Result<Value, CmdError> {
    need(room_open, app)?;
    if unsaved(app) && !discard {
        let reply = dialogs::open(app, Dialog::new(DialogKind::CollageClose))?;
        app.state_mut().collage.close = Some(closing);
        return Ok(reply);
    }
    if let Some(s) = app.session_mut().filter(|s| s.collage().is_open()) {
        s.execute("collage.close", &json!({ "discard": true }))?;
    }
    let reply = drop_room(app);
    if closing == CollageClose::Window {
        app.request_quit();
    }
    Ok(reply)
}

/// The window is asked to close: with unsaved changes in the collage the close question asks
/// first (what closes: the window), and `true` says to keep the window open meanwhile. While the
/// question is already asked the window stays open too.
pub fn ask_before_window_closes(app: &mut App) -> bool {
    if !app.state().rooms.contains(&Room::Collage) || !unsaved(app) {
        return false;
    }
    if app.state().dialogs.iter().any(|d| d.kind == DialogKind::CollageClose) {
        return true;
    }
    if app.state().room != Room::Collage {
        app.state_mut().room = Room::Collage;
        app.state_mut().collage.get_more = false;
    }
    close(app, false, CollageClose::Window).is_ok()
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
    let what = match (&p.ids, p.section) {
        (Some(ids), _) if !ids.is_empty() => Some(json!({ "ids": ids })),
        (_, Some(section)) => Some(json!({ "section": section })),
        _ => None,
    };
    if let Some(what) = what {
        app.run("create.collage", &what)?;
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

/// Runs an engine command of the collage session.
fn engine(app: &mut App, id: &str, params: &Value) -> Result<Value, CmdError> {
    match app.session_mut() {
        Some(s) => s.execute(id, params),
        None => Err(CmdError::Disabled("The library is still opening.".into())),
    }
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
    need(collage_open, app)?;
    engine(app, "collage.set", &json!({ "style": p.style }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Borders {
    None,
    White,
    Instant,
}

impl Borders {
    fn id(&self) -> &'static str {
        match self {
            Borders::None => "none",
            Borders::White => "white",
            Borders::Instant => "instant",
        }
    }
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
    spacing: Option<f64>,
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

/// The page format `format` names among the engine's `formats` (`collage.inspect`): a name, else a
/// ratio "W:H" either way round.
fn format_named(formats: &[Value], format: &str) -> Option<String> {
    let name = |f: &Value| f.get("name").and_then(Value::as_str).map(str::to_string);
    if let Some(f) = formats.iter().find(|f| name(f).as_deref() == Some(format)) {
        return name(f);
    }
    let (w, h) = format.split_once(':')?;
    let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
    let want = w / h;
    if !(want.is_finite() && want > 0.0) {
        return None;
    }
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    formats
        .iter()
        .filter(|f| name(f).as_deref() != Some(dalica_create::collage::page::CURRENT_DISPLAY))
        .find(|f| {
            let (fw, fh) = (f.get("width").and_then(Value::as_f64).unwrap_or(0.0), f.get("height").and_then(Value::as_f64).unwrap_or(0.0));
            fw > 0.0 && fh > 0.0 && (same(fw / fh, want) || same(fh / fw, want))
        })
        .and_then(name)
}

/// The engine's page formats now (`collage.inspect`'s `formats`).
pub fn formats(app: &mut App) -> Vec<Value> {
    let inspected = engine(app, "collage.inspect", &json!({})).unwrap_or(Value::Null);
    inspected.get("formats").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// The picture Use picture covers the page with: the background already chosen, else the first
/// picture selected on the canvas, else the first on the page.
fn picture_for_background(app: &mut App) -> Option<PhotoId> {
    let selected = app.state().collage.selection.first().copied();
    let session = app.session_mut()?;
    if let Some(id) = session.collage().document().and_then(|d| d.background.picture) {
        return Some(id);
    }
    let placements = session.collage_placements();
    selected.and_then(|n| placements.get(n as usize)).or_else(|| placements.first()).map(|p| p.id)
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
    if p.format.as_deref().is_some_and(|f| {
        let f = f.trim();
        f.is_empty() || f.chars().count() > MAX_FORMAT || f.chars().any(char::is_control) || f == "manual" || f == "current"
    }) {
        return Err(CmdError::BadParams("`format` is a page format's name, a ratio \"W:H\" or \"display\"".into()));
    }
    need(collage_open, app)?;
    let mut set = serde_json::Map::new();
    if let Some(b) = &p.borders {
        set.insert("borders".into(), json!(b.id()));
    }
    if let Some(s) = p.spacing {
        set.insert("gridSpacing".into(), json!(s * GRID_SPACING_RANGE.end()));
    }
    if let Some(c) = &p.background {
        set.insert("backgroundColor".into(), json!(c.to_ascii_lowercase()));
    }
    if let Some(o) = &p.orientation {
        set.insert("orientation".into(), json!(if matches!(o, Orientation::Portrait) { "portrait" } else { "landscape" }));
    }
    if let Some(v) = p.shadows {
        set.insert("shadows".into(), json!(v));
    }
    if let Some(v) = p.captions {
        set.insert("captions".into(), json!(v));
    }
    if let Some(f) = p.format.as_deref().map(str::trim) {
        if f == DISPLAY {
            let display = app.collage.display.ok_or_else(|| CmdError::Disabled("The display's size is not known yet.".into()))?;
            set.insert("pageFormat".into(), json!(dalica_create::collage::page::CURRENT_DISPLAY));
            set.insert("display".into(), json!(display));
        } else {
            let all = formats(app);
            let name = format_named(&all, f).ok_or_else(|| {
                let shown: String = f.chars().take(64).collect();
                CmdError::NotFound(format!("no page format {shown:?} (collage.inspect lists them)"))
            })?;
            set.insert("pageFormat".into(), json!(name));
        }
    }
    // Use picture: the picture chosen before the other settings are taken, so a refusal changes
    // nothing.
    let background = match p.use_picture {
        Some(true) => Some(json!(picture_for_background(app).ok_or_else(|| CmdError::Disabled(NO_PICTURE.into()))?)),
        Some(false) => Some(Value::Null),
        None => None,
    };
    let mut reply = Value::Null;
    if !set.is_empty() {
        reply = engine(app, "collage.set", &Value::Object(set))?;
    }
    if let Some(id) = background {
        reply = engine(app, "collage.background", &json!({ "id": id }))?;
    }
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clips {
    ids: Vec<PhotoId>,
}

fn select_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let mut p: Clips = parse(params)?;
    known_photos(app, &p.ids)?;
    need(room_open, app)?;
    let mut seen = std::collections::HashSet::new();
    p.ids.retain(|id| seen.insert(*id));
    app.state_mut().collage.clips = p.ids.clone();
    Ok(json!({ "clips": p.ids }))
}

fn add_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(clips_selected, app)?;
    let ids = app.state().collage.clips.clone();
    engine(app, "collage.place", &json!({ "ids": ids }))
}

fn remove_clips(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(clips_selected, app)?;
    let ids = app.state().collage.clips.clone();
    let reply = engine(app, "collage.clips.remove", &json!({ "ids": ids }))?;
    app.state_mut().collage.clips.clear();
    app.state_mut().collage.selection.clear();
    Ok(reply)
}

fn create_collage(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    write(app, CREATE, Purpose::Create)
}

fn save_as_draft(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    save_draft(app, None)
}

fn reset_settings(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(collage_open, app)?;
    engine(app, "collage.reset", &json!({}))
}

fn get_more(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(room_open, app)?;
    let state = app.state_mut();
    state.room = Room::Library;
    state.collage.get_more = true;
    Ok(json!({ "room": Room::Library.name() }))
}

fn back_to_collage(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(room_open, app)?;
    let tray = crate::tray::photos(app.state());
    let mut added = 0;
    if !tray.is_empty() && app.session().is_some_and(|s| s.collage().is_open()) {
        let reply = engine(app, "collage.clips.add", &json!({ "ids": tray }))?;
        added = reply.get("added").and_then(Value::as_u64).unwrap_or(0);
    }
    let state = app.state_mut();
    state.room = Room::Collage;
    state.collage.get_more = false;
    Ok(json!({ "room": Room::Collage.name(), "added": added }))
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
    fn the_room_opens_on_the_engines_collage_shows_its_tabs_and_closes() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("collage.getMore", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_OPEN));
        assert!(matches!(app.run(OPEN, &json!({ "ids": [999_999] })), Err(CmdError::NotFound(_))));
        app.run("collage.tab", &json!({ "tab": "clips" })).unwrap();
        assert_eq!(app.run(OPEN, &json!({ "ids": [ids[0], ids[1]] })).unwrap(), json!({ "room": "collage" }));
        let state = app.state();
        assert_eq!(
            (state.rooms.as_slice(), state.room, state.collage.tab),
            (&[Room::Library, Room::Collage][..], Room::Collage, CollageTab::Settings)
        );
        assert_eq!(app.session().unwrap().collage().document().unwrap().clips.len(), 2);
        app.run("collage.selectClips", &json!({ "ids": [ids[1], ids[1], ids[0]] })).unwrap();
        assert_eq!(app.state().collage.clips, [ids[1], ids[0]]);
        // Get More shows the Library tab; Back to Collage the room again, with the tray's photo.
        app.run("collage.getMore", &json!({})).unwrap();
        assert_eq!((app.state().room, app.state().collage.get_more), (Room::Library, true));
        app.run("select.set", &json!({ "ids": [ids[2]] })).unwrap();
        assert_eq!(app.run("collage.backToCollage", &json!({})).unwrap(), json!({ "room": "collage", "added": 1 }));
        assert_eq!((app.state().room, app.state().collage.get_more), (Room::Collage, false));
        assert_eq!(app.session().unwrap().collage().document().unwrap().clips.len(), 3);
        // The settings reach the engine's collage, each a room step.
        assert!(matches!(app.run("collage.style", &json!({ "style": "spiral" })), Err(CmdError::BadParams(_))));
        app.run("collage.style", &json!({ "style": "grid" })).unwrap();
        for bad in [json!({}), json!({ "spacing": 2 }), json!({ "background": "red" }), json!({ "format": "manual" })] {
            assert!(matches!(app.run("collage.settings", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(app.run("collage.settings", &json!({ "format": "7:3" })), Err(CmdError::NotFound(_))));
        app.run("collage.settings", &json!({ "spacing": 1.0, "background": "#336699", "format": "3:4", "orientation": "portrait" })).unwrap();
        let doc = app.session().unwrap().collage().document().unwrap().clone();
        assert_eq!((doc.style.id(), doc.background.colour, doc.page.format.name()), ("grid", [0x33, 0x66, 0x99], "4:3 (Standard screen)"));
        assert!((doc.settings.spacing - 0.05).abs() < 1e-9);
        assert_eq!(doc.page.orientation, dalica_create::collage::Orientation::Portrait);
        app.run("collage.settings", &json!({ "usePicture": true })).unwrap();
        assert!(app.session().unwrap().collage().document().unwrap().background.picture.is_some());
        app.run("collage.resetSettings", &json!({})).unwrap();
        assert_eq!(app.session().unwrap().collage().document().unwrap().style.id(), "pile");
        // − takes the selected clips out of the Clips.
        app.run("collage.removeClips", &json!({})).unwrap();
        assert_eq!(app.session().unwrap().collage().document().unwrap().clips.len(), 1);
        assert!(matches!(app.run("collage.addClips", &json!({})), Err(CmdError::Disabled(m)) if m == NO_CLIPS));
        // Unsaved changes: Close asks; Discard closes the engine's collage too.
        assert!(unsaved(&app));
        assert_eq!(app.run("collage.closeRoom", &json!({})).unwrap(), json!({ "dialog": "collageClose" }));
        assert_eq!(app.state().collage.close, Some(CollageClose::Tab));
        app.state_mut().dialogs.clear();
        assert_eq!(app.run("collage.closeRoom", &json!({ "discard": true })).unwrap(), json!({ "room": "library" }));
        assert_eq!((app.state().rooms.as_slice(), &app.state().collage.clips), (&[Room::Library][..], &Vec::<PhotoId>::new()));
        assert!(!app.session().unwrap().collage().is_open());
    }

    #[test]
    fn the_engines_collage_opens_and_closes_the_room() {
        let mut app = App::new(None);
        ran(&mut app, "create.collage", &Err(CmdError::Disabled("no".into())));
        assert_eq!(app.state().rooms, [Room::Library]);
        ran(&mut app, "photo.star.toggle", &Ok(json!({})));
        assert_eq!(app.state().rooms, [Room::Library]);
        ran(&mut app, "create.collage", &Ok(json!({})));
        assert_eq!((app.state().rooms.as_slice(), app.state().room), (&[Room::Library, Room::Collage][..], Room::Collage));
        ran(&mut app, "collage.close", &Ok(json!({ "closed": true })));
        assert_eq!((app.state().rooms.as_slice(), app.state().room), (&[Room::Library][..], Room::Library));
    }

    #[test]
    fn a_page_format_is_found_by_name_or_by_ratio_either_way_round() {
        let formats = vec![
            json!({ "name": "4 × 6", "width": 4.0, "height": 6.0, "custom": false }),
            json!({ "name": "16:9 (HD video)", "width": 16.0, "height": 9.0, "custom": false }),
            json!({ "name": "Current Display", "width": 1400.0, "height": 900.0, "custom": false }),
        ];
        assert_eq!(format_named(&formats, "4 × 6").as_deref(), Some("4 × 6"));
        assert_eq!(format_named(&formats, "16:9").as_deref(), Some("16:9 (HD video)"));
        assert_eq!(format_named(&formats, "3:2").as_deref(), Some("4 × 6"));
        assert_eq!(format_named(&formats, "14:9"), None, "Current Display is chosen by name only");
        for bad in ["5:0", "x:y", ":", "1:nan", "inf:1"] {
            assert_eq!(format_named(&formats, bad), None, "{bad}");
        }
        assert_eq!(new_photo(&json!({ "id": 7 })), Some(PhotoId::new(7)));
        assert_eq!(new_photo(&json!({ "photo": 3 })), Some(PhotoId::new(3)));
        assert_eq!(new_photo(&json!({ "id": "x" })), None);
    }
}
