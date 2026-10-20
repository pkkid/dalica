//! Options (`menus-and-shortcuts.md` §5, `design-system.md` §11.3): Tools ▸ Options… (Ctrl+,;
//! Dalica ▸ Preferences… on macOS, `app.preferences`) opens it on General, Help ▸ Keyboard
//! Shortcuts… (Ctrl+/, `app.shortcuts`) on Keyboard Shortcuts. Its draft is the view state's
//! `options` ([`OptionsDraft`]): the tab, the option values changed since the last Apply, the last
//! Apply's error, the scale and font size previewed and the shortcut editor.
//!
//! ```text
//! Options
//! [General] [File Types] [Slideshow] [Network] [People] [Photo Viewer] [Export] [Keyboard Shortcuts]
//! ┌──────────────────────────────────────────────────┐
//! │ the tab's body                                   │
//! └──────────────────────────────────────────────────┘
//! (the last Apply's error: text:dialog.error)
//!                                  [OK] [Cancel] [Apply]
//! ```
//!
//! This module is the frame: the tabs (`tab:options.<tab>`), the body's fixed height (the dialog is
//! 720 × 540 points at 100 %, never larger than the window), the error line and the buttons. Each
//! tab's body is a module of its own ([`general`], [`file_types`], [`slideshow`], [`network`],
//! [`people`], [`viewer`], [`export`], [`shortcuts`]), so the tasks that draw them never share a
//! file.
//!
//! - **Apply** (`button:dialog.apply`, enabled while the draft changes something) sends the changed
//!   values in one `options.set {values}` and the shortcut editor's changes
//!   ([`shortcuts::apply`]); the dialog stays open. A failed Apply keeps the draft and shows its
//!   error here (`options.error`), and changes nothing more.
//! - **OK** (the default) applies what changed, then closes; a failed apply keeps it open.
//! - **Cancel** and Esc drop the draft: the changes since the last successful Apply and the preview
//!   of scale and font size.

pub mod export;
pub mod file_types;
pub mod general;
pub mod network;
pub mod people;
pub mod shortcuts;
pub mod slideshow;
pub mod viewer;

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target};
use egui::{RichText, Ui, vec2};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::{OptionsDraft, OptionsTab};
use crate::tl;

pub const TITLE: &str = "Options";
/// The dialog's size at 100 % (points); a smaller window gets a smaller one.
pub const WIDTH: f32 = 720.0;
pub const HEIGHT: f32 = 540.0;
/// What the frame, title, tab row, error line and button row take besides a tab's body, and the
/// room kept above and below the dialog (points).
const CHROME_H: f32 = 150.0;
const SCREEN_MARGIN: f32 = 32.0;
/// The shortest a tab's body gets (points).
const MIN_CONTENT_H: f32 = 120.0;

/// The tabs: their state value, widget id and label.
pub const TABS: [(OptionsTab, &str, &str); 8] = [
    (OptionsTab::General, "tab:options.general", "General"),
    (OptionsTab::FileTypes, "tab:options.fileTypes", "File Types"),
    (OptionsTab::Slideshow, "tab:options.slideshow", "Slideshow"),
    (OptionsTab::Network, "tab:options.network", "Network"),
    (OptionsTab::People, "tab:options.people", "People"),
    (OptionsTab::Viewer, "tab:options.viewer", "Photo Viewer"),
    (OptionsTab::Export, "tab:options.export", "Export"),
    (OptionsTab::Shortcuts, "tab:options.shortcuts", "Keyboard Shortcuts"),
];

/// A tab's label of [`TABS`] (or of the photo viewer's Settings) in the language in effect.
pub fn tab_label(english: &str) -> String {
    match english {
        "General" => tl!(ctx = "options tab", "General"),
        "File Types" => tl!(ctx = "options tab", "File Types"),
        "Slideshow" => tl!(ctx = "options tab", "Slideshow"),
        "Network" => tl!(ctx = "options tab", "Network"),
        "People" => tl!(ctx = "options tab", "People"),
        "Photo Viewer" => tl!(ctx = "options tab", "Photo Viewer"),
        "Export" => tl!(ctx = "options tab", "Export"),
        "Keyboard Shortcuts" => tl!(ctx = "options tab", "Keyboard Shortcuts"),
        "Interface" => tl!(ctx = "options tab", "Interface"),
        other => crate::i18n::tr(other),
    }
}

/// The last Apply's error line, while there is one (its text as the label).
pub const ERROR_ID: &str = "text:dialog.error";

/// The buttons' labels.
pub const OK: &str = "OK";
pub const APPLY: &str = "Apply";

/// Opens the dialog on `tab` with nothing changed. Replies `{dialog: "options"}`.
pub fn open(app: &mut App, tab: OptionsTab) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::Options))?;
    app.state_mut().options = Some(OptionsDraft::on(tab));
    Ok(reply)
}

/// The height of a tab's body on a screen `screen_h` points tall.
pub fn content_height(screen_h: f32) -> f32 {
    (HEIGHT - CHROME_H).min(screen_h - CHROME_H - SCREEN_MARGIN).max(MIN_CONTENT_H)
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let changed = app.state().options.as_ref().is_some_and(OptionsDraft::changed);
    Look {
        title: tl!("Options"),
        width: WIDTH,
        buttons: vec![
            Button::new("ok", tl!("OK"), ButtonKind::Default),
            Button::cancel(),
            Button::new("apply", tl!("Apply"), ButtonKind::Other).enabled(changed).stays_open(),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(tab) = app.state().options.as_ref().map(|o| o.tab) else { return };
    let t = *app.tokens();
    let mut chosen = tab;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tab, id, label) in TABS {
            if widgets::text_button(ui, &t, id, &tab_label(label), chosen == tab).clicked() {
                chosen = tab;
            }
        }
    });
    if chosen != tab
        && let Some(o) = app.state_mut().options.as_mut()
    {
        o.tab = chosen;
    }
    ui.add_space(12.0);
    let height = content_height(ui.ctx().content_rect().height());
    let width = ui.available_width();
    ui.allocate_ui(vec2(width, height), |ui| {
        ui.set_min_size(vec2(width, height));
        ui.set_max_height(height);
        match chosen {
            OptionsTab::General => general::body(app, ui),
            OptionsTab::FileTypes => file_types::body(app, ui),
            OptionsTab::Slideshow => slideshow::body(app, ui),
            OptionsTab::Network => network::body(app, ui),
            OptionsTab::People => people::body(app, ui),
            OptionsTab::Viewer => viewer::body(app, ui),
            OptionsTab::Export => export::body(app, ui),
            OptionsTab::Shortcuts => shortcuts::body(app, ui),
        }
    });
    // The engine's error, looked up whole by its English (one carrying values stays English).
    let error = app.state().options.as_ref().and_then(|o| o.error.as_deref().map(crate::i18n::tr)).unwrap_or_default();
    let r = ui.add(egui::Label::new(RichText::new(&error).font(fonts::regular(t.body)).color(t.danger)).truncate());
    if !error.is_empty() {
        widgets::register(ui.ctx(), ERROR_ID, Role::Other, &error, Target::Painted { rect: r.rect, clip: ui.clip_rect() }, false);
    }
}

/// Applies the draft: the changed values in one `options.set {values}`, then the shortcut editor's
/// changes. On success the draft holds nothing changed (the preview is now the saved value) and the
/// reply lists what was applied; on failure its error is kept in the draft and returned.
pub fn apply(app: &mut App) -> Result<Value, CmdError> {
    let draft = app.state().options.clone().ok_or_else(|| CmdError::Internal("Options has no draft".into()))?;
    let result = run(app, &draft);
    let Some(o) = app.state_mut().options.as_mut() else { return result };
    match &result {
        Ok(_) => {
            o.values.clear();
            o.shortcuts.changes.clear();
            o.shortcuts.reset_all = false;
            o.error = None;
            o.preview_scale = None;
            o.preview_font_size = None;
        }
        Err(e) => o.error = Some(e.to_string()),
    }
    people::applied(app, &draft.values, result.is_ok());
    result
}

fn run(app: &mut App, draft: &OptionsDraft) -> Result<Value, CmdError> {
    // Photo Viewer ▸ File Types is `viewer.setDefault`'s, not `options.set`'s (M15.24).
    let mut values = draft.values.clone();
    let file_types = viewer::apply(app, &mut values)?;
    let options = if values.is_empty() { Value::Null } else { app.run("options.set", &json!({ "values": values }))? };
    let shortcuts = shortcuts::apply(app, &draft.shortcuts)?;
    let mut reply = json!({ "options": options, "shortcuts": shortcuts });
    if let (false, Some(o)) = (file_types.is_null(), reply.as_object_mut()) {
        o.insert("fileTypes".into(), file_types);
    }
    Ok(reply)
}

/// Apply runs [`apply`] and keeps the dialog open; OK applies what changed and closes.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let changed = app.state().options.as_ref().is_some_and(OptionsDraft::changed);
    match action {
        "apply" => apply(app),
        "ok" if changed => apply(app),
        "ok" => Ok(json!({})),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs;
    use crate::state::ShortcutChange;

    #[test]
    fn apply_keeps_the_dialog_open_and_a_failed_apply_keeps_the_draft_with_its_error() {
        let mut app = App::new(None);
        assert_eq!(open(&mut app, OptionsTab::Export).unwrap(), json!({ "dialog": "options" }));
        let v = dialogs::inspect(&app);
        assert_eq!(v["buttons"], json!(["button:dialog.ok", "button:dialog.cancel", "button:dialog.apply"]));
        assert_eq!(v["default"], "button:dialog.ok");
        assert!(matches!(dialogs::answer(&mut app, Some("apply")), Err(CmdError::Disabled(_))), "nothing changed");
        // A change that cannot be applied (no library is open): the error stays in the draft.
        app.state_mut().options.as_mut().unwrap().values.insert("interface.animations".into(), json!(false));
        assert!(dialogs::answer(&mut app, Some("apply")).is_err());
        let o = app.state().options.clone().unwrap();
        assert!(o.error.is_some() && o.values.len() == 1, "{o:?}");
        assert!(dialogs::answer(&mut app, None).is_err(), "OK fails the same way and keeps the dialog");
        assert_eq!(app.state().dialogs.len(), 1);
        // Nothing changed: OK closes; Cancel drops the draft.
        app.state_mut().options.as_mut().unwrap().values.clear();
        assert_eq!(dialogs::answer(&mut app, None).unwrap(), json!({}));
        assert_eq!((app.state().dialogs.len(), app.state().options.is_none()), (0, true));
        open(&mut app, OptionsTab::General).unwrap();
        let change = ShortcutChange { command: "view.edit".into(), context: dalica_engine::Context::Library, slot: 0, key: None };
        app.state_mut().options.as_mut().unwrap().shortcuts.changes.push(change);
        assert!(app.state().options.as_ref().unwrap().changed());
        dialogs::cancel(&mut app).unwrap();
        assert!(app.state().options.is_none());
        assert_eq!(content_height(900.0), HEIGHT - CHROME_H);
        assert_eq!(content_height(500.0), 500.0 - CHROME_H - SCREEN_MARGIN);
        assert_eq!(content_height(100.0), MIN_CONTENT_H);
    }
}
