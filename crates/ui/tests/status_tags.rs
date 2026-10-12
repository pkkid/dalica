//! The status line's tags field (M6.28, `ui-layout.md` §6.1), driven headlessly on a library of ten
//! scenes photos: several photos (or a section selected as a unit) that carry tags end the line
//! with "Tags: lisbon (8), tram (3)", most used first, ties in alphabetical order, counting each
//! photo of the Photo Tray (held photos and the selection) once; one photo, or photos without tags,
//! have no such field; the field follows tag changes and Undo at once; a line too long for the
//! window is cut at its end with "…". The line matches its fingerprints at 1400 × 900 and 900 × 600
//! (`tests/fingerprints/region-status-tags-<w>.json`; `DALICA_BLESS=1` stores new ones, renders go
//! to `target/test-output/ui/`).

use std::path::PathBuf;
use std::sync::Arc;

use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::status_text::{ELLIPSIS, SEP};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<u64>,
    folder: u64,
}

/// Ten photos in one folder, shown at `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, 10, 28).unwrap();
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
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
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

    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..3], "tags": "tram" }));
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[7..10], "tags": "harbour" }));
    assert_eq!(status(&t), format!("{base}{SEP}Tags: lisbon (8), tram (3), harbour (1)"), "most used first; only the selection counts");
    // Ties in alphabetical order, ignoring case.
    run(&mut t, "photo.tags.add", json!({ "ids": &ids[4..7], "tags": "Alfama" }));
    assert_eq!(status(&t), format!("{base}{SEP}Tags: lisbon (8), Alfama (3), tram (3), harbour (1)"));
    // Undo takes the field back with the tags.
    t.d.key("Ctrl+Z").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(status(&t), format!("{base}{SEP}Tags: lisbon (8), tram (3), harbour (1)"));

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
    assert_eq!(status(&t), base);
}

/// A tag list too long for the window is cut at its end with "…"; the line fits at both sizes.
#[test]
fn a_long_tag_list_is_cut_at_its_end() {
    for (w, h) in SIZES {
        let mut t = setup(w, h);
        let ids = t.ids.clone();
        let many: Vec<String> = (0..40).map(|i| format!("tag number {i:02}")).collect();
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..4], "tags": many }));
        select(&mut t, &ids[0..4]);
        let line = status(&t);
        assert!(line.contains("Tags: tag number 00 (4), tag number 01 (4)"), "{w}: {line}");
        assert!(line.ends_with(ELLIPSIS), "{w}: cut at its end: {line}");
        assert!(line.starts_with("4 pictures"), "{w}: the start stays: {line}");
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

#[test]
fn the_tagged_status_line_matches_its_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let mut t = setup(w, h);
        let ids = t.ids.clone();
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[0..8], "tags": "lisbon" }));
        run(&mut t, "photo.tags.add", json!({ "ids": &ids[2..8], "tags": "tram" }));
        select(&mut t, &ids[0..8]);
        assert!(status(&t).ends_with("Tags: lisbon (8), tram (6)"), "{w}: {}", status(&t));
        check_status(&t.d, &format!("region-status-tags-{w}"));
    }
}
