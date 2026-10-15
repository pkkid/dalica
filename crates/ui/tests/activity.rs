//! The activity indicator, the activity popover and the scan on the status line (M5.19,
//! `ui-layout.md` §3 item 8, §6.1, `organizing.md` §1), driven headlessly over a temp library whose
//! scan is held file by file: while it runs the indicator spins, a click opens the popover listing
//! the scan with its progress rising, its Cancel stops it, and the status line counts the pictures
//! found ("Scanning: 4 pictures found") in place of "No selection", or as the last field with a
//! selection, until the scan ends. At rest the indicator draws nothing. The popover and the status
//! line are checked against fingerprints at 1400 × 900 and 900 × 600
//! (`activity-<w>.json`, `region-status-scanning-<w>.json`); renders go to
//! `target/test-output/ui/`, and `DALICA_BLESS=1` stores new fingerprints. The UI thread is marked
//! throughout: nothing it does touches the disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::common::{FRAMES, new_driver, out_dir, widget};
use dalica_engine::watching::WatchMode;
use dalica_engine::{JobState, Session};
use dalica_scan::HeaderReader;
use dalica_scenes::fingerprint;
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::json;

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
/// Photos per folder, and folders under the root.
const PER_FOLDER: usize = 4;
const FOLDERS: [&str; 3] = ["2026-06-01 Lisbon", "2026-06-02 Porto", "2026-06-03 Sintra"];

/// The root `photos/Trip` under `dir`, with [`FOLDERS`] of [`PER_FOLDER`] photos each.
fn trip(dir: &Path) -> PathBuf {
    let root = dir.join("photos").join("Trip");
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 96, 64), 80, None).unwrap();
    for name in FOLDERS {
        let folder = root.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..PER_FOLDER {
            std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg).unwrap();
        }
    }
    root
}

/// Files the reader may still read; it waits (at most 20 s a file) while there are none.
type Permits = Arc<(Mutex<usize>, Condvar)>;

fn permitted() -> (HeaderReader, Permits) {
    let permits: Permits = Arc::new((Mutex::new(0), Condvar::new()));
    let (p, inner) = (Arc::clone(&permits), dalica_scan::default_reader());
    let reader: HeaderReader = Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
        let (lock, cv) = &*p;
        let mut n = cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(20), |n| *n == 0).unwrap().0;
        *n = n.saturating_sub(1);
        drop(n);
        inner(path, limits)
    });
    (reader, permits)
}

fn give(permits: &Permits, n: usize) {
    *permits.0.lock().unwrap() += n;
    permits.1.notify_all();
}

struct T {
    _dir: tempfile::TempDir,
    root: PathBuf,
    permits: Permits,
    d: Driver,
}

/// An empty library, a scan reader held by permits, and the window at `size`.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let root = trip(dir.path());
    let mut session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    // No watcher: the change events the system may still deliver for the files just written
    // (Windows reports attribute and size updates late) would start a rescan job of their own
    // after the scan, and this test is about the scan's job alone.
    session.set_watching(WatchMode::Off);
    let (reader, permits) = permitted();
    session.set_scan_reader(Some(reader));
    let mut d = new_driver(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, root, permits, d }
}

fn session(d: &Driver) -> &Session {
    d.app().session().unwrap()
}

fn has(d: &Driver, id: &str) -> bool {
    d.widgets().iter().any(|w| w.id == id)
}

/// Steps until the running scan has applied `n` photos.
fn until_indexed(d: &mut Driver, n: usize) {
    d.step_until(|app| app.session().is_some_and(|s| s.scan_progress().indexed >= n), FRAMES).unwrap_or_else(|_| panic!("{n} photos indexed"));
    // A few more frames, so what is drawn has caught up.
    d.wait(100).unwrap();
}

/// The only job: its id, state and progress.
fn job(d: &Driver) -> dalica_engine::JobEntry {
    let jobs = session(d).jobs();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    jobs[0].clone()
}

/// Checks the pixels of `rect` (points = pixels at scale 1) of `shot` against `<name>.json`.
fn check_fingerprint(name: &str, shot: &dalica_ui_kit::Image, rect: [f32; 4]) {
    let [x0, y0] = [rect[0].round() as u32, rect[1].round() as u32];
    let [x1, y1] = [((rect[0] + rect[2]).round() as u32).min(shot.width), ((rect[1] + rect[3]).round() as u32).min(shot.height)];
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// The pixels of `rect` in `shot`, row by row.
fn pixels(shot: &dalica_ui_kit::Image, rect: [f32; 4]) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for y in rect[1] as u32..(rect[1] + rect[3]) as u32 {
        for x in rect[0] as u32..(rect[0] + rect[2]) as u32 {
            out.push(shot.pixel(x, y).unwrap());
        }
    }
    out
}

/// How many pixels of `rect` differ from the toolbar's fill (sampled at the toolbar's left end).
fn ink(shot: &dalica_ui_kit::Image, rect: [f32; 4]) -> usize {
    let fill = shot.pixel(4, 26).unwrap();
    pixels(shot, rect).iter().filter(|p| p.iter().zip(fill).take(3).any(|(a, b)| a.abs_diff(b) > 12)).count()
}

fn status(d: &Driver) -> String {
    d.state().status_line.clone()
}

/// The done-when at both window sizes: the indicator spins while the scan runs and draws nothing
/// at rest; the popover lists the scan with its progress rising; the status line counts up in
/// place of "No selection" and returns to it when the scan ends.
#[test]
fn a_scan_spins_the_indicator_lists_its_job_and_counts_up_on_the_status_line() {
    for size in SIZES {
        let mut t = setup(size);
        let ui_thread = dalica_fs::mark_ui_thread();
        let refused = dalica_fs::guard::refused();
        let w = size.0;

        // At rest: nothing drawn, nothing to click, "No background work", "No selection".
        let indicator = widget(&t.d, "indicator:activity");
        assert_eq!((indicator.label.as_str(), indicator.active, indicator.enabled), ("No background work", false, false));
        assert_eq!(ink(&t.d.screenshot().unwrap(), indicator.rect), 0, "{w}: the indicator draws nothing at rest");
        assert_eq!(status(&t.d), "No selection");

        // The scan starts, held before its first file: the indicator spins.
        t.d.run("folders.add", &json!({ "path": t.root })).unwrap();
        t.d.wait(100).unwrap();
        assert!(session(&t.d).busy());
        let spinning = widget(&t.d, "indicator:activity");
        assert_eq!((spinning.label.as_str(), spinning.enabled), ("Working in the background", true));
        let (a, b) = (t.d.screenshot().unwrap(), {
            t.d.wait(200).unwrap();
            t.d.screenshot().unwrap()
        });
        assert!(ink(&a, indicator.rect) > 20, "{w}: the spinner is drawn");
        assert_ne!(pixels(&a, indicator.rect), pixels(&b, indicator.rect), "{w}: it turns");
        assert_eq!(status(&t.d), "Scanning: 0 pictures found", "{w}: in place of No selection");

        // A click opens the popover: the scan's row with its Cancel.
        t.d.click("indicator:activity").unwrap();
        assert!(t.d.state().activity);
        assert!(widget(&t.d, "indicator:activity").active);
        let id = job(&t.d).job;
        let (row, cancel) = (format!("job:{id}"), format!("job:{id}/cancel"));
        let popover = widget(&t.d, "popover:activity");
        for part in [&row, &cancel] {
            let r = widget(&t.d, part).rect;
            assert!(r[0] >= popover.rect[0] && r[0] + r[2] <= popover.rect[0] + popover.rect[2] + 0.01, "{w}: {part} inside the popover");
        }
        assert!(popover.rect[1] >= indicator.rect[1] + indicator.rect[3], "{w}: under the indicator");
        assert!(popover.rect[0] + popover.rect[2] <= indicator.rect[0] + indicator.rect[2] + 0.01, "{w}: right edges aligned");
        assert_eq!(t.d.app().panels().unwrap().unregistered_widgets(), [], "{w}: every widget of the popover has an id");
        assert!(t.d.widgets().iter().all(|wi| dalica_ui_kit::registry::is_valid_id(&wi.id)));

        // One folder read: the progress rises, the status line counts it.
        give(&t.permits, PER_FOLDER);
        until_indexed(&mut t.d, PER_FOLDER);
        let first = job(&t.d);
        assert_eq!((first.state, first.progress.done), (JobState::Running, PER_FOLDER as u64));
        assert_eq!(widget(&t.d, &row).label, "Scanning: 4 pictures found");
        assert_eq!(status(&t.d), "Scanning: 4 pictures found");
        let shot = t.d.screenshot().unwrap();
        check_fingerprint(&format!("activity-{w}"), &shot, widget(&t.d, "popover:activity").rect);
        check_fingerprint(&format!("region-status-scanning-{w}"), &shot, widget(&t.d, "region:statusLine").rect);
        std::fs::write(out_dir().join(format!("activity-window-{w}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();

        // The next folder: higher still.
        give(&t.permits, PER_FOLDER);
        until_indexed(&mut t.d, 2 * PER_FOLDER);
        assert!(job(&t.d).progress.done > first.progress.done, "{w}: rising progress");
        assert_eq!(status(&t.d), "Scanning: 8 pictures found");

        // The rest: the scan ends, the popover keeps the finished job without its Cancel, the
        // status line is the selection's again, the indicator stops.
        give(&t.permits, 1000);
        t.d.step_until(|app| app.session().is_some_and(|s| !s.busy()), FRAMES).unwrap();
        t.d.wait(100).unwrap();
        assert_eq!(job(&t.d).state, JobState::Done);
        assert!(has(&t.d, &row) && !has(&t.d, &cancel), "{w}: a finished job keeps its row, not its Cancel");
        assert_eq!(status(&t.d), "No selection");
        assert_eq!(widget(&t.d, "indicator:activity").label, "No background work");
        // A press outside closes the popover; at rest the indicator draws nothing again.
        t.d.click("text:status").unwrap();
        assert!(!t.d.state().activity, "{w}: closed by a press outside");
        assert!(!has(&t.d, "popover:activity"));
        assert_eq!(ink(&t.d.screenshot().unwrap(), indicator.rect), 0, "{w}: nothing drawn at rest");
        assert!(!widget(&t.d, "indicator:activity").enabled, "{w}: inert at rest");
        assert_eq!(session(&t.d).library().photos().count(), FOLDERS.len() * PER_FOLDER);

        assert_eq!(dalica_fs::guard::refused(), refused, "{w}: the UI thread touched the disk");
        drop(ui_thread);
    }
}

/// Cancel in the popover stops the scan (what `jobs.cancel` does): the job ends cancelled with
/// fewer photos than there are, and the status line stops counting.
#[test]
fn cancel_in_the_popover_stops_the_scan() {
    let mut t = setup((1400, 900));
    let _ui_thread = dalica_fs::mark_ui_thread();
    t.d.run("folders.add", &json!({ "path": t.root })).unwrap();
    give(&t.permits, PER_FOLDER);
    until_indexed(&mut t.d, PER_FOLDER);
    t.d.click("indicator:activity").unwrap();
    let id = job(&t.d).job;
    t.d.click(&format!("job:{id}/cancel")).unwrap();
    // The held file is read, then the walk stops.
    give(&t.permits, 1000);
    t.d.step_until(|app| app.session().is_some_and(|s| !s.busy()), FRAMES).unwrap();
    t.d.wait(100).unwrap();
    let end = job(&t.d);
    assert_eq!(end.state, JobState::Cancelled, "{end:?}");
    assert!(session(&t.d).library().photos().count() < FOLDERS.len() * PER_FOLDER);
    assert_eq!(status(&t.d), "No selection");
    assert!(t.d.state().activity, "the popover stays open on its own Cancel");
    assert!(!has(&t.d, &format!("job:{id}/cancel")));
}

/// With a selection the scan is the status line's last field; Esc closes the popover; `ui.set`'s
/// `activity` opens it.
#[test]
fn with_a_selection_the_scan_is_the_last_field() {
    let mut t = setup((1400, 900));
    t.d.run("folders.add", &json!({ "path": t.root })).unwrap();
    give(&t.permits, PER_FOLDER);
    until_indexed(&mut t.d, PER_FOLDER);
    let row = t.d.widgets().into_iter().find(|w| w.id.starts_with("tree:folder:")).expect("a folder row");
    t.d.click(&row.id).unwrap();
    let text = status(&t.d);
    assert!(text.starts_with("4 pictures") && text.ends_with("     Scanning: 4 pictures found"), "{text}");
    give(&t.permits, 1000);
    t.d.step_until(|app| app.session().is_some_and(|s| !s.busy()), FRAMES).unwrap();
    t.d.wait(100).unwrap();
    let text = status(&t.d);
    assert!(text.starts_with("4 pictures") && !text.contains("Scanning"), "{text}");

    t.d.app_mut().state_mut().activity = true;
    t.d.step();
    assert!(has(&t.d, "popover:activity"));
    t.d.key("Esc").unwrap();
    assert!(!t.d.state().activity, "Esc closes it");
}
