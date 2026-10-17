//! Compare and before/after in the edit view (`editing.md` §4):
//!
//! - `edit.compare {mode}` (the top bar's toggle): `one`, `two` for A│B, `same` for A│A. A│B
//!   starts with the photo shown on the left and its section neighbour (the next photo, else the
//!   previous one) on the right, which takes the focus (LightCraft's compare: the candidate is the
//!   active photo). A│A opens the photo's variant (`edit.variant.open`) on the right; the left keeps
//!   the focus. Leaving A│B keeps the focused photo shown.
//! - `edit.switchFocus` (⇄, a click on the other side): the other side takes the focus.
//! - `edit.switchLayout` (⇅): side by side, or one above the other.
//! - `edit.comparePhoto {id}` (the filmstrip in A│B): the focused side shows photo `id`; the photo
//!   already on the other side swaps sides with it (LightCraft's `set_photo`).
//! - `edit.showOriginal {on?}`: `\` held, or Show original pressed and held, shows the focused side
//!   unedited; `on` defaults to true (a key press, repeated while held, keeps it on), and the key's
//!   or the button's release sends `on: false` (`panels::edit::compare`, `panels::edit::panel`).
//!
//! **The focused side** is what the edit view works on: in A│B `edit_view::open_photo` is the
//! focused side's photo, so tools, Undo, Redo and the caption bar follow the focus;
//! in A│A the right side is the variant, so [`route`] (asked by `App::run` before anything else)
//! points the open photo's `photo.history.apply`, `.undo`, `.redo`, `.state` and `.tuning` and
//! the Tuning wands and picker there with `"variant": true`. The tool panels (Crop, Straighten,
//! Retouch, Tuning, Fill Light) work on the variant there too (`commands::tools::recipe`,
//! `commands::tools::variant_step`).
//!
//! **The two questions.** Switching the focus or the focused side's photo while a modal tool
//! (Crop, Straighten, Retouch) is open asks "Apply changes to the current photo?"
//! (`dialogs::apply_changes`), or applies at once once "Don't ask again, always apply" was ticked
//! (`compare.askBeforeApplying` off). Leaving A│A while the two sides differ, by the toggle or by
//! leaving the photo (Back to Library, another photo, a slideshow: [`route`]), asks "Keep which
//! version?" (`dialogs::keep_version`), or keeps the side "Don't ask again" stored
//! (`compare.keep`). Each question carries the action that asked (`then`), run once it is
//! answered; Cancel runs nothing.

use dalica_core::PhotoId;
use dalica_engine::settings::KeepVersion;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::edit_view::{self, Move};
use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::commands::tools;
use crate::dialogs::{self, apply_changes, keep_version};
use crate::selection::Table;
use crate::state::{Compare, CompareLayout, Side, View};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "edit.compare",
        label: "Compare",
        menu: "",
        keys: &[],
        params: r#"{mode: "one" | "two" | "same"}: one photo, two photos side by side (A│B: the photo shown and its neighbour, the right side focused), or two versions of this photo (A│A: the photo's variant on the right); leaving A│A with sides that differ asks "Keep which version?" first"#,
        run: compare,
    },
    UiCommand {
        id: "edit.switchFocus",
        label: "Switch Focus",
        menu: "",
        keys: &[],
        params: "{}: give the other compare side the focus (asks to apply an open tool first)",
        run: switch_focus,
    },
    UiCommand {
        id: "edit.switchLayout",
        label: "Switch Layout",
        menu: "",
        keys: &[],
        params: "{}: show the compare sides side by side, or one above the other",
        run: switch_layout,
    },
    UiCommand {
        id: "edit.comparePhoto",
        label: "Compare Photo",
        menu: "",
        keys: &[],
        params: "{id: PhotoId}: in A│B, show photo id on the focused side (the photo on the other side swaps with it)",
        run: compare_photo,
    },
    UiCommand {
        id: "edit.showOriginal",
        label: "Show Original (hold \\)",
        menu: "",
        keys: &[(Context::EditView, "\\")],
        params: "{on?: bool (true)}: show the focused photo unedited (true, while \\ or Show original is held) or as edited (false)",
        run: show_original,
    },
];

/// Why a command of the edit view cannot run outside it, and why the switches need a compare mode.
pub const NO_PHOTO: &str = "Open a photo in the edit view first.";
pub const NOT_COMPARING: &str = "Only while comparing.";
pub const NOT_TWO: &str = "Only while comparing two photos (A│B).";
/// Why A│B cannot start: the section has no other photo.
pub const NO_SECOND: &str = "There is no other photo here to compare with.";

pub const CHECKS: &[(&str, Check)] = &[
    ("edit.compare", editing),
    ("edit.switchFocus", comparing),
    ("edit.switchLayout", comparing),
    ("edit.comparePhoto", comparing_two),
    ("edit.showOriginal", editing),
];

/// The commands a question may run once answered (its `then`).
const THEN: &[&str] = &[
    "edit.compare",
    "edit.switchFocus",
    "edit.comparePhoto",
    "view.library",
    "view.edit",
    "view.slideshow",
    "edit.next",
    "edit.previous",
    "edit.first",
    "edit.last",
];
/// The commands that leave A│A (or step A│B's focused side).
const LEAVES: &[&str] = &["view.library", "view.edit", "view.slideshow", "edit.next", "edit.previous", "edit.first", "edit.last"];

fn editing(app: &App, _: &Value) -> Result<(), String> {
    edit_view::open_photo(app).map(|_| ()).ok_or_else(|| NO_PHOTO.into())
}

fn comparing(app: &App, params: &Value) -> Result<(), String> {
    editing(app, params)?;
    if app.state().edit.compare == Compare::One { Err(NOT_COMPARING.into()) } else { Ok(()) }
}

fn comparing_two(app: &App, params: &Value) -> Result<(), String> {
    editing(app, params)?;
    if app.state().edit.compare == Compare::Two { Ok(()) } else { Err(NOT_TWO.into()) }
}

/// The compare mode in force: the edit state's while the edit view shows a photo, else one photo.
pub fn mode(app: &App) -> Compare {
    let s = app.state();
    if s.view == View::Edit && s.edit.photo.is_some() { s.edit.compare } else { Compare::One }
}

/// A│A's right side has the focus: the edit view works on the variant.
pub fn on_variant(app: &App) -> bool {
    mode(app) == Compare::Same && app.state().edit.focus == Side::Right
}

/// A│A's variant of the photo shown is open and its recipe is not the photo's.
pub fn sides_differ(app: &App) -> bool {
    let (Some(photo), Some(s)) = (app.state().edit.photo, app.session()) else { return false };
    s.variant().photo() == Some(photo) && s.variant().edits().is_some_and(|v| v != s.library().info(photo).edits.as_slice())
}

/// A modal tool (Crop, Straighten, Retouch) is open: its draft is not applied yet.
fn modal_open(app: &App) -> bool {
    tools::open_tool(app).is_some_and(|t| t.modal())
}

/// The photo after `photo` in its section, else the one before it.
fn neighbour(app: &App, photo: PhotoId) -> Option<PhotoId> {
    let table = Table::of(app)?;
    let place = edit_view::place_in(&table, photo)?;
    let after = place.index.checked_add(1).and_then(|i| place.photos.get(i));
    let before = place.index.checked_sub(1).and_then(|i| place.photos.get(i));
    after.or(before).copied()
}

/// What a question runs once answered: `{id, params}`.
fn then_of(id: &str, params: &Value) -> Value {
    json!({ "id": id, "params": params })
}

/// Runs the action a question carries (`then`), when it is one a question may carry.
pub(crate) fn run_then(app: &mut App, then: Option<&Value>) -> Result<Value, CmdError> {
    let Some(t) = then else { return Ok(json!({})) };
    let id = t.get("id").and_then(Value::as_str).unwrap_or_default();
    let Some(id) = THEN.iter().find(|c| **c == id) else { return Ok(json!({})) };
    let params = t.get("params").cloned().unwrap_or_else(|| json!({}));
    app.run(id, &params)
}

/// Before an action that changes the focused side (`id` with `params`) while a modal tool is open:
/// "Apply changes to the current photo?" (`Some`: the question opened), or with "always apply" the
/// tool's Apply at once (`None` when it applied and the action can go on).
fn before_switch(app: &mut App, id: &str, params: &Value) -> Option<Result<Value, CmdError>> {
    if !modal_open(app) {
        return None;
    }
    let ask = app.session().is_none_or(|s| s.settings().compare().ask_before_applying);
    if ask {
        let mut d = apply_changes::dialog();
        d.fields.insert("dontAsk".into(), json!(false));
        d.fields.insert("then".into(), then_of(id, params));
        return Some(dialogs::open(app, d));
    }
    match app.run("edit.tool.apply", &json!({})) {
        Ok(_) => None,
        Err(e) => Some(Err(e)),
    }
}

/// Opens "Keep which version?" for leaving A│A by `id` with `params`.
fn ask_keep(app: &mut App, id: &str, params: &Value) -> Result<Value, CmdError> {
    let mut d = keep_version::dialog();
    d.fields.insert("dontAsk".into(), json!(false));
    d.fields.insert("then".into(), then_of(id, params));
    dialogs::open(app, d)
}

/// Keeps `keep` (`left` or `right`) of A│A's variant (`edit.variant.close`).
pub(crate) fn keep_side(app: &mut App, keep: &str) -> Result<Value, CmdError> {
    app.run("edit.variant.close", &json!({ "keep": keep }))
}

/// Back to one photo: the compare state reset.
pub(crate) fn one_photo(app: &mut App) {
    let edit = &mut app.state_mut().edit;
    edit.compare = Compare::One;
    edit.second = None;
    edit.focus = Side::Left;
}

/// Before leaving A│A by `id` with `params`: with sides that differ, "Keep which version?" (or the
/// side "Don't ask again" stored); with equal sides the variant is dropped. `Some` when the
/// question opened; `None` once back to one photo, so the action goes on.
fn leave_same(app: &mut App, id: &str, params: &Value) -> Option<Result<Value, CmdError>> {
    if mode(app) != Compare::Same {
        return None;
    }
    if sides_differ(app) {
        let keep = app.session().map(|s| s.settings().compare().keep).unwrap_or_default();
        let side = match keep {
            KeepVersion::Ask => return Some(ask_keep(app, id, params)),
            KeepVersion::Left => "left",
            KeepVersion::Right => "right",
        };
        if let Err(e) = keep_side(app, side) {
            // The stored side cannot be kept (the right side would drop an entry Dalica cannot
            // read): say why and ask.
            app.toast(&e.to_string(), dalica_ui_kit::widgets::ToastKind::Caution);
            return Some(ask_keep(app, id, params));
        }
    } else if app.session().is_some_and(|s| s.variant().photo().is_some() && s.variant().photo() == app.state().edit.photo) {
        let _ = keep_side(app, "left");
    }
    one_photo(app);
    None
}

/// Asked by `App::run` before every command (`Some`: this answers it instead). While comparing:
/// leaving A│A asks "Keep which version?" first; in A│B the photo steps (→, ←, Home, End, the
/// filmstrip's arrows) move the focused side; with A│A's right side focused the open photo's
/// history steps, Tuning changes and Tuning analyses go to the variant.
pub fn route(app: &mut App, id: &str, params: &Value) -> Option<Result<Value, CmdError>> {
    let current = mode(app);
    if current == Compare::One {
        return None;
    }
    if LEAVES.contains(&id) {
        if current == Compare::Two {
            let m = match id {
                "edit.next" => Move::Next,
                "edit.previous" => Move::Previous,
                "edit.first" => Move::First,
                "edit.last" => Move::Last,
                _ => return None,
            };
            return Some(step_focused(app, params, m));
        }
        return leave_same(app, id, params);
    }
    if !on_variant(app) {
        return None;
    }
    let photo = app.state().edit.photo?;
    let names_photo = params.get("id").and_then(Value::as_u64) == Some(photo.get());
    match id {
        "photo.history.apply"
        | "photo.history.undo"
        | "photo.history.redo"
        | "photo.history.state"
        | "photo.history.tuning"
        | "photo.tuning.auto"
        | "photo.tuning.pick"
            if names_photo && params.get("variant").is_none() =>
        {
            let mut p = params.clone();
            p.as_object_mut()?.insert("variant".into(), json!(true));
            Some(app.run(id, &p))
        }
        // Undo Save is the photo's; on the variant Undo walks the variant.
        "photo.undoSave" if params.get("ids") == Some(&json!([photo])) => {
            Some(app.run("photo.history.undo", &json!({ "id": photo, "variant": true })))
        }
        _ => None,
    }
}

/// A│B's photo steps: the focused side shows the next, previous, first or last photo of its
/// section (the other side's photo swaps sides when it is reached).
fn step_focused(app: &mut App, params: &Value, m: Move) -> Result<Value, CmdError> {
    no_params(params)?;
    let id = edit_view::open_photo(app).ok_or_else(|| CmdError::Disabled(NO_PHOTO.into()))?;
    let place = Table::of(app)
        .and_then(|t| edit_view::place_in(&t, id))
        .ok_or_else(|| CmdError::Disabled("The photo shown is not on the light table.".into()))?;
    let to = place.photos.get(m.from(place.index, place.photos.len())).copied().unwrap_or(id);
    if to != id {
        compare_photo(app, &json!({ "id": to }))?;
    }
    let nth = place.photos.iter().position(|p| *p == to).map_or(0, |i| i.saturating_add(1));
    Ok(json!({ "photo": to, "nth": nth, "of": place.photos.len() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mode {
    mode: Compare,
}

fn compare(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Mode = parse(params)?;
    editing(app, params).map_err(CmdError::Disabled)?;
    if app.state().edit.compare != p.mode {
        if let Some(r) = before_switch(app, "edit.compare", params) {
            return r;
        }
        if let Some(r) = leave_same(app, "edit.compare", params) {
            return r;
        }
        // Leaving A│B keeps the focused photo shown.
        let shown = edit_view::open_photo(app).ok_or_else(|| CmdError::Disabled(NO_PHOTO.into()))?;
        match p.mode {
            Compare::One => {
                one_photo(app);
                app.state_mut().edit.photo = Some(shown);
            }
            Compare::Two => {
                let second = neighbour(app, shown).ok_or_else(|| CmdError::Disabled(NO_SECOND.into()))?;
                let edit = &mut app.state_mut().edit;
                edit.photo = Some(shown);
                edit.second = Some(second);
                edit.compare = Compare::Two;
                edit.focus = Side::Right;
            }
            Compare::Same => {
                app.run("edit.variant.open", &json!({ "id": shown }))?;
                let edit = &mut app.state_mut().edit;
                edit.photo = Some(shown);
                edit.second = None;
                edit.compare = Compare::Same;
                edit.focus = Side::Left;
            }
        }
        let edit = &mut app.state_mut().edit;
        edit.close_tools();
        edit.show_original = false;
    }
    let edit = &app.state().edit;
    Ok(json!({ "compare": edit.compare, "focus": edit.focus, "photo": edit.photo, "second": edit.second }))
}

fn switch_focus(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    comparing(app, params).map_err(CmdError::Disabled)?;
    if let Some(r) = before_switch(app, "edit.switchFocus", params) {
        return r;
    }
    let edit = &mut app.state_mut().edit;
    edit.focus = match edit.focus {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    };
    // A Tuning session belongs to the side it was on (its changes are stored already).
    edit.close_tools();
    Ok(json!({ "focus": edit.focus }))
}

fn switch_layout(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    comparing(app, params).map_err(CmdError::Disabled)?;
    let edit = &mut app.state_mut().edit;
    edit.layout = match edit.layout {
        CompareLayout::SideBySide => CompareLayout::Stacked,
        CompareLayout::Stacked => CompareLayout::SideBySide,
    };
    Ok(json!({ "layout": edit.layout }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    id: PhotoId,
}

fn compare_photo(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let Pick { id } = parse(params)?;
    comparing_two(app, params).map_err(CmdError::Disabled)?;
    if app.session().and_then(|s| s.library().photo(id)).is_none() {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    if edit_view::open_photo(app) != Some(id) {
        if let Some(r) = before_switch(app, "edit.comparePhoto", params) {
            return r;
        }
        let edit = &mut app.state_mut().edit;
        let (mine, other) = match edit.focus {
            Side::Left => (edit.photo, edit.second),
            Side::Right => (edit.second, edit.photo),
        };
        let other = if other == Some(id) { mine } else { other };
        (edit.photo, edit.second) = match edit.focus {
            Side::Left => (Some(id), other),
            Side::Right => (other, Some(id)),
        };
        edit.close_tools();
    }
    let edit = &app.state().edit;
    Ok(json!({ "photo": edit.photo, "second": edit.second, "focus": edit.focus }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    on: Option<bool>,
}

fn show_original(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    editing(app, params).map_err(CmdError::Disabled)?;
    let edit = &mut app.state_mut().edit;
    edit.show_original = p.on.unwrap_or(true);
    Ok(json!({ "showOriginal": edit.show_original }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing_app(n: usize) -> (tempfile::TempDir, App, Vec<PhotoId>) {
        let (dir, s, ids) = crate::app::tests::session_with_photos(n);
        let mut app = App::new(Some(s));
        app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
        (dir, app, ids)
    }

    #[test]
    fn the_modes_set_their_sides_and_the_switches_need_one() {
        let (_d, s, _) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "one" })), Err(CmdError::Disabled(m)) if m == NO_PHOTO));
        let (_dir, mut app, _) = editing_app(3);
        assert!(matches!(app.run("edit.switchFocus", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_COMPARING));
        let shown = edit_view::open_photo(&app).unwrap();
        let r = app.run("edit.compare", &json!({ "mode": "two" })).unwrap();
        assert_eq!((r["compare"].clone(), r["focus"].clone()), (json!("two"), json!("right")));
        let second = app.state().edit.second.unwrap();
        assert_ne!(second, shown);
        assert_eq!(edit_view::open_photo(&app), Some(second), "the focused side is the photo the edit view works on");
        assert_eq!(app.run("edit.switchFocus", &json!({})).unwrap(), json!({ "focus": "left" }));
        assert_eq!(edit_view::open_photo(&app), Some(shown));
        // The other side's photo swaps sides.
        app.run("edit.comparePhoto", &json!({ "id": second })).unwrap();
        assert_eq!((app.state().edit.photo, app.state().edit.second), (Some(second), Some(shown)));
        assert_eq!(app.run("edit.switchLayout", &json!({})).unwrap(), json!({ "layout": "stacked" }));
        // A│A opens the variant; leaving with equal sides drops it without asking.
        app.run("edit.compare", &json!({ "mode": "same" })).unwrap();
        assert_eq!(app.session().unwrap().variant().photo(), Some(second));
        app.run("edit.compare", &json!({ "mode": "one" })).unwrap();
        assert_eq!(app.session().unwrap().variant().photo(), None);
        assert!(app.state().dialogs.is_empty());
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "three" })), Err(CmdError::BadParams(_))));
    }

    #[test]
    fn show_original_is_on_until_released() {
        let (_dir, mut app, _) = editing_app(1);
        assert_eq!(app.run("edit.showOriginal", &json!({})).unwrap(), json!({ "showOriginal": true }));
        assert_eq!(app.run("edit.showOriginal", &json!({})).unwrap(), json!({ "showOriginal": true }), "a repeated press keeps it on");
        assert_eq!(app.run("edit.showOriginal", &json!({ "on": false })).unwrap(), json!({ "showOriginal": false }));
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "two" })), Err(CmdError::Disabled(m)) if m == NO_SECOND));
    }
}
