//! W01, Organize and export, the headline workflow (`mcp.md` §9; `process.md` §5): a folder of 200
//! photos scanned, 5 starred, an album made of them, 3 captioned, one cropped and Auto Fixed, the
//! album exported at 1600 px; then 5 JPEGs with the long edge at most 1600, the captions in their
//! XMP and IPTC, the crop visible in the output, and the originals untouched: byte for byte where
//! nothing was written back, every byte outside the metadata blocks where a caption was.
//!
//! 195 of the photos are small, to keep the scan quick; the five the agent stars are 2400 × 1600, so
//! the export really scales them down to the 1600 px long edge.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_meta::{Limits, container};
use serde_json::json;

use crate::harness::{Workspace, bytes, eventually, photo_name};

/// The photos of the folder, and how many of them are large.
const PHOTOS: u64 = 200;
const LARGE: u64 = 5;
const LARGE_W: u32 = 2400;
const LARGE_H: u32 = 1600;
/// The export's long edge.
const SIDE: u64 = 1600;
/// The crop: the left half, so the 3:2 landscape exports as a 3:4 portrait (1200 × 1600).
const CROP: [f64; 4] = [0.0, 0.0, 0.5, 1.0];
const CAPTIONS: [&str; 3] = ["Tram 28 on the hill", "Alfama at dusk", "The river from the castle"];

/// The folder: `PHOTOS - LARGE` small procedural photos, then `LARGE` large ones (`IMG_0196.jpg` …).
fn holiday(ws: &Workspace) -> PathBuf {
    let folder = ws.folder("Lisbon", PHOTOS - LARGE, 0);
    for n in PHOTOS - LARGE + 1..=PHOTOS {
        let scene = dalica_scenes::Scene::new(1000 + n, LARGE_W, LARGE_H);
        std::fs::write(folder.join(photo_name(n)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
    }
    folder
}

/// The bytes outside a file's rewritable metadata blocks, joined (the pixels and the structure).
fn payload(bytes: &[u8]) -> Vec<u8> {
    let scan = container::scan(bytes, &Limits::default()).unwrap();
    container::payload(bytes, &scan.spans).concat()
}

/// The caption a file's XMP and IPTC carry, read back by the independent reader.
fn captions(path: &Path) -> (Option<String>, Option<String>) {
    let read = dalica_meta::read(&bytes(path), &Limits::default()).unwrap();
    assert!(read.problems.is_empty(), "{}: {:?}", path.display(), read.problems);
    (read.sources.xmp.caption, read.sources.iptc.caption)
}

#[test]
fn w01_organize_export() {
    let ws = Workspace::new();
    let folder = holiday(&ws);
    let names: Vec<String> = (1..=PHOTOS).map(photo_name).collect();
    let before: Vec<Vec<u8>> = names.iter().map(|n| bytes(&folder.join(n))).collect();
    let mut agent = ws.agent();

    // Scan the folder: 200 photos.
    let scanned = agent.tool("scan_folder", json!({ "path": folder }));
    assert_eq!((&scanned["photos"], &scanned["skipped"]), (&json!(PHOTOS), &json!(0)), "{scanned}");
    let section = scanned["section"].as_str().unwrap().to_string();
    let listed = agent.tool("query_photos", json!({ "section": section, "limit": 1000 }));
    let by_name: BTreeMap<String, u64> =
        listed["photos"].as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap().to_string(), p["id"].as_u64().unwrap())).collect();
    assert_eq!(by_name.len() as u64, PHOTOS, "{section}: {listed}");

    // Star the five large photos: they, and only they, are starred.
    let large = &names[(PHOTOS - LARGE) as usize..];
    let five: Vec<u64> = large.iter().map(|n| by_name[n]).collect();
    agent.tool("set_metadata", json!({ "ids": five, "star": true }));
    let mut starred = agent.query(json!({ "starred": true }));
    starred.sort_unstable();
    let mut want = five.clone();
    want.sort_unstable();
    assert_eq!(starred, want, "the starred photos");

    // An album of the starred photos.
    let album = agent.tool("create_album", json!({ "name": "Lisbon highlights", "ids": starred }));
    assert_eq!(album["count"], json!(LARGE), "{album}");
    let album_id = album["album"].as_str().unwrap().to_string();
    let mut in_album = agent.section_ids(album["section"].as_str().unwrap());
    in_album.sort_unstable();
    assert_eq!(in_album, want, "the album holds the starred photos: {album}");

    // Caption three of them, each its own.
    for (id, caption) in five.iter().zip(CAPTIONS) {
        agent.tool("set_metadata", json!({ "ids": [id], "caption": caption }));
        assert_eq!(agent.photo(*id)["caption"], json!(caption));
    }

    // Crop and Auto Fix the first.
    let edited = five[0];
    agent.tool("apply_edit", json!({ "ids": [edited], "op": "crop", "params": { "rect": CROP } }));
    agent.tool("apply_edit", json!({ "ids": [edited], "op": "autofix" }));
    let recipe = agent.tool("get_recipe", json!({ "id": edited }));
    let ops: Vec<&str> = recipe["ops"].as_array().unwrap().iter().filter_map(|o| o["op"].as_str()).collect();
    assert_eq!(ops, ["crop", "autofix"], "{recipe}");

    // Export the album at 1600 px.
    let out = ws.path().join("Exports");
    let exported = agent.tool("export", json!({ "album": album_id, "dir": out, "folder_name": "Lisbon", "size": SIDE }));
    assert_eq!((&exported["failed"], &exported["skipped"], &exported["cancelled"]), (&json!([]), &json!([]), &json!(false)), "{exported}");
    let files = exported["files"].as_array().unwrap();
    let mut exported_ids: Vec<u64> = files.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    exported_ids.sort_unstable();
    assert_eq!(exported_ids, want, "one file per photo of the album: {exported}");
    for f in files {
        let (id, path) = (f["id"].as_u64().unwrap(), PathBuf::from(f["path"].as_str().unwrap()));
        assert!(path.starts_with(out.join("Lisbon")), "{}", path.display());
        let file = bytes(&path);
        assert!(file.starts_with(&[0xFF, 0xD8, 0xFF]), "{}: a JPEG", path.display());
        let header = dalica_image::read_header(&file, &dalica_image::Limits::default()).unwrap();
        let (w, h) = (u64::from(header.width), u64::from(header.height));
        assert_eq!((f["width"].as_u64(), f["height"].as_u64()), (Some(w), Some(h)), "{f}");
        assert_eq!(w.max(h), SIDE, "{}: the long edge is the export's {SIDE} px ({w} × {h})", path.display());
        if id == edited {
            // The crop is in the output: the left half of a 3:2 landscape is a 3:4 portrait.
            assert_eq!((w, h), (SIDE * 3 / 4, SIDE), "{}: the crop is visible", path.display());
        } else {
            assert!(w > h && w.abs_diff(h * 3 / 2) <= 1, "{}: uncropped, 3:2 ({w} × {h})", path.display());
        }
        // The caption, in XMP and IPTC alike; none where the photo has none.
        let caption = five.iter().zip(CAPTIONS).find(|(p, _)| **p == id).map(|(_, c)| c.to_string());
        assert_eq!(captions(&path), (caption.clone(), caption), "{}", path.display());
    }

    // The originals: nothing saved, so no backup; the captions are written back into the three
    // captioned files' metadata (after a debounce), never into their pixels.
    let captioned = &large[..CAPTIONS.len()];
    for (name, caption) in captioned.iter().zip(CAPTIONS) {
        let path = folder.join(name);
        eventually(
            &format!("{name} carries its caption"),
            || (captions(&path).0.as_deref() == Some(caption)).then_some(()),
            || format!("{:?}", captions(&path)),
        );
    }
    let info = agent.photo(edited);
    assert_eq!(info["unsavedEdits"], json!(true), "the crop and Auto Fix stay unsaved: {info}");
    agent.quit();

    assert!(!folder.join(dalica_sidecar::originals::DIR_NAME).exists(), "nothing was saved, so no original was kept aside");
    for (name, old) in names.iter().zip(&before) {
        let now = bytes(&folder.join(name));
        if captioned.contains(name) {
            assert!(payload(&now) == payload(old), "{name}: bytes outside the metadata blocks changed");
        } else {
            assert!(now == *old, "{name}: the original changed");
        }
    }
}
