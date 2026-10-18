//! `people.json` in the library directory (`data-model.md` §4): every person (name, nickname,
//! thumbnail face) and the rejected suggestions, beside the faces each folder sidecar holds (the
//! source of truth for those) and the names each folder mirrors.
//!
//! ```json
//! {
//!   "dalica": { "version": 1 },
//!   "people": {
//!     "p7f3a2c": {
//!       "name": "Ana Costa",
//!       "nickname": "Aninhas",
//!       "thumbnail": { "photo": "/home/ana/Pictures/Lisbon/IMG_0001.jpg", "rect": [0.41, 0.22, 0.52, 0.38] }
//!     }
//!   },
//!   "rejected": [{ "person": "p7f3a2c", "photo": "/home/ana/Pictures/Porto/IMG_0107.jpg", "rect": [0.1, 0.2, 0.3, 0.4] }]
//! }
//! ```
//!
//! - **Reading** ([`read`], [`reading`]): the people whose id and name can be used, names cleaned
//!   and cut the way the library cleans them (the file keeps them whole), and the rejected
//!   suggestions (a face, by its photo's path and its rectangle, and the person it is not);
//!   anything else is kept in the file and reported once, by id or by position, never by name
//!   (`faces.md` §7). A damaged file (not JSON, not an object, past the limits of `security.md`
//!   §4.2) is set aside as `people.json.corrupt-<time>` and reported; the people then come back
//!   from the folders ([`crate::rebuild`]) and the rejected suggestions start empty.
//! - **Writing** ([`state`], [`write`]): the model's people as an owned [`PeopleState`] (so the
//!   write can run on the I/O worker), merged into the file on disk: entries of people the model
//!   no longer has go, keys and entries Dalica does not know stay, the rejected suggestions are
//!   written only when the state carries them (the model does not: the suggestions engine does),
//!   and the file is written only when its bytes change, through `fs`'s atomic replace over the
//!   version just read. A damaged file is set aside first and a new one written.
//! - Photos are named by absolute path (photo ids belong to one index). A path that is not UTF-8
//!   cannot be written in JSON: such a thumbnail or rejected pair is left out.
//! - Printing is the folder sidecar's: UTF-8, keys sorted by their bytes at every level, two-space
//!   indentation, LF, a final newline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_core::{DateTime, PersonId, Rect01, Timestamp};
use dalica_fs::Expect;
use dalica_library::{Library, MAX_PERSON_NAME};
use serde_json::{Map, Value};

/// The file's name in the library directory.
pub const FILE: &str = "people.json";
/// The object keyed by person ids.
pub const PEOPLE: &str = "people";
/// The list of rejected suggestions.
pub const REJECTED: &str = "rejected";
/// The format version this crate writes.
pub const VERSION: u64 = 1;
/// The largest file read.
pub const MAX_BYTES: u64 = 64 << 20;
/// The deepest nesting read.
pub const MAX_DEPTH: usize = 16;
/// The longest string read.
pub const MAX_STRING: usize = 1 << 20;
/// The most entries of one object or list read.
pub const MAX_ENTRIES: usize = 1_000_000;

/// One person as `people.json` gives them to the model (or as the model says them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersonDef {
    /// Clean and within the library's cap, as a people op takes it.
    pub name: String,
    /// Clean, or empty for none.
    pub nickname: String,
    /// The thumbnail face: its photo's path and its rectangle (stored frame).
    pub thumbnail: Option<(PathBuf, Rect01)>,
}

// A `Rect01` is always finite (never NaN), so equality is reflexive.
impl Eq for PersonDef {}

/// A suggestion the user rejected: the face (its photo's path and its rectangle) is not `person`.
#[derive(Clone, Debug, PartialEq)]
pub struct Rejected {
    pub photo: PathBuf,
    pub rect: Rect01,
    pub person: PersonId,
}

impl Eq for Rejected {}

/// What `people.json` gives the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeopleReading {
    pub people: BTreeMap<PersonId, PersonDef>,
    /// The rejected suggestions, in file order.
    pub rejected: Vec<Rejected>,
    /// One sentence per value or entry not used (it stays in the file).
    pub notes: Vec<String>,
}

/// What reading `people.json` gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleRead {
    /// There is no `people.json`: the people come back from the folders.
    Absent,
    Read(PeopleReading),
    /// It could not be read (a link in its place, a file too large, an I/O error).
    Unreadable(String),
    /// It was damaged: kept aside under this name (`None` when even that failed).
    SetAside {
        why: String,
        kept: Option<PathBuf>,
    },
}

/// The model's people, owned, as `people.json` should say them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeopleState {
    pub people: BTreeMap<PersonId, PersonDef>,
    /// The rejected suggestions to write, or `None` to keep the file's as they are.
    pub rejected: Option<Vec<Rejected>>,
}

/// What [`write`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleWrite {
    Written,
    /// The file already said this.
    Unchanged,
    /// The file on disk was damaged: kept aside under this name (`None` when that failed), and a
    /// new one written.
    SetAsideAndWritten {
        why: String,
        kept: Option<PathBuf>,
    },
    /// Nothing was written, and why (the file is as it was).
    Failed(String),
}

/// `people.json`'s path in the library directory `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Parses a whole `people.json`: a UTF-8 BOM accepted, a duplicate key takes the later value, the
/// limits checked, the top level an object.
pub fn parse(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err(format!("{} bytes is over the limit of {MAX_BYTES}", bytes.len()));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let value: Value = serde_json::from_slice(bytes).map_err(|e| format!("not valid JSON: {e}"))?;
    check_limits(&value, 0)?;
    match value {
        Value::Object(root) => Ok(root),
        _ => Err("the top level is not a JSON object".into()),
    }
}

fn check_limits(v: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("nesting deeper than {MAX_DEPTH}"));
    }
    match v {
        Value::String(s) if s.len() > MAX_STRING => Err(format!("a string of {} bytes", s.len())),
        Value::Array(a) if a.len() > MAX_ENTRIES => Err(format!("a list of {} entries", a.len())),
        Value::Array(a) => a.iter().try_for_each(|x| check_limits(x, depth + 1)),
        Value::Object(m) if m.len() > MAX_ENTRIES => Err(format!("an object of {} entries", m.len())),
        Value::Object(m) => {
            m.iter().try_for_each(|(k, x)| if k.len() > MAX_STRING { Err(format!("a key of {} bytes", k.len())) } else { check_limits(x, depth + 1) })
        }
        _ => Ok(()),
    }
}

/// The document as it is written to disk.
pub fn print(root: &Map<String, Value>) -> Vec<u8> {
    let mut out = String::new();
    print_value(&mut out, &Value::Object(root.clone()), 0);
    out.push('\n');
    out.into_bytes()
}

/// The folder sidecar's printer: keys sorted by their bytes whatever order the map keeps them in,
/// two-space indentation, scalars and keys with `serde_json`'s escaping.
fn print_value(out: &mut String, v: &Value, level: usize) {
    let indent = |out: &mut String, level: usize| out.extend(std::iter::repeat_n("  ", level));
    match v {
        Value::Object(m) if !m.is_empty() => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                indent(out, level + 1);
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push_str(": ");
                if let Some(x) = m.get(*k) {
                    print_value(out, x, level + 1);
                }
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            indent(out, level);
            out.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                indent(out, level + 1);
                print_value(out, x, level + 1);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            indent(out, level);
            out.push(']');
        }
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn new_root() -> Map<String, Value> {
    let mut dalica = Map::new();
    dalica.insert("version".into(), Value::from(VERSION));
    let mut root = Map::new();
    root.insert("dalica".into(), Value::Object(dalica));
    root
}

fn rect_of(v: &Value) -> Option<Rect01> {
    match v.as_array()?.as_slice() {
        [x0, y0, x1, y1] => Rect01::new(x0.as_f64()?, y0.as_f64()?, x1.as_f64()?, y1.as_f64()?).ok(),
        _ => None,
    }
}

fn rect_json(r: &Rect01) -> Value {
    Value::Array([r.x0(), r.y0(), r.x1(), r.y1()].iter().map(|v| Value::from(*v)).collect())
}

/// A face named by photo path and rectangle (`{"photo", "rect"}`), as written.
fn face_of(v: &Value) -> Option<(PathBuf, Rect01)> {
    let o = v.as_object()?;
    let photo = o.get("photo")?.as_str().filter(|p| !p.is_empty())?;
    Some((PathBuf::from(photo), rect_of(o.get("rect")?)?))
}

fn rejected_of(v: &Value) -> Option<Rejected> {
    let (photo, rect) = face_of(v)?;
    let person = PersonId::parse(v.get("person")?.as_str()?).ok()?;
    Some(Rejected { photo, rect, person })
}

/// A usable person entry: a person id with an object holding a name that is something once
/// cleaned. A nickname or thumbnail of the wrong shape reads as none (noted).
fn person_of(id: &str, v: &Value, notes: &mut Vec<String>) -> Option<(PersonId, PersonDef)> {
    let person = PersonId::parse(id).ok()?;
    let o = v.as_object()?;
    let raw = o.get("name")?.as_str()?;
    let name = dalica_library::clean_person_name(raw)?;
    let mut cut = raw.len() > MAX_PERSON_NAME;
    let nickname = match o.get("nickname") {
        None => String::new(),
        Some(Value::String(n)) => {
            cut |= n.len() > MAX_PERSON_NAME;
            dalica_library::clean_person_name(n).unwrap_or_default()
        }
        Some(_) => {
            notes.push(format!("{FILE}: person {person}'s nickname is not text: kept in the file, not used"));
            String::new()
        }
    };
    if cut {
        notes.push(format!("{FILE}: person {person}'s name or nickname is longer than {MAX_PERSON_NAME} bytes: shown cut, kept whole in the file"));
    }
    let thumbnail = match o.get("thumbnail") {
        None => None,
        Some(t) => {
            let face = face_of(t);
            if face.is_none() {
                notes.push(format!("{FILE}: person {person}'s thumbnail is not a photo path with a rectangle: kept in the file, not used"));
            }
            face
        }
    };
    Some((person, PersonDef { name, nickname, thumbnail }))
}

/// What the parsed file gives the model: people with a usable id and name, and the usable
/// rejected suggestions; everything else is noted (by id or position, never by name).
pub fn reading(root: &Map<String, Value>) -> PeopleReading {
    let mut out = PeopleReading::default();
    match root.get(PEOPLE) {
        None => {}
        Some(Value::Object(m)) => {
            for (id, v) in m {
                let mut notes = Vec::new();
                match person_of(id, v, &mut notes) {
                    Some((person, def)) => {
                        out.people.insert(person, def);
                        out.notes.append(&mut notes);
                    }
                    None => out.notes.push(format!("{FILE}: the entry {id:?} is not a person id with a name: kept in the file, not used")),
                }
            }
        }
        Some(_) => out.notes.push(format!("{FILE}: \"{PEOPLE}\" should be an object from person id to person")),
    }
    match root.get(REJECTED) {
        None => {}
        Some(Value::Array(list)) => {
            for (i, v) in list.iter().enumerate() {
                match rejected_of(v) {
                    Some(r) => out.rejected.push(r),
                    None => out.notes.push(format!(
                        "{FILE}: rejected suggestion {i} is not a photo path, a rectangle and a person id: kept in the file, not used"
                    )),
                }
            }
        }
        Some(_) => out.notes.push(format!("{FILE}: \"{REJECTED}\" should be a list of rejected suggestions")),
    }
    out
}

/// Reads `people.json` in the library directory `dir` through `dalica-fs` (never through a link);
/// a damaged one is set aside as `people.json.corrupt-<time>` and reported.
pub fn read(dir: &Path, now: Timestamp) -> PeopleRead {
    let path = path(dir);
    let bytes = match dalica_fs::read(&path, MAX_BYTES) {
        Ok(None) => return PeopleRead::Absent,
        Ok(Some(r)) => r.bytes,
        Err(e) => return PeopleRead::Unreadable(e.to_string()),
    };
    match parse(&bytes) {
        Ok(root) => PeopleRead::Read(reading(&root)),
        Err(why) => PeopleRead::SetAside { why, kept: dalica_fs::set_aside(&path, &corrupt_tag(now)).ok() },
    }
}

/// The model's people as `people.json` should say them: each person's name, nickname and
/// thumbnail face (its photo's path and rectangle; a thumbnail that is no longer theirs is left
/// out). The rejected suggestions are not the model's: `None`.
pub fn state(library: &Library) -> PeopleState {
    let path_of = |id| {
        let p = library.photo(id)?;
        let f = library.folder(p.folder)?;
        Some(f.path.join(&p.name))
    };
    let mut out = PeopleState::default();
    for (id, person) in library.people() {
        let thumbnail = person
            .thumbnail
            .filter(|k| library.faces(k.photo).iter().any(|f| f.rect == k.rect && f.person.person() == Some(id)))
            .and_then(|k| Some((path_of(k.photo)?, k.rect)));
        out.people.insert(id.clone(), PersonDef { name: person.name.clone(), nickname: person.nickname.clone(), thumbnail });
    }
    out
}

fn face_json(photo: &Path, rect: &Rect01, mut o: Map<String, Value>) -> Option<Value> {
    o.insert("photo".into(), Value::String(photo.to_str()?.to_string()));
    o.insert("rect".into(), rect_json(rect));
    Some(Value::Object(o))
}

/// `existing` (or a new file) made to say `state`: entries of people `state` does not have go
/// (entries Dalica cannot use stay), each person's keys are set in the object already there (keys
/// Dalica does not know stay; an empty value removes its key, but a value of the wrong type is
/// written over only by a value of the model's); with `state.rejected`, the usable
/// rejected suggestions become exactly those (each written into the entry it came from, so its
/// unknown keys stay), the unusable ones kept after them. A `"people"` or `"rejected"` of the wrong
/// type is written over only when there is something to write there.
pub fn merged(existing: Option<&Map<String, Value>>, state: &PeopleState) -> Map<String, Value> {
    let mut root = existing.cloned().unwrap_or_else(new_root);
    let mut people = match root.remove(PEOPLE) {
        Some(Value::Object(m)) => m,
        Some(other) if state.people.is_empty() => {
            root.insert(PEOPLE.into(), other);
            Map::new()
        }
        _ => Map::new(),
    };
    people.retain(|id, v| match PersonId::parse(id) {
        Ok(p) if state.people.contains_key(&p) => true,
        _ => person_of(id, v, &mut Vec::new()).is_none(),
    });
    for (id, def) in &state.people {
        let slot = people.entry(id.as_str()).or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Value::Object(entry) = slot else { continue };
        entry.insert("name".into(), Value::String(def.name.clone()));
        // A value of the wrong type is written over only by a value of the model's.
        if !def.nickname.is_empty() {
            entry.insert("nickname".into(), Value::String(def.nickname.clone()));
        } else if entry.get("nickname").is_some_and(Value::is_string) {
            entry.remove("nickname");
        }
        match &def.thumbnail {
            Some((photo, rect)) => {
                let keep = entry.remove("thumbnail").as_ref().and_then(Value::as_object).cloned().unwrap_or_default();
                if let Some(t) = face_json(photo, rect, keep) {
                    entry.insert("thumbnail".into(), t);
                }
            }
            None if entry.get("thumbnail").is_some_and(|t| face_of(t).is_some()) => {
                entry.remove("thumbnail");
            }
            None => {}
        }
    }
    if !people.is_empty() || !state.people.is_empty() {
        root.insert(PEOPLE.into(), Value::Object(people));
    }
    if let Some(rejected) = &state.rejected {
        let old = match root.remove(REJECTED) {
            Some(Value::Array(list)) => list,
            Some(other) if rejected.is_empty() => {
                root.insert(REJECTED.into(), other);
                return without_empty(root);
            }
            _ => Vec::new(),
        };
        let mut usable: Vec<Option<(Rejected, Map<String, Value>)>> = Vec::new();
        let mut kept = Vec::new();
        for v in old {
            match (rejected_of(&v), v) {
                (Some(r), Value::Object(o)) => usable.push(Some((r, o))),
                (_, v) => kept.push(v),
            }
        }
        let mut list: Vec<Value> = Vec::with_capacity(rejected.len() + kept.len());
        for r in rejected {
            let from = usable.iter_mut().find(|u| u.as_ref().is_some_and(|(read, _)| read == r)).and_then(Option::take).map(|(_, o)| o);
            let Some(mut item) = face_json(&r.photo, &r.rect, from.unwrap_or_default()) else { continue };
            if let Value::Object(o) = &mut item {
                o.insert("person".into(), Value::String(r.person.as_str().into()));
            }
            list.push(item);
        }
        list.extend(kept);
        if !list.is_empty() {
            root.insert(REJECTED.into(), Value::Array(list));
        }
    }
    without_empty(root)
}

/// `root` without an empty `"people"` object (nothing to say is no key).
fn without_empty(mut root: Map<String, Value>) -> Map<String, Value> {
    if root.get(PEOPLE).is_some_and(|p| p.as_object().is_some_and(Map::is_empty)) {
        root.remove(PEOPLE);
    }
    root
}

/// Writes `state` into `people.json` in the library directory `dir` ([`merged`] with the file on
/// disk), only when its bytes change, through `fs`'s atomic replace over the version just read.
/// Any thread but the UI thread may call it.
pub fn write(dir: &Path, state: &PeopleState, now: Timestamp) -> PeopleWrite {
    let path = path(dir);
    let read = match dalica_fs::read(&path, MAX_BYTES) {
        Ok(r) => r,
        Err(e) => return PeopleWrite::Failed(e.to_string()),
    };
    let (existing, expect, aside) = match read {
        None => (None, Expect::Absent, None),
        Some(r) => match parse(&r.bytes) {
            Ok(root) => {
                let new = print(&merged(Some(&root), state));
                if new == r.bytes {
                    return PeopleWrite::Unchanged;
                }
                (Some(root), Expect::Unchanged(r.identity), None)
            }
            Err(why) => match dalica_fs::set_aside(&path, &corrupt_tag(now)) {
                Ok(kept) => (None, Expect::Absent, Some((why, Some(kept)))),
                // Never write over a damaged file that could not be kept.
                Err(err) => return PeopleWrite::Failed(format!("{why}; it could not be kept aside: {err}")),
            },
        },
    };
    let bytes = print(&merged(existing.as_ref(), state));
    match dalica_fs::replace(&path, &bytes, expect) {
        Ok(_) => match aside {
            Some((why, kept)) => PeopleWrite::SetAsideAndWritten { why, kept },
            None => PeopleWrite::Written,
        },
        Err(e) => PeopleWrite::Failed(e.to_string()),
    }
}

/// `corrupt-20261010T051500Z` for the time `now`.
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_else(|| now.as_millis().to_string());
    format!("corrupt-{stamp}Z")
}
