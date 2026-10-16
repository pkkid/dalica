//! `library.inspect` → `pendingWrites` counts a sidecar write from the change until its file is
//! committed (`control-protocol.md` §5.8, `roadmap.md` M10.50): a caption still in the sidecar
//! writer's one-second debounce is counted, and once nothing is counted the sidecar holds it. A
//! rebuild started while such a write waits keeps the change (`data-model.md` §10.1).
//!
//! The session runs on a virtual clock, so the debounce ends only when the test moves the clock.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{entry, jpeg, open_with, virtual_clock};
use dalica_core::{PhotoId, VirtualClock};
use dalica_engine::Session;
use dalica_fs::Expect;
use serde_json::json;

/// How long a bounded wait for the I/O worker may take.
const PATIENCE: Duration = Duration::from_secs(60);

/// A library on a virtual clock watching `<disk>/Lisbon` of three photos, scanned, every write
/// after the scan committed: the library's folder, the disk's, the folder, the clock, the session
/// and the photos' ids in name order.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Arc<VirtualClock>, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i), Expect::Absent).unwrap();
    }
    let clock = virtual_clock();
    let mut s = open_with(lib.path(), clock.clone());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.flush().unwrap();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (lib, disk, folder, clock, s, ids.into_iter().map(|(_, id)| id).collect())
}

/// The sidecar writes `pendingWrites` counts (0 when it is `null`).
fn sidecars(s: &Session) -> usize {
    s.pending_writes().map_or(0, |p| p.sidecars)
}

/// The caption the folder's sidecar holds for photo `name`.
fn caption_on_disk(folder: &Path, name: &str) -> Option<String> {
    entry(folder, name)?.get("caption")?.as_str().map(str::to_string)
}

#[test]
fn a_sidecar_write_counts_until_its_file_is_committed() {
    let (_l, _d, folder, clock, mut s, ids) = setup();
    // A star written at once: the folder's next write waits for the debounce.
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.flush().unwrap();
    assert_eq!(sidecars(&s), 0, "the star is written");
    s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "The Forum at dusk" })).unwrap();
    assert_eq!(caption_on_disk(&folder, "IMG_0002.jpg"), None, "still in the debounce");
    assert_eq!(sidecars(&s), 1, "a write in the debounce is counted: {:?}", s.pending_writes());
    // The debounce ends; once the count is back to none the sidecar holds the caption.
    clock.advance(dalica_store::writer::DEBOUNCE);
    let start = Instant::now();
    while sidecars(&s) > 0 {
        assert!(start.elapsed() < PATIENCE, "the write was not committed: {:?}", s.pending_writes());
        std::thread::yield_now();
    }
    assert_eq!(caption_on_disk(&folder, "IMG_0002.jpg").as_deref(), Some("The Forum at dusk"));
}

#[test]
fn a_rebuild_started_while_a_sidecar_write_waits_keeps_it() {
    let (_l, _d, folder, _clock, mut s, ids) = setup();
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.flush().unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "The Forum at dusk" })).unwrap();
    assert_eq!(sidecars(&s), 1, "the caption's write waits");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let rebuilt = s.library().photos().find(|p| p.name == "IMG_0002.jpg").map(|p| p.id).unwrap();
    assert_eq!(s.library().caption(rebuilt), "The Forum at dusk", "the rebuilt model keeps the caption");
    s.flush().unwrap();
    assert_eq!(caption_on_disk(&folder, "IMG_0002.jpg").as_deref(), Some("The Forum at dusk"), "and so does the sidecar");
}
