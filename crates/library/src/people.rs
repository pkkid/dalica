//! Faces, people and geotags in the model (`data-model.md` §2–§4, `organizing.md` §6–§7,
//! `faces.md` §5–§6).
//!
//! - **Faces** are a photo's [`Face`] list (at most [`MAX_FACES`]): a rectangle in the stored frame
//!   (`data-model.md` §3.3) and who it is ([`FacePerson`]: unnamed, ignored or a person). A photo
//!   with no face has no entry. [`Op::SetFaces`] sets whole lists; naming, ignoring, moving and
//!   removing faces are each one such op built by the caller.
//! - **People** ([`Person`]: a name unique ignoring case, a nickname, a thumbnail face) live in
//!   the model by [`PersonId`]. [`Op::AddPerson`], [`Op::RenamePerson`], [`Op::MergePeople`] (every
//!   face of one moves to the other, whose name is kept), [`Op::DeletePerson`] (its faces go back to
//!   Unnamed) and [`Op::SetPersonThumbnail`] each return their exact inverse. A face names only a
//!   person the model has, so a person's faces always come back with it.
//! - **Geotags** ([`Geo`]) are the model's word on where a photo was taken: a point (six decimals,
//!   as the sidecar stores it) or removed by the user. With no word of the model's the file's GPS
//!   (the search fact) counts; a word of the model's wins over the file's ([`Library::is_geotagged`]).
//! - **Ids** of new people are `p` and 12 hex digits drawn from the caller's random source
//!   ([`Library::new_person_id`]), as album ids are.
//!
//! Names never appear in an error message: errors name people by id only (`faces.md` §7).

use std::collections::BTreeSet;

use dalica_core::geo::LatLon;
use dalica_core::{FolderId, PersonId, PhotoId, Rect01};
use serde::{Deserialize, Serialize};

use crate::model::Library;
use crate::op::Op;
use crate::tags::fold;
use crate::{Error, Result};

/// The most faces one photo holds (`data-model.md` §3: more in a sidecar are kept and ignored).
pub const MAX_FACES: usize = 256;
/// The most bytes of a person's name or nickname (UTF-8).
pub const MAX_PERSON_NAME: usize = 1024;
/// The most draws [`Library::new_person_id`] makes before giving up.
pub const MAX_PERSON_ID_DRAWS: usize = 64;

/// Who a face is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FacePerson {
    /// Found, not named yet: it shows in Unnamed.
    #[default]
    Unnamed,
    /// Ignored by the user (× or Delete in Unnamed): shown only with Show Ignored Faces.
    Ignored,
    /// Named.
    Person(PersonId),
}

impl FacePerson {
    /// The person, when the face is named.
    pub fn person(&self) -> Option<&PersonId> {
        match self {
            FacePerson::Person(id) => Some(id),
            _ => None,
        }
    }
}

/// One face on a photo: where it is, in the stored frame, and who it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    pub rect: Rect01,
    #[serde(default)]
    pub person: FacePerson,
}

// A `Rect01` is always finite (never NaN), so equality is reflexive.
impl Eq for Face {}

impl Face {
    /// A face at `rect`, of `person`.
    pub fn new(rect: Rect01, person: FacePerson) -> Face {
        Face { rect, person }
    }

    /// Whether it is a face the Faces filter counts: named or unnamed, not ignored.
    pub fn counts(&self) -> bool {
        self.person != FacePerson::Ignored
    }
}

/// One face, by its photo and its place in the photo's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceRef {
    pub photo: PhotoId,
    pub index: usize,
}

/// A face named by its photo and its rectangle, as a person's thumbnail is kept: it stays the
/// same face when other faces of the photo come or go.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceKey {
    pub photo: PhotoId,
    pub rect: Rect01,
}

impl Eq for FaceKey {}

/// A person (`people.json`, mirrored by name in the sidecar of each folder they are in).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Person {
    /// Clean ([`clean_person_name`]), not empty, unique among the model's people ignoring case.
    pub name: String,
    /// Clean, or empty for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub nickname: String,
    /// The face Set as Person Thumbnail chose, if any; it must be one of the person's faces when
    /// set. [`Library::person_thumbnail`] falls back to their first face once it no longer is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<FaceKey>,
}

impl Person {
    /// A person called `name`, nothing else said.
    pub fn named(name: impl Into<String>) -> Person {
        Person { name: name.into(), ..Person::default() }
    }

    pub(crate) fn check(&self) -> Result<()> {
        check_name(&self.name, "person name")?;
        if !self.nickname.is_empty() {
            check_name(&self.nickname, "nickname")?;
        }
        Ok(())
    }
}

fn check_name(name: &str, what: &'static str) -> Result<()> {
    if name.len() > MAX_PERSON_NAME {
        return Err(Error::TooLong { what, max: MAX_PERSON_NAME });
    }
    if clean_person_name(name).as_deref() != Some(name) {
        return Err(Error::BadText { what, why: "empty, untrimmed or with control characters" });
    }
    Ok(())
}

/// A person's name or nickname as stored: control characters made spaces, trimmed, cut to
/// [`MAX_PERSON_NAME`] bytes at a character boundary; `None` when nothing is left.
pub fn clean_person_name(text: &str) -> Option<String> {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let trimmed = spaced.trim();
    let mut end = trimmed.len().min(MAX_PERSON_NAME);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let name = trimmed.get(..end).unwrap_or_default().trim_end();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// The person id the 48 low bits of `bits` spell: `p` and 12 lowercase hex digits.
pub fn person_id_from(bits: u64) -> Option<PersonId> {
    PersonId::parse(&format!("p{:012x}", bits & 0xffff_ffff_ffff)).ok()
}

/// The model's word on where a photo was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Geo {
    /// Here (stored at six decimals).
    At(LatLon),
    /// The user removed the location: the photo is not geotagged, whatever its file says.
    Removed,
}

impl Geo {
    /// The point, when there is one.
    pub fn point(&self) -> Option<LatLon> {
        match self {
            Geo::At(p) => Some(*p),
            Geo::Removed => None,
        }
    }
}

/// How [`Library::people_sorted`] orders people.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PeopleSort {
    /// By name ignoring case, A to Z.
    #[default]
    Name,
    /// By how many photos they are in, most first, then by name.
    Count,
}

/// The people ops (`op.rs` dispatches here).
impl Library {
    /// A new person id: `p` and 12 hex digits from `draw` (the caller's random source), drawn
    /// again while it names an existing person, at most [`MAX_PERSON_ID_DRAWS`] times.
    pub fn new_person_id(&self, mut draw: impl FnMut() -> u64) -> Result<PersonId> {
        for _ in 0..MAX_PERSON_ID_DRAWS {
            if let Some(id) = person_id_from(draw())
                && !self.model.people.contains_key(&id)
            {
                return Ok(id);
            }
        }
        Err(Error::TooMany { what: "person id clashes", max: MAX_PERSON_ID_DRAWS })
    }

    /// The person other than `except` whose name equals `name` ignoring case, if any.
    pub(crate) fn name_holder(&self, name: &str, except: Option<&PersonId>) -> Option<&PersonId> {
        let key = fold(name);
        self.model.people.iter().find(|(id, p)| Some(*id) != except && fold(&p.name) == key).map(|(id, _)| id)
    }

    fn check_person(&self, id: &PersonId) -> Result<()> {
        if self.model.people.contains_key(id) { Ok(()) } else { Err(Error::UnknownPerson(id.clone())) }
    }

    /// The photos holding a face of `id`, with their face lists.
    fn faces_of(&self, id: &PersonId) -> Vec<(PhotoId, Vec<Face>)> {
        self.model.faces.iter().filter(|(_, list)| list.iter().any(|f| f.person.person() == Some(id))).map(|(p, l)| (*p, l.clone())).collect()
    }

    /// The folders of `photos`.
    fn folders_of(&self, photos: impl Iterator<Item = PhotoId>) -> BTreeSet<FolderId> {
        photos.filter_map(|p| self.model.photos.get(&p).map(|p| p.folder)).collect()
    }

    /// [`Op::SetFaces`].
    pub(crate) fn set_faces(&mut self, faces: Vec<(PhotoId, Vec<Face>)>, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        for (id, list) in &faces {
            if !self.model.photos.contains_key(id) {
                return Err(Error::UnknownPhoto(*id));
            }
            if list.len() > MAX_FACES {
                return Err(Error::TooMany { what: "faces on a photo", max: MAX_FACES });
            }
            for person in list.iter().filter_map(|f| f.person.person()) {
                self.check_person(person)?;
            }
        }
        let mut undo = Vec::with_capacity(faces.len());
        for (id, list) in faces {
            touched.extend(self.model.photos.get(&id).map(|p| p.folder));
            let old = if list.is_empty() { self.model.faces.remove(&id) } else { self.model.faces.insert(id, list) };
            undo.push((id, old.unwrap_or_default()));
        }
        undo.reverse();
        Ok(Op::SetFaces { faces: undo })
    }

    /// [`Op::SetGeo`].
    pub(crate) fn set_geo(&mut self, geo: Vec<(PhotoId, Option<Geo>)>, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        if let Some((id, _)) = geo.iter().find(|(id, _)| !self.model.photos.contains_key(id)) {
            return Err(Error::UnknownPhoto(*id));
        }
        let mut undo = Vec::with_capacity(geo.len());
        for (id, g) in geo {
            touched.extend(self.model.photos.get(&id).map(|p| p.folder));
            let old = match g {
                Some(Geo::At(p)) => self.model.geo.insert(id, Geo::At(p.round6())),
                Some(Geo::Removed) => self.model.geo.insert(id, Geo::Removed),
                None => self.model.geo.remove(&id),
            };
            undo.push((id, old));
        }
        undo.reverse();
        Ok(Op::SetGeo { geo: undo })
    }

    /// [`Op::AddPerson`].
    pub(crate) fn add_person(&mut self, id: PersonId, person: Person) -> Result<Op> {
        if self.model.people.contains_key(&id) {
            return Err(Error::PersonExists(id));
        }
        person.check()?;
        if let Some(other) = self.name_holder(&person.name, None) {
            return Err(Error::PersonNameTaken(other.clone()));
        }
        // The thumbnail is not checked here: putting a deleted person back sets their faces after
        // the person, and a thumbnail that is no longer theirs is only passed over.
        self.model.people.insert(id.clone(), person);
        Ok(Op::DeletePerson { id })
    }

    /// [`Op::RenamePerson`]: the folders the person is in mirror the name, so they are touched.
    pub(crate) fn rename_person(&mut self, id: PersonId, name: String, nickname: String, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        let old = self.model.people.get(&id).cloned().ok_or_else(|| Error::UnknownPerson(id.clone()))?;
        let new = Person { name, nickname, thumbnail: old.thumbnail };
        new.check()?;
        if let Some(other) = self.name_holder(&new.name, Some(&id)) {
            return Err(Error::PersonNameTaken(other.clone()));
        }
        let photos: Vec<PhotoId> = self.faces_of(&id).into_iter().map(|(p, _)| p).collect();
        touched.extend(self.folders_of(photos.into_iter()));
        self.model.people.insert(id.clone(), new);
        Ok(Op::RenamePerson { id, name: old.name, nickname: old.nickname })
    }

    /// [`Op::MergePeople`]: every face of `from` becomes `into`'s, and `from` goes.
    pub(crate) fn merge_people(&mut self, from: PersonId, into: PersonId, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        self.check_person(&from)?;
        self.check_person(&into)?;
        if from == into {
            return Err(Error::MergeIntoItself(from));
        }
        let lists = self.faces_of(&from);
        let moved: Vec<(PhotoId, Vec<Face>)> = lists
            .iter()
            .map(|(p, list)| {
                let list = list
                    .iter()
                    .map(|f| if f.person.person() == Some(&from) { Face::new(f.rect, FacePerson::Person(into.clone())) } else { f.clone() })
                    .collect();
                (*p, list)
            })
            .collect();
        let restore = self.set_faces(moved, touched)?;
        let Some(info) = self.model.people.remove(&from) else { return Err(Error::UnknownPerson(from)) };
        Ok(with_faces(Op::AddPerson { id: from, person: info }, restore))
    }

    /// [`Op::DeletePerson`]: its faces go back to Unnamed.
    pub(crate) fn delete_person(&mut self, id: PersonId, touched: &mut BTreeSet<FolderId>) -> Result<Op> {
        self.check_person(&id)?;
        let unnamed: Vec<(PhotoId, Vec<Face>)> = self
            .faces_of(&id)
            .into_iter()
            .map(|(p, list)| {
                let list =
                    list.into_iter().map(|f| if f.person.person() == Some(&id) { Face::new(f.rect, FacePerson::Unnamed) } else { f }).collect();
                (p, list)
            })
            .collect();
        let restore = self.set_faces(unnamed, touched)?;
        let Some(info) = self.model.people.remove(&id) else { return Err(Error::UnknownPerson(id)) };
        Ok(with_faces(Op::AddPerson { id, person: info }, restore))
    }

    /// [`Op::SetPersonThumbnail`].
    pub(crate) fn set_person_thumbnail(&mut self, id: PersonId, thumbnail: Option<FaceKey>) -> Result<Op> {
        self.check_person(&id)?;
        if let Some(key) = &thumbnail {
            self.check_thumbnail(&id, key)?;
        }
        let old = match self.model.people.get_mut(&id) {
            Some(p) => std::mem::replace(&mut p.thumbnail, thumbnail),
            None => return Err(Error::UnknownPerson(id)),
        };
        Ok(Op::SetPersonThumbnail { id, thumbnail: old })
    }

    /// Checks that `key` is a face of person `id`.
    fn check_thumbnail(&self, id: &PersonId, key: &FaceKey) -> Result<()> {
        let theirs = self.model.faces.get(&key.photo).is_some_and(|l| l.iter().any(|f| f.rect == key.rect && f.person.person() == Some(id)));
        if theirs { Ok(()) } else { Err(Error::NotPersonsFace { person: id.clone(), photo: key.photo }) }
    }

    /// The photos whose faces `op` changes, for the last-changed stamp: a person merged away or
    /// deleted changes the photos holding their faces.
    pub(crate) fn photos_of_person<'a>(&'a self, id: &'a PersonId) -> impl Iterator<Item = PhotoId> + 'a {
        self.model.faces.iter().filter(move |(_, l)| l.iter().any(|f| f.person.person() == Some(id))).map(|(p, _)| *p)
    }
}

/// `add` then `restore` (the faces as they were) as one op, or `add` alone when no face changed.
fn with_faces(add: Op, restore: Op) -> Op {
    match &restore {
        Op::SetFaces { faces } if faces.is_empty() => add,
        _ => Op::Batch { ops: vec![add, restore] },
    }
}

/// The people a photo's faces name, each once, by id.
pub(crate) fn named_in(faces: &[Face]) -> Vec<PersonId> {
    let set: BTreeSet<&PersonId> = faces.iter().filter_map(|f| f.person.person()).collect();
    set.into_iter().cloned().collect()
}
