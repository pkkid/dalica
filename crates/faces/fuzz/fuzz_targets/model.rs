//! Fuzz target `model`: any bytes as a face model (`faces.md` §2, `security.md` §4.2). Whatever the
//! input, `Model::from_bytes` refuses it with a reason or loads it; a model that loads runs on a
//! fixed input of its declared shape (each dynamic size taken as 1) and gives finite outputs of the
//! shapes `check_input` promised, or an error. Never a panic, an abort or an allocation past the
//! limits.

#![no_main]

use dalica_faces::net::Tensor;
use dalica_faces::runtime::Model;
use libfuzzer_sys::fuzz_target;

/// The largest input run, in elements: the run is about the operators, not the input's size.
const MAX_INPUT: usize = 1 << 16;

fuzz_target!(|data: &[u8]| {
    let Ok(model) = Model::from_bytes(data) else { return };
    let shape: Vec<usize> = model.input_shape().iter().map(|&d| d.max(1)).collect();
    let n: usize = shape.iter().product();
    if shape.is_empty() || n > MAX_INPUT {
        return;
    }
    let Ok(promised) = model.check_input(&shape) else { return };
    let input = (0..n).map(|i| (i % 251) as f32 / 125.0 - 1.0).collect();
    let Ok(input) = Tensor::new(shape, input) else { return };
    if let Ok(outputs) = model.run(input) {
        assert_eq!(outputs.len(), promised.len());
        for ((_, t), shape) in outputs.iter().zip(&promised) {
            assert_eq!(&t.shape, shape);
            assert!(t.data.iter().all(|v| v.is_finite()));
        }
    }
});
