//! The Properties pane (M6.27, `ui-layout.md` §7), driven headlessly on a folder holding a JPEG
//! whose EXIF, XMP and IPTC each say something (`tagged.jpg`), a PNG without metadata and a plain
//! JPEG:
//!
//! - every row of the tagged JPEG with its value, after a first frame that shows what the library
//!   knows and "Reading the file…"; no frame touched the file (the `fs` guard, with this thread
//!   marked as the UI thread) and the perf counters saw every frame of it;
//! - Show All Metadata lists the file's other fields grouped by EXIF, GPS, IPTC and XMP; Show Fewer
//!   takes them away;
//! - the pane follows the selection: another photo, two photos, the folder as a unit, nothing;
//! - a long path is cut with "…" and shown whole in a tooltip; the list scrolls;
//! - the "Changed in another application; Dalica's value kept" note;
//! - a photo turned by its recipe shows its displayed size in the Dimensions row (noting the
//!   file's), the status line, the Resolution caption and the cell's tooltip;
//! - the pane matches its fingerprints at 1400 × 900 and 900 × 600, with and without Show All
//!   (`tests/fingerprints/pane-properties-*.json`; the Path row's value is masked, as its folder
//!   differs per machine; `DALICA_BLESS=1` stores new ones, renders go to `target/test-output/ui/`),
//!   and every widget shown has a valid, unique id.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use crate::common::{FRAMES, new_driver, out_dir, shot};
use dalica_core::{DateTime, PhotoId};
use dalica_engine::settings::MetadataWrites;
use dalica_engine::{Field, Fields, Session};
use dalica_scenes::fingerprint;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::properties::{self, Shown, answer};
use dalica_ui::state::Pane;
use dalica_ui::{App, Driver, status_text};
use egui::{Modifiers, PointerButton, vec2};
use serde_json::{Value, json};

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const WAIT: Duration = Duration::from_secs(60);
/// The files' modification time: 2026-05-28 20:26:40 UTC.
const MTIME: u64 = 1_780_000_000;
/// A folder name long enough that the Path row is always cut.
const OUTER: &str = "Holiday photos from a long weekend away";

// A little-endian TIFF structure (TIFF 6.0 / EXIF 2.32): IFD0 with the EXIF and GPS IFDs after it
// (the same file as the engine's `photo_metadata.rs`).

enum V {
    Ascii(&'static str),
    Short(u16),
    Long(u32),
    Rational(&'static [(u32, u32)]),
}

fn ifd(entries: &mut [(u16, V)], at: usize) -> Vec<u8> {
    entries.sort_by_key(|(tag, _)| *tag);
    let end = at + 2 + 12 * entries.len() + 4;
    let (mut table, mut data) = ((entries.len() as u16).to_le_bytes().to_vec(), Vec::new());
    for (tag, v) in entries.iter() {
        let (kind, count, bytes): (u16, usize, Vec<u8>) = match v {
            V::Ascii(s) => (2, s.len() + 1, [s.as_bytes(), &[0]].concat()),
            V::Short(n) => (3, 1, n.to_le_bytes().to_vec()),
            V::Long(n) => (4, 1, n.to_le_bytes().to_vec()),
            V::Rational(r) => (5, r.len(), r.iter().flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat()).collect()),
        };
        table.extend(tag.to_le_bytes());
        table.extend(kind.to_le_bytes());
        table.extend((count as u32).to_le_bytes());
        if bytes.len() <= 4 {
            table.extend(&bytes);
            table.extend(vec![0; 4 - bytes.len()]);
        } else {
            table.extend(((end + data.len()) as u32).to_le_bytes());
            data.extend(&bytes);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    table.extend(0u32.to_le_bytes());
    [table, data].concat()
}

fn tiff() -> Vec<u8> {
    let mut exif = vec![
        (0x829a, V::Rational(&[(1, 250)])),
        (0x829d, V::Rational(&[(28, 10)])),
        (0x8827, V::Short(400)),
        (0x9003, V::Ascii("2026:06:04 19:42:10")),
        (0x9004, V::Ascii("2026:06:04 19:42:11")),
        (0x9209, V::Short(0x10)),
        (0x920a, V::Rational(&[(35, 1)])),
        (0xa001, V::Short(1)),
        (0xa405, V::Short(52)),
        (0xa434, V::Ascii("EF35mm f/2")),
    ];
    let mut gps = vec![
        (0x0001, V::Ascii("N")),
        (0x0002, V::Rational(&[(38, 1), (42, 1), (36, 1)])),
        (0x0003, V::Ascii("W")),
        (0x0004, V::Rational(&[(9, 1), (8, 1), (24, 1)])),
    ];
    let ifd0 = |exif_at: u32, gps_at: u32| {
        vec![
            (0x010f, V::Ascii("Canon")),
            (0x0110, V::Ascii("Canon EOS 5D")),
            (0x0112, V::Short(1)),
            (0x8769, V::Long(exif_at)),
            (0x8825, V::Long(gps_at)),
        ]
    };
    let len0 = ifd(&mut ifd0(0, 0), 8).len();
    let exif_at = 8 + len0;
    let exif_ifd = ifd(&mut exif, exif_at);
    let gps_at = exif_at + exif_ifd.len();
    let gps_ifd = ifd(&mut gps, gps_at);
    let head = [&b"II"[..], &42u16.to_le_bytes(), &8u32.to_le_bytes()].concat();
    [head, ifd(&mut ifd0(exif_at as u32, gps_at as u32), 8), exif_ifd, gps_ifd].concat()
}

/// Photoshop image resources holding one IPTC-IIM stream (resource 0x0404).
fn iptc() -> Vec<u8> {
    let mut iim = Vec::new();
    for (record, number, data) in [(1u8, 90u8, &b"\x1b%G"[..]), (2, 25, b"lisbon"), (2, 25, b"tram"), (2, 120, b"An IPTC caption")] {
        iim.extend([0x1c, record, number]);
        iim.extend((data.len() as u16).to_be_bytes());
        iim.extend(data);
    }
    let mut out = [&b"Photoshop 3.0\0"[..], b"8BIM", &0x0404u16.to_be_bytes(), &[0, 0], &(iim.len() as u32).to_be_bytes()].concat();
    out.extend(&iim);
    if iim.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn xmp() -> Vec<u8> {
    let body = r#"<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><dc:description><rdf:Alt><rdf:li xml:lang="x-default">Elétrico 28</rdf:li></rdf:Alt></dc:description><xmp:Rating>3</xmp:Rating></rdf:Description>"#;
    let packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{body}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    [&b"http://ns.adobe.com/xap/1.0/\0"[..], packet.as_bytes()].concat()
}

/// A `scenes` JPEG with APP1 EXIF, APP1 XMP and APP13 IPTC right after its SOI.
fn tagged_jpeg() -> Vec<u8> {
    let plain = dalica_scenes::jpeg(&dalica_scenes::Scene::new(7, 120, 80), 80, None).unwrap();
    let mut out = plain[..2].to_vec();
    for (marker, payload) in [(0xe1, [&b"Exif\0\0"[..], &tiff()].concat()), (0xe1, xmp()), (0xed, iptc())] {
        out.extend([0xff, marker]);
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
    }
    out.extend(&plain[2..]);
    out
}

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    folder: PathBuf,
    tagged: PhotoId,
    plain: PhotoId,
    other: PhotoId,
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(MTIME)).unwrap();
}

/// `<OUTER>/Lisbon` holding `tagged.jpg` (EXIF, XMP, IPTC), `plain.png` (none) and `other.jpg`,
/// shown at `w × h` with Dalica's caption and tags on the tagged photo.
///
/// Captions and tags are not written into the files (Options → General → Files): the pane is
/// checked against the files as written here. With the write-back on, it would rewrite
/// `tagged.jpg` once its debounce ran out, on the clock, at some frame of the test: on a loaded
/// machine after the pane had read the file, so the pane read it again in the middle of a check
/// (the listing vanished under the scroll, the IPTC caption and the XMP fields changed).
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(OUTER).join("Lisbon");
    std::fs::create_dir_all(&folder).unwrap();
    write(&folder.join("tagged.jpg"), &tagged_jpeg());
    write(&folder.join("plain.png"), &dalica_scenes::png(&dalica_scenes::Scene::new(3, 64, 48)).unwrap());
    write(&folder.join("other.jpg"), &dalica_scenes::jpeg(&dalica_scenes::Scene::new(5, 90, 60), 80, None).unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.change_settings(|st| st.set_metadata_writes(&MetadataWrites { captions_tags_places: false, ..MetadataWrites::default() }));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = |s: &Session, name: &str| s.library().photos().find(|p| p.name == name).unwrap().id;
    let (tagged, plain, other) = (id(&s, "tagged.jpg"), id(&s, "plain.png"), id(&s, "other.jpg"));
    s.execute("photo.caption.set", &json!({ "ids": [tagged], "caption": "Tram 28 at sunset" })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [tagged], "tags": "lisbon, tram" })).unwrap();
    assert_eq!(s.pending_metadata_writes(), 0, "nothing waits to be written into the files");
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, folder, tagged, plain, other }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn shown(t: &T) -> Shown {
    properties::shown(t.d.ctx()).expect("the pane was drawn")
}

/// Lets frames run until the pane no longer waits for the file, waiting for the engine's readers
/// between frames.
fn ready(t: &mut T) -> Shown {
    for _ in 0..100 {
        let s = shown(t);
        if !s.loading {
            return s;
        }
        assert!(t.d.app().session().unwrap().wait_photo_metadata(WAIT));
        t.d.step();
    }
    panic!("the pane still reads the file: {:?}", shown(t));
}

fn select(t: &mut T, ids: &[PhotoId]) {
    let ids: Vec<u64> = ids.iter().map(|i| i.get()).collect();
    run(t, "select.set", json!({ "ids": ids }));
}

fn rows(s: &Shown) -> Vec<(String, String)> {
    s.rows.iter().map(|r| (r.label.clone(), r.value.clone())).collect()
}

fn keys(s: &Shown) -> Vec<&str> {
    s.rows.iter().map(|r| r.key.as_str()).collect()
}

fn row<'a>(s: &'a Shown, key: &str) -> &'a properties::ShownRow {
    s.rows.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no {key} row: {s:?}"))
}

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

fn when(secs: u64) -> String {
    answer::date_time(DateTime::from_unix_secs(secs as i64).unwrap())
}

#[test]
fn every_row_of_a_jpeg_with_exif_xmp_and_iptc_and_no_frame_reads_the_file() {
    let mut t = setup(1400, 900);
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    t.d.perf_mut().reset();
    let frames_before = t.d.frames();

    // The first frame: what the library knows, and the loading row.
    // (Between frames, so the next frame is the first to draw the pane on this photo.)
    let ids = json!({ "ids": [t.tagged.get()] });
    t.d.app_mut().run("select.set", &ids).unwrap();
    t.d.app_mut().run("pane.properties", &json!({})).unwrap();
    t.d.step();
    let first = shown(&t);
    assert_eq!(first.heading, "Properties of tagged.jpg");
    assert!(first.loading, "the file is read on a worker: {first:?}");
    assert_eq!(keys(&first), ["path", "fileSize", "dimensions", "dateTaken", "fileModified", "caption", "tags"]);

    // Then every row, in the pane's order, as it is written.
    let s = ready(&mut t);
    let size = std::fs::metadata(t.folder.join("tagged.jpg")).unwrap().len();
    let expected: Vec<(String, String)> = [
        ("Path", t.folder.join("tagged.jpg").display().to_string()),
        ("File size", status_text::size(size)),
        ("Dimensions", "120 × 80".into()),
        ("Date taken", "June 4, 2026 7:42:10 PM".into()),
        ("Date digitized", "June 4, 2026 7:42:11 PM".into()),
        ("File modified", when(MTIME)),
        ("Camera", "Canon EOS 5D".into()),
        ("Lens", "EF35mm f/2".into()),
        ("Exposure", "1/250 s".into()),
        ("Aperture", "f/2.8".into()),
        ("ISO", "400".into()),
        ("Focal length", "35 mm (52 mm in 35 mm film)".into()),
        ("Flash", "Did not fire".into()),
        ("Orientation", "Normal".into()),
        ("Color space", "sRGB".into()),
        ("GPS", "38.710000° N, 9.140000° W".into()),
        ("Caption", "Tram 28 at sunset".into()),
        ("Tags", "lisbon, tram".into()),
        ("Rating", "3".into()),
    ]
    .into_iter()
    .map(|(l, v)| (l.to_string(), v))
    .collect();
    assert_eq!(rows(&s), expected);
    assert!(s.rows.iter().all(|r| r.note.is_none()), "{s:?}");
    assert_eq!(when(MTIME), "May 28, 2026 8:26:40 PM");
    assert!(s.groups.is_empty() && s.message.is_none() && s.error.is_none() && s.note.is_none());

    // Show All Metadata: the file's other fields, grouped.
    assert!(t.d.widgets().iter().any(|w| w.id == properties::SHOW_ALL_ID && w.label == "Show All Metadata"));
    t.d.click(properties::SHOW_ALL_ID).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().properties_show_all);
    let all = ready(&mut t);
    assert_eq!(rows(&all), expected, "the rows stay above the listing");
    let names: Vec<&str> = all.groups.iter().map(|(g, _)| g.as_str()).collect();
    for group in ["EXIF", "GPS", "IPTC", "XMP"] {
        assert!(names.contains(&group), "{group} in {names:?}");
    }
    let field = |group: &str, needle: &str| all.groups.iter().filter(|(g, _)| g == group).flat_map(|(_, f)| f).any(|f| f.value.contains(needle));
    assert!(field("EXIF", "Canon EOS 5D") && field("EXIF", "EF35mm f/2") && field("GPS", "38"), "{all:?}");
    assert!(field("IPTC", "An IPTC caption") && field("IPTC", "tram") && field("XMP", "Elétrico 28"), "{all:?}");
    assert!(all.message.is_none());
    assert!(t.d.widgets().iter().any(|w| w.id == properties::SHOW_ALL_ID && w.label == "Show Fewer" && w.active));
    t.d.click(properties::SHOW_ALL_ID).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(!t.d.state().properties_show_all);
    assert!(shown(&t).groups.is_empty(), "Show Fewer takes them away");

    // No frame touched the file; the perf counters saw every frame.
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "a frame made an fs call");
    let report = t.d.perf().report();
    let frames = t.d.frames() - frames_before;
    assert!(report.frames as u64 >= frames.min(dalica_ui::perf::WINDOW as u64), "{report:?}");
    eprintln!("properties: {frames} frames, max {:.2} ms, mean {:.2} ms, p95 {:.2} ms", report.max_frame_ms, report.frame_ms, report.frame_p95_ms);
}

/// `settle()` waits for the pane's read (`Driver::busy` counts the engine's pending metadata
/// reads), so a settled pane shows the photo's full rows, not "Reading the file…". `big.jpg` is
/// the tagged JPEG with 48 MB after its end, so its read is still under way right after it is
/// asked for.
#[test]
fn settle_waits_until_the_pane_has_the_files_rows() {
    let mut t = setup(1400, 900);
    write(&t.folder.join("big.jpg"), &[tagged_jpeg(), vec![0; 48 << 20]].concat());
    let session = t.d.app_mut().session_mut().unwrap();
    let _ = session.start_verification();
    session.wait_idle();
    let big = session.library().photos().find(|p| p.name == "big.jpg").expect("the rescan found big.jpg").id;
    run(&mut t, "pane.properties", json!({}));
    for id in [big, t.plain] {
        let ids = json!({ "ids": [id.get()] });
        t.d.app_mut().run("select.set", &ids).unwrap();
        // Ask as the pane's next frame will, and look at once: the read is queued or under way.
        let session = t.d.app_mut().session_mut().unwrap();
        let first = session.execute("photo.metadata", &json!({ "id": id.get() })).unwrap();
        assert_eq!(first["pending"], json!(true));
        if id == big {
            // (A tiny file may be read before `busy` looks; 48 MB is not.)
            assert!(session.photo_metadata_pending() > 0, "48 MB are still being read");
            let busy = t.d.busy(false);
            assert!(busy.iter().any(|b| b.starts_with("photo metadata: ")), "settle would not wait: {busy:?}");
        }
        t.d.settle(FRAMES).unwrap();
        let s = shown(&t);
        assert!(!s.loading && s.error.is_none(), "settled while still reading: {s:?}");
        assert_eq!(t.d.app().session().unwrap().photo_metadata_pending(), 0);
        if id == big {
            assert_eq!(keys(&s).len(), 19, "every row of the file (the file's caption and tags are imported on scan): {s:?}");
            assert_eq!(row(&s, "camera").value, "Canon EOS 5D");
        } else {
            assert_eq!(keys(&s), ["path", "fileSize", "dimensions", "dateTaken", "fileModified"]);
        }
    }
}

#[test]
fn the_pane_follows_the_selection() {
    let mut t = setup(1400, 900);
    let ids = [t.tagged];
    select(&mut t, &ids);
    run(&mut t, "pane.properties", json!({}));
    assert_eq!(t.d.state().pane, Some(Pane::Properties));
    assert_eq!(ready(&mut t).heading, "Properties of tagged.jpg");

    // Another photo: its own rows; a file with no date of its own says so.
    let ids = [t.plain];
    select(&mut t, &ids);
    let s = ready(&mut t);
    assert_eq!(s.heading, "Properties of plain.png");
    assert_eq!(keys(&s), ["path", "fileSize", "dimensions", "dateTaken", "fileModified"]);
    assert_eq!(row(&s, "dimensions").value, "64 × 48");
    assert_eq!((row(&s, "dateTaken").value.as_str(), row(&s, "dateTaken").note.as_deref()), (when(MTIME).as_str(), Some("date from file")));

    // Two photos: what the library knows of them.
    let ids = [t.tagged, t.other];
    select(&mut t, &ids);
    let s = shown(&t);
    assert_eq!(s.heading, "Properties of 2 photos");
    assert!(!s.loading);
    let bytes: u64 = ["tagged.jpg", "other.jpg"].iter().map(|n| std::fs::metadata(t.folder.join(n)).unwrap().len()).sum();
    let expected: Vec<(String, String)> = [
        ("Pictures", "2".to_string()),
        ("Dates", "May 28, 2026 to June 4, 2026".into()),
        ("Size on disk", status_text::size(bytes)),
        ("Tags", "lisbon (1), tram (1)".into()),
    ]
    .into_iter()
    .map(|(l, v)| (l.to_string(), v))
    .collect();
    assert_eq!(rows(&s), expected);
    t.d.click(properties::SHOW_ALL_ID).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(shown(&t).message.as_deref(), Some(properties::ONE_PHOTO));
    t.d.click(properties::SHOW_ALL_ID).unwrap();
    t.d.settle(FRAMES).unwrap();

    // The folder as a unit: its name, path and place.
    let folder = t.d.app().session().unwrap().library().photo(t.tagged).unwrap().folder;
    run(&mut t, "folder.info.set", json!({ "id": folder, "place": "Lisbon, Portugal" }));
    run(&mut t, "select.section", json!({ "section": format!("folder:{folder}") }));
    let s = shown(&t);
    assert_eq!(s.heading, "Lisbon");
    assert_eq!(keys(&s), ["path", "place", "pictures", "dates", "size", "tags"]);
    assert_eq!(row(&s, "path").value, t.folder.display().to_string());
    assert_eq!((row(&s, "place").value.as_str(), row(&s, "pictures").value.as_str()), ("Lisbon, Portugal", "3"));
    shot(&t.d, "pane-properties-folder-1400x900");

    // Nothing: no rows and no switch.
    run(&mut t, "select.none", json!({}));
    let s = shown(&t);
    assert_eq!(s.heading, "No selection");
    assert!(s.rows.is_empty() && !has(&t, properties::SHOW_ALL_ID));

    // Back to the first photo: its answer was kept by the engine, no loading row.
    let ids = [t.tagged];
    select(&mut t, &ids);
    let s = shown(&t);
    assert!(!s.loading && keys(&s).contains(&"camera"), "{s:?}");
}

#[test]
fn a_long_path_is_cut_and_shown_whole_in_a_tooltip_and_the_rows_scroll() {
    let mut t = setup(900, 600);
    let ids = [t.tagged];
    select(&mut t, &ids);
    run(&mut t, "pane.properties", json!({}));
    let s = ready(&mut t);
    let path = row(&s, "path");
    let (rect, cut) = path.painted.expect("the Path row is in view");
    assert!(cut, "a path this long is cut");
    assert_eq!(row(&s, "camera").painted.map(|p| p.1), Some(false), "a short value is whole");

    // The tooltip shows it whole.
    let events = [PointerEvent { kind: PointerKind::Move, pos: rect.center() }];
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.wait(1500).unwrap();
    assert_eq!(t.d.app().tooltip(), Some(t.folder.join("tagged.jpg").display().to_string().as_str()));

    // The rows below the pane's bottom are not laid out until the wheel brings them in.
    let last = s.rows.last().unwrap();
    assert_eq!(last.key, "rating");
    assert!(last.painted.is_none(), "below the fold at 900 × 600: {last:?}");
    t.d.scroll(Some("pane:properties"), vec2(0.0, -2000.0)).unwrap();
    t.d.settle(FRAMES).unwrap();
    let s = shown(&t);
    assert!(row(&s, "rating").painted.is_some() && row(&s, "path").painted.is_none(), "scrolled to the end: {s:?}");
}

#[test]
fn the_changed_elsewhere_note_and_a_turned_photo() {
    let mut t = setup(1400, 900);
    let ids = [t.tagged];
    select(&mut t, &ids);
    run(&mut t, "pane.properties", json!({}));
    ready(&mut t);
    let (tagged, plain) = (t.tagged, t.plain);
    let session = t.d.app_mut().session_mut().unwrap();
    session.note_changed_elsewhere(tagged, Fields::of(&[Field::Caption, Field::Place]));
    session.note_changed_elsewhere(plain, Fields::of(&[Field::Tags]));
    // The note is no library change: the pane asks again within a second.
    t.d.wait(1100).unwrap();
    let s = shown(&t);
    for key in ["caption", "gps"] {
        assert_eq!(row(&s, key).note.as_deref(), Some(answer::CHANGED_ELSEWHERE), "{key}");
    }
    assert_eq!(row(&s, "tags").note, None);
    select(&mut t, &[plain]);
    let s = ready(&mut t);
    assert_eq!(s.note.as_deref(), Some(answer::CHANGED_ELSEWHERE), "no Tags row on this photo: the note is the pane's");

    // A quarter turn: the size it shows at, here and in the light table.
    select(&mut t, &[tagged]);
    run(&mut t, "photo.rotate.cw", json!({ "ids": [tagged.get()] }));
    let s = ready(&mut t);
    let dims = row(&s, "dimensions");
    assert_eq!((dims.value.as_str(), dims.note.as_deref()), ("80 × 120", Some("120 × 80 in the file")));
    let status = &t.d.state().status_line;
    assert!(status.contains("80 × 120") && !status.contains("120 × 80"), "{status}");
    run(&mut t, "view.caption", json!({ "caption": "resolution" }));
    let cells = &t.d.app().panels().unwrap().light_table.cells;
    let cell = cells.shown().iter().find(|c| c.photo == tagged).unwrap();
    assert_eq!(cell.caption.as_deref(), Some("80 × 120"));
    shot(&t.d, "pane-properties-turned-1400x900");
    t.d.hover(&format!("thumb:{}", tagged.get()), 1000).unwrap();
    let cells = &t.d.app().panels().unwrap().light_table.cells;
    let (_, tip) = cells.tooltip().expect("the cell's tooltip shows");
    assert!(tip.detail.contains("80 × 120") && !tip.detail.contains("120 × 80"), "{}", tip.detail);
    // Undone: as stored again, with no note.
    run(&mut t, "edit.undo", json!({}));
    let s = ready(&mut t);
    assert_eq!((row(&s, "dimensions").value.as_str(), row(&s, "dimensions").note.as_deref()), ("120 × 80", None));
}

/// Checks the pane's body in the last frame against `tests/fingerprints/<name>.json`, with the
/// Path row's value masked (its folder differs per machine and per OS), and writes it and the
/// whole window to the test output.
fn check_pane(t: &T, name: &str) {
    let shot = t.d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let r = t.d.widgets().into_iter().find(|w| w.id == "pane:properties").expect("the pane").rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mask = row(&shown(t), "path").painted.expect("the Path row in view").0;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let masked = mask.contains(egui::pos2(x as f32 + 0.5, y as f32 + 0.5));
            rgba.extend_from_slice(&if masked { [128, 128, 128, 255] } else { shot.pixel(x, y).unwrap() });
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
fn the_pane_matches_its_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let mut t = setup(w, h);
        let ids = [t.tagged];
        select(&mut t, &ids);
        run(&mut t, "pane.properties", json!({}));
        ready(&mut t);
        check_pane(&t, &format!("pane-properties-{w}x{h}"));
        shot(&t.d, &format!("pane-properties-{w}x{h}"));

        t.d.click(properties::SHOW_ALL_ID).unwrap();
        t.d.settle(FRAMES).unwrap();
        ready(&mut t);
        t.d.scroll(Some("pane:properties"), vec2(0.0, -2000.0)).unwrap();
        t.d.settle(FRAMES).unwrap();
        shot(&t.d, &format!("pane-properties-all-{w}x{h}"));
        assert!(shown(&t).groups.iter().flat_map(|(_, f)| f).any(|f| f.painted.is_some()), "the listing scrolled into view");
        // The Path row is scrolled away here: fingerprint the listing as it stands.
        check_listing(&t, &format!("pane-properties-all-{w}x{h}"));
    }
}

/// [`check_pane`] for a view without the Path row on screen.
fn check_listing(t: &T, name: &str) {
    let shot = t.d.screenshot().unwrap();
    let r = t.d.widgets().into_iter().find(|w| w.id == "pane:properties").expect("the pane").rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    assert!(row(&shown(t), "path").painted.is_none());
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
