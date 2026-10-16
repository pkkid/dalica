//! The workflow smoke run over real stdio (`mcp.md` §9; `process.md` §5): `dalica-cli mcp` spawned
//! as a process on a persistent library, driven over its standard input and output as an MCP
//! client drives it, through the steps of two workflows: W02 (the Photo Tray across two folders,
//! rotated, both folders' sidecars, one undo) and W03 (Save, Undo Save, Revert to the exact original
//! bytes). The workflows themselves, with all their checks, run in-process in `dalica-mcp`'s
//! `workflows` test binary; this proves the same tools answer the same through the binary, and that
//! its standard output carries nothing but JSON-RPC messages.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How long one reply, or one background write (a sidecar after its debounce), may take.
const PATIENCE: Duration = Duration::from_secs(60);
/// The folders' sidecar and the folder that keeps originals aside on a Save (`data-model.md` §4,
/// §6), named here rather than taken from `dalica-sidecar`: the names on disk are part of the check.
const SIDECAR: &str = ".dalica.json";
const ORIGINALS: &str = ".dalica-originals";

/// `dalica-cli mcp` as a child process, with a reader thread turning its standard output into lines.
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    /// Every line the server wrote to standard output, checked at the end.
    seen: Vec<String>,
    next: u64,
}

impl Server {
    fn spawn(args: &[&std::ffi::OsStr], temp: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dalica-cli"))
            .arg("mcp")
            .args(args)
            .env("DALICA_CACHE_DIR", temp.join("cache"))
            .envs(["TMPDIR", "TMP", "TEMP"].map(|k| (k, temp)))
            .env("DALICA_LOG", "info")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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
        Server { child, input, lines, seen: Vec::new(), next: 0 }
    }

    fn send(&mut self, msg: &Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
    }

    /// Sends request `method` and waits for its reply, skipping notifications (progress).
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let start = Instant::now();
        loop {
            let left = PATIENCE.checked_sub(start.elapsed()).unwrap_or_default();
            let line = self.lines.recv_timeout(left).unwrap_or_else(|e| panic!("{method}: no reply ({e})"));
            self.seen.push(line.clone());
            let msg: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC on standard output ({e}): {line}"));
            if msg["id"] == json!(id) {
                assert!(msg.get("error").is_none(), "{method} {params}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    /// Calls tool `name`, failing on a tool error; answers its `structuredContent`.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    fn section_ids(&mut self, section: &str) -> Vec<u64> {
        let r = self.tool("doc_inspect", json!({ "photos": section, "limit": 1000 }));
        r["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect()
    }

    fn name(&mut self, id: u64) -> String {
        self.tool("photo_info", json!({ "id": id }))["name"].as_str().unwrap().to_string()
    }

    /// Closes standard input (the client goes away) and waits for the process: it must exit
    /// cleanly, having written only JSON-RPC to standard output. Answers its standard error.
    fn finish(mut self) -> String {
        drop(self.input.take());
        let out = self.child.wait_with_output().unwrap();
        assert!(out.status.success(), "{out:?}");
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(5)) {
            self.seen.push(line);
        }
        for line in &self.seen {
            let msg: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON-RPC on standard output ({e}): {line}"));
            assert_eq!(msg["jsonrpc"], json!("2.0"), "{line}");
        }
        String::from_utf8_lossy(&out.stderr).into_owned()
    }
}

/// A folder `name` of `n` procedural JPEGs (`IMG_0001.jpg` …).
fn folder(root: &Path, name: &str, n: u64, seed: u64) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..n {
        let scene = dalica_scenes::Scene::new(seed + i, 160, 120);
        std::fs::write(dir.join(format!("IMG_{:04}.jpg", i + 1)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
    }
    dir
}

/// The edits photo `name`'s entry in `dir`'s sidecar records (none when it has no entry).
fn edits(dir: &Path, name: &str) -> Vec<String> {
    let sidecar: Value = std::fs::read(dir.join(SIDECAR)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    sidecar
        .pointer(&format!("/photos/{name}/edits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Waits until every photo of `names` in `dir` records exactly `want` in the sidecar.
fn wait_edits(dir: &Path, names: &[String], want: &[&str]) {
    let start = Instant::now();
    while !names.iter().all(|n| edits(dir, n) == want) {
        assert!(start.elapsed() < PATIENCE, "{}: {names:?} do not record {want:?}", dir.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn mcp_process_runs_the_tray_and_save_workflows_over_stdio() {
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("Photos");
    let a = folder(&photos, "Lisbon", 4, 0);
    let b = folder(&photos, "Porto", 4, 100);
    let c = folder(&photos, "Trams", 2, 7);
    let library = temp.path().join("library");
    let args = ["--library".as_ref(), library.as_os_str(), "--automation-read-root".as_ref(), photos.as_os_str(), "--no-client-roots".as_ref()];
    let mut server = Server::spawn(&args, temp.path());
    let init = server
        .request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "smoke", "version": "0" } }));
    assert_eq!(init["serverInfo"]["name"], json!("dalica"), "{init}");
    server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let [sa, sb, sc] = [&a, &b, &c].map(|dir| server.tool("scan_folder", json!({ "path": dir }))["section"].as_str().unwrap().to_string());
    let (in_a, in_b) = (server.section_ids(&sa), server.section_ids(&sb));
    assert_eq!((in_a.len(), in_b.len()), (4, 4));

    // W02: two held in Lisbon, three selected in Porto, rotated together, undone in one step.
    server.tool("select_photos", json!({ "ids": &in_a[..2], "section": sa, "hold": true }));
    let tray = server.tool("select_photos", json!({ "ids": &in_b[..3], "section": sb }));
    assert_eq!(tray["tray"].as_array().map(Vec::len), Some(5), "{tray}");
    let names_a: Vec<String> = in_a[..2].iter().map(|id| server.name(*id)).collect();
    let names_b: Vec<String> = in_b[..3].iter().map(|id| server.name(*id)).collect();
    server.tool("cmd_photo_rotate_cw", json!({}));
    wait_edits(&a, &names_a, &["rotate(90)"]);
    wait_edits(&b, &names_b, &["rotate(90)"]);
    server.tool("cmd_edit_undo", json!({}));
    wait_edits(&a, &names_a, &[]);
    wait_edits(&b, &names_b, &[]);

    // W03: a crop saved (the original kept aside), Undo Save, saved again, reverted.
    let id = server.section_ids(&sc)[0];
    let name = server.name(id);
    let (path, backup) = (c.join(&name), c.join(ORIGINALS).join(&name));
    let original = std::fs::read(&path).unwrap();
    server.tool("apply_edit", json!({ "ids": [id], "op": "crop", "params": { "rect": [0.1, 0.1, 0.9, 0.9] } }));
    server.tool("cmd_photo_save", json!({ "ids": [id], "confirm": true }));
    assert_eq!(std::fs::read(&backup).unwrap(), original, "Save keeps the original aside");
    assert_ne!(std::fs::read(&path).unwrap(), original, "the file holds the crop");
    server.tool("cmd_photo_undoSave", json!({ "ids": [id] }));
    assert_eq!(std::fs::read(&path).unwrap(), original, "Undo Save puts the original back");
    server.tool("cmd_photo_save", json!({ "ids": [id], "confirm": true }));
    server.tool("cmd_photo_revert", json!({ "ids": [id], "confirm": true }));
    assert_eq!(std::fs::read(&path).unwrap(), original, "Revert returns the exact original bytes");
    assert!(!backup.exists(), "the kept original went back into place");

    let stderr = server.finish();
    assert!(stderr.contains("dalica-cli mcp:"), "the logs go to standard error: {stderr}");
    assert!(library.is_dir(), "the persistent library stays");
}
