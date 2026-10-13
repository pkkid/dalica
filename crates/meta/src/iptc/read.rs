//! Reading Photoshop image resources and IPTC-IIM datasets, bounded (`security.md` §4.2: every
//! length inside its block, a dataset ≤ [`Limits::max_dataset`], at most
//! [`Limits::max_iptc_datasets`] datasets and [`Limits::max_resources`] resources).
//!
//! Text is UTF-8 when 1:90 declares it; without 1:90 it is UTF-8 when valid and Latin-1 otherwise
//! (`xmp-interop.md` §2).

use std::ops::Range;

use dalica_core::{Date, DateTime};

use super::*;
use crate::container::{be_u16, be_u32, latin1, take};
use crate::fields::{DateTaken, IptcOwned, clean_tags, clean_text, parse_offset};
use crate::{Error, Limits, Result};

/// One Photoshop image resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource<'a> {
    pub signature: [u8; 4],
    pub id: u16,
    /// The resource's Pascal-string name (usually empty).
    pub name: &'a [u8],
    pub data: &'a [u8],
    /// The whole resource in its block, padding included.
    pub range: Range<usize>,
}

/// The signatures image resources are known to use.
const SIGNATURES: [&[u8; 4]; 6] = [b"8BIM", b"8B64", b"PHUT", b"AgHg", b"DCSR", b"MeSa"];

/// Every image resource of a block. Zero bytes after the last one are padding.
pub fn resources<'a>(block: &'a [u8], limits: &Limits) -> Result<Vec<Resource<'a>>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < block.len() {
        let rest = block.get(at..).unwrap_or_default();
        if rest.iter().all(|&b| b == 0) {
            break;
        }
        if out.len() >= limits.max_resources {
            return Err(Error::TooMany("Photoshop resources"));
        }
        let sig = take(block, at, 4)?;
        let signature =
            [sig.first().copied().unwrap_or(0), sig.get(1).copied().unwrap_or(0), sig.get(2).copied().unwrap_or(0), sig.get(3).copied().unwrap_or(0)];
        if !SIGNATURES.contains(&&signature) {
            return Err(Error::Malformed("an image resource without a known signature".into()));
        }
        let id = be_u16(block, at + 4)?;
        let name_len = usize::from(*take(block, at + 6, 1)?.first().unwrap_or(&0));
        let name = take(block, at + 7, name_len)?;
        // The name with its length byte is padded to an even size.
        let name_total = (1 + name_len + 1) & !1;
        let size_at = at + 6 + name_total;
        let size = be_u32(block, size_at)? as usize;
        let data = take(block, size_at + 4, size)?;
        let end = (size_at + 4 + size + (size & 1)).min(block.len());
        out.push(Resource { signature, id, name, data, range: at..end });
        at = end;
    }
    Ok(out)
}

/// Whether a block where Photoshop image resources are expected (a PNG "Raw profile type 8bim"
/// or "iptc" text, TIFF tag 34377) holds a bare IIM stream instead, as ImageMagick writes it: it
/// starts with the dataset tag marker 0x1C, which no resource signature does.
pub fn is_iim(block: &[u8]) -> bool {
    block.first() == Some(&IIM_MARKER)
}

/// The byte every IIM dataset starts with.
const IIM_MARKER: u8 = 0x1c;

/// One IPTC-IIM dataset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dataset<'a> {
    pub record: u8,
    pub number: u8,
    pub data: &'a [u8],
    /// The whole dataset, tag included.
    pub range: Range<usize>,
}

/// Every dataset of an IIM stream. Zero bytes after the last one are padding.
pub fn datasets<'a>(stream: &'a [u8], limits: &Limits) -> Result<Vec<Dataset<'a>>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < stream.len() {
        let rest = stream.get(at..).unwrap_or_default();
        if rest.first() != Some(&IIM_MARKER) {
            if rest.iter().all(|&b| b == 0) {
                break;
            }
            return Err(Error::Malformed("an IPTC dataset without its tag marker".into()));
        }
        if out.len() >= limits.max_iptc_datasets {
            return Err(Error::TooMany("IPTC datasets"));
        }
        let head = take(stream, at, 5)?;
        let (record, number) = (head.get(1).copied().unwrap_or(0), head.get(2).copied().unwrap_or(0));
        let short = be_u16(stream, at + 3)?;
        let (len, data_at) = if short & 0x8000 == 0 {
            (usize::from(short), at + 5)
        } else {
            // Extended length: the low 15 bits say how many bytes hold the length.
            let n = usize::from(short & 0x7fff);
            if n == 0 || n > 4 {
                return Err(Error::Malformed("an IPTC extended length of an unusable size".into()));
            }
            let bytes = take(stream, at + 5, n)?;
            (bytes.iter().fold(0usize, |v, &b| v << 8 | usize::from(b)), at + 5 + n)
        };
        if len > limits.max_dataset {
            return Err(Error::TooMany("IPTC dataset bytes"));
        }
        let data = take(stream, data_at, len)?;
        out.push(Dataset { record, number, data, range: at..data_at + len });
        at = data_at + len;
    }
    Ok(out)
}

/// How a stream's text is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charset {
    /// 1:90 declares UTF-8.
    Utf8,
    /// No 1:90 (or another one): UTF-8 when valid, else Latin-1.
    Guess,
}

impl Charset {
    pub fn of(datasets: &[Dataset<'_>]) -> Charset {
        match datasets.iter().find(|d| (d.record, d.number) == CODED_CHARACTER_SET) {
            Some(d) if d.data == UTF8_ESCAPE => Charset::Utf8,
            _ => Charset::Guess,
        }
    }

    pub fn decode(self, data: &[u8]) -> String {
        match (self, std::str::from_utf8(data)) {
            (_, Ok(s)) => s.to_owned(),
            (Charset::Utf8, Err(_)) => String::from_utf8_lossy(data).into_owned(),
            (Charset::Guess, Err(_)) => latin1(data),
        }
    }
}

/// The owned fields of an IIM stream: 2:120, every 2:25, and 2:55 with 2:60.
pub fn read(stream: &[u8], limits: &Limits) -> Result<IptcOwned> {
    let ds = datasets(stream, limits)?;
    let charset = Charset::of(&ds);
    let all = |id: (u8, u8)| ds.iter().filter(move |d| (d.record, d.number) == id).map(move |d| charset.decode(d.data));
    let caption = all(CAPTION).find_map(|c| clean_text(&c, limits.max_string));
    let keywords: Vec<String> = all(KEYWORDS).collect();
    let keywords = clean_tags(keywords.iter().map(String::as_str), limits.max_string, limits.max_tags);
    let date = all(DATE_CREATED).next();
    let time = all(TIME_CREATED).next();
    let date_created = date.and_then(|d| date_time(&d, time.as_deref()));
    Ok(IptcOwned { caption, keywords, date_created })
}

/// `CCYYMMDD` and `HHMMSS±HHMM` (or `HHMMSS`, or no time: midnight).
fn date_time(date: &str, time: Option<&str>) -> Option<DateTaken> {
    let d = date.trim();
    let n = |s: Option<&str>| s.filter(|s| s.bytes().all(|b| b.is_ascii_digit())).and_then(|s| s.parse::<u16>().ok());
    if d.len() != 8 {
        return None;
    }
    let date = Date::new(n(d.get(..4))?, u8::try_from(n(d.get(4..6))?).ok()?, u8::try_from(n(d.get(6..8))?).ok()?).ok()?;
    let t = time.map(str::trim).unwrap_or("000000");
    if t.len() != 6 && t.len() != 11 {
        return DateTaken::new(DateTime::new(date, 0, 0, 0).ok()?, None, None);
    }
    let field = |r: Range<usize>| n(t.get(r)).and_then(|v| u8::try_from(v).ok());
    let local = DateTime::new(date, field(0..2)?, field(2..4)?, field(4..6)?).ok()?;
    let offset = t.get(6..).filter(|o| !o.is_empty()).and_then(parse_offset);
    DateTaken::new(local, offset, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iptc_dates() {
        assert_eq!(date_time("20260604", Some("194210+0900")).unwrap().to_string(), "2026-06-04T19:42:10+09:00");
        assert_eq!(date_time("20260604", Some("194210")).unwrap().to_string(), "2026-06-04T19:42:10");
        assert_eq!(date_time("20260604", None).unwrap().to_string(), "2026-06-04T00:00:00");
        assert_eq!(date_time("20260600", None), None);
        assert_eq!(date_time("2026", None), None);
        assert_eq!(date_time("20260604", Some("256000")), None);
    }

    #[test]
    fn bare_streams_are_told_from_resources() {
        assert!(is_iim(&[0x1c, 2, 120, 0, 0]));
        assert!(!is_iim(b"8BIM\x04\x04"));
        assert!(!is_iim(&[]));
        assert!(!is_iim(&[0, 0x1c]));
    }

    #[test]
    fn extended_lengths_and_padding() {
        let l = Limits::default();
        let mut s = vec![0x1c, 2, 120, 0x80, 0x02, 0x00, 0x03];
        s.extend(b"abc");
        s.extend([0, 0]);
        let ds = datasets(&s, &l).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].data, b"abc");
        assert!(matches!(datasets(&[0x1c, 2, 120, 0x80, 0x00], &l), Err(Error::Malformed(_))));
        assert!(matches!(datasets(&[0x1c, 2, 120, 0x80, 0x05, 0, 0, 0, 0, 1], &l), Err(Error::Malformed(_))));
        assert!(matches!(datasets(&[0x1c, 2, 120, 0, 1, b'a', 7], &l), Err(Error::Malformed(_))));
        assert_eq!(datasets(&[0x1c, 2, 120, 0, 9, b'a'], &l), Err(Error::Truncated));
    }
}
