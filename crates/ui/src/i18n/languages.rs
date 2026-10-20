//! The language registry (`localization.md` → Languages and plural rules) and how a language tag
//! (a setting, `DALICA_LANGUAGE`, one of the system's preferred languages) finds its language.
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/i18n/mod.rs` (`LANGUAGES`, `candidates`,
//! `lang_from_tag`; revision d95fdf31; MIT OR Apache-2.0); the script field follows LightCraft
//! `crates/ui-egui/src/i18n.rs` (revision d999e382): an ISO 15924 code, so no script set is
//! assumed (`Latn`, `Thai`, `Hans`, `Hant`, `Jpan`, `Kore`, `Cyrl`, …).
//!
//! `cargo xtask i18n` reads [`LANGUAGES`] from this file: keep one `Language { … }` per entry,
//! with `code: "…"`, `plural: Plural::…`, `complete_menus: …` and the catalog as
//! `include_str!("<code>.tsv")` (or `""` for English, the source language).

use super::plural::Plural;

/// One language the interface can be drawn in.
#[derive(Debug)]
pub struct Language {
    /// The BCP 47 tag, as stored in `interface.language` (`en`, `th`, `zh-Hans`, `pt-BR`).
    pub code: &'static str,
    /// Its name in itself, as the Language list shows it (never translated).
    pub endonym: &'static str,
    /// The ISO 15924 script its text is written in: it picks the fallback faces.
    pub script: &'static str,
    pub plural: Plural,
    /// Whether the catalog covers every enforced key (`cargo xtask i18n` fails on a missing one).
    pub complete_menus: bool,
    /// The catalog's TSV text (empty for English).
    pub catalog: &'static str,
    /// The decimal mark and the thousands separator of numbers.
    pub decimal: &'static str,
    pub thousands: &'static str,
}

/// English: the source language and the fallback.
pub const ENGLISH: Language = Language {
    code: "en",
    endonym: "English",
    script: "Latn",
    plural: Plural::OneOther,
    complete_menus: false,
    catalog: "",
    decimal: ".",
    thousands: ",",
};

/// Thai (M15.35): a machine-assisted catalog, every entry a draft for the fluent review
/// (`docs/localization-th.md`); one plural form; its text marked for line breaks at load
/// (`docs/thai-text.md`).
pub const THAI: Language = Language {
    code: "th",
    endonym: "ไทย",
    script: "Thai",
    plural: Plural::Single,
    complete_menus: true,
    catalog: include_str!("th.tsv"),
    decimal: ".",
    thousands: ",",
};

/// The registry, English first.
pub static LANGUAGES: &[Language] = &[ENGLISH, THAI];

/// The pseudo-locale's code (`localization.md` → Checking a translation): every text bracketed
/// and 40 % longer, built at run time from the English ([`super::pseudo`]).
pub const PSEUDO_CODE: &str = "qps";

/// The pseudo-locale. Not in [`LANGUAGES`]: it has no catalog and no glyphs of its own.
pub static PSEUDO: Language = Language {
    code: PSEUDO_CODE,
    endonym: "Pseudo-locale",
    script: "Latn",
    plural: Plural::OneOther,
    complete_menus: false,
    catalog: "",
    decimal: ".",
    thousands: ",",
};

/// The tag `tag` written plainly: the encoding and modifier dropped (`zh_TW.UTF-8@x` → `zh-tw`),
/// `_` as `-`, lower case, empty parts left out.
fn normalise(tag: &str) -> Vec<String> {
    let base = tag.split(['.', '@']).next().unwrap_or_default();
    base.split(['-', '_']).filter(|p| !p.is_empty()).map(str::to_ascii_lowercase).collect()
}

/// The language codes to try for `tag`, most specific first: `zh_TW` → `zh-tw`, `zh-hant`, `zh`;
/// `de-AT` → `de-at`, `de`.
pub fn candidates(tag: &str) -> Vec<String> {
    let parts = normalise(tag);
    let Some(primary) = parts.first() else { return Vec::new() };
    let mut out: Vec<String> = (1..=parts.len()).rev().filter_map(|n| parts.get(..n)).map(|p| p.join("-")).collect();
    if primary == "zh" && !parts.iter().any(|p| p == "hans" || p == "hant") {
        // Chinese by region when no script is given: Taiwan, Hong Kong and Macao write Traditional.
        let script = if parts.iter().any(|p| matches!(p.as_str(), "tw" | "hk" | "mo")) { "zh-hant" } else { "zh-hans" };
        out.insert(out.len().saturating_sub(1), script.to_string());
    }
    out
}

/// The language of `registry` closest to `tag`: an exact code, then a shorter form of the tag
/// (`de-AT` → `de`), then the same language in another region (`pt-PT` → `pt-BR`). Traditional
/// and Simplified Chinese stay apart: `zh-TW` never finds `zh-Hans`, nor `zh-CN` `zh-Hant`.
/// `C` and `POSIX` are English. `None` when nothing fits.
pub fn resolve<'a>(tag: &str, registry: &[&'a Language]) -> Option<&'a Language> {
    let cands = candidates(tag);
    let first = cands.first()?;
    if first == "c" || first == "posix" {
        return registry.iter().copied().find(|l| l.code == ENGLISH.code);
    }
    let by_code = |c: &str| registry.iter().copied().find(|l| l.code.eq_ignore_ascii_case(c));
    if let Some(l) = cands.iter().find_map(|c| by_code(c)) {
        return Some(l);
    }
    let primary = cands.last()?;
    if primary == "zh" {
        // A Chinese tag found neither its script nor plain `zh`: never the other script.
        return None;
    }
    registry.iter().copied().find(|l| l.code.split('-').next().is_some_and(|p| p.eq_ignore_ascii_case(primary)))
}

/// Every language a setting may name: the registry, then the pseudo-locale.
pub fn all() -> impl Iterator<Item = &'static Language> {
    LANGUAGES.iter().chain(std::iter::once(&PSEUDO))
}

/// The language whose code is `code` (any case), the pseudo-locale included.
pub fn by_code(code: &str) -> Option<&'static Language> {
    all().find(|l| l.code.eq_ignore_ascii_case(code))
}
