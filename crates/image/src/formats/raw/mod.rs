//! Camera raw files: only their embedded preview is shown (`formats.md` §2), one module per
//! container family so M5.9 (TIFF-structured) and M5.10 (the others) never share a file.
//!
//! Each module has the entry points of `formats/mod.rs` (its `decode` decodes the largest
//! embedded preview) plus `recognise(head) -> bool`, which [`crate::sniff`] asks in the order of
//! [`RawKind::ALL`].

use std::ops::Range;

use crate::Header;
use crate::Result;
use crate::formats::DecodeFn;
use crate::limits::Limits;
use crate::sniff::RawKind;

pub mod cr3;
pub mod crw;
pub mod mrw;
pub mod raf;
pub mod tiff_based;
pub mod x3f;

/// The RAW container `head` starts, if any.
pub fn recognise(head: &[u8]) -> Option<RawKind> {
    RawKind::ALL.into_iter().find(|&kind| match kind {
        RawKind::TiffBased => tiff_based::recognise(head),
        RawKind::Cr3 => cr3::recognise(head),
        RawKind::Raf => raf::recognise(head),
        RawKind::Crw => crw::recognise(head),
        RawKind::Mrw => mrw::recognise(head),
        RawKind::X3f => x3f::recognise(head),
    })
}

pub fn header(kind: RawKind, bytes: &[u8], limits: &Limits) -> Result<Header> {
    match kind {
        RawKind::TiffBased => tiff_based::header(bytes, limits),
        RawKind::Cr3 => cr3::header(bytes, limits),
        RawKind::Raf => raf::header(bytes, limits),
        RawKind::Crw => crw::header(bytes, limits),
        RawKind::Mrw => mrw::header(bytes, limits),
        RawKind::X3f => x3f::header(bytes, limits),
    }
}

pub fn decoder(kind: RawKind) -> DecodeFn {
    match kind {
        RawKind::TiffBased => tiff_based::decode,
        RawKind::Cr3 => cr3::decode,
        RawKind::Raf => raf::decode,
        RawKind::Crw => crw::decode,
        RawKind::Mrw => mrw::decode,
        RawKind::X3f => x3f::decode,
    }
}

pub fn embedded_thumbnail(kind: RawKind, bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    match kind {
        RawKind::TiffBased => tiff_based::embedded_thumbnail(bytes, limits),
        RawKind::Cr3 => cr3::embedded_thumbnail(bytes, limits),
        RawKind::Raf => raf::embedded_thumbnail(bytes, limits),
        RawKind::Crw => crw::embedded_thumbnail(bytes, limits),
        RawKind::Mrw => mrw::embedded_thumbnail(bytes, limits),
        RawKind::X3f => x3f::embedded_thumbnail(bytes, limits),
    }
}

pub fn decodes(kind: RawKind) -> bool {
    match kind {
        RawKind::TiffBased => tiff_based::DECODES,
        RawKind::Cr3 => cr3::DECODES,
        RawKind::Raf => raf::DECODES,
        RawKind::Crw => crw::DECODES,
        RawKind::Mrw => mrw::DECODES,
        RawKind::X3f => x3f::DECODES,
    }
}

pub fn reason(kind: RawKind) -> &'static str {
    match kind {
        RawKind::TiffBased => tiff_based::REASON,
        RawKind::Cr3 => cr3::REASON,
        RawKind::Raf => raf::REASON,
        RawKind::Crw => crw::REASON,
        RawKind::Mrw => mrw::REASON,
        RawKind::X3f => x3f::REASON,
    }
}
