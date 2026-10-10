//! Position keys (`data-model.md` §3.2): short strings of digits and ASCII letters that sort in
//! album order by plain byte comparison, with a key between any two different keys.
//!
//! # Encoding
//!
//! A key is a sequence of one or more *integers*. Each integer is a head byte followed by base-62
//! digits (`0-9A-Za-z`, in byte order); the head says how many digits follow:
//!
//! - `a` … `z`: 1 … 26 digits, non-negative values, longer = larger (`a0` is zero, `az` + 1 = `b00`);
//! - `A` … `Z`: 26 … 1 digits, negative values, shorter = larger (`Zz` is −1, `Z0` − 1 = `Yzz`).
//!
//! Integers are prefix-free and their byte order is their numeric order, so comparing two keys
//! byte by byte compares their integer sequences lexicographically, with a key sorting before
//! every longer key it is a prefix of. The smallest integer (`A` and 26 zeros) never ends a key,
//! which is what guarantees a key between any two.
//!
//! # Choosing a key
//!
//! Halving the gap on every insert adds a digit every ~6 inserts at the same spot (10 000 inserts →
//! ~1 700 characters). Instead, [`PosKey::after`] and [`PosKey::before`] step the first integer by
//! a fixed [`STEP`], and [`PosKey::between`] takes the midpoint of the first differing integers when
//! there is room, or keeps `a`'s integer and steps one level down. Inserting repeatedly at one spot
//! (appending, prepending, before or after the previous insert) therefore grows keys with the
//! logarithm of the number of inserts: 10 000 such inserts stay under 10 bytes. Inserting into the
//! middle of a growing list squeezes the gap from both sides and grows keys linearly with any
//! scheme (about a byte per 5 inserts here); past [`MAX_KEY_LEN`] the caller rewrites the album's
//! keys.
//!
//! Integers of up to 21 digits are stepped as numbers; longer ones (never reached in practice)
//! step by one, which keeps every operation total.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{Error, excerpt};

/// The longest key accepted or produced, in bytes. Longer input (from a hostile sidecar) is refused.
pub const MAX_KEY_LEN: usize = 512;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const ZERO_DIGIT: u8 = b'0';
const TOP_DIGIT: u8 = b'z';
/// The integer zero, the first key of an empty album.
const ZERO: &[u8] = b"a0";
/// How far `after` and `before` move an integer: room for ~12 midpoint inserts on each side.
pub const STEP: i128 = 62 * 62;
/// The most digits an integer may have to be handled as an `i128`.
const NUMERIC_DIGITS: usize = 21;

/// A position key. Ordering is byte order of the string.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PosKey(String);

impl PosKey {
    /// The key for the first member of an empty album.
    pub fn first() -> PosKey {
        PosKey(String::from("a0"))
    }

    /// Checks that `s` is a well-formed key.
    pub fn parse(s: &str) -> Result<PosKey, Error> {
        if s.len() > MAX_KEY_LEN {
            return Err(Error::InvalidKey(excerpt(s)));
        }
        integers(s.as_bytes()).map_err(|()| Error::InvalidKey(excerpt(s)))?;
        Ok(PosKey(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A key that sorts after `self`.
    pub fn after(&self) -> Result<PosKey, Error> {
        finish(after(&self.split()))
    }

    /// A key that sorts before `self`.
    pub fn before(&self) -> Result<PosKey, Error> {
        finish(before(&self.split())?)
    }

    /// A key that sorts strictly between `a` and `b`; `a` must sort before `b`.
    pub fn between(a: &PosKey, b: &PosKey) -> Result<PosKey, Error> {
        if a >= b {
            return Err(Error::KeysNotOrdered { a: excerpt(&a.0), b: excerpt(&b.0) });
        }
        finish(between(&a.split(), &b.split())?)
    }

    /// The integers of a key that was checked when it was made.
    fn split(&self) -> Vec<&[u8]> {
        integers(self.0.as_bytes()).unwrap_or_default()
    }
}

impl fmt::Display for PosKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for PosKey {
    type Error = Error;

    fn try_from(s: String) -> Result<PosKey, Error> {
        PosKey::parse(&s)
    }
}

impl From<PosKey> for String {
    fn from(k: PosKey) -> String {
        k.0
    }
}

fn finish(bytes: Vec<u8>) -> Result<PosKey, Error> {
    if bytes.len() > MAX_KEY_LEN {
        return Err(Error::KeyTooLong(MAX_KEY_LEN));
    }
    // Every byte comes from `DIGITS` or is a head letter, so this is always ASCII.
    String::from_utf8(bytes).map(PosKey).map_err(|e| Error::InvalidKey(excerpt(&String::from_utf8_lossy(e.as_bytes()))))
}

/// Digits that follow a head byte, or `None` if `head` is not a head.
fn digits_after(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some(usize::from(head - b'a') + 1),
        b'A'..=b'Z' => Some(usize::from(b'Z' - head) + 1),
        _ => None,
    }
}

fn is_digit(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// The smallest integer: head `A`, 26 zero digits.
fn is_min(int: &[u8]) -> bool {
    int.first() == Some(&b'A') && int.iter().skip(1).all(|&d| d == ZERO_DIGIT)
}

/// Splits a key into its integers; `Err` unless it is non-empty, well formed and does not end in
/// the smallest integer.
fn integers(mut s: &[u8]) -> Result<Vec<&[u8]>, ()> {
    let mut out = Vec::new();
    while let Some(&head) = s.first() {
        let n = digits_after(head).ok_or(())?;
        let int = s.get(..=n).ok_or(())?;
        if !int.iter().skip(1).all(|&b| is_digit(b)) {
            return Err(());
        }
        out.push(int);
        s = s.get(n + 1..).ok_or(())?;
    }
    match out.last() {
        Some(last) if !is_min(last) => Ok(out),
        _ => Err(()),
    }
}

fn digit_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'A'..=b'Z' => b - b'A' + 10,
        _ => b.saturating_sub(b'a').saturating_add(36),
    }
}

fn digit(v: u8) -> u8 {
    DIGITS.get(usize::from(v)).copied().unwrap_or(ZERO_DIGIT)
}

/// The integer one larger, or `None` for the largest.
fn inc(int: &[u8]) -> Option<Vec<u8>> {
    let (&head, digits) = int.split_first()?;
    let mut digits = digits.to_vec();
    for d in digits.iter_mut().rev() {
        if *d == TOP_DIGIT {
            *d = ZERO_DIGIT;
        } else {
            *d = digit(digit_value(*d) + 1);
            return Some([&[head][..], &digits].concat());
        }
    }
    // Every digit was the top digit: move to the next head.
    let len = digits.len();
    let (head, len) = match head {
        b'z' => return None,
        b'Z' => return Some(ZERO.to_vec()),
        b'a'..=b'y' => (head + 1, len + 1),
        _ => (head + 1, len.saturating_sub(1)),
    };
    let mut out = vec![head];
    out.resize(len + 1, ZERO_DIGIT);
    Some(out)
}

/// The integer one smaller, or `None` for the smallest.
fn dec(int: &[u8]) -> Option<Vec<u8>> {
    let (&head, digits) = int.split_first()?;
    let mut digits = digits.to_vec();
    for d in digits.iter_mut().rev() {
        if *d == ZERO_DIGIT {
            *d = TOP_DIGIT;
        } else {
            *d = digit(digit_value(*d) - 1);
            return Some([&[head][..], &digits].concat());
        }
    }
    // Every digit was zero: move to the previous head.
    let len = digits.len();
    let (head, len) = match head {
        b'A' => return None,
        b'a' => return Some(b"Zz".to_vec()),
        b'b'..=b'z' => (head - 1, len.saturating_sub(1)),
        _ => (head - 1, len + 1),
    };
    let mut out = vec![head];
    out.resize(len + 1, TOP_DIGIT);
    Some(out)
}

/// `62^n`, or `None` if it does not fit.
fn pow62(n: usize) -> Option<i128> {
    (0..n).try_fold(1_i128, |acc, _| acc.checked_mul(62))
}

/// How many integers have fewer than `len` digits on the non-negative side: `62 + 62² + … + 62^(len-1)`.
fn below(len: usize) -> Option<i128> {
    (1..len).try_fold(0_i128, |acc, j| acc.checked_add(pow62(j)?))
}

/// The numeric value of an integer of at most [`NUMERIC_DIGITS`] digits.
fn value(int: &[u8]) -> Option<i128> {
    let (&head, digits) = int.split_first()?;
    if digits.len() > NUMERIC_DIGITS {
        return None;
    }
    let d = digits.iter().try_fold(0_i128, |acc, &b| acc.checked_mul(62)?.checked_add(i128::from(digit_value(b))))?;
    if head.is_ascii_lowercase() { below(digits.len())?.checked_add(d) } else { d.checked_sub(below(digits.len() + 1)?) }
}

/// The integer with numeric value `v`, or `None` past [`NUMERIC_DIGITS`] digits.
fn from_value(v: i128) -> Option<Vec<u8>> {
    let len = (1..=NUMERIC_DIGITS).find(|&len| match below(len + 1) {
        Some(limit) if v >= 0 => v < limit,
        Some(limit) => v >= -limit,
        None => false,
    })?;
    let (head, mut d) = if v >= 0 {
        (b'a' + u8::try_from(len - 1).ok()?, v.checked_sub(below(len)?)?)
    } else {
        (b'Z' - u8::try_from(len - 1).ok()?, v.checked_add(below(len + 1)?)?)
    };
    let mut out = vec![ZERO_DIGIT; len + 1];
    *out.first_mut()? = head;
    for slot in out.iter_mut().skip(1).rev() {
        *slot = digit(u8::try_from(d.rem_euclid(62)).ok()?);
        d = d.div_euclid(62);
    }
    Some(out)
}

/// An integer [`STEP`] larger (one larger for huge integers), or `None` for the largest.
fn step_up(int: &[u8]) -> Option<Vec<u8>> {
    value(int).and_then(|v| from_value(v.checked_add(STEP)?)).or_else(|| inc(int))
}

/// An integer [`STEP`] smaller (one smaller for huge integers), or `None` for the smallest.
fn step_down(int: &[u8]) -> Option<Vec<u8>> {
    value(int).and_then(|v| from_value(v.checked_sub(STEP)?)).or_else(|| dec(int))
}

/// An integer strictly between `a < b`, or `None` when they are adjacent.
fn midpoint(a: &[u8], b: &[u8]) -> Option<Vec<u8>> {
    match (value(a), value(b)) {
        (Some(va), Some(vb)) => {
            let gap = vb.checked_sub(va)?;
            if gap < 2 { None } else { from_value(va.checked_add(gap / 2)?) }
        }
        _ => inc(a).filter(|next| next.as_slice() < b),
    }
}

/// A key after the integer sequence `k` (the empty sequence is before everything).
fn after(k: &[&[u8]]) -> Vec<u8> {
    match k.split_first() {
        None => ZERO.to_vec(),
        Some((first, rest)) => match step_up(first) {
            Some(next) => next,
            None => [*first, &after(rest)].concat(),
        },
    }
}

/// A key before the non-empty integer sequence `k`.
fn before(k: &[&[u8]]) -> Result<Vec<u8>, Error> {
    let Some((first, rest)) = k.split_first() else {
        return Err(Error::InvalidKey(String::new()));
    };
    match step_down(first) {
        // The smallest integer may not end a key: follow it with zero.
        Some(prev) if is_min(&prev) => Ok([&prev[..], ZERO].concat()),
        Some(prev) => Ok(prev),
        None if rest.is_empty() => Err(Error::InvalidKey(String::from_utf8_lossy(first).into_owned())),
        None => Ok([*first, &before(rest)?].concat()),
    }
}

/// A key strictly between the integer sequences `a < b`.
fn between(a: &[&[u8]], b: &[&[u8]]) -> Result<Vec<u8>, Error> {
    let not_ordered = || Error::KeysNotOrdered { a: String::new(), b: String::new() };
    match (a.split_first(), b.split_first()) {
        // `a` is a prefix of `b`: anything below `b`'s tail fits.
        (None, Some(_)) => before(b),
        (_, None) => Err(not_ordered()),
        (Some((ia, ra)), Some((ib, rb))) => match ia.cmp(ib) {
            Ordering::Equal => Ok([*ia, &between(ra, rb)?].concat()),
            Ordering::Greater => Err(not_ordered()),
            Ordering::Less => match midpoint(ia, ib) {
                Some(mid) => Ok(mid),
                // Adjacent integers: stay on `ia` and go past `a`'s tail.
                None => Ok([*ia, &after(ra)].concat()),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn min_int() -> Vec<u8> {
        let mut v = vec![b'A'];
        v.resize(27, ZERO_DIGIT);
        v
    }

    fn key(s: &str) -> PosKey {
        PosKey::parse(s).unwrap()
    }

    #[test]
    fn integers_step_across_heads() {
        assert_eq!(inc(b"a0").unwrap(), b"a1");
        assert_eq!(inc(b"az").unwrap(), b"b00");
        assert_eq!(inc(b"bzz").unwrap(), b"c000");
        assert_eq!(inc(b"Zz").unwrap(), b"a0");
        assert_eq!(inc(b"Yzz").unwrap(), b"Z0");
        assert_eq!(dec(b"a0").unwrap(), b"Zz");
        assert_eq!(dec(b"b00").unwrap(), b"az");
        assert_eq!(dec(b"Z0").unwrap(), b"Yzz");
        assert_eq!(dec(b"a9").unwrap(), b"a8");
        assert_eq!(dec(b"aA").unwrap(), b"a9");
        assert_eq!(inc(b"a9").unwrap(), b"aA");
        assert!(inc(&[b'z'; 27]).is_none());
        assert!(dec(&min_int()).is_none());
    }

    #[test]
    fn inc_and_dec_are_inverse_and_ordered() {
        let mut k = b"Xzzz".to_vec();
        for _ in 0..300_000 {
            let n = inc(&k).unwrap();
            assert!(n > k, "{:?} -> {:?}", String::from_utf8_lossy(&k), String::from_utf8_lossy(&n));
            assert_eq!(dec(&n).unwrap(), k);
            k = n;
        }
    }

    #[test]
    fn parse_refuses_malformed_keys() {
        let min = String::from_utf8(min_int()).unwrap();
        let long = format!("{}a0", "z".repeat(27).repeat(19)); // 515 bytes
        for bad in ["", "a", "0", "a-", "b0", "a0 ", "é", "a0b", &min, &format!("a0{min}"), &long] {
            assert!(PosKey::parse(bad).is_err(), "{bad:?}");
        }
        for good in ["a0", "Zz", "b00", "a0a0", &format!("{min}a0"), &"z".repeat(27)] {
            assert!(PosKey::parse(good).is_ok(), "{good:?}");
        }
        assert!(serde_json::from_str::<PosKey>("\"b3\"").is_err());
        assert_eq!(serde_json::from_str::<PosKey>("\"a3\"").unwrap(), key("a3"));
    }

    #[test]
    fn after_and_before_work_at_the_ends_of_the_range() {
        let max = key(&"z".repeat(27));
        let past = max.after().unwrap();
        assert!(past > max);
        assert_eq!(past.as_str(), format!("{}a0", "z".repeat(27)));
        let low = key(&format!("A{}1", "0".repeat(25)));
        let below = low.before().unwrap();
        assert!(below < low);
        assert_eq!(below.as_str(), format!("{}a0", String::from_utf8(min_int()).unwrap()));
        assert!(below.before().unwrap() < below);
    }

    #[test]
    fn between_refuses_equal_or_inverted_keys() {
        assert!(matches!(PosKey::between(&key("a1"), &key("a1")), Err(Error::KeysNotOrdered { .. })));
        assert!(matches!(PosKey::between(&key("a2"), &key("a1")), Err(Error::KeysNotOrdered { .. })));
    }

    #[test]
    fn values_map_both_ways() {
        for (int, v) in [(&b"a0"[..], 0), (b"az", 61), (b"b00", 62), (b"Zz", -1), (b"Z0", -62), (b"Yzz", -63)] {
            assert_eq!(value(int), Some(v), "{}", String::from_utf8_lossy(int));
            assert_eq!(from_value(v).as_deref(), Some(int));
        }
        let mut k = b"Wzzzz".to_vec();
        for _ in 0..100_000 {
            let n = inc(&k).unwrap();
            assert_eq!(value(&n), value(&k).map(|v| v + 1));
            assert_eq!(from_value(value(&n).unwrap()).unwrap(), n);
            k = n;
        }
        // Beyond 21 digits there is no numeric form, and stepping falls back to one.
        let huge = [b'v'; 23];
        assert_eq!(value(&huge), None);
        assert_eq!(step_up(&huge), inc(&huge));
    }

    #[test]
    fn between_prefers_short_keys() {
        assert_eq!(PosKey::between(&key("a1"), &key("a5")).unwrap().as_str(), "a3");
        assert_eq!(PosKey::between(&key("a1"), &key("a2")).unwrap().as_str(), "a1a0");
        assert_eq!(PosKey::between(&key("a1"), &key("a1a0")).unwrap().as_str(), "a1Y10");
        assert_eq!(PosKey::between(&key("a1a0"), &key("a2")).unwrap().as_str(), "a1bz0");
        assert_eq!(PosKey::first().after().unwrap().as_str(), "bz0");
        assert_eq!(PosKey::first().before().unwrap().as_str(), "Y10");
    }
}
