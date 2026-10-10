//! The model ↔ sidecar path (`data-model.md` §3, §10.1, §10.5): stars reach `.dalica.json`, other
//! keys survive, writes are debounced on virtual time, a lost index is rebuilt from the sidecars,
//! and a failed write leaves the old or the new file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::{Clock, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::mapping::{PHOTO_FIELDS, Source};
use dalica_store::{Outcome, SidecarWriter, import_sidecars, write_folder};

const NAMES: [&str; 4] = ["IMG_0001.jpg", "a]b \"c\";=.jpg", "東京.jpg", "IMG_0004.jpg"];

/// A library with one folder at `dir` holding `NAMES` (Windows cannot name a file with `"`, but the
/// model and the sidecar can).
fn library(dir: &Path) -> (Library, FolderId, Vec<PhotoId>) {
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.to_path_buf() } }).unwrap();
    let ids: Vec<PhotoId> = NAMES.iter().map(|_| lib.next_photo_id()).collect();
    let photos = ids
        .iter()
        .zip(NAMES)
        .map(|(&id, name)| Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 1, height: 1, date: None, star: false })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, f, ids)
}

fn sidecar(dir: &Path) -> Option<Sidecar> {
    std::fs::read(dir.join(".dalica.json")).ok().map(|b| Sidecar::parse(&b, &Limits::default()).unwrap())
}

fn star(lib: &mut Library, writer: &mut SidecarWriter, id: PhotoId, on: bool) {
    lib.apply(Op::SetStars { stars: vec![(id, on)] }).unwrap();
    writer.mark_dirty(lib.photo(id).unwrap().folder);
}

/// Every model field has a home; adding a field to `Photo` breaks this destructuring until it is
/// placed in `PHOTO_FIELDS` (nothing is silently dropped).
#[test]
fn every_photo_field_is_mapped() {
    let Photo { id: _, folder: _, name: _, size: _, mtime: _, width: _, height: _, date: _, star: _ } = Photo {
        id: PhotoId::new(1),
        folder: FolderId::new(1),
        name: String::new(),
        size: 0,
        mtime: Timestamp::EPOCH,
        width: 0,
        height: 0,
        date: None,
        star: false,
    };
    let names: Vec<&str> = PHOTO_FIELDS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["id", "folder", "name", "size", "mtime", "width", "height", "date", "star"]);
    assert!(PHOTO_FIELDS.contains(&("star", Source::Sidecar("star"))));
}

#[test]
fn a_star_reaches_the_sidecar_and_planted_keys_survive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".dalica.json"), br#"{"planted": {"by": "another app"}, "photos": {"other.jpg": {"x": [1, 2]}}}"#).unwrap();
    let (mut lib, _, ids) = library(dir.path());
    let mut w = SidecarWriter::new();
    star(&mut lib, &mut w, ids[1], true);
    let out = w.flush(&lib, Timestamp::EPOCH);
    assert_eq!(out.values().collect::<Vec<_>>(), [&Outcome::Written]);
    let s = sidecar(dir.path()).unwrap();
    assert_eq!(s.star(NAMES[1]), Field::Value(true));
    assert_eq!(s.star(NAMES[0]), Field::Absent);
    let text = String::from_utf8(s.print()).unwrap();
    assert!(text.contains("\"planted\": {\n    \"by\": \"another app\"\n  }"), "{text}");
    assert!(text.contains("\"other.jpg\": {\n      \"x\": ["), "{text}");
    // Another program edits the file; the next write merges into its version.
    let mut theirs = sidecar(dir.path()).unwrap().print();
    theirs = String::from_utf8(theirs).unwrap().replacen("\"planted\": {", "\"theirs\": true,\n  \"planted\": {", 1).into_bytes();
    std::fs::write(dir.path().join(".dalica.json"), theirs).unwrap();
    star(&mut lib, &mut w, ids[2], true);
    w.flush(&lib, Timestamp::EPOCH);
    let text = String::from_utf8(sidecar(dir.path()).unwrap().print()).unwrap();
    assert!(text.contains("\"theirs\": true") && text.contains("\"planted\""), "{text}");
    // Unstarring removes the key; writing the same content again writes nothing.
    star(&mut lib, &mut w, ids[1], false);
    assert_eq!(w.flush(&lib, Timestamp::EPOCH).into_values().collect::<Vec<_>>(), [Outcome::Written]);
    assert_eq!(sidecar(dir.path()).unwrap().star(NAMES[1]), Field::Absent);
    w.mark_dirty(lib.photo(ids[0]).unwrap().folder);
    assert_eq!(w.flush(&lib, Timestamp::EPOCH).into_values().collect::<Vec<_>>(), [Outcome::Unchanged]);
}

#[test]
fn a_folder_without_user_data_gets_no_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let (lib, f, _) = library(dir.path());
    let mut w = SidecarWriter::new();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, Timestamp::EPOCH).into_values().collect::<Vec<_>>(), [Outcome::Unchanged]);
    assert!(!dir.path().join(".dalica.json").exists());
}

/// On virtual time, a folder starred every 50 ms for 3.5 s is written at most once per second.
#[test]
fn virtual_time_shows_at_most_one_write_per_folder_per_second() {
    let dirs = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut lib = Library::new();
    let mut folders = Vec::new();
    let mut photos = Vec::new();
    for d in &dirs {
        let (l, ..) = library(d.path());
        // Merge the two one-folder libraries into one.
        for f in l.folders() {
            let id = lib.next_folder_id();
            lib.apply(Op::AddFolder { folder: Folder { id, path: f.path.clone() } }).unwrap();
            folders.push(id);
            for p in l.photos_in(f.id) {
                let pid = lib.next_photo_id();
                lib.apply(Op::AddPhotos { photos: vec![Photo { id: pid, folder: id, ..p.clone() }] }).unwrap();
                photos.push(pid);
            }
        }
    }
    let clock = VirtualClock::new(Timestamp::from_millis(1_000_000));
    let mut w = SidecarWriter::new();
    let mut writes: Vec<(FolderId, Timestamp)> = Vec::new();
    for step in 0..70u64 {
        let id = photos[(step % photos.len() as u64) as usize];
        let on = !lib.photo(id).unwrap().star;
        star(&mut lib, &mut w, id, on);
        for (f, o) in w.write_due(&lib, clock.now()) {
            if o == Outcome::Written {
                writes.push((f, clock.now()));
            }
        }
        clock.advance(Duration::from_millis(50));
    }
    for f in &folders {
        let times: Vec<Timestamp> = writes.iter().filter(|(g, _)| g == f).map(|(_, t)| *t).collect();
        assert!(times.len() >= 3, "{f}: {} writes", times.len());
        assert!(times.windows(2).all(|t| t[1].since(t[0]) >= Duration::from_secs(1)), "{f}: {times:?}");
    }
    // Whatever is still pending lands on flush, and the files match the model.
    w.flush(&lib, clock.now());
    assert!(!w.has_pending());
    for (d, f) in dirs.iter().zip(&folders) {
        let s = sidecar(d.path()).unwrap();
        for p in lib.photos_in(*f) {
            assert_eq!(s.star(&p.name) == Field::Value(true), p.star, "{}", p.name);
        }
    }
}

/// `data-model.md` §10.1 for stars: delete the index, scan again, and the stars come back from the sidecars.
#[test]
fn deleting_the_index_and_rebuilding_gives_the_same_stars() {
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, _, ids) = library(dir.path());
    // The index (journal) records the ops; the writer writes the sidecar.
    let mut opened = dalica_store::Store::open(lib_dir.path(), Timestamp::EPOCH).unwrap();
    let mut w = SidecarWriter::new();
    for &id in &ids[1..3] {
        let op = Op::SetStars { stars: vec![(id, true)] };
        lib.apply(op.clone()).unwrap();
        opened.store.append(&op).unwrap();
        w.mark_dirty(lib.photo(id).unwrap().folder);
    }
    w.flush(&lib, Timestamp::EPOCH);
    let before: Vec<(String, bool)> = lib.photos().map(|p| (p.name.clone(), p.star)).collect();
    drop(opened);
    // Delete the index.
    for e in std::fs::read_dir(lib_dir.path()).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    // A fresh scan finds the same folders and photos, with no stars; the sidecars bring them back.
    let (mut rebuilt, _, _) = library(dir.path());
    let import = import_sidecars(&rebuilt, Timestamp::EPOCH);
    assert!(import.problems.is_empty() && import.read_only.is_empty(), "{import:?}");
    for op in import.ops {
        rebuilt.apply(op).unwrap();
    }
    let after: Vec<(String, bool)> = rebuilt.photos().map(|p| (p.name.clone(), p.star)).collect();
    assert_eq!(after, before);
    assert_eq!(rebuilt.starred().len(), 2);
}

#[test]
fn a_damaged_sidecar_is_kept_aside_and_its_folder_becomes_read_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".dalica.json"), b"{\"photos\": {").unwrap();
    let (mut lib, f, ids) = library(dir.path());
    let import = import_sidecars(&lib, Timestamp::from_millis(1_791_000_000_000));
    assert_eq!(import.read_only, [f]);
    assert_eq!(import.problems.len(), 1);
    assert!(dir.path().join(".dalica.json.corrupt-20261003T040000Z").exists());
    assert!(!dir.path().join(".dalica.json").exists());

    // The writer learns it on its own too: with a damaged file back in place, it sets it aside
    // and from then on writes nothing there.
    std::fs::write(dir.path().join(".dalica.json"), b"not json").unwrap();
    let mut w = SidecarWriter::new();
    star(&mut lib, &mut w, ids[0], true);
    let out = w.flush(&lib, Timestamp::from_millis(1_791_000_000_000));
    assert!(matches!(&out[&f], Outcome::SetAside(p) if p.ends_with(".dalica.json.corrupt-20261003T040000Z-001")), "{out:?}");
    assert_eq!(w.read_only().collect::<Vec<_>>(), [f]);
    star(&mut lib, &mut w, ids[1], true);
    assert_eq!(w.flush(&lib, Timestamp::EPOCH)[&f], Outcome::ReadOnly);
    assert!(!dir.path().join(".dalica.json").exists());
    w.clear_read_only(f);
    star(&mut lib, &mut w, ids[1], true);
    assert_eq!(w.flush(&lib, Timestamp::EPOCH)[&f], Outcome::Written);
}

/// `data-model.md` §10.5: a crash or error at any step of a sidecar write leaves the old or the new
/// file, and the next flush lands the new one.
#[test]
fn a_failed_write_leaves_the_old_or_the_new_sidecar() {
    use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all};
    for point in ATOMIC_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let (mut lib, f, ids) = library(dir.path());
            let mut w = SidecarWriter::new();
            star(&mut lib, &mut w, ids[0], true);
            w.flush(&lib, Timestamp::EPOCH);
            let old = std::fs::read(dir.path().join(".dalica.json")).unwrap();
            star(&mut lib, &mut w, ids[1], true);
            arm(point, fault);
            let out = w.flush(&lib, Timestamp::EPOCH);
            disarm_all();
            assert!(matches!(out[&f], Outcome::Failed(_)), "{point} {fault:?}: {out:?}");
            let now = std::fs::read(dir.path().join(".dalica.json")).unwrap();
            let s = Sidecar::parse(&now, &Limits::default()).unwrap();
            let is_new = s.star(NAMES[1]) == Field::Value(true);
            assert!(now == old || is_new, "{point} {fault:?}");
            assert!(w.is_dirty(f), "{point} {fault:?}: a failed write must stay pending");
            let out = w.flush(&lib, Timestamp::EPOCH);
            assert!(matches!(out[&f], Outcome::Written | Outcome::Unchanged), "{point} {fault:?}: {out:?}");
            assert_eq!(sidecar(dir.path()).unwrap().star(NAMES[1]), Field::Value(true));
        }
    }
}

#[test]
fn write_folder_on_a_missing_folder_fails_and_stays_pending() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("offline");
    let (mut lib, f, ids) = library(&gone);
    let mut w = SidecarWriter::new();
    star(&mut lib, &mut w, ids[0], true);
    assert!(matches!(w.flush(&lib, Timestamp::EPOCH)[&f], Outcome::Failed(_)));
    assert!(w.is_dirty(f));
    let state = dalica_store::mapping::folder_state(&lib, f).unwrap();
    assert_eq!(state.path, PathBuf::from(&gone));
    assert!(matches!(write_folder(&state, Timestamp::EPOCH), Outcome::Failed(_)));
}
