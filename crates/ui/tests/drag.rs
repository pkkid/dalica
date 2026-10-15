//! Drag and drop of photos (M9.23, `ui-layout.md` §4, §5.5 and §6.2), driven headlessly by widget
//! id on temp folders `Lisbon` (`IMG_0001.jpg` … `IMG_0003.jpg`) and `Porto` (`PORTO_0001.jpg`):
//!
//! - a thumbnail dropped on a tree folder moves it, asking "Move these files on disk?" the first
//!   time; with Ctrl held it copies ("Copy %d files to %s?");
//! - onto a user album's tree row it joins the album; onto the tray it is held; out of the tray
//!   onto an album it joins it too;
//! - dropped between two cells of an album it reorders the album, and the order survives a
//!   restart;
//! - onto its own folder, and with Esc pressed during the drag, nothing changes;
//! - the tree scrolls while a drag is held near its bottom edge;
//! - the drag pill for a move, a copy and an add matches its fingerprints at 1400 × 900
//!   (`tests/fingerprints/drag-{move,copy,add}-1400x900.json`; `DALICA_BLESS=1` stores
//!   new ones).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_region, new_driver, out_dir, put_jpeg, try_widget};
use dalica_core::{AlbumId, FolderId, PhotoId};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::headless::{Drag, KeyState, PointerEvent, PointerKind, Target};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use egui::{Modifiers, PointerButton, Pos2, pos2};
use serde_json::{Value, json};

struct Fx {
    dir: tempfile::TempDir,
    lisbon: PathBuf,
    porto: PathBuf,
    lisbon_id: FolderId,
    porto_id: FolderId,
    d: Driver,
}

fn open(dir: &Path) -> Session {
    Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap()
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let (lisbon, porto) = (dir.path().join("photos").join("Lisbon"), dir.path().join("photos").join("Porto"));
    for i in 1..=3 {
        put_jpeg(&lisbon.join(format!("IMG_000{i}.jpg")), i);
    }
    put_jpeg(&porto.join("PORTO_0001.jpg"), 10);
    let mut s = open(dir.path());
    for f in [&lisbon, &porto] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let (lisbon_id, porto_id) = (s.library().folder_by_path(&lisbon).unwrap(), s.library().folder_by_path(&porto).unwrap());
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, lisbon, porto, lisbon_id, porto_id, d }
}

impl Fx {
    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    /// The photo called `name` in `folder`.
    fn photo(&self, folder: FolderId, name: &str) -> PhotoId {
        self.session().library().photos_in(folder).into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no {name}")).id
    }

    /// A new user album holding `ids`, in that order.
    fn album(&mut self, name: &str, ids: &[PhotoId]) -> AlbumId {
        let reply = self.d.run("album.create", &json!({ "name": name, "ids": ids })).unwrap();
        self.d.settle(FRAMES).unwrap();
        AlbumId::parse(reply["album"].as_str().unwrap()).unwrap()
    }

    fn members(&self, album: &AlbumId) -> Vec<PhotoId> {
        self.session().library().album_members(album).to_vec()
    }

    fn top(&self) -> Option<DialogKind> {
        self.d.state().dialogs.last().map(|d| d.kind)
    }

    /// Clicks `id`, then waits for what it started (file jobs included).
    fn press(&mut self, id: &str) {
        self.d.click(id).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    /// Drags widget `from` onto widget `to` and drops it there, then waits for what that started.
    fn drop_on(&mut self, from: &str, to: &str, modifiers: Modifiers) {
        self.d.drag(from, Target::Widget(to), &Drag { modifiers, ..Drag::default() }).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    /// Presses on widget `from` and moves the pointer to `to` in 12 frames, keeping the button
    /// down.
    fn hold(&mut self, from: &str, to: Pos2) {
        let start = try_widget(&self.d, from).unwrap_or_else(|| panic!("no widget {from}")).center();
        let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
        events.extend((1..=12).map(|i| PointerEvent { kind: PointerKind::Drag, pos: start.lerp(to, i as f32 / 12.0) }));
        self.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    }

    /// Releases the button where the pointer is, then waits.
    fn release(&mut self) {
        let pos = self.d.pointer_pos().unwrap();
        self.d.pointer(&[PointerEvent { kind: PointerKind::Up, pos }], PointerButton::Primary, Modifiers::NONE).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    /// `ui.inspect` → `drag`.
    fn drag(&self) -> Value {
        dalica_ui::drag::inspect(self.d.app())
    }
}

fn center(d: &Driver, id: &str) -> Pos2 {
    try_widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).center()
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().ends_with(".jpg"))
        .collect();
    v.sort();
    v
}

/// The first photo cell of the last frame lying wholly inside the light table, top row first.
fn first_visible_thumb(d: &Driver) -> WidgetInfo {
    let table = try_widget(d, "region:lightTable").expect("the light table").rect;
    let inside = |r: [f32; 4]| r[0] >= table[0] && r[1] >= table[1] && r[0] + r[2] <= table[0] + table[2] && r[1] + r[3] <= table[1] + table[3];
    d.widgets()
        .into_iter()
        .filter(|w| w.id.strip_prefix("thumb:").is_some_and(|n| n.parse::<u64>().is_ok()) && inside(w.rect))
        .min_by(|a, b| (a.rect[1], a.rect[0]).partial_cmp(&(b.rect[1], b.rect[0])).unwrap())
        .expect("a photo cell wholly in view")
}

fn thumb(id: PhotoId) -> String {
    format!("thumb:{}", id.get())
}

/// A thumbnail onto a tree folder moves it, asking the first time; the pill names the folder.
#[test]
fn a_thumbnail_dropped_on_a_folder_moves_it_asking_the_first_time() {
    let mut fx = fixture((1400, 900));
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    let row = format!("tree:folder:{}", fx.porto_id.get());
    let to = center(&fx.d, &row);
    fx.hold(&thumb(one), to);
    assert_eq!(fx.drag(), json!({ "from": thumb(one), "ids": [one], "target": row, "op": "move", "pill": "Move 1 photo to Porto" }));
    assert_eq!(try_widget(&fx.d, "drag:pill").map(|w| w.label), Some("Move 1 photo to Porto".into()));
    fx.release();
    assert_eq!(fx.drag(), Value::Null, "the drag ended");
    assert_eq!(fx.top(), Some(DialogKind::MoveFiles), "the first move asks");
    fx.press("button:dialog.move");
    assert_eq!(files(&fx.lisbon), ["IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "PORTO_0001.jpg"]);
    assert_eq!(fx.session().library().photo(one).unwrap().folder, fx.porto_id, "the photo keeps its id");
    // The question stays on (no "Don't ask again"): it asks again; Cancel moves nothing.
    fx.drop_on(&thumb(two), &row, Modifiers::NONE);
    assert_eq!(fx.top(), Some(DialogKind::MoveFiles));
    fx.press("button:dialog.cancel");
    assert_eq!(files(&fx.lisbon), ["IMG_0002.jpg", "IMG_0003.jpg"]);
    // Several selected photos go together when one of them is dragged.
    fx.d.run("select.set", &json!({ "ids": [two, fx.photo(fx.lisbon_id, "IMG_0003.jpg")] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.drop_on(&thumb(two), &row, Modifiers::NONE);
    fx.d.click("check:dialog.dontAsk").unwrap();
    fx.press("button:dialog.move");
    assert_eq!(files(&fx.lisbon), Vec::<String>::new());
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "PORTO_0001.jpg"]);
}

/// With Ctrl held the drop copies, after "Copy %d files to %s?"; the pill shows +.
#[test]
fn with_ctrl_a_drop_on_a_folder_copies() {
    let mut fx = fixture((1400, 900));
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    let row = format!("tree:folder:{}", fx.porto_id.get());
    fx.drop_on(&thumb(one), &row, Modifiers::COMMAND | Modifiers::CTRL);
    assert_eq!(fx.top(), Some(DialogKind::CopyFiles));
    fx.press("button:dialog.copy");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"], "the original stays");
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "PORTO_0001.jpg"]);
}

/// Onto a user album's row the photo joins the album; onto the tray it is held; a tray mini
/// dragged onto an album row joins it too.
#[test]
fn drops_on_an_album_row_and_the_tray_add_and_hold() {
    let mut fx = fixture((1400, 900));
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    let porto = fx.photo(fx.porto_id, "PORTO_0001.jpg");
    let album = fx.album("Trip", &[porto]);
    let row = format!("tree:album:{album}");
    fx.hold(&thumb(one), center(&fx.d, &row));
    assert_eq!((fx.drag()["op"].clone(), fx.drag()["pill"].clone()), (json!("add"), json!("Add 1 photo to Trip")));
    fx.release();
    assert_eq!(fx.members(&album), [porto, one]);
    assert!(fx.d.state().dialogs.is_empty(), "adding asks nothing");
    // The album holds it now: its row is no target for it any more.
    fx.hold(&thumb(one), center(&fx.d, &row));
    assert_eq!(fx.drag()["target"], Value::Null);
    fx.release();
    assert_eq!(fx.members(&album), [porto, one]);

    // Onto the tray: held.
    fx.hold(&thumb(two), center(&fx.d, "tray"));
    assert_eq!((fx.drag()["target"].clone(), fx.drag()["pill"].clone()), (json!("tray"), json!("Hold 1 photo in the tray")));
    save_window(&fx.d, "drag-hold-1400x900");
    fx.release();
    assert_eq!(fx.d.state().tray.held, [two]);
    // Out of the tray onto the album: added.
    fx.d.run("select.none", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let mini = format!("tray:item:{}", two.get());
    fx.hold(&mini, center(&fx.d, &row));
    assert_eq!(fx.drag()["from"], json!(mini));
    fx.release();
    assert_eq!(fx.members(&album), [porto, one, two]);
}

/// Dropped between two cells of an album the photo moves there in the album's order, and the order
/// survives a restart.
#[test]
fn a_reorder_within_an_album_survives_a_restart() {
    let mut fx = fixture((1400, 900));
    let ids: Vec<PhotoId> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"].iter().map(|n| fx.photo(fx.lisbon_id, n)).collect();
    let album = fx.album("Order", &ids);
    let cell = |id: PhotoId| format!("thumb:{}/in/{album}", id.get());
    // The third photo onto the left half of the first's cell: before it.
    let first = try_widget(&fx.d, &cell(ids[0])).expect("the album's section is on screen");
    let before = pos2(first.rect[0] + first.rect[2] * 0.2, first.rect[1] + first.rect[3] / 2.0);
    fx.hold(&cell(ids[2]), before);
    assert_eq!((fx.drag()["target"].clone(), fx.drag()["op"].clone()), (json!(cell(ids[0])), json!("reorder")));
    assert_eq!(fx.drag()["pill"], "Reorder 1 photo");
    save_window(&fx.d, "drag-reorder-1400x900");
    fx.release();
    let want = vec![ids[2], ids[0], ids[1]];
    assert_eq!(fx.members(&album), want);
    // Beside itself it is no target.
    let own = try_widget(&fx.d, &cell(ids[2])).unwrap();
    fx.hold(&cell(ids[2]), pos2(own.rect[0] + own.rect[2] * 0.8, own.rect[1] + own.rect[3] / 2.0));
    assert_eq!(fx.drag()["target"], Value::Null);
    fx.release();
    assert_eq!(fx.members(&album), want);

    // A restart reads the same order back.
    let Fx { dir, d, .. } = fx;
    let mut app = d.into_app();
    app.take_session().unwrap().flush().unwrap();
    drop(app);
    let s = open(dir.path());
    assert_eq!(s.library().album_members(&album), want.as_slice());
}

/// A drop on the photo's own folder and a drag ended with Esc change nothing.
#[test]
fn onto_its_own_folder_or_with_esc_nothing_changes() {
    let mut fx = fixture((1400, 900));
    let steps = fx.session().undo_stack().len();
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    let own = format!("tree:folder:{}", fx.lisbon_id.get());
    fx.hold(&thumb(one), center(&fx.d, &own));
    assert_eq!((fx.drag()["target"].clone(), fx.drag()["op"].clone(), fx.drag()["pill"].clone()), (Value::Null, Value::Null, json!("1 photo")));
    fx.release();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);

    // Esc over Porto's row: the drag ends; releasing there afterwards drops nothing.
    let row = format!("tree:folder:{}", fx.porto_id.get());
    fx.hold(&thumb(one), center(&fx.d, &row));
    assert_eq!(fx.drag()["target"], json!(row));
    fx.d.key("Esc").unwrap();
    assert_eq!(fx.drag(), Value::Null, "Esc ends the drag");
    assert!(try_widget(&fx.d, "drag:pill").is_none(), "and its pill");
    fx.release();
    assert!(fx.d.state().dialogs.is_empty(), "nothing asked");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["PORTO_0001.jpg"]);
    assert_eq!(fx.session().undo_stack().len(), steps, "nothing ran");
}

/// The tree scrolls while a drag is held near its bottom edge, so a folder off screen is reached.
#[test]
fn the_tree_scrolls_near_its_edge_during_a_drag() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = open(dir.path());
    for k in 0..40 {
        let folder = dir.path().join("photos").join(format!("Folder {k:02}"));
        put_jpeg(&folder.join(format!("F{k:02}.jpg")), 100 + k);
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    let steps = d.app().session().unwrap().undo_stack().len();
    let tree = try_widget(&d, "region:tree").unwrap().rect;
    let info = |d: &Driver| dalica_ui::panels::folders::info(d.ctx());
    assert_eq!(info(&d).offset, 0.0);
    assert!(info(&d).height > info(&d).view_h, "the list is taller than the pane");
    let near_bottom = pos2(tree[0] + tree[2] / 2.0, tree[1] + tree[3] - 4.0);
    // A photo this frame shows: folders sort by their photos' dates (newest first, to the second),
    // so when the files' times cross a second the first photo written lies below the view.
    let start = first_visible_thumb(&d).center();
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
    events.extend((1..=12).map(|i| PointerEvent { kind: PointerKind::Drag, pos: start.lerp(near_bottom, i as f32 / 12.0) }));
    // Held there for a second.
    events.extend((0..60).map(|_| PointerEvent { kind: PointerKind::Drag, pos: near_bottom }));
    d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    let scrolled = info(&d).offset;
    assert!(scrolled > 100.0, "the rows scrolled: {scrolled}");
    // In the middle of the pane it stays.
    let middle = pos2(near_bottom.x, tree[1] + tree[3] / 2.0);
    d.pointer(&[PointerEvent { kind: PointerKind::Drag, pos: middle }; 10], PointerButton::Primary, Modifiers::NONE).unwrap();
    let held = info(&d).offset;
    d.pointer(&[PointerEvent { kind: PointerKind::Drag, pos: middle }; 10], PointerButton::Primary, Modifiers::NONE).unwrap();
    assert_eq!(info(&d).offset, held);
    d.key("Esc").unwrap();
    d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: middle }], PointerButton::Primary, Modifiers::NONE).unwrap();
    assert_eq!(d.app().session().unwrap().undo_stack().len(), steps);
}

/// The scroll speed near the tree's edges: none in the middle, faster nearer the edge, none outside.
#[test]
fn the_tree_drag_scroll_speed_grows_towards_the_edge() {
    use dalica_ui::panels::folders::{DRAG_EDGE, drag_scroll_speed};
    let (top, bottom) = (100.0, 500.0);
    assert_eq!(drag_scroll_speed(300.0, top, bottom), 0.0);
    assert_eq!(drag_scroll_speed(bottom - DRAG_EDGE - 1.0, top, bottom), 0.0);
    let (near, nearer) = (drag_scroll_speed(bottom - DRAG_EDGE + 2.0, top, bottom), drag_scroll_speed(bottom - 1.0, top, bottom));
    assert!(near > 0.0 && nearer > near, "{near} {nearer}");
    assert_eq!(drag_scroll_speed(top + 1.0, top, bottom), -nearer, "the same going up");
    assert_eq!(drag_scroll_speed(bottom + 10.0, top, bottom), 0.0, "outside the pane");
    for bad in [f32::NAN, f32::INFINITY] {
        assert_eq!(drag_scroll_speed(bad, top, bottom), 0.0);
        assert_eq!(drag_scroll_speed(300.0, bad, bottom), 0.0);
    }
}

/// The window as last drawn, to the test output (and to `DALICA_SHOTS`, when set): for a look.
fn save_window(d: &Driver, name: &str) {
    let s = d.screenshot().unwrap();
    let window = fingerprint::png_rgba(s.width, s.height, &s.rgba);
    std::fs::write(out_dir().join(format!("{name}-window.png")), &window).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}-window.png")), &window).unwrap();
    }
}

/// The pill for a move, a copy and an add, over the target it names (one size: the pill does not
/// depend on the window's).
#[test]
fn the_drag_pill_matches_its_fingerprints() {
    let case = format!("{}x{}", LARGE.0, LARGE.1);
    let mut fx = fixture(LARGE);
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    let porto = fx.photo(fx.porto_id, "PORTO_0001.jpg");
    let album = fx.album("Trip", &[porto]);
    let (folder_row, album_row) = (format!("tree:folder:{}", fx.porto_id.get()), format!("tree:album:{album}"));
    fx.d.settle_for(FRAMES, true);

    fx.hold(&thumb(one), center(&fx.d, &folder_row));
    assert_eq!(fx.drag()["pill"], "Move 1 photo to Porto", "{case}");
    check_region(&fx.d, "drag:pill", &format!("drag-move-{case}"));
    // Ctrl held down over the same row: a copy, with the +.
    fx.d.key_raw(egui::Key::F20, Modifiers::COMMAND | Modifiers::CTRL, KeyState::Down);
    assert_eq!(fx.drag()["pill"], "Copy 1 photo to Porto", "{case}");
    check_region(&fx.d, "drag:pill", &format!("drag-copy-{case}"));
    fx.d.key_raw(egui::Key::F20, Modifiers::NONE, KeyState::Up);
    fx.d.key("Esc").unwrap();
    fx.release();

    fx.hold(&thumb(one), center(&fx.d, &album_row));
    assert_eq!(fx.drag()["pill"], "Add 1 photo to Trip", "{case}");
    check_region(&fx.d, "drag:pill", &format!("drag-add-{case}"));
    fx.d.key("Esc").unwrap();
    fx.release();
    assert_eq!(fx.members(&album), [porto], "{case}: Esc dropped nothing");
}

/// While the button is held on a dragged thumbnail, `ui.inspect` reports the drag (from, ids,
/// target, op, pill) and `ui.state` does not; a drag from a selected photo carries the selection
/// and selects nothing.
#[test]
fn a_thumbnail_drag_is_reported_while_the_button_is_held() {
    use dalica_ui::control::{Config, Connection, Control, Request, serve};
    let mut fx = fixture(LARGE);
    let (a, b) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg").get(), fx.photo(fx.lisbon_id, "IMG_0002.jpg").get());
    let control = Control::new(Config::default());
    let mut conn = Connection::new();
    let mut ok = |d: &mut Driver, method: &str, params: Value| {
        serve(&control, &mut conn, d, &Request::new(method, params.clone())).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
    };
    let d = &mut fx.d;
    let mid = |d: &Driver, id: u64| {
        let [x, y, w, h] = try_widget(d, &format!("thumb:{id}")).unwrap().rect;
        (x + w / 2.0, y + h / 2.0)
    };
    let (cx, cy) = mid(d, a);
    assert_eq!(ok(d, "ui.inspect", json!({}))["drag"], Value::Null);
    let events = json!([{ "kind": "down", "x": cx, "y": cy }, { "kind": "drag", "x": cx + 20.0, "y": cy + 4.0 }, { "kind": "drag", "x": cx + 60.0, "y": cy + 10.0 }]);
    ok(d, "ui.pointer", json!({ "events": events }));
    let drag = ok(d, "ui.inspect", json!({}))["drag"].clone();
    assert_eq!(drag, json!({ "from": format!("thumb:{a}"), "ids": [a], "target": null, "op": null, "pill": "1 photo" }));
    assert!(ok(d, "ui.state", json!({})).get("drag").is_none());
    ok(d, "ui.pointer", json!({ "events": [{ "kind": "up", "x": cx + 60.0, "y": cy + 10.0 }] }));
    assert_eq!(ok(d, "ui.inspect", json!({}))["drag"], Value::Null);
    // From a selected photo: the selection.
    d.run("select.set", &json!({ "ids": [a, b] })).unwrap();
    d.settle(FRAMES).unwrap();
    let (cx, cy) = mid(d, b);
    let events =
        json!([{ "kind": "down", "x": cx, "y": cy }, { "kind": "drag", "x": cx + 30.0, "y": cy }, { "kind": "drag", "x": cx + 80.0, "y": cy }]);
    ok(d, "ui.pointer", json!({ "events": events }));
    assert_eq!(ok(d, "ui.inspect", json!({}))["drag"]["ids"], json!([a, b]));
    ok(d, "ui.pointer", json!({ "events": [{ "kind": "up", "x": cx + 80.0, "y": cy }] }));
    assert_eq!(ok(d, "ui.inspect", json!({}))["drag"], Value::Null);
    assert_eq!(ok(d, "ui.state", json!({}))["selection"]["ids"], json!([a, b]), "a drag selects nothing");
}
