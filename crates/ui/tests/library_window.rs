//! The plain library window (M1.18), driven headlessly: add a folder through File ▸ Add Folder…
//! (the Folder Manager since M6.39: the folder's row, Scan Always, OK), click a thumbnail by widget
//! id, press Ctrl+8, and see the star in the engine, in the widget list and in the screenshot —
//! with the UI thread marked, so any file access from it is counted.
//!
//! The screenshots are checked **by region** (M4.6), so a task that changes one region re-blesses
//! only its own file: `region-<name>-<case>.json` for the chrome (menu bar and toolbar), the tree
//! pane (with the splitter), the section headers, the grid cells, the scroll bar, the status line
//! and the tray (the whole bottom area). Headers and cells share the light table's rect: each is
//! that rect with the other's pixels blacked out. Together the regions cover every pixel of the
//! window exactly once (checked). Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores
//! new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::common::{FRAMES, new_driver, out_dir};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scan::Places;
use dalica_scenes::fingerprint;
use dalica_ui::dialogs::folder_manager::{State, mode_id, row_id};
use dalica_ui::{App, Driver};

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

fn check_fingerprint(name: &str, width: u32, height: u32, rgba: &[u8]) {
    let png = fingerprint::png_rgba(width, height, rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(width, height, rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(width, height, rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// A pixel rectangle `[x0, y0, x1, y1)`.
type Px = [u32; 4];

fn px(r: [f32; 4]) -> Px {
    [r[0].round() as u32, r[1].round() as u32, (r[0] + r[2]).round() as u32, (r[1] + r[3]).round() as u32]
}

fn union(a: Px, b: Px) -> Px {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn inside(p: (u32, u32), r: Px) -> bool {
    p.0 >= r[0] && p.0 < r[2] && p.1 >= r[1] && p.1 < r[3]
}

/// One region of the screenshot: a rectangle, and which of its pixels belong to it (the others are
/// blacked out).
struct Region {
    name: &'static str,
    rect: Px,
    keep: Box<dyn Fn(u32, u32) -> bool>,
}

/// The regions of the window in the last frame (`ui-layout.md` §2), from the registered region
/// rects and the light table's header rects.
fn regions(d: &Driver) -> Vec<Region> {
    let (w, h, _) = d.size();
    let widgets = d.widgets();
    let rect = |name: &str| px(widgets.iter().find(|wi| wi.id == format!("region:{name}")).unwrap_or_else(|| panic!("no region {name}")).rect);
    let info = d.app().panels().unwrap().light_table.frame_info().clone();
    let headers: Vec<Px> = info
        .headers
        .iter()
        .map(|(_, r)| r)
        .chain(info.sticky.as_ref().map(|(_, r)| r))
        .map(|r| px([r.min.x, r.min.y, r.width(), r.height()]))
        .collect();
    let in_header = move |x: u32, y: u32| headers.iter().any(|r| inside((x, y), *r));
    let in_header2 = in_header.clone();
    let status = rect("statusLine");
    let all = |_: u32, _: u32| true;
    vec![
        Region { name: "chrome", rect: union(rect("menuBar"), rect("toolbar")), keep: Box::new(all) },
        Region { name: "tree", rect: union(rect("tree"), rect("splitter")), keep: Box::new(all) },
        Region { name: "header", rect: rect("lightTable"), keep: Box::new(in_header) },
        Region { name: "cells", rect: rect("lightTable"), keep: Box::new(move |x, y| !in_header2(x, y)) },
        Region { name: "scrollbar", rect: rect("scrollbar"), keep: Box::new(all) },
        Region { name: "status", rect: status, keep: Box::new(all) },
        Region { name: "tray", rect: [0, status[3], w, h], keep: Box::new(all) },
    ]
}

/// Checks each region of `shot` against `region-<name>-<case>.json`, after checking that the
/// regions cover every pixel exactly once.
fn check_regions(d: &Driver, case: &str, shot: &dalica_ui_kit::Image) {
    let regions = regions(d);
    let (w, h) = (shot.width, shot.height);
    let mut covered = vec![0u8; (w * h) as usize];
    for r in &regions {
        for y in r.rect[1]..r.rect[3].min(h) {
            for x in r.rect[0]..r.rect[2].min(w) {
                if (r.keep)(x, y) {
                    covered[(y * w + x) as usize] += 1;
                }
            }
        }
    }
    let bad = covered.iter().filter(|&&c| c != 1).count();
    assert_eq!(bad, 0, "{case}: {bad} pixels are not covered by exactly one region");
    for r in &regions {
        let (rw, rh) = (r.rect[2].min(w) - r.rect[0], r.rect[3].min(h) - r.rect[1]);
        let mut rgba = Vec::with_capacity((rw * rh * 4) as usize);
        for y in r.rect[1]..r.rect[1] + rh {
            for x in r.rect[0]..r.rect[0] + rw {
                let p = if (r.keep)(x, y) { shot.pixel(x, y).unwrap() } else { [0, 0, 0, 255] };
                rgba.extend_from_slice(&p);
            }
        }
        check_fingerprint(&format!("region-{}-{case}", r.name), rw, rh, &rgba);
    }
    std::fs::write(out_dir().join(format!("window-{case}.png")), fingerprint::png_rgba(w, h, &shot.rgba)).unwrap();
}

/// The M1.18 done-when.
#[test]
fn click_a_thumbnail_press_ctrl_8_and_the_badge_shows() {
    let dir = tempfile::tempdir().unwrap();
    // The standard places are planted: the photos are in the home folder's Pictures.
    let home = dir.path().join("home");
    let pictures = home.join("Pictures");
    let folder = photos(&pictures, 12);
    let mut session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    session.set_places(Places::under(&home), Vec::new());

    // From here on this thread is the UI thread: every file access from it would be refused and
    // counted.
    let ui_thread = dalica_fs::mark_ui_thread();
    let refused_before = dalica_fs::guard::refused();
    let started = Instant::now();
    let mut d = new_driver(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();

    // File ▸ Add Folder… opens the Folder Manager: Pictures set to Scan Always, then OK; the
    // listing and the scan run on the engine's threads.
    d.click("menu:File").unwrap();
    d.click("menuitem:folders.manager").unwrap();
    let row = row_id(&pictures, &pictures);
    for _ in 0..FRAMES {
        if d.widgets().iter().any(|w| w.id == row) {
            break;
        }
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    d.click(&row).unwrap();
    d.click(&mode_id(State::Always)).unwrap();
    d.click("button:dialog.ok").unwrap();
    assert!(d.state().dialogs.is_empty());
    d.settle(FRAMES).unwrap();
    let thumbs: Vec<String> = d.widgets().into_iter().map(|w| w.id).filter(|id| id.starts_with("thumb:") && !id.ends_with("/star")).collect();
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
    let badge = widgets.iter().find(|w| w.id == format!("{target}/star")).expect("the star badge is registered");
    let cell = widgets.iter().find(|w| &w.id == target).unwrap();
    let inside = |r: [f32; 4], o: [f32; 4]| r[0] >= o[0] && r[1] >= o[1] && r[0] + r[2] <= o[0] + o[2] && r[1] + r[3] <= o[1] + o[3];
    assert!(inside(badge.rect, cell.rect), "badge {:?} inside cell {:?}", badge.rect, cell.rect);
    // Its cell in the folder, and its copy in Starred Photos (`thumb:<id>/in/starred`), above it.
    let stars: Vec<&str> = widgets.iter().map(|w| w.id.as_str()).filter(|w| w.starts_with("thumb:") && w.ends_with("/star")).collect();
    assert_eq!(stars, [format!("{target}/in/starred/star"), format!("{target}/star")], "{stars:?}");

    // The screenshot shows it: a light star glyph in the middle of the badge's dark plate.
    let shot = d.screenshot().unwrap();
    let (cx, cy) = ((badge.rect[0] + badge.rect[2] / 2.0) as u32, (badge.rect[1] + badge.rect[3] / 2.0) as u32);
    let glyph = shot.pixel(cx, cy).unwrap();
    assert!(glyph[0] > 180 && glyph[1] > 180 && glyph[2] > 180, "star glyph pixel {glyph:?}");
    check_regions(&d, "starred", &shot);

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
    let mut d = new_driver(App::new(Some(session)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    assert!(matches!(d.click("button:star"), Err(dalica_ui::DriverError::Disabled(_))));
    let first = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    d.click(&first.id).unwrap();
    d.click("button:star").unwrap();
    // Starred Photos shows it at the top of the light table, its badge on.
    assert!(d.widgets().iter().any(|w| w.id == format!("{}/in/starred/star", first.id)));
    assert!(d.widgets().iter().any(|w| w.id == "button:star" && w.active));
    d.click("button:star").unwrap();
    assert!(!d.widgets().iter().any(|w| w.id.ends_with("/star") && w.id.starts_with("thumb:")));
    // Ctrl+8 with nothing selected is refused with a reason on the status line, not a crash.
    d.run("select.none", &serde_json::json!({})).unwrap();
    d.key("Ctrl+8").unwrap();
    assert!(!d.state().status.is_empty());
    check_regions(&d, "small", &d.screenshot().unwrap());
    assert_eq!(dalica_fs::guard::refused(), refused_before);
}
