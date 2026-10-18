//! dalica-faces: Face detection, alignment, embeddings and clustering on decoded RGB buffers.
//!
//! Layer L1; see `docs/architecture.md` §2 and `docs/faces.md`. Built so far (M13.6): the
//! pure-Rust ONNX interpreter the detector and the recogniser run on: [`onnx`] (a probe of a
//! file's inputs and outputs), [`graph`] (a bounded read of a whole model), [`net`] (the
//! single-threaded float32 CPU interpreter), [`runtime`] (a loaded, checked [`runtime::Model`]),
//! [`limits`] (every limit a model is held to) and [`synthetic`] (seeded networks for tests); and
//! (M13.7) grouping and suggestions as pure maths on unit embeddings: [`calibration`] (confidence
//! to cosine), [`cluster`] (grouping unnamed faces), [`matching`] (name suggestions) and
//! [`evaluate`] (suggestion quality on the user's own named faces).
//!
//! Every model file is hostile input: sizes are capped, numbers must be finite, and a malformed or
//! unsupported file is an error naming why, never a panic.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod calibration;
pub mod cluster;
pub mod evaluate;
pub mod graph;
pub mod limits;
pub mod matching;
pub mod net;
pub mod onnx;
pub mod runtime;
pub mod synthetic;
