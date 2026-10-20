//! W17, Video, through the real `dalica-cli mcp` over standard input and output (`m15.rs` part 6;
//! `mcp.md` §9), as an MCP client drives it, on a new library with automation reading inside the
//! test's folder and writing inside it: `scan_folder` on a folder of two photos and two clips (an
//! H.264 one, `clip.mp4`, and an MPEG-4 Part 2 one, `old.mov`, which Dalica cannot play);
//! `photo_info` says each clip is a video of 2 s; `cmd_video_inspect` says the H.264 clip plays
//! and the other does not, with the reason. The H.264 clip plays (the session's silent sound),
//! takes `trim(500,1500)` through `cmd_edit_recipe_set`, and `cmd_video_saveFrame` at 1 s writes
//! the frame shown (by its grey) into `<Pictures>/Dalica/Video Frames`, which joins the library;
//! the other clip opens with the reason and its Save Frame is refused, writing nothing.
//! `cmd_export_run` as first frames writes three JPEGs and skips the clip it cannot play with the
//! reason; the `export` tool with `videos: "full"` copies both clips byte for byte. The clips are
//! only read.
//!
//! The same workflow runs in-process in `dalica-mcp`'s `workflows` binary, where the pass rate and
//! its floor of 17 are counted (`cargo xtask stats --check`, `m15_checks.rs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::clips;
use crate::common::{COLOURS, painted_jpeg};

const PATIENCE: Duration = Duration::from_secs(120);
const REASON: &str = "This video can't be played (MPEG-4 Part 2)";
/// The grey the H.264 clip is lit from.
const BASE: u8 = 40;

/// `dalica-cli`, built for this test run.
fn cli_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// `dalica-cli mcp ARGS` with every folder inside `home`, driven over its standard input and
/// output; killed if the test ends first.
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
            cmd.env(k, v);
        }
        for k in [
            "DALICA_HOME",
            "DALICA_CONTROL_PORT",
            "DALICA_AUTOMATION_READ_ROOT",
            "DALICA_AUTOMATION_WRITE_ROOT",
            "DALICA_PROFILE",
            "DALICA_DEVICE_ROOTS",
        ] {
            cmd.env_remove(k);
        }
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
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m15", "version": "0" } }),
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

    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({ "name": name, "arguments": args }))
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    /// The player's state once `done` holds of it.
    fn player_until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let v = self.tool("cmd_video_state", json!({}));
            if done(&v) {
                return v;
            }
            assert!(start.elapsed() < PATIENCE, "{what}: {v}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

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

fn refusal(r: &Value) -> String {
    assert_eq!(r["isError"], json!(true), "refused: {r}");
    r["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

fn project_row(inspected: &Value, path: &Path) -> Option<Value> {
    let groups = inspected["tree"]["groups"].as_array()?;
    let projects = groups.iter().find(|g| g["group"] == json!("projects"))?;
    projects["rows"].as_array()?.iter().find(|r| r["path"].as_str().map(Path::new) == Some(path)).cloned()
}

pub fn w17(home: &Path) {
    let started = Instant::now();
    let cli = cli_exe();
    let home = &dalica_engine::automation::plain(&std::fs::canonicalize(home).unwrap()).join("w17");
    let trip = home.join("Trip");
    std::fs::create_dir_all(&trip).unwrap();
    std::fs::write(trip.join("IMG_0001.jpg"), painted_jpeg(320, 240, COLOURS[0])).unwrap();
    std::fs::write(trip.join("IMG_0002.jpg"), painted_jpeg(320, 240, COLOURS[1])).unwrap();
    let (clip, old) = (trip.join("clip.mp4"), trip.join("old.mov"));
    std::fs::write(&clip, clips::mp4(BASE)).unwrap();
    std::fs::write(&old, clips::mpeg4_part2()).unwrap();
    let originals: Vec<(PathBuf, Vec<u8>)> =
        std::fs::read_dir(&trip).unwrap().flatten().map(|e| (e.path(), std::fs::read(e.path()).unwrap())).collect();
    let frames = home.join("Pictures").join("Dalica").join("Video Frames");
    let library = home.join("library");
    let args: Vec<&std::ffi::OsStr> = vec![
        "--library".as_ref(),
        library.as_os_str(),
        "--automation-read-root".as_ref(),
        home.as_os_str(),
        "--automation-write-root".as_ref(),
        home.as_os_str(),
        "--no-client-roots".as_ref(),
    ];
    let mut m = Mcp::spawn(&cli, home, &args);

    let section = m.tool("scan_folder", json!({ "path": trip }))["section"].as_str().unwrap().to_string();
    let four = ids(&m.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })));
    assert_eq!(four.len(), 4, "{four:?}");
    let mut by_name = std::collections::BTreeMap::new();
    for id in &four {
        let info = m.tool("photo_info", json!({ "id": id }));
        by_name.insert(info["name"].as_str().unwrap().to_string(), (*id, info));
    }
    for name in ["clip.mp4", "old.mov"] {
        let info = &by_name[name].1;
        assert_eq!((&info["kind"], &info["durationMs"]), (&json!("video"), &json!(clips::DURATION_MS)), "{name}: {info}");
    }
    assert_eq!(by_name["IMG_0001.jpg"].1["kind"], "still");
    let (clip_id, old_id) = (by_name["clip.mp4"].0, by_name["old.mov"].0);
    let v = m.tool("cmd_video_inspect", json!({ "id": clip_id }));
    assert_eq!((&v["playable"], &v["videoCodec"]), (&json!(true), &json!("avc1")), "{v}");
    let v = m.tool("cmd_video_inspect", json!({ "id": old_id }));
    assert_eq!((&v["playable"], &v["reason"]), (&json!(false), &json!(REASON)), "{v}");

    // Plays, takes its trim, and Save Frame writes the frame shown.
    let opened = m.tool("cmd_video_open", json!({ "id": clip_id }));
    assert_eq!((&opened["open"], &opened["playable"]), (&json!(true), &json!(true)), "{opened}");
    m.player_until("ready", |v| v["ready"] == true);
    let played = m.tool("cmd_video_play", json!({}));
    assert_eq!(played["playing"], true, "{played}");
    m.player_until("plays on", |v| v["positionMs"].as_u64().is_some_and(|ms| ms > 0));
    m.tool("cmd_video_pause", json!({}));
    let set = m.tool("cmd_edit_recipe_set", json!({ "ids": [clip_id], "recipe": "trim(500,1500)" }));
    assert_eq!(set["photos"][0]["changed"], true, "{set}");
    assert_eq!(m.tool("cmd_video_inspect", json!({ "id": clip_id }))["trim"], json!({ "startMs": 500, "endMs": 1500 }));
    m.tool("cmd_video_seek", json!({ "ms": 1000 }));
    m.player_until("at 1 s", |v| v["pending"] == false && v["frame"]["index"] == 25);
    let saved = m.tool("cmd_video_saveFrame", json!({}));
    assert_eq!(saved["state"], "done", "{saved}");
    let jpeg = PathBuf::from(saved["result"]["path"].as_str().unwrap_or_else(|| panic!("{saved}")));
    assert_eq!(jpeg, frames.join("clip 00-00-01.jpg"));
    let img = dalica_image::decode(&std::fs::read(&jpeg).unwrap(), &dalica_image::Limits::default()).unwrap();
    let grey = img.data[(((img.height / 2) * img.width + img.width * 3 / 4) * 4) as usize];
    assert_eq!(clips::frame_of(BASE, grey), Some(25), "the frame shown (grey {grey})");
    let start = Instant::now();
    loop {
        let inspected = m.tool("doc_inspect", json!({}));
        if project_row(&inspected, &frames).is_some_and(|r| r["count"] == json!(1)) {
            break;
        }
        assert!(start.elapsed() < PATIENCE, "Video Frames never joined the projects: {}", inspected["tree"]);
        std::thread::sleep(Duration::from_millis(50));
    }
    m.tool("cmd_video_close", json!({}));

    // The clip Dalica cannot play: the reason, and Save Frame refused, nothing written.
    let opened = m.tool("cmd_video_open", json!({ "id": old_id }));
    assert_eq!((&opened["playable"], &opened["reason"]), (&json!(false), &json!(REASON)), "{opened}");
    assert!(refusal(&m.call("cmd_video_saveFrame", json!({}))).contains(REASON));
    m.tool("cmd_video_close", json!({}));
    let in_frames =
        std::fs::read_dir(&frames).unwrap().flatten().filter(|e| e.path().is_file() && !e.file_name().to_string_lossy().starts_with('.')).count();
    assert_eq!(in_frames, 1, "the refused Save Frame wrote nothing");

    // Export as first frames, then as full videos.
    let out = home.join("Out");
    let exported = m.tool("cmd_export_run", json!({ "section": section, "dir": out, "folderName": "Trip" }));
    assert_eq!(exported["state"], "done", "{exported}");
    assert_eq!(exported["result"]["written"].as_array().map(Vec::len), Some(3), "{exported}");
    let skipped = exported["result"]["skipped"].as_array().unwrap();
    assert!(skipped.len() == 1 && skipped[0]["reason"] == REASON, "{exported}");
    let full = m.tool("export", json!({ "section": section, "dir": out, "folder_name": "Trip Full", "videos": "full" }));
    let files = full["files"].as_array().unwrap_or_else(|| panic!("{full}"));
    assert_eq!(files.len(), 4, "{full}");
    for source in [&clip, &old] {
        let name = source.file_name().unwrap().to_str().unwrap();
        let copy = files.iter().find(|f| f["path"].as_str().is_some_and(|p| p.ends_with(name))).unwrap_or_else(|| panic!("no {name}: {full}"));
        assert_eq!(std::fs::read(copy["path"].as_str().unwrap()).unwrap(), std::fs::read(source).unwrap(), "{name}: copied as it is");
    }
    m.finish();
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}: only read", path.display());
    }
    println!("  W17 over stdio: passed in {:.1} s", started.elapsed().as_secs_f64());
}
