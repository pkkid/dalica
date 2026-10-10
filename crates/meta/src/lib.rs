//! dalica-meta: EXIF / IPTC / XMP reading; write-back of owned fields by segment surgery (merge, never replace); MWG face regions; `.xmp` sidecars for RAW.
//!
//! Layer L1; see `docs/architecture.md` §2. A stub until M6.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
