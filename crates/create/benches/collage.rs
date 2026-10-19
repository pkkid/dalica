//! Collages at scale (`development.md` §8, `roadmap.md` M14.22): each style's layout of 200
//! pictures, and a 3000 px render of 50 `scenes` photos, its time and peak memory. Run it with
//!
//! ```sh
//! cargo xtask bench collage                                    # release, history
//! cargo bench -p dalica-create --bench collage -- --json
//! ```
//!
//! **The layouts.** A collage of `--pictures` (200) pictures in each of the six styles, at the
//! default 16:9 page and settings, a fixed seed; the pictures' aspect ratios cycle through 3:2,
//! 2:3, 4:3, 16:9 and 1:1. `layout::layout` (what the canvas, the render, the room's thumbnail and
//! the MCP reply all read) is timed `--samples` (21) times per style: rows `layout <style> <n>`.
//!
//! **The render.** A Picture Pile of `--photos` (50) `scenes` JPEGs of 2048 px on their long side
//! (every third portrait) with a white border, shadows and captions, drawn at 3000 px on the long
//! side (Create Collage's size, 3000 × 1688) and encoded as its JPEG (`render::render`). Each
//! picture is decoded from its file at the size its place asks (`decode_thumbnail`, as the engine
//! decodes a photo to the size asked before its edits), one at a time, as the engine's render
//! does. Timed `--renders` (5) times: row `render <n>`, with `peak_mib`, the most memory the render
//! held at once on its own thread (the page, the picture in hand and its decode, the encoder's
//! buffers), measured once by `allocation-counter`.
//!
//! Rows carry `p50_ms`, `p95_ms`, `max_ms` and their budgets (`budget_ms` on the median, and
//! `budget_peak_mib` on the render's memory); `cargo xtask budgets` judges them, `--check` makes
//! the bench judge them itself (in release, at full size), `--plant-ms N` adds N ms to each judged
//! time. Without `--bench` (under `cargo test --benches`) it runs a smoke pass: 30 pictures, 6
//! photos, two samples.

use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_create::collage::render::{self, Picture};
use dalica_create::collage::{Border, Collage, Placement, Style, layout};
use dalica_image::Limits;
use dalica_scenes::{Scene, jpeg};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two hardware threads, 2026-10-19: the layouts' medians 0.0103 ms (pile), 0.420
/// (mosaic), 0.712 (frame mosaic), 0.0077 (grid), 0.0065 (contact sheet), 0.0047 (multiple
/// exposure); the render's 1 401 ms and 42.98 MiB), the layouts rounded up to 0.05 ms, the render
/// to 5 ms and its memory to 5 MiB.
const LAYOUT_BUDGETS_MS: [(&str, f64); 6] =
    [("pile", 0.05), ("mosaic", 0.65), ("frameMosaic", 1.1), ("grid", 0.05), ("contactSheet", 0.05), ("multipleExposure", 0.05)];
const RENDER_BUDGET_MS: f64 = 2105.0;
const RENDER_PEAK_MIB_BUDGET: f64 = 65.0;
/// The sizes the budgets hold for.
const PICTURES: usize = 200;
const PHOTOS: usize = 50;
const LONG_SIDE: u32 = 3000;
/// The photos' long side, in pixels.
const PHOTO_SIDE: u32 = 2048;
const SEED: u64 = 0xc011_a6e5;

struct Args {
    pictures: usize,
    photos: usize,
    samples: usize,
    renders: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        pictures: if full { PICTURES } else { 30 },
        photos: if full { PHOTOS } else { 6 },
        samples: if full { 21 } else { 2 },
        renders: if full { 5 } else { 1 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--pictures" => a.pictures = value()?.parse().map_err(|e| format!("--pictures: {e}"))?,
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--renders" => a.renders = value()?.parse().map_err(|e| format!("--renders: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --pictures N, --photos N, --samples N, --renders N, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(1..=dalica_create::collage::MAX_CLIPS).contains(&a.pictures) || !(1..=1000).contains(&a.photos) {
        return Err(format!("--pictures must be 1..={} and --photos 1..=1000", dalica_create::collage::MAX_CLIPS));
    }
    if !(1..=1000).contains(&a.samples) || !(1..=100).contains(&a.renders) {
        return Err("--samples must be 1..=1000 and --renders 1..=100".into());
    }
    if !(0.0..=100_000.0).contains(&a.plant_ms) {
        return Err("--plant-ms must be 0..=100000".into());
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

fn row(name: &str, times: &[Duration], extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

/// Picture `i`'s aspect ratio (width over height).
fn aspect(i: usize) -> f64 {
    [1.5, 2.0 / 3.0, 4.0 / 3.0, 16.0 / 9.0, 1.0][i % 5]
}

fn collage(style: Style, n: usize) -> Collage {
    let mut c = Collage::new((1..=n as u64).map(PhotoId::new), SEED);
    c.style = style;
    c
}

/// Each style's layout of `n` pictures, `samples` times.
fn layouts(a: &Args) -> Vec<Value> {
    let aspects: Vec<f64> = (0..a.pictures).map(aspect).collect();
    let mut rows = Vec::new();
    for (id, budget) in LAYOUT_BUDGETS_MS {
        let style = Style::from_id(id).expect("a style id");
        let c = collage(style, a.pictures);
        let mut times = Vec::with_capacity(a.samples);
        let mut placed = 0;
        for _ in 0..a.samples {
            let t = Instant::now();
            let p = std::hint::black_box(layout(&c, &aspects));
            times.push(t.elapsed());
            placed = p.len();
        }
        assert_eq!(placed, a.pictures, "{id}: every picture placed");
        rows.push(row(&format!("layout {id} {}", a.pictures), &times, json!({ "budget_ms": budget, "pictures": a.pictures })));
    }
    rows
}

/// The `n` photos as JPEG files' bytes: 2048 px on the long side, every third portrait.
fn photos(n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            let (w, h) = if i % 3 == 1 { (PHOTO_SIDE * 2 / 3, PHOTO_SIDE) } else { (PHOTO_SIDE, PHOTO_SIDE * 2 / 3) };
            jpeg(&Scene::new(1000 + i as u64, w, h), 90, None).expect("a scenes JPEG")
        })
        .collect()
}

/// The render of a pile of the photos at [`LONG_SIDE`]: its times and the peak memory of one more.
fn render_rows(a: &Args) -> Value {
    let files = photos(a.photos);
    let mut c = collage(Style::Pile, a.photos);
    c.settings.border = Border::White;
    c.settings.captions = true;
    let aspects: Vec<f64> = (0..a.photos).map(|i| if i % 3 == 1 { 2.0 / 3.0 } else { 1.5 }).collect();
    let placements = layout(&c, &aspects);
    let size = c.page.pixel_size(LONG_SIDE);
    let limits = Limits::default();
    let asked = std::cell::Cell::new(0usize);
    let run = || {
        let mut picture = |p: &Placement, (w, h): (u32, u32)| {
            asked.set(asked.get() + 1);
            let image = dalica_image::decode_thumbnail(files.get(p.clip)?, w.max(h).max(1), &limits).ok()?;
            Some(Picture { image, caption: format!("Day {} at the lake", p.clip + 1), name: format!("IMG_{:04}.jpg", p.clip + 1) })
        };
        render::render(&c, &placements, &mut picture, size, None, &|| false, &mut |_, _| {}).expect("the collage renders")
    };
    let mut times = Vec::with_capacity(a.renders);
    let mut bytes = 0;
    for _ in 0..a.renders {
        asked.set(0);
        let t = Instant::now();
        bytes = std::hint::black_box(run()).len();
        times.push(t.elapsed());
        assert_eq!(asked.get(), a.photos, "every photo asked once");
    }
    let info = allocation_counter::measure(|| {
        std::hint::black_box(run());
    });
    let peak_mib = info.bytes_max as f64 / (1024.0 * 1024.0);
    row(
        &format!("render {}", a.photos),
        &times,
        json!({ "budget_ms": RENDER_BUDGET_MS, "peak_mib": peak_mib, "budget_peak_mib": RENDER_PEAK_MIB_BUDGET, "photos": a.photos,
                "page": [size.0, size.1], "jpeg_bytes": bytes }),
    )
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("collage: {e}");
            std::process::exit(2);
        }
    };
    let mut rows = layouts(&a);
    rows.push(render_rows(&a));
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "collage",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "pictures": a.pictures,
        "photos": a.photos,
        "photo_side": PHOTO_SIDE,
        "long_side": LONG_SIDE,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<32} {:>8} {:>10} {:>10} {:>10} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms", "peak MiB");
        for r in &rows {
            println!(
                "{:<32} {:>8} {:>10.3} {:>10.3} {:>10.3} {:>9}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
                r["peak_mib"].as_f64().map_or(String::new(), |m| format!("{m:.1}")),
            );
        }
    }
    if a.check && mode == "release" && a.pictures >= PICTURES && a.photos >= PHOTOS {
        let mut missed = Vec::new();
        for r in &rows {
            let name = r["name"].as_str().unwrap_or("?");
            if let Some(budget) = r["budget_ms"].as_f64() {
                let judged = r["p50_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
                if judged.is_nan() || judged > budget {
                    missed.push(format!("{name}: median {judged:.3} ms, over the {budget} ms budget"));
                }
            }
            if let (Some(peak), Some(budget)) = (r["peak_mib"].as_f64(), r["budget_peak_mib"].as_f64())
                && peak > budget
            {
                missed.push(format!("{name}: peak {peak:.1} MiB, over the {budget} MiB budget"));
            }
        }
        for m in &missed {
            eprintln!("collage: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
