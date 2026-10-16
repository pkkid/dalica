//! W08, Foreign metadata survives (`mcp.md` §9; `data-model.md` §2, §10.8; `xmp-interop.md`):
//! photos carrying other applications' XMP descriptions and EXIF are captioned and tagged; the
//! caption and tags reach the files' XMP and IPTC, the other applications' descriptions stay byte
//! for byte, everything outside the metadata blocks (the pixels) is unchanged, and so is each file's
//! modification time (the default of "Update file modification time on metadata changes").

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use dalica_meta::{Limits, container};
use serde_json::json;

use crate::harness::{HEIGHT, WIDTH, Workspace, eventually, ids};

/// Another application's develop settings, in a description of its own (attribute form).
const DEVELOP: &str = r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" crs:Exposure2012="+0.50" crs:Temperature="5600" xmpMM:DocumentID="xmp.did:7f3a2c91"/>"#;
/// Another application's keyword hierarchy (element form).
const HIERARCHY: &str = r#"<rdf:Description rdf:about="" xmlns:lr="http://ns.adobe.com/lightroom/1.0/"><lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|Portugal|Lisbon</rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description>"#;

const CAPTION: &str = "Tram 28 at sunset";
const TAGS: [&str; 2] = ["lisbon", "tram"];

/// An XMP packet holding only the foreign descriptions, with padding.
fn packet() -> Vec<u8> {
    let mut p = String::from("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    p.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    p.push_str(DEVELOP);
    p.push('\n');
    p.push_str(HIERARCHY);
    p.push_str("\n</rdf:RDF></x:xmpmeta>\n");
    p.push_str(&" ".repeat(1024));
    p.push_str("\n<?xpacket end=\"w\"?>");
    p.into_bytes()
}

/// `jpeg` with an APP1 XMP segment holding [`packet`] after its leading APP0/APP1 segments.
fn with_xmp(jpeg: &[u8]) -> Vec<u8> {
    let mut at = 2;
    while jpeg.get(at) == Some(&0xFF) && matches!(jpeg.get(at + 1), Some(0xE0 | 0xE1)) {
        at += 2 + usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
    }
    let mut body = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    body.extend(packet());
    let mut out = jpeg[..at].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(u16::try_from(body.len() + 2).unwrap().to_be_bytes());
    out.extend(body);
    out.extend(&jpeg[at..]);
    out
}

/// A folder of `n` JPEGs from a camera (EXIF make and model) that another application described.
fn described_folder(ws: &Workspace, n: u64) -> Vec<PathBuf> {
    let folder = ws.path().join("Archive");
    std::fs::create_dir_all(&folder).unwrap();
    let exif = dalica_scenes::exif::Exif { make: Some("Example Optics".into()), model: Some("EX-1".into()), ..Default::default() };
    (0..n)
        .map(|i| {
            let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i, WIDTH, HEIGHT), 90, Some(&exif)).unwrap();
            let path = folder.join(format!("DSC_{:04}.jpg", i + 1));
            std::fs::write(&path, with_xmp(&jpeg)).unwrap();
            path
        })
        .collect()
}

/// The bytes outside a file's rewritable metadata blocks, joined.
fn payload(bytes: &[u8]) -> Vec<u8> {
    let scan = container::scan(bytes, &Limits::default()).unwrap();
    container::payload(bytes, &scan.spans).concat()
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn w08_foreign_metadata() {
    let ws = Workspace::new();
    let files = described_folder(&ws, 3);
    let before: Vec<(Vec<u8>, SystemTime)> = files.iter().map(|p| (std::fs::read(p).unwrap(), modified(p))).collect();
    let limits = Limits::default();
    for (bytes, _) in &before {
        let read = dalica_meta::read(bytes, &limits).unwrap();
        assert!(read.problems.is_empty() && read.owned.caption.is_none(), "the test files read cleanly: {read:?}");
    }

    let mut agent = ws.agent();
    let section = agent.scan(files[0].parent().unwrap());
    let all = agent.section_ids(&section);
    assert_eq!(all.len(), files.len(), "{all:?}");
    let r = agent.tool("set_metadata", json!({ "ids": all, "caption": CAPTION, "tags_add": TAGS }));
    assert_eq!(r["ids"], json!(all), "{r}");
    let found = agent.tool("query_photos", json!({ "query": "tram", "limit": 10 }));
    assert_eq!(ids(&found).len(), files.len(), "{found}");

    for (path, (old, old_time)) in files.iter().zip(&before) {
        // The caption and tags reach the file's XMP and IPTC (written after a debounce).
        let new = eventually(
            &format!("{} carries the caption and tags", path.display()),
            || {
                let bytes = std::fs::read(path).ok()?;
                let read = dalica_meta::read(&bytes, &limits).ok()?;
                let x = &read.sources.xmp;
                let i = &read.sources.iptc;
                let done = x.caption.as_deref() == Some(CAPTION) && x.tags == TAGS && i.caption.as_deref() == Some(CAPTION) && i.keywords == TAGS;
                done.then_some(bytes)
            },
            || format!("{:?}", std::fs::read(path).ok().and_then(|b| dalica_meta::read(&b, &limits).ok()).map(|m| m.sources)),
        );
        let read = dalica_meta::read(&new, &limits).unwrap();
        assert!(read.problems.is_empty(), "{}: {:?}", path.display(), read.problems);

        // The other applications' data: their XMP descriptions byte for byte, the EXIF as it was.
        let scan = container::scan(&new, &limits).unwrap();
        let xmp = String::from_utf8(scan.xmp.as_deref().unwrap().to_vec()).unwrap();
        for foreign in [DEVELOP, HIERARCHY] {
            assert!(xmp.contains(foreign), "{}: a foreign description changed: {xmp}", path.display());
        }
        assert_eq!(read.summary, dalica_meta::read(old, &limits).unwrap().summary, "{}: the camera's EXIF", path.display());

        // Pixels untouched: every byte outside the metadata blocks; the modification time kept.
        assert!(payload(&new) == payload(old), "{}: bytes outside the metadata blocks changed", path.display());
        assert_eq!(modified(path), *old_time, "{}: the modification time changed", path.display());
    }
    agent.quit();
}
