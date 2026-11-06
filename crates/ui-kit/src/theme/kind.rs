//! The themes Dalica can wear (`design-system.md` §2, Themes): [`ThemeKind`], its ids, labels and
//! the names it is known by.
//!
//! Ported from PhotoCraft `crates/ui-egui/src/theme.rs` at revision 3a896a5e (`ThemeKind`, `ALL`,
//! `id`, `label`, `from_name`, `next`; MIT OR Apache-2.0, see `NOTICE`), with Dalica's own Dark
//! first: it is the default and owns the name `dark`. PhotoCraft's aliases are kept except `dark`
//! and `light` and those that name another company's product.

/// A theme: Dalica's Dark (the default, LightCraft's look) or one of PhotoCraft's fourteen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeKind {
    /// Dalica's dark grey theme, `Tokens::DARK` value for value.
    #[default]
    Dark,
    /// The darkest of the four Pro brightness levels: flat charcoal panels, a blue accent.
    Pro,
    /// The second Pro brightness level: mid-grey panels.
    ProMedium,
    /// The third Pro brightness level: light grey panels, dark text.
    ProLightGray,
    /// The lightest Pro brightness level: near-white panels, dark text.
    ProLight,
    /// Near-black surfaces, rounded corners, a soft violet accent.
    Studio,
    /// Studio on light surfaces.
    StudioLight,
    /// Grey bevels, square corners and a navy selection, as desktops looked around 2000.
    Classic,
    /// GNOME's libadwaita light style.
    Adwaita,
    /// GNOME's libadwaita dark style.
    AdwaitaDark,
    /// Ethan Schoonover's Solarized palette, dark.
    SolarizedDark,
    /// An adaptation of KDE Plasma's Breeze light look.
    BreezeLight,
    /// An adaptation of KDE Plasma's Breeze dark look.
    BreezeDark,
    /// Muted violet-grey surfaces (never near-black), softened off-white text, a periwinkle accent.
    SoftDark,
    /// Pale violet-grey surfaces, softened dark-violet text, a periwinkle accent.
    SoftLight,
}

impl ThemeKind {
    /// Every theme, in the order the menu and the Options row list them; [`next`](Self::next) goes
    /// round it.
    pub const ALL: [ThemeKind; 15] = [
        ThemeKind::Dark,
        ThemeKind::Pro,
        ThemeKind::ProMedium,
        ThemeKind::ProLightGray,
        ThemeKind::ProLight,
        ThemeKind::Studio,
        ThemeKind::StudioLight,
        ThemeKind::Classic,
        ThemeKind::Adwaita,
        ThemeKind::AdwaitaDark,
        ThemeKind::SolarizedDark,
        ThemeKind::BreezeLight,
        ThemeKind::BreezeDark,
        ThemeKind::SoftDark,
        ThemeKind::SoftLight,
    ];

    /// The canonical name: the setting's value (`interface.theme`), the command `theme.<id>` and
    /// `ui.set {theme}`.
    pub fn id(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Pro => "pro",
            ThemeKind::ProMedium => "proMedium",
            ThemeKind::ProLightGray => "proLightGray",
            ThemeKind::ProLight => "proLight",
            ThemeKind::Studio => "studio",
            ThemeKind::StudioLight => "studioLight",
            ThemeKind::Classic => "classic",
            ThemeKind::Adwaita => "adwaita",
            ThemeKind::AdwaitaDark => "adwaitaDark",
            ThemeKind::SolarizedDark => "solarizedDark",
            ThemeKind::BreezeLight => "breezeLight",
            ThemeKind::BreezeDark => "breezeDark",
            ThemeKind::SoftDark => "softDark",
            ThemeKind::SoftLight => "softLight",
        }
    }

    /// The name the menu and the Options row show.
    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark",
            ThemeKind::Pro => "Pro (Dark)",
            ThemeKind::ProMedium => "Pro (Medium Gray)",
            ThemeKind::ProLightGray => "Pro (Light Gray)",
            ThemeKind::ProLight => "Pro (Light)",
            ThemeKind::Studio => "Studio (Dark)",
            ThemeKind::StudioLight => "Studio (Light)",
            ThemeKind::Classic => "Classic",
            ThemeKind::Adwaita => "Adwaita (Light)",
            ThemeKind::AdwaitaDark => "Adwaita (Dark)",
            ThemeKind::SolarizedDark => "Solarized Dark",
            ThemeKind::BreezeLight => "Breeze Light",
            ThemeKind::BreezeDark => "Breeze Dark",
            ThemeKind::SoftDark => "Soft Dark",
            ThemeKind::SoftLight => "Soft Light",
        }
    }

    /// The theme after this one in [`ALL`](Self::ALL), the last coming back to the first.
    pub fn next(self) -> ThemeKind {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL.get((i + 1) % Self::ALL.len()).copied().unwrap_or_default()
    }

    /// The theme `name` names: its id, its label or an alias, ignoring case, spaces, `_`, `-` and
    /// brackets (`"Pro (Medium Gray)"`, `"proMedium"`, `"mediumgray"`). `None` for anything else.
    pub fn from_name(name: &str) -> Option<ThemeKind> {
        let key: String = name.chars().filter(|c| !matches!(c, ' ' | '_' | '-' | '(' | ')')).flat_map(char::to_lowercase).collect();
        Some(match key.as_str() {
            "dark" => ThemeKind::Dark,
            "pro" | "prodark" => ThemeKind::Pro,
            "promedium" | "promediumgray" | "medium" | "mediumgray" => ThemeKind::ProMedium,
            "prolightgray" | "lightgray" | "mediumlight" => ThemeKind::ProLightGray,
            "prolight" | "prolightest" | "lightest" => ThemeKind::ProLight,
            "studio" | "studiodark" => ThemeKind::Studio,
            "studiolight" => ThemeKind::StudioLight,
            "classic" | "retro" => ThemeKind::Classic,
            "adwaita" | "adwaitalight" => ThemeKind::Adwaita,
            "adwaitadark" => ThemeKind::AdwaitaDark,
            "solarizeddark" | "solarized" => ThemeKind::SolarizedDark,
            "breezelight" | "breeze" => ThemeKind::BreezeLight,
            "breezedark" => ThemeKind::BreezeDark,
            "softdark" => ThemeKind::SoftDark,
            "softlight" => ThemeKind::SoftLight,
            _ => return None,
        })
    }
}
