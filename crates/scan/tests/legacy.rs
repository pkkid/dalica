//! The previous organizer's sidecar read on scan (`data-model.md` §9, `roadmap.md` M14.11), on temp
//! folders. Every fixture is written here from the documented format; the file and section names
//! come from `dalica-sidecar`'s constant, never a literal.

use std::path::{Path, PathBuf};

use dalica_core::{Date, Rect01};
use dalica_library::{FolderInfo, Geo};
use dalica_scan::legacy::{FolderSidecar, LegacyFace, LegacyPerson};
use dalica_scan::{FoundFolder, Options, Scan, scan_with};
use dalica_scenes::{Scene, jpeg};
use dalica_sidecar::legacy::{folder_section, sidecar_names};

use crate::common::put;

const ALBUM: &str = "f07f69266e7760526d5b1553c7f451fa";
const ZOE: &str = "0632e71e2ffd6c6d";
const GRANDPA: &str = "00000000000000aa";

fn photos(dir: &Path, names: &[&str]) {
    for (i, n) in names.iter().enumerate() {
        put(&dir.join(n), &jpeg(&Scene::new(i as u64, 32, 24), 80, None).unwrap());
    }
}

/// The current (hidden) legacy sidecar of `dir`.
fn legacy_path(dir: &Path) -> PathBuf {
    dir.join(&sidecar_names()[0])
}

/// Writes `lines` as a legacy sidecar (CRLF, as the legacy organizer writes it).
fn write_legacy(path: &Path, lines: &[String]) {
    put(path, (lines.join("\r\n") + "\r\n").as_bytes());
}

fn scan(root: &Path, options: &Options) -> Scan {
    scan_with(&[root.to_path_buf()], options)
}

fn folder<'a>(scan: &'a Scan, dir: &Path) -> &'a FoundFolder {
    scan.folders.iter().find(|f| f.path == dir).unwrap_or_else(|| panic!("{} listed", dir.display()))
}

/// A legacy-only folder: two photos, a stale section, the folder's section, contacts and an album.
fn garden(dir: &Path) {
    photos(dir, &["a.jpg", "b.jpg"]);
    write_legacy(
        &legacy_path(dir),
        &[
            "[encoding]".into(),
            "utf8=1".into(),
            "[a.jpg]".into(),
            "star=yes".into(),
            "hidden=yes".into(),
            "caption=At the lake; she said \"a=b\"".into(),
            "keywords=beer, empanadas ,beer".into(),
            "rotate=rotate(1)".into(),
            "filters=bw=1;".into(),
            "redo=sepia=1;".into(),
            "faces=rect64(4000400080008000),632e71e2ffd6c6d;rect64(1000100020002000),ffffffffffffffff;rect64(1000100010002000),aa;".into(),
            format!("albums={ALBUM}"),
            "geotag=33.770556,-84.293055".into(),
            "[gone.jpg]".into(),
            "star=yes".into(),
            "caption=../../etc/passwd".into(),
            format!("[{}]", folder_section()),
            "description=Planting the beds".into(),
            "date=42078.5".into(),
            "location=Atlanta, GA".into(),
            "P2category=Hidden Folders".into(),
            "[Contacts2]".into(),
            "632e71e2ffd6c6d=Zoë Ann;;".into(),
            format!("[.album:{ALBUM}]"),
            "name= Garden 2015 ".into(),
            format!("token={ALBUM}"),
            "date=2015-03-15T09:59:59-07:00".into(),
            "location=Lake Lanier".into(),
        ],
    );
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

#[test]
fn a_legacy_only_folder_gives_every_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Garden");
    garden(&dir);
    let scan = scan(tmp.path(), &Options::default());
    let f = folder(&scan, &dir);
    assert_eq!(f.sidecar, FolderSidecar::Legacy);
    assert_eq!(
        f.info,
        FolderInfo {
            description: "Planting the beds".into(),
            date: Some(Date::new(2015, 3, 15).unwrap()),
            place: "Atlanta, GA".into(),
            hidden: true
        }
    );
    let legacy = f.legacy.as_ref().unwrap();
    assert_eq!(legacy.path, legacy_path(&dir));
    let album = &legacy.albums[ALBUM];
    assert_eq!((album.name.as_str(), album.date, album.place.as_str()), ("Garden 2015", Some(Date::new(2015, 3, 15).unwrap()), "Lake Lanier"));
    assert_eq!(legacy.contacts.clone().into_iter().collect::<Vec<_>>(), [(ZOE.to_string(), "Zoë Ann".to_string())], "only the contacts named");

    let a = f.photos.iter().find(|p| p.name == "a.jpg").unwrap();
    assert!(a.star && a.info.hidden);
    assert_eq!(a.info.caption, "At the lake; she said \"a=b\"");
    assert_eq!(a.info.tags, ["beer", "empanadas"]);
    assert_eq!(a.info.edits, ["rotate(90)", "bw()"]);
    assert_eq!(a.info.redo, ["sepia()"]);
    let Some(Geo::At(at)) = a.geo else { panic!("geotag") };
    assert_eq!((at.lat(), at.lon()), (33.770556, -84.293055));
    let la = a.legacy.as_ref().unwrap();
    assert_eq!(la.albums, [ALBUM]);
    let quarter = 0x4000 as f64 / 65535.0;
    let half = 0x8000 as f64 / 65535.0;
    let sixteenth = 0x1000 as f64 / 65535.0;
    let eighth = 0x2000 as f64 / 65535.0;
    assert_eq!(
        la.faces,
        [
            LegacyFace { rect: rect(quarter, quarter, half, half), person: LegacyPerson::Contact(ZOE.into()) },
            LegacyFace { rect: rect(sixteenth, sixteenth, eighth, eighth), person: LegacyPerson::Ignored },
        ],
        "the empty rectangle is left out"
    );

    let b = f.photos.iter().find(|p| p.name == "b.jpg").unwrap();
    assert!(!b.star && b.legacy.is_none(), "no section names it");
}

#[test]
fn with_both_files_only_dalicas_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Garden");
    garden(&dir);
    put(&dir.join(".dalica.json"), br#"{ "dalica": { "version": 1 }, "photos": { "b.jpg": { "star": true } } }"#);
    let scan = scan(tmp.path(), &Options::default());
    let f = folder(&scan, &dir);
    assert_eq!(f.sidecar, FolderSidecar::Both);
    assert!(f.legacy.is_none());
    assert_eq!(f.info, FolderInfo::default());
    let star = |n: &str| f.photos.iter().find(|p| p.name == n).unwrap().star;
    assert!(!star("a.jpg") && star("b.jpg"));
}

#[test]
fn with_the_option_off_nothing_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Garden");
    garden(&dir);
    let scan = scan(tmp.path(), &Options { legacy: false, ..Options::default() });
    let f = folder(&scan, &dir);
    assert_eq!(f.sidecar, FolderSidecar::None);
    assert!(f.legacy.is_none() && f.info == FolderInfo::default());
    assert!(f.photos.iter().all(|p| !p.star && p.legacy.is_none() && p.info.caption.is_empty()));
}

#[test]
fn a_name_differing_in_case_matches_once_and_an_ambiguous_one_not_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Cases");
    photos(&dir, &["One.JPG", "two.jpg"]);
    write_legacy(
        &legacy_path(&dir),
        &["[one.jpg]".into(), "caption=one".into(), "[TWO.JPG]".into(), "caption=TWO".into(), "[Two.Jpg]".into(), "caption=Two".into()],
    );
    let scan = scan(tmp.path(), &Options::default());
    let f = folder(&scan, &dir);
    let caption = |n: &str| f.photos.iter().find(|p| p.name == n).unwrap().info.caption.clone();
    assert_eq!(caption("One.JPG"), "one");
    assert_eq!(caption("two.jpg"), "", "two sections fold to it: neither is used");
}

#[test]
fn a_contact_named_only_in_the_parent_folder_names_the_face() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().join("Family");
    let child = parent.join("Picnic");
    // The parent holds no photo: only its contacts.
    write_legacy(&legacy_path(&parent), &["[Contacts2]".into(), "aa=Grandpa;;".into()]);
    photos(&child, &["a.jpg"]);
    write_legacy(&legacy_path(&child), &["[a.jpg]".into(), "faces=rect64(4000400080008000),aa;".into()]);
    let scan = scan(tmp.path(), &Options::default());
    let f = folder(&scan, &child);
    assert_eq!(f.legacy.as_ref().unwrap().contacts.get(GRANDPA).map(String::as_str), Some("Grandpa"));
    assert_eq!(f.photos[0].legacy.as_ref().unwrap().faces[0].person, LegacyPerson::Contact(GRANDPA.into()));
}

#[cfg(unix)]
#[test]
fn a_legacy_file_through_a_link_is_not_read() {
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tmp.path().join("elsewhere.ini");
    write_legacy(&elsewhere, &["[a.jpg]".into(), "star=yes".into()]);
    let root = tmp.path().join("root");
    let dir = root.join("Linked");
    photos(&dir, &["a.jpg"]);
    std::os::unix::fs::symlink(&elsewhere, legacy_path(&dir)).unwrap();
    let scan = scan(&root, &Options::default());
    let f = folder(&scan, &dir);
    assert!(!f.photos[0].star && f.legacy.is_none());
    assert_eq!(f.sidecar, FolderSidecar::None);
    assert!(scan.warnings.iter().any(|w| w.path == legacy_path(&dir) && w.reason.contains("link")));
}

#[test]
fn every_legacy_file_is_left_as_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Garden");
    garden(&dir);
    let older = tmp.path().join("Older");
    photos(&older, &["a.jpg"]);
    write_legacy(&older.join(&sidecar_names()[1]), &["[a.jpg]".into(), "star=yes".into()]);
    let damaged = tmp.path().join("Damaged");
    photos(&damaged, &["a.jpg"]);
    put(&legacy_path(&damaged), &[b'x'; 2 << 20]);
    let files = [legacy_path(&dir), older.join(&sidecar_names()[1]), legacy_path(&damaged)];
    let before: Vec<_> = files.iter().map(|p| (std::fs::read(p).unwrap(), std::fs::metadata(p).unwrap().modified().unwrap())).collect();
    let scan = scan(tmp.path(), &Options::default());
    assert!(folder(&scan, &older).photos[0].star, "the older name is read too");
    let after: Vec<_> = files.iter().map(|p| (std::fs::read(p).unwrap(), std::fs::metadata(p).unwrap().modified().unwrap())).collect();
    assert!(before == after, "bytes and times unchanged");
    let names = |d: &Path| {
        let mut n: Vec<_> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name()).collect();
        n.sort();
        n
    };
    assert_eq!(names(&damaged).len(), 2, "nothing set aside beside the damaged file");
}

#[test]
fn a_hostile_file_is_skipped_with_a_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let long = tmp.path().join("Long");
    photos(&long, &["a.jpg"]);
    // One line past the reader's 1 MiB line limit.
    put(&legacy_path(&long), &[b"[a.jpg]\nstar=yes\ncaption=".as_slice(), &vec![b'x'; (1 << 20) + 1]].concat());
    let soup = tmp.path().join("Soup");
    photos(&soup, &["a.jpg"]);
    let bytes: Vec<u8> = (0u32..65_536).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    put(&legacy_path(&soup), &bytes);
    let scan = scan(tmp.path(), &Options::default());
    let l = folder(&scan, &long);
    assert!(!l.photos[0].star && l.legacy.is_none() && l.sidecar == FolderSidecar::None);
    assert!(scan.warnings.iter().any(|w| w.path == legacy_path(&long) && w.reason.contains("line")));
    // Random bytes parse to nothing usable, and never panic.
    assert!(!folder(&scan, &soup).photos[0].star);
}
