//! Sidecar writes for folders whose volume is offline (`data-model.md` §7, `security.md` §5.9): a
//! volume "unplugged" by renaming its root. The write fails without creating the folder, the folder
//! stays dirty and waits (looked at again only when marked dirty or after `WAIT_RETRY`), and once
//! the root is back the next write lands the star. A guarded root (M6.43) is checked right before
//! each write under it, and one that fails its check gets nothing written, though its folder is
//! there.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::writer::{DEBOUNCE, RootCheck, WAIT_RETRY};
use dalica_store::{Outcome, SidecarWriter, write_folder};

/// A library with one folder `<root>/2026/Trip` holding two photos.
fn library(root: &Path) -> (Library, FolderId, Vec<PhotoId>, PathBuf) {
    let folder = root.join("2026").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: folder.clone() } }).unwrap();
    let ids: Vec<PhotoId> = (0..2).map(|_| lib.next_photo_id()).collect();
    let photos = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| Photo {
            id,
            folder: f,
            name: format!("IMG_{:04}.jpg", i + 1),
            size: 1,
            mtime: Timestamp::EPOCH,
            width: 1,
            height: 1,
            date: None,
            star: false,
        })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, f, ids, folder)
}

fn star_in(folder: &Path, name: &str) -> Field<bool> {
    let bytes = std::fs::read(folder.join(".dalica.json")).unwrap();
    Sidecar::parse(&bytes, &Limits::default()).unwrap().star(name)
}

fn at(secs: u64) -> Timestamp {
    Timestamp::EPOCH.saturating_add(Duration::from_secs(secs))
}

#[test]
fn a_missing_folder_is_never_created_to_write_into() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Card");
    let (mut lib, f, ids, folder) = library(&root);
    std::fs::rename(&root, dir.path().join("Card-away")).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    let state = dalica_store::mapping::folder_state(&lib, f).unwrap();
    let outcome = write_folder(&state, at(0));
    assert!(matches!(&outcome, Outcome::Failed(m) if m.contains("not there")), "{outcome:?}");
    assert!(!folder.exists() && !root.exists(), "no folder is created at the old path");
}

/// The whole round trip: star while offline, the write waits (no folder made, no retry every
/// second), the root comes back, and the star lands with the photo's other entry intact.
#[test]
fn a_star_made_while_offline_waits_and_lands_when_the_root_returns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Card");
    let away = dir.path().join("Card-away");
    let (mut lib, f, ids, folder) = library(&root);
    let mut w = SidecarWriter::new();

    // Online: a first star is written.
    lib.apply(Op::SetStars { stars: vec![(ids[1], true)] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.write_due(&lib, at(0))[&f], Outcome::Written);

    // Unplugged.
    std::fs::rename(&root, &away).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    w.mark_dirty(f);
    assert_eq!((w.pending(), w.waiting().count()), (1, 0));
    let out = w.write_due(&lib, at(2));
    assert!(matches!(&out[&f], Outcome::Failed(m) if m.contains("not there")), "{out:?}");
    assert!(w.is_dirty(f), "the write stays pending");
    assert_eq!(w.waiting().collect::<Vec<_>>(), [f]);
    assert_eq!(w.pending(), 1);
    assert!(!root.exists(), "no folder is created at the old path");

    // Waiting: not looked at again every second, only after WAIT_RETRY.
    let step = DEBOUNCE.as_secs() + 1;
    assert!(w.write_due(&lib, at(2 + step)).is_empty(), "a waiting folder is not retried at the debounce");
    let retry = w.write_due(&lib, at(2 + WAIT_RETRY.as_secs()));
    assert!(matches!(retry.get(&f), Some(Outcome::Failed(_))), "{retry:?}");
    assert!(!root.exists());

    // Back: marking the folder dirty (as the engine does when its root returns) writes it at once.
    std::fs::rename(&away, &root).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.write_due(&lib, at(13))[&f], Outcome::Written);
    assert_eq!(star_in(&folder, "IMG_0001.jpg"), Field::Value(true));
    assert_eq!(star_in(&folder, "IMG_0002.jpg"), Field::Value(true), "the earlier star is kept");
    assert_eq!((w.pending(), w.waiting().count()), (0, 0));
}

/// Without being told, the writer finds the returned root on its own at the next retry.
#[test]
fn a_waiting_folder_is_written_at_its_next_retry_once_it_is_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Share");
    let away = dir.path().join("Share-away");
    let (mut lib, f, ids, folder) = library(&root);
    let mut w = SidecarWriter::new();
    std::fs::rename(&root, &away).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    w.mark_dirty(f);
    assert!(matches!(w.flush(&lib, at(0))[&f], Outcome::Failed(_)));
    std::fs::rename(&away, &root).unwrap();
    assert!(w.write_due(&lib, at(1)).is_empty());
    assert_eq!(w.write_due(&lib, at(WAIT_RETRY.as_secs()))[&f], Outcome::Written);
    assert_eq!(star_in(&folder, "IMG_0001.jpg"), Field::Value(true));
    assert!(!w.has_pending());
}

/// A flush tries waiting folders too (and still creates nothing); a folder that left the model
/// while it waited is dropped.
#[test]
fn flush_tries_waiting_folders_and_a_removed_folder_stops_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Disk");
    let (mut lib, f, ids, _folder) = library(&root);
    let mut w = SidecarWriter::new();
    std::fs::rename(&root, dir.path().join("Disk-away")).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    w.mark_dirty(f);
    for t in 0..3 {
        assert!(matches!(w.flush(&lib, at(t))[&f], Outcome::Failed(_)));
    }
    assert!(!root.exists());
    lib.apply(Op::RemoveFolder { id: f }).unwrap();
    assert_eq!(w.flush(&lib, at(4))[&f], Outcome::Unchanged);
    assert_eq!((w.pending(), w.waiting().count()), (0, 0));
}

/// A guarded root that fails its check right before the write (another volume mounted in its
/// place, with the same folder) gets no sidecar: the write fails, the folder waits (not retried at
/// the debounce), and once the check passes again the star lands. The check runs once per pass
/// however many folders are under the root, and one that panics counts as failed.
#[test]
fn a_guarded_root_that_fails_its_check_gets_no_sidecar_until_it_passes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Card");
    let (mut lib, f, ids, folder) = library(&root);
    let other = root.join("2025");
    std::fs::create_dir_all(&other).unwrap();
    let g = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: g, path: other.clone() } }).unwrap();
    let id = lib.next_photo_id();
    let photo = Photo { id, folder: g, name: "x.jpg".into(), size: 1, mtime: Timestamp::EPOCH, width: 1, height: 1, date: None, star: true };
    lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();

    let ours = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (o, c) = (Arc::clone(&ours), Arc::clone(&calls));
    let check: RootCheck = Arc::new(move |r: &Path| {
        c.fetch_add(1, Ordering::SeqCst);
        assert!(r.ends_with("Card"));
        o.load(Ordering::SeqCst)
    });
    let mut w = SidecarWriter::new();
    w.guard(vec![(root.clone(), check)]);
    assert_eq!(w.guarded().collect::<Vec<_>>(), [root.as_path()]);

    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    w.mark_dirty(f);
    w.mark_dirty(g);
    let out = w.write_due(&lib, at(0));
    assert_eq!(out.len(), 2);
    for o in out.values() {
        assert!(matches!(o, Outcome::Failed(m) if m.contains("not the volume")), "{out:?}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one check per root per pass");
    assert!(!folder.join(".dalica.json").exists() && !other.join(".dalica.json").exists(), "nothing written");
    assert_eq!(w.waiting().collect::<Vec<_>>(), [f, g]);
    assert!(w.write_due(&lib, at(DEBOUNCE.as_secs() + 1)).is_empty(), "waits, not retried at the debounce");
    assert!(matches!(w.flush(&lib, at(3))[&f], Outcome::Failed(_)), "a flush reports it unsaved");
    assert!(!folder.join(".dalica.json").exists());

    // The volume is back: the next look (after the wait) checks again and writes.
    ours.store(true, Ordering::SeqCst);
    let back = w.write_due(&lib, at(3 + WAIT_RETRY.as_secs()));
    assert_eq!((back.get(&f), back.get(&g)), (Some(&Outcome::Written), Some(&Outcome::Written)), "{back:?}");
    assert_eq!(star_in(&folder, "IMG_0001.jpg"), Field::Value(true));
    assert!(!w.has_pending());

    // A check that panics counts as failed; folders under no guarded root are not checked.
    w.guard(vec![(root.clone(), Arc::new(|_: &Path| panic!("deliberate")))]);
    lib.apply(Op::SetStars { stars: vec![(ids[1], true)] }).unwrap();
    w.mark_dirty(f);
    assert!(matches!(w.flush(&lib, at(30))[&f], Outcome::Failed(_)));
    assert_eq!(star_in(&folder, "IMG_0002.jpg"), Field::Absent);
    w.guard(vec![(dir.path().join("Elsewhere"), Arc::new(|_: &Path| false))]);
    assert_eq!(w.flush(&lib, at(31))[&f], Outcome::Written);
    assert_eq!(star_in(&folder, "IMG_0002.jpg"), Field::Value(true));
}
