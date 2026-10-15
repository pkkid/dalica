//! Cold start and idle memory at library scale (`development.md` §8, `roadmap.md` M10.22): a fresh
//! process opens a 100 k-photo library and draws its first settled frame; it then idles and reads
//! its own resident memory. Run it with
//!
//! ```sh
//! cargo xtask bench cold_start                                     # release: 100 k photos, 5 runs, history
//! cargo bench -p dalica-ui --bench cold_start -- --runs 3 --json
//! ```
//!
//! The bench writes the index-only synthetic library (`dalica_engine::synthetic`, `--photos`
//! photos, 100 000 in 1 000 folders, each with its colour sketch in the index; not timed), then
//! runs itself `--runs` times (5) as a **fresh process** (`--child`), timing each from the spawn.
//! The child opens the library as the app does ([`Session::open_with`] reading the index; there
//! are no photo files, so thumbnails are off and every cell draws its sketch), shows it with the
//! real [`App`] in the headless driver at 1400 × 900, and steps frames until the **first settled
//! frame**: nothing pending (no opening, scan, visible thumbnail, queued input or animation) and
//! every visible cell of the light table drawn from its sketch (none a placeholder, none failed).
//! It says so on its standard output, which the parent reads as the cold start's end. Then it
//! checks the app stays settled, idles `--idle-ms` (5 000) without frames, as the window does with
//! nothing to show, and reads its **resident memory** in-process (Linux: `/proc/self/status`;
//! macOS: `ps` on itself; elsewhere not measured).
//!
//! Rows (their names are the keys `xtask bench` compares runs by):
//! - `cold start <n>`: from the spawn to the first settled frame, by nearest rank over the runs
//!   (budget ≤ 2 000 ms on the median); `open_ms` (the index read) and `first_frame_ms` (the first
//!   settled frame, in-process) are the children's medians;
//! - `idle memory <n>`: `rss_mib` (the largest of the runs; budget ≤ 600 MiB), `rss_p50_mib` and
//!   `peak_mib` (the high-water mark, Linux).
//!
//! What it leaves out: the OS window and the GPU (the window's own setup; the headless driver
//! tessellates and does not rasterize), and a dropped page cache (the index was just written, and
//! the binary has just been built; dropping the cache needs root). With `--check` the bench exits
//! non-zero when a budget is missed (in release, over 100 k photos); `cargo xtask budgets` judges the
//! report itself. Without `--bench` (under `cargo test --benches`) it runs a smoke pass: one run
//! over 2 000 photos with a short idle.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// The budgets (`development.md` §8).
const COLD_START_BUDGET_MS: f64 = 2000.0;
const IDLE_BUDGET_MIB: f64 = 600.0;
/// The library the budgets hold for.
const PHOTOS: usize = 100_000;
/// Photos per folder on average: the synthetic 100 k preset's 100.
const PER_FOLDER: usize = 100;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// The most frames the first settled frame may take.
const MAX_FRAMES: usize = 600;
/// The line a child prints at its first settled frame.
const SETTLED: &str = "cold_start: settled";

struct Args {
    photos: usize,
    runs: usize,
    idle_ms: u64,
    json: bool,
    check: bool,
    /// `--child <dir>`: open the library written in `dir` and report.
    child: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 2_000 },
        runs: if full { 5 } else { 1 },
        idle_ms: if full { 5_000 } else { 200 },
        json: false,
        check: false,
        child: None,
    };
    let mut it = all.iter();
    let number = |v: Option<&String>, what: &str| v.and_then(|v| v.parse::<u64>().ok()).ok_or_else(|| format!("{what} takes a number"));
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--photos" => a.photos = usize::try_from(number(it.next(), "--photos")?).map_err(|e| e.to_string())?,
            "--runs" => a.runs = usize::try_from(number(it.next(), "--runs")?).map_err(|e| e.to_string())?,
            "--idle-ms" => a.idle_ms = number(it.next(), "--idle-ms")?,
            "--child" => a.child = Some(PathBuf::from(it.next().ok_or("--child takes a folder")?)),
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --runs N, --idle-ms N, --check, --json)")),
        }
    }
    if !(PER_FOLDER..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be {PER_FOLDER}..={}", synthetic::MAX_PHOTOS));
    }
    if !(1..=100).contains(&a.runs) {
        return Err("--runs must be 1..=100".into());
    }
    if a.idle_ms > 600_000 {
        return Err("--idle-ms must be at most 600000".into());
    }
    Ok(a)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// `100000` → `100k`, `2500` → `2.5k`, `999` → `999`.
fn label(n: usize) -> String {
    match n {
        n if n >= 1000 && n % 1000 == 0 => format!("{}k", n / 1000),
        n if n >= 1000 => format!("{}k", n as f64 / 1000.0),
        n => n.to_string(),
    }
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}

/// This process's resident memory and its high-water mark, in MiB, read by the process itself.
fn memory() -> (Option<f64>, Option<f64>) {
    let mib = |kib: f64| kib / 1024.0;
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let field = |key: &str| {
            status.lines().find_map(|l| l.strip_prefix(key)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok()).map(mib)
        };
        return (field("VmRSS:"), field("VmHWM:"));
    }
    if cfg!(target_os = "macos") {
        let pid = std::process::id().to_string();
        let rss = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
            .map(mib);
        return (rss, None);
    }
    (None, None)
}

/// The child: open the library in `dir`, step to the first settled frame, idle, report.
fn child(dir: &Path, idle_ms: u64) {
    let started = Instant::now();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let session = Session::open_with(&synthetic::library_dir(dir), Arc::new(dalica_core::SystemClock), &options).expect("the library opens");
    let opened = started.elapsed();
    let photos = session.library().photos().count();
    let mut d = Driver::new(App::new(Some(session)), WINDOW.0, WINDOW.1, 1.0);
    let ready = |d: &Driver| {
        let Some(p) = d.app().panels() else { return false };
        let (c, cells) = (p.thumbnails.counts(), p.light_table.frame_info().cells as u64);
        d.busy(false).is_empty() && cells > 0 && c.sketch_cells == cells && c.placeholder_cells == 0 && c.failed_cells == 0
    };
    let mut frames = 0;
    while !ready(&d) {
        assert!(frames < MAX_FRAMES, "no settled frame after {MAX_FRAMES} frames; busy: {:?}", d.busy(false));
        d.step();
        frames += 1;
    }
    let first = started.elapsed();
    println!("{SETTLED} after {:.0} ms in-process", ms(first));
    let p = d.app().panels().expect("the library is open");
    let (cells, sketched) = (p.light_table.frame_info().cells, p.thumbnails.counts().sketch_cells);
    // Settled it stays: nothing pending for the driver's quiet frames, every cell still sketched.
    d.settle(MAX_FRAMES).expect("the app stays settled");
    assert!(ready(&d), "after settling, every visible cell is still drawn from its sketch");
    let repaint = d.repaint_delay();
    std::thread::sleep(Duration::from_millis(idle_ms));
    let (rss, peak) = memory();
    println!(
        "{}",
        json!({
            "photos": photos,
            "open_ms": ms(opened),
            "first_frame_ms": ms(first),
            "frames": frames + 2,
            "cells": cells,
            "sketch_cells": sketched,
            "idle_ms": idle_ms,
            "idle_repaint_ms": repaint.map(ms),
            "rss_mib": rss,
            "peak_mib": peak,
        })
    );
}

/// One fresh process: the time from its spawn to its settled line, and its report.
fn run_child(dir: &Path, idle_ms: u64) -> Result<(f64, Value), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this bench: {e}"))?;
    let spawned = Instant::now();
    let mut child = Command::new(exe)
        .arg("--child")
        .arg(dir)
        .args(["--idle-ms", &idle_ms.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start the child: {e}"))?;
    let stdout = child.stdout.take().ok_or("the child has no output")?;
    let (mut cold, mut report) = (None, None);
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("cannot read the child: {e}"))?;
        if cold.is_none() && line.starts_with(SETTLED) {
            cold = Some(ms(spawned.elapsed()));
        } else if line.starts_with('{') {
            report = serde_json::from_str::<Value>(&line).ok();
        }
    }
    let status = child.wait().map_err(|e| format!("the child: {e}"))?;
    if !status.success() {
        return Err(format!("the child failed ({status})"));
    }
    match (cold, report) {
        (Some(cold), Some(report)) => Ok((cold, report)),
        _ => Err("the child printed no settled line or no report".into()),
    }
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cold_start: {e}");
            std::process::exit(2);
        }
    };
    if let Some(dir) = &a.child {
        child(dir, a.idle_ms);
        return;
    }
    let dir = tempfile::tempdir().expect("a temporary folder");
    let spec = Spec::new(a.photos, a.photos / PER_FOLDER, synthetic::SEED);
    let written = Instant::now();
    synthetic::write_index(dir.path(), &spec).expect("the synthetic library is written");
    eprintln!("cold_start: {} photos in {} folders written in {:.0} ms", a.photos, spec.folders, ms(written.elapsed()));

    let mut runs = Vec::new();
    for n in 1..=a.runs {
        match run_child(dir.path(), a.idle_ms) {
            Ok((cold, report)) => {
                eprintln!(
                    "cold_start: run {n}: settled {cold:.0} ms after the spawn (open {:.0} ms, first settled frame {:.0} ms in-process), {} MiB resident after {} ms idle",
                    report["open_ms"].as_f64().unwrap_or(f64::NAN),
                    report["first_frame_ms"].as_f64().unwrap_or(f64::NAN),
                    report["rss_mib"].as_f64().map_or("?".into(), |m| format!("{m:.0}")),
                    a.idle_ms,
                );
                runs.push((cold, report));
            }
            Err(e) => {
                eprintln!("cold_start: run {n}: {e}");
                std::process::exit(1);
            }
        }
    }
    let field = |key: &str| sorted(runs.iter().filter_map(|(_, r)| r[key].as_f64()).collect());
    let cold = sorted(runs.iter().map(|(c, _)| *c).collect());
    let (open, first, rss, peak) = (field("open_ms"), field("first_frame_ms"), field("rss_mib"), field("peak_mib"));
    let last = runs.last().map(|(_, r)| r.clone()).unwrap_or_default();
    let name = label(a.photos);
    let rows = vec![
        json!({
            "name": format!("cold start {name}"),
            "samples": cold.len(),
            "p50_ms": rank(&cold, 50),
            "p95_ms": rank(&cold, 95),
            "max_ms": cold.last().copied().unwrap_or(0.0),
            "budget_ms": COLD_START_BUDGET_MS,
            "open_ms": rank(&open, 50),
            "first_frame_ms": rank(&first, 50),
            "frames": last["frames"],
            "cells": last["cells"],
            "sketch_cells": last["sketch_cells"],
        }),
        json!({
            "name": format!("idle memory {name}"),
            "samples": rss.len(),
            "rss_mib": if rss.len() == runs.len() { rss.last().copied() } else { None },
            "rss_p50_mib": (!rss.is_empty()).then(|| rank(&rss, 50)),
            "peak_mib": peak.last().copied(),
            "budget_mib": IDLE_BUDGET_MIB,
            "idle_ms": a.idle_ms,
            "idle_repaint_ms": last["idle_repaint_ms"],
        }),
    ];
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({ "bench": "cold_start", "mode": mode, "photos": a.photos, "runs": a.runs, "os": std::env::consts::OS, "rows": rows });
    if a.json {
        println!("{report}");
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
    let mut missed = Vec::new();
    if a.check && mode == "release" && a.photos >= PHOTOS {
        let p50 = rank(&cold, 50);
        if p50 > COLD_START_BUDGET_MS {
            missed.push(format!("cold start: median {p50:.0} ms, over the {COLD_START_BUDGET_MS} ms budget"));
        }
        match rows[1]["rss_mib"].as_f64() {
            Some(m) if m > IDLE_BUDGET_MIB => missed.push(format!("idle memory: {m:.0} MiB, over the {IDLE_BUDGET_MIB} MiB budget")),
            Some(_) => {}
            None => missed.push(format!("idle memory: not measured on {}", std::env::consts::OS)),
        }
    }
    for m in &missed {
        eprintln!("cold_start: over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
