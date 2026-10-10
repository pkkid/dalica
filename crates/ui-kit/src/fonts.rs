//! Inter Regular and SemiBold (OFL, `assets/fonts/`), compiled in and installed by [`install`].
//! egui's own default fonts are off: every glyph comes from these files.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};

const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// The family name of Inter SemiBold.
pub const SEMIBOLD_FAMILY: &str = "semibold";

/// Our font set: Inter for proportional and (until a mono face is needed) monospace text, Inter
/// SemiBold as the named family [`SEMIBOLD_FAMILY`].
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    defs.font_data.insert("Inter-Regular".into(), Arc::new(FontData::from_static(REGULAR)));
    defs.font_data.insert("Inter-SemiBold".into(), Arc::new(FontData::from_static(SEMIBOLD)));
    defs.families.insert(FontFamily::Proportional, vec!["Inter-Regular".into()]);
    defs.families.insert(FontFamily::Monospace, vec!["Inter-Regular".into()]);
    defs.families.insert(FontFamily::Name(SEMIBOLD_FAMILY.into()), vec!["Inter-SemiBold".into(), "Inter-Regular".into()]);
    defs
}

/// Installs [`definitions`]; they take effect on the next pass.
pub fn install(ctx: &Context) {
    ctx.set_fonts(definitions());
}

/// Inter SemiBold at `size` points.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD_FAMILY.into()))
}

/// Inter Regular at `size` points.
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_faces_are_real_truetype_files() {
        for font in [REGULAR, SEMIBOLD] {
            assert!(font.len() > 100_000);
            assert_eq!(font.get(..4), Some(&[0, 1, 0, 0][..]), "a TrueType header");
        }
        let defs = definitions();
        assert_eq!(defs.font_data.len(), 2);
        assert!(defs.families.contains_key(&FontFamily::Name(SEMIBOLD_FAMILY.into())));
    }
}
