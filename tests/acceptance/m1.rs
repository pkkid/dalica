//! M1 "Star a photo, end to end" (`docs/roadmap.md` → M1 → Done when; run by `cargo xtask accept
//! M1`): the headless steps of `m1_steps.rs` (200 photos, star 3 and undo 1, rebuild, a second
//! process refused), then the fault-injection suites and `panic_hunt`, then a headless snapshot of
//! that library through the real `dalica-cli snapshot` that shows its 2 star badges (fingerprint).
//! The manual check is in `m1.md`.

#[path = "m1_steps.rs"]
mod steps;

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use dalica_scenes::fingerprint;

/// Runs `cargo test` with `args`; panics with its output if it fails.
fn cargo_test(args: &[&str]) {
    let started = Instant::now();
    let out = dalica_acceptance::cargo().arg("test").args(args).output().unwrap();
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

#[test]
fn star_a_photo_end_to_end() {
    let started = Instant::now();
    // 1–4: the headless steps.
    let outcome = steps::run();

    // 5. Fault injection never leaves a torn sidecar (or any torn file); panic_hunt stays green.
    cargo_test(&["-p", "dalica-store", "--test", "sidecars", "a_failed_write_leaves_the_old_or_the_new_sidecar"]);
    cargo_test(&["-p", "dalica-fs", "--lib", "each_fault_point_leaves_the_old_or_the_new_file"]);
    cargo_test(&["-p", "dalica-engine", "--test", "panic_hunt", "--", "--ignored"]);

    // 6. A headless snapshot of the library through the real CLI shows the 2 star badges.
    let out_dir = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
    std::fs::create_dir_all(&out_dir).unwrap();
    let png_path = out_dir.join("m1-snapshot.png");
    let _ = std::fs::remove_file(&png_path);
    let snap = Command::new(&outcome.cli)
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
    let badges: Vec<String> = driver.widgets().into_iter().map(|w| w.id).filter(|id| id.starts_with("thumb:") && id.ends_with("/star")).collect();
    assert_eq!(badges.len(), 2, "{badges:?}");
    let library = driver.app().session().unwrap().library();
    let mut names: Vec<String> = badges
        .iter()
        .filter_map(|b| b.trim_start_matches("thumb:").trim_end_matches("/star").parse().ok())
        .filter_map(|id| library.photo(dalica_core::PhotoId::new(id)))
        .map(|p| p.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, ["IMG_0001.jpg", "IMG_0002.jpg"]);
    let in_process = driver.screenshot().unwrap();
    assert!(in_process.rgba == rgba, "the CLI's snapshot and the in-process frame differ");
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
