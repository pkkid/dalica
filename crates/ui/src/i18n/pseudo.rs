//! The pseudo-locale `qps` (`localization.md` → Checking a translation): every translation is
//! the English text in `⁅…⁆`, 40 % longer, built at run time from whatever key is looked up, so
//! it covers the whole key set with no catalog. A text drawn without the brackets did not go
//! through a catalog: the headless driver lists what a frame drew ([`crate::headless::Driver::drawn_texts`])
//! and [`untranslated`] names the texts that are not bracketed.
//!
//! Placeholders stay as written (their values fill in after), so a sentence with values is still
//! one bracketed text: `{count} photos` → `⁅3 photos····⁆`.

/// The opening and closing marks: brackets with quill (U+2045, U+2046), which the interface's
/// font draws and ordinary text never holds (`⟦…⟧`, first planned, have no glyph in Inter).
pub const OPEN: char = '⁅';
pub const CLOSE: char = '⁆';
/// The padding character.
pub const PAD: char = '·';

/// `english` as the pseudo-locale shows it: bracketed, and padded by 40 % of its length in
/// characters (rounded up).
pub fn translate(english: &str) -> String {
    let chars = english.chars().count();
    let pad = chars.saturating_mul(2).div_ceil(5);
    let mut out = String::with_capacity(english.len().saturating_add(pad.saturating_mul(PAD.len_utf8())).saturating_add(6));
    out.push(OPEN);
    out.push_str(english);
    out.extend(std::iter::repeat_n(PAD, pad));
    out.push(CLOSE);
    out
}

/// Whether `text` holds a pseudo-translated part.
pub fn is_marked(text: &str) -> bool {
    text.contains(OPEN)
}

/// The texts of `drawn` that hold a letter and no pseudo-translated part: under the
/// pseudo-locale, the untranslated ones (and user data, which is never translated).
pub fn untranslated<'a>(drawn: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    drawn.into_iter().filter(|t| !is_marked(t) && t.chars().any(char::is_alphabetic)).collect()
}
