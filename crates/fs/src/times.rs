//! A file's times (`xmp-interop.md` §1 principle 4, `data-model.md` §2): a metadata write keeps a
//! photo's modification time unless "Update file modification time on metadata changes" is on.
//!
//! - [`stat`] describes a file without following a final link: its [`Identity`], its [`Times`]
//!   and whether the current user may write it (a read-only file is never rewritten, though an
//!   atomic replace by rename would get past its permission bits).
//! - [`set_times`] sets a file's modification (and access) time, if the file is still the version
//!   the caller wrote ([`Identity`]), never through a link. It returns the file's new identity
//!   (the modification time is part of it). Fault point [`SET_TIMES`] fails or "crashes" it.
//!
//! A metadata write keeps the time with
//! [`replace_keeping_times`](crate::atomic::replace_keeping_times), which sets it on the temporary
//! file before the rename: a crash at any point leaves the old file with its old time or the new
//! file with the old time. [`set_times`] after the rename is the fallback for a platform or file
//! system that did not keep it, and for a write an earlier try made without it.

use std::fs::{self, File, FileTimes, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use crate::atomic::Identity;
use crate::faults::{self, Fault};
use crate::{Error, Result, guard, io_err};

/// Fault point: setting a file's times ([`set_times`]).
pub const SET_TIMES: &str = "times.set";

/// The fault points of this module.
pub const TIMES_POINTS: [&str; 1] = [SET_TIMES];

/// A file's modification time and, where the platform keeps one, its access time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Times {
    pub modified: SystemTime,
    pub accessed: Option<SystemTime>,
}

impl Times {
    /// The times to set on an open file: the modification time, and the access time when known.
    pub(crate) fn file_times(&self) -> FileTimes {
        let set = FileTimes::new().set_modified(self.modified);
        match self.accessed {
            Some(accessed) => set.set_accessed(accessed),
            None => set,
        }
    }

    fn of(meta: &Metadata, path: &Path) -> Result<Times> {
        let modified = meta.modified().map_err(io_err("stat", path))?;
        Ok(Times { modified, accessed: meta.accessed().ok() })
    }
}

/// What [`stat`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub identity: Identity,
    pub times: Times,
    /// A regular file (not a folder, device, FIFO or socket).
    pub is_file: bool,
    /// The current user may write it: it has write permission (on Unix, for this user), and is not
    /// marked read-only.
    pub writable: bool,
}

/// Describes `path` without following a final link (a link is [`Error::Link`]); `Ok(None)` when
/// it does not exist.
pub fn stat(path: &Path) -> Result<Option<Stat>> {
    guard::check()?;
    let Some(meta) = target(path)? else { return Ok(None) };
    let identity = Identity::of_path(path, &meta, false).map_err(io_err("stat", path))?;
    Ok(Some(Stat { identity, times: Times::of(&meta, path)?, is_file: meta.is_file(), writable: writable(path, &meta) }))
}

/// Sets `path`'s modification time (and access time, when given) to `times`, if it is still the
/// version `expect` names; returns its identity afterwards. A file that changed, vanished or became
/// a link since is refused ([`Error::Changed`], [`Error::Link`]) and left as it is.
pub fn set_times(path: &Path, times: &Times, expect: Identity) -> Result<Identity> {
    guard::check()?;
    match target(path)?.map(|meta| Identity::of_path(path, &meta, false)) {
        Some(Ok(id)) if id == expect => {}
        _ => return Err(Error::Changed(path.to_path_buf())),
    }
    match faults::hit(SET_TIMES) {
        Some(Fault::Error) => return Err(io_err("set times", path)(faults::injected(SET_TIMES))),
        Some(Fault::Crash) => return Err(Error::Crashed(SET_TIMES)),
        None => {}
    }
    let file = open_for_times(path)?;
    // The path was swapped between the check and the open.
    if Identity::of_file(&file).map_err(io_err("stat", path))? != expect {
        return Err(Error::Changed(path.to_path_buf()));
    }
    file.set_times(times.file_times()).map_err(io_err("set times", path))?;
    Identity::of_file(&file).map_err(io_err("stat", path))
}

/// Metadata of `path` without following a final link; `None` if it does not exist.
fn target(path: &Path) -> Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(Error::Link(path.to_path_buf())),
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err("stat", path)(e)),
    }
}

#[cfg(unix)]
fn writable(path: &Path, meta: &Metadata) -> bool {
    // No write bit at all is read-only even for the superuser, whom `access` would let through.
    !meta.permissions().readonly() && rustix::fs::access(path, rustix::fs::Access::WRITE_OK).is_ok()
}

#[cfg(not(unix))]
fn writable(_path: &Path, meta: &Metadata) -> bool {
    !meta.permissions().readonly()
}

/// Opens `path` to set its times, never through a final link.
#[cfg(unix)]
fn open_for_times(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags};
    use rustix::io::Errno;
    // Reading is enough for the owner to set times (`futimens`); `NONBLOCK`: a FIFO planted under
    // the name cannot hang the open.
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NOCTTY | OFlags::NONBLOCK | OFlags::CLOEXEC;
    match rustix::fs::open(path, flags, Mode::empty()) {
        Ok(fd) => Ok(File::from(fd)),
        Err(Errno::LOOP) => Err(Error::Link(path.to_path_buf())),
        Err(e) => Err(io_err("open", path)(e.into())),
    }
}

/// Opens `path` to set its times, never through a final link (a reparse point is opened itself).
#[cfg(windows)]
fn open_for_times(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(io_err("open", path))
}

#[cfg(not(any(unix, windows)))]
fn open_for_times(path: &Path) -> Result<File> {
    fs::OpenOptions::new().write(true).open(path).map_err(io_err("open", path))
}
