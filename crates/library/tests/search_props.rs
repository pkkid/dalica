//! `dalica-library` search properties (`organizing.md` §8.1): an index kept in step through random
//! op sequences (every op of `tests/ops.rs` in the mix, their inverses too) always equals one built
//! from scratch, and its answers always equal the reference matcher's, photo by photo; any text at
//! all is bounded by the grammar's caps and never panics.
//!
//! The op generator is `tests/seed.rs`, shared with `tests/ops.rs`.

use dalica_library::search::{Age, Extract, Filters, MAX_TERM, MAX_WORDS, Parsed, Query, Scope, SearchIndex, Update};
use dalica_library::{Library, Op};
use proptest::prelude::*;

use crate::seed::*;

/// Queries over the generator's vocabulary: tags, captions, album and folder names, file names,
/// dates, cameras, operators and their combinations.
const QUERIES: [&str; 24] = [
    "",
    "beach",
    "BEA",
    "sea side",
    "-beach",
    "tag:sea",
    "-tag:beach dog",
    "tram",
    "sunset -tram",
    "n1",
    "n1.jpg",
    "jpg -n3",
    "f1",
    "sub",
    "lisbon",
    "best",
    "trams -x",
    "ünï",
    "2026",
    "2020-09",
    "september",
    "camera:canon",
    "camera:z -nikon",
    "person:ana -beach",
];

fn scopes() -> [Scope; 3] {
    [
        Scope::default(),
        Scope { hidden_pictures: true, ..Scope::default() },
        Scope { hidden_pictures: true, small_pictures: true, small_threshold: 500, ..Scope::default() },
    ]
}

/// A library to start from: four folders (one nested), twelve photos with captions, tags, dates,
/// hidden flags and facts, folder infos and two albums, so every op has something to act on.
fn start() -> Library {
    let mut lib = Library::new();
    let seeds = [
        Seed::AddFolder(0),
        Seed::AddFolder(1),
        Seed::AddFolder(6),
        Seed::AddFolder(3),
        Seed::AddPhotos((0..12).map(|n| (n % 4, n)).collect()),
        Seed::SetPhotoInfo((0..12).map(|n| (n, n, vec![n, n + 3])).collect()),
        Seed::SetFolderInfo(vec![(0, 1), (1, 3), (2, 2), (3, 0)]),
        Seed::SetFacts((0..12).map(|n| (n, n)).collect()),
        Seed::CreateAlbums(vec![(0, 0), (1, 1)]),
        Seed::AddMembers(vec![(0, 0, 0), (0, 5, 1), (0, 7, 2), (1, 2, 0), (1, 5, 1)]),
    ];
    for s in &seeds {
        let op = build(&mut lib, s);
        lib.apply(op).unwrap();
    }
    lib
}

/// Every query, filter and age agree between the index and the reference matcher.
fn check_answers(lib: &Library, index: &SearchIndex, filters: Filters, age: Option<u8>) -> Result<(), TestCaseError> {
    let extract = Extract::from_library(lib);
    for scope in scopes() {
        for q in QUERIES {
            let query = Query { text: q.into(), filters, age: age.map(|d| Age { days: u32::from(d) * 20, now: time(30) }) };
            let want = extract.matching(&query, &scope);
            prop_assert_eq!(index.matching(&query, &scope), want.clone(), "{:?}", query);
            let mut got = index.search(&query, &scope).ids;
            got.sort();
            prop_assert_eq!(got, want);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    #[test]
    fn an_updated_index_equals_a_fresh_one(
        seeds in proptest::collection::vec(seed(), 1..32),
        undo in proptest::collection::vec(any::<bool>(), 32),
        filters in (any::<bool>(), any::<bool>(), any::<bool>()),
        age in proptest::option::of(any::<u8>()),
    ) {
        let mut lib = start();
        let mut index = SearchIndex::from_library(&lib);
        prop_assert_eq!(index.len(), 12);
        let mut inverses: Vec<Op> = Vec::new();
        for (i, s) in seeds.iter().enumerate() {
            let op = build(&mut lib, s);
            if let Ok(inverse) = lib.apply(op.clone()) {
                index.apply(Update::for_op(&lib, &op));
                inverses.push(inverse);
            }
            if undo.get(i).copied().unwrap_or(false) && let Some(inverse) = inverses.pop() {
                lib.apply(inverse.clone()).unwrap();
                index.apply(Update::for_op(&lib, &inverse));
            }
            prop_assert!(index == SearchIndex::from_library(&lib), "after {:?}", s);
        }
        let filters = Filters { starred: filters.0, videos: filters.1, geotagged: filters.2, faces: false };
        check_answers(&lib, &index, filters, age)?;
        check_answers(&lib, &index, Filters::default(), None)?;
    }

    #[test]
    fn any_text_is_bounded_and_never_panics(text in ".{0,400}", words in proptest::collection::vec("[-a-z:._ \u{202e}\u{0}]{0,12}", 0..200)) {
        let mut lib = Library::new();
        for s in [Seed::AddFolder(1), Seed::AddPhotos(vec![(0, 1), (0, 2), (0, 3)]), Seed::SetPhotoInfo(vec![(0, 1, vec![0, 3]), (1, 2, vec![4])])] {
            let op = build(&mut lib, &s);
            lib.apply(op).unwrap();
        }
        let index = SearchIndex::from_library(&lib);
        let joined = words.join(" ");
        for t in [text.as_str(), joined.as_str()] {
            let p = Parsed::new(t);
            prop_assert!(p.atoms.len() <= MAX_WORDS);
            prop_assert!(p.atoms.iter().flat_map(|a| &a.tokens).all(|t| !t.is_empty() && t.len() <= MAX_TERM));
            let query = Query::text(t);
            prop_assert_eq!(index.matching(&query, &Scope::default()), Extract::from_library(&lib).matching(&query, &Scope::default()));
            let _ = index.search(&query, &Scope::default());
            prop_assert!(index.suggest(t, &Scope::default(), 1000).len() <= 100);
        }
    }
}
