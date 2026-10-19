//! The edit top bar (`editing.md` §2): 40 high on `chrome`, from the left **Back to Library**
//! (`button:edit.back`, `view.library`) and **Play** (`button:edit.play`, `view.slideshow`: a
//! slideshow from this photo, enabled as the command is); the filmstrip between the two groups
//! ([`super::filmstrip`], its own region);
//! on the right the **compare toggle**, three joined segments "A", "A|B", "A|A" (a plain bar: the
//! UI font has no box-drawing line)
//! (`button:compare.one`, `button:compare.two`, `button:compare.same`, tooltips "Show one photo",
//! "Compare two photos", "Compare two versions of this photo"), each running `edit.compare {mode}`
//! and enabled as that command is for its mode (A│B and A│A arrive with M11.16). Switch Focus and
//! Switch Layout show only in the compare modes (M11.16), so their place stays empty.
//! **Edit Collage** (`button:edit.editCollage`, `collage.edit {id}`: the Collage room reopened from
//! the photo's recipe, `creating.md` §3) shows left of the compare toggle for a collage the engine's
//! `collage.canEdit` can reopen ([`shows_edit_collage`], M14.20): asked each frame, which the engine
//! answers from what it found for the photo as it is now (a photo not looked at yet is looked at
//! on a worker, the button showing once that look says yes); an ordinary photo shows none.
//!
//! The groups take the widths `panels::layout` reserves for them ([`LEFT_GROUP`] and
//! [`RIGHT_GROUP`] add up to its `TOP_LEFT_GROUP` and `TOP_RIGHT_GROUP`).

use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, focus_ring};
use egui::{Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::state::Compare;

/// Back to Library.
pub const BACK_ID: &str = "button:edit.back";
pub const BACK: &str = "Back to Library";
/// Play: a slideshow from this photo (M11).
pub const PLAY_ID: &str = "button:edit.play";
pub const PLAY: &str = "Play";
pub const PLAY_TIP: &str = "Play a slideshow from this photo";

/// The compare toggle's segments: id, label, tooltip, mode.
pub const COMPARE: [(&str, &str, &str, Compare); 3] = [
    ("button:compare.one", "A", "Show one photo", Compare::One),
    ("button:compare.two", "A|B", "Compare two photos", Compare::Two),
    ("button:compare.same", "A|A", "Compare two versions of this photo", Compare::Same),
];

/// Edit Collage: its id, label, tooltip and width.
pub const EDIT_COLLAGE_ID: &str = "button:edit.editCollage";
pub const EDIT_COLLAGE: &str = "Edit Collage";
pub const EDIT_COLLAGE_TIP: &str = "Change this collage in the Collage room";
pub const EDIT_COLLAGE_W: f32 = 120.0;

/// Whether Edit Collage shows for the photo in the edit view: for a collage the engine's
/// `collage.canEdit` can reopen (a readable recipe beside it). No while the engine still looks,
/// and with no photo or no session.
pub fn shows_edit_collage(app: &mut App) -> bool {
    let Some(photo) = app.state().edit.photo else { return false };
    let Some(s) = app.session_mut() else { return false };
    s.execute("collage.canEdit", &json!({ "id": photo })).ok().and_then(|r| r.get("canEdit").and_then(serde_json::Value::as_bool)).unwrap_or(false)
}

/// The margin at each end of the bar and between its widgets.
pub const GAP: f32 = 8.0;
/// Back to Library's and Play's widths.
pub const BACK_W: f32 = 140.0;
pub const PLAY_W: f32 = 76.0;
/// The compare toggle's width (three segments of 44) and Switch Focus / Switch Layout's edge.
pub const COMPARE_W: f32 = 132.0;
pub const SWITCH_W: f32 = 32.0;
/// The left group: margin, Back, gap, Play, gap (the filmstrip starts after it).
pub const LEFT_GROUP: f32 = GAP + BACK_W + GAP + PLAY_W + GAP;
/// The right group: gap, the compare toggle, gap, Switch Focus and Switch Layout, margin.
pub const RIGHT_GROUP: f32 = GAP + COMPARE_W + GAP + 2.0 * SWITCH_W + GAP;

/// Draws the top bar's two groups into `ui`'s rect (the whole bar).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let bar = ui.max_rect();
    let h = t.metrics.button_h;
    let y = (bar.center().y - h / 2.0).round();
    let back = Rect::from_min_size(pos2(bar.min.x + GAP, y), vec2(BACK_W, h));
    if labelled_button(ui, &t, BACK_ID, EditIcon::BackToLibrary, BACK, back, true, BACK).clicked() {
        let _ = app.run("view.library", &json!({}));
    }
    let play = Rect::from_min_size(pos2(back.max.x + GAP, y), vec2(PLAY_W, h));
    // A slideshow from this photo, live as `view.slideshow` is.
    let can_play = crate::commands::enabled(app, "view.slideshow", &json!({})).is_ok();
    if labelled_button(ui, &t, PLAY_ID, EditIcon::Play, PLAY, play, can_play, PLAY_TIP).clicked() {
        let _ = app.run("view.slideshow", &json!({}));
    }

    let right = bar.max.x - GAP - 2.0 * SWITCH_W - GAP;
    let compare = Rect::from_min_size(pos2(right - COMPARE_W, y), vec2(COMPARE_W, h));
    let live = COMPARE.map(|(.., mode)| crate::commands::enabled(app, "edit.compare", &json!({ "mode": mode })).is_ok());
    if compare.min.x >= play.max.x
        && let Some(mode) = compare_toggle(ui, &t, compare, app.state().edit.compare, live)
    {
        let _ = app.run("edit.compare", &json!({ "mode": mode }));
    }

    let edit_collage = Rect::from_min_size(pos2(compare.min.x - GAP - EDIT_COLLAGE_W, y), vec2(EDIT_COLLAGE_W, h));
    if edit_collage.min.x >= play.max.x
        && shows_edit_collage(app)
        && let Some(photo) = app.state().edit.photo
    {
        let mut c = child(ui, edit_collage, true);
        if widgets::text_button(&mut c, &t, EDIT_COLLAGE_ID, EDIT_COLLAGE, false).on_hover_text(EDIT_COLLAGE_TIP).clicked() {
            let _ = app.run("collage.edit", &json!({ "id": photo }));
        }
    }
}

/// A child of `ui` over `rect`, disabled unless `enabled`.
pub(crate) fn child(ui: &mut Ui, rect: Rect, enabled: bool) -> Ui {
    let mut c = ui.new_child(UiBuilder::new().max_rect(rect));
    if !enabled {
        c.disable();
    }
    c
}

/// The fill of a button plate: `pressed` held, `hover` hovered, `button` at rest.
fn plate(t: &Tokens, r: &Response) -> Color32 {
    if r.enabled() && r.is_pointer_button_down_on() {
        t.pressed
    } else if r.enabled() && r.hovered() {
        t.hover
    } else {
        t.button
    }
}

/// A button with an edit glyph (16) left of its label on a `text_button` plate, filling `rect`;
/// `text_disabled` glyph and label when disabled, the tooltip shown either way.
#[allow(clippy::too_many_arguments)] // the button's parts, as `widgets::text_button` takes them
pub(crate) fn labelled_button(ui: &mut Ui, t: &Tokens, id: &str, icon: EditIcon, label: &str, rect: Rect, enabled: bool, tooltip: &str) -> Response {
    let c = child(ui, rect, enabled);
    let r = c.interact(rect, Id::new(id), Sense::click());
    let p = c.painter();
    p.rect(rect, CornerRadius::same(t.radius), plate(t, &r), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    focus_ring(p, t, &r, rect, t.radius);
    let (glyph, text) = if enabled { (if r.hovered() { t.text } else { t.icon }, t.text) } else { (t.text_disabled, t.text_disabled) };
    let font = fonts::semibold(t.button_label);
    let galley = p.layout_no_wrap(label.to_owned(), font, text);
    let icon_side = 16.0;
    let content = icon_side + 6.0 + galley.size().x;
    let x = (rect.center().x - content / 2.0).max(rect.min.x + 6.0).round();
    paint_edit(p, Rect::from_min_size(pos2(x, (rect.center().y - icon_side / 2.0).round()), Vec2::splat(icon_side)), icon, glyph);
    p.galley(pos2(x + icon_side + 6.0, (rect.center().y - galley.size().y / 2.0).round()), galley, text);
    widgets::register(c.ctx(), id, Role::Button, label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled { r.on_hover_text(tooltip) } else { r.on_disabled_hover_text(tooltip) }
}

/// A button over `rect` showing an edit glyph (60 % of its shorter side): a `hover × 0.7` plate
/// when hovered, the glyph `icon` at rest, `text` hovered, `text_disabled` disabled.
pub(crate) fn glyph_button(ui: &mut Ui, t: &Tokens, id: &str, icon: EditIcon, rect: Rect, enabled: bool, tooltip: &str) -> Response {
    let c = child(ui, rect, enabled);
    let r = c.interact(rect, Id::new(id), Sense::click());
    let p = c.painter();
    if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    focus_ring(p, t, &r, rect, t.radius);
    let glyph = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    let side = (rect.width().min(rect.height()) * 0.6).round();
    paint_edit(p, Rect::from_center_size(rect.center(), Vec2::splat(side)), icon, glyph);
    widgets::register(c.ctx(), id, Role::IconButton, tooltip, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, tooltip));
    if enabled { r.on_hover_text(tooltip) } else { r.on_disabled_hover_text(tooltip) }
}

/// The compare toggle in `rect`: three joined segments, `current` filled `tool_active` with a
/// `pick` label; a segment is enabled when `live` says so. Returns the mode clicked.
fn compare_toggle(ui: &mut Ui, t: &Tokens, rect: Rect, current: Compare, live: [bool; 3]) -> Option<Compare> {
    let w = rect.width() / COMPARE.len() as f32;
    let font = fonts::semibold(t.button_label);
    let mut clicked = None;
    for (i, (id, label, tip, mode)) in COMPARE.iter().enumerate() {
        let seg = Rect::from_min_size(pos2(rect.min.x + w * i as f32, rect.min.y), vec2(w, rect.height()));
        let enabled = live.get(i).copied().unwrap_or(false);
        let c = child(ui, seg, enabled);
        let r = c.interact(seg, Id::new(*id), Sense::click());
        let on = current == *mode;
        let last = i + 1 == COMPARE.len();
        let corners = CornerRadius {
            nw: if i == 0 { t.radius } else { 0 },
            sw: if i == 0 { t.radius } else { 0 },
            ne: if last { t.radius } else { 0 },
            se: if last { t.radius } else { 0 },
        };
        let p = c.painter();
        p.rect_filled(seg, corners, if on { t.tool_active } else { plate(t, &r) });
        let color = if !enabled {
            t.text_disabled
        } else if on {
            t.pick
        } else {
            t.text
        };
        widgets::label_in(p, seg, Align2::CENTER_CENTER, label, font.clone(), color);
        if i > 0 {
            p.vline(seg.min.x, seg.y_range().shrink(1.0), Stroke::new(1.0, t.button_border));
        }
        focus_ring(p, t, &r, seg, t.radius);
        widgets::register(c.ctx(), id, Role::Toggle, tip, Target::Widget(&r), on);
        r.widget_info(|| egui::WidgetInfo::selected(WidgetType::RadioButton, enabled, on, *tip));
        let r = if enabled { r.on_hover_text(*tip) } else { r.on_disabled_hover_text(*tip) };
        if r.clicked() {
            clicked = Some(*mode);
        }
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(t.radius), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::layout::{TOP_LEFT_GROUP, TOP_RIGHT_GROUP};

    #[test]
    fn the_groups_fill_the_room_the_layout_keeps_for_them() {
        assert_eq!(LEFT_GROUP, TOP_LEFT_GROUP);
        assert_eq!(RIGHT_GROUP, TOP_RIGHT_GROUP);
        assert_eq!(COMPARE.iter().filter(|c| c.3 == Compare::One).count(), 1);
    }
}
