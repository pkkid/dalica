//! The eframe side of the app: the `Session` is opened on its own thread, the UI thread is marked
//! (so the `fs` guard refuses disk access from it in debug builds), and every frame hands over to
//! `dalica_ui::App`, which polls the engine and paints. Each frame is measured into a [`Perf`]
//! (`ui.inspect` → `perf`; `DALICA_PROFILE=1` logs slow frames). With `--control`, requests are
//! answered from the logic tick ([`crate::control::play`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use dalica_engine::Session;
use dalica_ui::control::Control;
use dalica_ui::perf::{self, Perf, Sample};
use dalica_ui::{App, Picker};
use dalica_ui_kit::{Tokens, fonts, registry};
use egui::{Align2, Frame};

use crate::control::play::WindowControl;
use crate::control::{Listening, TokenFileGuard};

/// Native folder dialogs through `rfd` (the XDG portal on Linux). Called on a dialog thread.
pub struct RfdPicker;

impl Picker for RfdPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Add Folder").pick_folder()
    }
}

/// Which library the window opens.
pub enum Source {
    Library(PathBuf),
    /// The demo library in this folder (the persistent one or a fresh one), made on first use
    /// (`dalica_engine::demo`).
    Demo(PathBuf),
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
    _ui_thread: Option<dalica_fs::UiThread>,
}

impl Window {
    /// Starts opening the library on another thread; the window shows "Opening…" until it arrives.
    /// With `control`, starts accepting connections; returns the token file to remove at exit.
    pub fn new(ctx: &egui::Context, source: Source, control: Option<ControlSetup>) -> (Window, Option<TokenFileGuard>) {
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
        let app = App::with_picker(None, picker);
        app.install(ctx);
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
        (Window { app, opening: Some(rx), failed, control, perf, sample: None, _ui_thread: None }, token_file)
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
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
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
        let at = ui.ctx().input(|i| i.time);
        self.sample = Some(Sample { at, frame: layout, update: started.elapsed(), ..Sample::default() });
    }

    fn on_exit(&mut self) {
        // No more frames: this thread may write again (the flush below).
        self._ui_thread = None;
        if let Some(session) = self.app.session_mut()
            && let Err(e) = session.flush()
        {
            log::error!("dalica: not every change was saved: {e}");
        }
    }
}

impl Window {
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
            return;
        }
        self.app.frame(ui);
        if !self.app.idle() {
            // Scans, thumbnails and dialogs finish on other threads: keep polling while they run.
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }
}
