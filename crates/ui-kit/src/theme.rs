//! Design tokens (`docs/design-system.md` §2). Every colour, metric and font size the UI paints with
//! comes from [`Tokens`]; widgets never hard-code a colour. M0 has one token; M1.16 adds the rest.

use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    /// `#2d2d2d`: toolbar, side panes, bottom tray area, menu bar, dialogs (`panel_fill`, `window_fill`).
    pub chrome: Color32,
}

impl Tokens {
    /// The dark theme, the only theme until after 1.0.
    pub const DARK: Tokens = Tokens { chrome: Color32::from_rgb(0x2d, 0x2d, 0x2d) };
}

impl Default for Tokens {
    fn default() -> Self {
        Tokens::DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_matches_the_spec() {
        assert_eq!(Tokens::default().chrome.to_array(), [0x2d, 0x2d, 0x2d, 0xff]);
    }
}
