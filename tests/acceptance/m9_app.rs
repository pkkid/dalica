//! Driving the live app for M9's acceptance (`m9.rs`): the waits, commands, widgets, the photos by
//! path, the disk, and whole-window fingerprints with masked parts. Built on `m6_live.rs`'s
//! process and connection, the way `m8.rs` drives it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

use crate::live::{self, Client, Shot};

pub fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

// --- waiting ------------------------------------------------------------------------------------

/// `jobs.list`.
pub fn jobs(c: &mut Client) -> Vec<Value> {
    c.ok("jobs.list", json!({})).as_array().cloned().unwrap_or_default()
}

/// Frames until nothing is pending, background work included, however long it takes in real time
/// (at most ten minutes): `ui.settle` with `jobs` waits for the scans, thumbnails and file jobs,
/// and every job `jobs.list` shows unfinished is waited for with `jobs.wait`, until a settle finds
/// none.
pub fn idle(c: &mut Client) {
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

/// An engine or UI command that must succeed, then [`idle`]; its result.
pub fn run(c: &mut Client, command: &str, params: Value) -> Value {
    let r = c.ok("engine.execute", json!({ "command": command, "params": params }));
    let r = match r.get("job") {
        // A file job replies at once; its result is the job's.
        Some(job) if r["pending"] == true => {
            let entry = wait_job(c, &job.clone());
            assert_eq!(entry["state"], "done", "{command} {params}: {entry}");
            entry
        }
        _ => r,
    };
    idle(c);
    r
}

/// The job `job` once it has ended (`jobs.wait`, as often as it takes, at most ten minutes).
pub fn wait_job(c: &mut Client, job: &Value) -> Value {
    for _ in 0..12 {
        let entry = c.ok("jobs.wait", json!({ "job": job, "timeoutMs": 50000 }));
        if entry["pending"] != true {
            idle(c);
            return entry;
        }
    }
    panic!("job {job} did not end within ten minutes");
}

/// Lets the sidecar writes' debounce run out (`store::writer::DEBOUNCE`, a second on the session's
/// clock, which is the system's even headless) and waits for them, so the folders'
/// `.dalica.json` files say what the library says.
pub fn flush(c: &mut Client) {
    idle(c);
    std::thread::sleep(dalica_store_debounce() + Duration::from_millis(400));
    c.ok("ui.wait", json!({ "ms": 100 }));
    idle(c);
}

/// `dalica_store::writer::DEBOUNCE` (this crate does not depend on `store`).
fn dalica_store_debounce() -> Duration {
    Duration::from_secs(1)
}

/// How long a look at the disk waits for a write it expects (see [`flush_until`]).
const DISK_WAIT: Duration = Duration::from_secs(60);

/// [`flush`], then waits (at most [`DISK_WAIT`]) until `holds`, a look at the disk, is true; it
/// returns either way, and the caller's assertions that follow say what is wrong.
///
/// [`flush`] is a guess: the control channel cannot ask for the sidecar writes (only `app.quit`
/// flushes them, `Session::flush`). They are made on the I/O worker on the session's clock, each
/// folder at most once per debounce, so one made just before the last change waits a debounce for
/// the next; on a loaded machine that write starts late and takes long (Windows: the `fsync`, a
/// virus scanner reading every new file), and one that fails (a file another program holds open)
/// is retried a debounce later. So a test waits for what it means to read.
pub fn flush_until(c: &mut Client, mut holds: impl FnMut() -> bool) {
    flush(c);
    let end = Instant::now() + DISK_WAIT;
    while !holds() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Edit ▸ Undo over `engine.execute` (a file operation's undo runs as a job), then [`idle`].
pub fn undo(c: &mut Client) -> Value {
    run(c, "edit.undo", json!({}))
}

pub fn redo(c: &mut Client) -> Value {
    run(c, "edit.redo", json!({}))
}

// --- the window ---------------------------------------------------------------------------------

pub fn key(c: &mut Client, combo: &str) {
    let field = if combo.contains('+') { "combo" } else { "key" };
    c.ok("ui.key", json!({ field: combo }));
    idle(c);
}

pub fn click(c: &mut Client, id: &str) {
    c.ok("ui.clickWidget", json!({ "id": id }));
    idle(c);
}

/// The widget `id`, if it is shown.
pub fn widget(c: &mut Client, id: &str) -> Option<Value> {
    c.widgets(id).into_iter().find(|w| w["id"] == id)
}

/// The label a widget shows.
pub fn label(c: &mut Client, id: &str) -> String {
    widget(c, id).and_then(|w| w["label"].as_str().map(str::to_string)).unwrap_or_default()
}

/// The open dialog's kind (`ui.state` → `dialogs`), if any.
pub fn dialog(c: &mut Client) -> Option<String> {
    let s = c.ok("ui.state", json!({}));
    s["dialogs"].as_array().and_then(|d| d.last()).and_then(|d| d["kind"].as_str().or_else(|| d.as_str())).map(str::to_string)
}

/// The rect of the widget `id`, if it is shown.
pub fn rect_of(c: &mut Client, id: &str) -> Option<[f64; 4]> {
    let w = widget(c, id)?;
    Some([0, 1, 2, 3].map(|i| w["rect"][i].as_f64().unwrap_or(0.0)))
}

pub fn center(r: [f64; 4]) -> (f64, f64) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

/// Rests the pointer on the status line's left end and lets the frames run (without waiting for
/// jobs).
pub fn park(c: &mut Client, h: f64) {
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": 2.0, "y": h - 106.0 }] }));
    c.ok("ui.settle", json!({ "timeoutMs": 5000 }));
}

/// Lets the toasts of the operations before run out (headless, `ui.wait` advances the app's
/// clock), so a screenshot shows none whatever the operations took in real time.
pub fn quiet(c: &mut Client, h: f64) {
    park(c, h);
    c.ok("ui.wait", json!({ "ms": 20000 }));
    idle(c);
}

pub fn resize(c: &mut Client, (w, h): (u32, u32)) {
    c.ok("ui.resize", json!({ "width": w, "height": h }));
    idle(c);
    park(c, f64::from(h));
}

/// A whole-window screenshot (`live::screenshot`) whose `masks` (rects in points) are painted one
/// grey before it is matched against `fingerprints/<name>.json` (written first under
/// `DALICA_BLESS=1`): parts that hold a temporary path or a time.
pub fn masked_shot(c: &mut Client, home: &Path, name: &str, (w, h): (u32, u32), masks: &[[f64; 4]]) -> Shot {
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

/// The rects that follow the clock or a job whatever the test does: the status line's text and
/// the activity indicator.
pub fn moving_parts(c: &mut Client) -> Vec<[f64; 4]> {
    ["text:status", "indicator:activity"].iter().filter_map(|id| rect_of(c, id)).collect()
}

// --- the library --------------------------------------------------------------------------------

/// One photo as the library knows it.
#[derive(Clone, Debug)]
pub struct Photo {
    pub id: Value,
    pub folder: u64,
    pub inspected: Value,
}

/// Every photo of the library by path relative to `root` (`/`-separated:
/// `"Trips/Lisbon/IMG_0001.jpg"`), with what `photo.inspect` says of it; photos outside `root`
/// are left out.
pub fn photos(c: &mut Client, root: &Path) -> BTreeMap<String, Photo> {
    let ids: Vec<Value> = c.photos().into_iter().map(|p| p["id"].clone()).collect();
    let mut out = BTreeMap::new();
    for chunk in ids.chunks(500) {
        let r = c.ok("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": chunk } }));
        for p in r.as_array().cloned().unwrap_or_default() {
            let path = PathBuf::from(p["path"].as_str().unwrap_or_default());
            if let Ok(rel) = path.strip_prefix(root) {
                let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                let folder = p["folder"].as_u64().unwrap_or_else(|| panic!("photo.inspect names the folder: {p}"));
                out.insert(rel.join("/"), Photo { id: p["id"].clone(), folder, inspected: p });
            }
        }
    }
    out
}

/// The photos of a section (`library.query {section}`), in its order, as paths relative to `root`.
pub fn section(c: &mut Client, root: &Path, section: &str) -> Vec<String> {
    let all = photos(c, root);
    let by_id: BTreeMap<String, String> = all.iter().map(|(rel, p)| (p.id.to_string(), rel.clone())).collect();
    let r = c.ok("engine.execute", json!({ "command": "library.query", "params": { "section": section, "limit": 10000 } }));
    r["photos"].as_array().unwrap().iter().map(|p| by_id.get(&p["id"].to_string()).cloned().unwrap_or_else(|| format!("?{}", p["id"]))).collect()
}

/// `albums.list`'s entry for album `album`, if there is one.
pub fn album(c: &mut Client, album: &str) -> Option<Value> {
    let r = c.ok("engine.execute", json!({ "command": "albums.list", "params": {} }));
    r.as_array().and_then(|a| a.iter().find(|x| x["id"] == album).cloned())
}

// --- the disk -----------------------------------------------------------------------------------

/// `root/<rel>` for `"Trips/Lisbon"`.
pub fn under(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|c| !c.is_empty()).fold(root.to_path_buf(), |p, c| p.join(c))
}

pub fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// The names in `dir` that are not Dalica's own (`.dalica…`), sorted, once no write is in flight
/// there: a file being replaced atomically (a sidecar's debounced write, which no command waits
/// for) shows as its temporary, `.<name>.dalica-tmp-<hex>` ([`dalica_fs::atomic::is_temp_name`]),
/// until its rename. The listing waits for it to go, at most [`DISK_WAIT`]: one still there then
/// is a leftover the user would see, and fails the test.
pub fn files(dir: &Path) -> Vec<String> {
    let end = Instant::now() + DISK_WAIT;
    loop {
        let listed: Vec<std::ffi::OsString> = std::fs::read_dir(dir).map(|l| l.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
        let writing: Vec<&std::ffi::OsString> = listed.iter().filter(|n| dalica_fs::atomic::is_temp_name(n)).collect();
        if writing.is_empty() {
            let mut names: Vec<String> = listed.iter().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.starts_with(".dalica")).collect();
            names.sort();
            return names;
        }
        assert!(Instant::now() < end, "{}: a write never finished, its temporary file is still there: {writing:?}", dir.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A folder's sidecar entry for `name`.
pub fn entry(folder: &Path, name: &str) -> Option<Value> {
    let v: Value = serde_json::from_slice(&read(&folder.join(".dalica.json"))?).ok()?;
    v.get("photos").and_then(|p| p.get(name)).cloned()
}

/// A JPEG of `scenes` with camera EXIF, taken at `taken` (`"2024-06-03T10:00:00"`).
pub fn jpeg(seed: u64, (w, h): (u32, u32), make: &str, model: &str, taken: &str) -> Vec<u8> {
    let taken: dalica_core::DateTime = taken.parse().unwrap();
    let exif = dalica_scenes::Exif {
        orientation: 1,
        date_time_original: Some(taken),
        make: Some(make.into()),
        model: Some(model.into()),
        ..Default::default()
    };
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, w, h), 85, Some(&exif)).unwrap()
}

/// Writes `bytes` at `path` (its folders made) with a modification time fixed by `i`.
pub fn put(path: &Path, bytes: &[u8], i: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    let at = std::time::UNIX_EPOCH + Duration::from_secs(1_717_236_000 + 60 * i);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
}
