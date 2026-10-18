//! Typed access to the keys Dalica reads and writes (`data-model.md` §3): per photo (`star`,
//! `caption`, `tags`, `hidden`, `edits`, `redo`, `date` under `photos.<name>`) and per folder
//! (`description`, `date`, `place`, `hidden` under `"folder"`). M8 adds `savedAs` per photo (an
//! object: the copy's file `name` and the `recipe` it holds) and `project` in `"folder"`
//! ([`M8_PHOTO_KEYS`], [`M8_FOLDER_KEYS`]); writing `savedAs` keeps the keys of its object that
//! Dalica does not know.
//!
//! M9 adds a photo's `albums` (an object from album id to position key, `data-model.md` §3.2), the
//! folder's `sort` and `reverse` in `"folder"`, and the top-level `albums` object (album id →
//! `{"name": …}`, the names of the albums with members in the folder) ([`M9_PHOTO_KEYS`],
//! [`M9_FOLDER_KEYS`], [`M9_TOP_KEYS`]). An entry of either `albums` object whose id is not an
//! album id, or whose key or name is not usable, is kept verbatim: reading leaves it out and lists
//! it ([`PhotoAlbums::bad`], [`AlbumNames::bad`], for the reader to report), and writing never
//! touches it. An `albums` that is not an object is a [`Problem`].
//!
//! M10 adds a photo's `changed`, the UTC time Dalica last changed what is said about it
//! (`"2026-10-15T09:41:07.250Z"`, milliseconds always written; [`M10_PHOTO_KEYS`], [`utc_text`],
//! [`parse_utc`]), so Recently Updated survives a rebuild.
//!
//! M13 adds a photo's `faces` (a list of `{"rect": [x0, y0, x1, y1], "person": …}`, the person a
//! person id, `"ignored"`, or no key for an unnamed face) and `geo` (`[lat, lon]`), and the
//! top-level `people` (person id → `{"name", "nickname"?}`, the people present in the folder)
//! ([`M13_PHOTO_KEYS`], [`M13_TOP_KEYS`]). Coordinates are kept to six decimals ([`FaceRect`],
//! [`Geo`]). A face that cannot be used (a rectangle outside the photo, inverted or not finite, a
//! person that is not a person id) and the faces past [`MAX_FACES`] are kept verbatim and left
//! out ([`PhotoFaces::bad`], [`PhotoFaces::over`]); so are the keys of a face Dalica does not know.
//! A `geo` that is not a place on the globe is a value of the wrong type; `"geo": null` says the
//! user removed the photo's location ([`GeoTag::Removed`], M13.9), so the file's own GPS no longer
//! counts.
//!
//! Reading never fails: each key is [`Field::Absent`], a value of the right type, or
//! [`Field::WrongType`] holding what is there (kept, reported by [`Sidecar::problems`]). A date
//! that does not parse counts as a value of the wrong type. Writing sets one key or clears it:
//! clearing removes the key, then the photo's entry or the `"folder"` object when nothing else is
//! left in it, and never creates anything. A write is refused ([`WrongType`]) when the object it
//! goes into is not an object; the value there is kept.

use std::collections::BTreeMap;

use dalica_core::{AlbumId, Date, DateTime, PersonId, PosKey, Timestamp};
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
/// The photo keys M9 adds: the photo's album memberships with their position keys.
pub const M9_PHOTO_KEYS: [&str; 1] = [ALBUMS];
/// The `"folder"` keys M9 adds: the folder's photo sort and whether it is reversed.
pub const M9_FOLDER_KEYS: [&str; 2] = [SORT, REVERSE];
/// The top-level keys M9 adds: the names of the albums with members in the folder.
pub const M9_TOP_KEYS: [&str; 1] = [ALBUMS];
/// A photo's memberships (`photos.<name>.albums`), and the folder's album names (top level).
pub const ALBUMS: &str = "albums";
/// The photo keys M10 adds: when Dalica last changed what is said about the photo.
pub const M10_PHOTO_KEYS: [&str; 1] = [CHANGED];
/// A photo's last-changed time (`photos.<name>.changed`), a UTC time with milliseconds.
pub const CHANGED: &str = "changed";
/// The photo keys M13 adds: the photo's faces and its geotag.
pub const M13_PHOTO_KEYS: [&str; 2] = [FACES, GEO];
/// The top-level keys M13 adds: the people present in the folder.
pub const M13_TOP_KEYS: [&str; 1] = [PEOPLE];
/// A photo's faces (`photos.<name>.faces`), a list of `{"rect", "person"?}`.
pub const FACES: &str = "faces";
/// A photo's geotag (`photos.<name>.geo`), `[lat, lon]` in degrees, or `null`: removed by the user.
pub const GEO: &str = "geo";
/// The folder's people (top level), person id → `{"name", "nickname"?}`.
pub const PEOPLE: &str = "people";
/// The person of an ignored face.
pub const IGNORED: &str = "ignored";
/// The most faces a photo has: later usable entries are kept in the file and left out.
pub const MAX_FACES: usize = 256;
/// A folder's photo sort, in `"folder"` (`manual | name | date | size | modified`).
pub const SORT: &str = "sort";
/// Whether a folder's photo sort is reversed, in `"folder"`.
pub const REVERSE: &str = "reverse";

/// What a photo's `albums` object says: the usable memberships, and the ids of the entries that
/// are kept but not used (an id that is not an album id, or a value that is not a position key).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhotoAlbums {
    pub members: BTreeMap<AlbumId, PosKey>,
    /// The keys of the entries left out, in file order.
    pub bad: Vec<String>,
}

/// What the top-level `albums` object says: each usable album's name as written, and the ids of
/// the entries kept but not used (an id that is not an album id, or no non-empty string `name`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumNames {
    pub names: BTreeMap<AlbumId, String>,
    /// The keys of the entries left out, in file order.
    pub bad: Vec<String>,
}

/// What a photo's `savedAs` says: the JPEG a Save wrote beside it (a file name in the same folder,
/// as written: whether it may reach a file is the reader's to check, [`crate::plain_name`]) and
/// the recipe it was rendered with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedAs {
    pub name: String,
    pub recipe: Vec<String>,
}

/// `v` kept to six decimals (`data-model.md` §3), `-0` as `0`: the nearest double to a decimal of
/// at most six places, so it prints as that decimal and reads back exactly.
fn six_decimals(v: f64) -> f64 {
    (v * 1e6).round() / 1e6 + 0.0
}

/// A face's rectangle, normalised in the stored frame (`data-model.md` §3.3): `[x0, y0, x1, y1]`,
/// finite, inside `0..=1`, corners in order (`x0 ≤ x1`, `y0 ≤ y1`), six decimals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceRect([f64; 4]);

impl FaceRect {
    /// The rectangle with its corners kept to six decimals, or `None` when it is not finite, not
    /// inside the photo or inverted.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Option<FaceRect> {
        let r = [x0, y0, x1, y1].map(six_decimals);
        let inside = r.iter().all(|v| (0.0..=1.0).contains(v));
        (inside && r[0] <= r[2] && r[1] <= r[3]).then_some(FaceRect(r))
    }

    /// `[x0, y0, x1, y1]`.
    pub fn corners(&self) -> [f64; 4] {
        self.0
    }

    fn to_json(self) -> Value {
        Value::Array(self.0.iter().map(|v| Value::from(*v)).collect())
    }

    fn from_json(v: &Value) -> Option<FaceRect> {
        match v.as_array()?.as_slice() {
            [x0, y0, x1, y1] => FaceRect::new(x0.as_f64()?, y0.as_f64()?, x1.as_f64()?, y1.as_f64()?),
            _ => None,
        }
    }
}

/// Whose face it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FacePerson {
    /// Not named yet (no `person` key): shown under Unnamed.
    Unnamed,
    /// `"person": "ignored"`: never shown, never found again.
    Ignored,
    /// `"person": "<person id>"`.
    Named(PersonId),
}

/// One face of a photo.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub rect: FaceRect,
    pub person: FacePerson,
}

impl Face {
    /// A usable face entry: an object with a usable `rect` and, if present, a `person` that is a
    /// person id or `"ignored"`.
    fn from_json(v: &Value) -> Option<Face> {
        let o = v.as_object()?;
        let rect = FaceRect::from_json(o.get("rect")?)?;
        let person = match o.get("person") {
            None => FacePerson::Unnamed,
            Some(Value::String(s)) if s == IGNORED => FacePerson::Ignored,
            Some(Value::String(s)) => FacePerson::Named(PersonId::parse(s).ok()?),
            Some(_) => return None,
        };
        Some(Face { rect, person })
    }

    /// Writes this face into `object` (an entry already there, whose other keys are kept).
    fn write_into(&self, mut object: Map<String, Value>) -> Value {
        object.insert("rect".into(), self.rect.to_json());
        match &self.person {
            FacePerson::Unnamed => object.remove("person"),
            FacePerson::Ignored => object.insert("person".into(), Value::String(IGNORED.into())),
            FacePerson::Named(id) => object.insert("person".into(), Value::String(id.as_str().into())),
        };
        Value::Object(object)
    }
}

/// What a photo's `faces` list says: the usable faces in file order (at most [`MAX_FACES`]), the
/// positions of the entries kept but not used, and how many usable ones were past the cap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoFaces {
    pub faces: Vec<Face>,
    /// The positions (in the list) of the entries that are not usable faces.
    pub bad: Vec<usize>,
    /// Usable faces after the first [`MAX_FACES`]: kept in the file, left out.
    pub over: usize,
}

/// A geotag: latitude in `-90..=90` and longitude in `-180..=180` degrees, finite, six decimals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    lat: f64,
    lon: f64,
}

impl Geo {
    /// The place kept to six decimals, or `None` when it is not finite or not on the globe.
    pub fn new(lat: f64, lon: f64) -> Option<Geo> {
        let (lat, lon) = (six_decimals(lat), six_decimals(lon));
        ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some(Geo { lat, lon })
    }

    pub fn lat(&self) -> f64 {
        self.lat
    }

    pub fn lon(&self) -> f64 {
        self.lon
    }

    fn from_json(v: &Value) -> Option<Geo> {
        match v.as_array()?.as_slice() {
            [lat, lon] => Geo::new(lat.as_f64()?, lon.as_f64()?),
            _ => None,
        }
    }
}

/// What a photo's `geo` says: a place, or that the user removed the location (`"geo": null`), so
/// the photo is not geotagged whatever its file says. No key says nothing (the file's GPS counts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeoTag {
    At(Geo),
    Removed,
}

impl GeoTag {
    fn from_json(v: &Value) -> Option<GeoTag> {
        match v {
            Value::Null => Some(GeoTag::Removed),
            v => Geo::from_json(v).map(GeoTag::At),
        }
    }

    fn to_json(self) -> Value {
        match self {
            GeoTag::At(g) => Value::Array(vec![Value::from(g.lat), Value::from(g.lon)]),
            GeoTag::Removed => Value::Null,
        }
    }
}

/// A person as the folder's `people` names them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonName {
    pub name: String,
    /// `None` when absent or empty.
    pub nickname: Option<String>,
}

/// What the top-level `people` object says: each usable person, and the ids of the entries kept
/// but not used (an id that is not a person id, no non-empty string `name`, or a `nickname` that
/// is not a string).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPeople {
    pub people: BTreeMap<PersonId, PersonName>,
    /// The keys of the entries left out, in file order.
    pub bad: Vec<String>,
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
    /// When Dalica last changed what is said about the photo (M10).
    pub changed: Field<Timestamp>,
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
    /// The last-changed time (M10); a time outside the years 1 to 9999 is written as no key.
    Changed(Option<Timestamp>),
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
            PhotoValue::Changed(t) => (CHANGED, t.and_then(utc_text).map(Value::String)),
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

/// `2026-10-15T09:41:07.250Z` for `t`: the UTC date and time with milliseconds; `None` outside the
/// years 1 to 9999.
pub fn utc_text(t: Timestamp) -> Option<String> {
    let ms = t.as_millis().rem_euclid(1000);
    DateTime::from_unix_secs(t.as_secs()).map(|d| format!("{d}.{ms:03}Z"))
}

/// The time a `changed` value says: a date and time as [`DateTime`] reads it, then optionally `.`
/// and exactly three digits of milliseconds, then `Z` (UTC). `None` for anything else.
pub fn parse_utc(s: &str) -> Option<Timestamp> {
    let s = s.strip_suffix('Z')?;
    let (main, ms) = match s.split_once('.') {
        Some((main, frac)) if frac.len() == 3 && frac.bytes().all(|b| b.is_ascii_digit()) => (main, frac.parse::<i64>().ok()?),
        Some(_) => return None,
        None => (s, 0),
    };
    let d: DateTime = main.parse().ok()?;
    unix_secs(d).checked_mul(1000)?.checked_add(ms).map(Timestamp::from_millis)
}

/// Seconds since 1970-01-01T00:00:00 of `d`, read as UTC (the inverse of
/// [`DateTime::from_unix_secs`]; years 1 to 9999 keep every step small).
fn unix_secs(d: DateTime) -> i64 {
    let date = d.date();
    let (month, day) = (i64::from(date.month()), i64::from(date.day()));
    let year = i64::from(date.year()) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second())
}

fn read_utc(v: Option<&Value>) -> Field<Timestamp> {
    match v {
        None => Field::Absent,
        Some(Value::String(s)) => match parse_utc(s) {
            Some(t) => Field::Value(t),
            None => Field::WrongType(Value::String(s.clone())),
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
        ALBUMS => v.is_object(),
        CHANGED => v.as_str().and_then(parse_utc).is_some(),
        FACES => v.is_array(),
        GEO => GeoTag::from_json(v).is_some(),
        _ => true,
    };
    (!ok).then_some(match key {
        "star" | "hidden" => "true or false",
        "caption" => "a string",
        "date" => "a date and time (2026-06-04T19:42:10)",
        SAVED_AS => "an object with a file name (\"name\") and a list of strings (\"recipe\")",
        ALBUMS => "an object from album id to position key",
        CHANGED => "a UTC time (2026-10-15T09:41:07.250Z)",
        FACES => "a list of faces ({\"rect\": [x0, y0, x1, y1], \"person\": …})",
        GEO => "a place ([latitude, longitude] in degrees) or null",
        _ => "a list of strings",
    })
}

fn folder_problem(key: &str, v: &Value) -> Option<&'static str> {
    let ok = match key {
        "hidden" | REVERSE => v.is_boolean(),
        "description" | "place" | PROJECT | SORT => v.is_string(),
        "date" => v.as_str().is_some_and(|s| s.parse::<Date>().is_ok()),
        _ => true,
    };
    (!ok).then_some(match key {
        "hidden" | REVERSE => "true or false",
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
            changed: read_utc(get(CHANGED)),
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
                for key in FOLDER_KEYS.iter().chain(&M8_FOLDER_KEYS).chain(&M9_FOLDER_KEYS) {
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
                            for key in PHOTO_KEYS.iter().chain(&M8_PHOTO_KEYS).chain(&M9_PHOTO_KEYS).chain(&M10_PHOTO_KEYS).chain(&M13_PHOTO_KEYS) {
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
        match self.root.get(ALBUMS) {
            None => {}
            Some(Value::Object(_)) => {}
            Some(_) => out.push(Problem { pointer: format!("/{ALBUMS}"), expected: "an object from album id to {\"name\": …}" }),
        }
        match self.root.get(PEOPLE) {
            None | Some(Value::Object(_)) => {}
            Some(_) => out.push(Problem { pointer: format!("/{PEOPLE}"), expected: "an object from person id to {\"name\": …}" }),
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
        self.set_folder_key(PROJECT, project.and_then(text))
    }

    /// Sets or clears one key of the `"folder"` object (`None` removes it, then the object when
    /// it is left empty). Refused if `"folder"` is not an object.
    fn set_folder_key(&mut self, key: &str, value: Option<Value>) -> Result<(), WrongType> {
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
                folder.insert(key.into(), v);
                self.root.insert(FOLDER.into(), Value::Object(folder));
                Ok(())
            }
        }
    }

    /// Photo `name`'s `albums`: absent, its memberships (with the entries left out), or a value
    /// that is not an object (kept, not used).
    pub fn photo_albums(&self, name: &str) -> Field<PhotoAlbums> {
        match self.photo_entry(name).and_then(|e| e.get(ALBUMS)) {
            None => Field::Absent,
            Some(Value::Object(m)) => {
                let mut out = PhotoAlbums::default();
                for (id, v) in m {
                    match membership(id, v) {
                        Some((album, key)) => {
                            out.members.insert(album, key);
                        }
                        None => out.bad.push(id.clone()),
                    }
                }
                Field::Value(out)
            }
            Some(other) => Field::WrongType(other.clone()),
        }
    }

    /// Makes photo `name`'s usable memberships of the albums `managed` answers for exactly
    /// `members` (each of which must be managed): usable entries of managed albums not in it go,
    /// its entries are set (over an unusable entry of the same id too), and the other entries
    /// (usable ones of albums not managed, and those left out by reading) are kept verbatim. The
    /// key goes when nothing is left in it, and the photo's entry when nothing is left in that;
    /// clearing never creates anything. A value of the wrong type is written over only by
    /// memberships. Refused if `photos` or the entry is not an object.
    pub fn set_photo_albums(&mut self, name: &str, members: &BTreeMap<AlbumId, PosKey>, managed: impl Fn(&AlbumId) -> bool) -> Result<(), WrongType> {
        let entry_pointer = || format!("/photos/{}", escape(name));
        if members.is_empty() && !self.root.get("photos").and_then(Value::as_object).is_some_and(|p| p.contains_key(name)) {
            // Nothing to clear (or a `photos` of the wrong type, which clearing never touches).
            return match self.root.get("photos") {
                None | Some(Value::Object(_)) => Ok(()),
                Some(_) => Err(WrongType { pointer: "/photos".into() }),
            };
        }
        let photos = self.root.entry("photos").or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(photos) = photos else { return Err(WrongType { pointer: "/photos".into() }) };
        let entry = photos.entry(name).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(entry) = entry else { return Err(WrongType { pointer: entry_pointer() }) };
        let had = entry.contains_key(ALBUMS);
        let mut object = match entry.remove(ALBUMS) {
            Some(Value::Object(m)) => m,
            Some(other) if members.is_empty() => {
                entry.insert(ALBUMS.into(), other);
                return Ok(());
            }
            _ => Map::new(),
        };
        object.retain(|id, v| match membership(id, v) {
            Some((album, _)) => !managed(&album) && !members.contains_key(&album),
            None => !AlbumId::parse(id).is_ok_and(|a| members.contains_key(&a)),
        });
        for (album, key) in members {
            object.insert(album.as_str().into(), Value::String(key.as_str().into()));
        }
        if !object.is_empty() {
            entry.insert(ALBUMS.into(), Value::Object(object));
        }
        if had && entry.is_empty() {
            photos.remove(name);
        }
        Ok(())
    }

    /// The folder's album names (the top-level `albums`): absent, the usable names as written
    /// (with the entries left out), or a value that is not an object (kept, not used).
    pub fn album_names(&self) -> Field<AlbumNames> {
        match self.root.get(ALBUMS) {
            None => Field::Absent,
            Some(Value::Object(m)) => {
                let mut out = AlbumNames::default();
                for (id, v) in m {
                    match album_name(id, v) {
                        Some((album, name)) => {
                            out.names.insert(album, name);
                        }
                        None => out.bad.push(id.clone()),
                    }
                }
                Field::Value(out)
            }
            Some(other) => Field::WrongType(other.clone()),
        }
    }

    /// Makes the folder's usable album names exactly `names`: usable entries not in it go, each
    /// of its albums gets its `name` set in the object already there (other keys of that object
    /// kept) or a new one, and the entries left out by reading are kept verbatim (one with the id
    /// of an album in `names` is replaced). The key goes when nothing is left in it. A value of
    /// the wrong type is written over only by names.
    pub fn set_album_names(&mut self, names: &BTreeMap<AlbumId, String>) {
        let mut object = match self.root.remove(ALBUMS) {
            Some(Value::Object(m)) => m,
            Some(other) if names.is_empty() => {
                self.root.insert(ALBUMS.into(), other);
                return;
            }
            _ => Map::new(),
        };
        object.retain(|id, v| match AlbumId::parse(id) {
            Ok(a) if names.contains_key(&a) => v.is_object(),
            Ok(_) => album_name(id, v).is_none(),
            Err(_) => true,
        });
        for (album, name) in names {
            let slot = object.entry(album.as_str()).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(o) = slot {
                o.insert("name".into(), Value::String(name.clone()));
            }
        }
        if !object.is_empty() {
            self.root.insert(ALBUMS.into(), Value::Object(object));
        }
    }

    /// The folder's `sort`, as written.
    pub fn folder_sort(&self) -> Field<String> {
        read_text(self.folder_object().and_then(|f| f.get(SORT)))
    }

    /// Sets or clears the folder's `sort` (`None` or `""` removes it).
    pub fn set_folder_sort(&mut self, sort: Option<&str>) -> Result<(), WrongType> {
        self.set_folder_key(SORT, sort.and_then(text))
    }

    /// The folder's `reverse` flag.
    pub fn folder_reverse(&self) -> Field<bool> {
        read_bool(self.folder_object().and_then(|f| f.get(REVERSE)))
    }

    /// Sets or clears the folder's `reverse` flag (`false` removes it).
    pub fn set_folder_reverse(&mut self, on: bool) -> Result<(), WrongType> {
        self.set_folder_key(REVERSE, flag(on))
    }

    /// Photo `name`'s `faces`: absent, its faces (with the entries left out), or a value that is
    /// not a list (kept, not used).
    pub fn photo_faces(&self, name: &str) -> Field<PhotoFaces> {
        match self.photo_entry(name).and_then(|e| e.get(FACES)) {
            None => Field::Absent,
            Some(Value::Array(list)) => {
                let mut out = PhotoFaces::default();
                for (i, v) in list.iter().enumerate() {
                    match Face::from_json(v) {
                        Some(_) if out.faces.len() >= MAX_FACES => out.over += 1,
                        Some(face) => out.faces.push(face),
                        None => out.bad.push(i),
                    }
                }
                Field::Value(out)
            }
            Some(other) => Field::WrongType(other.clone()),
        }
    }

    /// Makes photo `name`'s usable faces exactly `faces`, in that order. Each face is written into
    /// the entry it came from, so the keys of a face Dalica does not know stay with it: an entry
    /// with the same rectangle first, then (a face moved, or every face mapped by a Save) the
    /// entries left in their order. The entries left out by reading (not usable, or past
    /// [`MAX_FACES`]) are kept verbatim after them. The key goes when nothing is left in it, and the
    /// photo's entry when nothing is left in that; clearing never creates anything. A value of the
    /// wrong type is written over only by faces. Refused if `photos` or the entry is not an object.
    pub fn set_photo_faces(&mut self, name: &str, faces: &[Face]) -> Result<(), WrongType> {
        let Some((entry, had)) = self.entry_for(name, faces.is_empty())? else { return Ok(()) };
        let old = match entry.remove(FACES) {
            Some(Value::Array(list)) => list,
            Some(other) if faces.is_empty() => {
                entry.insert(FACES.into(), other);
                return Ok(());
            }
            _ => Vec::new(),
        };
        // The usable entries Dalica read (with their rectangles) and the ones it keeps as they are.
        let mut usable: Vec<Option<(FaceRect, Map<String, Value>)>> = Vec::new();
        let mut kept = Vec::new();
        for v in old {
            match (Face::from_json(&v), v) {
                (Some(face), Value::Object(o)) if usable.len() < MAX_FACES => usable.push(Some((face.rect, o))),
                (_, v) => kept.push(v),
            }
        }
        let mut objects: Vec<Option<Map<String, Value>>> = faces
            .iter()
            .map(|f| usable.iter_mut().find(|u| u.as_ref().is_some_and(|(r, _)| *r == f.rect)).and_then(Option::take).map(|(_, o)| o))
            .collect();
        let mut left = usable.into_iter().flatten();
        for slot in objects.iter_mut().filter(|o| o.is_none()) {
            *slot = left.next().map(|(_, o)| o);
        }
        let mut list: Vec<Value> = faces.iter().zip(objects).map(|(f, o)| f.write_into(o.unwrap_or_default())).collect();
        list.extend(kept);
        if !list.is_empty() {
            entry.insert(FACES.into(), Value::Array(list));
        }
        if had && entry.is_empty() {
            self.remove_photo_entry(name);
        }
        Ok(())
    }

    /// Photo `name`'s `geo`: absent, its place or `null` (the location removed), or a value that is
    /// neither (kept, not used).
    pub fn geo(&self, name: &str) -> Field<GeoTag> {
        match self.photo_entry(name).and_then(|e| e.get(GEO)) {
            None => Field::Absent,
            Some(v) => GeoTag::from_json(v).map_or_else(|| Field::WrongType(v.clone()), Field::Value),
        }
    }

    /// Sets or clears photo `name`'s `geo` (`[lat, lon]` with six decimals, or `null` for a location
    /// the user removed), the way [`Sidecar::set_photo`] sets a key: setting writes over whatever is
    /// there, clearing removes the key and the entry left empty. Refused if `photos` or the entry is
    /// not an object.
    pub fn set_geo(&mut self, name: &str, geo: Option<GeoTag>) -> Result<(), WrongType> {
        let Some((entry, had)) = self.entry_for(name, geo.is_none())? else { return Ok(()) };
        match geo {
            Some(g) => {
                entry.insert(GEO.into(), g.to_json());
            }
            None => {
                entry.remove(GEO);
            }
        }
        if had && entry.is_empty() {
            self.remove_photo_entry(name);
        }
        Ok(())
    }

    /// The folder's people (the top-level `people`): absent, the usable ones (with the entries
    /// left out), or a value that is not an object (kept, not used).
    pub fn people(&self) -> Field<FolderPeople> {
        match self.root.get(PEOPLE) {
            None => Field::Absent,
            Some(Value::Object(m)) => {
                let mut out = FolderPeople::default();
                for (id, v) in m {
                    match person_name(id, v) {
                        Some((person, name)) => {
                            out.people.insert(person, name);
                        }
                        None => out.bad.push(id.clone()),
                    }
                }
                Field::Value(out)
            }
            Some(other) => Field::WrongType(other.clone()),
        }
    }

    /// Makes the folder's usable people exactly `people`, as [`Sidecar::set_album_names`] does for
    /// albums: usable entries not in it go, each person gets `name` (and `nickname`, or none) set
    /// in the object already there (other keys kept) or a new one, and the entries left out by
    /// reading are kept verbatim (one with the id of a person in `people` is replaced). The key
    /// goes when nothing is left in it. A value of the wrong type is written over only by people.
    pub fn set_people(&mut self, people: &BTreeMap<PersonId, PersonName>) {
        let mut object = match self.root.remove(PEOPLE) {
            Some(Value::Object(m)) => m,
            Some(other) if people.is_empty() => {
                self.root.insert(PEOPLE.into(), other);
                return;
            }
            _ => Map::new(),
        };
        object.retain(|id, v| match PersonId::parse(id) {
            Ok(p) if people.contains_key(&p) => v.is_object(),
            Ok(_) => person_name(id, v).is_none(),
            Err(_) => true,
        });
        for (person, name) in people {
            let slot = object.entry(person.as_str()).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(o) = slot {
                o.insert("name".into(), Value::String(name.name.clone()));
                put(o, "nickname", name.nickname.as_deref().and_then(text));
            }
        }
        if !object.is_empty() {
            self.root.insert(PEOPLE.into(), Value::Object(object));
        }
    }

    /// Photo `name`'s entry to write into, and whether it was there before: `None` when there is
    /// nothing to clear (`clearing`, no entry). Refused if `photos` or the entry is not an object.
    fn entry_for(&mut self, name: &str, clearing: bool) -> Result<Option<EntryToWrite<'_>>, WrongType> {
        if clearing && !self.root.contains_key("photos") {
            return Ok(None);
        }
        let photos = self.root.entry("photos").or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(photos) = photos else { return Err(WrongType { pointer: "/photos".into() }) };
        let had = photos.contains_key(name);
        if !had && clearing {
            return Ok(None);
        }
        match photos.entry(name).or_insert_with(|| Value::Object(Map::new())) {
            Value::Object(entry) => Ok(Some((entry, had))),
            _ => Err(WrongType { pointer: format!("/photos/{}", escape(name)) }),
        }
    }

    /// Removes photo `name`'s entry (left empty by a write).
    fn remove_photo_entry(&mut self, name: &str) {
        if let Some(Value::Object(photos)) = self.root.get_mut("photos") {
            photos.remove(name);
        }
    }
}

/// A photo's entry to write into, and whether it was there before the write.
type EntryToWrite<'a> = (&'a mut Map<String, Value>, bool);

/// A usable person entry: a person id with an object holding a non-empty string `name` and, if
/// present, a string `nickname` (empty reads as none).
fn person_name(id: &str, v: &Value) -> Option<(PersonId, PersonName)> {
    let o = v.as_object()?;
    let name = o.get("name")?.as_str().filter(|n| !n.is_empty())?;
    let nickname = match o.get("nickname") {
        None => None,
        Some(Value::String(n)) => Some(n.clone()).filter(|n| !n.is_empty()),
        Some(_) => return None,
    };
    Some((PersonId::parse(id).ok()?, PersonName { name: name.to_string(), nickname }))
}

/// A usable membership: an album id with a position key.
fn membership(id: &str, v: &Value) -> Option<(AlbumId, PosKey)> {
    Some((AlbumId::parse(id).ok()?, PosKey::parse(v.as_str()?).ok()?))
}

/// A usable album name entry: an album id with an object holding a non-empty string `name`.
fn album_name(id: &str, v: &Value) -> Option<(AlbumId, String)> {
    let name = v.as_object()?.get("name")?.as_str().filter(|n| !n.is_empty())?;
    Some((AlbumId::parse(id).ok()?, name.to_string()))
}
