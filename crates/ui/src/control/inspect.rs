//! `library.inspect {photos?, section?, offset?, limit?}` (`control-protocol.md` §6.4): the library
//! as the user sees it, the engine query of that name plus the UI's fields. Its entry of
//! [`METHODS`](super::METHODS) is [`METHOD`], so building it changes only this module. Not built
//! yet: it answers `unsupported`.

use super::{Method, later};

/// The method's entry in [`METHODS`](super::METHODS).
pub(super) const METHOD: Method = later(
    "library.inspect",
    "{photos?, section?, offset?, limit?}",
    "the library as the user sees it",
    "planned for M10 (M10.7); use the library.query command",
);
