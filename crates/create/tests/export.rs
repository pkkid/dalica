//! Export planning and files (`creating.md` §2) on temporary folders: the exported files are
//! decoded and checked (long side, never enlarged, each quality preset's estimate, metadata copied,
//! GPS removed only when asked, edits applied); a PNG with alpha stays PNG; collisions get `-001`
//! and numbers keep the tray order; the export folder takes a free name; videos and undecodable
//! files are skipped with their reason; a cancel at any step leaves only complete, decodable files.

use crate::common;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use dalica_create::export::{self, ExportOptions, Item, Kind, Outcome, QualityPreset, Size, Summary};
use dalica_create::output::OutFormat;
use dalica_edit::Recipe;
use dalica_image::{Format, Limits, jpeg_quality, read_header};
use dalica_meta::{Changes, Place};

const FALLBACK: &str = "Export 2026-06-04";

struct Fixture {
    _dir: tempfile::TempDir,
    src: PathBuf,
    out: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    let out = dir.path().join("Exports");
    Fixture { src, out, _dir: dir }
}

impl Fixture {
    /// Writes `bytes` as `name` in the source folder.
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.src.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

fn photo(path: &Path) -> Item {
    Item { path: path.to_path_buf(), kind: Kind::Photo, recipe: Recipe::default(), changes: Changes::default() }
}

fn options(size: Size, quality: QualityPreset) -> ExportOptions {
    ExportOptions { size, quality, ..ExportOptions::default() }
}

fn never() -> bool {
    false
}

/// Plans and runs an export of `items` into `out/<folder>`.
fn export(out: &Path, folder: &str, items: Vec<Item>, options: &ExportOptions) -> (export::Plan, Summary) {
    let plan = export::plan(out, folder, FALLBACK, items, options).unwrap();
    let summary = export::run(&plan, &never);
    (plan, summary)
}

/// The names in `folder`, sorted.
fn listing(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn files_are_resized_to_the_long_side_and_never_enlarged() {
    let f = fixture();
    let wide = f.put("wide.jpg", &jpeg(1, 600, 400, 90, 1));
    let tall = f.put("tall.jpg", &jpeg(2, 300, 500, 90, 1));
    let small = f.put("small.jpg", &jpeg(3, 160, 120, 90, 1));
    let (plan, s) = export(&f.out, "Sized", vec![photo(&wide), photo(&tall), photo(&small)], &options(Size::LongSide(250), QualityPreset::Normal));
    assert_eq!(plan.destination.folder, f.out.join("Sized"));
    assert_eq!(s.written.len(), 3, "{s:?}");
    let sizes: Vec<(u32, u32)> = s.written.iter().map(|w| decode(&std::fs::read(&w.path).unwrap())).map(|d| (d.width, d.height)).collect();
    assert_eq!(sizes, [(250, 167), (150, 250), (160, 120)], "the long side, never enlarged");
    assert_eq!(listing(&plan.destination.folder), ["small.jpg", "tall.jpg", "wide.jpg"]);

    let (_, s) = export(&f.out, "Original", vec![photo(&wide)], &options(Size::Original, QualityPreset::Normal));
    let d = decode(&std::fs::read(&s.written[0].path).unwrap());
    assert_eq!((d.width, d.height), (600, 400));
}

#[test]
fn each_quality_preset_lands_within_its_bounds() {
    let f = fixture();
    let q75 = f.put("q75.jpg", &jpeg(4, 320, 240, 75, 1));
    let png = f.put("flat.png", &png(5, 320, 240));
    let cases = [
        (QualityPreset::Normal, 85),
        (QualityPreset::Maximum, 95),
        (QualityPreset::Minimum, 60),
        (QualityPreset::Custom(40), 40),
        (QualityPreset::Custom(100), 100),
    ];
    for (preset, want) in cases {
        let (_, s) = export(&f.out, "Quality", vec![photo(&q75)], &options(Size::Original, preset));
        let bytes = std::fs::read(&s.written[0].path).unwrap();
        let got = jpeg_quality::quality(&bytes).unwrap();
        assert!(got.abs_diff(want) <= 2, "{preset:?}: estimated {got}, want {want} ± 2");
        assert_eq!(s.written[0].report.quality, Some(want));
    }
    // Automatic keeps the source's quality, and 90 when it has none.
    let (_, s) = export(&f.out, "Quality", vec![photo(&q75), photo(&png)], &options(Size::Original, QualityPreset::Automatic));
    let estimated: Vec<u8> = s.written.iter().map(|w| jpeg_quality::quality(&std::fs::read(&w.path).unwrap()).unwrap()).collect();
    assert!(estimated[0].abs_diff(75) <= 2, "{estimated:?}");
    assert!(estimated[1].abs_diff(90) <= 2, "{estimated:?}");
    assert_eq!(s.written[1].name, "flat.jpg", "an opaque PNG exports as JPEG");
}

/// A JPEG with camera EXIF (make, model, GPS) and a caption and tags written as Dalica writes them.
fn described() -> Vec<u8> {
    let source = jpeg(6, 300, 200, 90, 1);
    let changes = Changes {
        caption: Some(Some("Harbour at dusk".into())),
        tags: Some(vec!["boats".into()]),
        place: Some(Place::new(38.708, -9.136)),
        ..Changes::default()
    };
    dalica_meta::write_owned(&source, &changes, &dalica_meta::WriteOptions::default()).unwrap()
}

#[test]
fn metadata_is_copied_and_gps_removed_only_when_asked() {
    let f = fixture();
    let src = f.put("harbour.jpg", &described());
    let item =
        || Item { changes: Changes { tags: Some(vec!["boats".into(), "exported".into()]), star: Some(true), ..Changes::default() }, ..photo(&src) };
    let (_, kept) = export(&f.out, "Kept", vec![item()], &ExportOptions::default());
    let after = meta(&std::fs::read(&kept.written[0].path).unwrap());
    assert!(after.problems.is_empty(), "{:?}", after.problems);
    assert_eq!(after.summary.make.as_deref(), Some("Dalicam"));
    assert_eq!(after.summary.model.as_deref(), Some("Model 7"));
    assert_eq!(after.owned.caption.as_deref(), Some("Harbour at dusk"));
    assert_eq!(after.owned.tags, ["boats", "exported"], "the library's owned fields");
    assert!(after.owned.starred(), "stars are written as ratings on an export");
    assert!(after.owned.place.is_some(), "GPS kept unless asked");
    assert!(read_header(&std::fs::read(&kept.written[0].path).unwrap(), &Limits::default()).unwrap().thumbnail.is_none(), "no EXIF thumbnail");

    let removing = ExportOptions { remove_location: true, ..ExportOptions::default() };
    let (_, removed) = export(&f.out, "Removed", vec![item()], &removing);
    let after = meta(&std::fs::read(&removed.written[0].path).unwrap());
    assert_eq!(after.owned.place, None);
    assert_eq!(after.sources.exif.place, None);
    assert_eq!(after.sources.xmp.place, None);
    assert_eq!(after.owned.caption.as_deref(), Some("Harbour at dusk"), "the rest stays");
    // The source is never touched.
    assert_eq!(std::fs::read(&src).unwrap(), described());
}

#[test]
fn edits_are_rendered_into_the_export() {
    let f = fixture();
    let src = f.put("turned.jpg", &jpeg(7, 400, 300, 90, 1));
    let item = Item { recipe: recipe(&["rotate(90)"]), ..photo(&src) };
    let (_, s) = export(&f.out, "Edited", vec![item], &options(Size::LongSide(200), QualityPreset::Normal));
    let out = std::fs::read(&s.written[0].path).unwrap();
    let d = decode(&out);
    assert_eq!((d.width, d.height), (150, 200), "rotated, then fitted");
    assert_eq!(read_header(&out, &Limits::default()).unwrap().orientation.get(), 1);
}

#[test]
fn a_png_with_alpha_exports_as_png() {
    let f = fixture();
    let src = f.put("logo.png", &png_rgba(&with_alpha(8, 200, 100)));
    let (_, s) = export(&f.out, "Alpha", vec![photo(&src)], &options(Size::LongSide(100), QualityPreset::Normal));
    let w = &s.written[0];
    assert_eq!((w.name.as_str(), w.report.format), ("logo.png", OutFormat::Png));
    let bytes = std::fs::read(&w.path).unwrap();
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().format, Format::Png);
    let d = decode(&bytes);
    assert_eq!((d.width, d.height), (100, 50));
    assert!(d.data.chunks(4).any(|p| p[3] < 255), "transparency kept");
}

#[test]
fn collisions_get_001_and_numbers_keep_the_tray_order() {
    let f = fixture();
    let a = f.put("one/beach.jpg", &jpeg(9, 64, 48, 90, 1));
    let b = f.put("two/beach.jpg", &jpeg(10, 64, 48, 90, 1));
    let c = f.put("three/beach.png", &png(11, 64, 48));
    let (plan, s) = export(&f.out, "Beaches", vec![photo(&a), photo(&b), photo(&c)], &options(Size::Original, QualityPreset::Normal));
    assert_eq!(s.written.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), ["beach.jpg", "beach-001.jpg", "beach-002.jpg"]);
    assert_eq!(listing(&plan.destination.folder), ["beach-001.jpg", "beach-002.jpg", "beach.jpg"]);
    // Each file is its own source's picture.
    for w in &s.written {
        assert_eq!(decode(&std::fs::read(&w.path).unwrap()).width, 64);
    }
    assert_ne!(std::fs::read(&s.written[0].path).unwrap(), std::fs::read(&s.written[1].path).unwrap());

    // Numbered: the tray order, not the names' order, and the same stem twice still collides-free.
    let z = f.put("zebra.jpg", &jpeg(12, 64, 48, 90, 1));
    let numbered = ExportOptions { numbers: true, ..options(Size::Original, QualityPreset::Normal) };
    let (plan, s) = export(&f.out, "Ordered", vec![photo(&z), photo(&b), photo(&a), photo(&c)], &numbered);
    let names: Vec<&str> = s.written.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["001-zebra.jpg", "002-beach.jpg", "003-beach.jpg", "004-beach.jpg"]);
    assert_eq!(listing(&plan.destination.folder), names, "sorting by name gives the tray order");
}

#[test]
fn the_export_folder_takes_a_free_sanitized_name() {
    let f = fixture();
    let src = f.put("p.jpg", &jpeg(13, 64, 48, 90, 1));
    let o = options(Size::Original, QualityPreset::Normal);
    let (first, _) = export(&f.out, "Lisbon", vec![photo(&src)], &o);
    let (second, _) = export(&f.out, "Lisbon", vec![photo(&src)], &o);
    assert_eq!((first.destination.name.as_str(), second.destination.name.as_str()), ("Lisbon", "Lisbon-001"));
    assert_eq!(listing(&first.destination.folder), ["p.jpg"], "a second export never reuses the first folder");
    // Hostile and empty names.
    let (p, _) = export(&f.out, "../../etc", vec![], &o);
    assert_eq!(p.destination.folder, f.out.join("etc"));
    let (p, _) = export(&f.out, "CON", vec![], &o);
    assert_eq!(p.destination.name, "_CON");
    let (p, _) = export(&f.out, " .. ", vec![], &o);
    assert_eq!(p.destination.name, FALLBACK);
    assert_eq!(listing(&f.out), ["Export 2026-06-04", "Lisbon", "Lisbon-001", "_CON", "etc"]);
}

#[test]
fn videos_and_undecodable_files_are_skipped_with_their_reason() {
    let f = fixture();
    let good = f.put("good.jpg", &jpeg(14, 64, 48, 90, 1));
    let junk = f.put("junk.jpg", b"not a picture at all");
    let clip = f.put("clip.mp4", b"\0\0\0\x18ftypmp42");
    let gone = f.src.join("gone.jpg");
    let video = Item { kind: Kind::Video, ..photo(&clip) };
    let (plan, s) = export(&f.out, "Mixed", vec![photo(&junk), video, photo(&good), photo(&gone)], &ExportOptions::default());
    assert_eq!(plan.skipped.len(), 1, "the video is skipped by the plan");
    assert_eq!(s.written.iter().map(|w| w.index).collect::<Vec<_>>(), [2]);
    assert_eq!(s.skipped.iter().map(|k| k.index).collect::<Vec<_>>(), [0, 1]);
    assert!(!s.skipped[0].reason.is_empty() && s.skipped[1].reason.contains("video"), "{:?}", s.skipped);
    assert_eq!(s.failed.len(), 1);
    assert_eq!(s.failed[0].index, 3);
    assert!(s.failed[0].reason.contains("gone"), "{}", s.failed[0].reason);
    assert!(!s.cancelled);
    assert_eq!(listing(&plan.destination.folder), ["good.jpg"]);
}

#[test]
fn bad_options_are_refused_before_anything_is_made() {
    let f = fixture();
    for o in [
        options(Size::LongSide(99), QualityPreset::Normal),
        options(Size::LongSide(4001), QualityPreset::Normal),
        options(Size::Original, QualityPreset::Custom(0)),
    ] {
        assert!(matches!(export::plan(&f.out, "Bad", FALLBACK, vec![], &o), Err(export::Error::Options(_))), "{o:?}");
    }
    assert!(!f.out.exists(), "no folder made");
    // A location that is a file cannot hold an export folder.
    let file = f.put("not-a-folder", b"x");
    assert!(matches!(export::plan(&file, "X", FALLBACK, vec![], &ExportOptions::default()), Err(export::Error::Folder(_))));
}

/// Every file in `folder` decodes whole, no temporary file is left, and the names are exactly
/// those the summary reports written.
fn only_complete_files(folder: &Path, s: &Summary) {
    let mut written: Vec<String> = s.written.iter().map(|w| w.name.clone()).collect();
    written.sort();
    assert_eq!(listing(folder), written, "only the files reported written");
    for name in &written {
        let bytes = std::fs::read(folder.join(name)).unwrap();
        let header = read_header(&bytes, &Limits::default()).unwrap();
        let d = dalica_image::decode(&bytes, &Limits::default()).unwrap();
        assert_eq!((d.width, d.height), (header.width, header.height), "{name} decodes whole");
    }
}

#[test]
fn a_cancel_at_any_step_leaves_only_complete_files() {
    let f = fixture();
    let items: Vec<Item> = (0..3).map(|i| photo(&f.put(&format!("p{i}.jpg"), &jpeg(20 + i, 120, 90, 90, 1)))).collect();
    // Each item asks three times (before reading, before rendering, before writing): cancel after
    // the n-th question, for every n.
    for n in 0..=9u32 {
        let plan = export::plan(&f.out, &format!("Cancel {n}"), FALLBACK, items.clone(), &ExportOptions::default()).unwrap();
        let asked = Cell::new(0u32);
        let cancel = || {
            asked.set(asked.get() + 1);
            asked.get() > n
        };
        let s = export::run(&plan, &cancel);
        assert_eq!(s.cancelled, n < 9, "n = {n}");
        assert_eq!(s.written.len(), (n / 3) as usize, "n = {n}: only items whose every step ran");
        only_complete_files(&plan.destination.folder, &s);
    }
    // One item cancelled just before its write leaves nothing.
    let plan = export::plan(&f.out, "One", FALLBACK, items[..1].to_vec(), &ExportOptions::default()).unwrap();
    let asked = Cell::new(0u32);
    let late = || {
        asked.set(asked.get() + 1);
        asked.get() == 3
    };
    assert_eq!(export::export_one(&plan.items[0], &plan.destination, &late), Outcome::Cancelled);
    assert!(listing(&plan.destination.folder).is_empty());
}

#[test]
fn a_cancel_from_another_thread_leaves_only_complete_files() {
    let f = fixture();
    let items: Vec<Item> = (0..6).map(|i| photo(&f.put(&format!("big{i}.jpg"), &jpeg(30 + i, 1200, 900, 92, 1)))).collect();
    let plan = export::plan(&f.out, "Raced", FALLBACK, items, &options(Size::Original, QualityPreset::Normal)).unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let setter = {
        let (flag, folder) = (Arc::clone(&flag), plan.destination.folder.clone());
        std::thread::spawn(move || {
            // Cancel as soon as the first file is there, while the next one is on its way.
            for _ in 0..2000 {
                if listing(&folder).iter().any(|n| n.ends_with(".jpg")) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            flag.store(true, Ordering::SeqCst);
        })
    };
    let s = export::run(&plan, &|| flag.load(Ordering::SeqCst));
    setter.join().unwrap();
    assert!(!s.written.is_empty());
    only_complete_files(&plan.destination.folder, &s);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(cases(2048)))]

    /// Whatever a source is called, its planned name (numbered or not, with either extension) is
    /// one plain component `fs` accepts for a new file.
    #[test]
    fn planned_names_are_always_plain_file_names(name in "\\PC{0,40}|[./\\\\:*?\"<>| -]{0,12}|(CON|nul|COM1)(\\.[a-z]{0,4})?|[é東🌊a. .-]{100,300}", numbers: bool) {
        let item = photo(&Path::new("src").join(&name));
        let (planned, _) = export::names(vec![item], numbers);
        for ext in ["jpg", "png"] {
            let file = format!("{}.{ext}", planned[0].stem);
            proptest::prop_assert!(dalica_fs::create::plain_name(&file), "{name:?} → {file:?}");
            proptest::prop_assert!(file.len() <= 255);
            proptest::prop_assert!(!file.starts_with(['.', '-']) && !file.contains(char::is_control));
        }
    }
}

#[test]
fn the_watermark_is_drawn_bottom_right_when_asked() {
    let f = fixture();
    let src = f.put("w.png", &png(40, 400, 300));
    let plain = options(Size::Original, QualityPreset::Maximum);
    let marked = ExportOptions { watermark: Some("© Ana Costa 2026".into()), ..plain.clone() };
    let (_, a) = export(&f.out, "Plain", vec![photo(&src)], &plain);
    let (_, b) = export(&f.out, "Marked", vec![photo(&src)], &marked);
    let (a, b) = (decode(&std::fs::read(&a.written[0].path).unwrap()), decode(&std::fs::read(&b.written[0].path).unwrap()));
    // The text box: 3 % of the long side high, near the bottom-right corner.
    let p = dalica_create::watermark::layout(400, 300, "© Ana Costa 2026").unwrap();
    let changed = |x0: u32, y0: u32, x1: u32, y1: u32| {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * 400 + x) * 4) as usize;
                n += usize::from((0..3).any(|c| a.data[i + c].abs_diff(b.data[i + c]) > 24));
            }
        }
        n
    };
    let (x0, y0) = (p.x.floor() as u32, p.y.floor() as u32);
    let (x1, y1) = (((p.x + p.width).ceil() as u32).min(400), ((p.y + p.height).ceil() as u32).min(300));
    assert!(changed(x0, y0, x1, y1) > 50, "the text is in its box");
    assert_eq!(changed(0, 0, 200, 150), 0, "the rest of the picture is unchanged");
}
