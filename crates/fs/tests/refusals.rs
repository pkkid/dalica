//! What the disk refuses (`security.md` §5, `data-model.md` §6): a full disk, a real file-size
//! limit, a read-only folder (a locked card, a read-only share), two names that share one identity
//! (every file on a share that reports inode 0), and a folder removed between opening it and the
//! rename. Each refusal leaves the folders byte for byte as they were (or, after the rename of an
//! atomic replace, with the whole new file) and names the reason.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dalica_fs::create::new_file;
use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, arm_full, disarm_all, disk_full, points, run_at};
use dalica_fs::transfer::{Outcome, copy_file, move_file};
use dalica_fs::{Error, Expect, identity, replace};

const NAME: &str = "photo.jpg";
const OLD: &[u8] = b"\xff\xd8 the photo as it was \xff\xd9";
const NEW: &[u8] = b"\xff\xd8 the photo as it would be after a write \xff\xd9";

/// Everything under `dir`, by path relative to it: each file's bytes, `None` for a folder.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if fs::symlink_metadata(&p).unwrap().is_dir() {
                out.insert(rel, None);
                walk(root, &p, out);
            } else {
                out.insert(rel, Some(fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The OS error behind a refusal.
fn source(e: &Error) -> &io::Error {
    match e {
        Error::Io { source, .. } => source,
        other => panic!("not an I/O error: {other:?}"),
    }
}

/// A folder `photos` in a new temporary folder, holding the photo `NAME` (`OLD`) and another file.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("photos");
    fs::create_dir(&folder).unwrap();
    replace(&folder.join(NAME), OLD, Expect::Absent).unwrap();
    fs::write(folder.join("notes.txt"), b"other bytes").unwrap();
    (root, folder)
}

#[test]
fn a_full_disk_at_any_step_of_a_replace_says_so_and_leaves_the_old_or_the_new_file() {
    let full = disk_full().to_string();
    for (i, point) in ATOMIC_POINTS.into_iter().enumerate() {
        let (_root, folder) = setup();
        let before = tree(&folder);
        let photo = folder.join(NAME);
        let id = identity(&photo).unwrap().unwrap();
        arm_full(point);
        let err = replace(&photo, NEW, Expect::Unchanged(id)).unwrap_err();
        disarm_all();
        assert_eq!(source(&err).kind(), io::ErrorKind::StorageFull, "{point}: {err}");
        assert!(err.to_string().ends_with(&full), "{point}: the user reads {err:?}, not {full:?}");
        let mut expected = before;
        // Steps after the rename (`atomic.fsync_dir`, `atomic.verify`) fail with the new file in place.
        if i > ATOMIC_POINTS.iter().position(|p| *p == points::RENAME).unwrap() {
            expected.insert(NAME.into(), Some(NEW.to_vec()));
        }
        assert_eq!(tree(&folder), expected, "{point}: the folder (no temporary file left)");
    }
}

#[test]
fn a_full_disk_while_writing_a_new_file_or_a_copy_leaves_nothing_behind() {
    let full = disk_full().to_string();
    for point in [points::NEW_TEMP, points::NEW_WRITE, points::NEW_FSYNC, points::NEW_COMMIT] {
        let (_root, folder) = setup();
        let before = tree(&folder);
        arm_full(point);
        let err = new_file(&folder, "edited.jpg", NEW).unwrap_err();
        disarm_all();
        assert_eq!(source(&err).kind(), io::ErrorKind::StorageFull, "{point}: {err}");
        assert!(err.to_string().ends_with(&full), "{point}: {err}");
        assert_eq!(tree(&folder), before, "{point}");
    }
    for point in [points::COPY_TEMP, points::COPY_WRITE, points::COPY_FSYNC, points::COPY_COMMIT] {
        let (root, folder) = setup();
        let dest = root.path().join("card");
        fs::create_dir(&dest).unwrap();
        let before = tree(root.path());
        let id = identity(&folder.join(NAME)).unwrap().unwrap();
        arm_full(point);
        let err = copy_file(&folder, NAME, id, &dest, NAME).unwrap_err();
        disarm_all();
        assert_eq!(source(&err).kind(), io::ErrorKind::StorageFull, "{point}: {err}");
        assert!(err.to_string().ends_with(&full), "{point}: {err}");
        assert_eq!(tree(root.path()), before, "{point}");
    }
}

/// The helper for the real file-size limit: run under `ulimit -f`, it tries a replace and a new
/// file, each larger than the limit, and prints what each answered.
#[test]
fn fsize_helper() {
    let Ok(dir) = std::env::var("DALICA_FSIZE_HELPER") else { return };
    let folder = Path::new(&dir);
    let big = vec![7u8; 1 << 20];
    let photo = folder.join(NAME);
    let id = identity(&photo).unwrap().unwrap();
    let answers = [replace(&photo, &big, Expect::Unchanged(id)).map(drop), new_file(folder, "edited.jpg", &big).map(drop)];
    for answer in answers {
        match answer {
            Ok(()) => println!("HELPER:WROTE"),
            Err(Error::Io { op, source, .. }) => println!("HELPER:{op} {:?}", source.kind()),
            Err(e) => println!("HELPER:OTHER {e}"),
        }
    }
}

/// A real limit, not a fault point: a child whose file-size limit (`RLIMIT_FSIZE`, through the
/// shell's `ulimit -f`, with `SIGXFSZ` ignored so the write fails instead of killing it) is far
/// below what it writes gets the OS's own error part-way through the write, and cleans up.
#[cfg(unix)]
#[test]
fn a_real_file_size_limit_fails_the_write_and_leaves_the_folder_as_it_was() {
    let (_root, folder) = setup();
    let before = tree(&folder);
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(r#"trap '' XFSZ; ulimit -f 8 && exec "$0" "$@""#)
        .arg(std::env::current_exe().unwrap())
        .args(["refusals::fsize_helper", "--exact", "--nocapture", "--test-threads=1"])
        .env("DALICA_FSIZE_HELPER", &folder)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let answers: Vec<&str> = stdout.lines().filter_map(|l| l.split_once("HELPER:")).map(|(_, a)| a).collect();
    assert_eq!(answers, ["write FileTooLarge", "write FileTooLarge"], "{stdout}");
    assert_eq!(tree(&folder), before, "no temporary file, no new file, the photo unchanged");
}

/// Makes `dir` read-only for the life of the guard (Unix permissions).
#[cfg(unix)]
struct ReadOnly(PathBuf);

#[cfg(unix)]
impl ReadOnly {
    /// `None` when this process may write there anyway (running as root): the refusal cannot be
    /// seen, so the test says so and stops.
    fn set(dir: &Path) -> Option<ReadOnly> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
        let guard = ReadOnly(dir.to_path_buf());
        let probe = dir.join("probe");
        if fs::write(&probe, b"").is_ok() {
            let _ = fs::remove_file(&probe);
            eprintln!("skipped: this process may write into a read-only folder (root?)");
            return None;
        }
        Some(guard)
    }
}

#[cfg(unix)]
impl Drop for ReadOnly {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

/// A read-only folder refuses each step of a Save (the backup folder, the move into it, the
/// saved file under the photo's name or a new one) and a photo moved or copied into it, before
/// anything is written, naming the folder and the OS's reason.
#[cfg(unix)]
#[test]
fn a_read_only_folder_refuses_a_save_before_anything_is_written() {
    use dalica_fs::originals::{DIR_NAME, back_up, ensure_dir};
    let (root, folder) = setup();
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("other.jpg"), OLD).unwrap();
    let photo = folder.join(NAME);
    let id = identity(&photo).unwrap().unwrap();
    let other = identity(&elsewhere.join("other.jpg")).unwrap().unwrap();
    let before = tree(root.path());
    let Some(locked) = ReadOnly::set(&folder) else { return };
    let refusals: Vec<(&str, Error)> = vec![
        ("ensure_dir", ensure_dir(&folder).unwrap_err()),
        ("back_up", back_up(&folder, NAME, id, None).unwrap_err()),
        ("replace", replace(&photo, NEW, Expect::Unchanged(id)).unwrap_err()),
        ("new_file", new_file(&folder, "edited.jpg", NEW).unwrap_err()),
        ("move_file", move_file(&elsewhere, "other.jpg", other, &folder, "other.jpg").unwrap_err()),
        ("copy_file", copy_file(&elsewhere, "other.jpg", other, &folder, "other.jpg").unwrap_err()),
    ];
    for (what, err) in &refusals {
        assert_eq!(source(err).kind(), io::ErrorKind::PermissionDenied, "{what}: {err}");
        let text = err.to_string();
        assert!(text.contains(&folder.display().to_string()) && text.ends_with(&source(err).to_string()), "{what}: {text}");
    }
    drop(locked);
    assert_eq!(tree(root.path()), before);

    // A backup folder already there (an earlier Save) does not help: the photo cannot leave a
    // read-only folder either.
    ensure_dir(&folder).unwrap();
    let before = tree(root.path());
    let Some(_locked) = ReadOnly::set(&folder) else { return };
    let err = back_up(&folder, NAME, id, None).unwrap_err();
    assert_eq!(source(&err).kind(), io::ErrorKind::PermissionDenied, "{err}");
    assert_eq!(tree(root.path()), before);
    assert!(folder.join(DIR_NAME).is_dir() && photo.is_file());
}

/// On a share that reports inode 0 for every file, two different names look like one file. A
/// rename that only changes the case of a name trusts that look to tell "the same file under a
/// case-blind name" from "another file", so it must still never replace: two names of one file
/// (a hard link) stand in for the share here, and the rename answers `Exists` with both kept.
#[test]
fn two_names_with_one_identity_never_let_a_case_rename_replace() {
    let (_root, folder) = setup();
    let photo = folder.join(NAME);
    if fs::hard_link(&photo, folder.join("Photo.JPG")).is_err() {
        // A volume that ignores case (or has no links) cannot hold the two names.
        eprintln!("skipped: the volume cannot hold photo.jpg and Photo.JPG side by side");
        return;
    }
    let before = tree(&folder);
    let id = identity(&photo).unwrap().unwrap();
    assert_eq!(identity(&folder.join("Photo.JPG")).unwrap(), Some(id), "one identity, two names");
    assert_eq!(move_file(&folder, NAME, id, &folder, "Photo.JPG").unwrap(), Outcome::Exists);
    assert_eq!(tree(&folder), before);
}

/// The photo's folder removed between opening it and the rename (an unplugged card, another
/// program): the replace fails, the folder is not made again, and a new folder made at the same
/// path in that moment never receives the write.
#[test]
fn a_folder_removed_before_the_rename_fails_the_replace_and_gets_nothing() {
    for remake in [false, true] {
        let (root, folder) = setup();
        let photo = folder.join(NAME);
        let id = identity(&photo).unwrap().unwrap();
        let f = folder.clone();
        run_at(points::RENAME, move || {
            fs::remove_dir_all(&f).unwrap();
            if remake {
                fs::create_dir(&f).unwrap();
            }
        });
        let result = replace(&photo, NEW, Expect::Unchanged(id));
        disarm_all();
        assert!(result.is_err(), "remade {remake}: {result:?}");
        let mut expected = BTreeMap::new();
        if remake {
            expected.insert(PathBuf::from("photos"), None);
        }
        assert_eq!(tree(root.path()), expected, "remade {remake}: {result:?}");
    }
}

/// The destination folder removed before a move or a copy gives the file its name: the source
/// stays, byte for byte, never sent to the trash, and the destination is not made again.
#[test]
fn a_destination_removed_before_a_move_or_a_copy_commits_leaves_the_source() {
    type Op = fn(&Path, &str, dalica_fs::Identity, &Path, &str) -> dalica_fs::Result<()>;
    let moved: Op = |a, b, c, d, e| move_file(a, b, c, d, e).map(drop);
    let copied: Op = |a, b, c, d, e| copy_file(a, b, c, d, e).map(drop);
    let cases: [(&str, &'static str, bool, Op); 3] = [
        ("move", points::MOVE_RENAME, false, moved),
        ("copy", points::COPY_COMMIT, false, copied),
        ("move across volumes", points::COPY_COMMIT, true, moved),
    ];
    for (what, point, cross, op) in cases {
        let (root, folder) = setup();
        let dest = root.path().join("card");
        fs::create_dir(&dest).unwrap();
        let id = identity(&folder.join(NAME)).unwrap().unwrap();
        let mut expected = tree(root.path());
        expected.remove(Path::new("card"));
        let d = dest.clone();
        run_at(point, move || fs::remove_dir_all(&d).unwrap());
        if cross {
            arm(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
        let result = op(&folder, NAME, id, &dest, NAME);
        disarm_all();
        assert!(result.is_err(), "{what}: {result:?}");
        assert_eq!(tree(root.path()), expected, "{what}: {result:?}");
    }
}
