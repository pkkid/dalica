//! The edit view (`editing.md` §2): one photo, the edit top bar, the edit panel, the caption bar,
//! the optional side pane and the library's status line and bottom area. [`show`] lays the regions
//! out at the rects of [`layout::compute_edit`], registers them (`region:editTopBar`,
//! `region:editPanel`, `region:canvas`, `region:filmstrip`, `region:captionBar`, …) and hands each
//! to its module under `edit/`, so the tasks that fill them never share a file:
//!
//! | Module | Part | Filled by |
//! |---|---|---|
//! | [`top_bar`], [`filmstrip`], [`caption_bar`] | the top bar, the filmstrip in it, the caption bar | M7.10 |
//! | [`canvas`], [`zoom`], [`navigator`] | the photo, the zoom read-out, the navigator | M7.15 |
//! | [`panel`], [`basic`], [`history`], [`histogram`] | the edit panel, Basic fixes, Undo / Redo, the histogram box | M7.16 |
//! | [`crop`], [`straighten`], [`fill_light`], [`tuning`], [`retouch`] | the tools ([`crate::commands::tools::Tool`]) | M7.19–M7.22 |

pub mod basic;
pub mod canvas;
pub mod caption_bar;
pub mod crop;
pub mod fill_light;
pub mod filmstrip;
pub mod histogram;
pub mod history;
pub mod navigator;
pub mod panel;
pub mod retouch;
pub mod straighten;
pub mod top_bar;
pub mod tuning;
pub mod zoom;

use egui::{Ui, UiBuilder, vec2};

use super::{Panels, bottom, layout, light_table, menubar, region, side_pane, status};
use crate::app::App;
use crate::state::ToolKind;

/// Lays out and paints the edit view's regions, and registers them.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let l = layout::compute_edit(ui.max_rect(), app.state());
    l.register(ui.ctx());
    // The sections the menus and the title bar read, as in the library.
    light_table::refresh(p, app);
    region(ui, "menuBar", l.menu_bar, t.chrome, |ui| {
        let mut inner = ui.new_child(UiBuilder::new().max_rect(ui.max_rect().shrink2(vec2(8.0, 0.0))));
        menubar::show(p, app, &mut inner);
    });
    region(ui, "editTopBar", l.top_bar, t.chrome, |ui| top_bar::show(app, ui));
    region(ui, "filmstrip", l.filmstrip, t.chrome, |ui| filmstrip::show(app, ui));
    if let Some(rect) = l.edit_panel {
        region(ui, "editPanel", rect, t.chrome, |ui| panel::show(app, ui));
    }
    region(ui, "canvas", l.canvas, t.canvas, |ui| {
        canvas::show(app, ui);
        match app.state().edit.tool {
            Some(ToolKind::Straighten) => straighten::show(app, ui),
            Some(ToolKind::Crop) => crop::on_canvas(app, ui),
            Some(ToolKind::Retouch) => retouch::on_canvas(app, ui),
            _ => {}
        }
    });
    region(ui, "captionBar", l.caption_bar, t.canvas, |ui| caption_bar::show(app, ui));
    if let Some(rect) = l.side_pane {
        region(ui, "sidePane", rect, t.chrome, |ui| side_pane::show(app, ui));
    }
    region(ui, "statusLine", l.status_line, t.chrome, |ui| status::show(p, app, ui));
    region(ui, "bottom", l.bottom, t.chrome, |ui| bottom::show(p, app, ui));
}
