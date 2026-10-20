//! The first languages (M15.35, `localization.md`, `localization-th.md`): English and Thai. The
//! registry lists them; Thai's catalog reads without a fault, its one plural form reads right for
//! 0, 1, 2, 5 and 21 and its text carries break marks between words (`docs/thai-text.md`). The
//! library, the edit view and the Options dialog drawn in Thai at 1400 × 900 and 900 × 600: no
//! label cut at the side of its area, every wrapped Thai label broken only between words, and
//! (built with `CRAFT_FONTS_DIR`) every character drawn by a face that has it, every mark above or
//! below its consonant; switching between English and Thai keeps the view, the selection and the
//! undo history; and (with the fonts) a fingerprint of the library in Thai at 1400 × 900.
//! `DALICA_SHOTS=<dir>` writes the windows (`i18n-th-*.png`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_pixels, demo_open, new_driver, shot};
use dalica_ui::i18n::{self, ENGLISH, LANGUAGES, Plural, languages};
use dalica_ui::{App, Driver, tl, tln};
use dalica_ui_kit::fonts::CRAFT_FONTS;
use dalica_ui_kit::linebreak::{MARK, is_thai};
use egui::epaint::{ClippedShape, Shape, TextShape};
use serde_json::json;

fn thai() -> &'static languages::Language {
    languages::by_code("th").expect("Thai is registered")
}

#[test]
fn the_registry_lists_english_then_thai_and_thais_catalog_reads_whole() {
    let codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
    assert_eq!(codes, ["en", "th"]);
    let th = thai();
    assert_eq!((th.endonym, th.script, th.plural, th.complete_menus), ("ไทย", "Thai", Plural::Single, true));
    let c = i18n::catalog(th);
    assert_eq!(c.problems(), &[] as &[String], "every line of th.tsv is read");
    assert!(c.len() > 1500, "{} entries", c.len());
    // Every entry is machine-assisted until the fluent review takes its `~` away.
    assert!(c.draft_count() <= c.len());
    eprintln!("th: {} entries, {} drafts awaiting the fluent review", c.len(), c.draft_count());
    // Options ▸ General ▸ Language lists Thai by its own name; `th-TH` finds it.
    assert!(i18n::choices().contains(&("th", "ไทย")));
    assert_eq!(i18n::resolve_setting("th-TH", &[]).code, "th");
    assert_eq!(i18n::resolve_setting("system", &["th_TH.UTF-8".to_string()]).code, "th");
}

/// `s` without its break marks.
fn bare(s: &str) -> String {
    s.chars().filter(|&c| c != MARK).collect()
}

#[test]
fn thais_single_plural_form_reads_right_and_its_words_carry_break_marks() {
    i18n::with_language(thai(), || {
        for (n, want) in [(0u64, "0 รูป"), (1, "1 รูป"), (2, "2 รูป"), (5, "5 รูป"), (21, "21 รูป")] {
            assert_eq!(tln!(n, "{n} photo", "{n} photos"), want, "{n}");
        }
        for n in [0u64, 1, 2, 5, 21] {
            assert_eq!(bare(&tln!(n, "{n} folder", "{n} folders")), format!("{n} โฟลเดอร์"), "{n}");
            assert_eq!(
                bare(&tln!(n, "Moved {n} photo to {folder}", "Moved {n} photos to {folder}", folder = "Lisbon")),
                format!("ย้ายรูปภาพ {n} รูปไปยัง Lisbon แล้ว")
            );
        }
        // A context picks its own word; a month and a weekday through ui-kit's table.
        assert_eq!(tl!(ctx = "month", "May"), "พฤษภาคม");
        assert_eq!(bare(&tl!(ctx = "verb", "Open")), "Open", "a key the catalog lacks shows in English");
        // Thai text is marked between words, once, at load; marks never land beside a placeholder.
        let label = tl!("Folder Manager");
        assert_eq!(bare(&label), "ตัวจัดการโฟลเดอร์");
        assert!(label.contains(MARK), "{label:?}");
        let filled = tl!("Could not save {name}: {reason}", name = "IMG_0001.JPG", reason = "x");
        assert_eq!(bare(&filled), "บันทึก IMG_0001.JPG ไม่ได้: x");
    });
    // English needs no catalog and no marks.
    i18n::with_language(&ENGLISH, || {
        assert_eq!(tln!(5u64, "{n} photo", "{n} photos"), "5 photos");
        assert!(!tl!("Folder Manager").contains(MARK));
    });
}

/// The demo library drawn in Thai at `(w, h)`.
fn thai_library(dir: &std::path::Path, (w, h): (u32, u32)) -> Driver {
    let s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.run("options.set", &json!({ "values": { "interface.language": "th" } })).unwrap();
    d.settle(FRAMES).unwrap();
    d
}

/// Every text of `shapes` with its clip.
fn texts(shapes: &[ClippedShape]) -> Vec<(TextShape, egui::Rect)> {
    fn walk(shape: &Shape, clip: egui::Rect, out: &mut Vec<(TextShape, egui::Rect)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            Shape::Text(t) if !t.galley.text().trim().is_empty() => out.push((t.clone(), clip)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(&s.shape, s.clip_rect, &mut out);
    }
    out
}

/// Thai marks drawn above or below their consonant (สระอำ, a spacing vowel, aside).
fn is_mark(c: char) -> bool {
    matches!(c, '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}') && c != '\u{0E4D}'
}

fn is_below(c: char) -> bool {
    matches!(c, '\u{0E38}'..='\u{0E3A}')
}

/// What the next frame draws wrong in Thai, each with `place`: a label cut at the side of its area
/// (`cut`), a Thai label wrapped inside a word, a character no face has, a mark away from its
/// consonant (`faults`; the last two only with the fonts).
fn look(d: &mut Driver, place: &str, cut: &mut BTreeSet<String>, faults: &mut BTreeSet<String>) -> usize {
    d.settle(FRAMES).unwrap();
    let shapes = crate::accessibility::frame(d, Vec::new()).shapes;
    let screen = d.ctx().content_rect();
    let ctx = d.ctx().clone();
    let mut thai_texts = 0;
    for (t, clip) in texts(&shapes) {
        let r = t.visual_bounding_rect();
        let text = t.galley.text().to_owned();
        if r.intersect(clip).is_positive() && (r.left() < clip.left() - 0.5 || r.right() > clip.right() + 0.5 || r.right() > screen.right() + 0.5) {
            cut.insert(format!("{place}: {text:?} cut at the side: {r:?} in {clip:?}"));
        }
        if !text.chars().any(is_thai) {
            continue;
        }
        thai_texts += 1;
        // A wrapped row ends between words: at a mark, a space or a newline, never between two
        // Thai letters.
        for pair in t.galley.rows.windows(2) {
            let (row, next) = (&pair[0], &pair[1]);
            if row.ends_with_newline {
                continue;
            }
            let (last, first) = (row.row.glyphs.last().map(|g| g.chr), next.row.glyphs.first().map(|g| g.chr));
            if let (Some(a), Some(b)) = (last, first)
                && is_thai(a)
                && is_thai(b)
            {
                faults.insert(format!("{place}: {text:?} wrapped inside a word, between {a:?} and {b:?}"));
            }
        }
        if CRAFT_FONTS.is_empty() {
            continue;
        }
        for section in &t.galley.job.sections {
            let font = section.format.font_id.clone();
            let part = text.get(section.byte_range.start.0..section.byte_range.end.0).unwrap_or_default();
            for c in part.chars().filter(|c| is_thai(*c)) {
                if !ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
                    faults.insert(format!("{place}: {text:?}: no face draws {c:?} in {font:?}"));
                }
            }
        }
        for row in &t.galley.rows {
            let size = t.galley.job.sections.first().map_or(13.0, |s| s.format.font_id.size);
            let mut base: Option<&egui::epaint::text::Glyph> = None;
            for g in &row.row.glyphs {
                if !is_mark(g.chr) {
                    if g.chr != MARK {
                        base = Some(g);
                    }
                    continue;
                }
                if g.uv_rect.is_nothing() {
                    continue;
                }
                let Some(b) = base else {
                    faults.insert(format!("{place}: {text:?}: {:?} with no consonant before it", g.chr));
                    continue;
                };
                let x = g.pos.x + g.uv_rect.offset.x + g.uv_rect.size.x / 2.0;
                let y = g.uv_rect.offset.y + g.uv_rect.size.y / 2.0;
                let (left, right) = (b.pos.x - 1.5, b.pos.x + b.advance_width + 1.5);
                let stacked = if is_below(g.chr) { y > 0.0 } else { y < -0.35 * size };
                if x < left || x > right || !stacked {
                    faults
                        .insert(format!("{place}: {text:?}: {:?} at ({x:.1}, {y:.1}), its consonant {:?} spans {left:.1}–{right:.1}", g.chr, b.chr));
                }
            }
        }
    }
    thai_texts
}

/// The library, the edit view and each tab of Options in Thai at `size`, each painted as
/// `i18n-th-<place>-<size>.png`; what is cut and what is drawn wrong.
fn walk(size: (u32, u32)) -> (BTreeSet<String>, BTreeSet<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut d = thai_library(dir.path(), size);
    let tag = format!("{}x{}", size.0, size.1);
    let (mut cut, mut faults) = (BTreeSet::new(), BTreeSet::new());
    let mut see = |d: &mut Driver, place: &str| {
        let n = look(d, place, &mut cut, &mut faults);
        assert!(n > 5, "{place}: the interface is drawn in Thai ({n} Thai texts)");
        shot(d, &format!("i18n-th-{}-{tag}", place.replace(' ', "-")));
    };
    see(&mut d, "library");
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    see(&mut d, "edit basic");
    for tab in ["tuning", "effects"] {
        d.run("edit.tab", &json!({ "tab": tab })).unwrap();
        see(&mut d, &format!("edit {tab}"));
    }
    d.run("edit.tab", &json!({ "tab": "basic" })).unwrap();
    d.run("view.library", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    for tab in ["general", "fileTypes", "slideshow", "network", "people", "viewer", "export", "shortcuts"] {
        d.run("app.preferences", &json!({ "tab": tab })).unwrap();
        see(&mut d, &format!("options {tab}"));
        d.key("Escape").unwrap();
        d.settle(FRAMES).unwrap();
    }
    (cut, faults)
}

#[test]
fn the_library_the_edit_view_and_options_in_thai_fit_and_wrap_between_words() {
    if CRAFT_FONTS.is_empty() {
        eprintln!("built without CRAFT_FONTS_DIR: the glyph and mark checks are skipped (the cut and wrap checks run)");
    }
    for size in [LARGE, SMALL] {
        let (cut, faults) = walk(size);
        assert!(cut.is_empty(), "{size:?}: {} labels clipped:\n{}", cut.len(), cut.into_iter().collect::<Vec<_>>().join("\n"));
        assert!(faults.is_empty(), "{size:?}: {} faults:\n{}", faults.len(), faults.into_iter().collect::<Vec<_>>().join("\n"));
    }
}

/// The texts the next frame draws.
fn next_frame_texts(d: &mut Driver) -> Vec<String> {
    d.step();
    d.drawn_texts().iter().map(|t| bare(t)).collect()
}

#[test]
fn switching_between_english_and_thai_keeps_the_view_the_selection_and_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let s = demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    let starred = d.app().session().unwrap().library().photo(first).unwrap().star;
    d.run("photo.star.toggle", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    let state = |d: &Driver| (d.state().view, d.state().selection.clone(), d.app().session().unwrap().undo_stack().undo_label().map(str::to_string));
    let before = state(&d);
    assert!(before.2.is_some(), "an edit to undo");
    assert!(next_frame_texts(&mut d).iter().any(|t| t == "Auto Fix"), "the edit view in English");
    for (code, word) in [("th", "แก้ไขอัตโนมัติ"), ("en", "Auto Fix"), ("th", "แก้ไขอัตโนมัติ")]
    {
        d.run("options.set", &json!({ "values": { "interface.language": code } })).unwrap();
        let texts = next_frame_texts(&mut d);
        assert!(texts.iter().any(|t| t == word), "{code}: {texts:?}");
        assert_eq!(state(&d), before, "{code}: the view, the selection and the undo history kept");
    }
    // Back in the library, Undo takes the star back, in Thai.
    d.run("view.library", &json!({})).unwrap();
    d.run("edit.undo", &json!({})).unwrap();
    assert_ne!(state(&d).2, before.2, "the history still undoes in Thai");
    assert_eq!(d.app().session().unwrap().library().photo(first).unwrap().star, starred, "the star undone");
}

#[test]
fn with_the_fonts_a_fingerprint_of_the_library_in_thai() {
    if CRAFT_FONTS.is_empty() {
        eprintln!("skipped with_the_fonts_a_fingerprint_of_the_library_in_thai: built without CRAFT_FONTS_DIR");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = thai_library(dir.path(), LARGE);
    let s = d.screenshot().unwrap();
    check_pixels(&format!("i18n-th-library-{}x{}", LARGE.0, LARGE.1), s.width, s.height, &s.rgba);
}
