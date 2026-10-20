//! Localisation (`localization.md`): English source strings are the keys, a TSV catalog per
//! language supplies the translations, and a missing entry falls back to English.
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/i18n/mod.rs` (revision d95fdf31; MIT OR
//! Apache-2.0), in our own shape:
//!
//! - [`tl!`](crate::tl) wraps a literal: `tl!("Folder Manager")`, `tl!(ctx = "verb", "Open")`,
//!   with named placeholders `tl!("{count} photos selected", count = n)`; [`tln!`](crate::tln)
//!   picks a plural form (`tln!(n, "{n} photo", "{n} photos")`, `{n}` written with
//!   [`format::number`]). Both give a `String`.
//! - [`command_label`] looks a command's label up by its id, then by its English text;
//!   [`option_label`] and [`option_hint`] look an Options row's texts up by its settings key
//!   (`@id option:<key>`, `@id option:<key>/hint`), then by the English text.
//! - [`release_note`] looks one line of the release notes up.
//! - [`sync`] sets the language the next draw uses from `interface.language` (`system`: the
//!   system's preferred languages, [`system::tags`]), `DALICA_LANGUAGE` overriding it for one
//!   launch. The language is per thread: the window draws on one, and each headless driver of a
//!   test on its own.
//! - The pseudo-locale `qps` ([`pseudo`]) translates every key at run time.
//!
//! Command ids, widget ids, control and MCP names and log messages are never translated.

pub mod catalog;
pub mod format;
pub mod languages;
pub mod plural;
pub mod pseudo;
pub mod system;

use std::cell::Cell;
use std::fmt::Display;
use std::sync::OnceLock;

pub use catalog::Catalog;
pub use languages::{ENGLISH, LANGUAGES, Language, PSEUDO, PSEUDO_CODE};
pub use plural::Plural;

/// The settings value that follows the system.
pub const SYSTEM: &str = "system";
/// The environment variable that sets the language for one launch.
pub const ENV: &str = "DALICA_LANGUAGE";

thread_local! {
    static CURRENT: Cell<&'static Language> = const { Cell::new(&ENGLISH) };
}

/// The language this thread draws in.
pub fn current() -> &'static Language {
    CURRENT.get()
}

/// Draw in `lang` on this thread from now on.
pub fn set_current(lang: &'static Language) {
    CURRENT.set(lang);
}

/// Draws `f` in `lang`, then goes back to the language before (even if `f` unwinds).
pub fn with_language<R>(lang: &'static Language, f: impl FnOnce() -> R) -> R {
    struct Restore(&'static Language);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_current(self.0);
        }
    }
    let _restore = Restore(current());
    set_current(lang);
    f()
}

/// Whether this thread draws in the pseudo-locale.
pub fn is_pseudo() -> bool {
    current().code == PSEUDO_CODE
}

/// The parsed catalog of `lang` (parsed on first use; its problems logged once, and those lines
/// shown in English).
pub fn catalog(lang: &'static Language) -> &'static Catalog {
    static CATALOGS: OnceLock<Vec<Catalog>> = OnceLock::new();
    static EMPTY: OnceLock<Catalog> = OnceLock::new();
    let all = CATALOGS.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|l| {
                let c = Catalog::parse(l.catalog, l.plural);
                if c.problem_count() > 0 {
                    log::warn!(
                        "translation catalog {}: {} line(s) left out, shown in English: {}",
                        l.code,
                        c.problem_count(),
                        c.problems().iter().take(5).cloned().collect::<Vec<_>>().join("; ")
                    );
                }
                c
            })
            .collect()
    });
    LANGUAGES.iter().position(|l| std::ptr::eq(l, lang)).and_then(|i| all.get(i)).unwrap_or_else(|| EMPTY.get_or_init(Catalog::default))
}

/// `english` in the language in effect.
pub fn tr(english: &str) -> String {
    let lang = current();
    if lang.code == PSEUDO_CODE {
        return pseudo::translate(english);
    }
    catalog(lang).tr(english).to_string()
}

/// A translated `text` as a value the user keeps (a default name offered in a field, saved if
/// left as it is): without the line-break marks catalog text carries for wrapping
/// ([`dalica_ui_kit::linebreak::unmark`]; Thai, `docs/thai-text.md`).
pub fn user_text(text: String) -> String {
    match dalica_ui_kit::linebreak::unmark(&text) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(plain) => plain,
    }
}

/// `english` in `context` (a word that reads differently by use).
pub fn tr_ctx(context: &str, english: &str) -> String {
    let lang = current();
    if lang.code == PSEUDO_CODE {
        return pseudo::translate(english);
    }
    catalog(lang).tr_ctx(context, english).to_string()
}

/// A text keyed by `id` (`@id`) whose English is `english`.
pub fn tr_id(id: &str, english: &str) -> String {
    let lang = current();
    if lang.code == PSEUDO_CODE {
        return pseudo::translate(english);
    }
    catalog(lang).tr_id(id, english).to_string()
}

/// The plural form for count `n` of the message whose English forms are `one` and `other`.
pub fn tr_plural(n: u64, one: &str, other: &str) -> String {
    let lang = current();
    if lang.code == PSEUDO_CODE {
        return pseudo::translate(if n == 1 { one } else { other });
    }
    catalog(lang).tr_plural(lang.plural, n, one, other).to_string()
}

/// The label of command `id`, whose English label is `english` (menus, the shortcut editor).
pub fn command_label(id: &str, english: &str) -> String {
    tr_id(id, english)
}

/// The label of the Options row stored under `key`.
pub fn option_label(key: &str, english: &str) -> String {
    tr_id(&format!("option:{key}"), english)
}

/// The hint under the Options row stored under `key`.
pub fn option_hint(key: &str, english: &str) -> String {
    tr_id(&format!("option:{key}/hint"), english)
}

/// One line of the release notes (a heading, or a bullet without its `- `); an edited line
/// shows in English until its catalog entry is updated.
pub fn release_note(line: &str) -> String {
    tr(line)
}

/// `template` with each `{name}` of `args` filled in, in one pass (a value's own braces are never
/// filled); a placeholder not in `args` stays as written.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        let filled = after.find('}').and_then(|close| {
            let name = after.get(..close)?;
            let (_, v) = args.iter().find(|(k, _)| *k == name)?;
            Some((v.to_string(), close))
        });
        match filled {
            Some((value, close)) => {
                out.push_str(&value);
                rest = after.get(close + 1..).unwrap_or_default();
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The languages Options ▸ General ▸ Language lists after System, `(code, endonym)`: the registry
/// and the pseudo-locale (which the dialog shows only while it is chosen). The engine is given
/// these at start (`Session::set_languages`), so this registry is the one list.
pub fn choices() -> Vec<(&'static str, &'static str)> {
    languages::all().map(|l| (l.code, l.endonym)).collect()
}

/// The language for the setting `setting` (`system` or a tag) with the system preferring
/// `system_tags`: a tag the registry knows (or its closest language), else the first system
/// language it knows, else English. `qps` is the pseudo-locale.
pub fn resolve_setting(setting: &str, system_tags: &[String]) -> &'static Language {
    let registry: Vec<&'static Language> = LANGUAGES.iter().collect();
    if !setting.eq_ignore_ascii_case(SYSTEM) {
        if setting.eq_ignore_ascii_case(PSEUDO_CODE) {
            return &PSEUDO;
        }
        if let Some(l) = languages::resolve(setting, &registry) {
            return l;
        }
    }
    system_tags.iter().find_map(|t| languages::resolve(t, &registry)).unwrap_or(&ENGLISH)
}

/// `DALICA_LANGUAGE`, read once: the language it names, if it names one.
fn env_override() -> Option<&'static Language> {
    static ENV_LANG: OnceLock<Option<&'static Language>> = OnceLock::new();
    *ENV_LANG.get_or_init(|| {
        let tag = std::env::var(ENV).ok()?;
        let tag = tag.trim();
        if tag.is_empty() || tag.len() > system::MAX_TAG_BYTES {
            return None;
        }
        let lang = resolve_setting(tag, &[]);
        log::info!("{ENV}={tag}: the interface is drawn in {}", lang.code);
        Some(lang)
    })
}

/// The language in effect for the setting `setting` (`None`: no library open, so `system`).
pub fn in_effect(setting: Option<&str>) -> &'static Language {
    env_override().unwrap_or_else(|| resolve_setting(setting.unwrap_or(SYSTEM), system::tags()))
}

/// Sets this thread's language from the setting, for the frame about to be drawn
/// ([`crate::App::frame`] calls it first), and orders `ctx`'s fallback faces for its script
/// (`ui-kit`'s [`fonts::set_script`](dalica_ui_kit::fonts::set_script), which does nothing while
/// the script stays the same); returns it.
pub fn sync(ctx: &egui::Context, setting: Option<&str>) -> &'static Language {
    let lang = in_effect(setting);
    set_current(lang);
    if dalica_ui_kit::fonts::set_script(ctx, lang.script) {
        ctx.request_repaint();
    }
    // `ui-kit`'s own texts (`ui_kit::text`) in this language, set when the language changes.
    let key = egui::Id::new("dalica.i18n.kitTexts");
    if ctx.data(|d| d.get_temp::<&'static str>(key)) != Some(lang.code) {
        use dalica_ui_kit::text::{self, Texts};
        text::set(ctx, Texts::translated(|t| Some(tr_ctx(t.context(), t.english()))));
        ctx.data_mut(|d| d.insert_temp(key, lang.code));
    }
    lang
}

/// A literal through the catalog of the language in effect: `tl!("Folder Manager")`,
/// `tl!(ctx = "verb", "Open")`, `tl!("{count} photos selected", count = n)`. Gives a `String`.
#[macro_export]
macro_rules! tl {
    (ctx = $ctx:expr, $s:literal $(,)?) => {
        $crate::i18n::tr_ctx($ctx, $s)
    };
    (ctx = $ctx:expr, $s:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::tr_ctx($ctx, $s), &[$((stringify!($k), &$v as &dyn ::std::fmt::Display)),+])
    };
    ($s:literal $(,)?) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::tr($s), &[$((stringify!($k), &$v as &dyn ::std::fmt::Display)),+])
    };
}

/// A plural message for count `n` (any integer; a negative count reads as the largest):
/// `tln!(n, "{n} photo", "{n} photos")`, more placeholders after (`…, album = name`). `{n}` is
/// the count written with [`format::number`]. Gives a `String`.
#[macro_export]
macro_rules! tln {
    ($n:expr, $one:literal, $other:literal $(, $k:ident = $v:expr)* $(,)?) => {{
        let n: u64 = ::std::convert::TryFrom::try_from($n).unwrap_or(u64::MAX);
        $crate::i18n::fill(
            &$crate::i18n::tr_plural(n, $one, $other),
            &[("n", &$crate::i18n::format::number(n) as &dyn ::std::fmt::Display) $(, (stringify!($k), &$v as &dyn ::std::fmt::Display))*],
        )
    }};
}
