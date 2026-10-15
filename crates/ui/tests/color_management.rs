//! View → Use Color Management (`menus-and-shortcuts.md` §1, `formats.md` §4) in the running app:
//! the menu row is live and checked by default, a click turns it off (and its check follows), the
//! setting is saved, and the light table redraws a ProPhoto-tagged photo's thumbnail differently
//! while an untagged photo's stays the same.

use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::PhotoId;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_scenes::Scene;
use dalica_scenes::icc::{Space, Version};
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::{App, Driver};
use dalica_ui_kit::Image;
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    tagged: PhotoId,
    plain: PhotoId,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Tagged");
    std::fs::create_dir_all(&folder).unwrap();
    let icc = dalica_scenes::icc::rgb(Space::ProPhoto, Version::V4);
    std::fs::write(folder.join("IMG_0001.jpg"), dalica_scenes::formats::jpeg_icc(&Scene::new(31, 480, 320), 92, None, &icc).unwrap()).unwrap();
    std::fs::write(folder.join("IMG_0002.jpg"), dalica_scenes::jpeg(&Scene::new(32, 480, 320), 92, None).unwrap()).unwrap();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(dalica_core::SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap().id;
    let (tagged, plain) = (id("IMG_0001.jpg"), id("IMG_0002.jpg"));
    let control = Control::new(Config::default());
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    d.settle_for(FRAMES, true);
    T { _dir: dir, control, conn: Connection::new(), d, tagged, plain }
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params.clone())).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

/// The row of `view.colorManagement` in `ui.menu.tree`.
fn row(t: &mut T) -> Value {
    fn find(nodes: &Value) -> Option<Value> {
        nodes.as_array()?.iter().find_map(|n| if n["id"] == "view.colorManagement" { Some(n.clone()) } else { find(&n["children"]) })
    }
    find(&ok(t, "ui.menu.tree", json!({}))).expect("the row is in the View menu")
}

/// The mean colour of the thumbnail cell of `id` in the last frame.
fn mean(t: &T, shot: &Image, id: PhotoId) -> [f64; 3] {
    let w = t.d.widgets().into_iter().find(|w| w.id == format!("thumb:{}", id.get())).expect("the cell is on screen");
    let [x, y, width, height] = w.rect;
    // The middle half of the cell: the photo, not its frame or shadow.
    let (x0, y0, x1, y1) = ((x + width / 4.0) as u32, (y + height / 4.0) as u32, (x + 3.0 * width / 4.0) as u32, (y + 3.0 * height / 4.0) as u32);
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for yy in y0..y1 {
        for xx in x0..x1 {
            let p = shot.pixel(xx, yy).unwrap();
            for c in 0..3 {
                sum[c] += f64::from(p[c]);
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum()
}

/// Keeps the last frame as `target/test-output/ui/<name>.png` (for a person to look at).
fn save(d: &Driver, name: &str) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

fn saved(t: &T) -> bool {
    t.d.app().session().unwrap().settings().color_management()
}

#[test]
fn the_menu_row_switches_colour_management_and_its_check_follows() {
    let mut t = setup();
    let r = row(&mut t);
    assert_eq!((r["live"].clone(), r["enabled"].clone(), r["checked"].clone()), (json!(true), json!(true), json!(true)), "{r}");
    let before = t.d.screenshot().unwrap();
    let (tagged, plain) = (mean(&t, &before, t.tagged), mean(&t, &before, t.plain));
    save(&t.d, "color-management-on-1400x900");

    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Use Color Management", "wait": true }));
    assert_eq!(row(&mut t)["checked"], json!(false));
    assert!(!t.d.app().session().unwrap().color_management());
    assert!(!saved(&t), "saved in settings.json");
    t.d.settle_for(FRAMES, true);
    let after = t.d.screenshot().unwrap();
    let moved = distance(tagged, mean(&t, &after, t.tagged));
    let still = distance(plain, mean(&t, &after, t.plain));
    assert!(moved > 6.0, "the tagged photo is drawn without its profile: moved {moved:.2}");
    assert!(still < 1.0, "an untagged photo looks the same either way: moved {still:.2}");
    save(&t.d, "color-management-off-1400x900");
    t.d.resize(900, 600);
    t.d.settle_for(FRAMES, true);
    save(&t.d, "color-management-off-900x600");

    ok(&mut t, "ui.menu.invoke", json!({ "path": "View/Use Color Management", "wait": true }));
    assert_eq!(row(&mut t)["checked"], json!(true));
    assert!(saved(&t));
    t.d.settle_for(FRAMES, true);
    save(&t.d, "color-management-on-900x600");
    t.d.resize(1400, 900);
    t.d.settle_for(FRAMES, true);
    let back = t.d.screenshot().unwrap();
    assert!(distance(tagged, mean(&t, &back, t.tagged)) < 1.0, "back to the managed rendering");
}

#[test]
fn before_a_library_opens_the_row_reads_on() {
    let app = App::new(None);
    assert!(dalica_ui::menus::checked(&app, "view.colorManagement", &Value::Null));
}

fn textures(d: &Driver) -> u64 {
    d.app().panels().unwrap().thumbnails.counts().thumb_textures
}

fn uploads(d: &Driver) -> u64 {
    d.app().panels().unwrap().thumbnails.totals().uploads
}

/// Folder → Refresh Thumbnails drops the textures of that folder's photos only: the other
/// folder's cells keep theirs (no upload for them).
#[test]
fn a_refresh_drops_only_its_sections_textures() {
    let dir = tempfile::tempdir().unwrap();
    let mut folders = Vec::new();
    for (name, seed) in [("A", 0u64), ("B", 100)] {
        let folder = dir.path().join("photos").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..4 {
            std::fs::write(
                folder.join(format!("IMG_{:04}.jpg", seed + i + 1)),
                dalica_scenes::jpeg(&Scene::new(seed + i, 300, 200), 85, None).unwrap(),
            )
            .unwrap();
        }
        folders.push(folder);
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(dalica_core::SystemClock), &options).unwrap();
    for f in &folders {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let a = s.library().folder_by_path(&folders[0]).unwrap();
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    d.settle_for(FRAMES, true);
    assert_eq!(textures(&d), 8, "both folders on screen, every cell covered");
    let before = uploads(&d);
    // The refreshed scans wait at a gate until the drop has been seen, so no new image can arrive
    // in the same frame however fast the machine is.
    let gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> = Arc::default();
    let held = Arc::clone(&gate);
    d.app_mut().session_mut().unwrap().set_thumbnail_scan_hook(Some(Arc::new(move |_| {
        let (lock, cv) = &*held;
        let open = lock.lock().unwrap();
        let _ = cv.wait_timeout_while(open, std::time::Duration::from_secs(60), |open| !*open).unwrap();
    })));
    d.run("section.refreshThumbs", &json!({ "section": format!("folder:{a}") })).unwrap();
    assert_eq!(textures(&d), 4, "folder A's textures went, folder B's stayed");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    d.settle_for(FRAMES, true);
    assert_eq!(textures(&d), 8);
    assert_eq!(uploads(&d) - before, 4, "only folder A's four cells uploaded again");
}

/// Switching colour management drops every texture at once: every frame from the switch until each
/// cell is covered again (each timed whole: the app's frame, the session's poll, uploads) stays
/// within the light table's 8 ms frame budget (`development.md` §8), asserted in a release build on
/// a scanned library (1 000 photos, 200 in a task's quick run; `DALICA_CM_PHOTOS` sets another
/// count, e.g. 10 000).
#[test]
#[cfg(not(windows))]
fn the_frames_after_a_whole_drop_stay_within_the_budget() {
    use dalica_engine::synthetic::{self, Spec};
    let photos: usize =
        std::env::var("DALICA_CM_PHOTOS").ok().and_then(|v| v.parse().ok()).unwrap_or(if crate::common::full() { 1_000 } else { 200 });
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    // The thumbnails go to `cache` (named in the options, not set for the whole test process).
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(dir.path().join("cache")), ..ThumbOptions::default() } };
    synthetic::write_photos(&lib, &Spec::new(photos, photos.div_ceil(100), synthetic::SEED), synthetic::DEFAULT_FILE_SIDE).unwrap();
    let mut first = Session::open_with(&synthetic::library_dir(&lib), Arc::new(dalica_core::SystemClock), &options).unwrap();
    first.execute("folders.add", &json!({ "path": synthetic::photos_dir(&lib) })).unwrap();
    first.wait_idle();
    drop(first);
    let session = Session::open_with(&synthetic::library_dir(&lib), Arc::new(dalica_core::SystemClock), &options).unwrap();
    let mut d = new_driver(App::new(Some(session)), 1400, 900, 1.0);
    d.settle_for(FRAMES, true);
    let covered = textures(&d);
    assert!(covered > 20, "{covered} textures");
    // Each frame timed whole (the command's frame included), until every cell is covered again.
    let mut times = Vec::new();
    let t = std::time::Instant::now();
    d.run("view.colorManagement", &json!({ "on": false })).unwrap();
    times.push(t.elapsed().as_secs_f64() * 1000.0);
    let started = std::time::Instant::now();
    while textures(&d) < covered || d.app().session().unwrap().thumbnails_pending() > 0 {
        assert!(started.elapsed().as_secs() < 120, "the cells never came back");
        let t = std::time::Instant::now();
        d.step();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let frames = times.len();
    let first = times[0];
    times.sort_by(f64::total_cmp);
    let p95 = times[(frames * 95).div_ceil(100) - 1];
    let max = times[frames - 1];
    println!(
        "{photos} photos: {covered} textures dropped; {frames} frames until every cell is covered again: p95 {p95:.2} ms, max {max:.2} ms (the toggle's own frame {first:.2} ms)"
    );
    assert_eq!(textures(&d), covered, "every cell covered again");
    if !cfg!(debug_assertions) {
        assert!(p95 <= 8.0 && max <= 8.0, "p95 {p95:.2} ms, max {max:.2} ms: over the 8 ms frame budget");
    }
}
