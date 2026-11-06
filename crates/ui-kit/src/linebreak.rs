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

/// Runs of the Thai catalog that ICU4X's dictionary misreads, as their words (M16.24,
/// `docs/thai-text.md`). After ผล it reads ผลการค้นหา as ผลก|า|รค|้|นหา, and it splits loanwords
/// mid-syllable (แพ|ตช|์), so with the marks it cannot place (before a vowel or a mark) left out a
/// row broke inside a word. Where a run occurs, [`mark`] breaks at its words' edges and segments
/// only the text between runs with the dictionary. A loanword kept whole is a run of one word.
/// Found by segmenting every catalog text and listing those with a boundary no Thai line takes
/// (`never_starts_a_line`, `never_ends_a_line`).
const RUNS: &[&[&str]] = &[
    &["ผล", "การ"],
    &["ตรวจ", "หา", "รายการ"],
    &["เพื่อ", "นำ"],
    &["ล่าง", "สุด"],
    &["ตาม", "ที่"],
    &["ทำ", "ไม่", "ได้"],
    &["ดู", "และ"],
    &["นั้น", "และ"],
    &["ด้าน", "ใน"],
    &["ด้าน", "นอก"],
    &["สี", "สอง", "แถบ"],
    &["เมตา", "ของ", "รูปภาพ"],
    &["มาตรฐาน"],
    &["เอฟเฟกต์"],
    &["ฟิลเตอร์"],
    &["แพตช์"],
    &["ไดรฟ์"],
    &["ฮิสโตแกรม"],
    &["อินสแตนต์"],
];

/// Each of [`RUNS`] written out, beside its words, made once.
static RUN_TEXTS: LazyLock<Vec<(String, &'static [&'static str])>> = LazyLock::new(|| RUNS.iter().map(|run| (run.concat(), *run)).collect());

/// The words of [`RUNS`] found in `text`, as byte ranges, sorted.
fn run_words(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut words = Vec::new();
    for (whole, run) in RUN_TEXTS.iter() {
        for (start, _) in text.match_indices(whole.as_str()) {
            let mut edge = start;
            for word in run.iter() {
                words.push(edge..edge + word.len());
                edge += word.len();
            }
        }
    }
    words.sort_unstable_by_key(|w| (w.start, w.end));
    words
}

/// Every place `text` may break, in byte offsets, ascending: the edges of the [`RUNS`]' words,
/// and the dictionary's boundaries in each stretch between them (segmented on its own, so a run
/// does not sway its neighbours), never inside a run's word.
fn boundaries(text: &str) -> Vec<usize> {
    let words = run_words(text);
    if words.is_empty() {
        return SEGMENTER.segment_str(text).collect();
    }
    let mut edges: Vec<usize> = words.iter().flat_map(|w| [w.start, w.end]).collect();
    edges.sort_unstable();
    edges.dedup();
    let mut out = Vec::new();
    let mut from = 0;
    for edge in edges.iter().copied().chain(std::iter::once(text.len())) {
        if let Some(piece) = text.get(from..edge) {
            out.extend(SEGMENTER.segment_str(piece).map(|at| from + at));
        }
        out.push(edge);
        from = edge;
    }
    out.retain(|at| !words.iter().any(|w| w.start < *at && *at < w.end));
    out.sort_unstable();
    out.dedup();
    out
}

/// `text` with a [`MARK`] at every dictionary word boundary between two Thai letters, where a
/// wrapped row may then break; the runs the dictionary misreads ([`RUNS`]) break at their words.
/// Borrowed (unchanged) when `text` has no Thai or no such boundary; marking marked text changes
/// nothing.
pub fn mark(text: &str) -> Cow<'_, str> {
    if !has_thai(text) {
        return Cow::Borrowed(text);
    }
    let mut out: Option<String> = None;
    let mut copied = 0;
    for at in boundaries(text) {
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
