//! Typed access to the keys Dalica reads and writes (`data-model.md` §3): per photo (`star`,
//! `caption`, `tags`, `hidden`, `edits`, `redo`, `date` under `photos.<name>`) and per folder
//! (`description`, `date`, `place`, `hidden` under `"folder"`).
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

/// What a value at a known key should be, if the one there is not that.
fn photo_problem(key: &str, v: &Value) -> Option<&'static str> {
    let ok = match key {
        "star" | "hidden" => v.is_boolean(),
        "caption" => v.is_string(),
        "tags" | "edits" | "redo" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        "date" => v.as_str().is_some_and(|s| s.parse::<DateTime>().is_ok()),
        _ => true,
    };
    (!ok).then_some(match key {
        "star" | "hidden" => "true or false",
        "caption" => "a string",
        "date" => "a date and time (2026-06-04T19:42:10)",
        _ => "a list of strings",
    })
}

fn folder_problem(key: &str, v: &Value) -> Option<&'static str> {
    let ok = match key {
        "hidden" => v.is_boolean(),
        "description" | "place" => v.is_string(),
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
                for key in FOLDER_KEYS {
                    if let Some(expected) = folder.get(key).and_then(|v| folder_problem(key, v)) {
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
                            for key in PHOTO_KEYS {
                                if let Some(expected) = e.get(key).and_then(|v| photo_problem(key, v)) {
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
}
