//! `batch {steps, stopOnError?}` (`control-protocol.md` §6.3): several command and method steps in
//! one request, in order, with the semantics of separate requests. Its entry of
//! [`METHODS`](super::METHODS) is [`METHOD`], so building it changes only this module. Not built
//! yet: it answers `unsupported`.

use super::{Method, later};

/// The method's entry in [`METHODS`](super::METHODS).
pub(super) const METHOD: Method = later("batch", "{steps, stopOnError?}", "{completed, failed, results}", "planned for M10 (M10.8)");
