//! An idle app lands a queued metadata write (M10.54). The headless loop (and the window) run a
//! frame only while `App::idle` is false; the metadata writer's job is registered and its results
//! applied by the session's poll in a frame, so a caption's write must keep the app from being
//! idle until it is in the photo file, then let it rest again.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, new_driver, put_jpeg};
use dalica_engine::Session;
use dalica_ui::App;
use serde_json::json;

/// How the headless loop waits between looks while the app is idle, and while it is busy
/// (`apps/dalica`'s `IDLE_TICK` and `BUSY_TICK`).
const IDLE_TICK: Duration = Duration::from_millis(250);
const BUSY_TICK: Duration = Duration::from_millis(16);
const WAIT: Duration = Duration::from_secs(30);

fn file_caption(path: &Path) -> Option<String> {
    dalica_meta::read(&std::fs::read(path).unwrap(), &dalica_meta::Limits::default()).unwrap().owned.caption
}

/// Not idle while the caption's write is queued; ticked only as the headless loop ticks an idle
/// app (a frame only when not idle; no input, no settle), the caption reaches the photo file, and
/// the app is idle again afterwards.
#[test]
fn an_idle_app_lands_a_queued_caption_write() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    let photo = folder.join("IMG_0001.jpg");
    put_jpeg(&photo, 1);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let start = Instant::now();
    while !d.app().idle() {
        assert!(start.elapsed() < WAIT, "the app comes to rest after opening");
        d.step();
        std::thread::sleep(BUSY_TICK);
    }

    d.run("photo.caption.set", &json!({ "ids": [id.get()], "caption": "Alfama" })).unwrap();
    assert!(!d.app().idle(), "not idle while the caption's write is queued");

    let start = Instant::now();
    let mut frames = 0u32;
    while file_caption(&photo).is_none() || !d.app().idle() {
        assert!(start.elapsed() < WAIT, "the write lands and the app rests ({frames} frames)");
        let tick = if d.app().idle() { IDLE_TICK } else { BUSY_TICK };
        std::thread::sleep(tick);
        if !d.app().idle() {
            d.step();
            frames += 1;
        }
    }
    assert_eq!(file_caption(&photo).as_deref(), Some("Alfama"));
    assert_eq!(d.app().session().unwrap().pending_metadata_writes(), 0);
}
