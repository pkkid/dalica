//! Colour search end to end (`organizing.md` §8.1, M16.10): a scan's sketches bring each photo's
//! colour summary in the same op, so `color:` finds procedural photos by colour; an edit that
//! redraws the sketch moves the summary with it; a library stored without summaries gains them
//! from its stored sketches at open, without decoding a photo; and Tools ▸ Experimental ▸ Search
//! for ▸ (`search.color`) sets the search as the field would.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_library::Op;
use serde_json::{Value, json};

const W: u32 = 128;
const H: u32 = 96;

/// The five pictures, each painted by a function of (x, y): a green field, a blue sky, a red barn
/// under a grey sky, a black-and-white street and a sunset.
/// Paints one pixel.
type Paint = fn(u32, u32) -> [u8; 3];

const PICTURES: [(&str, Paint); 5] = [
    ("field", |x, y| {
        let n = ((x * 7 + y * 13) % 40) as u8;
        let furrow = if (y / 8) % 4 == 0 { 30 } else { 0 };
        [40 + n / 2, 150 + n - furrow, 35 + n / 3]
    }),
    ("sky", |_, y| {
        let t = (y * 80 / H) as u8;
        [70 + t, 130 + t / 2 + t / 4, 220 + t / 4]
    }),
    ("barn", |x, y| {
        if !(H / 4..H * 7 / 8).contains(&y) {
            // Overcast sky above, gravel below: no colour.
            let v = (120 + (x + y) % 20) as u8;
            [v; 3]
        } else if x % 16 == 0 {
            // Dark planks between boards.
            [110, 20, 18]
        } else {
            [170, 32, 26]
        }
    }),
    ("street", |x, y| {
        let v = ((x * 2 + if (y / 12) % 2 == 0 { 40 } else { 0 }) % 256) as u8;
        [v; 3]
    }),
    ("sunset", |_, y| {
        let t = (y * 40 / H) as u8;
        [250 - t / 2, 140 - t, 40 - t / 4]
    }),
];

fn png(paint: Paint) -> Vec<u8> {
    let rgba: Vec<u8> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let [r, g, b] = paint(x, y);
            [r, g, b, 255]
        })
        .collect();
    dalica_scenes::fingerprint::png_rgba(W, H, &rgba)
}

/// The pictures as `<name>.png` in `dir/Colours`.
fn pictures(dir: &Path) -> PathBuf {
    let folder = dir.join("Colours");
    dalica_fs::create_dir_all(&folder).unwrap();
    for (name, paint) in PICTURES {
        dalica_fs::replace(&folder.join(format!("{name}.png")), &png(paint), Expect::Absent).unwrap();
    }
    folder
}

fn open(library: &Path, cache: CacheDir) -> Session {
    let options = Options { thumbs: ThumbOptions { cache, workers: 2, ..ThumbOptions::default() } };
    Session::open_with(library, Arc::new(SystemClock), &options).unwrap()
}

/// The photo named `<name>.png`.
fn id(s: &Session, name: &str) -> PhotoId {
    s.library().photos().find(|p| p.name == format!("{name}.png")).unwrap().id
}

/// The names of the photos `library.query` finds for `text`, sorted, and what it calls unavailable.
fn find(s: &mut Session, text: &str) -> (Vec<String>, Value) {
    let v = s.execute("library.query", &json!({ "text": text })).unwrap();
    let ids: Vec<u64> = v["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let mut names: Vec<String> = ids.iter().map(|n| s.library().photo(PhotoId::new(*n)).unwrap().name.trim_end_matches(".png").to_string()).collect();
    names.sort();
    (names, v["unavailable"].clone())
}

/// Each colour finds its picture and only it.
fn assert_colours(s: &mut Session, expected: &[(&str, &[&str])]) {
    for (colour, names) in expected {
        let (found, unavailable) = find(s, &format!("color:{colour}"));
        assert_eq!(found, names.to_vec(), "color:{colour}");
        assert_eq!(unavailable, json!([]), "color:{colour}");
    }
}

const EACH: [(&str, &[&str]); 5] = [("green", &["field"]), ("blue", &["sky"]), ("red", &["barn"]), ("bw", &["street"]), ("orange", &["sunset"])];

#[test]
fn a_scanned_folder_answers_each_colour_and_an_edit_moves_its_photo() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    let mut s = open(&dir.path().join("library"), CacheDir::Temp);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    assert!(s.library().photos().all(|p| s.library().colours(p.id).is_some()), "every sketch brought its summary");
    assert_colours(&mut s, &EACH);

    // Black and white on the green field: its sketch is redrawn, and its summary with it.
    let field = id(&s, "field");
    s.execute("photo.history.apply", &json!({ "id": field, "op": "bw()" })).unwrap();
    s.update_thumbnail_recipes();
    s.wait_idle();
    assert_eq!(find(&mut s, "color:green").0, Vec::<String>::new());
    assert_eq!(find(&mut s, "color:bw").0, ["field", "street"]);
    // Undo: the colour comes back.
    s.execute("photo.history.undo", &json!({ "id": field })).unwrap();
    s.update_thumbnail_recipes();
    s.wait_idle();
    assert_colours(&mut s, &EACH);
}

#[test]
fn a_library_stored_without_summaries_gains_them_from_its_sketches_without_a_decode() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    let cache = CacheDir::At(dir.path().join("cache"));
    let mut s = open(&dir.path().join("first"), cache.clone());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.flush().unwrap();
    // The same library as one indexed before summaries existed: sketches, no summaries.
    let mut library = s.library().clone();
    let roots = crate::common::read(&dir.path().join("first").join(dalica_engine::session::FOLDERS_FILE));
    drop(s);
    let ids: Vec<PhotoId> = library.photos().map(|p| p.id).collect();
    library.apply(Op::SetColours { colours: ids.iter().map(|id| (*id, None)).collect() }).unwrap();
    assert!(ids.iter().all(|id| library.sketch(*id).is_some() && library.colours(*id).is_none()));
    let lib_dir = dir.path().join("second");
    dalica_fs::create_dir_all(&lib_dir).unwrap();
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &library, 0).unwrap();
    dalica_fs::replace(&lib_dir.join(dalica_engine::session::FOLDERS_FILE), &roots, Expect::Absent).unwrap();

    let mut s = open(&lib_dir, cache);
    assert_eq!(find(&mut s, "color:green").1, json!(["color"]), "no summaries yet");
    s.wait_idle();
    let stats = s.thumbnail_stats();
    assert_eq!((stats.decodes, stats.sketches), (0, 0), "no photo was decoded, no sketch made: the summaries came from the stored sketches");
    assert!(ids.iter().all(|id| s.library().colours(*id).is_some()));
    assert_colours(&mut s, &EACH);
}

#[test]
fn search_for_a_colour_sets_the_search_text() {
    let dir = tempfile::tempdir().unwrap();
    let folder = pictures(dir.path());
    let mut s = open(&dir.path().join("library"), CacheDir::Temp);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let v = s.execute("search.color", &json!({ "color": "blue" })).unwrap();
    assert_eq!((v["text"].as_str(), v["mode"].as_str(), v["changed"].as_bool()), (Some("color:blue"), Some("search"), Some(true)));
    s.wait_search();
    let found = s.search_state().result.as_ref().map(|r| r.photos().to_vec()).unwrap_or_default();
    assert_eq!(found, [id(&s, "sky")]);
    assert_eq!(s.execute("search.color", &json!({ "color": "BLUE" })).unwrap()["changed"], json!(false), "any case, the same search");
    assert!(matches!(s.execute("search.color", &json!({ "color": "teal" })), Err(CmdError::BadParams(_))));
    assert_eq!(s.search_state().text, "color:blue", "a refused colour changes nothing");
    s.execute("search.clear", &json!({})).unwrap();
    assert_eq!(s.undo_stack().len(), 0);
}
