//! `albums.json` in the library directory (`data-model.md` §4): the albums' definitions (name,
//! description, date, "use the newest photo's date", place, cover) and a copy of each album's
//! order, beside the memberships each folder sidecar holds (the source of truth for those).
//!
//! - **Reading** ([`read`], [`reading`]): the albums whose id, name and values can be used, with
//!   names cleaned and text cut the way the library cleans them (the file keeps them whole);
//!   anything else is kept in the file and reported once. A damaged file (not JSON, not an object,
//!   past the limits of `security.md` §4.2) is set aside as `albums.json.corrupt-<time>` and
//!   reported; the albums then come back from the folders ([`crate::rebuild`]).
//! - **Writing** ([`state`], [`write`]): the model's albums as an owned [`AlbumsState`] (so the
//!   write can run on the I/O worker), merged into the file on disk: entries of albums the model
//!   no longer has go, keys and entries Dalica does not know stay, and the file is written only
//!   when its bytes change, through `fs`'s atomic replace over the version just read. A damaged
//!   file is set aside first and a new one written.
//! - **Legacy ids** (M14, `data-model.md` §9): an album found in the previous organizer's
//!   sidecars keeps that program's album token as `"legacy"` in its entry ([`AlbumDef::legacy`],
//!   [`AlbumsState::legacy`]), so the same album seen again (in another folder, after a rebuild) is
//!   one album. A write sets the model's ids and never removes one from an entry: an album the
//!   session did not see, or whose id the model lost, keeps the file's. A value that is not a clean
//!   id ([`crate::legacy::clean_id`]) is kept in the file, reported and not used.
//! - Photos are named by absolute path (photo ids belong to one index). A path that is not UTF-8
//!   cannot be written in JSON: such a cover or member is left out of the copy (the sidecar keeps
//!   the membership).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, DateTime, PosKey, Timestamp};
use dalica_fs::Expect;
use dalica_library::{AlbumInfo, Library, MAX_DESCRIPTION, MAX_PLACE};
use dalica_sidecar::Limits;
use dalica_sidecar::albums::{self as file, Album, AlbumsFile};

/// The file's name in the library directory.
pub const FILE: &str = file::FILE_NAME;

/// One album as `albums.json` gives it to the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumDef {
    /// Clean and within the library's caps, as an album op takes it.
    pub info: AlbumInfo,
    /// The cover photo's path.
    pub cover: Option<PathBuf>,
    /// The members in album order, with their keys (a copy: the sidecars hold the memberships).
    pub order: Vec<(PathBuf, PosKey)>,
    /// The album's legacy id (M14), clean.
    pub legacy: Option<String>,
}

/// What `albums.json` gives the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumsReading {
    pub albums: BTreeMap<AlbumId, AlbumDef>,
    /// One sentence per value or entry not used (it stays in the file).
    pub notes: Vec<String>,
}

/// What reading `albums.json` gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlbumsRead {
    /// There is no `albums.json`: the albums come back from the folders.
    Absent,
    Read(AlbumsReading),
    /// It could not be read (a link in its place, a file too large, an I/O error).
    Unreadable(String),
    /// It was damaged: kept aside under this name (`None` when even that failed).
    SetAside {
        why: String,
        kept: Option<PathBuf>,
    },
}

/// The model's albums, owned, as `albums.json` should say them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumsState {
    pub albums: BTreeMap<AlbumId, Album>,
    /// The legacy ids of albums in `albums` (M14): set in their entries; an album not listed keeps
    /// the file's.
    pub legacy: BTreeMap<AlbumId, String>,
}

/// What [`write`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlbumsWrite {
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

/// `albums.json`'s path in the library directory `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// What the parsed file gives the model: albums with a usable id and name; text past the caps cut
/// (and noted), names cleaned; an entry whose name is nothing once cleaned is not used.
pub fn reading(f: &AlbumsFile) -> AlbumsReading {
    let mut out = AlbumsReading::default();
    for p in f.problems() {
        out.notes.push(format!("{FILE}{} should be {}", p.pointer, p.expected));
    }
    let mut legacy = f.legacy_ids();
    for (id, a) in f.albums().albums {
        let Some(name) = dalica_library::clean_album_name(&a.name) else {
            out.notes.push(format!("{FILE}: album {id} has no usable name: kept in the file, not used"));
            continue;
        };
        let mut cut = |text: String, max: usize, clean: fn(&str) -> String, what: &str| {
            if text.len() > max {
                out.notes.push(format!("{FILE}: album {id}'s {what} is longer than {max} bytes: shown cut, kept whole in the file"));
                clean(&text)
            } else {
                text
            }
        };
        let description = cut(a.description, MAX_DESCRIPTION, dalica_library::clean_description, "description");
        let place = cut(a.place, MAX_PLACE, dalica_library::clean_place, "place");
        let info = AlbumInfo { name, description, date: a.date, use_newest: a.use_newest, place };
        let order = a.order.into_iter().map(|(p, k)| (PathBuf::from(p), k)).collect();
        let legacy = legacy.remove(&id).and_then(|l| {
            let clean = crate::legacy::clean_id(&l).map(str::to_string);
            if clean.is_none() {
                out.notes.push(format!("{FILE}: album {id}'s legacy id is not one Dalica takes: kept in the file, not used"));
            }
            clean
        });
        out.albums.insert(id, AlbumDef { info, cover: a.cover.map(PathBuf::from), order, legacy });
    }
    out
}

/// Reads `albums.json` in the library directory `dir` through `dalica-fs` (never through a link);
/// a damaged one is set aside as `albums.json.corrupt-<time>` and reported.
pub fn read(dir: &Path, now: Timestamp) -> AlbumsRead {
    let path = path(dir);
    let limits = Limits::default();
    let bytes = match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(None) => return AlbumsRead::Absent,
        Ok(Some(r)) => r.bytes,
        Err(e) => return AlbumsRead::Unreadable(e.to_string()),
    };
    match AlbumsFile::parse(&bytes, &limits) {
        Ok(f) => AlbumsRead::Read(reading(&f)),
        Err(e) => AlbumsRead::SetAside { why: e.to_string(), kept: dalica_fs::set_aside(&path, &corrupt_tag(now)).ok() },
    }
}

/// The model's albums as `albums.json` should say them: each album's info, its cover's path and
/// its members' paths with their keys in album order.
pub fn state(library: &Library) -> AlbumsState {
    let path_of = |id| {
        let p = library.photo(id)?;
        let f = library.folder(p.folder)?;
        f.path.join(&p.name).to_str().map(str::to_string)
    };
    let mut out = AlbumsState::default();
    for (id, info) in library.albums() {
        let order = library.album_members(id).iter().filter_map(|&p| Some((path_of(p)?, library.member_key(id, p)?.clone()))).collect();
        let album = Album {
            name: info.name.clone(),
            description: info.description.clone(),
            date: info.date,
            use_newest: info.use_newest,
            place: info.place.clone(),
            cover: library.cover_set(id).and_then(path_of),
            order,
        };
        out.albums.insert(id.clone(), album);
    }
    out.legacy = library.album_legacies().map(|(id, l)| (id.clone(), l.to_string())).collect();
    out
}

/// `existing` (or a new file) made to say `state`: entries of albums `state` does not have go
/// (entries Dalica cannot use stay), each album's keys are set (keys Dalica does not know stay).
pub fn merged(existing: Option<&AlbumsFile>, state: &AlbumsState) -> AlbumsFile {
    let mut f = existing.cloned().unwrap_or_default();
    let usable = f.albums();
    for id in usable.albums.keys().filter(|id| !state.albums.contains_key(*id)) {
        f.remove(id.as_str());
    }
    for (id, album) in &state.albums {
        f.set(id, album);
    }
    for (id, legacy) in state.legacy.iter().filter(|(id, _)| state.albums.contains_key(*id)) {
        f.set_legacy(id, legacy);
    }
    f
}

/// Writes `state` into `albums.json` in the library directory `dir` ([`merged`] with the file on
/// disk), only when its bytes change, through `fs`'s atomic replace over the version just read.
/// Any thread but the UI thread may call it.
pub fn write(dir: &Path, state: &AlbumsState, now: Timestamp) -> AlbumsWrite {
    let path = path(dir);
    let limits = Limits::default();
    let read = match dalica_fs::read(&path, limits.max_bytes as u64) {
        Ok(r) => r,
        Err(e) => return AlbumsWrite::Failed(e.to_string()),
    };
    let (existing, expect, aside) = match read {
        None => (None, Expect::Absent, None),
        Some(r) => match AlbumsFile::parse(&r.bytes, &limits) {
            Ok(f) => {
                let new = merged(Some(&f), state).print();
                if new == r.bytes {
                    return AlbumsWrite::Unchanged;
                }
                (Some(f), Expect::Unchanged(r.identity), None)
            }
            Err(e) => match dalica_fs::set_aside(&path, &corrupt_tag(now)) {
                Ok(kept) => (None, Expect::Absent, Some((e.to_string(), Some(kept)))),
                // Never write over a damaged file that could not be kept.
                Err(err) => return AlbumsWrite::Failed(format!("{e}; it could not be kept aside: {err}")),
            },
        },
    };
    let bytes = merged(existing.as_ref(), state).print();
    match dalica_fs::replace(&path, &bytes, expect) {
        Ok(_) => match aside {
            Some((why, kept)) => AlbumsWrite::SetAsideAndWritten { why, kept },
            None => AlbumsWrite::Written,
        },
        Err(e) => AlbumsWrite::Failed(e.to_string()),
    }
}

/// `corrupt-20261010T051500Z` for the time `now`.
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_else(|| now.as_millis().to_string());
    format!("corrupt-{stamp}Z")
}
