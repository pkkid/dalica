//! A damaged folder's banner and Reset folder data (M10.52, `data-model.md` §3,
//! `ui-layout.md` §5.2), driven headlessly by widget id on a library of two folders, `Lisbon`
//! (whose `.dalica.json` cannot be parsed, so the scan's write kept it aside) and `Porto`:
//!
//! - the caution banner shows in the damaged folder's header block only;
//! - [Reset folder data] asks first (`dialog:resetFolderData`, naming the folder, with [Reset]
//!   [Cancel]); Cancel changes nothing; Reset runs `folder.resetData`, a new sidecar is written,
//!   the kept copy stays, and the banner goes.
//!
//! The banner and the question at 1400 × 900 go to `target/test-output/ui/` (and `DALICA_SHOTS`)
//! as `damaged-banner-1400x900.png` and `damaged-question-1400x900.png`, to look at.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, new_driver, shot, try_widget};
use dalica_core::FolderId;
use dalica_engine::Session;
use dalica_ui::dialogs::{self, confirm};
use dalica_ui::panels::light_table::folder_banner;
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::json;

const BROKEN: &[u8] = b"{\"photos\": {\"IMG_0001.jpg\": {\"star\": tru";

struct Fx {
    _dir: tempfile::TempDir,
    lisbon: PathBuf,
    damaged: FolderId,
    fine: FolderId,
    d: Driver,
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    let (lisbon, porto) = (photos.join("Lisbon"), photos.join("Porto"));
    for (folder, seed) in [(&lisbon, 1), (&porto, 10)] {
        std::fs::create_dir_all(folder).unwrap();
        for i in 0..2 {
            std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), jpeg(seed + i)).unwrap();
        }
    }
    std::fs::write(lisbon.join(".dalica.json"), BROKEN).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    assert!(s.flush().is_err(), "the damaged sidecar is reported");
    let damaged = s.library().folder_by_path(&lisbon).unwrap();
    let fine = s.library().folder_by_path(&porto).unwrap();
    assert_eq!(s.damaged_folders(), [damaged]);
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, damaged, fine, d }
}

fn kept(folder: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".dalica.json.corrupt-"))
        .collect();
    v.sort();
    v
}

#[test]
fn the_banner_asks_cancel_keeps_and_reset_lifts_the_folder() {
    let mut fx = fixture();
    let d = &mut fx.d;
    let banner = try_widget(d, &folder_banner::banner_id(fx.damaged)).expect("the damaged folder's banner");
    assert_eq!(banner.label, folder_banner::TITLE);
    let section = try_widget(d, &format!("section:folder:{}", fx.damaged)).unwrap();
    let ([sx0, sy0, sx1, sy1], [bx0, by0, bx1, by1]) = (section.rect, banner.rect);
    assert!(sx0 <= bx0 && sy0 <= by0 && bx1 <= sx1 && by1 <= sy1, "in the folder's header block: {:?} in {:?}", banner.rect, section.rect);
    assert!(try_widget(d, &folder_banner::banner_id(fx.fine)).is_none(), "only on the damaged folder");
    assert!(try_widget(d, &folder_banner::button_id(fx.fine)).is_none());
    shot(d, "damaged-banner-1400x900");

    // Reset folder data asks first.
    let button = folder_banner::button_id(fx.damaged);
    assert_eq!(try_widget(d, &button).unwrap().label, folder_banner::RESET_LABEL);
    d.click(&button).unwrap();
    d.settle(FRAMES).unwrap();
    let top = dialogs::top(d.app()).expect("a question is open");
    assert_eq!(top.kind, DialogKind::ResetFolderData);
    let text = confirm::question(d.app(), top);
    assert!(text.starts_with("Reset the data of the folder \"Lisbon\"?") && text.contains("The damaged file stays beside the photos"), "{text}");
    let v = dialogs::inspect(d.app());
    assert_eq!(v["buttons"], json!(["button:dialog.cancel", "button:dialog.reset"]));
    assert_eq!(try_widget(d, "button:dialog.reset").unwrap().label, "Reset");
    shot(d, "damaged-question-1400x900");

    // Cancel changes nothing.
    let copies = kept(&fx.lisbon);
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty());
    assert_eq!(d.app().session().unwrap().damaged_folders(), [fx.damaged]);
    assert!(!fx.lisbon.join(".dalica.json").exists());
    assert!(try_widget(d, &folder_banner::banner_id(fx.damaged)).is_some());

    // Reset runs folder.resetData: a new sidecar with the star set while damaged, the kept copy
    // untouched, the banner gone.
    let photo = d.app().session().unwrap().library().photos_in(fx.damaged)[0].id;
    d.run("photo.star.set", &json!({ "ids": [photo], "on": true })).unwrap();
    d.click(&button).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("button:dialog.reset").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty());
    assert!(d.app().session().unwrap().damaged_folders().is_empty());
    assert!(try_widget(d, &folder_banner::banner_id(fx.damaged)).is_none(), "the banner goes");
    assert_eq!(kept(&fx.lisbon), copies);
    assert_eq!(std::fs::read(fx.lisbon.join(&copies[0])).unwrap(), BROKEN);
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(fx.lisbon.join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(written["photos"]["IMG_0001.jpg"]["star"], true, "a new data file from the model");
}
