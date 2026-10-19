//! `cargo xtask bench [name…] [-- bench args…]` (`docs/development.md` §3, §13): the quick local
//! before/after check for a hot path. Builds each bench in release (`cargo bench`), runs it with
//! `--json`, prints its rows, appends the run to `<target dir>/bench/history.jsonl` and flags a
//! CPU-time regression of more than [`THRESHOLD_PCT`] against the last run of the same bench, with
//! the same arguments, on the same machine.
//!
//! **What a bench prints.** With `--json`, one JSON object on its last line of standard output
//! with a `rows` array; each row has a `name` (the key runs are compared by) and numbers. A row's
//! **CPU-time fields** are the numbers whose key names a time unit as one of its `_`-separated
//! words (`p50_ms`, `pack_us_per_read`, `write_s`), except tail statistics (`p95_…`, `p99_…`,
//! `max_…`), which are too noisy to flag on. Other numbers (counts, `counters`) are printed only.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

/// A regression is flagged when a CPU-time field grew by more than this share of the last run.
pub const THRESHOLD_PCT: f64 = 20.0;

/// A registered bench: a `harness = false` bench target that understands `--json`.
pub struct Bench {
    pub name: &'static str,
    pub package: &'static str,
    pub what: &'static str,
}

pub const BENCHES: &[Bench] = &[
    Bench {
        name: "library_bench",
        package: "dalica-ui",
        what: "the light table scrolling a synthetic 10 k and 100 k-photo library: UI frame time, cells visited",
    },
    Bench { name: "pack", package: "dalica-thumbs", what: "the packed thumbnail cache against loose files (writes about 1 GB)" },
    Bench {
        name: "first_thumbnails",
        package: "dalica-engine",
        what: "first thumbnails after adding 1 k and 10 k JPEGs, the full scan, and the scanned 100 k light table at top thumb speed (writes about 3 GB)",
    },
    Bench {
        name: "edit_preview",
        package: "dalica-engine",
        what: "the edit view's preview of a 24 MP JPEG: open cold and from the cache, a Tuning drag's drafts and finals, next/previous prefetched",
    },
    Bench {
        name: "save_export",
        package: "dalica-engine",
        what: "Save of a 24 MP JPEG (rendered, and rotate-only) and the 200-photo mixed export at 1600 px with a watermark",
    },
    Bench {
        name: "search",
        package: "dalica-engine",
        what: "search of a synthetic 100 k library with captions, tags, folders and albums: the index built, one word, three words and Starred, a -word, search.set to its result applied",
    },
    Bench {
        name: "scan_throughput",
        package: "dalica-engine",
        what: "a first scan of 5 000 JPEGs with embedded thumbnails: photos per second until each is in the library with its embedded thumbnail available (writes about 0.5 GB)",
    },
    Bench {
        name: "cold_start",
        package: "dalica-ui",
        what: "a fresh process opening a 100 k index-only library to its first settled frame, then its resident memory after 5 s idle",
    },
    Bench {
        name: "import",
        package: "dalica-engine",
        what: "listing, then importing, 1 000 JPEGs from a temporary card: time to the first screenful of thumbnails, the listing, files and MB per second copied and verified (writes about 0.4 GB)",
    },
    Bench { name: "batch_edit", package: "dalica-engine", what: "Auto Fix on 1 000 photos as one step (edit.apply), then its undo" },
    Bench {
        name: "slideshow",
        package: "dalica-ui",
        what: "a slideshow of 24 MP JPEGs in the headless app, every transition: an advance to its first frame with the next photo prefetched, a transition frame, an upload frame, and changes that waited",
    },
    Bench {
        name: "effects",
        package: "dalica-edit",
        what: "the 27 effects at their defaults: a slider's draft and final on the 2560 × 1600 screen, and a 24 MP render as Save renders it",
    },
    Bench {
        name: "effect_tiles",
        package: "dalica-engine",
        what: "an effect tab's tiles of a 24 MP JPEG, from opening the photo in the edit view to the tab's last tile, tabs 3, 4 and 5",
    },
    Bench {
        name: "inference",
        package: "dalica-faces",
        what: "face detection on a 640 px picture and one embedding per candidate recogniser, on seeded networks of the candidate models' shapes",
    },
    Bench {
        name: "faces_scan",
        package: "dalica-engine",
        what: "the face scan over 1 000 photos of 2048 px: faces embedded per second, memory per worker, the owner's frame time while it runs (writes about 0.1 GB)",
    },
    Bench {
        name: "people_places",
        package: "dalica-ui",
        what: "the Unnamed grid opening with 10 000 faces and the Places pane with 10 000 geotagged photos, its markers grouped, in the headless app",
    },
    Bench {
        name: "collage",
        package: "dalica-create",
        what: "each collage style's layout of 200 pictures, and a 3000 px render of a pile of 50 photos: its time and peak memory",
    },
    Bench {
        name: "legacy_scan",
        package: "dalica-scan",
        what: "a first scan of 10 000 photos in 100 folders with only the previous organizer's sidecar against the same with .dalica.json, and a 64 MiB legacy sidecar read whole (writes about 0.2 GB)",
    },
    Bench {
        name: "collage_canvas",
        package: "dalica-ui",
        what: "the Collage room's frame with 100 pictures on the page while one is dragged, in the headless app",
    },
];

/// The benches whose source file is not named after the bench (`[[bench]]` with a `path`): the
/// bench's name, and its file's stem under the crate's `benches/` (checked by the tests; cargo
/// itself finds them by name).
#[cfg(test)]
pub const BENCH_FILES: [(&str, &str); 2] = [("legacy_scan", "legacy"), ("collage_canvas", "collage")];

/// The stem of `bench`'s source file under its crate's `benches/`.
#[cfg(test)]
pub fn bench_file(bench: &str) -> &str {
    BENCH_FILES.iter().find(|(name, _)| *name == bench).map_or(bench, |(_, file)| file)
}

/// Time units a field may name, in milliseconds.
const UNITS: [(&str, f64); 4] = [("s", 1000.0), ("ms", 1.0), ("us", 0.001), ("ns", 0.000_001)];
/// Tail statistics: printed, never compared.
const TAILS: [&str; 3] = ["p95", "p99", "max"];

/// Whether a row's numeric field `key` is a CPU time to compare between runs.
pub fn is_time_field(key: &str) -> bool {
    let words: Vec<&str> = key.split('_').collect();
    let tail = words.first().is_some_and(|w| TAILS.contains(w));
    !tail && words.iter().any(|w| UNITS.iter().any(|(u, _)| u == w))
}

/// The JSON report a bench printed: the last line of `stdout` that parses as an object with `rows`.
pub fn parse_report(stdout: &str) -> Option<Value> {
    stdout
        .lines()
        .rev()
        .filter(|l| l.trim_start().starts_with('{'))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("rows").is_some_and(Value::is_array))
}

/// The rows of a report by name.
fn rows(report: &Value) -> Vec<(&str, &Map<String, Value>)> {
    report
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|r| Some((r.get("name")?.as_str()?, r.as_object()?))).collect())
        .unwrap_or_default()
}

/// One line per CPU-time field that grew by more than `threshold_pct` from `previous` to `current`.
/// Rows or fields missing from either run, and zero or non-finite baselines, are not compared.
pub fn regressions(previous: &Value, current: &Value, threshold_pct: f64) -> Vec<String> {
    let before = rows(previous);
    let mut out = Vec::new();
    for (name, row) in rows(current) {
        let Some((_, old)) = before.iter().find(|(n, _)| *n == name) else { continue };
        for (key, value) in row {
            if !is_time_field(key) {
                continue;
            }
            let (Some(now), Some(then)) = (value.as_f64(), old.get(key).and_then(Value::as_f64)) else { continue };
            if !(now.is_finite() && then.is_finite() && then > 0.0) {
                continue;
            }
            let pct = (now - then) / then * 100.0;
            if pct > threshold_pct {
                out.push(format!("{name}: {key} {then:.4} → {now:.4} (+{pct:.0} %)"));
            }
        }
    }
    out
}

/// The last run in `history` (JSON lines; bad lines are skipped) of `bench` on `machine` with
/// `args`.
pub fn last_run(history: &str, bench: &str, machine: &str, args: &[String]) -> Option<Value> {
    history.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).rfind(|r| {
        r.get("bench").and_then(Value::as_str) == Some(bench)
            && r.get("machine").and_then(Value::as_str) == Some(machine)
            && r.get("args").and_then(Value::as_array).is_some_and(|a| a.iter().map(Value::as_str).eq(args.iter().map(|s| Some(s.as_str()))))
    })
}

/// Names the machine: OS, architecture, CPU count and model, host name. Runs are compared only
/// within one machine.
pub fn machine_id(os: &str, arch: &str, cpus: usize, model: &str, host: &str) -> String {
    let model = model.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "{os}-{arch}, {cpus} CPUs, {}, {}",
        if model.is_empty() { "unknown CPU" } else { &model },
        if host.is_empty() { "unknown host" } else { host }
    )
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn this_machine() -> String {
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| c.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split_once(':')).map(|(_, m)| m.trim().to_string()))
        .or_else(|| output("sysctl", &["-n", "machdep.cpu.brand_string"]))
        .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())
        .unwrap_or_default();
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| output("hostname", &[]))
        .unwrap_or_default();
    let cpus = std::thread::available_parallelism().map(usize::from).unwrap_or(1);
    machine_id(std::env::consts::OS, std::env::consts::ARCH, cpus, &model, &host)
}

/// `CARGO_TARGET_DIR` (relative to the workspace root when relative), else `<root>/target`.
pub fn target_dir(root: &Path, env: Option<std::ffi::OsString>) -> PathBuf {
    match env.filter(|v| !v.is_empty()) {
        Some(dir) => root.join(dir),
        None => root.join("target"),
    }
}

/// The row lines `bench` prints for a report: times first, then the other numbers and counters.
pub fn summary(report: &Value) -> Vec<String> {
    let num = |v: &Value| match v.as_f64() {
        Some(f) if v.is_f64() => format!("{f:.3}"),
        _ => v.to_string(),
    };
    rows(report)
        .into_iter()
        .map(|(name, row)| {
            // The percentiles first, in order; then the rest as the report sorts them.
            let order = |k: &str| ["p50_", "p95_", "p99_", "max_"].iter().position(|p| k.starts_with(p)).unwrap_or(4);
            let mut fields: Vec<_> = row.iter().filter(|(k, _)| *k != "name" && *k != "counters").collect();
            fields.sort_by_key(|(k, _)| order(k));
            let mut parts: Vec<String> = fields.iter().map(|(k, v)| format!("{k} {}", num(v))).collect();
            if let Some(c) = row.get("counters").and_then(Value::as_object) {
                parts.extend(c.iter().map(|(k, v)| format!("{k} {}", num(v))));
            }
            format!("  {name:<28} {}", parts.join(", "))
        })
        .collect()
}

/// What `cargo xtask bench` was asked: the benches (all when none is named) and the arguments
/// passed on to each after `--`.
pub fn parse_args(args: &[String]) -> Result<(Vec<&'static Bench>, Vec<String>), String> {
    let (names, extra) = match args.iter().position(|a| a == "--") {
        Some(i) => (args.get(..i).unwrap_or(&[]), args.get(i + 1..).unwrap_or(&[])),
        None => (args, &[][..]),
    };
    let mut picked = Vec::new();
    for n in names {
        match BENCHES.iter().find(|b| b.name == n) {
            Some(b) => picked.push(b),
            None => {
                let known: Vec<_> = BENCHES.iter().map(|b| format!("  {:<14} {} ({})", b.name, b.what, b.package)).collect();
                return Err(format!("bench: no bench `{n}`; the benches are:\n{}", known.join("\n")));
            }
        }
    }
    if picked.is_empty() {
        picked = BENCHES.iter().collect();
    }
    Ok((picked, extra.to_vec()))
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").current_dir(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let (benches, extra) = parse_args(args)?;
    let history_path = target_dir(root, std::env::var_os("CARGO_TARGET_DIR")).join("bench").join("history.jsonl");
    let machine = this_machine();
    let commit = git(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = git(root, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut flagged = Vec::new();
    for b in benches {
        println!("==> bench: {} ({})", b.name, b.package);
        let mut cmd = Command::new(&cargo);
        cmd.current_dir(root).args(["bench", "-p", b.package, "--bench", b.name, "--", "--json"]).args(&extra);
        let out = cmd.stderr(Stdio::inherit()).output().map_err(|e| format!("bench: cannot run cargo: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let report = parse_report(&stdout);
        if !out.status.success() {
            print!("{stdout}");
            return Err(format!("bench: {} failed ({})", b.name, out.status));
        }
        let Some(report) = report else {
            print!("{stdout}");
            return Err(format!("bench: {} printed no JSON report with `rows`", b.name));
        };
        for line in summary(&report) {
            println!("{line}");
        }
        let history = std::fs::read_to_string(&history_path).unwrap_or_default();
        match last_run(&history, b.name, &machine, &extra) {
            Some(prev) => {
                let found = regressions(prev.get("report").unwrap_or(&Value::Null), &report, THRESHOLD_PCT);
                let when = prev.get("commit").and_then(Value::as_str).unwrap_or("?");
                if found.is_empty() {
                    println!("  no CPU-time regression over {THRESHOLD_PCT} % against the last run ({when})");
                }
                for f in &found {
                    println!("  REGRESSION over {THRESHOLD_PCT} % against the last run ({when}): {f}");
                }
                flagged.extend(found.into_iter().map(|f| format!("{}: {f}", b.name)));
            }
            None => println!("  first run of this bench with these arguments on this machine: nothing to compare"),
        }
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let record = json!({ "bench": b.name, "machine": machine, "commit": commit, "dirty": dirty, "unix": unix, "args": extra, "report": report });
        append(&history_path, &record.to_string())?;
    }
    println!("bench: history in {}", history_path.display());
    if flagged.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "bench: {} CPU-time regression(s) over {THRESHOLD_PCT} % (check the load average; this run is now the last one compared against)",
            flagged.len()
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// M11's budgets (`docs/development.md` §8), joined to `accept::BUDGETS`: each judge reads its
// bench's `--json` report, as `cargo xtask budgets` runs it in release, and adds a planted slowdown
// of `plant` ms once per measured operation.
// ---------------------------------------------------------------------------------------------

/// The import budgets (M11.23): 1.5 × the first measurement (the median of three runs), release on two cores
/// (`import` → `FIRST_BUDGET_MS`, `LIST_BUDGET_MS`, `FILES_PER_S_BUDGET`, `MB_PER_S_BUDGET`).
pub const IMPORT_FIRST_MS: f64 = 35.0;
pub const IMPORT_LIST_MS: f64 = 320.0;
pub const IMPORT_FILES_PER_S: f64 = 22.9;
pub const IMPORT_MB_PER_S: f64 = 2.3;
/// The card the import budgets hold for: 1 000 JPEGs of 2048 pixels.
pub const IMPORT_PHOTOS: u64 = 1_000;
pub const IMPORT_SIDE_PX: u64 = 2048;

/// The batch edit budgets (M11.23), medians over 1 000 photos (`batch_edit` → `APPLY_BUDGET_MS`,
/// `UNDO_BUDGET_MS`).
pub const BATCH_APPLY_MS: f64 = 7.0;
pub const BATCH_UNDO_MS: f64 = 5.0;
pub const BATCH_PHOTOS: u64 = 1_000;

/// The slideshow budgets (M11.23; `slideshow` → `ADVANCE_BUDGET_MS`, `TRANSITION_BUDGET_MS`,
/// `FRAME_CAP_MS`): the advance's median, a transition frame's p95 (never above
/// [`SLIDESHOW_FRAME_CAP_MS`] whatever the first measurement), and no change waiting for its preview.
pub const SLIDESHOW_ADVANCE_MS: f64 = 24.0;
/// A transition frame's p95: an absolute 1 ms, set by the maintainer at M11.R (2026-10-17). The
/// first budget, 0.3 ms (1.5 × the first measurement, 0.199 ms), left no headroom for load: it
/// missed once at 0.32 ms on a busy machine, where 0.21 to 0.26 ms is typical.
pub const SLIDESHOW_TRANSITION_P95_MS: f64 = 1.0;
pub const SLIDESHOW_FRAME_CAP_MS: f64 = 16.0;
/// The photos the slideshow budgets hold for: a 24 MP camera's, in pixels.
pub const SLIDESHOW_PIXELS: u64 = 6000 * 4000;

/// A report from a release run, or why it cannot be judged.
fn release_report(bench: &str, stdout: &str) -> Result<Value, String> {
    let report = parse_report(stdout).ok_or(format!("{bench} printed no JSON report with `rows`"))?;
    let mode = report.get("mode").and_then(Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("{bench} ran in a {mode} build; the budgets hold in release only"));
    }
    Ok(report)
}

/// The finite number `key` of the row `name` of `report`.
fn row_number(bench: &str, report: &Value, name: &str, key: &str) -> Result<f64, String> {
    let row = report
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|r| r.get("name").and_then(Value::as_str) == Some(name)))
        .ok_or(format!("{bench}: no row `{name}`"))?;
    row.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or(format!("{bench}: `{name}` without `{key}`"))
}

fn planted(measured: f64, plant: f64, unit: &str) -> String {
    if plant > 0.0 { format!(" ({measured:.2} {unit} measured + {plant:.2} ms planted)") } else { String::new() }
}

/// `import --json` in release over the 1 000-photo card: the first screenful and the listing
/// within their budgets (a planted slowdown added once each), and the import's files and MB per
/// second from its median at least theirs (a planted slowdown added once per file copied).
pub fn judge_import(stdout: &str, plant: f64) -> Result<String, String> {
    let (report, n, side) = import_report(stdout)?;
    let first = row_number("import", &report, &format!("first thumbnails {n}"), "p50_ms")?;
    let judged = first + plant;
    if judged > IMPORT_FIRST_MS {
        return Err(format!("first thumbnails: median {judged:.1} ms{}, over the {IMPORT_FIRST_MS} ms budget", planted(first, plant, "ms")));
    }
    let list = row_number("import", &report, &format!("list {n}"), "p50_ms")?;
    let listed = list + plant;
    if listed > IMPORT_LIST_MS {
        return Err(format!("the listing: median {listed:.0} ms{}, over the {IMPORT_LIST_MS} ms budget", planted(list, plant, "ms")));
    }
    let copied = import_copy(&report, n, plant)?;
    Ok(format!(
        "{n} photos of {side} px: first screenful {judged:.1} ms ≤ {IMPORT_FIRST_MS} ms, listed in {listed:.0} ms ≤ {IMPORT_LIST_MS} ms, {copied}"
    ))
}

/// What the review's Budgets job judges absolutely when the first screenful and the listing are
/// judged against a base commit on the same runner (M12.26, `accept.rs` → `Relative::rest`): the
/// card's size and the import's files and MB per second, as [`judge_import`] does.
pub fn judge_import_copy(stdout: &str, plant: f64) -> Result<String, String> {
    let (report, n, side) = import_report(stdout)?;
    Ok(format!("{n} photos of {side} px: {}", import_copy(&report, n, plant)?))
}

/// The import report in release, with its photos and side, over the card the budgets hold for.
fn import_report(stdout: &str) -> Result<(Value, u64, u64), String> {
    let report = release_report("import", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    let side = report.get("side_px").and_then(Value::as_u64).unwrap_or(0);
    if photos < IMPORT_PHOTOS || side < IMPORT_SIDE_PX {
        return Err(format!("import ran over {photos} photos of {side} px; the budgets hold for {IMPORT_PHOTOS} of {IMPORT_SIDE_PX} px"));
    }
    Ok((report, photos, side))
}

/// The import's files and MB per second from its median, at least their budgets.
fn import_copy(report: &Value, n: u64, plant: f64) -> Result<String, String> {
    let name = format!("import {n}");
    let import_s = row_number("import", report, &name, "p50_ms")? / 1000.0;
    let bytes = row_number("import", report, &name, "bytes")?;
    let copied_s = import_s + plant * n as f64 / 1000.0;
    let per_s = n as f64 / copied_s.max(f64::MIN_POSITIVE);
    let mb_s = bytes / 1e6 / copied_s.max(f64::MIN_POSITIVE);
    let note = if plant > 0.0 { format!(" ({import_s:.2} s measured + {plant:.2} ms planted per file)") } else { String::new() };
    if per_s < IMPORT_FILES_PER_S || mb_s < IMPORT_MB_PER_S {
        return Err(format!(
            "the import: {n} files in {copied_s:.2} s{note}: {per_s:.1} files/s, {mb_s:.2} MB/s, under the {IMPORT_FILES_PER_S} files/s and {IMPORT_MB_PER_S} MB/s budget"
        ));
    }
    Ok(format!("imported in {copied_s:.2} s{note}: {per_s:.1} files/s ≥ {IMPORT_FILES_PER_S}, {mb_s:.2} MB/s ≥ {IMPORT_MB_PER_S}"))
}

/// `batch_edit --json` in release over 1 000 photos: Auto Fix's median and its undo's within their
/// budgets (a planted slowdown added to each).
pub fn judge_batch_edit(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("batch_edit", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    if photos < BATCH_PHOTOS {
        return Err(format!("batch_edit ran over {photos} photos; the budgets hold for {BATCH_PHOTOS}"));
    }
    let mut parts = Vec::new();
    for (name, budget) in [(format!("auto fix {photos}"), BATCH_APPLY_MS), (format!("undo auto fix {photos}"), BATCH_UNDO_MS)] {
        let p50 = row_number("batch_edit", &report, &name, "p50_ms")?;
        let judged = p50 + plant;
        if judged > budget {
            return Err(format!("{name}: median {judged:.2} ms{}, over the {budget} ms budget", planted(p50, plant, "ms")));
        }
        parts.push(format!("{name} {judged:.2} ms ≤ {budget} ms"));
    }
    Ok(format!("medians: {}", parts.join("; ")))
}

/// `slideshow --json` in release over 24 MP photos: no change waited for its preview, the advance's
/// median, a transition frame's p95 and an upload frame's p95 (M12.21) within their budgets, each
/// p95 within 16 ms (a planted slowdown added to every frame).
pub fn judge_slideshow(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("slideshow", stdout)?;
    let px = report.get("photo_px").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).product::<u64>()).unwrap_or(0);
    if px < SLIDESHOW_PIXELS {
        return Err(format!("slideshow ran over photos of {px} pixels; the budgets hold for {SLIDESHOW_PIXELS} (6000 × 4000)"));
    }
    let changes = row_number("slideshow", &report, "advance", "changes")?;
    let waits = row_number("slideshow", &report, "advance", "waits")?;
    if changes < 1.0 || waits != 0.0 {
        return Err(format!("{waits} of {changes} changes waited for their preview (must be 0 of at least 1)"));
    }
    let advance = row_number("slideshow", &report, "advance", "p50_ms")?;
    let judged = advance + plant;
    if judged > SLIDESHOW_ADVANCE_MS {
        return Err(format!("advance: median {judged:.2} ms{}, over the {SLIDESHOW_ADVANCE_MS} ms budget", planted(advance, plant, "ms")));
    }
    let p95 = row_number("slideshow", &report, "transition frame", "p95_ms")?;
    let frame = p95 + plant;
    let budget = SLIDESHOW_TRANSITION_P95_MS.min(SLIDESHOW_FRAME_CAP_MS);
    if frame > budget {
        return Err(format!("transition frame: p95 {frame:.2} ms{}, over the {budget} ms budget", planted(p95, plant, "ms")));
    }
    let upload_p95 = row_number("slideshow", &report, "upload frame", "p95_ms")?;
    let upload = upload_p95 + plant;
    let upload_budget = SLIDESHOW_UPLOAD_P95_MS.min(SLIDESHOW_FRAME_CAP_MS);
    if upload > upload_budget {
        return Err(format!("upload frame: p95 {upload:.2} ms{}, over the {upload_budget} ms budget", planted(upload_p95, plant, "ms")));
    }
    Ok(format!(
        "{changes} changes, 0 waits; advance median {judged:.2} ms ≤ {SLIDESHOW_ADVANCE_MS} ms; transition frame p95 {frame:.3} ms ≤ {budget} ms (cap {SLIDESHOW_FRAME_CAP_MS} ms); upload frame p95 {upload:.2} ms ≤ {upload_budget} ms"
    ))
}

// ---------------------------------------------------------------------------------------------
// M12's budgets (M12.21, `docs/development.md` §8): the 27 effects' renders, the effect tabs' tile
// grids and the slideshow's upload frame.
// ---------------------------------------------------------------------------------------------

/// A slideshow frame that uploads the next photo's texture during the display time (`slideshow` →
/// the `upload frame` row): its p95, 1.5 × the first measurement (M12.21), never above
/// [`SLIDESHOW_FRAME_CAP_MS`].
pub const SLIDESHOW_UPLOAD_P95_MS: f64 = 6.8;

/// The effects' budgets (`effects` → `BUDGETS`), per effect: the scale its draft budget was
/// measured at (½ or ¼, the engine's choice on the measuring machine; the `<effect> draft` row
/// renders at it on any machine, M13.R), and in ms its slider draft's and final's p95 on the
/// 2560 × 1600 screen (M13.43: 1.5 × the measurement after M13.34–M13.47, drafts rounded up to
/// 0.1 ms and finals to 1 ms, never above [`EFFECT_DRAFT_CAP_MS`] and [`EFFECT_FINAL_CAP_MS`]) and
/// its 24 MP Save render's median (M12.21).
pub const EFFECT_BUDGETS: [(&str, f64, f64, f64, f64); 27] = [
    ("sharpen", 0.25, 3.5, 63.0, 1225.0),
    ("sepia", 0.25, 4.2, 65.0, 485.0),
    ("bw", 0.5, 5.1, 21.0, 275.0),
    ("warmify", 0.25, 4.1, 64.0, 1485.0),
    ("grain", 0.5, 16.0, 48.0, 870.0),
    ("tint", 0.25, 5.0, 78.0, 460.0),
    ("saturation", 0.25, 4.7, 74.0, 2060.0),
    ("soft_focus", 0.25, 13.7, 63.0, 1280.0),
    ("glow", 0.25, 5.0, 58.0, 2565.0),
    ("filtered_bw", 0.5, 5.1, 21.0, 240.0),
    ("focal_bw", 0.25, 5.5, 85.0, 505.0),
    ("graduated_tint", 0.25, 3.7, 56.0, 370.0),
    ("infrared", 0.5, 5.1, 21.0, 245.0),
    ("plastic_lens", 0.25, 9.9, 73.0, 2295.0),
    ("hdr", 0.25, 6.2, 94.0, 3775.0),
    ("widescreen", 0.5, 10.2, 37.0, 765.0),
    ("dream_glow", 0.25, 4.4, 60.0, 2250.0),
    ("invert", 0.5, 5.4, 22.0, 240.0),
    ("duotone", 0.25, 5.3, 82.0, 500.0),
    ("boost", 0.25, 4.8, 74.0, 2460.0),
    ("soften", 0.25, 6.2, 56.0, 2700.0),
    ("vignette", 0.25, 4.4, 69.0, 780.0),
    ("pixelate", 0.5, 10.1, 38.0, 670.0),
    ("focal_zoom", 0.25, 11.9, 100.0, 22355.0),
    ("border", 0.25, 4.2, 64.0, 470.0),
    ("drop_shadow", 0.5, 9.1, 36.0, 340.0),
    ("museum_matte", 0.5, 10.1, 38.0, 390.0),
];
/// An empty recipe's final on the screen (p95) and its 24 MP Save render (median) (`effects` →
/// `EMPTY_FINAL_BUDGET_MS`, `EMPTY_SAVE_BUDGET_MS`).
pub const EFFECT_EMPTY_FINAL_MS: f64 = 12.0;
pub const EFFECT_EMPTY_SAVE_MS: f64 = 55.0;
/// The slider's caps (the edit view's budgets, `development.md` §8): a draft's and a final's p95
/// on the screen, which no effect's budget is ever above (`effects` → `DRAFT_CAP_MS`,
/// `FINAL_CAP_MS`).
pub const EFFECT_DRAFT_CAP_MS: f64 = 16.0;
pub const EFFECT_FINAL_CAP_MS: f64 = 100.0;
/// The photo and the screen the effects' budgets hold for.
pub const EFFECT_PHOTO: [u64; 2] = [6000, 4000];
pub const EFFECT_SCREEN: [u64; 2] = [2560, 1600];

/// The tile grids' budgets (`effect_tiles` → `TAB_BUDGETS_MS`): per effect tab, the median from
/// opening a 24 MP photo in the edit view to the tab's last tile, in ms.
pub const TILE_BUDGETS: [(u8, f64); 3] = [(3, 355.0), (4, 350.0), (5, 360.0)];

/// `[w, h]` of `key` in `report`, `[0, 0]` when missing.
fn size_of(report: &Value, key: &str) -> [u64; 2] {
    let v: Vec<u64> = report.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    match v.as_slice() {
        [w, h] => [*w, *h],
        _ => [0, 0],
    }
}

/// `effects --json` in release over a 24 MP photo on the 2560 × 1600 screen: every effect's draft
/// at its budget's scale and final p95 (within its budget and the slider's 16 and 100 ms), its
/// draft at the engine's own scale (p95 within the slider's 16 ms), its Save render's median, and
/// the empty recipe's final p95 and Save median within their budgets (a planted slowdown added once
/// per render). Every miss is listed. A draft rendered at another scale than its budget's judges
/// nothing (M13.R: the runner drafted Glow, Soften and Border at ½, four times the pixels of the
/// ¼ their budgets were measured at).
pub fn judge_effects(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("effects", stdout)?;
    let (photo, fit) = (size_of(&report, "photo_px"), size_of(&report, "fit"));
    if photo[0] * photo[1] < EFFECT_PHOTO[0] * EFFECT_PHOTO[1] || fit != EFFECT_SCREEN {
        return Err(format!(
            "effects ran over a {} × {} photo on a {} × {} screen; the budgets hold for {} × {} on {} × {}",
            photo[0], photo[1], fit[0], fit[1], EFFECT_PHOTO[0], EFFECT_PHOTO[1], EFFECT_SCREEN[0], EFFECT_SCREEN[1]
        ));
    }
    let mut checks: Vec<(String, &str, f64, &str)> =
        vec![("no effect final".into(), "p95_ms", EFFECT_EMPTY_FINAL_MS, "p95"), ("no effect save".into(), "p50_ms", EFFECT_EMPTY_SAVE_MS, "median")];
    for (name, scale, draft, final_, save) in EFFECT_BUDGETS {
        let row = format!("{name} draft");
        let ran = row_number("effects", &report, &row, "draft_scale")?;
        if (ran - scale).abs() > 1e-9 {
            return Err(format!("effects: `{row}` ran at {ran}; its budget holds at {scale}"));
        }
        checks.push((row, "p95_ms", draft.min(EFFECT_DRAFT_CAP_MS), "p95"));
        checks.push((format!("{name} engine draft"), "p95_ms", EFFECT_DRAFT_CAP_MS, "p95"));
        checks.push((format!("{name} final"), "p95_ms", final_.min(EFFECT_FINAL_CAP_MS), "p95"));
        checks.push((format!("{name} save"), "p50_ms", save, "median"));
    }
    let (mut missed, mut worst) = (Vec::new(), (0.0f64, String::new()));
    for (name, key, budget, stat) in &checks {
        let measured = row_number("effects", &report, name, key)?;
        let judged = measured + plant;
        if judged > *budget {
            missed.push(format!("{name}: {stat} {judged:.2} ms{}, over the {budget} ms budget", planted(measured, plant, "ms")));
        }
        if judged / budget > worst.0 {
            worst = (judged / budget, name.clone());
        }
    }
    if !missed.is_empty() {
        return Err(format!("{} of {} missed: {}", missed.len(), checks.len(), missed.join("; ")));
    }
    Ok(format!(
        "{} effects' drafts, finals and Saves and the empty recipe within their budgets, the closest at {:.0} % of its own ({})",
        EFFECT_BUDGETS.len(),
        worst.0 * 100.0,
        worst.1
    ))
}

/// `effect_tiles --json` in release over a 24 MP photo: each tab's median from opening the photo to
/// its last tile within its budget (a planted slowdown added once per sample).
pub fn judge_effect_tiles(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("effect_tiles", stdout)?;
    let photo = size_of(&report, "photo_px");
    if photo[0] * photo[1] < EFFECT_PHOTO[0] * EFFECT_PHOTO[1] {
        return Err(format!(
            "effect_tiles ran over a {} × {} photo; the budgets hold for {} × {}",
            photo[0], photo[1], EFFECT_PHOTO[0], EFFECT_PHOTO[1]
        ));
    }
    let mut parts = Vec::new();
    for (tab, budget) in TILE_BUDGETS {
        let name = format!("tab {tab} tiles");
        let p50 = row_number("effect_tiles", &report, &name, "p50_ms")?;
        let judged = p50 + plant;
        if judged > budget {
            return Err(format!("{name}: median {judged:.1} ms{}, over the {budget} ms budget", planted(p50, plant, "ms")));
        }
        parts.push(format!("{name} {judged:.1} ms ≤ {budget} ms"));
    }
    Ok(format!("medians to the last tile: {}", parts.join("; ")))
}

// ---------------------------------------------------------------------------------------------
// M13's budgets (M13.31, `docs/development.md` §8, `docs/faces.md` §8): face inference on seeded
// networks of the candidate models' shapes, the background face scan, and the people and places
// views at 10 000.
// ---------------------------------------------------------------------------------------------

/// The inference budgets (`inference` → `DETECT_BUDGET_MS`, `SFACE_BUDGET_MS`,
/// `AURAFACE_BUDGET_MS`): per row, its median in ms, 1.5 × the first measurement.
pub const INFERENCE_BUDGETS: [(&str, f64); 3] = [("detect 640", 62.5), ("embed sface", 68.0), ("embed auraface", 350.0)];
/// The detector's input side the inference budgets hold for.
pub const INFERENCE_SIDE: u64 = 640;

/// The face scan's budgets (`faces_scan` → `FACES_PER_S_BUDGET`, `WORKER_MIB_BUDGET`,
/// `FRAME_BUDGET_MS`, `FRAME_CAP_MS`): faces embedded per second at least the first measurement ÷
/// 1.5, the memory per worker at most 1.5 × the first, a frame's p95 at most 1.5 × the first and
/// never above [`SCAN_FRAME_CAP_MS`].
pub const SCAN_FACES_PER_S: f64 = 7.3;
pub const SCAN_WORKER_MIB: f64 = 105.0;
pub const SCAN_FRAME_P95_MS: f64 = 0.5;
pub const SCAN_FRAME_CAP_MS: f64 = 16.0;
/// The scan the budgets hold for: 1 000 photos of 2048 px, the SFace-shaped recogniser.
pub const SCAN_PHOTOS: u64 = 1_000;
pub const SCAN_SIDE: u64 = 2048;

/// The people and places views' budgets (`people_places` → `UNNAMED_OPEN_BUDGET_MS`,
/// `UNNAMED_SCROLL_BUDGET_MS`, `PLACES_OPEN_BUDGET_MS`, `PLACES_PAN_BUDGET_MS`): per row, the
/// statistic judged and its budget in ms, 1.5 × the first measurement.
pub const PEOPLE_PLACES_BUDGETS: [(&str, &str, f64); 4] =
    [("unnamed open", "p50_ms", 32.5), ("unnamed scroll", "p95_ms", 1.0), ("places open", "p50_ms", 2.0), ("places pan", "p95_ms", 2.0)];
/// The library the people and places budgets hold for.
pub const PEOPLE_PLACES_PHOTOS: u64 = 10_000;

/// "median" or "p95" for a row's statistic.
pub fn stat_name(key: &str) -> &'static str {
    if key.starts_with("p95") { "p95" } else { "median" }
}

/// Judges each `(row, statistic, budget)` of `report` with `plant` ms added; every miss is listed.
fn judge_rows(bench: &str, report: &Value, checks: &[(&str, &str, f64)], plant: f64) -> Result<Vec<String>, String> {
    let (missed, parts) = row_verdicts(bench, report, checks, plant)?;
    if missed.is_empty() { Ok(parts) } else { Err(format!("{} of {} missed: {}", missed.len(), checks.len(), missed.join("; "))) }
}

/// Each `(row, statistic, budget)` of `report` judged with `plant` ms added: the misses, and every
/// row's line.
fn row_verdicts(bench: &str, report: &Value, checks: &[(&str, &str, f64)], plant: f64) -> Result<(Vec<String>, Vec<String>), String> {
    let (mut missed, mut parts) = (Vec::new(), Vec::new());
    for (name, key, budget) in checks {
        let measured = row_number(bench, report, name, key)?;
        let judged = measured + plant;
        let stat = stat_name(key);
        if judged > *budget {
            missed.push(format!("{name}: {stat} {judged:.2} ms{}, over the {budget} ms budget", planted(measured, plant, "ms")));
        }
        parts.push(format!("{name} {judged:.2} ms ≤ {budget} ms"));
    }
    Ok((missed, parts))
}

/// `inference --json` in release at the detector's 640 px: detection's and each recogniser's
/// median within its budget (a planted slowdown added once per run).
pub fn judge_inference(stdout: &str, plant: f64) -> Result<String, String> {
    let report = inference_report(stdout)?;
    let checks: Vec<(&str, &str, f64)> = INFERENCE_BUDGETS.iter().map(|(name, budget)| (*name, "p50_ms", *budget)).collect();
    Ok(format!("medians: {}", judge_rows("inference", &report, &checks, plant)?.join("; ")))
}

/// The release report of `inference --json`, refused unless it ran at the detector's 640 px.
fn inference_report(stdout: &str) -> Result<Value, String> {
    let report = release_report("inference", stdout)?;
    let side = report.get("side").and_then(Value::as_u64).unwrap_or(0);
    if side != INFERENCE_SIDE {
        return Err(format!("inference ran at {side} px; the budgets hold for the detector's {INFERENCE_SIDE} px"));
    }
    Ok(report)
}

/// What the review's relative judge of `inference` (`accept::BUDGETS`) still holds absolutely: a
/// release run at the detector's 640 px.
pub fn judge_inference_shape(stdout: &str, _plant: f64) -> Result<String, String> {
    inference_report(stdout).map(|_| format!("at the detector's {INFERENCE_SIDE} px"))
}

/// `faces_scan --json` in release over 1 000 photos of 2048 px with the SFace-shaped recogniser:
/// faces embedded per second at least its budget (a planted slowdown counted once per face), the
/// memory per worker within its budget, and a frame's p95 while the scan runs within its budget
/// and 16 ms (a planted slowdown added). Every miss is listed.
pub fn judge_faces_scan(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("faces_scan", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    let side = report.get("side_px").and_then(Value::as_u64).unwrap_or(0);
    let recogniser = report.get("recogniser").and_then(Value::as_str).unwrap_or("?");
    if photos < SCAN_PHOTOS || side < SCAN_SIDE || recogniser != "sface" {
        return Err(format!(
            "faces_scan ran over {photos} photos of {side} px with {recogniser}; the budgets hold for {SCAN_PHOTOS} of {SCAN_SIDE} px with sface"
        ));
    }
    let name = format!("scan {photos}");
    let faces = row_number("faces_scan", &report, &name, "faces")?;
    let secs = row_number("faces_scan", &report, &name, "scan_s")? + plant * faces / 1000.0;
    let per_s = faces / secs.max(f64::MIN_POSITIVE);
    let workers = row_number("faces_scan", &report, &name, "workers")?;
    let mib = row_number("faces_scan", &report, &name, "per_worker_mib")?;
    let p95 = row_number("faces_scan", &report, "frame while scanning", "p95_ms")?;
    let frame = p95 + plant;
    let frame_budget = SCAN_FRAME_P95_MS.min(SCAN_FRAME_CAP_MS);
    let mut missed = Vec::new();
    if faces < 1.0 || per_s < SCAN_FACES_PER_S {
        let note = if plant > 0.0 { format!(" ({plant:.2} ms planted per face)") } else { String::new() };
        missed.push(format!("{faces} faces in {secs:.1} s{note}: {per_s:.1} faces/s, under the {SCAN_FACES_PER_S} faces/s budget"));
    }
    if workers < 1.0 || mib > SCAN_WORKER_MIB {
        missed.push(format!("{mib:.1} MiB per worker over {workers} workers, over the {SCAN_WORKER_MIB} MiB budget"));
    }
    if frame > frame_budget {
        missed.push(format!("a frame while scanning: p95 {frame:.2} ms{}, over the {frame_budget} ms budget", planted(p95, plant, "ms")));
    }
    if !missed.is_empty() {
        return Err(format!("{} of 3 missed: {}", missed.len(), missed.join("; ")));
    }
    Ok(format!(
        "{faces} faces in {secs:.1} s: {per_s:.1} faces/s ≥ {SCAN_FACES_PER_S}; {mib:.1} MiB per worker ≤ {SCAN_WORKER_MIB} MiB ({workers} workers); a frame while scanning p95 {frame:.2} ms ≤ {frame_budget} ms (cap {SCAN_FRAME_CAP_MS} ms)"
    ))
}

/// `people_places --json` in release over 10 000 photos: the openings' medians and the frames'
/// p95 within their budgets (a planted slowdown added to each).
pub fn judge_people_places(stdout: &str, plant: f64) -> Result<String, String> {
    let report = people_places_report(stdout)?;
    Ok(judge_rows("people_places", &report, &PEOPLE_PLACES_BUDGETS, plant)?.join("; "))
}

/// The release report of `people_places --json`, refused unless it ran over 10 000 photos.
fn people_places_report(stdout: &str) -> Result<Value, String> {
    let report = release_report("people_places", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    if photos < PEOPLE_PLACES_PHOTOS {
        return Err(format!("people_places ran over {photos} photos; the budgets hold for {PEOPLE_PLACES_PHOTOS}"));
    }
    Ok(report)
}

/// What the review's relative judge of `people_places` (`accept::BUDGETS`) still holds
/// absolutely: a release run over 10 000 photos.
pub fn judge_people_places_shape(stdout: &str, _plant: f64) -> Result<String, String> {
    people_places_report(stdout).map(|_| format!("over {PEOPLE_PLACES_PHOTOS} photos"))
}

// ---------------------------------------------------------------------------------------------
// M14's budgets (M14.22, `docs/development.md` §8): collage layouts and their render, legacy scans,
// the collage canvas. Each judge adds a planted slowdown once per measured operation (a layout, a
// render, a scan, a frame).
// ---------------------------------------------------------------------------------------------

/// The collage layouts' budgets (`collage` → `LAYOUT_BUDGETS_MS`): per style, the median of one
/// layout of [`COLLAGE_PICTURES`] pictures in ms, 1.5 × the first measurement rounded up to 0.05 ms.
pub const COLLAGE_LAYOUT_BUDGETS: [(&str, f64); 6] =
    [("pile", 0.05), ("mosaic", 0.65), ("frameMosaic", 1.1), ("grid", 0.05), ("contactSheet", 0.05), ("multipleExposure", 0.05)];
/// The render's budgets (`collage` → `RENDER_BUDGET_MS`, `RENDER_PEAK_MIB_BUDGET`): its median and
/// the most memory it holds, 1.5 × the first measurement.
pub const COLLAGE_RENDER_MS: f64 = 2105.0;
pub const COLLAGE_RENDER_PEAK_MIB: f64 = 65.0;
/// What the collage budgets hold for: layouts of 200 pictures, a render of 50 photos at 3000 px.
pub const COLLAGE_PICTURES: u64 = 200;
pub const COLLAGE_PHOTOS: u64 = 50;
pub const COLLAGE_LONG_SIDE: u64 = 3000;

/// The legacy scans' budgets (`legacy_scan` → `LEGACY_SCAN_BUDGET_MS`, `DALICA_SCAN_BUDGET_MS`,
/// `LARGE_FILE_BUDGET_MS`): per row, its median in ms, 1.5 × the first measurement.
pub const LEGACY_SCAN_BUDGETS: [(&str, f64); 3] = [("legacy scan 10000", 195.0), ("dalica scan 10000", 190.0), ("legacy file 64 MiB", 995.0)];
/// What the legacy scan budgets hold for: 10 000 photos, and a legacy sidecar of 64 MiB.
pub const LEGACY_SCAN_PHOTOS: u64 = 10_000;
pub const LEGACY_FILE_BYTES: u64 = 64 << 20;

/// The collage canvas's budget (`collage_canvas` → `DRAG_BUDGET_MS`, `FRAME_CAP_MS`): a drag
/// frame's p95, 1.5 × the first measurement rounded up to 0.5 ms, never above
/// [`CANVAS_FRAME_CAP_MS`].
pub const CANVAS_DRAG_P95_MS: f64 = 0.5;
pub const CANVAS_FRAME_CAP_MS: f64 = 16.0;
/// The collage the canvas budget holds for.
pub const CANVAS_PICTURES: u64 = 100;

/// `collage --json` in release over 200 pictures and 50 photos at 3000 px: each style's layout
/// median, the render's median (a planted slowdown added to each) and its peak memory within their
/// budgets. Every miss is listed.
pub fn judge_collage(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("collage", stdout)?;
    let number = |key: &str| report.get(key).and_then(Value::as_u64).unwrap_or(0);
    let (pictures, photos, long) = (number("pictures"), number("photos"), number("long_side"));
    if pictures < COLLAGE_PICTURES || photos < COLLAGE_PHOTOS || long < COLLAGE_LONG_SIDE {
        return Err(format!(
            "collage ran with {pictures} pictures and a render of {photos} photos at {long} px; the budgets hold for {COLLAGE_PICTURES}, and {COLLAGE_PHOTOS} at {COLLAGE_LONG_SIDE} px"
        ));
    }
    let layouts: Vec<(String, f64)> = COLLAGE_LAYOUT_BUDGETS.iter().map(|(style, b)| (format!("layout {style} {pictures}"), *b)).collect();
    let render = format!("render {photos}");
    let mut checks: Vec<(&str, &str, f64)> = layouts.iter().map(|(name, b)| (name.as_str(), "p50_ms", *b)).collect();
    checks.push((render.as_str(), "p50_ms", COLLAGE_RENDER_MS));
    let (mut missed, parts) = row_verdicts("collage", &report, &checks, plant)?;
    let peak = row_number("collage", &report, &render, "peak_mib")?;
    if peak > COLLAGE_RENDER_PEAK_MIB {
        missed.push(format!("{render}: peak {peak:.1} MiB, over the {COLLAGE_RENDER_PEAK_MIB} MiB budget"));
    }
    if !missed.is_empty() {
        return Err(format!("{} of {} missed: {}", missed.len(), checks.len() + 1, missed.join("; ")));
    }
    Ok(format!("medians: {}; {render} peak {peak:.1} MiB ≤ {COLLAGE_RENDER_PEAK_MIB} MiB", parts.join("; ")))
}

/// `legacy_scan --json` in release over 10 000 photos and a 64 MiB legacy sidecar: each scan's
/// median within its budget (a planted slowdown added to each).
pub fn judge_legacy_scan(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("legacy_scan", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    let bytes = report.get("large_bytes").and_then(Value::as_u64).unwrap_or(0);
    if photos != LEGACY_SCAN_PHOTOS || bytes != LEGACY_FILE_BYTES {
        return Err(format!(
            "legacy_scan ran over {photos} photos and a legacy sidecar of {bytes} bytes; the budgets hold for {LEGACY_SCAN_PHOTOS} and {LEGACY_FILE_BYTES}"
        ));
    }
    let checks: Vec<(&str, &str, f64)> = LEGACY_SCAN_BUDGETS.iter().map(|(name, b)| (*name, "p50_ms", *b)).collect();
    Ok(format!("medians: {}", judge_rows("legacy_scan", &report, &checks, plant)?.join("; ")))
}

/// `collage_canvas --json` in release with 100 pictures: a drag frame's p95 within its budget and
/// 16 ms (a planted slowdown added).
pub fn judge_collage_canvas(stdout: &str, plant: f64) -> Result<String, String> {
    let report = release_report("collage_canvas", stdout)?;
    let pictures = report.get("pictures").and_then(Value::as_u64).unwrap_or(0);
    if pictures < CANVAS_PICTURES {
        return Err(format!("collage_canvas ran with {pictures} pictures; the budget holds for {CANVAS_PICTURES}"));
    }
    let budget = CANVAS_DRAG_P95_MS.min(CANVAS_FRAME_CAP_MS);
    Ok(format!(
        "{pictures} pictures: {} (cap {CANVAS_FRAME_CAP_MS} ms)",
        judge_rows("collage_canvas", &report, &[("drag frame", "p95_ms", budget)], plant)?.join("; ")
    ))
}

fn append(path: &Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    writeln!(f, "{line}").map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const MACHINE: &str = "linux-x86_64, 24 CPUs, Some CPU, host";

    fn report(p50: f64, p95: f64) -> Value {
        json!({ "bench": "library_bench", "rows": [
            { "name": "scroll light table 100k", "samples": 2400, "p50_ms": p50, "p95_ms": p95, "max_ms": p95 * 4.0,
              "counters": { "cells_visited_p50": 35, "cells_visited_max": 42 } },
            { "name": "sections refresh 100k", "samples": 7, "p50_ms": 4.0, "p95_ms": 5.0, "max_ms": 5.0, "counters": {} },
        ]})
    }

    fn record(machine: &str, args: &[&str], commit: &str, report: Value) -> String {
        json!({ "bench": "library_bench", "machine": machine, "commit": commit, "args": args, "report": report }).to_string()
    }

    /// A planted history: an older and a newer run here, a faster run on another machine and one
    /// with other arguments after them, and a damaged line.
    fn history() -> String {
        [
            record(MACHINE, &[], "old", report(5.0, 6.0)),
            record(MACHINE, &[], "last", report(10.0, 12.0)),
            "{ not json".to_string(),
            record("other machine", &[], "elsewhere", report(1.0, 1.0)),
            record(MACHINE, &["--photos", "10000"], "quick", report(1.0, 1.0)),
            json!({ "bench": "pack", "machine": MACHINE, "args": [], "report": report(1.0, 1.0) }).to_string(),
        ]
        .join("\n")
    }

    #[test]
    fn a_25_percent_slowdown_is_flagged_and_a_10_percent_one_is_not() {
        let h = history();
        let prev = last_run(&h, "library_bench", MACHINE, &[]).unwrap();
        assert_eq!(prev["commit"], "last", "the last run on this machine with these arguments");
        let flagged = regressions(&prev["report"], &report(12.5, 12.0), THRESHOLD_PCT);
        assert_eq!(flagged, ["scroll light table 100k: p50_ms 10.0000 → 12.5000 (+25 %)"]);
        assert!(regressions(&prev["report"], &report(11.0, 12.0), THRESHOLD_PCT).is_empty(), "10 % is not flagged");
        assert!(regressions(&prev["report"], &report(9.0, 12.0), THRESHOLD_PCT).is_empty(), "faster is not flagged");
        // Tail statistics are printed, never flagged: a doubled p95 and max pass.
        assert!(regressions(&prev["report"], &report(10.0, 24.0), THRESHOLD_PCT).is_empty());
    }

    #[test]
    fn runs_compare_only_within_one_machine_bench_and_arguments() {
        let h = history();
        let quick = vec!["--photos".to_string(), "10000".to_string()];
        assert_eq!(last_run(&h, "library_bench", MACHINE, &quick).unwrap()["commit"], "quick");
        assert!(last_run(&h, "library_bench", "a new machine", &[]).is_none());
        assert!(last_run(&h, "library_bench", MACHINE, &["--frames".to_string()]).is_none());
        assert!(last_run(&h, "nothing", MACHINE, &[]).is_none());
        assert!(last_run("", "library_bench", MACHINE, &[]).is_none());
        assert_eq!(last_run(&h, "pack", MACHINE, &[]).unwrap()["bench"], "pack");
    }

    #[test]
    fn rows_and_fields_missing_on_either_side_or_zero_are_not_compared() {
        let prev = json!({ "rows": [{ "name": "a", "p50_ms": 0.0, "mean_us": 2.0 }, { "name": "gone", "p50_ms": 1.0 }] });
        let now = json!({ "rows": [{ "name": "a", "p50_ms": 9.0, "mean_us": 9.0, "new_ms": 5.0 }, { "name": "b", "p50_ms": 50.0 }] });
        assert_eq!(regressions(&prev, &now, THRESHOLD_PCT), ["a: mean_us 2.0000 → 9.0000 (+350 %)"]);
        assert!(regressions(&Value::Null, &now, THRESHOLD_PCT).is_empty());
        assert!(regressions(&json!({ "rows": "x" }), &json!({ "rows": [{ "p50_ms": 1 }] }), THRESHOLD_PCT).is_empty());
    }

    #[test]
    fn time_fields_name_a_unit_and_tails_are_left_out() {
        for k in ["p50_ms", "pack_us_per_read", "loose_us_per_read", "write_s", "mean_ns", "ms"] {
            assert!(is_time_field(k), "{k}");
        }
        for k in ["p95_ms", "max_ms", "p99_us", "samples", "cells_visited_max", "msg", "items", "photos"] {
            assert!(!is_time_field(k), "{k}");
        }
    }

    #[test]
    fn the_report_is_the_last_json_line_with_rows() {
        let out = "library_bench: 10k photos opened\n{\"no\":\"rows\"}\n{\"rows\":[{\"name\":\"a\",\"p50_ms\":1.5}]}\ntrailing text\n{ broken";
        assert_eq!(parse_report(out).unwrap()["rows"][0]["name"], "a");
        assert!(parse_report("nothing here\n{\"rows\": 3}").is_none());
        assert!(parse_report("").is_none());
    }

    #[test]
    fn the_summary_prints_times_and_counters() {
        let lines = summary(&report(0.1667, 0.18));
        assert_eq!(lines.len(), 2);
        let l = &lines[0];
        assert!(l.contains("scroll light table 100k") && l.contains("p50_ms 0.167, p95_ms 0.180, max_ms 0.720, samples 2400"), "{l}");
        assert!(l.contains("cells_visited_p50 35") && l.contains("cells_visited_max 42") && l.contains("samples 2400"), "{l}");
    }

    #[test]
    fn arguments_pick_benches_and_pass_the_rest_on() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let (b, extra) = parse_args(&s(&["library_bench", "--", "--photos", "10000"])).unwrap();
        assert_eq!((b.iter().map(|b| b.name).collect::<Vec<_>>(), extra), (vec!["library_bench"], s(&["--photos", "10000"])));
        let (all, extra) = parse_args(&[]).unwrap();
        assert_eq!((all.len(), extra.len()), (BENCHES.len(), 0));
        let e = parse_args(&s(&["nope"])).err().unwrap();
        assert!(e.contains("no bench `nope`") && e.contains("library_bench") && e.contains("pack"), "{e}");
    }

    #[test]
    fn machine_and_target_dir() {
        assert_eq!(machine_id("linux", "x86_64", 24, "  AMD   Ryzen 9 ", "box"), "linux-x86_64, 24 CPUs, AMD Ryzen 9, box");
        assert_eq!(machine_id("macos", "aarch64", 8, "", ""), "macos-aarch64, 8 CPUs, unknown CPU, unknown host");
        let root = Path::new("/work");
        assert_eq!(target_dir(root, None), root.join("target"));
        assert_eq!(target_dir(root, Some("".into())), root.join("target"));
        assert_eq!(target_dir(root, Some("t2".into())), root.join("t2"));
        let abs = std::env::temp_dir().join("agent-3");
        assert_eq!(target_dir(root, Some(abs.clone().into_os_string())), abs);
    }

    /// What `import --json` prints: the first measurement (2026-10-17) by default.
    pub fn import_out(mode: &str, photos: u64, side: u64, first: f64, list: f64, import_ms: f64) -> String {
        let r = json!({ "bench": "import", "mode": mode, "photos": photos, "side_px": side, "rows": [
            { "name": format!("first thumbnails {photos}"), "p50_ms": first },
            { "name": format!("list {photos}"), "p50_ms": list },
            { "name": format!("import {photos}"), "p50_ms": import_ms, "bytes": 103_645_431u64 },
            { "name": format!("in library {photos}"), "p50_ms": import_ms },
        ]});
        format!("{r}\n")
    }

    /// What `batch_edit --json` prints.
    pub fn batch_out(mode: &str, photos: u64, apply: f64, undo: f64) -> String {
        let r = json!({ "bench": "batch_edit", "mode": mode, "photos": photos, "rows": [
            { "name": format!("auto fix {photos}"), "p50_ms": apply },
            { "name": format!("undo auto fix {photos}"), "p50_ms": undo },
        ]});
        format!("{r}\n")
    }

    /// What `slideshow --json` prints, an upload frame at its first measurement (M12.21).
    pub fn slideshow_out(mode: &str, px: [u64; 2], advance: f64, p95: f64, waits: u64) -> String {
        slideshow_upload_out(mode, px, advance, p95, waits, SLIDESHOW_UPLOAD_MEASURED)
    }

    /// The upload frame's first measurement (M12.21): its p95.
    pub const SLIDESHOW_UPLOAD_MEASURED: f64 = 4.51;

    /// What `slideshow --json` prints, with an upload frame's p95.
    pub fn slideshow_upload_out(mode: &str, px: [u64; 2], advance: f64, p95: f64, waits: u64, upload: f64) -> String {
        let r = json!({ "bench": "slideshow", "mode": mode, "photo_px": px, "rows": [
            { "name": "advance", "p50_ms": advance, "changes": 18, "waits": waits },
            { "name": "transition frame", "p50_ms": p95 / 2.0, "p95_ms": p95 },
            { "name": "upload frame", "p50_ms": upload / 2.0, "p95_ms": upload },
        ]});
        format!("{r}\n")
    }

    /// The effects' measurements, in ms: per effect, its draft's and final's p95 (M13.43, the median
    /// of three runs after M13.34–M13.47) and its Save's median (M12.21's first measurement).
    pub const EFFECTS_MEASURED: [(&str, f64, f64, f64); 27] = [
        ("sharpen", 2.333, 41.934, 816.017),
        ("sepia", 2.771, 42.881, 320.319),
        ("bw", 3.380, 13.431, 180.871),
        ("warmify", 2.690, 42.006, 987.811),
        ("grain", 12.886, 31.381, 579.218),
        ("tint", 3.293, 51.608, 303.866),
        ("saturation", 3.103, 48.777, 1372.597),
        ("soft_focus", 9.130, 41.567, 850.319),
        ("glow", 3.295, 38.560, 1706.819),
        ("filtered_bw", 3.376, 13.516, 158.344),
        ("focal_bw", 3.649, 56.564, 333.724),
        ("graduated_tint", 2.415, 37.157, 245.386),
        ("infrared", 3.388, 13.658, 161.591),
        ("plastic_lens", 6.559, 48.120, 1527.933),
        ("hdr", 4.068, 62.247, 2515.803),
        ("widescreen", 6.767, 24.171, 507.836),
        ("dream_glow", 2.931, 39.579, 1498.082),
        ("invert", 3.559, 14.052, 159.155),
        ("duotone", 3.482, 54.376, 333.263),
        ("boost", 3.193, 49.041, 1639.044),
        ("soften", 4.100, 37.116, 1798.765),
        ("vignette", 2.912, 45.742, 519.994),
        ("pixelate", 6.669, 24.707, 446.290),
        ("focal_zoom", 7.900, 66.222, 14900.034),
        ("border", 2.739, 42.022, 313.240),
        ("drop_shadow", 6.003, 23.453, 223.459),
        ("museum_matte", 6.726, 25.297, 259.663),
    ];
    /// The empty recipe's first measurement: its final's p95 and its Save's median.
    pub const EMPTY_MEASURED: (f64, f64) = (7.500, 33.480);

    /// What `effects --json` prints: the empty recipe's rows from `empty`, each effect's from
    /// `times` (draft p95, final p95, Save median), its draft at its budget's scale, and the
    /// engine's draft at the same scale as measured ([`EFFECTS_MEASURED`]).
    pub fn effects_out(mode: &str, photo: [u64; 2], fit: [u64; 2], empty: (f64, f64), times: &[(&str, f64, f64, f64)]) -> String {
        let engine: Vec<(&str, f64, f64)> = EFFECTS_MEASURED.iter().map(|(name, draft, ..)| (*name, budget_scale(name), *draft)).collect();
        effects_out_with(mode, photo, fit, empty, times, &engine)
    }

    /// The scale of `name`'s draft budget.
    pub fn budget_scale(name: &str) -> f64 {
        EFFECT_BUDGETS.iter().find(|b| b.0 == name).map_or(1.0, |b| b.1)
    }

    /// [`effects_out`] with each effect's engine draft from `engine` (name, scale, p95; none
    /// printed for an effect it does not name).
    pub fn effects_out_with(
        mode: &str,
        photo: [u64; 2],
        fit: [u64; 2],
        empty: (f64, f64),
        times: &[(&str, f64, f64, f64)],
        engine: &[(&str, f64, f64)],
    ) -> String {
        let mut rows = vec![
            json!({ "name": "no effect final", "p50_ms": empty.0 / 2.0, "p95_ms": empty.0 }),
            json!({ "name": "no effect save", "p50_ms": empty.1, "p95_ms": empty.1 * 1.1 }),
        ];
        for (name, draft, final_, save) in times {
            rows.push(json!({ "name": format!("{name} draft"), "p50_ms": draft / 2.0, "p95_ms": draft, "draft_scale": budget_scale(name) }));
            if let Some((_, scale, p95)) = engine.iter().find(|e| e.0 == *name) {
                rows.push(json!({ "name": format!("{name} engine draft"), "p50_ms": p95 / 2.0, "p95_ms": p95, "draft_scale": scale }));
            }
            rows.push(json!({ "name": format!("{name} final"), "p50_ms": final_ / 2.0, "p95_ms": final_ }));
            rows.push(json!({ "name": format!("{name} save"), "p50_ms": save, "p95_ms": save * 1.1 }));
        }
        let r = json!({ "bench": "effects", "mode": mode, "photo_px": photo, "fit": fit, "effects": times.len(), "rows": rows });
        format!("{r}\n")
    }

    /// The tile grids' first measurement (M12.21): per tab, the median to the last tile.
    pub const TILES_MEASURED: [(u8, f64); 3] = [(3, 234.8), (4, 230.9), (5, 239.0)];

    /// What `effect_tiles --json` prints.
    pub fn tiles_out(mode: &str, photo: [u64; 2], medians: &[(u8, f64)]) -> String {
        let rows: Vec<Value> = medians
            .iter()
            .map(|(tab, p50)| json!({ "name": format!("tab {tab} tiles"), "p50_ms": p50, "p95_ms": p50 * 1.2, "first_p50_ms": p50 / 2.0 }))
            .collect();
        let r = json!({ "bench": "effect_tiles", "mode": mode, "photo_px": photo, "fit": [2560, 1600], "tile_px": [78, 48], "rows": rows });
        format!("{r}\n")
    }

    #[test]
    fn the_import_budgets_judge_the_first_screenful_the_listing_and_the_copy_rate() {
        let ok = judge_import(&import_out("release", 1000, 2048, 23.2, 211.2, 29_101.0), 0.0).unwrap();
        assert!(ok.contains("first screenful 23.2 ms ≤ 35 ms") && ok.contains("34.4 files/s ≥ 22.9") && ok.contains("3.56 MB/s ≥ 2.3"), "{ok}");
        let e = judge_import(&import_out("release", 1000, 2048, 36.0, 211.2, 29_101.0), 0.0).unwrap_err();
        assert!(e.starts_with("first thumbnails: median 36.0 ms"), "{e}");
        assert!(judge_import(&import_out("release", 1000, 2048, 23.2, 321.0, 29_101.0), 0.0).unwrap_err().starts_with("the listing"));
        let e = judge_import(&import_out("release", 1000, 2048, 23.2, 211.2, 44_000.0), 0.0).unwrap_err();
        assert!(e.contains("22.7 files/s"), "{e}");
        // A planted slowdown counts once on the first screenful and the listing, once per file on
        // the copy: 8 ms stays inside the 1.5 × headroom, 15 ms does not.
        assert!(
            judge_import(&import_out("release", 1000, 2048, 23.2, 211.2, 29_101.0), 8.0)
                .unwrap()
                .contains("(29.10 s measured + 8.00 ms planted per file)")
        );
        let e = judge_import(&import_out("release", 1000, 2048, 23.2, 211.2, 29_101.0), 15.0).unwrap_err();
        assert!(e.contains("38.2 ms (23.20 ms measured + 15.00 ms planted)"), "{e}");
        let e = judge_import(&import_out("release", 1000, 2048, 1.0, 1.0, 29_101.0), 15.0).unwrap_err();
        assert!(e.contains("44.10 s (29.10 s measured + 15.00 ms planted per file)"), "{e}");
        assert!(judge_import(&import_out("debug", 1000, 2048, 1.0, 1.0, 1.0), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_import(&import_out("release", 60, 2048, 1.0, 1.0, 1.0), 0.0).unwrap_err().contains("hold for 1000 of 2048 px"));
        assert!(judge_import(&import_out("release", 1000, 320, 1.0, 1.0, 1.0), 0.0).unwrap_err().contains("hold for 1000 of 2048 px"));
        assert!(judge_import("no report", 0.0).unwrap_err().contains("no JSON report"));
        assert!(judge_import("{\"mode\":\"release\",\"photos\":1000,\"side_px\":2048,\"rows\":[]}", 0.0).unwrap_err().contains("no row"));
        // Judged against a base (M12.26), the first screenful and the listing are the base's to
        // judge: the copy judge leaves them out and still holds the card and the copy rate.
        let ok = judge_import_copy(&import_out("release", 1000, 2048, 40.0, 346.0, 29_101.0), 0.0).unwrap();
        assert!(ok.contains("34.4 files/s ≥ 22.9") && !ok.contains("listed"), "{ok}");
        assert!(judge_import_copy(&import_out("release", 1000, 2048, 1.0, 1.0, 44_000.0), 0.0).unwrap_err().contains("22.7 files/s"));
        assert!(judge_import_copy(&import_out("release", 60, 2048, 1.0, 1.0, 1.0), 0.0).unwrap_err().contains("hold for 1000 of 2048 px"));
        assert!(judge_import_copy(&import_out("debug", 1000, 2048, 1.0, 1.0, 1.0), 0.0).unwrap_err().contains("debug build"));
    }

    #[test]
    fn the_batch_edit_budgets_judge_each_median() {
        assert!(judge_batch_edit(&batch_out("release", 1000, 4.65, 3.31), 0.0).unwrap().contains("auto fix 1000 4.65 ms ≤ 7 ms"));
        assert!(judge_batch_edit(&batch_out("release", 1000, 7.1, 1.0), 0.0).unwrap_err().starts_with("auto fix 1000: median 7.10 ms"));
        assert!(judge_batch_edit(&batch_out("release", 1000, 1.0, 5.1), 0.0).unwrap_err().starts_with("undo auto fix 1000"));
        let e = judge_batch_edit(&batch_out("release", 1000, 4.65, 3.31), 8.0).unwrap_err();
        assert!(e.contains("12.65 ms (4.65 ms measured + 8.00 ms planted)"), "{e}");
        assert!(judge_batch_edit(&batch_out("debug", 1000, 1.0, 1.0), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_batch_edit(&batch_out("release", 100, 1.0, 1.0), 0.0).unwrap_err().contains("hold for 1000"));
    }

    #[test]
    fn the_slideshow_budgets_need_no_wait_and_judge_the_advance_and_a_transition_frame() {
        let full = [6000, 4000];
        let ok = judge_slideshow(&slideshow_out("release", full, 16.0, 0.199, 0), 0.0).unwrap();
        assert!(ok.contains("18 changes, 0 waits") && ok.contains("advance median 16.00 ms ≤ 24 ms") && ok.contains("p95 0.199 ms ≤ 1 ms"), "{ok}");
        assert!(judge_slideshow(&slideshow_out("release", full, 16.0, 0.199, 1), 0.0).unwrap_err().contains("1 of 18 changes waited"));
        assert!(judge_slideshow(&slideshow_out("release", full, 24.5, 0.199, 0), 0.0).unwrap_err().starts_with("advance: median 24.50 ms"));
        assert!(judge_slideshow(&slideshow_out("release", full, 16.0, 1.02, 0), 0.0).unwrap_err().starts_with("transition frame: p95 1.02 ms"));
        // 8 ms planted in every frame misses either.
        let e = judge_slideshow(&slideshow_out("release", full, 17.0, 0.199, 0), 8.0).unwrap_err();
        assert!(e.contains("25.00 ms (17.00 ms measured + 8.00 ms planted)"), "{e}");
        let e = judge_slideshow(&slideshow_out("release", full, 16.0, 0.164, 0), 8.0).unwrap_err();
        assert!(e.starts_with("transition frame: p95 8.16 ms"), "{e}");
        assert!(SLIDESHOW_TRANSITION_P95_MS.min(SLIDESHOW_FRAME_CAP_MS) <= 16.0, "a transition frame within 16 ms whatever the first measurement");
        // The upload frame (M12.21): its p95 within its budget, a planted slowdown added.
        assert!(ok.contains(&format!("upload frame p95 {SLIDESHOW_UPLOAD_MEASURED:.2} ms ≤ {SLIDESHOW_UPLOAD_P95_MS} ms")), "{ok}");
        let over = SLIDESHOW_UPLOAD_P95_MS + 0.01;
        let e = judge_slideshow(&slideshow_upload_out("release", full, 16.0, 0.199, 0, over), 0.0).unwrap_err();
        assert!(e.starts_with("upload frame: p95"), "{e}");
        let upload_cap = SLIDESHOW_UPLOAD_P95_MS.min(SLIDESHOW_FRAME_CAP_MS);
        assert!((upload_cap - SLIDESHOW_UPLOAD_P95_MS).abs() < f64::EPSILON, "an upload frame within 16 ms whatever the first measurement");
        assert!(judge_slideshow("{\"mode\":\"release\",\"photo_px\":[6000,4000],\"rows\":[{\"name\":\"advance\",\"p50_ms\":16.0,\"changes\":18,\"waits\":0},{\"name\":\"transition frame\",\"p95_ms\":0.2}]}", 0.0).unwrap_err().contains("no row `upload frame`"));
        assert!(judge_slideshow(&slideshow_out("debug", full, 1.0, 0.1, 0), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_slideshow(&slideshow_out("release", [640, 427], 1.0, 0.1, 0), 0.0).unwrap_err().contains("6000 × 4000"));
    }

    #[test]
    fn the_effects_budgets_judge_every_draft_final_and_save_and_the_empty_recipe() {
        let (photo, screen) = (EFFECT_PHOTO, EFFECT_SCREEN);
        let out = |times: &[(&str, f64, f64, f64)], plant: f64| judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, times), plant);
        let ok = out(&EFFECTS_MEASURED, 0.0).unwrap();
        assert!(ok.starts_with("27 effects' drafts, finals and Saves and the empty recipe within their budgets"), "{ok}");
        // Each budget is 1.5 × its measurement rounded up (drafts to 0.1 ms, finals to 1 ms, Saves
        // to 5 ms; the empty final to 1 ms), never above the slider's caps, in the grids' order.
        for ((name, scale, draft, final_, save), (m, d, f, s)) in EFFECT_BUDGETS.iter().zip(EFFECTS_MEASURED) {
            assert_eq!(*name, m);
            // The scale the engine drafted at when the budget was measured: ½ after a final within
            // 32 ms (four times the 8 ms draft target), else ¼ (`development.md` §8, M13.43).
            assert_eq!(*scale, if f <= 32.0 { 0.5 } else { 0.25 }, "{name}: its draft budget was measured at another scale");
            assert!(*draft <= EFFECT_DRAFT_CAP_MS && *final_ <= EFFECT_FINAL_CAP_MS, "{name}: a budget above its cap");
            assert!(*draft == EFFECT_DRAFT_CAP_MS || one_and_a_half(*draft, d, 0.1), "{name}: draft {draft} is not 1.5 × {d}");
            assert!(*final_ == EFFECT_FINAL_CAP_MS || one_and_a_half(*final_, f, 1.0), "{name}: final {final_} is not 1.5 × {f}");
            assert!(*draft >= d && *final_ >= f, "{name}: the measurement within its budget");
            assert!(one_and_a_half(*save, s, 5.0), "{name}: save {save} is not 1.5 × {s} rounded up");
        }
        assert!(one_and_a_half(EFFECT_EMPTY_FINAL_MS, EMPTY_MEASURED.0, 1.0) && one_and_a_half(EFFECT_EMPTY_SAVE_MS, EMPTY_MEASURED.1, 5.0));
        // A slow draft, final or Save misses alone, also one under its own budget but over the
        // slider's cap; every miss is listed.
        for (i, (name, _, draft, final_, save)) in EFFECT_BUDGETS.iter().enumerate() {
            for (stage, field, over) in [("draft", 1, draft + 0.01), ("final", 2, final_ + 0.5), ("save", 3, save + 1.0)] {
                let mut slow = EFFECTS_MEASURED;
                match field {
                    1 => slow[i].1 = over,
                    2 => slow[i].2 = over,
                    _ => slow[i].3 = over,
                }
                let e = out(&slow, 0.0).unwrap_err();
                let stat = if stage == "save" { "median" } else { "p95" };
                assert!(e.starts_with(&format!("1 of 110 missed: {name} {stage}: {stat}")), "{e}");
            }
        }
        let mut slow = EFFECTS_MEASURED;
        slow[1].1 = 30.0;
        slow[26].3 *= 4.0;
        let e = out(&slow, 0.0).unwrap_err();
        assert!(e.starts_with("2 of 110 missed: sepia draft: p95 30.00 ms") && e.contains("museum_matte save: median"), "{e}");
        let e = judge_effects(&effects_out("release", photo, screen, (EFFECT_EMPTY_FINAL_MS + 0.5, EMPTY_MEASURED.1), &EFFECTS_MEASURED), 0.0)
            .unwrap_err();
        assert!(e.starts_with("1 of 110 missed: no effect final: p95"), "{e}");
        // M13.R: the runner's finals of Glow, Soften and Border took under 32 ms, so the engine
        // drafted them at ½ (11.10, 9.94 and 8.40 ms): judged against the slider's 16 ms only, they
        // hold; their drafts at ¼ are judged against their budgets.
        let runner = |glow: f64| -> Vec<(&str, f64, f64)> {
            EFFECTS_MEASURED
                .iter()
                .map(|(n, d, ..)| match *n {
                    "glow" => (*n, 0.5, glow),
                    "soften" => (*n, 0.5, 9.94),
                    "border" => (*n, 0.5, 8.40),
                    _ => (*n, budget_scale(n), *d),
                })
                .collect()
        };
        let ok = judge_effects(&effects_out_with("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED, &runner(11.10)), 0.0).unwrap();
        assert!(ok.starts_with("27 effects'"), "{ok}");
        // An engine draft over 16 ms misses alone, whatever its scale; a planted slowdown misses it.
        let e = judge_effects(&effects_out_with("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED, &runner(16.01)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 110 missed: glow engine draft: p95 16.01 ms, over the 16 ms budget"), "{e}");
        let e = judge_effects(&effects_out_with("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED, &runner(11.10)), 5.0).unwrap_err();
        assert!(e.contains("glow engine draft: p95 16.10 ms (11.10 ms measured + 5.00 ms planted)"), "{e}");
        // A draft row at another scale than its budget's judges nothing: the glow draft at ½.
        let mut at_half: Value = serde_json::from_str(&effects_out("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED)).unwrap();
        let glow = at_half["rows"].as_array_mut().unwrap().iter_mut().find(|r| r["name"] == "glow draft").unwrap();
        glow["draft_scale"] = json!(0.5);
        let e = judge_effects(&format!("{at_half}\n"), 0.0).unwrap_err();
        assert_eq!(e, "effects: `glow draft` ran at 0.5; its budget holds at 0.25");
        // A planted slowdown added to every render: 17 ms misses every draft (and the empty final),
        // 101 ms every final too.
        let e = out(&EFFECTS_MEASURED, 17.0).unwrap_err();
        for (name, ..) in EFFECT_BUDGETS {
            assert!(e.contains(&format!("{name} draft: p95")), "{name}'s draft misses with 17 ms planted: {e}");
            assert!(e.contains(&format!("{name} engine draft: p95")), "{name}'s engine draft misses with 17 ms planted: {e}");
        }
        assert!(e.contains("no effect final: p95") && e.contains("measured + 17.00 ms planted"), "{e}");
        let e = out(&EFFECTS_MEASURED, 101.0).unwrap_err();
        for (name, ..) in EFFECT_BUDGETS {
            assert!(e.contains(&format!("{name} draft: p95")) && e.contains(&format!("{name} final: p95")), "{name} with 101 ms planted: {e}");
        }
        // A missing effect, a debug build, a small photo or another screen judge nothing.
        let e = out(&EFFECTS_MEASURED[1..], 0.0).unwrap_err();
        assert!(e.contains("no row `sharpen draft`"), "{e}");
        assert!(judge_effects(&effects_out("debug", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED), 0.0).unwrap_err().contains("debug build"));
        let e = judge_effects(&effects_out("release", [600, 400], [256, 160], EMPTY_MEASURED, &EFFECTS_MEASURED), 0.0).unwrap_err();
        assert!(e.contains("hold for 6000 × 4000"), "{e}");
        let e = judge_effects(&effects_out("release", photo, [1400, 900], EMPTY_MEASURED, &EFFECTS_MEASURED), 0.0).unwrap_err();
        assert!(e.contains("on 2560 × 1600"), "{e}");
        assert!(judge_effects("no report", 0.0).unwrap_err().contains("no JSON report"));
    }

    #[test]
    fn the_tile_grid_budgets_judge_each_tabs_median() {
        let ok = judge_effect_tiles(&tiles_out("release", EFFECT_PHOTO, &TILES_MEASURED), 0.0).unwrap();
        assert!(ok.contains("tab 3 tiles") && ok.contains("tab 5 tiles"), "{ok}");
        for ((tab, budget), (t, measured)) in TILE_BUDGETS.iter().zip(TILES_MEASURED) {
            assert!(*tab == t && *budget >= measured, "tab {tab}");
            let mut slow = TILES_MEASURED;
            slow[usize::from(t - 3)].1 = budget + 1.0;
            let e = judge_effect_tiles(&tiles_out("release", EFFECT_PHOTO, &slow), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("tab {tab} tiles: median")), "{e}");
        }
        let plant = TILE_BUDGETS.iter().zip(TILES_MEASURED).map(|((_, b), (_, m))| b - m).fold(f64::INFINITY, f64::min) + 1.0;
        let e = judge_effect_tiles(&tiles_out("release", EFFECT_PHOTO, &TILES_MEASURED), plant).unwrap_err();
        assert!(e.contains("ms planted)"), "{e}");
        assert!(judge_effect_tiles(&tiles_out("debug", EFFECT_PHOTO, &TILES_MEASURED), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_effect_tiles(&tiles_out("release", [600, 400], &TILES_MEASURED), 0.0).unwrap_err().contains("hold for 6000 × 4000"));
        assert!(judge_effect_tiles(&tiles_out("release", EFFECT_PHOTO, &TILES_MEASURED[..2]), 0.0).unwrap_err().contains("no row `tab 5 tiles`"));
    }

    /// The inference's first measurement (M13.31): per row, its median.
    pub const INFERENCE_MEASURED: [(&str, f64); 3] = [("detect 640", 41.336), ("embed sface", 45.218), ("embed auraface", 230.284)];

    /// What `inference --json` prints at `side`, with each row's median.
    pub fn inference_out(mode: &str, side: u64, medians: &[(&str, f64)]) -> String {
        let rows: Vec<Value> =
            medians.iter().map(|(name, p50)| json!({ "name": name, "samples": 21, "p50_ms": p50, "p95_ms": p50 * 1.1, "weights": 1 })).collect();
        let r = json!({ "bench": "inference", "mode": mode, "threads": 1, "side": side, "rows": rows });
        format!("{r}\n")
    }

    /// The face scan's first measurement (M13.31): seconds for 2 000 faces, MiB per worker, workers,
    /// and a frame's p95 while it ran.
    pub const SCAN_MEASURED: (f64, f64, u64, f64) = (180.885, 67.090, 1, 0.201);

    /// What `faces_scan --json` prints over `photos` of `side` px with `recogniser`.
    pub fn faces_scan_out(mode: &str, photos: u64, side: u64, recogniser: &str, measured: (f64, f64, u64, f64)) -> String {
        let (scan_s, mib, workers, p95) = measured;
        let faces = photos * 2;
        let r = json!({ "bench": "faces_scan", "mode": mode, "photos": photos, "side_px": side, "recogniser": recogniser, "rows": [
            { "name": format!("scan {photos}"), "faces": faces, "scan_s": scan_s, "faces_per_s": faces as f64 / scan_s, "workers": workers,
              "per_worker_mib": mib, "rss_base_mib": 100.0, "rss_peak_mib": 100.0 + mib * workers as f64 },
            { "name": "frame while scanning", "samples": 6000, "p50_ms": p95 / 2.0, "p95_ms": p95, "max_ms": p95 * 3.0 },
        ]});
        format!("{r}\n")
    }

    /// The people and places views' first measurement (M13.31): per row, the statistic judged.
    pub const PEOPLE_PLACES_MEASURED: [f64; 4] = [21.530, 0.481, 1.048, 1.124];

    /// What `people_places --json` prints over `photos`, each row's judged statistic from `values`.
    pub fn people_places_out(mode: &str, photos: u64, values: [f64; 4]) -> String {
        let rows: Vec<Value> = PEOPLE_PLACES_BUDGETS
            .iter()
            .zip(values)
            .map(|((name, key, _), v)| {
                let (p50, p95) = if *key == "p50_ms" { (v, v * 1.2) } else { (v / 1.2, v) };
                json!({ "name": name, "samples": 11, "p50_ms": p50, "p95_ms": p95, "max_ms": p95 })
            })
            .collect();
        let r = json!({ "bench": "people_places", "mode": mode, "photos": photos, "window": [1400, 900], "rows": rows });
        format!("{r}\n")
    }

    /// A budget is 1.5 × its measurement rounded up by less than `step` (a rate: ÷ 1.5 rounded down).
    fn one_and_a_half(budget: f64, measured: f64, step: f64) -> bool {
        budget >= 1.5 * measured && budget < 1.5 * measured + step
    }

    #[test]
    fn the_inference_budgets_judge_each_median() {
        let ok = judge_inference(&inference_out("release", 640, &INFERENCE_MEASURED), 0.0).unwrap();
        assert!(ok.contains("detect 640") && ok.contains("embed sface") && ok.contains("embed auraface"), "{ok}");
        for ((name, budget), (m, measured)) in INFERENCE_BUDGETS.iter().zip(INFERENCE_MEASURED) {
            assert!(*name == m && one_and_a_half(*budget, measured, 5.0), "{name}: {budget} is not 1.5 × {measured}");
            let slow: Vec<(&str, f64)> = INFERENCE_MEASURED.iter().map(|(n, v)| (*n, if *n == m { budget + 0.1 } else { *v })).collect();
            let e = judge_inference(&inference_out("release", 640, &slow), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("1 of 3 missed: {name}: median")), "{e}");
        }
        // The largest planted slowdown misses all three; a debug build, another side or a missing
        // row judge nothing.
        let e = judge_inference(&inference_out("release", 640, &INFERENCE_MEASURED), crate::accept::MAX_PLANT_MS).unwrap_err();
        assert!(e.starts_with("3 of 3 missed") && e.contains("ms planted)"), "{e}");
        assert!(judge_inference(&inference_out("debug", 640, &INFERENCE_MEASURED), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_inference(&inference_out("release", 160, &INFERENCE_MEASURED), 0.0).unwrap_err().contains("640 px"));
        assert!(judge_inference(&inference_out("release", 640, &INFERENCE_MEASURED[..2]), 0.0).unwrap_err().contains("no row `embed auraface`"));
    }

    #[test]
    fn the_face_scan_budgets_judge_the_rate_the_memory_and_the_frames() {
        let out = |m| faces_scan_out("release", 1000, 2048, "sface", m);
        let ok = judge_faces_scan(&out(SCAN_MEASURED), 0.0).unwrap();
        assert!(ok.starts_with("2000 faces in") && ok.contains("faces/s ≥") && ok.contains("MiB per worker ≤"), "{ok}");
        let (secs, mib, workers, p95) = SCAN_MEASURED;
        assert!(SCAN_FACES_PER_S <= 2000.0 / secs / 1.5 && SCAN_FACES_PER_S > 2000.0 / secs / 1.5 - 1.0, "÷ 1.5, rounded down");
        assert!(one_and_a_half(SCAN_WORKER_MIB, mib, 5.0) && one_and_a_half(SCAN_FRAME_P95_MS, p95, 0.5));
        assert!(SCAN_FRAME_P95_MS.min(SCAN_FRAME_CAP_MS) <= 16.0, "a frame within 16 ms whatever the first measurement");
        // Each budget missed alone: a slow scan, a heavy worker, a slow frame, no worker at all.
        let e = judge_faces_scan(&out((secs * 1.6, mib, workers, p95)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 3 missed: 2000 faces in"), "{e}");
        let e = judge_faces_scan(&out((secs, SCAN_WORKER_MIB + 1.0, workers, p95)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 3 missed") && e.contains("MiB per worker"), "{e}");
        let e = judge_faces_scan(&out((secs, mib, workers, SCAN_FRAME_P95_MS + 0.01)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 3 missed: a frame while scanning: p95"), "{e}");
        assert!(judge_faces_scan(&out((secs, mib, 0, p95)), 0.0).unwrap_err().contains("over 0 workers"));
        // 8 ms planted misses the frame (a fraction of a millisecond), not the rate (16 s more over
        // 2 000 faces is inside its headroom); the largest planted slowdown misses both.
        let e = judge_faces_scan(&out(SCAN_MEASURED), 8.0).unwrap_err();
        assert!(e.starts_with("1 of 3 missed: a frame while scanning") && e.contains("+ 8.00 ms planted"), "{e}");
        let e = judge_faces_scan(&out(SCAN_MEASURED), crate::accept::MAX_PLANT_MS).unwrap_err();
        assert!(e.starts_with("2 of 3 missed") && e.contains("ms planted per face"), "{e}");
        // Another scan judges nothing.
        assert!(judge_faces_scan(&faces_scan_out("debug", 1000, 2048, "sface", SCAN_MEASURED), 0.0).unwrap_err().contains("debug build"));
        for bad in [faces_scan_out("release", 12, 2048, "sface", SCAN_MEASURED), faces_scan_out("release", 1000, 640, "sface", SCAN_MEASURED)] {
            assert!(judge_faces_scan(&bad, 0.0).unwrap_err().contains("hold for 1000 of 2048 px with sface"));
        }
        assert!(judge_faces_scan(&faces_scan_out("release", 1000, 2048, "auraface", SCAN_MEASURED), 0.0).unwrap_err().contains("with sface"));
    }

    #[test]
    fn the_people_and_places_budgets_judge_the_openings_and_the_frames() {
        let ok = judge_people_places(&people_places_out("release", 10_000, PEOPLE_PLACES_MEASURED), 0.0).unwrap();
        assert!(ok.contains("unnamed open") && ok.contains("places pan"), "{ok}");
        for (i, ((name, key, budget), measured)) in PEOPLE_PLACES_BUDGETS.iter().zip(PEOPLE_PLACES_MEASURED).enumerate() {
            assert!(one_and_a_half(*budget, measured, 0.5), "{name}: {budget} is not 1.5 × {measured}");
            let mut slow = PEOPLE_PLACES_MEASURED;
            slow[i] = budget + 0.01;
            let e = judge_people_places(&people_places_out("release", 10_000, slow), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("1 of 4 missed: {name}: {}", stat_name(key))), "{e}");
        }
        let e = judge_people_places(&people_places_out("release", 10_000, PEOPLE_PLACES_MEASURED), 8.0).unwrap_err();
        assert!(e.contains("places pan: p95") && e.contains("+ 8.00 ms planted"), "{e}");
        let e = judge_people_places(&people_places_out("release", 10_000, PEOPLE_PLACES_MEASURED), crate::accept::MAX_PLANT_MS).unwrap_err();
        assert!(e.starts_with("4 of 4 missed"), "{e}");
        assert!(judge_people_places(&people_places_out("debug", 10_000, PEOPLE_PLACES_MEASURED), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_people_places(&people_places_out("release", 1_000, PEOPLE_PLACES_MEASURED), 0.0).unwrap_err().contains("hold for 10000"));
    }

    /// The collage bench's first measurement (M14.22): each style's layout median, then the
    /// render's median (ms) and peak memory (MiB).
    pub const COLLAGE_LAYOUTS_MEASURED: [f64; 6] = [0.0103, 0.4198, 0.7122, 0.0077, 0.0065, 0.0047];
    pub const COLLAGE_RENDER_MEASURED: (f64, f64) = (1401.4, 42.98);

    /// What `collage --json` prints with `pictures` and a render of `photos` at `long` px.
    pub fn collage_out(mode: &str, pictures: u64, photos: u64, long: u64, layouts: [f64; 6], render: (f64, f64)) -> String {
        let mut rows: Vec<Value> = COLLAGE_LAYOUT_BUDGETS
            .iter()
            .zip(layouts)
            .map(|((style, _), v)| json!({ "name": format!("layout {style} {pictures}"), "samples": 21, "p50_ms": v, "p95_ms": v * 1.2 }))
            .collect();
        rows.push(json!({ "name": format!("render {photos}"), "samples": 5, "p50_ms": render.0, "p95_ms": render.0 * 1.1, "peak_mib": render.1 }));
        let r = json!({ "bench": "collage", "mode": mode, "pictures": pictures, "photos": photos, "long_side": long, "rows": rows });
        format!("{r}\n")
    }

    /// The legacy scan bench's first measurement (M14.22): each row's median.
    pub const LEGACY_SCAN_MEASURED: [f64; 3] = [128.73, 124.16, 661.12];

    /// What `legacy_scan --json` prints over `photos` and a legacy sidecar of `bytes`.
    pub fn legacy_scan_out(mode: &str, photos: u64, bytes: u64, values: [f64; 3]) -> String {
        let rows: Vec<Value> = LEGACY_SCAN_BUDGETS
            .iter()
            .zip(values)
            .map(|((name, _), v)| json!({ "name": name, "samples": 5, "p50_ms": v, "p95_ms": v * 1.1 }))
            .collect();
        let r = json!({ "bench": "legacy_scan", "mode": mode, "folders": 100, "photos": photos, "large_bytes": bytes, "rows": rows });
        format!("{r}\n")
    }

    /// The canvas bench's first measurement (M14.22): a drag frame's p95.
    pub const CANVAS_MEASURED: f64 = 0.167;

    /// What `collage_canvas --json` prints with `pictures` and a drag frame's p95.
    pub fn collage_canvas_out(mode: &str, pictures: u64, p95: f64) -> String {
        let r = json!({ "bench": "collage_canvas", "mode": mode, "pictures": pictures, "window": [1400, 900], "rows": [
            { "name": "drag frame", "samples": 240, "p50_ms": p95 * 0.9, "p95_ms": p95, "max_ms": p95 * 2.0 },
            { "name": "drop frame", "samples": 4, "p50_ms": p95, "p95_ms": p95, "max_ms": p95 },
        ]});
        format!("{r}\n")
    }

    #[test]
    fn the_collage_budgets_judge_each_layout_the_render_and_its_memory() {
        let out = |layouts, render| collage_out("release", 200, 50, 3000, layouts, render);
        let ok = judge_collage(&out(COLLAGE_LAYOUTS_MEASURED, COLLAGE_RENDER_MEASURED), 0.0).unwrap();
        assert!(ok.contains("layout mosaic 200") && ok.contains("render 50") && ok.contains("peak 43.0 MiB ≤ 65 MiB"), "{ok}");
        for (i, ((style, budget), measured)) in COLLAGE_LAYOUT_BUDGETS.iter().zip(COLLAGE_LAYOUTS_MEASURED).enumerate() {
            assert!(one_and_a_half(*budget, measured, 0.05), "{style}: {budget} is not 1.5 × {measured}");
            let mut slow = COLLAGE_LAYOUTS_MEASURED;
            slow[i] = budget + 0.001;
            let e = judge_collage(&out(slow, COLLAGE_RENDER_MEASURED), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("1 of 8 missed: layout {style} 200: median")), "{e}");
        }
        let (render, peak) = COLLAGE_RENDER_MEASURED;
        assert!(one_and_a_half(COLLAGE_RENDER_MS, render, 5.0) && one_and_a_half(COLLAGE_RENDER_PEAK_MIB, peak, 5.0));
        let e = judge_collage(&out(COLLAGE_LAYOUTS_MEASURED, (COLLAGE_RENDER_MS + 1.0, peak)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 8 missed: render 50: median"), "{e}");
        let e = judge_collage(&out(COLLAGE_LAYOUTS_MEASURED, (render, COLLAGE_RENDER_PEAK_MIB + 0.5)), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 8 missed: render 50: peak"), "{e}");
        // 8 ms planted misses every layout (under 2 ms), not the render (seconds); the largest
        // planted slowdown misses the render too.
        let e = judge_collage(&out(COLLAGE_LAYOUTS_MEASURED, COLLAGE_RENDER_MEASURED), 8.0).unwrap_err();
        assert!(e.starts_with("6 of 8 missed") && !e.contains("render 50"), "{e}");
        let e = judge_collage(&out(COLLAGE_LAYOUTS_MEASURED, COLLAGE_RENDER_MEASURED), crate::accept::MAX_PLANT_MS).unwrap_err();
        assert!(e.starts_with("7 of 8 missed") && !e.contains("peak"), "{e}");
        // Another run judges nothing.
        assert!(
            judge_collage(&collage_out("debug", 200, 50, 3000, COLLAGE_LAYOUTS_MEASURED, COLLAGE_RENDER_MEASURED), 0.0)
                .unwrap_err()
                .contains("debug build")
        );
        for (pictures, photos, long) in [(30, 50, 3000), (200, 6, 3000), (200, 50, 1000)] {
            let e =
                judge_collage(&collage_out("release", pictures, photos, long, COLLAGE_LAYOUTS_MEASURED, COLLAGE_RENDER_MEASURED), 0.0).unwrap_err();
            assert!(e.contains("the budgets hold for 200, and 50 at 3000 px"), "{e}");
        }
    }

    #[test]
    fn the_legacy_scan_budgets_judge_each_scan() {
        let full = |values| legacy_scan_out("release", 10_000, 64 << 20, values);
        let ok = judge_legacy_scan(&full(LEGACY_SCAN_MEASURED), 0.0).unwrap();
        assert!(ok.contains("legacy scan 10000") && ok.contains("dalica scan 10000") && ok.contains("legacy file 64 MiB"), "{ok}");
        for (i, ((name, budget), measured)) in LEGACY_SCAN_BUDGETS.iter().zip(LEGACY_SCAN_MEASURED).enumerate() {
            assert!(one_and_a_half(*budget, measured, 5.0), "{name}: {budget} is not 1.5 × {measured}");
            let mut slow = LEGACY_SCAN_MEASURED;
            slow[i] = budget + 0.1;
            let e = judge_legacy_scan(&full(slow), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("1 of 3 missed: {name}: median")), "{e}");
        }
        // 8 ms planted is inside every scan's headroom; the largest planted slowdown misses all.
        assert!(judge_legacy_scan(&full(LEGACY_SCAN_MEASURED), 8.0).is_ok());
        let e = judge_legacy_scan(&full(LEGACY_SCAN_MEASURED), crate::accept::MAX_PLANT_MS).unwrap_err();
        assert!(e.starts_with("3 of 3 missed") && e.contains("ms planted)"), "{e}");
        assert!(judge_legacy_scan(&legacy_scan_out("debug", 10_000, 64 << 20, LEGACY_SCAN_MEASURED), 0.0).unwrap_err().contains("debug build"));
        for (photos, bytes) in [(100, 64 << 20), (10_000, 1 << 20)] {
            let e = judge_legacy_scan(&legacy_scan_out("release", photos, bytes, LEGACY_SCAN_MEASURED), 0.0).unwrap_err();
            assert!(e.contains("the budgets hold for 10000 and 67108864"), "{e}");
        }
    }

    #[test]
    fn the_collage_canvas_budget_judges_a_drag_frame() {
        assert!(one_and_a_half(CANVAS_DRAG_P95_MS, CANVAS_MEASURED, 0.5) && CANVAS_DRAG_P95_MS <= CANVAS_FRAME_CAP_MS);
        let ok = judge_collage_canvas(&collage_canvas_out("release", 100, CANVAS_MEASURED), 0.0).unwrap();
        assert!(ok.starts_with("100 pictures: drag frame 0.17 ms ≤ 0.5 ms"), "{ok}");
        let e = judge_collage_canvas(&collage_canvas_out("release", 100, CANVAS_DRAG_P95_MS + 0.01), 0.0).unwrap_err();
        assert!(e.starts_with("1 of 1 missed: drag frame: p95"), "{e}");
        let e = judge_collage_canvas(&collage_canvas_out("release", 100, CANVAS_MEASURED), 8.0).unwrap_err();
        assert!(e.contains("+ 8.00 ms planted"), "{e}");
        assert!(judge_collage_canvas(&collage_canvas_out("debug", 100, CANVAS_MEASURED), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_collage_canvas(&collage_canvas_out("release", 12, CANVAS_MEASURED), 0.0).unwrap_err().contains("holds for 100"));
    }

    /// The benches' own `--check` budgets are the ones judged here.
    #[test]
    fn the_benches_carry_the_budgets_judged_here() {
        let root = crate::workspace_root();
        let read =
            |krate: &str, bench: &str| std::fs::read_to_string(root.join("crates").join(krate).join("benches").join(format!("{bench}.rs"))).unwrap();
        let import = read("engine", "import");
        let batch = read("engine", "batch_edit");
        let slideshow = read("ui", "slideshow");
        for (source, constant, ours) in [
            (&import, "FIRST_BUDGET_MS", IMPORT_FIRST_MS),
            (&import, "LIST_BUDGET_MS", IMPORT_LIST_MS),
            (&import, "FILES_PER_S_BUDGET", IMPORT_FILES_PER_S),
            (&import, "MB_PER_S_BUDGET", IMPORT_MB_PER_S),
            (&batch, "APPLY_BUDGET_MS", BATCH_APPLY_MS),
            (&batch, "UNDO_BUDGET_MS", BATCH_UNDO_MS),
            (&slideshow, "ADVANCE_BUDGET_MS", SLIDESHOW_ADVANCE_MS),
            (&slideshow, "TRANSITION_BUDGET_MS", SLIDESHOW_TRANSITION_P95_MS),
            (&slideshow, "FRAME_CAP_MS", SLIDESHOW_FRAME_CAP_MS),
        ] {
            assert!(source.contains(&format!("const {constant}: f64 = {ours:?};")), "the bench's {constant} is ours: {ours}");
        }
        // M12.21: the effects' table, row by row, and the tile grids'.
        let effects = std::fs::read_to_string(root.join("crates").join("edit").join("benches").join("effects.rs")).unwrap();
        for (name, scale, draft, final_, save) in EFFECT_BUDGETS {
            let row = format!("(\"{name}\", {scale:?}, {draft:?}, {final_:?}, {save:?}),");
            assert!(effects.contains(&row), "the effects bench's budgets for {name} are ours: {row}");
        }
        assert!(effects.contains(&format!("const DRAFT_CAP_MS: f64 = {EFFECT_DRAFT_CAP_MS:?};")));
        assert!(effects.contains(&format!("const FINAL_CAP_MS: f64 = {EFFECT_FINAL_CAP_MS:?};")));
        assert!(effects.contains(&format!("const EMPTY_FINAL_BUDGET_MS: f64 = {EFFECT_EMPTY_FINAL_MS:?};")));
        assert!(effects.contains(&format!("const EMPTY_SAVE_BUDGET_MS: f64 = {EFFECT_EMPTY_SAVE_MS:?};")));
        for printed in [
            "format!(\"{} draft\", m.name)",
            "format!(\"{} engine draft\", m.name)",
            "format!(\"{} final\", m.name)",
            "format!(\"{} save\", m.name)",
            "row(\"no effect final\"",
            "\\\"draft_scale\\\":{budget_scale}",
        ] {
            assert!(effects.contains(printed), "effects prints {printed}");
        }
        let tiles = read("engine", "effect_tiles");
        let table = TILE_BUDGETS.iter().map(|(t, b)| format!("({t}, {b:?})")).collect::<Vec<_>>().join(", ");
        assert!(tiles.contains(&format!("const TAB_BUDGETS_MS: [(u8, f64); 3] = [{table}];")), "the tile grids' budgets are ours: {table}");
        // M13.31: inference, the face scan, the people and places views.
        let inference = std::fs::read_to_string(root.join("crates").join("faces").join("benches").join("inference.rs")).unwrap();
        let scan = read("engine", "faces_scan");
        let views = read("ui", "people_places");
        let [(_, detect), (_, sface), (_, aura)] = INFERENCE_BUDGETS;
        let [(_, _, open), (_, _, scroll), (_, _, places), (_, _, pan)] = PEOPLE_PLACES_BUDGETS;
        for (source, constant, ours) in [
            (&inference, "DETECT_BUDGET_MS", detect),
            (&inference, "SFACE_BUDGET_MS", sface),
            (&inference, "AURAFACE_BUDGET_MS", aura),
            (&scan, "FACES_PER_S_BUDGET", SCAN_FACES_PER_S),
            (&scan, "WORKER_MIB_BUDGET", SCAN_WORKER_MIB),
            (&scan, "FRAME_BUDGET_MS", SCAN_FRAME_P95_MS),
            (&scan, "FRAME_CAP_MS", SCAN_FRAME_CAP_MS),
            (&views, "UNNAMED_OPEN_BUDGET_MS", open),
            (&views, "UNNAMED_SCROLL_BUDGET_MS", scroll),
            (&views, "PLACES_OPEN_BUDGET_MS", places),
            (&views, "PLACES_PAN_BUDGET_MS", pan),
        ] {
            assert!(source.contains(&format!("const {constant}: f64 = {ours:?};")), "the bench's {constant} is ours: {ours}");
        }
        for printed in ["format!(\"detect {}\", a.side)", "format!(\"embed {name}\")", "\"sface\"", "\"auraface\"", "const SIDE: usize = 640;"] {
            assert!(inference.contains(printed), "inference prints {printed}");
        }
        assert!(scan.contains("format!(\"scan {n}\")") && scan.contains("\"frame while scanning\"") && scan.contains("\"per_worker_mib\""));
        assert!(
            scan.contains(&format!("const PHOTOS: usize = {};", grouped_thousands(SCAN_PHOTOS)))
                && scan.contains(&format!("const SIDE: u32 = {SCAN_SIDE};"))
        );
        for (name, _, _) in PEOPLE_PLACES_BUDGETS {
            assert!(views.contains(&format!("row(\"{name}\"")), "people_places prints `{name}`");
        }
        assert!(views.contains(&format!("const PHOTOS: usize = {};", grouped_thousands(PEOPLE_PLACES_PHOTOS))));
        // M14.22: the collage layouts and render, the legacy scans, the collage canvas.
        let collage = std::fs::read_to_string(root.join("crates").join("create").join("benches").join("collage.rs")).unwrap();
        let legacy = std::fs::read_to_string(root.join("crates").join("scan").join("benches").join("legacy.rs")).unwrap();
        let canvas = read("ui", "collage");
        for (style, budget) in COLLAGE_LAYOUT_BUDGETS {
            assert!(collage.contains(&format!("(\"{style}\", {budget:?})")), "the collage bench's budget for {style} is ours: {budget}");
        }
        let [(_, legacy_scan), (_, dalica_scan), (_, large)] = LEGACY_SCAN_BUDGETS;
        for (source, constant, ours) in [
            (&collage, "RENDER_BUDGET_MS", COLLAGE_RENDER_MS),
            (&collage, "RENDER_PEAK_MIB_BUDGET", COLLAGE_RENDER_PEAK_MIB),
            (&legacy, "LEGACY_SCAN_BUDGET_MS", legacy_scan),
            (&legacy, "DALICA_SCAN_BUDGET_MS", dalica_scan),
            (&legacy, "LARGE_FILE_BUDGET_MS", large),
            (&canvas, "DRAG_BUDGET_MS", CANVAS_DRAG_P95_MS),
            (&canvas, "FRAME_CAP_MS", CANVAS_FRAME_CAP_MS),
        ] {
            assert!(source.contains(&format!("const {constant}: f64 = {ours:?};")), "the bench's {constant} is ours: {ours}");
        }
        assert!(
            collage.contains(&format!("const PICTURES: usize = {COLLAGE_PICTURES};"))
                && collage.contains(&format!("const PHOTOS: usize = {COLLAGE_PHOTOS};"))
                && collage.contains(&format!("const LONG_SIDE: u32 = {COLLAGE_LONG_SIDE};"))
                && collage.contains("format!(\"layout {id} {}\", a.pictures)")
                && collage.contains("format!(\"render {}\", a.photos)")
        );
        assert!(legacy.contains("const FOLDERS: usize = 100;") && legacy.contains("const PER_FOLDER: usize = 100;"));
        assert!(
            legacy.contains("format!(\"legacy scan {n}\")")
                && legacy.contains("format!(\"dalica scan {n}\")")
                && legacy.contains("format!(\"legacy file {mib:.0} MiB\")")
        );
        assert!(canvas.contains(&format!("const PICTURES: usize = {CANVAS_PICTURES};")) && canvas.contains("\"drag frame\""));
    }

    /// `n` as Rust source writes it with `_` between thousands (`1_000`, `10_000`).
    fn grouped_thousands(n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push('_');
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn every_registered_bench_exists_as_a_harnessless_target() {
        let root = crate::workspace_root();
        for b in BENCHES {
            let dir = root.join("crates").join(b.package.trim_start_matches("dalica-"));
            let file = bench_file(b.name);
            assert!(dir.join("benches").join(format!("{file}.rs")).is_file(), "{}", b.name);
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            assert!(manifest.contains(&format!("name = \"{}\"\nharness = false", b.name)), "{}: [[bench]] with harness = false", b.name);
            if file != b.name {
                let target = format!("name = \"{}\"\nharness = false\npath = \"benches/{file}.rs\"", b.name);
                assert!(manifest.contains(&target), "{}: [[bench]] with its path", b.name);
            }
        }
        for (name, _) in BENCH_FILES {
            assert!(BENCHES.iter().any(|b| b.name == name), "{name} is a registered bench");
        }
    }
}
