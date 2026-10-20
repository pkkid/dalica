//! macOS's open-documents and quit Apple events (`viewing.md` §3.4).
//!
//! Adapted from PhotoCraft `apps/photocraft/src/apple_events.rs` @ 297f7f85 (MIT OR Apache-2.0).
//!
//! Finder's Open With, a double-click, a file dropped on the Dock icon and `open -a Dalica file`
//! pass no paths on the command line: Launch Services sends the running (or just started) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit owns the application delegate and does not
//! handle it, and handling it here needs Objective-C class declarations, i.e. `unsafe`, which this
//! workspace forbids. The audited `fmv-macos-events` crate (pinned) wraps exactly that: an
//! `NSAppleEventManager` handler registered before Finder's launch event, leaving winit's delegate
//! alone, behind a safe main-thread API. Registering the open handler replaces AppKit's own quit
//! handler too, so the quit event is passed on to the app ([`dalica_ui::App::request_quit`]).

use std::path::PathBuf;

use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the handlers registered; hold it until the event loop returns (main thread only).
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Registers the handlers. On the main thread, before the event loop starts, so the event
    /// that started the app is caught too.
    pub fn install() -> AppleEvents {
        let (registration, inbox) = Registration::install();
        AppleEvents { _registration: registration, inbox }
    }

    /// Events arriving later call `wake` (the window's repaint).
    pub fn set_wake(&self, wake: impl Fn() + 'static) {
        self.inbox.set_wake(wake);
    }

    /// The files of each open-documents event since the last call, and whether a quit event came.
    pub fn drain(&self) -> (Vec<Vec<PathBuf>>, bool) {
        let mut opened = Vec::new();
        let mut quit = false;
        for e in self.inbox.drain() {
            match e {
                Event::Open(paths) => opened.push(paths.into_iter().take(super::MAX_PATHS).collect()),
                Event::Quit => quit = true,
            }
        }
        (opened, quit)
    }
}
