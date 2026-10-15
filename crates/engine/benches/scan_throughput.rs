//! Initial scan throughput (`development.md` §8, `roadmap.md` M10.22): a first scan of a folder of
//! JPEGs that carry embedded thumbnails, in photos per second until every photo is in the library
//! with its embedded thumbnail available. Run it with
//!
//! ```sh
//! cargo xtask bench scan_throughput                                   # release: 5 000 JPEGs, history
//! cargo bench -p dalica-engine --bench scan_throughput -- --photos 5000 --json
//! cargo bench -p dalica-engine --bench scan_throughput -- --dir /fast/disk --side 2048
//! ```
//!
//! `--photos` synthetic `scenes` JPEGs (5 000; `--side` pixels on their long side, 1024 by default,
//! each with a 160-pixel EXIF thumbnail as cameras write) in folders of about 100 are written under
//! `--dir` first (not timed; by default this target directory's temporary folder, on the disk the
//! build runs on: the runner's SSD on the budgets job). A new library on an empty thumbnail cache
//! then runs `folders.add` on them, and frames run at 60 per second as the app's would, each one
//! calling [`Session::poll`]. The row `scan <n>` reports:
//! - `scan_s` and `photos_per_s`: from `folders.add` to the frame where the scan has ended and
//!   every photo is in the library: each file's header read, so its embedded thumbnail is
//!   available to the light table at once (the quick path of `engine::thumbs`); budget ≥ 500
//!   photos per second. The thumbnail jobs run alongside, as in the app;
//! - `thumbs_s` and `thumbs_photos_per_s`: until every photo also has its 128 and 256 entries made
//!   from a decode (printed, not budgeted: `first_thumbnails` budgets what the user sees first);
//! - `embedded`: the photos whose header names an embedded thumbnail, checked after the timing
//!   (must be every photo).
//!
//! The files are scanned just after they are written, so they are in the page cache, as after a
//! copy from a memory card; dropping the cache needs root. Files are removed afterwards. It exits
//! non-zero when a photo has no embedded thumbnail, and with `--check` also when the budget is
//! missed (in release, over 5 000 photos or more); `cargo xtask budgets` judges the report itself
//! (a planted slowdown there adds its milliseconds per photo). Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass over 200 photos of 320 pixels.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::SystemClock;
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use serde_json::{Value, json};

/// The budget: photos per second until the embedded thumbnails are available (`development.md` §8).
const BUDGET_PHOTOS_PER_S: f64 = 500.0;
/// The photos the budget holds for.
const PHOTOS: usize = 5_000;
/// The embedded thumbnail's long side, as cameras write it.
const EMBEDDED: u32 = 160;
/// Photos per folder on average.
const PER_FOLDER: usize = 100;
/// Frames per second the app runs at.
const FPS: f64 = 60.0;
/// The longest the scan may run.
const TIMEOUT: Duration = Duration::from_secs(1800);

struct Args {
    photos: usize,
    side: u32,
    dir: PathBuf,
    json: bool,
    check: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 200 },
        side: if full { 1024 } else { synthetic::DEFAULT_FILE_SIDE },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
        check: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --side PX, --dir PATH, --check, --json)")),
        }
    }
    if !(1..=synthetic::MAX_SCANNED_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be 1..={}", synthetic::MAX_SCANNED_PHOTOS));
    }
    if !(EMBEDDED..=synthetic::FILE_SIDE.1).contains(&a.side) {
        return Err(format!("--side must be {EMBEDDED}..={} (the embedded thumbnail is {EMBEDDED} px)", synthetic::FILE_SIDE.1));
    }
    Ok(a)
}

/// Sleeps until frame `n` of a run that started at `start` is due.
fn pace(start: Instant, n: u64) {
    let due = start + Duration::from_secs_f64(n as f64 / FPS);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

/// Photos under `root` whose header names an embedded thumbnail.
fn with_embedded(root: &Path) -> usize {
    let limits = dalica_image::Limits::default();
    let mut found = 0;
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "jpg")
                && let Ok((header, bytes)) = dalica_image::read_header_file(&path, &limits)
                && header.thumbnail(&bytes).is_some()
            {
                found += 1;
            }
        }
    }
    found
}

fn scan(a: &Args) -> Value {
    let n = a.photos;
    let dir = a.dir.join(format!("scan-throughput-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    synthetic::write_photos_with(&dir, &spec, a.side, Some(EMBEDDED)).unwrap();
    eprintln!("scan_throughput: {n} photos of {} px written in {:.1} s under {}", a.side, written.elapsed().as_secs_f64(), dir.display());
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(dir.join("cache")), ..ThumbOptions::default() } };
    let mut s = Session::open_with(&synthetic::library_dir(&dir), Arc::new(SystemClock), &options).unwrap();

    let start = Instant::now();
    s.execute("folders.add", &json!({ "path": synthetic::photos_dir(&dir) })).unwrap();
    let (mut scan, mut thumbs) = (None, None);
    let mut frame = 0u64;
    while thumbs.is_none() {
        assert!(start.elapsed() < TIMEOUT, "{n} photos: no end after {} s", TIMEOUT.as_secs());
        s.poll();
        if scan.is_none() && !s.busy() && s.library().photos().count() == n {
            scan = Some(start.elapsed());
        }
        if scan.is_some() && s.thumbnails_pending() == 0 && s.thumbnail_stats().scanned + s.thumbnail_stats().failed >= n {
            thumbs = Some(start.elapsed());
        }
        frame += 1;
        pace(start, frame);
    }
    let stats = s.thumbnail_stats();
    drop(s);
    let embedded = with_embedded(&synthetic::photos_dir(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    let (scan, thumbs) = (scan.unwrap_or_default().as_secs_f64(), thumbs.unwrap_or_default().as_secs_f64());
    json!({
        "name": format!("scan {n}"),
        "photos": n,
        "side_px": a.side,
        "embedded_px": EMBEDDED,
        "scan_s": scan,
        "photos_per_s": n as f64 / scan.max(1e-9),
        "budget_photos_per_s": BUDGET_PHOTOS_PER_S,
        "thumbs_s": thumbs,
        "thumbs_photos_per_s": n as f64 / thumbs.max(1e-9),
        "embedded": embedded,
        "thumbs_failed": stats.failed,
        "frames": frame,
    })
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("scan_throughput: {e}");
            std::process::exit(2);
        }
    };
    let row = scan(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "scan_throughput",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "rows": [row],
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{}", serde_json::to_string_pretty(&row).unwrap_or_default());
    }
    let mut missed = Vec::new();
    if row["embedded"].as_u64() != Some(a.photos as u64) {
        missed.push(format!("{} of {} photos have an embedded thumbnail", row["embedded"], a.photos));
    }
    let per_s = row["photos_per_s"].as_f64().unwrap_or(0.0);
    if a.check && mode == "release" && a.photos >= PHOTOS && per_s < BUDGET_PHOTOS_PER_S {
        missed.push(format!("{per_s:.0} photos/s, under the {BUDGET_PHOTOS_PER_S} photos/s budget"));
    }
    for m in &missed {
        eprintln!("scan_throughput: over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
