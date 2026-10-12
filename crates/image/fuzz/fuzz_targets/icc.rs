//! Fuzz target `icc`: ICC profile checks and parsing, then the colour stage with the profile on a few pixels of every model and depth.

#![no_main]

use dalica_image::color::{load_profile, to_srgb8};
use dalica_image::{Budget, ColorModel, DecodeOptions, Decoded, Limits, Orientation, Samples};
use libfuzzer_sys::fuzz_target;

/// Small enough that the fuzzer never runs out of memory on a legitimate allocation.
fn limits() -> Limits {
    Limits { max_alloc: 64 << 20, max_pixels: 1 << 24, ..Limits::default() }
}

fuzz_target!(|data: &[u8]| {
    let l = limits();
    for model in [ColorModel::Gray, ColorModel::Rgb, ColorModel::Cmyk] {
        let _ = load_profile(data, model, &l);
        let n = model.channels() * 4;
        for samples in [Samples::U8(vec![128; n]), Samples::U16(vec![40_000; n]), Samples::F32(vec![0.25; n])] {
            let d = Decoded { width: 2, height: 2, model, alpha: false, samples, icc: Some(data.to_vec()), orientation: Orientation::UPRIGHT };
            let mut budget = Budget::new(&l, 2, 2);
            let _ = to_srgb8(d, &DecodeOptions::default(), &mut budget, &l);
        }
    }
});
