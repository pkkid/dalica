//! `cargo xtask accept M<n> [--headless]`: runs a milestone's acceptance test
//! (`cargo test -p dalica-acceptance --test m<n>`, or with `--headless` its headless half
//! `m<n>_headless` when there is one) and prints its manual checks (`tests/acceptance/<test>.md`)
//! (`docs/roadmap.md` → How milestones work).

use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// `M1` / `m1` → 1.
pub fn parse_milestone(arg: &str) -> Result<u32, String> {
    arg.strip_prefix(['M', 'm'])
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n < 1000)
        .ok_or_else(|| format!("expected a milestone like M1, not `{arg}`"))
}

/// Set to `1` on the acceptance test when the crate suites it would re-run already ran in the same
/// `ci` (`tests/acceptance/src/lib.rs` → `SUITES_RAN`).
pub const SUITES_RAN: &str = "DALICA_SUITES_RAN";

/// Runs the test; `quiet` skips printing the manual checks (used by `ci`). `suites_ran` (set by
/// `ci`, whose test step ran first) lets the test skip the crate suites it would re-run; otherwise
/// [`SUITES_RAN`] is cleared, so a standalone run checks everything.
pub fn accept(root: &Path, milestone: u32, headless: bool, quiet: bool, suites_ran: bool) -> Result<(), String> {
    let dir = root.join("tests").join("acceptance");
    // `--headless` runs the milestone's headless half (`m<n>_headless`) when it has one.
    let name = match format!("m{milestone}_headless") {
        h if headless && dir.join(format!("{h}.rs")).is_file() => h,
        _ => format!("m{milestone}"),
    };
    let test = dir.join(format!("{name}.rs"));
    if !test.is_file() {
        return Err(format!("M{milestone} has no acceptance test yet ({} is missing)", test.display()));
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(["test", "-p", "dalica-acceptance", "--test", &name, "--", "--nocapture"]);
    if headless {
        cmd.env("DALICA_ACCEPT_HEADLESS", "1");
    }
    if suites_ran {
        cmd.env(SUITES_RAN, "1");
    } else {
        cmd.env_remove(SUITES_RAN);
    }
    let started = Instant::now();
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    let secs = started.elapsed().as_secs_f64();
    if !status.success() {
        return Err(format!("accept M{milestone}: FAILED after {secs:.1} s"));
    }
    println!("accept M{milestone}: passed in {secs:.1} s");
    if !quiet {
        match std::fs::read_to_string(dir.join(format!("{name}.md"))) {
            Ok(manual) => println!("\nWhat a human should look at (tests/acceptance/{name}.md):\n\n{manual}"),
            Err(_) => println!("(no tests/acceptance/{name}.md with manual checks)"),
        }
    }
    Ok(())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let milestone = parse_milestone(args.first().map(String::as_str).ok_or("usage: cargo xtask accept M<n> [--headless]")?)?;
    let headless = args.iter().any(|a| a == "--headless");
    accept(root, milestone, headless, false, false)
}

// ---------------------------------------------------------------------------------------------
// `cargo xtask budgets`: the release frame budgets (`.github/workflows/budgets.yml`)
// ---------------------------------------------------------------------------------------------

/// The UI frame budget, p95, in milliseconds (`docs/development.md` §8; `tests/acceptance/m4_thumb.rs`
/// → `BUDGET_MS`).
pub const FRAME_BUDGET_MS: f64 = 8.0;
/// The largest planted slowdown `--plant-ms` takes.
pub const MAX_PLANT_MS: f64 = 1000.0;

/// A budgeted measurement the `budgets` job runs in a release build: a cargo command and the judge
/// of what it printed. `judge(stdout, plant_ms)` returns the line to print when the budget holds
/// and the failure otherwise; `plant_ms` is a planted slowdown added to every measured frame.
pub struct Budget {
    /// The milestone whose acceptance test asserts the same budget with `DALICA_ACCEPT_RELEASE=1`.
    pub milestone: u32,
    pub name: &'static str,
    pub what: &'static str,
    pub cargo: &'static [&'static str],
    pub judge: fn(&str, f64) -> Result<String, String>,
}

/// The release budget test of `tests/acceptance/m4.rs` (part 2).
const THUMB_TEST: &str = "the_thumb_held_at_top_speed_through_100k_sketched_photos_never_shows_a_placeholder";

/// The first screenful's budget after adding a folder (`docs/development.md` §8 for 1 000 JPEGs,
/// the roadmap's M5 for 10 000), in milliseconds (`first_thumbnails` → `FIRST_BUDGET_MS`).
pub const FIRST_THUMBNAILS_MS: f64 = 1000.0;

/// The folder sizes the per-push job adds, and the photos its scanned pass scrolls through. The
/// full 100 k pass (`first_thumbnails`' default) writes about 1 GB more and takes minutes; it runs
/// with `DALICA_ACCEPT_RELEASE=1 cargo xtask accept M5` and `cargo xtask bench first_thumbnails`.
const FIRST_PHOTOS: &str = "1000,10000";
const SCROLL_PHOTOS: &str = "20000";

/// The budgets, in the order the job runs them. Later milestones add theirs here.
pub const BUDGETS: &[Budget] = &[
    Budget {
        milestone: 4,
        name: "light table scroll",
        what: "library_bench in full: the 100 k scroll's UI frame time p95 ≤ 8 ms; cells visited per frame flat from 10 k to 100 k",
        cargo: &["bench", "-p", "dalica-ui", "--bench", "library_bench", "--", "--json"],
        judge: judge_scroll,
    },
    Budget {
        milestone: 4,
        name: "thumb at top speed",
        what: "the speed thumb held at its top speed through 100 k sketched photos: UI frame time p95 ≤ 8 ms, cells flat, no placeholder",
        cargo: &["test", "--release", "-p", "dalica-acceptance", "--test", "m4", "--", "--exact", THUMB_TEST, "--nocapture"],
        judge: judge_thumb,
    },
    Budget {
        milestone: 5,
        name: "first thumbnails",
        what: "first_thumbnails: the first screenful drawn ≤ 1 s after adding a folder of 1 k and of 10 k JPEGs (1024 px, EXIF thumbnails); the full scan's time and photos per second (writes about 1.1 GB, removed after)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", FIRST_PHOTOS, "--scroll", "0", "--json"],
        judge: judge_first_thumbnails,
    },
    Budget {
        milestone: 5,
        name: "scanned scroll",
        what: "first_thumbnails' scanned pass: 20 k small JPEGs scanned into the thumbnail cache, then the light table's requests at the speed thumb's top speed from top to bottom: 0 placeholder cells, owner time per frame p95 ≤ 8 ms (the 100 k pass: DALICA_ACCEPT_RELEASE=1 cargo xtask accept M5)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", "0", "--scroll", SCROLL_PHOTOS, "--json"],
        judge: judge_scanned_scroll,
    },
];

/// `p95 + plant` against [`FRAME_BUDGET_MS`]: a planted slowdown of `plant` ms in every frame moves
/// every percentile of the frame times by exactly `plant`.
fn within_frame_budget(what: &str, p95: f64, plant: f64) -> Result<f64, String> {
    let judged = p95 + plant;
    let planted = if plant > 0.0 { format!(" ({p95:.2} ms measured + {plant:.2} ms planted)") } else { String::new() };
    if !judged.is_finite() || judged > FRAME_BUDGET_MS {
        return Err(format!("{what}: UI frame time p95 {judged:.2} ms{planted}, over the {FRAME_BUDGET_MS} ms budget"));
    }
    Ok(judged)
}

/// `library_bench --json` in release: the 100 k scroll's p95 within the frame budget and the cells
/// visited per frame flat (the bench's own verdict, and its counters read here, as `m4.rs` does).
pub fn judge_scroll(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("library_bench printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("library_bench ran in a {mode} build; the budgets hold in release only"));
    }
    let row = |name: &str| {
        report
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.iter().find(|r| r.get("name").and_then(serde_json::Value::as_str) == Some(name)))
            .ok_or(format!("library_bench: no row `{name}`"))
    };
    let (small, big) = (row("scroll light table 10k")?, row("scroll light table 100k")?);
    let cells = |r: &serde_json::Value| r.pointer("/counters/cells_visited_max").and_then(serde_json::Value::as_u64);
    let (Some(c_small), Some(c_big)) = (cells(small), cells(big)) else {
        return Err("library_bench: the scroll rows have no `cells_visited_max`".into());
    };
    if report.get("cells_flat") != Some(&serde_json::Value::Bool(true)) || c_small == 0 || c_big > c_small + c_small / 10 {
        return Err(format!("library_bench: the cells visited per frame grew with the library: at most {c_small} at 10 k, {c_big} at 100 k"));
    }
    let p95 = big.get("p95_ms").and_then(serde_json::Value::as_f64).ok_or("library_bench: the 100 k scroll has no `p95_ms`")?;
    let judged = within_frame_budget("the 100 k scroll", p95, plant)?;
    let p50 = big.get("p50_ms").and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN);
    let samples = big.get("samples").and_then(serde_json::Value::as_u64).unwrap_or(0);
    Ok(format!(
        "100 k scroll p50 {p50:.2} ms, p95 {judged:.2} ms ≤ {FRAME_BUDGET_MS} ms over {samples} frames; cells visited per frame at most {c_small} at 10 k, {c_big} at 100 k"
    ))
}

/// The number after `key` in `line` (`"p95 1.23 ms"` → 1.23).
fn number_after(line: &str, key: &str) -> Option<f64> {
    let rest = line.get(line.find(key)? + key.len()..)?;
    rest.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.parse().ok()
}

/// The release thumb test of `m4.rs`: it passed (it asserts every frame, and the budget itself in
/// release), and the p95 it printed is within the frame budget.
pub fn judge_thumb(stdout: &str, plant: f64) -> Result<String, String> {
    let line = stdout.lines().find(|l| l.starts_with("thumb at top speed (")).ok_or("the thumb test printed no `thumb at top speed` line")?;
    if !line.starts_with("thumb at top speed (release)") {
        return Err(format!("the thumb test ran in a debug build; the budgets hold in release only: {line}"));
    }
    if !stdout.lines().any(|l| l.starts_with("test result: ok. 1 passed")) {
        return Err("the thumb test did not pass".into());
    }
    let p95 = number_after(line, "UI time p50 ").and(number_after(line, ", p95 ")).ok_or(format!("no UI time p95 in: {line}"))?;
    let judged = within_frame_budget("the thumb at top speed", p95, plant)?;
    let p50 = number_after(line, "UI time p50 ").unwrap_or(f64::NAN);
    let cells = number_after(line, "cells visited at most ").unwrap_or(f64::NAN);
    Ok(format!("thumb at top speed through 100 k: p50 {p50:.2} ms, p95 {judged:.2} ms ≤ {FRAME_BUDGET_MS} ms; cells visited at most {cells}"))
}

/// A row of a `first_thumbnails` report by name.
fn report_row<'a>(report: &'a serde_json::Value, name: &str) -> Result<&'a serde_json::Value, String> {
    report
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.iter().find(|r| r.get("name").and_then(serde_json::Value::as_str) == Some(name)))
        .ok_or(format!("first_thumbnails: no row `{name}`"))
}

fn field(row: &serde_json::Value, key: &str) -> Result<f64, String> {
    row.get(key).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite()).ok_or(format!("first_thumbnails: a row without `{key}`"))
}

/// `first_thumbnails --photos 1000,10000 --scroll 0 --json`: each folder's first screenful within
/// [`FIRST_THUMBNAILS_MS`]. A planted slowdown of `plant` ms per frame delays the frame that finds
/// the screenful complete by `plant`, so it is added to each time (the frames before it are paced
/// at 60 per second and the decoding runs on workers, so it does not add up per frame).
pub fn judge_first_thumbnails(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("first_thumbnails printed no JSON report with `rows`")?;
    let mut parts = Vec::new();
    for n in FIRST_PHOTOS.split(',') {
        let row = report_row(&report, &format!("first thumbnails {n}"))?;
        let first = field(row, "first_ms")?;
        let judged = first + plant;
        let planted = if plant > 0.0 { format!(" ({first:.0} ms measured + {plant:.2} ms planted)") } else { String::new() };
        if judged > FIRST_THUMBNAILS_MS {
            return Err(format!("{n} JPEGs: the first screenful after {judged:.0} ms{planted}, over the {FIRST_THUMBNAILS_MS} ms budget"));
        }
        let (scan, per_s, thumbs) = (field(row, "scan_s")?, field(row, "scan_photos_per_s")?, field(row, "thumbs_s")?);
        parts.push(format!(
            "{n} JPEGs: first screenful {judged:.0} ms{planted}, scan {scan:.2} s ({per_s:.0} photos/s), every thumbnail {thumbs:.1} s"
        ));
    }
    Ok(format!("{} (budget {FIRST_THUMBNAILS_MS} ms)", parts.join("; ")))
}

/// `first_thumbnails --photos 0 --scroll 20000 --json`: no placeholder cell at the top thumb
/// speed, and the owner thread's time per frame (p95) within the frame budget.
pub fn judge_scanned_scroll(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("first_thumbnails printed no JSON report with `rows`")?;
    let row = report_row(&report, &format!("scanned scroll {SCROLL_PHOTOS}"))?;
    let placeholders = row.get("placeholder_cells").and_then(serde_json::Value::as_u64).ok_or("first_thumbnails: no `placeholder_cells`")?;
    if placeholders > 0 {
        return Err(format!("the scanned {SCROLL_PHOTOS}-photo pass drew {placeholders} placeholder cells at the top thumb speed (must be 0)"));
    }
    let judged = within_frame_budget(&format!("the scanned {SCROLL_PHOTOS}-photo pass"), field(row, "frame_p95_ms")?, plant)?;
    let (p50, frames, scan, per_s) = (field(row, "frame_p50_ms")?, field(row, "frames")?, field(row, "scan_s")?, field(row, "scan_photos_per_s")?);
    Ok(format!(
        "scanned {SCROLL_PHOTOS} at the top thumb speed: 0 placeholder cells over {frames:.0} frames, owner time p50 {p50:.2} ms, p95 {judged:.2} ms ≤ {FRAME_BUDGET_MS} ms; scanned with its thumbnails in {scan:.1} s ({per_s:.0} photos/s)"
    ))
}

/// `--plant-ms N`: the planted slowdown, 0 without the flag.
pub fn parse_budget_args(args: &[String]) -> Result<f64, String> {
    let usage = "usage: cargo xtask budgets [--plant-ms N]";
    match args {
        [] => Ok(0.0),
        [flag, n] if flag == "--plant-ms" => n
            .parse::<f64>()
            .ok()
            .filter(|n| (0.0..=MAX_PLANT_MS).contains(n))
            .ok_or_else(|| format!("--plant-ms takes milliseconds from 0 to {MAX_PLANT_MS}, not `{n}`")),
        _ => Err(usage.to_string()),
    }
}

/// Runs every budget with `cargo` (`cargo(args)` → `(succeeded, stdout)`), judges each, prints a
/// line per budget and fails when any is missed. Every budget runs, so the log shows all numbers.
pub fn budgets_with(plant: f64, mut cargo: impl FnMut(&[&str]) -> Result<(bool, String), String>) -> Result<(), String> {
    if plant > 0.0 {
        println!("budgets: a planted slowdown of {plant} ms per frame (--plant-ms)");
    }
    let mut missed = Vec::new();
    for b in BUDGETS {
        println!("==> budgets: M{} {}: {}\n    cargo {}", b.milestone, b.name, b.what, b.cargo.join(" "));
        let started = Instant::now();
        let verdict = match cargo(b.cargo) {
            Ok((true, stdout)) => (b.judge)(&stdout, plant),
            Ok((false, stdout)) => {
                print!("{stdout}");
                Err(format!("cargo {} failed", b.cargo.join(" ")))
            }
            Err(e) => Err(e),
        };
        let secs = started.elapsed().as_secs_f64();
        match verdict {
            Ok(line) => println!("  ok ({secs:.1} s): {line}"),
            Err(e) => {
                println!("  MISSED ({secs:.1} s): {e}");
                missed.push(format!("M{} {}: {e}", b.milestone, b.name));
            }
        }
    }
    if missed.is_empty() {
        println!("budgets: all {} held", BUDGETS.len());
        Ok(())
    } else {
        Err(format!("budgets: {} of {} missed:\n  {}", missed.len(), BUDGETS.len(), missed.join("\n  ")))
    }
}

/// `cargo xtask budgets [--plant-ms N]`: builds in release and runs [`BUDGETS`].
pub fn budgets(root: &Path, args: &[String]) -> Result<(), String> {
    let plant = parse_budget_args(args)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    budgets_with(plant, |args| {
        let out = Command::new(&cargo)
            .current_dir(root)
            .args(args)
            .env_remove("DALICA_BLESS")
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestones() {
        assert_eq!(parse_milestone("M0"), Ok(0));
        assert_eq!(parse_milestone("m12"), Ok(12));
        assert!(parse_milestone("M").is_err());
        assert!(parse_milestone("X1").is_err());
        assert!(parse_milestone("M1.2").is_err());
    }

    #[test]
    fn missing_test_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(accept(dir.path(), 999, false, true, false).unwrap_err().contains("no acceptance test"));
        // Without a headless half, --headless still looks for the milestone's own test.
        assert!(accept(dir.path(), 999, true, true, true).unwrap_err().contains("m999.rs is missing"));
    }

    // -- budgets ------------------------------------------------------------------------------

    /// What `cargo bench … library_bench -- --json` prints, with the 100 k scroll's p95.
    fn bench_out(mode: &str, p95: f64, cells_10k: u64, cells_100k: u64) -> String {
        let flat = cells_100k <= cells_10k + cells_10k / 10;
        let row = |n: &str, p95: f64, cells: u64| {
            serde_json::json!({ "name": format!("scroll light table {n}"), "samples": 2400, "p50_ms": p95 / 2.0, "p95_ms": p95, "max_ms": p95 * 3.0,
                "counters": { "cells_visited_min": 30, "cells_visited_p50": 35, "cells_visited_max": cells } })
        };
        let report = serde_json::json!({ "bench": "library_bench", "mode": mode, "cells_flat": flat,
            "rows": [row("10k", 1.0, cells_10k), { "name": "layout 100k", "p95_ms": 0.1 }, row("100k", p95, cells_100k)] });
        format!("running 0 tests\n{report}\n")
    }

    /// What the release thumb test prints (`m4.rs`), with its p95.
    fn thumb_out(mode: &str, p95: f64, passed: bool) -> String {
        format!(
            "running 1 test\nthumb at top speed ({mode}): 100000 photos, 2900 screens in 4400 frames, 180000 sketched cells, 0 placeholders; \
             cells visited at most 48 (bound 54); UI time p50 {:.2} ms, p95 {p95:.2} ms, max {:.2} ms; 30.1 s\ntest the_thumb ... ok\n\n\
             test result: {}. 1 passed; 0 failed\n",
            p95 / 2.0,
            p95 * 3.0,
            if passed { "ok" } else { "FAILED" }
        )
    }

    /// What `first_thumbnails --photos 1000,10000 --scroll 0 --json` prints.
    fn first_out(first_1k: f64, first_10k: f64) -> String {
        let row = |n: u32, first: f64| {
            serde_json::json!({ "name": format!("first thumbnails {n}"), "first_ms": first, "budget_ms": 1000.0, "scan_s": f64::from(n) / 20_000.0,
                "scan_photos_per_s": 20_000.0, "thumbs_s": f64::from(n) / 1500.0, "thumbs_photos_per_s": 1500.0, "frame_p95_ms": 1.0 })
        };
        format!("{}\n", serde_json::json!({ "rows": [row(1000, first_1k), row(10000, first_10k)] }))
    }

    /// What `first_thumbnails --photos 0 --scroll 20000 --json` prints.
    fn scroll_out(p95: f64, placeholders: u64) -> String {
        let row = serde_json::json!({ "name": "scanned scroll 20000", "scan_s": 6.0, "scan_photos_per_s": 3333.0, "frames": 1130,
            "placeholder_cells": placeholders, "frame_p50_ms": p95 / 3.0, "frame_p95_ms": p95, "frame_max_ms": p95 * 2.0, "budget_ms": 8.0 });
        format!("20000 photos written in 5.1 s\n{}\n", serde_json::json!({ "rows": [row] }))
    }

    #[test]
    fn the_first_thumbnails_budget_holds_within_a_second_and_a_planted_slowdown_adds_to_it() {
        let line = judge_first_thumbnails(&first_out(100.0, 517.0), 0.0).unwrap();
        assert!(line.contains("1000 JPEGs: first screenful 100 ms") && line.contains("10000 JPEGs: first screenful 517 ms"), "{line}");
        assert!(line.contains("scan 0.50 s (20000 photos/s)") && line.contains("(budget 1000 ms)"), "{line}");
        assert!(judge_first_thumbnails(&first_out(100.0, 1000.0), 0.0).is_ok(), "the budget itself holds");
        let e = judge_first_thumbnails(&first_out(100.0, 1001.0), 0.0).unwrap_err();
        assert!(e.contains("10000 JPEGs: the first screenful after 1001 ms, over the 1000 ms budget"), "{e}");
        assert!(judge_first_thumbnails(&first_out(1200.0, 500.0), 0.0).unwrap_err().contains("1000 JPEGs"), "either folder misses it");
        assert!(judge_first_thumbnails(&first_out(100.0, 517.0), 8.0).is_ok(), "8 ms planted per frame delays a fast screenful by 8 ms");
        let e = judge_first_thumbnails(&first_out(100.0, 995.0), 8.0).unwrap_err();
        assert!(e.contains("(995 ms measured + 8.00 ms planted)"), "{e}");
        assert!(judge_first_thumbnails("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        assert!(judge_first_thumbnails("{\"rows\":[]}", 0.0).unwrap_err().contains("no row `first thumbnails 1000`"));
        let no_time = "{\"rows\":[{\"name\":\"first thumbnails 1000\",\"first_ms\":null}]}";
        assert!(judge_first_thumbnails(no_time, 0.0).unwrap_err().contains("without `first_ms`"));
    }

    #[test]
    fn the_scanned_scroll_budget_needs_no_placeholder_and_its_frames_within_the_budget() {
        let line = judge_scanned_scroll(&scroll_out(0.45, 0), 0.0).unwrap();
        assert!(line.contains("0 placeholder cells over 1130 frames") && line.contains("p95 0.45 ms ≤ 8 ms"), "{line}");
        assert!(judge_scanned_scroll(&scroll_out(0.45, 3), 0.0).unwrap_err().contains("3 placeholder cells"));
        assert!(judge_scanned_scroll(&scroll_out(8.5, 0), 0.0).unwrap_err().contains("over the 8 ms budget"));
        let e = judge_scanned_scroll(&scroll_out(0.45, 0), 8.0).unwrap_err();
        assert!(e.contains("p95 8.45 ms (0.45 ms measured + 8.00 ms planted)"), "{e}");
        assert!(judge_scanned_scroll(&first_out(1.0, 1.0), 0.0).unwrap_err().contains("no row `scanned scroll 20000`"));
    }

    #[test]
    fn the_scroll_budget_holds_on_a_fast_release_run() {
        let line = judge_scroll(&bench_out("release", 2.5, 40, 42), 0.0).unwrap();
        assert!(line.contains("p95 2.50 ms ≤ 8 ms") && line.contains("at most 40 at 10 k, 42 at 100 k"), "{line}");
        assert!(judge_scroll(&bench_out("release", 8.0, 40, 40), 0.0).is_ok(), "the budget itself holds");
    }

    #[test]
    fn a_planted_slowdown_misses_the_scroll_budget() {
        let out = bench_out("release", 2.5, 40, 42);
        assert!(judge_scroll(&out, 5.5).is_ok(), "2.5 + 5.5 = 8 holds");
        let e = judge_scroll(&out, 6.0).unwrap_err();
        assert!(e.contains("p95 8.50 ms (2.50 ms measured + 6.00 ms planted), over the 8 ms budget"), "{e}");
        assert!(judge_scroll(&bench_out("release", 8.01, 40, 40), 0.0).unwrap_err().contains("over the 8 ms budget"));
        assert!(judge_scroll(&bench_out("release", f64::INFINITY, 40, 40), 0.0).is_err());
    }

    #[test]
    fn the_scroll_budget_needs_flat_cells_a_release_build_and_a_report() {
        assert!(judge_scroll(&bench_out("release", 2.0, 40, 45), 0.0).unwrap_err().contains("grew with the library"));
        assert!(judge_scroll(&bench_out("release", 2.0, 0, 0), 0.0).unwrap_err().contains("grew with the library"), "no cells counted");
        assert!(judge_scroll(&bench_out("debug", 2.0, 40, 40), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_scroll("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        let no_big = "{\"mode\":\"release\",\"cells_flat\":true,\"rows\":[{\"name\":\"scroll light table 10k\"}]}";
        assert!(judge_scroll(no_big, 0.0).unwrap_err().contains("no row `scroll light table 100k`"));
    }

    #[test]
    fn the_thumb_budget_holds_on_a_fast_release_run_and_a_planted_slowdown_misses_it() {
        let out = thumb_out("release", 3.25, true);
        let line = judge_thumb(&out, 0.0).unwrap();
        assert!(line.contains("p50 1.62 ms, p95 3.25 ms ≤ 8 ms") && line.contains("at most 48"), "{line}");
        let e = judge_thumb(&out, 5.0).unwrap_err();
        assert!(e.contains("p95 8.25 ms (3.25 ms measured + 5.00 ms planted), over the 8 ms budget"), "{e}");
        assert!(judge_thumb(&thumb_out("release", 9.0, true), 0.0).unwrap_err().contains("over the 8 ms budget"));
        assert!(judge_thumb(&thumb_out("debug", 3.0, true), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_thumb(&thumb_out("release", 3.0, false), 0.0).unwrap_err().contains("did not pass"));
        assert!(judge_thumb("running 0 tests\n", 0.0).unwrap_err().contains("no `thumb at top speed` line"));
        let garbled = "thumb at top speed (release): UI time p50 x ms\ntest result: ok. 1 passed;";
        assert!(judge_thumb(garbled, 0.0).unwrap_err().contains("no UI time p95"));
    }

    #[test]
    fn plant_ms_is_a_bounded_number() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_budget_args(&[]), Ok(0.0));
        assert_eq!(parse_budget_args(&s(&["--plant-ms", "8"])), Ok(8.0));
        assert_eq!(parse_budget_args(&s(&["--plant-ms", "0.5"])), Ok(0.5));
        for bad in ["-1", "NaN", "inf", "1001", "x", ""] {
            assert!(parse_budget_args(&s(&["--plant-ms", bad])).unwrap_err().contains("from 0 to 1000"), "{bad}");
        }
        assert!(parse_budget_args(&s(&["--plant-ms"])).unwrap_err().contains("usage"));
        assert!(parse_budget_args(&s(&["--fast"])).unwrap_err().contains("usage"));
    }

    /// The job end to end with cargo's outputs planted: today's numbers pass; a planted slowdown,
    /// a slow measurement, a failed command or a missing cargo each fail the run, and every budget
    /// still runs.
    #[test]
    fn the_job_fails_on_a_planted_slowdown_and_passes_without() {
        let fake = |scroll_p95: f64, thumb_p95: f64, first_10k: f64| {
            move |args: &[&str]| -> Result<(bool, String), String> {
                Ok(if args.contains(&"library_bench") {
                    (true, bench_out("release", scroll_p95, 40, 42))
                } else if args.contains(&"first_thumbnails") && args.contains(&"0") && args.contains(&"20000") {
                    (true, scroll_out(0.45, 0))
                } else if args.contains(&"first_thumbnails") {
                    (true, first_out(100.0, first_10k))
                } else {
                    (true, thumb_out("release", thumb_p95, true))
                })
            }
        };
        assert_eq!(budgets_with(0.0, fake(2.0, 3.0, 517.0)), Ok(()));
        // 8 ms planted per frame: every frame budget misses; a first screenful well inside its
        // second does not.
        let e = budgets_with(8.0, fake(2.0, 3.0, 517.0)).unwrap_err();
        assert!(
            e.contains("3 of 4 missed")
                && e.contains("M4 light table scroll")
                && e.contains("M4 thumb at top speed")
                && e.contains("M5 scanned scroll"),
            "{e}"
        );
        let e = budgets_with(0.0, fake(2.0, 8.5, 517.0)).unwrap_err();
        assert!(e.contains("1 of 4 missed") && e.contains("M4 thumb at top speed"), "{e}");
        let e = budgets_with(0.0, fake(2.0, 3.0, 1100.0)).unwrap_err();
        assert!(e.contains("1 of 4 missed") && e.contains("M5 first thumbnails: 10000 JPEGs"), "{e}");
        let mut ran = Vec::new();
        let e = budgets_with(0.0, |args: &[&str]| {
            ran.push(args.join(" "));
            Ok((false, String::new()))
        })
        .unwrap_err();
        assert!(e.contains("4 of 4 missed") && e.contains("failed"), "{e}");
        assert_eq!(ran.len(), BUDGETS.len(), "every budget runs after one fails");
        assert!(budgets_with(0.0, |_: &[&str]| Err("cannot run cargo: gone".to_string())).unwrap_err().contains("cannot run cargo"));
    }

    /// The commands name targets that exist: the bench is registered, the test is in `m4.rs`.
    #[test]
    fn every_budget_runs_a_target_that_exists_in_release() {
        let root = crate::workspace_root();
        for b in BUDGETS {
            assert!(b.cargo.first() == Some(&"bench") || b.cargo.contains(&"--release"), "{}: a release build", b.name);
        }
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "library_bench" && b.package == "dalica-ui"));
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "first_thumbnails" && b.package == "dalica-engine"));
        let bench = std::fs::read_to_string(root.join("crates").join("engine").join("benches").join("first_thumbnails.rs")).unwrap();
        for printed in [
            "\"name\": format!(\"first thumbnails {n}\")",
            "\"name\": format!(\"scanned scroll {n}\")",
            "\"first_ms\"",
            "\"placeholder_cells\"",
            "\"frame_p95_ms\"",
        ] {
            assert!(bench.contains(printed), "first_thumbnails prints {printed}");
        }
        assert!(bench.contains("const FIRST_BUDGET_MS: f64 = 1000.0;"), "the bench's budget is ours: {FIRST_THUMBNAILS_MS}");
        let m4 = std::fs::read_to_string(root.join("tests").join("acceptance").join("m4.rs")).unwrap();
        assert!(m4.contains(&format!("fn {THUMB_TEST}()")), "m4.rs has the thumb test");
        assert!(m4.contains("\"thumb at top speed ({mode}): ") && m4.contains("UI time p50 {:.2} ms, p95 {:.2} ms"), "the line judge_thumb reads");
        assert!(crate::COMMANDS.contains(&"budgets"));
    }
}
