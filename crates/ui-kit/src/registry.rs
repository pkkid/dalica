//! The widget registry: every `ui-kit` widget records its automation id, role, label, rectangle and
//! state for the current pass, so agents and tests find widgets by id (`architecture.md` §8) instead
//! of by pixel position. The list belongs to the [`egui::Context`]; [`widgets`] reads the last
//! completed pass.

use egui::{Context, Id, Rect};
use serde::Serialize;

/// What kind of control a widget is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Button,
    IconButton,
    Toggle,
    Badge,
    /// A widget defined outside `ui-kit` (a thumbnail cell, a tree row) that registers itself.
    Other,
}

/// One registered widget.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetInfo {
    /// The automation id, e.g. `"library.star"`; unique within a pass.
    pub id: String,
    pub role: Role,
    /// What a screen reader says: the label or tooltip.
    pub label: String,
    /// `[x, y, width, height]` in points.
    pub rect: [f32; 4],
    pub enabled: bool,
    /// Toggled on, active or selected.
    pub active: bool,
}

impl WidgetInfo {
    pub fn new(id: &str, role: Role, label: &str, rect: Rect, enabled: bool, active: bool) -> Self {
        WidgetInfo { id: id.into(), role, label: label.into(), rect: [rect.min.x, rect.min.y, rect.width(), rect.height()], enabled, active }
    }

    /// The centre of the widget, where a click lands.
    pub fn center(&self) -> egui::Pos2 {
        egui::pos2(self.rect[0] + self.rect[2] / 2.0, self.rect[1] + self.rect[3] / 2.0)
    }
}

#[derive(Debug, Clone, Default)]
struct Registry {
    pass: u64,
    widgets: Vec<WidgetInfo>,
}

fn key() -> Id {
    Id::new("dalica-ui-kit.registry")
}

/// Records `info` for the current pass. A widget registered twice in one pass keeps its last entry.
pub fn register(ctx: &Context, info: WidgetInfo) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(key());
        if reg.pass != pass {
            reg.pass = pass;
            reg.widgets.clear();
        }
        reg.widgets.retain(|w| w.id != info.id);
        reg.widgets.push(info);
    });
}

/// The widgets registered in the current pass or, between passes, the one that just ended, in
/// paint order. Empty if that pass registered none.
pub fn widgets(ctx: &Context) -> Vec<WidgetInfo> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Registry>(key()).filter(|r| r.pass.saturating_add(1) >= pass).map(|r| r.widgets).unwrap_or_default())
}

/// The widget with automation id `id`, if the last pass registered one.
pub fn find(ctx: &Context, id: &str) -> Option<WidgetInfo> {
    widgets(ctx).into_iter().find(|w| w.id == id)
}
