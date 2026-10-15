//! `photo.metadata` on a temporary library (`roadmap.md` M6.16; `ui-layout.md` §7,
//! `xmp-interop.md` §2 and §5): a JPEG whose EXIF, XMP and IPTC each say something. The first ask
//! answers `pending` with the library's rows, a worker reads the file, and the next ask gives every
//! Properties row, the file's owned fields from each source, and with `all` every field for Show
//! All Metadata; all of it while the owner thread is marked as the UI thread and made no `fs` call.
//! Also: a file without metadata, a missing file, a new selection while one answer is cached, the
//! "changed in another application" note, and a file changed on disk being read again.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::common::open;
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Field, Fields, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(60);

// A little-endian TIFF structure (TIFF 6.0 / EXIF 2.32): IFD0 with the EXIF and GPS IFDs after it.

enum V {
    Ascii(&'static str),
    Short(u16),
    Long(u32),
    Rational(&'static [(u32, u32)]),
}

/// One IFD at offset `at`: the entries (sorted by tag), no next IFD, then the values that do not
/// fit in four bytes.
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

/// A library watching `Lisbon`, holding `tagged.jpg` (EXIF, XMP, IPTC) and `plain.png` (none).
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, PhotoId, PhotoId) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    dalica_fs::replace(&folder.join("tagged.jpg"), &tagged_jpeg(), Expect::Absent).unwrap();
    let png = dalica_scenes::png(&dalica_scenes::Scene::new(3, 64, 48)).unwrap();
    dalica_fs::replace(&folder.join("plain.png"), &png, Expect::Absent).unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = |s: &Session, name: &str| s.library().photos().find(|p| p.name == name).unwrap().id;
    let (tagged, plain) = (id(&s, "tagged.jpg"), id(&s, "plain.png"));
    (lib, disk, folder, s, tagged, plain)
}

/// The rows as `key → value`, in order.
fn rows(v: &Value) -> Vec<(String, String)> {
    v["rows"].as_array().unwrap().iter().map(|r| (r["key"].as_str().unwrap().to_string(), r["value"].as_str().unwrap().to_string())).collect()
}

fn keys(v: &Value) -> Vec<String> {
    rows(v).into_iter().map(|(k, _)| k).collect()
}

fn value<'a>(rows: &'a [(String, String)], key: &str) -> &'a str {
    rows.iter().find(|(k, _)| k == key).map_or("", |(_, v)| v.as_str())
}

/// Asks until the answer is no longer pending (the readers are waited for between asks).
fn ask_ready(s: &mut Session, params: Value) -> Value {
    let start = Instant::now();
    loop {
        let v = s.execute("photo.metadata", &params).unwrap();
        if v["pending"] == json!(false) {
            return v;
        }
        assert!(start.elapsed() < WAIT, "still pending: {v}");
        assert!(s.wait_photo_metadata(WAIT));
    }
}

#[test]
fn a_jpeg_with_exif_xmp_and_iptc_answers_pending_then_every_row_and_the_owner_thread_reads_no_file() {
    let (_l, _d, folder, mut s, id, _) = setup();
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Tram 28 at sunset" })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [id], "tags": "lisbon, tram" })).unwrap();
    let (model, undo) = (s.library().model().clone(), s.undo_stack().len());
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();

    // The first ask: pending, with what the library knows.
    let first = s.execute("photo.metadata", &json!({ "id": id })).unwrap();
    assert_eq!((&first["id"], &first["name"], &first["pending"]), (&json!(id), &json!("tagged.jpg"), &json!(true)));
    assert_eq!(keys(&first), ["path", "fileSize", "dimensions", "dateTaken", "fileModified", "caption", "tags"]);
    assert!(first.get("file").is_none() && first.get("all").is_none());

    // Then every row, in the pane's order.
    let started = Instant::now();
    let v = ask_ready(&mut s, json!({ "id": id, "all": true }));
    let took = started.elapsed();
    let r = rows(&v);
    assert_eq!(
        keys(&v),
        [
            "path",
            "fileSize",
            "dimensions",
            "dateTaken",
            "dateDigitized",
            "fileModified",
            "camera",
            "lens",
            "exposure",
            "aperture",
            "iso",
            "focalLength",
            "flash",
            "orientation",
            "colorSpace",
            "gps",
            "caption",
            "tags",
            "rating"
        ]
    );
    let photo = s.library().photo(id).unwrap().clone();
    assert_eq!(value(&r, "path"), folder.join("tagged.jpg").display().to_string());
    assert!(value(&r, "fileSize").ends_with(" KB"), "{}", value(&r, "fileSize"));
    assert_eq!(value(&r, "dimensions"), "120 × 80");
    assert_eq!(value(&r, "dateTaken"), "2026-06-04T19:42:10");
    assert_eq!(value(&r, "dateDigitized"), "2026-06-04T19:42:11");
    assert_eq!(value(&r, "fileModified"), dalica_core::DateTime::from_unix_secs(photo.mtime.as_secs()).unwrap().to_string());
    assert_eq!(value(&r, "camera"), "Canon EOS 5D");
    assert_eq!(value(&r, "lens"), "EF35mm f/2");
    assert_eq!(value(&r, "exposure"), "1/250 s");
    assert_eq!(value(&r, "aperture"), "f/2.8");
    assert_eq!(value(&r, "iso"), "400");
    assert_eq!(value(&r, "focalLength"), "35 mm (52 mm in 35 mm film)");
    assert_eq!(value(&r, "flash"), "Did not fire");
    assert_eq!(value(&r, "orientation"), "Normal");
    assert_eq!(value(&r, "colorSpace"), "sRGB");
    assert_eq!(value(&r, "gps"), "38.710000° N, 9.140000° W");
    // Dalica's caption and tags win for display (`xmp-interop.md` §5 rule 2) …
    assert_eq!((value(&r, "caption"), value(&r, "tags")), ("Tram 28 at sunset", "lisbon, tram"));
    assert_eq!(value(&r, "rating"), "3");
    let labels: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert_eq!(&labels[..4], ["Path", "File size", "Dimensions", "Date taken"]);
    assert_eq!(labels.last(), Some(&"Rating"));
    // … and `file` says what the file holds, each field from its source: the caption from XMP
    // (over IPTC's), the tags from IPTC, the rating from XMP, the place and date from EXIF.
    let mut file = v["file"].clone();
    let place = file["place"].take();
    assert_eq!(
        file,
        json!({ "caption": "Elétrico 28", "tags": ["lisbon", "tram"], "rating": 3, "place": null,
                "dateTaken": "2026-06-04T19:42:10", "dateSource": "exif" })
    );
    let (lat, lon) = (place["latitude"].as_f64().unwrap(), place["longitude"].as_f64().unwrap());
    assert!((lat - 38.71).abs() < 1e-9 && (lon + 9.14).abs() < 1e-9, "{place}");
    assert_eq!(v["problems"], json!([]));
    assert!(v.get("changedElsewhere").is_none() && v.get("error").is_none());

    // Show All Metadata: every field, grouped.
    let all = v["all"]["rows"].as_array().unwrap();
    assert_eq!(v["all"]["truncated"], json!(false));
    let has = |group: &str, needle: &str| all.iter().any(|r| r["group"] == json!(group) && r["value"].as_str().unwrap().contains(needle));
    assert!(has("EXIF", "Canon EOS 5D") && has("GPS", "38") && has("IPTC", "An IPTC caption") && has("XMP", "Elétrico 28"), "{all:?}");
    // Without `all` the listing is left out.
    let short = s.execute("photo.metadata", &json!({ "id": id })).unwrap();
    assert!(short.get("all").is_none());
    assert_eq!(short["rows"], v["rows"]);

    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!(s.library().model(), &model, "a query changes nothing");
    assert_eq!(s.undo_stack().len(), undo);
    eprintln!("photo.metadata: pending to every row in {} µs", took.as_micros());
}

#[test]
fn a_file_without_metadata_a_missing_file_and_a_new_selection() {
    let (_l, _d, folder, mut s, tagged, plain) = setup();
    let ui = dalica_fs::mark_ui_thread();
    let v = ask_ready(&mut s, json!({ "id": plain, "all": true }));
    assert_eq!(keys(&v), ["path", "fileSize", "dimensions", "dateTaken", "fileModified"]);
    assert_eq!(v["rows"][3]["note"], json!("date from file"), "no date in the file: the file time, flagged");
    assert_eq!(v["file"]["caption"], Value::Null);
    assert_eq!(v["all"]["rows"], json!([]));

    // A new selection while the tagged photo's answer is cached: both are answered.
    let t = ask_ready(&mut s, json!({ "id": tagged }));
    assert_eq!(rows(&t).len(), 19, "with the caption and tags imported from the file on scan (M6.32)");
    assert_eq!(s.execute("photo.metadata", &json!({ "id": plain })).unwrap()["pending"], json!(false), "still cached");
    drop(ui);

    // A new version of the file (seen by a rescan) is read again; this one is gone by the time
    // the reader comes: the answer says so, with the library's rows still there.
    let old = s.library().photo(tagged).unwrap().clone();
    std::fs::write(folder.join("tagged.jpg"), [tagged_jpeg(), vec![0; 10]].concat()).unwrap();
    s.start_verification();
    s.wait_idle();
    let now = s.library().photo(tagged).unwrap().clone();
    assert_eq!(now.size, old.size + 10, "the rescan saw the new version");
    std::fs::remove_file(folder.join("tagged.jpg")).unwrap();
    let first = s.execute("photo.metadata", &json!({ "id": tagged })).unwrap();
    assert_eq!(first["pending"], json!(true), "a new version of the file is read again");
    let gone = ask_ready(&mut s, json!({ "id": tagged }));
    assert_eq!(gone["error"], json!("the file is missing"));
    assert!(gone.get("file").is_none());
    // The library's rows, with the caption and tags imported from the file on scan (M6.32).
    assert_eq!(keys(&gone), ["path", "fileSize", "dimensions", "dateTaken", "fileModified", "caption", "tags"]);
}

#[test]
fn the_changed_elsewhere_note_lasts_until_the_file_changes() {
    let (_l, _d, folder, mut s, tagged, plain) = setup();
    s.note_changed_elsewhere(tagged, Fields::of(&[Field::Caption, Field::Tags]));
    let v = s.execute("photo.metadata", &json!({ "id": tagged })).unwrap();
    assert_eq!(v["changedElsewhere"], json!({ "fields": ["caption", "tags"], "note": "Changed in another application; Dalica's value kept" }));
    assert!(s.execute("photo.metadata", &json!({ "id": plain })).unwrap().get("changedElsewhere").is_none());
    s.note_changed_elsewhere(tagged, Fields::NONE);
    assert!(s.execute("photo.metadata", &json!({ "id": tagged })).unwrap().get("changedElsewhere").is_none());

    // A new version of the file ends it.
    s.note_changed_elsewhere(tagged, Fields::of(&[Field::Caption]));
    std::fs::write(folder.join("tagged.jpg"), [tagged_jpeg(), vec![0; 4]].concat()).unwrap();
    s.start_verification();
    s.wait_idle();
    assert!(s.execute("photo.metadata", &json!({ "id": tagged })).unwrap().get("changedElsewhere").is_none());
    // Unknown photos are left alone.
    s.note_changed_elsewhere(PhotoId::new(999_999), Fields::ALL);
}

#[test]
fn the_contract_hostile_params_and_no_undo_step() {
    let (_l, _d, _f, mut s, tagged, _) = setup();
    let spec = *s.spec("photo.metadata").unwrap();
    assert!((spec.enabled)(&s, &json!({ "id": tagged })).is_ok());
    assert_eq!(spec.effect, dalica_engine::Effect::Query);
    assert!(matches!(s.execute("photo.metadata", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
    for bad in [
        json!(null),
        json!({}),
        json!([]),
        json!("id"),
        json!({ "id": "1" }),
        json!({ "id": -1 }),
        json!({ "id": 1.5 }),
        json!({ "id": tagged, "all": 1 }),
        json!({ "id": tagged, "zzz": true }),
        json!({ "ids": [tagged] }),
    ] {
        assert!(matches!(s.execute("photo.metadata", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
    ask_ready(&mut s, json!({ "id": tagged, "all": true }));
    assert_eq!(s.undo_stack().len(), 0);
    assert!(s.wait_photo_metadata(WAIT));
}

/// Each session keeps its own answers and notes: a second library in the same process, watching
/// the same folder, reads the file itself (pending first) and does not see the first one's note.
/// An answer arriving counts in `poll()`, and a session closed with reads waiting drops them.
#[test]
fn two_sessions_in_one_process_do_not_share_answers_or_notes() {
    let (_l, _d, folder, mut a, tagged_a, _) = setup();
    let ready_a = ask_ready(&mut a, json!({ "id": tagged_a }));
    a.note_changed_elsewhere(tagged_a, Fields::of(&[Field::Caption]));
    assert!(a.execute("photo.metadata", &json!({ "id": tagged_a })).unwrap().get("changedElsewhere").is_some());

    let lib_b = tempfile::tempdir().unwrap();
    let mut b = open(lib_b.path());
    b.execute("folders.add", &json!({ "path": folder })).unwrap();
    b.wait_idle();
    let tagged_b = b.library().photos().find(|p| p.name == "tagged.jpg").unwrap().id;
    let first = b.execute("photo.metadata", &json!({ "id": tagged_b })).unwrap();
    assert_eq!(first["pending"], json!(true), "the other session's answer is not shared");
    assert!(first.get("changedElsewhere").is_none(), "nor its note");
    assert!(b.wait_photo_metadata(WAIT));
    assert!(b.poll() >= 1, "the answer that arrived counts as work done in poll()");
    let ready_b = b.execute("photo.metadata", &json!({ "id": tagged_b })).unwrap();
    assert_eq!(ready_b["pending"], json!(false));
    assert_eq!((&ready_b["rows"], &ready_b["file"]), (&ready_a["rows"], &ready_a["file"]));
    assert_eq!(b.photo_metadata_pending(), 0);

    // Closing a session with reads waiting neither hangs nor touches the other session.
    let plain_b = b.library().photos().find(|p| p.name == "plain.png").unwrap().id;
    b.execute("photo.metadata", &json!({ "id": plain_b })).unwrap();
    drop(b);
    assert_eq!(a.execute("photo.metadata", &json!({ "id": tagged_a })).unwrap()["pending"], json!(false));
}
