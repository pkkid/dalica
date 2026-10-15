//! The selection model and keyboard navigation (M4.13, `ui-layout.md` §5.4), driven headlessly
//! by widget id and key as a user would: click, Ctrl-click, Shift-click, right-click and the rubber
//! band, all scoped to one section; the arrows, Home and End (Shift extends) stopping at the
//! section's ends and scrolling what they reach into view; `select.all`, `select.invert`,
//! `select.starred`, `select.none` and `select.section` per section; every select command fails
//! gracefully; the keys are bound without conflicts on every platform.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::PhotoId;
use dalica_engine::keymap::{Keymap, Platform};
use dalica_engine::{CmdError, Context, Key, Session};
use dalica_ui::headless::{Click, PointerEvent, PointerKind, modifiers};
use dalica_ui::state::{Section, Selection};
use dalica_ui::{App, Driver, UI_COMMANDS};
use egui::{PointerButton, Pos2, Rect, pos2};
use serde_json::{Value, json};

/// A session on a library under `dir`: `folders` folders sharing `photos` scenes photos.
fn session(dir: &Path, folders: usize, photos: usize) -> Session {
    let files = dalica_scenes::demo_library(folders, photos, 5).unwrap();
    let mut roots = BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    s
}

/// A window `size` on that library with thumbnails `thumb` points wide, settled.
fn driver(dir: &Path, folders: usize, photos: usize, size: (u32, u32), thumb: f32) -> Driver {
    let mut d = new_driver(App::new(Some(session(dir, folders, photos))), size.0, size.1, 1.0);
    d.app_mut().state_mut().thumb_size = thumb;
    d.settle(FRAMES).unwrap();
    d
}

/// The sections in order, each with its photos in display order.
fn sections(d: &Driver) -> Vec<(Section, Vec<PhotoId>)> {
    d.app().panels().unwrap().sections.list.iter().map(|s| (s.id, s.photos.clone())).collect()
}

fn cols(d: &Driver) -> usize {
    d.app().panels().unwrap().light_table.layout().unwrap().grid().cols
}

fn thumb(id: PhotoId) -> String {
    format!("thumb:{}", id.get())
}

fn rect_of(d: &Driver, id: &str) -> Rect {
    let w = d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no widget {id}"));
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]))
}

fn sel(d: &Driver) -> Selection {
    d.state().selection.clone()
}

fn ctrl() -> egui::Modifiers {
    modifiers(true, false, false, false)
}

fn shift() -> egui::Modifiers {
    modifiers(false, true, false, false)
}

fn click(d: &mut Driver, id: PhotoId, mods: egui::Modifiers) {
    d.click_with(&thumb(id), &Click { modifiers: mods, ..Click::default() }).unwrap();
}

fn right_click(d: &mut Driver, id: PhotoId) {
    d.click_with(&thumb(id), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
}

/// A drag from `from` to `to` in screen points, one frame per step.
fn band(d: &mut Driver, from: Pos2, to: Pos2, mods: egui::Modifiers) {
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: from }, PointerEvent { kind: PointerKind::Down, pos: from }];
    for i in 1..=10 {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: from.lerp(to, i as f32 / 10.0) });
    }
    events.push(PointerEvent { kind: PointerKind::Up, pos: to });
    d.pointer(&events, PointerButton::Primary, mods).unwrap();
}

fn set(ids: &[PhotoId]) -> HashSet<PhotoId> {
    ids.iter().copied().collect()
}

/// Click, Ctrl-click, Shift-click and right-click by widget id; a click in another section replaces
/// the selection (with Ctrl too), a range never crosses into it, and the tree's folder follows.
#[test]
fn clicks_select_toggle_and_range_inside_one_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 16, (1400, 900), 64.0);
    let s = sections(&d);
    let ((a, pa), (b, pb)) = (s[0].clone(), s[1].clone());
    let fb = b.folder().unwrap();

    click(&mut d, pa[1], egui::Modifiers::NONE);
    assert_eq!(sel(&d), Selection::new(vec![pa[1]], Some(pa[1]), Some(a), false));
    click(&mut d, pa[3], ctrl());
    assert_eq!((sel(&d).ids, sel(&d).anchor), (vec![pa[1], pa[3]], Some(pa[3])), "Ctrl-click adds");
    click(&mut d, pa[3], ctrl());
    assert_eq!(sel(&d).ids, vec![pa[1]], "Ctrl-click again removes");
    click(&mut d, pa[5], shift());
    assert_eq!((sel(&d).ids, sel(&d).anchor), (pa[1..=5].to_vec(), Some(pa[1])), "Shift-click: the range from the anchor");
    click(&mut d, pa[0], shift());
    assert_eq!(sel(&d).ids, vec![pa[1], pa[0]], "the range follows the click; the anchor stays");

    // Another section: Ctrl-click replaces, Shift-click never reaches back.
    click(&mut d, pb[2], ctrl());
    assert_eq!(sel(&d), Selection::new(vec![pb[2]], Some(pb[2]), Some(b), false));
    assert_eq!(d.state().folder, Some(fb), "the tree's folder follows the selection");
    click(&mut d, pa[4], shift());
    assert_eq!((sel(&d).ids, sel(&d).section), (vec![pa[4]], Some(a)), "a range never crosses a section");

    // Right-click: an unselected photo is selected first; a selected one keeps the selection.
    click(&mut d, pa[2], shift());
    let before = sel(&d);
    right_click(&mut d, pa[3]);
    assert_eq!(sel(&d), before, "right-click on a selected photo keeps the selection");
    right_click(&mut d, pa[7]);
    assert_eq!(sel(&d).ids, vec![pa[7]]);

    // A click on empty space (the left margin beside the first row) selects nothing.
    let cell = rect_of(&d, &thumb(pa[0]));
    d.pointer(
        &[
            PointerEvent { kind: PointerKind::Move, pos: pos2(cell.left() - 8.0, cell.center().y) },
            PointerEvent { kind: PointerKind::Down, pos: pos2(cell.left() - 8.0, cell.center().y) },
            PointerEvent { kind: PointerKind::Up, pos: pos2(cell.left() - 8.0, cell.center().y) },
        ],
        PointerButton::Primary,
        egui::Modifiers::NONE,
    )
    .unwrap();
    assert_eq!(sel(&d), Selection::default());
}

/// The rubber band selects what it touches in the section it started in, never beyond it; Ctrl adds
/// to the selection; pressing on a thumbnail and dragging starts no band.
#[test]
fn a_band_selects_what_it_touches_and_never_crosses_a_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 40, (1400, 900), 64.0);
    let s = sections(&d);
    let ((a, pa), (_, pb)) = (s[0].clone(), s[1].clone());
    let n = cols(&d);
    assert!(pa.len() > n, "section A has two rows ({} photos, {n} columns)", pa.len());
    let (c0, c1) = (rect_of(&d, &thumb(pa[0])), rect_of(&d, &thumb(pa[1])));
    let gap = rect_of(&d, &thumb(pa[1])).left() - c0.right();
    assert!(d.widgets().iter().any(|w| w.id == "lightTable"), "the empty space has its id");

    // From the left margin of the first row to between the 2nd and 3rd column of the same row.
    band(&mut d, pos2(c0.left() - 8.0, c0.center().y), pos2(c1.right() + gap / 2.0, c0.center().y + 4.0), egui::Modifiers::NONE);
    assert_eq!((sel(&d).ids, sel(&d).section), (vec![pa[0], pa[1]], Some(a)));

    // Across every column and down into the next section and beyond: only A's photos.
    let last_b = rect_of(&d, &thumb(*pb.last().unwrap()));
    let right = rect_of(&d, &thumb(pa[n - 1])).right() + 4.0;
    band(&mut d, pos2(c0.left() - 8.0, c0.center().y), pos2(right, last_b.bottom() + 4.0), egui::Modifiers::NONE);
    assert_eq!((set(&sel(&d).ids), sel(&d).section), (set(&pa), Some(a)), "the whole of A, nothing of B");

    // Ctrl adds the band to the selection in its section.
    click(&mut d, pa[n], egui::Modifiers::NONE);
    band(&mut d, pos2(c0.left() - 8.0, c0.center().y), pos2(c0.right() + gap / 2.0, c0.center().y), ctrl());
    assert_eq!(set(&sel(&d).ids), set(&[pa[n], pa[0]]));

    // Pressing on a thumbnail and dragging selects nothing new.
    let before = sel(&d);
    let on_thumb = rect_of(&d, &thumb(pa[3])).center();
    band(&mut d, on_thumb, last_b.center(), egui::Modifiers::NONE);
    assert_eq!(sel(&d), before, "a drag from a thumbnail is not a band");
}

/// Arrows move (Shift extends), Home / End go to the section's first / last photo, and all of them
/// stop at the section's ends.
#[test]
fn arrows_home_and_end_stop_at_the_section_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 40, (1400, 900), 64.0);
    let s = sections(&d);
    let ((a, pa), (b, pb)) = (s[0].clone(), s[1].clone());
    let n = cols(&d);
    let anchor = |d: &Driver| d.state().selection.anchor;

    // Nothing selected: an arrow selects the current section's first photo.
    d.key("Right").unwrap();
    assert_eq!((sel(&d).ids, sel(&d).section), (vec![pa[0]], Some(a)));
    d.key("Left").unwrap();
    assert_eq!(anchor(&d), Some(pa[0]), "Left stops at the first photo");
    d.key("Up").unwrap();
    assert_eq!(anchor(&d), Some(pa[0]), "Up stops at the first row");
    d.key("Right").unwrap();
    assert_eq!(anchor(&d), Some(pa[1]));
    d.key("Down").unwrap();
    assert_eq!(anchor(&d), Some(pa[1 + n]));
    d.key("End").unwrap();
    assert_eq!(sel(&d).ids, vec![*pa.last().unwrap()]);
    for k in ["Right", "Down"] {
        d.key(k).unwrap();
        assert_eq!((anchor(&d), sel(&d).section), (pa.last().copied(), Some(a)), "{k} stops at the section's last photo");
    }
    d.key("Home").unwrap();
    assert_eq!(sel(&d).ids, vec![pa[0]]);

    // Shift extends from the anchor and shrinks back; Shift+End selects to the end.
    d.key("Shift+Right").unwrap();
    d.key("Shift+Right").unwrap();
    assert_eq!((sel(&d).ids, anchor(&d)), (pa[0..=2].to_vec(), Some(pa[0])));
    d.key("Shift+Down").unwrap();
    assert_eq!(sel(&d).ids, pa[0..=2 + n].to_vec());
    d.key("Shift+Left").unwrap();
    assert_eq!(sel(&d).ids, pa[0..=1 + n].to_vec());
    d.key("Shift+End").unwrap();
    assert_eq!((sel(&d).ids, sel(&d).section), (pa.clone(), Some(a)), "Shift+End: the rest of the section, not the next one");
    d.key("Shift+Home").unwrap();
    assert_eq!(sel(&d).ids, vec![pa[0]]);

    // In the second section, Left and Up stop at its first photo instead of going back to A.
    click(&mut d, pb[0], egui::Modifiers::NONE);
    for k in ["Left", "Up", "Shift+Left", "Shift+Up", "Shift+Home"] {
        d.key(k).unwrap();
        assert_eq!((sel(&d).ids, sel(&d).section), (vec![pb[0]], Some(b)), "{k}");
    }
}

/// An arrow that reaches a photo out of view scrolls it into view.
#[test]
fn arrows_scroll_the_photo_they_reach_into_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 1, 120, (900, 600), 144.0);
    let s = sections(&d);
    let pa = s[0].1.clone();
    let area = d.app().panels().unwrap().light_table.frame_info().area.unwrap();
    let visible = |d: &Driver, id: PhotoId| {
        d.widgets()
            .into_iter()
            .find(|w| w.id == thumb(id))
            .is_some_and(|w| w.rect[1] >= area.top() - 0.5 && w.rect[1] + w.rect[3] <= area.bottom() + 0.5)
    };
    let last = *pa.last().unwrap();
    assert!(!visible(&d, last), "the last photo starts out of view");
    d.key("End").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(visible(&d, last), "End scrolled the last photo into view");
    d.key("Home").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(visible(&d, pa[0]), "Home scrolled back");
    assert!(d.app().panels().unwrap().light_table.offset() < 1.0, "to the section's header");
}

/// Ctrl+A, Ctrl+I, Ctrl+D and Select Starred per section, and `select.section` as a unit.
#[test]
fn the_selection_commands_work_per_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 16, (1400, 900), 64.0);
    let s = sections(&d);
    let ((a, pa), (b, pb)) = (s[0].clone(), s[1].clone());
    let fb = b.folder().unwrap();

    click(&mut d, pa[2], egui::Modifiers::NONE);
    d.key("Ctrl+I").unwrap();
    let rest: Vec<PhotoId> = pa.iter().copied().filter(|&p| p != pa[2]).collect();
    assert_eq!((sel(&d).ids, sel(&d).section), (rest, Some(a)), "Ctrl+I: the rest of the section");
    d.key("Ctrl+A").unwrap();
    assert_eq!((sel(&d).ids, sel(&d).section, sel(&d).whole), (pa.clone(), Some(a), false), "Ctrl+A: the section");
    d.key("Ctrl+I").unwrap();
    assert_eq!(sel(&d), Selection::default(), "inverting everything leaves nothing");
    d.key("Ctrl+A").unwrap();
    d.key("Ctrl+D").unwrap();
    assert_eq!(sel(&d), Selection::default());

    // Starred: only the section's starred photos; none starred refuses and changes nothing.
    d.run("photo.star.toggle", &json!({ "ids": [pb[1].get(), pb[4].get()] })).unwrap();
    d.run("select.starred", &json!({ "section": b.to_string() })).unwrap();
    assert_eq!((sel(&d).ids, sel(&d).section), (vec![pb[1], pb[4]], Some(b)));
    assert_eq!(d.state().folder, Some(fb));
    let before = sel(&d);
    assert!(matches!(d.app_mut().run("select.starred", &json!({ "section": a.to_string() })), Err(CmdError::Disabled(_))));
    assert_eq!(sel(&d), before);
    // Without a section the current one (the selection's) is used.
    d.run("select.all", &json!({})).unwrap();
    assert_eq!(sel(&d).ids, pb);
    d.run("select.invert", &json!({ "section": a.to_string() })).unwrap();
    assert_eq!((sel(&d).ids, sel(&d).section), (pa.clone(), Some(a)), "inverting another section: all of it");

    // A section as a unit.
    d.run("select.section", &json!({ "section": b.to_string() })).unwrap();
    assert_eq!(sel(&d), Selection::new(pb.clone(), Some(pb[0]), Some(b), true));
    assert_eq!(d.state().folder, Some(fb));
    click(&mut d, pb[3], ctrl());
    assert!(!sel(&d).whole && !sel(&d).ids.contains(&pb[3]), "a gesture makes it a plain selection again");
}

/// Every select command refuses bad parameters with an error, never a panic, and changes nothing.
#[test]
fn every_select_command_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 2, 8, (900, 600), 144.0);
    let s = sections(&d);
    click(&mut d, s[0].1[1], egui::Modifiers::NONE);
    let before = d.state().clone();
    let hostile = [
        json!([1]),
        json!("x"),
        json!(7),
        json!(true),
        json!({ "zzz": 1 }),
        json!({ "section": 3 }),
        json!({ "section": "folder:x" }),
        json!({ "section": "album:1" }),
        json!({ "section": "folder:999999" }),
        json!({ "ids": [] , "mode": "spiral" }),
        json!({ "ids": [999_999] }),
        json!({ "ids": [999_999], "mode": "range" }),
        json!({ "ids": [s[0].1[0].get(), s[0].1[1].get()], "mode": "range" }),
        json!({ "ids": [], "mode": "range" }),
        json!({ "ids": [-1] }),
        json!({ "ids": [u64::MAX] }),
    ];
    let commands: Vec<&str> = UI_COMMANDS.iter().map(|c| c.id).filter(|id| id.starts_with("select.")).collect();
    assert_eq!(commands.len(), 18, "{commands:?}");
    for id in &commands {
        for params in &hostile {
            match d.app_mut().run(id, params) {
                Err(CmdError::BadParams(_) | CmdError::NotFound(_)) => {}
                other => panic!("{id} {params}: {other:?}"),
            }
        }
    }
    let mut after = d.state().clone();
    after.status.clone_from(&before.status);
    assert_eq!(after, before, "nothing changed");

    // Without a library: an error, never a panic.
    let mut bare = App::new(None);
    for id in &commands {
        let params = if *id == "select.set" {
            json!({ "ids": [1] })
        } else if *id == "select.section" {
            json!({ "section": "folder:1" })
        } else {
            json!({})
        };
        let r = bare.run(id, &params);
        assert!(*id == "select.none" || r.is_err(), "{id}: {r:?}");
    }
    // An empty library: the keys say there is nothing to select.
    let empty = tempfile::tempdir().unwrap();
    let s = Session::open(&empty.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    for id in commands.iter().filter(|id| !matches!(**id, "select.set" | "select.none" | "select.section")) {
        assert!(matches!(d.app_mut().run(id, &json!({})), Err(CmdError::Disabled(_))), "{id}");
    }
    let shown: Value = serde_json::to_value(&d.state().selection).unwrap();
    assert_eq!(shown, json!({ "ids": [], "anchor": null, "section": null, "whole": false }));
}

/// The keys are bound in the library on every platform, with no conflict (Cmd on macOS: Cmd+I is
/// Invert Selection there too).
#[test]
fn the_selection_keys_are_bound_on_every_platform() {
    let specs = dalica_engine::cmd::specs();
    for p in Platform::ALL {
        let mut map = Keymap::from_specs_on(&specs, p).unwrap();
        map.add_commands(dalica_ui::keys::ui_keys(), p).unwrap();
        assert_eq!(map.conflicts(), [], "{p:?}");
        for (id, k) in [
            ("select.all", "Ctrl+A"),
            ("select.none", "Ctrl+D"),
            ("select.invert", "Ctrl+I"),
            ("select.left", "Left"),
            ("select.right", "Right"),
            ("select.up", "Up"),
            ("select.down", "Down"),
            ("select.first", "Home"),
            ("select.last", "End"),
            ("select.extendLeft", "Shift+Left"),
            ("select.extendRight", "Shift+Right"),
            ("select.extendUp", "Shift+Up"),
            ("select.extendDown", "Shift+Down"),
            ("select.extendFirst", "Shift+Home"),
            ("select.extendLast", "Shift+End"),
        ] {
            assert_eq!(map.keys_for(id, Context::Library), [Key::parse(k).unwrap()], "{id} on {p:?}");
        }
    }
}
