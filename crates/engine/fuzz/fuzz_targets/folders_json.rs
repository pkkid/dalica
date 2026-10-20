//! Fuzz target `folders_json`: any bytes as the library's `folders.json`, the watched folders and
//! their rules (`data-model.md` §4, `security.md` §4.2, §7.1), read as a session reads it when it
//! opens. Whatever the input: refused (not JSON, not an object with a `folders` array, a version
//! that is not a whole number), and a session keeps the file aside as damaged; or read, and then
//!
//! - every folder read is one absolute path in its normal form, read once, with one rule that sets
//!   something (Scan Once alone may be marked indexed); the units are the folders the rules index;
//! - every entry of the file is either a folder read or an entry kept as it was, and every
//!   top-level key Dalica does not know is kept with its value;
//! - the file written back (v2) reads again as the same folders, rules, unknown keys and kept
//!   entries, and writing that again gives the same bytes.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_engine::folders::parse_for_fuzzing;
use dalica_scan::{Mode, folder_path};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let Some(read) = parse_for_fuzzing(data) else { return };
    assert!(read.rules.windows(2).all(|w| w[0].0 < w[1].0), "a folder is read twice");
    for (path, rule) in &read.rules {
        assert_eq!(folder_path(path).as_ref(), Ok(path), "a folder read is not in its normal form");
        assert!(!rule.is_empty(), "{}: a rule that sets nothing", path.display());
        assert!(!rule.indexed || rule.mode == Some(Mode::Once), "{}: indexed without Scan Once", path.display());
    }
    for path in read.entry_extra.keys() {
        assert!(read.rules.iter().any(|(p, _)| p == path), "unknown keys of a folder not read");
    }
    let units: Vec<_> = read.rules.iter().filter(|(_, r)| r.mode.is_some_and(Mode::indexes)).map(|(p, _)| p.clone()).collect();
    assert_eq!(read.roots, units);

    // What the file held: each entry read or kept, each unknown top-level key kept.
    let Ok(Value::Object(mut top)) = serde_json::from_slice::<Value>(data) else { panic!("a refused file was read") };
    let Some(Value::Array(entries)) = top.remove("folders") else { panic!("a file without a folders array was read") };
    top.remove("version");
    assert_eq!(read.extra, top, "the unknown top-level keys changed");
    assert_eq!(read.rules.len() + read.unreadable.len(), entries.len(), "an entry was dropped");
    assert!(read.unreadable.iter().all(|e| entries.contains(e)), "a kept entry changed");

    let again = parse_for_fuzzing(&read.written).unwrap_or_else(|| panic!("a written folders.json does not read"));
    assert_eq!(again.version, 2);
    assert_eq!(again.rules, read.rules);
    assert_eq!(again.roots, read.roots);
    assert_eq!(again.extra, read.extra);
    assert_eq!(again.entry_extra, read.entry_extra);
    assert_eq!(again.unreadable, read.unreadable);
    assert!(again.written == read.written, "writing folders.json again changed it");
});
