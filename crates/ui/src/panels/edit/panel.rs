//! The edit panel (`editing.md` §2, §5): the five icon tabs, the tab's content (or the open tool's
//! panel), the Undo / Redo pair and the Histogram & Camera Info box, top to bottom.
//!
//! The frame lays the parts out ([`parts`]) and hands each to its module; M7.16 draws the tabs and
//! fills the parts. While a modal tool is open (Crop, Retouch) its panel replaces the content, and
//! the rest of the panel is drawn disabled.

use egui::{Rect, Ui, UiBuilder, pos2};

use super::{basic, crop, histogram, history, retouch, tuning};
use crate::app::App;
use crate::commands::tools;
use crate::state::{EditTab, ToolKind};

/// The tab row's height and its margin from the panel's top; the Undo / Redo row's height; the
/// histogram box's height; the margin between the parts and around them.
pub const TABS: f32 = 32.0;
pub const HISTORY: f32 = 28.0;
pub const HISTOGRAM: f32 = 150.0;
pub const MARGIN: f32 = 8.0;

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

/// The parts of a panel `rect`: the tabs at the top, the histogram box at the bottom, Undo / Redo
/// above it, the content between. A panel too short for them squeezes the content first.
pub fn parts(rect: Rect) -> Parts {
    let (l, r) = (rect.min.x + MARGIN, rect.max.x - MARGIN);
    let tabs = span(l, r, rect.min.y + MARGIN, rect.min.y + MARGIN + TABS);
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
    let open = tools::open_tool(app);
    let free = open.is_none_or(|t| !t.modal());
    part(ui, "tabs", p.tabs, free, |_ui| {});
    part(ui, "content", p.content, true, |ui| match (open.map(|t| t.kind()), app.state().edit.tab) {
        (Some(ToolKind::Crop), _) => crop::show(app, ui),
        (Some(ToolKind::Retouch), _) => retouch::show(app, ui),
        (Some(ToolKind::Straighten), _) => {
            ui.disable();
            basic::show(app, ui);
        }
        (_, EditTab::Basic) => basic::show(app, ui),
        (_, EditTab::Tuning) => tuning::show(app, ui),
        // The effect tabs (M12) show an empty state until then.
        (_, EditTab::Effects | EditTab::Effects2 | EditTab::Effects3) => {}
    });
    part(ui, "history", p.history, free, |ui| history::show(app, ui));
    part(ui, "histogram", p.histogram, true, |ui| histogram::show(app, ui));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_parts_stack_inside_the_panel_and_never_turn_inside_out() {
        let p = parts(Rect::from_min_size(pos2(0.0, 64.0), vec2(280.0, 720.0)));
        assert_eq!(p.tabs, Rect::from_min_max(pos2(8.0, 72.0), pos2(272.0, 104.0)));
        assert_eq!(p.histogram, Rect::from_min_max(pos2(8.0, 626.0), pos2(272.0, 776.0)));
        assert_eq!(p.history, Rect::from_min_max(pos2(8.0, 590.0), pos2(272.0, 618.0)));
        assert_eq!((p.content.min.y, p.content.max.y), (112.0, 582.0));
        for h in [0.0, 50.0, 200.0, f32::NAN] {
            let p = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(280.0, h)));
            for r in [p.tabs, p.content, p.history, p.histogram] {
                assert!(r.width() >= 0.0 && r.height() >= 0.0, "{h}: {r:?}");
            }
        }
    }
}
