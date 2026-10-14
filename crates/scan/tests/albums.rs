//! M9's sidecar fields as the walk reads them (`data-model.md` §3, §3.2, `roadmap.md` M9.10) on
//! temp folders: each photo's album memberships with their position keys, the folder's photo sort
//! and the names it gives its albums; entries Dalica cannot use left out; a folder without a
//! sidecar says nothing, and one whose sidecar cannot be read says so; `diff` leaves the album
//! fields to the engine.

use std::path::Path;

use dalica_core::{AlbumId, PosKey};
use dalica_library::{Library, Op, PhotoSort, SortBy};
use dalica_scan::{FoundFolder, scan};
use dalica_scenes::{Scene, jpeg};

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn photos(dir: &Path, names: &[&str]) {
    for (i, n) in names.iter().enumerate() {
        put(&dir.join(n), &jpeg(&Scene::new(i as u64, 32, 24), 80, None).unwrap());
    }
}

fn folder(root: &Path, name: &str) -> FoundFolder {
    scan(&[root.to_path_buf()], None).folders.into_iter().find(|f| f.path.ends_with(name)).unwrap()
}

fn key(s: &str) -> PosKey {
    PosKey::parse(s).unwrap()
}

fn album(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

#[test]
fn the_walk_reads_memberships_the_sort_and_the_album_names() {
    let dir = tempfile::tempdir().unwrap();
    let lisbon = dir.path().join("Lisbon");
    photos(&lisbon, &["a.jpg", "b.jpg", "c.jpg"]);
    let doc = r#"{
        "dalica": { "version": 1 },
        "folder": { "sort": "size", "reverse": true },
        "photos": {
            "a.jpg": { "albums": { "a91c0e": "a1", "a77d02": "bz0", "nope": "a1", "a1234": "!" } },
            "b.jpg": { "albums": "not an object" },
            "c.jpg": { "star": true }
        },
        "albums": { "a91c0e": { "name": " Best\tof 2026 " }, "a77d02": { "name": "" }, "x": { "name": "Bad" } }
    }"#;
    put(&lisbon.join(".dalica.json"), doc.as_bytes());
    let found = folder(dir.path(), "Lisbon");
    let by = |n: &str| found.photos.iter().find(|p| p.name == n).unwrap();
    assert_eq!(by("a.jpg").albums.clone().into_iter().collect::<Vec<_>>(), [(album("a77d02"), key("bz0")), (album("a91c0e"), key("a1"))]);
    assert!(by("b.jpg").albums.is_empty(), "a value of the wrong type gives none");
    assert!(by("c.jpg").albums.is_empty());
    assert!(by("c.jpg").star);
    assert_eq!(found.sort, PhotoSort::new(SortBy::Size, true));
    assert_eq!(
        found.album_names.clone().into_iter().collect::<Vec<_>>(),
        [(album("a91c0e"), "Best of 2026".to_string())],
        "cleaned; empty names left out"
    );
    assert!(!found.sidecar_unread);
}

#[test]
fn a_folder_without_a_sidecar_says_nothing_and_one_that_cannot_be_read_says_so() {
    let dir = tempfile::tempdir().unwrap();
    photos(&dir.path().join("Plain"), &["a.jpg"]);
    photos(&dir.path().join("Damaged"), &["a.jpg"]);
    put(&dir.path().join("Damaged").join(".dalica.json"), b"{ not json");
    let plain = folder(dir.path(), "Plain");
    assert_eq!((plain.sort, plain.album_names.len(), plain.sidecar_unread), (PhotoSort::default(), 0, false));
    let damaged = folder(dir.path(), "Damaged");
    assert!(damaged.sidecar_unread);
    assert!(damaged.photos[0].albums.is_empty());
    // A sort Dalica cannot sort by leaves the default.
    put(&dir.path().join("Plain").join(".dalica.json"), br#"{ "folder": { "sort": "manual" } }"#);
    assert_eq!(folder(dir.path(), "Plain").sort, PhotoSort::default());
}

/// `diff` brings no album op of its own: the engine adds them to the ops that add the folder and
/// its photos (it knows the albums and `albums.json`).
#[test]
fn diff_leaves_the_album_fields_to_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let lisbon = dir.path().join("Lisbon");
    photos(&lisbon, &["a.jpg"]);
    put(&lisbon.join(".dalica.json"), br#"{ "photos": { "a.jpg": { "albums": { "a91c0e": "a1" } } }, "albums": { "a91c0e": { "name": "Trip" } } }"#);
    let s = scan(&[dir.path().to_path_buf()], None);
    let mut lib = Library::new();
    let ops = dalica_scan::diff(&mut lib, &s);
    let mut stack: Vec<&Op> = ops.iter().collect();
    while let Some(op) = stack.pop() {
        match op {
            Op::Batch { ops } => stack.extend(ops),
            Op::CreateAlbums { .. } | Op::AddMembers { .. } | Op::SetFolderSorts { .. } => panic!("diff made an album op: {op:?}"),
            _ => {}
        }
    }
}
