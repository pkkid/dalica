//! The collage canvas (M14.19, `creating.md` §3), driven headlessly by automation id on collages
//! of the demo photos with a fixed seed: a pile's picture dragged moves and its wheel handle turns
//! it a quarter, each one step of the room's history; a grid's picture dragged onto another swaps
//! them; the picture menu's Bring to Top puts its picture last; Delete takes two pictures back to
//! the Clips; Set as Frame Center moves a Frame Mosaic's centre; every picture's centre on the
//! canvas is its placement's; Ctrl+Z undoes; the canvas never touches the disk on the UI thread;
//! fingerprints of a pile with a selection and of a frame mosaic in the large window.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_region, demo_library, new_driver, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::headless::{Click, Drag, Target};
use dalica_ui::panels::collage::canvas::{self, geometry};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};

/// A library of `n` demo photos in `dir` and their ids.
fn library(dir: &Path, n: usize) -> (Session, Vec<PhotoId>) {
    let files = demo_library(1, n, 7).unwrap();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &f.bytes).unwrap();
    }
    let folder = dir.join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let ids = found["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect();
    (s, ids)
}

/// The Collage room in front on a collage of `n` demo photos in `style`, every thumbnail drawn.
fn driver(dir: &Path, n: usize, style: &str) -> Driver {
    let (s, ids) = library(dir, n);
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.run("create.collage", &json!({ "ids": ids, "seed": "c011a6e" })).unwrap();
    d.run("collage.set", &json!({ "style": style })).unwrap();
    d.run("collage.open", &json!({})).unwrap();
    loaded(&mut d);
    d
}

/// Settles, then steps until no picture waits for its thumbnail.
fn loaded(d: &mut Driver) {
    d.settle(FRAMES).unwrap();
    for _ in 0..FRAMES {
        if canvas::waiting(d.ctx()) == 0 {
            break;
        }
        d.step();
    }
    assert_eq!(canvas::waiting(d.ctx()), 0, "every picture has its thumbnail");
}

fn inspect(d: &mut Driver) -> Value {
    d.app_mut().session_mut().unwrap().execute("collage.inspect", &json!({})).unwrap()
}

fn placements(d: &mut Driver) -> Vec<Value> {
    inspect(d)["placements"].as_array().unwrap().clone()
}

/// The photos on the page by picture number.
fn photos(d: &mut Driver) -> Vec<u64> {
    placements(d).iter().map(|p| p["id"].as_u64().unwrap()).collect()
}

fn undo_depth(d: &mut Driver) -> u64 {
    inspect(d)["undo"].as_u64().unwrap()
}

fn rect_of(d: &Driver, id: &str) -> Rect {
    let r = widget(d, id).rect;
    Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]))
}

/// Placement `p`'s centre on the screen.
fn centre_of(page: Rect, p: &Value) -> Pos2 {
    let c = p["centre"].as_array().unwrap();
    pos2(page.min.x + c[0].as_f64().unwrap() as f32 * page.width(), page.min.y + c[1].as_f64().unwrap() as f32 * page.height())
}

/// The lowest picture whose centre no other picture covers (a click there reaches it).
fn uncovered(d: &mut Driver) -> u32 {
    let page = rect_of(d, canvas::CANVAS_ID);
    let s = d.app_mut().session_mut().unwrap();
    let doc = s.collage().document().unwrap().clone();
    let cards: Vec<geometry::Card> = s.collage_placements().iter().enumerate().filter_map(|(i, p)| geometry::card(&doc, page, i as u32, p)).collect();
    cards.iter().find(|c| geometry::hit(&cards, c.centre()).map(|h| h.item) == Some(c.item)).unwrap().item
}

fn right_click(d: &mut Driver, id: &str) {
    d.click_with(id, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
}

#[test]
fn a_pile_picture_moves_and_turns_a_quarter_one_step_each() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 5, "pile");
    let page = rect_of(&d, canvas::CANVAS_ID);
    // Each picture's centre on the canvas is its placement's, within a pixel.
    for (n, p) in placements(&mut d).iter().enumerate() {
        let shown = rect_of(&d, &canvas::item_id(n as u32)).center();
        assert!((shown - centre_of(page, p)).length() <= 1.0, "item {n}: {shown:?} and {:?}", centre_of(page, p));
    }

    // The top picture dragged 80 left and 30 down: one step, its centre moved with the pointer.
    let top = placements(&mut d).len() as u32 - 1;
    let (before, steps) = (placements(&mut d)[top as usize].clone(), undo_depth(&mut d));
    d.drag(&canvas::item_id(top), Target::Delta(vec2(-80.0, 30.0)), &Drag::default()).unwrap();
    let after = placements(&mut d)[top as usize].clone();
    assert_eq!(after["id"], before["id"]);
    assert!((centre_of(page, &after) - centre_of(page, &before) - vec2(-80.0, 30.0)).length() <= 1.0, "{before} → {after}");
    assert_eq!((undo_depth(&mut d), d.state().collage.selection.clone(), d.state().collage.gesture.clone()), (steps + 1, vec![top], None));

    // Its wheel handle taken a quarter turn clockwise round its centre: one step, 90° more.
    d.step();
    let centre = rect_of(&d, &canvas::item_id(top)).center();
    let wheel = rect_of(&d, canvas::HANDLE_ID).center();
    let arm = wheel - centre;
    d.drag(canvas::HANDLE_ID, Target::Point(centre + vec2(-arm.y, arm.x)), &Drag::default()).unwrap();
    let turned = placements(&mut d)[top as usize].clone();
    let (a0, a1) = (after["angle"].as_f64().unwrap(), turned["angle"].as_f64().unwrap());
    let swept = (a1 - a0 + 540.0).rem_euclid(360.0) - 180.0;
    assert!((swept - 90.0).abs() < 0.5, "{a0} → {a1}");
    let size = |p: &Value| p["size"][0].as_f64().unwrap();
    assert!((size(&turned) / size(&after) - 1.0).abs() < 0.01, "the same distance keeps the size");
    assert_eq!(undo_depth(&mut d), steps + 2);

    // Ctrl+Z undoes the turn.
    d.key("Ctrl+Z").unwrap();
    assert_eq!(placements(&mut d)[top as usize]["angle"], after["angle"]);
    assert_eq!(undo_depth(&mut d), steps + 1);

    // The pile with its selection.
    loaded(&mut d);
    check_region(&d, canvas::CANVAS_ID, "collage-canvas-pile-1400x900");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    check_fits(&d, "the collage canvas");
    crate::common::shot(&d, "collage-canvas-pile-900x600");
}

#[test]
fn bring_to_top_from_the_picture_menu_puts_it_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 5, "pile");
    let n = uncovered(&mut d);
    let photo = photos(&mut d)[n as usize];
    assert_ne!(photos(&mut d).last(), Some(&photo), "not on top yet");
    right_click(&mut d, &canvas::item_id(n));
    assert_eq!(d.state().collage.selection, [n], "the picture right-clicked is selected");
    d.click("menuitem:collageCanvas.order/top").unwrap();
    assert_eq!(photos(&mut d).last(), Some(&photo));
    let last = photos(&mut d).len() as u32 - 1;
    assert_eq!(d.state().collage.selection, [last], "the selection goes with it");
}

#[test]
fn a_grid_picture_dragged_onto_another_swaps_them_and_delete_takes_two_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 6, "grid");
    let page = rect_of(&d, canvas::CANVAS_ID);
    let before = placements(&mut d);
    let steps = undo_depth(&mut d);
    d.drag(&canvas::item_id(0), Target::Widget(&canvas::item_id(1)), &Drag::default()).unwrap();
    let after = placements(&mut d);
    let at = |list: &[Value], id: &Value| centre_of(page, list.iter().find(|p| p["id"] == *id).unwrap());
    assert!((at(&after, &before[0]["id"]) - centre_of(page, &before[1])).length() <= 1.0, "the first is where the second was");
    assert!((at(&after, &before[1]["id"]) - centre_of(page, &before[0])).length() <= 1.0, "and the second where the first was");
    assert_eq!(undo_depth(&mut d), steps + 1);

    // Two pictures selected (a click, then a Ctrl-click), and Delete takes them to the Clips.
    d.click(&canvas::item_id(2)).unwrap();
    d.click_with(&canvas::item_id(4), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    assert_eq!(d.state().collage.selection, [2, 4]);
    let gone = [photos(&mut d)[2], photos(&mut d)[4]];
    d.key("Delete").unwrap();
    let left = photos(&mut d);
    assert_eq!(left.len(), 4);
    assert!(gone.iter().all(|id| !left.contains(id)));
    let clips = inspect(&mut d)["clips"].as_array().unwrap().clone();
    assert_eq!(clips.len(), 6, "they stay in the Clips");
    assert_eq!(clips.iter().filter(|c| c["onPage"] == json!(false)).count(), 2);
    assert!(d.state().collage.selection.is_empty());
    // Ctrl+Z puts them back.
    d.key("Ctrl+Z").unwrap();
    assert_eq!(photos(&mut d).len(), 6);
}

#[test]
fn set_as_frame_center_moves_the_centre() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 7, "frameMosaic");
    assert!(!widget(&d, "button:collageCanvas.setFrameCenter").enabled, "nothing selected");
    let area = |p: &Value| p["size"][0].as_f64().unwrap() * p["size"][1].as_f64().unwrap();
    let largest = |list: &[Value]| list.iter().max_by(|a, b| area(a).total_cmp(&area(b))).unwrap()["id"].clone();
    let list = placements(&mut d);
    let centre = largest(&list);
    let (n, other) = list.iter().enumerate().find(|(_, p)| p["id"] != centre).map(|(n, p)| (n as u32, p["id"].clone())).unwrap();
    d.click(&canvas::item_id(n)).unwrap();
    d.click("button:collageCanvas.setFrameCenter").unwrap();
    assert_eq!(inspect(&mut d)["document"]["frameCentre"], other);
    assert_eq!(largest(&placements(&mut d)), other, "the picture is the frame's centre");
    loaded(&mut d);
    check_region(&d, canvas::CANVAS_ID, "collage-canvas-frame-mosaic-1400x900");
}

/// The canvas with 30 pictures: drawn and dragged with the UI thread marked, nothing on it reads a
/// file; the frame times go to the log (`ui.inspect` → perf).
#[test]
fn the_canvas_never_waits_on_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 30, "pile");
    let ui_thread = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    d.perf_mut().reset();
    let top = photos(&mut d).len() as u32 - 1;
    d.drag(&canvas::item_id(top), Target::Delta(vec2(-120.0, 30.0)), &Drag { steps: 60, ..Drag::default() }).unwrap();
    d.run("collageCanvas.scramble", &json!({})).unwrap();
    loaded(&mut d);
    let perf = d.perf().report();
    println!(
        "30 pictures: {} frames, p95 {:.2} ms, max {:.2} ms, {} textures",
        perf.frames,
        perf.frame_p95_ms,
        perf.max_frame_ms,
        canvas::textures(d.ctx())
    );
    assert_eq!(dalica_fs::guard::refused(), refused, "file access was attempted on the UI thread");
    drop(ui_thread);
}
