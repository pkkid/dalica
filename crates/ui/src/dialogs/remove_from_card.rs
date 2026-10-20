//! After an import with "Delete copied files from the card" (`organizing.md` §3): "Remove the
//! imported files from the card? This cannot be undone." [Remove Imported Files] [Keep Files].
//!
//! The Import room opens it once the import's copying is over and verified
//! (`crate::panels::import::progress`, [`crate::commands::import::finished`]); its draft
//! (`dialogs[].fields`) is `count`, how many files the import copied. **Remove Imported Files** runs
//! `import.removeFromSource {confirm: true}` for the last import: the engine removes only the files
//! that import copied and verified, each re-checked unchanged, and nothing else (`security.md` §5.7);
//! it runs as a job whose end is a toast ("Removed 6 pictures from the card"). **Keep Files** is the
//! Cancel button (Esc): nothing leaves the card. The dialog has no default button, so Enter
//! removes nothing.
//!
//! Nothing is ever removed without this question: After copying "delete" removes nothing by itself.
//! Automation never removes: `engine.execute` refuses `import.removeFromSource`
//! (`automation::NEVER_AUTOMATED`), and the button refuses while a control request drives the app
//! (`crate::control::refuse`: `ui.dialog.confirm`, a synthetic click or key); only the person's own
//! click removes.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::tl;

pub const TITLE: &str = "Remove from Card";
/// The question.
pub const TEXT: &str = "Remove the imported files from the card? This cannot be undone.";
/// The button that removes them, and the one that keeps them (the Cancel button).
pub const REMOVE: &str = "Remove Imported Files";
pub const KEEP: &str = "Keep Files";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// Remove Imported Files' action (`button:dialog.remove`) and the command it runs.
pub const REMOVE_ACTION: &str = "remove";
pub const COMMAND: &str = "import.removeFromSource";

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::RemoveFromCard)
}

/// Asks about the `count` files the last import copied. Replies `{dialog: "removeFromCard"}`.
pub fn open(app: &mut App, count: u64) -> Result<Value, CmdError> {
    let mut d = dialog();
    d.fields.insert("count".into(), json!(count));
    super::open(app, d)
}

/// Whether the question is open.
pub fn is_open(app: &App) -> bool {
    app.state().dialogs.iter().any(|d| d.kind == DialogKind::RemoveFromCard)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: tl!("Remove from Card"),
        width: WIDTH,
        buttons: vec![
            Button::new("remove", tl!("Remove Imported Files"), ButtonKind::Other),
            Button::new("cancel", tl!("Keep Files"), ButtonKind::Cancel),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(
        Label::new(RichText::new(tl!("Remove the imported files from the card? This cannot be undone.")).font(fonts::regular(t.body)).color(t.text))
            .wrap(),
    );
}

/// Remove Imported Files: `import.removeFromSource` confirmed, for the last import.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        REMOVE_ACTION => {
            let params = json!({ "confirm": true });
            crate::control::refuse(COMMAND, &params, &[])?;
            app.run(COMMAND, &params)
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
