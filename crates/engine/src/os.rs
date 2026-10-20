//! The operating system's own ways with files (`organizing.md` §9, `viewing.md` §3.3,
//! `security.md` §5.11; `roadmap.md` M15 → *OS integration*), behind a service trait as the native
//! dialogs are (`ui`'s `Picker`): reveal a file in the file manager with it selected, the
//! applications that open a file's type, open a file with one of them, open files in the external
//! editor, put files, a picture or text on the system clipboard, and the default handler of a file
//! type and setting Dalica as it.
//!
//! Every call is fallible with a reason ([`OsError`]): a feature the system offers no way to do
//! answers [`OsError::Unavailable`] with the system's reason, one that tried and failed
//! [`OsError::Failed`]. Calls that can block (starting a program, a D-Bus call, the clipboard) are
//! made off the UI thread by the commands that use them (M15.17).
//!
//! [`Os::available`] says, without blocking, whether a service can be used at all (the window greys
//! its menu rows with the reason; the commands' `enabled` asks it).
//!
//! [`Recording`] is the fake that headless sessions, `dalica-cli`, MCP and tests use: it keeps each
//! call in order and answers from what it was given. The desktop app plugs in the real service per
//! OS (M15.12) with [`Session::set_os`]; a session starts with [`Recording::new`], a headless one:
//! there is no system clipboard without a window ([`NO_CLIPBOARD`]). Tests that stand for a window
//! plug in [`Recording::windowed`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use dalica_scan::FileType;

use crate::session::Session;

/// Why an OS call did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsError {
    /// The system offers no way to do it (no window, no file manager that selects, …): the reason
    /// the window shows beside the greyed item.
    Unavailable(String),
    /// It was tried and failed: the reason.
    Failed(String),
}

impl std::fmt::Display for OsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsError::Unavailable(why) | OsError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for OsError {}

/// Why a headless session (no window: `dalica-cli`, MCP, tests) cannot use the system clipboard.
pub const NO_CLIPBOARD: &str = "There is no system clipboard without Dalica's window.";

/// The services [`Os::available`] answers for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Service {
    /// [`Os::reveal`]: Locate on Disk.
    Reveal,
    /// [`Os::handlers`] and [`Os::open_with`]: Open With.
    OpenWith,
    /// [`Os::open_in_editor`]: Open in External Editor.
    Editor,
    /// [`Os::put_on_clipboard`]: Copy Path, the files and the picture.
    Clipboard,
    /// [`Os::default_handler`] and [`Os::set_default_viewer`].
    DefaultViewer,
}

/// An application that opens a file's type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    /// How the system names it (a desktop entry's id, a ProgID, a bundle id): what
    /// [`Os::open_with`] takes back.
    pub id: String,
    /// Its name as the system shows it ("Image Viewer").
    pub name: String,
}

/// What [`Os::put_on_clipboard`] puts on the system clipboard.
#[derive(Clone, Debug, PartialEq)]
pub enum Clip {
    /// Files, so a paste in a file manager copies them.
    Files(Vec<PathBuf>),
    /// A picture: `width` × `height` pixels of 8-bit RGBA, row by row.
    Picture { width: u32, height: u32, rgba: Vec<u8> },
    /// Text (Copy Path's lines).
    Text(String),
}

/// What setting the default viewer did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultSet {
    /// The defaults are set (Linux's `mimeapps.list`).
    Set,
    /// Only the user may choose, in the system's settings, which were opened: what to choose there.
    SettingsOpened(String),
}

/// The operating system's services. Every method may block; none may panic.
pub trait Os: Send + Sync {
    /// Whether `service` can be used here at all, answered at once from what the service knows
    /// (never blocking: the window asks it for every menu it draws): `Err` with the reason the
    /// greyed item shows. Every service is available unless the implementation says otherwise.
    fn available(&self, service: Service) -> Result<(), OsError> {
        let _ = service;
        Ok(())
    }

    /// Shows `path` in the system's file manager, selected (a folder: its parent opened on it).
    fn reveal(&self, path: &std::path::Path) -> Result<(), OsError>;

    /// The applications that open `path`'s type, the system's default first.
    fn handlers(&self, path: &std::path::Path) -> Result<Vec<Handler>, OsError>;

    /// Opens `path` with the application `handler` names ([`Handler::id`]).
    fn open_with(&self, path: &std::path::Path, handler: &str) -> Result<(), OsError>;

    /// Opens `paths` in the external editor: `editor` (a program's name or path, from
    /// `files.externalEditor`), or the system's edit verb or default application when empty.
    fn open_in_editor(&self, paths: &[PathBuf], editor: &str) -> Result<(), OsError>;

    /// Puts `clip` on the system clipboard.
    fn put_on_clipboard(&self, clip: &Clip) -> Result<(), OsError>;

    /// The application that opens `ty` by default, if the system says.
    fn default_handler(&self, ty: FileType) -> Result<Option<Handler>, OsError>;

    /// Makes Dalica the default viewer for `types` and gives every other type Dalica holds back to
    /// the handler it replaced (none: "Don't use Dalica as a viewer").
    fn set_default_viewer(&self, types: &[FileType]) -> Result<DefaultSet, OsError>;
}

/// One call [`Recording`] kept.
#[derive(Clone, Debug, PartialEq)]
pub enum OsCall {
    Reveal(PathBuf),
    Handlers(PathBuf),
    OpenWith { path: PathBuf, handler: String },
    OpenInEditor { paths: Vec<PathBuf>, editor: String },
    PutOnClipboard(Clip),
    DefaultHandler(FileType),
    SetDefaultViewer(Vec<FileType>),
}

impl OsCall {
    /// The service the call uses.
    pub fn service(&self) -> Service {
        match self {
            OsCall::Reveal(_) => Service::Reveal,
            OsCall::Handlers(_) | OsCall::OpenWith { .. } => Service::OpenWith,
            OsCall::OpenInEditor { .. } => Service::Editor,
            OsCall::PutOnClipboard(_) => Service::Clipboard,
            OsCall::DefaultHandler(_) | OsCall::SetDefaultViewer(_) => Service::DefaultViewer,
        }
    }
}

#[derive(Debug, Default)]
struct Recorded {
    calls: Vec<OsCall>,
    /// Handlers by lowercased file extension (without the dot).
    handlers: BTreeMap<String, Vec<Handler>>,
    defaults: BTreeMap<FileType, Handler>,
    failure: Option<OsError>,
    /// Services answered as unavailable, with the reason.
    unavailable: BTreeMap<Service, String>,
}

/// The fake [`Os`]: keeps each call in order ([`Recording::calls`]) and answers from what it was
/// given: no handlers and no defaults until [`Recording::set_handlers`] and
/// [`Recording::set_default`], every call `Ok` until [`Recording::fail_with`], every service
/// available until [`Recording::set_unavailable`] (a call to an unavailable service answers its
/// reason). A call that fails is still kept.
#[derive(Debug, Default)]
pub struct Recording {
    inner: Mutex<Recorded>,
}

impl Recording {
    /// A headless session's: the clipboard is unavailable ([`NO_CLIPBOARD`]), the rest available.
    pub fn new() -> Recording {
        let r = Recording::default();
        r.set_unavailable(Service::Clipboard, Some(NO_CLIPBOARD));
        r
    }

    /// One standing for a window's system: every service available.
    pub fn windowed() -> Recording {
        Recording::default()
    }

    /// Makes `service` unavailable with `reason` (`None`: available again).
    pub fn set_unavailable(&self, service: Service, reason: Option<&str>) {
        self.with(|r| match reason {
            Some(why) => r.unavailable.insert(service, why.to_string()),
            None => r.unavailable.remove(&service),
        });
    }

    fn with<R>(&self, f: impl FnOnce(&mut Recorded) -> R) -> R {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Keeps `call` and answers its service's unavailability, else the failure set, if any.
    fn keep(&self, call: OsCall) -> Result<(), OsError> {
        let service = call.service();
        self.with(|r| {
            r.calls.push(call);
            if let Some(why) = r.unavailable.get(&service) {
                return Err(OsError::Unavailable(why.clone()));
            }
            r.failure.clone().map_or(Ok(()), Err)
        })
    }

    /// The calls made so far, oldest first.
    pub fn calls(&self) -> Vec<OsCall> {
        self.with(|r| r.calls.clone())
    }

    /// Forgets the calls made so far.
    pub fn clear(&self) {
        self.with(|r| r.calls.clear());
    }

    /// The handlers [`Os::handlers`] answers for files ending in `.<extension>` (any case).
    pub fn set_handlers(&self, extension: &str, handlers: Vec<Handler>) {
        self.with(|r| r.handlers.insert(extension.to_ascii_lowercase(), handlers));
    }

    /// The handler [`Os::default_handler`] answers for `ty`.
    pub fn set_default(&self, ty: FileType, handler: Handler) {
        self.with(|r| r.defaults.insert(ty, handler));
    }

    /// Every call from now on fails with `error` (`None`: they succeed again).
    pub fn fail_with(&self, error: Option<OsError>) {
        self.with(|r| r.failure = error);
    }
}

impl Os for Recording {
    fn available(&self, service: Service) -> Result<(), OsError> {
        self.with(|r| r.unavailable.get(&service).map_or(Ok(()), |why| Err(OsError::Unavailable(why.clone()))))
    }

    fn reveal(&self, path: &std::path::Path) -> Result<(), OsError> {
        self.keep(OsCall::Reveal(path.to_path_buf()))
    }

    fn handlers(&self, path: &std::path::Path) -> Result<Vec<Handler>, OsError> {
        self.keep(OsCall::Handlers(path.to_path_buf()))?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        Ok(self.with(|r| r.handlers.get(&ext).cloned().unwrap_or_default()))
    }

    fn open_with(&self, path: &std::path::Path, handler: &str) -> Result<(), OsError> {
        self.keep(OsCall::OpenWith { path: path.to_path_buf(), handler: handler.to_string() })
    }

    fn open_in_editor(&self, paths: &[PathBuf], editor: &str) -> Result<(), OsError> {
        self.keep(OsCall::OpenInEditor { paths: paths.to_vec(), editor: editor.to_string() })
    }

    fn put_on_clipboard(&self, clip: &Clip) -> Result<(), OsError> {
        self.keep(OsCall::PutOnClipboard(clip.clone()))
    }

    fn default_handler(&self, ty: FileType) -> Result<Option<Handler>, OsError> {
        self.keep(OsCall::DefaultHandler(ty))?;
        Ok(self.with(|r| r.defaults.get(&ty).cloned()))
    }

    fn set_default_viewer(&self, types: &[FileType]) -> Result<DefaultSet, OsError> {
        self.keep(OsCall::SetDefaultViewer(types.to_vec()))?;
        Ok(DefaultSet::Set)
    }
}

impl Session {
    /// The OS service in use: a [`Recording`] unless the app plugged in the real one.
    pub fn os(&self) -> Arc<dyn Os> {
        Arc::clone(&self.os)
    }

    /// Plugs in the OS service (the desktop app's real one, or a test's [`Recording`]).
    pub fn set_os(&mut self, os: Arc<dyn Os>) {
        self.os = os;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test of the fake: every call kept in order, failures included, and its answers.
    #[test]
    fn the_recording_keeps_each_call_in_order() {
        let os = Recording::windowed();
        let (a, b) = (PathBuf::from("/p/a.JPG"), PathBuf::from("/p/b.png"));
        let viewer = Handler { id: "org.example.Viewer.desktop".into(), name: "Image Viewer".into() };
        os.set_handlers("jpg", vec![viewer.clone()]);
        os.set_default(FileType::Png, viewer.clone());
        os.reveal(&a).unwrap();
        assert_eq!(os.handlers(&a).unwrap(), std::slice::from_ref(&viewer), "by extension, any case");
        assert_eq!(os.handlers(&b).unwrap(), []);
        os.open_with(&a, &viewer.id).unwrap();
        os.open_in_editor(&[a.clone(), b.clone()], "").unwrap();
        os.put_on_clipboard(&Clip::Text("/p/a.JPG\n/p/b.png".into())).unwrap();
        assert_eq!(os.default_handler(FileType::Png).unwrap(), Some(viewer.clone()));
        assert_eq!(os.set_default_viewer(&[FileType::Jpeg, FileType::Png]).unwrap(), DefaultSet::Set);
        os.fail_with(Some(OsError::Unavailable("No file manager".into())));
        assert_eq!(os.reveal(&b), Err(OsError::Unavailable("No file manager".into())));
        assert_eq!(
            os.calls(),
            [
                OsCall::Reveal(a.clone()),
                OsCall::Handlers(a.clone()),
                OsCall::Handlers(b.clone()),
                OsCall::OpenWith { path: a.clone(), handler: viewer.id.clone() },
                OsCall::OpenInEditor { paths: vec![a.clone(), b.clone()], editor: String::new() },
                OsCall::PutOnClipboard(Clip::Text("/p/a.JPG\n/p/b.png".into())),
                OsCall::DefaultHandler(FileType::Png),
                OsCall::SetDefaultViewer(vec![FileType::Jpeg, FileType::Png]),
                OsCall::Reveal(b.clone()),
            ]
        );
        os.clear();
        assert!(os.calls().is_empty());
    }

    /// A headless session's fake has no clipboard: asked, and called (the call kept), it answers
    /// why; a service made unavailable answers its reason the same way, and available again after.
    #[test]
    fn a_headless_recording_has_no_clipboard() {
        let os = Recording::new();
        let unavailable = Err(OsError::Unavailable(NO_CLIPBOARD.into()));
        assert_eq!(os.available(Service::Clipboard), unavailable);
        assert_eq!(os.put_on_clipboard(&Clip::Text("a".into())), unavailable);
        assert_eq!(os.available(Service::Reveal), Ok(()));
        os.set_unavailable(Service::Reveal, Some("No file manager"));
        assert_eq!(os.reveal(std::path::Path::new("/p/a.jpg")), Err(OsError::Unavailable("No file manager".into())));
        os.set_unavailable(Service::Reveal, None);
        assert_eq!(os.available(Service::Reveal), Ok(()));
        assert_eq!(os.calls().len(), 2);
        assert_eq!(Recording::windowed().available(Service::Clipboard), Ok(()));
    }
}
