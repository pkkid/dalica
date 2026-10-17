//! The slideshow's commands (`viewing.md` §1): starting a show and, while one plays, the keys of
//! §1.4 in the Slideshow context.
//!
//! **What plays** (`viewing.md` §1.1, `roadmap.md` M11's decisions): `view.slideshow` (View ▸
//! Slideshow, Ctrl+4, the bottom area's Slideshow button, the edit top bar's Play) plays the photos
//! named (`ids`, as the photo viewer's Play passes them); else, in the edit view, the photo's
//! folder or album from that photo; else, in the library, the Photo Tray's photos in tray order when
//! it holds two or more, else the section of the focused photo from that photo, else the first
//! section in view from its start (a search's results are the search section). `section.slideshow`
//! (a section header's ▶, Folder ▸ View Slideshow) plays that section, or the current one, from the
//! selected photo when one is selected in it. Either way the engine's `slideshow.list` makes the
//! list (the section's display order, hidden photos only while shown, videos left out).
//!
//! **The show.** The view state's `slideshow` is what scripts read and set; the show itself is the
//! engine's [`Running`] slideshow (M11.4's state machine on the session's clock, the photo shown and
//! the next in the preview's two panes), held here per UI thread, as the edit view's tools keep
//! their state. Every frame the panel calls [`sync`]: the show catches up with the clock, and its
//! place, playing state, bar and transition are written back into the view state; a change a script
//! made there (another list, another place, paused, zoomed) is applied to the show first. A show
//! started by its commands asks the OS for full screen through the view state, and Exit gives the
//! window back as it was (a window already full screen stays so); a show a script set up with
//! `ui.set` leaves the window to the script.
//!
//! **Keys** (Slideshow context): Esc exits, Space pauses and resumes (showing the bar), → and PgDn
//! next, ← and PgUp previous (a command has at most two keys per context, so ↓ and ↑ are left to the
//! user to bind; the wheel steps too), Home and End the first and last photo, + and − zoom, Ctrl+8
//! stars and Ctrl+R / Ctrl+Shift+R rotate the photo shown through the library's own commands (one
//! undo step each; commands of their own, so the library's keys of `photo.star.toggle` and
//! `photo.rotate.*` stay as they are). Exit goes back to where the show started, the last photo
//! shown selected and scrolled into view (in the edit view: shown there).
//!
//! **The bar's settings** (`viewing.md` §1.3, §1.5): `slideshow.transition`, `slideshow.captions`
//! and `slideshow.displayTime` store Options ▸ Slideshow's setting through `options.set` (one
//! write of `settings.json`, no undo step), so the bar, the edit view's caption-bar toggle and the
//! Options tab change one setting; they run whenever a library is open, a show playing or not.
//! [`sync`] gives the held show the settings in effect every frame (display time, transition and
//! Loop), so a change from any of them applies to the show playing at once: a new transition from
//! the next change, a display time counting the time already spent (M11.4's rule).
//!
//! **Zoom** (`viewing.md` §1.3): + and − (and the bar's magnifier and slider, [`set_zoom`]) set the
//! view state's zoom; above 1 the show pauses (M11.4's Zoomed) and a drag pans
//! (`panels/slideshow/zoom.rs`). Moving to another photo ends the zoom: the next photo is shown
//! fitted, the pan back at the centre.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use dalica_core::PhotoId;
use dalica_create::slideshow::{Show, State};
use dalica_engine::cmd::slideshow::{Frame, ShowStats, Slideshow as Running, show_settings};
use dalica_engine::{CmdError, Context, Session};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::selection::{Table, commit};
use crate::state::{SLIDE_ZOOM_MAX, Section, Selection, ShowFrom, Slideshow, TransitionRun, View, Zoom};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "view.slideshow",
        label: "Slideshow",
        menu: "View/Slideshow",
        keys: &[(Context::Library, "Ctrl+4"), (Context::EditView, "Ctrl+4")],
        params: "{ids?: [PhotoId], start?: PhotoId}: play a slideshow: the photos named, else (in the edit view) the photo's folder or album from it, else the tray's two or more, else the focused photo's section from it, else the first section in view",
        run: start,
    },
    UiCommand {
        id: "section.slideshow",
        label: "View Slideshow",
        // Its menu row (Folder ▸ View Slideshow) is the catalog's; Ctrl+4 is View ▸ Slideshow's.
        menu: "",
        keys: &[],
        params: r#"{section?: "folder:<id>" | "album:<id>" | "album:starred" | "album:recent" | "album:search", ids?: [PhotoId]}: play a slideshow of the section (else the current one), from the first of `ids` (else the selected photo, when it is in it)"#,
        run: start_section,
    },
    UiCommand {
        id: "slideshow.exit",
        label: "Exit Slideshow",
        menu: "",
        keys: &[(Context::Slideshow, "Esc")],
        params: "{}: end the slideshow and go back to where it started, the last photo shown selected",
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
        params: "{on?: bool}: pause (true) or play (false); toggles without `on` (at the end, Play restarts from the first photo); shows the control bar",
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
        params: "{}: zoom the photo shown out, back to fitted (the show goes on as before)",
        run: zoom_out,
    },
    UiCommand {
        id: "slideshow.star",
        label: "Star / Unstar",
        menu: "",
        keys: &[(Context::Slideshow, "Ctrl+8")],
        params: "{}: star the photo shown, or unstar it (photo.star.toggle; one undo step)",
        run: star,
    },
    UiCommand {
        id: "slideshow.rotateRight",
        label: "Rotate Clockwise",
        menu: "",
        keys: &[(Context::Slideshow, "Ctrl+R")],
        params: "{}: turn the photo shown a quarter turn clockwise (photo.rotate.cw; one undo step)",
        run: rotate_right,
    },
    UiCommand {
        id: "slideshow.rotateLeft",
        label: "Rotate Counterclockwise",
        menu: "",
        keys: &[(Context::Slideshow, "Ctrl+Shift+R")],
        params: "{}: turn the photo shown a quarter turn counterclockwise (photo.rotate.ccw; one undo step)",
        run: rotate_left,
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

/// Why the show's own commands cannot run.
pub const NOT_PLAYING: &str = "No slideshow is playing.";
/// Why a show cannot start: nothing to play.
pub const NOTHING_TO_PLAY: &str = "There are no photos to play.";
/// Why a show cannot start while one plays.
pub const ALREADY_PLAYING: &str = "A slideshow is already playing.";
/// Why the photo viewer cannot start a show without naming its photos.
pub const VIEWER_NAMES: &str = "Name the photos to play.";
const OPENING: &str = "The library is still opening.";

/// The zoom step of + and −.
pub const ZOOM_STEP: f32 = 1.25;

/// The display a show's previews cover until the panel has drawn it once (device pixels).
pub const DEFAULT_FIT: (u32, u32) = (1920, 1080);

pub const CHECKS: &[(&str, Check)] = &[
    ("view.slideshow", can_start),
    ("section.slideshow", can_start_section),
    ("slideshow.exit", playing),
    ("slideshow.next", playing),
    ("slideshow.previous", playing),
    ("slideshow.first", playing),
    ("slideshow.last", playing),
    ("slideshow.pause", playing),
    ("slideshow.zoomIn", playing),
    ("slideshow.zoomOut", playing),
    ("slideshow.star", playing),
    ("slideshow.rotateRight", playing),
    ("slideshow.rotateLeft", playing),
    ("slideshow.transition", opened),
    ("slideshow.captions", opened),
    ("slideshow.displayTime", opened),
];

fn opened(app: &App, _: &Value) -> Result<(), String> {
    if app.session().is_some() { Ok(()) } else { Err(OPENING.into()) }
}

fn playing(app: &App, _: &Value) -> Result<(), String> {
    show(app).map(|_| ()).ok_or_else(|| NOT_PLAYING.into())
}

/// Whether a show can start from the view shown (cheap: the buttons ask every frame).
fn can_start(app: &App, params: &Value) -> Result<(), String> {
    if app.session().is_none() {
        return Err(OPENING.into());
    }
    let named = params.get("ids").and_then(Value::as_array).is_some_and(|ids| !ids.is_empty());
    match app.state().view {
        View::Slideshow => Err(ALREADY_PLAYING.into()),
        View::Viewer if !named => Err(VIEWER_NAMES.into()),
        View::Edit if !named && app.state().edit.photo.is_none() => Err(NOTHING_TO_PLAY.into()),
        _ => Ok(()),
    }
}

fn can_start_section(app: &App, _: &Value) -> Result<(), String> {
    if app.session().is_none() {
        return Err(OPENING.into());
    }
    if app.state().view == View::Slideshow { Err(ALREADY_PLAYING.into()) } else { Ok(()) }
}

/// The slideshow playing, if one is.
pub fn show(app: &App) -> Option<&Slideshow> {
    app.state().slideshow.as_ref().filter(|_| app.state().view == View::Slideshow)
}

// --- The show held for the view state --------------------------------------------------------------

/// The engine's show behind the view state's `slideshow`, on this (the UI) thread.
struct Held {
    /// The library it plays from (a show of another session on this thread is dropped, never
    /// closed: its panes are that session's).
    owner: PathBuf,
    show: Running,
    /// The view state's `slideshow` as last written from the show: what a script changed since is
    /// told apart from it.
    mirrored: Slideshow,
    /// Whether the window was full screen before the show asked for it (`None`: it did not ask).
    was_full_screen: Option<bool>,
    /// The last frame [`sync`] answered.
    frame: Option<Frame>,
}

thread_local! {
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
    /// The display the panel last drew the show on (device pixels).
    static FIT: Cell<(u32, u32)> = const { Cell::new(DEFAULT_FIT) };
    /// A command is starting a show: whether the window was full screen before it.
    static ASKED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The display the show's previews cover (the panel's rect in device pixels).
pub fn set_fit(fit: (u32, u32)) {
    FIT.with(|f| f.set((fit.0.max(1), fit.1.max(1))));
}

/// The show's counts (changes, and those whose incoming photo was not ready), while one is held.
pub fn stats() -> Option<ShowStats> {
    HELD.with(|h| h.borrow().as_ref().map(|held| held.show.stats()))
}

/// The last frame of the show (what the panel paints), while one is held.
pub fn frame() -> Option<Frame> {
    HELD.with(|h| h.borrow().as_ref().and_then(|held| held.frame))
}

/// Runs `f` on the held show's state machine, if a show is held.
pub fn with_show<R>(f: impl FnOnce(&mut Show) -> R) -> Option<R> {
    HELD.with(|h| h.borrow_mut().as_mut().map(|held| f(held.show.show_mut())))
}

/// What [`sync`] decided about the window and the view state.
struct Synced {
    /// The view state's `slideshow` as the show now is (`None`: no show).
    state: Option<Slideshow>,
    /// Full screen to set back as it was when a show its commands started ends.
    full_screen: Option<bool>,
    frame: Option<Frame>,
}

/// Brings the held show in line with the view state (starting, restarting or ending it, applying a
/// place, a pause or a zoom a script set), moves it on to the session clock's time and writes its
/// place, playing state, bar and transition back into the view state. Answers the frame to paint.
pub fn sync(app: &mut App) -> Option<Frame> {
    let wanted = show(app).cloned();
    let asked = ASKED.with(Cell::take);
    let fit = FIT.with(Cell::get);
    let session = app.session_mut()?;
    let synced = HELD.with(|h| sync_held(&mut h.borrow_mut(), session, wanted, asked, fit));
    let state = app.state_mut();
    if let Some(on) = synced.full_screen {
        state.full_screen = on;
    }
    if let Some(s) = synced.state
        && state.view == View::Slideshow
    {
        state.slideshow = Some(s);
    }
    synced.frame
}

fn sync_held(slot: &mut Option<Held>, session: &mut Session, wanted: Option<Slideshow>, asked: Option<bool>, fit: (u32, u32)) -> Synced {
    let owner = session.dir().to_path_buf();
    if slot.as_ref().is_some_and(|h| h.owner != owner) {
        *slot = None;
    }
    let Some(want) = wanted else {
        let ended = slot.take().and_then(|h| {
            h.show.close(session);
            h.was_full_screen
        });
        return Synced { state: None, full_screen: ended, frame: None };
    };
    if asked.is_some() || slot.as_ref().is_none_or(|h| h.mirrored.photos != want.photos || h.mirrored.index != want.index) {
        let was_full_screen = match slot.take() {
            Some(h) => {
                h.show.close(session);
                h.was_full_screen.or(asked)
            }
            None => asked,
        };
        let show = Running::start(session, want.photos.clone(), want.index, fit);
        // Fresh from the engine: playing and fitted; what the view state says otherwise is applied
        // below.
        let mirrored = Slideshow { playing: true, zoom: 1.0, ..want.clone() };
        *slot = Some(Held { owner, show, mirrored, was_full_screen, frame: None });
    }
    let Some(held) = slot.as_mut() else { return Synced { state: None, full_screen: None, frame: None } };
    held.show.set_fit(session, fit);
    let settings = show_settings(&session.settings().slideshow());
    let machine = held.show.show_mut();
    apply_settings(machine, settings);
    let zoomed = want.zoom > 1.0;
    if zoomed != (held.mirrored.zoom > 1.0) {
        machine.set_zoomed(zoomed);
    }
    if want.playing != held.mirrored.playing && !zoomed {
        machine.play_pause();
    }
    let frame = held.show.tick(session);
    let v = frame.view;
    // Another photo (a step, the timer, Play at the end) ends the zoom: it is shown fitted.
    let unzoomed = zoomed && v.state != State::Zoomed;
    let state = Slideshow {
        index: v.index.unwrap_or(0).min(want.photos.len().saturating_sub(1)),
        playing: v.state == State::Playing,
        bar: v.bar,
        transition: v.change.map(|(from, progress)| TransitionRun { from, progress }),
        zoom: if unzoomed { 1.0 } else { want.zoom },
        pan: if unzoomed { [0.5, 0.5] } else { want.pan },
        ..want
    };
    held.mirrored = state.clone();
    held.frame = Some(frame);
    Synced { state: Some(state), full_screen: None, frame: Some(frame) }
}

/// Gives the show the settings in effect (Options ▸ Slideshow), changing only what differs.
fn apply_settings(show: &mut Show, wanted: dalica_create::slideshow::Settings) {
    let have = show.settings();
    if have.display_secs != wanted.display_secs {
        show.set_display_secs(wanted.display_secs);
    }
    if have.transition != wanted.transition {
        show.set_transition(wanted.transition);
    }
    if have.looped != wanted.looped {
        show.set_looped(wanted.looped);
    }
}

/// Runs `input` on the show playing (after bringing it up to date), then brings it up to date
/// again; `disabled` when none plays.
fn act(app: &mut App, input: impl FnOnce(&mut Show) -> bool) -> Result<Value, CmdError> {
    if show(app).is_none() {
        return Err(CmdError::Disabled(NOT_PLAYING.into()));
    }
    sync(app);
    with_show(input).ok_or_else(|| CmdError::Disabled(NOT_PLAYING.into()))?;
    sync(app);
    let s = show(app).ok_or_else(|| CmdError::Disabled(NOT_PLAYING.into()))?;
    Ok(json!({ "index": s.index, "photo": s.photo(), "playing": s.playing }))
}

// --- Starting --------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    start: Option<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartSection {
    #[serde(default)]
    section: Option<Section>,
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
}

/// Where a show started from the view shown goes back to.
fn from_view(app: &App) -> ShowFrom {
    match app.state().view {
        View::Edit => ShowFrom::Edit,
        View::Viewer => ShowFrom::Viewer,
        View::Library | View::Slideshow => ShowFrom::Library,
    }
}

/// The focused photo of the library: the selection's anchor when it is selected, else its first.
fn focused(app: &App) -> Option<PhotoId> {
    let sel = &app.state().selection;
    sel.anchor.filter(|a| sel.contains(*a)).or_else(|| sel.ids.first().copied())
}

/// The `slideshow.list` query of a section from a photo.
fn section_query(section: Section, start: Option<PhotoId>) -> Value {
    json!({ "section": section.to_string(), "start": start })
}

/// What `view.slideshow` plays (module docs), as a `slideshow.list` query.
fn what_plays(app: &App, p: &Start) -> Result<Value, CmdError> {
    if let Some(ids) = &p.ids {
        return Ok(json!({ "ids": ids, "start": p.start }));
    }
    let state = app.state();
    let table = Table::of(app).ok_or_else(|| CmdError::Disabled(OPENING.into()))?;
    if state.view == View::Edit {
        let photo = state.edit.photo.ok_or_else(|| CmdError::Disabled(NOTHING_TO_PLAY.into()))?;
        let folder = table.library.photo(photo).map(|ph| Section::Folder(ph.folder));
        let section = table.section_of(photo).or(folder).ok_or_else(|| CmdError::NotFound(format!("no photo {photo}")))?;
        return Ok(section_query(section, Some(p.start.unwrap_or(photo))));
    }
    let tray = crate::tray::photos(state);
    if tray.len() >= 2 {
        return Ok(json!({ "ids": tray, "start": p.start }));
    }
    if let Some(photo) = focused(app)
        && let Some(section) = table.section_of(photo)
    {
        return Ok(section_query(section, Some(p.start.unwrap_or(photo))));
    }
    let shown = |s: Option<Section>| s.filter(|s| table.scope(*s).is_some());
    let section = shown(state.scroll.section).or_else(|| table.current(state)).ok_or_else(|| CmdError::Disabled(NOTHING_TO_PLAY.into()))?;
    Ok(section_query(section, p.start))
}

/// Lists what `query` names and plays it, going back to `from` at the end.
fn play(app: &mut App, query: &Value, from: ShowFrom) -> Result<Value, CmdError> {
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled(OPENING.into()))?;
    let list = session.execute("slideshow.list", query)?;
    let photos: Vec<PhotoId> = serde_json::from_value(list["photos"].clone()).map_err(|e| CmdError::Internal(e.to_string()))?;
    let index = list["start"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
    let Some(s) = (Slideshow { photos, index, from, ..Slideshow::default() }).sanitized() else {
        return Err(CmdError::Disabled(NOTHING_TO_PLAY.into()));
    };
    let reply = json!({ "photos": s.photos.len(), "start": s.index, "videos": list["videos"] });
    let state = app.state_mut();
    ASKED.with(|a| a.set(Some(state.full_screen)));
    state.slideshow = Some(s);
    state.view = View::Slideshow;
    state.peek = None;
    state.full_screen = true;
    sync(app);
    Ok(reply)
}

fn start(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Start = parse_object(params)?;
    can_start(app, params).map_err(CmdError::Disabled)?;
    let query = what_plays(app, &p)?;
    let from = from_view(app);
    play(app, &query, from)
}

fn start_section(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: StartSection = parse_object(params)?;
    can_start_section(app, params).map_err(CmdError::Disabled)?;
    let state = app.state();
    let section = match p.section {
        Some(s) => s,
        None => Table::of(app).and_then(|t| t.current(state)).ok_or_else(|| CmdError::Disabled(NOTHING_TO_PLAY.into()))?,
    };
    let selected = (state.selection.section == Some(section)).then(|| focused(app)).flatten();
    let start = p.ids.as_ref().and_then(|ids| ids.first().copied()).or(selected);
    let from = from_view(app);
    play(app, &section_query(section, start), from)
}

// --- The show's commands ---------------------------------------------------------------------------

fn exit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    if show(app).is_none() {
        return Err(CmdError::Disabled(NOT_PLAYING.into()));
    }
    sync(app);
    let Some(s) = show(app).cloned() else { return Err(CmdError::Disabled(NOT_PLAYING.into())) };
    let photo = s.photo();
    let state = app.state_mut();
    state.slideshow = None;
    state.view = match s.from {
        ShowFrom::Edit if state.edit.photo.is_some() => View::Edit,
        ShowFrom::Viewer if state.viewer.is_some() => View::Viewer,
        _ => View::Library,
    };
    // The show ends: its panes are given back, its textures let go, the window is as it was.
    sync(app);
    crate::panels::slideshow::show::release();
    if let Some(id) = photo.filter(|id| app.session().is_some_and(|s| s.library().photo(*id).is_some())) {
        come_back_to(app, id);
    }
    Ok(json!({ "photo": photo }))
}

/// Selects photo `id` where the show went back to: shown in the edit view, or selected and
/// scrolled into view in the library.
fn come_back_to(app: &mut App, id: PhotoId) {
    let (section, reveal) = match Table::of(app) {
        Some(t) => {
            let section = t.section_of(id);
            (section, section.and_then(|s| t.reveal(s, id)))
        }
        None => (None, None),
    };
    let state = app.state_mut();
    match state.view {
        View::Edit => {
            if state.edit.photo != Some(id) {
                state.edit.close_tools();
                state.edit.zoom = Zoom::Fit;
                state.edit.photo = Some(id);
            }
        }
        View::Library => {
            if let Some(anchor) = reveal {
                state.scroll = anchor;
            }
        }
        View::Slideshow | View::Viewer => return,
    }
    if state.selection.contains(id) && state.selection.ids.len() == 1 {
        state.selection.anchor = Some(id);
    } else {
        commit(state, Selection::new(vec![id], Some(id), section, false));
    }
}

fn next(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    act(app, Show::next_photo)
}

fn previous(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    act(app, Show::previous_photo)
}

fn first(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    act(app, Show::first_photo)
}

fn last(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    act(app, Show::last_photo)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    on: Option<bool>,
}

fn pause(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    act(app, |show| {
        let playing = show.state() == State::Playing;
        // Space toggles (and shows the bar); `on` only moves toward what it asks.
        if p.on.is_none_or(|pause| pause == playing) {
            show.space()
        } else {
            show.mouse_moved();
            false
        }
    })
}

/// Sets the zoom of the show playing (the [`sync`] after it pauses a zoomed show and lets it go on
/// at 1).
fn zoom_by(app: &mut App, change: impl FnOnce(f32) -> f32) -> Result<Value, CmdError> {
    if show(app).is_none() {
        return Err(CmdError::Disabled(NOT_PLAYING.into()));
    }
    sync(app);
    if let Some(s) = app.state_mut().slideshow.as_mut() {
        let z = change(s.zoom);
        s.zoom = if z.is_finite() { z.clamp(1.0, SLIDE_ZOOM_MAX) } else { 1.0 };
        if s.zoom <= 1.0 {
            s.pan = [0.5, 0.5];
        }
    }
    act(app, |_| true)
}

/// Sets the zoom of the show playing to `zoom` (1 fitted … [`SLIDE_ZOOM_MAX`]; the bar's slider
/// and magnifier); `disabled` when none plays.
pub fn set_zoom(app: &mut App, zoom: f32) -> Result<Value, CmdError> {
    zoom_by(app, |_| zoom)
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    zoom_by(app, |z| z * ZOOM_STEP)
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    // Within a step of fitted is fitted (no rounding error left above 1).
    zoom_by(app, |z| if z / ZOOM_STEP < 1.0 + 1e-3 { 1.0 } else { z / ZOOM_STEP })
}

/// Runs the library's command `id` on the photo shown.
fn on_photo_shown(app: &mut App, params: &Value, id: &str) -> Result<Value, CmdError> {
    no_params(params)?;
    let photo = show(app).and_then(Slideshow::photo).ok_or_else(|| CmdError::Disabled(NOT_PLAYING.into()))?;
    app.run(id, &json!({ "ids": [photo] }))
}

fn star(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    on_photo_shown(app, params, "photo.star.toggle")
}

fn rotate_right(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    on_photo_shown(app, params, "photo.rotate.cw")
}

fn rotate_left(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    on_photo_shown(app, params, "photo.rotate.ccw")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetTransition {
    transition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayTime {
    seconds: u32,
}

/// Stores Options ▸ Slideshow's `key` as `value` (`options.set`; no undo step), then gives the show
/// playing the new settings.
fn keep_setting(app: &mut App, key: &str, value: Value) -> Result<(), CmdError> {
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled(OPENING.into()))?;
    session.execute("options.set", &json!({ "values": { key: value } }))?;
    if show(app).is_some() {
        sync(app);
    }
    Ok(())
}

fn transition(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SetTransition = parse(params)?;
    let Some(t) = dalica_engine::settings::Transition::from_id(&p.transition) else {
        let ids: Vec<String> = dalica_engine::settings::Transition::ALL.iter().map(|t| format!("\"{}\"", t.id())).collect();
        return Err(CmdError::BadParams(format!("`transition` is one of {}, not {:?}", ids.join(", "), p.transition)));
    };
    keep_setting(app, "slideshow.transition", json!(t.id()))?;
    Ok(json!({ "transition": t.id() }))
}

fn captions(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    let now = app.session().ok_or_else(|| CmdError::Disabled(OPENING.into()))?.settings().slideshow().captions;
    let on = p.on.unwrap_or(!now);
    keep_setting(app, "slideshow.captions", json!(on))?;
    Ok(json!({ "on": on }))
}

fn display_time(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: DisplayTime = parse(params)?;
    let range = dalica_create::slideshow::DISPLAY_MIN..=dalica_create::slideshow::DISPLAY_MAX;
    if !range.contains(&p.seconds) {
        return Err(CmdError::BadParams(format!("`seconds` is {} … {}, not {}", range.start(), range.end(), p.seconds)));
    }
    keep_setting(app, "slideshow.displayTime", json!(p.seconds))?;
    Ok(json!({ "seconds": p.seconds }))
}
