//! Fuzz target `snapshot`: any bytes as the index snapshot `index.snap` (`security.md` §4.2,
//! §7.1). The first byte picks how the rest is read: as the file's bytes as they are, or framed as
//! one checksummed record (so the JSON inside it is reached). Whatever the input: refused with a
//! reason, or loaded through the ops that build it; a loaded model, written as a snapshot, loads
//! back at the same op number and writes the same bytes again. Never a panic, a hang or an
//! allocation far past the input.

#![no_main]

mod common;

use dalica_store::snapshot;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode & 1 == 0 {
        rest.to_vec()
    } else {
        common::framed(&rest.iter().map(|&b| if b == b'\n' { b' ' } else { b }).collect::<Vec<u8>>(), false)
    };
    if let Ok((library, seq)) = snapshot::read(&bytes) {
        common::snapshot_fixed_point(&library, seq);
    }
});
