//! Rebuilding user data from the folders (`data-model.md` §10.1). The index is a cache: after it is
//! lost, a scan rebuilds the folders and photos (crate `scan`), and [`import_sidecars`] reads each
//! folder's `.dalica.json` to give back what the user set — today, stars. Nothing a sidecar holds
//! is lost by deleting the index.

use dalica_core::{FolderId, Timestamp};
use dalica_library::{Library, Op};
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};

use crate::mapping;
use crate::writer::{self, Outcome};

/// What the folders' sidecars say, as ops on the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// Ops that make the model's user data match the sidecars (empty if it already does).
    pub ops: Vec<Op>,
    /// Folders whose sidecar was damaged: kept aside, folder read-only.
    pub read_only: Vec<FolderId>,
    /// Folders whose sidecar could not be read, or held values of the wrong type, and why.
    pub problems: Vec<(FolderId, String)>,
}

/// Reads the sidecar of every folder in `library` and returns the ops that apply its user data.
/// Any thread but the UI thread may call it.
pub fn import_sidecars(library: &Library, now: Timestamp) -> Import {
    let limits = Limits::default();
    let mut out = Import::default();
    let mut stars = Vec::new();
    for folder in library.folders() {
        let path = folder.path.join(FILE_NAME);
        let sidecar = match dalica_fs::read(&path, limits.max_bytes as u64) {
            Ok(None) => continue,
            Ok(Some(read)) => match Sidecar::parse(&read.bytes, &limits) {
                Ok(s) => s,
                Err(e) => {
                    out.problems.push((folder.id, e.to_string()));
                    if let Outcome::SetAside(_) = writer::set_aside(&path, now) {
                        out.read_only.push(folder.id);
                    }
                    continue;
                }
            },
            Err(e) => {
                out.problems.push((folder.id, e.to_string()));
                continue;
            }
        };
        for p in sidecar.problems() {
            out.problems.push((folder.id, format!("{} should be {}", p.pointer, p.expected)));
        }
        for photo in library.photos_in(folder.id) {
            if let Some(star) = mapping::star_of(&sidecar, &photo.name)
                && star != photo.star
            {
                stars.push((photo.id, star));
            }
        }
    }
    if !stars.is_empty() {
        out.ops.push(Op::SetStars { stars });
    }
    out
}
