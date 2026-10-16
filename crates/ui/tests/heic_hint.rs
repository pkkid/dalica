//! HEIC in a build without its decoder (`formats.md` §3): the photo is listed with the size and
//! date read from its container, its cell is the "cannot be shown" tile whose tooltip gives the
//! reason and, under it, what to do about it, and the edit view says the same. The HEIC is
//! written here as a minimal container (`ftyp`, `meta` with `ispe` and an `Exif` item, no HEVC
//! data). Renders go to `target/test-output/ui/` (and `DALICA_SHOTS`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, new_driver, shot, widget};
use dalica_engine::Session;
use dalica_image::formats::heif::{HINT, REASON};
use dalica_scenes::Scene;
use dalica_ui::App;
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::panels::thumbnails::Drawn;
use serde_json::{Value, json};

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

/// A version 0 full box.
fn full(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0, 0, 0, 0][..], payload].concat())
}

/// A HEIC of a 4032 × 3024 primary item (`hvc1`, no coded data) and an `Exif` item holding
/// `tiff`, as an iPhone lays them out: `ftyp`, `meta`, `mdat`.
fn heic(tiff: &[u8]) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    let hdlr = full(b"hdlr", &[&[0u8; 4][..], b"pict", &[0; 13]].concat());
    let pitm = full(b"pitm", &1u16.to_be_bytes());
    let infe = |id: u16, kind: &[u8; 4]| bx(b"infe", &[&[2u8, 0, 0, 0][..], &id.to_be_bytes(), &[0, 0], kind, &[0]].concat());
    let iinf = full(b"iinf", &[2u16.to_be_bytes().to_vec(), infe(1, b"hvc1"), infe(2, b"Exif")].concat());
    let ispe = full(b"ispe", &[4032u32.to_be_bytes(), 3024u32.to_be_bytes()].concat());
    // Item 1: one association, property 1 (`ispe`).
    let ipma = full(b"ipma", &[&1u32.to_be_bytes()[..], &1u16.to_be_bytes(), &[1, 1]].concat());
    let iprp = bx(b"iprp", &[bx(b"ipco", &ispe), ipma].concat());
    let item = [&0u32.to_be_bytes()[..], tiff].concat();
    // Item 2 at `offset`: 4-byte offsets and lengths, no base offset, one extent.
    let iloc = |offset: u32| {
        let entry =
            [&[0x44u8, 0][..], &1u16.to_be_bytes(), &2u16.to_be_bytes(), &[0, 0, 0, 1], &offset.to_be_bytes(), &(item.len() as u32).to_be_bytes()]
                .concat();
        full(b"iloc", &entry)
    };
    let meta = |offset: u32| full(b"meta", &[hdlr.clone(), pitm.clone(), iinf.clone(), iloc(offset), iprp.clone()].concat());
    let start = (ftyp.len() + meta(0).len() + 8) as u32;
    [ftyp, meta(start), bx(b"mdat", &item)].concat()
}

fn ok(control: &Control, conn: &mut Connection, d: &mut dalica_ui::Driver, method: &str, params: Value) -> Value {
    serve(control, conn, d, &Request::new(method, params.clone())).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

#[test]
fn a_heic_is_listed_by_its_date_and_its_tile_says_what_to_do() {
    assert!(!dalica_image::Format::Heif.supported(), "the UI's tests run without the `heif` feature");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Phone");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let jpeg = dalica_scenes::jpeg(&Scene::new(60 + i, 600, 400), 85, None).unwrap();
        std::fs::write(folder.join(format!("IMG_000{}.jpg", i + 1)), jpeg).unwrap();
    }
    let exif = dalica_scenes::Exif { date_time_original: "2026-06-04T19:42:10".parse().ok(), make: Some("Apple".into()), ..Default::default() };
    let tiff = dalica_scenes::exif::tiff(&exif, 4032, 3024, None).unwrap();
    std::fs::write(folder.join("IMG_0004.HEIC"), heic(&tiff)).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let control = Control::new(Config::default());
    let mut conn = Connection::new();
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();

    // Listed with the size and capture date its container gives.
    let found = ok(&control, &mut conn, &mut d, "engine.execute", json!({ "command": "library.query", "params": { "limit": 10 } }));
    let photo = found["photos"].as_array().unwrap().iter().find(|p| p["name"] == "IMG_0004.HEIC").unwrap().clone();
    assert_eq!((&photo["width"], &photo["height"], &photo["date"]), (&json!(4032), &json!(3024), &json!("2026-06-04T19:42:10")), "{photo}");
    let id = photo["id"].as_u64().unwrap();

    // The tile, and its tooltip: the reason, then the hint.
    let cells = &d.app().panels().unwrap().light_table.cells;
    let tile = cells.shown().iter().find(|c| c.photo.get() == id).unwrap();
    assert_eq!(tile.drawn, Drawn::Failed);
    d.hover(&format!("thumb:{id}"), 700).unwrap();
    let (_, tip) = d.app().panels().unwrap().light_table.cells.tooltip().unwrap();
    assert_eq!(tip.detail, format!("Cannot show this photo: {REASON}\n{HINT}"));
    shot(&d, "heic-hint-tooltip-1400x900");

    // The edit view: "This photo cannot be shown.", the reason and the hint.
    ok(&control, &mut conn, &mut d, "engine.execute", json!({ "command": "view.edit", "params": { "id": id } }));
    d.settle(FRAMES).unwrap();
    let label = widget(&d, "canvas:edit").label;
    assert_eq!(label, format!("This photo cannot be shown.\n{REASON}\n{HINT}"));
    shot(&d, "heic-hint-edit-1400x900");
}
