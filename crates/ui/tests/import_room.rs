//! The Import room (M11.15, `organizing.md` §3 items 1–3), driven headlessly by automation id on a
//! temporary camera card the session's device list finds inside a mount folder (as
//! `DALICA_DEVICE_ROOTS` names one): Ctrl+M lists the card and its pictures arrive under their
//! group headers; a group's clock selects the group; X excludes the selected picture and its cell
//! dims under the ⊘; "Exclude duplicates" leaves the library's photo out of the count, and off
//! counts it in; the preview's star and rotations reach `import.list`; the arrows, Ctrl+A and the
//! context menu's Exclude All; Folder… with the scripted picker lists that folder; the room's
//! fingerprint in the large window (the small one has to fit).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_region, new_driver, try_widget, widget};
use dalica_core::{Date, DateTime};
use dalica_engine::Session;
use dalica_ui::headless::Click;
use dalica_ui::{App, Driver, ScriptedPicker};
use egui::PointerButton;
use serde_json::{Value, json};

/// A 192 × 128 JPEG of scene `seed` taken on 2026-06-04 at `h:m` (`day` days later).
fn shot(seed: u64, day: u8, h: u8, m: u8) -> Vec<u8> {
    let taken = DateTime::new(Date::new(2026, 6, 4 + day).unwrap(), h, m, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 192, 128), 85, Some(&exif)).unwrap()
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The card's pictures: three in the morning, two after lunch, one the next day; IMG_0001 is the
/// library's photo too.
const CARD: [(&str, u64, u8, u8, u8); 6] = [
    ("IMG_0001.JPG", 1, 0, 8, 0),
    ("IMG_0002.JPG", 2, 0, 8, 5),
    ("IMG_0003.JPG", 3, 0, 8, 10),
    ("IMG_0004.JPG", 4, 0, 13, 0),
    ("IMG_0005.JPG", 5, 0, 13, 2),
    ("IMG_0006.JPG", 6, 1, 9, 30),
];

/// A library holding IMG_0001's twin, a mount folder holding the card `EOS_DIGITAL` when `card`,
/// and a window at `size` with `picker`.
fn driver(dir: &Path, card: bool, picker: Option<ScriptedPicker>, (w, h): (u32, u32)) -> Driver {
    let (_, seed, day, hour, min) = CARD[0];
    put(&dir.join("photos/2026/twin.jpg"), &shot(seed, day, hour, min));
    let mounts = dir.join("mounts");
    std::fs::create_dir_all(&mounts).unwrap();
    if card {
        for (name, seed, day, hour, min) in CARD {
            put(&mounts.join("EOS_DIGITAL/DCIM/100CANON").join(name), &shot(seed, day, hour, min));
        }
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": dir.join("photos/2026") })).unwrap();
    s.wait_idle();
    s.set_import_device_roots(Some(std::env::join_paths([&mounts]).unwrap()), Duration::from_millis(50));
    let app = match picker {
        Some(p) => App::with_picker(Some(s), Arc::new(p)),
        None => App::new(Some(s)),
    };
    let mut d = new_driver(app, w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn run(d: &mut Driver, id: &str, params: Value) -> Value {
    d.app_mut().session_mut().unwrap().execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// The listing as `import.list` gives it.
fn listing(d: &mut Driver) -> Value {
    run(d, "import.list", json!({}))
}

fn candidate(d: &mut Driver, name: &str) -> Value {
    let l = listing(d);
    l["candidates"].as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap_or_else(|| panic!("{name} not listed: {l}"))
}

fn id_of(d: &mut Driver, name: &str) -> u64 {
    candidate(d, name)["id"].as_u64().unwrap()
}

/// Steps frames (letting the listing's worker run) until `done` holds; at most a minute.
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&mut Driver) -> bool) {
    let start = Instant::now();
    while !done(d) {
        assert!(start.elapsed() < Duration::from_secs(60), "never: {what}");
        d.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The listing is done and every thumbnail is in memory.
fn listed(d: &mut Driver) -> bool {
    let l = listing(d);
    l["state"] == "done" && l["candidates"].as_array().is_some_and(|c| !c.is_empty() && c.iter().all(|c| c["thumbnail"] == true))
}

/// The cells and group headers on screen.
fn shown(d: &Driver) -> (usize, usize) {
    let ws = d.widgets();
    let cells = ws.iter().filter(|w| w.id.strip_prefix("candidate:").is_some_and(|n| n.parse::<u64>().is_ok())).count();
    let groups = ws.iter().filter(|w| w.id.starts_with("group:import/") && !w.id.ends_with("/select")).count();
    (cells, groups)
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

fn key(d: &mut Driver, chord: &str) {
    d.key(chord).unwrap();
    d.settle(FRAMES).unwrap();
}

#[test]
fn the_card_arrives_by_group_and_the_tray_and_preview_act_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), true, None, LARGE);

    // Ctrl+M: the room opens on the card (as soon as the device list has it), its pictures arriving
    // under their headers, never fewer than before.
    d.key("Ctrl+M").unwrap();
    let mut seen = (0, 0);
    until(&mut d, "the card listed with its thumbnails", |d| {
        let now = shown(d);
        assert!(now.0 >= seen.0 && now.1 >= seen.1, "cells and groups only arrive: {seen:?} then {now:?}");
        seen = now;
        listed(d) && now == (6, 3)
    });
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "choice:import.source").label, "EOS_DIGITAL");
    assert_eq!(widget(&d, "group:import/0").label, "June 4, 2026 8:00 AM \u{2013} 8:10 AM");
    assert_eq!(widget(&d, "group:import/1").label, "June 4, 2026 1:00 PM \u{2013} 1:02 PM");
    assert_eq!(widget(&d, "group:import/2").label, "June 5, 2026 9:30 AM");
    // IMG_0001 is the library's photo: left out by default, dimmed.
    let twin = id_of(&mut d, "IMG_0001.JPG");
    assert!(candidate(&mut d, "IMG_0001.JPG")["duplicate"]["photo"].is_u64());
    assert_eq!(widget(&d, "text:import.status").label, "6 files \u{b7} 5 to import");
    assert_eq!(widget(&d, &format!("candidate:{twin}/excluded")).label, "Already in your library");
    assert!(widget(&d, "check:import.excludeDuplicates").active);

    // A group's clock selects the group.
    click(&mut d, "group:import/1/select");
    let afternoon = vec![id_of(&mut d, "IMG_0004.JPG"), id_of(&mut d, "IMG_0005.JPG")];
    assert_eq!(d.state().import.selection, afternoon);
    assert!(widget(&d, "group:import/1/select").active && !widget(&d, "group:import/0/select").active);

    // X excludes the selected picture: its cell dims; X again includes it.
    let second = id_of(&mut d, "IMG_0002.JPG");
    click(&mut d, &format!("candidate:{second}"));
    assert_eq!((d.state().import.selection.clone(), d.state().import.preview), (vec![second], Some(second)));
    assert!(try_widget(&d, &format!("candidate:{second}/excluded")).is_none());
    key(&mut d, "X");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["excluded"], true);
    assert_eq!(widget(&d, &format!("candidate:{second}/excluded")).label, "Excluded");
    assert_eq!(widget(&d, "text:import.status").label, "6 files \u{b7} 4 to import");
    key(&mut d, "X");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["excluded"], false);
    assert!(try_widget(&d, &format!("candidate:{second}/excluded")).is_none());

    // "Exclude duplicates" off counts the library's photo in, and its cell is no longer dimmed.
    click(&mut d, "check:import.excludeDuplicates");
    assert_eq!(listing(&mut d)["excludeDuplicates"], false);
    assert_eq!(widget(&d, "text:import.status").label, "6 files \u{b7} 6 to import");
    assert!(try_widget(&d, &format!("candidate:{twin}/excluded")).is_none());
    click(&mut d, "check:import.excludeDuplicates");
    assert_eq!(widget(&d, "text:import.status").label, "6 files \u{b7} 5 to import");

    // The preview: its header, the star and the rotations reach the listing.
    assert!(widget(&d, "text:import.preview").label.starts_with("IMG_0002.JPG \u{b7} June 4, 2026 8:05 AM \u{b7} "));
    click(&mut d, "button:import.star");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["starred"], true);
    assert!(widget(&d, "button:import.star").active && try_widget(&d, &format!("candidate:{second}/star")).is_some());
    click(&mut d, "button:import.rotateRight");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["rotation"], 90);
    click(&mut d, "button:import.rotateLeft");
    click(&mut d, "button:import.rotateLeft");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["rotation"], 270);
    click(&mut d, "button:import.rotateRight");
    click(&mut d, "button:import.exclude");
    assert_eq!(candidate(&mut d, "IMG_0002.JPG")["excluded"], true);
    click(&mut d, "button:import.exclude");

    // The fingerprint, IMG_0002 starred and selected; the small window has to fit.
    check_region(&d, "region:room", "import-room-1400x900");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    check_fits(&d, "the Import room");
    crate::common::shot(&d, "import-room-900x600-window");
}

#[test]
fn the_arrows_ctrl_a_and_the_context_menu_act_in_the_tray() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), true, None, LARGE);
    d.key("Ctrl+M").unwrap();
    until(&mut d, "the card listed", |d| listed(d) && shown(d).0 == 6);
    let ids: Vec<u64> = CARD.iter().map(|(name, ..)| id_of(&mut d, name)).collect();

    // → from nothing reaches the first picture, then the next; Shift+→ runs a range; ← steps back.
    key(&mut d, "Right");
    assert_eq!(d.state().import.preview, Some(ids[0]));
    key(&mut d, "Right");
    key(&mut d, "Shift+Right");
    assert_eq!((d.state().import.selection.clone(), d.state().import.focus), (vec![ids[1], ids[2]], Some(ids[1])));
    click(&mut d, "button:import.previous");
    assert_eq!(d.state().import.selection, [ids[1]]);
    // Ctrl-click adds, Shift-click runs from the focused picture.
    d.click_with(&format!("candidate:{}", ids[4]), &Click { modifiers: egui::Modifiers::COMMAND, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().import.selection, [ids[1], ids[4]]);
    d.click_with(&format!("candidate:{}", ids[3]), &Click { modifiers: egui::Modifiers::SHIFT, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().import.selection, [ids[4], ids[3]]);

    // Ctrl+A selects every picture.
    key(&mut d, "Ctrl+A");
    assert_eq!(d.state().import.selection, ids);

    // The context menu: Exclude All leaves every one out, Include All brings them back.
    d.click_with(&format!("candidate:{}", ids[5]), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "menuitem:import.exclude").enabled && !widget(&d, "menuitem:import.exclude/include").enabled);
    click(&mut d, "menuitem:import.excludeAll");
    assert_eq!(listing(&mut d)["toImport"], 0);
    d.click_with(&format!("candidate:{}", ids[5]), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    click(&mut d, "menuitem:import.excludeAll/include");
    assert_eq!(listing(&mut d)["toImport"], 5);
}

#[test]
fn folder_lists_the_folder_the_picker_gives() {
    let dir = tempfile::tempdir().unwrap();
    let scans = dir.path().join("Scans");
    put(&scans.join("a.jpg"), &shot(7, 0, 10, 0));
    put(&scans.join("b.jpg"), &shot(8, 0, 10, 1));
    let picker = ScriptedPicker::new();
    picker.push_folder(&scans);
    let mut d = driver(dir.path(), false, Some(picker), LARGE);

    // No card: the room says how to start.
    key(&mut d, "Ctrl+M");
    assert_eq!(widget(&d, "empty:import").label, dalica_ui::panels::import::EMPTY_TITLE);
    assert_eq!(widget(&d, "choice:import.source").label, "Choose a source");
    // "Import from:" ▸ Folder…: the picker's folder is listed.
    click(&mut d, "choice:import.source");
    click(&mut d, "choice:import.source/folder");
    until(&mut d, "the folder listed", |d| listed(d) && shown(d).0 == 2);
    let l = listing(&mut d);
    assert_eq!((l["source"]["kind"].as_str(), l["source"]["path"].as_str()), (Some("folder"), scans.to_str()));
    assert_eq!(widget(&d, "choice:import.source").label, "Scans");
    assert!(try_widget(&d, "empty:import").is_none());
}
