//! The thumbnail cache's upkeep and rendering options in a session (`roadmap.md` M5.16,
//! `architecture.md` §6): pruning at start stops at 80 % of the cache size and keeps the entries of
//! photos whose folder cannot be reached (never made again while it is away); pruning also runs
//! when scans push the cache past its size; entries of photos that leave are made dead; a broken
//! file answers its `formats.md` §1 reason once and is tried again after it is rewritten; View →
//! Use Color Management changes a tagged photo's thumbnail and its cache key, is saved, and a
//! library saved with it off makes its first thumbnails that way; Folder → Refresh Thumbnails makes
//! a section again; a photo on screen shows its embedded thumbnail before its scan finishes. The
//! owner thread makes no `fs` call throughout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::thumbs::Size;
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_scenes::icc::{Space, Version};
use dalica_scenes::{Exif, Scene};
use dalica_thumbs::ladder::{Render, Source};
use dalica_thumbs::pack::{Cache, Key};
use serde_json::json;

const W: u32 = 640;
const H: u32 = 480;

fn write(path: &Path, bytes: &[u8]) {
    dalica_fs::replace(path, bytes, Expect::Absent).unwrap();
}

/// `n` photos `IMG_0001.jpg`… of `W × H` in `dir/<folder>`, the first numbered `from`.
fn photos(dir: &Path, folder: &str, from: usize, n: usize) -> PathBuf {
    let folder = dir.join(folder);
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in from..from + n {
        let bytes = dalica_scenes::jpeg(&Scene::new(i as u64, W, H), 80, None).unwrap();
        write(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes);
    }
    folder
}

fn options(cache: CacheDir, workers: usize, budget: Option<u64>) -> Options {
    Options { thumbs: ThumbOptions { cache, workers, budget, ..ThumbOptions::default() } }
}

fn open(library: &Path, options: &Options) -> Session {
    Session::open_with(library, Arc::new(SystemClock), options).unwrap()
}

fn add(s: &mut Session, folder: &Path) {
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
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

fn image(t: &Thumb) -> Option<Arc<dalica_thumbs::Rgba8>> {
    match t {
        Thumb::Image { image, .. } => Some(Arc::clone(image)),
        _ => None,
    }
}

/// The image a cell of `need` pixels draws once what it asks for has arrived (a scan that finds its
/// entries cached answers without images: the cell then fetches them).
fn settled(s: &mut Session, id: PhotoId, need: u32) -> Arc<dalica_thumbs::Rgba8> {
    for _ in 0..4 {
        if let Some(i) = image(&s.thumbnail(id, need)) {
            return i;
        }
        s.wait_idle();
    }
    panic!("photo {id} has no image for {need} px");
}

/// The mean absolute difference of two images of the same size, per channel.
fn difference(a: &dalica_thumbs::Rgba8, b: &dalica_thumbs::Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let sum: u64 = a.data.iter().zip(&b.data).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    sum as f64 / a.data.len() as f64
}

/// Pruning at start, with a folder "unplugged" (renamed away): the cache ends at or just under
/// 80 % of its size, every entry of the unreachable folder's photos is still there, and nothing is
/// decoded or marked failed.
#[test]
fn pruning_at_start_stops_at_80_percent_and_keeps_offline_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let online = photos(dir.path(), "Online", 0, 24);
    let offline = photos(dir.path(), "Away", 100, 8);
    let library = dir.path().join("library");
    // Every photo with its 128, 256 and 512 entries.
    let mut s = open(&library, &options(CacheDir::At(cache_dir.clone()), 3, Some(u64::MAX)));
    // The offline folder first: its entries are the oldest, the first pruning would take.
    add(&mut s, &offline);
    add(&mut s, &online);
    let mut by_folder: Vec<PhotoId> = ids(&s);
    by_folder.sort_by_key(|id| std::cmp::Reverse(s.library().photo(*id).unwrap().name.clone()));
    for id in by_folder {
        let _ = s.thumbnail(id, 400);
    }
    s.wait_idle();
    let away: Vec<Key> =
        ids(&s).into_iter().filter(|id| s.library().photo(*id).unwrap().name.as_str() > "IMG_0100").map(|id| source(&s, id).key()).collect();
    assert_eq!(away.len(), 8);
    s.flush().unwrap();
    drop(s);
    let full = Cache::open(&cache_dir, dalica_thumbs::pack::Options::default()).unwrap().disk_bytes();

    // The volume goes away; the cache size is now 90 % of what it holds.
    std::fs::rename(&offline, dir.path().join("Away (unplugged)")).unwrap();
    let budget = full * 9 / 10;
    let mut s = open(&library, &options(CacheDir::At(cache_dir.clone()), 3, Some(budget)));
    s.wait_idle();
    let stats = s.thumbnail_stats();
    let target = budget * 8 / 10;
    assert!(stats.prunes >= 1 && stats.pruned > 0, "pruned at start: {stats:?}");
    assert!(stats.disk_bytes <= target, "{} > the 80 % target {target}", stats.disk_bytes);
    assert!(stats.disk_bytes + 64 * 1024 > target, "stops at the target, not far below: {} for {target}", stats.disk_bytes);
    assert!(stats.kept > 0, "{stats:?}");
    assert_eq!((stats.decodes, stats.failed), (0, 0), "nothing made again, nothing failed: {stats:?}");
    s.flush().unwrap();
    drop(s);
    let cache = Cache::open(&cache_dir, dalica_thumbs::pack::Options::default()).unwrap();
    for key in &away {
        for size in [Size::S128, Size::S256, Size::S512] {
            assert!(cache.contains(size, *key), "an offline photo lost its {size:?} entry");
        }
    }
}

/// Scans that push the cache past its size start pruning on their own; the cache ends within it.
#[test]
fn scans_past_the_cache_size_start_pruning() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Many", 0, 40);
    // About a third of what 40 photos' 128 and 256 entries take.
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, Some(150_000)));
    add(&mut s, &folder);
    let stats = s.thumbnail_stats();
    assert!(stats.prunes >= 1, "{stats:?}");
    assert!(stats.disk_bytes <= 150_000, "{stats:?}");
    assert_eq!(s.library().sketch_count(), 40, "every photo still has its sketch");
}

/// Photos that leave the library have their entries made dead; another library's entries in the
/// same cache are never touched.
#[test]
fn entries_of_removed_photos_are_made_dead_and_other_libraries_kept() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    // Another library used the cache first.
    let other = photos(dir.path(), "Other", 50, 3);
    let mut o = open(&dir.path().join("other-library"), &options(CacheDir::At(cache_dir.clone()), 2, None));
    add(&mut o, &other);
    let theirs: Vec<Key> = ids(&o).into_iter().map(|id| source(&o, id).key()).collect();
    o.flush().unwrap();
    drop(o);

    let folder = photos(dir.path(), "Mine", 0, 3);
    let mut s = open(&dir.path().join("library"), &options(CacheDir::At(cache_dir.clone()), 2, None));
    add(&mut s, &folder);
    let gone = ids(&s)[1];
    let gone_key = source(&s, gone).key();
    std::fs::remove_file(folder.join("IMG_0002.jpg")).unwrap();
    s.start_verification();
    s.wait_idle();
    assert!(s.library().photo(gone).is_none());
    assert_eq!(s.thumbnail_stats().forgotten, 2, "its 128 and 256 entries");
    s.flush().unwrap();
    drop(s);
    let cache = Cache::open(&cache_dir, dalica_thumbs::pack::Options::default()).unwrap();
    assert!(!cache.contains(Size::S128, gone_key) && !cache.contains(Size::S256, gone_key));
    for key in theirs {
        assert!(cache.contains(Size::S128, key) && cache.contains(Size::S256, key), "another library's entry went");
    }
}

/// The first SOF segment cut after its header: the header reads, the pixels do not.
fn broken_jpeg(seed: u64) -> Vec<u8> {
    let bytes = dalica_scenes::jpeg(&Scene::new(seed, W, H), 80, None).unwrap();
    let sof = bytes.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let len = usize::from(u16::from_be_bytes([bytes[sof + 2], bytes[sof + 3]]));
    let mut cut = bytes[..sof + 2 + len].to_vec();
    cut.extend_from_slice(&[0xff, 0xd9]);
    cut
}

#[test]
fn a_broken_file_gets_its_reason_once_and_is_retried_after_a_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Mixed", 0, 2);
    write(&folder.join("IMG_0009.jpg"), &broken_jpeg(9));
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, None));
    add(&mut s, &folder);
    let broken = *ids(&s).last().unwrap();
    assert_eq!(s.thumbnail_failure(broken), Some("Damaged file"), "the formats.md §1 tile text");
    let decodes = s.thumbnail_stats().decodes;
    // Asked again and again: not tried again.
    for _ in 0..3 {
        assert_eq!(s.thumbnail(broken, 128), Thumb::NotScanned);
        let _ = s.prefetch_thumbnails(&ids(&s), 0..3, 10.0, 128);
        assert_eq!(s.thumbnails_pending(), 0, "a failed photo is not retried until it changes");
        s.wait_idle();
    }
    assert_eq!(s.thumbnail_stats().decodes, decodes);
    assert_eq!(s.thumbnail_stats().failed, 1);

    // Rewritten whole: the next scan sees the change, and it is tried again.
    std::thread::sleep(Duration::from_millis(20));
    let good = dalica_scenes::jpeg(&Scene::new(9, W + 8, H), 80, None).unwrap();
    let id = dalica_fs::identity(&folder.join("IMG_0009.jpg")).unwrap().unwrap();
    dalica_fs::replace(&folder.join("IMG_0009.jpg"), &good, Expect::Unchanged(id)).unwrap();
    s.start_verification();
    s.wait_idle();
    assert_eq!(s.thumbnail_failure(broken), None);
    assert!(image(&s.thumbnail(broken, 128)).is_some());
    assert_eq!(s.thumbnail_stats().failed, 0);
}

/// A ProPhoto-tagged photo: with colour management its thumbnail is converted to sRGB, without it
/// the pixels are taken as sRGB. The two renderings differ, live under different cache keys, and
/// switching back reads the first again from the cache. The setting is saved; a library saved with
/// it off makes its first thumbnails that way.
#[test]
fn colour_management_changes_a_tagged_photos_thumbnail_and_its_cache_key() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Tagged");
    dalica_fs::create_dir_all(&folder).unwrap();
    let icc = dalica_scenes::icc::rgb(Space::ProPhoto, Version::V4);
    write(&folder.join("IMG_0001.jpg"), &dalica_scenes::formats::jpeg_icc(&Scene::new(31, W, H), 92, None, &icc).unwrap());
    let cache_dir = dir.path().join("cache");
    let library = dir.path().join("library");
    let mut s = open(&library, &options(CacheDir::At(cache_dir.clone()), 2, None));
    add(&mut s, &folder);
    let id = ids(&s)[0];
    let managed = image(&s.thumbnail(id, 128)).expect("the 128 entry in memory after the scan");
    let src = source(&s, id);

    let before = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(s.execute("view.colorManagement", &json!({})).unwrap()["on"], json!(false));
    assert!(!s.color_management());
    // The image in memory was made the other way: the cell draws its sketch while it is made again.
    assert!(matches!(s.thumbnail(id, 128), Thumb::Sketch(_)));
    let plain = settled(&mut s, id, 128);
    let d = difference(&managed, &plain);
    assert!(d > 3.0, "ProPhoto pixels converted and not converted differ: {d:.2}");
    let decodes = s.thumbnail_stats().decodes;
    // Back on: read from the cache, not decoded.
    s.execute("view.colorManagement", &json!({ "on": true })).unwrap();
    assert_eq!(*settled(&mut s, id, 128), *managed, "the managed rendering again");
    assert_eq!(s.thumbnail_stats().decodes, decodes, "from the cache");
    s.execute("view.colorManagement", &json!({ "on": false })).unwrap();
    s.flush().unwrap();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
    drop(s);

    let off = src.key_for(&Render { color_managed: false });
    assert_ne!(off, src.key());
    {
        let cache = Cache::open(&cache_dir, dalica_thumbs::pack::Options::default()).unwrap();
        for key in [src.key(), off] {
            assert!(cache.contains(Size::S128, key) && cache.contains(Size::S256, key));
        }
    }

    // Saved off: a session on a new cache makes its first thumbnails without colour management.
    let fresh = dir.path().join("fresh-cache");
    let mut s = open(&library, &options(CacheDir::At(fresh.clone()), 2, None));
    assert!(!s.color_management(), "read from settings.json");
    s.wait_idle();
    assert_eq!(s.thumbnail_stats().scanned, 1);
    drop(s);
    let cache = Cache::open(&fresh, dalica_thumbs::pack::Options::default()).unwrap();
    assert!(cache.contains(Size::S128, off), "the first sync rendered without colour management");
    assert!(!cache.contains(Size::S128, src.key()), "nothing was made with it on");
}

/// Folder → Refresh Thumbnails: every photo of the section is decoded again (new entries, same
/// keys), the other folder is left alone.
#[test]
fn refresh_regenerates_a_section() {
    let dir = tempfile::tempdir().unwrap();
    let a = photos(dir.path(), "A", 0, 4);
    let b = photos(dir.path(), "B", 10, 3);
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, None));
    add(&mut s, &a);
    add(&mut s, &b);
    let decodes = s.thumbnail_stats().decodes;
    assert_eq!(decodes, 7);
    let folder_a = s.library().folder_by_path(&a).unwrap();
    let r = s.execute("section.refreshThumbs", &json!({ "section": format!("folder:{folder_a}") })).unwrap();
    assert_eq!(r["photos"], json!(4));
    s.wait_idle();
    let stats = s.thumbnail_stats();
    assert_eq!(stats.decodes - decodes, 4, "the four photos of A, and only them: {stats:?}");
    assert_eq!((stats.scanned, stats.failed), (7, 0));
    for id in ids(&s) {
        assert!(image(&s.thumbnail(id, 128)).is_some());
    }
}

/// A gate scan jobs wait at (through `Session::set_thumbnail_scan_hook`) until it is opened, or
/// for 60 s at most, so a failing test never hangs.
#[derive(Clone, Default)]
struct Gate(Arc<(std::sync::Mutex<(bool, usize)>, std::sync::Condvar)>);

impl Gate {
    fn hook(&self) -> dalica_engine::thumbs::ScanHook {
        let gate = self.clone();
        Arc::new(move |_| {
            let (lock, cv) = &*gate.0;
            let mut g = lock.lock().unwrap();
            g.1 += 1;
            cv.notify_all();
            let _ = cv.wait_timeout_while(g, Duration::from_secs(60), |g| !g.0).unwrap();
        })
    }

    /// Scan jobs that reached the gate.
    fn held(&self) -> usize {
        self.0.0.lock().unwrap().1
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap().0 = true;
        cv.notify_all();
    }
}

/// A photo on screen whose scan is still running shows its embedded thumbnail first, when it
/// covers the cell; the scan's own downscale replaces it. The scan is held at a gate until the
/// embedded thumbnail has been shown, so the order does not depend on how fast the machine is.
#[test]
fn a_new_photo_on_screen_shows_its_embedded_thumbnail_first() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Camera");
    dalica_fs::create_dir_all(&folder).unwrap();
    let exif = Exif { thumbnail: Some(160), ..Exif::default() };
    write(&folder.join("IMG_0001.jpg"), &dalica_scenes::jpeg(&Scene::new(5, 2400, 1600), 90, Some(&exif)).unwrap());
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, None));
    let gate = Gate::default();
    s.set_thumbnail_scan_hook(Some(gate.hook()));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    let start = Instant::now();
    let first = loop {
        s.poll();
        let id = s.library().photos().next().map(|p| p.id);
        if let Some(img) = id.and_then(|id| image(&s.thumbnail(id, 144))) {
            break img;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "no thumbnail");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(first.width == 160 && (106..=107).contains(&first.height), "the embedded thumbnail came first: {} × {}", first.width, first.height);
    let stats = s.thumbnail_stats();
    assert_eq!((stats.quick, stats.scanned, stats.decodes), (1, 0, 0), "shown while the scan is still held: {stats:?}");
    assert_eq!(gate.held(), 1, "the photo's scan was queued and is waiting");
    gate.open();
    s.wait_idle();
    let id = ids(&s)[0];
    match s.thumbnail(id, 144) {
        Thumb::Image { size, image, covers } => assert_eq!((size, covers, image.width), (Size::S256, true, 256), "replaced by the ladder's"),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.thumbnail_stats().decodes, 1);
}

/// A cell whose embedded thumbnail is smaller than it keeps its sketch until the scan is done:
/// the embedded image is shown only when it covers the cell.
#[test]
fn an_embedded_thumbnail_too_small_for_the_cell_is_not_shown() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Camera");
    dalica_fs::create_dir_all(&folder).unwrap();
    let exif = Exif { thumbnail: Some(160), ..Exif::default() };
    write(&folder.join("IMG_0001.jpg"), &dalica_scenes::jpeg(&Scene::new(6, 1200, 800), 90, Some(&exif)).unwrap());
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, None));
    let gate = Gate::default();
    s.set_thumbnail_scan_hook(Some(gate.hook()));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    let start = Instant::now();
    // Until the cell has asked and the embedded job has answered (it runs while the scan is
    // held: then only the scan is pending).
    let mut asked = false;
    while !(asked && gate.held() == 1 && s.thumbnails_pending() == 1) {
        s.poll();
        let id = s.library().photos().next().map(|p| p.id);
        if let Some(id) = id {
            assert!(image(&s.thumbnail(id, 300)).is_none(), "nothing covers a 300 px cell yet");
            asked = true;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "the embedded job never answered");
        std::thread::sleep(Duration::from_millis(1));
    }
    let id = ids(&s)[0];
    assert!(image(&s.thumbnail(id, 300)).is_none(), "a 160 px embedded thumbnail does not cover 300 px");
    gate.open();
    s.wait_idle();
    let _ = s.thumbnail(id, 300);
    s.wait_idle();
    assert!(image(&s.thumbnail(id, 300)).is_some());
}

/// A rebuild gives the same files new ids: their entries are still theirs, never made dead, and
/// nothing is decoded again.
#[test]
fn a_rebuild_keeps_the_entries_of_the_same_files() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), "Kept", 0, 5);
    let mut s = open(&dir.path().join("library"), &options(CacheDir::Temp, 2, None));
    add(&mut s, &folder);
    let decodes = s.thumbnail_stats().decodes;
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let stats = s.thumbnail_stats();
    assert_eq!(s.library().photos().count(), 5);
    assert_eq!((stats.forgotten, stats.decodes), (0, decodes), "{stats:?}");
}
