//! The field-by-field sync decision (`xmp-interop.md` §5, `sync`): every rule on its own, the
//! hashes pinned (they live in the index, so their form never changes), and a property test that
//! runs edits in Dalica and in another application against a model of who changed what.

use dalica_core::DateTime;
use dalica_library::Synced;
use dalica_library::sync::{Decision, Field, Values, decide, decide_field, reconcile};
use proptest::prelude::*;

fn values(caption: &str, tags: &[&str], star: bool) -> Values {
    Values { caption: caption.to_owned(), tags: tags.iter().map(|t| (*t).to_owned()).collect(), star, place: None, date: None }
}

fn date(s: &str) -> DateTime {
    s.parse().unwrap()
}

#[test]
fn each_rule_of_section_5() {
    use Decision::*;
    let (a, b, c) = (1, 2, 3);
    // The file and Dalica agree: nothing, whatever else.
    for synced in [None, Some(a), Some(b)] {
        for first in [false, true] {
            assert_eq!(decide(a, a, synced, first), Keep);
        }
    }
    // Rule 1: first scan imports.
    assert_eq!(decide(a, b, None, true), Import);
    assert_eq!(decide(a, b, Some(b), true), Import);
    // Rule 2 without a last-synced value (after a rebuild): Dalica's value wins.
    assert_eq!(decide(a, b, None, false), Write);
    // Only Dalica changed: written.
    assert_eq!(decide(a, b, Some(a), false), Write);
    // Rule 3: only the file changed: imported.
    assert_eq!(decide(a, b, Some(b), false), Import);
    // Both changed: Dalica's kept, reported.
    assert_eq!(decide(a, b, Some(c), false), Conflict);
}

#[test]
fn a_date_is_never_removed_from_a_file() {
    let file = Values { date: Some(date("2026-06-04T19:42:10")), ..Values::default() };
    let ours = Values::default();
    // No last-synced value: rule 2 would write "no date"; there is nothing to write.
    assert_eq!(decide_field(Field::Date, &file, &ours, None, false), Decision::Keep);
    let synced = Values { date: Some(date("2020-01-01T00:00:00")), ..Values::default() }.hashes();
    assert_eq!(decide_field(Field::Date, &file, &ours, Some(&synced), false), Decision::Conflict);
    // The other way round is a write.
    assert_eq!(decide_field(Field::Date, &ours, &file, None, false), Decision::Write);
    // A place can be removed.
    let placed = Values { place: Some((1.0, 2.0)), ..Values::default() };
    assert_eq!(decide_field(Field::Place, &placed, &Values::default(), None, false), Decision::Write);
}

#[test]
fn reconcile_decides_every_field_in_order() {
    let file = values("From the file", &["a"], true);
    let ours = values("Ours", &["a"], false);
    let synced = Synced { caption: Some(ours.hash(Field::Caption)), ..file.hashes() };
    let got = reconcile(&file, &ours, Some(&synced), false);
    assert_eq!(
        got,
        [
            (Field::Caption, Decision::Import),
            (Field::Tags, Decision::Keep),
            (Field::Rating, Decision::Write),
            (Field::Place, Decision::Keep),
            (Field::Date, Decision::Keep),
        ]
    );
    assert_eq!(Field::ALL.map(Field::name), ["caption", "tags", "star", "place", "date taken"]);
}

#[test]
fn hashes_compare_what_matters() {
    let h = |v: &Values, f| v.hash(f);
    // Tags are a set: their order means nothing, their spelling does.
    assert_eq!(h(&values("", &["b", "a"], false), Field::Tags), h(&values("", &["a", "b"], false), Field::Tags));
    assert_ne!(h(&values("", &["A"], false), Field::Tags), h(&values("", &["a"], false), Field::Tags));
    assert_ne!(h(&values("", &["ab", "c"], false), Field::Tags), h(&values("", &["a", "bc"], false), Field::Tags));
    assert_ne!(h(&values("", &[], false), Field::Tags), h(&values("", &[""], false), Field::Tags));
    // Equal bytes in two fields hash apart.
    assert_ne!(h(&values("x", &[], false), Field::Caption), h(&values("", &["x"], false), Field::Tags));
    // A place to the micro-degree: what a degree/minute/second round trip moves stays equal.
    let p = |lat, lon| Values { place: Some((lat, lon)), ..Values::default() };
    assert_eq!(h(&p(-33.875, 151.21), Field::Place), h(&p(-33.875_000_2, 151.209_999_9), Field::Place));
    assert_ne!(h(&p(-33.875, 151.21), Field::Place), h(&p(-33.875_01, 151.21), Field::Place));
    assert_ne!(h(&p(0.0, 0.0), Field::Place), h(&Values::default(), Field::Place));
    // Values that are not numbers still hash (as 0).
    assert_eq!(h(&p(f64::NAN, f64::INFINITY), Field::Place), h(&p(0.0, 0.0), Field::Place));
    // Dates to the second.
    let d = |s| Values { date: Some(date(s)), ..Values::default() };
    assert_ne!(h(&d("2026-06-04T19:42:10"), Field::Date), h(&d("2026-06-04T19:42:11"), Field::Date));
}

/// The hashes are stored in the index: these numbers must never change.
#[test]
fn hashes_are_pinned() {
    let v = Values {
        caption: "Pier at dawn, ünïcödé".to_owned(),
        tags: vec!["sea".to_owned(), "trip/2026".to_owned()],
        star: true,
        place: Some((-33.875, 151.21)),
        date: Some(date("2026-06-04T19:42:10")),
    };
    let got: Vec<u64> = Field::ALL.iter().map(|f| v.hash(*f)).collect();
    let empty: Vec<u64> = Field::ALL.iter().map(|f| Values::default().hash(*f)).collect();
    assert_eq!(got, PINNED, "{got:#x?}");
    assert_eq!(empty, PINNED_EMPTY, "{empty:#x?}");
}

const PINNED: [u64; 5] = [0x91f6_34bb_6cb0_e058, 0x87fb_4b59_b598_c771, 0x0839_5307_b4f1_348c, 0xdef3_dd08_ba58_70b0, 0x4bb9_f2b5_c582_64e0];
const PINNED_EMPTY: [u64; 5] = [0xe604_823a_2490_29bf, 0x529a_2cdc_8ff5_33ac, 0x0839_5407_b4f1_363f, 0x0835_ee07_b4ee_5316, 0x0824_f007_b4df_e349];

#[test]
fn synced_hashes_by_field() {
    let mut s = Synced::default();
    for (i, f) in Field::ALL.into_iter().enumerate() {
        assert_eq!(s.get(f), None);
        s.set(f, Some(i as u64 + 10));
    }
    assert_eq!(s, Synced { caption: Some(10), tags: Some(11), rating: Some(12), place: Some(13), date: Some(14) });
    s.set(Field::Tags, None);
    assert_eq!(s.tags, None);
    let v = values("c", &["t"], true);
    let all = v.hashes();
    assert!(Field::ALL.iter().all(|f| all.get(*f) == Some(v.hash(*f))));
}

/// One field edited in Dalica, in another application, both or neither between two syncs.
#[derive(Clone, Debug)]
enum Event {
    Ours(u8),
    Theirs(u8),
    Sync,
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![(0u8..4).prop_map(Event::Ours), (0u8..4).prop_map(Event::Theirs), Just(Event::Sync)]
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    /// Against a model that remembers the value at the last sync: an edit made only in Dalica is
    /// written, one made only elsewhere is imported, one made on both sides is a conflict with
    /// Dalica's value kept, and after each sync both sides agree. Before the first sync (no
    /// last-synced value) Dalica's value wins.
    #[test]
    fn edits_on_either_side_go_the_right_way(start in 0u8..4, events in prop::collection::vec(event(), 0..40)) {
        let caption = |n: u8| values(&format!("caption {n}"), &[], false);
        let (mut file, mut ours) = (start, start);
        let mut base: Option<u8> = None;
        for e in events {
            match e {
                Event::Ours(v) => ours = v,
                Event::Theirs(v) => file = v,
                Event::Sync => {
                    let synced = base.map(|b| caption(b).hashes());
                    let d = decide_field(Field::Caption, &caption(file), &caption(ours), synced.as_ref(), false);
                    let want = if file == ours {
                        Decision::Keep
                    } else {
                        match base {
                            None => Decision::Write,
                            Some(b) if b == file => Decision::Write,
                            Some(b) if b == ours => Decision::Import,
                            Some(_) => Decision::Conflict,
                        }
                    };
                    prop_assert_eq!(d, want);
                    match d {
                        Decision::Write | Decision::Conflict => file = ours,
                        Decision::Import => ours = file,
                        Decision::Keep => {}
                    }
                    prop_assert_eq!(file, ours);
                    base = Some(file);
                }
            }
        }
    }
}
