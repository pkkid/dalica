//! The effect tabs' tile grids (`development.md` §8, `roadmap.md` M12.21). Run it with
//!
//! ```sh
//! cargo xtask bench effect_tiles                                   # release, a 24 MP JPEG
//! cargo bench -p dalica-engine --bench effect_tiles -- --json
//! cargo bench -p dalica-engine --bench effect_tiles -- --check --plant-ms 400   # must fail
//! ```
//!
//! A `scenes` JPEG of `--width × --height` (6000 × 4000, 24 MP) is written first (not timed). Each
//! sample is a new session on an empty thumbnail cache with the photo's folder added and scanned
//! (not timed); then the photo opens in the edit view, its preview fitted to `--fit` (2560 × 1600,
//! as the `edit_preview` bench), and an effect tab's tiles are asked for at `--tile` device pixels
//! (78 × 48, the tile's preview at 1×) in the same instant, as the edit view does when it opens on
//! an effect tab. Timed from `open_preview` to the poll that hands over the tab's last tile: the
//! tiles wait while the preview has work on its way (`Session::preview_busy`), then one small decode
//! of the photo and the tab's tiles, rendered in grid order on the tiles' worker.
//!
//! Rows (p50/p95/max by nearest rank over `--samples` sessions, 7, with `first_p50_ms` to the first
//! tile and `budget_ms`): `tab 3 tiles` (12 tiles), `tab 4 tiles` (7) and `tab 5 tiles` (8),
//! judged on the median. It exits non-zero when a tile was not made or the photo was decoded more
//! than once; with `--check` also when a median misses its budget (in release, at 24 MP or larger);
//! `--plant-ms N` adds `N` ms to every judged time. `cargo xtask budgets` judges the report itself.
//! Without `--bench` (under `cargo test --benches`) it runs a smoke pass: a 600 × 400 photo, one
//! sample per tab.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_edit::catalog;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

/// The budgets (`development.md` §8): a tab's tiles from opening the photo to the last tile, the
/// median, 1.5 × the first measurement (release on two cores, `taskset -c 0,1`, 2026-10-18, the
/// median of three runs: 234.8, 230.9 and 239.0 ms), rounded up to 5 ms; tabs 3, 4 and 5.
const TAB_BUDGETS_MS: [(u8, f64); 3] = [(3, 355.0), (4, 350.0), (5, 360.0)];
/// The photo the budgets hold for: a 24 MP camera's.
const FULL: (u32, u32) = (6000, 4000);
const WAIT: Duration = Duration::from_secs(120);

struct Args {
    width: u32,
    height: u32,
    fit: (u32, u32),
    tile: (u32, u32),
    samples: usize,
    dir: PathBuf,
    json: bool,
    check: bool,
    plant: f64,
}

fn pair(arg: &str, v: &str) -> Result<(u32, u32), String> {
    let (w, h) = v.split_once('x').ok_or(format!("{arg} takes WxH"))?;
    Ok((w.parse().map_err(|e| format!("{arg}: {e}"))?, h.parse().map_err(|e| format!("{arg}: {e}"))?))
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        width: if full { FULL.0 } else { 600 },
        height: if full { FULL.1 } else { 400 },
        fit: if full { (2560, 1600) } else { (256, 160) },
        tile: (78, 48),
        samples: if full { 7 } else { 1 },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
        check: false,
        plant: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--width" => a.width = value()?.parse().map_err(|e| format!("--width: {e}"))?,
            "--height" => a.height = value()?.parse().map_err(|e| format!("--height: {e}"))?,
            "--fit" => a.fit = pair(arg, &value()?)?,
            "--tile" => a.tile = pair(arg, &value()?)?,
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--plant-ms" => a.plant = value()?.parse().ok().filter(|p: &f64| p.is_finite() && *p >= 0.0).ok_or("--plant-ms takes milliseconds")?,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --width PX, --height PX, --fit WxH, --tile WxH, --samples N, --dir PATH, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(16..=12_000).contains(&a.width) || !(16..=12_000).contains(&a.height) {
        return Err("--width and --height must be 16..=12000".into());
    }
    if !(16..=8192).contains(&a.fit.0) || !(16..=8192).contains(&a.fit.1) {
        return Err("--fit must be 16..=8192 each way".into());
    }
    let max = dalica_engine::effect_tiles::MAX_TILE;
    if !(1..=max).contains(&a.tile.0) || !(1..=max).contains(&a.tile.1) {
        return Err(format!("--tile must be 1..={max} each way"));
    }
    if !(1..=100).contains(&a.samples) {
        return Err("--samples must be 1..=100".into());
    }
    Ok(a)
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// A new library on the empty thumbnail cache `cache` with the photos' folder added and scanned.
fn session(root: &Path, photos: &Path, cache: &Path) -> (Session, PhotoId) {
    let name = cache.file_name().and_then(|n| n.to_str()).unwrap_or("x");
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), ..ThumbOptions::default() } };
    let mut s = Session::open_with(&root.join(format!("library-{name}")), Arc::new(SystemClock), &options).expect("a library");
    s.execute("folders.add", &json!({ "path": photos })).expect("the folder added");
    s.wait_idle();
    let id = s.library().photos().map(|p| p.id).next().expect("the photo scanned");
    (s, id)
}

/// One sample: the photo opened in the edit view and tab `tab`'s tiles asked for; the times to the
/// first and the last tile.
fn sample(a: &Args, root: &Path, photos: &Path, tab: u8, i: usize) -> Result<(f64, f64), String> {
    let (mut s, id) = session(root, photos, &root.join(format!("cache-{tab}-{i}")));
    let want = catalog::effects(tab).count();
    let started = Instant::now();
    s.open_preview(id, a.fit).map_err(|e| format!("open_preview: {e:?}"))?;
    s.request_effect_tiles(id, tab, a.tile).map_err(|e| format!("request_effect_tiles: {e}"))?;
    let (mut got, mut first) = (0usize, None);
    while got < want {
        if started.elapsed() > WAIT {
            return Err(format!("tab {tab}: {got} of {want} tiles after {} s", WAIT.as_secs()));
        }
        s.poll();
        let made = s.take_effect_tiles();
        if !made.is_empty() && first.is_none() {
            first = Some(ms(started.elapsed()));
        }
        got += made.len();
        if got < want {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    let last = ms(started.elapsed());
    let stats = s.effect_tile_stats();
    if stats.decodes != 1 || stats.failures != 0 || stats.renders != want as u64 {
        return Err(format!("tab {tab}: {stats:?} (one decode and {want} tiles expected)"));
    }
    s.take_preview_updates();
    s.wait_idle();
    Ok((first.unwrap_or(last), last))
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("effect_tiles: {e}");
            std::process::exit(2);
        }
    };
    let root = a.dir.join(format!("effect-tiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let photos = root.join("photos");
    dalica_fs::create_dir_all(&photos).expect("the photos' folder");
    let written = Instant::now();
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, a.width, a.height), 90, None).expect("a JPEG");
    dalica_fs::replace(&photos.join("IMG_0001.jpg"), &bytes, Expect::Absent).expect("the photo written");
    eprintln!("effect_tiles: a {} × {} JPEG written in {:.1} s", a.width, a.height, written.elapsed().as_secs_f64());

    let mut rows: Vec<Value> = Vec::new();
    let mut missed = Vec::new();
    let mut failed = Vec::new();
    for (tab, budget) in TAB_BUDGETS_MS {
        let (mut firsts, mut lasts) = (Vec::new(), Vec::new());
        for i in 0..a.samples {
            match sample(&a, &root, &photos, tab, i) {
                Ok((first, last)) => {
                    firsts.push(first);
                    lasts.push(last);
                }
                Err(e) => failed.push(e),
            }
        }
        firsts.sort_by(f64::total_cmp);
        lasts.sort_by(f64::total_cmp);
        let p50 = rank(&lasts, 50);
        eprintln!("  tab {tab}: the last tile after {p50:.1} ms (median), the first after {:.1} ms", rank(&firsts, 50));
        if lasts.is_empty() || p50 + a.plant > budget {
            missed.push(format!("tab {tab} tiles: median {:.1} ms over the {budget} ms budget", p50 + a.plant));
        }
        rows.push(json!({
            "name": format!("tab {tab} tiles"),
            "tiles": catalog::effects(tab).count(),
            "samples": lasts.len(),
            "p50_ms": p50,
            "p95_ms": rank(&lasts, 95),
            "max_ms": lasts.last().copied().unwrap_or(0.0),
            "first_p50_ms": rank(&firsts, 50),
            "budget_ms": budget,
        }));
    }
    let _ = std::fs::remove_dir_all(&root);

    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "effect_tiles", "mode": mode, "photo_px": [a.width, a.height], "fit": [a.fit.0, a.fit.1],
        "tile_px": [a.tile.0, a.tile.1], "plant_ms": a.plant, "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    }
    for f in &failed {
        eprintln!("effect_tiles: {f}");
    }
    if !failed.is_empty() {
        std::process::exit(1);
    }
    if !a.check {
        return;
    }
    if mode != "release" {
        eprintln!("effect_tiles: the budgets hold in release only; a {mode} build judges nothing");
        return;
    }
    if (a.width, a.height) < FULL {
        eprintln!("effect_tiles: the budgets hold for a {} × {} photo; nothing judged", FULL.0, FULL.1);
        std::process::exit(1);
    }
    for m in &missed {
        eprintln!("over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
    eprintln!("effect_tiles: every budget held");
}
