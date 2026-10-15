//! M9's *Search and filters* in the live app (`m9.rs` part 2): the grammar of `organizing.md`
//! §8.1–§8.2 over `library.query`, the filters and the age; search mode through `search.set` and
//! the toolbar's search field, the banner's text and counts against the spec and against the files
//! on disk (whole-window fingerprints of the banner and the matching sections); Show All; Save
//! Search as Album and its undo; Find Duplicates' groups and banner.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dalica_meta::{Changes, Place, WriteOptions};
use serde_json::{Value, json};

use crate::app::{self, click, idle, quiet, resize, run, undo};
use crate::live::{self, Client};

/// A photo: (path under `Pictures`, seed, size, make, model, taken, located).
type Spec = (&'static str, u64, (u32, u32), &'static str, &'static str, &'static str, bool);

/// The photos.
const PHOTOS: [Spec; 8] = [
    ("Harbour/IMG_0001.jpg", 11, (640, 480), "Canon", "EOS R5", "2024-06-01T09:01:00", false),
    ("Harbour/IMG_0002.jpg", 12, (640, 480), "Canon", "EOS R5", "2024-06-01T09:02:00", false),
    ("Harbour/IMG_0003.jpg", 13, (640, 480), "Canon", "EOS R5", "2024-06-01T09:03:00", false),
    ("Market/IMG_0101.jpg", 21, (640, 480), "Nikon", "Z6", "2023-12-24T11:00:00", false),
    ("Market/IMG_0102.jpg", 22, (640, 480), "Nikon", "Z6", "2023-12-24T12:00:00", false),
    ("Market/IMG_0103.jpg", 23, (640, 480), "Nikon", "Z6", "2023-12-24T13:00:00", false),
    ("Beach/IMG_0201.jpg", 31, (800, 600), "Canon", "EOS R5", "2024-06-15T16:00:00", true),
    ("Beach/IMG_0202.jpg", 32, (800, 600), "Canon", "EOS R5", "2024-06-15T16:30:00", true),
];

const H: [&str; 3] = ["Harbour/IMG_0001.jpg", "Harbour/IMG_0002.jpg", "Harbour/IMG_0003.jpg"];
const M: [&str; 3] = ["Market/IMG_0101.jpg", "Market/IMG_0102.jpg", "Market/IMG_0103.jpg"];
const B: [&str; 2] = ["Beach/IMG_0201.jpg", "Beach/IMG_0202.jpg"];

fn write(pictures: &Path) {
    for (i, (rel, seed, size, make, model, taken, located)) in PHOTOS.into_iter().enumerate() {
        let mut jpeg = app::jpeg(seed, size, make, model, taken);
        if located {
            let place = Place::new(38.69 + i as f64 / 100.0, -9.42);
            jpeg = dalica_meta::write_owned(&jpeg, &Changes { place: Some(place), ..Changes::default() }, &WriteOptions::default()).unwrap();
        }
        app::put(&app::under(pictures, rel), &jpeg, i as u64);
    }
}

fn set(parts: &[&[&str]]) -> BTreeSet<String> {
    parts.iter().flat_map(|p| p.iter().map(|s| s.to_string())).collect()
}

/// What `library.query` answers for `params`: the matching photos by path, and the reply.
fn query(c: &mut Client, by_id: &BTreeMap<String, String>, params: Value) -> (BTreeSet<String>, Value) {
    let mut p = params;
    p["limit"] = json!(10000);
    let r = c.ok("engine.execute", json!({ "command": "library.query", "params": p }));
    let found =
        r["photos"].as_array().unwrap().iter().map(|x| by_id.get(&x["id"].to_string()).cloned().unwrap_or_else(|| format!("?{}", x["id"]))).collect();
    (found, r)
}

/// `search.set` (or anything else that changed the query) has been answered: `library.inspect` →
/// `search` once nothing is pending (the 120 ms debounce runs on the app's clock, which `ui.wait`
/// moves on headless).
fn answered(c: &mut Client) -> Value {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        c.ok("ui.wait", json!({ "ms": 200 }));
        let s = state(c);
        if s["pending"] == false {
            c.settle();
            return s;
        }
        assert!(Instant::now() < end, "the search was not answered within two minutes: {s}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `library.inspect`'s `search` (`control-protocol.md` §6.4), as `search.set` with nothing to
/// change replies it.
fn state(c: &mut Client) -> Value {
    c.ok("engine.execute", json!({ "command": "search.set", "params": {} }))
}

/// The banner's line as `ui-layout.md` §9 writes it: "Showing 33 pictures in 4 folders (0.01 s) ·
/// 439.8 MB" (decimal units, one decimal), from numbers counted here.
fn banner_line(pictures: usize, folders: usize, seconds: f64, bytes: u64) -> String {
    let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    let size = match bytes as f64 {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b if b >= 1e3 => format!("{:.1} KB", b / 1e3),
        _ => format!("{bytes} bytes"),
    };
    format!("Showing {} in {} ({seconds:.2} s) · {size}", n(pictures, "picture", "pictures"), n(folders, "folder", "folders"))
}

fn bytes_of(root: &Path, rels: &BTreeSet<String>) -> u64 {
    rels.iter().map(|r| std::fs::metadata(app::under(root, r)).unwrap().len()).sum()
}

fn folders_of(rels: &BTreeSet<String>) -> usize {
    rels.iter().map(|r| r.rsplit_once('/').unwrap().0.to_string()).collect::<BTreeSet<_>>().len()
}

/// Checks the banner of the search answered as `s` for the matches `want`: the engine's numbers
/// are the files', and the window's banner reads as the spec writes it.
fn banner(c: &mut Client, root: &Path, s: &Value, want: &BTreeSet<String>) -> String {
    let (bytes, folders) = (bytes_of(root, want), folders_of(want));
    assert_eq!(
        (s["active"].clone(), s["matches"].as_u64(), s["folders"].as_u64(), s["bytes"].as_u64()),
        (json!(true), Some(want.len() as u64), Some(folders as u64), Some(bytes)),
        "the banner's numbers are the matches' files: {s}"
    );
    let seconds = s["seconds"].as_f64().unwrap();
    let line = banner_line(want.len(), folders, seconds, bytes);
    assert_eq!(app::label(c, "banner:search"), line, "the banner's text");
    assert!(c.has("button:search.showAll"), "and its Show All");
    line
}

/// Days from the photos' newest date before 2024 (2023-12-24) to now, less a week: an age that
/// keeps 2024's photos and leaves 2023's out whenever the test runs.
fn age_2024() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let new_year_2024 = 1_704_067_200; // 2024-01-01T00:00:00Z
    (now - new_year_2024) / 86_400 + 1
}

/// Part 2: the grammar, the filters and the age, the banner, Show All, Save Search as Album and
/// Find Duplicates.
pub fn search(exe: &Path, home: &Path) {
    let started = Instant::now();
    let root = home.join("Pictures");
    write(&root);
    let (app_, mut cl) = live::start(exe, home);
    let c = &mut cl;
    run(c, "folders.add", json!({ "path": root }));
    let all = app::photos(c, &root);
    assert_eq!(all.len(), PHOTOS.len(), "{:?}", all.keys());
    let id = |rel: &str| all[rel].id.clone();
    let by_id: BTreeMap<String, String> = all.iter().map(|(rel, p)| (p.id.to_string(), rel.clone())).collect();

    // What the user said: a caption, tags, a star, a folder's description, an album.
    run(c, "photo.caption.set", json!({ "ids": [id(H[0])], "caption": "Fishing boats at dawn" }));
    run(c, "photo.tags.add", json!({ "ids": [id(H[0]), id(H[1])], "tags": ["boats"] }));
    run(c, "photo.tags.add", json!({ "ids": [id(M[0])], "tags": ["tram"] }));
    run(c, "photo.star.set", json!({ "ids": [id(B[0])], "on": true }));
    run(c, "folder.info.set", json!({ "id": all[M[0]].folder, "description": "Old town" }));
    let summer = run(c, "album.create", json!({ "name": "Summer Trip", "ids": [id(H[2]), id(B[1])] }))["album"].as_str().unwrap().to_string();

    // The grammar (`organizing.md` §8.1), over `library.query`'s text.
    let everything = set(&[&H, &M, &B]);
    let cases: Vec<(&str, BTreeSet<String>)> = vec![
        ("", everything.clone()),
        ("harbour", set(&[&H])),
        ("HARB", set(&[&H])),
        ("fishing", set(&[&[H[0]]])),
        ("fish dawn", set(&[&[H[0]]])),
        ("fishing tram", set(&[])),
        ("harbour -boats", set(&[&[H[2]]])),
        ("-boats", set(&[&[H[2]], &M, &B])),
        ("tag:boats", set(&[&[H[0], H[1]]])),
        ("tag:tram", set(&[&[M[0]]])),
        ("camera:nikon", set(&[&M])),
        ("camera:canon", set(&[&H, &B])),
        ("eos", set(&[&H, &B])),
        ("2023", set(&[&M])),
        ("2024", set(&[&H, &B])),
        ("2024-06", set(&[&H, &B])),
        ("2024-06-01", set(&[&H])),
        ("december", set(&[&M])),
        ("June", set(&[&H, &B])),
        ("640x480", set(&[&H, &M])),
        ("800x600", set(&[&B])),
        ("summer", set(&[&[H[2], B[1]]])),
        ("old town", set(&[&M])),
        ("img:0101", set(&[&[M[0]]])),
        ("img_0003", set(&[&[H[2]]])),
    ];
    for (text, want) in &cases {
        let (found, r) = query(c, &by_id, json!({ "text": text }));
        assert_eq!(&found, want, "the query {text:?}: {r}");
    }
    for (text, op) in [("person:ana", "person"), ("color:red", "color")] {
        let (found, r) = query(c, &by_id, json!({ "text": text }));
        assert!(found.is_empty(), "{text} matches nothing before its milestone");
        assert!(r["unavailable"].as_array().is_some_and(|u| u.iter().any(|x| x == op)), "{text}: reported unavailable: {r}");
    }
    println!(
        "  the grammar: {} queries (words, prefixes and case, -word, tag:, camera:, dates, sizes, albums, folders, unknown fields, person: and color:)",
        cases.len() + 2
    );

    // The filters and the age, alone and with text.
    let age = age_2024();
    let filters: Vec<(Value, BTreeSet<String>)> = vec![
        (json!({ "filters": { "starred": true } }), set(&[&[B[0]]])),
        (json!({ "filters": { "geotagged": true } }), set(&[&B])),
        (json!({ "filters": { "videos": true } }), set(&[])),
        (json!({ "text": "canon", "filters": { "geotagged": true } }), set(&[&B])),
        (json!({ "text": "harbour", "filters": { "starred": true } }), set(&[])),
        (json!({ "age": age }), set(&[&H, &B])),
        (json!({ "text": "canon", "age": age, "filters": { "starred": true } }), set(&[&[B[0]]])),
        (json!({ "age": 0 }), everything.clone()),
    ];
    for (params, want) in &filters {
        let (found, r) = query(c, &by_id, params.clone());
        assert_eq!(&found, want, "{params}: {r}");
    }
    let (found, r) = query(c, &by_id, json!({ "filters": { "faces": true } }));
    assert!(found.is_empty() && r["unavailable"].as_array().is_some_and(|u| u.iter().any(|x| x == "faces")), "Faces waits for faces: {r}");
    println!("  the filters Starred, Geotagged, Videos and Faces and the age ({age} days), alone and with text");

    // Search mode over `search.set`: the banner's numbers and text.
    c.ok("engine.execute", json!({ "command": "search.set", "params": { "text": "harbour" } }));
    let s = answered(c);
    let line = banner(c, &root, &s, &set(&[&H]));
    // The sections that keep a match, the search album first (an album's with its matches only).
    let sections: BTreeMap<String, u64> =
        s["sections"].as_array().unwrap().iter().map(|x| (x["section"].as_str().unwrap().to_string(), x["count"].as_u64().unwrap())).collect();
    assert_eq!(s["sections"][0]["section"], "album:search", "{s}");
    assert_eq!(sections.get("album:search"), Some(&3));
    assert_eq!(sections.get(&format!("folder:{}", all[H[0]].folder)), Some(&3), "{s}");
    assert_eq!(sections.get(&format!("album:{summer}")), Some(&1), "Summer Trip keeps its one match: {s}");
    for rel in [M[0], B[0]] {
        assert!(!sections.contains_key(&format!("folder:{}", all[rel].folder)), "{rel}'s folder drops out: {s}");
    }
    println!("  search.set \"harbour\": \"{line}\"");
    c.ok("engine.execute", json!({ "command": "search.set", "params": { "text": "fishing" } }));
    let s = answered(c);
    let line = banner(c, &root, &s, &set(&[&[H[0]]]));
    assert!(line.starts_with("Showing 1 picture in 1 folder ("), "{line}");
    run(c, "search.clear", json!({}));
    assert!(!c.has("banner:search"), "Show All leaves search mode");

    // Typed in the toolbar's search field: the light table narrows to the matching sections under
    // the banner.
    quiet(c, 900.0);
    click(c, "field:search");
    c.ok("ui.type", json!({ "text": "june" }));
    let s = answered(c);
    let june = set(&[&H, &B]);
    let line = banner(c, &root, &s, &june);
    assert!(c.has("region:banner") || app::widget(c, "banner:search").is_some());
    assert!(c.has("section:album:search"), "the search album's section heads the light table");
    let listed: Vec<String> = s["sections"].as_array().unwrap().iter().map(|x| x["section"].as_str().unwrap().to_string()).collect();
    for (rel, shown) in [(H[0], true), (B[0], true), (M[0], false)] {
        assert_eq!(listed.contains(&format!("folder:{}", all[rel].folder)), shown, "{rel}'s folder shown: {shown} ({listed:?})");
    }
    run(c, "select.none", json!({}));
    // A click on the banner's text takes the focus (and the caret) from the field; the text stays.
    click(c, "banner:search");
    for size in [(1400, 900), (900, 600)] {
        resize(c, size);
        let masks = app::moving_parts(c);
        app::masked_shot(c, home, &format!("m9-{}x{}-search", size.0, size.1), size, &masks);
    }
    resize(c, (1400, 900));
    println!("  typed in the search field: \"{line}\"");

    // Save Search as Album: the matches in light-table order, one undo step.
    let results = app::section(c, &root, "album:search");
    assert_eq!(results.iter().cloned().collect::<BTreeSet<_>>(), june, "album:search lists the results");
    let r = run(c, "search.saveAsAlbum", json!({ "name": "June" }));
    let saved = r["album"].as_str().unwrap().to_string();
    assert_eq!(app::section(c, &root, &format!("album:{saved}")), results, "the album holds the results in their order");
    undo(c);
    assert!(app::album(c, &saved).is_none(), "Edit ▸ Undo removes the saved album");
    println!("  Save Search as Album: {} photos in light-table order; undone", results.len());
    click(c, "button:search.showAll");
    assert!(!c.has("banner:search"), "Show All leaves search mode");
    assert_eq!(state(c)["active"], false);

    // Find Duplicates: identical files, grouped and confirmed byte for byte.
    let dupes = home.join("Dupes");
    std::fs::create_dir_all(&dupes).unwrap();
    for (name, from) in [("DUP_0001.jpg", H[2]), ("DUP_0002.jpg", M[1]), ("DUP_0003.jpg", M[1])] {
        std::fs::copy(app::under(&root, from), dupes.join(name)).unwrap();
    }
    run(c, "folders.add", json!({ "path": dupes }));
    // It replies {job} at once; the groups are on show once the job ended.
    let r = c.ok("engine.execute", json!({ "command": "search.duplicates", "params": {} }));
    let job = r.get("job").cloned().unwrap_or_else(|| panic!("search.duplicates runs as a job: {r}"));
    let entry = app::wait_job(c, &job);
    assert_eq!(entry["state"], "done", "{entry}");
    let s = answered(c);
    assert_eq!((s["mode"].as_str(), s["banner"].as_str()), (Some("duplicates"), Some("2 duplicate groups (5 files)")), "{s}");
    let groups: Vec<u64> = s["groups"].as_array().unwrap().iter().map(|g| g["count"].as_u64().unwrap()).collect();
    assert_eq!(groups, [3, 2], "largest first: {s}");
    assert_eq!(app::label(c, "banner:search"), "2 duplicate groups (5 files)");
    click(c, "button:search.showAll");
    assert_eq!(state(c)["active"], false, "Show All leaves the duplicates");
    println!("  Find Duplicates: \"2 duplicate groups (5 files)\", largest first");
    idle(c);
    live::quit(app_, cl);
    println!("M9 search in the live app: passed in {:.1} s", app::secs(started));
}
