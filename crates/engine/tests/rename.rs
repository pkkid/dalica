//! Renaming photos and folders (`roadmap.md` M9.14; `organizing.md` §9, `data-model.md` §8) on
//! temporary folders: three photos renamed `Lisbon` become `Lisbon-001.jpg` … `Lisbon-003.jpg` in
//! light-table order with their data (their sidecar entries travel, unknown keys and album
//! memberships included); the
//! date taken and the size join the name when asked; a clash with a file on disk takes the next
//! number; a change of letter case renames in place (and, on a volume that keeps case apart, a
//! name another photo holds in the other case takes a number); a failure planted on the second
//! file leaves all three as they were; undo and redo; a renamed folder keeps its ids and sidecar,
//! and a renamed watched folder its `folders.json` entry (through `Driver::followed`);
//! names with `]`, `"`, `;`, `=`, non-ASCII and 255 bytes round-trip (`data-model.md` §10.7); each
//! command's contract and graceful failures; the owner thread makes no `fs` call.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::{FaultLock, entry, files, jpeg, off_owner, options, read, read_json, shared, virtual_clock};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::cmd::rename::planted;
use dalica_engine::watching::WatchMode;
use dalica_engine::{CmdError, Direction, FileOpEvent, FileOpKind, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// Whether the temporary folders' volume ignores letter case (the macOS and Windows runners).
fn ignores_case(dir: &Path) -> bool {
    let dir = dir.to_path_buf();
    off_owner(move || {
        std::fs::write(dir.join("case-probe"), b"x").unwrap();
        let ignores = dir.join("CASE-PROBE").exists();
        std::fs::remove_file(dir.join("case-probe")).unwrap();
        ignores
    })
}

/// Every photo of the library, by id.
fn photos(s: &Session) -> Vec<dalica_library::Photo> {
    s.library().photos().cloned().collect()
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    _faults: FaultLock,
}

impl Lib {
    /// `Pictures/<folder>` with each folder's photos (written in order), watched and scanned, on a
    /// virtual clock, the watcher off. `before_open` runs on the `Pictures` folder first.
    fn with(folders: &[(&str, &[&str])], before_open: impl FnOnce(&Path)) -> Lib {
        let _faults = shared();
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Pictures");
        let mut seed = 0;
        for (folder, names) in folders {
            let path = folder.split('/').fold(pictures.clone(), |p, part| p.join(part));
            dalica_fs::create_dir_all(&path).unwrap();
            for name in *names {
                dalica_fs::replace(&path.join(name), &jpeg(seed), Expect::Absent).unwrap();
                seed += 1;
            }
        }
        before_open(&pictures);
        let library = dir.path().join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        let mut s = Session::open_with(&library, virtual_clock(), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": pictures })).unwrap();
        s.wait_idle();
        Lib { dir, s, _faults }
    }

    /// `Pictures/Lisbon` with `IMG_0001.jpg` … `IMG_000n.jpg`.
    fn new(n: usize) -> Lib {
        let names: Vec<String> = (1..=n).map(|i| format!("IMG_{i:04}.jpg")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        Lib::with(&[("Lisbon", &names)], |_| {})
    }

    /// `Pictures/<name>`, `/` separating levels.
    fn folder(&self, name: &str) -> PathBuf {
        name.split('/').fold(self.dir.path().join("Pictures"), |p, part| p.join(part))
    }

    fn folder_id(&self, name: &str) -> FolderId {
        self.s.library().folder_by_path(&self.folder(name)).unwrap()
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        let f = self.folder_id(folder);
        self.s.library().photo_by_name(f, name).unwrap_or_else(|| panic!("no photo {name}")).id
    }

    fn name(&self, id: PhotoId) -> String {
        self.s.library().photo(id).unwrap().name.clone()
    }

    /// Runs `id` with `p` and waits for its file job; the reply.
    fn run(&mut self, id: &str, p: Value) -> Value {
        let reply = self.s.execute(id, &p).unwrap();
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        self.s.flush().unwrap();
        reply
    }

    fn last_event(&self) -> FileOpEvent {
        self.s.file_op_events().last().cloned().unwrap()
    }

    fn undo(&mut self) {
        self.s.execute("edit.undo", &json!({})).unwrap();
        assert!(self.s.wait_file_jobs(PATIENCE));
        self.s.flush().unwrap();
    }

    fn redo(&mut self) {
        self.s.execute("edit.redo", &json!({})).unwrap();
        assert!(self.s.wait_file_jobs(PATIENCE));
        self.s.flush().unwrap();
    }

    /// The operations left in `file-ops.json`.
    fn journal_ops(&self) -> usize {
        read_json(&self.dir.path().join("library").join("file-ops.json")).map_or(0, |v| v["ops"].as_array().unwrap().len())
    }
}

/// Three photos renamed `Lisbon`, named in an order of our own, become `Lisbon-001.jpg` …
/// `Lisbon-003.jpg` in light-table order, keeping their ids, their data and their sidecar entries
/// (unknown keys too); the operation is one undo step whose undo and redo move the files and the
/// entries back and forth; the journal is empty after each; the owner thread makes no `fs` call.
#[test]
fn three_photos_renamed_lisbon_are_numbered_in_order_with_their_data() {
    let mut l = Lib::new(3);
    let lisbon = l.folder("Lisbon");
    let ids: Vec<PhotoId> = (1..=3).map(|i| l.photo("Lisbon", &format!("IMG_{i:04}.jpg"))).collect();
    l.s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [ids[2]], "caption": "Tram 28" })).unwrap();
    l.s.execute("photo.tags.add", &json!({ "ids": [ids[0], ids[2]], "tags": "tram" })).unwrap();
    let album = l.s.execute("album.create", &json!({ "name": "Trams", "ids": [ids[2], ids[0]] })).unwrap()["album"].clone();
    l.s.flush().unwrap();
    let in_album = |l: &Lib| {
        let entry3 = entry(&lisbon, &l.name(ids[2])).unwrap();
        let album_id = album.as_str().unwrap();
        (l.s.library().album_members(&serde_json::from_value(album.clone()).unwrap()).to_vec(), entry3["albums"].get(album_id).is_some())
    };
    assert_eq!(in_album(&l), (vec![ids[2], ids[0]], true), "in the album, its sidecar entry says so");
    {
        let lisbon = lisbon.clone();
        off_owner(move || {
            let path = lisbon.join(".dalica.json");
            let mut root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            root["photos"]["IMG_0001.jpg"]["x-other"] = json!({ "rating": 5 });
            std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
        });
    }
    let before = photos(&l.s);
    let steps = l.s.undo_stack().len();
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();

    let reply = l.run("photo.rename", json!({ "ids": [ids[2], ids[0], ids[1]], "name": "  Lisbon " }));
    assert_eq!((reply["pending"].clone(), reply["total"].clone(), reply["changed"].clone()), (json!(true), json!(3), json!(3)));
    let e = l.last_event();
    assert_eq!((e.kind, e.direction, e.done, e.total), (FileOpKind::Rename, Direction::Do, 3, 3));
    assert!(e.failures.is_empty() && e.skipped.is_empty(), "{e:?}");
    assert_eq!(e.destination.as_deref(), Some(lisbon.as_path()));
    assert_eq!(files(&lisbon), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"]);
    let names: Vec<String> = ids.iter().map(|id| l.name(*id)).collect();
    assert_eq!(names, ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"], "in light-table order, ids kept");
    assert_eq!(read(&lisbon.join("Lisbon-001.jpg")), jpeg(0), "the files themselves");
    let lib = l.s.library();
    assert!(lib.photo(ids[1]).unwrap().star);
    assert_eq!(lib.info(ids[2]).caption, "Tram 28");
    assert_eq!(lib.info(ids[0]).tags, ["tram"]);
    assert_eq!(l.s.undo_stack().len(), steps + 1, "one step");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Rename"));
    // The entries travelled under the new names.
    assert_eq!(entry(&lisbon, "Lisbon-001.jpg").unwrap()["x-other"], json!({ "rating": 5 }));
    assert_eq!(entry(&lisbon, "Lisbon-002.jpg").unwrap()["star"], true);
    assert_eq!(entry(&lisbon, "Lisbon-003.jpg").unwrap()["caption"], "Tram 28");
    for old in ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"] {
        assert_eq!(entry(&lisbon, old), None, "{old}");
    }
    assert_eq!(l.journal_ops(), 0, "the operation left the journal");
    assert_eq!(in_album(&l), (vec![ids[2], ids[0]], true), "the album memberships travelled");
    let renamed = photos(&l.s);

    // Undo: the names, the entries and the model as they were.
    l.undo();
    assert_eq!(files(&lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(l.last_event().direction, Direction::Undo);
    assert_eq!(photos(&l.s), before);
    assert_eq!(entry(&lisbon, "IMG_0001.jpg").unwrap()["x-other"], json!({ "rating": 5 }));
    assert_eq!(entry(&lisbon, "IMG_0002.jpg").unwrap()["star"], true);
    assert_eq!(entry(&lisbon, "Lisbon-001.jpg"), None);
    assert_eq!(in_album(&l), (vec![ids[2], ids[0]], true));
    assert_eq!(l.s.undo_stack().len(), steps);
    assert_eq!(l.s.undo_stack().redo_label(), Some("Rename"));

    // Redo: the same names again.
    l.redo();
    assert_eq!(files(&lisbon), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"]);
    assert_eq!(photos(&l.s), renamed);
    assert_eq!(entry(&lisbon, "Lisbon-003.jpg").unwrap()["caption"], "Tram 28");
    assert_eq!(l.journal_ops(), 0);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
}

/// "Add the date taken" and "Add the image size" join the name: the date the photo sorts by and
/// its pixel size; one photo gets no number.
#[test]
fn the_date_taken_and_the_size_join_the_name() {
    let mut l = Lib::new(2);
    let (one, two) = (l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg"));
    l.s.execute("photo.dates.set", &json!({ "ids": [one, two], "set": "2026-06-04T10:00:00" })).unwrap();
    l.run("photo.rename", json!({ "ids": [one, two], "name": "Lisbon", "addDate": true, "addSize": true }));
    assert_eq!(files(&l.folder("Lisbon")), ["Lisbon-2026-06-04-96x64-001.jpg", "Lisbon-2026-06-04-96x64-002.jpg"]);
    l.run("photo.rename", json!({ "ids": [two], "name": "Tram", "addDate": true }));
    assert_eq!(l.name(two), "Tram-2026-06-04.jpg");
    l.run("photo.rename", json!({ "ids": [one], "name": "Tram", "addSize": true }));
    assert_eq!(l.name(one), "Tram-96x64.jpg");
}

/// A file on disk the library does not know holds a name: that photo and those after it take the
/// next free numbers, and the file is left as it was.
#[test]
fn a_clash_with_an_existing_file_takes_the_next_number() {
    let mut l = Lib::new(3);
    let lisbon = l.folder("Lisbon");
    let other = lisbon.join("Lisbon-002.jpg");
    {
        let other = other.clone();
        off_owner(move || std::fs::write(other, b"not a photo of this library").unwrap());
    }
    let ids: Vec<PhotoId> = (1..=3).map(|i| l.photo("Lisbon", &format!("IMG_{i:04}.jpg"))).collect();
    l.run("photo.rename", json!({ "ids": ids, "name": "Lisbon" }));
    assert!(l.last_event().failures.is_empty(), "{:?}", l.last_event());
    let names: Vec<String> = ids.iter().map(|id| l.name(*id)).collect();
    assert_eq!(names, ["Lisbon-001.jpg", "Lisbon-003.jpg", "Lisbon-004.jpg"]);
    assert_eq!(files(&lisbon), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg", "Lisbon-004.jpg"]);
    assert_eq!(read(&other), b"not a photo of this library");
    // One photo onto a name another photo of the library holds.
    l.run("photo.rename", json!({ "ids": [ids[0]], "name": "Lisbon-003" }));
    assert_eq!(l.name(ids[0]), "Lisbon-003-001.jpg");
    l.undo();
    assert_eq!(l.name(ids[0]), "Lisbon-001.jpg");
}

/// `img.jpg` renamed `IMG` becomes `IMG.jpg` in place, keeping its data, on any volume (on one that
/// ignores case, the taken name is the photo itself, decided by file identity); undo puts the old
/// case back. On a volume that keeps case apart, a name another photo holds in the other case
/// takes a number.
#[test]
fn a_change_of_case_renames_in_place() {
    let mut l = Lib::with(&[("Lisbon", &["img.jpg"])], |_| {});
    let lisbon = l.folder("Lisbon");
    let id = l.photo("Lisbon", "img.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    l.run("photo.rename", json!({ "ids": [id], "name": "IMG" }));
    assert!(l.last_event().failures.is_empty(), "{:?}", l.last_event());
    assert_eq!(files(&lisbon), ["IMG.jpg"]);
    assert_eq!(l.name(id), "IMG.jpg");
    assert_eq!(entry(&lisbon, "IMG.jpg").unwrap()["star"], true);
    l.undo();
    assert_eq!(files(&lisbon), ["img.jpg"]);
    assert_eq!(l.name(id), "img.jpg");
    assert_eq!(entry(&lisbon, "img.jpg").unwrap()["star"], true);

    if ignores_case(&lisbon) {
        return;
    }
    let mut l = Lib::with(&[("Lisbon", &["img.jpg", "IMG.jpg"])], |_| {});
    let id = l.photo("Lisbon", "img.jpg");
    l.run("photo.rename", json!({ "ids": [id], "name": "IMG" }));
    assert_eq!(l.name(id), "IMG-001.jpg");
    assert_eq!(files(&l.folder("Lisbon")), ["IMG-001.jpg", "IMG.jpg"]);
}

/// A failure planted on the second file's rename leaves all three photos as they were: the first
/// is moved back, the model and the entries are untouched, and no undo step is recorded.
#[test]
fn a_failure_on_the_second_file_leaves_all_three_as_they_were() {
    let mut l = Lib::new(3);
    let lisbon = l.folder("Lisbon");
    let ids: Vec<PhotoId> = (1..=3).map(|i| l.photo("Lisbon", &format!("IMG_{i:04}.jpg"))).collect();
    l.s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    l.s.flush().unwrap();
    let before = photos(&l.s);
    let steps = l.s.undo_stack().len();
    planted::fail_rename_of(&lisbon.join("IMG_0002.jpg"));
    l.run("photo.rename", json!({ "ids": ids, "name": "Lisbon" }));
    let e = l.last_event();
    assert_eq!((e.done, e.total, e.failures.len()), (0, 3, 3), "{e:?}");
    assert!(e.failures[0].reason.contains("injected fault"), "{e:?}");
    assert_eq!(files(&lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(photos(&l.s), before);
    assert_eq!(entry(&lisbon, "IMG_0001.jpg").unwrap()["star"], true);
    assert_eq!(l.s.undo_stack().len(), steps, "no step");
    assert_eq!(l.journal_ops(), 0);
    // The same rename runs once nothing is planted.
    l.run("photo.rename", json!({ "ids": ids, "name": "Lisbon" }));
    assert_eq!(files(&lisbon), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"]);
}

/// Photos of two folders: each folder's photos are renamed together, numbered across the batch in
/// light-table order (the newest folder first), and one undo puts both back.
#[test]
fn a_batch_over_two_folders_is_one_step() {
    let mut l = Lib::with(&[("Lisbon", &["a.jpg", "b.jpg"]), ("Porto", &["c.jpg"])], |_| {});
    let (a, b, c) = (l.photo("Lisbon", "a.jpg"), l.photo("Lisbon", "b.jpg"), l.photo("Porto", "c.jpg"));
    l.s.execute("photo.dates.set", &json!({ "ids": [a, b], "set": "2024-01-01T10:00:00" })).unwrap();
    l.s.execute("photo.dates.set", &json!({ "ids": [c], "set": "2025-01-01T10:00:00" })).unwrap();
    let steps = l.s.undo_stack().len();
    l.run("photo.rename", json!({ "ids": [a, b, c], "name": "Trip" }));
    assert_eq!(l.last_event().destination, None, "two folders");
    assert_eq!((l.name(c), l.name(a), l.name(b)), ("Trip-001.jpg".into(), "Trip-002.jpg".into(), "Trip-003.jpg".into()));
    assert_eq!(files(&l.folder("Porto")), ["Trip-001.jpg"]);
    assert_eq!(l.s.undo_stack().len(), steps + 1);
    l.undo();
    assert_eq!(files(&l.folder("Lisbon")), ["a.jpg", "b.jpg"]);
    assert_eq!(files(&l.folder("Porto")), ["c.jpg"]);
}

/// A renamed folder keeps its id, its photos' ids and its sidecar (the data inside it), the
/// folders under it follow, and undo and redo move it back and forth; a name already on disk
/// fails with nothing changed.
#[test]
fn a_renamed_folder_keeps_its_ids_and_sidecar() {
    let mut l = Lib::with(&[("Lisbon", &["a.jpg"]), ("Lisbon/Alfama", &["b.jpg"])], |_| {});
    let (folder, sub) = (l.folder_id("Lisbon"), l.folder_id("Lisbon/Alfama"));
    let (a, b) = (l.photo("Lisbon", "a.jpg"), l.photo("Lisbon/Alfama", "b.jpg"));
    l.s.execute("photo.star.set", &json!({ "ids": [a, b], "on": true })).unwrap();
    l.s.flush().unwrap();
    let steps = l.s.undo_stack().len();
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    let reply = l.run("folder.rename", json!({ "folder": folder, "name": "Lisboa" }));
    assert_eq!(reply["path"], json!(l.folder("Lisboa")));
    let e = l.last_event();
    assert_eq!((e.kind, e.done, e.failures.len()), (FileOpKind::RenameFolder, 1, 0), "{e:?}");
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
    let lib = l.s.library();
    assert_eq!(lib.folder(folder).unwrap().path, l.folder("Lisboa"));
    assert_eq!(lib.folder(sub).unwrap().path, l.folder("Lisboa").join("Alfama"));
    assert_eq!((lib.photo(a).unwrap().folder, lib.photo(b).unwrap().folder), (folder, sub));
    assert!(lib.photo(a).unwrap().star && lib.photo(b).unwrap().star);
    assert_eq!(entry(&l.folder("Lisboa"), "a.jpg").unwrap()["star"], true);
    assert_eq!(entry(&l.folder("Lisboa").join("Alfama"), "b.jpg").unwrap()["star"], true);
    assert_eq!(l.s.undo_stack().undo_label(), Some("Rename Folder"));
    assert_eq!(l.s.undo_stack().len(), steps + 1);
    l.undo();
    assert_eq!(l.s.library().folder(folder).unwrap().path, l.folder("Lisbon"));
    assert_eq!(files(&l.folder("Lisbon")), ["Alfama", "a.jpg"]);
    l.redo();
    assert_eq!(l.s.library().folder(folder).unwrap().path, l.folder("Lisboa"));
    assert_eq!(l.journal_ops(), 0);

    // Onto a folder that is there (not in the library): fails, nothing changes.
    let taken = l.folder("Porto");
    {
        let taken = taken.clone();
        off_owner(move || std::fs::create_dir(taken).unwrap());
    }
    let steps = l.s.undo_stack().len();
    l.run("folder.rename", json!({ "folder": folder, "name": "Porto" }));
    let e = l.last_event();
    assert_eq!((e.done, e.failures.len()), (0, 1), "{e:?}");
    assert_eq!(l.s.library().folder(folder).unwrap().path, l.folder("Lisboa"));
    assert_eq!(files(&taken), Vec::<String>::new());
    assert_eq!(l.s.undo_stack().len(), steps);
}

/// Names that are hard to write (`data-model.md` §10.7): photos named with `]`, `"` (not on
/// Windows), `;`, `=` and non-ASCII are renamed and get their names back exactly; a photo and a
/// folder take such names, and a 255-byte one.
#[test]
fn hard_names_round_trip() {
    let mut hard = vec!["a]b.jpg", "c;d=e.jpg", "Praça à noite.jpg"];
    if !cfg!(windows) {
        hard.push("q\"uote.jpg");
    }
    let mut l = Lib::with(&[("Lisbon", &hard)], |_| {});
    let lisbon = l.folder("Lisbon");
    let ids: Vec<PhotoId> = hard.iter().map(|n| l.photo("Lisbon", n)).collect();
    l.run("photo.rename", json!({ "ids": ids, "name": "Lisbon" }));
    assert_eq!(files(&lisbon).len(), hard.len());
    assert!(files(&lisbon).iter().all(|n| n.starts_with("Lisbon-00")));
    l.undo();
    let mut want: Vec<String> = hard.iter().map(|n| n.to_string()).collect();
    want.sort();
    assert_eq!(files(&lisbon), want);

    // Into such a name, and back.
    l.run("photo.rename", json!({ "ids": [ids[0]], "name": "Tram ]28; a=b ç 東京" }));
    assert_eq!(l.name(ids[0]), "Tram ]28; a=b ç 東京.jpg");
    assert!(files(&lisbon).contains(&"Tram ]28; a=b ç 東京.jpg".to_string()));
    // 255 bytes with its extension (few UTF-16 units, so the path stays short on Windows).
    let long = format!("{}xx", "長".repeat(83));
    l.run("photo.rename", json!({ "ids": [ids[1]], "name": long }));
    let name = l.name(ids[1]);
    assert_eq!((name.len(), name.as_str()), (255, format!("{long}.jpg").as_str()));
    assert!(files(&lisbon).contains(&name));
    l.undo();
    l.undo();
    assert_eq!(files(&lisbon), want);

    // A folder.
    let folder = l.folder_id("Lisbon");
    l.run("folder.rename", json!({ "folder": folder, "name": "Praça ]; a=b" }));
    assert_eq!(l.s.library().folder(folder).unwrap().path, l.folder("Praça ]; a=b"));
    l.undo();
    assert_eq!(l.s.library().folder(folder).unwrap().path, lisbon);
    assert_eq!(files(&lisbon), want);
}

/// The commands' contract: names are sanitized, an unchanged name starts nothing, and bad, empty,
/// unknown or oversized input is refused with an error (never a panic) and changes nothing.
#[test]
fn contract_and_graceful_failures() {
    let mut l = Lib::new(2);
    let lisbon = l.folder("Lisbon");
    let (one, two) = (l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg"));
    let folder = l.folder_id("Lisbon");
    let ids = dalica_engine::cmd::specs().iter().map(|s| s.id).collect::<Vec<_>>();
    assert!(ids.contains(&"photo.rename") && ids.contains(&"folder.rename"));

    // Sanitized: separators and characters Windows refuses go, leading dots and dashes too.
    l.run("photo.rename", json!({ "ids": [one], "name": "../-Tram: 28?/" }));
    assert_eq!(l.name(one), "Tram 28.jpg");
    // Unchanged: no job, no step.
    let steps = l.s.undo_stack().len();
    assert_eq!(l.s.execute("photo.rename", &json!({ "ids": [one], "name": "Tram 28" })).unwrap()["job"], Value::Null);
    assert_eq!(l.s.execute("folder.rename", &json!({ "folder": folder, "name": "Lisbon" })).unwrap()["job"], Value::Null);
    assert_eq!(l.s.undo_stack().len(), steps);

    let model = l.s.library().model().clone();
    let bad_params = [
        json!({}),
        json!({ "ids": [one] }),
        json!({ "name": "x" }),
        json!({ "ids": [one], "name": 5 }),
        json!({ "ids": "x", "name": "x" }),
        json!({ "ids": [one], "name": "x", "addDate": "yes" }),
        json!({ "ids": [one], "name": "x", "zzz": 1 }),
        json!({ "ids": [one], "name": "   " }),
        json!([]),
        json!("text"),
    ];
    for p in &bad_params {
        assert!(matches!(l.s.execute("photo.rename", p), Err(CmdError::BadParams(_))), "{p}");
    }
    assert!(matches!(l.s.execute("photo.rename", &json!({ "ids": [999_999], "name": "x" })), Err(CmdError::NotFound(_))));
    assert!(matches!(l.s.execute("photo.rename", &json!({ "ids": [], "name": "x" })), Err(CmdError::Disabled(_))));
    assert!(matches!(l.s.execute("photo.rename", &json!({ "ids": [two], "name": "x".repeat(5000) })), Err(CmdError::Limit(_))));
    for p in [
        json!({}),
        json!({ "folder": folder }),
        json!({ "name": "x" }),
        json!({ "folder": "x", "name": "x" }),
        json!({ "folder": folder, "name": "" }),
    ] {
        assert!(matches!(l.s.execute("folder.rename", &p), Err(CmdError::BadParams(_))), "{p}");
    }
    assert!(matches!(l.s.execute("folder.rename", &json!({ "folder": 999_999, "name": "x" })), Err(CmdError::NotFound(_))));
    assert!(matches!(l.s.execute("folder.rename", &json!({ "folder": folder, "name": "x".repeat(5000) })), Err(CmdError::Limit(_))));
    assert!(l.s.wait_file_jobs(PATIENCE));
    assert_eq!(l.s.library().model(), &model, "nothing changed");
    assert_eq!(files(&lisbon), ["IMG_0002.jpg", "Tram 28.jpg"]);

    // Hostile parameters of every shape.
    for p in
        [json!(42), json!(null), json!({ "ids": [-1], "name": "x" }), json!({ "ids": [1.5], "name": "x" }), json!({ "ids": [one], "name": null })]
    {
        for id in ["photo.rename", "folder.rename"] {
            let r = l.s.execute(id, &p);
            assert!(r.is_err() && !matches!(r, Err(CmdError::Internal(_))), "{id} {p}: {r:?}");
        }
    }
    assert_eq!(l.s.library().model(), &model);
}

/// A folder renamed onto the name of another library folder beside it is refused before anything
/// moves.
#[test]
fn a_folder_is_not_renamed_onto_another_library_folder() {
    let mut l = Lib::with(&[("Lisbon", &["a.jpg"]), ("Porto", &["b.jpg"])], |_| {});
    let folder = l.folder_id("Lisbon");
    let r = l.s.execute("folder.rename", &json!({ "folder": folder, "name": "Porto" }));
    assert!(matches!(&r, Err(CmdError::Disabled(m)) if m.contains("already in the library")), "{r:?}");
    assert!(l.s.wait_file_jobs(PATIENCE));
    assert_eq!(files(&l.folder("Lisbon")), ["a.jpg"]);
}

/// The watched-folder entries of a renamed folder follow it (`folders.json`, through
/// `Driver::followed`), with their rules, on doing, undoing and redoing.
#[test]
fn a_renamed_watched_folder_keeps_its_folders_json_entry() {
    let mut l = Lib::with(&[("Lisbon", &["a.jpg"]), ("Lisbon/Alfama", &["b.jpg"])], |_| {});
    // Watched on its own too, and a subfolder that only scans once.
    l.s.execute("folders.add", &json!({ "path": l.folder("Lisbon") })).unwrap();
    l.s.execute("folders.set", &json!({ "changes": [{ "path": l.folder("Lisbon/Alfama"), "mode": "once" }] })).unwrap();
    l.s.wait_idle();
    l.s.flush().unwrap();
    let library = l.dir.path().join("library");
    let entries = |l: &Lib| -> Vec<(PathBuf, Option<String>)> {
        let v = read_json(&library.join("folders.json")).unwrap();
        let mut out: Vec<(PathBuf, Option<String>)> = v["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (PathBuf::from(e["path"].as_str().unwrap()), e.get("mode").and_then(Value::as_str).map(str::to_string)))
            .collect();
        out.sort();
        let _ = l;
        out
    };
    let pictures = l.dir.path().join("Pictures");
    let before = entries(&l);
    assert!(before.iter().any(|(p, _)| *p == l.folder("Lisbon")), "{before:?}");
    let folder = l.folder_id("Lisbon");
    l.run("folder.rename", json!({ "folder": folder, "name": "Lisboa" }));
    assert_eq!(l.last_event().done, 1, "{:?}", l.last_event());
    let renamed = entries(&l);
    let expect: Vec<(PathBuf, Option<String>)> = before
        .iter()
        .map(|(p, m)| match p.strip_prefix(l.folder("Lisbon")) {
            Ok(rest) if rest.as_os_str().is_empty() => (l.folder("Lisboa"), m.clone()),
            Ok(rest) => (l.folder("Lisboa").join(rest), m.clone()),
            Err(_) => (p.clone(), m.clone()),
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(renamed, expect);
    assert!(renamed.iter().any(|(p, m)| *p == l.folder("Lisboa").join("Alfama") && m.as_deref() == Some("once")), "{renamed:?}");
    assert!(renamed.iter().any(|(p, _)| *p == pictures), "the others kept");
    l.undo();
    assert_eq!(entries(&l), before);
    l.redo();
    assert_eq!(entries(&l), renamed);
}
