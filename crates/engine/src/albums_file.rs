//! `albums.json` in the library directory (`data-model.md` §3.2, §4): the albums' definitions (id,
//! name, description, date, place, cover) and a copy of their order, beside the memberships each
//! folder sidecar holds. It is read when the library opens and written through the I/O worker after
//! the ops that touch albums; when it is missing, a rebuild recovers memberships, order and names
//! from the sidecars (`scanning.rs`).
//!
//! - **At open** ([`Session::read_albums_at_open`], on a thread of its own): what the file says is
//!   kept for the scans, which create an album a folder names but the model lacks with its
//!   definition from here ([`AlbumsFile::defs`]); a damaged file is set aside and reported, and a
//!   new one is written when the model has albums. A file that reads is never written over at
//!   open: when the index was lost the model may lack albums the file still defines, and the scans
//!   bring them back first.
//! - **After an op** ([`Session::albums_applied`], called by `Session::apply` for every op): an op
//!   that changes an album, a membership or a cover, or moves, renames or removes photos or
//!   folders while the library has albums (their paths are in the copy), marks the file to be
//!   written. The model's albums are taken as an owned [`AlbumsState`] on the owner thread and
//!   written on the I/O worker ([`dalica_store::albums::write`]: merged into the file, only when its
//!   bytes change), so the owner thread never touches the disk. A failed write is reported (a scan
//!   warning) and tried again with the next change.
//! - **Never an album it has not seen:** the write drops the entries of albums the library no
//!   longer has, which must mean albums this session saw go (deleted, or gone in a rebuild). An
//!   entry of an album the session never had in its model (the index was lost and no scan has
//!   brought it back yet) is carried over as the file says it, so a write never loses a definition
//!   the model has not taken in.
//! - **Held while scans apply** ([`Session::write_albums_file`]): while a scan applies what it
//!   found, and while a rebuild runs or waits to run again, the file is not written (a rebuild's
//!   model lacks albums until it is done, and a write would drop their entries); the scans write it
//!   once they are done (`scanning.rs`). The watcher's rescans and commands write it at once.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::AlbumId;
use dalica_library::Op;
use dalica_sidecar::albums::AlbumsFile as RawFile;
use dalica_store::albums::{AlbumsRead, AlbumsReading, AlbumsState, AlbumsWrite};

use crate::session::{Msg, Session};

/// What the session knows of `albums.json`.
#[derive(Default)]
pub(crate) struct AlbumsFile {
    /// An op touched albums since the file was last written.
    dirty: bool,
    /// What was last sent to be written (`None`: nothing yet, or the last write failed).
    last: Option<AlbumsState>,
    /// What the file said when the library opened: definitions for the albums a scan finds named
    /// in a folder and the model lacks.
    pub(crate) defs: Option<AlbumsReading>,
    /// Every album the model had this session (at open, created or deleted since): only their
    /// entries are the model's to write or drop.
    seen: BTreeSet<AlbumId>,
    /// What the I/O worker's writes reported (and whether the write failed), for the owner.
    reports: Arc<Mutex<Vec<(String, bool)>>>,
}

impl AlbumsFile {
    /// Notes the albums `op` creates or deletes as seen.
    fn see(&mut self, op: &Op) {
        let mut stack = vec![op];
        while let Some(op) = stack.pop() {
            match op {
                Op::CreateAlbums { albums } => self.seen.extend(albums.iter().map(|(id, _)| id.clone())),
                Op::DeleteAlbums { ids } => self.seen.extend(ids.iter().cloned()),
                Op::Batch { ops } => stack.extend(ops),
                _ => {}
            }
        }
    }

    /// After a rebuild's model replaced the one before: its albums are seen.
    pub(crate) fn see_all<'a>(&mut self, ids: impl Iterator<Item = &'a AlbumId>) {
        self.seen.extend(ids.cloned());
    }

    /// The model changed without an op the file follows (a rebuild's model swapped in): the file
    /// is written once nothing holds it.
    pub(crate) fn changed(&mut self) {
        self.dirty = true;
    }

    /// Whether album `id` was deleted this session (seen, and not in `library` any more): a scan
    /// never brings it back from a folder that still names it.
    pub(crate) fn deleted(&self, library: &dalica_library::Library, id: &AlbumId) -> bool {
        self.seen.contains(id) && library.album(id).is_none()
    }
}

/// `state` with the entries of the file at `path` (read here, on the I/O worker) for the albums
/// the session never saw, as the file says them, so the write keeps them. A file that cannot be
/// read adds nothing (the write itself sets a damaged one aside).
fn with_unseen(path: &std::path::Path, mut state: AlbumsState, seen: &BTreeSet<AlbumId>) -> AlbumsState {
    let limits = dalica_sidecar::Limits::default();
    let Ok(Some(read)) = dalica_fs::read(path, limits.max_bytes as u64) else { return state };
    let Ok(file) = RawFile::parse(&read.bytes, &limits) else { return state };
    for (id, album) in file.albums().albums {
        if !seen.contains(&id) && !state.albums.contains_key(&id) {
            state.albums.insert(id, album);
        }
    }
    state
}

/// Whether `op` changes an album, a membership or a cover (`albums`), or moves, renames or removes
/// photos or folders (`paths`). Walks nested batches without recursion.
fn touches(op: &Op) -> (bool, bool) {
    let (mut albums, mut paths) = (false, false);
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::CreateAlbums { .. }
            | Op::DeleteAlbums { .. }
            | Op::SetAlbumInfo { .. }
            | Op::AddMembers { .. }
            | Op::RemoveMembers { .. }
            | Op::SetMemberKeys { .. }
            | Op::SetCovers { .. } => albums = true,
            Op::RemovePhotos { .. } | Op::RemoveFolder { .. } | Op::UpdatePhotos { .. } | Op::MovePhotos { .. } | Op::MoveFolder { .. } => {
                paths = true
            }
            Op::Batch { ops } => stack.extend(ops),
            _ => {}
        }
    }
    (albums, paths)
}

impl Session {
    /// At session open: `albums.json` is read on a thread of its own; what it says is kept for the
    /// scans, a damaged one is set aside and reported, and a missing or damaged one is written anew
    /// when the model has albums.
    pub(crate) fn read_albums_at_open(&mut self) {
        let seen = self.library.albums().map(|(id, _)| id.clone()).collect();
        self.scanning.albums_file.seen = seen;
        let (dir, now) = (self.dir.clone(), self.clock.now());
        let read = std::thread::scope(|scope| {
            let spawned = std::thread::Builder::new().name("dalica-albums".into()).spawn_scoped(scope, || dalica_store::albums::read(&dir, now));
            match spawned {
                Ok(handle) => handle.join().unwrap_or_else(|_| AlbumsRead::Unreadable("reading it panicked".into())),
                Err(e) => AlbumsRead::Unreadable(format!("no thread to read it ({e})")),
            }
        });
        let path = dalica_store::albums::path(&self.dir);
        let missing = match read {
            AlbumsRead::Absent => true,
            AlbumsRead::Read(reading) => {
                self.scan_warnings.extend(reading.notes.iter().cloned());
                self.scanning.albums_file.defs = Some(reading);
                false
            }
            AlbumsRead::Unreadable(why) => {
                self.scan_warnings.push(format!("{}: {why}", path.display()));
                false
            }
            AlbumsRead::SetAside { why, kept } => {
                let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "it could not be kept aside".into());
                self.scan_warnings.push(format!("{}: {why}; {kept}", path.display()));
                true
            }
        };
        if missing && self.library.albums().next().is_some() {
            self.scanning.albums_file.dirty = true;
            self.write_albums_file();
        }
    }

    /// After `op` was applied: when it touched albums (or moved, renamed or removed photos or
    /// folders while the library has albums), `albums.json` is to be written; it is, through the
    /// I/O worker, unless scans hold it ([`Session::write_albums_file`]).
    pub(crate) fn albums_applied(&mut self, op: &Op) {
        let (albums, paths) = touches(op);
        if albums {
            self.scanning.albums_file.see(op);
        }
        let file = &self.scanning.albums_file;
        let any = || self.library.albums().next().is_some() || file.last.as_ref().is_some_and(|l| !l.albums.is_empty());
        if albums || (paths && any()) {
            self.scanning.albums_file.dirty = true;
            self.write_albums_file();
        }
    }

    /// Whether `albums.json` waits: a scan is applying what it found, or a rebuild runs or waits to
    /// run again (its model lacks albums until it is done).
    fn albums_file_held(&self) -> bool {
        self.scanning.applying() || self.scanning.rebuild_pending()
    }

    /// Writes `albums.json` through the I/O worker when an op touched albums since the last write
    /// and nothing holds it; the reports of earlier writes are taken in as scan warnings.
    pub(crate) fn write_albums_file(&mut self) {
        let reports = std::mem::take(&mut *self.scanning.albums_file.reports.lock().unwrap_or_else(PoisonError::into_inner));
        for (report, failed) in reports {
            if failed {
                // Tried again with the next change.
                self.scanning.albums_file.last = None;
            }
            self.scan_warnings.push(report);
        }
        if !self.scanning.albums_file.dirty || self.albums_file_held() {
            return;
        }
        let file = &mut self.scanning.albums_file;
        file.dirty = false;
        let state = dalica_store::albums::state(&self.library);
        // A library that never had albums makes no file.
        if file.last.as_ref() == Some(&state) || (state.albums.is_empty() && file.last.is_none() && file.defs.is_none() && file.seen.is_empty()) {
            return;
        }
        file.last = Some(state.clone());
        let (dir, now, reports, seen) = (self.dir.clone(), self.clock.now(), Arc::clone(&file.reports), file.seen.clone());
        self.send(Msg::Run(Box::new(move || {
            let path = dalica_store::albums::path(&dir);
            let state = with_unseen(&path, state, &seen);
            let report = match dalica_store::albums::write(&dir, &state, now) {
                AlbumsWrite::Written | AlbumsWrite::Unchanged => None,
                AlbumsWrite::SetAsideAndWritten { why, kept } => {
                    let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "it could not be kept aside".into());
                    Some((format!("{}: {why}; {kept}; a new one was written", path.display()), false))
                }
                AlbumsWrite::Failed(why) => Some((format!("{}: not written: {why}", path.display()), true)),
            };
            if let Some(r) = report {
                log::warn!("{}", r.0);
                reports.lock().unwrap_or_else(PoisonError::into_inner).push(r);
            }
        })));
    }
}
