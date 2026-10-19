//! The user's input, reported to the face scan (`faces.md` §4, *Pace follows the user*): each frame
//! [`App::frame`](crate::App::frame) reads the frame's input once, before the panels take the keys,
//! and reports one [`UserActivity`] to [`Session::user_activity`]:
//!
//! - `Input` for a key, typed text (or a paste), a wheel, a pinch, a touch, a pointer button down or
//!   a drag;
//! - `Pointer` for a pointer move alone;
//! - `Away` while the window is minimised, or with no input while the People view is shown (the
//!   Unnamed grid or a person's section in the library);
//! - `Idle` otherwise.
//!
//! A report goes when the activity changes, and at most once a second while it stays the same (the
//! session notes the time; the scan's pace reads it). The per-frame cost is one read of the input.
//! Input that automation injects (the control channel's clicks, keys and wheel) reaches the window
//! as input and counts as the user's: the window cannot tell them apart, and the scan only slows.

use std::time::Duration;

use dalica_core::Timestamp;
use dalica_engine::Session;
use dalica_engine::faces_scan::UserActivity;

use crate::state::{Room, UiState, View};

/// The longest an unchanged activity goes unreported.
pub const REPEAT: Duration = Duration::from_secs(1);

/// What the window last reported, and when (on the session's clock).
#[derive(Clone, Copy, Debug, Default)]
pub struct Reporter {
    last: Option<(UserActivity, Timestamp)>,
}

impl Reporter {
    /// Reads this frame's input in `ctx` and reports it to `session` when it changed or a second
    /// has passed since the last report.
    pub fn frame(&mut self, session: &mut Session, state: &UiState, ctx: &egui::Context) {
        let activity = ctx.input(|i| read(i, people_view(state)));
        let now = session.clock().now();
        let due = match self.last {
            Some((last, at)) => last != activity || now < at || now.since(at) >= REPEAT,
            None => true,
        };
        if due {
            self.last = Some((activity, now));
            session.user_activity(activity);
        }
    }

    /// The last report and when it went, if any.
    pub fn last(&self) -> Option<(UserActivity, Timestamp)> {
        self.last
    }
}

/// Whether the People view is shown: the Unnamed grid, or a person's section, in the library.
pub fn people_view(state: &UiState) -> bool {
    state.view == View::Library
        && state.room == Room::Library
        && (state.unnamed.open || state.selection.section.and_then(crate::state::Section::person).is_some())
}

/// What one frame's `input` says of the user (see the module's doc); `people_view` as
/// [`people_view`] gives it.
pub fn read(input: &egui::InputState, people_view: bool) -> UserActivity {
    use egui::Event;
    let typed = input.events.iter().any(|e| {
        matches!(
            e,
            Event::Key { .. }
                | Event::Text(_)
                | Event::Paste(_)
                | Event::Copy
                | Event::Cut
                | Event::MouseWheel { .. }
                | Event::Zoom(_)
                | Event::Rotate(_)
                | Event::Ime(_)
                | Event::Touch { .. }
                | Event::PointerButton { pressed: true, .. }
        )
    });
    if typed || input.pointer.any_down() {
        return UserActivity::Input;
    }
    if input.events.iter().any(|e| matches!(e, Event::PointerMoved(_) | Event::MouseMoved(_))) {
        return UserActivity::Pointer;
    }
    if input.viewport().minimized == Some(true) || people_view {
        return UserActivity::Away;
    }
    UserActivity::Idle
}
