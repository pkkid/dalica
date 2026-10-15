//! Quick tags (M6.31, `organizing.md` §5.2), driven headlessly by widget id on a library of ten
//! scenes photos:
//!
//! - Configure Quick Tags (the pane's gear): clear "Use the top two buttons for recently used
//!   tags", type three slots, OK; the buttons show them.
//! - Quick tag 2 applied by a click and by Ctrl+Alt+2, each toggled off again, one undo step each;
//!   Ctrl+Alt+2 acts on the tray (held photos too), a click on the selection.
//! - With the box ticked, buttons 1 and 2 follow the tags added through "Add a tag:"; a "+" button
//!   opens Configure Quick Tags.
//! - Fill Empty with Common Tags fills the empty slots with the tags on the most photos; Cancel
//!   changes nothing.
//! - The pane and the dialog match their fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//!   (`tests/fingerprints/pane-tags-quick-1400x900.json`, `dialog-quick-tags-1400x900.json`;
//!   `DALICA_BLESS=1` stores new ones, renders go to `target/test-output/ui/`), and every widget
//!   shown has a valid, unique id.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_sizes, new_driver, shot};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::state::{DialogKind, Pane};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<u64>,
}

/// A library of ten photos in one folder, shown at `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, 10, 28).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let mut ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 10);
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, ids }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    t.d.settle(FRAMES).unwrap();
}

fn key(t: &mut T, chord: &str) {
    t.d.key(chord).unwrap();
    t.d.settle(FRAMES).unwrap();
}

fn type_into(t: &mut T, field: &str, text: &str) {
    click(t, field);
    t.d.type_text(text).unwrap();
    t.d.settle(FRAMES).unwrap();
}

/// Selects the first `n` photos and opens the Tags pane.
fn select_with_pane(t: &mut T, n: usize) {
    let ids: Vec<u64> = t.ids.iter().take(n).copied().collect();
    run(t, "select.set", json!({ "ids": ids }));
    if t.d.state().pane != Some(Pane::Tags) {
        run(t, "pane.tags", json!({}));
    }
}

/// The ten buttons as `(label, active, enabled)`.
fn buttons(t: &T) -> Vec<(String, bool, bool)> {
    let widgets = t.d.widgets();
    (1..=10)
        .map(|n| {
            let id = format!("quicktag:{n}");
            let w = widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}"));
            (w.label.clone(), w.active, w.enabled)
        })
        .collect()
}

fn labels(t: &T) -> Vec<String> {
    buttons(t).into_iter().map(|b| b.0).collect()
}

/// What `tags.quick.get` says now.
fn quick(t: &mut T) -> Value {
    t.d.app_mut().session_mut().unwrap().execute("tags.quick.get", &json!({})).unwrap()
}

/// How many photos hold `tag`.
fn count(t: &T, tag: &str) -> usize {
    t.d.app().session().unwrap().library().tag_index().count(tag)
}

fn holds(t: &T, id: u64, tag: &str) -> bool {
    t.d.app().session().unwrap().library().tags_of(PhotoId::new(id)).iter().any(|x| x == tag)
}

fn undo_steps(t: &T) -> usize {
    t.d.app().session().unwrap().undo_stack().len()
}

fn draft(t: &T) -> serde_json::Map<String, Value> {
    t.d.state().dialogs.last().map(|d| d.fields.clone()).expect("a dialog")
}

fn enabled(t: &T, id: &str) -> bool {
    t.d.widgets().iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}")).enabled
}

/// Opens Configure Quick Tags with the gear, clears the recent-tags box and types three slots.
fn configure_three(t: &mut T) {
    click(t, "button:quicktags.configure");
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::QuickTags));
    assert!(!enabled(t, "field:dialog.slot1") && !enabled(t, "field:dialog.slot2"), "the box ticked: 1 and 2 follow recent tags");
    click(t, "check:dialog.useRecent");
    assert_eq!(draft(t)["useRecent"], json!(false));
    assert!(enabled(t, "field:dialog.slot1") && enabled(t, "field:dialog.slot2"));
    type_into(t, "field:dialog.slot1", "lisbon");
    type_into(t, "field:dialog.slot2", "tram");
    type_into(t, "field:dialog.slot3", " beach ");
    assert_eq!(draft(t)["slots"].as_array().map(|s| s.len()), Some(10));
    click(t, "button:dialog.ok");
}

#[test]
fn three_slots_configured_then_slot_2_toggles_by_click_and_by_ctrl_alt_2() {
    let mut t = setup(1400, 900);
    select_with_pane(&mut t, 4);
    assert_eq!(labels(&t), vec!["+"; 10], "nothing assigned yet");
    configure_three(&mut t);
    assert!(t.d.state().dialogs.is_empty(), "OK closes the dialog");
    let q = quick(&mut t);
    assert_eq!(q["slots"], json!(["lisbon", "tram", "beach", null, null, null, null, null, null, null]));
    assert_eq!(q["useRecent"], json!(false));
    assert_eq!(undo_steps(&t), 0, "quick tags are a setting, not an undo step");
    assert_eq!(&labels(&t)[..4], ["lisbon", "tram", "beach", "+"]);

    // A click toggles the tag on the selection, one undo step; the button then looks pressed.
    click(&mut t, "quicktag:2");
    assert_eq!(count(&t, "tram"), 4);
    assert!((0..4).all(|i| holds(&t, t.ids[i], "tram")) && !holds(&t, t.ids[4], "tram"));
    assert_eq!(buttons(&t)[1], ("tram".to_string(), true, true));
    assert_eq!(undo_steps(&t), 1);
    click(&mut t, "quicktag:2");
    assert_eq!(count(&t, "tram"), 0, "toggled off again");
    assert!(!buttons(&t)[1].1);
    assert_eq!(undo_steps(&t), 2);

    // Ctrl+Alt+2 does the same.
    key(&mut t, "Ctrl+Alt+2");
    assert_eq!(count(&t, "tram"), 4);
    assert!(buttons(&t)[1].1);
    key(&mut t, "Ctrl+Alt+2");
    assert_eq!(count(&t, "tram"), 0);
    assert_eq!(undo_steps(&t), 4);

    // A partly tagged selection: one toggle puts the tag on all.
    let first = t.ids[0];
    run(&mut t, "photo.tags.add", json!({ "ids": [first], "tags": "beach" }));
    assert!(!buttons(&t)[2].1, "only some hold beach");
    click(&mut t, "quicktag:3");
    assert_eq!(count(&t, "beach"), 4);

    // The keys act on the tray: a held photo and the selection.
    let held = t.ids[9];
    run(&mut t, "tray.hold", json!({ "ids": [held] }));
    key(&mut t, "Ctrl+Alt+1");
    assert_eq!(count(&t, "lisbon"), 5, "the four selected and the held one");
    assert!(holds(&t, held, "lisbon"));
    // A click acts on the selection only.
    click(&mut t, "quicktag:1");
    assert_eq!(count(&t, "lisbon"), 1, "the four selected lost it, the held one kept it");
    assert!(holds(&t, held, "lisbon"));

    // With nothing selected the tag buttons are greyed and the "+" ones are not.
    run(&mut t, "select.none", json!({}));
    let b = buttons(&t);
    assert!(!b[0].2 && !b[1].2 && !b[2].2 && b[3].2, "{b:?}");
    // An unassigned key says why on the status line and changes nothing.
    key(&mut t, "Ctrl+Alt+4");
    assert_eq!(t.d.state().status, "Quick tag 4 is not assigned.");
}

#[test]
fn the_top_two_follow_recent_tags_and_a_plus_opens_the_dialog() {
    let mut t = setup(1400, 900);
    select_with_pane(&mut t, 2);
    type_into(&mut t, "field:tags.add", "harbour");
    key(&mut t, "Enter");
    type_into(&mut t, "field:tags.add", "boats");
    key(&mut t, "Enter");
    assert_eq!(&labels(&t)[..3], ["boats", "harbour", "+"], "the most recent first");
    // Toggling through a quick tag does not reorder them.
    click(&mut t, "quicktag:2");
    assert_eq!(count(&t, "harbour"), 0);
    assert_eq!(&labels(&t)[..2], ["boats", "harbour"]);
    // "+" opens Configure Quick Tags.
    click(&mut t, "quicktag:5");
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::QuickTags));
    key(&mut t, "Esc");
    assert!(t.d.state().dialogs.is_empty());
}

#[test]
fn fill_empty_uses_the_most_common_tags() {
    let mut t = setup(1400, 900);
    let ids = t.ids.clone();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..9], "tags": "sea" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..7], "tags": "Lisbon" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..5], "tags": "tram" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..2], "tags": "bus, cat" }));
    run(&mut t, "tags.quick.set", json!({ "slots": ["", "", "", "LISBON"] }));
    select_with_pane(&mut t, 1);
    click(&mut t, "button:quicktags.configure");
    click(&mut t, "button:dialog.fillCommon");
    // The box is ticked: slots 1 and 2 stay empty; Lisbon is already in slot 4.
    let slots = draft(&t)["slots"].clone();
    assert_eq!(slots, json!(["", "", "sea", "LISBON", "tram", "bus", "cat", "", "", ""]));
    // No tag is left to fill with: another click changes nothing.
    click(&mut t, "button:dialog.fillCommon");
    assert_eq!(draft(&t)["slots"], slots);
    click(&mut t, "button:dialog.ok");
    assert_eq!(quick(&mut t)["slots"], json!([null, null, "sea", "LISBON", "tram", "bus", "cat", null, null, null]));
}

#[test]
fn cancel_changes_nothing() {
    let mut t = setup(1400, 900);
    run(&mut t, "tags.quick.set", json!({ "slots": ["", "", "beach"] }));
    let before = quick(&mut t);
    select_with_pane(&mut t, 1);
    click(&mut t, "button:quicktags.configure");
    assert_eq!(draft(&t)["slots"][2], json!("beach"), "the draft starts from the stored quick tags");
    click(&mut t, "check:dialog.useRecent");
    type_into(&mut t, "field:dialog.slot4", "tram");
    click(&mut t, "button:dialog.fillCommon");
    click(&mut t, "button:dialog.cancel");
    assert!(t.d.state().dialogs.is_empty());
    assert_eq!(quick(&mut t), before);
    // Esc too.
    click(&mut t, "button:quicktags.configure");
    type_into(&mut t, "field:dialog.slot5", "bus");
    key(&mut t, "Esc");
    key(&mut t, "Esc");
    assert!(t.d.state().dialogs.is_empty());
    assert_eq!(quick(&mut t), before);
}

/// A selection with many tags scrolls its list; the ten buttons stay whole inside the pane (and its
/// 8 pt margin), in the large window and the small one.
#[test]
fn many_tags_leave_the_buttons_whole() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ids = t.ids.clone();
    let many: Vec<String> = (0..40).map(|i| format!("tag {i:02}")).collect();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..2], "tags": many }));
    for (w, h) in [LARGE, SMALL] {
        t.d.resize(w, h);
        // The pane opened again in the small window (going small closes it).
        select_with_pane(&mut t, 2);
        t.d.settle(FRAMES).unwrap();
        let widgets = t.d.widgets();
        let pane = widgets.iter().find(|w| w.id == "pane:tags").unwrap().rect;
        let pane_bottom = pane[1] + pane[3];
        for n in 1..=10 {
            let r = widgets.iter().find(|w| w.id == format!("quicktag:{n}")).unwrap_or_else(|| panic!("{w}×{h}: no quicktag:{n}")).rect;
            assert!(r[3] >= 23.5 && r[1] + r[3] <= pane_bottom - 8.0, "{w}×{h}: quicktag:{n} {r:?} inside the pane {pane:?}");
        }
    }
}

/// The pane with recent tags, three assigned slots (one held by every selected photo, one cut
/// short) and "+" buttons, and the dialog with its fields filled (which fits the small window).
#[test]
fn the_pane_and_the_dialog_match_their_fingerprints() {
    let (w, h) = LARGE;
    let mut t = setup(w, h);
    let ids = t.ids.clone();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon, tram" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..3], "tags": "beach" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "sea" }));
    run(&mut t, "tags.quick.set", json!({ "slots": ["", "", "tram", "beach", "a much longer quick tag than fits", "", "harbour"] }));
    select_with_pane(&mut t, 8);
    // The recent tags not in slots 3–10 (sea, lisbon) on the top two.
    let b = buttons(&t);
    let shown: Vec<(&str, bool)> = b.iter().take(7).map(|(l, on, _)| (l.as_str(), *on)).collect();
    assert_eq!(
        shown,
        [
            ("sea", true),
            ("lisbon", true),
            ("tram", true),
            ("beach", false),
            ("a much longer quick tag than fits", false),
            ("+", false),
            ("harbour", false)
        ],
        "{w}×{h}"
    );
    check_region(&t.d, "pane:tags", &format!("pane-tags-quick-{w}x{h}"));
    shot(&t.d, &format!("pane-tags-quick-{w}x{h}"));

    click(&mut t, "button:quicktags.configure");
    shot(&t.d, &format!("dialog-quick-tags-{w}x{h}"));
    check_sizes(&mut t.d, "dialog:quickTags", "dialog-quick-tags");
}
