//! First thumbnails after adding a folder, the full scan, and the scanned 100 k-photo light table
//! at the speed thumb's top speed (`development.md` §8, `roadmap.md` M5.16). Run it with
//!
//! ```sh
//! cargo xtask bench first_thumbnails                       # release: 1 k and 10 k JPEGs, then the scanned 100 k pass
//! cargo bench -p dalica-engine --bench first_thumbnails -- --photos 1000 --scroll 0 --json
//! cargo bench -p dalica-engine --bench first_thumbnails -- --photos 0 --dir /fast/disk --scroll 100000
//! ```
//!
//! **First thumbnails** (rows `first thumbnails <n>`). `<n>` synthetic JPEGs (`--side` pixels on
//! their long side, 1024 by default, each with a 160-pixel EXIF thumbnail as cameras write;
//! `--no-embedded` leaves it out) in folders of about 100 are written first (not timed). A new
//! library on an empty thumbnail cache then runs `folders.add` on them, and frames run at 60 per
//! second as the app's would: each one calls [`Session::poll`] and asks for the thumbnails of the
//! first screenful, as the light table does at 1400 × 900 with the default 144-point cells
//! ([`SCREENFUL`] cells: the newest folder's first photos, then the next folder's). Reported:
//! - `first_ms`: from `folders.add` to the frame where every cell of the first screenful has an
//!   image (budget 1 000 ms: `development.md` §8 for 1 000 JPEGs, the roadmap's for 10 000);
//! - `scan_s` and `scan_photos_per_s`: until the scan job ends (every photo in the library);
//! - `thumbs_s` and `thumbs_photos_per_s`: until every photo has its 128 and 256 entries;
//! - `frame_p50_ms`, `frame_p95_ms`, `frame_max_ms`: the owner thread's time per frame
//!   (`poll` plus the requests), by nearest rank, while all that runs.
//!
//! **The scanned pass** (row `scanned scroll <n>`, `--scroll`, 100 000 by default): `<n>` small
//! JPEGs (320 pixels) are written and scanned into a thumbnail cache (`scan_s`, photos per second),
//! then the library is opened again with nothing in memory and the light table's requests are
//! replayed at the top speed of the speed thumb (40 screen heights a second, `ui-layout.md` §5.6)
//! from the top to the bottom, 60 frames a second: the same cells, sections and prefetch calls as
//! the light table at 1400 × 900 (7 columns of 144-point cells 156 apart, rows 166 apart, a 96-point
//! header and 24 points before each section). At that speed the light table asks only for
//! prefetch, so each visible cell draws what is in memory, else its sketch: a cell with neither is
//! a **placeholder** (must be 0), and `frame_p95_ms` (the owner thread's time per frame: poll,
//! prefetch, every cell's answer) must stay within the 8 ms frame budget. The UI's own layout and
//! drawing are measured by `library_bench` and `tests/acceptance/m4_thumb.rs`.
//!
//! Files go under `--dir` (default: this target directory's temporary folder) and are removed
//! afterwards. The bench exits non-zero when a budget is missed. Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass: 100 photos and a 300-photo scroll.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{FolderId, PhotoId, SystemClock};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use serde_json::{Value, json};

/// Cells of the first screenful at 1400 × 900: 7 columns of 144-point cells, 4 rows.
const SCREENFUL: usize = 28;
/// The light table's cell edge in device pixels at 1× (the default thumbnail size).
const NEED: u32 = 144;
/// The budget for the first screenful (`development.md` §8), and the frame budget.
const FIRST_BUDGET_MS: f64 = 1000.0;
const FRAME_BUDGET_MS: f64 = 8.0;
/// Frames per second the app runs at.
const FPS: f64 = 60.0;
/// The light table at 1400 × 900: columns, cell pitch, row pitch, header, gap before a section,
/// the view's height (points).
const COLUMNS: usize = 7;
const ROW: f64 = 166.0;
const HEADER: f64 = 96.0;
const GAP: f64 = 24.0;
const VIEW_H: f64 = 680.0;
/// The speed thumb's top speed: screen heights per second.
const TOP_SCREENS_PER_S: f64 = 40.0;
/// Photos per folder on average.
const PER_FOLDER: usize = 100;
/// The longest a case may run.
const TIMEOUT: Duration = Duration::from_secs(1800);

struct Args {
    photos: Vec<usize>,
    scroll: usize,
    side: u32,
    embedded: bool,
    dir: PathBuf,
    json: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { vec![1_000, 10_000] } else { vec![100] },
        scroll: if full { 100_000 } else { 300 },
        side: if full { 1024 } else { 320 },
        embedded: true,
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.split(',').map(|n| n.trim().parse().map_err(|e| format!("--photos: {e}"))).collect::<Result<_, _>>()?,
            "--scroll" => a.scroll = value()?.parse().map_err(|e| format!("--scroll: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--no-embedded" => a.embedded = false,
            "--json" => a.json = true,
            _ => {}
        }
    }
    // `--photos 0`: no first-thumbnail case.
    a.photos.retain(|&n| n > 0);
    if a.photos.iter().chain([&a.scroll]).any(|&n| n > synthetic::MAX_SCANNED_PHOTOS) {
        return Err(format!("at most {} photos", synthetic::MAX_SCANNED_PHOTOS));
    }
    Ok(a)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn spread(mut times: Vec<f64>) -> (f64, f64, f64) {
    times.sort_by(f64::total_cmp);
    (rank(&times, 50), rank(&times, 95), times.last().copied().unwrap_or(0.0))
}

/// Sleeps until frame `n` of a run that started at `start` is due.
fn pace(start: Instant, n: u64) {
    let due = start + Duration::from_secs_f64(n as f64 / FPS);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

fn options(cache: &Path) -> Options {
    Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), ..ThumbOptions::default() } }
}

/// A fresh folder under `root` for one case.
fn case_dir(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(format!("first-thumbnails-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The first screenful's files: the first photos of the newest folders (tree order is newest
/// first, names follow the capture times).
fn first_screenful(folders: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::with_capacity(SCREENFUL);
    for folder in folders {
        let mut names: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.retain(|n| n.ends_with(".jpg"));
        names.sort();
        for name in names {
            if out.len() == SCREENFUL {
                return out;
            }
            out.push((folder.clone(), name));
        }
    }
    out
}

fn first_thumbnails(a: &Args, n: usize) -> Value {
    let dir = case_dir(&a.dir, &n.to_string());
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    let folders = synthetic::write_photos_with(&dir, &spec, a.side, a.embedded.then_some(160)).unwrap();
    eprintln!("{n} photos of {} px written in {:.1} s", a.side, written.elapsed().as_secs_f64());
    let targets = first_screenful(&folders);
    let mut s = Session::open_with(&synthetic::library_dir(&dir), Arc::new(SystemClock), &options(&dir.join("cache"))).unwrap();

    let start = Instant::now();
    s.execute("folders.add", &json!({ "path": synthetic::photos_dir(&dir) })).unwrap();
    let (mut first, mut scan, mut thumbs) = (None, None, None);
    let mut frames: Vec<f64> = Vec::new();
    let mut frame = 0u64;
    while thumbs.is_none() {
        assert!(start.elapsed() < TIMEOUT, "{n} photos: no end after {} s", TIMEOUT.as_secs());
        let t = Instant::now();
        s.poll();
        let mut shown = 0;
        for (folder, name) in &targets {
            let id = s.library().folder_by_path(folder).and_then(|f| s.library().photo_by_name(f, name)).map(|p| p.id);
            if let Some(id) = id
                && matches!(s.thumbnail(id, NEED), Thumb::Image { .. })
            {
                shown += 1;
            }
        }
        frames.push(ms(t.elapsed()));
        if first.is_none() && shown == targets.len() {
            first = Some(start.elapsed());
        }
        if scan.is_none() && !s.busy() {
            scan = Some(start.elapsed());
        }
        if scan.is_some() && s.thumbnails_pending() == 0 && s.thumbnail_stats().scanned >= n {
            thumbs = Some(start.elapsed());
        }
        frame += 1;
        pace(start, frame);
    }
    let stats = s.thumbnail_stats();
    let photos = s.library().photos().count();
    assert_eq!(photos, n, "every photo indexed");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
    let (first, scan, thumbs) = (first.unwrap_or_default(), scan.unwrap_or_default(), thumbs.unwrap_or_default());
    let (p50, p95, max) = spread(frames);
    json!({
        "name": format!("first thumbnails {n}"),
        "first_ms": ms(first),
        "budget_ms": FIRST_BUDGET_MS,
        "scan_s": scan.as_secs_f64(),
        "scan_photos_per_s": n as f64 / scan.as_secs_f64().max(1e-9),
        "thumbs_s": thumbs.as_secs_f64(),
        "thumbs_photos_per_s": n as f64 / thumbs.as_secs_f64().max(1e-9),
        "frame_p50_ms": p50,
        "frame_p95_ms": p95,
        "frame_max_ms": max,
        "frames": frame,
        "embedded_shown": stats.quick,
        "decodes": stats.decodes,
        "side_px": a.side,
    })
}

/// The photos in the light table's order (newest folder first; in a folder by capture time, then
/// name), each with the section it starts, in one pass over the library.
fn display_order(s: &Session) -> (Vec<PhotoId>, Vec<usize>) {
    let lib = s.library();
    let mut by: std::collections::HashMap<FolderId, Vec<&dalica_library::Photo>> = std::collections::HashMap::new();
    for p in lib.photos() {
        by.entry(p.folder).or_default().push(p);
    }
    let mut folders: Vec<(Option<dalica_core::DateTime>, &Path, Vec<&dalica_library::Photo>)> = by
        .into_iter()
        .filter_map(|(f, mut photos)| {
            photos.sort_by(|a, b| a.sort_date().cmp(&b.sort_date()).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id)));
            let date = photos.iter().filter_map(|p| p.sort_date()).min();
            Some((date, lib.folder(f)?.path.as_path(), photos))
        })
        .collect();
    folders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let mut order = Vec::new();
    let mut sizes = Vec::new();
    for (_, _, photos) in folders {
        sizes.push(photos.len());
        order.extend(photos.into_iter().map(|p| p.id));
    }
    (order, sizes)
}

/// One row of cells: its top (points) and the cells it holds (indices into the order).
struct Row {
    top: f64,
    cells: std::ops::Range<usize>,
}

fn rows(sizes: &[usize]) -> (Vec<Row>, f64) {
    let mut out = Vec::new();
    let (mut y, mut at) = (0.0, 0usize);
    for &n in sizes {
        y += GAP + HEADER;
        let mut left = n;
        while left > 0 {
            let k = left.min(COLUMNS);
            out.push(Row { top: y, cells: at..at + k });
            at += k;
            left -= k;
            y += ROW;
        }
    }
    (out, y)
}

fn scanned_scroll(a: &Args, n: usize) -> Value {
    let dir = case_dir(&a.dir, &format!("scroll-{n}"));
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    synthetic::write_photos(&dir, &spec, synthetic::DEFAULT_FILE_SIDE).unwrap();
    eprintln!("{n} photos written in {:.1} s", written.elapsed().as_secs_f64());
    let cache = dir.join("cache");
    let scan = {
        let mut s = Session::open_with(&synthetic::library_dir(&dir), Arc::new(SystemClock), &options(&cache)).unwrap();
        let start = Instant::now();
        s.execute("folders.add", &json!({ "path": synthetic::photos_dir(&dir) })).unwrap();
        s.wait_idle();
        let took = start.elapsed();
        let stats = s.thumbnail_stats();
        assert_eq!((stats.scanned, stats.failed), (n, 0), "every photo scanned: {stats:?}");
        s.flush().unwrap();
        took
    };
    eprintln!("{n} photos scanned with their thumbnails in {:.1} s", scan.as_secs_f64());

    let mut s = Session::open_with(&synthetic::library_dir(&dir), Arc::new(SystemClock), &options(&cache)).unwrap();
    let (order, sizes) = display_order(&s);
    let (rows, height) = rows(&sizes);
    let step = TOP_SCREENS_PER_S * VIEW_H / FPS;
    let cells_per_view = (VIEW_H / ROW * COLUMNS as f64).max(1.0);
    let velocity = TOP_SCREENS_PER_S * cells_per_view;
    let (mut covered, mut upscaled, mut sketch, mut placeholder, mut failed) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut frames = Vec::new();
    let start = Instant::now();
    let mut offset = 0.0;
    let mut frame = 0u64;
    while offset < height - VIEW_H {
        let t = Instant::now();
        s.poll();
        let first = rows.partition_point(|r| r.top + ROW <= offset);
        let last = rows.partition_point(|r| r.top < offset + VIEW_H);
        let visible = match (rows.get(first), rows.get(last.saturating_sub(1))) {
            (Some(a), Some(b)) if first < last => a.cells.start..b.cells.end,
            _ => 0..0,
        };
        let _ = s.prefetch_thumbnails(&order, visible.clone(), velocity, NEED);
        for &id in &order[visible] {
            if s.thumbnail_failure(id).is_some() {
                failed += 1;
                continue;
            }
            match s.thumbnail_now(id, NEED) {
                Thumb::Image { covers: true, .. } => covered += 1,
                Thumb::Image { covers: false, .. } => upscaled += 1,
                Thumb::Sketch(_) => sketch += 1,
                Thumb::NotScanned => placeholder += 1,
            }
        }
        frames.push(ms(t.elapsed()));
        offset += step;
        frame += 1;
        pace(start, frame);
    }
    let stats = s.thumbnail_stats();
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
    let (p50, p95, max) = spread(frames);
    json!({
        "name": format!("scanned scroll {n}"),
        "scan_s": scan.as_secs_f64(),
        "scan_photos_per_s": n as f64 / scan.as_secs_f64().max(1e-9),
        "frames": frame,
        "screens": height / VIEW_H,
        "covered_cells": covered,
        "upscaled_cells": upscaled,
        "sketch_cells": sketch,
        "placeholder_cells": placeholder,
        "failed_cells": failed,
        "frame_p50_ms": p50,
        "frame_p95_ms": p95,
        "frame_max_ms": max,
        "budget_ms": FRAME_BUDGET_MS,
        "loads": stats.loads,
        "decodes": stats.decodes,
    })
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("first_thumbnails: {e}");
            std::process::exit(2);
        }
    };
    let mut rows: Vec<Value> = a.photos.iter().map(|&n| first_thumbnails(&a, n)).collect();
    if a.scroll > 0 {
        rows.push(scanned_scroll(&a, a.scroll));
    }
    let mut missed = Vec::new();
    for r in &rows {
        let name = r["name"].as_str().unwrap_or_default();
        if r["first_ms"].as_f64().is_some_and(|f| f > FIRST_BUDGET_MS) {
            missed.push(format!("{name}: first thumbnails after {:.0} ms (budget {FIRST_BUDGET_MS} ms)", r["first_ms"].as_f64().unwrap_or(0.0)));
        }
        if r["placeholder_cells"].as_u64().is_some_and(|p| p > 0) {
            missed.push(format!("{name}: {} placeholder cells", r["placeholder_cells"]));
        }
        if r.get("placeholder_cells").is_some() && r["frame_p95_ms"].as_f64().is_some_and(|p| p > FRAME_BUDGET_MS) {
            missed.push(format!("{name}: frame p95 {:.2} ms (budget {FRAME_BUDGET_MS} ms)", r["frame_p95_ms"].as_f64().unwrap_or(0.0)));
        }
    }
    if a.json {
        println!("{}", json!({ "rows": rows }));
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
    for m in &missed {
        eprintln!("over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
