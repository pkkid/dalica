//! The in-memory face index (`faces.md` §5–§6, `roadmap.md` M13 → *Grouping*): the embeddings of
//! the recogniser in use, loaded from its cache file beside the index (`dalica-store`'s
//! `faces_cache`) and added to as the face scan (`faces_scan.rs`) embeds faces, which photos the
//! detector has searched, and the groups of unnamed faces recomputed from them (never user data).
//!
//! - **One model at a time.** The index holds the embeddings of one recogniser
//!   ([`FacesIndex::model`]); switching models resets it and loads the other model's cache, so
//!   results of two models are never mixed (`faces.md` §3.3).
//! - **Keyed by the file.** A photo's entry carries the stamp it was made for (its index id, file
//!   size and modification time); a photo whose file changed reads as never embedded nor searched.
//!   A face is found by its photo and its rectangle (stored frame, [`FaceAt`]), so it keeps its
//!   embedding when other faces of the photo come or go.
//! - **8 bits in memory.** Embeddings are kept as [`Packed`] (a quarter of float32), so 100 k
//!   faces stay within the memory budget (`faces.md` §6).
//! - **Groups** are made on a worker from a snapshot of the unnamed faces in photo order
//!   (`dalica-faces`'s `cluster::Groups::build`, so the same faces give the same groups after a
//!   restart), largest first; [`FacesIndex::group_of`] answers which group a face is in.
//!
//! Nothing here touches the disk; nothing here is logged (`faces.md` §7).

use std::collections::HashMap;

use dalica_core::{PhotoId, Rect01};
use dalica_faces::calibration::{Calibration, Confidence};
use dalica_faces::matching::Packed;
use dalica_library::{FaceRef, Library, Photo};
use dalica_store::faces_cache::PhotoStamp;

use crate::session::Session;

/// A face by its photo and its rectangle (stored frame), as the index and the groups know it: it
/// stays the same face when other faces of the photo come or go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceAt {
    pub photo: PhotoId,
    /// The rectangle's corners by their bits (a [`Rect01`] is always finite, six decimals).
    rect: [u64; 4],
}

impl FaceAt {
    pub fn new(photo: PhotoId, rect: &Rect01) -> FaceAt {
        FaceAt { photo, rect: [rect.x0(), rect.y0(), rect.x1(), rect.y1()].map(f64::to_bits) }
    }

    /// Whether this is the face at `rect`.
    pub fn is_at(&self, rect: &Rect01) -> bool {
        FaceAt::new(self.photo, rect) == *self
    }

    /// The face of the library this names, if its photo still has a face at the rectangle.
    pub fn resolve(&self, lib: &Library) -> Option<FaceRef> {
        let index = lib.faces(self.photo).iter().position(|f| self.is_at(&f.rect))?;
        Some(FaceRef { photo: self.photo, index })
    }
}

/// The recogniser whose embeddings the index holds.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    /// The model id (`people.model`; names its cache file).
    pub id: String,
    /// The model file's SHA-256.
    pub sha256: String,
    /// The embedding dimension.
    pub dim: usize,
    /// Its confidence → cosine map (`faces.md` §5).
    pub calibration: Calibration,
}

impl ModelInfo {
    /// The cosine threshold of a confidence slider value (50 … 95; another value is the default).
    pub fn threshold(&self, confidence: u32) -> f32 {
        let c = u8::try_from(confidence).ok().and_then(Confidence::new).unwrap_or_default();
        self.calibration.threshold(c)
    }
}

/// What the index holds for one photo.
#[derive(Clone, Debug)]
pub(crate) struct Indexed {
    pub(crate) stamp: PhotoStamp,
    /// The detector has searched the photo (as stamped).
    pub(crate) searched: bool,
    /// The embedded faces, by rectangle.
    pub(crate) faces: Vec<(Rect01, Packed)>,
}

/// The face index's state.
#[derive(Debug, Default)]
pub struct FacesIndex {
    model: Option<ModelInfo>,
    generation: u64,
    photos: HashMap<PhotoId, Indexed>,
    faces: usize,
    /// The groups of unnamed faces, largest first; within each, the faces nearest its centre first.
    groups: Vec<Vec<FaceAt>>,
    group_of: HashMap<FaceAt, usize>,
    /// What the groups were made from ([`FacesIndex::grouping_key`]); `None`: never made.
    grouped: Option<u64>,
}

impl FacesIndex {
    /// The id of the model whose embeddings are loaded, if any.
    pub fn model(&self) -> Option<&str> {
        self.model.as_ref().map(|m| m.id.as_str())
    }

    /// The model whose embeddings are loaded, if any.
    pub fn model_info(&self) -> Option<&ModelInfo> {
        self.model.as_ref()
    }

    /// Rises each time the faces or their groups change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many faces have an embedding (of any photo the index holds, as stamped).
    pub fn faces(&self) -> usize {
        self.faces
    }

    /// The embedding of `photo`'s face at `rect`, when it was made for the photo's file as it is.
    pub fn embedding(&self, photo: &Photo, rect: &Rect01) -> Option<&Packed> {
        self.current(photo)?.faces.iter().find(|(r, _)| r == rect).map(|(_, v)| v)
    }

    /// Whether the detector has searched `photo` (its file as it is).
    pub fn searched(&self, photo: &Photo) -> bool {
        self.current(photo).is_some_and(|e| e.searched)
    }

    /// The groups of unnamed faces, largest first; within each, the faces nearest its centre first.
    pub fn groups(&self) -> &[Vec<FaceAt>] {
        &self.groups
    }

    /// The group (its place in [`FacesIndex::groups`]) of `photo`'s face at `rect`, if grouped.
    pub fn group_of(&self, photo: PhotoId, rect: &Rect01) -> Option<usize> {
        self.group_of.get(&FaceAt::new(photo, rect)).copied()
    }

    fn current(&self, photo: &Photo) -> Option<&Indexed> {
        self.photos.get(&photo.id).filter(|e| e.stamp == PhotoStamp::of(photo))
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Starts over for `model` (or none): every embedding, search and group goes.
    pub(crate) fn reset(&mut self, model: Option<ModelInfo>) {
        self.model = model;
        self.photos.clear();
        self.faces = 0;
        self.groups.clear();
        self.group_of.clear();
        self.grouped = None;
        self.bump();
    }

    /// Takes in what the scan found or the cache held for one photo: `searched` when the detector
    /// searched it, `faces` embedded (replacing the same rectangles). An entry made for another
    /// stamp of the photo starts afresh.
    pub(crate) fn add(&mut self, stamp: PhotoStamp, searched: bool, faces: Vec<(Rect01, Packed)>) {
        let dim = self.model.as_ref().map(|m| m.dim);
        let entry = self.photos.entry(stamp.id).or_insert_with(|| Indexed { stamp, searched: false, faces: Vec::new() });
        if entry.stamp != stamp {
            self.faces = self.faces.saturating_sub(entry.faces.len());
            *entry = Indexed { stamp, searched: false, faces: Vec::new() };
        }
        entry.searched |= searched;
        for (rect, vec) in faces {
            if dim.is_some_and(|d| d != vec.dim()) {
                continue;
            }
            match entry.faces.iter_mut().find(|(r, _)| *r == rect) {
                Some(slot) => slot.1 = vec,
                None => {
                    entry.faces.push((rect, vec));
                    self.faces = self.faces.saturating_add(1);
                }
            }
        }
        self.bump();
    }

    /// Forgets everything about `photo` (Find Faces Again).
    pub(crate) fn forget(&mut self, photo: PhotoId) {
        if let Some(gone) = self.photos.remove(&photo) {
            self.faces = self.faces.saturating_sub(gone.faces.len());
            self.bump();
        }
    }

    /// What the groups depend on: the model, the threshold, and the unnamed faces with an
    /// embedding, in photo order. Cheap next to grouping: one pass over the unnamed faces.
    pub(crate) fn grouping_key(&self, lib: &Library, threshold: f32) -> u64 {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut h = std::hash::BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().build_hasher();
        self.model().hash(&mut h);
        threshold.to_bits().hash(&mut h);
        for r in lib.unnamed_faces() {
            let (Some(face), Some(photo)) = (lib.face(*r), lib.photo(r.photo)) else { continue };
            FaceAt::new(r.photo, &face.rect).hash(&mut h);
            PhotoStamp::of(photo).hash(&mut h);
            self.embedding(photo, &face.rect).is_some().hash(&mut h);
        }
        h.finish()
    }

    /// Whether the groups were made from what `key` describes.
    pub(crate) fn grouped_for(&self, key: u64) -> bool {
        self.grouped == Some(key)
    }

    /// The unnamed faces with an embedding, each with its place in photo order: what the groups
    /// are made from.
    pub(crate) fn members(&self, lib: &Library) -> Vec<dalica_faces::cluster::Member<FaceAt>> {
        lib.unnamed_faces()
            .iter()
            .enumerate()
            .filter_map(|(order, r)| {
                let face = lib.face(*r)?;
                let photo = lib.photo(r.photo)?;
                let vec = self.embedding(photo, &face.rect)?.clone();
                Some(dalica_faces::cluster::Member { face: FaceAt::new(r.photo, &face.rect), order: order as u64, vec })
            })
            .collect()
    }

    /// Takes the groups made for `key`.
    pub(crate) fn set_groups(&mut self, key: u64, groups: Vec<Vec<FaceAt>>) {
        self.group_of = groups.iter().enumerate().flat_map(|(i, g)| g.iter().map(move |f| (*f, i))).collect();
        self.groups = groups;
        self.grouped = Some(key);
        self.bump();
    }
}

impl Session {
    /// The in-memory face index (`faces.md` §5).
    pub fn faces_index(&self) -> &FacesIndex {
        &self.faces_index
    }

    /// The group of unnamed face `r` (its place in [`FacesIndex::groups`]), once the index has one
    /// (`faces.list`'s `group`).
    pub fn face_group(&self, r: FaceRef) -> Option<usize> {
        let face = self.library.face(r)?;
        self.faces_index.group_of(r.photo, &face.rect)
    }
}

#[cfg(test)]
mod tests {
    use dalica_core::Timestamp;
    use dalica_faces::matching::Embedding;

    use super::*;

    fn rect(x: f64) -> Rect01 {
        Rect01::new(x, 0.1, x + 0.2, 0.4).unwrap()
    }

    fn packed(v: &[f32]) -> Packed {
        Embedding::new(v).unwrap().pack()
    }

    fn stamp(id: u64, size: u64) -> PhotoStamp {
        PhotoStamp { id: PhotoId::new(id), size, mtime: Timestamp::from_millis(5) }
    }

    /// A face keeps its key whatever other faces come or go; a new stamp starts the photo afresh;
    /// embeddings of another dimension than the model's are refused; forgetting drops the photo.
    #[test]
    fn faces_are_kept_per_stamp_and_rectangle() {
        let mut index = FacesIndex::default();
        let model = ModelInfo { id: "m".into(), sha256: "0".repeat(64), dim: 2, calibration: Calibration::linear(0.5).unwrap() };
        index.reset(Some(model));
        index.add(stamp(1, 10), true, vec![(rect(0.1), packed(&[1.0, 0.0])), (rect(0.5), packed(&[0.0, 1.0]))]);
        index.add(stamp(1, 10), false, vec![(rect(0.1), packed(&[0.0, 1.0])), (rect(0.3), packed(&[1.0, 0.0, 0.0]))]);
        assert_eq!(index.faces(), 2, "replaced in place; the wrong length refused");
        let e = index.photos.get(&PhotoId::new(1)).unwrap();
        assert!(e.searched);
        assert_eq!(e.faces[0].1, packed(&[0.0, 1.0]));
        index.add(stamp(1, 11), false, vec![(rect(0.1), packed(&[1.0, 0.0]))]);
        let e = index.photos.get(&PhotoId::new(1)).unwrap();
        assert!(!e.searched && e.faces.len() == 1 && index.faces() == 1, "the file changed: started afresh");
        index.forget(PhotoId::new(1));
        assert_eq!(index.faces(), 0);
        let a = FaceAt::new(PhotoId::new(3), &rect(0.1));
        assert!(a.is_at(&rect(0.1)) && !a.is_at(&rect(0.2)));
    }

    #[test]
    fn a_threshold_comes_from_the_models_calibration() {
        let model = ModelInfo { id: "m".into(), sha256: "0".repeat(64), dim: 2, calibration: Calibration::linear(0.5).unwrap() };
        assert!((model.threshold(85) - 0.5).abs() < 1e-6);
        assert!((model.threshold(95) - 0.6).abs() < 1e-6);
        assert!((model.threshold(7) - 0.5).abs() < 1e-6, "not a slider value: the default");
    }
}
