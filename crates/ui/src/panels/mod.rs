//! The regions of the window (`ui-layout.md` §2), one module each, so the tasks that fill them never
//! share a file: the menu bar, the toolbar, the library tree (folders), the filter banner of search
//! mode ([`search_banner`]), the light table, the side pane, the status line and the bottom area;
//! the edit view's regions are in [`edit`]. [`layout`]
//! says where each one is; [`show`] gives each its rect and registers the regions.
//!
//! M11's surfaces, stubs until the tasks that draw them: the room tab strip ([`rooms`]), the Import
//! room ([`import`]), the slideshow ([`slideshow`]), the quick full-screen preview ([`peek`]) and
//! the photo viewer ([`viewer`]); compare is the edit view's (`edit::compare`). A slideshow and the
//! viewer fill the window; a room in front replaces the library's content under the tab strip;
//! the quick preview covers whatever is shown.
//!
//! M13's surfaces, stubs until the tasks that draw them: the Unnamed grid ([`faces`], in the light
//! table's region while `unnamed.open`), the People pane ([`people_pane`]) and the Places pane
//! ([`places`]) in the side pane, the face boxes on the edit view's photo (`edit::faces`) and a
//! person section's suggestion strip (`light_table::suggestions`).
//!
//! M14's surfaces, stubs until the tasks that draw them: the Collage room ([`collage`]: its Settings
//! and Clips tabs, M14.18, and its canvas, M14.19) under the tab strip while it is in front.

pub mod activity;
pub mod bottom;
pub mod collage;
pub mod edit;
pub mod faces;
pub mod folders;
pub mod import;
pub mod layout;
pub mod light_table;
pub mod menubar;
pub mod peek;
pub mod people_pane;
pub mod places;
pub mod properties;
pub mod rooms;
pub mod search_banner;
pub mod side_pane;
pub mod slideshow;
pub mod status;
pub mod tags;
pub mod thumbnails;
pub mod toolbar;
pub mod viewer;

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
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        use crate::state::{Room, View};
        peek::watch(app, ui);
        match (app.state().view, app.state().room) {
            (View::Slideshow, _) => slideshow::show(app, ui),
            (View::Viewer, _) => viewer::show(app, ui),
            (_, Room::Import | Room::Collage) => room(p, app, ui),
            (View::Library, Room::Library) => regions(p, app, ui),
            (View::Edit, Room::Library) => edit::show(p, app, ui),
        }
        // The quick preview covers whatever is shown while Ctrl+Alt is held.
        peek::show(p, app, ui);
    });
}

/// Where toasts show for `state` in `window`: at the bottom of the light table, of the photo area
/// in the edit view, of a room's content, of the whole window over a slideshow or the viewer.
pub fn toast_area(window: Rect, state: &crate::state::UiState) -> Rect {
    use crate::state::{Room, View};
    match (state.view, state.room) {
        (View::Slideshow | View::Viewer, _) => window,
        (_, Room::Import | Room::Collage) => layout::compute_room(window).room,
        (View::Library, Room::Library) => layout::compute(window, state).light_table,
        (View::Edit, Room::Library) => layout::compute_edit(window, state).toast_area(),
    }
}

/// A room in front (`ui-layout.md` §8): the menu bar, the tab strip, the room's content and the
/// status line.
fn room(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let l = layout::compute_room(ui.max_rect());
    l.register(ui.ctx());
    light_table::refresh(p, app);
    region(ui, "menuBar", l.menu_bar, t.chrome, |ui| {
        let mut inner = ui.new_child(UiBuilder::new().max_rect(ui.max_rect().shrink2(vec2(8.0, 0.0))));
        menubar::show(p, app, &mut inner);
    });
    region(ui, "tabStrip", l.tab_strip, t.chrome, |ui| rooms::show(app, ui));
    region(ui, "room", l.room, t.grid_bg, |ui| match app.state().room {
        crate::state::Room::Import => import::show(app, ui),
        crate::state::Room::Collage => collage::show(app, ui),
        crate::state::Room::Library => {}
    });
    region(ui, "statusLine", l.status_line, t.chrome, |ui| status::show(p, app, ui));
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
    if let Some(rect) = l.tab_strip {
        region(ui, "tabStrip", rect, t.chrome, |ui| rooms::show(app, ui));
    }
    region(ui, "toolbar", l.toolbar, t.chrome, |ui| toolbar::show(app, ui));
    region(ui, "tree", l.tree, t.chrome, |ui| folders::show(p, app, ui));
    // A tree click asks to scroll to its folder's section: the light table eases there.
    if p.scroll_to.take().is_some()
        && let Some(folder) = app.state().folder
    {
        let _ = app.run("section.scrollTo", &serde_json::json!({ "section": crate::state::Section::Folder(folder) }));
    }
    layout::splitter(ui, &t, &l, app.state_mut());
    // Search mode: the filter banner over the light table.
    if let Some(rect) = l.banner {
        region(ui, "banner", rect, t.filter_banner, |ui| search_banner::show(app, ui));
    }
    // The light table and its scroll bar, side by side; the Unnamed grid in their place while it
    // is open.
    if app.state().unnamed.open {
        region(ui, "lightTable", l.light_table.union(l.scrollbar), t.grid_bg, |ui| faces::show(app, ui));
    } else {
        region(ui, "lightTable", l.light_table.union(l.scrollbar), t.grid_bg, |ui| light_table::show(p, app, ui, l.light_table, l.scrollbar));
    }
    if let Some(rect) = l.side_pane {
        region(ui, "sidePane", rect, t.chrome, |ui| side_pane::show(app, ui));
    }
    region(ui, "statusLine", l.status_line, t.chrome, |ui| status::show(p, app, ui));
    region(ui, "bottom", l.bottom, t.chrome, |ui| bottom::show(p, app, ui));
}
