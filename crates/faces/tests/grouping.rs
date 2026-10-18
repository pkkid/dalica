//! Grouping and suggestions on synthetic embeddings around seeded centres (`docs/faces.md` §5): no
//! model needed.

use dalica_faces::calibration::{Calibration, CalibrationError, Confidence, STEPS};
use dalica_faces::cluster::{GroupError, Groups, Member};
use dalica_faces::evaluate::{Named, evaluate};
use dalica_faces::matching::{Bars, DEFAULT_MARGIN, Embedding, EmbeddingError, Gallery, NoExact, Packed, REFERENCES, Reference, similarity};
use proptest::prelude::*;

const DIM: usize = 64;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A seeded generator (SplitMix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in −1 … 1.
    fn signed(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }

    fn vector(&mut self) -> Vec<f32> {
        (0..DIM).map(|_| self.signed()).collect()
    }
}

fn unit(v: &[f32]) -> Embedding {
    Embedding::new(v).unwrap()
}

/// `a` mixed with `b` at weight `w` (both unit), as a unit vector.
fn mix(a: &Embedding, b: &[f32], w: f32) -> Embedding {
    let b = unit(b);
    unit(&a.as_slice().iter().zip(b.as_slice()).map(|(x, y)| x + w * y).collect::<Vec<_>>())
}

/// A face of the person at `centre`: about 0.85 cosine from it.
fn face(rng: &mut Rng, centre: &Embedding) -> Embedding {
    let noise = rng.vector();
    mix(centre, &noise, 0.6)
}

/// Three people whose centres are about 0.3 cosine apart (they share a common part).
fn three_centres(rng: &mut Rng) -> Vec<Embedding> {
    let common = unit(&rng.vector());
    (0..3).map(|_| mix(&common, &rng.vector(), 1.5)).collect()
}

fn reference(face: u32, taken: i64, e: &Embedding) -> Reference<u32> {
    Reference { face, taken: Some(taken), vec: e.pack() }
}

#[test]
fn thresholds_rise_with_the_slider_value() {
    let table = Calibration::table([0.20, 0.24, 0.28, 0.31, 0.34, 0.37, 0.40, 0.43, 0.47, 0.52]).unwrap();
    let linear = Calibration::linear(0.43).unwrap();
    for cal in [table, linear] {
        let t: Vec<f32> = Confidence::all().map(|c| cal.threshold(c)).collect();
        assert!(t.windows(2).all(|w| w[0] < w[1]), "{t:?}");
        assert!((cal.threshold(Confidence::default()) - 0.43).abs() < 1e-6, "85 is matchCosine");
    }
    assert!((linear.threshold(Confidence::new(50).unwrap()) - 0.08).abs() < 1e-6);
    assert!((linear.threshold(Confidence::new(95).unwrap()) - 0.53).abs() < 1e-6);
    assert_eq!(Calibration::linear(0.99).unwrap().threshold(Confidence::new(95).unwrap()), 1.0, "kept a cosine");
    // only the slider's steps, and only tables that never fall
    assert_eq!(STEPS.iter().filter(|s| Confidence::new(**s).is_some()).count(), 10);
    assert!([0, 49, 52, 100, 255].iter().all(|v| Confidence::new(*v).is_none()));
    let mut falls = [0.5f32; 10];
    falls[3] = 0.4;
    assert_eq!(Calibration::table(falls), Err(CalibrationError::Falls));
    assert_eq!(Calibration::table([f32::NAN; 10]), Err(CalibrationError::NotACosine));
    assert_eq!(Calibration::linear(1.5), Err(CalibrationError::NotACosine));
}

/// 3 people × 12 faces, `order` interleaved across people.
fn planted(seed: u64) -> (Vec<Member<u32>>, Vec<u32>) {
    let mut rng = Rng(seed);
    let centres = three_centres(&mut rng);
    let mut members = Vec::new();
    let mut person = Vec::new();
    for i in 0..36u32 {
        let p = (i % 3) as usize;
        members.push(Member { face: i, order: u64::from(i), vec: face(&mut rng, &centres[p]).pack() });
        person.push(p as u32);
    }
    (members, person)
}

#[test]
fn three_planted_people_form_three_groups_at_85_and_merge_at_50() {
    let cal = Calibration::linear(0.5).unwrap();
    let (members, person) = planted(7);
    let at = |c: u8| Groups::build(cal.threshold(Confidence::new(c).unwrap()), members.clone()).groups();
    let groups = at(85);
    assert_eq!(groups.len(), 3, "{groups:?}");
    for g in &groups {
        assert_eq!(g.faces.len(), 12);
        let p = person[g.faces[0] as usize];
        assert!(g.faces.iter().all(|f| person[*f as usize] == p), "one person per group: {g:?}");
    }
    let merged = at(50);
    assert_eq!(merged.len(), 1, "{merged:?}");
    assert_eq!(merged[0].faces.len(), 36);
}

#[test]
fn the_merge_pass_joins_clusters_whose_centroids_meet() {
    let mut e = vec![0.0f32; DIM];
    e[0] = 1.0;
    let a = unit(&e);
    e[0] = 0.0;
    e[1] = 1.0;
    let b = unit(&e);
    e[0] = 1.0;
    let between = unit(&e); // 0.707 from both
    let mut g = Groups::new(0.6);
    g.add(Member { face: 0u32, order: 0, vec: a.pack() }).unwrap();
    g.add(Member { face: 1, order: 1, vec: b.pack() }).unwrap();
    assert_eq!(g.len(), 2, "0.0 apart: two clusters");
    for f in 2..8 {
        // the tie between the two goes to the older cluster, whose centroid drifts towards b
        g.add(Member { face: f, order: u64::from(f), vec: between.pack() }).unwrap();
    }
    assert_eq!(g.len(), 2, "adding never merges");
    assert_eq!(g.merge_pass(), 1);
    assert_eq!(g.groups().len(), 1);
    assert_eq!(g.merge_pass(), 0, "nothing changed since");
    // the refusals
    assert_eq!(g.add(Member { face: 0, order: 9, vec: a.pack() }), Err(GroupError::Duplicate));
    let short = unit(&[1.0, 0.0]).pack();
    assert_eq!(g.add(Member { face: 99, order: 9, vec: short }), Err(GroupError::Dimension { expected: DIM, found: 2 }));
    // removing a face takes it out of its cluster; the last face takes the cluster with it
    assert!(g.remove(&1) && !g.remove(&1) && !g.contains(&1));
    assert_eq!(g.faces(), 7);
    for f in [0, 2, 3, 4, 5, 6, 7] {
        assert!(g.remove(&f));
    }
    assert!(g.is_empty());
}

#[test]
fn groups_are_largest_first_closest_first_and_the_same_on_every_run() {
    let cal = Calibration::linear(0.5).unwrap();
    let (mut members, _) = planted(11);
    members.truncate(30); // sizes 10, 10, 10 …
    members.retain(|m| m.face != 2 && m.face != 5); // … then 10, 10, 8
    let t = cal.threshold(Confidence::default());
    let groups = Groups::build(t, members.clone()).groups();
    let sizes: Vec<usize> = groups.iter().map(|g| g.faces.len()).collect();
    assert_eq!(sizes, vec![10, 10, 8]);
    assert!(groups[0].faces.contains(&0), "equal sizes: the earliest photo first");
    // within a group, most similar to the centroid first
    let by_face: std::collections::BTreeMap<u32, &Packed> = members.iter().map(|m| (m.face, &m.vec)).collect();
    for g in &groups {
        let mut sum = vec![0.0f32; DIM];
        for f in &g.faces {
            for (s, x) in sum.iter_mut().zip(by_face[f].unpack()) {
                *s += x;
            }
        }
        let c = unit(&sum);
        let sims: Vec<f32> = g.faces.iter().map(|f| c.cosine(&unit(&by_face[f].unpack())).unwrap()).collect();
        assert!(sims.windows(2).all(|w| w[0] >= w[1] - 1e-3), "{sims:?}");
    }
    // any arrival order gives the same groups
    let mut rng = Rng(3);
    for _ in 0..5 {
        let mut shuffled = members.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, (rng.next() % (i as u64 + 1)) as usize);
        }
        assert_eq!(Groups::build(t, shuffled).groups(), groups);
    }
}

/// Ana (person 1) and Bea (person 2), five faces each.
fn two_people(rng: &mut Rng) -> (Gallery<u32, u32>, Vec<Embedding>) {
    let centres = three_centres(rng);
    let mut g = Gallery::new();
    for p in 0..2u32 {
        let refs = (0..5).map(|i| reference(100 * (p + 1) + i, i64::from(i), &face(rng, &centres[p as usize]))).collect();
        g.set_person(p + 1, refs);
    }
    (g, centres)
}

#[test]
fn a_suggestion_needs_the_threshold_and_the_margin() {
    let mut rng = Rng(21);
    let (g, centres) = two_people(&mut rng);
    let ana = face(&mut rng, &centres[0]).pack();
    let s = g.suggest(&1, &ana, Bars::new(0.5), &NoExact).unwrap();
    assert_eq!((s.person, s.runner_up.map(|r| r.0)), (1, Some(2)));
    assert!(g.suggest(&1, &ana, Bars::new(s.score + 0.02), &NoExact).is_none(), "below the threshold");
    // a near tie: a face halfway between Ana and Bea passes the threshold but not the margin
    let half = mix(&centres[0], centres[1].as_slice(), 1.0);
    let r = g.rank(&2, &half.pack());
    assert!((r[0].score - r[1].score).abs() < DEFAULT_MARGIN, "{r:?}");
    assert!(r[0].score > 0.3);
    assert!(g.suggest(&2, &half.pack(), Bars::new(0.3), &NoExact).is_none());
    assert!(g.suggest(&2, &half.pack(), Bars { threshold: 0.3, margin: 0.0 }, &NoExact).is_some(), "the margin held it back");
}

#[test]
fn confirmed_faces_are_never_suggested_and_rejected_pairs_never_return() {
    let mut rng = Rng(5);
    let (mut g, centres) = two_people(&mut rng);
    let ana = face(&mut rng, &centres[0]);
    // one of Ana's own faces is never offered a name
    let own = g.references(&1)[0].clone();
    assert!(g.is_confirmed(&own.face));
    assert!(g.suggest(&own.face, &own.vec, Bars::new(-1.0), &NoExact).is_none());
    // a rejected pair is skipped, now and after Ana gains faces
    assert_eq!(g.suggest(&7, &ana.pack(), Bars::new(0.4), &NoExact).map(|s| s.person), Some(1));
    g.reject(7, 1);
    assert!(g.rank(&7, &ana.pack()).iter().all(|r| r.person != 1));
    assert_ne!(g.suggest(&7, &ana.pack(), Bars::new(0.4), &NoExact).map(|s| s.person), Some(1));
    let mut refs = g.references(&1).to_vec();
    refs.push(reference(150, 99, &face(&mut rng, &centres[0])));
    g.set_person(1, refs);
    assert_ne!(g.suggest(&7, &ana.pack(), Bars::new(0.4), &NoExact).map(|s| s.person), Some(1));
    assert!(g.find_more(&1, [(7, &ana.pack())], 0.4).is_empty());
    // confirming a face makes it a reference (and never suggested again)
    let new = face(&mut rng, &centres[0]);
    let mut refs = g.references(&1).to_vec();
    refs.push(reference(8, 100, &new));
    g.set_person(1, refs);
    assert!(g.is_confirmed(&8) && g.suggest(&8, &new.pack(), Bars::new(0.4), &NoExact).is_none());
    // deleting a person forgets their confirmations and rejections
    g.remove_person(&1);
    assert!(!g.is_confirmed(&8) && !g.is_rejected(&7, &1) && g.references(&1).is_empty());
}

#[test]
fn a_person_with_ten_thousand_faces_is_scored_against_64() {
    let mut rng = Rng(9);
    let centre = unit(&rng.vector());
    let all: Vec<Reference<u32>> = (0..10_000u32).map(|i| reference(i, i64::from(i), &face(&mut rng, &centre))).collect();
    let mut g = Gallery::new();
    g.set_person(1u32, all.clone());
    let refs = g.references(&1);
    assert_eq!(refs.len(), REFERENCES);
    assert_eq!(refs[0].face, 9_999, "the most recent first");
    assert!(refs.iter().all(|r| r.face >= 10_000 - 4 * REFERENCES as u32), "from the most recent");
    // most varied: the chosen faces are less alike than the 64 most recent
    let spread = |r: &[Reference<u32>]| -> f32 { r.iter().flat_map(|a| r.iter().map(move |b| similarity(&a.vec, &b.vec).unwrap().value)).sum() };
    assert!(spread(refs) < spread(&all[10_000 - REFERENCES..]), "chosen for variety");
    let ranked = g.rank(&99_999, &face(&mut rng, &centre).pack());
    assert_eq!(ranked.len(), 1);
    assert!(ranked[0].score > 0.7);
    // a person whose faces have no usable vector at all is not in the gallery
    g.set_person(2, Vec::new());
    assert!(g.people().eq([&1]));
}

#[test]
fn find_more_lists_near_misses_most_similar_first() {
    let mut rng = Rng(33);
    let (g, centres) = two_people(&mut rng);
    let threshold = 0.75;
    let candidates: Vec<(u32, Packed)> = (0..20u32)
        .map(|i| {
            let c = &centres[(i % 3) as usize];
            (1000 + i, mix(c, &rng.vector(), 0.3 + 0.1 * (i / 3) as f32).pack())
        })
        .collect();
    let more = g.find_more(&1, candidates.iter().map(|(f, v)| (*f, v)), threshold);
    assert!(!more.is_empty());
    assert!(more.iter().all(|(_, s)| *s > 0.8 * threshold));
    assert!(more.windows(2).all(|w| w[0].1 >= w[1].1), "{more:?}");
    // everything above the bar is listed, including faces below the threshold
    let ana = g.references(&1);
    let best = |v: &Packed| ana.iter().map(|r| similarity(v, &r.vec).unwrap().value).fold(f32::MIN, f32::max);
    let expected = candidates.iter().filter(|(_, v)| best(v) > 0.8 * threshold).count();
    assert_eq!(more.len(), expected);
    assert!(more.iter().any(|(_, s)| *s < threshold), "near misses too");
    assert!(g.find_more(&3, candidates.iter().map(|(f, v)| (*f, v)), threshold).is_empty(), "unknown person");
}

#[test]
fn a_decision_within_the_8_bit_error_is_settled_at_float32() {
    let mut rng = Rng(77);
    let centre = unit(&rng.vector());
    let refs: Vec<(u32, Embedding)> = (0..4).map(|i| (i, face(&mut rng, &centre))).collect();
    let query = face(&mut rng, &centre);
    let mut g = Gallery::new();
    g.set_person(1u32, refs.iter().map(|(f, e)| reference(*f, 0, e)).collect());
    let packed = g.rank(&50, &query.pack())[0].score;
    let exact = refs.iter().map(|(_, e)| query.cosine(e).unwrap()).fold(f32::MIN, f32::max);
    assert!((packed - exact).abs() < 0.01 && packed != exact, "{packed} {exact}");
    // a threshold between the two: the 8-bit score and the float32 score disagree
    let threshold = (packed + exact) / 2.0;
    let source = |f: &u32| if *f == 50 { Some(query.clone()) } else { refs.iter().find(|(r, _)| r == f).map(|(_, e)| e.clone()) };
    let with_exact = g.suggest(&50, &query.pack(), Bars::new(threshold), &source).is_some();
    let without = g.suggest(&50, &query.pack(), Bars::new(threshold), &NoExact).is_some();
    assert_eq!(with_exact, exact >= threshold);
    assert_eq!(without, packed >= threshold);
    assert_ne!(with_exact, without);
}

#[test]
fn evaluate_counts_suggestions_per_step_and_ignores_bursts() {
    let mut rng = Rng(41);
    let centres = three_centres(&mut rng);
    let mut named: Vec<Named<u32, u32>> = Vec::new();
    for i in 0..30u32 {
        let p = i % 3;
        named.push(Named { face: i, person: p, taken: Some(1000 * i64::from(i)), vec: face(&mut rng, &centres[p as usize]).pack() });
    }
    let cal = Calibration::linear(0.5).unwrap();
    let report = evaluate(&named, &cal, DEFAULT_MARGIN, &NoExact);
    assert_eq!(report.faces, 30);
    assert_eq!(report.steps.iter().map(|s| s.confidence.value()).collect::<Vec<_>>(), STEPS.to_vec());
    assert!(report.steps.windows(2).all(|w| w[0].suggested >= w[1].suggested), "{report:?}");
    assert!(report.steps.iter().all(|s| s.right <= s.suggested));
    let at85 = report.steps[7];
    assert!(at85.suggested >= 25 && at85.right == at85.suggested, "{at85:?}");
    // a person whose only other face is a burst shot of the same moment gets no suggestion
    let twin = face(&mut rng, &centres[0]);
    let solo =
        vec![Named { face: 0u32, person: 9u32, taken: Some(50), vec: twin.pack() }, Named { face: 1, person: 9, taken: Some(52), vec: twin.pack() }];
    let burst = evaluate(&solo, &cal, DEFAULT_MARGIN, &NoExact);
    assert!(burst.steps.iter().all(|s| s.suggested == 0), "{burst:?}");
    let apart: Vec<Named<u32, u32>> = solo.into_iter().map(|n| Named { taken: n.taken.map(|t| t * 100), ..n }).collect();
    assert!(evaluate(&apart, &cal, DEFAULT_MARGIN, &NoExact).steps.iter().all(|s| s.suggested == 2 && s.right == 2));
}

#[test]
fn embeddings_refuse_what_has_no_direction() {
    assert_eq!(Embedding::new(&[]), Err(EmbeddingError::Empty));
    assert_eq!(Embedding::new(&[0.0; 8]), Err(EmbeddingError::Zero));
    assert_eq!(Embedding::new(&[1.0, f32::NAN]), Err(EmbeddingError::NotFinite));
    assert_eq!(Embedding::new(&[f32::INFINITY]), Err(EmbeddingError::NotFinite));
    assert_eq!(Embedding::new(&vec![1.0; 5000]), Err(EmbeddingError::TooLong(5000)));
    let big = Embedding::new(&[f32::MAX, f32::MAX]).unwrap();
    assert!((big.cosine(&big).unwrap() - 1.0).abs() < 1e-6, "the norm is taken without overflow");
    assert!(similarity(&big.pack(), &unit(&[1.0, 0.0, 0.0]).pack()).is_none(), "lengths differ");
}

fn arb_vec() -> impl Strategy<Value = Vec<f32>> {
    let num = prop_oneof![
        4 => -2.0f32..2.0,
        1 => Just(0.0f32),
        1 => Just(f32::NAN),
        1 => Just(f32::INFINITY),
        1 => Just(f32::MIN_POSITIVE / 4.0),
        1 => Just(f32::MAX),
    ];
    prop::collection::vec(num, 0..6)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if full() { 512 } else { 32 }))]

    #[test]
    fn the_8_bit_score_stays_within_its_bound(a in prop::collection::vec(-1.0f32..1.0, 1..96), seed in any::<u64>()) {
        let mut rng = Rng(seed);
        let b: Vec<f32> = a.iter().map(|_| rng.signed()).collect();
        if let (Ok(a), Ok(b)) = (Embedding::new(&a), Embedding::new(&b)) {
            let s = similarity(&a.pack(), &b.pack()).unwrap();
            let exact = a.cosine(&b).unwrap();
            prop_assert!((s.value - exact).abs() <= s.bound, "{} {} {}", s.value, exact, s.bound);
            prop_assert!(s.bound < 0.1);
        }
    }

    #[test]
    fn no_input_panics(vecs in prop::collection::vec(arb_vec(), 0..24), threshold in prop_oneof![-2.0f32..2.0, Just(f32::NAN), Just(f32::INFINITY)]) {
        let packed: Vec<(u32, Packed)> =
            vecs.iter().enumerate().filter_map(|(i, v)| Embedding::new(v).ok().map(|e| (i as u32, e.pack()))).collect();
        let members = packed.iter().map(|(f, v)| Member { face: *f, order: u64::from(*f % 5), vec: v.clone() });
        let mut groups = Groups::build(threshold, members);
        let listed: usize = groups.groups().iter().map(|g| g.faces.len()).sum();
        prop_assert_eq!(listed, groups.faces());
        if let Some((f, _)) = packed.first() {
            groups.remove(f);
            groups.merge_pass();
        }
        let mut gallery: Gallery<u32, u32> = Gallery::new();
        for (i, chunk) in packed.chunks(3).enumerate() {
            gallery.set_person(i as u32, chunk.iter().map(|(f, v)| Reference { face: *f, taken: None, vec: v.clone() }).collect());
        }
        let source = |f: &u32| vecs.get(*f as usize).and_then(|v| Embedding::new(v).ok());
        for (f, v) in &packed {
            let _ = gallery.suggest(&(f + 1000), v, Bars { threshold, margin: threshold }, &source);
            let _ = gallery.find_more(&0, [(*f + 1000, v)], threshold);
        }
        let named: Vec<Named<u32, u32>> =
            packed.iter().map(|(f, v)| Named { face: *f, person: f % 3, taken: [None, Some(i64::MIN), Some(0), Some(i64::MAX)].get(*f as usize % 4).copied().flatten(), vec: v.clone() }).collect();
        let _ = evaluate(&named, &Calibration::linear(0.4).unwrap(), threshold, &source);
    }
}
