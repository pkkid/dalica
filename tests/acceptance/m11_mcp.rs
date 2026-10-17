//! M11's two workflows through the real `dalica-cli mcp` over standard input and output
//! (`m11.rs` part 5), as an MCP client drives it, on a new library with automation reading inside
//! the test's folder and writing into its `Pictures` (`mcp.md` §9):
//!
//! - **W10, import with duplicates**: a card-like folder (`EOS_DIGITAL/DCIM/100CANON`, six shots on
//!   two days) listed and imported through the `cmd_import_*` tools into one folder per capture date,
//!   every copy holding its source's bytes; listed and imported again, every shot is a duplicate,
//!   nothing is copied and the six are reported as skipped duplicates; removing the copies from the
//!   card is refused over MCP and the card keeps every shot;
//! - **W07, batch edit is one step**: Auto Fix through `apply_edit` on the 20 photos of a folder held
//!   in the Photo Tray: every recipe ends in `autofix()` in the library and the folder's sidecar, one
//!   undo step "Auto Fix", and one `cmd_edit_undo` takes all 20 back; no photo file changes.
//!
//! The same workflows run in-process in `dalica-mcp`'s `workflows` binary, where the pass rate and
//! its floor of 11 are counted (`cargo xtask stats --check`, `m11_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::secs;

/// How long one reply, or one background write (a sidecar after its debounce), may take.
const PATIENCE: Duration = Duration::from_secs(120);
const SHOTS: u64 = 6;
const DAYS: [&str; 2] = ["2026-06-04", "2026-06-05"];
const BATCH: u64 = 20;

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
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m11", "version": "0" } }),
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

    /// Calls tool `name`: the whole result (`isError`, `structuredContent`, `content`).
    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({ "name": name, "arguments": args }))
    }

    /// Calls tool `name`, failing on a tool error; its `structuredContent`.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    fn ids(answer: &Value) -> Vec<u64> {
        answer["photos"].as_array().unwrap_or_else(|| panic!("no photos in {answer}")).iter().map(|p| p["id"].as_u64().unwrap()).collect()
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

/// Waits until `holds`; panics naming `what` after [`PATIENCE`].
fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    let start = Instant::now();
    while !holds() {
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A procedural JPEG of scene `seed` taken `secs` after 2026-06-04T08:00:00.
fn shot(seed: u64, secs: i64) -> Vec<u8> {
    let at = dalica_core::DateTime::from_unix_secs(1_780_560_000 + secs).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(at), ..dalica_scenes::Exif::default() };
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 85, Some(&exif)).unwrap()
}

/// Opens `dcim` in the Import room and waits for its listing to end; its last answer.
fn list(m: &mut Mcp, dcim: &Path) -> Value {
    let opened = m.tool("cmd_import_openSource", json!({ "path": dcim }));
    assert_eq!(opened["source"]["path"], json!(dcim), "{opened}");
    let start = Instant::now();
    loop {
        let v = m.tool("cmd_import_list", json!({}));
        if v["state"] != "loading" {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "the listing never ended: {v}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn jpgs(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "JPG")).count()
}

/// W10 on `root`: the card at `root/EOS_DIGITAL`, imported into `pictures`.
fn w10(m: &mut Mcp, root: &Path, pictures: &Path) {
    let dcim = root.join("EOS_DIGITAL").join("DCIM");
    let shots: Vec<(String, Vec<u8>)> =
        (0..SHOTS).map(|i| (format!("IMG_{:04}.JPG", i + 1), shot(800 + i, ((i / 3) * 86_400 + (i % 3) * 60) as i64))).collect();
    std::fs::create_dir_all(dcim.join("100CANON")).unwrap();
    for (name, b) in &shots {
        std::fs::write(dcim.join("100CANON").join(name), b).unwrap();
    }
    let listed = list(m, &dcim);
    assert_eq!((listed["state"].clone(), listed["total"].clone(), listed["duplicates"].clone()), (json!("done"), json!(SHOTS), json!(0)), "{listed}");
    let run = json!({ "destination": pictures, "folderName": { "dateTaken": true }, "after": "leave" });
    m.tool("cmd_import_run", run.clone());
    let status = m.tool("cmd_import_status", json!({}));
    let days: Vec<PathBuf> = DAYS.iter().map(|d| pictures.join(d)).collect();
    assert_eq!((status["state"].clone(), status["imported"].clone(), status["failed"].clone()), (json!("done"), json!(SHOTS), json!([])), "{status}");
    assert_eq!(status["folders"], json!(days), "one folder per capture date: {status}");
    for (i, (name, b)) in shots.iter().enumerate() {
        let copy = days[i / 3].join(name);
        assert!(std::fs::read(&copy).unwrap() == *b, "{} holds its source's bytes", copy.display());
    }
    let again = list(m, &dcim);
    assert_eq!((again["duplicates"].clone(), again["toImport"].clone()), (json!(SHOTS), json!(0)), "{again}");
    m.tool("cmd_import_run", run);
    let status = m.tool("cmd_import_status", json!({}));
    assert_eq!((status["imported"].clone(), status["skipped"]["duplicates"].clone()), (json!(0), json!(SHOTS)), "{status}");
    for day in &days {
        assert_eq!(jpgs(day), 3, "nothing copied again into {}", day.display());
    }
    let removed = m.call("cmd_import_removeFromSource", json!({ "confirm": true }));
    assert_eq!(removed["isError"], json!(true), "never over MCP: {removed}");
    assert!(shots.iter().all(|(name, _)| dcim.join("100CANON").join(name).exists()), "the card keeps every shot");
    println!("  W10: {SHOTS} shots imported into {} and {}, then {SHOTS} skipped as duplicates; removal refused", DAYS[0], DAYS[1]);
}

fn sidecar_edits(folder: &Path, name: &str) -> Vec<String> {
    let v: Value = std::fs::read(folder.join(".dalica.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    v.pointer(&format!("/photos/{name}/edits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// W07 on the folder `root/Market` of [`BATCH`] photos.
fn w07(m: &mut Mcp, root: &Path) {
    let folder = root.join("Market");
    std::fs::create_dir_all(&folder).unwrap();
    let names: Vec<String> = (1..=BATCH).map(|n| format!("IMG_{n:04}.jpg")).collect();
    for (i, n) in names.iter().enumerate() {
        std::fs::write(folder.join(n), dalica_scenes::jpeg(&dalica_scenes::Scene::new(700 + i as u64, 160, 120), 90, None).unwrap()).unwrap();
    }
    let originals: Vec<Vec<u8>> = names.iter().map(|n| std::fs::read(folder.join(n)).unwrap()).collect();
    let section = m.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
    let ids = Mcp::ids(&m.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })));
    assert_eq!(ids.len() as u64, BATCH, "{section}");
    let t = m.tool("select_photos", json!({ "ids": ids, "section": section }));
    assert_eq!(t["tray"].as_array().map(Vec::len), Some(BATCH as usize), "{t}");
    let applied = m.tool("apply_edit", json!({ "op": "autofix" }));
    assert_eq!(applied["photos"].as_array().map(Vec::len), Some(BATCH as usize), "{applied}");
    for id in &ids {
        let info = m.tool("photo_info", json!({ "id": id }));
        assert_eq!(info["recipe"]["list"], json!(["autofix()"]), "photo {id}: {info}");
    }
    let recorded = |want: &[&str]| names.iter().all(|n| sidecar_edits(&folder, n) == want);
    eventually("the sidecar records autofix() for all 20", || recorded(&["autofix()"]));
    let undo = m.tool("doc_inspect", json!({}))["undo"].clone();
    assert_eq!(undo["undo"], json!("Auto Fix"), "one step for all {BATCH}: {undo}");
    m.tool("cmd_edit_undo", json!({}));
    for id in &ids {
        let info = m.tool("photo_info", json!({ "id": id }));
        assert_eq!(info["recipe"]["list"], json!([]), "photo {id}: {info}");
    }
    eventually("the sidecar records no edits after one undo", || recorded(&[]));
    let now: Vec<Vec<u8>> = names.iter().map(|n| std::fs::read(folder.join(n)).unwrap()).collect();
    assert!(now == originals, "the photo files are untouched");
    println!("  W07: apply_edit Auto Fix on the tray's {BATCH} photos was one step \"Auto Fix\"; one cmd_edit_undo took all {BATCH} back");
}

/// W10, then W07, in one `dalica-cli mcp --library` session over real stdio.
pub fn workflows(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    // Resolved and plain, as the paths the engine answers with.
    let root = dalica_engine::automation::plain(&std::fs::canonicalize(home).unwrap()).join("work");
    let pictures = root.join("Pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    let library = root.join("library");
    let args: Vec<&std::ffi::OsStr> = vec![
        "--library".as_ref(),
        library.as_os_str(),
        "--automation-read-root".as_ref(),
        root.as_os_str(),
        "--automation-write-root".as_ref(),
        pictures.as_os_str(),
        "--no-client-roots".as_ref(),
    ];
    let mut m = Mcp::spawn(&cli, home, &args);
    w10(&mut m, &root, &pictures);
    w07(&mut m, &root);
    m.finish();
    println!("  M11 workflows over stdio in {:.1} s", secs(started));
}
