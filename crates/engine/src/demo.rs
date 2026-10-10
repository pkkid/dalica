//! `--demo` (`architecture.md` §4): a library of `scenes` photos in a fixed folder under the OS temp
//! folder, so the app and the CLI can be tried and tested without personal photos. The photos are
//! written through `dalica-fs` (`scenes` itself never writes) the first time; later runs reuse the
//! same folder, so a star survives a restart. Delete the folder to start over.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::Clock;
use dalica_fs::Expect;
use serde_json::json;

use crate::{CmdError, OpenError, Session};

/// Folders and photos in the demo, and the seed that decides them.
pub const FOLDERS: usize = 3;
pub const PHOTOS: usize = 60;
pub const SEED: u64 = 2026;
/// Written last: the photos are complete.
const DONE: &str = ".dalica-demo-complete";

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

/// The demo's folder: `<temp>/dalica-demo`.
pub fn default_dir() -> PathBuf {
    std::env::temp_dir().join("dalica-demo")
}

/// The library folder inside a demo folder.
pub fn library_dir(dir: &Path) -> PathBuf {
    dir.join("library")
}

/// Writes the demo photos under `dir/photos` unless a previous run finished doing so. Returns the
/// photo folders. Off the UI thread.
pub fn write_photos(dir: &Path) -> Result<Vec<PathBuf>, DemoError> {
    let root = dir.join("photos");
    let files = dalica_scenes::demo_library(FOLDERS, PHOTOS, SEED).map_err(|e| DemoError::Scenes(e.to_string()))?;
    let folders: BTreeSet<PathBuf> = files.iter().filter_map(|f| f.path.parent().map(|p| root.join(p))).collect();
    if dalica_fs::identity(&root.join(DONE))?.is_none() {
        for folder in &folders {
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
    Ok(folders.into_iter().collect())
}

/// Opens the demo library in `dir` (making it on first use) and waits until its folders are
/// scanned. Off the UI thread.
pub fn open(dir: &Path, clock: Arc<dyn Clock>) -> Result<Session, DemoError> {
    let folders = write_photos(dir)?;
    let mut session = Session::open(&library_dir(dir), clock)?;
    if session.roots().is_empty() {
        for folder in folders {
            session.execute("folders.add", &json!({ "path": folder }))?;
        }
    }
    session.wait_idle();
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_is_made_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        assert_eq!(s.library().folders().count(), FOLDERS);
        assert_eq!(s.library().photos().count(), PHOTOS);
        let id = s.library().photos().next().unwrap().id;
        s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
        s.flush().unwrap();
        drop(s);
        // A second run reuses the photos and the library: the star is still there.
        let s = open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        assert_eq!(s.roots().len(), FOLDERS);
        assert_eq!(s.library().photos().count(), PHOTOS);
        assert_eq!(s.library().photos().filter(|p| p.star).count(), 1);
    }

    #[test]
    fn an_interrupted_first_run_is_completed() {
        let dir = tempfile::tempdir().unwrap();
        let folders = write_photos(dir.path()).unwrap();
        // Simulate a run that stopped before the marker: one photo missing, no marker.
        std::fs::remove_file(dir.path().join("photos").join(DONE)).unwrap();
        let first = std::fs::read_dir(&folders[0]).unwrap().next().unwrap().unwrap().path();
        std::fs::remove_file(&first).unwrap();
        let s = open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        assert_eq!(s.library().photos().count(), PHOTOS);
    }
}
