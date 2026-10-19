//! The collage session (`roadmap.md` M14.16; `creating.md` §3): `create.collage` and the room's
//! `collage.*` commands over a library of photos of several shapes. A collage of 12 photos lays out
//! the same after a restart with the same seed; Scramble and Shuffle change what they say; a move,
//! a swap and an order change each undo in the room's own history and leave the library's undo
//! stack alone; Remove keeps a picture in the Clips and + puts it back; deleting a custom page
//! format removes it from the Crop panel's list; Close with changes asks first; the commands' own
//! refusals; the owner thread makes no file system call.

use serde_json::{Value, json};

use crate::common::{jpeg_of, lisbon, open, shared};
use dalica_engine::Session;
use dalica_engine::cmd::collage::NOT_OPEN;

/// 12 photos of four shapes: landscape, portrait, square and wide.
fn twelve() -> Vec<Vec<u8>> {
    let shapes = [(96, 64), (64, 96), (80, 80), (120, 40)];
    (0..12).map(|i| jpeg_of(i as u64, shapes[i % 4].0, shapes[i % 4].1)).collect()
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn code(s: &mut Session, id: &str, p: Value) -> &'static str {
    s.execute(id, &p).map(|v| panic!("{id} {p} ran: {v}")).unwrap_err().code()
}

fn inspect(s: &mut Session) -> Value {
    run(s, "collage.inspect", json!({}))
}

/// Each placement's photo, centre, size and angle, in stacking order.
fn placements(s: &mut Session) -> Vec<Value> {
    inspect(s)["placements"].as_array().unwrap().iter().map(|p| json!([p["id"], p["centre"], p["size"], p["angle"]])).collect()
}

/// The photos on the page in slot order (the placements sorted by centre, then the photo).
fn slots(s: &mut Session) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> =
        inspect(s)["placements"].as_array().unwrap().iter().map(|p| (format!("{}|{}", p["centre"], p["size"]), p["id"].as_u64().unwrap())).collect();
    v.sort();
    v
}

#[test]
fn twelve_photos_lay_out_the_same_after_a_restart_with_the_same_seed() {
    let _faults = shared();
    let (lib, _disk, _folder, mut s, ids) = lisbon(&twelve(), None);
    for style in ["mosaic", "frameMosaic", "grid", "contactSheet", "multipleExposure", "pile"] {
        let opened = run(&mut s, "create.collage", json!({ "ids": ids, "seed": "5eed" }));
        assert_eq!(opened, json!({ "added": 12, "clips": 12, "left": 0, "opened": true }));
        run(&mut s, "collage.set", json!({ "style": style }));
        let v = inspect(&mut s);
        assert_eq!(v["document"]["seed"], "0000000000005eed");
        assert_eq!(v["placements"].as_array().unwrap().len(), 12, "{style}");
        assert_eq!(v["clips"][1]["aspect"].as_f64().map(|a| (a * 1000.0).round()), Some(667.0), "the shape from the model");
        run(&mut s, "collage.close", json!({ "confirm": true }));
    }
    run(&mut s, "create.collage", json!({ "ids": ids, "seed": "5eed" }));
    let before = placements(&mut s);
    // The last style is remembered: the next collage starts as it.
    assert_eq!(inspect(&mut s)["document"]["style"], "pile");
    drop(s);

    let mut s = open(lib.path());
    assert!(!s.collage().is_open(), "a collage is never reopened by itself");
    run(&mut s, "create.collage", json!({ "ids": ids, "seed": "5eed" }));
    assert_eq!(placements(&mut s), before, "the same seed lays out the same after a restart");
    run(&mut s, "collage.close", json!({}));
    run(&mut s, "create.collage", json!({ "ids": ids, "seed": "5eee" }));
    assert_ne!(placements(&mut s), before, "another seed lays out otherwise");
}

#[test]
fn scramble_and_shuffle_change_what_they_say_and_every_change_is_a_step_of_the_rooms_own_history() {
    let _faults = shared();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&twelve(), None);
    let library_steps = s.undo_stack().len();
    let ui = dalica_fs::mark_ui_thread();
    run(&mut s, "create.collage", json!({ "ids": ids, "seed": "1" }));
    run(&mut s, "collage.set", json!({ "style": "pile", "shadows": false }));
    assert_eq!(s.collage().history_depth(), (1, 0));

    // A move in the pile: one room step, undone and redone exactly.
    let before = inspect(&mut s);
    let moved = run(&mut s, "collage.move", json!({ "item": 3, "centre": [0.25, 0.75], "scale": 1.5, "angle": 350 }));
    assert_eq!(moved, json!({ "changed": true, "unsaved": true, "undo": 2, "redo": 0 }));
    let after = inspect(&mut s);
    let id = before["placements"][3]["id"].clone();
    let p = after["placements"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(p["centre"], json!([0.25, 0.75]));
    assert_eq!(p["angle"], json!(-10.0), "350° folds to -10°");
    run(&mut s, "collage.undo", json!({}));
    assert_eq!(inspect(&mut s)["document"], before["document"], "undo gives the document back exactly");
    run(&mut s, "collage.redo", json!({}));
    assert_eq!(inspect(&mut s)["document"], after["document"]);

    // Bring to Top puts the picture last in the stacking order; undo puts it back.
    let bottom = inspect(&mut s)["placements"][0]["id"].clone();
    run(&mut s, "collage.order", json!({ "item": 0, "to": "top" }));
    assert_eq!(inspect(&mut s)["placements"][11]["id"], bottom);
    run(&mut s, "collage.order", json!({ "item": 11, "to": "bottom" }));
    assert_eq!(inspect(&mut s)["placements"][0]["id"], bottom);
    run(&mut s, "collage.undo", json!({}));
    assert_eq!(inspect(&mut s)["placements"][11]["id"], bottom);
    run(&mut s, "collage.undo", json!({}));
    assert_eq!(inspect(&mut s)["placements"][0]["id"], bottom);

    // Scramble: a new seed, the moves dropped, the same pictures on the page.
    let doc = inspect(&mut s)["document"].clone();
    run(&mut s, "collage.scramble", json!({}));
    let scrambled = inspect(&mut s)["document"].clone();
    assert_ne!(scrambled["seed"], doc["seed"]);
    assert_eq!(scrambled["moves"], json!([]), "the user's placements are dropped");
    assert_eq!((&scrambled["shuffle"], &scrambled["clips"]), (&doc["shuffle"], &doc["clips"]), "the assignment stays");

    // In a grid: Shuffle keeps the slots and deals the pictures again; a swap exchanges two.
    run(&mut s, "collage.set", json!({ "style": "grid", "gridSpacing": 2.5 }));
    let grid = slots(&mut s);
    run(&mut s, "collage.shuffle", json!({}));
    let shuffled = slots(&mut s);
    let places = |v: &[(String, u64)]| v.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
    assert_eq!(places(&shuffled), places(&grid), "the same slots");
    assert_ne!(shuffled, grid, "other pictures in them");
    let two = inspect(&mut s)["placements"].clone();
    run(&mut s, "collage.swap", json!({ "a": 0, "b": 5 }));
    let swapped = inspect(&mut s)["placements"].clone();
    assert_eq!((&swapped[0]["id"], &swapped[5]["id"]), (&two[5]["id"], &two[0]["id"]));
    assert_eq!((&swapped[0]["centre"], &swapped[5]["centre"]), (&two[0]["centre"], &two[5]["centre"]), "the slots stay");
    run(&mut s, "collage.undo", json!({}));
    assert_eq!(inspect(&mut s)["placements"], two);
    assert_eq!(s.execute("collage.move", &json!({ "item": 0, "angle": 5 })).unwrap_err().code(), "disabled", "a grid's pictures swap");

    // The library's undo stack was never reached; Ctrl+Z in the room is the room's.
    assert_eq!(s.undo_stack().len(), library_steps);
    let keys = s.keymap();
    let key = |k: &str| dalica_engine::Key::parse(k).unwrap();
    assert_eq!(keys.resolve(&key("Ctrl+Z"), dalica_engine::Context::Collage, false).as_deref(), Some("collage.undo"));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
}

#[test]
fn removing_a_picture_keeps_it_in_the_clips_and_plus_puts_it_back() {
    let _faults = shared();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&twelve()[..6], None);
    run(&mut s, "create.collage", json!({ "ids": &ids[..4] }));
    let p = inspect(&mut s)["placements"].as_array().unwrap().len();
    assert_eq!(p, 4);
    run(&mut s, "collage.unplace", json!({ "ids": [ids[1]] }));
    let v = inspect(&mut s);
    assert_eq!(v["placements"].as_array().unwrap().len(), 3);
    assert_eq!(
        v["clips"][1],
        json!({ "id": ids[1], "thumb": ids[1], "name": "IMG_0002.jpg", "onPage": false, "aspect": v["clips"][1]["aspect"], "missing": false })
    );
    run(&mut s, "collage.place", json!({ "ids": [ids[1]] }));
    assert_eq!(inspect(&mut s)["placements"].as_array().unwrap().len(), 4);

    // Opening again (Back to Collage) adds the new pictures to the open collage's Clips.
    let again = run(&mut s, "create.collage", json!({ "ids": [ids[3], ids[4]] }));
    assert_eq!(again, json!({ "added": 1, "clips": 5, "left": 1, "opened": false }));
    let added = run(&mut s, "collage.clips.add", json!({ "ids": [ids[5]] }));
    assert_eq!((&added["added"], &added["clips"]), (&json!(1), &json!(6)));

    // − takes a clip out of the Clips, and its background and frame-centre roles with it.
    run(&mut s, "collage.set", json!({ "style": "frameMosaic" }));
    run(&mut s, "collage.frameCenter", json!({ "id": ids[2] }));
    run(&mut s, "collage.background", json!({ "id": ids[2] }));
    assert_eq!(inspect(&mut s)["document"]["frameCentre"], json!(ids[2]));
    run(&mut s, "collage.clips.remove", json!({ "ids": [ids[2]] }));
    let v = inspect(&mut s);
    assert_eq!(v["clips"].as_array().unwrap().len(), 5);
    assert_eq!((&v["document"]["frameCentre"], &v["document"]["background"]["picture"]), (&Value::Null, &Value::Null));
    assert_eq!(code(&mut s, "collage.place", json!({ "ids": [ids[2]] })), "not_found");
    run(&mut s, "collage.background", json!({ "id": null }));
}

#[test]
fn page_formats_are_the_crop_panels_and_deleting_a_custom_one_here_deletes_it_there() {
    let _faults = shared();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&twelve()[..3], None);
    run(&mut s, "crop.ratios.add", json!({ "name": "Panorama", "width": 3, "height": 1 }));
    run(&mut s, "create.collage", json!({ "ids": ids, "display": [1920, 1200] }));
    let names: Vec<Value> = inspect(&mut s)["formats"].as_array().unwrap().iter().map(|f| f["name"].clone()).collect();
    assert!(
        names.contains(&json!("Panorama")) && names.contains(&json!("Current Display")) && names.contains(&json!("16:9 (HD video)")),
        "{names:?}"
    );

    run(&mut s, "collage.set", json!({ "pageFormat": "Panorama", "orientation": "portrait" }));
    let v = inspect(&mut s);
    assert_eq!(v["page"], json!({ "width": 1000, "height": 3000, "ratio": 1.0 / 3.0 }));
    assert_eq!(s.settings().collage().page_format, "Panorama", "remembered for the next collage");

    let r = run(&mut s, "collage.formats.remove", json!({ "name": "Panorama" }));
    assert!(!r["formats"].as_array().unwrap().iter().any(|f| f["name"] == "Panorama"));
    let crop = run(&mut s, "crop.ratios.list", json!({}));
    assert_eq!(crop["custom"], json!([]), "gone from the Crop panel too");
    let v = inspect(&mut s);
    assert_eq!(v["document"]["page"]["format"]["ratio"]["name"], "16:9 (HD video)", "the page goes to the default format");
    assert_eq!(s.settings().collage().page_format, "16:9 (HD video)");
    assert_eq!(code(&mut s, "collage.formats.remove", json!({ "name": "Panorama" })), "not_found");

    // Current Display keeps the display's size.
    run(&mut s, "collage.set", json!({ "pageFormat": "Current Display", "orientation": "landscape" }));
    assert_eq!(inspect(&mut s)["page"], json!({ "width": 1920, "height": 1200, "ratio": 1.6 }));
    // Reset brings the Settings tab's defaults back, and remembers them.
    run(&mut s, "collage.set", json!({ "borders": "instant", "backgroundColor": "#336699", "shadows": false }));
    run(&mut s, "collage.reset", json!({}));
    assert_eq!(s.settings().collage(), dalica_engine::settings::CollageSettings::default());
    assert_eq!(inspect(&mut s)["document"]["background"]["colour"], "#000000");
}

#[test]
fn closing_with_changes_asks_first_and_the_commands_refuse_what_they_cannot_do() {
    let _faults = shared();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&twelve()[..4], None);
    // Nothing open: the room's commands are disabled; inspect says so.
    assert_eq!(inspect(&mut s), json!({ "open": false }));
    for id in ["collage.set", "collage.scramble", "collage.undo", "collage.close", "collage.clips.add"] {
        let e = s.execute(id, &json!({ "ids": [] })).unwrap_err();
        assert_eq!(e.to_string(), NOT_OPEN, "{id}");
    }
    assert_eq!(code(&mut s, "create.collage", json!({})), "disabled");
    assert_eq!(code(&mut s, "create.collage", json!({ "ids": [999_999] })), "not_found");
    assert_eq!(code(&mut s, "create.collage", json!({ "ids": [], "seed": "x" })), "bad_params");

    run(&mut s, "create.collage", json!({ "ids": ids }));
    // Unchanged: Close closes at once.
    assert_eq!(run(&mut s, "collage.close", json!({})), json!({ "closed": true, "discarded": false }));
    run(&mut s, "create.collage", json!({ "ids": ids }));
    assert_eq!(code(&mut s, "collage.undo", json!({})), "disabled");
    run(&mut s, "collage.scramble", json!({}));
    let e = s.execute("collage.close", &json!({})).unwrap_err();
    assert_eq!(e.code(), "disabled");
    assert!(e.to_string().contains("\"confirm\": true"), "{e}");
    assert!(s.collage().is_open(), "nothing closed");
    // Undoing back to the opened document leaves nothing unsaved.
    run(&mut s, "collage.undo", json!({}));
    assert!(!s.collage().is_changed());
    run(&mut s, "collage.redo", json!({}));
    assert_eq!(code(&mut s, "collage.redo", json!({})), "disabled");

    // Refusals that leave the room as it was.
    let doc = inspect(&mut s)["document"].clone();
    for (id, p, want) in [
        ("collage.set", json!({}), "bad_params"),
        ("collage.set", json!({ "style": "spiral" }), "bad_params"),
        ("collage.set", json!({ "gridSpacing": 6 }), "bad_params"),
        ("collage.set", json!({ "pageFormat": "Nowhere" }), "not_found"),
        ("collage.set", json!({ "pageFormat": "Current Display" }), "bad_params"),
        ("collage.set", json!({ "display": [0, 10] }), "bad_params"),
        ("collage.move", json!({ "item": 0 }), "bad_params"),
        ("collage.move", json!({ "item": 99, "scale": 2 }), "not_found"),
        ("collage.move", json!({ "item": 0, "scale": 50 }), "bad_params"),
        ("collage.move", json!({ "item": 0, "centre": [3, 0] }), "bad_params"),
        ("collage.swap", json!({ "a": 0, "b": 1 }), "disabled"),
        ("collage.order", json!({ "item": 0, "to": "middle" }), "bad_params"),
        ("collage.frameCenter", json!({ "id": ids[0] }), "disabled"),
        ("collage.background", json!({}), "bad_params"),
        ("collage.background", json!({ "id": 999_999 }), "not_found"),
        ("collage.unplace", json!({ "ids": [999_999] }), "not_found"),
    ] {
        assert_eq!(code(&mut s, id, p.clone()), want, "{id} {p}");
        assert_eq!(inspect(&mut s)["document"], doc, "{id} {p} changed nothing");
    }
    run(&mut s, "collage.set", json!({ "style": "grid" }));
    assert_eq!(code(&mut s, "collage.swap", json!({ "a": 1, "b": 1 })), "bad_params");

    assert_eq!(run(&mut s, "collage.close", json!({ "discard": true })), json!({ "closed": true, "discarded": true }));
    assert!(!s.collage().is_open());
}
