//! Sidecar entries travel with their files (`data-model.md` §8): what Dalica says about a photo
//! in its folder's `.dalica.json`, and the photo's originals backup with its record (§6), go
//! where the file goes when Dalica moves, copies or renames it, and a folder merge carries every
//! entry of the photos it moves.
//!
//! - **The entry** ([`entries`]): a moved photo's whole entry under `photos` (unknown keys
//!   included) leaves the source folder's sidecar and enters the destination's under the new name;
//!   a renamed photo's stays in its folder under the new name; a copied photo's is duplicated. The
//!   sidecars are merged entry by entry, never replaced: every other entry and key of both files
//!   stays as it is. When the destination already has an entry under the new name (a stale one,
//!   §7, or one the model just wrote), the two are merged key by key and the carried keys win;
//!   the model's values are written over them by the next write of that folder
//!   ([`crate::SidecarWriter::carry`] does it at once). The names of the albums a carried entry
//!   belongs to (the top-level `albums`) are copied where the destination lacks them, and the
//!   stale marks (`dalica.missing`) of the names involved are dropped.
//! - **Order:** the destination is written first, then the source. A failure in between leaves
//!   the entry in both files ([`CarryError::SourceKept`]): duplicated, never lost. Carrying again is
//!   harmless: an entry no longer in the source is not carried twice, and one in both is merged
//!   again the same way.
//! - **The backup** ([`backups`]): a photo saved by Dalica has its original in
//!   `.dalica-originals/<name>` and its record in `.dalica-originals/.dalica.json`. The backup file
//!   moves or is copied through `dalica-fs` ([`dalica_fs::transfer`]: never replacing a file,
//!   copy-verify-trash across volumes), then the record's entry travels like a sidecar entry. Only
//!   recorded backups travel; a backup of the new name already in the destination is never
//!   touched ([`Refused`]), and the photo's backup then stays where it was.
//! - **Refused, with nothing written:** a folder that is not there (never created), a damaged
//!   sidecar or record (kept aside as `.dalica.json.corrupt-<time>`, as the writer does), a
//!   `photos` or entry of the wrong type where the entry must go, a name that is not one plain path
//!   component. A refusal for one name leaves the others of a batch carried.
//!
//! Any thread but the UI thread; the engine runs [`entries`] on its I/O worker through
//! [`crate::SidecarWriter::carry`] (sequenced with the folders' pending writes) and [`backups`] in
//! the file operation's own work, right after the file moved.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::Expect;
use dalica_fs::transfer::{self, Outcome as Transfer};
use dalica_sidecar::{FILE_NAME, Limits, Originals, Sidecar, originals, plain_name};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Whether the file left its source (a move or a rename) or was duplicated (a copy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Move,
    Copy,
}

/// Entries to carry from one folder to another (the same folder for a rename): each
/// `(name, new name)` pair is one photo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carry {
    pub mode: Mode,
    pub from: PathBuf,
    pub to: PathBuf,
    pub names: Vec<(String, String)>,
}

impl Carry {
    /// One photo `name` in `from` going to `new_name` in `to`.
    pub fn one(mode: Mode, from: &Path, name: &str, to: &Path, new_name: &str) -> Carry {
        Carry { mode, from: from.to_path_buf(), to: to.to_path_buf(), names: vec![(name.into(), new_name.into())] }
    }

    /// Photos renamed in `folder`.
    pub fn rename(folder: &Path, names: Vec<(String, String)>) -> Carry {
        Carry { mode: Mode::Move, from: folder.to_path_buf(), to: folder.to_path_buf(), names }
    }

    fn same_folder(&self) -> bool {
        self.from == self.to
    }
}

/// What a carry did, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// `(name, new name)` of every entry carried.
    pub carried: Vec<(String, String)>,
    /// New names under which the destination already had an entry: merged, the carried keys won.
    pub merged: Vec<String>,
    /// Names the source has no entry for: nothing to carry (carried before, or never said).
    pub absent: Vec<String>,
    /// Names not carried, and why: their entries stay where they are.
    pub refused: Vec<Refused>,
}

impl Carried {
    /// Whether any file was written.
    pub fn changed(&self) -> bool {
        !self.carried.is_empty()
    }
}

/// A name a carry left alone, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub name: String,
    pub reason: String,
}

/// Why a carry wrote nothing (or, for [`CarryError::SourceKept`], only the destination).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CarryError {
    /// The folder is not there (an offline volume, or moved away): nothing is created in it.
    #[error("{}: the folder is not there; nothing was carried", .0.display())]
    Missing(PathBuf),
    /// A sidecar or originals record could not be parsed: kept aside, nothing carried.
    #[error("{}: damaged ({why}); kept as {}; nothing was carried", .path.display(), .kept.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| "it is".into()))]
    SetAside { path: PathBuf, why: String, kept: Option<PathBuf> },
    /// The folder's sidecar was damaged earlier and the folder is read-only until the user resets
    /// it ([`crate::SidecarWriter::clear_read_only`]).
    #[error("{}: its sidecar was damaged and nothing is written there; nothing was carried", .0.display())]
    ReadOnly(PathBuf),
    /// A file could not be read or written.
    #[error("{0}")]
    Fs(String),
    /// The destination holds the entries now but the source could not be written: they are in
    /// both files (carry again to finish).
    #[error("the entries reached {} but could not leave {}: {why}", .to.display(), .from.display())]
    SourceKept { from: PathBuf, to: PathBuf, why: String, carried: Box<Carried> },
}

/// Which of Dalica's two per-folder documents: the folder sidecar or the originals record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doc {
    Sidecar,
    Record,
}

impl Doc {
    /// The key whose object holds one entry per file name.
    fn key(self) -> &'static str {
        match self {
            Doc::Sidecar => "photos",
            Doc::Record => originals::KEY,
        }
    }

    /// The document's canonical bytes (`print`) if it parses within the limits.
    fn canon(self, bytes: &[u8], limits: &Limits) -> Result<Vec<u8>, String> {
        match self {
            Doc::Sidecar => Sidecar::parse(bytes, limits).map(|s| s.print()).map_err(|e| e.to_string()),
            Doc::Record => Originals::parse(bytes, limits).map(|s| s.print()).map_err(|e| e.to_string()),
        }
    }

    fn empty(self) -> Vec<u8> {
        match self {
            Doc::Sidecar => Sidecar::new().print(),
            Doc::Record => Originals::new().print(),
        }
    }
}

/// One document read for a carry: its JSON, its bytes as read and what the write must expect.
struct Loaded {
    path: PathBuf,
    doc: Doc,
    root: Map<String, Value>,
    before: Option<Vec<u8>>,
    expect: Expect,
}

fn object(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err("the top level is not a JSON object".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// `corrupt-20261010T051500Z` for `now`.
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_else(|| now.as_millis().to_string());
    format!("corrupt-{stamp}Z")
}

/// Reads `path` (absent: an empty document); a damaged one is kept aside and refused.
fn load(path: &Path, doc: Doc, now: Timestamp) -> Result<Loaded, CarryError> {
    let limits = Limits::default();
    let damaged = |why: String| CarryError::SetAside { path: path.to_path_buf(), kept: dalica_fs::set_aside(path, &corrupt_tag(now)).ok(), why };
    let (canon, before, expect) = match dalica_fs::read(path, limits.max_bytes as u64) {
        Ok(None) => (doc.empty(), None, Expect::Absent),
        Ok(Some(read)) => match doc.canon(&read.bytes, &limits) {
            Ok(canon) => (canon, Some(read.bytes), Expect::Unchanged(read.identity)),
            Err(why) => return Err(damaged(why)),
        },
        Err(e @ dalica_fs::Error::TooLarge { .. }) => return Err(damaged(e.to_string())),
        Err(e) => return Err(CarryError::Fs(e.to_string())),
    };
    let root = object(&canon).map_err(CarryError::Fs)?;
    Ok(Loaded { path: path.to_path_buf(), doc, root, before, expect })
}

/// Writes `l` back if its bytes changed (a document that never existed and says nothing is not
/// created), through the atomic replace over the version read.
fn save(l: &Loaded) -> Result<(), String> {
    let json = serde_json::to_vec(&Value::Object(l.root.clone())).map_err(|e| e.to_string())?;
    let bytes = l.doc.canon(&json, &Limits::default()).map_err(|e| format!("{}: {e}", l.path.display()))?;
    if l.before.as_deref() == Some(bytes.as_slice()) || (l.before.is_none() && bytes == l.doc.empty()) {
        return Ok(());
    }
    dalica_fs::replace(&l.path, &bytes, l.expect).map(|_| ()).map_err(|e| e.to_string())
}

/// Whether `folder` is there (a folder, not through a link).
fn present(folder: &Path) -> Result<bool, CarryError> {
    match dalica_fs::stat(folder) {
        Ok(Some(s)) => Ok(!s.is_file),
        Ok(None) => Ok(false),
        Err(e) => Err(CarryError::Fs(e.to_string())),
    }
}

/// The entry objects of `root` (`photos` or `originals`): `Ok(None)` when absent, `Err` when not an
/// object.
fn entries_of<'a>(root: &'a Map<String, Value>, key: &str) -> Result<Option<&'a Map<String, Value>>, ()> {
    match root.get(key) {
        None => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        Some(_) => Err(()),
    }
}

/// Drops the stale marks (`dalica.missing`, `stale.rs`) of `names`, when there are marks.
fn unmark(root: &mut Map<String, Value>, names: &BTreeSet<String>) {
    let Some(Value::Object(dalica)) = root.get_mut("dalica") else { return };
    let Some(Value::Object(missing)) = dalica.get_mut(crate::stale::MISSING_KEY) else { return };
    let before = missing.len();
    missing.retain(|n, _| !names.contains(n));
    if missing.is_empty() && before > 0 {
        dalica.remove(crate::stale::MISSING_KEY);
    }
}

/// The pure part of a carry: takes the entries of `c.names` out of `src` (for a move) and merges
/// them into `dst` (`None`: the same document as `src`). Names it cannot carry are refused one by
/// one and left where they are.
fn carry_in(c: &Carry, doc: Doc, src: &mut Map<String, Value>, dst: Option<&mut Map<String, Value>>) -> Carried {
    let key = doc.key();
    let mut out = Carried::default();
    let refuse = |out: &mut Carried, name: &str, reason: &str| out.refused.push(Refused { name: name.into(), reason: reason.into() });
    // What to take: the source's entries, checked against the destination's place for them.
    let src_entries = match entries_of(src, key) {
        Ok(e) => e.cloned().unwrap_or_default(),
        Err(()) => {
            for (n, _) in &c.names {
                refuse(&mut out, n, &format!("the source's `{key}` is not an object"));
            }
            return out;
        }
    };
    let dst_entries = match dst.as_deref().map_or_else(|| entries_of(src, key), |d| entries_of(d, key)) {
        Ok(e) => e.cloned().unwrap_or_default(),
        Err(()) => {
            for (n, _) in &c.names {
                refuse(&mut out, n, &format!("the destination's `{key}` is not an object"));
            }
            return out;
        }
    };
    let mut taken: Vec<(String, String, Value)> = Vec::new();
    let mut targets = BTreeSet::new();
    for (name, new) in &c.names {
        if !plain_name(name) || !plain_name(new) {
            refuse(&mut out, name, "not a plain file name");
            continue;
        }
        if !targets.insert(new.clone()) {
            refuse(&mut out, name, "another entry of this carry goes to the same name");
            continue;
        }
        let Some(entry) = src_entries.get(name) else {
            out.absent.push(name.clone());
            continue;
        };
        if c.same_folder() && name == new {
            // Nothing moves: the entry already is where it goes.
            out.absent.push(name.clone());
            continue;
        }
        // The entry the destination already holds under the new name, unless it is one leaving.
        let leaving = c.same_folder() && c.mode == Mode::Move && c.names.iter().any(|(n, _)| n == new);
        match (dst_entries.get(new).filter(|_| !leaving), entry) {
            (None, _) => {}
            (Some(Value::Object(_)), Value::Object(_)) => out.merged.push(new.clone()),
            (Some(old), e) if old == e => out.merged.push(new.clone()),
            (Some(_), _) => {
                refuse(&mut out, name, "the destination's entry of that name is not an object; it is kept and not written over");
                continue;
            }
        }
        taken.push((name.clone(), new.clone(), entry.clone()));
    }
    if taken.is_empty() {
        return out;
    }
    // Out of the source (a move), marks of the names that left included.
    if c.mode == Mode::Move {
        let gone: BTreeSet<String> = taken.iter().map(|(n, _, _)| n.clone()).collect();
        if let Some(Value::Object(m)) = src.get_mut(key) {
            m.retain(|n, _| !gone.contains(n));
            if m.is_empty() {
                src.remove(key);
            }
        }
        unmark(src, &gone);
    }
    // Album names the carried entries need, from the source's top level.
    let src_names = match src.get(dalica_sidecar::fields::ALBUMS) {
        Some(Value::Object(m)) if doc == Doc::Sidecar => m.clone(),
        _ => Map::new(),
    };
    let dst: &mut Map<String, Value> = match dst {
        Some(d) => d,
        None => src,
    };
    let entries = dst.entry(key).or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(entries) = entries else { return out };
    let mut albums = BTreeSet::new();
    for (_, new, entry) in &taken {
        if let Some(Value::Object(a)) = entry.get(dalica_sidecar::fields::ALBUMS) {
            albums.extend(a.keys().cloned());
        }
        match (entries.get_mut(new), entry) {
            (Some(Value::Object(old)), Value::Object(e)) => {
                for (k, v) in e {
                    old.insert(k.clone(), v.clone());
                }
            }
            _ => {
                entries.insert(new.clone(), entry.clone());
            }
        }
    }
    let arrived: BTreeSet<String> = taken.iter().map(|(_, n, _)| n.clone()).collect();
    unmark(dst, &arrived);
    let wanted: Vec<(String, Value)> = albums.into_iter().filter_map(|id| src_names.get(&id).map(|v| (id, v.clone()))).collect();
    if !wanted.is_empty() {
        let names = dst.entry(dalica_sidecar::fields::ALBUMS).or_insert_with(|| Value::Object(Map::new()));
        // A top-level `albums` of the wrong type is never written over.
        if let Value::Object(names) = names {
            for (id, v) in wanted {
                names.entry(id).or_insert(v);
            }
        }
    }
    out.carried = taken.into_iter().map(|(n, new, _)| (n, new)).collect();
    out
}

/// Carries the entries of `c.names` between the two folders' documents at `src_path` and
/// `dst_path` (one file for a rename).
fn carry_files(c: &Carry, doc: Doc, src_path: &Path, dst_path: &Path, now: Timestamp) -> Result<Carried, CarryError> {
    let mut src = load(src_path, doc, now)?;
    if c.same_folder() {
        let carried = carry_in(c, doc, &mut src.root, None);
        if carried.changed() {
            save(&src).map_err(CarryError::Fs)?;
        }
        return Ok(carried);
    }
    let mut dst = load(dst_path, doc, now)?;
    let carried = carry_in(c, doc, &mut src.root, Some(&mut dst.root));
    if !carried.changed() {
        return Ok(carried);
    }
    // The destination first: a failure after it leaves the entries in both, never in neither.
    save(&dst).map_err(CarryError::Fs)?;
    if c.mode == Mode::Move
        && let Err(why) = save(&src)
    {
        return Err(CarryError::SourceKept { from: src_path.to_path_buf(), to: dst_path.to_path_buf(), why, carried: Box::new(carried) });
    }
    Ok(carried)
}

/// Carries the folder sidecar entries of `c.names` from `c.from` to `c.to` (see the module docs).
/// Both folders must be there; nothing is created but the destination's sidecar when it had none.
pub fn entries(c: &Carry, now: Timestamp) -> Result<Carried, CarryError> {
    for folder in [&c.from, &c.to] {
        if !present(folder)? {
            return Err(CarryError::Missing(folder.clone()));
        }
    }
    carry_files(c, Doc::Sidecar, &c.from.join(FILE_NAME), &c.to.join(FILE_NAME), now)
}

/// Carries the originals backups of `c.names` and their record entries from `c.from` to `c.to`
/// (see the module docs): each recorded backup moves (or, for a copy, is copied) through
/// `dalica-fs` under the new name into the destination's `.dalica-originals/` (made if needed,
/// never through a link), then the record entries travel like sidecar entries. A name whose backup
/// cannot travel (one of that name is already in the destination, a failed move) is refused and
/// its record entry stays with its backup. Carrying again after an interruption finishes the job:
/// a backup already in the destination with the recorded size counts as carried.
pub fn backups(c: &Carry, now: Timestamp) -> Result<Carried, CarryError> {
    for folder in [&c.from, &c.to] {
        if !present(folder)? {
            return Err(CarryError::Missing(folder.clone()));
        }
    }
    let src_dir = c.from.join(originals::DIR_NAME);
    if !present(&src_dir)? {
        // Nothing of the source folder was ever saved.
        return Ok(Carried { absent: c.names.iter().map(|(n, _)| n.clone()).collect(), ..Carried::default() });
    }
    let record = load(&src_dir.join(originals::FILE_NAME), Doc::Record, now)?;
    let recorded = match entries_of(&record.root, originals::KEY) {
        Ok(e) => e.cloned().unwrap_or_default(),
        Err(()) => Map::new(),
    };
    let mut out = Carried::default();
    let mut go = Vec::new();
    let mut dst_dir: Option<PathBuf> = None;
    for (name, new) in &c.names {
        let Some(entry) = recorded.get(name) else {
            out.absent.push(name.clone());
            continue;
        };
        if !plain_name(name) || !plain_name(new) {
            out.refused.push(Refused { name: name.clone(), reason: "not a plain file name".into() });
            continue;
        }
        if c.same_folder() && name == new {
            out.absent.push(name.clone());
            continue;
        }
        let dir = match &dst_dir {
            Some(d) => d.clone(),
            None => match dalica_fs::originals::ensure_dir(&c.to) {
                Ok(d) => {
                    dst_dir = Some(d.clone());
                    d
                }
                Err(e) => return Err(CarryError::Fs(e.to_string())),
            },
        };
        match carry_backup(c.mode, &src_dir, name, &dir, new, entry.get("size").and_then(Value::as_u64)) {
            Ok(()) => go.push((name.clone(), new.clone())),
            Err(reason) => out.refused.push(Refused { name: name.clone(), reason }),
        }
    }
    if go.is_empty() {
        return Ok(out);
    }
    let records = Carry { mode: c.mode, from: c.from.clone(), to: c.to.clone(), names: go };
    let dst_record = c.to.join(originals::DIR_NAME).join(originals::FILE_NAME);
    let carried = carry_files(&records, Doc::Record, &src_dir.join(originals::FILE_NAME), &dst_record, now)?;
    out.carried = carried.carried;
    out.merged = carried.merged;
    out.absent.extend(carried.absent);
    out.refused.extend(carried.refused);
    Ok(out)
}

/// Moves or copies one backup file; `Ok` when it is in the destination (now or already).
fn carry_backup(mode: Mode, src_dir: &Path, name: &str, dst_dir: &Path, new: &str, size: Option<u64>) -> Result<(), String> {
    let at_src = dalica_fs::identity(&src_dir.join(name)).map_err(|e| e.to_string())?;
    let at_dst = dalica_fs::identity(&dst_dir.join(new)).map_err(|e| e.to_string())?;
    let recorded = |id: &dalica_fs::Identity| size.is_some_and(|s| s == id.size());
    let taken = || format!("a backup named {new} is already in {}; it is kept, and this photo's backup stays where it was", dst_dir.display());
    match (at_src, at_dst) {
        (Some(id), None) => {
            let done = match mode {
                Mode::Move => transfer::move_file(src_dir, name, id, dst_dir, new).map(|o| matches!(o, Transfer::Done(_))),
                Mode::Copy => transfer::copy_file(src_dir, name, id, dst_dir, new).map(|o| matches!(o, Transfer::Done(_))),
            };
            match done {
                Ok(true) => Ok(()),
                Ok(false) => Err(taken()),
                Err(e) => Err(e.to_string()),
            }
        }
        // Carried before an interruption (a copy: copied before).
        (None, Some(d)) if mode == Mode::Move && recorded(&d) => Ok(()),
        (Some(s), Some(d)) if mode == Mode::Copy && recorded(&d) && d.size() == s.size() => Ok(()),
        (_, Some(_)) => Err(taken()),
        // A record without its backup: the record travels, as it was.
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn doc(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    fn mv(names: &[(&str, &str)]) -> Carry {
        Carry { mode: Mode::Move, from: "/a".into(), to: "/b".into(), names: names.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }
    }

    #[test]
    fn a_moved_entry_leaves_with_its_unknown_keys_and_album_names() {
        let mut src = doc(json!({
            "dalica": { "version": 1, "missing": { "a.jpg": "2026-01-01T00:00:00Z", "x.jpg": "2026-01-01T00:00:00Z" } },
            "photos": { "a.jpg": { "star": true, "faces": [1], "albums": { "a91c0e": "a1" } }, "b.jpg": { "star": true } },
            "albums": { "a91c0e": { "name": "Trams", "extra": 1 } }
        }));
        let mut dst = doc(
            json!({ "dalica": { "version": 1, "missing": { "n.jpg": "2026-01-01T00:00:00Z" } }, "photos": { "n.jpg": { "caption": "old", "geo": 1 } } }),
        );
        let out = carry_in(&mv(&[("a.jpg", "n.jpg")]), Doc::Sidecar, &mut src, Some(&mut dst));
        assert_eq!(out.carried, [("a.jpg".to_string(), "n.jpg".to_string())]);
        assert_eq!(out.merged, ["n.jpg"]);
        assert_eq!(
            Value::Object(src),
            json!({
                "dalica": { "version": 1, "missing": { "x.jpg": "2026-01-01T00:00:00Z" } },
                "photos": { "b.jpg": { "star": true } },
                "albums": { "a91c0e": { "name": "Trams", "extra": 1 } }
            })
        );
        assert_eq!(
            Value::Object(dst),
            json!({
                "dalica": { "version": 1 },
                "photos": { "n.jpg": { "caption": "old", "geo": 1, "star": true, "faces": [1], "albums": { "a91c0e": "a1" } } },
                "albums": { "a91c0e": { "name": "Trams", "extra": 1 } }
            })
        );
    }

    #[test]
    fn hostile_shapes_are_refused_one_by_one() {
        let mut src = doc(json!({ "photos": { "a.jpg": 7, "b.jpg": { "star": true }, "../c": { "star": true } } }));
        let mut dst = doc(json!({ "photos": { "a.jpg": { "star": true } }, "albums": 3 }));
        let out =
            carry_in(&mv(&[("a.jpg", "a.jpg"), ("b.jpg", "b.jpg"), ("../c", "c"), ("none.jpg", "none.jpg")]), Doc::Sidecar, &mut src, Some(&mut dst));
        assert_eq!(out.carried, [("b.jpg".to_string(), "b.jpg".to_string())]);
        assert_eq!(out.refused.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["a.jpg", "../c"]);
        assert_eq!(out.absent, ["none.jpg"]);
        assert_eq!(src["photos"], json!({ "a.jpg": 7, "../c": { "star": true } }));
        assert_eq!(dst["albums"], json!(3));
        // `photos` of the wrong type on either side: nothing moves.
        let mut bad = doc(json!({ "photos": [1] }));
        let mut ok = doc(json!({ "photos": { "a.jpg": { "star": true } } }));
        let out = carry_in(&mv(&[("a.jpg", "a.jpg")]), Doc::Sidecar, &mut ok, Some(&mut bad));
        assert!(out.carried.is_empty() && out.refused.len() == 1);
        assert_eq!(ok["photos"], json!({ "a.jpg": { "star": true } }));
    }

    #[test]
    fn renames_in_one_folder_may_swap_names() {
        let mut one = doc(json!({ "photos": { "a.jpg": { "caption": "A" }, "b.jpg": { "caption": "B" } } }));
        let c = Carry::rename(Path::new("/f"), vec![("a.jpg".into(), "b.jpg".into()), ("b.jpg".into(), "a.jpg".into())]);
        let out = carry_in(&c, Doc::Sidecar, &mut one, None);
        assert_eq!((out.carried.len(), out.merged.len()), (2, 0));
        assert_eq!(one["photos"], json!({ "a.jpg": { "caption": "B" }, "b.jpg": { "caption": "A" } }));
    }
}
