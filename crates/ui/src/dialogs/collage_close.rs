//! The close question of the Collage room (`creating.md` §3): closing the room's tab (its × or
//! Close) or the window with unsaved changes asks "Save the changes to this collage?" [Save Draft]
//! [Discard] [Cancel]. What closes is the view state's `collage.close` (`tab` or `window`), set
//! when it opens and cleared when it closes. Save Draft starts the engine's draft
//! (`collage.saveDraft`, a job) and closes what was closing once the draft is written
//! (`crate::commands::collage::save_draft`; a draft that fails keeps the room open and says why);
//! while the engine cannot write drafts it is refused "Not available yet." and the question stays.
//! Discard closes without saving (`collage.closeRoom {discard: true}`); Cancel keeps the room
//! open.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::Value;

use super::{Button, Dialog, Look};
use crate::app::App;
use crate::state::CollageClose;

pub const TITLE: &str = "Close Collage";
pub const QUESTION: &str = "Save the changes to this collage?";
/// The dialog's width (points).
pub const WIDTH: f32 = 420.0;

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![
            Button::new("saveDraft", "Save Draft", ButtonKind::Default),
            Button::new("discard", "Discard", ButtonKind::Other),
            Button::cancel(),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    super::text_lines(app, ui, &[QUESTION.to_string()]);
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let closing = app.state().collage.close.unwrap_or(CollageClose::Tab);
    match action {
        "saveDraft" => crate::commands::collage::save_draft(app, Some(closing)),
        "discard" => crate::commands::collage::close(app, true, closing),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
