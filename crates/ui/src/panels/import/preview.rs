//! The Import room's preview (`organizing.md` §3 item 3): the right of the room, about 35 % of its
//! width. It shows the item in the preview (the one last clicked or reached; the first listed when
//! none is) large, turned as the room's rotation says and dimmed under the ⊘ when it will not be
//! imported, under its header "filename · date time · size" ([`HEADER_ID`]). Below it, centred:
//! the star toggle and Exclude or Include (the engine's `import.mark {star}` and `import.exclude`
//! for that item), Previous and Next (`import.previous`, `import.next`, as ← and →), and Rotate
//! Left and Rotate Right (`import.mark {rotate: -1 | 1}`).
//!
//! The picture is the candidate's in-memory thumbnail (at most 256 pixels on its longer side)
//! scaled up: the listing makes no larger one, and the room never decodes a file on the UI
//! thread.

use dalica_ui_kit::icons::{self, Glyph, Icon, ViewingIcon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, CornerRadius, Id, Rect, Response, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use super::{Action, Snapshot, Textures, one_line, paint_turned, turned};
use crate::app::App;
use crate::panels::thumbnails::fit_within;
use crate::tl;

/// The header's automation id (its label is the text shown).
pub const HEADER_ID: &str = "text:import.preview";
/// The picture's automation id.
pub const IMAGE_ID: &str = "image:import.preview";
/// The buttons.
pub const STAR_ID: &str = "button:import.star";
pub const EXCLUDE_ID: &str = "button:import.exclude";
pub const PREVIOUS_ID: &str = "button:import.previous";
pub const NEXT_ID: &str = "button:import.next";
pub const ROTATE_LEFT_ID: &str = "button:import.rotateLeft";
pub const ROTATE_RIGHT_ID: &str = "button:import.rotateRight";
/// What the preview says with nothing listed.
pub const NOTHING: &str = "No picture to preview";

/// The space around the preview's parts.
pub const PAD: f32 = 12.0;
/// The header line's height.
pub const HEADER_H: f32 = 28.0;
/// The buttons' row height, and an icon button's edge.
pub const BUTTONS_H: f32 = 40.0;
pub const BUTTON: f32 = 28.0;
/// The space between buttons, and between the button groups.
pub const BUTTON_GAP: f32 = 4.0;
pub const GROUP_GAP: f32 = 16.0;

/// The header: `IMG_0001.JPG · June 4, 2026 8:00 AM · 2.4 MB`.
pub fn header(name: &str, captured: dalica_core::DateTime, size: u64) -> String {
    tl!("{name} \u{b7} {date} \u{b7} {size}", name = name, date = crate::status_text::date_time(captured), size = crate::status_text::size(size))
}

/// The item the preview shows: the room's preview when it is listed, else the first item.
pub fn shown(app: &App, snap: &Snapshot) -> Option<usize> {
    app.state().import.preview.and_then(|id| snap.find(id)).map(|(i, _)| i).or_else(|| (!snap.candidates.is_empty()).then_some(0))
}

/// Draws the preview into `ui`'s rect.
pub fn show(app: &App, ui: &mut Ui, t: &Tokens, textures: &mut Textures, snap: &Snapshot, actions: &mut Vec<Action>) {
    let Some(session) = app.session() else { return };
    let rect = ui.max_rect().shrink(PAD);
    let p = ui.painter().clone();
    let Some(index) = shown(app, snap) else {
        widgets::label_in(&p, rect, Align2::CENTER_CENTER, &tl!("No picture to preview"), fonts::regular(t.body), t.text_dim);
        return;
    };
    let Some(c) = snap.candidates.get(index) else { return };

    // The header.
    let text = header(&c.name, c.captured, c.size);
    let g = one_line(&p, &text, fonts::regular(t.body), t.text_label, rect.width());
    let at = pos2(rect.center().x - g.size().x / 2.0, rect.min.y + (HEADER_H - g.size().y) / 2.0).round();
    let line = Rect::from_min_size(at, g.size());
    p.galley(at, g, t.text_label);
    widgets::register(ui.ctx(), HEADER_ID, Role::Other, &text, Target::Painted { rect: line, clip: ui.clip_rect() }, false);

    // The picture.
    let buttons = Rect::from_min_max(pos2(rect.min.x, rect.max.y - BUTTONS_H), rect.max);
    let area = Rect::from_min_max(pos2(rect.min.x, rect.min.y + HEADER_H + 8.0), pos2(rect.max.x, buttons.min.y - 8.0));
    if area.is_positive() {
        let ctx = ui.ctx().clone();
        let image = match textures.get(&ctx, session, c.id) {
            Some((tex, [w, h])) => {
                let (dw, dh) = turned(w, h, c.turns);
                let image = fit_within(area, dw, dh);
                widgets::photo_shadow(&p, image);
                paint_turned(&p, image, tex, c.turns);
                image
            }
            None => {
                let image = fit_within(area, 3, 2);
                p.rect_filled(image, 0, t.placeholder);
                image
            }
        };
        if snap.left_out(index) {
            widgets::excluded_mark(&p, t, image, image);
        }
        widgets::register(&ctx, IMAGE_ID, Role::Other, &c.name, Target::Painted { rect: image, clip: ui.clip_rect() }, snap.left_out(index));
    }

    // The buttons: star and exclude, previous and next, the two rotations.
    let last = snap.candidates.len().saturating_sub(1);
    let width = t.metrics.star_button.x + BUTTON_GAP + 5.0 * BUTTON + 3.0 * BUTTON_GAP + 2.0 * GROUP_GAP;
    let row = Rect::from_center_size(buttons.center(), vec2(width, BUTTON));
    ui.scope_builder(UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = BUTTON_GAP;
        if widgets::star_toggle(ui, t, STAR_ID, c.starred).clicked() {
            actions.push(("import.mark", json!({ "ids": [c.id], "star": !c.starred })));
        }
        let (tip, on) = if c.excluded { (tl!(ctx = "import", "Include"), false) } else { (tl!(ctx = "import", "Exclude"), true) };
        if glyph_button(ui, t, EXCLUDE_ID, Glyph::Viewing(ViewingIcon::Exclude), c.excluded, true, &tip).clicked() {
            actions.push(("import.exclude", json!({ "ids": [c.id], "on": on })));
        }
        ui.add_space(GROUP_GAP - BUTTON_GAP);
        if glyph_button(ui, t, PREVIOUS_ID, Glyph::Viewing(ViewingIcon::Previous), false, index > 0, &tl!("Previous")).clicked() {
            actions.push(("import.previous", json!({})));
        }
        if glyph_button(ui, t, NEXT_ID, Glyph::Viewing(ViewingIcon::Next), false, index < last, &tl!("Next")).clicked() {
            actions.push(("import.next", json!({})));
        }
        ui.add_space(GROUP_GAP - BUTTON_GAP);
        if glyph_button(ui, t, ROTATE_LEFT_ID, Glyph::Shell(Icon::RotateCcw), false, true, &tl!("Rotate Left")).clicked() {
            actions.push(("import.mark", json!({ "ids": [c.id], "rotate": -1 })));
        }
        if glyph_button(ui, t, ROTATE_RIGHT_ID, Glyph::Shell(Icon::RotateCw), false, true, &tl!("Rotate Right")).clicked() {
            actions.push(("import.mark", json!({ "ids": [c.id], "rotate": 1 })));
        }
    });
}

/// A square button of edge [`BUTTON`] showing `glyph`, as ui-kit's icon button draws one (any
/// glyph set): a highlight `tool_active` when active, `hover × 0.7` hovered; the glyph `icon`,
/// `text` hovered, `pick` active, `text_disabled` disabled. Registered as `id` (role
/// `iconButton`), labelled `tip`.
fn glyph_button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, active: bool, enabled: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let (_, rect) = ui.allocate_space(Vec2::splat(BUTTON));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.tool_active);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    let color = if !enabled {
        t.text_disabled
    } else if active {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    icons::paint_glyph(p, Rect::from_center_size(rect.center(), Vec2::splat(t.metrics.icon_menu)), glyph, color);
    widgets::register(ui.ctx(), id, Role::IconButton, tip, Target::Widget(&r), active);
    widgets::tooltip(r, t, tip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_names_the_file_its_time_and_size() {
        let at = dalica_core::DateTime::new(dalica_core::Date::new(2026, 6, 4).unwrap(), 8, 5, 0).unwrap();
        assert_eq!(header("IMG_0001.JPG", at, 2_400_000), "IMG_0001.JPG \u{b7} June 4, 2026 8:05 AM \u{b7} 2.4 MB");
    }
}
