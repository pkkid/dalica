//! `cargo xtask i18n` (`localization.md` → Adding or maintaining a language): the key set the
//! catalogs translate, each registered language's coverage of it, and the catalogs' faults.
//!
//! Adapted from PhotoCraft `xtask/src/i18n_coverage.rs` (revision d95fdf31; MIT OR Apache-2.0),
//! read from the sources as it does, so it builds nothing:
//!
//! - **The key set:** every `tl!` literal of `crates/ui/src` outside test modules (with its
//!   `ctx = "…"`), every `tln!` plural (`@plural one|other`), every command label (`label: "…"` in
//!   `crates/engine/src/cmd` and `crates/ui/src/commands`, and the menu catalog's items and
//!   submenus), every Options row's group, label, hint and choice label
//!   (`crates/engine/src/options.rs`), and every text of `ui-kit`'s own table (`texts!` in
//!   `crates/ui-kit/src/text.rs`, each with its context).
//! - **The languages:** `LANGUAGES` in `crates/ui/src/i18n/languages.rs`, each with its plural
//!   rule, `complete_menus` and catalog (`include_str!("<code>.tsv")`).
//! - **Failures:** a malformed line (not three columns, a broken escape), an empty entry, a
//!   placeholder in one column only, a malformed plural (its source not `one|other`, or not the
//!   rule's number of forms), a duplicate key, and for a `complete_menus` language a key of the set
//!   it lacks. Missing keys of other languages and untranslated release-note lines are reported,
//!   never failed.
//! - **Drafts:** an entry whose line starts with `~` (before its context) is a machine-assisted
//!   draft awaiting the fluent review (`localization.md` → Draft entries); each language's draft
//!   count is printed, never failed.
//! - **`cargo xtask i18n --missing <code>`** prints the keys `<code>`'s catalog lacks as catalog
//!   lines with an empty translation, ready to fill in.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// One translatable text: its context (`""`, `@plural`, or a `ctx = "…"`) and English source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub context: String,
    pub source: String,
}

impl Key {
    fn new(context: &str, source: &str) -> Key {
        Key { context: context.to_string(), source: source.to_string() }
    }
}

/// A registered language, as `languages.rs` writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lang {
    pub code: String,
    /// The plural rule's name (`OneOther`, `Single`, …).
    pub plural: String,
    pub complete_menus: bool,
    /// The catalog's file name next to `languages.rs` (`None`: English, the source).
    pub catalog: Option<String>,
}

/// What `cargo xtask i18n` found.
#[derive(Debug, Default)]
pub struct Report {
    pub keys: BTreeSet<Key>,
    /// How many keys each source gave (before merging).
    pub origins: Vec<(&'static str, usize)>,
    /// Per language: code, keys translated, keys missing.
    pub coverage: Vec<(String, usize, usize)>,
    /// Per language: release-note lines translated, of how many.
    pub notes: Vec<(String, usize, usize)>,
    /// Per language: entries marked as drafts (`~`), awaiting the fluent review.
    pub drafts: Vec<(String, usize)>,
    /// Per language with a catalog: the keys it lacks.
    pub missing: BTreeMap<String, Vec<Key>>,
    pub failures: Vec<String>,
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    match args {
        [] => {}
        [flag, code] if flag == "--missing" => {
            let report = check(root)?;
            let missing = report.missing.get(code.as_str()).ok_or_else(|| format!("`{code}` is not a registered language with a catalog"))?;
            for k in missing {
                println!("{}\t{}\t", escape(&k.context), escape(&k.source));
            }
            return Ok(());
        }
        [a, ..] => return Err(format!("cargo xtask i18n takes no arguments but `--missing <code>` (got `{a}`)")),
    }
    let report = check(root)?;
    print!("{}", render(&report));
    if report.failures.is_empty() { Ok(()) } else { Err(format!("i18n: {} problem(s)", report.failures.len())) }
}

/// The plural forms a rule lists (`crates/ui/src/i18n/plural.rs`, `Plural::forms`).
fn forms(rule: &str) -> Option<usize> {
    match rule {
        "Single" => Some(1),
        "OneOther" | "ZeroOneOther" => Some(2),
        "EastSlavic" | "Polish" | "Czech" => Some(3),
        _ => None,
    }
}

/// Checks the tree at `root`.
pub fn check(root: &Path) -> Result<Report, String> {
    let ui = root.join("crates/ui/src");
    let mut report = Report::default();
    let add = |report: &mut Report, origin: &'static str, keys: BTreeSet<Key>| {
        report.origins.push((origin, keys.len()));
        report.keys.extend(keys);
    };
    let ui_sources = sources(&ui)?;
    add(&mut report, "tl! literals", ui_sources.iter().flat_map(|(_, s)| tl_keys(s)).collect());
    add(&mut report, "tln! plurals", ui_sources.iter().flat_map(|(_, s)| plural_keys(s)).collect());
    let mut labels = BTreeSet::new();
    for dir in ["crates/engine/src/cmd", "crates/ui/src/commands"] {
        for (_, s) in sources(&root.join(dir))? {
            labels.extend(label_keys(&s));
        }
    }
    labels.extend(menu_keys(&read(&ui.join("menu_catalog.rs"))?));
    add(&mut report, "command labels", labels);
    add(&mut report, "options rows", option_keys(&read(&root.join("crates/engine/src/options.rs"))?));
    add(&mut report, "ui-kit texts", kit_keys(&read(&root.join("crates/ui-kit/src/text.rs"))?));
    if report.keys.is_empty() {
        return Err("no translatable keys found in crates/ui, the commands or the options".into());
    }
    let notes = release_note_lines(&fs::read_to_string(root.join("docs/whats-new.md")).unwrap_or_default());
    let dir = ui.join("i18n");
    for lang in registry(&read(&dir.join("languages.rs"))?)? {
        let Some(file) = &lang.catalog else {
            report.coverage.push((lang.code.clone(), report.keys.len(), 0));
            continue;
        };
        let Some(n) = forms(&lang.plural) else {
            report.failures.push(format!("{}: unknown plural rule `{}`", lang.code, lang.plural));
            continue;
        };
        let path = dir.join(file);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (entries, problems) = parse(&text, n);
        report.failures.extend(problems.into_iter().map(|p| format!("{file}: {p}")));
        report.drafts.push((lang.code.clone(), entries.iter().filter(|e| e.draft).count()));
        let have: BTreeSet<Key> = entries.into_iter().map(|e| e.key).collect();
        let missing: Vec<&Key> = report.keys.iter().filter(|k| !have.contains(k)).collect();
        report.missing.insert(lang.code.clone(), missing.iter().map(|k| (*k).clone()).collect());
        report.coverage.push((lang.code.clone(), report.keys.len() - missing.len(), missing.len()));
        if lang.complete_menus {
            for k in &missing {
                let ctx = if k.context.is_empty() { String::new() } else { format!(" ({})", k.context) };
                report.failures.push(format!("{file}: missing {:?}{ctx}: {} is complete_menus", k.source, lang.code));
            }
        }
        let translated = notes.iter().filter(|l| have.contains(&Key::new("", l))).count();
        report.notes.push((lang.code.clone(), translated, notes.len()));
    }
    Ok(report)
}

fn render(r: &Report) -> String {
    let mut out = format!("translatable keys: {}\n", r.keys.len());
    for (origin, n) in &r.origins {
        out.push_str(&format!("  {origin:<16} {n:>6}\n"));
    }
    out.push_str(&format!("{:<10} {:>16} {:>9}\n", "language", "translated", "coverage"));
    let total = r.keys.len().max(1);
    for (code, done, _) in &r.coverage {
        out.push_str(&format!("{code:<10} {done:>7}/{:<8} {:>8.1}%\n", r.keys.len(), 100.0 * *done as f64 / total as f64));
    }
    for (code, n) in &r.drafts {
        out.push_str(&format!("{code}: {n} draft entr{} awaiting the fluent review (`~`)\n", if *n == 1 { "y" } else { "ies" }));
    }
    for (code, done, of) in &r.notes {
        out.push_str(&format!("{code}: release notes {done}/{of} lines translated (reported, never failed)\n"));
    }
    for f in &r.failures {
        out.push_str(&format!("FAIL {f}\n"));
    }
    out
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The `.rs` files under `dir`, their test modules cut off, sorted by path.
fn sources(dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", d.display()))?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = read(&path)?;
                out.push((path, without_tests(&text)));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `source` without its test module (from `#[cfg(test)]` before a `mod`) and without comment
/// lines, so doc examples are not keys.
fn without_tests(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if t.starts_with("#[cfg(test)]") && (t.contains("mod ") || lines.peek().is_some_and(|n| n.trim_start().starts_with("mod "))) {
            break;
        }
        if t.starts_with("//") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A Rust string literal at the start of `s` (after blanks): its value and the rest after it.
/// Plain (`"…"`, with escapes) and raw (`r"…"`, `r#"…"#`) literals.
fn literal(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if let Some(raw) = s.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let body = raw.get(hashes..)?.strip_prefix('"')?;
        let close = format!("\"{}", "#".repeat(hashes));
        let end = body.find(&close)?;
        return Some((body.get(..end)?.to_string(), body.get(end + close.len()..)?));
    }
    let body = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, body.get(i + 1..)?)),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let rest = body.get(i + 2..)?;
                    let end = rest.find('}')?;
                    let code = u32::from_str_radix(rest.get(1..end)?, 16).ok()?;
                    out.push(char::from_u32(code)?);
                    for _ in 0..=end {
                        chars.next();
                    }
                }
                '\n' => {
                    // A line continuation: the next line's leading blanks are skipped.
                    while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                        chars.next();
                    }
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

/// The arguments of the call (or list) whose opening bracket came just before `s`, split at
/// top-level commas (strings and nested brackets respected), up to its closing bracket; `None`
/// when it is not closed.
fn call_args(s: &str) -> Option<Vec<&str>> {
    let mut depth = 0usize;
    let mut args = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                let (_, rest) = literal(s.get(i..)?)?;
                let skip = s.len() - rest.len();
                while chars.clone().next().is_some_and(|(j, _)| j < skip) {
                    chars.next();
                }
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ')' | ']' | '}' => {
                args.push(s.get(start..i)?);
                return Some(args);
            }
            ',' if depth == 0 => {
                args.push(s.get(start..i)?);
                start = i + 1;
            }
            _ => {}
        }
    }
    None
}

/// Each place `name(` is called in `s` (not defined: `fn name(` is skipped; not a longer name
/// ending in it), with the text after the `(`.
fn calls<'a>(s: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    s.match_indices(name).filter_map(move |(at, _)| {
        let before = s.get(..at)?;
        if before.ends_with(|c: char| c.is_alphanumeric() || c == '_') || before.trim_end().ends_with("fn") {
            return None;
        }
        s.get(at + name.len()..)
    })
}

/// The `tl!` keys of `s`: `tl!("…")`, `tl!("…", k = v)`, `tl!(ctx = "c", "…")`.
pub fn tl_keys(s: &str) -> BTreeSet<Key> {
    let mut out = BTreeSet::new();
    for rest in calls(s, "tl!(") {
        let rest = rest.trim_start();
        let (ctx, rest) = match rest.strip_prefix("ctx") {
            Some(r) => {
                let Some(r) = r.trim_start().strip_prefix('=') else { continue };
                let Some((ctx, r)) = literal(r) else { continue };
                let Some(r) = r.trim_start().strip_prefix(',') else { continue };
                (ctx, r)
            }
            None => (String::new(), rest),
        };
        if let Some((source, _)) = literal(rest) {
            out.insert(Key { context: ctx, source });
        }
    }
    out
}

/// The `tln!(n, "one", "other", …)` keys of `s`, as `@plural one|other`.
pub fn plural_keys(s: &str) -> BTreeSet<Key> {
    calls(s, "tln!(")
        .filter_map(|rest| {
            let args = call_args(rest)?;
            let (one, _) = literal(args.get(1)?)?;
            let (other, _) = literal(args.get(2)?)?;
            Some(Key::new("@plural", &format!("{one}|{other}")))
        })
        .collect()
}

/// The `label: "…"` values of `s` (command specs and UI commands), empty ones left out.
pub fn label_keys(s: &str) -> BTreeSet<Key> {
    s.match_indices("label: \"")
        .filter_map(|(at, _)| literal(s.get(at + "label: ".len()..)?))
        .filter(|(l, _)| !l.trim().is_empty())
        .map(|(l, _)| Key::new("", &l))
        .collect()
}

/// The menu catalog's item and submenu labels: `item("…", …)`, `Submenu("…", …)`.
pub fn menu_keys(s: &str) -> BTreeSet<Key> {
    let mut out = BTreeSet::new();
    for name in ["item(", "Submenu("] {
        for rest in calls(s, name) {
            if let Some((l, _)) = literal(rest) {
                out.insert(Key::new("", &l));
            }
        }
    }
    out
}

/// `ui-kit`'s own texts: each `Name => ("context", "English"),` of the `texts!` table.
pub fn kit_keys(s: &str) -> BTreeSet<Key> {
    let Some((_, table)) = s.split_once("texts! {") else { return BTreeSet::new() };
    table
        .match_indices("=> (")
        .filter_map(|(at, _)| {
            let args = call_args(table.get(at + "=> (".len()..)?)?;
            let (context, _) = literal(args.first()?)?;
            let (source, _) = literal(args.get(1)?)?;
            Some(Key { context, source })
        })
        .collect()
}

/// The Options catalogue's texts: each `row(key, tab, "Group", "Label", …)`'s group and label,
/// each `file_type(key, "Label", …)`'s label, each `.hint("…")`, and the labels of the choice
/// tables (`&[("id", "Label"), …]`).
pub fn option_keys(s: &str) -> BTreeSet<Key> {
    let mut out = BTreeSet::new();
    let mut take = |arg: Option<&&str>| {
        if let Some((l, rest)) = arg.and_then(|a| literal(a))
            && rest.trim().is_empty()
            && !l.trim().is_empty()
        {
            out.insert(Key::new("", &l));
        }
    };
    for rest in calls(s, "row(") {
        if let Some(args) = call_args(rest) {
            take(args.get(2));
            take(args.get(3));
        }
    }
    for rest in calls(s, "file_type(") {
        if let Some(args) = call_args(rest) {
            take(args.get(1));
        }
    }
    for rest in calls(s, ".hint(") {
        if let Some(args) = call_args(rest) {
            take(args.first());
        }
    }
    const TABLE: &str = ": &[(&str, &str)] =";
    for (at, _) in s.match_indices(TABLE) {
        let Some(table) = s.get(at + TABLE.len()..).and_then(|t| t.trim_start().strip_prefix("&[")).and_then(call_args) else { continue };
        for pair in table {
            if let Some(args) = pair.trim().strip_prefix('(').and_then(call_args) {
                take(args.get(1));
            }
        }
    }
    out
}

/// The registry: each entry of `pub static LANGUAGES` (a `Language { … }` or the name of a
/// `const` one in the same file).
pub fn registry(s: &str) -> Result<Vec<Lang>, String> {
    let block = s
        .split_once("pub static LANGUAGES")
        .and_then(|(_, r)| r.split_once("= &["))
        .and_then(|(_, r)| r.split_once("];"))
        .map(|(b, _)| b)
        .ok_or("crates/ui/src/i18n/languages.rs: no `pub static LANGUAGES: &[Language] = &[…];`")?;
    let entries = call_args(&format!("{block})"))
        .ok_or("LANGUAGES: unbalanced brackets")?
        .into_iter()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut out: Vec<Lang> = Vec::new();
    for entry in entries {
        let body = if entry.starts_with("Language") {
            entry.clone()
        } else {
            let decl = format!("const {entry}: Language = ");
            let at = s.find(&decl).ok_or_else(|| format!("LANGUAGES: `{entry}` is not a `const … : Language` of languages.rs"))?;
            s.get(at + decl.len()..)
                .and_then(|r| r.split_once("};"))
                .map(|(b, _)| b.to_string())
                .ok_or_else(|| format!("LANGUAGES: `{entry}` is not closed"))?
        };
        let field = |name: &str| -> Option<&str> {
            let at = body.find(&format!("{name}:"))?;
            Some(body.get(at + name.len() + 1..)?.trim_start())
        };
        let code = field("code").and_then(literal).map(|(c, _)| c).ok_or_else(|| format!("LANGUAGES: `{entry}` has no code"))?;
        let plural = field("plural")
            .and_then(|r| r.strip_prefix("Plural::"))
            .map(|r| r.chars().take_while(|c| c.is_alphanumeric()).collect::<String>())
            .ok_or_else(|| format!("{code}: no `plural: Plural::…`"))?;
        let complete_menus = field("complete_menus").is_some_and(|r| r.starts_with("true"));
        let catalog = match field("catalog") {
            Some(r) if r.starts_with("include_str!(") => {
                let (file, _) = literal(r.get("include_str!(".len()..).unwrap_or_default()).ok_or_else(|| format!("{code}: a malformed catalog"))?;
                let p = Path::new(&file);
                if p.components().count() != 1 || p.extension().is_none_or(|e| e != "tsv") {
                    return Err(format!("{code}: the catalog `{file}` is not a .tsv file beside languages.rs"));
                }
                Some(file)
            }
            _ => None,
        };
        if out.iter().any(|l| l.code.eq_ignore_ascii_case(&code)) {
            return Err(format!("LANGUAGES: `{code}` is registered twice"));
        }
        out.push(Lang { code, plural, complete_menus, catalog });
    }
    if !out.first().is_some_and(|l| l.code == "en") {
        return Err("LANGUAGES: English (`en`) must come first".into());
    }
    Ok(out)
}

/// `s` unescaped (`\n`, `\t`, `\\`), `None` for any other backslash.
fn unescape(s: &str) -> Option<String> {
    let mut out = String::new();
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

/// `s` escaped for a catalog column (`\\`, `\n`, `\t`).
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

/// The `{name}` placeholders of `s`.
fn placeholders(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find('}') else { break };
        let name = after.get(..close).unwrap_or_default();
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            out.insert(name.to_string());
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out
}

/// One catalog entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: Key,
    pub translation: String,
    /// Marked `~`: a machine-assisted draft awaiting the fluent review.
    pub draft: bool,
}

/// A catalog's entries and its faults (`line N: …`), for a language whose plurals list `forms`.
pub fn parse(text: &str, forms: usize) -> (Vec<Entry>, Vec<String>) {
    let mut entries: Vec<Entry> = Vec::new();
    let mut seen: BTreeMap<Key, usize> = BTreeMap::new();
    let mut faults = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (draft, line) = match line.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let cols: Vec<&str> = line.split('\t').collect();
        let [ctx, src, tr] = cols.as_slice() else {
            faults.push(format!("line {n}: expected `context<TAB>source<TAB>translation`, found {} column(s)", cols.len()));
            continue;
        };
        let (Some(ctx), Some(src), Some(tr)) = (unescape(ctx), unescape(src), unescape(tr)) else {
            faults.push(format!("line {n}: a backslash that is not \\n, \\t or \\\\"));
            continue;
        };
        if src.trim().is_empty() || tr.trim().is_empty() {
            faults.push(format!("line {n}: an empty entry"));
            continue;
        }
        let (wanted, given): (BTreeSet<String>, BTreeSet<String>) = if ctx == "@plural" {
            let one_other: Vec<&str> = src.split('|').collect();
            let translated: Vec<&str> = tr.split('|').collect();
            if one_other.len() != 2 || one_other.iter().any(|f| f.trim().is_empty()) {
                faults.push(format!("line {n}: a malformed plural: the source is not `one|other`"));
                continue;
            }
            if translated.len() != forms || translated.iter().any(|f| f.trim().is_empty()) {
                faults.push(format!("line {n}: a malformed plural: {} form(s), the language has {forms}", translated.len()));
                continue;
            }
            (one_other.iter().flat_map(|f| placeholders(f)).collect(), translated.iter().flat_map(|f| placeholders(f)).collect())
        } else if ctx == "@id" {
            (BTreeSet::new(), BTreeSet::new())
        } else {
            (placeholders(&src), placeholders(&tr))
        };
        if wanted != given {
            faults.push(format!("line {n}: a placeholder in one column only ({wanted:?} / {given:?})"));
            continue;
        }
        let key = Key { context: ctx, source: src };
        if let Some(first) = seen.get(&key) {
            faults.push(format!("line {n}: a duplicate key (first on line {first})"));
            continue;
        }
        seen.insert(key.clone(), n);
        entries.push(Entry { key, translation: tr, draft });
    }
    (entries, faults)
}

/// The release notes' texts a catalog may translate, as Help ▸ Release Notes looks them up
/// (`crates/ui/src/dialogs/release_notes.rs`, `blocks`): HTML comments and the `#` title left out,
/// each `##`-or-deeper heading's text, each bullet without its `- ` (or `* `) with its wrapped
/// continuation lines joined by a space, each paragraph joined likewise, and inline marks reduced
/// to their text (`**`, `` ` ``, `[text](url)` → `text`).
pub fn release_note_lines(md: &str) -> Vec<String> {
    let mut text = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(start) = rest.find("<!--") {
        text.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 4..).unwrap_or_default();
        rest = after.find("-->").and_then(|end| after.get(end + 3..)).unwrap_or_default();
    }
    text.push_str(rest);
    let mut out: Vec<String> = Vec::new();
    // Whether the last text still takes continuation lines (a bullet or a paragraph).
    let mut open = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            open = false;
            continue;
        }
        let hashes = t.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && t.as_bytes().get(hashes).is_none_or(|&b| b == b' ') {
            open = false;
            if hashes > 1 {
                out.push(inline(t.get(hashes..).unwrap_or_default().trim()));
            }
            continue;
        }
        if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push(inline(item.trim()));
            open = true;
            continue;
        }
        let more = inline(t);
        match out.last_mut() {
            Some(last) if open => {
                last.push(' ');
                last.push_str(&more);
            }
            _ => {
                out.push(more);
                open = true;
            }
        }
    }
    out
}

/// Inline Markdown reduced to its text, as the release-notes dialog shows it: a link keeps its
/// text, emphasis and code marks go.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = rest.get(open + 1..).unwrap_or_default();
        let link = after.find(']').filter(|&close| after.get(close + 1..).is_some_and(|r| r.starts_with('('))).and_then(|close| {
            let target = after.get(close + 2..)?;
            let end = target.find(')')?;
            Some((after.get(..close)?, target.get(end + 1..)?))
        });
        match link {
            Some((label, tail)) => {
                out.push_str(rest.get(..open).unwrap_or_default());
                out.push_str(label);
                rest = tail;
            }
            None => {
                out.push_str(rest.get(..=open).unwrap_or_default());
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small tree with one complete language (`xx`, one/other) whose catalog covers every key.
    fn tree(catalog: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let w = |rel: &str, text: &str| {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        w(
            "crates/ui/src/panel.rs",
            "fn f() { tl!(\"Open\"); tl!(ctx = \"verb\", \"Close\"); tl!(\"{count} photos\", count = n); tln!(n, \"{n} photo\", \"{n} photos\"); }\n\
             // tl!(\"In a comment\")\n#[cfg(test)]\nmod tests { fn t() { tl!(\"In a test\"); } }\n",
        );
        w("crates/ui/src/commands/view.rs", "const C: UiCommand = UiCommand { id: \"view.x\", label: \"Show X\", menu: \"\" };\n");
        w("crates/ui/src/menu_catalog.rs", "pub const M: &[Entry] = &[item(\"New Album…\", \"album.new\").e(), Entry::Submenu(\"Sort By\", S)];\n");
        w("crates/engine/src/cmd/photo.rs", "CommandSpec { id: \"photo.star\", label: \"Star\", menu: \"\" }\n");
        w(
            "crates/engine/src/options.rs",
            "const fn row(key: &'static str, tab: Tab, group: &'static str, label: &'static str) -> O { }\n\
             const SIZES: &[(&str, &str)] = &[(\"small\", \"Small\")];\n\
             static CATALOGUE: &[O] = &[row(\"a.b\", Tab::General, \"Interface\", \"Use animations\", Kind::Bool).hint(\"Smooth.\")];\n",
        );
        w(
            "crates/ui/src/i18n/languages.rs",
            "pub const ENGLISH: Language = Language { code: \"en\", plural: Plural::OneOther, complete_menus: false, catalog: \"\" };\n\
             pub static LANGUAGES: &[Language] = &[\n    ENGLISH,\n    Language { code: \"xx\", plural: Plural::OneOther, complete_menus: true, catalog: include_str!(\"xx.tsv\") },\n];\n",
        );
        w("crates/ui-kit/src/text.rs", "texts! {\n    May => (\"month\", \"May\"),\n    /// A doc.\n    CloseNamed => (\"room tab\", \"Close {name}\"),\n}\n");
        w("crates/ui/src/i18n/xx.tsv", catalog);
        w("docs/whats-new.md", "# Dalica 1.0\n\n<!-- - Hidden -->\n## 1.0\n\n- Videos\n- Open\n");
        dir
    }

    const GOOD: &str = "# xx\n\tOpen\tOpenx\nverb\tClose\tClosex\n\t{count} photos\t{count} photosx\n@plural\t{n} photo|{n} photos\t{n} x|{n} xs\n\
                        \tShow X\tShowx\n\tNew Album…\tNewx…\n\tSort By\tSortx\n\tStar\tStarx\n\tInterface\tInterfacex\n\tUse animations\tAnimx\n\
                        \tSmooth.\tSmoothx.\n\tSmall\tSmallx\n~month\tMay\tMayx\n~room tab\tClose {name}\tClosex {name}\n";

    #[test]
    fn the_key_set_comes_from_every_source() {
        let dir = tree(GOOD);
        let r = check(dir.path()).unwrap();
        let sources: BTreeSet<(&str, &str)> = r.keys.iter().map(|k| (k.context.as_str(), k.source.as_str())).collect();
        let wanted: BTreeSet<(&str, &str)> = [
            ("", "Open"),
            ("verb", "Close"),
            ("", "{count} photos"),
            ("@plural", "{n} photo|{n} photos"),
            ("", "Show X"),
            ("", "New Album…"),
            ("", "Sort By"),
            ("", "Star"),
            ("", "Interface"),
            ("", "Use animations"),
            ("", "Smooth."),
            ("", "Small"),
            ("month", "May"),
            ("room tab", "Close {name}"),
        ]
        .into_iter()
        .collect();
        assert_eq!(sources, wanted);
        assert_eq!(r.failures, Vec::<String>::new());
        assert_eq!(r.coverage, [("en".to_string(), 14, 0), ("xx".to_string(), 14, 0)]);
        assert_eq!(r.notes, [("xx".to_string(), 1, 3)]);
        assert_eq!(r.drafts, [("xx".to_string(), 2)]);
    }

    #[test]
    fn each_planted_fault_fails() {
        for (what, line, expect) in [
            ("an empty entry", "\tVideos\t \n", "an empty entry"),
            ("a placeholder in one column", "\tVideos {n}\tVideosx\n", "a placeholder in one column only"),
            ("a malformed plural", "@plural\t{n} a|{n} b\t{n} x\n", "a malformed plural"),
            ("a plural source", "@plural\t{n} a\t{n} x|{n} xs\n", "a malformed plural"),
            ("a duplicate key", "\tOpen\tOpeny\n", "a duplicate key"),
            ("a broken escape", "\tVideos\\q\tVideosx\n", "a backslash"),
            ("two columns", "\tVideos\n", "expected `context<TAB>source<TAB>translation`"),
        ] {
            let dir = tree(&format!("{GOOD}{line}"));
            let r = check(dir.path()).unwrap();
            assert_eq!(r.failures.len(), 1, "{what}: {:?}", r.failures);
            assert!(r.failures[0].contains(expect), "{what}: {:?}", r.failures);
        }
        // A missing key fails a complete_menus language, and only reports another.
        let dir = tree(&GOOD.replace("\tStar\tStarx\n", ""));
        let r = check(dir.path()).unwrap();
        assert_eq!(r.failures.len(), 1, "{:?}", r.failures);
        assert!(r.failures[0].contains("missing \"Star\""), "{:?}", r.failures);
        let languages = dir.path().join("crates/ui/src/i18n/languages.rs");
        fs::write(&languages, fs::read_to_string(&languages).unwrap().replace("complete_menus: true", "complete_menus: false")).unwrap();
        let r = check(dir.path()).unwrap();
        assert_eq!((r.failures.len(), r.coverage.get(1).cloned()), (0, Some(("xx".to_string(), 13, 1))));
        assert_eq!(r.missing.get("xx").cloned(), Some(vec![Key::new("", "Star")]));
    }

    #[test]
    fn release_notes_are_counted_as_the_dialog_draws_them() {
        let md = "# What's new\n\n<!-- a comment\n- not shown -->\n## 0.1 — June 2026\n\nIntro that\nwraps.\n\n### Library\n\n- **Star** photos with `photo.star.toggle`\n  and more on the next line\n  - nested [see the guide](docs/x.md)\n";
        assert_eq!(
            release_note_lines(md),
            ["0.1 — June 2026", "Intro that wraps.", "Library", "Star photos with photo.star.toggle and more on the next line", "nested see the guide"]
        );
        assert_eq!(escape("a\tb\nc\\d"), "a\\tb\\nc\\\\d");
    }

    #[test]
    fn literals_calls_and_the_registry_parse() {
        assert_eq!(literal(r#" "a\"b\n\u{e9}" rest"#), Some(("a\"b\né".to_string(), " rest")));
        assert_eq!(literal(r##"r#"raw "x""# z"##), Some(("raw \"x\"".to_string(), " z")));
        assert_eq!(literal("\"open"), None);
        assert_eq!(call_args("a, f(b, c), \"d,)\", [e, f]) tail"), Some(vec!["a", " f(b, c)", " \"d,)\"", " [e, f]"]));
        assert!(registry("pub static LANGUAGES: &[Language] = &[Language { code: \"de\", plural: Plural::OneOther }];").is_err(), "English first");
        assert!(registry("nothing").is_err());
    }

    #[test]
    fn the_tree_passes_and_its_key_set_holds_every_kind() {
        let r = check(&crate::workspace_root()).unwrap();
        assert_eq!(r.failures, Vec::<String>::new());
        for (ctx, source) in [
            ("", "Star / Unstar"),
            ("", "New Album…"),
            ("", "Thumbnail Caption"),
            ("", "Use animations"),
            ("", "Smooth transitions when rotating, applying edits and previewing."),
            ("", "Show JPEG files and:"),
            ("", "HEIC/HEIF"),
            ("", "Mouse wheel zooms (hold Ctrl to navigate)"),
            ("", "Small"),
            ("", "Browse…"),
            ("month", "June"),
            ("@plural", "{n} byte|{n} bytes"),
        ] {
            assert!(r.keys.contains(&Key::new(ctx, source)), "{ctx:?} {source:?}");
        }
        assert!(r.keys.len() > 400, "{}", r.keys.len());
    }
}
