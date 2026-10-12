//! TIFF-structured raws (DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW, …): a stub until M5.9 fills it
//! with the largest embedded JPEG preview found by our own bounded IFD walk (`formats.md` §2).
//!
//! [`recognise`] already tells these raws apart from plain TIFFs by content; the other entry
//! points answer [`Error::Unsupported`] with the tile text [`REASON`] until then (contracts in
//! `formats/mod.rs`).

use std::ops::Range;

use crate::decode::Decoded;
use crate::ifd::{Ifd, Tiff, Visited};
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = false;

/// The tile text for a raw this build does not show.
pub const REASON: &str = "RAW previews aren't supported yet";

const FORMAT: Format = Format::Raw(RawKind::TiffBased);

const TAG_PHOTOMETRIC: u16 = 0x0106;
const TAG_SUB_IFDS: u16 = 0x014a;
const TAG_DNG_VERSION: u16 = 0xc612;
/// Photometric interpretations only raw sensor data uses: CFA and LinearRaw (DNG 1.4 §A).
const RAW_PHOTOMETRIC: [u32; 2] = [32803, 34892];
/// IFDs looked at while recognising: IFD0's chain and its sub-IFDs, never more.
const MAX_IFDS: usize = 32;

/// Whether `head` starts a TIFF-structured raw: a vendor magic number (CR2's "CR" after the TIFF
/// header, ORF's "IIRO"/"IIRS"/"MMOR", RW2's "IIU\0"), or a TIFF whose IFDs carry a DNGVersion
/// tag or raw sensor data. A walk error (truncated `head`, a loop) means "not recognised".
pub fn recognise(head: &[u8]) -> bool {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    if (at(0, b"II*\0") && at(8, b"CR\x02")) || at(0, b"IIRO") || at(0, b"IIRS") || at(0, b"MMOR") || at(0, b"IIU\0") {
        return true;
    }
    Tiff::new(head).is_ok_and(|tiff| has_raw_ifd(head, &tiff).unwrap_or(false))
}

fn has_raw_ifd(head: &[u8], tiff: &Tiff) -> Result<bool> {
    let little = head.starts_with(b"II");
    let limits = Limits { max_ifds: MAX_IFDS, ..Limits::default() };
    let mut visited = Visited::default();
    let mut work = Work::new(MAX_IFDS, "IFDs");
    let mut next = tiff.first_ifd()?;
    while next != 0 {
        work.step()?;
        let ifd = tiff.ifd(next, &mut visited, &limits)?;
        if is_raw(tiff, &ifd) {
            return Ok(true);
        }
        // SubIFDs (TIFF 6.0 Technical Note 1): LONG or IFD offsets, inline when there is one.
        if let Some(e) = ifd.get(TAG_SUB_IFDS) {
            let values = tiff.value(e)?.unwrap_or_default();
            for &[a, b, c, d] in values.as_chunks::<4>().0 {
                work.step()?;
                let sub = if little { u32::from_le_bytes([a, b, c, d]) } else { u32::from_be_bytes([a, b, c, d]) };
                // A bad sub-IFD does not hide a raw one later in the list.
                if tiff.ifd(sub, &mut visited, &limits).is_ok_and(|sub| is_raw(tiff, &sub)) {
                    return Ok(true);
                }
            }
        }
        next = ifd.next;
    }
    Ok(false)
}

fn is_raw(tiff: &Tiff, ifd: &Ifd) -> bool {
    ifd.get(TAG_DNG_VERSION).is_some() || ifd.get(TAG_PHOTOMETRIC).and_then(|e| tiff.uint(e)).is_some_and(|p| RAW_PHOTOMETRIC.contains(&p))
}

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(FORMAT))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(FORMAT))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(FORMAT))
}
