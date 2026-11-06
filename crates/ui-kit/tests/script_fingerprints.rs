//! User text in each script the fonts are for still draws (M16.27): Latin with Inter alone, and,
//! built with `CRAFT_FONTS_DIR`, Japanese, Simplified and Traditional Chinese and Korean with the
//! subset CJK faces, and Thai: every character has a glyph in Regular and SemiBold, and a
//! fingerprint per script pins how it looks.

use ab_glyph::{Font, FontRef};
use dalica_ui_kit::fonts::{self, CRAFT_FONTS};
use dalica_ui_kit::{Tokens, snapshot};
use egui::{Color32, FontFamily, FontId, RichText};

use crate::common;

/// Per script: its ISO 15924 code, the fingerprint's name, and the kind of text a user brings
/// (folder names, captions, tags), every character in the four national sets the faces keep.
const SCRIPTS: &[(&str, &str, &[&str])] = &[
    ("Latn", "script-latin", &["Holiday 2026 – Café de Flore, Zürich", "Ærøskøbing, Łódź; IMG_0042.JPG"]),
    ("Jpan", "script-japanese", &["京都の紅葉・写真アルバム２０２６", "東京駅で撮った家族の思い出（夏休み）"]),
    ("Hans", "script-chinese-simplified", &["北京旅行照片：长城与故宫", "春节全家福，二〇二六年"]),
    ("Hant", "script-chinese-traditional", &["臺灣旅遊相簿：阿里山日出", "畢業典禮與學校風景"]),
    ("Kore", "script-korean", &["제주도 여행 사진 앨범", "가족과 함께한 여름 휴가 2026"]),
    ("Thai", "script-thai", &["อัลบั้มรูปเที่ยวเชียงใหม่", "งานแต่งงานของพี่สาว ปี ๒๕๖๙"]),
];

/// The characters of `lines` no face of the fonts for `script` draws, in Regular or SemiBold: the
/// first face of the family's order with a glyph draws a character, as egui picks.
fn missing(script: &str, lines: &[&str]) -> Vec<char> {
    let defs = fonts::definitions_for(fonts::faces(), script);
    let mut out = Vec::new();
    for family in [FontFamily::Proportional, FontFamily::Name(fonts::SEMIBOLD_FAMILY.into())] {
        for c in lines.iter().flat_map(|l| l.chars()).filter(|c| !c.is_whitespace()) {
            let drawn = defs.families[&family].iter().any(|name| {
                let data = &defs.font_data[name];
                FontRef::try_from_slice_and_index(&data.font, data.index).is_ok_and(|f| f.glyph_id(c).0 != 0)
            });
            if !drawn && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

#[test]
fn user_text_in_every_script_draws_and_keeps_its_fingerprint() {
    let mut failures = Vec::new();
    for (script, name, lines) in SCRIPTS {
        if *script != "Latn" && CRAFT_FONTS.is_empty() {
            eprintln!("skipped {name}: built without CRAFT_FONTS_DIR");
            continue;
        }
        let lacking = missing(script, lines);
        assert!(lacking.is_empty(), "{script}: no face draws {lacking:?}");
        let img = snapshot(440, 120, 1.0, &Tokens::DARK, |ui| {
            fonts::set_script(ui.ctx(), script);
            egui::Frame::NONE.inner_margin(10.0).show(ui, |ui| {
                for line in *lines {
                    ui.label(RichText::new(*line).font(FontId::proportional(20.0)).color(Color32::WHITE));
                    ui.label(RichText::new(*line).font(fonts::semibold(13.0)));
                }
            });
        })
        .unwrap();
        common::check(name, &img, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
