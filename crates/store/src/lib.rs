//! dalica-store: Persistence of the model: index journal + snapshot, model ↔ sidecar mapping, the debounced sidecar writer, the metadata write queue, rebuild from folders.
//!
//! Layer L3; see `docs/architecture.md` §2. A stub until M1.9, M1.10.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
