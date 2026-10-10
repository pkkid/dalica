//! dalica-scan: Watched-folder rules, walks, change detection, `notify` events, offline volumes and card/folder import; turns what it finds into `Op`s.
//!
//! Layer L3; see `docs/architecture.md` §2. A stub until M1.11.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
