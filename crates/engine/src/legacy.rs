//! The previous organizer's data in the session (`data-model.md` §9, `roadmap.md` M14 → *Legacy
//! sidecars*, M14.13): what the scans found in legacy sidecars, applied as a scan's imports are,
//! the folder facts, the first writes the I/O worker reports, and the first-run notice. The
//! settings are [`crate::settings::LegacySettings`]; the commands are `cmd/legacy.rs`.
//!
//! - **Imports** ([`with_legacy`], called from `scanning.rs`'s `with_sidecar` for every op a scan
//!   or a watcher rescan applies): a folder the op adds gets its fact (the sidecar in effect,
//!   `SetFolderSidecars`) in the same op; each photo it adds from a legacy-only folder gets its
//!   faces and album memberships. An album is found by its legacy id (in the model, else, in a
//!   rebuild, in `albums.json`), else created from the folder's definition with the legacy id kept
//!   (`SetLegacyIds`); an album no folder's sidecar defines is not created. A face's person is
//!   found by the contact id, else (named in the folder's or an ancestor's sidecar) by the name
//!   ignoring case (that person takes the contact id when they have none), else made; a contact
//!   named nowhere gives an Unnamed face, the ignore marker an Ignored one. All of it with no undo
//!   step and **nothing written**: a scan's op leaves a legacy-only folder unmarked for a sidecar
//!   write ([`Session::keep_legacy_unwritten`], from `Session::apply`), so its first
//!   `.dalica.json` comes with the user's first change (the store's `legacy::first_write`).
//! - **The folder read again** ([`transition`], once per folder a scan hands out): when the
//!   sidecar in effect changes between the model's word and the scan's, the fact follows; when
//!   the data's source changes (a legacy-only folder read with the option off, a folder without a
//!   sidecar read with it on, or a `.dalica.json` that appeared before the first write), the
//!   folder's known photos take what the scan found, as new photos would (stars, info, faces,
//!   geotags, memberships, the folder's info), and the albums and people with a legacy id that
//!   this leaves empty go. So turning "Read photo data left by the previous organizer" off and on
//!   (each a rescan, `cmd/options.rs`) takes the legacy-only data out and brings it back, writing
//!   nothing either way.
//! - **First writes** ([`Session::poll_legacy`]): the I/O worker passes on what its writer's
//!   `take_legacy` reports: a folder converted gets the fact `both`; one where a `.dalica.json`
//!   appeared is scanned again and read from that file ([`transition`]).
//! - **The notice** ([`Session::legacy_notice`]): the number of folders with legacy data, given
//!   once per session after the first scan that finds any while `files.legacyNoticeShown` is off;
//!   `legacy.noticeShown` sets it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId, PosKey};
use dalica_library::{AlbumInfo, Face, FacePerson, FolderSidecar, Geo, Library, Op, PhotoInfo};
use dalica_scan::legacy::{LegacyFolder, LegacyPerson, LegacyPhoto};
use dalica_scan::{FoundFolder, FoundPhoto};
use dalica_store::LegacyWrite;
use dalica_store::albums::AlbumsReading;
use dalica_store::people::PeopleReading;
use dalica_store::rebuild::{Defined, People};

use crate::session::Session;

/// What the I/O worker reports of legacy-only folders' first writes, for [`Session::poll_legacy`].
pub(crate) type Writes = Arc<Mutex<BTreeMap<FolderId, LegacyWrite>>>;

/// How many times a new album or person id is drawn before the import gives up on one.
const DRAWS: usize = 64;

/// The legacy data's state.
#[derive(Debug, Default)]
pub struct Legacy {
    /// The number of folders with legacy data the first-run notice names, while it waits.
    notice: Option<usize>,
    /// The notice was given (or answered) this session: it comes once.
    given: bool,
    /// The first writes the I/O worker reported, not taken in yet.
    writes: Writes,
    /// Folders where a `.dalica.json` appeared before the first write: their next scan reads them
    /// from it.
    reread: BTreeSet<PathBuf>,
}

impl Legacy {
    /// The number of folders with legacy data the first-run notice names, while it waits to be
    /// shown ([`Session::legacy_notice`] takes it).
    pub fn notice(&self) -> Option<usize> {
        self.notice
    }

    /// The list the I/O worker fills (`session.rs`).
    pub(crate) fn writes(&self) -> Writes {
        Arc::clone(&self.writes)
    }
}

/// What a rebuild read from `albums.json` and `people.json`, for finding legacy albums and people
/// by their legacy id before those files' entries are created (`None` for merge scans: the model
/// has them).
#[derive(Clone, Copy, Default)]
pub(crate) struct Defs<'a> {
    pub albums: Option<&'a AlbumsReading>,
    pub people: Option<&'a PeopleReading>,
}

/// The fact the model records for what a scan found: only legacy data's (`legacy`, `both`); a
/// folder with Dalica's file only, or none, records nothing, so a scan, a rescan after Dalica's
/// first write and a rebuild leave the same model (`cmd/legacy.rs` tells `dalica` from `none`
/// by what the model holds).
fn recorded(found: FolderSidecar) -> FolderSidecar {
    if found.has_legacy() { found } else { FolderSidecar::None }
}

/// The data source a fact stands for: whether the folder's data came from the legacy sidecar
/// only.
fn legacy_only(s: FolderSidecar) -> bool {
    s == FolderSidecar::Legacy
}

/// The fact folder `id` had before the op whose inverse is `inverse`, when the op changed it.
fn fact_before(inverse: &Op, id: FolderId) -> Option<FolderSidecar> {
    let mut stack = vec![inverse];
    let mut out = None;
    while let Some(op) = stack.pop() {
        match op {
            Op::Batch { ops } => stack.extend(ops),
            Op::SetFolderSidecars { folders } => {
                if let Some((_, s)) = folders.iter().find(|(f, _)| *f == id) {
                    out = Some(*s);
                }
            }
            _ => {}
        }
    }
    out
}

/// The ops of `op` (its children when a batch).
fn children(op: &Op) -> &[Op] {
    match op {
        Op::Batch { ops } => ops,
        other => std::slice::from_ref(other),
    }
}

/// `op` followed by `extra`, as one batch.
fn joined(op: &Op, mut extra: Vec<Op>) -> Op {
    match op.clone() {
        Op::Batch { ops: mut all } => {
            all.append(&mut extra);
            Op::Batch { ops: all }
        }
        other => {
            extra.insert(0, other);
            Op::Batch { ops: extra }
        }
    }
}

/// Finds or makes the albums and people legacy data names, for one op: what it decides is
/// collected as ops ([`Mapper::ops`]) that go before the faces and memberships.
struct Mapper<'a> {
    lib: &'a Library,
    defs: Defs<'a>,
    who: People,
    /// Legacy album tokens and contact ids decided in this op (or by ops already in it).
    albums: BTreeMap<String, AlbumId>,
    people: BTreeMap<String, PersonId>,
    /// Albums and people the op creates.
    create: Vec<(AlbumId, AlbumInfo)>,
    add_people: Vec<Op>,
    /// Legacy ids to set.
    album_ids: Vec<(AlbumId, Option<String>)>,
    person_ids: Vec<(PersonId, Option<String>)>,
    /// The last position key given in each album the op adds members to (`None`: an album it
    /// creates, no member yet).
    keys: BTreeMap<AlbumId, Option<PosKey>>,
}

impl<'a> Mapper<'a> {
    /// A mapper for an op that already holds `already` (another folder's albums, people and
    /// memberships, in a rescan of several).
    fn new(lib: &'a Library, defs: Defs<'a>, already: &[Op]) -> Mapper<'a> {
        let mut m = Mapper {
            lib,
            defs,
            who: People::new(lib),
            albums: BTreeMap::new(),
            people: BTreeMap::new(),
            create: Vec::new(),
            add_people: Vec::new(),
            album_ids: Vec::new(),
            person_ids: Vec::new(),
            keys: BTreeMap::new(),
        };
        let mut stack: Vec<&Op> = already.iter().collect();
        while let Some(op) = stack.pop() {
            match op {
                Op::Batch { ops } => stack.extend(ops),
                Op::AddPerson { id, person } => m.who.assume(id, &person.name),
                Op::SetLegacyIds { albums, people } => {
                    for (id, l) in albums {
                        if let Some(l) = l {
                            m.albums.insert(l.clone(), id.clone());
                        }
                    }
                    for (id, l) in people {
                        if let Some(l) = l {
                            m.people.insert(l.clone(), id.clone());
                        }
                    }
                }
                Op::CreateAlbums { albums } => {
                    for (id, _) in albums {
                        m.keys.entry(id.clone()).or_insert(None);
                    }
                }
                Op::AddMembers { members } => {
                    for (album, _, key) in members {
                        let last = m.keys.entry(album.clone()).or_insert(None);
                        if last.as_ref().is_none_or(|l| key > l) {
                            *last = Some(key.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        m
    }

    /// Whether album `id` exists or will once the op applies.
    fn album_known(&self, id: &AlbumId) -> bool {
        self.lib.album(id).is_some() || self.create.iter().any(|(a, _)| a == id) || self.keys.contains_key(id)
    }

    /// The album of legacy token `token`, found or made (with `folder`'s definition); `None` when
    /// no folder's sidecar defines it.
    fn album(&mut self, token: &str, folder: &LegacyFolder) -> Option<AlbumId> {
        if let Some(id) = self.albums.get(token) {
            return Some(id.clone());
        }
        let found = self.lib.album_by_legacy(token).filter(|id| self.lib.album(id).is_some()).cloned();
        let id = match found {
            Some(id) => id,
            None => {
                // A rebuild: the album `albums.json` keeps under this legacy id, with its id.
                let kept = self.defs.albums.and_then(|d| {
                    d.albums
                        .iter()
                        .find(|(id, def)| def.legacy.as_deref() == Some(token) && !self.album_known(id))
                        .map(|(id, def)| (id.clone(), def.info.clone()))
                });
                let (id, info) = match kept {
                    Some(k) => k,
                    None => {
                        let info = folder.albums.get(token)?.clone();
                        let id =
                            (0..DRAWS).find_map(|_| self.lib.new_album_id(crate::search::random_bits).ok().filter(|id| !self.album_known(id)))?;
                        (id, info)
                    }
                };
                self.create.push((id.clone(), info));
                self.keys.insert(id.clone(), None);
                // The legacy id is set only when no album holds it yet.
                if self.lib.album_by_legacy(token).is_none() {
                    self.album_ids.push((id.clone(), Some(token.to_string())));
                }
                id
            }
        };
        self.albums.insert(token.to_string(), id.clone());
        Some(id)
    }

    /// The next position key at the end of album `album`.
    fn next_key(&mut self, album: &AlbumId) -> Option<PosKey> {
        let last = match self.keys.get(album) {
            Some(last) => last.clone(),
            None => self.lib.album_members(album).last().and_then(|p| self.lib.member_key(album, *p)).cloned(),
        };
        let next = match last {
            Some(last) => last.after().ok()?,
            None => PosKey::first(),
        };
        self.keys.insert(album.clone(), Some(next.clone()));
        Some(next)
    }

    /// The face legacy face `face` gives, its person found or made.
    fn face(&mut self, rect: dalica_core::Rect01, person: &LegacyPerson, folder: &LegacyFolder) -> Face {
        let who = match person {
            LegacyPerson::Ignored => FacePerson::Ignored,
            LegacyPerson::Contact(contact) => self.person(contact, folder).map_or(FacePerson::Unnamed, FacePerson::Person),
        };
        Face::new(rect, who)
    }

    /// The person of contact id `contact`: found by it, else by the name the folder gives it
    /// (ignoring case), else made; `None` when it is named nowhere.
    fn person(&mut self, contact: &str, folder: &LegacyFolder) -> Option<PersonId> {
        if let Some(id) = self.people.get(contact) {
            return Some(id.clone());
        }
        if let Some(id) = self.lib.person_by_legacy(contact).filter(|id| self.lib.person(id).is_some()).cloned() {
            self.people.insert(contact.to_string(), id.clone());
            return Some(id);
        }
        // A rebuild: the person `people.json` keeps under this legacy id, with their id and name.
        let kept = self.defs.people.and_then(|d| {
            let id = d.legacy.iter().find(|(id, l)| l.as_str() == contact && self.lib.person(id).is_none()).map(|(id, _)| id.clone())?;
            let def = d.people.get(&id)?;
            Some((id, def.name.clone(), def.nickname.clone()))
        });
        let (id, name, nickname) = match kept {
            Some(k) => k,
            None => {
                let name = folder.contacts.get(contact)?.clone();
                let id = (0..DRAWS).find_map(|_| self.lib.new_person_id(crate::search::random_bits).ok().filter(|id| !self.who.knows(id)))?;
                (id, name, String::new())
            }
        };
        let person = match self.who.define(self.lib, &id, name, nickname, false) {
            Defined::Op(add) => {
                self.add_people.push(add);
                id
            }
            Defined::Is(holder) => holder,
            Defined::Kept => id,
            Defined::NameTaken(_) => return None,
        };
        // The person takes the contact id when they have none and no one holds it.
        let free = self.lib.person_by_legacy(contact).is_none() && !self.person_ids.iter().any(|(p, _)| *p == person);
        if free && self.lib.person_legacy(&person).is_none() {
            self.person_ids.push((person.clone(), Some(contact.to_string())));
        }
        self.people.insert(contact.to_string(), person.clone());
        Some(person)
    }

    /// The faces and albums one photo's legacy data gives.
    fn photo(&mut self, legacy: &LegacyPhoto, folder: &LegacyFolder) -> (Vec<Face>, Vec<AlbumId>) {
        let faces = legacy.faces.iter().map(|f| self.face(f.rect, &f.person, folder)).collect();
        let mut albums: Vec<AlbumId> = Vec::new();
        for token in &legacy.albums {
            if let Some(id) = self.album(token, folder)
                && !albums.contains(&id)
            {
                albums.push(id);
            }
        }
        (faces, albums)
    }

    /// The albums and people decided, with their legacy ids, as ops that go first.
    fn ops(self) -> Vec<Op> {
        let mut out = Vec::new();
        if !self.create.is_empty() {
            out.push(Op::CreateAlbums { albums: self.create });
        }
        out.extend(self.add_people);
        if !self.album_ids.is_empty() || !self.person_ids.is_empty() {
            out.push(Op::SetLegacyIds { albums: self.album_ids, people: self.person_ids });
        }
        out
    }
}

/// `op`, one of the ops a scan applies for `folder`, with the folder's fact when the op adds the
/// folder, and the faces and album memberships legacy data gives the photos it adds (see the
/// module's notes); `None` when there is nothing to add.
pub(crate) fn with_legacy(lib: &Library, op: &Op, folder: &FoundFolder, defs: Defs<'_>) -> Option<Op> {
    let kids = children(op);
    let added = kids.iter().find_map(|o| match o {
        Op::AddFolder { folder: f } if f.path == folder.path => Some(f.id),
        _ => None,
    });
    let id: FolderId = added.or_else(|| lib.folder_by_path(&folder.path))?;
    // Renamed or moved on disk: what the model said comes back with it (`Session::files_op_after`).
    if kids.iter().any(|o| matches!(o, Op::RemoveFolder { id: r } if *r == id)) {
        return None;
    }
    let mut extra = Vec::new();
    if added.is_some() && folder.sidecar.has_legacy() {
        extra.push(Op::SetFolderSidecars { folders: vec![(id, folder.sidecar)] });
    }
    if let Some(legacy) = folder.legacy.as_deref() {
        let mut mapper = Mapper::new(lib, defs, kids);
        let (mut faces, mut members) = (Vec::new(), Vec::new());
        for o in kids {
            let Op::AddPhotos { photos } = o else { continue };
            for p in photos.iter().filter(|p| p.folder == id && lib.photo(p.id).is_none()) {
                let Some(data) = found(folder, &p.name).and_then(|f| f.legacy.as_ref()) else { continue };
                let (list, albums) = mapper.photo(data, legacy);
                if !list.is_empty() {
                    faces.push((p.id, list));
                }
                for album in albums {
                    if let Some(key) = mapper.next_key(&album) {
                        members.push((album, p.id, key));
                    }
                }
            }
        }
        extra.extend(mapper.ops());
        if !faces.is_empty() {
            extra.push(Op::SetFaces { faces });
        }
        if !members.is_empty() {
            extra.push(Op::AddMembers { members });
        }
    }
    (!extra.is_empty()).then(|| joined(op, extra))
}

/// `op` with folder `folder`'s fact when the op adds the folder (the fallback when the rest of
/// [`with_legacy`] cannot apply); `None` when there is none to add.
pub(crate) fn with_fact(lib: &Library, op: &Op, folder: &FoundFolder) -> Option<Op> {
    let id = children(op).iter().find_map(|o| match o {
        Op::AddFolder { folder: f } if f.path == folder.path && lib.folder(f.id).is_none() => Some(f.id),
        _ => None,
    })?;
    folder.sidecar.has_legacy().then(|| joined(op, vec![Op::SetFolderSidecars { folders: vec![(id, folder.sidecar)] }]))
}

/// The photo `name` of `folder` as the scan found it.
fn found<'f>(folder: &'f FoundFolder, name: &str) -> Option<&'f FoundPhoto> {
    folder.photos.binary_search_by(|f| f.name.as_str().cmp(name)).ok().and_then(|i| folder.photos.get(i))
}

/// The op that brings a folder the model knows in line with the sidecar in effect the scan found
/// (see the module's notes): the fact, and when the data's source changed (`reread`: a
/// `.dalica.json` appeared before the first write) the known photos and the folder read again
/// from what the scan found. `None` when the fact is the model's already.
pub(crate) fn transition(lib: &Library, folder: &FoundFolder, reread: bool, defs: Defs<'_>) -> Option<Op> {
    let id = lib.folder_by_path(&folder.path)?;
    let (before, after) = (lib.folder_sidecar(id), folder.sidecar);
    // Only a change where legacy data is involved is recorded: a `.dalica.json` Dalica wrote
    // into a folder that had none changes nothing in the model (a rescan imports nothing).
    if before == recorded(after) {
        return None;
    }
    let read_again = match (before, after) {
        (FolderSidecar::Legacy, FolderSidecar::None) | (FolderSidecar::None, FolderSidecar::Legacy) => true,
        (FolderSidecar::Legacy, FolderSidecar::Dalica | FolderSidecar::Both) => reread,
        _ => false,
    };
    let fact = Op::SetFolderSidecars { folders: vec![(id, recorded(after))] };
    if !read_again {
        return Some(fact);
    }
    Some(Op::Batch { ops: read_folder_again(lib, id, folder, defs, fact) })
}

/// The diff's `ops` for the folder a `transition` reads again, with the stars it sets: the diff was
/// made against the model before the transition, and a photo it updates (its file changed on
/// disk) carries the star the model had then, which would undo the transition's (M15.50: the
/// legacy star brought back with the option off, and written into a folder with no sidecar in
/// effect).
pub(crate) fn after_transition(transition: &Op, ops: &mut [Op]) {
    let mut stars: BTreeMap<PhotoId, bool> = BTreeMap::new();
    let mut stack = vec![transition];
    while let Some(op) = stack.pop() {
        match op {
            Op::Batch { ops } => stack.extend(ops),
            Op::SetStars { stars: list } => stars.extend(list.iter().copied()),
            _ => {}
        }
    }
    if stars.is_empty() {
        return;
    }
    let mut stack: Vec<&mut Op> = ops.iter_mut().collect();
    while let Some(op) = stack.pop() {
        match op {
            Op::Batch { ops } => stack.extend(ops.iter_mut()),
            Op::UpdatePhotos { photos } => {
                for p in photos.iter_mut() {
                    if let Some(star) = stars.get(&p.id) {
                        p.star = *star;
                    }
                }
            }
            _ => {}
        }
    }
}

/// The ops that give folder `id`'s known photos and the folder itself what the scan found, after
/// `fact`; then the albums and people with a legacy id they leave empty go.
fn read_folder_again(lib: &Library, id: FolderId, folder: &FoundFolder, defs: Defs<'_>, fact: Op) -> Vec<Op> {
    let mut mapper = Mapper::new(lib, defs, &[]);
    let legacy = folder.legacy.as_deref().filter(|_| folder.sidecar == FolderSidecar::Legacy);
    // Dalica's own file's people (a `.dalica.json` read again), by its names.
    let mut own_people = Vec::new();
    if legacy.is_none() {
        for (person, name) in &folder.people {
            if let Defined::Op(add) = mapper.who.define(lib, person, name.name.clone(), name.nickname.clone().unwrap_or_default(), false) {
                own_people.push(add);
            }
        }
    }
    let (mut stars, mut infos, mut faces, mut geo) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut add, mut remove) = (Vec::new(), Vec::new());
    // What leaves: memberships and named faces, to find the albums and people left empty.
    let mut left_albums: BTreeSet<AlbumId> = BTreeSet::new();
    let mut left_people: BTreeSet<PersonId> = BTreeSet::new();
    let mut changed_photos: BTreeSet<PhotoId> = BTreeSet::new();
    let mut new_people: BTreeSet<PersonId> = BTreeSet::new();
    for photo in lib.photos_in(id) {
        let Some(f) = found(folder, &photo.name) else { continue };
        if f.star != photo.star {
            stars.push((photo.id, f.star));
        }
        let info: PhotoInfo = f.info.clone();
        if info != *lib.info(photo.id) {
            infos.push((photo.id, info));
        }
        let (list, albums): (Vec<Face>, Vec<(AlbumId, Option<PosKey>)>) = match (legacy, &f.legacy) {
            (Some(l), Some(data)) => {
                let (list, albums) = mapper.photo(data, l);
                (list, albums.into_iter().map(|a| (a, None)).collect())
            }
            (Some(_), None) => (Vec::new(), Vec::new()),
            (None, _) => {
                let list = mapper.who.faces(f.faces.clone()).0;
                let albums = f.albums.iter().filter(|(a, _)| lib.album(a).is_some()).map(|(a, k)| (a.clone(), Some(k.clone()))).collect();
                (list, albums)
            }
        };
        let own = lib.faces(photo.id);
        if list.as_slice() != own {
            for p in own.iter().filter_map(|f| f.person.person()) {
                left_people.insert(p.clone());
            }
            for p in list.iter().filter_map(|f| f.person.person()) {
                new_people.insert(p.clone());
            }
            changed_photos.insert(photo.id);
            faces.push((photo.id, list));
        } else {
            for p in own.iter().filter_map(|f| f.person.person()) {
                new_people.insert(p.clone());
            }
        }
        let word: Option<Geo> = f.geo;
        if word != lib.geo(photo.id) {
            geo.push((photo.id, word));
        }
        let current: BTreeMap<AlbumId, PosKey> = lib.albums_of(photo.id).into_iter().map(|(a, k)| (a.clone(), k.clone())).collect();
        for album in current.keys() {
            if !albums.iter().any(|(a, _)| a == album) {
                remove.push((album.clone(), photo.id));
                left_albums.insert(album.clone());
            }
        }
        for (album, key) in albums {
            if current.contains_key(&album) {
                continue;
            }
            let key = match key {
                Some(k) => Some(k),
                None => mapper.next_key(&album),
            };
            if let Some(key) = key {
                add.push((album, photo.id, key));
            }
        }
    }
    let mut ops = vec![fact];
    ops.extend(mapper.ops());
    ops.extend(own_people);
    if !stars.is_empty() {
        ops.push(Op::SetStars { stars });
    }
    if !infos.is_empty() {
        ops.push(Op::SetPhotoInfo { photos: infos });
    }
    if folder.info != *lib.folder_info(id) {
        ops.push(Op::SetFolderInfo { folders: vec![(id, folder.info.clone())] });
    }
    if !remove.is_empty() {
        ops.push(Op::RemoveMembers { members: remove.clone() });
    }
    if !add.is_empty() {
        ops.push(Op::AddMembers { members: add.clone() });
    }
    if !faces.is_empty() {
        ops.push(Op::SetFaces { faces });
    }
    if !geo.is_empty() {
        ops.push(Op::SetGeo { geo });
    }
    // Albums and people only legacy data named, left with nothing in them.
    let gone_albums: Vec<AlbumId> = left_albums
        .into_iter()
        .filter(|a| lib.album_legacy(a).is_some() && !add.iter().any(|(b, ..)| b == a))
        .filter(|a| lib.album_members(a).iter().all(|p| remove.iter().any(|(b, q)| b == a && q == p)))
        .collect();
    if !gone_albums.is_empty() {
        ops.push(Op::DeleteAlbums { ids: gone_albums });
    }
    for person in left_people {
        if new_people.contains(&person) || lib.person_legacy(&person).is_none() {
            continue;
        }
        if lib.person_photos(&person).iter().all(|p| changed_photos.contains(p)) {
            ops.push(Op::DeletePerson { id: person });
        }
    }
    ops
}

impl Session {
    /// The legacy data's state (`data-model.md` §9).
    pub fn legacy(&self) -> &Legacy {
        &self.legacy
    }

    /// The number of folders with legacy data, the first time a scan finds any this session while
    /// `files.legacyNoticeShown` is off (the first-run notice, `organizing.md` §1); `None` after
    /// that, or while no scan found any. Taken once.
    pub fn legacy_notice(&mut self) -> Option<usize> {
        self.legacy.notice.take()
    }

    /// The first-run notice was answered: it is not given again (`files.legacyNoticeShown`).
    pub(crate) fn legacy_notice_shown(&mut self) -> bool {
        self.legacy.notice = None;
        self.legacy.given = true;
        let before = self.settings().legacy();
        if before.legacy_notice_shown {
            return false;
        }
        self.change_settings(|st| {
            let mut l = st.legacy();
            l.legacy_notice_shown = true;
            st.set_legacy(&l);
        });
        true
    }

    /// After a scan ended: the notice waits with the number of folders with legacy data, the
    /// first time any is found while it was never answered.
    pub(crate) fn legacy_after_scan(&mut self) {
        if self.legacy.given || self.settings().legacy().legacy_notice_shown {
            return;
        }
        let count = self.library.legacy_folders().len();
        if count > 0 {
            self.legacy.notice = Some(count);
            self.legacy.given = true;
        }
    }

    /// Whether the folder at `path` is to be read again from its `.dalica.json` (one appeared
    /// before the first write); asking forgets it.
    pub(crate) fn legacy_reread(&mut self, path: &std::path::Path) -> bool {
        self.legacy.reread.remove(path)
    }

    /// Takes in the first writes the I/O worker reported: a converted folder's fact becomes
    /// `both`; a folder where a `.dalica.json` appeared is scanned again and read from it. Never
    /// touches the disk. Returns how many it took in.
    pub(crate) fn poll_legacy(&mut self) -> usize {
        let writes = std::mem::take(&mut *self.legacy.writes.lock().unwrap_or_else(PoisonError::into_inner));
        let n = writes.len();
        for (folder, what) in writes {
            let Some(path) = self.library.folder(folder).map(|f| f.path.clone()) else { continue };
            match what {
                LegacyWrite::Converted => {
                    if legacy_only(self.library.folder_sidecar(folder))
                        && let Err(e) = self.apply_derived(Op::SetFolderSidecars { folders: vec![(folder, FolderSidecar::Both)] })
                    {
                        self.scan_warnings.push(format!("{}: the folder's sidecar in effect not recorded: {e}", path.display()));
                    }
                }
                LegacyWrite::Appeared => {
                    self.legacy.reread.insert(path.clone());
                    self.start_region_scan(&path);
                }
            }
        }
        n
    }

    /// The folders of `touched` (marked for a sidecar write by an op from the files, whose inverse
    /// is `inverse`) that are written. Legacy data enters the model as a scan's imports do, with
    /// nothing written (`data-model.md` §9): a folder whose fact the op changed to or from
    /// legacy-only is left out, and so is a legacy-only folder, unless a write of the user's (a
    /// command's) is queued there already (that write then takes the scan's data too). A write
    /// of the files' queued there (sent while the folder was read without legacy data, the option
    /// off) lets nothing in (M15.50).
    pub(crate) fn keep_legacy_unwritten(&self, touched: BTreeSet<FolderId>, inverse: &Op) -> BTreeSet<FolderId> {
        let mut steady = BTreeSet::new();
        let mut quiet = BTreeSet::new();
        for &f in &touched {
            let now = self.library.folder_sidecar(f);
            match fact_before(inverse, f) {
                Some(before) if legacy_only(before) || legacy_only(now) => {
                    quiet.insert(f);
                }
                None if legacy_only(now) => {
                    steady.insert(f);
                }
                _ => {}
            }
        }
        if quiet.is_empty() && steady.is_empty() {
            return touched;
        }
        let queued = if steady.is_empty() { BTreeSet::new() } else { self.queued_user_sidecars() };
        touched.into_iter().filter(|f| !quiet.contains(f) && (!steady.contains(f) || queued.contains(f))).collect()
    }
}
