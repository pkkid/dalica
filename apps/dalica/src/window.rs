//! The eframe side of the app: the `Session` is opened on its own thread, the UI thread is marked
//! (so the `fs` guard refuses disk access from it in debug builds), and every frame hands over to
//! `dalica_ui::App`, which polls the engine and paints.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use dalica_engine::{OpenError, Session};
use dalica_ui::{App, Picker};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Frame};

/// Native folder dialogs through `rfd` (the XDG portal on Linux). Called on a dialog thread.
pub struct RfdPicker;

impl Picker for RfdPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Add Folder").pick_folder()
    }
}

pub struct Window {
    app: App,
    opening: Option<Receiver<Result<Session, OpenError>>>,
    failed: Option<String>,
    _ui_thread: Option<dalica_fs::UiThread>,
}

impl Window {
    /// Starts opening the library in `dir` on another thread; the window shows "Opening…" until it
    /// arrives.
    pub fn new(ctx: &egui::Context, dir: PathBuf) -> Window {
        let app = App::with_picker(None, Arc::new(RfdPicker));
        app.install(ctx);
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("dalica-open".into()).spawn(move || {
            let opened = Session::open(&dir, Arc::new(dalica_core::SystemClock));
            let _ = tx.send(opened);
            ctx.request_repaint();
        });
        let failed = spawned.err().map(|e| format!("cannot start opening the library: {e}"));
        Window { app, opening: Some(rx), failed, _ui_thread: None }
    }
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self._ui_thread.is_none() {
            self._ui_thread = Some(dalica_fs::mark_ui_thread());
        }
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
            return;
        }
        self.app.frame(ui);
        if !self.app.idle() {
            // Scans, thumbnails and dialogs finish on other threads: keep polling while they run.
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self) {
        if let Some(session) = self.app.session_mut()
            && let Err(e) = session.flush()
        {
            log::error!("dalica: not every change was saved: {e}");
        }
    }
}
