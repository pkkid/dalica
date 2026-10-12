//! Fuzz target `sniff`: Sniffing: any bytes give a format or `None`; a sniffed format's header read never panics.

#![no_main]

use dalica_image::{Limits, formats, sniff};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    if let Some(format) = sniff(data) {
        let _ = formats::header(format, data, &limits());
    }
});
