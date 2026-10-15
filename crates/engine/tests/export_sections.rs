//! Export of album sections and search results (`roadmap.md` M10.31; `creating.md` §2):
//! `export.run` with `section` (or `album`) naming a user album, Starred Photos or the search
//! results exports that section's photos in its display order, and `export.defaults` suggests its
//! name; an album the library does not have, or the search results while not searching, fail
//! without starting a job. (Folder sections: `export.rs`.)

use std::path::PathBuf;
use std::time::Duration;

use crate::common::{exists, files, jpeg_of, open};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, JobKind, JobState, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(300);

/// A library watching `Photos`, whose folders `A` (`IMG_A1.jpg`, `IMG_A2.jpg`), `B`
/// (`harbour_1.jpg`, `harbour_2.jpg`) and `C` (`IMG_C1.jpg`) hold small photos.
struct Fx {
    dir: tempfile::TempDir,
    s: Session,
}

impl Fx {
    fn new() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos");
        let layout = [("A", "IMG_A1.jpg"), ("A", "IMG_A2.jpg"), ("B", "harbour_1.jpg"), ("B", "harbour_2.jpg"), ("C", "IMG_C1.jpg")];
        for (seed, (folder, name)) in layout.iter().enumerate() {
            let folder = photos.join(folder);
            dalica_fs::create_dir_all(&folder).unwrap();
            dalica_fs::replace(&folder.join(name), &jpeg_of(seed as u64, 64, 48), Expect::Absent).unwrap();
        }
        let mut s = open(&dir.path().join("library"));
        s.execute("folders.add", &json!({ "path": photos })).unwrap();
        s.wait_idle();
        Fx { dir, s }
    }

    /// The photo named `name`.
    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    /// Where exports go: outside the watched folder.
    fn out(&self) -> PathBuf {
        self.dir.path().join("Out")
    }

    /// Runs `export.run` with `target` into `Out/<folder_name>` (numbered, 100 px), waits for the
    /// job, and lists the files written.
    fn export(&mut self, target: Value, folder_name: &str) -> Vec<String> {
        let mut params = json!({ "dir": self.out(), "folderName": folder_name, "numbers": true, "size": 100 });
        for (k, v) in target.as_object().unwrap() {
            params[k] = v.clone();
        }
        let reply = self.s.execute("export.run", &params).unwrap();
        let entry = self.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
        assert_eq!(entry.state, JobState::Done, "{entry:?}");
        self.s.wait_idle();
        self.s.flush().unwrap(); // the folder's sidecar has landed: its temporary file is gone
        files(&self.out().join(folder_name))
    }

    fn folder_name(&mut self, target: Value) -> Value {
        self.s.execute("export.defaults", &target).unwrap()["folderName"].clone()
    }
}

fn code(r: Result<Value, CmdError>) -> &'static str {
    match r {
        Ok(v) => panic!("expected an error, got {v}"),
        Err(e) => e.code(),
    }
}

#[test]
fn an_album_over_three_folders_exports_in_album_order() {
    let mut fx = Fx::new();
    let order = [fx.id("IMG_C1.jpg"), fx.id("IMG_A2.jpg"), fx.id("harbour_1.jpg")];
    let album = fx.s.execute("album.create", &json!({ "name": "Lisbon Weekend", "ids": order })).unwrap()["album"].clone();
    let section = format!("album:{}", album.as_str().unwrap());
    assert_eq!(fx.folder_name(json!({ "section": section })), "Lisbon Weekend");
    assert_eq!(fx.s.execute("export.defaults", &json!({ "album": album })).unwrap()["count"], 3);
    let written = fx.export(json!({ "section": section }), "Lisbon Weekend");
    assert_eq!(written, ["001-IMG_C1.jpg", "002-IMG_A2.jpg", "003-harbour_1.jpg"]);
}

#[test]
fn the_search_results_export_the_matches() {
    let mut fx = Fx::new();
    fx.s.execute("search.set", &json!({ "text": "harbour" })).unwrap();
    fx.s.wait_search();
    assert_eq!(fx.folder_name(json!({ "section": "album:search" })), "harbour");
    let written = fx.export(json!({ "section": "album:search" }), "harbour");
    // The numbers follow the search's order; the names are the matches.
    let mut stems: Vec<&str> = written.iter().map(|n| n.split_once('-').unwrap().1).collect();
    stems.sort_unstable();
    assert_eq!(stems, ["harbour_1.jpg", "harbour_2.jpg"], "{written:?}");
}

#[test]
fn starred_photos_export() {
    let mut fx = Fx::new();
    let starred = [fx.id("IMG_A1.jpg"), fx.id("IMG_C1.jpg")];
    fx.s.execute("photo.star.set", &json!({ "ids": starred, "on": true })).unwrap();
    assert_eq!(fx.folder_name(json!({ "album": "starred" })), "Starred Photos");
    let written = fx.export(json!({ "album": "starred" }), "Starred");
    let mut stems: Vec<&str> = written.iter().map(|n| n.split_once('-').unwrap().1).collect();
    stems.sort_unstable();
    assert_eq!(stems, ["IMG_A1.jpg", "IMG_C1.jpg"], "{written:?}");
}

#[test]
fn an_unknown_album_or_no_search_fails_gracefully() {
    let mut fx = Fx::new();
    let dir = fx.out();
    for (target, want) in [
        (json!({ "section": "album:a91c0e" }), "not_found"),
        (json!({ "album": "a91c0e" }), "not_found"),
        (json!({ "album": "Lisbon" }), "bad_params"),
        (json!({ "section": "album:" }), "bad_params"),
        (json!({ "section": "album:search" }), "disabled"),
        (json!({ "album": "search" }), "disabled"),
    ] {
        assert_eq!(code(fx.s.execute("export.defaults", &target)), want, "{target}");
        let mut params = json!({ "dir": dir, "folderName": "X" });
        for (k, v) in target.as_object().unwrap() {
            params[k] = v.clone();
        }
        assert_eq!(code(fx.s.execute("export.run", &params)), want, "{params}");
    }
    assert!(!exists(&dir), "nothing was made");
    assert!(fx.s.jobs().iter().all(|j| j.kind != JobKind::Export), "no job started");
}
