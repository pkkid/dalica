//! Import copies (`organizing.md` §3, `security.md` §5.7): the bytes and the time kept, a target
//! created at the last moment never replaced, a copy that fails its check removed with the source
//! untouched, the bytes counted as they go, a stop between files, every fault point, hostile names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{modified, names, old_file, old_time, temps};
use dalica_fs::faults::{Fault, IMPORT_POINTS, arm, disarm_all, points, run_at};
use dalica_fs::transfer::{Imported, Progress, import_copy};
use dalica_fs::{Error, Identity, guard, identity};

/// More than one read chunk (1 MiB), with a pattern that shows a misplaced byte.
fn photo_bytes() -> Vec<u8> {
    (0..(2 * 1024 * 1024 + 4321u32)).map(|i| (i % 251) as u8).collect()
}

fn id(path: &Path) -> Identity {
    identity(path).unwrap().unwrap()
}

/// A card `DCIM/100TEST/IMG_0001.JPG` (an old modification time) and a destination holding a
/// bystander.
struct Card {
    _dir: tempfile::TempDir,
    src: PathBuf,
    dest: PathBuf,
    bytes: Vec<u8>,
    id: Identity,
}

impl Card {
    fn new() -> Card {
        let dir = tempfile::tempdir().unwrap();
        let (src, dest) = (dir.path().join("DCIM").join("100TEST"), dir.path().join("Pictures"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir(&dest).unwrap();
        let bytes = photo_bytes();
        old_file(&src.join("IMG_0001.JPG"), &bytes);
        fs::write(dest.join("bystander.jpg"), b"bystander").unwrap();
        let id = id(&src.join("IMG_0001.JPG"));
        Card { _dir: dir, src, dest, bytes, id }
    }

    fn import(&self, to: &str, progress: &Progress) -> dalica_fs::Result<Imported> {
        import_copy(&self.src, "IMG_0001.JPG", self.id, &self.dest, to, progress)
    }

    /// The source is exactly as it was listed, and the bystander untouched.
    fn source_whole(&self, case: &str) {
        let src = self.src.join("IMG_0001.JPG");
        assert!(fs::read(&src).unwrap() == self.bytes, "{case}: source bytes");
        assert_eq!(id(&src), self.id, "{case}: source identity");
        assert_eq!(fs::read(self.dest.join("bystander.jpg")).unwrap(), b"bystander", "{case}");
    }
}

#[test]
fn a_copy_keeps_the_bytes_and_the_time_and_counts_them() {
    let card = Card::new();
    let progress = Progress::new();
    let r = card.import("IMG_0001.JPG", &progress).unwrap();
    let copy = card.dest.join("IMG_0001.JPG");
    let Imported::Copied(copied) = r else { panic!("{r:?}") };
    assert!(fs::read(&copy).unwrap() == card.bytes);
    assert_eq!(modified(&copy), old_time());
    assert_eq!(id(&copy), copied);
    assert_ne!(copied, card.id, "a copy is a new file");
    assert_eq!(progress.bytes(), card.bytes.len() as u64);
    card.source_whole("after the copy");
    assert_eq!(temps(&card.dest), 0);
    // A source changed since it was listed is refused.
    fs::write(card.src.join("IMG_0001.JPG"), b"rewritten").unwrap();
    let r = card.import("again.jpg", &progress);
    assert!(matches!(r, Err(Error::Changed(_))), "{r:?}");
    assert_eq!(names(&card.dest), ["IMG_0001.JPG", "bystander.jpg"]);
}

#[test]
fn a_file_created_at_the_target_before_the_commit_is_never_replaced() {
    let card = Card::new();
    // A plain clash first.
    assert_eq!(card.import("bystander.jpg", &Progress::new()).unwrap(), Imported::Exists);
    // Then another thread makes the target between the check and the commit.
    let planted = card.dest.join("IMG_0001.JPG");
    let p = planted.clone();
    run_at(points::COPY_COMMIT, move || std::thread::spawn(move || fs::write(&p, b"made by another thread").unwrap()).join().unwrap());
    let r = card.import("IMG_0001.JPG", &Progress::new()).unwrap();
    disarm_all();
    assert_eq!(r, Imported::Exists);
    assert_eq!(fs::read(&planted).unwrap(), b"made by another thread");
    card.source_whole("racing thread");
    assert_eq!(temps(&card.dest), 0);
}

#[test]
fn a_flipped_byte_fails_the_check_removes_the_copy_and_leaves_the_source() {
    let card = Card::new();
    arm(points::COPY_FLIP, Fault::Error);
    let r = card.import("IMG_0001.JPG", &Progress::new());
    disarm_all();
    assert!(matches!(r, Err(Error::VerifyFailed(_))), "{r:?}");
    assert_eq!(names(&card.dest), ["bystander.jpg"]);
    card.source_whole("flipped byte");
}

#[test]
fn a_stop_finishes_the_current_file_and_starts_no_other() {
    let card = Card::new();
    let progress = Arc::new(Progress::new());
    let p = Arc::clone(&progress);
    // Asked to stop in the middle of a copy: that copy still completes.
    run_at(points::COPY_FSYNC, move || p.stop());
    let r = card.import("first.jpg", &progress).unwrap();
    disarm_all();
    assert!(matches!(r, Imported::Copied(_)), "{r:?}");
    assert!(progress.stopped());
    assert!(fs::read(card.dest.join("first.jpg")).unwrap() == card.bytes);
    // The next file is not started.
    assert_eq!(card.import("second.jpg", &progress).unwrap(), Imported::Stopped);
    assert_eq!(names(&card.dest), ["bystander.jpg", "first.jpg"]);
    assert_eq!(progress.bytes(), card.bytes.len() as u64);
}

#[test]
fn every_fault_point_leaves_the_source_whole_or_the_copy_verified() {
    for &point in &IMPORT_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let card = Card::new();
            let case = format!("{point} as {fault:?}");
            arm(point, fault);
            let r = card.import("IMG_0001.JPG", &Progress::new());
            disarm_all();
            assert!(r.is_err(), "{case}: {r:?}");
            card.source_whole(&case);
            // Under its name there is nothing, or the whole verified copy.
            if let Ok(bytes) = fs::read(card.dest.join("IMG_0001.JPG")) {
                assert!(bytes == card.bytes, "{case}: a copy under its name is whole");
            }
            if fault == Fault::Error {
                assert_eq!(temps(&card.dest), 0, "{case}");
            }
        }
    }
}

#[test]
fn hostile_names_are_refused_with_nothing_written() {
    let card = Card::new();
    for (name, to) in [
        ("../IMG_0001.JPG", "x.jpg"),
        ("IMG_0001.JPG", "../x.jpg"),
        ("IMG_0001.JPG", "sub/x.jpg"),
        ("IMG_0001.JPG", ""),
        ("IMG_0001.JPG", ".."),
        ("IMG_0001.JPG", "CON"),
        ("IMG_0001.JPG", "x.jpg:stream"),
        ("IMG_0001.JPG", "trailing."),
        ("IMG_0001.JPG", "a\nb.jpg"),
        ("IMG_0001.JPG", ".dalica.json"),
        ("IMG_0001.JPG", ".x.jpg.dalica-tmp-0123456789abcdef"),
        (".dalica.json", "x.json"),
    ] {
        let r = import_copy(&card.src, name, card.id, &card.dest, to, &Progress::new());
        assert!(matches!(&r, Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::InvalidInput), "{name:?} → {to:?}: {r:?}");
    }
    assert_eq!(names(&card.dest), ["bystander.jpg"]);
    card.source_whole("hostile names");
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_source_or_a_folder_is_refused() {
    use std::os::unix::fs::symlink;
    let card = Card::new();
    symlink(card.src.join("IMG_0001.JPG"), card.src.join("link.jpg")).unwrap();
    let r = import_copy(&card.src, "link.jpg", card.id, &card.dest, "link.jpg", &Progress::new());
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    let dest_link = card.dest.with_file_name("dest-link");
    symlink(&card.dest, &dest_link).unwrap();
    let r = import_copy(&card.src, "IMG_0001.JPG", card.id, &dest_link, "x.jpg", &Progress::new());
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    assert_eq!(names(&card.dest), ["bystander.jpg"]);
}

#[test]
fn the_ui_thread_is_refused() {
    let card = Card::new();
    let _ui = guard::mark_ui_thread();
    let r = card.import("IMG_0001.JPG", &Progress::new());
    if cfg!(debug_assertions) {
        assert!(matches!(r, Err(Error::OnUiThread)), "{r:?}");
    }
}
