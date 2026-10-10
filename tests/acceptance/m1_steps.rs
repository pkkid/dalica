//! The steps of M1's acceptance that need no window, shared by `m1_headless.rs` (`cargo xtask accept
//! M1 --headless`) and `m1.rs` (`cargo xtask accept M1`). Everything goes through the real
//! `dalica-cli` binary and the files on disk:
//!
//! 1. a folder of 200 `scenes` JPEGs in all 8 EXIF orientations, with keys planted in its
//!    `.dalica.json` by "another program", is added to a temporary library;
//! 2. three photos are starred and one star is undone → the sidecar holds exactly 2 stars and the
//!    planted keys are intact;
//! 3. the index is deleted and `library.rebuild` runs → the same 2 stars come back;
//! 4. while one process has the library open, a second one is refused.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

/// What the later steps of `m1.rs` use: the library with its 2 stars, and the CLI.
pub struct Outcome {
    pub photos: tempfile::TempDir,
    pub lib: tempfile::TempDir,
    pub cli: PathBuf,
}

/// `dalica-cli`, built for this test run (it is another package's binary).
pub fn cli() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// Runs a script; returns the exit code and one JSON value per result line.
fn run_script(cli: &Path, lib: &Path, script: &[Value]) -> (Option<i32>, Vec<Value>) {
    let mut child = Command::new(cli).args(["run", "--library"]).arg(lib).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let text: String = script.iter().map(|v| format!("{v}\n")).collect();
    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let lines = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (out.status.code(), lines)
}

fn inspect(cli: &Path, lib: &Path) -> Value {
    let out = Command::new(cli).args(["inspect", "--library"]).arg(lib).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn starred_names(inspected: &Value) -> BTreeSet<String> {
    inspected["starred"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| Path::new(p["path"].as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// The names whose `star` is `true` in a sidecar, and the parsed sidecar.
fn sidecar(folder: &Path) -> (BTreeSet<String>, Value) {
    let v: Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
    let stars = v["photos"].as_object().unwrap().iter().filter(|(_, e)| e["star"] == json!(true)).map(|(n, _)| n.clone()).collect();
    (stars, v)
}

pub fn run() -> Outcome {
    let started = Instant::now();
    let cli = cli();
    let photos = tempfile::tempdir().unwrap();
    let lib = tempfile::tempdir().unwrap();

    // 1. 200 scenes JPEGs in one folder, all 8 orientations, plus another program's keys.
    let files = dalica_scenes::demo_library(1, 200, 1).unwrap();
    let orientations: BTreeSet<u8> = files.iter().map(|f| f.orientation).collect();
    assert_eq!(orientations, (1..=8).collect());
    let folder = photos.path().join(files[0].path.parent().unwrap());
    std::fs::create_dir_all(&folder).unwrap();
    for f in &files {
        std::fs::write(photos.path().join(&f.path), &f.bytes).unwrap();
    }
    let planted = json!({ "planted": { "by": "another program", "list": [1, 2, 3] }, "photos": { "IMG_0007.jpg": { "x-other": { "rating": 4 } } } });
    std::fs::write(folder.join(".dalica.json"), serde_json::to_vec_pretty(&planted).unwrap()).unwrap();

    let (code, lines) = run_script(&cli, lib.path(), &[json!({ "command": "folders.add", "params": { "path": folder } })]);
    assert_eq!(code, Some(0), "{lines:?}");
    let (code, lines) = run_script(&cli, lib.path(), &[json!({ "command": "library.query", "params": { "limit": 1000 } })]);
    assert_eq!(code, Some(0));
    let found = lines[0]["result"]["photos"].as_array().unwrap().clone();
    assert_eq!(found.len(), 200);
    // Every photo is seen upright: 480 × 320, or 320 × 480 for the portrait ones, whatever its orientation.
    assert!(found.iter().all(|p| matches!((p["width"].as_u64(), p["height"].as_u64()), (Some(480), Some(320)) | (Some(320), Some(480)))));
    let id_of = |name: &str| found.iter().find(|p| p["name"] == name).unwrap()["id"].clone();

    // 2. Star 3, undo 1.
    let script: Vec<Value> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]
        .iter()
        .map(|n| json!({ "command": "photo.star.toggle", "params": { "ids": [id_of(n)] } }))
        .chain([json!({ "command": "edit.undo", "params": {} })])
        .collect();
    let (code, lines) = run_script(&cli, lib.path(), &script);
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines[3]["result"], json!({ "undone": "Star / Unstar" }));
    let (stars, doc) = sidecar(&folder);
    assert_eq!(stars, BTreeSet::from(["IMG_0001.jpg".to_string(), "IMG_0002.jpg".to_string()]));
    assert_eq!(doc["planted"], planted["planted"]);
    assert_eq!(doc["photos"]["IMG_0007.jpg"]["x-other"], planted["photos"]["IMG_0007.jpg"]["x-other"]);
    println!(
        "after star 3, undo 1 — {}:\n{}",
        folder.join(".dalica.json").display(),
        String::from_utf8_lossy(&std::fs::read(folder.join(".dalica.json")).unwrap())
    );

    // 3. Delete the index, rebuild: the same 2 stars come back from the sidecar.
    for name in ["index.log", "index.snap"] {
        let _ = std::fs::remove_file(lib.path().join(name));
    }
    assert!(starred_names(&inspect(&cli, lib.path())).is_empty(), "the index really is gone");
    let (code, lines) = run_script(&cli, lib.path(), &[json!({ "command": "library.rebuild", "params": {} })]);
    assert_eq!(code, Some(0), "{lines:?}");
    let after = inspect(&cli, lib.path());
    assert_eq!(after["photos"], 200);
    assert_eq!(starred_names(&after), BTreeSet::from(["IMG_0001.jpg".to_string(), "IMG_0002.jpg".to_string()]));

    // 4. One process holds the library; a second is refused.
    let mut first = Command::new(&cli).args(["run", "--library"]).arg(lib.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut first_in = first.stdin.take().unwrap();
    writeln!(first_in, "{}", json!({ "command": "library.query", "params": { "limit": 1 } })).unwrap();
    let mut first_out = BufReader::new(first.stdout.take().unwrap());
    let mut line = String::new();
    first_out.read_line(&mut line).unwrap(); // its first result: it has the library open
    assert!(line.contains("\"ok\":true"), "{line}");
    let second = Command::new(&cli).args(["inspect", "--library"]).arg(lib.path()).output().unwrap();
    assert_eq!(second.status.code(), Some(3));
    let why = String::from_utf8_lossy(&second.stderr);
    assert!(why.contains("open in another Dalica process"), "{why}");
    drop(first_in);
    assert!(first.wait().unwrap().success());
    println!("second process: exit 3, {}", why.trim());
    println!("M1 headless steps: passed in {:.1} s", started.elapsed().as_secs_f64());
    Outcome { photos, lib, cli }
}
