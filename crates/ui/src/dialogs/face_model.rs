//! The face model dialog (`faces.md` §3.1): for each model file its licence, whether commercial use
//! is allowed, what it was trained on (or that this is not known), its size and the site it comes
//! from; **Download** stays disabled until "I have read these terms and accept them for my own use"
//! is ticked, then shows the download's progress with Cancel; **Open page** and **Add a model
//! file…** (an `.onnx` dropped on the window comes here too) install a local file after the same
//! licence. Offered by Options ▸ People's Download…, by turning on Find faces in my photos without
//! a model and by the first open of People.
//!
//! Its draft is the view state's `faceModel` ([`FaceModelDraft`]): the model offered or the local
//! file, the box ticked and the progress, set when it opens and cleared when it closes.
//!
//! M13.4's stub: its title, buttons and draft are final, and Download is enabled only once the box
//! is ticked; the body shows the draft as text, and Download says what it waits for. M13.27 draws
//! it and makes it act (`faces.models.download {id, acknowledged: true}`, `faces.models.install
//! {path, acknowledged: true}`).

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::FaceModelDraft;

pub const TITLE: &str = "Face Model";
/// The licence box's text.
pub const ACCEPT: &str = "I have read these terms and accept them for my own use";
/// The dialog's width (points).
pub const WIDTH: f32 = 520.0;
/// Why Download does not act yet.
pub const NOT_YET: &str = "Arrives with face models.";

/// Opens the dialog with `draft`. Replies `{dialog: "faceModel"}`.
pub fn open(app: &mut App, draft: FaceModelDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::FaceModel))?;
    app.state_mut().face_model = Some(draft);
    Ok(reply)
}

/// Download is enabled once the licence box is ticked, and not while a download runs.
pub fn look(app: &App, _d: &Dialog) -> Look {
    let draft = app.state().face_model.as_ref();
    let ready = draft.is_some_and(|f| f.accepted && f.progress.is_none());
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("download", "Download", ButtonKind::Default).enabled(ready), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let draft = app.state().face_model.clone().unwrap_or_default();
    let lines = [
        format!("entry: {}", json!(draft.entry)),
        format!("file: {}", json!(draft.file)),
        format!("{ACCEPT}: {}", draft.accepted),
        format!("progress: {}", json!(draft.progress)),
    ];
    super::text_lines(app, ui, &lines);
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "download" => Err(CmdError::Disabled(NOT_YET.into())),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
