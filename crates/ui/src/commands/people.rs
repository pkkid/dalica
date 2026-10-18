//! The People group, the person sections and the person menu (`organizing.md` §6.2–§6.3,
//! `menus-and-shortcuts.md` §1.3 and §4):
//!
//! - the person menu (a person's tree row, its section header; [`menu_nodes`], [`show_menu`]):
//!   **Rename…** ([`RENAME`]), **Merge Into ▸** a person ([`MERGE`], one row per other person by
//!   name, `{into}`: `menuitem:people.merge/<PersonId>`) and **Delete Person** ([`DELETE`]),
//!   [openers](super::Opener) of the engine's `people.rename`, `people.merge` and `people.delete`:
//!   a click opens the dialog (`crate::dialogs::people`), whose default button runs the engine
//!   command, one library undo step; `engine.execute` runs it directly. Their `person` is the one
//!   named, else the person whose section is selected;
//! - **Set as Person Thumbnail** (`people.thumbnail {face?}`: the face named, else the first face
//!   selected (the face menu's), else the person's face on the focused photo of their section
//!   selected), through the engine's `people.setThumbnail`, one undo step;
//! - **Delete** in a person's section (`people.removeFaces {ids?}`, `crate::keys::delete_route`):
//!   Remove from Person on the person's faces on the photos named, else selected; back to Unnamed,
//!   one undo step;
//! - a person section header's **Zoom to Faces** / **Show Whole Photos** (`people.zoomToFaces`);
//! - the view-options popup's **Sort People by Name / Count / Top 10** (`view.peopleSort`).

use dalica_core::PersonId;
use dalica_engine::CmdError;
use egui::Response;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Check, Opener, UiCommand, parse, parse_object};
use crate::app::App;
use crate::dialogs::{self, DialogKind};
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node, Platform};
use crate::state::{FaceRef, MAX_PEOPLE, PeopleSort};

/// Why the person menu's rows are off with no person to act on.
pub const NO_PERSON: &str = "Select a person first.";
/// Why Set as Person Thumbnail is off with no face to act on.
pub const NO_FACE: &str = "Select a face first.";
/// Why the commands refuse before the library is open.
pub const OPENING: &str = "The library is still opening.";
/// The submenu the other people are listed in, and the row standing in for them while there are
/// none.
pub const MERGE_INTO: &str = "Merge Into";
pub const NO_OTHERS: &str = "No other people";

/// The view-options popup's people sorts.
pub const SORT_ID: &str = "view.peopleSort";
/// [`SORT_ID`]'s parameter doc.
pub const SORT_PARAMS: &str = "{by: \"name\" | \"count\" | \"top10\"}: the order of the people in the People group and of their sections (Top 10: the ten people in the most photos, by count, then the others by name)";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "people.thumbnail",
        label: "Set as Person Thumbnail",
        menu: "",
        keys: &[],
        params: "{face?}: make the face (default: the first face selected, else the person's face on the focused photo of their section) its person's thumbnail; one undo step",
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
    UiCommand { id: SORT_ID, label: "Sort People", menu: "", keys: &[], params: SORT_PARAMS, run: sort },
    UiCommand {
        id: REMOVE_FACES,
        label: "Remove from Person",
        menu: "",
        keys: &[],
        params: "{ids?: [PhotoId]}: in a person's section, their faces on the photos (default: the selection) go back to Unnamed; one undo step (Delete there)",
        run: remove_faces,
    },
];

/// What Delete runs in a person's section.
pub const REMOVE_FACES: &str = "people.removeFaces";

pub const CHECKS: &[(&str, Check)] = &[("people.thumbnail", has_face)];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    #[serde(default)]
    ids: Option<Vec<dalica_core::PhotoId>>,
}

/// `people.removeFaces {ids?}`: the faces of the person whose section is selected on `ids` (else
/// the photos selected) go back to Unnamed.
fn remove_faces(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Ids = parse_object(params)?;
    let person = person_of(app, None)?;
    let ids = p.ids.unwrap_or_else(|| app.state().selection.ids.clone());
    let lib = app.session().map(dalica_engine::Session::library).ok_or_else(|| CmdError::Disabled(OPENING.into()))?;
    let faces: Vec<FaceRef> = ids.iter().filter_map(|id| crate::panels::light_table::header::face_on(lib, *id, &person)).collect();
    if faces.is_empty() {
        return Err(CmdError::Disabled(NO_FACE.into()));
    }
    app.run("faces.remove", &json!({ "faces": faces }))
}

/// The person menu's Rename…: opens Rename Person for `{person?}`.
pub const RENAME: Opener = Opener { id: "people.rename", dialog: "renamePerson", keys: &[], open: open_rename, check: has_person };
/// The person menu's Merge Into ▸ rows: open the merge question for `{person?, into}`.
pub const MERGE: Opener = Opener { id: "people.merge", dialog: "mergePeople", keys: &[], open: open_merge, check: has_person };
/// The person menu's Delete Person: opens its question for `{person?}`.
pub const DELETE: Opener = Opener { id: "people.delete", dialog: "deletePerson", keys: &[], open: open_delete, check: has_person };

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
    #[serde(default)]
    person: Option<PersonId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Merge {
    #[serde(default)]
    person: Option<PersonId>,
    into: PersonId,
}

/// The person a person command acts on: `given`, else the one whose section is selected; it must
/// be the library's (`not_found` otherwise; `disabled` with nobody to act on or no library).
pub fn person_of(app: &App, given: Option<PersonId>) -> Result<PersonId, CmdError> {
    let session = app.session().ok_or_else(|| CmdError::Disabled(OPENING.into()))?;
    let person = given
        .or_else(|| app.state().selection.section.and_then(crate::state::Section::person))
        .ok_or_else(|| CmdError::Disabled(NO_PERSON.into()))?;
    if session.library().person(&person).is_none() {
        return Err(CmdError::NotFound(format!("no person {person}")));
    }
    Ok(person)
}

fn has_person(app: &App, params: &Value) -> Result<(), String> {
    let given = params.get("person").and_then(|v| serde_json::from_value::<PersonId>(v.clone()).ok());
    person_of(app, given).map(|_| ()).map_err(|e| e.to_string())
}

/// The face Set as Person Thumbnail takes: `given`, else the first face selected, else the
/// person's face on the focused (else first) photo selected in their section.
fn thumbnail_face(app: &App, given: Option<FaceRef>) -> Option<FaceRef> {
    if given.is_some() {
        return given;
    }
    let state = app.state();
    if let Some(f) = state.unnamed.selection.first() {
        return Some(*f);
    }
    let person = state.selection.section.and_then(crate::state::Section::person)?;
    let lib = app.session()?.library();
    let sel = &state.selection;
    sel.anchor.into_iter().chain(sel.ids.iter().copied()).find_map(|id| crate::panels::light_table::header::face_on(lib, id, &person))
}

fn has_face(app: &App, params: &Value) -> Result<(), String> {
    if app.session().is_none() {
        return Err(OPENING.into());
    }
    let given = params.get("face").and_then(|v| serde_json::from_value::<FaceRef>(v.clone()).ok());
    thumbnail_face(app, given).map(|_| ()).ok_or_else(|| NO_FACE.into())
}

fn thumbnail(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Face = parse_object(params)?;
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    let face = thumbnail_face(app, p.face).ok_or_else(|| CmdError::Disabled(NO_FACE.into()))?;
    app.run("people.setThumbnail", &json!({ "face": face }))
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
    let p: Person = parse_object(params)?;
    let person = person_of(app, p.person)?;
    // Name and Nickname start as they are.
    let (name, nickname) = app.session().and_then(|s| s.library().person(&person).map(|x| (x.name.clone(), x.nickname.clone()))).unwrap_or_default();
    open_with(
        app,
        DialogKind::RenamePerson,
        Map::from_iter([("person".into(), json!(person)), ("name".into(), json!(name)), ("nickname".into(), json!(nickname))]),
    )
}

fn open_merge(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Merge = parse(params)?;
    let person = person_of(app, p.person)?;
    let into = person_of(app, Some(p.into))?;
    if person == into {
        return Err(CmdError::BadParams("a person cannot be merged into themselves".into()));
    }
    open_with(app, DialogKind::MergePeople, Map::from_iter([("person".into(), json!(person)), ("into".into(), json!(into))]))
}

fn open_delete(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse_object(params)?;
    let person = person_of(app, p.person)?;
    open_with(app, DialogKind::DeletePerson, Map::from_iter([("person".into(), json!(person))]))
}

/// The person menu's rows for `person`: Rename… and Delete Person act on them, Merge Into ▸ lists
/// the other people by name (or the greyed [`NO_OTHERS`]).
pub fn menu_nodes(app: &App, person: &PersonId) -> Vec<Node> {
    let mut rows = menus::person_menu(app, Platform::current());
    let params = json!({ "person": person });
    let ready = has_person(app, &params);
    for node in rows.iter_mut() {
        match node {
            Node::Item(item) => {
                item.params = params.clone();
                set_ready(item, ready.clone());
            }
            Node::Submenu { label, children, .. } if *label == MERGE_INTO => {
                let others: Vec<(PersonId, String)> =
                    crate::panels::faces::menu::people_by_name(app).into_iter().filter(|(id, _)| id != person).collect();
                *children = if others.is_empty() {
                    vec![merge_row("none", NO_OTHERS.into(), Value::Null, Err("There is nobody else to merge with.".into()))]
                } else {
                    others
                        .into_iter()
                        .map(|(into, name)| merge_row(into.as_str(), name, json!({ "person": person, "into": into }), ready.clone()))
                        .collect()
                };
            }
            _ => {}
        }
    }
    rows
}

/// A live row is enabled when `ready` says so, else greyed with why.
fn set_ready(item: &mut ItemNode, ready: Result<(), String>) {
    if !item.live {
        return;
    }
    (item.enabled, item.reason) = match ready {
        Ok(()) => (true, None),
        Err(why) => (false, Some(why)),
    };
}

fn merge_row(part: &str, label: String, params: Value, ready: Result<(), String>) -> Node {
    let (enabled, reason) = match ready {
        Ok(()) => (true, None),
        Err(why) => (false, Some(why)),
    };
    Node::Item(ItemNode {
        id: "people.merge",
        label,
        // No catalog label: a menu path finds the row by the person's name only.
        catalog_label: "",
        params,
        widget: format!("menuitem:people.merge/{part}"),
        shortcut: None,
        live: true,
        enabled,
        reason,
        mark: Mark::None,
        checked: false,
        dialog: Some("mergePeople"),
    })
}

/// Draws the person menu of `person` on `response` (their tree row's, their section header's)
/// while it is open, and runs the row clicked. Returns the command id run, if any.
pub fn show_menu(response: &Response, app: &mut App, person: &PersonId) -> Option<&'static str> {
    let rows = menu_nodes(app, person);
    menus::context_menu(response, app, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_and_zoom_act_on_the_view_state() {
        let mut app = App::new(None);
        assert_eq!(app.run(SORT_ID, &json!({ "by": "count" })).unwrap(), json!({ "by": "count" }));
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
        assert_eq!(app.run("people.thumbnail", &json!({ "face": "1.0" })), Err(CmdError::Disabled(OPENING.into())));
        for (id, bad) in [("people.zoomToFaces", json!({ "person": "ana" })), ("people.thumbnail", json!({ "face": 3 }))] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        for bad in [json!({ "by": "age" }), json!({}), json!({ "by": "name", "x": 1 }), json!([])] {
            assert!(matches!(sort(&mut app, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }
}
