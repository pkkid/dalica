//! `cargo xtask stats [--check]` (`process.md` §5, `mcp.md` §9): the behaviour number, the pass rate
//! of the workflow acceptance tests. Runs `cargo test -p dalica-mcp --test workflows` and prints
//! *passing / total*, the failing workflows and the ignored ones with their reasons (a workflow whose
//! feature is not built yet is `#[ignore = "M<n>: …"]`). A workflow is a test, or a module of tests,
//! named `wNN_<slug>`: it passes when all its tests pass, is ignored when one is ignored, and fails
//! when one fails.
//!
//! `--check` holds the floor: fewer passing workflows than [`FLOOR`] fail. Floors only rise (AGENTS.md
//! §3): a task that makes more workflows pass raises [`FLOOR`] with it. `cargo xtask ci` runs the
//! check in the review gate and in a task gate that tests `dalica-mcp`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// The workflows passing on `main` when the floor was last raised: M11.22, the eleven workflows
/// required by M11 (`mcp.md` §9: W01–W10 and W12; W07 and W10 joined the nine P0 workflows of
/// M10.19, and M10.20 had set five before W04, W05, W06 and W12 landed).
pub const FLOOR: usize = 11;
/// The package and test target holding the workflows.
pub const PACKAGE: &str = "dalica-mcp";
pub const TARGET: &str = "workflows";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stats {
    pub passing: Vec<String>,
    pub failing: Vec<String>,
    /// Ignored workflows with their reasons (empty when the test gives none).
    pub ignored: Vec<(String, String)>,
}

impl Stats {
    pub fn total(&self) -> usize {
        self.passing.len().saturating_add(self.failing.len()).saturating_add(self.ignored.len())
    }

    pub fn report(&self) -> String {
        let mut out = format!(
            "workflows: {} / {} passing ({} failing, {} ignored)\n",
            self.passing.len(),
            self.total(),
            self.failing.len(),
            self.ignored.len()
        );
        for w in &self.failing {
            out.push_str(&format!("  FAILED  {w}\n"));
        }
        for (w, why) in &self.ignored {
            out.push_str(&format!("  ignored {w}: {}\n", if why.is_empty() { "(no reason given)" } else { why }));
        }
        out
    }
}

/// The workflow a libtest name belongs to: its first path segment of the form `wNN…`.
fn workflow(name: &str) -> Option<&str> {
    name.split("::").find(|seg| {
        let mut c = seg.chars();
        c.next() == Some('w') && c.next().is_some_and(|d| d.is_ascii_digit()) && c.next().is_some_and(|d| d.is_ascii_digit())
    })
}

/// Reads libtest's output (`test <name> ... ok | FAILED | ignored[, <reason>]`).
pub fn parse(output: &str) -> Stats {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum State {
        Passed,
        Ignored,
        Failed,
    }
    let mut seen: BTreeMap<String, (State, String)> = BTreeMap::new();
    for line in output.lines() {
        let Some((name, result)) = line.trim().strip_prefix("test ").and_then(|l| l.split_once(" ... ")) else { continue };
        let Some(w) = workflow(name.trim()) else { continue };
        let result = result.trim();
        let (state, why) = if result == "ok" {
            (State::Passed, String::new())
        } else if let Some(rest) = result.strip_prefix("ignored") {
            (State::Ignored, rest.trim_start_matches(',').trim().to_string())
        } else if result.starts_with("FAILED") {
            (State::Failed, String::new())
        } else {
            continue;
        };
        let entry = seen.entry(w.to_string()).or_insert((state, why.clone()));
        if state > entry.0 {
            *entry = (state, why);
        }
    }
    let mut stats = Stats::default();
    for (w, (state, why)) in seen {
        match state {
            State::Passed => stats.passing.push(w),
            State::Ignored => stats.ignored.push((w, why)),
            State::Failed => stats.failing.push(w),
        }
    }
    stats
}

/// The floor holds: at least `floor` workflows pass.
pub fn verdict(stats: &Stats, floor: usize) -> Result<(), String> {
    if stats.passing.len() < floor {
        return Err(format!(
            "stats: {} workflow(s) pass, under the floor of {floor} (xtask/src/stats.rs); a passing workflow was ignored or broke",
            stats.passing.len()
        ));
    }
    println!("stats: the floor holds ({} passing, floor {floor})", stats.passing.len());
    Ok(())
}

/// Runs the workflow tests (with `env`, e.g. full strength) and reads their results. No workflow
/// tests yet: none counted.
pub fn collect(root: &Path, env: &[(&str, &str)]) -> Result<Stats, String> {
    let dir = root.join("crates").join("mcp").join("tests");
    if !dir.join(TARGET).join("main.rs").is_file() && !dir.join(format!("{TARGET}.rs")).is_file() {
        println!("stats: no workflow tests yet (crates/mcp/tests/{TARGET}/)");
        return Ok(Stats::default());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args(["test", "--no-fail-fast", "-p", PACKAGE, "--test", TARGET]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.contains("test result:") {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("stats: the workflow tests did not run:\n{}", tail.join("\n")));
    }
    Ok(parse(&stdout))
}

/// `cargo xtask ci`'s step: the workflows run and the floor holds.
pub fn check(root: &Path, env: &[(&str, &str)]) -> Result<(), String> {
    let stats = collect(root, env)?;
    print!("{}", stats.report());
    verdict(&stats, FLOOR)
}

/// `cargo xtask stats [--check]`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if let Some(a) = args.iter().find(|a| a.as_str() != "--check") {
        return Err(format!("stats: unknown argument `{a}` (usage: cargo xtask stats [--check])"));
    }
    if args.is_empty() {
        let stats = collect(root, &[])?;
        print!("{}", stats.report());
        Ok(())
    } else {
        check(root, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "\nrunning 6 tests\n\
test harness::tests::pipes_frame_messages ... ok\n\
test w02_tray_across_folders::w02_tray_across_folders ... ok\n\
test w03_save_undo_revert::save_then_undo ... ok\n\
test w03_save_undo_revert::revert_restores_bytes ... ok\n\
test w07_batch_edit_one_step::w07_batch_edit_one_step ... ignored, M11: Batch Edit\n\
test w08_foreign_metadata ... FAILED\n\
test w12_destructive_asks ... ignored\n\n\
test result: FAILED. 4 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.20s\n";

    #[test]
    fn stats_count_workflows_with_the_ignored_ones_and_their_reasons() {
        let s = parse(OUTPUT);
        assert_eq!(s.passing, ["w02_tray_across_folders", "w03_save_undo_revert"]);
        assert_eq!(s.failing, ["w08_foreign_metadata"]);
        assert_eq!(
            s.ignored,
            [("w07_batch_edit_one_step".to_string(), "M11: Batch Edit".to_string()), ("w12_destructive_asks".to_string(), String::new())]
        );
        assert_eq!(s.total(), 5, "the harness's own test is no workflow");
        let report = s.report();
        assert!(report.starts_with("workflows: 2 / 5 passing (1 failing, 2 ignored)\n"), "{report}");
        assert!(report.contains("  ignored w07_batch_edit_one_step: M11: Batch Edit\n"), "{report}");
        // One ignored test makes its workflow ignored; one failing test makes it fail.
        let mixed = "test w03_x::a ... ok\ntest w03_x::b ... ignored, M12: later\ntest w04_y::a ... FAILED\ntest w04_y::b ... ignored, z\n";
        let s = parse(mixed);
        assert_eq!((s.passing.len(), s.ignored.len(), s.failing.len()), (0, 1, 1));
        assert_eq!(parse("running 0 tests\n"), Stats::default());
    }

    #[test]
    fn the_check_fails_under_the_floor() {
        let s = parse(OUTPUT);
        assert!(verdict(&s, 2).is_ok());
        let err = verdict(&s, 3).unwrap_err();
        assert!(err.contains("2 workflow(s) pass, under the floor of 3"), "{err}");
        // A passing workflow planted as ignored drops the count under the floor.
        let planted = OUTPUT.replace("w02_tray_across_folders ... ok", "w02_tray_across_folders ... ignored, planted");
        assert!(verdict(&parse(&planted), 2).is_err());
    }

    /// `--check` at the real floor: the passing workflows of today hold it, and one of them planted
    /// as `#[ignore]` fails it.
    #[test]
    fn a_planted_ignore_fails_the_check_at_the_floor() {
        let passing: String = (1..=FLOOR).map(|n| format!("test w{n:02}_workflow::w{n:02}_workflow ... ok\n")).collect();
        assert!(verdict(&parse(&passing), FLOOR).is_ok());
        let planted = passing.replacen("... ok", "... ignored, planted", 1);
        assert_eq!(parse(&planted).passing.len(), FLOOR - 1);
        assert!(verdict(&parse(&planted), FLOOR).is_err(), "an ignored workflow drops the count under the floor");
        assert!(verdict(&Stats::default(), FLOOR).is_err(), "no workflow running fails the floor");
    }
}
