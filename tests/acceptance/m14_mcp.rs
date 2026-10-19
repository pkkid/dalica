//! M14's workflows through the real `dalica-cli mcp` over standard input and output (`m14.rs` part
//! 3), as an MCP client drives it, on new libraries with automation reading inside the test's
//! folder (`mcp.md` §5 and §9):
//!
//! - **W15, a collage**: `cmd_create_collage` opens the room with eight photos and `cmd_collage_set`
//!   makes it a grid; `cmd_collage_create`, with no folder named, writes a JPEG of the page's ratio
//!   at 3000 px on its long side and its `.dalica-collage` recipe beside it in
//!   `<Pictures>/Dalica/Collages` (the write root given), the photos untouched; the new photo is in
//!   the library, its folder the Collages project in the Projects group; `cmd_collage_edit` reopens
//!   the room with the eight clips in the grid.
//! - **W16, a legacy library**: `scan_folder` on a folder holding only the previous organizer's
//!   sidecar shows its stars, captions, a recipe keeping an operation Dalica has no equivalent for
//!   as a `legacy:` entry, an album and a person (`--automation-allow-people`); the scan writes
//!   nothing; `set_metadata` starring one photo writes `.dalica.json` holding every photo while the
//!   legacy file stays byte-identical, and `cmd_legacy_inspect` then says `both`.
//!
//! The same workflows run in-process in `dalica-mcp`'s `workflows` binary, where the pass rate and
//! its floor of 16 are counted (`cargo xtask stats --check`, `m14_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use dalica_sidecar::legacy::sidecar_names;
use serde_json::{Value, json};

use crate::app::secs;
use crate::common;

/// How long one reply, or one background write, may take.
const PATIENCE: Duration = Duration::from_secs(120);
/// The long side Create Collage writes.
const LONG_SIDE: u64 = 3000;

/// `dalica-cli`, built for this test run.
fn cli_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// The environment of a process whose every folder lies inside `home` (its Pictures folder
/// `home/Pictures`).
fn contain(c: &mut Command, home: &Path) {
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
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m14", "version": "0" } }),
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

    /// `scan_folder` then `doc_inspect`: the folder's photo ids.
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

/// Waits until `found` gives something; panics naming `what` (and `state`) after [`PATIENCE`].
fn eventually<T>(what: &str, mut found: impl FnMut() -> Option<T>, state: impl Fn() -> String) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = found() {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}: {}", state());
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A folder of `n` procedural JPEGs (`IMG_0001.jpg` …) in `root`.
fn folder(root: &Path, name: &str, n: u64, seed: u64) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 1..=n {
        std::fs::write(dir.join(photo_name(i)), dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed + i, 160, 120), 90, None).unwrap()).unwrap();
    }
    dir
}

fn photo_name(n: u64) -> String {
    format!("IMG_{n:04}.jpg")
}

/// The resolved, plain form of `home` (as the engine answers paths).
fn root_of(home: &Path) -> PathBuf {
    dalica_engine::automation::plain(&std::fs::canonicalize(home).unwrap())
}

fn sidecar(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(".dalica.json")).unwrap_or_default()).unwrap_or(Value::Null)
}

/// W15 in one `dalica-cli mcp` session over real stdio.
pub fn w15(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    let home = &root_of(home).join("w15");
    let garden = folder(home, "Garden", 8, 1500);
    let originals = common::files(&garden);
    let collages = home.join("Pictures").join("Dalica").join("Collages");
    let (library, write_root) = (home.join("library"), home.join("Pictures").join("Dalica"));
    // A write root is a folder that exists (its links resolved, `control-protocol.md` §5.7).
    std::fs::create_dir_all(&write_root).unwrap();
    let args: Vec<&std::ffi::OsStr> = vec![
        "--library".as_ref(),
        library.as_os_str(),
        "--automation-read-root".as_ref(),
        home.as_os_str(),
        "--automation-write-root".as_ref(),
        write_root.as_os_str(),
        "--no-client-roots".as_ref(),
    ];
    let mut m = Mcp::spawn(&cli, home, &args);
    let eight = sorted(m.scan(&garden));
    assert_eq!(eight.len(), 8, "{eight:?}");

    let opened = m.tool("cmd_create_collage", json!({ "ids": eight }));
    assert_eq!((&opened["added"], &opened["opened"]), (&json!(8), &json!(true)), "{opened}");
    assert_eq!(m.tool("cmd_collage_set", json!({ "style": "grid" }))["changed"], json!(true));
    let room = m.tool("cmd_collage_inspect", json!({}));
    assert_eq!(room["placements"].as_array().map(Vec::len), Some(8), "every clip on the page: {room}");
    let ratio = room["page"]["ratio"].as_f64().unwrap();

    let made = m.tool("cmd_collage_create", json!({}));
    assert_eq!(made["state"], json!("done"), "{made}");
    let jpeg = PathBuf::from(made["result"]["path"].as_str().unwrap_or_else(|| panic!("no path: {made}")));
    let recipe = PathBuf::from(made["result"]["recipe"].as_str().unwrap());
    assert_eq!((jpeg.parent(), recipe.parent()), (Some(collages.as_path()), Some(collages.as_path())), "{made}");
    assert_eq!(recipe.file_stem(), jpeg.file_stem(), "the recipe has the picture's name");
    let bytes = std::fs::read(&jpeg).unwrap();
    let header = dalica_image::read_header(&bytes, &dalica_image::Limits::default()).unwrap();
    let (w, h) = (u64::from(header.width), u64::from(header.height));
    assert_eq!(w.max(h), LONG_SIDE, "{w} × {h}");
    assert!((w as f64 / h as f64 - ratio).abs() <= ratio / w.min(h) as f64, "the page's ratio {ratio}: {w} × {h}");
    let saved: Value = serde_json::from_slice(&std::fs::read(&recipe).unwrap()).unwrap();
    let recorded: Vec<u64> = saved["clips"].as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
    assert_eq!((&saved["style"], sorted(recorded)), (&json!("grid"), eight.clone()), "{saved}");
    assert!(
        common::files(&garden).into_iter().filter(|(n, _)| n.ends_with(".jpg")).eq(originals.into_iter().filter(|(n, _)| n.ends_with(".jpg"))),
        "the photos are untouched"
    );

    // In the library, in the Collages project.
    let section = eventually(
        "the Collages folder in the projects group with the collage",
        || {
            let inspected = m.tool("doc_inspect", json!({}));
            let groups = inspected["tree"]["groups"].as_array()?.clone();
            let projects = groups.iter().find(|g| g["group"] == json!("projects"))?.clone();
            let row = projects["rows"].as_array()?.iter().find(|r| r["path"].as_str().map(Path::new) == Some(collages.as_path()))?.clone();
            (row["count"] == json!(1)).then(|| row["section"].as_str().unwrap().to_string())
        },
        String::new,
    );
    let listed = ids(&m.tool("doc_inspect", json!({ "photos": section, "limit": 10 })));
    assert_eq!(listed.len(), 1, "{listed:?}");
    let info = m.tool("photo_info", json!({ "id": listed[0] }));
    assert_eq!(Path::new(info["path"].as_str().unwrap()), jpeg.as_path(), "{info}");
    assert_eq!(sidecar(&collages)["folder"]["project"], json!("collages"), "the folder's mark");

    let reopened = m.tool("cmd_collage_edit", json!({ "id": listed[0] }));
    assert_eq!(
        (&reopened["state"], &reopened["result"]["opened"], &reopened["result"]["missing"]),
        (&json!("done"), &json!(true), &json!([])),
        "{reopened}"
    );
    let room = m.tool("cmd_collage_inspect", json!({}));
    let clips: Vec<u64> = room["clips"].as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
    assert_eq!((&room["document"]["style"], sorted(clips)), (&json!("grid"), eight), "{room}");
    m.finish();
    println!(
        "  W15 over stdio: a grid of eight written as a {w} × {h} JPEG and its recipe in the Collages project, reopened by Edit Collage ({:.1} s)",
        secs(started)
    );
}

/// The album's token and the contact's id, as the legacy format writes them.
const ALBUM: &str = "5e1f0c2a9b3d4e6f708192a3b4c5d6e7";
const CONTACT: &str = "1a2b3c4d5e6f7081";
const PERSON: &str = "Marta Vieira";
const ALBUM_NAME: &str = "Summer House";

/// W16 in one `dalica-cli mcp` session over real stdio.
pub fn w16(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    let home = &root_of(home).join("w16");
    let summer = folder(home, "Summer", 3, 1600);
    let legacy = summer.join(&sidecar_names()[0]);
    let lines = [
        "[encoding]".to_string(),
        "utf8=1".into(),
        format!("[{}]", photo_name(1)),
        "star=yes".into(),
        "caption=Breakfast on the terrace".into(),
        "filters=bw=1;redeye=1;".into(),
        format!("faces=rect64(4000400080008000),{CONTACT};"),
        format!("albums={ALBUM}"),
        format!("[{}]", photo_name(2)),
        "caption=The garden gate".into(),
        format!("albums={ALBUM}"),
        format!("[{}]", photo_name(3)),
        "star=yes".into(),
        "[Contacts2]".into(),
        format!("{CONTACT}={PERSON};;"),
        format!("[.album:{ALBUM}]"),
        format!("name={ALBUM_NAME}"),
        format!("token={ALBUM}"),
    ];
    std::fs::write(&legacy, lines.join("\r\n") + "\r\n").unwrap();
    let legacy_bytes = std::fs::read(&legacy).unwrap();
    let before = common::files(&summer);
    let library = home.join("library");
    let args: Vec<&std::ffi::OsStr> = vec![
        "--library".as_ref(),
        library.as_os_str(),
        "--automation-read-root".as_ref(),
        home.as_os_str(),
        "--automation-allow-people".as_ref(),
        "--no-client-roots".as_ref(),
    ];
    let mut m = Mcp::spawn(&cli, home, &args);
    let three = m.scan(&summer);
    assert_eq!(three.len(), 3, "{three:?}");
    let mut by_name = std::collections::BTreeMap::new();
    for id in &three {
        let info = m.tool("photo_info", json!({ "id": id }));
        by_name.insert(info["name"].as_str().unwrap().to_string(), (*id, info));
    }
    let (_, info) = by_name[&photo_name(1)].clone();
    assert_eq!((&info["star"], &info["caption"]), (&json!(true), &json!("Breakfast on the terrace")), "{info}");
    assert_eq!(info["recipe"]["list"], json!(["bw()", "legacy:redeye(1)"]), "{info}");
    assert_eq!(info["albums"][0]["name"], json!(ALBUM_NAME), "{info}");
    assert_eq!(info["faces"][0]["name"], json!(PERSON), "{info}");
    let (second, info) = by_name[&photo_name(2)].clone();
    assert_eq!(
        (&info["star"], &info["caption"], &info["albums"][0]["name"]),
        (&json!(false), &json!("The garden gate"), &json!(ALBUM_NAME)),
        "{info}"
    );
    let source = |m: &mut Mcp| m.tool("cmd_legacy_inspect", json!({}))["folders"][0]["source"].as_str().unwrap_or_default().to_string();
    assert_eq!(source(&mut m), "legacy");
    assert_eq!(common::files(&summer), before, "the scan writes nothing");

    let set = m.tool("set_metadata", json!({ "ids": [second], "star": true }));
    assert_eq!(set["changed"], json!(["star"]), "{set}");
    let names: Vec<String> = (1..=3).map(photo_name).collect();
    eventually(
        "the first write gives the folder a .dalica.json holding every photo",
        || names.iter().all(|n| sidecar(&summer)["photos"][n]["star"] == json!(true)).then_some(()),
        || format!("{:#}", sidecar(&summer)),
    );
    let entry = &sidecar(&summer)["photos"][photo_name(1)];
    assert_eq!((&entry["caption"], &entry["edits"]), (&json!("Breakfast on the terrace"), &json!(["bw()", "legacy:redeye(1)"])), "{entry}");
    assert!(sidecar(&summer).to_string().contains(ALBUM_NAME), "the album");
    assert_eq!(std::fs::read(&legacy).unwrap(), legacy_bytes, "the legacy file is byte-identical");
    let mut now = common::files(&summer);
    now.remove(".dalica.json");
    assert!(now == before, "nothing but Dalica's own sidecar was written");
    eventually("the folder's source turns both", || (source(&mut m) == "both").then_some(()), String::new);
    m.finish();
    println!(
        "  W16 over stdio: the legacy folder's stars, captions, legacy: recipe, album and person; the first change wrote .dalica.json, the legacy file byte-identical ({:.1} s)",
        secs(started)
    );
}
