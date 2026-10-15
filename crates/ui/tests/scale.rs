//! Display scales (`design-system.md` §11): five flows that depend on hit-testing and on rounding
//! points to pixels, each run at 1, 1.5 and 2 pixels per point in a 1400 × 900 point window (so
//! 2800 × 1800 pixels at 2), with the same outcome at every scale: a crop drawn and a corner
//! dragged on the photo, the straighten slider dragged to +3.5° and applied, a thumbnail dragged
//! onto the tray, the tree splitter dragged to each of its limits, and a dialog's buttons clicked
//! (the tray's Clear question: Cancel keeps the tray, Clear Tray empties it).

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, new_driver, put_jpeg, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::headless::{Drag, PointerEvent, PointerKind, Target};
use dalica_ui::panels::edit::{canvas, crop, straighten};
use dalica_ui::{App, Driver, DriverError, ScriptedPicker};
use egui::{Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};

/// The display scales every flow runs at.
const SCALES: [f32; 3] = [1.0, 1.5, 2.0];
/// The window in points, at every scale.
const POINTS: (f32, f32) = (1400.0, 900.0);

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    /// The photos of folder `Lisbon` (600 × 400), in name order.
    ids: Vec<PhotoId>,
    scale: f32,
}

/// A library of folders `Lisbon` (three 600 × 400 photos) and `Porto` (one), in a 1400 × 900
/// point window at `scale`.
fn setup(scale: f32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (lisbon, porto) = (dir.path().join("photos").join("Lisbon"), dir.path().join("photos").join("Porto"));
    for i in 1..=3u64 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i, 600, 400), 90, None).unwrap();
        std::fs::create_dir_all(&lisbon).unwrap();
        std::fs::write(lisbon.join(format!("IMG_000{i}.jpg")), bytes).unwrap();
    }
    put_jpeg(&porto.join("PORTO_0001.jpg"), 10);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in [&lisbon, &porto] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let folder = s.library().folder_by_path(&lisbon).unwrap();
    let mut named: Vec<(String, PhotoId)> = s.library().photos_in(folder).into_iter().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    let (w, h) = ((POINTS.0 * scale).round() as u32, (POINTS.1 * scale).round() as u32);
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), w, h, scale);
    settle_driver(&mut d);
    let size = d.ctx().content_rect().size();
    assert_eq!((size.x, size.y), POINTS, "scale {scale}: the same window in points");
    T { _dir: dir, d, ids: named.into_iter().map(|(_, id)| id).collect(), scale }
}

/// Frames until nothing is pending, the edit view's preview awaited in real time (a loaded
/// machine's slow render is not a hang; `straighten_tool.rs` does the same).
fn settle_driver(d: &mut Driver) {
    for _ in 0..10 {
        if d.app().session().map_or(0, Session::preview_busy) > 0 {
            assert!(d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(60)), "the preview did not finish within 60 s");
        }
        match d.settle(FRAMES) {
            Ok(_) => return,
            Err(DriverError::NotSettled { busy, .. }) => {
                let preview_only = busy.iter().all(|b| b.starts_with("edit preview: ") || b == "animation running");
                assert!(preview_only, "not settled: {busy:?}");
            }
            Err(e) => panic!("{e}"),
        }
    }
    panic!("not settled after ten waits for the preview");
}

impl T {
    fn settle(&mut self) {
        settle_driver(&mut self.d);
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        let v = self.d.run(id, &params).unwrap_or_else(|e| panic!("scale {}: {id} {params}: {e}", self.scale));
        self.settle();
        v
    }

    fn click(&mut self, id: &str) {
        self.d.click(id).unwrap_or_else(|e| panic!("scale {}: click {id}: {e}", self.scale));
        self.settle();
    }

    fn rect(&self, id: &str) -> Rect {
        let [x, y, w, h] = widget(&self.d, id).rect;
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    /// A press at `from`, moved in eight steps to `to`, released there (screen points).
    fn gesture(&mut self, from: Pos2, to: Pos2, release: bool) {
        let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: from }, PointerEvent { kind: PointerKind::Down, pos: from }];
        events.extend((1..=8).map(|i| PointerEvent { kind: PointerKind::Drag, pos: from.lerp(to, i as f32 / 8.0) }));
        if release {
            events.push(PointerEvent { kind: PointerKind::Up, pos: to });
        }
        self.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
        self.settle();
    }

    /// The edit view on the first photo.
    fn edit_first(&mut self) {
        let id = self.ids[0].get();
        self.run("view.edit", json!({ "id": id }));
    }

    /// A screen point at `(x, y)` normalised to the photo shown.
    fn at(&self, x: f32, y: f32) -> Pos2 {
        let i = canvas::view(self.d.ctx()).expect("the canvas was drawn").image;
        pos2(i.min.x + x * i.width(), i.min.y + y * i.height())
    }

    fn crop_rect(&self) -> [f32; 4] {
        self.d.state().edit.crop.as_ref().and_then(|c| c.rect).expect("a crop rectangle")
    }

    fn edits(&mut self) -> Value {
        let id = self.ids[0].get();
        self.run("photo.history.state", json!({ "id": id }))["edits"].clone()
    }
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// A crop drawn on the photo spans what the pointer spanned, and a corner dragged lands where it
/// was dropped while the opposite corner stays, at every scale.
#[test]
fn a_crop_drag_lands_where_the_pointer_does_at_every_scale() {
    for scale in SCALES {
        let mut t = setup(scale);
        t.edit_first();
        t.click("tool:crop");
        assert_eq!(t.d.state().edit.crop.as_ref().map(|c| c.ratio.as_str()), Some("manual"));
        let (a, b) = (t.at(0.1, 0.2), t.at(0.7, 0.5));
        t.gesture(a, b, true);
        let [x, y, w, h] = t.crop_rect();
        assert!(near(x, 0.1, 0.005) && near(y, 0.2, 0.005) && near(w, 0.6, 0.005) && near(h, 0.3, 0.005), "scale {scale}: drawn {:?}", t.crop_rect());
        // The bottom-right corner, by the rectangle's own widget.
        let corner = t.rect(crop::RECT_ID).right_bottom();
        let to = t.at(0.9, 0.85);
        t.gesture(corner, to, true);
        let r = t.crop_rect();
        assert!(near(r[0], x, 1e-3) && near(r[1], y, 1e-3), "scale {scale}: the opposite corner stays: {r:?}");
        assert!(near(r[0] + r[2], 0.9, 0.005) && near(r[1] + r[3], 0.85, 0.005), "scale {scale}: the corner follows: {r:?}");
    }
}

/// The straighten slider dragged to +3.5° reads +3.5° (its 0.1° steps survive the rounding), and
/// Apply, clicked, stores exactly that, at every scale.
#[test]
fn a_straighten_drag_reads_the_same_angle_at_every_scale() {
    for scale in SCALES {
        let mut t = setup(scale);
        // The floating bar takes clicks without an automation id (`straighten_tool.rs`'s
        // TODO(maintainer)); every other widget is still checked by the other flows.
        t.d.check_ids(false);
        t.edit_first();
        t.click("tool:straighten");
        let track = t.rect(straighten::SLIDER_ID);
        let ring = t.d.app().tokens().metrics.slider_ring;
        let (x0, x1, y) = (track.min.x + ring + 1.0, track.max.x - ring - 1.0, track.center().y);
        let from = pos2((x0 + x1) / 2.0, y);
        let to = pos2(from.x + 3.5 / (2.0 * dalica_ui::state::STRAIGHTEN_MAX) * (x1 - x0), y);
        t.gesture(from, to, true);
        assert_eq!(t.d.state().edit.straighten.as_ref().map(|s| s.angle), Some(3.5), "scale {scale}");
        t.click(straighten::APPLY_ID);
        assert_eq!(t.edits(), json!(["straighten(3.5)"]), "scale {scale}");
    }
}

/// A thumbnail dragged onto the tray is held there, at every scale.
#[test]
fn a_tray_drag_holds_the_photo_at_every_scale() {
    for scale in SCALES {
        let mut t = setup(scale);
        let id = t.ids[1];
        t.d.drag(&format!("thumb:{}", id.get()), Target::Widget("tray"), &Drag::default()).unwrap();
        t.settle();
        assert_eq!(t.d.state().tray.held, [id], "scale {scale}");
        assert!(dalica_ui::drag::inspect(t.d.app()).is_null(), "scale {scale}: the drag ended");
    }
}

/// The tree splitter moves the tree by what it is dragged, and stops at 200 and at W − 480, at
/// every scale.
#[test]
fn the_tree_splitter_drags_to_its_limits_at_every_scale() {
    for scale in SCALES {
        let mut t = setup(scale);
        t.d.app_mut().state_mut().tree_width = 240.0;
        t.settle();
        for (dx, expected) in [(60.0, 300.0), (-500.0, 200.0), (2000.0, POINTS.0 - 480.0)] {
            t.d.drag("splitter:tree", Target::Delta(vec2(dx, 0.0)), &Drag::default()).unwrap();
            t.settle();
            assert!(near(t.d.state().tree_width, expected, 0.5), "scale {scale}, drag {dx}: {}", t.d.state().tree_width);
            let tree = t.rect("region:tree");
            assert!(near(tree.width(), expected, 1.0), "scale {scale}, drag {dx}: the tree region is {tree:?}");
        }
    }
}

/// A dialog's buttons lie inside it without overlapping, and each click lands on the button it
/// was aimed at: Cancel keeps the tray, Clear Tray empties it, at every scale.
#[test]
fn a_dialogs_buttons_take_their_own_clicks_at_every_scale() {
    for scale in SCALES {
        let mut t = setup(scale);
        let ids: Vec<u64> = t.ids[..2].iter().map(|p| p.get()).collect();
        t.run("select.set", json!({ "ids": ids }));
        t.click("button:tray.hold");
        t.run("select.none", json!({}));
        assert_eq!(t.d.state().tray.held.len(), 2, "scale {scale}");
        for (button, held) in [("button:dialog.cancel", 2), ("button:dialog.clearTray", 0)] {
            t.click("button:tray.clear");
            let dialog = t.rect("dialog:clearTray");
            let (ok, cancel) = (t.rect("button:dialog.clearTray"), t.rect("button:dialog.cancel"));
            assert!(dialog.contains_rect(ok) && dialog.contains_rect(cancel), "scale {scale}: {ok:?} and {cancel:?} in {dialog:?}");
            assert!(!ok.intersects(cancel), "scale {scale}: {ok:?} overlaps {cancel:?}");
            t.click(button);
            assert!(t.d.state().dialogs.is_empty(), "scale {scale}: {button} closes the dialog");
            assert_eq!(t.d.state().tray.held.len(), held, "scale {scale}: after {button}");
        }
    }
}
