//! The menus (M3.3): `ui.menu.tree` lists every item of `menus-and-shortcuts.md` §1 in order,
//! `ui.menu.invoke` runs, refuses or opens what a click would, and menus drawn at 900 × 600 stay
//! inside the window and scroll when they are taller than it (`design-system.md` §11.2).
//!
//! Renders go to `target/test-output/ui/` (`menus-*.png`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::menus;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::registry;
use dalica_ui_kit::{Image, Rasterizer};
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, vec2};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;

// ---------------------------------------------------------------------------------------------
// `ui.menu.tree` against §1
// ---------------------------------------------------------------------------------------------

/// §1 transcribed for Windows and Linux: per menu, its rows top to bottom; `—` a separator; a
/// submenu's rows follow it, prefixed with its label and `▸ `.
fn expected() -> Vec<(&'static str, Vec<String>)> {
    fn sub(label: &str, rows: &[&str]) -> Vec<String> {
        let mut out = vec![format!("{label} ▸")];
        out.extend(rows.iter().map(|r| format!("{label} ▸ {r}")));
        out
    }
    fn rows(parts: Vec<Vec<String>>) -> Vec<String> {
        parts.concat()
    }
    let s = |items: &[&str]| items.iter().map(|i| i.to_string()).collect::<Vec<_>>();
    let colours = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Black and White"];
    let mut experimental = vec!["Experimental ▸".to_string(), "Experimental ▸ Show Duplicate Files".to_string()];
    experimental.extend(sub("Search for", &colours).into_iter().map(|r| format!("Experimental ▸ {r}")));
    experimental.extend(
        s(&["Save Search Results…", "Show Tag as Album…", "Write People Names to Photos…"]).into_iter().map(|r| format!("Experimental ▸ {r}")),
    );
    vec![
        (
            "File",
            s(&[
                "New Album…",
                "—",
                "Add Folder…",
                "Open File…",
                "Import From…",
                "—",
                "Open in External Editor",
                "—",
                "Move to New Folder…",
                "Rename…",
                "—",
                "Save",
                "Revert…",
                "—",
                "Save As…",
                "Save a Copy",
                "Export Pictures to Folder…",
                "—",
                "Locate on Disk",
                "Delete from Disk…",
                "—",
                "Exit",
            ]),
        ),
        (
            "Edit",
            s(&[
                "Undo",
                "Redo",
                "—",
                "Cut",
                "Copy",
                "Paste",
                "—",
                "Copy All Edits",
                "Paste All Edits",
                "—",
                "Select All",
                "Select Starred",
                "Invert Selection",
                "Select None",
            ]),
        ),
        (
            "View",
            rows(vec![
                s(&[
                    "Library View",
                    "—",
                    "Small Thumbnails",
                    "Normal Thumbnails",
                    "Edit View",
                    "—",
                    "Properties",
                    "Tags",
                    "People",
                    "Places",
                    "—",
                    "Show Edit Controls",
                    "—",
                    "Slideshow",
                    "—",
                    "Small Pictures",
                    "Hidden Pictures",
                    "—",
                    "Use Color Management",
                    "—",
                ]),
                sub("Thumbnail Caption", &["None", "Filename", "Caption", "Tags", "Resolution", "Date"]),
                sub("Folder View", &["Flat Folder View", "Tree View", "—", "Simplified Tree View"]),
                s(&["—", "Full Screen"]),
            ]),
        ),
        (
            "Folder",
            rows(vec![
                s(&["Edit Description…", "View Slideshow", "—", "Refresh Thumbnails"]),
                sub("Sort By", &["Date", "Name", "Size", "Reverse Order"]),
                s(&["—", "Hide", "Unhide", "—", "Locate on Disk", "Remove from Dalica…", "—", "Move…", "Delete…"]),
            ]),
        ),
        (
            "Picture",
            rows(vec![
                s(&["View and Edit"]),
                sub(
                    "Batch Edit",
                    &[
                        "Rename…",
                        "—",
                        "Rotate Clockwise",
                        "Rotate Counterclockwise",
                        "—",
                        "Auto Contrast",
                        "Auto Color",
                        "Auto Fix",
                        "—",
                        "Sepia",
                        "Sharpen",
                        "Warmify",
                        "Film Grain",
                        "Black and White",
                    ],
                ),
                s(&[
                    "—",
                    "Undo All Edits…",
                    "—",
                    "Hide",
                    "Unhide",
                    "—",
                    "Find Faces Again",
                    "—",
                    "Properties",
                    "—",
                    "Star / Unstar",
                    "Edit Caption",
                    "Rotate Clockwise",
                    "Rotate Counterclockwise",
                    "Flip Horizontal",
                    "Flip Vertical",
                ]),
            ]),
        ),
        ("Create", s(&["Picture Collage…"])),
        (
            "Tools",
            rows(vec![
                s(&["Folder Manager…", "People Manager…", "—", "Configure Photo Viewer…", "—", "Adjust Date and Time…", "—"]),
                sub("Geotag", &["Clear Location…", "Copy Location", "Paste Location"]),
                experimental,
                s(&["—", "Options…"]),
            ]),
        ),
        (
            "Help",
            s(&[
                "Help Contents",
                "Keyboard Shortcuts…",
                "—",
                "Release Notes",
                "Report a Problem…",
                "Show Log Folder",
                "Rebuild Library…",
                "—",
                "About Dalica",
            ]),
        ),
    ]
}

/// The same for this platform: macOS has its application menu first and takes Exit, Options… and
/// About Dalica (with the separators before them) out of File, Tools and Help.
fn expected_here() -> Vec<(&'static str, Vec<String>)> {
    let mut menus = expected();
    if cfg!(target_os = "macos") {
        for (_, rows) in &mut menus {
            if let Some(last) = rows.last()
                && ["Exit", "Options…", "About Dalica"].contains(&last.as_str())
            {
                rows.pop();
                rows.pop();
            }
        }
        let app = ["About Dalica", "—", "Preferences…", "—", "Hide Dalica", "Quit Dalica"].iter().map(|s| s.to_string()).collect();
        menus.insert(0, ("Dalica", app));
    }
    menus
}

/// A menu's rows as [`expected`] writes them.
fn flatten(children: &Value, prefix: &str, out: &mut Vec<String>) {
    for row in children.as_array().unwrap() {
        if row["separator"] == true {
            out.push(format!("{prefix}—"));
        } else if let Some(kids) = row.get("children") {
            let label = row["label"].as_str().unwrap();
            out.push(format!("{prefix}{label} ▸"));
            flatten(kids, &format!("{prefix}{label} ▸ "), out);
        } else {
            out.push(format!("{prefix}{}", row["label"].as_str().unwrap()));
        }
    }
}

/// A running app on a library of one folder of `n` scenes photos, driven headlessly, with a
/// control channel.
struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    ids: Vec<u64>,
    extra: PathBuf,
}

fn setup(n: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, n.max(1), 3).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10000 })).unwrap();
    let ids = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let extra = dir.path().join("extra");
    let control = Control::new(Config { read_roots: vec![extra.clone()], ..Config::default() });
    let mut d = Driver::new(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, ids, extra }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

/// The M3.3 done-when: walking `ui.menu.tree` finds every item of §1, in order.
#[test]
fn the_menu_tree_lists_every_item_of_the_spec_in_order() {
    let mut t = setup(1);
    let tree = ok(&mut t, "ui.menu.tree", json!({}));
    let menus = tree.as_array().unwrap();
    let want = expected_here();
    let titles: Vec<&str> = menus.iter().map(|m| m["label"].as_str().unwrap()).collect();
    assert_eq!(titles, want.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let mut ids = Vec::new();
    for (menu, (title, rows)) in menus.iter().zip(&want) {
        assert_eq!(menu["widget"], format!("menu:{title}"));
        let mut got = Vec::new();
        flatten(&menu["children"], "", &mut got);
        assert_eq!(&got, rows, "{title}");
        collect_items(&menu["children"], &mut ids);
    }
    // Every item has a command id, a widget id in the grammar of §7 (unique within its menu),
    // `enabled` and `live`; disabled items are listed, never hidden.
    let total = ids.len();
    assert!(total >= 110, "{total} items");
    for menu in menus {
        let mut items = Vec::new();
        collect_items(&menu["children"], &mut items);
        let mut widgets: Vec<&str> = items.iter().map(|i| i["widget"].as_str().unwrap()).collect();
        for (i, w) in items.iter().zip(&widgets) {
            assert!(registry::is_valid_id(w) && w.starts_with("menuitem:"), "{w}");
            assert!(i["id"].as_str().is_some_and(|id| w.contains(id)) && i["enabled"].is_boolean() && i["live"].is_boolean(), "{i}");
        }
        widgets.sort();
        let n = widgets.len();
        widgets.dedup();
        assert_eq!(widgets.len(), n, "{}: widget ids repeat", menu["label"]);
    }
    // Checkable rows say whether they are on; radio rows say so too.
    let all: Vec<&Value> = ids.clone();
    let props = all.iter().find(|i| i["id"] == "pane.properties").unwrap();
    assert_eq!(props["checked"], false);
    let small = all.iter().find(|i| i["id"] == "view.thumbsSmall").unwrap();
    assert_eq!((small["checked"].clone(), small["radio"].clone()), (json!(false), json!(true)));
    let shortcut = |id: &str| all.iter().find(|i| i["id"] == id).unwrap().get("shortcut").cloned();
    let ctrl = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    assert_eq!(shortcut("album.new"), Some(json!(format!("{ctrl}+N"))));
    assert_eq!(shortcut("window.fullscreen"), Some(json!("F11")));
    assert_eq!(shortcut("photo.star.toggle"), Some(json!(format!("{ctrl}+8"))));
    assert_eq!(shortcut("faces.reset"), None);
    // The section context menu is the fourth menu's rows.
    let section = ok(&mut t, "ui.menu.tree", json!({ "context": "section" }));
    let folder = menus.iter().find(|m| m["widget"] == "menu:Folder").unwrap();
    assert_eq!(section, folder["children"]);
    for c in ["photo", "tray", "treeGroup", "tag", "face"] {
        assert_eq!(code(&mut t, "ui.menu.tree", json!({ "context": c })), "unsupported", "{c}");
    }
    for bad in [json!({ "context": "nope" }), json!({ "context": 3 }), json!({ "extra": 1 }), json!([1])] {
        assert_eq!(code(&mut t, "ui.menu.tree", bad.clone()), "bad_params", "{bad}");
    }
    assert_eq!(ok(&mut t, "ui.menu.tree", Value::Null), tree, "null params are {{}}");
}

fn collect_items<'v>(children: &'v Value, out: &mut Vec<&'v Value>) {
    for row in children.as_array().unwrap() {
        if let Some(kids) = row.get("children") {
            collect_items(kids, out);
        } else if row.get("id").is_some() {
            out.push(row);
        }
    }
}

/// The spec itself: every item row of §1's tables names an item the tree shows (so the
/// transcription above cannot drift from the document unnoticed).
#[test]
fn every_item_row_of_the_spec_tables_is_in_the_tree() {
    let doc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/menus-and-shortcuts.md")).unwrap();
    let start = doc.find("## 1. Menu bar").unwrap();
    let end = doc.find("### 1.8 Help").unwrap();
    let mut labels = Vec::new();
    for (title, rows) in expected() {
        labels.push(title.to_string());
        labels.extend(rows.into_iter().map(|r| r.rsplit(" ▸ ").next().unwrap().trim_end_matches(" ▸").to_string()));
    }
    let mut checked = 0;
    for line in doc[start..end].lines().filter(|l| l.starts_with("| ")) {
        let first = line.trim_start_matches("| ").split(" |").next().unwrap().trim();
        if first.is_empty() || first == "Item" || first == "—" || first.starts_with('`') || first.starts_with("Command") {
            continue;
        }
        let label = first.split(" *").next().unwrap().split(" ▸").next().unwrap();
        let label = if label.contains(" / ") && label != "Star / Unstar" { label.split(" / ").next().unwrap() } else { label };
        assert!(labels.iter().any(|l| l == label), "§1 row `{first}` ({label}) is not in the menus");
        checked += 1;
    }
    assert!(checked >= 70, "{checked} rows checked");
}

/// The M3.3 done-when: invoking a disabled item is `disabled`, an unknown path `not_found`; and
/// the rest of `ui.menu.invoke`.
#[test]
fn invoking_items_runs_refuses_or_opens_them_like_a_click() {
    let mut t = setup(2);
    let starred = |t: &T, id: u64| {
        let s = t.d.app().session().unwrap();
        s.library().photo(dalica_core::PhotoId::new(id)).unwrap().star
    };
    // Disabled: Star with nothing selected (live, but its command says no), by path and by id.
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": "Picture/Star \\/ Unstar" })), "disabled");
    let e = call(&mut t, "ui.menu.invoke", json!({ "id": "photo.star.toggle" })).unwrap_err();
    assert!(e.message.contains("Select some photos first."), "{}", e.message);
    // Unknown paths and ids.
    for bad in [json!({ "path": "Picture/Nope" }), json!({ "path": "Nope" }), json!({ "path": "Picture/Batch Edit" }), json!({ "id": "no.such" })] {
        assert_eq!(code(&mut t, "ui.menu.invoke", bad.clone()), "not_found", "{bad}");
    }
    // Catalog items without a command yet, and `…` items whose dialog is not built.
    for p in ["File/New Album…", "View/Slideshow", "Help/Rebuild Library…", "View/Thumbnail Caption/Date"] {
        assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": p })), "unsupported", "{p}");
    }
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "id": "view.caption", "params": { "caption": "tags" } })), "unsupported");
    // Bad parameters change nothing.
    for bad in [
        json!({}),
        json!({ "path": "Edit/Select None", "id": "select.none" }),
        json!({ "path": "" }),
        json!({ "path": "Edit//Select None" }),
        json!({ "path": "Edit\\Select None" }),
        json!({ "path": 3 }),
        json!({ "id": "" }),
        json!({ "id": "x".repeat(300) }),
        json!({ "id": "select.none", "params": [1] }),
        json!({ "id": "select.none", "wait": "yes" }),
        json!({ "id": "select.none", "extra": true }),
        json!("Edit/Select None"),
    ] {
        assert_eq!(code(&mut t, "ui.menu.invoke", bad.clone()), "bad_params", "{bad}");
    }
    // A live, enabled item runs with the selection: the reply is the command's result.
    let (a, b) = (t.ids[0], t.ids[1]);
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [a] } }));
    let r = ok(&mut t, "ui.menu.invoke", json!({ "path": "Picture/Star \\/ Unstar", "wait": true }));
    assert!(r.to_string().contains("true"), "{r}");
    assert!(starred(&t, a) && !starred(&t, b));
    // Undo now names the step, and is found by that label.
    let tree = ok(&mut t, "ui.menu.tree", json!({}));
    let mut items = Vec::new();
    for m in tree.as_array().unwrap() {
        collect_items(&m["children"], &mut items);
    }
    let undo = items.iter().find(|i| i["id"] == "edit.undo").unwrap()["label"].as_str().unwrap().to_string();
    assert!(undo.starts_with("Undo ") && undo.len() > 5, "{undo}");
    ok(&mut t, "ui.menu.invoke", json!({ "path": format!("Edit/{}", undo.replace('/', "\\/")) }));
    assert!(!starred(&t, a), "undone");
    // The caller's params win over the selection.
    ok(&mut t, "ui.menu.invoke", json!({ "id": "photo.star.toggle", "params": { "ids": [b] } }));
    assert!(starred(&t, b) && !starred(&t, a));
    // A UI command.
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "Edit/Select None" })), json!({ "selection": [] }));
    assert!(t.d.state().selection.ids.is_empty());
    // A dialog item opens its dialog; the picker is answered as usual.
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": "File/Add Folder…", "params": { "path": "/x" } })), "bad_params");
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "File/Add Folder" })), json!({ "dialog": "filePicker" }));
    assert!(t.control.stub().waiting().is_some());
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "id": "folders.add" })), "disabled", "one folder dialog at a time");
    let inside = t.extra.join("2026-07 More");
    std::fs::create_dir_all(&inside).unwrap();
    let r = ok(&mut t, "ui.filePicker", json!({ "path": inside }));
    assert_eq!(r["new"], true);
    assert_eq!(t.d.app().session().unwrap().roots().len(), 2);
    // `methods` lists both as built.
    let methods = ok(&mut t, "methods", json!({}));
    for m in ["ui.menu.tree", "ui.menu.invoke"] {
        let entry = methods.as_array().unwrap().iter().find(|e| e["method"] == m).unwrap();
        assert_eq!(entry["built"], true, "{m}");
    }
    let confirm = methods.as_array().unwrap().iter().find(|e| e["method"] == "ui.dialog.confirm").unwrap();
    assert!(confirm["planned"].as_str().unwrap().contains("M5"), "{confirm}");
}

// ---------------------------------------------------------------------------------------------
// Menus drawn: bounded by the window, scrolling when too tall
// ---------------------------------------------------------------------------------------------

/// A window holding only the in-window menu bar (24 pt, as `ui-layout.md` §2) over an empty
/// light table, driven frame by frame on a 60 Hz virtual clock.
struct Window {
    ctx: egui::Context,
    raster: Rasterizer,
    app: App,
    w: u32,
    h: u32,
    /// Pixels per point: the UI scale.
    ppp: f32,
    time: f64,
    events: Vec<Event>,
    /// The last frame's shapes, painted only when a shot is saved.
    shapes: Vec<egui::epaint::ClippedShape>,
    /// Texture frees to apply before the next frame's uploads.
    to_free: egui::TexturesDelta,
    /// Right-clicking the light table opens the section context menu instead of nothing.
    context: bool,
}

impl Window {
    /// A `w × h` pixel window at `ppp` pixels per point.
    fn new(w: u32, h: u32, ppp: f32, context: bool) -> Window {
        let app = App::with_picker(None, Arc::new(ScriptedPicker::new()));
        let ctx = egui::Context::default();
        app.install(&ctx);
        let mut win = Window {
            ctx,
            raster: Rasterizer::new(),
            app,
            w,
            h,
            ppp,
            time: 0.0,
            events: Vec::new(),
            shapes: Vec::new(),
            to_free: Default::default(),
            context,
        };
        win.frames(3);
        win
    }

    fn frame(&mut self) {
        let mut input = dalica_ui_kit::raw_input(self.w, self.h, self.ppp);
        input.time = Some(self.time);
        input.events = std::mem::take(&mut self.events);
        let app = &mut self.app;
        let context = self.context;
        let t = *app.tokens();
        let out = self.ctx.run_ui(input, |ui| {
            let bar = egui::Frame::new().fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 0));
            egui::Panel::top("menubar").exact_size(24.0).frame(bar).show(ui, |ui| {
                menus::show_bar(ui, app);
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.grid_bg)).show(ui, |ui| {
                let r = ui.interact(ui.max_rect(), egui::Id::new("light-table"), egui::Sense::click());
                dalica_ui_kit::widgets::register(
                    ui.ctx(),
                    "lightTable",
                    registry::Role::Other,
                    "Light table",
                    dalica_ui_kit::widgets::Target::Widget(&r),
                    false,
                );
                if context {
                    let nodes = menus::section_menu(app, menus::Platform::Other);
                    menus::context_menu(&r, app, &nodes);
                }
            });
        });
        self.raster.free_textures(&mut self.to_free);
        let mut delta = out.textures_delta;
        self.raster.set_textures(&mut delta);
        self.to_free = delta;
        self.shapes = out.shapes;
        self.time += 1.0 / 60.0;
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn find(&self, id: &str) -> Option<dalica_ui_kit::WidgetInfo> {
        registry::find(&self.ctx, id)
    }

    fn click_at(&mut self, pos: Pos2, button: PointerButton) {
        self.events.push(Event::PointerMoved(pos));
        self.frame();
        self.events.push(Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE });
        self.frame();
        self.events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
        self.frames(3);
    }

    fn click(&mut self, id: &str) {
        let w = self.find(id).unwrap_or_else(|| panic!("no `{id}`"));
        self.click_at(w.center(), PointerButton::Primary);
    }

    fn wheel_at(&mut self, pos: Pos2, dy: f32) {
        self.events.push(Event::PointerMoved(pos));
        let wheel = |delta, phase| Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE };
        self.events.push(wheel(vec2(0.0, 0.0), TouchPhase::Start));
        self.events.push(wheel(vec2(0.0, dy), TouchPhase::Move));
        self.frame();
        self.events.push(wheel(vec2(0.0, 0.0), TouchPhase::End));
        self.frames(30);
    }

    /// Every registered widget lies inside the window; no interactive widget lacks an id.
    fn all_inside(&self, state: &str) {
        let (w, h) = (self.w as f32 / self.ppp, self.h as f32 / self.ppp);
        for wi in registry::widgets(&self.ctx) {
            let [x, y, rw, rh] = wi.rect;
            assert!(x >= 0.0 && y >= 0.0 && x + rw <= w + 0.01 && y + rh <= h + 0.01, "{state}: `{}` at {:?}", wi.id, wi.rect);
        }
        assert_eq!(registry::unregistered(&self.ctx), [], "{state}: interactive widgets without an id");
    }

    fn save(&mut self, name: &str) {
        let primitives = self.ctx.tessellate(self.shapes.clone(), self.ppp);
        let shot: Image = self.raster.render(&primitives, self.ppp, self.w, self.h, self.app.tokens().chrome).unwrap();
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    }
}

/// The M3.3 done-when: a 900 × 600 window (the smallest the window gets) shows the longest menu,
/// View, inside the window. At 100 % UI scale it fits (31 rows in about 500 of the 552 points
/// below the bar), so it is shown there without arrows; at 150 % (`design-system.md` §11.4: the
/// same 900 × 600 pixels are 600 × 400 points) it is taller than the window and scrolls inside it.
#[test]
fn the_longest_menu_stays_inside_a_900_by_600_window_and_scrolls_when_taller() {
    let mut win = Window::new(900, 600, 1.0, false);
    for title in ["File", "Edit", "View", "Folder", "Picture", "Create", "Tools", "Help"] {
        assert!(win.find(&format!("menu:{title}")).is_some(), "{title}");
    }
    win.click("menu:View");
    win.frames(2);
    assert!(win.find("menu:View").unwrap().active, "the open menu's title is active");
    win.all_inside("View open at 100 %");
    win.save("menus-view-900x600");
    assert!(win.find("button:menu.scrollUp/1").is_none(), "View fits at 100 %");
    assert!(win.find("menuitem:view.library").is_some() && win.find("menuitem:window.fullscreen").is_some());

    let mut win = Window::new(900, 600, 1.5, false);
    win.click("menu:View");
    win.frames(2);
    win.all_inside("View open at 150 %");
    win.save("menus-view-900x600-150");
    let up = win.find("button:menu.scrollUp/1").expect("an up arrow");
    let down = win.find("button:menu.scrollDown/1").expect("a down arrow");
    assert!(up.rect[1] >= 24.0, "the menu hangs below the bar: {:?}", up.rect);
    assert!(down.rect[1] + down.rect[3] <= 400.0, "{:?}", down.rect);
    assert!(win.find("menuitem:view.library").is_some(), "the top rows show first");
    assert!(win.find("menuitem:window.fullscreen").is_none(), "Full Screen is below the fold");
    // Disabled items are drawn (greyed), not hidden.
    assert!(!win.find("menuitem:view.library").unwrap().enabled);
    // The wheel scrolls the menu to its end.
    let at = Pos2::new(up.rect[0] + up.rect[2] / 2.0, 200.0);
    win.wheel_at(at, -2000.0);
    win.all_inside("View scrolled");
    win.save("menus-view-900x600-150-scrolled");
    assert!(win.find("menuitem:window.fullscreen").is_some(), "Full Screen scrolled into view");
    assert!(win.find("menuitem:view.library").is_none(), "the top rows scrolled away");
    // Resting the pointer on the up arrow scrolls back; a click on the down arrow steps down.
    win.events.push(Event::PointerMoved(up.center()));
    win.frames(120);
    assert!(win.find("menuitem:view.library").is_some(), "back at the top");
    win.click("button:menu.scrollDown/1");
    assert!(win.find("menuitem:view.library").is_none(), "a click steps down");
    // A click outside closes it.
    win.click_at(Pos2::new(500.0, 300.0), PointerButton::Primary);
    assert!(win.find("menuitem:view.thumbsSmall").is_none() && !win.find("menu:View").unwrap().active, "closed");
}

/// Short menus do not scroll, and submenus stay inside the window too.
#[test]
fn short_menus_and_submenus_fit_without_arrows() {
    let mut win = Window::new(900, 600, 1.0, false);
    win.click("menu:Tools");
    win.frames(2);
    assert!(win.find("button:menu.scrollUp/1").is_none(), "Tools fits");
    let experimental = win.find("submenu:Tools/Experimental").expect("the Experimental row");
    win.events.push(Event::PointerMoved(experimental.center()));
    win.frames(20);
    let search = win.find("submenu:Tools/Experimental/Search for").expect("Experimental opened on hover");
    win.events.push(Event::PointerMoved(Pos2::new(search.rect[0] + 10.0, search.rect[1] + search.rect[3] / 2.0)));
    win.frames(20);
    assert!(win.find("menuitem:search.color/bw").is_some(), "Search for opened on hover");
    win.all_inside("Tools ▸ Experimental ▸ Search for open");
    win.save("menus-tools-submenus-900x600");
}

/// A context menu taller than a short window scrolls inside it as well.
#[test]
fn a_context_menu_in_a_short_window_scrolls_inside_it() {
    let mut win = Window::new(900, 300, 1.0, true);
    win.click_at(Pos2::new(450.0, 200.0), PointerButton::Secondary);
    win.frames(2);
    assert!(win.find("menuitem:section.properties").is_some(), "the section menu opened");
    assert!(win.find("button:menu.scrollDown/1").is_some(), "it scrolls");
    win.all_inside("section menu open");
    win.save("menus-section-context-900x300");
    // Clicking a live item runs it: none of the section items is live yet, so nothing runs and the
    // menu stays usable.
    assert!(!win.find("menuitem:section.properties").unwrap().enabled);
}

/// Clicking a live item in a drawn menu runs its command and closes the menu.
#[test]
fn clicking_an_item_runs_it_and_closes_the_menu() {
    let mut win = Window::new(1400, 900, 1.0, false);
    win.app.state_mut().status = "something".into();
    win.click("menu:Edit");
    let item = win.find("menuitem:select.none").unwrap();
    assert!(item.enabled, "Select None runs without a library");
    win.click("menuitem:select.none");
    assert!(win.app.state().status.is_empty(), "the command ran (a success clears the status line)");
    assert!(win.find("menuitem:select.none").is_none(), "the menu closed");
    // A disabled item does not run.
    win.click("menu:Picture");
    let star = win.find("menuitem:photo.star.toggle").unwrap();
    assert!(!star.enabled);
}

/// In the real window the bar sits in the layout's menu-bar region and its menus hang below it,
/// inside the window, at both sizes.
#[test]
fn the_window_draws_the_menus_in_its_menu_bar_region() {
    for (w, h) in [(1400, 900), (900, 600)] {
        let mut t = setup(1);
        t.d.resize(w, h);
        t.d.settle(FRAMES).unwrap();
        let widgets = t.d.widgets();
        let region = widgets.iter().find(|wi| wi.id == "region:menuBar").expect("the menu-bar region").rect;
        let inside =
            |r: [f32; 4], o: [f32; 4]| r[0] >= o[0] && r[1] >= o[1] && r[0] + r[2] <= o[0] + o[2] + 0.01 && r[1] + r[3] <= o[1] + o[3] + 0.01;
        for title in ["File", "Edit", "View", "Folder", "Picture", "Create", "Tools", "Help"] {
            let r = widgets.iter().find(|wi| wi.id == format!("menu:{title}")).unwrap_or_else(|| panic!("{w}×{h}: menu:{title}")).rect;
            assert!(inside(r, region), "{w}×{h}: menu:{title} {r:?} outside the bar {region:?}");
        }
        t.d.click("menu:View").unwrap();
        t.d.settle(FRAMES).unwrap();
        let widgets = t.d.widgets();
        let items: Vec<_> = widgets.iter().filter(|wi| wi.id.starts_with("menuitem:")).collect();
        assert_eq!(items.len(), 14, "{w}×{h}: every View item is on screen");
        for wi in items {
            assert!(wi.rect[1] >= region[1] + region[3], "{w}×{h}: `{}` overlaps the bar", wi.id);
            assert!(inside(wi.rect, [0.0, 0.0, w as f32, h as f32]), "{w}×{h}: `{}` at {:?}", wi.id, wi.rect);
        }
        assert_eq!(t.d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h}");
        let shot = t.d.screenshot().unwrap();
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
        std::fs::create_dir_all(&dir).unwrap();
        let png = dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba);
        std::fs::write(dir.join(format!("menus-window-view-{w}x{h}.png")), png).unwrap();
    }
}

/// The in-window bar draws the Windows/Linux layout on every OS: on macOS the Mac layout (an
/// application menu first, Options… moved into it) belongs to the system menu bar
/// (`design-system.md` §11.1), and whole-window fingerprints are blessed once for every OS. The
/// drawn bar is compared with the `Other` model here (it fails on macOS when the bar follows the
/// OS), and the source check fails on any OS when `show_bar` asks the OS again.
#[test]
fn the_in_window_bar_draws_the_same_layout_on_every_os() {
    let other: Vec<&str> = menus::bar_menus(menus::Platform::Other).iter().map(|m| m.key).collect();
    let mac: Vec<&str> = menus::bar_menus(menus::Platform::Mac).iter().map(|m| m.key).collect();
    assert_ne!(other, mac, "the two layouts differ, so the check below means something");

    let mut win = Window::new(1400, 900, 1.0, false);
    let mut drawn: Vec<_> = registry::widgets(&win.ctx).into_iter().filter(|wi| wi.id.starts_with("menu:")).collect();
    drawn.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
    let drawn: Vec<String> = drawn.iter().map(|wi| wi.id.trim_start_matches("menu:").to_string()).collect();
    assert_eq!(drawn, other, "the bar draws the Windows/Linux menus, left to right");
    // Options… stays in Tools (on macOS the system menu bar's application menu holds it).
    win.click("menu:Tools");
    assert!(win.find("menuitem:app.preferences").is_some(), "Tools ▸ Options… is drawn");

    let src = include_str!("../src/menus.rs");
    let start = src.find("pub fn show_bar(").expect("show_bar");
    let body = src.get(start..).and_then(|s| s.find("\n}\n").and_then(|end| s.get(..end))).expect("show_bar's body");
    assert!(!body.contains("Platform::current()"), "show_bar must not draw a per-OS layout");
}
