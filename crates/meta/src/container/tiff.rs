//! TIFF (TIFF 6.0): the file is its own EXIF structure; IFD0 holds XMP (tag 700), IPTC (33723),
//! Photoshop resources (34377) and the ICC profile (34675). Writes follow the append-only rule
//! (`xmp-interop.md` §3), so the header's IFD0 pointer, IFD0 and the XMP, IPTC and Photoshop
//! values are the rewritable spans; the EXIF and GPS IFDs are reported but stay where they are.
//! Tag 34377 holding a bare IIM stream (as ImageMagick writes it) is reported as IPTC, read only
//! when tag 33723 is absent.

use std::borrow::Cow;
use std::ops::Range;

use dalica_image::ifd::{Entry, Tiff, Visited};

use super::{BlockKind, Container, Scan};
use crate::exif::{TAG_EXIF_IFD, TAG_GPS_IFD, TAG_ICC, TAG_IPTC, TAG_PHOTOSHOP, TAG_XMP};
use crate::iptc::read::is_iim;
use crate::{Limits, Result};

pub(crate) fn scan<'a>(b: &'a [u8], limits: &Limits) -> Result<Scan<'a>> {
    let mut out = Scan::new(Container::Tiff);
    let tiff = Tiff::new(b)?;
    let il = limits.ifd();
    let mut visited = Visited::default();
    let first = tiff.first_ifd()?;
    let ifd0 = tiff.ifd(first, &mut visited, &il)?;
    out.exif = Some(Cow::Borrowed(b));
    out.span(BlockKind::Structure, 4..8, true);
    out.span(BlockKind::Structure, table(first, ifd0.entries.len()), true);
    for e in &ifd0.entries {
        let (kind, rewritable) = match e.tag {
            TAG_XMP => (BlockKind::Xmp, true),
            TAG_IPTC => (BlockKind::Iptc, true),
            TAG_PHOTOSHOP => (BlockKind::Photoshop, true),
            TAG_ICC => (BlockKind::Icc, false),
            _ => continue,
        };
        let Some(value) = tiff.value(e)? else { continue };
        // ImageMagick puts a bare IIM stream where Photoshop resources belong.
        let kind = if kind == BlockKind::Photoshop && is_iim(&value) { BlockKind::Iptc } else { kind };
        if let Some(range) = tiff.value_range(e)? {
            out.span(kind, range, rewritable);
        }
        let slot = match kind {
            BlockKind::Xmp => &mut out.xmp,
            BlockKind::Iptc => &mut out.iptc,
            BlockKind::Photoshop => &mut out.photoshop,
            _ => continue,
        };
        // Of two bare streams, the first is the one read (tag 33723 before tag 34377).
        if kind != BlockKind::Iptc || slot.is_none() {
            *slot = Some(value);
        }
    }
    // The EXIF and GPS IFDs, for the record; a broken one is the EXIF reader's problem to report.
    for tag in [TAG_EXIF_IFD, TAG_GPS_IFD] {
        if let Some(e) = ifd0.get(tag)
            && let Some(at) = pointer(&tiff, e)
            && let Ok(ifd) = tiff.ifd(at, &mut visited, &il)
        {
            out.span(BlockKind::Exif, table(at, ifd.entries.len()), false);
        }
    }
    out.spans.sort_by_key(|s| s.range.start);
    Ok(out)
}

/// The bytes of an IFD: its count, entries and next pointer.
fn table(at: u32, entries: usize) -> Range<usize> {
    let start = at as usize;
    start..start + 2 + 12 * entries + 4
}

fn pointer(tiff: &Tiff<'_>, e: &Entry) -> Option<u32> {
    tiff.uint(e).filter(|&o| o != 0)
}
