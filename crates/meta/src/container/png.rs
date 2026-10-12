//! PNG: the chunks up to `IEND` (PNG 3rd edition §5). `eXIf`, XMP in `iTXt` (or `tEXt`/`zTXt`)
//! under the keyword `XML:com.adobe.xmp`, the "Raw profile type" text chunks some tools use for
//! EXIF, IPTC and XMP, `iCCP`, and every other text chunk for the listing. Compressed text is
//! inflated under [`Limits::max_inflate`]; a text chunk that cannot be read is a problem, not a
//! failure of the walk.

use std::borrow::Cow;

use super::{BlockKind, Container, Scan, Text, be_u32, latin1, strip_exif_prefix, take};
use crate::{Error, Limits, Problem, Result};

/// The keyword of an XMP text chunk (XMP Part 3, 1.1.5).
pub const XMP_KEYWORD: &str = "XML:com.adobe.xmp";

const SIGNATURE_LEN: usize = 8;

/// A text chunk's keyword and text (decompressed, still bytes: Latin-1 in `tEXt`/`zTXt`, UTF-8 in
/// `iTXt`).
struct TextChunk<'a> {
    keyword: String,
    text: Cow<'a, [u8]>,
    utf8: bool,
}

pub(crate) fn scan<'a>(b: &'a [u8], limits: &Limits) -> Result<Scan<'a>> {
    let mut out = Scan::new(Container::Png);
    let mut at = SIGNATURE_LEN;
    let mut chunks = 0usize;
    loop {
        chunks += 1;
        if chunks > limits.max_segments {
            return Err(Error::TooMany("PNG chunks"));
        }
        let len = be_u32(b, at)?;
        if len > 0x7fff_ffff {
            return Err(Error::Malformed("a PNG chunk length past 2³¹ − 1".into()));
        }
        let len = len as usize;
        let kind = take(b, at + 4, 4)?;
        let data = take(b, at + 8, len)?;
        take(b, at + 8 + len, 4)?; // the CRC
        let range = at..at + 12 + len;
        let next = range.end;
        match kind {
            b"IEND" => break,
            b"eXIf" => {
                let first = out.exif.is_none();
                if first {
                    out.exif = Some(Cow::Borrowed(strip_exif_prefix(data)));
                }
                out.span(BlockKind::Exif, range, first);
            }
            b"iCCP" => out.span(BlockKind::Icc, range, false),
            b"tEXt" | b"zTXt" | b"iTXt" => match text_chunk(kind, data, limits) {
                Ok(t) => text(&mut out, t, range, limits),
                Err(error) => {
                    out.problems.push(Problem { block: BlockKind::Text, error });
                    out.span(BlockKind::Text, range, false);
                }
            },
            _ => {}
        }
        at = next;
    }
    Ok(out)
}

/// Files one text chunk under what it holds.
fn text<'a>(out: &mut Scan<'a>, t: TextChunk<'a>, range: std::ops::Range<usize>, limits: &Limits) {
    match t.keyword.as_str() {
        XMP_KEYWORD => {
            let first = out.xmp.is_none();
            if first {
                out.xmp = Some(t.text);
            }
            out.span(BlockKind::Xmp, range, first);
        }
        k if k.starts_with("Raw profile type ") => {
            let name = k.trim_start_matches("Raw profile type ").to_ascii_lowercase();
            let data = match raw_profile(&t.text, limits) {
                Ok(d) => d,
                Err(error) => {
                    out.problems.push(Problem { block: BlockKind::Text, error });
                    out.span(BlockKind::Text, range, false);
                    return;
                }
            };
            match name.as_str() {
                "exif" | "app1" if out.exif.is_none() => {
                    out.exif = Some(Cow::Owned(strip_exif_prefix(&data).to_vec()));
                    out.span(BlockKind::Exif, range, true);
                }
                "iptc" if out.iptc.is_none() && out.photoshop.is_none() => {
                    let photoshop = data.starts_with(b"8BIM");
                    if photoshop {
                        out.photoshop = Some(Cow::Owned(data));
                    } else {
                        out.iptc = Some(Cow::Owned(data));
                    }
                    out.span(if photoshop { BlockKind::Photoshop } else { BlockKind::Iptc }, range, true);
                }
                "8bim" if out.photoshop.is_none() => {
                    out.photoshop = Some(Cow::Owned(data));
                    out.span(BlockKind::Photoshop, range, true);
                }
                "xmp" if out.xmp.is_none() => {
                    out.xmp = Some(Cow::Owned(data));
                    out.span(BlockKind::Xmp, range, true);
                }
                "icc" | "icm" => out.span(BlockKind::Icc, range, false),
                _ => out.span(BlockKind::Text, range, false),
            }
        }
        _ => {
            let shown = t.text.get(..t.text.len().min(limits.max_string)).unwrap_or_default();
            let value = if t.utf8 { String::from_utf8_lossy(shown).into_owned() } else { latin1(shown) };
            out.texts.push(Text { keyword: t.keyword, value });
            out.span(BlockKind::Text, range, false);
        }
    }
}

/// The keyword and text of a `tEXt`, `zTXt` or `iTXt` chunk.
fn text_chunk<'a>(kind: &[u8], data: &'a [u8], limits: &Limits) -> Result<TextChunk<'a>> {
    let nul = data.iter().position(|&c| c == 0).ok_or_else(|| Error::Malformed("a PNG text chunk without a keyword".into()))?;
    if !(1..=79).contains(&nul) {
        return Err(Error::Malformed("a PNG text keyword of the wrong length".into()));
    }
    let keyword = latin1(data.get(..nul).unwrap_or_default());
    let rest = data.get(nul + 1..).unwrap_or_default();
    match kind {
        b"tEXt" => Ok(TextChunk { keyword, text: Cow::Borrowed(rest), utf8: false }),
        b"zTXt" => {
            let (&method, compressed) = rest.split_first().ok_or(Error::Truncated)?;
            if method != 0 {
                return Err(Error::Malformed("an unknown PNG compression method".into()));
            }
            Ok(TextChunk { keyword, text: Cow::Owned(inflate(compressed, limits.max_inflate)?), utf8: false })
        }
        _ => {
            let (&compressed, rest) = rest.split_first().ok_or(Error::Truncated)?;
            let (&method, rest) = rest.split_first().ok_or(Error::Truncated)?;
            let lang_end = rest.iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
            let rest = rest.get(lang_end + 1..).unwrap_or_default();
            let translated_end = rest.iter().position(|&c| c == 0).ok_or(Error::Truncated)?;
            let text = rest.get(translated_end + 1..).unwrap_or_default();
            let text = match (compressed, method) {
                (0, _) => Cow::Borrowed(text),
                (1, 0) => Cow::Owned(inflate(text, limits.max_inflate)?),
                _ => return Err(Error::Malformed("an unknown PNG compression method".into())),
            };
            Ok(TextChunk { keyword, text, utf8: true })
        }
    }
}

/// A zlib stream inflated to at most `max` bytes.
pub(crate) fn inflate(data: &[u8], max: usize) -> Result<Vec<u8>> {
    fdeflate::decompress_to_vec_bounded(data, max).map_err(|e| match e {
        fdeflate::BoundedDecompressionError::OutputTooLarge { .. } => Error::TooMany("inflated text bytes"),
        fdeflate::BoundedDecompressionError::DecompressionError { .. } => Error::Malformed("bad zlib data in a PNG text chunk".into()),
    })
}

/// The bytes of a "Raw profile type" text: a line with the profile's name, a decimal length, then
/// that many bytes as hex digits with white space between them.
pub(crate) fn raw_profile(text: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    let bad = || Error::Malformed("a raw profile text that is not name, length and hex".into());
    let mut i = text.iter().position(|c| !c.is_ascii_whitespace()).ok_or_else(bad)?;
    // The name line.
    i += text.get(i..).unwrap_or_default().iter().position(|&c| c == b'\n').ok_or_else(bad)?;
    let rest = text.get(i..).unwrap_or_default();
    let start = rest.iter().position(|c| !c.is_ascii_whitespace()).ok_or_else(bad)?;
    let digits = rest.get(start..).unwrap_or_default().iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 10 {
        return Err(bad());
    }
    let len: usize = std::str::from_utf8(rest.get(start..start + digits).unwrap_or_default()).ok().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    if len > limits.max_inflate {
        return Err(Error::TooMany("raw profile bytes"));
    }
    let hex = rest.get(start + digits..).unwrap_or_default();
    let mut out = Vec::with_capacity(len.min(hex.len() / 2));
    let mut high: Option<u8> = None;
    for &c in hex {
        if out.len() == len {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err(bad()),
        };
        match high.take() {
            Some(h) => out.push(h << 4 | v),
            None => high = Some(v),
        }
    }
    if out.len() != len {
        return Err(Error::Truncated);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_profiles() {
        let l = Limits::default();
        assert_eq!(raw_profile(b"\nexif\n       4\n45786966\n", &l), Ok(b"Exif".to_vec()));
        assert_eq!(raw_profile(b"\nexif\n 3\n45 78\n69 66", &l), Ok(b"Exi".to_vec()));
        assert_eq!(raw_profile(b"\nexif\n 5\n45786966", &l), Err(Error::Truncated));
        assert!(raw_profile(b"\nexif\n 2\nzz", &l).is_err());
        assert!(raw_profile(b"\nexif\n", &l).is_err());
        assert!(raw_profile(b"", &l).is_err());
        assert_eq!(raw_profile(b"\nexif\n 99999999\n00", &Limits { max_inflate: 100, ..l }), Err(Error::TooMany("raw profile bytes")));
    }

    #[test]
    fn inflate_is_capped() {
        let packed = fdeflate::compress_to_vec(&[7u8; 1000]);
        assert_eq!(inflate(&packed, 1000).map(|v| v.len()), Ok(1000));
        assert_eq!(inflate(&packed, 999), Err(Error::TooMany("inflated text bytes")));
        assert!(matches!(inflate(b"not zlib", 1000), Err(Error::Malformed(_))));
    }
}
