//! The TSV catalog format (`localization.md` → Catalog format): `context<TAB>source<TAB>translation`
//! per line, `#` comments and blank lines ignored, escapes `\n`, `\t` and `\\`.
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/i18n/catalog.rs` (revision d95fdf31; MIT OR
//! Apache-2.0). Ours checks more on reading: a line that is malformed (not three columns, an empty
//! column, a broken escape, a placeholder in one column only, a plural with the wrong number of
//! forms, a key given twice, longer than [`MAX_LINE_BYTES`]) is left out and named in
//! [`Catalog::problems`], so its text shows in English; nothing here panics on any input.

use std::collections::{BTreeSet, HashMap};

use super::plural::Plural;

/// The longest catalog line read, in bytes; a longer one is a problem and left out.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// The most problems kept (the count is still exact).
const MAX_PROBLEMS: usize = 100;

/// The context of a command-id entry.
pub const ID: &str = "@id";
/// The context of a plural entry.
pub const PLURAL: &str = "@plural";

/// One entry as written: its context, English source and translation, unescaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The 1-based line it was read from.
    pub line: usize,
    pub context: String,
    pub source: String,
    pub translation: String,
}

/// A parsed catalog.
#[derive(Debug, Default)]
pub struct Catalog {
    /// Context-free strings: English → translation (the hot path).
    plain: HashMap<String, String>,
    /// Strings with a disambiguating context, by `(context, source)`.
    contextual: HashMap<(String, String), String>,
    /// Strings keyed by an id (`@id`): a command id, or an option's `option:<key>`.
    ids: HashMap<String, String>,
    /// Plural messages: `one|other` → the language's forms.
    plurals: HashMap<String, Vec<String>>,
    problems: Vec<String>,
    problem_count: usize,
}

/// `s` unescaped (`\n`, `\t`, `\\`), or `None` for any other backslash sequence.
pub fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '\\' => out.push('\\'),
            _ => return None,
        }
    }
    Some(out)
}

/// The `{name}` placeholders of `s` (names of ASCII letters, digits and `_`).
pub fn placeholders(s: &str) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let after = rest.get(open + 1..).unwrap_or_default();
        match after.find('}') {
            Some(close) => {
                let name = after.get(..close).unwrap_or_default();
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    out.insert(name);
                }
                rest = after.get(close + 1..).unwrap_or_default();
            }
            None => break,
        }
    }
    out
}

/// Reads every entry of `text`, checking each line on its own; returns the entries and the
/// problems (`line N: …`). `plural` is the language's rule, which fixes a plural's form count.
pub fn entries(text: &str, plural: Plural) -> (Vec<Entry>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for (i, line) in text.lines().enumerate() {
        let n = i.saturating_add(1);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match entry(line, plural) {
            Ok((context, source, translation)) => {
                if !seen.insert((context.clone(), source.clone())) {
                    problems.push(format!("line {n}: the key is given twice; the first is used"));
                    continue;
                }
                out.push(Entry { line: n, context, source, translation });
            }
            Err(why) => problems.push(format!("line {n}: {why}")),
        }
    }
    (out, problems)
}

fn entry(line: &str, plural: Plural) -> Result<(String, String, String), String> {
    if line.len() > MAX_LINE_BYTES {
        return Err(format!("longer than {MAX_LINE_BYTES} bytes"));
    }
    let mut cols = line.split('\t');
    let (Some(ctx), Some(src), Some(tr), None) = (cols.next(), cols.next(), cols.next(), cols.next()) else {
        return Err("expected `context<TAB>source<TAB>translation`".into());
    };
    let bad_escape = || "a backslash that is not \\n, \\t or \\\\".to_string();
    let (ctx, src, tr) = (unescape(ctx).ok_or_else(bad_escape)?, unescape(src).ok_or_else(bad_escape)?, unescape(tr).ok_or_else(bad_escape)?);
    if src.trim().is_empty() {
        return Err("the source is empty".into());
    }
    if tr.trim().is_empty() {
        return Err("the translation is empty".into());
    }
    if ctx == PLURAL {
        let forms: Vec<&str> = src.split('|').collect();
        if forms.len() != 2 || forms.iter().any(|f| f.trim().is_empty()) {
            return Err("a plural's source must be `one|other`".into());
        }
        let translated: Vec<&str> = tr.split('|').collect();
        if translated.len() != plural.forms() || translated.iter().any(|f| f.trim().is_empty()) {
            return Err(format!("a plural of this language lists {} forms ({})", plural.forms(), plural.categories().join("|")));
        }
        let wanted: BTreeSet<&str> = forms.iter().flat_map(|f| placeholders(f)).collect();
        let given: BTreeSet<&str> = translated.iter().flat_map(|f| placeholders(f)).collect();
        if wanted != given {
            return Err(placeholder_problem(&wanted, &given));
        }
    } else if ctx != ID {
        let (wanted, given) = (placeholders(&src), placeholders(&tr));
        if wanted != given {
            return Err(placeholder_problem(&wanted, &given));
        }
    }
    Ok((ctx, src, tr))
}

fn placeholder_problem(wanted: &BTreeSet<&str>, given: &BTreeSet<&str>) -> String {
    let names = |s: &BTreeSet<&str>| s.iter().map(|n| format!("{{{n}}}")).collect::<Vec<_>>().join(" ");
    let missing: BTreeSet<&str> = wanted.difference(given).copied().collect();
    let extra: BTreeSet<&str> = given.difference(wanted).copied().collect();
    match (missing.is_empty(), extra.is_empty()) {
        (false, true) => format!("the translation lacks {}", names(&missing)),
        (true, false) => format!("the translation adds {}", names(&extra)),
        _ => format!("the translation lacks {} and adds {}", names(&missing), names(&extra)),
    }
}

impl Catalog {
    /// The catalog of `text` for a language with `plural`; malformed lines are left out and
    /// named in [`Catalog::problems`].
    pub fn parse(text: &str, plural: Plural) -> Catalog {
        let (list, problems) = entries(text, plural);
        let mut c = Catalog { problem_count: problems.len(), problems, ..Catalog::default() };
        c.problems.truncate(MAX_PROBLEMS);
        for e in list {
            match e.context.as_str() {
                "" => {
                    c.plain.insert(e.source, e.translation);
                }
                ID => {
                    c.ids.insert(e.source, e.translation);
                }
                PLURAL => {
                    c.plurals.insert(e.source, e.translation.split('|').map(str::to_string).collect());
                }
                _ => {
                    c.contextual.insert((e.context, e.source), e.translation);
                }
            }
        }
        c
    }

    /// The lines left out, `line N: why` (at most the first hundred).
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// How many lines were left out.
    pub fn problem_count(&self) -> usize {
        self.problem_count
    }

    /// How many entries it holds.
    pub fn len(&self) -> usize {
        self.plain.len() + self.contextual.len() + self.ids.len() + self.plurals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The translation of a plain string.
    pub fn plain(&self, source: &str) -> Option<&str> {
        self.plain.get(source).map(String::as_str)
    }

    /// The translation of `source` in `context`.
    pub fn contextual(&self, context: &str, source: &str) -> Option<&str> {
        self.contextual.get(&(context.to_string(), source.to_string())).map(String::as_str)
    }

    /// The translation keyed by `id`.
    pub fn id(&self, id: &str) -> Option<&str> {
        self.ids.get(id).map(String::as_str)
    }

    /// Form `index` of the plural whose English forms are `one` and `other`.
    pub fn plural(&self, one: &str, other: &str, index: usize) -> Option<&str> {
        let forms = self.plurals.get(&format!("{one}|{other}"))?;
        forms.get(index.min(forms.len().saturating_sub(1))).map(String::as_str)
    }

    /// `source`, translated: the plain entry, else the English.
    pub fn tr<'a>(&'a self, source: &'a str) -> &'a str {
        self.plain(source).unwrap_or(source)
    }

    /// `source` in `context`: the context's entry, else the plain one, else the English.
    pub fn tr_ctx<'a>(&'a self, context: &str, source: &'a str) -> &'a str {
        self.contextual(context, source).unwrap_or_else(|| self.tr(source))
    }

    /// A text keyed by `id` whose English is `english`: the id's entry wins, else the English
    /// text's, else the English.
    pub fn tr_id<'a>(&'a self, id: &str, english: &'a str) -> &'a str {
        self.id(id).unwrap_or_else(|| self.tr(english))
    }

    /// The form of a plural for count `n` under `rule`, else the English form (`one` for 1).
    pub fn tr_plural<'a>(&'a self, rule: Plural, n: u64, one: &'a str, other: &'a str) -> &'a str {
        self.plural(one, other, rule.index(n)).unwrap_or(if n == 1 { one } else { other })
    }
}
