//! Read-only queries over the model.
//!
//! M8's queries (`data-model.md` §6) say what Save, Undo Save and Revert may do: whether a photo
//! has unsaved edits, each folder's count of them (worked out once per revision), whether a photo
//! has a finished save to undo or revert, and which operation of a recipe cannot be rendered.
//!
//! M9's queries (`organizing.md` §4, `data-model.md` §3.2) are worked out on first use after each
//! change they read and kept until the next ([`Cache`]): an album's members in order (by position
//! key; equal keys by folder path, then file name), Starred Photos (by date, like a folder),
//! Recently Updated (the photos updated within some days of a given now, newest first), and each
//! album's date, count and cover. Photo orders ([`PhotoSort`]) sort a folder's photos and Sort
//! Album By.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, PosKey, Timestamp};

use crate::model::{Folder, Library, Photo, PhotoSort, Saved, SortBy};

/// The album queries' answers, each worked out on first use. Every op that bumps the revision
/// drops them, and so does a derived op that sets first-indexed or last-changed times.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cache {
    /// One slot per album, each sorted on first use (a drop into one album sorts only that one).
    members: OnceLock<HashMap<AlbumId, OnceLock<Vec<PhotoId>>>>,
    starred: OnceLock<Vec<PhotoId>>,
    /// Every photo with its updated time, newest first.
    updated: OnceLock<Vec<(Timestamp, PhotoId)>>,
    albums: OnceLock<HashMap<AlbumId, AlbumStats>>,
}

/// What the tree shows of an album beside its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlbumStats {
    /// Its members, hidden ones included.
    pub count: usize,
    /// Its own date (at midnight) unless it uses its newest photo's; that one otherwise.
    pub date: Option<DateTime>,
    /// The cover the user set, else the first member; `None` for an empty album.
    pub cover: Option<PhotoId>,
}

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

    /// Whether photo `id` has unsaved edits (the *edited* badge): its recipe is not empty and is
    /// not the one the copy saved beside it holds. The redo list does not count.
    pub fn has_unsaved_edits(&self, id: PhotoId) -> bool {
        let edits = &self.info(id).edits;
        !edits.is_empty() && self.saved_as(id).is_none_or(|s| s.recipe != *edits)
    }

    /// How many photos of `folder` have unsaved edits (hidden ones included: Save in the folder
    /// header saves them all). Cached per revision.
    pub fn unsaved_count(&self, folder: FolderId) -> usize {
        self.unsaved_counts().get(&folder).copied().unwrap_or(0)
    }

    /// Every folder's count of photos with unsaved edits (folders without any are left out),
    /// worked out on the first call after each revision.
    pub fn unsaved_counts(&self) -> &HashMap<FolderId, usize> {
        self.unsaved.get_or_init(|| {
            let mut counts: HashMap<FolderId, usize> = HashMap::new();
            for (&id, info) in &self.model.photo_info {
                if info.edits.is_empty() || !self.has_unsaved_edits(id) {
                    continue;
                }
                if let Some(p) = self.model.photos.get(&id) {
                    let n = counts.entry(p.folder).or_insert(0);
                    *n = n.saturating_add(1);
                }
            }
            counts
        })
    }

    /// The photos of `folder` with unsaved edits, in display order.
    pub fn unsaved_in(&self, folder: FolderId) -> Vec<PhotoId> {
        if self.unsaved_count(folder) == 0 {
            return Vec::new();
        }
        self.photos_in(folder).into_iter().map(|p| p.id).filter(|&id| self.has_unsaved_edits(id)).collect()
    }

    /// Whether Undo Save applies to photo `id`: it was saved in place and the save finished.
    pub fn can_undo_save(&self, id: PhotoId) -> bool {
        self.saved(id).is_some_and(Saved::is_done)
    }

    /// Whether Revert applies to photo `id`: the same as Undo Save (a finished in-place save).
    pub fn can_revert(&self, id: PhotoId) -> bool {
        self.can_undo_save(id)
    }

    /// The first operation a Save of photo `id` would render that cannot be rendered, if any:
    /// the recorded recipe of an earlier save, then the recipe. `renders` is `edit`'s parse (an
    /// unknown or `legacy:` operation cannot be rendered); the library leaves operations opaque.
    /// A Save refuses a photo with one, so baking pixels never drops an operation.
    pub fn unrenderable(&self, id: PhotoId, renders: impl Fn(&str) -> bool) -> Option<&str> {
        let saved = self.saved(id).map(|s| s.recipe.as_slice()).unwrap_or_default();
        saved.iter().chain(&self.info(id).edits).map(String::as_str).find(|op| !renders(op))
    }
}

/// M9's queries: photo orders, album members, Starred Photos, Recently Updated and album stats.
impl Library {
    /// `photos` in the order `sort` gives: by effective date ([`Library::sort_date`]), by file
    /// name ignoring case, or by bytes; ties by file name, then id; `reverse` turns all of it
    /// around. Each key is worked out once.
    pub fn sorted<'a>(&self, photos: impl Iterator<Item = &'a Photo>, sort: PhotoSort) -> Vec<PhotoId> {
        let mut out: Vec<PhotoId> = match sort.by {
            SortBy::Date => in_photo_order(self, photos).into_iter().map(|p| p.id).collect(),
            SortBy::Name => {
                let mut keyed: Vec<(String, &Photo)> = photos.map(|p| (p.name.to_lowercase(), p)).collect();
                keyed.sort_by(|(fa, a), (fb, b)| fa.cmp(fb).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
                keyed.into_iter().map(|(_, p)| p.id).collect()
            }
            SortBy::Size => {
                let mut list: Vec<&Photo> = photos.collect();
                list.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
                list.into_iter().map(|p| p.id).collect()
            }
        };
        if sort.reverse {
            out.reverse();
        }
        out
    }

    /// The photos of `folder` in its own photo sort ([`Library::folder_sort`]).
    pub fn photos_sorted(&self, folder: FolderId) -> Vec<PhotoId> {
        self.sorted(self.model.photos.values().filter(|p| p.folder == folder), self.folder_sort(folder))
    }

    /// Album `id`'s members in album order: by position key, equal keys by folder path, then file
    /// name, then id (`data-model.md` §3.2). Empty for an unknown or empty album.
    pub fn album_members(&self, id: &AlbumId) -> &[PhotoId] {
        let slots = self.cache.members.get_or_init(|| self.model.members.keys().map(|a| (a.clone(), OnceLock::new())).collect());
        let Some((slot, members)) = slots.get(id).zip(self.model.members.get(id)) else { return &[] };
        slot.get_or_init(|| {
            let mut keyed: Vec<(&PosKey, Option<&Path>, &str, PhotoId)> = members
                .iter()
                .map(|(id, key)| {
                    let p = self.model.photos.get(id);
                    let path = p.and_then(|p| self.model.folders.get(&p.folder)).map(|f| f.path.as_path());
                    (key, path, p.map_or("", |p| p.name.as_str()), *id)
                })
                .collect();
            keyed.sort();
            keyed.into_iter().map(|(_, _, _, id)| id).collect()
        })
    }

    /// Starred Photos: every starred photo, by date like a folder (hidden ones included).
    pub fn starred_photos(&self) -> &[PhotoId] {
        self.cache.starred.get_or_init(|| in_photo_order(self, self.model.photos.values().filter(|p| p.star)).into_iter().map(|p| p.id).collect())
    }

    /// Recently Updated: the photos whose updated time ([`Library::updated_time`]) is within
    /// `days` days before `now` (or after it), newest first, then by file name and id. The sort is
    /// worked out once per change; each call only cuts it.
    pub fn recently_updated(&self, now: Timestamp, days: u32) -> Vec<PhotoId> {
        let all = self.cache.updated.get_or_init(|| {
            let mut list: Vec<(Timestamp, &Photo)> = self.model.photos.values().map(|p| (self.updated_time(p), p)).collect();
            list.sort_by(|(ta, a), (tb, b)| tb.cmp(ta).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
            list.into_iter().map(|(t, p)| (t, p.id)).collect()
        });
        let since = now.saturating_sub(Duration::from_secs(u64::from(days).saturating_mul(86_400)));
        all.iter().take_while(|(t, _)| *t >= since).map(|(_, id)| *id).collect()
    }

    /// Album `id`'s count, date and cover (all zero for an unknown album).
    pub fn album_stats(&self, id: &AlbumId) -> AlbumStats {
        let all = self.cache.albums.get_or_init(|| {
            let mut out = HashMap::with_capacity(self.model.albums.len());
            for (album, info) in &self.model.albums {
                let members = self.album_members(album);
                let own = info.date.filter(|_| !info.use_newest).and_then(|d| DateTime::new(d, 0, 0, 0).ok());
                let date = own.or_else(|| members.iter().filter_map(|id| self.model.photos.get(id)).filter_map(|p| self.sort_date(p)).max());
                let cover = self.model.covers.get(album).copied().or_else(|| members.first().copied());
                out.insert(album.clone(), AlbumStats { count: members.len(), date, cover });
            }
            out
        });
        all.get(id).copied().unwrap_or_default()
    }

    /// Album `id`'s date: its own unless it uses its newest photo's or has none.
    pub fn album_date(&self, id: &AlbumId) -> Option<DateTime> {
        self.album_stats(id).date
    }

    /// Album `id`'s cover: the one set, else its first member.
    pub fn album_cover(&self, id: &AlbumId) -> Option<PhotoId> {
        self.album_stats(id).cover
    }

    /// How many photos album `id` holds.
    pub fn album_count(&self, id: &AlbumId) -> usize {
        self.album_stats(id).count
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
