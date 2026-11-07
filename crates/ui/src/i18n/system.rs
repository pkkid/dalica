//! The system's preferred UI languages, read once, with no subprocess: `GetUserPreferredUILanguages`
//! on Windows, `CFLocaleCopyPreferredLanguages` on macOS, `LANGUAGE` / `LC_ALL` / `LC_MESSAGES` /
//! `LANG` elsewhere, through the pure-Rust `sys-locale` crate.
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/i18n/system.rs` (revision d95fdf31; MIT OR
//! Apache-2.0). What the system reports is input: at most [`MAX_TAGS`] tags of at most
//! [`MAX_TAG_BYTES`] bytes each, of the characters a locale tag uses, are kept.

use std::sync::OnceLock;

/// The most tags kept.
pub const MAX_TAGS: usize = 64;
/// The longest tag kept, in bytes.
pub const MAX_TAG_BYTES: usize = 128;

/// Stands in for the system's preferred languages, comma-separated (`en`, `fr-FR,en`): the tests
/// and the gate set it so System draws the same language on every machine, and a chosen language
/// still wins over it, unlike `DALICA_LANGUAGE`.
pub const ENV: &str = "DALICA_SYSTEM_LANGUAGES";

static TAGS: OnceLock<Vec<String>> = OnceLock::new();

/// Sets the system's preferred languages for this process before anything reads them, so a
/// test draws the same language on every machine; `false` once they were read or set already.
pub fn set(tags: &[&str]) -> bool {
    TAGS.set(bounded(tags.iter().map(|t| (*t).to_string()))).is_ok()
}

/// The system's preferred languages, most preferred first (read on the first call), or those
/// [`ENV`] names, or those [`set`] gave.
pub fn tags() -> &'static [String] {
    TAGS.get_or_init(|| {
        // The crate's own tests read English texts: System is English under them on every machine.
        if cfg!(test) {
            return vec!["en".to_string()];
        }
        if let Ok(list) = std::env::var(ENV) {
            return bounded(list.split(',').map(str::to_string));
        }
        // A failure in the platform's interop leaves the interface in English, never down.
        std::panic::catch_unwind(|| bounded(sys_locale::get_locales())).unwrap_or_default()
    })
}

/// `tags` trimmed, the empty and malformed ones dropped, at most [`MAX_TAGS`].
pub fn bounded(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    tags.into_iter()
        .take(MAX_TAGS)
        .map(|t| t.trim().to_string())
        .filter(|t| {
            !t.is_empty() && t.len() <= MAX_TAG_BYTES && t.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'@'))
        })
        .collect()
}
