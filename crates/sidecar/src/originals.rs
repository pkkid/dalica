//! The originals record `.dalica-originals/.dalica.json` (`data-model.md` §6): what each Save in a
//! folder kept. One entry per original under `"originals"`, keyed by the photo's file name:
//!
//! ```json
//! {
//!   "dalica": { "version": 1 },
//!   "originals": {
//!     "IMG_0001.jpg": {
//!       "digest": "md5:9e107d9d372bb6826bd81d3542a419d6",
//!       "height": 3000,
//!       "mtime": 1780000000000,
//!       "recipe": ["rotate(90)", "autofix()"],
//!       "size": 2483112,
//!       "state": "done",
//!       "time": 1791000000000,
//!       "width": 4000
//!     }
//!   }
//! }
//! ```
//!
//! `size` (bytes), `mtime` (milliseconds since the Unix epoch) and `state` (`"pending"` or
//! `"done"`) are required; `recipe` (absent: empty), `width` and `height` (absent: 0, unknown),
//! `digest` (absent: empty) and `time` (when the Save was made, milliseconds since the epoch;
//! absent: unknown) are optional. An entry with a required key missing or any known key of the
//! wrong type is [`Field::WrongType`]: kept as it is, reported by [`Originals::problems`], never
//! used. Keys Dalica does not know, in the document or in an entry, are kept verbatim.
//!
//! A name is used to reach a file only when it is one plain path component ([`crate::plain_name`],
//! `security.md` §5.4): [`Originals::entries`] leaves the others out (they stay in the file and
//! are reported), and [`Originals::set`] refuses to write one.
//!
//! Parsing and printing are those of the folder sidecar: a BOM and CRLF accepted, the limits of
//! `security.md` §4.2 (the photo count applies to `"originals"`), keys sorted by their bytes at
//! every level.

use serde_json::{Map, Value};

use crate::{Field, Limits, ParseError, Problem, WrongType, escape, plain_name};

/// The originals folder inside each photo folder.
pub const DIR_NAME: &str = ".dalica-originals";
/// The record's file name inside [`DIR_NAME`].
pub const FILE_NAME: &str = ".dalica.json";
/// The top-level object keyed by file names.
pub const KEY: &str = "originals";
/// The keys of an entry that Dalica reads, in the order of `data-model.md` §6.
pub const ENTRY_KEYS: [&str; 8] = ["recipe", "size", "mtime", "width", "height", "digest", "state", "time"];

/// Where a Save stands: written `pending` before anything moves, marked `done` last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum State {
    #[default]
    Pending,
    Done,
}

impl State {
    /// The spelling in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Done => "done",
        }
    }

    fn parse(s: &str) -> Option<State> {
        match s {
            "pending" => Some(State::Pending),
            "done" => Some(State::Done),
            _ => None,
        }
    }
}

/// One kept original, as the record says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// The whole recipe the saved file holds (operation strings, opaque here).
    pub recipe: Vec<String>,
    /// The kept original's size in bytes.
    pub size: u64,
    /// The kept original's modification time, in milliseconds since the Unix epoch.
    pub mtime: i64,
    /// The kept original's size in pixels (as displayed); 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// The kept original's digest as text (`<algorithm>:<lowercase hex>`; empty when unknown).
    pub digest: String,
    pub state: State,
    /// When the Save was made, in milliseconds since the Unix epoch.
    pub time: Option<i64>,
}

/// Why [`Originals::set`] or [`Originals::remove`] wrote nothing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// The place to write holds a value of another type; it is kept.
    #[error(transparent)]
    WrongType(#[from] WrongType),
    /// The name is not one plain path component; nothing is written under it.
    #[error("{0:?} is not a plain file name; nothing is written under it")]
    Name(String),
}

/// A folder's originals record: the whole JSON document, with typed access to its entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Originals {
    root: Map<String, Value>,
}

impl Default for Originals {
    fn default() -> Originals {
        Originals::new()
    }
}

impl Originals {
    /// An empty record: `{"dalica": {"version": 1}}`.
    pub fn new() -> Originals {
        Originals { root: crate::new_root() }
    }

    /// Parses a record. A file that is not JSON, not an object or over the limits is a
    /// [`ParseError`]: the caller sets it aside as `.dalica.json.corrupt-<time>`.
    pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Originals, ParseError> {
        crate::parse_root(bytes, limits, KEY).map(|root| Originals { root })
    }

    /// The document as it is written to disk.
    pub fn print(&self) -> Vec<u8> {
        crate::print::document(&self.root)
    }

    /// `dalica.version`, if it is a non-negative integer.
    pub fn version(&self) -> Option<u64> {
        self.root.get("dalica")?.get("version")?.as_u64()
    }

    /// Every name under `"originals"`, plain or not, whatever its value.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.root.get(KEY).and_then(Value::as_object).into_iter().flat_map(|m| m.keys().map(String::as_str))
    }

    /// What the record says about `name` (any name: this reaches no file).
    pub fn entry(&self, name: &str) -> Field<Entry> {
        match self.root.get(KEY).and_then(Value::as_object).and_then(|m| m.get(name)) {
            None => Field::Absent,
            Some(v) => match read_entry(v) {
                Some(e) => Field::Value(e),
                None => Field::WrongType(v.clone()),
            },
        }
    }

    /// The entries Dalica may use: a plain name and a readable entry, in name order. The others
    /// stay in the file ([`Originals::problems`] names them).
    pub fn entries(&self) -> Vec<(String, Entry)> {
        let Some(m) = self.root.get(KEY).and_then(Value::as_object) else { return Vec::new() };
        let mut out: Vec<(String, Entry)> = m.iter().filter(|(k, _)| plain_name(k)).filter_map(|(k, v)| Some((k.clone(), read_entry(v)?))).collect();
        out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        out
    }

    /// Whether the record holds no entry at all (usable or not).
    pub fn is_empty(&self) -> bool {
        self.names().next().is_none()
    }

    /// Writes `entry` for `name`: its keys go into the object already there (keys Dalica does not
    /// know are kept; `time: None` removes `time`) or a new one. Refused for a name that is not
    /// plain, and when `"originals"` or the entry there is not an object.
    pub fn set(&mut self, name: &str, entry: &Entry) -> Result<(), Refused> {
        if !plain_name(name) {
            return Err(Refused::Name(name.to_string()));
        }
        let all = self.root.entry(KEY).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(all) = all else { return Err(WrongType { pointer: format!("/{KEY}") }.into()) };
        let object = all.entry(name).or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(object) = object else { return Err(WrongType { pointer: format!("/{KEY}/{}", escape(name)) }.into()) };
        object.insert("recipe".into(), Value::Array(entry.recipe.iter().cloned().map(Value::String).collect()));
        object.insert("size".into(), Value::from(entry.size));
        object.insert("mtime".into(), Value::from(entry.mtime));
        object.insert("width".into(), Value::from(entry.width));
        object.insert("height".into(), Value::from(entry.height));
        object.insert("digest".into(), Value::String(entry.digest.clone()));
        object.insert("state".into(), Value::String(entry.state.as_str().into()));
        match entry.time {
            Some(t) => object.insert("time".into(), Value::from(t)),
            None => object.remove("time"),
        };
        Ok(())
    }

    /// Removes `name`'s entry, whatever it holds; returns whether there was one. The
    /// `"originals"` object goes when it is left empty. Refused for a name that is not plain and
    /// when `"originals"` is not an object.
    pub fn remove(&mut self, name: &str) -> Result<bool, Refused> {
        if !plain_name(name) {
            return Err(Refused::Name(name.to_string()));
        }
        let all = match self.root.get_mut(KEY) {
            None => return Ok(false),
            Some(Value::Object(all)) => all,
            Some(_) => return Err(WrongType { pointer: format!("/{KEY}") }.into()),
        };
        let removed = all.remove(name).is_some();
        if removed && all.is_empty() {
            self.root.remove(KEY);
        }
        Ok(removed)
    }

    /// Everything Dalica cannot use, as JSON pointers: values of the wrong type at known places,
    /// entries with a required key missing, and names that are not one plain path component.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        crate::dalica_problems(&self.root, &mut out);
        match self.root.get(KEY) {
            None => {}
            Some(Value::Object(all)) => {
                for (name, v) in all {
                    let at = format!("/{KEY}/{}", escape(name));
                    if !plain_name(name) {
                        out.push(Problem { pointer: at.clone(), expected: "a plain file name" });
                    }
                    let Some(e) = v.as_object() else {
                        out.push(Problem { pointer: at, expected: "an object" });
                        continue;
                    };
                    for key in ENTRY_KEYS {
                        match e.get(key) {
                            None if REQUIRED.contains(&key) => out.push(Problem { pointer: format!("{at}/{key}"), expected: expected(key) }),
                            Some(v) if !key_ok(key, v) => out.push(Problem { pointer: format!("{at}/{key}"), expected: expected(key) }),
                            _ => {}
                        }
                    }
                }
            }
            Some(_) => out.push(Problem { pointer: format!("/{KEY}"), expected: "an object" }),
        }
        out
    }
}

/// The keys an entry must have.
const REQUIRED: [&str; 3] = ["size", "mtime", "state"];

fn key_ok(key: &str, v: &Value) -> bool {
    match key {
        "recipe" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        "size" => v.as_u64().is_some(),
        "mtime" | "time" => v.as_i64().is_some(),
        "width" | "height" => v.as_u64().is_some_and(|n| u32::try_from(n).is_ok()),
        "digest" => v.is_string(),
        "state" => v.as_str().and_then(State::parse).is_some(),
        _ => true,
    }
}

fn expected(key: &str) -> &'static str {
    match key {
        "recipe" => "a list of strings",
        "size" => "a byte count",
        "mtime" | "time" => "milliseconds since 1970 (an integer)",
        "width" | "height" => "a pixel count",
        "digest" => "a string",
        "state" => "\"pending\" or \"done\"",
        _ => "a value",
    }
}

/// An entry, if every known key has the right type and the required ones are there.
fn read_entry(v: &Value) -> Option<Entry> {
    let e = v.as_object()?;
    if ENTRY_KEYS.iter().any(|k| e.get(*k).is_some_and(|v| !key_ok(k, v))) {
        return None;
    }
    let count = |k: &str| e.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    Some(Entry {
        recipe: e
            .get("recipe")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        size: e.get("size")?.as_u64()?,
        mtime: e.get("mtime")?.as_i64()?,
        width: count("width"),
        height: count("height"),
        digest: e.get("digest").and_then(Value::as_str).unwrap_or_default().to_string(),
        state: State::parse(e.get("state")?.as_str()?)?,
        time: e.get("time").and_then(Value::as_i64),
    })
}
