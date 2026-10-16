//! M6 "Photo info, metadata and Release 0.1" (`docs/roadmap.md` → M6 → the *Done when* of Photo
//! info, of Tags and metadata write-back, and of Release 0.1; run by `cargo xtask accept M6`).
//!
//! 1. **The live app** (`m6_live.rs`): the real `dalica` binary in `--headless` mode on a fresh
//!    library, driven over its control channel as an agent would. A folder of nine `scenes` JPEGs
//!    of one picture, stored with EXIF orientations 1–8, and a second folder. Rotations and flips
//!    from the commands: the cells take the displayed shape (a quarter turn is a portrait cell),
//!    and photos whose recipes add up to the same turn show the same pixels whatever their EXIF
//!    orientation (a clockwise turn of orientation 1 and of 2; 180° made of two clockwise and of two
//!    counterclockwise turns; a left–right flip and a top–bottom flip turned 180°; four quarter
//!    turns and none), while different turns differ; one undo and redo. A photo and a folder
//!    hidden, captions, tags with partial counts, dates set and shifted, folder description, place
//!    and date. Whole-window screenshots at 1400 × 900 (captions shown) and 900 × 600 (View →
//!    Hidden Pictures: the hidden photo dimmed, the Hidden Folders group) are matched against
//!    `fingerprints/m6-*.json`. Every new M6 command is registered and live with its params doc,
//!    refuses hostile params without changing anything, and each dialog M6 adds opens and cancels.
//! 2. **Metadata write-back in the live app:** the captions, tags and dates reach the JPEG files
//!    (read back by `dalica-meta`, and by `exiv2` when it is installed: another application) with
//!    the pixels untouched, and `photo.metadata` reports them from the file.
//! 3. **A rebuild:** the app quits, the index is deleted, the app starts again and rebuilds from the
//!    sidecars: hide (photo and folder), captions, tags, dates, rotations and folder info are all
//!    back, and the window looks as before (the 1400 × 900 fingerprint again).
//! 4. **The crate suites, run as they are** (`m6_checks.rs`): the model, recipe, sidecar and index
//!    fields; every M6 command's contract (behaviour, undo and redo, `enabled`, hostile params, the
//!    sidecar after it, surviving a rebuild) in the engine suites; the panes, dialogs and cells;
//!    `xmp-interop.md` §8 tests 1–4 and 6–10 in the `meta`, `store`, `fs` and `library` suites;
//!    import on scan and write-back end to end; the application id, About and the release notes.
//!    Inside `cargo xtask ci` its test step already ran them, so they are skipped there
//!    (`dalica_acceptance::SUITES_RAN`). `panic_hunt -- --ignored` always runs.
//! 5. **The metadata corpus**, when `corpus/metadata/` is present and verified by `cargo xtask
//!    corpus`: every file other applications wrote reads, and after a Dalica write of each owned
//!    field every byte outside the owned properties is kept (`dalica-meta`'s corpus test in
//!    release, with its floors). When it is not, or inside `ci`, this says so plainly; the CI
//!    `corpus` job is the strict run.
//! 6. **The release dry run:** the latest finished run of the Release workflow on `main` (else the
//!    newest on any branch, said which), read through `gh`: built on Linux, Windows and macOS, its
//!    checksums written and verified on all three. Said plainly when `gh` cannot answer, and not
//!    read inside `ci`.
//!
//! The manual checks (rotate, hide and caption photos in the app; a tag seen in another photo
//! application; installing a package and browsing your own photos) are in `m6.md`.

#[path = "m6_checks.rs"]
mod checks;
#[path = "m6_live.rs"]
mod live;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_scenes::{Exif, Scene};
use live::{Client, difference, variation};
use serde_json::{Value, json};

/// The photos of the `Lisbon` folder: (name, EXIF orientation), all of one picture.
const TURNS: [(&str, u8); 9] = [
    ("IMG_0001.jpg", 1),
    ("IMG_0002.jpg", 2),
    ("IMG_0003.jpg", 3),
    ("IMG_0004.jpg", 4),
    ("IMG_0005.jpg", 5),
    ("IMG_0006.jpg", 6),
    ("IMG_0007.jpg", 7),
    ("IMG_0008.jpg", 8),
    ("IMG_0009.jpg", 1),
];
/// The picture every photo of the folder shows, and its upright size.
const SCENE: u64 = 61;
const W: u32 = 600;
const H: u32 = 400;

/// The caption of IMG_0001 (Unicode: a middle dot and a non-ASCII letter).
const CAPTION_1: &str = "Tram 28 · Graça";
const CAPTION_3: &str = "Miradouro de Santa Luzia";

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

/// Writes the two folders into `home/Pictures`: `Lisbon` (the nine photos of [`TURNS`], taken a
/// minute apart on 1 May 2024) and `Private` (two photos). Every file's time is fixed, so the
/// order and the headers are the same in every time zone. Returns both folders and the files'
/// first bytes by name.
fn photos(home: &Path) -> (PathBuf, PathBuf, BTreeMap<String, Vec<u8>>) {
    let lisbon = home.join("Pictures").join("Lisbon");
    let private = home.join("Pictures").join("Private");
    let mut bytes = BTreeMap::new();
    for (folder, list) in [(&lisbon, TURNS.to_vec()), (&private, vec![("IMG_0101.jpg", 1u8), ("IMG_0102.jpg", 1)])] {
        std::fs::create_dir_all(folder).unwrap();
        for (i, (name, o)) in list.into_iter().enumerate() {
            let seed = if folder == &lisbon { SCENE } else { 200 + i as u64 };
            let taken: dalica_core::DateTime = format!("2024-05-01T09:{:02}:00", i).parse().unwrap();
            let exif = Exif {
                orientation: o,
                date_time_original: Some(taken),
                make: Some("Dalica".into()),
                model: Some("Scenes 1".into()),
                ..Exif::default()
            };
            let jpeg = dalica_scenes::jpeg(&Scene::new(seed, W, H), 90, Some(&exif)).unwrap();
            let path = folder.join(name);
            std::fs::write(&path, &jpeg).unwrap();
            let at = UNIX_EPOCH + Duration::from_secs(1_714_554_000 + 60 * i as u64);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
            bytes.insert(name.to_string(), jpeg);
        }
    }
    (lisbon, private, bytes)
}

/// Every photo of the library by name (`photo.inspect`), the hidden ones and those of hidden
/// folders included. A plain `library.query` leaves those out unless the view shows hidden
/// pictures (M10.40), and the headless window's View ▸ Hidden Pictures is not stored in the
/// session, so each photo is asked for by id: the library gives ids out one by one from 1
/// (`Library::next_photo_id`), and `library.inspect`'s photo count says when all were found.
fn by_name(c: &mut Client) -> BTreeMap<String, Value> {
    let inspected = c.ok("engine.execute", json!({ "command": "library.inspect", "params": {} }));
    let count = inspected["counts"]["photos"].as_u64().unwrap_or_else(|| panic!("library.inspect counts the photos: {inspected}"));
    let mut all = BTreeMap::new();
    let mut id = 0u64;
    while (all.len() as u64) < count {
        id += 1;
        assert!(id <= count + 10_000, "only {} of {count} photos found by id: {:?}", all.len(), all.keys());
        let r = c.send("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [id] } }));
        if r["ok"] == true {
            let p = r["result"][0].clone();
            all.insert(p["name"].as_str().unwrap().to_string(), p);
        }
    }
    all
}

/// The decoded pixels of a JPEG.
fn pixels(bytes: &[u8]) -> Vec<u8> {
    dalica_image::decode(bytes, &dalica_image::Limits::default()).unwrap().data
}

/// The owned metadata fields a file holds, as another reader (`dalica-meta`) sees them.
fn file_fields(path: &Path) -> dalica_meta::OwnedFields {
    dalica_meta::read(&std::fs::read(path).unwrap(), &dalica_meta::Limits::default()).unwrap().owned
}

/// One photo's expected state: recipe, displayed size, caption, tags, hidden, effective date.
type Expected = (Vec<&'static str>, (u32, u32), Option<&'static str>, Vec<&'static str>, bool, &'static str);

/// What each photo of the `Lisbon` folder should be after part 1: (recipe, displayed size, caption,
/// tags, hidden, effective date).
fn expected() -> BTreeMap<&'static str, Expected> {
    let land = (W, H);
    let port = (H, W);
    BTreeMap::from([
        ("IMG_0001.jpg", (vec!["rotate(90)"], port, Some(CAPTION_1), vec!["lisbon", "tram"], false, "2024-05-01T09:00:00")),
        ("IMG_0002.jpg", (vec!["rotate(90)"], port, None, vec!["lisbon", "tram"], false, "2024-05-04T10:30:00")),
        ("IMG_0003.jpg", (vec!["rotate(180)"], land, Some(CAPTION_3), vec!["lisbon", "tram"], false, "2024-05-01T10:32:00")),
        ("IMG_0004.jpg", (vec!["flip_h()"], land, None, vec!["beach"], false, "2024-05-01T09:03:00")),
        ("IMG_0005.jpg", (vec!["rotate(180)"], land, None, vec![], false, "2024-05-01T09:04:00")),
        ("IMG_0006.jpg", (vec![], land, None, vec![], false, "2024-05-01T09:05:00")),
        ("IMG_0007.jpg", (vec!["flip_v()", "rotate(180)"], land, None, vec![], false, "2024-05-01T09:06:00")),
        ("IMG_0008.jpg", (vec![], land, None, vec![], false, "2024-05-01T09:07:00")),
        ("IMG_0009.jpg", (vec![], land, None, vec![], true, "2024-05-01T09:08:00")),
    ])
}

/// Checks every photo of `Lisbon` against [`expected`] (`photo.inspect`).
fn check_photos(c: &mut Client, when: &str) {
    let photos = by_name(c);
    for (name, (edits, displayed, caption, tags, hidden, date)) in expected() {
        let p = photos.get(name).unwrap_or_else(|| panic!("{when}: no {name}"));
        let i = c.inspect(&p["id"]);
        assert_eq!(i["recipe"]["edits"], json!(edits), "{when}: {name}: recipe");
        assert_eq!(
            (i["displayed"]["width"].as_u64(), i["displayed"]["height"].as_u64()),
            (Some(u64::from(displayed.0)), Some(u64::from(displayed.1))),
            "{when}: {name}"
        );
        let shown = i["caption"].as_str().filter(|s| !s.is_empty());
        assert_eq!(shown, caption, "{when}: {name}: caption");
        let mut have: Vec<String> = i["tags"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_lowercase()).collect();
        have.sort();
        assert_eq!(have, tags, "{when}: {name}: tags");
        assert_eq!(i["hidden"], hidden, "{when}: {name}: hidden");
        assert!(
            i["effectiveDate"].as_str().is_some_and(|d| d.starts_with(date)),
            "{when}: {name}: effective date {} (want {date})",
            i["effectiveDate"]
        );
    }
}

/// The folder's description, date and place (`folder.info.set` with no change answers them).
fn folder_info(c: &mut Client, folder: &Value) -> Value {
    let r = c.ok("engine.execute", json!({ "command": "folder.info.set", "params": { "id": folder } }));
    assert_eq!(r["changed"], false, "{r}");
    json!({ "description": r["description"], "date": r["date"], "place": r["place"] })
}

/// Every command M6 adds (and the View and pane items it makes live), as `engine.commands` lists
/// them.
const M6_COMMANDS: [&str; 35] = [
    "photo.rotate.cw",
    "photo.rotate.ccw",
    "edit.flipH",
    "edit.flipV",
    "photo.hide",
    "photo.unhide",
    "folder.hide",
    "folder.unhide",
    "photo.caption.set",
    "folder.info.set",
    "photo.tags.add",
    "photo.tags.remove",
    "photo.tags.toggle",
    "tags.list",
    "tags.rename",
    "tags.quick.get",
    "tags.quick.set",
    "tags.quick.apply",
    "tags.quick.1",
    "tags.quick.5",
    "tags.quick.10",
    "photo.dates.set",
    "photo.metadata",
    "photo.inspect",
    "view.hiddenPictures",
    "view.caption",
    "pane.properties",
    "pane.tags",
    "photo.caption.edit",
    "section.properties",
    "photo.adjustDate",
    "quicktags.configure",
    "tag.rename",
    "app.about",
    "help.releaseNotes",
];

/// Part 1's contract checks: every M6 command is registered and live with a label and a params
/// doc; hostile params are refused and change nothing.
fn contract(c: &mut Client, photo: &Value, folder: &Value) {
    let listed = c.ok("engine.commands", json!({}));
    let listed = listed.as_array().cloned().or_else(|| listed["commands"].as_array().cloned()).unwrap_or_default();
    for id in M6_COMMANDS {
        let cmd = listed.iter().find(|c| c["id"] == id).unwrap_or_else(|| panic!("{id} is not registered"));
        assert_eq!(cmd["live"], true, "{id}: {cmd}");
        assert!(cmd["label"].as_str().is_some_and(|l| !l.is_empty()) && cmd["params"].as_str().is_some_and(|p| !p.is_empty()), "{id}: {cmd}");
    }
    let before: Vec<Value> = by_name(c).values().map(|p| c.inspect(&p["id"])).collect();
    let huge = "x".repeat(20 * 1024);
    let section = format!("folder:{folder}");
    let hostile: Vec<(&str, Value)> = vec![
        ("photo.rotate.cw", json!({ "ids": "x" })),
        ("photo.rotate.cw", json!({ "ids": [999_999] })),
        ("photo.rotate.ccw", json!({})),
        ("edit.flipH", json!({ "ids": [-1] })),
        ("edit.flipV", json!({ "ids": [1.5] })),
        ("photo.hide", json!({ "ids": [999_999] })),
        ("photo.unhide", json!({ "ids": [photo] })),
        ("folder.hide", json!({ "section": "folder:999999" })),
        ("folder.unhide", json!({ "section": section })),
        ("folder.hide", json!({ "section": 5 })),
        ("photo.caption.set", json!({ "ids": [photo], "caption": 5 })),
        ("photo.caption.set", json!({ "ids": [photo] })),
        ("folder.info.set", json!({ "id": 999_999, "description": "x" })),
        ("folder.info.set", json!({ "id": folder, "date": "2024-13-45" })),
        ("folder.info.set", json!({ "id": folder, "place": 1 })),
        ("photo.tags.add", json!({ "ids": [photo], "tags": 5 })),
        ("photo.tags.add", json!({ "ids": "x", "tags": "a" })),
        ("photo.tags.remove", json!({ "ids": [999_999], "tags": "lisbon" })),
        ("photo.tags.toggle", json!({ "ids": [photo] })),
        ("tags.list", json!({ "ids": "x" })),
        ("tags.rename", json!({ "from": "tram", "to": "streetcar" })),
        ("tags.rename", json!({ "from": 5, "to": "x", "confirm": true })),
        ("tags.quick.set", json!({})),
        ("tags.quick.set", json!({ "slots": [1, 2] })),
        ("tags.quick.apply", json!({ "n": 0, "ids": [photo] })),
        ("tags.quick.apply", json!({ "n": 11, "ids": [photo] })),
        ("tags.quick.1", json!({ "ids": "x" })),
        ("photo.dates.set", json!({ "ids": [photo], "set": "not a date" })),
        ("photo.dates.set", json!({ "ids": [photo], "shift": "x" })),
        ("photo.dates.set", json!({ "ids": [photo] })),
        ("photo.metadata", json!({ "id": 999_999 })),
        ("photo.metadata", json!({ "id": "x" })),
        ("photo.inspect", json!({ "ids": [999_999] })),
        ("view.caption", json!({ "caption": "bogus" })),
        ("view.hiddenPictures", json!({ "on": "yes" })),
        ("photo.caption.edit", json!({ "id": "x" })),
        ("section.properties", json!({ "folder": -1 })),
        ("photo.adjustDate", json!({ "ids": "x" })),
        ("tag.rename", json!({ "tag": 5 })),
    ];
    let mut codes: BTreeMap<String, usize> = BTreeMap::new();
    for (id, params) in &hostile {
        let e = c.refused(id, params.clone());
        *codes.entry(e["code"].as_str().unwrap_or("?").to_string()).or_default() += 1;
    }
    c.settle();
    let after: Vec<Value> = by_name(c).values().map(|p| c.inspect(&p["id"])).collect();
    assert_eq!(after, before, "hostile params changed nothing");
    // A caption past the cap (`security.md` §4.4) is cut to 16 KiB, one undo step.
    c.run("photo.caption.set", json!({ "ids": [photo], "caption": huge }));
    assert_eq!(c.inspect(photo)["caption"].as_str().map(str::len), Some(16 * 1024), "a long caption is cut to the cap");
    c.run("edit.undo", json!({}));
    let undone: Vec<Value> = by_name(c).values().map(|p| c.inspect(&p["id"])).collect();
    assert_eq!(undone, before, "one undo takes the long caption back");
    let codes: Vec<String> = codes.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "contract: {} M6 commands registered and live; {} hostile calls refused ({}), nothing changed",
        M6_COMMANDS.len(),
        hostile.len(),
        codes.join(", ")
    );
}

/// Opens each dialog M6 adds and cancels it.
fn dialogs(c: &mut Client, photo: &Value, folder: &Value) {
    let opened: Vec<(&str, Value)> = vec![
        ("app.about", json!({})),
        ("help.releaseNotes", json!({})),
        ("quicktags.configure", json!({})),
        ("section.properties", json!({ "folder": folder })),
        ("photo.adjustDate", json!({ "ids": [photo] })),
        ("tag.rename", json!({ "tag": "tram" })),
    ];
    let mut names = Vec::new();
    for (id, params) in opened {
        let r = c.run(id, params);
        let dialog = r["dialog"].as_str().unwrap_or_else(|| panic!("{id}: no dialog: {r}")).to_string();
        c.ok("ui.dialog.cancel", json!({}));
        c.settle();
        names.push(dialog);
    }
    println!("dialogs opened and cancelled: {}", names.join(", "));
}

/// Lets the app run (its frames poll the session, which takes the writes' results and registers
/// their job) until the file `name` in `folder` holds what `done` wants; returns its fields.
fn written(c: &mut Client, folder: &Path, name: &str, done: impl Fn(&dalica_meta::OwnedFields) -> bool) -> dalica_meta::OwnedFields {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let f = file_fields(&folder.join(name));
        if done(&f) {
            return f;
        }
        assert!(Instant::now() < end, "{name}: not written within 60 s: {f:?}");
        c.ok("ui.wait", json!({ "ms": 100 }));
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `exiv2 -px FILE` (another photo application's view of the XMP), when `exiv2` is installed.
fn exiv2_xmp(path: &Path) -> Option<String> {
    let out = std::process::Command::new("exiv2").arg("-px").arg(path).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parts 1 to 3.
#[test]
fn photo_info_tags_write_back_and_a_rebuild_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let (lisbon, private, originals) = photos(home.path());

    // 1. A fresh library with both folders.
    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    assert!(c.photos().is_empty(), "a fresh library");
    for folder in [&lisbon, &private] {
        c.ok("engine.execute", json!({ "command": "folders.add", "params": { "path": folder } }));
    }
    c.settle_jobs();
    let photos = by_name(&mut c);
    assert_eq!(photos.len(), TURNS.len() + 2, "{photos:?}");
    let id = |name: &str| photos[name]["id"].clone();
    let (lisbon_id, private_id) = (photos["IMG_0001.jpg"]["folder"].clone(), photos["IMG_0101.jpg"]["folder"].clone());
    for (name, _) in TURNS {
        let i = c.inspect(&id(name));
        assert_eq!(
            (i["displayed"]["width"].as_u64(), i["displayed"]["height"].as_u64()),
            (Some(u64::from(W)), Some(u64::from(H))),
            "{name}: upright"
        );
    }

    // Rotations and flips.
    let turn = |c: &mut Client, cmd: &str, name: &str, times: usize| {
        for _ in 0..times {
            c.run(cmd, json!({ "ids": [id(name)] }));
        }
    };
    turn(&mut c, "photo.rotate.cw", "IMG_0001.jpg", 1);
    turn(&mut c, "photo.rotate.cw", "IMG_0002.jpg", 1);
    turn(&mut c, "photo.rotate.cw", "IMG_0003.jpg", 2);
    turn(&mut c, "photo.rotate.ccw", "IMG_0005.jpg", 2);
    turn(&mut c, "edit.flipH", "IMG_0004.jpg", 1);
    turn(&mut c, "edit.flipV", "IMG_0007.jpg", 1);
    turn(&mut c, "photo.rotate.cw", "IMG_0007.jpg", 2);
    turn(&mut c, "photo.rotate.cw", "IMG_0006.jpg", 4);
    // One undo and redo through the app.
    turn(&mut c, "photo.rotate.cw", "IMG_0008.jpg", 1);
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["displayed"], json!({ "width": H, "height": W }));
    c.run("edit.undo", json!({}));
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["recipe"]["edits"], json!([]));
    c.run("edit.redo", json!({}));
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["recipe"]["edits"], json!(["rotate(90)"]));
    c.run("edit.undo", json!({}));
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["displayed"], json!({ "width": W, "height": H }));

    // The cells: shapes and pixels, before any caption is shown under them.
    c.run("select.none", json!({}));
    c.run("view.caption", json!({ "caption": "none" }));
    c.settle_jobs();
    c.park_pointer(900.0);
    let shot = live::screenshot(&mut c, home.path(), None, "m6-1400x900-cells", 1400, 900);
    let mut cells = BTreeMap::new();
    for (name, _) in TURNS {
        let cell = c.rect(&format!("thumb:{}", id(name)));
        let (w, h, rgb) = shot.region(shot.picture(cell));
        let v = variation(&rgb);
        assert!(v > 12.0, "{name}: its cell shows no picture (variation {v:.1})");
        cells.insert(name, (w, h, rgb));
    }
    for name in ["IMG_0001.jpg", "IMG_0002.jpg"] {
        assert!(cells[name].1 > cells[name].0, "{name}: a quarter turn shows a portrait picture ({} × {})", cells[name].0, cells[name].1);
    }
    for name in ["IMG_0003.jpg", "IMG_0004.jpg", "IMG_0005.jpg", "IMG_0006.jpg", "IMG_0007.jpg", "IMG_0008.jpg"] {
        assert!(cells[name].0 > cells[name].1, "{name}: a landscape picture ({} × {})", cells[name].0, cells[name].1);
    }
    let diff = |a: &str, b: &str| difference(&cells[a].2, &cells[b].2);
    let mut same = Vec::new();
    for (a, b, what) in [
        ("IMG_0001.jpg", "IMG_0002.jpg", "a quarter turn clockwise (EXIF 1 and 2)"),
        ("IMG_0003.jpg", "IMG_0005.jpg", "180° as two clockwise and as two counterclockwise turns (EXIF 3 and 5)"),
        ("IMG_0004.jpg", "IMG_0007.jpg", "a left–right flip, and a top–bottom flip turned 180° (EXIF 4 and 7)"),
        ("IMG_0006.jpg", "IMG_0008.jpg", "four quarter turns and none (EXIF 6 and 8)"),
    ] {
        let d = diff(a, b);
        assert!(d < 4.0, "{what}: {a} and {b} differ by {d:.2}");
        same.push(format!("{what}: {d:.2}"));
    }
    for (a, b) in [("IMG_0003.jpg", "IMG_0008.jpg"), ("IMG_0004.jpg", "IMG_0008.jpg"), ("IMG_0004.jpg", "IMG_0003.jpg")] {
        let d = diff(a, b);
        assert!(d > 8.0, "different turns look different: {a} and {b} differ by only {d:.2}");
    }
    println!("rotated cells (mean difference per channel, same net turn):\n  {}", same.join("\n  "));

    // Hide a photo and a folder.
    c.run("photo.hide", json!({ "ids": [id("IMG_0009.jpg")] }));
    c.run("folder.hide", json!({ "section": format!("folder:{private_id}") }));
    assert!(!c.has(&format!("thumb:{}", id("IMG_0009.jpg"))), "a hidden photo has no cell");
    assert!(!c.has(&format!("tree:folder:{private_id}")) && !c.has("tree:group:hidden"), "a hidden folder leaves the tree");
    assert!(!c.has(&format!("thumb:{}", id("IMG_0101.jpg"))), "nor any cell of the hidden folder");

    // Captions, tags, dates, folder info.
    c.run("photo.caption.set", json!({ "ids": [id("IMG_0001.jpg")], "caption": format!("  {CAPTION_1} ") }));
    c.run("photo.caption.set", json!({ "ids": [id("IMG_0003.jpg")], "caption": CAPTION_3 }));
    c.run("photo.tags.add", json!({ "ids": [id("IMG_0001.jpg"), id("IMG_0002.jpg"), id("IMG_0003.jpg")], "tags": "lisbon, tram" }));
    c.run("photo.tags.add", json!({ "ids": [id("IMG_0004.jpg")], "tags": ["beach"] }));
    let partial = c.ok("engine.execute", json!({ "command": "tags.list", "params": { "ids": [id("IMG_0001.jpg"), id("IMG_0004.jpg")] } }));
    let labels: Vec<&str> = partial["tags"].as_array().unwrap().iter().filter_map(|t| t["label"].as_str()).collect();
    assert_eq!(labels.len(), 3, "{partial}");
    assert!(labels.contains(&"lisbon (1 of 2)") && labels.contains(&"beach (1 of 2)"), "{partial}");
    // Quick tags: slot 2 toggles a tag on and off, one undo step each.
    c.run("tags.quick.set", json!({ "slots": ["lisbon", "harbour"], "useRecent": false }));
    c.run("tags.quick.apply", json!({ "n": 2, "ids": [id("IMG_0008.jpg")] }));
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["tags"], json!(["harbour"]));
    c.run("tags.quick.2", json!({ "ids": [id("IMG_0008.jpg")] }));
    assert_eq!(c.inspect(&id("IMG_0008.jpg"))["tags"], json!([]));
    c.run("photo.dates.set", json!({ "ids": [id("IMG_0002.jpg")], "set": "2024-05-04T10:30:00" }));
    c.run("photo.dates.set", json!({ "ids": [id("IMG_0003.jpg")], "shift": 5400 }));
    c.run("folder.info.set", json!({ "id": lisbon_id, "description": "Trams and viewpoints", "place": "Lisbon", "date": "2024-05-01" }));
    let info = folder_info(&mut c, &lisbon_id);
    assert_eq!(info, json!({ "description": "Trams and viewpoints", "date": "2024-05-01", "place": "Lisbon" }));
    check_photos(&mut c, "after the commands");

    // The contract of every M6 command, and its dialogs.
    contract(&mut c, &id("IMG_0001.jpg"), &lisbon_id);
    dialogs(&mut c, &id("IMG_0001.jpg"), &lisbon_id);

    // 1400 × 900 with captions under the cells; 900 × 600 with View → Hidden Pictures.
    c.run("view.caption", json!({ "caption": "caption" }));
    c.run("select.none", json!({}));
    c.run("scroll.top", json!({}));
    c.settle_jobs();
    c.park_pointer(900.0);
    let large = live::screenshot(&mut c, home.path(), Some("m6-1400x900"), "m6-1400x900", 1400, 900);
    let header = c.widgets(&format!("section:folder:{lisbon_id}/description"));
    assert!(header.iter().any(|w| w["label"] == "Trams and viewpoints"), "the header shows the description: {header:?}");
    c.run("view.hiddenPictures", json!({ "on": true }));
    c.ok("ui.resize", json!({ "width": 900, "height": 600 }));
    c.settle();
    c.run("scroll.bottom", json!({}));
    c.settle_jobs();
    assert!(c.has(&format!("tree:folder:{private_id}")) && c.has("tree:group:hidden"), "Hidden Folders, with Hidden Pictures");
    c.park_pointer(600.0);
    let small = live::screenshot(&mut c, home.path(), Some("m6-900x600"), "m6-900x600", 900, 600);
    for name in ["IMG_0009.jpg", "IMG_0101.jpg", "IMG_0102.jpg"] {
        assert!(c.has(&format!("thumb:{}", id(name))), "{name}: hidden photos and hidden folders' photos show with Hidden Pictures");
    }
    c.run("view.hiddenPictures", json!({ "on": false }));
    c.ok("ui.resize", json!({ "width": 1400, "height": 900 }));
    c.run("scroll.top", json!({}));
    c.settle();

    // 2. The write-back: the files hold the captions, tags and dates; the pixels are untouched.
    let t2 = Instant::now();
    let f1 = written(&mut c, &lisbon, "IMG_0001.jpg", |f| f.caption.as_deref() == Some(CAPTION_1) && f.tags.len() == 2);
    let mut tags1 = f1.tags.clone();
    tags1.sort();
    assert_eq!(tags1, ["lisbon", "tram"]);
    let f2 = written(&mut c, &lisbon, "IMG_0002.jpg", |f| {
        f.date_taken.as_ref().is_some_and(|d| d.date_time().to_string().starts_with("2024-05-04T10:30:00"))
    });
    assert_eq!(f2.tags.len(), 2);
    written(&mut c, &lisbon, "IMG_0004.jpg", |f| f.tags == ["beach"]);
    for name in ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0004.jpg"] {
        let now = std::fs::read(lisbon.join(name)).unwrap();
        assert_ne!(now, originals[name], "{name}: written");
        assert!(pixels(&now) == pixels(&originals[name]), "{name}: the pixels are untouched");
    }
    for name in ["IMG_0005.jpg", "IMG_0006.jpg", "IMG_0007.jpg", "IMG_0008.jpg"] {
        assert_eq!(std::fs::read(lisbon.join(name)).unwrap(), originals[name], "{name}: rotations and flips never touch the file");
    }
    let metadata = {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let m = c.ok("engine.execute", json!({ "command": "photo.metadata", "params": { "id": id("IMG_0001.jpg") } }));
            if m["pending"] != true {
                break m;
            }
            assert!(Instant::now() < end, "photo.metadata stayed pending: {m}");
            c.ok("ui.wait", json!({ "ms": 100 }));
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let file = &metadata["file"];
    assert_eq!(file["caption"], CAPTION_1, "photo.metadata reads the file: {metadata}");
    match exiv2_xmp(&lisbon.join("IMG_0001.jpg")) {
        Some(xmp) => {
            assert!(xmp.contains("Xmp.dc.subject") && xmp.contains("lisbon") && xmp.contains("tram"), "exiv2 sees the tags:\n{xmp}");
            assert!(xmp.contains("Xmp.dc.description") && xmp.contains("Graça"), "exiv2 sees the caption:\n{xmp}");
            let lines: Vec<&str> = xmp.lines().filter(|l| l.contains("dc.subject") || l.contains("dc.description")).collect();
            println!("exiv2 -px IMG_0001.jpg (another application):\n  {}", lines.join("\n  "));
        }
        None => println!("exiv2: not installed, so the files were read by dalica-meta only (m6.md checks another application by hand)"),
    }
    println!(
        "write-back: captions, tags and dates in the JPEGs within {:.1} s, pixels untouched; IMG_0001 {:?} {:?}",
        secs(t2),
        f1.caption.as_deref().unwrap_or_default(),
        tags1
    );
    let before_rebuild = secs(t);
    live::quit(app, c);

    // 3. The index deleted, the app started again, the library rebuilt from the sidecars.
    let t3 = Instant::now();
    let library = home.path().join("library");
    let mut deleted = Vec::new();
    for e in std::fs::read_dir(&library).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("index") {
            std::fs::remove_file(e.path()).unwrap();
            deleted.push(name);
        }
    }
    assert!(!deleted.is_empty(), "the library has an index");
    let (app, mut c) = live::start(&exe, home.path());
    c.ok("engine.execute", json!({ "command": "library.rebuild", "params": { "confirm": true } }));
    c.settle_jobs();
    let rebuilt = by_name(&mut c);
    assert_eq!(rebuilt.len(), TURNS.len() + 2, "every photo is back: {:?}", rebuilt.keys());
    check_photos(&mut c, "after a rebuild");
    let lisbon_id = rebuilt["IMG_0001.jpg"]["folder"].clone();
    let private_id = rebuilt["IMG_0101.jpg"]["folder"].clone();
    assert_eq!(folder_info(&mut c, &lisbon_id), info, "the folder's description, date and place after a rebuild");
    assert!(!c.has(&format!("tree:folder:{private_id}")), "the folder is still hidden after a rebuild");
    // The same view as the screenshot before (the thumbnail caption mode is view state, not the
    // library's).
    c.run("view.caption", json!({ "caption": "caption" }));
    c.run("select.none", json!({}));
    c.run("scroll.top", json!({}));
    c.settle_jobs();
    c.park_pointer(900.0);
    let again = live::screenshot(&mut c, home.path(), Some("m6-1400x900"), "m6-1400x900-rebuilt", 1400, 900);
    let d = difference(&again.rgba, &large.rgba);
    println!("rebuilt: index files {deleted:?} deleted; every field back; the window differs from before by {d:.3} per channel");
    live::quit(app, c);

    println!(
        "live app: build {build:.1} s, part 1 and 2 {before_rebuild:.1} s, the rebuild {:.1} s; screenshots:\n  {}\n  {}\n  {}\n  {}",
        secs(t3),
        shot.path.display(),
        large.path.display(),
        small.path.display(),
        again.path.display()
    );
    println!("M6 live app: passed in {:.1} s", secs(started));
}

/// Parts 4 to 6.
#[test]
fn the_crate_suites_panic_hunt_the_metadata_corpus_and_the_release_dry_run() {
    let started = Instant::now();
    let in_ci = dalica_acceptance::suites_ran();
    if !in_ci {
        checks::crate_suites();
    }
    let suites = secs(started);
    let t = Instant::now();
    checks::panic_hunt();
    let hunt = secs(t);
    let t = Instant::now();
    checks::corpus(in_ci);
    let corpus = secs(t);
    checks::release_dry_run(in_ci);
    println!("suites {suites:.1} s, panic_hunt {hunt:.1} s, corpus {corpus:.1} s; M6 checks: passed in {:.1} s", secs(started));
}
