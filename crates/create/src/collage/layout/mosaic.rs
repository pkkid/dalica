//! The Mosaic layout (`creating.md` §3): Dalica's own seeded guillotine partition of the page into
//! one block per picture, filling it without holes, each block's ratio near its picture's.
//!
//! The partition is a binary tree of cuts. Each cut either puts its two halves side by side or one
//! above the other, and a set of pictures laid out by a tree has a *natural* ratio: side by side
//! the halves' ratios add, stacked their inverses add. Fitting the tree into a rectangle places
//! every cut by those natural ratios (the cuts weighted by the pictures' ratios), which turns every
//! block from its picture's ratio by the same factor: the rectangle's ratio over the tree's. So
//! the layout looks for a tree whose natural ratio is the page's:
//!
//! 1. *Grow* a tree top down: a set of pictures is cut across its longer side, at a seeded split
//!    point in the middle third of the set (so the tree stays shallow), each half aiming at the
//!    share of the rectangle its count of pictures would cover at equal areas.
//! 2. *Turn* cuts: every cut in turn is tried the other way and kept that way when that lowers
//!    the cost: the tree's miss of the page's ratio (which turns every block) counted heavily,
//!    plus how far each cut moves its pictures' areas off their equal shares (a few passes; each
//!    try costs the tree's depth).
//! 3. Keep the cheapest of a few seeded trees.
//!
//! The partition is made for the pictures' ratios in slot order before the user's swaps
//! ([`Ctx::slot_aspects`]), so a swap exchanges two pictures' blocks and moves no other, each
//! picture showing the part of itself that fills its block; Shuffle Pictures, a new order, makes
//! a new partition (the same one when every picture has the same ratio).
//!
//! The spacing is a gap between the blocks and round the page's edge: the tree tiles the page
//! inset by half a gap, and each block gives up half a gap on every side. The gap is narrowed to
//! half the smallest block's side when the pictures are too many for it.

use super::{Ctx, Slot, fill, sane_aspect};
use crate::collage::seed::Rng;

/// How many seeded trees are grown and the best one kept.
const CANDIDATES: u64 = 4;
/// How many times every cut is tried the other way.
const TURN_PASSES: usize = 4;
/// How much more the whole tree's miss of the page counts than one cut's strain.
const ROOT_WEIGHT: f64 = 8.0;

/// A rectangle in page units: the page's height is 1 and its width the page's ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    /// The page of ratio `ratio`.
    pub fn page(ratio: f64) -> Rect {
        Rect { x0: 0.0, y0: 0.0, x1: ratio, y1: 1.0 }
    }

    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn area(&self) -> f64 {
        self.w().max(0.0) * self.h().max(0.0)
    }

    /// Width over height, as a layout can use it.
    pub fn ratio(&self) -> f64 {
        sane_aspect(Some(self.w() / self.h()))
    }

    /// `d` smaller on every side.
    pub fn inset(&self, d: f64) -> Rect {
        Rect { x0: self.x0 + d, y0: self.y0 + d, x1: self.x1 - d, y1: self.y1 - d }
    }
}

/// The Mosaic: the page partitioned for the slots' pictures.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    let ratio = ctx.page_ratio;
    finish(ctx, |gap| partition(Rect::page(ratio).inset(gap / 2.0), ctx.slot_aspects, ctx.seed, 0))
}

/// The slots of a partition: `expanded(gap)` answers one rectangle per slot tiling the page inset
/// by half of `gap`, and each slot is its rectangle less half the gap on every side, showing the
/// part of its picture that fills it.
pub(super) fn finish(ctx: &Ctx, expanded: impl Fn(f64) -> Vec<Rect>) -> Vec<Slot> {
    let ratio = ctx.page_ratio;
    let mut gap = ctx.settings.spacing.clamp(0.0, 1.0) * ratio.min(1.0);
    if !gap.is_finite() {
        gap = 0.0;
    }
    let mut rects = expanded(gap);
    let smallest = rects.iter().map(|r| r.w().min(r.h())).fold(f64::INFINITY, f64::min);
    if smallest.is_finite() && gap > smallest / 2.0 {
        gap = (smallest / 2.0).max(0.0);
        rects = expanded(gap);
    }
    rects
        .iter()
        .zip(ctx.aspects)
        .map(|(r, &aspect)| {
            let b = r.inset(gap / 2.0);
            let (w, h) = (b.w().max(0.0), b.h().max(0.0));
            Slot { centre: [(b.x0 + b.x1) / 2.0 / ratio, (b.y0 + b.y1) / 2.0], size: [w / ratio, h], angle: 0.0, shown: fill(aspect, w / h) }
        })
        .collect()
}

/// `rect` cut into one rectangle per ratio of `aspects` (in their order), tiling it exactly.
/// `stream` keeps two partitions of one seed (Frame Mosaic's bands) apart.
pub(super) fn partition(rect: Rect, aspects: &[f64], seed: u64, stream: u64) -> Vec<Rect> {
    if aspects.is_empty() {
        return Vec::new();
    }
    let target = rect.ratio();
    let mut best: Option<(f64, Tree)> = None;
    for candidate in 0..CANDIDATES {
        let mut rng = Rng::stream(seed, stream.wrapping_mul(CANDIDATES).wrapping_add(candidate));
        let mut tree = Tree::grow(aspects, target, &mut rng);
        tree.turn(target);
        let miss = tree.cost(target);
        if best.as_ref().is_none_or(|(m, _)| miss < *m) {
            best = Some((miss, tree));
        }
    }
    let mut out = vec![rect; aspects.len()];
    if let Some((_, tree)) = best {
        tree.place(rect, &mut out);
    }
    out
}

/// A cut or a picture.
#[derive(Clone, Copy, Debug)]
enum Node {
    /// The picture at this position of the partition's ratios.
    Leaf(usize),
    /// Two halves side by side (`across`) or one above the other.
    Cut { across: bool, a: usize, b: usize },
}

/// A partition's tree, the root last. Per node: its parent, its natural ratio, how many pictures
/// it holds and, for a cut, the share of its area its first half's pictures should have (as
/// many shares as pictures: every picture should cover about as much of the page).
#[derive(Clone, Debug)]
struct Tree {
    nodes: Vec<Node>,
    parent: Vec<Option<usize>>,
    natural: Vec<f64>,
    count: Vec<usize>,
    share: Vec<f64>,
}

impl Tree {
    /// A tree for `aspects` aiming at `target`.
    fn grow(aspects: &[f64], target: f64, rng: &mut Rng) -> Tree {
        let cap = 2 * aspects.len();
        let mut tree = Tree {
            nodes: Vec::with_capacity(cap),
            parent: Vec::with_capacity(cap),
            natural: Vec::with_capacity(cap),
            count: Vec::with_capacity(cap),
            share: Vec::with_capacity(cap),
        };
        tree.grow_part(aspects, 0, aspects.len(), target, rng);
        tree
    }

    /// The node for `aspects[lo..hi]` (at least one) aiming at `target`, cut across its longer
    /// side. The split point lies in the middle third, so the depth stays under log base 3/2 of
    /// the count.
    fn grow_part(&mut self, aspects: &[f64], lo: usize, hi: usize, target: f64, rng: &mut Rng) -> usize {
        let n = hi.saturating_sub(lo);
        if n <= 1 {
            return self.push(Node::Leaf(lo), sane_aspect(aspects.get(lo).copied()), 1, 1.0);
        }
        let across = target >= 1.0;
        let first = n.div_ceil(3).max(1);
        let last = (2 * n / 3).clamp(first, n - 1);
        let k = first + rng.below((last - first + 1) as u64) as usize;
        let share = k as f64 / n as f64;
        let (ta, tb) = if across { (target * share, target * (1.0 - share)) } else { (target / share, target / (1.0 - share)) };
        let a = self.grow_part(aspects, lo, lo + k, sane_aspect(Some(ta)), rng);
        let b = self.grow_part(aspects, lo + k, hi, sane_aspect(Some(tb)), rng);
        let natural = combine(across, self.natural_of(a), self.natural_of(b));
        let id = self.push(Node::Cut { across, a, b }, natural, n, share);
        for child in [a, b] {
            if let Some(p) = self.parent.get_mut(child) {
                *p = Some(id);
            }
        }
        id
    }

    fn push(&mut self, node: Node, natural: f64, count: usize, share: f64) -> usize {
        self.nodes.push(node);
        self.parent.push(None);
        self.natural.push(natural);
        self.count.push(count);
        self.share.push(share);
        self.nodes.len() - 1
    }

    fn natural_of(&self, node: usize) -> f64 {
        self.natural.get(node).copied().unwrap_or(1.0)
    }

    fn count_of(&self, node: usize) -> f64 {
        self.count.get(node).copied().unwrap_or(1) as f64
    }

    fn root(&self) -> usize {
        self.nodes.len().saturating_sub(1)
    }

    /// The share of a cut's area its first half gets when the tree is fitted (see [`Tree::place`]).
    fn placed_share(&self, across: bool, a: usize, b: usize) -> f64 {
        let (na, nb) = (self.natural_of(a), self.natural_of(b));
        // Side by side the widths go as the natural ratios; stacked the heights go as their
        // inverses, so the first half's share is the second's ratio.
        let share = if across { na / (na + nb) } else { nb / (na + nb) };
        if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.5 }
    }

    /// How far the tree's ratio is from `target`, as the logarithm of their quotient: every block
    /// turns from its picture's ratio by this much.
    fn miss(&self, target: f64) -> f64 {
        (target / self.natural_of(self.root())).ln().abs()
    }

    /// How far a cut moves its pictures' areas off their shares: the logarithm of each half's
    /// area over its share, counted once per picture in it.
    fn strain(&self, node: usize) -> f64 {
        let Some(&Node::Cut { across, a, b }) = self.nodes.get(node) else { return 0.0 };
        let want = self.share.get(node).copied().unwrap_or(0.5);
        let got = self.placed_share(across, a, b).clamp(1e-12, 1.0 - 1e-12);
        self.count_of(a) * (got / want).ln().abs() + self.count_of(b) * ((1.0 - got) / (1.0 - want)).ln().abs()
    }

    /// The part of the cost a turn of `node` changes: the strain of it and its ancestors, and
    /// the tree's miss of `target`.
    fn cost_above(&self, node: usize, target: f64) -> f64 {
        let mut cost = self.miss_cost(target);
        let mut at = Some(node);
        while let Some(n) = at {
            cost += self.strain(n);
            at = self.parent.get(n).copied().flatten();
        }
        cost
    }

    /// The miss counted for every picture, [`ROOT_WEIGHT`] times over.
    fn miss_cost(&self, target: f64) -> f64 {
        ROOT_WEIGHT * self.count_of(self.root()) * self.miss(target)
    }

    /// The tree's whole cost for `target` (the candidates are compared by it).
    fn cost(&self, target: f64) -> f64 {
        self.miss_cost(target) + (0..self.nodes.len()).map(|n| self.strain(n)).sum::<f64>()
    }

    /// Tries every cut the other way, keeping each turn that lowers the cost.
    fn turn(&mut self, target: f64) {
        for _ in 0..TURN_PASSES {
            let mut turned = false;
            for id in 0..self.nodes.len() {
                if !matches!(self.nodes.get(id), Some(Node::Cut { .. })) {
                    continue;
                }
                let before = self.cost_above(id, target);
                self.flip(id);
                if self.cost_above(id, target) + 1e-12 < before {
                    turned = true;
                } else {
                    self.flip(id);
                }
            }
            if !turned {
                break;
            }
        }
    }

    /// Turns the cut `id` and updates the natural ratios up to the root.
    fn flip(&mut self, id: usize) {
        if let Some(Node::Cut { across, .. }) = self.nodes.get_mut(id) {
            *across = !*across;
        }
        let mut at = Some(id);
        while let Some(node) = at {
            if let Some(&Node::Cut { across, a, b }) = self.nodes.get(node) {
                let natural = combine(across, self.natural_of(a), self.natural_of(b));
                if let Some(v) = self.natural.get_mut(node) {
                    *v = natural;
                }
            }
            at = self.parent.get(node).copied().flatten();
        }
    }

    /// Writes each picture's rectangle into `out`, the tree fitted to `rect`.
    fn place(&self, rect: Rect, out: &mut [Rect]) {
        let mut stack = vec![(self.root(), rect)];
        while let Some((node, r)) = stack.pop() {
            match self.nodes.get(node) {
                Some(&Node::Leaf(i)) => {
                    if let Some(slot) = out.get_mut(i) {
                        *slot = r;
                    }
                }
                Some(&Node::Cut { across, a, b }) => {
                    let share = self.placed_share(across, a, b);
                    let (ra, rb) = if across {
                        let x = r.x0 + r.w() * share;
                        (Rect { x1: x, ..r }, Rect { x0: x, ..r })
                    } else {
                        let y = r.y0 + r.h() * share;
                        (Rect { y1: y, ..r }, Rect { y0: y, ..r })
                    };
                    stack.push((a, ra));
                    stack.push((b, rb));
                }
                None => {}
            }
        }
    }
}

/// The natural ratio of two halves side by side (`across`) or stacked.
fn combine(across: bool, a: f64, b: f64) -> f64 {
    let v = if across { a + b } else { a * b / (a + b) };
    if v.is_finite() && v > 0.0 { v } else { 1.0 }
}
