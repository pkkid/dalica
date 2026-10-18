//! Face boxes on the photo in the edit view (`organizing.md` §6.4): each face of the photo shown as
//! ui-kit's `face_box`, mapped from the stored frame through the preview's geometry, highlighted
//! while its face is hovered in the People pane (`peoplePane.hovered`) and back; and Add a Person
//! Manually's box (`peoplePane.manual`) with "Drag the box to fit the face, then type a name." and
//! its name field.
//!
//! M13.4's stub: its place among the canvas's overlays; it draws nothing until M13.24.

use egui::Ui;

use crate::app::App;

/// Draws the face boxes over the photo area `ui`.
pub fn on_canvas(_app: &mut App, _ui: &mut Ui) {}
