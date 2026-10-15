//! Stale temporary cleanup (`security.md` §5.8): only files of exactly the atomic-replace temporary
//! shape, more than a day old, never through a link, with a report of what went.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::common::names;
use dalica_fs::faults::{Fault, arm, disarm_all, points};
use dalica_fs::{Error, Expect, mark_ui_thread, remove_stale_temps, replace, stale::STALE_AFTER};

const HOUR: Duration = Duration::from_secs(60 * 60);
const OLD_TEMP: &str = ".photo.jpg.dalica-tmp-0123456789abcdef";
const YOUNG_TEMP: &str = ".photo.jpg.dalica-tmp-fedcba9876543210";

/// Writes `name` in `dir`, last modified `age` before `now`.
fn file_aged(dir: &Path, name: &str, now: SystemTime, age: Duration) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, b"half a sidecar").unwrap();
    File::options().write(true).open(&p).unwrap().set_modified(now - age).unwrap();
    p
}

/// Makes `link` a symbolic link to the file `target`; `false` where the platform refuses.
fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    match made {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping the link case: cannot create a link here: {e}");
            false
        }
    }
}

#[test]
fn removes_a_25_hour_old_temp_and_keeps_a_23_hour_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let old = file_aged(dir.path(), OLD_TEMP, now, 25 * HOUR);
    file_aged(dir.path(), YOUNG_TEMP, now, 23 * HOUR);
    let out = remove_stale_temps(dir.path(), now).unwrap();
    assert_eq!(out.removed, [old]);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(names(dir.path()), [YOUNG_TEMP]);
    assert_eq!(STALE_AFTER, 24 * HOUR);
}

#[test]
fn near_miss_names_foreign_files_folders_and_subfolders_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let keep = [
        "photo.jpg",
        ".dalica.json",
        // Near misses of `.<name>.dalica-tmp-<16 lowercase hex>`.
        "photo.jpg.dalica-tmp-0123456789abcdef",
        ".photo.jpg.dalica-tmp-0123456789abcde",
        ".photo.jpg.dalica-tmp-0123456789abcdef0",
        ".photo.jpg.dalica-tmp-0123456789ABCDEF",
        ".photo.jpg.dalica-tmp-0123456789abcdeg",
        ".photo.jpg.dalica-tmp-0123456789abcdef.bak",
        "..dalica-tmp-0123456789abcdef",
        ".photo.jpg.dalica-temp-0123456789abcdef",
    ];
    for name in keep {
        file_aged(dir.path(), name, now, 1000 * HOUR);
    }
    // A folder with a temporary's name, and an old temporary inside a subfolder (not this folder's).
    let folder = dir.path().join(".album.dalica-tmp-aaaaaaaaaaaaaaaa");
    fs::create_dir(&folder).unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    file_aged(&sub, OLD_TEMP, now, 1000 * HOUR);
    // A temporary from the future (a clock set wrong) is never stale.
    let future = ".future.jpg.dalica-tmp-bbbbbbbbbbbbbbbb";
    let p = dir.path().join(future);
    fs::write(&p, b"x").unwrap();
    File::options().write(true).open(&p).unwrap().set_modified(now + 1000 * HOUR).unwrap();

    let before = names(dir.path());
    let out = remove_stale_temps(dir.path(), now).unwrap();
    assert!(out.removed.is_empty(), "{:?}", out.removed);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(names(dir.path()), before);
    assert_eq!(names(&sub), [OLD_TEMP]);
}

#[test]
fn a_link_with_a_temporary_name_is_kept_and_so_is_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    // The link's target is itself an old temporary, in another folder.
    let target = file_aged(elsewhere.path(), OLD_TEMP, now, 1000 * HOUR);
    let link = dir.path().join(".link.jpg.dalica-tmp-cccccccccccccccc");
    if !link_file(&target, &link) {
        return;
    }
    // Far enough in the future that the link itself is old too.
    let out = remove_stale_temps(dir.path(), now + 1000 * HOUR).unwrap();
    assert!(out.removed.is_empty(), "{:?}", out.removed);
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"half a sidecar");
}

#[test]
fn a_crashed_write_leaves_a_temp_that_is_cleaned_a_day_later() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(".dalica.json");
    let id = replace(&p, b"old", Expect::Absent).unwrap();
    arm(points::RENAME, Fault::Crash);
    assert!(matches!(replace(&p, b"new", Expect::Unchanged(id)), Err(Error::Crashed(_))));
    disarm_all();
    let left = names(dir.path());
    assert_eq!(left.len(), 2, "{left:?}");
    let now = SystemTime::now();
    // The same day it stays (the write could still be running).
    assert!(remove_stale_temps(dir.path(), now).unwrap().removed.is_empty());
    let out = remove_stale_temps(dir.path(), now + 25 * HOUR).unwrap();
    assert_eq!(out.removed.len(), 1);
    assert_eq!(names(dir.path()), [".dalica.json"]);
    assert_eq!(fs::read(&p).unwrap(), b"old");
}

#[test]
fn a_failed_removal_is_reported_and_the_rest_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let first = file_aged(dir.path(), ".a.jpg.dalica-tmp-0000000000000000", now, 25 * HOUR);
    let second = file_aged(dir.path(), ".b.jpg.dalica-tmp-1111111111111111", now, 25 * HOUR);
    arm(points::STALE_REMOVE, Fault::Error);
    let out = remove_stale_temps(dir.path(), now).unwrap();
    disarm_all();
    assert_eq!(out.removed, [second]);
    assert!(matches!(out.errors.as_slice(), [Error::Io { op: "remove", .. }]), "{:?}", out.errors);
    assert!(first.exists());
    // A crash stops where it is.
    arm(points::STALE_REMOVE, Fault::Crash);
    assert!(matches!(remove_stale_temps(dir.path(), now), Err(Error::Crashed(points::STALE_REMOVE))));
    disarm_all();
    assert!(first.exists());
    assert_eq!(remove_stale_temps(dir.path(), now).unwrap().removed, [first]);
}

#[test]
fn a_missing_folder_a_file_and_the_ui_thread_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    assert!(matches!(remove_stale_temps(&dir.path().join("gone"), now), Err(Error::Io { .. })));
    let file = file_aged(dir.path(), OLD_TEMP, now, 25 * HOUR);
    assert!(matches!(remove_stale_temps(&file, now), Err(Error::Io { .. })));
    let ui = mark_ui_thread();
    assert!(matches!(remove_stale_temps(dir.path(), now), Err(Error::OnUiThread)));
    drop(ui);
    assert!(file.exists());
}
