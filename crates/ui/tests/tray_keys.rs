//! The photo shortcuts on the Photo Tray, and the minis' place in it (M5.23, `ui-layout.md`
//! §6.2–§6.3), driven headlessly at 1400 × 900: with 2 photos held and 3 selected, Ctrl+8 and F4
//! star all 5 in one undo step, the same set the star button acts on; with nothing held they act on
//! the selection; with an empty tray they are refused with the reason on the status line; the
//! minis sit centred in the tray box at 1, 12 and 40 items, and beside a token.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, try_widget};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use dalica_ui::{App, Driver, DriverError, ScriptedPicker};
use serde_json::json;

/// A library in `root` with two folders of `per` scenes photos each, added and scanned.
fn session(root: &Path, per: usize) -> Session {
    let files = crate::common::demo_library(2, 2 * per, 13).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    assert_eq!(folders.len(), 2);
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(root: &Path, per: usize) -> Driver {
    let mut d = new_driver(App::with_picker(Some(session(root, per)), Arc::new(ScriptedPicker::new())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn sections(d: &Driver) -> Vec<Vec<PhotoId>> {
    d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect()
}

fn n(ids: &[PhotoId]) -> Vec<u64> {
    ids.iter().map(|id| id.get()).collect()
}

/// The rects of the tray's minis and of its "+N" place, in the order drawn.
fn places(d: &Driver) -> Vec<[f32; 4]> {
    d.widgets().into_iter().filter(|w| w.id.starts_with("tray:item:") && !w.id.ends_with("/held") || w.id == "tray:more").map(|w| w.rect).collect()
}

fn starred(d: &Driver, ids: &[PhotoId]) -> Vec<bool> {
    let library = d.app().session().unwrap().library();
    ids.iter().map(|id| library.photo(*id).unwrap().star).collect()
}

fn select(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": n(ids) })).unwrap();
    d.settle(FRAMES).unwrap();
}

fn undo_is_empty(d: &mut Driver) -> bool {
    matches!(d.run("edit.undo", &json!({})), Err(DriverError::Command(CmdError::Disabled(_))))
}

/// The done-when: with 2 held and 3 selected, the star key stars all 5 in one undo step; F4 too;
/// with nothing held, the keys act on the selection only.
#[test]
fn the_star_keys_act_on_the_trays_photos_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    let (a, b) = (&s[0], &s[1]);
    select(&mut d, &a[0..2]);
    d.click("button:tray.hold").unwrap();
    select(&mut d, &b[3..6]);
    let five: Vec<PhotoId> = a[0..2].iter().chain(&b[3..6]).copied().collect();
    assert_eq!(d.state().selection.ids, b[3..6], "3 selected");
    assert_eq!(d.state().tray.held, a[0..2], "2 held");

    for key in ["Ctrl+8", "F4"] {
        d.key(key).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(starred(&d, &five), [true; 5], "{key} stars all five");
        assert!(try_widget(&d, "button:star").unwrap().active, "{key}: the star button agrees");
        d.run("edit.undo", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(starred(&d, &five), [false; 5], "{key}: one undo restores all five");
        assert!(undo_is_empty(&mut d), "{key}: one step only");
    }

    // Stars all unless all are starred: one starred held photo is not all.
    d.run("photo.star.set", &json!({ "ids": [a[0].get()], "on": true })).unwrap();
    d.key("Ctrl+8").unwrap();
    assert_eq!(starred(&d, &five), [true; 5]);
    d.key("Ctrl+8").unwrap();
    assert_eq!(starred(&d, &five), [false; 5], "all starred: the key unstars them all");

    // Nothing held: the keys act on the selection alone.
    d.run("tray.clear", &json!({ "confirm": true })).unwrap();
    select(&mut d, &b[0..2]);
    d.key("F4").unwrap();
    assert_eq!(starred(&d, &b[0..3]), [true, true, false]);
    assert_eq!(starred(&d, &a[0..2]), [false, false], "photos that left the tray are untouched");
}

/// The keys and the menus pass the same photos as the bottom buttons: every tray command gets the
/// tray's photos, everything else the selection.
#[test]
fn shortcuts_menus_and_buttons_share_one_set_of_targets() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4);
    let s = sections(&d);
    select(&mut d, &s[0][0..1]);
    d.click("button:tray.hold").unwrap();
    select(&mut d, &s[1][0..2]);
    let tray: Vec<PhotoId> = vec![s[0][0], s[1][0], s[1][1]];
    for id in dalica_ui::keys::TRAY_COMMANDS {
        assert!(dalica_ui::keys::acts_on_tray(id));
        assert_eq!(dalica_ui::keys::targets(d.app(), id), tray, "{id}");
    }
    let star = dalica_ui::keys::params_for(d.app(), "photo.star.toggle");
    assert_eq!(star, json!({ "ids": n(&tray) }));
    assert_eq!(dalica_ui::keys::params_for(d.app(), "photo.inspect"), json!({ "ids": n(&s[1][0..2]) }), "not a tray command");
    assert_eq!(dalica_ui::keys::params_for(d.app(), "edit.undo"), json!({}), "no ids");
    assert_eq!(dalica_ui::keys::params_for(d.app(), "no.such.command"), json!({}));
    assert!(!dalica_ui::keys::acts_on_tray("") && !dalica_ui::keys::acts_on_tray("photo.star"));
}

/// Graceful failures: the star keys on an empty tray are refused with the reason on the status line
/// and change nothing; a session-less app ignores them.
#[test]
fn the_star_keys_fail_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4);
    let s = sections(&d);
    for key in ["Ctrl+8", "F4"] {
        d.key(key).unwrap();
        assert_eq!(d.state().status, "Select some photos first.", "{key}: the reason is on the status line");
        assert!(starred(&d, &s[0]).iter().chain(&starred(&d, &s[1])).all(|s| !s), "{key}: nothing starred");
        assert!(undo_is_empty(&mut d), "{key}: nothing to undo");
    }
    // Held photos only (the selection empty): the keys still reach them.
    select(&mut d, &s[0][0..1]);
    d.click("button:tray.hold").unwrap();
    d.key("Ctrl+D").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().selection.ids.is_empty());
    d.key("Ctrl+8").unwrap();
    assert_eq!(starred(&d, &s[0][0..1]), [true]);

    let mut none = new_driver(App::new(None), 900, 600, 1.0);
    for _ in 0..3 {
        none.step();
    }
    for key in ["Ctrl+8", "F4"] {
        none.key(key).unwrap();
    }
    assert!(dalica_ui::keys::targets(none.app(), "photo.star.toggle").is_empty());
}

/// The left and right room between the tray box and its minis' block.
fn margins(tray: [f32; 4], places: &[[f32; 4]]) -> (f32, f32) {
    let left = places.iter().map(|r| r[0]).fold(f32::INFINITY, f32::min);
    let right = places.iter().map(|r| r[0] + r[2]).fold(f32::NEG_INFINITY, f32::max);
    (left - tray[0], tray[0] + tray[2] - right)
}

/// The done-when: the minis sit centred in the tray box at 1, 12 and 40 items (and with the "+N"
/// place at 100), on whole points.
#[test]
fn the_minis_are_centred_in_the_tray() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 60);
    let s = sections(&d);
    let all: Vec<PhotoId> = s[0].iter().chain(&s[1]).copied().collect();
    let tray = try_widget(&d, "tray").unwrap().rect;
    for count in [1, 12, 40, 100] {
        d.run("tray.clear", &json!({ "confirm": true })).ok();
        d.run("tray.hold", &json!({ "ids": n(&all[..count]) })).unwrap();
        d.settle(FRAMES).unwrap();
        let p = places(&d);
        assert!(!p.is_empty(), "{count}");
        let (left, right) = margins(tray, &p);
        assert!((left - right).abs() <= 1.0, "{count} items: {left} left, {right} right");
        assert!(left >= 4.0, "{count}: at least the margin");
        assert!(p.iter().all(|r| r[0].fract() == 0.0 && r[1].fract() == 0.0), "{count}: on whole points");
        if count == 1 {
            let centre = p[0][0] + p[0][2] / 2.0;
            assert!((centre - (tray[0] + tray[2] / 2.0)).abs() <= 1.0, "one mini sits in the middle");
        }
    }
}

/// Beside a token the held minis are centred in the room right of its pill.
#[test]
fn minis_beside_a_token_are_centred_in_the_room_left() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    d.run("tray.hold", &json!({ "ids": [s[0][0].get()] })).unwrap();
    let section = d.app().panels().unwrap().sections.list[1].id;
    d.run("select.section", &json!({ "section": section })).unwrap();
    d.settle(FRAMES).unwrap();
    let token = try_widget(&d, "tray:token").expect("the token").rect;
    let tray = try_widget(&d, "tray").unwrap().rect;
    let p = places(&d);
    assert_eq!(p.len(), 1);
    let room = [token[0] + token[2], tray[1], tray[0] + tray[2] - (token[0] + token[2]), tray[3]];
    let (left, right) = margins(room, &p);
    assert!(left > 0.0 && (left - right).abs() <= 6.0, "{left} left of the mini, {right} right of it");
}
