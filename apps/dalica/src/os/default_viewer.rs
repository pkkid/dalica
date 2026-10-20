//! Dalica as the default viewer (`viewing.md` §3.3): the default handler of a file type and
//! setting Dalica as it, per OS. M15.24 builds it; until then both answer that it is not
//! available, and the window keeps the File Types tab greyed with that reason.

use dalica_engine::os::{DefaultSet, Handler, OsError};
use dalica_scan::FileType;

use super::Native;

const NOT_YET: &str = "Setting the default viewer arrives with OS integration.";

/// The application that opens `ty` by default.
pub fn default_handler(_os: &Native, _ty: FileType) -> Result<Option<Handler>, OsError> {
    Err(OsError::Unavailable(NOT_YET.into()))
}

/// Makes Dalica the default viewer for `types`.
pub fn set_default_viewer(_os: &Native, _types: &[FileType]) -> Result<DefaultSet, OsError> {
    Err(OsError::Unavailable(NOT_YET.into()))
}
