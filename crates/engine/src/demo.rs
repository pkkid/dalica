//! `--demo` and `--demo-fresh` (`architecture.md` §4, `control-protocol.md` §1): a library of
//! `scenes` photos, so the app and the CLI can be tried and tested without personal photos.
//!
//! - The photos are written through `dalica-fs` (`scenes` itself never writes) under `dir/photos`,
//!   and the library (index, `folders.json`, `settings.json`) lives in `dir/library`, so a demo
//!   never reads or changes the user's settings or library.
//! - `--demo` uses [`default_dir`]: made on first use and reused, so a star survives a restart;
//!   delete the folder to start over. `--demo-fresh` uses a [`fresh_dir`], removed on exit.
//! - Ids are deterministic (`control-protocol.md` §5.4): folders are numbered from 1 in tree order
//!   (newest folder first) and photos from 1 in light-table order (each folder oldest first), so
//!   every demo library lists the same ids for the same paths.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{Clock, DateTime};
use dalica_fs::Expect;
use serde_json::json;

use crate::{CmdError, OpenError, Session};

/// Folders and photos in the demo, and the seed that decides them.
pub const FOLDERS: usize = 3;
pub const PHOTOS: usize = 60;
pub const SEED: u64 = 2026;
/// Written last: the photos are complete.
const DONE: &str = ".dalica-demo-complete";
/// The persistent demo's folder name in the OS temp folder, and the prefix of fresh ones.
const NAME: &str = "dalica-demo";

#[derive(Debug, thiserror::Error)]
pub enum DemoError {
    #[error("cannot make the demo photos: {0}")]
    Scenes(String),
    #[error("cannot write the demo photos: {0}")]
    Fs(#[from] dalica_fs::Error),
    #[error(transparent)]
    Open(#[from] OpenError),
    #[error("cannot add the demo folders: {0}")]
    Command(#[from] CmdError),
}

/// The persistent demo's folder: `<temp>/dalica-demo`.
pub fn default_dir() -> PathBuf {
    std::env::temp_dir().join(NAME)
}

/// A new, empty folder for a fresh demo, `<temp>/dalica-demo-<pid>-<n>`, removed with everything in
/// it when the returned value is closed or dropped. Close it off the UI thread, after the session
/// that uses it is gone.
pub fn fresh_dir() -> Result<dalica_fs::TempDir, DemoError> {
    Ok(dalica_fs::TempDir::new(NAME)?)
}

/// The library folder inside a demo folder.
pub fn library_dir(dir: &Path) -> PathBuf {
    dir.join("library")
}

/// The folder the demo photos are written in.
pub fn photos_dir(dir: &Path) -> PathBuf {
    dir.join("photos")
}

/// Writes the demo photos under `dir/photos` unless a previous run finished doing so. Returns the
/// photo folders in tree order (newest first, as the library shows them). Off the UI thread.
pub fn write_photos(dir: &Path) -> Result<Vec<PathBuf>, DemoError> {
    let root = photos_dir(dir);
    let files = dalica_scenes::demo_library(FOLDERS, PHOTOS, SEED).map_err(|e| DemoError::Scenes(e.to_string()))?;
    // Each folder's date is its oldest photo's, as in `Library::folder_date`.
    let mut dates: BTreeMap<PathBuf, DateTime> = BTreeMap::new();
    for f in &files {
        if let Some(folder) = f.path.parent() {
            let date = dates.entry(root.join(folder)).or_insert(f.date_time_original);
            *date = (*date).min(f.date_time_original);
        }
    }
    if dalica_fs::identity(&root.join(DONE))?.is_none() {
        for folder in dates.keys() {
            dalica_fs::create_dir_all(folder)?;
        }
        for f in &files {
            let path = root.join(&f.path);
            // A run that stopped half way left some files: replace them.
            let expect = dalica_fs::identity(&path)?.map_or(Expect::Absent, Expect::Unchanged);
            dalica_fs::replace(&path, &f.bytes, expect)?;
        }
        dalica_fs::replace(&root.join(DONE), b"", Expect::Absent)?;
    }
    // Tree order, as `Library::folders_in_display_order`: newest date first, then path.
    let mut folders: Vec<(DateTime, PathBuf)> = dates.into_iter().map(|(path, date)| (date, path)).collect();
    folders.sort_by(|(da, pa), (db, pb)| db.cmp(da).then_with(|| pa.cmp(pb)));
    Ok(folders.into_iter().map(|(_, path)| path).collect())
}

/// What opening a demo is doing, for the window's opening screen ([`open_reporting`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Making the demo photos (first use) or finding them complete.
    Photos,
    /// Reading the library's index.
    Index,
    /// Adding the demo's folder `n` of `of`, each scanned before the next (first use).
    Folder { n: usize, of: usize },
    /// The index's ids were out of order: indexing the demo again.
    Reindex,
}

impl Step {
    /// The step as the opening screen says it.
    pub fn describe(self) -> String {
        match self {
            Step::Photos => "Making the demo photos…".to_string(),
            Step::Index => "Reading the library…".to_string(),
            Step::Folder { n, of } => format!("Adding the demo folders ({n} of {of})…"),
            Step::Reindex => "Indexing the demo again…".to_string(),
        }
    }
}

/// Opens the demo library in `dir` (making it on first use), as [`open_reporting`] without a
/// report.
pub fn open(dir: &Path, clock: Arc<dyn Clock>) -> Result<Session, DemoError> {
    open_reporting(dir, clock, &|_| {})
}

/// Opens the demo library in `dir`, making it on first use, and calls `report` as each [`Step`]
/// starts. A new demo has its three small folders scanned before it returns, so its ids are the
/// same everywhere (well under a second); a demo opened again waits for nothing: scans, rescans
/// and thumbnails run in the background, as in any library (`data-model.md` §7). A demo whose ids
/// are not in order (one made before ids were deterministic, or one whose folders
/// were changed) has its index set aside and is indexed again; its stars come back from the
/// sidecars. Off the UI thread.
pub fn open_reporting(dir: &Path, clock: Arc<dyn Clock>, report: &dyn Fn(Step)) -> Result<Session, DemoError> {
    report(Step::Photos);
    let folders = write_photos(dir)?;
    let session = open_library(dir, &folders, Arc::clone(&clock), report)?;
    if ids_in_order(&session) {
        return Ok(session);
    }
    report(Step::Reindex);
    let stamp = clock.now().as_secs();
    drop(session);
    let kept = dalica_fs::set_aside(&library_dir(dir), &format!("unordered-{stamp}"))?;
    log::info!("demo: ids were not in tree order; kept the old index as {} and indexed the demo again", kept.display());
    open_library(dir, &folders, clock, report)
}

/// Opens the library in `dir`; a new one gets `folders` added one at a time, each scanned before
/// the next, so folder and photo ids are handed out in tree and light-table order.
fn open_library(dir: &Path, folders: &[PathBuf], clock: Arc<dyn Clock>, report: &dyn Fn(Step)) -> Result<Session, DemoError> {
    report(Step::Index);
    let mut session = Session::open(&library_dir(dir), clock)?;
    if session.roots().is_empty() {
        for (n, folder) in (1..).zip(folders) {
            report(Step::Folder { n, of: folders.len() });
            session.execute("folders.add", &json!({ "path": folder }))?;
            session.wait_idle();
        }
    }
    Ok(session)
}

/// Whether folders are numbered 1, 2, … in tree order and photos 1, 2, … in light-table order.
fn ids_in_order(session: &Session) -> bool {
    let lib = session.library();
    let mut photo = 0;
    for (n, folder) in (1..).zip(lib.folders_in_display_order()) {
        if folder.id.get() != n {
            return false;
        }
        for p in lib.photos_in(folder.id) {
            photo += 1;
            if p.id.get() != photo {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> Arc<dyn Clock> {
        Arc::new(dalica_core::SystemClock)
    }

    /// `(id, path under the demo's photo folder)` pairs.
    type Ids = Vec<(u64, PathBuf)>;

    /// Every folder in tree order and every photo in light-table order, as `library.query` lists
    /// them.
    fn listing(dir: &Path, s: &mut Session) -> (Ids, Ids) {
        let root = photos_dir(dir);
        let rel = |p: &Path| p.strip_prefix(&root).unwrap().to_path_buf();
        let folders: Ids = s.library().folders_in_display_order().iter().map(|f| (f.id.get(), rel(&f.path))).collect();
        let by_id: BTreeMap<u64, PathBuf> = folders.iter().cloned().collect();
        let q = s.execute("library.query", &json!({})).unwrap();
        let photos = q["photos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["id"].as_u64().unwrap(), by_id[&p["folder"].as_u64().unwrap()].join(p["name"].as_str().unwrap())))
            .collect();
        (folders, photos)
    }

    /// A new demo scans its folders before it returns; one opened again waits for nothing (no
    /// folder step, no wait): its scans and thumbnails go on in the background.
    #[test]
    fn a_reopened_demo_waits_for_nothing_and_says_what_it_does() {
        let dir = fresh_dir().unwrap();
        let steps = std::sync::Mutex::new(Vec::new());
        let note = |s: Step| steps.lock().unwrap().push(s);
        drop(open_reporting(dir.path(), clock(), &note).unwrap());
        let first: Vec<Step> = std::mem::take(&mut *steps.lock().unwrap());
        let folders = (1..=FOLDERS).map(|n| Step::Folder { n, of: FOLDERS });
        assert_eq!(first, [Step::Photos, Step::Index].into_iter().chain(folders).collect::<Vec<_>>());
        let mut s = open_reporting(dir.path(), clock(), &note).unwrap();
        assert_eq!(*steps.lock().unwrap(), [Step::Photos, Step::Index]);
        assert_eq!(s.library().photos().count(), PHOTOS, "the index is there at once");
        s.wait_idle();
        assert_eq!(Step::Folder { n: 2, of: 3 }.describe(), "Adding the demo folders (2 of 3)…");
    }

    #[test]
    fn two_fresh_demos_list_the_same_ids_and_paths() {
        let (a, b) = (fresh_dir().unwrap(), fresh_dir().unwrap());
        assert_ne!(a.path(), b.path());
        assert!(a.path().starts_with(std::env::temp_dir()));
        let first = listing(a.path(), &mut open(a.path(), clock()).unwrap());
        let second = listing(b.path(), &mut open(b.path(), clock()).unwrap());
        assert_eq!(first, second);
        let (folders, photos) = first;
        // Folders from 1 in tree order (newest first), photos from 1 in light-table order.
        let names: Vec<(u64, &str)> = folders.iter().map(|(id, p)| (*id, p.to_str().unwrap())).collect();
        assert_eq!(names, [(1, "2026-06-06 Garden"), (2, "2026-06-05 Hills"), (3, "2026-06-04 Harbour")]);
        assert_eq!(photos.iter().map(|(id, _)| *id).collect::<Vec<_>>(), (1..=PHOTOS as u64).collect::<Vec<_>>());
        assert_eq!(photos[0].1, Path::new("2026-06-06 Garden").join("IMG_0041.jpg"));
        assert_eq!(photos[19].1, Path::new("2026-06-06 Garden").join("IMG_0060.jpg"));
        assert_eq!(photos[20].1, Path::new("2026-06-05 Hills").join("IMG_0021.jpg"));
        assert_eq!(photos[59].1, Path::new("2026-06-04 Harbour").join("IMG_0020.jpg"));
        // A fresh demo's folder goes away with everything in it.
        let path = a.path().to_path_buf();
        a.close().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn the_demo_is_made_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), clock()).unwrap();
        assert_eq!(s.library().folders().count(), FOLDERS);
        assert_eq!(s.library().photos().count(), PHOTOS);
        let before = listing(dir.path(), &mut s);
        let id = s.library().photos().next().unwrap().id;
        s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
        s.flush().unwrap();
        drop(s);
        // A second run reuses the photos and the library: the star and the ids are still there.
        let mut s = open(dir.path(), clock()).unwrap();
        assert_eq!(s.roots().len(), FOLDERS);
        assert_eq!(s.library().photos().count(), PHOTOS);
        assert_eq!(s.library().photos().filter(|p| p.star).map(|p| p.id).collect::<Vec<_>>(), [id]);
        assert_eq!(listing(dir.path(), &mut s), before);
    }

    #[test]
    fn a_demo_numbered_out_of_order_is_indexed_again_keeping_its_stars() {
        let dir = tempfile::tempdir().unwrap();
        let mut folders = write_photos(dir.path()).unwrap();
        // As an older version made it: folders added oldest first, all scanned together.
        folders.reverse();
        let mut s = Session::open(&library_dir(dir.path()), clock()).unwrap();
        for f in &folders {
            s.execute("folders.add", &json!({ "path": f })).unwrap();
        }
        s.wait_idle();
        assert!(!ids_in_order(&s));
        let oldest = s.library().photos_in(s.library().folder_by_path(&folders[0]).unwrap())[0].clone();
        s.execute("photo.star.set", &json!({ "ids": [oldest.id], "on": true })).unwrap();
        s.flush().unwrap();
        drop(s);
        let mut s = open(dir.path(), clock()).unwrap();
        assert!(ids_in_order(&s));
        assert_eq!(listing(dir.path(), &mut s).1.len(), PHOTOS);
        let starred: Vec<&str> = s.library().starred().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(starred, [oldest.name.as_str()]);
        // The old index was kept aside, not deleted.
        let names = || -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        let after_first = names();
        assert_eq!(after_first.len(), 3, "{after_first:?}");
        assert!(after_first[1].starts_with("library.unordered-"), "{after_first:?}");
        drop(s);
        // The next run finds the ids in order and changes nothing.
        let s = open(dir.path(), clock()).unwrap();
        assert!(ids_in_order(&s));
        assert_eq!(names(), after_first);
    }

    #[test]
    fn the_demo_keeps_its_settings_to_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), clock()).unwrap();
        assert_eq!(s.dir(), library_dir(dir.path()));
        s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "Ctrl+Shift+8" })).unwrap();
        s.flush().unwrap();
        assert!(library_dir(dir.path()).join(crate::settings::FILE).is_file());
    }

    #[test]
    fn an_interrupted_first_run_is_completed() {
        let dir = tempfile::tempdir().unwrap();
        let folders = write_photos(dir.path()).unwrap();
        // Simulate a run that stopped before the marker: one photo missing, no marker.
        std::fs::remove_file(photos_dir(dir.path()).join(DONE)).unwrap();
        let first = std::fs::read_dir(&folders[0]).unwrap().next().unwrap().unwrap().path();
        std::fs::remove_file(&first).unwrap();
        let s = open(dir.path(), clock()).unwrap();
        assert_eq!(s.library().photos().count(), PHOTOS);
        assert!(ids_in_order(&s));
    }
}
