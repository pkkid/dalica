//! The Import room's commands (`organizing.md` §3): `import.open` (File ▸ Import From…, Ctrl+M, the
//! toolbar's Import) opens the room as a tab and brings it to the front, and with nothing listed
//! yet lists the first camera card it finds; `import.chooseFolder` is "Import from:" ▸ Folder… (the
//! folder picker, whose answer runs the engine's `import.openSource {path}`); the import tray's
//! selection is `import.select` (a click, Ctrl-click, Shift-click, a group's clock),
//! `import.selectAll` (Ctrl+A) and the arrows (`import.previous` … `import.extendDown`, with Shift
//! the extending forms), all in the Import context; X toggles exclude on the selected items
//! (`import.toggleExclude`) and Enter imports them (`import.importSelected`), the keys of
//! `menus-and-shortcuts.md` §2.4.
//!
//! The destination strip (`organizing.md` §3 items 4–6) drafts where the pictures go in the view
//! state (`state.import.destination`) through `import.destination` ("Import to:", "Folder name:",
//! "After copying:"; `import.chooseDestination` is "Import to:" ▸ Choose…, the folder picker), and
//! its buttons run `import.importAll`, `import.importSelected` (both the engine's `import.run`
//! with the draft: the place drafted, else [`Session::import_folder`]) and `import.cancel` (Cancel:
//! while an import copies, it stops after the current file; otherwise it closes the room). Both
//! import buttons are disabled until a folder name is chosen ([`CHOOSE_NAME`]). When an import the
//! room followed ends, [`finished`] decides what follows: with "Delete copied files from the card"
//! the question first (`crate::dialogs::remove_from_card`), then [`finish`] closes the room and
//! selects the first new folder in the library (its toast comes from the job's end,
//! `crate::notices`); an import cancelled or with failures leaves the room open on its result
//! (`crate::panels::import::progress`).
//!
//! The listing, its choices and the sources are the engine's (`import.sources`,
//! `import.openSource`, `import.list`, `import.exclude`, `import.excludeAll`, `import.mark`,
//! `import.options`); the room's focus, selection and preview are view state (`state.import`), by
//! the candidate ids the listing gives. Up and Down move by the import tray's columns as last drawn
//! (`crate::panels::import::tray::columns`).

use std::collections::HashSet;

use std::path::PathBuf;

use dalica_engine::{CmdError, Context, Session};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse};
use crate::app::App;
use crate::picker::FolderRequest;
use crate::selection::Step;
use crate::state::{AfterCopy, FolderName, ImportDestination, ImportRoom, MAX_DRAFT, MAX_IDS, MAX_LOCATION, Room, Section, Selection};

/// The step commands (the arrows, and with Shift): id, label, key, function, step, extend.
macro_rules! steps {
    ($( $id:literal, $label:literal, $key:literal, $f:ident, $step:expr, $extend:expr; )*) => {
        $(
            fn $f(app: &mut App, params: &Value) -> Result<Value, CmdError> {
                no_params(params)?;
                step(app, $step, $extend)
            }
        )*
        const STEPS: [UiCommand; 8] = [$(UiCommand {
            id: $id,
            label: $label,
            menu: "",
            keys: &[(Context::Import, $key)],
            params: "{}: from the item in the preview (else the focused one); replies {selection, focus, preview}",
            run: $f,
        }),*];
    };
}

steps! {
    "import.previous", "Select Previous Item", "Left", previous, Step::Left, false;
    "import.next", "Select Next Item", "Right", next, Step::Right, false;
    "import.up", "Select Item Above", "Up", up, Step::Up, false;
    "import.down", "Select Item Below", "Down", down, Step::Down, false;
    "import.extendPrevious", "Extend Selection to Previous Item", "Shift+Left", extend_previous, Step::Left, true;
    "import.extendNext", "Extend Selection to Next Item", "Shift+Right", extend_next, Step::Right, true;
    "import.extendUp", "Extend Selection to Item Above", "Shift+Up", extend_up, Step::Up, true;
    "import.extendDown", "Extend Selection to Item Below", "Shift+Down", extend_down, Step::Down, true;
}

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "import.open",
        label: "Import From…",
        menu: "File/Import From…",
        keys: &[(Context::Global, "Ctrl+M")],
        params: r#"{}: open the Import room, or bring it to the front; with no source listed yet, list the first camera card import.sources gives; replies {room: "import"}"#,
        run: open,
    },
    UiCommand {
        id: "import.chooseFolder",
        label: "Folder…",
        menu: "",
        keys: &[],
        params: r#"{}: "Import from:" ▸ Folder…: the folder picker; the folder chosen is listed (import.openSource {path}); replies {dialog: "filePicker"}"#,
        run: choose_folder,
    },
    UiCommand {
        id: "import.select",
        label: "Select in Import",
        menu: "",
        keys: &[],
        params: r#"{"ids": [candidate id], "mode": "replace" | "toggle" | "range" = "replace"}: the import tray's selection, by import.list's ids: replace (a click, a group's clock; the last id is focused and previewed; none clears it), toggle (Ctrl-click), range (Shift-click: from the focused item to the one id given, in capture order); replies {selection, focus, preview}"#,
        run: select,
    },
    UiCommand {
        id: "import.selectAll",
        label: "Select All",
        menu: "",
        keys: &[(Context::Import, "Ctrl+A")],
        params: "{}: select every item listed; replies {selection, focus, preview}",
        run: select_all,
    },
    STEPS[0],
    STEPS[1],
    STEPS[2],
    STEPS[3],
    STEPS[4],
    STEPS[5],
    STEPS[6],
    STEPS[7],
    UiCommand {
        id: "import.toggleExclude",
        label: "Exclude or Include",
        menu: "",
        keys: &[(Context::Import, "X")],
        params: "{}: exclude the Import room's selected items from the import, or include them again when every one is excluded; replies as import.exclude",
        run: toggle_exclude,
    },
    UiCommand {
        id: "import.importSelected",
        label: "Import Selected",
        menu: "",
        keys: &[(Context::Import, "Enter")],
        params: "{}: Import Selected (n): import the Import room's selected items (those not left out) into the destination drafted (import.run with their ids); replies as import.run",
        run: import_selected,
    },
    UiCommand {
        id: "import.importAll",
        label: "Import All",
        menu: "",
        keys: &[],
        params: "{}: Import All: import every item listed that is not left out into the destination drafted (import.run); replies as import.run",
        run: import_all,
    },
    UiCommand {
        id: "import.cancel",
        label: "Cancel",
        menu: "",
        keys: &[],
        params: r#"{}: the destination strip's Cancel: while an import copies, stop it after the current file (what was copied stays; replies {cancelled: [job]}); otherwise close the Import room (replies {room})"#,
        run: cancel,
    },
    UiCommand {
        id: "import.destination",
        label: "Import Destination",
        menu: "",
        keys: &[],
        params: r#"{"place"?: absolute path | null, "folderName"?: text | {"dateTaken": true} | {"today": true} | null, "after"?: "leave" | "delete", "path"?: absolute path}: the destination strip's draft (state.import.destination), only the fields given: "Import to:" (`place`; null: Options ▸ General's "Save imported pictures in"; `path` is the folder picker's answer, read as `place`), "Folder name:" (an empty text is null: none chosen) and "After copying:"; nothing is applied before Import All or Import Selected; replies the draft"#,
        run: set_destination,
    },
    UiCommand {
        id: "import.chooseDestination",
        label: "Choose\u{2026}",
        menu: "",
        keys: &[],
        params: r#"{}: "Import to:" ▸ Choose…: the folder picker; the folder chosen becomes the destination (import.destination {path}); replies {dialog: "filePicker"}"#,
        run: choose_destination,
    },
];

/// Why the room's keys (X, Enter, the arrows, Ctrl+A) do nothing: no source is listed.
pub const NOT_YET: &str = dalica_engine::import::NOTHING_OPEN;
/// Why the import buttons are disabled with no folder name chosen (the hint line's first text).
pub const CHOOSE_NAME: &str = "Choose a folder name to continue";
/// Why they are disabled with no destination known (no Pictures folder, none chosen).
pub const CHOOSE_PLACE: &str = "Choose where to import to.";
/// Why Import Selected (and Enter) does nothing with nothing selected.
pub const SELECT_FIRST: &str = "Select pictures to import.";
/// The destination picker's title.
pub const DESTINATION_TITLE: &str = "Import To";
/// Why X does nothing with nothing selected.
pub const NOTHING_SELECTED: &str = "Select pictures to exclude or include.";
/// The folder picker's title.
pub const FOLDER_TITLE: &str = "Import from Folder";

pub const CHECKS: &[(&str, Check)] = &[
    ("import.toggleExclude", selected_check),
    ("import.importSelected", import_selected_check),
    ("import.importAll", import_check),
    ("import.chooseDestination", pick_check),
    ("import.selectAll", open_check),
    ("import.chooseFolder", pick_check),
    ("import.previous", open_check),
    ("import.next", open_check),
    ("import.up", open_check),
    ("import.down", open_check),
    ("import.extendPrevious", open_check),
    ("import.extendNext", open_check),
    ("import.extendUp", open_check),
    ("import.extendDown", open_check),
];

fn is_open(app: &App) -> bool {
    app.session().is_some_and(|s| s.import_session().source().is_some())
}

fn open_check(app: &App, _: &Value) -> Result<(), String> {
    if is_open(app) { Ok(()) } else { Err(NOT_YET.into()) }
}

/// Whether Import All can run now: a source listed, no import running, a folder name and a
/// destination chosen (the engine refuses while the source is still listed).
fn import_check(app: &App, params: &Value) -> Result<(), String> {
    open_check(app, params)?;
    let Some(s) = app.session() else { return Err(NOT_YET.into()) };
    if let Some(spec) = s.spec("import.run") {
        (spec.enabled)(s, &json!({})).map_err(|d| d.0)?;
    }
    let draft = &app.state().import.destination;
    if draft.folder_name.is_none() {
        return Err(CHOOSE_NAME.into());
    }
    if place(app, draft).is_none() {
        return Err(CHOOSE_PLACE.into());
    }
    Ok(())
}

fn import_selected_check(app: &App, params: &Value) -> Result<(), String> {
    import_check(app, params)?;
    if targets(app).is_empty() { Err(SELECT_FIRST.into()) } else { Ok(()) }
}

/// The folder imported into: the place drafted, else Options ▸ General's "Save imported pictures
/// in" (else the Pictures folder).
pub fn place(app: &App, draft: &ImportDestination) -> Option<PathBuf> {
    draft.place.as_ref().map(PathBuf::from).or_else(|| app.session().and_then(Session::import_folder))
}

/// `import.run`'s parameters for the draft: `ids` given (Import Selected) or all (Import All).
pub fn run_params(destination: &std::path::Path, draft: &ImportDestination, ids: Option<&[u64]>) -> Option<Value> {
    let folder = draft.folder_name.as_ref()?;
    let after = match draft.after {
        AfterCopy::Leave => "leave",
        AfterCopy::Delete => "delete",
    };
    let mut p = json!({ "destination": destination, "folderName": folder, "after": after });
    if let (Some(ids), Some(o)) = (ids, p.as_object_mut()) {
        o.insert("ids".into(), json!(ids));
    }
    Some(p)
}

/// Runs `import.run` for the draft (`ids`: Import Selected's).
fn start(app: &mut App, ids: Option<&[u64]>) -> Result<Value, CmdError> {
    let draft = app.state().import.destination.clone();
    let destination = place(app, &draft).ok_or_else(|| CmdError::Disabled(CHOOSE_PLACE.into()))?;
    let mut params = run_params(&destination, &draft, ids).ok_or_else(|| CmdError::Disabled(CHOOSE_NAME.into()))?;
    // "After copying:" is shown for cards and drives only: a folder's import leaves it as it was.
    if !removable(app)
        && let Some(o) = params.as_object_mut()
    {
        o.remove("after");
    }
    // Over automation the destination keeps to the write roots and the watched folders.
    let watched = app.session().map(|s| s.roots().to_vec()).unwrap_or_default();
    crate::control::refuse("import.run", &params, &watched)?;
    app.run("import.run", &params)
}

/// Whether the source listed is a card or a drive ("After copying:" shows).
pub fn removable(app: &App) -> bool {
    app.session().and_then(|s| s.import_session().source().map(|src| src.kind != dalica_engine::import::SourceKind::Folder)).unwrap_or(false)
}

fn selected_check(app: &App, params: &Value) -> Result<(), String> {
    open_check(app, params)?;
    if targets(app).is_empty() { Err(NOTHING_SELECTED.into()) } else { Ok(()) }
}

fn pick_check(app: &App, _: &Value) -> Result<(), String> {
    if app.session().is_none() {
        return Err("The library is still opening.".into());
    }
    app.can_pick()
}

/// The candidate ids listed so far, in capture order (the tray's order).
pub fn order(app: &App) -> Vec<u64> {
    app.session().map(|s| s.import_session().candidates().iter().map(|c| c.id).collect()).unwrap_or_default()
}

/// The selected items still listed.
fn targets(app: &App) -> Vec<u64> {
    let listed: HashSet<u64> = order(app).into_iter().collect();
    app.state().import.selection.iter().copied().filter(|id| listed.contains(id)).collect()
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let reply = super::rooms::open(app, Room::Import);
    if let Some(s) = app.session_mut()
        && s.import_session().source().is_none()
        && let Ok(sources) = s.execute("import.sources", &json!({}))
        && let Some(card) = crate::panels::import::source::first_card(&sources)
    {
        // A card pulled out meanwhile is no error: the room shows "Import from:" to choose.
        let _ = s.execute("import.openSource", &json!({ "device": card }));
    }
    Ok(reply)
}

fn choose_folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    pick_check(app, params).map_err(CmdError::Disabled)?;
    let request = FolderRequest { title: FOLDER_TITLE, folder: None, command: "import.openSource", params: json!({}), output: false };
    app.pick_folder_for(request).map_err(CmdError::Disabled)?;
    Ok(json!({ "dialog": crate::menus::FILE_PICKER }))
}

/// How [`import.select`](select) changes the selection.
#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Replace,
    Toggle,
    Range,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectParams {
    ids: Vec<u64>,
    #[serde(default)]
    mode: Mode,
}

/// `room` after selecting `ids` with `mode` among the items `order` lists (every id listed):
/// replace selects them (the last focused and previewed; none clears the selection and the focus);
/// toggle flips each (the last given focused, and previewed when now selected); range selects from
/// the focused item (the anchor, kept) to the one id given, in `order`, and previews it.
pub fn selected(room: &ImportRoom, order: &[u64], ids: &[u64], mode: Mode) -> Result<ImportRoom, CmdError> {
    let mut next = room.clone();
    match mode {
        Mode::Replace => {
            let mut seen = HashSet::with_capacity(ids.len());
            next.selection = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
            next.focus = ids.last().copied();
            next.preview = ids.last().copied().or(room.preview);
        }
        Mode::Toggle => {
            for id in ids {
                if next.selection.contains(id) {
                    next.selection.retain(|x| x != id);
                } else {
                    next.selection.push(*id);
                }
            }
            if let Some(&last) = ids.last() {
                next.focus = Some(last);
                if next.selection.contains(&last) {
                    next.preview = Some(last);
                }
            }
        }
        Mode::Range => {
            let [to] = ids[..] else {
                return Err(CmdError::BadParams("a range takes exactly one id: the end it runs to from the focused item".into()));
            };
            let anchor = room.focus.filter(|f| order.contains(f)).unwrap_or(to);
            next.selection = range(order, anchor, to).unwrap_or_else(|| vec![to]);
            next.focus = Some(anchor);
            next.preview = Some(to);
        }
    }
    Ok(next)
}

/// The items of `order` from `from` to `to` (both included), listed from `from`; `None` when either
/// is not listed.
fn range(order: &[u64], from: u64, to: u64) -> Option<Vec<u64>> {
    let a = order.iter().position(|&x| x == from)?;
    let b = order.iter().position(|&x| x == to)?;
    Some(if a <= b { order.get(a..=b)?.to_vec() } else { order.get(b..=a)?.iter().rev().copied().collect() })
}

/// Commits `next` as the room's view state; the reply of the selection commands.
fn commit(app: &mut App, next: ImportRoom) -> Value {
    let reply = json!({ "selection": next.selection, "focus": next.focus, "preview": next.preview });
    app.state_mut().import = next;
    reply
}

fn select(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SelectParams = parse(params)?;
    if p.ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} ids")));
    }
    let order = order(app);
    let listed: HashSet<u64> = order.iter().copied().collect();
    if let Some(missing) = p.ids.iter().find(|id| !listed.contains(id)) {
        return Err(CmdError::NotFound(format!("no candidate {missing} in this listing")));
    }
    let next = selected(&app.state().import, &order, &p.ids, p.mode)?;
    Ok(commit(app, next))
}

fn select_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    open_check(app, params).map_err(CmdError::Disabled)?;
    let order = order(app);
    let mut next = app.state().import.clone();
    let keep = |x: Option<u64>| x.filter(|id| order.contains(id));
    next.focus = keep(next.focus).or_else(|| order.first().copied());
    next.preview = keep(next.preview).or(next.focus);
    next.selection = order;
    Ok(commit(app, next))
}

/// `room` after an arrow (`extend`: with Shift) among `order` in a tray of `cols` columns: from
/// the item in the preview (else the focused one; with neither, the first item is the one reached).
/// A plain arrow selects, focuses and previews the item it reaches; with Shift the selection runs
/// from the focused item (kept) to it.
pub fn stepped(room: &ImportRoom, order: &[u64], cols: usize, step: Step, extend: bool) -> ImportRoom {
    let mut next = room.clone();
    let at = |id: Option<u64>| id.and_then(|i| order.iter().position(|x| *x == i));
    let from = at(room.preview).or_else(|| at(room.focus));
    let to = match from {
        Some(k) => step.from(k, order.len(), cols),
        None => 0,
    };
    let Some(&target) = order.get(to) else { return next };
    if extend {
        let anchor = room.focus.filter(|f| order.contains(f)).or_else(|| from.and_then(|k| order.get(k).copied())).unwrap_or(target);
        next.selection = range(order, anchor, target).unwrap_or_else(|| vec![target]);
        next.focus = Some(anchor);
    } else {
        next.selection = vec![target];
        next.focus = Some(target);
    }
    next.preview = Some(target);
    next
}

fn step(app: &mut App, step: Step, extend: bool) -> Result<Value, CmdError> {
    open_check(app, &Value::Null).map_err(CmdError::Disabled)?;
    let order = order(app);
    let cols = crate::panels::import::tray::columns();
    let next = stepped(&app.state().import, &order, cols, step, extend);
    Ok(commit(app, next))
}

fn toggle_exclude(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    selected_check(app, params).map_err(CmdError::Disabled)?;
    let ids = targets(app);
    let excluded: HashSet<u64> =
        app.session().map(|s| s.import_session().candidates().into_iter().filter(|c| c.excluded).map(|c| c.id).collect()).unwrap_or_default();
    let on = !ids.iter().all(|id| excluded.contains(id));
    app.run("import.exclude", &json!({ "ids": ids, "on": on }))
}

fn import_selected(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    import_selected_check(app, params).map_err(CmdError::Disabled)?;
    let ids = targets(app);
    start(app, Some(&ids))
}

fn import_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    import_check(app, params).map_err(CmdError::Disabled)?;
    start(app, None)
}

/// The import job copying now (not a removal from the card, not an undo), if one is.
pub fn copying(app: &App) -> Option<dalica_engine::JobId> {
    let s = app.session()?;
    let job = s.import_run().job()?;
    s.jobs().into_iter().find(|j| j.job == job && !j.state.is_finished()).map(|j| j.job)
}

fn cancel(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    if let Some(job) = copying(app) {
        let cancelled = match app.session_mut() {
            Some(s) => s.cancel_jobs(Some(job))?,
            None => Vec::new(),
        };
        return Ok(json!({ "cancelled": cancelled }));
    }
    Ok(close_room(app))
}

/// Closes the Import room (never asking: nothing is copying, or the copying is over) and brings
/// the library to the front; replies `{room}`.
pub fn close_room(app: &mut App) -> Value {
    let state = app.state_mut();
    state.rooms.retain(|r| *r != Room::Import);
    if state.room == Room::Import {
        state.room = Room::Library;
    }
    json!({ "room": state.room.name() })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DestinationParams {
    #[serde(default, deserialize_with = "some")]
    place: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    folder_name: Option<Option<FolderName>>,
    after: Option<AfterCopy>,
    path: Option<String>,
}

/// A field given (even as `null`) is `Some`; one left out is `None`.
fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// The draft `draft` after `import.destination` with `params`.
fn drafted(draft: &ImportDestination, params: &Value) -> Result<ImportDestination, CmdError> {
    let p: DestinationParams = parse(params)?;
    let mut next = draft.clone();
    let place = match (p.place, p.path) {
        (Some(_), Some(_)) => return Err(CmdError::BadParams("give `place` or `path`, not both".into())),
        (Some(place), None) => Some(place),
        (None, Some(path)) => Some(Some(path)),
        (None, None) => None,
    };
    if let Some(place) = place {
        if let Some(p) = &place {
            if p.len() > MAX_LOCATION {
                return Err(CmdError::Limit(format!("`place` is at most {MAX_LOCATION} bytes")));
            }
            if !std::path::Path::new(p).is_absolute() {
                return Err(CmdError::BadParams(format!("`place` is an absolute folder path, not {p:?}")));
            }
        }
        next.place = place;
    }
    if let Some(name) = p.folder_name {
        next.folder_name = match name {
            Some(FolderName::Text(t)) if t.chars().count() > MAX_DRAFT => {
                return Err(CmdError::Limit(format!("`folderName` is at most {MAX_DRAFT} characters")));
            }
            Some(FolderName::Text(t)) if t.trim().is_empty() => None,
            other => other,
        };
    }
    if let Some(after) = p.after {
        next.after = after;
    }
    Ok(next)
}

fn set_destination(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let next = drafted(&app.state().import.destination, params)?;
    let reply = serde_json::to_value(&next).map_err(|e| CmdError::Internal(e.to_string()))?;
    app.state_mut().import.destination = next;
    Ok(reply)
}

fn choose_destination(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    pick_check(app, params).map_err(CmdError::Disabled)?;
    let folder = place(app, &app.state().import.destination);
    let request = FolderRequest { title: DESTINATION_TITLE, folder, command: "import.destination", params: json!({}), output: true };
    app.pick_folder_for(request).map_err(CmdError::Disabled)?;
    Ok(json!({ "dialog": crate::menus::FILE_PICKER }))
}

/// What follows an import the room followed, once it ended (`status`: `import.status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// Cancelled, or some files failed: the room stays open on the result.
    Stay,
    /// "Delete copied files from the card": ask first ([`crate::dialogs::remove_from_card`]), then
    /// finish with this folder.
    Ask { count: u64, folder: Option<PathBuf> },
    /// Close the room and select this folder (the first the import copied into).
    Finish(Option<PathBuf>),
}

/// What follows the import `status` (`import.status`) reports ended.
pub fn finished(status: &Value) -> Ending {
    let folder = status["folders"].get(0).and_then(Value::as_str).map(PathBuf::from);
    let imported = status["imported"].as_u64().unwrap_or(0);
    let failed = status["failed"].as_array().is_some_and(|f| !f.is_empty());
    if status["state"] != "done" || failed {
        return Ending::Stay;
    }
    if status["after"] == "delete" && status["removable"] == true && imported > 0 {
        return Ending::Ask { count: imported, folder };
    }
    Ending::Finish(folder)
}

/// Finishing (`organizing.md` §3): the room closes, and the library selects `folder` as a unit
/// and scrolls to it (when the library holds it).
pub fn finish(app: &mut App, folder: Option<&std::path::Path>) {
    close_room(app);
    let Some(path) = folder else { return };
    let Some(session) = app.session() else { return };
    let library = session.library();
    let Some(id) = library.folder_by_path(path) else { return };
    let photos: Vec<_> = library.photos_in(id).iter().map(|p| p.id).collect();
    let section = Section::Folder(id);
    let state = app.state_mut();
    state.view = crate::state::View::Library;
    crate::selection::commit(state, Selection::new(photos.clone(), photos.first().copied(), Some(section), true));
    state.scroll = crate::state::ScrollAnchor { section: Some(section), offset: 0.0 };
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
        assert!(matches!(app.run("import.toggleExclude", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_YET));
        assert!(matches!(app.run("import.importSelected", &json!({ "ids": [1] })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("import.importAll", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_YET));
        // Cancel with nothing copying closes the room.
        assert_eq!(app.run("import.cancel", &json!({})).unwrap(), json!({ "room": "library" }));
        assert_eq!(app.state().rooms, [Room::Library]);
    }

    #[test]
    fn the_draft_changes_only_the_fields_given_and_refuses_hostile_ones() {
        let d = ImportDestination::default();
        let d = drafted(&d, &json!({ "folderName": { "dateTaken": true } })).unwrap();
        assert_eq!((d.place.clone(), d.folder_name.clone(), d.after), (None, Some(FolderName::DateTaken), AfterCopy::Leave));
        // An absolute folder on this system (`/media/out` is relative on Windows: no drive).
        let out = if cfg!(windows) { r"D:\media\out" } else { "/media/out" };
        let d = drafted(&d, &json!({ "after": "delete", "path": out })).unwrap();
        assert_eq!((d.place.as_deref(), d.after), (Some(out), AfterCopy::Delete));
        assert_eq!(d.folder_name, Some(FolderName::DateTaken), "kept");
        // An empty name is none chosen; null puts the default place back.
        let d = drafted(&d, &json!({ "folderName": "  ", "place": null })).unwrap();
        assert_eq!((d.place.clone(), d.folder_name.clone()), (None, None));
        for bad in [
            json!({ "place": "relative/dir" }),
            json!({ "place": "/a", "path": "/b" }),
            json!({ "folderName": { "today": false } }),
            json!({ "after": "erase" }),
            json!({ "extra": 1 }),
            json!([]),
        ] {
            assert!(matches!(drafted(&d, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(drafted(&d, &json!({ "folderName": "x".repeat(MAX_DRAFT + 1) })), Err(CmdError::Limit(_))));
    }

    #[test]
    fn an_ended_import_finishes_asks_or_stays() {
        let done = json!({ "state": "done", "after": "leave", "removable": true, "imported": 3, "folders": ["/p/2026-06-04", "/p/2026-06-05"], "failed": [] });
        assert_eq!(finished(&done), Ending::Finish(Some(PathBuf::from("/p/2026-06-04"))));
        let mut ask = done.clone();
        ask["after"] = json!("delete");
        assert_eq!(finished(&ask), Ending::Ask { count: 3, folder: Some(PathBuf::from("/p/2026-06-04")) });
        ask["removable"] = json!(false);
        assert_eq!(finished(&ask), Ending::Finish(Some(PathBuf::from("/p/2026-06-04"))), "a folder source is never asked about");
        let mut cancelled = done.clone();
        cancelled["state"] = json!("cancelled");
        assert_eq!(finished(&cancelled), Ending::Stay);
        let mut failed = done;
        failed["failed"] = json!([{ "file": "/c/IMG_1.JPG", "reason": "unreadable" }]);
        assert_eq!(finished(&failed), Ending::Stay);
        assert_eq!(finished(&json!(null)), Ending::Stay);
    }

    fn room(focus: Option<u64>, selection: &[u64], preview: Option<u64>) -> ImportRoom {
        ImportRoom { focus, selection: selection.to_vec(), preview, ..ImportRoom::default() }
    }

    #[test]
    fn clicks_select_toggle_and_run_a_range_from_the_focused_item() {
        let order = [10, 11, 12, 13, 14];
        let r = selected(&ImportRoom::default(), &order, &[12, 12], Mode::Replace).unwrap();
        assert_eq!(r, room(Some(12), &[12], Some(12)));
        let r = selected(&r, &order, &[14], Mode::Toggle).unwrap();
        assert_eq!(r, room(Some(14), &[12, 14], Some(14)));
        // Toggling one off keeps the preview where it was.
        let r = selected(&r, &order, &[12], Mode::Toggle).unwrap();
        assert_eq!(r, room(Some(12), &[14], Some(14)));
        // Shift-click: from the focused item, which stays the anchor.
        let r = selected(&room(Some(13), &[13], Some(13)), &order, &[10], Mode::Range).unwrap();
        assert_eq!(r, room(Some(13), &[13, 12, 11, 10], Some(10)));
        assert!(matches!(selected(&r, &order, &[10, 11], Mode::Range), Err(CmdError::BadParams(_))));
        // Nothing given clears the selection and the focus.
        assert_eq!(selected(&r, &order, &[], Mode::Replace).unwrap(), room(None, &[], Some(10)));
    }

    #[test]
    fn the_arrows_move_from_the_preview_by_the_trays_columns() {
        let order = [1, 2, 3, 4, 5, 6, 7];
        // Nothing yet: any arrow reaches the first item.
        assert_eq!(stepped(&ImportRoom::default(), &order, 3, Step::Down, false), room(Some(1), &[1], Some(1)));
        let at2 = room(Some(2), &[2], Some(2));
        assert_eq!(stepped(&at2, &order, 3, Step::Down, false), room(Some(5), &[5], Some(5)));
        assert_eq!(stepped(&at2, &order, 3, Step::Right, false), room(Some(3), &[3], Some(3)));
        assert_eq!(stepped(&at2, &order, 3, Step::Up, false), at2, "the first row stops Up");
        // Shift: the range from the focused item.
        let r = stepped(&at2, &order, 3, Step::Down, true);
        assert_eq!(r, room(Some(2), &[2, 3, 4, 5], Some(5)));
        assert_eq!(stepped(&r, &order, 3, Step::Left, true), room(Some(2), &[2, 3, 4], Some(4)));
        // Hostile: an empty listing, no columns.
        assert_eq!(stepped(&at2, &[], 0, Step::Down, false), at2);
        assert_eq!(stepped(&at2, &order, 0, Step::Down, false), room(Some(3), &[3], Some(3)));
    }
}
