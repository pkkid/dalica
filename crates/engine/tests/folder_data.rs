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
//! - a second damage after a reset keeps both copies, each under its own name;
//! - the photo files are held too (M10.57): a caption set in the damaged folder leaves each photo
//!   file's bytes and time as they were, at close and after a restart, without keeping the session
//!   busy; the reset sends the held writes (one held before a restart included); a photo moved out
//!   of the folder before the reset has its held write dropped and reported; a write queued before
//!   the damage was found is held as well.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::common::{jpeg, names, off_owner, open, open_with, read, read_json, virtual_clock, wait_files};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const BROKEN: &[u8] = b"{\"photos\": {\"IMG_0001.jpg\": {\"star\": tru";

const WAIT: Duration = Duration::from_secs(60);

struct Fx {
    lib: tempfile::TempDir,
    disk: tempfile::TempDir,
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
    Fx { lib, disk, folder, s, id, photos: photos.into_iter().map(|(_, id)| id).collect() }
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

/// The bytes and modification time of `path` (read off the owner thread).
fn stamp(path: &Path) -> (Vec<u8>, SystemTime) {
    let path = path.to_path_buf();
    off_owner(move || (std::fs::read(&path).unwrap(), std::fs::metadata(&path).unwrap().modified().unwrap()))
}

/// The stamps of the photo files in `folder`.
fn photo_files(folder: &Path) -> Vec<(Vec<u8>, SystemTime)> {
    ["IMG_0001.jpg", "IMG_0002.jpg"].iter().map(|n| stamp(&folder.join(n))).collect()
}

/// The caption the photo file at `path` holds.
fn file_caption(path: &Path) -> Option<String> {
    dalica_meta::read(&read(path), &dalica_meta::Limits::default()).unwrap().owned.caption
}

#[test]
fn a_damaged_folder_is_listed_writes_nothing_and_survives_a_restart() {
    let mut fx = damaged();
    assert_eq!(fx.s.damaged_folders(), [fx.id]);
    let copies = kept(&fx.folder);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(fx.s.damaged_folder(fx.id).unwrap(), [fx.folder.join(&copies[0])]);
    let files = photo_files(&fx.folder);
    caption(&mut fx.s, fx.photos[0], "Tram 28");
    fx.s.flush().unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(sidecar(&fx.folder), None, "nothing written into the damaged folder");
    assert_eq!(photo_files(&fx.folder), files, "nor into its photo files");
    assert_eq!(fx.s.pending_metadata_writes(), 1, "the photo's write is held");
    assert!(!fx.s.metadata_writes_busy(), "a held write keeps nobody busy");

    drop(fx.s);
    assert_eq!(photo_files(&fx.folder), files, "closing the session does not write it");
    let mut s = open(fx.lib.path());
    assert_eq!(s.damaged_folders(), [fx.id], "the list survives a restart");
    caption(&mut s, fx.photos[1], "Sunset");
    s.flush().unwrap();
    assert!(s.flush_metadata(WAIT));
    assert_eq!(sidecar(&fx.folder), None, "a restart does not start writing there");
    assert_eq!(photo_files(&fx.folder), files);
    assert_eq!(kept(&fx.folder), copies);
    // The reset sends both, the one held before the restart included.
    reset(&mut s, fx.id).unwrap();
    assert!(s.flush_metadata(WAIT));
    assert_eq!(file_caption(&fx.folder.join("IMG_0001.jpg")).as_deref(), Some("Tram 28"));
    assert_eq!(file_caption(&fx.folder.join("IMG_0002.jpg")).as_deref(), Some("Sunset"));
    assert_eq!(s.pending_metadata_writes(), 0);
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
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(file_caption(&fx.folder.join("IMG_0001.jpg")).as_deref(), Some("Set while damaged"), "the held write lands in the photo");
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

#[test]
fn a_photo_moved_out_before_the_reset_has_its_held_write_dropped_and_reported() {
    let mut fx = damaged();
    let porto = fx.disk.path().join("Porto");
    dalica_fs::create_dir_all(&porto).unwrap();
    dalica_fs::replace(&porto.join("PORTO_0001.jpg"), &jpeg(7), Expect::Absent).unwrap();
    fx.s.execute("folders.add", &json!({ "path": porto })).unwrap();
    fx.s.wait_idle();
    let porto_id = fx.s.library().folder_by_path(&porto).unwrap();
    caption(&mut fx.s, fx.photos[0], "Held, then moved");
    fx.s.execute("photo.moveTo", &json!({ "ids": [fx.photos[0]], "folder": porto_id, "confirm": true })).unwrap();
    wait_files(&mut fx.s);
    let _ = fx.s.flush();
    let moved = porto.join("IMG_0001.jpg");
    let before = stamp(&moved);

    reset(&mut fx.s, fx.id).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(stamp(&moved), before, "the dropped write is not made");
    assert_eq!(fx.s.pending_metadata_writes(), 0, "nothing is held any more");
    let reported: Vec<String> = fx.s.metadata_events().filter(|e| e.photo == fx.photos[0]).map(|e| e.message()).collect();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("caption") && reported[0].contains("left Lisbon"), "{reported:?}");
    assert_eq!(fx.s.library().info(fx.photos[0]).caption, "Held, then moved", "the library keeps the value");
}

#[test]
fn a_write_queued_before_the_damage_was_found_is_held_too() {
    // A healthy folder on a clock that never runs, so the caption's write waits in the queue.
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..2 {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i), Expect::Absent).unwrap();
    }
    let mut s = open_with(lib.path(), virtual_clock());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.flush().unwrap();
    let id = s.library().folders().next().unwrap().id;
    let photo = s.library().photo_by_name(id, "IMG_0001.jpg").unwrap().id;
    let files = photo_files(&folder);
    // The sidecar is damaged behind Dalica's back: the caption's write is queued before the
    // caption's sidecar write finds the damage.
    let path = folder.join(".dalica.json");
    let now = dalica_fs::identity(&path).unwrap();
    dalica_fs::replace(&path, BROKEN, now.map_or(Expect::Absent, Expect::Unchanged)).unwrap();
    caption(&mut s, photo, "Queued first");
    assert_eq!(s.pending_metadata_writes(), 1);
    assert!(s.flush().is_err(), "the damage is reported");
    assert_eq!(s.damaged_folders(), [id]);

    assert!(s.flush_metadata(WAIT));
    s.poll();
    assert_eq!(photo_files(&folder), files, "the queued write is held, not made");
    assert_eq!(s.pending_metadata_writes(), 1);
    assert!(!s.metadata_writes_busy());
    reset(&mut s, id).unwrap();
    assert!(s.flush_metadata(WAIT));
    assert_eq!(file_caption(&folder.join("IMG_0001.jpg")).as_deref(), Some("Queued first"));
}
