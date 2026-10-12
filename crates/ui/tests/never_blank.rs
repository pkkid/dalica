//! Never-blank thumbnails on screen (M4.12, `architecture.md` §6.5): every cell draws the best the
//! session has on the frame it appears. The tests scroll the light table at the speed thumb's top
//! speed and check the thumbnails' counters (`ui.inspect` → `perf`), summed over every frame:
//!
//! - a **scanned 10 k-photo** synthetic library, reopened with its thumbnail cache on disk and
//!   nothing in memory: no placeholder cell on any frame, textures within the cap and uploads
//!   within the per-frame cap, nothing read from disk on the UI thread; once the view stops, every
//!   visible cell draws its covering size, and back at the top the light table is pixel for pixel
//!   what it was before the scroll;
//! - an **index-only 100 k-photo** library (no files, thumbnails off): every cell draws its sketch,
//!   no photo is marked as failed, and nothing is pending.
//!
//! The scroll is driven by the wheel (`ui.scroll`: a step every other frame, so on average the
//! thumb's top speed), which does not depend on the scroll bar's controls.
//!
//! The scanned 10 k run does not run on Windows (maintainer decision at M4.R, 2026-10-11): writing
//! and scanning 10 000 files took 180 s on the Windows CI runner against 52 s on Linux, and the code
//! it checks does not depend on the OS; Windows file and scan paths are covered by the `scan`,
//! `pack` and M1 acceptance tests. Linux and macOS run it.
#![cfg_attr(windows, allow(dead_code, unused_imports))]

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use dalica_core::{Clock, SystemClock};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::panels::thumbnails::{Counts, MAX_TEXTURES, Totals, UPLOADS_PER_FRAME};
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::json;

const FRAMES: usize = 60 * 120;
/// The speed thumb's top speed: screen heights per second (`ui-layout.md` §5.6).
const TOP_SCREENS_PER_S: f64 = 40.0;

fn clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

fn counts(d: &Driver) -> Counts {
    d.app().panels().unwrap().thumbnails.counts()
}

fn totals(d: &Driver) -> Totals {
    d.app().panels().unwrap().thumbnails.totals()
}

/// What changed in the totals from `a` to `b` (cells summed, maxima of `b`).
fn since(a: &Totals, b: &Totals) -> Totals {
    Totals {
        frames: b.frames - a.frames,
        covered_cells: b.covered_cells - a.covered_cells,
        upscaled_cells: b.upscaled_cells - a.upscaled_cells,
        sketch_cells: b.sketch_cells - a.sketch_cells,
        placeholder_cells: b.placeholder_cells - a.placeholder_cells,
        failed_cells: b.failed_cells - a.failed_cells,
        uploads: b.uploads - a.uploads,
        ..*b
    }
}

/// The light table's offset, content height and view height in the last frame.
fn view(d: &Driver) -> (f64, f64, f64) {
    let info = d.app().panels().unwrap().light_table.frame_info().clone();
    (info.offset, info.height, f64::from(info.area.unwrap().height()))
}

/// Wheels the light table at `screens_per_s` (one step every other frame) until the view has
/// moved `screens` screens or reached the bottom. Returns the frames run.
fn wheel(d: &mut Driver, screens_per_s: f64, screens: f64) -> usize {
    let (start, height, view_h) = view(d);
    let step = (2.0 * screens_per_s * view_h / 60.0) as f32;
    let goal = (start + screens * view_h).min(height - view_h - 1.0);
    let mut frames = 0;
    while view(d).0 < goal {
        d.scroll(Some("region:lightTable"), vec2(0.0, -step)).unwrap();
        frames += 2;
        assert!(frames < 60 * 600, "the view did not move on");
    }
    frames
}

/// Writes the last frame to `target/test-output/ui/<name>.png`, to look at.
fn save(d: &Driver, name: &str) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// The light table's pixels in the last frame.
fn light_table_pixels(d: &Driver) -> Vec<u8> {
    let shot = d.screenshot().unwrap();
    let r = d.app().panels().unwrap().light_table.frame_info().area.unwrap();
    let mut out = Vec::new();
    for y in r.min.y as u32..r.max.y as u32 {
        for x in r.min.x as u32..r.max.x as u32 {
            out.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    out
}

/// A scanned synthetic library in `dir` whose thumbnails were made into `cache`, reopened with
/// that cache: sketches in the index, 128 and 256 entries on disk, nothing in memory.
fn scanned(dir: &Path, cache: &Path, spec: &Spec) -> Session {
    let started = Instant::now();
    dalica_engine::thumbs::set_process_cache_dir(Some(cache.to_path_buf()));
    let first = synthetic::open_scanned(dir, spec, synthetic::DEFAULT_FILE_SIDE, clock());
    dalica_engine::thumbs::set_process_cache_dir(None);
    let first = first.unwrap();
    let stats = first.thumbnail_stats();
    assert_eq!((stats.scanned, stats.failed), (spec.photos, 0), "every photo scanned: {stats:?}");
    assert_eq!(first.thumbnail_cache_dir(), Some(cache));
    drop(first);
    println!("{} photos written and scanned in {:.1} s", spec.photos, started.elapsed().as_secs_f64());
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), ..ThumbOptions::default() } };
    let session = Session::open_with(&synthetic::library_dir(dir), clock(), &options).unwrap();
    assert!(session.library().photos().all(|p| session.library().sketch(p.id).is_some()), "every photo sketched in the index");
    session
}

/// The M4.12 done-when (not on Windows: see the module comment).
#[test]
#[cfg(not(windows))]
fn a_scanned_10k_library_scrolled_at_top_speed_never_shows_a_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let session = scanned(&dir.path().join("lib"), &dir.path().join("cache"), &Spec::new(10_000, 100, synthetic::SEED));

    let ui_thread = dalica_fs::mark_ui_thread();
    let refused_before = dalica_fs::guard::refused();
    let mut d = Driver::new(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let c = counts(&d);
    assert!(c.covered_cells > 20, "{c:?}");
    assert_eq!((c.upscaled_cells, c.sketch_cells, c.placeholder_cells, c.failed_cells), (0, 0, 0, 0), "settled: every cell covered");
    let before = light_table_pixels(&d);

    // Top to bottom at the thumb's top speed.
    let started = Instant::now();
    let t0 = totals(&d);
    let frames = wheel(&mut d, TOP_SCREENS_PER_S, f64::INFINITY);
    let t = since(&t0, &totals(&d));
    let elapsed = started.elapsed();
    assert!(frames > 100, "the scroll is long enough to mean something: {frames} frames");
    assert_eq!(t.frames as usize, frames);
    assert_eq!((t.placeholder_cells, t.failed_cells), (0, 0), "no placeholder on any frame: {t:?}");
    assert!(t.max_textures as usize <= MAX_TEXTURES, "{t:?}");
    assert!(t.max_uploads as usize <= UPLOADS_PER_FRAME, "{t:?}");
    assert!(t.sketch_cells > 0, "the scroll outran the thumbnails, so sketches were drawn: {t:?}");
    println!(
        "{frames} frames at {TOP_SCREENS_PER_S} screens/s ({:.1} ms/frame): {} cells covered, {} upscaled, {} sketched, 0 \
         placeholders; at most {} textures ({:.1} MiB) and {} uploads a frame",
        elapsed.as_secs_f64() * 1000.0 / frames as f64,
        t.covered_cells,
        t.upscaled_cells,
        t.sketch_cells,
        t.max_textures,
        t.max_texture_bytes as f64 / (1 << 20) as f64,
        t.max_uploads,
    );

    let perf = d.perf().report();
    println!("UI time over the last {} frames: p95 {:.2} ms, max {:.2} ms", perf.frames, perf.frame_p95_ms, perf.max_frame_ms);
    save(&d, "never-blank-top-speed-1400x900");

    // Stopped: every visible cell gets its covering size.
    d.settle(FRAMES).unwrap();
    save(&d, "never-blank-stopped-1400x900");
    let c = counts(&d);
    assert!(c.covered_cells > 0, "{c:?}");
    assert_eq!((c.upscaled_cells, c.sketch_cells, c.placeholder_cells), (0, 0, 0), "stopped at the bottom: {c:?}");

    // Back up at a browsing speed (a wheel's 2 screens a second, through the middle of the
    // library): prefetch and uploads keep most cells sharp, and still nothing is a placeholder.
    d.app_mut().state_mut().scroll = Default::default();
    d.settle(FRAMES).unwrap();
    let t0 = totals(&d);
    let frames = wheel(&mut d, 2.0, 20.0);
    let t = since(&t0, &totals(&d));
    assert_eq!((t.placeholder_cells, t.failed_cells), (0, 0), "{t:?}");
    assert!(t.max_textures as usize <= MAX_TEXTURES && t.max_uploads as usize <= UPLOADS_PER_FRAME, "{t:?}");
    assert!(t.covered_cells > 0, "{t:?}");
    println!(
        "{frames} frames at 2 screens/s: {} cells covered, {} upscaled, {} sketched; at most {} textures and {} uploads a frame",
        t.covered_cells, t.upscaled_cells, t.sketch_cells, t.max_textures, t.max_uploads
    );

    // Back at the top (eased there through the whole library), the light table is what it was.
    let t0 = totals(&d);
    d.app_mut().state_mut().scroll = Default::default();
    d.settle(FRAMES).unwrap();
    assert_eq!(view(&d).0, 0.0);
    assert_eq!(since(&t0, &totals(&d)).placeholder_cells, 0);
    let c = counts(&d);
    assert_eq!((c.upscaled_cells, c.sketch_cells, c.placeholder_cells), (0, 0, 0), "back at the top: {c:?}");
    assert!(before == light_table_pixels(&d), "the light table at the top differs from before the scroll");

    // Small cells in a large window (some 160 on screen at the top, more further down): the textures stay within the cap.
    d.app_mut().state_mut().thumb_size = dalica_ui::state::THUMB_MIN;
    d.resize(2000, 1400);
    d.settle(FRAMES).unwrap();
    let t0 = totals(&d);
    let on_screen = counts(&d).covered_cells;
    let frames = wheel(&mut d, 2.0, 12.0);
    let t = since(&t0, &totals(&d));
    assert_eq!((t.placeholder_cells, t.failed_cells), (0, 0), "{t:?}");
    assert!(on_screen > 100 && (on_screen as usize) < MAX_TEXTURES, "{on_screen} cells on screen");
    assert!(t.max_textures as usize <= MAX_TEXTURES && t.max_textures as usize > MAX_TEXTURES * 9 / 10, "the cap is reached, not passed: {t:?}");
    println!(
        "{frames} frames of {on_screen} small cells at 2 screens/s: at most {} textures ({:.1} MiB)",
        t.max_textures,
        t.max_texture_bytes as f64 / (1 << 20) as f64
    );

    assert_eq!(dalica_fs::guard::refused(), refused_before, "file access was attempted on the UI thread");
    drop(ui_thread);
}

/// The index-only library has no files: thumbnails are off, every cell draws its sketch and no
/// photo is marked as failed (which would draw the "!" tile).
#[test]
fn an_index_only_100k_library_draws_sketches_and_fails_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let session = synthetic::open(dir.path(), &Spec::PHOTOS_100K, clock()).unwrap();
    assert_eq!(session.thumbnail_cache_dir(), None, "thumbnails are off");
    let mut d = Driver::new(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let t0 = totals(&d);
    // A stretch at the top speed, then a jump near the far end and another stretch.
    let mut frames = wheel(&mut d, TOP_SCREENS_PER_S, 100.0);
    let (_, height, view_h) = view(&d);
    while view(&d).0 < height * 0.9 - view_h {
        d.scroll(Some("region:lightTable"), vec2(0.0, -900_000.0)).unwrap();
        frames += 2;
    }
    frames += wheel(&mut d, TOP_SCREENS_PER_S, 50.0);
    assert!(view(&d).0 > height * 0.9 - view_h, "the jump reached the far end");
    d.resize(900, 600);
    d.settle(FRAMES).unwrap();
    save(&d, "never-blank-sketches-900x600");
    let t = since(&t0, &totals(&d));
    assert!(t.sketch_cells > 0, "{t:?}");
    assert_eq!((t.placeholder_cells, t.failed_cells, t.covered_cells, t.upscaled_cells, t.max_textures), (0, 0, 0, 0, 0), "{t:?}");
    let stats = d.app().session().unwrap().thumbnail_stats();
    assert_eq!((stats.failed, stats.jobs, stats.decodes), (0, 0, 0), "{stats:?}");
    assert_eq!(d.app().panels().unwrap().thumbnails.pending(), 0);
    // The last frame's counters, as `ui.inspect` reports them.
    let control = Control::new(Config::default());
    let perf = serve(&control, &mut Connection::new(), &mut d, &Request::new("ui.inspect", json!({}))).unwrap()["perf"].clone();
    assert!(perf["sketchCells"].as_u64().unwrap() > 0, "{perf}");
    for zero in ["placeholderCells", "upscaledCells", "coveredCells", "failedCells", "thumbTextures", "thumbUploads"] {
        assert_eq!(perf[zero], 0, "{zero}: {perf}");
    }
    println!("{frames} frames over 100 k photos: {} cells, all sketches; no failures", t.sketch_cells);
}
