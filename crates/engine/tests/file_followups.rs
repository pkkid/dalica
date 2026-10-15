//! The file-operation follow-ups of M9 (`roadmap.md` M10.32; `data-model.md` §6, §8, §10.2):
//!
//! - undoing a copy of a saved photo sends the copy, the kept original it brought and that
//!   original's record entry away, so the destination is as it was but for the copy's stale sidecar
//!   entry (`data-model.md` §7); redo brings them back;
//! - a folder moved outside Dalica from one Scan Always folder into another, found by the
//!   verification pass (one region scan per folder), keeps its id, its photos' ids, what was said
//!   about them and their album places.
//!
//! The 255-byte name that goes to the trash and comes back is `dalica-fs`'s `trash.rs`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::{entry, jpeg, names, off_owner, options, read_opt, shared, under, virtual_clock, wait_files};
use dalica_core::{AlbumId, FolderId, PhotoId};
use dalica_engine::watching::WatchMode;
use dalica_engine::{JobId, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// A library whose watched folders are `roots` (each Scan Always) under a temporary folder, with
/// `files` (paths under it) written as photos and scanned; the watcher off.
fn library(roots: &[&str], files: &[&str]) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    for (i, rel) in files.iter().enumerate() {
        let path = under(dir.path(), rel);
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        dalica_fs::replace(&path, &jpeg(i as u64 + 1), Expect::Absent).unwrap();
    }
    let library = dir.path().join("library");
    dalica_fs::create_dir_all(&library).unwrap();
    let mut s = Session::open_with(&library, virtual_clock(), &options()).unwrap();
    s.set_watching(WatchMode::Off);
    let changes: Vec<Value> = roots.iter().map(|r| json!({ "path": under(dir.path(), r), "mode": "always" })).collect();
    s.execute("folders.set", &json!({ "changes": changes })).unwrap();
    s.wait_idle();
    (dir, s)
}

fn folder(s: &Session, path: &Path) -> FolderId {
    s.library().folder_by_path(path).unwrap_or_else(|| panic!("no folder {}", path.display()))
}

fn photo(s: &Session, folder_path: &Path, name: &str) -> PhotoId {
    s.library().photo_by_name(folder(s, folder_path), name).unwrap_or_else(|| panic!("no photo {name}")).id
}

/// Runs `id` and waits for its file job and what it wrote; the operation must be clean.
fn run(s: &mut Session, id: &str, p: Value) {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e:?}"));
    wait_files(s);
    s.wait_idle();
    s.flush().unwrap();
    let e = s.file_op_events().last().cloned().unwrap();
    assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{id}: {e:?}");
}

/// Every name under `folder` (one level into its subfolders), sorted (read off the owner thread).
fn tree(folder: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in names(folder) {
        let inside = names(&folder.join(&name));
        out.push(name.clone());
        out.extend(inside.into_iter().map(|n| format!("{name}/{n}")));
    }
    out
}

/// `data-model.md` §6 and §8: a copy of a saved photo brings its kept original and record entry;
/// undoing the copy sends them to the trash with it, and the destination's folder holds what it held
/// before (its sidecar aside, where the copy's entry is stale); redo brings them back; the source
/// keeps its own.
#[test]
fn undoing_a_copy_of_a_saved_photo_leaves_nothing_behind() {
    let _faults = shared();
    let (dir, mut s) = library(&["Pictures"], &["Pictures/Lisbon/IMG_0001.jpg", "Pictures/Porto/PORTO_0001.jpg"]);
    let (lisbon, porto) = (under(dir.path(), "Pictures/Lisbon"), under(dir.path(), "Pictures/Porto"));
    let one = photo(&s, &lisbon, "IMG_0001.jpg");
    s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": one, "op": "autofix()" })).unwrap();
    let reply = s.execute("photo.save", &json!({ "ids": [one], "confirm": true })).unwrap();
    let job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    assert!(s.wait_job(job, PATIENCE).unwrap().state.is_finished());
    s.wait_idle();
    s.flush().unwrap();
    let kept = lisbon.join(".dalica-originals").join("IMG_0001.jpg");
    let original = read_opt(&kept).expect("the save kept the original");
    let porto_before = tree(&porto);
    assert!(!porto_before.iter().any(|n| n.starts_with(".dalica-originals")), "{porto_before:?}");

    let porto_id = folder(&s, &porto);
    run(&mut s, "photo.copyTo", json!({ "ids": [one], "folder": porto_id }));
    let copy_kept = porto.join(".dalica-originals").join("IMG_0001.jpg");
    assert_eq!(read_opt(&copy_kept), Some(original.clone()), "the copy brought the kept original");
    let record =
        |folder: &Path| read_opt(&folder.join(".dalica-originals").join(".dalica.json")).map(|b| serde_json::from_slice::<Value>(&b).unwrap());
    assert!(record(&porto).unwrap()["originals"].get("IMG_0001.jpg").is_some());
    assert!(entry(&porto, "IMG_0001.jpg").is_some());

    run(&mut s, "edit.undo", json!({}));
    let sidecar = |n: &String| n != ".dalica.json";
    let after: Vec<String> = tree(&porto).into_iter().filter(sidecar).collect();
    assert_eq!(after, porto_before.into_iter().filter(sidecar).collect::<Vec<_>>(), "the destination holds what it held before");
    // The copy's sidecar entry is stale, as a trashed photo's (`data-model.md` §7): pruned after
    // 30 days, so a copy put back from the trash meanwhile gets its data back.
    let doc = crate::common::read_json(&porto.join(".dalica.json")).unwrap();
    assert!(doc["dalica"]["missing"].get("IMG_0001.jpg").is_some(), "{doc}");
    assert_eq!(read_opt(&kept), Some(original.clone()), "the source keeps its original");
    assert!(record(&lisbon).unwrap()["originals"].get("IMG_0001.jpg").is_some(), "and its record");

    run(&mut s, "edit.redo", json!({}));
    assert_eq!(read_opt(&copy_kept), Some(original), "redo brings the kept original back");
    assert!(record(&porto).unwrap()["originals"].get("IMG_0001.jpg").is_some());
}

/// `data-model.md` §10.2 across two Scan Always folders: the verification pass scans each on its
/// own, and a folder moved from one into the other outside Dalica (either way, so either scan may
/// end first) is still one folder that moved, not one gone and one new: it keeps its id, its
/// photos' ids, stars and captions, and its photos their places in an album that also holds photos
/// of other folders.
#[test]
fn a_folder_moved_between_two_scan_always_folders_keeps_its_ids_data_and_album_places() {
    let _faults = shared();
    for (from_rel, to_rel) in [("One/Trip", "Two/Trip"), ("Two/Trip", "One/Away/Trip")] {
        let names = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"];
        let mut files: Vec<String> = names.iter().map(|n| format!("{from_rel}/{n}")).collect();
        files.extend(["One/Stay/IMG_0101.jpg".to_string(), "Two/Other/IMG_0201.jpg".to_string()]);
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let (dir, mut s) = library(&["One", "Two"], &files);
        let (from, to): (PathBuf, PathBuf) = (under(dir.path(), from_rel), under(dir.path(), to_rel));
        let trip = folder(&s, &from);
        let ids: Vec<PhotoId> = names.iter().map(|n| photo(&s, &from, n)).collect();
        let (stay, other) = (photo(&s, &under(dir.path(), "One/Stay"), "IMG_0101.jpg"), photo(&s, &under(dir.path(), "Two/Other"), "IMG_0201.jpg"));
        s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
        s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "Ponte" })).unwrap();
        let members = [ids[2], stay, ids[0], other, ids[1]];
        let album = s.execute("album.create", &json!({ "name": "Trip", "ids": members })).unwrap();
        let album = AlbumId::parse(album["album"].as_str().unwrap()).unwrap();
        assert_eq!(s.library().album_members(&album), members);
        s.wait_idle();
        s.flush().unwrap();
        let before: BTreeSet<PathBuf> = s.library().folders().map(|f| f.path.clone()).filter(|p| *p != from).collect();

        off_owner(|| {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::rename(&from, &to).unwrap();
        });
        let scans = s.start_verification();
        assert_eq!(scans.len(), 2, "one region scan per Scan Always folder");
        s.wait_idle();

        let (case, lib) = (format!("{from_rel} to {to_rel}"), s.library());
        assert_eq!(lib.folder_by_path(&from), None, "{case}: the old path left the library");
        assert_eq!(lib.folder_by_path(&to), Some(trip), "{case}: the folder keeps its id");
        let now: Vec<PhotoId> = names.iter().map(|n| photo(&s, &to, n)).collect();
        assert_eq!(now, ids, "{case}: and its photos theirs");
        assert!(lib.photo(ids[0]).unwrap().star, "{case}");
        assert_eq!(lib.info(ids[1]).caption, "Ponte", "{case}");
        assert_eq!(lib.album_members(&album), members, "{case}: the album places");
        let after: BTreeSet<PathBuf> = lib.folders().map(|f| f.path.clone()).filter(|p| *p != to && Some(p.as_path()) != to.parent()).collect();
        assert!(after.is_subset(&before), "{case}: no other folder new: {after:?}");
    }
}
