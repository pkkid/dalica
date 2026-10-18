//! "Evaluate on your photos" (`faces.evaluate`, `docs/faces.md` §5): hide each named face in turn,
//! ask who it looks like from the others, and count per confidence step how many suggestions would
//! be made and how many would be right, so the user can choose the Suggestion confidence from
//! evidence. Shots taken within [`BURST_SECONDS`] of the hidden face are ignored too: a burst of
//! the same moment would make every answer look easy. Our own code.

use std::collections::BTreeMap;

use crate::calibration::{Calibration, Confidence};
use crate::matching::{Bars, ExactSource, Gallery, Packed, Reference};

/// Shots this close in time (seconds) to the hidden face are left out of its gallery.
pub const BURST_SECONDS: u64 = 5;

/// One named face.
#[derive(Clone, Debug, PartialEq)]
pub struct Named<F, P> {
    pub face: F,
    pub person: P,
    /// When the photo was taken (seconds; `None` when unknown, then never a burst).
    pub taken: Option<i64>,
    pub vec: Packed,
}

/// The result at one confidence step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub confidence: Confidence,
    pub threshold: f32,
    /// Hidden faces that got a "Name?".
    pub suggested: usize,
    /// Of those, the ones named after the right person.
    pub right: usize,
}

/// What `faces.evaluate` reports.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// How many named faces were hidden in turn.
    pub faces: usize,
    /// One per step, lowest confidence first.
    pub steps: Vec<Step>,
}

fn burst(a: Option<i64>, b: Option<i64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a.abs_diff(b) <= BURST_SECONDS)
}

/// Hide each of `named` in turn and suggest a person for it from the others' references (the same
/// [`crate::matching::REFERENCES`] per person that suggestions use), at every step of `calibration`.
pub fn evaluate<F: Clone + Ord, P: Clone + Ord>(
    named: &[Named<F, P>],
    calibration: &Calibration,
    margin: f32,
    exact: &impl ExactSource<F>,
) -> Report {
    let mut by_person: BTreeMap<P, Vec<Reference<F>>> = BTreeMap::new();
    for n in named {
        by_person.entry(n.person.clone()).or_default().push(Reference { face: n.face.clone(), taken: n.taken, vec: n.vec.clone() });
    }
    let mut gallery: Gallery<F, P> = Gallery::new();
    for (p, refs) in by_person {
        gallery.set_person(p, refs);
    }
    let mut steps: Vec<Step> =
        Confidence::all().map(|c| Step { confidence: c, threshold: calibration.threshold(c), suggested: 0, right: 0 }).collect();
    for hidden in named {
        let keep = |r: &Reference<F>| r.face != hidden.face && !burst(r.taken, hidden.taken);
        let ranked = gallery.rank_where(&hidden.vec, |_| true, keep);
        for step in &mut steps {
            let bars = Bars { threshold: step.threshold, margin };
            if let Some(s) = gallery.decide(&hidden.face, ranked.clone(), bars, exact, keep) {
                step.suggested += 1;
                if s.person == hidden.person {
                    step.right += 1;
                }
            }
        }
    }
    Report { faces: named.len(), steps }
}
