//! Numbers, file sizes and dates in the language in effect (`localization.md` → Writing
//! translatable code): never a hand-written `format!` pattern at a call site.
//!
//! The digits' separators come from the language's registry row; the words and the order of a
//! date or a size come from the catalog (`tl!(ctx = "date", "{month} {day}, {year}")`), so a
//! language may reorder them, and under the pseudo-locale they are bracketed like any text.

use dalica_core::Date;

use super::current;
use crate::tl;

/// `n` with the language's thousands separator: `12,345`.
pub fn number(n: u64) -> String {
    group(&n.to_string(), current().thousands)
}

/// `x` rounded to `places` decimals (at most 6) with the language's decimal mark: `3.5`. Not a
/// number reads as `0`.
pub fn decimal(x: f64, places: usize) -> String {
    let lang = current();
    let x = if x.is_finite() { x } else { 0.0 };
    let text = format!("{:.*}", places.min(6), x.abs());
    let (whole, frac) = text.split_once('.').unwrap_or((text.as_str(), ""));
    let mut out = String::new();
    if x < 0.0 && text.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
        out.push('-');
    }
    out.push_str(&group(whole, lang.thousands));
    if !frac.is_empty() {
        out.push_str(lang.decimal);
        out.push_str(frac);
    }
    out
}

/// Digits `digits` grouped by three with `sep`.
fn group(digits: &str, sep: &str) -> String {
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3 * sep.len());
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push_str(sep);
        }
        out.push(c);
    }
    out
}

/// A file size in decimal units, one decimal from a kilobyte on (`3.1 MB`), whole bytes below
/// (`512 bytes`, `1 byte`), as the status line writes them.
pub fn file_size(bytes: u64) -> String {
    const UNITS: [(f64, usize); 4] = [(1e12, 3), (1e9, 2), (1e6, 1), (1e3, 0)];
    let b = bytes as f64;
    match UNITS.iter().find(|(scale, _)| b >= *scale) {
        Some(&(scale, unit)) => {
            let size = decimal(b / scale, 1);
            match unit {
                3 => tl!("{size} TB", size = size),
                2 => tl!("{size} GB", size = size),
                1 => tl!("{size} MB", size = size),
                _ => tl!("{size} KB", size = size),
            }
        }
        None => crate::tln!(bytes, "{n} byte", "{n} bytes"),
    }
}

/// The name of month `month` (1–12): `June`; `?` outside.
pub fn month_name(month: u8) -> String {
    // One literal per name, so `cargo xtask i18n` finds every key.
    match month {
        1 => tl!(ctx = "month", "January"),
        2 => tl!(ctx = "month", "February"),
        3 => tl!(ctx = "month", "March"),
        4 => tl!(ctx = "month", "April"),
        5 => tl!(ctx = "month", "May"),
        6 => tl!(ctx = "month", "June"),
        7 => tl!(ctx = "month", "July"),
        8 => tl!(ctx = "month", "August"),
        9 => tl!(ctx = "month", "September"),
        10 => tl!(ctx = "month", "October"),
        11 => tl!(ctx = "month", "November"),
        12 => tl!(ctx = "month", "December"),
        _ => "?".into(),
    }
}

/// The name of weekday `day` (0 Sunday … 6 Saturday): `Thursday`; `?` outside.
pub fn weekday_name(day: u8) -> String {
    match day {
        0 => tl!(ctx = "weekday", "Sunday"),
        1 => tl!(ctx = "weekday", "Monday"),
        2 => tl!(ctx = "weekday", "Tuesday"),
        3 => tl!(ctx = "weekday", "Wednesday"),
        4 => tl!(ctx = "weekday", "Thursday"),
        5 => tl!(ctx = "weekday", "Friday"),
        6 => tl!(ctx = "weekday", "Saturday"),
        _ => "?".into(),
    }
}

/// The weekday of `date`, 0 Sunday … 6 Saturday (Sakamoto's method).
pub fn weekday(date: Date) -> u8 {
    const T: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let m = usize::from(date.month().clamp(1, 12));
    let mut y = u32::from(date.year());
    if m < 3 {
        y = y.saturating_sub(1);
    }
    let t = T.get(m - 1).copied().unwrap_or(0);
    let d = y + y / 4 - y / 100 + y / 400 + t + u32::from(date.day());
    u8::try_from(d % 7).unwrap_or(0)
}

/// A date written out: `June 4, 2026`.
pub fn long_date(date: Date) -> String {
    tl!(ctx = "date", "{month} {day}, {year}", month = month_name(date.month()), day = date.day(), year = date.year())
}

/// A date with its weekday: `Thursday, June 4, 2026`.
pub fn long_date_with_weekday(date: Date) -> String {
    tl!(ctx = "date", "{weekday}, {date}", weekday = weekday_name(weekday(date)), date = long_date(date))
}
