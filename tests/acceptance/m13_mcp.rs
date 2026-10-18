//! M13's workflows through the real `dalica-cli mcp` over standard input and output (`m13.rs` part
//! 5), as an MCP client drives it, on new libraries with automation reading inside the test's
//! folder (`mcp.md` §5 and §9):
//!
//! - **W13, geotags**: `set_metadata {geo}` gives three photos of five a place in one undo step; the
//!   GPS reaches their files (read back by the independent reader) and no other; `photo_info` and
//!   `query_photos {geotagged: true}` report them; `set_metadata {geo: null}` clears it and the GPS
//!   leaves the files; one `cmd_edit_undo` brings it back, in the library and the files.
//! - **W14, people**: started with `--automation-allow-people` and Store people names in photos on,
//!   the faces planted in four photos (by the folder's sidecar, as a scan finds them) named Ana by
//!   one `cmd_faces_name`; `query_photos {query: "person:Ana"}` returns those four; their files hold
//!   Ana's MWG face region where the face is, a fifth photo's unnamed face none. A second session
//!   without the permission answers names `null` and no rectangles, counts kept, and `person:Ana`
//!   finds nothing.
//!
//! The same workflows run in-process in `dalica-mcp`'s `workflows` binary, where the pass rate and
//! its floor of 14 are counted (`cargo xtask stats --check`, `m13_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use dalica_meta::Limits;
use serde_json::{Value, json};

use crate::app::secs;

/// How long one reply, or one background write, may take.
const PATIENCE: Duration = Duration::from_secs(120);
/// Lisbon, Praça do Comércio.
const LISBON: [f64; 2] = [38.707_5, -9.136_6];
/// A file's GPS and regions round through rationals and a centre and size.
const CLOSE: f64 = 1e-5;
/// The face planted in each of the first five photos of W14.
const FACE: [f64; 4] = [0.3, 0.2, 0.6, 0.7];

/// `dalica-cli`, built for this test run.
fn cli_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// The environment of a process whose every folder lies inside `home`.
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
        ("DALICA_FACE_MODELS", home.join("models")),
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
}

/// `dalica-cli mcp ARGS`, driven over its standard input and output; killed if the test ends first.
struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    next: u64,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Mcp {
    fn spawn(cli: &Path, home: &Path, args: &[&std::ffi::OsStr]) -> Mcp {
        let mut cmd = Command::new(cli);
        cmd.arg("mcp").args(args);
        contain(&mut cmd, home);
        let log = std::fs::File::create(home.join("mcp-stderr.log")).unwrap();
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
        let mut m = Mcp { child, input, lines, next: 0 };
        let init = m.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m13", "version": "0" } }),
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

    /// Requests `method`; its result, skipping notifications. Standard output carries only JSON-RPC.
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

    /// The ids `query_photos` answers for `args`, sorted.
    fn query(&mut self, mut args: Value) -> Vec<u64> {
        args["limit"] = json!(1000);
        sorted(ids(&self.tool("query_photos", args)))
    }

    /// `scan_folder` then `doc_inspect`: the folder's photo ids in light-table order.
    fn scan(&mut self, folder: &Path) -> Vec<u64> {
        let section = self.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
        ids(&self.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })))
    }

    /// Closes standard input and waits for the process to exit cleanly.
    fn finish(mut self) {
        drop(self.input.take());
        let end = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "dalica-cli mcp exited with {status}");
                return;
            }
            assert!(Instant::now() < end, "dalica-cli mcp did not exit after its input closed");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn ids(answer: &Value) -> Vec<u64> {
    answer["photos"].as_array().unwrap_or_else(|| panic!("no photos in {answer}")).iter().map(|p| p["id"].as_u64().unwrap()).collect()
}

fn sorted(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v
}

/// Waits until `holds`; panics naming `what` (and `state`) after [`PATIENCE`].
fn eventually(what: &str, mut holds: impl FnMut() -> bool, state: impl Fn() -> String) {
    let start = Instant::now();
    while !holds() {
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}: {}", state());
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A folder of `n` procedural JPEGs (`IMG_0001.jpg` …) in `root`.
fn folder(root: &Path, name: &str, n: u64, seed: u64) -> (PathBuf, Vec<PathBuf>) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let files: Vec<PathBuf> = (1..=n).map(|i| dir.join(format!("IMG_{i:04}.jpg"))).collect();
    for (i, f) in files.iter().enumerate() {
        std::fs::write(f, dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed + i as u64, 160, 120), 90, None).unwrap()).unwrap();
    }
    (dir, files)
}

/// The arguments of a `dalica-cli mcp` session on `library`, reading inside `root`.
fn args<'a>(library: &'a Path, root: &'a Path, people: bool) -> Vec<&'a std::ffi::OsStr> {
    let mut a: Vec<&std::ffi::OsStr> =
        vec!["--library".as_ref(), library.as_os_str(), "--automation-read-root".as_ref(), root.as_os_str(), "--no-client-roots".as_ref()];
    if people {
        a.push("--automation-allow-people".as_ref());
    }
    a
}

fn gps(path: &Path) -> Option<[f64; 2]> {
    let read = dalica_meta::read(&std::fs::read(path).ok()?, &Limits::default()).ok()?;
    read.owned.place.map(|p| [p.latitude(), p.longitude()])
}

fn at_lisbon(p: Option<[f64; 2]>) -> bool {
    p.is_some_and(|[lat, lon]| (lat - LISBON[0]).abs() < CLOSE && (lon - LISBON[1]).abs() < CLOSE)
}

/// The resolved, plain form of `home` (as the engine answers paths).
fn root_of(home: &Path) -> PathBuf {
    dalica_engine::automation::plain(&std::fs::canonicalize(home).unwrap())
}

/// W13 in one `dalica-cli mcp` session over real stdio.
pub fn w13(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    let root = root_of(home).join("w13");
    let (dir, files) = folder(&root, "Lisbon", 5, 1310);
    let library = root.join("library");
    let mut m = Mcp::spawn(&cli, home, &args(&library, &root, false));
    let all = m.scan(&dir);
    assert_eq!(all.len(), 5, "{all:?}");
    let (three, two) = all.split_at(3);
    assert!(m.query(json!({ "geotagged": true })).is_empty());
    let depth = m.tool("doc_inspect", json!({}))["undo"]["depth"].as_u64().unwrap();
    let set = m.tool("set_metadata", json!({ "ids": three, "geo": LISBON }));
    assert_eq!(set["results"][0]["changed"], json!(3), "{set}");
    assert_eq!(m.tool("doc_inspect", json!({}))["undo"]["depth"], json!(depth + 1), "one undo step");
    let state = || format!("{:?}", files.iter().map(|f| gps(f)).collect::<Vec<_>>());
    eventually(
        "the GPS of the three is in their files, and only theirs",
        || files[..3].iter().all(|f| at_lisbon(gps(f))) && files[3..].iter().all(|f| gps(f).is_none()),
        state,
    );
    for id in three {
        let info = m.tool("photo_info", json!({ "id": id }));
        let g = &info["geotag"];
        assert!(at_lisbon(g[0].as_f64().zip(g[1].as_f64()).map(|(a, b)| [a, b])), "photo {id}: {info}");
    }
    assert_eq!(m.query(json!({ "geotagged": true })), three);
    assert_eq!(m.query(json!({ "geotagged": false })), two);
    m.tool("set_metadata", json!({ "ids": three, "geo": null }));
    eventually("the GPS is gone from every file", || files.iter().all(|f| gps(f).is_none()), state);
    assert!(m.query(json!({ "geotagged": true })).is_empty());
    m.tool("cmd_edit_undo", json!({}));
    assert_eq!(m.query(json!({ "geotagged": true })), three, "one undo restores the three");
    eventually(
        "one undo writes the GPS back into the three files",
        || files[..3].iter().all(|f| at_lisbon(gps(f))) && files[3..].iter().all(|f| gps(f).is_none()),
        state,
    );
    m.finish();
    println!("  W13 over stdio: three geotagged in one step, in their files; cleared; one undo brings it back ({:.1} s)", secs(started));
}

/// The face regions of the file: (name, whether its box is [`FACE`]).
fn regions(path: &Path) -> Option<Vec<(Option<String>, bool)>> {
    let read = dalica_meta::read(&std::fs::read(path).ok()?, &Limits::default()).ok()?;
    let close = |r: &dalica_core::Rect01| [r.x0() - FACE[0], r.y0() - FACE[1], r.x1() - FACE[2], r.y1() - FACE[3]].iter().all(|d| d.abs() < CLOSE);
    Some(read.owned.faces.iter().map(|f| (f.name.clone(), close(&f.rect))).collect())
}

fn has_rect(face: &Value) -> bool {
    face.get("rect").is_some() || face.get("displayed").is_some()
}

/// W14 in two `dalica-cli mcp` sessions over real stdio, with and without the people permission.
pub fn w14(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    let root = root_of(home).join("w14");
    let (dir, files) = folder(&root, "Party", 6, 1410);
    let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let photos: serde_json::Map<String, Value> = names[..5].iter().map(|n| (n.clone(), json!({ "faces": [{ "rect": FACE }] }))).collect();
    std::fs::write(dir.join(".dalica.json"), serde_json::to_vec_pretty(&json!({ "dalica": { "version": 1 }, "photos": photos })).unwrap()).unwrap();
    let library = root.join("library");

    let mut m = Mcp::spawn(&cli, home, &args(&library, &root, true));
    m.tool("cmd_options_set", json!({ "values": { "people.storeNamesInPhotos": true } }));
    let all = m.scan(&dir);
    assert_eq!(all.len(), 6, "{all:?}");
    let unnamed = m.tool("cmd_faces_list", json!({ "unnamed": true }));
    let faces = unnamed["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 5, "the five planted faces: {unnamed}");
    assert!(faces.iter().all(|f| has_rect(f) && f["name"].is_null()), "{unnamed}");
    let four: Vec<Value> = faces.iter().filter(|f| all[..4].contains(&f["photo"].as_u64().unwrap())).map(|f| f["id"].clone()).collect();
    assert_eq!(four.len(), 4, "{unnamed}");
    let named = m.tool("cmd_faces_name", json!({ "faces": four, "name": "Ana", "create": true }));
    assert_eq!((&named["created"], &named["changed"]), (&json!(true), &json!(4)), "{named}");
    let person = named["person"].as_str().unwrap().to_string();
    let mut want = all[..4].to_vec();
    want.sort_unstable();
    assert_eq!(m.query(json!({ "query": "person:Ana" })), want, "person:Ana finds her four photos");
    let people = m.tool("cmd_people_list", json!({}));
    assert_eq!((&people["people"][0]["name"], &people["people"][0]["count"]), (&json!("Ana"), &json!(4)), "{people}");
    let state = || format!("{:?}", files.iter().map(|p| regions(p)).collect::<Vec<_>>());
    eventually(
        "Ana's face region is in the four files and in no other",
        || {
            files[..4].iter().all(|p| regions(p) == Some(vec![(Some("Ana".to_string()), true)]))
                && files[4..].iter().all(|p| regions(p) == Some(Vec::new()))
        },
        state,
    );
    m.finish();

    let mut m = Mcp::spawn(&cli, home, &args(&library, &root, false));
    let people = m.tool("cmd_people_list", json!({}));
    let ana = &people["people"][0];
    assert_eq!((&ana["id"], &ana["name"], &ana["count"]), (&json!(person), &Value::Null, &json!(4)), "names withheld, counts kept: {people}");
    let theirs = m.tool("cmd_faces_list", json!({ "person": person }));
    let faces = theirs["faces"].as_array().unwrap();
    assert!(faces.len() == 4 && faces.iter().all(|f| f["name"].is_null() && !has_rect(f)), "no names, no rectangles: {theirs}");
    assert!(m.query(json!({ "query": "person:Ana" })).is_empty(), "without the permission person:Ana finds nothing");
    m.finish();
    println!(
        "  W14 over stdio: four faces named Ana in one call, found by person:Ana, her regions in their files; without the permission no names, rectangles or matches ({:.1} s)",
        secs(started)
    );
}
