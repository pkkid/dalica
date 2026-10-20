//! Line breaks for Thai, which writes no spaces between words (`docs/thai-text.md`, M15.47).
//!
//! egui's wrapping breaks a row at whitespace, after or before a CJK character, at a dash or
//! punctuation, and only then anywhere; it does not break at U+200B ZERO WIDTH SPACE (not
//! whitespace to Rust's `char::is_whitespace`). So [`mark`] puts a [`MARK`] (U+2009 THIN SPACE,
//! whitespace, so a break candidate) at each word boundary between two Thai letters, from ICU4X's
//! Thai dictionary, and the fonts draw it with no width ([`crate::fonts`] sets every face's
//! `thin_space_width` to 0). Text without Thai is returned as it is, without a copy.
//!
//! Mark text that is only shown: catalog text, and captions, tags and folder names where a view
//! wraps them. Never mark the value of a text field, or text that is saved, compared or sent;
//! where shown text becomes such a value (a translated default name), [`unmark`] it first.

use std::borrow::Cow;
use std::sync::LazyLock;

use icu_segmenter::LineSegmenter;
use icu_segmenter::LineSegmenterBorrowed;
use icu_segmenter::options::LineBreakOptions;

/// The break mark: U+2009 THIN SPACE, drawn with no width by Dalica's fonts.
pub const MARK: char = '\u{2009}';

/// ICU4X's line segmenter with the baked Thai (and Lao, Khmer, Burmese) dictionaries, made once.
static SEGMENTER: LazyLock<LineSegmenterBorrowed<'static>> = LazyLock::new(|| LineSegmenter::new_dictionary(LineBreakOptions::default()));

/// Whether `c` is in the Thai block (letters, vowels, tone marks, digits and signs).
pub fn is_thai(c: char) -> bool {
    ('\u{0E01}'..='\u{0E5B}').contains(&c)
}

/// Thai characters a line never starts with: ฯ, the following vowels, the marks above and below,
/// ๅ and ๆ.
fn never_starts_a_line(c: char) -> bool {
    matches!(c, '\u{0E2F}' | '\u{0E30}'..='\u{0E3A}' | '\u{0E45}'..='\u{0E4E}')
}

/// Thai characters a line never ends with: the leading vowels เ แ โ ใ ไ, written before their
/// consonant.
fn never_ends_a_line(c: char) -> bool {
    ('\u{0E40}'..='\u{0E44}').contains(&c)
}

/// Whether `text` has a Thai character.
pub fn has_thai(text: &str) -> bool {
    text.chars().any(is_thai)
}

/// `text` with a [`MARK`] at every dictionary word boundary between two Thai letters, where a
/// wrapped row may then break. Borrowed (unchanged) when `text` has no Thai or no such boundary;
/// marking marked text changes nothing.
pub fn mark(text: &str) -> Cow<'_, str> {
    if !has_thai(text) {
        return Cow::Borrowed(text);
    }
    let mut out: Option<String> = None;
    let mut copied = 0;
    for at in SEGMENTER.segment_str(text) {
        let (Some(before), Some(after)) = (text.get(..at).and_then(|s| s.chars().next_back()), text.get(at..).and_then(|s| s.chars().next())) else {
            continue;
        };
        if !is_thai(before) || !is_thai(after) || never_ends_a_line(before) || never_starts_a_line(after) {
            continue;
        }
        let buf = out.get_or_insert_with(|| String::with_capacity(text.len() + text.len() / 8));
        buf.push_str(text.get(copied..at).unwrap_or_default());
        buf.push(MARK);
        copied = at;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(text.get(copied..).unwrap_or_default());
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(text),
    }
}

/// `text` without its [`MARK`]s: the text [`mark`] was given. Borrowed (unchanged) when it has
/// none. For a marked text (catalog text) that becomes a value the user keeps, such as a default
/// name.
pub fn unmark(text: &str) -> Cow<'_, str> {
    if text.contains(MARK) { Cow::Owned(text.chars().filter(|&c| c != MARK).collect()) } else { Cow::Borrowed(text) }
}
