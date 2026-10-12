//! The regions of the window (`ui-layout.md` §2), one module each, so the tasks that fill them never
//! share a file: the menu bar, the toolbar, the library tree (folders), the light table, the side
//! pane, the status line and the bottom area. [`layout`] says where each one is; [`show`] gives
//! each its rect and registers the regions.

pub mod activity;
pub mod bottom;
pub mod folders;
pub mod layout;
pub mod light_table;
pub mod menubar;
pub mod side_pane;
pub mod status;
pub mod thumbnails;
pub mod toolbar;

use std::sync::Arc;

use egui::{Rect, Ui, UiBuilder, vec2};

use crate::app::App;
use crate::picker::{Pending, Picker};

/// Panel state that is not view state: caches, the open dialog, a pending scroll.
pub struct Panels {
    pub thumbnails: thumbnails::Thumbnails,
    /// The light table's sections, in tree order (also read by the tree pane and the title bar).
    pub sections: light_table::Sections,
    /// The light table's layout, scroll position and parts.
    pub light_table: light_table::LightTable,
    pub picker: Option<Arc<dyn Picker>>,
    pub dialog: Option<Pending>,
    /// Set by a click on a tree row: scroll the light table to the selected folder's section
    /// (`section.scrollTo`) this frame. The value itself is not used.
    pub scroll_to: Option<f32>,
    /// The context the panels were last drawn in, for [`Panels::unregistered_widgets`].
    ctx: Option<egui::Context>,
}

impl Panels {
    pub fn new(picker: Option<Arc<dyn Picker>>) -> Panels {
        Panels {
            thumbnails: thumbnails::Thumbnails::new(),
            sections: Default::default(),
            light_table: Default::default(),
            picker,
            dialog: None,
            scroll_to: None,
            ctx: None,
        }
    }

    /// Nothing visible is still on its way: no thumbnail, no open dialog, no scroll animation.
    pub fn idle(&self) -> bool {
        self.thumbnails.pending() == 0 && self.dialog.is_none() && !self.light_table.animating()
    }

    /// The interactive widgets of the last frame that have no automation id (`control-protocol.md`
    /// §7): always empty unless a panel forgot to register one. Call it between frames.
    pub fn unregistered_widgets(&self) -> Vec<dalica_ui_kit::registry::Unregistered> {
        self.ctx.as_ref().map(dalica_ui_kit::registry::unregistered).unwrap_or_default()
    }

    /// Opens the folder dialog (File ▸ Add Folder…) unless one is open or there is no picker.
    pub fn add_folder(&mut self) {
        if self.dialog.is_none()
            && let Some(picker) = &self.picker
        {
            self.dialog = Pending::pick_folder(picker);
        }
    }
}

/// Runs `add` in a child of `ui` that fills `rect` (clipped to it), painted `fill` first. A region
/// with no area is skipped.
fn region(ui: &mut Ui, name: &'static str, rect: Rect, fill: egui::Color32, add: impl FnOnce(&mut Ui)) {
    if !rect.is_positive() {
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt(("region", name)).max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.painter().rect_filled(rect, 0, fill);
    add(&mut child);
}

/// Lays out and paints every region at the rects of [`layout::compute`], and registers them.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    if p.ctx.is_none() {
        p.ctx = Some(ui.ctx().clone());
    }
    let t = *app.tokens();
    if let Some(session) = app.session_mut() {
        p.thumbnails.begin_frame(ui.ctx(), session, &t);
    }
    if let Some(answer) = p.dialog.as_ref().and_then(Pending::poll) {
        p.dialog = None;
        if let Some(path) = answer {
            let _ = app.run("folders.add", &serde_json::json!({ "path": path }));
        }
    }
    // One central panel over the whole window, so egui counts every point as used (the pointer is
    // over egui anywhere); the regions are laid out inside it at the layout's rects.
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| regions(p, app, ui));
}

fn regions(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let l = layout::compute(ui.max_rect(), app.state());
    l.register(ui.ctx());
    // The sections every region reads, in tree order, before any region is drawn.
    light_table::refresh(p, app);
    region(ui, "menuBar", l.menu_bar, t.chrome, |ui| {
        let mut inner = ui.new_child(UiBuilder::new().max_rect(ui.max_rect().shrink2(vec2(8.0, 0.0))));
        menubar::show(p, app, &mut inner);
    });
    region(ui, "toolbar", l.toolbar, t.chrome, |ui| toolbar::show(app, ui));
    region(ui, "tree", l.tree, t.chrome, |ui| folders::show(p, app, ui));
    // A tree click asks to scroll to its folder's section: the light table eases there.
    if p.scroll_to.take().is_some()
        && let Some(folder) = app.state().folder
    {
        let _ = app.run("section.scrollTo", &serde_json::json!({ "section": crate::state::Section::Folder(folder) }));
    }
    layout::splitter(ui, &t, &l, app.state_mut());
    // The light table and its scroll bar, side by side.
    region(ui, "lightTable", l.light_table.union(l.scrollbar), t.grid_bg, |ui| light_table::show(p, app, ui, l.light_table, l.scrollbar));
    if let Some(rect) = l.side_pane {
        region(ui, "sidePane", rect, t.chrome, |ui| side_pane::show(app, ui));
    }
    region(ui, "statusLine", l.status_line, t.chrome, |ui| status::show(p, app, ui));
    region(ui, "bottom", l.bottom, t.chrome, |ui| bottom::show(p, app, ui));
}
