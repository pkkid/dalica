//! The plain library window (M1.18), driven headlessly: add a folder through File ▸ Add Folder…,
//! click a thumbnail by widget id, press Ctrl+8, and see the star in the engine, in the widget list
//! and in the screenshot — with the UI thread marked, so any file access from it is counted.
//! Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores a new fingerprint.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::{App, Driver, ScriptedPicker};

const FRAMES: usize = 60 * 60;

/// A folder of `n` scenes photos in `root`; returns its path.
fn photos(root: &Path, n: usize) -> PathBuf {
    let files = dalica_scenes::demo_library(1, n, 11).unwrap();
    for f in &files {
        let path = root.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &f.bytes).unwrap();
    }
    root.join(files[0].path.parent().unwrap())
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn check_fingerprint(name: &str, shot: &dalica_ui_kit::Image) {
    let png = fingerprint::png_rgba(shot.width, shot.height, &shot.rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(shot.width, shot.height, &shot.rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(shot.width, shot.height, &shot.rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// The M1.18 done-when.
#[test]
fn click_a_thumbnail_press_ctrl_8_and_the_badge_shows() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(&dir.path().join("photos"), 12);
    let session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let picker = ScriptedPicker::new();
    picker.push_folder(&folder);

    // From here on this thread is the UI thread: every file access from it would be refused and
    // counted.
    let ui_thread = dalica_fs::mark_ui_thread();
    let refused_before = dalica_fs::guard::refused();
    let started = Instant::now();
    let mut d = Driver::new(App::with_picker(Some(session), Arc::new(picker)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();

    // File ▸ Add Folder…, answered by the scripted picker; the scan runs on the engine's threads.
    d.click("menu.file").unwrap();
    d.click("menu.file.addFolder").unwrap();
    d.settle(FRAMES).unwrap();
    let thumbs: Vec<String> = d.widgets().into_iter().map(|w| w.id).filter(|id| id.starts_with("thumb:") && !id.ends_with(".star")).collect();
    assert_eq!(thumbs.len(), 12, "{thumbs:?}");
    let opened = started.elapsed();

    // Click the third thumbnail by its id, then Ctrl+8.
    let target = &thumbs[2];
    let id = PhotoId::new(target.trim_start_matches("thumb:").parse().unwrap());
    d.click(target).unwrap();
    assert_eq!(d.state().selection.ids, [id]);
    d.key("Ctrl+8").unwrap();
    d.settle(FRAMES).unwrap();

    // The engine has the star; the badge is registered over that thumbnail.
    let library = d.app().session().unwrap().library();
    assert!(library.photo(id).unwrap().star, "the photo is starred in the engine");
    assert_eq!(library.photos().filter(|p| p.star).count(), 1);
    let widgets = d.widgets();
    let badge = widgets.iter().find(|w| w.id == format!("{target}.star")).expect("the star badge is registered");
    let cell = widgets.iter().find(|w| &w.id == target).unwrap();
    let inside = |r: [f32; 4], o: [f32; 4]| r[0] >= o[0] && r[1] >= o[1] && r[0] + r[2] <= o[0] + o[2] && r[1] + r[3] <= o[1] + o[3];
    assert!(inside(badge.rect, cell.rect), "badge {:?} inside cell {:?}", badge.rect, cell.rect);
    assert_eq!(widgets.iter().filter(|w| w.id.starts_with("thumb:") && w.id.ends_with(".star")).count(), 1);

    // The screenshot shows it: a light star glyph in the middle of the badge's dark plate.
    let shot = d.screenshot().unwrap();
    let (cx, cy) = ((badge.rect[0] + badge.rect[2] / 2.0) as u32, (badge.rect[1] + badge.rect[3] / 2.0) as u32);
    let glyph = shot.pixel(cx, cy).unwrap();
    assert!(glyph[0] > 180 && glyph[1] > 180 && glyph[2] > 180, "star glyph pixel {glyph:?}");
    check_fingerprint("library-starred", &shot);

    // Nothing touched the disk from the UI thread.
    assert_eq!(dalica_fs::guard::refused(), refused_before, "file access was attempted on the UI thread");
    drop(ui_thread);
    println!("opened and scanned 12 photos in {:.0} ms; {} frames in all", opened.as_secs_f64() * 1000.0, d.frames());

    // The star reached the folder's sidecar.
    let mut app = d.into_app();
    app.session_mut().unwrap().flush().unwrap();
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
    let starred: Vec<&String> = sidecar["photos"].as_object().unwrap().iter().filter(|(_, e)| e["star"] == true).map(|(n, _)| n).collect();
    assert_eq!(starred.len(), 1, "{sidecar}");
}

/// The star button does what Ctrl+8 does; with nothing selected it is disabled.
#[test]
fn the_star_button_toggles_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(&dir.path().join("photos"), 3);
    let mut session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    session.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
    session.wait_idle();
    let _ui_thread = dalica_fs::mark_ui_thread();
    let refused_before = dalica_fs::guard::refused();
    let mut d = Driver::new(App::new(Some(session)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    assert!(matches!(d.click("bottom.star"), Err(dalica_ui::DriverError::Disabled(_))));
    let first = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    d.click(&first.id).unwrap();
    d.click("bottom.star").unwrap();
    assert!(d.widgets().iter().any(|w| w.id == format!("{}.star", first.id)));
    assert!(d.widgets().iter().any(|w| w.id == "bottom.star" && w.active));
    d.click("bottom.star").unwrap();
    assert!(!d.widgets().iter().any(|w| w.id.ends_with(".star") && w.id.starts_with("thumb:")));
    // Ctrl+8 with nothing selected is refused with a reason on the status line, not a crash.
    d.run("select.none", &serde_json::json!({})).unwrap();
    d.key("Ctrl+8").unwrap();
    assert!(!d.state().status.is_empty());
    check_fingerprint("library-small", &d.screenshot().unwrap());
    assert_eq!(dalica_fs::guard::refused(), refused_before);
}
