//! The Unnamed grid and the face menu (`organizing.md` §6.2–§6.3, `menus-and-shortcuts.md` §4):
//! naming a group from its "Add a name" field, ignoring a group (its ×) or the selected faces
//! (Ignore Face, Delete), moving faces to a new or an existing person and back to Unnamed, and the
//! header's [Show Ignored Faces] and [Expand Groups].
//!
//! The faces a command acts on are its `faces` (each `"<PhotoId>.<n>"`), else the faces selected in
//! the grid (`unnamed.selection`). Naming, ignoring and moving are one library undo step each,
//! through the engine's `faces.name`, `faces.ignore`, `faces.move` and `faces.remove` (M13.15);
//! a name nobody has yet asks New Person first (`crate::dialogs::new_person`), and ignoring asks
//! "Ignore these faces?" unless told not to.
//!
//! M13.4's stubs: the ids, parameters and keys are final, and the two toggles act on the view
//! state; the commands that change the library check their parameters, then refuse with
//! [`NOT_YET`] (their menu rows greyed with it) until M13.21 makes them act. Their ids are the UI's
//! own, never the engine's, so `engine.execute faces.ignore` always reaches the engine.

use dalica_core::PersonId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::FaceRef;

/// Why the commands that change the library do not act yet.
pub const NOT_YET: &str = "Arrives with the Unnamed faces.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "faces.nameGroup",
        label: "Name Group",
        menu: "",
        keys: &[],
        params: "{group: n, name?}: name every face of Unnamed group n (`group:<n>`) with `name`, else the name typed under it; one undo step",
        run: name_group,
    },
    UiCommand {
        id: "faces.ignoreGroup",
        label: "Ignore Group",
        menu: "",
        keys: &[],
        params: "{group: n}: ignore every face of Unnamed group n (its ×), after \"Ignore these faces?\"",
        run: ignore_group,
    },
    UiCommand {
        id: "faces.ignoreSelected",
        label: "Ignore Face",
        menu: "",
        keys: &[],
        params: "{faces?}: ignore the faces (default: the faces selected), after \"Ignore these faces?\"; Delete in the Unnamed grid",
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

/// The commands that wait for M13.21: their rows are greyed with [`NOT_YET`].
pub const CHECKS: &[(&str, Check)] = &[
    ("faces.nameGroup", not_yet),
    ("faces.ignoreGroup", not_yet),
    ("faces.ignoreSelected", not_yet),
    ("faces.moveToNewPerson", not_yet),
    ("faces.moveToPerson", not_yet),
    ("faces.removeFromPerson", not_yet),
];

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    group: u32,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faces {
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
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

/// A stub that changes the library: what it will run once M13.21 lands (the engine's `command`
/// with `params`), refused with [`NOT_YET`] for now.
fn waits(_command: &str, _params: Value) -> Result<Value, CmdError> {
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn name_group(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Group = parse(params)?;
    let name = p.name.or_else(|| app.state().unnamed.names.get(&p.group).cloned()).unwrap_or_default();
    if name.chars().count() > crate::state::MAX_DRAFT {
        return Err(CmdError::BadParams(format!("`name` is at most {} characters", crate::state::MAX_DRAFT)));
    }
    waits("faces.name", json!({ "group": p.group, "name": name }))
}

fn ignore_group(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Group = parse(params)?;
    if p.name.is_some() {
        return Err(CmdError::BadParams("`faces.ignoreGroup` takes `group` only".into()));
    }
    waits("faces.ignore", json!({ "group": p.group }))
}

fn ignore_selected(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = known_faces(app, targets(app, p.faces))?;
    waits("faces.ignore", json!({ "faces": faces }))
}

fn move_to_new_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = known_faces(app, targets(app, p.faces))?;
    waits("faces.name", json!({ "faces": faces, "create": true }))
}

fn move_to_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: ToPerson = parse(params)?;
    let faces = known_faces(app, targets(app, p.faces))?;
    waits("faces.move", json!({ "faces": faces, "person": p.person }))
}

fn remove_from_person(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = known_faces(app, targets(app, p.faces))?;
    waits("faces.remove", json!({ "faces": faces }))
}

fn show_ignored(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: On = parse_object(params)?;
    let u = &mut app.state_mut().unnamed;
    u.show_ignored = p.on.unwrap_or(!u.show_ignored);
    Ok(json!({ "showIgnored": u.show_ignored }))
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
    fn the_toggles_act_on_the_view_state_and_the_stubs_wait() {
        let mut app = App::new(None);
        assert_eq!(app.run("faces.showIgnored", &json!({})).unwrap(), json!({ "showIgnored": true }));
        assert_eq!(app.run("faces.showIgnored", &json!({ "on": true })).unwrap(), json!({ "showIgnored": true }));
        assert_eq!(app.run("faces.expandGroups", &Value::Null).unwrap(), json!({ "expanded": true }));
        assert!(app.state().unnamed.show_ignored && app.state().unnamed.expanded);
        for (id, params) in [
            ("faces.nameGroup", json!({ "group": 0, "name": "Ana" })),
            ("faces.ignoreGroup", json!({ "group": 2 })),
            ("faces.ignoreSelected", json!({ "faces": ["12.0"] })),
            ("faces.moveToNewPerson", json!({})),
            ("faces.moveToPerson", json!({ "person": "p00a1" })),
            ("faces.removeFromPerson", json!({ "faces": ["12.0", "3.255"] })),
        ] {
            assert_eq!(app.run(id, &params), Err(CmdError::Disabled(NOT_YET.into())), "{id}");
            assert_eq!(super::super::enabled(&app, id, &json!({})), Err(NOT_YET.into()), "{id}");
        }
        // A face past a photo's 256, a bad person, a long name: bad_params.
        for (id, params) in [
            ("faces.ignoreSelected", json!({ "faces": ["12.256"] })),
            ("faces.ignoreSelected", json!({ "faces": ["12"] })),
            ("faces.moveToPerson", json!({ "person": "a00a1" })),
            ("faces.nameGroup", json!({ "group": 0, "name": "x".repeat(4097) })),
            ("faces.nameGroup", json!({ "group": -1 })),
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
