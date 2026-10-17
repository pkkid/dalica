//! The edit panel (`editing.md` §2, §5): the five icon tabs, the tab's content (or the open tool's
//! panel), the Undo / Redo pair and the Histogram & Camera Info box, top to bottom.
//!
//! The tabs (`tab:edit.basic` … `tab:edit.effects3`, `edit.tab`) run along the panel's top on a
//! `canvas` strip, 40 × 32 each with their glyph and tooltip, the one shown filled `chrome` and
//! joined to the panel below. The content is the tab's: Basic Fixes ([`basic`]), Tuning
//! ([`tuning`]), or an empty state for the three effect tabs until M12. While a modal tool is open
//! (Crop, Straighten, Retouch) the panel of Crop and Retouch replaces the content, and the rest of
//! the panel is drawn disabled until Apply or Cancel: the tab content and Undo / Redo. The tabs stay
//! clickable (`editing.md` §5): another tab shows its content disabled, the open tool's panel
//! stays. A Tuning session leaves the panel enabled.
//!
//! **Show original** (`button:edit.showOriginal`, `editing.md` §4) sits at the tab strip's right
//! end: pressed and held, it shows the focused photo unedited (`edit.showOriginal {on: true}`), and
//! let go, as edited again (`{on: false}`).
//!
//! **A│A's right side** (the variant, `commands::compare::on_variant`): the panel is drawn as on
//! any photo, and every part of it acts on the variant (`commands::tools::recipe`): the tools,
//! the Fill Light row, the Tuning tab, the one-click fixes, Undo and Redo.

use dalica_ui_kit::Tokens;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Tab, Target};
use egui::{CornerRadius, Id, Rect, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use super::{basic, crop, histogram, history, retouch, tuning};
use crate::app::App;
use crate::commands::tools;
use crate::state::{EditTab, ToolKind};

/// The tab row's height; the Undo / Redo row's height; the histogram box's height; the margin
/// between the parts and around them.
pub const TABS: f32 = 32.0;
pub const HISTORY: f32 = 28.0;
pub const HISTOGRAM: f32 = 150.0;
pub const MARGIN: f32 = 8.0;

/// The tabs in order: the tab, its id, its glyph and its tooltip (`editing.md` §5).
pub const TAB_ITEMS: [(EditTab, &str, EditIcon, &str); 5] = [
    (EditTab::Basic, "tab:edit.basic", EditIcon::BasicFixes, "Basic fixes"),
    (EditTab::Tuning, "tab:edit.tuning", EditIcon::Tuning, "Lighting and color tuning"),
    (EditTab::Effects, "tab:edit.effects", EditIcon::Effects, "Effects"),
    (EditTab::Effects2, "tab:edit.effects2", EditIcon::MoreEffects, "More effects"),
    (EditTab::Effects3, "tab:edit.effects3", EditIcon::EvenMoreEffects, "Even more effects"),
];

/// The effect tabs' empty state until M12.
pub const EFFECTS_EMPTY_ID: &str = "empty:edit.effects";
pub const EFFECTS_EMPTY_TITLE: &str = "No effects yet";
pub const EFFECTS_EMPTY_BODY: &str = "Effects come in a later version of Dalica.";

/// Show original: its automation id, label, tooltip and width.
pub const ORIGINAL_ID: &str = "button:edit.showOriginal";
pub const ORIGINAL: &str = "Original";
pub const ORIGINAL_TIP: &str = "Press and hold to show the original (or hold \\)";
pub const ORIGINAL_W: f32 = 68.0;

/// Where the parts of the edit panel are inside its rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub tabs: Rect,
    pub content: Rect,
    pub history: Rect,
    pub histogram: Rect,
}

/// A rect from `x0..x1` × `y0..y1`, never inside out.
fn span(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)))
}

/// The parts of a panel `rect`: the tabs along the top edge (the whole width), the histogram box at
/// the bottom, Undo / Redo above it, the content between. A panel too short for them squeezes the
/// content first.
pub fn parts(rect: Rect) -> Parts {
    let (l, r) = (rect.min.x + MARGIN, rect.max.x - MARGIN);
    let tabs = span(rect.min.x, rect.max.x, rect.min.y, rect.min.y + TABS);
    let histogram = span(l, r, (rect.max.y - MARGIN - HISTOGRAM).max(tabs.max.y), (rect.max.y - MARGIN).max(tabs.max.y));
    let history = span(l, r, (histogram.min.y - MARGIN - HISTORY).max(tabs.max.y), (histogram.min.y - MARGIN).max(tabs.max.y));
    let content = span(l, r, tabs.max.y + MARGIN, history.min.y - MARGIN);
    Parts { tabs, content, history, histogram }
}

/// Runs `add` in a child of `ui` filling `rect`, disabled when `enabled` is false.
fn part(ui: &mut Ui, salt: &str, rect: Rect, enabled: bool, add: impl FnOnce(&mut Ui)) {
    if !rect.is_positive() {
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt(("editPanel", salt)).max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    if !enabled {
        child.disable();
    }
    add(&mut child);
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = parts(ui.max_rect());
    // A modal tool (Crop, Straighten, Retouch) holds the panel until Apply or Cancel.
    let modal = tools::open_tool(app).filter(|t| t.modal()).map(|t| t.kind());
    part(ui, "tabs", p.tabs, true, |ui| tabs(app, ui));
    let tab = app.state().edit.tab;
    part(ui, "content", p.content, true, |ui| match modal {
        Some(ToolKind::Crop) => crop::show(app, ui),
        Some(ToolKind::Retouch) => retouch::show(app, ui),
        _ => {
            if modal.is_some() {
                ui.disable();
            }
            content(app, ui, tab);
        }
    });
    part(ui, "history", p.history, modal.is_none(), |ui| history::show(app, ui));
    part(ui, "histogram", p.histogram, true, |ui| histogram::show(app, ui));
}

/// The content of `tab`.
fn content(app: &mut App, ui: &mut Ui, tab: EditTab) {
    match tab {
        EditTab::Basic => basic::show(app, ui),
        EditTab::Tuning => tuning::show(app, ui),
        // The effect tabs (M12) show an empty state until then.
        EditTab::Effects | EditTab::Effects2 | EditTab::Effects3 => {
            let t = *app.tokens();
            widgets::empty_state(ui, &t, EFFECTS_EMPTY_ID, EFFECTS_EMPTY_TITLE, EFFECTS_EMPTY_BODY, None);
        }
    }
}

/// The tab strip filling `ui`'s rect: `canvas` behind, the tabs from the left, the one shown
/// `chrome` with its top corners rounded so it joins the panel. A click shows that tab (`edit.tab`).
fn tabs(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let strip = ui.max_rect();
    ui.painter().rect_filled(strip, 0, t.canvas);
    let shown = app.state().edit.tab;
    let mut clicked = None;
    for (i, (tab, id, icon, tip)) in TAB_ITEMS.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(strip.min.x + t.metrics.edit_tab.x * i as f32, strip.min.y), vec2(t.metrics.edit_tab.x, strip.height()));
        if rect.max.x > strip.max.x + 0.5 {
            break;
        }
        if one_tab(ui, &t, id, *icon, tip, rect, *tab == shown) {
            clicked = Some(*tab);
        }
    }
    if let Some(tab) = clicked
        && tab != shown
        && let Err(e) = app.run("edit.tab", &json!({ "tab": tab }))
    {
        app.toast(&e.to_string(), widgets::ToastKind::Caution);
    }
    let h = (strip.height() - 8.0).max(0.0);
    let rect = Rect::from_min_size(pos2(strip.max.x - MARGIN - ORIGINAL_W, strip.center().y - h / 2.0), vec2(ORIGINAL_W, h));
    if rect.min.x >= strip.min.x + t.metrics.edit_tab.x * TAB_ITEMS.len() as f32 {
        show_original(app, ui, &t, rect);
    }
}

/// Show original over `rect`: on while pressed, off once let go.
fn show_original(app: &mut App, ui: &mut Ui, t: &Tokens, rect: Rect) {
    let enabled = ui.is_enabled();
    let r = ui.interact(rect, Id::new(ORIGINAL_ID), if enabled { egui::Sense::click_and_drag() } else { egui::Sense::hover() });
    let down = enabled && r.is_pointer_button_down_on();
    let on = app.state().edit.show_original;
    let p = ui.painter();
    let fill = if down || on {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), egui::StrokeKind::Inside);
    let color = if enabled { t.text } else { t.text_disabled };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, ORIGINAL, dalica_ui_kit::fonts::semibold(t.button_label), color);
    widgets::register(ui.ctx(), ORIGINAL_ID, Role::Button, ORIGINAL, Target::Widget(&r), on);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, ORIGINAL));
    let held_id = Id::new(ORIGINAL_ID).with("held");
    let held = ui.ctx().data(|d| d.get_temp::<bool>(held_id)).unwrap_or(false);
    if down && !held {
        ui.ctx().data_mut(|d| d.insert_temp(held_id, true));
        let _ = app.run("edit.showOriginal", &json!({ "on": true }));
    } else if !down && held {
        ui.ctx().data_mut(|d| d.insert_temp(held_id, false));
        let _ = app.run("edit.showOriginal", &json!({ "on": false }));
    }
    widgets::tooltip(r, t, ORIGINAL_TIP, None);
}

/// One tab at `rect`, drawn as `widgets::tabs` draws its tabs but with an edit glyph; registered as
/// `id` (role `tab`, active when shown). Returns whether it was clicked.
fn one_tab(ui: &mut Ui, t: &Tokens, id: &str, icon: EditIcon, tip: &str, rect: Rect, on: bool) -> bool {
    let enabled = ui.is_enabled();
    let r = Tab::new(Id::new(id), tip, on).focus_stroke(Stroke::new(t.focus_width, t.accent)).show_at(ui, rect, |ui, r| {
        let p = ui.painter();
        if on {
            p.rect_filled(rect, CornerRadius { nw: t.radius, ne: t.radius, sw: 0, se: 0 }, t.chrome);
        } else if enabled && r.is_pointer_button_down_on() {
            p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(t.radius), t.pressed);
        } else if enabled && r.hovered() {
            p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
        }
        let glyph = if !enabled {
            t.text_disabled
        } else if on {
            t.pick
        } else if r.hovered() {
            t.text
        } else {
            t.icon
        };
        paint_edit(p, Rect::from_center_size(rect.center(), Vec2::splat(t.metrics.icon_toolbar)), icon, glyph);
    });
    widgets::register(ui.ctx(), id, Role::Tab, tip, Target::Widget(&r), on);
    widgets::tooltip(r, t, tip, None).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_stack_inside_the_panel_and_never_turn_inside_out() {
        let p = parts(Rect::from_min_size(pos2(0.0, 64.0), vec2(280.0, 720.0)));
        assert_eq!(p.tabs, Rect::from_min_max(pos2(0.0, 64.0), pos2(280.0, 96.0)));
        assert_eq!(p.histogram, Rect::from_min_max(pos2(8.0, 626.0), pos2(272.0, 776.0)));
        assert_eq!(p.history, Rect::from_min_max(pos2(8.0, 590.0), pos2(272.0, 618.0)));
        assert_eq!((p.content.min.y, p.content.max.y), (104.0, 582.0));
        for h in [0.0, 50.0, 200.0, f32::NAN] {
            let p = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(280.0, h)));
            for r in [p.tabs, p.content, p.history, p.histogram] {
                assert!(r.width() >= 0.0 && r.height() >= 0.0, "{h}: {r:?}");
            }
        }
    }

    #[test]
    fn five_tabs_fit_the_panel_with_unique_ids() {
        let t = Tokens::DARK;
        assert_eq!(TAB_ITEMS.len(), usize::from(t.metrics.edit_tabs));
        assert!(t.metrics.edit_tab.x * TAB_ITEMS.len() as f32 <= t.metrics.edit_panel_w);
        let ids: std::collections::HashSet<_> = TAB_ITEMS.iter().map(|i| i.1).collect();
        assert_eq!(ids.len(), TAB_ITEMS.len());
        for (tab, id, ..) in TAB_ITEMS {
            assert_eq!(serde_json::to_value(tab).unwrap(), json!(id.trim_start_matches("tab:edit.")));
        }
    }
}
