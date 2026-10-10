//! Read-only queries over the model.

use std::cmp::Ordering;
use std::path::Path;

use dalica_core::{DateTime, FolderId, PhotoId};

use crate::model::{Folder, Library, Photo};

impl Photo {
    /// The date photos sort by: the capture date, else the file's modification time (UTC).
    pub fn sort_date(&self) -> Option<DateTime> {
        self.date.or_else(|| DateTime::from_unix_secs(self.mtime.as_secs()))
    }
}

/// Display order inside a folder (`organizing.md` §10): capture date, oldest first, then file
/// name, then id.
fn photo_order(a: &Photo, b: &Photo) -> Ordering {
    a.sort_date().cmp(&b.sort_date()).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id))
}

impl Library {
    pub fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.model.folders.get(&id)
    }

    pub fn photo(&self, id: PhotoId) -> Option<&Photo> {
        self.model.photos.get(&id)
    }

    /// Every folder, by id.
    pub fn folders(&self) -> impl Iterator<Item = &Folder> {
        self.model.folders.values()
    }

    /// Every photo, by id.
    pub fn photos(&self) -> impl Iterator<Item = &Photo> {
        self.model.photos.values()
    }

    pub fn folder_by_path(&self, path: &Path) -> Option<FolderId> {
        self.by_path.get(path).copied()
    }

    pub fn photo_by_name(&self, folder: FolderId, name: &str) -> Option<&Photo> {
        self.by_name.get(&(folder, name.to_string())).and_then(|id| self.model.photos.get(id))
    }

    /// The photos of `folder` in display order.
    pub fn photos_in(&self, folder: FolderId) -> Vec<&Photo> {
        let mut out: Vec<&Photo> = self.model.photos.values().filter(|p| p.folder == folder).collect();
        out.sort_by(|a, b| photo_order(a, b));
        out
    }

    /// A folder's date: its oldest photo's sort date (`organizing.md` §10).
    pub fn folder_date(&self, folder: FolderId) -> Option<DateTime> {
        self.model.photos.values().filter(|p| p.folder == folder).filter_map(Photo::sort_date).min()
    }

    /// Folders in the default display order: newest folder date first, folders without photos
    /// last, then by path.
    pub fn folders_in_display_order(&self) -> Vec<&Folder> {
        let mut out: Vec<(Option<DateTime>, &Folder)> = self.model.folders.values().map(|f| (self.folder_date(f.id), f)).collect();
        out.sort_by(|(da, fa), (db, fb)| match (da, db) {
            (Some(a), Some(b)) => b.cmp(a).then_with(|| fa.path.cmp(&fb.path)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => fa.path.cmp(&fb.path),
        });
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// Starred photos, by sort date.
    pub fn starred(&self) -> Vec<&Photo> {
        let mut out: Vec<&Photo> = self.model.photos.values().filter(|p| p.star).collect();
        out.sort_by(|a, b| photo_order(a, b));
        out
    }
}
