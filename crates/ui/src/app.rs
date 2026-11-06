//! `App`: the UI's half of the program. It owns the [`UiState`] and (once opened) the engine's
//! [`Session`]. Every action goes through [`App::run`]: UI-only commands ([`UI_COMMANDS`]) change
//! the view state here, every other id is forwarded to `Session::execute` (`architecture.md` §3).

use dalica_engine::{CmdError, Session};
use std::sync::Arc;

use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Frame};
use serde_json::Value;

pub use crate::commands::{UI_COMMANDS, UiCommand};
use crate::notices::Notices;
use crate::panels::{self, Panels};
use crate::picker::{FolderRequest, OpenRequest, Pending, Picker, SaveRequest};
use crate::state::{THUMB_DEFAULT, TREE_DEFAULT, UiState};
use dalica_engine::settings::{ViewOptions, ViewState};

pub struct App {
    state: UiState,
    tokens: Tokens,
    session: Option<Session>,
    picker: Option<Arc<dyn Picker>>,
    /// Created on the first frame with a session (it starts the thumbnail workers).
    panels: Option<Panels>,
    /// File ▸ Add Folder… was chosen while the panels were busy drawing: open the picker on the
    /// next frame.
    want_folder_picker: bool,
    /// Whether the folder dialog was open when this frame's panels were taken out to draw (the
    /// menus drawn meanwhile ask [`App::can_pick_folder`]).
    folder_dialog_open: bool,
    /// The save dialog (File ▸ Save As…) while it is open, and what its answer runs.
    save_dialog: Option<(Pending, SaveRequest)>,
    /// A folder dialog other than Add Folder (the Export dialog's Browse…) while it is open, and
    /// what its answer runs.
    folder_request: Option<(Pending, FolderRequest)>,
    /// The open dialog (Add a Model File…) while it is open, and what its answer runs.
    open_request: Option<(Pending, OpenRequest)>,
    /// What the window knows of the face models between frames (`crate::commands::face_model`).
    pub(crate) face_models: crate::commands::face_model::Models,
    /// The Save As job its picker started, and a picker to open again (`crate::commands::save_as`).
    pub(crate) save_as: crate::commands::save_as::Following,
    /// The Collage room's Create Collage or Save Draft job and the display's size
    /// (`crate::commands::collage`).
    pub(crate) collage: crate::commands::collage::Following,
    /// Restore the view state from the library's settings and save it when it changes
    /// ([`App::persist_view`]); off for headless and snapshot sessions, which never read or write
    /// the user's settings (`control-protocol.md` §8).
    persist_view: bool,
    /// The view state as last restored or saved.
    saved_view: Option<Persisted>,
    /// File ▸ Exit (`app.quit`) asked the window to close; the next frame tells it.
    quit_requested: bool,
    /// The text of the tooltip the last frame showed (`ui.inspect` → `tooltip`).
    tooltip: Option<String>,
    /// The toasts showing what the session reported (`crate::notices`).
    notices: Notices,
    /// The frame clock's time at the start of the last frame (seconds): when a toast pushed by a
    /// command between frames appears.
    now: f64,
    /// While the library opens: what the opening is doing ([`App::set_opening_detail`]) and the
    /// frame clock's time of the first opening frame.
    opening_detail: Option<String>,
    opening_since: Option<f64>,
    /// The user's input as last reported to the face scan (`crate::user_activity`).
    activity: crate::user_activity::Reporter,
    /// The window closes on a thread of its own ([`App::close_on_thread`]), and what that thread
    /// does when the window does not go once the close has finished; `None` (headless and
    /// snapshot sessions, which call [`App::shutdown`]) closes in the exit.
    close_on_thread: Option<Stalled>,
    /// The close running on its thread, once the window asked to close ([`App::begin_close`]).
    closing: Option<Closing>,
}

/// The window's close, running on a thread of its own ([`App::begin_close`]): the session's
/// writes land and its workers stop there while the window is hidden and its event loop keeps
/// answering the desktop.
pub struct Closing {
    started: std::time::Instant,
    thread: Option<std::thread::JoinHandle<Result<(), CmdError>>>,
    done: Arc<std::sync::atomic::AtomicBool>,
    /// Set once the window has gone (or the app is dropped): the close's thread stops watching.
    released: Arc<std::sync::atomic::AtomicBool>,
}

/// What the close's thread does when the window has not gone [`CLOSE_STALL`] after every write
/// landed (its event loop gets no turn: a Wayland window the compositor stopped drawing); the
/// argument is whether every write landed. The window ends the process (`apps/dalica`).
pub type Stalled = Arc<dyn Fn(bool) + Send + Sync>;

/// How long the close's thread waits, after every write landed, for the window to go before it
/// calls the [`Stalled`] hook.
pub const CLOSE_STALL: std::time::Duration = std::time::Duration::from_secs(5);

impl Drop for Closing {
    fn drop(&mut self) {
        self.released.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Closing {
    /// Whether every write has landed and the library is closed: the window may go.
    pub fn finished(&self) -> bool {
        self.done.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How long ago the close began.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// Waits for the close to end; `Err(Unsaved)` names what could not be written.
    fn wait(&mut self) -> Result<(), CmdError> {
        self.released.store(true, std::sync::atomic::Ordering::SeqCst);
        match self.thread.take() {
            Some(t) => t.join().unwrap_or_else(|_| Err(CmdError::Unsaved("closing the library stopped unexpectedly".into()))),
            None => Ok(()),
        }
    }
}

/// How long the close may take before the window, if it is still on screen (a desktop that cannot
/// hide it), says "Saving changes…".
pub const SAVING_NOTE_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// A stage of closing that takes longer than this is named in the log.
const SLOW_CLOSE_STAGE: std::time::Duration = std::time::Duration::from_millis(250);

/// Times the stages of closing; a slow one is named in the log.
struct Stages {
    at: std::time::Instant,
}

impl Stages {
    fn new() -> Stages {
        Stages { at: std::time::Instant::now() }
    }

    fn done(&mut self, what: &str) {
        let took = self.at.elapsed();
        if took >= SLOW_CLOSE_STAGE {
            log::info!("dalica: closing: {what} took {} ms", took.as_millis());
        }
        self.at = std::time::Instant::now();
    }
}

/// The session's half of closing ([`App::shutdown`], [`App::begin_close`]): the running scans are
/// let go, the metadata writes still waiting for their debounce are made, then every journal
/// record, sidecar and settings write lands. Blocks the calling thread.
fn close_session(session: &mut Session) -> Result<(), CmdError> {
    let mut stages = Stages::new();
    // A scan still walking is let go, never waited for.
    let stopped = session.stop_scans_for_close();
    if stopped > 0 {
        log::info!("dalica: {stopped} scan(s) stopped on close; the next session verifies the folders again");
    }
    stages.done("stopping the scans");
    if !session.flush_metadata(SHUTDOWN_METADATA_WAIT) {
        log::warn!("dalica: metadata writes are still running; they finish before the library closes");
    }
    stages.done("the metadata writes");
    let flushed = session.flush();
    stages.done("the journal, sidecar and settings writes");
    flushed
}

/// The persisted part of the view state: the window's (M3) and the library view's options (M4).
type Persisted = (ViewState, ViewOptions);

/// How long [`App::shutdown`] waits for the queued metadata writes before it goes on to the
/// sidecars; the session's own close finishes any still running.
pub const SHUTDOWN_METADATA_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

impl App {
    /// An app showing `session`, or the "opening" state while it is `None`.
    pub fn new(mut session: Option<Session>) -> App {
        if let Some(s) = &mut session {
            crate::keys::join_keymap(s);
            s.set_languages(&crate::i18n::choices());
        }
        let mut app = App {
            state: UiState::default(),
            tokens: Tokens::default(),
            session,
            picker: None,
            panels: None,
            want_folder_picker: false,
            folder_dialog_open: false,
            save_dialog: None,
            folder_request: None,
            open_request: None,
            face_models: Default::default(),
            save_as: Default::default(),
            collage: Default::default(),
            persist_view: false,
            saved_view: None,
            quit_requested: false,
            tooltip: None,
            notices: Notices::default(),
            now: 0.0,
            opening_detail: None,
            opening_since: None,
            activity: Default::default(),
            close_on_thread: None,
            closing: None,
        };
        app.show_open_viewer();
        app
    }

    /// `dalica --view`: a session holding an open photo viewer starts on the viewer's view.
    fn show_open_viewer(&mut self) {
        if self.session.as_ref().is_some_and(|s| s.viewer().is_open()) {
            self.state.view = crate::state::View::Viewer;
            self.state.viewer.get_or_insert_with(Default::default);
        }
    }

    /// The same, with the dialogs of `picker` (File ▸ Add Folder…).
    pub fn with_picker(session: Option<Session>, picker: Arc<dyn Picker>) -> App {
        App { picker: Some(picker), ..App::new(session) }
    }

    /// Installs fonts, the theme and the input options; call once before the first frame.
    /// egui's own Ctrl+= / Ctrl+− zoom of the whole window is off: those keys are the shortcuts'
    /// (the edit view's photo zoom). egui's wheel modifiers stay as they are (Ctrl + wheel
    /// zooms), because Ctrl + wheel is the light table's thumbnail size (`ui-layout.md` §5.5);
    /// `wheel::configure` would make it scroll sideways.
    pub fn install(&self, ctx: &egui::Context) {
        self.tokens.apply(ctx);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // The window's AccessKit tree: landmarks, the light table's cells and sections (`a11y`).
        crate::a11y::install(ctx);
    }

    /// Prepares one frame's raw input (the window's `raw_input_hook`): on Windows a touchpad pinch
    /// arrives as Ctrl + wheel in fractions of a notch, and is turned back into a pinch
    /// (`wheel::fold_legacy_pinch`).
    pub fn raw_input(&self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if cfg!(target_os = "windows") {
            dalica_ui_kit::input::wheel::fold_legacy_pinch(ctx, raw);
        }
    }

    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    pub fn state(&self) -> &UiState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut UiState {
        &mut self.state
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    /// Hands over the library once it has been opened (off the UI thread).
    pub fn set_session(&mut self, mut session: Session) {
        crate::keys::join_keymap(&mut session);
        // Options ▸ General ▸ Language lists the window's registry (`localization.md`).
        session.set_languages(&crate::i18n::choices());
        self.session = Some(session);
        if self.persist_view {
            self.restore_view();
        }
        self.show_open_viewer();
    }

    /// Asks the window to close, as its Close button does (`app.quit`, File ▸ Exit): the next frame
    /// sends the close request, and the window writes everything before it goes.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Whether a quit was asked for and not yet passed to the window.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// From now on the view state (tree width, side pane, thumbnail size, the view options) is
    /// restored from the library's `settings.json` (now if the library is open, else when it is)
    /// and saved there when it changes (`ui-layout.md` §2). The window turns it on; headless and
    /// snapshot sessions leave it off.
    pub fn persist_view(&mut self) {
        self.persist_view = true;
        if self.session.is_some() {
            self.restore_view();
        }
    }

    /// Applies the stored view state: a pane name this version does not know is a closed pane.
    fn restore_view(&mut self) {
        let Some(session) = &self.session else { return };
        let stored = (session.settings().view(), session.settings().view_options());
        apply_view(&mut self.state, &stored);
        self.saved_view = Some(view_of(&self.state));
        // The window has opened a library: a new one offers the first-run panel (`organizing.md` §1).
        crate::dialogs::first_run::offer(self);
    }

    /// Saves the view state if it changed since it was restored or last saved. Cheap when nothing
    /// changed; the write runs on the session's I/O worker.
    pub fn save_view(&mut self) {
        if !self.persist_view {
            return;
        }
        let now = view_of(&self.state);
        if self.saved_view.as_ref() == Some(&now) {
            return;
        }
        if let Some(session) = &mut self.session {
            session.change_settings(|s| {
                s.set_view(&now.0);
                s.set_view_options(&now.1);
            });
            self.saved_view = Some(now);
        }
    }

    /// Takes the session out, to flush and close it.
    pub fn take_session(&mut self) -> Option<Session> {
        self.session.take()
    }

    /// What closing does, the window's (its Close button, File ▸ Exit, the system's close) and the
    /// headless app's (`app.quit`) alike: the text an inline editor holds is committed, as a click
    /// elsewhere would commit it (the caption under a cell or in the edit view's caption bar, a
    /// section's description; one undo step
    /// each, nothing when unchanged); the view state is saved, when it is persisted; the running
    /// scans are let go, as Cancel lets them go (`Session::stop_scans_for_close`); the metadata
    /// writes still waiting for their debounce are made now, so their results reach the sidecars;
    /// then every journal record, sidecar and settings write lands (`control-protocol.md` §5.8).
    ///
    /// Call it once the frames are over and the thread is no longer marked as the UI thread: it
    /// blocks until the writes have landed. `Err(Unsaved)` names what could not be written. Metadata
    /// writes still running after [`SHUTDOWN_METADATA_WAIT`] are not an error: dropping the session
    /// finishes them before it returns.
    pub fn shutdown(&mut self) -> Result<(), CmdError> {
        self.prepare_close();
        let Some(session) = &mut self.session else { return Ok(()) };
        close_session(session)
    }

    /// The UI's half of closing, on the UI thread: the editors' text is committed and the view
    /// state saved (both only queue writes).
    fn prepare_close(&mut self) {
        if let Some(edit) = self.state.caption_edit.take() {
            crate::panels::light_table::caption_edit::commit(self, edit.photo, &edit.draft);
        }
        if let Some(edit) = self.state.caption_bar.take() {
            crate::panels::edit::caption_bar::commit(self, edit.photo, &edit.draft);
        }
        if let Some(edit) = self.state.description_edit.take()
            && let Some(owner) = crate::panels::light_table::header::Owner::of(edit.section)
        {
            let (id, params) = owner.set_description(&edit.draft);
            let _ = self.run(id, &params);
        }
        self.save_view();
    }

    /// From now on a close request of the window closes on a thread of its own
    /// ([`App::begin_close`]) instead of in the window's exit; `stalled` runs on that thread if the
    /// window has not gone [`CLOSE_STALL`] after every write landed. The window turns it on;
    /// headless and snapshot sessions leave it off and call [`App::shutdown`].
    pub fn close_on_thread(&mut self, stalled: Stalled) {
        self.close_on_thread = Some(stalled);
    }

    /// The window's close (its Close button, File ▸ Exit, the system's close) without freezing it
    /// (issue #723): what [`App::shutdown`] does, with the session's half (the metadata flush of up
    /// to [`SHUTDOWN_METADATA_WAIT`], the journal and sidecar flush, the joins of the session's
    /// workers) moved to a thread of its own, which wakes `ctx` when it is done. The UI thread
    /// returns at once: the window is hidden meanwhile and goes once [`Closing::finished`]. Every
    /// write still lands before the process ends ([`App::finish_close`] waits for it). `false` when
    /// there is no library to close; `true` when a close runs (or already ran).
    pub fn begin_close(&mut self, ctx: &egui::Context) -> bool {
        if self.closing.is_some() {
            return true;
        }
        let Some(stalled) = self.close_on_thread.clone() else { return false };
        if self.session.is_none() {
            return false;
        }
        self.prepare_close();
        let Some(session) = self.session.take() else { return false };
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (flag, gone, wake) = (Arc::clone(&done), Arc::clone(&released), ctx.clone());
        // The session goes over a channel, so it stays here if the thread cannot start.
        let (tx, rx) = std::sync::mpsc::channel::<Session>();
        let spawned = std::thread::Builder::new().name("dalica-close".into()).spawn(move || {
            let Ok(mut session) = rx.recv() else { return Ok(()) };
            let started = std::time::Instant::now();
            let closed = close_session(&mut session);
            let mut stages = Stages::new();
            drop(session);
            stages.done("closing the library (its workers and caches)");
            log::info!("dalica: closing: every write landed and the library closed in {} ms", started.elapsed().as_millis());
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            // The window goes in its next turn; keep waking it until it has.
            let landed = std::time::Instant::now();
            while !gone.load(std::sync::atomic::Ordering::SeqCst) {
                if landed.elapsed() >= CLOSE_STALL {
                    stalled(closed.is_ok());
                    break;
                }
                wake.request_repaint();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            closed
        });
        match spawned {
            Ok(thread) => match tx.send(session) {
                Ok(()) => {
                    log::info!("dalica: closing: the window hides while its writes land on a thread of its own");
                    self.closing = Some(Closing { started: std::time::Instant::now(), thread: Some(thread), done, released });
                    true
                }
                Err(std::sync::mpsc::SendError(session)) => {
                    self.session = Some(session);
                    false
                }
            },
            Err(e) => {
                log::warn!("dalica: cannot close on a thread of its own ({e}); closing in the window's exit");
                self.session = Some(session);
                false
            }
        }
    }

    /// The close running on its thread, if the window began one.
    pub fn closing(&self) -> Option<&Closing> {
        self.closing.as_ref()
    }

    /// Waits for the close [`App::begin_close`] began (at once when it has finished); `None` when
    /// none began, and [`App::shutdown`] is still to run.
    pub fn finish_close(&mut self) -> Option<Result<(), CmdError>> {
        self.closing.as_mut().map(Closing::wait)
    }

    /// The window's logic tick while closing on a thread: a close running holds the window
    /// (`CancelClose`), a close that has finished lets it go (`Close`); and a close request of a
    /// window not on screen (minimised, hidden), which draws no frame to see it, begins one.
    pub fn close_tick(&mut self, ctx: &egui::Context) {
        if self.close_on_thread.is_none() {
            return;
        }
        let (requested, shown) = ctx.input(|i| {
            let v = i.viewport();
            (v.close_requested(), v.visible() != Some(false) && v.minimized != Some(true))
        });
        match self.closing.as_ref().map(Closing::finished) {
            Some(true) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
            }
            Some(false) if requested => ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose),
            Some(false) => {}
            None if requested && !shown && self.begin_close(ctx) => ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose),
            None => {}
        }
    }

    /// A frame while the close runs on its thread: the window is hidden where the desktop allows;
    /// where it stays on screen it shows the window's ground, and "Saving changes…" once the close
    /// has taken [`SAVING_NOTE_AFTER`]. A close request is held until the close has finished.
    fn closing_frame(&mut self, ui: &mut egui::Ui) {
        let Some(closing) = &self.closing else { return };
        if !closing.finished() && ui.input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let note = closing.elapsed() >= SAVING_NOTE_AFTER;
        let t = self.tokens;
        egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
            if note {
                let c = ui.max_rect().center();
                ui.painter().text(c, Align2::CENTER_CENTER, "Saving changes…", fonts::semibold(18.0), t.text_label);
            }
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }

    /// Runs command `id`: a UI-only one here, anything else in the engine. A command whose UI asks
    /// first, run without `"confirm": true` when its question applies, asks through the dialog host
    /// instead and replies `{dialog: <kind>}` (`crate::dialogs::asking`). A failure's message
    /// becomes the status line. In the edit view the rotations, the flips, Edit ▸ Undo / Redo and
    /// the one-click fixes are steps of the open photo's history (`commands::edit_view::route`).
    /// On an album section `section.properties` opens Album Properties (`commands::albums::route`).
    ///
    /// While a control request drives the app (a synthetic click or key from `ui.clickWidget`,
    /// `ui.pointer`, `ui.key`, …), a command automation never runs (`automation::NEVER_AUTOMATED`:
    /// Locate on Disk, Open With, Open in External Editor, removing imported files from a card) is
    /// refused here, whatever route reached it; the person's own clicks are not marked.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        if crate::control::automated().is_some()
            && let Err(r) = dalica_engine::automation::never_automated(id)
        {
            self.state.status = r.message.clone();
            return Err(CmdError::Disabled(r.message));
        }
        if let Some(r) = crate::commands::compare::route(self, id, params) {
            return r;
        }
        if let Some((routed, p)) = crate::commands::edit_view::route(self, id, params) {
            return self.run(routed, &p);
        }
        if let Some(p) = crate::commands::save::route(self, id, params) {
            return self.run(id, &p);
        }
        let routed = crate::commands::albums::route(self, id, params);
        let result = if let Some(r) = routed {
            r
        } else if let Some(ask) = crate::dialogs::asking(self, id, params) {
            crate::dialogs::ask(self, ask, params)
        } else {
            match UI_COMMANDS.iter().find(|c| c.id == id) {
                Some(c) => (c.run)(self, params),
                None => match &mut self.session {
                    Some(s) => s.execute(id, params),
                    None if dalica_engine::cmd::specs().iter().any(|s| s.id == id) => Err(CmdError::Disabled("The library is still opening.".into())),
                    None => Err(CmdError::UnknownCommand(id.into())),
                },
            }
        };
        match &result {
            Ok(_) => self.state.status.clear(),
            Err(e) => self.state.status = e.to_string(),
        }
        crate::commands::save_as::ran(self, id, params, &result);
        crate::commands::batch::ran(self, id, &result);
        crate::commands::collage::ran(self, id, &result);
        result
    }

    /// Nothing is running in the background: no scan, no metadata write into a photo file queued
    /// or being written (M10.54: the session's poll in a frame lands it), no thumbnail on its way,
    /// no open dialog. The headless loop and the window run frames only while this is false.
    pub fn idle(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.busy() && !s.metadata_writes_busy())
            && self.panels.as_ref().is_none_or(Panels::idle)
            && self.save_dialog.is_none()
            && self.folder_request.is_none()
            && self.open_request.is_none()
    }

    /// Whether File ▸ Add Folder… can open the folder picker now; why not, if it cannot.
    pub fn can_pick_folder(&self) -> Result<(), String> {
        if self.session.is_none() {
            Err("The library is still opening.".into())
        } else if self.picker.is_none() {
            Err("There is no folder dialog here.".into())
        } else if self.want_folder_picker || self.panels.as_ref().map_or(self.folder_dialog_open, |p| p.dialog.is_some()) {
            Err("A folder dialog is already open.".into())
        } else if self.save_dialog.is_some() || self.folder_request.is_some() || self.open_request.is_some() {
            Err("A file dialog is already open.".into())
        } else {
            Ok(())
        }
    }

    /// Whether a native file dialog (the folder picker or the save picker) can open now; why not,
    /// if it cannot: one at a time.
    pub fn can_pick(&self) -> Result<(), String> {
        self.can_pick_folder().map_err(|e| e.replace("folder dialog", "file dialog"))
    }

    /// Opens the save picker for `request` (File ▸ Save As…) on its own thread; the frame that sees
    /// its answer runs the request's command with the path chosen ([`SaveRequest::params_with`]),
    /// and a cancel runs nothing. Refused while another file dialog is open or there is no picker.
    pub fn pick_save(&mut self, request: SaveRequest) -> Result<(), String> {
        self.can_pick()?;
        let picker = self.picker.as_ref().ok_or("There is no file dialog here.")?;
        let pending = Pending::pick_save(picker, request.clone()).ok_or("The file dialog could not open.")?;
        self.save_dialog = Some((pending, request));
        Ok(())
    }

    /// Opens the folder picker for `request` (the Export dialog's Browse…) on its own thread; the
    /// frame that sees its answer runs the request's command with the folder chosen
    /// ([`FolderRequest::params_with`]), and a cancel runs nothing. Refused while another file
    /// dialog is open or there is no picker.
    pub fn pick_folder_for(&mut self, request: FolderRequest) -> Result<(), String> {
        self.can_pick()?;
        let picker = self.picker.as_ref().ok_or("There is no file dialog here.")?;
        let pending = Pending::pick_folder_for(picker, request.clone()).ok_or("The file dialog could not open.")?;
        self.folder_request = Some((pending, request));
        Ok(())
    }

    /// Opens the open-file picker for `request` (Add a Model File…) on its own thread; the frame
    /// that sees its answer runs the request's command with the file chosen
    /// ([`OpenRequest::params_with`]), and a cancel runs nothing. Refused while another file dialog
    /// is open or there is no picker.
    pub fn pick_file_for(&mut self, request: OpenRequest) -> Result<(), String> {
        self.can_pick()?;
        let picker = self.picker.as_ref().ok_or("There is no file dialog here.")?;
        let pending = Pending::pick_file(picker, request.clone()).ok_or("The file dialog could not open.")?;
        self.open_request = Some((pending, request));
        Ok(())
    }

    /// The open-file picker's request while it is open.
    pub fn open_request(&self) -> Option<&OpenRequest> {
        self.open_request.as_ref().map(|(_, r)| r)
    }

    /// Runs the open-file picker's answer once it has one.
    fn poll_open_request(&mut self) {
        let Some(answer) = self.open_request.as_ref().and_then(|(p, _)| p.poll()) else { return };
        if let Some((_, request)) = self.open_request.take()
            && let Some(path) = answer
        {
            let _ = self.run(request.command, &request.params_with(&path));
        }
    }

    /// The folder picker's request (not Add Folder's) while it is open.
    pub fn folder_request(&self) -> Option<&FolderRequest> {
        self.folder_request.as_ref().map(|(_, r)| r)
    }

    /// Runs the folder picker's answer once it has one. Move Folder's (`folder.move`) asks the
    /// collision question first when it would merge into a folder of the same name whose photos'
    /// names clash (`crate::dialogs::collision::run_or_ask_folder`), and the folder chosen is
    /// remembered as the last place a folder moved into (`"organize"` → `lastMoveFolder`) once the
    /// move runs.
    fn poll_folder_request(&mut self) {
        let Some(answer) = self.folder_request.as_ref().and_then(|(p, _)| p.poll()) else { return };
        if let Some((_, request)) = self.folder_request.take()
            && let Some(path) = answer
        {
            let params = request.params_with(&path);
            let _ = if request.command == "folder.move" {
                crate::dialogs::collision::run_or_ask_folder(self, params)
            } else {
                self.run(request.command, &params)
            };
        }
    }

    /// The save picker's request while it is open.
    pub fn save_dialog(&self) -> Option<&SaveRequest> {
        self.save_dialog.as_ref().map(|(_, r)| r)
    }

    /// Runs the save picker's answer once it has one.
    fn poll_save_dialog(&mut self) {
        let Some(answer) = self.save_dialog.as_ref().and_then(|(p, _)| p.poll()) else { return };
        if let Some((_, request)) = self.save_dialog.take()
            && let Some(path) = answer
        {
            let _ = self.run(request.command, &request.params_with(&path));
        }
        crate::commands::save_as::closed(self);
    }

    /// What Save As follows (`crate::commands::save_as`).
    pub fn save_as(&self) -> &crate::commands::save_as::Following {
        &self.save_as
    }

    /// What the Collage room follows (`crate::commands::collage`).
    pub fn collage(&self) -> &crate::commands::collage::Following {
        &self.collage
    }

    /// Opens the folder picker (File ▸ Add Folder…) unless [`App::can_pick_folder`] says no: now
    /// between frames, or on the next frame when the panels are drawing.
    pub fn add_folder(&mut self) {
        if self.can_pick_folder().is_err() {
            return;
        }
        match &mut self.panels {
            Some(p) => p.add_folder(),
            None => self.want_folder_picker = true,
        }
    }

    /// The text of the tooltip the last frame showed, if any.
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// The toasts showing now.
    pub fn notices(&self) -> &Notices {
        &self.notices
    }

    /// The toasts, to add one ([`Notices::push`]).
    pub fn notices_mut(&mut self) -> &mut Notices {
        &mut self.notices
    }

    /// Shows `text` as a toast from the last frame's time on (`crate::notices`): a command's own
    /// message ("Select a photo to edit.").
    pub fn toast(&mut self, text: &str, kind: dalica_ui_kit::widgets::ToastKind) {
        self.notices.push(text, kind, self.now);
    }

    /// What opening the library is doing ("Reading the library…"), shown under "Opening the
    /// library…" until a session arrives; `None` shows the title alone.
    pub fn set_opening_detail(&mut self, detail: Option<String>) {
        self.opening_detail = detail;
    }

    /// The panels' state, once the first frame with a library has run.
    pub fn panels(&self) -> Option<&Panels> {
        self.panels.as_ref()
    }

    /// One frame: applies finished background work, runs this frame's shortcuts, then paints.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if self.closing.is_some() {
            self.closing_frame(ui);
            return;
        }
        if std::mem::take(&mut self.quit_requested) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // The language this frame is drawn in: `interface.language` (`DALICA_LANGUAGE` first).
        crate::i18n::sync(ui.ctx(), self.session.as_ref().map(|s| s.settings().interface().language).as_deref());
        // The theme, then UI scale and font size: the Options dialog's preview, else the saved
        // settings.
        crate::theme::apply(ui.ctx(), &self.state, self.session.as_ref().map(Session::settings), &mut self.tokens);
        crate::scale::apply(ui.ctx(), &self.state, self.session.as_ref().map(Session::settings), &mut self.tokens);
        let Some(session) = &mut self.session else {
            let t = self.tokens;
            let now = ui.input(|i| i.time);
            let since = *self.opening_since.get_or_insert(now);
            let detail = opening_detail(self.opening_detail.as_deref(), now - since);
            egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
                let c = ui.max_rect().center();
                ui.painter().text(c - egui::vec2(0.0, 12.0), Align2::CENTER_CENTER, "Opening the library…", fonts::semibold(18.0), t.text_label);
                if let Some(detail) = &detail {
                    ui.painter().text(c + egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, detail, fonts::regular(t.body), t.text_dim);
                }
            });
            // The seconds count up while a step takes long, so the window is seen to be working.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            return;
        };
        // The face scan's pace follows the user: this frame's input, read before anything takes it.
        self.activity.frame(session, &self.state, ui.ctx());
        session.poll();
        let now = ui.input(|i| i.time);
        self.now = now;
        self.notices.poll(session, now);
        self.poll_save_dialog();
        crate::commands::save_as::follow(self);
        crate::commands::collage::follow(self);
        // Closing the window with unsaved changes in the collage asks Save Draft, Discard or
        // Cancel first; the window stays open meanwhile.
        if ui.input(|i| i.viewport().close_requested()) {
            if crate::commands::collage::ask_before_window_closes(self) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.close_on_thread.is_some() && self.begin_close(ui.ctx()) {
                // The writes land on the close's thread: the window hides at once and goes once
                // they have landed (`close_tick`), never waiting in its exit.
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.closing_frame(ui);
                return;
            }
        }
        self.poll_folder_request();
        self.poll_open_request();
        // The face models: a model file dropped on the window, the face model dialog's downloads
        // one after another, and the dialog offered on the first open of People.
        crate::commands::face_model::frame(self, ui.ctx());
        // Under an open dialog the window takes no keys: no shortcut runs and no panel sees a key
        // press or typed text; the dialog gets them back once the panels are drawn.
        // While the shortcut editor records, every key press is its own (not the keymap's, not the
        // dialog's Esc and Enter).
        crate::keys::capture_for_editor(self, ui.ctx());
        // Esc ends a drag of photos before any shortcut sees it.
        crate::drag::escape(self, ui.ctx());
        // Keyboard-only use: Tab in reading order, the menu bar's F10, Alt and arrows; a focused
        // control's or an open menu's Enter, Space and arrows are kept from the shortcuts.
        let held = crate::a11y::keyboard::before_keys(self, ui.ctx());
        let mut keys = if self.state.dialogs.is_empty() {
            // The edit view resolves keys in its own context, the open tool first; the library, a
            // room, a slideshow and the viewer in theirs (`keys::view_context`).
            match crate::keys::view_context(&self.state) {
                dalica_engine::Context::EditView => crate::commands::edit_view::dispatch(self, ui.ctx()),
                _ => crate::keys::dispatch(self, ui.ctx()),
            };
            Vec::new()
        } else {
            ui.ctx().input_mut(|i| {
                let (keys, rest) =
                    std::mem::take(&mut i.events).into_iter().partition(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
                i.events = rest;
                keys
            })
        };
        if self.state.dialogs.is_empty() {
            crate::a11y::keyboard::give_back(ui.ctx(), held);
        } else {
            keys.splice(0..0, held);
        }
        let mut p = self.panels.take().unwrap_or_else(|| Panels::new(self.picker.clone()));
        if std::mem::take(&mut self.want_folder_picker) {
            p.add_folder();
        }
        self.folder_dialog_open = p.dialog.is_some();
        panels::show(&mut p, self, ui);
        crate::panels::places::tiles::end_frame(self, ui.ctx());
        // A drag of photos: its target, its pill, and its drop on release (`crate::drag`).
        crate::drag::frame(&mut p, self, ui.ctx());
        self.panels = Some(p);
        if !keys.is_empty() {
            ui.ctx().input_mut(|i| i.events.extend(keys));
        }
        // Toasts at the bottom of the light table (the photo area in the edit view), over the panels
        // and under any dialog.
        let area = panels::toast_area(ui.max_rect(), &self.state);
        self.notices.show(ui, &self.tokens, area, now);
        crate::dialogs::show(self, ui.ctx());
        self.tooltip = tooltip_text(ui.ctx());
        // A focused cell follows the selection; the frame's nodes for the AccessKit tree.
        crate::a11y::keyboard::after_frame(self, ui.ctx());
        crate::a11y::describe(self, ui.ctx());
        // Not in the middle of a drag (the splitter): save once the button is up.
        if !ui.ctx().input(|i| i.pointer.any_down()) {
            self.save_view();
        }
    }
}

/// The text of the tooltips painted this pass (egui's tooltip layers), lines joined with `\n`;
/// `None` when no tooltip is shown. Call at the end of the pass, once every widget is drawn.
fn tooltip_text(ctx: &egui::Context) -> Option<String> {
    /// How deep nested shape lists are followed, and how much text is kept.
    const DEPTH: usize = 8;
    const MAX_CHARS: usize = 4096;
    fn walk(shape: &egui::Shape, depth: usize, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) if !t.galley.text().trim().is_empty() => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) if depth < DEPTH => v.iter().for_each(|s| walk(s, depth + 1, out)),
            _ => {}
        }
    }
    let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Tooltip).collect());
    let mut lines = Vec::new();
    ctx.graphics(|g| {
        for layer in &layers {
            if let Some(list) = g.get(*layer) {
                list.all_entries().for_each(|c| walk(&c.shape, 0, &mut lines));
            }
        }
    });
    (!lines.is_empty()).then(|| lines.join("\n").chars().take(MAX_CHARS).collect())
}

/// A view-state value as its protocol name (`"tree"`, `"recent"`).
fn name_of<T: serde::Serialize>(v: T) -> Option<String> {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string))
}

/// A protocol name as a view-state value; `None` for a name this version does not know.
fn from_name<T: serde::de::DeserializeOwned>(name: &Option<String>) -> Option<T> {
    name.as_ref().and_then(|n| serde_json::from_value(Value::String(n.clone())).ok())
}

/// The persisted part of `state` (`ui-layout.md` §2): the tree width and side pane, the thumbnail
/// size and the view options (tree mode, folder sort, caption, small and hidden pictures).
fn view_of(state: &UiState) -> Persisted {
    let view = ViewState { tree_width: Some(state.tree_width), pane: state.pane.and_then(name_of), pane_large: state.pane_large };
    let options = ViewOptions {
        thumb_size: Some(state.thumb_size),
        tree_mode: name_of(state.tree_mode),
        folder_sort: name_of(state.folder_sort),
        folder_sort_reverse: state.folder_sort_reverse,
        caption: name_of(state.caption),
        small_pictures: state.small_pictures,
        hidden_pictures: state.hidden_pictures,
    };
    (view, options)
}

/// Applies stored view state to `state`, made safe by [`UiState::sanitized`]: an absent or hostile
/// value keeps the default, a tree width or thumbnail size out of range is clamped (the layout
/// clamps the width further to the window), an unknown pane is closed, an unknown name is the
/// default.
fn apply_view(state: &mut UiState, (v, o): &Persisted) {
    state.tree_width = v.tree_width.unwrap_or(TREE_DEFAULT);
    state.pane = from_name(&v.pane);
    state.pane_large = v.pane_large;
    state.thumb_size = o.thumb_size.unwrap_or(THUMB_DEFAULT);
    state.tree_mode = from_name(&o.tree_mode).unwrap_or_default();
    state.folder_sort = from_name(&o.folder_sort).unwrap_or_default();
    state.folder_sort_reverse = o.folder_sort_reverse;
    state.caption = from_name(&o.caption).unwrap_or_default();
    state.small_pictures = o.small_pictures;
    state.hidden_pictures = o.hidden_pictures;
    *state = std::mem::take(state).sanitized();
}

/// The line under "Opening the library…": the step, with the seconds it has taken once they pass
/// [`OPENING_COUNT_AFTER`], so a slow opening is seen to be working.
fn opening_detail(detail: Option<&str>, elapsed: f64) -> Option<String> {
    let seconds = (elapsed >= OPENING_COUNT_AFTER).then(|| format!("{} s", elapsed.floor() as u64));
    match (detail, seconds) {
        (Some(d), Some(s)) => Some(format!("{d}  {s}")),
        (Some(d), None) => Some(d.to_string()),
        (None, s) => s,
    }
}

/// Seconds of opening before the opening screen counts them.
const OPENING_COUNT_AFTER: f64 = 3.0;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::TREE_MIN;
    use dalica_core::PhotoId;
    use serde_json::json;
    use std::sync::Arc;

    /// The opening screen names the step, and counts the seconds once an opening takes a while.
    #[test]
    fn the_opening_line_names_the_step_and_counts_slow_seconds() {
        assert_eq!(opening_detail(None, 0.5), None);
        assert_eq!(opening_detail(Some("Reading the library…"), 1.0).as_deref(), Some("Reading the library…"));
        assert_eq!(opening_detail(Some("Reading the library…"), 12.7).as_deref(), Some("Reading the library…  12 s"));
        assert_eq!(opening_detail(None, 4.2).as_deref(), Some("4 s"));
    }

    /// A session on a fresh library holding one folder of `n` scenes photos; and their ids.
    pub(crate) fn session_with_photos(n: usize) -> (tempfile::TempDir, Session, Vec<PhotoId>) {
        let dir = tempfile::tempdir().unwrap();
        let files = dalica_scenes::demo_library(1, n, 7).unwrap();
        for f in &files {
            let path = dir.path().join("photos").join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.bytes).unwrap();
        }
        let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
        let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
        let ids = found["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect();
        (dir, s, ids)
    }

    /// Installing turns off egui's keyboard zoom of the whole window before the first frame, and
    /// leaves Ctrl + wheel zooming (the light table's thumbnail size).
    #[test]
    fn install_turns_off_keyboard_zoom() {
        let ctx = egui::Context::default();
        assert!(ctx.options(|o| o.zoom_with_keyboard), "egui's default");
        App::new(None).install(&ctx);
        let (keyboard, zoom) = ctx.options(|o| (o.zoom_with_keyboard, o.input_options.zoom_modifier));
        assert!(!keyboard);
        assert_eq!(zoom, egui::Modifiers::COMMAND);
        // A frame's raw input with no pinch in it passes unchanged.
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        App::new(None).raw_input(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 1);
    }

    /// `ui-layout.md` §2: the tree width and the side pane survive a restart through
    /// `settings.json`, when the app persists them (the window); hostile stored values are not used.
    #[test]
    fn the_view_state_survives_a_restart() {
        use crate::state::Pane;
        let (dir, s, _ids) = session_with_photos(1);
        let library = dir.path().join("library");
        let mut app = App::new(Some(s));
        app.persist_view();
        assert_eq!(app.state().tree_width, TREE_DEFAULT);
        app.state_mut().tree_width = 333.0;
        app.state_mut().pane = Some(Pane::Tags);
        app.state_mut().pane_large = true;
        app.save_view();
        app.take_session().unwrap().flush().unwrap();
        drop(app);
        // A new app on the reopened library starts where the last one stopped.
        let reopened = || Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
        let mut app = App::new(None);
        app.persist_view();
        app.set_session(reopened());
        assert_eq!((app.state().tree_width, app.state().pane, app.state().pane_large), (333.0, Some(Pane::Tags), true));
        // Closing the pane is saved too.
        app.state_mut().pane = None;
        app.save_view();
        app.take_session().unwrap().flush().unwrap();
        let mut app = App::new(Some(reopened()));
        // Without persistence (headless, snapshots) the stored state is not read.
        assert_eq!(app.state().tree_width, TREE_DEFAULT);
        app.persist_view();
        assert_eq!((app.state().tree_width, app.state().pane), (333.0, None));
        // Hostile values stored by hand: the defaults (and the minimum width) are used.
        let mut session = app.take_session().unwrap();
        session.change_settings(|s| s.merge("view", &[("treeWidth", Some(json!(-40.0))), ("pane", Some(json!("attic")))]));
        let mut app = App::new(Some(session));
        app.persist_view();
        assert_eq!((app.state().tree_width, app.state().pane), (TREE_DEFAULT, None));
        // Below the minimum: the minimum; wide but possible: kept (the layout fits it to the window).
        for (stored, used) in [(12.0, TREE_MIN), (9000.0, 9000.0)] {
            let mut session = app.take_session().unwrap();
            session.change_settings(|s| s.merge("view", &[("treeWidth", Some(json!(stored)))]));
            app = App::new(Some(session));
            app.persist_view();
            assert_eq!(app.state().tree_width, used, "{stored}");
        }
        // A width no window can have is dropped when read: the default.
        let mut session = app.take_session().unwrap();
        session.change_settings(|s| s.merge("view", &[("treeWidth", Some(json!(1e9)))]));
        let mut app = App::new(Some(session));
        app.persist_view();
        assert_eq!(app.state().tree_width, TREE_DEFAULT);
    }

    /// Closing the window with unsaved changes in the collage asks Save Draft, Discard or Cancel
    /// (what closes: the window), keeping the window open; Discard then closes the room and asks
    /// the window to close again, which it now may.
    #[test]
    fn closing_the_window_with_an_unsaved_collage_asks_first() {
        use crate::state::{CollageClose, Room};
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        let ctx = egui::Context::default();
        app.install(&ctx);
        let close = |app: &mut App| {
            let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
            input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
            let mut out = ctx.run_ui(input, |ui| app.frame(ui));
            out.textures_delta.clear();
            out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose))
        };
        // Nothing unsaved: the window closes.
        assert!(!close(&mut app));
        app.run("collage.open", &json!({ "ids": ids })).unwrap();
        assert!(!close(&mut app), "a collage without changes");
        app.run("collage.settings", &json!({ "orientation": "portrait" })).unwrap();
        app.run("room.select", &json!({ "room": "library" })).unwrap();
        assert!(close(&mut app), "kept open while it asks");
        let state = app.state();
        assert_eq!((state.room, state.collage.close), (Room::Collage, Some(CollageClose::Window)));
        assert_eq!(crate::dialogs::top(&app).map(|d| d.kind.name()), Some("collageClose"));
        assert!(close(&mut app), "asked once, still open");
        assert_eq!(app.state().dialogs.len(), 1);
        crate::dialogs::answer(&mut app, Some("discard")).unwrap();
        assert!(app.quit_requested(), "Discard closes the window after the room");
        assert_eq!(app.state().rooms, [Room::Library]);
        assert!(!close(&mut app));
    }

    #[test]
    fn without_a_session_engine_commands_wait_and_unknown_ones_are_unknown() {
        let mut app = App::new(None);
        assert!(!app.idle());
        assert!(matches!(app.run("photo.star.toggle", &json!({ "ids": [1] })), Err(CmdError::Disabled(_))));
        assert!(matches!(app.run("nope", &json!({})), Err(CmdError::UnknownCommand(_))));
        assert!(matches!(app.run("select.set", &json!({ "ids": [1] })), Err(CmdError::NotFound(_))));
        assert_eq!(app.run("select.set", &json!({ "ids": [] })).unwrap(), json!({ "selection": [] }));
    }
}
