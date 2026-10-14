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
use crate::picker::{FolderRequest, Pending, Picker, SaveRequest};
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
    /// The Save As job its picker started, and a picker to open again (`crate::commands::save_as`).
    pub(crate) save_as: crate::commands::save_as::Following,
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
}

/// The persisted part of the view state: the window's (M3) and the library view's options (M4).
type Persisted = (ViewState, ViewOptions);

impl App {
    /// An app showing `session`, or the "opening" state while it is `None`.
    pub fn new(mut session: Option<Session>) -> App {
        if let Some(s) = &mut session {
            crate::keys::join_keymap(s);
        }
        App {
            state: UiState::default(),
            tokens: Tokens::default(),
            session,
            picker: None,
            panels: None,
            want_folder_picker: false,
            folder_dialog_open: false,
            save_dialog: None,
            folder_request: None,
            save_as: Default::default(),
            persist_view: false,
            saved_view: None,
            quit_requested: false,
            tooltip: None,
            notices: Notices::default(),
            now: 0.0,
            opening_detail: None,
            opening_since: None,
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
        self.session = Some(session);
        if self.persist_view {
            self.restore_view();
        }
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

    /// Runs command `id`: a UI-only one here, anything else in the engine. A command whose UI asks
    /// first, run without `"confirm": true` when its question applies, asks through the dialog host
    /// instead and replies `{dialog: <kind>}` (`crate::dialogs::asking`). A failure's message
    /// becomes the status line. In the edit view the rotations, the flips, Edit ▸ Undo / Redo and
    /// the one-click fixes are steps of the open photo's history (`commands::edit_view::route`).
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        if let Some((routed, p)) = crate::commands::edit_view::route(self, id, params) {
            return self.run(routed, &p);
        }
        if let Some(p) = crate::commands::save::route(self, id, params) {
            return self.run(id, &p);
        }
        let result = if let Some(ask) = crate::dialogs::asking(self, id, params) {
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
        result
    }

    /// Nothing is running in the background: no scan, no thumbnail on its way, no open dialog.
    pub fn idle(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.busy())
            && self.panels.as_ref().is_none_or(Panels::idle)
            && self.save_dialog.is_none()
            && self.folder_request.is_none()
    }

    /// Whether File ▸ Add Folder… can open the folder picker now; why not, if it cannot.
    pub fn can_pick_folder(&self) -> Result<(), String> {
        if self.session.is_none() {
            Err("The library is still opening.".into())
        } else if self.picker.is_none() {
            Err("There is no folder dialog here.".into())
        } else if self.want_folder_picker || self.panels.as_ref().map_or(self.folder_dialog_open, |p| p.dialog.is_some()) {
            Err("A folder dialog is already open.".into())
        } else if self.save_dialog.is_some() || self.folder_request.is_some() {
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

    /// The folder picker's request (not Add Folder's) while it is open.
    pub fn folder_request(&self) -> Option<&FolderRequest> {
        self.folder_request.as_ref().map(|(_, r)| r)
    }

    /// Runs the folder picker's answer once it has one.
    fn poll_folder_request(&mut self) {
        let Some(answer) = self.folder_request.as_ref().and_then(|(p, _)| p.poll()) else { return };
        if let Some((_, request)) = self.folder_request.take()
            && let Some(path) = answer
        {
            let _ = self.run(request.command, &request.params_with(&path));
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
        if std::mem::take(&mut self.quit_requested) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
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
        session.poll();
        let now = ui.input(|i| i.time);
        self.now = now;
        self.notices.poll(session, now);
        self.poll_save_dialog();
        crate::commands::save_as::follow(self);
        self.poll_folder_request();
        // Under an open dialog the window takes no keys: no shortcut runs and no panel sees a key
        // press or typed text; the dialog gets them back once the panels are drawn.
        let keys = if self.state.dialogs.is_empty() {
            // The edit view resolves keys in its own context, the open tool first.
            match self.state.view {
                crate::state::View::Library => crate::keys::dispatch(self, ui.ctx()),
                crate::state::View::Edit => crate::commands::edit_view::dispatch(self, ui.ctx()),
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
        let mut p = self.panels.take().unwrap_or_else(|| Panels::new(self.picker.clone()));
        if std::mem::take(&mut self.want_folder_picker) {
            p.add_folder();
        }
        self.folder_dialog_open = p.dialog.is_some();
        panels::show(&mut p, self, ui);
        self.panels = Some(p);
        if !keys.is_empty() {
            ui.ctx().input_mut(|i| i.events.extend(keys));
        }
        // Toasts at the bottom of the light table (the photo area in the edit view), over the panels
        // and under any dialog.
        let area = match self.state.view {
            crate::state::View::Library => panels::layout::compute(ui.max_rect(), &self.state).light_table,
            crate::state::View::Edit => panels::layout::compute_edit(ui.max_rect(), &self.state).toast_area(),
        };
        self.notices.show(ui, &self.tokens, area, now);
        crate::dialogs::show(self, ui.ctx());
        self.tooltip = tooltip_text(ui.ctx());
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
