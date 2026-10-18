//! Batch Edit, Copy and Paste All Edits, and Undo All Edits in the window (M11.18, `editing.md` §9,
//! `menus-and-shortcuts.md` §1.2 and §1.5), driven headlessly by id through the control channel as
//! an agent would:
//!
//! - Picture ▸ Batch Edit ▸ Auto Fix acts on the tray's photos (20 held ones here), and one Ctrl+Z
//!   gives all 20 their recipes back;
//! - its five effects (Sepia, Sharpen, Warmify, Film Grain, Black and White) are live, and Black and
//!   White on 3 held photos gives each a `bw()` entry as one library step ("Undo B&W");
//! - Edit ▸ Copy All Edits on one photo, then Paste All Edits on three selected, gives them its
//!   recipe; a photo holding an entry Dalica cannot read is skipped and named in a toast;
//! - in the edit view Paste All Edits is a step of the open photo's history, which its Undo button
//!   takes back;
//! - Picture ▸ Undo All Edits… asks "Remove all edits from %d pictures?" and Remove Edits removes
//!   them; the question matches its fingerprint at 1400 × 900 and fits 900 × 600
//!   (`tests/fingerprints/dialog-undo-all-1400x900.json`; `DALICA_BLESS=1` stores a new one).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, put_jpeg};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The photos by name (`IMG_0001.jpg` first).
    ids: Vec<PhotoId>,
}

/// One folder of `n` photos, `sidecar` (a `.dalica.json`) beside them when given, in the large
/// window.
fn setup(n: usize, sidecar: Option<Value>) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    for i in 1..=n {
        put_jpeg(&folder.join(format!("IMG_{i:04}.jpg")), i as u64);
    }
    if let Some(sidecar) = sidecar {
        std::fs::write(folder.join(".dalica.json"), serde_json::to_vec_pretty(&sidecar).unwrap()).unwrap();
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut named: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    let ids = named.into_iter().map(|(_, id)| id).collect();
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, ids }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    let v = call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    ok(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn menu(t: &mut T, path: &str) -> Value {
    ok(t, "ui.menu.invoke", json!({ "path": path, "wait": true }))
}

fn select(t: &mut T, ids: &[PhotoId]) {
    run(t, "select.set", json!({ "ids": ids }));
}

fn edits(t: &T, id: PhotoId) -> Vec<String> {
    t.d.app().session().unwrap().library().info(id).edits.clone()
}

#[test]
fn batch_auto_fix_on_twenty_held_photos_is_one_undo_step() {
    let mut t = setup(22, None);
    let held: Vec<PhotoId> = t.ids[..20].to_vec();
    select(&mut t, &held);
    run(&mut t, "tray.hold", json!({}));
    // Held, not selected: Batch Edit acts on the tray's photos all the same.
    select(&mut t, &[]);
    menu(&mut t, "Picture/Batch Edit/Auto Fix");
    for id in &held {
        assert_eq!(edits(&t, *id), ["autofix()"], "photo {id}");
    }
    assert!(edits(&t, t.ids[20]).is_empty() && edits(&t, t.ids[21]).is_empty(), "photos outside the tray are left alone");
    assert_eq!(t.d.app().session().unwrap().undo_stack().undo_label(), Some("Auto Fix"));
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+Z" }));
    for id in &held {
        assert!(edits(&t, *id).is_empty(), "one Ctrl+Z restores photo {id}");
    }
}

#[test]
fn the_five_effects_are_live_and_black_and_white_gives_three_held_photos_a_bw_entry() {
    let mut t = setup(4, None);
    let held: Vec<PhotoId> = t.ids[..3].to_vec();
    select(&mut t, &held);
    run(&mut t, "tray.hold", json!({}));
    select(&mut t, &[]);
    let tree = ok(&mut t, "ui.menu.tree", json!({}));
    let picture = tree.as_array().unwrap().iter().find(|m| m["label"] == "Picture").unwrap();
    let batch = picture["children"].as_array().unwrap().iter().find(|i| i["label"] == "Batch Edit").unwrap();
    let rows = batch["children"].as_array().unwrap();
    for label in ["Sepia", "Sharpen", "Warmify", "Film Grain", "Black and White"] {
        let row = rows.iter().find(|i| i["label"] == label).unwrap_or_else(|| panic!("{label}"));
        assert_eq!((row["live"].as_bool(), row["enabled"].as_bool(), row.get("reason")), (Some(true), Some(true), None), "{label}: {row}");
    }

    let steps = t.d.app().session().unwrap().undo_stack().len();
    menu(&mut t, "Picture/Batch Edit/Black and White");
    for id in &held {
        assert_eq!(edits(&t, *id), ["bw()"], "photo {id}");
    }
    assert!(edits(&t, t.ids[3]).is_empty(), "a photo outside the tray is left alone");
    let stack = t.d.app().session().unwrap().undo_stack();
    assert_eq!((stack.len(), stack.undo_label()), (steps + 1, Some("B&W")), "one library step, \"Undo B&W\"");
}

#[test]
fn copy_all_edits_then_paste_all_edits_gives_three_photos_its_recipe_and_names_what_it_skips() {
    let sidecar = json!({ "photos": { "IMG_0005.jpg": { "edits": ["legacy:redeye(1)"] } } });
    let mut t = setup(5, Some(sidecar));
    let ids = t.ids.clone();
    run(&mut t, "edit.apply", json!({ "ids": [ids[0]], "op": "autocontrast" }));
    run(&mut t, "edit.apply", json!({ "ids": [ids[0]], "op": "rotate(90)" }));
    let recipe = edits(&t, ids[0]);
    assert_eq!(recipe.len(), 2);
    let e = call(&mut t, "ui.menu.invoke", json!({ "path": "Edit/Paste All Edits", "wait": true })).unwrap_err();
    assert_eq!(e.code.as_str(), "disabled", "greyed until something is copied");
    let e = call(&mut t, "ui.menu.invoke", json!({ "path": "Edit/Copy All Edits", "wait": true })).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.message.contains(dalica_ui::commands::batch::NO_PHOTO)),
        ("disabled", true),
        "greyed with nothing selected: {}",
        e.message
    );
    select(&mut t, &[ids[0]]);
    menu(&mut t, "Edit/Copy All Edits");
    select(&mut t, &ids[1..4]);
    menu(&mut t, "Edit/Paste All Edits");
    for id in &ids[1..4] {
        assert_eq!(edits(&t, *id), recipe, "photo {id}");
    }
    assert_eq!(t.d.app().session().unwrap().undo_stack().undo_label(), Some("Paste All Edits"));

    // A photo holding an entry Dalica cannot read keeps its recipe, and a toast says so.
    select(&mut t, &[ids[4]]);
    menu(&mut t, "Edit/Paste All Edits");
    assert_eq!(edits(&t, ids[4]), ["legacy:redeye(1)"]);
    let toasts = ok(&mut t, "ui.inspect", json!({}))["toasts"].to_string();
    assert!(toasts.contains("skipped 1: IMG_0005.jpg") && toasts.contains("legacy:redeye(1)"), "{toasts}");
}

#[test]
fn in_the_edit_view_a_paste_is_a_step_the_photos_undo_button_takes_back() {
    let mut t = setup(3, None);
    let ids = t.ids.clone();
    run(&mut t, "edit.apply", json!({ "ids": [ids[0]], "op": "autofix" }));
    run(&mut t, "edit.copyRecipe", json!({ "id": ids[0] }));
    let library_steps = t.d.app().session().unwrap().undo_stack().len();
    run(&mut t, "view.edit", json!({ "id": ids[1] }));
    assert_eq!(t.d.state().edit.photo, Some(ids[1]));
    menu(&mut t, "Edit/Paste All Edits");
    assert_eq!(edits(&t, ids[1]), ["autofix()"]);
    assert_eq!(t.d.app().session().unwrap().undo_stack().len(), library_steps, "a step of the photo's history, not of the library");
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:edit.undo" }));
    assert!(edits(&t, ids[1]).is_empty(), "the Undo button takes the paste back");
}

#[test]
fn undo_all_edits_asks_then_removes_and_matches_its_fingerprint() {
    let mut t = setup(4, None);
    let ids = t.ids.clone();
    select(&mut t, &ids[..3]);
    let e = call(&mut t, "ui.menu.invoke", json!({ "path": "Picture/Undo All Edits…", "wait": true })).unwrap_err();
    assert_eq!(e.code.as_str(), "disabled", "nothing to remove");
    run(&mut t, "edit.apply", json!({ "ids": [ids[0], ids[1]], "op": "autofix" }));
    run(&mut t, "edit.apply", json!({ "ids": [ids[3]], "op": "autocolor" }));
    assert_eq!(menu(&mut t, "Picture/Undo All Edits…"), json!({ "dialog": "undoAllEdits" }));
    let dialog = ok(&mut t, "ui.inspect", json!({}))["dialog"].clone();
    assert_eq!(dialog["title"], "Undo All Edits");
    assert_eq!(dialog["fields"]["question"], "Remove all edits from 2 pictures?");
    assert_eq!(dialog["buttons"], json!(["button:dialog.removeEdits", "button:dialog.cancel"]));
    check_sizes(&mut t.d, "dialog:undoAllEdits", "dialog-undo-all");

    // Cancel removes nothing; Remove Edits removes the selected photos' edits only.
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:dialog.cancel" }));
    assert_eq!(edits(&t, ids[0]), ["autofix()"]);
    menu(&mut t, "Picture/Undo All Edits…");
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:dialog.removeEdits" }));
    assert!(t.d.state().dialogs.is_empty());
    assert!(edits(&t, ids[0]).is_empty() && edits(&t, ids[1]).is_empty());
    assert_eq!(edits(&t, ids[3]), ["autocolor()"], "a photo outside the tray keeps its edits");
    assert_eq!(t.d.app().session().unwrap().undo_stack().undo_label(), Some("Remove Edits"));
}
