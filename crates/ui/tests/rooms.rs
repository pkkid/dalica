//! The room tab strip (M11.13, `ui-layout.md` §8), driven headlessly by automation id: opening
//! Import shows the strip and the room in place of the library; the Library tab brings the light
//! table back with its scroll and selection as they were left; Import's × closes it and the strip
//! goes; closing Import while a (fake) import job copies asks "Stop importing?", Keep Importing
//! leaves it copying and Stop Import cancels the job and closes the room; the Import room's keys
//! act only while it is in front; the strip's fingerprint in the large window (the small one has to
//! fit).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_region, new_driver, try_widget, widget};
use dalica_engine::{Done, FileItem, FileJob, JobKind, JobState, Session};
use dalica_ui::state::Room;
use dalica_ui::{App, Driver};
use serde_json::json;

/// A window on a library of three folders of 20 scenes photos each (more than the light table
/// shows), at `size`.
fn driver(dir: &Path, (w, h): (u32, u32)) -> Driver {
    let files = crate::common::demo_library(3, 60, 5).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn rect(d: &Driver, id: &str) -> Option<[f32; 4]> {
    try_widget(d, id).map(|w| w.rect)
}

/// The thumbnails on screen, top to bottom, with their rects.
fn thumbs(d: &Driver) -> Vec<(String, [f32; 4])> {
    d.widgets().into_iter().filter(|w| w.id.strip_prefix("thumb:").is_some_and(|n| n.parse::<u64>().is_ok())).map(|w| (w.id, w.rect)).collect()
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

#[test]
fn import_opens_under_the_strip_and_the_library_tab_comes_back_as_it_was_left() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    assert_eq!(rect(&d, "region:tabStrip"), None, "only the Library open: no strip");
    // The light table scrolled down and a photo selected.
    d.scroll(Some("region:lightTable"), egui::vec2(0.0, -600.0)).unwrap();
    d.settle(FRAMES).unwrap();
    let (scroll, shown) = (d.state().scroll, thumbs(&d));
    assert!(scroll.section.is_some() || scroll.offset > 0.0, "scrolled: {scroll:?}");
    let picked = shown.get(2).unwrap().0.clone();
    click(&mut d, &picked);
    let selection = d.state().selection.clone();
    assert!(!selection.ids.is_empty(), "{selection:?}");

    // File ▸ Import From… (Ctrl+M): the strip under the menu bar, the Import tab in front.
    d.key("Ctrl+M").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(rect(&d, "region:tabStrip"), Some([0.0, 32.0, 1400.0, 28.0]));
    assert!(rect(&d, "region:room").is_some() && rect(&d, "region:lightTable").is_none());
    let (library, import) = (widget(&d, "tab:room/library"), widget(&d, "tab:room/import"));
    assert_eq!((library.rect[2], library.rect[3]), (190.0, 28.0), "a room tab is 190 × 28");
    assert_eq!((library.label.as_str(), import.label.as_str()), ("Library", "Import"));
    assert!(import.active && !library.active);
    assert!(rect(&d, "tab:room/library/close").is_none(), "the Library tab has no ×");
    assert_eq!(widget(&d, "tab:room/import/close").label, "Close Import");
    check_region(&d, "region:tabStrip", "rooms-strip-import-1400x900");

    // The Library tab: the light table where it was, the photo still selected, the strip kept.
    click(&mut d, "tab:room/library");
    assert_eq!(d.state().room, Room::Library);
    assert!(widget(&d, "tab:room/library").active);
    assert_eq!(d.state().scroll, scroll);
    assert_eq!(d.state().selection, selection);
    let back = thumbs(&d);
    let moved = |(id, r): &(String, [f32; 4])| back.iter().find(|(b, _)| b == id).map(|(_, br)| br[1] - r[1]);
    let delta = moved(shown.get(2).unwrap()).unwrap();
    assert_eq!(delta, 28.0, "the cells sit where they were, below the 28-high strip");
    check_region(&d, "region:tabStrip", "rooms-strip-library-1400x900");

    // Import's ×: the room closes, the strip goes, the library under the menu bar again.
    click(&mut d, "tab:room/import");
    click(&mut d, "tab:room/import/close");
    assert_eq!((d.state().rooms.as_slice(), d.state().room), (&[Room::Library][..], Room::Library));
    assert_eq!(rect(&d, "region:tabStrip"), None);
    assert_eq!(d.state().selection, selection);
}

#[test]
fn the_strip_fits_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SMALL);
    d.key("Ctrl+M").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(rect(&d, "region:tabStrip"), Some([0.0, 32.0, 900.0, 28.0]));
    assert_eq!(rect(&d, "tab:room/import").unwrap()[2], 190.0, "two tabs keep their width at 900");
    check_fits(&d, "the Import room");
    crate::common::shot(&d, "rooms-strip-import-900x600-window");
}

struct Item(PathBuf);

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.0
    }
}

/// An import job that copies nothing: each item waits until `gate` opens (at most 20 s).
fn fake_import(d: &mut Driver, gate: &Arc<AtomicBool>) -> dalica_engine::JobId {
    let gate = Arc::clone(gate);
    let job = FileJob::new(JobKind::Import, "Copying", vec![Item("a.jpg".into()), Item("b.jpg".into())], move |_| {
        let start = Instant::now();
        while !gate.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(Done { changes: Vec::new(), value: () })
    });
    d.app_mut().session_mut().unwrap().start_file_job(job)
}

#[test]
fn closing_import_while_it_copies_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    click(&mut d, "button:toolbar.import");
    let gate = Arc::new(AtomicBool::new(false));
    let job = fake_import(&mut d, &gate);
    let state = |d: &Driver| d.app().session().unwrap().job(job).map(|j| j.state);

    // The ×: the question, with Keep Importing as its Cancel button; it leaves the import running.
    click(&mut d, "tab:room/import/close");
    let kinds: Vec<&str> = d.state().dialogs.iter().map(|x| x.kind.name()).collect();
    assert_eq!(kinds, ["stopImport"]);
    assert_eq!(widget(&d, "button:dialog.cancel").label, "Keep Importing");
    assert_eq!(widget(&d, "button:dialog.stopImport").label, "Stop Import");
    click(&mut d, "button:dialog.cancel");
    assert!(d.state().dialogs.is_empty());
    assert_eq!((d.state().room, state(&d)), (Room::Import, Some(JobState::Running)));

    // Stop Import: the job is cancelled and the room closes.
    click(&mut d, "tab:room/import/close");
    click(&mut d, "button:dialog.stopImport");
    assert!(d.state().dialogs.is_empty());
    assert_eq!(d.state().rooms, [Room::Library]);
    assert!(d.app().session().unwrap().jobs().iter().any(|j| j.job == job && j.cancellable));
    gate.store(true, Ordering::SeqCst);
    let ended = d.app_mut().session_mut().unwrap().wait_job(job, Duration::from_secs(20)).unwrap();
    assert_eq!(ended.state, JobState::Cancelled);

    // No import running: the × closes at once.
    click(&mut d, "button:toolbar.import");
    click(&mut d, "tab:room/import/close");
    assert!(d.state().dialogs.is_empty());
    assert_eq!(d.state().rooms, [Room::Library]);
}

#[test]
fn the_import_rooms_keys_act_only_while_it_is_in_front() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), LARGE);
    let not_yet = dalica_ui::commands::import::NOT_YET;
    d.key("Ctrl+M").unwrap();
    d.settle(FRAMES).unwrap();
    // In front: X answers for the room; a Library key (thumbnail size) does nothing.
    let size = d.state().thumb_size;
    d.key("Ctrl+1").unwrap();
    d.key("X").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().status.as_str(), d.state().thumb_size), (not_yet, size));
    // Open behind the Library tab: X is not the room's.
    click(&mut d, "tab:room/library");
    d.key("X").unwrap();
    d.settle(FRAMES).unwrap();
    assert_ne!(d.state().status, not_yet, "X ran the Import room's command behind its tab");
}
