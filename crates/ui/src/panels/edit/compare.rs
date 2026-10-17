//! Compare in the edit view (`editing.md` §4): A│B's and A│A's two sides in the photo area, the
//! focused side's "Selected" pill, Switch Focus and Switch Layout, and before/after.
//!
//! - **The sides** ([`panes`]): one photo fills the photo area; while comparing the area splits
//!   into two halves side by side (or one above the other after ⇅) with a 1 pt `divider` line
//!   between them. A│B's left (top) side is `edit.photo`, its right (bottom) side `edit.second`;
//!   A│A shows the photo on both, the right side its variant. The canvas draws each side from its
//!   preview pane ([`super::canvas`]); the focused side is always pane 0, the pane the tools hand
//!   their candidates to, so a tool previews on the side it acts on. Zoom and pan are synced: one
//!   `edit.zoom` and one shared centre (LightCraft's compare at 3b4279fc keeps one zoom and pan
//!   for both photos too).
//! - **The pill** (`label:compare.selected`): the focused side's "Selected", white SemiBold 14 on
//!   a grey plate, in the side's margin at the end next to the divider (the top margin's inner end
//!   side by side; the margin along the divider stacked). Ui-kit has no token for the spec's
//!   `#666` grey; the plate is `tool_active`, the compare toggle's own "on" grey.
//! - **Switch Focus ⇄ and Switch Layout ⇅** (`button:compare.switchFocus`,
//!   `button:compare.switchLayout`): drawn while comparing in the place the top bar keeps for them,
//!   at its right end (`top_bar::RIGHT_GROUP`).
//! - **Before/after**: `\` pressed runs `edit.showOriginal` (the keymap; `{}` means on); its
//!   release, seen here, sends `on: false`, and so does the window losing the focus (a release it
//!   would not see). The focused
//!   pane then shows its photo unedited (`PaneShows::Unedited`).
//! - A│B whose second photo left the library goes back to one photo.

use dalica_core::PhotoId;
use dalica_engine::preview::PaneShows;
use dalica_ui_kit::registry::{self, Role};
use dalica_ui_kit::widgets::{self, Target, focus_ring};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, WidgetType, pos2, vec2};
use serde_json::json;

use super::{canvas, top_bar};
use crate::app::App;
use crate::commands::compare as cmd;
use crate::commands::edit_view;
use crate::state::{Compare, CompareLayout, Side};

/// The pill's automation id and text.
pub const PILL_ID: &str = "label:compare.selected";
pub const PILL: &str = "Selected";
/// The pill's text size, height and side padding; its distance from the divider when stacked.
pub const PILL_TEXT: f32 = 14.0;
pub const PILL_H: f32 = 20.0;
pub const PILL_PAD: f32 = 10.0;
pub const PILL_INSET: f32 = 2.0;
/// Switch Focus and Switch Layout.
pub const SWITCH_FOCUS_ID: &str = "button:compare.switchFocus";
pub const SWITCH_FOCUS: &str = "Switch focus to the other side";
pub const SWITCH_LAYOUT_ID: &str = "button:compare.switchLayout";
pub const SWITCH_LAYOUT: &str = "Show side by side, or one above the other";

/// One side of the photo area: where it is, the photo and the preview pane it shows, what that
/// pane shows of it, whether it has the focus, its key (0 the left or only side, 1 the right) and
/// whether a compare mode is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pane {
    pub area: Rect,
    pub photo: PhotoId,
    pub pane: usize,
    pub shows: PaneShows,
    pub focused: bool,
    pub key: usize,
    pub comparing: bool,
}

/// The two halves of `area`: side by side, or one above the other; the divider is the 1 pt
/// between them.
pub fn halves(area: Rect, layout: CompareLayout) -> [Rect; 2] {
    match layout {
        CompareLayout::SideBySide => {
            let mid = area.center().x.round();
            [Rect::from_min_max(area.min, pos2(mid - 0.5, area.max.y)), Rect::from_min_max(pos2(mid + 0.5, area.min.y), area.max)]
        }
        CompareLayout::Stacked => {
            let mid = area.center().y.round();
            [Rect::from_min_max(area.min, pos2(area.max.x, mid - 0.5)), Rect::from_min_max(pos2(area.min.x, mid + 0.5), area.max)]
        }
    }
}

/// The sides the photo area shows now, left (top) first; empty outside the edit view.
pub fn panes(app: &App, area: Rect) -> Vec<Pane> {
    let edit = &app.state().edit;
    let Some(left) = edit_view::open_photo(app).and(edit.photo) else { return Vec::new() };
    let original = |focused: bool, shows: PaneShows| if focused && edit.show_original { PaneShows::Unedited } else { shows };
    let (right, right_shows) = match (cmd::mode(app), edit.second) {
        (Compare::Two, Some(second)) => (second, PaneShows::Edited),
        (Compare::Same, _) => (left, PaneShows::Variant),
        _ => {
            return vec![Pane { area, photo: left, pane: 0, shows: original(true, PaneShows::Edited), focused: true, key: 0, comparing: false }];
        }
    };
    let [a, b] = halves(area, edit.layout);
    let left_focused = edit.focus == Side::Left;
    let pane = |focused: bool| if focused { 0 } else { 1 };
    vec![
        Pane {
            area: a,
            photo: left,
            pane: pane(left_focused),
            shows: original(left_focused, PaneShows::Edited),
            focused: left_focused,
            key: 0,
            comparing: true,
        },
        Pane {
            area: b,
            photo: right,
            pane: pane(!left_focused),
            shows: original(!left_focused, right_shows),
            focused: !left_focused,
            key: 1,
            comparing: true,
        },
    ]
}

/// Draws the compare modes' parts over the photo area (`ui`'s rect, the `canvas` region), after
/// the canvas: the divider, the pill and the switches; and ends a held Show original.
pub fn show(app: &mut App, ui: &mut Ui) {
    release_original(app, ui.ctx());
    let gone = |s: PhotoId| app.session().is_some_and(|x| x.library().photo(s).is_none());
    if cmd::mode(app) == Compare::Two && app.state().edit.second.is_none_or(gone) {
        cmd::one_photo(app);
    }
    if cmd::mode(app) == Compare::One {
        return;
    }
    let t = *app.tokens();
    let area = ui.max_rect();
    let edit = &app.state().edit;
    let (layout, focus) = (edit.layout, edit.focus);
    let [a, b] = halves(area, layout);
    let painter = ui.painter_at(area);
    match layout {
        CompareLayout::SideBySide => painter.vline(a.max.x + 0.5, area.y_range(), Stroke::new(1.0, t.divider)),
        CompareLayout::Stacked => painter.hline(area.x_range(), a.max.y + 0.5, Stroke::new(1.0, t.divider)),
    };
    let side = if focus == Side::Left { a } else { b };
    pill(ui, &t, pill_rect(ui, side, layout, focus));
    switches(app, ui, &t);
}

/// `\` let go (or the window losing the focus) ends Show original.
fn release_original(app: &mut App, ctx: &egui::Context) {
    if !app.state().edit.show_original {
        return;
    }
    let released = ctx.input(|i| {
        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Backslash, pressed: false, .. } | egui::Event::WindowFocused(false)))
    });
    if released {
        let _ = app.run("edit.showOriginal", &json!({ "on": false }));
    }
}

/// Where the pill goes on the focused `side`: in its margin ([`canvas::PADDING`]) at the end next
/// to the divider.
fn pill_rect(ui: &Ui, side: Rect, layout: CompareLayout, focus: Side) -> Rect {
    let w = ui.painter().layout_no_wrap(PILL.into(), fonts::semibold(PILL_TEXT), Color32::WHITE).size().x + 2.0 * PILL_PAD;
    let size = vec2(w.round(), PILL_H);
    let y_top = side.min.y + (canvas::PADDING - PILL_H) / 2.0;
    let min = match (layout, focus) {
        (CompareLayout::SideBySide, Side::Left) => pos2(side.max.x - canvas::PADDING - size.x, y_top),
        (CompareLayout::SideBySide, Side::Right) => pos2(side.min.x + canvas::PADDING, y_top),
        (CompareLayout::Stacked, Side::Left) => pos2(side.center().x - size.x / 2.0, side.max.y - PILL_INSET - size.y),
        (CompareLayout::Stacked, Side::Right) => pos2(side.center().x - size.x / 2.0, side.min.y + PILL_INSET),
    };
    Rect::from_min_size(pos2(min.x.round(), min.y.round()), size)
}

fn pill(ui: &Ui, t: &Tokens, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((PILL_H / 2.0) as u8), t.tool_active);
    p.text(rect.center(), Align2::CENTER_CENTER, PILL, fonts::semibold(PILL_TEXT), t.overlay_text);
    widgets::register(ui.ctx(), PILL_ID, Role::Other, PILL, Target::Painted { rect, clip: ui.clip_rect() }, true);
}

/// Switch Focus and Switch Layout at the top bar's right end.
fn switches(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let Some(bar) = registry::find(ui.ctx(), "region:editTopBar") else { return };
    let [x, y, w, h] = bar.rect;
    let bar = Rect::from_min_size(pos2(x, y), vec2(w, h));
    let side = top_bar::SWITCH_W;
    let cy = bar.center().y;
    let right = bar.max.x - top_bar::GAP;
    let focus = Rect::from_center_size(pos2(right - side * 1.5, cy), vec2(side, side));
    let layout = Rect::from_center_size(pos2(right - side * 0.5, cy), vec2(side, side));
    let mut over = ui.new_child(UiBuilder::new().max_rect(bar));
    over.set_clip_rect(bar);
    if switch(&mut over, t, SWITCH_FOCUS_ID, SWITCH_FOCUS, focus, false) {
        let _ = app.run("edit.switchFocus", &json!({}));
    }
    if switch(&mut over, t, SWITCH_LAYOUT_ID, SWITCH_LAYOUT, layout, true) {
        let _ = app.run("edit.switchLayout", &json!({}));
    }
}

/// One switch button over `rect`: two arrows, ⇄ (or ⇅ when `vertical`), drawn as the top bar's
/// glyph buttons are (no plate at rest, `hover × 0.7` hovered). Returns whether it was clicked.
fn switch(ui: &mut Ui, t: &Tokens, id: &str, tip: &str, rect: Rect, vertical: bool) -> bool {
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let p = ui.painter();
    if r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    focus_ring(p, t, &r, rect, t.radius);
    let s = Stroke::new(1.5, if r.hovered() { t.text } else { t.icon });
    let c = rect.center();
    let (len, gap, head) = ((rect.width() * 0.3).round(), 3.0, 3.5);
    // `u` runs along the arrows, `v` across them.
    let at = |u: f32, v: f32| if vertical { pos2(c.x + v, c.y + u) } else { pos2(c.x + u, c.y + v) };
    let arrow = |from: Pos2, to: Pos2, tip_u: f32, v: f32, dir: f32| {
        p.line_segment([from, to], s);
        p.line_segment([to, at(tip_u - dir * head, v - head)], s);
        p.line_segment([to, at(tip_u - dir * head, v + head)], s);
    };
    arrow(at(-len, -gap), at(len, -gap), len, -gap, 1.0);
    arrow(at(len, gap), at(-len, gap), -len, gap, -1.0);
    widgets::register(ui.ctx(), id, Role::IconButton, tip, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, true, tip));
    r.on_hover_text(tip).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_halves_split_the_area_with_a_point_between() {
        let area = Rect::from_min_size(pos2(280.0, 64.0), vec2(800.0, 600.0));
        let [a, b] = halves(area, CompareLayout::SideBySide);
        assert_eq!((a.min, a.max.x, b.min.x, b.max), (area.min, 679.5, 680.5, area.max));
        let [a, b] = halves(area, CompareLayout::Stacked);
        assert_eq!((a.max.y, b.min.y), (363.5, 364.5));
    }
}
