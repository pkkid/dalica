//! The star's flight in the edit view (`editing.md` §2.1, issue #755), headless: a click on the
//! star button stars the photo at once and starts the burst, which ends within its second and
//! draws the star above the button meanwhile; unstarring starts the 450 ms drop; a star set without
//! the person's input (a command, as over the control channel) and a star with Use animations off
//! fly nothing. `DALICA_SHOTS=<dir>` writes the mid-flight screenshot there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, new_driver, shot};
use dalica_scenes::Scene;
use dalica_ui::panels::edit::star_flight::{self, Kind, STAR_SECONDS, UNSTAR_SECONDS};
use dalica_ui::{App, Driver, DriverError};
use serde_json::json;

const BUTTON: &str = "button:star";

/// The edit view at 1400 × 900 on one scenes photo, its preview drawn.
fn setup(dir: &std::path::Path) -> (Driver, u64) {
    let folder = dir.join("photos").join("Meadow");
    std::fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&Scene::new(11, 900, 600), 90, None).unwrap();
    std::fs::write(folder.join("IMG_0001.jpg"), bytes).unwrap();
    let mut s = dalica_engine::Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10 })).unwrap();
    let id = found["photos"][0]["id"].as_u64().unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": id })).unwrap();
    settle(&mut d);
    (d, id)
}

/// Frames until nothing is pending, the preview awaited in real time (as `edit_canvas.rs`).
fn settle(d: &mut Driver) {
    for _ in 0..10 {
        if d.app().session().is_some_and(|s| s.preview_busy() > 0) {
            assert!(d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(60)));
        }
        match d.settle(FRAMES) {
            Ok(_) => return,
            Err(DriverError::NotSettled { .. }) => {}
            Err(e) => panic!("{e}"),
        }
    }
    panic!("not settled");
}

fn starred(d: &Driver, id: u64) -> bool {
    let id = dalica_core::PhotoId::new(id);
    d.app().session().unwrap().library().photo(id).unwrap().star
}

/// Frames (at most `max`) until a flight is in the air; the frames it took.
fn until_flying(d: &mut Driver, max: usize) -> Option<usize> {
    (0..=max).find(|_| {
        let flying = star_flight::flight(d.ctx()).is_some();
        if !flying {
            d.step();
        }
        flying
    })
}

/// How many pixels of the window's `rect` (points at scale 1) are near white: the flying star.
fn bright(d: &Driver, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
    let s = d.screenshot().unwrap();
    let mut n = 0;
    for y in y0..y1.min(s.height) {
        for x in x0..x1.min(s.width) {
            let i = ((y * s.width + x) * 4) as usize;
            if s.rgba[i..i + 3].iter().all(|&c| c > 180) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn starring_by_hand_flies_and_lands_within_its_time_and_the_star_is_written_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, id) = setup(dir.path());
    assert!(!starred(&d, id));
    let button = d.widgets().into_iter().find(|w| w.id == BUTTON).expect("the star button");
    let (bx, by) = (button.rect[0] + button.rect[2] / 2.0, button.rect[1] + button.rect[3] / 2.0);
    // Above the button: where the burst rises (the button's own glyph is outside the box).
    let above = ((bx - 24.0) as u32, (by - 70.0) as u32, (bx + 24.0) as u32, (by - 13.0) as u32);
    assert_eq!(bright(&d, above), 0, "nothing above the button at rest");

    d.click(BUTTON).unwrap();
    assert!(starred(&d, id), "the star is written by the click itself, not after the flight");
    assert!(until_flying(&mut d, 3).is_some(), "the burst starts within a few frames of the click");
    let f = star_flight::flight(d.ctx()).unwrap();
    assert_eq!(f.kind, Kind::Star);
    assert!(d.repaint_delay().is_some_and(|r| r.is_zero()), "a flight asks for the next frame");

    // Mid-flight (60 %): the star is drawn above the button.
    let frames = (STAR_SECONDS * 60.0).ceil() as usize;
    for _ in 0..frames * 3 / 5 {
        d.step();
    }
    assert!(star_flight::flight(d.ctx()).is_some());
    assert!(bright(&d, above) > 20, "the star is in the air above the button");
    shot(&d, "star-flight-mid-1400x900");

    // It ends within its duration (a frame of slack), and the star stays.
    for _ in 0..frames {
        d.step();
    }
    assert_eq!(star_flight::flight(d.ctx()), None, "the burst ends within {STAR_SECONDS} s");
    assert_eq!(bright(&d, above), 0, "nothing left in the air");
    assert!(starred(&d, id));
    d.settle(FRAMES).unwrap();

    // Unstarring drops the star, within its own time.
    d.click(BUTTON).unwrap();
    assert!(!starred(&d, id), "the unstar is written at once");
    assert!(until_flying(&mut d, 3).is_some());
    assert_eq!(star_flight::flight(d.ctx()).map(|f| f.kind), Some(Kind::Unstar));
    for _ in 0..(UNSTAR_SECONDS * 60.0).ceil() as usize + 1 {
        d.step();
    }
    assert_eq!(star_flight::flight(d.ctx()), None, "the drop ends within {UNSTAR_SECONDS} s");
    d.settle(FRAMES).unwrap();
}

#[test]
fn a_star_set_without_the_persons_input_or_with_animations_off_does_not_fly() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, id) = setup(dir.path());
    // A command alone (the control channel's engine.execute, the CLI, MCP): written, no flight.
    d.wait(1000).unwrap();
    d.run("photo.star.toggle", &json!({ "ids": [id] })).unwrap();
    assert!(starred(&d, id));
    assert_eq!(until_flying(&mut d, 30), None, "no flight for a star nobody clicked");

    // Use animations off: the click unstars, and nothing flies.
    d.app_mut().session_mut().unwrap().execute("options.set", &json!({ "values": { "interface.animations": false } })).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(BUTTON).unwrap();
    assert!(!starred(&d, id));
    assert_eq!(until_flying(&mut d, 30), None, "no flight with Use animations off");
}
