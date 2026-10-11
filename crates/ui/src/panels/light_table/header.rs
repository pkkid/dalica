//! A section's header block (`ui-layout.md` §5.2, `design-system.md` §7): `folder_header` fill
//! across the light table, the 2 pt `accent` bar of the selected section, the title and the date
//! line; and its sticky form, the first line only, pinned over the cells.
//!
//! **Boundary.** The parent calls [`show`] once per header on screen, in painting order, with the
//! frame, this part's state ([`Headers`]), the section, the rect the layout gives it (full width ×
//! [`Spacing::header`](super::layout::Spacing::header), or × [`STICKY`](super::layout::STICKY) for
//! [`Form::Sticky`], whose top may be above the view while the next header pushes it up) and the
//! form. It paints inside the rect, registers `section:<id>` and its parts, and returns what the
//! header asks for ([`HeaderOut`]); the parent hands that to [`apply`] once the library is no
//! longer borrowed.

use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Rect, Ui, vec2};
use serde_json::Value;

use super::{Frame, Section};
use crate::app::App;

/// The header's left padding (points).
pub const PADDING: f32 = 12.0;

/// Which form of the header to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The full block above the section's grid.
    Block,
    /// Pinned at the top while the section scrolls under it: the first line only.
    Sticky,
}

/// What the headers keep between frames.
#[derive(Debug, Default)]
pub struct Headers {}

/// What a header asks for: commands to run, in order.
#[derive(Debug, Default)]
pub struct HeaderOut {
    pub commands: Vec<(String, Value)>,
}

/// Paints `section`'s header in `rect` (see the module's boundary).
pub fn show(ui: &mut Ui, f: &Frame<'_>, _headers: &mut Headers, section: &Section, rect: Rect, form: Form) -> HeaderOut {
    let t = f.t;
    let painter = ui.painter();
    painter.rect_filled(rect, 0, t.folder_header);
    let selected = f.state.folder == Some(section.folder);
    if selected {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0, t.accent);
    }
    match form {
        Form::Block => {
            painter.text(rect.min + vec2(PADDING, 10.0), Align2::LEFT_TOP, &section.title, fonts::semibold(t.title), t.text);
            painter.text(rect.min + vec2(PADDING, 32.0), Align2::LEFT_TOP, &section.subtitle, fonts::regular(12.5), t.text_dim);
            let id = format!("section:{}", section.id);
            widgets::register(ui.ctx(), &id, Role::Other, &section.title, Target::Painted { rect, clip: ui.clip_rect() }, selected);
        }
        Form::Sticky => {
            painter.text(rect.left_center() + vec2(PADDING, 0.0), Align2::LEFT_CENTER, &section.title, fonts::semibold(t.title), t.text);
        }
    }
    HeaderOut::default()
}

/// Runs what a header asked for.
pub fn apply(app: &mut App, out: HeaderOut) {
    for (id, params) in out.commands {
        let _ = app.run(&id, &params);
    }
}
