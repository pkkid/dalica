//! dalica-fs: Every write to the user's disk: atomic replace + fsync + verify, the read-before-write guard, the library lock, move/copy with verify, the OS trash, bounded walks, the UI-thread guard and fault-injection points (feature `faults`).
//!
//! Layer L0; see `docs/architecture.md` §2. A stub until M1.2, M1.3.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
