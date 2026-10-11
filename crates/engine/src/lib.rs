//! dalica-engine: `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo library setup.
//!
//! Layer L4; see `docs/architecture.md` §2–§5. [`Session::execute`] is the one seam every surface
//! (UI, keyboard, CLI, control channel, MCP) goes through; commands are [`CommandSpec`]s
//! registered per area in `cmd/`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
pub mod demo;
pub mod error;
pub mod keymap;
pub mod registry;
pub mod session;
pub mod settings;
pub mod synthetic;
pub mod thumbs;
pub mod undo;

pub use error::{CmdError, OpenError};
pub use keymap::{Binding, Key, Keymap, Override};
pub use registry::{CommandInfo, CommandSpec, Context, Disabled, Effect, History};
pub use session::{Options, Session};
pub use thumbs::{CacheDir, Thumb, ThumbOptions, ThumbStats};
