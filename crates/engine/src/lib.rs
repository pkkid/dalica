//! dalica-engine: `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo library setup.
//!
//! Layer L4; see `docs/architecture.md` §2. A stub until M1.13, M1.14.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
