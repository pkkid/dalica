//! dalica-faces: Face detection, alignment, embeddings and clustering on decoded RGB buffers.
//!
//! Layer L1; see `docs/architecture.md` §2 and `docs/faces.md`. Built so far (M13.6): the
//! pure-Rust ONNX interpreter the detector and the recogniser run on: [`onnx`] (a probe of a
//! file's inputs and outputs), [`graph`] (a bounded read of a whole model), [`net`] (the
//! single-threaded float32 CPU interpreter), [`runtime`] (a loaded, checked [`runtime::Model`]),
//! [`limits`] (every limit a model is held to) and [`synthetic`] (seeded networks for tests); and
//! (M13.7) grouping and suggestions as pure maths on unit embeddings: [`calibration`] (confidence
//! to cosine), [`cluster`] (grouping unnamed faces), [`matching`] (name suggestions) and
//! [`evaluate`] (suggestion quality on the user's own named faces); and (M13.10) the recogniser on
//! top of them: [`detect`] (boxes, scores and five landmarks normalised on the photo), [`align`]
//! (the 112 × 112 crop), [`embed`] (one unit vector per face), [`manifest`] and [`catalog`] (what a
//! model is and the user's own list), [`known`] (models known by hash, the built-in list, dropped
//! files), [`hash`] and [`selftest`] (the checks before an install).
//!
//! Every model file is hostile input: sizes are capped, numbers must be finite, and a malformed or
//! unsupported file is an error naming why, never a panic.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod align;
pub mod calibration;
pub mod catalog;
pub mod cluster;
pub mod detect;
pub mod embed;
pub mod evaluate;
pub mod graph;
pub mod hash;
pub mod known;
pub mod limits;
pub mod manifest;
pub mod matching;
pub mod net;
pub mod onnx;
pub mod runtime;
pub mod selftest;
pub mod synthetic;
