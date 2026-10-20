//! What the store's targets share: framing bytes as checksummed records, and the snapshot fixed
//! point every loaded model must reach.

use std::path::PathBuf;
use std::sync::OnceLock;

use dalica_library::Library;
use dalica_store::{crc32, snapshot};

/// `<crc32 as 8 hex digits> <json>\n` for each `\n`-separated piece of `body`, the way the journal
/// and the snapshot write a record, so the fuzzer reaches what is inside a record without having
/// to forge its checksum; `torn` leaves the last newline off.
pub fn framed(body: &[u8], torn: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 16);
    for piece in body.split(|&b| b == b'\n') {
        out.extend_from_slice(format!("{:08x} ", crc32(piece)).as_bytes());
        out.extend_from_slice(piece);
        out.push(b'\n');
    }
    if torn {
        out.pop();
    }
    out
}

/// A directory of this process's own for the snapshot round trip.
fn scratch() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dalica-store-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    })
}

/// Writes `library` as a snapshot at `seq`, reads it back and writes that again: the read must
/// succeed with the same op number, and the second file must be the first byte for byte (a
/// snapshot holds only what ops can build, so loading one rebuilds exactly what was written).
pub fn snapshot_fixed_point(library: &Library, seq: u64) {
    let path = scratch().join(snapshot::FILE);
    snapshot::write(&path, library, seq).expect("a snapshot is written");
    let first = std::fs::read(&path).expect("the snapshot reads");
    let (again, at) = snapshot::read(&first).unwrap_or_else(|e| panic!("a written snapshot does not load: {e}"));
    assert_eq!(at, seq);
    snapshot::write(&path, &again, at).expect("a snapshot is written");
    let second = std::fs::read(&path).expect("the snapshot reads");
    assert!(first == second, "loading a snapshot and writing it again changed it");
}
