//! PNG, WebP and TIFF write-back (`xmp-interop.md` §3–§4 and §8): every owned field written and
//! read back exactly (test 1), foreign chunks, tags, strips and maker notes kept byte for byte and
//! in place (test 2), the same write twice giving the same bytes (test 3), IPTC 1:90 and digests
//! (test 6), the size checks (test 7, with the unit tests of each writer), and hostile input
//! (test 10). The images are real enough to decode, and decode to the same pixels after a write.

use crate::build;

use crate::common::{caption, cases, l, xmp_text};
use build::*;
use dalica_image::ifd::{Tiff as TiffReader, Visited};
use dalica_meta::container::payload_digest;
use dalica_meta::iptc::read::{datasets, resources};
use dalica_meta::{BlockKind, Changes, Container, DateTaken, Error, Limits, OwnedFields, Place, WriteOptions, read, scan, verify, write_owned};
use proptest::prelude::*;

const NS: &str = "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"";
/// Another application's description: history, a document id and an unknown namespace.
const FOREIGN: &str = "<rdf:Description rdf:about=\"\" xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\" xmlns:zz=\"urn:example:unknown\" xmpMM:DocumentID=\"xmp.did:0042\" zz:Setting=\"+0.35\"><xmpMM:History><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><zz:action>saved</zz:action></rdf:li></rdf:Seq></xmpMM:History></rdf:Description>";

/// The time of the write, as the engine passes it from the Session's clock.
const WRITTEN_AT: &str = "2026-10-12T09:30:00+02:00";

fn opts() -> WriteOptions {
    WriteOptions { stars_as_ratings: true, metadata_date: DateTaken::parse_iso(WRITTEN_AT), ..WriteOptions::default() }
}

/// Every owned field: an RTL caption with emoji and markup, tags with `/`, `,` and non-ASCII, a
/// star, a southern/western place, a date with an offset and a fraction.
fn everything() -> Changes {
    Changes {
        caption: Some(Some("مرفأ عند الغسق 🌅 & <harbour> \"at dusk\"\nsecond line".to_owned())),
        tags: Some(vec!["Family/2026".into(), "Smith, Jane".into(), "Ünïcødé".into(), "東京".into()]),
        star: Some(true),
        place: Some(Place::new(-33.856_784, -151.215_297)),
        date_taken: DateTaken::parse_iso("2026-06-04T19:42:10.123-03:30"),
    }
}

/// Without the place and the date: what a TIFF without XMP date mirrors and EXIF values takes.
fn text_fields() -> Changes {
    Changes { place: None, date_taken: None, ..everything() }
}

fn owned(bytes: &[u8]) -> OwnedFields {
    read(bytes, &l()).unwrap().owned
}

/// Each requested field reads back exactly (the place within the verification's tolerance).
fn assert_reads_back(bytes: &[u8], c: &Changes) {
    let got = owned(bytes);
    if let Some(caption) = &c.caption {
        assert_eq!(got.caption.as_deref(), caption.as_deref());
    }
    if let Some(tags) = &c.tags {
        assert_eq!(&got.tags, tags);
    }
    if c.star == Some(true) {
        assert_eq!(got.rating, Some(5));
    }
    if let Some(Some(place)) = c.place {
        assert!(got.place.is_some_and(|p| p.close_to(&place, 1e-7)), "{:?}", got.place);
    }
    if let Some(date) = &c.date_taken {
        assert_eq!(got.date_taken.as_ref(), Some(date));
    }
}

fn pixels(bytes: &[u8]) -> dalica_image::Rgba8 {
    dalica_image::decode(bytes, &dalica_image::Limits::default()).unwrap()
}

fn same_pixels(a: &[u8], b: &[u8]) {
    let (pa, pb) = (pixels(a), pixels(b));
    assert_eq!((pa.width, pa.height), (pb.width, pb.height));
    assert!(pa.data == pb.data, "the pixels changed");
}

fn digest_outside_metadata(bytes: &[u8]) -> u64 {
    payload_digest(bytes, &scan(bytes, &l()).unwrap().spans)
}

fn write(bytes: &[u8], c: &Changes) -> Vec<u8> {
    write_owned(bytes, c, &opts()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// PNG.

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

/// An 8 × 6 RGB PNG with real image data: IHDR, `before` chunks, IDAT, `after` chunks, IEND.
fn real_png(before: &[(&[u8; 4], Vec<u8>)], after: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let (w, h) = (8u32, 6u32);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            raw.extend([(x * 31) as u8, (y * 40) as u8, ((x ^ y) * 17) as u8]);
        }
    }
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_chunk(b"IHDR", &ihdr));
    for (kind, data) in before {
        out.extend(png_chunk(kind, data));
    }
    out.extend(png_chunk(b"IDAT", &fdeflate::compress_to_vec(&raw)));
    for (kind, data) in after {
        out.extend(png_chunk(kind, data));
    }
    out.extend(png_chunk(b"IEND", &[]));
    out
}

/// Every chunk's type and data; each CRC checked.
fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 8;
    while at < bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        let data = bytes[at + 8..at + 8 + len].to_vec();
        let crc = u32::from_be_bytes(bytes[at + 8 + len..at + 12 + len].try_into().unwrap());
        assert_eq!(crc, crc32(&bytes[at + 4..at + 8 + len]), "CRC of {}", String::from_utf8_lossy(&kind));
        out.push((kind, data));
        at += 12 + len;
        if &kind == b"IEND" {
            break;
        }
    }
    out
}

fn kinds(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<String> {
    chunks.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).collect()
}

/// A chunk to build: its type and data.
type Chunk = (&'static [u8; 4], Vec<u8>);

/// The foreign chunks every PNG test carries: text, an unknown ancillary chunk, and a text chunk
/// after the image data.
fn png_foreign() -> (Vec<Chunk>, Vec<Chunk>) {
    (
        vec![
            (b"tEXt", text("Software", b"Another Editor 3.1")),
            (b"prVt", b"private data, kept".to_vec()),
            (b"gAMA", 45455u32.to_be_bytes().to_vec()),
        ],
        vec![(b"zTXt", ztxt("Comment", b"written after the image data"))],
    )
}

#[test]
fn png_without_metadata_gets_exif_and_xmp_before_the_image_data() {
    let (before, after) = png_foreign();
    let file = real_png(&before, &after);
    let out = write(&file, &everything());
    assert_reads_back(&out, &everything());
    let chunks = png_chunks(&out);
    assert_eq!(kinds(&chunks), ["IHDR", "tEXt", "prVt", "gAMA", "eXIf", "iTXt", "IDAT", "zTXt", "IEND"]);
    // The XMP chunk: keyword, uncompressed, no language, no translated keyword.
    assert!(chunks[5].1.starts_with(b"XML:com.adobe.xmp\0\0\0\0\0<?xpacket begin="));
    // Every other chunk is byte for byte, and the pixels decode the same.
    let old = png_chunks(&file);
    for kind in [b"IHDR", b"tEXt", b"prVt", b"gAMA", b"IDAT", b"zTXt"] {
        assert_eq!(old.iter().find(|c| &c.0 == kind), chunks.iter().find(|c| &c.0 == kind));
    }
    assert_eq!(digest_outside_metadata(&file), digest_outside_metadata(&out));
    same_pixels(&file, &out);
    // The caption also reads back in full from the XMP alone (PNG gets no IPTC of its own).
    let m = read(&out, &l()).unwrap();
    assert_eq!(m.sources.iptc, Default::default());
    assert_eq!(m.sources.exif.place, owned(&out).place);
}

#[test]
fn png_blocks_are_rewritten_where_they_are() {
    let packet = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} dc:description=\"\"/>{FOREIGN}"));
    let exif = Tiff {
        ifd0: vec![ascii(0x010f, "Canon"), ascii(0x0110, "Canon EOS 5D")],
        exif: vec![ascii(0x9003, "2001:01:01 00:00:00"), undefined(0x927c, b"MAKERNOTE\x00\x01\x02 with absolute offsets")],
        ..Tiff::new()
    }
    .build();
    let iptc = iim(&[(2, 0, &[0, 4]), (2, 5, b"Object name"), (2, 120, b"old caption"), (2, 116, b"(c) someone")]);
    let (before, after) = png_foreign();
    let mut chunks = before.clone();
    chunks.extend([
        (b"eXIf", exif.clone()),
        (b"iTXt", itxt("XML:com.adobe.xmp", &packet, false)),
        (b"zTXt", ztxt("Raw profile type iptc", &raw_profile("IPTC", &iptc))),
    ]);
    let file = real_png(&chunks, &after);
    let out = write(&file, &everything());
    assert_reads_back(&out, &everything());
    let (old, new) = (png_chunks(&file), png_chunks(&out));
    // Same chunks in the same order; the metadata ones rewritten in their own form.
    assert_eq!(kinds(&old), kinds(&new));
    let text_of = |c: &[([u8; 4], Vec<u8>)]| String::from_utf8(c.iter().find(|c| c.1.starts_with(b"XML:com")).unwrap().1.clone()).unwrap();
    assert!(text_of(&new).contains("xmp.did:0042") && text_of(&new).contains("<zz:action>saved</zz:action>"));
    // The IPTC raw profile is still a compressed raw profile; it declares UTF-8 and keeps the
    // other datasets; the caption follows.
    let m = read(&out, &l()).unwrap();
    assert_eq!(m.sources.iptc.caption, everything().caption.unwrap());
    let stream = scan(&out, &l()).unwrap().iptc.unwrap().into_owned();
    let sets = datasets(&stream, &l()).unwrap();
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (1, 90, &b"\x1b%G"[..])));
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (2, 116, &b"(c) someone"[..])));
    // The maker note kept its bytes and its offset inside the EXIF block.
    let old_exif = &old.iter().find(|c| &c.0 == b"eXIf").unwrap().1;
    let new_exif = &new.iter().find(|c| &c.0 == b"eXIf").unwrap().1;
    assert_eq!(maker_note(old_exif), maker_note(new_exif));
    assert_eq!(digest_outside_metadata(&file), digest_outside_metadata(&out));
    same_pixels(&file, &out);
}

#[test]
fn png_xmp_in_ztxt_becomes_itxt_and_raw_profile_exif_keeps_its_form() {
    let packet = xmp_packet(&format!(
        "<rdf:Description rdf:about=\"\" {NS}><dc:subject><rdf:Bag><rdf:li>old</rdf:li></rdf:Bag></dc:subject></rdf:Description>"
    ));
    let exif = Tiff { exif: vec![ascii(0x9003, "2001:01:01 00:00:00")], ..Tiff::new() }.build();
    let file = real_png(
        &[
            (b"zTXt", ztxt("XML:com.adobe.xmp", &packet)),
            (b"tEXt", text("Raw profile type exif", &raw_profile("exif", &[&b"Exif\0\0"[..], &exif].concat()))),
        ],
        &[],
    );
    let c = Changes { tags: Some(vec!["new".into()]), date_taken: DateTaken::parse_iso("2020-02-29T23:59:59+05:45"), ..Changes::default() };
    let out = write(&file, &c);
    assert_reads_back(&out, &c);
    let chunks = png_chunks(&out);
    assert_eq!(kinds(&chunks), ["IHDR", "iTXt", "tEXt", "IDAT", "IEND"]);
    let raw = &chunks[2].1;
    assert!(raw.starts_with(b"Raw profile type exif\0\nexif\n"));
    // The rewritten raw profile still holds the Exif\0\0 prefix it had.
    let hex: String = String::from_utf8_lossy(raw).lines().skip(3).collect();
    assert!(hex.starts_with("457869660000"), "{}", &hex[..12]);
    same_pixels(&file, &out);
}

// ---------------------------------------------------------------------------------------------
// WebP.

/// Writes bits least significant first, as VP8L reads them.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// A real lossless image (RFC 9649): no transforms, no colour cache, one group of simple prefix
/// codes: two greens (a pattern, one bit per pixel), one red, one blue, one alpha.
fn vp8l(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let mut b = Bits { out: vec![0x2f], acc: 0, n: 0 };
    b.put(w - 1, 14);
    b.put(h - 1, 14);
    b.put(u32::from(alpha), 1);
    b.put(0, 3); // version
    b.put(0, 1); // no transform
    b.put(0, 1); // no colour cache
    b.put(0, 1); // no meta prefix codes
    // Green: a simple code of two 8-bit symbols.
    b.put(1, 1);
    b.put(1, 1);
    b.put(1, 1);
    b.put(0x20, 8);
    b.put(0xc0, 8);
    // Red, blue, alpha: one 8-bit symbol each; distance: one 1-bit symbol.
    for v in [0x40, 0x90, if alpha { 0x80 } else { 0xff }] {
        b.put(1, 1);
        b.put(0, 1);
        b.put(1, 1);
        b.put(v, 8);
    }
    b.put(1, 1);
    b.put(0, 1);
    b.put(0, 1);
    b.put(0, 1);
    for y in 0..h {
        for x in 0..w {
            b.put(u32::from((x * 3 + y * 5) % 7 < 3), 1);
        }
    }
    b.finish()
}

/// A real lossy key frame (RFC 6386) whose partitions are all zero bits: every macroblock
/// predicted, no residual. It decodes to a flat image.
fn vp8(w: u16, h: u16) -> Vec<u8> {
    let first = 512u32;
    let tag = (1 << 4) | (first << 5); // key frame, version 0, shown
    let mut out = tag.to_le_bytes()[..3].to_vec();
    out.extend([0x9d, 0x01, 0x2a]);
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(vec![0u8; first as usize + 512]);
    out
}

fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    for (kind, data) in chunks {
        body.extend(*kind);
        body.extend((data.len() as u32).to_le_bytes());
        body.extend(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// An extended-format file: VP8X with the flags of `chunks`, the image, then `chunks`.
fn real_webp(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let (w, h) = (9u32, 7u32);
    let mut flags = 0u8;
    for (kind, _) in chunks {
        flags |= match *kind {
            b"EXIF" => 0x08,
            b"XMP " => 0x04,
            _ => 0,
        };
    }
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend(&(w - 1).to_le_bytes()[..3]);
    vp8x.extend(&(h - 1).to_le_bytes()[..3]);
    let mut all = vec![(b"VP8X", vp8x), (b"VP8L", vp8l(w, h, false))];
    all.extend(chunks.iter().cloned());
    riff(&all)
}

/// Every chunk's type and data, and the RIFF size checked.
fn riff_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert_eq!(size + 8, bytes.len(), "the RIFF size");
    let mut out = Vec::new();
    let mut at = 12;
    while at < bytes.len() {
        let kind: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        out.push((kind, bytes[at + 8..at + 8 + len].to_vec()));
        at += 8 + len + (len & 1);
    }
    out
}

#[test]
fn webp_extended_gets_exif_and_xmp_after_the_image() {
    let file = real_webp(&[(b"ABCD", b"an unknown chunk, kept".to_vec())]);
    let out = write(&file, &everything());
    assert_reads_back(&out, &everything());
    let chunks = riff_chunks(&out);
    assert_eq!(kinds(&chunks), ["VP8X", "VP8L", "ABCD", "EXIF", "XMP "]);
    assert_eq!(chunks[0].1[0], 0x08 | 0x04, "the EXIF and XMP flags");
    let old = riff_chunks(&file);
    assert_eq!(old[0].1[1..], chunks[0].1[1..], "the canvas size");
    assert_eq!(old[1..3], chunks[1..3]);
    assert_eq!(digest_outside_metadata(&file), digest_outside_metadata(&out));
    same_pixels(&file, &out);
}

#[test]
fn webp_chunks_are_replaced_where_they_are() {
    let packet = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"3\"/>{FOREIGN}"));
    let exif = Tiff {
        ifd0: vec![ascii(0x010e, "an old caption")],
        gps: vec![ascii(0x0001, "N"), rationals(0x0002, &[(1, 1), (2, 1), (3, 1)]), ascii(0x0003, "E"), rationals(0x0004, &[(4, 1), (5, 1), (6, 1)])],
        exif: vec![undefined(0x927c, b"MAKER NOTE BYTES")],
        ..Tiff::new()
    }
    .build();
    let file = real_webp(&[(b"EXIF", [&b"Exif\0\0"[..], &exif].concat()), (b"XMP ", packet), (b"ABCD", b"after the metadata".to_vec())]);
    let out = write(&file, &everything());
    assert_reads_back(&out, &everything());
    let chunks = riff_chunks(&out);
    assert_eq!(kinds(&chunks), ["VP8X", "VP8L", "EXIF", "XMP ", "ABCD"]);
    assert!(chunks[2].1.starts_with(b"Exif\0\0"), "the EXIF prefix is kept");
    assert_eq!(maker_note(&chunks[2].1[6..]), maker_note(&exif));
    let xmp = String::from_utf8(chunks[3].1.clone()).unwrap();
    assert!(xmp.contains("xmp.did:0042"));
    same_pixels(&file, &out);
    // Unstarring now writes 0 (Dalica's 5); a file rating of 3 would have been kept.
    let unstar = write(&out, &Changes { star: Some(false), ..Changes::default() });
    assert_eq!(owned(&unstar).rating, Some(0));
    let kept = write_owned(&file, &Changes { star: Some(false), ..Changes::default() }, &opts()).unwrap();
    assert_eq!(kept, file, "unstarring a 3-star file changes nothing");
}

#[test]
fn simple_lossless_webp_is_promoted() {
    for alpha in [false, true] {
        let file = riff(&[(b"VP8L", vp8l(13, 5, alpha))]);
        let out = write(&file, &everything());
        assert_reads_back(&out, &everything());
        let (old, new) = (riff_chunks(&file), riff_chunks(&out));
        assert_eq!(kinds(&new), ["VP8X", "VP8L", "EXIF", "XMP "]);
        let flags = if alpha { 0x10 | 0x08 | 0x04 } else { 0x08 | 0x04 };
        assert_eq!(new[0].1, [flags, 0, 0, 0, 12, 0, 0, 4, 0, 0], "flags and canvas");
        assert_eq!(old[0], new[1], "the image chunk is unchanged");
        same_pixels(&file, &out);
    }
}

#[test]
fn simple_lossy_webp_is_promoted() {
    let file = riff(&[(b"VP8 ", vp8(40, 24))]);
    let before = pixels(&file);
    assert_eq!((before.width, before.height), (40, 24));
    let out = write(&file, &caption("lossy"));
    let new = riff_chunks(&out);
    assert_eq!(kinds(&new), ["VP8X", "VP8 ", "XMP "]);
    assert_eq!(new[0].1, [0x04, 0, 0, 0, 39, 0, 0, 23, 0, 0]);
    assert_eq!(owned(&out).caption.as_deref(), Some("lossy"));
    same_pixels(&file, &out);
    // A first chunk that is not an image cannot be promoted.
    let odd = riff(&[(b"ABCD", vec![1, 2]), (b"VP8 ", vp8(40, 24))]);
    assert!(matches!(write_owned(&odd, &caption("x"), &opts()), Err(Error::Malformed(_))));
}

// ---------------------------------------------------------------------------------------------
// TIFF.

const STRIP_OFFSETS: u16 = 0x0111;

/// An entry's tag, type, count and value range.
type EntryAt = (u16, u16, u32, Option<std::ops::Range<usize>>);

/// IFD0 of a TIFF file: its offset and entries.
fn ifd0(bytes: &[u8]) -> (u32, Vec<EntryAt>) {
    let t = TiffReader::new(bytes).unwrap();
    let first = t.first_ifd().unwrap();
    let ifd = t.ifd(first, &mut Visited::default(), &l().ifd()).unwrap();
    (first, ifd.entries.iter().map(|e| (e.tag, e.kind, e.count, t.value_range(e).unwrap())).collect())
}

fn entry(bytes: &[u8], tag: u16) -> Option<(u16, u32, Option<std::ops::Range<usize>>)> {
    ifd0(bytes).1.into_iter().find(|e| e.0 == tag).map(|(_, k, c, r)| (k, c, r))
}

/// The strip offset value of IFD0 and the maker note's place, which a write must never move.
fn fixed_offsets(bytes: &[u8]) -> (u32, Option<std::ops::Range<usize>>) {
    let t = TiffReader::new(bytes).unwrap();
    let ifd = t.ifd(t.first_ifd().unwrap(), &mut Visited::default(), &l().ifd()).unwrap();
    let strip = t.uint(ifd.get(STRIP_OFFSETS).unwrap()).unwrap();
    (strip, maker_note_range(bytes))
}

fn maker_note_range(block: &[u8]) -> Option<std::ops::Range<usize>> {
    let w = dalica_meta::exif::read::walk(block, &l()).unwrap();
    let e = w.get(dalica_meta::exif::IfdKind::Exif, 0x927c)?;
    w.tiff.value_range(e).unwrap()
}

fn maker_note(block: &[u8]) -> (Option<std::ops::Range<usize>>, Vec<u8>) {
    let r = maker_note_range(block);
    (r.clone(), r.map(|r| block[r].to_vec()).unwrap_or_default())
}

/// A TIFF with an EXIF IFD holding a maker note (no date), and an XMP packet with a date mirror.
fn real_tiff(big_endian: bool, extra: Vec<F>) -> Vec<u8> {
    let packet =
        xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} exif:DateTimeOriginal=\"2001-01-01T00:00:00\" dc:description=\"\"/>{FOREIGN}"));
    let mut ifd0 = vec![ascii(0x010f, "Scanner Co"), bytes(0x02bc, &packet)];
    ifd0.extend(extra);
    tiff_file(Tiff {
        big_endian,
        ifd0,
        exif: vec![undefined(0x927c, b"MAKERNOTE with an absolute offset \x00\x00\x01\x20"), ascii(0x9286, "user comment")],
        ..Tiff::new()
    })
}

/// Every byte of the old file outside IFD0's table and the metadata values is still at its offset.
fn kept_in_place(before: &[u8], after: &[u8]) {
    let spans = scan(before, &l()).unwrap().spans;
    assert!(after.len() >= before.len());
    for r in dalica_meta::container::outside(before.len(), &spans) {
        assert_eq!(before[r.clone()], after[r.clone()], "bytes {r:?} moved or changed");
    }
}

#[test]
fn tiff_takes_every_field_through_xmp_and_iptc() {
    for big_endian in [false, true] {
        let file = real_tiff(big_endian, vec![]);
        let out = write(&file, &everything());
        assert_reads_back(&out, &everything());
        let m = read(&out, &l()).unwrap();
        assert_eq!(m.sources.exif.date_taken, None, "EXIF is never written in a TIFF");
        assert_eq!(m.sources.xmp.date_time_original, everything().date_taken);
        assert_eq!(m.sources.iptc.caption, everything().caption.unwrap());
        // A new IPTC stream is a LONG value (whole words), IFD0 appended with it.
        let (kind, count, range) = entry(&out, 0x83bb).unwrap();
        assert_eq!(kind, LONG);
        assert_eq!(range.unwrap().len(), count as usize * 4);
        assert_ne!(ifd0(&file).0, ifd0(&out).0, "IFD0 gained an entry, so it moved");
        assert_eq!(fixed_offsets(&file), fixed_offsets(&out));
        assert_eq!(maker_note(&file), maker_note(&out));
        kept_in_place(&file, &out);
        same_pixels(&file, &out);
        let xmp = String::from_utf8(scan(&out, &l()).unwrap().xmp.unwrap().into_owned()).unwrap();
        assert!(xmp.contains("xmp.did:0042") && xmp.contains("zz:Setting=\"+0.35\""));
    }
}

#[test]
fn tiff_values_are_rewritten_in_place_or_appended() {
    let file = real_tiff(false, vec![]);
    // The first caption adds an IPTC stream, so IFD0 gains an entry and is appended whole.
    let first = write(&file, &caption("a first caption"));
    assert_ne!(ifd0(&first).0, ifd0(&file).0);
    kept_in_place(&file, &first);
    // A shorter one fits where the old values are: same places, same IFD0, same length.
    let small = write(&first, &caption("short"));
    assert_eq!(small.len(), first.len());
    assert_eq!(ifd0(&small).0, ifd0(&first).0);
    for tag in [0x02bc, 0x83bb] {
        assert_eq!(entry(&small, tag).unwrap().2.unwrap().start, entry(&first, tag).unwrap().2.unwrap().start);
    }
    assert_eq!(owned(&small).caption.as_deref(), Some("short"));
    // A long one does not fit: appended, the old packet zeroed, IFD0 rewritten where it is.
    let long = "long caption ".repeat(400);
    let big = write(&small, &caption(&long));
    let old = entry(&small, 0x02bc).unwrap().2.unwrap();
    let new = entry(&big, 0x02bc).unwrap().2.unwrap();
    assert!(new.start >= small.len() && new.start.is_multiple_of(2));
    assert!(big[old].iter().all(|&b| b == 0), "the old packet is zeroed");
    assert_eq!(ifd0(&big).0, ifd0(&small).0);
    assert_eq!(owned(&big).caption.as_deref(), Some(long.trim_end()));
    assert_eq!(fixed_offsets(&file), fixed_offsets(&big));
    kept_in_place(&small, &big);
    same_pixels(&file, &big);
}

#[test]
fn tiff_image_description_follows_the_caption() {
    let file = tiff_file(Tiff { ifd0: vec![ascii(0x010e, "A scanned family photo")], ..Tiff::new() });
    assert_eq!(owned(&file).caption.as_deref(), Some("A scanned family photo"));
    let old_value = entry(&file, 0x010e).unwrap().2.unwrap();
    let set = write(&file, &caption("Grandparents, 1962"));
    assert_eq!(owned(&set).caption.as_deref(), Some("Grandparents, 1962"));
    assert_eq!(read(&set, &l()).unwrap().sources.exif.caption.as_deref(), Some("Grandparents, 1962"));
    // The old text is no longer referenced but is not moved or zeroed (it lies outside what may change).
    assert_eq!(set[old_value.clone()], file[old_value]);
    kept_in_place(&file, &set);
    let cleared = write(&set, &Changes { caption: Some(None), ..Changes::default() });
    assert_eq!(owned(&cleared).caption, None);
    assert!(entry(&cleared, 0x010e).is_none());
    same_pixels(&file, &cleared);
    // A camera placeholder is not a caption and is left alone.
    let placeholder = tiff_file(Tiff { ifd0: vec![ascii(0x010e, "OLYMPUS DIGITAL CAMERA")], ..Tiff::new() });
    let out = write(&placeholder, &caption("new"));
    assert_eq!(entry(&out, 0x010e).map(|e| e.2), entry(&placeholder, 0x010e).map(|e| e.2));
}

#[test]
fn tiff_place_and_date_go_only_to_the_xmp_mirrors() {
    let place = Changes { place: Some(Place::new(10.0, 20.0)), ..Changes::default() };
    let date = Changes { date_taken: DateTaken::parse_iso("2020-01-01T00:00:00"), ..Changes::default() };
    // With a packet, the place and the date go to their mirrors.
    let file = real_tiff(true, vec![]);
    let out = write(&file, &place);
    assert!(owned(&out).place.is_some_and(|p| p.close_to(&Place::new(10.0, 20.0).unwrap(), 1e-7)));
    assert_reads_back(&write(&file, &date), &date);
}

/// The refusal of a TIFF place or date edit: the error the writer returns before writing anything,
/// naming the field and why (not the verification's "does not read back" after a write), its own
/// variant so its notice never says the written file failed a check.
fn refusal(field: &'static str, why: &str) -> Error {
    Error::Refused { field, why: why.to_owned() }
}

#[test]
fn tiff_place_and_date_that_cannot_land_are_refused_up_front_naming_the_field() {
    let place = Changes { place: Some(Place::new(10.0, 20.0)), ..Changes::default() };
    let clear = Changes { place: Some(None), ..Changes::default() };
    let date = Changes { date_taken: DateTaken::parse_iso("2020-01-01T00:00:00"), ..Changes::default() };
    let no_packet = "this TIFF has no XMP packet for it, and Dalica writes a TIFF's place only there";
    // No packet: no mirror takes a place, and a place alone never starts one.
    let bare = tiff_file(Tiff::new());
    assert_eq!(write_owned(&bare, &place, &opts()), Err(refusal("place", no_packet)));
    // The EXIF holds the date (it wins on reading), and a TIFF's EXIF is never written.
    let exif_date = "this TIFF's EXIF holds the date taken, which comes first on reading, and Dalica writes a TIFF's date taken only to XMP";
    let dated = tiff_file(Tiff { exif: vec![ascii(0x9003, "2001:01:01 00:00:00")], ..Tiff::new() });
    assert_eq!(write_owned(&dated, &date, &opts()), Err(refusal("date taken", exif_date)));
    // Even beside a packet with a mirror, and with a caption in the same write: nothing is written.
    let packet = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} exif:DateTimeOriginal=\"2001-01-01T00:00:00\"/>"));
    let mirrored = tiff_file(Tiff { ifd0: vec![bytes(0x02bc, &packet)], exif: vec![ascii(0x9003, "2001:01:01 00:00:00")], ..Tiff::new() });
    let with_caption = Changes { caption: Some(Some("kept back".into())), ..date.clone() };
    assert_eq!(write_owned(&mirrored, &with_caption, &opts()), Err(refusal("date taken", exif_date)));
    // The EXIF holds a place: neither a new place nor clearing it can land.
    let exif_place = "this TIFF's EXIF holds the place, which comes first on reading, and Dalica writes a TIFF's place only to XMP";
    let gps =
        vec![ascii(0x0001, "N"), rationals(0x0002, &[(1, 1), (0, 1), (0, 1)]), ascii(0x0003, "E"), rationals(0x0004, &[(2, 1), (0, 1), (0, 1)])];
    let placed = tiff_file(Tiff { ifd0: vec![bytes(0x02bc, &packet)], gps, ..Tiff::new() });
    assert!(owned(&placed).place.is_some());
    assert_eq!(write_owned(&placed, &place, &opts()), Err(refusal("place", exif_place)));
    assert_eq!(write_owned(&placed, &clear, &opts()), Err(refusal("place", exif_place)));
    // Asking for the place the EXIF already holds is no refusal: it reads back as asked.
    let same = Changes { place: Some(owned(&placed).place), ..Changes::default() };
    assert!(write_owned(&placed, &same, &opts()).is_ok());
    // A packet without any date property: the date has no mirror to go to.
    let undated = tiff_file(Tiff {
        ifd0: vec![bytes(0x02bc, &xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} dc:description=\"\"/>")))],
        ..Tiff::new()
    });
    let no_date_mirror = "this TIFF has no XMP date property for it, and Dalica writes a TIFF's date taken only there";
    assert_eq!(write_owned(&undated, &date, &opts()), Err(refusal("date taken", no_date_mirror)));
    // The same edits pass where the mirrors take them; other containers are not affected.
    assert!(write_owned(&real_tiff(false, vec![]), &date, &opts()).is_ok());
}

#[test]
fn tiff_iptc_is_updated_where_it_lives() {
    // A Latin-1 bare stream (no 1:90) stays readable: 1:90 added, the old text re-encoded.
    let latin1 = iim(&[(2, 5, b"Caf\xe9 au lait"), (2, 25, b"old")]);
    let mut padded = latin1.clone();
    padded.resize(latin1.len().next_multiple_of(4), 0);
    let words: Vec<Word> = padded.chunks(4).map(|c| Word::U32(u32::from_le_bytes(c.try_into().unwrap()))).collect();
    let stream = F::Value { tag: 0x83bb, kind: LONG, count: words.len() as u32, words };
    let file = tiff_file(Tiff { ifd0: vec![stream], ..Tiff::new() });
    assert_eq!(scan(&file, &l()).unwrap().iptc.as_deref().map(<[u8]>::len), Some(padded.len()));
    let out = write(&file, &Changes { tags: Some(vec!["new".into()]), ..Changes::default() });
    let s = scan(&out, &l()).unwrap().iptc.unwrap().into_owned();
    let sets = datasets(&s, &l()).unwrap();
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (1, 90, &b"\x1b%G"[..])));
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (2, 5, "Café au lait".as_bytes())));
    assert_eq!(owned(&out).tags, ["new"]);
    assert_eq!(entry(&out, 0x83bb).unwrap().0, LONG);
    // Photoshop resources holding IPTC: updated there, digest recomputed, no bare stream added.
    let ps = irb(&[(0x03ed, b"resolution info"), (0x0404, &iim(&[(2, 120, b"old")]))]);
    let file = tiff_file(Tiff { ifd0: vec![undefined(0x8649, &ps)], ..Tiff::new() });
    let out = write(&file, &caption("new caption"));
    assert!(entry(&out, 0x83bb).is_none());
    let block = scan(&out, &l()).unwrap().photoshop.unwrap().into_owned();
    let rs = resources(&block, &l()).unwrap();
    assert_eq!(rs[0].data, b"resolution info");
    let stream = rs.iter().find(|r| r.id == 0x0404).unwrap().data;
    let digest = rs.iter().find(|r| r.id == 0x0425).unwrap().data;
    assert_eq!(digest, dalica_meta::iptc::write::digest(stream));
    assert_eq!(read(&out, &l()).unwrap().sources.iptc.caption.as_deref(), Some("new caption"));
    // Only removing values from a file without IPTC needs no stream.
    let bare = tiff_file(Tiff::new());
    let out = write(&bare, &Changes { tags: Some(vec![]), caption: Some(None), ..Changes::default() });
    assert_eq!(out, bare);
}

/// A caption, tags and a star: what an ImageMagick-written file takes into its IPTC and XMP.
fn caption_and_tags() -> Changes {
    Changes { caption: Some(Some("Nebel über der Brücke".into())), tags: Some(vec!["fog".into(), "Brücke".into()]), ..Changes::default() }
}

#[test]
fn imagemagick_png_takes_iptc_in_its_8bim_profile() {
    // ImageMagick keeps IPTC in a PNG as a bare IIM stream in a "Raw profile type 8bim" text.
    let iptc = iim(&[(2, 0, &[0, 4]), (2, 120, b"Fog over the bridge"), (2, 25, b"fog"), (2, 116, b"(c) someone")]);
    let (before, after) = png_foreign();
    let mut chunks = before.clone();
    chunks.push((b"zTXt", ztxt("Raw profile type 8bim", &raw_profile("8bim", &iptc))));
    let file = real_png(&chunks, &after);
    assert_eq!(owned(&file).caption.as_deref(), Some("Fog over the bridge"));
    let c = caption_and_tags();
    let out = write_owned(&file, &c, &opts()).unwrap();
    verify(&file, &out, &c, &l()).unwrap();
    assert_reads_back(&out, &c);
    // Still a compressed 8bim profile holding a bare stream: 1:90 declared, the other datasets kept.
    let new = png_chunks(&out);
    let profile = new.iter().find(|c| c.1.starts_with(b"Raw profile type 8bim\0")).unwrap();
    assert_eq!(&profile.0, b"zTXt");
    let m = read(&out, &l()).unwrap();
    assert_eq!(m.sources.iptc.caption, c.caption.clone().unwrap());
    let stream = scan(&out, &l()).unwrap().iptc.unwrap().into_owned();
    assert!(dalica_meta::iptc::read::is_iim(&stream));
    let sets = datasets(&stream, &l()).unwrap();
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (1, 90, &b"\x1b%G"[..])));
    assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (2, 116, &b"(c) someone"[..])));
    assert_eq!(kinds(&png_chunks(&file)).len() + 1, kinds(&new).len(), "only the new XMP iTXt is added");
    assert_eq!(digest_outside_metadata(&file), digest_outside_metadata(&out));
    same_pixels(&file, &out);
    assert_eq!(write(&out, &c), out, "writing twice gives the same bytes");
}

/// The block a "Raw profile type" text chunk (`tEXt` or `zTXt`) holds, decoded.
fn profile_bytes(data: &[u8]) -> Vec<u8> {
    let nul = data.iter().position(|&c| c == 0).unwrap();
    // A `zTXt` has its compression method (0) after the keyword; a `tEXt` profile starts "\n".
    let text = if data.get(nul + 1) == Some(&0) { fdeflate::decompress_to_vec(&data[nul + 2..]).unwrap() } else { data[nul + 1..].to_vec() };
    let text = String::from_utf8(text).unwrap();
    let mut lines = text.split_whitespace();
    lines.next(); // the name
    let len: usize = lines.next().unwrap().parse().unwrap();
    let hex: String = lines.collect();
    let out: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
    assert_eq!(out.len(), len);
    out
}

/// The caption of a bare IIM stream or of the IPTC resource of Photoshop resources.
fn caption_in(block: &[u8]) -> Option<String> {
    let stream = if dalica_meta::iptc::read::is_iim(block) {
        block.to_vec()
    } else {
        resources(block, &l()).unwrap().into_iter().find(|r| r.id == 0x0404)?.data.to_vec()
    };
    datasets(&stream, &l()).unwrap().iter().find(|d| (d.record, d.number) == (2, 120)).map(|d| String::from_utf8_lossy(d.data).into_owned())
}

#[test]
fn png_with_two_iptc_profiles_has_every_copy_rewritten() {
    // An `iptc` profile and a later `8bim` one holding the same caption (one tool's copy and
    // another's), and a third `8bim` profile of resources with no IPTC in it.
    let old = b"old caption";
    let iptc = iim(&[(2, 0, &[0, 4]), (2, 120, old), (2, 116, b"(c) someone")]);
    let resolution = (0x03ed, &b"\x00\x48\x00\x00\x00\x01\x00\x01\x00\x48\x00\x00\x00\x01\x00\x01"[..]);
    let ps = irb(&[resolution, (0x0404, &iim(&[(2, 0, &[0, 4]), (2, 120, old), (2, 5, b"Object name")]))]);
    let no_iptc = irb(&[resolution]);
    let (before, after) = png_foreign();
    let mut chunks = before.clone();
    chunks.push((b"zTXt", ztxt("Raw profile type iptc", &raw_profile("IPTC", &iptc))));
    chunks.push((b"tEXt", text("Raw profile type 8bim", &raw_profile("8bim", &ps))));
    chunks.push((b"zTXt", ztxt("Raw profile type 8bim", &raw_profile("8bim", &no_iptc))));
    let file = real_png(&chunks, &after);
    let c = caption_and_tags();
    let out = write_owned(&file, &c, &opts()).unwrap();
    assert_reads_back(&out, &c);
    let (old_chunks, new) = (png_chunks(&file), png_chunks(&out));
    let profiles =
        |cs: &[([u8; 4], Vec<u8>)]| -> Vec<([u8; 4], Vec<u8>)> { cs.iter().filter(|c| c.1.starts_with(b"Raw profile type ")).cloned().collect() };
    let (was, now) = (profiles(&old_chunks), profiles(&new));
    assert_eq!(now.len(), 3);
    let want = c.caption.clone().unwrap().unwrap();
    // Each IPTC-holding copy, in its own chunk type and form, has the new caption; no old caption
    // lingers in any of them; their other datasets and resources stay.
    for (i, kind) in [(0, b"zTXt"), (1, b"tEXt")] {
        assert_eq!(&now[i].0, kind);
        assert_eq!(caption_in(&profile_bytes(&now[i].1)).as_deref(), Some(want.as_str()), "profile {i}");
    }
    assert!(new.iter().all(|c| !c.1.windows(old.len()).any(|w| w == old)), "an old caption is left");
    let first = profile_bytes(&now[0].1);
    assert!(datasets(&first, &l()).unwrap().iter().any(|d| (d.record, d.number, d.data) == (2, 116, &b"(c) someone"[..])));
    let second = profile_bytes(&now[1].1);
    assert!(!dalica_meta::iptc::read::is_iim(&second));
    let res = resources(&second, &l()).unwrap();
    assert!(res.iter().any(|r| r.id == 0x03ed && r.data == resolution.1));
    let stream = res.iter().find(|r| r.id == 0x0404).unwrap().data;
    assert!(datasets(stream, &l()).unwrap().iter().any(|d| (d.record, d.number, d.data) == (2, 5, &b"Object name"[..])));
    // The profile without IPTC, and every other chunk, keep their bytes and their order.
    assert_eq!(now[2], was[2]);
    assert_eq!(kinds(&old_chunks).len() + 1, kinds(&new).len(), "only the new XMP iTXt is added");
    let others = |cs: &[([u8; 4], Vec<u8>)]| -> Vec<([u8; 4], Vec<u8>)> {
        cs.iter()
            .filter(|c| {
                !c.1.starts_with(b"Raw profile type iptc")
                    && !(c.1.starts_with(b"Raw profile type 8bim") && caption_in(&profile_bytes(&c.1)).is_some())
                    && !c.1.starts_with(b"XML:com")
            })
            .cloned()
            .collect()
    };
    assert_eq!(others(&old_chunks), others(&new));
    same_pixels(&file, &out);
    assert_eq!(write(&out, &c), out, "writing twice gives the same bytes");
}

#[test]
fn imagemagick_tiff_takes_iptc_in_its_34377_stream() {
    // ImageMagick keeps IPTC in a TIFF as a bare IIM stream in tag 34377 (Photoshop resources' tag).
    let iptc = iim(&[(2, 0, &[0, 4]), (2, 120, b"Fog over the bridge"), (2, 25, b"fog"), (2, 116, b"(c) someone")]);
    let c = caption_and_tags();
    for big_endian in [false, true] {
        let file = real_tiff(big_endian, vec![bytes(0x8649, &iptc)]);
        assert_eq!(read(&file, &l()).unwrap().sources.iptc.caption.as_deref(), Some("Fog over the bridge"));
        let out = write_owned(&file, &c, &opts()).unwrap();
        verify(&file, &out, &c, &l()).unwrap();
        assert_reads_back(&out, &c);
        // The stream stays in tag 34377 in its own form and type; no tag 33723 is started.
        assert!(entry(&out, 0x83bb).is_none());
        assert_eq!(entry(&out, 0x8649).unwrap().0, BYTE);
        let s = scan(&out, &l()).unwrap();
        assert!(s.photoshop.is_none());
        let stream = s.iptc.unwrap().into_owned();
        let sets = datasets(&stream, &l()).unwrap();
        assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (1, 90, &b"\x1b%G"[..])));
        assert!(sets.iter().any(|d| (d.record, d.number, d.data) == (2, 116, &b"(c) someone"[..])));
        assert_eq!(read(&out, &l()).unwrap().sources.iptc.caption, c.caption.clone().unwrap());
        assert_eq!(fixed_offsets(&file), fixed_offsets(&out));
        kept_in_place(&file, &out);
        same_pixels(&file, &out);
        assert_eq!(write(&out, &c), out, "writing twice gives the same bytes");
    }
    // With tag 33723 too (ImageMagick writes both): both streams follow, so no old caption lingers.
    let file = real_tiff(false, vec![undefined(0x83bb, &iptc), bytes(0x8649, &iptc)]);
    let out = write(&file, &caption("one caption"));
    let t = TiffReader::new(&out).unwrap();
    let ifd = t.ifd(t.first_ifd().unwrap(), &mut Visited::default(), &l().ifd()).unwrap();
    for tag in [0x83bb, 0x8649] {
        let value = t.value(ifd.get(tag).unwrap()).unwrap().unwrap();
        let caption = datasets(&value, &l()).unwrap().into_iter().find(|d| (d.record, d.number) == (2, 120)).unwrap().data.to_vec();
        assert_eq!(caption, b"one caption", "tag {tag}");
    }
}

#[test]
fn tiff_with_shared_or_repeated_metadata_is_refused() {
    // The XMP value overlaps the strip: an in-place write could reach the pixels.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x02bc, BYTE, PIXELS.len() as u32, 0));
    let file = tiff_file(t);
    let strip = fixed_offsets(&file).0;
    let mut patched = file.clone();
    let (first, entries) = ifd0(&file);
    let i = entries.iter().position(|e| e.0 == 0x02bc).unwrap();
    let at = first as usize + 2 + 12 * i + 8;
    patched[at..at + 4].copy_from_slice(&strip.to_le_bytes());
    assert!(matches!(write_owned(&patched, &caption("x"), &opts()), Err(Error::Malformed(_))));
    // Two XMP tags in IFD0.
    let packet = xmp_packet("");
    let twice = tiff_file(Tiff { ifd0: vec![bytes(0x02bc, &packet), bytes(0x02bc, &packet)], ..Tiff::new() });
    assert!(matches!(write_owned(&twice, &caption("x"), &opts()), Err(Error::Malformed(_))));
    // A TIFF-structured raw is still never written.
    let cr2 = b"II*\0\x10\0\0\0CR\x02\0\0\0\0\0\0\0\0\0\0\0".to_vec();
    assert_eq!(write_owned(&cr2, &caption("x"), &opts()), Err(Error::Unsupported(Container::Tiff)));
}

// ---------------------------------------------------------------------------------------------
// Every container.

/// One file per container, with every kind of block the writers handle.
fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let packet = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"5\"/>{FOREIGN}"));
    let exif = Tiff { ifd0: vec![ascii(0x010e, "old")], exif: vec![ascii(0x9003, "2001:01:01 00:00:00")], ..Tiff::new() }.build();
    let (before, after) = png_foreign();
    let mut png_chunks = before;
    png_chunks.extend([(b"eXIf", exif.clone()), (b"iTXt", itxt("XML:com.adobe.xmp", &packet, false))]);
    vec![
        ("png", real_png(&png_chunks, &after)),
        ("png bare", real_png(&[], &[])),
        ("webp", real_webp(&[(b"EXIF", exif), (b"XMP ", packet)])),
        ("webp simple", riff(&[(b"VP8L", vp8l(4, 4, true))])),
        ("tiff", real_tiff(false, vec![])),
        ("tiff big-endian", real_tiff(true, vec![ascii(0x010e, "described")])),
    ]
}

#[test]
fn writing_twice_gives_the_same_bytes() {
    for (name, file) in samples() {
        let c = if name.starts_with("tiff") { text_fields() } else { everything() };
        let once = write(&file, &c);
        assert_reads_back(&once, &c);
        assert_eq!(write(&once, &c), once, "{name}");
        // Writing what the file already holds returns it unchanged.
        assert_eq!(write(&once, &caption(c.caption.clone().unwrap().unwrap().as_str())), once, "{name}");
        assert!(verify(&file, &once, &c, &l()).is_ok(), "{name}");
        same_pixels(&file, &once);
    }
}

#[test]
fn a_new_packet_gets_the_mirrors_in_one_write() {
    // Files without a packet: the packet started for the caption also takes the place and the
    // date, so the second write of the same values finds nothing to change.
    for file in [real_png(&[], &[]), riff(&[(b"VP8L", vp8l(4, 4, false))]), tiff_file(Tiff::new())] {
        let once = write(&file, &everything());
        let xmp = xmp_text(&once);
        assert!(xmp.contains("<exif:GPSLatitude>33,51.40704S</exif:GPSLatitude>"), "{xmp}");
        assert!(xmp.contains("<exif:DateTimeOriginal>2026-06-04T19:42:10.123-03:30</exif:DateTimeOriginal>"), "{xmp}");
        assert!(xmp.contains(&format!("<xmp:MetadataDate>{WRITTEN_AT}</xmp:MetadataDate>")), "{xmp}");
        assert_eq!(write(&once, &everything()), once);
        // A later write at another time moves the metadata date with it.
        let later = WriteOptions { metadata_date: DateTaken::parse_iso("2027-01-01T00:00:00Z"), ..opts() };
        let again = write_owned(&once, &caption("changed"), &later).unwrap();
        assert!(xmp_text(&again).contains("<xmp:MetadataDate>2027-01-01T00:00:00+00:00</xmp:MetadataDate>"));
    }
    // A place or date alone never starts a packet (PNG and WebP keep them in EXIF).
    let file = real_png(&[], &[]);
    let place = write(&file, &Changes { place: Some(Place::new(1.0, 2.0)), ..Changes::default() });
    assert!(scan(&place, &l()).unwrap().xmp.is_none());
}

#[test]
fn without_a_time_the_metadata_date_is_left_alone() {
    let clockless = WriteOptions { metadata_date: None, ..opts() };
    // A new description gets none; an existing one keeps its own.
    let file = real_png(&[], &[]);
    let once = write_owned(&file, &caption("first"), &clockless).unwrap();
    assert!(!xmp_text(&once).contains("MetadataDate"));
    let dated = write(&file, &caption("first"));
    let again = write_owned(&dated, &caption("second"), &clockless).unwrap();
    assert!(xmp_text(&again).contains(&format!("<xmp:MetadataDate>{WRITTEN_AT}</xmp:MetadataDate>")));
    assert_eq!(owned(&again).caption.as_deref(), Some("second"));
    // The same write twice gives the same bytes either way.
    assert_eq!(write_owned(&once, &caption("first"), &clockless).unwrap(), once);
}

#[test]
fn clearing_removes_from_every_source() {
    for (name, file) in samples() {
        let c = if name.starts_with("tiff") { text_fields() } else { everything() };
        let full = write(&file, &c);
        let clear = Changes { caption: Some(None), tags: Some(vec![]), place: Some(None), ..Changes::default() };
        let clear = if name.starts_with("tiff") { Changes { place: None, ..clear } } else { clear };
        let out = write(&full, &clear);
        let got = owned(&out);
        assert_eq!((got.caption, got.tags), (None, vec![]), "{name}");
        if clear.place.is_some() {
            assert_eq!(got.place, None, "{name}");
        }
    }
}

#[test]
fn a_failed_write_returns_an_error_and_no_bytes() {
    // §8 test 7 for these containers: no APP1 limit applies; their 32-bit limits (2³¹ − 1 bytes a
    // PNG chunk, 4 GiB of RIFF or of TIFF offsets) are checked by each writer's unit tests. A write
    // that cannot be completed is an Err and hands back nothing, so the caller keeps its file.
    let dated = tiff_file(Tiff { exif: vec![ascii(0x9003, "2001:01:01 00:00:00")], ..Tiff::new() });
    let both = Changes { caption: Some(Some("kept back".into())), date_taken: DateTaken::parse_iso("2020-01-01T00:00:00"), ..Changes::default() };
    let err = write_owned(&dated, &both, &opts()).unwrap_err();
    assert!(matches!(&err, Error::Refused { field: "date taken", .. }), "{err:?}");
    assert!(err.to_string().starts_with("the date taken cannot be written: "), "{err}");
    let many = WriteOptions { limits: Limits { max_segments: 0, max_ifd_entries: 3, ..l() }, ..opts() };
    for (name, file) in samples() {
        assert!(matches!(write_owned(&file, &caption("x"), &many), Err(Error::TooMany(_))), "{name}");
    }
}

#[test]
fn truncated_and_flipped_files_never_panic() {
    let c = everything();
    for (name, file) in samples() {
        for cut in 0..file.len() {
            let part = &file[..cut];
            if let Ok(out) = write_owned(part, &c, &opts()) {
                assert!(scan(&out, &l()).is_ok(), "{name} cut at {cut}");
            }
        }
        for at in 0..file.len() {
            let mut bad = file.clone();
            bad[at] ^= 0xa5;
            if let Ok(out) = write_owned(&bad, &c, &opts())
                && out != bad
            {
                assert!(verify(&bad, &out, &c.allowed(&opts()), &l()).is_ok(), "{name} flipped at {at}");
            }
        }
    }
}

#[test]
fn every_written_block_is_where_the_scan_finds_it() {
    for (name, file) in samples() {
        let c = if name.starts_with("tiff") { text_fields() } else { everything() };
        let out = write(&file, &c);
        let s = scan(&out, &l()).unwrap();
        assert!(s.spans.iter().any(|s| s.kind == BlockKind::Xmp && s.rewritable), "{name}");
        assert!(s.problems.is_empty(), "{name}: {:?}", s.problems);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn mutated_files_never_panic(which in 0usize..6, edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8)) {
        let mut file = samples().swap_remove(which).1;
        for (at, byte) in edits {
            let n = file.len();
            file[at % n] = byte;
        }
        if let Ok(out) = write_owned(&file, &everything(), &opts()) {
            prop_assert!(read(&out, &l()).is_ok());
        }
    }

    #[test]
    fn random_captions_and_tags_round_trip(cap in "\\PC{0,40}", tags in proptest::collection::vec("[^\\x00-\\x1f]{1,12}", 0..5), which in 0usize..6) {
        let file = samples().swap_remove(which).1;
        let c = Changes { caption: Some(Some(cap)), tags: Some(tags), ..Changes::default() };
        // Text that cleans to nothing or repeats a tag is fine: verification compares cleaned values.
        let out = write_owned(&file, &c, &opts());
        prop_assert!(out.is_ok(), "{:?}", out.err());
    }
}
