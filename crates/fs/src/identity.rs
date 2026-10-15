//! What identifies one version of a file or folder (`security.md` §5.2, §5.3): the object on disk
//! (device and inode on Unix; volume serial number, file index and creation time on Windows)
//! together with its size and modification time.
//!
//! - **Unix:** read from the metadata the caller already has (`stat`, `lstat`, `fstat`), at no
//!   extra cost.
//! - **Windows:** stable Rust's `Metadata` does not expose the file index, so the identity is read
//!   from a handle with `GetFileInformationByHandle` (through `winapi-util`'s safe wrapper; no
//!   `unsafe` here): the volume serial number, file index, creation time, size and modification
//!   time all come from that one call, so they describe one moment. A path is opened for that with
//!   no data access and every sharing mode, so it never blocks another program, and without
//!   following a final link or junction unless asked to. The creation time alone (the identity
//!   before M8.27) is not enough: NTFS "tunnels" a replaced file's creation time onto a file
//!   renamed over its name within about 15 seconds.
//! - **Elsewhere:** the metadata's size and modification time only.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// What identifies one version of a file: the object (device and inode on Unix; volume serial
/// number, file index and creation time on Windows), its size and its modification time. A rewrite
/// through a rename (a new object), an append or an in-place edit changes it, unless the edit is
/// in place and keeps both the size and the modification time. It is a read-before-write guard
/// against other programs' ordinary rewrites, not a content hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    dev: u64,
    ino: u64,
    /// Windows: the creation time (`FILETIME`), kept beside the file index for volumes whose index
    /// is not unique or is always 0 (some network shares); 0 elsewhere.
    born: u64,
    size: u64,
    mtime_ns: i128,
}

impl Identity {
    /// The identity of an open file or folder.
    pub(crate) fn of_file(file: &File) -> io::Result<Identity> {
        #[cfg(windows)]
        {
            let info = winapi_util::file::information(file)?;
            Ok(Identity {
                dev: info.volume_serial_number(),
                ino: info.file_index(),
                born: info.creation_time().unwrap_or(0),
                size: info.file_size(),
                mtime_ns: filetime_ns(info.last_write_time().unwrap_or(0)),
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Identity::of(&file.metadata()?))
        }
    }

    /// The identity of what `path` names: without following a final link (`meta` from
    /// `symlink_metadata`, `follow` false) or through it (`meta` from `metadata`, `follow` true).
    /// On Unix `meta` is all it reads; on Windows it opens `path` (a name that vanished since is
    /// `NotFound`).
    pub(crate) fn of_path(path: &Path, meta: &Metadata, follow: bool) -> io::Result<Identity> {
        #[cfg(windows)]
        {
            let _ = meta;
            Identity::of_file(&open_for_identity(path, follow)?)
        }
        #[cfg(not(windows))]
        {
            let _ = (path, follow);
            Ok(Identity::of(meta))
        }
    }

    /// The identity `meta` describes (not on Windows, where metadata lacks the file index).
    #[cfg(not(windows))]
    pub(crate) fn of(meta: &Metadata) -> Identity {
        let mtime_ns = meta.modified().ok().map_or(0, nanos_since_epoch);
        let (dev, ino) = dev_ino(meta);
        Identity { dev, ino, born: 0, size: meta.len(), mtime_ns }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    #[cfg(unix)]
    pub(crate) fn from_parts(dev: u64, ino: u64, size: u64, mtime_ns: i128) -> Identity {
        Identity { dev, ino, born: 0, size, mtime_ns }
    }

    /// The same file or folder (device and inode; volume, file index and creation time), whatever
    /// its size and time now.
    pub(crate) fn same_object(&self, other: &Identity) -> bool {
        (self.dev, self.ino, self.born) == (other.dev, other.ino, other.born)
    }

    /// On the same volume (device; volume serial number).
    #[cfg(all(feature = "faults", debug_assertions))]
    pub(crate) fn same_volume(&self, other: &Identity) -> bool {
        self.dev == other.dev
    }

    /// Modification time in nanoseconds since the Unix epoch (negative before it).
    pub(crate) fn mtime_ns(&self) -> i128 {
        self.mtime_ns
    }

    /// Whether this version's modification time is `time` (to the nanosecond the platform keeps).
    pub fn modified_at(&self, time: SystemTime) -> bool {
        self.mtime_ns == nanos_since_epoch(time)
    }
}

pub(crate) fn nanos_since_epoch(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

/// A Windows `FILETIME` (100 ns intervals since 1601-01-01 UTC) in nanoseconds since the Unix
/// epoch: the same number `nanos_since_epoch` gives for the `SystemTime` std makes of it.
#[cfg(any(windows, test))]
fn filetime_ns(intervals: u64) -> i128 {
    /// 1970-01-01 as a `FILETIME`.
    const UNIX_EPOCH_INTERVALS: i128 = 116_444_736_000_000_000;
    (i128::from(intervals) - UNIX_EPOCH_INTERVALS) * 100
}

/// Opens `path` only to read its identity: no data access and every sharing mode (so it neither
/// needs read permission nor blocks another program's open, rename or delete), folders too
/// (`FILE_FLAG_BACKUP_SEMANTICS`), and without following a final link or junction unless `follow`
/// (`FILE_FLAG_OPEN_REPARSE_POINT`). The same flags std uses for `symlink_metadata`.
#[cfg(windows)]
fn open_for_identity(path: &Path, follow: bool) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4; // read | write | delete
    let flags = if follow { FILE_FLAG_BACKUP_SEMANTICS } else { FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT };
    std::fs::OpenOptions::new().access_mode(0).share_mode(FILE_SHARE_ALL).custom_flags(flags).open(path)
}

#[cfg(unix)]
fn dev_ino(meta: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(not(any(unix, windows)))]
fn dev_ino(_meta: &Metadata) -> (u64, u64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetimes_convert_like_std() {
        assert_eq!(filetime_ns(116_444_736_000_000_000), 0);
        assert_eq!(filetime_ns(116_444_736_000_000_001), 100);
        assert_eq!(filetime_ns(0), -11_644_473_600_000_000_000);
        assert_eq!(filetime_ns(u64::MAX), (i128::from(u64::MAX) - 116_444_736_000_000_000) * 100);
    }

    /// The handle's identity and the path's are one, and the time is the one std reports.
    #[test]
    fn a_file_has_one_identity_by_handle_and_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"some bytes").unwrap();
        let by_name = Identity::of_path(&p, &std::fs::symlink_metadata(&p).unwrap(), false).unwrap();
        let file = File::open(&p).unwrap();
        assert_eq!(Identity::of_file(&file).unwrap(), by_name);
        assert_eq!(by_name.size(), 10);
        assert!(by_name.modified_at(std::fs::metadata(&p).unwrap().modified().unwrap()));
        let folder = Identity::of_path(dir.path(), &std::fs::metadata(dir.path()).unwrap(), true).unwrap();
        assert!(!folder.same_object(&by_name));
        // Windows opens the name, so one that vanished since its metadata was read says so.
        #[cfg(windows)]
        assert_eq!(
            Identity::of_path(&dir.path().join("gone"), &std::fs::metadata(dir.path()).unwrap(), false).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
