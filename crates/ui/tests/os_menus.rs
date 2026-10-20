//! The OS integration in the window (M15.22, `organizing.md` §9, `menus-and-shortcuts.md` §1),
//! driven headlessly with the recording `Os` standing for a window's system (nothing is ever
//! launched): the photo menu's Locate on Disk, Open in External Editor and Copy Path, the Folder
//! menu's and the folder header icon's Locate on Disk, the tray menu's and Ctrl+Enter hand the
//! right files; Open With ▸ says "Loading…", then lists the system's applications (Dalica left
//! out) and choosing one opens the photo with it; Ctrl+C puts the files on the system clipboard
//! and still pastes inside Dalica; Options ▸ General ▸ External editor's Browse… sets
//! `files.externalEditor`; a system without a service greys its rows with its reason; at 900 × 600
//! the open Open With ▸ stays inside the window. The PR's screenshots are written to
//! `target/test-output/ui/os-menus-*.png`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, new_driver, try_widget, write_png};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_engine::os::{Clip, Handler, OsCall, Recording, Service};
use dalica_scenes::fingerprint;
use dalica_ui::headless::Click;
use dalica_ui::menus::{self, Node, Platform};
use dalica_ui::state::Section;
use dalica_ui::{App, Driver, ScriptedPicker};
use egui::PointerButton;
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    os: Arc<Recording>,
    picker: Arc<ScriptedPicker>,
    /// The light table's sections' photos, in order, and their folders.
    s: Vec<Vec<PhotoId>>,
    folders: Vec<FolderId>,
}

fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(2, 6, 22).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut session = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        session.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    session.wait_idle();
    let os = Arc::new(Recording::windowed());
    session.set_os(os.clone());
    let picker = Arc::new(ScriptedPicker::new());
    let mut d = new_driver(App::with_picker(Some(session), picker.clone()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let sections = &d.app().panels().unwrap().sections.list;
    let s: Vec<Vec<PhotoId>> = sections.iter().map(|s| s.photos.clone()).collect();
    let folders: Vec<FolderId> = sections.iter().filter_map(|s| s.folder).collect();
    assert_eq!((s.len(), folders.len()), (2, 2));
    T { _dir: dir, d, os, picker, s, folders }
}

impl T {
    fn path(&self, id: PhotoId) -> PathBuf {
        let lib = self.d.app().session().unwrap().library();
        let p = lib.photo(id).unwrap();
        lib.folder(p.folder).unwrap().path.join(&p.name)
    }

    fn folder_path(&self, id: FolderId) -> PathBuf {
        self.d.app().session().unwrap().library().folder(id).unwrap().path.clone()
    }

    /// Selects `id` with a click, then right-clicks it: the photo menu opens for it.
    fn open_menu(&mut self, id: PhotoId) {
        let cell = format!("thumb:{}", id.get());
        self.d.click(&cell).unwrap();
        self.d.click_with(&cell, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
        self.d.settle(FRAMES).unwrap();
        assert!(try_widget(&self.d, "menuitem:view.edit").is_some(), "the photo menu is open");
    }

    /// Clicks the row `id` and returns the calls the system got from it.
    fn click_row(&mut self, id: &str) -> Vec<OsCall> {
        self.os.clear();
        self.d.click(id).unwrap();
        self.d.settle(FRAMES).unwrap();
        self.os.calls()
    }

    fn shot(&self, name: &str) {
        let shot = self.d.screenshot().unwrap();
        write_png(name, &fingerprint::png_rgba(shot.width, shot.height, &shot.rgba));
    }
}

/// The rows of the open Open With ▸ in the photo menu's model.
fn open_with(app: &App) -> Vec<(String, String, bool)> {
    let menu = menus::photo_menu(app, Platform::Other);
    let Some(Node::Submenu { children, .. }) = menu.into_iter().find(|n| matches!(n, Node::Submenu { label: "Open With", .. })) else {
        panic!("no Open With ▸")
    };
    children
        .into_iter()
        .filter_map(|n| match n {
            Node::Item(i) => Some((i.widget, i.label, i.enabled)),
            _ => None,
        })
        .collect()
}

#[test]
fn each_row_and_the_folder_icon_hand_the_system_the_right_files() {
    let mut t = setup(LARGE);
    let (a, b) = (t.s[0][1], t.s[1][0]);

    t.open_menu(a);
    assert_eq!(t.click_row("menuitem:photo.locate"), [OsCall::Reveal(t.path(a))], "Locate on Disk");
    t.open_menu(a);
    assert_eq!(t.click_row("menuitem:photo.openExternal"), [OsCall::OpenInEditor { paths: vec![t.path(a)], editor: String::new() }]);
    t.open_menu(a);
    let text = t.path(a).to_string_lossy().into_owned();
    assert_eq!(t.click_row("menuitem:photo.copyPath"), [OsCall::PutOnClipboard(Clip::Text(text))], "Copy Path");

    // Ctrl+Enter in the library is Locate on Disk on the photo last clicked.
    t.os.clear();
    t.d.key("Ctrl+Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.os.calls(), [OsCall::Reveal(t.path(a))], "Ctrl+Enter");

    // The Folder menu's Locate on Disk shows the current folder; the header icon its own folder.
    t.d.click("menu:Folder").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.click_row("menuitem:folder.locate"), [OsCall::Reveal(t.folder_path(t.folders[0]))], "Folder ▸ Locate on Disk");
    let icon = format!("section:{}/icon", Section::Folder(t.folders[1]));
    assert_eq!(t.click_row(&icon), [OsCall::Reveal(t.folder_path(t.folders[1]))], "the header icon");

    // The tray menu's Locate on Disk shows the first photo it acts on.
    let nodes = dalica_ui::panels::bottom::tray::context_nodes(t.d.app(), &[b, a]);
    let locate = menus::find_id(&nodes, "photo.locate", &json!({ "id": b })).cloned().unwrap();
    assert!(locate.enabled, "{:?}", locate.reason);
    t.os.clear();
    menus::activate(t.d.app_mut(), &locate, &Value::Null).unwrap();
    assert_eq!(t.os.calls(), [OsCall::Reveal(t.path(b))], "the tray's Locate on Disk");
}

#[test]
fn open_with_lists_the_systems_applications_and_opens_the_photo_with_one() {
    let mut t = setup(LARGE);
    let a = t.s[0][0];
    let ext = t.path(a).extension().unwrap().to_string_lossy().into_owned();
    let handler = |id: &str, name: &str| Handler { id: id.into(), name: name.into() };
    t.os.set_handlers(
        &ext,
        vec![handler("gimp.desktop", "GIMP"), handler("org.dalica.Dalica.desktop", "Dalica"), handler("eog.desktop", "Image Viewer")],
    );
    t.open_menu(a);
    assert_eq!(open_with(t.d.app()), [("menuitem:openWith.list/loading".to_string(), "Loading…".to_string(), false)], "until it opens");
    t.os.clear();
    t.d.click("submenu:Photo/Open With").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.os.calls(), [OsCall::Handlers(t.path(a))], "asked once, when the submenu opened");
    assert_eq!(
        open_with(t.d.app()),
        [
            ("menuitem:photo.openWith/0".to_string(), "GIMP".to_string(), true),
            ("menuitem:photo.openWith/1".to_string(), "Image Viewer".to_string(), true)
        ],
        "Dalica left out"
    );
    assert!(try_widget(&t.d, "menuitem:photo.openWith/1").is_some(), "drawn");
    t.shot("os-menus-open-with-1400x900");
    assert_eq!(t.click_row("menuitem:photo.openWith/1"), [OsCall::OpenWith { path: t.path(a), handler: "eog.desktop".into() }]);

    // A type the system has nothing for: "No other applications".
    t.os.set_handlers(&ext, Vec::new());
    t.d.run("openWith.close", &json!({})).unwrap();
    t.d.run("openWith.list", &json!({ "id": a })).unwrap();
    assert_eq!(open_with(t.d.app()), [("menuitem:openWith.list/none".to_string(), "No other applications".to_string(), false)]);
}

#[test]
fn copy_puts_the_files_on_the_system_clipboard_and_still_pastes_inside_dalica() {
    let mut t = setup(LARGE);
    let (a, b) = (t.s[0][0], t.s[0][1]);
    t.d.click(&format!("thumb:{}", a.get())).unwrap();
    t.d.click_with(&format!("thumb:{}", b.get()), &Click { modifiers: egui::Modifiers::COMMAND, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.os.clear();
    t.d.key("Ctrl+C").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.os.calls(), [OsCall::PutOnClipboard(Clip::Files(vec![t.path(a), t.path(b)]))]);
    let clip = t.d.state().clipboard.clone().unwrap();
    assert_eq!(clip.ids, [a, b], "kept for Paste");
    // Paste into the other folder asks "Copy 2 files to …?", as it did before.
    t.d.run("select.section", &json!({ "section": Section::Folder(t.folders[1]) })).unwrap();
    let reply = t.d.run("photo.paste", &json!({})).unwrap();
    assert!(reply.get("dialog").is_some(), "{reply}");
}

#[test]
fn browse_sets_the_external_editor() {
    let mut t = setup(LARGE);
    let program = std::env::temp_dir().join("editors").join("gimp");
    t.d.run("app.preferences", &json!({ "tab": "general" })).unwrap();
    t.d.settle(FRAMES).unwrap();
    // The Files group runs below the dialog's fold: scroll the tab down to the row.
    t.d.scroll(Some("check:option.interface.tooltips"), egui::vec2(0.0, -2000.0)).unwrap();
    t.d.settle(FRAMES).unwrap();
    let ids: Vec<String> = t.d.widgets().into_iter().map(|w| w.id).filter(|i| i.contains("option.")).collect();
    let field = try_widget(&t.d, "choice:option.files.externalEditor").unwrap_or_else(|| panic!("the External editor row: {ids:?}"));
    assert_eq!(field.label, "System default");
    t.shot("os-menus-options-editor-1400x900");
    t.picker.push_file(&program);
    t.d.click("button:option.files.externalEditor/browse").unwrap();
    t.d.step_until(|app| app.state().options.as_ref().is_some_and(|o| o.values.contains_key("files.externalEditor")), FRAMES).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(try_widget(&t.d, "choice:option.files.externalEditor").unwrap().label, "gimp", "the program by its name");
    t.shot("os-menus-options-editor-chosen-1400x900");
    t.d.click("button:dialog.ok").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().session().unwrap().settings().external_editor(), program.to_string_lossy());
    // Open in External Editor starts it.
    let a = t.s[0][0];
    t.open_menu(a);
    assert_eq!(
        t.click_row("menuitem:photo.openExternal"),
        [OsCall::OpenInEditor { paths: vec![t.path(a)], editor: program.to_string_lossy().into_owned() }]
    );
}

#[test]
fn a_system_without_a_service_greys_its_rows_with_its_reason() {
    let mut t = setup(LARGE);
    let a = t.s[0][0];
    let why = "There is no file manager to show it in.";
    t.os.set_unavailable(Service::Reveal, Some(why));
    t.os.set_unavailable(Service::OpenWith, Some("Open With is not offered here."));
    t.open_menu(a);
    let menu = menus::photo_menu(t.d.app(), Platform::Other);
    let locate = menus::find_id(&menu, "photo.locate", &Value::Null).unwrap();
    assert_eq!((locate.enabled, locate.reason.as_deref()), (false, Some(why)));
    assert_eq!(open_with(t.d.app()), [("menuitem:openWith.list/none".to_string(), "No other applications".to_string(), false)]);
    assert!(!try_widget(&t.d, "menuitem:photo.locate").unwrap().enabled, "drawn greyed");
    let tray = dalica_ui::panels::bottom::tray::context_nodes(t.d.app(), &[a]);
    let row = menus::find_id(&tray, "photo.locate", &json!({ "id": a })).unwrap();
    assert_eq!((row.enabled, row.reason.as_deref()), (false, Some(why)));
    t.d.key("Escape").unwrap();
    t.d.settle(FRAMES).unwrap();
    let icon = format!("section:{}/icon", Section::Folder(t.folders[0]));
    assert!(!try_widget(&t.d, &icon).unwrap().enabled, "the header icon is greyed");
}

#[test]
fn at_900_by_600_the_open_with_submenu_stays_inside_the_window() {
    let mut t = setup(SMALL);
    // The last photo of the light table, as low on the screen as a photo goes.
    let last = *t.s[1].last().unwrap();
    t.d.run("section.scrollTo", &json!({ "section": Section::Folder(t.folders[1]) })).unwrap();
    t.d.settle(FRAMES).unwrap();
    let ext = t.path(last).extension().unwrap().to_string_lossy().into_owned();
    let apps: Vec<Handler> = (0..12).map(|n| Handler { id: format!("app{n}.desktop"), name: format!("Application {n}") }).collect();
    t.os.set_handlers(&ext, apps);
    t.open_menu(last);
    t.d.click("submenu:Photo/Open With").unwrap();
    t.d.settle(FRAMES).unwrap();
    let rows: Vec<[f32; 4]> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:photo.openWith/")).map(|w| w.rect).collect();
    assert!(!rows.is_empty(), "the applications are drawn");
    let (w, h) = (SMALL.0 as f32, SMALL.1 as f32);
    for r in &rows {
        assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= w + 0.5 && r[1] + r[3] <= h + 0.5, "{r:?} outside {w} × {h}");
    }
    t.shot("os-menus-open-with-900x600");
}

/// The viewer's ⋯ More: Show in File Manager shows the file shown, selected (a photo outside the
/// library, through the system directly); Open With ▸ is greyed for it, with the reason.
#[test]
fn the_viewers_more_menu_shows_the_file_and_greys_open_with_outside_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("C.jpg");
    std::fs::write(&file, dalica_scenes::jpeg(&dalica_scenes::Scene::new(7, 600, 400), 85, None).unwrap()).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": false } })).unwrap();
    s.open_viewer(&file).unwrap();
    let os = Arc::new(Recording::windowed());
    s.set_os(os.clone());
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.step_until(|app| app.state().view == dalica_ui::state::View::Viewer, FRAMES).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("button:viewer.more").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "menuitem:viewer.showInFileManager").unwrap().enabled);
    d.click("menuitem:viewer.openWith").unwrap();
    d.settle(FRAMES).unwrap();
    let row = try_widget(&d, "menuitem:openWith.list/none").expect("Open With ▸'s row");
    assert!(!row.enabled, "greyed: {}", dalica_ui::commands::os::NOT_IN_LIBRARY);
    os.clear();
    d.click("menuitem:viewer.showInFileManager").unwrap();
    d.settle(FRAMES).unwrap();
    let shown = std::fs::canonicalize(&file).unwrap();
    let calls = os.calls();
    assert!(matches!(&calls[..], [OsCall::Reveal(p)] if std::fs::canonicalize(p).unwrap() == shown), "{calls:?}");
}
