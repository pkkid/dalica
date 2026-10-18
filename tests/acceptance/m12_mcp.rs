//! M12's workflow through the real `dalica-cli mcp` over standard input and output (`m12.rs` part
//! 6), as an MCP client drives it, on a new library with automation reading inside the test's
//! folder (`mcp.md` §9):
//!
//! - **W11, effects stacked**: `list_edit_ops` lists the 27 effects, every one available (each
//!   renders now); Sepia, Vignette and Border applied through `apply_edit` on one photo keep that
//!   order in the recipe (`photo_info`, `get_recipe`) and the folder's sidecar; `render_photo`
//!   keeps the photo's size through Sepia and Vignette and grows it by the Border's bands on every
//!   side, at full size and scaled down, black outside and white inside with the toned photo
//!   copied unchanged within; the photo file is never touched.
//!
//! The same workflow runs in-process in `dalica-mcp`'s `workflows` binary, where the pass rate and
//! its floor of 12 are counted (`cargo xtask stats --check`, `m12_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::secs;
use crate::common::{decode, px};

/// How long one reply, or one background write (a sidecar after its debounce), may take.
const PATIENCE: Duration = Duration::from_secs(120);
const PHOTO: (u32, u32) = (160, 120);
/// The Border applied: outer and inner bands of 80 and 20 thousandths of the shorter side, so 12
/// px on every side.
const BORDER: (u32, u32) = (80, 20);

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
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m12", "version": "0" } }),
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

    /// `render_photo` as a PNG, decoded.
    fn render(&mut self, args: Value) -> dalica_image::Rgba8 {
        let r = self.call("render_photo", args);
        assert_eq!(r["isError"], json!(false), "{r}");
        let block = &r["content"][0];
        assert_eq!(block["type"], "image", "{r}");
        let img = decode(&base64(block["data"].as_str().unwrap()));
        let meta = &r["structuredContent"];
        assert_eq!((meta["width"].as_u64(), meta["height"].as_u64()), (Some(u64::from(img.width)), Some(u64::from(img.height))), "{meta}");
        img
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

/// Standard base64 (with padding) decoded.
fn base64(text: &str) -> Vec<u8> {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes().filter(|b| *b != b'=' && !b.is_ascii_whitespace()) {
        let v = ABC.iter().position(|a| *a == b).unwrap_or_else(|| panic!("not base64: {b}")) as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// Waits until `holds`; panics naming `what` after [`PATIENCE`].
fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    let start = Instant::now();
    while !holds() {
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
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

/// W11 in one `dalica-cli mcp --library` session over real stdio.
pub fn w11(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    // Resolved and plain, as the paths the engine answers with.
    let root = dalica_engine::automation::plain(&std::fs::canonicalize(home).unwrap()).join("work");
    let folder = root.join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    let name = "IMG_0001.jpg";
    std::fs::write(folder.join(name), dalica_scenes::jpeg(&dalica_scenes::Scene::new(1100, PHOTO.0, PHOTO.1), 90, None).unwrap()).unwrap();
    let original = std::fs::read(folder.join(name)).unwrap();
    let library = root.join("library");
    let args: Vec<&std::ffi::OsStr> =
        vec!["--library".as_ref(), library.as_os_str(), "--automation-read-root".as_ref(), root.as_os_str(), "--no-client-roots".as_ref()];
    let mut m = Mcp::spawn(&cli, home, &args);

    let listed = m.tool("list_edit_ops", json!({}));
    let effects = dalica_edit::catalog::catalog().iter().filter(|op| op.effect.is_some()).count();
    assert_eq!(effects, 27, "the catalogue's effects");
    let ops = listed["ops"].as_array().unwrap();
    for op in dalica_edit::catalog::catalog().iter().filter(|op| op.effect.is_some()) {
        let row = ops.iter().find(|r| r["name"] == op.name).unwrap_or_else(|| panic!("list_edit_ops leaves out {}: {listed}", op.name));
        assert_eq!(row["available"], json!(true), "{} is available: {row}", op.name);
    }

    let section = m.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
    let photos = m.tool("doc_inspect", json!({ "photos": section, "limit": 10 }));
    let ids: Vec<u64> = photos["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let [id] = ids[..] else { panic!("{section}: {ids:?}") };

    let plain = m.render(json!({ "id": id, "format": "png" }));
    assert_eq!((plain.width, plain.height), PHOTO, "the photo at its own size");
    let sepia = m.tool("apply_edit", json!({ "ids": [id], "op": "sepia" }));
    let vignette = m.tool("apply_edit", json!({ "ids": [id], "op": "vignette" }));
    let toned = m.render(json!({ "id": id, "format": "png" }));
    assert_eq!((toned.width, toned.height), PHOTO, "Sepia and Vignette keep the size");
    let border = m.tool("apply_edit", json!({ "ids": [id], "op": "border", "params": { "outer": BORDER.0, "inner": BORDER.1 } }));
    let stack: Vec<Value> = [&sepia, &vignette, &border].iter().map(|a| a["entry"].clone()).collect();
    for (entry, op) in stack.iter().zip(["sepia(", "vignette(", "border("]) {
        assert!(entry.as_str().is_some_and(|e| e.starts_with(op)), "{stack:?}");
    }
    let info = m.tool("photo_info", json!({ "id": id }));
    assert_eq!(info["recipe"]["list"], json!(stack), "the recipe in the order applied: {info}");
    let recipe = m.tool("get_recipe", json!({ "id": id }));
    let names: Vec<&str> = recipe["ops"].as_array().unwrap().iter().filter_map(|o| o["op"].as_str()).collect();
    assert_eq!(names, ["sepia", "vignette", "border"], "{recipe}");
    let want: Vec<String> = stack.iter().filter_map(Value::as_str).map(str::to_string).collect();
    eventually("the sidecar records the three effects in order", || sidecar_edits(&folder, name) == want);

    let band = (BORDER.0 + BORDER.1) * PHOTO.0.min(PHOTO.1) / 1000;
    let framed = m.render(json!({ "id": id, "format": "png" }));
    assert_eq!((framed.width, framed.height), (PHOTO.0 + 2 * band, PHOTO.1 + 2 * band), "the Border's bands around the photo");
    let half = (PHOTO.0 + 2 * band) / 2;
    let scaled = m.render(json!({ "id": id, "size": half, "format": "png" }));
    assert_eq!((scaled.width, scaled.height), (half, (PHOTO.1 + 2 * band) / 2), "the scaled render keeps the framed shape");
    assert_eq!(px(&framed.data, framed.width, 0, 0), [0, 0, 0], "the outer band");
    assert_eq!(px(&framed.data, framed.width, band - 1, framed.height / 2), [255, 255, 255], "the inner band");
    assert_eq!(
        px(&framed.data, framed.width, band + PHOTO.0 / 2, band + PHOTO.1 / 2),
        px(&toned.data, toned.width, PHOTO.0 / 2, PHOTO.1 / 2),
        "the toned photo copied unchanged inside"
    );
    assert!(std::fs::read(folder.join(name)).unwrap() == original, "the photo file is untouched");
    m.finish();
    println!(
        "  W11: list_edit_ops lists the 27 effects, all available; Sepia, Vignette and Border stacked in order, rendered at {} × {} (grown by {band} px a side); the file untouched",
        framed.width, framed.height
    );
    println!("  M12 W11 over stdio in {:.1} s", secs(started));
}
