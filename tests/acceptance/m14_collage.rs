//! The Collage room in the live app (`m14.rs` part 1): `dalica --headless` on eight painted photos
//! in `Pictures/Garden`, Ana's face on the first three, driven over its control channel:
//!
//! - **where a collage starts**: the bottom bar's Collage with three held photos, Create ▸ Picture
//!   Collage… on the tray, a folder header's Collage (the whole folder) and Ana's section's (her
//!   three) each open the room with exactly those clips;
//! - **the layouts**: each of the six styles lays the eight out the same way for the same seed,
//!   twice in a session and again after a restart, and as `fingerprints/m14-layouts.json` records
//!   them (by file name, within 10⁻⁶: the same on every system);
//! - **the canvas**: a pile's top picture dragged 80 points left and 30 down moves with the
//!   pointer, its wheel handle turns it a quarter, one step of the room's history each and never
//!   one of the library's; a grid's first picture dragged onto its second swaps them; Shuffle keeps
//!   the slots and the seed and deals the pictures again; Scramble draws a new seed and drops the
//!   moves and swaps, the clips and settings kept; the room with a pile and with a mosaic match
//!   their whole-window fingerprints;
//! - **the output**: Create Collage renders on a worker while the window keeps drawing (its
//!   progress and Cancel showing), writes a JPEG of the page's ratio at 3000 px on its long side in
//!   `Pictures/Dalica/Collages` with its `.dalica-collage` recipe beside it, and opens it in the
//!   edit view; each grid picture's colour in the JPEG is the one the canvas showed in the same
//!   place; the folder is the Collages project in the Projects group; Edit Collage reopens the room
//!   with its clips and settings; a change, then Close asks, and Save Draft writes a second collage
//!   with `"draft": true` beside the first, which is never replaced.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_ui::panels::collage::{canvas, settings};
use dalica_ui::panels::edit::top_bar;
use serde_json::{Value, json};

use crate::app::{self, click, idle, rect_of, run, secs};
use crate::common::{self, COLOURS, LARGE, Rgb, collage, room, until};
use crate::live::{self, Client, Launch};

/// The seed every layout here starts from.
const SEED: &str = "5eed14";
/// The long side Create Collage writes.
const LONG_SIDE: u32 = 3000;
/// The styles, by id.
const STYLES: [&str; 6] = ["pile", "mosaic", "frameMosaic", "grid", "contactSheet", "multipleExposure"];
/// Placements agree within this (normalised to the page; degrees for the angle).
const CLOSE: f64 = 1e-6;
/// A grid picture's mean colour on the canvas and in the JPEG agree within this, per channel.
const SAME_COLOUR: f64 = 28.0;

/// The photos: (name, width, height), painted in [`COLOURS`] in this order.
const PHOTOS: [(&str, u32, u32); 8] = [
    ("IMG_0001.jpg", 640, 480),
    ("IMG_0002.jpg", 480, 640),
    ("IMG_0003.jpg", 600, 400),
    ("IMG_0004.jpg", 640, 360),
    ("IMG_0005.jpg", 500, 500),
    ("IMG_0006.jpg", 640, 480),
    ("IMG_0007.jpg", 400, 600),
    ("IMG_0008.jpg", 640, 427),
];

/// The app with `Pictures/Dalica` (where collages go) a write root beside its temporary folder
/// (where the screenshots go); its Pictures folder `home/Pictures` on Windows too (`USERPROFILE`).
fn launch(home: &Path) -> Launch {
    Launch::default()
        .env("USERPROFILE", home)
        .arg("--automation-write-root")
        .arg(home.join("Pictures").join("Dalica"))
        .arg("--automation-write-root")
        .arg(home.join("tmp"))
}

/// The ids of the eight photos in [`PHOTOS`]' order.
fn ids(c: &mut Client, pictures: &Path) -> Vec<Value> {
    let by_path = app::photos(c, pictures);
    PHOTOS.iter().map(|(n, _, _)| by_path[&format!("Garden/{n}")].id.clone()).collect()
}

/// The open collage's clips, sorted.
fn clips(c: &mut Client) -> Vec<u64> {
    let mut v: Vec<u64> = collage(c)["clips"].as_array().map(|a| a.iter().map(|c| c["id"].as_u64().unwrap()).collect()).unwrap_or_default();
    v.sort_unstable();
    v
}

fn sorted(ids: &[Value]) -> Vec<u64> {
    let mut v: Vec<u64> = ids.iter().map(|i| i.as_u64().unwrap()).collect();
    v.sort_unstable();
    v
}

/// The room is in front with exactly `want`; then closed, its changes discarded.
fn opened_with(c: &mut Client, want: &[Value], how: &str) {
    assert_eq!(room(c), "collage", "{how}: the room is in front");
    assert_eq!(clips(c), sorted(want), "{how}: the clips");
    run(c, "collage.closeRoom", json!({ "discard": true }));
    assert!(collage(c)["open"] == false, "{how}: closed");
}

fn placements(c: &mut Client) -> Vec<Value> {
    collage(c)["placements"].as_array().cloned().unwrap_or_default()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// A placement as compared: its photo's name and its numbers.
fn layout_of(c: &mut Client, names: &[(u64, &str)]) -> Value {
    let list: Vec<Value> = placements(c)
        .iter()
        .map(|p| {
            let id = p["id"].as_u64().unwrap();
            let name = names.iter().find(|(i, _)| *i == id).map(|(_, n)| *n).unwrap_or("?");
            json!({ "name": name, "centre": p["centre"], "size": p["size"], "angle": p["angle"], "z": p["z"], "shown": p["shown"] })
        })
        .collect();
    Value::Array(list)
}

/// Whether two layouts agree: the same names in the same order, every number within [`CLOSE`].
fn same_layout(a: &Value, b: &Value) -> Result<(), String> {
    fn walk(a: &Value, b: &Value, at: &str) -> Result<(), String> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
                if (x - y).abs() <= CLOSE { Ok(()) } else { Err(format!("{at}: {x} and {y}")) }
            }
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                x.iter().zip(y).enumerate().try_for_each(|(i, (x, y))| walk(x, y, &format!("{at}[{i}]")))
            }
            (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
                x.iter().try_for_each(|(k, v)| y.get(k).ok_or_else(|| format!("{at}.{k}: missing")).and_then(|w| walk(v, w, &format!("{at}.{k}"))))
            }
            _ if a == b => Ok(()),
            _ => Err(format!("{at}: {a} and {b}")),
        }
    }
    walk(a, b, "")
}

/// Every style's layout of the eight for [`SEED`], at the default settings (the room opened with
/// the seed, Reset, the style set, the room closed).
fn layouts(c: &mut Client, ids: &[Value], names: &[(u64, &str)]) -> Value {
    let mut out = serde_json::Map::new();
    for style in STYLES {
        run(c, "create.collage", json!({ "ids": ids, "seed": SEED }));
        run(c, "collage.reset", json!({}));
        run(c, "collage.set", json!({ "style": style }));
        assert_eq!(collage(c)["document"]["style"], style);
        out.insert(style.to_string(), layout_of(c, names));
        run(c, "collage.closeRoom", json!({ "discard": true }));
    }
    Value::Object(out)
}

/// The recorded layouts (`fingerprints/m14-layouts.json`, written under `DALICA_BLESS=1`).
fn recorded(layouts: &Value) -> Value {
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fingerprints/m14-layouts.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, serde_json::to_string_pretty(layouts).unwrap() + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e} (bless with DALICA_BLESS=1)", stored.display()));
    serde_json::from_str(&text).unwrap()
}

/// The library's undo depth (`library.inspect`).
fn library_undo(c: &mut Client) -> u64 {
    c.exec("library.inspect", json!({}))["undo"]["depth"].as_u64().unwrap_or_else(|| panic!("library.inspect has the undo depth"))
}

fn room_undo(c: &mut Client) -> u64 {
    collage(c)["undo"].as_u64().unwrap()
}

/// The canvas's page on the screen, `[x, y, w, h]`.
fn page(c: &mut Client) -> [f64; 4] {
    rect_of(c, canvas::CANVAS_ID).unwrap_or_else(|| panic!("the canvas shows"))
}

/// A placement's centre on the screen.
fn on_screen(page: [f64; 4], p: &Value) -> (f64, f64) {
    (page[0] + num(&p["centre"][0]) * page[2], page[1] + num(&p["centre"][1]) * page[3])
}

/// The mean colour of the box `[x0, y0, x1, y1]` (pixels) of an RGBA image `w` wide.
fn mean(rgba: &[u8], w: u32, [x0, y0, x1, y1]: [u32; 4]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * w + x) * 4) as usize;
            for (k, s) in sum.iter_mut().enumerate() {
                *s += f64::from(rgba[i + k]);
            }
            n += 1.0;
        }
    }
    assert!(n > 0.0, "an empty box");
    sum.map(|s| s / n)
}

/// The box around a placement's centre, `fraction` of its size, in an image `w × h` showing the
/// page from `(ox, oy)`.
fn around(p: &Value, (ox, oy, w, h): (f64, f64, f64, f64), fraction: f64) -> [u32; 4] {
    let (cx, cy) = (ox + num(&p["centre"][0]) * w, oy + num(&p["centre"][1]) * h);
    let (hw, hh) = (num(&p["size"][0]) * w * fraction / 2.0, num(&p["size"][1]) * h * fraction / 2.0);
    [(cx - hw).round() as u32, (cy - hh).round() as u32, (cx + hw).round() as u32, (cy + hh).round() as u32]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

fn rgb(c: Rgb) -> [f64; 3] {
    c.map(f64::from)
}

pub fn collage_room(exe: &Path, home: &Path) {
    let started = Instant::now();
    let pictures = home.join("Pictures");
    let garden = pictures.join("Garden");
    for (i, (name, w, h)) in PHOTOS.iter().enumerate() {
        app::put(&garden.join(name), &common::painted_jpeg(*w, *h, COLOURS[i]), i as u64);
    }
    let photos_of = |dir: &Path| -> Vec<(String, Vec<u8>)> { common::files(dir).into_iter().filter(|(n, _)| n.ends_with(".jpg")).collect() };
    let originals = photos_of(&garden);
    // The write root exists (a root that does not exist holds nothing, `security.md` §5.7).
    std::fs::create_dir_all(pictures.join("Dalica")).unwrap();
    let (a, mut cl) = live::start_with(exe, home, launch(home));
    let c = &mut cl;
    app::resize(c, LARGE);
    run(c, "folders.add", json!({ "path": garden }));
    let ids = ids(c, &pictures);
    let names: Vec<(u64, &str)> = ids.iter().zip(PHOTOS).map(|(i, (n, _, _))| (i.as_u64().unwrap(), n)).collect();
    let folder = app::photos(c, &pictures)["Garden/IMG_0001.jpg"].folder;
    let ana = run(c, "people.create", json!({ "name": "Ana" }))["person"].as_str().unwrap().to_string();
    for id in &ids[..3] {
        run(c, "faces.add", json!({ "id": id, "rect": [0.3, 0.15, 0.65, 0.75], "person": ana }));
    }

    // Where a collage starts.
    run(c, "select.set", json!({ "ids": [ids[2], ids[4], ids[6]] }));
    run(c, "tray.hold", json!({}));
    run(c, "select.set", json!({ "ids": [] }));
    click(c, "action:collage");
    opened_with(c, &[ids[2].clone(), ids[4].clone(), ids[6].clone()], "the bottom bar's Collage with three held photos");
    run(c, "select.set", json!({ "ids": [ids[7]] }));
    click(c, "menu:Create");
    click(c, "menuitem:create.collage");
    opened_with(c, &[ids[2].clone(), ids[4].clone(), ids[6].clone(), ids[7].clone()], "Create ▸ Picture Collage… on the tray");
    run(c, "tray.clear", json!({ "confirm": true }));
    run(c, "select.set", json!({ "ids": [] }));
    click(c, &format!("section:folder:{folder}/collage"));
    opened_with(c, &ids, "a folder header's Collage");
    click(c, &format!("tree:person:{ana}"));
    click(c, &format!("section:person:{ana}/collage"));
    opened_with(c, &ids[..3], "Ana's section's Collage");
    click(c, &format!("tree:folder:{folder}"));
    println!("  the tray, Create ▸ Picture Collage…, a folder header and a person section open the room with their photos");

    // Each style lays the eight out the same way for the same seed.
    let first = layouts(c, &ids, &names);
    let again = layouts(c, &ids, &names);
    for style in STYLES {
        same_layout(&first[style], &again[style]).unwrap_or_else(|e| panic!("{style}: the same seed laid out differently: {e}"));
        assert_eq!(first[style].as_array().map(Vec::len), Some(8), "{style}: every picture placed");
    }
    let stored = recorded(&first);
    for style in STYLES {
        same_layout(&first[style], &stored[style])
            .unwrap_or_else(|e| panic!("{style}: not the recorded layout (fingerprints/m14-layouts.json): {e}"));
    }
    println!("  the six styles lay the same seed out the same way, twice, and as recorded");

    // A pile: the top picture moved and turned, one step of the room's history each.
    let library_steps = library_undo(c);
    run(c, "create.collage", json!({ "ids": &ids[..6], "seed": SEED }));
    run(c, "collage.reset", json!({}));
    run(c, "collage.open", json!({}));
    assert_eq!((room(c), &collage(c)["document"]["style"]), ("collage".to_string(), &json!("pile")));
    let page_r = page(c);
    let top = placements(c).len() - 1;
    let (before, steps) = (placements(c)[top].clone(), room_undo(c));
    c.ok("ui.dragWidget", json!({ "id": canvas::item_id(top as u32), "dx": -80.0, "dy": 30.0, "steps": 12 }));
    idle(c);
    let after = placements(c)[top].clone();
    assert_eq!(after["id"], before["id"], "the same picture on top");
    let (b, a2) = (on_screen(page_r, &before), on_screen(page_r, &after));
    assert!(((a2.0 - b.0) + 80.0).abs() <= 1.5 && ((a2.1 - b.1) - 30.0).abs() <= 1.5, "moved with the pointer: {b:?} → {a2:?}");
    assert_eq!(room_undo(c), steps + 1, "one step of the room's history");
    let centre = on_screen(page_r, &after);
    let wheel = app::center(rect_of(c, canvas::HANDLE_ID).unwrap_or_else(|| panic!("the wheel handle shows on the selected picture")));
    let arm = (wheel.0 - centre.0, wheel.1 - centre.1);
    c.ok("ui.dragWidget", json!({ "id": canvas::HANDLE_ID, "toX": centre.0 - arm.1, "toY": centre.1 + arm.0, "steps": 24 }));
    idle(c);
    let turned = placements(c)[top].clone();
    let swept = (num(&turned["angle"]) - num(&after["angle"]) + 540.0).rem_euclid(360.0) - 180.0;
    assert!((swept - 90.0).abs() < 1.0, "a quarter turn: {} → {}", after["angle"], turned["angle"]);
    assert!((num(&turned["size"][0]) / num(&after["size"][0]) - 1.0).abs() < 0.02, "the same distance keeps the size");
    assert_eq!(room_undo(c), steps + 2, "one more step");
    assert_eq!(library_undo(c), library_steps, "never a step of the library's undo stack");
    common::steady_shot(c, home, "m14-1400x900-room-pile", &[]);
    println!("  a pile's picture moved with the pointer and turned a quarter by its wheel, one step each, none in the library");

    // Scramble: a new seed, the move dropped; the clips and settings kept.
    let before = collage(c)["document"].clone();
    click(c, "button:collageCanvas.scramble");
    let after = collage(c)["document"].clone();
    assert_ne!(after["seed"], before["seed"], "Scramble draws a new seed");
    assert_eq!(
        (&after["clips"], &after["settings"], &after["style"]),
        (&before["clips"], &before["settings"], &before["style"]),
        "the clips and settings stay"
    );
    assert!(
        after["moves"].as_array().is_none_or(Vec::is_empty) && after["moves"].as_object().is_none_or(serde_json::Map::is_empty),
        "the moves dropped: {after}"
    );

    // A grid: the first picture dragged onto the second swaps them.
    run(c, "collage.set", json!({ "style": "grid" }));
    let page_r = page(c);
    let before = placements(c);
    c.ok("ui.dragWidget", json!({ "id": canvas::item_id(0), "toWidget": canvas::item_id(1), "steps": 12 }));
    idle(c);
    let after = placements(c);
    let at = |list: &[Value], id: &Value| on_screen(page_r, list.iter().find(|p| p["id"] == *id).unwrap());
    let near = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() <= 1.0 && (p.1 - q.1).abs() <= 1.0;
    assert!(near(at(&after, &before[0]["id"]), on_screen(page_r, &before[1])), "the first is where the second was");
    assert!(near(at(&after, &before[1]["id"]), on_screen(page_r, &before[0])), "and the second where the first was");
    for p in &before[2..] {
        assert!(near(at(&after, &p["id"]), on_screen(page_r, p)), "the others stay");
    }

    // Shuffle: the same slots and seed, the pictures dealt again.
    let doc_before = collage(c)["document"].clone();
    let slots = |list: &[Value]| {
        let mut s: Vec<String> = list
            .iter()
            .map(|p| format!("{:.6} {:.6} {:.6} {:.6}", num(&p["centre"][0]), num(&p["centre"][1]), num(&p["size"][0]), num(&p["size"][1])))
            .collect();
        s.sort();
        s
    };
    let deal = |list: &[Value]| {
        list.iter()
            .map(|p| (format!("{:.6} {:.6}", num(&p["centre"][0]), num(&p["centre"][1])), p["id"].clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    click(c, "button:collageCanvas.shuffle");
    let shuffled = placements(c);
    let doc_after = collage(c)["document"].clone();
    assert_eq!(slots(&shuffled), slots(&after), "Shuffle keeps the slots");
    assert_ne!(deal(&shuffled), deal(&after), "and deals the pictures to them again");
    assert_eq!(
        (&doc_after["seed"], &doc_after["clips"], &doc_after["settings"]),
        (&doc_before["seed"], &doc_before["clips"], &doc_before["settings"]),
        "the seed, clips and settings stay"
    );
    println!("  a grid swap, Shuffle (same slots and seed) and Scramble (a new seed, the moves dropped) each change only what they say");

    // A mosaic of the same six and seed, fingerprinted (Scramble drew a seed of its own).
    run(c, "collage.closeRoom", json!({ "discard": true }));
    run(c, "create.collage", json!({ "ids": &ids[..6], "seed": SEED }));
    run(c, "collage.open", json!({}));
    run(c, "collage.set", json!({ "style": "mosaic" }));
    run(c, "collageCanvas.selectNone", json!({}));
    common::steady_shot(c, home, "m14-1400x900-room-mosaic", &[]);

    // The canvas and the created JPEG agree: the grid, nothing selected.
    run(c, "collage.set", json!({ "style": "grid" }));
    run(c, "collageCanvas.selectNone", json!({}));
    let page_r = page(c);
    let shot = common::steady(c, home, "m14-1400x900-grid-before-create");
    let grid = placements(c);
    let room_doc = collage(c);
    let library_steps = library_undo(c);
    let collages = pictures.join("Dalica").join("Collages");
    c.ok("ui.clickWidget", json!({ "id": settings::CREATE_ID }));
    // The render runs on a worker: the window keeps drawing, with its progress and Cancel.
    let mut saw_progress = false;
    for _ in 0..20 {
        c.ok("ui.settle", json!({ "timeoutMs": 2000 }));
        if app::widget(c, settings::PROGRESS_ID).is_some() {
            assert!(app::widget(c, settings::CANCEL_ID).is_some(), "Cancel beside the progress");
            saw_progress = true;
            break;
        }
        if c.state()["view"] == "edit" {
            break;
        }
    }
    until(c, "the new collage in the edit view", |c| c.state()["view"] == "edit" && collage_job_done(c));
    let made = c.state()["edit"]["photo"].clone();
    assert!(!made.is_null() && !ids.contains(&made), "a new photo in the edit view: {made}");
    let info = c.inspect(&made);
    let jpeg = PathBuf::from(info["path"].as_str().unwrap());
    assert_eq!(jpeg.parent(), Some(collages.as_path()), "in Pictures/Dalica/Collages: {}", jpeg.display());
    let recipe = jpeg.with_extension("dalica-collage");
    assert!(recipe.is_file(), "its recipe beside it: {}", recipe.display());
    let bytes = std::fs::read(&jpeg).unwrap();
    let image = dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap();
    let (w, h) = (image.width, image.height);
    assert_eq!(w.max(h), LONG_SIDE, "{w} × {h}");
    let (pw, ph) = (num(&room_doc["page"]["width"]), num(&room_doc["page"]["height"]));
    assert_eq!((f64::from(w), f64::from(h)), (pw, ph), "the page's size");
    let ratio = num(&room_doc["page"]["ratio"]);
    assert!((f64::from(w) / f64::from(h) - ratio).abs() <= ratio / f64::from(w.min(h)), "the page's ratio {ratio}: {w} × {h}");
    let saved: Value = serde_json::from_slice(&std::fs::read(&recipe).unwrap()).unwrap();
    assert_eq!((&saved["style"], saved.get("draft").cloned().unwrap_or(json!(false))), (&json!("grid"), json!(false)), "{saved}");
    assert_eq!(library_undo(c), library_steps, "creating a collage is not an undo step");
    for (n, p) in grid.iter().enumerate() {
        let on_canvas = mean(&shot.rgba, shot.width, around(p, (page_r[0], page_r[1], page_r[2], page_r[3]), 0.3));
        let in_file = mean(&image.data, w, around(p, (0.0, 0.0, f64::from(w), f64::from(h)), 0.3));
        let photo = ids.iter().position(|i| *i == p["id"]).unwrap();
        assert!(
            distance(on_canvas, in_file) <= SAME_COLOUR,
            "picture {n} ({}): the canvas showed {on_canvas:?}, the JPEG has {in_file:?}",
            PHOTOS[photo].0
        );
        let nearest = COLOURS.iter().enumerate().min_by(|a, b| distance(in_file, rgb(*a.1)).total_cmp(&distance(in_file, rgb(*b.1)))).unwrap().0;
        assert_eq!(nearest, photo, "picture {n}: the JPEG shows {} there", PHOTOS[nearest].0);
    }
    assert!(photos_of(&garden) == originals, "the photos are untouched");
    println!(
        "  Create Collage ({}): {w} × {h} at the page's ratio, its recipe beside it, every grid picture's colour where the canvas showed it",
        if saw_progress { "its progress and Cancel shown while it rendered" } else { "rendered before a frame showed its progress" }
    );

    // The Collages project in the Projects group.
    until(c, "the Collages folder in the Projects group with the collage", |c| projects_row(c, &collages).is_some_and(|r| r["count"] == 1));
    let folder_mark: Value = serde_json::from_slice(&std::fs::read(collages.join(".dalica.json")).unwrap_or_default()).unwrap_or(Value::Null);
    assert_eq!(folder_mark["folder"]["project"], "collages", "the folder is marked the Collages project: {folder_mark}");

    // Edit Collage reopens it with its clips and settings.
    run(c, "collage.closeRoom", json!({ "discard": true }));
    until(c, "Edit Collage in the edit view's top bar", |c| app::widget(c, top_bar::EDIT_COLLAGE_ID).is_some());
    click(c, top_bar::EDIT_COLLAGE_ID);
    until(c, "the room reopened", |c| collage(c)["open"] == true);
    let reopened = collage(c);
    assert_eq!(room(c), "collage");
    assert_eq!(
        (&reopened["document"]["style"], &reopened["document"]["settings"]),
        (&room_doc["document"]["style"], &room_doc["document"]["settings"]),
        "its settings"
    );
    assert_eq!(clips(c), sorted(&ids[..6]), "its clips");

    // A change, then Close asks; Save Draft writes a second collage beside the first.
    let first_bytes = std::fs::read(&jpeg).unwrap();
    click(c, settings::ORIENTATION_IDS[1]);
    assert_eq!(collage(c)["unsaved"], true);
    click(c, settings::CLOSE_ID);
    assert_eq!(app::dialog(c).as_deref(), Some("collageClose"), "Close asks");
    click(c, "button:dialog.cancel");
    assert_eq!(collage(c)["open"], true, "Cancel keeps the room");
    click(c, settings::CLOSE_ID);
    click(c, "button:dialog.saveDraft");
    until(c, "the draft written and the room closed", |c| collage(c)["open"] == false);
    let written = common::files(&collages);
    let jpegs: Vec<&String> = written.keys().filter(|n| n.ends_with(".jpg")).collect();
    let recipes: Vec<(&String, &Vec<u8>)> = written.iter().filter(|(n, _)| n.ends_with(".dalica-collage")).collect();
    assert_eq!((jpegs.len(), recipes.len()), (2, 2), "{:?}", written.keys().collect::<Vec<_>>());
    let drafts: Vec<&String> =
        recipes.iter().filter(|(_, b)| serde_json::from_slice::<Value>(b).is_ok_and(|v| v["draft"] == true)).map(|(n, _)| *n).collect();
    assert_eq!(drafts.len(), 1, "one draft: {drafts:?}");
    let draft_jpeg = drafts[0].replace(".dalica-collage", ".jpg");
    assert!(written.contains_key(&draft_jpeg), "the draft's picture beside its recipe: {draft_jpeg}");
    let first_name = jpeg.file_name().unwrap().to_string_lossy().into_owned();
    // The same day's name taken, the next is `-001` (a draft made after midnight has the new date).
    let same_day = draft_jpeg.starts_with(first_name.trim_end_matches(".jpg"));
    assert!(draft_jpeg != first_name && (!same_day || draft_jpeg.ends_with("-001.jpg")), "a new name, the first never replaced: {draft_jpeg}");
    assert_eq!(std::fs::read(&jpeg).unwrap(), first_bytes, "the first collage is never replaced");
    println!("  Edit Collage reopened it with its clips and settings; Close asked; Save Draft wrote {draft_jpeg} beside it");
    live::quit(a, cl);

    // Another run: the same layouts.
    let (a, mut cl) = live::start_with(exe, home, launch(home));
    let c = &mut cl;
    app::resize(c, LARGE);
    let again = layouts(c, &ids, &names);
    for style in STYLES {
        same_layout(&first[style], &again[style]).unwrap_or_else(|e| panic!("{style}: laid out differently after a restart: {e}"));
    }
    live::quit(a, cl);
    println!("  after a restart the six styles lay the seed out the same way again");
    println!("  M14 collage room in {:.1} s", secs(started));
}

/// No collage job queued or running (`jobs.list`).
fn collage_job_done(c: &mut Client) -> bool {
    app::jobs(c).iter().all(|j| j["kind"] != "collage" || (j["state"] != "queued" && j["state"] != "running"))
}

/// The Projects group's row of `library.inspect`'s tree for the folder at `path`.
fn projects_row(c: &mut Client, path: &Path) -> Option<Value> {
    let tree = c.exec("library.inspect", json!({}))["tree"].clone();
    let groups = tree["groups"].as_array()?;
    let projects = groups.iter().find(|g| g["group"] == "projects")?;
    projects["rows"].as_array()?.iter().find(|r| r["path"].as_str().map(Path::new) == Some(path)).cloned()
}
