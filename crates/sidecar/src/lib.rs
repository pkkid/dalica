//! dalica-sidecar: The `.dalica.json` folder sidecar: parse and print with unknown keys preserved; the read-only legacy reader and its conversion to Dalica's model.
//!
//! Layer L1; see `docs/architecture.md` §2 and `docs/data-model.md` §3. Version 1 knows, per
//! photo, `star`, `caption`, `tags`, `hidden`, `edits`, `redo` and `date` under `photos.<name>`,
//! and the folder's `description`, `date`, `place` and `hidden` under `"folder"` ([`fields`]);
//! everything else in the file, known or not, is kept verbatim and printed back unchanged
//! (forward compatibility). A value of the wrong type is kept and reported, never
//! dropped and never fatal. A file that is not JSON, or breaks the limits of `security.md` §4.2,
//! is a [`ParseError`]: the caller sets it aside as `.dalica.json.corrupt-<time>`.
//!
//! M8 adds a photo's `savedAs` (the copy a Save wrote beside it) and the folder's `project` mark
//! ([`fields`]), and the originals record `.dalica-originals/.dalica.json` ([`originals`]), parsed
//! and printed the same way.
//!
//! Printing is deterministic: keys sorted by their bytes at every level, two-space indentation,
//! LF line endings, a final newline.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod fields;
pub mod originals;
mod print;

use serde_json::{Map, Value};

pub use fields::{FolderFields, FolderValue, PhotoFields, PhotoValue, SavedAs};
pub use originals::Originals;

/// The sidecar's file name.
pub const FILE_NAME: &str = ".dalica.json";
/// The format version this crate writes.
pub const VERSION: u64 = 1;

/// Size and shape limits for reading (`security.md` §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_string: usize,
    pub max_photos: usize,
    pub max_keys_per_entry: usize,
    pub max_list: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: 64 << 20, max_depth: 16, max_string: 1 << 20, max_photos: 1_000_000, max_keys_per_entry: 1000, max_list: 100_000 }
    }
}

/// Why a file is not a readable sidecar. The caller keeps it aside and writes nothing in that folder.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("not valid JSON at line {line}, column {column}: {message}")]
    Json { line: usize, column: usize, message: String },
    #[error("the top level is not a JSON object")]
    NotAnObject,
    #[error("over the sidecar limits: {0}")]
    Limit(String),
}

/// A value of the wrong type for a key Dalica knows: kept as it is, reported once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Where, as a JSON pointer (`/photos/IMG_0001.jpg/star`).
    pub pointer: String,
    /// What Dalica expected there.
    pub expected: &'static str,
}

/// A field Dalica reads: absent, a value of the right type, or something else (kept, not used).
#[derive(Clone, Debug, PartialEq)]
pub enum Field<T> {
    Absent,
    Value(T),
    WrongType(Value),
}

/// Refused edits: the place to write holds a value of another type, which is never overwritten
/// by an edit elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{pointer} holds a value of the wrong type; it is kept and not written over")]
pub struct WrongType {
    pub pointer: String,
}

/// One folder's sidecar: the whole JSON document, with typed access to what Dalica knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Sidecar {
    root: Map<String, Value>,
}

impl Default for Sidecar {
    fn default() -> Sidecar {
        Sidecar::new()
    }
}

impl Sidecar {
    /// An empty sidecar: `{"dalica": {"version": 1}}`.
    pub fn new() -> Sidecar {
        Sidecar { root: new_root() }
    }

    /// Parses a sidecar. Accepts a UTF-8 BOM and CRLF line endings; a duplicate key takes the
    /// later value.
    pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Sidecar, ParseError> {
        parse_root(bytes, limits, "photos").map(|root| Sidecar { root })
    }

    /// The document as it is written to disk.
    pub fn print(&self) -> Vec<u8> {
        print::document(&self.root)
    }

    /// `dalica.version`, if it is a non-negative integer.
    pub fn version(&self) -> Option<u64> {
        self.root.get("dalica")?.get("version")?.as_u64()
    }

    /// The names under `photos` (every key, also those whose value is not an object).
    pub fn photo_names(&self) -> impl Iterator<Item = &str> {
        self.root.get("photos").and_then(Value::as_object).into_iter().flat_map(|m| m.keys().map(String::as_str))
    }

    /// `photos.<name>.star`.
    pub fn star(&self, name: &str) -> Field<bool> {
        fields::read_bool(self.photo_entry(name).and_then(|e| e.get("star")))
    }

    /// Sets or clears `photos.<name>.star`. Clearing removes the key, and the photo's entry when
    /// nothing else is left in it. Refused if `photos` or the entry is not an object.
    pub fn set_star(&mut self, name: &str, on: bool) -> Result<(), WrongType> {
        self.set_photo(name, &PhotoValue::Star(on))
    }

    /// Every value of the wrong type at a place Dalica reads.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        dalica_problems(&self.root, &mut out);
        self.field_problems(&mut out);
        out
    }
}

/// Parses a whole document whose top-level object `entries` is keyed by file names: a UTF-8 BOM
/// and CRLF accepted, a duplicate key takes the later value, the limits checked.
pub(crate) fn parse_root(bytes: &[u8], limits: &Limits, entries: &str) -> Result<Map<String, Value>, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::Limit(format!("{} bytes", bytes.len())));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let value: Value = serde_json::from_slice(bytes).map_err(|e| ParseError::Json { line: e.line(), column: e.column(), message: e.to_string() })?;
    check_limits(&value, limits, 0, false, entries)?;
    match value {
        Value::Object(root) => Ok(root),
        _ => Err(ParseError::NotAnObject),
    }
}

/// `{"dalica": {"version": 1}}`, the start of every document Dalica writes.
pub(crate) fn new_root() -> Map<String, Value> {
    let mut dalica = Map::new();
    dalica.insert("version".into(), Value::from(VERSION));
    let mut root = Map::new();
    root.insert("dalica".into(), Value::Object(dalica));
    root
}

/// The problems of the `"dalica"` object itself.
pub(crate) fn dalica_problems(root: &Map<String, Value>, out: &mut Vec<Problem>) {
    match root.get("dalica") {
        None | Some(Value::Object(_)) => {}
        Some(_) => out.push(Problem { pointer: "/dalica".into(), expected: "an object" }),
    }
    if let Some(v) = root.get("dalica").and_then(|d| d.get("version"))
        && v.as_u64().is_none()
    {
        out.push(Problem { pointer: "/dalica/version".into(), expected: "a non-negative integer" });
    }
}

/// A JSON-pointer token (RFC 6901): `~` → `~0`, `/` → `~1`.
pub(crate) fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Checks `v` against `l`; `entries` names the top-level object whose keys are file names
/// (`photos` in a folder sidecar, `originals` in the originals record).
pub(crate) fn check_limits(v: &Value, l: &Limits, depth: usize, in_photos: bool, entries: &str) -> Result<(), ParseError> {
    if depth > l.max_depth {
        return Err(ParseError::Limit(format!("nesting deeper than {}", l.max_depth)));
    }
    match v {
        Value::String(s) if s.len() > l.max_string => Err(ParseError::Limit(format!("a string of {} bytes", s.len()))),
        Value::Array(a) if a.len() > l.max_list => Err(ParseError::Limit(format!("a list of {} entries", a.len()))),
        Value::Array(a) => a.iter().try_for_each(|x| check_limits(x, l, depth + 1, false, entries)),
        Value::Object(m) => {
            let photos_here = depth == 0 && m.get(entries).is_some_and(|p| p.as_object().is_some_and(|p| p.len() > l.max_photos));
            if photos_here {
                return Err(ParseError::Limit(format!("more than {} photos", l.max_photos)));
            }
            if in_photos && depth == 2 && m.len() > l.max_keys_per_entry {
                return Err(ParseError::Limit(format!("a photo entry with {} keys", m.len())));
            }
            for (k, x) in m {
                if k.len() > l.max_string {
                    return Err(ParseError::Limit(format!("a key of {} bytes", k.len())));
                }
                check_limits(x, l, depth + 1, in_photos || (depth == 0 && k == entries), entries)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Whether `name` (a key read from a sidecar) may be used to reach a file in its folder: one plain
/// path component, not empty, not `.` or `..`, no `/`, `\` or NUL, no drive or stream prefix
/// (`C:`, `name:stream`), not a Windows reserved device name (`security.md` §5.4).
pub fn plain_name(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
        "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0', ':']) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

#[cfg(test)]
mod tests;
