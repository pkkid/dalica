//! `dalica-library` tags (`organizing.md` §5.2, `security.md` §4.4): counts per tag, partial counts
//! over a set of photos, prefix lookup, the case rule, renames that merge, the caps, and the build
//! time of a 100 k-photo tag index (at full strength; smaller in a task's gate).

use std::path::PathBuf;
use std::time::Instant;

use dalica_core::{FolderId, PhotoId};
use dalica_library::{
    Error, Folder, Library, MAX_CAPTION, MAX_TAG, MAX_TAGS, Op, PartialCount, Photo, PhotoInfo, TagCount, clean_caption, clean_tag, clean_tags,
};

fn photo(id: PhotoId, folder: FolderId, n: usize) -> Photo {
    Photo { size: 1000, width: 480, height: 320, ..crate::common::photo(id, folder, &format!("IMG_{n:06}.jpg")) }
}

fn library(n: usize) -> (Library, Vec<PhotoId>) {
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: folder, path: PathBuf::from("/photos/a") } }).unwrap();
    let ids: Vec<PhotoId> = (0..n).map(|_| lib.next_photo_id()).collect();
    lib.apply(Op::AddPhotos { photos: ids.iter().enumerate().map(|(i, &id)| photo(id, folder, i)).collect() }).unwrap();
    (lib, ids)
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn add(lib: &mut Library, ids: &[PhotoId], tags: &[&str]) -> Op {
    lib.apply(Op::AddTags { ids: ids.to_vec(), tags: strings(tags) }).unwrap()
}

fn counts(lib: &Library) -> Vec<(String, usize)> {
    lib.tag_index().counts().into_iter().map(|TagCount { tag, count }| (tag, count)).collect()
}

fn pairs(list: &[(&str, usize)]) -> Vec<(String, usize)> {
    list.iter().map(|(t, n)| (t.to_string(), *n)).collect()
}

#[test]
fn counts_per_tag_in_case_insensitive_order() {
    let (mut lib, ids) = library(5);
    add(&mut lib, &ids[..3], &["beach", "Sunset"]);
    add(&mut lib, &ids[2..], &["dog"]);
    assert_eq!(counts(&lib), pairs(&[("beach", 3), ("dog", 3), ("Sunset", 3)]));
    assert_eq!(lib.tag_index().count("BEACH"), 3);
    assert_eq!(lib.tag_index().count("cat"), 0);
    assert_eq!(lib.tag_index().len(), 3);
    assert_eq!(lib.tags_of(ids[2]), strings(&["beach", "Sunset", "dog"]), "appended in the order added");
    // Removing the tags from every photo empties the index.
    lib.apply(Op::RemoveTags { ids: ids.clone(), tags: strings(&["BEACH", "sunset", "Dog"]) }).unwrap();
    assert!(lib.tag_index().is_empty());
    assert!(lib.infos().next().is_none(), "photos with nothing said about them leave the info map");
}

#[test]
fn partial_counts_over_a_selection() {
    let (mut lib, ids) = library(8);
    add(&mut lib, &ids[..3], &["beach"]);
    add(&mut lib, &ids, &["holiday"]);
    add(&mut lib, &ids[7..], &["Zoo"]);
    let got = lib.tag_counts_in(&ids);
    assert_eq!(
        got,
        [
            PartialCount { tag: "beach".into(), count: 3, of: 8 },
            PartialCount { tag: "holiday".into(), count: 8, of: 8 },
            PartialCount { tag: "Zoo".into(), count: 1, of: 8 },
        ]
    );
    assert_eq!(got.iter().map(ToString::to_string).collect::<Vec<_>>(), ["beach (3 of 8)", "holiday", "Zoo (1 of 8)"]);
    assert!(got[0].is_partial() && !got[1].is_partial());
    // Repeated and unknown ids count once and not at all.
    let mixed = [ids[0], ids[0], ids[5], PhotoId::new(9_999)];
    assert_eq!(
        lib.tag_counts_in(&mixed),
        [PartialCount { tag: "beach".into(), count: 1, of: 2 }, PartialCount { tag: "holiday".into(), count: 2, of: 2 }]
    );
    assert!(lib.tag_counts_in(&[]).is_empty());
}

#[test]
fn prefix_lookup_for_autocomplete() {
    let (mut lib, ids) = library(3);
    add(&mut lib, &ids[..1], &["Beach", "beer", "bee", "Berlin", "apple", "Ärger"]);
    add(&mut lib, &ids[1..], &["beach"]);
    let tags = |prefix: &str, limit: usize| lib.tag_index().with_prefix(prefix, limit).into_iter().map(|c| c.tag).collect::<Vec<_>>();
    assert_eq!(tags("be", 10), ["Beach", "bee", "beer", "Berlin"], "case-insensitive, in folded order, the spelling in use");
    assert_eq!(tags("BEE", 10), ["bee", "beer"]);
    assert_eq!(tags("be", 2), ["Beach", "bee"]);
    assert_eq!(tags("är", 10), ["Ärger"]);
    assert_eq!(tags("x", 10), Vec::<String>::new());
    assert_eq!(tags("", 2), ["apple", "Beach"]);
    assert_eq!(lib.tag_index().with_prefix("bea", 5), [TagCount { tag: "Beach".into(), count: 3 }]);
}

#[test]
fn the_case_rule_keeps_the_first_spelling() {
    let (mut lib, ids) = library(4);
    add(&mut lib, &ids[..1], &["Beach"]);
    // Adding another spelling of a tag in use takes the spelling in use.
    add(&mut lib, &ids[1..2], &["beach"]);
    add(&mut lib, &ids[2..3], &["BEACH", "beach"]);
    for id in &ids[..3] {
        assert_eq!(lib.tags_of(*id), ["Beach"]);
    }
    // A photo never holds the same tag twice.
    let undo = add(&mut lib, &ids[..1], &["bEaCh"]);
    assert_eq!(lib.tags_of(ids[0]), ["Beach"]);
    assert_eq!(undo, Op::SetTags { tags: vec![] }, "nothing changed");
    assert_eq!(counts(&lib), pairs(&[("Beach", 3)]));
    assert_eq!(lib.tag_index().spelling("BEACH"), Some("Beach"));
    // Spellings imported from files stay as they are; the index shows the one most photos use.
    lib.apply(Op::SetTags { tags: vec![(ids[3], strings(&["beach"]))] }).unwrap();
    assert_eq!(counts(&lib), pairs(&[("Beach", 4)]));
    lib.apply(Op::SetTags { tags: vec![(ids[0], strings(&["beach"])), (ids[1], strings(&["beach"]))] }).unwrap();
    assert_eq!(counts(&lib), pairs(&[("beach", 4)]));
    // A list holding one tag twice is refused.
    let twice = Op::SetTags { tags: vec![(ids[0], strings(&["dog", "Dog"]))] };
    assert!(matches!(lib.apply(twice), Err(Error::BadText { .. })));
    // `clean_tags` drops later spellings.
    assert_eq!(clean_tags(["Dog", " dog ", "cat", "", "DOG"]), ["Dog", "cat"]);
}

#[test]
fn rename_everywhere_and_onto_an_existing_tag_merges() {
    let (mut lib, ids) = library(4);
    add(&mut lib, &ids[..2], &["beech", "trees"]);
    add(&mut lib, &ids[1..3], &["Beach"]);
    let before = lib.model().clone();
    // A plain rename keeps each tag's place in its list.
    let undo = lib.apply(Op::RenameTag { from: "trees".into(), to: "forest".into() }).unwrap();
    assert_eq!(lib.tags_of(ids[0]), ["beech", "forest"]);
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &before);

    // Renaming "beech" to "beach" merges onto "Beach": the spelling in use is kept, a photo that
    // had both keeps one, and the undo restores every list exactly.
    let undo = lib.apply(Op::RenameTag { from: "BEECH".into(), to: "beach".into() }).unwrap();
    assert_eq!(lib.tags_of(ids[0]), ["Beach", "trees"]);
    assert_eq!(lib.tags_of(ids[1]), ["trees", "Beach"]);
    assert_eq!(lib.tags_of(ids[2]), ["Beach"]);
    assert_eq!(counts(&lib), pairs(&[("Beach", 3), ("trees", 2)]));
    let (merged, index) = (lib.model().clone(), lib.tag_index().clone());
    let redo = lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &before);
    assert_eq!(counts(&lib), pairs(&[("Beach", 2), ("beech", 2), ("trees", 2)]));
    // Redo (the inverse of the undo) gives the merged lists again.
    lib.apply(redo).unwrap();
    assert_eq!((lib.model(), lib.tag_index()), (&merged, &index));

    // A rename in case only respells the tag everywhere.
    lib.apply(Op::RenameTag { from: "beach".into(), to: "BEACH".into() }).unwrap();
    assert_eq!(counts(&lib), pairs(&[("BEACH", 3), ("trees", 2)]));
    // Renaming a tag nobody has changes nothing.
    let model = lib.model().clone();
    lib.apply(Op::RenameTag { from: "nothing".into(), to: "else".into() }).unwrap();
    assert_eq!(lib.model(), &model);
}

#[test]
fn caps_and_clean_text() {
    let (mut lib, ids) = library(1);
    let id = ids[0];
    let before = (lib.model().clone(), lib.revision());
    let long_tag = "t".repeat(MAX_TAG + 1);
    let refused = [
        Op::AddTags { ids: vec![id], tags: vec![long_tag.clone()] },
        Op::AddTags { ids: vec![id], tags: vec!["".into()] },
        Op::AddTags { ids: vec![id], tags: vec![" padded".into()] },
        Op::AddTags { ids: vec![id], tags: vec!["two\nlines".into()] },
        Op::RemoveTags { ids: vec![id], tags: vec!["\t".into()] },
        Op::RenameTag { from: "a".into(), to: "".into() },
        Op::SetTags { tags: vec![(id, (0..=MAX_TAGS).map(|i| format!("t{i}")).collect())] },
        Op::SetCaptions { captions: vec![(id, "c".repeat(MAX_CAPTION + 1))] },
        Op::SetPhotoInfo { photos: vec![(id, PhotoInfo { tags: vec![long_tag.clone()], ..PhotoInfo::default() })] },
    ];
    for op in refused {
        let label = format!("{op:?}");
        assert!(lib.apply(op).is_err(), "{}", &label[..label.len().min(80)]);
        assert_eq!((lib.model().clone(), lib.revision()), before);
    }
    // Adding past the per-photo cap is refused before anything changes.
    let many: Vec<String> = (0..MAX_TAGS).map(|i| format!("t{i}")).collect();
    lib.apply(Op::SetTags { tags: vec![(id, many)] }).unwrap();
    assert!(matches!(lib.apply(Op::AddTags { ids: vec![id], tags: vec!["one more".into()] }), Err(Error::TooMany { .. })));
    assert_eq!(lib.tags_of(id).len(), MAX_TAGS);
    // Exactly at the caps is fine.
    lib.apply(Op::SetCaptions { captions: vec![(id, "c".repeat(MAX_CAPTION))] }).unwrap();
    lib.apply(Op::SetTags { tags: vec![(id, vec!["t".repeat(MAX_TAG)])] }).unwrap();
    // The error never carries the text.
    let err = lib.apply(Op::SetCaptions { captions: vec![(id, "secret ".repeat(MAX_CAPTION))] }).unwrap_err();
    assert!(!err.to_string().contains("secret"), "{err}");

    // The cleaners bring any text inside the caps, cutting at a character boundary.
    assert_eq!(clean_tag("  sea\tside \n"), Some("sea side".into()));
    assert_eq!(clean_tag(" \n\t "), None);
    assert_eq!(clean_tag("a, b/c"), Some("a, b/c".into()), "commas and slashes stay");
    let wide = "é".repeat(MAX_TAG); // two bytes each
    let cut = clean_tag(&wide).unwrap();
    assert_eq!(cut.len(), MAX_TAG);
    assert!(cut.chars().all(|c| c == 'é'));
    let odd = format!("x{}", "é".repeat(MAX_TAG));
    assert_eq!(clean_tag(&odd).unwrap().len(), MAX_TAG - 1, "never inside a character");
    assert_eq!(clean_caption("  Tram 28\nat sunset  "), "Tram 28\nat sunset", "captions keep their line breaks");
    assert_eq!(clean_caption(&"ü".repeat(MAX_CAPTION)).len(), MAX_CAPTION);
    for text in ["", " ", "\u{202e}gpj.exe", "\0", &"x".repeat(100_000)] {
        if let Some(t) = clean_tag(text) {
            lib.apply(Op::SetTags { tags: vec![(id, vec![t])] }).unwrap();
        }
        lib.apply(Op::SetCaptions { captions: vec![(id, clean_caption(text))] }).unwrap();
    }
}

/// At full strength 100 k photos; in a task's gate 4 000 (every word of the vocabulary still used),
/// the same checks.
#[test]
fn a_100k_photo_tag_index_builds_quickly() {
    let n: usize = if crate::common::full() { 100_000 } else { 4_000 };
    const VOCABULARY: usize = 2_000;
    let (mut lib, ids) = library(n);
    // Five tags per photo out of a 2 000-tag vocabulary, some sharing a spelling in other cases.
    let lists: Vec<(PhotoId, Vec<String>)> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let tags = (0..5).map(|k| {
                let t = (i * 7 + k * 389) % VOCABULARY;
                if t.is_multiple_of(10) && i.is_multiple_of(2) { format!("Tag {t}") } else { format!("tag {t}") }
            });
            (id, clean_tags(tags.collect::<Vec<_>>().iter().map(String::as_str)))
        })
        .collect();
    let start = Instant::now();
    lib.apply(Op::SetTags { tags: lists }).unwrap();
    let built = start.elapsed();
    let start = Instant::now();
    let all = lib.tag_index().counts();
    let listed = start.elapsed();
    let start = Instant::now();
    let selection = lib.tag_counts_in(&ids[..n / 10]);
    let partial = start.elapsed();
    let start = Instant::now();
    let hits = lib.tag_index().with_prefix("tag 1", 20);
    let prefix = start.elapsed();
    println!(
        "{n} photos × 5 tags ({} tags): index built in {built:?}, all counts in {listed:?}, partial counts over {} in {partial:?}, prefix lookup in {prefix:?}",
        all.len(),
        n / 10
    );
    assert_eq!(all.len(), VOCABULARY);
    assert_eq!(all.iter().map(|c| c.count).sum::<usize>(), n * 5);
    assert!(!selection.is_empty() && hits.len() == 20);
    // Generous: catches quadratic work even in an unoptimised build on a slow runner.
    assert!(built.as_secs_f64() < 10.0, "building took {built:?}");
    assert!(listed.as_secs_f64() < 1.0 && prefix.as_secs_f64() < 0.1, "{listed:?} {prefix:?}");
}
