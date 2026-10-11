//! The index snapshot, `index.snap`: the whole model at one op number, as a single checksummed
//! record (`<crc32> <json>\n`), written atomically. Loading it applies one batch op to an empty
//! library, so a snapshot can only hold what ops can build.
//!
//! Sketches (derived data, `data-model.md` §4) are a `sketches` list of `[photo id, base64]` pairs,
//! left out when there are none. A snapshot written before sketches existed reads as one without
//! them, and a reader that does not know the key ignores it, so the version stays 1.

use std::path::Path;

use dalica_core::PhotoId;
use dalica_library::{Folder, Library, Op, Photo, Sketch};
use serde::{Deserialize, Serialize};

use crate::journal::{decode, encode, write_whole};
use crate::{Error, Result};

/// The snapshot's file name in the library directory.
pub const FILE: &str = "index.snap";
/// The format version this crate writes and reads.
pub const VERSION: u64 = 1;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    snapshot: u64,
    /// Ops `1..=seq` are in it.
    seq: u64,
    folders: Vec<Folder>,
    photos: Vec<Photo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sketches: Vec<(PhotoId, Sketch)>,
}

/// Writes the model of `library` as of op `seq`.
pub fn write(path: &Path, library: &Library, seq: u64) -> Result<()> {
    let snap = Snapshot {
        snapshot: VERSION,
        seq,
        folders: library.folders().cloned().collect(),
        photos: library.photos().cloned().collect(),
        sketches: library.sketches().map(|(id, s)| (id, *s)).collect(),
    };
    let json = serde_json::to_vec(&snap).map_err(|e| Error::Json(e.to_string()))?;
    write_whole(path, &encode(&json))
}

/// The library and op number a snapshot holds, or why it cannot be used.
pub fn read(bytes: &[u8]) -> std::result::Result<(Library, u64), String> {
    let line = bytes.strip_suffix(b"\n").ok_or("the snapshot is cut off")?;
    let json = decode(line)?;
    let snap: Snapshot = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    if snap.snapshot != VERSION {
        return Err(format!("snapshot version {} (this Dalica reads {VERSION})", snap.snapshot));
    }
    let mut library = Library::new();
    let mut ops: Vec<Op> = snap.folders.into_iter().map(|folder| Op::AddFolder { folder }).collect();
    ops.push(Op::AddPhotos { photos: snap.photos });
    if !snap.sketches.is_empty() {
        ops.push(Op::SetSketches { sketches: snap.sketches.into_iter().map(|(id, s)| (id, Some(s))).collect() });
    }
    library.apply(Op::Batch { ops }).map_err(|e| format!("inconsistent snapshot: {e}"))?;
    Ok((library, snap.seq))
}
