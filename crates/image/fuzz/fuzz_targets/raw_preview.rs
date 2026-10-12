//! Fuzz target `raw_preview`: Every RAW container module's preview decode and recognition.

#![no_main]

use dalica_image::{DecodeOptions, Format, Limits, RawKind, decode_as, formats};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    let l = limits();
    let _ = formats::raw::recognise(data);
    for kind in RawKind::ALL {
        let _ = decode_as(Format::Raw(kind), data, &DecodeOptions::default(), &l);
    }
});
