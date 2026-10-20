//! One Dalica per user (`viewing.md` §3.4), with the real binary: two launches on one per-user
//! settings folder end as one, the later one's file in the first's viewer; a request with a wrong
//! token, an oversized one and one with 65 paths are refused and nothing opens; a first Dalica that
//! hangs makes the later launch start on its own once its time runs out; Esc after a hand-off
//! leaves the library where it was. Every Dalica here runs headless (its viewer served over the
//! control channel) except the later launches that only hand over and exit.

use crate::common::{dalica, free_port};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

/// The settings folder of the sandbox in `home`: the platform's per-user library folder.
fn settings(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("localappdata").join("Dalica").join("Library")
    } else if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join("Dalica").join("Library")
    } else {
        home.join("data").join("dalica").join("library")
    }
}

/// Kills the app if a test ends while it runs.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl App {
    fn exited(&mut self) -> Option<ExitStatus> {
        self.0.try_wait().unwrap()
    }

    fn wait(&mut self, within: Duration) -> ExitStatus {
        let end = Instant::now() + within;
        loop {
            if let Some(s) = self.exited() {
                return s;
            }
            assert!(Instant::now() < end, "still running after {within:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `dalica --headless --control PORT --control-token-file <home>/<name>.token [--view FILE]` on the
/// per-user library of `home`.
fn headless(home: &Path, name: &str, port: u16, view: Option<&Path>) -> App {
    let token = home.join(format!("{name}.token"));
    let mut c = dalica(home, ["--headless", "--control", &port.to_string(), "--control-token-file"]);
    c.arg(&token);
    if let Some(file) = view {
        c.arg("--view").arg(file);
    }
    App(c.spawn().unwrap())
}

/// `dalica --view FILE` in the window's mode: it hands FILE over and exits, never opening a window.
fn hand(home: &Path, file: &Path) -> App {
    let mut c = dalica(home, ["--view"]);
    c.arg(file).env("DALICA_SKIP_LIB_CHECK", "1");
    App(c.spawn().unwrap())
}

/// Two small photos in `home/Photos`.
fn photos(home: &Path) -> (PathBuf, PathBuf) {
    let folder = home.join("Photos");
    std::fs::create_dir_all(&folder).unwrap();
    for (i, name) in ["a.jpg", "b.jpg"].iter().enumerate() {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 48, 32), 80, None).unwrap();
        std::fs::write(folder.join(name), jpeg).unwrap();
    }
    (folder.join("a.jpg"), folder.join("b.jpg"))
}

/// A control connection to the app on `port`, authenticated with its token file.
struct Client {
    out: TcpStream,
    replies: BufReader<TcpStream>,
    next: u64,
}

impl Client {
    fn connect(home: &Path, name: &str, port: u16, app: &mut App) -> Client {
        let end = Instant::now() + Duration::from_secs(120);
        let token = loop {
            if let Ok(t) = std::fs::read_to_string(home.join(format!("{name}.token")))
                && t.trim().len() == 64
                && let Ok(out) = TcpStream::connect(("127.0.0.1", port))
            {
                break (t.trim().to_string(), out);
            }
            assert!(app.exited().is_none(), "{name} exited: {}", std::fs::read_to_string(home.join("stderr.log")).unwrap_or_default());
            assert!(Instant::now() < end, "{name}: no control channel");
            std::thread::sleep(Duration::from_millis(50));
        };
        let (token, out) = token;
        out.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
        let replies = BufReader::new(out.try_clone().unwrap());
        let mut c = Client { out, replies, next: 0 };
        let auth = c.send("auth", json!({ "token": token }));
        assert_eq!(auth["ok"], true, "{auth}");
        c
    }

    fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = format!("{}\n", json!({ "id": self.next, "method": method, "params": params }));
        self.out.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| panic!("{method}: not JSON ({e}): {reply:?}"))
    }

    fn view(&mut self) -> String {
        self.send("ui.state", json!({}))["result"]["view"].as_str().unwrap_or_default().to_string()
    }

    /// The file the open viewer shows, once its folder is listed; `None` while none is open.
    fn viewer_file(&mut self) -> Option<PathBuf> {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let r = self.send("engine.execute", json!({ "command": "viewer.list" }));
            if r["ok"] != true {
                return None;
            }
            if r["result"]["pending"] != true {
                return r["result"]["file"].as_str().map(PathBuf::from);
            }
            assert!(Instant::now() < end, "viewer.list stayed pending: {r}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Asks until `f` holds (or 60 s).
    fn until(&mut self, what: &str, f: impl Fn(&mut Client) -> bool) {
        let end = Instant::now() + Duration::from_secs(60);
        while !f(self) {
            assert!(Instant::now() < end, "never: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The running Dalica's port and token, from its info file.
fn info(home: &Path) -> (u16, String) {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(bytes) = std::fs::read(settings(home).join("instance.json"))
            && let Ok(v) = serde_json::from_slice::<Value>(&bytes)
            && let (Some(port), Some(token)) = (v["port"].as_u64(), v["token"].as_str())
        {
            return (port as u16, token.to_string());
        }
        assert!(Instant::now() < end, "no instance.json");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Sends `bytes` as one hand-off request; whether the running Dalica said it took them (a refusal,
/// or a connection it closed on an oversized request, is `false`).
fn offer(port: u16, bytes: &[u8]) -> bool {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    if s.write_all(bytes).and_then(|()| s.write_all(b"\n")).is_err() {
        return false;
    }
    let mut reply = String::new();
    let _ = BufReader::new(&mut s).read_line(&mut reply);
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    serde_json::from_str::<Value>(&reply).ok().and_then(|v| v["ok"].as_bool()) == Some(true)
}

#[test]
fn two_launches_at_once_end_as_one_with_the_second_file_in_the_first_viewer() {
    let home = tempfile::tempdir().unwrap();
    let (a, b) = photos(home.path());
    let ports = [free_port(), free_port()];
    let mut apps = [headless(home.path(), "a", ports[0], Some(&a)), headless(home.path(), "b", ports[1], Some(&b))];
    let end = Instant::now() + Duration::from_secs(60);
    let gone = loop {
        if let Some(i) = (0..2).find(|&i| apps[i].exited().is_some()) {
            break i;
        }
        assert!(Instant::now() < end, "neither handed its file to the other");
        std::thread::sleep(Duration::from_millis(20));
    };
    let first = 1 - gone;
    let status = apps[gone].wait(Duration::ZERO);
    assert_eq!(status.code(), Some(0), "the later launch exits 0 once its file is taken");
    let (names, files) = (["a", "b"], [&a, &b]);
    let mut c = Client::connect(home.path(), names[first], ports[first], &mut apps[first]);
    let handed = files[gone].clone();
    c.until("the later launch's file in the first's viewer", |c| c.viewer_file().as_ref() == Some(&handed));
    assert_eq!(c.view(), "viewer");
    assert!(apps[first].exited().is_none(), "the first runs on");
    c.send("app.quit", json!({}));
    assert!(apps[first].wait(Duration::from_secs(60)).success());
}

#[test]
fn esc_after_a_hand_off_leaves_the_library_where_it_was() {
    let home = tempfile::tempdir().unwrap();
    let (_, b) = photos(home.path());
    let port = free_port();
    let mut first = headless(home.path(), "first", port, None);
    let mut c = Client::connect(home.path(), "first", port, &mut first);
    assert_eq!(c.view(), "library");
    assert_eq!(c.viewer_file(), None);
    let mut later = hand(home.path(), &b);
    assert_eq!(later.wait(Duration::from_secs(60)).code(), Some(0));
    c.until("the handed file in the viewer", |c| c.viewer_file().as_ref() == Some(&b));
    assert_eq!(c.view(), "viewer");
    let esc = c.send("ui.key", json!({ "key": "Escape" }));
    assert_eq!(esc["ok"], true, "{esc}");
    c.until("back in the library", |c| c.view() == "library");
    let state = c.send("ui.state", json!({}));
    assert_eq!(state["result"]["fullScreen"], false, "{state}");
    assert!(first.exited().is_none(), "Esc after a hand-off does not quit");
    c.send("app.quit", json!({}));
    assert!(first.wait(Duration::from_secs(60)).success());
}

#[test]
fn a_wrong_token_an_oversized_request_and_65_paths_are_refused_and_nothing_opens() {
    let home = tempfile::tempdir().unwrap();
    let (a, _) = photos(home.path());
    let port = free_port();
    let mut first = headless(home.path(), "first", port, None);
    let mut c = Client::connect(home.path(), "first", port, &mut first);
    let (handoff, token) = info(home.path());
    let a = a.to_string_lossy().into_owned();
    let wrong = "0".repeat(64);
    assert!(!offer(handoff, json!({ "token": wrong, "open": [a], "wait_ms": 2000 }).to_string().as_bytes()), "a wrong token");
    let huge = json!({ "token": token, "open": [a], "pad": "x".repeat(600 * 1024), "wait_ms": 2000 }).to_string();
    assert!(!offer(handoff, huge.as_bytes()), "an oversized request");
    let many = vec![a.clone(); 65];
    assert!(!offer(handoff, json!({ "token": token, "open": many, "wait_ms": 2000 }).to_string().as_bytes()), "65 paths");
    assert_eq!(c.viewer_file(), None, "nothing opened");
    assert_eq!(c.view(), "library");
    // The right request still opens it (the refusals did not break the channel).
    assert!(offer(handoff, json!({ "token": token, "open": [a], "wait_ms": 10000 }).to_string().as_bytes()));
    c.until("the file in the viewer", |c| c.viewer_file().is_some());
    c.send("app.quit", json!({}));
    assert!(first.wait(Duration::from_secs(60)).success());
}

#[test]
fn a_first_dalica_that_hangs_makes_the_later_launch_start_on_its_own() {
    let home = tempfile::tempdir().unwrap();
    let (_, b) = photos(home.path());
    // A first Dalica that holds the lock and accepts connections but never answers.
    let dir = settings(home.path());
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::File::create(dir.join("instance.lock")).unwrap();
    lock.lock().unwrap();
    let hung = TcpListener::bind("127.0.0.1:0").unwrap();
    let hung_port = hung.local_addr().unwrap().port();
    std::fs::write(dir.join("instance.json"), json!({ "port": hung_port, "token": "0".repeat(64), "pid": 1 }).to_string()).unwrap();
    let started = Instant::now();
    let port = free_port();
    let mut later = headless(home.path(), "later", port, Some(&b));
    let mut c = Client::connect(home.path(), "later", port, &mut later);
    assert_eq!(c.viewer_file(), Some(b.clone()), "its own viewer, on its own file");
    assert!(started.elapsed() >= Duration::from_secs(9), "it waited its time first: {:?}", started.elapsed());
    c.send("app.quit", json!({}));
    assert!(later.wait(Duration::from_secs(60)).success());
    drop(hung);
}
