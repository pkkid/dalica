//! Collision names (`data-model.md` §6, `security.md` §5.4): when `photo.jpg` is taken, the next
//! candidates are `photo-001.jpg`, `photo-002.jpg`, … `photo-999.jpg`, `photo-1000.jpg`, ….
//!
//! The number goes before the extension: the text after the last dot, unless that dot is the first
//! or last character (`.hidden` and `notes.` have no extension; `archive.tar.gz` becomes
//! `archive.tar-001.gz`). An empty stem gets the bare number (`001`), never a leading `-`.
//! Candidates stay within [`MAX_NAME_BYTES`] by shortening the stem on a character boundary.
//!
//! Callers that create files try [`candidates`] in order with exclusive creation (never "check,
//! then create"); [`next_free`] is for in-memory planning only.
//!
//! Every name Dalica makes up for a new file or folder (an export folder named after an album, an
//! exported photo's stem, later import folders and collage files) first goes through
//! [`sanitize`], the one sanitizer `security.md` §5.4 asks for.

use crate::dates::Date;
use crate::error::{Error, excerpt};

/// The longest file name, in bytes, on the file systems Dalica writes to.
pub const MAX_NAME_BYTES: usize = 255;

/// The highest number tried before giving up.
pub const MAX_NUMBER: u32 = 99_999;

/// Splits `name` into stem and extension (without the dot), as described in the module docs.
pub fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => (name.get(..dot).unwrap_or(name), name.get(dot + 1..)),
        _ => (name, None),
    }
}

/// `name` with the collision number `n` (1 → `-001`) before its extension.
pub fn numbered(name: &str, n: u32) -> String {
    let (stem, ext) = split_ext(name);
    let number = if stem.is_empty() { format!("{n:03}") } else { format!("-{n:03}") };
    let ext_len = ext.map_or(0, |e| e.len() + 1);
    // A very long extension is treated as part of the stem, so the number always fits.
    let (stem, ext) = if ext_len + number.len() >= MAX_NAME_BYTES { (name, None) } else { (stem, ext) };
    let ext_len = ext.map_or(0, |e| e.len() + 1);
    let room = MAX_NAME_BYTES.saturating_sub(number.len() + ext_len);
    let stem = fit(stem, room);
    match ext {
        Some(ext) => format!("{stem}{number}.{ext}"),
        None => format!("{stem}{number}"),
    }
}

/// `name` itself, then `name-001`, `name-002`, … up to [`MAX_NUMBER`].
pub fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(name.to_string()).chain((1..=MAX_NUMBER).map(move |n| numbered(name, n)))
}

/// The first candidate for which `taken` is false.
pub fn next_free(name: &str, mut taken: impl FnMut(&str) -> bool) -> Result<String, Error> {
    candidates(name).find(|c| !taken(c)).ok_or_else(|| Error::NamesExhausted(excerpt(name)))
}

/// The name [`sanitize`] gives when neither the name nor the fallback leaves anything.
pub const FALLBACK: &str = "Untitled";

/// Characters a generated name never holds besides control and bidirectional ones: the path
/// separators and the characters Windows refuses in a name.
pub const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Windows device names, refused as a name's stem (the text before its first dot) in any case.
const RESERVED: [&str; 30] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Whether `c` sets or changes the direction of text (marks, embeddings, overrides, isolates):
/// such a character can make `photo\u{202E}gpj.exe` look like `photoexe.jpg`.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Whether `name`'s stem (the text before its first dot, trailing spaces ignored) is a Windows
/// device name such as `CON`, `nul.txt` or `COM1`.
pub fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// `name` made fit for a new file or folder (`security.md` §5.4):
///
/// 1. path separators, `: * ? " < > |`, control characters and bidirectional controls removed;
/// 2. leading dots, dashes and white space and trailing dots and white space stripped (no hidden
///    file, nothing read as an option, nothing Windows would silently drop);
/// 3. cut to [`MAX_NAME_BYTES`] on a character boundary (then trailing dots and spaces stripped
///    again);
/// 4. a Windows device name (`CON`, `aux.jpg`, `COM1`) gets a leading `_` (`_CON`).
///
/// When nothing is left, `fallback` goes through the same steps (callers pass a name such as
/// [`dated`]`("Export", today)`), and when that leaves nothing too, [`FALLBACK`]. Never empty, never
/// a path, and running it again changes nothing.
pub fn sanitize(name: &str, fallback: &str) -> String {
    clean(name).or_else(|| clean(fallback)).unwrap_or_else(|| FALLBACK.to_string())
}

fn clean(name: &str) -> Option<String> {
    let kept: String = name.chars().filter(|&c| !FORBIDDEN.contains(&c) && !c.is_control() && !is_bidi_control(c)).collect();
    let trimmed = trim_end(fit(trim_start(&kept), MAX_NAME_BYTES));
    if trimmed.is_empty() {
        return None;
    }
    if !is_reserved(trimmed) {
        return Some(trimmed.to_string());
    }
    let prefixed = format!("_{trimmed}");
    Some(trim_end(fit(&prefixed, MAX_NAME_BYTES)).to_string())
}

fn trim_start(s: &str) -> &str {
    s.trim_start_matches(|c: char| c == '.' || c == '-' || c.is_whitespace())
}

fn trim_end(s: &str) -> &str {
    s.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
}

/// `"<word> <date>"`, such as `Export 2026-06-04`: the fallback name of an export of a mixed tray.
pub fn dated(word: &str, date: Date) -> String {
    format!("{word} {date}")
}

/// The longest prefix of `s` of at most `max` bytes that ends on a character boundary.
pub fn fit(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn split_ext_follows_the_last_dot() {
        assert_eq!(split_ext("photo.jpg"), ("photo", Some("jpg")));
        assert_eq!(split_ext("archive.tar.gz"), ("archive.tar", Some("gz")));
        assert_eq!(split_ext("README"), ("README", None));
        assert_eq!(split_ext(".hidden"), (".hidden", None));
        assert_eq!(split_ext(".hidden.jpg"), (".hidden", Some("jpg")));
        assert_eq!(split_ext("notes."), ("notes.", None));
        assert_eq!(split_ext(""), ("", None));
    }

    #[test]
    fn numbered_puts_the_number_before_the_extension() {
        assert_eq!(numbered("photo.jpg", 1), "photo-001.jpg");
        assert_eq!(numbered("photo.jpg", 42), "photo-042.jpg");
        assert_eq!(numbered("photo.jpg", 1000), "photo-1000.jpg");
        assert_eq!(numbered("photo-001.jpg", 1), "photo-001-001.jpg");
        assert_eq!(numbered("Export 2026-06-04", 2), "Export 2026-06-04-002");
    }

    #[test]
    fn numbered_handles_empty_and_dot_only_names() {
        assert_eq!(numbered("", 1), "001");
        assert_eq!(numbered(".", 1), ".-001");
        assert_eq!(numbered("..", 1), "..-001");
        assert_eq!(numbered("...", 3), "...-003");
        assert_eq!(numbered(".jpg", 1), ".jpg-001");
        assert_eq!(numbered("a.", 1), "a.-001");
    }

    #[test]
    fn numbered_keeps_non_ascii_intact() {
        assert_eq!(numbered("Lisboa à noite.jpg", 1), "Lisboa à noite-001.jpg");
        assert_eq!(numbered("東京.png", 7), "東京-007.png");
        assert_eq!(numbered("naïve.jpég", 1), "naïve-001.jpég");
        assert_eq!(numbered("🌊", 1), "🌊-001");
    }

    #[test]
    fn numbered_stays_within_the_name_limit() {
        let long = format!("{}.jpg", "é".repeat(200)); // 404 bytes
        let n = numbered(&long, 1);
        assert!(n.len() <= MAX_NAME_BYTES, "{}", n.len());
        assert!(n.ends_with("-001.jpg"));
        let long_ext = format!("a.{}", "x".repeat(300));
        let n = numbered(&long_ext, 12345);
        assert!(n.len() <= MAX_NAME_BYTES && n.ends_with("-12345"));
    }

    #[test]
    fn next_free_skips_taken_names() {
        let taken: HashSet<&str> = ["photo.jpg", "photo-001.jpg", "photo-002.jpg"].into();
        assert_eq!(next_free("photo.jpg", |c| taken.contains(c)).unwrap(), "photo-003.jpg");
        assert_eq!(next_free("other.jpg", |c| taken.contains(c)).unwrap(), "other.jpg");
        assert!(matches!(next_free("x", |_| true), Err(Error::NamesExhausted(_))));
    }

    #[test]
    fn candidates_are_distinct_and_in_order() {
        let c: Vec<String> = candidates("p.jpg").take(1002).collect();
        assert_eq!(c[0], "p.jpg");
        assert_eq!(c[1], "p-001.jpg");
        assert_eq!(c[1001], "p-1001.jpg");
        assert_eq!(c.iter().collect::<HashSet<_>>().len(), c.len());
        assert_eq!(candidates("p").count(), MAX_NUMBER as usize + 1);
    }
}
