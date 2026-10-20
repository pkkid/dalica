//! "Name?" with ✓ / × in the People pane (`organizing.md` §6.4, `faces.md` §5): under "Unnamed
//! faces", the block "Suggestions:" lists the pane's unnamed faces that have a suggested name, a
//! row each: the face, "Ana?" and ✓ (`face:<PhotoId>.<n>/confirm`: the face becomes Ana's, one undo
//! step) and × (`/reject`: never suggested as Ana again). While the Unnamed grid shows in the
//! library its own tiles carry the marks, so the pane leaves the block out (as it leaves out its
//! unnamed faces there).

use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{PeopleIcon, paint_people};
use dalica_ui_kit::widgets::{self, SUGGEST_MARK, suggest_marks_size};
use egui::{Align2, Color32, CornerRadius, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use super::{model, pictures};
use crate::app::App;
use crate::state::{FaceRef, View};
use crate::tl;

/// The block's caption.
pub const TITLE: &str = "Suggestions";
/// A row and its face (points).
const ROW: f32 = 32.0;
const FACE: f32 = 24.0;
/// Space above the block (points).
const GAP: f32 = 16.0;

/// Draws the suggestion marks of the People pane.
pub fn show(app: &mut App, ui: &mut Ui) {
    if app.state().view != View::Edit && app.state().unnamed.open {
        return;
    }
    let snapshot = crate::commands::suggestions::current(app);
    if snapshot.list.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let pane = model::model(app, &ctx);
    let Some(library) = app.session().map(dalica_engine::Session::library) else { return };
    let rows: Vec<(FaceRef, dalica_core::Rect01, String)> =
        pane.unnamed.iter().filter_map(|u| Some((u.face, u.rect, library.person(snapshot.suggested(u.face)?)?.name.clone()))).collect();
    if rows.is_empty() {
        return;
    }
    let t = *app.tokens();
    ui.add_space(GAP);
    ui.label(RichText::new(tl!(ctx = "form label", "{label}:", label = tl!("Suggestions"))).font(fonts::regular(t.body)).color(t.text_label));
    ui.add_space(4.0);
    let mut ask = None;
    for (face, rect, name) in rows {
        let width = ui.available_width();
        let (row, r) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        if !row.intersects(ui.clip_rect()) {
            continue;
        }
        if r.hovered() || app.state().people_pane.hovered == Some(face) {
            ui.painter().rect_filled(row, CornerRadius::same(t.radius), t.hover);
        }
        let at = Rect::from_min_size(pos2(row.min.x + 4.0, row.center().y - FACE / 2.0), Vec2::splat(FACE));
        match pictures::picture(&ctx, face, rect, pictures::side_px(&ctx, FACE)) {
            Some(pic) => {
                ui.painter().image(pic.texture, at, pic.uv, Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(at, 0, t.placeholder);
                paint_people(ui.painter(), at.shrink(5.0), PeopleIcon::Person, t.text_disabled);
            }
        }
        let marks = suggest_marks_size();
        let marks_at = Rect::from_min_size(pos2(row.max.x - 4.0 - marks.x, row.center().y - SUGGEST_MARK / 2.0), marks);
        let text = Rect::from_min_max(pos2(at.max.x + 8.0, row.min.y), pos2(marks_at.min.x - 6.0, row.max.y));
        if text.width() > 0.0 {
            let mut job =
                egui::text::LayoutJob::simple_singleline(tl!(ctx = "suggestion", "{name}?", name = name), fonts::regular(t.body), t.text_dim);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text.width());
            let g = ui.painter().layout_job(job);
            ui.painter().galley(Align2::LEFT_CENTER.align_size_within_rect(g.size(), text).min.round(), g, t.text_dim);
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(marks_at));
        let out = widgets::suggest_marks(&mut child, &t, &face.widget());
        if out.confirm {
            ask = Some(("suggestions.confirm", face));
        } else if out.reject {
            ask = Some(("suggestions.reject", face));
        }
    }
    if let Some((id, face)) = ask {
        // A refusal's reason is on the status line.
        let _ = app.run(id, &json!({ "faces": [face] }));
    }
}
