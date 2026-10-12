//! `.xmp` sidecars for RAW files (`xmp-interop.md` §7): the sidecar's name for each case, reading
//! a RAW with its sidecar (else a DNG's embedded packet), the merged bytes of a whole sidecar file
//! with raw converters' develop settings and history kept byte for byte, damaged sidecars reported,
//! and hostile input (random bytes, every truncation, random folder listings) never panicking.

mod build;

use std::collections::HashSet;

use build::*;
use dalica_meta::xmp_sidecar::{self, RawMetadata, SidecarName, SidecarWrite, Sidecars, XmpFrom};
use dalica_meta::{BlockKind, Changes, DateTaken, Error, Limits, Place, WriteOptions, xmp};
use proptest::prelude::*;

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

fn l() -> Limits {
    Limits::default()
}

fn now() -> DateTaken {
    DateTaken::parse_iso("2026-10-12T09:30:00+02:00").unwrap()
}

/// Every write option on, written at [`now`].
fn all() -> WriteOptions {
    WriteOptions { captions_tags_places: true, stars_as_ratings: true, dates: true, metadata_date: Some(now()), limits: l() }
}

fn caption(c: &str) -> Changes {
    Changes { caption: Some(Some(c.to_owned())), ..Changes::default() }
}

fn full() -> Changes {
    Changes {
        caption: Some(Some("Pier at dawn, ünïcödé".to_owned())),
        tags: Some(vec!["sea".to_owned(), "trip/2026".to_owned(), "a, b".to_owned()]),
        star: Some(true),
        ..Changes::default()
    }
}

fn text(b: &[u8]) -> &str {
    std::str::from_utf8(b).unwrap()
}

// Name resolution.

/// The RAWs of these tests, by extension (the scan knows from the content).
fn is_raw(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("cr3" | "cr2" | "nef" | "dng" | "arw" | "raw")) || name == "RAWFILE"
}

fn sidecars(names: &[&str]) -> Sidecars {
    Sidecars::new(names, is_raw)
}

fn name(existing: Option<&str>, write: &str) -> SidecarName {
    SidecarName { existing: existing.map(str::to_owned), write: write.to_owned() }
}

#[test]
fn a_raw_alone_writes_stem_xmp() {
    let s = sidecars(&["IMG_0001.CR3"]);
    assert_eq!(s.get("IMG_0001.CR3"), Some(&name(None, "IMG_0001.xmp")));
    let s = sidecars(&["holiday.v2.NEF", "other.jpg"]);
    assert_eq!(s.get("holiday.v2.NEF"), Some(&name(None, "holiday.v2.xmp")));
    // A RAW without an extension.
    assert_eq!(sidecars(&["RAWFILE"]).get("RAWFILE"), Some(&name(None, "RAWFILE.xmp")));
}

#[test]
fn every_accepted_form_is_found_and_written_back_under_its_own_name() {
    for found in ["IMG_0001.xmp", "IMG_0001.XMP", "IMG_0001.Xmp", "IMG_0001.CR3.xmp", "IMG_0001.CR3.XMP"] {
        let s = sidecars(&["IMG_0001.CR3", found]);
        assert_eq!(s.get("IMG_0001.CR3"), Some(&name(Some(found), found)), "{found}");
        assert_eq!(s.owner(found), Some("IMG_0001.CR3"), "{found}");
    }
}

#[test]
fn the_stem_form_wins_then_the_lower_case_extension() {
    let all_four = ["IMG_0001.CR3.XMP", "IMG_0001.CR3.xmp", "IMG_0001.XMP", "IMG_0001.xmp", "IMG_0001.CR3"];
    let s = sidecars(&all_four);
    assert_eq!(s.get("IMG_0001.CR3").unwrap().existing.as_deref(), Some("IMG_0001.xmp"));
    // The others belong to no one.
    for other in ["IMG_0001.CR3.XMP", "IMG_0001.CR3.xmp", "IMG_0001.XMP"] {
        assert_eq!(s.owner(other), None, "{other}");
    }
    let s = sidecars(&["IMG_0001.CR3.xmp", "IMG_0001.XMP", "IMG_0001.CR3"]);
    assert_eq!(s.get("IMG_0001.CR3").unwrap().existing.as_deref(), Some("IMG_0001.XMP"));
    let s = sidecars(&["IMG_0001.CR3.XMP", "IMG_0001.CR3.xmp", "IMG_0001.CR3"]);
    assert_eq!(s.get("IMG_0001.CR3").unwrap().existing.as_deref(), Some("IMG_0001.CR3.xmp"));
}

#[test]
fn a_raw_jpeg_pair_s_stem_sidecar_is_the_raw_s() {
    for names in [&["IMG_0002.JPG", "IMG_0002.CR2"][..], &["IMG_0002.CR2", "IMG_0002.JPG", "IMG_0002.xmp"]] {
        let s = sidecars(names);
        assert_eq!(s.get("IMG_0002.JPG"), None, "the JPEG keeps its metadata inside itself");
        let raw = s.get("IMG_0002.CR2").unwrap();
        assert_eq!(raw.write, "IMG_0002.xmp");
        if names.contains(&"IMG_0002.xmp") {
            assert_eq!(s.owner("IMG_0002.xmp"), Some("IMG_0002.CR2"));
        }
    }
    // A sidecar named after the JPEG is not a RAW sidecar.
    let s = sidecars(&["IMG_0003.JPG", "IMG_0003.JPG.xmp"]);
    assert_eq!(s.owner("IMG_0003.JPG.xmp"), None);
    // A JPEG alone does not get one either.
    assert_eq!(sidecars(&["IMG_0004.jpg", "IMG_0004.xmp"]).owner("IMG_0004.xmp"), None);
}

#[test]
fn raws_sharing_a_stem_never_share_a_sidecar() {
    let s = sidecars(&["IMG_0005.NEF", "IMG_0005.DNG", "IMG_0005.JPG"]);
    assert_eq!(s.get("IMG_0005.DNG"), Some(&name(None, "IMG_0005.xmp")), "the first by name owns the stem");
    assert_eq!(s.get("IMG_0005.NEF"), Some(&name(None, "IMG_0005.NEF.xmp")));
    let s = sidecars(&["IMG_0005.NEF", "IMG_0005.DNG", "IMG_0005.xmp", "IMG_0005.NEF.xmp"]);
    assert_eq!(s.owner("IMG_0005.xmp"), Some("IMG_0005.DNG"));
    assert_eq!(s.owner("IMG_0005.NEF.xmp"), Some("IMG_0005.NEF"));
    // Only `<stem>.xmp` exists: the second RAW does not read it.
    let s = sidecars(&["IMG_0005.NEF", "IMG_0005.DNG", "IMG_0005.xmp"]);
    assert_eq!(s.get("IMG_0005.NEF").unwrap().existing, None);
}

#[test]
fn a_raw_named_the_stem_owns_it() {
    // `RAWFILE.xmp` is RAWFILE's own `<file>.xmp`, so `RAWFILE.CR3` may not take it as its stem
    // sidecar.
    let s = sidecars(&["RAWFILE.CR3", "RAWFILE", "RAWFILE.xmp"]);
    assert_eq!(s.get("RAWFILE"), Some(&name(Some("RAWFILE.xmp"), "RAWFILE.xmp")));
    assert_eq!(s.get("RAWFILE.CR3"), Some(&name(None, "RAWFILE.CR3.xmp")));
}

#[test]
fn names_differing_only_by_case_are_one_file() {
    // On Windows and macOS `img_0006.xmp` is IMG_0006.CR3's stem sidecar.
    let s = sidecars(&["IMG_0006.CR3", "img_0006.xmp"]);
    assert_eq!(s.get("IMG_0006.CR3"), Some(&name(Some("img_0006.xmp"), "img_0006.xmp")));
    // Two RAWs whose stems differ by case: one stem sidecar between them.
    let s = sidecars(&["IMG_0007.CR3", "img_0007.NEF"]);
    assert_eq!(s.get("IMG_0007.CR3").unwrap().write, "IMG_0007.xmp");
    assert_eq!(s.get("img_0007.NEF").unwrap().write, "img_0007.NEF.xmp");
    // The RAW's own spelling first.
    let s = sidecars(&["IMG_0008.CR3", "img_0008.xmp", "IMG_0008.xmp"]);
    assert_eq!(s.get("IMG_0008.CR3").unwrap().existing.as_deref(), Some("IMG_0008.xmp"));
}

#[test]
fn a_sidecar_name_is_never_another_file_s() {
    // A RAW (by content) oddly named like a sidecar keeps its name to itself.
    let s = sidecars(&["IMG_0009.CR3", "IMG_0009.xmp"]);
    let s2 = Sidecars::new(&["IMG_0009.CR3", "IMG_0009.xmp"], |n: &str| n.starts_with("IMG_0009"));
    assert_eq!(s.get("IMG_0009.CR3").unwrap().existing.as_deref(), Some("IMG_0009.xmp"));
    assert_eq!(s2.get("IMG_0009.CR3"), Some(&name(None, "IMG_0009.CR3.xmp")));
    assert_eq!(s2.get("IMG_0009.xmp"), Some(&name(None, "IMG_0009.xmp.xmp")));
    assert_eq!(s2.owner("IMG_0009.xmp"), None);
}

#[test]
fn listings_of_owned_strings_work_and_non_raws_and_strangers_have_nothing() {
    let names: Vec<String> = ["a.CR3", "a.xmp", "b.txt", "c.xmp", ".xmp", "x."].iter().map(|s| s.to_string()).collect();
    let s = Sidecars::new(&names, is_raw);
    assert_eq!(s.get("a.CR3").unwrap().existing.as_deref(), Some("a.xmp"));
    for n in ["b.txt", "c.xmp", "missing.CR3", ".xmp", "x."] {
        assert_eq!(s.get(n), None, "{n}");
        assert_eq!(s.owner(n), None, "{n}");
    }
    assert_eq!(Sidecars::new::<&str>(&[], is_raw), Sidecars::default());
}

#[test]
fn a_large_folder_resolves_quickly() {
    let names: Vec<String> = (0..50_000).flat_map(|i| [format!("IMG_{i:05}.CR3"), format!("IMG_{i:05}.JPG"), format!("IMG_{i:05}.xmp")]).collect();
    let started = std::time::Instant::now();
    let s = Sidecars::new(&names, is_raw);
    assert_eq!(s.owner("IMG_49999.xmp"), Some("IMG_49999.CR3"));
    // Linear-ish, not quadratic: 150,000 names well under the scan's budget even in debug builds.
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
}

// Sidecars of other applications (our own synthetic files, laid out the way raw developers do).

/// A raw developer's sidecar: no packet wrapper, develop settings as attributes and elements, a
/// history, and no property Dalica owns.
const DEVELOPER: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Other XMP Core 6.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#"
    xmp:CreatorTool="Other Raw Developer 13.0"
    xmp:ModifyDate="2024-03-01T10:00:00+01:00"
    tiff:Make="Example Camera Co"
    crs:Version="15.0"
    crs:WhiteBalance="As Shot"
    crs:Exposure2012="+0.35"
    crs:Contrast2012="+12"
    crs:HasSettings="True">
   <crs:ToneCurvePV2012>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>128, 140</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012>
   <!-- kept as written -->
   <xmpMM:History>
    <rdf:Seq>
     <rdf:li
      stEvt:action="saved"
      stEvt:when="2024-03-01T10:00:00+01:00"
      stEvt:softwareAgent="Other Raw Developer 13.0"
      stEvt:changed="/metadata"/>
    </rdf:Seq>
   </xmpMM:History>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

/// Another developer's sidecar: an XML declaration, a history of operations with encoded
/// parameters, and owned properties (rating, tags) in its own description.
const HISTORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.4.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:dev="http://example.com/developer/1.0/"
   xmp:Rating="3"
   dev:xmp_version="5"
   dev:history_end="2">
   <dev:history>
    <rdf:Seq>
     <rdf:li dev:operation="exposure" dev:enabled="1" dev:params="0000000000000000cdcc4c3e"/>
     <rdf:li dev:operation="colorin" dev:enabled="1" dev:params="gz12eJxjYGBgAAAABQAB"/>
    </rdf:Seq>
   </dev:history>
   <dc:subject>
    <rdf:Bag>
     <rdf:li>old tag</rdf:li>
    </rdf:Bag>
   </dc:subject>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

/// The same kind of sidecar inside a packet wrapper with padding.
fn wrapped(padding: usize) -> Vec<u8> {
    let inner = DEVELOPER.trim_end();
    format!("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n{inner}\n{}<?xpacket end=\"w\"?>", " ".repeat(padding)).into_bytes()
}

/// Dalica's description in a merged sidecar (the last description) and the file without it.
fn split_ours(packet: &[u8]) -> (String, String) {
    let s = text(packet);
    let close = s.rfind("</rdf:Description>").unwrap() + "</rdf:Description>".len();
    let open = s[..close].rfind("<rdf:Description").unwrap();
    let start = s[..open].trim_end_matches([' ', '\n']).len();
    (s[open..close].to_owned(), format!("{}{}", &s[..start], &s[close..]))
}

fn write(raw: Option<&[u8]>, sidecar: Option<&[u8]>, c: &Changes) -> SidecarWrite {
    xmp_sidecar::merge(raw, sidecar, c, &all()).unwrap()
}

fn written(w: SidecarWrite) -> Vec<u8> {
    match w {
        SidecarWrite::Write(b) => b,
        other => panic!("expected a write, got {other:?}"),
    }
}

fn read_back(raw: Option<&[u8]>, sidecar: &[u8]) -> RawMetadata {
    let r = xmp_sidecar::read(raw, Some(sidecar), &l());
    assert_eq!(r.damaged, None);
    assert_eq!(r.xmp_from, XmpFrom::Sidecar);
    r
}

fn assert_full(r: &RawMetadata) {
    let o = &r.metadata.owned;
    assert_eq!(o.caption.as_deref(), Some("Pier at dawn, ünïcödé"));
    assert_eq!(o.tags, ["sea", "trip/2026", "a, b"]);
    assert_eq!(o.rating, Some(5));
}

#[test]
fn develop_settings_and_history_are_kept_byte_for_byte() {
    let out = written(write(None, Some(DEVELOPER.as_bytes()), &full()));
    let (ours, rest) = split_ours(&out);
    assert_eq!(rest, DEVELOPER, "everything outside Dalica's description is the old file");
    assert!(ours.contains("<dc:description>") && ours.contains("<xmp:MetadataDate>2026-10-12T09:30:00+02:00</xmp:MetadataDate>"), "{ours}");
    assert_full(&read_back(None, &out));
    // Again with the same values: nothing to write.
    assert_eq!(write(None, Some(&out), &full()), SidecarWrite::Unchanged);
    // A change keeps the file around Dalica's description as it was.
    let again = written(write(None, Some(&out), &caption("Pier at noon")));
    assert_eq!(split_ours(&again).1, DEVELOPER);
    assert_eq!(read_back(None, &again).metadata.owned.caption.as_deref(), Some("Pier at noon"));
}

#[test]
fn owned_properties_move_out_of_another_application_s_description_and_nothing_else_does() {
    let out = written(write(None, Some(HISTORY.as_bytes()), &full()));
    let s = text(&out);
    assert!(s.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"XMP Core 4.4.0\">"));
    // The history and its parameters, byte for byte.
    let history = &HISTORY[HISTORY.find("   dev:xmp_version").unwrap()..HISTORY.find("   <dc:subject>").unwrap()];
    assert!(s.contains(history), "{s}");
    // The rating and the tags were cut from the developer's description, only them.
    let (_, rest) = split_ours(&out);
    let cut_rating = HISTORY.replace("\n   xmp:Rating=\"3\"", "");
    let cut_subject = cut_rating.replace("\n   <dc:subject>\n    <rdf:Bag>\n     <rdf:li>old tag</rdf:li>\n    </rdf:Bag>\n   </dc:subject>", "");
    assert_eq!(rest, cut_subject);
    assert_full(&read_back(None, &out));
}

#[test]
fn unstarring_keeps_another_application_s_rating() {
    assert_eq!(write(None, Some(HISTORY.as_bytes()), &Changes { star: Some(false), ..Changes::default() }), SidecarWrite::Unchanged);
}

#[test]
fn the_packet_wrapper_and_its_padding_are_kept() {
    let before = wrapped(2000);
    let out = written(write(None, Some(&before), &full()));
    assert_eq!(out.len(), before.len(), "the padding absorbs the change");
    let s = text(&out);
    assert!(s.starts_with("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta"));
    assert!(s.ends_with("<?xpacket end=\"w\"?>"));
    assert_full(&read_back(None, &out));
}

#[test]
fn a_new_sidecar_is_a_plain_xmpmeta_document() {
    let place = Place::new(-33.875, 151.21).unwrap();
    let c = Changes { place: Some(Some(place)), ..full() };
    let out = written(write(None, None, &c));
    let s = text(&out);
    assert!(s.starts_with("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF"), "{s}");
    assert!(s.ends_with("</x:xmpmeta>\n"), "no packet wrapper, no padding: {s}");
    assert!(!s.contains("xpacket"));
    let r = read_back(None, &out);
    assert_full(&r);
    assert!(r.metadata.owned.place.is_some_and(|p| p.close_to(&place, 1e-6)), "a sidecar takes the place too");
    // A blank file counts as none.
    assert_eq!(written(write(None, Some(b"  \n\0"), &c)), out);
    // The same again: nothing to change.
    assert_eq!(write(None, Some(&out), &c), SidecarWrite::Unchanged);
}

#[test]
fn nothing_to_write_creates_no_file() {
    assert_eq!(write(None, None, &Changes::default()), SidecarWrite::Unchanged);
    assert_eq!(write(None, None, &Changes { star: Some(false), ..Changes::default() }), SidecarWrite::Unchanged);
    // The options still decide: captions off, stars off.
    let off = WriteOptions { captions_tags_places: false, stars_as_ratings: false, dates: true, metadata_date: Some(now()), limits: l() };
    assert_eq!(xmp_sidecar::merge(None, None, &full(), &off).unwrap(), SidecarWrite::Unchanged);
    assert_eq!(xmp_sidecar::merge(None, Some(DEVELOPER.as_bytes()), &full(), &off).unwrap(), SidecarWrite::Unchanged);
    let stars_off = WriteOptions { stars_as_ratings: false, ..all() };
    let out = written(xmp_sidecar::merge(None, None, &full(), &stars_off).unwrap());
    assert_eq!(read_back(None, &out).metadata.owned.rating, None);
}

// A DNG: EXIF, and XMP embedded in IFD0 (tag 700).

const EMBEDDED: &str = r#"<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+1.00" xmp:Rating="5"><dc:description><rdf:Alt><rdf:li xml:lang="x-default">Embedded caption</rdf:li></rdf:Alt></dc:description><dc:subject><rdf:Bag><rdf:li>embedded</rdf:li></rdf:Bag></dc:subject></rdf:Description>"#;

fn dng(xmp_body: Option<&str>) -> Vec<u8> {
    let mut t = Tiff::new();
    t.ifd0.push(bytes(0xc612, &[1, 4, 0, 0])); // DNGVersion
    t.exif.push(ascii(0x9003, "2021:07:08 09:10:11"));
    if let Some(body) = xmp_body {
        let mut packet = xmp_packet(body);
        // Most writers pad the embedded packet.
        let end = packet.len() - "<?xpacket end=\"w\"?>".len();
        packet.splice(end..end, std::iter::repeat_n(b' ', 1000));
        t.ifd0.push(undefined(0x02bc, &packet));
    }
    tiff_file(t)
}

fn exif_date() -> DateTaken {
    DateTaken::parse_iso("2021-07-08T09:10:11").unwrap()
}

#[test]
fn a_dng_without_a_sidecar_reads_its_embedded_packet() {
    let raw = dng(Some(EMBEDDED));
    let r = xmp_sidecar::read(Some(&raw), None, &l());
    assert_eq!((r.xmp_from, r.damaged.clone()), (XmpFrom::Embedded, None));
    let o = &r.metadata.owned;
    assert_eq!(o.caption.as_deref(), Some("Embedded caption"));
    assert_eq!(o.tags, ["embedded"]);
    assert_eq!(o.rating, Some(5));
    assert_eq!(o.date_taken, Some(exif_date()));
    // A blank sidecar is no sidecar.
    assert_eq!(xmp_sidecar::read(Some(&raw), Some(b"\n"), &l()), r);
}

#[test]
fn a_sidecar_takes_the_place_of_the_embedded_packet_but_not_of_the_exif() {
    let raw = dng(Some(EMBEDDED));
    let sidecar = written(write(None, None, &caption("Sidecar caption")));
    let r = xmp_sidecar::read(Some(&raw), Some(&sidecar), &l());
    assert_eq!((r.xmp_from, r.damaged.clone()), (XmpFrom::Sidecar, None));
    let o = &r.metadata.owned;
    assert_eq!(o.caption.as_deref(), Some("Sidecar caption"));
    assert!(o.tags.is_empty(), "the sidecar is the RAW's XMP: the embedded tags no longer count");
    assert_eq!(o.rating, None);
    assert_eq!(o.date_taken, Some(exif_date()), "the RAW's EXIF still counts");
}

#[test]
fn a_new_sidecar_for_a_dng_starts_from_its_embedded_packet() {
    let raw = dng(Some(EMBEDDED));
    let out = written(write(Some(&raw), None, &Changes { tags: Some(vec!["new".to_owned()]), ..Changes::default() }));
    let s = text(&out);
    assert!(s.contains("crs:Exposure2012=\"+1.00\""), "the embedded packet's data comes along: {s}");
    assert!(!s.contains("     "), "a file needs no padding: {s}");
    let r = read_back(Some(&raw), &out);
    let o = &r.metadata.owned;
    assert_eq!(o.caption.as_deref(), Some("Embedded caption"), "values not changed stay as the RAW had them");
    assert_eq!(o.rating, Some(5));
    assert_eq!(o.tags, ["new"]);
    // Unstarring a DNG starred elsewhere does need one.
    let unstar = written(write(Some(&raw), None, &Changes { star: Some(false), ..Changes::default() }));
    assert_eq!(read_back(Some(&raw), &unstar).metadata.owned.rating, Some(0));
}

#[test]
fn raws_this_crate_cannot_read_still_have_sidecars() {
    // A RAW that is not TIFF-structured (CR3, RAF, …): nothing from the RAW itself.
    let cr3 = b"\0\0\0\x18ftypcrx \0\0\0\x01 and the rest of the file".to_vec();
    let r = xmp_sidecar::read(Some(&cr3), None, &l());
    assert_eq!((r.xmp_from, r.damaged, r.metadata.problems.len()), (XmpFrom::Nothing, None, 0));
    let out = written(write(Some(&cr3), None, &full()));
    assert_full(&read_back(Some(&cr3), &out));
    assert_eq!(xmp_sidecar::read(None, Some(&out), &l()).metadata.owned, read_back(Some(&cr3), &out).metadata.owned);
    // A DNG without an embedded packet.
    let raw = dng(None);
    assert_eq!(xmp_sidecar::read(Some(&raw), None, &l()).xmp_from, XmpFrom::Nothing);
    let out = written(write(Some(&raw), None, &full()));
    assert!(text(&out).starts_with("<x:xmpmeta"));
    // A TIFF-structured RAW whose structure is broken is reported; its sidecar still reads.
    let broken = b"II*\0\xff\xff\xff\x7f".to_vec();
    let r = xmp_sidecar::read(Some(&broken), Some(&out), &l());
    assert_eq!(r.metadata.problems.iter().map(|p| p.block).collect::<Vec<_>>(), [BlockKind::Structure]);
    assert_full(&r);
}

// Damaged sidecars.

fn damaged_sidecars() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("not XML", b"this is not XMP at all".to_vec()),
        ("cut short", DEVELOPER.as_bytes()[..DEVELOPER.len() / 2].to_vec()),
        ("mismatched tags", b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><a></b></x:xmpmeta>".to_vec()),
        ("no rdf:RDF", b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>".to_vec()),
        ("empty rdf:RDF", format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF_NS}\"/></x:xmpmeta>").into_bytes()),
        ("not UTF-8", b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\xff\xfe</x:xmpmeta>".to_vec()),
        ("unbound prefix", b"<x:xmpmeta><rdf:RDF/></x:xmpmeta>".to_vec()),
        (
            "a DOCTYPE",
            b"<?xml version=\"1.0\"?><!DOCTYPE lolz [<!ENTITY lol \"lol\">]><x:xmpmeta xmlns:x=\"adobe:ns:meta/\">&lol;</x:xmpmeta>".to_vec(),
        ),
        ("two roots", format!("{DEVELOPER}{DEVELOPER}").into_bytes()),
        ("too deep", format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">{}{}</x:xmpmeta>", "<a>".repeat(100), "</a>".repeat(100)).into_bytes()),
    ]
}

#[test]
fn a_damaged_sidecar_is_reported_and_replaced_by_a_new_one() {
    let raw = dng(Some(EMBEDDED));
    for (what, bad) in damaged_sidecars() {
        let r = xmp_sidecar::read(Some(&raw), Some(&bad), &l());
        assert!(r.damaged.is_some(), "{what}");
        assert_eq!(r.xmp_from, XmpFrom::Embedded, "{what}: the embedded packet is read instead");
        assert_eq!(r.metadata.owned.caption.as_deref(), Some("Embedded caption"), "{what}");
        assert!(xmp_sidecar::well_formed(&bad, &l()).is_err(), "{what}");

        match write(Some(&raw), Some(&bad), &caption("New")) {
            SidecarWrite::ReplaceDamaged { bytes, damaged } => {
                assert_eq!(Some(damaged), r.damaged, "{what}");
                let back = read_back(Some(&raw), &bytes);
                assert_eq!(back.metadata.owned.caption.as_deref(), Some("New"), "{what}");
                assert_eq!(back.metadata.owned.tags, ["embedded"], "{what}: started from the embedded packet");
            }
            other => panic!("{what}: {other:?}"),
        }
        // Without a RAW to start from, the new sidecar is a plain one.
        match write(None, Some(&bad), &caption("New")) {
            SidecarWrite::ReplaceDamaged { bytes, .. } => assert!(text(&bytes).starts_with("<x:xmpmeta"), "{what}"),
            other => panic!("{what}: {other:?}"),
        }
        // Nothing to write: the damaged file is left as it is.
        assert_eq!(write(Some(&raw), Some(&bad), &Changes::default()), SidecarWrite::Unchanged, "{what}");
    }
}

#[test]
fn a_sidecar_past_the_size_limit_is_damaged() {
    let limits = Limits { max_xmp: DEVELOPER.len() - 1, ..l() };
    let r = xmp_sidecar::read(None, Some(DEVELOPER.as_bytes()), &limits);
    assert_eq!(r.damaged, Some(Error::TooMany("XMP bytes")));
    let at_limit = Limits { max_xmp: DEVELOPER.len(), ..l() };
    assert_eq!(xmp_sidecar::read(None, Some(DEVELOPER.as_bytes()), &at_limit).damaged, None);
}

#[test]
fn a_damaged_embedded_packet_falls_back_to_a_plain_new_sidecar() {
    let raw = dng(Some("<rdf:Description rdf:about=\"\"><unclosed></rdf:Description>"));
    let r = xmp_sidecar::read(Some(&raw), None, &l());
    assert!(r.metadata.problems.iter().any(|p| p.block == BlockKind::Xmp));
    let out = written(write(Some(&raw), None, &caption("x")));
    assert!(text(&out).starts_with("<x:xmpmeta"));
    // With a readable sidecar the embedded packet's problem no longer counts.
    let r = xmp_sidecar::read(Some(&raw), Some(&out), &l());
    assert!(r.metadata.problems.is_empty(), "{:?}", r.metadata.problems);
}

// Hostile input.

/// Whatever comes back is safe to write: well-formed, and it reads back as asked.
fn check_write(raw: Option<&[u8]>, sidecar: Option<&[u8]>, c: &Changes) {
    let Ok(w) = xmp_sidecar::merge(raw, sidecar, c, &all()) else { return };
    let bytes = match w {
        SidecarWrite::Unchanged => return,
        SidecarWrite::Write(b) => b,
        SidecarWrite::ReplaceDamaged { bytes, .. } => bytes,
    };
    assert_eq!(xmp_sidecar::well_formed(&bytes, &l()), Ok(()));
    let r = xmp_sidecar::read(raw, Some(&bytes), &l());
    assert_eq!(r.damaged, None);
    if let Some(Some(want)) = &c.caption {
        assert_eq!(r.metadata.owned.caption.as_deref(), Some(want.as_str()));
    }
}

#[test]
fn every_truncation_and_flip_of_a_sidecar_is_handled() {
    for base in [DEVELOPER.as_bytes(), HISTORY.as_bytes(), &wrapped(64)] {
        for cut in 0..=base.len() {
            let s = &base[..cut];
            let _ = xmp_sidecar::read(None, Some(s), &l());
            check_write(None, Some(s), &caption("cut"));
        }
        for at in (0..base.len()).step_by(7) {
            let mut s = base.to_vec();
            s[at] ^= 0x5a;
            let _ = xmp_sidecar::read(None, Some(&s), &l());
            check_write(None, Some(&s), &caption("flip"));
        }
    }
}

#[test]
fn every_truncation_of_a_dng_is_handled() {
    let raw = dng(Some(EMBEDDED));
    for cut in 0..=raw.len() {
        let r = &raw[..cut];
        let _ = xmp_sidecar::read(Some(r), None, &l());
        check_write(Some(r), None, &caption("cut"));
    }
}

/// Some bytes that look a little like XMP.
fn xmpish() -> impl Strategy<Value = Vec<u8>> {
    let token = prop::sample::select(vec![
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">",
        "</x:xmpmeta>",
        "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
        "</rdf:RDF>",
        "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">",
        "</rdf:Description>",
        "<dc:subject><rdf:Bag><rdf:li>t</rdf:li></rdf:Bag></dc:subject>",
        "<?xpacket end=\"w\"?>",
        "<!-- c -->",
        " ",
        "\n",
        "&amp;",
        "<",
        ">",
    ]);
    prop::collection::vec(token, 0..24).prop_map(|t| t.concat().into_bytes())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(sidecar in prop::collection::vec(any::<u8>(), 0..512), raw in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = xmp_sidecar::read(Some(&raw), Some(&sidecar), &l());
        let _ = xmp_sidecar::well_formed(&sidecar, &l());
        check_write(Some(&raw), Some(&sidecar), &full());
        check_write(None, Some(&sidecar), &caption("x"));
    }

    #[test]
    fn random_markup_never_panics(sidecar in xmpish(), body in xmpish()) {
        let raw = dng(Some(&String::from_utf8_lossy(&body)));
        let _ = xmp_sidecar::read(Some(&raw), Some(&sidecar), &l());
        check_write(Some(&raw), Some(&sidecar), &caption("x"));
        check_write(Some(&raw), None, &full());
    }

    /// In any folder: no two RAWs share a sidecar name (without case), none is another file's
    /// name, and each existing sidecar's owner is its RAW.
    #[test]
    fn random_folders_resolve_consistently(names in prop::collection::vec(
        prop::sample::select(vec!["IMG", "img", "IMG.CR3", "img.cr3", "IMG.NEF", "IMG.JPG", "IMG.xmp", "IMG.XMP", "img.xmp", "IMG.CR3.xmp", "IMG.cr3.XMP", "IMG.NEF.xmp", "a.b", "a.b.CR3", "a.xmp", "a.b.xmp", "RAWFILE", "RAWFILE.xmp", ".xmp", "x.", "IMG.xmp.xmp"]),
        0..12,
    ), raw_xmp in any::<bool>()) {
        // Sometimes a file named like a sidecar is a RAW by content.
        let raw = |n: &str| is_raw(n) || (raw_xmp && n == "IMG.xmp");
        let s = Sidecars::new(&names, raw);
        let mut given = HashSet::new();
        let lowered: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
        // A folder lists each name once (the index copes with repeats too).
        let distinct: HashSet<&str> = names.iter().copied().collect();
        for &n in distinct.iter().filter(|&&n| raw(n)) {
            let Some(sc) = s.get(n) else { continue };
            prop_assert!(given.insert(sc.write.to_lowercase()), "{names:?}: {n} → {sc:?}");
            prop_assert!(sc.write.to_lowercase().ends_with(".xmp"));
            prop_assert!(names.iter().filter(|&&m| raw(m)).all(|m| m.to_lowercase() != sc.write.to_lowercase()), "{names:?}: {n} → {sc:?}");
            match &sc.existing {
                Some(e) => {
                    prop_assert_eq!(e, &sc.write);
                    prop_assert_eq!(s.owner(e), Some(n));
                }
                // A new name is no file's yet.
                None => prop_assert!(!lowered.contains(&sc.write.to_lowercase()), "{names:?}: {n} → {sc:?}"),
            }
        }
        for &n in names.iter().filter(|&&n| !raw(n)) {
            prop_assert_eq!(s.get(n), None);
        }
    }
}

#[test]
fn the_sidecar_read_matches_the_xmp_reader() {
    // The sidecar's owned values are exactly what the XMP reader finds in it.
    let out = written(write(None, Some(HISTORY.as_bytes()), &full()));
    let x = xmp::read(&out, None, &l()).unwrap();
    assert_eq!(read_back(None, &out).metadata.sources.xmp, x);
}
