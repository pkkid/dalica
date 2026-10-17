//! Listing an import source (`organizing.md` §3, `security.md` §5.9): temp trees of `scenes`
//! photos read as untrusted trees; times, digests, thumbnails and groups; hostile entries bounded
//! and reported; cancelling between files.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::common::put;
use dalica_core::DateTime;
use dalica_image::Format;
use dalica_scan::FileTypes;
use dalica_scan::import::{Candidate, ListEvent, ListOptions, TimeSource, groups, list_all, list_source};
use dalica_scenes::{Exif, Scene, jpeg, png};

fn at(text: &str) -> DateTime {
    text.parse().unwrap()
}

fn shot(seed: u64, time: &str, thumbnail: Option<u32>) -> Vec<u8> {
    let exif = Exif { date_time_original: Some(at(time)), thumbnail, ..Exif::default() };
    jpeg(&Scene::new(seed, 320, 200), 80, Some(&exif)).unwrap()
}

fn md5_of(path: &Path) -> String {
    format!("md5:{:x}", md5::compute(std::fs::read(path).unwrap()))
}

fn by_name<'a>(list: &'a [Candidate], name: &str) -> &'a Candidate {
    list.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name} not listed: {list:?}"))
}

#[test]
fn a_card_lists_every_picture_with_its_time_digest_and_thumbnail_grouped_at_two_hour_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let dcim = dir.path().join("DCIM");
    put(&dcim.join("100CANON/IMG_0001.JPG"), &shot(1, "2026-06-04T10:00:00", None));
    put(&dcim.join("100CANON/IMG_0002.JPG"), &shot(2, "2026-06-04T11:30:00", Some(160)));
    // Exactly two hours after the one before: still the same group.
    put(&dcim.join("100CANON/IMG_0003.JPG"), &shot(3, "2026-06-04T13:30:00", None));
    put(&dcim.join("101CANON/IMG_0004.JPG"), &shot(4, "2026-06-04T15:30:01", None));
    put(&dcim.join("101CANON/IMG_0005.JPG"), &shot(5, "2026-06-05T09:00:00", None));
    // No date of its own: the file's time.
    let screen = dcim.join("101CANON/screen.png");
    put(&screen, &png(&Scene::new(6, 300, 300)).unwrap());
    let file_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_780_653_600); // 2026-06-05T10:00:00Z
    std::fs::File::options().write(true).open(&screen).unwrap().set_modified(file_time).unwrap();
    put(&dcim.join("MISC/notes.txt"), b"not a picture, and long enough to be sniffed as nothing at all");
    put(&dcim.join(".Trashes/IMG_0009.JPG"), &shot(9, "2026-06-04T10:00:00", None));

    let got = list_all(&dcim, &ListOptions::default());
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let list = &got.candidates;
    let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["IMG_0001.JPG", "IMG_0002.JPG", "IMG_0003.JPG", "IMG_0004.JPG", "IMG_0005.JPG", "screen.png"]);
    assert_eq!(got.listing.progress.files, 7, "the text file is looked at, the dot folder never");
    assert_eq!((got.listing.progress.done, got.listing.progress.listed), (7, 6));
    assert!(!got.listing.cancelled && !got.listing.truncated);
    for (i, c) in list.iter().enumerate() {
        assert_eq!(c.index, i);
        assert!(c.exact_name);
        assert_eq!(c.digest, md5_of(&c.path), "{}", c.name);
        assert_eq!(c.size, std::fs::metadata(&c.path).unwrap().len());
        let t = c.thumbnail.as_ref().unwrap_or_else(|| panic!("{} has no thumbnail: {:?}", c.name, c.problem));
        assert!(t.width.max(t.height) <= 256 && t.width > 0 && t.height > 0, "{} × {}", t.width, t.height);
        assert!(c.problem.is_none());
    }
    let one = by_name(list, "IMG_0001.JPG");
    assert_eq!((one.captured, one.time_source, one.format), (at("2026-06-04T10:00:00"), TimeSource::Metadata, Format::Jpeg));
    assert_eq!((one.width, one.height), (320, 200));
    assert_eq!(one.relative, PathBuf::from("100CANON"));
    // The scaled decode fills the 256 px; the embedded thumbnail is used as it is.
    assert_eq!(one.thumbnail.as_ref().map(|t| (t.width, t.height)), Some((256, 160)));
    let two = by_name(list, "IMG_0002.JPG");
    assert!(two.thumbnail.as_ref().is_some_and(|t| t.width.max(t.height) <= 160), "{:?}", two.thumbnail.as_ref().map(|t| (t.width, t.height)));
    let screen = by_name(list, "screen.png");
    assert_eq!((screen.captured, screen.time_source), (at("2026-06-05T10:00:00"), TimeSource::File));

    let members: Vec<Vec<&str>> = got.listing.groups.iter().map(|g| g.members.iter().map(|&i| list[i].name.as_str()).collect()).collect();
    assert_eq!(members, [vec!["IMG_0001.JPG", "IMG_0002.JPG", "IMG_0003.JPG"], vec!["IMG_0004.JPG"], vec!["IMG_0005.JPG", "screen.png"]]);
    assert_eq!((got.listing.groups[0].start, got.listing.groups[0].end), (at("2026-06-04T10:00:00"), at("2026-06-04T13:30:00")));

    // File Types: a type turned off is not listed.
    let jpeg_only = list_all(&dcim, &ListOptions { types: FileTypes::jpeg_only(), ..ListOptions::default() });
    assert_eq!(jpeg_only.candidates.len(), 5);
}

#[test]
fn groups_sort_by_time_and_split_past_two_hours() {
    let times = [at("2026-06-04T12:00:00"), at("2026-06-04T09:00:00"), at("2026-06-04T10:59:59"), at("2026-06-04T09:00:00")];
    let g = groups(&times);
    assert_eq!(g.iter().map(|g| g.members.clone()).collect::<Vec<_>>(), [vec![1, 3, 2, 0]]);
    let g = groups(&[at("2026-06-04T09:00:00"), at("2026-06-04T11:00:01")]);
    assert_eq!(g.len(), 2);
    assert!(groups(&[]).is_empty());
}

/// A JPEG whose frame header claims 60 000 × 60 000 pixels.
fn lying_jpeg() -> Vec<u8> {
    let mut b = jpeg(&Scene::new(7, 64, 48), 80, None).unwrap();
    let mut at = 2;
    while at + 9 < b.len() {
        assert_eq!(b[at], 0xFF);
        let marker = b[at + 1];
        let len = usize::from(u16::from_be_bytes([b[at + 2], b[at + 3]]));
        if (0xC0..=0xC2).contains(&marker) {
            b[at + 5..at + 9].copy_from_slice(&[0xEA, 0x60, 0xEA, 0x60]);
            return b;
        }
        at += 2 + len;
    }
    panic!("no frame header");
}

#[test]
fn hostile_entries_are_bounded_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    put(&src.join("ok.jpg"), &shot(1, "2026-06-04T10:00:00", None));
    put(&src.join("lies.jpg"), &lying_jpeg());
    // Past the depth limit (4 here; the walk's 32 by default).
    put(&src.join("a/b/c/d/e/f/deep.jpg"), &shot(2, "2026-06-04T10:00:00", None));
    #[cfg(unix)]
    {
        assert!(std::process::Command::new("mkfifo").arg(src.join("pipe.jpg")).status().unwrap().success());
        std::os::unix::fs::symlink(&src, src.join("loop")).unwrap();
    }
    let options = ListOptions { max_depth: 4, ..ListOptions::default() };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(list_all(&src, &options)).unwrap());
    let got = rx.recv_timeout(Duration::from_secs(60)).expect("the listing blocked");
    let names: Vec<&str> = got.candidates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["lies.jpg", "ok.jpg"]);
    let lies = by_name(&got.candidates, "lies.jpg");
    assert!(lies.thumbnail.is_none() && lies.problem.is_some(), "{lies:?}");
    let src = dir.path().join("src");
    let warned = |path: &Path| got.warnings.iter().any(|w| w.path == path);
    assert!(warned(&src.join("lies.jpg")), "{:?}", got.warnings);
    assert!(warned(&src.join("a/b/c/d")), "{:?}", got.warnings);
    if cfg!(unix) {
        assert!(warned(&src.join("pipe.jpg")), "{:?}", got.warnings);
        assert!(warned(&src.join("loop")), "{:?}", got.warnings);
    }

    // The file cap: the rest are not looked at.
    let capped = list_all(&src, &ListOptions { max_files: 1, max_depth: 4, ..ListOptions::default() });
    assert!(capped.listing.truncated);
    assert_eq!(capped.listing.progress.files, 1);
}

#[test]
fn cancel_stops_between_files() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..8u64 {
        put(&dir.path().join(format!("IMG_{i:04}.JPG")), &shot(i, "2026-06-04T10:00:00", None));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let options = ListOptions { cancel: Some(cancel.clone()), batch: 1, ..ListOptions::default() };
    let (mut seen, mut warned) = (0, false);
    let listing = list_source(dir.path(), &options, |event| {
        match event {
            ListEvent::Batch(b) => {
                seen += b.len();
                cancel.store(true, Ordering::Relaxed);
            }
            ListEvent::Warning(w) => warned = w.reason.starts_with("Cancelled"),
            _ => {}
        }
        true
    });
    assert!(listing.cancelled && warned);
    assert_eq!(seen, 1, "one file read, then the flag was seen");
    assert_eq!(listing.progress.done, 1);
    // A receiver that goes away stops it the same way.
    let gone = list_source(dir.path(), &ListOptions { batch: 1, ..ListOptions::default() }, |e| !matches!(e, ListEvent::Batch(_)));
    assert!(gone.cancelled);
    assert_eq!(gone.progress.done, 1);
}

/// A name that is not valid Unicode is listed, and the file is opened by its own name, never by
/// the lossy form (which here names another file). Linux only: macOS and Windows refuse such
/// names.
#[cfg(target_os = "linux")]
#[test]
fn a_non_unicode_name_is_listed_and_never_opened_by_its_lossy_form() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9.jpg"));
    put(&raw, &shot(1, "2026-06-04T10:00:00", None));
    let lossy = dir.path().join("caf\u{FFFD}.jpg");
    put(&lossy, &shot(2, "2026-06-04T18:00:00", None));
    let got = list_all(dir.path(), &ListOptions::default());
    assert_eq!(got.candidates.len(), 2, "{:?}", got.warnings);
    let odd = got.candidates.iter().find(|c| !c.exact_name).unwrap();
    assert_eq!(odd.name, "caf\u{FFFD}.jpg");
    assert_eq!(odd.path, raw);
    assert_eq!(odd.digest, md5_of(&raw));
    assert_ne!(odd.digest, md5_of(&lossy));
    assert_eq!(odd.captured, at("2026-06-04T10:00:00"));
}
