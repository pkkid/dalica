//! `dalica --view FILE`, what the file manager's Open with runs (`releasing.md` → Windows;
//! `viewing.md` §3): the real binary takes any file (existing, missing, odd characters, a leading
//! `-`) without a usage error, opens its library as usual and opens the photo viewer on the file;
//! with `--headless` the viewer's state is served over the control channel (`viewer.list`).
//! Started headless with every folder it could touch inside a temporary directory.
//!
//! The control port is one the OS just handed out and released, so another process (a test
//! running beside this one) can take it before the app binds it. The app then says so and exits 2
//! ("cannot listen on 127.0.0.1:<port>: the port is in use"); on exactly that exit the launch is
//! made again with a fresh port, at most [`LAUNCHES`] times. Any other early exit fails the test.

use crate::common::{free_port, sandbox};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The most launches of one test, each on a fresh port.
const LAUNCHES: usize = 5;

/// How a launch ended.
enum Launch {
    /// The library opened; the app runs on this port.
    Opened(App, u16),
    /// Another process had taken the control port.
    PortTaken,
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

/// Starts `dalica --view FILE` headless on a library in `home` and waits until the library has
/// opened (its folder holds files); panics if the app exits first, with what it printed.
fn opens_with(home: &Path, file: OsString) -> (App, u16) {
    opens_on(home, file, std::iter::repeat_with(free_port)).0
}

/// [`opens_with`] on the control ports `ports` hands out, the next one after each launch that
/// found its port taken; returns the app, its port and how many launches it took.
fn opens_on(home: &Path, file: OsString, mut ports: impl Iterator<Item = u16>) -> ((App, u16), usize) {
    for n in 1..=LAUNCHES {
        let port = ports.next().unwrap();
        match launch(home, &file, port) {
            Launch::Opened(app, port) => return ((app, port), n),
            Launch::PortTaken => eprintln!("dalica --view {file:?}: port {port} was taken; launching again"),
        }
    }
    panic!("dalica --view {file:?}: the control port was taken {LAUNCHES} times in a row");
}

/// One launch of `dalica --view FILE` with `--control port`.
fn launch(home: &Path, file: &OsString, port: u16) -> Launch {
    let library = home.join("library");
    let stderr = home.join("stderr.log");
    let mut c = Command::new(env!("CARGO_BIN_EXE_dalica"));
    sandbox(&mut c, home);
    c.arg("--view")
        .arg(file)
        .arg("--library")
        .arg(&library)
        .args(["--control", &port.to_string(), "--headless", "--control-token-file"])
        .arg(home.join("control.token"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    let mut app = App(c.spawn().unwrap());
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        if std::fs::read_dir(&library).is_ok_and(|mut d| d.next().is_some()) {
            break;
        }
        if let Some(status) = app.0.try_wait().unwrap() {
            let said = std::fs::read_to_string(&stderr).unwrap_or_default();
            let taken = format!("cannot listen on 127.0.0.1:{port}: the port is in use");
            if status.code() == Some(2) && said.contains(&taken) {
                return Launch::PortTaken;
            }
            panic!("dalica --view {file:?} exited with {status} before opening its library:\n{said}");
        }
        assert!(Instant::now() < end, "dalica --view {file:?} did not open its library");
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = std::fs::read_to_string(&stderr).unwrap_or_default();
    assert!(!said.contains("usage:"), "dalica --view {file:?} printed its usage:\n{said}");
    Launch::Opened(app, port)
}

/// A control connection, authenticated with the token file in `home`.
struct Client {
    out: TcpStream,
    replies: BufReader<TcpStream>,
    next: u64,
}

impl Client {
    fn connect(home: &Path, port: u16) -> Client {
        let end = Instant::now() + Duration::from_secs(120);
        let token = loop {
            if let Ok(t) = std::fs::read_to_string(home.join("control.token"))
                && t.trim().len() == 64
            {
                break t.trim().to_string();
            }
            assert!(Instant::now() < end, "no control token");
            std::thread::sleep(Duration::from_millis(50));
        };
        let out = TcpStream::connect(("127.0.0.1", port)).unwrap();
        out.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
        let replies = BufReader::new(out.try_clone().unwrap());
        let mut c = Client { out, replies, next: 0 };
        let auth = c.send("auth", json!({ "token": token }));
        assert_eq!(auth["ok"], true, "{auth}");
        c
    }

    /// Sends one request and returns its reply.
    fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = format!("{}\n", json!({ "id": self.next, "method": method, "params": params }));
        self.out.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| panic!("{method}: not JSON ({e}): {reply:?}"))
    }

    /// `viewer.list` of the open viewer, asked again while it is pending: the reply.
    fn viewer(&mut self) -> Value {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let r = self.send("engine.execute", json!({ "command": "viewer.list" }));
            if r["ok"] != true || r["result"]["pending"] != true {
                return r;
            }
            assert!(Instant::now() < end, "viewer.list stayed pending: {r}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `names` as small JPEGs in a fresh folder of `home`; the folder.
fn photos(home: &Path, names: &[&str]) -> PathBuf {
    let folder = home.join("Photos");
    std::fs::create_dir_all(&folder).unwrap();
    for (i, name) in names.iter().enumerate() {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 48, 32), 80, None).unwrap();
        std::fs::write(folder.join(name), jpeg).unwrap();
    }
    folder
}

/// A control port another process holds: the launch on it exits 2 saying so, and the next launch,
/// on a fresh port, opens the library.
#[test]
fn a_taken_control_port_is_launched_again_on_another() {
    let home = tempfile::tempdir().unwrap();
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = held.local_addr().unwrap().port();
    let photo = home.path().join("photo.jpg");
    std::fs::write(&photo, b"not really a JPEG").unwrap();
    let (_app, launches) = opens_on(home.path(), photo.into_os_string(), std::iter::once(taken).chain(std::iter::repeat_with(free_port)));
    assert_eq!(launches, 2, "the first launch found port {taken} taken");
    drop(held);
}

/// The viewer opens on the photo: `ui.state` says `view: "viewer"`, its folder lists in name order
/// with the photo's place, Esc quits, nothing is written into the folder.
#[test]
fn view_headless_serves_the_viewer_on_that_photo() {
    let home = tempfile::tempdir().unwrap();
    let folder = photos(home.path(), &["c.jpg", "a.jpg", "b.jpg"]);
    let (mut app, port) = opens_with(home.path(), folder.join("b.jpg").into_os_string());
    let mut c = Client::connect(home.path(), port);
    let r = c.viewer();
    assert_eq!(r["ok"], true, "{r}");
    let v = &r["result"];
    assert_eq!(v["file"], json!(folder.join("b.jpg")));
    assert_eq!(v["index"], 1);
    let names: Vec<&str> = v["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["a.jpg", "b.jpg", "c.jpg"]);
    assert_eq!(v["readOnly"], false);
    let state = c.send("ui.state", json!({}));
    assert_eq!(state["result"]["view"], "viewer", "{state}");
    // Esc closes the viewer, and the app with it.
    assert_eq!(c.send("ui.key", json!({ "combo": "Esc" }))["ok"], true);
    let end = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = app.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "Esc did not quit dalica");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status}");
    let mut left: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["a.jpg", "b.jpg", "c.jpg"], "viewing writes nothing into the folder");
}

/// A file Dalica cannot show: the viewer says why, the file is left as it is, the app runs on.
#[test]
fn view_refuses_a_file_it_cannot_show_with_its_reason() {
    let home = tempfile::tempdir().unwrap();
    let photo = home.path().join("photo.jpg");
    std::fs::write(&photo, b"not really a JPEG").unwrap();
    let (_app, port) = opens_with(home.path(), photo.clone().into_os_string());
    let mut c = Client::connect(home.path(), port);
    let r = c.viewer();
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().unwrap().contains("photo.jpg"), "{r}");
    assert_eq!(std::fs::read(&photo).unwrap(), b"not really a JPEG");
    assert_eq!(c.send("ui.state", json!({}))["ok"], true, "still answering");
}

#[test]
fn view_opens_the_library_for_a_missing_file() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("no such folder").join("gone.cr3");
    let (_app, port) = opens_with(home.path(), missing.clone().into_os_string());
    let r = Client::connect(home.path(), port).viewer();
    assert!(r["error"].as_str().unwrap().contains("no such file"), "{r}");
    assert!(!missing.exists());
    assert!(!missing.parent().unwrap().exists());
}

#[test]
fn view_opens_odd_file_names() {
    // Names Windows forbids (`"`, `*`, `?`, `<`, `>`, `|`, `:`) are only used where they can exist.
    let mut names = vec!["ünïcødé 写真 (1) & $(echo run).jpeg".to_string(), "-x.jpg".to_string(), "--demo".to_string()];
    if !cfg!(windows) {
        names.push("it's \"quoted\" *?<>|;.jpg".to_string());
    }
    for name in names {
        let home = tempfile::tempdir().unwrap();
        let folder = photos(home.path(), &[name.as_str()]);
        let (_app, port) = opens_with(home.path(), folder.join(&name).into_os_string());
        let r = Client::connect(home.path(), port).viewer();
        assert_eq!(r["ok"], true, "{name}: {r}");
        assert_eq!(r["result"]["index"], 0, "{name}");
        assert_eq!(r["result"]["photos"][0]["name"], json!(name));
        assert!(!home.path().join("library").join(&name).exists());
    }
    // A bare name that looks like an option is a file in the working folder, not an option.
    let home = tempfile::tempdir().unwrap();
    let _ = opens_with(home.path(), OsString::from("--demo"));
}
