//! Search on a worker (`roadmap.md` M9.11; `organizing.md` §8, `ui-layout.md` §9): `search.set`
//! debounced on the session clock and run on the search worker against `library`'s index, which
//! follows every op; results equal to the index's own answer for the same library; filters and the
//! age combining with text; `search.clear`; `search.suggest`; Save Search as Album as one undo
//! step; `album:search` in `library.query`; a 100 k synthetic library answered off the owner
//! thread. No sleeps: the debounce runs on a virtual clock and `wait_search` waits for the worker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{AlbumId, Clock, DateTime, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::search::{DEBOUNCE, WORKER_THREAD, scope_of};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_library::search::{Age, Filters, Query, SearchIndex};
use dalica_library::{AlbumInfo, Facts, Folder, Library, Op, Photo};
use serde_json::{Value, json};

use crate::common::pick;

/// 2026-06-10T00:00:00 UTC.
const NOW: i64 = 1_781_049_600;

fn at(text: &str) -> DateTime {
    text.parse().unwrap()
}

fn photo(id: u64, folder: u64, name: &str, date: Option<&str>, star: bool) -> Photo {
    Photo {
        id: PhotoId::new(id),
        folder: FolderId::new(folder),
        name: name.into(),
        size: 1_000 * id,
        mtime: Timestamp::from_millis(1_450_000_000_000),
        width: 4000,
        height: 3000,
        date: date.map(at),
        star,
    }
}

fn facts(make: &str, video: bool, geotagged: bool) -> Facts {
    Facts { make: make.into(), model: String::new(), video, geotagged }
}

struct Fx {
    _tmp: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    s: Session,
    album: AlbumId,
}

/// Three folders of a watched root that holds no files (the session never scans it):
///
/// | id | folder | name | date | star | tags, caption | facts |
/// |---|---|---|---|---|---|---|
/// | 1 | Lisbon | IMG_0001.jpg | 2026-06-04 | ★ | tram, lisbon; "Tram 28 up the hill" | Canon, geotagged |
/// | 2 | Lisbon | IMG_0002.jpg | 2026-06-04 | | lisbon, sunset; "Sunset over the river" | geotagged |
/// | 3 | Lisbon | IMG_0003.jpg | 2026-06-04 | | tram | |
/// | 4 | Lisbon | CLIP_0004.mp4 | 2026-06-04 | | tram | video |
/// | 5 | Dunes | IMG_0005.jpg | 2025-08-10 | ★ | beach, sunset; "beach sunset" | Sony |
/// | 6 | Dunes | IMG_0006.jpg | 2025-08-10 | | beach | geotagged |
/// | 7 | Dunes | VID_0007.mp4 | 2025-08-10 | ★ | | video |
/// | 8 | Harbour | IMG_0008.jpg | 2016-01-02 | ★ | tram; "old tram depot" | |
/// | 9 | Harbour | IMG_0009.jpg | none (file time 2015) | | | |
///
/// and the album "Best of the year" holding 8 then 1.
fn fixture() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    let folders = [(1, "2026-06-04 Lisbon"), (2, "2025-08-10 Dunes"), (3, "2016-01-02 Harbour")];
    let mut ops: Vec<Op> =
        folders.iter().map(|(id, name)| Op::AddFolder { folder: Folder { id: FolderId::new(*id), path: root.join(name) } }).collect();
    ops.push(Op::AddPhotos {
        photos: vec![
            photo(1, 1, "IMG_0001.jpg", Some("2026-06-04T09:00:00"), true),
            photo(2, 1, "IMG_0002.jpg", Some("2026-06-04T09:05:00"), false),
            photo(3, 1, "IMG_0003.jpg", Some("2026-06-04T09:10:00"), false),
            photo(4, 1, "CLIP_0004.mp4", Some("2026-06-04T09:20:00"), false),
            photo(5, 2, "IMG_0005.jpg", Some("2025-08-10T18:00:00"), true),
            photo(6, 2, "IMG_0006.jpg", Some("2025-08-10T18:05:00"), false),
            photo(7, 2, "VID_0007.mp4", Some("2025-08-10T18:10:00"), true),
            photo(8, 3, "IMG_0008.jpg", Some("2016-01-02T12:00:00"), true),
            photo(9, 3, "IMG_0009.jpg", None, false),
        ],
    });
    let p = PhotoId::new;
    ops.push(Op::SetCaptions {
        captions: vec![
            (p(1), "Tram 28 up the hill".into()),
            (p(2), "Sunset over the river".into()),
            (p(5), "beach sunset".into()),
            (p(8), "old tram depot".into()),
        ],
    });
    ops.push(Op::AddTags { ids: vec![p(1), p(3), p(4), p(8)], tags: vec!["tram".into()] });
    ops.push(Op::AddTags { ids: vec![p(1), p(2)], tags: vec!["lisbon".into()] });
    ops.push(Op::AddTags { ids: vec![p(2), p(5)], tags: vec!["sunset".into()] });
    ops.push(Op::AddTags { ids: vec![p(5), p(6)], tags: vec!["beach".into()] });
    ops.push(Op::SetFacts {
        facts: vec![
            (p(1), facts("Canon", false, true)),
            (p(2), facts("", false, true)),
            (p(4), facts("", true, false)),
            (p(5), facts("Sony", false, false)),
            (p(6), facts("", false, true)),
            (p(7), facts("", true, false)),
        ],
    });
    let album = AlbumId::parse("a000000000001").unwrap();
    ops.push(Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named("Best of the year"))] });
    let mut library = Library::new();
    library.apply(Op::Batch { ops }).unwrap();
    let add = library.album_add_op(&album, &[p(8), p(1)], None).unwrap();
    library.apply(add).unwrap();
    let lib_dir = tmp.path().join("library");
    std::fs::create_dir_all(&lib_dir).unwrap();
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &library, 0).unwrap();
    let roots = json!({ "version": 1, "folders": [{ "path": root }] });
    std::fs::write(lib_dir.join(dalica_engine::session::FOLDERS_FILE), serde_json::to_vec(&roots).unwrap()).unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(NOW * 1000)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let s = Session::open_with(&lib_dir, clock.clone(), &options).unwrap();
    Fx { _tmp: tmp, clock, s, album }
}

fn ids(v: &[u64]) -> Vec<PhotoId> {
    v.iter().map(|n| PhotoId::new(*n)).collect()
}

impl Fx {
    /// `search.set` with `params`, then the debounce passes and the result is applied.
    fn search(&mut self, params: Value) -> Vec<PhotoId> {
        self.s.execute("search.set", &params).unwrap();
        self.settle();
        self.results()
    }

    fn settle(&mut self) {
        self.clock.advance(DEBOUNCE);
        self.s.poll();
        self.s.wait_search();
    }

    fn results(&self) -> Vec<PhotoId> {
        self.s.search_state().result.as_ref().map(|r| r.photos().to_vec()).unwrap_or_default()
    }

    /// What `library`'s own index answers for the session's current query.
    fn expected(&self) -> dalica_library::search::SearchResult {
        let st = self.s.search_state();
        let now = self.clock.now();
        let query = Query { text: st.text.clone(), filters: st.filters, age: st.age_days.map(|days| Age { days, now }) };
        let scope = scope_of(self.s.library(), self.s.roots(), &self.s.search_tree_options());
        SearchIndex::from_library(self.s.library()).search(&query, &scope)
    }

    fn assert_matches_library(&self) {
        let want = self.expected();
        let r = self.s.search_state().result.clone().unwrap();
        assert_eq!(r.photos(), want.ids.as_slice(), "{:?}", self.s.search_state().text);
        assert_eq!(
            (&r.sections, r.folders, r.bytes, &r.unavailable, r.truncated),
            (&want.sections, want.folders, want.bytes, &want.unavailable, want.truncated)
        );
    }

    fn query(&mut self, params: Value) -> Vec<PhotoId> {
        let v = self.s.execute("library.query", &params).unwrap();
        v["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect()
    }
}

#[test]
fn five_keystrokes_inside_the_debounce_run_one_query() {
    let mut fx = fixture();
    for text in ["t", "tr", "tra", "tram", "tram "] {
        let v = fx.s.execute("search.set", &json!({ "text": text })).unwrap();
        assert_eq!((v["changed"].as_bool(), v["pending"].as_bool(), v["matches"].clone()), (Some(true), Some(true), Value::Null));
        fx.clock.advance(Duration::from_millis(40));
        fx.s.poll();
        assert!(fx.s.search_state().result.is_none());
    }
    // The debounce starts again with each keystroke: 119 ms after the last one nothing has run.
    fx.clock.advance(DEBOUNCE - Duration::from_millis(41));
    fx.s.poll();
    assert!(fx.s.search_state().pending());
    assert_eq!(fx.s.search_stats().queries, 0);
    fx.clock.advance(Duration::from_millis(1));
    fx.s.poll();
    assert_eq!(fx.s.wait_search(), 1);
    let stats = fx.s.search_stats();
    assert_eq!((stats.queries, stats.builds), (1, 1), "{stats:?}");
    assert_eq!(fx.results(), ids(&[1, 3, 4, 8]));
    let v = fx.s.search_inspect();
    assert_eq!(
        (v["matches"].as_u64(), v["folders"].as_u64(), v["bytes"].as_u64(), v["pending"].as_bool()),
        (Some(4), Some(2), Some(16_000), Some(false))
    );
    assert_eq!(v["text"], "tram ");
    assert_eq!(
        v["sections"],
        json!([{ "section": "album:search", "count": 4 },
            { "section": "album:starred", "count": 2 },
            { "section": "album:a000000000001", "count": 2 },
            { "section": "folder:1", "count": 3 },
            { "section": "folder:3", "count": 1 }])
    );
    assert!(v["seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(fx.s.search_state().result.as_ref().unwrap().thread, WORKER_THREAD);
}

#[test]
fn a_stale_result_is_dropped() {
    let mut fx = fixture();
    fx.s.execute("search.set", &json!({ "text": "dunes" })).unwrap();
    fx.clock.advance(DEBOUNCE);
    fx.s.poll();
    // The query changes while the first one is on the worker: its answer never shows.
    fx.s.execute("search.set", &json!({ "text": "harbour" })).unwrap();
    fx.s.wait_search();
    assert_eq!(fx.results(), ids(&[9, 8]), "the undated photo sorts by its file time, 2015");
    let r = fx.s.search_state().result.as_ref().unwrap();
    assert_eq!((r.generation, r.text.as_str()), (fx.s.search_state().generation, "harbour"));
}

#[test]
fn results_equal_the_library_index_for_the_same_library() {
    let mut fx = fixture();
    let cases = [
        json!({ "text": "tram" }),
        json!({ "text": "sunset" }),
        json!({ "text": "tram -lisbon" }),
        json!({ "text": "tag:beach" }),
        json!({ "text": "camera:canon" }),
        json!({ "text": "best" }),
        json!({ "text": "2025-08" }),
        json!({ "text": "june" }),
        json!({ "text": "img 4000x3000" }),
        json!({ "text": "person:ana" }),
        json!({ "text": "nothing-matches-this" }),
        json!({ "text": "", "filters": { "faces": true } }),
        json!({ "text": "x".repeat(5000) }),
    ];
    for case in cases {
        fx.search(case.clone());
        fx.assert_matches_library();
    }
    // Light-table order: the newest folder first, each folder's photos in its own sort.
    assert_eq!(fx.search(json!({ "text": "img", "filters": { "faces": false } })), ids(&[1, 2, 3, 5, 6, 9, 8]));
    // The age reads the sort date: the undated photo ages by its file time.
    assert_eq!(fx.search(json!({ "text": "harbour", "age": 3650 })), Vec::<PhotoId>::new());
    let truncated = fx.s.execute("search.set", &json!({ "text": "x ".repeat(100), "age": 0 })).unwrap();
    assert_eq!(truncated["changed"], true);
    fx.settle();
    assert_eq!(fx.s.search_inspect()["truncated"], true);
}

#[test]
fn a_star_changed_during_search_updates_the_results_without_a_rebuild() {
    let mut fx = fixture();
    assert_eq!(fx.search(json!({ "text": "tram", "filters": { "starred": true } })), ids(&[1, 8]));
    fx.s.execute("photo.star.set", &json!({ "ids": [3], "on": true })).unwrap();
    // The results follow once the debounce has passed.
    fx.s.poll();
    assert!(fx.s.search_state().pending());
    assert_eq!(fx.results(), ids(&[1, 8]));
    fx.settle();
    assert_eq!(fx.results(), ids(&[1, 3, 8]));
    fx.assert_matches_library();
    fx.s.execute("edit.undo", &json!({})).unwrap();
    fx.settle();
    assert_eq!(fx.results(), ids(&[1, 8]));
    // A caption and a tag, and a folder's place, are found once set.
    fx.s.execute("photo.caption.set", &json!({ "ids": [9], "caption": "Tram stop at dusk" })).unwrap();
    fx.s.execute("photo.star.set", &json!({ "ids": [9], "on": true })).unwrap();
    fx.settle();
    assert_eq!(fx.results(), ids(&[1, 9, 8]));
    fx.assert_matches_library();
    let stats = fx.s.search_stats();
    assert_eq!(stats.builds, 1, "the index followed the ops: {stats:?}");
    assert!(stats.ops >= 4, "{stats:?}");
}

#[test]
fn filters_and_the_age_combine_with_text() {
    let mut fx = fixture();
    let cases: [(Value, &[u64]); 7] = [
        (json!({ "text": "tram" }), &[1, 3, 4, 8]),
        (json!({ "text": "tram", "filters": { "starred": true } }), &[1, 8]),
        (json!({ "text": "tram", "filters": { "starred": false, "videos": true } }), &[4]),
        (json!({ "text": "", "filters": { "videos": true } }), &[4, 7]),
        (json!({ "text": "sunset", "filters": { "videos": false, "geotagged": true } }), &[2]),
        (json!({ "text": "tram", "filters": { "geotagged": false }, "age": 30 }), &[1, 3, 4]),
        (json!({ "text": "", "filters": { "starred": true }, "age": 365 }), &[1, 5, 7]),
    ];
    for (params, want) in &cases {
        assert_eq!(fx.search(params.clone()), ids(want), "{params}");
        fx.assert_matches_library();
    }
    // A filter left out stays as it was; the age stays until set again.
    let v = fx.s.execute("search.set", &json!({ "text": "beach" })).unwrap();
    assert_eq!((v["filters"]["starred"].as_bool(), v["age"].as_u64()), (Some(true), Some(365)));
    fx.settle();
    assert_eq!(fx.results(), ids(&[5]));
    // Everything off: search mode ends.
    let v = fx.s.execute("search.set", &json!({ "text": " ", "filters": { "starred": false }, "age": null })).unwrap();
    assert_eq!((v["active"].as_bool(), v["matches"].clone(), v["pending"].as_bool()), (Some(false), Value::Null, Some(false)));
}

#[test]
fn search_clear_shows_everything_again() {
    let mut fx = fixture();
    assert_eq!(fx.search(json!({ "text": "dunes", "filters": { "starred": true }, "age": 400 })), ids(&[5, 7]));
    assert_eq!(fx.s.search_filter().unwrap().ids(), ids(&[5, 7]).as_slice());
    let v = fx.s.execute("search.clear", &json!({})).unwrap();
    assert_eq!(v["active"], false);
    assert_eq!((v["text"].as_str(), v["age"].as_u64(), v["filters"]["starred"].as_bool()), (Some(""), Some(0), Some(false)));
    assert!(fx.s.search_filter().is_none());
    assert!(fx.s.search_state().result.is_none());
    assert_eq!(fx.s.execute("library.query", &json!({})).unwrap()["total"], 9);
    assert!(matches!(fx.s.execute("library.query", &json!({ "section": "album:search" })), Err(CmdError::NotFound(_))));
    assert_eq!(fx.s.undo_stack().len(), 0, "searching is no undo step");
}

#[test]
fn suggestions_list_tags_folders_and_albums_with_counts() {
    let mut fx = fixture();
    let v = fx.s.execute("search.suggest", &json!({ "text": "b" })).unwrap();
    let got: Vec<(String, String, u64, Value)> = v["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["kind"].as_str().unwrap().into(), s["text"].as_str().unwrap().into(), s["count"].as_u64().unwrap(), s["section"].clone()))
        .collect();
    assert_eq!(
        got,
        [("tag".into(), "beach".into(), 2, Value::Null), ("album".into(), "Best of the year".into(), 2, json!(format!("album:{}", fx.album))),]
    );
    let v = fx.s.execute("search.suggest", &json!({ "text": "sunset tag:t", "limit": 5 })).unwrap();
    assert_eq!(v["suggestions"], json!([{ "kind": "tag", "text": "tram", "count": 4, "section": null }]));
    let v = fx.s.execute("search.suggest", &json!({ "text": "har" })).unwrap();
    assert_eq!(v["suggestions"][0]["section"], json!("folder:3"));
    assert_eq!(fx.s.execute("search.suggest", &json!({ "text": "" })).unwrap()["suggestions"], json!([]));
    // The result of a search carries the suggestions for its text, answered without asking again.
    fx.search(json!({ "text": "lis" }));
    let asked = fx.s.search_stats().asked;
    let v = fx.s.execute("search.suggest", &json!({ "text": "lis" })).unwrap();
    assert_eq!(v["suggestions"][0]["text"], "lisbon");
    assert_eq!(fx.s.search_stats().asked, asked);
    assert_eq!(fx.s.undo_stack().len(), 0);
}

#[test]
fn save_search_as_album_keeps_the_order_and_undo_removes_it() {
    let mut fx = fixture();
    let found = fx.search(json!({ "text": "tram" }));
    assert_eq!(found, ids(&[1, 3, 4, 8]));
    let before = fx.s.undo_stack().len();
    let v = fx.s.execute("search.saveAsAlbum", &json!({ "name": "  Trams \n" })).unwrap();
    assert_eq!((v["name"].as_str(), v["count"].as_u64()), (Some("Trams"), Some(4)));
    let album = AlbumId::parse(v["album"].as_str().unwrap()).unwrap();
    assert_ne!(album, fx.album);
    assert_eq!(fx.s.library().album(&album).unwrap().name, "Trams");
    assert_eq!(fx.s.library().album_members(&album), found.as_slice());
    assert_eq!(fx.s.undo_stack().len(), before + 1, "one undo step");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(fx.s.library().album(&album).is_none());
    assert!(fx.s.library().album_members(&album).is_empty());
    fx.s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(fx.s.library().album_members(&album), found.as_slice());
    // Saved before the result arrived: the worker answers the current query at once.
    fx.s.execute("search.set", &json!({ "text": "sunset" })).unwrap();
    let v = fx.s.execute("search.saveAsAlbum", &json!({ "name": "Sunsets" })).unwrap();
    let sunsets = AlbumId::parse(v["album"].as_str().unwrap()).unwrap();
    assert_eq!(fx.s.library().album_members(&sunsets), ids(&[2, 5]).as_slice());
    // The new album's name is found by the next search.
    assert_eq!(fx.search(json!({ "text": "trams" })), ids(&[1, 3, 4, 8]));
}

#[test]
fn album_search_lists_the_results_and_library_query_takes_the_grammar() {
    let mut fx = fixture();
    let found = fx.search(json!({ "text": "tram" }));
    assert_eq!(fx.query(json!({ "section": "album:search" })), found);
    assert_eq!(fx.query(json!({ "section": "album:search", "offset": 1, "limit": 2 })), found[1..3].to_vec());
    assert_eq!(fx.query(json!({ "section": "album:search", "starred": true })), ids(&[1, 8]));
    // The grammar in `text`, without touching search mode.
    let v = fx.s.execute("library.query", &json!({ "text": "sunset", "filters": { "geotagged": true } })).unwrap();
    assert_eq!((v["total"].as_u64(), v["photos"][0]["id"].as_u64(), v["truncated"].as_bool()), (Some(1), Some(2), Some(false)));
    assert_eq!(fx.s.search_state().text, "tram");
    assert_eq!(fx.query(json!({ "text": "tram", "folder": 3 })), ids(&[8]));
    assert_eq!(fx.query(json!({ "text": "", "age": 30 })), ids(&[1, 2, 3, 4]));
    assert_eq!(fx.s.execute("library.query", &json!({ "text": "person:ana" })).unwrap()["unavailable"], json!(["person"]));
    // Other sections, alone and with text.
    assert_eq!(fx.query(json!({ "section": format!("album:{}", fx.album) })), ids(&[8, 1]));
    assert_eq!(fx.query(json!({ "section": format!("album:{}", fx.album), "text": "lisbon" })), ids(&[1]));
    assert_eq!(fx.query(json!({ "section": "folder:2" })), ids(&[5, 6, 7]));
    assert_eq!(fx.query(json!({ "section": "album:starred", "text": "img" })).len(), 3);
    for (bad, code) in [
        (json!({ "section": "album:a0000000000ff" }), "not_found"),
        (json!({ "section": "folder:99" }), "not_found"),
        (json!({ "section": "nope" }), "bad_params"),
        (json!({ "section": 3 }), "bad_params"),
        (json!({ "text": "x".repeat(16 * 1024 + 1) }), "limit"),
        (json!({ "age": -3 }), "bad_params"),
        (json!({ "filters": { "people": true } }), "bad_params"),
    ] {
        assert_eq!(fx.s.execute("library.query", &bad).unwrap_err().code(), code, "{bad}");
    }
}

#[test]
fn photos_a_scan_adds_while_searching_are_found_with_their_first_indexed_times() {
    // The clock is years after the files' times: only the first-indexed time, recorded by a
    // derived op after the scan's, makes the photos Recently Updated.
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("photos").join("Lisbon");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 64, 48), 80, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{i:04}.jpg")), bytes).unwrap();
    }
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_900_000_000_000)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&tmp.path().join("library"), clock.clone(), &options).unwrap();
    s.execute("search.set", &json!({ "text": "img" })).unwrap();
    s.wait_search();
    assert_eq!(s.search_state().result.as_ref().unwrap().count(), 0);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    clock.advance(DEBOUNCE);
    s.poll();
    s.wait_search();
    let r = s.search_state().result.clone().unwrap();
    assert_eq!(r.count(), 3);
    let scope = scope_of(s.library(), s.roots(), &s.search_tree_options());
    let want = SearchIndex::from_library(s.library()).search(&Query::text("img"), &scope);
    assert_eq!((r.photos(), &r.sections), (want.ids.as_slice(), &want.sections));
    assert!(r.sections.iter().any(|c| c.section.to_string() == "album:recent" && c.count == 3), "{:?}", r.sections);
    assert_eq!(s.search_stats().builds, 1, "the scan's ops reached the index one by one");
}

#[test]
fn a_rebuild_that_replaces_the_library_gives_the_worker_a_fresh_copy() {
    // The watched root of an index-only library holds no files: a rebuild empties the library.
    let tmp = tempfile::tempdir().unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(NOW * 1000)));
    let mut s = synthetic::open(tmp.path(), &Spec::new(40, 4, 7), clock.clone()).unwrap();
    std::fs::create_dir_all(synthetic::photos_dir(tmp.path())).unwrap();
    s.execute("search.set", &json!({ "text": "img" })).unwrap();
    s.wait_search();
    assert_eq!(s.search_state().result.as_ref().unwrap().count(), 40);
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 0);
    // The rebuilt library came without an op: the next poll sees it and sends a fresh copy.
    s.poll();
    s.wait_search();
    assert_eq!(s.search_state().result.as_ref().unwrap().count(), 0);
    assert_eq!(s.search_stats().builds, 2, "{:?}", s.search_stats());
}

/// 100 000 photos at the review gate (timed; the `search` bench judges the times in release), 5 000
/// in a task's gate.
#[test]
fn a_query_on_a_100k_library_answers_off_the_owner_thread() {
    let (spec, label) = pick((Spec::new(5_000, 100, synthetic::SEED), "5 000"), (Spec::PHOTOS_100K, "100 k"));
    let tmp = tempfile::tempdir().unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(NOW * 1000)));
    let t = Instant::now();
    let mut s = synthetic::open(tmp.path(), &spec, clock.clone()).unwrap();
    let opened = t.elapsed();
    let t = Instant::now();
    s.execute("search.set", &json!({ "text": "harbour" })).unwrap();
    let first_set = t.elapsed();
    clock.advance(DEBOUNCE);
    let t = Instant::now();
    s.poll();
    let poll = t.elapsed();
    let t = Instant::now();
    s.wait_search();
    let waited = t.elapsed();
    let r = s.search_state().result.clone().unwrap();
    assert_eq!(r.thread, WORKER_THREAD);
    assert_ne!(std::thread::current().name(), Some(WORKER_THREAD));
    assert!(r.count() > 0);
    // Later keystrokes: the owner only sends and applies.
    let t = Instant::now();
    s.execute("search.set", &json!({ "text": "harbour 2020", "filters": { "starred": true } })).unwrap();
    clock.advance(DEBOUNCE);
    s.poll();
    let owner = t.elapsed();
    s.wait_search();
    let r2 = s.search_state().result.clone().unwrap();
    assert_eq!(r2.thread, WORKER_THREAD);
    let scope = scope_of(s.library(), s.roots(), &s.search_tree_options());
    let query = Query { text: "harbour 2020".into(), filters: Filters { starred: true, ..Filters::default() }, age: None };
    let want = SearchIndex::from_library(s.library()).search(&query, &scope);
    assert_eq!(r2.photos(), want.ids.as_slice());
    println!(
        "{label} photos: opened {opened:.2?}; first search.set (starts the worker, copies the library) {first_set:.2?}; poll {poll:.2?}; \
         index built and first query answered {waited:.2?} (query {:.1} ms, {} matches); next keystroke on the owner thread {owner:.2?}, \
         its query {:.1} ms on the worker ({} matches)",
        r.seconds * 1000.0,
        r.count(),
        r2.seconds * 1000.0,
        r2.count()
    );
}

#[test]
fn every_command_fails_gracefully() {
    let mut fx = fixture();
    let model = fx.s.library().model().clone();
    // Save Search as Album with a name and with matches (`cmd::search`'s tests: only while searching).
    fx.s.execute("search.set", &json!({ "text": "zzzz" })).unwrap();
    assert_eq!(fx.s.execute("search.saveAsAlbum", &json!({ "name": "\u{7}" })).unwrap_err().code(), "bad_params");
    assert_eq!(fx.s.execute("search.saveAsAlbum", &json!({ "name": "x" })).unwrap_err().code(), "disabled");
    assert_eq!(fx.s.execute("search.saveAsAlbum", &json!({ "name": 4 })).unwrap_err().code(), "bad_params");
    // Hostile text never panics: control and bidi characters, 8 000 words, operators alone.
    for text in ["\u{202e}tram\u{0}", "- -- tag: camera: person:", &"a ".repeat(8_000), "\"", "tag:\u{feff}"] {
        fx.s.execute("search.set", &json!({ "text": text })).unwrap();
        fx.settle();
        fx.assert_matches_library();
        fx.s.execute("search.suggest", &json!({ "text": text })).unwrap();
    }
    assert_eq!(fx.s.library().model(), &model);
    assert_eq!(fx.s.undo_stack().len(), 0);
}
