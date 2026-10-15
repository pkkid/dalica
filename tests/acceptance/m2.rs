//! M2 "Agent eyes" (`docs/roadmap.md` → M2 → Done when; run by `cargo xtask accept M2`): an agent's
//! control session against the real `dalica` binary in `--headless` mode on a fresh demo library
//! (auth, click a thumbnail, Ctrl+8, screenshot), checking the star in the engine, in the drawn
//! widgets and in the sidecar on disk, and the screenshot's PNG fingerprint; then the protocol's
//! security tests (`apps/dalica/tests/control_security.rs`: wrong token, oversized line,
//! non-loopback bind refused, …), run as they are. The manual check (a real window on port 7990,
//! an agent's screenshot matches the screen) is in `m2.md`.
//!
//! The app runs with its home, settings, cache, run and temp folders inside a temporary directory,
//! on a port the OS picked (never 7990), so nothing outside the test is read or written.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// `dalica`, built for this test run (it is another package's binary).
fn dalica_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica"]).status().unwrap();
    assert!(status.success(), "building dalica failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX))
}

/// Runs `cargo test` with `args`; panics with its output if it fails.
fn cargo_test(args: &[&str]) {
    let started = Instant::now();
    let out = dalica_acceptance::cargo_split(&[&["test"][..], args].concat(), |_| {}).unwrap();
    assert!(
        out.status.success(),
        "cargo test {}:\n{}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let summary =
        String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.starts_with("test result: ok")).map(str::to_string).collect::<Vec<_>>().join("; ");
    println!("cargo test {}: {summary} ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
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

    /// Photo 1 as `photo.inspect` reports it.
    fn photo_1(&mut self) -> Value {
        self.ok("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [1] } }))[0].clone()
    }

    /// The ids of the star badges drawn in the last frame.
    fn star_badges(&mut self) -> Vec<String> {
        let widgets = self.ok("ui.widgets", json!({ "filter": "/star" }));
        let ids = widgets.as_array().unwrap().iter().filter_map(|w| w["id"].as_str()).map(str::to_string);
        ids.filter(|id| id.starts_with("thumb:") && id.ends_with("/star")).collect()
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
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

/// Whether the sidecar next to `photo` says it is starred (`None` while there is no sidecar yet).
fn starred_in_sidecar(photo: &Path) -> Option<bool> {
    let sidecar = photo.parent()?.join(".dalica.json");
    let v: Value = serde_json::from_slice(&std::fs::read(sidecar).ok()?).ok()?;
    let name = photo.file_name()?.to_str()?;
    Some(v["photos"][name]["star"] == json!(true))
}

#[test]
fn agent_eyes() {
    let started = Instant::now();
    let exe = dalica_exe();
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let token_file = home.path().join("agent.token");
    // The OS temporary folder (here, the isolated one) is a default write root (§5.7).
    let shot = home.path().join("tmp").join("m2-screenshot.png");

    // 1. A headless instance with its control channel on a fresh demo library.
    let p = port.to_string();
    let args = ["--headless", "--control", &p, "--demo-fresh", "--control-token-file", token_file.to_str().unwrap()];
    let mut app = App { child: dalica(&exe, home.path(), &args).spawn().unwrap() };
    let token = token(&token_file, &mut app);
    let ready = ms(started);

    // 2. auth: nothing but `auth` opens the door, then the whole surface is there.
    let t = Instant::now();
    let mut c = Client::connect(port);
    let auth = c.ok("auth", json!({ "token": token }));
    let connect_auth = ms(t);
    assert_eq!((auth["app"].clone(), auth["protocol"].clone()), (json!("dalica"), json!(1)), "{auth}");
    let settled = c.ok("ui.settle", json!({ "timeoutMs": 30000 }));
    assert_eq!(settled["settled"], true, "{settled}");
    let before = c.photo_1();
    assert_eq!(before["star"], false, "a fresh demo has no stars: {before}");
    assert_eq!(c.star_badges(), Vec::<String>::new(), "no star badge drawn yet");
    let photo_path = PathBuf::from(before["path"].as_str().unwrap());
    assert_ne!(starred_in_sidecar(&photo_path), Some(true));

    // 3. Click photo 1's thumbnail: it alone is selected.
    let t = Instant::now();
    c.ok("ui.clickWidget", json!({ "id": "thumb:1" }));
    let click = ms(t);
    let state = c.ok("ui.state", json!({}));
    assert_eq!(state["selection"]["ids"], json!([1]), "{state}");

    // 4. Ctrl+8 stars it: in the engine, as a badge on its thumbnail (and no other), in the sidecar.
    let t = Instant::now();
    c.ok("ui.key", json!({ "combo": "Ctrl+8" }));
    let key = ms(t);
    assert_eq!(c.photo_1()["star"], true, "Ctrl+8 starred the clicked photo");
    c.ok("ui.settle", json!({ "timeoutMs": 30000 }));
    // Its cell in its folder and its copy in Starred Photos, first on the light table (M9.17).
    assert_eq!(c.star_badges(), ["thumb:1/in/starred/star", "thumb:1/star"], "exactly photo 1's star badges are drawn");
    let end = Instant::now() + Duration::from_secs(30);
    while starred_in_sidecar(&photo_path) != Some(true) {
        assert!(Instant::now() < end, "the star never reached {}", photo_path.with_file_name(".dalica.json").display());
        std::thread::sleep(Duration::from_millis(50));
    }

    // 5. The screenshot: a 1400 × 900 PNG whose fingerprint matches the approved baseline.
    let t = Instant::now();
    let r = c.ok("ui.screenshot", json!({ "path": shot }));
    let screenshot = ms(t);
    assert_eq!((r["width"].clone(), r["height"].clone(), r["headless"].clone()), (json!(1400), json!(900), json!(true)), "{r}");
    let png = std::fs::read(&shot).unwrap();
    let out_dir = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
    std::fs::create_dir_all(&out_dir).unwrap();
    let kept = out_dir.join("m2-screenshot.png");
    std::fs::write(&kept, &png).unwrap();
    let (w, h, rgba) = decode_png(&png);
    assert_eq!((w, h), (1400, 900));
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fingerprints/m2-screenshot.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        panic!("m2 screenshot: {} (the screenshot is {})", m.message, kept.display());
    }

    // 6. The agent quits the app; the fresh demo goes with it.
    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
    let left: Vec<_> = std::fs::read_dir(home.path().join("tmp")).unwrap().filter_map(Result::ok).map(|e| e.file_name()).collect();
    assert!(!left.iter().any(|n| n.to_string_lossy().starts_with("dalica-demo-")), "the fresh demo is removed on exit: {left:?}");
    println!(
        "control session: ready {ready:.0} ms, connect+auth {connect_auth:.1} ms, click {click:.1} ms, Ctrl+8 {key:.1} ms, \
         screenshot {screenshot:.0} ms; screenshot in {}",
        kept.display()
    );

    // 7. The protocol's security tests: wrong token, no auth, oversized line, non-JSON and HTTP
    //    lines, the 17th connection, a readable token file, no non-loopback bind.
    //    (Unless the ci test step already ran them: `dalica_acceptance::SUITES_RAN`.)
    if !dalica_acceptance::suites_ran() {
        cargo_test(&["-p", "dalica", "--test", "control_security"]);
    }
    println!("M2 acceptance: passed in {:.1} s", started.elapsed().as_secs_f64());
}
