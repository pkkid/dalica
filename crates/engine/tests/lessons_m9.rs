//! M9's data lessons end to end (`roadmap.md` M9.24; `data-model.md` §10), through the commands
//! (`Session::execute`) and a real (polling) watcher where the lesson says so, on temporary
//! folders:
//!
//! - §10.2: a folder moved (to another parent, under another name) outside Dalica, between two
//!   sessions (found by the verification pass of the next one) and while Dalica watches (found by
//!   the watcher), keeps its id and its photos' ids, stars, captions, tags, recipes, album
//!   memberships in their order and the album's cover, shows nothing as new, and a write at its
//!   new path keeps all of it on disk;
//! - §10.3 (photos moved inside Dalica carry their data) is `move_copy.rs`, `fileops_store.rs` and,
//!   with the watcher on, `undo_emptied_folder.rs`;
//! - §10.7: names with `]`, `"` (not on Windows), `;`, `=`, non-ASCII and very long names through
//!   move, copy and delete, and the undo of each (renames: `rename.rs`);
//! - §10.12: an album whose members live in several folders, after M9's file operations moved and
//!   renamed some of them, keeps its exact order once `albums.json` and the index are deleted and
//!   the library is rebuilt.
//!
//! These tests use the real OS trash and put back what they sent there (where the system allows).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{entry, files, off_owner, options, read_opt, shared, under};
use dalica_core::{AlbumId, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::cmd::delete::RESTORES;
use dalica_engine::watching::{FIRST_VERIFICATION, WatchMode};
use dalica_engine::{FileOpEvent, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// A folder's sidecar (`Null` when there is none).
fn sidecar(folder: &Path) -> Value {
    read_opt(&folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

/// Puts back the files undo sent to the trash (`path`, with the identity it had then), so the
/// trash is left as it was and the temporary folders take them with them.
fn untrash(items: &[(PathBuf, dalica_fs::Identity)]) {
    if !RESTORES {
        return;
    }
    for (path, identity) in items {
        let (path, identity) = (path.clone(), *identity);
        let back = off_owner(move || {
            let original = std::fs::canonicalize(path.parent().unwrap()).unwrap().join(path.file_name().unwrap());
            dalica_fs::trash::restore(&[dalica_fs::trash::Trashed { original, identity, folder: false }]).unwrap()
        });
        assert_eq!(back, [dalica_fs::trash::Restore::Restored], "back from the trash");
    }
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96 + 8 * (seed % 5) as u32, 64), 80, None).unwrap()
}

/// A library watching `Pictures` on a virtual clock, the watcher off unless a test turns it on.
struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    s: Session,
}

impl Lib {
    /// `files` (paths under `Pictures`) written as photos, the library opened and scanned.
    fn new(files: &[&str]) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Pictures");
        for (i, rel) in files.iter().enumerate() {
            let path = under(&pictures, rel);
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &jpeg(i as u64), Expect::Absent).unwrap();
        }
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
        let mut s = Lib::open(dir.path(), &clock);
        s.execute("folders.add", &json!({ "path": pictures })).unwrap();
        s.wait_idle();
        Lib { dir, clock, s }
    }

    /// `Trips/Lisbon` (3 photos), `Trips/Porto` (2) and `Trips/Sintra` (2).
    fn trips() -> Lib {
        Lib::new(&[
            "Trips/Lisbon/IMG_0001.jpg",
            "Trips/Lisbon/IMG_0002.jpg",
            "Trips/Lisbon/IMG_0003.jpg",
            "Trips/Porto/IMG_0101.jpg",
            "Trips/Porto/IMG_0102.jpg",
            "Trips/Sintra/IMG_0201.jpg",
            "Trips/Sintra/IMG_0202.jpg",
        ])
    }

    fn open(dir: &Path, clock: &Arc<VirtualClock>) -> Session {
        let library = dir.join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        let mut s = Session::open_with(&library, clock.clone(), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s
    }

    /// Closes the session (everything written) and opens the library again, the watcher off.
    fn reopen(mut self) -> Lib {
        self.settle_writes();
        let Lib { dir, clock, s } = self;
        drop(s);
        let s = Lib::open(dir.path(), &clock);
        Lib { dir, clock, s }
    }

    fn path(&self, rel: &str) -> PathBuf {
        under(&self.dir.path().join("Pictures"), rel)
    }

    fn folder(&self, rel: &str) -> FolderId {
        self.s.library().folder_by_path(&self.path(rel)).unwrap_or_else(|| panic!("no folder {rel}"))
    }

    /// The photo at `"Trips/Lisbon/IMG_0001.jpg"`.
    fn photo(&self, rel: &str) -> PhotoId {
        let (folder, name) = rel.rsplit_once('/').unwrap();
        self.s.library().photo_by_name(self.folder(folder), name).unwrap_or_else(|| panic!("no photo {rel}")).id
    }

    /// Photo `id` as `"Trips/Lisbon/IMG_0001.jpg"`.
    fn rel(&self, id: PhotoId) -> String {
        let p = self.s.library().photo(id).unwrap();
        let folder = &self.s.library().folder(p.folder).unwrap().path;
        let rel = folder.strip_prefix(self.dir.path().join("Pictures")).unwrap();
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        format!("{}/{}", parts.join("/"), p.name)
    }

    /// Album `a`'s members in album order, by path.
    fn members(&self, a: &AlbumId) -> Vec<String> {
        self.s.library().album_members(a).iter().map(|m| self.rel(*m)).collect()
    }

    fn album(&mut self, name: &str, members: &[&str]) -> AlbumId {
        let ids: Vec<PhotoId> = members.iter().map(|m| self.photo(m)).collect();
        let v = self.s.execute("album.create", &json!({ "name": name, "ids": ids })).unwrap();
        AlbumId::parse(v["album"].as_str().unwrap()).unwrap()
    }

    /// Everything the commands sent on its way is written: file jobs, metadata write-backs and
    /// sidecars.
    fn settle_writes(&mut self) {
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        assert!(self.s.flush_metadata(PATIENCE), "the metadata writes ended");
        self.s.wait_idle();
        self.s.flush().unwrap();
    }

    /// Runs `id`, then waits for its file job and what it wrote: the operation's event, which
    /// must be clean.
    fn run(&mut self, id: &str, p: Value) -> FileOpEvent {
        let before = self.s.file_op_events().count();
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e:?}"));
        self.settle_writes();
        let events: Vec<FileOpEvent> = self.s.file_op_events().cloned().collect();
        assert!(events.len() > before, "{id} ran a file operation");
        let e = events.last().cloned().unwrap();
        assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{id}: {e:?}");
        e
    }

    fn undo(&mut self) -> FileOpEvent {
        self.run("edit.undo", json!({}))
    }

    /// Lets the polling watcher sweep a few times on the virtual clock and applies what it found.
    fn settle_watcher(&mut self) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(self.s.wait_watching(Duration::from_secs(60)), "the watcher answered");
            self.s.wait_idle();
        }
    }

    /// The verification pass of a session's first minute (changes made while it was closed).
    fn first_verification(&mut self) {
        self.s.poll();
        self.clock.advance(FIRST_VERIFICATION + Duration::from_secs(1));
        self.s.poll();
        self.s.wait_idle();
    }

    fn albums_json(&mut self) -> Value {
        self.s.flush().unwrap();
        read_opt(&self.dir.path().join("library").join("albums.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
    }
}

/// What the user said about the photos of a folder, by name: (star, caption, tags, recipe).
fn said(l: &Lib, folder: FolderId) -> BTreeMap<String, (bool, String, Vec<String>, Vec<String>)> {
    let lib = l.s.library();
    lib.photos_in(folder)
        .into_iter()
        .map(|p| {
            let i = lib.info(p.id);
            (p.name.clone(), (p.star, i.caption.clone(), i.tags.clone(), i.edits.clone()))
        })
        .collect()
}

/// How the folder `Trips/Porto` left: moved outside Dalica.
#[derive(Clone, Copy, Debug)]
enum Outside {
    /// While no session was open: the next session's verification pass finds it.
    BetweenSessions,
    /// While Dalica watched: the (polling) watcher finds it.
    WhileWatching,
}

/// `data-model.md` §10.2: a folder moved outside Dalica (to another parent, under another name)
/// keeps everything said about it and its photos — stars, captions, tags, recipes, album
/// memberships and their order — and, as one folder that moved rather than one gone and one new,
/// its id, its photos' ids, the album's cover and their first-indexed times (nothing shows as
/// new); a write at the new path keeps it all on disk, and `albums.json` follows the cover.
#[test]
fn a_folder_moved_outside_dalica_keeps_all_its_data() {
    let _faults = shared();
    for how in [Outside::BetweenSessions, Outside::WhileWatching] {
        let mut l = Lib::trips();
        if matches!(how, Outside::WhileWatching) {
            l.s.set_watching(WatchMode::Polling);
            l.settle_watcher();
        }
        let porto = l.folder("Trips/Porto");
        let (first, second) = (l.photo("Trips/Porto/IMG_0101.jpg"), l.photo("Trips/Porto/IMG_0102.jpg"));
        l.s.execute("photo.star.set", &json!({ "ids": [first], "on": true })).unwrap();
        l.s.execute("photo.caption.set", &json!({ "ids": [first], "caption": "Ponte Luís I" })).unwrap();
        l.s.execute("photo.tags.add", &json!({ "ids": [second], "tags": "porto, river" })).unwrap();
        l.s.execute("photo.history.apply", &json!({ "id": second, "op": "sepia()" })).unwrap();
        let trip =
            l.album("Trip", &["Trips/Porto/IMG_0102.jpg", "Trips/Lisbon/IMG_0001.jpg", "Trips/Porto/IMG_0101.jpg", "Trips/Sintra/IMG_0201.jpg"]);
        let river = l.album("River", &["Trips/Porto/IMG_0102.jpg", "Trips/Porto/IMG_0101.jpg"]);
        l.s.execute("album.setCover", &json!({ "album": trip, "id": first })).unwrap();
        l.settle_writes();
        let said_before = said(&l, porto);
        let (trip_ids, river_ids) = (l.s.library().album_members(&trip).to_vec(), l.s.library().album_members(&river).to_vec());
        let trip_order = l.members(&trip);
        assert_eq!(trip_order, ["Trips/Porto/IMG_0102.jpg", "Trips/Lisbon/IMG_0001.jpg", "Trips/Porto/IMG_0101.jpg", "Trips/Sintra/IMG_0201.jpg"]);
        let indexed = l.s.library().first_indexed(first);
        assert!(indexed.is_some());

        // Long after they were indexed (by the clock), so a photo taken for new would be recent.
        l.clock.advance(Duration::from_secs(60 * 86_400));
        let (from, to) = (l.path("Trips/Porto"), l.path("Elsewhere/Oporto"));
        let moved = || {
            off_owner(|| {
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::rename(&from, &to).unwrap();
            });
        };
        match how {
            Outside::BetweenSessions => {
                l = l.reopen();
                moved();
                l.first_verification();
            }
            Outside::WhileWatching => {
                moved();
                l.settle_watcher();
            }
        }
        let case = format!("{how:?}");
        let lib = l.s.library();
        assert_eq!(lib.folder_by_path(&from), None, "{case}: the old path left the library");
        assert_eq!(lib.folder_by_path(&to), Some(porto), "{case}: the folder keeps its id");
        assert_eq!(
            (l.photo("Elsewhere/Oporto/IMG_0101.jpg"), l.photo("Elsewhere/Oporto/IMG_0102.jpg")),
            (first, second),
            "{case}: and its photos theirs"
        );
        assert_eq!(said(&l, porto), said_before, "{case}: stars, captions, tags and recipes");
        assert_eq!((lib.album_members(&trip), lib.album_members(&river)), (&trip_ids[..], &river_ids[..]), "{case}: memberships in order");
        assert_eq!(l.members(&trip), trip_order.iter().map(|p| p.replace("Trips/Porto/", "Elsewhere/Oporto/")).collect::<Vec<_>>());
        assert_eq!(lib.cover_set(&trip), Some(first), "{case}: the cover");
        assert_eq!(lib.first_indexed(first), indexed, "{case}: first indexed when it was");
        assert!(lib.recently_updated(l.s.clock().now(), 30).is_empty(), "{case}: nothing shows as new");

        // A write at the new path keeps everything on disk.
        l.s.execute("photo.star.set", &json!({ "ids": [second], "on": true })).unwrap();
        l.settle_writes();
        let doc = sidecar(&to);
        let one = &doc["photos"]["IMG_0101.jpg"];
        let two = &doc["photos"]["IMG_0102.jpg"];
        assert_eq!((one["star"].clone(), one["caption"].clone()), (json!(true), json!("Ponte Luís I")), "{case}: {doc}");
        assert_eq!((two["tags"].clone(), two["edits"].clone()), (json!(["porto", "river"]), json!(["sepia()"])), "{case}: {doc}");
        for album in [&trip, &river] {
            assert!(one["albums"].get(album.as_str()).is_some() && two["albums"].get(album.as_str()).is_some(), "{case}: {doc}");
        }
        assert_eq!(l.albums_json()["albums"][trip.as_str()]["cover"], json!(to.join("IMG_0101.jpg")), "{case}: albums.json follows the cover");
    }
}

/// The names `data-model.md` §10.7 lists: `]`, `"` (not on Windows, which refuses it), `;`, `=`,
/// non-ASCII and a very long one: 243 bytes (few UTF-16 units, so paths stay short on Windows).
/// Not 255: the freedesktop trash names its record `<name>.trashinfo`, so a name over 245 bytes
/// cannot go to the trash there (delete, and the undo of a copy, fail with the file left in
/// place); the 255-byte name goes through the renames (`rename.rs`).
fn hard_names() -> Vec<String> {
    let long = format!("{}xx.jpg", "長".repeat(79));
    assert_eq!(long.len(), 243);
    let mut v = vec!["a]b.jpg".to_string(), "c;d=e.jpg".into(), "Praça à noite 東京.jpg".into(), long];
    if !cfg!(windows) {
        v.push("q\"uote.jpg".into());
    }
    v.sort();
    v
}

/// `data-model.md` §10.7 through M9's file operations: photos with hard names, in and into a
/// folder with a hard name, are moved, copied and deleted, and each undo gives back the exact
/// names, bytes and sidecar entries.
#[test]
fn hard_names_survive_move_copy_delete_and_their_undo() {
    let _faults = shared();
    let names = hard_names();
    let mut files_: Vec<String> = names.iter().map(|n| format!("Odd/{n}")).collect();
    files_.push("Dest ]; a=b ç/IMG_0001.jpg".into());
    let l_files: Vec<&str> = files_.iter().map(String::as_str).collect();
    let mut l = Lib::new(&l_files);
    let (odd, dest) = (l.path("Odd"), l.path("Dest ]; a=b ç"));
    let (odd_id, dest_id) = (l.folder("Odd"), l.folder("Dest ]; a=b ç"));
    let ids: Vec<PhotoId> = names.iter().map(|n| l.photo(&format!("Odd/{n}"))).collect();
    for (id, n) in ids.iter().zip(&names) {
        l.s.execute("photo.tags.add", &json!({ "ids": [id], "tags": n.trim_end_matches(".jpg").chars().take(20).collect::<String>() })).unwrap();
    }
    l.s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
    l.settle_writes();
    let bytes: Vec<Vec<u8>> = names.iter().map(|n| read_opt(&odd.join(n)).unwrap()).collect();
    let entries: Vec<Value> = names.iter().map(|n| entry(&odd, n).unwrap()).collect();
    let at_home = |l: &Lib, case: &str| {
        assert_eq!(files(&odd), names, "{case}: the exact names");
        for (i, n) in names.iter().enumerate() {
            assert!(read_opt(&odd.join(n)).unwrap() == bytes[i], "{case}: {n}");
            assert_eq!(entry(&odd, n).unwrap()["star"], entries[i]["star"], "{case}: {n}");
            assert_eq!(entry(&odd, n).unwrap()["tags"], entries[i]["tags"], "{case}: {n}");
            assert_eq!(l.s.library().photo(ids[i]).map(|p| (p.folder, p.name.clone())), Some((odd_id, n.clone())), "{case}: {n}");
        }
    };
    at_home(&l, "before");

    // Move, and its undo.
    l.run("photo.moveTo", json!({ "ids": ids, "folder": dest_id, "confirm": true }));
    let mut want = names.clone();
    want.push("IMG_0001.jpg".into());
    want.sort();
    assert_eq!(files(&dest), want, "moved under the same names");
    for (i, n) in names.iter().enumerate() {
        assert!(read_opt(&dest.join(n)).unwrap() == bytes[i], "{n}");
        assert_eq!(entry(&dest, n).unwrap()["tags"], entries[i]["tags"], "{n}");
        assert_eq!(l.s.library().photo(ids[i]).map(|p| p.folder), Some(dest_id));
    }
    assert!(files(&odd).is_empty());
    l.undo();
    at_home(&l, "move undone");

    // Copy, and its undo.
    l.run("photo.copyTo", json!({ "ids": ids, "folder": dest_id }));
    assert_eq!(files(&dest), want, "copied under the same names");
    for (i, n) in names.iter().enumerate() {
        assert!(read_opt(&dest.join(n)).unwrap() == bytes[i], "{n}");
        assert_eq!(entry(&dest, n).unwrap()["star"], json!(true), "{n}");
    }
    at_home(&l, "copied");
    let copies: Vec<(PathBuf, dalica_fs::Identity)> = names
        .iter()
        .map(|n| {
            let path = dest.join(n);
            let p = path.clone();
            (path, off_owner(move || dalica_fs::identity(&p).unwrap().unwrap()))
        })
        .collect();
    l.undo();
    assert_eq!(files(&dest), ["IMG_0001.jpg"], "copy undone");
    at_home(&l, "copy undone");
    untrash(&copies);

    // Delete to the trash, and its undo where the system can list its trash.
    l.run("photo.delete", json!({ "ids": ids, "confirm": true }));
    assert!(files(&odd).is_empty(), "every hard name went to the trash");
    assert!(ids.iter().all(|id| l.s.library().photo(*id).is_none()));
    if RESTORES {
        l.undo();
        at_home(&l, "delete undone");
    }
}

/// `data-model.md` §10.12 after M9's file operations: an album over three folders in a custom
/// order, some of its members then moved to another folder, renamed, and their folder renamed and
/// moved through Dalica, keeps its exact order once `albums.json` and the index are deleted and the
/// library is rebuilt from the folders.
#[test]
fn an_album_keeps_its_order_after_file_operations_and_a_rebuild_from_nothing() {
    let _faults = shared();
    let mut l = Lib::trips();
    let trip = l.album(
        "Trip",
        &[
            "Trips/Sintra/IMG_0202.jpg",
            "Trips/Lisbon/IMG_0002.jpg",
            "Trips/Porto/IMG_0101.jpg",
            "Trips/Lisbon/IMG_0003.jpg",
            "Trips/Porto/IMG_0102.jpg",
        ],
    );
    let (lisbon2, porto1) = (l.photo("Trips/Lisbon/IMG_0002.jpg"), l.photo("Trips/Porto/IMG_0101.jpg"));
    let (sintra, porto, lisbon) = (l.folder("Trips/Sintra"), l.folder("Trips/Porto"), l.folder("Trips/Lisbon"));
    l.run("photo.moveTo", json!({ "ids": [lisbon2], "folder": sintra, "confirm": true }));
    l.run("photo.rename", json!({ "ids": [porto1], "name": "Ponte" }));
    l.run("folder.rename", json!({ "folder": porto, "name": "Oporto" }));
    l.run("folder.move", json!({ "folder": porto, "to": lisbon }));
    let order = l.members(&trip);
    assert_eq!(
        order,
        [
            "Trips/Sintra/IMG_0202.jpg",
            "Trips/Sintra/IMG_0002.jpg",
            "Trips/Lisbon/Oporto/Ponte.jpg",
            "Trips/Lisbon/IMG_0003.jpg",
            "Trips/Lisbon/Oporto/IMG_0102.jpg"
        ]
    );
    l.settle_writes();

    // Everything but the folders lost: the index and `albums.json`.
    let mut l = l.reopen();
    let Lib { dir, clock, s } = l;
    drop(s);
    let library = dir.path().join("library");
    for name in ["index.log", "index.snap", "albums.json"] {
        let path = library.join(name);
        off_owner(move || match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{}: {e}", path.display()),
        });
    }
    let s = Lib::open(dir.path(), &clock);
    l = Lib { dir, clock, s };
    assert_eq!(l.s.library().photos().count(), 0, "the index is gone");
    l.s.execute("library.rebuild", &json!({})).unwrap();
    l.s.wait_idle();
    let found: Vec<AlbumId> = l.s.library().albums().filter(|(_, i)| i.name == "Trip").map(|(id, _)| id.clone()).collect();
    assert_eq!(found, std::slice::from_ref(&trip), "the album and its id from the sidecars");
    assert_eq!(l.members(&trip), order, "the exact order");
}
