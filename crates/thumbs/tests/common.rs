//! Helpers shared by the test modules of `all.rs`.

use proptest::test_runner::Config;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's cases: `full` at full strength, `quick` in a task's gate.
pub fn cases(full: u32, quick: u32) -> Config {
    Config::with_cases(if self::full() { full } else { quick })
}
