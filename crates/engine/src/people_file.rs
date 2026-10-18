//! `people.json` in the library directory (`data-model.md` §4, `faces.md` §6): every person (id,
//! name, nickname, thumbnail face) beside the faces each folder sidecar holds, so a nickname, a
//! thumbnail and a person with no face survive a rebuild (which reads the file, M13.45). It is
//! written through the I/O worker after the ops that touch people, as `albums.json` is
//! (`albums_file.rs`).
//!
//! - **After an op** ([`Session::people_applied`]): an op that adds, renames, merges or deletes a
//!   person, sets a thumbnail or changes faces, or moves, renames or removes photos or folders
//!   while the library has people (a thumbnail names its photo by path), marks the file to be
//!   written. The model's people are taken as an owned [`PeopleState`] on the owner thread and
//!   written on the I/O worker ([`dalica_store::people::write`]: merged into the file, unknown keys
//!   and entries kept, the rejected suggestions left as the file has them, written only when its
//!   bytes change, atomically through `fs`), so the owner thread never touches the disk. A failed
//!   write is reported (a scan warning) and tried again with the next change.
//! - **Never a person it has not seen:** the store's merge drops the entries of people the state
//!   lacks, which must mean people this session saw go (deleted, merged away, gone in a rebuild).
//!   An entry of a person the session never had in its model (the index was lost and no scan has
//!   brought them back yet) is carried over as the file says it.
//! - **Held while scans apply** (and while a rebuild runs or waits to run again): a rebuild's model
//!   lacks people until it is done, so the file waits; it is written at the next poll after they
//!   end ([`Session::write_people_file`]).
//! - A library that never had people makes no file. A read-only session writes nothing (the I/O
//!   worker drops the work).
//!
//! The hooks are called from `search.rs` (`search_applied`, which `Session::apply` calls for every
//! op, and `poll_search`, which `Session::poll` calls each frame), so this module needs no line of
//! `session.rs`; its state lives in the search state for the same reason.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::PersonId;
use dalica_library::Op;
use dalica_store::people::{PeopleState, PeopleWrite};

use crate::session::{Msg, Session};

/// What the session knows of `people.json`.
#[derive(Debug, Default)]
pub(crate) struct PeopleFile {
    /// An op touched people since the file was last written.
    dirty: bool,
    /// What was last sent to be written (`None`: nothing yet, or the last write failed).
    last: Option<PeopleState>,
    /// Every person the model had this session (at the first op, and every one an op named since):
    /// only their entries are the model's to write or drop. `None` until the first op.
    seen: Option<BTreeSet<PersonId>>,
    /// What the I/O worker's writes reported (and whether the write failed), for the owner.
    reports: Arc<Mutex<Vec<(String, bool)>>>,
}

/// Whether `op` touches people (`people`: a person, a thumbnail or faces) or moves, renames or
/// removes photos or folders (`paths`), and the people it names. Walks nested batches without
/// recursion.
fn touches(op: &Op, named: &mut BTreeSet<PersonId>) -> (bool, bool) {
    let (mut people, mut paths) = (false, false);
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::AddPerson { id, .. } | Op::RenamePerson { id, .. } | Op::DeletePerson { id } | Op::SetPersonThumbnail { id, .. } => {
                named.insert(id.clone());
                people = true;
            }
            Op::MergePeople { from, into } => {
                named.insert(from.clone());
                named.insert(into.clone());
                people = true;
            }
            Op::SetFaces { .. } => people = true,
            Op::RemovePhotos { .. } | Op::RemoveFolder { .. } | Op::UpdatePhotos { .. } | Op::MovePhotos { .. } | Op::MoveFolder { .. } => {
                paths = true
            }
            Op::Batch { ops } => stack.extend(ops),
            _ => {}
        }
    }
    (people, paths)
}

/// `state` with the people of the file at `dir` (read here, on the I/O worker) the session never
/// saw, as the file says them, so the write keeps them. A file that cannot be read adds nothing
/// (the write itself sets a damaged one aside).
fn with_unseen(dir: &std::path::Path, mut state: PeopleState, seen: &BTreeSet<PersonId>) -> PeopleState {
    let Ok(Some(read)) = dalica_fs::read(&dalica_store::people::path(dir), dalica_store::people::MAX_BYTES) else { return state };
    let Ok(root) = dalica_store::people::parse(&read.bytes) else { return state };
    for (id, def) in dalica_store::people::reading(&root).people {
        if !seen.contains(&id) && !state.people.contains_key(&id) {
            state.people.insert(id, def);
        }
    }
    state
}

impl Session {
    /// After `op` was applied: when it touched people (or moved, renamed or removed photos or
    /// folders while the library has people), `people.json` is to be written; it is, through the
    /// I/O worker, unless scans hold it ([`Session::write_people_file`]).
    pub(crate) fn people_applied(&mut self, op: &Op) {
        let mut named = BTreeSet::new();
        let (people, paths) = touches(op, &mut named);
        let file = &mut self.search.people_file;
        // The model's people when the first op came, then every person an op names.
        let library = &self.library;
        let seen = file.seen.get_or_insert_with(|| library.people().map(|(id, _)| id.clone()).collect());
        let names_someone = !named.is_empty();
        seen.extend(named);
        let any = names_someone || self.library.people().next().is_some() || file.last.as_ref().is_some_and(|l| !l.people.is_empty());
        if (people || paths) && any {
            file.dirty = true;
            self.write_people_file();
        }
    }

    /// Whether `people.json` waits: a scan is applying what it found, or a rebuild runs or waits to
    /// run again (its model lacks people until it is done).
    fn people_file_held(&self) -> bool {
        self.scanning.applying() || self.scanning.rebuild_pending()
    }

    /// Writes `people.json` through the I/O worker when an op touched people since the last write
    /// and nothing holds it; the reports of earlier writes are taken in as scan warnings. Cheap
    /// when nothing waits (each poll calls it).
    pub(crate) fn write_people_file(&mut self) {
        let reports = std::mem::take(&mut *self.search.people_file.reports.lock().unwrap_or_else(PoisonError::into_inner));
        for (report, failed) in reports {
            if failed {
                // Tried again with the next change.
                self.search.people_file.last = None;
            }
            self.scan_warnings.push(report);
        }
        if !self.search.people_file.dirty || self.people_file_held() {
            return;
        }
        let state = dalica_store::people::state(&self.library);
        let file = &mut self.search.people_file;
        file.dirty = false;
        let seen = file.seen.clone().unwrap_or_default();
        // A library that never had people makes no file.
        if file.last.as_ref() == Some(&state) || (state.people.is_empty() && file.last.is_none() && seen.is_empty()) {
            return;
        }
        file.last = Some(state.clone());
        let (dir, now, reports) = (self.dir.clone(), self.clock.now(), Arc::clone(&file.reports));
        self.send(Msg::Run(Box::new(move || {
            let path = dalica_store::people::path(&dir);
            let state = with_unseen(&dir, state, &seen);
            let report = match dalica_store::people::write(&dir, &state, now) {
                PeopleWrite::Written | PeopleWrite::Unchanged => None,
                PeopleWrite::SetAsideAndWritten { why, kept } => {
                    let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "it could not be kept aside".into());
                    Some((format!("{}: {why}; {kept}; a new one was written", path.display()), false))
                }
                PeopleWrite::Failed(why) => Some((format!("{}: not written: {why}", path.display()), true)),
            };
            if let Some(r) = report {
                // Paths and reasons only: never a name (`faces.md` §7).
                log::warn!("{}", r.0);
                reports.lock().unwrap_or_else(PoisonError::into_inner).push(r);
            }
        })));
    }
}
