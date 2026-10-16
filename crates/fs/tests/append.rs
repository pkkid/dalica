//! A failed append leaves the file as it was before it (`append` module docs): a full disk at
//! either step of an append cuts the half-written or unsynced record off before the error is
//! reported, and the next append lands whole right after the last good record.

use std::fs;
use std::io::ErrorKind;

use dalica_fs::faults::{arm_full, disarm_all, points};
use dalica_fs::{Appender, Error};

#[test]
fn a_full_disk_on_one_append_leaves_the_file_as_it_was_and_the_next_lands_whole() {
    for point in [points::APPEND_WRITE, points::APPEND_FSYNC] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("index.log");
        let mut a = Appender::open(&p, 1 << 20).unwrap();
        a.append(b"first record\n").unwrap();
        arm_full(point);
        let err = a.append(b"second record\n").unwrap_err();
        disarm_all();
        assert!(matches!(&err, Error::Io { source, .. } if source.kind() == ErrorKind::StorageFull), "{point}: {err:?}");
        assert_eq!(fs::read(&p).unwrap(), b"first record\n", "{point}");
        assert_eq!(a.size(), 13, "{point}");
        a.append(b"third record\n").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"first record\nthird record\n", "{point}");
    }
}
