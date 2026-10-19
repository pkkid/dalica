//! The Places pane (M13.22, `organizing.md` §7), driven headlessly by widget id on a folder of
//! eight scenes photos, with no network (the map draws its offline layer only):
//!
//! - three photos dragged from the light table onto the map and OK geotag them where they were let
//!   go, as one undo step, and the map shows one marker of 3; dropping photos that have a location
//!   asks "Move %d photos here?";
//! - Remove Location in a marker's bubble clears its photos, asking first past 5; Show These Photos
//!   selects them; a marker drawn as a group of places speaks and acts for all their photos
//!   (M14.29), and one zoom level in its places part again;
//! - typed coordinates in "Search for a place:" move the pin and the map there, and the pin's OK
//!   puts the selection there;
//! - Tools ▸ Geotag ▸ Copy Location and Paste Location work from the menu, and Clear Location…
//!   asks;
//! - a photo tagged in Dalica shows the geotag badge in the light table;
//! - the pane matches its fingerprints with markers and with a bubble at 1400 × 900
//!   (`tests/fingerprints/places-{markers,bubble}-1400x900.json`; `DALICA_BLESS=1` stores new
//!   ones, renders go to `target/test-output/ui/`) and fits at 900 × 600, and every widget shown has
//!   a valid, unique id.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_small, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_core::geo::LatLon;
use dalica_engine::Session;
use dalica_ui::headless::{Drag, Target};
use dalica_ui::state::{Bubble, DialogKind, Pane};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<PhotoId>,
}

/// Lisbon and Porto.
const LISBON: [f64; 2] = [38.7116, -9.1389];
const PORTO: [f64; 2] = [41.1579, -8.6291];
/// A point 0.44° east of Lisbon: about 20 points from it at zoom 6 (one marker of the two), 40 at
/// zoom 7 (two markers).
const EAST: [f64; 2] = [38.7116, -8.70];

/// A library of eight photos in one folder at `w × h`, the Places pane open on the map of Portugal.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 8, 22).unwrap();
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
    let mut ids: Vec<PhotoId> = found["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 8);
    let mut t = T { _dir: dir, d: new_driver(App::new(Some(s)), w, h, 1.0), ids };
    t.d.settle(FRAMES).unwrap();
    run(&mut t, "pane.places", json!({}));
    run(&mut t, "map.pan", json!({ "centre": [39.6, -8.0], "zoom": 6 }));
    assert_eq!(t.d.state().pane, Some(Pane::Places));
    t
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

fn has(t: &T, id: &str) -> bool {
    try_widget(&t.d, id).is_some()
}

fn select(t: &mut T, ids: &[PhotoId]) {
    run(t, "select.set", json!({ "ids": ids }));
}

fn geotag(t: &T, id: PhotoId) -> Option<LatLon> {
    t.d.app().session().unwrap().library().geotag(id)
}

fn located(t: &T, id: PhotoId) -> bool {
    t.d.app().session().unwrap().library().is_geotagged(id)
}

/// Gives `ids` a point, as an agent would.
fn put(t: &mut T, ids: &[PhotoId], at: [f64; 2]) {
    run(t, "photo.geo.set", json!({ "ids": ids, "geo": at, "confirm": true }));
}

/// The labels of the markers shown (`marker:<n>` and its count), in order.
fn markers(t: &T) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("marker:")).map(|w| (w.id, w.label)).collect();
    v.sort();
    v
}

/// Drags the selected photo `from` from the light table onto the middle of the map.
fn drop_on_map(t: &mut T, from: PhotoId) {
    let map = widget(&t.d, "map:places").center();
    t.d.drag(&format!("thumb:{from}"), Target::Point(map), &Drag::default()).unwrap();
    t.d.settle(FRAMES).unwrap();
}

#[test]
fn three_photos_dropped_on_the_map_and_ok_are_geotagged_as_one_step() {
    let mut t = setup(LARGE);
    let three = t.ids[..3].to_vec();
    select(&mut t, &three);
    assert_eq!(widget(&t.d, "text:places.status").label, "No places in the selection");
    // Where the drop lands: the point under the map's middle.
    let centre = t.d.state().places.centre;
    drop_on_map(&mut t, three[1]);
    assert_eq!(t.d.state().places.bubble, Some(Bubble::Drop));
    assert_eq!(widget(&t.d, "map:places/bubble").label, "Put 3 photos here?");
    assert!(has(&t, "map:places/pin"), "the drop's point shows while the bubble asks");
    click(&mut t, "button:places.put");
    let at = geotag(&t, three[0]).expect("geotagged");
    assert!((at.lat() - centre[0]).abs() < 0.05 && (at.lon() - centre[1]).abs() < 0.05, "{at:?} near {centre:?}");
    assert!(three.iter().all(|id| geotag(&t, *id) == Some(at)), "the three at one point");
    assert!(!located(&t, t.ids[3]));
    assert_eq!(t.d.state().places.bubble, None);
    assert_eq!(markers(&t), [("marker:0".to_string(), "3".to_string())]);
    assert_eq!(widget(&t.d, "text:places.status").label, "1 place in the selection");
    // One undo step.
    run(&mut t, "edit.undo", json!({}));
    assert!(three.iter().all(|id| !located(&t, *id)));
    assert!(markers(&t).is_empty());
}

#[test]
fn dropping_photos_that_have_a_location_asks_move() {
    let mut t = setup(LARGE);
    let two = t.ids[..2].to_vec();
    put(&mut t, &two[..1], PORTO);
    select(&mut t, &two);
    drop_on_map(&mut t, two[0]);
    assert_eq!(widget(&t.d, "map:places/bubble").label, "Move 2 photos here?");
    // Cancel changes nothing.
    click(&mut t, "button:places.cancel");
    assert_eq!((t.d.state().places.bubble, geotag(&t, two[0])), (None, Some(LatLon::new(PORTO[0], PORTO[1]).unwrap())));
    assert!(!located(&t, two[1]));
}

#[test]
fn remove_location_asks_past_five_and_show_these_photos_selects_them() {
    let mut t = setup(LARGE);
    let (six, porto) = (t.ids[..6].to_vec(), t.ids[6]);
    put(&mut t, &six, LISBON);
    put(&mut t, &[porto], PORTO);
    let all = t.ids.clone();
    select(&mut t, &all);
    assert_eq!(widget(&t.d, "text:places.status").label, "2 places in the selection");
    assert_eq!(markers(&t), [("marker:0".to_string(), "6".to_string()), ("marker:1".to_string(), "1".to_string())]);
    // Show These Photos.
    click(&mut t, "marker:1");
    assert_eq!(widget(&t.d, "map:places/bubble").label, "1 photo here");
    click(&mut t, "button:places.showPhotos");
    assert_eq!(t.d.state().selection.ids, vec![porto]);
    // Remove Location on six photos asks first; Cancel keeps them.
    select(&mut t, &all);
    click(&mut t, "marker:0");
    assert_eq!(widget(&t.d, "map:places/bubble").label, "6 photos here");
    click(&mut t, "button:places.removeLocation");
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::ClearLocation));
    assert_eq!(widget(&t.d, "dialog:clearLocation").label, "Clear Location");
    click(&mut t, "button:dialog.cancel");
    assert!(six.iter().all(|id| located(&t, *id)));
    click(&mut t, "marker:0");
    click(&mut t, "button:places.removeLocation");
    click(&mut t, "button:dialog.remove");
    assert!(six.iter().all(|id| !located(&t, *id)), "removed once confirmed");
    assert_eq!(markers(&t), [("marker:0".to_string(), "1".to_string())]);
    // One photo goes without a question.
    click(&mut t, "marker:0");
    click(&mut t, "button:places.removeLocation");
    assert!(t.d.state().dialogs.is_empty() && !located(&t, porto));
    assert_eq!(widget(&t.d, "text:places.status").label, "No places in the selection");
}

#[test]
fn a_grouped_markers_bubble_counts_shows_and_removes_every_photo_it_counts() {
    let mut t = setup(LARGE);
    let (a, b) = (t.ids[0], t.ids[1]);
    put(&mut t, &[a], LISBON);
    put(&mut t, &[b], EAST);
    let all = t.ids.clone();
    select(&mut t, &all);
    assert_eq!(widget(&t.d, "text:places.status").label, "2 places in the selection");
    // At zoom 6 the two places are one marker of 2; its bubble agrees.
    assert_eq!(markers(&t), [("marker:0".to_string(), "2".to_string())]);
    run(&mut t, "places.bubble", json!({ "marker": 0 }));
    assert_eq!(widget(&t.d, "map:places/bubble").label, "2 photos here");
    shot(&t.d, "places-grouped-bubble-1400x900");
    click(&mut t, "button:places.showPhotos");
    assert_eq!(t.d.state().selection.ids, vec![a, b], "Show These Photos selects both");
    assert_eq!(t.d.state().places.bubble, None);
    // One zoom level in, the two single markers each say 1.
    select(&mut t, &all);
    run(&mut t, "map.pan", json!({ "centre": [38.7, -8.9], "zoom": 7 }));
    assert_eq!(markers(&t), [("marker:0".to_string(), "1".to_string()), ("marker:1".to_string(), "1".to_string())]);
    for m in ["marker:0", "marker:1"] {
        click(&mut t, m);
        assert_eq!(widget(&t.d, "map:places/bubble").label, "1 photo here", "{m}");
    }
    // Back out: Remove Location clears both, without a question (2 is not past 5).
    run(&mut t, "places.bubble", json!({}));
    run(&mut t, "map.pan", json!({ "centre": [38.7, -8.9], "zoom": 6 }));
    run(&mut t, "places.bubble", json!({ "marker": 0 }));
    assert_eq!(widget(&t.d, "map:places/bubble").label, "2 photos here");
    click(&mut t, "button:places.removeLocation");
    assert!(t.d.state().dialogs.is_empty() && !located(&t, a) && !located(&t, b));
    assert!(markers(&t).is_empty());
    // One undo step.
    run(&mut t, "edit.undo", json!({}));
    assert!(located(&t, a) && located(&t, b));
}

#[test]
fn typed_coordinates_move_the_pin_and_its_ok_puts_the_selection_there() {
    let mut t = setup(LARGE);
    let two = t.ids[2..4].to_vec();
    select(&mut t, &two);
    click(&mut t, "field:places.search");
    t.d.type_text("41.1579, -8.6291").unwrap();
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().places.pin.as_ref().map(|p| p.at), Some(PORTO));
    assert_eq!(t.d.state().places.centre, PORTO, "the map moved there");
    // The pin is at the map's middle, its bubble open.
    let (pin, map) = (widget(&t.d, "map:places/pin").rect, widget(&t.d, "map:places").center());
    assert!((pin[0] + pin[2] / 2.0 - map.x).abs() < 2.0 && pin[1] < map.y && pin[1] + pin[3] >= map.y - 2.0, "{pin:?} at {map:?}");
    assert_eq!(widget(&t.d, "map:places/bubble").label, "Put 2 photos here?");
    click(&mut t, "button:places.put");
    assert!(two.iter().all(|id| geotag(&t, *id) == Some(LatLon::new(PORTO[0], PORTO[1]).unwrap())));
    assert!(t.d.state().places.pin.is_none() && !has(&t, "map:places/pin"), "the pin became a marker");
    assert_eq!(markers(&t).len(), 1);
    // A name is no coordinates: it waits for the place search, and the pin stays away.
    click(&mut t, "field:places.search");
    t.d.key("Ctrl+A").unwrap();
    t.d.type_text("Lisbon").unwrap();
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().places.pin.is_none());
    // The pin tool: armed, a click on the map places the pin.
    click(&mut t, "button:map.pin");
    assert!(widget(&t.d, "button:map.pin").active);
    let map = widget(&t.d, "map:places");
    t.d.click_with("map:places", &dalica_ui::headless::Click { fx: 0.5, fy: 0.75, ..Default::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(!widget(&t.d, "button:map.pin").active, "the tool disarms once the pin is placed");
    let pin = t.d.state().places.pin.as_ref().expect("placed").at;
    assert!(pin[0] < t.d.state().places.centre[0] && map.rect[3] > 0.0, "below the middle: south of it");
}

#[test]
fn copy_and_paste_location_work_from_the_menu_and_clear_location_asks() {
    let mut t = setup(LARGE);
    let (a, b, c) = (t.ids[0], t.ids[1], t.ids[2]);
    put(&mut t, &[a], LISBON);
    let menu = |t: &mut T, item: &str| {
        click(t, "menu:Tools");
        t.d.hover("submenu:Tools/Geotag", 300).unwrap();
        t.d.settle(FRAMES).unwrap();
        click(t, item);
    };
    select(&mut t, &[a]);
    menu(&mut t, "menuitem:geo.copy");
    select(&mut t, &[b, c]);
    menu(&mut t, "menuitem:geo.paste");
    assert!([b, c].iter().all(|id| geotag(&t, *id) == geotag(&t, a)) && geotag(&t, a).is_some());
    menu(&mut t, "menuitem:geo.clear");
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::ClearLocation), "Clear Location… asks");
    click(&mut t, "button:dialog.remove");
    assert!(!located(&t, b) && !located(&t, c) && located(&t, a));
}

#[test]
fn a_photo_tagged_in_dalica_shows_the_geotag_badge() {
    let mut t = setup(LARGE);
    let id = t.ids[4];
    let badge = |t: &T| t.d.app().panels().unwrap().light_table.cells.get(id).unwrap().badges.geotagged;
    assert!(!badge(&t));
    put(&mut t, &[id], LISBON);
    assert!(badge(&t), "the badge reads the library's geotag");
    run(&mut t, "geo.clear", json!({ "ids": [id] }));
    assert!(!badge(&t));
}

/// The pane and every widget fit the small window (narrowing it closes the pane, `ui-layout.md`
/// §7, so it is opened again there), then the window is large again.
fn fits_small(t: &mut T) {
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    if t.d.state().pane.is_none() {
        run(t, "pane.places", json!({}));
    }
    check_small(&mut t.d, "pane:places");
    shot(&t.d, &format!("places-pane-{}x{}", SMALL.0, SMALL.1));
    t.d.resize(LARGE.0, LARGE.1);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().pane, Some(Pane::Places));
}

#[test]
fn the_pane_matches_its_fingerprints_and_fits_the_small_window() {
    let mut t = setup(LARGE);
    let ids = t.ids.clone();
    put(&mut t, &ids[..3], LISBON);
    put(&mut t, &ids[3..5], PORTO);
    put(&mut t, &ids[5..6], [37.0194, -7.9304]);
    select(&mut t, &ids);
    assert!(t.d.app().panels().unwrap().unregistered_widgets().is_empty());
    check_region(&t.d, "pane:places", "places-markers-1400x900");
    fits_small(&mut t);
    click(&mut t, "marker:0");
    assert_eq!(widget(&t.d, "map:places/bubble").label, "3 photos here");
    check_region(&t.d, "pane:places", "places-bubble-1400x900");
    fits_small(&mut t);
    shot(&t.d, "places-pane-1400x900");
    // A drop's bubble and the Geotag menu, for the record.
    run(&mut t, "places.drop", json!({ "ids": &ids[6..], "at": [40.2, -8.4] }));
    shot(&t.d, "places-drop-1400x900");
    click(&mut t, "menu:Tools");
    t.d.hover("submenu:Tools/Geotag", 300).unwrap();
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "places-geotag-menu-1400x900");
    t.d.resize(900, 600);
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "places-geotag-menu-900x600");
}
