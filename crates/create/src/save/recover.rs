//! The repair of a Save, Undo Save or Revert that stopped part-way (`data-model.md` §6 and
//! §10.5): a pending entry in a folder's originals record is a Save that did not finish, and a
//! `done` entry whose kept original is back at the photo's name is an Undo Save or Revert that
//! moved the original but did not drop the entry.
//!
//! [`recover`] looks at each such entry and leaves the folder wholly saved or wholly original:
//!
//! - **Rolled forward** (a pending entry marked `done`): the kept original is the recorded one,
//!   and the file at the photo's name is complete and holds the recorded recipe. "Holds" is checked
//!   by content, so it never depends on clocks: the file is not the original, it is not the
//!   version the library last saw there (a later Save's old saved file, [`Seen`]), it decodes whole
//!   under the limits, and its pixels are the original rendered with the recorded recipe through
//!   the one output pipeline, within [`TOLERANCE`] (two encodings of the same pixels).
//! - **Rolled back** (the entry dropped): any other pending entry whose original is in the backup
//!   or still at the name, and every interrupted Undo Save or Revert. The kept original moves back
//!   over whatever is at the name (`fs`'s restore: only while the backup is the recorded original
//!   and the file at the name is the one just looked at); what it replaces is a rendering of the
//!   original that the recipe can make again. The caller puts the recipe back as unsaved edits.
//! - **Left** as found, with the reason: an entry whose original is neither in the backup nor at
//!   the name, a backup that is not the recorded one, a link where a file should be. Nothing is
//!   ever moved or dropped without the recorded original's bytes in hand.
//!
//! Every move is guarded by the identities `fs` checks, so a repair that is itself interrupted
//! leaves a state the next [`recover`] repairs. First, the temporary files a stopped Save left
//! (`fs`'s atomic-replace shape, `security.md` §5.8) are removed by their shape: the photos' own
//! (`.<name>.dalica-tmp-<16 hex>`, for the names the record holds) and the record's in
//! `.dalica-originals/`, and only those last modified before the repair began, so a write
//! running meanwhile keeps its file.
//!
//! [`check`] answers, reading only, whether a folder needs [`recover`]. Both touch the disk only
//! through `dalica-fs`: call them on a worker, never on the UI thread.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use dalica_core::Timestamp;
use dalica_edit::Recipe;
use dalica_fs::{Expect, Identity, originals as backup};
use dalica_sidecar::originals::{Entry, FILE_NAME, State};

use super::record::{Record, digest_matches};
use super::{Error, MAX_FILE, Options, Result, Seen, backup_name_ok, check_recipe};
use crate::output::{self, Purpose, Spec};

/// The largest mean difference per sample (0–255) between the file at the photo's name and the
/// original rendered again with the recorded recipe for the file to count as holding that recipe:
/// two JPEG encodings of the same pixels at the same quality differ by well under one level, a
/// lossless turn by about one; a different recipe (an older Save's file) by much more.
pub const TOLERANCE: f64 = 3.0;

/// The atomic-replace temporary marker (`dalica_fs::atomic::is_temp_name`).
const TEMP_MARK: &str = ".dalica-tmp-";
/// The longest part of a target's name a temporary name keeps (`dalica-fs`'s own bound).
const TEMP_NAME_KEEP: usize = 255 - 1 - TEMP_MARK.len() - 16;

/// What the repair did with one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The Save is finished: the file at the name holds the recorded recipe and the entry is
    /// `done`.
    RolledForward,
    /// Taken back: the original is at its name and the entry is gone. A pending Save's recipe
    /// (`Repair::entry`'s, the whole one) goes back as unsaved edits; so does an interrupted Undo
    /// Save's or Revert's (which of the two it was is not known, so the edits are kept).
    RolledBack,
    /// Left as found, with why.
    Left(String),
}

/// One entry the repair looked at.
#[derive(Clone, Debug, PartialEq)]
pub struct Repair {
    pub name: String,
    /// The entry as found: `pending` (a Save), or `done` (an Undo Save or Revert).
    pub entry: Entry,
    pub outcome: Outcome,
    /// The identity of the file at the name afterwards, when it was rolled one way or the other.
    pub identity: Option<Identity>,
}

/// What [`recover`] did in one folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recovered {
    /// The entries repaired or left, in name order.
    pub repairs: Vec<Repair>,
    /// The temporary files removed.
    pub removed: Vec<PathBuf>,
    /// What could not be looked at or cleaned (the rest went on).
    pub problems: Vec<String>,
}

/// Whether `folder` holds anything [`recover`] would repair or remove: a pending entry, a `done`
/// entry whose kept original is gone from the backup, or a temporary file of the record. Reads
/// only; a record that cannot be read needs no repair here (the scan sets a damaged one aside).
pub fn check(folder: &Path) -> Result<bool> {
    let dir = folder.join(backup::DIR_NAME);
    match dalica_fs::stat(&dir)? {
        Some(s) if !s.is_file => {}
        _ => return Ok(false),
    }
    let Ok(record) = Record::read(folder, Timestamp::from_millis(0), false) else { return Ok(false) };
    for (name, entry) in usable_entries(&record) {
        if entry.state == State::Pending || dalica_fs::stat(&dir.join(&name))?.is_none() {
            return Ok(true);
        }
    }
    Ok(!dalica_fs::list_shaped(&dir, |n| temp_of(n, FILE_NAME))?.is_empty())
}

/// Repairs every interrupted Save, Undo Save and Revert in `folder` (see the module docs), having
/// first removed their temporary files. `seen` is the library's last view of each photo's file (by
/// name); a name missing from it can still be rolled forward by content. `options` gives the
/// decode limits, the encoder's threads and the time the repair began (`options.now`); its `seen`
/// is not used.
///
/// `Err` only when the folder's record cannot be read (a damaged one is set aside first, as Save
/// does) or a simulated crash stopped the repair; anything else is reported per entry.
pub fn recover(folder: &Path, seen: &BTreeMap<String, Seen>, options: &Options) -> Result<Recovered> {
    let mut out = Recovered::default();
    let dir = folder.join(backup::DIR_NAME);
    match dalica_fs::stat(&dir)? {
        Some(s) if !s.is_file => {}
        _ => return Ok(out),
    }
    let mut record = Record::read(folder, options.now, true)?;
    let entries = usable_entries(&record);
    // The temporary files a stopped Save left, first: the record's own writes below never leave
    // a photo's, and an entry dropped below would no longer name its photo's.
    let names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();
    remove_temps(folder, &names, options.now, &mut out);
    remove_temps(&dir, &[FILE_NAME.to_string()], options.now, &mut out);
    for (name, entry) in entries {
        let step = match entry.state {
            State::Pending => {
                Found::look(folder, &name, &entry).and_then(|found| repair_save(folder, &name, &entry, &found, seen.get(&name), options))
            }
            // A saved photo whose original is kept: only a stat (never reading every saved photo).
            State::Done => match dalica_fs::stat(&dir.join(&name)) {
                Ok(Some(_)) => Ok(Step::Nothing),
                Ok(None) => Found::look(folder, &name, &entry).map(|found| repair_restore(&found)),
                Err(e) => Err(e.into()),
            },
        };
        let (outcome, identity) = match step {
            Ok(Step::Nothing) => continue,
            Ok(Step::Forward(id)) => (Outcome::RolledForward, Some(id)),
            Ok(Step::Back(id)) => (Outcome::RolledBack, Some(id)),
            Ok(Step::Left(why)) => (Outcome::Left(why), None),
            Err(e) if crashed(&e) => return Err(e),
            Err(e) => (Outcome::Left(e.reason()), None),
        };
        // The record follows the files: an entry is marked done or dropped only after the move.
        let written = match &outcome {
            Outcome::RolledForward => record.set(&name, &Entry { state: State::Done, ..entry.clone() }).and_then(|()| record.store()),
            Outcome::RolledBack => record.put_back(&name, None).and_then(|()| record.store()),
            Outcome::Left(_) => Ok(()),
        };
        let outcome = match written {
            Ok(()) => outcome,
            Err(e) if crashed(&e) => return Err(e),
            Err(e) => Outcome::Left(format!("the originals record could not be written: {}", e.reason())),
        };
        out.repairs.push(Repair { name, entry, outcome, identity });
    }

    Ok(out)
}

/// The entries of `record` Dalica can use, in name order (the others stay in the file).
fn usable_entries(record: &Record) -> Vec<(String, Entry)> {
    record.doc.entries().into_iter().filter(|(name, _)| backup_name_ok(name)).collect()
}

/// What the repair does with one entry.
enum Step {
    /// Nothing to repair.
    Nothing,
    /// Mark the entry done; the file at the name (this identity) holds its recipe.
    Forward(Identity),
    /// Drop the entry; the original is at its name (this identity).
    Back(Identity),
    /// Leave it, with why.
    Left(String),
}

/// The state of a photo's two places, as the repair finds them.
struct Found {
    /// The kept original in `.dalica-originals/`.
    backup: Place,
    /// The file at the photo's name.
    photo: Place,
}

enum Place {
    Absent,
    /// A regular file: its identity, modification time and bytes, and whether they are the
    /// recorded original.
    File {
        identity: Identity,
        mtime: Timestamp,
        bytes: Vec<u8>,
        original: bool,
    },
    /// Something else (a link, a folder), a file too large to read or one that changed while it
    /// was read: never touched.
    Other(String),
}

impl Found {
    fn look(folder: &Path, name: &str, entry: &Entry) -> Result<Found> {
        Ok(Found { backup: Place::look(&folder.join(backup::DIR_NAME).join(name), entry)?, photo: Place::look(&folder.join(name), entry)? })
    }
}

impl Place {
    fn look(path: &Path, entry: &Entry) -> Result<Place> {
        let stat = match dalica_fs::stat(path) {
            Ok(None) => return Ok(Place::Absent),
            Ok(Some(s)) if s.is_file => s,
            Ok(Some(_)) => return Ok(Place::Other(format!("{} is not a regular file", path.display()))),
            Err(dalica_fs::Error::Link(p)) => return Ok(Place::Other(format!("{} is a link", p.display()))),
            Err(e) => return Err(e.into()),
        };
        if stat.identity.size() > MAX_FILE {
            return Ok(Place::Other(format!("{} is too large to check", path.display())));
        }
        let Some(read) = dalica_fs::read(path, MAX_FILE)? else { return Ok(Place::Absent) };
        if read.identity != stat.identity {
            return Ok(Place::Other(format!("{} changed while it was read", path.display())));
        }
        let original = read.identity.size() == entry.size && digest_matches(&entry.digest, &mut read.bytes.as_slice()).unwrap_or(false);
        let mtime = Timestamp::from_system_time(stat.times.modified);
        Ok(Place::File { identity: read.identity, mtime, bytes: read.bytes, original })
    }
}

/// A pending entry: rolled forward when the file at the name holds its recipe, else rolled back
/// (the original moved back, or found still at its name), else left.
fn repair_save(folder: &Path, name: &str, entry: &Entry, found: &Found, seen: Option<&Seen>, options: &Options) -> Result<Step> {
    let original = match (&found.backup, &found.photo) {
        (Place::File { bytes, original: true, .. }, _) => bytes,
        // The original never left its name (the Save stopped before or at the move): only the
        // entry goes.
        (Place::Absent, Place::File { identity, original: true, .. }) => return Ok(Step::Back(*identity)),
        (Place::Absent, _) => return Ok(Step::Left("the original is neither kept nor at its name".into())),
        (Place::Other(why), _) => return Ok(Step::Left(why.clone())),
        (Place::File { .. }, _) => return Ok(Step::Left("the kept original is not the one recorded".into())),
    };
    let at_name = match &found.photo {
        Place::File { identity, mtime, bytes, original: false } => {
            // The version the library last saw there is an older Save's file, not this one's.
            let unseen = seen.is_none_or(|s| (s.size, s.mtime) != (identity.size(), *mtime));
            if unseen && holds(original, bytes, &entry.recipe, options) {
                return Ok(Step::Forward(*identity));
            }
            Expect::Unchanged(*identity)
        }
        Place::File { identity, .. } => Expect::Unchanged(*identity),
        Place::Absent => Expect::Absent,
        Place::Other(why) => return Ok(Step::Left(why.clone())),
    };
    let check = |r: &mut dyn io::Read| digest_matches(&entry.digest, r);
    let kept = backup::Recorded { size: entry.size, digest_matches: &check };
    Ok(Step::Back(backup::restore(folder, name, at_name, &kept)?))
}

/// A `done` entry: nothing to do while its kept original is in the backup; an Undo Save or Revert
/// that moved the original back but did not drop the entry is finished by dropping it.
fn repair_restore(found: &Found) -> Step {
    match (&found.backup, &found.photo) {
        (Place::Absent, Place::File { identity, original: true, .. }) => Step::Back(*identity),
        (Place::Absent, _) => Step::Left("the kept original is missing and the file at the name is not the original".into()),
        _ => Step::Nothing,
    }
}

/// Whether `file` holds `original` rendered with `recipe`: it decodes whole, and its pixels are
/// the pipeline's rendering within [`TOLERANCE`]. Anything that cannot be decoded or rendered
/// does not.
fn holds(original: &[u8], file: &[u8], recipe: &[String], options: &Options) -> bool {
    let Ok(recipe) = Recipe::parse(recipe) else { return false };
    if check_recipe(&recipe).is_err() {
        return false;
    }
    let spec = Spec { limits: options.limits, threads: options.threads, write: options.write.clone(), ..Spec::new(Purpose::InPlace) };
    let Ok(out) = output::render(original, &recipe, &spec) else { return false };
    let (Ok(want), Ok(got)) = (dalica_image::decode(&out.bytes, &options.limits), dalica_image::decode(file, &options.limits)) else {
        return false;
    };
    if (want.width, want.height) != (got.width, got.height) || want.data.len() != got.data.len() || want.data.is_empty() {
        return false;
    }
    let sum: u64 = want.data.iter().zip(&got.data).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    (sum as f64 / want.data.len() as f64) <= TOLERANCE
}

/// Removes from `dir` the atomic-replace temporary files of the `targets` last modified at or
/// before `before`. Problems are reported, never fatal.
fn remove_temps(dir: &Path, targets: &[String], before: Timestamp, out: &mut Recovered) {
    if targets.is_empty() {
        return;
    }
    let shape = |n: &str| targets.iter().any(|t| temp_of(n, t));
    let listed = match dalica_fs::list_shaped(dir, shape) {
        Ok(l) => l,
        Err(e) => {
            out.problems.push(format!("temporary files in {} not looked at: {e}", dir.display()));
            return;
        }
    };
    for file in listed {
        let path = dir.join(&file.name);
        let old = match dalica_fs::stat(&path) {
            Ok(Some(s)) => s.is_file && Timestamp::from_system_time(s.times.modified) <= before,
            Ok(None) => false,
            Err(e) => {
                out.problems.push(format!("{} not looked at: {e}", path.display()));
                false
            }
        };
        if !old {
            continue;
        }
        match dalica_fs::remove_shaped(&path, shape) {
            Ok(true) => out.removed.push(path),
            Ok(false) => {}
            Err(e) => out.problems.push(format!("{} not removed: {e}", path.display())),
        }
    }
}

/// Whether `name` is an atomic-replace temporary file of `target`: `.<target>.dalica-tmp-<16 hex>`,
/// the target cut to the length `dalica-fs` keeps.
fn temp_of(name: &str, target: &str) -> bool {
    if !dalica_fs::atomic::is_temp_name(OsStr::new(name)) {
        return false;
    }
    let mut keep = target.len().min(TEMP_NAME_KEEP);
    while !target.is_char_boundary(keep) {
        keep -= 1;
    }
    let stem = target.get(..keep).unwrap_or_default();
    name.strip_prefix('.').and_then(|rest| rest.rsplit_once(TEMP_MARK)).is_some_and(|(t, _)| t == stem)
}

fn crashed(e: &Error) -> bool {
    matches!(e, Error::Fs(dalica_fs::Error::Crashed(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_names_match_their_target_only() {
        assert!(temp_of(".IMG_0001.jpg.dalica-tmp-0123456789abcdef", "IMG_0001.jpg"));
        assert!(temp_of("..dalica.json.dalica-tmp-0123456789abcdef", ".dalica.json"));
        for other in [
            ".IMG_0002.jpg.dalica-tmp-0123456789abcdef",
            ".IMG_0001.jpg.dalica-tmp-0123",
            "IMG_0001.jpg",
            ".IMG_0001.jpg.dalica-tmp-0123456789ABCDEF",
            "..dalica.json.dalica-tmp-0123456789abcdef.x",
        ] {
            assert!(!temp_of(other, "IMG_0001.jpg"), "{other}");
        }
        let long = "é".repeat(150);
        let mut keep = TEMP_NAME_KEEP;
        while !long.is_char_boundary(keep) {
            keep -= 1;
        }
        let tmp = format!(".{}{TEMP_MARK}0123456789abcdef", &long[..keep]);
        assert!(temp_of(&tmp, &long));
    }
}
