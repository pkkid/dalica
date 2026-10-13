//! An embedded thumbnail only stands in (`architecture.md` §6.5, the first screenful first): a cell
//! on screen shows a new photo's embedded EXIF thumbnail while the photo's scan runs, then the
//! scan's own downscale replaces it, and `ui.settle` waits for that. So a settled frame is the same
//! whichever of the photo's two jobs (the embedded thumbnail, the scan) finished first.
//!
//! Regression test for the Windows-only failure of `accept M1` ("the CLI's snapshot and the
//! in-process frame differ"): on a runner with few cores the in-process session's cold cache made
//! the embedded thumbnails of some visible photos arrive before their scans; each such cell drew
//! the embedded image as its covering size (`Drawn::Image`), never asked again, and the settle
//! counted it as done, while the `dalica-cli snapshot` process, whose cache was warm, drew the
//! ladder's images. Here the scan is held at a gate, so the order is fixed on every machine.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_scenes::{Exif, Scene};
use dalica_ui::panels::thumbnails::{Drawn, Painted};
use dalica_ui::{App, Driver};
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// A gate the photo's scan job waits at until it is opened (60 s at most, so a failing test never
/// hangs).
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn hook(&self) -> dalica_engine::thumbs::ScanHook {
        let gate = self.clone();
        Arc::new(move |_| {
            let (lock, cv) = &*gate.0;
            let g = lock.lock().unwrap();
            let _ = cv.wait_timeout_while(g, Duration::from_secs(60), |open| !*open).unwrap();
        })
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}

/// A camera-sized JPEG with a 160-pixel EXIF thumbnail in `dir/Camera`, added to a library whose
/// thumbnail cache is `cache`; the photo's thumbnail scan waits at `gate` (when given).
fn session(dir: &Path, cache: &Path, gate: Option<&Gate>) -> Session {
    let folder = dir.join("Camera");
    if !folder.exists() {
        std::fs::create_dir_all(&folder).unwrap();
        let exif = Exif { thumbnail: Some(160), ..Exif::default() };
        std::fs::write(folder.join("IMG_0001.jpg"), dalica_scenes::jpeg(&Scene::new(5, 2400, 1600), 90, Some(&exif)).unwrap()).unwrap();
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), workers: 2, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.join("library"), Arc::new(SystemClock), &options).unwrap();
    s.set_thumbnail_scan_hook(gate.map(Gate::hook));
    if s.library().photos().next().is_none() {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    // The folder scan (not the thumbnail scan, which the gate may hold).
    let start = Instant::now();
    while s.busy() || s.library().photos().next().is_none() {
        s.poll();
        assert!(start.elapsed() < Duration::from_secs(60), "the folder scan did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    s
}

fn the_photo(d: &Driver) -> PhotoId {
    d.app().session().unwrap().library().photos().next().unwrap().id
}

fn painted(d: &Driver, id: PhotoId) -> Painted {
    let cell = d.widgets().into_iter().find(|w| w.id == format!("thumb:{id}")).unwrap().rect;
    let inside = |p: &Painted| p.rect.min.x >= cell[0] - 0.5 && p.rect.max.x <= cell[0] + cell[2] + 0.5 && p.rect.min.y >= cell[1] - 0.5;
    d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo == id && inside(p)).unwrap()
}

/// The pixels of the image `p` painted, from the last frame.
fn pixels(d: &Driver, p: &Painted) -> Vec<u8> {
    let shot = d.screenshot().unwrap();
    let r = p.rect;
    let (x0, y0, w, h) = (r.min.x.round() as u32, r.min.y.round() as u32, r.width().round() as u32, r.height().round() as u32);
    (y0..y0 + h).flat_map(|y| (x0..x0 + w).map(move |x| (x, y))).flat_map(|(x, y)| shot.pixel(x, y).unwrap()).collect()
}

#[test]
fn the_embedded_thumbnail_stands_in_until_the_scan_replaces_it_and_settle_waits_for_that() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let gate = Gate::default();
    let mut d = Driver::new(App::new(Some(session(dir.path(), &cache, Some(&gate)))), 1400, 900, 1.0);
    let id = the_photo(&d);

    // The embedded thumbnail arrives while the scan is held, and the cell draws it.
    let start = Instant::now();
    while d.app().session().unwrap().thumbnail_stats().quick == 0 || painted(&d, id).drawn != Drawn::Image {
        d.step();
        assert!(start.elapsed() < Duration::from_secs(60), "the embedded thumbnail was never shown");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(d.app().session().unwrap().thumbnail_stand_in(id), "it stands in for the ladder's image");
    assert_eq!(d.app().session().unwrap().thumbnail_stats().scanned, 0, "the scan is still held");
    let standing_in = pixels(&d, &painted(&d, id));

    // Not settled while the scan that replaces it is on its way: the cell is still pending.
    let held = d.settle_for(120, false);
    assert!(!held.settled, "a frame showing a stand-in is not settled: {held:?}");
    assert!(held.busy.iter().any(|b| b.starts_with("visible thumbnails")), "{held:?}");

    // The scan finishes: the cell draws the ladder's image (no longer the embedded one), settled.
    gate.open();
    d.settle(FRAMES).unwrap();
    assert!(!d.app().session().unwrap().thumbnail_stand_in(id));
    assert_eq!(painted(&d, id).drawn, Drawn::Image);
    let replaced = pixels(&d, &painted(&d, id));
    assert_eq!(replaced.len(), standing_in.len(), "the same cell");
    assert_ne!(replaced, standing_in, "the scan's downscale replaced the embedded thumbnail");
    drop(d);

    // The same library again, its cache warm: the settled cell is the same pixels as the cold
    // session's, whichever job finished first this time.
    let mut again = Driver::new(App::new(Some(session(dir.path(), &cache, None))), 1400, 900, 1.0);
    again.settle(FRAMES).unwrap();
    let id = the_photo(&again);
    assert_eq!(pixels(&again, &painted(&again, id)), replaced, "a settled frame does not depend on the race");
}
