//! JPEG write-back (`xmp-interop.md` §3–§4, §8 tests 1–4, 6, 7 and 10 on JPEG): every owned field
//! round-trips through EXIF, XMP and IPTC; every other segment, the scan data and the foreign data
//! inside the rewritten blocks survive byte for byte; the file decodes to the same pixels; writing
//! twice gives the same bytes; the rating rule; IPTC's character set and digest; the APP1 limits;
//! new segments where readers look for them; hostile and truncated input never panics.

use crate::build;

use crate::common::{cases, date, l, xmp_text};
use build::*;
use dalica_meta::container::payload_digest;
use dalica_meta::exif::edit::MAX_JPEG_EXIF;
use dalica_meta::exif::read::walk;
use dalica_meta::fields::{clean_tags, clean_text};
use dalica_meta::iptc::read::{datasets, resources};
use dalica_meta::write::PLACE_TOLERANCE;
use dalica_meta::xmp::JPEG_MAX_PACKET;
use dalica_meta::{BlockKind, Changes, DateTaken, Error, Metadata, Place, WriteOptions, listing, read, scan, verify, write_owned};
use proptest::prelude::*;

/// The time of every write in these tests (`xmp:MetadataDate`).
const WRITTEN: &str = "2026-10-12T09:30:00+02:00";

/// Every write option on (stars included, off by default), with the time of the write.
fn on() -> WriteOptions {
    WriteOptions { stars_as_ratings: true, metadata_date: DateTaken::parse_iso(WRITTEN), ..WriteOptions::default() }
}

const CAPTION: &str = "Grandma's 90th — «שלום עולם» 🎂 & <b>\"quoted\"</b>\nsecond line";

fn tags() -> Vec<String> {
    ["beach", "a/b", "x, y", "Zürich", "日本", "R&D <team>"].map(String::from).to_vec()
}

/// A southern, western place.
fn place() -> Place {
    Place::new(-33.856_784, -70.651_234).unwrap()
}

/// A write of every owned field.
fn everything() -> Changes {
    Changes {
        caption: Some(Some(CAPTION.into())),
        tags: Some(tags()),
        star: Some(true),
        place: Some(Some(place())),
        date_taken: Some(date("2026-06-04T19:42:10", Some(-210), Some("050"))),
    }
}

fn write(file: &[u8], changes: &Changes) -> Vec<u8> {
    let out = write_owned(file, changes, &on()).unwrap();
    // `write_owned` verified it already; the check is public, so ask again.
    verify(file, &out, &changes.allowed(&on()), &l()).unwrap();
    out
}

fn meta(file: &[u8]) -> Metadata {
    let m = read(file, &l()).unwrap();
    assert!(m.problems.is_empty(), "{:?}", m.problems);
    m
}

// Files.

/// A camera's EXIF block: a real caption in ImageDescription, a date with offset and fraction, a
/// maker note, GPS altitude and time beside the place, and a thumbnail in IFD1.
fn camera() -> Tiff {
    Tiff {
        big_endian: false,
        ifd0: vec![ascii(0x010e, "Harbour at dusk"), ascii(0x010f, "Canon"), ascii(0x0110, "Canon EOS 5D"), short(0x0112, 1)],
        exif: vec![
            rationals(0x829a, &[(1, 250)]),
            ascii(0x9003, "2020:01:02 03:04:05"),
            ascii(0x9004, "2020:01:02 03:04:05"),
            ascii(0x9011, "+10:00"),
            // A maker note whose own entries hold offsets from the TIFF header.
            undefined(0x927c, b"Nikon\0\x02\x10\0\0MM\0*\0\0\0\x08 absolute offsets live here"),
        ],
        gps: vec![
            bytes(0x0000, &[2, 2, 0, 0]),
            ascii(0x0001, "N"),
            rationals(0x0002, &[(48, 1), (51, 1), (2412, 100)]),
            ascii(0x0003, "E"),
            rationals(0x0004, &[(2, 1), (17, 1), (4020, 100)]),
            rationals(0x0006, &[(35, 1)]),
        ],
        interop: vec![ascii(0x0001, "R98")],
        ifd1: vec![short(0x0103, 6), F::OffsetTo { tag: 0x0201, bytes: b"\xff\xd8 thumbnail \xff\xd9".to_vec() }, long(0x0202, 15)],
    }
}

const NS: &str = "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\" xmlns:stEvt=\"http://ns.adobe.com/xap/1.0/sType/ResourceEvent#\" xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\"";

/// Another application's description: its own caption, rating, history and develop settings, a
/// comment, and the extended-XMP declaration.
fn foreign_description(guid: &str) -> String {
    format!(
        "\n  <rdf:Description rdf:about=\"\" {NS} xmp:CreatorTool=\"Other App 7\" xmp:Rating=\"3\" crs:Exposure2012=\"+0.35\" xmpNote:HasExtendedXMP=\"{guid}\">\n   <!-- written by another application -->\n   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">Old caption</rdf:li><rdf:li xml:lang=\"fr\">Ancienne légende</rdf:li></rdf:Alt></dc:description>\n   <xmpMM:History><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><stEvt:action>saved</stEvt:action><stEvt:when>2020-05-06T07:08:09+01:00</stEvt:when></rdf:li></rdf:Seq></xmpMM:History>\n  </rdf:Description>\n  <rdf:Description rdf:about=\"\" xmlns:zz=\"urn:example:unknown/\"><zz:anything>kept <![CDATA[as <is>]]></zz:anything></rdf:Description>\n "
    )
}

/// A packet around `body` with `padding` spaces before the trailer.
fn padded_packet(body: &str, padding: usize) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{body}</rdf:RDF>\n</x:xmpmeta>\n{}<?xpacket end=\"w\"?>",
        " ".repeat(padding)
    )
    .into_bytes()
}

const EXTENDED: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/></x:xmpmeta>";

/// The extended packet's GUID: its MD5 digest in upper-case hex (XMP Part 3).
fn guid() -> String {
    md5_of(EXTENDED).iter().map(|b| format!("{b:02X}")).collect()
}

/// An IIM stream of another application: no 1:90, a Latin-1 object name and caption, a by-line and
/// a keyword.
fn latin1_iptc() -> Vec<u8> {
    iim(&[(1, 0, b"\0\x04"), (2, 0, b"\0\x04"), (2, 5, b"Caf\xe9"), (2, 25, b"old tag"), (2, 80, b"Ren\xe9e"), (2, 120, b"Ol\xe9 caption")])
}

/// Every segment kind a camera or another application leaves in a JPEG.
fn rich() -> Vec<u8> {
    let ps = irb(&[(0x03ed, &[0, 0x48, 0, 0, 0, 1, 0, 1, 0, 0x48, 0, 0, 0, 1, 0, 1]), (0x0404, &latin1_iptc()), (0x040c, b"pretend thumbnail")]);
    jpeg(&[
        app1_exif(&camera().build()),
        app1_xmp(&padded_packet(&foreign_description(&guid()), 512)),
        app1_extended(&guid(), EXTENDED.len() as u32, 0, &EXTENDED[..40]),
        app1_extended(&guid(), EXTENDED.len() as u32, 40, &EXTENDED[40..]),
        app2_icc(b"pretend ICC profile"),
        (APP2, b"MPF\0MM\0*\0\0\0\x08 pretend index".to_vec()),
        (0xe5, b"unknown application data".to_vec()),
        (COM, b"a comment".to_vec()),
        app13(&ps),
        // A second EXIF and a second XMP segment: only the first of each is read and rewritten.
        app1_exif(b"MM\0*\0\0\0\x08\0\0\0\0\0\0"),
        app1_xmp(b"<second packet, never read/>"),
        (0xee, b"Adobe\0\x64\0\0\0\0\x01".to_vec()),
    ])
}

/// The marker and bytes of every segment before the scan, and the rest of the file from SOS on.
fn segments(file: &[u8]) -> (Vec<(u8, Vec<u8>)>, Vec<u8>) {
    let mut at = 2;
    let mut out = Vec::new();
    loop {
        assert_eq!(file[at], 0xff);
        let marker = file[at + 1];
        if marker == 0xda {
            return (out, file[at..].to_vec());
        }
        let len = u16::from_be_bytes([file[at + 2], file[at + 3]]) as usize;
        out.push((marker, file[at..at + 2 + len].to_vec()));
        at += 2 + len;
    }
}

/// What each segment is, for order checks.
fn kinds(file: &[u8]) -> Vec<&'static str> {
    segments(file)
        .0
        .iter()
        .map(|(m, b)| match (*m, &b[4..]) {
            (0xe0, _) => "APP0",
            (0xe1, p) if p.starts_with(b"Exif\0") => "EXIF",
            (0xe1, p) if p.starts_with(b"http://ns.adobe.com/xap/1.0/\0") => "XMP",
            (0xe1, p) if p.starts_with(b"http://ns.adobe.com/xmp/extension/\0") => "XMP+",
            (0xe2, p) if p.starts_with(b"ICC_PROFILE\0") => "ICC",
            (0xe2, _) => "MPF",
            (0xed, _) => "APP13",
            (0xee, _) => "APP14",
            (0xfe, _) => "COM",
            (0xe5, _) => "APP5",
            (0xc0, _) => "SOF0",
            (0xc4, _) => "DHT",
            (0xdb, _) => "DQT",
            _ => "?",
        })
        .collect()
}

/// The segments a write must keep: all but the first EXIF, the first XMP and the APP13s.
fn kept(file: &[u8]) -> Vec<Vec<u8>> {
    let (segs, _) = segments(file);
    let (mut exif, mut xmp) = (false, false);
    let mut out = Vec::new();
    for (m, b) in segs {
        let p = &b[4..];
        if m == 0xe1 && p.starts_with(b"Exif\0") && !exif {
            exif = true;
        } else if m == 0xe1 && p.starts_with(b"http://ns.adobe.com/xap/1.0/\0") && !xmp {
            xmp = true;
        } else if m != 0xed {
            out.push(b);
        }
    }
    out
}

fn digest(file: &[u8]) -> u64 {
    payload_digest(file, &scan(file, &l()).unwrap().spans)
}

fn iptc_stream(file: &[u8]) -> Vec<u8> {
    scan(file, &l()).unwrap().iptc_stream(&l()).unwrap().unwrap().to_vec()
}

fn assert_everything(m: &Metadata) {
    let o = &m.owned;
    assert_eq!(o.caption.as_deref(), Some(CAPTION));
    assert_eq!(o.tags, tags());
    assert_eq!(o.rating, Some(5));
    assert!(o.place.unwrap().close_to(&place(), PLACE_TOLERANCE), "{:?}", o.place);
    assert_eq!(o.date_taken.as_ref().map(ToString::to_string).as_deref(), Some("2026-06-04T19:42:10.050-03:30"));
    // Each source holds its part: XMP and IPTC the caption and tags, EXIF the place and date.
    let s = &m.sources;
    assert_eq!((s.xmp.caption.as_deref(), &s.xmp.tags, s.xmp.rating), (Some(CAPTION), &tags(), Some(5)));
    assert_eq!((s.iptc.caption.as_deref(), &s.iptc.keywords), (Some(CAPTION), &tags()));
    assert!(s.exif.place.unwrap().close_to(&place(), PLACE_TOLERANCE));
    assert_eq!(s.exif.date_taken, o.date_taken);
}

// §8 test 1: every owned field round-trips.

#[test]
fn every_owned_field_round_trips_in_each_kind_of_jpeg() {
    let bases = [
        ("bare", jpeg(&[])),
        ("exif only", jpeg(&[app1_exif(&camera().build())])),
        ("big-endian exif", jpeg(&[app1_exif(&Tiff { big_endian: true, ..camera() }.build())])),
        ("xmp only", jpeg(&[app1_xmp(&padded_packet(&foreign_description(&guid()), 100))])),
        ("iptc only", jpeg(&[app13(&irb(&[(0x0404, &latin1_iptc())]))])),
        ("everything", rich()),
    ];
    for (name, file) in bases {
        let out = write(&file, &everything());
        assert_everything(&meta(&out));
        assert!(xmp_text(&out).contains(&format!("<xmp:MetadataDate>{WRITTEN}</xmp:MetadataDate>")), "{name}");
        // Clearing removes each clearable field from every source.
        let cleared = write(&out, &Changes { caption: Some(None), tags: Some(vec![]), place: Some(None), star: Some(false), ..Changes::default() });
        let m = meta(&cleared);
        assert_eq!((m.owned.caption.as_deref(), m.owned.tags.len(), m.owned.rating, m.owned.place), (None, 0, Some(0), None), "{name}");
        assert_eq!(
            (m.sources.xmp.caption.as_deref(), m.sources.iptc.caption.as_deref(), m.sources.exif.caption.as_deref()),
            (None, None, None),
            "{name}"
        );
        assert!(m.sources.iptc.keywords.is_empty() && m.sources.xmp.place.is_none() && m.sources.exif.place.is_none(), "{name}");
        // The other languages of the caption stay.
        if name == "xmp only" || name == "everything" {
            assert!(xmp_text(&cleared).contains("Ancienne légende"), "{name}");
        }
    }
}

#[test]
fn dates_with_offsets_and_fractions_and_places_on_each_side_of_the_globe() {
    let file = rich();
    for (d, p) in [
        (date("2001-02-03T04:05:06", None, None), Place::new(0.0, 0.0).unwrap()),
        (date("2026-12-31T23:59:59", Some(345), Some("123456789")), Place::new(51.5, -0.1275).unwrap()),
        (date("1999-07-01T12:00:00", Some(-600), None), Place::new(-90.0, 180.0).unwrap()),
        (date("2010-10-10T10:10:10", Some(0), Some("5")), Place::new(-0.000_001, -179.999_999).unwrap()),
    ] {
        let out = write(&file, &Changes { date_taken: Some(d.clone()), place: Some(Some(p)), ..Changes::default() });
        let m = meta(&out);
        assert_eq!(m.owned.date_taken.as_ref(), Some(&d));
        assert!(m.owned.place.unwrap().close_to(&p, PLACE_TOLERANCE), "{p}");
    }
}

#[test]
fn the_caption_s_image_description_copy_follows_it_and_placeholders_stay() {
    let caption = Changes { caption: Some(Some("New".into())), ..Changes::default() };
    // A real caption in ImageDescription is replaced.
    let out = write(&jpeg(&[app1_exif(&camera().build())]), &caption);
    assert_eq!(meta(&out).sources.exif.caption.as_deref(), Some("New"));
    // A placeholder is left alone: the EXIF segment is byte for byte the old one.
    let mut t = camera();
    t.ifd0[0] = ascii(0x010e, "OLYMPUS DIGITAL CAMERA");
    let file = jpeg(&[app1_exif(&t.build())]);
    let out = write(&file, &caption);
    assert_eq!(segments(&out).0[1], segments(&file).0[1]);
    assert_eq!(meta(&out).owned.caption.as_deref(), Some("New"));
}

#[test]
fn noncharacters_xml_forbids_are_dropped_from_every_block_alike() {
    let out =
        write(&rich(), &Changes { caption: Some(Some("a\u{fffe}b\u{ffff}".into())), tags: Some(vec!["t\u{ffff}".into()]), ..Changes::default() });
    let m = meta(&out);
    assert_eq!(
        (m.sources.xmp.caption.as_deref(), m.sources.iptc.caption.as_deref(), m.owned.caption.as_deref()),
        (Some("ab"), Some("ab"), Some("ab"))
    );
    assert_eq!((m.sources.xmp.tags.clone(), m.sources.iptc.keywords.clone()), (vec!["t".to_owned()], vec!["t".to_owned()]));
}

// New segments where readers look for them.

#[test]
fn new_segments_go_where_readers_expect_them() {
    // A bare file: EXIF and XMP after APP0, APP13 after them, all before the tables and frame.
    let out = write(&jpeg(&[]), &everything());
    assert_eq!(kinds(&out), ["APP0", "EXIF", "XMP", "APP13", "SOF0"]);
    // With an ICC profile and an Adobe APP14: APP13 after the last APP0–APP12, before APP14.
    let file = jpeg(&[app2_icc(b"icc"), (COM, b"c".to_vec()), (0xee, b"Adobe\0\x64\0\0\0\0\x01".to_vec())]);
    assert_eq!(kinds(&write(&file, &everything())), ["APP0", "EXIF", "XMP", "ICC", "APP13", "COM", "APP14", "SOF0"]);
    // An existing EXIF segment: a new XMP goes right after it, wherever it is.
    let file = jpeg(&[app2_icc(b"icc"), app1_exif(&camera().build())]);
    assert_eq!(kinds(&write(&file, &everything())), ["APP0", "ICC", "EXIF", "XMP", "APP13", "SOF0"]);
    // No APP0 at all: right after SOI.
    let file = jpeg(&[]);
    let no_app0 = [&file[..2], &file[2 + 4 + 14..]].concat();
    assert_eq!(kinds(&write(&no_app0, &everything())), ["EXIF", "XMP", "APP13", "SOF0"]);
}

#[test]
fn only_the_segments_a_change_needs_are_added() {
    let bare = jpeg(&[]);
    // A caption needs no EXIF (Dalica never adds an ImageDescription).
    let caption = write(&bare, &Changes { caption: Some(Some("c".into())), ..Changes::default() });
    assert_eq!(kinds(&caption), ["APP0", "XMP", "APP13", "SOF0"]);
    // A place or a date needs only EXIF (the XMP mirrors go only into a packet that exists).
    let place_only = write(&bare, &Changes { place: Some(Some(place())), ..Changes::default() });
    assert_eq!(kinds(&place_only), ["APP0", "EXIF", "SOF0"]);
    // A star needs only XMP.
    assert_eq!(kinds(&write(&bare, &Changes { star: Some(true), ..Changes::default() })), ["APP0", "XMP", "SOF0"]);
    // Clearing what is not there changes nothing at all.
    let clear = Changes { caption: Some(None), tags: Some(vec![]), place: Some(None), star: Some(false), ..Changes::default() };
    assert_eq!(write(&bare, &clear), bare);
    // Tags into a file whose Photoshop resources hold no IPTC: added beside the others.
    let ps = jpeg(&[app13(&irb(&[(0x03ed, &[1; 16])]))]);
    let out = write(&ps, &Changes { tags: Some(tags()), ..Changes::default() });
    let out_ps = scan(&out, &l()).unwrap().photoshop.unwrap().into_owned();
    let ids: Vec<u16> = resources(&out_ps, &l()).unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, [0x03ed, 0x0404, 0x0425]);
    // But clearing them adds none.
    assert_eq!(write(&ps, &Changes { tags: Some(vec![]), ..Changes::default() }), ps);
}

// §8 test 2: foreign data survives.

#[test]
fn foreign_data_survives_byte_for_byte() {
    let file = rich();
    let out = write(&file, &everything());
    // Every segment but the rewritten ones, in order, and everything from SOS on.
    assert_eq!(kept(&out), kept(&file));
    assert_eq!(segments(&out).1, segments(&file).1);
    assert_eq!(digest(&out), digest(&file));
    assert_eq!(kinds(&out), kinds(&file));

    // EXIF: the maker note and the thumbnail keep their bytes and their offsets.
    let (old_block, new_block) = (scan(&file, &l()).unwrap().exif.unwrap().into_owned(), scan(&out, &l()).unwrap().exif.unwrap().into_owned());
    let entry = |block: &[u8], tag: u16| {
        let w = walk(block, &l()).unwrap();
        let e = *w.ifds.iter().flat_map(|(_, i)| i.entries.iter()).find(|e| e.tag == tag).unwrap();
        (w.tiff.value_range(&e).unwrap(), w.tiff.value(&e).unwrap().unwrap().to_vec())
    };
    let maker_note = |block: &[u8]| {
        let (range, value) = entry(block, 0x927c);
        let range = range.unwrap();
        assert_eq!(block[range.clone()], value[..]);
        (range, value)
    };
    assert_eq!(maker_note(&new_block), maker_note(&old_block));
    // The thumbnail: the same offset (inline), and the same bytes there.
    let thumbnail = |block: &[u8]| {
        let (_, offset) = entry(block, 0x0201);
        let at = u32::from_le_bytes([offset[0], offset[1], offset[2], offset[3]]) as usize;
        (at, block[at..at + 15].to_vec())
    };
    assert_eq!(thumbnail(&new_block), thumbnail(&old_block));
    assert!(new_block.len() >= old_block.len());
    let m = meta(&out);
    assert_eq!((m.summary.make.as_deref(), m.summary.model.as_deref()), (Some("Canon"), Some("Canon EOS 5D")));

    // XMP: the foreign description keeps everything but the owned properties this write moved.
    let x = xmp_text(&out);
    for kept in [
        "xmp:CreatorTool=\"Other App 7\"",
        "crs:Exposure2012=\"+0.35\"",
        &format!("xmpNote:HasExtendedXMP=\"{}\"", guid()),
        "<!-- written by another application -->",
        "<xmpMM:History><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><stEvt:action>saved</stEvt:action><stEvt:when>2020-05-06T07:08:09+01:00</stEvt:when></rdf:li></rdf:Seq></xmpMM:History>",
        "<rdf:Description rdf:about=\"\" xmlns:zz=\"urn:example:unknown/\"><zz:anything>kept <![CDATA[as <is>]]></zz:anything></rdf:Description>",
        "<rdf:li xml:lang=\"fr\">Ancienne légende</rdf:li>",
    ] {
        assert!(x.contains(kept), "{kept} lost from\n{x}");
    }
    assert!(!x.contains("Old caption") && !x.contains("xmp:Rating=\"3\""));
    // Extended XMP is untouched and still belongs to the packet.
    assert_eq!(scan(&out, &l()).unwrap().extended_xmp.as_deref(), Some(EXTENDED));

    // IPTC: other datasets and resources keep their bytes and places.
    let ps_old = scan(&file, &l()).unwrap().photoshop.unwrap().into_owned();
    let ps_new = scan(&out, &l()).unwrap().photoshop.unwrap().into_owned();
    let res = |b: &[u8]| resources(b, &l()).unwrap().iter().map(|r| (r.id, r.data.to_vec())).collect::<Vec<_>>();
    let (r_old, r_new) = (res(&ps_old), res(&ps_new));
    assert_eq!(r_new.iter().map(|r| r.0).collect::<Vec<_>>(), [0x03ed, 0x0404, 0x040c, 0x0425]);
    assert_eq!((&r_new[0], &r_new[2]), (&r_old[0], &r_old[2]));
    let stream = iptc_stream(&out);
    let ds: Vec<(u8, u8, Vec<u8>)> = datasets(&stream, &l()).unwrap().iter().map(|d| (d.record, d.number, d.data.to_vec())).collect();
    assert!(ds.contains(&(1, 0, b"\0\x04".to_vec())) && ds.contains(&(2, 0, b"\0\x04".to_vec())));
    // Latin-1 text re-encoded so it reads the same under the new UTF-8 declaration.
    assert!(ds.contains(&(2, 5, "Café".as_bytes().to_vec())) && ds.contains(&(2, 80, "Renée".as_bytes().to_vec())));
}

#[test]
fn the_bytes_outside_the_metadata_hash_the_same_for_every_kind_of_change() {
    let file = rich();
    let changes = [
        Changes { caption: Some(Some("only a caption".into())), ..Changes::default() },
        Changes { tags: Some(vec!["one".into()]), ..Changes::default() },
        Changes { star: Some(true), ..Changes::default() },
        Changes { place: Some(None), ..Changes::default() },
        Changes { date_taken: Some(date("2000-01-01T00:00:00", None, None)), ..Changes::default() },
        everything(),
    ];
    for c in changes {
        let out = write(&file, &c);
        assert_eq!(digest(&out), digest(&file), "{c:?}");
        assert_eq!(kept(&out), kept(&file), "{c:?}");
    }
}

/// A baseline greyscale JPEG that really decodes: 32 × 16 pixels, eight blocks of different
/// greys, DC coefficients only, Huffman tables of fixed-length codes. Built from ITU-T T.81 so
/// the test needs no encoder.
fn real_jpeg(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let (w, h) = (32u16, 16u16);
    let mut segs = segments.to_vec();
    segs.push((0xdb, [&[0u8][..], &[8u8; 64]].concat()));
    segs.push((0xc0, [&[8u8][..], &h.to_be_bytes(), &w.to_be_bytes(), &[1, 1, 0x11, 0]].concat()));
    // DC: categories 0–11, all four-bit codes. AC: one two-bit code for end of block.
    let mut dc = vec![0x00];
    dc.extend([0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    dc.extend(0..12u8);
    let mut ac = vec![0x10];
    ac.extend([0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    ac.push(0x00);
    segs.push((0xc4, dc));
    segs.push((0xc4, ac));
    // The scan: each block's DC difference (category code, extra bits) and an end of block.
    let levels: [i32; 8] = [-90, 40, 100, -20, 10, -60, 75, 0];
    let mut bits: Vec<bool> = Vec::new();
    let put = |value: u32, n: u32, bits: &mut Vec<bool>| (0..n).rev().for_each(|i| bits.push(value >> i & 1 == 1));
    let mut prev = 0;
    for level in levels {
        let diff = level - prev;
        prev = level;
        let cat = 32 - diff.unsigned_abs().leading_zeros();
        put(cat, 4, &mut bits);
        let extra = if diff >= 0 { diff as u32 } else { (diff - 1) as u32 & ((1 << cat) - 1) };
        put(extra, cat, &mut bits);
        put(0, 2, &mut bits);
    }
    while !bits.len().is_multiple_of(8) {
        bits.push(true);
    }
    let mut data = Vec::new();
    for byte in bits.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| a << 1 | u8::from(b))) {
        data.push(byte);
        if byte == 0xff {
            data.push(0);
        }
    }
    let mut out = vec![0xff, 0xd8];
    let mut all = vec![(0xe0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0".to_vec())];
    all.extend(segs);
    for (marker, payload) in all {
        out.extend([0xff, marker]);
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
    }
    out.extend([0xff, 0xda, 0, 8, 1, 1, 0x00, 0, 0x3f, 0]);
    out.extend(data);
    out.extend([0xff, 0xd9]);
    out
}

#[test]
fn the_image_decodes_to_identical_pixels() {
    let il = dalica_image::Limits::default();
    for file in [
        real_jpeg(&[]),
        real_jpeg(&[app1_exif(&camera().build()), app1_xmp(&padded_packet(&foreign_description(&guid()), 64)), (COM, b"comment".to_vec())]),
        real_jpeg(&[app13(&irb(&[(0x0404, &latin1_iptc())])), (0xee, b"Adobe\0\x64\0\0\0\0\x00".to_vec())]),
    ] {
        let before = dalica_image::decode(&file, &il).unwrap();
        // The test image is not flat, so equal pixels mean something.
        let mut greys: Vec<u8> = before.data.chunks(4).map(|p| p[0]).collect();
        greys.sort_unstable();
        greys.dedup();
        assert!(greys.len() >= 8, "{greys:?}");
        let out = write(&file, &everything());
        assert_eq!(dalica_image::decode(&out, &il).unwrap(), before);
        assert_eq!(segments(&out).1, segments(&file).1);
    }
}

// A camera's multi-picture file: the primary image, its MPF index, and data after its EOI.

/// The APP2 MPF payload (CIPA DC-007 §5.2): a big-endian MP header and an MP Index IFD with
/// MPFVersion, NumberOfImages and two MP entries (the primary at offset 0 and a secondary image
/// of `size` bytes at `offset`, counted from the MP header).
fn mpf(size: u32, offset: u32) -> Vec<u8> {
    let mut p = b"MPF\0MM\0*\0\0\0\x08".to_vec();
    p.extend(3u16.to_be_bytes());
    // Three 12-byte entries and the next-IFD offset: the entries' data is at 8 + 2 + 36 + 4 = 50.
    p.extend([0xb0, 0x00, 0, 7, 0, 0, 0, 4]);
    p.extend(b"0100");
    p.extend([0xb0, 0x01, 0, 4, 0, 0, 0, 1, 0, 0, 0, 2]);
    p.extend([0xb0, 0x02, 0, 7, 0, 0, 0, 32, 0, 0, 0, 50]);
    p.extend([0; 4]);
    // The primary (offset 0), then the secondary (a large preview).
    p.extend([0x20, 0x03, 0, 0, 0, 0, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    p.extend([0x00, 0x01, 0, 1]);
    p.extend(size.to_be_bytes());
    p.extend(offset.to_be_bytes());
    p.extend([0; 4]);
    p
}

/// The MP header's position in `file`: 8 bytes into the MPF segment (FF E2, the length, `MPF\0`).
fn mpf_header(file: &[u8]) -> Option<usize> {
    let mut at = 2;
    for (marker, bytes) in segments(file).0 {
        if marker == APP2 && bytes[4..].starts_with(b"MPF\0") {
            return Some(at + 8);
        }
        at += bytes.len();
    }
    None
}

/// Where the image of each MP entry other than the primary starts in `file`: the MP header's
/// position plus the entry's offset.
fn mpf_secondaries(file: &[u8]) -> Vec<usize> {
    let header = mpf_header(file).unwrap();
    let p = &file[header..];
    let be = |at: usize| u32::from_be_bytes(p[at..at + 4].try_into().unwrap()) as usize;
    let entries = be(42);
    (0..2).map(|i| be(entries + 16 * i + 8)).filter(|&o| o != 0).map(|o| header + o).collect()
}

/// A phone's file: EXIF, the MPF index, ICC and `after` after it (where phones put ICC), the
/// primary image, a secondary JPEG right after the primary's EOI, and a trailer after that (a
/// motion photo's video, here the start of an MP4 box).
fn multi_picture(after: &[(u8, Vec<u8>)]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let secondary = real_jpeg(&[]);
    let trailer = b"\0\0\0\x18ftypmp42\0\0\0\0isommp42 motion photo trailer".to_vec();
    let build = |offset: u32| {
        let mut segs = vec![app1_exif(&camera().build()), (APP2, mpf(secondary.len() as u32, offset)), app2_icc(b"pretend ICC profile")];
        segs.extend(after.iter().cloned());
        [real_jpeg(&segs), secondary.clone(), trailer.clone()].concat()
    };
    let draft = build(0);
    let primary_len = draft.len() - secondary.len() - trailer.len();
    let file = build((primary_len - mpf_header(&draft).unwrap()) as u32);
    assert_eq!(mpf_secondaries(&file), [primary_len]);
    (file, secondary, trailer)
}

/// Data after the primary image's EOI is not image data the writer may move without care: an MP
/// entry's offset counts from the MPF segment, so a segment added or grown after that segment
/// moves the secondary image away from where the index says it is.
#[test]
fn a_multi_picture_file_keeps_its_secondary_images_and_trailer() {
    let (file, secondary, trailer) = multi_picture(&[]);
    let il = dalica_image::Limits::default();
    let pixels = dalica_image::decode(&file, &il).unwrap();
    // Every field, and a caption and tags alone: the new APP13 would go after ICC, so after the
    // MPF segment, where it moved the secondary image.
    let sets = [everything(), Changes { caption: Some(Some(CAPTION.into())), tags: Some(tags()), ..Changes::default() }];
    for c in &sets {
        let out = write(&file, c);
        let starts = mpf_secondaries(&out);
        assert_eq!(starts.len(), 1, "{c:?}");
        let at = starts[0];
        assert!(
            out.get(at..at + secondary.len()) == Some(&secondary[..]),
            "{c:?}: the MP entry points at {at}, where {:02x?} is",
            out.get(at..at + 4)
        );
        assert!(out.ends_with(&trailer), "the trailer changed");
        assert_eq!(at + secondary.len() + trailer.len(), out.len());
        assert_eq!(dalica_image::decode(&out, &il).unwrap(), pixels);
        assert_eq!(write(&out, c), out, "writing twice");
        assert_eq!(mpf_header(&out).map(|h| out[h - 8..h + 82].to_vec()), mpf_header(&file).map(|h| file[h - 8..h + 82].to_vec()));
    }
    assert_eq!(kinds(&write(&file, &sets[1]))[..5], ["APP0", "EXIF", "XMP", "APP13", "MPF"]);
}

/// A write that would still move the images an MPF index locates is refused, naming the field: a
/// segment after the index that grows (here an XMP packet with no padding), or a size change after
/// an index that cannot be read in a file with data after its EOI. Without data after the EOI
/// there is nothing to move, and the write goes ahead.
#[test]
fn a_write_that_would_move_mpf_images_is_refused() {
    let caption = Changes { caption: Some(Some(CAPTION.into())), ..Changes::default() };
    let (file, ..) = multi_picture(&[app1_xmp(&padded_packet(&foreign_description(&guid()), 0))]);
    let err = write_owned(&file, &caption, &on()).unwrap_err();
    assert!(matches!(&err, Error::Refused { field: "caption", why } if why.contains("MPF")), "{err}");
    // A star grows the packet too, and is refused the same way, naming the star.
    assert!(matches!(write_owned(&file, &Changes { star: Some(true), ..Changes::default() }, &on()), Err(Error::Refused { field: "star", .. })));
    // An index that cannot be read, with an APP13 after it that a caption rewrites.
    let unreadable = |trailer: &[u8]| {
        let segs = [(APP2, b"MPF\0MM\0*\0\0\0\x08 unreadable".to_vec()), app13(&irb(&[(0x0404, &latin1_iptc())]))];
        [real_jpeg(&segs), trailer.to_vec()].concat()
    };
    assert!(matches!(write_owned(&unreadable(&real_jpeg(&[])), &caption, &on()), Err(Error::Refused { field: "caption", .. })));
    let alone = unreadable(b"");
    assert_eq!(meta(&write(&alone, &caption)).owned.caption.as_deref(), Some(CAPTION));
}

// §8 test 3: idempotence.

#[test]
fn writing_the_same_values_twice_gives_identical_bytes() {
    let sets = [
        everything(),
        Changes { caption: Some(Some("c".into())), ..Changes::default() },
        Changes { tags: Some(tags()), star: Some(false), ..Changes::default() },
        Changes { place: Some(None), date_taken: Some(date("2030-03-03T03:03:03", Some(60), None)), ..Changes::default() },
    ];
    for file in [jpeg(&[]), rich(), real_jpeg(&[app1_exif(&camera().build())])] {
        for c in &sets {
            let once = write(&file, c);
            let twice = write(&once, c);
            assert_eq!(twice, once, "{c:?}");
        }
    }
}

#[test]
fn padding_absorbs_a_change_so_the_xmp_segment_keeps_its_length() {
    let file = jpeg(&[app1_xmp(&padded_packet(&foreign_description(&guid()), 2048))]);
    let xmp_len = |f: &[u8]| segments(f).0.iter().find(|(m, b)| *m == 0xe1 && b[4..].starts_with(b"http://ns.adobe.com/xap/1.0/\0")).unwrap().1.len();
    let starred = write(&file, &Changes { star: Some(true), ..Changes::default() });
    assert_eq!(starred.len(), file.len());
    let captioned = write(&starred, &Changes { caption: Some(Some("a caption".into())), ..Changes::default() });
    let longer = write(&captioned, &Changes { caption: Some(Some("a much longer caption than the one before".into())), ..Changes::default() });
    assert_eq!((xmp_len(&captioned), xmp_len(&longer)), (xmp_len(&file), xmp_len(&file)));
}

// §8 test 4: the rating rule.

#[test]
fn unstarring_never_clears_a_rating_dalica_did_not_set() {
    let rated = |r: &str| jpeg(&[app1_xmp(&padded_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"{r}\"/>"), 256))]);
    let unstar = Changes { star: Some(false), ..Changes::default() };
    let star = Changes { star: Some(true), ..Changes::default() };
    for r in ["3", "-1", "0", "4"] {
        let file = rated(r);
        assert_eq!(write(&file, &unstar), file, "rating {r}");
        // Starring writes 5; unstarring that writes 0.
        let starred = write(&file, &star);
        assert_eq!(meta(&starred).owned.rating, Some(5));
        assert_eq!(meta(&write(&starred, &unstar)).owned.rating, Some(0));
    }
    // A 5 by anyone is a star; unstarring it writes 0.
    assert_eq!(meta(&write(&rated("5"), &unstar)).owned.rating, Some(0));
    // No packet, no rating: nothing to do.
    assert_eq!(write(&jpeg(&[]), &unstar), jpeg(&[]));
    // With the option off (the default) a star is never written.
    let file = rated("3");
    assert_eq!(write_owned(&file, &star, &WriteOptions::default()).unwrap(), file);
}

// §8 test 6: IPTC.

#[test]
fn iptc_declares_utf8_and_its_digest_matches() {
    for file in [jpeg(&[app13(&irb(&[(0x0404, &latin1_iptc())]))]), jpeg(&[])] {
        let had = scan(&file, &l()).unwrap().photoshop.is_some();
        let out = write(&file, &Changes { tags: Some(tags()), ..Changes::default() });
        let stream = iptc_stream(&out);
        let ds = datasets(&stream, &l()).unwrap();
        assert_eq!(ds.iter().find(|d| (d.record, d.number) == (1, 90)).map(|d| d.data), Some(&b"\x1b%G"[..]));
        let ps = scan(&out, &l()).unwrap().photoshop.unwrap().into_owned();
        let digest = resources(&ps, &l()).unwrap().into_iter().find(|r| r.id == 0x0425).map(|r| r.data.to_vec());
        assert_eq!(digest, Some(md5_of(&stream)));
        let m = meta(&out);
        assert_eq!(m.sources.iptc.keywords, tags());
        if had {
            // The Latin-1 caption reads the same before and after the declaration.
            assert_eq!(meta(&file).sources.iptc.caption.as_deref(), Some("Olé caption"));
            assert_eq!(m.sources.iptc.caption.as_deref(), Some("Olé caption"));
            let rows = listing(&out, &l()).unwrap().rows;
            assert!(rows.iter().any(|r| r.label == "Object name" && r.value == "Café"), "{rows:?}");
        } else {
            // A new record 2 starts with its version.
            assert_eq!(ds.iter().find(|d| (d.record, d.number) == (2, 0)).map(|d| d.data), Some(&[0u8, 4][..]));
        }
    }
}

/// MD5 (RFC 1321), independent of the crate under test.
fn md5_of(data: &[u8]) -> Vec<u8> {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16,
        23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    let mut h = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d)];
    }
    h.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
fn the_md5_used_here_is_rfc_1321() {
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    assert_eq!(hex(&md5_of(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&md5_of(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
}

#[test]
fn a_caption_past_the_iptc_limit_goes_whole_to_xmp() {
    let long = "é".repeat(1500); // 3,000 bytes
    let out = write(&jpeg(&[]), &Changes { caption: Some(Some(long.clone())), ..Changes::default() });
    let m = meta(&out);
    assert_eq!(m.owned.caption.as_deref(), Some(long.as_str()));
    assert_eq!(m.sources.iptc.caption.map(|c| c.len()), Some(2000));
}

#[test]
fn photoshop_resources_over_several_segments_are_rejoined_and_split_at_the_limit() {
    // A 70,000-byte foreign resource cannot fit one APP13: the file carries it over two.
    let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    let ps = irb(&[(0x03ed, &[7; 16]), (0x040c, &big), (0x0404, &latin1_iptc())]);
    let cut = 60_000;
    let file = jpeg(&[app13(&ps[..cut]), (COM, b"between".to_vec()), app13(&ps[cut..])]);
    assert_eq!(scan(&file, &l()).unwrap().photoshop.as_deref(), Some(&ps[..]));
    let out = write(&file, &Changes { tags: Some(tags()), caption: Some(Some("c".into())), ..Changes::default() });
    // Rejoined in the first one's place and split at the segment limit, the comment kept.
    assert_eq!(kinds(&out), ["APP0", "XMP", "APP13", "APP13", "COM", "SOF0"]);
    assert!(segments(&out).0.iter().all(|(_, b)| b.len() <= 65_537));
    let new_ps = scan(&out, &l()).unwrap().photoshop.unwrap().into_owned();
    let r = resources(&new_ps, &l()).unwrap();
    assert_eq!(r.iter().map(|r| r.id).collect::<Vec<_>>(), [0x03ed, 0x040c, 0x0404, 0x0425]);
    assert_eq!(r[1].data, &big[..]);
    assert_eq!(meta(&out).sources.iptc.keywords, tags());
}

// §8 test 7: size limits.

/// A JPEG whose packet is `len` bytes long with `padding` of them padding, its bulk another
/// application's property.
fn packet_of(len: usize, padding: usize) -> Vec<u8> {
    let empty = padded_packet("\n  <rdf:Description rdf:about=\"\" xmlns:zz=\"urn:example:unknown/\" zz:bulk=\"\"/>\n ", padding);
    let bulk = "x".repeat(len - empty.len());
    let p = padded_packet(&format!("\n  <rdf:Description rdf:about=\"\" xmlns:zz=\"urn:example:unknown/\" zz:bulk=\"{bulk}\"/>\n "), padding);
    assert_eq!(p.len(), len);
    p
}

#[test]
fn an_xmp_packet_past_one_app1_drops_its_padding_first_then_fails_naming_the_field() {
    // Full to the limit, with 2,000 bytes of padding: a caption that fits once the padding goes.
    let file = jpeg(&[app1_xmp(&packet_of(JPEG_MAX_PACKET, 2000))]);
    let out = write(&file, &Changes { caption: Some(Some("c".repeat(1500))), ..Changes::default() });
    let x = scan(&out, &l()).unwrap().xmp.unwrap().into_owned();
    assert_eq!(x.len(), JPEG_MAX_PACKET);
    assert_eq!(meta(&out).owned.caption.map(|c| c.len()), Some(1500));
    assert!(x.ends_with(b"<?xpacket end=\"w\"?>"));
    // Past the padding: the write fails naming the field, and nothing is returned.
    let fail = |c: Changes| write_owned(&file, &c, &on());
    assert_eq!(fail(Changes { caption: Some(Some("c".repeat(3000))), ..Changes::default() }), Err(Error::TooMany("XMP bytes for the caption")));
    let many: Vec<String> = (0..200).map(|i| format!("tag number {i}")).collect();
    assert_eq!(
        fail(Changes { caption: Some(Some("short".into())), tags: Some(many), ..Changes::default() }),
        Err(Error::TooMany("XMP bytes for the tags"))
    );
    // Without any padding, even a star does not fit.
    let full = jpeg(&[app1_xmp(&packet_of(JPEG_MAX_PACKET, 0))]);
    assert_eq!(write_owned(&full, &Changes { star: Some(true), ..Changes::default() }, &on()), Err(Error::TooMany("XMP bytes for the star")));
    // The other blocks are not written either: the error is for the whole write.
    assert_eq!(
        write_owned(&full, &Changes { star: Some(true), place: Some(Some(place())), ..Changes::default() }, &on()),
        Err(Error::TooMany("XMP bytes for the star"))
    );
}

#[test]
fn an_exif_block_past_one_app1_fails_naming_the_field() {
    // 20 bytes short of the limit, the bulk in another application's tag at the end of the data.
    let with = |fill: usize| {
        Tiff {
            ifd0: vec![ascii(0x010e, "A caption")],
            exif: vec![ascii(0x9003, "2020:01:02 03:04:05"), ascii(0x9004, "2020:01:02 03:04:05"), undefined(0xea1c, &vec![0x5a; fill])],
            ..Tiff::new()
        }
        .build()
    };
    let block = with(MAX_JPEG_EXIF - 20 - with(0).len());
    assert_eq!(block.len(), MAX_JPEG_EXIF - 20);
    let file = jpeg(&[app1_exif(&block)]);
    let fail = |c: Changes| write_owned(&file, &c, &on());
    assert_eq!(
        fail(Changes { date_taken: Some(date("2021-01-01T00:00:00", Some(60), Some("123456789"))), ..Changes::default() }),
        Err(Error::TooMany("EXIF bytes for the date taken"))
    );
    assert_eq!(fail(Changes { place: Some(Some(place())), ..Changes::default() }), Err(Error::TooMany("EXIF bytes for the place")));
    assert_eq!(fail(Changes { caption: Some(Some("a".repeat(100))), ..Changes::default() }), Err(Error::TooMany("EXIF bytes for the caption")));
    // An edit in place still fits.
    let out = write(&file, &Changes { date_taken: Some(date("2021-01-01T00:00:00", None, None)), ..Changes::default() });
    assert_eq!(scan(&out, &l()).unwrap().exif.unwrap().len(), MAX_JPEG_EXIF - 20);
}

// §8 test 10: hostile input.

fn samples() -> Vec<Vec<u8>> {
    vec![rich(), jpeg(&[]), real_jpeg(&[app1_exif(&camera().build())]), jpeg(&[app13(&irb(&[(0x0404, &latin1_iptc())]))])]
}

#[test]
fn truncated_inputs_are_err_without_panicking() {
    for file in samples() {
        let sos = file.len() - segments(&file).1.len();
        for cut in 0..file.len() {
            let part = &file[..cut];
            let got = write_owned(part, &everything(), &on());
            if cut < sos + 2 {
                assert!(got.is_err(), "a file cut at {cut} of {} was written", file.len());
            } else if let Ok(out) = got {
                // Cut inside the scan data: the metadata is whole and the write verified.
                assert!(verify(part, &out, &everything(), &l()).is_ok());
            }
        }
    }
}

#[test]
fn a_file_whose_blocks_cannot_be_read_is_not_rewritten() {
    // An EXIF IFD that loops, XMP that is not well formed, a Photoshop block cut short.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x8769, LONG, 1, 8));
    for file in [jpeg(&[app1_exif(&t.build())]), jpeg(&[app1_xmp(b"<x:xmpmeta><unclosed")]), jpeg(&[app13(b"8BIM\x04\x04\0\0\0\0\xff\xff")])] {
        assert!(matches!(write_owned(&file, &everything(), &on()), Err(Error::Malformed(_))));
    }
    // A packet that reads (as nothing) but has no rdf:RDF to write into is refused, not replaced.
    let file = jpeg(&[app1_xmp(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>")]);
    assert!(matches!(write_owned(&file, &Changes { star: Some(true), ..Changes::default() }, &on()), Err(Error::Malformed(_))));
    assert!(read(&file, &l()).unwrap().problems.iter().all(|p| p.block != BlockKind::Xmp));
}

/// A fuzz finding: IFD0's only entry is an ImageDescription of four SRATIONALs (32 bytes) at
/// offset 0 of a 26-byte block, so its value runs past the block's end. The first date write
/// appends after the block, inside that range; the next write must still find its values there
/// rather than move them again.
#[test]
fn a_value_running_past_the_block_does_not_make_a_second_write_differ() {
    let block = b"II*\0\x08\0\0\0\x01\0\x0e\x01\x0a\0\x04\0\0\0\0\0\0\0\0\0\0\0";
    assert_eq!(block.len(), 26);
    let d = Changes { date_taken: DateTaken::parse_iso("2026-06-04T19:42:10.250+10:00"), ..Changes::default() };
    let edit = dalica_meta::exif::edit::ExifEdit::of(&d);
    let once = dalica_meta::exif::edit::edit(Some(block), &edit, MAX_JPEG_EXIF, &l()).unwrap().unwrap();
    let twice = dalica_meta::exif::edit::edit(Some(&once), &edit, MAX_JPEG_EXIF, &l()).unwrap().unwrap();
    assert_eq!(twice, once);
    // Through the JPEG writer too.
    let file = jpeg(&[app1_exif(block)]);
    let once = write(&file, &d);
    assert_eq!(write(&once, &d), once);
    assert_eq!(meta(&once).owned.date_taken, d.date_taken);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn random_mutations_never_panic_and_what_is_written_verifies(which in 0usize..4, edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8)) {
        let mut file = samples()[which].clone();
        for (at, byte) in edits {
            let n = file.len();
            file[at % n] = byte;
        }
        if let Ok(out) = write_owned(&file, &everything(), &on()) {
            prop_assert!(verify(&file, &out, &everything(), &l()).is_ok());
            prop_assert!(read(&out, &l()).is_ok());
        }
    }

    #[test]
    fn random_captions_and_tags_round_trip(caption in "\\PC{0,80}", tags in proptest::collection::vec("\\PC{1,20}", 0..6), noise in any::<bool>()) {
        // Now and then with the two noncharacters XML forbids, which every writer drops.
        let caption = if noise { format!("\u{fffe}{caption}\u{ffff}") } else { caption };
        let out = write_owned(&rich(), &Changes { caption: Some(Some(caption.clone())), tags: Some(tags.clone()), ..Changes::default() }, &on());
        prop_assert!(out.is_ok(), "{out:?}");
        let m = meta(&out.unwrap());
        prop_assert_eq!(m.owned.caption, clean_text(&caption, l().max_string));
        prop_assert_eq!(m.owned.tags, clean_tags(tags.iter().map(String::as_str), l().max_string, l().max_tags));
    }
}
