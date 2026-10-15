//! Helpers the `dalica-meta` tests share (the file builders are in `build/mod.rs`).

#![allow(dead_code)]

use dalica_core::DateTime;
use dalica_meta::{Changes, DateTaken, Limits};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's case count: `n` in full mode, at most 32 in quick mode.
pub fn cases(n: u32) -> u32 {
    if full() { n } else { n.min(32) }
}

pub fn l() -> Limits {
    Limits::default()
}

/// The time of the writes (`xmp:MetadataDate`).
pub fn now() -> DateTaken {
    DateTaken::parse_iso("2026-10-12T09:30:00+02:00").unwrap()
}

pub fn text(b: &[u8]) -> &str {
    std::str::from_utf8(b).unwrap()
}

/// A change of the caption alone.
pub fn caption(c: &str) -> Changes {
    Changes { caption: Some(Some(c.to_owned())), ..Changes::default() }
}

pub fn date(text: &str, offset: Option<i16>, subsec: Option<&str>) -> DateTaken {
    DateTaken::new(text.parse::<DateTime>().unwrap(), offset, subsec).unwrap()
}

/// The XMP packet of a file, as text.
pub fn xmp_text(file: &[u8]) -> String {
    String::from_utf8(dalica_meta::scan(file, &l()).unwrap().xmp.unwrap().into_owned()).unwrap()
}
