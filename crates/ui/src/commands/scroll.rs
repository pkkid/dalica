//! Scrolling the light table (`ui-layout.md` §5.6): the scroll bar's keys (PgUp / PgDn, Ctrl+↑ /
//! Ctrl+↓ to the previous / next section, Ctrl+Home / Ctrl+End) arrive here with the speed-control
//! scroll bar. They move [`UiState::scroll`](crate::state::UiState::scroll), the anchor the light
//! table's viewport follows.

use super::UiCommand;

/// None yet: the scroll bar's task adds them.
pub const COMMANDS: &[UiCommand] = &[];
