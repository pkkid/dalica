//! Read-only queries over the model.

use std::cmp::Ordering;
use std::path::Path;

use dalica_core::{DateTime, FolderId, PhotoId};

use crate::model::{Folder, Library, Photo};

impl Photo {
    /// The date the file gives: its capture date, else its modification time (UTC). The library
    /// sorts by [`Library::sort_date`], which puts a date set by the user first.
    pub fn sort_date(&self) -> Option<DateTime> {
        self.date.or_else(|| DateTime::from_unix_secs(self.mtime.as_secs()))
    }
}

/// `photos` in display order (`organizing.md` §10): effective date (a date set by the user, else
/// the capture date, else the file time), oldest first, then file name, then id. Each photo's date
/// is looked up once.
fn in_photo_order<'a>(lib: &Library, photos: impl Iterator<Item = &'a Photo>) -> Vec<&'a Photo> {
    let mut keyed: Vec<(Option<DateTime>, &Photo)> = photos.map(|p| (lib.sort_date(p), p)).collect();
    keyed.sort_by(|(da, a), (db, b)| da.cmp(db).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
    keyed.into_iter().map(|(_, p)| p).collect()
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
        in_photo_order(self, self.model.photos.values().filter(|p| p.folder == folder))
    }

    /// A folder's date: its oldest photo's effective sort date ([`Library::sort_date`];
    /// `organizing.md` §10).
    pub fn folder_date(&self, folder: FolderId) -> Option<DateTime> {
        self.model.photos.values().filter(|p| p.folder == folder).filter_map(|p| self.sort_date(p)).min()
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

    /// Starred photos, by effective sort date.
    pub fn starred(&self) -> Vec<&Photo> {
        in_photo_order(self, self.model.photos.values().filter(|p| p.star))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};

    use crate::model::{Folder, Library, Photo};
    use crate::op::Op;

    fn date(s: &str) -> DateTime {
        s.parse().unwrap()
    }

    /// Two folders: `old` holds `a` (2020-01-02) and `b` (2020-01-03); `new` holds `z`
    /// (2024-05-05) and `m`, which has no date and a file time of 2020-01-01.
    fn library() -> (Library, [FolderId; 2], [PhotoId; 4]) {
        let mut lib = Library::new();
        let (old, new) = (lib.next_folder_id(), lib.next_folder_id());
        for (id, path) in [(old, "/p/old"), (new, "/p/new")] {
            lib.apply(Op::AddFolder { folder: Folder { id, path: PathBuf::from(path) } }).unwrap();
        }
        let mut add = |folder, name: &str, d: Option<&str>, secs: i64| {
            let id = lib.next_photo_id();
            let mtime = Timestamp::from_millis(secs * 1000);
            let p = Photo { id, folder, name: name.into(), size: 1, mtime, width: 1, height: 1, date: d.map(date), star: true };
            lib.apply(Op::AddPhotos { photos: vec![p] }).unwrap();
            id
        };
        let a = add(old, "a.jpg", Some("2020-01-02T00:00:00"), 0);
        let b = add(old, "b.jpg", Some("2020-01-03T00:00:00"), 0);
        let z = add(new, "z.jpg", Some("2024-05-05T00:00:00"), 0);
        let m = add(new, "m.jpg", None, 1_577_836_800);
        (lib, [old, new], [a, b, z, m])
    }

    fn ids(photos: Vec<&Photo>) -> Vec<PhotoId> {
        photos.into_iter().map(|p| p.id).collect()
    }

    /// A date set by the user (Adjust Date and Time) re-sorts the folder's photos, moves the
    /// folder's date and the folder order, and orders the starred photos; undoing it puts every
    /// order back.
    #[test]
    fn orders_follow_the_users_date() {
        let (mut lib, [old, new], [a, b, z, m]) = library();
        assert_eq!(ids(lib.photos_in(old)), [a, b]);
        assert_eq!(ids(lib.photos_in(new)), [m, z], "no date: the file time");
        assert_eq!(lib.folder_date(new), Some(date("2020-01-01T00:00:00")));
        assert_eq!(lib.folders_in_display_order().iter().map(|f| f.id).collect::<Vec<_>>(), [old, new]);
        assert_eq!(ids(lib.starred()), [m, a, b, z]);

        // `a` moves after `b`; `m` gets a date of its own after `z`.
        let undo = lib.apply(Op::SetDates { dates: vec![(a, Some(date("2020-01-04T00:00:00"))), (m, Some(date("2025-01-01T00:00:00")))] }).unwrap();
        assert_eq!(ids(lib.photos_in(old)), [b, a]);
        assert_eq!(ids(lib.photos_in(new)), [z, m]);
        assert_eq!(lib.folder_date(old), Some(date("2020-01-03T00:00:00")));
        assert_eq!(lib.folder_date(new), Some(date("2024-05-05T00:00:00")));
        assert_eq!(lib.folders_in_display_order().iter().map(|f| f.id).collect::<Vec<_>>(), [new, old], "newest folder date first");
        assert_eq!(ids(lib.starred()), [b, a, z, m]);
        // The file's own date is untouched.
        assert_eq!(lib.photo(a).unwrap().sort_date(), Some(date("2020-01-02T00:00:00")));

        lib.apply(undo).unwrap();
        assert_eq!((ids(lib.photos_in(old)), ids(lib.photos_in(new))), (vec![a, b], vec![m, z]));
        assert_eq!(ids(lib.starred()), [m, a, b, z]);
    }

    /// Equal dates fall back to the file name, then the id.
    #[test]
    fn equal_dates_order_by_name() {
        let (mut lib, [old, _], [a, b, ..]) = library();
        lib.apply(Op::SetDates { dates: vec![(a, Some(date("2020-01-04T00:00:00")))] }).unwrap();
        assert_eq!(ids(lib.photos_in(old)), [b, a]);
        lib.apply(Op::SetDates { dates: vec![(b, Some(date("2020-01-04T00:00:00")))] }).unwrap();
        assert_eq!(ids(lib.photos_in(old)), [a, b]);
    }
}
