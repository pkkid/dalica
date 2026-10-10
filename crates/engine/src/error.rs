//! Errors of commands and of opening a session. Command errors carry the control protocol's error
//! code (`control-protocol.md` §3), so every surface reports them the same way.

use std::path::PathBuf;

/// Why a command did not run (or did not finish).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CmdError {
    #[error("no command `{0}`")]
    UnknownCommand(String),
    /// A missing, extra or ill-typed parameter; the message names it.
    #[error("bad parameters: {0}")]
    BadParams(String),
    /// The command cannot run now; the message is what the status line shows.
    #[error("{0}")]
    Disabled(String),
    /// An unknown photo, folder or path.
    #[error("{0}")]
    NotFound(String),
    /// A size or count over its limit.
    #[error("{0}")]
    Limit(String),
    /// The change was applied in memory but could not be recorded on disk.
    #[error("{0}")]
    Unsaved(String),
    /// A panic caught by the dispatch guard; the session keeps running.
    #[error("internal error: {0}")]
    Internal(String),
}

impl CmdError {
    /// The control protocol's error code.
    pub fn code(&self) -> &'static str {
        match self {
            CmdError::UnknownCommand(_) => "unknown_command",
            CmdError::BadParams(_) => "bad_params",
            CmdError::Disabled(_) => "disabled",
            CmdError::NotFound(_) => "not_found",
            CmdError::Limit(_) => "limit",
            CmdError::Unsaved(_) => "unsaved",
            CmdError::Internal(_) => "internal",
        }
    }
}

/// Why a session could not open.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// Another Dalica process has this library open.
    #[error("the library at {} is open in another Dalica process", .0.display())]
    Locked(PathBuf),
    #[error(transparent)]
    Fs(dalica_fs::Error),
    #[error(transparent)]
    Store(#[from] dalica_store::Error),
    #[error("cannot start the background writer: {0}")]
    Thread(String),
}

impl From<dalica_fs::Error> for OpenError {
    fn from(e: dalica_fs::Error) -> OpenError {
        match e {
            dalica_fs::Error::Locked(p) => OpenError::Locked(p),
            other => OpenError::Fs(other),
        }
    }
}
