//! The Collage room (M14.18, `creating.md` §3, `ui-layout.md` §8), driven headlessly by
//! automation id on a library of eight scenes photos: the tray's six photos open the room with six
//! clips on the page; each style greys the options it lacks; the eyedropper on the canvas sets the
//! background colour; portrait turns the page; Get More then Back to Collage adds the selection;
//! Create Collage opens the new photo in the edit view and Close with changes asks, Save Draft
//! writing a draft; at 900 × 600 the room fits; the left panel's two tabs are fingerprinted at
//! 1400 × 900 (the canvas is M14.19's, fingerprinted there).

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::commands::collage;
use dalica_ui::headless::Click;
use dalica_ui::panels::collage::{PANEL_W, settings};
use dalica_ui::state::{Room, View};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A library in `root` holding one folder of eight scenes photos, added and scanned; its
/// pictures folder (where collages go) inside `root`.
fn session(root: &Path) -> (Session, Vec<PhotoId>) {
    let files = crate::common::demo_library(1, 8, 21).unwrap();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = root.join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(root.join("Pictures")));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 100 })).unwrap();
    let ids = found["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect();
    (s, ids)
}

/// The window at `size` on that library, settled; and the photos.
fn driver(root: &Path, (w, h): (u32, u32)) -> (Driver, Vec<PhotoId>) {
    let (s, ids) = session(root);
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    (d, ids)
}

fn run(d: &mut Driver, id: &str, params: Value) -> Value {
    d.run(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    d.settle(FRAMES).unwrap();
}

fn inspect(d: &mut Driver) -> Value {
    d.app_mut().session_mut().unwrap().execute("collage.inspect", &json!({})).unwrap()
}

/// The tray's first six photos open the room through the bottom bar's Collage.
fn open_with_six(d: &mut Driver, ids: &[PhotoId]) {
    run(d, "select.set", json!({ "ids": &ids[..6] }));
    d.settle(FRAMES).unwrap();
    click(d, "action:collage");
}

/// The left panel (its tabs and their page) as `[x, y, w, h]`.
fn panel(d: &Driver) -> [f32; 4] {
    let r = widget(d, "region:collage").rect;
    [r[0], r[1], PANEL_W.min(r[2]), r[3]]
}

#[test]
fn the_trays_photos_open_the_room_with_their_clips_and_each_style_greys_what_it_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = driver(dir.path(), LARGE);
    open_with_six(&mut d, &ids);
    assert_eq!((d.state().room, d.state().rooms.as_slice()), (Room::Collage, &[Room::Library, Room::Collage][..]));
    let c = inspect(&mut d);
    assert_eq!(c["placements"].as_array().unwrap().len(), 6, "six pictures on the page");
    assert_eq!(widget(&d, "tab:collage/clips").label, "Clips (6)");
    click(&mut d, "tab:collage/clips");
    for id in &ids[..6] {
        assert!(try_widget(&d, &format!("clip:{id}")).is_some(), "clip:{id}");
    }
    assert!(try_widget(&d, &format!("clip:{}", ids[6])).is_none());
    click(&mut d, "tab:collage/settings");

    // Each style greys the options it lacks (`creating.md` §3).
    let enabled = |d: &Driver, id: &str| widget(d, id).enabled;
    for (style, borders, spacing, captions, background) in [
        ("pile", true, false, true, true),
        ("mosaic", false, true, false, true),
        ("frameMosaic", false, true, false, true),
        ("grid", false, true, false, true),
        ("contactSheet", true, false, false, true),
        ("multipleExposure", false, false, false, false),
    ] {
        click(&mut d, settings::STYLE_ID);
        click(&mut d, &format!("{}/{style}", settings::STYLE_ID));
        assert_eq!(inspect(&mut d)["document"]["style"], style);
        assert_eq!(widget(&d, settings::STYLE_ID).label.split(':').next(), Some(dalica_create::collage::Style::from_id(style).unwrap().label()));
        assert_eq!(settings::BORDER_IDS.map(|id| enabled(&d, id)), [borders; 3], "{style}: borders");
        assert_eq!(enabled(&d, settings::SPACING_ID), spacing, "{style}: spacing");
        assert_eq!(enabled(&d, settings::CAPTIONS_ID), captions, "{style}: captions");
        assert_eq!((enabled(&d, settings::SOLID_ID), enabled(&d, settings::PICTURE_ID)), (background, background), "{style}: background");
        assert!(enabled(&d, settings::SHADOWS_ID) && enabled(&d, settings::FORMAT_ID), "{style}: always offered");
    }
    // A border toggle and the spacing slider reach the collage.
    click(&mut d, settings::STYLE_ID);
    click(&mut d, &format!("{}/pile", settings::STYLE_ID));
    click(&mut d, settings::BORDER_IDS[2]);
    assert_eq!(inspect(&mut d)["document"]["settings"]["border"], "instant");
    assert!(widget(&d, settings::BORDER_IDS[2]).active);
}

#[test]
fn the_eyedropper_sets_the_background_and_portrait_turns_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = driver(dir.path(), LARGE);
    open_with_six(&mut d, &ids);
    assert_eq!(inspect(&mut d)["document"]["background"]["colour"], "#000000");
    // The swatch's popup, its eyedropper, then a click on the page at the top picture's centre.
    click(&mut d, settings::SWATCH_ID);
    click(&mut d, &format!("{}/eyedropper", settings::SWATCH_ID));
    assert!(settings::eyedropper_armed(d.ctx()));
    assert!(try_widget(&d, dalica_ui::panels::collage::EYEDROPPER_ID).is_some(), "the canvas takes the pick");
    let c = inspect(&mut d);
    let top = c["placements"].as_array().unwrap().iter().max_by_key(|p| p["z"].as_u64().unwrap()).unwrap().clone();
    let at = [top["centre"][0].as_f64().unwrap() as f32, top["centre"][1].as_f64().unwrap() as f32];
    let want = settings::colour_at(d.app_mut().session_mut().unwrap(), [f64::from(at[0]), f64::from(at[1])]).expect("a picture there");
    d.click_with(dalica_ui::panels::collage::EYEDROPPER_ID, &Click { fx: at[0], fy: at[1], ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!settings::eyedropper_armed(d.ctx()), "one pick, then disarmed");
    let colour = inspect(&mut d)["document"]["background"]["colour"].as_str().unwrap().to_string();
    assert_eq!(colour, dalica_core::color::hex_u8(want));
    assert_ne!(colour, "#000000", "the picture's colour, not the old black");

    // Portrait turns the page: taller than wide.
    let page = inspect(&mut d)["page"].clone();
    assert!(page["width"].as_u64() > page["height"].as_u64(), "landscape first: {page}");
    click(&mut d, settings::ORIENTATION_IDS[1]);
    let page = inspect(&mut d)["page"].clone();
    assert!(page["width"].as_u64() < page["height"].as_u64(), "portrait: {page}");
    assert!(widget(&d, settings::ORIENTATION_IDS[1]).active);
}

#[test]
fn get_more_then_back_to_collage_adds_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = driver(dir.path(), LARGE);
    open_with_six(&mut d, &ids);
    click(&mut d, "tab:collage/clips");
    click(&mut d, "button:collage.getMore");
    assert_eq!((d.state().room, d.state().collage.get_more), (Room::Library, true));
    run(&mut d, "select.set", json!({ "ids": [ids[6], ids[7]] }));
    d.settle(FRAMES).unwrap();
    click(&mut d, "button:tray.backToCollage");
    assert_eq!((d.state().room, d.state().collage.get_more), (Room::Collage, false));
    assert_eq!(inspect(&mut d)["clips"].as_array().unwrap().len(), 8);
    assert_eq!(widget(&d, "tab:collage/clips").label, "Clips (8)");
    assert!(try_widget(&d, "button:tray.backToCollage").is_none());
    // − takes a selected clip out; + puts a clip taken off the page back on it.
    click(&mut d, &format!("clip:{}", ids[7]));
    assert_eq!(d.state().collage.clips, [ids[7]]);
    click(&mut d, "button:collage.removeClips");
    assert_eq!(inspect(&mut d)["clips"].as_array().unwrap().len(), 7);
    run(&mut d, "collage.unplace", json!({ "ids": [ids[6]] }));
    click(&mut d, &format!("clip:{}", ids[6]));
    click(&mut d, "button:collage.addClips");
    let on: Vec<bool> = inspect(&mut d)["clips"].as_array().unwrap().iter().map(|c| c["onPage"].as_bool().unwrap()).collect();
    assert!(on.iter().all(|o| *o), "{on:?}");
}

#[test]
fn create_collage_opens_the_new_photo_and_close_with_changes_asks() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = driver(dir.path(), LARGE);
    open_with_six(&mut d, &ids);
    // Create Collage: its job's progress and Cancel while it renders, then the new photo in the
    // edit view, the Library tab in front.
    assert!(!collage::unsaved(d.app()));
    click(&mut d, settings::CREATE_ID);
    if d.app().collage().job().is_some() {
        assert!(try_widget(&d, settings::PROGRESS_ID).is_some() && try_widget(&d, settings::CANCEL_ID).is_some(), "progress and Cancel");
        assert!(try_widget(&d, settings::CREATE_ID).is_none());
    }
    until(&mut d, |a| a.collage().job().is_none() && a.state().view == View::Edit);
    let photo = d.state().edit.photo.expect("the new collage in the edit view");
    assert!(!ids.contains(&photo), "a new photo");
    assert_eq!(d.state().room, Room::Library);
    run(&mut d, "room.select", json!({ "room": "collage" }));
    d.settle(FRAMES).unwrap();
    // A change, then Close asks Save Draft, Discard or Cancel; Cancel keeps the room.
    click(&mut d, settings::ORIENTATION_IDS[1]);
    assert!(collage::unsaved(d.app()));
    click(&mut d, settings::CLOSE_ID);
    assert_eq!(d.state().dialogs.last().map(|x| x.kind.name()), Some("collageClose"));
    assert_eq!(d.state().collage.close, Some(dalica_ui::state::CollageClose::Tab));
    click(&mut d, "button:dialog.cancel");
    assert!(d.state().rooms.contains(&Room::Collage), "Cancel keeps the room");
    // Save Draft writes a draft, then the room closes.
    click(&mut d, settings::CLOSE_ID);
    click(&mut d, "button:dialog.saveDraft");
    until(&mut d, |a| !a.state().rooms.contains(&Room::Collage));
    let recipes: Vec<_> = walk(&dir.path().join("Pictures")).into_iter().filter(|p| p.extension().is_some_and(|e| e == "dalica-collage")).collect();
    let drafts = recipes.iter().filter(|p| std::fs::read_to_string(p).unwrap().contains("\"draft\": true")).count();
    assert_eq!((recipes.len(), drafts), (2, 1), "the collage's recipe and the draft's: {recipes:?}");
    assert!(!d.app().session().unwrap().collage().is_open());
}

/// Steps frames until `done`, giving the collage's worker real time (a 3000 px render in a debug
/// build takes seconds), for at most two minutes.
fn until(d: &mut Driver, done: impl Fn(&App) -> bool) {
    let start = std::time::Instant::now();
    while !done(d.app()) {
        assert!(start.elapsed() < std::time::Duration::from_secs(120), "timed out: {:?}", d.app().session().unwrap().jobs());
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    d.settle(FRAMES).unwrap();
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else { out.push(p) }
        }
    }
    out
}

#[test]
fn the_rooms_tabs_are_fingerprinted_and_fit_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = driver(dir.path(), LARGE);
    open_with_six(&mut d, &ids);
    check_rect(&d, "collage-room-settings-1400x900", panel(&d));
    click(&mut d, "tab:collage/clips");
    run(&mut d, "collage.selectClips", json!({ "ids": [ids[1]] }));
    d.settle(FRAMES).unwrap();
    check_rect(&d, "collage-room-clips-1400x900", panel(&d));
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    check_fits(&d, "the Clips tab");
    shot(&d, "collage-room-clips-900x600");
    click(&mut d, "tab:collage/settings");
    check_fits(&d, "the Settings tab");
    shot(&d, "collage-room-settings-900x600");
    for id in [settings::CREATE_ID, settings::RESET_ID, settings::CLOSE_ID, settings::SHADOWS_ID] {
        assert!(try_widget(&d, id).is_some(), "{id} at 900 × 600");
    }
    assert!(d.app().panels().unwrap().unregistered_widgets().is_empty());
}
