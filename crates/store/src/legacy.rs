//! Legacy data on disk (M14, `data-model.md` §3–§4, §9): the first write of a folder whose data
//! came from the previous organizer's sidecar, and the shape of the legacy ids kept in
//! `albums.json` and `people.json`.
//!
//! **The first write** ([`first_write`]) of a folder whose fact is
//! [`FolderSidecar::Legacy`](dalica_library::FolderSidecar::Legacy) (only the legacy sidecar was
//! there when the scan read it): a new `.dalica.json` holding everything the model has for the
//! folder (every photo, not only the one that changed), so switching files never loses data.
//!
//! 1. A `.dalica.json` that is there already appeared since the scan read the folder (another
//!    program, or another Dalica): it wins. Nothing is written, and the folder is to be read again
//!    ([`FirstWrite::Appeared`]).
//! 2. The model's state is printed into a new sidecar, parsed back and the state applied to the
//!    copy again: the bytes must come out the same, or nothing is written.
//! 3. The file is created under its exact name by `fs`'s create that never replaces
//!    ([`dalica_fs::exact_file`]): a verified temporary file committed in one step the OS makes
//!    exclusive, so a `.dalica.json` that appears at any moment before the commit is kept byte for
//!    byte ([`FirstWrite::Appeared`]); a crash leaves no file or the whole verified one.
//!
//! The legacy sidecar itself is never opened here: legacy files are read-only (never written,
//! renamed or deleted), and it stays byte-identical with its time.

use dalica_core::Timestamp;
use dalica_library::MAX_LEGACY_ID;
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::mapping::{self, FolderState};

/// What [`first_write`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstWrite {
    /// `.dalica.json` was created with the folder's whole state: the folder now has both files.
    Converted,
    /// A `.dalica.json` is there that this write did not make: nothing was written; it wins and
    /// the folder is to be read again.
    Appeared,
    /// The model has nothing to say about the folder: no file was made (it stays legacy-only).
    Nothing,
    /// Nothing was written, and why (a disk error, or the printed state did not read back).
    Failed(String),
}

/// What a legacy-only folder's write meant, as [`crate::SidecarWriter::take_legacy`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyWrite {
    /// Its first `.dalica.json` was written: its fact is now `both`.
    Converted,
    /// A `.dalica.json` appeared since the scan read it: nothing was written; read it again.
    Appeared,
}

/// Writes the first `.dalica.json` of the legacy-only folder `state` (see the module docs). The
/// folder must be there (the writer checks it first). Any thread but the UI thread may call it.
pub fn first_write(state: &FolderState, _now: Timestamp) -> FirstWrite {
    let path = state.path.join(FILE_NAME);
    match dalica_fs::identity(&path) {
        Ok(None) => {}
        Ok(Some(_)) => return FirstWrite::Appeared,
        // A link in its place is not Dalica's file either: never written through.
        Err(dalica_fs::Error::Link(_)) => return FirstWrite::Appeared,
        Err(e) => return FirstWrite::Failed(e.to_string()),
    }
    let mut sidecar = Sidecar::new();
    let _refused = mapping::apply(state, &mut sidecar);
    if sidecar == Sidecar::new() {
        return FirstWrite::Nothing;
    }
    let bytes = sidecar.print();
    if let Err(why) = reads_back(state, &bytes) {
        return FirstWrite::Failed(format!("{}: the converted data did not read back ({why}); nothing was written", path.display()));
    }
    match dalica_fs::exact_file(&state.path, FILE_NAME, &bytes) {
        Ok(_) => FirstWrite::Converted,
        Err(dalica_fs::Error::Exists(_) | dalica_fs::Error::Link(_)) => FirstWrite::Appeared,
        Err(e) => FirstWrite::Failed(e.to_string()),
    }
}

/// Whether `bytes` parse back into a sidecar that already says `state`: applying the state to the
/// parsed copy changes nothing.
fn reads_back(state: &FolderState, bytes: &[u8]) -> Result<(), String> {
    let mut back = Sidecar::parse(bytes, &Limits::default()).map_err(|e| e.to_string())?;
    let _refused = mapping::apply(state, &mut back);
    if back.print() == bytes { Ok(()) } else { Err("it differs from the model".into()) }
}

/// `id` when it has the shape of a legacy id the library takes: not empty, at most
/// [`MAX_LEGACY_ID`] bytes, no control characters or surrounding white space.
pub fn clean_id(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= MAX_LEGACY_ID && id.trim() == id && !id.chars().any(char::is_control)).then_some(id)
}
