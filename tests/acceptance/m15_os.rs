//! The operating system's own ways with files (`m15.rs` part 3; `roadmap.md` M15 → OS
//! integration's *Done when*), in two halves:
//!
//! - **By the person, through the recording `Os`** (in process: the window's own keys, clicks and
//!   menus drive `dalica_ui`'s headless `Driver`, as a person's input reaches the app, with
//!   `dalica_engine::os::Recording` standing for a desktop's system, so nothing is launched): on
//!   a folder of two photos, Ctrl+Enter (Locate on Disk) reveals the photo clicked; the folder
//!   header's icon reveals its folder; Ctrl+Shift+O hands the selected photos to the external editor
//!   set in Options; Open With ▸ lists the system's applications for the type and opens the photo
//!   with the one chosen; Copy Path puts the photo's path, and Ctrl+C the files, on the clipboard.
//! - **Over automation, in the real `dalica --headless`** started with
//!   `--automation-allow-external`: Locate on Disk (`photo.locate`, `folder.locate`), Open With
//!   (`photo.openWith`) and Open in External Editor (`photo.openExternal`) are refused with
//!   `external_denied` whatever was granted, and so are Ctrl+Enter and a folder icon's click sent as
//!   a control request (the status line says why); `photo.openWithList` only lists; a headless
//!   app has no system clipboard, so Copy Path and Copy say so (`disabled`).
//!
//! The real services per OS (the argument vectors, the handler lists, the clipboard) are
//! `apps/dalica`'s `os` suite; a drag out, the default viewer and the real programs are checked by
//! hand on each OS (`m15.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::os::{Clip, Handler, OsCall, Recording};
use dalica_ui::headless::Click;
use dalica_ui::{App, Driver};
use egui::PointerButton;
use serde_json::{Value, json};

use crate::app::{idle, run};
use crate::common::{self, COLOURS, LARGE};
use crate::live::{self, Launch};

/// Frames a step may take to settle.
const FRAMES: usize = 3600;

struct Window {
    d: Driver,
    os: Arc<Recording>,
}

impl Window {
    fn path(&self, id: PhotoId) -> PathBuf {
        let lib = self.d.app().session().unwrap().library();
        let p = lib.photo(id).unwrap();
        lib.folder(p.folder).unwrap().path.join(&p.name)
    }

    /// What the system was asked while `act` ran.
    fn calls(&mut self, act: impl FnOnce(&mut Driver)) -> Vec<OsCall> {
        self.os.clear();
        act(&mut self.d);
        self.d.settle(FRAMES).unwrap();
        self.os.calls()
    }

    fn photo_menu(&mut self, id: PhotoId) {
        let cell = format!("thumb:{}", id.get());
        self.d.click(&cell).unwrap();
        self.d.click_with(&cell, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
        self.d.settle(FRAMES).unwrap();
    }
}

pub fn by_hand(home: &Path) {
    let garden = home.join("Garden");
    std::fs::create_dir_all(&garden).unwrap();
    for (i, name) in ["IMG_0001.jpg", "IMG_0002.jpg"].iter().enumerate() {
        std::fs::write(garden.join(name), common::painted_jpeg(320, 240, COLOURS[i])).unwrap();
    }
    let mut session = Session::open(&home.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    session.execute("folders.add", &json!({ "path": garden })).unwrap();
    session.wait_idle();
    let editor = home.join("bin").join("editor");
    session.execute("options.set", &json!({ "values": { "files.externalEditor": editor } })).unwrap();
    let os = Arc::new(Recording::windowed());
    os.set_handlers("jpg", vec![Handler::new("gimp.desktop", "GIMP"), Handler::new("eog.desktop", "Image Viewer")]);
    session.set_os(os.clone());
    let mut w = Window { d: Driver::new(App::new(Some(session)), LARGE.0, LARGE.1, 1.0), os };
    w.d.settle(FRAMES).unwrap();
    let sections = &w.d.app().panels().unwrap().sections.list;
    let (photos, folder) = (sections[0].photos.clone(), sections[0].folder.unwrap());
    let (a, b) = (photos[0], photos[1]);
    let (pa, pb) = (w.path(a), w.path(b));

    let cell = |id: PhotoId| format!("thumb:{}", id.get());
    let calls = w.calls(|d| {
        d.click(&cell(a)).unwrap();
        d.key("Ctrl+Enter").unwrap();
    });
    assert_eq!(calls, [OsCall::Reveal(pa.clone())], "Ctrl+Enter: Locate on Disk");
    let icon = format!("section:{}/icon", dalica_ui::state::Section::Folder(folder));
    assert_eq!(w.calls(|d| d.click(&icon).unwrap()), [OsCall::Reveal(garden.clone())], "the folder header's icon");

    let calls = w.calls(|d| {
        d.click(&cell(a)).unwrap();
        d.click_with(&cell(b), &Click { modifiers: egui::Modifiers::COMMAND, ..Click::default() }).unwrap();
        d.key("Ctrl+Shift+O").unwrap();
    });
    let editor_name = editor.to_string_lossy().into_owned();
    assert_eq!(calls, [OsCall::OpenInEditor { paths: vec![pa.clone(), pb.clone()], editor: editor_name }], "Ctrl+Shift+O");
    assert_eq!(w.calls(|d| d.key("Ctrl+C").unwrap()), [OsCall::PutOnClipboard(Clip::Files(vec![pa.clone(), pb.clone()]))], "Ctrl+C");

    w.photo_menu(b);
    assert_eq!(w.calls(|d| d.click("submenu:Photo/Open With").unwrap()), [OsCall::Handlers(pb.clone())], "Open With ▸ asks the system");
    let row = w.d.widgets().into_iter().find(|x| x.id == "menuitem:photo.openWith/1").expect("the second application listed");
    assert_eq!(row.label, "Image Viewer");
    assert_eq!(w.calls(|d| d.click("menuitem:photo.openWith/1").unwrap()), [OsCall::OpenWith { path: pb.clone(), handler: "eog.desktop".into() }]);

    w.photo_menu(a);
    let path_text = pa.to_string_lossy().into_owned();
    assert_eq!(w.calls(|d| d.click("menuitem:photo.copyPath").unwrap()), [OsCall::PutOnClipboard(Clip::Text(path_text))], "Copy Path");
}

pub fn over_automation(exe: &Path, home: &Path) {
    let garden = home.join("Garden");
    std::fs::create_dir_all(&garden).unwrap();
    std::fs::write(garden.join("IMG_0001.jpg"), common::painted_jpeg(320, 240, COLOURS[0])).unwrap();
    let (app_, mut c) = live::start_with(exe, home, Launch::default().arg("--automation-allow-external"));
    run(&mut c, "folders.add", json!({ "path": garden }));
    idle(&mut c);
    let photo = c.photos()[0]["id"].as_u64().unwrap();
    let folder = c.inspect(&json!(photo))["folder"].as_u64().unwrap();
    for (command, params) in [
        ("photo.locate", json!({ "id": photo })),
        ("folder.locate", json!({ "id": folder })),
        ("photo.openWith", json!({ "id": photo, "handler": "eog.desktop" })),
        ("photo.openExternal", json!({ "ids": [photo] })),
    ] {
        let r = c.refused(command, params);
        assert_eq!(r["code"], "external_denied", "{command}: {r}");
        assert!(r["error"].as_str().unwrap().contains("never runs over automation"), "{command} says why: {r}");
    }
    let listed = c.exec("photo.openWithList", json!({ "id": photo }));
    assert!(listed["handlers"].is_array(), "listing the applications launches nothing: {listed}");
    for command in ["photo.copyPath", "photo.copyToSystem"] {
        let r = c.refused(command, json!({ "ids": [photo] }));
        assert_eq!((&r["code"], &r["error"]), (&json!("disabled"), &json!(dalica_engine::os::NO_CLIPBOARD)), "{command}: {r}");
    }
    // A control request's key and click are automation too.
    c.exec("select.set", json!({ "ids": [photo] }));
    c.ok("ui.key", json!({ "combo": "Ctrl+Enter" }));
    idle(&mut c);
    let status: Value = c.state()["status"].clone();
    assert!(status.as_str().is_some_and(|s| s.contains("never runs over automation")), "Ctrl+Enter over the channel: {status}");
    live::quit(app_, c);
}
