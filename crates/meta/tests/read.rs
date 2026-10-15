//! `dalica-meta` reading (`xmp-interop.md` §2 and §4, `security.md` §4.2): each owned field from
//! each source in each container, the precedence rules, IPTC text encodings, camera placeholder
//! descriptions, extended XMP, the listing, hostile EXIF structures, truncation and random bytes,
//! and the write skeleton with its verification.

use crate::build;

use crate::common::{cases, l};
use build::*;
use dalica_core::DateTime;
use dalica_meta::container::payload_digest;
use dalica_meta::exif::{self, ColorSpace, Rational};
use dalica_meta::fields::{ExifOwned, IptcOwned, XmpOwned};
use dalica_meta::{
    BlockKind, Changes, Container, DateSource, DateTaken, Error, Limits, OwnedFields, Place, Sources, WriteOptions, listing, read, scan, verify,
    write_owned,
};
use proptest::prelude::*;

const GUID: &str = "4F1E3C2A9B8D7E6F5A4B3C2D1E0F9A8B";

/// The EXIF block every container test carries: a caption, a southern/eastern place, a date with
/// offset and fraction, and the Properties summary tags.
fn exif_block(big_endian: bool) -> Vec<u8> {
    exif_tiff(big_endian).build()
}

fn exif_tiff(big_endian: bool) -> Tiff {
    Tiff {
        big_endian,
        ifd0: vec![ascii(0x010e, "Harbour at dusk"), ascii(0x010f, "Canon"), ascii(0x0110, "Canon EOS 5D"), short(0x0112, 6)],
        exif: vec![
            rationals(0x829a, &[(1, 250)]),
            rationals(0x829d, &[(28, 10)]),
            short(0x8827, 400),
            ascii(0x9003, "2026:06:04 19:42:10"),
            ascii(0x9004, "2026:06:04 19:42:11"),
            ascii(0x9011, "+10:00"),
            ascii(0x9291, "250"),
            short(0x9209, 1),
            rationals(0x920a, &[(35, 1)]),
            short(0xa001, 0xffff),
            ascii(0xa434, "EF 35mm f/1.4L"),
        ],
        interop: vec![ascii(0x0001, "R03")],
        gps: vec![
            bytes(0x0000, &[2, 3, 0, 0]),
            ascii(0x0001, "S"),
            rationals(0x0002, &[(33, 1), (51, 1), (315, 10)]),
            ascii(0x0003, "E"),
            rationals(0x0004, &[(151, 1), (12, 1), (40, 1)]),
        ],
        ifd1: vec![],
    }
}

fn expected_exif() -> ExifOwned {
    ExifOwned {
        caption: Some("Harbour at dusk".into()),
        place: Place::new(-(33.0 + 51.0 / 60.0 + 31.5 / 3600.0), 151.0 + 12.0 / 60.0 + 40.0 / 3600.0),
        date_taken: DateTaken::from_exif("2026:06:04 19:42:10", Some("+10:00"), Some("250")),
    }
}

/// The IPTC stream: UTF-8 declared, a caption, tags with `/`, `,` and non-ASCII, a date.
fn iptc_stream() -> Vec<u8> {
    iim(&[
        (1, 90, b"\x1b%G"),
        (2, 0, &[0, 4]),
        (2, 120, "Hafen am Abend — 港".as_bytes()),
        (2, 25, b"beach"),
        (2, 25, "summer/2026, Ünïcode".as_bytes()),
        (2, 25, b"Beach"),
        (2, 55, b"20250101"),
        (2, 60, b"120000+0100"),
        (2, 80, b"A. Photographer"),
    ])
}

fn expected_iptc() -> IptcOwned {
    IptcOwned {
        caption: Some("Hafen am Abend — 港".into()),
        keywords: vec!["beach".into(), "summer/2026, Ünïcode".into()],
        date_created: DateTaken::parse_iso("2025-01-01T12:00:00+01:00"),
    }
}

fn packet() -> Vec<u8> {
    xmp_packet("<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"/>")
}

/// One file of each container holding EXIF, IPTC (where the container can) and XMP.
fn every_container() -> Vec<(&'static str, Vec<u8>, bool)> {
    let exif = exif_block(false);
    let exif_be = exif_block(true);
    let iptc = iptc_stream();
    let ps = irb(&[(0x03ed, &[0; 16]), (0x0404, &iptc)]);
    let xmp = packet();
    let mut tiff = exif_tiff(false);
    tiff.ifd0.extend([undefined(0x02bc, &xmp), undefined(0x83bb, &iptc)]);
    let mut tiff_be = exif_tiff(true);
    tiff_be.ifd0.extend([bytes(0x02bc, &xmp), raw(0x83bb, LONG, 0, 0)]);
    vec![
        ("jpeg", jpeg(&[app1_exif(&exif), app1_xmp(&xmp), app2_icc(b"profile"), app13(&ps)]), true),
        ("jpeg big-endian", jpeg(&[app1_exif(&exif_be), app13(&ps), app1_xmp(&xmp)]), true),
        (
            "png",
            png(&[
                (b"eXIf", exif.clone()),
                (b"iTXt", itxt("XML:com.adobe.xmp", &xmp, false)),
                (b"zTXt", ztxt("Raw profile type iptc", &raw_profile("IPTC", &iptc))),
            ]),
            true,
        ),
        (
            "png raw profiles",
            png(&[
                (b"zTXt", ztxt("Raw profile type exif", &raw_profile("exif", &[&b"Exif\0\0"[..], &exif].concat()))),
                (b"iTXt", itxt("XML:com.adobe.xmp", &xmp, true)),
                (b"tEXt", text("Raw profile type 8bim", &raw_profile("8bim", &ps))),
            ]),
            true,
        ),
        ("webp", webp(&[(b"EXIF", exif.clone()), (b"XMP ", xmp.clone())]), false),
        ("webp with prefix", webp(&[(b"EXIF", [&b"Exif\0\0"[..], &exif].concat()), (b"XMP ", xmp.clone())]), false),
        ("tiff", tiff_file(tiff), true),
        ("tiff big-endian", tiff_file(tiff_be), false),
        (
            "png ImageMagick",
            png(&[
                (b"eXIf", exif.clone()),
                (b"iTXt", itxt("XML:com.adobe.xmp", &xmp, false)),
                (b"zTXt", ztxt("Raw profile type 8bim", &raw_profile("8bim", &iptc))),
            ]),
            true,
        ),
        ("tiff ImageMagick", tiff_file(imagemagick_tiff(&iptc)), true),
    ]
}

/// A TIFF as ImageMagick writes IPTC into it: a bare IIM stream in tag 34377, where Photoshop
/// resources belong, and no tag 33723.
fn imagemagick_tiff(iptc: &[u8]) -> Tiff {
    let mut tiff = exif_tiff(false);
    tiff.ifd0.extend([bytes(0x02bc, &packet()), bytes(0x8649, iptc)]);
    tiff
}

#[test]
fn imagemagick_bare_iptc_streams_read_as_iptc() {
    let iptc = iptc_stream();
    let png_file = png(&[(b"zTXt", ztxt("Raw profile type 8bim", &raw_profile("8bim", &iptc)))]);
    let tiff = tiff_file(imagemagick_tiff(&iptc));
    for (name, file) in [("png", &png_file), ("tiff", &tiff)] {
        let s = scan(file, &l()).unwrap();
        // Read as the bare stream it is, not as damaged Photoshop resources, and rewritable.
        assert_eq!(s.iptc.as_deref(), Some(&iptc[..]), "{name}");
        assert_eq!(s.photoshop, None, "{name}");
        assert!(s.spans.iter().any(|s| s.kind == BlockKind::Iptc && s.rewritable), "{name}");
        assert!(!s.spans.iter().any(|s| s.kind == BlockKind::Photoshop), "{name}");
        let m = read(file, &l()).unwrap();
        assert!(m.problems.is_empty(), "{name}: {:?}", m.problems);
        assert_eq!(m.sources.iptc, expected_iptc(), "{name}");
        let list = listing(file, &l()).unwrap();
        assert!(list.rows.iter().any(|r| r.group == "IPTC" && r.label == "Caption" && r.value == "Hafen am Abend — 港"), "{name}");
    }
    // Real resources under the same names still read as resources.
    let ps = irb(&[(0x0404, &iptc)]);
    let mut t = exif_tiff(false);
    t.ifd0.push(undefined(0x8649, &ps));
    for file in [png(&[(b"tEXt", text("Raw profile type iptc", &raw_profile("iptc", &ps)))]), tiff_file(t)] {
        let s = scan(&file, &l()).unwrap();
        assert!(s.photoshop.is_some() && s.iptc.is_none());
        assert_eq!(read(&file, &l()).unwrap().sources.iptc, expected_iptc());
    }
    // A TIFF with both: tag 33723 is the stream read; the copy in 34377 is IPTC too, and rewritable.
    let other = iim(&[(2, 120, b"the 34377 copy")]);
    let mut both = exif_tiff(false);
    both.ifd0.extend([undefined(0x83bb, &iptc), bytes(0x8649, &other)]);
    let both = tiff_file(both);
    let s = scan(&both, &l()).unwrap();
    assert_eq!(s.iptc.as_deref(), Some(&iptc[..]));
    assert_eq!(s.spans.iter().filter(|s| s.kind == BlockKind::Iptc && s.rewritable).count(), 2);
    // A second IPTC profile in a PNG is not read (the first one wins) but is rewritable, as in a
    // TIFF, so a write leaves no old caption in it.
    let two = png(&[
        (b"zTXt", ztxt("Raw profile type 8bim", &raw_profile("8bim", &iptc))),
        (b"zTXt", ztxt("Raw profile type iptc", &raw_profile("iptc", &other))),
    ]);
    let s = scan(&two, &l()).unwrap();
    assert_eq!(s.iptc.as_deref(), Some(&iptc[..]));
    assert_eq!(s.spans.iter().filter(|s| s.kind == BlockKind::Iptc && s.rewritable).count(), 2);
}

#[test]
fn each_field_from_each_source_in_each_container() {
    for (name, file, has_iptc) in every_container() {
        let s = scan(&file, &l()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(s.xmp.as_deref(), Some(&packet()[..]), "{name}: XMP packet");
        let m = read(&file, &l()).unwrap();
        assert!(m.problems.is_empty(), "{name}: {:?}", m.problems);
        assert_eq!(m.sources.exif, expected_exif(), "{name}: EXIF");
        assert_eq!(m.sources.xmp, XmpOwned::default(), "{name}: XMP (read from M6.9 on)");
        let summary = &m.summary;
        assert_eq!(summary.camera().as_deref(), Some("Canon EOS 5D"), "{name}");
        assert_eq!((summary.exposure_time, summary.f_number), (Rational::new(1, 250), Rational::new(28, 10)), "{name}");
        assert_eq!((summary.iso, summary.flash_text(), summary.orientation), (Some(400), Some("Fired"), Some(6)), "{name}");
        assert_eq!(summary.color_space, Some(ColorSpace::AdobeRgb), "{name}: uncalibrated + R03");
        assert_eq!(summary.lens.as_deref(), Some("EF 35mm f/1.4L"), "{name}");
        assert_eq!(summary.date_digitized.as_ref().map(ToString::to_string).as_deref(), Some("2026-06-04T19:42:11"), "{name}");
        let o = &m.owned;
        if has_iptc {
            assert_eq!(m.sources.iptc, expected_iptc(), "{name}: IPTC");
            // IPTC beats EXIF for the caption; tags come from IPTC.
            assert_eq!(o.caption, expected_iptc().caption, "{name}");
            assert_eq!(o.tags, expected_iptc().keywords, "{name}");
        } else {
            assert_eq!(m.sources.iptc, IptcOwned::default(), "{name}");
            assert_eq!(o.caption, expected_exif().caption, "{name}");
            assert!(o.tags.is_empty(), "{name}");
        }
        // EXIF beats IPTC for the date, and is the only place.
        assert_eq!((o.date_taken.clone(), o.date_source), (expected_exif().date_taken, Some(DateSource::Exif)), "{name}");
        assert_eq!(o.place, expected_exif().place, "{name}");
        assert_eq!(o.rating, None);
    }
}

#[test]
fn spans_cover_every_metadata_block() {
    for (name, file, _) in every_container() {
        let s = scan(&file, &l()).unwrap();
        let mut last = 0;
        for span in &s.spans {
            assert!(span.range.start >= last && span.range.end <= file.len(), "{name}: {span:?}");
            last = span.range.end;
        }
        let kinds: Vec<BlockKind> = s.spans.iter().filter(|s| s.rewritable).map(|s| s.kind).collect();
        assert!(kinds.contains(&BlockKind::Exif) || name.starts_with("tiff"), "{name}: {kinds:?}");
        assert!(kinds.contains(&BlockKind::Xmp), "{name}: {kinds:?}");
        // The pixels are outside every rewritable span.
        let outside: Vec<u8> = dalica_meta::container::payload(&file, &s.spans).concat();
        assert!(outside.windows(PIXELS.len()).any(|w| w == PIXELS), "{name}");
    }
    let j = jpeg(&[app1_exif(&exif_block(false)), app2_icc(b"p"), (COM, b"hello".to_vec())]);
    let kinds: Vec<(BlockKind, bool)> = scan(&j, &l()).unwrap().spans.iter().map(|s| (s.kind, s.rewritable)).collect();
    assert_eq!(kinds, [(BlockKind::Exif, true), (BlockKind::Icc, false), (BlockKind::Comment, false)]);
}

#[test]
fn precedence_rules() {
    let d = |s: &str| DateTaken::parse_iso(s);
    let place = |lat| Place::new(lat, 10.0);
    let exif = ExifOwned { caption: Some("exif".into()), place: place(1.0), date_taken: d("2001-01-01T00:00:00") };
    let iptc = IptcOwned { caption: Some("iptc".into()), keywords: vec!["i".into()], date_created: d("2005-01-01T00:00:00") };
    let xmp = XmpOwned {
        caption: Some("xmp".into()),
        tags: vec!["x".into()],
        rating: Some(3),
        place: place(2.0),
        date_time_original: d("2002-01-01T00:00:00"),
        date_created: d("2003-01-01T00:00:00"),
        create_date: d("2004-01-01T00:00:00"),
    };
    let all = Sources { exif: exif.clone(), iptc: iptc.clone(), xmp: xmp.clone() };
    let o = all.resolve();
    // Caption, tags and rating: XMP first. Place and date: EXIF first.
    assert_eq!((o.caption.as_deref(), o.tags.clone(), o.rating), (Some("xmp"), vec!["x".to_owned()], Some(3)));
    assert_eq!((o.place, o.date_source), (place(1.0), Some(DateSource::Exif)));
    assert!(!o.starred());
    // Remove sources one by one.
    let no_xmp_text = Sources { xmp: XmpOwned { caption: None, tags: vec![], ..xmp.clone() }, ..all.clone() };
    assert_eq!((no_xmp_text.resolve().caption.as_deref(), no_xmp_text.resolve().tags), (Some("iptc"), vec!["i".to_owned()]));
    let only_exif_caption = Sources { xmp: XmpOwned::default(), iptc: IptcOwned { caption: None, ..iptc.clone() }, exif: exif.clone() };
    assert_eq!(only_exif_caption.resolve().caption.as_deref(), Some("exif"));
    let no_exif = Sources { exif: ExifOwned::default(), ..all.clone() };
    assert_eq!((no_exif.resolve().place, no_exif.resolve().date_source), (place(2.0), Some(DateSource::XmpDateTimeOriginal)));
    let mut dates = no_exif.clone();
    dates.xmp.date_time_original = None;
    assert_eq!(dates.resolve().date_source, Some(DateSource::XmpDateCreated));
    dates.xmp.date_created = None;
    assert_eq!((dates.resolve().date_source, dates.resolve().date_taken), (Some(DateSource::XmpCreateDate), d("2004-01-01T00:00:00")));
    dates.xmp.create_date = None;
    assert_eq!((dates.resolve().date_source, dates.resolve().date_taken), (Some(DateSource::Iptc), d("2005-01-01T00:00:00")));
    dates.iptc.date_created = None;
    assert_eq!((dates.resolve().date_source, dates.resolve().date_taken), (None, None));
    assert!(Sources { xmp: XmpOwned { rating: Some(5), ..XmpOwned::default() }, ..Sources::default() }.resolve().starred());
    assert_eq!(Sources::default().resolve(), OwnedFields::default());

    // The same rules through a file: EXIF caption under an IPTC one, IPTC date without an EXIF one.
    let mut t = Tiff::new();
    t.ifd0.push(ascii(0x010e, "from exif"));
    let file = jpeg(&[app1_exif(&t.build()), app13(&irb(&[(0x0404, &iim(&[(2, 55, b"20240229")]))]))]);
    let o = read(&file, &l()).unwrap().owned;
    assert_eq!(o.caption.as_deref(), Some("from exif"));
    assert_eq!((o.date_source, o.date_taken.map(|d| d.to_string())), (Some(DateSource::Iptc), Some("2024-02-29T00:00:00".into())));
}

#[test]
fn iptc_latin1_and_utf8() {
    let caption = |stream: Vec<u8>| read(&jpeg(&[app13(&irb(&[(0x0404, &stream)]))]), &l()).unwrap().owned.caption;
    // No 1:90: valid UTF-8 is UTF-8, anything else is Latin-1.
    assert_eq!(caption(iim(&[(2, 120, "Café".as_bytes())])).as_deref(), Some("Café"));
    assert_eq!(caption(iim(&[(2, 120, b"Caf\xe9 cr\xe8me")])).as_deref(), Some("Café crème"));
    // 1:90 declares UTF-8: invalid bytes become replacement characters, never Latin-1.
    assert_eq!(caption(iim(&[(1, 90, b"\x1b%G"), (2, 120, b"Caf\xe9")])).as_deref(), Some("Caf\u{fffd}"));
    assert_eq!(caption(iim(&[(1, 90, b"\x1b%G"), (2, 120, "Grüße".as_bytes())])).as_deref(), Some("Grüße"));
    // Another 1:90 (here ISO 646) falls back to the guess.
    assert_eq!(caption(iim(&[(1, 90, b"\x1b(B"), (2, 120, b"na\xefve")])).as_deref(), Some("naïve"));
    // Keywords in Latin-1, control characters dropped, duplicates (any case) once.
    let k =
        read(&jpeg(&[app13(&irb(&[(0x0404, &iim(&[(2, 25, b"pr\xe9"), (2, 25, b"PR\xc9"), (2, 25, b"a\x07b"), (2, 25, b"  ")]))]))]), &l()).unwrap();
    assert_eq!(k.owned.tags, ["pré", "ab"]);
}

#[test]
fn a_camera_placeholder_description_is_ignored() {
    for placeholder in ["OLYMPUS DIGITAL CAMERA         ", "SONY DSC", "", "   ", "NIKON"] {
        let mut t = Tiff::new();
        t.ifd0.extend([ascii(0x010e, placeholder), ascii(0x010f, "NIKON")]);
        let m = read(&jpeg(&[app1_exif(&t.build())]), &l()).unwrap();
        assert_eq!(m.sources.exif.caption, None, "{placeholder:?}");
        assert_eq!(m.owned.caption, None);
    }
    let mut t = Tiff::new();
    t.ifd0.extend([ascii(0x010e, "Grandma's 90th"), ascii(0x010f, "NIKON")]);
    assert_eq!(read(&jpeg(&[app1_exif(&t.build())]), &l()).unwrap().owned.caption.as_deref(), Some("Grandma's 90th"));
    // A Latin-1 description.
    let mut t = Tiff::new();
    t.ifd0.push(ascii_bytes(0x010e, b"Caf\xe9"));
    assert_eq!(read(&jpeg(&[app1_exif(&t.build())]), &l()).unwrap().owned.caption.as_deref(), Some("Café"));
}

fn extended_file(main_guid: Option<&str>, chunks: &[(&str, u32, u32, &[u8])]) -> Vec<u8> {
    let body = match main_guid {
        Some(g) => format!("<rdf:Description xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\" xmpNote:HasExtendedXMP=\"{g}\"/>"),
        None => String::new(),
    };
    let mut segs = vec![app1_xmp(&xmp_packet(&body))];
    segs.extend(chunks.iter().map(|&(g, full, off, data)| app1_extended(g, full, off, data)));
    jpeg(&segs)
}

#[test]
fn extended_xmp_is_reassembled() {
    let ext = b"<x:xmpmeta>extended part, in two chunks</x:xmpmeta>";
    let (a, b) = ext.split_at(20);
    let full = ext.len() as u32;
    // The GUID is the packet's MD5.
    let guid = String::from_utf8(dalica_meta::container::jpeg::extended_guid(ext).to_vec()).unwrap();
    let guid = guid.as_str();
    // Out of order, the GUID in lower case in the chunks, plus a foreign chunk that is ignored.
    let file =
        extended_file(Some(guid), &[(&guid.to_lowercase(), full, 20, b), ("00000000000000000000000000000000", 5, 0, b"other"), (guid, full, 0, a)]);
    let s = scan(&file, &l()).unwrap();
    assert_eq!(s.extended_xmp.as_deref(), Some(&ext[..]));
    assert!(s.problems.is_empty());
    let kinds: Vec<_> = s.spans.iter().map(|s| (s.kind, s.rewritable)).collect();
    assert_eq!(kinds, [(BlockKind::Xmp, true), (BlockKind::ExtendedXmp, false), (BlockKind::ExtendedXmp, false), (BlockKind::ExtendedXmp, false)]);
    // Exactly at the limit (standard + extended) is accepted.
    let main_len = scan(&file, &l()).unwrap().xmp.unwrap().len();
    let at_limit = Limits { max_xmp: main_len + ext.len(), ..l() };
    assert!(scan(&file, &at_limit).unwrap().extended_xmp.is_some());
    let over = Limits { max_xmp: main_len + ext.len() - 1, ..l() };
    let s = scan(&file, &over).unwrap();
    assert_eq!((s.extended_xmp, s.problems[0].error.clone()), (None, Error::TooMany("XMP bytes")));
}

#[test]
fn an_oversized_or_mismatched_extended_xmp_is_refused() {
    let refused = |file: &[u8]| {
        let s = scan(file, &l()).unwrap();
        assert_eq!(s.extended_xmp, None);
        assert_eq!(s.problems.len(), 1);
        assert_eq!(s.problems[0].block, BlockKind::ExtendedXmp);
        // The rest of the file still reads, and the problem is reported.
        let m = read(file, &l()).unwrap();
        assert_eq!(m.problems.len(), 1);
        s.problems[0].error.clone()
    };
    // Declares 1 GiB.
    assert_eq!(refused(&extended_file(Some(GUID), &[(GUID, 1 << 30, 0, b"abc")])), Error::TooMany("XMP bytes"));
    // A chunk past the declared length.
    assert_eq!(refused(&extended_file(Some(GUID), &[(GUID, 4, 2, b"abc")])), Error::OutOfRange);
    assert_eq!(refused(&extended_file(Some(GUID), &[(GUID, 4, u32::MAX, b"abc")])), Error::OutOfRange);
    // The GUID does not match the declaration; no declaration at all.
    let other = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    assert!(matches!(refused(&extended_file(Some(GUID), &[(other, 3, 0, b"abc")])), Error::Malformed(_)));
    // Whole and declared, but the GUID is not the packet's MD5 (another packet's chunks).
    assert!(matches!(refused(&extended_file(Some(GUID), &[(GUID, 3, 0, b"abc")])), Error::Malformed(_)));
    assert!(matches!(refused(&extended_file(None, &[(GUID, 3, 0, b"abc")])), Error::Malformed(_)));
    // Chunks that disagree on the length, leave a gap, or stop short.
    assert!(matches!(refused(&extended_file(Some(GUID), &[(GUID, 6, 0, b"abc"), (GUID, 7, 3, b"def")])), Error::Malformed(_)));
    assert!(matches!(refused(&extended_file(Some(GUID), &[(GUID, 6, 0, b"ab"), (GUID, 6, 3, b"def")])), Error::Malformed(_)));
    assert!(matches!(refused(&extended_file(Some(GUID), &[(GUID, 6, 0, b"abc")])), Error::Malformed(_)));
    // A chunk header cut short.
    let file = jpeg(&[app1_xmp(&xmp_packet("")), (APP1, b"http://ns.adobe.com/xmp/extension/\0ABC".to_vec())]);
    assert!(matches!(refused(&file), Error::Malformed(_)));
}

#[test]
fn the_listing() {
    let mut t = exif_tiff(false);
    t.ifd0.push(undefined(0x927c, &[1; 300]));
    t.exif.extend([undefined(0x9000, b"0232"), undefined(0x9286, b"ASCII\0\0\0a comment"), srational(0x9204, -1, 3)]);
    t.ifd1 = vec![long(0x0201, 0), long(0x0202, 0)];
    let file = jpeg(&[app1_exif(&t.build()), app13(&irb(&[(0x0404, &iptc_stream())])), (COM, b"made by hand\x1b".to_vec())]);
    let list = listing(&file, &l()).unwrap();
    assert!(!list.truncated);
    let row = |group: &str, label: &str| list.rows.iter().find(|r| r.group == group && r.label == label).map(|r| r.value.as_str());
    assert_eq!(row("EXIF", "Make"), Some("Canon"));
    assert_eq!(row("EXIF", "Exposure time"), Some("1/250"));
    assert_eq!(row("EXIF", "EXIF version"), Some("0232"));
    assert_eq!(row("EXIF", "User comment"), Some("a comment"));
    assert_eq!(row("EXIF", "Exposure bias"), Some("-1/3"));
    assert_eq!(row("EXIF", "Maker note"), Some("(300 bytes)"));
    assert_eq!(row("EXIF", "Date taken"), Some("2026:06:04 19:42:10"));
    assert_eq!(row("EXIF interoperability", "Interoperability index"), Some("R03"));
    assert_eq!(row("GPS", "Latitude reference"), Some("S"));
    assert_eq!(row("GPS", "Latitude"), Some("33/1 51/1 315/10"));
    assert_eq!(row("GPS", "GPS version"), Some("2 3 0 0"));
    assert_eq!(row("EXIF thumbnail", "Thumbnail length"), Some("0"));
    assert_eq!(row("IPTC", "Coded character set"), Some("UTF-8"));
    assert_eq!(row("IPTC", "Record version"), Some("4"));
    assert_eq!(row("IPTC", "Caption"), Some("Hafen am Abend — 港"));
    assert_eq!(list.rows.iter().filter(|r| r.label == "Keywords").count(), 3);
    assert_eq!(row("Comment", "Comment"), Some("made by hand "));
    // EXIF rows come first, in IFD order: IFD0 then the EXIF IFD.
    assert_eq!(list.rows[0].label, "Image description");
    // PNG text chunks are listed by keyword.
    let p = png(&[(b"tEXt", text("Title", b"Sunset")), (b"zTXt", ztxt("Comment", b"zipped")), (b"iTXt", itxt("Author", "Zoë".as_bytes(), true))]);
    let list = listing(&p, &l()).unwrap();
    let rows: Vec<(&str, &str, &str)> = list.rows.iter().map(|r| (r.group.as_str(), r.label.as_str(), r.value.as_str())).collect();
    assert_eq!(rows, [("PNG text", "Title", "Sunset"), ("PNG text", "Comment", "zipped"), ("PNG text", "Author", "Zoë")]);
}

#[test]
fn the_listing_is_bounded() {
    let mut t = Tiff::new();
    t.ifd0 = (0..500u16).map(|i| short(0x9000 + i, i)).collect();
    t.ifd0.push(undefined(0xc000, &[b'x'; 64]));
    t.ifd0.push(ascii(0xc001, &"y".repeat(5000)));
    t.ifd0.push(F::Value { tag: 0xc002, kind: SHORT, count: 100, words: vec![Word::U16(7); 100] });
    let file = jpeg(&[app1_exif(&t.build())]);
    let all = listing(&file, &l()).unwrap();
    assert_eq!((all.rows.len(), all.truncated), (503, false));
    let long = all.rows.iter().find(|r| r.label == "Tag 0xc001").unwrap();
    assert_eq!(long.value.chars().count(), l().max_row_value + 1, "cut, with an ellipsis");
    let many = all.rows.iter().find(|r| r.label == "Tag 0xc002").unwrap();
    assert!(many.value.ends_with(" …") && many.value.split(' ').count() == 17, "{}", many.value);
    let few = listing(&file, &Limits { max_rows: 10, ..l() }).unwrap();
    assert_eq!((few.rows.len(), few.truncated), (10, true));
    // An unreadable EXIF block is one row, not a failure.
    let mut bad = Tiff::new();
    bad.ifd0.push(raw(0x8769, LONG, 1, 8));
    let list = listing(&jpeg(&[app1_exif(&bad.build())]), &l()).unwrap();
    assert_eq!(list.rows.len(), 1);
    assert_eq!((list.rows[0].group.as_str(), list.rows[0].label.as_str()), ("EXIF", "Unreadable"));
}

#[test]
fn ifd_loops_offsets_past_the_end_and_huge_counts_are_err() {
    let read_exif = |t: &Tiff| exif::read::read(&t.build(), &l());
    // The EXIF IFD pointing back at IFD0; IFD1 pointing at itself.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x8769, LONG, 1, 8));
    assert_eq!(read_exif(&t), Err(Error::Loop));
    let mut t = Tiff::new();
    t.ifd0.push(ascii(0x010f, "x"));
    let selfish = t.build();
    // IFD0 (one entry, at 8) ends at 8 + 2 + 12 + 4: make its next pointer IFD0 itself.
    let mut looped = selfish.clone();
    looped[8 + 2 + 12..8 + 2 + 12 + 4].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(exif::read::read(&looped, &l()), Err(Error::Loop));
    // A string past the end; a count whose size is far beyond the block; an IFD past the end.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x010e, ASCII, 20, 10_000));
    assert_eq!(read_exif(&t), Err(Error::OutOfRange));
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x010e, ASCII, u32::MAX, 8));
    assert_eq!(read_exif(&t), Err(Error::OutOfRange));
    let mut t = Tiff::new();
    t.exif.push(raw(0x829a, RATIONAL, 0x2000_0000, 8));
    assert_eq!(read_exif(&t), Err(Error::OutOfRange));
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x8825, LONG, 1, 0xffff_0000));
    assert_eq!(read_exif(&t), Err(Error::OutOfRange));
    // More entries than an IFD may hold, at the limit and one past it.
    let entries = |n: u16| {
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend(n.to_le_bytes());
        b.extend(vec![0u8; usize::from(n) * 12 + 4]);
        b
    };
    let small = Limits { max_ifd_entries: 100, ..l() };
    assert!(exif::read::read(&entries(100), &small).is_ok());
    assert_eq!(exif::read::read(&entries(101), &small), Err(Error::TooMany("IFD entries")));
    assert_eq!(exif::read::read(b"II*\0\x08\0\0\0\xff\xff", &l()), Err(Error::TooMany("IFD entries")));
    // Depth: the interoperability IFD is the third level.
    assert!(exif::read::read(&exif_block(false), &Limits { max_ifd_depth: 3, ..l() }).is_ok());
    assert_eq!(exif::read::read(&exif_block(false), &Limits { max_ifd_depth: 2, ..l() }), Err(Error::TooMany("IFD depth")));
    // Not a TIFF header at all.
    assert!(exif::read::read(b"XX*\0\x08\0\0\0", &l()).is_err());
    // Invalid values are left out, not errors: zero denominators, a latitude past 90°.
    let mut t = Tiff::new();
    t.gps = vec![rationals(0x0002, &[(33, 0), (0, 1), (0, 1)]), rationals(0x0004, &[(10, 1)])];
    assert_eq!(read_exif(&t).unwrap().owned.place, None);
    let mut t = Tiff::new();
    t.gps = vec![rationals(0x0002, &[(91, 1)]), rationals(0x0004, &[(10, 1)])];
    assert_eq!(read_exif(&t).unwrap().owned.place, None);
    let mut t = Tiff::new();
    t.exif = vec![ascii(0x9003, "    :  :     :  :  "), rationals(0x829a, &[(1, 0)])];
    let e = read_exif(&t).unwrap();
    assert_eq!((e.owned.date_taken, e.summary.exposure_time), (None, None));
    // In a file, a broken EXIF block is a problem; the IPTC beside it still reads.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x8769, LONG, 1, 8));
    let file = jpeg(&[app1_exif(&t.build()), app13(&irb(&[(0x0404, &iptc_stream())]))]);
    let m = read(&file, &l()).unwrap();
    assert_eq!(m.problems.iter().map(|p| (p.block, p.error.clone())).collect::<Vec<_>>(), [(BlockKind::Exif, Error::Loop)]);
    assert_eq!(m.owned.caption, expected_iptc().caption);
}

#[test]
fn container_limits_and_damage() {
    // Segments and chunks are counted.
    let segs: Vec<(u8, Vec<u8>)> = (0..20).map(|_| (COM, b"c".to_vec())).collect();
    let file = jpeg(&segs);
    assert!(scan(&file, &Limits { max_segments: 23, ..l() }).is_ok());
    assert_eq!(scan(&file, &Limits { max_segments: 22, ..l() }), Err(Error::TooMany("JPEG segments")));
    let chunks: Vec<(&[u8; 4], Vec<u8>)> = (0..20).map(|_| (b"tEXt", text("k", b"v"))).collect();
    let file = png(&chunks);
    assert!(scan(&file, &Limits { max_segments: 23, ..l() }).is_ok());
    assert_eq!(scan(&file, &Limits { max_segments: 22, ..l() }), Err(Error::TooMany("PNG chunks")));
    // IPTC dataset size and count, at the limit and past it.
    let big = vec![b'a'; 40_000];
    let stream = iim(&[(2, 120, &big)]);
    let small = Limits { max_dataset: 40_000, ..l() };
    assert!(dalica_meta::iptc::read::read(&stream, &small).is_ok());
    assert_eq!(dalica_meta::iptc::read::read(&stream, &Limits { max_dataset: 39_999, ..l() }), Err(Error::TooMany("IPTC dataset bytes")));
    let many = iim(&vec![(2u8, 25u8, &b"k"[..]); 50]);
    assert!(dalica_meta::iptc::read::read(&many, &Limits { max_iptc_datasets: 50, ..l() }).is_ok());
    assert_eq!(dalica_meta::iptc::read::read(&many, &Limits { max_iptc_datasets: 49, ..l() }), Err(Error::TooMany("IPTC datasets")));
    // A compressed text chunk past the inflate cap is a problem, not a failure.
    let file = png(&[(b"zTXt", ztxt("Comment", &[b'z'; 5000]))]);
    let s = scan(&file, &Limits { max_inflate: 4999, ..l() }).unwrap();
    assert_eq!(s.problems[0].error, Error::TooMany("inflated text bytes"));
    assert_eq!(scan(&file, &Limits { max_inflate: 5000, ..l() }).unwrap().texts[0].value.len(), 5000);
    // A Photoshop block whose resource runs past its end is a problem; the EXIF still reads.
    let mut ps = irb(&[(0x0404, &iptc_stream())]);
    ps.truncate(ps.len() - 3);
    let m = read(&jpeg(&[app1_exif(&exif_block(false)), app13(&ps)]), &l()).unwrap();
    assert_eq!(m.problems.iter().map(|p| p.block).collect::<Vec<_>>(), [BlockKind::Photoshop]);
    assert_eq!(m.sources.exif, expected_exif());
    // Damaged structure: a JPEG without a scan, a PNG without IEND, a RIFF size past the end.
    assert_eq!(scan(b"\xff\xd8\xff\xe1\x00\x02", &l()), Err(Error::Truncated));
    assert!(matches!(scan(b"\xff\xd8\xff\xe1\x00\x01\xff\xda", &l()), Err(Error::Malformed(_))));
    assert!(matches!(scan(b"\xff\xd8\xff\xe1\x00\x02\x00", &l()), Err(Error::Malformed(_))));
    let p = png(&[]);
    assert_eq!(scan(&p[..p.len() - 12], &l()), Err(Error::Truncated));
    let mut w = webp(&[]);
    w[4] = w[4].wrapping_add(10);
    assert_eq!(scan(&w, &l()), Err(Error::Truncated));
    assert!(scan(&webp_simple(), &l()).unwrap().exif.is_none());
}

/// Every file each test builds, for the hostile-input sweeps.
fn samples() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = every_container().into_iter().map(|(_, f, _)| f).collect();
    let ext = b"0123456789abcdef";
    let guid = String::from_utf8(dalica_meta::container::jpeg::extended_guid(ext).to_vec()).unwrap();
    out.push(extended_file(Some(&guid), &[(&guid, 16, 0, &ext[..8]), (&guid, 16, 8, &ext[8..])]));
    out.push(png(&[(b"zTXt", ztxt("Comment", b"zipped")), (b"iTXt", itxt("Author", b"me", true))]));
    out
}

/// Every file cut at every offset and flipped at every byte reads, lists and writes or refuses,
/// never panics.
#[test]
fn truncation_and_byte_flips_never_panic() {
    for file in samples() {
        for at in 0..file.len() {
            let mut f = file.clone();
            f[at] ^= 0xa5;
            let _ = read(&f, &l());
            let _ = listing(&f, &l());
        }
        for cut in 0..file.len() {
            let part = &file[..cut];
            let _ = read(part, &l());
            let _ = listing(part, &l());
            let _ = write_owned(part, &Changes { caption: Some(Some("x".into())), ..Changes::default() }, &WriteOptions::default());
        }
        // A cut inside the metadata is an error or a partial read, never a panic; the full file reads.
        assert!(read(&file, &l()).is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    #[test]
    fn random_bytes_never_panic(prefix in 0usize..6, body in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let magic: [&[u8]; 6] = [b"\xff\xd8\xff", b"\x89PNG\r\n\x1a\n", b"RIFF\xff\x0f\0\0WEBP", b"II*\0\x08\0\0\0", b"MM\0*\0\0\0\x08", b""];
        let file = [magic[prefix], &body].concat();
        let _ = read(&file, &l());
        let _ = listing(&file, &l());
        let _ = exif::read::read(&file, &l());
        let _ = dalica_meta::iptc::read::read(&body, &l());
        let _ = dalica_meta::iptc::read::resources(&body, &l());
        let _ = write_owned(&file, &Changes { star: Some(true), ..Changes::default() }, &WriteOptions { stars_as_ratings: true, ..WriteOptions::default() });
    }
}

#[test]
fn write_owned_writes_every_container_and_skips_empty_changes() {
    let caption = Changes { caption: Some(Some("new".into())), ..Changes::default() };
    for (name, file, _) in every_container() {
        // Every container is written (JPEG: tests/write_jpeg.rs; PNG, WebP, TIFF: tests/write_png_webp_tiff.rs).
        assert_eq!(
            read(&write_owned(&file, &caption, &WriteOptions::default()).unwrap(), &l()).unwrap().owned.caption.as_deref(),
            Some("new"),
            "{name}"
        );
        // Nothing to write: the same bytes back.
        assert_eq!(write_owned(&file, &Changes::default(), &WriteOptions::default()).as_deref(), Ok(&file[..]), "{name}");
        // Options off: the change is dropped, so nothing is written.
        let off = WriteOptions { captions_tags_places: false, ..WriteOptions::default() };
        assert_eq!(write_owned(&file, &caption, &off).as_deref(), Ok(&file[..]), "{name}");
    }
    // Stars only with their option (off by default), dates with theirs.
    let file = jpeg(&[]);
    let star = Changes { star: Some(true), ..Changes::default() };
    assert_eq!(write_owned(&file, &star, &WriteOptions::default()).as_deref(), Ok(&file[..]));
    let starred = write_owned(&file, &star, &WriteOptions { stars_as_ratings: true, ..WriteOptions::default() }).unwrap();
    assert_eq!(read(&starred, &l()).unwrap().owned.rating, Some(5));
    let date = Changes { date_taken: DateTaken::parse_iso("2020-02-02T02:02:02"), ..Changes::default() };
    assert_eq!(write_owned(&file, &date, &WriteOptions { dates: false, ..WriteOptions::default() }).as_deref(), Ok(&file[..]));
    // A file whose EXIF cannot be read is not rewritten; an unknown format is refused.
    let mut t = Tiff::new();
    t.ifd0.push(raw(0x8769, LONG, 1, 8));
    assert!(matches!(write_owned(&jpeg(&[app1_exif(&t.build())]), &caption, &WriteOptions::default()), Err(Error::Malformed(_))));
    assert_eq!(write_owned(b"GIF89a....", &caption, &WriteOptions::default()), Err(Error::Unknown));
    // A TIFF-structured raw is never written, whatever its writer can do.
    // A CR2 header: TIFF, with "CR" 2 0 after the IFD0 pointer, and an empty IFD0 at 16.
    let cr2 = b"II*\0\x10\0\0\0CR\x02\0\0\0\0\0\0\0\0\0\0\0".to_vec();
    assert!(matches!(dalica_image::sniff(&cr2), Some(dalica_image::Format::Raw(_))));
    assert_eq!(scan(&cr2, &l()).unwrap().container, Container::Tiff);
    assert_eq!(write_owned(&cr2, &caption, &WriteOptions::default()), Err(Error::Unsupported(Container::Tiff)));
}

/// A JPEG whose IPTC caption and EXIF fields are as given.
fn jpeg_with(caption: &str, exif: &Tiff, scan_data: &[u8]) -> Vec<u8> {
    jpeg_with_scan(
        &[app1_exif(&exif.build()), app2_icc(b"profile"), app13(&irb(&[(0x03ed, &[1; 16]), (0x0404, &iim(&[(2, 120, caption.as_bytes())]))]))],
        scan_data,
    )
}

#[test]
fn verification_of_a_write() {
    let mut t = Tiff::new();
    t.ifd0.push(ascii(0x010f, "Canon"));
    let before = jpeg_with("old", &t, PIXELS);
    let want = Changes { caption: Some(Some("  new caption ".into())), ..Changes::default() };
    // A good write: only the rewritable segments differ (here IPTC grew and EXIF gained a place).
    let mut t2 = t.clone();
    t2.gps = vec![ascii(1, "N"), rationals(2, &[(10, 1), (30, 1), (0, 1)]), ascii(3, "W"), rationals(4, &[(20, 1), (15, 1), (0, 1)])];
    let after = jpeg_with("new caption", &t2, PIXELS);
    assert_eq!(verify(&before, &after, &want, &l()), Ok(()));
    let place = Changes { place: Some(Place::new(10.5, -20.25)), ..want.clone() };
    assert_eq!(verify(&before, &after, &place, &l()), Ok(()));
    let a = scan(&before, &l()).unwrap();
    let b = scan(&after, &l()).unwrap();
    assert_eq!(payload_digest(&before, &a.spans), payload_digest(&after, &b.spans));
    // The pixels changed; the ICC profile (not rewritable) changed.
    assert!(matches!(verify(&before, &jpeg_with("new caption", &t2, b"other pixels"), &want, &l()), Err(Error::Verify(_))));
    let icc = jpeg_with_scan(
        &[app1_exif(&t2.build()), app2_icc(b"PROFILE"), app13(&irb(&[(0x03ed, &[1; 16]), (0x0404, &iim(&[(2, 120, b"new caption")]))]))],
        PIXELS,
    );
    assert_eq!(verify(&before, &icc, &want, &l()), Err(Error::Verify("bytes outside the metadata changed".into())));
    // A field that does not read back as asked; the message names the field, not the value.
    assert_eq!(
        verify(&before, &after, &Changes { caption: Some(Some("other".into())), ..Changes::default() }, &l()),
        Err(Error::Verify("the caption does not read back as written".into()))
    );
    assert!(
        matches!(verify(&before, &after, &Changes { place: Some(None), ..Changes::default() }, &l()), Err(Error::Verify(m)) if m.contains("place"))
    );
    assert!(
        matches!(verify(&before, &after, &Changes { tags: Some(vec!["a".into()]), ..Changes::default() }, &l()), Err(Error::Verify(m)) if m.contains("tags"))
    );
    assert!(
        matches!(verify(&before, &after, &Changes { date_taken: DateTaken::parse_iso("2020-01-01"), ..Changes::default() }, &l()), Err(Error::Verify(m)) if m.contains("date"))
    );
    assert!(
        matches!(verify(&before, &after, &Changes { star: Some(true), ..Changes::default() }, &l()), Err(Error::Verify(m)) if m.contains("star"))
    );
    // Unstarring a file Dalica never starred must leave its rating alone (no rating here: none).
    assert_eq!(verify(&before, &after, &Changes { star: Some(false), ..Changes::default() }, &l()), Ok(()));
    // A different container, a written file that no longer reads, a broken rewritten block.
    assert!(matches!(verify(&before, &png(&[]), &want, &l()), Err(Error::Verify(_))));
    assert!(matches!(verify(&before, &after[..after.len() / 2], &want, &l()), Err(Error::Verify(_))));
    let mut bad = Tiff::new();
    bad.ifd0.push(raw(0x8769, LONG, 1, 8));
    let broken = jpeg_with("new caption", &bad, PIXELS);
    assert!(matches!(verify(&before, &broken, &want, &l()), Err(Error::Verify(m)) if m.contains("Exif")));
}

#[test]
fn verification_of_an_append_only_tiff_write() {
    let iptc = iim(&[(2, 120, b"old")]);
    let mut t = Tiff::new();
    t.ifd0.push(undefined(0x83bb, &iptc));
    let before = tiff_file(t);
    assert_eq!(read(&before, &l()).unwrap().owned.caption.as_deref(), Some("old"));
    // Append the new IPTC and repoint the 33723 entry at it: the old bytes stay where they were.
    let new = iim(&[(2, 120, b"brand new caption")]);
    let mut after = before.clone();
    let at = after.len() as u32;
    after.extend(&new);
    let count_at = entry_value_field(&after, 0x83bb) - 4;
    after[count_at..count_at + 4].copy_from_slice(&(new.len() as u32).to_le_bytes());
    after[count_at + 4..count_at + 8].copy_from_slice(&at.to_le_bytes());
    let want = Changes { caption: Some(Some("brand new caption".into())), ..Changes::default() };
    assert_eq!(verify(&before, &after, &want, &l()), Ok(()));
    // Rewriting a strip byte in place is caught; so is a shorter file.
    let mut moved = after.clone();
    let strip = before.windows(PIXELS.len()).position(|w| w == PIXELS).unwrap();
    moved[strip] ^= 1;
    assert_eq!(verify(&before, &moved, &want, &l()), Err(Error::Verify("bytes outside the metadata changed".into())));
}

/// Where the value field of `tag` lies in a little-endian TIFF's IFD0.
fn entry_value_field(b: &[u8], tag: u16) -> usize {
    let ifd0 = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let n = u16::from_le_bytes(b[ifd0..ifd0 + 2].try_into().unwrap()) as usize;
    (0..n).map(|i| ifd0 + 2 + 12 * i).find(|&e| u16::from_le_bytes(b[e..e + 2].try_into().unwrap()) == tag).unwrap() + 8
}

#[test]
fn dates_with_offsets_and_fractions_and_places_west() {
    let mut t = Tiff::new();
    t.exif = vec![ascii(0x9003, "1999:12:31 23:59:59"), ascii(0x9011, "-03:30"), ascii(0x9291, "007")];
    t.gps = vec![ascii(1, "N"), rationals(2, &[(40, 1), (42, 1), (4608, 100)]), ascii(3, "W"), rationals(4, &[(74, 1), (0, 1), (2160, 100)])];
    let o = read(&png(&[(b"eXIf", t.build())]), &l()).unwrap().owned;
    let date = o.date_taken.unwrap();
    assert_eq!(date.to_string(), "1999-12-31T23:59:59.007-03:30");
    assert_eq!(date.date_time(), "1999-12-31T23:59:59".parse::<DateTime>().unwrap());
    assert_eq!((date.offset_minutes(), date.subsec(), date.offset_text().as_deref()), (Some(-210), Some("007"), Some("-03:30")));
    let p = o.place.unwrap();
    assert!((p.latitude() - 40.712_8).abs() < 1e-6 && (p.longitude() + 74.006).abs() < 1e-6, "{p}");
    // A missing reference counts as north/east.
    let mut t = Tiff::new();
    t.gps = vec![rationals(2, &[(1, 2)]), rationals(4, &[(3, 4)])];
    assert_eq!(read(&jpeg(&[app1_exif(&t.build())]), &l()).unwrap().owned.place, Place::new(0.5, 0.75));
}
