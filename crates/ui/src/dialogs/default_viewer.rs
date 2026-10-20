//! The default-viewer question (`viewing.md` §3.3), asked once, after the first-run panel's
//! Continue ([`offer`]; `defaultViewer.ask` asks it any time): "Use Dalica as the default viewer
//! for these file types:" with a checkbox per type Dalica can open by default, showing the
//! application that opens it now ("JPEG (Image Viewer)", from the engine's `viewer.fileTypes`),
//! nothing chosen; [Use Dalica] (enabled once a type is chosen) runs the engine's
//! `viewer.setDefault {types}` (where only the user may choose, a toast says what to choose), [Not
//! Now] closes it and changes nothing. Its draft is the view state's `defaultViewer` (`{types}`),
//! set when it opens and cleared when it closes. A system that cannot make Dalica the default
//! viewer is never asked.
//!
//! Widget ids: `dialog:defaultViewer`, a type `check:defaultViewer.type/<type>` (`jpeg`, …),
//! `button:dialog.use`, `button:dialog.cancel`.

use dalica_engine::CmdError;
use dalica_engine::os::Service;
use dalica_ui_kit::widgets::{ButtonKind, checkbox};
use egui::Ui;
use serde_json::Value;

use super::options::viewer as part;
use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::DefaultViewerDraft;
use crate::tl;

pub const TITLE: &str = "Default Viewer";
pub const QUESTION: &str = "Use Dalica as the default viewer for these file types:";
/// The line under the types.
pub const HINT: &str = "You can change this at any time in Tools → Options → Photo Viewer.";
/// The engine's command that makes Dalica the default for the types chosen.
pub const SET_DEFAULT: &str = part::SET_DEFAULT;
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// The id of a type's checkbox.
pub fn type_id(ty: &str) -> String {
    format!("check:defaultViewer.type/{ty}")
}

/// Opens the question with nothing chosen. Replies `{dialog: "defaultViewer"}`.
pub fn open(app: &mut App) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::DefaultViewer))?;
    app.state_mut().default_viewer = Some(DefaultViewerDraft::default());
    Ok(reply)
}

/// After the first run: asks the question when the system can make Dalica the default viewer;
/// whether it opened.
pub fn offer(app: &mut App) -> bool {
    let can = app.session().is_some_and(|s| s.os().available(Service::DefaultViewer).is_ok());
    can && open(app).is_ok()
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let chosen = app.state().default_viewer.as_ref().is_some_and(|d| !d.types.is_empty());
    Look {
        title: tl!("Default Viewer"),
        width: WIDTH,
        buttons: vec![
            Button::new("use", tl!("Use Dalica"), ButtonKind::Default).enabled(chosen),
            Button::new("cancel", tl!("Not Now"), ButtonKind::Cancel),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let listing = part::listing(app, ui.ctx());
    let t = *app.tokens();
    let mut chosen = app.state().default_viewer.as_ref().map(|d| d.types.clone()).unwrap_or_default();
    super::text_lines(app, ui, &[tl!("Use Dalica as the default viewer for these file types:")]);
    ui.add_space(6.0);
    if let Some(reason) = &listing.reason {
        super::text_lines(app, ui, &[crate::i18n::tr(reason)]);
    }
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for ty in &listing.types {
            let mut on = chosen.contains(&ty.id);
            if checkbox(ui, &t, &type_id(&ty.id), &ty.label(), &mut on).clicked() {
                chosen.retain(|c| *c != ty.id);
                if on {
                    chosen.push(ty.id.clone());
                }
                changed = true;
            }
        }
    });
    ui.add_space(6.0);
    super::text_lines(app, ui, &[tl!("You can change this at any time in Tools → Options → Photo Viewer.")]);
    if changed && let Some(d) = app.state_mut().default_viewer.as_mut() {
        d.types = listing.ordered(&chosen);
    }
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "use" => {
            let types = app.state().default_viewer.as_ref().map(|d| d.types.clone()).unwrap_or_default();
            part::set_default(app, &types)
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
