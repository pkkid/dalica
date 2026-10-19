//! Thai in our text path (M15.47, `docs/thai-text.md`): shaping with craft-fonts' Noto Sans Thai
//! through egui's `harfrust` against a reference outside the product, and line breaks at
//! dictionary word boundaries with `linebreak::mark`.

use std::borrow::Cow;
use std::time::Instant;

use ab_glyph::{Font, FontRef};
use dalica_ui_kit::fonts::{self, CRAFT_FONTS};
use dalica_ui_kit::linebreak::{self, MARK};
use dalica_ui_kit::{Tokens, snapshot};
use egui::{Color32, Context, FontId, Galley};

use crate::common;

/// The words whose marks are checked: tone marks stacked over upper vowels (ที่, กี้), a lower
/// vowel and a tone mark on a tall consonant (ปู่), สระอำ split around a tone mark (น้ำ), and
/// tall consonants whose marks move aside (ฝั่ง, ฟ้า).
const WORDS: &[&str] = &["ที่", "กี้", "ปู่", "น้ำ", "ฝั่ง", "ฟ้า"];

/// More Thai for the glyph check: every consonant, the vowels, tone marks, digits and signs.
const GLYPH_SET: &str = "กขฃคฅฆงจฉชซฌญฎฏฐฑฒณดตถทธนบปผฝพฟภมยรฤลฦวศษสหฬอฮ ะัาำิีึืฺุู เแโใไๅๆ ่้๊๋์ํ๎ ฯ ๏๐๑๒๓๔๕๖๗๘๙๚๛ ฿";

/// One glyph as the reference shaper places it, in font units: advance, offset (x, y), ink box
/// (x bearing, y bearing, width, height; y up, height negative).
type RefGlyph = (i32, i32, i32, i32, i32, i32, i32);

/// The reference: `hb-shape --show-extents` (HarfBuzz 14.1.0, outside the product) over
/// craft-fonts' `NotoSansThai.ttf` at pin 3d9ab3a (sha256 5a1c559b…), one row per glyph.
const REFERENCE: &[(&str, &[RefGlyph])] = &[
    ("ที่", &[(609, 0, 0, 84, 558, 444, -558), (0, -3, 0, -514, 787, 426, -152), (0, -3, 0, -173, 1009, 85, -163)]),
    ("กี้", &[(600, 0, 0, 66, 558, 453, -558), (0, -2, 0, -514, 787, 426, -152), (0, -2, 0, -267, 1009, 261, -163)]),
    ("ปู่", &[(605, 0, 0, 81, 745, 442, -755), (0, -14, 0, -388, -64, 299, -201), (0, 1, 0, -344, 846, 88, -211)]),
    ("น้ำ", &[(613, 0, 0, 81, 548, 444, -558), (0, 0, 0, -268, 791, 218, -180), (0, -29, 0, -267, 1009, 261, -163), (406, 0, 0, 10, 558, 315, -558)]),
    (
        "ฝั่ง",
        &[(652, 0, 0, 86, 745, 475, -745), (0, -22, 0, -497, 779, 271, -144), (0, -209, -40, -173, 1009, 85, -163), (537, 0, 0, 0, 553, 483, -553)],
    ),
    ("ฟ้า", &[(682, 0, 0, 41, 745, 620, -745), (0, 23, -1, -499, 840, 271, -205), (406, 0, 0, 10, 558, 315, -558)]),
];

/// A paragraph of short plain sentences, as its words: sentences are separated by a space, as
/// Thai writes them, and words are not.
const PARAGRAPH: &[&[&str]] = &[
    &["ฉัน", "ชอบ", "ถ่าย", "รูป", "ดอกไม้", "ใน", "สวน", "ทุก", "วัน"],
    &["แม่", "ของ", "ฉัน", "ชอบ", "ดู", "รูป", "เหล่า", "นี้", "มาก"],
    &["เรา", "เก็บ", "รูป", "ไว้", "ใน", "คอมพิวเตอร์", "และ", "จัด", "เป็น", "อัลบั้ม", "ตาม", "ปี"],
    &["เมื่อ", "มี", "เวลา", "ว่าง", "เรา", "จะ", "เปิด", "ดู", "รูป", "เก่า", "ด้วย", "กัน"],
    &["รูป", "ที่", "สวย", "ที่สุด", "จะ", "พิมพ์", "ใส่", "กรอบ", "ไว้", "บน", "ผนัง", "บ้าน"],
    &["พ่อ", "ถ่าย", "รูป", "ทะเล", "ตอน", "เช้า"],
    &["น้อง", "ถ่าย", "รูป", "แมว", "ตัว", "เล็ก", "ที่", "กำลัง", "นอน", "อยู่", "ใต้", "ต้นไม้", "ใหญ่"],
];

fn paragraph(word_sep: &str) -> String {
    PARAGRAPH.iter().map(|s| s.join(word_sep)).collect::<Vec<_>>().join(" ")
}

/// A context with the fonts of the Thai interface, after the pass that loads them.
fn thai_ctx() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions_for(CRAFT_FONTS, "Thai"));
    ctx.run_ui(dalica_ui_kit::raw_input(200, 100, 1.0), |_| {}).textures_delta.clear();
    ctx
}

fn skipped(test: &str) -> bool {
    if CRAFT_FONTS.is_empty() {
        eprintln!("skipped {test}: built without CRAFT_FONTS_DIR");
        return true;
    }
    false
}

fn layout(ctx: &Context, text: &str, size: f32, wrap: f32) -> std::sync::Arc<Galley> {
    ctx.fonts_mut(|f| f.layout(text.to_string(), FontId::proportional(size), Color32::WHITE, wrap))
}

#[test]
fn thai_lays_out_without_a_panic_with_or_without_the_fonts() {
    let ctx = thai_ctx();
    let text = format!("{} {}", WORDS.join(" "), linebreak::mark(&paragraph("")));
    for size in [9.0, 13.0, 16.0, 40.0] {
        let galley = layout(&ctx, &text, size, 120.0);
        assert!(galley.size().x > 0.0 && galley.rows.len() > 1, "{size}");
    }
    if CRAFT_FONTS.is_empty() {
        eprintln!(
            "skipped with CRAFT_FONTS_DIR unset: with_the_fonts_every_thai_glyph_has_a_face, \
             with_the_fonts_the_marks_sit_where_the_reference_puts_them, with_the_fonts_a_fingerprint_of_the_set"
        );
    }
}

#[test]
fn with_the_fonts_every_thai_glyph_has_a_face() {
    if skipped("with_the_fonts_every_thai_glyph_has_a_face") {
        return;
    }
    let defs = fonts::definitions_for(CRAFT_FONTS, "Thai");
    let ctx = thai_ctx();
    let text = format!("{GLYPH_SET}{}{}", WORDS.concat(), paragraph(""));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Name(fonts::SEMIBOLD_FAMILY.into())] {
        let order = &defs.families[&family];
        for c in text.chars().filter(|c| !c.is_whitespace()) {
            // The first face in the family's order with a glyph for it draws it, as egui picks.
            let face = order.iter().find(|name| {
                let data = &defs.font_data[*name];
                FontRef::try_from_slice_and_index(&data.font, data.index).is_ok_and(|f| f.glyph_id(c).0 != 0)
            });
            let face = face.map(String::as_str).unwrap_or_default();
            // Inter draws the baht sign ฿; every other Thai character is Noto Sans Thai's.
            assert!(face.starts_with("Noto Sans Thai") || (c == '฿' && face.starts_with("Inter-")), "{c:?} in {family:?} drawn by {face:?}");
            // egui agrees (its has_glyph says no for the face that holds its replacement glyph).
            if !face.starts_with("Inter-") {
                assert!(ctx.fonts_mut(|f| f.has_glyph(&FontId::new(16.0, family.clone()), c)), "egui finds no glyph for {c:?} in {family:?}");
            }
        }
    }
}

#[test]
fn with_the_fonts_the_marks_sit_where_the_reference_puts_them() {
    if skipped("with_the_fonts_the_marks_sit_where_the_reference_puts_them") {
        return;
    }
    let thai = CRAFT_FONTS.iter().find(|f| f.family == "Noto Sans Thai").unwrap();
    let upem = FontRef::try_from_slice(thai.bytes).unwrap().units_per_em().unwrap();
    let ctx = thai_ctx();
    let size = 16.0;
    let px = size / upem;
    let mut worst = 0.0_f32;
    for (word, reference) in REFERENCE {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(word.to_string(), FontId::proportional(size), Color32::WHITE));
        let glyphs: Vec<_> = galley.rows[0].row.glyphs.iter().filter(|g| !g.uv_rect.is_nothing()).collect();
        assert_eq!(glyphs.len(), reference.len(), "{word}: one glyph per reference glyph (marks substituted, สระอำ split)");
        let mut pen = 0.0;
        for (i, (g, &(advance, dx, dy, bx, by, w, h))) in glyphs.iter().zip(reference.iter()).enumerate() {
            // The ink box's centre, from the start of the word and the baseline (y down).
            let want_x = pen + (dx + bx) as f32 * px + w as f32 * px / 2.0;
            let want_y = -((dy + by) as f32 + h as f32 / 2.0) * px;
            let got_x = g.pos.x + g.uv_rect.offset.x + g.uv_rect.size.x / 2.0;
            let got_y = g.uv_rect.offset.y + g.uv_rect.size.y / 2.0;
            let off = (got_x - want_x).abs().max((got_y - want_y).abs());
            worst = worst.max(off);
            assert!(off <= 1.0, "{word} glyph {i}: ink centre at ({got_x:.2}, {got_y:.2}), the reference at ({want_x:.2}, {want_y:.2})");
            pen += advance as f32 * px;
        }
    }
    eprintln!("Thai marks at {size} px: at most {worst:.2} px from the reference");
}

#[test]
fn with_the_fonts_a_fingerprint_of_the_set() {
    if skipped("with_the_fonts_a_fingerprint_of_the_set") {
        return;
    }
    let marked = linebreak::mark(&paragraph("")).into_owned();
    let img = snapshot(460, 300, 1.0, &Tokens::DARK, |ui| {
        fonts::set_script(ui.ctx(), "Thai");
        egui::Frame::NONE.inner_margin(12.0).show(ui, |ui| {
            ui.label(egui::RichText::new(WORDS.join("  ")).size(40.0));
            ui.label(egui::RichText::new(WORDS.join("  ")).size(16.0));
            ui.label(egui::RichText::new(GLYPH_SET).size(16.0));
            ui.allocate_ui(egui::vec2(200.0, 200.0), |ui| {
                ui.set_max_width(200.0);
                ui.add(egui::Label::new(egui::RichText::new(&marked).size(13.0)).wrap());
            });
        });
    })
    .unwrap();
    common::check_one("thai-text", &img);
}

#[test]
fn mark_breaks_a_paragraph_only_at_dictionary_word_boundaries() {
    let words: usize = PARAGRAPH.iter().map(|s| s.len()).sum();
    assert!(words >= 60, "{words} words");
    let plain = paragraph("");
    let marked = linebreak::mark(&plain).into_owned();
    assert_eq!(marked, paragraph(&MARK.to_string()), "a mark between every two words, and only there");
    assert_eq!(linebreak::mark(&marked), marked, "marking marked text changes nothing");
    // Not before ๆ or ฯ, not between Thai and Latin, not next to a space.
    for text in ["เด็กๆ เล่น", "กรุงเทพฯ", "ไฟล์JPEG", "JPEG ไฟล์"] {
        assert_eq!(linebreak::mark(text), text);
    }
    // Text without Thai comes back as it is, without a copy.
    for text in ["", "Holiday photos 2026", "写真を整理する・アルバム", "整理照片和相册", "Фото – альбом"] {
        assert!(matches!(linebreak::mark(text), Cow::Borrowed(t) if t == text), "{text}");
    }
}

#[test]
fn a_label_wrapped_at_200_px_breaks_only_at_the_marks() {
    let ctx = thai_ctx();
    let plain = paragraph("");
    let marked = linebreak::mark(&plain).into_owned();
    let mut galleys = Vec::new();
    ctx.run_ui(dalica_ui_kit::raw_input(400, 400, 1.0), |ui| {
        ui.set_max_width(200.0);
        for text in [&marked, &plain] {
            let (_, galley, _) = egui::Label::new(text.as_str()).wrap().layout_in_ui(ui);
            galleys.push(galley);
        }
    })
    .textures_delta
    .clear();
    // A row ends with its last glyph; a mark (or a space) is a glyph of its own.
    let ends = |galley: &Galley| -> Vec<usize> {
        let mut at = 0;
        galley
            .rows
            .iter()
            .map(|r| {
                at += r.row.glyphs.len();
                at
            })
            .collect()
    };
    let marked_rows = &galleys[0].rows;
    assert!(marked_rows.len() > 3, "{} rows", marked_rows.len());
    for row in &marked_rows[..marked_rows.len() - 1] {
        let last = row.row.glyphs.last().map(|g| g.chr);
        assert!(last == Some(MARK) || last == Some(' '), "a row broken inside a word, before {last:?}");
    }
    assert!(galleys[0].size().x <= 200.5);
    // Unmarked, the same label breaks inside words: what the marks are for. Where words end, in
    // glyphs (Thai is shaped word by word alike: no glyph reaches across a word boundary).
    let glyphs = |t: &str| ctx.fonts_mut(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(13.0), Color32::WHITE)).rows[0].row.glyphs.len();
    let mut word_ends = Vec::new();
    let mut at = 0;
    for sentence in PARAGRAPH {
        for word in *sentence {
            at += glyphs(word);
            word_ends.push(at);
        }
        at += 1;
        word_ends.push(at);
    }
    assert_eq!(word_ends.last().copied(), Some(at), "{at}");
    assert_eq!(ends(&galleys[1]).last().copied(), Some(at - 1), "the glyphs of the plain paragraph add up");
    let plain_ends = ends(&galleys[1]);
    let inside = plain_ends[..plain_ends.len() - 1].iter().filter(|e| !word_ends.contains(e)).count();
    assert!(inside > 0, "{plain_ends:?} vs {word_ends:?}");
    // The marks take no room.
    let wide = |t: &str| ctx.fonts_mut(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(13.0), Color32::WHITE)).size().x;
    assert!((wide(&marked) - wide(&plain)).abs() < 0.5, "marked {} px, plain {} px", wide(&marked), wide(&plain));
}

/// The measurement M15.47 starts from: egui 0.36 does not break a row at U+200B ZERO WIDTH
/// SPACE (not whitespace to Rust), which is why [`MARK`] is a thin space. If this fails, egui
/// learnt to: `docs/thai-text.md` says what to change.
#[test]
fn egui_does_not_break_at_a_zero_width_space() {
    let ctx = thai_ctx();
    let galley = layout(&ctx, &paragraph("\u{200B}"), 13.0, 200.0);
    let at_a_boundary = |c: Option<char>| matches!(c, Some('\u{200B}' | ' '));
    let inside = galley
        .rows
        .windows(2)
        .filter(|w| !at_a_boundary(w[0].row.glyphs.last().map(|g| g.chr)) && !at_a_boundary(w[1].row.glyphs.first().map(|g| g.chr)));
    assert!(inside.count() > 0, "every row broke at a U+200B");
}

#[test]
fn mark_costs_little_on_the_release_notes_length() {
    // Help ▸ Release Notes is about 52,000 characters: a Thai text of that length.
    let block = format!("{}\n", paragraph(""));
    let text: String = block.repeat(52_000 / block.chars().count() + 1);
    let chars = text.chars().count();
    let start = Instant::now();
    let marked = linebreak::mark(&text);
    let took = start.elapsed();
    let marks = marked.chars().filter(|c| *c == MARK).count();
    assert_eq!(marked.chars().count(), chars + marks);
    let caption = "แมวของฉันนอนอยู่ใต้ต้นไม้ใหญ่";
    let start = Instant::now();
    for _ in 0..100 {
        std::hint::black_box(linebreak::mark(std::hint::black_box(caption)));
    }
    let per_caption = start.elapsed() / 100;
    eprintln!(
        "linebreak::mark: {chars} Thai characters ({marks} marks) in {took:?}; a {}-character caption in {per_caption:?}",
        caption.chars().count()
    );
}
