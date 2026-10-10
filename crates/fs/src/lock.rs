//! The library lock (`data-model.md` §4, §10.6): one Dalica process per library. The lock is an
//! OS file lock on `<library>/lock`, so it is released when the process exits, however it exits.
//! The file itself stays; it is never deleted (that would race with the next process).
//!
//! The library directory is local (`security.md` §5.9): locks on network shares are unreliable.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::{Error, Result, guard, io_err};

/// The lock file's name inside the library directory.
pub const LOCK_FILE: &str = "lock";

/// Holds the library lock until dropped.
#[derive(Debug)]
pub struct LibraryLock {
    _file: File,
    path: PathBuf,
}

impl LibraryLock {
    /// Takes the lock of the library in `dir` (created if missing), or returns [`Error::Locked`]
    /// at once if another process holds it. Never waits.
    pub fn acquire(dir: &Path) -> Result<LibraryLock> {
        guard::check()?;
        let path = dir.join(LOCK_FILE);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => return Err(Error::Link(path)),
            _ => {}
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(io_err("open", &path))?;
        match file.try_lock() {
            Ok(()) => Ok(LibraryLock { _file: file, path }),
            Err(TryLockError::WouldBlock) => Err(Error::Locked(dir.to_path_buf())),
            Err(TryLockError::Error(e)) => Err(io_err("lock", &path)(e)),
        }
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_in_the_same_process_is_refused_until_the_first_drops() {
        let dir = tempfile::tempdir().unwrap();
        let first = LibraryLock::acquire(dir.path()).unwrap();
        assert!(first.path().ends_with(LOCK_FILE));
        assert!(matches!(LibraryLock::acquire(dir.path()), Err(Error::Locked(p)) if p == dir.path()));
        drop(first);
        LibraryLock::acquire(dir.path()).unwrap();
        assert!(dir.path().join(LOCK_FILE).exists());
    }

    #[test]
    fn a_missing_library_folder_is_an_error_not_a_lock() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(LibraryLock::acquire(&dir.path().join("missing")), Err(Error::Io { .. })));
    }

    #[test]
    fn locking_on_the_ui_thread_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _ui = guard::mark_ui_thread();
        assert!(matches!(LibraryLock::acquire(dir.path()), Err(Error::OnUiThread)));
    }
}
