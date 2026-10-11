//! M3 "Window shell" (`docs/roadmap.md` → M3 → Done when; run by `cargo xtask accept M3`).
//!
//! 1. The real `dalica` binary in `--headless` mode on a fresh demo library, driven over its control
//!    channel as an agent would: every region's rect from the widget registry (`ui.inspect` →
//!    `regions` and the `region:<name>` widgets of `ui.widgets`) and every shell control at
//!    1400 × 900 and 900 × 600, against rects written out here from `ui-layout.md` §2–§7 and §11 and
//!    `design-system.md` §4 and §11.1; the side pane opened, widened and closed, closing itself in a
//!    narrowing window and staying when opened on purpose; the splitter dragged to its limits; F11
//!    and View → Full Screen; the menu bar of `ui.menu.tree` against `menus-and-shortcuts.md` §1
//!    read from the document itself; the longest menu opened at 900 × 600; whole-window screenshots
//!    matched against `fingerprints/m3-*.json`.
//! 2. The crate-level suites, run as they are: the token contrast tests, the widget-gallery and icon
//!    fingerprints (`dalica-ui-kit`), the geometry tests at three sizes with the pane open and closed
//!    and in full screen (`dalica-ui --test layout`), the menu transcription (`--test menus`), the
//!    toolbar, tree/status/side pane and bottom-area suites, the per-platform keymap, and the window
//!    and view state surviving a restart.
//!
//! The manual check (the app at both sizes beside the docs, F11 on a real screen, window state after
//! a restart, the title bar and the macOS menu bar) is in `m3.md`.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written. Headless
//! renders always draw the Windows/Linux title bar (`design-system.md` §11.1), so the expectations
//! and fingerprints are the same on every OS; only `ui.menu.tree` differs on macOS (its application
//! menu), as the protocol says.

#[path = "m3_menus.rs"]
mod menus;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

type Rect = [f64; 4];

// ---------------------------------------------------------------------------------------------
// The app and its control channel (as in m2.rs)
// ---------------------------------------------------------------------------------------------

/// `dalica`, built for this test run (it is another package's binary).
fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// Runs `cargo test` with `args`; panics with its output if it fails. Returns the seconds it took.
fn cargo_test(args: &[&str]) -> f64 {
    let started = Instant::now();
    let out = dalica_acceptance::cargo().arg("test").args(args).output().unwrap();
    assert!(
        out.status.success(),
        "cargo test {}:\n{}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let passed: usize =
        stdout.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo test {}: no test ran:\n{stdout}", args.join(" "));
    let secs = started.elapsed().as_secs_f64();
    println!("cargo test {}: {passed} passed ({secs:.1} s)", args.join(" "));
    secs
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

    fn state(&mut self) -> Value {
        self.ok("ui.state", json!({}))
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
/// the tolerance M2 uses. Returns where the PNG is.
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

// ---------------------------------------------------------------------------------------------
// The geometry the spec gives
// ---------------------------------------------------------------------------------------------

/// egui rounds widget rects to 1/32 point, so a widget placed at a fractional split (0.365·W) may
/// stick out of its region by that much.
const ROUNDING: f64 = 1.0 / 32.0 + 0.001;

fn rect(v: &Value) -> Option<Rect> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let mut r = [0.0; 4];
    for (o, x) in r.iter_mut().zip(a) {
        *o = x.as_f64()?;
    }
    Some(r)
}

fn near(a: Rect, b: Rect) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

fn within(r: Rect, o: Rect) -> bool {
    r[0] >= o[0] - ROUNDING && r[1] >= o[1] - ROUNDING && r[0] + r[2] <= o[0] + o[2] + ROUNDING && r[1] + r[3] <= o[1] + o[3] + ROUNDING
}

fn overlap(a: Rect, b: Rect) -> bool {
    a[0] < b[0] + b[2] - ROUNDING && b[0] < a[0] + a[2] - ROUNDING && a[1] < b[1] + b[3] - ROUNDING && b[1] < a[1] + a[3] - ROUNDING
}

/// The regions of `ui-layout.md` §2 for a `w × h` library window with the tree `tree` wide and a
/// side pane `pane` wide (0: closed), as `design-system.md` §4 sizes them: menu bar 24, toolbar 40,
/// the middle row (tree, 8 pt splitter, light table, 16 pt scroll bar, side pane), status line 20,
/// bottom area 96 split at 0.365·W with the tray box from x 6 to 0.365·W − 56, 80 high (centred in
/// the 96). The tray buttons' column and the right half of the bottom area (controls row over the
/// action buttons) are checked by [`check_bottom_right`].
fn spec_regions(w: f64, h: f64, tree: f64, pane: f64) -> Vec<(&'static str, Option<Rect>)> {
    let top = 24.0 + 40.0;
    let middle = h - top - 20.0 - 96.0;
    let split = 0.365 * w;
    let light = w - tree - 8.0 - 16.0 - pane;
    vec![
        ("menuBar", Some([0.0, 0.0, w, 24.0])),
        ("toolbar", Some([0.0, 24.0, w, 40.0])),
        ("tabStrip", None),
        ("tree", Some([0.0, top, tree, middle])),
        ("splitter", Some([tree, top, 8.0, middle])),
        ("lightTable", Some([tree + 8.0, top, light, middle])),
        ("scrollbar", Some([tree + 8.0 + light, top, 16.0, middle])),
        ("sidePane", (pane > 0.0).then_some([w - pane, top, pane, middle])),
        ("statusLine", Some([0.0, h - 96.0 - 20.0, w, 20.0])),
        ("tray", Some([6.0, h - 88.0, split - 56.0 - 6.0, 80.0])),
        ("editPanel", None),
        ("canvas", None),
        ("filmstrip", None),
        ("captionBar", None),
    ]
}

/// The Hold/Clear/Album column (34 wide, as high as the tray box) between the tray box's right end
/// (0.365·W − 56) and the split; right of the split, the controls row (24 pt controls) over the
/// action buttons (64 × 56) under their 1 pt separator, together the bottom area's 96 from 0.365·W
/// to the right edge.
fn check_bottom_right(regions: &BTreeMap<String, Rect>, w: f64, h: f64, case: &str) {
    let split = 0.365 * w;
    let tb = regions["trayButtons"];
    let ok = tb[0] >= split - 56.0 - 0.01 && tb[0] + tb[2] <= split + 0.01 && near([tb[1], tb[2], tb[3], 0.0], [h - 88.0, 34.0, 80.0, 0.0]);
    assert!(ok, "{case}: trayButtons {tb:?} should be 34 × 80 at y {} between x {} and the split {split}", h - 88.0, split - 56.0);
    let (bc, ab) = (regions["bottomControls"], regions["actionButtons"]);
    let ok = (bc[0] - split).abs() < 0.01
        && (ab[0] - split).abs() < 0.01
        && (bc[2] - (w - split)).abs() < 0.01
        && (ab[2] - (w - split)).abs() < 0.01
        && (bc[1] - (h - 96.0)).abs() < 0.01
        && (ab[1] + ab[3] - h).abs() < 0.01
        && (ab[1] - (bc[1] + bc[3])).abs() <= 1.0 + 0.01
        && bc[3] >= 24.0
        && ab[3] >= 56.0;
    assert!(ok, "{case}: bottomControls {bc:?} over actionButtons {ab:?} should fill x {split}…{w}, y {}…{h}", h - 96.0);
}

/// The menus of the title bar, left to right.
const MENUS: [&str; 8] = ["menu:File", "menu:Edit", "menu:View", "menu:Folder", "menu:Picture", "menu:Create", "menu:Tools", "menu:Help"];
/// The toolbar of `ui-layout.md` §3, left to right, with its sizes (`None`: the search field, whose
/// width follows the window). The age slider is 100 wide (its 20 pt height is the slider's).
const TOOLBAR: [(&str, Option<[f64; 2]>); 12] = [
    ("button:toolbar.import", Some([110.0, 28.0])),
    ("button:toolbar.newAlbum", Some([30.0, 28.0])),
    ("button:tree.flat", Some([30.0, 28.0])),
    ("button:tree.tree", Some([30.0, 28.0])),
    ("button:toolbar.viewOptions", Some([22.0, 28.0])),
    ("button:filter.starred", Some([26.0, 26.0])),
    ("button:filter.faces", Some([26.0, 26.0])),
    ("button:filter.videos", Some([26.0, 26.0])),
    ("button:filter.geotagged", Some([26.0, 26.0])),
    ("slider:age", Some([100.0, 20.0])),
    ("field:search", None),
    ("indicator:activity", Some([28.0, 28.0])),
];
/// The bottom controls row of `ui-layout.md` §6.3–§6.4, left to right, with its sizes.
const CONTROLS_ROW: [(&str, [f64; 2]); 9] = [
    ("button:star", [36.0, 24.0]),
    ("button:rotate.ccw", [36.0, 24.0]),
    ("button:rotate.cw", [36.0, 24.0]),
    ("button:loupe", [24.0, 24.0]),
    ("slider:thumbSize", [128.0, 20.0]),
    ("toggle:pane.people", [32.0, 24.0]),
    ("toggle:pane.places", [32.0, 24.0]),
    ("toggle:pane.tags", [32.0, 24.0]),
    ("toggle:pane.properties", [32.0, 24.0]),
];
const TRAY_BUTTONS: [&str; 3] = ["button:tray.hold", "button:tray.clear", "button:tray.addToAlbum"];
const ACTIONS: [&str; 3] = ["action:export", "action:collage", "action:slideshow"];

/// Reads the window over the control channel and checks every region and every shell control of a
/// `w × h` window with the tree `tree` wide and the side pane `pane` (its protocol name and width)
/// open or not. Returns the regions.
fn check_window(c: &mut Client, w: f64, h: f64, tree: f64, pane: Option<(&str, f64)>, case: &str) -> BTreeMap<String, Rect> {
    let inspect = c.ok("ui.inspect", json!({}));
    assert_eq!((inspect["window"]["width"].as_f64(), inspect["window"]["height"].as_f64()), (Some(w), Some(h)), "{case}: {}", inspect["window"]);
    assert_eq!(inspect["view"], "library", "{case}");
    assert_eq!(inspect["pane"], pane.map_or(Value::Null, |(n, _)| json!(n)), "{case}");

    // The regions, from `ui.inspect` and from the registry's `region:<name>` widgets: the same rects,
    // where the spec puts them; regions not shown are `null` and not registered.
    let mut regions = BTreeMap::new();
    let widgets = c.ok("ui.widgets", json!({}));
    let widgets = widgets.as_array().unwrap();
    let mut by_id: BTreeMap<String, (Rect, bool, String)> = BTreeMap::new();
    for wi in widgets {
        let id = wi["id"].as_str().unwrap().to_string();
        let r = rect(&wi["rect"]).unwrap_or_else(|| panic!("{case}: `{id}` has no rect: {wi}"));
        let label = wi["label"].as_str().unwrap_or_default().to_string();
        assert!(by_id.insert(id.clone(), (r, wi["enabled"] == true, label)).is_none(), "{case}: `{id}` is registered twice");
    }
    let pane_width = pane.map_or(0.0, |(_, pw)| pw);
    for (name, want) in spec_regions(w, h, tree, pane_width) {
        let got = rect(&inspect["regions"][name]);
        let registered = by_id.get(&format!("region:{name}")).map(|(r, _, _)| *r);
        match want {
            Some(want) => {
                let got = got.unwrap_or_else(|| panic!("{case}: `{name}` is not shown: {}", inspect["regions"]));
                assert!(near(got, want), "{case}: `{name}` is {got:?}, the spec says {want:?}");
                assert_eq!(registered, Some(got), "{case}: `region:{name}` in the registry");
            }
            None => assert!(got.is_none() && registered.is_none(), "{case}: `{name}` is shown ({got:?}) but should not be"),
        }
    }
    for name in ["trayButtons", "bottomControls", "actionButtons"] {
        let got = rect(&inspect["regions"][name]).unwrap_or_else(|| panic!("{case}: `{name}` is not shown"));
        assert_eq!(by_id.get(&format!("region:{name}")).map(|(r, _, _)| *r), Some(got), "{case}: `region:{name}` in the registry");
    }
    for (name, r) in inspect["regions"].as_object().unwrap() {
        if let Some(r) = rect(r) {
            regions.insert(name.clone(), r);
        }
    }
    check_bottom_right(&regions, w, h, case);
    let registered_regions = by_id.keys().filter(|id| id.starts_with("region:")).count();
    assert_eq!(registered_regions, regions.len(), "{case}: every shown region is registered, and only those");

    // The controls.
    let get = |id: &str| by_id.get(id).unwrap_or_else(|| panic!("{case}: `{id}` is not registered")).clone();
    let size = |r: Rect| [r[2], r[3]];
    let mut placed: Vec<(String, Rect)> = Vec::new();
    let mut place = |id: &str, region: &str| {
        let r = get(id).0;
        assert!(within(r, regions[region]), "{case}: `{id}` at {r:?} is outside `{region}` {:?}", regions[region]);
        placed.push((id.to_string(), r));
        r
    };

    // The title bar (`design-system.md` §11.1): the menus after the 16 pt app mark, left to right;
    // the free gap that drags the window, holding the title; Minimize, Maximize and Close, 46 × 24,
    // flush in the top-right corner.
    let mut x = 16.0;
    for id in MENUS {
        let m = place(id, "menuBar");
        assert!(m[0] >= x - 0.01, "{case}: `{id}` at {m:?} is left of what comes before it");
        x = m[0] + m[2];
    }
    for (i, id) in ["button:window.minimize", "button:window.maximize", "button:window.close"].into_iter().enumerate() {
        let b = place(id, "menuBar");
        assert!(near(b, [w - 46.0 * (3 - i) as f64, 0.0, 46.0, 24.0]), "{case}: `{id}` at {b:?}");
    }
    let drag = place("titlebar:drag", "menuBar");
    assert!(drag[0] >= x - 0.01 && (drag[0] + drag[2] - (w - 138.0)).abs() < 0.01, "{case}: the drag gap {drag:?}");
    assert_eq!(get("titlebar:drag").2, "Dalica", "{case}: the window title with no folder selected");

    // The toolbar (`ui-layout.md` §3): left to right at its sizes; the filters at least 8 right of
    // the splitter; the search field to W − 64, at least 12 after the age slider and from 0.4·W
    // (its text starts right of the 28 pt magnifier); the "Filters" caption above the toggles
    // (which then rest on the toolbar's bottom rule) from 1100 wide, the toggles centred without it.
    let mut x = 0.0;
    for (id, want) in TOOLBAR {
        let r = place(id, "toolbar");
        if let Some(want) = want {
            assert_eq!(size(r), want, "{case}: `{id}`");
        }
        assert!(r[0] >= x - 0.01, "{case}: `{id}` at {r:?} overlaps the control before it");
        x = r[0] + r[2];
    }
    let starred = get("button:filter.starred").0;
    assert!(starred[0] >= regions["splitter"][0] + 8.0 + 8.0 - 0.01, "{case}: the filters start 8 right of the splitter: {starred:?}");
    let (search, age) = (get("field:search").0, get("slider:age").0);
    assert!(search[0] + search[2] <= w - 64.0 + 0.01, "{case}: the search field ends at W − 64: {search:?}");
    assert!(search[0] >= age[0] + age[2] + 12.0 - 0.01 && search[0] >= 0.4 * w - 28.0 - 0.01, "{case}: the search field starts at {search:?}");
    if w >= 1100.0 {
        assert!((starred[1] + starred[3] - 63.0).abs() <= 1.0, "{case}: with the caption the toggles rest on the bottom rule: {starred:?}");
    } else {
        assert!((starred[1] - (24.0 + (40.0 - 26.0) / 2.0)).abs() < 0.01, "{case}: without the caption the toggles are centred: {starred:?}");
    }

    // The tree (`ui-layout.md` §4): the Folders group header 30 high, rows 24 high, all inside the
    // tree; the splitter widget is the splitter region; thumbnails (144 by default) and the scroll
    // bar's parts in the light table's column; nothing outside the window.
    assert_eq!(place("splitter:tree", "splitter"), regions["splitter"], "{case}");
    assert_eq!(get("tree:group:folders").0[3], 30.0, "{case}: the group header is 30 high");
    let column = [regions["lightTable"][0], regions["lightTable"][1], regions["lightTable"][2] + regions["scrollbar"][2], regions["lightTable"][3]];
    let mut rows = 0;
    for (id, (r, _, _)) in &by_id {
        if id.starts_with("tree:folder:") {
            assert_eq!(r[3], 24.0, "{case}: `{id}` is a 24 pt row");
            rows += 1;
        }
        if id.starts_with("tree:") {
            assert!(within(*r, regions["tree"]), "{case}: `{id}` at {r:?} is outside the tree");
        }
        if id.starts_with("thumb:") || id.starts_with("scroll:") {
            assert!(within(*r, column), "{case}: `{id}` at {r:?} is outside the light table");
        }
        if id.starts_with("thumb:") && !id.contains('/') {
            assert!((r[2] - 144.0).abs() < 0.01, "{case}: `{id}` is a 144 pt cell: {r:?}");
        }
        assert!(within(*r, [0.0, 0.0, w, h]), "{case}: `{id}` at {r:?} is outside the window");
    }
    assert_eq!(rows, 3, "{case}: the demo's three folders are rows of the tree");

    // The side pane (`ui-layout.md` §7): a 30 pt header with « and × (24 × 24) and the pane's body
    // under it; no pane controls while it is closed.
    match pane {
        Some((name, _)) => {
            let p = regions["sidePane"];
            let widen = place("button:pane.widen", "sidePane");
            let close = place("button:pane.close", "sidePane");
            assert!(size(widen) == [24.0, 24.0] && widen[0] < p[0] + 6.0 && widen[1] + widen[3] <= p[1] + 30.0, "{case}: « at {widen:?}");
            assert!(
                size(close) == [24.0, 24.0] && close[0] + close[2] > p[0] + p[2] - 6.0 && close[1] + close[3] <= p[1] + 30.0,
                "{case}: × at {close:?}"
            );
            let body = place(&format!("pane:{name}"), "sidePane");
            assert!(near(body, [p[0], p[1] + 30.0, p[2], p[3] - 30.0]), "{case}: the pane body {body:?}");
        }
        None => assert!(!by_id.keys().any(|id| id.starts_with("pane:") || id.starts_with("button:pane.")), "{case}: no pane, no pane controls"),
    }

    // The status line (`ui-layout.md` §6.1) says what `ui.state` says: "No selection".
    place("text:status", "statusLine");
    assert_eq!(get("text:status").2, inspect["status"].as_str().unwrap_or("?"), "{case}");
    assert_eq!(inspect["status"], "No selection", "{case}");

    // The bottom area (`ui-layout.md` §6.2–§6.5): the tray box; Hold, Clear, Add to Album stacked
    // (34 × 24); the controls row at its sizes with the four pane toggles joined; Export, Collage,
    // Slideshow (64 × 56) from 1100 wide, only More… below.
    assert_eq!(place("tray", "tray"), regions["tray"], "{case}");
    let mut y = regions["trayButtons"][1];
    for id in TRAY_BUTTONS {
        let b = place(id, "trayButtons");
        assert_eq!(size(b), [34.0, 24.0], "{case}: `{id}`");
        assert!(b[1] >= y - 0.01, "{case}: `{id}` is under the one before it");
        y = b[1] + b[3];
    }
    let mut x = regions["bottomControls"][0];
    for (id, want) in CONTROLS_ROW {
        let r = place(id, "bottomControls");
        assert_eq!(size(r), want, "{case}: `{id}`");
        assert!(r[0] >= x - 0.01, "{case}: `{id}` at {r:?} overlaps the control before it");
        x = r[0] + r[2];
    }
    let toggles: Vec<Rect> = CONTROLS_ROW[5..].iter().map(|(id, _)| get(id).0).collect();
    assert!(toggles.windows(2).all(|p| (p[0][0] + p[0][2] - p[1][0]).abs() < 0.01), "{case}: the pane toggles are joined");
    let shown: Vec<&str> = ACTIONS.iter().chain(&["action:more"]).copied().filter(|id| by_id.contains_key(*id)).collect();
    let want: &[&str] = if w >= 1100.0 { &ACTIONS } else { &["action:more"] };
    assert_eq!(shown, want, "{case}: the action row (More… below 1100 wide)");
    for id in shown {
        let r = place(id, "actionButtons");
        assert_eq!(size(r), [64.0, 56.0], "{case}: `{id}`");
    }

    for (i, (a, ra)) in placed.iter().enumerate() {
        for (b, rb) in placed.iter().skip(i + 1) {
            assert!(!overlap(*ra, *rb), "{case}: `{a}` {ra:?} overlaps `{b}` {rb:?}");
        }
    }
    regions
}

/// The tree width `ui-layout.md` §11 gives beside a side pane `pane` wide (0: closed) in a window
/// `w` wide: the user's width while the light table (splitter and scroll bar included) keeps 480,
/// never under 200.
fn tree_beside(requested: f64, w: f64, pane: f64) -> f64 {
    requested.clamp(200.0, (w - pane - 480.0).max(200.0))
}

/// What `menus-and-shortcuts.md` §1 lists today, per layout: 8 menus, 112 items and 7 submenus on
/// Windows and Linux; on macOS Exit, Options… and About Dalica move to the application menu (109).
fn menu_floor(platform: menus::Platform) -> menus::Counts {
    let items = match platform {
        menus::Platform::Mac => 109,
        menus::Platform::Other => 112,
    };
    menus::Counts { menus: 8, items, submenus: 7 }
}

/// Both layouts' expectations come from the document on every OS, so the macOS path runs on Linux
/// and Windows too: each counts exactly its floor.
#[test]
fn the_menu_expectations_of_both_layouts_come_from_the_doc() {
    let doc = std::fs::read_to_string(dalica_acceptance::workspace_root().join("docs/menus-and-shortcuts.md")).unwrap();
    for platform in [menus::Platform::Other, menus::Platform::Mac] {
        let want = menus::for_platform(menus::from_doc(&doc), platform);
        assert_eq!(menus::count(&want), menu_floor(platform), "{platform:?}");
        let titles: Vec<&str> = want.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["File", "Edit", "View", "Folder", "Picture", "Create", "Tools", "Help"], "{platform:?}");
        // The label of a menu's last row ("" when it is not an item).
        let last = |title: &str| match want.iter().find(|m| m.title == title).and_then(|m| m.rows.last()) {
            Some(menus::Node::Item { label, .. }) => label.clone(),
            _ => String::new(),
        };
        let moved = [last("File") == "Exit", last("Tools") == "Options…", last("Help") == "About Dalica"];
        assert_eq!(moved, [platform == menus::Platform::Other; 3], "{platform:?}: Exit, Options… and About Dalica");
    }
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

// ---------------------------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------------------------

#[test]
fn window_shell() {
    let started = Instant::now();
    let doc = std::fs::read_to_string(dalica_acceptance::workspace_root().join("docs/menus-and-shortcuts.md")).unwrap();
    let menus_from_doc = menus::from_doc(&doc);
    // The rows of View (the longest menu) that are not separators, as §1.3 lists them.
    let view_rows = menus_from_doc.iter().find(|m| m.title == "View").map_or(0, |m| m.rows.iter().filter(|r| !matches!(r, menus::Node::Sep)).count());
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
    let auth = c.ok("auth", json!({ "token": token }));
    assert_eq!(auth["app"], "dalica", "{auth}");
    c.settle();
    let ready = secs(t);

    // 2. 1400 × 900: every region and control where the spec puts it; the whole window's fingerprint.
    let t = Instant::now();
    let (w, h) = (1400.0, 900.0);
    check_window(&mut c, w, h, 240.0, None, "1400×900");
    c.park_pointer(h);
    let shot_large = screenshot(&mut c, home.path(), "m3-1400x900", 1400, 900);

    // F11 fills the screen with the same layout (headless, the screen is the window); View → Full
    // Screen leaves it.
    c.act("ui.key", json!({ "key": "F11" }));
    assert_eq!(c.state()["fullScreen"], true, "F11 enters full screen");
    check_window(&mut c, w, h, 240.0, None, "1400×900, full screen");
    c.act("ui.menu.invoke", json!({ "path": "View/Full Screen" }));
    assert_eq!(c.state()["fullScreen"], false, "View → Full Screen leaves full screen");

    // The splitter drags the tree within 200 … W − 480 (`ui-layout.md` §2); the light table gives
    // or takes the difference.
    for (dx, want) in [(60.0, 300.0), (-500.0, 200.0), (2000.0, w - 480.0)] {
        c.act("ui.dragWidget", json!({ "id": "splitter:tree", "dx": dx, "dy": 0.0 }));
        assert_eq!(c.state()["treeWidth"].as_f64(), Some(want), "dragging the splitter by {dx}");
        check_window(&mut c, w, h, want, None, &format!("1400×900, tree {want}"));
    }
    c.act("ui.set", json!({ "treeWidth": 240.0 }));

    // The side pane (`ui-layout.md` §7): its toggle opens it at 280, pushing the light table; «
    // widens it to min(30 % W, 392); × closes it.
    c.act("ui.clickWidget", json!({ "id": "toggle:pane.tags" }));
    check_window(&mut c, w, h, tree_beside(240.0, w, 280.0), Some(("tags", 280.0)), "1400×900, Tags pane");
    c.act("ui.clickWidget", json!({ "id": "button:pane.widen" }));
    assert_eq!(c.state()["paneLarge"], true);
    let large = (0.3 * w).min(392.0);
    check_window(&mut c, w, h, tree_beside(240.0, w, large), Some(("tags", large)), "1400×900, Tags pane large");
    c.act("ui.clickWidget", json!({ "id": "button:pane.close" }));
    check_window(&mut c, w, h, 240.0, None, "1400×900, pane closed");
    let at_1400 = secs(t);

    // 3. 900 × 600 (§11): a pane left open closes itself as the window narrows (the light table would
    //    drop below 480 even with the tree at 200) and stays closed when it widens again; the tree
    //    keeps its 240; the filter caption hides; the action buttons go behind More….
    let t = Instant::now();
    c.act("ui.clickWidget", json!({ "id": "toggle:pane.tags" }));
    assert_eq!(c.state()["pane"], "tags");
    let (w, h) = (900.0, 600.0);
    c.act("ui.resize", json!({ "width": w, "height": h }));
    assert_eq!(c.state()["pane"], Value::Null, "the pane closed itself in the narrow window");
    check_window(&mut c, w, h, 240.0, None, "900×600");
    c.park_pointer(h);
    let shot_small = screenshot(&mut c, home.path(), "m3-900x600", 900, 600);

    // The longest menu, View, opens inside the 900 × 600 window (`design-system.md` §11.2): every row
    // below the menu bar and inside the window, its first and last rows shown; Escape closes it.
    c.act("ui.clickWidget", json!({ "id": "menu:View" }));
    let rows: Vec<Value> = c
        .ok("ui.widgets", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["id"].as_str().is_some_and(|id| id.starts_with("menuitem:") || id.starts_with("submenu:View/")))
        .cloned()
        .collect();
    assert_eq!(rows.len(), view_rows, "every row of the View menu is shown: {rows:?}");
    for r in &rows {
        let rr = rect(&r["rect"]).unwrap();
        assert!(within(rr, [0.0, 24.0, w, h - 24.0]), "`{}` at {rr:?} runs off the 900 × 600 window", r["id"]);
    }
    let ids: Vec<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
    assert!(ids.contains(&"menuitem:view.library") && ids.contains(&"menuitem:window.fullscreen"), "{ids:?}");
    c.park_pointer(h);
    let shot_menu = screenshot(&mut c, home.path(), "m3-900x600-view-menu", 900, 600);
    c.act("ui.key", json!({ "key": "Escape" }));
    assert_eq!(c.ok("ui.widgets", json!({ "filter": "menuitem:" })), json!([]), "Escape closed the menu");

    // A pane opened on purpose in the narrow window stays open; the tree makes room first (200).
    c.act("ui.clickWidget", json!({ "id": "toggle:pane.people" }));
    check_window(&mut c, w, h, 200.0, Some(("people", 280.0)), "900×600, People opened on purpose");
    c.act("ui.clickWidget", json!({ "id": "button:pane.close" }));
    c.act("ui.key", json!({ "key": "F11" }));
    check_window(&mut c, w, h, 240.0, None, "900×600, full screen");
    c.act("ui.key", json!({ "key": "F11" }));
    assert_eq!(c.state()["fullScreen"], false, "F11 again leaves full screen");
    let at_900 = secs(t);

    // 4. The menu bar: every item of `menus-and-shortcuts.md` §1, in order, read from the document.
    let t = Instant::now();
    let tree = c.ok("ui.menu.tree", json!({}));
    let want = menus::for_this_platform(menus_from_doc);
    let counts = menus::compare_bar(&want, &tree)
        .unwrap_or_else(|errors| panic!("`ui.menu.tree` against menus-and-shortcuts.md §1:\n  {}", errors.join("\n  ")));
    // Exactly what the document lists for this platform's layout, and never less than it lists
    // today (so an empty or truncated parse cannot pass).
    assert_eq!(counts, menus::count(&want), "`ui.menu.tree` shows exactly the rows §1 lists for {:?}", menus::Platform::current());
    let floor = menu_floor(menus::Platform::current());
    assert!(counts.menus >= floor.menus && counts.items >= floor.items && counts.submenus >= floor.submenus, "{counts:?} < {floor:?}");
    let menu_check = secs(t);

    // 5. The agent quits the app; the fresh demo goes with it.
    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
    println!(
        "live app: build {build:.1} s, ready {ready:.1} s, 1400×900 {at_1400:.1} s, 900×600 {at_900:.1} s, menus {menu_check:.2} s \
         ({} menus, {} items, {} submenus as §1 lists them)",
        counts.menus, counts.items, counts.submenus
    );

    // 6. The crate-level suites, as they are.
    let t = Instant::now();
    // Token contrast (`design-system.md` §9), the widget gallery in every state and the icon sheet
    // against their fingerprints.
    cargo_test(&["-p", "dalica-ui-kit", "--test", "contrast", "--test", "widgets", "--test", "gallery", "--test", "icons"]);
    // Geometry at 1400 × 900, 1100 × 700 and 900 × 600, pane open/closed/large, full screen; the
    // §1 transcription with submenus, invoke, scrolling menus; toolbar, tree/status/pane, bottom area.
    cargo_test(&["-p", "dalica-ui", "--test", "layout", "--test", "menus", "--test", "toolbar", "--test", "tree_status_pane", "--test", "bottom"]);
    // Per-platform default keys without conflicts on any OS; window and view state survive a restart.
    cargo_test(&["-p", "dalica-engine", "--test", "keymap"]);
    cargo_test(&["-p", "dalica-engine", "--lib", "window_and_view_state_survive_a_write_and_a_read"]);
    cargo_test(&["-p", "dalica-ui", "--lib", "the_view_state_survives_a_restart"]);
    let suites = secs(t);

    println!("suites: {suites:.1} s");
    println!(
        "Look at the screenshots beside docs/ui-layout.md §2 and §11:\n  {}\n  {}\n  {}",
        shot_large.display(),
        shot_small.display(),
        shot_menu.display()
    );
    println!("M3 acceptance: passed in {:.1} s", started.elapsed().as_secs_f64());
}
