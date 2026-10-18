//! People as data (`organizing.md` §6, `faces.md` §9; the face tiles are `crate::face_tiles`):
//! `people.list {sort}` (counts, thumbnails, pending suggestions), `people.create {name,
//! nickname?}`, `people.rename`, `people.merge`, `people.delete {confirm}` and
//! `people.setThumbnail`, each change one library undo step.
//!
//! - People are named by id (`"p7f3a2c"`): an id that is not one is `bad_params`, a person the
//!   library does not have `not_found`. A name is unique ignoring case: creating or renaming to a
//!   name another person holds is `bad_params` naming that person's id (Merge Into joins two).
//! - **Merge** moves every face of `person` to `into`, whose name is kept, and `person` goes;
//!   **Delete** asks first (`disabled` with [`DELETE_QUESTION`] without `"confirm": true`) and
//!   sends the person's faces back to Unnamed. Undo gives the person, their nickname and thumbnail
//!   and every face back.
//! - **Person albums**: each person is an album of the photos holding their faces, by date like a
//!   folder; `people.list` names its section `person:<id>` and counts its photos and faces.
//! - The people are written to `people.json` after each change (`people_file.rs`), so nicknames,
//!   thumbnails and people with no face survive a rebuild.
//! - Errors and logs name people by id, never by name (`faces.md` §7).

use dalica_core::PersonId;
use dalica_library::{FaceKey, FacePerson, Op, PeopleSort, Person};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::faces::{clean_name, draw, face_id, known_faces, known_person};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// Delete Person's question (`organizing.md` §6.3); `people.delete` without `"confirm": true` is
/// `disabled` with it (`needs_confirm` over automation).
pub const DELETE_QUESTION: &str = "Delete this person? Their faces go back to Unnamed.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "people.list",
            label: "List People",
            menu: "",
            keys: &[],
            params: r#"{"sort"?:"name"|"count" (default name)}: every person: {people:[{id, section:"person:<id>", name, nickname:string|null, count (photos), faces, thumbnail:"<PhotoId>.<n>"|null, pending (suggestions waiting: faces.suggest's count; 0 with Suggest names off or no model)}], unnamed (faces), ignored (faces)}. Over automation without --automation-allow-people names and nicknames are null; counts stay"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "people.create",
            label: "New Person",
            menu: "",
            keys: &[],
            params: r#"{"name":string, "nickname"?:string}: a new person with no face yet (a name another person holds, ignoring case, is refused). Replies {person}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: create,
        },
        CommandSpec {
            id: "people.rename",
            label: "Rename Person",
            menu: "",
            keys: &[],
            params: r#"{"person":PersonId, "name":string, "nickname"?:string ("" clears it; left out keeps it)}: the person's name (another person's name, ignoring case, is refused: merge them instead). Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: rename,
        },
        CommandSpec {
            id: "people.merge",
            label: "Merge People",
            menu: "",
            keys: &[],
            params: r#"{"person":PersonId, "into":PersonId}: every face of person becomes into's (whose name is kept) and person goes. Replies {person: into, moved}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: merge,
        },
        CommandSpec {
            id: "people.delete",
            label: "Delete Person",
            menu: "",
            keys: &[],
            params: r#"{"person":PersonId, "confirm":bool}: the person goes and their faces go back to Unnamed; asks first: needs "confirm": true. Replies {unnamed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: delete,
        },
        CommandSpec {
            id: "people.setThumbnail",
            label: "Set as Person Thumbnail",
            menu: "",
            keys: &[],
            params: r#"{"face":"<PhotoId>.<n>" (a named face), "person"?:PersonId (its person, when given)}: the face the person's tree row, People pane and list show. Replies {person, changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: set_thumbnail,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    #[serde(default)]
    sort: PeopleSort,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: ListParams = params(p)?;
    let pending = s.pending_suggestions();
    let lib = s.library();
    let people: Vec<Value> = lib
        .people_sorted(p.sort)
        .into_iter()
        .filter_map(|id| {
            let person = lib.person(&id)?;
            Some(json!({
                "id": id, "section": format!("person:{id}"), "name": person.name,
                "nickname": if person.nickname.is_empty() { Value::Null } else { json!(person.nickname) },
                "count": lib.person_count(&id), "faces": lib.person_faces(&id).len(),
                "thumbnail": lib.person_thumbnail(&id).map(face_id),
                "pending": pending.get(&id).copied().unwrap_or(0),
            }))
        })
        .collect();
    Ok(json!({ "people": people, "unnamed": lib.unnamed_count(), "ignored": lib.ignored_faces().len() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateParams {
    name: String,
    nickname: Option<String>,
}

/// The nickname `text` gives: clean, or empty for none (`""` clears).
fn nickname(text: &str) -> Result<String, CmdError> {
    if text.trim().is_empty() { Ok(String::new()) } else { clean_name(text, "nickname") }
}

/// Refuses `name` when a person other than `except` holds it, ignoring case.
fn name_free(s: &Session, name: &str, except: Option<&PersonId>) -> Result<(), CmdError> {
    match s.library().person_by_name(name) {
        Some(other) if Some(other) != except => Err(CmdError::BadParams(format!("person {other} has that name already"))),
        _ => Ok(()),
    }
}

fn create(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: CreateParams = params(p)?;
    let name = clean_name(&p.name, "name")?;
    let nickname = nickname(p.nickname.as_deref().unwrap_or_default())?;
    name_free(s, &name, None)?;
    let id = s.library().new_person_id(draw).map_err(|e| CmdError::Limit(e.to_string()))?;
    s.apply(Op::AddPerson { id: id.clone(), person: Person { name, nickname, thumbnail: None } })?;
    Ok(json!({ "person": id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameParams {
    person: PersonId,
    name: String,
    nickname: Option<String>,
}

fn rename(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: RenameParams = params(p)?;
    known_person(s.library(), &p.person)?;
    let name = clean_name(&p.name, "name")?;
    let old = s.library().person(&p.person).cloned().unwrap_or_default();
    let nickname = match &p.nickname {
        Some(n) => nickname(n)?,
        None => old.nickname.clone(),
    };
    if old.name == name && old.nickname == nickname {
        return Ok(json!({ "changed": false }));
    }
    name_free(s, &name, Some(&p.person))?;
    s.apply(Op::RenamePerson { id: p.person, name, nickname })?;
    Ok(json!({ "changed": true }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeParams {
    person: PersonId,
    into: PersonId,
}

fn merge(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: MergeParams = params(p)?;
    known_person(s.library(), &p.person)?;
    known_person(s.library(), &p.into)?;
    if p.person == p.into {
        return Err(CmdError::BadParams(format!("person {} cannot merge into itself", p.person)));
    }
    let moved = s.library().person_faces(&p.person).len();
    s.apply(Op::MergePeople { from: p.person, into: p.into.clone() })?;
    Ok(json!({ "person": p.into, "moved": moved }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteParams {
    person: PersonId,
    #[serde(default)]
    confirm: bool,
}

fn delete(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: DeleteParams = params(p)?;
    known_person(s.library(), &p.person)?;
    if !p.confirm {
        return Err(CmdError::Disabled(DELETE_QUESTION.into()));
    }
    let unnamed = s.library().person_faces(&p.person).len();
    s.apply(Op::DeletePerson { id: p.person })?;
    Ok(json!({ "unnamed": unnamed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThumbnailParams {
    face: String,
    person: Option<PersonId>,
}

fn set_thumbnail(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: ThumbnailParams = params(p)?;
    let lib = s.library();
    let face = known_faces(lib, std::slice::from_ref(&p.face))?.into_iter().next().ok_or_else(|| CmdError::BadParams("no face".into()))?;
    let Some(found) = lib.face(face) else { return Err(CmdError::NotFound(format!("no face {}", face_id(face)))) };
    let FacePerson::Person(owner) = &found.person else {
        return Err(CmdError::BadParams(format!("face {} is not named: only a person's face can be their thumbnail", face_id(face))));
    };
    if let Some(person) = &p.person {
        known_person(lib, person)?;
        if person != owner {
            return Err(CmdError::BadParams(format!("face {} is not person {person}'s", face_id(face))));
        }
    }
    let key = FaceKey { photo: face.photo, rect: found.rect };
    let owner = owner.clone();
    if lib.person(&owner).and_then(|p| p.thumbnail) == Some(key) {
        return Ok(json!({ "person": owner, "changed": false }));
    }
    s.apply(Op::SetPersonThumbnail { id: owner.clone(), thumbnail: Some(key) })?;
    Ok(json!({ "person": owner, "changed": true }))
}
