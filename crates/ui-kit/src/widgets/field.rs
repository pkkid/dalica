//! Text fields (`design-system.md` §6): the plain [`text_field`] (and its [`TextField`] builder)
//! and the toolbar's [`search_field`]. Both paint their own frame (`field` fill, `field_border`,
//! `accent` and `canvas` when focused) around a frameless egui `TextEdit`, so they follow the tokens
//! rather than egui's visuals, and both edit the same way ([`edit`], from LightCraft): Esc gives
//! back the text from before the edit, Return or a click away ends it once ([`Ending`]), and a
//! right-click opens Cut / Copy / Paste / Select All. New text boxes use these, never a bare
//! `egui::TextEdit`.

mod edit;

use egui::{CornerRadius, Id, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

pub use edit::Ending;
pub(crate) use edit::egui_id;

use super::marks;
use super::{Target, register};
use crate::Tokens;
use crate::registry::Role;

/// What a text field did this pass.
#[derive(Debug, Clone)]
pub struct FieldOut {
    /// The text edit's response (`changed()` on every edit, Esc's restore included).
    pub response: Response,
    /// The field's frame: where a list under it is anchored.
    pub rect: Rect,
    /// Set on the pass an edit ends, and only then (once per edit).
    pub ending: Option<Ending>,
    /// An edit is under way: the field has the focus, or its context menu does.
    pub editing: bool,
}

impl FieldOut {
    /// The edit ended with the text kept (Return or leaving the field).
    pub fn committed(&self) -> bool {
        matches!(self.ending, Some(Ending::Return | Ending::Away))
    }

    /// Esc ended the edit and the text is back as it was.
    pub fn cancelled(&self) -> bool {
        self.ending == Some(Ending::Escape)
    }
}

/// Gives the keyboard focus to the field registered as `id` (an inline editor that opens ready
/// for typing; with [`TextField::select_on_focus`] its text is then selected whole).
pub fn focus_field(ctx: &egui::Context, id: &str) {
    ctx.memory_mut(|m| m.request_focus(egui_id(id)));
}

/// The frame of a field in `rect`: `field` (or `canvas` focused) with a 1 pt border, `field_border`
/// at rest, `button_border` hovered and `accent` focused (`focus_width`, the focus ring).
fn frame(t: &Tokens, rect: Rect, hovered: bool, focused: bool, enabled: bool) -> Vec<Shape> {
    let fill = if focused { t.canvas } else { t.field };
    let (width, color) = if focused {
        (t.focus_width, t.accent)
    } else if enabled && hovered {
        (1.0, t.button_border)
    } else {
        (1.0, t.field_border)
    };
    vec![
        Shape::rect_filled(rect, CornerRadius::same(t.radius), fill),
        Shape::rect_stroke(rect, CornerRadius::same(t.radius), Stroke::new(width, color), StrokeKind::Inside),
    ]
}

/// A single-line text field `width × 24` showing `hint` in `text_dim` while empty. Text is 13 pt
/// `text` (`text_disabled` disabled). Registered as `id` on the text edit, labelled with `hint`;
/// the right-click menu's items as `<id>/cut`, `/copy`, `/paste` and `/selectAll`.
pub fn text_field(ui: &mut Ui, t: &Tokens, id: &str, text: &mut String, hint: &str, width: f32) -> FieldOut {
    TextField::new(id, hint, width).show(ui, t, text)
}

/// A [`text_field`] with options.
#[derive(Debug, Clone, Copy)]
pub struct TextField<'a> {
    id: &'a str,
    hint: &'a str,
    width: f32,
    select_on_focus: bool,
}

impl<'a> TextField<'a> {
    /// The field registered as `id`, `width` wide, showing `hint` while empty.
    pub fn new(id: &'a str, hint: &'a str, width: f32) -> Self {
        TextField { id, hint, width, select_on_focus: false }
    }

    /// Select all the text when the field takes the focus, so typing replaces it (names, dates,
    /// quick tags). A drag into the field still selects only what it covers.
    pub fn select_on_focus(mut self, on: bool) -> Self {
        self.select_on_focus = on;
        self
    }

    pub fn show(self, ui: &mut Ui, t: &Tokens, text: &mut String) -> FieldOut {
        let enabled = ui.is_enabled();
        let width = if self.width.is_finite() { self.width.max(24.0) } else { 120.0 };
        let (_, rect) = ui.allocate_space(vec2(width, t.metrics.text_field_h));
        let background = ui.painter().add(Shape::Noop);
        let inner = rect.shrink2(vec2(8.0, 0.0));
        let spec = edit::Spec { id: self.id, hint: self.hint, rect: inner, enabled, select_on_focus: self.select_on_focus };
        let e = edit::edit(ui, t, spec, text);
        let r = e.response;
        let shapes = frame(t, rect, r.hovered(), r.has_focus(), enabled);
        ui.painter().set(background, Shape::Vec(shapes));
        register(ui.ctx(), self.id, Role::Other, self.hint, Target::Widget(&r), r.has_focus());
        FieldOut { response: r, rect, ending: e.ending, editing: e.editing }
    }
}

/// What a [`search_field`] did this pass.
#[derive(Debug, Clone)]
pub struct SearchOut {
    /// The text edit's response.
    pub response: Response,
    /// The text changed (typed or cleared): filter again.
    pub changed: bool,
    /// The clear button was clicked.
    pub cleared: bool,
    /// The field's frame.
    pub rect: Rect,
    /// Set on the pass an edit ends (Esc gives back the text from before it: `changed` too).
    pub ending: Option<Ending>,
    /// An edit is under way: the field has the focus, or its context menu does.
    pub editing: bool,
}

impl From<SearchOut> for FieldOut {
    fn from(s: SearchOut) -> FieldOut {
        FieldOut { response: s.response, rect: s.rect, ending: s.ending, editing: s.editing }
    }
}

/// The search field (`design-system.md` §6): `width × 28`, radius 4, a 14 pt magnifier and the
/// `hint` placeholder in `text_dim`, and a clear (×) button registered as `clear_id` while the text is
/// not empty; clearing keeps the focus in the field. Filters run on every keystroke: `changed` says
/// when. It edits as a [`text_field`] does (Esc gives back the text from before the edit).
pub fn search_field(ui: &mut Ui, t: &Tokens, id: &str, clear_id: &str, text: &mut String, hint: &str, width: f32) -> SearchOut {
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width.max(64.0) } else { 160.0 };
    let (_, rect) = ui.allocate_space(vec2(width, t.metrics.search_h));
    let background = ui.painter().add(Shape::Noop);
    let has_text = !text.is_empty();
    let clear_rect = Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
    let right = if has_text { clear_rect.min.x - 2.0 } else { rect.max.x - 8.0 };
    let inner = Rect::from_min_max(pos2(rect.min.x + 28.0, rect.min.y), pos2(right.max(rect.min.x + 32.0), rect.max.y));
    let before = text.clone();
    let spec = edit::Spec { id, hint, rect: inner, enabled, select_on_focus: false };
    let e = edit::edit(ui, t, spec, text);
    let mut r = e.response;
    let mut cleared = false;
    let mut clear_shapes = Vec::new();
    if has_text {
        let c = ui.interact(clear_rect, Id::new(clear_id), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && c.hovered() {
            clear_shapes.push(Shape::circle_filled(clear_rect.center(), 8.0, t.hover));
        }
        if c.has_focus() {
            clear_shapes.push(Shape::circle_stroke(clear_rect.center(), 9.0, Stroke::new(t.focus_width, t.accent)));
        }
        let glyph = if !enabled {
            t.text_disabled
        } else if c.hovered() {
            t.text
        } else {
            t.text_dim
        };
        marks::cross(ui.painter(), clear_rect.shrink(2.0), glyph);
        register(ui.ctx(), clear_id, Role::Button, "Clear search", Target::Widget(&c), false);
        c.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Clear search"));
        if c.clicked() {
            text.clear();
            cleared = true;
            r.request_focus();
        }
    }
    let changed = *text != before;
    if changed {
        r.mark_changed();
    }
    let focused = r.has_focus();
    let mut shapes = frame(t, rect, r.hovered(), focused, enabled);
    shapes.extend(clear_shapes);
    ui.painter().set(background, Shape::Vec(shapes));
    let glass = Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), Vec2::splat(14.0));
    marks::magnifier(ui.painter(), glass, if enabled { t.text_dim } else { t.text_disabled });
    register(ui.ctx(), id, Role::Other, hint, Target::Widget(&r), focused);
    SearchOut { response: r, changed, cleared, rect, ending: e.ending, editing: e.editing }
}
