//! `albums.json` in the library directory (`data-model.md` §4): the album definitions and a copy
//! of each album's order. One entry per album under `"albums"`, keyed by its id:
//!
//! ```json
//! {
//!   "albums": {
//!     "a91c0e": {
//!       "cover": "/home/ana/Pictures/Lisbon/IMG_0002.jpg",
//!       "date": "2026-06-04",
//!       "description": "The best of the year",
//!       "name": "Best of 2026",
//!       "order": [["/home/ana/Pictures/Lisbon/IMG_0002.jpg", "a0"], ["/home/ana/Pictures/Porto/IMG_0107.jpg", "a1"]],
//!       "place": "Portugal",
//!       "useNewest": true
//!     }
//!   },
//!   "dalica": { "version": 1 }
//! }
//! ```
//!
//! `name` (a non-empty string) is required; `description`, `place` (strings), `date`
//! (`2026-06-04`), `useNewest` (a boolean), `cover` (the cover photo's absolute path) and `order`
//! (the members in album order, each `[absolute path, position key]`) are optional, absent when
//! empty. Photos are named by path because photo ids belong to one index and a rebuild makes new
//! ones. The sidecars stay the source of the memberships: the order here is a copy.
//!
//! An entry whose id is not an album id, that is not an object or that has no usable name is kept
//! verbatim and reported, never used ([`AlbumsRead::bad`]). A known key of the wrong type reads as
//! empty and is reported; an `order` item that is not a path with a position key is left out and
//! reported. Keys Dalica does not know, in the document or in an entry, are kept verbatim.
//! Parsing and printing are those of the folder sidecar (a BOM and CRLF accepted, the limits of
//! `security.md` §4.2 with the photo count applied to `"albums"`, keys sorted at every level).

use std::collections::BTreeMap;

use dalica_core::{AlbumId, Date, PosKey};
use serde_json::{Map, Value};

use crate::{Limits, ParseError, Problem, escape};

/// The file's name in the library directory.
pub const FILE_NAME: &str = "albums.json";
/// The top-level object keyed by album ids.
pub const KEY: &str = "albums";
/// The keys of an entry that Dalica reads, in the order of `data-model.md` §4.
pub const ENTRY_KEYS: [&str; 7] = ["name", "description", "date", "useNewest", "place", "cover", "order"];

/// One album, as the file says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Album {
    /// As written (not empty); the reader cleans it.
    pub name: String,
    pub description: String,
    pub date: Option<Date>,
    pub use_newest: bool,
    pub place: String,
    /// The cover photo's absolute path, as written.
    pub cover: Option<String>,
    /// The members in album order: (absolute path as written, position key).
    pub order: Vec<(String, PosKey)>,
}

/// What [`AlbumsFile::albums`] gives: the usable albums by id, and the ids of the entries kept but
/// not used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumsRead {
    pub albums: BTreeMap<AlbumId, Album>,
    /// The keys of the entries left out, in file order.
    pub bad: Vec<String>,
}

/// The whole `albums.json` document, with typed access to its entries.
#[derive(Clone, Debug, PartialEq)]
pub struct AlbumsFile {
    root: Map<String, Value>,
}

impl Default for AlbumsFile {
    fn default() -> AlbumsFile {
        AlbumsFile::new()
    }
}

impl AlbumsFile {
    /// An empty file: `{"dalica": {"version": 1}}`.
    pub fn new() -> AlbumsFile {
        AlbumsFile { root: crate::new_root() }
    }

    /// Parses a file. Accepts a UTF-8 BOM and CRLF; a duplicate key takes the later value.
    pub fn parse(bytes: &[u8], limits: &Limits) -> Result<AlbumsFile, ParseError> {
        crate::parse_root(bytes, limits, KEY).map(|root| AlbumsFile { root })
    }

    /// The document as it is written to disk.
    pub fn print(&self) -> Vec<u8> {
        crate::print::document(&self.root)
    }

    /// `dalica.version`, if it is a non-negative integer.
    pub fn version(&self) -> Option<u64> {
        self.root.get("dalica")?.get("version")?.as_u64()
    }

    /// The usable albums and the entries left out.
    pub fn albums(&self) -> AlbumsRead {
        let mut out = AlbumsRead::default();
        let Some(Value::Object(albums)) = self.root.get(KEY) else { return out };
        for (id, v) in albums {
            match (AlbumId::parse(id), v.as_object().and_then(album)) {
                (Ok(id), Some(a)) => {
                    out.albums.insert(id, a);
                }
                _ => out.bad.push(id.clone()),
            }
        }
        out
    }

    /// Every value Dalica cannot use, where it is and what was expected.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        crate::dalica_problems(&self.root, &mut out);
        match self.root.get(KEY) {
            None => {}
            Some(Value::Object(albums)) => {
                for (id, v) in albums {
                    let at = format!("/{KEY}/{}", escape(id));
                    if AlbumId::parse(id).is_err() {
                        out.push(Problem { pointer: at, expected: "an album id (a and 4 to 16 lowercase hex digits)" });
                        continue;
                    }
                    let Some(entry) = v.as_object() else {
                        out.push(Problem { pointer: at, expected: "an object" });
                        continue;
                    };
                    entry_problems(entry, &at, &mut out);
                }
            }
            Some(_) => out.push(Problem { pointer: format!("/{KEY}"), expected: "an object from album id to album" }),
        }
        out
    }

    /// The ids of every entry, usable or not, as written.
    pub fn ids(&self) -> Vec<String> {
        self.root.get(KEY).and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }

    /// Writes album `id`: its keys are set in the object already there (keys Dalica does not know
    /// are kept; an empty value removes its key) or in a new one. An entry of the wrong type
    /// under the id is replaced, and so is an `"albums"` that is not an object.
    pub fn set(&mut self, id: &AlbumId, a: &Album) {
        if !matches!(self.root.get(KEY), Some(Value::Object(_))) {
            self.root.insert(KEY.into(), Value::Object(Map::new()));
        }
        let Some(Value::Object(albums)) = self.root.get_mut(KEY) else { return };
        let slot = albums.entry(id.as_str()).or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Value::Object(entry) = slot else { return };
        let text = |s: &str| (!s.is_empty()).then(|| Value::String(s.to_string()));
        let values = [
            ("name", text(&a.name)),
            ("description", text(&a.description)),
            ("date", a.date.map(|d| Value::String(d.to_string()))),
            ("useNewest", a.use_newest.then_some(Value::Bool(true))),
            ("place", text(&a.place)),
            ("cover", a.cover.as_deref().and_then(text)),
            (
                "order",
                (!a.order.is_empty()).then(|| {
                    Value::Array(
                        a.order.iter().map(|(p, k)| Value::Array(vec![Value::String(p.clone()), Value::String(k.as_str().into())])).collect(),
                    )
                }),
            ),
        ];
        for (key, value) in values {
            match value {
                Some(v) => {
                    entry.insert(key.into(), v);
                }
                None => {
                    entry.remove(key);
                }
            }
        }
    }

    /// Removes the entry under `id` (as written), if there is one.
    pub fn remove(&mut self, id: &str) {
        if let Some(Value::Object(albums)) = self.root.get_mut(KEY) {
            albums.remove(id);
        }
    }
}

/// The album an entry holds, if its name is usable (other keys of the wrong type read as empty).
fn album(e: &Map<String, Value>) -> Option<Album> {
    let name = e.get("name")?.as_str().filter(|n| !n.is_empty())?.to_string();
    let text = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    Some(Album {
        name,
        description: text("description"),
        date: e.get("date").and_then(Value::as_str).and_then(|s| s.parse().ok()),
        use_newest: e.get("useNewest").and_then(Value::as_bool).unwrap_or(false),
        place: text("place"),
        cover: e.get("cover").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string),
        order: e.get("order").and_then(Value::as_array).map(|items| items.iter().filter_map(order_item).collect()).unwrap_or_default(),
    })
}

/// One `order` item: `[path, position key]`, the path not empty.
fn order_item(v: &Value) -> Option<(String, PosKey)> {
    match v.as_array()?.as_slice() {
        [Value::String(path), Value::String(key)] if !path.is_empty() => Some((path.clone(), PosKey::parse(key).ok()?)),
        _ => None,
    }
}

fn entry_problems(e: &Map<String, Value>, at: &str, out: &mut Vec<Problem>) {
    let mut push = |key: &str, expected: &'static str| out.push(Problem { pointer: format!("{at}/{key}"), expected });
    match e.get("name") {
        Some(Value::String(n)) if !n.is_empty() => {}
        _ => push("name", "a name (a non-empty string)"),
    }
    for key in ["description", "place", "cover"] {
        if e.get(key).is_some_and(|v| !v.is_string()) {
            push(key, "a string");
        }
    }
    if e.get("date").is_some_and(|v| v.as_str().and_then(|s| s.parse::<Date>().ok()).is_none()) {
        push("date", "a date (2026-06-04)");
    }
    if e.get("useNewest").is_some_and(|v| !v.is_boolean()) {
        push("useNewest", "true or false");
    }
    match e.get("order") {
        None => {}
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                if order_item(item).is_none() {
                    out.push(Problem { pointer: format!("{at}/order/{i}"), expected: "a photo's path with a position key" });
                }
            }
        }
        Some(_) => out.push(Problem { pointer: format!("{at}/order"), expected: "a list of [path, position key]" }),
    }
}
