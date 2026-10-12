//! Writing the IPTC-IIM datasets Dalica owns (`xmp-interop.md` §2–§3): 1:90 CodedCharacterSet
//! (UTF-8), 2:25 Keywords (one dataset per tag) and 2:120 Caption-Abstract, merged into an
//! existing stream so that every other dataset keeps its bytes, and into the Photoshop image
//! resources that carry the stream in JPEG APP13 (resource 0x0404), with the IPTC digest
//! (resource 0x0425) recomputed so other applications do not take the IPTC for stale.
//!
//! - [`write_stream`]: a bare IIM stream (TIFF tag 33723 and the inside of resource 0x0404).
//! - [`write_resources`]: a block of Photoshop image resources; every other resource is copied
//!   byte for byte, in its place.
//! - Datasets past the IPTC-IIM 4.2 length limits are cut at a character boundary (a caption at
//!   2,000 bytes, a keyword at 64): the full text goes to XMP. [`Written::truncated`] says which,
//!   and one log line says so per write (the value is never logged, `security.md` §4.4).
//! - Declaring UTF-8 must not turn another application's Latin-1 text into mojibake: when the
//!   stream did not declare UTF-8, record-2 text datasets that are not valid UTF-8 (read as
//!   Latin-1 before the write) are re-encoded to UTF-8, so every dataset reads the same text after
//!   the write as before it. Binary datasets and other records are never re-encoded.
//!
//! Provenance: written for Dalica from the public IPTC-IIM 4.2 specification (dataset tags, the
//! extended length form, the 1:90 escape `ESC % G`, the record-2 length limits), the public
//! description of Photoshop image resource blocks (signature, id, Pascal name padded to even,
//! size, data padded to even) and RFC 1321 (MD5, through the `md5` crate). No other writer's code
//! was consulted.

use super::read::{Charset, Dataset, datasets, resources};
use super::*;
use crate::container::latin1;
use crate::fields::{clean_tags, clean_text};
use crate::write::Changes;
use crate::{Error, Limits, Result};

/// The most bytes IPTC-IIM 4.2 allows in 2:120 Caption-Abstract.
pub const MAX_CAPTION: usize = 2000;
/// The most bytes IPTC-IIM 4.2 allows in one 2:25 Keywords dataset.
pub const MAX_KEYWORD: usize = 64;
/// 2:00 RecordVersion, written (as version 4) only when Dalica starts record 2 itself.
pub const RECORD_VERSION: (u8, u8) = (2, 0);

/// A dataset's record and number.
type Tag = (u8, u8);

/// Record-2 datasets that hold binary data, never re-encoded: the record version, the audio
/// datasets and the object preview.
const BINARY: &[u8] = &[0, 150, 151, 152, 153, 154, 200, 201, 202];

/// What to write. `None` leaves a field as the stream has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IptcEdit<'a> {
    /// `Some(None)` (or a caption that cleans to nothing) removes every 2:120.
    pub caption: Option<Option<&'a str>>,
    /// `Some(&[])` removes every 2:25.
    pub keywords: Option<&'a [String]>,
}

impl IptcEdit<'_> {
    /// The IPTC part of a write: the caption and the tags.
    pub fn of(changes: &Changes) -> IptcEdit<'_> {
        IptcEdit { caption: changes.caption.as_ref().map(Option::as_deref), keywords: changes.tags.as_deref() }
    }

    pub fn is_empty(&self) -> bool {
        self.caption.is_none() && self.keywords.is_none()
    }
}

/// Which values were cut to fit the IPTC-IIM limits (the full text goes to XMP).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Truncated {
    pub caption: bool,
    /// How many keywords were cut.
    pub keywords: usize,
}

impl Truncated {
    pub fn any(&self) -> bool {
        self.caption || self.keywords > 0
    }
}

/// The new bytes and what had to be cut to write them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub truncated: Truncated,
}

/// The MD5 digest Photoshop keeps in resource 0x0425: of the IIM stream, the data of resource
/// 0x0404.
pub fn digest(stream: &[u8]) -> [u8; 16] {
    md5::compute(stream).0
}

/// `stream` (an IIM stream, or `None` to start one) with `edit` written: 1:90 set to UTF-8, the
/// 2:120 and 2:25 datasets replaced where the first of each stood (or inserted in dataset order),
/// every other dataset's bytes kept in its place (re-encoded only as the module docs say), and
/// trailing padding dropped. `Err` when the stream cannot be read, so nothing in it would be lost.
/// A file without IPTC whose edit only removes values needs no stream; the caller skips it.
pub fn write_stream(stream: Option<&[u8]>, edit: &IptcEdit<'_>, limits: &Limits) -> Result<Written> {
    let old = match stream {
        Some(s) => datasets(s, limits)?,
        None => Vec::new(),
    };
    let reencode = Charset::of(&old) != Charset::Utf8;
    let mut truncated = Truncated::default();

    let caption: Option<Option<String>> = edit.caption.map(|c| {
        c.and_then(|c| clean_text(c, limits.max_string)).map(|c| {
            let (cut, was_cut) = cut_at(&c, MAX_CAPTION);
            truncated.caption = was_cut;
            cut
        })
    });
    let keywords: Option<Vec<String>> = edit.keywords.map(|tags| {
        let cleaned = clean_tags(tags.iter().map(String::as_str), limits.max_string, limits.max_tags);
        let cut: Vec<String> = cleaned
            .iter()
            .map(|t| {
                let (cut, was_cut) = cut_at(t, MAX_KEYWORD);
                truncated.keywords += usize::from(was_cut);
                cut
            })
            .collect();
        // Two long tags may share their first 64 bytes: keep the first, as the reader would.
        clean_tags(cut.iter().map(String::as_str), MAX_KEYWORD, limits.max_tags)
    });
    if truncated.any() {
        log::warn!(
            "IPTC: {} cut to the IPTC-IIM length limits (the full text goes to XMP)",
            match (truncated.caption, truncated.keywords) {
                (true, 0) => "the caption was".to_owned(),
                (false, n) => format!("{n} keyword(s) were"),
                (true, n) => format!("the caption and {n} keyword(s) were"),
            }
        );
    }

    // What goes in, by dataset tag; each is emitted before the first old dataset with a larger tag
    // unless the old stream had that dataset, in which case it takes the first one's place.
    let has = |id: (u8, u8)| old.iter().any(|d| (d.record, d.number) == id);
    let mut new: Vec<(Tag, Vec<Vec<u8>>)> = vec![(CODED_CHARACTER_SET, vec![UTF8_ESCAPE.to_vec()])];
    let adds_record2 = caption.as_ref().is_some_and(Option::is_some) || keywords.as_ref().is_some_and(|k| !k.is_empty());
    if adds_record2 && !old.iter().any(|d| d.record == 2) {
        new.push((RECORD_VERSION, vec![vec![0, 4]]));
    }
    if let Some(k) = &keywords {
        new.push((KEYWORDS, k.iter().map(|k| k.as_bytes().to_vec()).collect()));
    }
    if let Some(c) = &caption {
        new.push((CAPTION, c.iter().map(|c| c.as_bytes().to_vec()).collect()));
    }
    let replaced = |id: (u8, u8)| new.iter().any(|(n, _)| *n == id);

    let mut out = Vec::new();
    let mut count = 0usize;
    let mut emitted: Vec<(u8, u8)> = Vec::new();
    let emit_new = |id: (u8, u8), out: &mut Vec<u8>, count: &mut usize, emitted: &mut Vec<(u8, u8)>| -> Result<()> {
        if emitted.contains(&id) {
            return Ok(());
        }
        emitted.push(id);
        for (n, values) in &new {
            if *n == id {
                for v in values {
                    put(out, id, v)?;
                    *count += 1;
                }
            }
        }
        Ok(())
    };
    for d in &old {
        let id = (d.record, d.number);
        // New datasets the old stream lacks go before the first old one with a larger tag.
        for (n, _) in &new {
            if *n < id && !has(*n) {
                emit_new(*n, &mut out, &mut count, &mut emitted)?;
            }
        }
        if replaced(id) {
            emit_new(id, &mut out, &mut count, &mut emitted)?;
            continue;
        }
        put_kept(&mut out, d, stream.unwrap_or_default(), reencode, limits)?;
        count += 1;
    }
    for (n, _) in &new {
        emit_new(*n, &mut out, &mut count, &mut emitted)?;
    }
    if count > limits.max_iptc_datasets {
        return Err(Error::TooMany("IPTC datasets"));
    }
    Ok(Written { bytes: out, truncated })
}

/// `block` (Photoshop image resources, or `None` to start a block) with the IPTC stream of resource
/// 0x0404 written as [`write_stream`] does and the digest resource 0x0425 set to its MD5. Both take
/// the place of the first resource of their id (keeping its signature and name), or are appended
/// after the last resource; every other resource, and any trailing padding, is kept byte for byte.
pub fn write_resources(block: Option<&[u8]>, edit: &IptcEdit<'_>, limits: &Limits) -> Result<Written> {
    let block = block.unwrap_or_default();
    let all = resources(block, limits)?;
    let old_stream = all.iter().find(|r| r.id == RESOURCE_IPTC).map(|r| r.data);
    let written = write_stream(old_stream, edit, limits)?;
    let digest = digest(&written.bytes);

    let mut out = Vec::with_capacity(block.len() + written.bytes.len());
    let (mut iptc_done, mut digest_done) = (false, false);
    for r in &all {
        if r.id == RESOURCE_IPTC && !iptc_done {
            put_resource(&mut out, &r.signature, r.id, r.name, &written.bytes)?;
            iptc_done = true;
        } else if r.id == RESOURCE_IPTC_DIGEST && !digest_done {
            put_resource(&mut out, &r.signature, r.id, r.name, &digest)?;
            digest_done = true;
        } else {
            out.extend_from_slice(block.get(r.range.clone()).unwrap_or_default());
            // A resource cut short of its padding byte at the end of the block gets it back.
            if r.range.len() % 2 == 1 {
                out.push(0);
            }
        }
    }
    if !iptc_done {
        put_resource(&mut out, b"8BIM", RESOURCE_IPTC, &[], &written.bytes)?;
    }
    if !digest_done {
        put_resource(&mut out, b"8BIM", RESOURCE_IPTC_DIGEST, &[], &digest)?;
    }
    // The zero padding after the last resource stays at the end.
    let tail = all.last().map_or(0, |r| r.range.end);
    out.extend_from_slice(block.get(tail..).unwrap_or_default());
    Ok(Written { bytes: out, truncated: written.truncated })
}

/// `text` cut to at most `max` bytes at a character boundary (trailing white space dropped, as
/// the reader would), and whether anything was cut.
fn cut_at(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text.get(..end).unwrap_or_default().trim_end().to_owned(), true)
}

/// One dataset: the tag marker, record, number, length (the extended form past 32,767 bytes) and
/// data.
fn put(out: &mut Vec<u8>, (record, number): (u8, u8), data: &[u8]) -> Result<()> {
    out.extend([0x1c, record, number]);
    match u16::try_from(data.len()) {
        Ok(len) if len < 0x8000 => out.extend(len.to_be_bytes()),
        _ => {
            let len = u32::try_from(data.len()).map_err(|_| Error::TooMany("IPTC dataset bytes"))?;
            out.extend(0x8004u16.to_be_bytes());
            out.extend(len.to_be_bytes());
        }
    }
    out.extend_from_slice(data);
    Ok(())
}

/// An old dataset as it was, or (record-2 text that only read as Latin-1, in a stream about to
/// declare UTF-8) re-encoded to UTF-8 so it reads the same text.
fn put_kept(out: &mut Vec<u8>, d: &Dataset<'_>, stream: &[u8], reencode: bool, limits: &Limits) -> Result<()> {
    if reencode && d.record == 2 && !BINARY.contains(&d.number) && std::str::from_utf8(d.data).is_err() {
        let text = latin1(d.data);
        if text.len() > limits.max_dataset {
            return Err(Error::TooMany("IPTC dataset bytes"));
        }
        return put(out, (d.record, d.number), text.as_bytes());
    }
    out.extend_from_slice(stream.get(d.range.clone()).unwrap_or_default());
    Ok(())
}

/// One image resource: signature, id, Pascal name padded to an even size, size, data padded to an
/// even size.
fn put_resource(out: &mut Vec<u8>, signature: &[u8; 4], id: u16, name: &[u8], data: &[u8]) -> Result<()> {
    let name_len = u8::try_from(name.len()).map_err(|_| Error::Malformed("an image resource name longer than 255 bytes".into()))?;
    let size = u32::try_from(data.len()).map_err(|_| Error::TooMany("Photoshop resource bytes"))?;
    out.extend_from_slice(signature);
    out.extend(id.to_be_bytes());
    out.push(name_len);
    out.extend_from_slice(name);
    if name.len().is_multiple_of(2) {
        out.push(0);
    }
    out.extend(size.to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_at_char_boundaries() {
        assert_eq!(cut_at("abc", 3), ("abc".to_owned(), false));
        assert_eq!(cut_at("abcd", 3), ("abc".to_owned(), true));
        // "é" is two bytes: a cut through it backs off to before it.
        assert_eq!(cut_at("abé", 3), ("ab".to_owned(), true));
        assert_eq!(cut_at("a bc", 2), ("a".to_owned(), true));
    }

    #[test]
    fn long_datasets_use_the_extended_length() {
        let mut out = Vec::new();
        put(&mut out, (2, 120), &vec![b'a'; 0x8000]).unwrap();
        assert_eq!(out.get(..9), Some(&[0x1c, 2, 120, 0x80, 0x04, 0, 0, 0x80, 0][..]));
        let ds = datasets(&out, &Limits { max_dataset: 0x8000, ..Limits::default() }).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].data.len(), 0x8000);
    }

    #[test]
    fn md5_vectors() {
        // RFC 1321 A.5.
        let hex = |d: [u8; 16]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(digest(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(digest(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(digest(b"message digest")), "f96b697d7cb7938d525a2f31aaf161d0");
    }
}
