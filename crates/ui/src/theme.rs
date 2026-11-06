//! The theme the window wears (`design-system.md` §2, Themes; `menus-and-shortcuts.md` §1.3 and
//! §5 General), read every frame beside the UI scale and font size ([`crate::scale`]):
//!
//! - **In effect** ([`in_effect`]): the Options dialog's preview (`options.previewTheme`) while the
//!   dialog is open and its draft holds a changed Theme, else the saved `interface.theme`, else
//!   Dark; the id goes through `ui-kit`'s [`ThemeKind::from_name`], and an id it does not know
//!   reads as Dark. Apply empties the draft, so the preview ends with it (the saved theme is then
//!   the same one); Cancel and Esc drop the draft, so the preview reverts.
//! - **Switching** ([`apply`]): when the theme in effect is not the one the tokens wear,
//!   `Tokens::for_kind(kind).with_font_size(…)` (the font size in effect, so a theme change never
//!   compounds with it, and a later font size change keeps the theme) is applied to the context
//!   once and a repaint asked for: the next frame is drawn in the new theme, with no restart.
//!   Nothing in Dalica keeps a theme's colours past a frame (every widget, badge plate and icon is
//!   painted from [`Tokens::get`] or the tokens passed each frame, and egui tessellates every
//!   frame), so there is no cache to drop; a cache that bakes token colours must be keyed by
//!   [`Tokens::kind`]. A frame without a change costs one comparison.
//! - **The menu** (View ▸ Theme ▸, [`checked`]): a radio row per theme of [`ThemeKind::ALL`],
//!   running the engine's `theme.<id>`; the row of the theme in effect is checked.
//!
//! The engine holds the themes as a settings choice ([`THEMES`]) and never depends on `ui-kit`;
//! [`same_themes`] says whether the two lists agree, which a test checks.

use dalica_engine::Session;
use dalica_engine::settings::{Settings, THEME_COMMANDS, THEME_DEFAULT, THEMES};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::theme::ThemeKind;
use egui::Context;

use crate::app::App;
use crate::state::UiState;

/// The settings key of the theme (Options ▸ General ▸ Interface ▸ Theme).
pub const THEME: &str = "interface.theme";

/// The theme `id` names (an id, a label or an alias, as [`ThemeKind::from_name`] reads it); Dark
/// for one `ui-kit` does not know.
pub fn kind(id: &str) -> ThemeKind {
    ThemeKind::from_name(id).unwrap_or_default()
}

/// The theme in effect: the Options dialog's preview while its draft holds a changed Theme, else
/// `settings`' `interface.theme`, else Dark.
pub fn in_effect(state: &UiState, settings: Option<&Settings>) -> ThemeKind {
    let preview = state.options.as_ref().filter(|o| o.values.contains_key(THEME)).and_then(|o| o.preview_theme.as_deref());
    kind(preview.unwrap_or_else(|| settings.map_or(THEME_DEFAULT, |s| s.interface().theme)))
}

/// Puts the theme in effect ([`in_effect`]) on `ctx` and `tokens` when it is not the one they wear,
/// at the font size in effect, and asks for a repaint; returns whether it changed. Cheap when
/// nothing changed: call it every frame, before [`crate::scale::apply`].
pub fn apply(ctx: &Context, state: &UiState, settings: Option<&Settings>, tokens: &mut Tokens) -> bool {
    let want = in_effect(state, settings);
    if tokens.kind == want {
        return false;
    }
    let (_, font) = crate::scale::in_effect(state, settings);
    *tokens = Tokens::for_kind(want).with_font_size(font.font_size());
    tokens.apply(ctx);
    ctx.request_repaint();
    true
}

/// Whether the View ▸ Theme ▸ row of command `id` is checked: `Some(true)` for the `theme.<id>` of
/// the theme in effect, `Some(false)` for the other themes' commands, `None` for any other command.
pub fn checked(app: &App, id: &str) -> Option<bool> {
    let place = THEME_COMMANDS.iter().position(|(command, _)| *command == id)?;
    let (theme, _) = THEMES.get(place)?;
    Some(in_effect(app.state(), app.session().map(Session::settings)).id() == *theme)
}

/// Whether the engine's themes ([`THEMES`]) are `ui-kit`'s ([`ThemeKind::ALL`]): the same ids and
/// labels in the same order.
pub fn same_themes() -> bool {
    THEMES.len() == ThemeKind::ALL.len() && THEMES.iter().zip(ThemeKind::ALL).all(|((id, label), k)| *id == k.id() && *label == k.label())
}
