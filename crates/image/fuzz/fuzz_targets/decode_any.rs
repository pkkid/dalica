//! Fuzz target `decode_any`: Every entry point on any bytes, colour management on and off.

#![no_main]

use dalica_image::{DecodeOptions, Limits, decode_thumbnail, decode_with, read_header};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    let l = limits();
    let _ = read_header(data, &l);
    for color_managed in [true, false] {
        let _ = decode_with(data, &DecodeOptions { color_managed }, &l);
    }
    let _ = decode_thumbnail(data, 128, &l);
});
