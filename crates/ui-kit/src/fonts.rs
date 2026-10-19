//! The UI's fonts: Inter Regular and SemiBold (OFL, `assets/fonts/`), compiled in, and after them
//! the fallback faces of the optional craft-fonts build input ([`CRAFT_FONTS`], embedded by
//! `build.rs` from `CRAFT_FONTS_DIR`; `docs/assets.md` §4), ordered by the script of the interface
//! language ([`set_language`]). egui's own default fonts are off: every glyph comes from these.
//!
//! The craft-fonts part is adapted from LightCraft `crates/engine/src/fonts.rs` and the font
//! definitions of `crates/ui-egui/src/theme.rs`, with the script table of
//! `crates/ui-egui/src/i18n.rs` (revision d999e382); MIT OR Apache-2.0. Changed for Dalica: the
//! language's own script comes first for every script (Thai faces first for Thai), not only CJK;
//! no separately loaded web face; a bold variable face is not drawn heavier.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId, Id};

const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// The family name of Inter SemiBold.
pub const SEMIBOLD_FAMILY: &str = "semibold";

/// The font-data names of Inter Regular and SemiBold.
pub const INTER_REGULAR: &str = "Inter-Regular";
pub const INTER_SEMIBOLD: &str = "Inter-SemiBold";

/// The script of English and of any language the table does not know: Inter covers it.
pub const DEFAULT_SCRIPT: &str = "Latn";

/// A face from the optional craft-fonts build input (empty unless built with `CRAFT_FONTS_DIR`).
#[derive(Debug)]
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    /// The ISO 15924 scripts the face is for, e.g. `"Jpan"`.
    pub scripts: &'static [&'static str],
    /// Its licence (`OFL-1.1`) and the licence file's path inside the craft-fonts checkout, which
    /// the packages ship as `OFL-<family>.txt` (`docs/releasing.md`).
    pub licence: &'static str,
    pub licence_file: &'static str,
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

impl CraftFont {
    /// Whether the face is meant for `script` (ISO 15924).
    pub fn covers(&self, script: &str) -> bool {
        self.scripts.contains(&script)
    }

    /// Whether the face can stand in for `script`: it is meant for it or, while craft-fonts has no
    /// Traditional Chinese face, it is a Simplified Chinese one, which carries the Traditional
    /// characters too (with mainland forms; still closer than the Japanese faces).
    pub fn serves(&self, script: &str) -> bool {
        self.covers(script) || (script == "Hant" && self.covers("Hans"))
    }

    /// Mincho (serif) faces: document faces, the last choice for the UI.
    pub fn is_mincho(&self) -> bool {
        self.family.contains("Mincho")
    }

    /// Whether the face is for a CJK script.
    pub fn is_cjk(&self) -> bool {
        self.scripts.iter().any(|s| is_cjk_script(s))
    }

    /// Its font-data name in [`definitions_for`]: `"<family> <style>"`.
    pub fn name(&self) -> String {
        format!("{} {}", self.family, self.style)
    }
}

/// Whether an ISO 15924 script is written with Han, kana or hangul (Unicode's CJK scripts), which
/// Inter has no glyphs for.
pub fn is_cjk_script(script: &str) -> bool {
    matches!(script, "Hans" | "Hant" | "Jpan" | "Kore" | "Hang" | "Hani" | "Bopo" | "Yiii" | "Nshu" | "Tang")
}

/// The ISO 15924 script a language (a BCP-47 code such as `"ja"`, `"zh-Hant"`, `"th-TH"`) is
/// written in, which picks the order of the fallback faces; [`DEFAULT_SCRIPT`] for a language the
/// table does not know. Chinese without a script subtag follows its region (Taiwan, Hong Kong and
/// Macau write Traditional), else Simplified.
pub fn script_of(language: &str) -> &'static str {
    let lower = language.trim().to_ascii_lowercase().replace('_', "-");
    let mut parts = lower.split('-');
    let base = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    match base {
        "zh" => {
            if rest.contains(&"hant") || (rest.iter().any(|r| matches!(*r, "tw" | "hk" | "mo")) && !rest.contains(&"hans")) {
                "Hant"
            } else {
                "Hans"
            }
        }
        "ja" => "Jpan",
        "ko" => "Kore",
        "th" => "Thai",
        "ru" | "uk" | "be" | "bg" | "mk" | "kk" | "ky" | "mn" => "Cyrl",
        "sr" if !rest.contains(&"latn") => "Cyrl",
        "el" => "Grek",
        "ka" => "Geor",
        "ar" | "fa" | "ur" | "ps" => "Arab",
        "he" | "yi" => "Hebr",
        "hi" | "mr" | "ne" => "Deva",
        _ => DEFAULT_SCRIPT,
    }
}

/// Scripts Inter draws itself: no fallback face is preferred for them.
fn inter_draws(script: &str) -> bool {
    matches!(script, "Latn" | "Cyrl" | "Grek")
}

/// The craft-fonts faces to fall back on for `script`, in preference order, Latin-only faces left
/// out (Inter draws Latin): first the faces of that script (a face for that script alone before a
/// multi-script one, BIZ UDPGothic before the other Japanese faces, `style` before others, Mincho
/// last), then the other CJK faces in the same order, then every other script's faces (`style`
/// first). Han characters are shared between Chinese and Japanese, so the language's own forms
/// must win; file and folder names can be in any script whatever the language, so every face is
/// offered. Empty without craft-fonts.
pub fn ui_fallback<'a>(fonts: &'a [CraftFont], script: &str, style: &str) -> Vec<&'a CraftFont> {
    let own = |f: &CraftFont| !inter_draws(script) && f.serves(script);
    let mut faces: Vec<&CraftFont> = fonts.iter().filter(|f| f.scripts.iter().any(|s| *s != "Latn")).collect();
    faces.sort_by_key(|f| {
        let group = if own(f) {
            0
        } else if f.is_cjk() {
            1
        } else {
            2
        };
        let cjk_order = group < 2 || f.is_cjk();
        (group, cjk_order && f.scripts.len() != 1, cjk_order && f.family != "BIZ UDPGothic", f.style != style, f.is_mincho())
    });
    faces
}

/// Our font set for [`DEFAULT_SCRIPT`]: Inter, then the embedded craft-fonts faces (none without
/// `CRAFT_FONTS_DIR`).
pub fn definitions() -> FontDefinitions {
    definitions_for(CRAFT_FONTS, DEFAULT_SCRIPT)
}

/// Our font set for `script`: Inter for proportional and monospace text, Inter SemiBold as the
/// named family [`SEMIBOLD_FAMILY`], each followed by the [`ui_fallback`] faces of `craft`
/// (Regular faces preferred for the first two, Bold for SemiBold).
pub fn definitions_for(craft: &'static [CraftFont], script: &str) -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    defs.font_data.insert(INTER_REGULAR.into(), Arc::new(FontData::from_static(REGULAR)));
    defs.font_data.insert(INTER_SEMIBOLD.into(), Arc::new(FontData::from_static(SEMIBOLD)));
    for font in craft {
        defs.font_data.insert(font.name(), Arc::new(FontData::from_static(font.bytes)));
    }
    let names = |style: &str| ui_fallback(craft, script, style).into_iter().map(CraftFont::name).collect::<Vec<_>>();
    let mut regular = vec![INTER_REGULAR.to_string()];
    regular.extend(names("Regular"));
    let mut semibold = vec![INTER_SEMIBOLD.to_string(), INTER_REGULAR.to_string()];
    semibold.extend(names("Bold"));
    defs.families.insert(FontFamily::Proportional, regular.clone());
    defs.families.insert(FontFamily::Monospace, regular);
    defs.families.insert(FontFamily::Name(SEMIBOLD_FAMILY.into()), semibold);
    defs
}

fn script_key() -> Id {
    Id::new("dalica-ui-kit.fonts.script")
}

/// The script whose faces `ctx`'s fonts put first ([`DEFAULT_SCRIPT`] until [`set_script`]).
pub fn script(ctx: &Context) -> String {
    ctx.data(|d| d.get_temp::<String>(script_key())).unwrap_or_else(|| DEFAULT_SCRIPT.into())
}

/// Installs [`definitions_for`] the embedded faces and `ctx`'s [`script`]; they take effect on the
/// next pass.
pub fn install(ctx: &Context) {
    ctx.set_fonts(definitions_for(CRAFT_FONTS, &script(ctx)));
}

/// Orders the fallback faces for `script` from the next pass on, without a restart; nothing
/// happens when it is already the script. Returns whether the fonts changed.
pub fn set_script(ctx: &Context, script_code: &str) -> bool {
    if script(ctx) == script_code {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(script_key(), script_code.to_string()));
    install(ctx);
    true
}

/// [`set_script`] for the script of a language (`"th"`, `"ja"`, `"zh-hant"`; [`script_of`]).
pub fn set_language(ctx: &Context, language: &str) -> bool {
    set_script(ctx, script_of(language))
}

/// The licence files the embedded faces need in a package: one `(family, licence, licence file in
/// the craft-fonts checkout)` per family. Empty without craft-fonts.
pub fn licences() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out: Vec<(&str, &str, &str)> = Vec::new();
    for f in CRAFT_FONTS {
        if !out.iter().any(|(family, _, _)| *family == f.family) {
            out.push((f.family, f.licence, f.licence_file));
        }
    }
    out
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
        let defs = definitions_for(&[], DEFAULT_SCRIPT);
        assert_eq!(defs.font_data.len(), 2);
        assert!(defs.families.contains_key(&FontFamily::Name(SEMIBOLD_FAMILY.into())));
    }
}
