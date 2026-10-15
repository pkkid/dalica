//! Helpers shared by the test modules of `all.rs`.

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::Photo;
use proptest::test_runner::Config;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's cases: `full` at full strength, `quick` in a task's gate.
pub fn cases(full: u32, quick: u32) -> Config {
    Config::with_cases(if self::full() { full } else { quick })
}

/// A photo `name` in `folder`: 10 bytes, 4×3, a 2023 file time, no date, no star. Files change
/// what their tests need with `Photo { size, date, ..photo(id, folder, name) }`.
pub fn photo(id: PhotoId, folder: FolderId, name: &str) -> Photo {
    Photo { id, folder, name: name.into(), size: 10, mtime: Timestamp::from_millis(1_700_000_000_000), width: 4, height: 3, date: None, star: false }
}
