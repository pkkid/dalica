//! The dialog host (`design-system.md` §6 `dialog`, `control-protocol.md` §6.6, §6.10): every
//! in-app dialog is data in [`UiState::dialogs`](crate::state::UiState::dialogs), a stack whose last
//! entry is on top, drawn each frame from it with the `ui-kit` [`dialog`](widgets::dialog) frame
//! (the dimmed backdrop that takes the clicks, `dialog:<kind>`, `button:dialog.<action>`). Only the
//! top dialog takes input: Esc chooses its Cancel button and Enter its default one, unless a field
//! keeps the focus.
//!
//! - **Opening**: [`open`] pushes a dialog (one of each kind at a time, at most
//!   [`MAX_DIALOGS`]). Commands open theirs: `folders.manager` (Tools ▸ Folder Manager…), M6's
//!   `section.properties`, `photo.adjustDate`, `quicktags.configure`, `tag.rename`, `app.about` and
//!   `help.releaseNotes` (`crate::commands::{info, help}`), the Crop tool's Add Custom Ratio…
//!   ([`custom_ratio`], M7), and every command of [`confirm::ASKS`]
//!   run without `"confirm": true` when its question applies ([`asking`], called by
//!   [`App::run`](crate::App::run)).
//! - **Answering**: [`answer`] presses a button of the top dialog (none named: its default one),
//!   the same function a click, Enter and `ui.dialog.confirm` call; [`cancel`] closes it and runs
//!   nothing (its Cancel button, Esc, `ui.dialog.cancel`). A dialog closes when its action
//!   succeeds and stays open, the reason on the status line, when it fails.
//! - **Reporting**: [`inspect`] is `ui.inspect` → `dialog`: the top dialog's kind, title, draft,
//!   buttons and default button.
//! - **Stacking**: a question asked over a dialog (Watch Drive over the Folder Manager) is drawn
//!   on top; the dialogs under it take no input, and their buttons are registered as parts of
//!   their own id (`dialog:folderManager/ok`), so `button:dialog.<action>` always names a button
//!   of the top dialog and every id stays unique.
//! - **First run**: the window offers the first-run panel when it opens a new library
//!   ([`first_run::offer`]).
//!
//! Each kind lives in a module of its own with the same three functions, which the host
//! dispatches to: `look` (title, width, buttons), `body` (draws the draft and edits it in place)
//! and `act` (what a button other than Cancel does). A new kind adds its [`DialogKind`] variant,
//! its module and one arm in each dispatch below.
//!
//! The pattern (dialogs as data in the UI state, drawn from it each frame, confirmed and cancelled
//! by the same functions for the buttons and for automation) is adapted from PhotoCraft
//! `crates/ui-egui/src/dialogs.rs` @ 82c58fab (MIT OR Apache-2.0; `NOTICE`).

pub mod about;
pub mod adjust_date;
pub mod confirm;
pub mod custom_ratio;
pub mod first_run;
pub mod folder_manager;
pub mod folder_properties;
pub mod quick_tags;
pub mod release_notes;
pub mod rename_tag;

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::{self, ButtonKind, DialogButton};
use egui::Ui;
use serde_json::{Value, json};

use crate::app::App;
pub use crate::state::{Dialog, DialogCommand, DialogKind, MAX_DIALOGS};

/// One button of a dialog: `button:dialog.<action>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    /// `ok`, `cancel`, `clearTray`, …: the widget is `button:dialog.<action>`.
    pub action: &'static str,
    pub label: String,
    pub kind: ButtonKind,
    pub enabled: bool,
}

impl Button {
    pub fn new(action: &'static str, label: impl Into<String>, kind: ButtonKind) -> Button {
        Button { action, label: label.into(), kind, enabled: true }
    }

    /// Cancel: closes the dialog, runs nothing.
    pub fn cancel() -> Button {
        Button::new("cancel", "Cancel", ButtonKind::Cancel)
    }

    /// Close, the only button of a dialog that edits nothing (About, Release Notes): it is the
    /// Cancel button under another label, so Esc and `ui.dialog.cancel` close the dialog too.
    pub fn close() -> Button {
        Button::new("cancel", "Close", ButtonKind::Cancel)
    }

    pub fn enabled(self, enabled: bool) -> Button {
        Button { enabled, ..self }
    }

    /// Its automation id.
    pub fn widget(&self) -> String {
        format!("button:dialog.{}", self.action)
    }
}

/// How a dialog looks now: its title, width (points) and buttons, in the order the frame lays them
/// out (the last is rightmost).
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub title: String,
    pub width: f32,
    pub buttons: Vec<Button>,
}

/// The automation id of a dialog of `kind`.
pub fn widget_id(kind: DialogKind) -> String {
    format!("dialog:{}", kind.name())
}

fn look(app: &App, d: &Dialog) -> Look {
    match d.kind {
        DialogKind::ClearTray | DialogKind::RemoveFolder | DialogKind::WatchDrive => confirm::look(d),
        DialogKind::FolderManager => folder_manager::look(app, d),
        DialogKind::FirstRun => first_run::look(app, d),
        DialogKind::FolderProperties => folder_properties::look(app, d),
        DialogKind::AdjustDate => adjust_date::look(app, d),
        DialogKind::QuickTags => quick_tags::look(app, d),
        DialogKind::RenameTag => rename_tag::look(app, d),
        DialogKind::About => about::look(app, d),
        DialogKind::ReleaseNotes => release_notes::look(app, d),
        DialogKind::CustomRatio => custom_ratio::look(app, d),
    }
}

fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    match d.kind {
        DialogKind::ClearTray | DialogKind::RemoveFolder | DialogKind::WatchDrive => confirm::body(app, ui, d),
        DialogKind::FolderManager => folder_manager::body(app, ui, d),
        DialogKind::FirstRun => first_run::body(app, ui, d),
        DialogKind::FolderProperties => folder_properties::body(app, ui, d),
        DialogKind::AdjustDate => adjust_date::body(app, ui, d),
        DialogKind::QuickTags => quick_tags::body(app, ui, d),
        DialogKind::RenameTag => rename_tag::body(app, ui, d),
        DialogKind::About => about::body(app, ui, d),
        DialogKind::ReleaseNotes => release_notes::body(app, ui, d),
        DialogKind::CustomRatio => custom_ratio::body(app, ui, d),
    }
}

fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match d.kind {
        DialogKind::ClearTray | DialogKind::RemoveFolder | DialogKind::WatchDrive => confirm::act(app, d, action),
        DialogKind::FolderManager => folder_manager::act(app, d, action),
        DialogKind::FirstRun => first_run::act(app, d, action),
        DialogKind::FolderProperties => folder_properties::act(app, d, action),
        DialogKind::AdjustDate => adjust_date::act(app, d, action),
        DialogKind::QuickTags => quick_tags::act(app, d, action),
        DialogKind::RenameTag => rename_tag::act(app, d, action),
        DialogKind::About => about::act(app, d, action),
        DialogKind::ReleaseNotes => release_notes::act(app, d, action),
        DialogKind::CustomRatio => custom_ratio::act(app, d, action),
    }
}

/// The longest value [`draft_lines`] shows, in characters.
const LINE_CHARS: usize = 120;

/// Draws the draft fields `keys` of `d` as read-only lines `key: value` (strings as they are,
/// other values as JSON, each cut to one line): the body of a dialog whose own layout arrives
/// later, so its draft is visible on screen and in screenshots.
pub fn draft_lines(app: &App, ui: &mut Ui, d: &Dialog, keys: &[&str]) {
    let t = *app.tokens();
    for key in keys {
        let value = match d.fields.get(*key) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        };
        let mut line: String = format!("{key}: {value}").chars().take(LINE_CHARS).collect();
        line.retain(|c| !c.is_control());
        ui.label(egui::RichText::new(line).font(dalica_ui_kit::fonts::regular(t.body)).color(t.text_label));
    }
}

/// The top dialog, the one that takes input.
pub fn top(app: &App) -> Option<&Dialog> {
    app.state().dialogs.last()
}

/// Whether a dialog of `kind` can open now; why not, if it cannot.
pub fn can_open(app: &App, kind: DialogKind) -> Result<(), String> {
    let open = &app.state().dialogs;
    if open.iter().any(|d| d.kind == kind) {
        Err(format!("The {} dialog is already open.", kind.name()))
    } else if open.len() >= MAX_DIALOGS {
        Err(format!("At most {MAX_DIALOGS} dialogs are open at once."))
    } else {
        Ok(())
    }
}

/// Opens `dialog` on top of the others. Replies `{dialog: <kind>}`.
pub fn open(app: &mut App, dialog: Dialog) -> Result<Value, CmdError> {
    can_open(app, dialog.kind).map_err(CmdError::Disabled)?;
    let kind = dialog.kind;
    app.state_mut().dialogs.push(dialog);
    Ok(json!({ "dialog": kind.name() }))
}

/// Takes the dialog of `kind` off the stack.
fn close(app: &mut App, kind: DialogKind) -> Option<Dialog> {
    let dialogs = &mut app.state_mut().dialogs;
    let at = dialogs.iter().position(|d| d.kind == kind)?;
    Some(dialogs.remove(at))
}

/// Closes the top dialog without running anything: Cancel, Esc, `ui.dialog.cancel`. Replies `{}`.
pub fn cancel(app: &mut App) -> Result<Value, CmdError> {
    let kind = top(app).map(|d| d.kind).ok_or_else(|| CmdError::NotFound("no dialog is open".into()))?;
    close(app, kind);
    Ok(json!({}))
}

/// Presses button `action` of the top dialog (`None`: its default button), as a click does. Cancel
/// closes it ([`cancel`]); another button runs the dialog's action, and the dialog closes when that
/// succeeds (its result is the reply) and stays open when it fails.
pub fn answer(app: &mut App, action: Option<&str>) -> Result<Value, CmdError> {
    let d = top(app).cloned().ok_or_else(|| CmdError::NotFound("no dialog is open".into()))?;
    let look = look(app, &d);
    let button = match action {
        None => look
            .buttons
            .iter()
            .find(|b| b.kind == ButtonKind::Default)
            .ok_or_else(|| CmdError::Disabled(format!("the {} dialog has no default button", d.kind.name())))?,
        Some(a) => look.buttons.iter().find(|b| b.action == a).ok_or_else(|| {
            let have: Vec<String> = look.buttons.iter().map(|b| format!("dialog.{}", b.action)).collect();
            CmdError::NotFound(format!("the {} dialog has no button `dialog.{a}`; it has {}", d.kind.name(), have.join(", ")))
        })?,
    };
    if !button.enabled {
        return Err(CmdError::Disabled(format!("`{}` is disabled", button.label)));
    }
    if button.kind == ButtonKind::Cancel {
        return cancel(app);
    }
    let result = act(app, &d, button.action);
    if result.is_ok() {
        close(app, d.kind);
    }
    result
}

/// `ui.inspect` → `dialog`: the top dialog's `{kind, widget, title, fields, buttons, default,
/// command?}` (`buttons` and `default` as widget ids), `null` when none is open.
pub fn inspect(app: &App) -> Value {
    let Some(d) = top(app) else { return Value::Null };
    let look = look(app, d);
    let default = look.buttons.iter().find(|b| b.kind == ButtonKind::Default).map(Button::widget);
    let mut v = json!({
        "kind": d.kind.name(),
        "widget": widget_id(d.kind),
        "title": look.title,
        "fields": d.fields,
        "buttons": look.buttons.iter().map(Button::widget).collect::<Vec<_>>(),
        "default": default,
    });
    if let (Some(c), Some(o)) = (&d.command, v.as_object_mut()) {
        o.insert("command".into(), json!({ "id": c.id, "params": c.params }));
    }
    v
}

/// The confirmation running `command` with `params` asks for first: a row of [`confirm::ASKS`]
/// whose question applies, when `confirm` is absent or `false` (`true` runs the command; any other
/// value is left to the command to refuse).
pub fn asking(app: &App, command: &str, params: &Value) -> Option<&'static confirm::Ask> {
    let ask = confirm::ASKS.iter().find(|a| a.command == command)?;
    if !matches!(params.get("confirm"), None | Some(Value::Bool(false))) {
        return None;
    }
    (ask.asks)(app, params).then_some(ask)
}

/// Asks through the host: opens the confirmation `ask` for running its command with `params`.
pub fn ask(app: &mut App, ask: &confirm::Ask, params: &Value) -> Result<Value, CmdError> {
    open(app, confirm::dialog(ask, params))
}

/// Draws every open dialog, bottom first; the top one takes input. Call once per frame, after the
/// panels (the backdrop covers them).
pub fn show(app: &mut App, ctx: &egui::Context) {
    let count = app.state().dialogs.len();
    let t = *app.tokens();
    for i in 0..count {
        let Some(mut d) = app.state().dialogs.get(i).cloned() else { break };
        let look = look(app, &d);
        // The top dialog's buttons are `button:dialog.<action>`; a dialog under another one keeps
        // its buttons as parts of its own id, so ids stay unique while a question is asked over it.
        let top = i + 1 == count;
        let ids: Vec<String> = look.buttons.iter().map(|b| if top { b.widget() } else { format!("{}/{}", widget_id(d.kind), b.action) }).collect();
        let buttons: Vec<DialogButton<'_>> =
            look.buttons.iter().zip(&ids).map(|(b, id)| DialogButton { id, label: &b.label, kind: b.kind, enabled: b.enabled }).collect();
        let r = widgets::dialog(ctx, &t, &widget_id(d.kind), &look.title, look.width, &buttons, |ui| body(app, ui, &mut d));
        // The draft as the body left it.
        if let Some(slot) = app.state_mut().dialogs.get_mut(i).filter(|s| s.kind == d.kind) {
            *slot = d;
        }
        let pressed = r.clicked.and_then(|k| look.buttons.get(k));
        if top
            && let Some(b) = pressed
            && let Err(e) = answer(app, Some(b.action))
        {
            app.state_mut().status = e.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn dialogs_open_once_per_kind_answer_and_cancel() {
        let (_dir, s, _ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(matches!(cancel(&mut app), Err(CmdError::NotFound(_))));
        assert!(matches!(answer(&mut app, None), Err(CmdError::NotFound(_))));
        assert_eq!(inspect(&app), Value::Null);
        assert_eq!(open(&mut app, Dialog::new(DialogKind::FolderManager)).unwrap(), json!({ "dialog": "folderManager" }));
        assert!(matches!(open(&mut app, Dialog::new(DialogKind::FolderManager)), Err(CmdError::Disabled(_))), "one of each kind");
        open(&mut app, Dialog::new(DialogKind::FirstRun)).unwrap();
        let v = inspect(&app);
        assert_eq!(
            (v["kind"].as_str(), v["widget"].as_str(), v["default"].as_str()),
            (Some("firstRun"), Some("dialog:firstRun"), Some("button:dialog.continue"))
        );
        // Unknown and disabled buttons are refused; the top one answers first.
        assert!(matches!(answer(&mut app, Some("nope")), Err(CmdError::NotFound(m)) if m.contains("dialog.continue")));
        // "Choose folders…" with the Folder Manager already open only closes the panel.
        app.state_mut().dialogs.last_mut().unwrap().fields.insert("choice".into(), json!("choose"));
        assert_eq!(answer(&mut app, None).unwrap(), json!({ "dialog": "folderManager" }));
        assert_eq!(top(&app).map(|d| d.kind), Some(DialogKind::FolderManager));
        assert!(matches!(answer(&mut app, Some("help")), Err(CmdError::Disabled(_))));
        assert_eq!(app.state().dialogs.len(), 1, "a refused answer keeps the dialog");
        assert_eq!(answer(&mut app, Some("cancel")).unwrap(), json!({}));
        assert!(app.state().dialogs.is_empty());
        // At most MAX_DIALOGS.
        app.state_mut().dialogs = (0..MAX_DIALOGS).map(|_| Dialog::new(DialogKind::ClearTray)).collect();
        assert!(matches!(open(&mut app, Dialog::new(DialogKind::FirstRun)), Err(CmdError::Disabled(m)) if m.contains("At most")));
    }
}
