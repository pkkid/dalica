//! Fuzz target `embedded_thumbnail`: Embedded thumbnail and preview location in every format, then decoding what it points at.

#![no_main]

use dalica_image::{Format, Limits, decode_thumbnail, formats};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    let l = limits();
    for format in Format::ALL {
        if let Ok(Some(range)) = formats::embedded_thumbnail(format, data, &l)
            && let Some(thumb) = data.get(range)
        {
            let _ = decode_thumbnail(thumb, 128, &l);
        }
    }
});
