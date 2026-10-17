//! The real `dalica` binary in `--headless` mode for M11's acceptance, driven over its control
//! channel as an agent would: the process (with the arguments and environment a part needs:
//! `--view FILE`, an automation write root, `DALICA_DEVICE_ROOTS` for a simulated card), the
//! connection and whole-window screenshots. It offers what `m6_live.rs` offers (`Client`, `Shot`,
//! `screenshot`, `start`, `quit`), so `m9_app.rs`'s waits and lookups run on it unchanged, and
//! adds [`Launch`] for those arguments.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written. The port
//! is free when picked but another process may take it before the app binds it: the app then exits
//! 2 saying so, and the launch is made again on a fresh port.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// The most launches of one start, each on a fresh port.
const LAUNCHES: usize = 5;

/// `dalica`, built for this test run (it is another package's binary).
pub fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// A port nobody listens on now (the OS picks it).
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
    /// Waits for the process to exit; its status.
    pub fn wait(&mut self, within: Duration) -> ExitStatus {
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

/// What a start adds to `dalica --headless --control PORT --library home/library
/// --automation-read-root home`: more arguments (`--view FILE`, `--automation-write-root DIR`) and
/// environment variables.
#[derive(Default)]
pub struct Launch {
    pub args: Vec<OsString>,
    pub env: Vec<(&'static str, OsString)>,
}

impl Launch {
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Launch {
        self.args.push(a.as_ref().to_os_string());
        self
    }

    pub fn env(mut self, k: &'static str, v: impl AsRef<OsStr>) -> Launch {
        self.env.push((k, v.as_ref().to_os_string()));
        self
    }
}

/// `dalica ARGS` with every folder it could touch inside `home`.
fn dalica(exe: &Path, home: &Path, args: &[OsString], log: &Path) -> Command {
    let mut c = Command::new(exe);
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    // Standard error (the log) goes to a file: a pipe nobody reads could fill and stall the app.
    let log = std::fs::File::create(log).unwrap();
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
        "DALICA_DEVICE_ROOTS",
    ] {
        c.env_remove(k);
    }
    c
}

/// Starts `dalica --headless` on the library `home/library` with automation reading inside
/// `home`, and connects to it.
pub fn start(exe: &Path, home: &Path) -> (App, Client) {
    start_with(exe, home, Launch::default())
}

/// [`start`] with `launch`'s arguments and environment.
pub fn start_with(exe: &Path, home: &Path, launch: Launch) -> (App, Client) {
    let token_file = home.join("agent.token");
    let library = home.join("library");
    let log = home.join("stderr.log");
    for _ in 0..LAUNCHES {
        let _ = std::fs::remove_file(&token_file);
        let port = free_port();
        let mut args: Vec<OsString> = ["--headless", "--control"].map(OsString::from).to_vec();
        args.push(port.to_string().into());
        args.extend([OsString::from("--library"), library.clone().into()]);
        args.extend([OsString::from("--control-token-file"), token_file.clone().into()]);
        args.extend([OsString::from("--automation-read-root"), home.to_path_buf().into()]);
        args.extend(launch.args.iter().cloned());
        let mut cmd = dalica(exe, home, &args, &log);
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        let mut app = App { child: cmd.spawn().unwrap() };
        match token(&token_file, &mut app, &log, port) {
            Some(token) => {
                let mut c = Client::connect(port);
                assert_eq!(c.ok("auth", json!({ "token": token }))["app"], "dalica");
                c.settle();
                return (app, c);
            }
            None => eprintln!("dalica: port {port} was taken; launching again"),
        }
    }
    panic!("dalica: the control port was taken {LAUNCHES} times in a row");
}

/// Waits for the token file to hold a token; `None` when the app found its port taken.
fn token(path: &Path, app: &mut App, log: &Path, port: u16) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let t = text.trim();
            if t.len() == 64 {
                return Some(t.to_string());
            }
        }
        if let Some(status) = app.child.try_wait().unwrap() {
            let said = std::fs::read_to_string(log).unwrap_or_default();
            if status.code() == Some(2) && said.contains(&format!("cannot listen on 127.0.0.1:{port}: the port is in use")) {
                return None;
            }
            panic!("dalica exited early with {status}:\n{said}");
        }
        assert!(Instant::now() < end, "no token file at {}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
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

    /// A request that must fail; the reply (`code`, `error`).
    pub fn fails(&mut self, method: &str, params: Value) -> Value {
        let v = self.send(method, params.clone());
        assert_eq!(v["ok"], false, "{method} {params} should be refused: {v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{method}: a refusal says why: {v}");
        v
    }

    pub fn settle(&mut self) {
        let s = self.ok("ui.settle", json!({ "timeoutMs": 30000 }));
        assert_eq!(s["settled"], true, "{s}");
    }

    /// An engine (or UI) command that must fail; the reply (`code`, `error`).
    pub fn refused(&mut self, command: &str, params: Value) -> Value {
        self.fails("engine.execute", json!({ "command": command, "params": params }))
    }

    /// An engine (or UI) command's result, without waiting for anything.
    pub fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({ "command": command, "params": params }))
    }

    /// The library's photos in display order (`library.query`).
    pub fn photos(&mut self) -> Vec<Value> {
        let r = self.exec("library.query", json!({ "limit": 10000 }));
        r["photos"].as_array().unwrap().clone()
    }

    /// `photo.inspect` of one photo.
    pub fn inspect(&mut self, id: &Value) -> Value {
        let r = self.exec("photo.inspect", json!({ "ids": [id] }));
        r.as_array().and_then(|a| a.first()).cloned().unwrap_or_else(|| panic!("photo.inspect {id}: {r}"))
    }

    pub fn widgets(&mut self, filter: &str) -> Vec<Value> {
        self.ok("ui.widgets", json!({ "filter": filter })).as_array().unwrap().clone()
    }

    /// The view state (`ui.state`).
    pub fn state(&mut self) -> Value {
        self.ok("ui.state", json!({}))
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

/// A whole-window screenshot of size `w × h`, kept in `target/test-output/acceptance/<label>.png`
/// and, when `name` is given, matched against `fingerprints/<name>.json` (written first under
/// `DALICA_BLESS=1`) with the tolerance the earlier milestones use.
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
        check(name, pw, ph, &rgba, &kept);
    }
    Shot { path: kept, width: pw, rgba }
}

/// Matches `rgba` against `fingerprints/<name>.json` (written first under `DALICA_BLESS=1`).
pub fn check(name: &str, w: u32, h: u32, rgba: &[u8], kept: &Path) {
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, rgba)) + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e} (bless with DALICA_BLESS=1)", stored.display()));
    let expected = fingerprint::from_json(&text).unwrap();
    if let Err(m) = fingerprint::check(w, h, rgba, &expected, 2.0) {
        panic!("{name}: {} (the screenshot is {})", m.message, kept.display());
    }
}
