//! TIFF-structured raws (`formats.md` §2) and uncompressed EXIF strip thumbnails.
//!
//! Every file is built here, around `scenes` JPEGs, from public structure descriptions: TIFF 6.0
//! and Technical Note 1 (IFDs, strips, SubIFDs), DNG 1.4 (DNGVersion, NewSubFileType, raw
//! photometric interpretations, PreviewColorSpace), EXIF 2.32 (IFD1 thumbnails, the EXIF and
//! Interoperability IFDs, MakerNote) and published vendor notes on each raw's layout (CR2's header
//! and four IFDs, ORF's and RW2's header variants and RW2's JPEG tag, the Nikon, Olympus and Pentax
//! maker-note preview pointers). No camera file is used or committed.

use std::path::PathBuf;

use dalica_image::exif::{self, ColorSpace};
use dalica_image::formats;
use dalica_image::{
    ColorModel, DecodeOptions, Error, Format, Limits, RawKind, Rgba8, decode, decode_as, decode_thumbnail, decode_with, read_header, sniff,
};
use dalica_scenes::{ByteOrder, Exif, Scene, fingerprint, jpeg};
use proptest::prelude::*;

const RAW: Format = Format::Raw(RawKind::TiffBased);
const ON: DecodeOptions = DecodeOptions { color_managed: true };
const OFF: DecodeOptions = DecodeOptions { color_managed: false };
const SEED: u64 = 59;

// ---- A small TIFF writer -----------------------------------------------------------------------

/// A field value; its type and count follow from the variant.
#[derive(Clone, Debug)]
enum V {
    Byte(Vec<u8>),
    Ascii(&'static str),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Undefined(Vec<u8>),
    /// IFD offsets with the IFD type (13).
    Ifd(Vec<u32>),
}

/// Writes a TIFF structure into one buffer: blobs and IFDs where asked, values that do not fit in
/// an entry appended after it. Offsets count from the buffer's first byte.
struct Writer {
    little: bool,
    buf: Vec<u8>,
}

impl Writer {
    /// A buffer starting with `head` (the TIFF header, a maker note's preamble, …).
    fn new(little: bool, head: &[u8]) -> Writer {
        Writer { little, buf: head.to_vec() }
    }

    fn u16(&self, v: u16) -> [u8; 2] {
        if self.little { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    fn u32(&self, v: u32) -> [u8; 4] {
        if self.little { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    fn pad(&mut self) {
        if self.buf.len() % 2 == 1 {
            self.buf.push(0);
        }
    }

    fn here(&self) -> u32 {
        self.buf.len() as u32
    }

    fn blob(&mut self, b: &[u8]) -> u32 {
        self.pad();
        let at = self.here();
        self.buf.extend_from_slice(b);
        at
    }

    /// Room for an IFD of `n` entries at the current position, filled later by [`Writer::fill`].
    fn reserve(&mut self, n: usize) -> u32 {
        self.pad();
        let at = self.here();
        self.buf.resize(self.buf.len() + 2 + 12 * n + 4, 0);
        at
    }

    /// Fills the IFD reserved at `at` with `entries` (sorted here) and `next`.
    fn fill(&mut self, at: u32, mut entries: Vec<(u16, V)>, next: u32) {
        entries.sort_by_key(|(tag, _)| *tag);
        let mut table = self.u16(entries.len() as u16).to_vec();
        for (tag, v) in entries {
            let (kind, count, bytes): (u16, usize, Vec<u8>) = match v {
                V::Byte(b) => (1, b.len(), b),
                V::Ascii(s) => (2, s.len() + 1, [s.as_bytes(), b"\0"].concat()),
                V::Short(s) => (3, s.len(), s.iter().flat_map(|&v| self.u16(v)).collect()),
                V::Long(l) => (4, l.len(), l.iter().flat_map(|&v| self.u32(v)).collect()),
                V::Undefined(b) => (7, b.len(), b),
                V::Ifd(l) => (13, l.len(), l.iter().flat_map(|&v| self.u32(v)).collect()),
            };
            table.extend(self.u16(tag));
            table.extend(self.u16(kind));
            table.extend(self.u32(count as u32));
            if bytes.len() <= 4 {
                let mut inline = bytes.clone();
                inline.resize(4, 0);
                table.extend(inline);
            } else {
                let off = self.blob(&bytes);
                table.extend(self.u32(off));
            }
        }
        table.extend(self.u32(next));
        let at = at as usize;
        assert_eq!(table.len(), self.buf[at..].len().min(table.len()));
        self.buf[at..at + table.len()].copy_from_slice(&table);
    }

    fn ifd(&mut self, entries: Vec<(u16, V)>, next: u32) -> u32 {
        let at = self.reserve(entries.len());
        self.fill(at, entries, next);
        at
    }

    /// The buffer, with IFD0's offset written at 4 (TIFF headers).
    fn finish(mut self, ifd0: u32) -> Vec<u8> {
        let b = self.u32(ifd0);
        self.buf[4..8].copy_from_slice(&b);
        self.buf
    }
}

// Tags.
const NEW_SUBFILE_TYPE: u16 = 0x00fe;
const WIDTH: u16 = 0x0100;
const LENGTH: u16 = 0x0101;
const BITS: u16 = 0x0102;
const COMPRESSION: u16 = 0x0103;
const PHOTOMETRIC: u16 = 0x0106;
const MAKE: u16 = 0x010f;
const STRIP_OFFSETS: u16 = 0x0111;
const ORIENTATION: u16 = 0x0112;
const SPP: u16 = 0x0115;
const ROWS_PER_STRIP: u16 = 0x0116;
const STRIP_COUNTS: u16 = 0x0117;
const SUB_IFDS: u16 = 0x014a;
const JPEG_OFFSET: u16 = 0x0201;
const JPEG_LENGTH: u16 = 0x0202;
const EXIF_IFD: u16 = 0x8769;
const MAKER_NOTE: u16 = 0x927c;
const COLOR_SPACE: u16 = 0xa001;
const INTEROP_IFD: u16 = 0xa005;
const DNG_VERSION: u16 = 0xc612;
const PREVIEW_COLOR_SPACE: u16 = 0xc71a;

// ---- The pictures inside -----------------------------------------------------------------------

/// The pictures a synthetic raw carries.
struct Parts {
    /// The full-size preview (240 × 160 displayed), stored for orientation `o`.
    big: Vec<u8>,
    /// A medium preview (120 × 80).
    medium: Vec<u8>,
    /// A small JPEG thumbnail (64 × 43).
    small: Vec<u8>,
    /// A lossless JPEG claiming more pixels than any preview (raw sensor data).
    lossless: Vec<u8>,
    /// 32 × 21 RGB samples for an uncompressed strip thumbnail.
    strip: dalica_scenes::Rgb8,
}

fn parts(o: u8) -> Parts {
    let scene = |w, h| Scene::new(SEED, w, h);
    let exif = Exif { orientation: o, ..Exif::default() };
    Parts {
        big: jpeg(&scene(240, 160), 92, Some(&exif)).unwrap(),
        medium: jpeg(&scene(120, 80), 90, None).unwrap(),
        small: jpeg(&scene(64, 43), 85, None).unwrap(),
        lossless: [&b"\xff\xd8\xff\xc3\0\x0b\x0c\x10\0\x20\0\x01\x01\x11\0"[..], &[0u8; 64], b"\xff\xd9"].concat(),
        strip: scene(32, 21).render().unwrap(),
    }
}

/// The entries of an uncompressed 8-bit RGB strip image stored at `at` in strips of `rows` rows.
fn strip_entries(w: &mut Writer, img: &dalica_scenes::Rgb8, rows: u32) -> Vec<(u16, V)> {
    let row = img.width as usize * 3;
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    for chunk in img.data.chunks(row * rows as usize) {
        offsets.push(w.blob(chunk));
        counts.push(chunk.len() as u32);
    }
    vec![
        (WIDTH, V::Short(vec![img.width as u16])),
        (LENGTH, V::Short(vec![img.height as u16])),
        (BITS, V::Short(vec![8, 8, 8])),
        (COMPRESSION, V::Short(vec![1])),
        (PHOTOMETRIC, V::Short(vec![2])),
        (STRIP_OFFSETS, V::Long(offsets)),
        (SPP, V::Short(vec![3])),
        (ROWS_PER_STRIP, V::Long(vec![rows])),
        (STRIP_COUNTS, V::Long(counts)),
    ]
}

/// A raw IFD of CFA sensor data (a few bytes standing in for it).
fn cfa(w: &mut Writer) -> Vec<(u16, V)> {
    let at = w.blob(&[0x5a; 96]);
    vec![
        (NEW_SUBFILE_TYPE, V::Long(vec![0])),
        (WIDTH, V::Long(vec![4000])),
        (LENGTH, V::Long(vec![3000])),
        (COMPRESSION, V::Short(vec![1])),
        (PHOTOMETRIC, V::Short(vec![32803])),
        (STRIP_OFFSETS, V::Long(vec![at])),
        (STRIP_COUNTS, V::Long(vec![96])),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Dng,
    Cr2,
    Nef,
    Arw,
    Pef,
    Orf,
    Rw2,
    Srw,
}

const KINDS: [Kind; 8] = [Kind::Dng, Kind::Cr2, Kind::Nef, Kind::Arw, Kind::Pef, Kind::Orf, Kind::Rw2, Kind::Srw];

/// Options for a synthetic raw.
#[derive(Clone, Copy, Debug)]
struct Opts {
    orientation: u8,
    /// DNG PreviewColorSpace of the big preview (DNG only).
    preview_space: Option<u32>,
    /// EXIF ColorSpace, and whether the Interoperability IFD says "R03".
    color_space: Option<u16>,
    r03: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { orientation: 1, preview_space: None, color_space: None, r03: false }
    }
}

/// The EXIF IFD of a raw: the colour space (with an Interoperability IFD) and a maker note.
fn exif_ifd(w: &mut Writer, opts: &Opts, maker_note: Option<Vec<u8>>) -> u32 {
    let mut entries = vec![(0x9003, V::Ascii("2026:06:04 19:42:10"))];
    if let Some(cs) = opts.color_space {
        entries.push((COLOR_SPACE, V::Short(vec![cs])));
        if opts.r03 {
            let interop = w.ifd(vec![(0x0001, V::Ascii("R03"))], 0);
            entries.push((INTEROP_IFD, V::Long(vec![interop])));
        }
    }
    if let Some(note) = maker_note {
        entries.push((MAKER_NOTE, V::Undefined(note)));
    }
    w.ifd(entries, 0)
}

/// A synthetic raw of `kind`, and which of `parts` its largest preview and its thumbnail are.
fn raw(kind: Kind, p: &Parts, opts: Opts) -> Vec<u8> {
    let o = V::Short(vec![u16::from(opts.orientation)]);
    match kind {
        // IFD0: an RGB strip thumbnail with DNGVersion; SubIFDs: the raw (lossless JPEG), the
        // full-size preview and a medium one (JPEG compression 7 in one strip).
        Kind::Dng => {
            let mut w = Writer::new(true, b"II*\0\0\0\0\0");
            let jpeg_ifd = |w: &mut Writer, bytes: &[u8], space: Option<u32>| {
                let at = w.blob(bytes);
                let mut e = vec![
                    (NEW_SUBFILE_TYPE, V::Long(vec![1])),
                    (COMPRESSION, V::Short(vec![7])),
                    (PHOTOMETRIC, V::Short(vec![6])),
                    (STRIP_OFFSETS, V::Long(vec![at])),
                    (STRIP_COUNTS, V::Long(vec![bytes.len() as u32])),
                ];
                if let Some(s) = space {
                    e.push((PREVIEW_COLOR_SPACE, V::Long(vec![s])));
                }
                w.ifd(e, 0)
            };
            let raw_at = w.blob(&p.lossless);
            let raw_ifd = w.ifd(
                vec![
                    (NEW_SUBFILE_TYPE, V::Long(vec![0])),
                    (COMPRESSION, V::Short(vec![7])),
                    (PHOTOMETRIC, V::Short(vec![32803])),
                    (STRIP_OFFSETS, V::Long(vec![raw_at])),
                    (STRIP_COUNTS, V::Long(vec![p.lossless.len() as u32])),
                ],
                0,
            );
            let big = jpeg_ifd(&mut w, &p.big, opts.preview_space);
            let medium = jpeg_ifd(&mut w, &p.medium, Some(2));
            let exif = exif_ifd(&mut w, &opts, None);
            let mut e = strip_entries(&mut w, &p.strip, 8);
            e.extend([
                (NEW_SUBFILE_TYPE, V::Long(vec![1])),
                (ORIENTATION, o),
                (MAKE, V::Ascii("Dalica")),
                (SUB_IFDS, V::Long(vec![raw_ifd, big, medium])),
                (EXIF_IFD, V::Long(vec![exif])),
                (DNG_VERSION, V::Byte(vec![1, 4, 0, 0])),
            ]);
            let ifd0 = w.ifd(e, 0);
            w.finish(ifd0)
        }
        // "CR" after the header and the raw IFD's offset; IFD0: the preview as old-style JPEG
        // strips; IFD1: the JPEG thumbnail; IFD2: an RGB strip image; IFD3: lossless raw data.
        Kind::Cr2 => {
            let mut w = Writer::new(true, b"II*\0\x10\0\0\0CR\x02\0\0\0\0\0");
            let ifds = [w.reserve(8), w.reserve(2), w.reserve(9), w.reserve(3)];
            let big = w.blob(&p.big);
            let exif = exif_ifd(&mut w, &opts, None);
            w.fill(
                ifds[0],
                vec![
                    (WIDTH, V::Short(vec![240])),
                    (LENGTH, V::Short(vec![160])),
                    (COMPRESSION, V::Short(vec![6])),
                    (MAKE, V::Ascii("Canon")),
                    (STRIP_OFFSETS, V::Long(vec![big])),
                    (ORIENTATION, o),
                    (STRIP_COUNTS, V::Long(vec![p.big.len() as u32])),
                    (EXIF_IFD, V::Long(vec![exif])),
                ],
                ifds[1],
            );
            let small = w.blob(&p.small);
            w.fill(ifds[1], vec![(JPEG_OFFSET, V::Long(vec![small])), (JPEG_LENGTH, V::Long(vec![p.small.len() as u32]))], ifds[2]);
            let e = strip_entries(&mut w, &p.strip, 21);
            w.fill(ifds[2], e, ifds[3]);
            let raw_at = w.blob(&p.lossless);
            w.fill(
                ifds[3],
                vec![
                    (COMPRESSION, V::Short(vec![6])),
                    (STRIP_OFFSETS, V::Long(vec![raw_at])),
                    (STRIP_COUNTS, V::Long(vec![p.lossless.len() as u32])),
                ],
                0,
            );
            let mut b = w.finish(ifds[0]);
            b[12..16].copy_from_slice(&ifds[3].to_le_bytes());
            b
        }
        // Big-endian. IFD0: an RGB strip thumbnail; SubIFDs: the full-size JPEG and the raw; the
        // maker note's own TIFF header holds a preview IFD with the medium JPEG.
        Kind::Nef => {
            let mut w = Writer::new(false, b"MM\0*\0\0\0\0");
            let big = w.blob(&p.big);
            let preview = w.ifd(
                vec![
                    (NEW_SUBFILE_TYPE, V::Long(vec![1])),
                    (COMPRESSION, V::Short(vec![6])),
                    (JPEG_OFFSET, V::Long(vec![big])),
                    (JPEG_LENGTH, V::Long(vec![p.big.len() as u32])),
                ],
                0,
            );
            let c = cfa(&mut w);
            let raw_ifd = w.ifd(c, 0);
            let mut n = Writer::new(false, b"MM\0*\0\0\0\0");
            let main = n.reserve(2);
            let medium = n.blob(&p.medium);
            let pv = n.ifd(vec![(JPEG_OFFSET, V::Long(vec![medium])), (JPEG_LENGTH, V::Long(vec![p.medium.len() as u32]))], 0);
            n.fill(main, vec![(0x0001, V::Undefined(b"0210".to_vec())), (0x0011, V::Long(vec![pv]))], 0);
            let note = [&b"Nikon\0\x02\x10\0\0"[..], &n.finish(main)].concat();
            let exif = exif_ifd(&mut w, &opts, Some(note));
            let mut e = strip_entries(&mut w, &p.strip, 5);
            e.extend([
                (NEW_SUBFILE_TYPE, V::Long(vec![1])),
                (ORIENTATION, o),
                (MAKE, V::Ascii("NIKON CORPORATION")),
                (SUB_IFDS, V::Long(vec![preview, raw_ifd])),
                (EXIF_IFD, V::Long(vec![exif])),
            ]);
            let ifd0 = w.ifd(e, 0);
            w.finish(ifd0)
        }
        // IFD0: the preview through JPEGInterchangeFormat, the raw in a SubIFD; IFD1: the thumbnail.
        Kind::Arw => {
            let mut w = Writer::new(true, b"II*\0\0\0\0\0");
            let small = w.blob(&p.small);
            let ifd1 = w.ifd(vec![(JPEG_OFFSET, V::Long(vec![small])), (JPEG_LENGTH, V::Long(vec![p.small.len() as u32]))], 0);
            let c = cfa(&mut w);
            let raw_ifd = w.ifd(c, 0);
            let big = w.blob(&p.big);
            let exif = exif_ifd(&mut w, &opts, None);
            let ifd0 = w.ifd(
                vec![
                    (MAKE, V::Ascii("SONY")),
                    (ORIENTATION, o),
                    (SUB_IFDS, V::Long(vec![raw_ifd])),
                    (JPEG_OFFSET, V::Long(vec![big])),
                    (JPEG_LENGTH, V::Long(vec![p.big.len() as u32])),
                    (EXIF_IFD, V::Long(vec![exif])),
                ],
                ifd1,
            );
            w.finish(ifd0)
        }
        // IFD0: the raw; IFD1: the thumbnail; the maker note ("AOC\0", byte order, IFD at 6)
        // gives the preview's length and its start counted from the TIFF header.
        Kind::Pef => {
            let mut w = Writer::new(true, b"II*\0\0\0\0\0");
            let big = w.blob(&p.big);
            let mut n = Writer::new(true, b"AOC\0II");
            n.ifd(vec![(0x0001, V::Short(vec![0])), (0x0003, V::Long(vec![p.big.len() as u32])), (0x0004, V::Long(vec![big]))], 0);
            let exif = exif_ifd(&mut w, &opts, Some(n.buf));
            let small = w.blob(&p.small);
            let ifd1 = w.ifd(vec![(JPEG_OFFSET, V::Long(vec![small])), (JPEG_LENGTH, V::Long(vec![p.small.len() as u32]))], 0);
            let mut e = cfa(&mut w);
            e.extend([(MAKE, V::Ascii("PENTAX")), (ORIENTATION, o), (EXIF_IFD, V::Long(vec![exif]))]);
            let ifd0 = w.ifd(e, ifd1);
            w.finish(ifd0)
        }
        // "IIRO". IFD0: the raw; IFD1: the thumbnail; the maker note ("OLYMPUS\0", byte order,
        // version, IFD at 12) points to a camera-settings IFD with the preview's start and length,
        // counted from the maker note's start.
        Kind::Orf => {
            let mut w = Writer::new(true, b"IIRO\0\0\0\0");
            let mut n = Writer::new(true, b"OLYMPUS\0II\x03\0");
            let main = n.reserve(2);
            let big = n.blob(&p.big);
            let cs = n.ifd(vec![(0x0100, V::Long(vec![1])), (0x0101, V::Long(vec![big])), (0x0102, V::Long(vec![p.big.len() as u32]))], 0);
            n.fill(main, vec![(0x0000, V::Undefined(b"0100".to_vec())), (0x2020, V::Ifd(vec![cs]))], 0);
            let exif = exif_ifd(&mut w, &opts, Some(n.buf));
            let small = w.blob(&p.small);
            let ifd1 = w.ifd(vec![(JPEG_OFFSET, V::Long(vec![small])), (JPEG_LENGTH, V::Long(vec![p.small.len() as u32]))], 0);
            let mut e = cfa(&mut w);
            e.extend([(MAKE, V::Ascii("OLYMPUS")), (ORIENTATION, o), (EXIF_IFD, V::Long(vec![exif]))]);
            let ifd0 = w.ifd(e, ifd1);
            w.finish(ifd0)
        }
        // "IIU\0". IFD0: sensor size, the raw data's offset and the JPEG as tag 0x002E's value.
        Kind::Rw2 => {
            let mut w = Writer::new(true, b"IIU\0\0\0\0\0");
            let raw_at = w.blob(&[0x33; 64]);
            let exif = exif_ifd(&mut w, &opts, None);
            let ifd0 = w.ifd(
                vec![
                    (0x0002, V::Short(vec![4000])),
                    (0x0003, V::Short(vec![3000])),
                    (0x002e, V::Undefined(p.big.clone())),
                    (MAKE, V::Ascii("Panasonic")),
                    (ORIENTATION, o),
                    (0x0118, V::Long(vec![raw_at])),
                    (EXIF_IFD, V::Long(vec![exif])),
                ],
                0,
            );
            w.finish(ifd0)
        }
        // IFD0: the preview through JPEGInterchangeFormat and a medium one as JPEG strips, the
        // raw in a SubIFD.
        Kind::Srw => {
            let mut w = Writer::new(true, b"II*\0\0\0\0\0");
            let c = cfa(&mut w);
            let raw_ifd = w.ifd(c, 0);
            let big = w.blob(&p.big);
            let exif = exif_ifd(&mut w, &opts, None);
            let ifd0 = w.ifd(
                vec![
                    (NEW_SUBFILE_TYPE, V::Long(vec![1])),
                    (MAKE, V::Ascii("SAMSUNG")),
                    (ORIENTATION, o),
                    (SUB_IFDS, V::Long(vec![raw_ifd])),
                    (JPEG_OFFSET, V::Long(vec![big])),
                    (JPEG_LENGTH, V::Long(vec![p.big.len() as u32])),
                    (EXIF_IFD, V::Long(vec![exif])),
                ],
                0,
            );
            w.finish(ifd0)
        }
    }
}

/// The thumbnail each kind's header should offer.
fn expected_thumbnail(kind: Kind, p: &Parts) -> Option<Vec<u8>> {
    match kind {
        Kind::Dng | Kind::Nef => Some(p.medium.clone()),
        Kind::Cr2 | Kind::Arw | Kind::Pef | Kind::Orf => Some(p.small.clone()),
        Kind::Rw2 | Kind::Srw => None,
    }
}

// ---- Checking pictures -------------------------------------------------------------------------

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rgba(img: &dalica_scenes::Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
}

fn distance(a: &Rgba8, b: &Rgba8) -> f64 {
    fingerprint::distance(&fingerprint::fingerprint(a.width, a.height, &a.data), &fingerprint::fingerprint(b.width, b.height, &b.data))
}

/// Fails (writing actual/expected PNGs) unless the fingerprints agree within `tol` per value.
fn assert_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let d = distance(actual, expected);
    if d > tol || (actual.width, actual.height) != (expected.width, expected.height) {
        let dir = out_dir();
        std::fs::write(dir.join(format!("{case}-actual.png")), fingerprint::png_rgba(actual.width, actual.height, &actual.data)).unwrap();
        std::fs::write(dir.join(format!("{case}-expected.png")), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
        panic!("{case}: {d:.2} away from its reference, tolerance {tol} (PNGs in {})", dir.display());
    }
}

/// Compares `img` with `fingerprints/<name>.json` (`DALICA_BLESS=1` stores it instead).
fn assert_fingerprint(name: &str, img: &Rgba8, tol: f64) {
    let dir = out_dir();
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.data)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(img.width, img.height, &img.data, &expected, tol) {
        std::fs::write(dir.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {} (images in {})", m.message, dir.display());
    }
}

fn upright() -> Rgba8 {
    rgba(&Scene::new(SEED, 240, 160).render().unwrap())
}

// ---- Previews ----------------------------------------------------------------------------------

/// Each kind is recognised by content and shows its largest preview, the same pixels as the
/// preview decoded on its own, matching the stored fingerprint.
#[test]
fn each_kind_yields_its_largest_preview() {
    let l = Limits::default();
    let p = parts(1);
    let alone = decode(&p.big, &l).unwrap();
    assert_fingerprint("raw-tiff-preview", &alone, 2.0);
    assert_close("raw-tiff-preview-vs-scene", &alone, &upright(), 3.0);
    for kind in KINDS {
        let bytes = raw(kind, &p, Opts::default());
        assert_eq!(sniff(&bytes), Some(RAW), "{kind:?}");
        let h = read_header(&bytes, &l).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation.get()), (RAW, 240, 160, 1), "{kind:?}");
        assert_eq!(h.thumbnail(&bytes).map(<[u8]>::to_vec), expected_thumbnail(kind, &p), "{kind:?}");
        assert_eq!(formats::embedded_thumbnail(RAW, &bytes, &l).unwrap(), h.thumbnail, "{kind:?}");
        assert_eq!(h.exif.as_ref().and_then(|e| e.date_time_original).map(|d| d.to_string()).as_deref(), Some("2026-06-04T19:42:10"), "{kind:?}");
        let img = decode(&bytes, &l).unwrap();
        assert_eq!(img, alone, "{kind:?}");
        assert_fingerprint("raw-tiff-preview", &img, 2.0);
        assert_eq!(decode_as(RAW, &bytes, &ON, &l).unwrap(), alone, "{kind:?}");
        let t = decode_thumbnail(&bytes, 60, &l).unwrap();
        assert_eq!((t.width, t.height), (60, 40), "{kind:?}");
    }
}

/// IFD0's orientation turns the preview upright, in both byte orders.
#[test]
fn orientation_1_to_8_comes_from_ifd0() {
    let l = Limits::default();
    let reference = upright();
    for kind in [Kind::Dng, Kind::Nef, Kind::Orf, Kind::Rw2] {
        for o in 1..=8 {
            let bytes = raw(kind, &parts(o), Opts { orientation: o, ..Opts::default() });
            let h = read_header(&bytes, &l).unwrap();
            assert_eq!(h.orientation.get(), o, "{kind:?}");
            assert_eq!((h.width, h.height), if o >= 5 { (160, 240) } else { (240, 160) }, "{kind:?} {o}");
            assert_eq!(h.display_size(), (240, 160));
            let img = decode(&bytes, &l).unwrap();
            assert_close(&format!("raw-tiff-{kind:?}-orientation-{o}"), &img, &reference, 3.0);
        }
    }
}

fn adobe_icc() -> Vec<u8> {
    moxcms::ColorProfile::new_adobe_rgb().encode().unwrap()
}

/// A preview the raw tags Adobe RGB (DNG PreviewColorSpace, EXIF ColorSpace 2, or uncalibrated
/// with the "R03" index) goes through the colour stage like a JPEG carrying the profile.
#[test]
fn adobe_rgb_previews_are_converted() {
    let l = Limits::default();
    let p = parts(1);
    let untagged = decode(&p.big, &l).unwrap();
    let tagged = dalica_scenes::formats::jpeg_icc(&Scene::new(SEED, 240, 160), 92, None, &adobe_icc()).unwrap();
    let reference = decode(&tagged, &l).unwrap();
    assert!(distance(&reference, &untagged) > 3.0, "Adobe RGB must look different: {}", distance(&reference, &untagged));
    let cases = [
        (Kind::Dng, Opts { preview_space: Some(3), ..Opts::default() }),
        (Kind::Cr2, Opts { color_space: Some(2), ..Opts::default() }),
        (Kind::Nef, Opts { color_space: Some(0xffff), r03: true, ..Opts::default() }),
        (Kind::Arw, Opts { color_space: Some(0xffff), r03: true, ..Opts::default() }),
    ];
    for (kind, opts) in cases {
        let bytes = raw(kind, &p, opts);
        let img = decode(&bytes, &l).unwrap();
        assert_close(&format!("raw-tiff-{kind:?}-adobe"), &img, &reference, 1.0);
        assert_fingerprint("raw-tiff-adobe", &img, 2.0);
        // Colour management off: shown as stored.
        assert_eq!(decode_with(&bytes, &OFF, &l).unwrap(), untagged, "{kind:?}");
    }
    // sRGB, uncalibrated without "R03", and a DNG preview tagged sRGB stay as they are.
    for (kind, opts) in [
        (Kind::Cr2, Opts { color_space: Some(1), ..Opts::default() }),
        (Kind::Cr2, Opts { color_space: Some(0xffff), ..Opts::default() }),
        (Kind::Dng, Opts { preview_space: Some(2), ..Opts::default() }),
    ] {
        assert_eq!(decode(&raw(kind, &p, opts), &l).unwrap(), untagged, "{kind:?} {opts:?}");
    }
    // A DNG preview tagged ProPhoto is converted too (more saturated still).
    let pro = decode(&raw(Kind::Dng, &p, Opts { preview_space: Some(4), ..Opts::default() }), &l).unwrap();
    assert!(distance(&pro, &untagged) > distance(&reference, &untagged));
    // The EXIF colour space itself.
    let cs = |opts| read_header(&raw(Kind::Cr2, &p, opts), &l).unwrap().exif.unwrap().color_space;
    assert_eq!(cs(Opts { color_space: Some(1), ..Opts::default() }), Some(ColorSpace::Srgb));
    assert_eq!(cs(Opts { color_space: Some(0xffff), ..Opts::default() }), Some(ColorSpace::Uncalibrated));
    assert_eq!(cs(Opts { color_space: Some(0xffff), r03: true, ..Opts::default() }), Some(ColorSpace::AdobeRgb));
}

/// A raw whose only pictures are raw data (or a lossless JPEG) answers the generic RAW tile; a
/// raw with no image data at all is damaged.
#[test]
fn a_raw_without_a_usable_preview_answers_the_raw_tile() {
    let l = Limits::default();
    let mut w = Writer::new(true, b"II*\0\0\0\0\0");
    let mut e = cfa(&mut w);
    e.push((MAKE, V::Ascii("Dalica")));
    let ifd0 = w.ifd(e, 0);
    let bytes = w.finish(ifd0);
    assert_eq!(sniff(&bytes), Some(RAW));
    for result in [read_header(&bytes, &l).map(|_| ()), decode(&bytes, &l).map(|_| ()), decode_thumbnail(&bytes, 64, &l).map(|_| ())] {
        assert_eq!(result, Err(Error::Unsupported(RAW)));
    }
    assert_eq!(Error::Unsupported(RAW).reason(), "This RAW file has no preview Dalica can show");
    assert!(RAW.supported());
    // A CR2 whose only JPEG is the lossless raw data.
    let p = parts(1);
    let mut w = Writer::new(true, b"II*\0\x10\0\0\0CR\x02\0\0\0\0\0");
    let at = w.blob(&p.lossless);
    let ifd0 =
        w.ifd(vec![(COMPRESSION, V::Short(vec![6])), (STRIP_OFFSETS, V::Long(vec![at])), (STRIP_COUNTS, V::Long(vec![p.lossless.len() as u32]))], 0);
    assert_eq!(read_header(&w.finish(ifd0), &l), Err(Error::Unsupported(RAW)));
    // A DNG holding nothing.
    let mut w = Writer::new(true, b"II*\0\0\0\0\0");
    let ifd0 = w.ifd(vec![(DNG_VERSION, V::Byte(vec![1, 4, 0, 0]))], 0);
    assert_eq!(read_header(&w.finish(ifd0), &l).unwrap_err().reason(), "Damaged file");
}

// ---- Strip thumbnails --------------------------------------------------------------------------

/// An EXIF block whose IFD1 is an uncompressed strip thumbnail (RGB, or grey from its red).
fn strip_block(little: bool, img: &dalica_scenes::Rgb8, rows: u32, grey: bool) -> Vec<u8> {
    let mut w = Writer::new(little, if little { b"II*\0\0\0\0\0" } else { b"MM\0*\0\0\0\0" });
    let mut e = if grey {
        let g = dalica_scenes::Rgb8 { width: img.width, height: img.height, data: img.data.iter().step_by(3).copied().collect() };
        let at = w.blob(&g.data);
        vec![
            (WIDTH, V::Long(vec![g.width])),
            (LENGTH, V::Long(vec![g.height])),
            (BITS, V::Short(vec![8])),
            (COMPRESSION, V::Short(vec![1])),
            (PHOTOMETRIC, V::Short(vec![1])),
            (STRIP_OFFSETS, V::Long(vec![at])),
            (STRIP_COUNTS, V::Long(vec![g.data.len() as u32])),
        ]
    } else {
        strip_entries(&mut w, img, rows)
    };
    e.push((NEW_SUBFILE_TYPE, V::Long(vec![1])));
    let ifd1 = w.ifd(e, 0);
    let ifd0 = w.ifd(vec![(ORIENTATION, V::Short(vec![6]))], ifd1);
    w.finish(ifd0)
}

/// Uncompressed IFD1 thumbnails decode to exactly their samples, in both byte orders, in one
/// strip or many, RGB or grey.
#[test]
fn a_strip_thumbnail_decodes() {
    let l = Limits::default();
    let img = Scene::new(SEED, 32, 21).render().unwrap();
    for little in [true, false] {
        for rows in [1, 4, 21, 64] {
            let block = strip_block(little, &img, rows, false);
            let e = exif::read(&block, &l).unwrap();
            assert_eq!((e.orientation, e.thumbnail.clone()), (Some(6), None));
            let s = e.strip_thumbnail.unwrap();
            assert_eq!((s.width, s.height, s.model, s.strips.len()), (32, 21, ColorModel::Rgb, 21usize.div_ceil(rows as usize)));
            assert_eq!(exif::strip_thumbnail_rgba(&block, &s, &ON, &l).unwrap(), rgba(&img), "little {little}, rows {rows}");
        }
        let block = strip_block(little, &img, 21, true);
        let s = exif::read(&block, &l).unwrap().strip_thumbnail.unwrap();
        assert_eq!(s.model, ColorModel::Gray);
        let out = exif::strip_thumbnail_rgba(&block, &s, &OFF, &l).unwrap();
        let red: Vec<u8> = img.data.iter().step_by(3).flat_map(|&v| [v, v, v, 255]).collect();
        assert_eq!(out.data, red);
    }
    // Inside a raw (the CR2's IFD2 is not IFD1, so its thumbnail stays the JPEG one; an ORF
    // whose IFD1 holds strips offers them).
    let p = parts(1);
    let mut w = Writer::new(true, b"IIRO\0\0\0\0");
    let e = strip_entries(&mut w, &p.strip, 7);
    let ifd1 = w.ifd(e, 0);
    let big = w.blob(&p.big);
    let mut e = cfa(&mut w);
    e.extend([(JPEG_OFFSET, V::Long(vec![big])), (JPEG_LENGTH, V::Long(vec![p.big.len() as u32]))]);
    let ifd0 = w.ifd(e, ifd1);
    let bytes = w.finish(ifd0);
    let h = read_header(&bytes, &l).unwrap();
    let s = h.exif.unwrap().strip_thumbnail.unwrap();
    assert_eq!(exif::strip_thumbnail_rgba(&bytes, &s, &ON, &l).unwrap(), rgba(&p.strip));
    // Moved to where the block sits in a file.
    assert_eq!(s.offset_by(10).unwrap().strips[0].start, s.strips[0].start + 10);
    assert_eq!(s.offset_by(usize::MAX), None);
}

/// IFD1 of a little-endian block as a strip image (`exif::strip_image`, strict).
fn strip_of(b: &[u8], l: &Limits) -> Result<Option<exif::StripThumbnail>, Error> {
    let t = dalica_image::ifd::Tiff::new(b).unwrap();
    let mut v = dalica_image::ifd::Visited::default();
    let ifd0 = t.ifd(t.first_ifd().unwrap(), &mut v, l).unwrap();
    exif::strip_image(&t, &t.ifd(ifd0.next, &mut v, l).unwrap(), l)
}

/// `strip_image` refuses broken strips, which `exif::read` then leaves out (the rest of the
/// EXIF block still counts); unsupported layouts are no thumbnail; decoding checks again.
#[test]
fn strip_thumbnails_are_checked() {
    let l = Limits::default();
    let img = Scene::new(SEED, 32, 21).render().unwrap();
    let block = strip_block(true, &img, 4, false);
    let s = exif::read(&block, &l).unwrap().strip_thumbnail.unwrap();
    // Decoding from the wrong bytes.
    assert_eq!(exif::strip_thumbnail_rgba(&block[..100], &s, &ON, &l), Err(Error::Truncated));
    let mut short = s.clone();
    short.strips.pop();
    assert_eq!(exif::strip_thumbnail_rgba(&block, &short, &ON, &l), Err(Error::Truncated));
    let mut long = s.clone();
    long.strips.push(0..10);
    assert!(matches!(exif::strip_thumbnail_rgba(&block, &long, &ON, &l), Err(Error::Malformed(_))));
    let mut huge = s.clone();
    huge.width = 70_000;
    assert!(matches!(exif::strip_thumbnail_rgba(&block, &huge, &ON, &l), Err(Error::TooLarge { .. })));
    // Past the thumbnail limit.
    let small = Limits { max_thumbnail: 32 * 21 * 3 - 1, ..l };
    assert_eq!(strip_of(&block, &small), Err(Error::TooMany("thumbnail bytes")));
    let e = exif::read(&block, &small).unwrap();
    assert_eq!((e.orientation, e.strip_thumbnail), (Some(6), None));
    // Fewer strip bytes than the size needs; more strips than the cap.
    let patch_long = |b: &mut Vec<u8>, tag: u16, f: &dyn Fn(u32) -> u32| {
        let t = dalica_image::ifd::Tiff::new(b).unwrap();
        let mut v = dalica_image::ifd::Visited::default();
        let ifd0 = t.ifd(t.first_ifd().unwrap(), &mut v, &l).unwrap();
        let ifd1 = t.ifd(ifd0.next, &mut v, &l).unwrap();
        let n = ifd1.entries.iter().position(|e| e.tag == tag).unwrap();
        let at = ifd1.offset as usize + 2 + 12 * n + 8;
        let old = u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        b[at..at + 4].copy_from_slice(&f(old).to_le_bytes());
    };
    let mut b = strip_block(true, &img, 21, false);
    patch_long(&mut b, STRIP_COUNTS, &|v| v - 1);
    assert!(matches!(strip_of(&b, &l), Err(Error::Malformed(_))));
    assert_eq!(exif::read(&b, &l).unwrap().strip_thumbnail, None);
    // A strip outside the block; a list of strip offsets outside it.
    let mut b = strip_block(true, &img, 21, false);
    let end = b.len() as u32;
    patch_long(&mut b, STRIP_OFFSETS, &|_| end - 10);
    assert_eq!(strip_of(&b, &l), Err(Error::OutOfRange));
    let mut b = strip_block(true, &img, 4, false);
    patch_long(&mut b, STRIP_OFFSETS, &|_| u32::MAX - 2);
    assert_eq!(strip_of(&b, &l), Err(Error::OutOfRange));
    assert_eq!(exif::read(&b, &l).unwrap().strip_thumbnail, None);
    let b = strip_block(true, &img, 1, false);
    assert_eq!(strip_of(&b, &Limits { max_ifd_entries: 20, ..l }), Err(Error::TooMany("strips")));
    // 16-bit samples and compressed strips are not a strip thumbnail.
    for (tag, value) in [(BITS, 16u16), (COMPRESSION, 5), (PHOTOMETRIC, 6)] {
        let mut b = strip_block(true, &img, 21, true);
        let t = dalica_image::ifd::Tiff::new(&b).unwrap();
        let mut v = dalica_image::ifd::Visited::default();
        let ifd0 = t.ifd(t.first_ifd().unwrap(), &mut v, &l).unwrap();
        let ifd1 = t.ifd(ifd0.next, &mut v, &l).unwrap();
        let n = ifd1.entries.iter().position(|e| e.tag == tag).unwrap();
        let at = ifd1.offset as usize + 2 + 12 * n + 8;
        b[at..at + 2].copy_from_slice(&value.to_le_bytes());
        assert_eq!(exif::read(&b, &l).unwrap().strip_thumbnail, None, "{tag:#x}");
    }
}

// ---- Hostile structures ------------------------------------------------------------------------

/// The offset of entry `tag`'s value field in the IFD at `ifd` of a little-endian file.
fn value_at(b: &[u8], ifd: usize, tag: u16) -> usize {
    let n = u16::from_le_bytes([b[ifd], b[ifd + 1]]) as usize;
    (0..n).map(|k| ifd + 2 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == tag).unwrap() + 8
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn ifd0(b: &[u8]) -> usize {
    u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize
}

#[test]
fn ifd_loops_offsets_past_the_end_and_huge_counts_are_errors() {
    let l = Limits::default();
    let p = parts(1);
    let dng = raw(Kind::Dng, &p, Opts::default());
    let arw = raw(Kind::Arw, &p, Opts::default());
    let at0 = ifd0(&dng);
    let n0 = u16::from_le_bytes([dng[at0], dng[at0 + 1]]) as usize;
    let next_at = at0 + 2 + 12 * n0;
    let subs = u32::from_le_bytes(dng[value_at(&dng, at0, SUB_IFDS)..][..4].try_into().unwrap()) as usize;
    let cases: Vec<(&str, Vec<u8>, Error)> = vec![
        (
            "IFD0's next is IFD0",
            {
                let mut b = dng.clone();
                put(&mut b, next_at, at0 as u32);
                b
            },
            Error::Loop,
        ),
        (
            "a SubIFD is IFD0",
            {
                let mut b = dng.clone();
                put(&mut b, subs + 4, at0 as u32);
                b
            },
            Error::Loop,
        ),
        (
            "the EXIF IFD is IFD0",
            {
                let mut b = dng.clone();
                let a = value_at(&b, at0, EXIF_IFD);
                put(&mut b, a, at0 as u32);
                b
            },
            Error::Loop,
        ),
        (
            "two SubIFDs are one",
            {
                let mut b = dng.clone();
                let first = u32::from_le_bytes(b[subs..subs + 4].try_into().unwrap());
                put(&mut b, subs + 8, first);
                b
            },
            Error::Loop,
        ),
        (
            "IFD0 past the end",
            {
                let mut b = dng.clone();
                put(&mut b, 4, dng.len() as u32 + 8);
                b
            },
            Error::Truncated,
        ),
        (
            "IFD0 at u32::MAX",
            {
                let mut b = dng.clone();
                put(&mut b, 4, u32::MAX);
                b
            },
            Error::Truncated,
        ),
        (
            "a SubIFD past the end",
            {
                let mut b = dng.clone();
                put(&mut b, subs + 4, 0xffff_fff0);
                b
            },
            Error::Truncated,
        ),
        (
            "the preview past the end",
            {
                let mut b = arw.clone();
                let a = value_at(&b, ifd0(&arw), JPEG_OFFSET);
                put(&mut b, a, arw.len() as u32 - 10);
                b
            },
            Error::Truncated,
        ),
        (
            "a preview length past the end",
            {
                let mut b = arw.clone();
                let a = value_at(&b, ifd0(&arw), JPEG_LENGTH);
                put(&mut b, a, u32::MAX);
                b
            },
            Error::Truncated,
        ),
        (
            "u32::MAX SubIFDs",
            {
                let mut b = dng.clone();
                put(&mut b, value_at(&dng, at0, SUB_IFDS) - 4, u32::MAX);
                b
            },
            Error::TooMany("IFDs"),
        ),
        (
            "9000 entries in IFD0",
            {
                let mut b = dng.clone();
                b[at0..at0 + 2].copy_from_slice(&9000u16.to_le_bytes());
                b
            },
            Error::TooMany("IFD entries"),
        ),
    ];
    // Read as a raw directly: a broken IFD0 no longer sniffs as one.
    for (case, bytes, want) in cases {
        assert_eq!(formats::header(RAW, &bytes, &l).map(|_| ()), Err(want.clone()), "{case}");
        assert_eq!(decode_as(RAW, &bytes, &ON, &l).map(|_| ()), Err(want), "{case}");
        assert_eq!(formats::header(RAW, &bytes, &l).unwrap_err().reason(), "Damaged file", "{case}");
        let _ = read_header(&bytes, &l);
    }
    // The caps: IFDs per file and SubIFD depth.
    assert_eq!(read_header(&dng, &Limits { max_ifds: 4, ..l }).map(|_| ()), Err(Error::TooMany("IFDs")));
    assert_eq!(read_header(&dng, &Limits { max_ifd_depth: 1, ..l }).map(|_| ()), Err(Error::TooMany("IFD depth")));
    // A chain of SubIFDs deeper than the cap.
    let mut w = Writer::new(true, b"II*\0\0\0\0\0");
    let mut child = w.ifd(vec![(PHOTOMETRIC, V::Short(vec![32803]))], 0);
    for _ in 0..12 {
        child = w.ifd(vec![(SUB_IFDS, V::Long(vec![child])), (DNG_VERSION, V::Byte(vec![1, 4, 0, 0]))], 0);
    }
    let deep = w.finish(child);
    assert_eq!(sniff(&deep), Some(RAW));
    assert_eq!(read_header(&deep, &l).map(|_| ()), Err(Error::TooMany("IFD depth")));
}

/// Maker notes are vendor data: a loop or bad offset inside one only loses its preview.
#[test]
fn trouble_in_a_maker_note_only_loses_its_preview() {
    let l = Limits::default();
    let p = parts(1);
    for kind in [Kind::Nef, Kind::Orf, Kind::Pef] {
        let good = raw(kind, &p, Opts::default());
        let start = [&b"Nikon\0"[..], b"OLYMPUS\0", b"AOC\0"].iter().find_map(|m| good.windows(m.len()).position(|w| w == *m)).unwrap();
        // Each of the maker note's first 40 bytes inverted, one at a time.
        for k in 0..40 {
            let mut b = good.clone();
            b[start + k] ^= 0xff;
            let h = read_header(&b, &l);
            if kind == Kind::Nef {
                assert_eq!(h.map(|h| (h.width, h.height)), Ok((240, 160)), "{kind:?} byte {k}");
            } else {
                // The maker note held the big preview: the small thumbnail is shown instead.
                let (w, _) = h.map(|h| (h.width, h.height)).unwrap();
                assert!(w == 240 || w == 64, "{kind:?} byte {k}: {w}");
            }
        }
    }
}

/// Every prefix of every kind: `Ok` with the full size or `Err`, never a panic.
#[test]
fn truncation_at_every_offset_never_panics() {
    let l = Limits::default();
    let p = parts(6);
    for kind in KINDS {
        let bytes = raw(kind, &p, Opts { orientation: 6, ..Opts::default() });
        let full = read_header(&bytes, &l).unwrap();
        for len in 0..bytes.len() {
            let cut = &bytes[..len];
            if let Ok(h) = read_header(cut, &l) {
                assert_eq!(h.orientation, full.orientation, "{kind:?} prefix {len}");
            }
            if len % 97 == 0 {
                let _ = decode(cut, &l);
                let _ = formats::embedded_thumbnail(RAW, cut, &l);
            }
        }
    }
}

const HEADS: [&[u8]; 6] =
    [b"II*\0\x08\0\0\0", b"MM\0*\0\0\0\x08", b"II*\0\x10\0\0\0CR\x02\0", b"IIRO\x08\0\0\0", b"MMOR\0\0\0\x08", b"IIU\0\x08\0\0\0"];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Corruption anywhere in a synthetic raw, then truncation: never a panic.
    #[test]
    fn corrupted_raws_never_panic(kind in 0..KINDS.len(), cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12)) {
        let mut bytes = raw(KINDS[kind], &parts(1), Opts { color_space: Some(0xffff), r03: true, ..Opts::default() });
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1));
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = sniff(&bytes);
        let _ = read_header(&bytes, &l);
        let _ = decode_as(RAW, &bytes, &ON, &l);
        let _ = decode_thumbnail(&bytes, 64, &l);
        let _ = formats::embedded_thumbnail(RAW, &bytes, &l);
        if let Ok(e) = exif::read(&bytes, &l) && let Some(s) = e.strip_thumbnail {
            let _ = exif::strip_thumbnail_rgba(&bytes, &s, &ON, &l);
        }
    }

    /// Random bytes behind every raw header variant: never a panic.
    #[test]
    fn random_bytes_never_panic(head in 0..HEADS.len(), tail in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let bytes = [HEADS[head], &tail].concat();
        let l = Limits { max_alloc: 64 << 20, ..Limits::default() };
        let _ = sniff(&bytes);
        let _ = read_header(&bytes, &l);
        let _ = decode_as(RAW, &bytes, &OFF, &l);
        let _ = formats::embedded_thumbnail(RAW, &bytes, &l);
        let _ = exif::read(&bytes, &l);
    }
}

/// The preview's own EXIF (big-endian, orientation 8 here) does not decide the raw's orientation.
#[test]
fn orientation_is_ifd0_not_the_previews_own() {
    let mut p = parts(1);
    p.big = jpeg(&Scene::new(SEED, 240, 160), 92, Some(&Exif { orientation: 8, byte_order: ByteOrder::Big, ..Exif::default() })).unwrap();
    let h = read_header(&raw(Kind::Arw, &p, Opts::default()), &Limits::default()).unwrap();
    assert_eq!((h.orientation.get(), h.width, h.height), (1, 160, 240));
}

/// A JPEG whose EXIF block carries a strip thumbnail: the header's strip ranges count from the
/// file, and past 64 MP the thumbnail is made from the strips (the fast path).
#[test]
fn a_strip_thumbnail_is_the_fast_path_in_a_jpeg() {
    let l = Limits::default();
    let img = Scene::new(SEED, 32, 21).render().unwrap();
    let block = strip_block(true, &img, 4, false);
    let plain = jpeg(&Scene::new(SEED, 64, 42), 90, None).unwrap();
    let mut app1 = vec![0xff, 0xe1];
    app1.extend(((block.len() + 8) as u16).to_be_bytes());
    app1.extend(b"Exif\0\0");
    app1.extend(&block);
    let mut bytes = [&plain[..2], &app1, &plain[2..]].concat();
    let h = read_header(&bytes, &l).unwrap();
    assert_eq!(h.orientation.get(), 6);
    let s = h.exif.as_ref().unwrap().strip_thumbnail.clone().unwrap();
    assert_eq!(exif::strip_thumbnail_rgba(&bytes, &s, &ON, &l).unwrap(), rgba(&img));
    // The frame header now claims 9000 × 9000 (81 MP): the strips make the thumbnail.
    let sof = bytes.windows(2).rposition(|w| w == [0xff, 0xc0]).unwrap();
    bytes[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
    let t = decode_thumbnail(&bytes, 16, &l).unwrap();
    let expected =
        dalica_image::orient::apply(dalica_image::resize::fit(&rgba(&img), 16).unwrap(), dalica_image::Orientation::new(6).unwrap()).unwrap();
    assert_eq!(t, expected);
}

// ---- Real-camera layouts found by the corpus (M6.38) ---------------------------------------------

/// TIFF/EP's CFARepeatPatternDim and CFAPattern, and DNGPrivateData.
const CFA_REPEAT_PATTERN_DIM: u16 = 0x828d;
const CFA_PATTERN: u16 = 0x828e;
const DNG_PRIVATE_DATA: u16 = 0xc634;

/// Bytes past which a header read must read on (the first read takes 256 KiB).
const FIRST_READ: usize = 256 * 1024;

/// Writes `bytes` to a temporary file and reads its header the way scanning does.
fn header_from_file(name: &str, bytes: &[u8]) -> Result<dalica_image::Header, Error> {
    let dir = std::env::temp_dir().join(format!("dalica-image-raw-tiff-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.raw");
    std::fs::write(&path, bytes).unwrap();
    let result = dalica_image::read_header_file(&path, &Limits::default()).map(|(h, _)| h);
    std::fs::remove_dir_all(&dir).unwrap();
    result
}

/// A Samsung-style SRW: IFD0 (camera, EXIF) with SubIFDs for the preview (JPEG pair) and the raw
/// data, whose IFD has TIFF/EP's CFA pattern tags, a vendor compression and no photometric tag;
/// `pad` bytes before the SubIFDs put them past the first read.
fn srw(p: &Parts, pad: usize) -> Vec<u8> {
    let mut w = Writer::new(true, b"II*\0\0\0\0\0");
    let exif = exif_ifd(&mut w, &Opts::default(), None);
    let ifd0 = w.reserve(4);
    w.blob(&vec![0u8; pad]);
    let big = w.blob(&p.big);
    let preview =
        w.ifd(vec![(COMPRESSION, V::Short(vec![6])), (JPEG_OFFSET, V::Long(vec![big])), (JPEG_LENGTH, V::Long(vec![p.big.len() as u32]))], 0);
    let raw_at = w.blob(&[0x5a; 96]);
    let raw_ifd = w.ifd(
        vec![
            (NEW_SUBFILE_TYPE, V::Long(vec![0])),
            (WIDTH, V::Long(vec![4000])),
            (LENGTH, V::Long(vec![3000])),
            (BITS, V::Short(vec![12])),
            (COMPRESSION, V::Long(vec![32772])),
            (STRIP_OFFSETS, V::Long(vec![raw_at])),
            (STRIP_COUNTS, V::Long(vec![96])),
            (CFA_REPEAT_PATTERN_DIM, V::Short(vec![2, 2])),
            (CFA_PATTERN, V::Byte(vec![0, 1, 1, 2])),
        ],
        0,
    );
    w.fill(
        ifd0,
        vec![
            (MAKE, V::Ascii("SAMSUNG")),
            (ORIENTATION, V::Short(vec![1])),
            (SUB_IFDS, V::Long(vec![preview, raw_ifd])),
            (EXIF_IFD, V::Long(vec![exif])),
        ],
        0,
    );
    w.finish(ifd0)
}

/// A raw IFD known only by its CFA pattern tags is a raw, also when its sub-IFDs lie past the
/// first read (M6.38: both Samsung SRW files of the corpus sniffed as TIFF, then "Damaged file").
#[test]
fn an_srw_raw_ifd_with_cfa_pattern_tags_is_a_raw() {
    let l = Limits::default();
    let p = parts(1);
    let alone = decode(&p.big, &l).unwrap();
    let bytes = srw(&p, 0);
    assert_eq!(sniff(&bytes), Some(RAW));
    let h = read_header(&bytes, &l).unwrap();
    assert_eq!((h.format, h.width, h.height), (RAW, 240, 160));
    assert_eq!(decode(&bytes, &l).unwrap(), alone);
    // Without the CFA tags the same file is a TIFF without a page: damaged, as before.
    let mut plain = bytes.clone();
    for tag in [CFA_REPEAT_PATTERN_DIM, CFA_PATTERN] {
        let at = plain.windows(4).position(|w| w == [tag.to_le_bytes(), [if tag == CFA_PATTERN { 1 } else { 3 }, 0]].concat()).unwrap();
        plain[at..at + 2].copy_from_slice(&0x9999u16.to_le_bytes());
    }
    assert_eq!(sniff(&plain), Some(Format::Tiff));
    assert_eq!(read_header(&plain, &l).unwrap_err().reason(), "Damaged file");
    // Sub-IFDs past the first read: the start of the file asks for more, and the file reads whole.
    let far = srw(&p, FIRST_READ + 1000);
    assert_eq!(sniff(&far[..FIRST_READ]), Some(Format::Tiff));
    assert_eq!(read_header(&far[..FIRST_READ], &l).map(|_| ()), Err(Error::Truncated));
    let h = header_from_file("srw", &far).unwrap();
    assert_eq!((h.format, h.width, h.height), (RAW, 240, 160));
}

/// A Sony DSLR-A100-style ARW: IFD0 is the preview (JPEG pair) with DNGPrivateData, its SubIFDs
/// pointer aims at the raw data (read as an "IFD", hundreds of random entries), IFD1 is the
/// thumbnail.
fn a100(p: &Parts) -> Vec<u8> {
    let mut w = Writer::new(true, b"II*\0\0\0\0\0");
    let small = w.blob(&p.small);
    let ifd1 =
        w.ifd(vec![(COMPRESSION, V::Short(vec![6])), (JPEG_OFFSET, V::Long(vec![small])), (JPEG_LENGTH, V::Long(vec![p.small.len() as u32]))], 0);
    let private = w.blob(b"\0MRI\0\0\0\0");
    let big = w.blob(&p.big);
    let exif = exif_ifd(&mut w, &Opts::default(), None);
    // Compressed raw data: 600 "entries" out of tag order, one a SubIFDs pointer far past the end.
    let mut data = 600u16.to_le_bytes().to_vec();
    let mut x = 0x2545_f491u32;
    for k in 0..600u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let tag = if k == 1 { SUB_IFDS } else { (x >> 16) as u16 };
        data.extend(tag.to_le_bytes());
        data.extend(if k == 1 { 4u16 } else { (x & 0xffff) as u16 }.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(if k == 1 { 0xffff_fff0 } else { x.rotate_left(7) }.to_le_bytes());
    }
    data.extend([0x77; 64]);
    let raw_at = w.blob(&data);
    let ifd0 = w.ifd(
        vec![
            (NEW_SUBFILE_TYPE, V::Long(vec![1])),
            (WIDTH, V::Long(vec![3872])),
            (LENGTH, V::Long(vec![2592])),
            (COMPRESSION, V::Short(vec![6])),
            (MAKE, V::Ascii("SONY")),
            (ORIENTATION, V::Short(vec![1])),
            (SUB_IFDS, V::Long(vec![raw_at])),
            (JPEG_OFFSET, V::Long(vec![big])),
            (JPEG_LENGTH, V::Long(vec![p.big.len() as u32])),
            (EXIF_IFD, V::Long(vec![exif])),
            (DNG_PRIVATE_DATA, V::Byte(private.to_le_bytes().to_vec())),
        ],
        ifd1,
    );
    w.finish(ifd0)
}

/// DNGPrivateData marks a raw, and a SubIFDs pointer aimed at data is passed over (M6.38: the
/// corpus' Sony DSLR-A100 ARW sniffed as TIFF, then "Damaged file").
#[test]
fn an_arw_whose_sub_ifd_pointer_aims_at_raw_data_shows_its_preview() {
    let l = Limits::default();
    let p = parts(1);
    let bytes = a100(&p);
    assert_eq!(sniff(&bytes), Some(RAW));
    let h = read_header(&bytes, &l).unwrap();
    assert_eq!((h.format, h.width, h.height), (RAW, 240, 160));
    assert_eq!(h.thumbnail(&bytes), Some(&p.small[..]));
    assert_eq!(decode(&bytes, &l).unwrap(), decode(&p.big, &l).unwrap());
    assert_eq!(decode_thumbnail(&bytes, 60, &l).map(|t| (t.width, t.height)), Ok((60, 40)));
    // The walk that follows every pointer refuses the same structure.
    let t = dalica_image::ifd::Tiff::new(&bytes).unwrap();
    let walked = t.walk(t.first_ifd().unwrap(), &[SUB_IFDS, EXIF_IFD], &l, &mut dalica_image::ifd::Visited::default());
    assert!(walked.is_err(), "{walked:?}");
}

/// A raw whose EXIF maker note is the older Olympus layout ("OLYMP\0", a version, its IFD at 8,
/// offsets from the TIFF header): its ThumbnailImage tag holds the only JPEG.
fn old_orf(p: &Parts) -> Vec<u8> {
    let mut w = Writer::new(true, b"IIRS\0\0\0\0");
    let small = w.blob(&p.small);
    let mut note = b"OLYMP\0\x01\0".to_vec();
    note.extend(2u16.to_le_bytes());
    for (tag, kind, count, value) in [(0x0100u16, 7u16, p.small.len() as u32, small), (0x0201, 3, 1, 6)] {
        note.extend(tag.to_le_bytes());
        note.extend(kind.to_le_bytes());
        note.extend(count.to_le_bytes());
        note.extend(value.to_le_bytes());
    }
    note.extend(0u32.to_le_bytes());
    let exif = exif_ifd(&mut w, &Opts::default(), Some(note));
    let mut e = strip_entries(&mut w, &p.strip, 21);
    e.extend([(MAKE, V::Ascii("OLYMPUS CORPORATION")), (ORIENTATION, V::Short(vec![1])), (EXIF_IFD, V::Long(vec![exif]))]);
    let ifd0 = w.ifd(e, 0);
    w.finish(ifd0)
}

/// M6.38: the corpus' Olympus C5060WZ ORF answered "no preview"; its only JPEG is the older maker
/// note's ThumbnailImage.
#[test]
fn an_older_olympus_maker_note_offers_its_thumbnail_image() {
    let l = Limits::default();
    let p = parts(1);
    let bytes = old_orf(&p);
    assert_eq!(sniff(&bytes), Some(RAW));
    let h = read_header(&bytes, &l).unwrap();
    assert_eq!((h.width, h.height), (64, 43));
    assert_eq!(decode(&bytes, &l).unwrap(), decode(&p.small, &l).unwrap());
    // A thumbnail pointer past the end of the file only loses the preview.
    let mut b = bytes.clone();
    let at = b.windows(8).position(|w| w == b"OLYMP\0\x01\0").unwrap() + 8 + 2 + 8;
    put(&mut b, at, u32::MAX - 4);
    assert_eq!(read_header(&b, &l).map(|_| ()), Err(Error::Unsupported(RAW)));
}

/// An ORF/PEF whose raw strips (`raw` bytes) lie between the maker note and the preview, so that
/// the first read holds the maker note's start but not the preview: `olympus` puts the preview
/// inside the maker note (its extent passes the first read), else a Pentax maker note points at
/// it (only the pointer passes the first read).
fn far_preview(p: &Parts, olympus: bool, raw: usize) -> Vec<u8> {
    let mut w = Writer::new(true, if olympus { b"IIRO\0\0\0\0" } else { b"II*\0\0\0\0\0" });
    let note = if olympus {
        let mut n = Writer::new(true, b"OLYMPUS\0II\x03\0");
        let main = n.reserve(2);
        n.blob(&vec![0u8; raw]);
        let big = n.blob(&p.big);
        let cs = n.ifd(vec![(0x0100, V::Long(vec![1])), (0x0101, V::Long(vec![big])), (0x0102, V::Long(vec![p.big.len() as u32]))], 0);
        n.fill(main, vec![(0x0000, V::Undefined(b"0100".to_vec())), (0x2020, V::Ifd(vec![cs]))], 0);
        n.buf
    } else {
        let mut n = Writer::new(true, b"AOC\0II");
        n.ifd(vec![(0x0001, V::Short(vec![0])), (0x0003, V::Long(vec![p.big.len() as u32])), (0x0004, V::Long(vec![0]))], 0);
        n.buf
    };
    let exif = exif_ifd(&mut w, &Opts::default(), Some(note));
    let ifd0 = w.reserve(7);
    let raw_at = w.blob(&vec![0x5a; raw]);
    let big = if olympus { 0 } else { w.blob(&p.big) };
    w.fill(
        ifd0,
        vec![
            (WIDTH, V::Long(vec![4000])),
            (LENGTH, V::Long(vec![3000])),
            (PHOTOMETRIC, V::Short(vec![32803])),
            (STRIP_OFFSETS, V::Long(vec![raw_at])),
            (STRIP_COUNTS, V::Long(vec![raw as u32])),
            (MAKE, V::Ascii("Dalica")),
            (EXIF_IFD, V::Long(vec![exif])),
        ],
        0,
    );
    let mut b = w.finish(ifd0);
    if !olympus {
        // The Pentax preview start, counted from the TIFF header.
        let at = b.windows(6).position(|w| w == b"AOC\0II").unwrap() + 6 + 2 + 12 * 2 + 8;
        put(&mut b, at, big);
    }
    b
}

/// M6.38: the corpus' Olympus E-3 ORF decoded from the whole file, but a header read from its
/// first 256 KiB answered "no preview" instead of reading on.
#[test]
fn a_maker_note_preview_past_the_first_read_reads_on() {
    let l = Limits::default();
    let p = parts(1);
    let alone = decode(&p.big, &l).unwrap();
    for olympus in [true, false] {
        let bytes = far_preview(&p, olympus, FIRST_READ);
        assert!(bytes.len() > FIRST_READ + p.big.len());
        let h = read_header(&bytes, &l).unwrap();
        assert_eq!((h.width, h.height), (240, 160), "olympus {olympus}");
        assert_eq!(decode(&bytes, &l).unwrap(), alone, "olympus {olympus}");
        assert_eq!(read_header(&bytes[..FIRST_READ], &l).map(|_| ()), Err(Error::Truncated), "olympus {olympus}");
        let h = header_from_file(if olympus { "orf" } else { "pef" }, &bytes).unwrap();
        assert_eq!((h.width, h.height), (240, 160), "olympus {olympus}");
        // In a whole file a preview pointer past the end only loses the preview.
        let broken = if olympus {
            let mut b = bytes.clone();
            let length = b.windows(8).position(|w| w == [0x02, 0x01, 4, 0, 1, 0, 0, 0]).unwrap() + 8;
            put(&mut b, length, 0x7fff_0000);
            b
        } else {
            bytes[..bytes.len() - p.big.len()].to_vec()
        };
        assert_eq!(read_header(&broken, &l).map(|_| ()), Err(Error::Unsupported(RAW)), "olympus {olympus}");
    }
}
