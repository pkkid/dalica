//! dalica-image: Sniffing, scaled decoding, encoding, orientation, ICC → sRGB, decode limits, embedded thumbnails and RAW previews (a bounded TIFF/IFD walker of our own).
//!
//! Layer L1; see `docs/architecture.md` §2. A stub until M1.5, M1.6.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
