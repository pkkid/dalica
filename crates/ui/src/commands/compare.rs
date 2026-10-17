//! Compare and before/after in the edit view (`editing.md` §4): `edit.compare {mode}` (the top
//! bar's toggle: `one`, `two` for A│B, `same` for A│A), `edit.switchFocus` (⇄: the other side takes
//! the focus), `edit.switchLayout` (⇅: side by side or one above the other) and
//! `edit.showOriginal {on?}` (`\` held, or Show original pressed and held: the focused side shown
//! unedited).
//!
//! M11.2's stubs: they change only the view state's `edit`. A│B and A│A wait for the preview's
//! second pane and say so, so the toggle's two compare segments stay greyed until M11.16 makes
//! them live; one photo is always there. Show original is a hold: M11.16 turns `\`'s press and
//! release into `on: true` and `on: false`; until then its key toggles it.

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{Compare, CompareLayout, Side};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "edit.compare",
        label: "Compare",
        menu: "",
        keys: &[],
        params: r#"{mode: "one" | "two" | "same"}: one photo, two photos side by side (A│B), or two versions of this photo (A│A)"#,
        run: compare,
    },
    UiCommand {
        id: "edit.switchFocus",
        label: "Switch Focus",
        menu: "",
        keys: &[],
        params: "{}: give the other compare side the focus",
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
        id: "edit.showOriginal",
        label: "Show Original (hold \\)",
        menu: "",
        keys: &[(Context::EditView, "\\")],
        params: "{on?: bool}: show the focused photo unedited (true) or as edited (false); toggles without `on`",
        run: show_original,
    },
];

/// Why the compare modes cannot be chosen yet.
pub const NOT_YET: &str = "Arrives with compare.";
/// Why a command of the edit view cannot run outside it, and why the switches need a compare mode.
pub const NO_PHOTO: &str = "Open a photo in the edit view first.";
pub const NOT_COMPARING: &str = "Only while comparing.";

pub const CHECKS: &[(&str, Check)] =
    &[("edit.compare", can_compare), ("edit.switchFocus", comparing), ("edit.switchLayout", comparing), ("edit.showOriginal", editing)];

fn editing(app: &App, _: &Value) -> Result<(), String> {
    crate::commands::edit_view::open_photo(app).map(|_| ()).ok_or_else(|| NO_PHOTO.into())
}

fn comparing(app: &App, params: &Value) -> Result<(), String> {
    editing(app, params)?;
    if app.state().edit.compare == Compare::One { Err(NOT_COMPARING.into()) } else { Ok(()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mode {
    mode: Compare,
}

/// `edit.compare`'s check for what a click passes: the mode asked for (a click on a segment passes
/// it; none is "one").
fn can_compare(app: &App, params: &Value) -> Result<(), String> {
    editing(app, params)?;
    match params.get("mode").and_then(|m| serde_json::from_value::<Compare>(m.clone()).ok()) {
        Some(Compare::Two | Compare::Same) => Err(NOT_YET.into()),
        _ => Ok(()),
    }
}

fn compare(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Mode = parse(params)?;
    can_compare(app, &json!({ "mode": p.mode })).map_err(CmdError::Disabled)?;
    let edit = &mut app.state_mut().edit;
    edit.compare = p.mode;
    edit.second = None;
    edit.focus = Side::Left;
    Ok(json!({ "compare": p.mode }))
}

fn switch_focus(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    comparing(app, params).map_err(CmdError::Disabled)?;
    let edit = &mut app.state_mut().edit;
    edit.focus = match edit.focus {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    };
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
struct On {
    #[serde(default)]
    on: Option<bool>,
}

fn show_original(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    editing(app, params).map_err(CmdError::Disabled)?;
    let edit = &mut app.state_mut().edit;
    edit.show_original = p.on.unwrap_or(!edit.show_original);
    Ok(json!({ "showOriginal": edit.show_original }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::View;

    #[test]
    fn compare_stays_on_one_photo_and_the_switches_need_a_compare_mode() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "one" })), Err(CmdError::Disabled(m)) if m == NO_PHOTO));
        app.state_mut().edit.photo = Some(ids[0]);
        app.state_mut().view = View::Edit;
        assert_eq!(app.run("edit.compare", &json!({ "mode": "one" })).unwrap(), json!({ "compare": "one" }));
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "two" })), Err(CmdError::Disabled(m)) if m == NOT_YET));
        assert!(matches!(app.run("edit.switchFocus", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_COMPARING));
        // In a compare mode (as M11.16 will set it) the switches flip their state.
        app.state_mut().edit.compare = Compare::Same;
        assert_eq!(app.run("edit.switchFocus", &json!({})).unwrap(), json!({ "focus": "right" }));
        assert_eq!(app.run("edit.switchLayout", &json!({})).unwrap(), json!({ "layout": "stacked" }));
        assert_eq!(app.run("edit.showOriginal", &json!({ "on": true })).unwrap(), json!({ "showOriginal": true }));
        assert_eq!(app.run("edit.showOriginal", &json!({})).unwrap(), json!({ "showOriginal": false }));
        assert!(matches!(app.run("edit.compare", &json!({ "mode": "three" })), Err(CmdError::BadParams(_))));
    }
}
