//! W10, Import with duplicates (`mcp.md` §9; `organizing.md` §3): a card-like folder (`DCIM/100CANON`,
//! shots on two days) listed and imported through the `cmd_import_*` tools into one folder per
//! capture date under a write root, every copy holding its source's bytes; listed and imported
//! again, every photo is a duplicate: nothing is copied and the import reports them all as skipped
//! duplicates. Removing the copies from the card is never run over MCP.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_core::DateTime;
use dalica_engine::automation::{Policy, plain};
use dalica_mcp::{Headless, Source};
use dalica_scenes::{Exif, Scene};
use serde_json::{Value, json};

use crate::common::{Client, initialize};
use crate::harness::bytes;

/// Long enough for any machine; a listing that ends never waits for it.
const PATIENCE: Duration = Duration::from_secs(120);
/// The card's shots: six, three a day on 2026-06-04 and 2026-06-05, a minute apart.
const SHOTS: u64 = 6;
const DAYS: [&str; 2] = ["2026-06-04", "2026-06-05"];

/// A procedural JPEG of scene `seed` taken `secs` after 2026-06-04T08:00:00.
fn shot(seed: u64, secs: i64) -> Vec<u8> {
    let at = DateTime::from_unix_secs(1_780_560_000 + secs).unwrap();
    let exif = Exif { date_time_original: Some(at), ..Exif::default() };
    dalica_scenes::jpeg(&Scene::new(seed, 96, 64), 85, Some(&exif)).unwrap()
}

/// Calls tool `name` and returns its `structuredContent`, failing on a tool error.
fn tool(c: &mut Client, next: &mut u64, name: &str, args: Value) -> Value {
    *next += 1;
    let r = c.call(*next, name, args.clone());
    assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
    r["structuredContent"].clone()
}

/// Opens the folder `dcim` in the Import room and waits for its listing to end; its last answer.
fn list(c: &mut Client, next: &mut u64, dcim: &Path) -> Value {
    let opened = tool(c, next, "cmd_import_openSource", json!({ "path": dcim }));
    assert_eq!(opened["source"]["path"], json!(dcim), "{opened}");
    let start = Instant::now();
    loop {
        let v = tool(c, next, "cmd_import_list", json!({}));
        if v["state"] != "loading" {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "the listing never ended: {v}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn w10_import_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    // Resolved and plain, as the paths the engine answers with.
    let root: PathBuf = plain(&std::fs::canonicalize(dir.path()).unwrap());
    let dcim = root.join("EOS_DIGITAL").join("DCIM");
    let shots: Vec<(String, Vec<u8>)> =
        (0..SHOTS).map(|i| (format!("IMG_{:04}.JPG", i + 1), shot(800 + i, ((i / 3) * 86_400 + (i % 3) * 60) as i64))).collect();
    std::fs::create_dir_all(dcim.join("100CANON")).unwrap();
    for (name, b) in &shots {
        std::fs::write(dcim.join("100CANON").join(name), b).unwrap();
    }
    let pictures = root.join("Pictures");
    std::fs::create_dir_all(&pictures).unwrap();

    // As `dalica-cli mcp --library DIR --automation-read-root ROOT --automation-write-root PICTURES`.
    let (library, read, write) = (root.join("library"), root.clone(), pictures.clone());
    let mut c = Client::start(move || {
        let mut policy = Policy::default();
        policy.add_read(read);
        policy.add_write(write);
        Box::new(Headless::open_with(Source::Library(library), &[], policy, false).unwrap())
    });
    initialize(&mut c);
    let mut next = 100;

    // The first import: every shot is new, one folder per capture date.
    let listed = list(&mut c, &mut next, &dcim);
    assert_eq!((listed["state"].clone(), listed["total"].clone(), listed["duplicates"].clone()), (json!("done"), json!(SHOTS), json!(0)), "{listed}");
    assert_eq!(listed["toImport"], json!(SHOTS), "{listed}");
    let run = json!({ "destination": pictures, "folderName": { "dateTaken": true }, "after": "leave" });
    tool(&mut c, &mut next, "cmd_import_run", run.clone());
    let status = tool(&mut c, &mut next, "cmd_import_status", json!({}));
    let days: Vec<PathBuf> = DAYS.iter().map(|d| pictures.join(d)).collect();
    assert_eq!((status["state"].clone(), status["imported"].clone(), status["failed"].clone()), (json!("done"), json!(SHOTS), json!([])), "{status}");
    assert_eq!(status["folders"], json!(days), "one folder per capture date, named as configured: {status}");
    for (i, (name, b)) in shots.iter().enumerate() {
        let copy = days[i / 3].join(name);
        assert_eq!(&bytes(&copy), b, "{} holds its source's bytes", copy.display());
    }
    let found = tool(&mut c, &mut next, "query_photos", json!({ "limit": 100 }));
    assert_eq!(found["total"], json!(SHOTS), "the copies are library photos: {found}");

    // The second import of the same card: every shot is a duplicate, nothing is copied.
    let again = list(&mut c, &mut next, &dcim);
    assert_eq!((again["duplicates"].clone(), again["toImport"].clone()), (json!(SHOTS), json!(0)), "{again}");
    let candidates = again["candidates"].as_array().unwrap();
    assert!(candidates.iter().all(|k| k["duplicate"]["photo"].is_u64() && k["willImport"] == false), "each names its library photo: {again}");
    tool(&mut c, &mut next, "cmd_import_run", run);
    let status = tool(&mut c, &mut next, "cmd_import_status", json!({}));
    assert_eq!((status["imported"].clone(), status["skipped"]["duplicates"].clone()), (json!(0), json!(SHOTS)), "{status}");
    for day in &days {
        assert_eq!(
            std::fs::read_dir(day).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "JPG")).count(),
            3,
            "nothing copied again into {}",
            day.display()
        );
    }
    let found = tool(&mut c, &mut next, "query_photos", json!({ "limit": 100 }));
    assert_eq!(found["total"], json!(SHOTS), "no photo was added: {found}");

    // Removing the copies from the card never runs over MCP, whatever was confirmed.
    next += 1;
    let removed = c.call(next, "cmd_import_removeFromSource", json!({ "confirm": true }));
    assert_eq!(removed["isError"], json!(true), "{removed}");
    assert!(shots.iter().all(|(name, _)| dcim.join("100CANON").join(name).exists()), "the card keeps every shot");
    c.close();
}
