//! The People pane's commands (`organizing.md` §6.4): the people selected in it (`people.filter`:
//! several filter the light table to the photos holding all of them, with the banner "Showing
//! photos with Ana and Rui" and [Show All]) and **Add a Person Manually** (`faces.addManually`, the
//! edit view only: a box on the photo with "Drag the box to fit the face, then type a name.", whose
//! name stores the face through the engine's `faces.add` as one step of the photo's history).
//!
//! M13.4's stubs: both act on the view state (`peoplePane`); the light table does not filter yet
//! and the box is not drawn yet. M13.24 makes them act. `faces.add` stays the engine's id: this
//! module's command only opens and closes the box.

use dalica_core::PersonId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::{MANUAL_FACE_START, MAX_PEOPLE, ManualFace, View};

/// Why Add a Person Manually cannot start.
pub const EDIT_ONLY: &str = "Open a photo in the edit view to add a person by hand.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "people.filter",
        label: "Filter by People",
        menu: "",
        keys: &[],
        params: "{people: [PersonId]}: select these people in the People pane; several filter the light table to the photos holding all of them; [] clears",
        run: filter,
    },
    UiCommand {
        id: "faces.addManually",
        label: "Add a Person Manually",
        menu: "",
        keys: &[],
        params: "{on?}: in the edit view, show (on) or drop (off) the box to draw around a face; toggles without `on`",
        run: add_manually,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("faces.addManually", in_edit_view)];

fn in_edit_view(app: &App, _: &Value) -> Result<(), String> {
    if app.state().view == View::Edit { Ok(()) } else { Err(EDIT_ONLY.into()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Filter {
    people: Vec<PersonId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    on: Option<bool>,
}

fn filter(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let mut p: Filter = parse(params)?;
    if p.people.len() > MAX_PEOPLE {
        return Err(CmdError::BadParams(format!("`people` names at most {MAX_PEOPLE} people")));
    }
    let mut seen = std::collections::HashSet::new();
    p.people.retain(|x| seen.insert(x.clone()));
    app.state_mut().people_pane.selected = p.people.clone();
    Ok(json!({ "people": p.people }))
}

fn add_manually(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    in_edit_view(app, params).map_err(CmdError::Disabled)?;
    let pane = &mut app.state_mut().people_pane;
    let on = p.on.unwrap_or(pane.manual.is_none());
    match (on, pane.manual.is_some()) {
        (true, false) => pane.manual = Some(ManualFace { rect: MANUAL_FACE_START, name: String::new() }),
        (false, _) => pane.manual = None,
        (true, true) => {}
    }
    Ok(json!({ "manual": pane.manual }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pane_selects_people_and_the_box_needs_the_edit_view() {
        let mut app = App::new(None);
        app.run("people.filter", &json!({ "people": ["p00a1", "p00b2", "p00a1"] })).unwrap();
        assert_eq!(app.state().people_pane.selected.len(), 2, "kept once");
        app.run("people.filter", &json!({ "people": [] })).unwrap();
        assert!(app.state().people_pane.selected.is_empty());
        assert!(matches!(app.run("people.filter", &json!({ "people": ["x"] })), Err(CmdError::BadParams(_))));
        assert_eq!(app.run("faces.addManually", &json!({})), Err(CmdError::Disabled(EDIT_ONLY.into())));
        assert!(app.state().people_pane.manual.is_none());
        app.state_mut().view = View::Edit;
        app.run("faces.addManually", &json!({ "on": true })).unwrap();
        assert_eq!(app.state().people_pane.manual.as_ref().map(|m| m.rect), Some(MANUAL_FACE_START));
        app.run("faces.addManually", &json!({})).unwrap();
        assert!(app.state().people_pane.manual.is_none(), "toggled off");
    }
}
