//! `dalica --view FILE` (`viewing.md` §3): the photo viewer's window and its session.
//!
//! - **The session:** the library opens as usual and the viewer is opened on FILE
//!   ([`dalica_engine::Session::open_viewer`]; the engine lists the folder on a worker). When
//!   another Dalica holds the library, the viewer opens on an empty read-only library of its own in
//!   a temporary folder ([`dalica_engine::Session::open_locked_viewer`]: changes and Open in
//!   Dalica disabled with "Dalica is already open"), removed once the app is done.
//! - **The window:** borderless and filling the screen, transparent where the platform allows
//!   (the viewer paints its own dark translucent backdrop on it) and opaque black otherwise, or
//!   when Options ▸ Photo Viewer ▸ "Translucent background" is off. A platform that cannot make the
//!   window transparent shows the transparent clear colour as black.
//! - **Closing:** the viewer's Esc and × close it in the engine ([`Closed::Quit`]) and the app
//!   quits; Open in Dalica ([`Closed::OpenInDalica`]) turns the window into the main window: the
//!   frame's decorations, its saved size and place, no longer full screen or transparent, on the
//!   edit view of that photo. [`Follow`] decides it, once, from what the engine says.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{Clock, PhotoId};
use dalica_engine::viewer::Closed;
use dalica_engine::{OpenError, Session};
use dalica_ui::titlebar::Chrome;
use egui::{ViewportBuilder, ViewportCommand};

use crate::frame::FrameSetup;

/// `--view FILE`: the file and, once the library turned out to be held by another Dalica, the
/// temporary library the viewer opened on instead.
#[derive(Clone)]
pub struct Launch {
    file: PathBuf,
    scratch: Arc<Mutex<Option<dalica_fs::TempDir>>>,
}

impl Launch {
    /// The viewer on `file`, made absolute against `cwd` (the file manager hands an absolute path;
    /// a terminal may not). Never read here.
    pub fn new(file: PathBuf, cwd: &Path) -> Launch {
        let file = if file.is_absolute() { file } else { cwd.join(file) };
        Launch { file, scratch: Arc::new(Mutex::new(None)) }
    }

    /// The file the viewer opens.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Opens the viewer on the session the library opened as (`opened`): the library's own, or,
    /// when another Dalica holds it, a read-only one on a temporary library. Off the UI thread: it
    /// may make that folder. Any other failure to open is the library's, as without `--view`.
    pub fn attach(&self, opened: Result<Session, OpenError>, clock: Arc<dyn Clock>) -> Result<Session, String> {
        match opened {
            Ok(mut session) => {
                if let Err(e) = session.open_viewer(&self.file) {
                    log::warn!("dalica: --view: {e}");
                }
                Ok(session)
            }
            Err(OpenError::Locked(dir)) => {
                log::info!("dalica: --view: the library in {} is open in another Dalica: the viewer is read-only", dir.display());
                let scratch = dalica_fs::TempDir::new("dalica-viewer").map_err(|e| format!("cannot make the viewer's temporary library: {e}"))?;
                let session = Session::open_locked_viewer(scratch.path(), &self.file, clock, &dalica_engine::Options::default())
                    .map_err(|e| format!("cannot open the viewer: {e}"))?;
                *self.scratch.lock().unwrap_or_else(PoisonError::into_inner) = Some(scratch);
                Ok(session)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Removes the temporary library, if one was made, once the session in it is gone. On its own
    /// thread: the caller may have been the UI thread, which the `fs` guard keeps off the disk.
    pub fn finish(self) {
        let Some(dir) = self.scratch.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
        let path = dir.path().to_path_buf();
        let removed = std::thread::Builder::new().name("dalica-cleanup".into()).spawn(move || dir.close()).map(|t| t.join());
        match removed {
            Ok(Ok(Ok(()))) => log::info!("dalica: removed the viewer's temporary library in {}", path.display()),
            _ => log::warn!("dalica: the viewer's temporary library in {} was not removed", path.display()),
        }
    }
}

/// The viewer's window: `base` (the main window's) without decorations, filling the screen,
/// transparent when `translucent`.
pub fn viewport(base: ViewportBuilder, translucent: bool) -> ViewportBuilder {
    base.with_decorations(false).with_fullscreen(true).with_maximized(false).with_transparent(translucent)
}

/// The colour the viewer's window is cleared to: fully transparent when `translucent` (black where
/// the platform cannot show through), else opaque black.
pub fn clear_color(translucent: bool) -> [f32; 4] {
    if translucent { [0.0, 0.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0, 1.0] }
}

/// What the window does about the viewer this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Esc or ×: quit.
    Quit,
    /// Open in Dalica: become the main window, on the edit view of the photo.
    BecomeMain(PhotoId),
}

/// Follows the engine's viewer for the window: acts once when it closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Follow {
    done: bool,
}

impl Follow {
    /// One frame: `closed` is the engine's [`dalica_engine::viewer::Viewer::closed`].
    pub fn step(&mut self, closed: Option<Closed>) -> Action {
        match (self.done, closed) {
            (true, _) | (false, None) => Action::Nothing,
            (false, Some(c)) => {
                self.done = true;
                match c {
                    Closed::Quit => Action::Quit,
                    Closed::OpenInDalica(id) => Action::BecomeMain(id),
                }
            }
        }
    }
}

/// The commands that turn the viewer's window into the main window: out of full screen, opaque,
/// the frame's decorations, its saved size and place (maximized if it was).
pub fn main_window(frame: &FrameSetup) -> Vec<ViewportCommand> {
    let w = frame.stored.unwrap_or_default().sanitized();
    let mut out = vec![
        ViewportCommand::Fullscreen(false),
        ViewportCommand::Transparent(false),
        ViewportCommand::Decorations(frame.chrome != Chrome::Drawn),
        ViewportCommand::InnerSize(egui::vec2(w.size[0], w.size[1])),
    ];
    if let Some([x, y]) = w.position {
        out.push(ViewportCommand::OuterPosition(egui::pos2(x, y)));
    }
    out.push(ViewportCommand::Maximized(w.maximized));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Os, chrome_for};
    use dalica_engine::settings::WindowState;

    #[test]
    fn the_viewer_window_is_borderless_full_screen_and_transparent_when_translucent() {
        let frame = FrameSetup { chrome: chrome_for(Os::Other, true), stored: None };
        for translucent in [true, false] {
            let v = viewport(frame.viewport(), translucent);
            assert_eq!(v.decorations, Some(false));
            assert_eq!(v.fullscreen, Some(true));
            assert_eq!(v.transparent, Some(translucent));
            assert_eq!(clear_color(translucent)[3], if translucent { 0.0 } else { 1.0 });
            assert_eq!(&clear_color(translucent)[..3], &[0.0; 3], "black where it does not show through");
        }
    }

    #[test]
    fn the_window_acts_once_when_the_viewer_closes() {
        let mut f = Follow::default();
        assert_eq!(f.step(None), Action::Nothing);
        assert_eq!(f.step(Some(Closed::Quit)), Action::Quit);
        assert_eq!(f.step(Some(Closed::Quit)), Action::Nothing, "once");
        let mut f = Follow::default();
        let id: PhotoId = serde_json::from_value(serde_json::json!(7)).unwrap();
        assert_eq!(f.step(Some(Closed::OpenInDalica(id))), Action::BecomeMain(id));
        assert_eq!(f.step(Some(Closed::OpenInDalica(id))), Action::Nothing);
    }

    #[test]
    fn open_in_dalica_restores_the_frame_and_its_saved_place() {
        let stored = WindowState { size: [1200.0, 800.0], position: Some([40.0, 30.0]), maximized: true };
        for (system, decorated) in [(true, true), (false, false)] {
            let frame = FrameSetup { chrome: chrome_for(Os::Other, system), stored: Some(stored) };
            let c = main_window(&frame);
            assert!(c.contains(&ViewportCommand::Fullscreen(false)));
            assert!(c.contains(&ViewportCommand::Transparent(false)));
            assert!(c.contains(&ViewportCommand::Decorations(decorated)));
            assert!(c.contains(&ViewportCommand::InnerSize(egui::vec2(1200.0, 800.0))));
            assert!(c.contains(&ViewportCommand::OuterPosition(egui::pos2(40.0, 30.0))));
            assert_eq!(c.last(), Some(&ViewportCommand::Maximized(true)));
        }
        // Nothing saved: the default size, placed by the OS.
        let c = main_window(&FrameSetup { chrome: Chrome::System, stored: None });
        assert!(!c.iter().any(|x| matches!(x, ViewportCommand::OuterPosition(_))));
    }

    #[test]
    fn a_relative_file_is_taken_from_the_working_folder() {
        let cwd = std::env::temp_dir();
        assert_eq!(Launch::new(PathBuf::from("a b.jpg"), &cwd).file(), cwd.join("a b.jpg"));
        let abs = cwd.join("x.jpg");
        assert_eq!(Launch::new(abs.clone(), Path::new("/elsewhere")).file(), abs);
    }
}
