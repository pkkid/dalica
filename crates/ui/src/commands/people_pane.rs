//! The People pane's commands (`organizing.md` §6.4): the people selected in it (`people.filter`)
//! and **Add a Person Manually** (`faces.addManually`, the edit view only).
//!
//! - **`people.filter {people}`** selects people in the pane (a click selects one, Ctrl-click adds
//!   or drops one). Any selected filter the light table to the photos holding a face of every one
//!   of them, by id (the engine search's exact `people` filter, M13.48: selecting Ana never finds
//!   Anabel's photos); the search field stays as typed (the filter combines with it), and the
//!   filter banner says "Showing photos with Ana and Rui" ([`banner`]) with [Show All], which
//!   clears the filter and the search. `[]` clears the selection and the filter.
//! - **`faces.addManually {on?, name?}`** shows (on) or drops (off) the box drawn round a face on
//!   the photo in the edit view (`peoplePane.manual`; toggles without `on`). With `name` (or Enter
//!   in the box's name field) the box is stored ([`store_manual`]): its rectangle, as shown on the
//!   photo, is taken back to the stored frame through the photo's geometry (its rotations, flips,
//!   straighten and crop), and the face is added through the engine's `faces.add` as that person's
//!   in **one** library undo step: a name nobody has makes the person in the same step
//!   (`faces.add` then `faces.name {create}`, grouped). `faces.add` stays the engine's id.
//! - **Naming a face from the pane** ([`name_face`]): the engine's `faces.name {faces, name}`, one
//!   undo step; a name nobody has yet opens New Person (Name prefilled) instead, as in the Unnamed
//!   grid. Its × ignores the face (`faces.ignore`, one undo step).

use dalica_core::{PersonId, PhotoId};
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::{FaceRef, MANUAL_FACE_START, MAX_DRAFT, MAX_PEOPLE, ManualFace, View};

/// Why Add a Person Manually cannot start.
pub const EDIT_ONLY: &str = "Open a photo in the edit view to add a person by hand.";
/// Why the box cannot be stored without a name.
pub const NAME_FIRST: &str = "Type the person's name first.";
/// Why there is no box to store.
pub const NO_BOX: &str = "Choose Add a Person Manually first.";
/// The undo step of a face added by hand.
pub const ADD_STEP: &str = "Add a Person Manually";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "people.filter",
        label: "Filter by People",
        menu: "",
        keys: &[],
        params: "{people: [PersonId]}: select these people in the People pane; the light table shows only the photos holding a face of every one of them, by id (the engine search's people filter; the search field stays as typed; the banner \"Showing photos with Ana and Rui\"); [] clears the selection and the filter",
        run: filter,
    },
    UiCommand {
        id: "faces.addManually",
        label: "Add a Person Manually",
        menu: "",
        keys: &[],
        params: "{on?, name?}: in the edit view, show (on) or drop (off) the box to draw around a face; toggles without `on`; with `name`, stores the box as that person's face (a new name makes the person) in one undo step and drops it",
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
struct Manual {
    #[serde(default)]
    on: Option<bool>,
    #[serde(default)]
    name: Option<String>,
}

/// The names of `people` the library has, in order.
pub fn names(app: &App, people: &[PersonId]) -> Vec<String> {
    let Some(session) = app.session() else { return Vec::new() };
    people.iter().filter_map(|id| session.library().person(id)).map(|p| p.name.clone()).collect()
}

/// "Ana", "Ana and Rui", "Ana, Rui and Eva".
pub fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What the filter banner says while the light table shows the selected people's photos ("Showing
/// photos with Ana and Rui"), or `None` when no one the library has is selected.
pub fn banner(app: &App) -> Option<String> {
    let names = names(app, &app.state().people_pane.selected);
    (!names.is_empty()).then(|| format!("Showing photos with {}", listed(&names)))
}

/// Selects `people` in the pane and filters the light table to their photos (see the module docs):
/// the engine's search takes them at once when a library is open. Replies `{people}`.
pub fn select_people(app: &mut App, people: Vec<PersonId>) -> Value {
    app.state_mut().people_pane.selected = people.clone();
    if app.session().is_some() {
        let _ = super::search::push(app);
    }
    json!({ "people": people })
}

fn filter(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let mut p: Filter = parse(params)?;
    if p.people.len() > MAX_PEOPLE {
        return Err(CmdError::BadParams(format!("`people` names at most {MAX_PEOPLE} people")));
    }
    let mut seen = std::collections::HashSet::new();
    p.people.retain(|x| seen.insert(x.clone()));
    if let Some(s) = app.session()
        && let Some(unknown) = p.people.iter().find(|id| s.library().person(id).is_none())
    {
        return Err(CmdError::NotFound(format!("no person {unknown}")));
    }
    Ok(select_people(app, p.people))
}

fn add_manually(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Manual = parse_object(params)?;
    in_edit_view(app, params).map_err(CmdError::Disabled)?;
    if let Some(name) = p.name {
        if p.on == Some(false) {
            return Err(CmdError::BadParams("`name` stores the box: it goes without `on: false`".into()));
        }
        if app.state().people_pane.manual.is_none() {
            return Err(CmdError::Disabled(NO_BOX.into()));
        }
        return store_manual(app, &name);
    }
    let pane = &mut app.state_mut().people_pane;
    let on = p.on.unwrap_or(pane.manual.is_none());
    match (on, pane.manual.is_some()) {
        (true, false) => pane.manual = Some(ManualFace { rect: MANUAL_FACE_START, name: String::new() }),
        (false, _) => pane.manual = None,
        (true, true) => {}
    }
    Ok(json!({ "manual": pane.manual }))
}

/// Stores Add a Person Manually's box on the photo in the edit view as `name`'s face, in one
/// library undo step (a new name makes the person in that step), and drops the box. Replies
/// `{face, person, created}`.
pub fn store_manual(app: &mut App, name: &str) -> Result<Value, CmdError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CmdError::BadParams(NAME_FIRST.into()));
    }
    if name.chars().count() > MAX_DRAFT {
        return Err(CmdError::Limit(format!("a name is at most {MAX_DRAFT} characters")));
    }
    let Some(manual) = app.state().people_pane.manual.clone() else { return Err(CmdError::Disabled(NO_BOX.into())) };
    let photo = crate::commands::edit_view::open_photo(app).ok_or_else(|| CmdError::Disabled(EDIT_ONLY.into()))?;
    let rect = crate::panels::edit::faces::stored_rect(app, photo, manual.rect)
        .ok_or_else(|| CmdError::BadParams("the box does not lie on the photo".into()))?;
    let Some(session) = app.session_mut() else { return Err(CmdError::Disabled("The library is still opening.".into())) };
    let lib = session.library();
    let index = lib.faces(photo).len();
    let face = format!("{photo}.{index}");
    let rect = json!([rect.x0(), rect.y0(), rect.x1(), rect.y1()]);
    let (person, created) = match lib.person_by_name(name).cloned() {
        Some(person) => {
            session.execute("faces.add", &json!({ "id": photo, "rect": rect, "person": person }))?;
            (json!(person), false)
        }
        None => {
            let steps =
                [("faces.add", json!({ "id": photo, "rect": rect })), ("faces.name", json!({ "faces": [face], "name": name, "create": true }))];
            let out = session.execute_group(&steps, ADD_STEP)?;
            (out.get(1).and_then(|r| r.get("person")).cloned().unwrap_or(Value::Null), true)
        }
    };
    app.state_mut().people_pane.manual = None;
    Ok(json!({ "face": face, "person": person, "created": created }))
}

/// Names `face` `name` from the pane (`faces.name`, one undo step). A name nobody has opens New
/// Person for the face, its Name prefilled, and replies `{dialog: "newPerson"}`.
pub fn name_face(app: &mut App, face: FaceRef, name: &str) -> Result<Value, CmdError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CmdError::BadParams(NAME_FIRST.into()));
    }
    match app.run("faces.name", &json!({ "faces": [face.to_string()], "name": name })) {
        Err(CmdError::Disabled(why)) if why == dalica_engine::cmd::faces::NEEDS_PERSON => {
            let r = crate::dialogs::open(app, crate::dialogs::new_person::dialog(name, &[face]));
            if r.is_ok() {
                app.state_mut().status.clear();
            }
            r
        }
        other => other,
    }
}

/// Ignores `face` from the pane's × (`faces.ignore`, one undo step).
pub fn ignore_face(app: &mut App, face: FaceRef) -> Result<Value, CmdError> {
    app.run("faces.ignore", &json!({ "faces": [face.to_string()] }))
}

/// The photos the pane follows: the photo in the edit view, else the selection.
pub fn subject(app: &App) -> Vec<PhotoId> {
    match crate::commands::edit_view::open_photo(app) {
        Some(photo) => vec![photo],
        None if app.state().view == View::Edit => Vec::new(),
        None => app.state().selection.ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pane_selects_people_and_the_box_needs_the_edit_view() {
        let mut app = App::new(None);
        app.run("people.filter", &json!({ "people": ["p00a1", "p00b2", "p00a1"] })).unwrap();
        assert_eq!(app.state().people_pane.selected.len(), 2, "kept once");
        assert_eq!(app.state().search.text, "", "the search field is left as typed");
        app.run("people.filter", &json!({ "people": [] })).unwrap();
        assert!(app.state().people_pane.selected.is_empty());
        assert!(matches!(app.run("people.filter", &json!({ "people": ["x"] })), Err(CmdError::BadParams(_))));
        assert_eq!(app.run("faces.addManually", &json!({})), Err(CmdError::Disabled(EDIT_ONLY.into())));
        assert!(app.state().people_pane.manual.is_none());
        app.state_mut().view = View::Edit;
        app.run("faces.addManually", &json!({ "on": true })).unwrap();
        assert_eq!(app.state().people_pane.manual.as_ref().map(|m| m.rect), Some(MANUAL_FACE_START));
        assert!(matches!(app.run("faces.addManually", &json!({ "on": false, "name": "Ana" })), Err(CmdError::BadParams(_))));
        app.run("faces.addManually", &json!({})).unwrap();
        assert!(app.state().people_pane.manual.is_none(), "toggled off");
        assert_eq!(app.run("faces.addManually", &json!({ "name": "Ana" })), Err(CmdError::Disabled(NO_BOX.into())));
    }

    #[test]
    fn names_are_listed() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(listed(&s(&[])), "");
        assert_eq!(listed(&s(&["Ana"])), "Ana");
        assert_eq!(listed(&s(&["Ana", "Rui"])), "Ana and Rui");
        assert_eq!(listed(&s(&["Ana", "Rui", "Eva"])), "Ana, Rui and Eva");
    }
}
