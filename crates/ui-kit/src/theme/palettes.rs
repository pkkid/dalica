//! The palettes of PhotoCraft's twelve themes, a file per family (`design-system.md` §2, Themes),
//! so two tasks can port them side by side. Each family file answers [`theme`] for its own kinds;
//! a kind whose file has not been filled yet answers `None`, and [`Tokens::for_kind`] gives it
//! Dark's values until it is.
//!
//! [`Tokens::for_kind`]: super::Tokens::for_kind

mod adwaita;
mod breeze;
mod classic;
mod pro;
mod solarized;
mod studio;

use super::{Theme, ThemeKind};

/// Theme `kind`'s palette and overrides, or `None` for Dark (which has no palette: it is
/// `Tokens::DARK`) and for a theme not ported yet.
pub fn theme(kind: ThemeKind) -> Option<Theme> {
    match kind {
        ThemeKind::Dark => None,
        ThemeKind::Pro | ThemeKind::ProMedium | ThemeKind::ProLightGray | ThemeKind::ProLight => pro::theme(kind),
        ThemeKind::Studio | ThemeKind::StudioLight => studio::theme(kind),
        ThemeKind::Classic => classic::theme(kind),
        ThemeKind::Adwaita | ThemeKind::AdwaitaDark => adwaita::theme(kind),
        ThemeKind::SolarizedDark => solarized::theme(kind),
        ThemeKind::BreezeLight | ThemeKind::BreezeDark => breeze::theme(kind),
    }
}
