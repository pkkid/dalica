//! Who does this face look like? Embeddings, their 8-bit in-memory form, and name suggestions
//! from the faces people have confirmed (`docs/faces.md` §5).
//!
//! Ported from LightCraft `crates/faces/src/matching.rs` (`cosine`, `rank`, `suggest`: the best
//! match and the margin over the runner-up) @ f34c7a8a, MIT OR Apache-2.0 (`NOTICE`). Changed from
//! the original: a person's score is their single best match (the original averages the best two);
//! people are ids, not names; each person is scored against at most [`REFERENCES`] of their
//! confirmed faces, the most recent and most varied; embeddings are held at 8 bits per component
//! ([`Packed`]) and a decision that falls within the quantisation error of a bar is re-scored at
//! float32 ([`ExactSource`]); confirmed faces are never suggested and rejected pairs are skipped;
//! Find More ([`Gallery::find_more`]) is new.
//!
//! Embeddings are unit vectors, so closeness is the dot product (cosine similarity). The result is
//! only ever a suggestion for the user to confirm: it is better to leave a face unnamed than to
//! name it wrongly. Every input is hostile: a vector that is empty, too long, zero or not finite is
//! refused when it becomes an [`Embedding`]; vectors of different lengths never compare (they are
//! skipped, never a panic).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The longest embedding accepted (face models give 128 to 512 numbers).
pub const MAX_DIM: usize = 4096;

/// How many of a person's confirmed faces they are scored against.
pub const REFERENCES: usize = 64;

/// How many of a person's most recent faces the [`REFERENCES`] are chosen from.
pub const REFERENCE_POOL: usize = 4 * REFERENCES;

/// How far (cosine) the best person must be ahead of the second-best for a suggestion.
pub const DEFAULT_MARGIN: f32 = 0.05;

/// Find More lists faces scoring above this fraction of the suggestion threshold.
pub const FIND_MORE_FRACTION: f32 = 0.8;

/// The largest value of one 8-bit component.
const Q_MAX: f32 = 127.0;

/// Why a vector cannot be an embedding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingError {
    /// No numbers at all.
    Empty,
    /// More than [`MAX_DIM`] numbers.
    TooLong(usize),
    /// A `NaN` or an infinity.
    NotFinite,
    /// All zeros (or too close to zero to have a direction).
    Zero,
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "the embedding is empty"),
            Self::TooLong(n) => write!(f, "the embedding has {n} numbers, more than {MAX_DIM}"),
            Self::NotFinite => write!(f, "the embedding has a number that is not finite"),
            Self::Zero => write!(f, "the embedding has no direction (all zeros)"),
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// A unit-length float32 embedding: the exact form, as stored on disk and used to settle ties.
#[derive(Clone, Debug, PartialEq)]
pub struct Embedding(Box<[f32]>);

impl Embedding {
    /// Check `v` and scale it to unit length (the norm is taken in float64, so large numbers do not
    /// overflow).
    pub fn new(v: &[f32]) -> Result<Self, EmbeddingError> {
        if v.is_empty() {
            return Err(EmbeddingError::Empty);
        }
        if v.len() > MAX_DIM {
            return Err(EmbeddingError::TooLong(v.len()));
        }
        if v.iter().any(|x| !x.is_finite()) {
            return Err(EmbeddingError::NotFinite);
        }
        let norm = v.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>().sqrt();
        if !(norm.is_finite() && norm > 1e-30) {
            return Err(EmbeddingError::Zero);
        }
        let unit: Box<[f32]> = v.iter().map(|&x| (f64::from(x) / norm) as f32).collect();
        // A vector whose components are all subnormal can still lose its direction in float32.
        if unit.iter().all(|x| *x == 0.0) {
            return Err(EmbeddingError::Zero);
        }
        Ok(Self(unit))
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// The 8-bit in-memory form.
    pub fn pack(&self) -> Packed {
        let max = self.0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        // `max > 0`: `new` refused vectors with no direction.
        let scale = if max > 0.0 { max / Q_MAX } else { 1.0 };
        let q: Box<[i8]> = self.0.iter().map(|&x| quantise(x / scale)).collect();
        let l1 = q.iter().map(|&c| f32::from(c).abs()).sum::<f32>() * scale;
        Packed { q, scale, l1 }
    }

    /// The cosine similarity with another embedding (`None` when their lengths differ).
    pub fn cosine(&self, other: &Self) -> Option<f32> {
        cosine(&self.0, &other.0)
    }
}

/// One component rounded to 8 bits (`x` is already in `−127 ..= 127`).
fn quantise(x: f32) -> i8 {
    let r = x.round().clamp(-Q_MAX, Q_MAX);
    // In range after the clamp; `as` saturates in any case.
    r as i8
}

/// An embedding at 8 bits per component, with one scale per vector: a quarter of the memory of
/// float32, so 100 k faces stay within the memory budget. Made only from an [`Embedding`].
#[derive(Clone, Debug, PartialEq)]
pub struct Packed {
    q: Box<[i8]>,
    /// The value of one step of `q`.
    scale: f32,
    /// The L1 norm of the dequantised vector (for the error bound).
    l1: f32,
}

impl Packed {
    pub fn dim(&self) -> usize {
        self.q.len()
    }

    /// The vector back in float32 (within half a step of the original per component).
    pub fn unpack(&self) -> Vec<f32> {
        self.q.iter().map(|&c| f32::from(c) * self.scale).collect()
    }

    /// The largest error of one component.
    fn half_step(&self) -> f32 {
        self.scale * 0.5
    }

    /// Add the dequantised vector to `sum` (same length; checked by the caller).
    pub(crate) fn add_to(&self, sum: &mut [f32], sign: f32) {
        for (s, &c) in sum.iter_mut().zip(self.q.iter()) {
            *s += sign * f32::from(c) * self.scale;
        }
    }
}

/// A similarity computed from 8-bit vectors, and how far the exact float32 value can be from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub value: f32,
    /// `|exact − value| ≤ bound`.
    pub bound: f32,
}

/// Cosine similarity of two unit vectors. `None` when their lengths differ, they are empty, or the
/// result is not finite.
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let d: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    d.is_finite().then_some(d)
}

/// The similarity of two packed vectors (`None` when their lengths differ). The bound covers the
/// rounding of both: `|x·y − x̂·ŷ| ≤ |x̂|₁ e_y + |ŷ|₁ e_x + n e_x e_y`, `e` half a step.
pub fn similarity(a: &Packed, b: &Packed) -> Option<Score> {
    if a.q.len() != b.q.len() || a.q.is_empty() {
        return None;
    }
    // At most 127² × 4096 < 2³¹ in magnitude: no overflow.
    let dot: i32 = a.q.iter().zip(b.q.iter()).map(|(&x, &y)| i32::from(x) * i32::from(y)).sum();
    let value = dot as f32 * a.scale * b.scale;
    let (ea, eb) = (a.half_step(), b.half_step());
    let bound = a.l1 * eb + b.l1 * ea + a.q.len() as f32 * ea * eb;
    (value.is_finite() && bound.is_finite()).then_some(Score { value, bound: bound * 1.001 + 1e-6 })
}

/// The similarity of a packed vector and an exact one whose L1 norm is `c_l1`.
pub(crate) fn similarity_exact(a: &Packed, c: &[f32], c_l1: f32) -> Option<Score> {
    if a.q.len() != c.len() || c.is_empty() {
        return None;
    }
    let dot: f32 = a.q.iter().zip(c).map(|(&x, &y)| f32::from(x) * y).sum();
    let value = dot * a.scale;
    let bound = c_l1 * a.half_step();
    (value.is_finite() && bound.is_finite()).then_some(Score { value, bound: bound * 1.001 + 1e-6 })
}

/// Where the float32 embeddings live (the cache on disk), read only to settle a decision that falls
/// within the 8-bit error of a bar. `None` keeps the 8-bit score.
pub trait ExactSource<F> {
    fn exact(&self, face: &F) -> Option<Embedding>;
}

/// No float32 at hand: decisions use the 8-bit scores.
pub struct NoExact;

impl<F> ExactSource<F> for NoExact {
    fn exact(&self, _: &F) -> Option<Embedding> {
        None
    }
}

impl<F, T: Fn(&F) -> Option<Embedding>> ExactSource<F> for T {
    fn exact(&self, face: &F) -> Option<Embedding> {
        self(face)
    }
}

/// The two bars a suggestion must pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bars {
    /// The Suggestion threshold (cosine), from [`crate::calibration::Calibration::threshold`].
    pub threshold: f32,
    /// How far ahead of the second-best person (cosine).
    pub margin: f32,
}

impl Bars {
    pub fn new(threshold: f32) -> Self {
        Self { threshold, margin: DEFAULT_MARGIN }
    }
}

/// One confirmed face of a person.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference<F> {
    pub face: F,
    /// When the photo was taken (seconds; `None` when unknown, counted as the oldest).
    pub taken: Option<i64>,
    pub vec: Packed,
}

/// A person's score for one face: their best match.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked<P, F> {
    pub person: P,
    pub score: f32,
    /// How far the exact score can be from `score`.
    pub bound: f32,
    /// The reference that matched best.
    pub face: F,
}

/// A "Name?" for one face.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion<P> {
    pub person: P,
    pub score: f32,
    /// The next best person and their score.
    pub runner_up: Option<(P, f32)>,
}

/// The people's confirmed faces (at most [`REFERENCES`] each), every confirmed face, and the
/// rejected (face, person) pairs: what suggestions are made from. `F` identifies a face, `P` a
/// person; both are compared by `Ord`, so every result is in the same order on every run.
#[derive(Clone, Debug)]
pub struct Gallery<F, P> {
    people: BTreeMap<P, Vec<Reference<F>>>,
    confirmed: BTreeMap<F, P>,
    rejected: BTreeSet<(F, P)>,
}

impl<F, P> Default for Gallery<F, P> {
    fn default() -> Self {
        Self { people: BTreeMap::new(), confirmed: BTreeMap::new(), rejected: BTreeSet::new() }
    }
}

impl<F: Clone + Ord, P: Clone + Ord> Gallery<F, P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `person`'s confirmed faces (replacing what they had): each is never suggested again, and
    /// the person is scored against the [`REFERENCES`] chosen by [`select_references`].
    pub fn set_person(&mut self, person: P, faces: Vec<Reference<F>>) {
        self.confirmed.retain(|_, p| *p != person);
        for r in &faces {
            self.confirmed.insert(r.face.clone(), person.clone());
        }
        let refs = select_references(faces);
        if refs.is_empty() {
            self.people.remove(&person);
        } else {
            self.people.insert(person, refs);
        }
    }

    /// Forget `person` (deleted, or merged into another): their faces are no longer confirmed.
    pub fn remove_person(&mut self, person: &P) {
        self.people.remove(person);
        self.confirmed.retain(|_, p| p != person);
        self.rejected.retain(|(_, p)| p != person);
    }

    /// The user said `face` is not `person`: never suggest that pair again.
    pub fn reject(&mut self, face: F, person: P) {
        self.rejected.insert((face, person));
    }

    pub fn is_rejected(&self, face: &F, person: &P) -> bool {
        self.rejected.contains(&(face.clone(), person.clone()))
    }

    pub fn is_confirmed(&self, face: &F) -> bool {
        self.confirmed.contains_key(face)
    }

    /// The faces `person` is scored against (empty for an unknown person).
    pub fn references(&self, person: &P) -> &[Reference<F>] {
        self.people.get(person).map_or(&[], Vec::as_slice)
    }

    /// Every person with references, by id.
    pub fn people(&self) -> impl Iterator<Item = &P> {
        self.people.keys()
    }

    /// The people ranked for one face (`query`, the 8-bit form of `face`), best first, ties by
    /// person; people whose pair with `face` was rejected are left out.
    pub fn rank(&self, face: &F, query: &Packed) -> Vec<Ranked<P, F>> {
        self.rank_where(query, |p| !self.is_rejected(face, p), |_| true)
    }

    /// [`Gallery::rank`] over the people `keep_person` keeps, each scored against the references
    /// `keep_ref` keeps.
    pub(crate) fn rank_where(&self, query: &Packed, keep_person: impl Fn(&P) -> bool, keep_ref: impl Fn(&Reference<F>) -> bool) -> Vec<Ranked<P, F>> {
        let mut out: Vec<Ranked<P, F>> = Vec::new();
        for (person, refs) in &self.people {
            if !keep_person(person) {
                continue;
            }
            let mut best: Option<(Score, &F)> = None;
            for r in refs.iter().filter(|r| keep_ref(r)) {
                let Some(s) = similarity(query, &r.vec) else { continue };
                if best.is_none_or(|(b, _)| s.value > b.value) {
                    best = Some((s, &r.face));
                }
            }
            if let Some((s, f)) = best {
                out.push(Ranked { person: person.clone(), score: s.value, bound: s.bound, face: f.clone() });
            }
        }
        sort_ranked(&mut out);
        out
    }

    /// The "Name?" for one unnamed face, if its best person passes both bars: a score at or above
    /// the threshold, ahead of the second-best person by the margin. A confirmed face gets none.
    /// When a bar falls within the 8-bit error, the people within reach are re-scored at float32
    /// from `exact`.
    pub fn suggest(&self, face: &F, query: &Packed, bars: Bars, exact: &impl ExactSource<F>) -> Option<Suggestion<P>> {
        if self.is_confirmed(face) {
            return None;
        }
        let ranked = self.rank(face, query);
        self.decide(face, ranked, bars, exact, |_| true)
    }

    /// Apply the bars to `ranked` (from [`Gallery::rank_where`] with the same `keep_ref`).
    pub(crate) fn decide(
        &self,
        face: &F,
        mut ranked: Vec<Ranked<P, F>>,
        bars: Bars,
        exact: &impl ExactSource<F>,
        keep_ref: impl Fn(&Reference<F>) -> bool,
    ) -> Option<Suggestion<P>> {
        if !(bars.threshold.is_finite() && bars.margin.is_finite()) {
            return None;
        }
        if ambiguous(&ranked, bars) {
            self.rescore(face, &mut ranked, exact, keep_ref);
        }
        let best = ranked.first()?;
        let next = ranked.get(1);
        if best.score < bars.threshold || next.is_some_and(|n| best.score - n.score < bars.margin) {
            return None;
        }
        Some(Suggestion { person: best.person.clone(), score: best.score, runner_up: next.map(|n| (n.person.clone(), n.score)) })
    }

    /// Re-score at float32 the people whose 8-bit score could reach the runner-up's.
    fn rescore(&self, face: &F, ranked: &mut [Ranked<P, F>], exact: &impl ExactSource<F>, keep_ref: impl Fn(&Reference<F>) -> bool) {
        let Some(q) = exact.exact(face) else { return };
        let qp = q.pack();
        let floor = match (ranked.first(), ranked.get(1)) {
            (_, Some(n)) => n.score - 2.0 * n.bound,
            (Some(b), None) => b.score - 2.0 * b.bound,
            (None, None) => return,
        };
        for r in ranked.iter_mut().filter(|r| r.score + r.bound >= floor) {
            let Some(refs) = self.people.get(&r.person) else { continue };
            let low = r.score - 2.0 * r.bound;
            let mut best: Option<(f32, f32, F)> = None; // (score, bound, face)
            for rf in refs.iter().filter(|rf| keep_ref(rf)) {
                let Some(s) = similarity(&qp, &rf.vec).filter(|s| s.value + s.bound >= low) else { continue };
                let (v, b) = match exact.exact(&rf.face).and_then(|e| q.cosine(&e)) {
                    Some(v) => (v, 0.0),
                    None => (s.value, s.bound),
                };
                if best.as_ref().is_none_or(|(bv, _, _)| v > *bv) {
                    best = Some((v, b, rf.face.clone()));
                }
            }
            if let Some((v, b, f)) = best {
                (r.score, r.bound, r.face) = (v, b, f);
            }
        }
        sort_ranked(ranked);
    }

    /// Find More for `person`: the faces among `candidates` scoring above [`FIND_MORE_FRACTION`] of
    /// the suggestion `threshold`, most similar first (ties by face). Confirmed faces and rejected
    /// pairs are left out.
    pub fn find_more<'a>(&self, person: &P, candidates: impl IntoIterator<Item = (F, &'a Packed)>, threshold: f32) -> Vec<(F, f32)> {
        let refs = self.references(person);
        if refs.is_empty() || !threshold.is_finite() {
            return Vec::new();
        }
        let bar = threshold - (1.0 - FIND_MORE_FRACTION) * threshold.abs();
        let mut out: Vec<(F, f32)> = Vec::new();
        for (face, vec) in candidates {
            if self.is_confirmed(&face) || self.is_rejected(&face, person) {
                continue;
            }
            let best = refs.iter().filter_map(|r| similarity(vec, &r.vec)).map(|s| s.value).fold(f32::NEG_INFINITY, f32::max);
            if best > bar {
                out.push((face, best));
            }
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// Best first; ties by person.
fn sort_ranked<P: Ord, F>(r: &mut [Ranked<P, F>]) {
    r.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.person.cmp(&b.person)));
}

/// Whether the 8-bit error could change the decision: the best score within its error of the
/// threshold, or the lead within the two errors of the margin.
fn ambiguous<P, F>(ranked: &[Ranked<P, F>], bars: Bars) -> bool {
    let Some(best) = ranked.first() else { return false };
    if (best.score - bars.threshold).abs() <= best.bound {
        return true;
    }
    ranked.get(1).is_some_and(|n| ((best.score - n.score) - bars.margin).abs() <= best.bound + n.bound)
}

/// Choose the faces a person is scored against: from their [`REFERENCE_POOL`] most recent faces
/// (unknown dates last, ties by face), the most recent one, then repeatedly the one least like
/// those already chosen (the most varied: other angles, ages, lighting), until [`REFERENCES`].
/// Faces whose length differs from the most recent one's are left out. The cost is bounded by the
/// pool, however many faces the person has.
pub fn select_references<F: Ord>(mut faces: Vec<Reference<F>>) -> Vec<Reference<F>> {
    faces.sort_by(|a, b| {
        match (a.taken, b.taken) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.face.cmp(&b.face))
    });
    let Some(dim) = faces.first().map(|r| r.vec.dim()) else { return faces };
    faces.retain(|r| r.vec.dim() == dim);
    faces.truncate(REFERENCE_POOL);
    if faces.len() <= REFERENCES {
        return faces;
    }
    // Farthest-point selection: `closest[i]` is candidate i's highest similarity to a chosen face.
    let mut pool: Vec<Option<Reference<F>>> = faces.into_iter().map(Some).collect();
    let mut closest = vec![f32::NEG_INFINITY; pool.len()];
    let mut chosen: Vec<Reference<F>> = Vec::with_capacity(REFERENCES);
    let mut next = Some(0usize);
    while let Some(i) = next {
        let Some(r) = pool.get_mut(i).and_then(Option::take) else { break };
        for (slot, c) in pool.iter().zip(closest.iter_mut()) {
            if let Some(s) = slot.as_ref().and_then(|o| similarity(&r.vec, &o.vec)) {
                *c = c.max(s.value);
            }
        }
        chosen.push(r);
        if chosen.len() >= REFERENCES {
            break;
        }
        // The least similar remaining candidate; ties to the more recent (earlier in the pool).
        next = pool
            .iter()
            .zip(closest.iter())
            .enumerate()
            .filter(|(_, (slot, _))| slot.is_some())
            .min_by(|(i, (_, a)), (j, (_, b))| a.total_cmp(b).then_with(|| i.cmp(j)))
            .map(|(i, _)| i);
    }
    chosen
}
