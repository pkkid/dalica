//! The widget registry: every widget records its automation id, role, label, rectangle and state for
//! the current pass, so agents and tests find widgets by id (`architecture.md` §7,
//! `control-protocol.md` §7) instead of by pixel position. The list belongs to the
//! [`egui::Context`]; [`widgets`] reads the last completed pass.
//!
//! Widgets register through the one helper, [`crate::widgets::register`]; [`register`] here is the
//! store underneath it. [`unregistered`] lists the interactive widgets egui drew that no id covers,
//! and [`is_valid_id`] checks the id grammar.

use std::collections::HashSet;

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
    /// A menu-bar title (`menu:File`).
    Menu,
    /// An item of an open menu (`menuitem:<command id>`).
    MenuItem,
    /// A row of the library tree (`tree:folder:<FolderId>`).
    TreeRow,
    /// A light-table cell (`thumb:<PhotoId>`).
    Thumbnail,
    /// A scroll bar's track (`scroll:track`).
    ScrollTrack,
    /// A scroll bar's handle (`scroll:thumb`).
    ScrollThumb,
    /// A widget defined outside `ui-kit` with no role of its own yet.
    Other,
}

/// One registered widget.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetInfo {
    /// The automation id, e.g. `"thumb:42"`; unique within a pass (`control-protocol.md` §7).
    pub id: String,
    pub role: Role,
    /// What a screen reader says: the label or tooltip.
    pub label: String,
    /// `[x, y, width, height]` in points, clipped to what is visible.
    pub rect: [f32; 4],
    pub enabled: bool,
    /// Toggled on, active or selected.
    pub active: bool,
    /// The egui widget this id stands for; `None` for a painted part with no input of its own (a
    /// badge, a scroll handle).
    #[serde(skip)]
    pub egui_id: Option<Id>,
}

impl WidgetInfo {
    pub fn new(id: &str, role: Role, label: &str, rect: Rect, enabled: bool, active: bool) -> Self {
        WidgetInfo {
            id: id.into(),
            role,
            label: label.into(),
            rect: [rect.min.x, rect.min.y, rect.width(), rect.height()],
            enabled,
            active,
            egui_id: None,
        }
    }

    /// The same, standing for the egui widget `id`.
    pub fn with_egui_id(self, id: Id) -> Self {
        WidgetInfo { egui_id: Some(id), ..self }
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

/// Records `info` for the current pass. Use [`crate::widgets::register`], which fills in the rect,
/// state and egui id from the widget's response. An id registered twice in one pass is kept twice,
/// so the "ids are unique" test sees it; [`find`] returns the later one.
pub fn register(ctx: &Context, info: WidgetInfo) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(key());
        if reg.pass != pass {
            reg.pass = pass;
            reg.widgets.clear();
        }
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
    widgets(ctx).into_iter().rev().find(|w| w.id == id)
}

/// An interactive widget egui drew in the last pass that no automation id covers: a bug
/// (`control-protocol.md` §7, "Rules for implementers").
#[derive(Debug, Clone, PartialEq)]
pub struct Unregistered {
    pub egui_id: Id,
    /// `[x, y, width, height]` in points, clipped to what is visible.
    pub rect: [f32; 4],
    pub enabled: bool,
}

/// The widgets of the last completed pass that take clicks, drags or keyboard focus, are visible,
/// and have no registered automation id; sorted top to bottom, left to right. Call it between
/// passes. Not counted: a widget clipped out of sight, and a floating area's own background (the
/// `move` handle egui gives every area, e.g. an open menu's box), which is a container, not a
/// control.
pub fn unregistered(ctx: &Context) -> Vec<Unregistered> {
    let registered: HashSet<Id> = widgets(ctx).iter().filter_map(|w| w.egui_id).collect();
    let mut out: Vec<Unregistered> = ctx.viewport(|v| {
        v.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, list)| list.iter())
            .filter(|w| (w.sense.interactive() || w.sense.is_focusable()) && w.interact_rect.is_positive())
            .filter(|w| w.id != w.layer_id.id.with("move") && !registered.contains(&w.id))
            .map(|w| {
                let r = w.interact_rect;
                Unregistered { egui_id: w.id, rect: [r.min.x, r.min.y, r.width(), r.height()], enabled: w.enabled }
            })
            .collect()
    });
    out.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]).then(a.rect[0].total_cmp(&b.rect[0])));
    out
}

/// Whether `id` follows the grammar of `control-protocol.md` §7: `kind ":" name ("/" part)*`, with
/// `kind` lowercase ASCII letters and `name` and each `part` non-empty (a name may itself hold
/// colons: `tree:folder:12`); or a bare region name in lower camel case (`tray`, `lightTable`).
/// Never empty, never with surrounding whitespace or control characters.
pub fn is_valid_id(id: &str) -> bool {
    if id.is_empty() || id.trim() != id || id.chars().any(char::is_control) {
        return false;
    }
    match id.split_once(':') {
        None => id.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && id.chars().all(|c| c.is_ascii_alphabetic()),
        Some((kind, rest)) => !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase()) && rest.split('/').all(|part| !part.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_the_spec_grammar() {
        for good in [
            "menu:File",
            "menuitem:folders.add",
            "button:star",
            "thumb:42",
            "thumb:42/star",
            "tree:folder:7",
            "section:folder:12/title",
            "slider:tuning.fill_light",
            "tag:Summer trip",
            "scroll:thumb",
            "tray",
            "lightTable",
        ] {
            assert!(is_valid_id(good), "{good}");
        }
        for bad in [
            "",
            " button:star",
            "button:star ",
            "menu.file",
            "menu.file.addFolder",
            "bottom.star",
            "Menu:File",
            "menu:",
            ":File",
            "thumb:42/",
            "thumb:42//star",
            "button:st\nar",
            "LightTable",
            "light_table",
        ] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_duplicate_id_is_kept_so_tests_see_it_and_find_returns_the_later_one() {
        let ctx = Context::default();
        let r = |x: f32| Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(10.0, 10.0));
        let pass = |ui: &mut dyn FnMut(&mut egui::Ui)| ctx.run_ui(egui::RawInput::default(), |u| ui(u)).textures_delta.clear();
        pass(&mut |ui| {
            register(ui.ctx(), WidgetInfo::new("button:a", Role::Button, "A", r(0.0), true, false));
            register(ui.ctx(), WidgetInfo::new("button:a", Role::Button, "A", r(20.0), true, false));
        });
        let list = widgets(&ctx);
        assert_eq!(list.iter().filter(|w| w.id == "button:a").count(), 2);
        assert_eq!(find(&ctx, "button:a").map(|w| w.rect[0]), Some(20.0));
        assert_eq!(find(&ctx, "button:b"), None);
        // The next pass starts a new list.
        pass(&mut |_| {});
        assert!(widgets(&ctx).is_empty());
    }
}
