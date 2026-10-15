//! The Photo Tray (M4.16, `ui-layout.md` §6.2–§6.3), driven headlessly at 1400 × 900: the tray
//! mirrors the selection and keeps held photos (select 2 in folder A, Hold, select 3 in folder B:
//! 5 in the tray, star them in one undo step); a section selected as a unit is one token; Clear
//! asks first only when photos are held; the minis shrink with their count; the tray has its own
//! selection (click, Ctrl-click, rubber band) and menu; every tray command fails gracefully. The
//! held scenario is fingerprinted as `region-tray-held.json` (`DALICA_BLESS=1` stores it; renders
//! go to `target/test-output/ui/`). M9.18: an album section selected as a unit is one album token
//! with the album's cover (`region-tray-album-<w>x<h>.json` at both sizes), and Add to Album ▾
//! adds the tray's photos to an album or opens Album Properties for them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, out_dir, try_widget};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use dalica_scenes::fingerprint;
use dalica_ui::headless::{Click, PointerEvent, PointerKind, modifiers};
use dalica_ui::{App, Driver, DriverError, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;
use egui::{PointerButton, pos2};
use serde_json::{Value, json};

/// A library in `root` with two folders of `per` scenes photos each, added and scanned.
fn session(root: &Path, per: usize) -> Session {
    let files = dalica_scenes::demo_library(2, 2 * per, 13).unwrap();
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

/// The photos of the light table's sections, in order.
fn sections(d: &Driver) -> Vec<Vec<PhotoId>> {
    d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect()
}

fn n(ids: &[PhotoId]) -> Vec<u64> {
    ids.iter().map(|id| id.get()).collect()
}

/// The tray's minis: (photo, rect), in the order drawn.
fn minis(d: &Driver) -> Vec<(PhotoId, [f32; 4])> {
    d.widgets()
        .into_iter()
        .filter_map(|w| {
            let id = w.id.strip_prefix("tray:item:")?.parse::<u64>().ok()?;
            Some((PhotoId::new(id), w.rect))
        })
        .collect()
}

fn starred(d: &Driver, ids: &[PhotoId]) -> Vec<bool> {
    let library = d.app().session().unwrap().library();
    ids.iter().map(|id| library.photo(*id).unwrap().star).collect()
}

fn select(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": n(ids) })).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Checks the bottom area of the last frame against `tests/fingerprints/<name>.json`.
fn check_bottom(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    // The bottom area: the window's last 96 rows.
    let (x0, y0, w, h) = (0, shot.height - 96, shot.width, 96);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    let png = fingerprint::png_rgba(w, h, &rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// The M4.16 done-when, first part: select 2 in folder A, Hold, select 3 in folder B → the tray has
/// 5 → star → one undo restores all 5.
#[test]
fn held_photos_stay_while_the_selection_moves_and_star_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    let (a, b) = (&s[0], &s[1]);
    assert!(minis(&d).is_empty() && try_widget(&d, "tray").is_some());
    assert!(matches!(d.click("button:tray.hold"), Err(DriverError::Disabled(_))), "nothing to hold");
    assert!(matches!(d.click("button:star"), Err(DriverError::Disabled(_))), "nothing to star");

    select(&mut d, &a[0..2]);
    assert_eq!(minis(&d).iter().map(|m| m.0).collect::<Vec<_>>(), a[0..2], "the tray mirrors the selection");
    d.click("button:tray.hold").unwrap();
    assert_eq!(d.state().tray.held, a[0..2]);
    assert!(matches!(d.click("button:tray.hold"), Err(DriverError::Disabled(_))), "everything in the tray is held");
    select(&mut d, &b[3..6]);
    let tray: Vec<PhotoId> = minis(&d).iter().map(|m| m.0).collect();
    let five: Vec<PhotoId> = a[0..2].iter().chain(&b[3..6]).copied().collect();
    assert_eq!(tray, five, "held first, then the selection");
    for id in &a[0..2] {
        assert!(try_widget(&d, &format!("tray:item:{}/held", id.get())).is_some(), "{id} has the held badge");
    }
    assert!(try_widget(&d, &format!("tray:item:{}/held", b[3].get())).is_none());
    check_bottom(&d, "region-tray-held");

    // Star: all five in one step; undo: all five back.
    d.click("button:star").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(starred(&d, &five), [true; 5]);
    assert!(try_widget(&d, "button:star").unwrap().active);
    d.run("edit.undo", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(starred(&d, &five), [false; 5], "one undo restores all five");
    assert!(matches!(d.run("edit.undo", &json!({})), Err(DriverError::Command(CmdError::Disabled(_)))), "one step only");
    // Starred already: the star button unstars them all (one starred is not all).
    d.run("photo.star.set", &json!({ "ids": [b[3].get()], "on": true })).unwrap();
    d.click("button:star").unwrap();
    assert_eq!(starred(&d, &five), [true; 5], "stars all unless all are starred");
    d.click("button:star").unwrap();
    assert_eq!(starred(&d, &five), [false; 5]);

    // Ctrl+H holds what is selected; Ctrl+D leaves the held ones.
    d.key("Ctrl+H").unwrap();
    assert_eq!(d.state().tray.held.len(), 5);
    d.key("Ctrl+D").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(minis(&d).len(), 5, "held photos survive Select None");
}

/// A section selected as a unit is one token; its count is the section's.
#[test]
fn a_section_selected_as_a_unit_is_one_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    let section = d.app().panels().unwrap().sections.list[1].id;
    d.run("select.section", &json!({ "section": section })).unwrap();
    d.settle(FRAMES).unwrap();
    let token = try_widget(&d, "tray:token").expect("the token is drawn");
    assert_eq!(token.label, "Folder selected — 12 photos");
    assert!(minis(&d).is_empty(), "the section's photos are not minis");
    let tray = try_widget(&d, "tray").unwrap();
    assert!(token.rect[0] >= tray.rect[0] && token.rect[0] + token.rect[2] <= tray.rect[0] + tray.rect[2], "{token:?} in {tray:?}");
    // Held photos sit beside the token; holding the section turns it into minis.
    d.run("tray.hold", &json!({ "ids": [s[0][0].get()] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "tray:token").is_some());
    assert_eq!(minis(&d).iter().map(|m| m.0).collect::<Vec<_>>(), [s[0][0]]);
    d.click("button:tray.hold").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "tray:token").is_none(), "every photo is held: minis now");
    assert_eq!(minis(&d).len(), 13);
    // The star button stars the whole section.
    d.click("button:star").unwrap();
    assert!(starred(&d, &s[1]).iter().all(|s| *s));
}

/// Clear removes the tray-selected photos, or everything; it asks first only when photos are held.
#[test]
fn clear_asks_first_only_when_photos_are_held() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    assert!(matches!(d.click("button:tray.clear"), Err(DriverError::Disabled(_))), "an empty tray");

    // Nothing held: Clear empties the tray (and the selection it mirrors) at once.
    select(&mut d, &s[0][0..3]);
    d.click("button:tray.clear").unwrap();
    assert!(try_widget(&d, "dialog:clearTray").is_none(), "no question without held photos");
    assert!(d.state().selection.ids.is_empty() && minis(&d).is_empty());

    // Held photos: the question, Cancel keeps everything, Clear Tray empties it.
    select(&mut d, &s[0][0..2]);
    d.click("button:tray.hold").unwrap();
    select(&mut d, &s[1][0..1]);
    d.click("button:tray.clear").unwrap();
    let dialog = try_widget(&d, "dialog:clearTray").expect("Clear asks first");
    assert_eq!(dialog.label, "Clear Tray");
    assert_eq!(minis(&d).len(), 3, "nothing cleared yet");
    d.click("button:dialog.cancel").unwrap();
    assert!(try_widget(&d, "dialog:clearTray").is_none());
    assert_eq!(minis(&d).len(), 3);
    d.click("button:tray.clear").unwrap();
    d.click("button:dialog.clearTray").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(minis(&d).is_empty() && d.state().tray.held.is_empty() && d.state().selection.ids.is_empty());

    // Tray-selected photos: only they go, held or not, without a question.
    select(&mut d, &s[0][0..2]);
    d.click("button:tray.hold").unwrap();
    select(&mut d, &s[1][0..2]);
    let first = format!("tray:item:{}", s[0][0].get());
    let last = format!("tray:item:{}", s[1][1].get());
    d.click(&first).unwrap();
    d.click_with(&last, &Click { modifiers: modifiers(true, false, false, false), ..Click::default() }).unwrap();
    assert_eq!(d.state().tray.selected, [s[0][0], s[1][1]]);
    assert!(try_widget(&d, &first).unwrap().active, "a tray-selected mini is active");
    d.click("button:tray.clear").unwrap();
    assert!(try_widget(&d, "dialog:clearTray").is_none());
    assert_eq!(minis(&d).iter().map(|m| m.0).collect::<Vec<_>>(), [s[0][1], s[1][0]]);
    assert_eq!((d.state().tray.held.as_slice(), d.state().selection.ids.as_slice()), ([s[0][1]].as_slice(), [s[1][0]].as_slice()));

    // Run without `confirm`, the command asks through the dialog host; with it, it clears.
    let r = d.app_mut().run("tray.clear", &json!({}));
    assert_eq!(r.unwrap(), json!({ "dialog": "clearTray" }));
    assert_eq!(d.app_mut().run("tray.clear", &json!({ "confirm": true })).unwrap()["count"], 0);
}

/// The M4.16 done-when: mini-thumbnail sizes at 1, 12, 40 and 100 items in the 1400 × 900 window
/// (a tray 449 wide): 54 in a row, 30 in a row, 22 in three rows, 18 in three rows with the rest
/// counted on the last place.
#[test]
fn minis_shrink_as_the_count_grows() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 60);
    let s = sections(&d);
    let all: Vec<PhotoId> = s[0].iter().chain(&s[1]).copied().collect();
    let tray = try_widget(&d, "tray").unwrap().rect;
    for (count, edge, rows) in [(1, 54.0, 1), (12, 30.0, 1), (40, 22.0, 3), (100, 18.0, 3)] {
        d.run("tray.clear", &json!({ "confirm": true })).ok();
        d.run("tray.hold", &json!({ "ids": n(&all[..count]) })).unwrap();
        d.settle(FRAMES).unwrap();
        let m = minis(&d);
        let shown = m.len() + usize::from(try_widget(&d, "tray:more").is_some());
        assert!(m.iter().all(|(_, r)| r[2] == edge && r[3] == edge), "{count}: {m:?}");
        let tops: BTreeSet<i32> = m.iter().map(|(_, r)| r[1] as i32).collect();
        assert_eq!(tops.len(), rows, "{count}: rows {tops:?}");
        for (_, r) in &m {
            assert!(r[0] >= tray[0] && r[1] >= tray[1] && r[0] + r[2] <= tray[0] + tray[2] && r[1] + r[3] <= tray[1] + tray[3], "{count}: {r:?}");
        }
        if count == 100 {
            let more = try_widget(&d, "tray:more").expect("the rest is counted");
            assert_eq!(more.label, format!("{} more", count - m.len()));
            assert!(shown < count);
        } else {
            assert_eq!(shown, count, "{count}: all shown");
        }
    }
    // Hostile sizes never panic: the narrowest window still lays out.
    d.resize(900, 600);
    d.settle(FRAMES).unwrap();
    assert!(minis(&d).iter().all(|(_, r)| r[2] == 18.0));
}

/// The tray's own selection: a click, Ctrl-click, a click on empty space, and the rubber band; and
/// the tray menu.
#[test]
fn the_tray_has_its_own_selection_and_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 12);
    let s = sections(&d);
    select(&mut d, &s[0][0..5]);
    let m = minis(&d);
    d.click(&format!("tray:item:{}", m[1].0.get())).unwrap();
    assert_eq!(d.state().tray.selected, [m[1].0]);
    // A click on the box's empty space selects nothing.
    let tray = try_widget(&d, "tray").unwrap().rect;
    d.click_with("tray", &Click { fx: 0.95, ..Click::default() }).unwrap();
    assert!(d.state().tray.selected.is_empty());
    // A band from the empty right part over the last three minis.
    let y = m[0].1[1] + m[0].1[3] / 2.0;
    let from = pos2(tray[0] + tray[2] - 10.0, y - 20.0);
    let to = pos2(m[2].1[0] + 5.0, y + 5.0);
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: from }, PointerEvent { kind: PointerKind::Down, pos: from }];
    events.extend((1..=8).map(|i| PointerEvent { kind: PointerKind::Drag, pos: from.lerp(to, i as f32 / 8.0) }));
    events.push(PointerEvent { kind: PointerKind::Up, pos: to });
    d.pointer(&events, PointerButton::Primary, egui::Modifiers::NONE).unwrap();
    let mut got = d.state().tray.selected.clone();
    got.sort();
    let mut want = vec![m[2].0, m[3].0, m[4].0];
    want.sort();
    assert_eq!(got, want, "the band selects what it touches");
    // The selection moving on drops tray-selected photos the tray no longer shows.
    select(&mut d, &s[0][3..4]);
    assert_eq!(d.state().tray.selected, [s[0][3]]);

    // The tray menu: View and Edit (M7.4), Hold and Remove from Tray live, the rest greyed until their commands.
    select(&mut d, &s[0][0..3]);
    let item = format!("tray:item:{}", s[0][2].get());
    d.click_with(&item, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    assert_eq!(d.state().tray.selected, [s[0][2]], "a right-click selects the mini first");
    for (id, live) in [("menuitem:tray.hold", true), ("menuitem:tray.clear", true), ("menuitem:pane.properties", true), ("menuitem:view.edit", true)]
    {
        let w = try_widget(&d, id).unwrap_or_else(|| panic!("no {id}"));
        assert_eq!(w.enabled, live, "{id}");
    }
    d.click("menuitem:tray.clear").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(minis(&d).iter().map(|m| m.0).collect::<Vec<_>>(), &s[0][0..2], "Remove from Tray took just that one");
}

/// An action that needs photos with an empty tray says why, and every tray command refuses bad
/// parameters without changing anything.
#[test]
fn tray_commands_fail_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4);
    let s = sections(&d);
    d.key("Ctrl+H").unwrap();
    assert_eq!(d.state().status, "Select some photos first.");
    let app = d.app_mut();
    for (id, params) in [
        ("tray.hold", json!({ "ids": "1" })),
        ("tray.hold", json!({ "ids": [-1] })),
        ("tray.hold", json!({ "ids": [1.5] })),
        ("tray.hold", json!({ "extra": 1 })),
        ("tray.hold", json!([1])),
        ("tray.hold", json!("x")),
        ("tray.clear", json!({ "confirm": "yes" })),
        ("tray.clear", json!({ "ids": {} })),
        ("tray.clear", json!(7)),
        ("tray.select", json!({})),
        ("tray.select", Value::Null),
        ("tray.select", json!({ "ids": [], "mode": "sideways" })),
        ("tray.select", json!({ "ids": [], "extra": true })),
    ] {
        assert!(matches!(app.run(id, &params), Err(CmdError::BadParams(_))), "{id} {params}");
    }
    let before = app.state().clone();
    assert!(matches!(app.run("tray.hold", &json!({})), Err(CmdError::Disabled(_))), "nothing selected");
    assert!(matches!(app.run("tray.hold", &json!({ "ids": [] })), Err(CmdError::Disabled(_))));
    assert!(matches!(app.run("tray.hold", &json!({ "ids": [u64::MAX] })), Err(CmdError::NotFound(_))));
    assert!(matches!(app.run("tray.clear", &json!({})), Err(CmdError::Disabled(_))), "an empty tray");
    let too_many: Vec<u64> = (1..=1_000_001).collect();
    assert!(matches!(app.run("tray.hold", &json!({ "ids": too_many })), Err(CmdError::Limit(_))));
    app.run("select.set", &json!({ "ids": [s[0][0].get()] })).unwrap();
    assert!(matches!(app.run("tray.clear", &json!({ "ids": [s[1][0].get()] })), Err(CmdError::NotFound(_))), "not in the tray");
    assert!(matches!(app.run("tray.select", &json!({ "ids": [s[1][0].get()] })), Err(CmdError::NotFound(_))));
    let after = app.state();
    assert_eq!((&after.tray, after.selection.ids.as_slice()), (&before.tray, [s[0][0]].as_slice()), "nothing but the selection changed");
    // Without a library: refused, never a panic.
    let mut empty = App::new(None);
    assert!(matches!(empty.run("tray.hold", &json!({ "ids": [1] })), Err(CmdError::NotFound(_))));
    assert!(matches!(empty.run("tray.clear", &json!({})), Err(CmdError::Disabled(_))));
}

/// An album section selected as a unit is one token, "Album selected — 3 photos", showing the
/// album's cover; at both sizes.
#[test]
fn an_album_selected_as_a_unit_is_one_album_token() {
    use dalica_ui::state::{AlbumKey, AlbumSection, Section, Selection};
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4);
    let s = sections(&d);
    let members = [s[1][0], s[0][2], s[0][0]];
    let album = d.run("album.create", &json!({ "name": "Trams", "ids": n(&members) })).unwrap()["album"].as_str().unwrap().to_string();
    d.run("album.setCover", &json!({ "album": album, "id": s[0][2].get() })).unwrap();
    let id = dalica_core::AlbumId::parse(&album).unwrap();
    // Selecting the album section as a unit (its tree row or header) selects its photos so.
    d.app_mut().state_mut().selection = Selection::new(members.to_vec(), None, Some(Section::Album(AlbumSection::User(AlbumKey::of(&id)))), true);
    for (w, h) in [(1400, 900), (900, 600)] {
        d.resize(w, h);
        d.settle(FRAMES).unwrap();
        let token = try_widget(&d, "tray:token").expect("the album token is drawn");
        assert_eq!(token.label, "Album selected — 3 photos");
        assert!(minis(&d).is_empty(), "the album's photos are not minis");
        check_bottom(&d, &format!("region-tray-album-{w}x{h}"));
    }
}

/// The Add to Album ▾ popup's rows, top to bottom.
fn popup_rows(d: &Driver) -> Vec<(String, String, bool)> {
    let mut rows: Vec<WidgetInfo> = d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:album.")).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.into_iter().map(|w| (w.id, w.label, w.enabled)).collect()
}

/// Add to Album ▾ is live while the tray has photos: New Album…, then the albums (the most recent
/// first); an album adds the tray's photos (held ones first) in one step; New Album… opens Album
/// Properties for them.
#[test]
fn add_to_album_adds_the_tray_photos() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4);
    let s = sections(&d);
    let old = d.run("album.create", &json!({ "name": "Older", "date": "2001-05-01" })).unwrap()["album"].as_str().unwrap().to_string();
    let new = d.run("album.create", &json!({ "name": "Newer", "date": "2024-05-01" })).unwrap()["album"].as_str().unwrap().to_string();
    d.settle(FRAMES).unwrap();
    assert!(matches!(d.click("button:tray.addToAlbum"), Err(DriverError::Disabled(_))), "an empty tray");
    // One held photo of the first folder and two selected in the second: three in the tray.
    select(&mut d, &s[0][0..1]);
    d.click("button:tray.hold").unwrap();
    select(&mut d, &s[1][1..3]);
    assert!(try_widget(&d, "button:tray.addToAlbum").unwrap().enabled);
    d.click("button:tray.addToAlbum").unwrap();
    d.settle(FRAMES).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join("tray-add-to-album-1400x900-window.png"), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let rows = popup_rows(&d);
    let ids: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(ids, ["menuitem:album.new".to_string(), format!("menuitem:album.add/{new}"), format!("menuitem:album.add/{old}")]);
    assert_eq!(rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["New Album…", "Newer", "Older"]);
    assert!(rows.iter().all(|r| r.2), "all live");
    let button = try_widget(&d, "button:tray.addToAlbum").unwrap().rect;
    let new_album = try_widget(&d, "menuitem:album.new").unwrap().rect;
    assert!(new_album[1] + new_album[3] <= button[1] + 0.5, "the popup opens upward: {new_album:?} over {button:?}");
    // A second click closes it.
    d.click("button:tray.addToAlbum").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(popup_rows(&d).is_empty());

    // An album: the tray's photos, held first, in one step.
    let steps = d.app().session().unwrap().undo_stack().len();
    d.click("button:tray.addToAlbum").unwrap();
    d.settle(FRAMES).unwrap();
    d.click(&format!("menuitem:album.add/{old}")).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(popup_rows(&d).is_empty(), "the popup closed");
    let album = dalica_core::AlbumId::parse(&old).unwrap();
    let want = [s[0][0], s[1][1], s[1][2]];
    assert_eq!(d.app().session().unwrap().library().album_members(&album), want);
    assert_eq!(d.app().session().unwrap().undo_stack().len(), steps + 1);
    d.key("Ctrl+Z").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.app().session().unwrap().library().album_members(&album).is_empty(), "one undo takes them all out");

    // New Album…: Album Properties for the tray's photos.
    d.click("button:tray.addToAlbum").unwrap();
    d.settle(FRAMES).unwrap();
    d.click("menuitem:album.new").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "dialog:albumProperties").is_some());
    let draft = d.state().album_properties.clone().unwrap();
    assert_eq!((draft.album, draft.ids), (None, want.to_vec()));
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    // Without photos it is disabled again.
    d.run("tray.clear", &json!({ "confirm": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!try_widget(&d, "button:tray.addToAlbum").unwrap().enabled);
}
