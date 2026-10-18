//! Grouping unnamed faces into clusters of similar faces (`docs/faces.md` §5).
//!
//! Incremental: a face joins the cluster whose centroid is most similar when that similarity is at
//! or above the Grouping threshold, otherwise it starts a cluster; a merge pass then joins clusters
//! whose centroids meet the threshold, checking only the clusters that changed since the last
//! pass. Clusters are listed largest first, and within a cluster the faces most similar to its
//! centroid first. [`Groups::build`] adds faces in photo order, so the same faces give the same
//! clusters however they arrive.
//!
//! Faces are held at 8 bits ([`Packed`]); a centroid is the float32 sum of its faces' vectors,
//! normalised. Our own code; LightCraft has no grouping.

use std::collections::{BTreeMap, BTreeSet};

use crate::matching::{Packed, cosine, similarity_exact};

/// One unnamed face to group.
#[derive(Clone, Debug, PartialEq)]
pub struct Member<F> {
    pub face: F,
    /// The face's place in photo order (the library's order key); ties by face.
    pub order: u64,
    pub vec: Packed,
}

/// Why a face was not added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupError {
    /// The face is already grouped.
    Duplicate,
    /// Its embedding's length differs from the other faces'.
    Dimension { expected: usize, found: usize },
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate => write!(f, "the face is already grouped"),
            Self::Dimension { expected, found } => write!(f, "the face has {found} numbers, the others {expected}"),
        }
    }
}

impl std::error::Error for GroupError {}

/// One cluster as shown: its faces, most similar to the centroid first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group<F> {
    pub faces: Vec<F>,
}

#[derive(Clone, Debug)]
struct Cluster<F> {
    members: Vec<Member<F>>,
    sum: Vec<f32>,
    /// `sum` normalised (all zeros when the sum has no direction).
    centroid: Vec<f32>,
    centroid_l1: f32,
}

impl<F> Cluster<F> {
    fn new(m: Member<F>) -> Self {
        let mut c = Self { sum: vec![0.0; m.vec.dim()], members: Vec::new(), centroid: Vec::new(), centroid_l1: 0.0 };
        c.push(m);
        c
    }

    fn push(&mut self, m: Member<F>) {
        m.vec.add_to(&mut self.sum, 1.0);
        self.members.push(m);
        self.renormalise();
    }

    fn renormalise(&mut self) {
        let norm = self.sum.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>().sqrt();
        self.centroid = if norm.is_finite() && norm > 1e-12 {
            self.sum.iter().map(|&x| (f64::from(x) / norm) as f32).collect()
        } else {
            vec![0.0; self.sum.len()]
        };
        self.centroid_l1 = self.centroid.iter().map(|x| x.abs()).sum();
    }

    fn similarity(&self, v: &Packed) -> f32 {
        similarity_exact(v, &self.centroid, self.centroid_l1).map_or(f32::NEG_INFINITY, |s| s.value)
    }
}

/// The clusters of the unnamed faces at one Grouping threshold. A new threshold means a new
/// [`Groups::build`].
#[derive(Clone, Debug)]
pub struct Groups<F> {
    threshold: f32,
    dim: Option<usize>,
    /// By creation number: older clusters win ties and absorb younger ones.
    clusters: BTreeMap<u64, Cluster<F>>,
    of: BTreeMap<F, u64>,
    next: u64,
    /// Clusters changed since the last merge pass.
    dirty: BTreeSet<u64>,
}

impl<F: Clone + Ord> Groups<F> {
    /// No faces yet. A threshold that is not finite groups nothing together.
    pub fn new(threshold: f32) -> Self {
        let threshold = if threshold.is_finite() { threshold } else { f32::INFINITY };
        Self { threshold, dim: None, clusters: BTreeMap::new(), of: BTreeMap::new(), next: 0, dirty: BTreeSet::new() }
    }

    /// Group `faces` from scratch: added in photo order (then by face), then one merge pass.
    /// Faces [`Groups::add`] refuses (a duplicate, another length) are left out.
    pub fn build(threshold: f32, faces: impl IntoIterator<Item = Member<F>>) -> Self {
        let mut faces: Vec<Member<F>> = faces.into_iter().collect();
        faces.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.face.cmp(&b.face)));
        let mut g = Self::new(threshold);
        for m in faces {
            let _refused = g.add(m);
        }
        g.merge_pass();
        g
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// How many faces are grouped.
    pub fn faces(&self) -> usize {
        self.of.len()
    }

    /// How many clusters there are.
    pub fn len(&self) -> usize {
        self.clusters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }

    pub fn contains(&self, face: &F) -> bool {
        self.of.contains_key(face)
    }

    /// Add one newly embedded face: into the most similar cluster (ties to the oldest) if it meets
    /// the threshold, else into a cluster of its own.
    pub fn add(&mut self, m: Member<F>) -> Result<(), GroupError> {
        if self.of.contains_key(&m.face) {
            return Err(GroupError::Duplicate);
        }
        let dim = *self.dim.get_or_insert(m.vec.dim());
        if m.vec.dim() != dim {
            return Err(GroupError::Dimension { expected: dim, found: m.vec.dim() });
        }
        let mut best: Option<(u64, f32)> = None;
        for (&id, c) in &self.clusters {
            let s = c.similarity(&m.vec);
            if best.is_none_or(|(_, b)| s > b) {
                best = Some((id, s));
            }
        }
        let face = m.face.clone();
        let id = match best.filter(|(_, s)| *s >= self.threshold).and_then(|(id, _)| self.clusters.get_mut(&id).map(|c| (id, c))) {
            Some((id, c)) => {
                c.push(m);
                id
            }
            None => {
                let id = self.next;
                self.next = self.next.saturating_add(1);
                self.clusters.insert(id, Cluster::new(m));
                id
            }
        };
        self.of.insert(face, id);
        self.dirty.insert(id);
        Ok(())
    }

    /// Take a face out (named, ignored or deleted). `false` when it was not grouped.
    pub fn remove(&mut self, face: &F) -> bool {
        let Some(id) = self.of.remove(face) else { return false };
        let Some(c) = self.clusters.get_mut(&id) else { return false };
        if let Some(i) = c.members.iter().position(|m| m.face == *face) {
            let m = c.members.swap_remove(i);
            m.vec.add_to(&mut c.sum, -1.0);
        }
        if c.members.is_empty() {
            self.clusters.remove(&id);
            self.dirty.remove(&id);
        } else {
            c.renormalise();
            self.dirty.insert(id);
        }
        if self.of.is_empty() {
            self.dim = None;
        }
        true
    }

    /// Merge clusters whose centroids meet the threshold: each changed cluster with its most similar
    /// other cluster (ties to the oldest), the younger into the older, until nothing changes.
    /// Returns how many merges were made.
    pub fn merge_pass(&mut self) -> usize {
        let mut merges = 0;
        while let Some(id) = self.dirty.pop_first() {
            let Some(c) = self.clusters.get(&id) else { continue };
            let mut best: Option<(u64, f32)> = None;
            for (&other, o) in &self.clusters {
                if other == id {
                    continue;
                }
                let s = cosine(&c.centroid, &o.centroid).unwrap_or(f32::NEG_INFINITY);
                if best.is_none_or(|(_, b)| s > b) {
                    best = Some((other, s));
                }
            }
            let Some((other, _)) = best.filter(|(_, s)| *s >= self.threshold) else { continue };
            let (keep, gone) = (id.min(other), id.max(other));
            let Some(g) = self.clusters.remove(&gone) else { continue };
            self.dirty.remove(&gone);
            if let Some(k) = self.clusters.get_mut(&keep) {
                for m in g.members {
                    self.of.insert(m.face.clone(), keep);
                    m.vec.add_to(&mut k.sum, 1.0);
                    k.members.push(m);
                }
                k.renormalise();
            }
            self.dirty.insert(keep);
            merges += 1;
        }
        merges
    }

    /// The clusters, largest first (ties: the one with the earliest photo first); within each, the
    /// faces most similar to the centroid first (ties in photo order).
    pub fn groups(&self) -> Vec<Group<F>> {
        let mut out: Vec<(usize, (u64, F), Group<F>)> = self
            .clusters
            .values()
            .filter_map(|c| {
                let mut ranked: Vec<(f32, &Member<F>)> = c.members.iter().map(|m| (c.similarity(&m.vec), m)).collect();
                ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.order.cmp(&b.1.order)).then_with(|| a.1.face.cmp(&b.1.face)));
                let first = c.members.iter().map(|m| (m.order, m.face.clone())).min()?;
                Some((c.members.len(), first, Group { faces: ranked.into_iter().map(|(_, m)| m.face.clone()).collect() }))
            })
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        out.into_iter().map(|(_, _, g)| g).collect()
    }
}
