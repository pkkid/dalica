//! UI scale and font size (`design-system.md` §11.4–§11.5): Options ▸ General ▸ UI scale applied as
//! egui's zoom through `ui-kit`'s [`UiScale::factor`](dalica_ui_kit::theme::UiScale::factor) (Auto
//! from the operating system's scale and the window's monitor, the 4K rule), and the UI font size
//! through [`Tokens::with_font_size`]. The Options dialog's preview (`options.previewScale`,
//! `options.previewFontSize`) wins over the saved settings while the dialog is open, and reverts
//! when it closes.
//!
//! [`apply`] is called by [`App::frame`](crate::App::frame) at the start of every frame, with or
//! without a library. A stub until M10.10: it changes nothing yet, so every window is at 100 % and
//! Small.

use dalica_engine::settings::Settings;
use dalica_ui_kit::Tokens;

use crate::state::UiState;

/// Applies the UI scale and font size in effect (the dialog's preview, else `settings`, else the
/// defaults) to `ctx` and `tokens`. Cheap when nothing changed: call it every frame.
pub fn apply(_ctx: &egui::Context, _state: &UiState, _settings: Option<&Settings>, _tokens: &mut Tokens) {}
