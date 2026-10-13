//! The real `dalica` binary in `--headless` mode, driven over its control channel as an agent would
//! (as in `m5.rs`): the process, the connection, whole-window screenshots matched against
//! `fingerprints/<name>.json`, and pixel helpers.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written. Headless
//! renders always draw the Windows/Linux title bar, so the fingerprints are the same on every OS.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// `dalica`, built for this test run (it is another package's binary).
pub fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// A port nobody listens on now (the OS picks it; it is free again once the probe closes).
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A `dalica` process, killed if the test ends without quitting it.
pub struct App {
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

/// Starts `dalica --headless` on the library `home/library` (created when missing), with
/// automation reading inside `home`, and connects to it.
pub fn start(exe: &Path, home: &Path) -> (App, Client) {
    let port = free_port();
    let token_file = home.join("agent.token");
    let _ = std::fs::remove_file(&token_file);
    let library = home.join("library");
    let (p, read_root) = (port.to_string(), home.to_str().unwrap().to_string());
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
    let mut app = App { child: dalica(exe, home, &args).spawn().unwrap() };
    let token = token(&token_file, &mut app);
    let mut c = Client::connect(port);
    assert_eq!(c.ok("auth", json!({ "token": token }))["app"], "dalica");
    c.settle();
    (app, c)
}

/// Quits the app through the control channel (changes are written first) and waits for it.
pub fn quit(mut app: App, mut c: Client) {
    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
}

/// A control connection, used the way an agent would.
pub struct Client {
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
    pub fn send(&mut self, method: &str, params: Value) -> Value {
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
    pub fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.send(method, params.clone());
        assert_eq!(v["ok"], true, "{method} {params}: {v}");
        v["result"].clone()
    }

    /// Frames until nothing is pending, long jobs (the scan, thumbnail generation) included.
    pub fn settle_jobs(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 60000, "jobs": true }));
        assert_eq!(s["settled"], true, "{s}");
    }

    pub fn settle(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 30000 }));
        assert_eq!(s["settled"], true, "{s}");
    }

    /// An engine (or UI) command that must succeed, then a settle; its result.
    pub fn run(&mut self, command: &str, params: Value) -> Value {
        let r = self.ok("engine.execute", json!({ "command": command, "params": params }));
        self.settle();
        r
    }

    /// An engine (or UI) command that must fail; the reply (`code`, `error`).
    pub fn refused(&mut self, command: &str, params: Value) -> Value {
        let v = self.send("engine.execute", json!({ "command": command, "params": params.clone() }));
        assert_eq!(v["ok"], false, "{command} {params} should be refused: {v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{command}: a refusal says why: {v}");
        v
    }

    /// The library's photos in display order (`library.query`).
    pub fn photos(&mut self) -> Vec<Value> {
        let r = self.ok("engine.execute", json!({ "command": "library.query", "params": {} }));
        r["photos"].as_array().unwrap().clone()
    }

    /// `photo.inspect` of one photo.
    pub fn inspect(&mut self, id: &Value) -> Value {
        let r = self.ok("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [id] } }));
        r.as_array().and_then(|a| a.first()).cloned().unwrap_or_else(|| panic!("photo.inspect {id}: {r}"))
    }

    pub fn widgets(&mut self, filter: &str) -> Vec<Value> {
        self.ok("ui.widgets", json!({ "filter": filter })).as_array().unwrap().clone()
    }

    /// Whether a widget with exactly this id is on screen.
    pub fn has(&mut self, id: &str) -> bool {
        self.widgets(id).iter().any(|w| w["id"] == id)
    }

    /// The rect of the widget with exactly this id.
    pub fn rect(&mut self, id: &str) -> [f64; 4] {
        let w = self.widgets(id).into_iter().find(|w| w["id"] == id).unwrap_or_else(|| panic!("no widget {id}"));
        let r: Vec<f64> = (0..4).map(|i| w["rect"][i].as_f64().unwrap()).collect();
        [r[0], r[1], r[2], r[3]]
    }

    /// Rests the pointer on the status line's left end (no widget there), so screenshots show no
    /// hover highlight or tooltip.
    pub fn park_pointer(&mut self, h: f64) {
        self.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": 2.0, "y": h - 106.0 }] }));
        self.settle();
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

/// A whole-window screenshot of size `w × h`.
pub struct Shot {
    pub path: PathBuf,
    pub width: u32,
    pub rgba: Vec<u8>,
}

impl Shot {
    /// The pixels of `rect` (points = pixels at 1×), RGB.
    pub fn region(&self, rect: [f64; 4]) -> (u32, u32, Vec<u8>) {
        let (x0, y0) = (rect[0].round().max(0.0) as u32, rect[1].round().max(0.0) as u32);
        let (w, h) = (rect[2].round() as u32, rect[3].round() as u32);
        let mut out = Vec::with_capacity((w * h * 3) as usize);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let i = ((y * self.width + x) * 4) as usize;
                out.extend_from_slice(self.rgba.get(i..i + 3).unwrap_or(&[0, 0, 0]));
            }
        }
        (w, h, out)
    }
}

impl Shot {
    /// The rect of the picture letterboxed in the cell `cell`: the pixels that differ from the
    /// cell's background (its top-left pixel) by more than a few levels.
    pub fn picture(&self, cell: [f64; 4]) -> [f64; 4] {
        let (w, h, rgb) = self.region(cell);
        let bg = [rgb[0], rgb[1], rgb[2]];
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                if (0..3).any(|k| rgb[i + k].abs_diff(bg[k]) > 12) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        assert!(x1 > x0 && y1 > y0, "no picture in the cell {cell:?}");
        [cell[0].round() + f64::from(x0), cell[1].round() + f64::from(y0), f64::from(x1 - x0), f64::from(y1 - y0)]
    }
}

/// Mean absolute difference per channel of two RGB buffers of the same size.
pub fn difference(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "regions of different sizes");
    let sum: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    sum as f64 / a.len().max(1) as f64
}

/// How much the pixels vary: their mean distance from the mean colour, per channel. A placeholder
/// or an empty area is near 0; a picture is not.
pub fn variation(rgb: &[u8]) -> f64 {
    let px: Vec<[f64; 3]> = rgb.as_chunks::<3>().0.iter().map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]).collect();
    let n = px.len().max(1) as f64;
    let mean: Vec<f64> = (0..3).map(|k| px.iter().map(|p| p[k]).sum::<f64>() / n).collect();
    px.iter().map(|p| (0..3).map(|k| (p[k] - mean[k]).abs()).sum::<f64>() / 3.0).sum::<f64>() / n
}

/// A whole-window screenshot of size `w × h`, kept in `target/test-output/acceptance/<label>.png`
/// and, when `name` is given, matched against `fingerprints/<name>.json` (written first under
/// `DALICA_BLESS=1` when `label` is `name`) with the tolerance M2 to M5 use.
pub fn screenshot(c: &mut Client, home: &Path, name: Option<&str>, label: &str, w: u32, h: u32) -> Shot {
    let shot = home.join("tmp").join(format!("{label}.png"));
    let r = c.ok("ui.screenshot", json!({ "path": shot }));
    assert_eq!((r["width"].clone(), r["height"].clone(), r["headless"].clone()), (json!(w), json!(h), json!(true)), "{label}: {r}");
    let png = std::fs::read(&shot).unwrap();
    let out_dir = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
    std::fs::create_dir_all(&out_dir).unwrap();
    let kept = out_dir.join(format!("{label}.png"));
    std::fs::write(&kept, &png).unwrap();
    let (pw, ph, rgba) = decode_png(&png);
    assert_eq!((pw, ph), (w, h), "{label}");
    if let Some(name) = name {
        let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fingerprints/{name}.json"));
        if std::env::var_os("DALICA_BLESS").is_some() && name == label {
            std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(pw, ph, &rgba)) + "\n").unwrap();
        }
        let text = std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e} (bless with DALICA_BLESS=1)", stored.display()));
        let expected = fingerprint::from_json(&text).unwrap();
        if let Err(m) = fingerprint::check(pw, ph, &rgba, &expected, 2.0) {
            panic!("{label}: {} (the screenshot is {})", m.message, kept.display());
        }
    }
    Shot { path: kept, width: pw, rgba }
}
