//! Saves beside the photo, Save a Copy and Save As on temp folders (`data-model.md` §6,
//! `formats.md` §1): a PNG with transparency, a TIFF, a WebP and a raw each get a JPEG beside
//! them (the next free name when `<stem>.jpg` is taken) and stay byte-identical, with no originals
//! record; copies take the next free number and leave the photo alone; Save As writes the chosen
//! path in the chosen format and refuses an existing path; bad requests are refused with nothing
//! written.

use crate::common;

use std::path::Path;

use common::*;
use dalica_create::output::{OutFormat, Report};
use dalica_create::save::copy::{Copied, Destination, copy};
use dalica_create::save::{self, Error, Saving};
use dalica_edit::Recipe;
use dalica_image::{Format, Limits, Orientation, RawKind, read_header};
use dalica_meta::Changes;
use dalica_scenes::Exif;
use dalica_scenes::formats::{Tiff, Webp, tiff, webp};

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

/// A RAF raw around `jpeg` (its embedded preview), as the raw container tests build it.
fn raf(jpeg: &[u8]) -> Vec<u8> {
    let mut b = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
    let mut model = b"X-Dalica".to_vec();
    model.resize(32, 0);
    b.extend(model);
    b.extend(b"0100");
    b.extend([0u8; 20]);
    let at = 160u32;
    b.extend(at.to_be_bytes());
    b.extend((jpeg.len() as u32).to_be_bytes());
    b.extend([0u8; 16]);
    b.resize(at as usize, 0);
    b.extend(jpeg);
    b.extend([0x33u8; 200]);
    b
}

/// The files Dalica cannot write in place: (name, bytes, format, whether it has transparency).
fn sources() -> Vec<(&'static str, Vec<u8>, Format, bool)> {
    let exif = Exif { orientation: 1, make: Some("Dalicam".into()), ..Exif::default() };
    vec![
        ("alpha.png", png_rgba(&with_alpha(41, 96, 64)), Format::Png, true),
        ("scan.tif", tiff(&scene(42, 96, 64), &Tiff::default()).unwrap(), Format::Tiff, false),
        ("web.webp", webp(&scene(43, 96, 64), &Webp { exif: Some(exif), ..Webp::default() }).unwrap(), Format::WebP, false),
        ("DSCF0001.RAF", raf(&jpeg(44, 96, 64, 90, 1)), Format::Raw(RawKind::Raf), false),
    ]
}

/// Checks a new JPEG `bytes` as rendered from `source` with `recipe`: orientation 1, the turned
/// size, an EXIF thumbnail, the pixels of the edit (opaque sources).
fn check_jpeg(case: &str, bytes: &[u8], source: &[u8], recipe: &Recipe, opaque: bool, report: &Report) {
    let header = read_header(bytes, &Limits::default()).unwrap();
    assert_eq!(header.format, Format::Jpeg, "{case}");
    assert_eq!(header.orientation, Orientation::UPRIGHT, "{case}");
    let expected = oracle(source, recipe);
    assert_eq!(header.display_size(), (expected.width, expected.height), "{case}");
    assert_eq!((report.format, report.width, report.height), (OutFormat::Jpeg, expected.width, expected.height), "{case}");
    assert!(header.thumbnail.is_some() && report.thumbnail, "{case}: a fresh EXIF thumbnail");
    if opaque {
        let p = psnr(&decode(bytes), &expected);
        assert!(p > 32.0, "{case}: {p:.1} dB");
    }
}

#[test]
fn each_format_gets_a_jpeg_beside_and_stays_byte_identical() {
    let edits = recipe(&["rotate(90)", "autofix()"]);
    for (name, bytes, format, alpha) in sources() {
        let tmp = folder();
        let dir = tmp.path();
        std::fs::write(dir.join(name), &bytes).unwrap();
        assert_eq!(read_header(&bytes, &Limits::default()).unwrap().format, format, "{name}");

        let saving = save::save(dir, name, &edits, &Changes::default(), &options()).unwrap();
        let Saving::Beside(beside) = saving else { panic!("{name}: {saving:?}") };
        let stem = name.rsplit_once('.').unwrap().0;
        assert_eq!(beside.name, format!("{stem}.jpg"), "{name}");
        assert_eq!(beside.recipe, ["rotate(90)", "autofix()"], "{name}: the recipe the copy holds");
        assert_eq!(read(dir.join(name)), bytes, "{name}: the source is untouched");
        assert_eq!(names(dir), {
            let mut v = vec![name.to_string(), beside.name.clone()];
            v.sort();
            v
        });
        check_jpeg(name, &read(dir.join(&beside.name)), &bytes, &edits, !alpha, &beside.report);
        // Undo Save and Revert apply to in-place saves only.
        assert!(matches!(save::undo(dir, name, &options()), Err(Error::NotSaved(_))), "{name}");
        assert!(matches!(save::revert(dir, name, &options()), Err(Error::NotSaved(_))), "{name}");
    }
}

#[test]
fn transparency_is_composited_over_white() {
    let tmp = folder();
    let dir = tmp.path();
    let img = with_alpha(45, 96, 64);
    std::fs::write(dir.join("alpha.png"), png_rgba(&img)).unwrap();
    let Saving::Beside(beside) = save::save(dir, "alpha.png", &recipe(&["flip_h()"]), &Changes::default(), &options()).unwrap() else {
        panic!("not beside")
    };
    let out = decode(&read(dir.join(beside.name)));
    // The source's left edge is transparent; flipped, it is the right edge, now white.
    let right = &out.data[((out.width - 1) * 4) as usize..(out.width * 4) as usize];
    assert!(right[..3].iter().all(|&v| v >= 250), "{right:?}");
    assert!(out.data.chunks(4).all(|p| p[3] == 255), "opaque");
}

#[test]
fn a_taken_name_moves_to_the_next_free_number() {
    let tmp = folder();
    let dir = tmp.path();
    let source = tiff(&scene(46, 64, 48), &Tiff::default()).unwrap();
    std::fs::write(dir.join("scan.tif"), &source).unwrap();
    // A camera's JPEG of the same shot already has the name.
    std::fs::write(dir.join("scan.jpg"), b"someone else's file").unwrap();
    let mut got = Vec::new();
    for _ in 0..2 {
        let Saving::Beside(beside) = save::save(dir, "scan.tif", &recipe(&["rotate(180)"]), &Changes::default(), &options()).unwrap() else {
            panic!("not beside")
        };
        got.push(beside.name);
    }
    assert_eq!(got, ["scan-001.jpg", "scan-002.jpg"]);
    assert_eq!(read(dir.join("scan.jpg")), b"someone else's file", "never replaced");
    assert_eq!(read(dir.join("scan.tif")), source);
}

#[test]
fn copies_take_the_next_free_number_and_leave_the_photo_alone() {
    let tmp = folder();
    let dir = tmp.path();
    let photo = jpeg(47, 160, 120, 85, 1);
    std::fs::write(dir.join("IMG_0001.jpg"), &photo).unwrap();
    let edits = recipe(&["rotate(270)", "autofix()"]);
    // A copy is a new file: every owned field goes in, stars as ratings too, whatever the
    // write-back options say.
    let changes = Changes { caption: Some(Some("Pier".into())), star: Some(true), ..Changes::default() };
    let mut opts = options();
    opts.write.captions_tags_places = false;
    opts.write.stars_as_ratings = false;

    let first: Copied = copy(dir, "IMG_0001.jpg", &edits, &changes, &Destination::Numbered, &opts).unwrap();
    let second = copy(dir, "IMG_0001.jpg", &edits, &changes, &Destination::Numbered, &opts).unwrap();
    assert_eq!(first.path, dir.join("IMG_0001-001.jpg"));
    assert_eq!(second.path, dir.join("IMG_0001-002.jpg"));
    assert_eq!(read(dir.join("IMG_0001.jpg")), photo, "the photo is untouched");
    assert!(!dir.join(".dalica-originals").exists(), "no originals record");

    let bytes = read(&first.path);
    check_jpeg("copy", &bytes, &photo, &edits, true, &first.report);
    let meta = dalica_meta::read(&bytes, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.owned.caption.as_deref(), Some("Pier"));
    assert_eq!(meta.owned.rating, Some(5));
    assert_eq!(meta.summary.make.as_deref(), Some("Dalicam"), "the source's metadata is carried");

    // A gap is filled first; a PNG's copy is a JPEG too.
    std::fs::remove_file(&first.path).unwrap();
    let png_photo = png(48, 64, 48);
    std::fs::write(dir.join("IMG_0001.png"), &png_photo).unwrap();
    let third = copy(dir, "IMG_0001.png", &Recipe::default(), &Changes::default(), &Destination::Numbered, &options()).unwrap();
    assert_eq!(third.path, dir.join("IMG_0001-001.jpg"));
    assert_eq!(read_header(&read(&third.path), &Limits::default()).unwrap().format, Format::Jpeg);
}

#[test]
fn save_as_writes_the_chosen_path_and_format_and_refuses_an_existing_path() {
    let tmp = folder();
    let dir = tmp.path();
    let photo = jpeg(49, 128, 96, 90, 1);
    std::fs::write(dir.join("IMG_0001.jpg"), &photo).unwrap();
    std::fs::create_dir(dir.join("elsewhere")).unwrap();
    let edits = recipe(&["rotate(90)"]);

    let to = Destination::Path { path: dir.join("elsewhere").join("Harbour.png"), format: OutFormat::Png };
    let saved = copy(dir, "IMG_0001.jpg", &edits, &Changes::default(), &to, &options()).unwrap();
    assert_eq!(saved.path, dir.join("elsewhere").join("Harbour.png"));
    assert_eq!(saved.report.format, OutFormat::Png);
    let bytes = read(&saved.path);
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().format, Format::Png);
    assert_eq!(decode(&bytes), oracle(&photo, &edits), "a PNG holds the edit exactly");

    // The same path again, and a path some other file holds: refused, nothing changed.
    let err = copy(dir, "IMG_0001.jpg", &recipe(&["autofix()"]), &Changes::default(), &to, &options()).unwrap_err();
    assert!(err.reason().contains("already exists"), "{err}");
    assert_eq!(read(&saved.path), bytes);
    std::fs::write(dir.join("taken.jpg"), b"keep me").unwrap();
    let taken = Destination::Path { path: dir.join("taken.jpg"), format: OutFormat::Jpeg };
    assert!(copy(dir, "IMG_0001.jpg", &edits, &Changes::default(), &taken, &options()).is_err());
    assert_eq!(read(dir.join("taken.jpg")), b"keep me");

    // A JPEG Save As.
    let to = Destination::Path { path: dir.join("elsewhere").join("Harbour.jpg"), format: OutFormat::Jpeg };
    let saved = copy(dir, "IMG_0001.jpg", &edits, &Changes::default(), &to, &options()).unwrap();
    check_jpeg("save as", &read(&saved.path), &photo, &edits, true, &saved.report);
    assert_eq!(read(dir.join("IMG_0001.jpg")), photo, "the photo is untouched");
    assert_eq!(names(dir), ["IMG_0001.jpg", "elsewhere", "taken.jpg"]);
}

#[test]
fn bad_copies_are_refused_with_nothing_written() {
    let tmp = folder();
    let dir = tmp.path();
    std::fs::write(dir.join("IMG_0001.jpg"), jpeg(50, 64, 48, 90, 1)).unwrap();
    let opts = options();
    let numbered = Destination::Numbered;
    let none = Changes::default();

    let err = copy(dir, "missing.jpg", &Recipe::default(), &none, &numbered, &opts).unwrap_err();
    assert!(matches!(err, Error::Missing(_)), "{err:?}");
    for name in ["../IMG_0001.jpg", "", ".dalica-originals", "CON"] {
        let err = copy(dir, name, &Recipe::default(), &none, &numbered, &opts).unwrap_err();
        assert!(matches!(err, Error::Name(_)), "{name:?}: {err:?}");
    }
    let unknown = Recipe::parse(["sparkle(3)"]).unwrap();
    let err = copy(dir, "IMG_0001.jpg", &unknown, &none, &numbered, &opts).unwrap_err();
    assert!(matches!(err, Error::Unrenderable(ref op) if op == "sparkle(3)"), "{err:?}");
    let to = Destination::Path { path: dir.join("no such folder").join("x.jpg"), format: OutFormat::Jpeg };
    assert!(copy(dir, "IMG_0001.jpg", &Recipe::default(), &none, &to, &opts).is_err());
    std::fs::write(dir.join("text.jpg"), b"not a picture").unwrap();
    assert!(matches!(copy(dir, "text.jpg", &Recipe::default(), &none, &numbered, &opts), Err(Error::Output(_))));
    assert_eq!(names(dir), ["IMG_0001.jpg", "text.jpg"]);
}
