//! Metadata carried to a new file (`data-model.md` §6, `creating.md` §2): a JPEG, a TIFF and a
//! WebP source each give a JPEG and a PNG target whose EXIF, XMP and IPTC read back with
//! orientation 1 and the new size, foreign properties byte for byte; maker notes still parse (in
//! place, and packed when a TIFF's strips come first); GPS gone from EXIF and XMP when asked and
//! kept otherwise; the owned fields merged; the thumbnail replaced or dropped; `set_orientation`
//! changes only that tag's value; truncated and hostile input is `Err`, never a panic.

mod build;

use build::*;
use dalica_image::ifd::{Tiff as Ifds, Visited};
use dalica_meta::carry::exif::MAX_JPEG_EXIF;
use dalica_meta::carry::{Carried, Carry, carry, set_orientation};
use dalica_meta::container::jpeg::extended_guid;
use dalica_meta::container::payload_digest;
use dalica_meta::exif::IfdKind;
use dalica_meta::exif::read::walk;
use dalica_meta::iptc::read::{datasets, resources};
use dalica_meta::{Changes, DateTaken, Error, Limits, Place, WriteOptions, read, scan};
use proptest::prelude::*;

fn l() -> Limits {
    Limits::default()
}

/// The copy's size: the source's 6000 × 4000, turned a quarter and halved.
const W: u32 = 2000;
const H: u32 = 3000;

const MAKER_NOTE: u16 = 0x927c;
const FOREIGN_TAG: u16 = 0xabcd;
const FOREIGN_VALUE: &[u8] = b"a foreign EXIF value, kept byte for byte";
const OLD_THUMB: &[u8] = b"\xff\xd8 the source's old thumbnail \xff\xd9";
const NEW_THUMB: &[u8] = b"\xff\xd8 a fresh thumbnail of the new pixels \xff\xd9";
const FOREIGN_XMP: &str = "<foo:Bar><rdf:Seq><rdf:li>kept one</rdf:li><rdf:li>kept &amp; two</rdf:li></rdf:Seq></foo:Bar>";
const FOREIGN_DATASET: &[u8] = b"a foreign IPTC dataset";
const FOREIGN_RESOURCE: &[u8] = b"resolution info, kept";

/// A camera's EXIF: orientation 6, Adobe RGB through the interoperability index, a foreign tag, a
/// GPS position and a thumbnail.
fn camera(big_endian: bool, note: &[u8]) -> Tiff {
    Tiff {
        big_endian,
        ifd0: vec![
            ascii(0x010f, "Dalicam"),
            ascii(0x0110, "Model 7"),
            short(0x0112, 6),
            rationals(0x011a, &[(300, 1)]),
            rationals(0x011b, &[(300, 1)]),
            short(0x0128, 2),
            ascii(0x0131, "Firmware 1.0"),
            ascii(0x8298, "(c) Ana Costa"),
        ],
        exif: vec![
            rationals(0x829a, &[(1, 250)]),
            rationals(0x829d, &[(28, 10)]),
            short(0x8827, 200),
            ascii(0x9003, "2024:05:06 07:08:09"),
            short(0xa001, 0xffff),
            long(0xa002, 6000),
            long(0xa003, 4000),
            undefined(FOREIGN_TAG, FOREIGN_VALUE),
            undefined(MAKER_NOTE, note),
        ],
        gps: vec![
            bytes(0x0000, &[2, 3, 0, 0]),
            ascii(0x0001, "N"),
            rationals(0x0002, &[(38, 1), (42, 1), (30, 1)]),
            ascii(0x0003, "W"),
            rationals(0x0004, &[(9, 1), (8, 1), (12, 1)]),
            rationals(0x0006, &[(120, 1)]),
        ],
        interop: vec![ascii(0x0001, "R03")],
        ifd1: vec![short(0x0103, 6), F::OffsetTo { tag: 0x0201, bytes: OLD_THUMB.to_vec() }, long(0x0202, OLD_THUMB.len() as u32)],
    }
}

const NOTE_LEN: usize = 2 + 12 * 2 + 4 + 16 + 8;

/// A maker note that is an IFD at `at` whose two values sit inside it at offsets counted from the
/// TIFF header, as several vendors write them.
fn note_at(at: usize, big_endian: bool) -> Vec<u8> {
    let u16b = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let u32b = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let data = (at + 2 + 24 + 4) as u32;
    let mut note = Vec::new();
    note.extend(u16b(2));
    note.extend(u16b(0x0001));
    note.extend(u16b(ASCII));
    note.extend(u32b(16));
    note.extend(u32b(data));
    note.extend(u16b(0x0002));
    note.extend(u16b(RATIONAL));
    note.extend(u32b(1));
    note.extend(u32b(data + 16));
    note.extend(u32b(0));
    note.extend(b"vendor firmware\0");
    note.extend(u32b(7));
    note.extend(u32b(3));
    assert_eq!(note.len(), NOTE_LEN);
    note
}

/// The structure `make` builds around a maker note with absolute offsets, and where the note is.
fn noted(big_endian: bool, make: impl Fn(Vec<u8>) -> Vec<u8>) -> (Vec<u8>, usize) {
    let probe = make(vec![0xa7; NOTE_LEN]);
    let at = probe.windows(NOTE_LEN).position(|w| w.iter().all(|&b| b == 0xa7)).unwrap();
    let block = make(note_at(at, big_endian));
    assert_eq!(&block[at..at + NOTE_LEN], &note_at(at, big_endian)[..]);
    (block, at)
}

/// The maker note's IFD read at `at`, its values read at their absolute offsets.
fn note_values(block: &[u8], at: usize) -> Vec<Vec<u8>> {
    let t = Ifds::new(block).unwrap();
    let ifd = t.ifd(at as u32, &mut Visited::default(), &dalica_image::Limits::default()).unwrap();
    ifd.entries.iter().map(|e| t.value(e).unwrap().unwrap().to_vec()).collect()
}

/// Where the maker note of an EXIF block is.
fn note_offset(block: &[u8]) -> usize {
    let w = walk(block, &l()).unwrap();
    w.tiff.value_range(w.get(IfdKind::Exif, MAKER_NOTE).unwrap()).unwrap().unwrap().start
}

/// A whole packet around top-level descriptions, with 200 bytes of padding.
fn wrapped(descriptions: &str) -> Vec<u8> {
    format!(
        concat!(
            "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n",
            "{}\n</rdf:RDF></x:xmpmeta>\n{}\n<?xpacket end=\"w\"?>"
        ),
        descriptions,
        " ".repeat(200)
    )
    .into_bytes()
}

fn packet(extended_guid: Option<&str>) -> Vec<u8> {
    let ext = extended_guid.map_or(String::new(), |g| format!(" xmpNote:HasExtendedXMP=\"{g}\""));
    wrapped(&format!(
        concat!(
            " <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\"",
            " xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\" xmlns:foo=\"http://example.com/foo/\"",
            " xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\"",
            " tiff:Orientation=\"6\" tiff:ImageWidth=\"6000\" exif:GPSLatitude=\"38,42.5N\" exif:GPSLongitude=\"9,8.2W\" photoshop:City=\"Lisbon\"{}>\n",
            "  <exif:PixelXDimension>6000</exif:PixelXDimension>\n",
            "  <exif:PixelYDimension>4000</exif:PixelYDimension>\n",
            "  <exif:GPSAltitude>120/1</exif:GPSAltitude>\n",
            "  {}\n",
            " </rdf:Description>"
        ),
        ext, FOREIGN_XMP
    ))
}

fn stream() -> Vec<u8> {
    iim(&[(1, 90, b"\x1b%G"), (2, 120, b"Source caption"), (2, 25, b"tag one"), (2, 25, b"tag two"), (2, 90, b"Lisbon"), (2, 200, FOREIGN_DATASET)])
}

fn photoshop() -> Vec<u8> {
    irb(&[(0x03ed, FOREIGN_RESOURCE), (0x0404, &stream()), (0x040c, b"Photoshop's old thumbnail")])
}

/// A camera JPEG: EXIF with a maker note, XMP, IPTC in APP13 and an ICC profile.
fn jpeg_source(big_endian: bool) -> (Vec<u8>, usize) {
    let (block, at) = noted(big_endian, |note| camera(big_endian, &note).build());
    (jpeg(&[app1_exif(&block), app1_xmp(&packet(None)), app13(&photoshop()), app2_icc(b"an Adobe RGB profile")]), at)
}

/// A TIFF file with the same EXIF in its own IFDs, XMP in tag 700 and IPTC in tag 33723.
fn tiff_source(big_endian: bool) -> (Vec<u8>, usize) {
    noted(big_endian, |note| {
        let mut t = camera(big_endian, &note);
        t.ifd1.clear();
        t.ifd0.extend([bytes(0x02bc, &packet(None)), undefined(0x83bb, &stream())]);
        tiff_file(t)
    })
}

/// An extended WebP with EXIF and XMP.
fn webp_source(big_endian: bool) -> (Vec<u8>, usize) {
    let (block, at) = noted(big_endian, |note| camera(big_endian, &note).build());
    (webp(&[(b"EXIF", block), (b"XMP ", packet(None))]), at)
}

/// What Dalica's encoder writes: a JPEG with an ICC profile, or a PNG with one.
fn jpeg_target() -> Vec<u8> {
    jpeg(&[app2_icc(b"an sRGB profile")])
}

fn png_target() -> Vec<u8> {
    png(&[(b"iCCP", b"sRGB\0\0compressed profile".to_vec())])
}

fn spec() -> Carry<'static> {
    Carry { width: W, height: H, thumbnail: None, remove_location: false, changes: Changes::default(), options: WriteOptions::default() }
}

fn exif_of(file: &[u8]) -> Vec<u8> {
    scan(file, &l()).unwrap().exif.unwrap().into_owned()
}

fn xmp_of(file: &[u8]) -> String {
    String::from_utf8(scan(file, &l()).unwrap().xmp.unwrap().into_owned()).unwrap()
}

/// The value of `tag` in the first IFD of `kind`.
fn value(block: &[u8], kind: IfdKind, tag: u16) -> Option<Vec<u8>> {
    let w = walk(block, &l()).unwrap();
    w.get(kind, tag).map(|e| w.tiff.value(e).unwrap().unwrap().into_owned())
}

fn uint(block: &[u8], kind: IfdKind, tag: u16) -> Option<u32> {
    let w = walk(block, &l()).unwrap();
    w.get(kind, tag).and_then(|e| w.tiff.uint(e))
}

/// Everything the done-when asks of one carried file.
fn check_carried(name: &str, source: &[u8], note: usize, out: &Carried, target: &[u8], removed: bool) {
    let m = read(&out.bytes, &l()).unwrap();
    assert!(m.problems.is_empty(), "{name}: {:?}", m.problems);
    // The target's own bytes (image data, ICC profile) are all still there.
    let (ts, os) = (scan(target, &l()).unwrap(), scan(&out.bytes, &l()).unwrap());
    assert_eq!(payload_digest(target, &ts.spans), payload_digest(&out.bytes, &os.spans), "{name}: target bytes");

    // EXIF: orientation 1, the new size, sRGB, foreign values and the maker note as they were.
    let block = exif_of(&out.bytes);
    let src = exif_of(source);
    assert_eq!(m.summary.orientation, Some(1), "{name}");
    assert_eq!(uint(&block, IfdKind::Exif, 0xa002), Some(W), "{name}");
    assert_eq!(uint(&block, IfdKind::Exif, 0xa003), Some(H), "{name}");
    assert_eq!(uint(&block, IfdKind::Exif, 0xa001), Some(1), "{name}");
    assert_eq!(value(&block, IfdKind::Interop, 0x0001).as_deref(), Some(&b"R98\0"[..]), "{name}");
    for (kind, tag) in
        [(IfdKind::Image, 0x010f), (IfdKind::Image, 0x8298), (IfdKind::Exif, 0x829a), (IfdKind::Exif, FOREIGN_TAG), (IfdKind::Exif, 0x9003)]
    {
        assert_eq!(value(&block, kind, tag), value(&src, kind, tag), "{name}: {tag:#x}");
    }
    assert_eq!(m.summary.make.as_deref(), Some("Dalicam"));
    assert_eq!(note_offset(&block), note, "{name}: the maker note never moves");
    assert_eq!(note_values(&block, note), note_values(&src, note), "{name}: the maker note");
    for tag in [0x0111, 0x0117, 0x0103, 0x02bc, 0x83bb] {
        assert_eq!(value(&block, IfdKind::Image, tag), None, "{name}: {tag:#x} left out");
    }
    assert!(!block.windows(PIXELS.len()).any(|w| w == PIXELS), "{name}: no pixels in the EXIF");
    assert!(!block.windows(OLD_THUMB.len()).any(|w| w == OLD_THUMB), "{name}: no old thumbnail");
    for other in [&b"Source caption"[..], b"xmpmeta"] {
        assert!(!block.windows(other.len()).any(|w| w == other), "{name}: the other blocks are not in the EXIF");
    }

    // XMP: orientation 1 and the new size; foreign properties byte for byte.
    let x = xmp_of(&out.bytes);
    assert!(x.contains("tiff:Orientation=\"1\""), "{name}");
    assert!(x.contains(&format!("tiff:ImageWidth=\"{W}\"")), "{name}");
    assert!(x.contains(&format!("<exif:PixelXDimension>{W}</exif:PixelXDimension>")), "{name}");
    assert!(x.contains(&format!("<exif:PixelYDimension>{H}</exif:PixelYDimension>")), "{name}");
    assert!(x.contains(FOREIGN_XMP), "{name}");
    assert!(x.contains("photoshop:City=\"Lisbon\""), "{name}: place names stay");

    // IPTC: every dataset byte for byte (a WebP holds none).
    let s = scan(&out.bytes, &l()).unwrap();
    match s.iptc_stream(&l()).unwrap() {
        Some(iptc) => {
            assert_eq!(iptc, &stream()[..], "{name}: IPTC");
            assert_eq!(m.sources.iptc.caption.as_deref(), Some("Source caption"));
            assert!(datasets(iptc, &l()).unwrap().iter().any(|d| (d.record, d.number, d.data) == (2, 200, FOREIGN_DATASET)));
        }
        None => assert!(name.starts_with("WebP"), "{name}: IPTC"),
    }

    // GPS: kept, or gone from EXIF and XMP.
    let w = walk(&block, &l()).unwrap();
    let gps = w.ifds.iter().any(|(k, _)| *k == IfdKind::Gps);
    if removed {
        assert!(!gps, "{name}: GPS IFD");
        assert!(!x.contains("GPS"), "{name}: GPS in XMP");
        assert_eq!(m.owned.place, None);
    } else {
        assert!(gps, "{name}");
        assert_eq!(value(&block, IfdKind::Gps, 0x0006), value(&src, IfdKind::Gps, 0x0006));
        assert!(x.contains("exif:GPSLatitude=\"38,42.5N\"") && x.contains("<exif:GPSAltitude>120/1</exif:GPSAltitude>"), "{name}");
        let p = m.owned.place.unwrap();
        assert!((p.latitude() - (38.0 + 42.0 / 60.0 + 30.0 / 3600.0)).abs() < 1e-6, "{name}");
    }
}

type Source = fn(bool) -> (Vec<u8>, usize);

const SOURCES: [(&str, Source); 3] = [("JPEG", jpeg_source), ("TIFF", tiff_source), ("WebP", webp_source)];

#[test]
fn each_source_gives_a_jpeg_and_a_png() {
    for (name, make) in SOURCES {
        for big_endian in [false, true] {
            let (source, note) = make(big_endian);
            for (tname, target) in [("JPEG", jpeg_target()), ("PNG", png_target())] {
                for removed in [false, true] {
                    let name = format!("{name} → {tname}, {}, remove location {removed}", if big_endian { "MM" } else { "II" });
                    let out = carry(&source, &target, &Carry { remove_location: removed, ..spec() }).unwrap();
                    assert!(!out.thumbnail && out.dropped.is_empty(), "{name}");
                    check_carried(&name, &source, note, &out, &target, removed);
                    // The new blocks are where readers look for them.
                    if tname == "JPEG" {
                        let o = &out.bytes;
                        assert_eq!(&o[..4], b"\xff\xd8\xff\xe0", "{name}: SOI, then APP0");
                        let app1 = 2 + 2 + usize::from(u16::from_be_bytes([o[4], o[5]]));
                        assert_eq!(&o[app1..app1 + 2], b"\xff\xe1", "{name}: EXIF right after APP0");
                        assert_eq!(&o[app1 + 4..app1 + 10], b"Exif\0\0");
                    } else {
                        let idat = out.bytes.windows(4).position(|w| w == b"IDAT").unwrap();
                        let exif = out.bytes.windows(4).position(|w| w == b"eXIf").unwrap();
                        assert!(exif < idat, "{name}: before the image data");
                    }
                }
            }
        }
    }
}

#[test]
fn the_thumbnail_is_replaced_or_dropped() {
    let (source, _) = jpeg_source(false);
    for target in [jpeg_target(), png_target()] {
        let out = carry(&source, &target, &Carry { thumbnail: Some(NEW_THUMB), ..spec() }).unwrap();
        assert!(out.thumbnail);
        let block = exif_of(&out.bytes);
        let at = uint(&block, IfdKind::Thumbnail, 0x0201).unwrap() as usize;
        let len = uint(&block, IfdKind::Thumbnail, 0x0202).unwrap() as usize;
        assert_eq!(&block[at..at + len], NEW_THUMB);
        assert_eq!(uint(&block, IfdKind::Thumbnail, 0x0103), Some(6));

        let none = carry(&source, &target, &spec()).unwrap();
        let block = exif_of(&none.bytes);
        assert!(walk(&block, &l()).unwrap().ifds.iter().all(|(k, _)| *k != IfdKind::Thumbnail));
    }
    // An independent reader finds the new thumbnail too.
    let out = carry(&source, &jpeg_target(), &Carry { thumbnail: Some(NEW_THUMB), ..spec() }).unwrap();
    let header = dalica_image::read_header(&out.bytes, &dalica_image::Limits::default()).unwrap();
    assert_eq!(header.thumbnail(&out.bytes), Some(NEW_THUMB));
    assert_eq!(header.orientation, dalica_image::Orientation::default());
}

#[test]
fn a_thumbnail_that_does_not_fit_one_segment_is_left_out() {
    let (source, _) = jpeg_source(false);
    let big = vec![0x55; MAX_JPEG_EXIF];
    let out = carry(&source, &jpeg_target(), &Carry { thumbnail: Some(&big), ..spec() }).unwrap();
    assert!(!out.thumbnail);
    assert!(walk(&exif_of(&out.bytes), &l()).unwrap().ifds.iter().all(|(k, _)| *k != IfdKind::Thumbnail));
    // A PNG has room for it.
    let out = carry(&source, &png_target(), &Carry { thumbnail: Some(&big), ..spec() }).unwrap();
    assert!(out.thumbnail);
}

#[test]
fn the_owned_fields_are_merged() {
    let date = DateTaken::parse_iso("2025-01-02T03:04:05+01:00").unwrap();
    let place = Place::new(-33.8568, 151.2153).unwrap();
    let changes = Changes {
        caption: Some(Some("A new caption".into())),
        tags: Some(vec!["alpha".into(), "beta".into()]),
        star: Some(true),
        place: Some(Some(place)),
        date_taken: Some(date.clone()),
    };
    let options =
        WriteOptions { stars_as_ratings: true, metadata_date: DateTaken::parse_iso("2026-10-13T10:00:00+02:00"), ..WriteOptions::default() };
    for (name, make) in SOURCES {
        let (source, note) = make(false);
        for target in [jpeg_target(), png_target()] {
            let c = Carry { changes: changes.clone(), options: options.clone(), ..spec() };
            let out = carry(&source, &target, &c).unwrap();
            let m = read(&out.bytes, &l()).unwrap();
            assert_eq!(m.owned.caption.as_deref(), Some("A new caption"), "{name}");
            assert_eq!(m.owned.tags, ["alpha", "beta"], "{name}");
            assert_eq!(m.owned.rating, Some(5), "{name}");
            assert_eq!(m.owned.date_taken.as_ref(), Some(&date), "{name}");
            assert!(m.owned.place.unwrap().close_to(&place, 1e-6), "{name}");
            // IPTC follows the caption and tags, and keeps its foreign dataset (a WebP has none to
            // keep; a JPEG target gets a new one, a PNG only an existing one).
            let s = scan(&out.bytes, &l()).unwrap();
            if let Some(iptc) = s.iptc_stream(&l()).unwrap() {
                assert_eq!(m.sources.iptc.caption.as_deref(), Some("A new caption"), "{name}");
                assert_eq!(datasets(iptc, &l()).unwrap().iter().any(|d| d.data == FOREIGN_DATASET), name != "WebP", "{name}");
            } else {
                assert!(name == "WebP" && target[1] == b'P', "{name}");
            }
            // Still orientation 1 and the maker note intact.
            let block = exif_of(&out.bytes);
            assert_eq!(m.summary.orientation, Some(1));
            assert_eq!(note_values(&block, note_offset(&block)), note_values(&exif_of(&source), note), "{name}");
            assert!(xmp_of(&out.bytes).contains(FOREIGN_XMP));

            // With the location removed, a place is not written back.
            let gone = carry(&source, &target, &Carry { remove_location: true, ..c.clone() }).unwrap();
            let m = read(&gone.bytes, &l()).unwrap();
            assert_eq!(m.owned.place, None, "{name}");
            assert_eq!(m.owned.caption.as_deref(), Some("A new caption"));
        }
    }
    // Options off: the field is not written.
    let (source, _) = jpeg_source(false);
    let c = Carry { changes: changes.clone(), options: WriteOptions { captions_tags_places: false, ..WriteOptions::default() }, ..spec() };
    let out = carry(&source, &jpeg_target(), &c).unwrap();
    assert_eq!(read(&out.bytes, &l()).unwrap().owned.caption.as_deref(), Some("Source caption"));
}

#[test]
fn photoshop_resources_keep_iptc_and_drop_pixel_ones() {
    let (source, _) = jpeg_source(false);
    let out = carry(&source, &jpeg_target(), &spec()).unwrap();
    let s = scan(&out.bytes, &l()).unwrap();
    let ps = s.photoshop.as_deref().unwrap();
    let ids: Vec<(u16, Vec<u8>)> = resources(ps, &l()).unwrap().iter().map(|r| (r.id, r.data.to_vec())).collect();
    assert_eq!(ids, [(0x03ed, FOREIGN_RESOURCE.to_vec()), (0x0404, stream())]);
    // A TIFF's bare stream becomes resources with its digest in a JPEG, and stays bare in a PNG.
    let (tiff, _) = tiff_source(false);
    let out = carry(&tiff, &jpeg_target(), &spec()).unwrap();
    let s = scan(&out.bytes, &l()).unwrap();
    let ids: Vec<u16> = resources(s.photoshop.as_deref().unwrap(), &l()).unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, [0x0404, 0x0425]);
    let out = carry(&tiff, &png_target(), &spec()).unwrap();
    assert_eq!(scan(&out.bytes, &l()).unwrap().iptc.as_deref(), Some(&stream()[..]));
}

/// A TIFF whose strip comes before its metadata (as many writers lay it out): the metadata is
/// packed, the pixels never reach the copy, and the maker note's absolute offsets move with it.
#[test]
fn a_tiff_with_its_strips_first_is_packed() {
    let strip = vec![0x5a; 100_000];
    for big_endian in [false, true] {
        let (source, note) = noted(big_endian, |n| {
            let mut t = camera(big_endian, &n);
            t.ifd1.clear();
            t.ifd0.extend([
                long(0x0100, 100_000),
                long(0x0101, 1),
                short(0x0102, 8),
                short(0x0103, 1),
                short(0x0106, 1),
                F::OffsetTo { tag: 0x0111, bytes: strip.clone() },
                short(0x0115, 1),
                long(0x0116, 1),
                long(0x0117, 100_000),
            ]);
            t.build()
        });
        assert!(note > 100_000);
        for target in [jpeg_target(), png_target()] {
            let out = carry(&source, &target, &spec()).unwrap();
            let block = exif_of(&out.bytes);
            assert!(block.len() < 2048, "packed: {}", block.len());
            assert!(!block.windows(64).any(|w| w.iter().all(|&b| b == 0x5a)));
            assert_ne!(note_offset(&block), note);
            assert_eq!(note_values(&block, note_offset(&block)), note_values(&source, note));
            // ImageWidth and ImageLength take the new size.
            assert_eq!(uint(&block, IfdKind::Image, 0x0100), Some(W));
            assert_eq!(uint(&block, IfdKind::Image, 0x0101), Some(H));
            assert_eq!(value(&block, IfdKind::Exif, FOREIGN_TAG).as_deref(), Some(FOREIGN_VALUE));
        }
    }
}

/// A TIFF without a maker note is packed too; a maker note that is not an IFD with absolute
/// offsets is carried byte for byte.
#[test]
fn other_maker_notes_are_copied_as_they_are() {
    let opaque = b"Vendor\0\0opaque maker note with its own header".to_vec();
    let mut t = camera(false, &opaque);
    t.ifd1.clear();
    let source = tiff_file(t);
    let out = carry(&source, &png_target(), &spec()).unwrap();
    let block = exif_of(&out.bytes);
    assert_eq!(value(&block, IfdKind::Exif, MAKER_NOTE), Some(opaque));
    let mut t = camera(false, b"x");
    t.exif.retain(|f| f.tag() != MAKER_NOTE);
    let out = carry(&tiff_file(t), &jpeg_target(), &spec()).unwrap();
    assert_eq!(uint(&exif_of(&out.bytes), IfdKind::Image, 0x0112), Some(1));
}

#[test]
fn a_source_without_metadata_gets_the_facts_of_the_copy() {
    for source in [jpeg(&[]), png(&[]), webp_simple(), tiff_file(Tiff::new())] {
        for target in [jpeg_target(), png_target()] {
            let out = carry(&source, &target, &spec()).unwrap();
            let block = exif_of(&out.bytes);
            assert_eq!(uint(&block, IfdKind::Image, 0x0112), Some(1));
            assert_eq!(uint(&block, IfdKind::Exif, 0xa002), Some(W));
            assert_eq!(value(&block, IfdKind::Exif, 0x9000).as_deref(), Some(&b"0232"[..]));
            let s = scan(&out.bytes, &l()).unwrap();
            assert!(s.xmp.is_none() && s.iptc.is_none() && s.photoshop.is_none());
        }
    }
}

#[test]
fn a_targets_own_metadata_is_replaced() {
    let (source, _) = jpeg_source(false);
    let stale = Tiff { ifd0: vec![short(0x0112, 8), ascii(0x010f, "Stale")], ..Tiff::new() }.build();
    let target = jpeg(&[app1_exif(&stale), app1_xmp(&wrapped("")), app2_icc(b"an sRGB profile")]);
    let out = carry(&source, &target, &spec()).unwrap();
    let s = scan(&out.bytes, &l()).unwrap();
    assert_eq!(s.spans.iter().filter(|s| s.kind == dalica_meta::BlockKind::Exif).count(), 1);
    assert_eq!(read(&out.bytes, &l()).unwrap().summary.make.as_deref(), Some("Dalicam"));
    let ptarget = png(&[(b"eXIf", stale)]);
    let out = carry(&source, &ptarget, &spec()).unwrap();
    assert_eq!(out.bytes.windows(4).filter(|w| *w == b"eXIf").count(), 1);
}

#[test]
fn extended_xmp_goes_into_a_jpeg_only() {
    let ext = wrapped("<rdf:Description rdf:about=\"\" xmlns:foo=\"http://example.com/foo/\" foo:History=\"a long history\"/>");
    let guid = String::from_utf8(extended_guid(&ext).to_vec()).unwrap();
    let (block, _) = noted(false, |n| camera(false, &n).build());
    let source = jpeg(&[app1_exif(&block), app1_xmp(&packet(Some(&guid))), app1_extended(&guid, ext.len() as u32, 0, &ext)]);
    assert_eq!(scan(&source, &l()).unwrap().extended_xmp.as_deref(), Some(&ext[..]));
    let out = carry(&source, &jpeg_target(), &spec()).unwrap();
    assert!(out.dropped.is_empty());
    assert_eq!(scan(&out.bytes, &l()).unwrap().extended_xmp.as_deref(), Some(&ext[..]));
    let out = carry(&source, &png_target(), &spec()).unwrap();
    assert_eq!(out.dropped, ["extended XMP"]);
    // GPS in the extended packet and the location removed: it is left out.
    let gps = wrapped("<rdf:Description rdf:about=\"\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" exif:GPSAltitude=\"5/1\"/>");
    let guid = String::from_utf8(extended_guid(&gps).to_vec()).unwrap();
    let source = jpeg(&[app1_xmp(&packet(Some(&guid))), app1_extended(&guid, gps.len() as u32, 0, &gps)]);
    let out = carry(&source, &jpeg_target(), &Carry { remove_location: true, ..spec() }).unwrap();
    assert_eq!(out.dropped, ["extended XMP"]);
    assert!(scan(&out.bytes, &l()).unwrap().extended_xmp.is_none());
    let out = carry(&source, &jpeg_target(), &spec()).unwrap();
    assert_eq!(scan(&out.bytes, &l()).unwrap().extended_xmp.as_deref(), Some(&gps[..]));
}

#[test]
fn exif_past_one_segment_fails_for_a_jpeg_only() {
    let big = Tiff { ifd0: vec![undefined(0xc4a5, &vec![7u8; 70_000])], ..Tiff::new() }.build();
    let source = png(&[(b"eXIf", big)]);
    assert_eq!(carry(&source, &jpeg_target(), &spec()).unwrap_err(), Error::TooMany("EXIF bytes"));
    let out = carry(&source, &png_target(), &spec()).unwrap();
    assert_eq!(value(&exif_of(&out.bytes), IfdKind::Image, 0xc4a5).map(|v| v.len()), Some(70_000));
}

// set_orientation.

#[test]
fn set_orientation_changes_only_that_value() {
    for big_endian in [false, true] {
        let mut t = camera(big_endian, b"opaque note");
        t.ifd0.retain(|f| f.tag() != 0x0112);
        t.ifd0.push(short(0x0112, 1));
        let block = t.build();
        let upright = String::from_utf8(packet(None)).unwrap().replace("tiff:Orientation=\"6\"", "tiff:Orientation=\"1\"");
        let file = jpeg(&[app1_exif(&block), app1_xmp(upright.as_bytes()), app13(&photoshop())]);
        let out = set_orientation(&file, 6, &l()).unwrap();
        assert_eq!(read(&out, &l()).unwrap().summary.orientation, Some(6));
        let after = exif_of(&out);
        assert_eq!(after.len(), block.len());
        let differ: Vec<usize> = (0..block.len()).filter(|&i| block[i] != after[i]).collect();
        assert_eq!(differ.len(), 1, "only the SHORT's significant byte");
        // The XMP mirror follows; nothing else in the packet changes.
        assert_eq!(xmp_of(&out), String::from_utf8(packet(None)).unwrap());
        // Every other segment is the same (M6.4's verification ran too).
        let (a, b) = (scan(&file, &l()).unwrap(), scan(&out, &l()).unwrap());
        assert_eq!(payload_digest(&file, &a.spans), payload_digest(&out, &b.spans));
        assert_eq!(scan(&out, &l()).unwrap().photoshop, a.photoshop);
    }
}

#[test]
fn set_orientation_adds_what_is_missing() {
    // IFD0 without an orientation: IFD0 is appended with it, the rest stays where it was.
    let block = Tiff { ifd0: vec![ascii(0x010f, "Dalicam")], exif: vec![undefined(FOREIGN_TAG, FOREIGN_VALUE)], ..Tiff::new() }.build();
    let file = jpeg(&[app1_exif(&block)]);
    let out = set_orientation(&file, 8, &l()).unwrap();
    let after = exif_of(&out);
    assert_eq!(&after[8..block.len()], &block[8..]);
    assert_eq!(uint(&after, IfdKind::Image, 0x0112), Some(8));
    assert_eq!(value(&after, IfdKind::Exif, FOREIGN_TAG).as_deref(), Some(FOREIGN_VALUE));
    // No EXIF at all: one is added after APP0.
    let out = set_orientation(&jpeg(&[]), 3, &l()).unwrap();
    assert_eq!(read(&out, &l()).unwrap().summary.orientation, Some(3));
    assert_eq!(&out[20..22], b"\xff\xe1");
}

#[test]
fn set_orientation_refuses_what_it_cannot_do() {
    let file = jpeg(&[]);
    assert!(matches!(set_orientation(&file, 0, &l()), Err(Error::Refused { field: "orientation", .. })));
    assert!(matches!(set_orientation(&file, 9, &l()), Err(Error::Refused { .. })));
    assert_eq!(set_orientation(&png(&[]), 6, &l()), Err(Error::Unsupported(dalica_meta::Container::Png)));
    let bad = jpeg(&[app1_exif(b"II*\0\xff\xff\0\0")]);
    assert!(set_orientation(&bad, 6, &l()).is_err());
}

// Hostile input.

#[test]
fn unreadable_blocks_are_err() {
    let target = jpeg_target();
    let looping = Tiff { ifd0: vec![long(0x8769, 8)], ..Tiff::new() }.build();
    let out_of_range = b"II*\0\xff\xff\xff\x7f".to_vec();
    for source in [
        jpeg(&[app1_exif(&out_of_range)]),
        jpeg(&[app1_exif(&looping)]),
        jpeg(&[app1_xmp(b"<x:xmpmeta><unclosed")]),
        jpeg(&[app1_xmp(b"<!DOCTYPE x [<!ENTITY a 'b'>]><x/>")]),
        jpeg(&[app13(b"8BIM\x04\x04\0\0\xff\xff\xff\xff")]),
        jpeg(&[app13(b"XXXX not resources")]),
        png(&[(b"eXIf", out_of_range.clone())]),
    ] {
        assert!(carry(&source, &target, &spec()).is_err());
        assert!(carry(&source, &png_target(), &spec()).is_err());
    }
    assert_eq!(carry(b"GIF89a", &target, &spec()).unwrap_err(), Error::Unknown);
    assert_eq!(carry(&jpeg(&[]), b"GIF89a....", &spec()).unwrap_err(), Error::Unknown);
    assert_eq!(carry(&jpeg(&[]), &webp_simple(), &spec()).unwrap_err(), Error::Unsupported(dalica_meta::Container::WebP));
}

#[test]
fn truncation_at_every_offset_never_panics() {
    let (source, _) = jpeg_source(false);
    let (target, ptarget) = (jpeg_target(), png_target());
    for end in 0..source.len() {
        let cut = &source[..end];
        let _ = carry(cut, &target, &spec());
        let _ = carry(cut, &ptarget, &spec());
        let _ = set_orientation(cut, 6, &l());
    }
    // Up to the scan the target's structure is walked; the scan data itself is not read here.
    let sos = target.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    for end in 0..target.len() {
        let r = carry(&source, &target[..end], &spec());
        assert_eq!(r.is_err(), end < sos + 2, "{end}");
    }
    for end in 0..ptarget.len() {
        assert!(carry(&source, &ptarget[..end], &spec()).is_err());
    }
    let (tiff, _) = tiff_source(true);
    for end in (0..tiff.len()).step_by(3) {
        let _ = carry(&tiff[..end], &ptarget, &spec());
    }
}

#[test]
fn every_byte_flip_never_panics() {
    let (source, _) = jpeg_source(true);
    let target = jpeg_target();
    for i in 0..source.len() {
        let mut s = source.clone();
        s[i] ^= 0xff;
        if let Ok(out) = carry(&s, &target, &Carry { thumbnail: Some(NEW_THUMB), ..spec() }) {
            assert!(read(&out.bytes, &l()).unwrap().problems.is_empty());
        }
        let _ = set_orientation(&s, 5, &l());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = carry(&bytes, &jpeg_target(), &spec());
        let _ = carry(&jpeg(&[app1_exif(&bytes)]), &png_target(), &spec());
        let _ = carry(&jpeg(&[app1_xmp(&bytes)]), &jpeg_target(), &spec());
        let _ = carry(&jpeg(&[app13(&bytes)]), &png_target(), &spec());
        let _ = carry(&jpeg(&[]), &bytes, &spec());
        let _ = set_orientation(&jpeg(&[app1_exif(&bytes)]), 6, &l());
    }

    #[test]
    fn random_mutations_never_panic(edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8), size in 1u32..100_000) {
        let (mut source, _) = tiff_source(false);
        for (at, b) in edits {
            let i = at % source.len();
            source[i] = b;
        }
        if let Ok(out) = carry(&source, &png_target(), &Carry { width: size, height: size, ..spec() }) {
            prop_assert!(read(&out.bytes, &l()).unwrap().problems.is_empty());
        }
    }
}

/// IFDs at the entry limit carry in linear time (no work proportional to entries × ranges).
#[test]
fn full_ifds_stay_fast() {
    let removed: Vec<F> = (0..4000).map(|_| rationals(0x0111, &[(1, 2)])).collect();
    let kept: Vec<F> = (0..8000u32).map(|i| rationals(0xc000 + (i % 0x500) as u16, &[(i, 1)])).collect();
    let block = Tiff { ifd0: removed, exif: kept, ..Tiff::new() }.build();
    let source = png(&[(b"eXIf", block)]);
    let start = std::time::Instant::now();
    let out = carry(&source, &png_target(), &spec()).unwrap();
    let took = start.elapsed();
    eprintln!("carrying 12,000 EXIF entries took {took:?}");
    assert!(took.as_secs_f64() < 5.0, "{took:?}");
    assert_eq!(walk(&exif_of(&out.bytes), &l()).unwrap().ifds.iter().find(|(k, _)| *k == IfdKind::Exif).unwrap().1.entries.len(), 8003);

    let (source, _) = jpeg_source(false);
    let start = std::time::Instant::now();
    for _ in 0..100 {
        carry(&source, &jpeg_target(), &Carry { thumbnail: Some(NEW_THUMB), ..spec() }).unwrap();
    }
    eprintln!("one camera JPEG's metadata carried in {:?}", start.elapsed() / 100);
}
