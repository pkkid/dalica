//! The folder list (`ui-layout.md` §4), left of the light table: one row per folder, newest first,
//! with its photo count. Clicking a row selects its section and scrolls the light table to it.

use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Id, Rect, Sense, Ui, vec2};

use super::{Panels, light_table};
use crate::app::App;

pub const WIDTH: f32 = 240.0;
const GROUP_HEADER: f32 = 30.0;
const ROW: f32 = 24.0;

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let painter = ui.painter().clone();
    let Some(session) = app.session() else { return };
    p.sections.update(session.library());
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), GROUP_HEADER));
    painter.text(
        header.left_center() + vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        format!("Folders ({})", p.sections.list.len()),
        fonts::semibold(13.0),
        t.text_label,
    );
    let mut picked = None;
    for (i, s) in p.sections.list.iter().enumerate() {
        let row = Rect::from_min_size(rect.min + vec2(0.0, GROUP_HEADER + i as f32 * ROW), vec2(rect.width(), ROW));
        if row.min.y > rect.max.y {
            break;
        }
        let r = ui.interact(row, Id::new(("tree:folder", s.folder.get())), Sense::click());
        let selected = app.state().folder == Some(s.folder);
        if selected {
            painter.rect_filled(row, 0, t.canvas);
        } else if r.hovered() {
            painter.rect_filled(row, 0, t.hover);
        }
        let color = if selected { t.text } else { t.text_label };
        painter.text(row.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, &s.title, fonts::regular(t.body), color);
        painter.text(row.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, s.photos.len().to_string(), fonts::regular(12.0), t.text_dim);
        widgets::register(ui.ctx(), &format!("tree:folder:{}", s.folder.get()), Role::TreeRow, &s.title, Target::Widget(&r), selected);
        if r.clicked() {
            picked = Some(s.folder);
        }
    }
    if let Some(folder) = picked {
        app.state_mut().folder = Some(folder);
        let width = ui.ctx().content_rect().width() - WIDTH;
        p.scroll_to = light_table::offset_of(&p.sections, folder, width, app.state().thumb_size);
    }
}
