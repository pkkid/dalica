//! Closing the app (`App::shutdown`, the window's close and the headless `app.quit` alike), driven
//! headlessly with writes still pending: the caption typed in the inline editor (or in the edit
//! view's caption bar) and not yet committed, a section description being typed, a caption whose metadata write still waits for
//! its debounce, and a view state changed since it was last saved. After the shutdown the folder
//! sidecar holds both texts, the photo file holds the caption, `settings.json` holds the view
//! state, and the reopened library shows the typed caption.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, put_jpeg};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::App;
use dalica_ui::panels::edit::caption_bar;
use dalica_ui::panels::light_table::caption_edit::FIELD_ID;
use dalica_ui::state::{DescriptionEdit, Section};
use serde_json::{Value, json};

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The caption written into the photo file itself.
fn file_caption(path: &Path) -> Option<String> {
    dalica_meta::read(&std::fs::read(path).unwrap(), &dalica_meta::Limits::default()).unwrap().owned.caption
}

#[test]
fn closing_with_a_caption_still_pending_leaves_it_in_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let (library, folder): (PathBuf, PathBuf) = (dir.path().join("library"), dir.path().join("photos").join("Lisbon"));
    for i in 1..=3 {
        put_jpeg(&folder.join(format!("IMG_000{i}.jpg")), i);
    }
    let mut s = Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let folder_id: FolderId = s.library().folder_by_path(&folder).unwrap();
    let mut named: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    let ids: Vec<PhotoId> = named.into_iter().map(|(_, id)| id).collect();
    // The window's app: its view state is persisted.
    let mut app = App::new(Some(s));
    app.persist_view();
    let mut d = new_driver(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();

    // A caption set a moment ago: its write into the photo file waits for the debounce.
    d.run("photo.caption.set", &json!({ "ids": [ids[0].get()], "caption": "Alfama" })).unwrap();
    assert!(d.app().session().unwrap().pending_metadata_writes() > 0, "the write is queued, not made");
    // A caption typed in the inline editor, not committed.
    d.run("select.set", &json!({ "ids": [ids[1].get()] })).unwrap();
    d.run("photo.caption.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(FIELD_ID).unwrap();
    d.type_text("Tram 28").unwrap();
    assert_eq!(d.state().caption_edit.as_ref().map(|e| e.draft.as_str()), Some("Tram 28"));
    assert_eq!(d.app().session().unwrap().library().caption(ids[1]), "", "nothing committed yet");
    // A section description being typed, and a tree width no frame has saved yet.
    let app = d.app_mut();
    app.state_mut().description_edit = Some(DescriptionEdit { section: Section::Folder(folder_id), draft: "Spring in Lisbon".into() });
    app.state_mut().tree_width = 321.0;

    // The window closes: no more frames, then the shutdown.
    let mut app = d.into_app();
    app.shutdown().unwrap();
    assert_eq!((app.state().caption_edit.as_ref(), app.state().description_edit.as_ref()), (None, None), "the editors are closed");
    let session = app.session().unwrap();
    assert_eq!(session.pending_metadata_writes(), 0, "every queued metadata write was made");
    assert_eq!(session.library().caption(ids[1]), "Tram 28");

    let sidecar = read_json(&folder.join(".dalica.json"));
    assert_eq!(sidecar["photos"]["IMG_0001.jpg"]["caption"], "Alfama");
    assert_eq!(sidecar["photos"]["IMG_0002.jpg"]["caption"], "Tram 28", "the typed caption is in the sidecar");
    assert!(sidecar.to_string().contains("Spring in Lisbon"), "the typed description is in the sidecar: {sidecar}");
    assert_eq!(file_caption(&folder.join("IMG_0001.jpg")).as_deref(), Some("Alfama"), "the photo file holds its caption");
    assert_eq!(read_json(&library.join("settings.json"))["view"]["treeWidth"], json!(321.0), "the view state is saved");
    drop(app);

    // The reopened library starts where this one stopped.
    let s = Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    assert_eq!((s.library().caption(ids[0]), s.library().caption(ids[1])), ("Alfama", "Tram 28"));
}

/// The edit view's caption bar keeps what is typed in the view state, so closing with a caption
/// still being typed there commits it as the light table's editor is: it is in the sidecar.
#[test]
fn closing_while_typing_in_the_edit_views_caption_bar_keeps_the_caption() {
    let dir = tempfile::tempdir().unwrap();
    let (library, folder): (PathBuf, PathBuf) = (dir.path().join("library"), dir.path().join("photos").join("Porto"));
    put_jpeg(&folder.join("IMG_0001.jpg"), 1);
    let mut s = Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": id.get() })).unwrap();
    d.settle(FRAMES).unwrap();
    d.click(caption_bar::FIELD_ID).unwrap();
    d.type_text("Ribeira").unwrap();
    assert_eq!(d.state().caption_bar.as_ref().map(|e| (e.photo, e.draft.as_str())), Some((id, "Ribeira")), "the draft is in the view state");
    assert_eq!(d.app().session().unwrap().library().caption(id), "", "nothing committed yet");

    let mut app = d.into_app();
    app.shutdown().unwrap();
    assert_eq!(app.state().caption_bar, None, "the draft is committed");
    assert_eq!(app.session().unwrap().library().caption(id), "Ribeira");
    assert_eq!(read_json(&folder.join(".dalica.json"))["photos"]["IMG_0001.jpg"]["caption"], "Ribeira", "the typed caption is in the sidecar");
}

/// Closing while the library is still opening has nothing to write.
#[test]
fn closing_before_the_library_opens_writes_nothing() {
    let mut app = App::new(None);
    assert!(app.shutdown().is_ok());
}

/// A clock that, once armed, holds the metadata writer's thread the next time it asks the time,
/// until it is opened: a metadata write held in flight, whatever the disk's speed.
#[derive(Default)]
struct WriterGate {
    armed: std::sync::Mutex<bool>,
    opened: std::sync::Condvar,
    reached: std::sync::atomic::AtomicBool,
}

impl WriterGate {
    fn arm(&self, on: bool) {
        *self.armed.lock().unwrap() = on;
        self.opened.notify_all();
    }

    fn reached(&self) -> bool {
        self.reached.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl dalica_core::Clock for WriterGate {
    fn now(&self) -> dalica_core::Timestamp {
        if std::thread::current().name() == Some("dalica-metadata-writes") {
            let armed = self.armed.lock().unwrap();
            if *armed {
                self.reached.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            drop(self.opened.wait_while(armed, |armed| *armed).unwrap());
        }
        dalica_core::SystemClock.now()
    }
}

/// The window's close (issue #723): with a metadata write held in flight, the frame that sees the
/// close request returns at once, hides the window and holds its close, while the write goes on
/// on the close's thread; once the write is let through it lands in the photo file and the
/// sidecar, the close finishes, and the next close request is let go.
#[test]
fn the_window_closes_at_once_while_a_held_metadata_write_still_lands() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let (library, folder) = (dir.path().join("library"), dir.path().join("photos").join("Sintra"));
    let photo = folder.join("IMG_0001.jpg");
    put_jpeg(&photo, 1);
    let gate = Arc::new(WriterGate::default());
    let mut s = Session::open(&library, gate.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    let mut app = App::new(Some(s));
    app.close_on_thread(Arc::new(|_| panic!("the window went in time")));
    let mut d = new_driver(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("photo.caption.set", &json!({ "ids": [id.get()], "caption": "Pena" })).unwrap();
    assert!(d.app().session().unwrap().pending_metadata_writes() > 0, "the write is queued");
    gate.arm(true);

    let mut app = d.into_app();
    let ctx = egui::Context::default();
    app.install(&ctx);
    let frame = |app: &mut App, close: bool| {
        let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
        if close {
            input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(input, |ui| {
            app.close_tick(ui.ctx());
            app.frame(ui);
        });
        out.textures_delta.clear();
        out.viewport_output.remove(&egui::ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()
    };
    let start = Instant::now();
    let commands = frame(&mut app, true);
    let returned = start.elapsed();
    assert!(returned < Duration::from_secs(2), "the close frame returned in {returned:?}");
    assert!(commands.contains(&egui::ViewportCommand::CancelClose), "held while the writes land: {commands:?}");
    assert!(commands.contains(&egui::ViewportCommand::Visible(false)), "the window hides at once: {commands:?}");
    assert!(app.session().is_none(), "the session went to the close's thread");
    // The writer is held: the close cannot finish, and a second close request is held too.
    let held = Instant::now();
    while !gate.reached() {
        assert!(held.elapsed() < Duration::from_secs(30), "the writer reaches the gate");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.closing().unwrap().finished());
    assert!(frame(&mut app, true).contains(&egui::ViewportCommand::CancelClose), "still held");
    assert_eq!(file_caption(&photo), None, "the write has not landed yet");

    gate.arm(false);
    let wait = Instant::now();
    while !app.closing().unwrap().finished() {
        assert!(wait.elapsed() < Duration::from_secs(60), "the close finishes once the write is let through");
        std::thread::sleep(Duration::from_millis(10));
    }
    let commands = frame(&mut app, false);
    assert!(commands.contains(&egui::ViewportCommand::Close), "the finished close lets the window go: {commands:?}");
    assert!(!frame(&mut app, true).contains(&egui::ViewportCommand::CancelClose), "and its close request is not held");
    app.finish_close().unwrap().unwrap();
    assert_eq!(file_caption(&photo).as_deref(), Some("Pena"), "the held write landed in the photo file");
    assert_eq!(read_json(&folder.join(".dalica.json"))["photos"]["IMG_0001.jpg"]["caption"], "Pena", "and in the sidecar");
}
