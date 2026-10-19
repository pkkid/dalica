//! Undo Save and Revert from the previous organizer's kept originals through the engine's
//! commands (`roadmap.md` M14.14; `data-model.md` §6 and §9): a scan finds the legacy originals;
//! Undo Save copies the original back and brings the legacy recipe back as unsaved edits, Revert
//! copies it back and drops the edits; the legacy originals folder is never written; a photo with
//! a record of Dalica's own uses Dalica's; the owner thread makes no `fs` call.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use dalica_core::PhotoId;
use dalica_engine::cmd::save::NOTHING_SAVED;
use dalica_engine::{CacheDir, CmdError, JobEntry, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_sidecar::legacy::{originals_dir_name, sidecar_names};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);
const AUTOFIX: &str = "autofix()";

/// A watched folder `Lisbon` where the previous organizer saved `IMG_0001.jpg` (edits `sepia`
/// and a red-eye marker) and `IMG_0002.jpg` (`bw`), keeping their originals; `IMG_0003.jpg` was
/// never saved.
struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    ids: Vec<PhotoId>,
    originals: Vec<Vec<u8>>,
    saved: Vec<Vec<u8>>,
}

fn jpeg(seed: u64, w: u32, h: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, w, h), 85, None).unwrap()
}

impl Lib {
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        let legacy = lisbon.join(originals_dir_name());
        dalica_fs::create_dir_all(&legacy).unwrap();
        let originals: Vec<Vec<u8>> = (0..3).map(|i| jpeg(40 + i, 320, 240)).collect();
        let saved: Vec<Vec<u8>> = (0..2).map(|i| jpeg(50 + i, 200, 150)).collect();
        for (i, bytes) in originals.iter().enumerate() {
            let name = format!("IMG_{:04}.jpg", i + 1);
            let at_name = saved.get(i).unwrap_or(bytes);
            dalica_fs::replace(&lisbon.join(&name), at_name, Expect::Absent).unwrap();
            if i < 2 {
                dalica_fs::replace(&legacy.join(&name), bytes, Expect::Absent).unwrap();
            }
        }
        let ini = "[IMG_0001.jpg]\r\nfilters=sepia=1;redeye=1;\r\nwidth=320\r\nheight=240\r\n[IMG_0002.jpg]\r\nfilters=bw=1;\r\n";
        dalica_fs::replace(&legacy.join(&sidecar_names()[0]), ini.as_bytes(), Expect::Absent).unwrap();
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), std::sync::Arc::new(dalica_core::SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        Lib { dir, s, ids: photos.into_iter().map(|(_, id)| id).collect(), originals, saved }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn path(&self, id: PhotoId) -> PathBuf {
        self.lisbon().join(&self.s.library().photo(id).unwrap().name)
    }

    fn edits(&self, id: PhotoId) -> Vec<String> {
        self.s.library().info(id).edits.clone()
    }

    /// Every file of the legacy originals folder with its bytes and time.
    fn legacy(&self) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
        let mut out: Vec<_> = std::fs::read_dir(self.lisbon().join(originals_dir_name()))
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.clone(), read(&p), std::fs::metadata(&p).unwrap().modified().unwrap())
            })
            .collect();
        out.sort();
        out
    }

    /// Runs `cmd` with `params`, waits for its job and checks every photo was done.
    fn job(&mut self, cmd: &str, params: Value) -> JobEntry {
        let reply = self.s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e:?}"));
        let job = serde_json::from_value(reply["job"].clone()).unwrap();
        let entry = self.s.wait_job(job, PATIENCE).unwrap();
        assert_eq!(entry.state, JobState::Done, "{cmd}: {entry:?}");
        assert_eq!(entry.result.as_ref().unwrap()["failed"], json!([]), "{cmd}: every photo done");
        entry
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn undo_save_brings_the_legacy_recipe_back_as_unsaved_edits_and_revert_drops_it() {
    let mut lib = Lib::new();
    let (a, b, c) = (lib.ids[0], lib.ids[1], lib.ids[2]);
    let legacy = lib.legacy();
    assert_eq!(lib.s.library().legacy_undo_saves().len(), 2, "the scan found both legacy originals");
    let recipe = lib.s.library().legacy_original(a).unwrap().recipe.clone();
    assert_eq!(recipe, ["sepia()", "legacy:redeye(1)"]);
    // A photo never saved has nothing to bring back.
    assert_eq!(lib.s.execute("photo.undoSave", &json!({ "ids": [c] })), Err(CmdError::Disabled(NOTHING_SAVED.into())));

    let ui = dalica_fs::mark_ui_thread();
    // Undo Save, with an edit made since: the original's bytes, the legacy recipe in front of it.
    lib.s.execute("photo.history.apply", &json!({ "id": a, "op": AUTOFIX })).unwrap();
    let done = lib.job("photo.undoSave", json!({ "ids": [a] }));
    assert_eq!(done.label, "Undid the save of 1 photo");
    // Revert: the original's bytes, no edits.
    let done = lib.job("photo.revert", json!({ "ids": [b], "confirm": true }));
    assert_eq!(done.label, "Reverted 1 photo");
    lib.s.wait_idle();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");

    assert_eq!(read(&lib.path(a)), lib.originals[0], "the original's exact bytes");
    assert_eq!(lib.edits(a), ["sepia()", "legacy:redeye(1)", AUTOFIX]);
    assert!(lib.s.library().has_unsaved_edits(a));
    assert_eq!(read(&lib.path(b)), lib.originals[1]);
    assert!(lib.edits(b).is_empty());
    for id in [a, b] {
        assert!(lib.s.library().legacy_original(id).is_none() && !lib.s.library().can_undo_legacy_save(id));
        assert_eq!(lib.s.library().photo(id).unwrap().size, std::fs::metadata(lib.path(id)).unwrap().len(), "the entry is refreshed");
    }
    assert_eq!(lib.legacy(), legacy, "nothing in the legacy originals folder was written, moved or removed");
    // Nothing more to bring back, and a rescan offers nothing again: the photos are their originals.
    assert_eq!(lib.s.execute("photo.revert", &json!({ "ids": [a, b], "confirm": true })), Err(CmdError::Disabled(NOTHING_SAVED.into())));
    let model = lib.s.library().model().clone();
    lib.s.start_verification();
    lib.s.wait_idle();
    assert!(lib.s.library().legacy_undo_saves().is_empty());
    assert_eq!(lib.s.library().model(), &model, "nothing changed by a rescan");
}

#[test]
fn a_photo_with_a_record_of_dalicas_uses_dalicas() {
    let mut lib = Lib::new();
    let a = lib.ids[0];
    let legacy = lib.legacy();
    lib.s.execute("photo.history.apply", &json!({ "id": a, "op": AUTOFIX })).unwrap();
    // The legacy recipe is not in the photo's edits, so only Dalica's own edit is saved.
    lib.job("photo.save", json!({ "ids": [a], "confirm": true, "dontAsk": true }));
    assert!(lib.s.library().saved(a).is_some() && !lib.s.library().can_undo_legacy_save(a));

    // Undo Save puts Dalica's kept original back (the file the previous organizer saved) with
    // Dalica's recipe; the legacy original is offered again afterwards.
    lib.job("photo.undoSave", json!({ "ids": [a] }));
    assert_eq!(read(&lib.path(a)), lib.saved[0], "Dalica's kept original");
    assert_eq!(lib.edits(a), [AUTOFIX]);
    assert!(lib.s.library().saved(a).is_none());
    assert!(lib.s.library().can_undo_legacy_save(a), "the legacy original is still there to offer");
    assert_eq!(lib.legacy(), legacy);
}
