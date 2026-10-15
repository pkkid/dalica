//! Edit ▸ Cut, Copy and Paste (M9.23, `organizing.md` §9), driven headlessly by their shortcuts on
//! temp folders `Lisbon` (`IMG_0001.jpg`, `IMG_0002.jpg`) and `Porto` (`IMG_0001.jpg`): Cut then
//! Paste moves (asking the move question the first time, the collision question when names clash,
//! with the move question off too), Copy then Paste copies after "Copy %d files to %s?", Paste is
//! greyed in an album and with nothing cut or copied, and after Cut into the photos' own folder it
//! says so and moves nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::commands::clipboard;
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::state::{AlbumSection, DialogKind, Section, Selection};
use dalica_ui::{App, Driver};
use serde_json::json;

const FRAMES: usize = 60 * 60;

fn put_jpeg(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()).unwrap();
}

struct Fx {
    _dir: tempfile::TempDir,
    lisbon: PathBuf,
    porto: PathBuf,
    lisbon_id: FolderId,
    porto_id: FolderId,
    d: Driver,
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let (lisbon, porto) = (dir.path().join("photos").join("Lisbon"), dir.path().join("photos").join("Porto"));
    put_jpeg(&lisbon.join("IMG_0001.jpg"), 1);
    put_jpeg(&lisbon.join("IMG_0002.jpg"), 2);
    put_jpeg(&porto.join("IMG_0001.jpg"), 10);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in [&lisbon, &porto] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let (lisbon_id, porto_id) = (s.library().folder_by_path(&lisbon).unwrap(), s.library().folder_by_path(&porto).unwrap());
    let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, porto, lisbon_id, porto_id, d }
}

impl Fx {
    fn photo(&self, folder: FolderId, name: &str) -> PhotoId {
        let lib = self.d.app().session().unwrap().library();
        lib.photos_in(folder).into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no {name}")).id
    }

    /// Clicks photo `id`'s thumbnail: it alone is selected.
    fn select(&mut self, id: PhotoId) {
        self.d.click(&format!("thumb:{}", id.get())).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    /// Clicks `folder`'s tree row: the folder is current.
    fn open_folder(&mut self, folder: FolderId) {
        self.d.click(&format!("tree:folder:{}", folder.get())).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    fn key(&mut self, chord: &str) {
        self.d.key(chord).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    /// Clicks `id`, then waits for what it started (file jobs included).
    fn press(&mut self, id: &str) {
        self.d.click(id).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    fn top(&self) -> Option<DialogKind> {
        self.d.state().dialogs.last().map(|d| d.kind)
    }

    /// Edit ▸ Paste as the menu bar shows it now: enabled, and why not.
    fn paste_item(&self) -> (bool, Option<String>) {
        let bar = menus::bar(self.d.app(), Platform::current());
        let item = menus::find(&bar, &Lookup::Id { id: "photo.paste".into(), params: json!({}) }).cloned().unwrap();
        (item.enabled, item.reason)
    }
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

/// Ctrl+X, then Ctrl+V in another folder: the move question, then the photo moves; into the
/// photo's own folder Paste moves nothing.
#[test]
fn cut_then_paste_moves() {
    let mut fx = fixture();
    assert_eq!(fx.paste_item(), (false, Some(clipboard::NOTHING_KEPT.into())), "nothing cut yet");
    let two = fx.photo(fx.lisbon_id, "IMG_0002.jpg");
    fx.select(two);
    fx.key("Ctrl+X");
    assert!(fx.d.state().clipboard.is_some());
    // Into its own folder there is nothing to move: the reason, and no question.
    fx.key("Ctrl+V");
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!(fx.d.state().status, clipboard::ALREADY_THERE);
    fx.open_folder(fx.porto_id);
    assert_eq!(fx.paste_item(), (true, None));
    fx.key("Ctrl+V");
    assert_eq!(fx.top(), Some(DialogKind::MoveFiles), "the first move asks");
    fx.press("button:dialog.move");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    assert_eq!(fx.d.app().session().unwrap().library().photo(two).unwrap().folder, fx.porto_id);
}

/// Ctrl+C, then Ctrl+V in another folder: "Copy 1 files to Porto?", then a copy; the original
/// stays.
#[test]
fn copy_then_paste_copies() {
    let mut fx = fixture();
    let two = fx.photo(fx.lisbon_id, "IMG_0002.jpg");
    fx.select(two);
    fx.key("Ctrl+C");
    fx.open_folder(fx.porto_id);
    fx.key("Ctrl+V");
    assert_eq!(fx.top(), Some(DialogKind::CopyFiles));
    fx.press("button:dialog.copy");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    assert!(fx.d.state().clipboard.is_some(), "a copy can be pasted again");
}

/// With "Ask before moving" off, Paste after Cut onto a clashing name asks the collision question
/// before anything moves (it used to run the move straight away).
#[test]
fn paste_after_cut_asks_the_collision_question_with_the_move_question_off() {
    let mut fx = fixture();
    fx.d.app_mut().session_mut().unwrap().change_settings(|s| {
        let o = dalica_engine::settings::OrganizeSettings { ask_before_moving: false, ..s.organize() };
        s.set_organize(&o);
    });
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    fx.select(one);
    fx.key("Ctrl+X");
    fx.open_folder(fx.porto_id);
    fx.key("Ctrl+V");
    assert_eq!(fx.top(), Some(DialogKind::Collision));
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg"], "nothing moved yet");
    fx.press("button:dialog.renameDuplicates");
    assert_eq!(files(&fx.lisbon), ["IMG_0002.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001-001.jpg", "IMG_0001.jpg"]);
}

/// In an album Paste is greyed with its reason, and Ctrl+V does nothing.
#[test]
fn paste_is_disabled_in_an_album() {
    let mut fx = fixture();
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    fx.select(one);
    fx.key("Ctrl+C");
    fx.d.app_mut().state_mut().selection = Selection::new(vec![], None, Some(Section::Album(AlbumSection::Starred)), true);
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.paste_item(), (false, Some(clipboard::IN_ALBUM.into())));
    fx.key("Ctrl+V");
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!(fx.d.state().status, clipboard::IN_ALBUM);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg"]);
}
