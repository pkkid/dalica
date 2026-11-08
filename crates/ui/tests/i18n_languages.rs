//! The registered languages (M15.35, `localization.md`, `localization-th.md`,
//! `localization-fr.md`). English comes first; every other language's catalog reads without a
//! fault, every command's label has an entry (the macro-made ones `cargo xtask i18n` cannot read
//! included), and each writes counts, numbers, sizes, dates and times its own way. The library,
//! the edit view and the Options dialog drawn in each language at 1400 × 900 and 900 × 600 (Thai
//! on every run, the others at full strength): no label cut at the side of its area; in Thai also
//! no text left in English but user data and the names kept in Latin letters, every wrapped label
//! broken only between words, and (built with `CRAFT_FONTS_DIR`) every character drawn by a face
//! that has it, every mark above or below its consonant. Thai's one plural form reads right for 0,
//! 1, 2, 5 and 21 and its text carries break marks between words (`docs/thai-text.md`); every
//! Basic Fixes label is drawn whole in Thai (on two lines if need be, never cut with "…");
//! switching between English and Thai keeps the view, the selection and the undo history; and
//! (with the fonts) a fingerprint of the library in Thai at 1400 × 900. `DALICA_SHOTS=<dir>`
//! writes the windows (`i18n-<code>-*.png`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_pixels, demo_open, full, new_driver, shot};
use dalica_core::{Date, DateTime};
use dalica_ui::i18n::{self, ENGLISH, LANGUAGES, Plural, format, languages};
use dalica_ui::{App, Driver, tl, tln};
use dalica_ui_kit::fonts::CRAFT_FONTS;
use dalica_ui_kit::linebreak::{MARK, is_thai};
use egui::epaint::{ClippedShape, Shape, TextShape};
use serde_json::json;

fn thai() -> &'static languages::Language {
    languages::by_code("th").expect("Thai is registered")
}

/// Every registered language but English: its row, a regional tag and a POSIX locale that find it.
const ROWS: &[(&str, &str, &str, Plural, &str, &str)] =
    &[("th", "ไทย", "Thai", Plural::Single, "th-TH", "th_TH.UTF-8"), ("fr", "Français", "Latn", Plural::ZeroOneOther, "fr-CA", "fr_BE.UTF-8")];

#[test]
fn the_registry_lists_english_first_and_every_catalog_reads_whole() {
    assert_eq!(LANGUAGES.first().map(|l| l.code), Some("en"));
    let codes: Vec<&str> = LANGUAGES.iter().skip(1).map(|l| l.code).collect();
    assert_eq!(codes, ROWS.iter().map(|r| r.0).collect::<Vec<_>>(), "a row per registered language");
    for &(code, endonym, script, plural, regional, posix) in ROWS {
        let lang = languages::by_code(code).unwrap();
        assert_eq!((lang.endonym, lang.script, lang.plural, lang.complete_menus), (endonym, script, plural, true), "{code}");
        let c = i18n::catalog(lang);
        assert_eq!(c.problems(), &[] as &[String], "every line of {code}.tsv is read");
        assert!(c.len() > 1500, "{code}: {} entries", c.len());
        // Every entry is machine-assisted until the fluent review takes its `~` away.
        assert!(c.draft_count() <= c.len());
        eprintln!("{code}: {} entries, {} drafts awaiting the fluent review", c.len(), c.draft_count());
        // Options ▸ General ▸ Language lists it by its own name; a regional tag finds it.
        assert!(i18n::choices().contains(&(code, endonym)), "{code}");
        assert_eq!(i18n::resolve_setting(regional, &[]).code, code);
        assert_eq!(i18n::resolve_setting("system", &[posix.to_string()]).code, code);
    }
}

#[test]
fn every_command_has_a_label_in_every_complete_language() {
    // The labels `cargo xtask i18n` cannot read from the sources (made by a macro) are checked
    // here, from the registries themselves.
    let mut labels: Vec<(&str, &str)> = dalica_engine::cmd::specs().iter().map(|c| (c.id, c.label)).collect();
    labels.extend(dalica_ui::commands::UI_COMMANDS.iter().map(|c| (c.id, c.label)));
    for lang in LANGUAGES.iter().filter(|l| l.complete_menus) {
        let c = i18n::catalog(lang);
        let missing: Vec<String> = labels
            .iter()
            .filter(|(id, label)| !label.trim().is_empty() && c.id(id).is_none() && c.plain(label).is_none())
            .map(|(id, label)| format!("{id}: {label:?}"))
            .collect();
        assert!(missing.is_empty(), "{}: {} commands with no label:\n{}", lang.code, missing.len(), missing.join("\n"));
    }
}

/// What a language writes for 0, 1, 2 and 21 photos, 1234567, 3.5, 3.1 MB, 4 June 2026 (plain and
/// with its weekday) and 15:05 on that day (the long form, and the clock with seconds).
struct Writes {
    code: &'static str,
    photos: [&'static str; 4],
    number: &'static str,
    decimal: &'static str,
    size: &'static str,
    date: &'static str,
    weekday: &'static str,
    date_time: &'static str,
    clock: &'static str,
}

const WRITES: &[Writes] = &[
    Writes {
        code: "en",
        photos: ["0 photos", "1 photo", "2 photos", "21 photos"],
        number: "1,234,567",
        decimal: "3.5",
        size: "3.1 MB",
        date: "June 4, 2026",
        weekday: "Thursday, June 4, 2026",
        date_time: "June 4, 2026 3:05 PM",
        clock: "3:05:09 PM",
    },
    Writes {
        code: "fr",
        photos: ["0 photo", "1 photo", "2 photos", "21 photos"],
        number: "1\u{202f}234\u{202f}567",
        decimal: "3,5",
        size: "3,1 Mo",
        date: "4 juin 2026",
        weekday: "jeudi 4 juin 2026",
        date_time: "4 juin 2026, 15:05",
        clock: "15:05:09",
    },
];

#[test]
fn every_language_writes_counts_numbers_dates_and_times_its_own_way() {
    let date = Date::new(2026, 6, 4).unwrap();
    let at = DateTime::new(date, 15, 5, 9).unwrap();
    let morning = DateTime::new(date, 9, 5, 0).unwrap();
    for w in WRITES {
        let lang = languages::by_code(w.code).unwrap();
        i18n::with_language(lang, || {
            for (n, want) in [0u64, 1, 2, 21].into_iter().zip(w.photos) {
                assert_eq!(bare(&tln!(n, "{n} photo", "{n} photos")), want, "{}: {n}", w.code);
            }
            assert_eq!(format::number(1_234_567), w.number, "{}", w.code);
            assert_eq!(format::decimal(3.5, 1), w.decimal, "{}", w.code);
            assert_eq!(format::file_size(3_100_000), w.size, "{}", w.code);
            assert_eq!(format::long_date(date), w.date, "{}", w.code);
            assert_eq!(format::long_date_with_weekday(date), w.weekday, "{}", w.code);
            assert_eq!(dalica_ui::status_text::date_time(at), w.date_time, "{}", w.code);
            assert_eq!(dalica_ui::status_text::clock(at, true), w.clock, "{}", w.code);
            // The 24-hour clock writes the hour with two digits; the twelve-hour one does not.
            let nine = dalica_ui::status_text::clock(morning, false);
            assert!(nine.starts_with(if lang.clock24 { "09:05" } else { "9:05" }), "{}: {nine:?}", w.code);
        });
    }
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
fn thai_library(dir: &std::path::Path, size: (u32, u32)) -> Driver {
    library_in(dir, "th", size)
}

/// The demo library drawn in the language `code` at `(w, h)`.
fn library_in(dir: &std::path::Path, code: &str, (w, h): (u32, u32)) -> Driver {
    let s = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.run("options.set", &json!({ "values": { "interface.language": code } })).unwrap();
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

/// The names that stay in Latin letters (`localization-th.md` → What stays as written).
const KEPT: &[&str] = &["Dalica", "A|B", "A|A", "px", "AVIF", "BMP", "GIF", "JPEG", "JPEG XL", "PNG", "PSD", "TIFF", "WebP", "RAW", "HEIC/HEIF"];

/// A text with no Thai that is right as it is: no word in it, a kept name, user data (a folder's
/// name or path, the camera named in a photo), a key, a web address, a credit.
fn kept_in_latin(text: &str, data: &BTreeSet<String>) -> bool {
    let t = text.trim();
    t.chars().filter(|c| c.is_ascii_alphabetic()).count() < 2
        || KEPT.contains(&t)
        || data.iter().any(|d| t.contains(d.as_str()))
        || dalica_engine::Key::parse(t).is_ok()
        || t.starts_with("https://")
        || t.starts_with('/')
        || t.contains(":\\")
        || t.contains('©')
}

/// The texts no catalog may translate: the folders' names and paths, the photos' names, and the
/// camera the demo photos name.
fn user_data(d: &Driver) -> BTreeSet<String> {
    let lib = d.app().session().unwrap().library();
    let mut out: BTreeSet<String> = ["Dalica Scenes 1".to_string()].into_iter().collect();
    for f in lib.folders() {
        out.insert(f.path.display().to_string());
        out.extend(f.path.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    out.extend(lib.photos().map(|p| p.name.clone()));
    out.extend(std::env::var("HOME").ok().filter(|h| h.len() > 1));
    out.retain(|s| !s.trim().is_empty());
    out
}

/// Thai marks drawn above or below their consonant (สระอำ, a spacing vowel, aside).
fn is_mark(c: char) -> bool {
    matches!(c, '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}') && c != '\u{0E4D}'
}

fn is_below(c: char) -> bool {
    matches!(c, '\u{0E38}'..='\u{0E3A}')
}

/// What the next frame draws wrong, each with `place`: a label cut at the side of its area (`cut`);
/// in Thai (`thai`) also a text left in English (not user data in `data`, nor a name kept in Latin
/// letters), a label wrapped inside a word, a character no face has, a mark away from its
/// consonant (`faults`; the last two only with the fonts). Returns how many Thai texts it drew.
fn look(d: &mut Driver, place: &str, thai: bool, data: &BTreeSet<String>, cut: &mut BTreeSet<String>, faults: &mut BTreeSet<String>) -> usize {
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
        if !thai {
            continue;
        }
        if !text.chars().any(is_thai) {
            if !kept_in_latin(&text, data) {
                faults.insert(format!("{place}: {text:?} is drawn in English"));
            }
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

/// The library, the edit view and each tab of Options in the language `code` at `size`, each
/// painted as `i18n-<code>-<place>-<size>.png`; what is cut and what is drawn wrong.
fn walk(code: &str, size: (u32, u32)) -> (BTreeSet<String>, BTreeSet<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut d = library_in(dir.path(), code, size);
    let thai = languages::by_code(code).is_some_and(|l| l.script == "Thai");
    let tag = format!("{}x{}", size.0, size.1);
    let (mut cut, mut faults) = (BTreeSet::new(), BTreeSet::new());
    let data = user_data(&d);
    let mut see = |d: &mut Driver, place: &str| {
        let n = look(d, place, thai, &data, &mut cut, &mut faults);
        assert!(!thai || n > 5, "{place}: the interface is drawn in Thai ({n} Thai texts)");
        shot(d, &format!("i18n-{code}-{}-{tag}", place.replace(' ', "-")));
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
fn the_library_the_edit_view_and_options_fit_in_every_language_and_thai_wraps_between_words() {
    if CRAFT_FONTS.is_empty() {
        eprintln!("built without CRAFT_FONTS_DIR: the glyph and mark checks are skipped (the cut and wrap checks run)");
    }
    // Thai on every run; the other languages at full strength (the review gate).
    let codes: Vec<&str> = LANGUAGES.iter().skip(1).map(|l| l.code).filter(|c| *c == "th" || full()).collect();
    for code in codes {
        for size in [LARGE, SMALL] {
            let (cut, faults) = walk(code, size);
            assert!(cut.is_empty(), "{code} {size:?}: {} labels clipped:\n{}", cut.len(), cut.into_iter().collect::<Vec<_>>().join("\n"));
            assert!(faults.is_empty(), "{code} {size:?}: {} faults:\n{}", faults.len(), faults.into_iter().collect::<Vec<_>>().join("\n"));
        }
    }
}

/// In Thai at 900 × 600 a Basic Fixes label too wide for its button (Auto Contrast) wraps onto a
/// second line at a break mark rather than being cut with "…"; the grid grows by that line and the
/// Fill Light row still ends above Undo / Redo.
#[test]
fn in_thai_every_basic_fixes_label_is_drawn_whole() {
    use dalica_ui::panels::edit::{basic, panel};
    let dir = tempfile::tempdir().unwrap();
    let mut d = thai_library(dir.path(), SMALL);
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let buttons: Vec<(String, egui::Rect)> = basic::TOOLS
        .iter()
        .map(|(id, ..)| {
            let r = crate::common::widget(&d, id).rect;
            (id.to_string(), egui::Rect::from_min_size(egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3])))
        })
        .collect();
    let shapes = crate::accessibility::frame(&mut d, Vec::new()).shapes;
    let labels: Vec<(String, TextShape)> = texts(&shapes)
        .into_iter()
        .filter_map(|(t, _)| buttons.iter().find(|(_, r)| r.contains_rect(t.visual_bounding_rect())).map(|(id, _)| (id.clone(), t)))
        .collect();
    assert_eq!(labels.len(), 6, "{:?}", labels.iter().map(|l| &l.0).collect::<Vec<_>>());
    let cut: Vec<String> = labels.iter().filter(|(_, t)| t.galley.elided).map(|(id, t)| format!("{id}: {:?}", t.galley.text())).collect();
    assert!(cut.is_empty(), "labels cut with \u{2026}: {cut:?}");
    let wrapped = labels.iter().find(|(id, _)| id == "tool:autocontrast").map(|(_, t)| t.galley.rows.len());
    assert_eq!(wrapped, Some(2), "Auto Contrast takes two lines");
    let (contrast, undo) = (crate::common::widget(&d, "tool:autocontrast").rect, crate::common::widget(&d, "button:edit.undo").rect);
    let fill_bottom = contrast[1] + contrast[3] + basic::FILL_GAP + basic::FILL_H;
    assert!(fill_bottom <= undo[1] - panel::MARGIN + 0.01, "Fill Light ends at {fill_bottom}, under Undo at {undo:?}");
    shot(&d, "i18n-th-basic-fixes-900x600");
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
    // English chosen, not System: the machine running the test may prefer another language.
    d.run("options.set", &json!({ "values": { "interface.language": "en" } })).unwrap();
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
