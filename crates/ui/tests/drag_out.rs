//! Photos dragged out of the window to other applications (M15.23, `FILE-DRAGOUT`,
//! `ui-layout.md` §5.5), driven headlessly with a recording `DragOut` on temp folders `Lisbon`
//! (`IMG_0001.jpg` … `IMG_0003.jpg`) and `Porto` (`PORTO_0001.jpg`):
//!
//! - three thumbnails dragged out through the window's edge (over a folder on the way) hand their
//!   three files to the OS once, with the pill as the image, and released outside, no inside drop
//!   target fires;
//! - a drag released inside stays an inside drop, and one that left and came back in drops inside
//!   too (the OS's drag is cancelled);
//! - a photo with unsaved edits shows "Unsaved edits stay in Dalica" under the pill, and the note
//!   goes out with the drag;
//! - an OS that cannot drag files out leaves the drag inside and says why on the status line;
//! - a photo dragged from the edit view's filmstrip goes out alone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::common::{FRAMES, new_driver, put_jpeg, shot, try_widget, write_png};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::drag::out::{DragOut, Outgoing, UNSAVED_NOTE};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, pos2};
use serde_json::{Value, json};

/// A `DragOut` that records what it was handed; refuses with `refuse` when set.
#[derive(Default)]
struct Recorder {
    started: Mutex<Vec<Outgoing>>,
    cancels: AtomicUsize,
    refuse: Option<String>,
}

impl DragOut for Recorder {
    fn start(&self, out: Outgoing) -> Result<(), String> {
        if let Some(why) = &self.refuse {
            return Err(why.clone());
        }
        self.started.lock().unwrap().push(out);
        Ok(())
    }

    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }
}

impl Recorder {
    fn started(&self) -> Vec<Outgoing> {
        self.started.lock().unwrap().clone()
    }

    fn cancels(&self) -> usize {
        self.cancels.load(Ordering::SeqCst)
    }
}

/// Left of the window, level with the tree: out through its left edge.
const OUT: Pos2 = pos2(-40.0, 450.0);

struct Fx {
    _dir: tempfile::TempDir,
    lisbon: PathBuf,
    porto: PathBuf,
    lisbon_id: FolderId,
    porto_id: FolderId,
    d: Driver,
    out: Arc<Recorder>,
}

fn fixture(out: Recorder) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let (lisbon, porto) = (dir.path().join("photos").join("Lisbon"), dir.path().join("photos").join("Porto"));
    for i in 1..=3 {
        put_jpeg(&lisbon.join(format!("IMG_000{i}.jpg")), i);
    }
    put_jpeg(&porto.join("PORTO_0001.jpg"), 10);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in [&lisbon, &porto] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let (lisbon_id, porto_id) = (s.library().folder_by_path(&lisbon).unwrap(), s.library().folder_by_path(&porto).unwrap());
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    let out = Arc::new(out);
    dalica_ui::drag::out::install(d.ctx(), out.clone());
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, porto, lisbon_id, porto_id, d, out }
}

impl Fx {
    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    fn photo(&self, folder: FolderId, name: &str) -> PhotoId {
        self.session().library().photos_in(folder).into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no {name}")).id
    }

    fn center(&self, id: &str) -> Pos2 {
        try_widget(&self.d, id).unwrap_or_else(|| panic!("no widget {id}")).center()
    }

    /// Presses on widget `from` and moves the pointer through `via`, 12 frames to each, keeping
    /// the button down.
    fn hold(&mut self, from: &str, via: &[Pos2]) {
        let start = self.center(from);
        let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
        let mut at = start;
        for &to in via {
            events.extend((1..=12).map(|i| PointerEvent { kind: PointerKind::Drag, pos: at.lerp(to, i as f32 / 12.0) }));
            at = to;
        }
        self.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    }

    /// Moves on to `via` with the button still down.
    fn more(&mut self, via: &[Pos2]) {
        let mut at = self.d.pointer_pos().unwrap();
        let mut events = Vec::new();
        for &to in via {
            events.extend((1..=12).map(|i| PointerEvent { kind: PointerKind::Drag, pos: at.lerp(to, i as f32 / 12.0) }));
            at = to;
        }
        self.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    }

    fn release(&mut self) {
        let pos = self.d.pointer_pos().unwrap();
        self.d.pointer(&[PointerEvent { kind: PointerKind::Up, pos }], PointerButton::Primary, Modifiers::NONE).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    fn drag(&self) -> Value {
        dalica_ui::drag::inspect(self.d.app())
    }

    fn top(&self) -> Option<DialogKind> {
        self.d.state().dialogs.last().map(|d| d.kind)
    }

    fn note(&self) -> Option<String> {
        try_widget(&self.d, "drag:note").map(|w| w.label)
    }
}

fn thumb(id: PhotoId) -> String {
    format!("thumb:{}", id.get())
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// Three selected thumbnails dragged over a folder and out through the window's left edge: the OS
/// gets their three files once (however long the pointer stays out), with the pill as its image;
/// released out there, the folder under the way never takes them.
#[test]
fn three_thumbnails_dragged_out_hand_their_files_once_and_drop_nothing_inside() {
    let mut fx = fixture(Recorder::default());
    let ids: Vec<PhotoId> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"].iter().map(|n| fx.photo(fx.lisbon_id, n)).collect();
    fx.d.run("select.set", &json!({ "ids": ids })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let row = format!("tree:folder:{}", fx.porto_id.get());
    let over = fx.center(&row);
    fx.hold(&thumb(ids[1]), &[over]);
    assert_eq!(fx.drag()["target"], json!(row), "on the way out the folder offers itself");
    assert!(fx.out.started().is_empty(), "nothing goes to the OS while inside");
    fx.more(&[OUT, OUT + egui::vec2(-30.0, 40.0)]);
    let started = fx.out.started();
    assert_eq!(started.len(), 1, "handed once");
    let out = &started[0];
    let want: Vec<PathBuf> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"].iter().map(|n| fx.lisbon.join(n)).collect();
    assert_eq!(out.paths, want, "the files on disk, in the drag's order");
    assert_eq!(out.pill, "3 photos");
    assert_eq!(out.note, None, "no photo has unsaved edits");
    let image = out.image.as_ref().expect("the pill as the drag's image");
    assert_eq!(image.height, 32, "{image:?}");
    assert_eq!(fx.drag()["target"], Value::Null, "outside, no inside target");
    assert!(try_widget(&fx.d, "drag:pill").is_none(), "the OS shows the drag: no pill at the window's edge");
    fx.release();
    assert_eq!(fx.drag(), Value::Null, "the drag ended");
    assert_eq!(fx.top(), None, "no move question: nothing was dropped inside");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["PORTO_0001.jpg"]);
    assert_eq!(fx.out.started().len(), 1);
    assert_eq!(fx.out.cancels(), 0, "released outside, the OS finishes its drag");
}

/// Released inside, a drag stays an inside drop and the OS hears nothing; one that left and came
/// back in drops inside too, and the OS's drag is cancelled.
#[test]
fn a_drag_released_inside_stays_an_inside_drop() {
    let mut fx = fixture(Recorder::default());
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    let row = format!("tree:folder:{}", fx.porto_id.get());
    let over = fx.center(&row);
    fx.hold(&thumb(one), &[over]);
    fx.release();
    assert_eq!(fx.top(), Some(DialogKind::MoveFiles), "the move asks first");
    assert!(fx.out.started().is_empty());
    fx.d.click("button:dialog.cancel").unwrap();
    fx.d.settle(FRAMES).unwrap();
    // Out and back in.
    fx.hold(&thumb(two), &[OUT]);
    assert_eq!(fx.out.started().len(), 1);
    fx.more(&[over]);
    assert_eq!(fx.drag()["pill"], json!("Move 1 photo to Porto"), "back inside, an inside drag again");
    fx.release();
    assert_eq!(fx.top(), Some(DialogKind::MoveFiles));
    assert_eq!(fx.out.cancels(), 1, "the OS's drag ends with nothing dropped");
    assert_eq!(fx.out.started().len(), 1);
}

/// A photo with unsaved edits: over nothing the pill says they stay in Dalica, and the note goes
/// out with the drag; over a target, or without edits, there is no note.
#[test]
fn a_photo_with_unsaved_edits_shows_the_note() {
    let mut fx = fixture(Recorder::default());
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    fx.d.app_mut().session_mut().unwrap().execute("photo.history.apply", &json!({ "id": one, "op": "autofix()" })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    // Over the light table's empty room below the cells: no target.
    let table = try_widget(&fx.d, "region:lightTable").unwrap().rect;
    let empty = pos2(table[0] + table[2] / 2.0, table[1] + table[3] - 20.0);
    fx.hold(&thumb(two), &[empty]);
    assert_eq!(fx.drag()["target"], Value::Null);
    assert_eq!(fx.note(), None, "no edits, no note");
    shot(&fx.d, "drag-out-plain-1400x900");
    fx.release();
    fx.hold(&thumb(one), &[empty]);
    assert_eq!(fx.note().as_deref(), Some(UNSAVED_NOTE));
    shot(&fx.d, "drag-out-note-1400x900");
    let row = format!("tree:folder:{}", fx.porto_id.get());
    let over = fx.center(&row);
    fx.more(&[over]);
    assert_eq!(fx.note(), None, "over a folder the pill says what the drop does");
    fx.more(&[OUT]);
    let started = fx.out.started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].note.as_deref(), Some(UNSAVED_NOTE));
    assert_eq!(started[0].paths, [fx.lisbon.join("IMG_0001.jpg")], "the file on disk, without the edits");
    let image = started[0].image.as_ref().expect("the drag's image");
    assert!(image.height > 32, "the note is drawn under the pill");
    write_png("drag-out-image", &dalica_scenes::fingerprint::png_rgba(image.width, image.height, &image.rgba));
    fx.release();
    assert_eq!(fx.top(), None);
}

/// An OS with no way to drag files out: the drag stays inside, the status line says why, and
/// released outside nothing happens.
#[test]
fn an_os_that_cannot_drag_out_says_why() {
    let why = "Photos can't be dragged out of Dalica here; Copy puts their files on the clipboard.";
    let mut fx = fixture(Recorder { refuse: Some(why.into()), ..Recorder::default() });
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    fx.hold(&thumb(one), &[OUT]);
    assert_eq!(fx.d.state().status, why);
    assert!(fx.out.started().is_empty());
    fx.release();
    assert_eq!(fx.drag(), Value::Null);
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
}

/// From the edit view's filmstrip a photo goes out alone; the edit view stays.
#[test]
fn a_photo_dragged_from_the_filmstrip_goes_out() {
    let mut fx = fixture(Recorder::default());
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    fx.d.run("view.edit", &json!({ "id": one })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.hold(&format!("film:{}", two.get()), &[pos2(700.0, 450.0), OUT]);
    assert_eq!(fx.drag()["from"], json!(format!("film:{}", two.get())));
    let started = fx.out.started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].paths, [fx.lisbon.join("IMG_0002.jpg")]);
    fx.release();
    assert_eq!(fx.drag(), Value::Null);
    assert_eq!(fx.d.state().view, dalica_ui::state::View::Edit);
    // A click on a cell still opens its photo.
    fx.d.click(&format!("film:{}", two.get())).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.out.started().len(), 1);
}
