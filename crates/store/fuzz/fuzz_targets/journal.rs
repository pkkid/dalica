//! Fuzz target `journal`: any bytes as the index journal `index.log` (`security.md` §4.2, §7.1).
//! The first byte picks how the rest is read: as the file's bytes as they are (checksums, torn
//! tails and damaged lines reached as they come), or with each line framed as a checksummed record
//! (so the header and the ops inside the records are reached), its last newline kept or torn off.
//! Whatever the input:
//!
//! - reading stops at the first record that is torn, fails its checksum or does not parse, and
//!   everything before it is whole: the record ends rise, the last is the valid length, there is
//!   one end per record, ops only after a header, and the valid prefix reads again as the same
//!   records with nothing stopped;
//! - the ops replay onto an empty model the way opening the store does, stopping at the first that
//!   does not apply; the model they build, written as a snapshot, loads back and writes the same
//!   bytes again.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

mod common;

use dalica_library::Library;
use dalica_store::journal;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode & 1 == 0 { rest.to_vec() } else { common::framed(rest, mode & 2 != 0) };
    let c = journal::read(&bytes);
    assert!(c.valid_len <= bytes.len());
    assert_eq!(c.ends.len(), c.ops.len() + usize::from(c.header.is_some()));
    assert!(c.header.is_some() || c.ops.is_empty());
    assert!(c.ends.windows(2).all(|w| w[0] < w[1]), "record ends do not rise");
    assert_eq!(c.ends.last().copied().unwrap_or(0), c.valid_len);
    assert_eq!(c.stop.is_none(), c.valid_len == bytes.len());
    let prefix = journal::read(&bytes[..c.valid_len]);
    assert_eq!((&prefix.header, &prefix.ops, &prefix.ends, prefix.valid_len), (&c.header, &c.ops, &c.ends, c.valid_len));
    assert_eq!(prefix.stop, None);

    let Some(header) = c.header else { return };
    let mut library = Library::new();
    let mut applied = 0u64;
    for op in c.ops {
        if library.apply(op).is_err() {
            break;
        }
        applied += 1;
    }
    if applied > 0 {
        common::snapshot_fixed_point(&library, header.base.saturating_add(applied));
    }
});
