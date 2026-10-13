//! The filmstrip in the edit top bar (`editing.md` §2): ◀, the square-cropped thumbnails of the current section with the photo shown centred and outlined, ▶.
//!
//! A stub laid out by [`super`] (`panels/edit.rs`) until M7.10 fills it.

use egui::Ui;

use crate::app::App;

/// Draws this part into `ui`'s rect.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
