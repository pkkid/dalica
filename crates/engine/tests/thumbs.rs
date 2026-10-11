//! The session's thumbnail ladder (`architecture.md` §5, §6.5) on temporary libraries of `scenes`
//! photos: scan-time generation (one decode per photo, sketches applied without undo steps),
//! answers that never wait, on-demand sizes, direction-aware prefetch with stale prefetch
//! cancelled, and an owner thread that never touches the disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::thumbs::Size;
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_thumbs::ladder::Source;
use dalica_thumbs::pack::Cache;
use serde_json::json;

const W: u32 = 640;
const H: u32 = 480;

/// `n` photos `IMG_0001.jpg`… of `W × H` in `dir/<folder>`.
fn photos(dir: &Path, folder: &str, n: usize) -> PathBuf {
    let folder = dir.join(folder);
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, W, H), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    folder
}

fn options(cache: CacheDir, workers: usize) -> Options {
    Options { thumbs: ThumbOptions { cache, workers, ..ThumbOptions::default() } }
}

fn open(library: &Path, options: &Options) -> Session {
    Session::open_with(library, Arc::new(SystemClock), options).unwrap()
}

/// Photo ids in name order.
fn ids(s: &Session) -> Vec<PhotoId> {
    let mut v: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    v.sort();
    v.into_iter().map(|(_, id)| id).collect()
}

fn source(s: &Session, id: PhotoId) -> Source {
    let p = s.library().photo(id).unwrap();
    Source { path: s.library().folder(p.folder).unwrap().path.join(&p.name), file_size: p.size, mtime_ms: p.mtime.as_millis() }
}

fn size_of(t: &Thumb) -> Option<(Size, bool, (u32, u32))> {
    match t {
        Thumb::Image { size, image, covers } => Some((*size, *covers, (image.width, image.height))),
        _ => None,
    }
}

/// `roadmap.md` M4.8: after `wait_idle`, each of 200 photos has a sketch and its 128 and 256
/// entries, made from one decode each; the sketches took no undo step and survive a restart, and a
/// second session on the same cache decodes nothing.
#[test]
fn after_a_scan_every_photo_has_a_sketch_and_its_128_and_256_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let opts = options(CacheDir::At(cache_dir.clone()), 4);
    let mut s = open(&dir.path().join("library"), &opts);
    let folder = photos(dir.path(), "Lisbon", 200);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 200);
    assert_eq!(s.library().sketch_count(), 200, "every photo has its sketch");
    let stats = s.thumbnail_stats();
    assert_eq!((stats.scanned, stats.failed, stats.decodes, stats.jobs), (200, 0, 200, 0), "one decode per photo: {stats:?}");
    assert_eq!(s.undo_stack().len(), 0, "sketches are not undo steps");
    assert_eq!(s.thumbnail_cache_dir(), Some(cache_dir.as_path()));
    let sources: Vec<Source> = ids(&s).into_iter().map(|id| source(&s, id)).collect();
    let revision = s.library().revision();
    s.flush().unwrap();
    drop(s);

    let cache = Cache::open(&cache_dir, dalica_thumbs::pack::Options::default()).unwrap();
    for src in &sources {
        for size in [Size::S128, Size::S256] {
            let bytes = cache.get(size, src.key()).unwrap().unwrap_or_else(|| panic!("{size:?} of {}", src.path.display()));
            let img = dalica_thumbs::ladder::decode_entry(&bytes, &dalica_image::Limits::default()).unwrap();
            assert_eq!(img.width.max(img.height), size.pixels().unwrap());
        }
    }
    drop(cache);

    // Reopened: the sketches came back from the index; the entries are cached, so nothing is decoded.
    let mut s = open(&dir.path().join("library"), &opts);
    assert_eq!(s.library().sketch_count(), 200);
    assert_eq!(s.library().revision(), revision, "sketches did not touch the library revision");
    s.wait_idle();
    let stats = s.thumbnail_stats();
    assert_eq!((stats.scanned, stats.decodes), (200, 0), "{stats:?}");
}

/// `roadmap.md` M4.8: a 300 px cell answers with the 256 entry at once and gets the 512 after its
/// job; a smaller cell is covered by what the scan left in memory.
#[test]
fn a_300_px_cell_answers_with_the_256_entry_then_gets_the_512() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2));
    let folder = photos(dir.path(), "Porto", 6);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = ids(&s)[2];
    assert_eq!(size_of(&s.thumbnail(id, 300)), Some((Size::S256, false, (256, 192))), "the next smaller size, to scale up");
    assert_eq!(s.thumbnails_pending(), 1, "the 512 is queued");
    s.wait_idle();
    assert_eq!(size_of(&s.thumbnail(id, 300)), Some((Size::S512, true, (512, 384))));
    assert_eq!(s.thumbnails_pending(), 0, "nothing more to do");
    assert_eq!(size_of(&s.thumbnail(id, 100)), Some((Size::S128, true, (128, 96))));
    assert_eq!(size_of(&s.thumbnail(id, 200)), Some((Size::S256, true, (256, 192))));
    // The screen-size preview of a photo smaller than the screen is the photo's own size.
    s.set_screen_size(2000);
    let _ = s.thumbnail(id, 1500);
    s.wait_idle();
    assert_eq!(size_of(&s.thumbnail(id, 1500)), Some((Size::Screen, true, (W, H))));
    let stats = s.thumbnail_stats();
    assert_eq!(stats.decodes, 6 + 2, "the 512 and the screen size each decoded the photo once: {stats:?}");
}

/// The JPEG of a `scenes` photo cut after its frame header: the scan reads its size, no decoder
/// can make pixels of it.
fn broken_jpeg() -> Vec<u8> {
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, 64, 48), 80, None).unwrap();
    let sof = bytes.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let len = usize::from(u16::from_be_bytes([bytes[sof + 2], bytes[sof + 3]]));
    let mut cut = bytes[..sof + 2 + len].to_vec();
    cut.extend_from_slice(&[0xff, 0xd9]);
    cut
}

/// `roadmap.md` M4.8: a photo that was never scanned answers "not scanned", and so does a photo
/// whose thumbnail could not be made (with the reason); the others answer with a sketch or better.
#[test]
fn a_never_scanned_photo_answers_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Faro", 3);
    dalica_fs::replace(&folder.join("IMG_0009.jpg"), &broken_jpeg(), Expect::Absent).unwrap();

    // Thumbnails off: nothing is ever scanned.
    let mut off = open(&dir.path().join("off"), &options(CacheDir::Off, 1));
    off.execute("folders.add", &json!({ "path": folder })).unwrap();
    off.wait_idle();
    assert_eq!(off.library().photos().count(), 4);
    for id in ids(&off) {
        assert_eq!(off.thumbnail(id, 128), Thumb::NotScanned);
    }
    assert_eq!(off.thumbnail_cache_dir(), None);
    drop(off);

    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let all = ids(&s);
    let broken = *all.last().unwrap();
    assert_eq!(s.thumbnail(broken, 128), Thumb::NotScanned);
    assert!(s.thumbnail_failure(broken).is_some_and(|why| !why.is_empty()));
    assert_eq!(s.thumbnail_stats().failed, 1);
    assert_eq!(s.thumbnails_pending(), 0, "a failed photo is not retried until it changes");
    for &id in &all[..3] {
        assert!(matches!(s.thumbnail(id, 128), Thumb::Image { covers: true, .. }));
        assert!(s.thumbnail_failure(id).is_none());
    }
    // A photo that does not exist is never scanned either.
    assert_eq!(s.thumbnail(PhotoId::new(9999), 128), Thumb::NotScanned);
    // Hostile sizes answer like any other.
    for need in [0, 1, u32::MAX] {
        assert!(matches!(s.thumbnail(all[0], need), Thumb::Image { .. }));
    }
}

/// Without images in memory a scanned photo draws its sketch: a fresh session on a cache that has
/// the entries answers with the sketch first, then the image once loaded.
#[test]
fn a_scanned_photo_draws_its_sketch_until_an_image_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(CacheDir::At(dir.path().join("cache")), 2);
    let folder = photos(dir.path(), "Braga", 2);
    let mut s = open(&dir.path().join("library"), &opts);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.flush().unwrap();
    drop(s);
    let mut s = open(&dir.path().join("library"), &opts);
    s.wait_idle();
    let id = ids(&s)[0];
    let sketch = *s.library().sketch(id).unwrap();
    assert_eq!(s.thumbnail(id, 300), Thumb::Sketch(sketch));
    s.wait_idle();
    assert_eq!(size_of(&s.thumbnail(id, 300)), Some((Size::S512, true, (512, 384))));
    let stats = s.thumbnail_stats();
    assert_eq!((stats.loads, stats.decodes), (1, 1), "the 256 came from the cache, the 512 from the photo: {stats:?}");
}

/// `roadmap.md` M4.8: prefetch follows the scroll direction, reaches further the faster the view
/// moves, and cancels what the view has moved away from.
#[test]
fn prefetch_follows_the_scroll_direction_and_cancels_stale_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 1));
    let folder = photos(dir.path(), "Evora", 200);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let order = ids(&s);

    let down = s.prefetch_thumbnails(&order, 100..120, 40.0, 1000);
    assert_eq!(down[..3], order[120..123], "ahead first, nearest first");
    let ahead = down.iter().take_while(|id| order[120..].contains(id)).count();
    assert_eq!(ahead, 20 + 30, "a screen plus 0.75 s of travel");
    assert_eq!(down[ahead..], order[90..100].iter().rev().copied().collect::<Vec<_>>()[..], "then half a screen behind");
    assert!(s.thumbnail_stats().prefetching > 40);
    let furthest = down[ahead - 1];

    // The view turns around: prefetch goes the other way, and what lay ahead is cancelled.
    let up = s.prefetch_thumbnails(&order, 100..120, -40.0, 1000);
    assert_eq!(up[..3], [order[99], order[98], order[97]]);
    assert!(up.iter().all(|id| order[50..100].contains(id) || order[120..130].contains(id)));
    let stats = s.thumbnail_stats();
    assert!(stats.cancelled >= 30, "{stats:?}");
    s.wait_idle();
    assert_eq!(size_of(&s.thumbnail(up[0], 1000)).map(|t| (t.0, t.1)), Some((Size::S1024, true)), "prefetched");
    assert_eq!(size_of(&s.thumbnail(furthest, 1000)).map(|t| (t.0, t.1)), Some((Size::S256, false)), "cancelled before it ran");

    // Faster reaches further; standing still looks both ways.
    let fast = s.prefetch_thumbnails(&order, 100..120, 400.0, 1000);
    assert!(fast.iter().filter(|id| order[120..].contains(id)).count() > ahead);
    let still = s.prefetch_thumbnails(&order, 100..120, 0.0, 1000);
    assert_eq!(still[..2], [order[120], order[99]]);
    // Whatever it is given, prefetch stays inside the list.
    assert!(s.prefetch_thumbnails(&[], 0..10, f64::NAN, 128).is_empty());
    assert!(s.prefetch_thumbnails(&order, 500..900, 1e300, 0).iter().all(|id| order.contains(id)));
    assert!(s.prefetch_thumbnails(&[PhotoId::new(77_777)], 0..0, 1.0, 128).len() == 1);
    s.wait_idle();
}

/// `roadmap.md` M4.8: the owner thread makes no `fs` call: scanning, answering, prefetching,
/// polling, waiting, flushing and closing the session (which removes its temporary cache).
#[test]
fn the_owner_thread_never_touches_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Sintra", 30);
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2));
    let temp = s.thumbnail_cache_dir().unwrap().to_path_buf();
    assert!(temp.is_dir());
    let before = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    while s.busy() || s.thumbnails_pending() > 0 {
        s.poll();
        let order = ids(&s);
        for &id in order.iter().take(10) {
            let _ = s.thumbnail(id, 300);
        }
        let _ = s.prefetch_thumbnails(&order, 0..10, 20.0, 300);
        std::thread::yield_now();
    }
    s.wait_idle();
    assert_eq!(s.library().sketch_count(), 30);
    s.flush().unwrap();
    drop(s);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
    assert!(!temp.exists(), "the temporary cache went with the session");
}

/// A photo whose file changed is scanned again when a scan sees the change: a new sketch and new
/// entries, under its new key.
#[test]
fn a_changed_photo_gets_a_new_sketch() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Tavira", 3);
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = ids(&s)[0];
    let old = *s.library().sketch(id).unwrap();
    let path = folder.join("IMG_0001.jpg");
    let other = dalica_scenes::jpeg(&dalica_scenes::Scene::new(4242, H, W), 80, None).unwrap();
    let identity = dalica_fs::identity(&path).unwrap().unwrap();
    dalica_fs::replace(&path, &other, Expect::Unchanged(identity)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let new = *s.library().sketch(id).unwrap();
    assert_ne!(new, old);
    assert_eq!(size_of(&s.thumbnail(id, 200)), Some((Size::S256, true, (192, 256))), "the new picture, upright");
    assert_eq!(s.undo_stack().len(), 0);
}

/// Two sessions asking for one cache folder: the second cannot have it (it is locked), says so,
/// and keeps its thumbnails in a temporary folder instead.
#[test]
fn a_cache_in_use_falls_back_to_a_temporary_one() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(CacheDir::At(dir.path().join("cache")), 1);
    let first = open(&dir.path().join("one"), &opts);
    let mut second = open(&dir.path().join("two"), &opts);
    assert_eq!(first.thumbnail_cache_dir(), Some(dir.path().join("cache").as_path()));
    assert_ne!(second.thumbnail_cache_dir(), first.thumbnail_cache_dir());
    assert!(second.notices().iter().any(|n| n.contains("temporary")), "{:?}", second.notices());
    let folder = photos(dir.path(), "Lagos", 2);
    second.execute("folders.add", &json!({ "path": folder })).unwrap();
    second.wait_idle();
    assert_eq!(second.library().sketch_count(), 2);
}
