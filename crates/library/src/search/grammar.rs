//! The search grammar (`organizing.md` §8.1): the text of the search box read into words.
//!
//! Adapted from LightCraft `crates/catalog/src/query.rs` @ b7091805 (MIT OR Apache-2.0): the text
//! read as whitespace-separated words that must all match, each word lowercased and split once at
//! its first `:` into a fielded term (`field:value`) when the field is one the grammar knows.
//! Dalica's own: `-word` for NOT, unknown fields searched as plain words, prefix tokens split at
//! punctuation with date spellings kept whole, and the caps on hostile input.
//!
//! - Words are separated by whitespace, control characters and bidirectional formatting
//!   characters. Several words must all match (AND).
//! - A word starting with `-` excludes what it matches (NOT).
//! - `tag:value` and `camera:value` look at the tags or the camera make and model only.
//!   `person:value` and `color:value` are read but match nothing yet (people arrive in M13, colour
//!   search later); [`Parsed::unavailable`] names them so the caller can say so. Any other
//!   `name:value` is searched as plain words (`foo:bar` is `foo bar`).
//! - A value is split into tokens at punctuation (dot, underscore, dash and the rest of ASCII
//!   punctuation); every token must be the start of one of the photo's terms, ignoring case. A
//!   date spelling (`2026`, `2026-06`, `2026-06-04`, or one being typed, `2026-0`) stays one token.
//! - Caps: the text is read up to [`MAX_QUERY`] bytes and [`MAX_WORDS`] words, and each token up to
//!   [`MAX_TERM`] bytes; [`Parsed::truncated`] says when something was left out.

use serde::Serialize;

/// The most bytes of search text read; the rest is left out (and [`Parsed::truncated`] set).
pub const MAX_QUERY: usize = 1024;
/// The most words read from the search text.
pub const MAX_WORDS: usize = 32;
/// The most bytes of one term or token (cut at a character boundary): longer words still match by
/// their first [`MAX_TERM`] bytes.
pub const MAX_TERM: usize = 64;

/// Which of a photo's terms a word looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    /// Every searchable field: file name, caption, tags, folder name, description and place, album
    /// names, dates, camera, pixel size.
    Any,
    /// `tag:` — the tags only.
    Tag,
    /// `camera:` — the camera make and model only.
    Camera,
    /// `person:` — read, matching nothing until people exist (M13).
    Person,
    /// `color:` — read, matching nothing until colour search exists.
    Color,
}

impl Field {
    /// Whether words of this field can match anything yet.
    pub fn is_available(self) -> bool {
        matches!(self, Field::Any | Field::Tag | Field::Camera)
    }

    /// The operator's name (`tag`, `camera`, `person`, `color`; `""` for plain words).
    pub fn name(self) -> &'static str {
        match self {
            Field::Any => "",
            Field::Tag => "tag",
            Field::Camera => "camera",
            Field::Person => "person",
            Field::Color => "color",
        }
    }

    /// The field of an operator name (ASCII, any case), if the grammar knows it.
    fn from_name(name: &str) -> Option<Field> {
        [Field::Tag, Field::Camera, Field::Person, Field::Color].into_iter().find(|f| f.name().eq_ignore_ascii_case(name))
    }
}

/// One word of the query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Atom {
    /// `-word`: photos it matches are left out.
    pub negated: bool,
    pub field: Field,
    /// Lowercase prefixes; each must start one of the photo's terms in `field`. Never empty.
    pub tokens: Vec<String>,
}

/// A query's text as the grammar reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    /// The words, in the order typed. None: the text matches everything.
    pub atoms: Vec<Atom>,
    /// Operators used that match nothing yet (`person`, `color`), each once, in the order used.
    pub unavailable: Vec<&'static str>,
    /// Whether the text was cut at [`MAX_QUERY`] bytes or [`MAX_WORDS`] words.
    pub truncated: bool,
}

impl Parsed {
    /// Reads `text` (any text at all: the caps bound the work).
    pub fn new(text: &str) -> Parsed {
        let mut out = Parsed::default();
        let read = cut(text, MAX_QUERY);
        out.truncated = read.len() < text.len();
        for word in read.split(is_word_separator).filter(|w| !w.is_empty()) {
            if out.atoms.len() >= MAX_WORDS {
                out.truncated = true;
                break;
            }
            let (negated, body) = match word.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, word),
            };
            let (field, value) = match body.split_once(':') {
                Some((name, value)) => match Field::from_name(name) {
                    Some(field) => (field, value),
                    None => (Field::Any, body),
                },
                None => (Field::Any, body),
            };
            if !field.is_available() && !out.unavailable.contains(&field.name()) {
                out.unavailable.push(field.name());
            }
            let tokens = query_tokens(value);
            if !tokens.is_empty() {
                out.atoms.push(Atom { negated, field, tokens });
            }
        }
        out
    }

    /// Whether the text constrains nothing (it matches every photo).
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }
}

/// Whether `c` separates words: whitespace, control characters and bidirectional formatting
/// characters (so a hostile text cannot hide a word's edges).
pub(crate) fn is_word_separator(c: char) -> bool {
    c.is_whitespace() || c.is_control() || is_bidi_format(c) || c == '\u{feff}' || c == '\u{200b}'
}

/// The Unicode bidirectional formatting characters (marks, embeddings, overrides, isolates).
fn is_bidi_format(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Whether `c` separates tokens inside a word: a word separator or ASCII punctuation (dot,
/// underscore and dash among them).
fn is_token_separator(c: char) -> bool {
    is_word_separator(c) || c.is_ascii_punctuation()
}

/// `text` cut to at most `max` bytes at a character boundary.
pub(crate) fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// A token as stored and compared: lowercase, at most [`MAX_TERM`] bytes.
fn normalise(token: &str) -> String {
    let lower = token.to_lowercase();
    cut(&lower, MAX_TERM).to_string()
}

/// The terms of a field's text: its tokens, lowercase, each at most [`MAX_TERM`] bytes. The index
/// and the matcher both read text through this.
pub fn terms(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(is_token_separator).filter(|t| !t.is_empty()).map(normalise)
}

/// Whether `value` is a date spelling, whole or being typed: four digits, then optionally `-` and
/// up to two digits, then optionally `-` and up to two digits.
fn is_date_spelling(value: &str) -> bool {
    let mut parts = value.split('-');
    let year = parts.next().unwrap_or_default();
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    rest.len() <= 2
        && rest.iter().all(|p| p.len() <= 2 && p.bytes().all(|b| b.is_ascii_digit()))
        && (rest.len() < 2 || rest.first().is_some_and(|p| !p.is_empty()))
}

/// The tokens of one word's value.
fn query_tokens(value: &str) -> Vec<String> {
    if is_date_spelling(value) {
        return vec![normalise(value)];
    }
    let mut out: Vec<String> = Vec::new();
    for t in terms(value) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The English month names, for the date terms (`organizing.md` §8.1). Localised names come with
/// localisation (M15).
pub(crate) const MONTHS: [&str; 12] =
    ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_spellings() {
        for ok in ["2026", "2026-", "2026-0", "2026-06", "2026-06-", "2026-06-0", "2026-06-04"] {
            assert!(is_date_spelling(ok), "{ok}");
        }
        for bad in ["202", "20261", "2026-123", "2026--04", "2026-06-04-", "abcd", "2026-06-123", "２０２６"] {
            assert!(!is_date_spelling(bad), "{bad}");
        }
    }

    #[test]
    fn cut_keeps_char_boundaries() {
        assert_eq!(cut("ééé", 3), "é");
        assert_eq!(cut("abc", 10), "abc");
        assert_eq!(cut("é", 1), "");
    }
}
