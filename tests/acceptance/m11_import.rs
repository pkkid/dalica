//! M11's *Rooms and import* in the live app (`m11.rs` part 1): `dalica --headless` whose device
//! list is a temporary mount folder (`DALICA_DEVICE_ROOTS`) holding one simulated camera card,
//! `EOS_DIGITAL` with a `DCIM/100CANON` roll of eight `scenes` JPEGs shot on two days, and with
//! automation writing into `home/Pictures`:
//!
//! - Ctrl+M opens the Import room beside the Library (the room tabs show); the card is chosen
//!   under "Import from:" and its eight pictures are listed in three capture groups (split at gaps
//!   over 2 hours) with their thumbnails; the room matches
//!   `fingerprints/m11-1400x900-import-room.json`;
//! - Folder name ▸ Date taken, After copying ▸ Delete copied files from the card, Import All: the
//!   pictures land in `Pictures/2026-06-04` and `Pictures/2026-06-05`, each copy byte for byte its
//!   source with the source's modification time (`import.status`: every one imported, none failed),
//!   and a file planted beforehand under one of their names is left as it was while the copy takes
//!   `-001`;
//! - only once the copying is over and verified, "Remove the imported files from the card?" is
//!   asked; over the control channel it is refused on every route (`import.removeFromSource` with
//!   `confirm`, `ui.dialog.confirm`, a click), and Keep Files leaves the card whole;
//! - the card imported a second time: every picture is listed as a duplicate of its library copy,
//!   and the import copies nothing and reports the eight as skipped duplicates.
//!
//! The person's own click on Remove Imported Files (no control request running) is the UI crate's
//! (`crates/ui/tests/import_run_ui.rs`, run by `m11_checks.rs`): the control channel can never press
//! it, which is what this part proves.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

use crate::app::{self, click, dialog, idle, key, quiet, resize, secs};
use crate::live::{self, Client, Launch};

const LARGE: (u32, u32) = (1400, 900);
/// The card's pictures: name, scene seed, when taken.
const CARD: [(&str, u64, &str); 8] = [
    ("IMG_0001.JPG", 1101, "2026-06-04T08:00:00"),
    ("IMG_0002.JPG", 1102, "2026-06-04T08:05:00"),
    ("IMG_0003.JPG", 1103, "2026-06-04T08:10:00"),
    ("IMG_0004.JPG", 1104, "2026-06-04T13:00:00"),
    ("IMG_0005.JPG", 1105, "2026-06-04T13:20:00"),
    ("IMG_0006.JPG", 1106, "2026-06-05T09:30:00"),
    ("IMG_0007.JPG", 1107, "2026-06-05T09:31:00"),
    ("IMG_0008.JPG", 1108, "2026-06-05T09:32:00"),
];
const DAYS: [&str; 2] = ["2026-06-04", "2026-06-05"];
/// The name a file planted in the first day's folder takes before the import.
const CLASH: &str = "IMG_0002.JPG";
const CLASH_COPY: &str = "IMG_0002-001.JPG";
const PATIENCE: Duration = Duration::from_secs(120);

fn roll(home: &Path) -> PathBuf {
    home.join("mounts").join("EOS_DIGITAL").join("DCIM").join("100CANON")
}

fn day_of(taken: &str) -> &str {
    &taken[..10]
}

/// The card's files by name: bytes and modification time.
fn card_files(home: &Path) -> Vec<(String, Vec<u8>, SystemTime)> {
    let mut v: Vec<(String, Vec<u8>, SystemTime)> = std::fs::read_dir(roll(home))
        .unwrap()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap(), e.metadata().unwrap().modified().unwrap()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// The listing once it has ended with every thumbnail in memory.
fn listed(c: &mut Client, what: &str) -> Value {
    let start = Instant::now();
    loop {
        let l = c.exec("import.list", json!({}));
        let thumbs = l["candidates"].as_array().is_some_and(|k| !k.is_empty() && k.iter().all(|k| k["thumbnail"] == true));
        if l["state"] == "done" && thumbs {
            return l;
        }
        assert!(start.elapsed() < PATIENCE, "{what}: the listing never ended with its thumbnails: {l}");
        c.ok("ui.wait", json!({ "ms": 50 }));
    }
}

/// Opens the card under "Import from:" (the room's source menu), waiting for the device list to
/// find it; the listing once ended.
fn open_card(c: &mut Client, what: &str) -> Value {
    let start = Instant::now();
    loop {
        let s = c.exec("import.sources", json!({}));
        if s["sources"].as_array().is_some_and(|s| s.iter().any(|s| s["kind"] == "card")) {
            break;
        }
        assert!(start.elapsed() < PATIENCE, "{what}: the card never showed in import.sources: {s}");
        c.ok("ui.wait", json!({ "ms": 100 }));
    }
    idle(c);
    click(c, "choice:import.source");
    click(c, "choice:import.source/0");
    listed(c, what)
}

/// The import of the last `import.run` once it has ended.
fn finished(c: &mut Client) -> Value {
    let start = Instant::now();
    loop {
        let s = c.exec("import.status", json!({}));
        if s["state"] != "copying" && !s["state"].is_null() {
            return s;
        }
        assert!(start.elapsed() < PATIENCE, "the import never ended: {s}");
        c.ok("ui.wait", json!({ "ms": 50 }));
    }
}

pub fn import(exe: &Path, home: &Path) {
    let started = Instant::now();
    for (i, (name, seed, taken)) in CARD.iter().enumerate() {
        app::put(&roll(home).join(name), &app::jpeg(*seed, (480, 320), "Canon", "EOS R6", taken), i as u64);
    }
    let pictures = home.join("Pictures");
    let first_day = pictures.join(DAYS[0]);
    let planted = app::jpeg(1999, (200, 150), "Other", "Camera", "2025-01-01T00:00:00");
    app::put(&first_day.join(CLASH), &planted, 99);
    let card_before = card_files(home);

    // Writing into Pictures, and into the temporary folder (screenshots), which a write root given
    // replaces as the default.
    let launch = Launch::default()
        .arg("--automation-write-root")
        .arg(&pictures)
        .arg("--automation-write-root")
        .arg(home.join("tmp"))
        .env("DALICA_DEVICE_ROOTS", home.join("mounts"));
    let (a, mut cl) = live::start_with(exe, home, launch);
    let c = &mut cl;
    resize(c, LARGE);

    // Ctrl+M: the Import room beside the Library; the card under "Import from:".
    key(c, "Ctrl+M");
    let st = c.state();
    assert_eq!((st["room"].clone(), st["rooms"].clone()), (json!("import"), json!(["library", "import"])), "Ctrl+M opens the Import room");
    assert!(app::widget(c, "tab:room/library").is_some() && app::widget(c, "tab:room/import/close").is_some(), "the room tabs show");
    let l = open_card(c, "the first listing");
    assert_eq!((l["total"].clone(), l["duplicates"].clone(), l["toImport"].clone()), (json!(8), json!(0), json!(8)), "{l}");
    assert_eq!(l["source"]["kind"], "card", "{l}");
    let groups: Vec<u64> = l["groups"].as_array().unwrap().iter().map(|g| g["count"].as_u64().unwrap()).collect();
    assert_eq!(groups, [3, 2, 3], "groups split where capture times are more than 2 hours apart: {l}");
    quiet(c, f64::from(LARGE.1));
    let masks = app::moving_parts(c);
    app::masked_shot(c, home, "m11-1400x900-import-room", LARGE, &masks);
    println!("  Ctrl+M: the Import room and its tabs; the card's 8 pictures listed in 3 groups (matches its fingerprint)");

    // Date taken, Delete copied files from the card, Import All.
    c.ok("ui.set", json!({ "import": { "destination": { "place": pictures, "folderName": null, "after": "leave" } } }));
    idle(c);
    click(c, "choice:import.folderName");
    click(c, "choice:import.folderName/dateTaken");
    click(c, "choice:import.after");
    click(c, "choice:import.after/delete");
    let draft = c.state()["import"]["destination"].clone();
    assert_eq!(draft, json!({ "place": pictures, "folderName": { "dateTaken": true }, "after": "delete" }), "the destination strip's draft");
    c.ok("ui.clickWidget", json!({ "id": "button:import.importAll" }));
    let start = Instant::now();
    while dialog(c).as_deref() != Some("removeFromCard") {
        assert!(start.elapsed() < PATIENCE, "the question never came: {}", c.state());
        // Never before the copying is over and verified.
        let s = c.exec("import.status", json!({}));
        assert!(s["state"] == "copying" || s["state"] == "done", "{s}");
        c.ok("ui.wait", json!({ "ms": 50 }));
    }
    let status = finished(c);
    assert_eq!(status["state"], "done", "the question comes once the import is done: {status}");
    assert_eq!((status["imported"].clone(), status["failed"].clone()), (json!(8), json!([])), "every copy written and verified: {status}");
    let days: Vec<PathBuf> = DAYS.iter().map(|d| pictures.join(d)).collect();
    assert_eq!(status["folders"], json!(days), "one folder per capture date: {status}");
    for (name, bytes, modified) in &card_before {
        let taken = CARD.iter().find(|(n, _, _)| n == name).map(|(_, _, t)| *t).unwrap();
        let copy = pictures.join(day_of(taken)).join(if name == CLASH { CLASH_COPY } else { name.as_str() });
        assert!(std::fs::read(&copy).unwrap() == *bytes, "{}: byte for byte its source", copy.display());
        assert_eq!(std::fs::metadata(&copy).unwrap().modified().unwrap(), *modified, "{}: the source's time", copy.display());
    }
    assert!(std::fs::read(first_day.join(CLASH)).unwrap() == planted, "the planted {CLASH} is left as it was");
    println!(
        "  Import All into Date taken folders: 8 copies verified ({} bytes), {CLASH} planted beforehand kept, the copy named {CLASH_COPY}",
        status["bytes"]
    );

    // The question, refused on every automated route; Keep Files.
    let e = c.refused("import.removeFromSource", json!({ "confirm": true }));
    assert_eq!(e["code"], "external_denied", "{e}");
    let e2 = c.fails("ui.dialog.confirm", json!({ "button": "dialog.remove" }));
    assert_eq!(e2["code"], "external_denied", "{e2}");
    let e3 = c.fails("ui.clickWidget", json!({ "id": "button:dialog.remove" }));
    assert_eq!(e3["code"], "external_denied", "{e3}");
    idle(c);
    assert_eq!(dialog(c).as_deref(), Some("removeFromCard"), "the question is still asked");
    assert!(card_files(home) == card_before, "nothing left the card");
    click(c, "button:dialog.cancel");
    idle(c);
    assert_eq!(dialog(c), None);
    assert!(card_files(home) == card_before, "Keep Files keeps them");
    assert!(c.exec("import.status", json!({}))["removal"].is_null(), "no removal ran");
    println!("  the removal question once verified: refused over the control channel ({}), Keep Files", e["error"].as_str().unwrap_or_default());

    // The same card again: every picture a duplicate of its copy, nothing copied.
    idle(c);
    if c.state()["room"] != "import" {
        key(c, "Ctrl+M");
    }
    let again = open_card(c, "the second listing");
    assert_eq!((again["duplicates"].clone(), again["toImport"].clone()), (json!(8), json!(0)), "{again}");
    let candidates = again["candidates"].as_array().unwrap();
    assert!(candidates.iter().all(|k| k["duplicate"]["photo"].is_u64() && k["willImport"] == false), "each names its library photo: {again}");
    let before: Vec<Vec<String>> = days.iter().map(|d| app::files(d)).collect();
    c.exec("import.run", json!({ "destination": pictures, "folderName": { "dateTaken": true }, "after": "leave" }));
    let status = finished(c);
    assert_eq!((status["imported"].clone(), status["skipped"]["duplicates"].clone()), (json!(0), json!(8)), "{status}");
    idle(c);
    let after: Vec<Vec<String>> = days.iter().map(|d| app::files(d)).collect();
    assert_eq!(after, before, "nothing copied again");
    assert!(card_files(home) == card_before, "the card is whole");
    println!("  the card again: 8 duplicates listed, 0 copied, 8 reported as skipped duplicates");
    live::quit(a, cl);
    println!("  M11 import in {:.1} s", secs(started));
}
