//! The default-viewer question (`viewing.md` §3.3), a stub until M15.24 asks it after the first run
//! and makes it act: "Use Dalica as the default viewer for these file types:" with a checkbox per
//! type Dalica reads, showing the application that opens it now ("JPEG (Image Viewer)"), nothing
//! chosen; [Use Dalica] (enabled once a type is chosen) runs the engine's `viewer.setDefault
//! {types}`, [Not Now] closes it. Its draft is the view state's `defaultViewer` (`{types}`), set
//! when it opens (`defaultViewer.ask`) and cleared when it closes. Until M15.24 the body names
//! the types chosen and Use Dalica says what it waits for.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::DefaultViewerDraft;

pub const TITLE: &str = "Default Viewer";
pub const QUESTION: &str = "Use Dalica as the default viewer for these file types:";
/// The engine's command that makes Dalica the default for the types chosen (M15.24).
pub const SET_DEFAULT: &str = "viewer.setDefault";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// Opens the question with nothing chosen. Replies `{dialog: "defaultViewer"}`.
pub fn open(app: &mut App) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::DefaultViewer))?;
    app.state_mut().default_viewer = Some(DefaultViewerDraft::default());
    Ok(reply)
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let chosen = app.state().default_viewer.as_ref().is_some_and(|d| !d.types.is_empty());
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("use", "Use Dalica", ButtonKind::Default).enabled(chosen), Button::new("cancel", "Not Now", ButtonKind::Cancel)],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let types = app.state().default_viewer.as_ref().map(|d| d.types.join(", ")).unwrap_or_default();
    let chosen = if types.is_empty() { "Chosen: none".to_string() } else { format!("Chosen: {types}") };
    super::text_lines(app, ui, &[QUESTION.to_string(), chosen]);
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "use" => {
            let types = app.state().default_viewer.as_ref().map(|d| d.types.clone()).unwrap_or_default();
            if app.session().and_then(|s| s.spec(SET_DEFAULT)).is_none() {
                return Err(CmdError::Disabled(crate::commands::os::LATER.into()));
            }
            app.run(SET_DEFAULT, &json!({ "types": types }))
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
