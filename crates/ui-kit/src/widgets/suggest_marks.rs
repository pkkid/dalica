//! The ✓ and × marks of a suggested name (`organizing.md` §6.3–§6.4): under a suggested face in a
//! person's section and the Unnamed grid, and beside "Name?" in the People pane. ✓ confirms the
//! match, × rejects it; what that does is the caller's.
//!
//! Two small buttons side by side: [`SUGGEST_MARK`] square, [`SUGGEST_MARK_GAP`] apart, each a
//! `button` plate (radius 4, 1 pt `button_border`; `hover` hovered, `pressed` held) holding its
//! 12 pt mark in `icon` (`text` hovered, `text_disabled` disabled). The marks are part of the
//! widget (`form`'s check mark and the search field's cross), not icons a caller picks.

use egui::{CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, vec2};

use super::form::check_mark;
use super::marks::cross;
use super::{Target, focus_ring, register, tooltip};
use crate::Tokens;
use crate::registry::Role;
use crate::text::{self, Text};

/// The side of each mark's button (points).
pub const SUGGEST_MARK: f32 = 20.0;
/// The space between the two buttons (points).
pub const SUGGEST_MARK_GAP: f32 = 6.0;
/// The side of the mark drawn in a button (points).
const MARK_GLYPH: f32 = 12.0;

/// What [`suggest_marks`] did this pass.
#[derive(Debug, Clone)]
pub struct SuggestMarksOut {
    /// ✓ was clicked: confirm the match.
    pub confirm: bool,
    /// × was clicked: reject it.
    pub reject: bool,
    /// The ✓ button's response.
    pub confirm_response: Response,
    /// The × button's response.
    pub reject_response: Response,
}

/// The automation ids of the two marks of `id`: `<id>/confirm` and `<id>/reject`.
pub fn suggest_mark_ids(id: &str) -> [String; 2] {
    [format!("{id}/confirm"), format!("{id}/reject")]
}

/// The size of the pair: two [`SUGGEST_MARK`] buttons [`SUGGEST_MARK_GAP`] apart.
pub fn suggest_marks_size() -> Vec2 {
    vec2(2.0 * SUGGEST_MARK + SUGGEST_MARK_GAP, SUGGEST_MARK)
}

/// Which mark a button carries.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Confirm,
    Reject,
}

/// One mark's button at `rect`.
fn mark_button(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, mark: Mark, enabled: bool) -> Response {
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let fill = if enabled && r.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    focus_ring(p, t, &r, rect, t.radius);
    let color = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    let glyph = Rect::from_center_size(rect.center(), Vec2::splat(MARK_GLYPH));
    match mark {
        Mark::Confirm => check_mark(p, glyph, color),
        Mark::Reject => cross(p, glyph.expand(2.0), color),
    }
    let texts = text::texts(ui.ctx());
    let (label, tip) = match mark {
        Mark::Confirm => (texts.get(Text::Confirm), texts.get(Text::ConfirmMatch)),
        Mark::Reject => (texts.get(Text::Reject), texts.get(Text::NotThisPerson)),
    };
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), false);
    tooltip(r, t, tip, None)
}

/// The pair at `rect` (centred in it), for widgets that place it themselves (the face tile).
pub(crate) fn marks_at(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, enabled: bool) -> SuggestMarksOut {
    let pair = Rect::from_min_size((rect.center() - suggest_marks_size() / 2.0).round(), suggest_marks_size());
    let [confirm_id, reject_id] = suggest_mark_ids(id);
    let left = Rect::from_min_size(pair.min, Vec2::splat(SUGGEST_MARK));
    let right = Rect::from_min_size(pair.min + vec2(SUGGEST_MARK + SUGGEST_MARK_GAP, 0.0), Vec2::splat(SUGGEST_MARK));
    let confirm_response = mark_button(ui, t, &confirm_id, left, Mark::Confirm, enabled);
    let reject_response = mark_button(ui, t, &reject_id, right, Mark::Reject, enabled);
    SuggestMarksOut { confirm: confirm_response.clicked(), reject: reject_response.clicked(), confirm_response, reject_response }
}

/// The ✓ and × of a suggestion, laid out in the ui ([`suggest_marks_size`]). Registers
/// `<id>/confirm` ("Confirm") and `<id>/reject` ("Reject"), role `button`; disabled with the ui.
pub fn suggest_marks(ui: &mut Ui, t: &Tokens, id: &str) -> SuggestMarksOut {
    let enabled = ui.is_enabled();
    let (_, rect) = ui.allocate_space(suggest_marks_size());
    marks_at(ui, t, id, rect, enabled)
}
