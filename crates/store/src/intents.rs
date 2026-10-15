//! The file-operation journal `file-ops.json` (`data-model.md` §4, §8): what Dalica set out to do
//! to real files, written before a move, copy, rename or trash starts and after each item, so a
//! crash at any step leaves enough to finish or undo it at the next open, and [`plan`], which
//! decides per item whether to roll forward or back from what is on disk.
//!
//! ```json
//! {"version": 1, "ops": [{"id": 7, "kind": "move", "direction": "do",
//!   "items": [{"from": "/photos/a/IMG_1.jpg", "to": "/photos/b/IMG_1.jpg", "state": "pending"}]}]}
//! ```
//!
//! - **Writing.** [`Intents::begin`] enters an operation with every item `pending` and is on disk
//!   when it returns; [`Intents::item`] records an item's progress (`placed`: a cross-volume copy
//!   in place and verified, its source not yet in the trash; `done`); [`Intents::retarget`] records
//!   the name a clash gave before the file moves there; [`Intents::end`] removes the operation once
//!   its results are in the library. Every write replaces the whole file atomically through
//!   `dalica-fs`, over the version this journal last wrote; it runs on the engine's I/O worker.
//!   Item progress of a large operation is written in batches (at most [`ITEM_WRITES`] rewrites
//!   per pass over its items; every update of an operation of up to that many items is written at
//!   once), so a move of thousands of photos does not rewrite the file thousands of times. Nothing
//!   depends on an unwritten update: [`plan`] decides from the disk, the state only refines it.
//! - **Reading.** [`Intents::open`] reads what an interrupted session left. A file that is not
//!   JSON, of another version, over [`MAX_BYTES`], or with an operation that does not read (a
//!   wrong type, a relative path or one with `..`, a duplicate id, an item without the `to` its
//!   kind needs, past [`MAX_OPS`] or [`MAX_ITEMS`]) is kept aside as `file-ops.json.corrupt-<time>`
//!   and reported; the operations that read are kept and written to a new journal.
//! - **Paths** are absolute and UTF-8, like every path in the index; [`Intents::begin`] refuses
//!   others with nothing written.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::Expect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::carry::Mode;

/// The journal's file name in the library directory.
pub const FILE: &str = "file-ops.json";
/// The format version written and read.
pub const VERSION: u64 = 1;
/// The largest journal read or written.
pub const MAX_BYTES: u64 = 256 << 20;
/// The most operations the journal holds at once.
pub const MAX_OPS: usize = 4096;
/// The most items over all the operations it holds.
pub const MAX_ITEMS: usize = 1_000_000;
/// The longest path an item may name, in bytes.
pub const MAX_PATH: usize = 32 * 1024;
/// The most rewrites per pass over an operation's items (see the module docs).
pub const ITEM_WRITES: usize = 64;

/// What an operation does to the files (the engine's kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Move,
    Copy,
    Rename,
    Trash,
    MoveFolder,
    RenameFolder,
    TrashFolder,
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::Move, Kind::Copy, Kind::Rename, Kind::Trash, Kind::MoveFolder, Kind::RenameFolder, Kind::TrashFolder];

    /// Whether its items name where they go.
    pub fn has_target(self) -> bool {
        !matches!(self, Kind::Trash | Kind::TrashFolder)
    }
}

/// Which way an operation ran: done the first time, undone, or done again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Do,
    Undo,
    Redo,
}

/// How far an item got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Entered; maybe started.
    Pending,
    /// A cross-volume move whose copy is in place and verified while its source is not yet in the
    /// trash.
    Placed,
    /// Its files are done (its sidecar entry may still have to be carried).
    Done,
}

impl State {
    pub const ALL: [State; 3] = [State::Pending, State::Placed, State::Done];
}

/// One file or folder of an operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub from: PathBuf,
    #[serde(default)]
    pub to: Option<PathBuf>,
    pub state: State,
}

/// One operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub id: u64,
    pub kind: Kind,
    #[serde(default)]
    pub direction: Direction,
    pub items: Vec<Item>,
}

impl Intent {
    /// Why this operation cannot be journalled or read back, if it cannot.
    fn check(&self) -> Result<(), String> {
        for (i, item) in self.items.iter().enumerate() {
            check_path(&item.from).map_err(|e| format!("item {i}: from: {e}"))?;
            match (&item.to, self.kind.has_target()) {
                (Some(to), _) => check_path(to).map_err(|e| format!("item {i}: to: {e}"))?,
                (None, true) => return Err(format!("item {i}: a {:?} item names where it goes", self.kind)),
                (None, false) => {}
            }
        }
        Ok(())
    }
}

fn check_path(p: &Path) -> Result<(), String> {
    let Some(s) = p.to_str() else { return Err("not UTF-8".into()) };
    if s.len() > MAX_PATH {
        return Err(format!("longer than {MAX_PATH} bytes"));
    }
    if !p.is_absolute() {
        return Err(format!("{s:?} is not absolute"));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) || s.contains('\0') {
        return Err(format!("{s:?} is not a plain absolute path"));
    }
    Ok(())
}

/// Something [`Intents::open`] found wrong with the journal on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file could not be used whole: kept aside as `kept`; `kept_ops` of its operations read
    /// and are in the new journal, the others (and `why`) are in the kept file.
    SetAside { why: String, kept: PathBuf, kept_ops: usize },
}

/// Why a journal call failed. Nothing changed on disk; the in-memory journal is as it was, except
/// after a failed item write (the update is kept and written with the next one).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Fs(String),
    #[error("operation {0} is already in the file-operation journal")]
    Duplicate(u64),
    #[error("no operation {0} in the file-operation journal")]
    UnknownOp(u64),
    #[error("no item {index} in operation {op}")]
    UnknownItem { op: u64, index: usize },
    #[error("the file-operation journal cannot hold this operation: {0}")]
    Refused(String),
}

/// What [`Intents::open`] gives back.
#[derive(Debug)]
pub struct Opened {
    pub intents: Intents,
    pub problems: Vec<Problem>,
}

/// The file-operation journal of one library directory.
#[derive(Debug)]
pub struct Intents {
    path: PathBuf,
    ops: BTreeMap<u64, Intent>,
    /// What the file on disk is (the version this journal last read or wrote).
    expect: Expect,
    /// Per operation, item updates not yet written.
    unwritten: BTreeMap<u64, usize>,
}

#[derive(Serialize)]
struct FileOut<'a> {
    version: u64,
    ops: Vec<&'a Intent>,
}

/// `corrupt-20261010T051500Z` for `now`.
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_else(|| now.as_millis().to_string());
    format!("corrupt-{stamp}Z")
}

/// The operations of a journal's bytes that read, and why the rest do not (`Err`: none read).
pub fn parse(bytes: &[u8]) -> Result<(Vec<Intent>, Vec<String>), String> {
    let root = match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return Err("the top level is not a JSON object".into()),
        Err(e) => return Err(format!("not valid JSON: {e}")),
    };
    match root.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(v) => return Err(format!("version {v}; this Dalica reads version {VERSION}")),
        None => return Err("no version".into()),
    }
    let Some(Value::Array(ops)) = root.get("ops") else { return Err("`ops` is not a list".into()) };
    let (mut good, mut bad) = (Vec::new(), Vec::new());
    let mut items = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for (i, v) in ops.iter().enumerate() {
        if good.len() >= MAX_OPS {
            bad.push(format!("op {i}: more than {MAX_OPS} operations"));
            continue;
        }
        let intent: Intent = match serde_json::from_value(v.clone()) {
            Ok(x) => x,
            Err(e) => {
                bad.push(format!("op {i}: {e}"));
                continue;
            }
        };
        if let Err(e) = intent.check() {
            bad.push(format!("op {i}: {e}"));
            continue;
        }
        if !seen.insert(intent.id) {
            bad.push(format!("op {i}: id {} is used twice", intent.id));
            continue;
        }
        match items.checked_add(intent.items.len()).filter(|n| *n <= MAX_ITEMS) {
            Some(n) => items = n,
            None => {
                bad.push(format!("op {i}: more than {MAX_ITEMS} items"));
                continue;
            }
        }
        good.push(intent);
    }
    Ok((good, bad))
}

impl Intents {
    /// Opens the journal in the library directory `dir` (locked by the caller): the operations an
    /// interrupted session left are in [`Intents::ops`]. A damaged file is kept aside and
    /// reported (see the module docs); only an I/O error fails.
    pub fn open(dir: &Path, now: Timestamp) -> Result<Opened, Error> {
        let path = dir.join(FILE);
        let fs_err = |e: dalica_fs::Error| Error::Fs(e.to_string());
        let mut intents = Intents { path: path.clone(), ops: BTreeMap::new(), expect: Expect::Absent, unwritten: BTreeMap::new() };
        let read = match dalica_fs::read(&path, MAX_BYTES) {
            Ok(None) => return Ok(Opened { intents, problems: Vec::new() }),
            Ok(Some(read)) => Ok(read),
            Err(e @ dalica_fs::Error::TooLarge { .. }) => Err(e.to_string()),
            Err(e) => return Err(fs_err(e)),
        };
        let (ops, bad) = match read {
            Ok(read) => match parse(&read.bytes) {
                Ok((ops, bad)) if bad.is_empty() => {
                    intents.expect = Expect::Unchanged(read.identity);
                    intents.ops = ops.into_iter().map(|o| (o.id, o)).collect();
                    return Ok(Opened { intents, problems: Vec::new() });
                }
                Ok(read) => read,
                Err(why) => (Vec::new(), vec![why]),
            },
            Err(why) => (Vec::new(), vec![why]),
        };
        let why = bad.join("; ");
        // Damaged: kept aside whole; what read goes into a new journal.
        let Ok(kept) = dalica_fs::set_aside(&path, &corrupt_tag(now)) else {
            // Still in place: the new journal must not write over it.
            return Err(Error::Fs(format!("{}: damaged ({why}) and it could not be kept aside", path.display())));
        };
        let kept_ops = ops.len();
        intents.ops = ops.into_iter().map(|o| (o.id, o)).collect();
        if !intents.ops.is_empty() {
            intents.write()?;
        }
        Ok(Opened { intents, problems: vec![Problem::SetAside { why, kept, kept_ops }] })
    }

    /// The operations in the journal, by id.
    pub fn ops(&self) -> impl Iterator<Item = &Intent> {
        self.ops.values()
    }

    pub fn get(&self, id: u64) -> Option<&Intent> {
        self.ops.get(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// An id no operation in the journal has (one past the largest), for a session that starts
    /// numbering its operations while an earlier session's are still here.
    pub fn next_id(&self) -> u64 {
        self.ops.keys().next_back().map_or(1, |k| k.saturating_add(1))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Enters `intent` (its items as given, normally all `pending`); on disk when this returns.
    pub fn begin(&mut self, intent: Intent) -> Result<(), Error> {
        if self.ops.contains_key(&intent.id) {
            return Err(Error::Duplicate(intent.id));
        }
        intent.check().map_err(Error::Refused)?;
        if self.ops.len() >= MAX_OPS {
            return Err(Error::Refused(format!("it already holds {MAX_OPS} operations")));
        }
        let items: usize = self.ops.values().map(|o| o.items.len()).sum();
        if items.saturating_add(intent.items.len()) > MAX_ITEMS {
            return Err(Error::Refused(format!("more than {MAX_ITEMS} items")));
        }
        let id = intent.id;
        self.ops.insert(id, intent);
        if let Err(e) = self.write() {
            self.ops.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Item `index` of operation `op` reached `state`. Written at once for an operation of up to
    /// [`ITEM_WRITES`] items, else in batches (see the module docs); [`Intents::flush`] writes what
    /// waits.
    pub fn item(&mut self, op: u64, index: usize, state: State) -> Result<(), Error> {
        let intent = self.ops.get_mut(&op).ok_or(Error::UnknownOp(op))?;
        let total = intent.items.len();
        let item = intent.items.get_mut(index).ok_or(Error::UnknownItem { op, index })?;
        if item.state == state {
            return Ok(());
        }
        item.state = state;
        let waiting = self.unwritten.entry(op).or_insert(0);
        *waiting = waiting.saturating_add(1);
        if waiting.saturating_mul(ITEM_WRITES) >= total { self.write() } else { Ok(()) }
    }

    /// Item `index` of operation `op` goes to `to` instead (the name a clash gave); on disk when
    /// this returns, so call it before the file moves there.
    pub fn retarget(&mut self, op: u64, index: usize, to: PathBuf) -> Result<(), Error> {
        check_path(&to).map_err(Error::Refused)?;
        let intent = self.ops.get_mut(&op).ok_or(Error::UnknownOp(op))?;
        let item = intent.items.get_mut(index).ok_or(Error::UnknownItem { op, index })?;
        let old = item.to.replace(to);
        if let Err(e) = self.write() {
            if let Some(item) = self.ops.get_mut(&op).and_then(|i| i.items.get_mut(index)) {
                item.to = old;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Operation `op` is over (its results are in the library): it leaves the journal.
    pub fn end(&mut self, op: u64) -> Result<(), Error> {
        let intent = self.ops.remove(&op).ok_or(Error::UnknownOp(op))?;
        if let Err(e) = self.write() {
            self.ops.insert(op, intent);
            return Err(e);
        }
        self.unwritten.remove(&op);
        Ok(())
    }

    /// Writes the item updates that wait, if any.
    pub fn flush(&mut self) -> Result<(), Error> {
        if self.unwritten.values().any(|n| *n > 0) { self.write() } else { Ok(()) }
    }

    /// Replaces the file with the journal as it is now.
    fn write(&mut self) -> Result<(), Error> {
        let out = FileOut { version: VERSION, ops: self.ops.values().collect() };
        let bytes = serde_json::to_vec(&out).map_err(|e| Error::Refused(e.to_string()))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
            return Err(Error::Refused(format!("larger than {MAX_BYTES} bytes")));
        }
        let identity = match dalica_fs::replace(&self.path, &bytes, self.expect) {
            Ok(identity) => identity,
            Err(e) => {
                // A replace that failed after its rename left the new file: the next write goes
                // over whatever is there now (this process holds the library, nobody else writes).
                if let Ok(now) = dalica_fs::identity(&self.path) {
                    self.expect = now.map_or(Expect::Absent, Expect::Unchanged);
                }
                return Err(Error::Fs(e.to_string()));
            }
        };
        self.expect = Expect::Unchanged(identity);
        self.unwritten.clear();
        Ok(())
    }
}

/// Whether a path is there, as the caller found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Seen {
    Absent,
    Present,
    /// It could not be looked at (an offline volume, an I/O error).
    Unknown,
}

/// What is on disk for one item, as the caller found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Facts {
    /// The item's `from`.
    pub from: Seen,
    /// Its `to` (`Absent` for an item without one).
    pub to: Seen,
    /// Both are there and `to` is a complete copy of `from` (the caller compared them byte for
    /// byte, or for a folder file by file), or the same file.
    pub same: bool,
}

/// What to do about one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Roll back: the item did not happen (or its destination is not the photo's copy). The
    /// source is the photo; nothing on disk changes (a partial copy has a temporary name, which
    /// the stale-temporary sweep removes) and its entry stays where it is.
    Back,
    /// Roll forward: `finish` sends the source to the trash after checking the copy once more (a
    /// cross-volume move interrupted after its copy was placed); `carry` carries the sidecar entry
    /// and the originals backup with their record ([`crate::carry`], harmless when done before).
    Forward { finish: bool, carry: Option<Mode> },
    /// A side could not be looked at: leave the operation in the journal for the next open.
    Wait,
    /// Neither end holds the file: nothing to do; report it.
    Lost,
}

/// The decision for one item, with the reason the repair report gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Decision {
    pub action: Action,
    pub why: &'static str,
}

/// Decides, for every item of `op`, whether to roll it forward or back from `facts` (one per item,
/// in order; an item without facts waits). Pure: it reads nothing but what it is given.
pub fn plan(op: &Intent, facts: &[Facts]) -> Vec<Decision> {
    op.items
        .iter()
        .enumerate()
        .map(|(i, item)| match facts.get(i) {
            Some(f) => decide(op.kind, item.state, *f),
            None => Decision { action: Action::Wait, why: "nothing is known about it" },
        })
        .collect()
}

/// One item's decision (the table of `data-model.md` §8).
pub fn decide(kind: Kind, state: State, f: Facts) -> Decision {
    use Seen::{Absent, Present, Unknown};
    let d = |action, why| Decision { action, why };
    let fwd = |finish, carry| Action::Forward { finish, carry };
    let carry = match kind {
        Kind::Move | Kind::Rename => Some(Mode::Move),
        Kind::Copy => Some(Mode::Copy),
        // A folder's sidecar is inside it; trashed photos leave their entries to the stale sweep.
        Kind::MoveFolder | Kind::RenameFolder | Kind::Trash | Kind::TrashFolder => None,
    };
    match kind {
        Kind::Trash | Kind::TrashFolder => match f.from {
            Unknown => d(Action::Wait, "the file could not be looked at"),
            Present => d(Action::Back, "still in place: not in the trash"),
            Absent => d(fwd(false, None), "in the trash"),
        },
        Kind::Copy => match (f.from, f.to) {
            (Unknown, _) | (_, Unknown) => d(Action::Wait, "a side could not be looked at"),
            (Absent, Absent) => d(Action::Lost, "neither the source nor the copy is there"),
            (Present, Absent) => d(Action::Back, "never copied"),
            (_, Present) if state == State::Done => d(fwd(false, carry), "copied"),
            (Present, Present) if f.same => d(fwd(false, carry), "copied and verified"),
            (Present, Present) => d(Action::Back, "the file at the destination is not the copy"),
            (Absent, Present) => d(Action::Back, "the source is gone and the copy cannot be checked"),
        },
        Kind::Move | Kind::Rename | Kind::MoveFolder | Kind::RenameFolder => match (f.from, f.to) {
            (Unknown, _) | (_, Unknown) => d(Action::Wait, "a side could not be looked at"),
            (Absent, Absent) => d(Action::Lost, "neither the source nor the destination is there"),
            (Present, Absent) => d(Action::Back, "never moved"),
            (Absent, Present) => d(fwd(false, carry), "moved"),
            (Present, Present) if state == State::Done => d(fwd(false, carry), "moved; its source stayed (its volume has no trash)"),
            (Present, Present) if f.same => d(fwd(true, carry), "copied and verified; the source still to go to the trash"),
            (Present, Present) => d(Action::Back, "the file at the destination is not the moved copy"),
        },
    }
}
