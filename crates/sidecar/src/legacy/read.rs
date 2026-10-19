//! The bounded line reader: sections and `key=value` lines over raw bytes, merged, then decoded.

use std::collections::HashMap;

use super::{LegacyError, LegacyLimits, Section, Text};

const BOM: &[u8] = b"\xEF\xBB\xBF";
/// At most this many blocks in a text overlay's record (each block takes at least four bytes).
const MAX_TEXT_BLOCKS: usize = 10_000;

/// One section while reading: its raw name and its keys in first-seen order.
struct RawSection<'a> {
    name: &'a [u8],
    keys: Vec<(&'a [u8], &'a [u8])>,
    index: HashMap<&'a [u8], usize>,
}

/// What the reader hands back: the sections in first-seen order and whether the file is UTF-8.
pub(super) struct Read {
    pub utf8: bool,
    pub sections: Vec<Section>,
}

/// Reads `bytes` under `limits`. Lines end in LF or CRLF; a line `[name]` opens (or reopens) a
/// section; `key=value` splits on the first `=`; anything else, keys before the first section and
/// keys under an empty section name are skipped. A repeated section adds to the first, a repeated
/// key replaces its value (the later wins).
pub(super) fn read(bytes: &[u8], limits: &LegacyLimits) -> Result<Read, LegacyError> {
    if bytes.len() > limits.max_bytes {
        return Err(LegacyError::FileTooLarge { size: bytes.len(), limit: limits.max_bytes });
    }
    let (data, bom) = match bytes.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    let mut sections: Vec<RawSection> = Vec::new();
    let mut by_name: HashMap<&[u8], usize> = HashMap::new();
    let mut current: Option<usize> = None;
    let mut pos = 0usize;
    let mut line_no = 0usize;
    while pos < data.len() {
        line_no = line_no.saturating_add(1);
        let rest = data.get(pos..).unwrap_or_default();
        let len = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        if len > limits.max_line {
            return Err(LegacyError::LineTooLong { line: line_no, limit: limits.max_line });
        }
        let raw_line = rest.get(..len).unwrap_or_default();
        let mut next = pos.saturating_add(len).saturating_add(1);
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let trimmed = line.trim_ascii_end();
        if let Some(name) = trimmed.strip_prefix(b"[").and_then(|l| l.strip_suffix(b"]")) {
            current = if name.is_empty() {
                None
            } else if let Some(&i) = by_name.get(name) {
                Some(i)
            } else {
                if sections.len() >= limits.max_sections {
                    return Err(LegacyError::TooManySections { limit: limits.max_sections });
                }
                by_name.insert(name, sections.len());
                sections.push(RawSection { name, keys: Vec::new(), index: HashMap::new() });
                Some(sections.len() - 1)
            };
        } else if let Some(eq) = line.iter().position(|&b| b == b'=') {
            let (key, value) = (line.get(..eq).unwrap_or_default(), line.get(eq + 1..).unwrap_or_default());
            let Some(section) = current.and_then(|i| sections.get_mut(i)) else {
                pos = next;
                continue;
            };
            if key.is_empty() {
                pos = next;
                continue;
            }
            let mut value = value;
            if key == b"text" {
                // The overlay's text may hold line breaks: its length prefixes say where it ends.
                let start = pos.saturating_add(eq).saturating_add(1);
                let tail = data.get(start..).unwrap_or_default();
                if let Some(n) = text_record_len(tail, limits.max_line)
                    && n > value.len()
                {
                    value = tail.get(..n).unwrap_or(value);
                    let after = start.saturating_add(n);
                    let skip = data.get(after..).unwrap_or_default();
                    let to_eol = skip.iter().position(|&b| b == b'\n').map_or(skip.len(), |p| p.saturating_add(1));
                    line_no = line_no.saturating_add(value.iter().filter(|&&b| b == b'\n').count());
                    next = after.saturating_add(to_eol);
                }
            }
            if let Some(&k) = section.index.get(key) {
                if let Some(slot) = section.keys.get_mut(k) {
                    slot.1 = value;
                }
            } else {
                if section.keys.len() >= limits.max_keys {
                    return Err(LegacyError::TooManyKeys { section: excerpt(&String::from_utf8_lossy(section.name)), limit: limits.max_keys });
                }
                section.index.insert(key, section.keys.len());
                section.keys.push((key, value));
            }
        }
        pos = next;
    }
    let utf8 = bom
        || by_name
            .get(&b"encoding"[..])
            .and_then(|&i| sections.get(i))
            .is_some_and(|s| s.index.get(&b"utf8"[..]).and_then(|&k| s.keys.get(k)).is_some_and(|(_, v)| v.trim_ascii() == b"1"));
    let sections = sections
        .into_iter()
        .map(|s| Section { name: decode(s.name, utf8), utf8, keys: s.keys.into_iter().map(|(k, v)| (decode(k, utf8).text, v.to_vec())).collect() })
        .collect();
    Ok(Read { utf8, sections })
}

/// The length of a text overlay's record at the start of `buf` (`<count>;` then, per block,
/// `<length>;<length bytes>;`), or `None` when `buf` does not start with one within `cap` bytes.
pub(super) fn text_record_len(buf: &[u8], cap: usize) -> Option<usize> {
    let (count, mut at) = number(buf, 0)?;
    if count > MAX_TEXT_BLOCKS {
        return None;
    }
    for _ in 0..count {
        let (len, start) = number(buf, at)?;
        let end = start.checked_add(len)?;
        if end >= cap || buf.get(end) != Some(&b';') {
            return None;
        }
        at = end + 1;
    }
    Some(at)
}

/// The blocks of a text overlay's record, each its raw bytes (without the terminating `;`).
pub(super) fn text_blocks(buf: &[u8]) -> Option<Vec<&[u8]>> {
    let (count, mut at) = number(buf, 0)?;
    if count > MAX_TEXT_BLOCKS {
        return None;
    }
    let mut blocks = Vec::new();
    for _ in 0..count {
        let (len, start) = number(buf, at)?;
        let end = start.checked_add(len)?;
        blocks.push(buf.get(start..end)?);
        if buf.get(end) != Some(&b';') {
            return None;
        }
        at = end + 1;
    }
    Some(blocks)
}

/// A run of 1–9 ASCII digits at `at` followed by `;`: its value and the position after the `;`.
pub(super) fn number(buf: &[u8], at: usize) -> Option<(usize, usize)> {
    let rest = buf.get(at..)?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 9 || rest.get(digits) != Some(&b';') {
        return None;
    }
    let n = rest.get(..digits)?.iter().fold(0usize, |n, &d| n * 10 + usize::from(d - b'0'));
    Some((n, at + digits + 1))
}

/// Decodes `bytes` as UTF-8 (`utf8`) or Windows-1252; bytes that do not decode become U+FFFD and
/// mark the text `lossy`.
pub(super) fn decode(bytes: &[u8], utf8: bool) -> Text {
    if utf8 {
        match std::str::from_utf8(bytes) {
            Ok(s) => Text { text: s.to_string(), lossy: false },
            Err(_) => Text { text: String::from_utf8_lossy(bytes).into_owned(), lossy: true },
        }
    } else {
        let mut lossy = false;
        let text = bytes
            .iter()
            .map(|&b| {
                let c = windows_1252(b);
                lossy |= c == char::REPLACEMENT_CHARACTER;
                c
            })
            .collect();
        Text { text, lossy }
    }
}

/// One Windows-1252 byte; the five bytes the code page leaves undefined become U+FFFD.
fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}',
        '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}', '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}',
        '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
    ];
    match b {
        0x80..=0x9F => HIGH.get(usize::from(b - 0x80)).copied().unwrap_or(char::REPLACEMENT_CHARACTER),
        _ => char::from(b),
    }
}

/// At most 64 characters of `s`, for messages.
fn excerpt(s: &str) -> String {
    s.chars().take(64).collect()
}
