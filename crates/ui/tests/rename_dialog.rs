//! Rename Files (M9.20, `organizing.md` §9), driven headlessly by widget id on a temporary folder
//! of three dated photos:
//!
//! - F2 on three photos opens it: "3 files selected. Enter a new name:", the name field focused
//!   and empty, Rename greyed; the example line follows the typing and both boxes, reading exactly
//!   as the engine names the first photo;
//! - Rename renames the three files `-001`, `-002`, `-003` in light-table order, as one undo step,
//!   and Edit ▸ Undo puts the names back; Cancel and Esc rename nothing;
//! - every widget has an id, and the dialog matches its fingerprints at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/dialog-rename-files-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones,
//!   renders go to `target/test-output/ui/`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, check_region, new_driver, put_dated_jpeg, try_widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::dialogs::rename_files::{self, ADD_DATE_ID, ADD_SIZE_ID, NAME_ID};
use dalica_ui::state::RenameDraft;
use dalica_ui::{App, Driver};
use serde_json::json;

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const DIALOG: &str = "dialog:renameFiles";
const RENAME: &str = "button:dialog.rename";

struct Fx {
    _dir: tempfile::TempDir,
    folder: PathBuf,
    /// The photos in light-table order: IMG_0001 (June 4), IMG_0000 (June 5), IMG_0002 (June 6).
    ids: Vec<PhotoId>,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    for (i, day) in [(2026, 6, 5), (2026, 6, 4), (2026, 6, 6)].into_iter().enumerate() {
        put_dated_jpeg(&folder.join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let by_name = |s: &Session, n: &str| s.library().photos().find(|p| p.name == n).unwrap().id;
    let ids = vec![by_name(&s, "IMG_0001.jpg"), by_name(&s, "IMG_0000.jpg"), by_name(&s, "IMG_0002.jpg")];
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, folder, ids, d }
}

fn enabled(d: &Driver, id: &str) -> bool {
    try_widget(d, id).unwrap_or_else(|| panic!("no {id}")).enabled
}

fn draft(d: &Driver) -> RenameDraft {
    d.state().rename_files.clone().expect("Rename Files is open")
}

fn example(d: &Driver) -> String {
    rename_files::texts(d.app()).expect("Rename Files is open").1
}

/// The files in the folder, sorted.
fn on_disk(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    names
}

/// Each photo's name in the library, in `ids` order.
fn names(d: &Driver, ids: &[PhotoId]) -> Vec<String> {
    let lib = d.app().session().unwrap().library();
    ids.iter().map(|id| lib.photo(*id).unwrap().name.clone()).collect()
}

fn undo_steps(d: &Driver) -> (usize, Option<&'static str>) {
    let u = d.app().session().unwrap().undo_stack();
    (u.len(), u.undo_label())
}

/// F2 with `ids` selected.
fn open(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
    d.key("F2").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(d, DIALOG).is_some(), "F2 opens Rename Files");
}

/// Runs frames until the file jobs are done and their results are in the library.
fn settle_jobs(d: &mut Driver) {
    let s = d.settle_for(FRAMES, true);
    assert!(s.settled, "still busy: {:?}", s.busy);
}

/// F2 on three photos: the heading counts them, the example follows the typing and the boxes,
/// Rename renames the three files in light-table order as one undo step, and Edit ▸ Undo puts the
/// names back.
#[test]
fn f2_on_three_photos_renames_them_and_undo_puts_the_names_back() {
    let mut fx = fixture((1400, 900));
    let ids = fx.ids.clone();
    let before = vec!["IMG_0000.jpg".to_string(), "IMG_0001.jpg".into(), "IMG_0002.jpg".into()];
    assert_eq!(on_disk(&fx.folder), before);
    // In selection order the last photo first: the example still names the first in light-table
    // order (IMG_0001, the oldest).
    open(&mut fx.d, &[ids[2], ids[0], ids[1]]);
    let (heading, ex) = rename_files::texts(fx.d.app()).unwrap();
    assert_eq!(heading, "3 files selected. Enter a new name:");
    assert_eq!(ex, "Example: IMG_0001.jpg", "nothing typed: the first photo's name as it is");
    assert_eq!(draft(&fx.d), RenameDraft { ids: vec![ids[2], ids[0], ids[1]], name: String::new(), add_date: false, add_size: false });
    assert!(try_widget(&fx.d, NAME_ID).unwrap().active && fx.d.ctx().egui_wants_keyboard_input(), "the name field has the focus");
    assert!(!enabled(&fx.d, RENAME), "Rename waits for a name");
    for id in [NAME_ID, ADD_DATE_ID, ADD_SIZE_ID, "button:dialog.cancel"] {
        assert!(enabled(&fx.d, id), "{id}");
    }

    // The example follows the typing and both boxes.
    fx.d.type_text("Lisbon").unwrap();
    assert_eq!(draft(&fx.d).name, "Lisbon");
    assert_eq!(example(&fx.d), "Example: Lisbon-001.jpg");
    assert!(enabled(&fx.d, RENAME));
    fx.d.click(ADD_DATE_ID).unwrap();
    assert!(draft(&fx.d).add_date && try_widget(&fx.d, ADD_DATE_ID).unwrap().active);
    assert_eq!(example(&fx.d), "Example: Lisbon-2026-06-04-001.jpg");
    fx.d.click(ADD_SIZE_ID).unwrap();
    assert_eq!(example(&fx.d), "Example: Lisbon-2026-06-04-96x64-001.jpg");
    fx.d.click(ADD_DATE_ID).unwrap();
    assert_eq!(example(&fx.d), "Example: Lisbon-96x64-001.jpg");
    fx.d.click(ADD_SIZE_ID).unwrap();
    // Back in the field (a click on a box took the focus), at the end of the name.
    fx.d.click(NAME_ID).unwrap();
    fx.d.key("End").unwrap();
    fx.d.type_text(" Trip").unwrap();
    assert_eq!(example(&fx.d), "Example: Lisbon Trip-001.jpg");
    fx.d.key("Backspace").unwrap();
    for _ in 0..4 {
        fx.d.key("Backspace").unwrap();
    }
    assert_eq!(example(&fx.d), "Example: Lisbon-001.jpg");
    // The line drawn is the one the engine's names give.
    let lib = fx.d.app().session().unwrap().library();
    let first = lib.photo(ids[0]).unwrap();
    assert_eq!(example(&fx.d), format!("Example: {}", rename_files::example_name(lib, first, &draft(&fx.d))));
    assert_eq!(undo_steps(&fx.d).0, 0, "nothing is applied before Rename");
    assert_eq!(on_disk(&fx.folder), before);

    // Rename: one undo step, `-001` … in light-table order.
    fx.d.click(RENAME).unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty() && fx.d.state().rename_files.is_none(), "Rename closes the dialog");
    assert_eq!(on_disk(&fx.folder), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"]);
    assert_eq!(names(&fx.d, &ids), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"], "the ids stay, oldest first");
    assert_eq!(undo_steps(&fx.d), (1, Some("Rename")));

    // Edit ▸ Undo puts the names back.
    fx.d.click("menu:Edit").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("menuitem:edit.undo").unwrap();
    settle_jobs(&mut fx.d);
    assert_eq!(on_disk(&fx.folder), before);
    assert_eq!(names(&fx.d, &ids), ["IMG_0001.jpg", "IMG_0000.jpg", "IMG_0002.jpg"]);
}

/// Cancel and Esc rename nothing; a blank name keeps Rename greyed; with nothing selected F2 opens
/// nothing; one photo takes the name without a number.
#[test]
fn cancel_and_esc_rename_nothing_and_one_photo_gets_no_number() {
    let mut fx = fixture((1400, 900));
    let ids = fx.ids.clone();
    let before = on_disk(&fx.folder);
    // Nothing selected: F2 opens nothing and says why.
    fx.d.key("F2").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DIALOG).is_none());
    assert_eq!(fx.d.state().status, "Select some photos first.");

    open(&mut fx.d, &ids[..1]);
    assert_eq!(rename_files::texts(fx.d.app()).unwrap().0, "1 file selected. Enter a new name:");
    fx.d.type_text("   ").unwrap();
    assert!(!enabled(&fx.d, RENAME), "a blank name keeps Rename greyed");
    fx.d.key("Ctrl+A").unwrap();
    fx.d.type_text("Tram").unwrap();
    assert_eq!(example(&fx.d), "Example: Tram.jpg", "one photo: no number");
    fx.d.click("button:dialog.cancel").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty() && fx.d.state().rename_files.is_none());

    open(&mut fx.d, &ids[..1]);
    assert_eq!(draft(&fx.d).name, "", "each time it starts empty");
    fx.d.type_text("Tram").unwrap();
    fx.d.key("Escape").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    settle_jobs(&mut fx.d);
    assert_eq!((on_disk(&fx.folder), undo_steps(&fx.d).0), (before, 0), "Cancel and Esc rename nothing");

    // Rename on one photo: its own extension, no number.
    open(&mut fx.d, &ids[..1]);
    fx.d.type_text("Tram").unwrap();
    fx.d.click(RENAME).unwrap();
    settle_jobs(&mut fx.d);
    assert_eq!(names(&fx.d, &ids[..1]), ["Tram.jpg"]);
    assert!(on_disk(&fx.folder).contains(&"Tram.jpg".to_string()));
}

/// Every widget has an id, and the dialog (a name typed, the date box ticked) matches its
/// fingerprints at both window sizes.
#[test]
fn the_dialog_matches_its_fingerprints_at_both_sizes() {
    for size in SIZES {
        let (w, h) = size;
        let mut fx = fixture(size);
        let ids = fx.ids.clone();
        open(&mut fx.d, &ids);
        fx.d.type_text("Lisbon").unwrap();
        fx.d.click(ADD_DATE_ID).unwrap();
        fx.d.settle(FRAMES).unwrap();
        assert_eq!(example(&fx.d), "Example: Lisbon-2026-06-04-001.jpg");
        check_region(&fx.d, DIALOG, &format!("dialog-rename-files-{w}x{h}"));
    }
}
