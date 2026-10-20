//! Photos dragged out of the window to other applications (M15.23, `FILE-DRAGOUT`,
//! `ui-layout.md` §5.5): the app's [`DragOut`] service, which `dalica_ui::drag::out` hands a drag
//! of files the moment its pointer leaves the window, and the [`Host`] round the window that gives
//! it what only eframe's frame has (the window's handle, the end of a drag the OS kept to itself).
//! No `unsafe` here.
//!
//! - **X11** (`x11`): Dalica speaks XDND itself, as the drag's source, over x11rb's pure-Rust
//!   connection on a thread of its own: it follows the pointer, offers `text/uri-list` to the
//!   XDND-aware window under it (never to Dalica's own), drops on release, serves the files, and
//!   shows the pill beside the pointer in a small window. winit keeps the button's grab, so the
//!   window sees the release too and drops nothing inside when it is outside.
//! - **Windows and macOS** (`os_drag`): the `drag` crate, from the window's frame right after the
//!   frame that asked: OLE's `DoDragDrop` (modal: the frame waits until the drop) and an AppKit
//!   dragging session, the pill as the drag's image. The OS keeps the button's release to itself,
//!   so the host gives the window one once the drag has ended: where the OS drag ended, back inside
//!   the window on Windows (an inside drop there), else outside it.
//! - **Wayland**: a drag must be started on winit's own Wayland connection with the serial of the
//!   button press, which winit hands out only as raw pointers (`unsafe` to use); there is no safe
//!   way, so the drag stays inside and the status line points to Copy, which puts the files on the
//!   clipboard (`os::clipboard`).
//!
//! What leaves is the files on disk, as a copy (`dalica_ui::drag::out`). Files of the drag that
//! come back to Dalica's own window as a drop of files are not taken as one.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use dalica_ui::drag::out::{DragOut, Outgoing};

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod os_drag;
#[cfg(target_os = "linux")]
pub mod x11;

/// What the status line says where the OS has no safe way to drag files out.
pub const WAYLAND_REFUSAL: &str = "Photos can't be dragged out of Dalica on Wayland yet; Copy (Ctrl+C) puts their files on the clipboard.";
/// What the status line says before the window is known (the first frame).
const NOT_READY: &str = "Photos can't be dragged out of Dalica yet: the window is still opening.";

/// The kind of window Dalica has, from its handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// No frame yet.
    Unknown,
    /// An X11 window, with its id.
    X11(u32),
    Wayland,
    Windows,
    MacOs,
    /// Anything else: no drag out.
    Other,
}

impl Backend {
    /// The backend a window handle names.
    pub fn of(handle: &wgpu::rwh::RawWindowHandle) -> Backend {
        use wgpu::rwh::RawWindowHandle as H;
        match handle {
            H::Xlib(h) => x_id(h.window).map_or(Backend::Other, Backend::X11),
            H::Xcb(h) => Backend::X11(h.window.get()),
            H::Wayland(_) => Backend::Wayland,
            H::Win32(_) => Backend::Windows,
            H::AppKit(_) => Backend::MacOs,
            _ => Backend::Other,
        }
    }
}

/// An Xlib window id (a `c_ulong`, 32 or 64 bits by OS) as X11's 32 bits.
fn x_id<T: TryInto<u32>>(id: T) -> Option<u32> {
    id.try_into().ok()
}

/// A release the window never saw (the OS kept it during its drag): where to give it, in points
/// in the window, or `None` for outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Release {
    pub inside: Option<egui::Pos2>,
}

#[derive(Default)]
struct Inner {
    backend: Option<Backend>,
    /// A drag asked for, for the frame to start (Windows, macOS).
    pending: Option<Outgoing>,
    /// The files of the last drag handed out: a drop of them on Dalica's window is not taken.
    last: Vec<PathBuf>,
    /// A release to give the window.
    release: Option<Release>,
    #[cfg(target_os = "linux")]
    x11: Option<x11::Handle>,
    /// The window's context, to wake it when an OS drag ends.
    ctx: Option<egui::Context>,
}

/// The app's drag-out service: one per window.
#[derive(Default)]
pub struct Native {
    inner: Mutex<Inner>,
}

impl Native {
    pub fn new() -> Arc<Native> {
        Arc::new(Native::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The window's kind, once its frame is known.
    pub fn set_backend(&self, backend: Backend, ctx: &egui::Context) {
        let mut inner = self.lock();
        inner.backend = Some(backend);
        inner.ctx = Some(ctx.clone());
    }

    pub fn backend(&self) -> Backend {
        self.lock().backend.unwrap_or(Backend::Unknown)
    }

    /// The drag the frame is to start, if one was asked for.
    #[cfg(any(target_os = "windows", target_os = "macos", test))]
    pub fn take_pending(&self) -> Option<Outgoing> {
        self.lock().pending.take()
    }

    /// The OS's drag ended without the window seeing the release: it gets `release`.
    #[cfg(any(target_os = "windows", target_os = "macos", test))]
    pub fn ended(&self, release: Release) {
        let ctx = {
            let mut inner = self.lock();
            inner.release = Some(release);
            inner.ctx.clone()
        };
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }

    /// The release to give the window, once.
    pub fn take_release(&self) -> Option<Release> {
        self.lock().release.take()
    }

    /// Whether `path` is one of the last drag's files.
    pub fn is_outgoing(&self, path: &std::path::Path) -> bool {
        self.lock().last.iter().any(|p| p == path)
    }
}

impl DragOut for Native {
    fn start(&self, out: Outgoing) -> Result<(), String> {
        let backend = self.backend();
        let mut inner = self.lock();
        inner.last = out.paths.clone();
        match backend {
            #[cfg(target_os = "linux")]
            Backend::X11(own) => {
                if let Some(old) = inner.x11.take() {
                    old.cancel();
                }
                inner.x11 = Some(x11::start(out, own));
                Ok(())
            }
            Backend::Windows | Backend::MacOs if cfg!(any(target_os = "windows", target_os = "macos")) => {
                inner.pending = Some(out);
                Ok(())
            }
            Backend::Wayland => Err(WAYLAND_REFUSAL.into()),
            Backend::Unknown => Err(NOT_READY.into()),
            _ => Err("Photos can't be dragged out of Dalica on this system; Copy puts their files on the clipboard.".into()),
        }
    }

    fn cancel(&self) {
        let mut inner = self.lock();
        inner.pending = None;
        #[cfg(target_os = "linux")]
        if let Some(h) = inner.x11.take() {
            h.cancel();
        }
    }
}

/// The window, with the drag-out service at its side: everything is the window's, and after each
/// frame a drag the frame asked for is started with the frame's window handle (Windows, macOS);
/// before each frame a release the OS kept is given back, and a drop of the drag's own files on
/// the window is left out.
pub struct Host<A> {
    app: A,
    native: Arc<Native>,
}

impl<A> Host<A> {
    pub fn new(app: A, native: Arc<Native>) -> Host<A> {
        Host { app, native }
    }
}

/// The events that give the window a release it never saw: a move to where it happened (inside),
/// then the primary button let go there; outside the window, at a point left of it.
pub fn release_events(release: Release, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    let pos = release.inside.unwrap_or(egui::pos2(-1.0, -1.0));
    vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }]
}

impl<A: eframe::App> eframe::App for Host<A> {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.native.lock().backend.is_none() {
            use wgpu::rwh::HasWindowHandle as _;
            let backend = frame.window_handle().map_or(Backend::Other, |h| Backend::of(&h.as_raw()));
            self.native.set_backend(backend, ui.ctx());
        }
        self.app.ui(ui, frame);
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(out) = self.native.take_pending() {
            os_drag::start(frame, ui.ctx(), &out, Arc::clone(&self.native));
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
        if let Some(release) = self.native.take_release() {
            raw_input.events.extend(release_events(release, ctx.input(|i| i.modifiers)));
        }
        let native = &self.native;
        raw_input.dropped_files.retain(|f| !native.is_outgoing(f.path()));
        raw_input.hovered_files.retain(|f| f.path.as_deref().is_none_or(|p| !native.is_outgoing(p)));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn on_exit(&mut self) {
        self.native.cancel();
        self.app.on_exit();
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        self.app.auto_save_interval()
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }

    fn persist_egui_memory(&self) -> bool {
        self.app.persist_egui_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing() -> Outgoing {
        Outgoing { paths: vec![PathBuf::from("/photos/a.jpg")], pill: "1 photo".into(), note: None, image: None, scale: 1.0 }
    }

    #[test]
    fn wayland_and_an_unknown_window_say_why_and_remember_nothing_to_start() {
        let n = Native::default();
        assert_eq!(n.start(outgoing()), Err(NOT_READY.into()));
        n.set_backend(Backend::Wayland, &egui::Context::default());
        assert_eq!(n.start(outgoing()), Err(WAYLAND_REFUSAL.into()));
        assert!(n.take_pending().is_none());
        n.set_backend(Backend::Other, &egui::Context::default());
        assert!(n.start(outgoing()).is_err());
        // The drag's own files dropped back on the window are not a drop of files.
        assert!(n.is_outgoing(std::path::Path::new("/photos/a.jpg")));
        assert!(!n.is_outgoing(std::path::Path::new("/photos/b.jpg")));
    }

    #[test]
    fn a_kept_release_is_given_back_once_inside_or_outside() {
        let n = Native::default();
        n.ended(Release { inside: Some(egui::pos2(10.0, 20.0)) });
        let r = n.take_release().unwrap();
        let events = release_events(r, egui::Modifiers::NONE);
        assert_eq!(events.first(), Some(&egui::Event::PointerMoved(egui::pos2(10.0, 20.0))));
        assert!(matches!(events.get(1), Some(egui::Event::PointerButton { pressed: false, button: egui::PointerButton::Primary, .. })));
        assert!(n.take_release().is_none(), "once");
        let outside = release_events(Release { inside: None }, egui::Modifiers::NONE);
        assert_eq!(outside.first(), Some(&egui::Event::PointerMoved(egui::pos2(-1.0, -1.0))));
    }

    #[test]
    fn the_backend_comes_from_the_window_handle() {
        use std::num::{NonZeroIsize, NonZeroU32};
        let xcb = wgpu::rwh::XcbWindowHandle::new(NonZeroU32::new(77).unwrap());
        assert_eq!(Backend::of(&xcb.into()), Backend::X11(77));
        let xlib = wgpu::rwh::XlibWindowHandle::new(88);
        assert_eq!(Backend::of(&xlib.into()), Backend::X11(88));
        #[cfg(target_os = "linux")]
        {
            let xlib = wgpu::rwh::XlibWindowHandle::new(u64::from(u32::MAX) + 1);
            assert_eq!(Backend::of(&xlib.into()), Backend::Other, "an id X11 cannot have");
        }
        let win = wgpu::rwh::Win32WindowHandle::new(NonZeroIsize::new(5).unwrap());
        assert_eq!(Backend::of(&win.into()), Backend::Windows);
    }
}
