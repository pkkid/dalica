//! The fonts: Inter alone without craft-fonts, the craft-fonts fallback faces in the order the
//! interface language's script asks, switched without a restart, the build script's checks, and
//! its subsetter (M16.27).

use ab_glyph::{Font, FontRef, GlyphId};
use dalica_ui_kit::fonts::{self, CRAFT_FONTS, CraftFont, DEFAULT_SCRIPT};
use egui::{Color32, Context, FontDefinitions, FontFamily, FontId};

#[allow(dead_code, unreachable_pub)]
#[path = "../build.rs"]
mod build_script;

/// A sample for each script the fallback faces are for: Japanese with kana and Han, Simplified and
/// Traditional Chinese whose Han forms differ, Thai with stacked vowels and tone marks.
const SAMPLES: &[(&str, &str)] = &[
    ("Jpan", "写真を整理する・アルバム"),
    ("Hans", "整理照片和相册，导入图库"),
    ("Hant", "整理照片與相簿，匯入圖庫"),
    ("Thai", "ภาพถ่ายสวัสดี่ปี่น้ำ๑๒๓"),
    ("Arab", "صورة الألبوم"),
    ("Geor", "ფოტოები და ალბომი"),
];

/// A context with `defs` installed, after the pass that loads them.
fn ctx_with(defs: FontDefinitions) -> Context {
    let ctx = Context::default();
    ctx.set_fonts(defs);
    ctx.run_ui(dalica_ui_kit::raw_input(200, 100, 1.0), |_| {}).textures_delta.clear();
    ctx
}

#[test]
fn without_craft_fonts_the_faces_are_inter_alone_and_cjk_text_lays_out() {
    for script in ["Latn", "Jpan", "Hans", "Hant", "Thai"] {
        let defs = fonts::definitions_for(&[], script);
        let mut names: Vec<&String> = defs.font_data.keys().collect();
        names.sort();
        assert_eq!(names, [fonts::INTER_REGULAR, fonts::INTER_SEMIBOLD], "{script}");
        for family in defs.families.values() {
            assert!(family.iter().all(|n| n.starts_with("Inter-")), "{script}: {family:?}");
        }
    }
    let ctx = ctx_with(fonts::definitions_for(&[], "Jpan"));
    let text: String = SAMPLES.iter().map(|(_, s)| *s).collect::<Vec<_>>().join(" ");
    for font in [fonts::regular(13.0), fonts::semibold(13.0), FontId::monospace(13.0)] {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE));
        assert!(galley.size().x > 0.0 && galley.size().y > 0.0, "{font:?}");
    }
}

/// The face of `defs` that draws `c` in `family`: the first in the family's order with a glyph.
fn drawing_face<'a>(defs: &'a FontDefinitions, family: &'a [String], c: char) -> Option<&'a str> {
    family.iter().find_map(|name| {
        let data = defs.font_data.get(name)?;
        let face = FontRef::try_from_slice_and_index(&data.font, data.index).ok()?;
        (face.glyph_id(c).0 != 0).then_some(name.as_str())
    })
}

#[test]
fn with_craft_fonts_every_glyph_has_a_face_in_the_order_its_script_asks() {
    if CRAFT_FONTS.is_empty() {
        eprintln!("skipped with_craft_fonts_every_glyph_has_a_face_in_the_order_its_script_asks: built without CRAFT_FONTS_DIR");
        return;
    }
    // Every face parses, and its licence is listed for the packages.
    for f in CRAFT_FONTS {
        assert!(FontRef::try_from_slice(f.bytes).is_ok(), "{} parses", f.name());
        assert!(fonts::licences().iter().any(|(family, licence, file)| *family == f.family && !licence.is_empty() && !file.is_empty()));
    }
    // The CJK scripts the block names and Thai (1.0's second language) are always checked; every
    // other script the manifest lists needs a sample here.
    let mut scripts: Vec<&str> = vec!["Jpan", "Hans", "Hant", "Thai"];
    for f in CRAFT_FONTS {
        scripts.extend(f.scripts.iter().copied().filter(|s| *s != "Latn"));
    }
    scripts.sort_unstable();
    scripts.dedup();
    for script in scripts {
        let Some((_, sample)) = SAMPLES.iter().find(|(s, _)| *s == script) else {
            panic!("the manifest lists {script}: add a sample for it to SAMPLES");
        };
        let defs = fonts::definitions_for(CRAFT_FONTS, script);
        let ctx = ctx_with(defs.clone());
        for (family, style) in [(FontFamily::Proportional, "Regular"), (FontFamily::Name(fonts::SEMIBOLD_FAMILY.into()), "Bold")] {
            let order = &defs.families[&family];
            // The script's own faces come right after Inter, in ui_fallback's order.
            let expected: Vec<String> = fonts::ui_fallback(CRAFT_FONTS, script, style).iter().map(|f| f.name()).collect();
            let craft: Vec<String> = order.iter().filter(|n| !n.starts_with("Inter-")).cloned().collect();
            assert_eq!(craft, expected, "{script} {family:?}");
            let first = craft.first().and_then(|name| CRAFT_FONTS.iter().find(|f| f.name() == *name));
            assert!(first.is_some_and(|f| f.serves(script)), "{script} {family:?}: the script's own face first, not {first:?}");
            for c in sample.chars().filter(|c| !c.is_whitespace()) {
                let face = drawing_face(&defs, order, c);
                assert!(face.is_some(), "{script} {family:?}: no face draws {c:?}");
                let face = face.unwrap_or_default();
                if !face.starts_with("Inter-") && !c.is_ascii() {
                    let font = CRAFT_FONTS.iter().find(|f| f.name() == face).unwrap();
                    assert!(font.serves(script) || !c.is_alphabetic(), "{script} {family:?}: {c:?} drawn by {face}, not the script's own face");
                }
                let font = FontId::new(13.0, family.clone());
                assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "{script} {family:?}: egui finds no glyph for {c:?}");
            }
        }
    }
}

#[test]
fn the_language_orders_the_faces_and_switches_them_without_a_restart() {
    for (language, script) in [
        ("en", "Latn"),
        ("ja", "Jpan"),
        ("zh-hans", "Hans"),
        ("zh-Hant", "Hant"),
        ("zh-TW", "Hant"),
        ("zh_HK", "Hant"),
        ("zh-Hans-HK", "Hans"),
        ("zh", "Hans"),
        ("th", "Thai"),
        ("th-TH", "Thai"),
        ("ko", "Kore"),
        ("uk", "Cyrl"),
        ("ka", "Geor"),
        ("ar", "Arab"),
        ("", DEFAULT_SCRIPT),
        ("x-unknown", DEFAULT_SCRIPT),
    ] {
        assert_eq!(fonts::script_of(language), script, "{language:?}");
    }
    static FACES: &[CraftFont] = &[
        CraftFont { family: "Shippori Mincho", style: "Regular", scripts: &["Jpan", "Latn"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "BIZ UDPGothic", style: "Bold", scripts: &["Jpan", "Latn"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "BIZ UDPGothic", style: "Regular", scripts: &["Jpan", "Latn"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "Noto Sans CJK SC", style: "Regular", scripts: &["Hans", "Latn"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "Noto Sans Arabic", style: "Regular", scripts: &["Arab"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "Noto Sans Thai", style: "Regular", scripts: &["Thai"], licence: "", licence_file: "", bytes: &[] },
        CraftFont { family: "Latin Only Sans", style: "Regular", scripts: &["Latn"], licence: "", licence_file: "", bytes: &[] },
    ];
    let order = |script: &str, style: &str| fonts::ui_fallback(FACES, script, style).iter().map(|f| f.name()).collect::<Vec<_>>();
    let cjk_for_japanese = ["BIZ UDPGothic Regular", "BIZ UDPGothic Bold", "Shippori Mincho Regular", "Noto Sans CJK SC Regular"];
    assert_eq!(order("Jpan", "Regular")[..4], cjk_for_japanese);
    assert_eq!(order("Hans", "Regular")[0], "Noto Sans CJK SC Regular");
    // No Traditional Chinese face: the Simplified one stands in before the Japanese ones.
    assert_eq!(order("Hant", "Regular")[0], "Noto Sans CJK SC Regular");
    assert_eq!(order("Jpan", "Bold")[0], "BIZ UDPGothic Bold");
    // For any other script the CJK faces keep their order, a Mincho (document) face last.
    let cjk_for_others = ["BIZ UDPGothic Regular", "BIZ UDPGothic Bold", "Noto Sans CJK SC Regular", "Shippori Mincho Regular"];
    // Thai first for Thai, then the CJK faces, then the other scripts; no Latin-only face.
    assert_eq!(
        order("Thai", "Regular"),
        [
            "Noto Sans Thai Regular",
            "BIZ UDPGothic Regular",
            "BIZ UDPGothic Bold",
            "Noto Sans CJK SC Regular",
            "Shippori Mincho Regular",
            "Noto Sans Arabic Regular"
        ]
    );
    // English: CJK faces first (as for any script Inter draws), then the others.
    assert_eq!(order("Latn", "Regular")[..4], cjk_for_others);
    assert_eq!(order("Latn", "Regular")[4..], ["Noto Sans Arabic Regular", "Noto Sans Thai Regular"]);

    // Switching the language reinstalls the fonts on the same context: no restart.
    let ctx = Context::default();
    fonts::install(&ctx);
    assert_eq!(fonts::script(&ctx), DEFAULT_SCRIPT);
    assert!(fonts::set_language(&ctx, "th"));
    assert_eq!(fonts::script(&ctx), "Thai");
    assert!(!fonts::set_language(&ctx, "th-TH"), "the same script changes nothing");
    assert!(fonts::set_language(&ctx, "ja"));
    ctx.run_ui(dalica_ui_kit::raw_input(200, 100, 1.0), |ui| {
        ui.label("写真 ภาพถ่าย");
    })
    .textures_delta
    .clear();
    assert_eq!(fonts::script(&ctx), "Jpan");
}

#[test]
fn a_required_build_without_the_directory_fails_naming_it() {
    let scratch = std::env::temp_dir();
    let out = build_script::generate(None, true, &scratch);
    assert_eq!(out.errors.len(), 1, "{out:?}");
    assert!(out.errors[0].contains("CRAFT_FONTS_DIR"), "{}", out.errors[0]);
    let missing = std::env::temp_dir().join("dalica-no-such-craft-fonts-checkout");
    let out = build_script::generate(Some(&missing), true, &scratch);
    assert!(out.errors.iter().any(|e| e.contains("CRAFT_FONTS_DIR") && e.contains("dalica-no-such-craft-fonts-checkout")), "{out:?}");
    // Not required: Inter alone, with a warning only when a directory was named.
    let plain = build_script::generate(None, false, &scratch);
    assert!(plain.errors.is_empty() && plain.warnings.is_empty() && plain.embedded == 0);
    assert_eq!(plain.source, "pub static CRAFT_FONTS: &[CraftFont] = &[\n];\n");
    let broken = build_script::generate(Some(&missing), false, &scratch);
    assert!(broken.errors.is_empty() && broken.warnings.len() == 1, "{broken:?}");
    // A release manifest without the Thai UI face is refused, naming the face.
    let line = |family: &str, style: &str, scripts: &str| {
        format!("{family} | {style} | fonts/x/{family}.ttf | {scripts} | OFL-1.1 | fonts/x/OFL.txt | 00 | https://example.invalid\n")
    };
    let manifest = format!("# comment\n\n{}{}", line("BIZ UDPGothic", "Regular", "Jpan,Latn"), line("Noto Sans CJK SC", "Regular", "Hans,Latn"));
    let entries = build_script::parse_manifest(&manifest).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].scripts, ["Jpan", "Latn"]);
    assert_eq!(entries[1].licence_file, "fonts/x/OFL.txt");
    let thai = build_script::missing_required(&entries).unwrap_or_default();
    assert!(thai.contains("Noto Sans Thai"), "{thai}");
    let full = format!("{manifest}{}", line("Noto Sans Thai", "Regular", "Thai"));
    assert_eq!(build_script::missing_required(&build_script::parse_manifest(&full).unwrap()), None);
    assert!(build_script::parse_manifest("Only | Two").is_err());
}

#[test]
fn only_the_desktop_app_registers_the_faces() {
    // Debug builds (this test) fall back on every embedded face, so a test needs no registration;
    // registering the same faces, as the app does, changes nothing, and only the first counts.
    assert!(std::ptr::eq(fonts::faces(), CRAFT_FONTS));
    fonts::register(CRAFT_FONTS);
    assert!(std::ptr::eq(fonts::faces(), CRAFT_FONTS));
    assert!(!fonts::register(&[]), "only the first registration counts");
    assert!(std::ptr::eq(fonts::faces(), CRAFT_FONTS));
}

#[test]
fn the_cjk_faces_keep_the_four_national_sets() {
    let common = build_script::common_cjk();
    // The table is what the WHATWG decoders give for the four sets' byte ranges.
    let mut decoded = std::collections::BTreeSet::new();
    let mut add = |encoding: &'static encoding_rs::Encoding, leads: &[(u8, u8)], trails: &[(u8, u8)]| {
        for lead in leads.iter().flat_map(|(a, b)| *a..=*b) {
            for trail in trails.iter().flat_map(|(a, b)| *a..=*b) {
                let pair = [lead, trail];
                let (text, bad) = encoding.decode_without_bom_handling(&pair);
                if !bad {
                    decoded.extend(text.chars().map(u32::from).filter(|c| build_script::is_han_or_hangul(*c)));
                }
            }
        }
    };
    add(encoding_rs::GBK, &[(0xB0, 0xF7)], &[(0xA1, 0xFE)]); // GB 2312, rows 16 to 87
    add(encoding_rs::BIG5, &[(0xA4, 0xC5)], &[(0x40, 0x7E), (0xA1, 0xFE)]); // Big5 level 1
    add(encoding_rs::BIG5, &[(0xC6, 0xC6)], &[(0x40, 0x7E)]);
    add(encoding_rs::SHIFT_JIS, &[(0x81, 0x84), (0x88, 0x9F), (0xE0, 0xEA)], &[(0x40, 0x7E), (0x80, 0xFC)]); // JIS X 0208
    add(encoding_rs::EUC_KR, &[(0xA1, 0xFE)], &[(0xA1, 0xFE)]); // KS X 1001
    assert!(decoded.iter().all(|c| build_script::COMMON_CJK_BLOCKS.iter().any(|(a, b)| (a..=b).contains(&c))));
    assert_eq!(common, decoded, "build.rs's COMMON_CJK is the four sets");
    let count = |range: std::ops::RangeInclusive<u32>| common.iter().filter(|c| range.contains(c)).count();
    assert_eq!(count(0xAC00..=0xD7A3), 2_350, "KS X 1001's Hangul syllables");
    let han = common.iter().filter(|c| build_script::is_han_or_hangul(**c) && !(0xAC00..=0xD7A3).contains(*c)).count();
    assert!((10_500..=11_500).contains(&han), "{han} Han characters in GB 2312, Big5 level 1, JIS X 0208 and KS X 1001");
    let kept = |c: char| build_script::kept(&common, u32::from(c));
    // One of each set: 们 (GB 2312 only), 們 (Big5), 畑 (JIS X 0208's kokuji), 한, and kana,
    // punctuation, Latin and Thai, which are never subset.
    for c in ['们', '們', '畑', '中', '한', 'ア', 'ゞ', '・', '〇', 'A', 'ก'] {
        assert!(kept(c), "{c:?} kept");
    }
    // Left out: an extension A and an extension B character, a Hangul syllable outside KS X 1001.
    for c in ['㐀', '𠀀', '똠'] {
        assert!(!kept(c), "{c:?} left out");
    }
}

/// The outline points of `c` in `font`, if it has a glyph for it.
fn outline(font: &FontRef, c: char) -> Option<Vec<(i32, i32)>> {
    let id = font.glyph_id(c);
    if id.0 == 0 {
        return None;
    }
    outline_of(font, id)
}

fn outline_of(font: &FontRef, id: GlyphId) -> Option<Vec<(i32, i32)>> {
    let o = font.outline(id)?;
    Some(o.curves.iter().map(|c| format!("{c:?}")).map(|s| (s.len() as i32, s.bytes().map(i32::from).sum())).collect())
}

/// Subsets `font` with `keep`, and checks that what it keeps draws as before, what it drops has no
/// glyph, and the file shrank; returns the subset.
fn subset_and_check(name: &str, font: &[u8], keep: &dyn Fn(u32) -> bool, kept: &[char], dropped: &[char]) -> Vec<u8> {
    let small = build_script::subset(font, keep).unwrap();
    let (before, after) = (FontRef::try_from_slice(font).unwrap(), FontRef::try_from_slice(&small).unwrap());
    for c in kept {
        let was = outline(&before, *c);
        assert!(was.is_some(), "{name}: the original draws {c:?}");
        assert_eq!(outline(&after, *c), was, "{name}: {c:?} drawn as before");
        assert_eq!(after.h_advance_unscaled(after.glyph_id(*c)), before.h_advance_unscaled(before.glyph_id(*c)), "{name}: {c:?}'s advance");
    }
    for c in dropped {
        assert!(before.glyph_id(*c).0 != 0, "{name}: the original has {c:?}");
        assert_eq!(after.glyph_id(*c).0, 0, "{name}: {c:?} dropped");
    }
    assert!(small.len() < font.len(), "{name}: {} bytes, was {}", small.len(), font.len());
    eprintln!("{name}: {} bytes subset to {}", font.len(), small.len());
    small
}

#[test]
fn the_subsetter_keeps_the_kept_glyphs_whole_and_refuses_what_it_cannot_read() {
    // TrueType outlines (Inter, always here): keep everything but the capitals.
    let inter = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    let no_capitals = |c: u32| !(u32::from('A')..=u32::from('Z')).contains(&c);
    subset_and_check("Inter", inter, &no_capitals, &['a', 'z', 'é', '€', '0', 'Ж'], &['A', 'Q', 'Z']);
    // A file it cannot read is an error, at every cut, never a panic.
    assert!(build_script::subset(b"not a font", &no_capitals).is_err());
    assert!(build_script::subset(&[], &no_capitals).is_err());
    for cut in [4, 12, 100, 1_000, 50_000, inter.len() / 2, inter.len() - 1] {
        let _ = build_script::subset(&inter[..cut], &no_capitals);
    }
    assert!(build_script::subset(inter, &|_| false).is_err(), "nothing kept");

    // The CJK faces as the build subsets them: CFF (Noto Sans CJK SC, compacted) and TrueType
    // (BIZ UDPGothic), from the checkout the build used.
    let Some(dir) = option_env!("CRAFT_FONTS_DIR") else {
        eprintln!("skipped the CJK faces' subsets: built without CRAFT_FONTS_DIR");
        return;
    };
    let common = build_script::common_cjk();
    let keep = |c: u32| build_script::kept(&common, c);
    let kept = ['中', '国', '們', '畑', '写', '真', 'ア', 'が', '、', '（', 'A', 'g'];
    for (family, file, extra) in [
        ("Noto Sans CJK SC", "fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf", &['한', '글'][..]),
        ("BIZ UDPGothic", "fonts/biz-ud-pgothic/BIZUDPGothic-Regular.ttf", &[][..]),
    ] {
        let original = std::fs::read(std::path::Path::new(dir).join(file)).unwrap();
        let kept: Vec<char> = kept.iter().chain(extra).copied().collect();
        let dropped: Vec<char> = ['㐀', '똠', '龥'].into_iter().filter(|c| FontRef::try_from_slice(&original).unwrap().glyph_id(*c).0 != 0).collect();
        let small = subset_and_check(family, &original, &keep, &kept, &dropped);
        // Every character the subset maps draws as in the original.
        let (before, after) = (FontRef::try_from_slice(&original).unwrap(), FontRef::try_from_slice(&small).unwrap());
        for (id, c) in after.codepoint_ids().step_by(7) {
            assert_eq!(outline_of(&after, id), outline_of(&before, before.glyph_id(c)), "{family}: {c:?}");
        }
        // What the build embedded is this subset.
        let embedded = CRAFT_FONTS.iter().find(|f| f.family == family && f.style == "Regular").unwrap();
        assert!(embedded.bytes == small.as_slice(), "{family}: the embedded face is the subset");
    }
}
