//! Typed access to the keys Dalica reads and writes (`data-model.md` §3): per photo (`star`,
//! `caption`, `tags`, `hidden`, `edits`, `redo`, `date` under `photos.<name>`) and per folder
//! (`description`, `date`, `place`, `hidden` under `"folder"`). M8 adds `savedAs` per photo (an
//! object: the copy's file `name` and the `recipe` it holds) and `project` in `"folder"`
//! ([`M8_PHOTO_KEYS`], [`M8_FOLDER_KEYS`]); writing `savedAs` keeps the keys of its object that
//! Dalica does not know.
//!
//! Reading never fails: each key is [`Field::Absent`], a value of the right type, or
//! [`Field::WrongType`] holding what is there (kept, reported by [`Sidecar::problems`]). A date
//! that does not parse counts as a value of the wrong type. Writing sets one key or clears it:
//! clearing removes the key, then the photo's entry or the `"folder"` object when nothing else is
//! left in it, and never creates anything. A write is refused ([`WrongType`]) when the object it
//! goes into is not an object; the value there is kept.

use dalica_core::{Date, DateTime};
use serde_json::{Map, Value};

use crate::{Field, Problem, Sidecar, WrongType, escape};

/// The keys of a photo's entry that Dalica reads, in the documented order of `data-model.md` §3.
pub const PHOTO_KEYS: [&str; 7] = ["star", "caption", "tags", "hidden", "edits", "redo", "date"];
/// The keys of the `"folder"` object that Dalica reads.
pub const FOLDER_KEYS: [&str; 4] = ["description", "date", "place", "hidden"];
/// The key of the folder object.
pub const FOLDER: &str = "folder";
/// The photo keys M8 adds (`data-model.md` §3, §6): the copy a Save wrote beside the photo.
pub const M8_PHOTO_KEYS: [&str; 1] = [SAVED_AS];
/// The `"folder"` keys M8 adds: the project mark (`"project": "exported"`).
pub const M8_FOLDER_KEYS: [&str; 1] = [PROJECT];
/// A photo's copy saved beside it.
pub const SAVED_AS: &str = "savedAs";
/// A folder's project mark, in `"folder"`.
pub const PROJECT: &str = "project";

/// What a photo's `savedAs` says: the JPEG a Save wrote beside it (a file name in the same folder,
/// as written: whether it may reach a file is the reader's to check, [`crate::plain_name`]) and
/// the recipe it was rendered with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedAs {
    pub name: String,
    pub recipe: Vec<String>,
}

/// What a photo's entry says, key by key.
#[derive(Clone, Debug, PartialEq)]
pub struct PhotoFields {
    pub star: Field<bool>,
    pub caption: Field<String>,
    pub tags: Field<Vec<String>>,
    pub hidden: Field<bool>,
    pub edits: Field<Vec<String>>,
    pub redo: Field<Vec<String>>,
    pub date: Field<DateTime>,
}

/// What the `"folder"` object says, key by key.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderFields {
    pub description: Field<String>,
    pub date: Field<Date>,
    pub place: Field<String>,
    pub hidden: Field<bool>,
}

/// One value for a photo's entry. The empty value (`false`, `""`, `[]`, `None`) clears the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhotoValue {
    Star(bool),
    Caption(String),
    Tags(Vec<String>),
    Hidden(bool),
    Edits(Vec<String>),
    Redo(Vec<String>),
    Date(Option<DateTime>),
}

/// One value for the `"folder"` object. The empty value clears the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderValue {
    Description(String),
    Date(Option<Date>),
    Place(String),
    Hidden(bool),
}

impl PhotoValue {
    /// The key this value goes to.
    pub fn key(&self) -> &'static str {
        self.entry().0
    }

    /// The key and the JSON value to store (`None`: clear the key).
    fn entry(&self) -> (&'static str, Option<Value>) {
        match self {
            PhotoValue::Star(b) => ("star", flag(*b)),
            PhotoValue::Caption(s) => ("caption", text(s)),
            PhotoValue::Tags(l) => ("tags", list(l)),
            PhotoValue::Hidden(b) => ("hidden", flag(*b)),
            PhotoValue::Edits(l) => ("edits", list(l)),
            PhotoValue::Redo(l) => ("redo", list(l)),
            PhotoValue::Date(d) => ("date", d.map(|d| Value::String(d.to_string()))),
        }
    }
}

impl FolderValue {
    /// The key this value goes to under `"folder"`.
    pub fn key(&self) -> &'static str {
        self.entry().0
    }

    fn entry(&self) -> (&'static str, Option<Value>) {
        match self {
            FolderValue::Description(s) => ("description", text(s)),
            FolderValue::Date(d) => ("date", d.map(|d| Value::String(d.to_string()))),
            FolderValue::Place(s) => ("place", text(s)),
            FolderValue::Hidden(b) => ("hidden", flag(*b)),
        }
    }
}

fn flag(on: bool) -> Option<Value> {
    on.then_some(Value::Bool(true))
}

fn text(s: &str) -> Option<Value> {
    (!s.is_empty()).then(|| Value::String(s.to_string()))
}

fn list(l: &[String]) -> Option<Value> {
    (!l.is_empty()).then(|| Value::Array(l.iter().cloned().map(Value::String).collect()))
}

pub(crate) fn read_bool(v: Option<&Value>) -> Field<bool> {
    match v {
        None => Field::Absent,
        Some(Value::Bool(b)) => Field::Value(*b),
        Some(other) => Field::WrongType(other.clone()),
    }
}

fn read_text(v: Option<&Value>) -> Field<String> {
    match v {
        None => Field::Absent,
        Some(Value::String(s)) => Field::Value(s.clone()),
        Some(other) => Field::WrongType(other.clone()),
    }
}

fn read_list(v: Option<&Value>) -> Field<Vec<String>> {
    match v {
        None => Field::Absent,
        Some(Value::Array(a)) => match a.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<Vec<String>>>() {
            Some(l) => Field::Value(l),
            None => Field::WrongType(Value::Array(a.clone())),
        },
        Some(other) => Field::WrongType(other.clone()),
    }
}

fn read_parsed<T: std::str::FromStr>(v: Option<&Value>) -> Field<T> {
    match v {
        None => Field::Absent,
        Some(Value::String(s)) => match s.parse() {
            Ok(d) => Field::Value(d),
            Err(_) => Field::WrongType(Value::String(s.clone())),
        },
        Some(other) => Field::WrongType(other.clone()),
    }
}

/// `savedAs`: an object with a non-empty string `name` and, if present, a list of strings
/// `recipe` (absent reads as empty); other keys in it are allowed and kept.
fn read_saved_as(v: Option<&Value>) -> Field<SavedAs> {
    let Some(v) = v else { return Field::Absent };
    let wrong = || Field::WrongType(v.clone());
    let Some(o) = v.as_object() else { return wrong() };
    let name = match o.get("name") {
        Some(Value::String(n)) if !n.is_empty() => n.clone(),
        _ => return wrong(),
    };
    let recipe = match read_list(o.get("recipe")) {
        Field::Absent => Vec::new(),
        Field::Value(l) => l,
        Field::WrongType(_) => return wrong(),
    };
    Field::Value(SavedAs { name, recipe })
}

/// What a value at a known key should be, if the one there is not that.
fn photo_problem(key: &str, v: &Value) -> Option<&'static str> {
    let ok = match key {
        "star" | "hidden" => v.is_boolean(),
        "caption" => v.is_string(),
        "tags" | "edits" | "redo" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        "date" => v.as_str().is_some_and(|s| s.parse::<DateTime>().is_ok()),
        SAVED_AS => matches!(read_saved_as(Some(v)), Field::Value(_)),
        _ => true,
    };
    (!ok).then_some(match key {
        "star" | "hidden" => "true or false",
        "caption" => "a string",
        "date" => "a date and time (2026-06-04T19:42:10)",
        SAVED_AS => "an object with a file name (\"name\") and a list of strings (\"recipe\")",
        _ => "a list of strings",
    })
}

fn folder_problem(key: &str, v: &Value) -> Option<&'static str> {
    let ok = match key {
        "hidden" => v.is_boolean(),
        "description" | "place" | PROJECT => v.is_string(),
        "date" => v.as_str().is_some_and(|s| s.parse::<Date>().is_ok()),
        _ => true,
    };
    (!ok).then_some(match key {
        "hidden" => "true or false",
        "date" => "a date (2026-06-04)",
        _ => "a string",
    })
}

/// Sets or clears `key` in `object` (`None` removes it); reports whether the object is empty
/// afterwards.
fn put(object: &mut Map<String, Value>, key: &str, value: Option<Value>) -> bool {
    match value {
        Some(v) => {
            object.insert(key.to_string(), v);
        }
        None => {
            object.remove(key);
        }
    }
    object.is_empty()
}

impl Sidecar {
    /// The entry of photo `name` as an object, if it is one.
    pub(crate) fn photo_entry(&self, name: &str) -> Option<&Map<String, Value>> {
        self.root.get("photos")?.as_object()?.get(name)?.as_object()
    }

    /// The `"folder"` object, if it is one.
    fn folder_object(&self) -> Option<&Map<String, Value>> {
        self.root.get(FOLDER)?.as_object()
    }

    /// Every key Dalica reads in photo `name`'s entry (all absent when there is no entry, or it is
    /// not an object).
    pub fn photo(&self, name: &str) -> PhotoFields {
        let e = self.photo_entry(name);
        let get = |k: &str| e.and_then(|e| e.get(k));
        PhotoFields {
            star: read_bool(get("star")),
            caption: read_text(get("caption")),
            tags: read_list(get("tags")),
            hidden: read_bool(get("hidden")),
            edits: read_list(get("edits")),
            redo: read_list(get("redo")),
            date: read_parsed(get("date")),
        }
    }

    /// Every key Dalica reads in the `"folder"` object (all absent when there is none, or it is
    /// not an object).
    pub fn folder(&self) -> FolderFields {
        let f = self.folder_object();
        let get = |k: &str| f.and_then(|f| f.get(k));
        FolderFields {
            description: read_text(get("description")),
            date: read_parsed(get("date")),
            place: read_text(get("place")),
            hidden: read_bool(get("hidden")),
        }
    }

    /// Sets or clears one key of photo `name`'s entry. Setting creates the entry (and `photos`);
    /// clearing removes the key, and the entry when nothing else is left in it, and never creates
    /// anything. Refused if `photos` or the entry is not an object.
    pub fn set_photo(&mut self, name: &str, value: &PhotoValue) -> Result<(), WrongType> {
        let (key, value) = value.entry();
        let entry_pointer = || format!("/photos/{}", escape(name));
        if value.is_none() {
            match self.root.get_mut("photos") {
                None => return Ok(()),
                Some(Value::Object(photos)) => match photos.get_mut(name) {
                    None => return Ok(()),
                    Some(Value::Object(entry)) => {
                        if put(entry, key, None) {
                            photos.remove(name);
                        }
                        return Ok(());
                    }
                    Some(_) => return Err(WrongType { pointer: entry_pointer() }),
                },
                Some(_) => return Err(WrongType { pointer: "/photos".into() }),
            }
        }
        let photos = self.root.entry("photos").or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(photos) = photos else { return Err(WrongType { pointer: "/photos".into() }) };
        let entry = photos.entry(name).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(entry) = entry else { return Err(WrongType { pointer: entry_pointer() }) };
        put(entry, key, value);
        Ok(())
    }

    /// Sets or clears one key of the `"folder"` object, the same way as [`Sidecar::set_photo`]:
    /// clearing the last key removes the object. Refused if `"folder"` is not an object.
    pub fn set_folder(&mut self, value: &FolderValue) -> Result<(), WrongType> {
        let (key, value) = value.entry();
        let refused = || WrongType { pointer: format!("/{FOLDER}") };
        match (self.root.get_mut(FOLDER), value) {
            (None, None) => Ok(()),
            (Some(Value::Object(folder)), value) => {
                if put(folder, key, value) {
                    self.root.remove(FOLDER);
                }
                Ok(())
            }
            (Some(_), _) => Err(refused()),
            (None, Some(v)) => {
                let mut folder = Map::new();
                folder.insert(key.to_string(), v);
                self.root.insert(FOLDER.into(), Value::Object(folder));
                Ok(())
            }
        }
    }

    /// The wrong-type values of the photo entries and the folder object, after `/dalica`'s.
    pub(crate) fn field_problems(&self, out: &mut Vec<Problem>) {
        match self.root.get(FOLDER) {
            None => {}
            Some(Value::Object(folder)) => {
                for key in FOLDER_KEYS.iter().chain(&M8_FOLDER_KEYS) {
                    if let Some(expected) = folder.get(*key).and_then(|v| folder_problem(key, v)) {
                        out.push(Problem { pointer: format!("/{FOLDER}/{key}"), expected });
                    }
                }
            }
            Some(_) => out.push(Problem { pointer: format!("/{FOLDER}"), expected: "an object" }),
        }
        match self.root.get("photos") {
            None => {}
            Some(Value::Object(photos)) => {
                for (name, entry) in photos {
                    let at = format!("/photos/{}", escape(name));
                    match entry {
                        Value::Object(e) => {
                            for key in PHOTO_KEYS.iter().chain(&M8_PHOTO_KEYS) {
                                if let Some(expected) = e.get(*key).and_then(|v| photo_problem(key, v)) {
                                    out.push(Problem { pointer: format!("{at}/{key}"), expected });
                                }
                            }
                        }
                        _ => out.push(Problem { pointer: at, expected: "an object" }),
                    }
                }
            }
            Some(_) => out.push(Problem { pointer: "/photos".into(), expected: "an object" }),
        }
    }

    /// Photo `name`'s `savedAs`: absent, the copy saved beside it, or a value of another shape
    /// (kept, not used).
    pub fn saved_as(&self, name: &str) -> Field<SavedAs> {
        read_saved_as(self.photo_entry(name).and_then(|e| e.get(SAVED_AS)))
    }

    /// Sets or clears photo `name`'s `savedAs`. Setting writes `name` and `recipe` into the object
    /// already there (its other keys are kept; an empty recipe is written as no key) or a new one,
    /// creating the entry if needed; clearing removes the key, and the entry when nothing else is
    /// left in it. Refused if `photos`, the entry or the `savedAs` there is not an object (a value
    /// of the wrong type is replaced only by clearing it first).
    pub fn set_saved_as(&mut self, name: &str, value: Option<&SavedAs>) -> Result<(), WrongType> {
        let entry_pointer = || format!("/photos/{}", escape(name));
        let Some(value) = value else {
            match self.root.get_mut("photos") {
                None => return Ok(()),
                Some(Value::Object(photos)) => match photos.get_mut(name) {
                    None => return Ok(()),
                    Some(Value::Object(entry)) => {
                        if put(entry, SAVED_AS, None) {
                            photos.remove(name);
                        }
                        return Ok(());
                    }
                    Some(_) => return Err(WrongType { pointer: entry_pointer() }),
                },
                Some(_) => return Err(WrongType { pointer: "/photos".into() }),
            }
        };
        let photos = self.root.entry("photos").or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(photos) = photos else { return Err(WrongType { pointer: "/photos".into() }) };
        let entry = photos.entry(name).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(entry) = entry else { return Err(WrongType { pointer: entry_pointer() }) };
        let object = entry.entry(SAVED_AS).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(object) = object else { return Err(WrongType { pointer: format!("{}/{SAVED_AS}", entry_pointer()) }) };
        object.insert("name".into(), Value::String(value.name.clone()));
        put(object, "recipe", list(&value.recipe));
        Ok(())
    }

    /// The `"folder"` object's `project` mark, as written (`"exported"` is the one Dalica knows).
    pub fn project(&self) -> Field<String> {
        read_text(self.folder_object().and_then(|f| f.get(PROJECT)))
    }

    /// Sets or clears the folder's `project` mark, the way [`Sidecar::set_folder`] sets its keys.
    pub fn set_project(&mut self, project: Option<&str>) -> Result<(), WrongType> {
        let value = project.and_then(text);
        let refused = || WrongType { pointer: format!("/{FOLDER}") };
        match (self.root.get_mut(FOLDER), value) {
            (None, None) => Ok(()),
            (Some(Value::Object(folder)), value) => {
                if put(folder, PROJECT, value) {
                    self.root.remove(FOLDER);
                }
                Ok(())
            }
            (Some(_), _) => Err(refused()),
            (None, Some(v)) => {
                let mut folder = Map::new();
                folder.insert(PROJECT.into(), v);
                self.root.insert(FOLDER.into(), Value::Object(folder));
                Ok(())
            }
        }
    }
}
