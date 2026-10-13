//! The edit view's zoom commands (`editing.md` §3): Fit, 1:1, the zoom slider, Ctrl+= and Ctrl+−.
//! A stub until the photo canvas lands (M7.15); the zoom itself is the edit state's `zoom`
//! ([`crate::state::Zoom`]), which `ui.set` already drives.

use super::UiCommand;

pub const COMMANDS: &[UiCommand] = &[];
