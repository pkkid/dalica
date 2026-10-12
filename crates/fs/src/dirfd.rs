//! One held folder for every step of a write (`security.md` §5.6).
//!
//! [`Folder::open`] validates a folder (it exists and is a folder; whether its own name is a link),
//! then opens it **once** and checks that what it opened is the folder it validated. Every later
//! step names files relative to that one [`Folder`]:
//!
//! - **Unix:** a real directory handle (`O_DIRECTORY`, plus `O_NOFOLLOW` when the validated name
//!   was not a link) through `rustix`'s safe API. Creating, stating, renaming, removing and
//!   re-opening files are `openat`, `fstatat`, `renameat` and `unlinkat` on it, always without
//!   following a final link, so a folder swapped for a link after validation cannot redirect the
//!   write: it lands in the folder that was validated, or fails.
//! - **Elsewhere (Windows):** no handle-relative calls on stable Rust, so the folder's identity is
//!   compared again before each step that creates, renames or removes a file, and a file is
//!   compared before and after it is opened; a swap is refused as [`Error::Changed`].
//!
//! A folder whose own name is a link when it is validated (a watched folder that is a link to
//! another drive) is followed once, at validation, and then held like any other; refusing such
//! folders is the store's decision (`security.md` §5.6), not this module's.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::atomic::Identity;
use crate::faults::{self, Fault, points};
use crate::{Error, Result, io_err};

/// What a name in a [`Folder`] is, without following a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    Link,
    Other,
}

/// One entry of a [`Folder`], described without following a link.
#[derive(Debug)]
pub(crate) struct Entry {
    pub identity: Identity,
    pub kind: Kind,
    perms: Perms,
}

#[cfg(unix)]
type Perms = rustix::fs::Mode;
#[cfg(not(unix))]
type Perms = std::fs::Permissions;

/// Some of a folder's names ([`Folder::names`]).
#[derive(Debug)]
pub(crate) struct Names {
    pub names: Vec<OsString>,
    /// Every entry was looked at (the limit was not reached).
    pub complete: bool,
}

/// A validated folder, held open (Unix) or re-checked before each step (elsewhere).
#[derive(Debug)]
pub(crate) struct Folder {
    path: PathBuf,
    #[cfg(unix)]
    handle: File,
    #[cfg(not(unix))]
    validated: Identity,
    #[cfg(not(unix))]
    was_link: bool,
}

impl Folder {
    /// Validates `dir` and opens it once (fault point [`points::OPEN_DIR`] between the two).
    pub(crate) fn open(dir: &Path) -> Result<Folder> {
        let (validated, was_link) = validate(dir)?;
        match faults::hit(points::OPEN_DIR) {
            Some(Fault::Error) => return Err(io_err("open", dir)(faults::injected(points::OPEN_DIR))),
            Some(Fault::Crash) => return Err(Error::Crashed(points::OPEN_DIR)),
            None => {}
        }
        Folder::hold(dir, validated, was_link)
    }

    /// The folder's path, for messages and for the path of a file in it.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// `name` in this folder as a path (for messages).
    pub(crate) fn join(&self, name: &OsStr) -> PathBuf {
        self.path.join(name)
    }

    /// Copies `from`'s permission bits onto `file` (best effort is the caller's choice).
    pub(crate) fn copy_permissions(file: &File, from: &Entry) -> io::Result<()> {
        #[cfg(unix)]
        {
            rustix::fs::fchmod(file, from.perms).map_err(io::Error::from)
        }
        #[cfg(not(unix))]
        {
            file.set_permissions(from.perms.clone())
        }
    }
}

/// The folder's identity when validated, and whether its own name was a link (then followed).
fn validate(dir: &Path) -> Result<(Identity, bool)> {
    let meta = std::fs::symlink_metadata(dir).map_err(io_err("open", dir))?;
    let was_link = meta.file_type().is_symlink();
    let meta = if was_link { std::fs::metadata(dir).map_err(io_err("open", dir))? } else { meta };
    if !meta.is_dir() {
        return Err(Error::Io { op: "open", path: dir.to_path_buf(), source: io::Error::other("not a folder") });
    }
    Ok((Identity::of(&meta), was_link))
}

#[cfg(unix)]
mod imp {
    use std::os::unix::ffi::OsStrExt as _;

    use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat};
    use rustix::io::Errno;

    use super::*;

    /// Read and write for everyone, as `std::fs::OpenOptions` creates files (the umask applies).
    const SHARED: Mode = Mode::RUSR.union(Mode::WUSR).union(Mode::RGRP).union(Mode::WGRP).union(Mode::ROTH).union(Mode::WOTH);
    const OWNER_ONLY: Mode = Mode::RUSR.union(Mode::WUSR);

    impl Folder {
        pub(super) fn hold(dir: &Path, validated: Identity, was_link: bool) -> Result<Folder> {
            let mut flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            if !was_link {
                flags |= OFlags::NOFOLLOW;
            }
            let fd = rustix::fs::open(dir, flags, Mode::empty()).map_err(|e| match e {
                // The validated folder became a link (`O_NOFOLLOW`; some systems report it as
                // "not a folder" first) or something that is not a folder (`O_DIRECTORY`).
                Errno::LOOP | Errno::NOTDIR => match std::fs::symlink_metadata(dir) {
                    Ok(m) if m.file_type().is_symlink() => Error::Link(dir.to_path_buf()),
                    _ => Error::Changed(dir.to_path_buf()),
                },
                e => io_err("open", dir)(e.into()),
            })?;
            let held = rustix::fs::fstat(&fd).map_err(|e| io_err("stat", dir)(e.into()))?;
            // Something else was put in its place between validation and opening.
            if !identity(&held).same_object(&validated) {
                return Err(Error::Changed(dir.to_path_buf()));
            }
            Ok(Folder { path: dir.to_path_buf(), handle: File::from(fd) })
        }

        /// `name` described without following a link; `None` when it does not exist.
        pub(crate) fn entry(&self, name: &OsStr) -> Result<Option<Entry>> {
            match rustix::fs::statat(&self.handle, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(st) => Ok(Some(entry(&st))),
                Err(Errno::NOENT) => Ok(None),
                Err(e) => Err(io_err("stat", &self.join(name))(e.into())),
            }
        }

        /// Creates `name` exclusively for writing (owner-only when `private`), never through a
        /// link; `None` when the name is taken.
        pub(crate) fn create_new(&self, name: &OsStr, private: bool) -> Result<Option<File>> {
            let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let mode = if private { OWNER_ONLY } else { SHARED };
            match rustix::fs::openat(&self.handle, name, flags, mode) {
                Ok(fd) => Ok(Some(File::from(fd))),
                Err(Errno::EXIST) => Ok(None),
                Err(e) => Err(io_err("create", &self.join(name))(e.into())),
            }
        }

        /// Opens `name` for reading without following a final link ([`Error::Link`] if it is one).
        pub(crate) fn open_read(&self, name: &OsStr) -> Result<File> {
            // `NONBLOCK`: a FIFO planted under the name cannot hang the read.
            let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NOCTTY | OFlags::NONBLOCK | OFlags::CLOEXEC;
            match rustix::fs::openat(&self.handle, name, flags, Mode::empty()) {
                Ok(fd) => Ok(File::from(fd)),
                Err(Errno::LOOP) => Err(Error::Link(self.join(name))),
                Err(e) => Err(io_err("open", &self.join(name))(e.into())),
            }
        }

        /// Renames `from` to `to`, both in this folder (replacing `to`).
        pub(crate) fn rename(&self, from: &OsStr, to: &OsStr) -> Result<()> {
            rustix::fs::renameat(&self.handle, from, &self.handle, to).map_err(|e| io_err("rename", &self.join(to))(e.into()))
        }

        /// Removes the file (or link itself) `name`; never a folder.
        pub(crate) fn remove(&self, name: &OsStr) -> Result<()> {
            rustix::fs::unlinkat(&self.handle, name, AtFlags::empty()).map_err(|e| io_err("remove", &self.join(name))(e.into()))
        }

        /// Makes renames in the folder durable (`fsync` of the held handle).
        pub(crate) fn sync(&self) -> Result<()> {
            self.handle.sync_all().map_err(io_err("fsync", &self.path))
        }

        /// The names in the folder that pass `keep` (never `.` or `..`), read through the held
        /// handle; at most `limit` entries are looked at ([`Names::complete`] says whether that
        /// was all of them).
        pub(crate) fn names(&self, keep: impl Fn(&OsStr) -> bool, limit: usize) -> Result<Names> {
            let list = |e: Errno| io_err("list", &self.path)(e.into());
            let mut out = Names { names: Vec::new(), complete: true };
            for (seen, entry) in rustix::fs::Dir::read_from(&self.handle).map_err(list)?.enumerate() {
                if seen >= limit {
                    out.complete = false;
                    break;
                }
                let entry = entry.map_err(list)?;
                let name = OsStr::from_bytes(entry.file_name().to_bytes());
                if name != "." && name != ".." && keep(name) {
                    out.names.push(name.to_os_string());
                }
            }
            Ok(out)
        }
    }

    fn entry(st: &Stat) -> Entry {
        let kind = match FileType::from_raw_mode(st.st_mode) {
            FileType::RegularFile => Kind::File,
            FileType::Directory => Kind::Dir,
            FileType::Symlink => Kind::Link,
            _ => Kind::Other,
        };
        Entry { identity: identity(st), kind, perms: Mode::from_raw_mode(st.st_mode) }
    }

    /// The same numbers `Identity::of` reads from `std::fs::Metadata` (which casts the C fields the
    /// same way), so identities from either side compare equal for the same file.
    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
    pub(crate) fn identity(st: &Stat) -> Identity {
        let secs = i128::from(st.st_mtime as i64);
        let nanos = i128::from(st.st_mtime_nsec as i64);
        let mtime_ns = secs.saturating_mul(1_000_000_000).saturating_add(nanos);
        Identity::from_parts(st.st_dev as u64, st.st_ino as u64, st.st_size as u64, mtime_ns)
    }
}

#[cfg(not(unix))]
mod imp {
    use std::fs::{self, OpenOptions};

    use super::*;

    impl Folder {
        pub(super) fn hold(dir: &Path, validated: Identity, was_link: bool) -> Result<Folder> {
            let folder = Folder { path: dir.to_path_buf(), validated, was_link };
            folder.check()?;
            Ok(folder)
        }

        /// The folder at the path must still be the validated one (`security.md` §5.6: without a
        /// handle, the identity comparison stands in for it).
        fn check(&self) -> Result<()> {
            let (now, is_link) = super::validate(&self.path)?;
            if is_link && !self.was_link {
                return Err(Error::Link(self.path.clone()));
            }
            if now.same_object(&self.validated) { Ok(()) } else { Err(Error::Changed(self.path.clone())) }
        }

        pub(crate) fn entry(&self, name: &OsStr) -> Result<Option<Entry>> {
            self.check()?;
            let path = self.join(name);
            match fs::symlink_metadata(&path) {
                Ok(m) => {
                    let t = m.file_type();
                    let kind = if t.is_symlink() {
                        Kind::Link
                    } else if t.is_dir() {
                        Kind::Dir
                    } else if t.is_file() {
                        Kind::File
                    } else {
                        Kind::Other
                    };
                    Ok(Some(Entry { identity: Identity::of(&m), kind, perms: m.permissions() }))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(io_err("stat", &path)(e)),
            }
        }

        pub(crate) fn create_new(&self, name: &OsStr, _private: bool) -> Result<Option<File>> {
            self.check()?;
            // No permission bits here: the file gets its folder's inherited ACL.
            let path = self.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(f) => Ok(Some(f)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                Err(e) => Err(io_err("create", &path)(e)),
            }
        }

        /// Opens `name` for reading, comparing its identity before and after opening.
        pub(crate) fn open_read(&self, name: &OsStr) -> Result<File> {
            let path = self.join(name);
            let before = match self.entry(name)? {
                Some(e) if e.kind == Kind::Link => return Err(Error::Link(path)),
                Some(e) => e.identity,
                None => return Err(io_err("open", &path)(io::ErrorKind::NotFound.into())),
            };
            let file = File::open(&path).map_err(io_err("open", &path))?;
            let opened = file.metadata().map_err(io_err("stat", &path))?;
            if Identity::of(&opened) != before {
                return Err(Error::Changed(path));
            }
            self.check()?;
            Ok(file)
        }

        pub(crate) fn rename(&self, from: &OsStr, to: &OsStr) -> Result<()> {
            self.check()?;
            let to = self.join(to);
            fs::rename(self.join(from), &to).map_err(io_err("rename", &to))
        }

        pub(crate) fn remove(&self, name: &OsStr) -> Result<()> {
            self.check()?;
            let path = self.join(name);
            fs::remove_file(&path).map_err(io_err("remove", &path))
        }

        /// Windows journals renames itself and cannot open a folder as a file.
        pub(crate) fn sync(&self) -> Result<()> {
            Ok(())
        }

        pub(crate) fn names(&self, keep: impl Fn(&OsStr) -> bool, limit: usize) -> Result<Names> {
            self.check()?;
            let mut out = Names { names: Vec::new(), complete: true };
            for (seen, entry) in fs::read_dir(&self.path).map_err(io_err("list", &self.path))?.enumerate() {
                if seen >= limit {
                    out.complete = false;
                    break;
                }
                let name = entry.map_err(io_err("list", &self.path))?.file_name();
                if keep(&name) {
                    out.names.push(name);
                }
            }
            Ok(out)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Identities read through the handle equal those `std` reads for the same file and folder, so
    /// a caller's `Identity` (from `atomic::read`) can be checked relative to the handle.
    #[test]
    fn handle_identities_match_std_identities() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"some bytes").unwrap();
        let folder = Folder::open(dir.path()).unwrap();
        let e = folder.entry(OsStr::new("f")).unwrap().unwrap();
        assert_eq!(e.kind, Kind::File);
        assert_eq!(e.identity, Identity::of(&std::fs::symlink_metadata(&p).unwrap()));
        let held = rustix::fs::fstat(&folder.handle).unwrap();
        assert_eq!(imp::identity(&held), Identity::of(&std::fs::metadata(dir.path()).unwrap()));
        assert!(folder.entry(OsStr::new("missing")).unwrap().is_none());
        assert_eq!(folder.path(), dir.path());
    }

    #[test]
    fn a_file_or_a_missing_folder_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        assert!(matches!(Folder::open(&p), Err(Error::Io { .. })));
        assert!(matches!(Folder::open(&dir.path().join("gone")), Err(Error::Io { .. })));
    }

    #[test]
    fn names_lists_entries_through_the_handle() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["b", "a", ".hidden"] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let folder = Folder::open(dir.path()).unwrap();
        let mut all = folder.names(|_| true, usize::MAX).unwrap();
        assert!(all.complete);
        all.names.sort();
        assert_eq!(all.names, [".hidden", "a", "b"]);
        let dotted = folder.names(|n| n.as_encoded_bytes().starts_with(b"."), usize::MAX).unwrap();
        assert_eq!(dotted.names, [".hidden"]);
        // `.` and `..` count towards the limit (they are entries the OS lists) but are never named.
        let some = folder.names(|_| true, 1).unwrap();
        assert!(!some.complete && some.names.len() <= 1);
    }
}
