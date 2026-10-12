//! Show All Metadata: every field of a file as label/value rows (`ui-layout.md` §7), bounded.
//!
//! Rows come in file-independent order: EXIF (IFD0 and the EXIF IFD, then interoperability, GPS
//! and the thumbnail IFD), IPTC, XMP (from M6.9), then text chunks and comments, then the blocks
//! that could not be read. At most [`Limits::max_rows`] rows, each value at most
//! [`Limits::max_row_value`] characters; values are tainted text, so control characters become
//! spaces and bidirectional controls are shown as `<U+…>` (`security.md` §4.4).

use dalica_image::ifd::{Entry, Tiff, types};

use crate::container::{self, Container, Scan};
use crate::exif::read::{Walked, entry_text, rationals, walk, word};
use crate::exif::{IfdKind, TAG_MAKER_NOTE, TAG_USER_COMMENT, tag_name};
use crate::iptc::read::{Charset, datasets};
use crate::iptc::{CODED_CHARACTER_SET, UTF8_ESCAPE, dataset_name};
use crate::{BlockKind, Limits, Result, xmp};

/// One row of Show All Metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// "EXIF", "GPS", "IPTC", "XMP", "PNG text" …
    pub group: String,
    pub label: String,
    pub value: String,
}

/// Every row, and whether some were left out to stay within [`Limits::max_rows`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub rows: Vec<Row>,
    pub truncated: bool,
}

/// How many values of a multi-valued entry are shown before "…".
const MAX_VALUES: usize = 16;

struct Builder {
    out: Listing,
    limits: Limits,
}

impl Builder {
    fn push(&mut self, group: &str, label: &str, value: &str) {
        if self.out.rows.len() >= self.limits.max_rows {
            self.out.truncated = true;
            return;
        }
        self.out.rows.push(Row { group: group.to_owned(), label: display(label, 200), value: display(value, self.limits.max_row_value) });
    }
}

/// Walks a file and lists every field it holds.
pub fn listing(bytes: &[u8], limits: &Limits) -> Result<Listing> {
    Ok(listing_scan(&container::scan(bytes, limits)?, limits))
}

/// [`listing`] for a file already walked.
pub fn listing_scan(scan: &Scan<'_>, limits: &Limits) -> Listing {
    let mut b = Builder { out: Listing::default(), limits: *limits };
    if let Some(block) = scan.exif.as_deref() {
        match walk(block, limits) {
            Ok(w) => exif_rows(&w, &mut b),
            Err(e) => b.push("EXIF", "Unreadable", &e.to_string()),
        }
    }
    match scan.iptc_stream(limits) {
        Ok(Some(stream)) => match datasets(stream, limits) {
            Ok(ds) => {
                let charset = Charset::of(&ds);
                for d in &ds {
                    let label = dataset_name(d.record, d.number).map_or_else(|| format!("Dataset {}:{}", d.record, d.number), str::to_owned);
                    let value = if (d.record, d.number) == CODED_CHARACTER_SET {
                        if d.data == UTF8_ESCAPE { "UTF-8".to_owned() } else { format!("({} bytes)", d.data.len()) }
                    } else if d.record == 2 && d.number == 0 && d.data.len() == 2 {
                        u16::from_be_bytes([d.data.first().copied().unwrap_or(0), d.data.get(1).copied().unwrap_or(0)]).to_string()
                    } else {
                        charset.decode(d.data)
                    };
                    b.push("IPTC", &label, &value);
                }
            }
            Err(e) => b.push("IPTC", "Unreadable", &e.to_string()),
        },
        Ok(None) => {}
        Err(e) => b.push("IPTC", "Unreadable", &e.to_string()),
    }
    if let Some(packet) = scan.xmp.as_deref() {
        match xmp::rows(packet, scan.extended_xmp.as_deref(), limits) {
            Ok(rows) => {
                for r in rows {
                    b.push(&r.group, &r.label, &r.value);
                }
            }
            Err(e) => b.push("XMP", "Unreadable", &e.to_string()),
        }
    }
    let text_group = if scan.container == Container::Png { "PNG text" } else { "Comment" };
    for t in &scan.texts {
        b.push(text_group, &t.keyword, &t.value);
    }
    for p in &scan.problems {
        let group = match p.block {
            BlockKind::ExtendedXmp | BlockKind::Xmp => "XMP",
            _ => text_group,
        };
        b.push(group, "Unreadable", &p.error.to_string());
    }
    b.out
}

fn exif_rows(w: &Walked<'_>, b: &mut Builder) {
    let order = [IfdKind::Image, IfdKind::Exif, IfdKind::Interop, IfdKind::Gps, IfdKind::Thumbnail];
    for kind in order {
        for (_, ifd) in w.ifds.iter().filter(|(k, _)| *k == kind) {
            for e in &ifd.entries {
                let label = tag_name(kind, e.tag).map_or_else(|| format!("Tag 0x{:04x}", e.tag), str::to_owned);
                let value = entry_value(&w.tiff, e, &b.limits);
                b.push(kind.group(), &label, &value);
            }
        }
    }
}

/// An entry's value as text: strings as they are, numbers and rationals listed (the first
/// [`MAX_VALUES`]), opaque data by its size.
fn entry_value(t: &Tiff<'_>, e: &Entry, limits: &Limits) -> String {
    let value = match t.value(e) {
        Ok(Some(v)) => v,
        Ok(None) => return format!("(type {}, {} values)", e.kind, e.count),
        Err(_) => return "(outside the block)".to_owned(),
    };
    let little = t.is_little_endian();
    let more = |shown: usize| if (e.count as usize) > shown { " …" } else { "" };
    match e.kind {
        types::ASCII => entry_text(t, e, limits.max_string).ok().flatten().unwrap_or_default(),
        types::UNDEFINED if e.tag == TAG_MAKER_NOTE => format!("({} bytes)", value.len()),
        types::UNDEFINED if e.tag == TAG_USER_COMMENT => user_comment(&value, little),
        types::UNDEFINED => {
            let printable = value.len() <= 64 && !value.is_empty() && value.iter().all(|&c| c.is_ascii_graphic() || c == b' ');
            if printable { String::from_utf8_lossy(&value).into_owned() } else { format!("({} bytes)", value.len()) }
        }
        types::BYTE | types::SBYTE => {
            let v: Vec<String> =
                value.iter().take(MAX_VALUES).map(|&x| if e.kind == types::SBYTE { (x as i8).to_string() } else { x.to_string() }).collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::SHORT | types::SSHORT => {
            let v: Vec<String> = value
                .as_chunks::<2>()
                .0
                .iter()
                .take(MAX_VALUES)
                .map(|&[a, c]| {
                    let u = if little { u16::from_le_bytes([a, c]) } else { u16::from_be_bytes([a, c]) };
                    if e.kind == types::SSHORT { (u as i16).to_string() } else { u.to_string() }
                })
                .collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::LONG | types::SLONG | types::IFD => {
            let v: Vec<String> = value
                .as_chunks::<4>()
                .0
                .iter()
                .take(MAX_VALUES)
                .map(|&w| {
                    let u = word(w, little);
                    if e.kind == types::SLONG { (u as i32).to_string() } else { u.to_string() }
                })
                .collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::RATIONAL => {
            let v: Vec<String> =
                rationals(t, e, MAX_VALUES).unwrap_or_default().iter().map(|r| r.map_or_else(|| "invalid".to_owned(), |r| r.to_string())).collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::SRATIONAL => {
            let v: Vec<String> = value
                .as_chunks::<8>()
                .0
                .iter()
                .take(MAX_VALUES)
                .map(|&[a, b, c, d, e2, f, g, h]| {
                    let (n, dd) = (word([a, b, c, d], little) as i32, word([e2, f, g, h], little) as i32);
                    if dd == 0 { "invalid".to_owned() } else { format!("{n}/{dd}") }
                })
                .collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::FLOAT => {
            let v: Vec<String> = value.as_chunks::<4>().0.iter().take(MAX_VALUES).map(|&w| f32::from_bits(word(w, little)).to_string()).collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        types::DOUBLE => {
            let v: Vec<String> = value
                .as_chunks::<8>()
                .0
                .iter()
                .take(MAX_VALUES)
                .map(|&[a, b, c, d, e2, f, g, h]| {
                    let bits = if little { u64::from_le_bytes([a, b, c, d, e2, f, g, h]) } else { u64::from_be_bytes([a, b, c, d, e2, f, g, h]) };
                    f64::from_bits(bits).to_string()
                })
                .collect();
            format!("{}{}", v.join(" "), more(v.len()))
        }
        _ => format!("({} bytes)", value.len()),
    }
}

/// A UserComment: an 8-byte character code, then the text (Exif 2.32 §4.6.5).
fn user_comment(value: &[u8], little: bool) -> String {
    let (code, text) = (value.get(..8).unwrap_or_default(), value.get(8..).unwrap_or_default());
    let text = match code {
        b"ASCII\0\0\0" | b"\0\0\0\0\0\0\0\0" => String::from_utf8_lossy(text).into_owned(),
        b"UNICODE\0" => {
            let units: Vec<u16> =
                text.as_chunks::<2>().0.iter().map(|&[a, b]| if little { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) }).collect();
            String::from_utf16_lossy(&units)
        }
        _ => return format!("({} bytes)", value.len()),
    };
    text.trim_end_matches(['\0', ' ']).to_owned()
}

/// Tainted text for display: control characters as spaces, bidirectional controls visible,
/// at most `max` characters.
fn display(text: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        match c {
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}' => {
                out.push_str(&format!("<U+{:04X}>", u32::from(c)))
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_makes_text_safe() {
        assert_eq!(display("a\nb\u{202e}c", 100), "a b<U+202E>c");
        assert_eq!(display("abcdef", 3), "abc…");
    }

    #[test]
    fn user_comments() {
        assert_eq!(user_comment(b"ASCII\0\0\0hello\0\0", true), "hello");
        assert_eq!(user_comment(b"UNICODE\0h\0i\0", true), "hi");
        assert_eq!(user_comment(b"UNICODE\0\0h\0i", false), "hi");
        assert_eq!(user_comment(b"JIS\0\0\0\0\0xx", true), "(10 bytes)");
    }
}
