//! The model's data: folders and photos, and the [`Library`] that owns them with its revisions.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use serde::{Deserialize, Serialize};

/// A folder of photos, by its absolute path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub path: PathBuf,
}

/// One photo file and what Dalica knows about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub id: PhotoId,
    pub folder: FolderId,
    /// The file name inside its folder.
    pub name: String,
    /// Bytes on disk.
    pub size: u64,
    /// File modification time.
    pub mtime: Timestamp,
    /// Size as displayed (after EXIF orientation); 0 × 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// Capture date (EXIF DateTimeOriginal), if the file has one.
    pub date: Option<DateTime>,
    pub star: bool,
}

/// The data that ops change: everything except counters. Two libraries with equal models show the
/// same photos with the same stars.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
    pub(crate) folders: BTreeMap<FolderId, Folder>,
    pub(crate) photos: BTreeMap<PhotoId, Photo>,
}

/// The library: the model, its revision counters and the next free ids.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub(crate) model: Model,
    /// (folder, file name) → photo.
    pub(crate) by_name: HashMap<(FolderId, String), PhotoId>,
    /// Absolute path → folder.
    pub(crate) by_path: HashMap<PathBuf, FolderId>,
    pub(crate) revision: u64,
    pub(crate) folder_revisions: HashMap<FolderId, u64>,
    pub(crate) next_folder: u64,
    pub(crate) next_photo: u64,
}

impl Library {
    pub fn new() -> Library {
        Library::default()
    }

    /// The model, for comparisons and snapshots.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Bumped once by every applied op.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The library revision at which something in `folder` (the folder or its photos) last changed;
    /// 0 if never.
    pub fn folder_revision(&self, folder: FolderId) -> u64 {
        self.folder_revisions.get(&folder).copied().unwrap_or(0)
    }

    /// A folder id no folder has had in this library.
    pub fn next_folder_id(&mut self) -> FolderId {
        let max = self.model.folders.keys().next_back().map_or(0, |id| id.get());
        self.next_folder = self.next_folder.max(max).saturating_add(1);
        FolderId::new(self.next_folder)
    }

    /// A photo id no photo has had in this library.
    pub fn next_photo_id(&mut self) -> PhotoId {
        let max = self.model.photos.keys().next_back().map_or(0, |id| id.get());
        self.next_photo = self.next_photo.max(max).saturating_add(1);
        PhotoId::new(self.next_photo)
    }

    pub(crate) fn touch(&mut self, folder: FolderId) {
        self.folder_revisions.insert(folder, self.revision);
    }
}
