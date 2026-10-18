//! The Unnamed grid and the face menu (`organizing.md` §6.2–§6.3, `menus-and-shortcuts.md` §4):
//! naming a group from its "Add a name" field, ignoring a group (its ×) or the selected faces
//! (Ignore Face, Delete), moving faces to a new or an existing person and back to Unnamed, and the
//! header's [Show Ignored Faces] and [Expand Groups].
//!
//! - **The faces a command acts on** are its `faces` (each `"<PhotoId>.<n>"`), else the faces
//!   selected in the grid (`unnamed.selection`); a group's are every face of Unnamed group `n` as
//!   the grid numbers them now (`crate::panels::faces::model`), shown or not.
//! - **Naming** (`faces.nameGroup`) takes the name given, else the one typed under the group: a
//!   name a person has (ignoring case) names the faces as that person's through the engine's
//!   `faces.name`, one library undo step; a name nobody has opens New Person, Name prefilled
//!   (`crate::dialogs::new_person`), whose OK makes the person and names the faces as one step.
//! - **Ignoring** (`faces.ignoreGroup`, `faces.ignoreSelected`) asks "Ignore these faces?" first
//!   unless `"confirm": true` is sent or "Don't ask again" was ticked; then the engine's
//!   `faces.ignore`, one step.
//! - **Moving**: Move to New Person… opens New Person for the faces; Move to Person ▸ runs the
//!   engine's `faces.move`, Remove from Person its `faces.remove` (back to Unnamed; on an ignored
//!   face, it is no longer ignored). One step each.
//! - After a change, the names typed under the groups follow their groups and faces that left the
//!   grid leave the selection.
//!
//! The ids are the UI's own, never the engine's, so `engine.execute faces.ignore` always reaches
//! the engine.

use dalica_core::PersonId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::dialogs::new_person;
use crate::panels::faces::model::{self, Grid};
use crate::state::FaceRef;

/// Why the face rows are off with no face selected.
pub const NO_FACES: &str = "Select some faces first.";
/// Why Remove from Person is off on faces that are nobody's.
pub const UNNAMED_ALREADY: &str = "These faces are not anyone's.";
/// Why naming a group refuses an empty name.
pub const NO_NAME: &str = "Type a name first.";
/// Why the commands refuse before the library is open.
pub const OPENING: &str = "The library is still opening.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "faces.nameGroup",
        label: "Name Group",
        menu: "",
        keys: &[],
        params: "{group: n, name?}: name every face of Unnamed group n (`group:<n>`) with `name`, else the name typed under it; one undo step. A name nobody has opens New Person first ({dialog: \"newPerson\"})",
        run: name_group,
    },
    UiCommand {
        id: "faces.ignoreGroup",
        label: "Ignore Group",
        menu: "",
        keys: &[],
        params: "{group: n, confirm?}: ignore every face of Unnamed group n (its ×); asks \"Ignore these faces?\" first ({dialog: \"ignoreFaces\"}) unless confirm is true or the question was turned off",
        run: ignore_group,
    },
    UiCommand {
        id: "faces.ignoreSelected",
        label: "Ignore Face",
        menu: "",
        keys: &[],
        params: "{faces?, confirm?}: ignore the faces (default: the faces selected); asks \"Ignore these faces?\" first unless confirm is true or the question was turned off; Delete in the Unnamed grid",
        run: ignore_selected,
    },
    UiCommand {
        id: "faces.moveToNewPerson",
        label: "Move to New Person…",
        menu: "",
        keys: &[],
        params: "{faces?}: open New Person for the faces (default: the faces selected)",
        run: move_to_new_person,
    },
    UiCommand {
        id: "faces.moveToPerson",
        label: "Move to Person",
        menu: "",
        keys: &[],
        params: "{person, faces?}: name the faces (default: the faces selected) as `person`; one undo step",
        run: move_to_person,
    },
    UiCommand {
        id: "faces.removeFromPerson",
        label: "Remove from Person",
        menu: "",
        keys: &[],
        params: "{faces?}: send the faces (default: the faces selected) back to Unnamed; one undo step",
        run: remove_from_person,
    },
    UiCommand {
        id: "faces.showIgnored",
        label: "Show Ignored Faces",
        menu: "",
        keys: &[],
        params: "{on?}: show the ignored faces in the Unnamed grid (toggles without `on`)",
        run: show_ignored,
    },
    UiCommand {
        id: "faces.expandGroups",
        label: "Expand Groups",
        menu: "",
        keys: &[],
        params: "{on?}: show every face of each Unnamed group (toggles without `on`)",
        run: expand_groups,
    },
];

/// The face menu's rows are off with nothing to act on.
pub const CHECKS: &[(&str, Check)] = &[
    ("faces.ignoreSelected", has_faces),
    ("faces.moveToNewPerson", has_faces),
    ("faces.moveToPerson", has_faces),
    ("faces.removeFromPerson", named_faces),
];

/// The faces a check reads: the `faces` sent, else the selection (anything unreadable: none).
fn checked_faces(app: &App, params: &Value) -> Vec<FaceRef> {
    match params.get("faces") {
        Some(v) => serde_json::from_value(v.clone()).unwrap_or_default(),
        None => app.state().unnamed.selection.clone(),
    }
}

fn has_faces(app: &App, params: &Value) -> Result<(), String> {
    if app.session().is_none() {
        return Err(OPENING.into());
    }
    if checked_faces(app, params).is_empty() { Err(NO_FACES.into()) } else { Ok(()) }
}

/// Remove from Person needs a face that is someone's or ignored.
fn named_faces(app: &App, params: &Value) -> Result<(), String> {
    has_faces(app, params)?;
    let lib = app.session().map(dalica_engine::Session::library).ok_or_else(|| OPENING.to_string())?;
    let someone = checked_faces(app, params).iter().any(|f| {
        lib.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) })
            .is_some_and(|face| face.person != dalica_library::FacePerson::Unnamed)
    });
    if someone { Ok(()) } else { Err(UNNAMED_ALREADY.into()) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameGroup {
    group: u32,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreGroup {
    group: u32,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faces {
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreFaces {
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToPerson {
    person: PersonId,
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    on: Option<bool>,
}

/// The faces a command acts on: those named, else the faces selected in the grid.
pub fn targets(app: &App, faces: Option<Vec<FaceRef>>) -> Vec<FaceRef> {
    faces.unwrap_or_else(|| app.state().unnamed.selection.clone())
}

/// `faces`, each of a photo of the library (`not_found` for one that is not; unchecked while no
/// library is open).
pub fn known_faces(app: &App, faces: Vec<FaceRef>) -> Result<Vec<FaceRef>, CmdError> {
    if let Some(s) = app.session()
        && let Some(f) = faces.iter().find(|f| s.library().photo(f.photo).is_none())
    {
        return Err(CmdError::NotFound(format!("no photo {} for face {f}", f.photo)));
    }
    Ok(faces)
}

/// `faces`, known and not empty (`disabled` with [`NO_FACES`] when empty, or before the library
/// opens).
fn some_faces(app: &App, faces: Vec<FaceRef>) -> Result<Vec<FaceRef>, CmdError> {
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    let faces = known_faces(app, faces)?;
    if faces.is_empty() {
        return Err(CmdError::Disabled(NO_FACES.into()));
    }
    Ok(faces)
}

/// The grid as it is now (empty before the library opens).
pub fn grid_now(app: &mut App) -> Grid {
    let ignored = app.state().unnamed.show_ignored;
    app.session_mut().map(|s| model::grid(s, ignored)).unwrap_or_default()
}

/// The faces of group `n` of the grid now (`not_found` when there is no such group).
fn group_faces(app: &mut App, n: u32) -> Result<(Grid, Vec<FaceRef>), CmdError> {
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    let grid = grid_now(app);
    let faces = grid.group(n).map(|g| g.faces.clone()).ok_or_else(|| CmdError::NotFound(format!("the Unnamed grid has no group {n}")))?;
    Ok((grid, faces))
}

/// After the faces changed: the names typed follow their groups from `old` to the grid now, and
/// the selection keeps only the faces still shown.
pub fn settle(app: &mut App, old: &Grid) {
    let new = grid_now(app);
    let u = &mut app.state_mut().unnamed;
    u.names = model::remap_names(&u.names, old, &new);
    u.selection = model::still_shown(&u.selection, &new);
}

/// Runs the engine's `command` with `params`, then [`settle`]s from `old`.
fn change(app: &mut App, old: &Grid, command: &str, params: &Value) -> Result<Value, CmdError> {
    let reply = app.run(command, params)?;
    settle(app, old);
    Ok(reply)
}

/// Names `faces` `name`: a person's name names them at once; a new name opens New Person.
fn name_faces(app: &mut App, old: &Grid, faces: Vec<FaceRef>, name: &str) -> Result<Value, CmdError> {
    let known = app.session().is_some_and(|s| s.library().person_by_name(name).is_some());
    if !known {
        return crate::dialogs::open(app, new_person::dialog(name, &faces));
    }
    change(app, old, "faces.name", &json!({ "faces": faces, "name": name }))
}

/// Ignores `faces`, asking first unless `confirm` or the question is off.
fn ignore(app: &mut App, old: &Grid, faces: Vec<FaceRef>, confirm: bool) -> Result<Value, CmdError> {
    if !confirm && new_person::asks_before_ignoring(app) {
        return crate::dialogs::open(app, new_person::ignore_dialog(&faces));
    }
    change(app, old, "faces.ignore", &json!({ "faces": faces }))
}

fn name_group(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: NameGroup = parse(params)?;
    let name = p.name.or_else(|| app.state().unnamed.names.get(&p.group).cloned()).unwrap_or_default();
    if name.chars().count() > crate::state::MAX_DRAFT {
        return Err(CmdError::BadParams(format!("`name` is at most {} characters", crate::state::MAX_DRAFT)));
    }
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CmdError::Disabled(NO_NAME.into()));
    }
    let (grid, faces) = group_faces(app, p.group)?;
    name_faces(app, &grid, faces, &name)
}

fn ignore_group(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: IgnoreGroup = parse(params)?;
    let (grid, faces) = group_faces(app, p.group)?;
    ignore(app, &grid, faces, p.confirm)
}

fn ignore_selected(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: IgnoreFaces = parse_object(params)?;
    let faces = some_faces(app, targets(app, p.faces))?;
    let grid = grid_now(app);
    ignore(app, &grid, faces, p.confirm)
}

fn move_to_new_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = some_faces(app, targets(app, p.faces))?;
    crate::dialogs::open(app, new_person::dialog("", &faces))
}

fn move_to_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: ToPerson = parse(params)?;
    let faces = some_faces(app, targets(app, p.faces))?;
    let grid = grid_now(app);
    change(app, &grid, "faces.move", &json!({ "faces": faces, "person": p.person }))
}

fn remove_from_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = some_faces(app, targets(app, p.faces))?;
    let grid = grid_now(app);
    change(app, &grid, "faces.remove", &json!({ "faces": faces }))
}

fn show_ignored(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    let u = &mut app.state_mut().unnamed;
    u.show_ignored = p.on.unwrap_or(!u.show_ignored);
    let on = u.show_ignored;
    // Hidden ignored faces leave the selection.
    if !on {
        let shown = grid_now(app);
        let u = &mut app.state_mut().unnamed;
        u.selection = model::still_shown(&u.selection, &shown);
    }
    Ok(json!({ "showIgnored": on }))
}

fn expand_groups(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    let u = &mut app.state_mut().unnamed;
    u.expanded = p.on.unwrap_or(!u.expanded);
    Ok(json!({ "expanded": u.expanded }))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;

    #[test]
    fn the_toggles_act_on_the_view_state_and_bad_params_change_nothing() {
        let mut app = App::new(None);
        assert_eq!(app.run("faces.showIgnored", &json!({})).unwrap(), json!({ "showIgnored": true }));
        assert_eq!(app.run("faces.showIgnored", &json!({ "on": true })).unwrap(), json!({ "showIgnored": true }));
        assert_eq!(app.run("faces.expandGroups", &Value::Null).unwrap(), json!({ "expanded": true }));
        assert!(app.state().unnamed.show_ignored && app.state().unnamed.expanded);
        // Before the library opens the library's commands wait for it.
        for (id, params) in [
            ("faces.nameGroup", json!({ "group": 0, "name": "Ana" })),
            ("faces.ignoreGroup", json!({ "group": 2 })),
            ("faces.ignoreSelected", json!({ "faces": ["12.0"] })),
            ("faces.moveToNewPerson", json!({})),
            ("faces.moveToPerson", json!({ "person": "p00a1" })),
            ("faces.removeFromPerson", json!({ "faces": ["12.0", "3.255"] })),
        ] {
            assert_eq!(app.run(id, &params), Err(CmdError::Disabled(OPENING.into())), "{id}");
        }
        // A face past a photo's 256, a bad person, a long name: bad_params.
        for (id, params) in [
            ("faces.ignoreSelected", json!({ "faces": ["12.256"] })),
            ("faces.ignoreSelected", json!({ "faces": ["12"] })),
            ("faces.moveToPerson", json!({ "person": "a00a1" })),
            ("faces.nameGroup", json!({ "group": 0, "name": "x".repeat(4097) })),
            ("faces.nameGroup", json!({ "group": -1 })),
            ("faces.ignoreGroup", json!({ "group": 0, "name": "Ana" })),
        ] {
            assert!(matches!(app.run(id, &params), Err(CmdError::BadParams(_))), "{id} {params}");
        }
        // The faces a command acts on: those named, else the selection.
        let face = FaceRef { photo: PhotoId::new(4), index: 2 };
        app.state_mut().unnamed.selection = vec![face];
        assert_eq!(targets(&app, None), vec![face]);
        assert_eq!(targets(&app, Some(Vec::new())), Vec::<FaceRef>::new());
    }
}
