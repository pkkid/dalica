//! A damaged sidecar and `folder.resetData` (M10.52, `data-model.md` §3) on a temporary library
//! whose folder `Lisbon` holds two photos and an unparseable `.dalica.json`:
//!
//! - the folder is listed damaged once a write met the file (kept aside as
//!   `.dalica.json.corrupt-<time>`), a caption set there writes nothing, and the list survives
//!   reopening the session, which still writes nothing there;
//! - Reset asks first, keeps the kept copy byte for byte, writes a new sidecar with the model's
//!   values (the caption set while damaged included) and lifts the state; a folder that is not
//!   damaged is refused;
//! - a sidecar restored by hand before the reset is read into the model, not written over;
//! - a second damage after a reset keeps both copies, each under its own name.

use std::path::{Path, PathBuf};

use crate::common::{jpeg, names, open, read, read_json};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const BROKEN: &[u8] = b"{\"photos\": {\"IMG_0001.jpg\": {\"star\": tru";

struct Fx {
    lib: tempfile::TempDir,
    _disk: tempfile::TempDir,
    folder: PathBuf,
    s: Session,
    id: FolderId,
    photos: Vec<PhotoId>,
}

/// `Lisbon` with two photos and the unparseable sidecar [`BROKEN`], scanned and flushed: the
/// scan's write met the file and kept it aside.
fn damaged() -> Fx {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..2 {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i), Expect::Absent).unwrap();
    }
    dalica_fs::replace(&folder.join(".dalica.json"), BROKEN, Expect::Absent).unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    // The damaged file is reported (kept aside, the folder read-only).
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("damaged sidecar kept as")));
    let id = s.library().folders().next().unwrap().id;
    let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    Fx { lib, _disk: disk, folder, s, id, photos: photos.into_iter().map(|(_, id)| id).collect() }
}

/// The copies kept aside in `folder`, by name.
fn kept(folder: &Path) -> Vec<String> {
    names(folder).into_iter().filter(|n| n.starts_with(".dalica.json.corrupt-")).collect()
}

fn sidecar(folder: &Path) -> Option<Value> {
    read_json(&folder.join(".dalica.json"))
}

fn caption(s: &mut Session, id: PhotoId, text: &str) {
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": text })).unwrap();
}

fn reset(s: &mut Session, folder: FolderId) -> Result<Value, CmdError> {
    s.execute("folder.resetData", &json!({ "folder": folder, "confirm": true }))
}

#[test]
fn a_damaged_folder_is_listed_writes_nothing_and_survives_a_restart() {
    let mut fx = damaged();
    assert_eq!(fx.s.damaged_folders(), [fx.id]);
    let copies = kept(&fx.folder);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(fx.s.damaged_folder(fx.id).unwrap(), [fx.folder.join(&copies[0])]);
    caption(&mut fx.s, fx.photos[0], "Tram 28");
    fx.s.flush().unwrap();
    assert_eq!(sidecar(&fx.folder), None, "nothing written into the damaged folder");

    drop(fx.s);
    let mut s = open(fx.lib.path());
    assert_eq!(s.damaged_folders(), [fx.id], "the list survives a restart");
    caption(&mut s, fx.photos[1], "Sunset");
    s.flush().unwrap();
    assert_eq!(sidecar(&fx.folder), None, "a restart does not start writing there");
    assert_eq!(kept(&fx.folder), copies);
}

#[test]
fn reset_asks_keeps_the_copy_writes_the_models_values_and_lifts_the_state() {
    let mut fx = damaged();
    let copy = fx.folder.join(&kept(&fx.folder)[0]);
    caption(&mut fx.s, fx.photos[0], "Set while damaged");
    let asked = fx.s.execute("folder.resetData", &json!({ "folder": fx.id }));
    assert!(matches!(&asked, Err(CmdError::Disabled(m)) if m.contains("\"confirm\": true")), "{asked:?}");
    assert_eq!(fx.s.damaged_folders(), [fx.id], "asking changes nothing");

    let r = reset(&mut fx.s, fx.id).unwrap();
    assert_eq!(r, json!({ "folder": fx.id, "kept": [copy.display().to_string()], "restored": false, "written": true }));
    assert_eq!(read(&copy), BROKEN, "the kept copy is untouched");
    assert_eq!(sidecar(&fx.folder).unwrap()["photos"]["IMG_0001.jpg"]["caption"], "Set while damaged");
    assert!(fx.s.damaged_folders().is_empty());
    // Writes go on as for any folder, and the lifted state survives a restart.
    caption(&mut fx.s, fx.photos[1], "Sunset");
    fx.s.flush().unwrap();
    assert_eq!(sidecar(&fx.folder).unwrap()["photos"]["IMG_0002.jpg"]["caption"], "Sunset");
    drop(fx.s);
    let mut s = open(fx.lib.path());
    assert!(s.damaged_folders().is_empty());
    // A folder that is not damaged is refused.
    assert!(matches!(reset(&mut s, fx.id), Err(CmdError::Disabled(m)) if m.contains("not damaged")));
    assert_eq!(kept(&fx.folder).len(), 1);
}

#[test]
fn a_sidecar_restored_by_hand_is_read_not_written_over() {
    let mut fx = damaged();
    caption(&mut fx.s, fx.photos[0], "Set while damaged");
    fx.s.flush().unwrap();
    let restored = serde_json::to_vec_pretty(&json!({
        "dalica": { "version": 1 },
        "photos": { "IMG_0001.jpg": { "caption": "From the backup", "star": true }, "IMG_0002.jpg": { "tags": ["tram"] } },
        "zzz.other.app": { "kept": true }
    }))
    .unwrap();
    dalica_fs::replace(&fx.folder.join(".dalica.json"), &restored, Expect::Absent).unwrap();

    let r = reset(&mut fx.s, fx.id).unwrap();
    assert_eq!((&r["restored"], &r["written"]), (&json!(true), &json!(false)));
    let lib = fx.s.library();
    assert_eq!(lib.info(fx.photos[0]).caption, "From the backup");
    assert!(lib.photo(fx.photos[0]).unwrap().star);
    assert_eq!(lib.info(fx.photos[1]).tags, ["tram"]);
    assert!(fx.s.damaged_folders().is_empty());
    fx.s.flush().unwrap();
    let file = sidecar(&fx.folder).unwrap();
    assert_eq!(file["photos"]["IMG_0001.jpg"]["caption"], "From the backup");
    assert_eq!(file["zzz.other.app"], json!({ "kept": true }), "the restored file's other keys stay");
    assert_eq!(kept(&fx.folder).len(), 1);
}

#[test]
fn a_second_damage_keeps_both_copies() {
    let mut fx = damaged();
    let first = kept(&fx.folder);
    reset(&mut fx.s, fx.id).unwrap();
    // Damaged again: the next write keeps it aside under a name of its own.
    let again = b"{ not json either".as_slice();
    let path = fx.folder.join(".dalica.json");
    let now = dalica_fs::identity(&path).unwrap();
    dalica_fs::replace(&path, again, now.map_or(Expect::Absent, Expect::Unchanged)).unwrap();
    caption(&mut fx.s, fx.photos[0], "Again");
    assert!(fx.s.flush().is_err(), "the second damage is reported");
    assert_eq!(fx.s.damaged_folders(), [fx.id]);
    let both = kept(&fx.folder);
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both.contains(&first[0]));
    assert_eq!(read(&fx.folder.join(&first[0])), BROKEN);
    let second = both.iter().find(|n| **n != first[0]).unwrap();
    assert_eq!(read(&fx.folder.join(second)), again);
    assert_eq!(fx.s.damaged_folder(fx.id).unwrap().len(), 1, "this session's damage lists the copy it made");
    let r = reset(&mut fx.s, fx.id).unwrap();
    assert_eq!(r["kept"], json!([fx.folder.join(second).display().to_string()]));
    assert_eq!(sidecar(&fx.folder).unwrap()["photos"]["IMG_0001.jpg"]["caption"], "Again");
    assert_eq!(kept(&fx.folder).len(), 2);
}
