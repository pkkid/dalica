//! The localisation core (`localization.md`): a catalog fixture's plain, context, id and plural
//! entries, with English for anything missing; every plural rule at 0, 1, 2, 5, 11, 21 and 22;
//! hostile catalogs read without a panic, their bad lines in English; regional tags finding their
//! closest language (`de-AT`, `pt-PT`, `zh-TW`, `zh-CN`); `tl!` / `tln!` and the formats in English
//! and in the pseudo-locale.

use dalica_ui::i18n::catalog::{Catalog, MAX_LINE_BYTES};
use dalica_ui::i18n::languages::{self, Language, resolve};
use dalica_ui::i18n::{self, ENGLISH, PSEUDO, Plural, format, pseudo};
use dalica_ui::{tl, tln};

const FIXTURE: &str = "# A test catalog: context<TAB>source<TAB>translation.\n\
\n\
\tFolder Manager\tGestionnaire de dossiers\n\
verb\tOpen\tOuvrir\n\
\tOpen\tOuvert\n\
@id\tphoto.star.toggle\tÉtoile\n\
\tStar / Unstar\tAjouter / retirer l'étoile\n\
\t{count} photos selected\t{count} photos sélectionnées\n\
@plural\t{n} photo|{n} photos\t{n} photo|{n} photos (pl)\n\
\tLine\\none\\tand a \\\\\tLigne\\nune\\tet un \\\\\n";

#[test]
fn a_catalog_translates_plain_context_id_and_plural_entries_and_falls_back_to_english() {
    let c = Catalog::parse(FIXTURE, Plural::OneOther);
    assert_eq!(c.problems(), &[] as &[String]);
    assert_eq!(c.len(), 8);
    assert_eq!(c.tr("Folder Manager"), "Gestionnaire de dossiers");
    assert_eq!(c.tr("Not in the catalog"), "Not in the catalog");
    // A context wins; an unknown context falls back to the plain entry, then to English.
    assert_eq!(c.tr_ctx("verb", "Open"), "Ouvrir");
    assert_eq!(c.tr_ctx("noun", "Open"), "Ouvert");
    assert_eq!(c.tr_ctx("verb", "Close"), "Close");
    // An id wins over the English text; an unknown id uses the English text's entry.
    assert_eq!(c.tr_id("photo.star.toggle", "Star / Unstar"), "Étoile");
    assert_eq!(c.tr_id("photo.star.set", "Star / Unstar"), "Ajouter / retirer l'étoile");
    assert_eq!(c.tr_id("photo.star.set", "Set Star"), "Set Star");
    assert_eq!(c.tr_plural(Plural::OneOther, 1, "{n} photo", "{n} photos"), "{n} photo");
    assert_eq!(c.tr_plural(Plural::OneOther, 3, "{n} photo", "{n} photos"), "{n} photos (pl)");
    assert_eq!(c.tr_plural(Plural::OneOther, 3, "{n} album", "{n} albums"), "{n} albums");
    assert_eq!(c.tr_plural(Plural::OneOther, 1, "{n} album", "{n} albums"), "{n} album");
    assert_eq!(c.tr("Line\none\tand a \\"), "Ligne\nune\tet un \\");
    let filled = i18n::fill(c.tr("{count} photos selected"), &[("count", &12)]);
    assert_eq!(filled, "12 photos sélectionnées");
}

/// The category of each count of [`COUNTS`], per rule.
const COUNTS: [u64; 7] = [0, 1, 2, 5, 11, 21, 22];

#[test]
fn every_plural_rule_picks_the_right_form() {
    let table: [(Plural, [&str; 7]); 6] = [
        (Plural::Single, ["other"; 7]),
        (Plural::OneOther, ["other", "one", "other", "other", "other", "other", "other"]),
        (Plural::ZeroOneOther, ["one", "one", "other", "other", "other", "other", "other"]),
        (Plural::EastSlavic, ["many", "one", "few", "many", "many", "one", "few"]),
        (Plural::Polish, ["many", "one", "few", "many", "many", "many", "few"]),
        (Plural::Czech, ["other", "one", "few", "other", "other", "other", "other"]),
    ];
    assert_eq!(table.len(), Plural::ALL.len());
    for (rule, wanted) in table {
        let got: Vec<&str> = COUNTS.iter().map(|&n| rule.category(n)).collect();
        assert_eq!(got, wanted, "{rule:?}");
        assert!(COUNTS.iter().all(|&n| rule.index(n) < rule.forms()), "{rule:?}");
    }
    // Teens are never `one` or `few` in the Slavic rules, at any magnitude.
    for rule in [Plural::EastSlavic, Plural::Polish] {
        for n in [111, 112, 1012, 1_000_011] {
            assert_eq!(rule.category(n), "many", "{rule:?} {n}");
        }
        assert_eq!(rule.category(u64::MAX), "many");
    }
}

/// A Thai-like language: its own script and a single form, so a plural lists one form.
static SINGLE_FORM: Language =
    Language {
        code: "th", endonym: "ไทย", script: "Thai", plural: Plural::Single, complete_menus: false, catalog: "", decimal: ".", thousands: ","
    };

#[test]
fn a_single_form_language_lists_one_plural_form() {
    let text = "@plural\t{n} photo|{n} photos\tรูป {n} รูป\n@plural\t{n} album|{n} albums\tอัลบั้ม {n}|อัลบั้ม {n}\n";
    let c = Catalog::parse(text, SINGLE_FORM.plural);
    assert_eq!(c.problem_count(), 1, "{:?}", c.problems());
    assert!(c.problems()[0].starts_with("line 2:"), "{:?}", c.problems());
    for n in COUNTS {
        assert_eq!(c.tr_plural(SINGLE_FORM.plural, n, "{n} photo", "{n} photos"), "รูป {n} รูป");
    }
    // The malformed line is English.
    assert_eq!(c.tr_plural(SINGLE_FORM.plural, 2, "{n} album", "{n} albums"), "{n} albums");
}

#[test]
fn hostile_catalogs_never_panic_and_their_bad_lines_show_in_english() {
    let long = format!("\t{}\tx\n", "a".repeat(1 << 20));
    let cases: [(&str, &str); 9] = [
        ("a lone tab (a blank line), then a lone column", "\t\nOpen\n"),
        ("two columns", "\tOpen\n"),
        ("four columns", "\tOpen\tOuvrir\textra\n"),
        ("a placeholder in the source only", "\t{count} photos\tdes photos\n"),
        ("a placeholder in the translation only", "\tPhotos\t{count} photos\n"),
        ("a broken escape", "\tOpen\\x\tOuvrir\n"),
        ("a trailing backslash", "\tOpen\tOuvrir\\\n"),
        ("a malformed plural", "@plural\t{n} photo\t{n} photo|{n} photos\n"),
        ("an empty translation", "\tOpen\t \n"),
    ];
    for (what, text) in cases.iter().copied().chain([("a 1 MiB line", long.as_str())]) {
        for rule in Plural::ALL {
            let c = Catalog::parse(text, rule);
            assert!(c.is_empty(), "{what}: nothing is read");
            assert_eq!(c.problem_count(), 1, "{what}: {:?}", c.problems());
            assert_eq!(c.tr("Open"), "Open", "{what}");
            assert_eq!(c.tr("{count} photos"), "{count} photos", "{what}");
        }
    }
    assert!(long.len() > MAX_LINE_BYTES);
    // A key given twice keeps the first; good lines around bad ones are read.
    let c = Catalog::parse("\tOpen\tOuvrir\n\tOpen\tOuvert\n\x00\t\u{feff}\n\tClose\tFermer\n", Plural::OneOther);
    assert_eq!((c.len(), c.problem_count()), (2, 2), "{:?}", c.problems());
    assert_eq!((c.tr("Open"), c.tr("Close")), ("Ouvrir", "Fermer"));
    // Arbitrary bytes, many lines, nested braces: read without a panic.
    let noise: String = (0..20_000u32).map(|i| char::from_u32((i * 7919) % 0x3000).unwrap_or('\t')).collect();
    for rule in Plural::ALL {
        let _ = Catalog::parse(&noise, rule);
        let _ = Catalog::parse("{{{\t}}}\t{{a}}\n@plural\t|\t|\n@plural\ta|b\t||||\n", rule);
    }
}

/// The registry the tag tests resolve against: one language per case the spec names.
fn fixture(code: &'static str, plural: Plural, script: &'static str) -> &'static Language {
    Box::leak(Box::new(Language { code, endonym: code, script, plural, complete_menus: false, catalog: "", decimal: ".", thousands: "," }))
}

#[test]
fn regional_tags_resolve_to_the_closest_language() {
    let en = fixture("en", Plural::OneOther, "Latn");
    let de = fixture("de", Plural::OneOther, "Latn");
    let pt = fixture("pt-BR", Plural::ZeroOneOther, "Latn");
    let hant = fixture("zh-Hant", Plural::Single, "Hant");
    let hans = fixture("zh-Hans", Plural::Single, "Hans");
    let th = fixture("th", Plural::Single, "Thai");
    let registry = [en, de, pt, hant, hans, th];
    let code = |tag: &str| resolve(tag, &registry).map(|l| l.code);
    for (tag, wanted) in [
        ("de-AT", Some("de")),
        ("de_AT.UTF-8", Some("de")),
        ("pt-PT", Some("pt-BR")),
        ("pt", Some("pt-BR")),
        ("PT-br", Some("pt-BR")),
        ("zh-TW", Some("zh-Hant")),
        ("zh_HK.UTF-8", Some("zh-Hant")),
        ("zh-Hant-HK", Some("zh-Hant")),
        ("zh-CN", Some("zh-Hans")),
        ("zh-SG", Some("zh-Hans")),
        ("zh-Hans-TW", Some("zh-Hans")),
        ("zh", Some("zh-Hans")),
        ("th-TH", Some("th")),
        ("en-GB", Some("en")),
        ("C", Some("en")),
        ("POSIX", Some("en")),
        ("sv-SE", None),
        ("", None),
        ("_", None),
    ] {
        assert_eq!(code(tag), wanted, "{tag}");
    }
    // Traditional and Simplified Chinese never stand in for each other.
    assert_eq!(resolve("zh-TW", &[en, hans]).map(|l| l.code), None);
    assert_eq!(resolve("zh-CN", &[en, hant]).map(|l| l.code), None);
}

#[test]
fn the_setting_the_system_and_the_registry_decide_the_language() {
    let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // English is the registry until the first catalogs land; anything unknown is English.
    assert_eq!(i18n::resolve_setting("system", &tags(&["de-AT", "fr-FR"])).code, "en");
    assert_eq!(i18n::resolve_setting("system", &tags(&["en-GB"])).code, "en");
    assert_eq!(i18n::resolve_setting("xx-unknown", &[]).code, "en");
    assert_eq!(i18n::resolve_setting("QPS", &[]).code, "qps");
    assert!(i18n::LANGUAGES.first().is_some_and(|l| l.code == "en"));
    // The Language list: the registry, then the pseudo-locale; each code unique and found again.
    let choices = i18n::choices();
    assert_eq!(choices.first(), Some(&("en", "English")));
    assert_eq!(choices.last(), Some(&("qps", "Pseudo-locale")));
    for (code, _) in &choices {
        assert_eq!(languages::by_code(code).map(|l| l.code), Some(*code));
        assert_eq!(choices.iter().filter(|(c, _)| c == code).count(), 1);
    }
    for lang in languages::all() {
        assert!(!lang.script.is_empty() && lang.script.len() == 4, "{}: an ISO 15924 script", lang.code);
    }
    // The system's tags as read: bounded and clean.
    let read = i18n::system::bounded(["  fr-CA ".into(), String::new(), "x\0".into(), "a".repeat(200)]);
    assert_eq!(read, ["fr-CA"]);
    assert!(i18n::system::tags().len() <= i18n::system::MAX_TAGS);
}

#[test]
fn tl_and_tln_in_english_and_in_the_pseudo_locale() {
    let n = 3usize;
    let english = i18n::with_language(&ENGLISH, || {
        (
            tl!("Folder Manager"),
            tl!(ctx = "verb", "Open"),
            tl!("{count} photos selected", count = n),
            tln!(1, "{n} photo", "{n} photos"),
            tln!(1234, "{n} photo", "{n} photos"),
        )
    });
    assert_eq!(english, ("Folder Manager".into(), "Open".into(), "3 photos selected".into(), "1 photo".into(), "1,234 photos".into()));
    let pseudo = i18n::with_language(&PSEUDO, || {
        assert!(i18n::is_pseudo());
        (
            tl!("Folder Manager"),
            tl!("{count} photos selected", count = n),
            tln!(n, "{n} photo", "{n} photos"),
            i18n::option_label("interface.language", "Language"),
        )
    });
    assert_eq!(pseudo.0, "⁅Folder Manager······⁆");
    assert_eq!(pseudo.1, format!("⁅3 photos selected{}⁆", "·".repeat(10)));
    assert_eq!(pseudo.2, "⁅3 photos····⁆");
    assert_eq!(pseudo.3, "⁅Language····⁆");
    // The language goes back after `with_language`.
    assert_eq!(i18n::current().code, "en");
    // 40 % longer, rounded up, plus the marks.
    for s in ["", "a", "OK", "Options", "Import From…", "ไทย"] {
        let p = pseudo::translate(s);
        let (len, plen) = (s.chars().count(), p.chars().count());
        assert_eq!(plen, len + (len * 2).div_ceil(5) + 2, "{s}");
        assert!(p.starts_with(pseudo::OPEN) && p.ends_with(pseudo::CLOSE) && p.contains(s));
    }
    assert_eq!(pseudo::untranslated(["⁅Options··⁆", "Options", "42", "IMG_0001.jpg", "⁅Language····⁆:"]), ["Options", "IMG_0001.jpg"]);
    // A value's own braces are never filled; an unknown placeholder stays.
    assert_eq!(i18n::fill("{a} and {b} {c", &[("a", &"{b}"), ("b", &2)]), "{b} and 2 {c");
}

#[test]
fn numbers_sizes_and_dates_go_through_the_language() {
    let date = dalica_core::Date::new(2026, 6, 4).unwrap();
    let english = i18n::with_language(&ENGLISH, || {
        (
            format::number(0),
            format::number(1_234_567),
            format::decimal(-1234.56, 1),
            format::decimal(f64::NAN, 2),
            format::file_size(1),
            format::file_size(512),
            format::file_size(3_100_000),
            format::file_size(u64::MAX),
            format::long_date(date),
            format::long_date_with_weekday(date),
            format::month_name(13),
        )
    });
    assert_eq!(english.0, "0");
    assert_eq!(english.1, "1,234,567");
    assert_eq!(english.2, "-1,234.6");
    assert_eq!(english.3, "0.00");
    assert_eq!(english.4, "1 byte");
    assert_eq!(english.5, "512 bytes");
    assert_eq!(english.6, "3.1 MB");
    assert_eq!(english.7, "18,446,744.1 TB");
    assert_eq!(english.8, "June 4, 2026");
    assert_eq!(english.9, "Thursday, June 4, 2026");
    assert_eq!(english.10, "?");
    assert_eq!(format::weekday(dalica_core::Date::new(2000, 1, 1).unwrap()), 6);
    let pseudo = i18n::with_language(&PSEUDO, || (format::file_size(3_100_000), format::long_date(date)));
    assert!(pseudo.0.starts_with("⁅3.1 MB") && pseudo.1.starts_with("⁅⁅June··⁆ 4, 2026"), "{pseudo:?}");
}

/// The texts the next frame draws.
fn next_frame_texts(d: &mut dalica_ui::Driver) -> Vec<String> {
    d.step();
    d.drawn_texts().to_vec()
}

#[test]
fn choosing_the_pseudo_locale_over_options_set_changes_the_next_frame_and_keeps_the_view() {
    use crate::common::{FRAMES, LARGE, demo_open, new_driver};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let session = demo_open(dir.path(), std::sync::Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(dalica_ui::App::new(Some(session)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // A selection and an undo step, then Options ▸ General open over the library.
    d.run("select.all", &json!({})).unwrap();
    let ids: Vec<u64> = d.state().selection.ids.iter().take(2).map(|id| id.get()).collect();
    assert_eq!(ids.len(), 2, "the demo library's photos are selected");
    d.run("photo.star.toggle", &json!({ "ids": ids })).unwrap();
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.record_texts(true);
    let before = (d.state().view, d.state().selection.clone(), d.state().dialogs.len());
    let undo = || "Star / Unstar";
    assert_eq!(d.app().session().unwrap().undo_stack().undo_label(), Some(undo()));
    let english = next_frame_texts(&mut d);
    assert!(english.iter().any(|t| t == "Language:"), "{english:?}");
    assert!(english.iter().all(|t| !pseudo::is_marked(t)), "{english:?}");

    d.run("options.set", &json!({ "values": { "interface.language": "qps" } })).unwrap();
    let texts = next_frame_texts(&mut d);
    assert!(texts.iter().any(|t| t.starts_with("⁅⁅Language") && t.ends_with('⁆')), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("⁅Use animations")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Pseudo-locale"), "the language's own name: {texts:?}");
    assert!(pseudo::untranslated(texts.iter().map(String::as_str)).contains(&"Pseudo-locale"), "untranslated texts are named");
    assert_eq!((d.state().view, d.state().selection.clone(), d.state().dialogs.len()), before, "the view, the selection and the dialog kept");
    assert_eq!(d.app().session().unwrap().undo_stack().undo_label(), Some(undo()), "the undo history kept");

    d.run("options.set", &json!({ "values": { "interface.language": "system" } })).unwrap();
    let back = next_frame_texts(&mut d);
    assert!(back.iter().any(|t| t == "Language:") && back.iter().all(|t| !pseudo::is_marked(t)), "{back:?}");
}

#[test]
fn the_pseudo_locale_marks_have_glyphs_in_the_ui_fonts() {
    use dalica_ui_kit::{Tokens, fonts};
    let t = Tokens::default();
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    // The fonts are in place from the second frame on.
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |_| {}).textures_delta.clear();
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |ui| {
        for font in [fonts::regular(t.body), fonts::semibold(t.body)] {
            let uv = |c: char| {
                let g = ui.painter().layout_no_wrap(c.to_string(), font.clone(), egui::Color32::WHITE);
                g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.uv_rect)
            };
            let none = uv('\u{10FFFD}');
            assert_eq!(uv('\u{10FFFE}'), none, "the check sees a glyph the fonts lack");
            for c in [pseudo::OPEN, pseudo::CLOSE, pseudo::PAD] {
                assert_ne!(uv(c), none, "{c:?} has a glyph, so the pseudo-locale draws no missing-glyph box");
            }
        }
    })
    .textures_delta
    .clear();
}
