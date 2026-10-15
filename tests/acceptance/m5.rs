//! M5 "Folders, scanning and formats" (`docs/roadmap.md` → M5 → the *Done when* of Folders and
//! scanning, and of Formats and colour; run by `cargo xtask accept M5`).
//!
//! 1. **The live app on a mixed-format folder.** The real `dalica` binary in `--headless` mode on
//!    a fresh library directory, driven over its control channel as an agent would: `folders.add`
//!    of a folder holding one file of every format of `formats.md` §1 (JPEG, a turned JPEG,
//!    16-bit PNG, TIFF, 16-bit CMYK TIFF, WebP with alpha, GIF, BMP, JPEG XL, PSD, a RAW, a
//!    Display P3 JPEG), a HEIC, an AVIF and a damaged JPEG. Every file is listed; the twelve show
//!    their pictures; the HEIC, the AVIF and the damaged file show the `caution` "!" with their
//!    reasons in the tooltip. Whole-window screenshots at 1400 × 900 and 900 × 600 are matched
//!    against `fingerprints/m5-*.json`.
//! 2. **Temp-dir scenarios while the session runs** (`m5_folders.rs`, tests of their own in this
//!    binary, on the window through the headless driver and the session's virtual clock): files
//!    and folders added, removed, renamed, rewritten and moved, through the watcher; the same found
//!    by the verification pass alone with the watcher off; a volume "unplugged" by renaming its
//!    root dimmed offline, a star made meanwhile written when it returns; an emptied mount point
//!    offline with no sidecar written into it; first run on a fresh library, the thumbnails filling
//!    in while the scan runs.
//! 3. **Formats and colour** (`m5_formats.rs`, its own test): every format at each depth and
//!    colour model its container stores, compared with the scene it was made from; sRGB and
//!    wide-gamut profiles in every container that embeds one, with colour management on and off;
//!    orientation 1–8 in JPEG, PNG, TIFF, WebP and RAW; truncations and seeded damage of every
//!    format through every entry point, never a panic.
//! 4. **The crate suites, run as they are:** the decoders and their fingerprints (`image`: decode,
//!    colour, TIFF, WebP/GIF/BMP, JPEG XL, PSD, RAW in TIFF and other containers, HEIF with and
//!    without its feature), `scenes`' writers, `fs`' held folder handle and stale temporaries,
//!    `scan`'s skip rules, volumes, watcher, incremental diff and folder modes, the engine's jobs,
//!    progressive scans, folders, offline volumes, watching and thumbnail cache, `thumbs`' upkeep,
//!    `store`'s offline and stale writes, the tree model, the UI's dialogs, jobs, Folder Manager,
//!    first run, activity, offline folders, colour management and M4's loose ends, the shared
//!    fields and folder-mode icons, and `panic_hunt -- --ignored`. Inside `cargo xtask ci` its
//!    test step already ran them, so they are skipped there (`dalica_acceptance::SUITES_RAN`).
//! 5. **The first-thumbnails bench in a debug build:** 1 000 JPEGs added; the first screenful is
//!    drawn from the embedded thumbnails before the rest of the thumbnails are made (the
//!    progressive path); its time and the scan's are printed, not judged (a debug time limit
//!    would only measure the machine).
//! 6. **The corpus floors** (`development.md` §11), when `corpus/` holds every published set and
//!    `cargo xtask corpus` verifies them: `dalica-image`'s corpus test in release. When it does
//!    not, or inside `ci`, this says so plainly; the CI `corpus` job is the strict run. Sets not
//!    published yet (`photos`, `heif`) are reported as such, never counted as passed.
//! 7. **With `DALICA_ACCEPT_RELEASE=1`, the budgets in a release build:** `first_thumbnails` in
//!    full: the first screenful ≤ 1 s for 1 k and 10 k JPEGs with the full scan's numbers, and the
//!    scanned 100 k pass at the top thumb speed (0 placeholder cells, owner frame p95 ≤ 8 ms). The
//!    `budgets` job runs the same per push with a 20 k pass (`xtask/src/accept.rs` → `BUDGETS`).
//!
//! The manual check (first run on a fresh library directory, watching thumbnails fill in) is in
//! `m5.md`.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written. Headless
//! renders always draw the Windows/Linux title bar, so the fingerprints are the same on every OS.

#[path = "m5_folders.rs"]
mod folders;
#[path = "m5_formats.rs"]
mod formats;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// The environment variable that adds the release-build budgets (part 7).
const RELEASE: &str = "DALICA_ACCEPT_RELEASE";
/// The first screenful's budget (`development.md` §8; `xtask/src/accept.rs` → `FIRST_THUMBNAILS_MS`).
const FIRST_MS: f64 = 1000.0;
/// The UI frame budget, p95.
const FRAME_MS: f64 = 8.0;

// ---------------------------------------------------------------------------------------------
// Cargo, the app and its control channel (as in m4.rs)
// ---------------------------------------------------------------------------------------------

/// `dalica`, built for this test run (it is another package's binary).
fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output.
fn cargo(args: &[&str]) -> String {
    let started = Instant::now();
    let out = dalica_acceptance::cargo_split(args, |_| {}).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "cargo {}:\n{stdout}\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    println!("cargo {}: ok ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
    stdout
}

/// Runs `cargo test <args>`; panics with its output if it fails or runs nothing.
fn cargo_test(args: &[&str]) {
    let all: Vec<&str> = ["test"].iter().chain(args).copied().collect();
    let stdout = cargo(&all);
    let passed: usize =
        stdout.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{stdout}", all.join(" "));
    println!("  {passed} passed");
}

/// The JSON report a bench printed (its last line that parses as an object with `rows`).
fn bench_report(stdout: &str) -> Value {
    stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("rows").is_some_and(Value::is_array))
        .unwrap_or_else(|| panic!("no JSON report in:\n{stdout}"))
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["rows"].as_array().unwrap().iter().find(|r| r["name"] == name).unwrap_or_else(|| panic!("no row `{name}` in {report}"))
}

/// A port nobody listens on now (the OS picks it; it is free again once the probe closes).
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A `dalica` process, killed if the test ends without quitting it.
struct App {
    child: Child,
}

impl Drop for App {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl App {
    fn wait(&mut self, within: Duration) -> ExitStatus {
        let end = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < end, "dalica did not exit within {within:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// `dalica ARGS` with every folder it could touch inside `home`.
fn dalica(exe: &Path, home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(exe);
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    // Standard error (the log) goes to a file: a pipe nobody reads could fill and stall the app.
    let log = std::fs::OpenOptions::new().create(true).append(true).open(home.join("stderr.log")).unwrap();
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log));
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join("config")),
        ("XDG_CACHE_HOME", home.join("cache")),
        ("DALICA_CACHE_DIR", home.join("dalica-cache")),
        ("XDG_DATA_HOME", home.join("data")),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("APPDATA", home.join("appdata")),
        ("LOCALAPPDATA", home.join("localappdata")),
        ("TMPDIR", tmp.clone()),
        ("TMP", tmp.clone()),
        ("TEMP", tmp),
    ] {
        c.env(k, v);
    }
    for k in [
        "DALICA_HOME",
        "DALICA_CONTROL_PORT",
        "DALICA_CONTROL_TOKEN_FILE",
        "DALICA_AUTOMATION_READ_ROOT",
        "DALICA_AUTOMATION_WRITE_ROOT",
        "DALICA_PROFILE",
    ] {
        c.env_remove(k);
    }
    c
}

/// Waits for the token file to hold a token; returns it.
fn token(path: &Path, app: &mut App) -> String {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let t = text.trim();
            if t.len() == 64 {
                return t.to_string();
            }
        }
        if let Some(status) = app.child.try_wait().unwrap() {
            panic!("dalica exited early with {status}");
        }
        assert!(Instant::now() < end, "no token file at {}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A control connection, used the way an agent would.
struct Client {
    out: TcpStream,
    replies: BufReader<TcpStream>,
    next: u64,
}

impl Client {
    fn connect(port: u16) -> Client {
        let out = TcpStream::connect(("127.0.0.1", port)).unwrap();
        out.set_read_timeout(Some(Duration::from_secs(180))).unwrap();
        out.set_nodelay(true).unwrap();
        let replies = BufReader::new(out.try_clone().unwrap());
        Client { out, replies, next: 0 }
    }

    /// Sends one request line and reads its reply.
    fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = format!("{}\n", json!({ "id": self.next, "method": method, "params": params }));
        self.out.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        let v: Value = serde_json::from_str(&reply).unwrap_or_else(|e| panic!("{method}: not JSON ({e}): {reply:?}"));
        assert_eq!(v["id"], self.next, "{v}");
        v
    }

    /// A request that must succeed; its result.
    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.send(method, params.clone());
        assert_eq!(v["ok"], true, "{method} {params}: {v}");
        v["result"].clone()
    }

    /// Frames until nothing is pending, long jobs (the scan, thumbnail generation) included.
    fn settle_jobs(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 60000, "jobs": true }));
        assert_eq!(s["settled"], true, "{s}");
    }

    fn settle(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 30000 }));
        assert_eq!(s["settled"], true, "{s}");
    }

    fn act(&mut self, method: &str, params: Value) -> Value {
        let r = self.ok(method, params);
        self.settle();
        r
    }

    /// The library's photos in display order (`library.query`).
    fn photos(&mut self) -> Vec<Value> {
        let r = self.ok("engine.execute", json!({ "command": "library.query", "params": {} }));
        r["photos"].as_array().unwrap().clone()
    }

    fn widgets(&mut self, filter: &str) -> Vec<Value> {
        self.ok("ui.widgets", json!({ "filter": filter })).as_array().unwrap().clone()
    }

    /// The tooltip after resting the pointer on `id` (`ui.hoverWidget`), as `ui.inspect` reads it.
    fn tooltip(&mut self, id: &str) -> String {
        self.act("ui.hoverWidget", json!({ "id": id, "ms": 1500 }));
        self.ok("ui.inspect", json!({}))["tooltip"].as_str().unwrap_or_default().to_string()
    }

    /// Rests the pointer on the status line's left end (no widget there), so screenshots show no
    /// hover highlight or tooltip.
    fn park_pointer(&mut self, h: f64) {
        self.act("ui.pointer", json!({ "events": [{ "kind": "move", "x": 2.0, "y": h - 106.0 }] }));
    }
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// A whole-window screenshot of size `w × h`, kept in `target/test-output/acceptance/` and matched
/// against `fingerprints/<name>.json` (written first under `DALICA_BLESS=1`) with the tolerance M2
/// to M4 use. Returns where the PNG is and its pixels.
fn screenshot(c: &mut Client, home: &Path, name: &str, w: u32, h: u32) -> (PathBuf, Vec<u8>) {
    let shot = home.join("tmp").join(format!("{name}.png"));
    let r = c.ok("ui.screenshot", json!({ "path": shot }));
    assert_eq!((r["width"].clone(), r["height"].clone(), r["headless"].clone()), (json!(w), json!(h), json!(true)), "{name}: {r}");
    let png = std::fs::read(&shot).unwrap();
    let out_dir = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
    std::fs::create_dir_all(&out_dir).unwrap();
    let kept = out_dir.join(format!("{name}.png"));
    std::fs::write(&kept, &png).unwrap();
    let (pw, ph, rgba) = decode_png(&png);
    assert_eq!((pw, ph), (w, h), "{name}");
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(pw, ph, &rgba)) + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e} (bless with DALICA_BLESS=1)", stored.display()));
    let expected = fingerprint::from_json(&text).unwrap();
    if let Err(m) = fingerprint::check(pw, ph, &rgba, &expected, 2.0) {
        panic!("{name}: {} (the screenshot is {})", m.message, kept.display());
    }
    (kept, rgba)
}

/// Pixels of `rect` (points = pixels at 1×) in an RGBA screenshot `w` wide close to the `caution`
/// colour (`#e3a83c`) of the "!".
fn caution_pixels(rgba: &[u8], w: u32, rect: &Value) -> usize {
    let r: Vec<f64> = (0..4).map(|i| rect[i].as_f64().unwrap()).collect();
    let mut n = 0;
    for y in r[1].max(0.0) as u32..(r[1] + r[3]) as u32 {
        for x in r[0].max(0.0) as u32..((r[0] + r[2]) as u32).min(w) {
            let i = ((y * w + x) * 4) as usize;
            if let Some(p) = rgba.get(i..i + 3)
                && p[0] > 180
                && p[1] > 120
                && p[1] < 200
                && p[2] < 110
            {
                n += 1;
            }
        }
    }
    n
}

/// How much the pixels of `rect` vary: their mean distance from the rect's mean colour, per
/// channel. A placeholder (flat grey, a small "!") is near 0; a picture is not.
fn variation(rgba: &[u8], w: u32, rect: &Value) -> f64 {
    let r: Vec<f64> = (0..4).map(|i| rect[i].as_f64().unwrap()).collect();
    let mut px = Vec::new();
    for y in r[1].max(0.0) as u32..(r[1] + r[3]) as u32 {
        for x in r[0].max(0.0) as u32..((r[0] + r[2]) as u32).min(w) {
            let i = ((y * w + x) * 4) as usize;
            if let Some(p) = rgba.get(i..i + 3) {
                px.push([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
            }
        }
    }
    let n = px.len().max(1) as f64;
    let mean: Vec<f64> = (0..3).map(|k| px.iter().map(|p| p[k]).sum::<f64>() / n).collect();
    px.iter().map(|p| (0..3).map(|k| (p[k] - mean[k]).abs()).sum::<f64>() / 3.0).sum::<f64>() / n
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

// ---------------------------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------------------------

/// Part 2: the temp-dir scenarios on one running session.
#[test]
fn the_library_follows_its_folders_while_the_session_runs() {
    let t = folders::the_library_follows_its_folders();
    let line: Vec<String> = t.phases.iter().map(|(n, s)| format!("{n} {s:.2} s")).collect();
    println!("folders while running: {}", line.join(", "));
}

/// Part 2: first run on a fresh library directory, the thumbnails filling in.
#[test]
fn first_run_on_a_fresh_library_fills_in_its_thumbnails() {
    let f = folders::first_run_fills_in();
    println!(
        "first run: {} photos; frames after Continue: first picture {}, the first folder's {}, every one {}; {:.1} s",
        f.photos, f.first, f.first_folder, f.all, f.secs
    );
}

/// Part 3: formats and colour.
#[test]
fn every_format_decodes_at_each_depth_profile_and_orientation_and_damage_never_panics() {
    let started = Instant::now();
    let depths = formats::check_depths();
    println!("decodes compared with their scenes ({}):\n  {}", depths.len(), depths.join("\n  "));
    let profiles = formats::check_profiles();
    println!("profiles ({}):\n  {}", profiles.len(), profiles.join("\n  "));
    let orientations = formats::check_orientations();
    println!("orientation 1–8 (mean difference from upright):\n  {}", orientations.join("\n  "));
    let mixed = formats::check_mixed();
    println!("the mixed-format folder, decoded here: {}", mixed.join(", "));
    let d = formats::check_damage();
    let refused: Vec<String> = d.refused.iter().map(|(why, n)| format!("{n} \"{why}\"")).collect();
    println!(
        "damage: {} damaged or truncated inputs of {} files through every entry point, no panic; {} still decoded, refused: {}",
        d.inputs,
        d.files,
        d.decoded,
        refused.join(", ")
    );
    println!("formats and colour: {:.1} s", secs(started));
}

/// Not a check but a tool for the manual check in `m5.md`: writes, into the new folder
/// `$DALICA_M5_PHOTOS`, the mixed-format folder of part 1 and 5 000 camera-sized JPEGs (1024 px,
/// each with an EXIF thumbnail) in 50 folders, to add on a fresh library and watch fill in.
#[test]
#[ignore = "a tool for m5.md: DALICA_M5_PHOTOS=<new absolute folder> cargo test --release -p dalica-acceptance --test m5 -- --ignored --exact make_the_manual_check_photos"]
fn make_the_manual_check_photos() {
    use dalica_engine::synthetic::{self, Spec};
    let dir = std::env::var_os("DALICA_M5_PHOTOS").map(PathBuf::from).filter(|d| d.is_absolute());
    let dir = dir.unwrap_or_else(|| panic!("set DALICA_M5_PHOTOS to a new absolute folder"));
    assert!(!dir.exists(), "{} exists already: give a new folder", dir.display());
    let started = Instant::now();
    let spec = Spec::new(5000, 50, synthetic::SEED);
    synthetic::write_photos_with(&dir.join("synthetic"), &spec, 1024, Some(160)).unwrap();
    let photos = dir.join("Photos");
    std::fs::rename(synthetic::photos_dir(&dir.join("synthetic")), &photos).unwrap();
    let _ = std::fs::remove_dir_all(dir.join("synthetic"));
    let mixed = dir.join("Mixed formats");
    std::fs::create_dir_all(&mixed).unwrap();
    for f in formats::mixed_folder() {
        std::fs::write(mixed.join(f.name), &f.bytes).unwrap();
    }
    println!(
        "{} JPEGs in {} folders ({}) and the mixed-format folder ({}) written in {:.0} s",
        spec.photos,
        spec.folders,
        photos.display(),
        mixed.display(),
        secs(started)
    );
}

/// Parts 1 and 4 to 7.
#[test]
fn folders_scanning_and_formats() {
    let started = Instant::now();
    let exe = dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let token_file = home.path().join("agent.token");

    // 1. The mixed-format folder, each file a minute after the last (one day, at noon UTC, so the
    //    order and the header's date are the same in every time zone).
    let t = Instant::now();
    let mixed = home.path().join("Pictures").join("Mixed formats");
    std::fs::create_dir_all(&mixed).unwrap();
    let files = formats::mixed_folder();
    for (i, f) in files.iter().enumerate() {
        let path = mixed.join(f.name);
        std::fs::write(&path, &f.bytes).unwrap();
        let at = UNIX_EPOCH + Duration::from_secs(1_780_833_600 + 60 * i as u64);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
    }
    let library = home.path().join("library");
    let (p, read_root) = (port.to_string(), home.path().to_str().unwrap().to_string());
    let args = [
        "--headless",
        "--control",
        &p,
        "--library",
        library.to_str().unwrap(),
        "--control-token-file",
        token_file.to_str().unwrap(),
        "--automation-read-root",
        &read_root,
    ];
    let mut app = App { child: dalica(&exe, home.path(), &args).spawn().unwrap() };
    let token = token(&token_file, &mut app);
    let mut c = Client::connect(port);
    assert_eq!(c.ok("auth", json!({ "token": token }))["app"], "dalica");
    c.settle();
    assert!(c.photos().is_empty(), "a fresh library");
    c.ok("engine.execute", json!({ "command": "folders.add", "params": { "path": mixed } }));
    c.settle_jobs();
    let added = secs(t);

    // Every file is listed, in file-time order, under one folder.
    let photos = c.photos();
    let names: Vec<&str> = photos.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, files.iter().map(|f| f.name).collect::<Vec<_>>(), "every format listed, in order");
    let rows: Vec<String> = c.widgets("tree:folder:").iter().map(|w| w["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(rows, [format!("Mixed formats ({})", files.len())]);
    let jobs = c.ok("jobs.list", json!({}));
    assert!(jobs.as_array().unwrap().iter().any(|j| j["kind"] == "scan" && j["state"] == "done"), "the scan was a job: {jobs}");

    // Each tile: the twelve show their pictures; the three others a "!" with their reason.
    c.park_pointer(900.0);
    let (shot_large, rgba) = screenshot(&mut c, home.path(), "m5-1400x900", 1400, 900);
    let mut reasons = Vec::new();
    for (p, f) in photos.iter().zip(&files) {
        let id = format!("thumb:{}", p["id"]);
        let cell = c.widgets(&id).into_iter().find(|w| w["id"] == id.as_str()).unwrap_or_else(|| panic!("{}: no cell {id}", f.name));
        let (marks, varies) = (caution_pixels(&rgba, 1400, &cell["rect"]), variation(&rgba, 1400, &cell["rect"]));
        println!("  {:<12} caution pixels {marks:>4}, variation {varies:.1}", f.name);
        match f.reason {
            Some(reason) => {
                assert!(marks > 20 && varies < 6.0, "{}: no \"!\" on a placeholder ({marks} caution pixels, variation {varies:.1})", f.name);
                reasons.push(format!("{} \"{reason}\"", f.name));
            }
            None => assert!(varies > 12.0, "{}: its tile shows no picture (variation {varies:.1})", f.name),
        }
    }
    for (p, f) in photos.iter().zip(&files) {
        let tip = c.tooltip(&format!("thumb:{}", p["id"]));
        assert!(tip.contains(f.name), "{}: the tooltip names the file: {tip:?}", f.name);
        match f.reason {
            Some(reason) => assert!(tip.contains(&format!("Cannot show this photo: {reason}")), "{}: {tip:?}", f.name),
            None => assert!(!tip.contains("Cannot show"), "{}: {tip:?}", f.name),
        }
    }
    println!("mixed-format folder: {} files listed, {} shown, \"!\" with reasons: {}", files.len(), files.len() - reasons.len(), reasons.join(", "));

    // 900 × 600: the same folder in the smallest window, scrolled to its end (Ctrl+End), where
    // the three tiles that cannot be shown are.
    c.act("ui.resize", json!({ "width": 900, "height": 600 }));
    c.act("ui.key", json!({ "combo": "Ctrl+End" }));
    c.settle_jobs();
    c.park_pointer(600.0);
    let (shot_small, rgba) = screenshot(&mut c, home.path(), "m5-900x600", 900, 600);
    for (p, f) in photos.iter().zip(&files).filter(|(_, f)| f.reason.is_some()) {
        let id = format!("thumb:{}", p["id"]);
        let cell = c.widgets(&id).into_iter().find(|w| w["id"] == id.as_str()).unwrap_or_else(|| panic!("900 × 600: {} not on screen", f.name));
        assert!(caution_pixels(&rgba, 900, &cell["rect"]) > 20, "900 × 600: {}: no \"!\"", f.name);
    }
    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
    println!("live app: build {build:.1} s, add and show the folder {added:.1} s, all {:.1} s", secs(t));

    // 4. The crate suites (unless the ci test step already ran them).
    let t = Instant::now();
    let suites_ran = dalica_acceptance::suites_ran();
    if !suites_ran {
        crate_suites();
    }
    let suites = secs(t);

    // 5. The first-thumbnails bench, debug: the progressive path checked, its times printed.
    let t = Instant::now();
    let report =
        bench_report(&cargo(&["test", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", "1000", "--scroll", "0", "--json"]));
    let r = row(&report, "first thumbnails 1000");
    let (first, thumbs) = (r["first_ms"].as_f64().unwrap(), r["thumbs_s"].as_f64().unwrap() * 1000.0);
    assert!(r["embedded_shown"].as_u64().unwrap() > 0, "the first screenful came from the embedded thumbnails: {r}");
    assert!(first < thumbs, "the first screenful ({first:.0} ms) before every thumbnail ({thumbs:.0} ms): {r}");
    println!(
        "first_thumbnails (debug, 1 000 JPEGs of {} px): first screenful {first:.0} ms ({} from embedded thumbnails), scan {:.2} s ({:.0} photos/s), \
         every thumbnail {:.2} s; frame p95 {:.2} ms",
        r["side_px"],
        r["embedded_shown"],
        r["scan_s"].as_f64().unwrap(),
        r["scan_photos_per_s"].as_f64().unwrap(),
        thumbs / 1000.0,
        r["frame_p95_ms"].as_f64().unwrap()
    );
    let benches = secs(t);

    // 6. The corpus floors, when the corpus is here and verified.
    let t = Instant::now();
    corpus(suites_ran);
    let corpus_secs = secs(t);

    // 7. The budgets in release, when asked for.
    if std::env::var_os(RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        release_budgets();
        println!("release budgets: {:.1} s", secs(t));
    } else {
        println!("(the release budgets: run with {RELEASE}=1)");
    }

    println!("suites {suites:.1} s, benches {benches:.1} s, corpus {corpus_secs:.1} s");
    println!("Look at the screenshots of the mixed-format folder beside docs/formats.md §1:\n  {}\n  {}", shot_large.display(), shot_small.display());
    println!("M5 acceptance: passed in {:.1} s", secs(started));
}

/// Part 4: the crate suites, as they are.
fn crate_suites() {
    // Formats and colour: every decoder and its fingerprints, the colour stage, the writers.
    let mut image = vec!["-p", "dalica-image"];
    for suite in ["decode", "color", "tiff", "webp_gif_bmp", "jxl", "psd", "raw_tiff", "raw_containers", "heif"] {
        image.extend(["--test", suite]);
    }
    cargo_test(&image);
    cargo_test(&["-p", "dalica-image", "--features", "heif", "--test", "heif"]);
    cargo_test(&["-p", "dalica-scenes", "--test", "formats"]);
    // Folders and scanning, from the disk up.
    cargo_test(&["-p", "dalica-fs", "--test", "dirfd", "--test", "stale"]);
    let mut scan = vec!["-p", "dalica-scan"];
    for suite in ["scan", "skip", "volumes", "watch", "incremental", "rules"] {
        scan.extend(["--test", suite]);
    }
    cargo_test(&scan);
    cargo_test(&["-p", "dalica-library", "--test", "tree"]);
    cargo_test(&["-p", "dalica-store", "--test", "offline", "--test", "stale"]);
    cargo_test(&["-p", "dalica-thumbs", "--test", "upkeep"]);
    let mut engine = vec!["-p", "dalica-engine"];
    for suite in ["progressive_scan", "jobs", "folders", "offline", "watching", "thumb_cache"] {
        engine.extend(["--test", suite]);
    }
    cargo_test(&engine);
    dalica_acceptance::panic_hunt().unwrap();
    let mut ui = vec!["-p", "dalica-ui"];
    for suite in
        ["dialogs", "jobs_control", "folder_manager", "first_run", "activity", "offline_folders", "color_management", "band_autoscroll", "tray_keys"]
    {
        ui.extend(["--test", suite]);
    }
    cargo_test(&ui);
    cargo_test(&["-p", "dalica-ui-kit", "--test", "fields", "--test", "icons"]);
}

/// Part 6: the corpus floors when every published set is here and verified; otherwise it says so.
fn corpus(in_ci: bool) {
    if in_ci {
        println!("corpus: not run inside ci (the CI corpus job, `cargo xtask test-corpus`, is the strict run)");
        return;
    }
    let status = cargo(&["xtask", "corpus"]);
    let sets: Vec<(String, String)> = status
        .lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix("corpus/")?;
            let (set, state) = rest.split_once('/')?;
            Some((set.to_string(), state.trim().to_string()))
        })
        .collect();
    assert!(sets.iter().any(|(s, _)| s == "raw"), "`cargo xtask corpus` listed no sets:\n{status}");
    let unpublished: Vec<&str> = sets.iter().filter(|(_, st)| st == "not published yet").map(|(s, _)| s.as_str()).collect();
    let not_ready: Vec<String> =
        sets.iter().filter(|(_, st)| st != "not published yet" && !st.starts_with("present")).map(|(s, st)| format!("{s}: {st}")).collect();
    if !not_ready.is_empty() {
        println!(
            "corpus: NOT RUN: {} (`cargo xtask corpus --all` fetches and verifies them; the CI corpus job is the strict run)",
            not_ready.join("; ")
        );
    } else {
        let out = cargo(&["test", "--release", "-p", "dalica-image", "--features", "corpus,heif", "--test", "corpus", "--", "--nocapture"]);
        for l in out.lines().filter(|l| l.contains(" measured: ") || l.contains("SKIPPED")) {
            println!("{l}");
        }
        assert!(out.contains("test result: ok."), "{out}");
    }
    if !unpublished.is_empty() {
        println!("corpus: not published yet, so not measured: {}", unpublished.join(", "));
    }
}

/// Part 7: `first_thumbnails` in full, release: the first screenful ≤ 1 s for 1 k and 10 k JPEGs
/// with the full scan's numbers, and the scanned 100 k pass (0 placeholders, owner p95 ≤ 8 ms).
fn release_budgets() {
    let out = cargo(&["bench", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", "1000,10000", "--scroll", "100000", "--json"]);
    let report = bench_report(&out);
    for n in [1000, 10000] {
        let r = row(&report, &format!("first thumbnails {n}"));
        let first = r["first_ms"].as_f64().unwrap();
        println!(
            "first_thumbnails (release, {n} JPEGs of {} px): first screenful {first:.0} ms ({} from embedded thumbnails); scan {:.2} s \
             ({:.0} photos/s); every thumbnail {:.1} s ({:.0} photos/s); frame p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            r["side_px"],
            r["embedded_shown"],
            r["scan_s"].as_f64().unwrap(),
            r["scan_photos_per_s"].as_f64().unwrap(),
            r["thumbs_s"].as_f64().unwrap(),
            r["thumbs_photos_per_s"].as_f64().unwrap(),
            r["frame_p50_ms"].as_f64().unwrap(),
            r["frame_p95_ms"].as_f64().unwrap(),
            r["frame_max_ms"].as_f64().unwrap()
        );
        assert!(first <= FIRST_MS, "{n} JPEGs: the first screenful after {first:.0} ms, over the {FIRST_MS} ms budget");
    }
    let r = row(&report, "scanned scroll 100000");
    let (placeholders, p95) = (r["placeholder_cells"].as_u64().unwrap(), r["frame_p95_ms"].as_f64().unwrap());
    println!(
        "first_thumbnails (release, scanned 100 000 at the top thumb speed): scan with thumbnails {:.1} s ({:.0} photos/s); {} frames, \
         {} covered, {} upscaled, {} sketch, {placeholders} placeholder cells; owner time p50 {:.2} ms, p95 {p95:.2} ms, max {:.2} ms",
        r["scan_s"].as_f64().unwrap(),
        r["scan_photos_per_s"].as_f64().unwrap(),
        r["frames"],
        r["covered_cells"],
        r["upscaled_cells"],
        r["sketch_cells"],
        r["frame_p50_ms"].as_f64().unwrap(),
        r["frame_max_ms"].as_f64().unwrap()
    );
    assert_eq!(placeholders, 0, "no placeholder cell at the top thumb speed");
    assert!(p95 <= FRAME_MS, "the owner's time per frame p95 {p95:.2} ms, over the {FRAME_MS} ms budget");
}
