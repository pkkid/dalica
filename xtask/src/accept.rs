//! `cargo xtask accept M<n> [--headless]`: runs a milestone's acceptance test
//! (`cargo test -p dalica-acceptance --test m<n>`, or with `--headless` its headless half
//! `m<n>_headless` when there is one) and prints its manual checks (`tests/acceptance/<test>.md`)
//! (`docs/roadmap.md` → How milestones work).

use std::collections::BTreeMap;
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
/// [`SUITES_RAN`] is cleared, so a standalone run checks everything. A standalone run is at full
/// strength (`ci::FULL_TESTS`), as in the review gate.
pub fn accept(root: &Path, milestone: u32, headless: bool, quiet: bool, suites_ran: bool) -> Result<(), String> {
    accept_with(root, milestone, headless, quiet, suites_ran, true)
}

/// [`accept`], with the crate suites it runs at full strength or not (`ci::FULL_TESTS`).
pub fn accept_with(root: &Path, milestone: u32, headless: bool, quiet: bool, suites_ran: bool, full: bool) -> Result<(), String> {
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
    if full {
        cmd.env(crate::ci::FULL_TESTS, "1");
    } else {
        cmd.env_remove(crate::ci::FULL_TESTS);
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
    /// Judged against a base commit measured on the same runner, when `budgets --baseline` names
    /// one (the review's Budgets job does); `None` keeps the absolute budget.
    pub relative: Option<Relative>,
}

/// How a budget is judged against a base commit (maintainer decision, 2026-10-15): the same bench
/// runs in the base commit on the same runner, and each named row's median may be at most
/// [`RELATIVE_FACTOR`] × the base's. Runner hardware varies by more than the absolute budgets
/// allowed (search's index build: 446–490 ms on some runners, 718–769 ms on others); a ratio on one
/// runner does not.
pub struct Relative {
    /// The report rows judged, each with an absolute cap that still holds (a query never above
    /// 100 ms) or `None`.
    pub rows: &'static [(&'static str, Option<f64>)],
    /// The bench's source: the base commit must have it.
    pub source: &'static str,
    /// The commit that first measured these budgets, the base when the release named by
    /// `--baseline` predates the bench.
    pub since: &'static str,
}

/// A relative budget's base: its label and the bench's medians there; `None`: judge the absolute
/// budget.
pub type Base = Option<(String, BTreeMap<String, f64>)>;

/// The largest slowdown a relative budget allows against its base commit.
pub const RELATIVE_FACTOR: f64 = 1.3;

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
        relative: None,
    },
    Budget {
        milestone: 4,
        name: "thumb at top speed",
        what: "the speed thumb held at its top speed through 100 k sketched photos: UI frame time p95 ≤ 8 ms, cells flat, no placeholder",
        cargo: &["test", "--release", "-p", "dalica-acceptance", "--test", "m4", "--", "--exact", THUMB_TEST, "--nocapture"],
        judge: judge_thumb,
        relative: None,
    },
    Budget {
        milestone: 5,
        name: "first thumbnails",
        what: "first_thumbnails: the first screenful drawn ≤ 1 s after adding a folder of 1 k and of 10 k JPEGs (1024 px, EXIF thumbnails); the full scan's time and photos per second (writes about 1.1 GB, removed after)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", FIRST_PHOTOS, "--scroll", "0", "--json"],
        judge: judge_first_thumbnails,
        relative: None,
    },
    Budget {
        milestone: 5,
        name: "scanned scroll",
        what: "first_thumbnails' scanned pass: 20 k small JPEGs scanned into the thumbnail cache, then the light table's requests at the speed thumb's top speed from top to bottom: 0 placeholder cells, owner time per frame p95 ≤ 8 ms (the 100 k pass: DALICA_ACCEPT_RELEASE=1 cargo xtask accept M5)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "first_thumbnails", "--", "--photos", "0", "--scroll", SCROLL_PHOTOS, "--json"],
        judge: judge_scanned_scroll,
        relative: None,
    },
    Budget {
        milestone: 8,
        name: "save and export",
        what: "save_export: Save of a 24 MP JPEG with a Tuning edit and with only a quarter turn (render or lossless rotation, encode, write, verify), and the 200-photo mixed export at 1600 px, Normal, with a watermark and the location removed; medians within 1.5 × their first measurement",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "save_export", "--", "--json"],
        judge: judge_save_export,
        relative: Some(Relative {
            rows: &[("save", None), ("save rotate-only", None), ("export 200 mixed", None)],
            source: "crates/engine/benches/save_export.rs",
            since: "8bd31cb",
        }),
    },
    Budget {
        milestone: 9,
        name: "search",
        what: "search: a synthetic 100 k library with captions, tags, folders and albums: the search index built, a one-word query, three words with Starred, a lone `-word`, and search.set to its result applied (debounce excluded); medians within 1.5 × their first measurement, a query never above 100 ms",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "search", "--", "--json"],
        judge: judge_search,
        relative: Some(Relative {
            rows: &[
                ("index build", None),
                ("one word", Some(SEARCH_QUERY_CAP_MS)),
                ("three words and a filter", Some(SEARCH_QUERY_CAP_MS)),
                ("a -word", Some(SEARCH_QUERY_CAP_MS)),
                ("search.set to applied", Some(SEARCH_QUERY_CAP_MS)),
            ],
            source: "crates/engine/benches/search.rs",
            since: "0ad5663",
        }),
    },
    Budget {
        milestone: 10,
        name: "scan throughput",
        what: "scan_throughput: a first scan of 5 000 JPEGs (1024 px, 160 px EXIF thumbnails) into a new library: photos per second until every photo is in the library with its embedded thumbnail available, ≥ 500 (writes about 0.5 GB, removed after)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "scan_throughput", "--", "--json"],
        judge: judge_scan_throughput,
        relative: None,
    },
    Budget {
        milestone: 10,
        name: "cold start and idle memory",
        what: "cold_start: a fresh process, five times, opening the 100 k index-only library to its first settled frame with every visible cell drawn from its sketch, median ≤ 2 s; then its resident memory after 5 s idle, ≤ 600 MiB in every run",
        cargo: &["bench", "-p", "dalica-ui", "--bench", "cold_start", "--", "--json"],
        judge: judge_cold_start,
        relative: None,
    },
    Budget {
        milestone: 11,
        name: "import",
        what: "import: listing, then importing, 1 000 JPEGs (2048 px, 160 px EXIF thumbnails) from a temporary card, medians of three: the first screenful of thumbnails ≤ 35 ms, the listing ≤ 320 ms, then ≥ 22.9 files/s and ≥ 2.3 MB/s copied and verified (writes about 0.4 GB, removed after)",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "import", "--", "--json"],
        judge: crate::bench::judge_import,
        relative: None,
    },
    Budget {
        milestone: 11,
        name: "batch edit",
        what: "batch_edit: Auto Fix on 1 000 photos as one step (edit.apply), median ≤ 7 ms, then its undo, median ≤ 5 ms",
        cargo: &["bench", "-p", "dalica-engine", "--bench", "batch_edit", "--", "--json"],
        judge: crate::bench::judge_batch_edit,
        relative: None,
    },
    Budget {
        milestone: 11,
        name: "slideshow",
        what: "slideshow: 24 MP JPEGs in the headless app at 1400 × 900, every transition: no change waits for its preview, an advance to its first frame median ≤ 24 ms, a transition frame p95 ≤ 1 ms (never above 16 ms)",
        cargo: &["bench", "-p", "dalica-ui", "--bench", "slideshow", "--", "--json"],
        judge: crate::bench::judge_slideshow,
        relative: None,
    },
];

/// The save and export budgets, in milliseconds, judged on the median (`docs/development.md` §8:
/// 1.5 × the first measurement on the budgets job's runner; `save_export` → `SAVE_BUDGET_MS`,
/// `ROTATE_BUDGET_MS`, `EXPORT_BUDGET_MS`). The rotate-only Save's budget is 1.5 × the first
/// measurement of the lossless rotation (303 ms, M9.30); 815 ms before, from the rendered Save.
pub const SAVE_MS: f64 = 1050.0;
pub const ROTATE_SAVE_MS: f64 = 455.0;
pub const EXPORT_200_MS: f64 = 24000.0;

/// `save_export --json` in release: each row's median within its budget. A planted slowdown of
/// `plant` ms is added to each median (one planted delay per measured operation).
pub fn judge_save_export(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("save_export printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("save_export ran in a {mode} build; the budgets hold in release only"));
    }
    let mut parts = Vec::new();
    for (name, budget) in [("save", SAVE_MS), ("save rotate-only", ROTATE_SAVE_MS), ("export 200 mixed", EXPORT_200_MS)] {
        let row = report
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.iter().find(|r| r.get("name").and_then(serde_json::Value::as_str) == Some(name)))
            .ok_or(format!("save_export: no row `{name}`"))?;
        let p50 = row
            .get("p50_ms")
            .and_then(serde_json::Value::as_f64)
            .filter(|v| v.is_finite())
            .ok_or(format!("save_export: `{name}` without `p50_ms`"))?;
        let judged = p50 + plant;
        let planted = if plant > 0.0 { format!(" ({p50:.0} ms measured + {plant:.2} ms planted)") } else { String::new() };
        if judged > budget {
            return Err(format!("{name}: median {judged:.0} ms{planted}, over the {budget:.0} ms budget"));
        }
        parts.push(format!("{name} {judged:.0} ms{planted} ≤ {budget:.0} ms"));
    }
    Ok(format!("medians: {}", parts.join("; ")))
}

/// The search budgets, in milliseconds, judged on the median (`docs/development.md` §8: 1.5 × the
/// first measurement, release on two cores, 2026-10-14; a query never above [`SEARCH_QUERY_CAP_MS`];
/// `search` → `INDEX_BUDGET_MS`, `WORD_BUDGET_MS`, `FILTER_BUDGET_MS`, `NOT_BUDGET_MS`,
/// `APPLIED_BUDGET_MS`). The rows in the order the bench prints them, each with its budget and
/// whether it is a query.
pub const SEARCH_BUDGETS: [(&str, f64, bool); 5] = [
    ("index build", 630.0, false),
    ("one word", 1.7, true),
    ("three words and a filter", 0.6, true),
    ("a -word", 5.1, true),
    ("search.set to applied", 22.3, true),
];
/// No search query's budget is above this (`docs/roadmap.md` M9.12).
pub const SEARCH_QUERY_CAP_MS: f64 = 100.0;
/// The library the search budgets hold for.
pub const SEARCH_PHOTOS: u64 = 100_000;

/// `search --json` in release over the 100 k library: each row's median within its budget. A
/// planted slowdown of `plant` ms is added to each median (one planted delay per measured
/// operation).
pub fn judge_search(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("search printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("search ran in a {mode} build; the budgets hold in release only"));
    }
    let photos = report.get("photos").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if photos < SEARCH_PHOTOS {
        return Err(format!("search ran over {photos} photos; the budgets hold for {SEARCH_PHOTOS}"));
    }
    let mut parts = Vec::new();
    for (name, budget, query) in SEARCH_BUDGETS {
        let budget = if query { budget.min(SEARCH_QUERY_CAP_MS) } else { budget };
        let row = report
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.iter().find(|r| r.get("name").and_then(serde_json::Value::as_str) == Some(name)))
            .ok_or(format!("search: no row `{name}`"))?;
        let p50 =
            row.get("p50_ms").and_then(serde_json::Value::as_f64).filter(|v| v.is_finite()).ok_or(format!("search: `{name}` without `p50_ms`"))?;
        let judged = p50 + plant;
        let planted = if plant > 0.0 { format!(" ({p50:.2} ms measured + {plant:.2} ms planted)") } else { String::new() };
        if judged > budget {
            return Err(format!("{name}: median {judged:.2} ms{planted}, over the {budget} ms budget"));
        }
        let matches = row.get("matches").and_then(serde_json::Value::as_u64).map(|m| format!(", {m} matches")).unwrap_or_default();
        parts.push(format!("{name} {judged:.2} ms{planted} ≤ {budget} ms{matches}"));
    }
    Ok(format!("{photos} photos, medians: {}", parts.join("; ")))
}

/// The initial scan's budget, photos per second until the embedded thumbnails are available
/// (`docs/development.md` §8; `scan_throughput` → `BUDGET_PHOTOS_PER_S`), and the photos it holds for.
pub const SCAN_PHOTOS_PER_S: f64 = 500.0;
pub const SCAN_PHOTOS: u64 = 5_000;

/// `scan_throughput --json` in release over 5 000 photos: at least [`SCAN_PHOTOS_PER_S`], every
/// photo with its embedded thumbnail. A planted slowdown of `plant` ms is added per photo scanned
/// (each photo is one measured operation), so 8 ms takes 5 000 photos 40 s longer.
pub fn judge_scan_throughput(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("scan_throughput printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("scan_throughput ran in a {mode} build; the budgets hold in release only"));
    }
    let row = report_row(&report, &format!("scan {SCAN_PHOTOS}")).map_err(|_| format!("scan_throughput: no row `scan {SCAN_PHOTOS}`"))?;
    let num = |key: &str| row.get(key).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite()).ok_or(format!("scan_throughput: no `{key}`"));
    let (scan, photos, embedded, thumbs) = (num("scan_s")?, num("photos")?, num("embedded")?, num("thumbs_s")?);
    if embedded < photos {
        return Err(format!("scan_throughput: {embedded} of {photos} photos have an embedded thumbnail"));
    }
    let judged = scan + plant * photos / 1000.0;
    let per_s = photos / judged.max(f64::MIN_POSITIVE);
    let planted = if plant > 0.0 { format!(" ({scan:.2} s measured + {plant:.2} ms planted per photo)") } else { String::new() };
    if per_s < SCAN_PHOTOS_PER_S {
        return Err(format!("{photos} photos scanned in {judged:.2} s{planted}: {per_s:.0} photos/s, under the {SCAN_PHOTOS_PER_S} photos/s budget"));
    }
    Ok(format!(
        "{photos} photos scanned in {judged:.2} s{planted}: {per_s:.0} photos/s ≥ {SCAN_PHOTOS_PER_S}; every thumbnail made after {thumbs:.1} s"
    ))
}

/// The cold start's budget on the median of the runs, and idle memory's in every run
/// (`docs/development.md` §8; `cold_start` → `COLD_START_BUDGET_MS`, `IDLE_BUDGET_MIB`), and the
/// library they hold for.
pub const COLD_START_MS: f64 = 2000.0;
pub const IDLE_MIB: f64 = 600.0;
pub const COLD_START_PHOTOS: u64 = 100_000;

/// `cold_start --json` in release over 100 k photos: the median cold start within
/// [`COLD_START_MS`], and the largest resident memory after the idle within [`IDLE_MIB`]. A planted
/// slowdown of `plant` ms per frame is added for each frame up to the first settled one.
pub fn judge_cold_start(stdout: &str, plant: f64) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("cold_start printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("cold_start ran in a {mode} build; the budgets hold in release only"));
    }
    let photos = report.get("photos").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if photos < COLD_START_PHOTOS {
        return Err(format!("cold_start ran over {photos} photos; the budgets hold for {COLD_START_PHOTOS}"));
    }
    let start = report_row(&report, "cold start 100k").map_err(|_| "cold_start: no row `cold start 100k`".to_string())?;
    let memory = report_row(&report, "idle memory 100k").map_err(|_| "cold_start: no row `idle memory 100k`".to_string())?;
    let num = |row: &serde_json::Value, key: &str| row.get(key).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite());
    let p50 = num(start, "p50_ms").ok_or("cold_start: `cold start 100k` without `p50_ms`")?;
    let frames = num(start, "frames").ok_or("cold_start: `cold start 100k` without `frames`")?;
    let judged = p50 + plant * frames;
    let planted = if plant > 0.0 { format!(" ({p50:.0} ms measured + {plant:.2} ms planted in each of {frames:.0} frames)") } else { String::new() };
    if judged > COLD_START_MS {
        return Err(format!("cold start: median {judged:.0} ms{planted}, over the {COLD_START_MS} ms budget"));
    }
    let rss = num(memory, "rss_mib").ok_or("idle memory: not measured (`rss_mib`); the budget is read on Linux")?;
    if rss > IDLE_MIB {
        return Err(format!("idle memory: {rss:.0} MiB resident after the idle, over the {IDLE_MIB} MiB budget"));
    }
    let max = num(start, "max_ms").unwrap_or(f64::NAN);
    let open = num(start, "open_ms").unwrap_or(f64::NAN);
    let samples = num(start, "samples").unwrap_or(0.0);
    Ok(format!(
        "{photos} photos: cold start median {judged:.0} ms{planted} ≤ {COLD_START_MS} ms (max {max:.0} ms over {samples:.0} runs, the index read in {open:.0} ms); idle memory {rss:.0} MiB ≤ {IDLE_MIB} MiB"
    ))
}

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
#[cfg(test)]
pub fn parse_budget_args(args: &[String]) -> Result<f64, String> {
    Ok(parse_budget_flags(args)?.0)
}

/// `[--plant-ms N] [--baseline <git ref>]`, in any order.
pub fn parse_budget_flags(args: &[String]) -> Result<(f64, Option<String>), String> {
    let usage = "usage: cargo xtask budgets [--plant-ms N] [--baseline <git ref>]";
    let (mut plant, mut baseline) = (0.0, None);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match (flag.as_str(), it.next()) {
            ("--plant-ms", Some(n)) => {
                plant = n
                    .parse::<f64>()
                    .ok()
                    .filter(|n| (0.0..=MAX_PLANT_MS).contains(n))
                    .ok_or_else(|| format!("--plant-ms takes milliseconds from 0 to {MAX_PLANT_MS}, not `{n}`"))?;
            }
            ("--baseline", Some(r)) if !r.is_empty() && !r.starts_with('-') => baseline = Some(r.clone()),
            _ => return Err(usage.to_string()),
        }
    }
    Ok((plant, baseline))
}

/// The p50 of each row of a bench's JSON report, by name.
pub fn report_medians(stdout: &str) -> Option<BTreeMap<String, f64>> {
    let report = crate::bench::parse_report(stdout)?;
    let rows = report.get("rows")?.as_array()?;
    Some(rows.iter().filter_map(|r| Some((r.get("name")?.as_str()?.to_string(), r.get("p50_ms")?.as_f64().filter(|v| v.is_finite())?))).collect())
}

/// `stdout` of a release bench judged against `base` (the same bench's medians in the base
/// commit `label`, on the same runner): each of `rel`'s rows at most [`RELATIVE_FACTOR`] × its
/// base, and under its cap. A planted slowdown of `plant` ms is added to each median.
pub fn judge_relative(stdout: &str, plant: f64, rel: &Relative, label: &str, base: &BTreeMap<String, f64>) -> Result<String, String> {
    let report = crate::bench::parse_report(stdout).ok_or("the bench printed no JSON report with `rows`")?;
    let mode = report.get("mode").and_then(serde_json::Value::as_str).unwrap_or("?");
    if mode != "release" {
        return Err(format!("the bench ran in a {mode} build; the budgets hold in release only"));
    }
    let head = report_medians(stdout).unwrap_or_default();
    let mut parts = Vec::new();
    for (name, cap) in rel.rows {
        let p50 = *head.get(*name).ok_or(format!("no row `{name}`"))?;
        let was = *base.get(*name).ok_or(format!("the base {label} has no row `{name}`"))?;
        let judged = p50 + plant;
        let limit = was * RELATIVE_FACTOR;
        let planted = if plant > 0.0 { format!(" ({p50:.2} ms measured + {plant:.2} ms planted)") } else { String::new() };
        if judged > limit {
            return Err(format!("{name}: median {judged:.2} ms{planted}, over {RELATIVE_FACTOR} × the base {label}'s {was:.2} ms"));
        }
        if let Some(cap) = cap
            && judged > *cap
        {
            return Err(format!("{name}: median {judged:.2} ms{planted}, over its {cap} ms cap"));
        }
        parts.push(format!("{name} {judged:.2} ms ({:.2} × {was:.2} ms)", judged / was.max(f64::MIN_POSITIVE)));
    }
    Ok(format!("against {label}: {}", parts.join("; ")))
}

/// Runs every budget with `cargo` (`cargo(args)` → `(succeeded, stdout)`), judges each, prints a
/// line per budget and fails when any is missed. Every budget runs, so the log shows all numbers.
#[cfg(test)]
pub fn budgets_with(plant: f64, cargo: impl FnMut(&[&str]) -> Result<(bool, String), String>) -> Result<(), String> {
    budgets_against(plant, |_| Ok(None), cargo)
}

/// [`budgets_with`], where `base(budget)` gives a relative budget's base medians and the base's
/// label (`None`: judge it on its absolute budget).
pub fn budgets_against(
    plant: f64,
    mut base: impl FnMut(&Budget) -> Result<Base, String>,
    mut cargo: impl FnMut(&[&str]) -> Result<(bool, String), String>,
) -> Result<(), String> {
    if plant > 0.0 {
        println!("budgets: a planted slowdown of {plant} ms per frame (--plant-ms)");
    }
    let mut missed = Vec::new();
    for b in BUDGETS {
        println!("==> budgets: M{} {}: {}\n    cargo {}", b.milestone, b.name, b.what, b.cargo.join(" "));
        let started = Instant::now();
        let against = match b.relative.as_ref().map(|_| base(b)) {
            Some(Err(e)) => {
                println!("  MISSED: the base: {e}");
                missed.push(format!("M{} {}: the base: {e}", b.milestone, b.name));
                continue;
            }
            Some(Ok(found)) => found,
            None => None,
        };
        let verdict = match cargo(b.cargo) {
            Ok((true, stdout)) => match (&b.relative, &against) {
                (Some(rel), Some((label, medians))) => judge_relative(&stdout, plant, rel, label, medians),
                _ => (b.judge)(&stdout, plant),
            },
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

/// `cargo xtask budgets [--plant-ms N] [--baseline <git ref>]`: builds in release and runs
/// [`BUDGETS`]. With `--baseline` (the review's Budgets job: the last release tag), each relative
/// budget first runs its bench in that commit, or in the commit that first measured it when the
/// release predates the bench, checked out under `target/budget-base/` (its own target directory),
/// and is judged against it.
pub fn budgets(root: &Path, args: &[String]) -> Result<(), String> {
    let (plant, baseline) = parse_budget_flags(args)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let run = |dir: &Path, target: Option<&Path>, args: &[&str]| -> Result<(bool, String), String> {
        let mut cmd = Command::new(&cargo);
        cmd.current_dir(dir).args(args).env_remove("DALICA_BLESS").stderr(std::process::Stdio::inherit());
        if let Some(t) = target {
            cmd.env("CARGO_TARGET_DIR", t);
        }
        let out = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    };
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| format!("cannot run git: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
        }
    };
    let base = |b: &Budget| -> Result<Base, String> {
        let (Some(reference), Some(rel)) = (baseline.as_deref(), b.relative.as_ref()) else { return Ok(None) };
        let has_bench = git(&["cat-file", "-e", &format!("{reference}:{}", rel.source)]).is_ok();
        let commit = if has_bench { reference } else { rel.since };
        let sha = git(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")])?;
        let label = if commit == sha { sha.chars().take(9).collect() } else { format!("{commit} ({})", sha.chars().take(9).collect::<String>()) };
        let dir = root.join("target").join("budget-base").join(&sha);
        if !dir.join("Cargo.toml").is_file() {
            git(&["worktree", "add", "--detach", &dir.display().to_string(), &sha])?;
        }
        println!("  base: {label}, in {}", dir.display());
        let (ok, stdout) = run(&dir, Some(&root.join("target").join("budget-base-target")), b.cargo)?;
        if !ok {
            return Err(format!("cargo {} failed in the base {label}", b.cargo.join(" ")));
        }
        let medians = report_medians(&stdout).ok_or(format!("the base {label} printed no JSON report"))?;
        Ok(Some((label, medians)))
    };
    budgets_against(plant, base, |args| run(root, None, args))
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

    #[test]
    fn budget_flags() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_budget_flags(&a(&[])), Ok((0.0, None)));
        assert_eq!(parse_budget_flags(&a(&["--baseline", "v0.3.0", "--plant-ms", "8"])), Ok((8.0, Some("v0.3.0".into()))));
        assert!(parse_budget_flags(&a(&["--baseline"])).is_err() && parse_budget_flags(&a(&["--baseline", "--plant-ms"])).is_err());
        assert!(parse_budget_flags(&a(&["--plant-ms", "2000"])).is_err());
    }

    #[test]
    fn relative_budgets_judge_against_the_base_on_the_same_runner() {
        let rel = Relative { rows: &[("index build", None), ("one word", Some(100.0))], source: "x.rs", since: "abc" };
        let out = |build: f64, word: f64| {
            let r = serde_json::json!({ "mode": "release", "rows": [{ "name": "index build", "p50_ms": build }, { "name": "one word", "p50_ms": word }] });
            format!("{r}\n")
        };
        let base: BTreeMap<String, f64> = [("index build".to_string(), 600.0), ("one word".to_string(), 1.0)].into();
        // A slow runner: 749 ms against a base of 600 ms on it is 1.25 ×, within.
        assert!(judge_relative(&out(749.0, 1.2), 0.0, &rel, "v0.3.0", &base).unwrap().contains("index build 749.00 ms (1.25 ×"));
        let e = judge_relative(&out(781.0, 1.0), 0.0, &rel, "v0.3.0", &base).unwrap_err();
        assert!(e.starts_with("index build") && e.contains("1.3 × the base v0.3.0's 600.00 ms"), "{e}");
        // A planted slowdown counts; a cap still holds; the base must have every row.
        assert!(judge_relative(&out(600.0, 1.0), 200.0, &rel, "b", &base).is_err());
        let wide: BTreeMap<String, f64> = [("index build".to_string(), 600.0), ("one word".to_string(), 90.0)].into();
        assert!(judge_relative(&out(600.0, 101.0), 0.0, &rel, "b", &wide).unwrap_err().contains("100 ms cap"));
        let short: BTreeMap<String, f64> = [("index build".to_string(), 600.0)].into();
        assert!(judge_relative(&out(600.0, 1.0), 0.0, &rel, "b", &short).unwrap_err().contains("has no row `one word`"));
        assert!(judge_relative("{\"mode\":\"debug\",\"rows\":[]}\n", 0.0, &rel, "b", &base).unwrap_err().contains("debug build"));
        // Every relative budget names its bench's source and the commit that first measured it.
        for b in BUDGETS.iter().filter_map(|b| b.relative.as_ref()) {
            assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(b.source).is_file(), "{}", b.source);
            assert!(b.since.len() >= 7 && !b.rows.is_empty());
        }
    }

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

    /// What `save_export --json` prints, with each row's median.
    fn save_export_out(mode: &str, save: f64, rotate: f64, export: f64) -> String {
        let row = |name: &str, p50: f64| serde_json::json!({ "name": name, "p50_ms": p50, "p95_ms": p50 * 1.1, "max_ms": p50 * 1.2, "samples": 7 });
        let report = serde_json::json!({ "bench": "save_export", "mode": mode, "threads": 4,
            "rows": [row("save", save), row("save rotate-only", rotate), row("export 200 mixed", export)] });
        format!("a 6000 × 4000 JPEG written in 0.4 s\n{report}\n")
    }

    /// What `search --json` prints over `photos` photos, with each row's median (in
    /// [`SEARCH_BUDGETS`] order).
    fn search_out(mode: &str, photos: u64, p50: [f64; 5]) -> String {
        let mut rows: Vec<serde_json::Value> = SEARCH_BUDGETS
            .iter()
            .zip(p50)
            .map(|((name, budget, query), p50)| {
                let mut r =
                    serde_json::json!({ "name": name, "p50_ms": p50, "p95_ms": p50 * 1.2, "max_ms": p50 * 1.5, "samples": 50, "budget_ms": budget });
                if *query {
                    r["matches"] = serde_json::json!(13441);
                }
                r
            })
            .collect();
        rows.push(serde_json::json!({ "name": "first search", "p50_ms": 750.0, "p95_ms": 750.0, "max_ms": 750.0, "samples": 1 }));
        let report =
            serde_json::json!({ "bench": "search", "mode": mode, "threads": 2, "photos": photos, "folders": 1000, "albums": 100, "rows": rows });
        format!("a library of {photos} photos in 1000 folders, 100 albums, made and opened in 0.8 s\n{report}\n")
    }

    /// What `scan_throughput --json` prints: `photos` scanned in `scan_s`, `embedded` of them with
    /// an embedded thumbnail.
    fn scan_out(mode: &str, photos: u64, scan_s: f64, embedded: u64) -> String {
        let row = serde_json::json!({ "name": format!("scan {photos}"), "photos": photos, "side_px": 1024, "embedded_px": 160, "scan_s": scan_s,
            "photos_per_s": photos as f64 / scan_s, "budget_photos_per_s": 500.0, "thumbs_s": 21.4, "thumbs_photos_per_s": 233.0,
            "embedded": embedded, "thumbs_failed": 0, "frames": 1287 });
        let report = serde_json::json!({ "bench": "scan_throughput", "mode": mode, "threads": 2, "photos": photos, "rows": [row] });
        format!("scan_throughput: {photos} photos of 1024 px written in 200.2 s\n{report}\n")
    }

    /// What `cold_start --json` prints over `photos` photos: the median cold start, the frames to
    /// the first settled one, and the largest resident memory (`None`: not measured).
    fn cold_out(mode: &str, photos: u64, p50: f64, frames: u64, rss: Option<f64>) -> String {
        let n = if photos == 100_000 { "100k".to_string() } else { photos.to_string() };
        let report = serde_json::json!({ "bench": "cold_start", "mode": mode, "photos": photos, "runs": 5, "os": "linux", "rows": [
            { "name": format!("cold start {n}"), "samples": 5, "p50_ms": p50, "p95_ms": p50 * 1.3, "max_ms": p50 * 1.3, "budget_ms": 2000.0,
              "open_ms": p50 * 0.85, "first_frame_ms": p50 - 1.0, "frames": frames, "cells": 28, "sketch_cells": 28 },
            { "name": format!("idle memory {n}"), "samples": 5, "rss_mib": rss, "rss_p50_mib": rss, "peak_mib": rss, "budget_mib": 600.0,
              "idle_ms": 5000, "idle_repaint_ms": null },
        ]});
        format!("cold_start: run 1: settled {p50:.0} ms after the spawn\n{report}\n")
    }

    #[test]
    fn the_scan_budget_needs_500_photos_a_second_with_every_embedded_thumbnail() {
        let line = judge_scan_throughput(&scan_out("release", 5000, 0.484, 5000), 0.0).unwrap();
        assert!(line.contains("5000 photos scanned in 0.48 s: 10331 photos/s ≥ 500") && line.contains("every thumbnail made after 21.4 s"), "{line}");
        assert!(judge_scan_throughput(&scan_out("release", 5000, 10.0, 5000), 0.0).is_ok(), "the budget itself holds");
        let e = judge_scan_throughput(&scan_out("release", 5000, 10.1, 5000), 0.0).unwrap_err();
        assert!(e.contains("495 photos/s, under the 500 photos/s budget"), "{e}");
        // 8 ms planted per photo: 5 000 photos take 40 s longer.
        let e = judge_scan_throughput(&scan_out("release", 5000, 0.484, 5000), 8.0).unwrap_err();
        assert!(e.contains("40.48 s (0.48 s measured + 8.00 ms planted per photo): 124 photos/s"), "{e}");
        assert!(judge_scan_throughput(&scan_out("release", 5000, 0.484, 4999), 0.0).unwrap_err().contains("4999 of 5000 photos have an embedded"));
        assert!(judge_scan_throughput(&scan_out("debug", 5000, 0.484, 5000), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_scan_throughput(&scan_out("release", 200, 0.01, 200), 0.0).unwrap_err().contains("no row `scan 5000`"));
        assert!(judge_scan_throughput("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        let no_time = "{\"mode\":\"release\",\"rows\":[{\"name\":\"scan 5000\",\"photos\":5000,\"scan_s\":null}]}";
        assert!(judge_scan_throughput(no_time, 0.0).unwrap_err().contains("no `scan_s`"));
    }

    #[test]
    fn the_cold_start_budget_judges_the_median_and_idle_memory_every_run() {
        let line = judge_cold_start(&cold_out("release", 100_000, 238.6, 3, Some(83.0)), 0.0).unwrap();
        assert!(
            line.contains("cold start median 239 ms ≤ 2000 ms (max 310 ms over 5 runs") && line.contains("idle memory 83 MiB ≤ 600 MiB"),
            "{line}"
        );
        assert!(judge_cold_start(&cold_out("release", 100_000, COLD_START_MS, 3, Some(IDLE_MIB)), 0.0).is_ok(), "the budgets themselves hold");
        let e = judge_cold_start(&cold_out("release", 100_000, 2001.0, 3, Some(83.0)), 0.0).unwrap_err();
        assert!(e.starts_with("cold start: median 2001 ms, over the 2000 ms budget"), "{e}");
        let e = judge_cold_start(&cold_out("release", 100_000, 238.6, 3, Some(601.0)), 0.0).unwrap_err();
        assert!(e.starts_with("idle memory: 601 MiB resident after the idle, over the 600 MiB budget"), "{e}");
        assert!(judge_cold_start(&cold_out("release", 100_000, 238.6, 3, None), 0.0).unwrap_err().contains("not measured"));
        // A planted slowdown counts once per frame up to the first settled one.
        assert!(judge_cold_start(&cold_out("release", 100_000, 238.6, 3, Some(83.0)), 8.0).is_ok(), "8 ms in each of 3 frames");
        let e = judge_cold_start(&cold_out("release", 100_000, 238.6, 3, Some(83.0)), 600.0).unwrap_err();
        assert!(e.contains("median 2039 ms (239 ms measured + 600.00 ms planted in each of 3 frames)"), "{e}");
        assert!(judge_cold_start(&cold_out("debug", 100_000, 238.6, 3, Some(83.0)), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_cold_start(&cold_out("release", 2_000, 50.0, 3, Some(40.0)), 0.0).unwrap_err().contains("over 2000 photos"));
        assert!(judge_cold_start("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        let no_memory = "{\"mode\":\"release\",\"photos\":100000,\"rows\":[{\"name\":\"cold start 100k\",\"p50_ms\":1,\"frames\":3}]}";
        assert!(judge_cold_start(no_memory, 0.0).unwrap_err().contains("no row `idle memory 100k`"));
    }

    /// The first measurement (2026-10-14, release, two cores).
    const SEARCH_MEASURED: [f64; 5] = [417.396, 1.118, 0.379, 3.386, 14.859];

    #[test]
    fn the_search_budgets_judge_each_median_over_100k_photos_in_release() {
        let line = judge_search(&search_out("release", 100_000, SEARCH_MEASURED), 0.0).unwrap();
        assert!(line.starts_with("100000 photos, medians: index build 417.40 ms ≤ 630 ms;"), "{line}");
        assert!(line.contains("one word 1.12 ms ≤ 1.7 ms, 13441 matches") && line.contains("search.set to applied 14.86 ms ≤ 22.3 ms"), "{line}");
        let at_budget = SEARCH_BUDGETS.map(|(_, b, _)| b);
        assert!(judge_search(&search_out("release", 100_000, at_budget), 0.0).is_ok(), "the budgets themselves hold");
        for (i, (name, budget, _)) in SEARCH_BUDGETS.iter().enumerate() {
            let mut slow = SEARCH_MEASURED;
            slow[i] = budget + 0.01;
            let e = judge_search(&search_out("release", 100_000, slow), 0.0).unwrap_err();
            assert!(e.starts_with(&format!("{name}: median")) && e.contains("over the"), "{e}");
        }
        let e = judge_search(&search_out("release", 100_000, SEARCH_MEASURED), 8.0).unwrap_err();
        assert!(e.contains("one word: median 9.12 ms (1.12 ms measured + 8.00 ms planted)"), "a planted slowdown adds to the median: {e}");
        assert!(judge_search(&search_out("debug", 100_000, SEARCH_MEASURED), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_search(&search_out("release", 2_000, SEARCH_MEASURED), 0.0).unwrap_err().contains("over 2000 photos"));
        assert!(judge_search("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        let no_rows = "{\"mode\":\"release\",\"photos\":100000,\"rows\":[{\"name\":\"index build\",\"p50_ms\":1}]}";
        assert!(judge_search(no_rows, 0.0).unwrap_err().contains("no row `one word`"));
        let no_time = "{\"mode\":\"release\",\"photos\":100000,\"rows\":[{\"name\":\"index build\",\"p50_ms\":null}]}";
        assert!(judge_search(no_time, 0.0).unwrap_err().contains("without `p50_ms`"));
    }

    #[test]
    fn the_save_and_export_budgets_judge_each_median_in_release() {
        let line = judge_save_export(&save_export_out("release", 300.0, 150.0, 15_000.0), 0.0).unwrap();
        assert!(
            line.contains("save 300 ms ≤") && line.contains("save rotate-only 150 ms ≤") && line.contains("export 200 mixed 15000 ms ≤"),
            "{line}"
        );
        assert!(judge_save_export(&save_export_out("release", SAVE_MS, ROTATE_SAVE_MS, EXPORT_200_MS), 0.0).is_ok(), "the budgets themselves hold");
        let e = judge_save_export(&save_export_out("release", SAVE_MS + 1.0, 150.0, 15_000.0), 0.0).unwrap_err();
        assert!(e.starts_with("save: median") && e.contains("over the"), "{e}");
        let e = judge_save_export(&save_export_out("release", 300.0, ROTATE_SAVE_MS + 1.0, 15_000.0), 0.0).unwrap_err();
        assert!(e.starts_with("save rotate-only: median"), "{e}");
        // M9.30: the rotate-only budget is the lossless rotation's (1.5 × 303 ms); the rendered
        // rotation's first measurement (542 ms), within the old 815 ms, no longer passes.
        let e = judge_save_export(&save_export_out("release", 300.0, 542.0, 15_000.0), 0.0).unwrap_err();
        assert!(e.starts_with("save rotate-only: median 542 ms"), "{e}");
        let e = judge_save_export(&save_export_out("release", 300.0, 150.0, EXPORT_200_MS + 1.0), 0.0).unwrap_err();
        assert!(e.starts_with("export 200 mixed: median"), "{e}");
        let e = judge_save_export(&save_export_out("release", SAVE_MS - 5.0, 150.0, 15_000.0), 8.0).unwrap_err();
        assert!(e.contains("measured + 8.00 ms planted"), "a planted slowdown adds to the median: {e}");
        assert!(judge_save_export(&save_export_out("debug", 300.0, 150.0, 15_000.0), 0.0).unwrap_err().contains("debug build"));
        assert!(judge_save_export("no report\n", 0.0).unwrap_err().contains("no JSON report"));
        let no_export = "{\"mode\":\"release\",\"rows\":[{\"name\":\"save\",\"p50_ms\":1},{\"name\":\"save rotate-only\",\"p50_ms\":1}]}";
        assert!(judge_save_export(no_export, 0.0).unwrap_err().contains("no row `export 200 mixed`"));
        let no_time = "{\"mode\":\"release\",\"rows\":[{\"name\":\"save\",\"p50_ms\":null}]}";
        assert!(judge_save_export(no_time, 0.0).unwrap_err().contains("without `p50_ms`"));
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
                } else if args.contains(&"save_export") {
                    (true, save_export_out("release", 300.0, 150.0, 15_000.0))
                } else if args.contains(&"search") {
                    (true, search_out("release", 100_000, SEARCH_MEASURED))
                } else if args.contains(&"scan_throughput") {
                    (true, scan_out("release", 5000, 0.484, 5000))
                } else if args.contains(&"cold_start") {
                    (true, cold_out("release", 100_000, 238.6, 3, Some(83.0)))
                } else if args.contains(&"import") {
                    (true, crate::bench::tests::import_out("release", 1000, 2048, 23.2, 211.2, 29_101.0))
                } else if args.contains(&"batch_edit") {
                    (true, crate::bench::tests::batch_out("release", 1000, 4.65, 3.31))
                } else if args.contains(&"slideshow") {
                    (true, crate::bench::tests::slideshow_out("release", [6000, 4000], 16.0, 0.199, 0))
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
        // 8 ms planted per frame: every frame budget misses, the search queries' (a few ms), the
        // scan's (8 ms per photo), Batch Edit's and the slideshow's; a first screenful well inside
        // its second does not, nor the save and export medians well inside theirs, nor a cold start
        // of three frames, nor the import (8 s more over 1 000 files is inside its 1.5 ×).
        let e = budgets_with(8.0, fake(2.0, 3.0, 517.0)).unwrap_err();
        assert!(
            e.contains("7 of 11 missed")
                && e.contains("M4 light table scroll")
                && e.contains("M4 thumb at top speed")
                && e.contains("M5 scanned scroll")
                && e.contains("M9 search: one word")
                && e.contains("M10 scan throughput")
                && e.contains("M11 batch edit: auto fix 1000")
                && e.contains("M11 slideshow: transition frame"),
            "{e}"
        );
        // 15 ms planted misses the import too: its first screenful and, per file, its copy rate.
        let e = budgets_with(15.0, fake(2.0, 3.0, 517.0)).unwrap_err();
        assert!(e.contains("M11 import: first thumbnails"), "{e}");
        // The largest planted slowdown misses every budget, the cold start's included.
        let e = budgets_with(MAX_PLANT_MS, fake(2.0, 3.0, 517.0)).unwrap_err();
        assert!(e.contains("11 of 11 missed") && e.contains("M10 cold start and idle memory: cold start: median"), "{e}");
        let e = budgets_with(0.0, fake(2.0, 8.5, 517.0)).unwrap_err();
        assert!(e.contains("1 of 11 missed") && e.contains("M4 thumb at top speed"), "{e}");
        let e = budgets_with(0.0, fake(2.0, 3.0, 1100.0)).unwrap_err();
        assert!(e.contains("1 of 11 missed") && e.contains("M5 first thumbnails: 10000 JPEGs"), "{e}");
        let mut ran = Vec::new();
        let e = budgets_with(0.0, |args: &[&str]| {
            ran.push(args.join(" "));
            Ok((false, String::new()))
        })
        .unwrap_err();
        assert!(e.contains("11 of 11 missed") && e.contains("failed"), "{e}");
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
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "save_export" && b.package == "dalica-engine"));
        let save_export = std::fs::read_to_string(root.join("crates").join("engine").join("benches").join("save_export.rs")).unwrap();
        for (constant, ours) in [("SAVE_BUDGET_MS", SAVE_MS), ("ROTATE_BUDGET_MS", ROTATE_SAVE_MS), ("EXPORT_BUDGET_MS", EXPORT_200_MS)] {
            assert!(save_export.contains(&format!("const {constant}: f64 = {ours:?};")), "the bench's {constant} is ours: {ours}");
        }
        for printed in ["row(\"save\",", "row(\"save rotate-only\",", "row(\"export 200 mixed\",", "\"p50_ms\"", "\"mode\": mode"] {
            assert!(save_export.contains(printed), "save_export prints {printed}");
        }
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "search" && b.package == "dalica-engine"));
        let search = std::fs::read_to_string(root.join("crates").join("engine").join("benches").join("search.rs")).unwrap();
        let constants = ["INDEX_BUDGET_MS", "WORD_BUDGET_MS", "FILTER_BUDGET_MS", "NOT_BUDGET_MS", "APPLIED_BUDGET_MS"];
        for (constant, (name, ours, _)) in constants.iter().zip(SEARCH_BUDGETS) {
            assert!(search.contains(&format!("const {constant}: f64 = {ours:?};")), "the bench's {constant} is ours for `{name}`: {ours}");
            assert!(search.contains(&format!("\"{name}\"")), "search prints the row `{name}`");
        }
        assert!(search.contains(&format!("const QUERY_CAP_MS: f64 = {SEARCH_QUERY_CAP_MS:?};")), "the bench's query cap is ours");
        for printed in ["\"p50_ms\"", "\"mode\": mode", "\"photos\": photos", "Spec::PHOTOS_100K.photos"] {
            assert!(search.contains(printed), "search prints {printed}");
        }
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
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "scan_throughput" && b.package == "dalica-engine"));
        let scan = std::fs::read_to_string(root.join("crates").join("engine").join("benches").join("scan_throughput.rs")).unwrap();
        assert!(scan.contains(&format!("const BUDGET_PHOTOS_PER_S: f64 = {SCAN_PHOTOS_PER_S:?};")), "the bench's budget is ours");
        assert!(scan.contains("const PHOTOS: usize = 5_000;") && SCAN_PHOTOS == 5_000, "the bench's photos are ours");
        for printed in ["\"name\": format!(\"scan {n}\")", "\"scan_s\"", "\"embedded\"", "\"thumbs_s\"", "\"mode\": mode"] {
            assert!(scan.contains(printed), "scan_throughput prints {printed}");
        }
        assert!(crate::bench::BENCHES.iter().any(|b| b.name == "cold_start" && b.package == "dalica-ui"));
        let cold = std::fs::read_to_string(root.join("crates").join("ui").join("benches").join("cold_start.rs")).unwrap();
        assert!(cold.contains(&format!("const COLD_START_BUDGET_MS: f64 = {COLD_START_MS:?};")), "the bench's cold start budget is ours");
        assert!(cold.contains(&format!("const IDLE_BUDGET_MIB: f64 = {IDLE_MIB:?};")), "the bench's idle memory budget is ours");
        assert!(cold.contains("const PHOTOS: usize = 100_000;") && COLD_START_PHOTOS == 100_000, "the bench's library is ours");
        for printed in
            ["format!(\"cold start {name}\")", "format!(\"idle memory {name}\")", "\"p50_ms\"", "\"frames\"", "\"rss_mib\"", "\"mode\": mode"]
        {
            assert!(cold.contains(printed), "cold_start prints {printed}");
        }
        let m4 = std::fs::read_to_string(root.join("tests").join("acceptance").join("m4.rs")).unwrap();
        assert!(m4.contains(&format!("fn {THUMB_TEST}()")), "m4.rs has the thumb test");
        assert!(m4.contains("\"thumb at top speed ({mode}): ") && m4.contains("UI time p50 {:.2} ms, p95 {:.2} ms"), "the line judge_thumb reads");
        assert!(crate::COMMANDS.contains(&"budgets"));
    }
}
