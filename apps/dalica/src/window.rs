//! The eframe side of the app: the `Session` is opened on its own thread, the UI thread is marked
//! (so the `fs` guard refuses disk access from it in debug builds), and every frame hands over to
//! `dalica_ui::App`, which polls the engine and paints. Each frame is measured into a [`Perf`]
//! (`ui.inspect` → `perf`; `DALICA_PROFILE=1` logs slow frames). With `--control`, requests are
//! answered from the logic tick ([`crate::control::play`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use dalica_core::app_id::APP_ID;
use dalica_engine::Session;
use dalica_ui::control::Control;
use dalica_ui::perf::{self, Perf, Sample};
use dalica_ui::titlebar;
use dalica_ui::{App, Picker};
use dalica_ui_kit::{Tokens, fonts, registry};
use egui::{Align2, Frame, Rect, ViewportBuilder, ViewportCommand, vec2};

use crate::control::play::WindowControl;
use crate::control::{Listening, TokenFileGuard};
use crate::frame::{FrameSetup, Tracker};

/// The OS window to make: the frame's chrome and placement ([`FrameSetup::viewport`]), named with
/// the application id ([`APP_ID`]). eframe turns it into the Wayland app id and the X11 class
/// (`WM_CLASS`), which the desktop matches with Dalica's desktop file and icon; without it eframe
/// would use the app name, "Dalica". The window's own icon ([`crate::icon`]) shows where no
/// desktop file is installed (a binary run from the build folder) and on Windows and macOS.
pub fn viewport(frame: &FrameSetup) -> ViewportBuilder {
    let viewport = frame.viewport().with_app_id(APP_ID);
    match crate::icon::window_icon() {
        Some(icon) => viewport.with_icon(icon),
        None => viewport,
    }
}

/// Native folder and save dialogs through `rfd` (the XDG portal on Linux). Called on a dialog
/// thread.
pub struct RfdPicker;

impl Picker for RfdPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Add Folder").pick_folder()
    }

    fn pick_folder_for(&self, request: &dalica_ui::picker::FolderRequest) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new().set_title(request.title);
        if let Some(folder) = &request.folder {
            dialog = dialog.set_directory(folder);
        }
        dialog.pick_folder()
    }

    fn pick_save(&self, request: &dalica_ui::picker::SaveRequest) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new().set_title(request.title).set_file_name(&request.name);
        if let Some(folder) = &request.folder {
            dialog = dialog.set_directory(folder);
        }
        for f in request.filters {
            dialog = dialog.add_filter(f.name, f.extensions);
        }
        dialog.save_file()
    }
}

/// Which library the window opens.
pub enum Source {
    Library(PathBuf),
    /// The demo library in this folder (the persistent one or a fresh one), made on first use
    /// (`dalica_engine::demo`).
    Demo(PathBuf),
}

impl Source {
    /// The library directory this source opens (where its `settings.json` is).
    pub fn library_dir(&self) -> PathBuf {
        match self {
            Source::Library(dir) => dir.clone(),
            Source::Demo(dir) => dalica_engine::demo::library_dir(dir),
        }
    }
}

/// Opens the library of `source` (off the UI thread: it reads and may write the disk).
pub fn open(source: Source) -> Result<Session, String> {
    let clock = Arc::new(dalica_core::SystemClock);
    match source {
        Source::Library(dir) => Session::open(&dir, clock).map_err(|e| e.to_string()),
        Source::Demo(dir) => dalica_engine::demo::open(&dir, clock).map_err(|e| e.to_string()),
    }
}

/// `--control` for the window: the bound listener and what automation may do.
pub struct ControlSetup {
    pub listening: Listening,
    pub control: Control,
}

pub struct Window {
    app: App,
    opening: Option<Receiver<Result<Session, String>>>,
    failed: Option<String>,
    control: Option<WindowControl>,
    perf: Perf,
    /// The last frame's measurements, counted once its pass has ended (in the next logic tick).
    sample: Option<Sample>,
    /// The window's placement, saved in the library's settings once a change has settled.
    placement: Tracker,
    /// Keeps the OS window's full screen and the view state's in step.
    fullscreen: FullscreenSync,
    /// The system menu bar (macOS), and when it was last brought up to date.
    #[cfg(target_os = "macos")]
    native: Option<(crate::native_menu::NativeMenuBar, Instant)>,
    _ui_thread: Option<dalica_fs::UiThread>,
}

/// How often the macOS menu bar is brought up to date with the menu model.
#[cfg(target_os = "macos")]
const NATIVE_MENU_EVERY: Duration = Duration::from_millis(250);

impl Window {
    /// Starts opening the library on another thread; the window shows "Opening…" until it arrives.
    /// With `control`, starts accepting connections; returns the token file to remove at exit.
    /// `frame` is the chrome and placement the window was made with.
    pub fn new(ctx: &egui::Context, source: Source, control: Option<ControlSetup>, frame: FrameSetup) -> (Window, Option<TokenFileGuard>) {
        let mut failed = None;
        let mut token_file = None;
        let (picker, control) = match control {
            None => (Arc::new(RfdPicker) as Arc<dyn Picker>, None),
            Some(ControlSetup { listening, control }) => {
                let wake = ctx.clone();
                let picker = control.picker();
                match listening.start(Arc::new(move || wake.request_repaint())) {
                    Ok((hub, guard)) => {
                        token_file = Some(guard);
                        (picker, Some(WindowControl::new(hub, control)))
                    }
                    Err(e) => {
                        log::error!("dalica: {e}");
                        failed = Some(e);
                        (picker, None)
                    }
                }
            }
        };
        let mut app = App::with_picker(None, picker);
        app.install(ctx);
        titlebar::set_chrome(ctx, frame.chrome);
        // The window keeps its view state between runs; headless sessions never do.
        app.persist_view();
        #[cfg(target_os = "macos")]
        let native = {
            let bar = crate::native_menu::native_bar(&dalica_ui::menus::bar(&app, dalica_ui::menus::Platform::Mac));
            Some((crate::native_menu::NativeMenuBar::new(bar), Instant::now()))
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("dalica-open".into()).spawn(move || {
            let _ = tx.send(open(source));
            ctx.request_repaint();
        });
        if let Err(e) = spawned {
            failed = Some(format!("cannot start opening the library: {e}"));
        }
        let perf = Perf::new(perf::profile_from_env());
        let window = Window {
            app,
            opening: Some(rx),
            failed,
            control,
            perf,
            sample: None,
            placement: Tracker::new(frame.stored),
            fullscreen: FullscreenSync::default(),
            #[cfg(target_os = "macos")]
            native,
            _ui_thread: None,
        };
        (window, token_file)
    }
}

impl eframe::App for Window {
    /// Runs before every frame, and alone while the window is hidden: control requests are
    /// answered here (`control-protocol.md` §5.1).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self._ui_thread.is_none() {
            self._ui_thread = Some(dalica_fs::mark_ui_thread());
        }
        // The last frame's pass has ended: its widgets and reported counts are complete.
        if let Some(mut sample) = self.sample.take() {
            sample.count(&perf::reported(ctx), &registry::widgets(ctx));
            let _ = self.perf.record(sample);
        }
        if let Some(c) = &mut self.control {
            c.logic(&mut self.app, ctx, &mut self.perf);
        }
        #[cfg(target_os = "macos")]
        self.native_menu(ctx);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // The platform's own input first (a Windows touchpad pinch), then the control channel's.
        self.app.raw_input(ctx, raw_input);
        if let Some(c) = &mut self.control {
            c.raw_input(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.frame(ui);
        let layout = started.elapsed();
        if let Some(c) = &mut self.control {
            c.after_frame(&self.app, ui.ctx());
        }
        // The placement as this frame saw it (the logic tick has no viewport information). Where
        // the platform tells no window position (Wayland) egui gives no inner rect either: the
        // size is then the frame's own.
        let mut info = ui.ctx().input(|i| i.viewport().clone());
        if info.inner_rect.is_none() {
            info.inner_rect = Some(ui.ctx().content_rect());
        }
        if let Some(state) = self.placement.observe(&info, Instant::now()) {
            self.save_placement(state);
        }
        // F11 (and View → Full Screen, `ui.set`) asks the OS window to follow the view state; a
        // full screen the OS ends by itself (Esc on macOS, the window manager) flips it back.
        match self.fullscreen.step(self.app.state().full_screen, info.fullscreen) {
            FullscreenAction::Nothing => {}
            FullscreenAction::Send(on) => ui.ctx().send_viewport_cmd(ViewportCommand::Fullscreen(on)),
            FullscreenAction::Adopt(on) => {
                self.app.state_mut().full_screen = on;
                ui.ctx().request_repaint();
            }
        }
        let at = ui.ctx().input(|i| i.time);
        self.sample = Some(Sample { at, frame: layout, update: started.elapsed(), ..Sample::default() });
    }

    fn on_exit(&mut self) {
        // The last placement and view state, if they changed since they were saved.
        if let Some(state) = self.placement.unsaved() {
            self.save_placement(state);
        }
        self.app.save_view();
        // No more frames: this thread may write again (the flush below).
        self._ui_thread = None;
        if let Some(session) = self.app.session_mut()
            && let Err(e) = session.flush()
        {
            log::error!("dalica: not every change was saved: {e}");
        }
    }
}

/// What the window does this frame to keep the OS window's full screen and
/// [`dalica_ui::UiState::full_screen`] in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenAction {
    Nothing,
    /// Ask the OS window to enter (`true`) or leave full screen.
    Send(bool),
    /// The OS changed the window's full screen by itself: the view state takes it over.
    Adopt(bool),
}

/// Keeps the OS window's full screen and the view state's in step, in both directions, without the
/// two ever fighting: a request is sent once and then waited for (never resent while the OS has not
/// answered), and the view state adopts the OS's state only on an edge of what the OS reports while
/// no request is outstanding. Pure, so it is tested on every OS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FullscreenSync {
    /// What the OS reported the frame before.
    before: Option<bool>,
    /// The request sent and not yet seen through.
    sent: Option<bool>,
}

impl FullscreenSync {
    /// One frame: `want` is the view state's full screen, `have` what the OS reports (`None` when it
    /// tells nothing, taken as not full screen).
    pub fn step(&mut self, want: bool, have: Option<bool>) -> FullscreenAction {
        let changed = have.is_some() && have != self.before;
        if have.is_some() {
            self.before = have;
        }
        let have = have.unwrap_or(false);
        if have == want {
            // In step: whatever was asked for is done (or no longer wanted).
            self.sent = None;
            FullscreenAction::Nothing
        } else if self.sent == Some(want) {
            // Asked already: wait for the OS rather than asking again every frame.
            FullscreenAction::Nothing
        } else if changed && self.sent.is_none() {
            // The OS moved on its own (it was in step before): follow it.
            FullscreenAction::Adopt(have)
        } else {
            self.sent = Some(want);
            FullscreenAction::Send(want)
        }
    }
}

impl Window {
    /// Saves the window's placement in the library's settings (on the session's I/O worker); not
    /// before the library is open.
    fn save_placement(&mut self, state: dalica_engine::settings::WindowState) {
        if let Some(session) = self.app.session_mut() {
            session.change_settings(|s| s.set_window(&state));
            self.placement.saved(state);
        }
    }

    /// Brings the macOS menu bar up to date and runs the items chosen in it.
    #[cfg(target_os = "macos")]
    fn native_menu(&mut self, ctx: &egui::Context) {
        let Some((native, at)) = &mut self.native else { return };
        let clicked = native.clicked();
        for id in &clicked {
            crate::native_menu::run(&mut self.app, id);
        }
        if !clicked.is_empty() || at.elapsed() >= NATIVE_MENU_EVERY {
            native.update(crate::native_menu::native_bar(&dalica_ui::menus::bar(&self.app, dalica_ui::menus::Platform::Mac)));
            *at = Instant::now();
        }
        if !clicked.is_empty() {
            ctx.request_repaint();
        }
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.opening {
            match rx.try_recv() {
                Ok(Ok(session)) => {
                    log::info!("dalica: opened the library in {}", session.dir().display());
                    self.app.set_session(session);
                    self.opening = None;
                }
                Ok(Err(e)) => {
                    log::error!("dalica: cannot open the library: {e}");
                    self.failed = Some(e.to_string());
                    self.opening = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.failed = Some("opening the library stopped unexpectedly".into());
                    self.opening = None;
                }
            }
        }
        if let Some(why) = &self.failed {
            let t = Tokens::default();
            egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
                let c = ui.max_rect().center();
                ui.painter().text(
                    c - egui::vec2(0.0, 12.0),
                    Align2::CENTER_CENTER,
                    "The library could not be opened",
                    fonts::semibold(18.0),
                    t.text_label,
                );
                ui.painter().text(c + egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, why, fonts::regular(t.body), t.text_dim);
            });
            self.bare_title_bar(ui);
            titlebar::resize_edges(ui);
            return;
        }
        self.app.frame(ui);
        if self.app.session().is_none() {
            self.bare_title_bar(ui);
        }
        titlebar::resize_edges(ui);
        if !self.app.idle() {
            // Scans, thumbnails and dialogs finish on other threads: keep polling while they run.
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }

    /// The title bar over the "Opening…" and "could not be opened" screens, which have no panels:
    /// without OS decorations it is the only way to move, maximize or close the window then.
    fn bare_title_bar(&mut self, ui: &mut egui::Ui) {
        let content = ui.ctx().content_rect();
        let bar = Rect::from_min_size(content.min, vec2(content.width(), dalica_ui::panels::layout::MENU_BAR));
        ui.painter().rect_filled(bar, 0, self.app.tokens().chrome);
        let mut row = ui.new_child(egui::UiBuilder::new().id_salt("dalica-bare-title-bar").max_rect(bar.shrink2(vec2(8.0, 0.0))));
        row.set_clip_rect(bar);
        titlebar::show(&mut row, &mut self.app, bar, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{FullscreenAction as A, FullscreenSync};
    use crate::frame::{FrameSetup, Os, chrome_for};

    #[test]
    fn the_window_is_named_with_the_application_id() {
        for os in [Os::Mac, Os::Other] {
            for system_title_bar in [false, true] {
                let frame = FrameSetup { chrome: chrome_for(os, system_title_bar), stored: None };
                let v = super::viewport(&frame);
                assert_eq!(v.app_id.as_deref(), Some(dalica_core::app_id::APP_ID));
                assert_eq!(v.app_id.as_deref(), Some("org.dalica.Dalica"));
                // And carries the app icon.
                let icon = v.icon.as_deref().expect("the window sets its icon");
                let side = if cfg!(target_os = "macos") { 1024 } else { 256 };
                assert_eq!((icon.width, icon.height), (side, side));
                assert_eq!(icon.rgba.len(), (side * side * 4) as usize);
                // The rest of the frame is the frame's own.
                assert_eq!(v.title, frame.viewport().title);
                assert_eq!(v.decorations, frame.viewport().decorations);
            }
        }
    }

    /// Runs `frames` of (view state, what the OS reports) through a fresh sync and returns the actions.
    fn run(frames: &[(bool, Option<bool>)]) -> Vec<A> {
        let mut s = FullscreenSync::default();
        frames.iter().map(|&(want, have)| s.step(want, have)).collect()
    }

    #[test]
    fn a_window_in_step_does_nothing() {
        assert_eq!(run(&[(false, None), (false, Some(false)), (false, Some(false))]), [A::Nothing; 3]);
        assert_eq!(run(&[(true, Some(true)), (true, Some(true))]), [A::Nothing; 2]);
    }

    #[test]
    fn f11_asks_once_then_waits_for_the_os() {
        let f = [(false, Some(false)), (true, Some(false)), (true, Some(false)), (true, Some(false)), (true, Some(true)), (true, Some(true))];
        assert_eq!(run(&f), [A::Nothing, A::Send(true), A::Nothing, A::Nothing, A::Nothing, A::Nothing]);
        // And back out.
        let f = [(true, Some(true)), (false, Some(true)), (false, Some(true)), (false, Some(false)), (false, Some(false))];
        assert_eq!(run(&f), [A::Nothing, A::Send(false), A::Nothing, A::Nothing, A::Nothing]);
    }

    #[test]
    fn a_full_screen_the_os_ends_flips_the_view_state_back() {
        let mut s = FullscreenSync::default();
        assert_eq!(s.step(false, Some(false)), A::Nothing);
        assert_eq!(s.step(true, Some(false)), A::Send(true));
        assert_eq!(s.step(true, Some(true)), A::Nothing);
        // Esc on macOS, or the window manager: the OS leaves full screen by itself.
        assert_eq!(s.step(true, Some(false)), A::Adopt(false));
        // The caller adopted it: in step again, nothing sent back.
        assert_eq!(s.step(false, Some(false)), A::Nothing);
        // The OS entering full screen by itself (the macOS green button) is adopted too.
        assert_eq!(s.step(false, Some(true)), A::Adopt(true));
        assert_eq!(s.step(true, Some(true)), A::Nothing);
    }

    #[test]
    fn pressing_f11_again_before_the_os_answers_never_loops() {
        let mut s = FullscreenSync::default();
        assert_eq!(s.step(false, Some(false)), A::Nothing);
        assert_eq!(s.step(true, Some(false)), A::Send(true));
        // Changed its mind before the OS answered: in step with the OS, nothing to send.
        assert_eq!(s.step(false, Some(false)), A::Nothing);
        // The late answer to the first request arrives: the view state follows, it does not fight.
        assert_eq!(s.step(false, Some(true)), A::Adopt(true));
        assert_eq!(s.step(true, Some(true)), A::Nothing);
    }

    #[test]
    fn an_os_that_refuses_or_tells_nothing_is_asked_once() {
        // Never full screen, never reporting: one request, then quiet; F11 again ends it.
        let mut f = vec![(true, None); 50];
        f.push((false, None));
        let actions = run(&f);
        assert_eq!(actions.first(), Some(&A::Send(true)));
        assert!(actions.iter().skip(1).all(|a| *a == A::Nothing), "{actions:?}");
        let mut f = vec![(false, Some(false))];
        f.extend([(true, Some(false)); 50]);
        let actions = run(&f);
        assert_eq!(actions.iter().filter(|a| **a != A::Nothing).count(), 1, "{actions:?}");
    }

    /// Simulates an OS that applies each request `lag` frames later, and the view state adopting
    /// what the sync says, under F11 presses at random frames: the sends never outnumber the presses
    /// and the two end in step.
    #[test]
    fn with_a_lagging_os_the_two_settle_in_step() {
        for lag in 0..4usize {
            let mut s = FullscreenSync::default();
            let (mut want, mut os) = (false, false);
            let mut queue: Vec<(usize, bool)> = Vec::new();
            let (mut presses, mut sends) = (0, 0);
            for frame in 0..400usize {
                if frame < 300 && (frame * 7919) % 37 == 0 {
                    want = !want;
                    presses += 1;
                }
                if let Some(i) = queue.iter().position(|(at, _)| *at <= frame) {
                    os = queue.remove(i).1;
                }
                match s.step(want, Some(os)) {
                    A::Nothing => {}
                    A::Send(on) => {
                        sends += 1;
                        queue.push((frame + lag, on));
                    }
                    A::Adopt(on) => want = on,
                }
            }
            assert!(sends <= presses, "lag {lag}: {sends} sends for {presses} presses");
            assert_eq!(want, os, "lag {lag}: the view state and the OS end in step");
        }
    }
}
