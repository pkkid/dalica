//! Repairing a Save that was interrupted (`data-model.md` §6 and §10.5): a Save writes its record
//! into the folder's `.dalica-originals/.dalica.json` as pending before anything moves and marks
//! it done last, so a pending record found later means a Save stopped half-way; a `done` record
//! whose kept original is back at the photo's name is an Undo Save or Revert that stopped before
//! dropping it.
//!
//! The engine looks for them at two moments:
//!
//! - **When the session opens** ([`Session::recover_saves_at_open`]): the folders that can hold
//!   one (a photo with unsaved edits, which a Save may have been writing, or with a saved record)
//!   are checked, reading only, on a worker thread the open waits for (`create`'s
//!   `save::recover::check`: a stat of `.dalica-originals`, the record, a stat per kept original).
//! - **When a scan brings a pending record into the model** ([`Session::note_save_records`],
//!   called by [`Session::apply`] for every op from the files), unless a file job is running: a
//!   Save of that folder may be the one writing it (the model keeps the pending record, so the
//!   next open repairs it if that Save does not finish it).
//!
//! The folders that need it are repaired by one file job ([`LABEL`], `JobKind::Save`) on its
//! worker (`save::recover`: rolled forward when the file at the name holds the recorded recipe,
//! else rolled back, else left with the reason). Back on the owner thread each photo's model
//! follows: rolled forward, it takes the `done` record and keeps only the edits not in the saved
//! file; rolled back, it loses the record and gets the recipe back as unsaved edits (the whole
//! recorded recipe in front of the edits not in it), so a stopped Save never loses an edit; and
//! its entry takes the file's new size and time, so the watcher imports nothing. A repaired name
//! the library does not list (its photo left while the original was kept) has its folder scanned
//! when the job ends. Each repair is logged and kept in [`Session::scan_warnings`] (`ui.inspect`
//! → `notices`; `library.inspect` is not built yet), and the job's end is a notice like any file
//! job's. The owner thread never touches the disk here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_create::save::recover::{self, Outcome, Recovered, Repair};
use dalica_create::save::{self, Seen};
use dalica_library::{Op, Photo, SaveState, Saved};
use dalica_sidecar::originals::{Entry, State};

use crate::filejobs::{Applied, Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::JobKind;
use crate::session::Session;

/// The repair job's label: its progress reads "Repairing an interrupted save 1 of 1".
pub const LABEL: &str = "Repairing an interrupted save";

impl Session {
    /// At session open: the folders that can hold an interrupted Save are checked on a worker
    /// thread, and those that do are repaired by a file job. The owner thread reads nothing.
    pub(crate) fn recover_saves_at_open(&mut self) {
        let lib = self.library();
        let mut candidates: BTreeSet<FolderId> = lib.unsaved_counts().keys().copied().collect();
        candidates.extend(lib.saved_all().filter_map(|(id, _)| lib.photo(id)).map(|p| p.folder));
        let folders = self.online_paths(candidates);
        if folders.is_empty() {
            return;
        }
        let checked = std::thread::scope(|scope| {
            let spawned = std::thread::Builder::new().name("dalica-save-check".into()).spawn_scoped(scope, || needing_repair(folders));
            match spawned {
                Ok(handle) => handle.join().unwrap_or_default(),
                Err(e) => {
                    log::warn!("interrupted saves not looked for: no thread ({e})");
                    Vec::new()
                }
            }
        });
        self.start_repair(checked, &BTreeSet::new());
    }

    /// After an op from the files (a scan's, the watcher's) was applied: the folders whose pending
    /// Save records it brought are repaired, unless a file job is running.
    pub(crate) fn note_save_records(&mut self, op: &Op) {
        let mut photos = Vec::new();
        pending_in(op, &mut photos);
        if photos.is_empty() {
            return;
        }
        if self.file_jobs_running() {
            log::info!("a pending save record waits: a file job is running");
            return;
        }
        let lib = self.library();
        let folders: BTreeSet<FolderId> = photos.iter().filter_map(|id| lib.photo(*id)).map(|p| p.folder).collect();
        let folders = self.online_paths(folders);
        // The same scan may already have taken the new file's size and time into the model, so
        // the model's view of these photos says nothing of which Save wrote them.
        self.start_repair(folders, &photos.into_iter().collect());
    }

    /// The paths of `folders` that are in the library and not offline.
    fn online_paths(&self, folders: BTreeSet<FolderId>) -> Vec<(FolderId, PathBuf)> {
        let lib = self.library();
        folders.into_iter().filter(|f| !self.is_folder_offline(*f)).filter_map(|f| Some((f, lib.folder(f)?.path.clone()))).collect()
    }

    /// Starts the job that repairs `folders` (nothing when there are none), with the model's view
    /// of each photo's file but those of `unknown`.
    fn start_repair(&mut self, folders: Vec<(FolderId, PathBuf)>, unknown: &BTreeSet<PhotoId>) {
        if folders.is_empty() {
            return;
        }
        let now = self.clock.now();
        let lib = self.library();
        let items: Vec<Item> = folders
            .into_iter()
            .map(|(folder, path)| {
                let seen = lib
                    .photos_in(folder)
                    .into_iter()
                    .filter(|p| !unknown.contains(&p.id))
                    .map(|p| (p.name.clone(), Seen { size: p.size, mtime: p.mtime }))
                    .collect();
                let names = lib.photos_in(folder).into_iter().map(|p| p.name.clone()).collect();
                Item { folder, path, seen, names, options: save::Options::new(now) }
            })
            .collect();
        let said: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&said);
        let job = FileJob::new(JobKind::Save, LABEL, items, work)
            .on_applied(move |s, a| {
                let (ops, short) = s.repair_ops(a);
                told.lock().unwrap_or_else(PoisonError::into_inner).extend(short);
                Ok(ops)
            })
            .on_finish(move |_, tally| Notice { message: message(&said.lock().unwrap_or_else(PoisonError::into_inner), tally), folder: None });
        self.start_file_job(job);
    }

    /// The ops that bring one folder's repairs into the model, and a short sentence per repair
    /// (the whole one, with the path, is logged and kept in the warnings).
    fn repair_ops(&mut self, applied: Applied<Fixed>) -> (Vec<Op>, Vec<String>) {
        let Fixed { folder, path, recovered, facts } = applied.value;
        let mut ops = Vec::new();
        let mut short = Vec::new();
        for p in &recovered.problems {
            log::warn!("{p}");
        }
        for Repair { name, entry, outcome, .. } in recovered.repairs {
            let what = match &outcome {
                Outcome::RolledForward => "an interrupted save was finished; the edits are in the file".to_string(),
                Outcome::RolledBack => "an interrupted save was taken back; the original is in place and the edits are kept".to_string(),
                Outcome::Left(why) => format!("an interrupted save could not be repaired: {why}"),
            };
            let line = format!("{}: {what}", path.join(&name).display());
            match outcome {
                Outcome::Left(_) => log::warn!("{line}"),
                _ => log::info!("{line}"),
            }
            self.scan_warnings.push(line);
            short.push(format!("{name}: {what}"));
            let Some(photo) = self.library().photo_by_name(folder, &name).cloned() else { continue };
            if let Some(op) = photo_op(self, &photo, &entry, &outcome, facts.get(&name)) {
                ops.push(op);
            }
        }
        (ops, short)
    }
}

/// The photos an op from the files gives a pending saved record.
fn pending_in(op: &Op, out: &mut Vec<PhotoId>) {
    match op {
        Op::SetSaved { saved } => {
            out.extend(saved.iter().filter(|(_, s)| s.as_ref().is_some_and(|s| s.state == SaveState::Pending)).map(|(id, _)| *id));
        }
        Op::Batch { ops } => ops.iter().for_each(|o| pending_in(o, out)),
        _ => {}
    }
}

/// The folders among `folders` that need the repair (on the checking thread).
fn needing_repair(folders: Vec<(FolderId, PathBuf)>) -> Vec<(FolderId, PathBuf)> {
    folders
        .into_iter()
        .filter(|(_, path)| match recover::check(path) {
            Ok(needed) => needed,
            Err(e) => {
                log::warn!("{}: not checked for an interrupted save: {}", path.display(), e.reason());
                false
            }
        })
        .collect()
}

/// One folder to repair: everything its work needs.
struct Item {
    folder: FolderId,
    path: PathBuf,
    /// The library's last view of each photo's file, by name (a photo whose view says nothing is
    /// left out).
    seen: BTreeMap<String, Seen>,
    /// The names of the folder's photos in the library.
    names: BTreeSet<String>,
    options: save::Options,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.path
    }
}

/// What the repair of one folder did.
struct Fixed {
    folder: FolderId,
    path: PathBuf,
    recovered: Recovered,
    /// The files the repair left at the photos' names, as read back, by name.
    facts: BTreeMap<String, Facts>,
}

/// A file as a scan lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Facts {
    size: u64,
    mtime: Timestamp,
    width: u32,
    height: u32,
    date: Option<DateTime>,
}

/// One folder, on the job's worker.
fn work(item: Item) -> Result<Done<Fixed>, String> {
    let recovered = recover::recover(&item.path, &item.seen, &item.options).map_err(|e| e.reason())?;
    let mut facts = BTreeMap::new();
    let mut changes = Vec::new();
    for r in recovered.repairs.iter().filter(|r| matches!(r.outcome, Outcome::RolledForward | Outcome::RolledBack)) {
        if !item.names.contains(&r.name) && changes.is_empty() {
            // Not a photo of the library: its folder is scanned when the job ends.
            changes.push(FileChange::Folder { path: item.path.clone() });
        }
        if let Some(f) = read_facts(&item.path.join(&r.name)) {
            facts.insert(r.name.clone(), f);
        }
    }
    Ok(Done { changes, value: Fixed { folder: item.folder, path: item.path, recovered, facts } })
}

/// The file at `path` as a scan lists it; `None` when it is not there.
fn read_facts(path: &Path) -> Option<Facts> {
    let stat = dalica_fs::stat(path).ok().flatten().filter(|s| s.is_file)?;
    let (size, mtime) = (stat.identity.size(), Timestamp::from_system_time(stat.times.modified));
    Some(match dalica_image::read_header_file(path, &dalica_image::Limits::default()) {
        Ok((h, _)) => {
            let (width, height) = h.display_size();
            Facts { size, mtime, width, height, date: h.exif.as_ref().and_then(|e| e.date_time_original) }
        }
        Err(_) => Facts { size, mtime, width: 0, height: 0, date: None },
    })
}

/// The op that brings one repaired photo's model in line with its files, if anything changes.
fn photo_op(s: &Session, photo: &Photo, entry: &Entry, outcome: &Outcome, facts: Option<&Facts>) -> Option<Op> {
    let lib = s.library();
    let id = photo.id;
    let info = lib.info(id);
    let (saved, edits) = match outcome {
        Outcome::Left(_) => return None,
        Outcome::RolledForward => {
            // The saved file holds the whole recipe: the edits it holds are no longer unsaved.
            let held = overlap(&entry.recipe, &info.edits);
            (Some(saved_of(entry, SaveState::Done)), info.edits.get(held..).unwrap_or_default().to_vec())
        }
        Outcome::RolledBack => {
            // The original is back: the recorded recipe returns as unsaved edits in front of the
            // edits not in it (a pending Save took its last operations from them; an Undo Save's
            // or Revert's were made after the save).
            let held = if entry.state == State::Pending { overlap(&entry.recipe, &info.edits) } else { 0 };
            let mut edits = entry.recipe.get(..entry.recipe.len().saturating_sub(held)).unwrap_or_default().to_vec();
            edits.extend(info.edits.iter().cloned());
            (None, edits)
        }
    };
    let mut ops = Vec::new();
    if lib.saved(id) != saved.as_ref() {
        ops.push(Op::SetSaved { saved: vec![(id, saved)] });
    }
    if edits != info.edits {
        ops.push(Op::SetRecipes { recipes: vec![(id, dalica_library::Recipe { edits, redo: info.redo.clone() })] });
    }
    if let Some(f) = facts {
        let new = Photo { size: f.size, mtime: f.mtime, width: f.width, height: f.height, date: f.date, ..photo.clone() };
        if new != *photo {
            ops.push(Op::UpdatePhotos { photos: vec![new] });
        }
    }
    (!ops.is_empty()).then_some(Op::Batch { ops })
}

/// How many of `edits`' first operations end `recipe`: the edits a Save took into it.
fn overlap(recipe: &[String], edits: &[String]) -> usize {
    (1..=recipe.len().min(edits.len())).rev().find(|&k| recipe.get(recipe.len().saturating_sub(k)..) == edits.get(..k)).unwrap_or(0)
}

/// The model's record of `entry` in `state`, as the scan reads one.
fn saved_of(entry: &Entry, state: SaveState) -> Saved {
    Saved {
        recipe: entry.recipe.clone(),
        size: entry.size,
        mtime: Timestamp::from_millis(entry.mtime),
        width: entry.width,
        height: entry.height,
        digest: entry.digest.clone(),
        state,
        time: entry.time.map(Timestamp::from_millis),
    }
}

/// The job's message: what was repaired, or that nothing needed it.
fn message(short: &[String], tally: &Tally) -> String {
    match short {
        [] if tally.failures.is_empty() => "No interrupted save needed repair".into(),
        [] => "Interrupted saves could not be looked at".into(),
        [one] => format!("Repaired {one}"),
        many => format!("Repaired {} interrupted saves", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(ops: &[&str]) -> Vec<String> {
        ops.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_overlap_is_the_edits_a_save_took() {
        assert_eq!(overlap(&v(&["a", "b"]), &v(&["b"])), 1, "a second Save of b after a");
        assert_eq!(overlap(&v(&["a"]), &v(&["a", "c"])), 1, "c made while it saved");
        assert_eq!(overlap(&v(&["a", "b"]), &v(&[])), 0);
        assert_eq!(overlap(&v(&[]), &v(&["a"])), 0);
        assert_eq!(overlap(&v(&["a", "b"]), &v(&["a", "b"])), 2);
        assert_eq!(overlap(&v(&["a", "b"]), &v(&["c"])), 0);
    }

    #[test]
    fn the_message_counts_the_repairs() {
        let t = Tally::default();
        assert_eq!(message(&[], &t), "No interrupted save needed repair");
        assert_eq!(message(&v(&["IMG_1.jpg: done"]), &t), "Repaired IMG_1.jpg: done");
        assert_eq!(message(&v(&["x", "y"]), &t), "Repaired 2 interrupted saves");
    }
}
