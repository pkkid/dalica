//! The People pane, the face boxes in the edit view and Add a Person Manually (M13.24,
//! `organizing.md` §6.4), driven headlessly by widget id on a library of four drawn photos with
//! faces planted through the engine (Ana on the first two, Rui on the second and third, one
//! unnamed face on the first):
//!
//! - the pane lists the selection's people with counts and its unnamed faces;
//! - naming an unnamed face from the pane is one undo step (a new name asks New Person);
//! - hovering a face in the pane highlights its box on the photo, and back;
//! - a box drawn by hand on a photo turned 90° and named lands where it was drawn after a restart;
//! - two people selected in the pane filter the light table, and Show All clears it;
//! - at 900 × 600 the pane fits; fingerprints of the pane and of the face boxes (with and without
//!   the box drawn by hand) at 1400 × 900 (`tests/fingerprints/people-pane-*.json`,
//!   `edit-faces-*.json`; `DALICA_BLESS=1` stores new ones, renders go to `target/test-output/ui/`).
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_region, new_driver, shot, try_widget, widget};
use dalica_core::{PersonId, PhotoId};
use dalica_engine::Session;
use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata};
use dalica_ui::headless::{Click, Drag, Target};
use dalica_ui::panels::edit::faces as boxes;
use dalica_ui::panels::people_pane as pane;
use dalica_ui::state::{DialogKind, FaceRef, Pane};
use dalica_ui::{App, Driver};
use egui::{Modifiers, vec2};
use serde_json::{Value, json};

const SIZE: (u32, u32) = (480, 320);

/// The faces planted: photo, rectangle (stored frame), person (`None`: unnamed).
const FACES: &[(usize, [f64; 4], Option<&str>)] = &[
    (0, [0.2, 0.25, 0.35, 0.6], Some("Ana")),
    (0, [0.6, 0.3, 0.75, 0.65], None),
    (1, [0.15, 0.2, 0.3, 0.55], Some("Ana")),
    (1, [0.55, 0.25, 0.7, 0.6], Some("Rui")),
    (2, [0.4, 0.3, 0.55, 0.65], Some("Rui")),
];

/// Photo `n`: a soft gradient in its own hue with a lighter oval at each of its faces.
fn paint(n: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity((SIZE.0 * SIZE.1 * 4) as usize);
    let tint = [[70u8, 96, 130], [110, 90, 70], [80, 120, 90], [120, 100, 130]][n % 4];
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let (u, v) = (f64::from(x) / f64::from(SIZE.0), f64::from(y) / f64::from(SIZE.1));
            let shade = 0.6 + 0.4 * v;
            let mut c = tint.map(|c| (f64::from(c) * shade) as u8);
            for (_, r, _) in FACES.iter().filter(|(p, _, _)| *p == n) {
                let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                let (rx, ry) = ((r[2] - r[0]) / 2.0, (r[3] - r[1]) / 2.0);
                if ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) <= 0.8 {
                    c = [226, 186, 160];
                }
            }
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    data
}

struct T {
    dir: tempfile::TempDir,
    d: Driver,
    /// The photos in the order painted.
    photos: Vec<PhotoId>,
    ana: PersonId,
    rui: PersonId,
}

fn open(dir: &Path) -> Session {
    Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap()
}

fn photo_ids(s: &Session) -> Vec<PhotoId> {
    let mut named: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    named.into_iter().map(|(_, id)| id).collect()
}

fn person(s: &Session, name: &str) -> PersonId {
    s.library().person_by_name(name).cloned().unwrap()
}

/// Four photos with their faces, shown at `LARGE`.
fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    for n in 0..4 {
        let png = encode::png(&Image::rgba8(&Rgba8 { width: SIZE.0, height: SIZE.1, data: paint(n) }), &Metadata::default()).unwrap();
        std::fs::write(folder.join(format!("P{n}.png")), png).unwrap();
    }
    let mut s = open(dir.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let photos = photo_ids(&s);
    assert_eq!(photos.len(), 4);
    for name in ["Ana", "Rui"] {
        s.execute("people.create", &json!({ "name": name })).unwrap();
    }
    for (n, rect, who) in FACES {
        let mut p = json!({ "id": photos[*n], "rect": rect });
        if let Some(who) = who {
            p["person"] = json!(person(&s, who));
        }
        s.execute("faces.add", &p).unwrap();
    }
    let (ana, rui) = (person(&s, "Ana"), person(&s, "Rui"));
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    T { dir, d, photos, ana, rui }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    settle(t);
    v
}

fn settle(t: &mut T) {
    t.d.settle(FRAMES).unwrap();
}

/// Lets the face pictures asked for arrive, then settles.
fn pictures(t: &mut T) {
    settle(t);
    assert!(t.d.app_mut().session_mut().unwrap().wait_face_tiles(Duration::from_secs(30)), "the face tiles arrive");
    settle(t);
}

fn face(t: &T, photo: usize, index: u16) -> FaceRef {
    FaceRef { photo: t.photos[photo], index }
}

fn has(t: &T, id: &str) -> bool {
    try_widget(&t.d, id).is_some()
}

/// The People pane open on the first three photos selected.
fn pane_on_three(t: &mut T) {
    let three = &t.photos[..3];
    run(t, "select.set", json!({ "ids": three }));
    if t.d.state().pane != Some(Pane::People) {
        run(t, "pane.people", json!({}));
    }
    assert_eq!(t.d.state().pane, Some(Pane::People));
}

/// Photo `n` in the edit view with the People pane open.
fn edit_with_pane(t: &mut T, n: usize) {
    let id = t.photos[n];
    run(t, "view.edit", json!({ "id": id }));
    if t.d.state().pane != Some(Pane::People) {
        run(t, "pane.people", json!({}));
    }
}

#[test]
fn the_pane_lists_the_selections_people_with_counts_and_fits_at_900_by_600() {
    let mut t = setup();
    pane_on_three(&mut t);
    pictures(&mut t);
    let rows: Vec<(String, String)> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("person:")).map(|w| (w.id, w.label)).collect();
    assert_eq!(
        rows,
        [(pane::person_id(&t.ana), "Ana, 2 photos".to_string()), (pane::person_id(&t.rui), "Rui, 2 photos".to_string())],
        "by name, with how many of the selected photos hold them"
    );
    let unnamed = face(&t, 0, 1);
    assert!(has(&t, &unnamed.widget()) && has(&t, &format!("{}/ignore", unnamed.widget())));
    assert!(has(&t, &pane::name_field_id(unnamed)));
    assert!(!has(&t, &face(&t, 0, 0).widget()), "a named face is its person's row");
    assert!(!has(&t, pane::ADD_MANUALLY_ID), "the edit view only");
    assert!(widget(&t.d, pane::REGION_ID).rect == widget(&t.d, "pane:people").rect, "the region is the pane's body");
    // One photo: its people only.
    let last = t.photos[2];
    run(&mut t, "select.set", json!({ "ids": [last] }));
    let rows: Vec<String> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("person:")).map(|w| w.label).collect();
    assert_eq!(rows, ["Rui, 1 photo"]);
    assert!(!has(&t, &unnamed.widget()));

    pane_on_three(&mut t);
    pictures(&mut t);
    shot(&t.d, "people-pane-window-1400x900");
    check_region(&t.d, pane::REGION_ID, "people-pane-1400x900");
    // Narrowing the window closes a pane that no longer fits (`ui-layout.md` §11); opened on
    // purpose at 900 × 600 it stays, and fits.
    t.d.resize(SMALL.0, SMALL.1);
    settle(&mut t);
    pane_on_three(&mut t);
    pictures(&mut t);
    let r = widget(&t.d, pane::REGION_ID).rect;
    assert!(r[2] >= 280.0 - 0.01 && r[0] + r[2] <= SMALL.0 as f32 + 0.01 && r[1] + r[3] <= SMALL.1 as f32 + 0.01, "{r:?}");
    assert!(has(&t, &pane::person_id(&t.rui)) && has(&t, &pane::name_field_id(unnamed)));
    check_fits(&t.d, "the People pane");
    shot(&t.d, "people-pane-window-900x600");
}

#[test]
fn naming_an_unnamed_face_from_the_pane_is_one_step_and_a_new_name_asks_first() {
    let mut t = setup();
    pane_on_three(&mut t);
    let unnamed = face(&t, 0, 1);
    let steps = |t: &T| t.d.app().session().unwrap().undo_stack().len();
    let before = steps(&t);
    t.d.click(&pane::name_field_id(unnamed)).unwrap();
    t.d.type_text("Zed").unwrap();
    t.d.key("Enter").unwrap();
    settle(&mut t);
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::NewPerson), "a name nobody has asks New Person");
    assert_eq!(steps(&t), before, "nothing named yet");
    t.d.click("button:dialog.cancel").unwrap();
    settle(&mut t);
    assert!(t.d.state().dialogs.is_empty());

    t.d.click(&pane::name_field_id(unnamed)).unwrap();
    t.d.type_text("rui").unwrap();
    t.d.key("Enter").unwrap();
    settle(&mut t);
    let named = |t: &T| {
        let lib = t.d.app().session().unwrap().library();
        lib.faces(t.photos[0])[1].person.clone()
    };
    assert_eq!(named(&t), dalica_library::FacePerson::Person(t.rui.clone()), "an existing name, any case, is that person");
    assert_eq!(steps(&t), before + 1, "one undo step");
    assert!(!has(&t, &unnamed.widget()), "it leaves Unnamed faces");
    let rows: Vec<String> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("person:")).map(|w| w.label).collect();
    assert_eq!(rows, ["Ana, 2 photos", "Rui, 3 photos"]);
    run(&mut t, "edit.undo", json!({}));
    assert_eq!(named(&t), dalica_library::FacePerson::Unnamed);
    assert!(has(&t, &unnamed.widget()));

    // × ignores it, one step.
    t.d.click(&format!("{}/ignore", unnamed.widget())).unwrap();
    settle(&mut t);
    assert_eq!(named(&t), dalica_library::FacePerson::Ignored);
    assert!(!has(&t, &unnamed.widget()));
}

#[test]
fn hovering_a_face_in_the_pane_highlights_its_box_and_back() {
    let mut t = setup();
    edit_with_pane(&mut t, 0);
    pictures(&mut t);
    let (named, unnamed) = (face(&t, 0, 0), face(&t, 0, 1));
    let lit = |t: &T, f: FaceRef| widget(&t.d, &boxes::box_id(f)).active;
    assert!(!lit(&t, named) && !lit(&t, unnamed));
    assert_eq!(widget(&t.d, &boxes::box_id(named)).label, "Ana", "the box carries its person's name");
    // Each box lies on its face: Ana's left of the unnamed one, both on the photo.
    let canvas = widget(&t.d, "canvas:edit").rect;
    let (a, b) = (widget(&t.d, &boxes::box_id(named)).rect, widget(&t.d, &boxes::box_id(unnamed)).rect);
    assert!(a[0] < b[0] && a[0] > canvas[0] && b[0] + b[2] < canvas[0] + canvas[2], "{a:?} {b:?} in {canvas:?}");

    t.d.hover(&unnamed.widget(), 50).unwrap();
    assert_eq!(t.d.state().people_pane.hovered, Some(unnamed));
    assert!(lit(&t, unnamed) && !lit(&t, named), "the pane's face lights its box");
    check_region(&t.d, "region:canvas", "edit-faces-1400x900");
    t.d.hover(&pane::person_id(&t.ana), 50).unwrap();
    assert!(lit(&t, named) && !lit(&t, unnamed), "a person's row lights their face on the photo");
    t.d.hover(&boxes::box_id(unnamed), 50).unwrap();
    assert_eq!(t.d.state().people_pane.hovered, Some(unnamed), "and back: the box names its face");
    t.d.hover("button:pane.close", 50).unwrap();
    assert_eq!(t.d.state().people_pane.hovered, None);
    shot(&t.d, "edit-faces-window-1400x900");
    small_shot(&mut t, 0, "edit-faces-window-900x600");
}

/// The window at 900 × 600 with photo `n` in the edit view and the People pane open (narrowing
/// closes it; opened on purpose it stays).
fn small_shot(t: &mut T, n: usize, name: &str) {
    t.d.resize(SMALL.0, SMALL.1);
    settle(t);
    edit_with_pane(t, n);
    pictures(t);
    shot(&t.d, name);
}

#[test]
fn a_box_drawn_by_hand_on_a_turned_photo_lands_where_it_was_drawn_after_a_restart() {
    let mut t = setup();
    let id = t.photos[3];
    run(&mut t, "photo.history.apply", json!({ "id": id, "op": "rotate(90)" }));
    edit_with_pane(&mut t, 3);
    assert!(!has(&t, boxes::MANUAL_ID));
    t.d.click(pane::ADD_MANUALLY_ID).unwrap();
    settle(&mut t);
    assert!(has(&t, boxes::MANUAL_ID) && has(&t, &format!("{}/se", boxes::MANUAL_ID)) && has(&t, boxes::NAME_ID));
    assert!(widget(&t.d, pane::ADD_MANUALLY_ID).active);
    let start = t.d.state().people_pane.manual.clone().unwrap().rect;
    // Fit the box: its bottom-right corner out, then the whole box up and left.
    t.d.drag(&format!("{}/se", boxes::MANUAL_ID), Target::Delta(vec2(60.0, 40.0)), &Drag::default()).unwrap();
    settle(&mut t);
    t.d.drag(boxes::MANUAL_ID, Target::Delta(vec2(-120.0, -50.0)), &Drag::default()).unwrap();
    settle(&mut t);
    let drawn = t.d.state().people_pane.manual.clone().unwrap().rect;
    assert!(drawn[2] - drawn[0] > start[2] - start[0] && drawn[0] < start[0] && drawn[1] < start[1], "{start:?} → {drawn:?}");
    let on_screen = widget(&t.d, boxes::MANUAL_ID).rect;
    shot(&t.d, "edit-faces-manual-window-1400x900");
    check_region(&t.d, "region:canvas", "edit-faces-manual-1400x900");

    let steps = t.d.app().session().unwrap().undo_stack().len();
    t.d.click(boxes::NAME_ID).unwrap();
    t.d.type_text("Eva").unwrap();
    t.d.key("Enter").unwrap();
    settle(&mut t);
    assert!(t.d.state().people_pane.manual.is_none(), "stored: the box goes");
    assert_eq!(t.d.app().session().unwrap().undo_stack().len(), steps + 1, "the face and the new person: one undo step");
    let eva = person(t.d.app().session().unwrap(), "Eva");

    // A restart.
    let T { dir, d, photos, ana, rui } = t;
    drop(d.into_app());
    let mut d = new_driver(App::new(Some(open(dir.path()))), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { dir, d, photos, ana, rui };
    edit_with_pane(&mut t, 3);
    let added = face(&t, 3, 0);
    let listed = run(&mut t, "faces.list", json!({ "ids": [id] }));
    let f = &listed["faces"][0];
    assert_eq!(f["person"], json!(eva));
    let close = |a: &Value, b: [f32; 4]| (0..4).all(|i| (a[i].as_f64().unwrap() - f64::from(b[i])).abs() < 2e-3);
    assert!(close(&f["displayed"], drawn), "as the photo shows it: {} vs {drawn:?}", f["displayed"]);
    assert!(!close(&f["rect"], drawn), "stored in the stored frame, turned back: {}", f["rect"]);
    let again = widget(&t.d, &boxes::box_id(added));
    assert_eq!(again.label, "Eva");
    assert!(again.rect.iter().zip(on_screen).all(|(a, b)| (a - b).abs() < 1.5), "{:?} where it was drawn, {on_screen:?}", again.rect);
    pictures(&mut t);
    shot(&t.d, "edit-faces-turned-window-1400x900");
    small_shot(&mut t, 3, "edit-faces-turned-window-900x600");
}

#[test]
fn two_people_selected_filter_the_light_table_and_show_all_clears_it() {
    let mut t = setup();
    // The People group folded: its person sections (M13.25) would push the folder's off screen.
    run(&mut t, "view.groupCollapse", json!({ "group": "people", "collapsed": true }));
    pane_on_three(&mut t);
    let thumb = |n: usize, t: &T| format!("thumb:{}", t.photos[n]);
    for n in 0..4 {
        assert!(has(&t, &thumb(n, &t)), "{n}");
    }
    t.d.click(&pane::person_id(&t.ana)).unwrap();
    settle(&mut t);
    t.d.click_with(&pane::person_id(&t.rui), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    settle(&mut t);
    assert_eq!(t.d.state().people_pane.selected, [t.ana.clone(), t.rui.clone()]);
    assert!(widget(&t.d, &pane::person_id(&t.rui)).active, "selected rows show it");
    assert_eq!(widget(&t.d, "banner:search").label, "Showing photos with Ana and Rui");
    let shown: Vec<bool> = (0..4).map(|n| has(&t, &thumb(n, &t))).collect();
    assert_eq!(shown, [false, true, false, false], "only the photo holding both");
    // Ctrl-click again drops Rui: Ana's photos.
    t.d.click_with(&pane::person_id(&t.rui), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    settle(&mut t);
    assert_eq!(widget(&t.d, "banner:search").label, "Showing photos with Ana");
    let shown: Vec<bool> = (0..4).map(|n| has(&t, &thumb(n, &t))).collect();
    assert_eq!(shown, [true, true, false, false]);

    t.d.click("button:search.showAll").unwrap();
    settle(&mut t);
    assert!(t.d.state().people_pane.selected.is_empty() && t.d.state().search.text.is_empty());
    assert!(!has(&t, "banner:search"));
    assert!((0..4).all(|n| has(&t, &thumb(n, &t))), "every photo again");
}
