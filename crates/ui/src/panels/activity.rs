//! The activity popover under the toolbar's activity indicator (`ui-layout.md` §3 item 8): the
//! running and recent jobs, each a `progress_row` with its Cancel.
//!
//! **A stub until M5.19**, which fills this module and calls [`show`] from the toolbar (it owns
//! `panels/toolbar.rs`). The seams it keeps:
//!
//! - the popover is open while [`UiState::activity`](crate::state::UiState::activity) is set
//!   (`ui.set {activity: true}` opens it; a click on `indicator:activity` will toggle it);
//! - the rows come from [`Session::jobs`](dalica_engine::Session::jobs) (the same entries as
//!   `jobs.list` and `ui.inspect` → `jobs`), and Cancel runs what `jobs.cancel {job}` runs,
//!   [`Session::cancel_jobs`](dalica_engine::Session::cancel_jobs);
//! - it never blocks a frame: the registry is in memory.
//!
//! Until then it draws nothing.

use egui::{Rect, Ui};

use crate::app::App;

/// Draws the popover below `anchor` (the indicator's rect) while it is open. A stub: nothing yet.
pub fn show(app: &mut App, ui: &mut Ui, anchor: Rect) {
    let _ = (app.state().activity, ui, anchor);
}
