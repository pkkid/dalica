//! The control channel end to end (`control-protocol.md`, roadmap M2.4): the real `dalica` binary
//! started with `--headless --control <port> --demo-fresh`, driven over TCP like an agent would:
//! authenticate, star a photo by clicking its thumbnail and pressing Ctrl+8, take a screenshot,
//! quit. Each instance runs with its home, settings, cache, run and temp folders in a temporary
//! directory, so nothing outside it is read or written (and a fresh demo left by a failed test
//! goes with it).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::common::{dalica, free_port};
use serde_json::{Value, json};

/// A `dalica` process, killed if a test ends without quitting it.
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

/// Where the app makes its default token file for `port` with the folders of [`dalica`].
fn default_token_file(home: &Path, port: u16) -> PathBuf {
    let run = if cfg!(windows) {
        home.join("localappdata").join("Dalica").join("cache").join("run")
    } else if cfg!(target_os = "macos") {
        home.join("Library").join("Caches").join("Dalica").join("run")
    } else {
        home.join("run").join("dalica")
    };
    run.join(format!("control-{port}.token"))
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

/// A control connection.
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
        // One write per line, as a client should: a line in pieces waits on delayed acknowledgements.
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
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn an_agent_stars_a_photo_by_click_and_key_and_takes_a_screenshot() {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let token_file = home.path().join("agent.token");
    let shot = home.path().join("tmp").join("starred.png");
    let started = Instant::now();
    let p = port.to_string();
    let args = ["--headless", "--control", &p, "--demo-fresh", "--control-token-file", token_file.to_str().unwrap()];
    let mut app = App { child: dalica(home.path(), args).spawn().unwrap() };
    let token = token(&token_file, &mut app);
    let ready = ms(started);

    // A second instance on the same port exits 2 instead of starting without its channel.
    let second = dalica(home.path(), ["--headless", "--control", &p, "--demo-fresh"]).stderr(Stdio::piped()).output().unwrap();
    let why = String::from_utf8_lossy(&second.stderr);
    assert_eq!(second.status.code(), Some(2), "{why}");
    assert!(why.contains(&format!("127.0.0.1:{port}")), "{why}");

    let t = Instant::now();
    let mut c = Client::connect(port);
    let auth = c.ok("auth", json!({ "token": token }));
    let connect_auth = ms(t);
    assert_eq!((auth["app"].clone(), auth["protocol"].clone()), (json!("dalica"), json!(1)));

    let settled = c.ok("ui.settle", json!({ "timeoutMs": 30000 }));
    assert_eq!(settled["settled"], true, "{settled}");
    let star = |c: &mut Client| c.ok("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [1] } }))[0]["star"].clone();
    assert_eq!(star(&mut c), false);

    let t = Instant::now();
    c.ok("ui.clickWidget", json!({ "id": "thumb:1" }));
    let click = ms(t);
    let t = Instant::now();
    c.ok("ui.key", json!({ "combo": "Ctrl+8" }));
    let key = ms(t);
    assert_eq!(star(&mut c), true, "Ctrl+8 starred the clicked photo");
    // Starring it puts its copies in Starred Photos and Recently Updated (`thumb:1/in/starred`,
    // `thumb:1/in/recent`) above its folder (`organizing.md` §4.1), which pushes the folder's cell
    // down to the view's edge: the badge is read on the copies, each wholly in view.
    let widgets = c.ok("ui.widgets", json!({ "filter": "thumb:1/in/" }));
    let rect = |id: &str| {
        let w = widgets.as_array().and_then(|ws| ws.iter().find(|w| w["id"] == id)).unwrap_or_else(|| panic!("{id} is drawn: {widgets}"));
        let r: Vec<f64> = w["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        r
    };
    for copy in ["thumb:1/in/starred", "thumb:1/in/recent"] {
        let (badge, cell) = (rect(&format!("{copy}/star")), rect(copy));
        let inside =
            badge[0] >= cell[0] && badge[1] >= cell[1] && badge[0] + badge[2] <= cell[0] + cell[2] && badge[1] + badge[3] <= cell[1] + cell[3];
        assert!(inside, "the star badge is drawn inside {copy}: badge {badge:?}, cell {cell:?}");
    }

    let t = Instant::now();
    let r = c.ok("ui.screenshot", json!({ "path": shot }));
    let screenshot = ms(t);
    assert_eq!((r["width"].clone(), r["height"].clone(), r["headless"].clone()), (json!(1400), json!(900), json!(true)));
    let png = std::fs::read(&shot).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
    // A batch over the transport: steps in order, the screenshot drawn on the CPU as above.
    let again = home.path().join("tmp").join("unstarred.png");
    let r = c.ok(
        "batch",
        json!({ "steps": [
            { "command": "photo.star.set", "params": { "ids": [1], "on": false } },
            { "method": "ui.screenshot", "params": { "path": again, "headless": true } },
        ] }),
    );
    assert_eq!((r["completed"].clone(), r["failed"].clone()), (json!(2), json!(0)), "{r}");
    assert_eq!(r["results"][1]["result"]["headless"], true, "{r}");
    assert_eq!(star(&mut c), false);
    assert!(std::fs::read(&again).unwrap() != png, "the second picture shows the star gone");
    let t = Instant::now();
    for _ in 0..20 {
        c.ok("ui.state", json!({}));
    }
    let round_trip = ms(t) / 20.0;

    // An unknown method keeps the connection open.
    assert_eq!(c.send("ui.nope", json!({}))["code"], "unknown_method");
    assert_eq!(c.ok("ui.state", json!({}))["view"], "library");

    assert_eq!(c.ok("app.quit", json!({})), json!({}));
    let status = app.wait(Duration::from_secs(60));
    assert!(status.success(), "{status}");
    let left: Vec<_> = std::fs::read_dir(home.path().join("tmp")).unwrap().filter_map(Result::ok).map(|e| e.file_name()).collect();
    assert!(!left.iter().any(|n| n.to_string_lossy().starts_with("dalica-demo-")), "the fresh demo is removed on exit: {left:?}");
    assert!(token_file.is_file(), "a named token file is kept");
    eprintln!(
        "numbers: ready {ready:.0} ms, connect+auth {connect_auth:.1} ms, click {click:.1} ms, key {key:.1} ms, screenshot {screenshot:.0} ms, \
         ui.state round trip {round_trip:.2} ms"
    );
}

#[test]
fn the_default_token_file_is_made_private_for_the_launch_and_removed_at_exit() {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let p = port.to_string();
    let mut app = App { child: dalica(home.path(), ["--control", &p, "--headless", "--demo-fresh"]).spawn().unwrap() };
    let file = default_token_file(home.path(), port);
    let token = token(&file, &mut app);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(file.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let mut c = Client::connect(port);
    // Only `auth` opens the door; a wrong token closes it.
    let wrong = c.send("auth", json!({ "token": "0".repeat(64) }));
    assert_eq!(wrong["code"], "unauthorized");
    let mut c = Client::connect(port);
    c.ok("auth", json!({ "token": token }));
    c.ok("app.quit", json!({}));
    assert!(app.wait(Duration::from_secs(60)).success());
    assert!(!file.exists(), "the default token file is removed on exit");
}
