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
];

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
    let report = release_report("import", stdout)?;
    let photos = report.get("photos").and_then(Value::as_u64).unwrap_or(0);
    let side = report.get("side_px").and_then(Value::as_u64).unwrap_or(0);
    if photos < IMPORT_PHOTOS || side < IMPORT_SIDE_PX {
        return Err(format!("import ran over {photos} photos of {side} px; the budgets hold for {IMPORT_PHOTOS} of {IMPORT_SIDE_PX} px"));
    }
    let n = photos;
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
    let name = format!("import {n}");
    let import_s = row_number("import", &report, &name, "p50_ms")? / 1000.0;
    let bytes = row_number("import", &report, &name, "bytes")?;
    let copied_s = import_s + plant * n as f64 / 1000.0;
    let per_s = n as f64 / copied_s.max(f64::MIN_POSITIVE);
    let mb_s = bytes / 1e6 / copied_s.max(f64::MIN_POSITIVE);
    let note = if plant > 0.0 { format!(" ({import_s:.2} s measured + {plant:.2} ms planted per file)") } else { String::new() };
    if per_s < IMPORT_FILES_PER_S || mb_s < IMPORT_MB_PER_S {
        return Err(format!(
            "the import: {n} files in {copied_s:.2} s{note}: {per_s:.1} files/s, {mb_s:.2} MB/s, under the {IMPORT_FILES_PER_S} files/s and {IMPORT_MB_PER_S} MB/s budget"
        ));
    }
    Ok(format!(
        "{n} photos of {side} px: first screenful {judged:.1} ms ≤ {IMPORT_FIRST_MS} ms, listed in {listed:.0} ms ≤ {IMPORT_LIST_MS} ms, imported in {copied_s:.2} s{note}: {per_s:.1} files/s ≥ {IMPORT_FILES_PER_S}, {mb_s:.2} MB/s ≥ {IMPORT_MB_PER_S}"
    ))
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

/// The effects' budgets (`effects` → `BUDGETS`): per effect, its 24 MP Save render's median, in ms.
/// A draft's and a final's speed on the screen are printed, not budgeted: the slider's 16 ms draft
/// and 100 ms final at p95 are per-effect speed tasks of M13 (the maintainer at M12.R).
pub const EFFECT_BUDGETS: [(&str, f64); 27] = [
    ("sharpen", 1225.0),
    ("sepia", 485.0),
    ("bw", 275.0),
    ("warmify", 1485.0),
    ("grain", 870.0),
    ("tint", 460.0),
    ("saturation", 2060.0),
    ("soft_focus", 1280.0),
    ("glow", 2565.0),
    ("filtered_bw", 240.0),
    ("focal_bw", 505.0),
    ("graduated_tint", 370.0),
    ("infrared", 245.0),
    ("plastic_lens", 2295.0),
    ("hdr", 3775.0),
    ("widescreen", 765.0),
    ("dream_glow", 2250.0),
    ("invert", 240.0),
    ("duotone", 500.0),
    ("boost", 2460.0),
    ("soften", 2700.0),
    ("vignette", 780.0),
    ("pixelate", 670.0),
    ("focal_zoom", 22355.0),
    ("border", 470.0),
    ("drop_shadow", 340.0),
    ("museum_matte", 390.0),
];
/// An empty recipe's final on the screen (p95) and its 24 MP Save render (median) (`effects` →
/// `EMPTY_FINAL_BUDGET_MS`, `EMPTY_SAVE_BUDGET_MS`).
pub const EFFECT_EMPTY_FINAL_MS: f64 = 12.0;
pub const EFFECT_EMPTY_SAVE_MS: f64 = 55.0;
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

/// `effects --json` in release over a 24 MP photo on the 2560 × 1600 screen: every effect's Save
/// render's median and the empty recipe's final p95 and Save median within their budgets (a planted
/// slowdown added once per render); the drafts and finals of the effects are printed by the bench,
/// not judged. Every miss is listed.
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
    checks.extend(EFFECT_BUDGETS.iter().map(|(name, save)| (format!("{name} save"), "p50_ms", *save, "median")));
    let (mut missed, mut worst) = (Vec::new(), 0.0f64);
    for (name, key, budget, stat) in &checks {
        let measured = row_number("effects", &report, name, key)?;
        let judged = measured + plant;
        if judged > *budget {
            missed.push(format!("{name}: {stat} {judged:.2} ms{}, over the {budget} ms budget", planted(measured, plant, "ms")));
        }
        worst = worst.max(judged / budget);
    }
    if !missed.is_empty() {
        return Err(format!("{} of {} missed: {}", missed.len(), checks.len(), missed.join("; ")));
    }
    Ok(format!(
        "{} effects' Saves and the empty recipe within their budgets, the closest at {:.0} % of its own (drafts and finals printed)",
        EFFECT_BUDGETS.len(),
        worst * 100.0
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

    /// The effects' first measurement (M12.21): per effect, its draft's and final's p95 (printed,
    /// not budgeted) and its Save's median, in ms.
    pub const EFFECTS_MEASURED: [(&str, f64, f64, f64); 27] = [
        ("sharpen", 13.801, 223.673, 816.017),
        ("sepia", 4.722, 73.609, 320.319),
        ("bw", 3.098, 48.445, 180.871),
        ("warmify", 21.431, 294.772, 987.811),
        ("grain", 8.702, 139.321, 579.218),
        ("tint", 4.602, 71.512, 303.866),
        ("saturation", 29.037, 455.864, 1372.597),
        ("soft_focus", 41.183, 267.192, 850.319),
        ("glow", 29.391, 540.344, 1706.819),
        ("filtered_bw", 2.222, 34.716, 158.344),
        ("focal_bw", 5.048, 79.907, 333.724),
        ("graduated_tint", 3.619, 56.707, 245.386),
        ("infrared", 2.224, 34.895, 161.591),
        ("plastic_lens", 55.204, 473.580, 1527.933),
        ("hdr", 30.057, 750.487, 2515.803),
        ("widescreen", 21.563, 114.749, 507.836),
        ("dream_glow", 20.598, 355.676, 1498.082),
        ("invert", 2.231, 35.086, 159.155),
        ("duotone", 4.889, 77.492, 333.263),
        ("boost", 35.962, 428.028, 1639.044),
        ("soften", 23.120, 487.722, 1798.765),
        ("vignette", 10.646, 150.981, 519.994),
        ("pixelate", 7.209, 116.296, 446.290),
        ("focal_zoom", 81.393, 2682.801, 14900.034),
        ("border", 21.425, 68.319, 313.240),
        ("drop_shadow", 19.753, 45.839, 223.459),
        ("museum_matte", 20.237, 53.536, 259.663),
    ];
    /// The empty recipe's first measurement: its final's p95 and its Save's median.
    pub const EMPTY_MEASURED: (f64, f64) = (7.500, 33.480);

    /// What `effects --json` prints: the empty recipe's rows from `empty`, each effect's from
    /// `times` (draft p95, final p95, Save median).
    pub fn effects_out(mode: &str, photo: [u64; 2], fit: [u64; 2], empty: (f64, f64), times: &[(&str, f64, f64, f64)]) -> String {
        let mut rows = vec![
            json!({ "name": "no effect final", "p50_ms": empty.0 / 2.0, "p95_ms": empty.0 }),
            json!({ "name": "no effect save", "p50_ms": empty.1, "p95_ms": empty.1 * 1.1 }),
        ];
        for (name, draft, final_, save) in times {
            rows.push(json!({ "name": format!("{name} draft"), "p50_ms": draft / 2.0, "p95_ms": draft, "draft_scale": 0.25 }));
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
    fn the_effects_budgets_judge_every_save_and_the_empty_recipe() {
        let (photo, screen) = (EFFECT_PHOTO, EFFECT_SCREEN);
        let ok = judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED), 0.0).unwrap();
        assert!(ok.starts_with("27 effects' Saves and the empty recipe within their budgets"), "{ok}");
        // Each budget is 1.5 × the first measurement rounded up (5 ms; the empty final 1 ms), in
        // the grids' order.
        for ((name, budget), (m, _, _, measured)) in EFFECT_BUDGETS.iter().zip(EFFECTS_MEASURED) {
            assert_eq!(*name, m);
            assert!(*budget >= 1.5 * measured && *budget < 1.5 * measured + 5.0, "{name}: {budget} is not 1.5 × {measured} rounded up");
        }
        assert!(EFFECT_EMPTY_FINAL_MS >= 1.5 * EMPTY_MEASURED.0 && EFFECT_EMPTY_FINAL_MS < 1.5 * EMPTY_MEASURED.0 + 1.0);
        assert!(EFFECT_EMPTY_SAVE_MS >= 1.5 * EMPTY_MEASURED.1 && EFFECT_EMPTY_SAVE_MS < 1.5 * EMPTY_MEASURED.1 + 5.0);
        // Drafts and finals are not judged: a slow one passes (the caps are M13's speed tasks).
        let mut slow = EFFECTS_MEASURED;
        slow[0].1 = 500.0;
        slow[0].2 = 5000.0;
        assert!(judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, &slow), 0.0).is_ok());
        // A slow Save, or a slow empty recipe, misses; every miss is listed.
        let mut slow = EFFECTS_MEASURED;
        slow[1].3 *= 2.0;
        slow[26].3 *= 2.0;
        let e = judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, &slow), 0.0).unwrap_err();
        assert!(e.starts_with("2 of 29 missed: sepia save: median") && e.contains("museum_matte save: median"), "{e}");
        let e = judge_effects(&effects_out("release", photo, screen, (EFFECT_EMPTY_FINAL_MS + 0.5, EMPTY_MEASURED.1), &EFFECTS_MEASURED), 0.0)
            .unwrap_err();
        assert!(e.starts_with("1 of 29 missed: no effect final: p95"), "{e}");
        // A planted slowdown of 8 ms per render misses the empty final (1.5 × a few ms).
        let e = judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED), 8.0).unwrap_err();
        assert!(e.contains("no effect final: p95") && e.contains("measured + 8.00 ms planted"), "{e}");
        // A missing effect, a debug build, a small photo or another screen judge nothing.
        let e = judge_effects(&effects_out("release", photo, screen, EMPTY_MEASURED, &EFFECTS_MEASURED[1..]), 0.0).unwrap_err();
        assert!(e.contains("no row `sharpen save`"), "{e}");
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
        for (name, save) in EFFECT_BUDGETS {
            assert!(effects.contains(&format!("(\"{name}\", {save:?}),")), "the effects bench's budget for {name} is ours");
        }
        assert!(effects.contains(&format!("const EMPTY_FINAL_BUDGET_MS: f64 = {EFFECT_EMPTY_FINAL_MS:?};")));
        assert!(effects.contains(&format!("const EMPTY_SAVE_BUDGET_MS: f64 = {EFFECT_EMPTY_SAVE_MS:?};")));
        assert!(effects.contains("format!(\"{} save\", m.name)") && effects.contains("row(\"no effect final\""));
        let tiles = read("engine", "effect_tiles");
        let table = TILE_BUDGETS.iter().map(|(t, b)| format!("({t}, {b:?})")).collect::<Vec<_>>().join(", ");
        assert!(tiles.contains(&format!("const TAB_BUDGETS_MS: [(u8, f64); 3] = [{table}];")), "the tile grids' budgets are ours: {table}");
    }

    #[test]
    fn every_registered_bench_exists_as_a_harnessless_target() {
        let root = crate::workspace_root();
        for b in BENCHES {
            let dir = root.join("crates").join(b.package.trim_start_matches("dalica-"));
            assert!(dir.join("benches").join(format!("{}.rs", b.name)).is_file(), "{}", b.name);
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            assert!(manifest.contains(&format!("name = \"{}\"\nharness = false", b.name)), "{}: [[bench]] with harness = false", b.name);
        }
    }
}
