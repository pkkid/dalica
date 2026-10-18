//! MWG face regions (`xmp-interop.md` §6 and §8, item 5): pixel and normalised areas, every EXIF
//! orientation and `mwg-rs:Rotation` read back to the same upright boxes; named faces written into
//! every container that takes XMP and read back; non-face regions and a matched entry's other
//! fields kept byte for byte; the same write twice giving the same file; long region lists bounded.

use crate::build;

use crate::common::{caption, l, now, text, xmp_text};
use build::*;
use dalica_core::Rect01;
use dalica_meta::fields::FaceRegion;
use dalica_meta::write::{NamedFace, NamedFaces};
use dalica_meta::xmp::{self, regions::MAX_FACES};
use dalica_meta::xmp_sidecar::{self, SidecarWrite};
use dalica_meta::{Changes, WriteOptions, write_owned};

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const MWG: &str = r#"xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/" xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#" xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#""#;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

fn near(r: &Rect01, want: [f64; 4]) -> bool {
    [r.x0() - want[0], r.y0() - want[1], r.x1() - want[2], r.y1() - want[3]].iter().all(|d| d.abs() < 1e-6)
}

/// A pretty packet whose one description holds `mwg-rs:Regions` around `inner`, plus a foreign
/// property, the way most writers lay it out.
fn packet(inner: &str) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"{RDF_NS}\">\n  <rdf:Description rdf:about=\"\" xmlns:zz=\"http://example.com/zz/\" {MWG}>\n   <zz:keep>other application</zz:keep>\n   <mwg-rs:Regions rdf:parseType=\"Resource\">\n{inner}\n   </mwg-rs:Regions>\n  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n{}<?xpacket end=\"w\"?>",
        " ".repeat(512)
    )
    .into_bytes()
}

/// `AppliedToDimensions` and a `RegionList` bag of `items`.
fn list(dims: Option<(u32, u32)>, items: &[String]) -> String {
    let dims =
        dims.map_or(String::new(), |(w, h)| format!("    <mwg-rs:AppliedToDimensions stDim:w=\"{w}\" stDim:h=\"{h}\" stDim:unit=\"pixel\"/>\n"));
    format!("{dims}    <mwg-rs:RegionList>\n     <rdf:Bag>\n{}\n     </rdf:Bag>\n    </mwg-rs:RegionList>", items.join("\n"))
}

/// One entry in the `parseType="Resource"` form: its fields as attributes of the item, and an area.
fn li(attrs: &str, area: &str) -> String {
    format!("      <rdf:li rdf:parseType=\"Resource\" {attrs}>\n       <mwg-rs:Area {area}/>\n      </rdf:li>")
}

fn faces(packet: &[u8]) -> Vec<FaceRegion> {
    xmp::read(packet, None, &l()).unwrap().faces
}

fn named(faces: &[(&str, Rect01)]) -> NamedFaces {
    NamedFaces { width: 4000, height: 3000, faces: faces.iter().map(|(n, r)| NamedFace { name: (*n).to_owned(), rect: *r }).collect() }
}

fn change(faces: &[(&str, Rect01)]) -> Changes {
    Changes { faces: Some(named(faces)), ..Changes::default() }
}

fn people() -> WriteOptions {
    WriteOptions { people_names: true, ..WriteOptions::default() }
}

/// Whether `got` holds exactly the named faces of `want` (any order).
fn same(got: &[FaceRegion], want: &[(&str, Rect01)]) -> bool {
    let want: Vec<(String, Rect01)> = want.iter().map(|(n, r)| ((*n).to_owned(), *r)).collect();
    dalica_meta::xmp::regions::same_faces(got, &want)
}

fn merge(packet: Option<&[u8]>, c: &Changes) -> Option<Vec<u8>> {
    xmp::merge(packet, c, &now(), &l()).unwrap().map(|m| m.into_bytes())
}

#[test]
fn pixel_and_normalised_areas_read_as_upright_boxes() {
    let items = [
        // Normalised, the default unit.
        li(r#"mwg-rs:Name="Ana" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.4" stArea:w="0.2" stArea:h="0.3" stArea:unit="normalized""#),
        // The same box in pixels of the applied dimensions.
        li(r#"mwg-rs:Name="Ben" mwg-rs:Type="Face""#, r#"stArea:x="2000" stArea:y="1200" stArea:w="800" stArea:h="900" stArea:unit="pixel""#),
        // A nested description, the type as an element, a box over the right edge (clipped).
        r#"      <rdf:li><rdf:Description mwg-rs:Name="Cy"><mwg-rs:Type>face</mwg-rs:Type><mwg-rs:Area stArea:x="0.95" stArea:y="0.1" stArea:w="0.2" stArea:h="0.1"/></rdf:Description></rdf:li>"#.to_owned(),
        // Unnamed.
        li(r#"mwg-rs:Type="Face""#, r#"stArea:x="0.1" stArea:y="0.9" stArea:w="0.1" stArea:h="0.1""#),
        // Other types are ignored; bad boxes are skipped, the rest kept.
        li(r#"mwg-rs:Name="Rex" mwg-rs:Type="Pet""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0.2" stArea:h="0.2""#),
        li(r#"mwg-rs:Name="Zero" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0" stArea:h="0.2""#),
        li(r#"mwg-rs:Name="Neg" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0.1" stArea:h="-0.2""#),
        li(r#"mwg-rs:Name="NaN" mwg-rs:Type="Face""#, r#"stArea:x="NaN" stArea:y="0.5" stArea:w="0.1" stArea:h="0.2""#),
        li(r#"mwg-rs:Name="Inf" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="inf" stArea:h="0.2""#),
        li(r#"mwg-rs:Name="Off" mwg-rs:Type="Face""#, r#"stArea:x="2.5" stArea:y="0.5" stArea:w="0.1" stArea:h="0.2""#),
        li(r#"mwg-rs:Name="Inch" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0.1" stArea:h="0.2" stArea:unit="inch""#),
        li(r#"mwg-rs:Name="NoBox" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5""#),
    ];
    let got = faces(&packet(&list(Some((4000, 3000)), &items)));
    let names: Vec<Option<&str>> = got.iter().map(|f| f.name.as_deref()).collect();
    assert_eq!(names, [Some("Ana"), Some("Ben"), Some("Cy"), None], "{got:?}");
    assert!(near(&got[0].rect, [0.4, 0.25, 0.6, 0.55]) && near(&got[1].rect, [0.4, 0.25, 0.6, 0.55]), "{got:?}");
    assert!(near(&got[2].rect, [0.85, 0.05, 1.0, 0.15]) && near(&got[3].rect, [0.05, 0.85, 0.15, 0.95]), "{got:?}");
    // A box in pixels without the dimensions to divide by is skipped.
    let undimensioned = faces(&packet(&list(None, &items)));
    assert_eq!(undimensioned.iter().filter_map(|f| f.name.as_deref()).collect::<Vec<_>>(), ["Ana", "Cy"]);
    // The resolved fields carry them too.
    let file = jpeg(&[app1_xmp(&packet(&list(Some((4000, 3000)), &items)))]);
    assert_eq!(dalica_meta::read(&file, &l()).unwrap().owned.faces, got);
}

/// Regions are on the EXIF-upright photo: whatever the orientation, the boxes read are the boxes
/// written (`data-model.md` §3.3: the stored frame needs no conversion).
#[test]
fn every_orientation_reads_the_same_upright_boxes() {
    let items = [
        li(r#"mwg-rs:Name="Ana" mwg-rs:Type="Face""#, r#"stArea:x="0.3" stArea:y="0.4" stArea:w="0.2" stArea:h="0.3""#),
        li(r#"mwg-rs:Name="Ben" mwg-rs:Type="Face""#, r#"stArea:x="300" stArea:y="1500" stArea:w="400" stArea:h="600" stArea:unit="pixel""#),
    ];
    let packet = packet(&list(Some((2000, 3000)), &items));
    for orientation in 1..=8u16 {
        let exif = Tiff { ifd0: vec![short(0x0112, orientation)], ..Tiff::new() }.build();
        let file = jpeg(&[app1_exif(&exif), app1_xmp(&packet)]);
        let got = dalica_meta::read(&file, &l()).unwrap().owned.faces;
        assert_eq!(got.len(), 2, "orientation {orientation}");
        assert!(near(&got[0].rect, [0.2, 0.25, 0.4, 0.55]) && near(&got[1].rect, [0.05, 0.4, 0.25, 0.6]), "orientation {orientation}: {got:?}");
    }
}

/// A box given in the sensor's frame with `mwg-rs:Rotation` (−π/2 for orientation 6, +π/2 for 8,
/// ±π for 3) reads as the upright box; a write into such an entry keeps its rotation and reads back.
#[test]
fn rotation_turns_sensor_boxes_upright() {
    let area = r#"stArea:x="0.3" stArea:y="0.4" stArea:w="0.2" stArea:h="0.3""#;
    let items: Vec<String> = ["-1.57080", "1.57080", "0", "3.14159", "-3.14159", "4.71239"]
        .iter()
        .enumerate()
        .map(|(i, r)| li(&format!(r#"mwg-rs:Name="P{i}" mwg-rs:Type="Face" mwg-rs:Rotation="{r}""#), area))
        .collect();
    let packet = packet(&list(Some((3000, 4000)), &items));
    let got = faces(&packet);
    let want = [
        [0.45, 0.2, 0.75, 0.4],
        [0.25, 0.6, 0.55, 0.8],
        [0.2, 0.25, 0.4, 0.55],
        [0.6, 0.45, 0.8, 0.75],
        [0.6, 0.45, 0.8, 0.75],
        [0.45, 0.2, 0.75, 0.4],
    ];
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!(near(&g.rect, w), "{g:?} != {w:?}");
    }
    // Move the −π/2 face a little and rename it: the entry keeps its rotation, and reads back.
    let moved = rect(0.46, 0.21, 0.76, 0.41);
    let mut keep: Vec<(String, Rect01)> = got.iter().map(|f| (f.name.clone().unwrap(), f.rect)).collect();
    keep[0] = ("Turned".to_owned(), moved);
    let keep: Vec<(&str, Rect01)> = keep.iter().map(|(n, r)| (n.as_str(), *r)).collect();
    let out = merge(Some(&packet), &change(&keep)).unwrap();
    let back = faces(&out);
    assert!(same(&back, &keep), "{back:?}");
    let t = text(&out);
    assert!(t.contains(r#"mwg-rs:Name="Turned" mwg-rs:Type="Face" mwg-rs:Rotation="-1.57080""#), "{t}");
    // Only the turned entry's area changed; the others are as they were.
    assert_eq!(t.matches(area).count(), 5);
}

/// Every container that writes XMP takes the faces, a packet or none; they read back, and writing
/// the same faces again gives the same file.
#[test]
fn a_write_then_a_read_returns_every_named_face_in_every_container() {
    let faces = [("Ana", rect(0.1, 0.1, 0.3, 0.4)), ("Ben Ödön <&>", rect(0.5, 0.2, 0.7, 0.5)), ("  ", rect(0.8, 0.8, 0.9, 0.9))];
    let written = &faces[..2];
    let foreign = packet(&list(
        Some((4000, 3000)),
        &[li(r#"mwg-rs:Name="Rex" mwg-rs:Type="Pet""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0.2" stArea:h="0.2""#)],
    ));
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("jpeg, no packet", jpeg(&[])),
        ("jpeg", jpeg(&[app1_xmp(&foreign)])),
        ("png, no packet", png(&[])),
        ("png", png(&[(b"iTXt", itxt("XML:com.adobe.xmp", &foreign, false))])),
        ("webp, no packet", webp(&[])),
        ("webp", webp(&[(b"XMP ", foreign.clone())])),
        ("tiff, no packet", tiff_file(Tiff::new())),
        ("tiff", tiff_file(Tiff { ifd0: vec![bytes(0x02bc, &foreign)], ..Tiff::new() })),
    ];
    for (name, file) in files {
        // With the option off nothing is written.
        assert_eq!(write_owned(&file, &change(&faces), &WriteOptions::default()).unwrap(), file, "{name}");
        let once = write_owned(&file, &change(&faces), &people()).unwrap();
        assert_ne!(once, file, "{name}");
        let got = dalica_meta::read(&once, &l()).unwrap().owned.faces;
        assert!(same(&got, written), "{name}: {got:?}");
        assert_eq!(write_owned(&once, &change(&faces), &people()).unwrap(), once, "{name}: a second write changed the file");
        // A later caption keeps them; a changed face list replaces them.
        let captioned = write_owned(&once, &caption("Harbour"), &people()).unwrap();
        assert!(same(&dalica_meta::read(&captioned, &l()).unwrap().owned.faces, written), "{name}");
        let next = [("Ana Maria", rect(0.11, 0.1, 0.31, 0.4)), ("Cy", rect(0.4, 0.6, 0.5, 0.75))];
        let again = write_owned(&captioned, &change(&next), &people()).unwrap();
        let got = dalica_meta::read(&again, &l()).unwrap().owned;
        assert!(same(&got.faces, &next) && got.caption.as_deref() == Some("Harbour"), "{name}: {got:?}");
        // Removing every face leaves the other regions.
        let none = write_owned(&again, &change(&[]), &people()).unwrap();
        assert!(dalica_meta::read(&none, &l()).unwrap().owned.faces.is_empty(), "{name}");
        if name.contains("no packet") {
            continue;
        }
        assert!(xmp_text(&none).contains(r#"mwg-rs:Name="Rex" mwg-rs:Type="Pet""#), "{name}");
    }
    // An `.xmp` sidecar for a RAW file: new, then merged into.
    let SidecarWrite::Write(sidecar) = xmp_sidecar::merge(None, None, &change(&faces), &people()).unwrap() else { panic!("a sidecar") };
    assert!(same(&xmp::read(&sidecar, None, &l()).unwrap().faces, written));
    assert_eq!(xmp_sidecar::merge(None, Some(&sidecar), &change(&faces), &people()).unwrap(), SidecarWrite::Unchanged);
}

/// Non-face regions, the other fields of a matched entry, unreadable face entries and the rest of
/// the packet stay byte for byte; unmatched face entries go; new faces are appended.
#[test]
fn foreign_regions_and_a_matched_entrys_fields_survive_a_write() {
    let pet = li(r#"mwg-rs:Name="Rex" mwg-rs:Type="Pet""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0.2" stArea:h="0.2""#);
    let matched = "      <rdf:li rdf:parseType=\"Resource\" mwg-rs:Type=\"Face\" mwg-rs:Description=\"by the pier\">\n       <mwg-rs:Name>Old name</mwg-rs:Name>\n       <mwg-rs:Area stArea:x=\"0.2\" stArea:y=\"0.25\" stArea:w=\"0.2\" stArea:h=\"0.3\" stArea:unit=\"normalized\" stArea:d=\"0\"/>\n       <mwg-rs:Extensions rdf:parseType=\"Resource\"><zz:score>0.97</zz:score></mwg-rs:Extensions>\n      </rdf:li>".to_owned();
    let gone = li(r#"mwg-rs:Name="Gone" mwg-rs:Type="Face""#, r#"stArea:x="0.8" stArea:y="0.8" stArea:w="0.1" stArea:h="0.1""#);
    let broken = li(r#"mwg-rs:Name="Broken" mwg-rs:Type="Face""#, r#"stArea:x="0.5" stArea:y="0.5" stArea:w="0" stArea:h="0.1""#);
    let focus = r#"      <rdf:li><rdf:Description mwg-rs:Type="Focus"><mwg-rs:Area stArea:x="0.5" stArea:y="0.5" stArea:w="0.01" stArea:h="0.01"/></rdf:Description></rdf:li>"#.to_owned();
    let before = packet(&list(Some((4000, 3000)), &[pet.clone(), matched.clone(), gone.clone(), broken.clone(), focus.clone()]));
    // The matched face moved a little (IoU ≥ 0.5) and was renamed; a new face was added.
    let want = [("Ana", rect(0.11, 0.1, 0.31, 0.4)), ("Ben", rect(0.6, 0.1, 0.7, 0.3))];
    let after = merge(Some(&before), &change(&want)).unwrap();
    let t = text(&after);
    for kept in [&pet, &broken, &focus] {
        assert!(t.contains(kept.as_str()), "{kept}\n{t}");
    }
    assert!(!t.contains("Gone"), "{t}");
    // The matched entry: the name and the box changed, nothing else.
    let renamed = matched.replace("Old name", "Ana").replace(
        r#"stArea:x="0.2" stArea:y="0.25" stArea:w="0.2" stArea:h="0.3""#,
        r#"stArea:x="0.21" stArea:y="0.25" stArea:w="0.2" stArea:h="0.3""#,
    );
    assert!(t.contains(&renamed), "{renamed}\n{t}");
    assert!(t.contains("<zz:keep>other application</zz:keep>") && t.contains("stDim:w=\"4000\""));
    assert!(same(&faces(&after), &want), "{:?}", faces(&after));
    // The new entry reuses the packet's prefixes (nothing redeclared) and goes at the end.
    for ns in ["xmlns:rdf", "xmlns:mwg-rs", "xmlns:stArea", "xmlns:stDim"] {
        assert_eq!(t.matches(ns).count(), 1, "{ns}\n{t}");
    }
    assert!(t.find(focus.as_str()) < t.find(">Ben<"), "{t}");
    // Writing the faces as read gives nothing to do: a box in pixels stays in pixels.
    assert_eq!(merge(Some(&after), &change(&want)), None);
    let pixels = packet(&list(
        Some((4000, 3000)),
        &[li(r#"mwg-rs:Name="Px" mwg-rs:Type="Face""#, r#"stArea:x="2000" stArea:y="1200" stArea:w="800" stArea:h="900" stArea:unit="pixel""#)],
    ));
    let read: Vec<(String, Rect01)> = faces(&pixels).into_iter().map(|f| (f.name.unwrap(), f.rect)).collect();
    let read: Vec<(&str, Rect01)> = read.iter().map(|(n, r)| (n.as_str(), *r)).collect();
    assert_eq!(merge(Some(&pixels), &change(&read)), None);
    // A moved pixel box is written in pixels.
    let out = merge(Some(&pixels), &change(&[("Px", rect(0.425, 0.25, 0.625, 0.55))])).unwrap();
    assert!(text(&out).contains(r#"stArea:x="2100" stArea:y="1200" stArea:w="800" stArea:h="900" stArea:unit="pixel""#), "{}", text(&out));
}

/// The shapes a packet's regions take: other prefixes, no list, no array, empty elements, regions
/// without dimensions; each written into and read back.
#[test]
fn regions_of_every_shape_take_the_faces() {
    let want = [("Ana", rect(0.1, 0.1, 0.3, 0.4)), ("Ben", rect(0.5, 0.2, 0.7, 0.5))];
    let other = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><R:RDF xmlns:R=\"{RDF_NS}\"><R:Description R:about=\"\" xmlns:m=\"http://www.metadataworkinggroup.com/schemas/regions/\" xmlns:a=\"http://ns.adobe.com/xmp/sType/Area#\"><m:Regions><R:Description><m:RegionList><R:Seq><R:li R:parseType=\"Resource\"><m:Type>Face</m:Type><m:Area a:x=\"0.2\" a:y=\"0.25\" a:w=\"0.2\" a:h=\"0.3\"/></R:li></R:Seq></m:RegionList></R:Description></m:Regions></R:Description></R:RDF></x:xmpmeta>"
    );
    let shapes: Vec<(&str, String)> = vec![
        ("other prefixes", other),
        ("no list", String::from_utf8(packet("    <mwg-rs:AppliedToDimensions stDim:w=\"1\" stDim:h=\"1\" stDim:unit=\"pixel\"/>")).unwrap()),
        ("no array", String::from_utf8(packet("    <mwg-rs:RegionList/>")).unwrap()),
        ("empty array", String::from_utf8(packet("    <mwg-rs:RegionList><rdf:Bag/></mwg-rs:RegionList>")).unwrap()),
        (
            "empty regions",
            String::from_utf8(packet(""))
                .unwrap()
                .replace("<mwg-rs:Regions rdf:parseType=\"Resource\">\n\n   </mwg-rs:Regions>", "<mwg-rs:Regions rdf:parseType=\"Resource\"/>"),
        ),
        ("no dimensions", String::from_utf8(packet(&list(None, &[]))).unwrap()),
    ];
    for (name, p) in shapes {
        let out = merge(Some(p.as_bytes()), &change(&want)).unwrap_or_else(|| panic!("{name}"));
        assert!(same(&faces(&out), &want), "{name}: {:?}\n{}", faces(&out), text(&out));
        assert_eq!(merge(Some(&out), &change(&want)), None, "{name}: a second write changed it\n{}", text(&out));
        if name == "other prefixes" {
            // The unnamed face was matched and named in place, with the packet's own prefixes.
            assert!(text(&out).contains("<m:Name>Ana</m:Name>") && !text(&out).contains("xmlns:mwg-rs"), "{}", text(&out));
        }
    }
    // Regions in attribute form are not a structure: refused, never replaced.
    let attr = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF_NS}\"><rdf:Description {MWG} mwg-rs:Regions=\"x\"/></rdf:RDF></x:xmpmeta>"
    );
    assert!(xmp::merge(Some(attr.as_bytes()), &change(&want), &now(), &l()).is_err());
    // A new packet's regions sit in Dalica's description, self-contained, and stay there.
    let new = merge(None, &change(&want)).unwrap();
    let t = text(&new);
    assert!(t.contains("<mwg-rs:Regions xmlns:rdf=") && t.contains("stDim:w=\"4000\" stDim:h=\"3000\""), "{t}");
    let renamed = merge(Some(&new), &change(&[("Ana B", rect(0.1, 0.1, 0.3, 0.4)), ("Ben", rect(0.5, 0.2, 0.7, 0.5))])).unwrap();
    assert_eq!(text(&renamed).matches("<rdf:Description").count(), 1, "{}", text(&renamed));
    assert!(text(&renamed).contains("<mwg-rs:Name>Ana B</mwg-rs:Name>"));
}

/// A region list far past anything real stays bounded: the reader keeps at most [`MAX_FACES`]
/// faces, a write leaves exactly the faces given, and a list past the XML node cap is refused.
#[test]
fn a_region_list_of_100_000_entries_is_bounded() {
    let entry = |i: usize| {
        let x = 0.05 + (i % 9) as f64 * 0.1;
        format!(
            r#"<rdf:li mwg-rs:Name="P{i}" mwg-rs:Type="Face"><mwg-rs:Area stArea:x="{x}" stArea:y="0.5" stArea:w="0.05" stArea:h="0.05"/></rdf:li>"#
        )
    };
    let build = |n: usize| {
        let items: Vec<String> = (0..n).map(entry).collect();
        packet(&format!("    <mwg-rs:RegionList><rdf:Bag>{}</rdf:Bag></mwg-rs:RegionList>", items.concat()))
    };
    let huge = build(100_000);
    assert!(xmp::read(&huge, None, &l()).is_err());
    assert!(xmp::merge(Some(&huge), &change(&[("Ana", rect(0.1, 0.1, 0.3, 0.4))]), &now(), &l()).is_err());
    let many = build(20_000);
    assert_eq!(faces(&many).len(), MAX_FACES);
    let want = [("Ana", rect(0.02, 0.47, 0.08, 0.53)), ("Ben", rect(0.1, 0.1, 0.3, 0.4))];
    let out = merge(Some(&many), &change(&want)).unwrap();
    assert!(same(&faces(&out), &want), "{:?}", faces(&out));
    // The matched entry kept its place and its other fields; every other face entry went.
    assert!(text(&out).contains(r#"<rdf:li mwg-rs:Name="Ana" mwg-rs:Type="Face"><mwg-rs:Area stArea:x="0.05""#));
    assert_eq!(text(&out).matches("<rdf:li").count(), 2);
    // More faces than a photo holds: the first MAX_FACES are written.
    let lots: Vec<(String, Rect01)> = (0..MAX_FACES + 10).map(|i| (format!("P{i}"), rect(0.0, 0.0, 0.5, 0.5))).collect();
    let lots: Vec<(&str, Rect01)> = lots.iter().map(|(n, r)| (n.as_str(), *r)).collect();
    let out = merge(None, &change(&lots)).unwrap();
    assert_eq!(faces(&out).len(), MAX_FACES);
}
