//! The Folder Manager (`organizing.md` §2; Tools ▸ Folder Manager…, `folders.manager`).
//!
//! **A stub on the dialog host until M5.18**, which fills this module (and `folder_manager/`)
//! without touching the host: the folder tree with its roots loaded lazily through
//! `folders.browse`, the Scan Always / Scan Once / Remove radios and their state icons, the faces
//! toggle, the watched list, the drive and Remove Folder confirmations, and OK applying the draft
//! through `folders.set` as one undo step. The seams it keeps:
//!
//! - [`look`]: the title, the width (≈ 640) and the buttons Help (disabled until there is help),
//!   Cancel and OK, the default;
//! - [`body`]: draws the dialog from its draft ([`Dialog::fields`], edited in place: the host
//!   keeps what the body leaves there) and must never block a frame (listings come from
//!   commands that answer across frames);
//! - [`act`]: what OK does (Cancel never reaches it: it closes the dialog and changes nothing).
//!
//! Until then the dialog lists the watched folders and OK changes nothing.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, Look};
use crate::app::App;

/// The dialog's width (points, `organizing.md` §2: ≈ 640 × 480).
pub const WIDTH: f32 = 640.0;

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: "Folder Manager".into(),
        width: WIDTH,
        buttons: vec![Button::new("help", "Help", ButtonKind::Other).enabled(false), Button::cancel(), Button::new("ok", "OK", ButtonKind::Default)],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.label(RichText::new("Watched folders").font(fonts::semibold(t.body)).color(t.text));
    ui.add_space(4.0);
    let roots = app.session().map(|s| s.roots().to_vec()).unwrap_or_default();
    if roots.is_empty() {
        ui.label(RichText::new("No folders are watched.").font(fonts::regular(t.body)).color(t.text_dim));
    }
    for root in roots.iter().take(64) {
        ui.label(RichText::new(root.display().to_string()).font(fonts::regular(t.body)).color(t.text_label));
    }
}

/// OK: nothing to apply yet (M5.18 runs `folders.set` with the draft).
pub fn act(_app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    Ok(json!({}))
}
