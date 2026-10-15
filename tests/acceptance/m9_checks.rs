//! The parts of M9's acceptance that run other things: the crate suites behind each *Done when*
//! (the grammar tests among them), `panic_hunt`, and the `search` bench: a small debug pass that
//! checks the path and prints its times, and with `DALICA_ACCEPT_RELEASE=1` the 100 k library in
//! release judged on the budgets `cargo xtask budgets` holds it to.

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The search budgets of `development.md` §8, medians in milliseconds, in the order the bench
/// prints its rows, and whether the row is a query (`xtask/src/accept.rs` → `SEARCH_BUDGETS`,
/// `crates/engine/benches/search.rs`; 1.5 × the first measurement, 2026-10-14).
pub const SEARCH_BUDGETS: [(&str, f64, bool); 5] = [
    ("index build", 630.0, false),
    ("one word", 1.7, true),
    ("three words and a filter", 0.6, true),
    ("a -word", 5.1, true),
    ("search.set to applied", 22.3, true),
];
/// No query's budget is above this (`roadmap.md` M9.12).
pub const QUERY_CAP_MS: f64 = 100.0;
/// The library the budgets hold for.
pub const SEARCH_PHOTOS: u64 = 100_000;

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output and error.
/// A debug run never sees [`RELEASE`]; `release` sets it.
pub fn cargo(args: &[&str], release: bool) -> String {
    let started = Instant::now();
    let mut cmd = dalica_acceptance::cargo();
    cmd.args(args);
    if release {
        cmd.env(RELEASE, "1");
    } else {
        cmd.env_remove(RELEASE);
    }
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "cargo {}:\n{stdout}\n{stderr}", args.join(" "));
    println!("cargo {}: ok ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
    stdout + &stderr
}

/// Runs `cargo test <args>`; panics with its output if it fails or runs nothing.
pub fn cargo_test(args: &[&str]) -> String {
    let all: Vec<&str> = ["test"].iter().chain(args).copied().collect();
    let out = cargo(&all, false);
    let passed: usize =
        out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{out}", all.join(" "));
    println!("  {passed} passed");
    out
}

/// `cargo test -p <package> --test <suite>…` in debug; what follows a `"--"` in `list` goes to the
/// test binaries as it is.
fn suites(package: &str, list: &[&str]) -> String {
    let mut args = vec!["-p", package];
    let (tests, rest) = list.iter().position(|a| *a == "--").map_or((list, &[][..]), |i| (&list[..i], &list[i..]));
    for s in tests {
        args.extend(["--test", s]);
    }
    args.extend(rest);
    cargo_test(&args)
}

/// The crate suites behind M9's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Albums:");
    println!("  the model, position keys and re-keying, the tree's album group, albums on disk and in the index, §10.12 at the store level");
    suites("dalica-library", &["m9_fields", "albums", "ops", "tree"]);
    suites("dalica-sidecar", &["albums"]);
    suites("dalica-store", &["m9_fields", "albums"]);
    println!("  the album commands and their undo; the tree, sections, Album Properties, Add to Album, the menus and the tray");
    suites("dalica-engine", &["albums", "settings_m9"]);
    suites("dalica-ui", &["view_state_m9", "albums_tree", "album_properties", "section_header", "photo_menu", "add_to_album", "tray"]);
    suites("dalica-ui-kit", &["m9_widgets", "icons_albums"]);
    println!("Search and filters:");
    println!("  the grammar tests and the index (a random op sequence's index equals one built anew); facts read on scan");
    let out = suites("dalica-library", &["search", "search_props", "--", "--nocapture"]);
    for line in out.lines().filter(|l| l.starts_with("100000 photos:")) {
        println!("    the library's 100 k index (debug): {}", line.trim());
    }
    suites("dalica-scan", &["facts"]);
    println!("  search on a worker (a 100 k library answered off the owner thread), Find Duplicates; the window's search mode");
    let out = suites("dalica-engine", &["facts", "search", "duplicates", "--", "--nocapture"]);
    for line in out.lines().filter(|l| l.starts_with("100 k photos:") || l.starts_with("Find Duplicates, 10 000 photos")) {
        println!("    (debug) {}", line.trim());
    }
    suites("dalica-ui", &["search_ui", "toolbar", "light_table", "tags_pane"]);
    println!("File operations:");
    println!("  moves and copies that never replace a file, the OS trash; sidecar entries and backups that travel; the journal");
    suites("dalica-fs", &["transfer", "trash"]);
    suites("dalica-store", &["carry", "intents"]);
    println!("  move, copy, Move to New Folder, rename, delete, folder moves, each with its undo; a fault at every step and its repair");
    suites("dalica-engine", &["fileops", "fileops_store", "move_copy", "new_folder", "rename", "delete", "folder_move"]);
    suites("dalica-engine", &["fileops_faults", "fileops_recovery"]);
    println!("  data-model.md §10.2, §10.3, §10.7 and §10.12 through the commands and a watcher; two volumes");
    let out = suites("dalica-engine", &["lessons_m9", "two_volumes", "--", "--nocapture", "--test-threads=1"]);
    for line in out.lines().filter(|l| l.starts_with("two volumes:")) {
        println!("    {line}");
    }
    println!("  the questions, Rename Files, the delete questions, Move Folder, drag and drop, Cut, Copy and Paste, the notices");
    suites(
        "dalica-ui",
        &["move_dialogs", "new_folder", "notices", "rename_dialog", "delete_dialogs", "folder_properties", "folder_move", "drag", "clipboard"],
    );
}

/// `panic_hunt -- --ignored`: every command, M9's included, hostile params, never a panic.
pub fn panic_hunt() {
    cargo_test(&["-p", "dalica-engine", "--test", "panic_hunt", "--", "--ignored"]);
}

/// The JSON report a bench printed (its line starting with `{`).
fn report(out: &str) -> Value {
    out.lines()
        .rev()
        .find_map(|l| l.trim().starts_with('{').then(|| serde_json::from_str(l.trim()).ok()).flatten())
        .unwrap_or_else(|| panic!("no JSON report:\n{out}"))
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["rows"].as_array().and_then(|r| r.iter().find(|r| r["name"] == name)).unwrap_or_else(|| panic!("no row `{name}`: {report}"))
}

fn ms(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or_else(|| panic!("no `{key}`: {r}"))
}

/// The bench's rows as lines, each with the budget it is judged on in release; the medians in
/// [`SEARCH_BUDGETS`] order.
fn print_rows(report: &Value) -> [f64; 5] {
    println!(
        "search ({}, {} threads, {} photos in {} folders, {} albums):",
        report["mode"].as_str().unwrap_or("?"),
        report["threads"],
        report["photos"],
        report["folders"],
        report["albums"]
    );
    let mut medians = [0.0; 5];
    for (i, (name, budget, query)) in SEARCH_BUDGETS.into_iter().enumerate() {
        let r = row(report, name);
        medians[i] = ms(r, "p50_ms");
        let budget = if query { budget.min(QUERY_CAP_MS) } else { budget };
        let matches = r["matches"].as_u64().map(|m| format!(", {m} matches")).unwrap_or_default();
        println!("  {name:26} p50 {:9.3} ms  max {:9.3} ms  (budget {budget} ms on p50 in release{matches})", medians[i], ms(r, "max_ms"));
    }
    if let Some(first) = report["rows"].as_array().and_then(|r| r.iter().find(|r| r["name"] == "first search")) {
        println!("  {:26} p50 {:9.3} ms  (not budgeted: the worker's start and its first index)", "first search", ms(first, "p50_ms"));
    }
    medians
}

/// The `search` bench's path in a debug build (its smoke pass: 2 000 photos, two samples each):
/// every row measured and printed, nothing judged (an unoptimised build says nothing about the
/// budgets).
pub fn search_debug() {
    let out = cargo(&["test", "-p", "dalica-engine", "--bench", "search", "--", "--json"], false);
    let r = report(&out);
    assert_eq!(r["mode"], "debug", "{r}");
    let medians = print_rows(&r);
    assert!(medians.iter().all(|t| t.is_finite() && *t >= 0.0), "every row measured: {medians:?}");
}

/// The budgets in release (`DALICA_ACCEPT_RELEASE=1`), as `cargo xtask budgets` runs them: the
/// `search` bench over the synthetic 100 k library, each median within its budget (a query's
/// never above 100 ms). Returns the medians.
pub fn search_release() -> [f64; 5] {
    let out = cargo(&["bench", "-p", "dalica-engine", "--bench", "search", "--", "--json"], true);
    let r = report(&out);
    assert_eq!(r["mode"], "release", "{r}");
    assert!(r["photos"].as_u64().unwrap_or(0) >= SEARCH_PHOTOS, "the budgets hold for {SEARCH_PHOTOS} photos: {r}");
    let medians = print_rows(&r);
    for ((name, budget, query), t) in SEARCH_BUDGETS.into_iter().zip(medians) {
        let budget = if query { budget.min(QUERY_CAP_MS) } else { budget };
        assert!(t <= budget, "{name}: median {t:.3} ms, over the {budget} ms budget");
    }
    medians
}
