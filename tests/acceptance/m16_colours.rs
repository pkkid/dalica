//! M16's *Colour search* in the live app (`m16.rs` part 2): `dalica --headless` on a folder of
//! seven procedural pictures, one per search colour (a green field, a blue sky, a red barn under a
//! grey sky, a black-and-white street, a sunset, lemons and lavender), each summarised on scan from
//! its sketch. Each of the seven colours finds its picture and only it: typed as `color:<colour>`
//! in the toolbar's search field, chosen from Tools ▸ Experimental ▸ Search for ▸, and through
//! `query_photos` over `dalica-cli mcp --connect`. The window showing `color:green`'s results is
//! fingerprinted (`fingerprints/m16-1400x900-colour.json`). A B&W effect on the field moves it from
//! `color:green` to `color:bw`; Undo moves it back. Then the same library, its index snapshot
//! rewritten without any colour summary (as a library indexed before M16 is), opens with every
//! summary made again from the stored sketches: no scan job, no thumbnail written, the seven
//! colours answering as before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::{self, idle, run};
use crate::common::{LARGE, steady_shot, until};
use crate::live::{self, Client};
use crate::mcp::{self, Mcp};

const W: u32 = 160;
const H: u32 = 120;

/// Paints one pixel at (x, y).
type Paint = fn(u32, u32) -> [u8; 3];

/// The pictures, by file stem.
const PICTURES: [(&str, Paint); 7] = [
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
    ("lemons", |x, y| {
        let n = ((x * 5 + y * 3) % 30) as u8;
        [235 + n / 3, 205 + n / 2, 40 + n]
    }),
    ("lavender", |x, y| {
        let n = ((x * 3 + y * 11) % 30) as u8;
        [140 + n, 70 + n / 2, 195 + n / 2]
    }),
];

/// Each colour and the pictures it finds.
const EACH: [(&str, &[&str]); 7] = [
    ("red", &["barn"]),
    ("orange", &["sunset"]),
    ("yellow", &["lemons"]),
    ("green", &["field"]),
    ("blue", &["sky"]),
    ("purple", &["lavender"]),
    ("bw", &["street"]),
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

/// The pictures as `<name>.png` in `home/Colours`, with fixed times.
fn pictures(home: &Path) -> PathBuf {
    let folder = home.join("Colours");
    for (i, (name, paint)) in PICTURES.iter().enumerate() {
        app::put(&folder.join(format!("{name}.png")), &png(*paint), i as u64);
    }
    folder
}

/// `rel` (`"field.png"`) → `"field"`, sorted.
fn stems(rels: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = rels.into_iter().map(|r| r.trim_end_matches(".png").to_string()).collect();
    v.sort();
    v
}

/// What `library.query {text}` finds, by stem, and what it calls unavailable.
fn query(c: &mut Client, names: &BTreeMap<String, String>, text: &str) -> (Vec<String>, Value) {
    let r = c.exec("library.query", json!({ "text": text, "limit": 1000 }));
    let found = r["photos"].as_array().unwrap().iter().map(|p| names.get(&p["id"].to_string()).cloned().unwrap_or_else(|| format!("?{}", p["id"])));
    (stems(found), r["unavailable"].clone())
}

/// The search as `search.set` with nothing to change replies it, once nothing is pending (the
/// debounce runs on the app's clock, which `ui.wait` moves on headless).
fn answered(c: &mut Client) -> Value {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        c.ok("ui.wait", json!({ "ms": 200 }));
        let s = c.exec("search.set", json!({}));
        if s["pending"] == false {
            c.settle();
            return s;
        }
        assert!(Instant::now() < end, "the search was not answered within two minutes: {s}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The search's results in the window (the search album), by stem.
fn results(c: &mut Client, root: &Path) -> Vec<String> {
    stems(app::section(c, root, "album:search"))
}

/// Every photo's id (as `library.query` writes it) → its path under `root`.
fn names(c: &mut Client, root: &Path) -> BTreeMap<String, String> {
    app::photos(c, root).into_iter().map(|(rel, p)| (p.id.to_string(), rel)).collect()
}

/// Waits until every colour answers its pictures (the summaries come with the sketches, in the
/// background).
fn every_colour_answers(c: &mut Client, names: &BTreeMap<String, String>) {
    until(c, "every colour answers its pictures", |c| {
        EACH.iter().all(|(colour, want)| {
            let (found, unavailable) = query(c, names, &format!("color:{colour}"));
            found == *want && unavailable == json!([])
        })
    });
}

pub fn colours(exe: &Path, home: &Path) {
    let cli = mcp::cli_exe();
    let root = pictures(home);
    let (a, mut c) = live::start(exe, home);
    run(&mut c, "folders.add", json!({ "path": root }));
    idle(&mut c);
    let names = names(&mut c, &root);
    assert_eq!(names.len(), PICTURES.len(), "{names:?}");
    every_colour_answers(&mut c, &names);
    println!("  library.query: each of the seven colours finds its picture and only it");

    // Typed in the toolbar's search field.
    for (colour, want) in EACH {
        run(&mut c, "search.clear", json!({}));
        app::click(&mut c, "field:search");
        c.ok("ui.type", json!({ "text": format!("color:{colour}") }));
        let s = answered(&mut c);
        assert_eq!((&s["active"], &s["text"]), (&json!(true), &json!(format!("color:{colour}"))), "{s}");
        assert_eq!(results(&mut c, &root), want, "typed color:{colour}");
    }
    println!("  the search field: each colour typed shows its picture");

    // Tools ▸ Experimental ▸ Search for ▸.
    for (colour, want) in EACH {
        run(&mut c, "search.clear", json!({}));
        app::click(&mut c, "menu:Tools");
        app::click(&mut c, "submenu:Tools/Experimental");
        app::click(&mut c, "submenu:Tools/Experimental/Search for");
        app::click(&mut c, &format!("menuitem:search.color/{colour}"));
        let s = answered(&mut c);
        assert_eq!((&s["active"], &s["text"]), (&json!(true), &json!(format!("color:{colour}"))), "{s}");
        assert_eq!(results(&mut c, &root), want, "Search for ▸ {colour}");
        if colour == "green" {
            app::click(&mut c, "banner:search");
            app::park(&mut c, f64::from(LARGE.1));
            // The banner's time ("(0.00 s)") follows the machine: masked.
            let banner = app::rect_of(&mut c, "banner:search").into_iter().collect::<Vec<_>>();
            assert_eq!(banner.len(), 1, "the search banner is shown");
            steady_shot(&mut c, home, "m16-1400x900-colour", &banner);
        }
    }
    run(&mut c, "search.clear", json!({}));
    println!("  Tools ▸ Experimental ▸ Search for ▸: each colour shows its picture");

    // `query_photos` over MCP, on the same app.
    let mut m = Mcp::connect(&cli, home);
    for (colour, want) in EACH {
        let r = m.tool("query_photos", json!({ "query": format!("color:{colour}") }));
        let found = stems(r["photos"].as_array().unwrap_or_else(|| panic!("{r}")).iter().map(|p| p["name"].as_str().unwrap().to_string()));
        assert_eq!(found, want, "query_photos color:{colour}: {r}");
        assert_eq!(r["total"], json!(want.len()), "{r}");
    }
    m.finish();
    println!("  query_photos over dalica-cli mcp --connect: each colour finds its picture");

    // A B&W effect on the green field moves it to `bw`; Undo moves it back.
    let field = names.iter().find(|(_, rel)| *rel == "field.png").map(|(id, _)| id.parse::<u64>().unwrap()).unwrap();
    run(&mut c, "photo.history.apply", json!({ "id": field, "op": "bw()" }));
    until(&mut c, "the field is found as black and white", |c| {
        query(c, &names, "color:bw").0 == ["field", "street"] && query(c, &names, "color:green").0.is_empty()
    });
    println!("  B&W on the field: color:green finds nothing, color:bw finds the field and the street");
    run(&mut c, "photo.history.undo", json!({ "id": field }));
    every_colour_answers(&mut c, &names);
    live::quit(a, c);

    // The library as one indexed before M16: its snapshot without any summary.
    let library = home.join("library");
    let first = home.join("library-first");
    std::fs::rename(&library, &first).unwrap();
    std::fs::create_dir_all(&library).unwrap();
    let opened = dalica_store::Store::open(&first, dalica_core::Timestamp::from_millis(1_717_236_000_000)).unwrap();
    let mut index = opened.library;
    let ids: Vec<dalica_core::PhotoId> = index.photos().map(|p| p.id).collect();
    assert!(ids.iter().all(|id| index.sketch(*id).is_some() && index.colours(*id).is_some()), "the first run summarised every photo");
    index.apply(dalica_library::Op::SetColours { colours: ids.iter().map(|id| (*id, None)).collect() }).unwrap();
    assert!(ids.iter().all(|id| index.sketch(*id).is_some() && index.colours(*id).is_none()));
    dalica_store::snapshot::write(&library.join(dalica_store::snapshot::FILE), &index, 0).unwrap();
    for file in [dalica_engine::session::FOLDERS_FILE, "settings.json"] {
        if first.join(file).is_file() {
            std::fs::copy(first.join(file), library.join(file)).unwrap();
        }
    }
    let cache = home.join("dalica-cache");
    let cached_before = listing(&cache);
    assert!(!cached_before.is_empty(), "the first run cached its thumbnails in {}", cache.display());
    let (a, mut c) = live::start(exe, home);
    every_colour_answers(&mut c, &names);
    let kinds: Vec<Value> = app::jobs(&mut c).into_iter().map(|j| j["kind"].clone()).collect();
    assert!(!kinds.contains(&json!("scan")), "no scan ran: {kinds:?}");
    assert_eq!(listing(&cache), cached_before, "no thumbnail was made again: the summaries came from the stored sketches");
    println!("  a snapshot without summaries: every summary made again from its stored sketch, no scan, no thumbnail written");
    live::quit(a, c);
}

/// Every file under `dir` with its size, by path.
fn listing(dir: &Path) -> BTreeMap<PathBuf, u64> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.insert(p.clone(), e.metadata().map_or(0, |m| m.len()));
            }
        }
    }
    out
}
