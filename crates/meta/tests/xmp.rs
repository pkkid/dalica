//! XMP reading (`xmp-interop.md` §2–§3, `security.md` §4.2): every owned property in attribute and
//! element form, `rdf:Alt` with `x-default` and other languages, `rdf:Bag` and `rdf:Seq`,
//! namespaces matched by name rather than prefix, extended XMP, the listing rows, and hostile
//! packets (§8 test 10: `DOCTYPE` and entities refused, deep nesting and node bombs bounded,
//! truncation, byte flips and random bytes never panic).

mod build;

use build::*;
use dalica_meta::fields::XmpOwned;
use dalica_meta::xmp::{self, MAX_DEPTH, MAX_NODES};
use dalica_meta::{BlockKind, DateTaken, Error, Limits, Place, listing, read};
use proptest::prelude::*;

const NS: &str = "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"";

fn l() -> Limits {
    Limits::default()
}

fn owned(body: &str) -> XmpOwned {
    xmp::read(&xmp_packet(body), None, &l()).unwrap()
}

fn near(p: Option<Place>, latitude: f64, longitude: f64) -> bool {
    p.is_some_and(|p| p.close_to(&Place::new(latitude, longitude).unwrap(), 1e-9))
}

fn d(s: &str) -> Option<DateTaken> {
    DateTaken::parse_iso(s)
}

/// A packet holding every owned property in element form, with Unicode and other languages.
fn element_form() -> String {
    format!(
        "<rdf:Description rdf:about=\"\" {NS}>\
         <dc:description><rdf:Alt><rdf:li xml:lang=\"de\">Hafen</rdf:li><rdf:li xml:lang=\"x-default\">Harbour &amp; boats, Ünïcödé 日本 🎉</rdf:li></rdf:Alt></dc:description>\
         <dc:subject><rdf:Bag><rdf:li>a/b</rdf:li><rdf:li>x, y</rdf:li><rdf:li>Ünïcödé</rdf:li><rdf:li>A/B</rdf:li></rdf:Bag></dc:subject>\
         <xmp:Rating>5</xmp:Rating>\
         <exif:GPSLatitude>33,52.5S</exif:GPSLatitude><exif:GPSLongitude>151,12,36W</exif:GPSLongitude>\
         <exif:DateTimeOriginal>2026-06-04T19:42:10.123+09:00</exif:DateTimeOriginal>\
         <photoshop:DateCreated>2026-06-04</photoshop:DateCreated>\
         <xmp:CreateDate>2026-06-04T19:42Z</xmp:CreateDate>\
         </rdf:Description>"
    )
}

#[test]
fn element_form_reads_every_owned_property() {
    let o = owned(&element_form());
    assert_eq!(o.caption.as_deref(), Some("Harbour & boats, Ünïcödé 日本 🎉"));
    // Duplicates differing only in case are dropped, the first spelling kept.
    assert_eq!(o.tags, ["a/b", "x, y", "Ünïcödé"]);
    assert_eq!(o.rating, Some(5));
    assert!(near(o.place, -33.875, -151.21));
    assert_eq!(o.date_time_original, d("2026-06-04T19:42:10.123+09:00"));
    assert_eq!(o.date_created, d("2026-06-04"));
    assert_eq!(o.create_date, d("2026-06-04T19:42:00+00:00"));
}

#[test]
fn attribute_form_reads_every_simple_property() {
    let o = owned(&format!(
        "<rdf:Description rdf:about=\"\" {NS} dc:description=\"Simple &lt;caption&gt;\" xmp:Rating=\"-1\" exif:GPSLatitude=\"10,30N\" \
         exif:GPSLongitude=\"20.25\" exif:DateTimeOriginal=\"2001-02-03T04:05:06\" photoshop:DateCreated=\"2002\" xmp:CreateDate=\"2003-04\"/>"
    ));
    assert_eq!(o.caption.as_deref(), Some("Simple <caption>"));
    assert_eq!(o.rating, Some(-1));
    assert_eq!(o.place, Place::new(10.5, 20.25));
    assert_eq!((o.date_time_original, o.date_created, o.create_date), (d("2001-02-03T04:05:06"), d("2002"), d("2003-04")));
}

#[test]
fn language_alternatives_and_array_kinds() {
    let alt = |items: &str| {
        owned(&format!("<rdf:Description rdf:about=\"\" {NS}><dc:description><rdf:Alt>{items}</rdf:Alt></dc:description></rdf:Description>"))
    };
    // x-default wins wherever it is; its case does not matter.
    assert_eq!(alt("<rdf:li xml:lang=\"fr\">Port</rdf:li><rdf:li xml:lang=\"X-Default\">Harbour</rdf:li>").caption.as_deref(), Some("Harbour"));
    // An item without a language stands in for x-default; other languages never do.
    assert_eq!(alt("<rdf:li xml:lang=\"fr\">Port</rdf:li><rdf:li>Harbour</rdf:li>").caption.as_deref(), Some("Harbour"));
    assert_eq!(alt("<rdf:li xml:lang=\"fr\">Port</rdf:li>").caption, None);
    // A qualified value (rdf:value inside a description) reads as its value.
    assert_eq!(
        alt("<rdf:li xml:lang=\"x-default\"><rdf:Description><rdf:value>Qualified</rdf:value></rdf:Description></rdf:li>").caption.as_deref(),
        Some("Qualified")
    );
    let tags = |v: &str| owned(&format!("<rdf:Description rdf:about=\"\" {NS}><dc:subject>{v}</dc:subject></rdf:Description>")).tags;
    assert_eq!(tags("<rdf:Seq><rdf:li>one</rdf:li><rdf:li> two </rdf:li></rdf:Seq>"), ["one", "two"]);
    assert_eq!(tags("single"), ["single"]);
    assert_eq!(tags("<rdf:Bag><rdf:li>tab\there</rdf:li><rdf:li>\u{1}</rdf:li></rdf:Bag>"), ["tab here"]);
}

#[test]
fn namespaces_are_matched_by_name_not_prefix() {
    let o = owned(
        "<rdf:Description rdf:about=\"\" xmlns:foo=\"http://purl.org/dc/elements/1.1/\" xmlns:bar=\"http://ns.adobe.com/xap/1.0/\" bar:Rating=\"5\">\
         <foo:description><rdf:Alt><rdf:li xml:lang=\"x-default\">Renamed</rdf:li></rdf:Alt></foo:description></rdf:Description>\
         <rdf:Description rdf:about=\"\" xmlns:dc=\"http://example.com/not-dublin-core/\"><dc:subject>not a tag</dc:subject></rdf:Description>",
    );
    assert_eq!((o.caption.as_deref(), o.rating, o.tags.len()), (Some("Renamed"), Some(5), 0));
    // RDF itself under another prefix, and the default namespace.
    let other = "<?xpacket begin=\"\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><RDF xmlns=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                 <Description xmlns:r=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" r:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
                 <dc:subject><Bag><li>k</li></Bag></dc:subject></Description></RDF></x:xmpmeta><?xpacket end=\"w\"?>";
    assert_eq!(xmp::read(other.as_bytes(), None, &l()).unwrap().tags, ["k"]);
}

#[test]
fn the_first_description_with_a_property_wins_and_the_rest_are_read() {
    let o = owned(&format!(
        "<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"2\"/>\
         <rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"5\" dc:description=\"second\"/>"
    ));
    assert_eq!((o.rating, o.caption.as_deref()), (Some(2), Some("second")));
}

#[test]
fn bad_values_are_left_out() {
    let o = owned(&format!(
        "<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"7\" exif:GPSLatitude=\"95,0N\" exif:GPSLongitude=\"1,0E\" \
         exif:DateTimeOriginal=\"2026-02-30T00:00\" photoshop:DateCreated=\"yesterday\" dc:description=\" \u{1}\u{2} \"/>"
    ));
    assert_eq!(o, XmpOwned::default());
    // A latitude needs its longitude.
    assert_eq!(owned(&format!("<rdf:Description rdf:about=\"\" {NS} exif:GPSLatitude=\"10,0N\"/>")).place, None);
    // A blank packet (padding only) holds nothing.
    assert_eq!(xmp::read(b"  \n\0\0 ", None, &l()).unwrap(), XmpOwned::default());
    // A packet without RDF holds nothing either.
    assert_eq!(xmp::read(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>", None, &l()).unwrap(), XmpOwned::default());
}

#[test]
fn the_extended_packet_fills_what_the_standard_one_leaves_empty() {
    let main = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"3\"/>"));
    let ext = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS} xmp:Rating=\"5\" dc:description=\"from the extended part\"/>"));
    let o = xmp::read(&main, Some(&ext), &l()).unwrap();
    assert_eq!((o.rating, o.caption.as_deref()), (Some(3), Some("from the extended part")));
    assert!(xmp::read(&main, Some(b"<broken"), &l()).is_err());
}

#[test]
fn xmp_in_each_container_reaches_the_owned_fields() {
    let packet = xmp_packet(&element_form());
    let iptc = irb(&[(0x0404, &iim(&[(2, 120, b"IPTC caption"), (2, 25, b"iptc tag")]))]);
    let files = [
        jpeg(&[app13(&iptc), app1_xmp(&packet)]),
        png(&[(b"iTXt", itxt("XML:com.adobe.xmp", &packet, false))]),
        png(&[(b"iTXt", itxt("XML:com.adobe.xmp", &packet, true))]),
        webp(&[(b"XMP ", packet.clone())]),
        tiff_file(Tiff { ifd0: vec![undefined(0x02bc, &packet)], ..Tiff::new() }),
    ];
    for (n, file) in files.iter().enumerate() {
        let m = read(file, &l()).unwrap();
        assert!(m.problems.is_empty(), "{n}: {:?}", m.problems);
        // XMP wins over IPTC for caption and tags (`xmp-interop.md` §2).
        assert_eq!(m.owned.caption.as_deref(), Some("Harbour & boats, Ünïcödé 日本 🎉"), "{n}");
        assert_eq!(m.owned.tags, ["a/b", "x, y", "Ünïcödé"], "{n}");
        assert!(m.owned.starred(), "{n}");
        assert!(near(m.owned.place, -33.875, -151.21), "{n}");
    }
}

#[test]
fn an_unreadable_packet_is_a_problem_and_the_rest_still_reads() {
    let iptc = irb(&[(0x0404, &iim(&[(2, 120, b"IPTC caption")]))]);
    let file = jpeg(&[app13(&iptc), app1_xmp(b"<!DOCTYPE x [<!ENTITY a \"b\">]><x>&a;</x>")]);
    let m = read(&file, &l()).unwrap();
    assert_eq!(m.problems.len(), 1);
    assert_eq!(m.problems[0].block, BlockKind::Xmp);
    assert_eq!(m.owned.caption.as_deref(), Some("IPTC caption"));
}

#[test]
fn listing_rows() {
    let body = format!(
        "<rdf:Description rdf:about=\"\" {NS} xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\" xmlns:stEvt=\"http://ns.adobe.com/xap/1.0/sType/ResourceEvent#\" \
         xmlns:u=\"http://example.com/unknown/\" xmp:CreatorTool=\"Tool\" u:thing=\"x\">\
         <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">Cap</rdf:li><rdf:li xml:lang=\"de\">Kap</rdf:li></rdf:Alt></dc:description>\
         <dc:subject><rdf:Bag><rdf:li>a</rdf:li><rdf:li>b</rdf:li></rdf:Bag></dc:subject>\
         <xmpMM:History><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><stEvt:action>saved</stEvt:action></rdf:li></rdf:Seq></xmpMM:History>\
         </rdf:Description>"
    );
    let file = jpeg(&[app1_xmp(&xmp_packet(&body))]);
    let rows: Vec<(String, String, String)> = listing(&file, &l()).unwrap().rows.into_iter().map(|r| (r.group, r.label, r.value)).collect();
    let row = |label: &str, value: &str| ("XMP".to_owned(), label.to_owned(), value.to_owned());
    assert_eq!(
        rows,
        [
            row("xmp:CreatorTool", "Tool"),
            row("u:thing", "x"),
            row("dc:description [x-default]", "Cap"),
            row("dc:description [de]", "Kap"),
            row("dc:subject", "a; b"),
            row("xmpMM:History[1]/stEvt:action", "saved"),
        ]
    );
    // The rows are bounded like every listing.
    let many: String = (0..50).map(|i| format!("<u:p{i}>v</u:p{i}>")).collect();
    let file = jpeg(&[app1_xmp(&xmp_packet(&format!("<rdf:Description rdf:about=\"\" xmlns:u=\"http://example.com/u/\">{many}</rdf:Description>")))]);
    let out = listing(&file, &Limits { max_rows: 10, ..l() }).unwrap();
    assert_eq!((out.rows.len(), out.truncated), (10, true));
    // An unreadable packet is one row.
    let file = jpeg(&[app1_xmp(b"<a><b></a>")]);
    let out = listing(&file, &l()).unwrap();
    assert_eq!((out.rows.len(), out.rows[0].label.as_str()), (1, "Unreadable"));
}

// Hostile packets (§8 test 10).

#[test]
fn doctype_entities_and_external_references_are_refused() {
    for bad in [
        // The billion laughs.
        "<?xml version=\"1.0\"?><!DOCTYPE lolz [<!ENTITY lol \"lol\"><!ENTITY lol2 \"&lol;&lol;&lol;\">]><lolz>&lol2;</lolz>",
        "<!DOCTYPE x SYSTEM \"file:///etc/passwd\"><x/>",
        "<!DOCTYPE x [<!ENTITY e SYSTEM \"http://example.com/\">]><x>&e;</x>",
        "<x>&undefined;</x>",
        "<x a=\"&undefined;\"/>",
        "<x>&#0;</x>",
    ] {
        assert!(matches!(xmp::read(bad.as_bytes(), None, &l()), Err(Error::Malformed(_))), "{bad}");
    }
}

fn nested(depth: usize) -> String {
    format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth))
}

#[test]
fn nesting_is_bounded_at_its_limit() {
    assert_eq!(MAX_DEPTH, 64);
    assert!(xmp::read(nested(MAX_DEPTH).as_bytes(), None, &l()).is_ok());
    assert_eq!(xmp::read(nested(MAX_DEPTH + 1).as_bytes(), None, &l()), Err(Error::TooMany("XML nesting levels")));
    // Deep nesting inside a real packet: refused, and no recursion follows it.
    let deep = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS}><dc:subject>{}</dc:subject></rdf:Description>", nested(10_000)));
    assert_eq!(xmp::read(&deep, None, &l()), Err(Error::TooMany("XML nesting levels")));
}

/// One root element and `n - 1` empty children: `n` nodes.
fn wide(n: usize) -> String {
    format!("<a>{}</a>", "<b/>".repeat(n - 1))
}

#[test]
fn node_counts_are_bounded_at_their_limit() {
    assert_eq!(MAX_NODES, 200_000);
    assert!(xmp::read(wide(MAX_NODES).as_bytes(), None, &l()).is_ok());
    assert_eq!(xmp::read(wide(MAX_NODES + 1).as_bytes(), None, &l()), Err(Error::TooMany("XML nodes")));
    // Attributes and text count too.
    let attrs: String = (0..MAX_NODES).map(|i| format!(" a{i}=\"\"")).collect();
    assert_eq!(xmp::read(format!("<a{attrs}/>").as_bytes(), None, &l()), Err(Error::TooMany("XML nodes")));
    let texts = format!("<a>{}</a>", "t<!---->".repeat(MAX_NODES / 2));
    assert_eq!(xmp::read(texts.as_bytes(), None, &l()), Err(Error::TooMany("XML nodes")));
    // A bomb of many array items in a real packet stops at the limit too.
    let items = "<rdf:li>x</rdf:li>".repeat(MAX_NODES);
    let bomb = xmp_packet(&format!("<rdf:Description rdf:about=\"\" {NS}><dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject></rdf:Description>"));
    assert_eq!(xmp::read(&bomb, None, &l()), Err(Error::TooMany("XML nodes")));
    // Many namespace declarations and prefixed attributes stay linear (no quadratic lookups).
    let decls: String = (0..20_000).map(|i| format!(" xmlns:p{i}=\"u{i}\"")).collect();
    let uses: String = (0..20_000).map(|i| format!("<p{i}:e p{i}:a=\"1\"/>")).collect();
    assert!(xmp::read(format!("<r{decls}>{uses}</r>").as_bytes(), None, &l()).is_ok());
}

#[test]
fn size_and_encoding_are_checked() {
    let packet = xmp_packet(&element_form());
    assert!(xmp::read(&packet, None, &Limits { max_xmp: packet.len(), ..l() }).is_ok());
    assert_eq!(xmp::read(&packet, None, &Limits { max_xmp: packet.len() - 1, ..l() }), Err(Error::TooMany("XMP bytes")));
    assert!(matches!(xmp::read(b"<a>\xff\xfe</a>", None, &l()), Err(Error::Malformed(_))));
}

#[test]
fn truncation_and_byte_flips_never_panic() {
    let packet = xmp_packet(&format!(
        "{}<rdf:Description rdf:about=\"\" xmlns:u=\"http://example.com/u/\"><u:s rdf:parseType=\"Resource\"><u:f>&amp;&#x41;</u:f></u:s><!-- c --><u:c><![CDATA[x]]></u:c></rdf:Description>",
        element_form()
    ));
    for end in 0..packet.len() {
        let _ = xmp::read(&packet[..end], None, &l());
        let _ = xmp::rows(&packet[..end], Some(&packet[..end]), &l());
    }
    for at in 0..packet.len() {
        for flip in [0x01u8, 0x20, 0x80, 0xff] {
            let mut p = packet.clone();
            p[at] ^= flip;
            let _ = xmp::read(&p, None, &l());
            let _ = xmp::rows(&p, None, &l());
        }
    }
}

proptest! {
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = xmp::read(&bytes, Some(&bytes), &l());
        let _ = xmp::rows(&bytes, None, &l());
    }

    #[test]
    fn random_markup_never_panics(parts in proptest::collection::vec(prop::sample::select(vec![
        "<", ">", "/", "</", "/>", "=", "\"", "'", "&", ";", "&amp;", "&#x41;", "&#", "<?", "?>", "<!--", "-->", "<![CDATA[", "]]>", "<!",
        "rdf:", "RDF", "Description", "li", "Alt", "Bag", "Seq", "dc:", "description", "subject", "xml:lang", "x-default", "xmlns:rdf",
        "xmlns:dc", "\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"", "\"http://purl.org/dc/elements/1.1/\"", " ", "\n", "a", "é", "日",
        "rdf:parseType=\"Resource\"", "rdf:resource", "xpacket", "end",
    ]), 0..200)) {
        let s: String = parts.concat();
        let _ = xmp::read(s.as_bytes(), None, &l());
        let _ = xmp::rows(s.as_bytes(), None, &l());
    }
}
