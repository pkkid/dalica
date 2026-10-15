//! RAW previews from the non-TIFF containers (`formats.md` §2): CR3, RAF, CRW, MRW and X3F files
//! built here around `scenes` JPEGs, from the public structure descriptions the readers name.
//! Each yields its largest preview, upright, with the next smaller one as the thumbnail; hostile
//! structures are errors; truncation and random bytes never panic.

use crate::common::{assert_cells_close, cases, rgba};

use dalica_image::formats::{self, bmff, raw};
use dalica_image::{
    DecodeOptions, Error, Format, Limits, Orientation, RawKind, decode, decode_as, decode_thumbnail, orient, read_header, read_header_file, sniff,
};
use dalica_scenes::{Exif, Scene, jpeg};
use proptest::prelude::*;

// ---------------------------------------------------------------------------------------------
// Pictures and comparisons

/// A camera JPEG of `scene`, stored for orientation `o` and saying so in its EXIF (with an IFD1
/// thumbnail when `thumb` is set).
fn camera(scene: &Scene, o: u8, thumb: Option<u32>) -> Vec<u8> {
    let exif = Exif { orientation: o, thumbnail: thumb, make: Some("Dalica".into()), model: Some("Container 1".into()), ..Exif::default() };
    jpeg(scene, 92, Some(&exif)).unwrap()
}

/// The same JPEG with its EXIF segment made unrecognisable: pixels stored for orientation `o`,
/// but only the container says so (as raw previews usually are).
fn bare(scene: &Scene, o: u8) -> Vec<u8> {
    let mut b = camera(scene, o, None);
    let at = b.windows(6).position(|w| w == b"Exif\0\0").unwrap();
    b[at + 3] = b'z';
    b
}

fn plain(scene: &Scene) -> Vec<u8> {
    jpeg(scene, 92, None).unwrap()
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}

/// The byte range of `needle` inside `hay`.
fn range_of(hay: &[u8], needle: &[u8]) -> std::ops::Range<usize> {
    let at = find(hay, needle);
    at..at + needle.len()
}

// ---------------------------------------------------------------------------------------------
// Container builders (each from the structure its reader's provenance header describes)

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn uuid_box(id: &[u8; 16], payload: &[u8]) -> Vec<u8> {
    bx(b"uuid", &[&id[..], payload].concat())
}

/// A big-endian TIFF structure: one IFD0 at 8 with `entries` (tag, type, count, value field),
/// next IFD 0, then `tail`.
fn tiff_be(entries: &[(u16, u16, u32, u32)], tail: &[u8]) -> Vec<u8> {
    let mut b = b"MM\0*\0\0\0\x08".to_vec();
    b.extend((entries.len() as u16).to_be_bytes());
    for &(tag, kind, count, value) in entries {
        b.extend(tag.to_be_bytes());
        b.extend(kind.to_be_bytes());
        b.extend(count.to_be_bytes());
        b.extend(value.to_be_bytes());
    }
    b.extend(0u32.to_be_bytes());
    b.extend(tail);
    b
}

/// A SHORT value in a big-endian value field.
fn short(v: u16) -> u32 {
    u32::from(v) << 16
}

/// What goes into a CR3 file.
struct Cr3<'a> {
    /// CMT1's orientation.
    orientation: u16,
    thmb: Option<&'a [u8]>,
    prvw: Option<&'a [u8]>,
    /// The full-size JPEG of the first track.
    full: Option<&'a [u8]>,
    /// Use `stco` (32-bit offsets) instead of `co64`.
    stco: bool,
}

fn cr3(spec: &Cr3) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"crx \0\0\0\x01crx isom");
    let cmt1 = tiff_be(&[(0x0112, 3, 1, short(spec.orientation))], b"");
    let date = b"2026:06:04 19:42:10\0";
    let cmt2 = tiff_be(&[(0x9003, 2, date.len() as u32, 26)], date);
    let mut canon = [bx(b"CNCV", b"CanonCR3_001/00.09.00/00.00.00"), bx(b"CMT1", &cmt1), bx(b"CMT2", &cmt2)].concat();
    if let Some(t) = spec.thmb {
        let mut p = vec![0u8; 4];
        p.extend(160u16.to_be_bytes());
        p.extend(120u16.to_be_bytes());
        p.extend((t.len() as u32).to_be_bytes());
        p.extend([0, 1, 0, 0]);
        p.extend(t);
        canon.extend(bx(b"THMB", &p));
    }
    let preview = spec.prvw.map(|j| {
        let mut p = vec![0u8; 4];
        p.extend([0, 1]);
        p.extend(1620u16.to_be_bytes());
        p.extend(1080u16.to_be_bytes());
        p.extend([0, 1]);
        p.extend((j.len() as u32).to_be_bytes());
        p.extend(j);
        uuid_box(&raw::cr3::PREVIEW_UUID, &[&[0u8, 0, 0, 0, 0, 0, 0, 1][..], &bx(b"PRVW", &p)].concat())
    });
    let raw_data = vec![0x5au8; 300];
    let full_len = spec.full.map_or(0, <[u8]>::len);
    // A track: a CRAW sample entry (the 78 bytes of a visual sample entry, then its child box),
    // the first sample's size and offset.
    let trak = |child: &[u8; 4], size: usize, offset: u64| {
        let mut entry = vec![0u8; 78];
        entry.extend(bx(child, &[0, 0, 0, 0]));
        let stsd = bx(b"stsd", &[&[0u8, 0, 0, 0, 0, 0, 0, 1][..], &bx(b"CRAW", &entry)].concat());
        let mut stsz = vec![0u8; 8];
        stsz.extend(1u32.to_be_bytes());
        stsz.extend((size as u32).to_be_bytes());
        let offsets = if spec.stco {
            bx(b"stco", &[&[0u8, 0, 0, 0, 0, 0, 0, 1][..], &(offset as u32).to_be_bytes()].concat())
        } else {
            bx(b"co64", &[&[0u8, 0, 0, 0, 0, 0, 0, 1][..], &offset.to_be_bytes()].concat())
        };
        let stbl = bx(b"stbl", &[stsd, bx(b"stsz", &stsz), offsets].concat());
        bx(b"trak", &bx(b"mdia", &[bx(b"mdhd", &[0; 24]), bx(b"minf", &stbl)].concat()))
    };
    let moov = |base: u64| {
        let mut m = [uuid_box(&raw::cr3::CANON_UUID, &canon), bx(b"mvhd", &[0; 100])].concat();
        if spec.full.is_some() {
            m.extend(trak(b"JPEG", full_len, base));
        }
        m.extend(trak(b"CMP1", raw_data.len(), base + full_len as u64));
        bx(b"moov", &m)
    };
    let xmp = uuid_box(&[0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac], b"<x:xmpmeta/>");
    let head_len = ftyp.len() + moov(0).len() + xmp.len() + preview.as_ref().map_or(0, Vec::len) + 8;
    let mdat = bx(b"mdat", &[spec.full.unwrap_or_default(), &raw_data].concat());
    [ftyp, moov(head_len as u64), xmp, preview.unwrap_or_default(), mdat].concat()
}

/// A RAF file around `jpeg`.
fn raf(jpeg: &[u8]) -> Vec<u8> {
    let mut b = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
    let mut model = b"X-Dalica".to_vec();
    model.resize(32, 0);
    b.extend(model);
    b.extend(b"0100");
    b.extend([0u8; 20]);
    let at = 160u32;
    b.extend(at.to_be_bytes());
    b.extend((jpeg.len() as u32).to_be_bytes());
    b.extend([0u8; 16]); // CFA header and CFA pointers (not read)
    b.resize(at as usize, 0);
    b.extend(jpeg);
    b.extend([0x33u8; 200]);
    b
}

/// A little-endian CIFF heap: `records` (type, data) laid out, then the record table and its
/// offset. Records of a sub-heap kind hold a heap built the same way.
fn ciff_heap(records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut heap = Vec::new();
    let mut table = (records.len() as u16).to_le_bytes().to_vec();
    for (kind, data) in records {
        table.extend(kind.to_le_bytes());
        table.extend((data.len() as u32).to_le_bytes());
        table.extend((heap.len() as u32).to_le_bytes());
        heap.extend(data);
    }
    let table_at = heap.len() as u32;
    heap.extend(table);
    heap.extend(table_at.to_le_bytes());
    heap
}

/// A CRW file: the full-size JPEG and a thumbnail in the root heap, the image specification
/// (with `rotation`), make and model and the time stamp in a sub-heap.
fn crw(full: &[u8], thumb: Option<&[u8]>, rotation: i32) -> Vec<u8> {
    let mut spec = Vec::new();
    for v in [640u32, 480, 0x3f80_0000, rotation as u32, 12, 24, 1] {
        spec.extend(v.to_le_bytes());
    }
    let mut stamp = 1_780_602_130u32.to_le_bytes().to_vec(); // 2026-06-04 19:42:10
    stamp.extend([0u8; 8]);
    let props = ciff_heap(&[(0x1810, spec), (0x080a, b"Canon\0Canon EOS D30\0".to_vec()), (0x180e, stamp)]);
    let mut records = vec![(0x2007, full.to_vec()), (0x300a, props)];
    if let Some(t) = thumb {
        records.push((0x2008, t.to_vec()));
    }
    let mut b = b"II".to_vec();
    b.extend(26u32.to_le_bytes());
    b.extend(b"HEAPCCDR");
    b.extend(0x0001_0002u32.to_le_bytes());
    b.extend([0u8; 8]);
    b.extend(ciff_heap(&records));
    b
}

/// An MRW file: `\0PRD`, then `\0TTW` holding IFD0 (orientation, EXIF pointer), the EXIF IFD
/// with a maker note pointing at `preview`, and IFD1 pointing at `thumb`; then raw data.
fn mrw(orientation: u16, preview: &[u8], thumb: &[u8]) -> Vec<u8> {
    let (ifd0, exif, note, ifd1, data) = (8u32, 38u32, 56u32, 86u32, 116u32);
    let entry = |tag: u16, kind: u16, count: u32, value: u32| {
        let mut e = tag.to_be_bytes().to_vec();
        e.extend(kind.to_be_bytes());
        e.extend(count.to_be_bytes());
        e.extend(value.to_be_bytes());
        e
    };
    let mut t = b"MM\0*".to_vec();
    t.extend(ifd0.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    t.extend(entry(0x0112, 3, 1, short(orientation)));
    t.extend(entry(0x8769, 4, 1, exif));
    t.extend(ifd1.to_be_bytes());
    t.extend(1u16.to_be_bytes());
    t.extend(entry(0x927c, 7, 30, note));
    t.extend(0u32.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    t.extend(entry(0x0088, 4, 1, data + thumb.len() as u32));
    t.extend(entry(0x0089, 4, 1, preview.len() as u32));
    t.extend(0u32.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    t.extend(entry(0x0201, 4, 1, data));
    t.extend(entry(0x0202, 4, 1, thumb.len() as u32));
    t.extend(0u32.to_be_bytes());
    assert_eq!(t.len(), data as usize);
    t.extend(thumb);
    t.extend(preview);
    let block = |name: &[u8; 4], d: &[u8]| [&name[..], &(d.len() as u32).to_be_bytes(), d].concat();
    let blocks = [block(b"\0PRD", &[0u8; 24]), block(b"\0TTW", &t), block(b"\0WBG", &[0u8; 12])].concat();
    let mut b = b"\0MRM".to_vec();
    b.extend((blocks.len() as u32).to_be_bytes());
    b.extend(blocks);
    b.extend([0x44u8; 300]);
    b
}

/// An X3F file: a `SECi` image section per JPEG, a property section, then the directory.
fn x3f(jpegs: &[&[u8]]) -> Vec<u8> {
    let mut b = b"FOVb".to_vec();
    b.extend(0x0002_0002u32.to_le_bytes());
    b.resize(64, 0);
    let mut entries = Vec::new();
    for j in jpegs {
        let at = b.len() as u32;
        b.extend(b"SECi");
        for v in [0x0002_0000u32, 2, 18, 64, 48, 0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(*j);
        entries.push((at, b.len() as u32 - at, *b"IMA2"));
    }
    let at = b.len() as u32;
    b.extend(b"SECp\0\0\0\0props");
    entries.push((at, b.len() as u32 - at, *b"PROP"));
    let dir = b.len() as u32;
    b.extend(b"SECd");
    b.extend(0x0002_0000u32.to_le_bytes());
    b.extend((entries.len() as u32).to_le_bytes());
    for (offset, len, kind) in entries {
        b.extend(offset.to_le_bytes());
        b.extend(len.to_le_bytes());
        b.extend(kind);
    }
    b.extend(dir.to_le_bytes());
    b
}

// ---------------------------------------------------------------------------------------------
// Finding the preview

const L: Limits = Limits {
    max_side: 65_535,
    max_pixels: 1 << 30,
    max_alloc: 1536 * 1024 * 1024,
    max_ifds: 1024,
    max_ifd_depth: 10,
    max_ifd_entries: 8192,
    max_string: 64 * 1024,
    max_thumbnail: 64 * 1024 * 1024,
    max_segments: 4096,
    max_header_bytes: 64 * 1024 * 1024,
    max_icc: 16 * 1024 * 1024,
    max_icc_tags: 1000,
};

/// Every container sniffs as its kind, shows its largest preview exactly as the JPEG itself
/// decodes, and offers the next smaller one as the thumbnail.
#[test]
fn each_container_yields_its_largest_preview() {
    let big = Scene::new(31, 320, 212);
    let mid = Scene::new(31, 160, 106);
    let small = Scene::new(31, 80, 53);
    let (full, prvw, thmb) = (plain(&big), plain(&mid), plain(&small));
    let reference = decode(&full, &L).unwrap();
    assert_cells_close("raw-container-reference", &reference, &rgba(&big.render().unwrap()), 3.0);
    let files = [
        (RawKind::Cr3, cr3(&Cr3 { orientation: 1, thmb: Some(&thmb), prvw: Some(&prvw), full: Some(&full), stco: false }), Some(&prvw)),
        (RawKind::Cr3, cr3(&Cr3 { orientation: 1, thmb: Some(&thmb), prvw: Some(&prvw), full: Some(&full), stco: true }), Some(&prvw)),
        (RawKind::Raf, raf(&full), None),
        (RawKind::Crw, crw(&full, Some(&thmb), 0), Some(&thmb)),
        (RawKind::Mrw, mrw(1, &full, &thmb), Some(&thmb)),
        (RawKind::X3f, x3f(&[&thmb, &full, &prvw]), Some(&prvw)),
    ];
    for (kind, file, thumb) in files {
        let format = Format::Raw(kind);
        assert_eq!(sniff(&file), Some(format), "{kind:?}");
        assert!(format.supported(), "{kind:?}");
        let h = read_header(&file, &L).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation), (format, 320, 212, Orientation::UPRIGHT), "{kind:?}");
        assert_eq!(decode(&file, &L).unwrap(), reference, "{kind:?}");
        assert_eq!(decode_as(format, &file, &DecodeOptions::default(), &L).unwrap(), reference, "{kind:?}");
        if let Some(t) = thumb {
            assert_eq!(h.thumbnail, Some(range_of(&file, t)), "{kind:?}");
        }
        assert_eq!(formats::embedded_thumbnail(format, &file, &L).unwrap(), h.thumbnail, "{kind:?}");
        let small = decode_thumbnail(&file, 64, &L).unwrap();
        assert_eq!((small.width, small.height), (64, 42), "{kind:?}");
    }
}

/// RAF has one JPEG: its own EXIF gives the orientation, camera and thumbnail.
#[test]
fn raf_takes_everything_from_its_jpeg() {
    let scene = Scene::new(5, 200, 120);
    for o in 1..=8 {
        let jpg = camera(&scene, o, Some(48));
        let file = raf(&jpg);
        let h = read_header(&file, &L).unwrap();
        assert_eq!(h.orientation.get(), o);
        assert_eq!(h.display_size(), (200, 120));
        assert_eq!(h.exif.as_ref().and_then(|e| e.make.as_deref()), Some("Dalica"));
        let thumb = h.thumbnail(&file).unwrap();
        assert_eq!(read_header(thumb, &L).unwrap().format, Format::Jpeg);
        assert_eq!(decode(&file, &L).unwrap(), decode(&jpg, &L).unwrap(), "orientation {o}");
    }
}

/// CR3, MRW and CRW say the orientation in the container, not in the preview.
#[test]
fn container_orientation_turns_the_preview_upright() {
    let scene = Scene::new(9, 180, 120);
    let upright = rgba(&scene.render().unwrap());
    for o in 1..=8u8 {
        let stored = bare(&scene, o);
        let mut files = vec![
            ("cr3", cr3(&Cr3 { orientation: u16::from(o), thmb: None, prvw: None, full: Some(&stored), stco: false })),
            ("mrw", mrw(u16::from(o), &stored, &plain(&Scene::new(1, 16, 16)))),
        ];
        let rotation = match o {
            1 => Some(0),
            6 => Some(90),
            3 => Some(180),
            8 => Some(-90),
            _ => None,
        };
        if let Some(r) = rotation {
            files.push(("crw", crw(&stored, None, r)));
        }
        for (name, file) in files {
            let h = read_header(&file, &L).unwrap();
            assert_eq!(h.orientation.get(), o, "{name} {o}");
            assert_eq!(h.display_size(), (180, 120), "{name} {o}");
            let img = decode(&file, &L).unwrap();
            assert_cells_close(&format!("raw-{name}-orientation-{o}"), &img, &upright, 3.0);
        }
    }
}

#[test]
fn container_metadata_is_read() {
    let jpg = plain(&Scene::new(2, 64, 48));
    let h = read_header(&cr3(&Cr3 { orientation: 1, thmb: None, prvw: Some(&jpg), full: None, stco: false }), &L).unwrap();
    let e = h.exif.unwrap();
    assert_eq!(e.date_time_original.map(|d| d.to_string()).as_deref(), Some("2026-06-04T19:42:10"));
    let h = read_header(&crw(&jpg, None, 0), &L).unwrap();
    let e = h.exif.unwrap();
    assert_eq!((e.make.as_deref(), e.model.as_deref()), (Some("Canon"), Some("Canon EOS D30")));
    assert_eq!(e.date_time_original.map(|d| d.to_string()).as_deref(), Some("2026-06-04T19:42:10"));
}

/// A raw whose previews are missing or unreadable shows the generic RAW tile.
#[test]
fn no_usable_preview_is_the_generic_raw_tile() {
    // Both SOI bytes broken: an MRW JPEG with only its first byte wrong is read (M6.38).
    let mut not_jpeg = plain(&Scene::new(3, 32, 32));
    not_jpeg[0] = 0;
    not_jpeg[1] = 0;
    let files = [
        (RawKind::Cr3, cr3(&Cr3 { orientation: 1, thmb: None, prvw: None, full: None, stco: false })),
        (RawKind::Raf, raf(&not_jpeg)),
        (RawKind::Crw, crw(&not_jpeg, None, 0)),
        (RawKind::Mrw, mrw(1, &not_jpeg, &not_jpeg)),
        (RawKind::X3f, x3f(&[])),
    ];
    for (kind, file) in files {
        let err = read_header(&file, &L).unwrap_err();
        assert_eq!(err, Error::Unsupported(Format::Raw(kind)), "{kind:?}");
        assert_eq!(err.reason(), "This RAW file has no preview Dalica can show");
        assert_eq!(decode(&file, &L).unwrap_err(), err);
        assert_eq!(formats::embedded_thumbnail(Format::Raw(kind), &file, &L), Ok(None), "{kind:?}");
    }
}

/// MRW cameras store the maker note's JPEGs with a wrong first byte (seen as 0x00 and 0x02 where
/// the SOI marker's 0xFF belongs): they are read as JPEGs all the same, and the other containers
/// still refuse such bytes (M6.38: both MRW files of the corpus answered "no preview").
#[test]
fn an_mrw_preview_with_a_wrong_first_byte_is_shown() {
    let (full, thumb) = (plain(&Scene::new(8, 64, 48)), plain(&Scene::new(8, 16, 12)));
    let alone = decode(&full, &L).unwrap();
    for first in [0x00u8, 0x02, 0xd8] {
        let mut stored = full.clone();
        stored[0] = first;
        let b = mrw(1, &stored, &thumb);
        let h = read_header(&b, &L).unwrap();
        assert_eq!((h.format, h.width, h.height), (Format::Raw(RawKind::Mrw), 64, 48), "{first:#x}");
        // The thumbnail range is read from the file as it is: the IFD1 JPEG, never a repaired one.
        assert_eq!(h.thumbnail(&b), Some(&thumb[..]), "{first:#x}");
        assert_eq!(decode(&b, &L).unwrap(), alone, "{first:#x}");
        assert_eq!(decode_thumbnail(&b, 32, &L).map(|t| (t.width, t.height)), Ok((32, 24)), "{first:#x}");
        // Every prefix: never a panic, and a header only with the right size.
        for len in (0..b.len()).step_by(7) {
            if let Ok(h) = read_header(&b[..len], &L) {
                assert_eq!((h.width, h.height), (64, 48), "{first:#x} prefix {len}");
            }
            let _ = decode(&b[..len], &L);
        }
    }
    // The repaired preview's copy is charged to the decode's memory budget: the least memory the
    // intact file decodes in is too little for the repaired one, by the copy's size.
    let mut stored = full.clone();
    stored[0] = 0;
    let (intact, repaired) = (mrw(1, &full, &thumb), mrw(1, &stored, &thumb));
    let fits = |b: &[u8], max_alloc: u64| decode(b, &Limits { max_alloc, ..L }).is_ok();
    let (mut lo, mut hi) = (0u64, 64 << 20);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if fits(&intact, mid) { hi = mid } else { lo = mid + 1 }
    }
    assert!(!fits(&repaired, lo) && fits(&repaired, lo + stored.len() as u64));
    // A thumbnail with a wrong first byte is still the smaller preview, never the main one.
    let mut small = thumb.clone();
    small[0] = 0;
    assert_eq!(read_header(&mrw(1, &full, &small), &L).map(|h| (h.width, h.height)), Ok((64, 48)));
    // RAF does not repair: the same bytes are no preview there.
    assert_eq!(read_header(&raf(&stored), &L).unwrap_err(), Error::Unsupported(Format::Raw(RawKind::Raf)));
}

/// A header read from the first bytes reads on until the preview is inside them.
#[test]
fn partial_reads_read_on() {
    // Larger than the first 256 KiB a header read takes, so every container must read on.
    // (comment segments pad a small JPEG past it).
    let mut big = plain(&Scene::new(4, 240, 160));
    for _ in 0..5 {
        let mut com = vec![0xff, 0xfe];
        com.extend(65_002u16.to_be_bytes());
        com.extend([b'.'; 65_000]);
        big.splice(2..2, com);
    }
    assert!(big.len() > 300_000, "{}", big.len());
    let files = [
        ("cr3", cr3(&Cr3 { orientation: 1, thmb: None, prvw: None, full: Some(&big), stco: false })),
        ("raf", raf(&big)),
        ("crw", crw(&big, None, 0)),
        ("mrw", mrw(1, &big, &plain(&Scene::new(1, 16, 16)))),
        ("x3f", x3f(&[&big])),
    ];
    let dir = std::env::temp_dir().join(format!("dalica-raw-containers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, file) in files {
        let path = dir.join(format!("photo.{name}"));
        std::fs::write(&path, &file).unwrap();
        let (h, bytes) = read_header_file(&path, &L).unwrap();
        assert_eq!((h.width, h.height), (240, 160), "{name}");
        assert!(bytes.len() > 256 * 1024, "{name}");
        assert!(h.thumbnail.is_none_or(|t| t.end <= bytes.len()), "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Hostile structures

/// CR3: nesting past 12, boxes running past their parent or the file, too many boxes.
#[test]
fn cr3_box_loops_and_oversized_boxes_are_errors() {
    let jpg = plain(&Scene::new(6, 32, 24));
    let good = cr3(&Cr3 { orientation: 1, thmb: None, prvw: Some(&jpg), full: None, stco: false });
    assert!(read_header(&good, &L).is_ok());
    let ftyp = bx(b"ftyp", b"crx \0\0\0\x01crx isom");
    // moov inside moov inside moov…: the walk stops at the nesting cap, it never recurses on.
    let mut nested = bx(b"trak", b"");
    for _ in 0..20 {
        nested = bx(b"mdia", &nested);
    }
    let deep = [ftyp.clone(), bx(b"moov", &bx(b"trak", &nested))].concat();
    assert!(read_header(&deep, &L).is_err());
    // A box whose size never advances the walk.
    let mut stuck = ftyp.clone();
    stuck.extend(4u32.to_be_bytes());
    stuck.extend(b"moov");
    stuck.extend([0u8; 32]);
    assert!(matches!(read_header(&stuck, &L), Err(Error::Malformed(_))));
    // moov claiming more than the file holds: truncated (a partial read reads on).
    let mut over = good.clone();
    let moov = find(&over, b"moov") - 4;
    over[moov..moov + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(read_header(&over, &L), Err(Error::Truncated));
    // A PRVW whose JPEG length runs past the file.
    let mut long = good.clone();
    let prvw = find(&long, b"PRVW") + 4 + 12;
    long[prvw..prvw + 4].copy_from_slice(&1_000_000u32.to_be_bytes());
    assert_eq!(read_header(&long, &L), Err(Error::Truncated));
    // More boxes in one level than the cap.
    let many = [ftyp, bx(b"moov", &bx(b"free", b"").repeat(bmff::MAX_BOXES + 1))].concat();
    assert!(matches!(read_header(&many, &L), Err(Error::TooMany(_))));
}

#[test]
fn crw_heap_loops_and_bad_tables_are_errors() {
    let jpg = plain(&Scene::new(6, 32, 24));
    // A sub-heap whose only record is a sub-heap covering itself again: stopped by the depth cap.
    let mut b = b"II".to_vec();
    b.extend(26u32.to_le_bytes());
    b.extend(b"HEAPCCDR");
    b.extend([0u8; 12]);
    let heap_len = 2 + 10 + 4;
    // Root heap: one record of kind 0x300a covering the whole heap minus its trailing offset.
    b.extend(1u16.to_le_bytes());
    b.extend(0x300au16.to_le_bytes());
    b.extend((heap_len as u32 - 4).to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    assert_eq!(read_header(&b, &L).unwrap_err().reason(), "Damaged file");
    // A record table pointing past the file, and a record outside its heap.
    let mut bad = crw(&jpg, None, 0);
    let len = bad.len();
    bad[len - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(read_header(&bad, &L).is_err());
    let mut outside = crw(&jpg, None, 0);
    let table = len - 4 - (2 + 2 * 10);
    outside[table + 2 + 6..table + 2 + 10].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
    assert!(matches!(read_header(&outside, &L), Err(Error::Malformed(_))));
}

#[test]
fn mrw_ifd_loops_are_survived_or_refused() {
    let (full, thumb) = (plain(&Scene::new(7, 64, 48)), plain(&Scene::new(7, 16, 12)));
    // The maker note pointing back at IFD0: a loop; the IFD1 thumbnail still shows.
    let mut b = mrw(1, &full, &thumb);
    let ttw = find(&b, b"\0TTW") + 8;
    let note_field = ttw + 38 + 2 + 8;
    b[note_field..note_field + 4].copy_from_slice(&8u32.to_be_bytes());
    let h = read_header(&b, &L).unwrap();
    assert_eq!((h.width, h.height), (16, 12));
    // IFD0's next pointing at itself: EXIF is refused (upright), previews found through the maker note.
    let mut b = mrw(6, &full, &thumb);
    let next = ttw + 8 + 2 + 24;
    b[next..next + 4].copy_from_slice(&8u32.to_be_bytes());
    let h = read_header(&b, &L).unwrap();
    assert_eq!((h.width, h.height, h.orientation), (64, 48, Orientation::UPRIGHT));
    assert_eq!(h.exif_error, Some(Error::Loop));
    // A block length past the header, and a header past the file.
    let mut b = mrw(1, &full, &thumb);
    b[ttw - 4..ttw].copy_from_slice(&0x00ff_ffffu32.to_be_bytes());
    assert!(matches!(read_header(&b, &L), Err(Error::Malformed(_))));
    let mut b = mrw(1, &full, &thumb);
    b[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(read_header(&b, &L), Err(Error::Truncated));
}

#[test]
fn x3f_and_raf_hostile_directories() {
    let jpg = plain(&Scene::new(8, 32, 24));
    let mut b = x3f(&[&jpg]);
    let dir = u32::from_le_bytes(b[b.len() - 4..].try_into().unwrap()) as usize;
    b[dir + 8..dir + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read_header(&b, &L), Err(Error::TooMany("X3F directory entries")));
    let mut b = x3f(&[&jpg]);
    b[dir + 12 + 4..dir + 12 + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    // A section claiming more than the preview limit is passed over: the generic RAW tile.
    assert_eq!(read_header(&b, &L), Err(Error::Unsupported(Format::Raw(RawKind::X3f))));
    let mut b = x3f(&[&jpg]);
    b[dir + 12 + 4..dir + 12 + 8].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert_eq!(read_header(&b, &L), Err(Error::Truncated));
    let mut b = x3f(&[&jpg]);
    b[64..68].copy_from_slice(b"SECx");
    assert!(matches!(read_header(&b, &L), Err(Error::Malformed(_))));
    // RAF: the JPEG pointer past the end, and a length that overflows nothing.
    let mut b = raf(&jpg);
    b[84..88].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(read_header(&b, &L), Err(Error::Truncated));
    // A preview larger than the thumbnail limit is passed over: no preview.
    let small = Limits { max_thumbnail: 100, ..Limits::default() };
    assert_eq!(read_header(&raf(&jpg), &small), Err(Error::Unsupported(Format::Raw(RawKind::Raf))));
}

// ---------------------------------------------------------------------------------------------
// Never a panic

fn sample_files() -> Vec<Vec<u8>> {
    let (a, b) = (camera(&Scene::new(12, 40, 30), 6, Some(16)), plain(&Scene::new(12, 20, 15)));
    vec![
        cr3(&Cr3 { orientation: 6, thmb: Some(&b), prvw: Some(&a), full: Some(&a), stco: false }),
        raf(&a),
        crw(&a, Some(&b), 90),
        mrw(3, &a, &b),
        x3f(&[&b, &a]),
    ]
}

/// Every prefix: `Ok` with the full file's size, or `Err`; never a panic.
#[test]
fn truncation_at_every_offset_never_panics() {
    for file in sample_files() {
        let full = read_header(&file, &L).unwrap();
        for len in 0..file.len() {
            let cut = &file[..len];
            if let Ok(h) = read_header(cut, &L) {
                assert_eq!((h.width, h.height), (full.width, full.height), "{:?} prefix {len}", full.format);
            }
            if let Some(f) = sniff(cut) {
                let _ = formats::embedded_thumbnail(f, cut, &L);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn corrupted_containers_never_panic(which in 0usize..5, cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = sample_files().swap_remove(which);
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1));
        let _ = read_header(&bytes, &L);
        let _ = decode(&bytes, &L);
        for kind in RawKind::ALL {
            let _ = decode_as(Format::Raw(kind), &bytes, &DecodeOptions::default(), &L);
            let _ = formats::embedded_thumbnail(Format::Raw(kind), &bytes, &L);
        }
    }
}

#[test]
fn orient_is_applied_once() {
    // The decoded value carries the raw's orientation; `decode` applies it exactly once.
    let scene = Scene::new(10, 60, 40);
    let stored = bare(&scene, 6);
    let file = cr3(&Cr3 { orientation: 6, thmb: None, prvw: None, full: Some(&stored), stco: false });
    let as_stored = decode(&stored, &L).unwrap();
    assert_eq!(decode(&file, &L).unwrap(), orient::apply(as_stored, Orientation::new(6).unwrap()).unwrap());
}

/// Lead-approved follow-up for M5.6: `decode_thumbnail` of a TIFF uses its reduced-resolution
/// page. The main page's strips are wiped, so only the thumbnail page can give the right picture.
#[test]
fn decode_thumbnail_uses_a_tiff_thumbnail_page() {
    use dalica_image::ifd::{Tiff as TiffReader, Visited};
    use dalica_scenes::formats::{Tiff, tiff};
    let scene = Scene::new(14, 400, 300);
    let mut bytes = tiff(&scene, &Tiff { thumbnail: Some(64), orientation: 6, ..Tiff::default() }).unwrap();
    assert_eq!(sniff(&bytes), Some(Format::Tiff));
    let full = decode(&bytes, &L).unwrap();
    let original = bytes.clone();
    let (strips, ifd1) = {
        let t = TiffReader::new(&bytes).unwrap();
        let ifd0 = t.ifd(t.first_ifd().unwrap(), &mut Visited::default(), &L).unwrap();
        (t.uint(ifd0.get(273).unwrap()).unwrap() as usize, ifd0.next as usize)
    };
    assert!(strips < ifd1);
    bytes[strips..ifd1].fill(0);
    let t = decode_thumbnail(&bytes, 48, &L).unwrap();
    assert_eq!((t.width, t.height), (48, 36));
    let expected = dalica_image::resize::fit(&dalica_image::resize::fit(&full, 64).unwrap(), 48).unwrap();
    assert_cells_close("tiff-thumbnail-page-hook", &t, &expected, 4.0);
    // Without the thumbnail page's help (a target larger than it), the wiped main page shows.
    assert_ne!(decode_thumbnail(&bytes, 200, &L).ok(), decode_thumbnail(&original, 200, &L).ok());
    assert_eq!(decode_thumbnail(&original, 200, &L).map(|t| (t.width, t.height)), Ok((200, 150)));
}
