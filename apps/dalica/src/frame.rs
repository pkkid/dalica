//! The window's frame and placement (`design-system.md` §11.1, `ui-layout.md` §2), decided once,
//! when the window is made: which [`Chrome`] it gets (from the host OS and the "Use system title
//! bar" setting) and where it opens (the position, size and maximized state stored in the
//! library's `settings.json`). While it is open, a [`Tracker`] follows its placement and says when
//! a settled change should be saved.
//!
//! Only this module asks which OS it runs on, and only to build the window: renders get their
//! chrome from [`dalica_ui::titlebar::set_chrome`], so headless renders stay the same everywhere.

use std::time::{Duration, Instant};

use dalica_engine::settings::{Settings, WINDOW_MIN, WindowState};
use dalica_ui::titlebar::{APP_NAME, Chrome};
use egui::{ViewportBuilder, ViewportInfo};

/// How long a new placement must hold before it is saved: a drag or a resize writes once, at its end.
pub const SETTLE: Duration = Duration::from_millis(500);

/// The OS family, as far as the window frame cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    /// Windows and Linux.
    Other,
}

impl Os {
    /// The OS this build runs on. Asked once, when the window is made.
    pub fn host() -> Os {
        if cfg!(target_os = "macos") { Os::Mac } else { Os::Other }
    }
}

/// The chrome of a window on `os`: the macOS strip, the OS decorations when the user asked for the
/// system title bar (Windows and Linux only), else Dalica's own title bar.
pub fn chrome_for(os: Os, system_title_bar: bool) -> Chrome {
    match (os, system_title_bar) {
        (Os::Mac, _) => Chrome::Mac,
        (Os::Other, true) => Chrome::System,
        (Os::Other, false) => Chrome::Drawn,
    }
}

/// What the window is made with: its chrome and its stored placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSetup {
    pub chrome: Chrome,
    pub stored: Option<WindowState>,
}

impl FrameSetup {
    /// From the settings of the library the window opens (read before it opens; see
    /// [`Settings::peek`]).
    pub fn from_settings(settings: &Settings, os: Os) -> FrameSetup {
        FrameSetup { chrome: chrome_for(os, settings.system_title_bar()), stored: settings.window() }
    }

    /// The window to build: "Dalica", at the stored size and position (else the default size,
    /// placed by the OS), maximized if it was; no OS decorations with [`Chrome::Drawn`], the
    /// content under a transparent title bar on macOS.
    pub fn viewport(&self) -> ViewportBuilder {
        let w = self.stored.unwrap_or_default().sanitized();
        let mut v =
            ViewportBuilder::default().with_title(APP_NAME).with_inner_size(w.size).with_min_inner_size(WINDOW_MIN).with_maximized(w.maximized);
        if let Some(p) = w.position {
            v = v.with_position(p);
        }
        match self.chrome {
            Chrome::Drawn => v.with_decorations(false),
            Chrome::System => v.with_decorations(true),
            Chrome::Mac => v.with_fullsize_content_view(true).with_title_shown(false).with_titlebar_shown(false),
        }
    }
}

/// Follows the window's placement and says when to save it.
#[derive(Debug, Clone)]
pub struct Tracker {
    /// The placement while neither maximized, minimized nor full screen (what a restore returns to).
    normal: WindowState,
    seen: Option<WindowState>,
    since: Option<Instant>,
    saved: Option<WindowState>,
}

impl Tracker {
    /// Starts from the stored placement (what is in `settings.json` now).
    pub fn new(stored: Option<WindowState>) -> Tracker {
        Tracker { normal: stored.unwrap_or_default().sanitized(), seen: None, since: None, saved: stored.map(WindowState::sanitized) }
    }

    /// Looks at the window at `now`; returns the placement to save once a change has held for
    /// [`SETTLE`]. A minimized or full-screen window changes nothing (its normal placement is kept);
    /// a maximized one keeps the normal size and position to restore to.
    pub fn observe(&mut self, info: &ViewportInfo, now: Instant) -> Option<WindowState> {
        // No size reported (not a frame of the window), minimized or full screen: nothing to learn.
        let inner = info.inner_rect.filter(|r| r.is_finite() && r.is_positive())?;
        if info.minimized == Some(true) || info.fullscreen == Some(true) {
            return None;
        }
        let maximized = info.maximized.unwrap_or(false);
        if !maximized {
            self.normal.size = [inner.width(), inner.height()];
            if let Some(outer) = info.outer_rect.filter(|r| r.is_finite()) {
                self.normal.position = Some([outer.min.x, outer.min.y]);
            }
        }
        let state = WindowState { maximized, ..self.normal }.sanitized();
        if self.seen != Some(state) {
            self.seen = Some(state);
            self.since = Some(now);
            return None;
        }
        let held = self.since.is_some_and(|t| now.saturating_duration_since(t) >= SETTLE);
        (held && self.saved != Some(state)).then_some(state)
    }

    /// The placement seen last, if it was not saved yet (for the last save, at exit).
    pub fn unsaved(&self) -> Option<WindowState> {
        self.seen.filter(|s| self.saved != Some(*s))
    }

    /// Notes that `state` was saved.
    pub fn saved(&mut self, state: WindowState) {
        self.saved = Some(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2, vec2};

    #[test]
    fn the_chrome_follows_the_os_and_the_setting() {
        assert_eq!(chrome_for(Os::Other, false), Chrome::Drawn);
        assert_eq!(chrome_for(Os::Other, true), Chrome::System);
        assert_eq!(chrome_for(Os::Mac, false), Chrome::Mac);
        assert_eq!(chrome_for(Os::Mac, true), Chrome::Mac, "the setting is for Windows and Linux");
        let mut s = Settings::default();
        assert_eq!(FrameSetup::from_settings(&s, Os::Other).chrome, Chrome::Drawn);
        s.set_system_title_bar(true);
        assert_eq!(FrameSetup::from_settings(&s, Os::Other).chrome, Chrome::System);
    }

    #[test]
    fn the_window_opens_where_it_was() {
        let v = FrameSetup { chrome: Chrome::Drawn, stored: None }.viewport();
        assert_eq!(
            (v.inner_size, v.min_inner_size, v.position, v.maximized),
            (Some(vec2(1400.0, 900.0)), Some(vec2(900.0, 600.0)), None, Some(false))
        );
        assert_eq!(v.decorations, Some(false));
        let stored = WindowState { position: Some([-1500.0, 30.0]), size: [1000.0, 700.0], maximized: true };
        let v = FrameSetup { chrome: Chrome::System, stored: Some(stored) }.viewport();
        assert_eq!(
            (v.inner_size, v.position, v.maximized, v.decorations),
            (Some(vec2(1000.0, 700.0)), Some(pos2(-1500.0, 30.0)), Some(true), Some(true))
        );
        let v = FrameSetup { chrome: Chrome::Mac, stored: None }.viewport();
        assert_eq!((v.fullsize_content_view, v.title_shown, v.titlebar_shown), (Some(true), Some(false), Some(false)));
        // A hostile stored size is clamped before it reaches the OS.
        let v = FrameSetup { chrome: Chrome::Drawn, stored: Some(WindowState { position: None, size: [1.0, 1e9], maximized: false }) }.viewport();
        assert_eq!(v.inner_size, Some(vec2(900.0, 16384.0)));
    }

    fn info(outer: Option<[f32; 4]>, inner: [f32; 4], maximized: bool) -> ViewportInfo {
        let rect = |r: [f32; 4]| Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]));
        ViewportInfo { outer_rect: outer.map(rect), inner_rect: Some(rect(inner)), maximized: Some(maximized), ..ViewportInfo::default() }
    }

    #[test]
    fn a_placement_is_saved_once_it_has_held() {
        let t0 = Instant::now();
        let mut t = Tracker::new(None);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let moved = info(Some([100.0, 50.0, 1200.0, 800.0]), [100.0, 50.0, 1200.0, 800.0], false);
        assert_eq!(t.observe(&moved, at(0)), None, "just moved");
        assert_eq!(t.observe(&moved, at(100)), None, "not settled yet");
        let want = WindowState { position: Some([100.0, 50.0]), size: [1200.0, 800.0], maximized: false };
        assert_eq!(t.observe(&moved, at(600)), Some(want));
        t.saved(want);
        assert_eq!(t.observe(&moved, at(2000)), None, "saved already");
        assert_eq!(t.unsaved(), None);
        // Maximized: the normal placement is kept for the restore.
        let max = info(Some([0.0, 0.0, 2560.0, 1400.0]), [0.0, 0.0, 2560.0, 1400.0], true);
        t.observe(&max, at(3000));
        assert_eq!(t.observe(&max, at(3600)), Some(WindowState { maximized: true, ..want }));
        assert_eq!(t.unsaved(), Some(WindowState { maximized: true, ..want }));
        // Minimized or full screen: nothing changes. No outer rect (Wayland): no position.
        let mut gone = max.clone();
        gone.minimized = Some(true);
        assert_eq!(t.observe(&gone, at(5000)), None);
        assert_eq!(t.observe(&ViewportInfo::default(), at(6000)), None, "no size: nothing seen");
        assert_eq!(Tracker::new(None).unsaved(), None, "nothing seen, nothing to save at exit");
        let mut t = Tracker::new(None);
        let wayland = info(None, [0.0, 0.0, 1000.0, 700.0], false);
        t.observe(&wayland, at(0));
        assert_eq!(t.observe(&wayland, at(600)), Some(WindowState { position: None, size: [1000.0, 700.0], maximized: false }));
    }

    #[test]
    fn the_stored_placement_is_not_saved_again() {
        let stored = WindowState { position: Some([10.0, 20.0]), size: [1000.0, 700.0], maximized: false };
        let mut t = Tracker::new(Some(stored));
        let same = info(Some([10.0, 20.0, 1000.0, 700.0]), [10.0, 20.0, 1000.0, 700.0], false);
        let t0 = Instant::now();
        t.observe(&same, t0);
        assert_eq!(t.observe(&same, t0 + Duration::from_secs(1)), None);
    }
}
