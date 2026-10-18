//! EXIF edited in place or appended (`xmp-interop.md` §2–§3 and §8 tests 1, 2, 3, 7 and 10): dates
//! with offsets and sub-seconds, southern and western places, removing a place, the caption's
//! ImageDescription copy, a maker note with absolute offsets that reads the same after a write,
//! the APP1 size limit, and hostile blocks that never panic.

use crate::build;

use crate::common::{cases, date, l};
use build::*;
use dalica_image::ifd::{Tiff as Ifds, Visited};
use dalica_meta::exif::edit::{ExifEdit, MAX_JPEG_EXIF, edit};
use dalica_meta::exif::read::{read, walk};
use dalica_meta::exif::{IfdKind, TAG_GPS_IFD};
use dalica_meta::write::PLACE_TOLERANCE;
use dalica_meta::{Changes, Error, Place};
use proptest::prelude::*;

const MAKER_NOTE: u16 = 0x927c;

/// A camera's EXIF block: a caption in ImageDescription, a date with offset and fraction, a GPS
/// IFD with altitude and time stamp besides the place, an interoperability IFD and a thumbnail.
fn camera(big_endian: bool) -> Tiff {
    Tiff {
        big_endian,
        ifd0: vec![ascii(0x010e, "Harbour at dusk"), ascii(0x010f, "Canon"), ascii(0x0110, "Canon EOS 5D"), short(0x0112, 6)],
        exif: vec![
            rationals(0x829a, &[(1, 250)]),
            short(0x8827, 400),
            ascii(0x9003, "2026:06:04 19:42:10"),
            ascii(0x9004, "2026:06:04 19:42:11"),
            ascii(0x9011, "+10:00"),
            ascii(0x9291, "250"),
            ascii(0xa434, "EF 35mm f/1.4L"),
        ],
        gps: vec![
            bytes(0x0000, &[2, 2, 0, 0]),
            ascii(0x0001, "N"),
            rationals(0x0002, &[(48, 1), (51, 1), (2412, 100)]),
            ascii(0x0003, "E"),
            rationals(0x0004, &[(2, 1), (17, 1), (4020, 100)]),
            bytes(0x0005, &[0]),
            rationals(0x0006, &[(35, 1)]),
            rationals(0x0007, &[(17, 1), (42, 1), (10, 1)]),
        ],
        interop: vec![ascii(0x0001, "R98")],
        ifd1: vec![short(0x0103, 6), F::OffsetTo { tag: 0x0201, bytes: b"\xff\xd8 thumbnail \xff\xd9".to_vec() }, long(0x0202, 15)],
    }
}

fn change(edit: ExifEdit, block: &[u8]) -> Vec<u8> {
    dalica_meta::exif::edit::edit(Some(block), &edit, usize::MAX, &l()).unwrap().unwrap()
}

/// One entry: its IFD, tag, type, count, value bytes and, when out of line, where they lie.
type Seen = (IfdKind, u16, u16, u32, Vec<u8>, Option<std::ops::Range<usize>>);

/// Every entry of every IFD.
fn entries(block: &[u8]) -> Vec<Seen> {
    let w = walk(block, &l()).unwrap();
    let mut out = Vec::new();
    for (kind, ifd) in &w.ifds {
        for e in &ifd.entries {
            let value = w.tiff.value(e).unwrap().map(|v| v.to_vec()).unwrap_or_default();
            out.push((*kind, e.tag, e.kind, e.count, value, w.tiff.value_range(e).unwrap()));
        }
    }
    out
}

fn offsets(block: &[u8]) -> Vec<(IfdKind, u32)> {
    walk(block, &l()).unwrap().ifds.iter().map(|(k, i)| (*k, i.offset)).collect()
}

/// §8 test 1: dates with offsets and sub-seconds, in both byte orders, on both date tags.
#[test]
fn dates_with_offsets_and_subseconds_read_back() {
    for big_endian in [false, true] {
        let before = camera(big_endian).build();
        for want in [
            date("2019-12-31T23:59:58", Some(-210), Some("050")),
            date("2026-01-02T03:04:05", Some(345), Some("123456789")),
            date("2001-02-03T04:05:06", Some(0), None),
            date("1999-12-31T00:00:00", None, None),
        ] {
            let after = change(ExifEdit { date_taken: Some(want.clone()), ..ExifEdit::default() }, &before);
            let r = read(&after, &l()).unwrap();
            assert_eq!(r.owned.date_taken.as_ref(), Some(&want), "{want}");
            assert_eq!(r.summary.date_digitized.as_ref(), Some(&want), "{want}");
            // Everything else reads as before.
            let old = read(&before, &l()).unwrap();
            assert_eq!(r.owned.caption, old.owned.caption);
            assert_eq!(r.owned.place, old.owned.place);
            assert_eq!(r.summary.camera(), old.summary.camera());
        }
    }
}

/// §8 test 1: southern and western places (and the equator and the meridian), in both byte orders,
/// into a block with a GPS IFD and into one without.
#[test]
fn southern_and_western_places_read_back() {
    let places = [(-33.868_820, -70.669_265), (-0.000_001, -179.999_999), (51.477_928, -0.001_545), (0.0, 0.0), (-90.0, 180.0), (12.5, 99.25)];
    for big_endian in [false, true] {
        let with_gps = camera(big_endian).build();
        let without = Tiff { gps: vec![], ..camera(big_endian) }.build();
        for before in [&with_gps, &without] {
            for (lat, lon) in places {
                let want = Place::new(lat, lon).unwrap();
                let after = change(ExifEdit { place: Some(Some(want)), ..ExifEdit::default() }, before);
                let got = read(&after, &l()).unwrap().owned.place.unwrap();
                assert!(got.close_to(&want, PLACE_TOLERANCE), "{want} read back as {got}");
                let gps: Vec<_> = entries(&after).into_iter().filter(|e| e.0 == IfdKind::Gps).collect();
                let refs: Vec<&[u8]> = gps.iter().filter(|e| e.1 == 1 || e.1 == 3).map(|e| e.4.as_slice()).collect();
                assert_eq!(refs, [if lat < 0.0 { b"S\0" } else { b"N\0" }, if lon < 0.0 { b"W\0" } else { b"E\0" }]);
                // Degree/minute/second rationals.
                assert!(gps.iter().filter(|e| e.1 == 2 || e.1 == 4).all(|e| e.2 == RATIONAL && e.3 == 3));
            }
        }
    }
}

/// A new GPS IFD carries GPSVersionID 2.3.0.0; an existing version is kept.
#[test]
fn gps_version() {
    let place = Some(Some(Place::new(-12.0, -77.0).unwrap()));
    let version = |b: &[u8]| entries(b).into_iter().find(|e| e.0 == IfdKind::Gps && e.1 == 0).map(|e| (e.2, e.4));
    let fresh = change(ExifEdit { place, ..ExifEdit::default() }, &Tiff { gps: vec![], ..camera(false) }.build());
    assert_eq!(version(&fresh), Some((BYTE, vec![2, 3, 0, 0])));
    let kept = change(ExifEdit { place, ..ExifEdit::default() }, &camera(true).build());
    assert_eq!(version(&kept), Some((BYTE, vec![2, 2, 0, 0])));
}

/// A file without EXIF gets a block for a date or a place; a caption or a removed place needs none.
#[test]
fn a_new_block() {
    let want = date("2024-08-09T10:11:12", Some(-300), Some("7"));
    let place = Place::new(-22.9, -43.2).unwrap();
    let e = ExifEdit { date_taken: Some(want.clone()), place: Some(Some(place)), caption: Some(Some("Hi".into())) };
    let block = edit(None, &e, MAX_JPEG_EXIF, &l()).unwrap().unwrap();
    let r = read(&block, &l()).unwrap();
    assert_eq!(r.owned.date_taken, Some(want));
    assert!(r.owned.place.unwrap().close_to(&place, PLACE_TOLERANCE));
    assert_eq!(r.owned.caption, None, "Dalica never adds an ImageDescription");
    assert!(entries(&block).iter().any(|e| e.0 == IfdKind::Exif && e.1 == 0x9000 && e.4 == b"0232"));
    assert_eq!(edit(None, &ExifEdit { caption: Some(Some("Hi".into())), place: Some(None), ..ExifEdit::default() }, MAX_JPEG_EXIF, &l()), Ok(None));
    assert_eq!(edit(None, &ExifEdit::default(), MAX_JPEG_EXIF, &l()), Ok(None));
}

/// Removing a place deletes the four latitude/longitude tags and nothing else, in place, and
/// zeroes the coordinates' bytes so they do not linger in the file.
#[test]
fn removing_a_place() {
    for big_endian in [false, true] {
        let before = camera(big_endian).build();
        let old = entries(&before);
        let after = change(ExifEdit { place: Some(None), ..ExifEdit::default() }, &before);
        assert_eq!(after.len(), before.len(), "edited in place");
        assert_eq!(offsets(&after), offsets(&before));
        assert_eq!(read(&after, &l()).unwrap().owned.place, None);
        let new = entries(&after);
        let gone = [1u16, 2, 3, 4];
        let expected: Vec<_> = old.iter().filter(|e| !(e.0 == IfdKind::Gps && gone.contains(&e.1))).cloned().collect();
        assert_eq!(new, expected);
        for e in old.iter().filter(|e| e.0 == IfdKind::Gps && gone.contains(&e.1)) {
            if let Some(r) = &e.5 {
                assert!(after[r.clone()].iter().all(|&b| b == 0), "the old coordinates are zeroed");
            }
        }
        // Removing it again changes nothing.
        assert_eq!(change(ExifEdit { place: Some(None), ..ExifEdit::default() }, &after), after);
    }
}

/// A changed value of the same size is overwritten where it is: nothing grows, no IFD moves.
#[test]
fn same_size_values_are_overwritten_in_place() {
    let full = Tiff {
        exif: vec![
            ascii(0x9003, "2026:06:04 19:42:10"),
            ascii(0x9004, "2026:06:04 19:42:11"),
            ascii(0x9011, "+10:00"),
            ascii(0x9012, "+10:00"),
            ascii(0x9291, "250"),
            ascii(0x9292, "250"),
        ],
        ..camera(false)
    };
    let before = full.build();
    let want = date("2020-02-29T12:00:00", Some(-180), Some("999"));
    let place = Place::new(-41.3, 174.8).unwrap();
    let after = change(ExifEdit { date_taken: Some(want.clone()), place: Some(Some(place)), caption: Some(Some("Wharf at noon".into())) }, &before);
    assert_eq!(after.len(), before.len());
    assert_eq!(offsets(&after), offsets(&before));
    let old = entries(&before);
    let new = entries(&after);
    for (o, n) in old.iter().zip(&new) {
        assert_eq!((o.0, o.1, o.5.as_ref().map(|r| r.start)), (n.0, n.1, n.5.as_ref().map(|r| r.start)), "every value stays where it was");
    }
    let r = read(&after, &l()).unwrap();
    assert_eq!(r.owned.date_taken, Some(want));
    assert_eq!(r.owned.caption.as_deref(), Some("Wharf at noon"));
    assert!(r.owned.place.unwrap().close_to(&place, PLACE_TOLERANCE));
}

/// A larger or new value is appended and only the pointer to it changes; the old block is a
/// prefix of the new one except for the edited entries, tables and pointers.
#[test]
fn larger_values_are_appended() {
    let before = camera(true).build();
    let caption = "A much longer caption than the camera's description ever was";
    let after = change(ExifEdit { caption: Some(Some(caption.into())), ..ExifEdit::default() }, &before);
    assert!(after.len() > before.len());
    assert_eq!(read(&after, &l()).unwrap().owned.caption.as_deref(), Some(caption));
    assert_eq!(offsets(&after), offsets(&before), "only the value moved, not IFD0");
    let at = entries(&after).into_iter().find(|e| e.1 == 0x010e && e.0 == IfdKind::Image).unwrap().5.unwrap();
    assert!(at.start >= before.len() && at.start % 2 == 0, "appended, word aligned");
    // A shorter caption goes where the old one was.
    let shorter = change(ExifEdit { caption: Some(Some("Quay".into())), ..ExifEdit::default() }, &before);
    assert_eq!(shorter.len(), before.len());
    assert_eq!(read(&shorter, &l()).unwrap().owned.caption.as_deref(), Some("Quay"));
}

/// Clearing the caption removes a real ImageDescription (else it would read back as the caption)
/// and leaves a camera placeholder; a caption is never added where there was none.
#[test]
fn the_caption_copy() {
    let before = camera(false).build();
    let cleared = change(ExifEdit { caption: Some(None), ..ExifEdit::default() }, &before);
    assert_eq!(read(&cleared, &l()).unwrap().owned.caption, None);
    assert!(!entries(&cleared).iter().any(|e| e.1 == 0x010e && e.0 == IfdKind::Image));
    assert_eq!(cleared.len(), before.len());
    let blank = change(ExifEdit { caption: Some(Some(" \u{1}\t".into())), ..ExifEdit::default() }, &before);
    assert_eq!(blank, cleared, "a caption that cleans to nothing clears it");

    let placeholder = Tiff { ifd0: vec![ascii(0x010e, "OLYMPUS DIGITAL CAMERA"), ascii(0x010f, "OLYMPUS")], ..camera(false) }.build();
    for caption in [None, Some("Summit".to_owned())] {
        assert_eq!(change(ExifEdit { caption: Some(caption), ..ExifEdit::default() }, &placeholder), placeholder);
    }
    let none = Tiff { ifd0: vec![ascii(0x010f, "Canon")], ..camera(false) }.build();
    assert_eq!(change(ExifEdit { caption: Some(Some("Summit".into())), ..ExifEdit::default() }, &none), none);

    let unicode = "Café « Zürich » 東京 / tags, too";
    let after = change(ExifEdit { caption: Some(Some(unicode.into())), ..ExifEdit::default() }, &before);
    assert_eq!(read(&after, &l()).unwrap().owned.caption.as_deref(), Some(unicode));
}

/// `t` with a maker note in the EXIF IFD: an IFD of its own whose values are at offsets counted
/// from the TIFF header (as several vendors write them), pointing inside the maker note.
fn with_maker_note(t: &Tiff) -> (Vec<u8>, std::ops::Range<usize>) {
    const LEN: usize = 2 + 12 * 2 + 4 + 16 + 8;
    let big_endian = t.big_endian;
    let mut first = t.clone();
    first.exif.push(undefined(MAKER_NOTE, &[0xaa; LEN]));
    let at = entries(&first.build()).into_iter().find(|e| e.1 == MAKER_NOTE).unwrap().5.unwrap();
    let u16b = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let u32b = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut note = Vec::new();
    note.extend(u16b(2));
    // An ASCII string and a RATIONAL, both out of line, at absolute offsets.
    let data = (at.start + 2 + 24 + 4) as u32;
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
    assert_eq!(note.len(), LEN);
    let mut t = t.clone();
    t.exif.push(undefined(MAKER_NOTE, &note));
    let block = t.build();
    assert_eq!(entries(&block).into_iter().find(|e| e.1 == MAKER_NOTE).unwrap().5.unwrap(), at);
    (block, at)
}

/// The maker note's IFD read at its offset, with its values read at their absolute offsets.
fn maker_note_values(block: &[u8], at: usize) -> Vec<Vec<u8>> {
    let t = Ifds::new(block).unwrap();
    let ifd = t.ifd(at as u32, &mut Visited::default(), &dalica_image::Limits::default()).unwrap();
    ifd.entries.iter().map(|e| t.value(e).unwrap().unwrap().to_vec()).collect()
}

/// §8 test 2: every edit at once (a date with new offset and fraction tags, a new GPS IFD, a
/// longer caption) leaves the maker note, the thumbnail and every foreign tag where they were and
/// as they were; the maker note's absolute offsets still read the same values.
#[test]
fn a_maker_note_with_absolute_offsets_survives() {
    for big_endian in [false, true] {
        // Without a GPS IFD, so IFD0 has to grow (and move) to point to a new one.
        let (before, at) = with_maker_note(&Tiff { gps: vec![], ..camera(big_endian) });
        let notes = maker_note_values(&before, at.start);
        assert_eq!(notes[0], b"vendor firmware\0");
        let e = ExifEdit {
            caption: Some(Some("A caption longer than the original description".into())),
            date_taken: Some(date("2026-06-04T19:42:10", Some(-570), Some("5"))),
            place: Some(Some(Place::new(-45.0, -66.5).unwrap())),
        };
        let after = change(e.clone(), &before);
        assert_ne!(offsets(&after), offsets(&before), "IFD0 and the EXIF IFD grew and moved");
        assert_eq!(after[at.clone()], before[at.clone()], "the maker note never moves");
        assert_eq!(maker_note_values(&after, at.start), notes);
        let owned = [(IfdKind::Image, 0x010e), (IfdKind::Image, TAG_GPS_IFD), (IfdKind::Image, 0x8769)];
        let foreign = |b: &[u8]| -> Vec<_> {
            entries(b)
                .into_iter()
                .filter(|x| !owned.contains(&(x.0, x.1)) && x.0 != IfdKind::Gps)
                .filter(|x| !(x.0 == IfdKind::Exif && ((0x9003..=0x9012).contains(&x.1) || (0x9290..=0x9292).contains(&x.1))))
                .collect()
        };
        assert_eq!(foreign(&after), foreign(&before), "foreign tags keep their values and offsets");
        // The thumbnail is still where IFD1 says.
        let r = read(&after, &l()).unwrap();
        assert_eq!(r.owned.date_taken, e.date_taken);
        assert_eq!(r.summary.make.as_deref(), Some("Canon"));
        let thumb = entries(&after).into_iter().find(|x| x.0 == IfdKind::Thumbnail && x.1 == 0x0201).unwrap().4;
        let thumb_at =
            if big_endian { u32::from_be_bytes(thumb.try_into().unwrap()) } else { u32::from_le_bytes(thumb.try_into().unwrap()) } as usize;
        assert_eq!(&after[thumb_at..thumb_at + 15], b"\xff\xd8 thumbnail \xff\xd9");
        // Every byte of the old block is as it was, except the header's IFD0 offset and the old
        // values of the owned tags (overwritten in place, or zeroed once replaced elsewhere).
        let owned_values: Vec<_> = entries(&before)
            .into_iter()
            .filter(|x| x.0 == IfdKind::Image && x.1 == 0x010e || x.0 == IfdKind::Exif && [0x9003, 0x9004, 0x9011, 0x9291].contains(&x.1))
            .filter_map(|x| x.5)
            .collect();
        for (i, (a, b)) in before.iter().zip(&after).enumerate() {
            if a != b {
                assert!((4..8).contains(&i) || owned_values.iter().any(|r| r.contains(&i)), "byte {i} changed");
            }
        }
        // §8 test 3: writing the same values again gives identical bytes.
        assert_eq!(change(e, &after), after);
    }
}

/// §8 test 7: an edit that would push the block past one APP1 segment fails naming the field, and
/// the input is unchanged; an edit that fits in place still succeeds at the limit.
/// `t` with a maker note sized so the block is `room` bytes short of one APP1 segment.
fn near_limit(t: &Tiff, room: usize) -> Vec<u8> {
    let with = |n: usize| {
        let mut t = t.clone();
        t.exif.push(undefined(MAKER_NOTE, &vec![0x5a; n]));
        t.build()
    };
    let probe = with(1000).len();
    let block = with(1000 + MAX_JPEG_EXIF - room - probe);
    assert!((MAX_JPEG_EXIF - room - 1..=MAX_JPEG_EXIF - room + 1).contains(&block.len()), "{}", block.len());
    block
}

#[test]
fn the_app1_limit() {
    let before = near_limit(&camera(false), 20);
    let copy = before.clone();
    let place = Some(Some(Place::new(-1.0, -2.0).unwrap()));
    let new_date = ExifEdit { date_taken: Some(date("2026-06-04T19:42:10", Some(-570), Some("5"))), ..ExifEdit::default() };
    assert_eq!(edit(Some(&before), &new_date, MAX_JPEG_EXIF, &l()), Err(Error::TooMany("EXIF bytes for the date taken")));
    let long_caption = ExifEdit { caption: Some(Some("x".repeat(200))), place, ..ExifEdit::default() };
    assert_eq!(edit(Some(&before), &long_caption, MAX_JPEG_EXIF, &l()), Err(Error::TooMany("EXIF bytes for the caption")));
    assert_eq!(before, copy, "the input is unchanged");
    let without_gps = near_limit(&Tiff { gps: vec![], ..camera(false) }, 20);
    assert_eq!(
        edit(Some(&without_gps), &ExifEdit { place, ..ExifEdit::default() }, MAX_JPEG_EXIF, &l()),
        Err(Error::TooMany("EXIF bytes for the place"))
    );
    assert_eq!(Error::TooMany("EXIF bytes for the place").to_string(), "too many EXIF bytes for the place");

    // In place it fits: a date without offset or fraction (those tags removed), the place into the
    // existing GPS IFD, a shorter caption.
    let fits = ExifEdit { date_taken: Some(date("2000-01-01T00:00:00", None, None)), place, caption: Some(Some("Short".into())) };
    let after = edit(Some(&before), &fits, MAX_JPEG_EXIF, &l()).unwrap().unwrap();
    assert_eq!(after.len(), before.len());
    // And a block already at the limit is not refused for an edit that does not grow it.
    assert!(edit(Some(&before), &fits, before.len(), &l()).is_ok());
    assert!(edit(Some(&before), &new_date, before.len(), &l()).is_err());
}

/// `ExifEdit::of` takes the caption, the date and the place of a write.
#[test]
fn edit_of_changes() {
    let c = Changes {
        caption: Some(None),
        tags: Some(vec!["a".into()]),
        star: Some(true),
        place: Some(None),
        date_taken: Some(date("2026-01-01T00:00:00", None, None)),
        faces: None,
    };
    let e = ExifEdit::of(&c);
    assert_eq!(e, ExifEdit { caption: Some(None), date_taken: c.date_taken.clone(), place: Some(None) });
    assert!(!e.is_empty());
    assert!(ExifEdit::of(&Changes { tags: Some(vec![]), ..Changes::default() }).is_empty());
}

/// Hostile EXIF: loops, values outside the block and huge counts are `Err`, never a panic; a value
/// shared with another entry is neither reused nor zeroed.
#[test]
fn hostile_blocks() {
    let e = ExifEdit { date_taken: Some(date("2026-06-04T19:42:10", Some(60), Some("1"))), place: Some(None), caption: Some(None) };
    let looped = Tiff { ifd0: vec![long(0x8769, 8)], ..Tiff::new() }.build();
    assert!(edit(Some(&looped), &e, usize::MAX, &l()).is_err());
    let outside = Tiff { ifd0: vec![raw(0x010e, ASCII, 40, 0xffff_fff0)], ..Tiff::new() }.build();
    assert!(edit(Some(&outside), &e, usize::MAX, &l()).is_err());
    assert!(edit(Some(b"not a tiff"), &e, usize::MAX, &l()).is_err());
    assert!(edit(Some(b""), &e, usize::MAX, &l()).is_err());

    // DateTimeOriginal's value is also the maker note's first bytes: the date is appended and the
    // shared bytes stay.
    let base = Tiff { exif: vec![ascii(0x9003, "2026:06:04 19:42:10"), undefined(MAKER_NOTE, b"maker note bytes here!")], ..Tiff::new() }.build();
    let note = entries(&base).into_iter().find(|x| x.1 == MAKER_NOTE).unwrap();
    let mut t = Tiff {
        exif: vec![raw(0x9003, ASCII, 20, note.5.clone().unwrap().start as u32), undefined(MAKER_NOTE, b"maker note bytes here!")],
        ..Tiff::new()
    };
    t.exif.sort_by_key(F::tag);
    let shared = t.build();
    let after = change(ExifEdit { date_taken: Some(date("2001-01-01T01:01:01", None, None)), ..ExifEdit::default() }, &shared);
    let n = entries(&after).into_iter().find(|x| x.1 == MAKER_NOTE).unwrap();
    assert_eq!(n.4, b"maker note bytes here!");
    assert_eq!(read(&after, &l()).unwrap().owned.date_taken, Some(date("2001-01-01T01:01:01", None, None)));
}

fn every_edit() -> ExifEdit {
    ExifEdit {
        caption: Some(Some("Night ferry".into())),
        date_taken: Some(date("2026-06-04T19:42:10", Some(-570), Some("5"))),
        place: Some(Some(Place::new(-45.0, -66.5).unwrap())),
    }
}

/// §8 test 10: a block cut at every length is an `Err` or a clean edit, never a panic.
#[test]
fn truncation_never_panics() {
    let (block, _) = with_maker_note(&camera(true));
    for e in [every_edit(), ExifEdit { place: Some(None), caption: Some(None), ..ExifEdit::default() }] {
        for len in 0..block.len() {
            if let Ok(Some(out)) = edit(Some(&block[..len]), &e, MAX_JPEG_EXIF, &l()) {
                assert!(walk(&out, &l()).is_ok());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    /// Random single-byte changes to a real block never panic, and an edit that succeeds reads back.
    #[test]
    fn mutations_never_panic(at in any::<prop::sample::Index>(), value in any::<u8>(), big_endian in any::<bool>()) {
        let mut block = with_maker_note(&camera(big_endian)).0;
        let i = at.index(block.len());
        block[i] = value;
        let e = every_edit();
        // A block the reader refuses is never rewritten by `write_owned`; only readable ones count.
        if read(&block, &l()).is_ok() && let Ok(Some(out)) = edit(Some(&block), &e, MAX_JPEG_EXIF, &l()) {
            let r = read(&out, &l());
            prop_assert!(r.is_ok());
            prop_assert_eq!(r.unwrap().owned.date_taken, e.date_taken);
        }
    }
}
