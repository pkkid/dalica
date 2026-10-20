//! The running Dalica's side of the hand-off (`viewing.md` §3.4): files a later launch handed
//! over (and, on macOS, the open-documents events of Finder and the Dock) open in its photo viewer,
//! and Esc returns the window to where it was.
//!
//! Every frame of the window (and every tick of `--headless`) calls [`Receiver::step`] once the
//! library is open (a launch whose files arrive earlier waits, and is told so only once they are
//! taken):
//! - **A launch's files:** the window comes forward (out of the Dock or taskbar), and the viewer
//!   opens on the first file ([`dalica_engine::Session::hand_viewer`]), borderless and full screen
//!   as `dalica --view` would show it (the window keeps its opaque surface: a window made opaque
//!   cannot turn transparent). A launch without files only brings the window forward.
//! - **Esc or ×** on a viewer handed in closes it with [`Closed::Back`]: the window gets back its
//!   view (the library, an edit, …), its full screen, frame and maximized state as they were.
//!   Open in Dalica leaves it on the edit view of that photo, with the window's frame back.
//! - A file handed to the viewer of `dalica --view`'s own window replaces the file shown; Esc
//!   there still quits.

use std::path::PathBuf;

use dalica_engine::viewer::Closed;
use dalica_ui::App;
use dalica_ui::panels::viewer::listing::{self, Fetched};
use dalica_ui::state::{View, ViewerState};
use egui::ViewportCommand;

use super::Handoffs;

/// What the window looks like now, for a hand-off to remember.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowNow {
    /// The OS window is maximized.
    pub maximized: bool,
    /// The window is `dalica --view`'s viewer window (not yet the main window).
    pub viewer_window: bool,
}

/// Where the window was when a viewer was handed in: where Esc returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Before {
    view: View,
    full_screen: bool,
    maximized: bool,
}

/// Takes handed-in files to the viewer, and returns the window to where it was afterwards.
pub struct Receiver {
    handoffs: Option<Handoffs>,
    #[cfg(target_os = "macos")]
    apple: Option<super::apple::AppleEvents>,
    /// The main window has the OS's frame (taken off for the viewer, put back after).
    decorated: bool,
    before: Option<Before>,
    /// The file whose listing puts the viewer on its place once it arrives.
    placing: Option<PathBuf>,
}

impl Receiver {
    /// Files from `handoffs` (the [`super::Primary`]'s); `decorated`: the main window has the OS's
    /// frame.
    pub fn new(handoffs: Option<Handoffs>, decorated: bool) -> Receiver {
        Receiver {
            handoffs,
            #[cfg(target_os = "macos")]
            apple: None,
            decorated,
            before: None,
            placing: None,
        }
    }

    /// And the files of macOS's open-documents events (and its quit event).
    #[cfg(target_os = "macos")]
    pub fn with_apple(mut self, apple: super::apple::AppleEvents) -> Receiver {
        self.apple = Some(apple);
        self
    }

    /// One frame (`window`: `None` headless): takes the files handed in once the library is open,
    /// follows the viewer they opened, and returns the commands for the OS window.
    pub fn step(&mut self, app: &mut App, window: Option<WindowNow>) -> Vec<ViewportCommand> {
        let mut out = Vec::new();
        if app.session().is_some() {
            // macOS adds its open-documents events.
            #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
            let mut batches = self.handoffs.as_ref().map(Handoffs::take).unwrap_or_default();
            #[cfg(target_os = "macos")]
            if let Some(apple) = &self.apple {
                let (opened, quit) = apple.drain();
                batches.extend(opened);
                if quit {
                    app.request_quit();
                }
            }
            for paths in batches {
                self.hand_in(app, &paths, window, &mut out);
            }
        }
        self.place(app);
        self.follow(app, window.is_some(), &mut out);
        out
    }

    /// One launch's files: the window forward and the viewer on the first file.
    fn hand_in(&mut self, app: &mut App, paths: &[PathBuf], window: Option<WindowNow>, out: &mut Vec<ViewportCommand>) {
        if window.is_some() {
            out.push(ViewportCommand::Minimized(false));
            out.push(ViewportCommand::Focus);
        }
        let Some(file) = paths.first() else { return };
        let Some(session) = app.session_mut() else { return };
        // A file name can be private: it goes to the log at debug level only.
        log::debug!("dalica: a later launch handed over {}", file.display());
        if let Err(e) = session.hand_viewer(file) {
            log::warn!("dalica: a handed-over file: {e}");
            return;
        }
        let handed = session.viewer().handed();
        let state = app.state_mut();
        if handed && self.before.is_none() && !window.is_some_and(|w| w.viewer_window) {
            self.before = Some(Before { view: state.view, full_screen: state.full_screen, maximized: window.is_some_and(|w| w.maximized) });
            // The window's full screen follows the view state's (`window::FullscreenSync`).
            state.full_screen = true;
            if window.is_some() && self.decorated {
                out.push(ViewportCommand::Decorations(false));
            }
        }
        state.view = View::Viewer;
        state.viewer = Some(ViewerState::default());
        self.placing = Some(file.clone());
    }

    /// Puts the viewer on the handed file's place once its folder is listed.
    fn place(&mut self, app: &mut App) {
        let Some(file) = &self.placing else { return };
        if app.state().view != View::Viewer {
            self.placing = None;
            return;
        }
        match listing::fetch(app) {
            Fetched::Ready(l) if &l.file == file => {
                if let Some(v) = app.state_mut().viewer.as_mut() {
                    v.index = l.index.unwrap_or(0);
                }
                self.placing = None;
            }
            Fetched::Ready(_) | Fetched::Failed(_) => self.placing = None,
            Fetched::Pending => {}
        }
    }

    /// Once a handed-in viewer closes, the window goes back to where it was.
    fn follow(&mut self, app: &mut App, windowed: bool, out: &mut Vec<ViewportCommand>) {
        let Some(before) = self.before else { return };
        let Some(closed) = app.session().and_then(|s| s.viewer().closed()) else { return };
        self.before = None;
        self.placing = None;
        let state = app.state_mut();
        state.full_screen = before.full_screen;
        if closed == Closed::Back {
            state.viewer = None;
            // A slideshow the hand-off interrupted has ended: back to the library.
            state.view = match before.view {
                View::Viewer | View::Slideshow => View::Library,
                v => v,
            };
        }
        if windowed {
            if self.decorated {
                out.push(ViewportCommand::Decorations(true));
            }
            if before.maximized {
                out.push(ViewportCommand::Maximized(true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{Claim, TIMING, claim_with};
    use dalica_core::SystemClock;
    use dalica_engine::Session;
    use serde_json::json;
    use std::sync::Arc;

    fn app(dir: &std::path::Path) -> App {
        App::new(Some(Session::open(&dir.join("library"), Arc::new(SystemClock)).unwrap()))
    }

    /// Two small photos in `dir/Photos`; the second's path.
    fn photos(dir: &std::path::Path) -> PathBuf {
        let folder = dir.join("Photos");
        std::fs::create_dir_all(&folder).unwrap();
        for (i, name) in ["a.jpg", "b.jpg"].iter().enumerate() {
            let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 48, 32), 80, None).unwrap();
            std::fs::write(folder.join(name), jpeg).unwrap();
        }
        folder.join("b.jpg")
    }

    /// A receiver whose files come from a real first Dalica in `dir`, and the launch that hands
    /// `paths` to it (on its own thread: it waits until they are taken).
    fn handing(dir: &std::path::Path, paths: Vec<PathBuf>) -> (crate::instance::Primary, Receiver, std::thread::JoinHandle<bool>) {
        let settings = dir.join("settings");
        let Claim::Primary(primary) = claim_with(&settings, &[], TIMING) else { panic!("the first launch is the primary") };
        let receiver = Receiver::new(Some(primary.connect(None)), true);
        let launch = std::thread::spawn(move || matches!(claim_with(&settings, &paths, TIMING), Claim::Forwarded));
        (primary, receiver, launch)
    }

    /// Steps until `done` (or 30 s).
    fn until(r: &mut Receiver, app: &mut App, w: Option<WindowNow>, done: impl Fn(&App) -> bool) -> Vec<ViewportCommand> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut out = Vec::new();
        while !done(app) {
            assert!(std::time::Instant::now() < end, "timed out");
            out.extend(r.step(app, w));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        out
    }

    /// The library window, maximized with the OS's frame on the edit view: a file handed in opens
    /// the viewer on it, borderless and full screen, and Esc leaves the window where it was.
    #[test]
    fn esc_after_a_hand_off_leaves_the_library_window_where_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let file = photos(dir.path());
        let mut app = app(dir.path());
        app.state_mut().view = View::Edit;
        let before = app.state().clone();
        let (primary, mut r, launch) = handing(dir.path(), vec![file.clone()]);
        let now = WindowNow { maximized: true, viewer_window: false };
        let out = until(&mut r, &mut app, Some(now), |a| a.state().view == View::Viewer);
        assert!(launch.join().unwrap(), "the later launch was told its file was taken");
        assert_eq!(out, [ViewportCommand::Minimized(false), ViewportCommand::Focus, ViewportCommand::Decorations(false)]);
        assert!(app.state().full_screen, "full screen, as `dalica --view`");
        assert_eq!(app.session().unwrap().viewer().file(), Some(file.as_path()));
        // On the handed file's place once its folder is listed (b.jpg is the second photo).
        until(&mut r, &mut app, Some(now), |a| a.state().viewer.is_some_and(|v| v.index == 1));
        // Esc.
        app.run("viewer.close", &json!({})).unwrap();
        let out = r.step(&mut app, Some(now));
        assert_eq!(out, [ViewportCommand::Decorations(true), ViewportCommand::Maximized(true)]);
        assert_eq!(app.session().unwrap().viewer().closed(), Some(Closed::Back));
        assert_eq!(app.state(), &before, "the view state is where it was");
        assert!(!app.quit_requested(), "Esc did not quit");
        assert!(r.step(&mut app, Some(now)).is_empty(), "once");
        drop(primary);
    }

    /// Headless: no window commands; a launch without files takes nothing to the viewer; the
    /// same file handed twice is placed again.
    #[test]
    fn headless_a_launch_without_files_opens_nothing_and_a_file_handed_twice_is_placed_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = photos(dir.path());
        let mut app = app(dir.path());
        let (primary, mut r, launch) = handing(dir.path(), vec![]);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !launch.is_finished() {
            assert!(std::time::Instant::now() < end);
            assert!(r.step(&mut app, None).is_empty());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(launch.join().unwrap());
        assert_eq!(app.state().view, View::Library);
        assert!(!app.session().unwrap().viewer().is_open());
        for _ in 0..2 {
            let settings = dir.path().join("settings");
            let paths = vec![file.clone()];
            let launch = std::thread::spawn(move || matches!(claim_with(&settings, &paths, TIMING), Claim::Forwarded));
            assert!(until(&mut r, &mut app, None, |a| a.state().view == View::Viewer).is_empty());
            assert!(launch.join().unwrap());
            until(&mut r, &mut app, None, |a| a.state().viewer.is_some_and(|v| v.index == 1));
            app.run("viewer.close", &json!({})).unwrap();
            assert!(r.step(&mut app, None).is_empty());
            assert_eq!(app.state().view, View::Library);
            assert!(!app.state().full_screen);
        }
        drop(primary);
    }

    /// `dalica --view`'s own window: a file handed in shows there, and Esc still quits.
    #[test]
    fn a_file_handed_to_the_view_window_replaces_the_one_shown() {
        let dir = tempfile::tempdir().unwrap();
        let file = photos(dir.path());
        let mut app = app(dir.path());
        app.session_mut().unwrap().open_viewer(&file.with_file_name("a.jpg")).unwrap();
        app.state_mut().view = View::Viewer;
        app.state_mut().viewer = Some(ViewerState::default());
        let (primary, mut r, launch) = handing(dir.path(), vec![file.clone()]);
        let now = WindowNow { maximized: false, viewer_window: true };
        let out = until(&mut r, &mut app, Some(now), |a| a.session().unwrap().viewer().file() == Some(file.as_path()));
        assert!(launch.join().unwrap());
        assert_eq!(out, [ViewportCommand::Minimized(false), ViewportCommand::Focus]);
        app.run("viewer.close", &json!({})).unwrap();
        assert!(r.step(&mut app, Some(now)).is_empty());
        assert_eq!(app.session().unwrap().viewer().closed(), Some(Closed::Quit));
        drop(primary);
    }
}
