//! The regions of the window (`ui-layout.md` §2), one module each. M1 has the plain library window:
//! a menu bar, the folder list, the light table, the status line and the bottom bar.

pub mod bottom;
pub mod folders;
pub mod light_table;
pub mod menubar;
pub mod thumbnails;

use std::sync::Arc;

use egui::{Frame, Panel, Ui};

use crate::app::App;
use crate::picker::{Pending, Picker};

/// Panel state that is not view state: caches, the open dialog, a pending scroll.
pub struct Panels {
    pub thumbnails: thumbnails::Thumbnails,
    pub sections: light_table::Sections,
    pub picker: Option<Arc<dyn Picker>>,
    pub dialog: Option<Pending>,
    /// Scroll the light table to this offset on the next frame.
    pub scroll_to: Option<f32>,
}

impl Panels {
    pub fn new(picker: Option<Arc<dyn Picker>>) -> Panels {
        Panels { thumbnails: thumbnails::Thumbnails::new(), sections: Default::default(), picker, dialog: None, scroll_to: None }
    }

    /// Nothing visible is still on its way: no thumbnail, no open dialog.
    pub fn idle(&self) -> bool {
        self.thumbnails.pending() == 0 && self.dialog.is_none()
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

/// Lays out and paints every region.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    p.thumbnails.poll(ui.ctx());
    if let Some(answer) = p.dialog.as_ref().and_then(Pending::poll) {
        p.dialog = None;
        if let Some(path) = answer {
            let _ = app.run("folders.add", &serde_json::json!({ "path": path }));
        }
    }
    let menubar_frame = Frame::new().fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 0));
    Panel::top("menubar").exact_size(menubar::HEIGHT).frame(menubar_frame).show(ui, |ui| menubar::show(p, app, ui));
    Panel::bottom("bottom").exact_size(bottom::HEIGHT).frame(Frame::new().fill(t.chrome)).show(ui, |ui| bottom::show(app, ui));
    Panel::bottom("status").exact_size(bottom::STATUS_HEIGHT).frame(Frame::new().fill(t.chrome)).show(ui, |ui| bottom::status(p, app, ui));
    Panel::left("folders").resizable(false).exact_size(folders::WIDTH).frame(Frame::new().fill(t.chrome)).show(ui, |ui| folders::show(p, app, ui));
    egui::CentralPanel::default().frame(Frame::new().fill(t.grid_bg)).show(ui, |ui| light_table::show(p, app, ui));
}
