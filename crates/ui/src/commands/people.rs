//! The People group, the person sections and the person menu (`organizing.md` §6.2–§6.3,
//! `menus-and-shortcuts.md` §1.3 and §4):
//!
//! - the person menu (a person's tree row, its section header): **Rename…** ([`RENAME`]),
//!   **Merge Into ▸** a person ([`MERGE`], one row per other person, `{into}`) and **Delete
//!   Person** ([`DELETE`]), [openers](super::Opener) of the engine's `people.rename`,
//!   `people.merge` and `people.delete` (M13.15): a click opens the dialog
//!   (`crate::dialogs::people`), whose default button runs the engine command; `engine.execute`
//!   runs it directly;
//! - **Set as Person Thumbnail** (`people.thumbnail`: the face menu's, and a person section
//!   header's for its focused face);
//! - a person section header's **Zoom to Faces** / **Show Whole Photos** (`people.zoomToFaces`);
//! - the view-options popup's **Sort People by Name / Count / Top 10** (`view.peopleSort`,
//!   [`sort`]).
//!
//! M13.4's stubs: the ids, parameters and dialogs are final, and Zoom to Faces acts on the view
//! state (`people`); [`sort`] does too but is not registered yet: its popup rows go live, and
//! `tests/toolbar.rs`'s check that only the folder sorts are live changes, when M13.25 registers it; the menu's rows and Set as Person Thumbnail are greyed with
//! [`NOT_YET`] until M13.25 makes them act. People are checked by the shape of their id only until
//! the library holds people (M13.1).

use dalica_core::PersonId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, Opener, UiCommand, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, DialogKind};
use crate::state::{FaceRef, MAX_PEOPLE, PeopleSort};

/// Why the person menu and Set as Person Thumbnail do not act yet.
pub const NOT_YET: &str = "Arrives with the People group.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "people.thumbnail",
        label: "Set as Person Thumbnail",
        menu: "",
        keys: &[],
        params: "{face?}: make the face (default: the face selected) its person's thumbnail; one undo step",
        run: thumbnail,
    },
    UiCommand {
        id: "people.zoomToFaces",
        label: "Zoom to Faces",
        menu: "",
        keys: &[],
        params: "{person, on?}: the person's section shows their faces (on) or the whole photos (off); toggles without `on`",
        run: zoom_to_faces,
    },
];

/// The view-options popup's people sorts, registered by M13.25 with [`sort`] as its `run`.
pub const SORT_ID: &str = "view.peopleSort";
/// [`SORT_ID`]'s parameter doc.
pub const SORT_PARAMS: &str = "{by: \"name\" | \"count\" | \"top10\"}: the order of the people in the People group";

pub const CHECKS: &[(&str, Check)] = &[("people.thumbnail", not_yet)];

/// The person menu's Rename…: opens Rename Person for `{person}`.
pub const RENAME: Opener = Opener { id: "people.rename", dialog: "renamePerson", keys: &[], open: open_rename, check: not_yet };
/// The person menu's Merge Into ▸ rows: open the merge question for `{person, into}`.
pub const MERGE: Opener = Opener { id: "people.merge", dialog: "mergePeople", keys: &[], open: open_merge, check: not_yet };
/// The person menu's Delete Person: opens its question for `{person}`.
pub const DELETE: Opener = Opener { id: "people.delete", dialog: "deletePerson", keys: &[], open: open_delete, check: not_yet };

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

/// A stub: what it will run once M13.25 lands (the engine's `command` with `params`), refused with
/// [`NOT_YET`] for now.
fn waits(_command: &str, _params: Value) -> Result<Value, CmdError> {
    Err(CmdError::Disabled(NOT_YET.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Face {
    #[serde(default)]
    face: Option<FaceRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Zoom {
    person: PersonId,
    #[serde(default)]
    on: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sort {
    by: PeopleSort,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Person {
    person: PersonId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Merge {
    person: PersonId,
    into: PersonId,
}

fn thumbnail(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Face = parse_object(params)?;
    waits("people.setThumbnail", json!({ "face": p.face }))
}

fn zoom_to_faces(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Zoom = parse(params)?;
    let list = &app.state().people.zoom_to_faces;
    let now = list.contains(&p.person);
    let on = p.on.unwrap_or(!now);
    if on && !now && list.len() >= MAX_PEOPLE {
        return Err(CmdError::BadParams(format!("at most {MAX_PEOPLE} people are zoomed to their faces")));
    }
    let list = &mut app.state_mut().people.zoom_to_faces;
    if on && !now {
        list.push(p.person.clone());
    } else if !on {
        list.retain(|x| *x != p.person);
    }
    Ok(json!({ "person": p.person, "on": on }))
}

/// `view.peopleSort {by}`: the order of the people in the People group (`people.sort`).
pub fn sort(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Sort = parse(params)?;
    app.state_mut().people.sort = p.by;
    Ok(json!({ "by": p.by }))
}

/// Whether the row of `id` with `params` is checked: the sort chosen.
pub fn checked(app: &App, id: &str, params: &Value) -> Option<bool> {
    (id == SORT_ID).then(|| params.get("by").is_some_and(|by| *by == json!(app.state().people.sort)))
}

fn open_with(app: &mut App, kind: DialogKind, draft: Map<String, Value>) -> Result<Value, CmdError> {
    dialogs::open(app, dialogs::people::dialog(kind, draft))
}

fn open_rename(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    open_with(
        app,
        DialogKind::RenamePerson,
        Map::from_iter([("person".into(), json!(p.person)), ("name".into(), json!("")), ("nickname".into(), json!(""))]),
    )
}

fn open_merge(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Merge = parse(params)?;
    if p.person == p.into {
        return Err(CmdError::BadParams("a person cannot be merged into themselves".into()));
    }
    open_with(app, DialogKind::MergePeople, Map::from_iter([("person".into(), json!(p.person)), ("into".into(), json!(p.into))]))
}

fn open_delete(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    open_with(app, DialogKind::DeletePerson, Map::from_iter([("person".into(), json!(p.person))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_and_zoom_act_on_the_view_state() {
        let mut app = App::new(None);
        assert_eq!(sort(&mut app, &json!({ "by": "count" })).unwrap(), json!({ "by": "count" }));
        assert_eq!(app.state().people.sort, PeopleSort::Count);
        assert_eq!(checked(&app, "view.peopleSort", &json!({ "by": "count" })), Some(true));
        assert_eq!(checked(&app, "view.peopleSort", &json!({ "by": "name" })), Some(false));
        assert_eq!(checked(&app, "view.folderSort", &json!({ "by": "name" })), None);
        sort(&mut app, &json!({ "by": "top10" })).unwrap();
        assert_eq!(app.state().people.sort, PeopleSort::Top10);
        let ana = PersonId::parse("p00a1").unwrap();
        assert_eq!(app.run("people.zoomToFaces", &json!({ "person": "p00a1" })).unwrap(), json!({ "person": "p00a1", "on": true }));
        app.run("people.zoomToFaces", &json!({ "person": "p00a1", "on": true })).unwrap();
        assert_eq!(app.state().people.zoom_to_faces, vec![ana]);
        app.run("people.zoomToFaces", &json!({ "person": "p00a1" })).unwrap();
        assert!(app.state().people.zoom_to_faces.is_empty());
        assert_eq!(app.run("people.thumbnail", &json!({ "face": "1.0" })), Err(CmdError::Disabled(NOT_YET.into())));
        for (id, bad) in [("people.zoomToFaces", json!({ "person": "ana" })), ("people.thumbnail", json!({ "face": 3 }))] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        for bad in [json!({ "by": "age" }), json!({}), json!({ "by": "name", "x": 1 }), json!([])] {
            assert!(matches!(sort(&mut app, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }

    #[test]
    fn the_person_menu_opens_its_dialogs_with_their_drafts() {
        let mut app = App::new(None);
        assert_eq!((RENAME.check)(&app, &json!({})), Err(NOT_YET.into()));
        assert_eq!((RENAME.open)(&mut app, &json!({ "person": "p00a1" })).unwrap(), json!({ "dialog": "renamePerson" }));
        assert_eq!(dialogs::top(&app).map(|d| d.fields.get("person").cloned()), Some(Some(json!("p00a1"))));
        dialogs::cancel(&mut app).unwrap();
        assert_eq!((MERGE.open)(&mut app, &json!({ "person": "p00a1", "into": "p00b2" })).unwrap(), json!({ "dialog": "mergePeople" }));
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(m)) if m == NOT_YET), "the stub's Merge waits");
        dialogs::cancel(&mut app).unwrap();
        assert!(matches!((MERGE.open)(&mut app, &json!({ "person": "p00a1", "into": "p00a1" })), Err(CmdError::BadParams(_))));
        assert_eq!((DELETE.open)(&mut app, &json!({ "person": "p00a1" })).unwrap(), json!({ "dialog": "deletePerson" }));
        dialogs::cancel(&mut app).unwrap();
        for bad in [json!({}), json!({ "person": "x" }), json!({ "person": "p00a1", "zzz": 1 })] {
            assert!(matches!((DELETE.open)(&mut app, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(app.state().dialogs.is_empty());
    }
}
