//! Property tests for position keys (`data-model.md` §3.2, `architecture.md` §11): a key always
//! exists between two different keys, ordering is total and byte-wise, and keys stay short.

use dalica_core::PosKey;
use dalica_core::poskey::MAX_KEY_LEN;
use proptest::prelude::*;

/// One integer of the key encoding: a head letter and the digits it calls for.
fn integer() -> impl Strategy<Value = String> {
    let digit = proptest::sample::select(b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".to_vec());
    // Bias towards the extremes, where carries cross heads.
    let digit = prop_oneof![1 => Just(b'0'), 1 => Just(b'z'), 4 => digit];
    let head = proptest::sample::select(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".to_vec());
    let head = prop_oneof![1 => Just(b'A'), 1 => Just(b'z'), 2 => Just(b'a'), 2 => Just(b'Z'), 6 => head];
    head.prop_flat_map(move |h| {
        let n = if h.is_ascii_lowercase() { usize::from(h - b'a') + 1 } else { usize::from(b'Z' - h) + 1 };
        proptest::collection::vec(digit.clone(), n).prop_map(move |d| {
            let mut s = String::from(char::from(h));
            s.extend(d.into_iter().map(char::from));
            s
        })
    })
}

/// Any valid key: one to four integers, the last not the smallest integer.
fn key() -> impl Strategy<Value = PosKey> {
    proptest::collection::vec(integer(), 1..=4).prop_filter_map("ends in the smallest integer", |ints| PosKey::parse(&ints.concat()).ok())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn between_sorts_strictly_between_any_two_keys(a in key(), b in key()) {
        prop_assume!(a != b);
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let mid = PosKey::between(&lo, &hi).unwrap();
        prop_assert!(lo < mid && mid < hi, "{lo} < {mid} < {hi}");
        prop_assert!(lo.as_str().as_bytes() < mid.as_str().as_bytes() && mid.as_str().as_bytes() < hi.as_str().as_bytes());
        // The result is itself a valid key, and so is a key on each side of it.
        prop_assert_eq!(PosKey::parse(mid.as_str()).unwrap(), mid.clone());
        prop_assert!(PosKey::between(&lo, &mid).is_ok() && PosKey::between(&mid, &hi).is_ok());
    }

    #[test]
    fn after_and_before_move_the_right_way(k in key()) {
        prop_assert!(k.after().unwrap() > k);
        prop_assert!(k.before().unwrap() < k);
    }

    #[test]
    fn between_refuses_keys_out_of_order(a in key(), b in key()) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(PosKey::between(&hi, &lo).is_err());
    }

    #[test]
    fn parse_never_panics_and_round_trips(s in "\\PC{0,40}|[A-Za-z0-9]{0,60}") {
        if let Ok(k) = PosKey::parse(&s) {
            prop_assert_eq!(k.as_str(), s.as_str());
            let json = serde_json::to_string(&k).unwrap();
            prop_assert_eq!(serde_json::from_str::<PosKey>(&json).unwrap(), k);
        }
    }

    #[test]
    fn random_inserts_keep_a_sorted_list(positions in proptest::collection::vec(any::<prop::sample::Index>(), 1..400)) {
        let mut list = vec![PosKey::first()];
        for at in positions {
            let i = at.index(list.len() + 1);
            let k = match (i.checked_sub(1).and_then(|p| list.get(p)), list.get(i)) {
                (None, Some(next)) => next.before().unwrap(),
                (Some(prev), None) => prev.after().unwrap(),
                (Some(prev), Some(next)) => PosKey::between(prev, next).unwrap(),
                (None, None) => unreachable!(),
            };
            list.insert(i, k);
        }
        prop_assert!(list.windows(2).all(|w| w[0] < w[1]));
    }
}

/// Inserts 10 000 keys with `next(list)` choosing the slot, and returns the longest key.
fn longest_after_10k(slot: impl Fn(usize) -> usize) -> usize {
    let mut list = vec![PosKey::first(), PosKey::first().after().unwrap()];
    for _ in 0..10_000 {
        let i = slot(list.len());
        let k = match (i.checked_sub(1).and_then(|p| list.get(p)), list.get(i)) {
            (None, Some(next)) => next.before().unwrap(),
            (Some(prev), None) => prev.after().unwrap(),
            (Some(prev), Some(next)) => PosKey::between(prev, next).unwrap(),
            (None, None) => unreachable!(),
        };
        list.insert(i, k);
    }
    assert!(list.windows(2).all(|w| w[0] < w[1]));
    list.iter().map(|k| k.as_str().len()).max().unwrap()
}

/// Where the next insert goes, given the list's length.
type Slot = fn(usize) -> usize;

#[test]
fn keys_stay_short_after_10_000_inserts_at_one_spot() {
    let cases: [(&str, Slot, usize); 4] = [
        ("append at the end", |len| len, 9),
        ("prepend at the start", |_| 0, 9),
        ("insert at index 1, before the previous insert", |_| 1, 9),
        ("insert right after the previous insert (typing at a cursor)", |len| len - 1, 9),
    ];
    for (what, slot, bound) in cases {
        let longest = longest_after_10k(slot);
        println!("{what}: longest key {longest} bytes");
        assert!(longest <= bound, "{what}: {longest} > {bound}");
    }
}

/// Inserting into the middle of a growing list squeezes one gap from both sides, so keys grow
/// linearly with any scheme. They must still last a long time, and the cap must be a clean error
/// (the caller then rewrites the album's keys), never a panic.
#[test]
fn middle_inserts_last_long_and_end_in_a_clean_error() {
    use dalica_core::Error;
    let mut list = vec![PosKey::first(), PosKey::first().after().unwrap()];
    let mut inserted = 0;
    loop {
        let i = list.len() / 2;
        match PosKey::between(&list[i - 1], &list[i]) {
            Ok(k) => list.insert(i, k),
            Err(e) => {
                assert_eq!(e, Error::KeyTooLong(MAX_KEY_LEN));
                break;
            }
        }
        inserted += 1;
    }
    println!("insert in the middle: {inserted} inserts before a key passes {MAX_KEY_LEN} bytes");
    assert!(inserted >= 2_000, "{inserted}");
    assert!(list.windows(2).all(|w| w[0] < w[1]));
}
