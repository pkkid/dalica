//! Entering, leaving and moving through the edit view (`editing.md` §1, §12), its tabs and its
//! panel toggle:
//!
//! - `view.edit {id?}`: shows photo `id` (else the focused photo of the selection, else the first
//!   selected) in the edit view; Ctrl+3 and Enter in the library, a double-click on a thumbnail,
//!   View ▸ Edit View, Picture ▸ View and Edit, a click on a filmstrip thumbnail. With nothing
//!   selected it shows the toast "Select a photo to edit." and changes nothing. Ctrl+3 is a Global
//!   key (`menus-and-shortcuts.md` §2.1): in the edit view it keeps the photo shown, its zoom and
//!   its open tool (opening the photo already shown changes nothing).
//! - `view.library`: back to the library (Esc and Ctrl+1 in the edit view, View ▸ Library View,
//!   Back to Library); an open tool is cancelled, never asked about: edits are already stored. The
//!   library keeps its scroll position, unless the photo last viewed is out of sight (it is then
//!   scrolled into view), and its selection. Greyed (and refused) while the library is shown.
//! - `edit.next`, `edit.previous`, `edit.first`, `edit.last` (→ and PgDn, ← and PgUp, Home, End):
//!   step through the photos of the open photo's section in light-table order (hidden photos only
//!   while View ▸ Hidden Pictures shows them); they stay put at the ends.
//! - `edit.history.undo`, `edit.history.redo` (Ctrl+Z, Ctrl+Y / Cmd+Shift+Z) and `edit.autoFix`
//!   (Ctrl+Shift+E): steps of the open photo's own history (`photo.history.*`), never the library
//!   undo stack (`editing.md` §5.1). After a Save, with no edit made since, Undo is Undo Save, as
//!   the Undo button and Edit ▸ Undo read: Ctrl+Z runs `photo.undoSave` on the open photo exactly
//!   when `photo.history.state` answers `undoSave` ([`undoes_save`]).
//! - `edit.panelToggle {on?}`: shows or hides the edit panel (Ctrl+9, View ▸ Show Edit Controls). A
//!   Global key: in the library it sets whether the panel shows the next time the edit view opens.
//! - `edit.tab {tab}`: the edit panel's tab.
//!
//! **Deleting the photo shown** (the caption bar's trash button): [`successors`] lists, before the
//! delete question is asked, where the view goes once the photo is gone; [`left_behind`] goes
//! there: the next photo of its section, else the previous one, else back to the library.
//!
//! **The selection follows the photo shown.** Opening or moving to a photo that is selected makes it
//! the focused one (the selection's anchor); one that is not becomes the selection alone. So the
//! light table, the Photo Tray, the status line and the bottom area's buttons all speak of the photo
//! shown, and back in the library the photo last viewed is the focused thumbnail.
//!
//! [`dispatch`] is the edit view's shortcut dispatch: keys go to the open tool first
//! ([`super::tools::key`]), then to the keymap in the edit view's context. A command that acts on
//! photos (`"ids"`) gets the open photo; the rotations, the flips, Edit ▸ Undo / Redo and the
//! one-click fixes are [`route`]d to the photo's own history; a key bound to an opener (F2,
//! Rename…) opens its dialog for the open photo, as in the library.

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context, Key};
use dalica_ui_kit::widgets::ToastKind;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::selection::{self, Table};
use crate::state::{Compare, EditTab, Section, Selection, View, Zoom};

/// The toast `view.edit` shows with nothing to edit.
pub const NOTHING_SELECTED: &str = "Select a photo to edit.";
/// Why the edit view's own commands refuse outside it.
pub const NOT_EDITING: &str = "No photo is open in the edit view.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "view.edit",
        label: "Edit View",
        menu: "View/Edit View",
        keys: &[(Context::Library, "Ctrl+3"), (Context::Library, "Enter"), (Context::EditView, "Ctrl+3")],
        params: r#"{"id"?: PhotoId, "ids"?: [PhotoId]}: show a photo in the edit view: "id", else the first of "ids", else the focused or first selected photo"#,
        run: view_edit,
    },
    UiCommand {
        id: "view.library",
        label: "Library View",
        menu: "View/Library View",
        keys: &[(Context::EditView, "Ctrl+1"), (Context::EditView, "Esc")],
        params: "{}: back to the library; an open tool is cancelled",
        run: view_library,
    },
    UiCommand {
        id: "edit.next",
        label: "Next Photo",
        menu: "",
        keys: &[(Context::EditView, "Right"), (Context::EditView, "PageDown")],
        params: "{}: the next photo of the open photo's section, in light-table order; stays at the last",
        run: next,
    },
    UiCommand {
        id: "edit.previous",
        label: "Previous Photo",
        menu: "",
        keys: &[(Context::EditView, "Left"), (Context::EditView, "PageUp")],
        params: "{}: the previous photo of the open photo's section, in light-table order; stays at the first",
        run: previous,
    },
    UiCommand {
        id: "edit.first",
        label: "First Photo",
        menu: "",
        keys: &[(Context::EditView, "Home")],
        params: "{}: the first photo of the open photo's section",
        run: first,
    },
    UiCommand {
        id: "edit.last",
        label: "Last Photo",
        menu: "",
        keys: &[(Context::EditView, "End")],
        params: "{}: the last photo of the open photo's section",
        run: last,
    },
    UiCommand {
        id: "edit.history.undo",
        label: "Undo Edit",
        menu: "",
        keys: &[(Context::EditView, "Ctrl+Z")],
        params: "{}: undo the open photo's last edit (photo.history.undo)",
        run: history_undo,
    },
    UiCommand {
        id: "edit.history.redo",
        label: "Redo Edit",
        menu: "",
        keys: &[(Context::EditView, "win,linux:Ctrl+Y"), (Context::EditView, "mac:Ctrl+Shift+Z")],
        params: "{}: redo the open photo's last undone edit (photo.history.redo)",
        run: history_redo,
    },
    UiCommand {
        id: "edit.autoFix",
        label: "Auto Fix",
        menu: "",
        keys: &[(Context::EditView, "Ctrl+Shift+E")],
        params: "{}: Auto Fix on the open photo, one step of its history",
        run: auto_fix,
    },
    UiCommand {
        id: "edit.panelToggle",
        label: "Show Edit Controls",
        menu: "View/Show Edit Controls",
        keys: &[(Context::EditView, "Ctrl+9"), (Context::Library, "Ctrl+9")],
        params: r#"{"on"?: bool}: show or hide the edit panel; toggles without "on""#,
        run: panel_toggle,
    },
    UiCommand {
        id: "edit.tab",
        label: "Edit Tab",
        menu: "",
        keys: &[],
        params: r#"{"tab": "basic" | "tuning" | "effects" | "effects2" | "effects3"}: the edit panel's tab"#,
        run: tab,
    },
];

pub const CHECKS: &[(&str, Check)] = &[
    ("view.library", can_leave),
    ("edit.next", editing),
    ("edit.previous", editing),
    ("edit.first", editing),
    ("edit.last", editing),
    ("edit.history.undo", editing),
    ("edit.history.redo", editing),
    ("edit.autoFix", editing),
];

fn can_leave(app: &App, _params: &Value) -> Result<(), String> {
    if app.state().view == View::Library { Err("The library is already shown.".into()) } else { Ok(()) }
}

fn editing(app: &App, _params: &Value) -> Result<(), String> {
    open_photo(app).map(|_| ()).ok_or_else(|| NOT_EDITING.into())
}

/// The photo open in the edit view; `None` in the library. In A│B it is the focused side's photo
/// (`editing.md` §4: edits apply to the focused side).
pub fn open_photo(app: &App) -> Option<PhotoId> {
    let state = app.state();
    if state.view != View::Edit {
        return None;
    }
    match (state.edit.compare, state.edit.focus, state.edit.second) {
        (Compare::Two, crate::state::Side::Right, Some(second)) if state.edit.photo.is_some() => Some(second),
        _ => state.edit.photo,
    }
}

/// The recipe of the focused side: the open photo's, or A│A's variant's while the right side has
/// the focus (`editing.md` §4); `None` outside the edit view.
pub fn focused_edits(app: &App) -> Option<Vec<String>> {
    let photo = open_photo(app)?;
    let s = app.session()?;
    match s.variant().edits().filter(|_| super::compare::on_variant(app) && s.variant().photo() == Some(photo)) {
        Some(v) => Some(v.to_vec()),
        None => Some(s.library().info(photo).edits.clone()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    id: Option<PhotoId>,
    ids: Option<Vec<PhotoId>>,
}

/// The photo `view.edit` shows for `p`: the one named, else the selection's focused or first photo.
fn target(app: &App, p: &Open) -> Option<PhotoId> {
    let sel = &app.state().selection;
    p.id.or_else(|| p.ids.as_ref().and_then(|ids| ids.first().copied()))
        .or_else(|| sel.anchor.filter(|a| sel.contains(*a)))
        .or_else(|| sel.ids.first().copied())
}

fn view_edit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Open = parse_object(params)?;
    let Some(id) = target(app, &p) else {
        app.toast(NOTHING_SELECTED, ToastKind::Info);
        return Err(CmdError::Disabled(NOTHING_SELECTED.into()));
    };
    let session = app.session().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    if session.library().photo(id).is_none() {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    show(app, id);
    Ok(json!({ "view": View::Edit, "photo": id }))
}

/// Shows photo `id` in the edit view: its tools closed, fitted, one photo; the selection follows
/// it (focused if selected, else selected alone).
fn show(app: &mut App, id: PhotoId) {
    let section = Table::of(app).and_then(|t| t.section_of(id));
    let state = app.state_mut();
    state.view = View::Edit;
    let edit = &mut state.edit;
    if edit.photo != Some(id) {
        edit.close_tools();
        edit.zoom = Zoom::Fit;
    }
    edit.photo = Some(id);
    edit.compare = Compare::One;
    if state.selection.contains(id) {
        state.selection.anchor = Some(id);
    } else {
        // A photo outside every shown section (opened by a script while hidden) has no section.
        selection::commit(state, Selection::new(vec![id], Some(id), section, false));
    }
}

fn view_library(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    can_leave(app, params).map_err(CmdError::Disabled)?;
    // The photo last viewed is in sight when the library shows again.
    let reveal = open_photo(app).and_then(|id| {
        let table = Table::of(app)?;
        table.reveal(table.section_of(id)?, id)
    });
    // Back to one photo too: A│B's compared photo never outlives the photo it is compared with.
    super::compare::one_photo(app);
    let state = app.state_mut();
    state.view = View::Library;
    state.edit.close_tools();
    state.edit.photo = None;
    if let Some(anchor) = reveal {
        state.scroll = anchor;
    }
    Ok(json!({ "view": View::Library }))
}

/// Where the edit view goes once photo `id`, open in it, is deleted: the photos after it in its
/// section, in light-table order, then those before it, nearest first. Asked before the delete
/// question opens, while the photo is still on the light table; empty when no shown section holds
/// it (the view then goes back to the library).
pub fn successors(app: &App, id: PhotoId) -> Vec<PhotoId> {
    let Some(place) = Table::of(app).and_then(|t| place_in(&t, id)) else { return Vec::new() };
    let (before, after) = place.photos.split_at(place.index.min(place.photos.len()));
    after.iter().skip(1).chain(before.iter().rev()).copied().collect()
}

/// Once photo `gone`, open in the edit view, is no longer in the library (moved to the trash), shows
/// the first of `then` ([`successors`]) still in it, else goes back to the library. Returns whether
/// it moved; nothing happens while `gone` is still in the library or is not the photo shown.
pub fn left_behind(app: &mut App, gone: PhotoId, then: &[PhotoId]) -> bool {
    if open_photo(app) != Some(gone) {
        return false;
    }
    let Some(library) = app.session().map(|s| s.library()) else { return false };
    if library.photo(gone).is_some() {
        return false;
    }
    match then.iter().copied().find(|p| library.photo(*p).is_some()) {
        Some(next) => show(app, next),
        None => {
            let _ = view_library(app, &json!({}));
        }
    }
    true
}

/// Where the open photo stands among its section's photos, in light-table order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub section: Section,
    /// The section's photos as the light table shows them.
    pub photos: Vec<PhotoId>,
    /// The open photo's index in `photos`.
    pub index: usize,
}

/// Where photo `id` stands in its section of `table`; `None` when no shown section holds it.
pub fn place_in(table: &Table<'_>, id: PhotoId) -> Option<Place> {
    let section = table.section_of(id)?;
    let scope = table.scope(section)?;
    let index = scope.index(id)?;
    Some(Place { section, photos: scope.photos.to_vec(), index })
}

/// Which photo a step goes to from `index` of `len`: next, previous, first, last; at the ends it
/// stays put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Next,
    Previous,
    First,
    Last,
}

impl Move {
    pub fn from(self, index: usize, len: usize) -> usize {
        let last = len.saturating_sub(1);
        match self {
            Move::Next => index.saturating_add(1).min(last),
            Move::Previous => index.saturating_sub(1).min(last),
            Move::First => 0,
            Move::Last => last,
        }
    }
}

fn step(app: &mut App, params: &Value, m: Move) -> Result<Value, CmdError> {
    no_params(params)?;
    let id = open_photo(app).ok_or_else(|| CmdError::Disabled(NOT_EDITING.into()))?;
    let place =
        Table::of(app).and_then(|t| place_in(&t, id)).ok_or_else(|| CmdError::Disabled("The photo shown is not on the light table.".into()))?;
    let to = place.photos.get(m.from(place.index, place.photos.len())).copied().unwrap_or(id);
    if to != id {
        show(app, to);
    }
    let nth = place.photos.iter().position(|p| *p == to).map_or(0, |i| i.saturating_add(1));
    Ok(json!({ "photo": to, "nth": nth, "of": place.photos.len() }))
}

fn next(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Move::Next)
}

fn previous(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Move::Previous)
}

fn first(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Move::First)
}

fn last(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Move::Last)
}

/// Runs `command` (a `photo.history.*` one) on the open photo with `extra` parameters.
fn on_open_photo(app: &mut App, params: &Value, command: &str, extra: Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let id = open_photo(app).ok_or_else(|| CmdError::Disabled(NOT_EDITING.into()))?;
    let mut p = json!({ "id": id });
    if let (Value::Object(p), Value::Object(e)) = (&mut p, extra) {
        p.extend(e);
    }
    app.run(command, &p)
}

/// Undo on the open photo: Undo Save when [`undoes_save`], else its last edit undone.
fn history_undo(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let id = open_photo(app).ok_or_else(|| CmdError::Disabled(NOT_EDITING.into()))?;
    if undoes_save(app, id) {
        return app.run("photo.undoSave", &json!({ "ids": [id] }));
    }
    on_open_photo(app, params, "photo.history.undo", json!({}))
}

/// Whether photo `id`'s Undo is Undo Save: it is saved and has no edit since, which is when
/// `photo.history.state` answers `undoSave` and the Undo button reads "Undo Save"
/// (`editing.md` §5.1).
pub fn undoes_save(app: &App, id: PhotoId) -> bool {
    let Some(s) = app.session() else { return false };
    s.photo_history(id).is_some_and(|h| h.edits.is_empty()) && s.library().can_undo_save(id)
}

fn history_redo(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    on_open_photo(app, params, "photo.history.redo", json!({}))
}

fn auto_fix(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    on_open_photo(app, params, "photo.history.apply", json!({ "op": "autofix()" }))
}

/// What command `id` with `params` becomes in the edit view (`editing.md` §5.1, §10): the
/// rotations, the flips, Edit ▸ Undo / Redo and the one-click fixes (`edit.apply {op}`) are steps
/// of the open photo's own history, never library steps; Undo after a Save is Undo Save. [`App::run`] asks it first, so a menu
/// row, a shortcut and a script all walk the photo's history. `None` for every other command,
/// outside the edit view, and for `ids` that leave the open photo out (a script naming other
/// photos still gets the library command).
pub fn route(app: &App, id: &str, params: &Value) -> Option<(&'static str, Value)> {
    let photo = open_photo(app)?;
    if let Some(ids) = params.get("ids").and_then(Value::as_array)
        && !ids.iter().any(|v| v.as_u64() == Some(photo.get()))
    {
        return None;
    }
    let apply = |op: &str| Some(("photo.history.apply", json!({ "id": photo, "op": op })));
    match id {
        "photo.rotate.cw" => apply("rotate(90)"),
        "photo.rotate.ccw" => apply("rotate(270)"),
        "edit.flipH" => apply("flip_h()"),
        "edit.flipV" => apply("flip_v()"),
        "edit.undo" if undoes_save(app, photo) => Some(("photo.undoSave", json!({ "ids": [photo] }))),
        "edit.undo" => Some(("photo.history.undo", json!({ "id": photo }))),
        "edit.redo" => Some(("photo.history.redo", json!({ "id": photo }))),
        "edit.apply" => {
            let op = params.get("op")?.as_str()?;
            if op.contains('(') { apply(op) } else { apply(&format!("{op}()")) }
        }
        _ => None,
    }
}

/// The parameters a shortcut passes to `id` in the edit view: a command that acts on photos gets
/// the open photo; the others what the library would pass.
fn params_for(app: &App, id: &str) -> Value {
    let takes_ids = app.session().and_then(|s| s.spec(id)).is_some_and(|spec| spec.params.contains("\"ids\""));
    match open_photo(app) {
        Some(photo) if takes_ids => json!({ "ids": [photo] }),
        _ => crate::keys::params_for(app, id),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OnOff {
    on: Option<bool>,
}

fn panel_toggle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: OnOff = parse_object(params)?;
    let edit = &mut app.state_mut().edit;
    edit.panel_visible = p.on.unwrap_or(!edit.panel_visible);
    Ok(json!({ "panelVisible": edit.panel_visible }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tab {
    tab: EditTab,
}

fn tab(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Tab = parse(params)?;
    app.state_mut().edit.tab = p.tab;
    Ok(json!({ "tab": p.tab }))
}

/// Whether the menu row running `id` is checked, for this module's commands; `None` for others.
pub fn checked(app: &App, id: &str) -> Option<bool> {
    match id {
        "edit.panelToggle" => Some(app.state().edit.panel_visible),
        _ => None,
    }
}

/// The edit view's shortcuts: each key press of the frame goes to the open tool first, then is
/// resolved in the edit view's context (Global under it) and runs its command, [`route`]d to the
/// photo's history where it has a step there, with the open photo as its `ids`; an opener's key
/// opens its dialog for the open photo instead of running the command. While a text field
/// has the focus only Global keys run. Returns the ids that ran.
pub fn dispatch(app: &mut App, ctx: &egui::Context) -> Vec<String> {
    let presses: Vec<Key> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => crate::keys::from_egui(*key, *modifiers),
                _ => None,
            })
            .collect()
    });
    // egui has already dropped the focus of a field Esc ends this frame: the caption field says
    // whether it had it.
    let text_input = ctx.text_edit_focused() || crate::panels::edit::caption_bar::editing(ctx);
    let view = if text_input { Context::Global } else { Context::EditView };
    let mut ran = Vec::new();
    for key in presses {
        if !text_input && let Some(id) = super::tools::key(app, &key) {
            ran.push(id);
            continue;
        }
        let Some(id) = app.session().and_then(|s| s.keymap().resolve(&key, view, text_input)) else { continue };
        match route(app, &id, &json!({})) {
            Some((routed, params)) => {
                let _ = app.run(routed, &params);
                ran.push(routed.to_string());
            }
            None => {
                let params = params_for(app, &id);
                // A key bound to an opener opens its dialog for the open photo, as in the library
                // (`keys::run_key`); a refusal's reason goes to the status line.
                let r = match super::opener(&id) {
                    Some(o) => (o.open)(app, &params),
                    None => app.run(&id, &params),
                };
                if let (Err(e), Some(_)) = (&r, super::opener(&id)) {
                    app.state_mut().status = e.to_string();
                }
                ran.push(id);
            }
        }
    }
    ran
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn view_edit_opens_the_named_or_selected_photo_and_library_leaves() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("view.edit", &json!({})), Err(CmdError::Disabled(m)) if m == NOTHING_SELECTED));
        assert_eq!(app.state().view, View::Library);
        assert_eq!(app.notices().showing(0.0).last().map(|t| t.text.as_str()), Some(NOTHING_SELECTED));
        app.run("select.set", &json!({ "ids": [ids[1].get(), ids[2].get()] })).unwrap();
        app.run("view.edit", &json!({})).unwrap();
        assert_eq!((app.state().view, app.state().edit.photo), (View::Edit, Some(ids[2])), "the focused photo");
        app.run("view.edit", &json!({ "id": ids[1].get() })).unwrap();
        assert_eq!(app.state().edit.photo, Some(ids[1]));
        assert_eq!(
            (app.state().selection.ids.clone(), app.state().selection.anchor),
            (vec![ids[1], ids[2]], Some(ids[1])),
            "a selected photo is focused, the selection kept"
        );
        app.run("edit.tool.open", &json!({ "tool": "crop" })).unwrap();
        app.run("view.library", &json!({})).unwrap();
        assert_eq!(
            (app.state().view, app.state().edit.photo, app.state().edit.tool, app.state().edit.crop.clone()),
            (View::Library, None, None, None)
        );
        assert_eq!(app.state().selection.ids, vec![ids[1], ids[2]], "the selection is kept");
        assert!(matches!(app.run("view.library", &json!({})), Err(CmdError::Disabled(_))), "already there");
        app.run("view.edit", &json!({ "ids": [ids[0].get()] })).unwrap();
        assert_eq!(app.state().edit.photo, Some(ids[0]));
        assert_eq!(
            (app.state().selection.ids.clone(), app.state().selection.anchor),
            (vec![ids[0]], Some(ids[0])),
            "a photo not selected becomes the selection"
        );
    }

    #[test]
    fn edit_view_commands_refuse_bad_params_and_change_nothing() {
        let (_dir, s, _ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        for (id, bad) in [
            ("view.edit", json!({ "id": 999_999 })),
            ("view.edit", json!({ "id": "x" })),
            ("view.edit", json!({ "id": -1 })),
            ("view.edit", json!({ "ids": "x" })),
            ("view.edit", json!({ "zoom": 1 })),
            ("view.edit", json!([1])),
            ("view.library", json!({ "x": 1 })),
            ("edit.panelToggle", json!({ "on": "yes" })),
            ("edit.panelToggle", json!(3)),
            ("edit.tab", json!({})),
            ("edit.tab", json!({ "tab": "effects4" })),
            ("edit.next", json!({ "by": 2 })),
            ("edit.last", json!([])),
            ("edit.history.undo", json!({ "id": 1 })),
            ("edit.autoFix", json!("x")),
        ] {
            let before = app.state().clone();
            assert!(app.run(id, &bad).is_err(), "{id} {bad}");
            assert_eq!((app.state().view, &app.state().edit), (before.view, &before.edit), "{id} {bad}");
        }
        assert_eq!(app.run("edit.panelToggle", &json!({})).unwrap(), json!({ "panelVisible": false }));
        assert_eq!(app.run("edit.panelToggle", &json!({ "on": true })).unwrap(), json!({ "panelVisible": true }));
        assert_eq!(app.run("edit.tab", &json!({ "tab": "tuning" })).unwrap(), json!({ "tab": "tuning" }));
        assert_eq!(checked(&app, "edit.panelToggle"), Some(true));
        assert_eq!(checked(&app, "view.edit"), None);
    }

    #[test]
    fn moves_stay_put_at_the_ends() {
        for (m, k, len, want) in [
            (Move::Next, 0, 3, 1),
            (Move::Next, 2, 3, 2),
            (Move::Previous, 0, 3, 0),
            (Move::Previous, 2, 3, 1),
            (Move::First, 2, 3, 0),
            (Move::Last, 0, 3, 2),
            (Move::Next, 0, 0, 0),
            (Move::Last, 0, 0, 0),
            (Move::Previous, 99, 3, 2),
            (Move::Next, usize::MAX, 3, 2),
        ] {
            assert_eq!(m.from(k, len), want, "{m:?} from {k} of {len}");
        }
    }

    #[test]
    fn the_edit_view_routes_rotations_flips_and_undo_to_the_photo_history() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        assert_eq!(route(&app, "photo.rotate.cw", &json!({})), None, "the library keeps its own");
        for id in ["edit.next", "edit.history.undo", "edit.autoFix"] {
            assert!(matches!(app.run(id, &json!({})), Err(CmdError::Disabled(m)) if m == NOT_EDITING), "{id}");
        }
        app.run("view.edit", &json!({ "id": ids[0].get() })).unwrap();
        let p = ids[0].get();
        for (id, params, want) in [
            ("photo.rotate.cw", json!({}), json!({ "id": p, "op": "rotate(90)" })),
            ("photo.rotate.ccw", json!({}), json!({ "id": p, "op": "rotate(270)" })),
            ("edit.flipH", json!({}), json!({ "id": p, "op": "flip_h()" })),
            ("edit.flipV", json!({}), json!({ "id": p, "op": "flip_v()" })),
            ("edit.apply", json!({ "op": "autofix" }), json!({ "id": p, "op": "autofix()" })),
            ("edit.undo", json!({}), json!({ "id": p })),
            ("edit.redo", json!({}), json!({ "id": p })),
        ] {
            assert_eq!(route(&app, id, &params).map(|r| r.1), Some(want), "{id}");
        }
        assert_eq!(route(&app, "edit.apply", &json!({})), None, "no operation");
        assert_eq!(route(&app, "photo.rotate.cw", &json!({ "ids": [ids[1].get()] })), None, "other photos named");
        assert!(route(&app, "photo.rotate.cw", &json!({ "ids": [p] })).is_some());
        assert_eq!(route(&app, "photo.star.toggle", &json!({})), None);
        assert_eq!(params_for(&app, "photo.star.toggle"), json!({ "ids": [p] }), "the open photo");
        // Ctrl+Shift+E's Auto Fix is a step of the photo's history, undone there.
        app.run("edit.autoFix", &json!({})).unwrap();
        let state = app.run("photo.history.state", &json!({ "id": p })).unwrap();
        assert_eq!(state["edits"], json!(["autofix()"]), "{state}");
        app.run("edit.history.undo", &json!({})).unwrap();
        app.run("edit.history.redo", &json!({})).unwrap();
        app.run("edit.history.undo", &json!({})).unwrap();
        assert_eq!(app.run("photo.history.state", &json!({ "id": p })).unwrap()["edits"], json!([]));
        assert!(app.run("edit.undo", &json!({})).is_err(), "the library stack holds nothing");
    }

    /// After a Save with no edit since, Undo (Ctrl+Z, Edit ▸ Undo, a script's `edit.undo`) is Undo
    /// Save, as `photo.history.state` says; with an edit since, it undoes that edit.
    #[test]
    fn undo_after_a_save_is_undo_save_until_the_next_edit() {
        let (_dir, mut s, ids) = session_with_photos(1);
        let p = ids[0];
        s.execute("photo.history.apply", &json!({ "id": p, "op": "rotate(90)" })).unwrap();
        s.execute("photo.save", &json!({ "ids": [p], "confirm": true })).unwrap();
        s.wait_idle();
        assert!(s.library().can_undo_save(p));
        let mut app = App::new(Some(s));
        assert_eq!(route(&app, "edit.undo", &json!({})), None, "not open: the library's Undo is its own");
        app.run("view.edit", &json!({ "id": p })).unwrap();
        assert_eq!(app.run("photo.history.state", &json!({ "id": p })).unwrap()["undoSave"], json!(true));
        assert!(undoes_save(&app, p));
        assert_eq!(route(&app, "edit.undo", &json!({})), Some(("photo.undoSave", json!({ "ids": [p] }))));
        // An edit since: Undo undoes it.
        app.run("edit.autoFix", &json!({})).unwrap();
        assert!(!undoes_save(&app, p));
        assert_eq!(route(&app, "edit.undo", &json!({})).map(|r| r.0), Some("photo.history.undo"));
        app.run("edit.history.undo", &json!({})).unwrap();
        assert_eq!(app.run("photo.history.state", &json!({ "id": p })).unwrap()["edits"], json!([]));
        // Undo Save again: a save job; the edit comes back unsaved once it has run.
        let reply = app.run("edit.history.undo", &json!({})).unwrap();
        assert_eq!(reply["pending"], json!(true), "{reply}");
        let s = app.session_mut().unwrap();
        s.wait_idle();
        assert!(!s.library().can_undo_save(p) && s.library().has_unsaved_edits(p));
        assert_eq!(s.library().info(p).edits, ["rotate(90)"]);
        assert!(!undoes_save(&app, p));
        assert!(app.run("edit.history.undo", &json!({ "x": 1 })).is_err(), "params are refused before anything runs");
    }
}
