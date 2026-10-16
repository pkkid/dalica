//! M10's *MCP and workflows* in the live binaries (`m10.rs` part 3): W02 (the tray across two
//! folders, rotated, one undo) and W01 (scan, star five, an album of them, three captions, a crop
//! and Auto Fix, the album exported at 1600 px) through `dalica-cli mcp` over real stdio, twice on
//! copies of the same folders: once in headless mode (`--library`), once in connect mode
//! (`--connect`) against `dalica --headless --control`. Each run is checked as the workflows check
//! it (the files on disk: sidecars, the exported JPEGs' sizes and their XMP and IPTC captions, the
//! captions written back, the originals' bytes), and the two runs' outcomes must be equal: both
//! modes agree (`mcp.md` §1, §9).
//!
//! W01 here scans 25 photos rather than 200 (the five it stars are the same 2400 × 1600 ones);
//! the full workflows run in-process in `dalica-mcp`'s `workflows` binary (`m10_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::secs;

/// How long one reply, or one background write (a sidecar or a caption after its debounce), may take.
const PATIENCE: Duration = Duration::from_secs(120);
const SIDECAR: &str = ".dalica.json";
/// W01's photos: `SMALL` small ones, then `LARGE` of 2400 × 1600 (`IMG_0021.jpg` …).
const SMALL: u64 = 20;
const LARGE: u64 = 5;
const SIDE: u64 = 1600;
const CROP: [f64; 4] = [0.0, 0.0, 0.5, 1.0];
const CAPTIONS: [&str; 3] = ["Tram 28 on the hill", "Alfama at dusk", "The river from the castle"];

fn name(n: u64) -> String {
    format!("IMG_{n:04}.jpg")
}

/// `dalica-cli`, built for this test run.
fn cli_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// The environment of a process whose every folder lies inside `home` (as `m6_live.rs` sets it).
fn contain(c: &mut Command, home: &Path) {
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
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
}

/// A child process killed if the test ends without it exiting.
struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// `dalica-cli mcp ARGS`, driven over its standard input and output as an MCP client drives it.
struct Mcp {
    child: Proc,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    next: u64,
}

impl Mcp {
    fn spawn(cli: &Path, home: &Path, args: &[&std::ffi::OsStr]) -> Mcp {
        let mut cmd = Command::new(cli);
        cmd.arg("mcp").args(args);
        contain(&mut cmd, home);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(home.join("mcp-stderr.log")).unwrap();
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log)).spawn().unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut m = Mcp { child: Proc(child), input, lines, next: 0 };
        let init = m.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m10", "version": "0" } }),
        );
        assert_eq!(init["serverInfo"]["name"], "dalica", "{init}");
        m.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        m
    }

    fn send(&mut self, msg: &Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
    }

    /// Request `method`; its result, skipping notifications. Standard output carries only JSON-RPC.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let start = Instant::now();
        loop {
            let left = PATIENCE.checked_sub(start.elapsed()).unwrap_or_default();
            let line = self.lines.recv_timeout(left).unwrap_or_else(|e| panic!("{method} {params}: no reply ({e})"));
            let msg: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC on standard output ({e}): {line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{line}");
            if msg["id"] == json!(id) {
                assert!(msg.get("error").is_none(), "{method} {params}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    /// Calls tool `name`, failing on a tool error; its `structuredContent`.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    fn ids(answer: &Value) -> Vec<u64> {
        answer["photos"].as_array().unwrap_or_else(|| panic!("no photos in {answer}")).iter().map(|p| p["id"].as_u64().unwrap()).collect()
    }

    fn section_ids(&mut self, section: &str) -> Vec<u64> {
        let r = self.tool("doc_inspect", json!({ "photos": section, "limit": 1000 }));
        Self::ids(&r)
    }

    fn photo(&mut self, id: u64) -> Value {
        self.tool("photo_info", json!({ "id": id }))
    }

    fn name_of(&mut self, id: u64) -> String {
        self.photo(id)["name"].as_str().unwrap().to_string()
    }

    /// Closes standard input and waits for the process to exit cleanly.
    fn finish(mut self) {
        drop(self.input.take());
        let end = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.0.try_wait().unwrap() {
                assert!(status.success(), "dalica-cli mcp exited with {status}");
                return;
            }
            assert!(Instant::now() < end, "dalica-cli mcp did not exit after its input closed");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// `root/Lisbon` (W01: `SMALL` small photos and `LARGE` large ones) and `root/Porto` (four small).
fn folders(root: &Path) -> (PathBuf, PathBuf) {
    let (lisbon, porto) = (root.join("Lisbon"), root.join("Porto"));
    for dir in [&lisbon, &porto] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for n in 1..=SMALL + LARGE {
        let (w, h) = if n > SMALL { (2400, 1600) } else { (160, 120) };
        std::fs::write(lisbon.join(name(n)), dalica_scenes::jpeg(&dalica_scenes::Scene::new(1000 + n, w, h), 90, None).unwrap()).unwrap();
    }
    for n in 1..=4 {
        std::fs::write(porto.join(name(n)), dalica_scenes::jpeg(&dalica_scenes::Scene::new(2000 + n, 160, 120), 90, None).unwrap()).unwrap();
    }
    (lisbon, porto)
}

fn sidecar_edits(folder: &Path, name: &str) -> Vec<String> {
    let v: Value = std::fs::read(folder.join(SIDECAR)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    v.pointer(&format!("/photos/{name}/edits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Waits until `holds`; panics naming `what` after [`PATIENCE`].
fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    let start = Instant::now();
    while !holds() {
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The caption a file's XMP and IPTC carry, read back by `dalica-meta`.
fn captions(path: &Path) -> (Option<String>, Option<String>) {
    let read = dalica_meta::read(&std::fs::read(path).unwrap(), &dalica_meta::Limits::default()).unwrap();
    (read.sources.xmp.caption, read.sources.iptc.caption)
}

/// W02's steps; what they did, by photo name (no ids, no paths), and Lisbon's section.
fn w02(m: &mut Mcp, lisbon: &Path, porto: &Path) -> (Value, String) {
    let sa = m.tool("scan_folder", json!({ "path": lisbon }))["section"].as_str().unwrap().to_string();
    let sb = m.tool("scan_folder", json!({ "path": porto }))["section"].as_str().unwrap().to_string();
    let (in_a, in_b) = (m.section_ids(&sa), m.section_ids(&sb));
    assert_eq!((in_a.len() as u64, in_b.len()), (SMALL + LARGE, 4), "{sa}, {sb}");
    let held = &in_a[..2];
    let picked = &in_b[..3];
    m.tool("select_photos", json!({ "ids": held, "section": sa, "hold": true }));
    let t = m.tool("select_photos", json!({ "ids": picked, "section": sb }));
    let tray = t["tray"].as_array().map_or(0, Vec::len);
    assert_eq!(tray, 5, "the tray holds the held two and the selected three: {t}");
    let five: Vec<u64> = held.iter().chain(picked).copied().collect();
    let names_a: Vec<String> = held.iter().map(|id| m.name_of(*id)).collect();
    let names_b: Vec<String> = picked.iter().map(|id| m.name_of(*id)).collect();
    // Rotate Clockwise with no ids acts on the Photo Tray (`mcp.md` §4.1, §4.4), in both modes.
    m.tool("cmd_photo_rotate_cw", json!({}));
    let rotated: Vec<Value> = five.iter().map(|id| m.photo(*id)["rotation"].clone()).collect();
    assert_eq!(rotated, vec![json!(90); 5], "the tray's five are rotated");
    let untouched = m.photo(in_a[2])["rotation"].clone();
    assert_eq!(untouched, json!(0), "a photo outside the tray is not");
    let recorded =
        |want: &[&str]| names_a.iter().all(|n| sidecar_edits(lisbon, n) == want) && names_b.iter().all(|n| sidecar_edits(porto, n) == want);
    eventually("both folders' sidecars record rotate(90)", || recorded(&["rotate(90)"]));
    m.tool("cmd_edit_undo", json!({}));
    let undone: Vec<Value> = five.iter().map(|id| m.photo(*id)["rotation"].clone()).collect();
    assert_eq!(undone, vec![json!(0); 5], "one undo takes all five back");
    eventually("both folders' sidecars record no edit after the undo", || recorded(&[]));
    (json!({ "tray": tray, "rotated": rotated, "undone": undone, "lisbon": names_a, "porto": names_b }), sa)
}

/// W01's steps on `Lisbon`, scanned by [`w02`] as `section`; what they did, by photo name.
fn w01(m: &mut Mcp, lisbon: &Path, section: &str, exports: &Path) -> Value {
    let listed = m.tool("query_photos", json!({ "section": section, "limit": 1000 }));
    let by_name: std::collections::BTreeMap<String, u64> =
        listed["photos"].as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap().to_string(), p["id"].as_u64().unwrap())).collect();
    assert_eq!(by_name.len() as u64, SMALL + LARGE, "{section}: {listed}");
    let large: Vec<String> = (SMALL + 1..=SMALL + LARGE).map(name).collect();
    let five: Vec<u64> = large.iter().map(|n| by_name[n]).collect();
    m.tool("set_metadata", json!({ "ids": five, "star": true }));
    let starred = m.tool("query_photos", json!({ "starred": true, "limit": 1000 }));
    let mut starred = Mcp::ids(&starred);
    starred.sort_unstable();
    let mut want = five.clone();
    want.sort_unstable();
    assert_eq!(starred, want, "the five large photos, and only they, are starred");
    let album = m.tool("create_album", json!({ "name": "Lisbon highlights", "ids": starred }));
    assert_eq!(album["count"], json!(LARGE), "{album}");
    let album_id = album["album"].as_str().unwrap().to_string();
    for (id, caption) in five.iter().zip(CAPTIONS) {
        m.tool("set_metadata", json!({ "ids": [id], "caption": caption }));
    }
    let edited = five[0];
    m.tool("apply_edit", json!({ "ids": [edited], "op": "crop", "params": { "rect": CROP } }));
    m.tool("apply_edit", json!({ "ids": [edited], "op": "autofix" }));
    let recipe = m.tool("get_recipe", json!({ "id": edited }));
    let ops: Vec<String> = recipe["ops"].as_array().unwrap().iter().filter_map(|o| o["op"].as_str().map(str::to_string)).collect();
    assert_eq!(ops, ["crop", "autofix"], "{recipe}");
    let exported = m.tool("export", json!({ "album": album_id, "dir": exports, "folder_name": "Lisbon", "size": SIDE }));
    assert_eq!((&exported["failed"], &exported["cancelled"]), (&json!([]), &json!(false)), "{exported}");
    let mut files: Vec<Value> = exported["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let path = PathBuf::from(f["path"].as_str().unwrap());
            assert!(path.starts_with(exports.join("Lisbon")), "{}", path.display());
            let bytes = std::fs::read(&path).unwrap();
            let header = dalica_image::read_header(&bytes, &dalica_image::Limits::default()).unwrap();
            let (w, h) = (u64::from(header.width), u64::from(header.height));
            assert_eq!(w.max(h), SIDE, "{}: the long edge is {SIDE} px ({w} × {h})", path.display());
            let id = f["id"].as_u64().unwrap();
            if id == edited {
                assert_eq!((w, h), (SIDE * 3 / 4, SIDE), "{}: the crop is visible", path.display());
            }
            let caption = five.iter().zip(CAPTIONS).find(|(p, _)| **p == id).map(|(_, c)| c.to_string());
            let read = captions(&path);
            assert_eq!(read, (caption.clone(), caption), "{}: the caption in XMP and IPTC", path.display());
            json!({ "file": path.file_name().unwrap().to_string_lossy(), "photo": m.name_of(id), "width": w, "height": h, "caption": read.0 })
        })
        .collect();
    files.sort_by_key(|f| f["file"].as_str().unwrap_or_default().to_string());
    assert_eq!(files.len() as u64, LARGE, "one file per photo of the album");
    // The captions reach the photos' own files after their debounce, with nothing more asked of
    // the app (an agent that has stopped calling; `data-model.md` §5).
    for (n, caption) in large.iter().zip(CAPTIONS) {
        let path = lisbon.join(n);
        eventually(&format!("{n} carries its caption in its own file"), || captions(&path).0.as_deref() == Some(caption));
    }
    let info = m.photo(edited);
    assert_eq!(info["unsavedEdits"], true, "the crop and Auto Fix stay unsaved: {info}");
    let photos: Vec<Value> = large
        .iter()
        .map(|n| {
            let p = m.photo(by_name[n]);
            json!({ "name": n, "star": p["star"], "caption": p["caption"], "albums": p["albums"].as_array().map_or(0, Vec::len) })
        })
        .collect();
    json!({ "scanned": by_name.len(), "photos": photos, "album": album["count"], "ops": ops, "files": files })
}

/// The originals of `folder` other than `captioned` are byte for byte what `before` holds.
fn originals(folder: &Path, before: &[(String, Vec<u8>)], captioned: &[String]) {
    for (n, bytes) in before {
        if !captioned.contains(n) {
            assert!(std::fs::read(folder.join(n)).unwrap() == *bytes, "{n}: the original changed");
        }
    }
    assert!(!folder.join(".dalica-originals").exists(), "nothing was saved, so no original was kept aside");
}

fn snapshot(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(folder)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jpg"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}

/// Both workflows in one session of `m`, then the originals checked.
fn workflows(m: &mut Mcp, root: &Path, exports: &Path) -> Value {
    let (lisbon, porto) = (root.join("Lisbon"), root.join("Porto"));
    let before = snapshot(&lisbon);
    let (w02, section) = w02(m, &lisbon, &porto);
    let w01 = w01(m, &lisbon, &section, exports);
    let captioned: Vec<String> = (SMALL + 1..=SMALL + LARGE).map(name).take(CAPTIONS.len()).collect();
    originals(&lisbon, &before, &captioned);
    json!({ "W02": w02, "W01": w01 })
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// W01 and W02 headless, then in connect mode against `dalica --headless --control`; the two
/// outcomes are equal. Returns the seconds each mode took.
pub fn agree(dalica: &Path, home: &Path) -> (f64, f64) {
    let cli = cli_exe();

    // Headless: `dalica-cli mcp --library`, the folders under its read root, exports under its
    // write root.
    let t = Instant::now();
    let h = home.join("headless");
    let (photos, exports) = (h.join("Photos"), h.join("Exports"));
    folders(&photos);
    std::fs::create_dir_all(&exports).unwrap();
    let library = h.join("library");
    let args: Vec<&std::ffi::OsStr> = vec![
        "--library".as_ref(),
        library.as_os_str(),
        "--automation-read-root".as_ref(),
        photos.as_os_str(),
        "--automation-write-root".as_ref(),
        exports.as_os_str(),
        "--no-client-roots".as_ref(),
    ];
    let mut m = Mcp::spawn(&cli, &h, &args);
    let headless = workflows(&mut m, &photos, &exports);
    m.finish();
    let headless_s = secs(t);
    println!("  headless (dalica-cli mcp --library): W02 and W01 passed in {headless_s:.1} s");

    // Connect: the app runs with its own policy; `dalica-cli mcp --connect` drives it.
    let t = Instant::now();
    let k = home.join("connect");
    let (photos, exports) = (k.join("Photos"), k.join("Exports"));
    folders(&photos);
    std::fs::create_dir_all(&exports).unwrap();
    let (port, token) = (free_port(), k.join("agent.token"));
    let mut cmd = Command::new(dalica);
    let p = port.to_string();
    cmd.args(["--headless", "--control", &p, "--library"]).arg(k.join("library"));
    cmd.arg("--control-token-file").arg(&token).arg("--automation-read-root").arg(&photos).arg("--automation-write-root").arg(&exports);
    contain(&mut cmd, &k);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(k.join("stderr.log")).unwrap();
    let mut app = Proc(cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log)).spawn().unwrap());
    let end = Instant::now() + PATIENCE;
    while std::fs::read_to_string(&token).map_or(true, |t| t.trim().len() != 64) {
        assert!(app.0.try_wait().unwrap().is_none(), "dalica exited early");
        assert!(Instant::now() < end, "no token file at {}", token.display());
        std::thread::sleep(Duration::from_millis(50));
    }
    let addr = format!("127.0.0.1:{port}");
    let args: Vec<&std::ffi::OsStr> = vec!["--connect".as_ref(), addr.as_ref(), "--control-token-file".as_ref(), token.as_os_str()];
    let mut m = Mcp::spawn(&cli, &k, &args);
    let tools = m.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"screenshot") && names.contains(&"click"), "connect mode adds the UI tools");
    let connect = workflows(&mut m, &photos, &exports);
    let quit = m.tool("control_call", json!({ "method": "app.quit", "params": {} }));
    m.finish();
    let end = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = app.0.try_wait().unwrap() {
            assert!(status.success(), "dalica quit with {status} ({quit})");
            break;
        }
        assert!(Instant::now() < end, "dalica did not quit");
        std::thread::sleep(Duration::from_millis(50));
    }
    let connect_s = secs(t);
    println!("  connect (dalica-cli mcp --connect to dalica --headless --control): W02 and W01 passed in {connect_s:.1} s");

    assert_eq!(headless, connect, "both modes agree");
    println!("  both modes agree: {}", serde_json::to_string(&connect["W01"]["files"]).unwrap());
    (headless_s, connect_s)
}
