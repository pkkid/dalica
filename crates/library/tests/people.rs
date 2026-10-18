//! Faces and people in the model (`organizing.md` §6, `faces.md` §5–§6): each op and its inverse,
//! merging and deleting people, the case rule on names, the people queries, `person:` and the
//! Faces filter in search, and the search index following every people op.

use std::path::PathBuf;

use dalica_core::{DateTime, FolderId, PersonId, PhotoId, Rect01, Timestamp};
use dalica_library::search::{Extract, Filters, Query, Scope, SearchIndex, Update};
use dalica_library::{
    Error, Face, FaceKey, FacePerson, FaceRef, Folder, Library, MAX_FACES, Op, PeopleSort, Person, Photo, clean_person_name, person_id_from,
};

use crate::common::photo;

const LISBON: FolderId = FolderId::new(1);
const PORTO: FolderId = FolderId::new(2);
const A: PhotoId = PhotoId::new(1);
const B: PhotoId = PhotoId::new(2);
const C: PhotoId = PhotoId::new(3);
const D: PhotoId = PhotoId::new(4);

fn ana() -> PersonId {
    PersonId::parse("p0a0a").unwrap()
}

fn rui() -> PersonId {
    PersonId::parse("p0b0b").unwrap()
}

fn rect(x: f64) -> Rect01 {
    Rect01::new(x, 0.1, x + 0.1, 0.3).unwrap()
}

fn named(x: f64, id: &PersonId) -> Face {
    Face::new(rect(x), FacePerson::Person(id.clone()))
}

fn unnamed(x: f64) -> Face {
    Face::new(rect(x), FacePerson::Unnamed)
}

/// A photo large enough to show with Small Pictures off.
fn big(id: PhotoId, folder: FolderId, name: &str) -> Photo {
    Photo { width: 4000, height: 3000, ..photo(id, folder, name) }
}

fn day(d: u8) -> Option<DateTime> {
    DateTime::new(dalica_core::Date::new(2026, 6, d).unwrap(), 12, 0, 0).ok()
}

/// Lisbon holds A (June 3), B (June 1) and C (June 2); Porto holds D (no date, a 2023 file time).
/// Ana ("Nita") is on A and B, Rui on B; A and D have an unnamed face; C only an ignored one.
fn fixture() -> Library {
    let mut lib = Library::new();
    let ops = vec![
        Op::AddFolder { folder: Folder { id: LISBON, path: PathBuf::from("/p/Lisbon") } },
        Op::AddFolder { folder: Folder { id: PORTO, path: PathBuf::from("/p/Porto") } },
        Op::AddPhotos {
            photos: vec![
                Photo { date: day(3), ..big(A, LISBON, "a.jpg") },
                Photo { date: day(1), ..big(B, LISBON, "b.jpg") },
                Photo { date: day(2), ..big(C, LISBON, "c.jpg") },
                big(D, PORTO, "d.jpg"),
            ],
        },
        Op::AddPerson { id: ana(), person: Person { nickname: "Nita".into(), ..Person::named("Ana Costa") } },
        Op::AddPerson { id: rui(), person: Person::named("Rui") },
        Op::SetFaces {
            faces: vec![
                (A, vec![named(0.1, &ana()), unnamed(0.5)]),
                (B, vec![named(0.1, &rui()), named(0.5, &ana())]),
                (C, vec![Face::new(rect(0.2), FacePerson::Ignored)]),
                (D, vec![unnamed(0.3)]),
            ],
        },
    ];
    for op in ops {
        lib.apply(op).unwrap();
    }
    lib
}

/// Applying each op changes the model and bumps the revision once; its inverse restores the model
/// exactly, and the inverse of the inverse is the op again.
#[test]
fn each_op_and_its_inverse_restore_the_model() {
    let mut lib = fixture();
    let thumb = FaceKey { photo: B, rect: rect(0.5) };
    let ops = vec![
        Op::SetFaces { faces: vec![(A, vec![named(0.1, &ana()), named(0.5, &rui())]), (D, vec![])] },
        Op::SetFaces { faces: vec![(C, vec![unnamed(0.2), unnamed(0.6)])] },
        Op::AddPerson { id: PersonId::parse("p0c0c").unwrap(), person: Person::named("Zé") },
        Op::RenamePerson { id: ana(), name: "Ana C.".into(), nickname: String::new() },
        Op::SetPersonThumbnail { id: ana(), thumbnail: Some(thumb) },
        Op::MergePeople { from: rui(), into: ana() },
        Op::DeletePerson { id: ana() },
        Op::RemovePhotos { ids: vec![A] },
        Op::RemoveFolder { id: LISBON },
        Op::Batch {
            ops: vec![Op::AddPerson { id: rui(), person: Person::named("Rui") }, Op::SetFaces { faces: vec![(D, vec![named(0.1, &rui())])] }],
        },
    ];
    for op in ops {
        let before = lib.model().clone();
        let revision = lib.revision();
        let inverse = lib.apply(op.clone()).unwrap_or_else(|e| panic!("{op:?}: {e}"));
        assert_ne!(lib.model(), &before, "{op:?} changed nothing");
        assert_eq!(lib.revision(), revision + 1);
        let after = lib.model().clone();
        let again = lib.apply(inverse).unwrap();
        assert_eq!(lib.model(), &before, "the inverse of {op:?} restores the model");
        lib.apply(again).unwrap();
        assert_eq!(lib.model(), &after, "the inverse of the inverse of {op:?} is the op");
    }
}

/// Merging moves every face of one person to the other, whose name is kept; deleting a person
/// sends their faces to Unnamed; both come back with Undo.
#[test]
fn merging_moves_every_face_and_deleting_sends_faces_to_unnamed() {
    let mut lib = fixture();
    let undo = lib.apply(Op::MergePeople { from: rui(), into: ana() }).unwrap();
    assert!(lib.person(&rui()).is_none());
    assert_eq!(lib.person(&ana()).unwrap().name, "Ana Costa", "the target's name is kept");
    assert_eq!(lib.faces(B), [named(0.1, &ana()), named(0.5, &ana())]);
    assert_eq!(lib.person_photos(&ana()), [B, A]);
    assert!(lib.faces_all().all(|(_, faces)| faces.iter().all(|f| f.person.person() != Some(&rui()))));
    lib.apply(undo).unwrap();
    assert_eq!(lib.faces(B), [named(0.1, &rui()), named(0.5, &ana())]);
    assert_eq!(lib.person(&rui()).unwrap().name, "Rui");

    let before = lib.unnamed_faces().len();
    let undo = lib.apply(Op::DeletePerson { id: ana() }).unwrap();
    assert!(lib.person(&ana()).is_none() && lib.person_photos(&ana()).is_empty());
    assert_eq!(lib.faces(A), [unnamed(0.1), unnamed(0.5)]);
    assert_eq!(lib.faces(B), [named(0.1, &rui()), unnamed(0.5)], "only the deleted person's faces go");
    assert_eq!(lib.unnamed_count(), before + 2);
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), fixture().model());
}

/// Two people's names may not be equal ignoring case; a person may change the case of their own.
/// Faces name only people the model has, at most 256 to a photo; a thumbnail is one of the
/// person's faces. A refused op changes nothing.
#[test]
fn names_equal_ignoring_case_and_other_refusals() {
    let mut lib = fixture();
    let model = lib.model().clone();
    let new = PersonId::parse("p0d0d").unwrap();
    let ghost = PersonId::parse("p0e0e").unwrap();
    let too_many: Vec<Face> = (0..=MAX_FACES).map(|_| unnamed(0.1)).collect();
    for (op, want) in [
        (Op::AddPerson { id: new.clone(), person: Person::named("ANA COSTA") }, Error::PersonNameTaken(ana())),
        (Op::AddPerson { id: new.clone(), person: Person::named("rui") }, Error::PersonNameTaken(rui())),
        (Op::RenamePerson { id: rui(), name: "ana costa".into(), nickname: String::new() }, Error::PersonNameTaken(ana())),
        (Op::AddPerson { id: ana(), person: Person::named("Someone") }, Error::PersonExists(ana())),
        (Op::RenamePerson { id: ghost.clone(), name: "X".into(), nickname: String::new() }, Error::UnknownPerson(ghost.clone())),
        (Op::SetFaces { faces: vec![(A, vec![named(0.1, &ghost)])] }, Error::UnknownPerson(ghost.clone())),
        (Op::SetFaces { faces: vec![(A, too_many)] }, Error::TooMany { what: "faces on a photo", max: MAX_FACES }),
        (Op::SetFaces { faces: vec![(PhotoId::new(99), vec![])] }, Error::UnknownPhoto(PhotoId::new(99))),
        (Op::MergePeople { from: ana(), into: ana() }, Error::MergeIntoItself(ana())),
        (Op::MergePeople { from: ghost.clone(), into: ana() }, Error::UnknownPerson(ghost.clone())),
        (Op::DeletePerson { id: ghost.clone() }, Error::UnknownPerson(ghost.clone())),
        (
            Op::SetPersonThumbnail { id: rui(), thumbnail: Some(FaceKey { photo: A, rect: rect(0.1) }) },
            Error::NotPersonsFace { person: rui(), photo: A },
        ),
        (
            Op::Batch { ops: vec![Op::DeletePerson { id: rui() }, Op::AddPerson { id: new.clone(), person: Person::named("Ana costa") }] },
            Error::PersonNameTaken(ana()),
        ),
    ] {
        assert_eq!(lib.apply(op.clone()), Err(want), "{op:?}");
        assert_eq!(lib.model(), &model, "{op:?} changed nothing");
    }
    for bad in ["", " Ana", "Ana\n", "A\u{7}na"] {
        let err = lib.apply(Op::AddPerson { id: new.clone(), person: Person::named(bad) }).unwrap_err();
        assert!(matches!(err, Error::BadText { what: "person name", .. }), "{bad:?}: {err}");
    }
    assert!(lib.apply(Op::AddPerson { id: new.clone(), person: Person { nickname: " x".into(), ..Person::named("Zé") } }).is_err());
    // A name an error never shows: errors name people by id.
    assert!(!Error::PersonNameTaken(ana()).to_string().contains("Ana"));
    // The case of one's own name may change; the name is then found under any case.
    lib.apply(Op::RenamePerson { id: ana(), name: "ANA COSTA".into(), nickname: "Nita".into() }).unwrap();
    assert_eq!(lib.person_by_name("ana costa"), Some(&ana()));
    assert_eq!(lib.person_by_name("Ana"), None);
    // Clean names: control characters made spaces, trimmed, cut at a character boundary.
    assert_eq!(clean_person_name("  Ana\tCosta \n").as_deref(), Some("Ana Costa"));
    assert_eq!(clean_person_name(" \u{7} "), None);
    assert_eq!(clean_person_name(&"é".repeat(600)).map(|n| n.len()), Some(1024));
    // New ids: `p` and 12 hex digits, drawn again on a clash.
    let mut draws = [0x0a0a_u64, 0x0a0a, 0x1234].into_iter();
    let taken = PersonId::parse("p000000000a0a").unwrap();
    lib.apply(Op::AddPerson { id: taken, person: Person::named("Taken") }).unwrap();
    assert_eq!(lib.new_person_id(|| draws.next().unwrap_or(0)).unwrap().as_str(), "p000000001234");
    assert_eq!(person_id_from(u64::MAX).unwrap().as_str(), "pffffffffffff");
}

/// The person album by date, Unnamed and ignored faces in photo order, counts, the two sorts,
/// the photos with faces and the thumbnail (the one set while it is theirs, else the first face).
#[test]
fn people_queries() {
    let mut lib = fixture();
    assert_eq!(lib.person_photos(&ana()), [B, A], "by date: B (June 1), then A (June 3)");
    assert_eq!(lib.person_faces(&ana()), [FaceRef { photo: B, index: 1 }, FaceRef { photo: A, index: 0 }]);
    assert_eq!(lib.person_photos(&rui()), [B]);
    assert_eq!(lib.unnamed_faces(), [FaceRef { photo: D, index: 0 }, FaceRef { photo: A, index: 1 }], "D has a 2023 file time");
    assert_eq!(lib.unnamed_count(), 2);
    assert_eq!(lib.ignored_faces(), [FaceRef { photo: C, index: 0 }]);
    assert_eq!(lib.photos_with_faces(), [D, B, A], "C has an ignored face only");
    let counts: Vec<(PersonId, usize)> = lib.person_counts().into_iter().map(|(id, n)| (id.clone(), n)).collect();
    assert_eq!(counts, [(ana(), 2), (rui(), 1)]);
    let zed = PersonId::parse("p0c0c").unwrap();
    lib.apply(Op::AddPerson { id: zed.clone(), person: Person::named("albert") }).unwrap();
    assert_eq!(lib.people_sorted(PeopleSort::Name), [zed.clone(), ana(), rui()], "ignoring case");
    assert_eq!(lib.people_sorted(PeopleSort::Count), [ana(), rui(), zed.clone()]);
    assert_eq!(lib.person_count(&zed), 0);
    assert_eq!(lib.person_thumbnail(&zed), None);
    // The thumbnail: the first face until one is set; a set one that stops being theirs falls back.
    assert_eq!(lib.person_thumbnail(&ana()), Some(FaceRef { photo: B, index: 1 }));
    lib.apply(Op::SetPersonThumbnail { id: ana(), thumbnail: Some(FaceKey { photo: A, rect: rect(0.1) }) }).unwrap();
    assert_eq!(lib.person_thumbnail(&ana()), Some(FaceRef { photo: A, index: 0 }));
    lib.apply(Op::SetFaces { faces: vec![(A, vec![unnamed(0.5), named(0.1, &ana())])] }).unwrap();
    assert_eq!(lib.person_thumbnail(&ana()), Some(FaceRef { photo: A, index: 1 }), "the same face where it now is");
    lib.apply(Op::SetFaces { faces: vec![(A, vec![unnamed(0.5)])] }).unwrap();
    assert_eq!(lib.person_thumbnail(&ana()), Some(FaceRef { photo: B, index: 1 }));
    assert_eq!(lib.face(FaceRef { photo: B, index: 1 }), Some(&named(0.5, &ana())));
    assert_eq!(lib.face(FaceRef { photo: B, index: 2 }), None);
}

fn search(lib: &Library, query: &Query) -> Vec<PhotoId> {
    let index = SearchIndex::from_library(lib);
    let mut found = index.search(query, &Scope::default()).ids;
    found.sort();
    assert_eq!(found, Extract::from_library(lib).matching(query, &Scope::default()), "index and matcher disagree on {query:?}");
    found
}

/// `person:` finds the photos of the people whose name or nickname a word starts, and plain words
/// find them too; the Faces filter keeps the photos with a named or unnamed face. Neither is
/// reported unavailable once the library has people and faces.
#[test]
fn person_search_and_the_faces_filter() {
    let lib = fixture();
    assert_eq!(search(&lib, &Query::text("person:ana")), [A, B]);
    assert_eq!(search(&lib, &Query::text("person:Costa")), [A, B]);
    assert_eq!(search(&lib, &Query::text("person:nita")), [A, B], "the nickname");
    assert_eq!(search(&lib, &Query::text("person:rui")), [B]);
    assert_eq!(search(&lib, &Query::text("person:ana person:rui")), [B]);
    assert_eq!(search(&lib, &Query::text("-person:rui")), [A, C, D]);
    assert_eq!(search(&lib, &Query::text("person:zé")), Vec::<PhotoId>::new());
    assert_eq!(search(&lib, &Query::text("rui")), [B], "plain words find names");
    assert_eq!(search(&lib, &Query::text("person:a.jpg")), Vec::<PhotoId>::new(), "person: looks at names only");
    let faces = Query { filters: Filters { faces: true, ..Filters::default() }, ..Query::default() };
    assert_eq!(search(&lib, &faces), [A, B, D], "C has an ignored face only");
    let r = SearchIndex::from_library(&lib).search(&Query { text: "person:ana".into(), ..faces }, &Scope::default());
    assert!(r.unavailable.is_empty(), "{:?}", r.unavailable);
}

/// An index kept up to date op by op (and inverse by inverse) equals one built from scratch.
#[test]
fn the_search_index_follows_people_ops() {
    let mut lib = fixture();
    let mut index = SearchIndex::from_library(&lib);
    let ops = vec![
        Op::RenamePerson { id: rui(), name: "Rui Silva".into(), nickname: "Ruizinho".into() },
        Op::SetFaces { faces: vec![(C, vec![named(0.2, &rui())]), (D, vec![])] },
        Op::MergePeople { from: rui(), into: ana() },
        Op::DeletePerson { id: ana() },
        Op::AddPerson { id: rui(), person: Person::named("Rui") },
        Op::RemovePhotos { ids: vec![B] },
    ];
    for op in ops {
        let inverse = lib.apply(op.clone()).unwrap();
        index.apply(Update::for_op(&lib, &op));
        assert!(index == SearchIndex::from_library(&lib), "after {op:?}");
        let again = lib.apply(inverse.clone()).unwrap();
        index.apply(Update::for_op(&lib, &inverse));
        assert!(index == SearchIndex::from_library(&lib), "after undoing {op:?}");
        lib.apply(again.clone()).unwrap();
        index.apply(Update::for_op(&lib, &again));
    }
    assert!(index == SearchIndex::from_library(&lib));
}

/// Changing faces stamps the photos whose faces change, a person merged away or deleted the photos
/// holding their faces; the journal names the ops and reads them back.
#[test]
fn faces_stamp_the_last_changed_time_and_journal_by_name() {
    let mut lib = fixture();
    let now = Timestamp::from_millis(1_790_000_000_000);
    let op = lib.stamped(Op::DeletePerson { id: ana() }, now);
    lib.apply(op).unwrap();
    let changed: Vec<PhotoId> = lib.last_changed_all().map(|(id, _)| id).collect();
    assert_eq!(changed, [A, B]);
    let op = lib.stamped(Op::SetFaces { faces: vec![(D, vec![])] }, now);
    lib.apply(op).unwrap();
    assert_eq!(lib.last_changed(D), Some(now));
    assert_eq!(
        lib.stamped(Op::AddPerson { id: ana(), person: Person::named("Ana") }, now),
        Op::AddPerson { id: ana(), person: Person::named("Ana") }
    );
    for (op, name) in [
        (Op::SetFaces { faces: vec![(A, vec![named(0.1, &rui())])] }, "setFaces"),
        (Op::AddPerson { id: ana(), person: Person { thumbnail: Some(FaceKey { photo: A, rect: rect(0.1) }), ..Person::named("Ana") } }, "addPerson"),
        (Op::RenamePerson { id: ana(), name: "A".into(), nickname: "N".into() }, "renamePerson"),
        (Op::MergePeople { from: rui(), into: ana() }, "mergePeople"),
        (Op::DeletePerson { id: ana() }, "deletePerson"),
        (Op::SetPersonThumbnail { id: ana(), thumbnail: None }, "setPersonThumbnail"),
    ] {
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], name);
        assert_eq!(serde_json::from_value::<Op>(json).unwrap(), op);
    }
}
