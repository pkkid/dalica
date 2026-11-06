//! Colour summaries in the index (M16, `organizing.md` §8.1): derived data the journal and the
//! snapshot keep (the snapshot under `colours`, as `[photo id, byte]` pairs), never a sidecar. A
//! snapshot without the key reads as one with no summaries, and a byte with a bit the form does
//! not know is dropped with a note, never refusing the snapshot.

use std::path::PathBuf;

use dalica_core::color_summary::{ColourSummary, SearchColour};
use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_store::mapping::{PHOTO_DATA_M16, Source};
use dalica_store::{Store, snapshot};

use crate::common::NOW;

const A: PhotoId = PhotoId::new(1);
const B: PhotoId = PhotoId::new(2);

fn summaries() -> Vec<(PhotoId, Option<ColourSummary>)> {
    vec![
        (A, Some(ColourSummary::new(SearchColour::Red, &[SearchColour::Blue]).unwrap())),
        (B, Some(ColourSummary::new(SearchColour::Bw, &[]).unwrap())),
    ]
}

fn ops() -> Vec<Op> {
    let folder = FolderId::new(1);
    let photo = |id, name: &str| Photo {
        id,
        folder,
        name: name.into(),
        size: 10,
        mtime: Timestamp::from_millis(5),
        width: 4,
        height: 3,
        date: None,
        star: false,
    };
    vec![
        Op::AddFolder { folder: Folder { id: folder, path: PathBuf::from("/p/Trip") } },
        Op::AddPhotos { photos: vec![photo(A, "a.jpg"), photo(B, "b.jpg")] },
        Op::SetColours { colours: summaries() },
    ]
}

/// The same record with its checksum, as the snapshot file holds it.
fn line(json: &str) -> Vec<u8> {
    let mut out = format!("{:08x} ", dalica_store::crc32(json.as_bytes())).into_bytes();
    out.extend_from_slice(json.as_bytes());
    out.push(b'\n');
    out
}

#[test]
fn summaries_survive_a_journal_replay_and_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = Store::open(dir.path(), NOW).unwrap();
    let mut lib = Library::new();
    for op in ops() {
        lib.apply(op.clone()).unwrap();
        o.store.append(&op).unwrap();
    }
    drop(o);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((o.report.clone(), o.library.model()), (vec![], lib.model()), "the journal replays the summaries");
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    let text = std::fs::read_to_string(dir.path().join(snapshot::FILE)).unwrap();
    let bytes: Vec<String> = summaries().iter().map(|(id, c)| format!("[{id},{}]", c.unwrap().to_byte())).collect();
    assert!(text.contains(&format!("\"colours\":[{}]", bytes.join(","))), "{text}");
    let o = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((o.report.clone(), o.store.seq()), (vec![], o.store.snapshot_seq()));
    assert_eq!(o.library.model(), lib.model());
    assert_eq!(o.library.colours(A), summaries()[0].1);
}

#[test]
fn a_snapshot_without_summaries_reads_and_a_bad_byte_is_dropped_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(snapshot::FILE);
    let mut lib = Library::new();
    for op in ops().into_iter().take(2) {
        lib.apply(op).unwrap();
    }
    snapshot::write(&path, &lib, 2).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("colours"), "left out when none");
    let (back, seq, notes) = snapshot::read_noted(&bytes).unwrap();
    assert_eq!((back.model(), seq, notes), (lib.model(), 2, Vec::<String>::new()));
    assert!(!back.has_colours());

    // A snapshot written before M16 (no key) reads as one without summaries.
    let base = r#"{"snapshot":1,"seq":2,"folders":[{"id":1,"path":"/p"}],"photos":[{"id":1,"folder":1,"name":"a.jpg","size":1,"mtime":0,"width":1,"height":1,"date":null,"star":false},{"id":2,"folder":1,"name":"b.jpg","size":1,"mtime":0,"width":1,"height":1,"date":null,"star":false}]"#;
    let (old, ..) = snapshot::read_noted(&line(&format!("{base}}}"))).unwrap();
    assert!(!old.has_colours());

    // Bits for other hues beside `bw` (0b1000_0111) are not a summary: dropped with a note, the
    // good one kept.
    let red = summaries()[0].1.unwrap();
    let bad = line(&format!(r#"{base},"colours":[[1,{}],[2,135]]}}"#, red.to_byte()));
    let (back, _, notes) = snapshot::read_noted(&bad).unwrap();
    assert_eq!((back.colours(A), back.colours(B)), (Some(red), None));
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("photo 2") && notes[0].contains("135"), "{notes:?}");
    assert_eq!(snapshot::read(&bad).unwrap().0.model(), back.model(), "read keeps what read_noted keeps");
}

/// Nothing M16.9 adds is silently dropped: the summary is placed, index-only like the sketch.
#[test]
fn the_colour_summary_has_a_place() {
    assert_eq!(PHOTO_DATA_M16, [("colours", Source::Index)]);
}
