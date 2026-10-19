//! Legacy data in the model (M14, `data-model.md` §9): albums and people found by their legacy
//! ids, which deletes and merges take and their inverses put back; the derived facts (the sidecar
//! in effect per folder, legacy originals per photo) moving no revision; the folders whose data
//! came from a legacy sidecar; what only legacy data named; the photos offered a legacy Undo Save.

use std::path::PathBuf;

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId, PosKey, Rect01, Timestamp};
use dalica_library::{
    AlbumInfo, ContentDigest, Error, Face, FacePerson, Folder, FolderSidecar, LegacyOriginal, Library, MAX_LEGACY_ID, Op, Person, Project, Saved,
};
use serde_json::json;

use crate::common::photo;

const OLD: FolderId = FolderId::new(1);
const MIXED: FolderId = FolderId::new(2);
const NEW: FolderId = FolderId::new(3);
const A: PhotoId = PhotoId::new(1);
const B: PhotoId = PhotoId::new(2);
const C: PhotoId = PhotoId::new(3);

fn trip() -> AlbumId {
    AlbumId::parse("a0a0a").unwrap()
}

fn best() -> AlbumId {
    AlbumId::parse("a0b0b").unwrap()
}

fn ana() -> PersonId {
    PersonId::parse("p0a0a").unwrap()
}

fn rui() -> PersonId {
    PersonId::parse("p0b0b").unwrap()
}

fn face(person: &PersonId) -> Face {
    Face::new(Rect01::new(0.1, 0.1, 0.3, 0.3).unwrap(), FacePerson::Person(person.clone()))
}

/// Three folders: OLD (legacy sidecar only) holds A, MIXED (both sidecars) holds B, NEW (Dalica's
/// only) holds C. Albums Trip and Best, people Ana and Rui; no legacy ids or facts yet.
fn fixture() -> Library {
    let mut lib = Library::new();
    let ops = vec![
        Op::AddFolder { folder: Folder { id: OLD, path: PathBuf::from("/p/Old") } },
        Op::AddFolder { folder: Folder { id: MIXED, path: PathBuf::from("/p/Mixed") } },
        Op::AddFolder { folder: Folder { id: NEW, path: PathBuf::from("/p/New") } },
        Op::AddPhotos { photos: vec![photo(A, OLD, "a.jpg"), photo(B, MIXED, "b.jpg"), photo(C, NEW, "c.jpg")] },
        Op::CreateAlbums { albums: vec![(trip(), AlbumInfo::named("Trip")), (best(), AlbumInfo::named("Best"))] },
        Op::AddPerson { id: ana(), person: Person::named("Ana") },
        Op::AddPerson { id: rui(), person: Person::named("Rui") },
    ];
    lib.apply(Op::Batch { ops }).unwrap();
    lib
}

fn ids(albums: Vec<(AlbumId, Option<&str>)>, people: Vec<(PersonId, Option<&str>)>) -> Op {
    Op::SetLegacyIds {
        albums: albums.into_iter().map(|(a, l)| (a, l.map(String::from))).collect(),
        people: people.into_iter().map(|(p, l)| (p, l.map(String::from))).collect(),
    }
}

/// An album and a person are found by their legacy ids; setting them is an ordinary op whose
/// inverse forgets them; a deleted album, and a person deleted or merged away, take theirs, and
/// the inverse brings it back; an id held twice, or not clean, is refused and changes nothing.
#[test]
fn an_album_and_a_person_are_found_by_their_legacy_id() {
    let mut lib = fixture();
    let rev = lib.revision();
    let undo = lib.apply(ids(vec![(trip(), Some("4f1c2a"))], vec![(ana(), Some("9e8d7c6b5a493827"))])).unwrap();
    assert_eq!(lib.revision(), rev + 1, "an ordinary op: the ids live in albums.json and people.json");
    assert_eq!(lib.album_by_legacy("4f1c2a"), Some(&trip()));
    assert_eq!(lib.album_legacy(&trip()), Some("4f1c2a"));
    assert_eq!(lib.person_by_legacy("9e8d7c6b5a493827"), Some(&ana()));
    assert_eq!(lib.person_legacy(&ana()), Some("9e8d7c6b5a493827"));
    assert_eq!(lib.album_by_legacy("nope"), None);
    let model = lib.model().clone();
    let redo = lib.apply(undo).unwrap();
    assert_eq!((lib.album_by_legacy("4f1c2a"), lib.person_by_legacy("9e8d7c6b5a493827")), (None, None));
    lib.apply(redo).unwrap();
    assert_eq!(lib.model(), &model);

    // Refusals change nothing.
    let refused = [
        ids(vec![(best(), Some("4f1c2a"))], vec![]),
        ids(vec![], vec![(rui(), Some("9e8d7c6b5a493827"))]),
        ids(vec![(best(), Some(""))], vec![]),
        ids(vec![(best(), Some(" x"))], vec![]),
        ids(vec![], vec![(rui(), Some("a\nb"))]),
        ids(vec![(AlbumId::parse("a0c0c").unwrap(), Some("x"))], vec![]),
    ];
    for op in refused {
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!(lib.model(), &model);
    }
    let long = "x".repeat(MAX_LEGACY_ID + 1);
    assert_eq!(lib.apply(ids(vec![(best(), Some(&long))], vec![])), Err(Error::TooLong { what: "legacy id", max: MAX_LEGACY_ID }));
    // Handing an id from one album to another in one op is fine.
    let swap = lib.apply(ids(vec![(trip(), None), (best(), Some("4f1c2a"))], vec![])).unwrap();
    assert_eq!(lib.album_by_legacy("4f1c2a"), Some(&best()));
    lib.apply(swap).unwrap();
    assert_eq!(lib.model(), &model);

    // A deleted album takes its id; the inverse puts it back.
    let undo = lib.apply(Op::DeleteAlbums { ids: vec![trip()] }).unwrap();
    assert_eq!(lib.album_by_legacy("4f1c2a"), None);
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &model);
    assert_eq!(lib.album_by_legacy("4f1c2a"), Some(&trip()));

    // So do a deleted person and one merged away.
    for op in [Op::DeletePerson { id: ana() }, Op::MergePeople { from: ana(), into: rui() }] {
        lib.apply(Op::SetFaces { faces: vec![(A, vec![face(&ana())])] }).unwrap();
        let model = lib.model().clone();
        let undo = lib.apply(op).unwrap();
        assert_eq!(lib.person_by_legacy("9e8d7c6b5a493827"), None);
        assert_eq!(lib.person_legacy(&rui()), None, "a merge does not hand the id on");
        lib.apply(undo).unwrap();
        assert_eq!(lib.model(), &model);
        assert_eq!(lib.person_by_legacy("9e8d7c6b5a493827"), Some(&ana()));
    }
}

/// The derived facts: the sidecar in effect and legacy originals bump no revision and no folder
/// revision, are derived (never on an undo stack), and their inverses restore them; removing the
/// folder or the photo takes them, and the inverse puts them back. Their journal names.
#[test]
fn the_facts_ops_move_no_revision_and_stay_off_the_undo_stack() {
    let mut lib = fixture();
    let original = LegacyOriginal { size: 7, digest: "d1".into(), recipe: vec!["crop64=1,0,0,1".into()] };
    let facts = [
        Op::SetFolderSidecars { folders: vec![(OLD, FolderSidecar::Legacy), (MIXED, FolderSidecar::Both), (NEW, FolderSidecar::Dalica)] },
        Op::SetLegacyOriginals { originals: vec![(A, Some(original.clone()))] },
    ];
    for op in facts {
        assert!(op.is_derived(), "{op:?}");
        let (rev, folder_rev, empty) = (lib.revision(), lib.folder_revision(OLD), lib.model().clone());
        let undo = lib.apply(op).unwrap();
        assert!(undo.is_derived());
        assert_eq!((lib.revision(), lib.folder_revision(OLD)), (rev, folder_rev));
        let model = lib.model().clone();
        let redo = lib.apply(undo).unwrap();
        assert_eq!(lib.model(), &empty);
        lib.apply(redo).unwrap();
        assert_eq!(lib.model(), &model);
    }
    assert_eq!(lib.folder_sidecar(OLD), FolderSidecar::Legacy);
    assert_eq!(lib.legacy_original(A), Some(&original));
    let model = lib.model().clone();
    let undo = lib.apply(Op::RemoveFolder { id: OLD }).unwrap();
    assert_eq!((lib.folder_sidecar(OLD), lib.legacy_original(A)), (FolderSidecar::None, None));
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &model);
    let undo = lib.apply(Op::RemovePhotos { ids: vec![A] }).unwrap();
    assert_eq!(lib.legacy_original(A), None);
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &model);

    // Refused: unknown folders and photos, a digest past its cap.
    let long = LegacyOriginal { digest: "d".repeat(dalica_library::MAX_DIGEST + 1), ..LegacyOriginal::default() };
    assert!(lib.apply(Op::SetFolderSidecars { folders: vec![(FolderId::new(9), FolderSidecar::Legacy)] }).is_err());
    assert!(lib.apply(Op::SetLegacyOriginals { originals: vec![(PhotoId::new(9), Some(LegacyOriginal::default()))] }).is_err());
    assert!(lib.apply(Op::SetLegacyOriginals { originals: vec![(B, Some(long))] }).is_err());
    assert_eq!(lib.model(), &model);

    // Journal names, and the Collages mark's spelling.
    let names = [
        (ids(vec![], vec![]), "setLegacyIds"),
        (Op::SetFolderSidecars { folders: vec![] }, "setFolderSidecars"),
        (Op::SetLegacyOriginals { originals: vec![] }, "setLegacyOriginals"),
    ];
    for (op, name) in names {
        let value = serde_json::to_value(&op).unwrap();
        assert_eq!(value["op"], name);
        assert_eq!(serde_json::from_value::<Op>(value).unwrap(), op);
    }
    assert_eq!(serde_json::to_value(Project::Collages).unwrap(), json!("collages"));
    assert_eq!(serde_json::to_value(FolderSidecar::Both).unwrap(), json!("both"));
}

/// The folders whose data came from a legacy sidecar (`legacy` or `both`) are each counted once,
/// however often the scan records them.
#[test]
fn the_legacy_folders_are_counted_once_each() {
    let mut lib = fixture();
    assert!(lib.legacy_folders().is_empty());
    for _ in 0..3 {
        lib.apply(Op::SetFolderSidecars { folders: vec![(OLD, FolderSidecar::Legacy), (MIXED, FolderSidecar::Both), (OLD, FolderSidecar::Legacy)] })
            .unwrap();
    }
    lib.apply(Op::SetFolderSidecars { folders: vec![(NEW, FolderSidecar::Dalica)] }).unwrap();
    assert_eq!(lib.legacy_folders(), [OLD, MIXED]);
    lib.apply(Op::SetFolderSidecars { folders: vec![(MIXED, FolderSidecar::Dalica)] }).unwrap();
    assert_eq!(lib.legacy_folders(), [OLD]);
}

/// What only legacy data named: albums and people with a legacy id whose photos all sit in folders
/// whose only sidecar is the legacy one (an empty one too); a member or a face in a folder
/// Dalica's own sidecar describes keeps them, and so does having no legacy id.
#[test]
fn albums_and_people_only_legacy_data_named() {
    let mut lib = fixture();
    let ops = vec![
        Op::SetFolderSidecars { folders: vec![(OLD, FolderSidecar::Legacy), (MIXED, FolderSidecar::Both), (NEW, FolderSidecar::Dalica)] },
        ids(vec![(trip(), Some("t")), (best(), Some("b"))], vec![(ana(), Some("1")), (rui(), Some("2"))]),
        Op::AddMembers { members: vec![(trip(), A, PosKey::parse("a1").unwrap())] },
        Op::SetFaces { faces: vec![(A, vec![face(&ana())])] },
    ];
    lib.apply(Op::Batch { ops }).unwrap();
    assert_eq!(lib.legacy_only_albums(), [trip(), best()], "Best is empty: nothing of Dalica's names it");
    assert_eq!(lib.legacy_only_people(), [ana(), rui()]);

    let ops = vec![
        Op::AddMembers { members: vec![(trip(), B, PosKey::parse("a2").unwrap()), (best(), C, PosKey::parse("a1").unwrap())] },
        Op::SetFaces { faces: vec![(C, vec![face(&ana())])] },
    ];
    lib.apply(Op::Batch { ops }).unwrap();
    assert!(lib.legacy_only_albums().is_empty(), "a member in a folder with Dalica's sidecar");
    assert_eq!(lib.legacy_only_people(), [rui()]);

    lib.apply(ids(vec![], vec![(rui(), None)])).unwrap();
    assert!(lib.legacy_only_people().is_empty(), "a person without a legacy id is Dalica's");
}

/// A photo with a legacy original is offered a legacy Undo Save while its bytes differ (another
/// size, or the same size with a different known digest) and Dalica has no record of its own.
#[test]
fn the_photos_offered_a_legacy_undo_save() {
    let mut lib = fixture();
    let mtime = Timestamp::from_millis(1_700_000_000_000);
    let original = |size: u64, digest: &str| LegacyOriginal { size, digest: digest.into(), recipe: vec![] };
    let ops = vec![
        // A: another size. B: the same size, another digest. C: the same bytes.
        Op::SetLegacyOriginals { originals: vec![(A, Some(original(99, "x"))), (B, Some(original(10, "x"))), (C, Some(original(10, "same")))] },
        Op::SetDigests {
            digests: vec![
                (B, Some(ContentDigest { size: 10, mtime, digest: "y".into() })),
                (C, Some(ContentDigest { size: 10, mtime, digest: "same".into() })),
            ],
        },
    ];
    lib.apply(Op::Batch { ops }).unwrap();
    assert_eq!(lib.legacy_undo_saves(), [A, B]);
    assert!(lib.can_undo_legacy_save(B) && !lib.can_undo_legacy_save(C));

    // The same size with no digest of the file as it is now: Dalica cannot tell yet.
    lib.apply(Op::SetDigests { digests: vec![(B, None)] }).unwrap();
    assert_eq!(lib.legacy_undo_saves(), [A]);

    // Dalica's own record takes precedence.
    lib.apply(Op::SetSaved { saved: vec![(A, Some(Saved::default()))] }).unwrap();
    assert!(lib.legacy_undo_saves().is_empty());
}
