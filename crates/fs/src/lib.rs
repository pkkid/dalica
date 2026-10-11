//! dalica-fs: Every write to the user's disk: atomic replace + fsync + verify, the read-before-write guard, the library lock, move/copy with verify, the OS trash, bounded walks, the UI-thread guard and fault-injection points (feature `faults`).
//!
//! Layer L0; see `docs/architecture.md` §2. Only this crate calls `std::fs` write functions
//! (layer rule 4), so atomicity, verification and fault injection have one home
//! (`docs/security.md` §5).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod append;
pub mod aside;
pub mod atomic;
pub mod dirs;
pub mod faults;
pub mod guard;
pub mod lock;
pub mod pack;
pub mod walk;

use std::io;
use std::path::{Path, PathBuf};

pub use append::{Appender, append, rotate};
pub use aside::set_aside;
pub use atomic::{Expect, Identity, Read, identity, read, replace, replace_private};
pub use dirs::{TempDir, create_dir_all, create_private_dir_all, remove_own_file};
pub use guard::{UiThread, mark_ui_thread};
pub use lock::LibraryLock;
pub use pack::{CacheFile, PackFile, TempFile, list_shaped, remove_shaped};
pub use walk::{Entry, Limits, Walk, Warning, walk};

/// Everything a file operation can refuse or fail with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The OS refused a step. `op` names the step (`"rename"`, `"fsync"`, …).
    #[error("{op} {}: {source}", .path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Called on the thread marked as the UI thread (`guard`), which must never touch the disk.
    #[error("file system call on the UI thread")]
    OnUiThread,
    /// The target is not what the caller read: it changed, appeared or vanished since (`security.md` §5.3).
    #[error("{} changed on disk since it was read", .0.display())]
    Changed(PathBuf),
    /// The target is a symbolic link; writes never go through a link (`security.md` §5.6).
    #[error("{} is a symbolic link; Dalica does not write through links", .0.display())]
    Link(PathBuf),
    /// Re-reading the file after the write did not give back the bytes written.
    #[error("{} did not read back as written", .0.display())]
    VerifyFailed(PathBuf),
    /// A read would exceed the caller's size limit.
    #[error("{} is larger than {limit} bytes", .path.display())]
    TooLarge { path: PathBuf, limit: u64 },
    /// An append would take the file past its size cap; nothing was written.
    #[error("{} would grow past its cap of {cap} bytes", .path.display())]
    CapExceeded { path: PathBuf, cap: u64 },
    /// Another process holds the library lock (M1.3).
    #[error("{} is locked by another Dalica process", .0.display())]
    Locked(PathBuf),
    /// A fault point armed as a crash fired: the operation stopped where a killed process would,
    /// without cleaning up (feature `faults`, tests only).
    #[error("simulated crash at fault point {0}")]
    Crashed(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Wraps an `io::Error` with the step and path it belongs to.
pub(crate) fn io_err<'a>(op: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> Error + 'a {
    move |source| Error::Io { op, path: path.to_path_buf(), source }
}
