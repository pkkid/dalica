//! Helpers shared by the test modules of `all.rs`.

use std::path::Path;

use dalica_library::{Library, Op};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// Writes `bytes` to `path`, making its folders.
pub fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

pub fn apply_all(lib: &mut Library, ops: Vec<Op>) {
    for op in ops {
        lib.apply(op).unwrap();
    }
}
