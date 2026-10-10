//! Whether two `files` patterns of different tasks can name the same file. Conservative: two
//! patterns overlap when the literal part of one (up to its first wildcard) is a path-prefix of the
//! literal part of the other, or both are the same literal path.

use super::config::SHARED_FILES;

fn literal(pattern: &str) -> (&str, bool) {
    match pattern.find(['*', '?', '[', '{']) {
        Some(i) => (pattern.get(..i).unwrap_or(""), true),
        None => (pattern, false),
    }
}

pub fn patterns_overlap(a: &str, b: &str) -> bool {
    if SHARED_FILES.contains(&a) || SHARED_FILES.contains(&b) {
        return false;
    }
    let (la, wa) = literal(a);
    let (lb, wb) = literal(b);
    match (wa, wb) {
        (false, false) => la == lb,
        // `crates/fs/**` (literal `crates/fs/`) covers `crates/fs/src/lib.rs` and `crates/fs/**`.
        (true, false) => lb.starts_with(la),
        (false, true) => la.starts_with(lb),
        (true, true) => la.starts_with(lb) || lb.starts_with(la),
    }
}

pub fn lists_overlap(a: &[String], b: &[String]) -> Option<(String, String)> {
    a.iter().find_map(|x| b.iter().find(|y| patterns_overlap(x, y)).map(|y| (x.clone(), y.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_rules() {
        assert!(patterns_overlap("crates/**", "crates/fs/src/lib.rs"));
        assert!(patterns_overlap("crates/fs/src/lib.rs", "crates/fs/src/lib.rs"));
        assert!(patterns_overlap("apps/dalica-cli/src/**", "apps/dalica-cli/src/run.rs"));
        assert!(patterns_overlap("crates/engine/src/cmd/**", "crates/engine/src/cmd/app.rs"));
        assert!(patterns_overlap("crates/fs/**", "crates/fs/tests/**"));
        assert!(!patterns_overlap("crates/fs/src/lib.rs", "crates/fs/tests/**"));
        assert!(!patterns_overlap("crates/fs/src/atomic.rs", "crates/fs/src/lock.rs"));
        assert!(!patterns_overlap("crates/ui/**", "crates/ui-kit/src/theme.rs"));
        assert!(!patterns_overlap("docs/roadmap.md", "docs/roadmap.md"));
        assert!(!patterns_overlap("Cargo.lock", "Cargo.lock"));
        assert!(!patterns_overlap("xtask/src/main.rs", "xtask/src/main.rs"));
        assert!(patterns_overlap("crates/fs/Cargo.toml", "crates/fs/**"));
        assert!(!patterns_overlap("xtask/src/deps.rs", "xtask/src/names.rs"));
    }
}
