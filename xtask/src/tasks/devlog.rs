//! The private dev log, `log/devlog.md` in the main checkout (`docs/task-board.md` §8).
//!
//! Entries look like `## 2026-10-09 14:02 · agent-3 · M1.4 scenes` followed by the text.

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn path(main_checkout: &Path) -> PathBuf {
    main_checkout.join("log").join("devlog.md")
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix time → local `YYYY-MM-DD HH:MM`.
pub fn format_local(unix: i64, offset: i64) -> String {
    let t = unix.saturating_add(offset);
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Local `YYYY-MM-DD HH:MM` → unix time.
pub fn parse_local(s: &str, offset: i64) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let (h, min) = time.split_once(':')?;
    let (h, min): (i64, i64) = (h.parse().ok()?, min.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(0..24).contains(&h) || !(0..60).contains(&min) {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60 - offset)
}

/// Puts `Open:` and `Numbers:` on lines of their own, so a one-line message reads like §8.
fn shape(text: &str) -> String {
    let mut out = text.trim().to_string();
    for key in ["Open:", "Numbers:"] {
        out = out.replace(&format!(" {key}"), &format!("\n{key}"));
    }
    out
}

pub fn entry(now: i64, offset: i64, agent: &str, heading: &str, text: &str) -> String {
    format!("\n## {} · {agent} · {heading}\n{}\n", format_local(now, offset), shape(text))
}

/// Times of the entries for `task_id` (`## <time> · <agent> · <id> …`).
pub fn entry_times(log: &str, task_id: &str, offset: i64) -> Vec<i64> {
    log.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .filter_map(|l| {
            let mut parts = l.split(" · ");
            let when = parts.next()?;
            let _agent = parts.next()?;
            let what = parts.next()?;
            (what == task_id || what.starts_with(&format!("{task_id} "))).then(|| parse_local(when, offset)).flatten()
        })
        .collect()
}

/// The newest entry for `task_id`: its heading (without `## `) and its text, up to the next entry.
pub fn latest_entry(log: &str, task_id: &str) -> Option<(String, String)> {
    let mut found = None;
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in log.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(done) = current.take() {
                found = Some(done);
            }
            let what = heading.split(" · ").nth(2).unwrap_or("");
            if what == task_id || what.starts_with(&format!("{task_id} ")) {
                current = Some((heading.to_string(), Vec::new()));
            }
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    if let Some(done) = current {
        found = Some(done);
    }
    found.map(|(heading, lines)| (heading, lines.join("\n").trim().to_string()))
}

pub fn append(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    f.write_all(text.as_bytes()).and_then(|()| f.sync_all()).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        // 2026-10-09 14:02 EDT = 18:02 UTC
        let t = parse_local("2026-10-09 14:02", -4 * 3600).unwrap();
        assert_eq!(t, 1_791_568_920);
        assert_eq!(format_local(t, -4 * 3600), "2026-10-09 14:02");
        assert_eq!(format_local(0, 0), "1970-01-01 00:00");
        assert_eq!(format_local(951_782_400, 0), "2000-02-29 00:00");
        assert!(parse_local("2026-13-01 00:00", 0).is_none());
    }

    #[test]
    fn entries() {
        let e = entry(1_791_568_920, -4 * 3600, "agent-3", "M1.4 scenes", "Landed: a. Open: b. Numbers: c.");
        assert_eq!(e, "\n## 2026-10-09 14:02 · agent-3 · M1.4 scenes\nLanded: a.\nOpen: b.\nNumbers: c.\n");
        let log = format!("# log\n{e}\n## 2026-10-09 15:00 · agent-2 · M1.40 other\nx\n");
        assert_eq!(entry_times(&log, "M1.4", -4 * 3600), [1_791_568_920]);
        assert!(entry_times(&log, "M1.5", 0).is_empty());
    }

    #[test]
    fn latest_entry_is_the_newest_for_the_task() {
        let log = "# log\n\n## 2026-10-09 14:02 · agent-3 · M1.4 scenes\nLanded: first.\n\n\
                   ## 2026-10-09 15:00 · agent-2 · M1.40 other\nnot this\n\n\
                   ## 2026-10-09 16:00 · agent-3 · M1.4 scenes\nLanded: second.\n### a sub-heading\nOpen: x.\n\n\
                   ## 2026-10-09 17:00 · agent-1 · M1.5 next\ny\n";
        let (heading, text) = latest_entry(log, "M1.4").unwrap();
        assert_eq!(heading, "2026-10-09 16:00 · agent-3 · M1.4 scenes");
        assert_eq!(text, "Landed: second.\n### a sub-heading\nOpen: x.");
        // The last entry of the file ends at the end of the file.
        assert_eq!(latest_entry(log, "M1.5").unwrap().1, "y");
        assert!(latest_entry(log, "M1.6").is_none());
        assert!(latest_entry("", "M1.4").is_none());
    }
}
