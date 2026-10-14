//! `dalica-library` search (`organizing.md` §8.1–§8.2): the grammar per operator and in
//! combination, the prefix and case rules, every date spelling, the filters, the age, hidden
//! photos, the light-table order and the sections of a result, the banner's counts, suggestions,
//! updates photo by photo, hostile queries, and the times to build and to query 100 k photos.

use std::path::PathBuf;
use std::time::Instant;

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::search::{
    Age, Extract, Field, Filters, MAX_QUERY, MAX_SUGGESTIONS, MAX_TERM, MAX_WORDS, Parsed, Query, Scope, SearchIndex, SectionCount, SuggestionKind,
    Update,
};
use dalica_library::{
    AlbumInfo, AlbumRef, AlbumTree, Facts, Folder, FolderInfo, Library, Op, Photo, PhotoInfo, PhotoSort, Section, SectionFilter, SortBy, Tree,
    TreeOptions, section_photos,
};

const LISBON: FolderId = FolderId::new(1);
const BEACH: FolderId = FolderId::new(2);
const TRAM: PhotoId = PhotoId::new(1);
const TILES: PhotoId = PhotoId::new(2);
const SWIM: PhotoId = PhotoId::new(3);
const DOG: PhotoId = PhotoId::new(4);

fn date(y: u16, m: u8, d: u8) -> Option<DateTime> {
    DateTime::new(dalica_core::Date::new(y, m, d).unwrap(), 12, 0, 0).ok()
}

fn album() -> AlbumId {
    AlbumId::parse("a0b0c0").unwrap()
}

#[allow(clippy::too_many_arguments)]
fn photo(id: PhotoId, folder: FolderId, name: &str, size: u64, w: u32, h: u32, date: Option<DateTime>, star: bool) -> Photo {
    Photo { id, folder, name: name.into(), size, mtime: Timestamp::from_millis(1_780_000_000_000), width: w, height: h, date, star }
}

fn info(caption: &str, tags: &[&str], hidden: bool) -> PhotoInfo {
    PhotoInfo { caption: caption.into(), tags: tags.iter().map(|t| t.to_string()).collect(), hidden, ..PhotoInfo::default() }
}

/// Two folders, four photos, one album:
/// - Lisbon (place "Lisbon, Portugal", description "Old town walks"): IMG_0001.jpg (2026-06-04,
///   4000 × 3000, starred, "Tram 28 at sunset", tags tram and Lisbon, Canon EOS 5D) and
///   IMG_0002.jpg (2026-07-01, 3000 × 2000, tag tiles, NIKON Z 6, geotagged);
/// - Beach_Day: sea-side.mov (2025-12-25, 1920 × 1080, a video, "Christmas swim", tag beach) and
///   dog.png (no date, 800 × 600, hidden);
/// - the album "Best of Lisbon" holds IMG_0001.jpg and sea-side.mov.
fn fixture() -> Library {
    let mut lib = Library::new();
    let ops = vec![
        Op::AddFolder { folder: Folder { id: LISBON, path: PathBuf::from("/photos/Lisbon") } },
        Op::AddFolder { folder: Folder { id: BEACH, path: PathBuf::from("/photos/Beach_Day") } },
        Op::AddPhotos {
            photos: vec![
                photo(TRAM, LISBON, "IMG_0001.jpg", 4_000_000, 4000, 3000, date(2026, 6, 4), true),
                photo(TILES, LISBON, "IMG_0002.jpg", 3_000_000, 3000, 2000, date(2026, 7, 1), false),
                photo(SWIM, BEACH, "sea-side.mov", 50_000_000, 1920, 1080, date(2025, 12, 25), false),
                photo(DOG, BEACH, "dog.png", 200_000, 800, 600, None, false),
            ],
        },
        Op::SetPhotoInfo {
            photos: vec![
                (TRAM, info("Tram 28 at sunset", &["tram", "Lisbon"], false)),
                (TILES, info("", &["tiles"], false)),
                (SWIM, info("Christmas swim", &["beach"], false)),
                (DOG, info("", &[], true)),
            ],
        },
        Op::SetFolderInfo {
            folders: vec![(LISBON, FolderInfo { description: "Old town walks".into(), place: "Lisbon, Portugal".into(), ..FolderInfo::default() })],
        },
        Op::SetFacts {
            facts: vec![
                (TRAM, Facts { make: "Canon".into(), model: "EOS 5D".into(), video: false, geotagged: false }),
                (TILES, Facts { make: "NIKON CORPORATION".into(), model: "Z 6".into(), video: false, geotagged: true }),
                (SWIM, Facts { make: String::new(), model: String::new(), video: true, geotagged: false }),
            ],
        },
        Op::CreateAlbums { albums: vec![(album(), AlbumInfo::named("Best of Lisbon"))] },
        Op::AddMembers { members: vec![(album(), TRAM, PosKey::parse("a0").unwrap()), (album(), SWIM, PosKey::parse("a1").unwrap())] },
    ];
    for op in ops {
        lib.apply(op).unwrap();
    }
    lib
}

/// The matches of `text` under the default scope, by id, checked against the reference matcher.
fn find(index: &SearchIndex, lib: &Library, query: &Query, scope: &Scope) -> Vec<PhotoId> {
    let ids = index.matching(query, scope);
    assert_eq!(ids, Extract::from_library(lib).matching(query, scope), "index and matcher disagree on {query:?}");
    let mut found = index.search(query, scope).ids;
    found.sort();
    assert_eq!(found, ids);
    ids
}

fn text(index: &SearchIndex, lib: &Library, text: &str) -> Vec<PhotoId> {
    find(index, lib, &Query::text(text), &Scope::default())
}

#[test]
fn an_empty_query_matches_everything_shown() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(index.len(), 4);
    for blank in ["", "   ", "\t\n", "...", "-", "--", "tag:", "-camera:"] {
        assert_eq!(text(&index, &lib, blank), vec![TRAM, TILES, SWIM], "{blank:?}");
    }
    assert!(!Query::text("").is_active() && !Query::text("  ").is_active() && Query::text("x").is_active());
    let hidden = Scope { hidden_pictures: true, ..Scope::default() };
    assert_eq!(find(&index, &lib, &Query::default(), &hidden), vec![TRAM, TILES, SWIM, DOG]);
}

#[test]
fn words_match_prefixes_ignoring_case_in_every_field() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    // Caption and tag.
    assert_eq!(text(&index, &lib, "tram"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "TRA"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "Sunset"), vec![TRAM]);
    // A prefix only: the middle of a word does not match.
    assert_eq!(text(&index, &lib, "unset"), Vec::<PhotoId>::new());
    // File names split at dot, underscore and dash.
    assert_eq!(text(&index, &lib, "img"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "0002"), vec![TILES]);
    assert_eq!(text(&index, &lib, "img_0002.jpg"), vec![TILES]);
    assert_eq!(text(&index, &lib, "side"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "sea-side"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "mov"), vec![SWIM]);
    // Folder name, place and description; album names.
    assert_eq!(text(&index, &lib, "beach_day"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "portugal"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "walks"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "best"), vec![TRAM, SWIM]);
    assert_eq!(text(&index, &lib, "lis"), vec![TRAM, TILES, SWIM]);
    // Camera make and model, pixel size.
    assert_eq!(text(&index, &lib, "nikon"), vec![TILES]);
    assert_eq!(text(&index, &lib, "eos"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "4000x3000"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "4000x"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "1920"), vec![SWIM]);
    // Unicode case.
    let mut lib = lib;
    lib.apply(Op::SetCaptions { captions: vec![(TILES, "ÉVORA Azulejos".into())] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "évo"), vec![TILES]);
    assert_eq!(text(&index, &lib, "AZUL"), vec![TILES]);
}

#[test]
fn several_words_must_all_match_and_a_dash_excludes() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "lisbon tram"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "img nikon"), vec![TILES]);
    assert_eq!(text(&index, &lib, "tram beach"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "-tram"), vec![TILES, SWIM]);
    assert_eq!(text(&index, &lib, "lis -tram"), vec![TILES, SWIM]);
    assert_eq!(text(&index, &lib, "lis -tram -mov"), vec![TILES]);
    assert_eq!(text(&index, &lib, "-zzz"), vec![TRAM, TILES, SWIM]);
    // A negated word of two tokens excludes photos matching both.
    assert_eq!(text(&index, &lib, "-img_0001"), vec![TILES, SWIM]);
    // A dash inside a word splits it; it does not negate.
    assert_eq!(text(&index, &lib, "christmas-swim"), vec![SWIM]);
}

#[test]
fn tag_and_camera_look_at_their_field_only() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    // "lis" finds the folder and the album too; tag:lis only the tag.
    assert_eq!(text(&index, &lib, "tag:lis"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "TAG:Tram"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "tag:img"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "-tag:beach"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "tag:tiles img"), vec![TILES]);
    assert_eq!(text(&index, &lib, "camera:canon"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "camera:z"), vec![TILES]);
    assert_eq!(text(&index, &lib, "camera:corp"), vec![TILES]);
    assert_eq!(text(&index, &lib, "camera:tram"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "-camera:canon tag:t"), vec![TILES]);
}

#[test]
fn person_and_color_match_nothing_and_are_reported() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    let p = Parsed::new("person:ana color:green tram");
    assert_eq!(p.unavailable, vec!["person", "color"]);
    assert_eq!(p.atoms.iter().map(|a| a.field).collect::<Vec<_>>(), vec![Field::Person, Field::Color, Field::Any]);
    assert_eq!(text(&index, &lib, "person:ana"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "color:green tram"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "-person:ana"), vec![TRAM, TILES, SWIM]);
    let r = index.search(&Query::text("tram person:ana"), &Scope::default());
    assert!(r.ids.is_empty());
    assert_eq!(r.unavailable, vec!["person"]);
    // An operator without a value is reported but constrains nothing yet.
    let r = index.search(&Query::text("color: tram"), &Scope::default());
    assert_eq!((r.ids, r.unavailable), (vec![TRAM], vec!["color"]));
    // Faces matches nothing until people exist.
    let faces = Query { filters: Filters { faces: true, ..Filters::default() }, ..Query::default() };
    let r = index.search(&faces, &Scope::default());
    assert!(r.ids.is_empty() && r.unavailable == vec!["faces"]);
}

#[test]
fn an_unknown_field_is_searched_as_plain_words() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    let p = Parsed::new("img:0001");
    assert_eq!(p.atoms.len(), 1);
    assert_eq!((p.atoms[0].field, p.atoms[0].tokens.clone()), (Field::Any, vec!["img".to_string(), "0001".to_string()]));
    assert!(p.unavailable.is_empty());
    assert_eq!(text(&index, &lib, "img:0001"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "foo:tram"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "-img:0001"), vec![TILES, SWIM]);
    assert_eq!(text(&index, &lib, "12:30"), Vec::<PhotoId>::new());
}

#[test]
fn every_date_spelling() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "2026"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "2025"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "2026-06"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "2026-07"), vec![TILES]);
    assert_eq!(text(&index, &lib, "2026-06-04"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "2026-07-01"), vec![TILES]);
    assert_eq!(text(&index, &lib, "2026-06-05"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "2025-12"), vec![SWIM]);
    // Being typed.
    assert_eq!(text(&index, &lib, "2026-"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "2026-0"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "2026-06-0"), vec![TRAM]);
    // English month names, whole or a prefix, any case.
    assert_eq!(text(&index, &lib, "june"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "Jul"), vec![TILES]);
    assert_eq!(text(&index, &lib, "DECEMBER"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "ju"), vec![TRAM, TILES]);
    assert_eq!(text(&index, &lib, "june 2026"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "2026 -june"), vec![TILES]);
    // A date the user set wins over the file's.
    let mut lib = lib;
    lib.apply(Op::SetDates { dates: vec![(TILES, date(2024, 3, 9))] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "2024-03-09"), vec![TILES]);
    assert_eq!(text(&index, &lib, "march"), vec![TILES]);
    assert_eq!(text(&index, &lib, "2026"), vec![TRAM]);
}

#[test]
fn filters_combine_with_each_other_and_with_text() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    let with = |filters: Filters, t: &str| find(&index, &lib, &Query { text: t.into(), filters, age: None }, &Scope::default());
    let starred = Filters { starred: true, ..Filters::default() };
    let videos = Filters { videos: true, ..Filters::default() };
    let geotagged = Filters { geotagged: true, ..Filters::default() };
    assert_eq!(with(starred, ""), vec![TRAM]);
    assert_eq!(with(videos, ""), vec![SWIM]);
    assert_eq!(with(geotagged, ""), vec![TILES]);
    assert_eq!(with(starred, "lis"), vec![TRAM]);
    assert_eq!(with(videos, "lis"), vec![SWIM]);
    assert_eq!(with(videos, "-best"), Vec::<PhotoId>::new());
    assert_eq!(with(Filters { starred: true, geotagged: true, ..Filters::default() }, ""), Vec::<PhotoId>::new());
}

#[test]
fn the_age_counts_days_back_from_now() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    // 2026-07-10 12:00 UTC.
    let now = Timestamp::from_millis(1_783_684_800_000);
    let aged = |days: u32, t: &str| {
        find(&index, &lib, &Query { text: t.into(), filters: Filters::default(), age: Some(Age { days, now }) }, &Scope::default())
    };
    assert_eq!(aged(30, ""), vec![TILES]);
    assert_eq!(aged(40, ""), vec![TRAM, TILES]);
    assert_eq!(aged(40, "tram"), vec![TRAM]);
    assert_eq!(aged(365, ""), vec![TRAM, TILES, SWIM]);
    assert_eq!(aged(0, ""), Vec::<PhotoId>::new());
    // A span reaching before year 1 keeps everything.
    assert_eq!(aged(u32::MAX, ""), vec![TRAM, TILES, SWIM]);
    // The hidden photo without a date ages by its file time (2026-05-28).
    let scope = Scope { hidden_pictures: true, ..Scope::default() };
    let q = Query { text: String::new(), filters: Filters::default(), age: Some(Age { days: 50, now }) };
    assert_eq!(find(&index, &lib, &q, &scope), vec![TRAM, TILES, DOG]);
}

#[test]
fn hidden_and_small_photos_only_when_asked() {
    let mut lib = fixture();
    lib.apply(Op::AddPhotos { photos: vec![photo(PhotoId::new(5), BEACH, "icon.png", 900, 32, 32, None, false)] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "dog"), Vec::<PhotoId>::new());
    assert_eq!(text(&index, &lib, "icon"), Vec::<PhotoId>::new());
    let all = Scope { hidden_pictures: true, small_pictures: true, ..Scope::default() };
    assert_eq!(find(&index, &lib, &Query::text("dog"), &all), vec![DOG]);
    assert_eq!(find(&index, &lib, &Query::text("icon"), &all), vec![PhotoId::new(5)]);
    // A hidden folder hides its photos.
    lib.apply(Op::SetFoldersHidden { hidden: vec![(LISBON, true)] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, "img"), Vec::<PhotoId>::new());
    assert_eq!(find(&index, &lib, &Query::text("img"), &all), vec![TRAM, TILES]);
}

/// A bigger library for the order: three folders with different photo sorts and a reverse.
fn ordered() -> Library {
    let mut lib = Library::new();
    let folders = [(1, "/p/a"), (2, "/p/b"), (3, "/p/c")];
    for (id, path) in folders {
        lib.apply(Op::AddFolder { folder: Folder { id: FolderId::new(id), path: PathBuf::from(path) } }).unwrap();
    }
    let mut photos = Vec::new();
    for i in 0..30u64 {
        let folder = FolderId::new(i % 3 + 1);
        let name = format!("{}-pic{}.jpg", ["Zed", "alpha", "Mid"][(i % 3) as usize], (i * 7) % 10);
        photos.push(photo(PhotoId::new(i + 1), folder, &format!("{i}-{name}"), (i * 37) % 11, 640, 480, date(2026, 1 + (i % 12) as u8, 1), false));
    }
    lib.apply(Op::AddPhotos { photos }).unwrap();
    lib.apply(Op::SetFolderSorts {
        sorts: vec![(FolderId::new(2), PhotoSort::new(SortBy::Name, true)), (FolderId::new(3), PhotoSort::new(SortBy::Size, false))],
    })
    .unwrap();
    lib.apply(Op::SetFolderInfo {
        folders: vec![(FolderId::new(1), FolderInfo { date: dalica_core::Date::new(2020, 1, 1).ok(), ..FolderInfo::default() })],
    })
    .unwrap();
    lib.apply(Op::CreateAlbums { albums: vec![(album(), AlbumInfo::named("Picks"))] }).unwrap();
    let op = lib.album_add_op(&album(), &[PhotoId::new(9), PhotoId::new(2), PhotoId::new(4)], None).unwrap();
    lib.apply(op).unwrap();
    lib.apply(Op::SetStars { stars: vec![(PhotoId::new(4), true), (PhotoId::new(5), true)] }).unwrap();
    lib
}

#[test]
fn results_come_in_light_table_order_with_their_sections_and_counts() {
    let lib = ordered();
    let index = SearchIndex::from_library(&lib);
    for reverse in [false, true] {
        let options = TreeOptions { reverse, ..TreeOptions::default() };
        let roots = [PathBuf::from("/p")];
        let tree = Tree::build(&lib, &roots, &options);
        let albums = AlbumTree::build(&lib, &options);
        let sections = albums.sections_with(&tree);
        let scope = Scope::from_options(&options, sections.clone());
        for q in ["", "pic", "alpha", "-zed", "2026-0", "pic -mid", "picks"] {
            let r = index.search(&Query::text(q), &scope);
            // What the light table would show in search mode: the folder sections with the filter.
            let filtered = TreeOptions { filter: Some(std::sync::Arc::new(SectionFilter::new(r.ids.iter().copied()))), ..options.clone() };
            let folder_sections: Vec<Section> = sections.iter().filter(|s| matches!(s, Section::Folder(_))).cloned().collect();
            let expected: Vec<PhotoId> = section_photos(&lib, &folder_sections, &filtered).into_iter().flat_map(|(_, ids)| ids).collect();
            assert_eq!(r.ids, expected, "{q:?} reverse {reverse}");
            // The sections: the search album, then those of the scope with a match.
            let mut want: Vec<SectionCount> = Vec::new();
            if !r.ids.is_empty() {
                want.push(SectionCount { section: Section::Album(AlbumRef::Search), count: r.ids.len() });
            }
            for (s, ids) in section_photos(&lib, &sections, &filtered) {
                if !ids.is_empty() && s != Section::Album(AlbumRef::Search) {
                    want.push(SectionCount { section: s, count: ids.len() });
                }
            }
            assert_eq!(r.sections, want, "{q:?}");
            // The banner's numbers.
            let folders: std::collections::BTreeSet<FolderId> = r.ids.iter().map(|id| lib.photo(*id).unwrap().folder).collect();
            assert_eq!(r.folders, folders.len());
            assert_eq!(r.bytes, r.ids.iter().map(|id| lib.photo(*id).unwrap().size).sum::<u64>());
            assert_eq!(r.count(), r.ids.len());
        }
    }
    // With no sections given, folders come by path.
    let r = index.search(&Query::text("pic"), &Scope::default());
    let firsts: Vec<FolderId> = r.sections.iter().filter_map(|s| if let Section::Folder(f) = s.section { Some(f) } else { None }).collect();
    assert_eq!(firsts, vec![FolderId::new(1), FolderId::new(2), FolderId::new(3)]);
}

#[test]
fn recently_updated_counts_only_with_a_clock() {
    let lib = ordered();
    let index = SearchIndex::from_library(&lib);
    let recent = vec![Section::Album(AlbumRef::Recent), Section::Album(AlbumRef::Starred)];
    let now = Timestamp::from_millis(1_780_000_000_000 + 86_400_000);
    let scope = Scope { sections: recent.clone(), now: Some(now), ..Scope::default() };
    let r = index.search(&Query::text("pic"), &scope);
    assert_eq!(r.sections.get(1), Some(&SectionCount { section: Section::Album(AlbumRef::Recent), count: 30 }));
    assert_eq!(r.sections.get(2), Some(&SectionCount { section: Section::Album(AlbumRef::Starred), count: 2 }));
    let r = index.search(&Query::text("pic"), &Scope { sections: recent, ..Scope::default() });
    assert!(r.sections.iter().all(|s| s.section != Section::Album(AlbumRef::Recent)));
}

#[test]
fn changes_show_after_their_photos_update() {
    let mut lib = fixture();
    let mut index = SearchIndex::from_library(&lib);
    /// Applies `op` to the library and its update to the index, which then equals a fresh one.
    fn apply(lib: &mut Library, index: &mut SearchIndex, op: Op) -> Op {
        let inverse = lib.apply(op.clone()).unwrap();
        index.apply(Update::for_op(lib, &op));
        assert_eq!(*index, SearchIndex::from_library(lib), "after {op:?}");
        inverse
    }
    // A tag renamed everywhere.
    apply(&mut lib, &mut index, Op::RenameTag { from: "tram".into(), to: "Streetcar".into() });
    assert_eq!(text(&index, &lib, "tag:streetcar"), vec![TRAM]);
    assert_eq!(text(&index, &lib, "tag:tram"), Vec::<PhotoId>::new());
    // A caption changed.
    apply(&mut lib, &mut index, Op::SetCaptions { captions: vec![(TILES, "Yellow funicular".into())] });
    assert_eq!(text(&index, &lib, "funicular"), vec![TILES]);
    // A folder renamed: its photos follow, with one record changed.
    let update = {
        let op = Op::MoveFolder { id: BEACH, path: PathBuf::from("/photos/Algarve") };
        lib.apply(op.clone()).unwrap();
        Update::for_op(&lib, &op)
    };
    assert!(update.photos.is_empty() && update.folders.len() == 1);
    index.apply(update);
    assert_eq!(text(&index, &lib, "algarve"), vec![SWIM]);
    assert_eq!(text(&index, &lib, "beach_day"), Vec::<PhotoId>::new());
    // An album renamed, a photo added to it, then the album deleted and brought back by undo.
    apply(&mut lib, &mut index, Op::SetAlbumInfo { albums: vec![(album(), AlbumInfo::named("Favourites"))] });
    assert_eq!(text(&index, &lib, "fav"), vec![TRAM, SWIM]);
    apply(&mut lib, &mut index, Op::AddMembers { members: vec![(album(), TILES, PosKey::parse("a2").unwrap())] });
    assert_eq!(text(&index, &lib, "fav"), vec![TRAM, TILES, SWIM]);
    let undo = apply(&mut lib, &mut index, Op::DeleteAlbums { ids: vec![album()] });
    assert_eq!(text(&index, &lib, "fav"), Vec::<PhotoId>::new());
    apply(&mut lib, &mut index, undo);
    assert_eq!(text(&index, &lib, "fav"), vec![TRAM, TILES, SWIM]);
    // A photo moved and renamed; a star; facts read by a scan; a folder removed and restored.
    apply(&mut lib, &mut index, Op::MovePhotos { moves: vec![(DOG, LISBON, "puppy.png".into())] });
    let all = Scope { hidden_pictures: true, ..Scope::default() };
    assert_eq!(find(&index, &lib, &Query::text("puppy portugal"), &all), vec![DOG]);
    apply(&mut lib, &mut index, Op::SetStars { stars: vec![(SWIM, true)] });
    let starred = Query { filters: Filters { starred: true, ..Filters::default() }, ..Query::default() };
    assert_eq!(find(&index, &lib, &starred, &Scope::default()), vec![TRAM, SWIM]);
    apply(&mut lib, &mut index, Op::SetFacts { facts: vec![(SWIM, Facts { make: "GoPro".into(), ..Facts::default() })] });
    assert_eq!(text(&index, &lib, "camera:gopro"), vec![SWIM]);
    let undo = apply(&mut lib, &mut index, Op::RemoveFolder { id: LISBON });
    assert_eq!(index.len(), 1);
    apply(&mut lib, &mut index, undo);
    assert_eq!(index.len(), 4);
    assert_eq!(text(&index, &lib, "portugal"), vec![TRAM, TILES]);
}

#[test]
fn a_record_removed_and_made_again_in_one_batch_starts_afresh() {
    let mut lib = fixture();
    let mut index = SearchIndex::from_library(&lib);
    // The folder comes back under its id with one photo of the two: the other leaves the index.
    let tram = lib.photo(TRAM).unwrap().clone();
    let op = Op::Batch {
        ops: vec![
            Op::RemoveFolder { id: LISBON },
            Op::AddFolder { folder: Folder { id: LISBON, path: PathBuf::from("/photos/Lisbon") } },
            Op::AddPhotos { photos: vec![tram] },
        ],
    };
    lib.apply(op.clone()).unwrap();
    index.apply(Update::for_op(&lib, &op));
    assert_eq!(index, SearchIndex::from_library(&lib));
    assert_eq!(text(&index, &lib, "img"), vec![TRAM]);
    // The album comes back under its id, empty.
    let op =
        Op::Batch { ops: vec![Op::DeleteAlbums { ids: vec![album()] }, Op::CreateAlbums { albums: vec![(album(), AlbumInfo::named("Again"))] }] };
    lib.apply(op.clone()).unwrap();
    index.apply(Update::for_op(&lib, &op));
    assert_eq!(index, SearchIndex::from_library(&lib));
    assert_eq!(text(&index, &lib, "again"), Vec::<PhotoId>::new());
    // An update that changes nothing search reads is empty.
    let op = Op::SetCovers { covers: vec![(album(), None)] };
    lib.apply(op.clone()).unwrap();
    assert!(Update::for_op(&lib, &op).is_empty());
}

#[test]
fn suggestions_list_tags_folders_and_albums_with_counts() {
    let mut lib = fixture();
    lib.apply(Op::AddTags { ids: vec![SWIM], tags: vec!["LISBON".into()] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    let s = index.suggest("lis", &Scope::default(), 20);
    let got: Vec<(SuggestionKind, &str, usize)> = s.iter().map(|s| (s.kind, s.text.as_str(), s.count)).collect();
    assert_eq!(got, vec![(SuggestionKind::Tag, "Lisbon", 2), (SuggestionKind::Folder, "Lisbon", 2), (SuggestionKind::Album, "Best of Lisbon", 2)]);
    assert_eq!(s[1].section, Some(Section::Folder(LISBON)));
    assert_eq!(s[2].section, Some(Section::Album(AlbumRef::User(album()))));
    // The word being typed is the last one; a dash is ignored; tag: lists tags only.
    let kinds = |t: &str| index.suggest(t, &Scope::default(), 20).into_iter().map(|s| (s.kind, s.text)).collect::<Vec<_>>();
    assert_eq!(kinds("beach -tr"), vec![(SuggestionKind::Tag, "tram".to_string())]);
    assert_eq!(kinds("tag:li"), vec![(SuggestionKind::Tag, "Lisbon".to_string())]);
    assert_eq!(kinds("of"), vec![(SuggestionKind::Album, "Best of Lisbon".to_string())]);
    assert_eq!(kinds("day"), vec![(SuggestionKind::Folder, "Beach_Day".to_string())]);
    assert!(kinds("lis ").is_empty() && kinds("").is_empty() && kinds("camera:c").is_empty() && kinds("zzz").is_empty());
    // Counts follow the scope: a tag only on a hidden photo is not suggested.
    lib.apply(Op::AddTags { ids: vec![DOG], tags: vec!["puppy".into()] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert!(index.suggest("pup", &Scope::default(), 20).is_empty());
    assert_eq!(index.suggest("pup", &Scope { hidden_pictures: true, ..Scope::default() }, 20).len(), 1);
    // At most 100.
    let tags: Vec<String> = (0..150).map(|i| format!("lot{i:03}")).collect();
    lib.apply(Op::AddTags { ids: vec![TRAM], tags }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(index.suggest("lot", &Scope::default(), 1000).len(), MAX_SUGGESTIONS);
    assert_eq!(index.suggest("lot", &Scope::default(), 7).len(), 7);
    assert_eq!(index.suggest("lot", &Scope::default(), 1000)[0].text, "lot000");
}

#[test]
fn hostile_queries_are_bounded_and_never_panic() {
    let lib = fixture();
    let index = SearchIndex::from_library(&lib);
    let many = "tram ".repeat(10_000);
    let huge = "é".repeat(512 * 1024);
    let long_word = "x".repeat(1 << 20);
    let bidi = "tr\u{202e}am \u{2066}lis\u{2069} \u{200f}-\u{200e}tram";
    let controls = "tr\u{0}am\u{7}\u{1b}[31mlis\r\n";
    for q in [many.as_str(), huge.as_str(), long_word.as_str(), bidi, controls, "\u{feff}", "-:", ":::", "tag:tag:tag:", "-----x", "🦀🦀"] {
        let start = Instant::now();
        let p = Parsed::new(q);
        assert!(p.atoms.len() <= MAX_WORDS, "{} atoms", p.atoms.len());
        assert!(p.atoms.iter().flat_map(|a| &a.tokens).all(|t| !t.is_empty() && t.len() <= MAX_TERM));
        let tokens: usize = p.atoms.iter().map(|a| a.tokens.len()).sum();
        assert!(tokens <= MAX_QUERY);
        let r = index.search(&Query::text(q), &Scope::default());
        let _ = index.suggest(q, &Scope::default(), 100);
        let _ = text(&index, &lib, q);
        assert!(r.ids.len() <= 3);
        assert!(start.elapsed().as_secs_f64() < 2.0, "{:?}", start.elapsed());
    }
    assert!(Parsed::new(&many).truncated && Parsed::new(&huge).truncated && Parsed::new(&long_word).truncated);
    // Hidden characters split words: they never glue or hide one.
    assert_eq!(text(&index, &lib, bidi), Vec::<PhotoId>::new());
    assert_eq!(Parsed::new("tr\u{202e}am").atoms.len(), 2);
    assert_eq!(text(&index, &lib, "\u{2066}tram\u{2069}"), vec![TRAM]);
    // Long words match by their first MAX_TERM bytes.
    let mut lib = lib;
    let long = "y".repeat(MAX_TERM * 3);
    lib.apply(Op::SetCaptions { captions: vec![(TILES, long.clone())] }).unwrap();
    let index = SearchIndex::from_library(&lib);
    assert_eq!(text(&index, &lib, &long), vec![TILES]);
    assert_eq!(text(&index, &lib, &"y".repeat(MAX_TERM + 1)), vec![TILES]);
}

#[test]
fn a_100k_photo_index_builds_and_answers_quickly() {
    const PHOTOS: u64 = 100_000;
    const FOLDERS: u64 = 500;
    let mut lib = Library::new();
    for f in 0..FOLDERS {
        lib.apply(Op::AddFolder { folder: Folder { id: FolderId::new(f + 1), path: PathBuf::from(format!("/photos/{}/trip {f}", 2000 + f % 26)) } })
            .unwrap();
    }
    let words = ["tram", "beach", "sunset", "harbour", "market", "castle", "river", "bridge", "garden", "street"];
    let photos: Vec<Photo> = (0..PHOTOS)
        .map(|i| {
            let d = date(2000 + (i % 26) as u16, 1 + (i % 12) as u8, 1 + (i % 28) as u8);
            photo(PhotoId::new(i + 1), FolderId::new(i % FOLDERS + 1), &format!("IMG_{i:06}.jpg"), 1_000_000 + i, 4000, 3000, d, i % 9 == 0)
        })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    let infos: Vec<(PhotoId, PhotoInfo)> = (0..PHOTOS)
        .map(|i| {
            let w = |k: u64| words[((i * 7 + k * 3) % 10) as usize];
            let caption = format!("{} by the {} at {}", w(0), w(1), i % 997);
            (PhotoId::new(i + 1), PhotoInfo { caption, tags: vec![format!("tag {}", i % 2000), w(2).to_string()], ..PhotoInfo::default() })
        })
        .collect();
    lib.apply(Op::SetPhotoInfo { photos: infos }).unwrap();
    for a in 0..50u64 {
        let id = AlbumId::parse(&format!("a{a:04x}")).unwrap();
        lib.apply(Op::CreateAlbums { albums: vec![(id.clone(), AlbumInfo::named(format!("Album {a} {}", words[(a % 10) as usize])))] }).unwrap();
        let ids: Vec<PhotoId> = (0..200).map(|k| PhotoId::new((a * 1999 + k * 13) % PHOTOS + 1)).collect();
        let op = lib.album_add_op(&id, &ids, None).unwrap();
        lib.apply(op).unwrap();
    }
    let start = Instant::now();
    let extract = Extract::from_library(&lib);
    let extracted = start.elapsed();
    let start = Instant::now();
    let index = SearchIndex::build(extract);
    let built = start.elapsed();
    let scope = Scope::default();
    let mut times = Vec::new();
    for q in ["tram", "tram sunset 2010", "street -beach", "t", "img_0123", "tag:tag", "zzz"] {
        let filters = Filters { starred: q.contains("sunset"), ..Filters::default() };
        let start = Instant::now();
        let r = index.search(&Query { text: q.into(), filters, age: None }, &scope);
        times.push(format!("{q:?} {} in {:?}", r.count(), start.elapsed()));
    }
    let start = Instant::now();
    let suggestions = index.suggest("tag 1", &scope, 100);
    let suggested = start.elapsed();
    let mut lib2 = lib.clone();
    let mut index2 = index.clone();
    let op = Op::SetCaptions { captions: (1..=1000).map(|i| (PhotoId::new(i), format!("updated {i}"))).collect() };
    lib2.apply(op.clone()).unwrap();
    let start = Instant::now();
    index2.apply(Update::for_op(&lib2, &op));
    let updated = start.elapsed();
    println!(
        "{PHOTOS} photos: extract in {extracted:?}, index built in {built:?}; queries: {}; suggestions in {suggested:?}; 1 000 captions updated in {updated:?}",
        times.join(", ")
    );
    assert_eq!(index.len(), PHOTOS as usize);
    assert_eq!(suggestions.len(), 100);
    assert_eq!(index2.search(&Query::text("updated"), &scope).count(), 1000);
    // Generous: catches quadratic work even in an unoptimised build on a slow runner.
    assert!(built.as_secs_f64() < 30.0, "building took {built:?}");
}
