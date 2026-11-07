//! French (`localization.md`, `localization-fr.md`): registered after Thai, its catalog reads
//! without a fault and covers every command label, the singular for 0 and 1, a decimal comma and
//! a narrow no-break space between thousands, dates day first, and `fr-CA` finds it.

use dalica_core::Date;
use dalica_ui::i18n::{self, Plural, format, languages};
use dalica_ui::{tl, tln};

fn french() -> &'static languages::Language {
    languages::by_code("fr").expect("French is registered")
}

#[test]
fn frenchs_catalog_reads_whole_and_is_listed_by_its_own_name() {
    let fr = french();
    assert_eq!((fr.endonym, fr.script, fr.plural, fr.complete_menus), ("Français", "Latn", Plural::ZeroOneOther, true));
    let c = i18n::catalog(fr);
    assert_eq!(c.problems(), &[] as &[String], "every line of fr.tsv is read");
    assert!(c.len() > 1500, "{} entries", c.len());
    eprintln!("fr: {} entries, {} drafts awaiting the fluent review", c.len(), c.draft_count());
    assert!(i18n::choices().contains(&("fr", "Français")));
    assert_eq!(i18n::resolve_setting("fr-CA", &[]).code, "fr");
    assert_eq!(i18n::resolve_setting("system", &["fr_BE.UTF-8".to_string()]).code, "fr");
}

#[test]
fn every_command_has_a_french_label() {
    let mut labels: Vec<(&str, &str)> = dalica_engine::cmd::specs().iter().map(|c| (c.id, c.label)).collect();
    labels.extend(dalica_ui::commands::UI_COMMANDS.iter().map(|c| (c.id, c.label)));
    let c = i18n::catalog(french());
    let missing: Vec<String> = labels
        .iter()
        .filter(|(id, label)| !label.trim().is_empty() && c.id(id).is_none() && c.plain(label).is_none())
        .map(|(id, label)| format!("{id}: {label:?}"))
        .collect();
    assert!(missing.is_empty(), "{} commands with no French label:\n{}", missing.len(), missing.join("\n"));
}

#[test]
fn french_plurals_numbers_and_dates() {
    i18n::with_language(french(), || {
        for (n, want) in [(0u64, "0 photo"), (1, "1 photo"), (2, "2 photos"), (21, "21 photos")] {
            assert_eq!(tln!(n, "{n} photo", "{n} photos"), want, "{n}");
        }
        assert_eq!(tl!("Folder Manager"), "Gestionnaire de dossiers");
        assert_eq!(tl!("Could not save {name}: {reason}", name = "IMG_0001.JPG", reason = "x"), "Impossible d'enregistrer IMG_0001.JPG : x");
        assert_eq!(format::number(1_234_567), "1\u{202f}234\u{202f}567");
        assert_eq!(format::decimal(3.5, 1), "3,5");
        assert_eq!(format::file_size(3_100_000), "3,1 Mo");
        let date = Date::new(2026, 6, 4).expect("a date");
        assert_eq!(format::long_date(date), "4 juin 2026");
        assert_eq!(format::long_date_with_weekday(date), "jeudi 4 juin 2026");
    });
}
