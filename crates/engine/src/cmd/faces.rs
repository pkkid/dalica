//! Faces as data (`organizing.md` §6, `faces.md` §9): `faces.list {person | unnamed | ids,
//! ignored?}`, `faces.name {faces, person | name, create?}`, `faces.ignore {faces}`, `faces.move
//! {faces, person}`, `faces.remove {faces}` (back to Unnamed) and `faces.add {id, rect | displayed,
//! person?}` (a face drawn by hand), each change one library undo step. No model is needed: these
//! work on the rectangles and names the sidecars hold, whoever found them.
//!
//! - **Faces are named** `"<PhotoId>.<n>"` (`"12.0"`), the photo and the face's place in its list,
//!   as the window's widget ids name them (`face:12.0`, `control-protocol.md` §7). An id that is
//!   not one is `bad_params`; a photo or a place the library does not have is `not_found`.
//! - **Rectangles** are stored in the stored frame (`data-model.md` §3.3: after the EXIF
//!   orientation, before the recipe's rotations and flips). `faces.list` gives each face's `rect`
//!   in that frame and `displayed`, the same rectangle as the photo shows turned by its recipe;
//!   `faces.add` takes either (`rect` stored, `displayed` as drawn on the photo shown) and stores
//!   the stored one, so a box drawn on a photo turned 90° lands on the same face after a restart.
//! - **Naming** (`faces.name`): a person by id, or a name: a name another person holds, ignoring
//!   case, assigns to that person; an unknown name asks New Person first (`disabled` with
//!   [`NEEDS_PERSON`]) unless `create` is true, which makes the person (with `nickname`) and names
//!   the faces in the same undo step. Naming a group of faces is one step whatever its size.
//! - Errors and logs never hold a name, a rectangle or an embedding (`faces.md` §7): people are
//!   named by id, faces by their id.
//!
//! People themselves (create, rename, merge, delete, thumbnails, the list) are `cmd/people.rs`.

use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;

use dalica_core::{PersonId, PhotoId, Rect01};
use dalica_edit::Dihedral;
use dalica_edit::frame::Rect as FrameRect;
use dalica_library::{Face, FacePerson, FaceRef, Library, MAX_FACES, Op, Person, clean_person_name};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;

/// The most faces one command names.
pub const MAX_FACE_IDS: usize = 100_000;
/// The faces `faces.list` gives by default, and at most.
pub const DEFAULT_LIMIT: usize = 1_000;
pub const MAX_LIMIT: usize = 10_000;
/// The farthest a page of `faces.list` may start.
pub const MAX_OFFSET: usize = 1_000_000_000;

/// What `faces.name` answers for a name no person has, without `create` (New Person asks for it
/// first, `organizing.md` §6.2). Over automation it is the `disabled` error's text; the name is
/// never repeated in it.
pub const NEEDS_PERSON: &str =
    "needs_person: no person has that name; New Person asks for it first (Name, Nickname, OK); give \"create\": true to make them";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "faces.list",
            label: "List Faces",
            menu: "",
            keys: &[],
            params: r#"{"person"?:PersonId | "unnamed"?:true | "ids"?:[PhotoId] (at most one; none: every face), "ignored"?:bool (also the ignored faces; default false), "offset"?:int=0, "limit"?:int=1000 (at most 10000)}: faces in photo order, then their order on the photo: {faces:[{id:"<PhotoId>.<n>", photo, index, rect:[x0,y0,x1,y1] (stored frame), displayed:[x0,y0,x1,y1] (as the photo shows, turned by its recipe), person:PersonId|null, name:string|null, ignored, group:null (the group of similar unnamed faces once the face index has one)}], total, offset, limit}. Over automation without --automation-allow-people names are null and rectangles left out"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "faces.name",
            label: "Name Faces",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"], "person"?:PersonId | "name"?:string (exactly one), "create"?:bool, "nickname"?:string (with create)}: names the faces: a name another person holds, ignoring case, is that person; an unknown name is disabled ("needs_person": New Person asks first) unless create is true, which makes the person in the same undo step. Replies {person, created, changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_faces,
            run: name,
        },
        CommandSpec {
            id: "faces.ignore",
            label: "Ignore Faces",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"]}: the faces are ignored (shown only with Show Ignored Faces, never found again). Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_faces,
            run: ignore,
        },
        CommandSpec {
            id: "faces.move",
            label: "Move to Person",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"], "person":PersonId}: the faces become that person's. Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_faces,
            run: move_to,
        },
        CommandSpec {
            id: "faces.remove",
            label: "Remove from Person",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"]}: the faces go back to Unnamed. Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_faces,
            run: remove,
        },
        CommandSpec {
            id: "faces.add",
            label: "Add a Person Manually",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "rect"?:[x0,y0,x1,y1] (stored frame) | "displayed"?:[x0,y0,x1,y1] (as drawn on the photo shown, turned by its recipe) (exactly one; normalised 0..1, not empty), "person"?:PersonId}: a face drawn by hand, unnamed or that person's (at most 256 per photo; the same rectangle twice is refused). Replies {face:"<PhotoId>.<n>", rect (stored frame)}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: add,
        },
    ]
}

/// Disabled with nothing to act on: an empty `faces` list.
fn has_faces(_: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("faces").and_then(Value::as_array) {
        Some(list) if list.is_empty() => Err(Disabled("Select some faces first.".into())),
        _ => Ok(()),
    }
}

/// `"12.0"` → the face's photo and place, without checking that the library has it.
pub(crate) fn parse_face(text: &str) -> Result<FaceRef, CmdError> {
    let bad = || {
        let shown: String = text.chars().take(40).collect();
        CmdError::BadParams(format!("a face is \"<PhotoId>.<n>\" (\"12.0\"), not {shown:?}"))
    };
    let (photo, index) = text.split_once('.').ok_or_else(bad)?;
    let photo: PhotoId = photo.parse().map_err(|_| bad())?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let index: usize = index.parse().map_err(|_| bad())?;
    Ok(FaceRef { photo, index })
}

/// The id of face `r`: `"12.0"`.
pub(crate) fn face_id(r: FaceRef) -> String {
    format!("{}.{}", r.photo, r.index)
}

/// The faces `texts` name, each once in the order first given, every one a face the library has.
pub(crate) fn known_faces(lib: &Library, texts: &[String]) -> Result<Vec<FaceRef>, CmdError> {
    if texts.len() > MAX_FACE_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_FACE_IDS} faces at once")));
    }
    let mut out = Vec::with_capacity(texts.len());
    let mut seen = std::collections::HashSet::with_capacity(texts.len());
    for t in texts {
        let r = parse_face(t)?;
        if lib.photo(r.photo).is_none() {
            return Err(CmdError::NotFound(format!("no photo {}", r.photo)));
        }
        if lib.face(r).is_none() {
            return Err(CmdError::NotFound(format!("photo {} has no face {}", r.photo, r.index)));
        }
        if seen.insert(r) {
            out.push(r);
        }
    }
    Ok(out)
}

/// The person `id`, when the library has them.
pub(crate) fn known_person(lib: &Library, id: &PersonId) -> Result<(), CmdError> {
    if lib.person(id).is_some() { Ok(()) } else { Err(CmdError::NotFound(format!("no person {id}"))) }
}

/// The op giving each of `faces` to `who`, grouped per photo (one `SetFaces`), and how many faces
/// it changes; `None` when none would change.
pub(crate) fn reassign(lib: &Library, faces: &[FaceRef], who: &FacePerson) -> (Option<Op>, usize) {
    let mut lists: BTreeMap<PhotoId, Vec<Face>> = BTreeMap::new();
    let mut changed = 0usize;
    for r in faces {
        let list = lists.entry(r.photo).or_insert_with(|| lib.faces(r.photo).to_vec());
        if let Some(face) = list.get_mut(r.index)
            && face.person != *who
        {
            face.person = who.clone();
            changed += 1;
        }
    }
    if changed == 0 {
        return (None, 0);
    }
    let faces: Vec<(PhotoId, Vec<Face>)> = lists.into_iter().filter(|(p, list)| lib.faces(*p) != list.as_slice()).collect();
    (Some(Op::SetFaces { faces }), changed)
}

/// A random 64-bit number: the standard library's hasher keys, seeded by the operating system and
/// different for every call (as album ids are drawn, `cmd/albums.rs`).
pub(crate) fn draw() -> u64 {
    std::collections::hash_map::RandomState::new().hash_one(0u8)
}

/// A clean name or nickname for a person, or why not (the text itself is never in the message).
pub(crate) fn clean_name(text: &str, what: &str) -> Result<String, CmdError> {
    if text.len() > dalica_library::MAX_PERSON_NAME {
        return Err(CmdError::Limit(format!("a {what} is at most {} bytes", dalica_library::MAX_PERSON_NAME)));
    }
    clean_person_name(text).ok_or_else(|| CmdError::BadParams(format!("the {what} is empty")))
}

/// The recipe's net rotation and flip of photo `id` (identity when it has none or it does not
/// parse).
pub(crate) fn turn_of(lib: &Library, id: PhotoId) -> Dihedral {
    dalica_edit::Recipe::parse(&lib.info(id).edits).map(|r| r.orientation()).unwrap_or(Dihedral::IDENTITY)
}

/// `r` as the photo shows it, turned by `turn`.
pub(crate) fn displayed(r: &Rect01, turn: Dihedral) -> [f64; 4] {
    let d = FrameRect::new(r.x0(), r.y0(), r.x1(), r.y1()).to_display(turn);
    [d.x0, d.y0, d.x1, d.y1].map(|v| (v * 1e6).round() / 1e6)
}

/// A face as `faces.list` gives it.
pub(crate) fn face_json(lib: &Library, r: FaceRef, face: &Face, turn: Dihedral) -> Value {
    let (person, name) = match &face.person {
        FacePerson::Person(id) => (json!(id), json!(lib.person(id).map(|p| p.name.as_str()))),
        _ => (Value::Null, Value::Null),
    };
    json!({
        "id": face_id(r), "photo": r.photo, "index": r.index, "rect": face.rect, "displayed": displayed(&face.rect, turn),
        "person": person, "name": name, "ignored": face.person == FacePerson::Ignored, "group": Value::Null,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    person: Option<PersonId>,
    unnamed: Option<bool>,
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    ignored: bool,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: ListParams = params(p)?;
    let selectors = usize::from(p.person.is_some()) + usize::from(p.unnamed.is_some()) + usize::from(p.ids.is_some());
    if selectors > 1 {
        return Err(CmdError::BadParams("give at most one of person, unnamed and ids".into()));
    }
    let (offset, limit) = (p.offset.unwrap_or(0), p.limit.unwrap_or(DEFAULT_LIMIT));
    if offset > MAX_OFFSET {
        return Err(CmdError::BadParams(format!("offset is at most {MAX_OFFSET}")));
    }
    if limit > MAX_LIMIT {
        return Err(CmdError::BadParams(format!("limit is at most {MAX_LIMIT}")));
    }
    let lib = s.library();
    let all = |photos: &mut dyn Iterator<Item = PhotoId>| -> Vec<FaceRef> {
        photos
            .flat_map(|id| {
                lib.faces(id).iter().enumerate().filter(|(_, f)| p.ignored || f.counts()).map(move |(index, _)| FaceRef { photo: id, index })
            })
            .collect()
    };
    let refs: Vec<FaceRef> = if let Some(person) = &p.person {
        known_person(lib, person)?;
        lib.person_faces(person).to_vec()
    } else if p.unnamed == Some(true) {
        let mut v = lib.unnamed_faces().to_vec();
        if p.ignored {
            v.extend_from_slice(lib.ignored_faces());
        }
        v
    } else if let Some(ids) = p.ids {
        let ids = crate::cmd::photo::known(s, ids)?;
        all(&mut ids.into_iter())
    } else {
        // Every face, in the order of the photos with faces (by date, like a folder), then the
        // ignored ones' photos.
        let mut photos: Vec<PhotoId> = lib.photos_with_faces().to_vec();
        if p.ignored {
            let known: std::collections::HashSet<PhotoId> = photos.iter().copied().collect();
            let mut more: Vec<PhotoId> = lib.ignored_faces().iter().map(|r| r.photo).filter(|id| !known.contains(id)).collect();
            more.dedup();
            photos.extend(more);
        }
        all(&mut photos.into_iter())
    };
    let mut turns: HashMap<PhotoId, Dihedral> = HashMap::new();
    let page: Vec<Value> = refs
        .iter()
        .skip(offset)
        .take(limit)
        .filter_map(|r| {
            let face = lib.face(*r)?;
            let turn = *turns.entry(r.photo).or_insert_with(|| turn_of(lib, r.photo));
            Some(face_json(lib, *r, face, turn))
        })
        .collect();
    Ok(json!({ "faces": page, "total": refs.len(), "offset": offset, "limit": limit }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FacesParams {
    faces: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameParams {
    faces: Vec<String>,
    person: Option<PersonId>,
    name: Option<String>,
    #[serde(default)]
    create: bool,
    nickname: Option<String>,
}

fn name(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: NameParams = params(p)?;
    let faces = known_faces(s.library(), &p.faces)?;
    let (person, add) = match (p.person, p.name) {
        (Some(_), Some(_)) | (None, None) => return Err(CmdError::BadParams("give person or name, exactly one".into())),
        (Some(id), None) => {
            if p.nickname.is_some() {
                return Err(CmdError::BadParams("nickname goes with a new name only".into()));
            }
            known_person(s.library(), &id)?;
            (id, None)
        }
        (None, Some(text)) => {
            let name = clean_name(&text, "name")?;
            let nickname = p.nickname.as_deref().map(|n| clean_person_name(n).unwrap_or_default()).unwrap_or_default();
            if nickname.len() > dalica_library::MAX_PERSON_NAME {
                return Err(CmdError::Limit(format!("a nickname is at most {} bytes", dalica_library::MAX_PERSON_NAME)));
            }
            match s.library().person_by_name(&name) {
                Some(id) => (id.clone(), None),
                None if !p.create => return Err(CmdError::Disabled(NEEDS_PERSON.into())),
                None => {
                    let id = s.library().new_person_id(draw).map_err(|e| CmdError::Limit(e.to_string()))?;
                    (id.clone(), Some(Op::AddPerson { id, person: Person { name, nickname, thumbnail: None } }))
                }
            }
        }
    };
    let (set, changed) = reassign(s.library(), &faces, &FacePerson::Person(person.clone()));
    let created = add.is_some();
    let op = match (add, set) {
        (Some(add), Some(set)) => Some(Op::Batch { ops: vec![add, set] }),
        (Some(add), None) => Some(add),
        (None, set) => set,
    };
    if let Some(op) = op {
        s.apply(op)?;
    }
    Ok(json!({ "person": person, "created": created, "changed": changed }))
}

/// Gives `faces` to `who` as one op; replies how many changed.
fn set_all(s: &mut Session, faces: Vec<String>, who: FacePerson) -> Result<Value, CmdError> {
    let faces = known_faces(s.library(), &faces)?;
    let (op, changed) = reassign(s.library(), &faces, &who);
    if let Some(op) = op {
        s.apply(op)?;
    }
    Ok(json!({ "changed": changed }))
}

fn ignore(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: FacesParams = params(p)?;
    set_all(s, p.faces, FacePerson::Ignored)
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: FacesParams = params(p)?;
    set_all(s, p.faces, FacePerson::Unnamed)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveParams {
    faces: Vec<String>,
    person: PersonId,
}

fn move_to(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: MoveParams = params(p)?;
    known_person(s.library(), &p.person)?;
    set_all(s, p.faces, FacePerson::Person(p.person))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddParams {
    id: PhotoId,
    rect: Option<[f64; 4]>,
    displayed: Option<[f64; 4]>,
    person: Option<PersonId>,
}

fn add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: AddParams = params(p)?;
    let lib = s.library();
    if lib.photo(p.id).is_none() {
        return Err(CmdError::NotFound(format!("no photo {}", p.id)));
    }
    let [x0, y0, x1, y1] = match (p.rect, p.displayed) {
        (Some(r), None) => r,
        (None, Some(d)) => {
            if !d.iter().all(|v| v.is_finite()) {
                return Err(CmdError::BadParams("displayed must be four finite numbers".into()));
            }
            let s = FrameRect::new(d[0], d[1], d[2], d[3]).ordered().to_stored(turn_of(lib, p.id));
            [s.x0, s.y0, s.x1, s.y1]
        }
        _ => return Err(CmdError::BadParams("give rect or displayed, exactly one".into())),
    };
    let rect = Rect01::new(x0, y0, x1, y1).map_err(|e| CmdError::BadParams(format!("rect: {e}")))?;
    if let Some(person) = &p.person {
        known_person(lib, person)?;
    }
    let mut list = lib.faces(p.id).to_vec();
    if list.len() >= MAX_FACES {
        return Err(CmdError::Limit(format!("a photo holds at most {MAX_FACES} faces")));
    }
    if list.iter().any(|f| f.rect == rect) {
        return Err(CmdError::BadParams("the photo has a face at that rectangle already".into()));
    }
    let who = p.person.map(FacePerson::Person).unwrap_or_default();
    list.push(Face::new(rect, who));
    let index = list.len() - 1;
    s.apply(Op::SetFaces { faces: vec![(p.id, list)] })?;
    Ok(json!({ "face": face_id(FaceRef { photo: p.id, index }), "rect": rect }))
}
