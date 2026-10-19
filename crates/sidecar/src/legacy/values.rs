//! The typed values: rectangles, dates, ids, faces, places and the text overlay.

use dalica_core::{Date, DateTime};

use super::read::{decode, text_blocks};
use super::{LegacyFace, LegacyPerson, Text, TextOverlay};
use crate::fields::Geo;

/// The contact id that marks an ignored face.
pub const IGNORED_CONTACT: &str = "ffffffffffffffff";

/// Days from 1899-12-30 (where the legacy day count starts) to 1970-01-01.
const DAY_COUNT_EPOCH: f64 = 25_569.0;
/// Day counts further than this from 1899-12-30 are refused before any conversion (year 9999 is
/// about 2.96 million days away).
const MAX_DAY_COUNT: f64 = 3_000_000.0;

/// A rectangle written as up to 16 hexadecimal digits (leading zeros dropped): four 16-bit values,
/// left, top, right and bottom, each over 65 535 of the stored picture's width or height. The
/// digits are padded on the left to 16, so `ffffffff` is the whole picture `[0, 0, 1, 1]`. `None`
/// for anything else, and for corners out of order.
pub fn decode_rect(hex: &str) -> Option<[f64; 4]> {
    let hex = hex.trim();
    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u64::from_str_radix(hex, 16).ok()?;
    let part = |shift: u32| f64::from(u16::try_from((v >> shift) & 0xFFFF).unwrap_or(0)) / 65_535.0;
    let r = [part(48), part(32), part(16), part(0)];
    (r[0] <= r[2] && r[1] <= r[3]).then_some(r)
}

/// A rectangle in its wrapped form `rect64(<hex>)` ([`decode_rect`]).
pub fn decode_rect64(s: &str) -> Option<[f64; 4]> {
    decode_rect(s.trim().strip_prefix("rect64(")?.strip_suffix(')')?)
}

/// A local date and time from a day count: days since 1899-12-30 00:00 as a decimal number, the
/// fraction the time of day (for days before that, the fraction still counts forward from the
/// start of its day). `None` when it is not a finite number or falls outside years 1–9999.
pub fn day_count_date(s: &str) -> Option<DateTime> {
    let days: f64 = s.trim().parse().ok()?;
    if !days.is_finite() || days.abs() > MAX_DAY_COUNT {
        return None;
    }
    let whole = days.trunc();
    let secs_of_day = ((days - whole).abs() * 86_400.0).round();
    // Both are bounded above, so the conversions are exact.
    let secs = (whole - DAY_COUNT_EPOCH) * 86_400.0 + secs_of_day;
    DateTime::from_unix_secs(secs as i64)
}

/// A date written as text: `YYYY-MM-DDTHH:MM:SS` with optional fractional seconds and an optional
/// zone (`Z`, `±HH:MM` or `±HHMM`; the wall-clock time is kept, the zone dropped), or a bare
/// `YYYY-MM-DD` (midnight).
pub fn text_date(s: &str) -> Option<DateTime> {
    let s = s.trim();
    if let Ok(date) = s.parse::<Date>() {
        return DateTime::new(date, 0, 0, 0).ok();
    }
    let dt: DateTime = s.get(..19)?.parse().ok()?;
    let mut rest = s.get(19..)?;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = frac.get(digits..)?;
    }
    let zone_ok = match rest.as_bytes() {
        [] | [b'Z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] | [b'+' | b'-', h1, h2, m1, m2] => [h1, h2, m1, m2].iter().all(|b| b.is_ascii_digit()),
        _ => false,
    };
    zone_ok.then_some(dt)
}

/// A date in either form ([`text_date`], then [`day_count_date`]).
pub fn any_date(s: &str) -> Option<DateTime> {
    text_date(s).or_else(|| day_count_date(s))
}

/// A contact id: 1 to 16 hexadecimal digits, padded on the left with zeros to 16, lowercase.
pub fn contact_id(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("{:0>16}", s.to_ascii_lowercase()))
}

/// An album token: exactly 32 hexadecimal digits, lowercase.
pub fn album_token(s: &str) -> Option<String> {
    let s = s.trim();
    (s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

/// The faces list: `rect64(<hex>),<contact id>` items separated by `;` (a trailing `;` allowed).
/// Returns the faces read and how many items could not be read.
pub fn faces(s: &str) -> (Vec<LegacyFace>, usize) {
    let mut out = Vec::new();
    let mut bad = 0usize;
    for item in s.split(';').map(str::trim).filter(|i| !i.is_empty()) {
        match face(item) {
            Some(f) => out.push(f),
            None => bad = bad.saturating_add(1),
        }
    }
    (out, bad)
}

fn face(item: &str) -> Option<LegacyFace> {
    let (rect, id) = item.split_once("),")?;
    let rect = decode_rect64(&format!("{rect})"))?;
    let id = contact_id(id)?;
    let person = if id == IGNORED_CONTACT { LegacyPerson::Ignored } else { LegacyPerson::Contact(id) };
    Some(LegacyFace { rect, person })
}

/// `<lat>,<lon>` in decimal degrees.
pub fn geotag(s: &str) -> Option<Geo> {
    let (lat, lon) = s.split_once(',')?;
    Geo::new(lat.trim().parse().ok()?, lon.trim().parse().ok()?)
}

/// `<name>(<n>)` with a small whole number `n`.
pub fn call_number(s: &str, name: &str) -> Option<u8> {
    s.trim().strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?.trim().parse().ok()
}

/// The parts of a list separated by `sep`, trimmed, the empty ones left out. A part is marked
/// lossy when the whole was and the part holds a replacement character.
pub fn list(whole: &Text, sep: char) -> Vec<Text> {
    whole.text.split(sep).map(str::trim).filter(|p| !p.is_empty()).map(|p| part(whole, p)).collect()
}

/// `p`, a piece of `whole`, as a [`Text`].
pub fn part(whole: &Text, p: &str) -> Text {
    Text { text: p.to_string(), lossy: whole.lossy && p.contains(char::REPLACEMENT_CHARACTER) }
}

/// The text overlay, kept whole, with the text of each block read through its length prefixes.
pub fn overlay(raw: &[u8], utf8: bool) -> TextOverlay {
    let texts =
        text_blocks(raw).map(|blocks| blocks.into_iter().filter_map(|b| block_text(b).map(|t| decode(t, utf8))).collect()).unwrap_or_default();
    TextOverlay { raw: decode(raw, utf8), texts }
}

/// A block starts with `<text length>;<text>;`.
fn block_text(block: &[u8]) -> Option<&[u8]> {
    let (len, start) = super::read::number(block, 0)?;
    let end = start.checked_add(len)?;
    let text = block.get(start..end)?;
    (block.get(end) == Some(&b';')).then_some(text)
}
