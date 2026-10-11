//! The thumbnail ladder (`architecture.md` §6.5): the covering size and its fallbacks, the
//! direction-aware prefetch order, and the scan and fetch jobs on a cache in a temporary folder.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use dalica_image::Limits;
use dalica_thumbs::ladder::{self, Choice, Error, Source, choose, covering, entry_key, prefetch};
use dalica_thumbs::pack::{Cache, Options, Size};
use dalica_thumbs::{Rgba8, sketch};

const SCREEN: u32 = 2560;

fn not_cancelled() -> AtomicBool {
    AtomicBool::new(false)
}

/// A `scenes` JPEG of `w × h` in `dir`.
fn photo(dir: &Path, name: &str, w: u32, h: u32) -> Source {
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(7, w, h), 85, None).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, &bytes).unwrap();
    Source { path, file_size: bytes.len() as u64, mtime_ms: 1_700_000_000_000 }
}

fn cache(dir: &Path) -> Cache {
    Cache::open(&dir.join("thumbs"), Options::default()).unwrap()
}

fn dims(img: &Rgba8) -> (u32, u32) {
    (img.width, img.height)
}

#[test]
fn the_covering_size_is_the_smallest_at_least_as_long_as_the_cell() {
    let cases = [(0, Size::S128), (1, Size::S128), (128, Size::S128), (129, Size::S256), (300, Size::S512), (1024, Size::S1024)];
    for (need, size) in cases {
        assert_eq!(covering(need, SCREEN), size, "{need}");
    }
    assert_eq!(covering(1025, SCREEN), Size::Screen);
    // Above the ladder: the largest size (a full decode is the edit view's).
    assert_eq!(covering(9000, SCREEN), Size::Screen);
    // A small display sorts the screen size by its pixels.
    assert_eq!(covering(700, 800), Size::Screen);
    assert_eq!(covering(900, 800), Size::S1024);
    assert_eq!(covering(5000, 800), Size::S1024);
}

#[test]
fn a_cell_draws_what_covers_it_else_the_next_smaller_size() {
    let has = |sizes: &'static [Size]| move |s: Size| sizes.contains(&s);
    assert_eq!(choose(300, SCREEN, has(&[Size::S256, Size::S512])), Choice::Covers(Size::S512));
    assert_eq!(choose(300, SCREEN, has(&[Size::S128, Size::S256])), Choice::Smaller(Size::S256));
    assert_eq!(choose(300, SCREEN, has(&[Size::S128])), Choice::Smaller(Size::S128));
    // A larger size already in memory covers too; the smallest of those wins.
    assert_eq!(choose(300, SCREEN, has(&[Size::S1024, Size::Screen])), Choice::Covers(Size::S1024));
    assert_eq!(choose(300, SCREEN, has(&[])), Choice::Nothing);
    assert_eq!(choose(100, SCREEN, has(&[Size::S128, Size::S256])), Choice::Covers(Size::S128));
}

#[test]
fn prefetch_follows_the_scroll_direction_and_reaches_further_when_faster() {
    // Rows of 10 cells, 50 visible (100..150) out of 1000.
    let down = prefetch(100..150, 1000, 40.0);
    assert_eq!(down[..3], [150, 151, 152], "ahead first, nearest first");
    let ahead = down.iter().take_while(|&&i| i >= 150).count();
    assert_eq!(ahead, 50 + 30, "one screen plus 0.75 s at 40 cells/s");
    assert_eq!(down[ahead..], (75..100).rev().collect::<Vec<_>>()[..], "then half a screen behind, nearest first");

    let up = prefetch(100..150, 1000, -40.0);
    assert_eq!(up[..3], [99, 98, 97]);
    assert!(up.iter().take_while(|&&i| i < 100).count() == 80 && up.iter().filter(|&&i| i >= 150).count() == 25);

    let fast = prefetch(100..150, 1000, 4000.0);
    assert_eq!(fast.iter().filter(|&&i| i >= 150).count(), 200, "at most four screens ahead");
    let slow = prefetch(100..150, 1000, 1.0);
    assert!(slow.iter().filter(|&&i| i >= 150).count() < fast.iter().filter(|&&i| i >= 150).count());

    // Standing still: a screen each side, alternating, nearest first.
    let still = prefetch(100..150, 1000, 0.0);
    assert_eq!(still[..4], [150, 99, 151, 98]);
    assert_eq!(still.len(), 100);
}

#[test]
fn prefetch_stays_inside_the_list_whatever_it_is_given() {
    for (visible, len, v) in [
        (0..0, 0, 10.0),
        (990..1000, 1000, 50.0),
        (0..10, 1000, -50.0),
        (Range { start: 500, end: 400 }, 1000, 1.0),
        (2000..3000, 1000, 1.0),
        (0..10, 1000, f64::NAN),
        (0..10, 1000, f64::INFINITY),
        (usize::MAX - 1..usize::MAX, usize::MAX, 1e300),
    ] {
        let plan = prefetch(visible.clone(), len, v);
        assert!(plan.iter().all(|&i| i < len && !visible.contains(&i)), "{visible:?} {len} {v}");
        let mut unique = plan.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), plan.len(), "no cell twice");
    }
}

#[test]
fn keys_follow_the_file_and_the_screen_size() {
    let s = Source { path: PathBuf::from("/p/a.jpg"), file_size: 10, mtime_ms: 5 };
    let touched = Source { mtime_ms: 6, ..s.clone() };
    assert_ne!(s.key(), touched.key());
    assert_eq!(s.key(), s.clone().key(), "stable");
    for size in [Size::S128, Size::S256, Size::S512, Size::S1024] {
        assert_eq!(entry_key(s.key(), size, 1000), s.key());
    }
    assert_ne!(entry_key(s.key(), Size::Screen, 1000), entry_key(s.key(), Size::Screen, 2000));
    assert_ne!(entry_key(s.key(), Size::Screen, 1000), s.key());
}

#[test]
fn a_scan_decodes_once_and_makes_the_128_and_256_entries_and_the_sketch() {
    let dir = tempfile::tempdir().unwrap();
    let c = cache(dir.path());
    let src = photo(dir.path(), "a.jpg", 600, 400);
    let limits = Limits::default();
    let first = ladder::scan(&c, &src, true, &limits, &not_cancelled()).unwrap();
    assert!(first.decoded);
    let sizes: Vec<(Size, (u32, u32))> = first.images.iter().map(|(s, i)| (*s, dims(i))).collect();
    assert_eq!(sizes, [(Size::S128, (128, 86)), (Size::S256, (256, 171))], "the 128 is made from the 256");
    for (size, img) in &first.images {
        let bytes = c.get(*size, src.key()).unwrap().expect("the entry is cached");
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG");
        assert_eq!(dims(&ladder::decode_entry(&bytes, &limits).unwrap()), dims(img));
    }
    let sk = first.sketch.expect("asked for a sketch");
    assert!(sk.len() <= 26);
    assert!((sketch::decode(&sk).unwrap().aspect() - 1.5).abs() < 0.05);

    // What a scan hands back is the cached entry's pixels: the same as a later read of it.
    for (size, img) in &first.images {
        let read = ladder::decode_entry(&c.get(*size, src.key()).unwrap().unwrap(), &limits).unwrap();
        assert!(read == *img, "{size:?}: the scan's image differs from the cached entry");
    }

    // Again: the entries are there, nothing is decoded.
    let again = ladder::scan(&c, &src, false, &limits, &not_cancelled()).unwrap();
    assert_eq!((again.decoded, again.images.len(), again.sketch), (false, 0, None));
    // A sketch for a photo with cached entries comes from the cached 128 entry.
    let sketch_only = ladder::scan(&c, &src, true, &limits, &not_cancelled()).unwrap();
    assert!(!sketch_only.decoded);
    assert!(sketch::decode(&sketch_only.sketch.unwrap()).is_ok());
}

#[test]
fn a_fetch_reads_the_cache_and_decodes_only_what_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let c = cache(dir.path());
    let src = photo(dir.path(), "a.jpg", 1200, 800);
    let limits = Limits::default();
    let made = ladder::fetch(&c, &src, Size::S512, SCREEN, &limits, &not_cancelled()).unwrap();
    assert!(made.decoded);
    assert_eq!(dims(&made.image), (512, 341));
    let read = ladder::fetch(&c, &src, Size::S512, SCREEN, &limits, &not_cancelled()).unwrap();
    assert!(!read.decoded);
    assert_eq!(dims(&read.image), (512, 341));
    assert!(read.image == made.image, "made or read, the same pixels");
    // Never enlarged: the 1024 and screen entries of a 1200-pixel photo.
    assert_eq!(dims(&ladder::fetch(&c, &src, Size::S1024, SCREEN, &limits, &not_cancelled()).unwrap().image), (1024, 683));
    assert_eq!(dims(&ladder::fetch(&c, &src, Size::Screen, SCREEN, &limits, &not_cancelled()).unwrap().image), (1200, 800));
    // Another screen size is another entry.
    let small_screen = ladder::fetch(&c, &src, Size::Screen, 600, &limits, &not_cancelled()).unwrap();
    assert!(small_screen.decoded);
    assert_eq!(dims(&small_screen.image), (600, 400));
    assert!(!ladder::fetch(&c, &src, Size::Screen, 600, &limits, &not_cancelled()).unwrap().decoded);
}

#[test]
fn unreadable_photos_and_cancelled_jobs_fail_and_cache_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let c = cache(dir.path());
    let limits = Limits::default();
    let missing = Source { path: dir.path().join("gone.jpg"), file_size: 1, mtime_ms: 1 };
    assert!(matches!(ladder::scan(&c, &missing, true, &limits, &not_cancelled()), Err(Error::Photo(_))));
    std::fs::write(dir.path().join("junk.jpg"), b"\xff\xd8\xff\xe0 not really a jpeg").unwrap();
    let junk = Source { path: dir.path().join("junk.jpg"), file_size: 20, mtime_ms: 1 };
    assert!(matches!(ladder::scan(&c, &junk, true, &limits, &not_cancelled()), Err(Error::Photo(_))));
    assert!(matches!(ladder::fetch(&c, &junk, Size::S512, SCREEN, &limits, &not_cancelled()), Err(Error::Photo(_))));
    let good = photo(dir.path(), "a.jpg", 300, 200);
    let cancelled = AtomicBool::new(true);
    assert_eq!(ladder::scan(&c, &good, true, &limits, &cancelled), Err(Error::Cancelled));
    assert_eq!(ladder::fetch(&c, &good, Size::S512, SCREEN, &limits, &cancelled).map(|f| f.decoded), Err(Error::Cancelled));
    for size in Size::ALL {
        assert_eq!(c.stats(size).entries, 0, "{size:?}");
    }
}

#[test]
fn encoding_refuses_an_image_that_is_not_rgba() {
    for img in [
        Rgba8 { width: 0, height: 4, data: vec![] },
        Rgba8 { width: 2, height: 2, data: vec![0; 15] },
        Rgba8 { width: u32::MAX, height: u32::MAX, data: vec![0; 4] },
    ] {
        assert!(matches!(ladder::encode_jpeg(&img), Err(Error::Encode(_))));
    }
    let ok = ladder::encode_jpeg(&Rgba8 { width: 3, height: 2, data: vec![200; 24] }).unwrap();
    assert_eq!(dims(&ladder::decode_entry(&ok, &Limits::default()).unwrap()), (3, 2));
    assert!(ladder::decode_entry(b"nope", &Limits::default()).is_err());
}
