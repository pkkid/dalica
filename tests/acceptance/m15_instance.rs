//! One Dalica (`m15.rs` part 4; `roadmap.md` M15 → OS integration's *Done when*: "a second
//! `dalica --view FILE` hands FILE to the first Dalica and exits"), with the real binary in a
//! sandboxed home: a first `dalica --headless` on the per-user library (no `--library`, so it takes
//! part in the hand-off) shows the library; a later `dalica --view FILE`, in the window's mode,
//! hands FILE to it and exits 0 without opening a window; the first's photo viewer then shows FILE
//! and Esc brings the library back, the first still running.
//!
//! The refusals (a wrong token, an oversized request, too many paths, a first that hangs) are
//! `apps/dalica`'s `instance` suite (`m15_checks.rs`); macOS's open-documents events and Dalica
//! chosen as the default viewer in a file manager are checked by hand (`m15.md`).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::common::{COLOURS, painted_jpeg};

const PATIENCE: Duration = Duration::from_secs(120);

/// A process killed if the test ends while it runs.
struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl Proc {
    fn wait(&mut self, within: Duration) -> ExitStatus {
        let end = Instant::now() + within;
        loop {
            if let Some(s) = self.0.try_wait().unwrap() {
                return s;
            }
            assert!(Instant::now() < end, "still running after {within:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `dalica ARGS` with every folder it could touch inside `home`.
fn dalica(exe: &Path, home: &Path, args: &[&std::ffi::OsStr], log: &str) -> Command {
    let mut c = Command::new(exe);
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("USERPROFILE", home.to_path_buf()),
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
    let log = std::fs::File::create(home.join(log)).unwrap();
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log));
    c
}

/// A control connection, authenticated.
struct Client {
    out: TcpStream,
    replies: BufReader<TcpStream>,
    next: u64,
}

impl Client {
    fn connect(port: u16, token_file: &Path, first: &mut Proc, home: &Path) -> Client {
        let end = Instant::now() + PATIENCE;
        let (token, out) = loop {
            if let Ok(t) = std::fs::read_to_string(token_file)
                && t.trim().len() == 64
                && let Ok(out) = TcpStream::connect(("127.0.0.1", port))
            {
                break (t.trim().to_string(), out);
            }
            assert!(
                first.0.try_wait().unwrap().is_none(),
                "the first exited: {}",
                std::fs::read_to_string(home.join("first.log")).unwrap_or_default()
            );
            assert!(Instant::now() < end, "no control channel");
            std::thread::sleep(Duration::from_millis(50));
        };
        out.set_read_timeout(Some(PATIENCE)).unwrap();
        let replies = BufReader::new(out.try_clone().unwrap());
        let mut c = Client { out, replies, next: 0 };
        assert_eq!(c.ok("auth", json!({ "token": token }))["app"], "dalica");
        c
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = format!("{}\n", json!({ "id": self.next, "method": method, "params": params }));
        self.out.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        let v: Value = serde_json::from_str(&reply).unwrap_or_else(|e| panic!("{method}: not JSON ({e}): {reply:?}"));
        assert_eq!(v["ok"], true, "{method} {params}: {v}");
        v["result"].clone()
    }

    fn view(&mut self) -> String {
        self.ok("ui.state", json!({}))["view"].as_str().unwrap_or_default().to_string()
    }

    /// The file the open viewer shows, once listed; `None` while none is open.
    fn viewer_file(&mut self) -> Option<PathBuf> {
        let r = self.ok("ui.state", json!({}));
        if r["viewer"].is_null() {
            return None;
        }
        let listed = self.ok("engine.execute", json!({ "command": "viewer.list" }));
        if listed["pending"] == true { None } else { listed["file"].as_str().map(PathBuf::from) }
    }

    fn until(&mut self, what: &str, f: impl Fn(&mut Client) -> bool) {
        let end = Instant::now() + PATIENCE;
        while !f(self) {
            assert!(Instant::now() < end, "never: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub fn handoff(exe: &Path, home: &Path) {
    let photos = home.join("Photos");
    std::fs::create_dir_all(&photos).unwrap();
    let file = photos.join("handed.jpg");
    std::fs::write(&file, painted_jpeg(320, 240, COLOURS[3])).unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port().to_string();
    let token = home.join("first.token");
    let args: Vec<&std::ffi::OsStr> =
        vec!["--headless".as_ref(), "--control".as_ref(), port.as_ref(), "--control-token-file".as_ref(), token.as_os_str()];
    let mut first = Proc(dalica(exe, home, &args, "first.log").spawn().unwrap());
    let mut c = Client::connect(port.parse().unwrap(), &token, &mut first, home);
    c.ok("ui.settle", json!({ "timeoutMs": 30000 }));
    assert_eq!(c.view(), "library");
    assert_eq!(c.viewer_file(), None);

    let started = Instant::now();
    let mut later = Proc(dalica(exe, home, &["--view".as_ref(), file.as_os_str()], "later.log").env("DALICA_SKIP_LIB_CHECK", "1").spawn().unwrap());
    let status = later.wait(PATIENCE);
    assert_eq!(
        status.code(),
        Some(0),
        "the later launch hands its file over and exits 0: {}",
        std::fs::read_to_string(home.join("later.log")).unwrap_or_default()
    );
    println!("  the second dalica --view exited 0 after {:.2} s", started.elapsed().as_secs_f64());
    c.until("the handed file in the first's viewer", |c| c.viewer_file().as_ref() == Some(&file));
    assert_eq!(c.view(), "viewer");
    c.ok("ui.key", json!({ "key": "Escape" }));
    c.until("back in the library", |c| c.view() == "library");
    assert!(first.0.try_wait().unwrap().is_none(), "the first runs on");
    c.ok("app.quit", json!({}));
    assert!(first.wait(Duration::from_secs(60)).success());
}
