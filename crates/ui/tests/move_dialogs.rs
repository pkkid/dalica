//! The move and copy questions (M9.19, `organizing.md` §9), driven headlessly by widget id on temp
//! folders `Lisbon` (`IMG_0001.jpg` … `IMG_0003.jpg`) and `Porto` (`IMG_0001.jpg`):
//!
//! - Paste after Cut asks "Move these files on disk?" while "Ask before moving" is on: Cancel moves
//!   nothing and asks again next time; Move with "Don't ask again" ticked moves the files and turns
//!   the setting off, and the next move runs without a question;
//! - a move or copy whose names clash in the folder asks the collision question naming the files:
//!   Cancel moves nothing (and a "Don't ask again" ticked under it is not applied), Skip Duplicates
//!   leaves the clashing photo where it is and the folder's own file untouched, Rename Duplicates
//!   moves (or copies) it under the next free name; with the setting off the collision question
//!   comes straight away;
//! - the three dialogs match their fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//!   (`tests/fingerprints/dialog-{move,copy,collision}-1400x900.json`; `DALICA_BLESS=1` stores new
//!   ones).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, put_jpeg, try_widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::dialogs::{collision, copy_confirm, move_confirm};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct Fx {
    _dir: tempfile::TempDir,
    lisbon: PathBuf,
    porto: PathBuf,
    lisbon_id: FolderId,
    porto_id: FolderId,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let (lisbon, porto) = (dir.path().join("photos").join("Lisbon"), dir.path().join("photos").join("Porto"));
    for i in 1..=3 {
        put_jpeg(&lisbon.join(format!("IMG_000{i}.jpg")), i);
    }
    put_jpeg(&porto.join("IMG_0001.jpg"), 10);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in [&lisbon, &porto] {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    let (lisbon_id, porto_id) = (s.library().folder_by_path(&lisbon).unwrap(), s.library().folder_by_path(&porto).unwrap());
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, porto, lisbon_id, porto_id, d }
}

impl Fx {
    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    /// The photo called `name` in `folder`.
    fn photo(&self, folder: FolderId, name: &str) -> PhotoId {
        self.session().library().photos_in(folder).into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no {name}")).id
    }

    /// The names in `folder` as the library holds them, sorted.
    fn names(&self, folder: FolderId) -> Vec<String> {
        let mut v: Vec<String> = self.session().library().photos_in(folder).into_iter().map(|p| p.name.clone()).collect();
        v.sort();
        v
    }

    fn asks_before_moving(&self) -> bool {
        self.session().settings().organize().ask_before_moving
    }

    fn top(&self) -> Option<DialogKind> {
        self.d.state().dialogs.last().map(|d| d.kind)
    }

    /// Cut (or Copy) `ids`, then Paste into `folder`; the reply.
    fn paste(&mut self, cut: bool, ids: &[PhotoId], folder: FolderId) -> Value {
        self.d.run(if cut { "photo.cut" } else { "photo.copy" }, &json!({ "ids": ids })).unwrap();
        let reply = self.d.run("photo.paste", &json!({ "folder": folder })).unwrap();
        self.d.settle(FRAMES).unwrap();
        reply
    }

    /// Clicks `id`, then waits for what it started (file jobs included).
    fn press(&mut self, id: &str) {
        self.d.click(id).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
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

/// Paste after Cut asks while "Ask before moving" is on; with the box ticked it never asks again.
#[test]
fn a_move_asks_once_and_with_the_box_ticked_never_again() {
    let mut fx = fixture((1400, 900));
    let (two, three) = (fx.photo(fx.lisbon_id, "IMG_0002.jpg"), fx.photo(fx.lisbon_id, "IMG_0003.jpg"));
    assert!(fx.asks_before_moving());
    assert_eq!(fx.paste(true, &[two, three], fx.porto_id), json!({ "dialog": "moveFiles" }));
    assert!(try_widget(&fx.d, "dialog:moveFiles").is_some());
    let inspect = dalica_ui::dialogs::inspect(fx.d.app());
    assert_eq!((inspect["title"].as_str(), inspect["default"].as_str()), (Some(move_confirm::TITLE), Some("button:dialog.move")));
    assert!(try_widget(&fx.d, move_confirm::DONT_ASK_ID).is_some());
    // Cancel moves nothing and the next move asks again.
    fx.press("button:dialog.cancel");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(fx.names(fx.porto_id), ["IMG_0001.jpg"]);
    assert!(fx.asks_before_moving());
    assert_eq!(fx.paste(true, &[two, three], fx.porto_id), json!({ "dialog": "moveFiles" }));
    // Ticked, Move moves the files and the setting goes off.
    fx.d.click(move_confirm::DONT_ASK_ID).unwrap();
    assert_eq!(fx.d.state().move_files.as_ref().map(|q| q.dont_ask), Some(true));
    fx.press("button:dialog.move");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(fx.names(fx.porto_id), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert!(!fx.asks_before_moving(), "Don't ask again turned the question off");
    // Never again: the next paste moves at once.
    let reply = fx.paste(true, &[two], fx.lisbon_id);
    assert!(reply.get("job").is_some() && reply.get("dialog").is_none(), "{reply}");
    assert_eq!(fx.top(), None);
    assert!(fx.d.settle_for(FRAMES, true).settled);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    assert_eq!(fx.photo(fx.lisbon_id, "IMG_0002.jpg"), two, "the photo keeps its id");
}

/// A clash asks the collision question after the move question; Cancel, Skip and Rename each do
/// what they say.
#[test]
fn a_clash_offers_both_choices_and_each_does_what_it_says() {
    let mut fx = fixture((1400, 900));
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    let porto_bytes = std::fs::read(fx.porto.join("IMG_0001.jpg")).unwrap();
    let lisbon_bytes = std::fs::read(fx.lisbon.join("IMG_0001.jpg")).unwrap();
    // Move, ticked: the collision question names the file; Cancel moves nothing and keeps asking.
    fx.paste(true, &[one], fx.porto_id);
    fx.d.click(move_confirm::DONT_ASK_ID).unwrap();
    fx.press("button:dialog.move");
    assert_eq!(fx.top(), Some(DialogKind::Collision));
    assert_eq!(fx.d.state().collision.as_ref().map(|q| q.files.clone()), Some(vec!["IMG_0001.jpg".to_string()]));
    let v = dalica_ui::dialogs::inspect(fx.d.app());
    assert_eq!(v["title"], collision::TITLE);
    assert_eq!(v["buttons"], json!(["button:dialog.renameDuplicates", "button:dialog.skipDuplicates", "button:dialog.cancel"]));
    assert_eq!(v["command"]["params"]["dontAsk"], json!(true), "the box rides along");
    fx.press("button:dialog.cancel");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg"]);
    assert!(fx.asks_before_moving(), "Cancel applied nothing");
    // Skip Duplicates: the clashing photo stays, the folder's own file is untouched.
    fx.paste(true, &[one], fx.porto_id);
    fx.press("button:dialog.move");
    assert_eq!(fx.top(), Some(DialogKind::Collision));
    fx.press("button:dialog.skipDuplicates");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(std::fs::read(fx.porto.join("IMG_0001.jpg")).unwrap(), porto_bytes);
    assert_eq!(fx.session().library().photo(one).map(|p| p.folder), Some(fx.lisbon_id));
    // Rename Duplicates: moved under the next free name, and the ticked box now applies.
    fx.paste(true, &[one], fx.porto_id);
    fx.d.click(move_confirm::DONT_ASK_ID).unwrap();
    fx.press("button:dialog.move");
    fx.press("button:dialog.renameDuplicates");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001-001.jpg", "IMG_0001.jpg"]);
    assert_eq!(std::fs::read(fx.porto.join("IMG_0001-001.jpg")).unwrap(), lisbon_bytes);
    assert_eq!(std::fs::read(fx.porto.join("IMG_0001.jpg")).unwrap(), porto_bytes);
    assert_eq!(fx.photo(fx.porto_id, "IMG_0001-001.jpg"), one, "moved, keeping its id");
    assert!(!fx.asks_before_moving());
}

/// Copy asks "Copy 1 file to Porto?"; its Copy asks the collision question, whose Rename copies
/// under the next free name and whose Skip copies nothing.
#[test]
fn a_copy_asks_then_the_clash_question_follows() {
    let mut fx = fixture((1400, 900));
    let one = fx.photo(fx.lisbon_id, "IMG_0001.jpg");
    assert_eq!(fx.paste(false, &[one], fx.porto_id), json!({ "dialog": "copyFiles" }));
    assert!(try_widget(&fx.d, "dialog:copyFiles").is_some());
    assert_eq!(copy_confirm::question(1, "Porto"), "Copy 1 file to Porto?");
    fx.press("button:dialog.copy");
    assert_eq!(fx.top(), Some(DialogKind::Collision));
    fx.press("button:dialog.skipDuplicates");
    assert_eq!((files(&fx.lisbon).len(), files(&fx.porto)), (3, vec!["IMG_0001.jpg".to_string()]));
    fx.paste(false, &[one], fx.porto_id);
    fx.press("button:dialog.copy");
    fx.press("button:dialog.renameDuplicates");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"], "a copy leaves the source");
    assert_eq!(files(&fx.porto), ["IMG_0001-001.jpg", "IMG_0001.jpg"]);
    assert_eq!(fx.names(fx.porto_id), ["IMG_0001-001.jpg", "IMG_0001.jpg"]);
    // Without a clash Copy copies at once; Cancel copies nothing.
    let two = fx.photo(fx.lisbon_id, "IMG_0002.jpg");
    fx.paste(false, &[two], fx.porto_id);
    fx.press("button:dialog.cancel");
    assert_eq!(files(&fx.porto).len(), 2);
    fx.paste(false, &[two], fx.porto_id);
    fx.press("button:dialog.copy");
    assert_eq!(fx.top(), None);
    assert_eq!(files(&fx.porto), ["IMG_0001-001.jpg", "IMG_0001.jpg", "IMG_0002.jpg"]);
}

/// With "Ask before moving" off, a clashing move asks the collision question straight away, and a
/// move without a clash runs at once.
#[test]
fn with_the_question_off_a_clash_still_asks() {
    let mut fx = fixture((1400, 900));
    let (one, two) = (fx.photo(fx.lisbon_id, "IMG_0001.jpg"), fx.photo(fx.lisbon_id, "IMG_0002.jpg"));
    fx.d.app_mut().session_mut().unwrap().change_settings(|s| {
        let o = dalica_engine::settings::OrganizeSettings { ask_before_moving: false, ..s.organize() };
        s.set_organize(&o);
    });
    let reply = dalica_ui::commands::files::move_photos(fx.d.app_mut(), vec![one, two], fx.porto_id).unwrap();
    assert_eq!(reply, json!({ "dialog": "collision" }));
    fx.d.settle(FRAMES).unwrap();
    fx.press("button:dialog.skipDuplicates");
    assert_eq!(files(&fx.lisbon), ["IMG_0001.jpg", "IMG_0003.jpg"]);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    let three = fx.photo(fx.lisbon_id, "IMG_0003.jpg");
    let reply = dalica_ui::commands::files::move_photos(fx.d.app_mut(), vec![three], fx.porto_id).unwrap();
    assert!(reply.get("job").is_some(), "{reply}");
    assert!(fx.d.settle_for(FRAMES, true).settled);
    assert_eq!(files(&fx.porto), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
}

/// The move, copy and collision questions match their fingerprints, and fit the small window.
#[test]
fn the_dialogs_match_their_fingerprints_and_fit_the_small_window() {
    let mut fx = fixture(LARGE);
    let ids: Vec<PhotoId> = ["IMG_0001.jpg", "IMG_0002.jpg"].iter().map(|n| fx.photo(fx.lisbon_id, n)).collect();
    fx.paste(true, &ids, fx.porto_id);
    check_sizes(&mut fx.d, "dialog:moveFiles", "dialog-move");
    fx.press("button:dialog.move");
    assert_eq!(fx.top(), Some(DialogKind::Collision));
    check_sizes(&mut fx.d, "dialog:collision", "dialog-collision");
    fx.press("button:dialog.cancel");
    fx.paste(false, &ids, fx.porto_id);
    check_sizes(&mut fx.d, "dialog:copyFiles", "dialog-copy");
}
