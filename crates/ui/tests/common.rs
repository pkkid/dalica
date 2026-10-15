//! What the UI's integration tests share: the driver every test drives (with the automation-id
//! check on), finding widgets, fingerprint checks of a region, the test output folder, the quick
//! or full switch, and the photo fixtures.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use dalica_core::{Date, DateTime};
use dalica_scenes::fingerprint;
use dalica_ui::{App, Driver};
use dalica_ui_kit::{Image, WidgetInfo};

/// The most frames a test waits for the app to settle.
pub const FRAMES: usize = 60 * 60;
/// The size every fingerprint is taken at.
pub const LARGE: (u32, u32) = (1400, 900);
/// The smallest window the layout is checked at (`ui-layout.md`).
pub const SMALL: (u32, u32) = (900, 600);

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// One lock for what is global to the test process (none of the tests may change it unlocked).
pub fn process_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A driver whose every frame checks the automation ids (`control-protocol.md` §7): an
/// interactive widget without an id, an id registered twice or one outside the grammar makes the
/// next settle fail.
pub fn new_driver(app: App, width: u32, height: u32, pixels_per_point: f32) -> Driver {
    let mut d = Driver::new(app, width, height, pixels_per_point);
    d.check_ids(true);
    d
}

/// Where renders go: `target/test-output/ui/`.
pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `png` as `<name>.png` to the test output and to `DALICA_SHOTS`, when set.
pub fn write_png(name: &str, png: &[u8]) {
    std::fs::write(out_dir().join(format!("{name}.png")), png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }
}

/// The window, painted, as `<name>.png` in the test output (and `DALICA_SHOTS`).
pub fn shot(d: &Driver, name: &str) {
    let s = d.screenshot().unwrap();
    write_png(name, &fingerprint::png_rgba(s.width, s.height, &s.rgba));
}

/// The widget with automation id `id` in the last frame, if there is one.
pub fn try_widget(d: &Driver, id: &str) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == id)
}

/// The widget with automation id `id` in the last frame.
pub fn widget(d: &Driver, id: &str) -> WidgetInfo {
    try_widget(d, id).unwrap_or_else(|| panic!("no `{id}` in {:?}", d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

/// The pixels of `shot` from `(x0, y0)` to `(x1, y1)`, exclusive: `(width, height, rgba)`.
pub fn crop(shot: &Image, [x0, y0, x1, y1]: [u32; 4]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    (w, h, rgba)
}

/// The pixels of `rect` (`[x, y, width, height]` in points at scale 1), its edges rounded and
/// clipped to the window.
pub fn rect_pixels(shot: &Image, rect: [f32; 4]) -> (u32, u32, Vec<u8>) {
    let [x0, y0] = [rect[0].max(0.0).round() as u32, rect[1].max(0.0).round() as u32];
    let x1 = ((rect[0] + rect[2]).round() as u32).min(shot.width);
    let y1 = ((rect[1] + rect[3]).round() as u32).min(shot.height);
    crop(shot, [x0, y0, x1, y1])
}

/// Checks `rgba` (`w × h`) against `tests/fingerprints/<name>.json` (`DALICA_BLESS=1` stores a new
/// one); the image goes to the test output, and on a mismatch `<name>-actual.png` too.
pub fn check_pixels(name: &str, w: u32, h: u32, rgba: &[u8]) {
    write_png(name, &fingerprint::png_rgba(w, h, rgba));
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// Checks the region of widget `id` against `tests/fingerprints/<name>.json`; the window
/// (`<name>-window.png`) and the region go to the test output.
pub fn check_region(d: &Driver, id: &str, name: &str) {
    let s = d.screenshot().unwrap();
    write_png(&format!("{name}-window"), &fingerprint::png_rgba(s.width, s.height, &s.rgba));
    let r = widget(d, id).rect;
    let (x0, y0) = (r[0].round() as u32, r[1].round() as u32);
    let (w, h, rgba) = crop(&s, [x0, y0, x0 + r[2].round() as u32, y0 + r[3].round() as u32]);
    check_pixels(name, w, h, &rgba);
}

/// Checks `rect` of the window (`[x, y, width, height]`, see [`rect_pixels`]) against
/// `tests/fingerprints/<name>.json`; the window and the region go to the test output.
pub fn check_rect(d: &Driver, name: &str, rect: [f32; 4]) {
    let s = d.screenshot().unwrap();
    write_png(&format!("{name}-window"), &fingerprint::png_rgba(s.width, s.height, &s.rgba));
    let (w, h, rgba) = rect_pixels(&s, rect);
    check_pixels(name, w, h, &rgba);
}

/// Every widget of the last frame lies inside the window, with an area: what the small size checks
/// instead of a second fingerprint.
pub fn check_fits(d: &Driver, case: &str) {
    let (w, h, _) = d.size();
    for wi in d.widgets() {
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{case} at {w}×{h}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
}

/// Resizes the window to [`SMALL`], settles, and checks that the widget `id` (a dialog, a panel)
/// and every widget lie inside it.
pub fn check_small(d: &mut Driver, id: &str) {
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    let r = widget(d, id).rect;
    assert!(r[2] > 0.0 && r[3] > 0.0 && r[0] >= 0.0 && r[1] >= 0.0, "`{id}` at {r:?}");
    assert!(r[0] + r[2] <= SMALL.0 as f32 + 0.01 && r[1] + r[3] <= SMALL.1 as f32 + 0.01, "`{id}` at {r:?} does not fit {SMALL:?}");
    check_fits(d, id);
}

/// A 96 × 64 JPEG of scene `seed` at `path` (its folders made), with no EXIF.
pub fn put_jpeg(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()).unwrap();
}

/// A 96 × 64 JPEG of scene `seed` at `path` (its folders made), taken on `(year, month, day)` at
/// 10:00.
pub fn put_dated_jpeg(path: &Path, seed: u64, (y, m, d): (u16, u8, u8)) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let taken = DateTime::new(Date::new(y, m, d).unwrap(), 10, 0, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    std::fs::write(path, dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, Some(&exif)).unwrap()).unwrap();
}
