//! The standalone photo viewer (`viewing.md` §3): the photo over a dark translucent overlay (opaque
//! black when chosen or where the platform cannot), its name and size, the filmstrip of its folder
//! and the control bar.
//!
//! M11.2's stub: the window on the viewer's background (`viewer_overlay`, or `viewer_bg` when
//! opaque) with a line naming the photo's place in its folder. M11.17 draws the viewer (in modules
//! under `viewer/`).

use dalica_ui_kit::{fonts, widgets};
use egui::{Align2, Ui};

use crate::app::App;
use crate::state::Backdrop;

/// Draws the viewer over `ui`'s whole rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let Some(v) = app.state().viewer else { return };
    let fill = match v.backdrop {
        Backdrop::Translucent => t.viewer_overlay,
        Backdrop::Opaque => t.viewer_bg,
    };
    ui.painter().rect_filled(rect, 0, fill);
    let line = format!("Photo viewer: photo {} of its folder", v.index.saturating_add(1));
    widgets::label_in(ui.painter(), rect, Align2::CENTER_CENTER, &line, fonts::regular(t.body), t.text_dim);
}
