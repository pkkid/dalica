//! M1 "Star a photo, end to end" (`docs/roadmap.md` → M1 → Done when; run by `cargo xtask accept
//! M1`): the headless steps of `m1_steps.rs` (200 photos, star 3 and undo 1, rebuild, a second
//! process refused), then the fault-injection suites (unless the ci test step already ran them:
//! `dalica_acceptance::SUITES_RAN`) and `panic_hunt`, then a headless snapshot of
//! that library through the real `dalica-cli snapshot` that shows its 2 star badges (fingerprint).
//! The manual check is in `m1.md`.

#[path = "m1_steps.rs"]
mod steps;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use dalica_scenes::fingerprint;
use serde_json::{Value, json};

/// Runs `cargo test` with `args`; panics with its output if it fails.
fn cargo_test(args: &[&str]) {
    let started = Instant::now();
    let out = dalica_acceptance::cargo_split(&[&["test"][..], args].concat(), |_| {}).unwrap();
    assert!(
        out.status.success(),
        "cargo test {}:\n{}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let summary =
        String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.starts_with("test result: ok")).map(str::to_string).collect::<Vec<_>>().join("; ");
    println!("cargo test {}: {summary} ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
}

fn decode_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// Runs a `dalica-cli run` script on `lib`; one JSON value per result line, each `ok`.
fn run_script(cli: &Path, lib: &Path, script: &[Value]) -> Vec<Value> {
    let mut child = steps::command(cli, lib).args(["run", "--library"]).arg(lib).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let text: String = script.iter().map(|v| format!("{v}\n")).collect();
    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{script:?}: {}", String::from_utf8_lossy(&out.stdout));
    let lines: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(lines.iter().all(|l| l["ok"] == true), "{lines:?}");
    lines
}

/// Unstars and stars the starred photo `name` again, in a process of its own: its change is then
/// strictly the library's newest.
fn restar_later(cli: &Path, lib: &Path, name: &str) {
    let found = run_script(cli, lib, &[json!({ "command": "library.query", "params": { "limit": 1000 } })]);
    let photos = found[0]["result"]["photos"].as_array().unwrap();
    let id = photos.iter().find(|p| p["name"] == name).unwrap()["id"].clone();
    let toggle = json!({ "command": "photo.star.toggle", "params": { "ids": [id] } });
    run_script(cli, lib, &[toggle.clone(), toggle]);
}

#[test]
fn star_a_photo_end_to_end() {
    let started = Instant::now();
    // 1–4: the headless steps.
    let outcome = steps::run();

    // 5. Fault injection never leaves a torn sidecar (or any torn file); panic_hunt stays green
    //    (both run once in the review gate's test step; here only when run on their own).
    if !dalica_acceptance::suites_ran() {
        cargo_test(&["-p", "dalica-store", "--test", "sidecars", "a_failed_write_leaves_the_old_or_the_new_sidecar"]);
        cargo_test(&["-p", "dalica-fs", "--test", "dirfd", "each_fault_point_leaves_the_old_or_the_new_file"]);
    }
    dalica_acceptance::panic_hunt().unwrap();

    // 6. A headless snapshot of the library through the real CLI shows the 2 star badges.
    //    Recently Updated shows the two starred photos newest change first, then by name
    //    (`organizing.md` §4.1), and step 2 stamped both stars with the system clock in one
    //    process, in the same millisecond or not. So IMG_0002 is starred again in a later process
    //    (unstarred, starred: still the same 2 stars), and is the newest on every run.
    restar_later(&outcome.cli, outcome.lib.path(), "IMG_0002.jpg");
    let out_dir = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
    std::fs::create_dir_all(&out_dir).unwrap();
    let png_path = out_dir.join("m1-snapshot.png");
    let _ = std::fs::remove_file(&png_path);
    let snap = steps::command(&outcome.cli, outcome.lib.path())
        .args(["snapshot", "--library"])
        .arg(outcome.lib.path())
        .args(["--size", "1400x900", "-o"])
        .arg(&png_path)
        .output()
        .unwrap();
    assert!(snap.status.success(), "{}", String::from_utf8_lossy(&snap.stderr));
    println!("snapshot: {}", String::from_utf8_lossy(&snap.stdout).trim());
    let (w, h, rgba) = decode_png(&png_path);
    assert_eq!((w, h), (1400, 900));

    // The same frame in-process: exactly the 2 starred photos have a badge, and the pixels match
    // the CLI's.
    let session = dalica_engine::Session::open(outcome.lib.path(), Arc::new(dalica_core::SystemClock)).unwrap();
    let _ui_thread = dalica_fs::mark_ui_thread();
    let mut driver = dalica_ui::Driver::new(dalica_ui::App::new(Some(session)), 1400, 900, 1.0);
    driver.settle(60 * 60).unwrap();
    // Each starred photo has a copy in Starred Photos (M9.17, `thumb:<id>/in/starred`) and, being
    // changed since the library indexed it, one in Recently Updated (M10.29, `thumb:<id>/in/recent`,
    // `organizing.md` §4.1); both sections come first, above the folder, which they push down to
    // the view's bottom edge. Photos are found by id, never by position: every badge drawn is a
    // starred photo's and lies inside its own cell.
    let starred = ["IMG_0001.jpg", "IMG_0002.jpg"];
    let badged = |driver: &dalica_ui::Driver, section: &str| -> Vec<String> {
        let widgets = driver.widgets();
        let library = driver.app().session().unwrap().library();
        let mut names = Vec::new();
        for badge in widgets.iter().filter(|w| w.id.starts_with("thumb:") && w.id.ends_with("/star")) {
            let cell_id = badge.id.trim_end_matches("/star");
            let (photo, in_section) = cell_id.split_once("/in/").unwrap_or((cell_id, "folder"));
            let cell = widgets.iter().find(|w| w.id == cell_id).unwrap_or_else(|| panic!("{} has no cell", badge.id));
            let (b, c) = (badge.rect, cell.rect);
            let inside = b[0] >= c[0] && b[1] >= c[1] && b[0] + b[2] <= c[0] + c[2] && b[1] + b[3] <= c[1] + c[3];
            assert!(inside, "{} lies inside its cell: badge {b:?}, cell {c:?}", badge.id);
            let id: u64 = photo.trim_start_matches("thumb:").parse().unwrap();
            let name = library.photo(dalica_core::PhotoId::new(id)).unwrap().name.clone();
            assert!(starred.contains(&name.as_str()), "only the starred photos have a badge: {} ({name})", badge.id);
            if in_section == section {
                names.push(name);
            }
        }
        names.sort();
        names
    };
    assert_eq!(badged(&driver, "starred"), starred, "both starred photos' copies in Starred Photos have their badge");
    assert_eq!(badged(&driver, "recent"), starred, "both starred photos' copies in Recently Updated have their badge");
    let in_process = driver.screenshot().unwrap();
    assert!(in_process.rgba == rgba, "the CLI's snapshot and the in-process frame differ");
    // Their cells in their folder: the folder's row in the tree scrolls the light table to it.
    let folder = driver.widgets().into_iter().map(|w| w.id).find(|id| id.starts_with("tree:folder:")).unwrap();
    driver.click(&folder).unwrap();
    driver.settle(60 * 60).unwrap();
    assert_eq!(badged(&driver, "folder"), starred, "both starred photos' cells in their folder have their badge");
    assert_eq!(dalica_fs::guard::refused(), 0, "no file access from the UI thread");

    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fingerprints/m1-snapshot.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        panic!("m1 snapshot: {} (render in {})", m.message, png_path.display());
    }
    drop(outcome.photos);
    println!("M1 acceptance: passed in {:.1} s", started.elapsed().as_secs_f64());
}
