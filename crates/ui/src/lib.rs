//! dalica-ui: The app: serialisable `UiState`, panels, menus and the menu catalog, shortcut dispatch, `UI_COMMANDS`, the headless driver and the control-method handlers.
//!
//! Layer L5; see `docs/architecture.md` §2, §7 and §10.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod app;
pub mod control;
pub mod headless;
pub mod keys;
pub mod panels;
pub mod perf;
pub mod picker;
pub mod state;

pub use app::{App, UI_COMMANDS, UiCommand};
pub use headless::{Driver, DriverError};
pub use perf::Perf;
pub use picker::{Picker, ScriptedPicker};
pub use state::UiState;
