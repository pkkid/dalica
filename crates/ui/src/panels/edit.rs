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
//! | [`compare`] | the compare modes' second side and before/after | M11.16 |
//! | [`effects`] | the effect tabs' tile grids | M12.15 |
//! | [`effect_panel`] | an effect's parameter panel ([`crate::commands::effect_session`]) | M12.16 |
//! | [`crosshair`], [`soften_brush`] | an open effect's crosshair and Soften's brush on the photo | M12.17, M12.18 |
//! | [`faces`] | the face boxes and Add a Person Manually's box on the photo (no tool open) | M13.24 |
//! | [`star_flight`] | the star's flight from the bottom area's star button (kept going by [`canvas`]) | M16.35 |
//! | [`words`] | the edit catalogue's words in the language in effect | M15.33 |
//!
//! While a room is open the room tab strip sits under the menu bar (`ui-layout.md` §8): the edit
//! view is the Library tab's content.
//!
//! A slideshow started here plays in the photo's place (`viewing.md` §1.2, M16.32): the regions
//! around it are drawn dimmed and inert (the menu bar aside) and the show over the canvas.

pub mod basic;
pub mod canvas;
pub mod caption_bar;
pub mod compare;
pub mod crop;
pub mod crosshair;
pub mod effect_panel;
pub mod effects;
pub mod faces;
pub mod fill_light;
pub mod filmstrip;
pub mod histogram;
pub mod history;
pub mod navigator;
pub mod panel;
pub mod retouch;
pub mod soften_brush;
pub mod star_flight;
pub mod straighten;
pub mod top_bar;
pub mod tuning;
pub mod video;
pub mod words;
pub mod zoom;

use egui::{Ui, UiBuilder, vec2};

use super::{Panels, around_show, bottom, dim, layout, light_table, menubar, region, rooms, side_pane, slideshow_region, status};
use crate::app::App;
use crate::state::ToolKind;

/// Lays out and paints the edit view's regions, and registers them; with `slideshow`, a show plays
/// in the photo's place and the other regions (the menu bar aside) are dimmed and inert.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui, slideshow: bool) {
    let t = *app.tokens();
    let mut l = layout::compute_edit(ui.max_rect(), app.state());
    // A collage's Edit Collage sits left of the compare toggle: the filmstrip gives way (M14.20).
    if top_bar::shows_edit_collage(app) {
        l.filmstrip.max.x = (l.filmstrip.max.x - top_bar::EDIT_COLLAGE_W - top_bar::GAP).max(l.filmstrip.min.x);
    }
    l.register(ui.ctx());
    // The sections the menus and the title bar read, as in the library.
    light_table::refresh(p, app);
    region(ui, "menuBar", l.menu_bar, t.chrome, |ui| {
        let mut inner = ui.new_child(UiBuilder::new().max_rect(ui.max_rect().shrink2(vec2(8.0, 0.0))));
        menubar::show(p, app, &mut inner);
    });
    let outer = ui;
    let ui = &mut around_show(outer, slideshow);
    if let Some(rect) = l.tab_strip {
        region(ui, "tabStrip", rect, t.chrome, |ui| rooms::show(app, ui));
    }
    region(ui, "editTopBar", l.top_bar, t.chrome, |ui| top_bar::show(app, ui));
    region(ui, "filmstrip", l.filmstrip, t.chrome, |ui| filmstrip::show(app, ui));
    if let Some(rect) = l.edit_panel {
        region(ui, "editPanel", rect, t.chrome, |ui| panel::show(app, ui));
    }
    // The show is drawn last, over the dimming.
    let canvas = if slideshow { egui::Rect::NOTHING } else { l.canvas };
    region(ui, "canvas", canvas, t.canvas, |ui| {
        canvas::show(app, ui);
        compare::show(app, ui);
        match app.state().edit.tool {
            Some(ToolKind::Straighten) => straighten::show(app, ui),
            Some(ToolKind::Crop) => crop::on_canvas(app, ui),
            Some(ToolKind::Retouch) => retouch::on_canvas(app, ui),
            // An open effect's overlays: each draws only for the effects it serves.
            Some(ToolKind::Effect) => {
                crosshair::on_canvas(app, ui);
                soften_brush::on_canvas(app, ui);
            }
            _ => {}
        }
        faces::on_canvas(app, ui);
    });
    region(ui, "captionBar", l.caption_bar, t.canvas, |ui| caption_bar::show(app, ui));
    if let Some(rect) = l.side_pane {
        region(ui, "sidePane", rect, t.chrome, |ui| side_pane::show(app, ui));
    }
    region(ui, "statusLine", l.status_line, t.chrome, |ui| status::show(p, app, ui));
    region(ui, "bottom", l.bottom, t.chrome, |ui| bottom::show(p, app, ui));
    if slideshow {
        dim(outer, &t, l.menu_bar.max.y);
        slideshow_region(app, outer, l.canvas);
    }
}
