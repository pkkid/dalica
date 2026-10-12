//! The metadata hook (`data-model.md` §5, `xmp-interop.md` §2): after every op the session applies,
//! it is told which photos' owned metadata fields (caption, tags, star, place, date) the op
//! changed, and by what: a command (undo and redo included, so every surface reaches it) or the
//! files themselves (scans and rescans, whose changes are imports and never written back). It is
//! also called from [`Session::poll`] each frame.
//!
//! M6.5 lays the ground: the hook records what it is told and [`Session::take_metadata_changes`]
//! hands it out; [`Session::poll_metadata`] does nothing yet. M6.33 turns the changes into write
//! jobs through the store's metadata write queue, and maps the fields M6.2 adds to the photo
//! (caption, tags, place, the user's date) in [`owned`].

use std::collections::VecDeque;

use dalica_core::PhotoId;
use dalica_library::{Library, Op, Photo};

use crate::session::Session;

/// The most photo entries kept across the recorded changes; past it the oldest changes are dropped
/// (and counted), so a session nobody drains stays bounded.
pub const MAX_PENDING: usize = 1_000_000;

/// One owned metadata field (`xmp-interop.md` §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Caption,
    Tags,
    Star,
    Place,
    Date,
}

impl Field {
    pub const ALL: [Field; 5] = [Field::Caption, Field::Tags, Field::Star, Field::Place, Field::Date];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of [`Field`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fields(u8);

impl Fields {
    pub const NONE: Fields = Fields(0);
    pub const ALL: Fields = Fields(0b1_1111);

    pub fn of(fields: &[Field]) -> Fields {
        Fields(fields.iter().fold(0, |b, f| b | f.bit()))
    }

    pub fn contains(self, f: Field) -> bool {
        self.0 & f.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, f: Field) {
        self.0 |= f.bit();
    }

    /// The fields in the set, in [`Field::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Field> {
        Field::ALL.into_iter().filter(move |f| self.contains(*f))
    }
}

/// What made a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// A command: the user, an agent, or undo and redo of either.
    Command,
    /// What a scan or rescan found in the files: an import, never written back.
    Files,
}

/// The owned fields one applied op changed, per photo (by id, each photo once).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataChange {
    pub origin: Origin,
    pub photos: Vec<(PhotoId, Fields)>,
}

/// The hook's state: the changes recorded and not yet taken.
#[derive(Debug, Default)]
pub(crate) struct Metadata {
    changes: VecDeque<MetadataChange>,
    /// Photo entries in `changes`.
    entries: usize,
    /// Changes dropped to keep within [`MAX_PENDING`].
    dropped: u64,
}

impl Metadata {
    /// Records `change`, then drops the oldest changes (counting them) while more than `cap` photo
    /// entries are kept; the newest change is always kept.
    fn record(&mut self, change: MetadataChange, cap: usize) {
        self.entries = self.entries.saturating_add(change.photos.len());
        self.changes.push_back(change);
        while self.entries > cap && self.changes.len() > 1 {
            if let Some(old) = self.changes.pop_front() {
                self.entries = self.entries.saturating_sub(old.photos.len());
                self.dropped = self.dropped.saturating_add(1);
            }
        }
    }
}

/// The owned values of a photo the hook compares before and after an op. M6.33 adds the fields
/// M6.2 gives the photo (caption, tags, place and the user's date).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Owned {
    star: bool,
}

/// `p`'s owned values.
pub(crate) fn owned(p: &Photo) -> Owned {
    Owned { star: p.star }
}

/// The fields in which `a` and `b` differ.
fn differ(a: &Owned, b: &Owned) -> Fields {
    let mut f = Fields::NONE;
    if a.star != b.star {
        f.insert(Field::Star);
    }
    f
}

/// The photos `op` may change an owned field of, by id, each once. Photos added or removed are not changed: they enter or leave the library with the
/// values their files and sidecars hold. Walks nested batches without recursion.
pub(crate) fn candidates(op: &Op) -> Vec<PhotoId> {
    let mut out = std::collections::BTreeSet::new();
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::SetStars { stars } => out.extend(stars.iter().map(|(id, _)| *id)),
            Op::UpdatePhotos { photos } => out.extend(photos.iter().map(|p| p.id)),
            Op::Batch { ops } => stack.extend(ops),
            // Folders and photos entering or leaving the library, derived data, and the ops M6.2
            // adds until M6.33 maps them.
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// The owned values of `ids` in `library` now (photos not in it are left out).
pub(crate) fn snapshot(library: &Library, ids: &[PhotoId]) -> Vec<(PhotoId, Owned)> {
    ids.iter().filter_map(|id| library.photo(*id).map(|p| (*id, owned(p)))).collect()
}

impl Session {
    /// The hook, called by [`Session::apply`] after each applied op with the owned values of its
    /// candidate photos from before it: records the fields that changed, if any.
    pub(crate) fn metadata_applied(&mut self, before: Vec<(PhotoId, Owned)>, origin: Origin) {
        let photos: Vec<(PhotoId, Fields)> = before
            .into_iter()
            .filter_map(|(id, old)| {
                let now = owned(self.library.photo(id)?);
                Some((id, differ(&old, &now))).filter(|(_, f)| !f.is_empty())
            })
            .collect();
        if !photos.is_empty() {
            self.metadata.record(MetadataChange { origin, photos }, MAX_PENDING);
        }
    }

    /// Called from [`Session::poll`] each frame. Does nothing yet (M6.33 starts the writes here);
    /// returns how many things it handled.
    pub(crate) fn poll_metadata(&mut self) -> usize {
        0
    }

    /// The changes the metadata hook recorded since the last call, oldest first, and how many
    /// older ones were dropped to stay within [`MAX_PENDING`] photo entries.
    pub fn take_metadata_changes(&mut self) -> (Vec<MetadataChange>, u64) {
        let m = &mut self.metadata;
        m.entries = 0;
        (m.changes.drain(..).collect(), std::mem::take(&mut m.dropped))
    }
}

#[cfg(test)]
mod tests {
    use dalica_core::FolderId;

    use super::*;
    use crate::cmd::testing::library;

    /// Photos of a real library, so the test never spells out the photo's fields.
    fn photos() -> (crate::cmd::testing::Dirs, Session, Vec<Photo>) {
        let (dirs, s, ids) = library(3);
        let photos = ids.iter().map(|id| s.library().photo(*id).unwrap().clone()).collect();
        (dirs, s, photos)
    }

    #[test]
    fn candidates_walk_nested_batches_and_list_each_photo_once_by_id() {
        let (_dirs, _s, p) = photos();
        let id = |i: usize| p[i].id;
        let op = Op::Batch {
            ops: vec![
                Op::SetStars { stars: vec![(id(2), true), (id(0), true)] },
                Op::Batch { ops: vec![Op::UpdatePhotos { photos: vec![p[1].clone(), p[2].clone()] }, Op::Batch { ops: Vec::new() }] },
                Op::AddPhotos { photos: vec![p[0].clone()] },
                Op::RemovePhotos { ids: vec![PhotoId::new(8)] },
                Op::SetSketches { sketches: vec![(PhotoId::new(7), None)] },
                Op::RemoveFolder { id: FolderId::new(1) },
            ],
        };
        assert_eq!(candidates(&op), [id(0), id(1), id(2)]);
        // A deep nesting is walked without recursion.
        let mut deep = Op::SetStars { stars: vec![(PhotoId::new(5), true)] };
        for _ in 0..100_000 {
            deep = Op::Batch { ops: vec![deep] };
        }
        assert_eq!(candidates(&deep), [PhotoId::new(5)]);
        // Dropping a deeply nested op recurses; leak it so the test thread's stack is spared.
        std::mem::forget(deep);
    }

    #[test]
    fn fields_are_a_set() {
        let mut f = Fields::of(&[Field::Tags, Field::Star, Field::Tags]);
        assert!(f.contains(Field::Star) && f.contains(Field::Tags) && !f.contains(Field::Date));
        f.insert(Field::Caption);
        assert_eq!(f.iter().collect::<Vec<_>>(), [Field::Caption, Field::Tags, Field::Star]);
        assert!(Fields::NONE.is_empty() && Fields::ALL.iter().eq(Field::ALL));
        let (_dirs, _s, p) = photos();
        let mut a = owned(&p[0]);
        let b = a.clone();
        assert!(differ(&a, &b).is_empty());
        a.star = true;
        assert_eq!(differ(&a, &b), Fields::of(&[Field::Star]));
    }

    #[test]
    fn recorded_changes_stay_within_the_cap_and_the_newest_is_kept() {
        let change =
            |n: u64| MetadataChange { origin: Origin::Command, photos: (0..n).map(|i| (PhotoId::new(i), Fields::of(&[Field::Star]))).collect() };
        let mut m = Metadata::default();
        for _ in 0..4 {
            m.record(change(3), 10);
        }
        // 12 entries over a cap of 10: the oldest change went.
        assert_eq!((m.changes.len(), m.entries, m.dropped), (3, 9, 1));
        m.record(change(50), 10);
        assert_eq!((m.changes.len(), m.entries, m.dropped), (1, 50, 4), "the newest is kept even past the cap");
    }
}
