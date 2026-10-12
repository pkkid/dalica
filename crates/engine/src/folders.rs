//! The watched folders (`data-model.md` §4, §7): the roots the user added, kept in `folders.json`
//! in the library directory, read when the session opens and written through the I/O worker.
//!
//! Seam: M5.12 grows this into the Folder Manager's model: a mode per folder (Scan Always, Scan
//! Once, Remove) inherited by subfolders, the excluded subtrees handed to each scan
//! ([`crate::scanning`]'s options), `folders.json` v2 (reading v1), and the friendly names
//! `folders.list` shows. The file's reading, writing and the first-run question
//! ([`Session::is_new_library`]) live here so that work never touches `session.rs`.

use std::path::{Path, PathBuf};

use dalica_core::Timestamp;
use dalica_store::Recovery;
use serde_json::Value;

use crate::session::{Msg, Session};

/// The watched-folders file in the library directory (`data-model.md` §4).
pub const FOLDERS_FILE: &str = "folders.json";

/// The watched folders of a session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Folders {
    /// In the order they were added.
    pub(crate) roots: Vec<PathBuf>,
    /// `folders.json` existed when the session opened, or has been written since.
    pub(crate) saved: bool,
}

impl Folders {
    /// The watched folders from `folders.json` (call it off the UI thread); a damaged file is kept
    /// aside and reported.
    pub(crate) fn read(path: &Path, now: Timestamp) -> (Folders, Option<Recovery>) {
        let bytes = match dalica_fs::read(path, 64 << 20) {
            Ok(Some(r)) => r.bytes,
            Ok(None) => return (Folders::default(), None),
            Err(e) => return (Folders { roots: Vec::new(), saved: true }, Some(Recovery::RebuildNeeded { reason: format!("folders.json: {e}") })),
        };
        let parsed: Option<Vec<PathBuf>> = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.get("folders")?.as_array()?.iter().map(|f| f.get("path")?.as_str().map(PathBuf::from)).collect());
        match parsed {
            Some(roots) => (Folders { roots, saved: true }, None),
            None => {
                let stamp = dalica_core::DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
                let kept = dalica_fs::set_aside(path, &format!("corrupt-{stamp}Z")).map(|p| p.display().to_string()).unwrap_or_default();
                let reason = format!("folders.json was damaged and kept as {kept}; add your folders again");
                (Folders { roots: Vec::new(), saved: true }, Some(Recovery::RebuildNeeded { reason }))
            }
        }
    }

    /// `folders.json` as written.
    pub(crate) fn to_json(&self) -> Vec<u8> {
        let json = serde_json::json!({ "version": 1, "folders": self.roots.iter().map(|p| serde_json::json!({ "path": p })).collect::<Vec<_>>() });
        let mut bytes = serde_json::to_vec_pretty(&json).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

impl Session {
    /// The watched folders.
    pub fn roots(&self) -> &[PathBuf] {
        &self.folders.roots
    }

    /// A library that has never had a folder: no `folders.json` when it opened and none added
    /// since. The UI shows the first-run panel for it (`organizing.md` §1).
    pub fn is_new_library(&self) -> bool {
        !self.folders.saved && self.folders.roots.is_empty() && self.library.folders().next().is_none()
    }

    /// Adds a watched folder (saved in `folders.json`) and scans it in the background. Returns
    /// whether it was new.
    pub(crate) fn add_root(&mut self, path: PathBuf) -> bool {
        let new = !self.folders.roots.contains(&path);
        if new {
            self.folders.roots.push(path.clone());
            self.save_folders();
        }
        self.start_scan(vec![path], crate::scanning::ScanKind::Merge);
        new
    }

    /// Writes `folders.json` through the I/O worker.
    pub(crate) fn save_folders(&mut self) {
        self.folders.saved = true;
        let bytes = self.folders.to_json();
        self.send(Msg::Write(self.dir.join(FOLDERS_FILE), bytes));
    }
}
