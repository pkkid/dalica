//! The Photo Tray (`ui-layout.md` §6.2): Hold (Ctrl+H) and Clear arrive here with the tray. They
//! change [`UiState::tray`](crate::state::UiState::tray): the held photos and the tray's own
//! selection.

use super::UiCommand;

/// None yet: the tray's task adds them.
pub const COMMANDS: &[UiCommand] = &[];
