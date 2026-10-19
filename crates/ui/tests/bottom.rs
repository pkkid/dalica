//! The bottom area (M3.9, `ui-layout.md` §6.2–§6.5): every id of `control-protocol.md` §7's tray,
//! bottom-control and action-button rows is registered at its §6 size, in its region, at 1400 × 900
//! and 900 × 600; below 1100 wide the action buttons are in the More… menu; the thumbnail-size
//! slider changes the light table's cell size (the state Ctrl+1 / Ctrl+2 set); the pane toggles open,
//! switch and close the side pane. Sizes are written out here from the spec, not taken from the
//! panel, so a change to either shows.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, widget};
use dalica_engine::Session;
use dalica_ui::headless::{Drag, Target};
use dalica_ui::state::Pane;
use dalica_ui::{App, Driver, DriverError, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::Role;

const SIZES: [(f32, f32); 2] = [(1400.0, 900.0), (900.0, 600.0)];
const ACTIONS: [&str; 3] = ["action:export", "action:collage", "action:slideshow"];
const TOGGLES: [&str; 4] = ["toggle:pane.people", "toggle:pane.places", "toggle:pane.tags", "toggle:pane.properties"];

/// A library in `root` with two folders of scenes photos, added and scanned.
fn session(root: &Path) -> Session {
    let files = crate::common::demo_library(2, 24, 3).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(root: &Path, (w, h): (f32, f32)) -> Driver {
    let mut d = new_driver(App::with_picker(Some(session(root)), Arc::new(ScriptedPicker::new())), w as u32, h as u32, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn widgets(d: &Driver) -> HashMap<String, WidgetInfo> {
    d.widgets().into_iter().map(|w| (w.id.clone(), w)).collect()
}

fn size(w: &WidgetInfo) -> [f32; 2] {
    [w.rect[2], w.rect[3]]
}

fn inside(r: [f32; 4], o: [f32; 4]) -> bool {
    r[0] >= o[0] - 0.01 && r[1] >= o[1] - 0.01 && r[0] + r[2] <= o[0] + o[2] + 0.01 && r[1] + r[3] <= o[1] + o[3] + 0.01
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// The edge of the largest thumbnail cell on screen (a cell clipped by the light table's edge is
/// smaller; the ones wholly on screen are the cell size).
fn cell_edge(d: &Driver) -> f32 {
    d.widgets().iter().filter(|w| w.role == Role::Thumbnail).map(|w| w.rect[2].max(w.rect[3])).fold(0.0, f32::max)
}

/// The M3.9 done-when, first part: every bottom-area id with its §6 size at both sizes.
#[test]
fn every_bottom_area_id_is_registered_at_its_spec_size_at_both_window_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    for (w, h) in SIZES {
        let case = format!("{w}×{h}");
        d.resize(w as u32, h as u32);
        d.settle(FRAMES).unwrap();
        let all = widgets(&d);
        let region = |name: &str| all.get(&format!("region:{name}")).unwrap_or_else(|| panic!("{case}: no region:{name}")).rect;
        let split = 0.365 * w;

        // The tray box: x 6 … 0.365·W − 56, 80 high, centred in the 96 pt area.
        let tray = widget(&d, "tray");
        assert_eq!(tray.rect, [6.0, h - 88.0, split - 56.0 - 6.0, 80.0], "{case}: the tray box");
        // Hold, Clear, Add to Album: 34 × 24, stacked top to bottom in the tray-button column, disabled.
        let mut last_bottom = f32::MIN;
        for id in ["button:tray.hold", "button:tray.clear", "button:tray.addToAlbum"] {
            let b = widget(&d, id);
            assert_eq!(size(&b), [34.0, 24.0], "{case}: `{id}`");
            assert!(inside(b.rect, region("trayButtons")), "{case}: `{id}` at {:?} is outside the tray buttons", b.rect);
            assert!(b.rect[1] >= last_bottom, "{case}: `{id}` is under the one before it");
            assert!(!b.enabled, "{case}: `{id}` waits for the tray (M4)");
            last_bottom = b.rect[1] + b.rect[3];
        }

        // The controls row: star and the rotations 36 × 24 at its left, in that order.
        let controls = region("bottomControls");
        let mut last_right = controls[0];
        for id in ["button:star", "button:rotate.ccw", "button:rotate.cw"] {
            let b = widget(&d, id);
            assert_eq!(size(&b), [36.0, 24.0], "{case}: `{id}`");
            assert!(inside(b.rect, controls), "{case}: `{id}` at {:?} is outside the controls row", b.rect);
            assert!(b.rect[0] >= last_right, "{case}: `{id}` is right of the one before it");
            last_right = b.rect[0] + b.rect[2];
        }
        assert!(!widget(&d, "button:star").enabled, "{case}: nothing selected, nothing to star");
        for id in ["button:rotate.ccw", "button:rotate.cw"] {
            assert!(!widget(&d, id).enabled, "{case}: `{id}` waits for its command");
        }

        // At its right: the loupe, the 128 pt size slider, then the four joined 32 × 24 pane toggles.
        let loupe = widget(&d, "button:loupe");
        let slider = widget(&d, "slider:thumbSize");
        assert_eq!(size(&slider), [128.0, 20.0], "{case}: the thumbnail-size slider");
        assert!(slider.enabled);
        assert!(loupe.rect[0] + loupe.rect[2] <= slider.rect[0] && loupe.rect[0] > last_right, "{case}: the loupe is left of the slider");
        let mut x = slider.rect[0] + slider.rect[2];
        for (i, id) in TOGGLES.iter().enumerate() {
            let t = widget(&d, id);
            assert_eq!(size(&t), [32.0, 24.0], "{case}: `{id}`");
            assert_eq!(t.role, Role::Toggle, "{case}: `{id}`");
            assert!(t.enabled && !t.active, "{case}: `{id}` is live and off (no pane open)");
            if i == 0 {
                assert!(t.rect[0] >= x, "{case}: the toggles are right of the slider");
            } else {
                assert!(near(t.rect[0], x), "{case}: `{id}` is joined to the toggle before it");
            }
            x = t.rect[0] + t.rect[2];
        }
        assert!(x <= controls[0] + controls[2], "{case}: the toggles end inside the row");
        for id in ["button:loupe", "slider:thumbSize"].iter().chain(&TOGGLES) {
            assert!(inside(widget(&d, id).rect, controls), "{case}: `{id}` is in the controls row");
        }

        // The action row: 64 × 56 buttons; below 1100 wide only More…, which holds them.
        let row = region("actionButtons");
        let shown: Vec<&str> = ACTIONS.iter().chain(&["action:more"]).copied().filter(|id| all.contains_key(*id)).collect();
        if w >= 1100.0 {
            assert_eq!(shown, ACTIONS, "{case}: the action buttons, no More…");
        } else {
            assert_eq!(shown, ["action:more"], "{case}: only More…");
        }
        for id in shown {
            let b = widget(&d, id);
            assert_eq!(size(&b), [64.0, 56.0], "{case}: `{id}`");
            assert!(inside(b.rect, row), "{case}: `{id}` at {:?} is outside the action row", b.rect);
            assert_eq!(b.enabled, id != "action:export", "{case}: `{id}` is live once its command exists (Export needs a tray)");
        }
    }
}

/// The M3.9 done-when, second part: at 900 × 600, More… opens a vertical menu holding the action
/// buttons, above it, under the same ids; a click outside closes it.
#[test]
fn more_holds_the_action_buttons_at_900_by_600() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[1]);
    assert!(!widget(&d, "action:more").active);
    d.click("action:more").unwrap();
    d.settle(FRAMES).unwrap();
    let more = widget(&d, "action:more");
    assert!(more.active, "More… shows that its menu is open");
    let mut last_bottom = f32::MIN;
    for id in ACTIONS {
        let item = widget(&d, id);
        assert_eq!(item.role, Role::MenuItem, "`{id}` is a menu row");
        assert!(item.rect[1] + item.rect[3] <= more.rect[1] + 0.01, "`{id}` at {:?} is above More… at {:?}", item.rect, more.rect);
        assert!(item.rect[1] >= last_bottom - 0.01, "`{id}` is under the row before it: a vertical menu");
        assert!(item.rect[0] >= 0.0 && item.rect[0] + item.rect[2] <= 900.0, "`{id}` is inside the window");
        assert_eq!(
            item.enabled,
            id != "action:export",
            "`{id}` is live (Slideshow plays, M11.19; Collage opens the room, M14.16); Export needs a tray"
        );
        last_bottom = item.rect[1] + item.rect[3];
    }
    assert!(matches!(d.click("action:export"), Err(DriverError::Disabled(_))));

    // A click elsewhere closes the menu; its rows are gone.
    d.click("slider:thumbSize").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(ACTIONS.iter().all(|id| !widgets(&d).contains_key(*id)), "the menu closed");
    assert!(!widget(&d, "action:more").active);

    // Widened past 1100, the buttons are back on the row and More… is gone.
    d.resize(1100, 700);
    d.settle(FRAMES).unwrap();
    let all = widgets(&d);
    assert!(ACTIONS.iter().all(|id| all.get(*id).is_some_and(|w| w.role == Role::Button)) && !all.contains_key("action:more"));
}

/// The M3.9 done-when, third part: the size slider changes the light table's cell size, through
/// the same view state Ctrl+1 / Ctrl+2 set.
#[test]
fn the_size_slider_changes_the_light_tables_cell_size() {
    let (w, h) = SIZES[0];
    let case = format!("{w}×{h}");
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (w, h));
    assert_eq!((d.state().thumb_size, cell_edge(&d)), (144.0, 144.0), "{case}: the default cell");

    // To the left end: the smallest cells.
    d.drag("slider:thumbSize", Target::Delta(egui::vec2(-200.0, 0.0)), &Drag::default()).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().thumb_size, cell_edge(&d)), (64.0, 64.0), "{case}: dragged to the left end");

    // To the right end: the largest.
    d.drag("slider:thumbSize", Target::Delta(egui::vec2(200.0, 0.0)), &Drag::default()).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().thumb_size, 320.0, "{case}: dragged to the right end");
    assert_eq!(cell_edge(&d), 320.0, "{case}");

    // A click on the middle of the track: halfway, on a whole point.
    d.click("slider:thumbSize").unwrap();
    d.settle(FRAMES).unwrap();
    let mid = d.state().thumb_size;
    assert!((190.0..=194.0).contains(&mid) && mid.fract() == 0.0, "{case}: the middle is {mid}");
    assert_eq!(cell_edge(&d), mid, "{case}");

    // Ctrl+1 / Ctrl+2 set the same state; the slider follows, and drags on from there.
    d.key("Ctrl+1").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().thumb_size, cell_edge(&d)), (96.0, 96.0), "{case}: Ctrl+1");
    d.key("Ctrl+2").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().thumb_size, cell_edge(&d)), (144.0, 144.0), "{case}: Ctrl+2");
    let slider = widget(&d, "slider:thumbSize");
    // 144 sits at (144 − 64) / (320 − 64) of the track (inset by the thumb's half-width + 1).
    let x0 = slider.rect[0] + 2.5;
    let at_144 = x0 + (144.0 - 64.0) / 256.0 * (slider.rect[2] - 5.0);
    d.drag("slider:thumbSize", Target::Point(egui::pos2(at_144, slider.center().y)), &Drag::default()).unwrap();
    d.settle(FRAMES).unwrap();
    assert!((d.state().thumb_size - 144.0).abs() <= 1.0, "{case}: the thumb sits at 144: {}", d.state().thumb_size);
}

/// The pane toggles open their pane, switch to another, and close the open one, the state
/// `pane.toggle` (Ctrl+0) flips; the open pane's toggle is active.
#[test]
fn the_pane_toggles_open_switch_and_close_the_side_pane() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    let active = |d: &Driver| TOGGLES.iter().copied().filter(|id| widget(d, id).active).collect::<Vec<_>>();
    assert_eq!(active(&d), Vec::<&str>::new());

    d.click("toggle:pane.tags").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().pane, active(&d)), (Some(Pane::Tags), vec!["toggle:pane.tags"]));
    assert!(widgets(&d).contains_key("region:sidePane"), "the side pane is shown");

    d.click("toggle:pane.people").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().pane, active(&d)), (Some(Pane::People), vec!["toggle:pane.people"]), "another toggle switches panes");

    d.click("toggle:pane.people").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().pane, active(&d)), (None, vec![]), "the open pane's toggle closes it");
    assert!(!widgets(&d).contains_key("region:sidePane"));

    d.key("Ctrl+0").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().pane, active(&d)), (Some(Pane::Properties), vec!["toggle:pane.properties"]), "Ctrl+0 shows on the toggles");
    d.click("toggle:pane.properties").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().pane, None);
}

/// The star button acts on the selection (until the tray, M4); the buttons whose commands do not
/// exist yet refuse clicks without changing anything.
#[test]
fn the_star_acts_on_the_selection_and_the_waiting_buttons_refuse_clicks() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    // Slideshow plays a slideshow (M11.19, `slideshow.rs`); Collage opens the room (M14.16).
    for id in ["button:tray.hold", "button:tray.clear", "button:tray.addToAlbum", "button:rotate.ccw", "button:rotate.cw", "button:loupe"]
        .iter()
        .chain(ACTIONS.iter().filter(|id| !["action:slideshow", "action:collage"].contains(*id)))
    {
        assert!(matches!(d.click(id), Err(DriverError::Disabled(_))), "`{id}` refuses a click");
    }
    let before = d.state().clone();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state(), &before, "nothing changed");

    let first = d.widgets().into_iter().find(|w| w.role == Role::Thumbnail).unwrap().id;
    d.click(&first).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("button:star").unwrap();
    d.settle(FRAMES).unwrap();
    // Starred, it shows first in Starred Photos (and Recently Updated), above its folder.
    assert!(widget(&d, "button:star").active && widgets(&d).contains_key(&format!("{first}/in/starred/star")), "the selected photo is starred");
    d.click("button:star").unwrap();
    d.settle(FRAMES).unwrap();
    let w = widgets(&d);
    assert!(
        !widget(&d, "button:star").active
            && w.contains_key(&format!("{first}/in/recent"))
            && !w.keys().any(|k| k.starts_with(&format!("{first}/")) && k.ends_with("/star"))
    );
}

/// A thumbnail size outside the range in the view state (a damaged settings file; `ui.set` refuses
/// one) does not break the slider: it is drawn, and a drag pulls the size back into its range.
#[test]
fn a_hostile_thumbnail_size_does_not_break_the_slider() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[1]);
    for bad in [-5.0, 0.0, 1000.0] {
        d.app_mut().state_mut().thumb_size = bad;
        d.step();
        assert!(widgets(&d).contains_key("slider:thumbSize"), "{bad}: the slider is drawn");
        d.drag("slider:thumbSize", Target::Delta(egui::vec2(-300.0, 0.0)), &Drag::default()).unwrap();
        assert_eq!(d.state().thumb_size, 64.0, "{bad}: dragged to the left end");
    }
}

/// M8.20: Export (the UI command `output.export`) is live while the tray holds photos, on the row
/// at 1400 × 900 and in More… at 900 × 600, and opens the Export dialog for the tray's photos.
#[test]
fn export_is_live_with_a_tray_and_opens_the_export_dialog() {
    for (w, h) in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(dir.path(), (w, h));
        let narrow = w < 1100.0;
        let open_more = |d: &mut Driver| {
            if narrow {
                d.click("action:more").unwrap();
                d.settle(FRAMES).unwrap();
            }
        };
        open_more(&mut d);
        assert!(!widget(&d, "action:export").enabled, "{w}: an empty tray, nothing to export");
        if narrow {
            d.key("Esc").unwrap();
            d.settle(FRAMES).unwrap();
        }
        let first = d.widgets().into_iter().find(|w| w.role == Role::Thumbnail).unwrap().id;
        d.click(&first).unwrap();
        d.settle(FRAMES).unwrap();
        let tray = d.state().selection.ids.clone();
        assert_eq!(tray.len(), 1);
        open_more(&mut d);
        assert!(widget(&d, "action:export").enabled, "{w}: live with a tray");
        assert!(widget(&d, "action:collage").enabled && widget(&d, "action:slideshow").enabled, "{w}: Collage and Slideshow are live");
        d.click("action:export").unwrap();
        d.settle(FRAMES).unwrap();
        assert!(widgets(&d).contains_key("dialog:export"), "{w}: the Export dialog");
        assert_eq!(d.state().export.as_ref().map(|e| e.ids.clone()), Some(tray), "{w}: for the tray's photos");
    }
}
