//! Helpers shared by the test modules of `all.rs`.

use dalica_core::Timestamp;
use dalica_fs::faults::{Fault, points};
use proptest::test_runner::Config;

/// A fixed "now": 2026-10-03.
pub const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

/// One fault of each kind for a write this crate makes through `dalica-fs`'s atomic replace: an
/// error before the rename, an error after it, a crash. `dalica-fs`'s own tests try every point
/// (`ATOMIC_POINTS`) as an error and as a crash; here each kind shows the store copes with it.
pub const FAULTS: [(&str, Fault); 3] = [(points::WRITE, Fault::Error), (points::VERIFY, Fault::Error), (points::RENAME, Fault::Crash)];

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's cases: `full` at full strength, `quick` in a task's gate.
pub fn cases(full: u32, quick: u32) -> Config {
    Config::with_cases(if self::full() { full } else { quick })
}

pub fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}
