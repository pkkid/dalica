//! Entering and leaving the edit view (`editing.md` §1), its tabs and its panel toggle:
//!
//! - `view.edit {id?}`: shows photo `id` (else the focused photo of the selection, else the first
//!   selected) in the edit view; Ctrl+3 and Enter in the library, View ▸ Edit View, Picture ▸ View
//!   and Edit. With nothing selected it shows the toast "Select a photo to edit." and changes
//!   nothing.
//! - `view.library`: back to the library (Esc and Ctrl+1 in the edit view, View ▸ Library View,
//!   Back to Library); an open tool is cancelled, never asked about: edits are already stored.
//!   Greyed (and refused) while the library is shown.
//! - `edit.panelToggle {on?}`: shows or hides the edit panel (Ctrl+9, View ▸ Show Edit Controls).
//! - `edit.tab {tab}`: the edit panel's tab.
//!
//! [`dispatch`] is the edit view's shortcut dispatch: keys go to the open tool first
//! ([`super::tools::key`]), then to the keymap in the edit view's context.

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context, Key};
use dalica_ui_kit::widgets::ToastKind;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{Compare, EditTab, View, Zoom};

/// The toast `view.edit` shows with nothing to edit.
pub const NOTHING_SELECTED: &str = "Select a photo to edit.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "view.edit",
        label: "Edit View",
        menu: "View/Edit View",
        keys: &[(Context::Library, "Ctrl+3"), (Context::Library, "Enter")],
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
        id: "edit.panelToggle",
        label: "Show Edit Controls",
        menu: "View/Show Edit Controls",
        keys: &[(Context::EditView, "Ctrl+9")],
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

pub const CHECKS: &[(&str, Check)] = &[("view.library", can_leave)];

fn can_leave(app: &App, _params: &Value) -> Result<(), String> {
    if app.state().view == View::Library { Err("The library is already shown.".into()) } else { Ok(()) }
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
    let state = app.state_mut();
    state.view = View::Edit;
    let edit = &mut state.edit;
    edit.close_tools();
    edit.photo = Some(id);
    edit.zoom = Zoom::Fit;
    edit.compare = Compare::One;
    Ok(json!({ "view": View::Edit, "photo": id }))
}

fn view_library(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    can_leave(app, params).map_err(CmdError::Disabled)?;
    let state = app.state_mut();
    state.view = View::Library;
    state.edit.close_tools();
    state.edit.photo = None;
    Ok(json!({ "view": View::Library }))
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
/// resolved in the edit view's context (Global under it) and runs its command, as
/// [`crate::keys::dispatch`] does for the library. While a text field has the focus only Global
/// keys run. Returns the ids that ran.
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
    let text_input = ctx.egui_wants_keyboard_input();
    let view = if text_input { Context::Global } else { Context::EditView };
    let mut ran = Vec::new();
    for key in presses {
        if !text_input && let Some(id) = super::tools::key(app, &key) {
            ran.push(id);
            continue;
        }
        let Some(id) = app.session().and_then(|s| s.keymap().resolve(&key, view, text_input)) else { continue };
        let params = crate::keys::params_for(app, &id);
        let _ = app.run(&id, &params);
        ran.push(id);
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
        assert_eq!((app.state().view, app.state().edit.photo), (View::Edit, app.state().selection.anchor.or(Some(ids[1]))));
        app.run("view.edit", &json!({ "ids": [ids[0].get()] })).unwrap();
        assert_eq!(app.state().edit.photo, Some(ids[0]));
        app.run("edit.tool.open", &json!({ "tool": "crop" })).unwrap();
        app.run("view.library", &json!({})).unwrap();
        assert_eq!(
            (app.state().view, app.state().edit.photo, app.state().edit.tool, app.state().edit.crop.clone()),
            (View::Library, None, None, None)
        );
        assert_eq!(app.state().selection.ids, vec![ids[1], ids[2]], "the selection is kept");
        assert!(matches!(app.run("view.library", &json!({})), Err(CmdError::Disabled(_))), "already there");
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
}
