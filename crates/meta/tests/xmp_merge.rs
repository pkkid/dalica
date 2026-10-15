//! The XMP merge writer (`xmp-interop.md` §1–§3 and §8): owned properties written into one
//! description of Dalica's own and read back exactly (test 1), everything else byte for byte
//! (test 2), the same write twice giving the same bytes (test 3), the rating rule (test 4), clearing
//! values, the packet wrapper and its padding, new packets, the place and date mirrors, and hostile
//! input (test 10).

use crate::build;

use crate::common::{caption, cases, l, now, text};
use build::*;
use dalica_meta::fields::{XmpOwned, clean_tags, clean_text};
use dalica_meta::xmp::{self, JPEG_MAX_PACKET, Merged, NEW_PADDING};
use dalica_meta::{Changes, DateTaken, Error, Limits, Place};
use proptest::prelude::*;

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const NS: &str = "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"";

fn later() -> DateTaken {
    DateTaken::parse_iso("2027-01-01T00:00:00Z").unwrap()
}

fn merged(packet: Option<&[u8]>, c: &Changes) -> Merged {
    xmp::merge(packet, c, &now(), &l()).unwrap().expect("a change")
}

fn merge(packet: &[u8], c: &Changes) -> Vec<u8> {
    merged(Some(packet), c).into_bytes()
}

fn owned(packet: &[u8]) -> XmpOwned {
    xmp::read(packet, None, &l()).unwrap()
}

fn tags(t: &[&str]) -> Changes {
    Changes { tags: Some(t.iter().map(|s| s.to_string()).collect()), ..Changes::default() }
}

fn star(s: bool) -> Changes {
    Changes { star: Some(s), ..Changes::default() }
}

fn near(p: Option<Place>, want: Place) -> bool {
    p.is_some_and(|p| p.close_to(&want, 1e-6))
}

/// A wrapper with `padding` bytes of padding around `rdf`, laid out the way most writers do.
fn pretty(rdf: &str, padding: usize) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Other 1.0\">\n <rdf:RDF xmlns:rdf=\"{RDF_NS}\">\n{rdf}\n </rdf:RDF>\n</x:xmpmeta>\n{}<?xpacket end=\"w\"?>",
        " ".repeat(padding)
    )
    .into_bytes()
}

/// The base packets every round trip runs on: none, an empty compact one, an empty pretty one, and
/// one full of another application's data.
fn bases() -> Vec<(&'static str, Option<Vec<u8>>)> {
    vec![
        ("none", None),
        ("compact", Some(xmp_packet("<rdf:Description rdf:about=\"\"/>"))),
        ("pretty", Some(pretty("  <rdf:Description rdf:about=\"\"/>", 200))),
        ("foreign", Some(foreign())),
    ]
}

// The pieces of another application's packet (test 2): everything but the owned properties must
// come out byte for byte.
const D1_OPEN: &str = "  <rdf:Description rdf:about=\"\"\n    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n    xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\"\n    xmlns:stEvt=\"http://ns.adobe.com/xap/1.0/sType/ResourceEvent#\"\n    xmp:CreatorTool=\"Other Editor 7.1\"";
const RATING_ATTR: &str = "\n    xmp:Rating=\"5\"";
const D1_MID: &str = "\n    crs:Exposure2012=\"+0.50\"";
const DATE_ATTR: &str = "\n    xmp:MetadataDate=\"2020-05-06T07:08:09+01:00\"";
const D1_BODY: &str = ">\n   <xmpMM:History>\n    <rdf:Seq>\n     <rdf:li rdf:parseType=\"Resource\">\n      <stEvt:action>saved</stEvt:action>\n      <stEvt:when>2020-05-06T07:08:09+01:00</stEvt:when>\n     </rdf:li>\n    </rdf:Seq>\n   </xmpMM:History>";
const DESCRIPTION_ELEMENT: &str = "\n   <dc:description>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">Old caption</rdf:li>\n     <rdf:li xml:lang=\"de\">Alte Beschriftung</rdf:li>\n     <rdf:li xml:lang=\"fr-CA\">L&apos;ancienne</rdf:li>\n    </rdf:Alt>\n   </dc:description>";
const D1_MIDDLE: &str = "\n   <!-- a comment another application left -->\n   <crs:ToneCurvePV2012>\n    <rdf:Seq>\n     <rdf:li>0, 0</rdf:li>\n     <rdf:li>255, 255</rdf:li>\n    </rdf:Seq>\n   </crs:ToneCurvePV2012>";
const SUBJECT_ELEMENT: &str = "\n   <dc:subject><rdf:Bag><rdf:li>old tag</rdf:li></rdf:Bag></dc:subject>";
const D1_CLOSE: &str = "\n  </rdf:Description>";
const D2: &str = "\n  <rdf:Description rdf:about=\"\" xmlns:unk=\"http://example.com/unknown/1.0/\" unk:thing=\"kept\"><unk:list><rdf:Bag><rdf:li>1</rdf:li></rdf:Bag></unk:list><unk:raw><![CDATA[<b>verbatim</b>]]></unk:raw></rdf:Description>";

fn foreign_rdf(rating: bool, date: bool, description: bool, subject: bool) -> String {
    let pick = |on: bool, s: &'static str| if on { s } else { "" };
    [
        D1_OPEN,
        pick(rating, RATING_ATTR),
        D1_MID,
        pick(date, DATE_ATTR),
        D1_BODY,
        pick(description, DESCRIPTION_ELEMENT),
        D1_MIDDLE,
        pick(subject, SUBJECT_ELEMENT),
        D1_CLOSE,
        D2,
    ]
    .concat()
}

fn foreign() -> Vec<u8> {
    pretty(&foreign_rdf(true, true, true, true), 500)
}

/// Dalica's description in a merged packet (the last one) and the packet without it.
fn split_ours(packet: &[u8]) -> (String, String) {
    let s = text(packet);
    let close = s.rfind("</rdf:Description>").unwrap() + "</rdf:Description>".len();
    let open = s[..close].rfind("<rdf:Description").unwrap();
    // The line break and indentation before it go with it.
    let start = s[..open].trim_end_matches([' ', '\n']).len();
    (s[open..close].to_owned(), format!("{}{}", &s[..start], &s[close..]))
}

/// Everything but the padding.
fn unpadded(packet: &[u8]) -> String {
    let s = text(packet);
    let end = s.rfind("<?xpacket end").unwrap();
    format!("{}{}", s[..end].trim_end_matches(' '), &s[end..])
}

// Test 1: every owned field round-trips.

const CAPTIONS: &[&str] = &[
    "Harbour at dusk",
    "Ünïcödé 日本語 🎉 שלום مرحبا",
    "a & b < c > d \"q\" 'a' ]]> <![CDATA[ &amp;",
    "line one\r\nline two\nline three\rfour",
    "tab\tinside",
];

const TAGS: &[&str] = &["a/b", "x, y", "Ünïcödé", "日本", "R&D <x>", "with \"quotes\""];

#[test]
fn captions_and_tags_read_back_exactly() {
    for (name, base) in bases() {
        for &c in CAPTIONS {
            let out = merged(
                base.as_deref(),
                &Changes { caption: Some(Some(c.to_owned())), tags: Some(TAGS.iter().map(|t| t.to_string()).collect()), ..Changes::default() },
            );
            let o = owned(out.bytes());
            assert_eq!(o.caption, clean_text(c, l().max_string), "{name}: {c:?}");
            assert_eq!(o.tags, TAGS, "{name}");
        }
    }
}

#[test]
fn other_languages_are_kept_and_x_default_comes_first() {
    let out = merge(&foreign(), &caption("New caption"));
    let (ours, _) = split_ours(&out);
    let order: Vec<usize> =
        ["x-default\">New caption", "de\">Alte Beschriftung", "fr-CA\">L'ancienne"].iter().map(|s| ours.find(s).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{ours}");
    let rows = xmp::rows(&out, None, &l()).unwrap();
    let langs: Vec<&str> = rows.iter().filter(|r| r.label.starts_with("dc:description")).map(|r| r.value.as_str()).collect();
    assert_eq!(langs, ["New caption", "Alte Beschriftung", "L'ancienne"]);
}

#[test]
fn star_place_and_dates_read_back() {
    let place = Place::new(-33.856_789_123, -151.215_432_987).unwrap();
    let date = DateTaken::parse_iso("2024-02-29T23:59:58.050-03:30").unwrap();
    let base =
        pretty(&format!("  <rdf:Description rdf:about=\"\" {NS} exif:DateTimeOriginal=\"2000-01-01T00:00\" photoshop:DateCreated=\"2000\"/>"), 0);
    let all = Changes { star: Some(true), place: Some(Some(place)), date_taken: Some(date.clone()), ..Changes::default() };
    let o = owned(&merge(&base, &all));
    assert_eq!(o.rating, Some(5));
    assert!(near(o.place, place), "{:?}", o.place);
    assert_eq!((o.date_time_original.as_ref(), o.date_created.as_ref()), (Some(&date), Some(&date)));
    // xmp:CreateDate was not in the packet, so it is not added.
    assert_eq!(o.create_date, None);
    // The corners of the globe.
    for (la, lo) in [(90.0, 180.0), (-90.0, -180.0), (0.0, 0.0), (-0.000_001, 0.000_001)] {
        let p = Place::new(la, lo).unwrap();
        assert!(near(owned(&merge(&base, &Changes { place: Some(Some(p)), ..Changes::default() })).place, p), "{la} {lo}");
    }
}

// Test 2: foreign data survives byte for byte.

#[test]
fn foreign_data_survives_byte_for_byte() {
    let base = foreign();
    let change = Changes { caption: Some(Some("New".into())), tags: Some(vec!["new".into()]), star: Some(true), ..Changes::default() };
    let out = merge(&base, &change);
    // The padding absorbed the change: the packet can be rewritten in place.
    assert_eq!(out.len(), base.len());
    let (ours, rest) = split_ours(&out);
    // Without Dalica's description, the packet is the old one minus exactly the owned properties
    // (attribute and element form), every other byte where it was.
    let expected = pretty(&foreign_rdf(false, false, false, false), 0);
    assert_eq!(unpadded(rest.as_bytes()), unpadded(&expected));
    assert!(ours.contains("<xmp:Rating>5</xmp:Rating>") && ours.contains("<xmp:MetadataDate>2026-10-12T09:30:00+02:00</xmp:MetadataDate>"));
    assert_eq!(ours.matches("<rdf:Description").count(), 1);
    // Dalica's description sits just before </rdf:RDF>, indented like the others.
    assert!(text(&out).contains(&format!("</rdf:Description>\n  {ours}\n </rdf:RDF>")));
}

#[test]
fn only_the_properties_a_write_changes_are_moved() {
    let base = foreign();
    let out = merge(&base, &tags(&["only tags"]));
    let (ours, rest) = split_ours(&out);
    // The caption and rating stay where the other application put them; the tags and the metadata
    // date move.
    assert!(rest.contains(DESCRIPTION_ELEMENT) && rest.contains(RATING_ATTR) && !rest.contains(SUBJECT_ELEMENT) && !rest.contains(DATE_ATTR));
    assert!(!ours.contains("dc:description") && ours.contains("only tags"));
    let o = owned(&out);
    assert_eq!((o.caption.as_deref(), o.rating, o.tags), (Some("Old caption"), Some(5), vec!["only tags".to_owned()]));
}

#[test]
fn self_closing_and_emptied_descriptions_keep_their_shape() {
    let d = "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"";
    let base = xmp_packet(&format!(
        "{d} xmp:Rating=\"5\" xmp:Label=\"Red\"/>{d}><dc:subject><rdf:Bag><rdf:li>t</rdf:li></rdf:Bag></dc:subject></rdf:Description>"
    ));
    let out = merge(&base, &Changes { star: Some(false), tags: Some(vec![]), ..Changes::default() });
    let (_, rest) = split_ours(&out);
    assert_eq!(rest, text(&xmp_packet(&format!("{d} xmp:Label=\"Red\"/>{d}></rdf:Description>"))));
    let o = owned(&out);
    assert_eq!((o.rating, o.tags.len()), (Some(0), 0));
}

#[test]
fn other_prefixes_are_respected() {
    // RDF bound to `r:`, Dublin Core to `foo:`, `dc:` bound to something else entirely.
    let base = "<r:RDF xmlns:r=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" xmlns:dc=\"http://example.com/not-dc/\">\
                <r:Description r:about=\"\" xmlns:foo=\"http://purl.org/dc/elements/1.1/\"><foo:description>old</foo:description><dc:x>kept</dc:x></r:Description></r:RDF>";
    let out = merge(base.as_bytes(), &caption("new"));
    let s = text(&out);
    assert!(s.contains("<r:Description r:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=") && s.contains("<r:Alt>"), "{s}");
    assert!(s.contains("<dc:x>kept</dc:x>") && !s.contains("foo:description"));
    assert_eq!(owned(&out).caption.as_deref(), Some("new"));
    // RDF as the default namespace: Dalica's description declares its own `rdf:` prefix.
    let base = "<RDF xmlns=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><Description xmlns:r=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" r:about=\"a\"/></RDF>";
    let out = merge(base.as_bytes(), &caption("new"));
    assert!(text(&out).contains(&format!("<rdf:Description rdf:about=\"a\" xmlns:rdf=\"{RDF_NS}\"")), "{}", text(&out));
    assert_eq!(owned(&out).caption.as_deref(), Some("new"));
}

// Test 3: idempotence.

#[test]
fn writing_twice_gives_identical_bytes() {
    for (name, base) in bases() {
        let change = Changes { caption: Some(Some("Same".into())), tags: Some(vec!["a".into(), "b".into()]), star: Some(true), ..Changes::default() };
        let once = merged(base.as_deref(), &change).into_bytes();
        // Deterministic.
        assert_eq!(merged(base.as_deref(), &change).into_bytes(), once, "{name}");
        // Writing the same values again changes nothing, whatever the time.
        assert_eq!(xmp::merge(Some(&once), &change, &now(), &l()).unwrap(), None, "{name}");
        assert_eq!(xmp::merge(Some(&once), &change, &later(), &l()).unwrap(), None, "{name}");
    }
}

#[test]
fn repeated_writes_reuse_one_description() {
    let mut packet = foreign();
    let len = packet.len();
    let steps = [caption("one"), tags(&["t"]), star(true), caption("two"), tags(&[]), star(false), caption("")];
    for (n, c) in steps.iter().enumerate() {
        packet = merge(&packet, c);
        assert_eq!(text(&packet).matches("<rdf:Description").count(), 3, "step {n}: {}", text(&packet));
        assert_eq!(packet.len(), len, "step {n}");
    }
    let o = owned(&packet);
    // Every value Dalica did not change in a step stayed in its description.
    assert_eq!((o.caption, o.tags.len(), o.rating), (None, 0, Some(0)));
    // The other languages outlived the caption.
    assert!(text(&packet).contains("Alte Beschriftung"));
}

// Test 4: the rating rule.

#[test]
fn unstarring_never_clears_a_rating_dalica_did_not_set() {
    let with_rating = |r: &str| xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"{r}\"/>"));
    for r in ["3", "-1", "0", "4.0"] {
        let base = with_rating(r);
        assert_eq!(xmp::merge(Some(&base), &star(false), &now(), &l()).unwrap(), None, "{r}");
        // Starring writes 5 into Dalica's description and removes the other rating.
        let starred = merge(&base, &star(true));
        assert_eq!(owned(&starred).rating, Some(5));
        assert_eq!(text(&starred).matches("Rating").count(), 2, "{}", text(&starred));
        // Unstarring what Dalica starred writes 0.
        assert_eq!(owned(&merge(&starred, &star(false))).rating, Some(0));
    }
    // A 5 another application wrote counts as Dalica's star.
    assert_eq!(owned(&merge(&with_rating("5"), &star(false))).rating, Some(0));
    // No packet: unstarring needs nothing, starring makes one.
    assert_eq!(xmp::merge(None, &star(false), &now(), &l()).unwrap(), None);
    assert_eq!(owned(merged(None, &star(true)).bytes()).rating, Some(5));
}

// Clearing.

#[test]
fn clearing_a_value_removes_the_property() {
    let base = foreign();
    let out = merge(&base, &Changes { caption: Some(None), tags: Some(vec![]), ..Changes::default() });
    let s = text(&out);
    assert!(!s.contains("old tag") && !s.contains("dc:subject") && !s.contains("x-default"), "{s}");
    // The other languages stay; the caption is gone.
    assert!(s.contains("Alte Beschriftung") && s.contains("L'ancienne"));
    assert_eq!(owned(&out).caption, None);
    // With no other language, dc:description goes entirely.
    let base = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} dc:description=\"x\"/>"));
    let out = merge(&base, &caption("  "));
    assert!(!text(&out).contains("description"), "{}", text(&out));
    // Clearing what is not there changes nothing.
    assert_eq!(xmp::merge(Some(&out), &Changes { caption: Some(None), tags: Some(vec![]), ..Changes::default() }, &later(), &l()).unwrap(), None);
    assert_eq!(xmp::merge(None, &caption(""), &now(), &l()).unwrap(), None);
}

// The wrapper, padding and size.

#[test]
fn padding_absorbs_the_change_in_size() {
    let base = pretty("  <rdf:Description rdf:about=\"\"/>", 1000);
    let small = merged(Some(&base), &caption("short"));
    assert_eq!((small.len(), small.padding() < 1000), (base.len(), true));
    // A shorter caption gives the padding back.
    let shorter = merged(Some(small.bytes()), &caption("s"));
    assert_eq!((shorter.len(), shorter.padding()), (base.len(), small.padding() + 4));
    // Past the padding the packet grows, with none left (the line break before the trailer was part
    // of it).
    let big = merged(Some(&base), &caption(&"x".repeat(5000)));
    assert_eq!((big.padding(), big.len() > base.len()), (0, true));
    assert!(text(big.bytes()).ends_with("</x:xmpmeta><?xpacket end=\"w\"?>"));
    // Without a wrapper there is no padding, and nothing is added.
    let bare = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"></rdf:RDF></x:xmpmeta>";
    let out = merged(Some(bare.as_bytes()), &caption("c"));
    assert!(out.padding() == 0 && text(out.bytes()).starts_with("<x:xmpmeta") && text(out.bytes()).ends_with("</rdf:RDF></x:xmpmeta>"));
}

#[test]
fn the_size_for_the_app1_limit() {
    // Fits with its padding.
    let ok = merged(None, &caption("c"));
    assert_eq!(ok.fit(JPEG_MAX_PACKET).as_deref(), Some(ok.bytes()));
    // Fits once the padding goes: the packet is cut to the limit and still reads back.
    let near_limit = merged(None, &caption(&"é".repeat(32_000)));
    assert!(near_limit.len() > JPEG_MAX_PACKET && near_limit.min_len() <= JPEG_MAX_PACKET);
    let fitted = near_limit.fit(JPEG_MAX_PACKET).unwrap();
    assert_eq!(fitted.len(), JPEG_MAX_PACKET);
    assert_eq!(owned(&fitted).caption.map(|c| c.chars().count()), Some(32_000));
    // Does not fit at all: nothing else is ever dropped to make room.
    let too_big = merged(None, &caption(&"x".repeat(65_000)));
    assert!(too_big.min_len() > JPEG_MAX_PACKET);
    assert_eq!(too_big.fit(JPEG_MAX_PACKET), None);
}

#[test]
fn a_new_packet_when_the_file_has_none() {
    let out = merged(None, &Changes { caption: Some(Some("c".into())), tags: Some(vec!["t".into()]), ..Changes::default() });
    let s = text(out.bytes());
    assert!(s.starts_with("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>") && s.ends_with("<?xpacket end=\"w\"?>"));
    assert_eq!(out.padding(), NEW_PADDING);
    assert_eq!(owned(out.bytes()).tags, ["t"]);
    // A blank block counts as none.
    assert_eq!(merged(Some(b"  \0\0"), &caption("c")).into_bytes(), merged(None, &caption("c")).into_bytes());
    // Place and dates are mirrors: never the reason for a new packet.
    let mirrors = Changes { place: Some(Some(Place::new(1.0, 2.0).unwrap())), date_taken: Some(now()), ..Changes::default() };
    assert_eq!(xmp::merge(None, &mirrors, &now(), &l()).unwrap(), None);
}

#[test]
fn place_mirror_written_and_removed() {
    let base = xmp_packet(&format!(
        "<rdf:Description rdf:about=\"\" {NS} exif:GPSLatitude=\"1,0N\" exif:GPSLongitude=\"2,0E\" exif:GPSAltitude=\"100/1\" exif:GPSTimeStamp=\"2020-01-01T00:00:00Z\"/>"
    ));
    let p = Place::new(-45.5, -73.25).unwrap();
    let moved = merge(&base, &Changes { place: Some(Some(p)), ..Changes::default() });
    assert!(near(owned(&moved).place, p));
    let removed = merge(&moved, &Changes { place: Some(None), ..Changes::default() });
    let s = text(&removed);
    // Only latitude and longitude go; altitude and time stay.
    assert!(
        !s.contains("GPSLatitude") && !s.contains("GPSLongitude") && s.contains("exif:GPSAltitude=\"100/1\"") && s.contains("GPSTimeStamp"),
        "{s}"
    );
    assert_eq!(owned(&removed).place, None);
    // Removing a place the packet does not have changes nothing.
    assert_eq!(xmp::merge(Some(&removed), &Changes { place: Some(None), ..Changes::default() }, &later(), &l()).unwrap(), None);
}

#[test]
fn date_mirrors_only_where_present() {
    let date = DateTaken::parse_iso("2019-07-08T10:11:12+05:45").unwrap();
    let change = Changes { date_taken: Some(date.clone()), ..Changes::default() };
    let base =
        xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS}><photoshop:DateCreated>2000-01-01</photoshop:DateCreated></rdf:Description>"));
    let out = merge(&base, &change);
    let o = owned(&out);
    assert_eq!((o.date_created, o.date_time_original, o.create_date), (Some(date.clone()), None, None));
    // None present: no change.
    assert_eq!(xmp::merge(Some(&xmp_packet("<rdf:Description rdf:about=\"\"/>")), &change, &now(), &l()).unwrap(), None);
    // A later date change updates the mirror in Dalica's description.
    let next = DateTaken::parse_iso("2019-07-09T00:00:00").unwrap();
    assert_eq!(owned(&merge(&out, &Changes { date_taken: Some(next.clone()), ..Changes::default() })).date_created, Some(next));
}

#[test]
fn metadata_date_moves_to_dalicas_description() {
    let out = merge(&foreign(), &caption("x"));
    assert_eq!(text(&out).matches("MetadataDate").count(), 2, "one element: open and close tags");
    assert!(text(&out).contains("<xmp:MetadataDate>2026-10-12T09:30:00+02:00</xmp:MetadataDate>"));
}

// Test 10: hostile packets are never replaced.

#[test]
fn a_packet_that_is_not_well_formed_is_refused_not_replaced() {
    for bad in [
        &b"<!DOCTYPE x [<!ENTITY e \"x\">]><x>&e;</x>"[..],
        b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF>",
        b"<a><b></a>",
        b"\xff\xfe<\x00a\x00>",
        b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>",
        b"<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/>",
    ] {
        assert!(xmp::merge(Some(bad), &caption("c"), &now(), &l()).is_err(), "{}", String::from_utf8_lossy(bad));
    }
    let big = xmp_packet("<rdf:Description rdf:about=\"\"/>");
    assert_eq!(xmp::merge(Some(&big), &caption("c"), &now(), &Limits { max_xmp: big.len() - 1, ..l() }), Err(Error::TooMany("XMP bytes")));
}

#[test]
fn truncated_packets_never_panic() {
    let base = foreign();
    let change = Changes { caption: Some(Some("c".into())), tags: Some(vec!["t".into()]), star: Some(true), ..Changes::default() };
    for end in 0..base.len() {
        if let Ok(Some(m)) = xmp::merge(Some(&base[..end]), &change, &now(), &l()) {
            assert_eq!(owned(m.bytes()).caption.as_deref(), Some("c"));
        }
    }
}

fn xml_chars(s: &str) -> String {
    s.chars().filter(|&c| !matches!(c, '\u{fffe}' | '\u{ffff}')).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(64)))]

    #[test]
    fn random_captions_and_tags_round_trip(c in any::<String>(), t in proptest::collection::vec(any::<String>(), 0..8), base in 0usize..4) {
        let (_, packet) = bases().swap_remove(base);
        let change = Changes { caption: Some(Some(c.clone())), tags: Some(t.clone()), ..Changes::default() };
        if let Some(m) = xmp::merge(packet.as_deref(), &change, &now(), &l()).unwrap() {
            let o = owned(m.bytes());
            prop_assert_eq!(o.caption, clean_text(&xml_chars(&c), l().max_string));
            let cleaned: Vec<String> = t.iter().map(|x| xml_chars(x)).collect();
            prop_assert_eq!(o.tags, clean_tags(cleaned.iter().map(String::as_str), l().max_string, l().max_tags));
        }
    }
}
