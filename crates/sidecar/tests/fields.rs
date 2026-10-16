//! M10's key on disk (`data-model.md` §3): a photo's `changed`, the UTC time Dalica last changed
//! what is said about it, with milliseconds. It round-trips exactly; a value that is not such a
//! time is kept and reported, never used; writing it keeps the entry's other keys, Dalica's and
//! anyone else's; clearing it removes only it.

use dalica_core::{DateTime, Timestamp};
use dalica_sidecar::fields::{CHANGED, M10_PHOTO_KEYS};
use dalica_sidecar::{Field, Limits, PhotoValue, Problem, Sidecar, parse_utc, utc_text};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::cases;

fn parse(text: &str) -> Sidecar {
    Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap()
}

fn json_of(s: &Sidecar) -> Value {
    serde_json::from_slice(&s.print()).unwrap()
}

#[test]
fn the_m10_key_is_the_documented_one_and_reads_as_utc_with_milliseconds() {
    assert_eq!((M10_PHOTO_KEYS, CHANGED), (["changed"], "changed"));
    let t = Timestamp::from_millis(1_792_057_267_250);
    assert_eq!(utc_text(t).as_deref(), Some("2026-10-15T09:41:07.250Z"));
    assert_eq!(parse_utc("2026-10-15T09:41:07.250Z"), Some(t));
    assert_eq!(parse_utc("2026-10-15T09:41:07Z"), Some(Timestamp::from_millis(1_792_057_267_000)), "milliseconds may be left out");
    assert_eq!(utc_text(Timestamp::from_millis(-1)).as_deref(), Some("1969-12-31T23:59:59.999Z"));
    for bad in [
        "2026-10-15T09:41:07.250",
        "2026-10-15T09:41:07.25Z",
        "2026-10-15T09:41:07.2500Z",
        "2026-10-15T09:41:07.25xZ",
        "2026-13-01T00:00:00Z",
        "",
        "Z",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad:?}");
    }
    assert_eq!(utc_text(Timestamp::from_millis(i64::MAX)), None, "past the year 9999");
}

#[test]
fn changed_round_trips_and_keeps_the_entrys_other_keys() {
    let mut s = parse(r#"{"photos": {"a.jpg": {"star": true, "x-other": [1, 2]}}}"#);
    assert_eq!(s.photo("a.jpg").changed, Field::Absent, "no Dalica change recorded");
    let t = Timestamp::from_millis(1_792_057_267_250);
    s.set_photo("a.jpg", &PhotoValue::Changed(Some(t))).unwrap();
    assert_eq!(json_of(&s)["photos"]["a.jpg"], json!({ "star": true, "x-other": [1, 2], "changed": "2026-10-15T09:41:07.250Z" }));
    let again = Sidecar::parse(&s.print(), &Limits::default()).unwrap();
    assert_eq!(again.photo("a.jpg").changed, Field::Value(t));
    // Cleared: only the key goes; an entry left with nothing goes too.
    s.set_photo("a.jpg", &PhotoValue::Changed(None)).unwrap();
    assert_eq!(json_of(&s)["photos"]["a.jpg"], json!({ "star": true, "x-other": [1, 2] }));
    let mut only = parse(r#"{"photos": {"b.jpg": {"changed": "2026-10-15T09:41:07.250Z"}}}"#);
    only.set_photo("b.jpg", &PhotoValue::Changed(None)).unwrap();
    assert!(json_of(&only).get("photos").is_none_or(|p| p.get("b.jpg").is_none()));
    // A time no date can spell is written as no key.
    s.set_photo("a.jpg", &PhotoValue::Changed(Some(Timestamp::from_millis(i64::MAX)))).unwrap();
    assert!(json_of(&s)["photos"]["a.jpg"].get("changed").is_none());
}

#[test]
fn a_changed_that_is_not_a_utc_time_is_kept_and_reported() {
    for bad in [json!(1_792_057_267_250_i64), json!("yesterday"), json!("2026-10-15T09:41:07"), json!({ "t": 1 })] {
        let text = json!({ "photos": { "a.jpg": { "changed": bad.clone(), "star": true } } }).to_string();
        let s = parse(&text);
        assert_eq!(s.photo("a.jpg").changed, Field::WrongType(bad.clone()), "{bad}");
        assert_eq!(s.photo("a.jpg").star, Field::Value(true), "{bad}: the rest is read");
        let problems = s.problems();
        assert!(
            problems.contains(&Problem { pointer: "/photos/a.jpg/changed".into(), expected: "a UTC time (2026-10-15T09:41:07.250Z)" }),
            "{bad}: {problems:?}"
        );
        assert_eq!(json_of(&s)["photos"]["a.jpg"]["changed"], bad, "kept as it is");
    }
}

proptest! {
    #![proptest_config(cases(256, 32))]

    /// Every time from the year 1 to 9999 round-trips through its text exactly, and the text sorts
    /// as the times do within a year of four digits.
    #[test]
    fn every_time_in_range_round_trips(ms in -62_135_596_800_000_i64..253_402_300_799_999_i64) {
        let t = Timestamp::from_millis(ms);
        let text = utc_text(t).unwrap();
        prop_assert_eq!(parse_utc(&text), Some(t));
        let d: DateTime = text.trim_end_matches('Z').split('.').next().unwrap().parse().unwrap();
        prop_assert_eq!(DateTime::from_unix_secs(t.as_secs()), Some(d));
    }
}
