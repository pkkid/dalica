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

/// Runs the test; `quiet` skips printing the manual checks (used by `ci`).
pub fn accept(root: &Path, milestone: u32, headless: bool, quiet: bool) -> Result<(), String> {
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
    accept(root, milestone, headless, false)
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
        assert!(accept(dir.path(), 999, false, true).unwrap_err().contains("no acceptance test"));
        // Without a headless half, --headless still looks for the milestone's own test.
        assert!(accept(dir.path(), 999, true, true).unwrap_err().contains("m999.rs is missing"));
    }
}
