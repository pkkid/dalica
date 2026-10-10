//! `App`: the UI's half of the program. It owns the [`UiState`] and (once opened) the engine's
//! [`Session`]. Every action goes through [`App::run`]: UI-only commands ([`UI_COMMANDS`]) change
//! the view state here, every other id is forwarded to `Session::execute` (`architecture.md` §3).

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context, Session};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Frame};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::UiState;

/// A command that changes only the view state. Listed with the engine's commands so agents see one
/// catalogue.
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path, `""` if not in a menu.
    pub menu: &'static str,
    /// Default bindings (none yet: the keymap learns UI commands with the menus, M2).
    pub keys: &'static [(Context, &'static str)],
    /// Agent-readable parameter doc.
    pub params: &'static str,
    pub run: fn(&mut App, &Value) -> Result<Value, CmdError>,
}

pub const UI_COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "select.set",
        label: "Select",
        menu: "",
        keys: &[],
        params: r#"{"ids": [PhotoId], "mode": "replace" | "toggle" = "replace"}"#,
        run: select_set,
    },
    UiCommand { id: "select.none", label: "Select None", menu: "Edit/Select None", keys: &[], params: "{}", run: select_none },
];

fn parse<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, CmdError> {
    serde_json::from_value(params.clone()).map_err(|e| CmdError::BadParams(e.to_string()))
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    #[default]
    Replace,
    Toggle,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectSet {
    ids: Vec<PhotoId>,
    #[serde(default)]
    mode: Mode,
}

/// The most photos one selection command takes.
const MAX_IDS: usize = 1_000_000;

fn select_set(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SelectSet = parse(params)?;
    if p.ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} ids")));
    }
    let library = app.session.as_ref().map(Session::library);
    if let Some(missing) = p.ids.iter().find(|&&id| !library.is_some_and(|l| l.photo(id).is_some())) {
        return Err(CmdError::NotFound(format!("no photo {missing}")));
    }
    let sel = &mut app.state.selection;
    match p.mode {
        Mode::Replace => {
            sel.ids.clear();
            for id in &p.ids {
                if !sel.ids.contains(id) {
                    sel.ids.push(*id);
                }
            }
        }
        Mode::Toggle => {
            for id in &p.ids {
                match sel.ids.iter().position(|x| x == id) {
                    Some(i) => {
                        sel.ids.remove(i);
                    }
                    None => sel.ids.push(*id),
                }
            }
        }
    }
    sel.anchor = p.ids.last().copied().filter(|id| sel.ids.contains(id)).or_else(|| sel.ids.last().copied());
    Ok(json!({ "selection": sel.ids }))
}

fn select_none(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NoParams {}
    let _: NoParams = parse(params)?;
    app.state.selection = Default::default();
    Ok(json!({ "selection": [] }))
}

pub struct App {
    state: UiState,
    tokens: Tokens,
    session: Option<Session>,
}

impl App {
    /// An app showing `session`, or the "opening" state while it is `None`.
    pub fn new(session: Option<Session>) -> App {
        App { state: UiState::default(), tokens: Tokens::default(), session }
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
        if let Err(e) = &result {
            self.state.status = e.to_string();
        }
        result
    }

    /// Nothing is running in the background: no scan, no pending result to apply.
    pub fn idle(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.busy())
    }

    /// One frame: applies finished background work, then paints.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(s) = &mut self.session {
            s.poll();
        }
        let t = self.tokens;
        let opening = self.session.is_none();
        egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
            let rect = ui.max_rect();
            let p = ui.painter();
            let (title, body) = if opening { ("Opening the library…", "") } else { ("No folders yet", "Add a folder with File → Add Folder…") };
            p.text(rect.center() - egui::vec2(0.0, 12.0), Align2::CENTER_CENTER, title, fonts::semibold(18.0), t.text_label);
            p.text(rect.center() + egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, body, fonts::regular(t.body), t.text_dim);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn select_set_replaces_or_toggles_and_keeps_an_anchor() {
        let (_dir, s, ids) = session_with_photos(4);
        let mut app = App::new(Some(s));
        let n = |i: usize| ids[i].get();
        assert_eq!(app.run("select.set", &json!({ "ids": [n(0), n(1)] })).unwrap(), json!({ "selection": [n(0), n(1)] }));
        assert_eq!(app.state().selection.anchor, Some(ids[1]));
        app.run("select.set", &json!({ "ids": [n(1), n(2)], "mode": "toggle" })).unwrap();
        assert_eq!(app.state().selection.ids, [ids[0], ids[2]]);
        assert_eq!(app.state().selection.anchor, Some(ids[2]));
        // Toggling the anchor off moves it to the last selected photo.
        app.run("select.set", &json!({ "ids": [n(2)], "mode": "toggle" })).unwrap();
        assert_eq!(app.state().selection.anchor, Some(ids[0]));
        app.run("select.set", &json!({ "ids": [n(3), n(3)] })).unwrap();
        assert_eq!(app.state().selection.ids, [ids[3]], "duplicates collapse");
        app.run("select.none", &json!({})).unwrap();
        assert!(app.state().selection.ids.is_empty() && app.state().selection.anchor.is_none());
        // Engine commands are forwarded.
        let r = app.run("photo.star.toggle", &json!({ "ids": [n(0)] })).unwrap();
        assert!(r.to_string().contains("true"), "{r}");
    }

    #[test]
    fn hostile_params_and_unknown_ids_are_errors_never_panics() {
        let (_dir, s, ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        for (id, params) in [
            ("select.set", json!({})),
            ("select.set", json!({ "ids": "1" })),
            ("select.set", json!({ "ids": [-1] })),
            ("select.set", json!({ "ids": [1.5] })),
            ("select.set", json!({ "ids": [], "mode": "sideways" })),
            ("select.set", json!({ "ids": [], "extra": 1 })),
            ("select.set", json!([1, 2])),
            ("select.set", Value::Null),
            ("select.none", json!({ "all": true })),
            ("select.none", json!("x")),
        ] {
            assert!(matches!(app.run(id, &params), Err(CmdError::BadParams(_))), "{id} {params}");
        }
        assert!(matches!(app.run("select.set", &json!({ "ids": [ids[0].get() + 1000] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("select.set", &json!({ "ids": [u64::MAX] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("no.such.command", &json!({})), Err(CmdError::UnknownCommand(_))));
        assert!(app.state().selection.ids.is_empty(), "nothing changed");
        assert!(!app.state().status.is_empty(), "the last failure is on the status line");
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

    #[test]
    fn ui_commands_do_not_clash_with_engine_commands() {
        let engine = dalica_engine::cmd::specs();
        for c in UI_COMMANDS {
            assert!(!engine.iter().any(|s| s.id == c.id), "{} is both", c.id);
            assert!(UI_COMMANDS.iter().filter(|o| o.id == c.id).count() == 1);
        }
    }
}
