//! dalica-ui: The app: serialisable `UiState`, panels, menus and the menu catalog, shortcut dispatch, `UI_COMMANDS`, the headless driver and the control-method handlers.
//!
//! Layer L5; see `docs/architecture.md` §2, §7 and §10.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod app;
pub mod app_icon;
pub mod commands;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod drag;
pub mod headless;
pub mod keys;
pub mod menu_catalog;
pub mod menus;
pub mod notices;
pub mod panels;
pub mod perf;
pub mod picker;
pub mod scale;
pub mod selection;
pub mod state;
pub mod status_text;
pub mod titlebar;
pub mod tray;
pub mod user_activity;

pub use app::App;
pub use headless::{Driver, DriverError};
pub use menus::{UI_COMMANDS, UiCommand};
pub use perf::Perf;
pub use picker::{Picker, ScriptedPicker};
pub use state::UiState;
