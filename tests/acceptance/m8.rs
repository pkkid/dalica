//! M8 "Save and export" (`docs/roadmap.md` → M8 → the *Done when* of Save to disk and of Export;
//! run by `cargo xtask accept M8`).
//!
//! 1. **Save in the live app** (`m6_live.rs`'s helpers): the real `dalica` binary in `--headless`
//!    mode on a fresh library, driven over its control channel as an agent would, on a folder of
//!    six `scenes` JPEGs (640 × 480, so a quarter turn is lossless).
//!    - `data-model.md` §10.9 the way `mcp.md`'s W03 runs it, over `engine.execute`: an edit,
//!      `photo.save` refused without `confirm`, then with it the original kept byte for byte in
//!      `.dalica-originals/` and the record done; `photo.undoSave` puts the exact bytes back with
//!      the edit unsaved; Save again; `photo.revert` returns the exact original bytes, the backup and
//!      the record gone.
//!    - The same in the app's widgets: the section header's "2 edited" and Save (the window
//!      matched against `fingerprints/m8-*-header.json` at 1400 × 900 and 900 × 600), the
//!      first-time explanation and its Save; a rotate-only photo saved losslessly (its own
//!      quantisation tables, the decoded pixels the turned original's within 1 level); the edit
//!      panel's Undo reading "Undo Save" and bringing the exact bytes back; Ctrl+S and the
//!      explanation's Enter; File ▸ Revert… and the Revert question's Revert: the exact original
//!      bytes again.
//!    - Save a Copy (`<stem>-001.jpg` beside it, the photo and its edits untouched) and Save As PNG
//!      through File ▸ Save As… and the save picker, into a new folder that joins the library.
//!    - A Save killed at two steps (the `faults` feature, run straight through `create` while the
//!      app is closed, as a crash leaves the disk): before the commit (the original moved into the
//!      backup, nothing at the photo's name) and after it (before the record is marked done). The
//!      app starts again and repairs the folder: rolled back (the original at its name, byte for
//!      byte, the edit unsaved) and rolled forward (the edit in the file, the original kept, Undo
//!      Save giving its exact bytes back); no temporary file is left.
//! 2. **Export in the live app:** `export.run` of a folder at 320 pixels, Normal, numbered, with
//!    the location removed: every file decoded at its size, its quality within Normal's bounds
//!    (85 ± 2), its camera EXIF, date, caption and tags carried, no GPS in EXIF or XMP; the same
//!    export keeping the location keeps it. The Export dialog from the section header's Export
//!    (`fingerprints/m8-*-export.json`, the location field masked: it holds a temporary path),
//!    with a watermark: the files carry it in its box and are otherwise the agent's export. A
//!    long export from the dialog opens the progress window (`fingerprints/m8-1400x900-progress
//!    .json`, its changing line and bar masked), whose Cancel ends the job cancelled with only
//!    complete files written; the export folders are listed under Projects.
//! 3. **The crate suites, run as they are** (`m8_checks.rs`): the model, `fs`, the record, the
//!    encoders and the quality estimate, lossless transforms, carried metadata, Save in place,
//!    lossless and beside, a fault at every step of a Save and its repair, **the per-format suite of
//!    M8.23** (`save_formats`), the commands, the app's Save and Save As, export planning, the
//!    watermark, the export job, dialog and progress window, and **the 200-photo mixed export with
//!    its Cancel** (`export_mixed`, M8.24). Inside `cargo xtask ci` its test step already ran them,
//!    so they are skipped there (`dalica_acceptance::SUITES_RAN`). `panic_hunt -- --ignored` always
//!    runs.
//! 4. **The quality estimate's error** (JPEGs of known quality from two encoders) and **the
//!    `save_export` bench**: in a debug build its smoke pass checks the path (small saves, the
//!    200-photo tray at scale 1) and prints its times. With `DALICA_ACCEPT_RELEASE=1`, the budgets
//!    in release: Save of a 24 MP JPEG ≤ 1050 ms, its lossless rotation ≤ 815 ms, the 200-photo
//!    mixed export ≤ 24 s (medians).
//!
//! The manual checks (save one of your own photos and find the original, Undo Save, Revert, a
//! rotate-only save, Save As PNG, an export at 1600 with a watermark opened in another
//! application) are in `m8.md`.

#[path = "m8_checks.rs"]
mod checks;
#[allow(dead_code)]
#[path = "m6_live.rs"]
mod live;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dalica_core::Timestamp;
use dalica_create::save;
use dalica_edit::Recipe;
use dalica_fs::faults::{self, Fault, points};
use dalica_image::{Limits, Orientation, Rgba8};
use dalica_meta::{Changes, Place, WriteOptions};
use dalica_scenes::{Exif, Scene, fingerprint};
use live::{Client, Shot};
use serde_json::{Value, json};

/// What a failed byte comparison prints: sizes, never the bytes.
trait Shown {
    fn shown(&self) -> String;
}

impl Shown for [u8] {
    fn shown(&self) -> String {
        format!("{} bytes", self.len())
    }
}

impl Shown for Vec<u8> {
    fn shown(&self) -> String {
        self.as_slice().shown()
    }
}

impl<T: Shown + ?Sized> Shown for &T {
    fn shown(&self) -> String {
        (**self).shown()
    }
}

impl<T: Shown> Shown for Option<T> {
    fn shown(&self) -> String {
        self.as_ref().map_or_else(|| "no file".into(), Shown::shown)
    }
}

/// Two byte strings (or missing files) are the same; a failure prints their sizes.
macro_rules! same {
    ($a:expr, $b:expr) => { same!($a, $b, "the same bytes") };
    ($a:expr, $b:expr, $($m:tt)+) => {{
        let (a, b) = (&$a, &$b);
        assert!(a == b, "{}: the bytes differ ({} and {})", format!($($m)+), a.shown(), b.shown());
    }};
}

/// Two byte strings differ.
macro_rules! differ {
    ($a:expr, $b:expr) => { differ!($a, $b, "different bytes") };
    ($a:expr, $b:expr, $($m:tt)+) => {{
        let (a, b) = (&$a, &$b);
        assert!(a != b, "{}: the bytes are the same ({})", format!($($m)+), a.shown());
    }};
}

/// The photos of the `Lisbon` folder (Save): (name, seed), 640 × 480, taken a minute apart.
const LISBON: [(&str, u64); 6] =
    [("IMG_0001.jpg", 11), ("IMG_0002.jpg", 23), ("IMG_0003.jpg", 37), ("IMG_0004.jpg", 41), ("IMG_0005.jpg", 53), ("IMG_0006.jpg", 67)];
/// Saved, undone, saved and reverted through the app's widgets.
const BY_HAND: &str = "IMG_0001.jpg";
/// `mcp.md`'s W03 over `engine.execute`.
const BY_AGENT: &str = "IMG_0002.jpg";
/// Turned a quarter: saved losslessly.
const TURNED: &str = "IMG_0003.jpg";
/// Save a Copy and Save As PNG.
const COPIED: &str = "IMG_0004.jpg";
/// A Save killed before its commit, and one killed after it.
const ROLLED_BACK: &str = "IMG_0005.jpg";
const ROLLED_FORWARD: &str = "IMG_0006.jpg";
const SIZE: (u32, u32) = (640, 480);

const AUTOFIX: &str = "autofix()";
const TUNING: &str = "tuning(0.300,0.100,0.050,0.200,none)";
const TURN: &str = "rotate(90)";

/// The photos of the `Porto` folder (Export): (name, seed, has a place).
const PORTO: [(&str, u64, bool); 6] = [
    ("IMG_0001.jpg", 71, true),
    ("IMG_0002.jpg", 73, false),
    ("IMG_0003.jpg", 79, false),
    ("IMG_0004.jpg", 83, true),
    ("IMG_0005.jpg", 89, false),
    ("IMG_0006.jpg", 97, false),
];
/// The `Big` folder: copies of one 2400 × 1800 photo, exported whole at Maximum so the export
/// lasts long enough to be watched and cancelled.
const BIG: usize = 64;
const BIG_SIZE: (u32, u32) = (2400, 1800);
/// The export size and the watermark's text.
const LONG_SIDE: u32 = 320;
const WATERMARK: &str = "© Dalica M8";
/// Normal quality (`creating.md` §2) and how far the estimate may stray.
const NORMAL: u8 = 85;

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

/// The camera EXIF of every photo: photo `i` taken at 10:0i on June 1, 2024.
fn exif(i: usize) -> Exif {
    let taken: dalica_core::DateTime = format!("2024-06-01T10:{i:02}:00").parse().unwrap();
    Exif { orientation: 1, date_time_original: Some(taken), make: Some("Dalica".into()), model: Some("Scenes 1".into()), ..Exif::default() }
}

/// Writes `bytes` to `path` with a modification time fixed by `i`.
fn put(path: &Path, bytes: &[u8], i: usize) {
    std::fs::write(path, bytes).unwrap();
    let at = UNIX_EPOCH + Duration::from_secs(1_717_236_000 + 60 * i as u64);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
}

/// Writes the `Lisbon` folder into `home/Pictures`; the originals' bytes by name.
fn lisbon(home: &Path) -> (PathBuf, BTreeMap<&'static str, Vec<u8>>) {
    let folder = home.join("Pictures").join("Lisbon");
    std::fs::create_dir_all(&folder).unwrap();
    let mut originals = BTreeMap::new();
    for (i, (name, seed)) in LISBON.into_iter().enumerate() {
        let jpeg = dalica_scenes::jpeg(&Scene::new(seed, SIZE.0, SIZE.1), 90, Some(&exif(i))).unwrap();
        put(&folder.join(name), &jpeg, i);
        originals.insert(name, jpeg);
    }
    (folder, originals)
}

/// Writes the `Porto` folder (two photos with a place) and the `Big` folder into `home/Pictures`.
fn porto_and_big(home: &Path) -> (PathBuf, PathBuf) {
    let porto = home.join("Pictures").join("Porto");
    std::fs::create_dir_all(&porto).unwrap();
    for (i, (name, seed, located)) in PORTO.into_iter().enumerate() {
        let mut jpeg = dalica_scenes::jpeg(&Scene::new(seed, SIZE.0, SIZE.1), 92, Some(&exif(i))).unwrap();
        if located {
            let place = Place::new(41.14 + i as f64 / 100.0, -8.61);
            jpeg = dalica_meta::write_owned(&jpeg, &Changes { place: Some(place), ..Changes::default() }, &WriteOptions::default()).unwrap();
        }
        put(&porto.join(name), &jpeg, i);
    }
    let big = home.join("Pictures").join("Big");
    std::fs::create_dir_all(&big).unwrap();
    let jpeg = dalica_scenes::jpeg(&Scene::new(5, BIG_SIZE.0, BIG_SIZE.1), 95, Some(&exif(0))).unwrap();
    for i in 0..BIG {
        put(&big.join(format!("BIG_{:04}.jpg", i + 1)), &jpeg, i);
    }
    (porto, big)
}

// --- driving the app ------------------------------------------------------------------------------

/// Frames until nothing is pending, background work included, however long it takes in real time
/// (at most ten minutes): `ui.settle` with `jobs` waits for the scans and thumbnails, and every
/// job `jobs.list` shows unfinished (the save and export file jobs, the repair) is waited for with
/// `jobs.wait`, until a settle finds none.
fn idle(c: &mut Client) {
    let end = Instant::now() + Duration::from_secs(600);
    while Instant::now() < end {
        let settled = c.ok("ui.settle", json!({ "timeoutMs": 60000, "jobs": true }))["settled"] == true;
        let running: Vec<Value> =
            jobs(c).into_iter().filter(|j| j["state"] == "queued" || j["state"] == "running").map(|j| j["job"].clone()).collect();
        if settled && running.is_empty() {
            return;
        }
        for job in running {
            c.ok("jobs.wait", json!({ "job": job, "timeoutMs": 50000 }));
        }
    }
    panic!("the app did not settle within ten minutes");
}

/// `jobs.list`.
fn jobs(c: &mut Client) -> Vec<Value> {
    c.ok("jobs.list", json!({})).as_array().cloned().unwrap_or_default()
}

/// An engine or UI command that must succeed, then [`idle`]; its result.
fn run(c: &mut Client, command: &str, params: Value) -> Value {
    let r = c.ok("engine.execute", json!({ "command": command, "params": params }));
    idle(c);
    r
}

fn key(c: &mut Client, combo: &str) {
    let field = if combo.contains('+') { "combo" } else { "key" };
    c.ok("ui.key", json!({ field: combo }));
    idle(c);
}

fn click(c: &mut Client, id: &str) {
    c.ok("ui.clickWidget", json!({ "id": id }));
    idle(c);
}

/// The label a widget shows.
fn label(c: &mut Client, id: &str) -> String {
    c.widgets(id).into_iter().find(|w| w["id"] == id).and_then(|w| w["label"].as_str().map(str::to_string)).unwrap_or_default()
}

/// The open dialog's kind (`ui.state` → `dialogs`), if any.
fn dialog(c: &mut Client) -> Option<String> {
    let s = c.ok("ui.state", json!({}));
    s["dialogs"].as_array().and_then(|d| d.last()).and_then(|d| d["kind"].as_str().or_else(|| d.as_str())).map(str::to_string)
}

/// The job `job` once it has ended (`jobs.wait`, as often as it takes, at most ten minutes).
fn wait_job(c: &mut Client, job: &Value) -> Value {
    for _ in 0..12 {
        let entry = c.ok("jobs.wait", json!({ "job": job, "timeoutMs": 50000 }));
        if entry["pending"] != true {
            idle(c);
            return entry;
        }
    }
    panic!("job {job} did not end within ten minutes");
}

/// A file-job command that must succeed, run to its end; the job's final entry.
fn job(c: &mut Client, command: &str, params: Value) -> Value {
    let r = c.ok("engine.execute", json!({ "command": command, "params": params.clone() }));
    assert_eq!(r["pending"], true, "{command} {params} starts a job: {r}");
    let entry = wait_job(c, &r["job"]);
    assert_eq!(entry["state"], "done", "{command} {params}: {entry}");
    entry
}

/// The library's photo ids by name, in one folder (by its path's last component).
fn ids_in(c: &mut Client, folder: &str) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for p in c.photos() {
        let inspected = c.inspect(&p["id"]);
        let path = inspected["path"].as_str().map(PathBuf::from).unwrap_or_default();
        if path.parent().and_then(Path::file_name).is_some_and(|n| n == folder) {
            out.insert(p["name"].as_str().unwrap().to_string(), p["id"].clone());
        }
    }
    out
}

/// The section id (`folder:<FolderId>`) of the folder holding photo `id`.
fn section_of(c: &mut Client, id: &Value) -> String {
    let inspected = c.inspect(id);
    let folder = inspected["folder"].as_u64().unwrap_or_else(|| panic!("photo.inspect names the folder: {inspected}"));
    format!("folder:{folder}")
}

fn edits(c: &mut Client, id: &Value) -> Vec<String> {
    let h = c.ok("engine.execute", json!({ "command": "photo.history.state", "params": { "id": id } }));
    h["edits"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect()
}

fn apply(c: &mut Client, id: &Value, op: &str) {
    run(c, "photo.history.apply", json!({ "id": id, "op": op }));
}

// --- the disk -----------------------------------------------------------------------------------

fn bytes(folder: &Path, name: &str) -> Vec<u8> {
    std::fs::read(folder.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The original kept in `.dalica-originals/`, if any.
fn kept(folder: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(folder.join(".dalica-originals").join(name)).ok()
}

/// The originals record's entry for `name`: its state and recipe.
fn entry(folder: &Path, name: &str) -> Option<(String, Vec<String>)> {
    let text = std::fs::read(folder.join(".dalica-originals").join(".dalica.json")).ok()?;
    let record: Value = serde_json::from_slice(&text).unwrap();
    let e = record["originals"].get(name)?;
    let recipe = e["recipe"].as_array().map(|r| r.iter().map(|o| o.as_str().unwrap().to_string()).collect()).unwrap_or_default();
    Some((e["state"].as_str().unwrap().to_string(), recipe))
}

/// Temporary files left in `folder` or its `.dalica-originals/`.
fn temporaries(folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in [folder.to_path_buf(), folder.join(".dalica-originals")] {
        if let Ok(list) = std::fs::read_dir(&d) {
            out.extend(list.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".dalica-tmp-")));
        }
    }
    out
}

fn decode(bytes: &[u8]) -> Rgba8 {
    dalica_image::decode(bytes, &Limits::default()).unwrap()
}

/// The largest difference of one channel between two images of one size.
fn max_difference(a: &Rgba8, b: &Rgba8) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.data.iter().zip(&b.data).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// Pixels of `a` and `b` (one size) in `x0..x1 × y0..y1` whose colour differs by more than a JPEG's
/// noise.
fn changed(a: &Rgba8, b: &Rgba8, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
    let mut n = 0;
    for y in y0..y1.min(a.height) {
        for x in x0..x1.min(a.width) {
            let i = ((y * a.width + x) * 4) as usize;
            n += usize::from((0..3).any(|k| a.data[i + k].abs_diff(b.data[i + k]) > 24));
        }
    }
    n
}

/// The files of `dir`, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

// --- whole-window fingerprints with masked parts --------------------------------------------------

/// The rect of the widget `id`, if it is shown.
fn rect_of(c: &mut Client, id: &str) -> Option<[f64; 4]> {
    let w = c.widgets(id).into_iter().find(|w| w["id"] == id)?;
    Some([0, 1, 2, 3].map(|i| w["rect"][i].as_f64().unwrap_or(0.0)))
}

/// A whole-window screenshot (`live::screenshot`) whose `masks` (rects in points) are painted one
/// grey before it is matched against `fingerprints/<name>.json` (written first under
/// `DALICA_BLESS=1`): parts that hold a temporary path or follow a running job.
fn masked_shot(c: &mut Client, home: &Path, name: &str, (w, h): (u32, u32), masks: &[[f64; 4]]) -> Shot {
    let mut shot = live::screenshot(c, home, None, name, w, h);
    for r in masks {
        let (x0, y0) = (r[0].floor().max(0.0) as u32, r[1].floor().max(0.0) as u32);
        let (x1, y1) = (((r[0] + r[2]).ceil() as u32).min(w), ((r[1] + r[3]).ceil() as u32).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                shot.rgba[i..i + 4].copy_from_slice(&[128, 128, 128, 255]);
            }
        }
    }
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &shot.rgba)) + "\n").unwrap();
    }
    let masked = shot.path.with_file_name(format!("{name}-masked.png"));
    std::fs::write(&masked, fingerprint::png_rgba(w, h, &shot.rgba)).unwrap();
    let text = std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e} (bless with DALICA_BLESS=1)", stored.display()));
    let expected = fingerprint::from_json(&text).unwrap();
    if let Err(m) = fingerprint::check(w, h, &shot.rgba, &expected, 2.0) {
        panic!("{name}: {} (the screenshot is {}, masked {})", m.message, shot.path.display(), masked.display());
    }
    shot
}

/// Rests the pointer on the status line's left end and lets the frames run (without waiting for
/// jobs, which may be running on purpose).
fn park(c: &mut Client, h: f64) {
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": 2.0, "y": h - 106.0 }] }));
    c.ok("ui.settle", json!({ "timeoutMs": 5000 }));
}

/// Lets the toasts of the jobs before run out (headless, `ui.wait` advances the app's clock), so a
/// screenshot shows none whatever the jobs took in real time.
fn quiet(c: &mut Client, h: f64) {
    park(c, h);
    c.ok("ui.wait", json!({ "ms": 20000 }));
    idle(c);
}

// --- part 1: Save -------------------------------------------------------------------------------

/// `data-model.md` §10.9 as `mcp.md`'s W03 runs it, over `engine.execute`.
fn save_by_agent(c: &mut Client, folder: &Path, original: &[u8], id: &Value) {
    apply(c, id, TUNING);
    let refused = c.refused("photo.save", json!({ "ids": [id] }));
    assert_eq!(refused["code"], "needs_confirm", "Save asks first over automation: {refused}");
    same!(bytes(folder, BY_AGENT), original, "nothing is saved without confirm");

    let done = job(c, "photo.save", json!({ "ids": [id], "confirm": true }));
    assert_eq!(done["result"]["photos"], json!([id]), "{done}");
    differ!(bytes(folder, BY_AGENT), original, "Save writes the edit into the file");
    same!(kept(folder, BY_AGENT), Some(original.to_vec()), "the original is kept byte for byte");
    assert_eq!(entry(folder, BY_AGENT), Some(("done".into(), vec![TUNING.to_string()])), "the record is done");
    assert!(edits(c, id).is_empty(), "the edit is in the file");

    job(c, "photo.undoSave", json!({ "ids": [id] }));
    same!(bytes(folder, BY_AGENT), original, "Undo Save puts the exact bytes back");
    assert_eq!(edits(c, id), [TUNING], "with the edit unsaved");
    assert!(kept(folder, BY_AGENT).is_none() && entry(folder, BY_AGENT).is_none(), "no backup and no record");

    job(c, "photo.save", json!({ "ids": [id], "confirm": true }));
    differ!(bytes(folder, BY_AGENT), original);
    let refused = c.refused("photo.revert", json!({ "ids": [id] }));
    assert_eq!(refused["code"], "needs_confirm", "Revert asks first: {refused}");
    job(c, "photo.revert", json!({ "ids": [id], "confirm": true }));
    same!(bytes(folder, BY_AGENT), original, "Save → Undo Save → Save → Revert returns the exact original bytes");
    assert!(edits(c, id).is_empty(), "Revert drops the edits");
    assert!(kept(folder, BY_AGENT).is_none() && entry(folder, BY_AGENT).is_none(), "the backup and the record are gone");
    println!("W03 over engine.execute: Save (refused without confirm) → Undo Save → Save → Revert: the exact original bytes");
}

/// The rotate-only Save of [`TURNED`] was lossless: its DCT coefficients are the original's
/// turned (nothing requantised), and its decoded pixels the turned original's within the decoder's
/// rounding (`create`'s `save_lossless` suite: at most 3 levels after the colour conversion, a mean
/// under 0.05). Returns the largest difference.
fn lossless(folder: &Path, original: &[u8]) -> u8 {
    let saved = bytes(folder, TURNED);
    let header = dalica_image::read_header(&saved, &Limits::default()).unwrap();
    assert_eq!((header.width, header.height, header.orientation.get()), (SIZE.1, SIZE.0, 1), "turned, orientation 1");
    assert_eq!(dalica_image::jpeg_quality::quality(&saved), dalica_image::jpeg_quality::quality(original), "the original's quality");
    // Orientation 6: the stored pixels turned a quarter clockwise, as `rotate(90)` turns them.
    let quarter = Orientation::new(6).unwrap();
    let limits = Limits::default();
    let expected = dalica_image::jpeg_transform::read(original, &limits).unwrap().transformed(quarter).unwrap();
    assert!(dalica_image::jpeg_transform::read(&saved, &limits).unwrap() == expected, "the coefficients are the original's, turned");
    let (a, b) = (decode(&saved), dalica_image::orient::apply(decode(original), quarter).unwrap());
    let d = max_difference(&a, &b);
    let mean = a.data.iter().zip(&b.data).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / a.data.len() as f64;
    assert!(d <= 3 && mean < 0.05, "the saved pixels are the original's turned: at most {d} levels, mean {mean:.4}");
    d
}

/// Part 1b: §10.9 through the app's widgets, and the lossless rotation saved with it.
fn save_by_hand(c: &mut Client, home: &Path, folder: &Path, originals: &BTreeMap<&str, Vec<u8>>, ids: &BTreeMap<String, Value>) -> u8 {
    let (hand, turned) = (ids[BY_HAND].clone(), ids[TURNED].clone());
    let section = section_of(c, &hand);
    let save_edited = format!("section:{section}/saveEdited");
    assert!(!c.has(&save_edited), "no Save at the header's right without unsaved edits");
    apply(c, &hand, AUTOFIX);
    apply(c, &turned, TURN);
    assert!(c.has(&save_edited), "two edited photos: the header's Save");
    assert_eq!(label(c, &save_edited), "Save");
    run(c, "select.none", json!({}));
    quiet(c, 900.0);
    live::screenshot(c, home, Some("m8-1400x900-header"), "m8-1400x900-header", 1400, 900);
    c.ok("ui.resize", json!({ "width": 900, "height": 600 }));
    idle(c);
    park(c, 600.0);
    live::screenshot(c, home, Some("m8-900x600-header"), "m8-900x600-header", 900, 600);
    c.ok("ui.resize", json!({ "width": 1400, "height": 900 }));
    idle(c);

    // The header's Save: the explanation once, then both saved.
    click(c, &save_edited);
    assert_eq!(dialog(c).as_deref(), Some("saveExplain"), "the first Save explains");
    same!(bytes(folder, BY_HAND), originals[BY_HAND], "nothing saved before the answer");
    click(c, "button:dialog.save");
    assert_eq!(dialog(c), None);
    for name in [BY_HAND, TURNED] {
        differ!(bytes(folder, name), originals[name], "{name} saved");
        same!(kept(folder, name), Some(originals[name].clone()), "{name}: the original kept");
    }
    assert!(!c.has(&save_edited), "nothing left to save");
    let level = lossless(folder, &originals[TURNED]);
    assert_eq!(entry(folder, TURNED), Some(("done".into(), vec![TURN.to_string()])));

    // The edit panel's Undo reads "Undo Save" and brings the exact bytes back.
    run(c, "view.edit", json!({ "id": hand }));
    assert_eq!(label(c, "button:edit.undo"), "Undo Save");
    click(c, "button:edit.undo");
    same!(bytes(folder, BY_HAND), originals[BY_HAND], "Undo Save: the exact original bytes");
    assert_eq!(edits(c, &hand), [AUTOFIX], "the edit is back, unsaved");
    assert_eq!(label(c, "button:edit.undo"), "Undo Auto Fix");

    // Ctrl+S: the explanation again (nothing was ticked), Enter is its Save.
    key(c, "Ctrl+S");
    assert_eq!(dialog(c).as_deref(), Some("saveExplain"));
    key(c, "Enter");
    differ!(bytes(folder, BY_HAND), originals[BY_HAND], "saved again");
    assert!(edits(c, &hand).is_empty());

    // File ▸ Revert…: the question, its Revert.
    let r = c.ok("ui.menu.invoke", json!({ "id": "photo.revert" }));
    idle(c);
    assert_eq!((r["dialog"].as_str(), dialog(c).as_deref()), (Some("revert"), Some("revert")), "{r}");
    click(c, "button:dialog.revert");
    assert_eq!(dialog(c), None);
    same!(bytes(folder, BY_HAND), originals[BY_HAND], "Save → Undo Save → Save → Revert in the app: the exact original bytes");
    assert!(edits(c, &hand).is_empty());
    assert!(kept(folder, BY_HAND).is_none() && entry(folder, BY_HAND).is_none(), "no backup and no record");
    run(c, "view.library", json!({}));
    println!(
        "in the app: the header's \"2 edited\" and Save, the explanation, the lossless quarter turn (pixels within {level} levels), Undo Save, Ctrl+S, Revert: the exact original bytes"
    );
    level
}

/// Part 1c: Save a Copy and Save As PNG.
fn copies(c: &mut Client, home: &Path, folder: &Path, original: &[u8], id: &Value) {
    apply(c, id, AUTOFIX);
    run(c, "select.set", json!({ "ids": [id] }));
    c.ok("ui.menu.invoke", json!({ "id": "photo.saveCopy" }));
    idle(c);
    let copy = bytes(folder, "IMG_0004-001.jpg");
    same!(bytes(folder, COPIED), original, "Save a Copy leaves the photo untouched");
    assert_eq!(edits(c, id), [AUTOFIX], "and its edits");
    let edited = decode(&copy);
    assert_eq!((edited.width, edited.height), SIZE);
    assert!(max_difference(&edited, &decode(original)) > 8, "the copy holds the edit");

    // File ▸ Save As…: the save picker, answered with a PNG in a new folder.
    let r = c.ok("ui.menu.invoke", json!({ "id": "photo.saveAs" }));
    assert_eq!(r["dialog"], "filePicker", "{r}");
    let picker = c.ok("ui.inspect", json!({}))["filePicker"].clone();
    // The picker suggests a free name beside the photo: the copy took `-001`.
    assert_eq!((picker["command"].as_str(), picker["name"].as_str()), (Some("photo.saveTo"), Some("IMG_0004-002.jpg")), "{picker}");
    let saved = home.join("tmp").join("Saved");
    std::fs::create_dir_all(&saved).unwrap();
    let path = saved.join("IMG_0004.png");
    c.ok("ui.filePicker", json!({ "path": path }));
    idle(c);
    let png = std::fs::read(&path).unwrap();
    assert_eq!(dalica_image::read_header(&png, &Limits::default()).unwrap().format, dalica_image::Format::Png);
    let as_png = decode(&png);
    assert_eq!((as_png.width, as_png.height), SIZE);
    let mean = as_png.data.iter().zip(&edited.data).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / as_png.data.len() as f64;
    assert!(mean < 3.0, "Save As PNG holds the edit, as the copy does: mean difference {mean:.2}");
    assert!(c.photos().iter().any(|p| p["name"] == "IMG_0004.png"), "the new file joins the library");
    assert!(c.photos().iter().any(|p| p["name"] == "IMG_0004-001.jpg"), "and the copy");
    println!("Save a Copy: IMG_0004-001.jpg beside it, the photo untouched; Save As: {} (mean difference to the copy {mean:.2})", path.display());
}

/// Arms `point` to crash at the first time this thread reaches it with `when` holding (checked
/// just before each such step).
fn crash_when(point: &'static str, when: Rc<dyn Fn() -> bool>) {
    faults::run_at(point, move || if when() { faults::arm(point, Fault::Crash) } else { crash_when(point, when) });
}

/// A Save of `name` with [`AUTOFIX`] run straight through `create`, as the app's worker runs it,
/// killed at `point` once `when` holds.
fn killed_save(folder: &Path, name: &str, point: &'static str, when: Rc<dyn Fn() -> bool>) {
    crash_when(point, when);
    let options = save::Options::new(Timestamp::from_system_time(SystemTime::now()));
    let result = save::in_place(folder, name, &Recipe::parse([AUTOFIX]).unwrap(), &Changes::default(), &options);
    faults::disarm_all();
    assert!(matches!(result, Err(save::Error::Fs(dalica_fs::Error::Crashed(_)))), "{name}: the Save was killed: {result:?}");
}

#[test]
fn save_undo_save_revert_copies_and_a_killed_save_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let (folder, originals) = lisbon(home.path());

    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    run(&mut c, "folders.add", json!({ "path": folder }));
    let ids = ids_in(&mut c, "Lisbon");
    assert_eq!(ids.len(), LISBON.len(), "{ids:?}");
    save_by_agent(&mut c, &folder, &originals[BY_AGENT], &ids[BY_AGENT]);
    let level = save_by_hand(&mut c, home.path(), &folder, &originals, &ids);
    copies(&mut c, home.path(), &folder, &originals[COPIED], &ids[COPIED]);
    // Two photos edited, then the app quits with its writes landed.
    apply(&mut c, &ids[ROLLED_BACK], AUTOFIX);
    apply(&mut c, &ids[ROLLED_FORWARD], AUTOFIX);
    live::quit(app, c);
    let part1 = secs(t);

    // Two Saves killed: before the commit (the original in the backup, nothing at its name) and
    // after it (the record still pending).
    killed_save(&folder, ROLLED_BACK, points::BACKUP_FSYNC_DIR, Rc::new(|| true));
    assert!(!folder.join(ROLLED_BACK).exists(), "killed before the commit: nothing at the photo's name");
    assert_eq!(entry(&folder, ROLLED_BACK).map(|e| e.0).as_deref(), Some("pending"));
    // At the rename that marks the record done: the first one once the new file is at the name.
    let (path, original) = (folder.join(ROLLED_FORWARD), originals[ROLLED_FORWARD].clone());
    killed_save(&folder, ROLLED_FORWARD, points::RENAME, Rc::new(move || std::fs::read(&path).is_ok_and(|b| b != original)));
    let forward = bytes(&folder, ROLLED_FORWARD);
    differ!(forward, originals[ROLLED_FORWARD], "killed after the commit: the file holds the edit");
    assert_eq!(entry(&folder, ROLLED_FORWARD).map(|e| e.0).as_deref(), Some("pending"));

    // The app again: it repairs the folder.
    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    idle(&mut c);
    let ids = ids_in(&mut c, "Lisbon");
    same!(bytes(&folder, ROLLED_BACK), originals[ROLLED_BACK], "rolled back: the original at its name, byte for byte");
    assert!(kept(&folder, ROLLED_BACK).is_none() && entry(&folder, ROLLED_BACK).is_none(), "no backup and no record");
    assert_eq!(edits(&mut c, &ids[ROLLED_BACK]), [AUTOFIX], "rolled back: the edit unsaved");
    same!(bytes(&folder, ROLLED_FORWARD), forward, "rolled forward: the edit in the file");
    same!(kept(&folder, ROLLED_FORWARD), Some(originals[ROLLED_FORWARD].clone()), "rolled forward: the original kept");
    assert_eq!(entry(&folder, ROLLED_FORWARD), Some(("done".into(), vec![AUTOFIX.to_string()])));
    assert!(edits(&mut c, &ids[ROLLED_FORWARD]).is_empty());
    job(&mut c, "photo.undoSave", json!({ "ids": [ids[ROLLED_FORWARD]] }));
    same!(bytes(&folder, ROLLED_FORWARD), originals[ROLLED_FORWARD], "Undo Save of the repaired photo: the exact original bytes");
    assert_eq!(edits(&mut c, &ids[ROLLED_FORWARD]), [AUTOFIX]);
    // What part 1 left is as it was: the lossless save kept, the reverted photos plain.
    assert_eq!(edits(&mut c, &ids[COPIED]), [AUTOFIX], "the copied photo's edit survives the restart");
    assert_eq!(entry(&folder, TURNED), Some(("done".into(), vec![TURN.to_string()])));
    for name in [BY_HAND, BY_AGENT] {
        same!(bytes(&folder, name), originals[name]);
    }
    live::quit(app, c);
    let left = temporaries(&folder);
    assert!(left.is_empty(), "no temporary file is left: {left:?}");
    println!("a Save killed before its commit: rolled back; after it: rolled forward, Undo Save gives the original back; nothing left over");
    println!(
        "M8 save in the live app: passed in {:.1} s (build {build:.1} s, part 1 {part1:.1} s, the restart {:.1} s; lossless within {level})",
        secs(started),
        secs(t)
    );
}

// --- part 2: Export -----------------------------------------------------------------------------

/// The files of export folder `dir`: each decoded and checked (size, quality, metadata, places).
fn check_export(dir: &Path, numbered: bool, located: bool) -> Vec<(String, Rgba8)> {
    let mut out = Vec::new();
    let names: Vec<String> = files(dir).into_iter().filter(|n| n.ends_with(".jpg")).collect();
    assert_eq!(names.len(), PORTO.len(), "{}: {names:?}", dir.display());
    for (i, ((name, _, has_place), file)) in PORTO.into_iter().zip(&names).enumerate() {
        let want = if numbered { format!("{:03}-{name}", i + 1) } else { name.to_string() };
        assert_eq!(*file, want, "named in the folder's order");
        let b = bytes(dir, file);
        let header = dalica_image::read_header(&b, &Limits::default()).unwrap();
        assert_eq!((header.format, header.width, header.height, header.orientation.get()), (dalica_image::Format::Jpeg, 320, 240, 1), "{file}");
        let q = dalica_image::jpeg_quality::quality(&b).unwrap_or(0);
        assert!(q.abs_diff(NORMAL) <= 2, "{file}: quality {q}, Normal is {NORMAL} ± 2");
        let meta = dalica_meta::read(&b, &dalica_meta::Limits::default()).unwrap();
        assert_eq!((meta.summary.make.as_deref(), meta.summary.model.as_deref()), (Some("Dalica"), Some("Scenes 1")), "{file}: the camera");
        assert!(meta.owned.date_taken.is_some(), "{file}: the date taken");
        if i == 0 {
            assert!(meta.owned.tags.iter().any(|t| t == "tram"), "{file}: the library's tags: {:?}", meta.owned.tags);
        }
        if i == 1 {
            assert_eq!(meta.owned.caption.as_deref(), Some("Ribeira at dawn"), "{file}: the library's caption");
        }
        if located && has_place {
            assert!(meta.owned.place.is_some(), "{file}: the place kept");
        } else {
            assert_eq!(meta.owned.place, None, "{file}: no place");
            assert_eq!((meta.sources.exif.place, meta.sources.xmp.place), (None, None), "{file}: no GPS in EXIF or XMP");
        }
        out.push((file.clone(), decode(&b)));
    }
    out
}

/// The newest export job's id (`jobs.list`), if any.
fn export_job(c: &mut Client) -> Option<Value> {
    jobs(c).into_iter().filter(|j| j["kind"] == "export").map(|j| j["job"].clone()).max_by_key(Value::as_u64)
}

#[test]
fn exports_decoded_the_dialog_the_progress_window_and_cancel_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let (porto, big) = porto_and_big(home.path());
    let exports = home.path().join("tmp").join("Exports");

    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    run(&mut c, "folders.add", json!({ "path": porto }));
    run(&mut c, "folders.add", json!({ "path": big }));
    let ids = ids_in(&mut c, "Porto");
    assert_eq!(ids.len(), PORTO.len(), "{ids:?}");
    let first = ids["IMG_0001.jpg"].clone();
    let section = section_of(&mut c, &first);
    run(&mut c, "photo.tags.add", json!({ "ids": [first], "tags": ["tram"] }));
    run(&mut c, "photo.caption.set", json!({ "ids": [ids["IMG_0002.jpg"]], "caption": "Ribeira at dawn" }));
    idle(&mut c);

    // The agent's exports: the location removed, then kept.
    let params = |name: &str, numbers: bool, remove: bool| {
        json!({ "section": section, "dir": exports, "folderName": name, "numbers": numbers, "size": LONG_SIDE,
                "quality": "normal", "watermark": false, "removeLocation": remove })
    };
    job(&mut c, "export.run", params("Porto Places", false, false));
    check_export(&exports.join("Porto Places"), false, true);
    let done = job(&mut c, "export.run", params("Porto Agent", true, true));
    let r = &done["result"];
    assert_eq!((r["total"].as_u64(), r["done"].as_u64(), r["name"].as_str()), (Some(6), Some(6), Some("Porto Agent")), "{done}");
    assert_eq!((r["skipped"].clone(), r["failed"].clone(), r["cancelled"].clone()), (json!([]), json!([]), json!(false)));
    let plain = check_export(&exports.join("Porto Agent"), true, false);
    println!(
        "export.run at {LONG_SIDE} px, Normal: sizes, quality {NORMAL} ± 2, camera, date, caption and tags carried; GPS removed when asked, kept otherwise"
    );

    // The Export dialog from the section header's Export (its row in the tree brings it up), with
    // a watermark.
    quiet(&mut c, 900.0);
    click(&mut c, &format!("tree:{section}"));
    click(&mut c, &format!("section:{section}/export"));
    assert!(c.has("dialog:export"), "the header's Export opens the dialog");
    let draft = c.ok("ui.state", json!({}))["export"].clone();
    assert_eq!(draft["folderName"], "Porto", "the dialog suggests the folder's name: {draft}");
    let set = json!({ "export": { "location": exports, "originalSize": false, "resize": LONG_SIDE.to_string(), "quality": "normal",
                                  "numbers": false, "watermark": true, "watermarkText": WATERMARK } });
    c.ok("ui.set", set);
    idle(&mut c);
    for (w, h) in [(1400u32, 900u32), (900, 600)] {
        c.ok("ui.resize", json!({ "width": w, "height": h }));
        idle(&mut c);
        park(&mut c, f64::from(h));
        let location = rect_of(&mut c, "field:dialog.location").expect("the location field");
        masked_shot(&mut c, home.path(), &format!("m8-{w}x{h}-export"), (w, h), &[location]);
    }
    c.ok("ui.resize", json!({ "width": 1400, "height": 900 }));
    idle(&mut c);
    click(&mut c, "button:dialog.export");
    assert!(!c.has("dialog:export"), "Export closes the dialog");
    // Remove location is an option (Options ▸ Export), not a row of the dialog: the export takes
    // the one used last, on.
    let marked = check_export(&exports.join("Porto"), false, false);
    let mut least = usize::MAX;
    for ((name, with), (_, without)) in marked.iter().zip(&plain) {
        let b = dalica_create::watermark::layout(with.width, with.height, WATERMARK).unwrap();
        let text = (b.x.floor() as u32, b.y.floor() as u32, (b.x + b.width).ceil() as u32, (b.y + b.height).ceil() as u32);
        let inside = changed(with, without, text);
        least = least.min(inside);
        assert!(inside > 10, "{name}: the watermark is in its box ({inside} pixels changed)");
        assert_eq!(changed(with, without, (0, 0, with.width / 2, with.height / 2)), 0, "{name}: the rest is the agent's export");
    }
    println!("the Export dialog from the header: \"Porto\" suggested, the watermark in its box (at least {least} pixels), the rest unchanged");

    // A long export from the dialog: the progress window, then its Cancel.
    let big_id = ids_in(&mut c, "Big").into_values().next().unwrap();
    let big_section = section_of(&mut c, &big_id);
    quiet(&mut c, 900.0);
    click(&mut c, &format!("tree:{big_section}"));
    click(&mut c, &format!("section:{big_section}/export"));
    let set = json!({ "export": { "location": exports, "folderName": "Big", "originalSize": true, "quality": "maximum", "numbers": false, "watermark": false } });
    c.ok("ui.set", set);
    c.ok("ui.settle", json!({ "timeoutMs": 5000 }));
    c.ok("ui.clickWidget", json!({ "id": "button:dialog.export" }));
    park(&mut c, 900.0);
    assert!(c.has("dialog:exportProgress"), "the export opens its progress window");
    let bar = rect_of(&mut c, "dialog:exportProgress/bar").expect("the bar");
    // The line above the bar and the bar follow the job; so do the status line and the indicator.
    let mut masks = vec![[bar[0], bar[1] - 24.0, bar[2], bar[3] + 26.0]];
    masks.extend(["text:status", "indicator:activity"].iter().filter_map(|id| rect_of(&mut c, id)));
    let line = label(&mut c, "dialog:exportProgress/bar");
    assert!(line.starts_with("Exporting"), "{line}");
    let progress = masked_shot(&mut c, home.path(), "m8-1400x900-progress", (1400, 900), &masks);
    let job_id = export_job(&mut c).expect("the export job");
    click(&mut c, "button:exportProgress.cancel");
    let entry = wait_job(&mut c, &job_id);
    assert_eq!(entry["state"], "cancelled", "{entry}");
    let r = &entry["result"];
    let written = r["written"].as_array().map_or(0, Vec::len);
    assert!(written < BIG && r["cancelled"] == true, "Cancel stopped it: {written} of {BIG} written");
    assert!(!c.has("dialog:exportProgress"), "the window closes when its job ends");
    let out = exports.join("Big");
    let names: Vec<String> = files(&out).into_iter().filter(|n| n != ".dalica.json").collect();
    assert_eq!(names.len(), written, "only the files written are there: {names:?}");
    for name in &names {
        let img = decode(&bytes(&out, name));
        assert_eq!((img.width, img.height), BIG_SIZE, "{name} is complete");
    }
    assert!(temporaries(&out).is_empty());

    // The export folders are listed under Projects: their rows lie between the group's header
    // and the next group's.
    let mut exported: Vec<u64> = Vec::new();
    for p in c.photos() {
        let inspected = c.inspect(&p["id"]);
        let path = PathBuf::from(inspected["path"].as_str().unwrap_or_default());
        if path.parent().and_then(Path::parent) == Some(exports.as_path()) {
            exported.extend(inspected["folder"].as_u64().filter(|f| !exported.contains(f)));
        }
    }
    assert_eq!(exported.len(), 4, "Porto Places, Porto Agent, Porto and Big joined the library: {exported:?}");
    let projects = rect_of(&mut c, "tree:group:projects").expect("the tree's Projects group");
    let next = c.widgets("tree:group:").iter().filter_map(|w| w["rect"][1].as_f64()).filter(|y| *y > projects[1]).fold(f64::INFINITY, f64::min);
    for f in &exported {
        let row = rect_of(&mut c, &format!("tree:folder:{f}")).unwrap_or_else(|| panic!("the tree lists folder {f}"));
        assert!(
            row[1] > projects[1] && row[1] < next,
            "folder {f} is under Projects: {row:?} (Projects at {}, the next group at {next})",
            projects[1]
        );
    }
    println!(
        "the progress window \"{line}\", Cancel after {written} of {BIG}: only complete files; the {} export folders under Projects",
        exported.len()
    );
    live::quit(app, c);
    println!(
        "M8 export in the live app: passed in {:.1} s (build {build:.1} s, the app {:.1} s); screenshot {}",
        secs(started),
        secs(t),
        progress.path.display()
    );
}

// --- parts 3 and 4 ------------------------------------------------------------------------------

#[test]
fn the_crate_suites_panic_hunt_the_quality_estimate_and_the_save_export_budgets() {
    let started = Instant::now();
    if !dalica_acceptance::suites_ran() {
        checks::crate_suites();
    }
    let suites = secs(started);
    let t = Instant::now();
    checks::panic_hunt();
    let hunt = secs(t);
    let (error, files) = checks::quality_estimate_error();
    println!("the quality estimate: at most {error} off over {files} JPEGs of known quality (two encoders, 4:2:0 and 4:4:4)");
    let t = Instant::now();
    checks::save_export_debug();
    let bench = secs(t);
    if std::env::var_os(checks::RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        let [save, rotate, export] = checks::release_budgets();
        println!(
            "release budgets ({:.1} s): Save of a 24 MP JPEG {save:.0} ms, its lossless rotation {rotate:.0} ms, the 200-photo export {:.1} s; quality estimate error {error}",
            secs(t),
            export / 1000.0
        );
    } else {
        println!("(the release budgets, 24 MP saves and the 200-photo export: run with {}=1)", checks::RELEASE);
    }
    println!("suites {suites:.1} s, panic_hunt {hunt:.1} s, debug bench {bench:.1} s; M8 checks: passed in {:.1} s", secs(started));
}
