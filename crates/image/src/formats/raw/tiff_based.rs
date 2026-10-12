//! TIFF-structured raws (DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW, …): shown through the largest
//! embedded JPEG preview, found by our own bounded IFD walk (`formats.md` §2).
//!
//! Provenance: written for Dalica from public structure descriptions only: TIFF 6.0 and TIFF
//! Technical Note 1 (IFDs, strips, SubIFDs, old-style JPEG), the DNG 1.4 specification (DNGVersion,
//! NewSubFileType, PreviewColorSpace, raw photometric interpretations), EXIF 2.32 (the EXIF IFD,
//! MakerNote, JPEGInterchangeFormat), and published vendor notes on the header variants (CR2's
//! "CR" marker, ORF's "IIRO", RW2's "IIU\0" and its JPEG-from-raw tag 0x002E) and on the maker-note
//! layouts followed below, and TIFF/EP (CFARepeatPatternDim, CFAPattern). No raw converter's or
//! metadata tool's code was consulted. M6.38 checked these layouts against the real files of the
//! test corpus by looking at their structure only.
//!
//! **Recognising** (`formats.md` §2): a vendor header variant, or an IFD of IFD0's chain or its
//! sub-IFDs carrying DNGVersion or DNGPrivateData (Sony's ARW keeps its maker data there), a CFA
//! or LinearRaw photometric interpretation, or TIFF/EP's CFA pattern tags (Samsung's SRW raw IFD
//! has those and no photometric tag).
//!
//! **Where previews are looked for.** Every IFD reachable from IFD0 (its chain, SubIFDs and the
//! EXIF IFD; seen-set, depth and count caps of `security.md` §4.2), and in each:
//! JPEGInterchangeFormat/Length pairs; old-style or new-style JPEG compression (6, 7) stored in
//! contiguous strips; RW2's JPEG-from-raw tag. In the EXIF IFD's maker note, the vendor preview
//! pointers that are plain IFD offsets: Nikon's preview IFD (inside the maker note's own TIFF
//! header), Olympus' camera-settings IFD (preview start and length), the older Olympus maker note's
//! (`OLYMP\0`, its IFD at 8, offsets from the TIFF header) ThumbnailImage tag 0x0100, and Pentax's
//! preview start and length. A SubIFDs pointer to something whose entries are not in tag order
//! is not followed: Sony's DSLR-A100 aims it at the raw data. A candidate counts when its bytes
//! are a baseline, extended or progressive JPEG (a lossless JPEG is raw sensor data, as in CR2
//! and many DNGs). The largest by pixels wins; the smallest other one is the header's thumbnail.
//!
//! **Strict and lenient parts.** The main structure is read strictly: a loop, an offset past the
//! end or a count past the caps is an error ("Damaged file"). Maker notes are vendor data with
//! their own offset bases: they are walked under the maker-note caps (≤ 64 IFDs, depth ≤ 3) and
//! anything wrong in them only means no candidate from there. An offset past the end of the
//! bytes is reported as `Truncated`, so a header read from the start of a file asks for more; in
//! the maker note too (its extent, a preview pointer that lies past the end from either base),
//! but only when the bytes are known to be such a start: some IFD's strips or tiles run past
//! their end.
//!
//! **Colour.** A preview without its own ICC profile is tagged Adobe RGB (or ProPhoto) when the
//! DNG PreviewColorSpace of its IFD says so, else Adobe RGB when the raw's EXIF colour space or
//! the preview's own EXIF says so; the colour stage converts it to sRGB.
//!
//! **No preview.** A raw whose structure holds image data but no usable preview answers
//! [`Error::Unsupported`] with [`REASON`] (the generic RAW tile); one without any image data at
//! all is damaged.

use std::ops::Range;
use std::sync::OnceLock;

use crate::decode::Decoded;
use crate::exif::{self, ColorSpace};
use crate::formats::jpeg;
use crate::ifd::{Ifd, Tiff, Visited, tags, types};
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw that carries no preview Dalica can show (the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::TiffBased);

const TAG_COMPRESSION: u16 = 0x0103;
const TAG_PHOTOMETRIC: u16 = 0x0106;
const TAG_STRIP_OFFSETS: u16 = 0x0111;
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_STRIP_BYTE_COUNTS: u16 = 0x0117;
const TAG_TILE_OFFSETS: u16 = 0x0144;
const TAG_TILE_BYTE_COUNTS: u16 = 0x0145;
const TAG_JPEG_OFFSET: u16 = 0x0201;
const TAG_JPEG_LENGTH: u16 = 0x0202;
const TAG_MAKER_NOTE: u16 = 0x927c;
const TAG_DNG_VERSION: u16 = 0xc612;
/// DNGPrivateData (DNG 1.4): maker data some raws carry outside DNG too (Sony's ARW).
const TAG_DNG_PRIVATE_DATA: u16 = 0xc634;
/// TIFF/EP's CFARepeatPatternDim and CFAPattern: the colour filter layout of raw sensor data,
/// which some raws (Samsung's SRW) give instead of a CFA photometric interpretation.
const TAG_CFA_REPEAT_PATTERN_DIM: u16 = 0x828d;
const TAG_CFA_PATTERN: u16 = 0x828e;
const TAG_PREVIEW_COLOR_SPACE: u16 = 0xc71a;
/// RW2: the camera's JPEG, stored as the tag's value.
const TAG_RW2_JPEG: u16 = 0x002e;
/// RW2: where the raw data starts.
const TAG_RW2_RAW_OFFSET: u16 = 0x0118;
/// Photometric interpretations only raw sensor data uses: CFA and LinearRaw (DNG 1.4 §A).
const RAW_PHOTOMETRIC: [u32; 2] = [32803, 34892];
/// IFDs looked at while recognising: IFD0's chain and its sub-IFDs, never more.
const MAX_IFDS: usize = 32;
/// Strips of one JPEG candidate (previews are one strip in practice).
const MAX_JPEG_STRIPS: usize = 4096;

/// Maker-note tags: Nikon's preview IFD; Olympus' camera-settings IFD with the preview start and
/// length in it; Pentax's preview length and start.
const NIKON_PREVIEW_IFD: u16 = 0x0011;
const OLYMPUS_CAMERA_SETTINGS: u16 = 0x2020;
const OLYMPUS_PREVIEW_START: u16 = 0x0101;
const OLYMPUS_PREVIEW_LENGTH: u16 = 0x0102;
/// The older Olympus maker note (`OLYMP\0`): a small JPEG stored as the tag's value.
const OLYMPUS_THUMBNAIL_IMAGE: u16 = 0x0100;
const PENTAX_PREVIEW_LENGTH: u16 = 0x0003;
const PENTAX_PREVIEW_START: u16 = 0x0004;

/// Whether `head` starts a TIFF-structured raw: a vendor magic number (CR2's "CR" after the TIFF
/// header, ORF's "IIRO"/"IIRS"/"MMOR", RW2's "IIU\0"), or a TIFF whose IFDs carry a DNGVersion
/// tag or raw sensor data. A walk error (truncated `head`, a loop) means "not recognised".
pub fn recognise(head: &[u8]) -> bool {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    if (at(0, b"II*\0") && at(8, b"CR\x02")) || at(0, b"IIRO") || at(0, b"IIRS") || at(0, b"MMOR") || at(0, b"IIU\0") {
        return true;
    }
    Tiff::new(head).is_ok_and(|tiff| has_raw_ifd(&tiff).unwrap_or(false))
}

fn has_raw_ifd(tiff: &Tiff) -> Result<bool> {
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
        if let Some(e) = ifd.get(tags::SUB_IFDS) {
            for sub in tiff.uints(e, MAX_IFDS, "IFDs")? {
                work.step()?;
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

/// Whether `ifd` marks a camera raw: a DNG tag (DNGVersion, DNGPrivateData), raw sensor data by
/// its photometric interpretation, or TIFF/EP's colour filter pattern tags.
fn is_raw(tiff: &Tiff, ifd: &Ifd) -> bool {
    [TAG_DNG_VERSION, TAG_DNG_PRIVATE_DATA, TAG_CFA_REPEAT_PATTERN_DIM, TAG_CFA_PATTERN].iter().any(|&t| ifd.get(t).is_some())
        || ifd.get(TAG_PHOTOMETRIC).and_then(|e| tiff.uint(e)).is_some_and(|p| RAW_PHOTOMETRIC.contains(&p))
}

/// A colour space a preview is tagged with, other than sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    AdobeRgb,
    ProPhoto,
}

/// One embedded JPEG that can be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Preview {
    range: Range<usize>,
    width: u32,
    height: u32,
    /// From the DNG PreviewColorSpace of its IFD.
    space: Option<Space>,
}

impl Preview {
    fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// What the walk found.
#[derive(Debug, Default)]
struct Found {
    previews: Vec<Preview>,
    orientation: Orientation,
    /// Whether any IFD holds image data (strips, tiles, a raw offset or a JPEG of any kind).
    has_image: bool,
}

impl Found {
    fn largest(&self) -> Option<&Preview> {
        self.previews.iter().max_by_key(|p| (p.pixels(), p.range.len()))
    }

    /// The smallest preview smaller than the largest, as the fast-path thumbnail.
    fn thumbnail(&self) -> Option<&Preview> {
        let largest = self.largest()?.pixels();
        self.previews.iter().filter(|p| p.pixels() < largest).min_by_key(|p| (p.pixels(), p.range.len()))
    }

    /// The preview to show, or why there is none.
    fn best(&self) -> Result<&Preview> {
        match self.largest() {
            Some(p) => Ok(p),
            None if self.has_image => Err(Error::Unsupported(FORMAT)),
            None => Err(Error::Malformed("a raw without any image data".into())),
        }
    }
}

/// An offset past the end may only mean the bytes are the start of a longer file.
fn prefix(e: Error) -> Error {
    if e == Error::OutOfRange { Error::Truncated } else { e }
}

/// Walks the structure and collects every usable preview. A SubIFDs pointer that leads to
/// something whose entries are not in tag order is passed over: some cameras (Sony's DSLR-A100)
/// aim it at the raw data rather than at an IFD.
fn find(bytes: &[u8], limits: &Limits) -> Result<Found> {
    let tiff = Tiff::new_raw(bytes)?;
    let mut visited = Visited::default();
    let follow = [tags::SUB_IFDS, tags::EXIF_IFD];
    let ifds = tiff.walk_where(tiff.first_ifd()?, &follow, limits, &mut visited, |ifd, via| via != Some(tags::SUB_IFDS) || ifd.sorted())?;
    let cut = ifds.iter().any(|(ifd, _)| runs_past_the_end(&tiff, ifd));
    let mut found = Found::default();
    if let Some((ifd0, _)) = ifds.first() {
        let o = ifd0.get(TAG_ORIENTATION).and_then(|e| tiff.uint(e)).and_then(|o| u8::try_from(o).ok());
        found.orientation = o.and_then(Orientation::new).unwrap_or_default();
    }
    for (ifd, depth) in &ifds {
        let space = match ifd.get(TAG_PREVIEW_COLOR_SPACE).and_then(|e| tiff.uint(e)) {
            Some(3) => Some(Space::AdobeRgb),
            Some(4) => Some(Space::ProPhoto),
            _ => None,
        };
        let mut ranges = Vec::new();
        ranges.extend(pair(&tiff, ifd, TAG_JPEG_OFFSET, TAG_JPEG_LENGTH)?);
        if ifd.get(TAG_COMPRESSION).and_then(|e| tiff.uint(e)).is_some_and(|c| c == 6 || c == 7) {
            ranges.extend(strips(&tiff, ifd)?);
        }
        if *depth == 1
            && let Some(e) = ifd.get(TAG_RW2_JPEG).filter(|e| e.kind == types::UNDEFINED)
        {
            ranges.extend(tiff.value_range(e)?);
        }
        // Vendor data is read leniently, except that past the end of bytes known to be a prefix
        // it asks for more.
        match ifd.get(TAG_MAKER_NOTE).map(|e| tiff.value_range(e)) {
            Some(Ok(Some(note))) => ranges.extend(maker_note(&tiff, note, cut, limits)?),
            Some(Err(e)) if cut => return Err(e),
            _ => {}
        }
        if !ranges.is_empty() || [TAG_STRIP_OFFSETS, TAG_TILE_OFFSETS, TAG_RW2_RAW_OFFSET].iter().any(|&t| ifd.get(t).is_some()) {
            found.has_image = true;
        }
        found.previews.extend(ranges.into_iter().filter_map(|r| preview(bytes, r, space, limits)));
    }
    Ok(found)
}

/// Whether the strips or tiles of `ifd` run past the end of the bytes: then the bytes are surely
/// the start of a longer file (a header read), not a whole one.
fn runs_past_the_end(tiff: &Tiff, ifd: &Ifd) -> bool {
    let end = tiff.len() as u64;
    [(TAG_STRIP_OFFSETS, TAG_STRIP_BYTE_COUNTS), (TAG_TILE_OFFSETS, TAG_TILE_BYTE_COUNTS)].iter().any(|&(offsets, counts)| {
        let (Some(offsets), Some(counts)) = (ifd.get(offsets), ifd.get(counts)) else { return false };
        let max = crate::formats::tiff::MAX_CHUNKS as usize;
        match (tiff.uints(offsets, max, "strips"), tiff.uints(counts, max, "strips")) {
            (Ok(o), Ok(c)) => o.iter().zip(&c).any(|(&o, &c)| u64::from(o) + u64::from(c) > end),
            (Err(Error::OutOfRange), _) | (_, Err(Error::OutOfRange)) => true,
            _ => false,
        }
    })
}

/// The byte range an offset tag and a length tag of `ifd` describe, checked against the data.
fn pair(tiff: &Tiff, ifd: &Ifd, offset: u16, length: u16) -> Result<Option<Range<usize>>> {
    let (Some(at), Some(len)) = (ifd.get(offset).and_then(|e| tiff.uint(e)), ifd.get(length).and_then(|e| tiff.uint(e))) else { return Ok(None) };
    range(tiff, u64::from(at), u64::from(len)).map(Some)
}

/// `len` bytes at `at` of `tiff`, as a range of the data `tiff` counts from.
fn range(tiff: &Tiff, at: u64, len: u64) -> Result<Range<usize>> {
    tiff.bytes(at, len)?;
    // `bytes` succeeded, so both ends fit in `usize`.
    Ok(at as usize..(at + len) as usize)
}

/// A JPEG stored in strips: one strip, or strips that follow each other without a gap.
fn strips(tiff: &Tiff, ifd: &Ifd) -> Result<Option<Range<usize>>> {
    let (Some(offsets), Some(counts)) = (ifd.get(TAG_STRIP_OFFSETS), ifd.get(TAG_STRIP_BYTE_COUNTS)) else { return Ok(None) };
    let offsets = tiff.uints(offsets, MAX_JPEG_STRIPS, "strips")?;
    let counts = tiff.uints(counts, MAX_JPEG_STRIPS, "strips")?;
    let (Some(&first), true) = (offsets.first(), offsets.len() == counts.len()) else { return Ok(None) };
    let mut end = u64::from(first);
    for (&at, &len) in offsets.iter().zip(&counts) {
        if u64::from(at) != end {
            return Ok(None);
        }
        end = end.checked_add(u64::from(len)).ok_or(Error::OutOfRange)?;
    }
    range(tiff, u64::from(first), end - u64::from(first)).map(Some)
}

/// `range` of `bytes` as a preview, if it is a JPEG Dalica decodes within the size limit.
fn preview(bytes: &[u8], range: Range<usize>, space: Option<Space>, limits: &Limits) -> Option<Preview> {
    if range.len() as u64 > limits.max_thumbnail {
        return None;
    }
    let (width, height) = jpeg_frame(bytes.get(range.clone())?, limits)?;
    Some(Preview { range, width, height, space })
}

/// The frame size of a baseline, extended or progressive Huffman JPEG (T.81 SOF0–SOF2); `None`
/// for anything else, lossless JPEG (SOF3, raw sensor data) included.
fn jpeg_frame(b: &[u8], limits: &Limits) -> Option<(u32, u32)> {
    if !b.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut pos = 2usize;
    let mut work = Work::new(limits.max_segments, "JPEG segments");
    loop {
        work.step().ok()?;
        if b.get(pos) != Some(&0xff) {
            return None;
        }
        while b.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *b.get(pos)?;
        pos += 1;
        match marker {
            0x01 | 0xd0..=0xd7 => continue,
            0xd8..=0xda => return None,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*b.get(pos)?, *b.get(pos + 1)?]));
        match marker {
            0xc0..=0xc2 => {
                let at = |i: usize| b.get(pos + i).copied().map(u16::from);
                let height = (at(3)? << 8) | at(4)?;
                let width = (at(5)? << 8) | at(6)?;
                return (width > 0 && height > 0).then_some((u32::from(width), u32::from(height)));
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => return None,
            _ => {}
        }
        if len < 2 {
            return None;
        }
        pos = pos.checked_add(len)?;
    }
}

/// Preview ranges (of the whole file) that a maker note points to. Lenient: anything wrong only
/// means fewer candidates, except, when the bytes are known to be the start of a longer file
/// (`cut`), a vendor preview pointer past their end: that is `Err(OutOfRange)`, so the header
/// read reads on.
fn maker_note(tiff: &Tiff, note: Range<usize>, cut: bool, limits: &Limits) -> Result<Vec<Range<usize>>> {
    let caps = Limits { max_ifds: 64, max_ifd_depth: 3, ..*limits };
    let walk = |inner: &Tiff, ifd: u32, follow: &[u16]| inner.walk(ifd, follow, &caps, &mut Visited::default()).unwrap_or_default();
    let data = tiff.data();
    let start = note.start;
    let Some(head) = data.get(note) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    if head.starts_with(b"Nikon\0") {
        // A version, then a TIFF header of its own; offsets count from that header.
        let base = start + 10;
        let Some(inner) = data.get(base..).and_then(|d| Tiff::new(d).ok()) else { return Ok(out) };
        let Ok(first) = inner.first_ifd() else { return Ok(out) };
        for (ifd, _) in walk(&inner, first, &[NIKON_PREVIEW_IFD]) {
            if let Ok(Some(r)) = pair(&inner, &ifd, TAG_JPEG_OFFSET, TAG_JPEG_LENGTH) {
                out.push(r.start + base..r.end + base);
            }
        }
    } else if head.starts_with(b"OLYMPUS\0") {
        // Byte order and version, then the IFD at 12; offsets count from the maker note's start.
        let inner = Tiff::headerless(data.get(start..).unwrap_or_default(), head.get(8..10) == Some(b"II"));
        for (ifd, _) in walk(&inner, 12, &[OLYMPUS_CAMERA_SETTINGS]) {
            out.extend(vendor_pair(tiff, &inner, start, &ifd, (OLYMPUS_PREVIEW_START, OLYMPUS_PREVIEW_LENGTH), cut, limits)?);
        }
    } else if head.starts_with(b"OLYMP\0") {
        // The older Olympus layout: a version, then the IFD at 8, in the file's byte order with
        // offsets counting from the TIFF header; its ThumbnailImage holds a small JPEG.
        let at = u32::try_from(start + 8).map_err(|_| Error::OutOfRange)?;
        if let Some(e) = walk(tiff, at, &[]).first().and_then(|(ifd, _)| ifd.get(OLYMPUS_THUMBNAIL_IMAGE).copied()) {
            out.extend(tiff.value_range(&e).ok().flatten());
        }
    } else if head.starts_with(b"AOC\0") {
        // Byte order, then the IFD at 6.
        let inner = Tiff::headerless(data.get(start..).unwrap_or_default(), head.get(4..6) == Some(b"II"));
        for (ifd, _) in walk(&inner, 6, &[]) {
            out.extend(vendor_pair(tiff, &inner, start, &ifd, (PENTAX_PREVIEW_START, PENTAX_PREVIEW_LENGTH), cut, limits)?);
        }
    }
    Ok(out)
}

/// A maker note's preview start and length: models differ in what the start counts from, so the
/// TIFF header and the maker note's start (`base`) are both tried, and only a range that holds a
/// JPEG counts. When neither base gives a range inside bytes known to be a prefix (`cut`), the
/// preview lies in the part not read yet: `Err(OutOfRange)`.
fn vendor_pair(
    tiff: &Tiff,
    inner: &Tiff,
    base: usize,
    ifd: &Ifd,
    (start, length): (u16, u16),
    cut: bool,
    limits: &Limits,
) -> Result<Option<Range<usize>>> {
    let (Some(at), Some(len)) = (ifd.get(start).and_then(|e| inner.uint(e)), ifd.get(length).and_then(|e| inner.uint(e))) else { return Ok(None) };
    let (at, len) = (u64::from(at), u64::from(len));
    let tiff_relative = range(tiff, at, len).ok();
    let note_relative = range(inner, at, len).ok().map(|r| r.start + base..r.end + base);
    if cut && tiff_relative.is_none() && note_relative.is_none() && len > 0 {
        return Err(Error::OutOfRange);
    }
    Ok([tiff_relative, note_relative].into_iter().flatten().find(|r| tiff.data().get(r.clone()).is_some_and(|b| jpeg_frame(b, limits).is_some())))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let found = find(bytes, limits).map_err(prefix)?;
    let best = found.best()?;
    let tiff = Tiff::new_raw(bytes)?;
    let (exif, exif_error) = match exif::read_tiff(&tiff, limits) {
        Ok(e) => (Some(e), None),
        Err(e) => (None, Some(e)),
    };
    Ok(Header {
        format: FORMAT,
        width: best.width,
        height: best.height,
        orientation: found.orientation,
        exif,
        exif_error,
        thumbnail: found.thumbnail().map(|p| p.range.clone()),
    })
}

/// Decodes the largest preview, tagged with the colour space the raw declares for it.
pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let found = find(bytes, limits).map_err(prefix)?;
    let best = found.best()?;
    let data = bytes.get(best.range.clone()).ok_or(Error::Truncated)?;
    let preview = jpeg::header(data, limits)?;
    if (preview.width, preview.height) != (header.width, header.height) {
        return Err(Error::Malformed(format!(
            "the preview is {} × {}, the header says {} × {}",
            preview.width, preview.height, header.width, header.height
        )));
    }
    let mut decoded = jpeg::decode(data, &preview, budget, limits)?;
    decoded.orientation = header.orientation;
    if decoded.icc.is_none() {
        let adobe = |e: Option<&exif::Exif>| e.and_then(|e| e.color_space) == Some(ColorSpace::AdobeRgb);
        let raw_exif = Tiff::new_raw(bytes).ok().and_then(|t| exif::read_tiff(&t, limits).ok());
        let space = best.space.or_else(|| (adobe(raw_exif.as_ref()) || adobe(preview.exif.as_ref())).then_some(Space::AdobeRgb));
        decoded.icc = space.and_then(profile);
    }
    Ok(decoded)
}

/// The ICC profile of `space`, encoded once by `moxcms` from its published primaries.
fn profile(space: Space) -> Option<Vec<u8>> {
    static ADOBE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    static PROPHOTO: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let encode = |p: moxcms::ColorProfile| p.encode().map_err(|e| log::warn!("RAW: the preview's colour profile could not be built: {e:?}")).ok();
    match space {
        Space::AdobeRgb => ADOBE.get_or_init(|| encode(moxcms::ColorProfile::new_adobe_rgb())).clone(),
        Space::ProPhoto => PROPHOTO.get_or_init(|| encode(moxcms::ColorProfile::new_pro_photo_rgb())).clone(),
    }
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(header(bytes, limits)?.thumbnail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_huffman_jpeg_frames_count() {
        let l = Limits::default();
        let frame = |sof: u8| [&b"\xff\xd8\xff\xe0\0\x04ab\xff"[..], &[sof], b"\0\x0b\x08\0\x10\0\x20\x03\x01\x22\0"].concat();
        assert_eq!(jpeg_frame(&frame(0xc0), &l), Some((32, 16)));
        assert_eq!(jpeg_frame(&frame(0xc2), &l), Some((32, 16)));
        assert_eq!(jpeg_frame(&frame(0xc3), &l), None, "lossless");
        assert_eq!(jpeg_frame(&frame(0xc9), &l), None, "arithmetic");
        assert_eq!(jpeg_frame(b"\xff\xd8\xff\xda", &l), None);
        assert_eq!(jpeg_frame(b"\xff\xd8\xff\xe0\0\x01", &l), None);
        assert_eq!(jpeg_frame(b"\xff\xd8", &l), None);
        let zero = [&b"\xff\xd8\xff\xc0\0\x0b\x08\0\0\0\x20"[..], &[3, 1, 0x22, 0]].concat();
        assert_eq!(jpeg_frame(&zero, &l), None);
        // Endless APP0 segments stop at the segment cap.
        let many: Vec<u8> = [&b"\xff\xd8"[..], &b"\xff\xe0\0\x02".repeat(100)].concat();
        assert_eq!(jpeg_frame(&many, &Limits { max_segments: 10, ..l }), None);
    }

    #[test]
    fn an_offset_past_the_end_reads_as_a_prefix() {
        assert_eq!(prefix(Error::OutOfRange), Error::Truncated);
        assert_eq!(prefix(Error::Loop), Error::Loop);
    }
}
