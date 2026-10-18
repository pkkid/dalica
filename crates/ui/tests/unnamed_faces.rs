//! The Unnamed grid (M13.21, `organizing.md` §6.2–§6.3), driven headlessly by widget id on a
//! library of seven photos with one face each, planted in three groups (sizes 2, 4 and 1 in photo
//! order; the face index plants groups from M13.18, until then the test plants them for its
//! thread, `panels::faces::model::plant_groups`):
//!
//! - the groups show largest first (`group:0` holds the four);
//! - typing "Ana" (a person) under a group and Enter names the whole group as one undo step, and it
//!   leaves the grid; a new name opens New Person (Name prefilled, a Nickname), whose OK makes the
//!   person and names the group as one step;
//! - a group's × asks "Ignore these faces?" and ignores them; Show Ignored Faces shows them;
//!   "Don't ask again" turns the question off;
//! - a rubber band selects faces across groups, and Delete ignores them;
//! - the face menu's Move to Person ▸ lists the people by name and moves the face;
//! - at 900 × 600 the grid fits; at 1400 × 900 it matches `tests/fingerprints/unnamed-grid-1400x900.json`
//!   (`DALICA_BLESS=1` stores a new one; renders go to `target/test-output/ui/`).

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, shot, widget};
use dalica_core::PersonId;
use dalica_engine::Session;
use dalica_ui::headless::{Click, PointerEvent, PointerKind};
use dalica_ui::panels::faces::model::plant_groups;
use dalica_ui::panels::faces::{FACE_SIDE, picture_of};
use dalica_ui::panels::people_pane::pictures::side_px;
use dalica_ui::state::{DialogKind, FaceRef};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    /// The faces, one per photo, in photo order.
    faces: Vec<FaceRef>,
}

/// The planted groups of the faces, in photo order: two groups and a face alone.
const GROUPS: [u64; 7] = [20, 10, 10, 20, 10, 10, 30];

/// Seven photos with a face each, grouped as [`GROUPS`] says, the Unnamed grid open at `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 7, 41).unwrap();
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
    assert_eq!(ids.len(), 7);
    let mut faces = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let x = 0.2 + 0.05 * i as f64;
        let r = s.execute("faces.add", &json!({ "id": id, "rect": [x, 0.15, x + 0.35, 0.75] })).unwrap();
        faces.push(r["face"].as_str().unwrap().parse::<FaceRef>().unwrap());
    }
    plant_groups(&faces.iter().zip(GROUPS).map(|(f, g)| (*f, g)).collect::<Vec<_>>());
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.app_mut().state_mut().unnamed.open = true;
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, faces }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

/// The faces shown in card `group` (`group:<n>`), in the order shown.
fn faces_in(t: &T, group: &str) -> Vec<FaceRef> {
    let Some(card) = t.d.widgets().into_iter().find(|w| w.id == group) else { return Vec::new() };
    let [x, y, w, h] = card.rect;
    t.d.widgets()
        .into_iter()
        .filter(|f| f.id.starts_with("face:") && !f.id.contains('/'))
        .filter(|f| {
            let c = Pos2::new(f.rect[0] + f.rect[2] / 2.0, f.rect[1] + f.rect[3] / 2.0);
            c.x >= x && c.x <= x + w && c.y >= y && c.y <= y + h
        })
        .map(|f| f.id["face:".len()..].parse().unwrap())
        .collect()
}

/// The person a face is named as, if any.
fn person_of(t: &T, f: FaceRef) -> Option<PersonId> {
    let lib = t.d.app().session().unwrap().library();
    match &lib.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) }).unwrap().person {
        dalica_library::FacePerson::Person(id) => Some(id.clone()),
        _ => None,
    }
}

fn ignored(t: &T, f: FaceRef) -> bool {
    let lib = t.d.app().session().unwrap().library();
    lib.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) }).unwrap().person == dalica_library::FacePerson::Ignored
}

/// The faces of `GROUPS` group `g`.
fn planted(t: &T, g: u64) -> Vec<FaceRef> {
    t.faces.iter().zip(GROUPS).filter(|(_, x)| *x == g).map(|(f, _)| *f).collect()
}

/// Types `text` into widget `id` and presses Enter.
fn type_and_enter(t: &mut T, id: &str, text: &str) {
    t.d.click(id).unwrap();
    t.d.type_text(text).unwrap();
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
}

#[test]
fn groups_show_largest_first_and_naming_one_is_one_step() {
    let mut t = setup(LARGE.0, LARGE.1);
    assert_eq!(faces_in(&t, "group:0"), planted(&t, 10), "the four first");
    assert_eq!(faces_in(&t, "group:1"), planted(&t, 20));
    assert_eq!(faces_in(&t, "group:2"), planted(&t, 30));
    assert!(!has(&t, "group:3") && !has(&t, "group:ignored"));
    assert_eq!(widget(&t.d, "group:0").label, "4 faces");
    let ana = run(&mut t, "people.create", json!({ "name": "Ana" }))["person"].as_str().unwrap().to_string();
    // "ana" types the name of a person: the group is hers at once, in one step, and leaves.
    type_and_enter(&mut t, "field:faces.name/0", "ana");
    assert!(t.d.app().state().dialogs.is_empty(), "no New Person for a known name");
    for f in planted(&t, 10) {
        assert_eq!(person_of(&t, f).map(|p| p.to_string()), Some(ana.clone()), "{f}");
        assert!(!has(&t, &f.widget()), "{f} left the grid");
    }
    assert_eq!(faces_in(&t, "group:0"), planted(&t, 20), "the others moved up");
    assert!(t.d.app().state().unnamed.names.is_empty(), "the name typed went with its group");
    run(&mut t, "edit.undo", json!({}));
    assert!(planted(&t, 10).iter().all(|f| person_of(&t, *f).is_none()), "one undo step names none of them");
    assert_eq!(faces_in(&t, "group:0"), planted(&t, 10));
}

#[test]
fn a_new_name_opens_new_person_and_ok_names_the_group() {
    let mut t = setup(LARGE.0, LARGE.1);
    type_and_enter(&mut t, "field:faces.name/1", "Rui");
    let dialog = t.d.app().state().dialogs.last().cloned().expect("New Person");
    assert_eq!(dialog.kind, DialogKind::NewPerson);
    assert_eq!(dialog.fields["name"], "Rui");
    assert_eq!(dialog.fields["faces"], json!(planted(&t, 20)));
    shot(&t.d, "unnamed-new-person-1400x900");
    t.d.click("field:dialog.nickname").unwrap();
    t.d.type_text("Ruizinho").unwrap();
    t.d.click("button:dialog.ok").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.app().state().dialogs.is_empty());
    let lib = t.d.app().session().unwrap().library();
    let rui = lib.person_by_name("Rui").cloned().expect("Rui was made");
    assert_eq!(lib.person(&rui).unwrap().nickname, "Ruizinho");
    assert!(planted(&t, 20).iter().all(|f| person_of(&t, *f).as_ref() == Some(&rui)));
    run(&mut t, "edit.undo", json!({}));
    let lib = t.d.app().session().unwrap().library();
    assert!(lib.person_by_name("Rui").is_none() && planted(&t, 20).iter().all(|f| person_of(&t, *f).is_none()), "one step");
    // Move to New Person… asks the same for the faces selected, its Name empty.
    let face = planted(&t, 30)[0];
    t.d.click(&face.widget()).unwrap();
    run(&mut t, "faces.moveToNewPerson", json!({}));
    let dialog = t.d.app().state().dialogs.last().cloned().unwrap();
    assert_eq!((dialog.fields["name"].clone(), dialog.fields["faces"].clone()), (json!(""), json!([face])));
    assert!(!widget(&t.d, "button:dialog.ok").enabled, "OK waits for a name");
}

#[test]
fn the_cross_asks_and_ignores_and_show_ignored_shows_them() {
    let mut t = setup(LARGE.0, LARGE.1);
    let lone = planted(&t, 30)[0];
    t.d.click("group:2/ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().state().dialogs.last().map(|d| d.kind), Some(DialogKind::IgnoreFaces));
    assert!(!ignored(&t, lone), "nothing before the answer");
    t.d.click("button:dialog.ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(ignored(&t, lone) && !has(&t, "group:2") && !has(&t, &lone.widget()));
    t.d.click("button:faces.showIgnored").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(faces_in(&t, "group:ignored"), [lone]);
    // "Don't ask again": the next × ignores at once.
    t.d.click("group:1/ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    t.d.click("check:dialog.dontAsk").unwrap();
    t.d.click("button:dialog.ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(planted(&t, 20).iter().all(|f| ignored(&t, *f)));
    t.d.click("group:0/ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.app().state().dialogs.is_empty(), "not asked again");
    assert!(planted(&t, 10).iter().all(|f| ignored(&t, *f)));
    assert_eq!(faces_in(&t, "group:ignored").len(), 7);
    // Remove from Person on an ignored face puts it back in Unnamed.
    run(&mut t, "faces.removeFromPerson", json!({ "faces": [lone] }));
    assert!(!ignored(&t, lone) && faces_in(&t, "group:0") == [lone]);
}

#[test]
fn a_rubber_band_selects_faces_across_groups_and_delete_ignores_them() {
    let mut t = setup(LARGE.0, LARGE.1);
    let (g0, g1) = (widget(&t.d, "group:0").rect, widget(&t.d, "group:1").rect);
    assert!(g1[0] > g0[0] && (g1[1] - g0[1]).abs() < 0.5, "side by side");
    // From the empty space under group 1, up and left into the last two faces of group 0.
    let start = Pos2::new(g1[0] + g1[2] - 4.0, g1[1] + g1[3] + 40.0);
    let last_two = &planted(&t, 10)[2..];
    let into = widget(&t.d, &last_two[0].widget()).rect;
    let end = Pos2::new(into[0] + into[2] / 2.0, into[1] + into[3] / 2.0);
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
    for i in 1..=10 {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: start.lerp(end, i as f32 / 10.0) });
    }
    events.push(PointerEvent { kind: PointerKind::Up, pos: end });
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.settle(FRAMES).unwrap();
    let mut selected = t.d.app().state().unnamed.selection.clone();
    selected.sort();
    let mut expected: Vec<FaceRef> = last_two.iter().copied().chain(planted(&t, 20)).collect();
    expected.sort();
    assert_eq!(selected, expected, "two of group 0 and both of group 1");
    assert!(widget(&t.d, &last_two[0].widget()).active && widget(&t.d, "group:1").active);
    // Ctrl-click takes one away; Delete asks, then ignores the rest as one step.
    t.d.click_with(&last_two[0].widget(), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    assert_eq!(t.d.app().state().unnamed.selection.len(), 3);
    t.d.key("Delete").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().state().dialogs.last().map(|d| d.kind), Some(DialogKind::IgnoreFaces));
    t.d.click("button:dialog.ignore").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(ignored(&t, last_two[1]) && planted(&t, 20).iter().all(|f| ignored(&t, *f)) && !ignored(&t, last_two[0]));
    assert!(t.d.app().state().unnamed.selection.is_empty(), "ignored faces leave the selection");
    run(&mut t, "edit.undo", json!({}));
    assert!(!ignored(&t, last_two[1]) && planted(&t, 20).iter().all(|f| !ignored(&t, *f)));
}

#[test]
fn dragging_a_face_carries_the_faces_selected() {
    let mut t = setup(LARGE.0, LARGE.1);
    let two = planted(&t, 20);
    t.d.click(&two[0].widget()).unwrap();
    t.d.click_with(&two[1].widget(), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    let from = widget(&t.d, &two[1].widget()).rect;
    let start = Pos2::new(from[0] + from[2] / 2.0, from[1] + from[3] / 2.0);
    let over = Pos2::new(start.x + 200.0, start.y + 300.0);
    let events = [
        PointerEvent { kind: PointerKind::Move, pos: start },
        PointerEvent { kind: PointerKind::Down, pos: start },
        PointerEvent { kind: PointerKind::Drag, pos: start.lerp(over, 0.5) },
        PointerEvent { kind: PointerKind::Drag, pos: over },
    ];
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    let drag = t.d.app().state().drag.clone().expect("a drag of faces");
    let mut carried = drag.faces.clone();
    carried.sort();
    assert_eq!((carried, drag.ids.is_empty(), drag.from), (two.clone(), true, two[1].widget()));
    assert_eq!(widget(&t.d, "drag:pill").label, "2 faces");
    t.d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: over }], PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.app().state().drag.is_none() && two.iter().all(|f| person_of(&t, *f).is_none()), "dropped on nothing: no change");
}

#[test]
fn the_face_menu_lists_people_and_moves_a_face() {
    let mut t = setup(LARGE.0, LARGE.1);
    let rui = run(&mut t, "people.create", json!({ "name": "Rui" }))["person"].as_str().unwrap().to_string();
    let ana = run(&mut t, "people.create", json!({ "name": "ana" }))["person"].as_str().unwrap().to_string();
    let face = planted(&t, 30)[0];
    t.d.click_with(&face.widget(), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().state().unnamed.selection, [face], "a right-click selects the face");
    for row in ["menuitem:faces.moveToNewPerson", "menuitem:faces.removeFromPerson", "menuitem:faces.ignoreSelected"] {
        assert!(has(&t, row), "{row}");
    }
    assert!(!widget(&t.d, "menuitem:faces.removeFromPerson").enabled, "the face is nobody's");
    t.d.hover("submenu:Face/Move to Person", 300).unwrap();
    t.d.settle(FRAMES).unwrap();
    let rows: Vec<String> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:faces.moveToPerson/")).map(|w| w.id).collect();
    assert_eq!(rows, [format!("menuitem:faces.moveToPerson/{ana}"), format!("menuitem:faces.moveToPerson/{rui}")], "by name");
    t.d.click(&format!("menuitem:faces.moveToPerson/{rui}")).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(person_of(&t, face).map(|p| p.to_string()), Some(rui));
    assert!(!has(&t, "group:2"), "the face left the grid");
}

#[test]
fn the_grid_matches_its_fingerprint_and_fits_the_small_window() {
    let mut t = setup(LARGE.0, LARGE.1);
    // The face pictures are cut on a worker: wait until every face has one.
    let shown = |t: &T| {
        let side = side_px(t.d.ctx(), FACE_SIDE);
        t.faces.iter().all(|f| picture_of(t.d.app(), t.d.ctx(), *f, side).is_some())
    };
    for _ in 0..200 {
        let done = shown(&t);
        if done {
            break;
        }
        t.d.app_mut().session_mut().unwrap().wait_face_tiles(Duration::from_secs(5));
        t.d.settle(FRAMES).unwrap();
    }
    assert!(shown(&t), "every face has its picture");
    check_sizes(&mut t.d, "region:lightTable", "unnamed-grid");
    // Screenshots for the review: a name being typed with the people offered, and the face menu.
    run(&mut t, "people.create", json!({ "name": "Ana" }));
    t.d.click("field:faces.name/1").unwrap();
    t.d.type_text("A").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(has(&t, "field:faces.name/1/option/0"), "Ana is offered");
    shot(&t.d, "unnamed-name-1400x900");
    t.d.key("Escape").unwrap();
    t.d.key("Escape").unwrap();
    let face = planted(&t, 10)[0];
    t.d.click_with(&face.widget(), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    t.d.hover("submenu:Face/Move to Person", 300).unwrap();
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "unnamed-menu-1400x900");
    t.d.key("Escape").unwrap();
    t.d.key("Escape").unwrap();
    t.d.resize(900, 600);
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "unnamed-grid-900x600");
}
