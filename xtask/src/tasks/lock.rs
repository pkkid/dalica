//! Local locks in the git common directory, shared by every worktree on this machine
//! (`docs/task-board.md` §7). The OS releases the lock when the file is closed, even after a crash.

use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct Lock {
    _file: File,
}

impl Lock {
    /// Blocks until the lock at `path` is ours.
    pub fn acquire(path: &Path) -> Result<Lock, String> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("cannot open lock {}: {e}", path.display()))?;
        file.lock().map_err(|e| format!("cannot lock {}: {e}", path.display()))?;
        Ok(Lock { _file: file })
    }
}
