//! The cache-file primitives (`architecture.md` §6.6): positioned reads and appends, the streamed
//! temporary file committed by rename, listing and removing only files of a given name shape, the
//! fault points on each, and the UI-thread guard.

use std::fs;

use crate::common::names;
use dalica_fs::faults::{Fault, arm, disarm_all};
use dalica_fs::pack::{PACK_POINTS, PACK_WRITE_POINTS, points};
use dalica_fs::{Error, PackFile, TempFile, atomic, list_shaped, mark_ui_thread, remove_shaped};

fn is_pack(name: &str) -> bool {
    name.strip_suffix(".pack").is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn appends_read_back_at_their_offsets_and_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("1.pack");
    let mut f = PackFile::open(&p).unwrap();
    assert!(f.is_empty());
    assert_eq!(f.append(b"hello").unwrap(), 0);
    assert_eq!(f.append(b"").unwrap(), 5);
    assert_eq!(f.append(b", world").unwrap(), 5);
    f.sync().unwrap();
    assert_eq!(f.len(), 12);
    let mut buf = [0u8; 5];
    f.read_at(7, &mut buf).unwrap();
    assert_eq!(&buf, b"world");
    assert_eq!(f.read_vec(0, 5, 100).unwrap(), b"hello");
    // Past the end, or over the limit, is an error.
    assert!(matches!(f.read_at(10, &mut buf), Err(Error::Io { .. })));
    assert!(matches!(f.read_vec(0, 1 << 40, 1 << 20), Err(Error::TooLarge { .. })));
    drop(f);
    let mut f = PackFile::open(&p).unwrap();
    assert_eq!(f.len(), 12);
    assert_eq!(f.append(b"!").unwrap(), 12);
    f.truncate(5).unwrap();
    f.truncate(9).unwrap(); // never grows
    assert_eq!(fs::read(&p).unwrap(), b"hello");
    assert_eq!(f.append(b"?").unwrap(), 5);
    assert_eq!(fs::read(&p).unwrap(), b"hello?");
}

#[test]
fn reads_from_many_threads_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = PackFile::open(&dir.path().join("1.pack")).unwrap();
    let data: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();
    f.append(&data).unwrap();
    std::thread::scope(|s| {
        for t in 0..4u64 {
            let (f, data) = (&f, &data);
            s.spawn(move || {
                for i in 0..200u64 {
                    let at = (i * 97 + t * 13) % 60_000;
                    let got = f.read_vec(at, 1000, 1 << 20).unwrap();
                    assert_eq!(got.as_slice(), &data[at as usize..at as usize + 1000]);
                }
            });
        }
    });
}

#[test]
fn a_folder_or_a_link_is_not_opened_as_a_pack() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("2.pack")).unwrap();
    assert!(matches!(PackFile::open(&dir.path().join("2.pack")), Err(Error::Io { .. })));
    #[cfg(unix)]
    {
        fs::write(dir.path().join("real"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("3.pack")).unwrap();
        assert!(matches!(PackFile::open(&dir.path().join("3.pack")), Err(Error::Link(_))));
    }
}

#[test]
fn a_temp_file_streams_then_commits_by_rename() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("5.pack");
    fs::write(&target, b"old").unwrap();
    let mut t = TempFile::create(&target).unwrap();
    assert!(atomic::is_temp_name(t.temp_path().file_name().unwrap()), "{}", t.temp_path().display());
    assert_eq!(t.write(b"new ").unwrap(), 0);
    assert_eq!(t.write(b"bytes").unwrap(), 4);
    assert_eq!(t.len(), 9);
    // Nothing visible until the commit.
    assert_eq!(fs::read(&target).unwrap(), b"old");
    t.commit().unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new bytes");
    assert_eq!(names(dir.path()), ["5.pack"]);
    // Dropped without a commit: the temporary file goes, the target stays.
    let mut t = TempFile::create(&dir.path().join("6.pack")).unwrap();
    t.write(b"abandoned").unwrap();
    drop(t);
    assert_eq!(names(dir.path()), ["5.pack"]);
}

#[test]
fn listing_and_removing_touch_only_the_given_shape() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for n in ["1.pack", "22.pack", "notes.txt", "1.pack.bak", "x.pack", ".pack"] {
        fs::write(d.join(n), n.as_bytes()).unwrap();
    }
    fs::create_dir(d.join("3.pack")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(d.join("notes.txt"), d.join("4.pack")).unwrap();
    let listed = list_shaped(d, is_pack).unwrap();
    assert_eq!(listed.iter().map(|f| (f.name.as_str(), f.len)).collect::<Vec<_>>(), [("1.pack", 6), ("22.pack", 7)]);
    assert!(remove_shaped(&d.join("1.pack"), is_pack).unwrap());
    assert!(!remove_shaped(&d.join("1.pack"), is_pack).unwrap(), "already gone");
    for foreign in ["notes.txt", "1.pack.bak", "x.pack"] {
        assert!(matches!(remove_shaped(&d.join(foreign), is_pack), Err(Error::Io { .. })), "{foreign}");
    }
    assert!(remove_shaped(&d.join("3.pack"), is_pack).is_err(), "a folder");
    #[cfg(unix)]
    assert!(matches!(remove_shaped(&d.join("4.pack"), is_pack), Err(Error::Link(_))), "a link");
    let mut left = names(d);
    left.retain(|n| n != "4.pack");
    assert_eq!(left, [".pack", "1.pack.bak", "22.pack", "3.pack", "notes.txt", "x.pack"]);
    assert!(list_shaped(&d.join("missing"), is_pack).unwrap().is_empty());
}

#[test]
fn every_call_is_refused_on_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("1.pack");
    let mut f = PackFile::open(&p).unwrap();
    f.append(b"abc").unwrap();
    let mut t = TempFile::create(&dir.path().join("2.pack")).unwrap();
    let ui = mark_ui_thread();
    assert!(matches!(PackFile::open(&p), Err(Error::OnUiThread)));
    assert!(matches!(f.append(b"x"), Err(Error::OnUiThread)));
    assert!(matches!(f.read_at(0, &mut [0; 1]), Err(Error::OnUiThread)));
    assert!(matches!(f.sync(), Err(Error::OnUiThread)));
    assert!(matches!(f.truncate(1), Err(Error::OnUiThread)));
    assert!(matches!(TempFile::create(&dir.path().join("3.pack")), Err(Error::OnUiThread)));
    assert!(matches!(t.write(b"x"), Err(Error::OnUiThread)));
    assert!(matches!(list_shaped(dir.path(), is_pack), Err(Error::OnUiThread)));
    assert!(matches!(remove_shaped(&p, is_pack), Err(Error::OnUiThread)));
    drop(ui);
    assert_eq!(fs::read(&p).unwrap(), b"abc");
}

#[test]
fn every_fault_point_fails_its_step_and_a_crash_skips_the_cleanup() {
    assert!(PACK_WRITE_POINTS.iter().all(|p| PACK_POINTS.contains(p)));
    for point in PACK_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();
            let p = d.join("1.pack");
            let mut f = PackFile::open(&p).unwrap();
            f.append(b"0123456789").unwrap();
            arm(point, fault);
            let result = match point {
                points::READ => f.read_vec(0, 4, 100).map(drop),
                points::APPEND => f.append(b"abcdefgh").map(drop),
                points::SYNC => f.sync(),
                points::TRUNCATE => f.truncate(4),
                points::REMOVE => remove_shaped(&p, is_pack).map(drop),
                _ => TempFile::create(&d.join("2.pack")).and_then(|mut t| {
                    t.write(b"new pack")?;
                    t.commit()
                }),
            };
            disarm_all();
            match fault {
                Fault::Crash => assert!(matches!(result, Err(Error::Crashed(p)) if p == point), "{point}: {result:?}"),
                Fault::Error => assert!(matches!(result, Err(Error::Io { .. })), "{point}: {result:?}"),
            }
            // The pack is still whole, or torn only at its tail; the target of an unfinished
            // commit never appears; only a crash leaves a temporary file.
            let bytes = fs::read(&p).unwrap();
            assert!(bytes.starts_with(b"0123456789"), "{point} {fault:?}: {bytes:?}");
            if point == points::APPEND {
                assert_eq!(bytes.len(), 14, "half of the append landed");
                assert_eq!(f.len(), 14, "the handle re-read its length");
            } else {
                assert_eq!(bytes.len(), 10, "{point} {fault:?}");
            }
            assert!(!d.join("2.pack").exists(), "{point} {fault:?}");
            let temps = names(d).into_iter().filter(|n| atomic::is_temp_name(n.as_ref())).count();
            let crash_leaves_temp = fault == Fault::Crash && [points::TEMP_WRITE, points::TEMP_SYNC, points::TEMP_RENAME].contains(&point);
            assert_eq!(temps, usize::from(crash_leaves_temp), "{point} {fault:?}: {:?}", names(d));
            // The file keeps working.
            let at = f.append(b"more").unwrap();
            assert_eq!(f.read_vec(at, 4, 100).unwrap(), b"more");
        }
    }
}
