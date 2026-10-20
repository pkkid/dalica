//! The status line's tags field (M6.28, `ui-layout.md` §6.1), driven headlessly on a library of ten
//! scenes photos: several photos (or a section selected as a unit) that carry tags end the line
//! with "Tags: lisbon (8), tram (3)", most used first, ties in alphabetical order, counting each
//! photo of the Photo Tray (held photos and the selection) once; one photo, or photos without tags,
//! have no such field; the field follows tag changes and Undo at once; a line too long for the
//! window is cut at its end with "…". The line matches its fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//! (`tests/fingerprints/region-status-tags-1400.json`; `DALICA_BLESS=1` stores new ones, renders go
//! to `target/test-output/ui/`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_pixels, new_driver, out_dir};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_library::Photo;
use dalica_scenes::fingerprint;
use dalica_ui::status_text::{ELLIPSIS, SEP, several};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<u64>,
    folder: u64,
}

/// Ten photos in one folder, shown at `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 10, 28).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let photos = found["photos"].as_array().unwrap();
    let mut ids: Vec<u64> = photos.iter().map(|p| p["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    let folder = photos[0]["folder"].as_u64().unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, ids, folder }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn status(t: &T) -> String {
    t.d.state().status_line.clone()
}

/// The status line of photos `ids` without the tags field, from the library as the last frame drew
/// it: their size on disk grows as the tag writes land in their files (in the background, so a
/// line kept from before them goes stale).
fn untagged(t: &T, ids: &[u64]) -> String {
    let lib = t.d.app().session().unwrap().library();
    let photos: Vec<&Photo> = ids.iter().map(|id| lib.photo(PhotoId::new(*id)).unwrap()).collect();
    several(lib, &photos, &[]).text()
}

fn select(t: &mut T, ids: &[u64]) {
    run(t, "select.set", json!({ "ids": ids }));
}

#[test]
fn the_status_line_lists_the_tags_of_a_tagged_selection() {
    let mut t = setup(1400, 900);
    let ids = t.ids.clone();
    select(&mut t, &ids[0..8]);
    let base = status(&t);
    assert!(base.starts_with("8 pictures") && !base.contains("Tags:"), "no tags, no field: {base}");
    assert_eq!(base, untagged(&t, &ids[0..8]));

    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..3], "tags": "tram" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[7..10], "tags": "harbour" }));
    assert_eq!(
        status(&t),
        format!("{}{SEP}Tags: lisbon (8), tram (3), harbour (1)", untagged(&t, &ids[0..8])),
        "most used first; only the selection counts"
    );
    // Ties in alphabetical order, ignoring case.
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[4..7], "tags": "Alfama" }));
    assert_eq!(status(&t), format!("{}{SEP}Tags: lisbon (8), Alfama (3), tram (3), harbour (1)", untagged(&t, &ids[0..8])));
    // Undo takes the field back with the tags.
    t.d.key("Ctrl+Z").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(status(&t), format!("{}{SEP}Tags: lisbon (8), tram (3), harbour (1)", untagged(&t, &ids[0..8])));

    // One photo: its own fields, no tags field.
    select(&mut t, &ids[0..1]);
    assert!(!status(&t).contains("Tags:"), "{}", status(&t));
    // Held photos count with a selection made elsewhere, each photo once.
    run(&mut t, "tray.hold", json!({ "ids": &ids[8..10] }));
    select(&mut t, &ids[0..1]);
    assert!(status(&t).starts_with("3 pictures"), "{}", status(&t));
    assert!(status(&t).ends_with("Tags: harbour (2), lisbon (1), tram (1)"), "{}", status(&t));
    run(&mut t, "tray.clear", json!({ "confirm": true }));

    // A section selected as a unit: all ten.
    let section = format!("folder:{}", t.folder);
    run(&mut t, "select.section", json!({ "section": section }));
    assert_eq!(t.d.state().selection.ids.len(), 10);
    assert!(status(&t).starts_with("10 pictures"), "{}", status(&t));
    assert!(status(&t).ends_with("Tags: lisbon (8), harbour (3), tram (3)"), "{}", status(&t));
    // Untagged photos only: no field.
    run(&mut t, "photo.tags.remove", json!({ "ids": &ids, "tags": "lisbon, tram, harbour" }));
    select(&mut t, &ids[0..8]);
    assert_eq!(status(&t), untagged(&t, &ids[0..8]));
}

/// A tag list too long for the window is cut at its end with "…"; the line fits in the large
/// window and the small one.
#[test]
fn a_long_tag_list_is_cut_at_its_end() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ids = t.ids.clone();
    let many: Vec<String> = (0..40).map(|i| format!("tag number {i:02}")).collect();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..4], "tags": many }));
    select(&mut t, &ids[0..4]);
    for (w, h) in [LARGE, SMALL] {
        t.d.resize(w, h);
        t.d.settle(FRAMES).unwrap();
        let line = status(&t);
        assert!(line.contains("Tags: tag number 00 (4), tag number 01 (4)"), "{w}: {line}");
        assert!(line.ends_with(ELLIPSIS), "{w}: cut at its end: {line}");
        assert!(line.starts_with("4 pictures"), "{w}: the start stays: {line}");
    }
}

/// Checks the status line's region in the last frame against `tests/fingerprints/<name>.json`,
/// and writes it and the whole window to the test output.
fn check_status(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let r = d.widgets().into_iter().find(|w| w.id == "region:statusLine").expect("the status line").rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    check_pixels(name, w, h, &rgba);
}

#[test]
fn the_tagged_status_line_matches_its_fingerprint() {
    let (w, h) = LARGE;
    let mut t = setup(w, h);
    let ids = t.ids.clone();
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[2..8], "tags": "tram" }));
    select(&mut t, &ids[0..8]);
    assert!(status(&t).ends_with("Tags: lisbon (8), tram (6)"), "{w}: {}", status(&t));
    check_status(&t.d, &format!("region-status-tags-{w}"));
}
