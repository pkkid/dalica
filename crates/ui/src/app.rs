//! `App`: the UI's half of the program. It owns the [`UiState`] and (once opened) the engine's
//! [`Session`]. Every action goes through [`App::run`]: UI-only commands ([`UI_COMMANDS`]) change
//! the view state here, every other id is forwarded to `Session::execute` (`architecture.md` §3).

use dalica_engine::{CmdError, Session};
use std::sync::Arc;

use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Frame};
use serde_json::Value;

pub use crate::menus::{UI_COMMANDS, UiCommand};
use crate::panels::{self, Panels};
use crate::picker::Picker;
use crate::state::UiState;

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
}

impl App {
    /// An app showing `session`, or the "opening" state while it is `None`.
    pub fn new(session: Option<Session>) -> App {
        App { state: UiState::default(), tokens: Tokens::default(), session, picker: None, panels: None, want_folder_picker: false }
    }

    /// The same, with the dialogs of `picker` (File ▸ Add Folder…).
    pub fn with_picker(session: Option<Session>, picker: Arc<dyn Picker>) -> App {
        App { picker: Some(picker), ..App::new(session) }
    }

    /// Installs fonts and the theme; call once before the first frame.
    pub fn install(&self, ctx: &egui::Context) {
        self.tokens.apply(ctx);
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
    pub fn set_session(&mut self, session: Session) {
        self.session = Some(session);
    }

    /// Takes the session out, to flush and close it.
    pub fn take_session(&mut self) -> Option<Session> {
        self.session.take()
    }

    /// Runs command `id`: a UI-only one here, anything else in the engine. A failure's message
    /// becomes the status line.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        let result = match UI_COMMANDS.iter().find(|c| c.id == id) {
            Some(c) => (c.run)(self, params),
            None => match &mut self.session {
                Some(s) => s.execute(id, params),
                None if dalica_engine::cmd::specs().iter().any(|s| s.id == id) => Err(CmdError::Disabled("The library is still opening.".into())),
                None => Err(CmdError::UnknownCommand(id.into())),
            },
        };
        match &result {
            Ok(_) => self.state.status.clear(),
            Err(e) => self.state.status = e.to_string(),
        }
        result
    }

    /// Nothing is running in the background: no scan, no thumbnail on its way, no open dialog.
    pub fn idle(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.busy()) && self.panels.as_ref().is_none_or(Panels::idle)
    }

    /// Whether File ▸ Add Folder… can open the folder picker now; why not, if it cannot.
    pub fn can_pick_folder(&self) -> Result<(), String> {
        if self.session.is_none() {
            Err("The library is still opening.".into())
        } else if self.picker.is_none() {
            Err("There is no folder dialog here.".into())
        } else if self.want_folder_picker || self.panels.as_ref().is_some_and(|p| p.dialog.is_some()) {
            Err("A folder dialog is already open.".into())
        } else {
            Ok(())
        }
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

    /// The panels' state, once the first frame with a library has run.
    pub fn panels(&self) -> Option<&Panels> {
        self.panels.as_ref()
    }

    /// One frame: applies finished background work, runs this frame's shortcuts, then paints.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let Some(session) = &mut self.session else {
            let t = self.tokens;
            egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
                let rect = ui.max_rect();
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Opening the library…", fonts::semibold(18.0), t.text_label);
            });
            return;
        };
        session.poll();
        crate::keys::dispatch(self, ui.ctx());
        let mut p = self.panels.take().unwrap_or_else(|| Panels::new(self.picker.clone()));
        if std::mem::take(&mut self.want_folder_picker) {
            p.add_folder();
        }
        panels::show(&mut p, self, ui);
        self.panels = Some(p);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dalica_core::PhotoId;
    use serde_json::json;
    use std::sync::Arc;

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
