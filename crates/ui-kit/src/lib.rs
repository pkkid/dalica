//! dalica-ui-kit: The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots.
//!
//! Layer L5; see `docs/architecture.md` §2. A stub until M1.16.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod theme;

pub use theme::Tokens;
