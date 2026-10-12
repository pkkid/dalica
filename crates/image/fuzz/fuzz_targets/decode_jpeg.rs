//! Fuzz target `decode_jpeg`: The Jpeg module, without sniffing: header, decode (colour management on and off) and embedded thumbnail.

#![no_main]

use dalica_image::{DecodeOptions, Format, Limits, decode_as, formats};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    let l = limits();
    let _ = formats::header(Format::Jpeg, data, &l);
    let _ = formats::embedded_thumbnail(Format::Jpeg, data, &l);
    for color_managed in [true, false] {
        let _ = decode_as(Format::Jpeg, data, &DecodeOptions { color_managed }, &l);
    }
});
