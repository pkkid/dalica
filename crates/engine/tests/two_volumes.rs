//! A move across two file systems (`roadmap.md` M9.24; `organizing.md` §9, `data-model.md` §8,
//! §10.3, §10.13): photos and a folder moved from one volume to another through `photo.moveTo`
//! and `folder.move` are copied, verified byte for byte, and only then is the source sent to the
//! trash; a copy that does not verify leaves the source untouched and nothing at the target; undo
//! brings the files, their sidecar entries and the library back.
//!
//! On Linux, when `/dev/shm` and the temporary folder are different devices, the second volume is
//! a folder in `/dev/shm` and the move crosses a real device boundary; elsewhere (or when they are
//! one device) the same test runs with the cross-volume path forced
//! (`dalica_fs::faults::points::MOVE_CROSS_VOLUME`). It prints which ran. What it sent to the trash
//! is put back and deleted with its temporary folders (where the system's trash can be listed),
//! and a trash folder it had to make on `/dev/shm` is removed again once empty.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{entry, exclusive, names, off_owner, options, read_opt, wait_files};
use dalica_core::{AlbumId, FolderId, PhotoId, SystemClock};
use dalica_engine::cmd::delete::RESTORES;
use dalica_engine::watching::WatchMode;
use dalica_engine::{Direction, FileOpEvent, FileOpKind, Session};
use dalica_fs::Expect;
use dalica_fs::Identity;
use dalica_fs::faults::{self, Fault, points};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    off_owner(move || path.symlink_metadata().is_ok())
}

fn identity(path: &Path) -> Identity {
    let path = path.to_path_buf();
    off_owner(move || dalica_fs::identity(&path).unwrap().unwrap())
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96 + 8 * (seed % 5) as u32, 64), 80, None).unwrap()
}

/// The two volumes: a temporary folder on each, and whether the move between them is forced.
struct Volumes {
    here: tempfile::TempDir,
    there: tempfile::TempDir,
    forced: bool,
    /// A trash folder `there` that this test made (removed again once empty).
    made_trash: Option<PathBuf>,
}

impl Volumes {
    fn new() -> Volumes {
        let here = tempfile::tempdir().unwrap();
        if let Some((there, made_trash)) = Volumes::shm(here.path()) {
            println!("two volumes: a real cross-device move between {} and {}", here.path().display(), there.path().display());
            return Volumes { here, there, forced: false, made_trash };
        }
        let there = tempfile::tempdir().unwrap();
        println!("two volumes: one device here, so the cross-volume path is forced ({} and {})", here.path().display(), there.path().display());
        Volumes { here, there, forced: true, made_trash: None }
    }

    /// A folder in `/dev/shm` when that is another device than `here` (Linux), with the trash
    /// folder this user would get there if it is not there yet.
    #[cfg(target_os = "linux")]
    fn shm(here: &Path) -> Option<(tempfile::TempDir, Option<PathBuf>)> {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let (a, b) = (std::fs::metadata(here).ok()?, std::fs::metadata(shm).ok()?);
        if !b.is_dir() || a.dev() == b.dev() {
            return None;
        }
        let trash = shm.join(format!(".Trash-{}", std::fs::metadata(here).ok()?.uid()));
        let made = (!trash.exists()).then_some(trash);
        let dir = tempfile::Builder::new().prefix("dalica-two-volumes-").tempdir_in(shm).ok()?;
        Some((dir, made))
    }

    #[cfg(not(target_os = "linux"))]
    fn shm(_here: &Path) -> Option<(tempfile::TempDir, Option<PathBuf>)> {
        None
    }

    /// Arms the forced path, when it is the one that runs.
    fn arm(&self) {
        if self.forced {
            faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
    }
}

impl Drop for Volumes {
    fn drop(&mut self) {
        faults::disarm_all();
        // A trash folder made for this test goes once nothing is left in it (never anything more).
        if let Some(trash) = &self.made_trash {
            let empty = |d: &Path| std::fs::read_dir(d).map(|mut e| e.next().is_none()).unwrap_or(true);
            if empty(&trash.join("files")) && empty(&trash.join("info")) {
                for sub in ["files", "info", "expunged"] {
                    let _ = std::fs::remove_dir(trash.join(sub));
                }
                let _ = std::fs::remove_dir(trash);
            }
        }
    }
}

/// Puts back what the test sent to the trash (`original`, with the identity it had then), first
/// deleting what stands at that path now (the test's own temporary file or folder), so the trash is
/// left as it was and the temporary folders take everything with them.
fn untrash(items: &[(PathBuf, Identity, bool)]) {
    if !RESTORES {
        return;
    }
    for (path, identity, folder) in items {
        let (path, identity, folder) = (path.clone(), *identity, *folder);
        let back = off_owner(move || {
            if folder {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
            let original = std::fs::canonicalize(path.parent().unwrap()).unwrap().join(path.file_name().unwrap());
            dalica_fs::trash::restore(&[dalica_fs::trash::Trashed { original, identity, folder }]).unwrap()
        });
        assert_eq!(back, [dalica_fs::trash::Restore::Restored], "back from the trash");
    }
}

/// A library watching `Pictures` on one volume (`Lisbon` with three photos, one of them with a
/// hard name, a note, and `Lisbon/Day2` with one) and `Away` on the other (`Porto` with one).
struct Lib {
    v: Volumes,
    s: Session,
}

const HARD: &str = "Praça ]; a=b.jpg";

impl Lib {
    fn new() -> Lib {
        let v = Volumes::new();
        let lisbon = v.here.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon.join("Day2")).unwrap();
        for (i, name) in ["IMG_0001.jpg", "IMG_0002.jpg", HARD].iter().enumerate() {
            dalica_fs::replace(&lisbon.join(name), &jpeg(i as u64), Expect::Absent).unwrap();
        }
        dalica_fs::replace(&lisbon.join("notes.txt"), b"not a picture", Expect::Absent).unwrap();
        dalica_fs::replace(&lisbon.join("Day2").join("DAY_0001.jpg"), &jpeg(7), Expect::Absent).unwrap();
        let porto = v.there.path().join("Away").join("Porto");
        dalica_fs::create_dir_all(&porto).unwrap();
        dalica_fs::replace(&porto.join("PORTO_0001.jpg"), &jpeg(9), Expect::Absent).unwrap();
        let mut s = Session::open_with(&v.here.path().join("library"), Arc::new(SystemClock), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        for root in [v.here.path().join("Pictures"), v.there.path().join("Away")] {
            s.execute("folders.add", &json!({ "path": root })).unwrap();
        }
        s.wait_idle();
        Lib { v, s }
    }

    fn lisbon(&self) -> PathBuf {
        self.v.here.path().join("Pictures").join("Lisbon")
    }

    fn porto(&self) -> PathBuf {
        self.v.there.path().join("Away").join("Porto")
    }

    fn folder(&self, path: &Path) -> FolderId {
        self.s.library().folder_by_path(path).unwrap_or_else(|| panic!("no folder {}", path.display()))
    }

    fn photo(&self, folder: &Path, name: &str) -> PhotoId {
        self.s.library().photo_by_name(self.folder(folder), name).unwrap().id
    }

    fn where_is(&self, id: PhotoId) -> PathBuf {
        let p = self.s.library().photo(id).unwrap();
        self.s.library().folder(p.folder).unwrap().path.join(&p.name)
    }

    /// Runs `id` with the cross-volume path armed when forced, waits for it and what it wrote: its
    /// event.
    fn run(&mut self, id: &str, p: Value) -> FileOpEvent {
        let before = self.s.file_op_events().count();
        self.v.arm();
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e:?}"));
        wait_files(&mut self.s);
        faults::disarm_all();
        assert!(self.s.flush_metadata(PATIENCE), "the metadata writes ended");
        self.s.wait_idle();
        self.s.flush().unwrap();
        let events: Vec<FileOpEvent> = self.s.file_op_events().cloned().collect();
        assert!(events.len() > before, "{id} ran a file operation");
        events.last().cloned().unwrap()
    }
}

fn clean(e: &FileOpEvent) {
    assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{e:?}");
}

/// Photos moved to the other volume are copied, verified byte for byte, and their sources go to
/// the trash; they keep their ids, data, album places and sidecar entries; a copy that does not
/// verify changes nothing; undo brings everything back.
#[test]
fn photos_moved_to_another_volume_are_copied_verified_then_trashed() {
    let _faults = exclusive();
    let mut l = Lib::new();
    let (lisbon, porto) = (l.lisbon(), l.porto());
    let names_moved = ["IMG_0001.jpg", HARD];
    let ids: Vec<PhotoId> = names_moved.iter().map(|n| l.photo(&lisbon, n)).collect();
    let other = l.photo(&porto, "PORTO_0001.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "Praça" })).unwrap();
    l.s.execute("photo.tags.add", &json!({ "ids": ids, "tags": "lisbon" })).unwrap();
    l.s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()" })).unwrap();
    let v = l.s.execute("album.create", &json!({ "name": "Trip", "ids": [ids[1], other, ids[0]] })).unwrap();
    let album = AlbumId::parse(v["album"].as_str().unwrap()).unwrap();
    assert!(l.s.flush_metadata(PATIENCE));
    l.s.flush().unwrap();
    let bytes: Vec<Vec<u8>> = names_moved.iter().map(|n| read_opt(&lisbon.join(n)).unwrap()).collect();
    let sources: Vec<Identity> = names_moved.iter().map(|n| identity(&lisbon.join(n))).collect();
    let entries: Vec<Value> = names_moved.iter().map(|n| entry(&lisbon, n).unwrap()).collect();
    let model = l.s.library().model().clone();
    let order = l.s.library().album_members(&album).to_vec();
    let porto_id = l.folder(&porto);
    let steps = l.s.undo_stack().len();

    // A byte of a copy flipped: the verification catches it, the source stays, nothing is left.
    faults::arm_global(points::COPY_FLIP, Fault::Error);
    let e = l.run("photo.moveTo", json!({ "ids": [ids[0]], "folder": porto_id, "confirm": true }));
    assert_eq!((e.done, e.failures.len()), (0, 1), "{e:?}");
    assert!(read_opt(&lisbon.join("IMG_0001.jpg")).unwrap() == bytes[0], "the source is untouched");
    assert_eq!(names(&porto), [".dalica.json", "PORTO_0001.jpg"], "nothing at the target");
    assert_eq!(l.s.library().model(), &model);
    assert_eq!(l.s.undo_stack().len(), steps);

    // Copied, verified, then the sources to the trash: one photo per move, since the forced path's
    // switch fires once (for one item).
    for id in &ids {
        let e = l.run("photo.moveTo", json!({ "ids": [id], "folder": porto_id, "confirm": true }));
        clean(&e);
        assert_eq!((e.kind, e.direction, e.done), (FileOpKind::Move, Direction::Do, 1));
    }
    let mut copies = Vec::new();
    for (i, n) in names_moved.iter().enumerate() {
        assert!(read_opt(&porto.join(n)).unwrap() == bytes[i], "{n}: the same bytes on the other volume");
        let copy = identity(&porto.join(n));
        assert_ne!(copy, sources[i], "{n}: a copy, not a rename");
        copies.push(copy);
        assert!(!exists(&lisbon.join(n)), "{n}: the source went to the trash");
        assert_eq!(l.where_is(ids[i]), porto.join(n), "{n}: the same photo, now there");
        let moved = entry(&porto, n).unwrap();
        for key in ["star", "caption", "tags", "edits", "albums"] {
            assert_eq!(moved.get(key), entries[i].get(key), "{n}: its {key} went with it");
        }
        assert_eq!(entry(&lisbon, n), None);
    }
    assert!(l.s.library().photo(ids[0]).unwrap().star);
    assert_eq!(l.s.library().info(ids[1]).caption, "Praça");
    assert_eq!(l.s.library().album_members(&album), order, "the album's order");

    // Undo: back across the volumes, every byte, entry and the library as they were.
    for _ in &ids {
        let e = l.run("edit.undo", json!({}));
        clean(&e);
        assert_eq!((e.direction, e.done), (Direction::Undo, 1));
    }
    for (i, n) in names_moved.iter().enumerate() {
        assert!(read_opt(&lisbon.join(n)).unwrap() == bytes[i], "{n}: back");
        assert!(!exists(&porto.join(n)), "{n}: gone from the other volume");
        assert_eq!(entry(&lisbon, n).unwrap().get("tags"), entries[i].get("tags"), "{n}");
    }
    assert_eq!(l.s.library().model(), &model, "the library as it was");

    // The trash as it was: the first sources, and the copies undo sent there.
    let mut trashed: Vec<(PathBuf, Identity, bool)> = names_moved.iter().zip(&sources).map(|(n, id)| (lisbon.join(n), *id, false)).collect();
    trashed.extend(names_moved.iter().zip(&copies).map(|(n, id)| (porto.join(n), *id, false)));
    untrash(&trashed);
    assert!(read_opt(&lisbon.join("IMG_0001.jpg")).unwrap() == bytes[0]);
}

/// A folder moved to the other volume is copied file by file, each verified, then the source tree
/// goes to the trash; the folder and its photos keep their ids and data; undo brings it back.
#[test]
fn a_folder_moved_to_another_volume_is_copied_verified_then_trashed() {
    let _faults = exclusive();
    let mut l = Lib::new();
    let (lisbon, porto) = (l.lisbon(), l.porto());
    let day2 = lisbon.join("Day2");
    let (lisbon_id, day2_id, porto_id) = (l.folder(&lisbon), l.folder(&day2), l.folder(&porto));
    let (one, day) = (l.photo(&lisbon, "IMG_0001.jpg"), l.photo(&day2, "DAY_0001.jpg"));
    l.s.execute("photo.star.set", &json!({ "ids": [one, day], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [day], "caption": "Day two" })).unwrap();
    assert!(l.s.flush_metadata(PATIENCE));
    l.s.flush().unwrap();
    let tree = names(&lisbon);
    let files: Vec<(String, Vec<u8>)> = ["IMG_0001.jpg", "IMG_0002.jpg", HARD, "notes.txt", "Day2/DAY_0001.jpg"]
        .iter()
        .map(|n| (n.to_string(), read_opt(&n.split('/').fold(lisbon.clone(), |p, c| p.join(c))).unwrap()))
        .collect();
    let source = identity(&lisbon);
    let model = l.s.library().model().clone();

    let e = l.run("folder.move", json!({ "folder": lisbon_id, "to": porto_id }));
    clean(&e);
    assert_eq!((e.kind, e.done), (FileOpKind::MoveFolder, 1));
    let moved = porto.join("Lisbon");
    assert!(!exists(&lisbon), "the source tree went to the trash");
    assert_eq!(names(&moved), tree);
    for (n, b) in &files {
        assert!(read_opt(&n.split('/').fold(moved.clone(), |p, c| p.join(c))).unwrap() == *b, "{n}: the same bytes on the other volume");
    }
    let copy = identity(&moved);
    let lib = l.s.library();
    assert_eq!((lib.folder_by_path(&moved), lib.folder_by_path(&moved.join("Day2"))), (Some(lisbon_id), Some(day2_id)), "the folders keep their ids");
    assert_eq!((l.where_is(one), l.where_is(day)), (moved.join("IMG_0001.jpg"), moved.join("Day2").join("DAY_0001.jpg")));
    assert!(lib.photo(one).unwrap().star && lib.photo(day).unwrap().star);
    assert_eq!(entry(&moved.join("Day2"), "DAY_0001.jpg").unwrap()["caption"], "Day two", "the sidecars went with it");

    let e = l.run("edit.undo", json!({}));
    clean(&e);
    assert_eq!(names(&lisbon), tree, "back");
    for (n, b) in &files {
        assert!(read_opt(&n.split('/').fold(lisbon.clone(), |p, c| p.join(c))).unwrap() == *b, "{n}: back");
    }
    assert!(!exists(&moved));
    assert_eq!(l.s.library().model(), &model, "the library as it was");
    untrash(&[(lisbon.clone(), source, true), (moved, copy, true)]);
    assert_eq!(names(&lisbon), tree);
}
