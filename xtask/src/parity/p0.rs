//! `cargo xtask parity --p0` (`process.md` §5): lists every P0 row of `docs/parity.md` that is not
//! ✅ and fails while one remains. A 🟡 row whose note ends "awaiting the M<n>.R manual check" for
//! the current milestone (the first with an unfinished task block) is a check only a person can
//! make at that review: it is listed apart and accepted. The same note naming another milestone
//! fails like any other row.

use std::path::Path;

use super::Row;
use crate::tasks::roadmap::Roadmap;

/// The milestone whose review a row may wait for: the first one with an unfinished task block.
pub fn current_milestone(roadmap: &Roadmap) -> Option<u32> {
    roadmap.tasks.iter().filter(|t| !t.done).map(|t| t.milestone).min()
}

/// `n` when `note` ends "awaiting the M<n>.R manual check" (a final full stop allowed).
pub fn awaiting(note: &str) -> Option<u32> {
    let note = note.trim_end().trim_end_matches('.');
    let rest = note.strip_suffix(".R manual check")?;
    let at = rest.rfind("awaiting the M")?;
    rest.get(at + "awaiting the M".len()..)?.parse().ok()
}

/// The P0 rows that are not ✅: the ones that fail the gate, and the ones accepted as awaiting the
/// current milestone's manual check.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gate {
    pub open: Vec<Row>,
    pub awaiting: Vec<Row>,
    pub done: usize,
}

pub fn gate(rows: &[Row], current: Option<u32>) -> Gate {
    let mut g = Gate::default();
    for r in rows.iter().filter(|r| r.tier.as_deref() == Some("P0")) {
        if r.status == "✅" {
            g.done = g.done.saturating_add(1);
        } else if r.status == "🟡" && current.is_some() && awaiting(&r.evidence) == current {
            g.awaiting.push(r.clone());
        } else {
            g.open.push(r.clone());
        }
    }
    g
}

/// The report `--p0` prints.
pub fn report(g: &Gate, current: Option<u32>) -> String {
    let short = |r: &Row| {
        let note: String = r.evidence.chars().take(110).collect();
        let more = if r.evidence.chars().count() > 110 { "…" } else { "" };
        format!("  {} {} ({}): {note}{more}\n", r.status, r.id, r.section)
    };
    let mut out = String::new();
    if !g.open.is_empty() {
        out.push_str(&format!("P0 rows not ✅ ({}):\n", g.open.len()));
        g.open.iter().for_each(|r| out.push_str(&short(r)));
    }
    if !g.awaiting.is_empty() {
        let m = current.map_or_else(String::new, |m| format!("M{m}.R "));
        out.push_str(&format!("awaiting the {m}manual check, accepted ({}):\n", g.awaiting.len()));
        g.awaiting.iter().for_each(|r| out.push_str(&short(r)));
    }
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join("docs").join("parity.md")).map_err(|e| format!("cannot read docs/parity.md: {e}"))?;
    let roadmap = crate::tasks::load_roadmap(root)?;
    let current = current_milestone(&roadmap);
    let g = gate(&super::parse_rows(&text), current);
    print!("{}", report(&g, current));
    if g.open.is_empty() {
        println!("parity --p0: ok ({} P0 rows ✅, {} awaiting the manual check)", g.done, g.awaiting.len());
        Ok(())
    } else {
        Err(format!("parity --p0: {} P0 row(s) not ✅ ({} ✅, {} awaiting the manual check)", g.open.len(), g.done, g.awaiting.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rows: &[(&str, &str, &str, &str)]) -> Vec<Row> {
        let mut text =
            String::from("# P\n\n## A. Library view (LIB)\n\n| Id | Feature | Tier | Status | Evidence / notes |\n|---|---|---|---|---|\n");
        for (id, tier, status, note) in rows {
            text.push_str(&format!("| {id} | f | {tier} | {status} | {note} |\n"));
        }
        super::super::parse_rows(&text)
    }

    #[test]
    fn a_p0_row_not_done_fails_the_gate() {
        let rows =
            doc(&[("LIB-A", "P0", "✅", "done"), ("LIB-B", "P0", "⬜", "missing"), ("LIB-C", "P1", "⬜", "later"), ("LIB-D", "P0", "🟡", "partial")]);
        let g = gate(&rows, Some(10));
        assert_eq!(g.open.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["LIB-B", "LIB-D"]);
        assert_eq!((g.done, g.awaiting.len()), (1, 0));
        assert!(report(&g, Some(10)).contains("  ⬜ LIB-B (A. Library view): missing\n"));
        // Every P0 row done: the gate passes.
        assert!(gate(&doc(&[("LIB-A", "P0", "✅", "done"), ("LIB-C", "P1", "⬜", "later")]), Some(10)).open.is_empty());
    }

    #[test]
    fn an_awaiting_row_of_the_current_milestone_is_listed_apart() {
        let rows = doc(&[
            ("LIB-A", "P0", "🟡", "pinch measured on Linux; awaiting the M10.R manual check"),
            ("LIB-B", "P0", "🟡", "the macOS build, awaiting the M9.R manual check."),
            ("LIB-C", "P0", "⬜", "awaiting the M10.R manual check"),
            ("LIB-D", "P0", "🟡", "awaiting the M10.R manual check, then more"),
        ]);
        let g = gate(&rows, Some(10));
        assert_eq!(g.awaiting.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["LIB-A"]);
        // Another milestone's review, a ⬜ row and a note that does not end so all fail.
        assert_eq!(g.open.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["LIB-B", "LIB-C", "LIB-D"]);
        assert!(report(&g, Some(10)).contains("awaiting the M10.R manual check, accepted (1):\n  🟡 LIB-A"));
        assert_eq!(awaiting("x; awaiting the M12.R manual check."), Some(12));
        assert_eq!(awaiting("awaiting the Mx.R manual check"), None);
    }

    #[test]
    fn the_current_milestone_is_the_first_with_an_unfinished_task() {
        let text = "| P | M0 | A | a | ⬜ |\n| | M1 | B | b | ⬜ |\n\n- ✅ **M0.1 X**\n  - after: —\n  - files: —\n  - scope: s\n  - done when: d\n\
                    - ⬜ **M0.R Y**\n  - after: M0.1\n  - owner: maintainer\n  - files: —\n  - scope: s\n  - done when: d\n- ⬜ **M1.1 Z**\n  - after: M0.R\n  - files: —\n  - scope: s\n  - done when: d\n";
        let r = crate::tasks::roadmap::parse(text).unwrap();
        assert_eq!(current_milestone(&r), Some(0));
    }
}
