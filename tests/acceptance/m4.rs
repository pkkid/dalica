//! M4 "Library browsing" (`docs/roadmap.md` → M4 → the *Done when* of Light table, Library tree,
//! and Selection and the Photo Tray; run by `cargo xtask accept M4`).
//!
//! 1. **The live app.** The real `dalica` binary in `--headless` mode on a fresh demo library (three
//!    folders of twenty photos), driven over its control channel as an agent would, at 1400 × 900:
//!    the tree's rows are the light table's sections in the same order; a row click leaves that
//!    section's header at the top and selects the folder as a unit (one tray token, its status
//!    line); collapsing Folders removes its sections and expanding brings them back; the arrow keys,
//!    Home and End stop at a section's edges; select 2 photos in one folder, Hold (Ctrl+H), select 3
//!    in another → the tray has 5 → the bottom Star button stars all 5 in one undo step → Ctrl+Z
//!    restores all 5; the status line reads each case of `ui-layout.md` §6.1 (none, one photo,
//!    several, a folder). Whole-window screenshots at 1400 × 900 and 900 × 600 are matched against
//!    `fingerprints/m4-*.json`.
//! 2. **The thumb at full speed through 100 k photos** (`m4_thumb.rs`, its own test in this binary):
//!    no placeholder on any frame of an index-only 100 k library, every cell its sketch.
//! 3. **The crate suites, run as they are:** layout (justification, sticky push-up, offsets, cells
//!    visited at 10 k and 100 k, `section.scrollTo`), the section header, cells and badges, the
//!    scroll bar on virtual time (speed curve, dead zone, spring-back, ⇑ / ⇓ section jumps), never
//!    blank (a scanned 10 k library at the top speed), selection and keyboard navigation, the tree
//!    pane (rows and order in both modes and every sort, row click, collapse, 5 k folders within the
//!    budget), the tray, the status line, the view state; the tree model, the sketch, the index,
//!    the pack cache, the ladder and the synthetic libraries below the UI. Inside `cargo xtask ci`
//!    its test step already ran them, so they are skipped there (`dalica_acceptance::SUITES_RAN`);
//!    a standalone `accept M4` runs them.
//! 4. **The benchmarks in a debug build:** `library_bench` scrolls the 10 k and 100 k synthetic
//!    libraries and the cells visited per frame must be flat (the frame times are printed); the
//!    pack bench's smoke pass prints packed against loose reads.
//! 5. **With `DALICA_ACCEPT_RELEASE=1`, the budgets in a release build:** `library_bench` in full
//!    (p95 of the 100 k scroll ≤ 8 ms, cells flat), the thumb test of part 2 (p95 ≤ 8 ms) and the
//!    pack bench at 100 000 entries. A debug time limit would only measure the machine, so
//!    `cargo xtask ci` (which runs this test in debug) checks the counters and prints the times.
//!
//! The manual check (scroll the demo library with the thumb and tune its curve) is in `m4.md`.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written. Headless
//! renders always draw the Windows/Linux title bar, so the fingerprints are the same on every OS.

#[path = "m4_thumb.rs"]
mod thumb;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// The environment variable that adds the release-build budgets (part 5).
const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

// ---------------------------------------------------------------------------------------------
// Cargo, the app and its control channel (as in m3.rs)
// ---------------------------------------------------------------------------------------------

/// `dalica`, built for this test run (it is another package's binary).
fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output. The
/// suites never see `DALICA_BLESS` (`dalica_acceptance::cargo`): blessing this test's screenshots
/// must not re-bless theirs.
fn cargo(args: &[&str]) -> String {
    let started = Instant::now();
    let out = dalica_acceptance::cargo().args(args).output().unwrap();
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
        out.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
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

    fn settle(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 30000 }));
        assert_eq!(s["settled"], true, "{s}");
    }

    /// A request that changes what is shown, then a settle so the next read sees its frame.
    fn act(&mut self, method: &str, params: Value) -> Value {
        let r = self.ok(method, params);
        self.settle();
        r
    }

    fn click(&mut self, id: &str) {
        self.act("ui.clickWidget", json!({ "id": id }));
    }

    fn key(&mut self, combo: &str) {
        self.act("ui.key", json!({ "combo": combo }));
    }

    fn state(&mut self) -> Value {
        self.ok("ui.state", json!({}))
    }

    fn status(&mut self) -> String {
        self.state()["status"].as_str().unwrap().to_string()
    }

    /// The library's photos in display order (`library.query`: id, folder, name, star, …).
    fn photos(&mut self) -> Vec<Value> {
        let r = self.ok("engine.execute", json!({ "command": "library.query", "params": {} }));
        r["photos"].as_array().unwrap().clone()
    }

    /// The light table's section header ids drawn now, top to bottom.
    fn headers(&mut self) -> Vec<String> {
        let mut h: Vec<(f64, String)> = self
            .widgets("section:")
            .iter()
            .filter_map(|w| {
                let id = w["id"].as_str()?;
                (!id.contains('/')).then(|| (w["rect"][1].as_f64().unwrap(), id.to_string()))
            })
            .collect();
        h.sort_by(|a, b| a.0.total_cmp(&b.0));
        h.into_iter().map(|(_, id)| id).collect()
    }

    /// The selected photo ids.
    fn selected(&mut self) -> Vec<u64> {
        self.state()["selection"]["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
    }

    /// The widgets whose id contains `filter`.
    fn widgets(&mut self, filter: &str) -> Vec<Value> {
        self.ok("ui.widgets", json!({ "filter": filter })).as_array().unwrap().clone()
    }

    /// Rests the pointer on the status line's left end (no widget there), so screenshots show no
    /// hover highlight left over from the last click.
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

/// Takes a whole-window screenshot of size `w × h`, keeps it in `target/test-output/acceptance/`,
/// and matches it against `fingerprints/<name>.json` (written first under `DALICA_BLESS=1`), with
/// the tolerance M2 and M3 use. Returns where the PNG is.
fn screenshot(c: &mut Client, home: &Path, name: &str, w: u32, h: u32) -> PathBuf {
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
    kept
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

// ---------------------------------------------------------------------------------------------
// The library, the tray and the status line as an agent reads them
// ---------------------------------------------------------------------------------------------

/// The photo ids of folder `folder` (as `library.query` lists them).
fn in_folder(photos: &[Value], folder: u64) -> Vec<u64> {
    photos.iter().filter(|p| p["folder"] == folder).map(|p| p["id"].as_u64().unwrap()).collect()
}

fn starred(photos: &[Value]) -> Vec<u64> {
    photos.iter().filter(|p| p["star"] == true).map(|p| p["id"].as_u64().unwrap()).collect()
}

fn sorted(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v
}

fn ids(v: &Value) -> Vec<u64> {
    sorted(v.as_array().unwrap_or_else(|| panic!("not a list: {v}")).iter().map(|v| v.as_u64().unwrap()).collect())
}

/// The tray's minis (`tray:item:<id>`, without their held badges) and the held badges' photos.
fn tray(c: &mut Client) -> (Vec<u64>, Vec<u64>) {
    let (mut minis, mut held) = (Vec::new(), Vec::new());
    for w in c.widgets("tray:item:") {
        let id = w["id"].as_str().unwrap().strip_prefix("tray:item:").unwrap();
        match id.strip_suffix("/held") {
            Some(h) => held.push(h.parse().unwrap()),
            None => minis.push(id.parse().unwrap()),
        }
    }
    (sorted(minis), sorted(held))
}

/// "N pictures     <date or range>     <size> on disk" (`ui-layout.md` §6.1; the demo has no tags).
fn assert_several(status: &str, n: usize) {
    let fields: Vec<&str> = status.split("     ").collect();
    assert_eq!(fields.len(), 3, "{status:?}");
    assert_eq!(fields[0], format!("{n} pictures"), "{status:?}");
    assert!(!fields[1].is_empty() && !fields[1].contains("  "), "a date or a date range: {status:?}");
    assert!(fields[2].ends_with(" MB on disk") || fields[2].ends_with(" KB on disk"), "{status:?}");
}

// ---------------------------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------------------------

/// Part 2: the speed thumb held at its top speed through an index-only 100 k library.
#[test]
fn the_thumb_held_at_top_speed_through_100k_sketched_photos_never_shows_a_placeholder() {
    let started = Instant::now();
    let r = thumb::hold_through_100k();
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    println!(
        "thumb at top speed ({mode}): {} photos, {:.0} screens in {} frames, {} sketched cells, 0 placeholders; cells visited \
         at most {} (bound {}); UI time p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; {:.1} s",
        r.photos,
        r.screens,
        r.frames,
        r.sketch_cells,
        r.cells_max,
        r.cells_bound,
        r.p50_ms,
        r.p95_ms,
        r.max_ms,
        secs(started)
    );
    if !cfg!(debug_assertions) {
        assert!(r.p95_ms <= thumb::BUDGET_MS, "UI time p95 {:.2} ms over the {} ms budget", r.p95_ms, thumb::BUDGET_MS);
    }
}

/// Not a check but a tool for the manual check in `m4.md`: writes a scanned synthetic library of
/// 20 000 photos in 200 folders (small JPEGs, sketches in the index, 128 and 256 thumbnails in
/// `<dir>/cache/thumbs`) into the new folder `$DALICA_M4_LIBRARY`, big enough to feel the speed
/// thumb, and prints how to open it.
#[test]
#[ignore = "a tool for m4.md: DALICA_M4_LIBRARY=<new absolute folder> cargo test --release -p dalica-acceptance --test m4 -- --ignored --exact make_the_large_demo_library"]
fn make_the_large_demo_library() {
    use dalica_engine::synthetic::{self, Spec};
    let dir = std::env::var_os("DALICA_M4_LIBRARY").map(PathBuf::from).filter(|d| d.is_absolute());
    let dir = dir.unwrap_or_else(|| panic!("set DALICA_M4_LIBRARY to a new absolute folder"));
    let started = Instant::now();
    let cache = dir.join("cache");
    dalica_engine::thumbs::set_process_cache_dir(Some(cache.join("thumbs")));
    let spec = Spec::new(synthetic::MAX_SCANNED_PHOTOS, synthetic::MAX_SCANNED_PHOTOS / 100, synthetic::SEED);
    let session = synthetic::open_scanned(&dir, &spec, synthetic::DEFAULT_FILE_SIDE, std::sync::Arc::new(dalica_core::SystemClock)).unwrap();
    let stats = session.thumbnail_stats();
    assert_eq!((stats.scanned, stats.failed), (spec.photos, 0), "{stats:?}");
    drop(session);
    println!(
        "{} photos in {} folders written and scanned in {:.0} s. Open them with\n\n  DALICA_CACHE_DIR={} cargo run --release -p dalica -- --library {}\n",
        spec.photos,
        spec.folders,
        secs(started),
        cache.display(),
        synthetic::library_dir(&dir).display()
    );
}

/// Parts 1, 3, 4 and 5.
#[test]
fn library_browsing() {
    let started = Instant::now();
    let exe = dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let token_file = home.path().join("agent.token");

    // 1. A headless instance with its control channel on a fresh demo library (1400 × 900).
    let t = Instant::now();
    let p = port.to_string();
    let args = ["--headless", "--control", &p, "--demo-fresh", "--control-token-file", token_file.to_str().unwrap()];
    let mut app = App { child: dalica(&exe, home.path(), &args).spawn().unwrap() };
    let token = token(&token_file, &mut app);
    let mut c = Client::connect(port);
    assert_eq!(c.ok("auth", json!({ "token": token }))["app"], "dalica");
    c.settle();
    assert_eq!(c.status(), "No selection");

    // The tree: the demo's folders are numbered in tree order (`control-protocol.md` §5.4), the
    // Folders group's rows list them top to bottom, and the light table's sections follow the
    // same order, each with the folder's photos (numbered in light-table order).
    let photos = c.photos();
    assert_eq!(photos.len(), 60, "the demo's sixty photos");
    let mut rows: Vec<(f64, String)> =
        c.widgets("tree:folder:").iter().map(|w| (w["rect"][1].as_f64().unwrap(), w["id"].as_str().unwrap().to_string())).collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rows: Vec<String> = rows.into_iter().map(|(_, id)| id).collect();
    assert_eq!(rows, ["tree:folder:1", "tree:folder:2", "tree:folder:3"], "the tree's rows in order");
    assert_eq!(c.headers()[..2], ["section:folder:1", "section:folder:2"], "the sections in the tree's order");
    let (a, b) = (in_folder(&photos, 1), in_folder(&photos, 2));
    assert_eq!(a, (1..=20).collect::<Vec<u64>>(), "folder 1's photos, first in the light table");
    assert_eq!(b, (21..=40).collect::<Vec<u64>>(), "folder 2's photos, next");
    let first: Vec<u64> = c.widgets("thumb:").iter().filter_map(|w| w["id"].as_str()?.strip_prefix("thumb:")?.parse().ok()).take(20).collect();
    assert_eq!(first, a, "folder 1's cells in light-table order");

    // A row click: the section's header at the top of the light table (after the 0.2 s ease) and
    // the folder selected as a unit, one token in the tray.
    c.click("tree:folder:2");
    let ui = c.ok("ui.inspect", json!({}));
    assert_eq!(ui["scroll"], json!({ "section": "folder:2", "offset": 0.0 }), "the scroll anchor: folder 2's top");
    let header = c.widgets("section:folder:2").into_iter().find(|w| w["id"] == "section:folder:2").unwrap();
    assert_eq!(header["rect"][1], ui["regions"]["lightTable"][1], "the header's top is the light table's top");
    let state = c.state();
    assert_eq!((&state["selection"]["section"], &state["selection"]["whole"]), (&json!("folder:2"), &json!(true)));
    assert_eq!(c.selected(), b, "the folder's photos");
    assert_eq!(c.widgets("tray:token").len(), 1, "one token for the folder");
    assert_eq!(tray(&mut c).0, Vec::<u64>::new(), "no minis beside the token");
    assert_several(&c.status(), 20);

    // Collapsing Folders removes its sections; expanding brings them back.
    c.click("tree:group:folders");
    assert_eq!(c.state()["collapsedGroups"], json!(["folders"]));
    assert!(c.widgets("thumb:").is_empty() && c.headers().is_empty(), "no sections while Folders is collapsed");
    c.click("tree:group:folders");
    assert_eq!(c.state()["collapsedGroups"], json!([]));
    assert!(!c.headers().is_empty());

    // Keyboard: back at the top, the arrows, Home and End stop at the first section's edges.
    c.key("Ctrl+Home");
    assert_eq!(c.headers()[0], "section:folder:1");
    c.click(&format!("thumb:{}", a[19]));
    assert_eq!(c.selected(), vec![a[19]]);
    // One photo: its name first.
    let name = photos.iter().find(|p| p["id"] == a[19]).unwrap()["name"].as_str().unwrap().to_string();
    let status = c.status();
    assert!(status.starts_with(&format!("{name}     ")) && status.split("     ").count() == 4, "{status:?}");
    for k in ["Right", "Down"] {
        c.key(k);
        assert_eq!(c.selected(), vec![a[19]], "{k} stops at the section's last photo");
    }
    c.key("Home");
    assert_eq!(c.selected(), vec![a[0]]);
    for k in ["Left", "Up"] {
        c.key(k);
        assert_eq!(c.selected(), vec![a[0]], "{k} stops at the section's first photo");
    }
    c.key("End");
    assert_eq!(c.selected(), vec![a[19]]);
    c.key("Ctrl+D");
    assert_eq!(c.status(), "No selection");
    let browse = secs(t);

    // 2. The tray: 2 in folder A, Hold, 3 in folder B → 5 in the tray; Star stars all 5 in one undo
    //    step; one Ctrl+Z restores all 5.
    let t = Instant::now();
    c.click(&format!("thumb:{}", a[1]));
    c.act("ui.clickWidget", json!({ "id": format!("thumb:{}", a[3]), "ctrl": true }));
    assert_eq!(c.selected(), vec![a[1], a[3]]);
    assert_several(&c.status(), 2);
    c.key("Ctrl+H");
    assert_eq!(ids(&c.state()["tray"]["held"]), vec![a[1], a[3]]);
    c.click("tree:folder:2");
    c.click(&format!("thumb:{}", b[0]));
    c.act("ui.clickWidget", json!({ "id": format!("thumb:{}", b[2]), "shift": true }));
    assert_eq!(c.selected(), b[0..3].to_vec());
    let five = sorted(vec![a[1], a[3], b[0], b[1], b[2]]);
    assert_eq!(tray(&mut c), (five.clone(), vec![a[1], a[3]]), "the tray: 5 minis, 2 of them held");
    assert_several(&c.status(), 5);
    assert_eq!(starred(&c.photos()), Vec::<u64>::new());
    c.park_pointer(900.0);
    let shot_large = screenshot(&mut c, home.path(), "m4-1400x900", 1400, 900);

    c.click("button:star");
    assert_eq!(starred(&c.photos()), five, "the tray's 5 photos starred");
    c.key("Ctrl+Z");
    assert_eq!(starred(&c.photos()), Vec::<u64>::new(), "one undo restored all 5");
    assert_eq!(tray(&mut c), (five.clone(), vec![a[1], a[3]]), "the tray is unchanged");
    let tray_secs = secs(t);

    // 3. 900 × 600: the same state in the smallest window.
    c.act("ui.resize", json!({ "width": 900, "height": 600 }));
    assert_several(&c.status(), 5);
    c.park_pointer(600.0);
    let shot_small = screenshot(&mut c, home.path(), "m4-900x600", 900, 600);

    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
    println!("live app: build {build:.1} s, browsing {browse:.1} s, tray {tray_secs:.1} s");

    // 4. The crate suites, as they are (unless the ci test step already ran them:
    //    `dalica_acceptance::SUITES_RAN`).
    let t = Instant::now();
    if !dalica_acceptance::suites_ran() {
        crate_suites();
    }
    let suites = secs(t);

    // 5. The benchmarks, debug: the counters are checked, the times printed.
    let t = Instant::now();
    let args = ["test", "-p", "dalica-ui", "--bench", "library_bench", "--", "--photos", "100000", "--frames", "30", "--json"];
    check_library_bench(&bench_report(&cargo(&args)), None);
    print_pack(&bench_report(&cargo(&["test", "-p", "dalica-thumbs", "--bench", "pack", "--", "--json"])));
    let benches = secs(t);

    // 6. The budgets in release, when asked for.
    if std::env::var_os(RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        let report = bench_report(&cargo(&["bench", "-p", "dalica-ui", "--bench", "library_bench", "--", "--json"]));
        check_library_bench(&report, Some(thumb::BUDGET_MS));
        let out = cargo(&[
            "test",
            "--release",
            "-p",
            "dalica-acceptance",
            "--test",
            "m4",
            "--",
            "--exact",
            "the_thumb_held_at_top_speed_through_100k_sketched_photos_never_shows_a_placeholder",
            "--nocapture",
        ]);
        let line = out.lines().find(|l| l.starts_with("thumb at top speed")).unwrap_or_else(|| panic!("{out}"));
        println!("{line}");
        assert!(out.contains("test result: ok. 1 passed"), "{out}");
        print_pack(&bench_report(&cargo(&["bench", "-p", "dalica-thumbs", "--bench", "pack", "--", "--json"])));
        println!("release budgets: {:.1} s", secs(t));
    } else {
        println!("(the release budgets: run with {RELEASE}=1)");
    }

    println!("suites {suites:.1} s, benches {benches:.1} s");
    println!("Look at the screenshots beside docs/ui-layout.md §4–§6:\n  {}\n  {}", shot_large.display(), shot_small.display());
    println!("M4 acceptance: passed in {:.1} s", secs(started));
}

/// Part 4: the crate suites, as they are.
fn crate_suites() {
    // Light table (layout, sticky push-up, offsets, cells visited at 10 k and 100 k,
    // section.scrollTo, Ctrl+wheel), header, cells, scroll bar, never blank, selection, tree pane,
    // tray, status line, view state.
    let mut ui = vec!["-p", "dalica-ui"];
    for suite in ["light_table", "section_header", "cells", "scrollbar", "never_blank", "selection", "tree", "tray", "status_line", "view_state"] {
        ui.extend(["--test", suite]);
    }
    cargo_test(&ui);
    // Badges; the tree model; the sketch in thumbs, the library and the index; the pack cache and
    // its fs support; the ladder and scan-time thumbnails; the synthetic libraries.
    cargo_test(&["-p", "dalica-ui-kit", "--test", "badges"]);
    cargo_test(&["-p", "dalica-library", "--test", "tree", "--test", "sketch"]);
    cargo_test(&["-p", "dalica-thumbs", "--test", "sketch", "--test", "pack", "--test", "ladder"]);
    cargo_test(&["-p", "dalica-store", "--test", "sketch"]);
    cargo_test(&["-p", "dalica-fs", "--test", "pack"]);
    cargo_test(&["-p", "dalica-engine", "--test", "synthetic", "--test", "thumbs"]);
}

/// `library_bench`'s report: the cells visited per frame are flat from 10 k to 100 k (the bench's
/// own verdict, and its counters read here); with `budget`, the 100 k scroll's p95 is within it.
fn check_library_bench(report: &Value, budget: Option<f64>) {
    assert_eq!(report["cells_flat"], true, "{report}");
    let (small, big) = (row(report, "scroll light table 10k"), row(report, "scroll light table 100k"));
    let max = |r: &Value| r["counters"]["cells_visited_max"].as_u64().unwrap();
    assert!(max(small) > 0 && max(big) <= max(small) + max(small) / 10, "cells visited: {} at 10 k, {} at 100 k", max(small), max(big));
    let p95 = big["p95_ms"].as_f64().unwrap();
    println!(
        "library_bench ({}): 100 k scroll p50 {:.2} ms, p95 {p95:.2} ms, max {:.2} ms over {} frames; cells visited per frame \
         (min / p50 / max) 10 k {} / {} / {}, 100 k {} / {} / {}",
        report["mode"].as_str().unwrap_or("?"),
        big["p50_ms"].as_f64().unwrap(),
        big["max_ms"].as_f64().unwrap(),
        big["samples"],
        small["counters"]["cells_visited_min"],
        small["counters"]["cells_visited_p50"],
        max(small),
        big["counters"]["cells_visited_min"],
        big["counters"]["cells_visited_p50"],
        max(big),
    );
    if let Some(budget) = budget {
        assert!(p95 <= budget, "the 100 k scroll's p95 is {p95:.2} ms, over the {budget} ms budget");
    }
}

/// The pack bench's report: packed against loose reads, per read.
fn print_pack(report: &Value) {
    let entries = report["entries"].as_u64().unwrap();
    for r in report["rows"].as_array().unwrap() {
        println!(
            "pack ({entries} entries): {}: packed {:.3} µs, loose {:.3} µs per read",
            r["name"].as_str().unwrap(),
            r["pack_us_per_read"].as_f64().unwrap(),
            r["loose_us_per_read"].as_f64().unwrap()
        );
    }
}
