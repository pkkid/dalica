//! Helpers shared by the test modules of `all.rs`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_fs::atomic::is_temp_name;

/// 2001-02-03 04:05:06.789 UTC: a time no test file has by accident.
pub fn old_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(981_173_106_789)
}

/// The modification time of `p` itself (a link is not followed).
pub fn modified(p: &Path) -> SystemTime {
    fs::symlink_metadata(p).unwrap().modified().unwrap()
}

/// Writes `bytes` to `p` and gives it [`old_time`].
pub fn old_file(p: &Path, bytes: &[u8]) {
    fs::write(p, bytes).unwrap();
    fs::File::options().write(true).open(p).unwrap().set_modified(old_time()).unwrap();
}

/// Every name in `dir`, sorted.
pub fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// How many temporary files of an atomic write are in `dir`.
pub fn temps(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().filter(|e| is_temp_name(&e.as_ref().unwrap().file_name())).count()
}
