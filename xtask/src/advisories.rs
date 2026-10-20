//! `cargo xtask advisories`: the RustSec advisory check of `docs/security.md` §8.4–§8.5.
//!
//! Reads a checkout of the RustSec advisory database (`crates/<name>/RUSTSEC-*.md`, each with a
//! TOML front matter) and matches every advisory against the crates the release packages ship: the
//! product graph of each release target, as `cargo xtask sbom` lists it. A vulnerability in a
//! shipped crate fails the check unless `security.md` §8.4 records its id with an owner, a
//! rationale, a compensating control and a review date (a dated `*Review:*`). Informational
//! advisories (unmaintained, unsound, notices) are printed as warnings to review; so is a recorded
//! exception that no longer matches anything, or whose review date has passed.
//!
//! The check itself is offline: it reads the database from disk (`--db`, `DALICA_ADVISORY_DB`, or
//! `target/advisory-db`). Only `--fetch` uses the network, to clone or update that checkout; the
//! review gate (`cargo xtask ci --full`) and the release workflow fetch, nothing else does, and
//! the app never does. Version requirements follow Cargo's semver rules (the RustSec crate's
//! `VersionReq` matching), implemented here so xtask needs no new dependency.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where `--fetch` clones the database from.
pub const DB_URL: &str = "https://github.com/rustsec/advisory-db";

/// Overrides the database directory.
pub const DB_ENV: &str = "DALICA_ADVISORY_DB";

/// At most this many advisory files are read (the database holds about a thousand).
const MAX_FILES: usize = 50_000;
/// An advisory file larger than this is refused.
const MAX_FILE_BYTES: u64 = 1 << 20;

// ---------------------------------------------------------------------------------------------
// Versions and requirements (Cargo's semver rules).

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Alpha(String),
}

impl Ord for Ident {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
            (Ident::Num(_), Ident::Alpha(_)) => Ordering::Less,
            (Ident::Alpha(_), Ident::Num(_)) => Ordering::Greater,
            (Ident::Alpha(a), Ident::Alpha(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `major.minor.patch[-pre]` (build metadata dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<Ident>,
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_pre(s: &str) -> Option<Vec<Ident>> {
    s.split('.')
        .map(|id| {
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                None
            } else if id.chars().all(|c| c.is_ascii_digit()) {
                id.parse().ok().map(Ident::Num)
            } else {
                Some(Ident::Alpha(id.to_string()))
            }
        })
        .collect()
}

fn number(s: &str) -> Option<u64> {
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim();
    let s = s.split_once('+').map_or(s, |(v, _)| v);
    let (core, pre) = s.split_once('-').map_or((s, None), |(c, p)| (c, Some(p)));
    let mut parts = core.split('.');
    let v = Version {
        major: number(parts.next()?)?,
        minor: number(parts.next()?)?,
        patch: number(parts.next()?)?,
        pre: match pre {
            Some(p) => parse_pre(p)?,
            None => Vec::new(),
        },
    };
    parts.next().is_none().then_some(v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<Ident>,
    /// `*`: any version.
    any: bool,
}

/// One requirement: comparators joined by `,` (all must hold).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Req(Vec<Comparator>);

fn parse_comparator(s: &str) -> Option<Comparator> {
    let s = s.trim();
    let (op, rest) =
        [(">=", Op::GreaterEq), ("<=", Op::LessEq), (">", Op::Greater), ("<", Op::Less), ("=", Op::Exact), ("~", Op::Tilde), ("^", Op::Caret)]
            .into_iter()
            .find_map(|(p, op)| s.strip_prefix(p).map(|r| (op, r.trim())))
            .unwrap_or((Op::Caret, s));
    let (core, pre) = rest.split_once('-').map_or((rest, None), |(c, p)| (c, Some(p)));
    let mut parts = core.split('.');
    let wild = |p: &str| matches!(p, "*" | "x" | "X");
    let major_s = parts.next()?;
    if wild(major_s) {
        return (op == Op::Caret && parts.next().is_none() && pre.is_none()).then_some(Comparator {
            op: Op::Wildcard,
            major: 0,
            minor: None,
            patch: None,
            pre: vec![],
            any: true,
        });
    }
    let major = number(major_s)?;
    let mut op = op;
    let mut field = |op: &mut Op| -> Option<Option<u64>> {
        match parts.next() {
            None => Some(None),
            Some(p) if wild(p) => {
                if *op != Op::Caret && *op != Op::Wildcard {
                    return None;
                }
                *op = Op::Wildcard;
                Some(None)
            }
            Some(_) if *op == Op::Wildcard => None,
            Some(p) => number(p).map(Some),
        }
    };
    let minor = field(&mut op)?;
    let patch = if minor.is_some() { field(&mut op)? } else { None };
    if parts.next().is_some() {
        return None;
    }
    let pre = match pre {
        Some(p) if patch.is_some() => parse_pre(p)?,
        Some(_) => return None,
        None => vec![],
    };
    Some(Comparator { op, major, minor, patch, pre, any: false })
}

pub fn parse_req(s: &str) -> Option<Req> {
    let comps: Option<Vec<Comparator>> = s.split(',').map(parse_comparator).collect();
    comps.filter(|c| !c.is_empty()).map(Req)
}

fn v(major: u64, minor: u64, patch: u64) -> Version {
    Version { major, minor, patch, pre: vec![] }
}

impl Comparator {
    fn lower(&self) -> Version {
        Version { major: self.major, minor: self.minor.unwrap_or(0), patch: self.patch.unwrap_or(0), pre: self.pre.clone() }
    }

    /// Whether `ver` satisfies this comparator, ignoring the pre-release rule.
    fn matches(&self, ver: &Version) -> bool {
        let (major, minor, patch) = (self.major, self.minor, self.patch);
        let lo = self.lower();
        // The version just past the given parts: `I` → `I+1.0.0`, `I.J` → `I.J+1.0`.
        let next = |m: u64, n: Option<u64>| match n {
            None => v(m.saturating_add(1), 0, 0),
            Some(n) => v(m, n.saturating_add(1), 0),
        };
        match self.op {
            Op::Wildcard => match minor {
                _ if self.any => true,
                None => ver.major == major,
                Some(n) => ver.major == major && ver.minor == n,
            },
            Op::Exact => match (minor, patch) {
                (Some(_), Some(_)) => *ver == lo,
                _ => *ver >= lo && *ver < next(major, minor),
            },
            Op::Greater => match (minor, patch) {
                (Some(_), Some(_)) => *ver > lo,
                _ => *ver >= next(major, minor),
            },
            Op::GreaterEq => *ver >= lo,
            Op::Less => *ver < lo,
            Op::LessEq => match (minor, patch) {
                (Some(_), Some(_)) => *ver <= lo,
                _ => *ver < next(major, minor),
            },
            Op::Tilde => match minor {
                None => *ver >= lo && *ver < next(major, None),
                Some(n) => *ver >= lo && *ver < next(major, Some(n)),
            },
            Op::Caret => {
                let upper = match (major, minor, patch) {
                    (0, None, _) => v(1, 0, 0),
                    (0, Some(0), None) => v(0, 1, 0),
                    (0, Some(0), Some(p)) => v(0, 0, p.saturating_add(1)),
                    (0, Some(n), _) => v(0, n.saturating_add(1), 0),
                    (m, _, _) => v(m.saturating_add(1), 0, 0),
                };
                *ver >= lo && *ver < upper
            }
        }
    }
}

impl Req {
    /// Cargo's rule: every comparator holds, and a pre-release version matches only when a
    /// comparator names the same `major.minor.patch` with a pre-release.
    pub fn matches(&self, ver: &Version) -> bool {
        self.0.iter().all(|c| c.matches(ver))
            && (ver.pre.is_empty()
                || self.0.iter().any(|c| !c.pre.is_empty() && c.major == ver.major && c.minor == Some(ver.minor) && c.patch == Some(ver.patch)))
    }
}

// ---------------------------------------------------------------------------------------------
// Advisories.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Advisory {
    pub id: String,
    pub package: String,
    pub title: String,
    /// `unmaintained`, `unsound`, `notice`…: not a vulnerability.
    pub informational: Option<String>,
    pub withdrawn: bool,
    pub patched: Vec<String>,
    pub unaffected: Vec<String>,
    /// `[affected] os` / `arch`: empty means every platform.
    pub os: Vec<String>,
    pub arch: Vec<String>,
}

/// A TOML string or array of strings (the only value shapes the fields read here take).
fn toml_strings(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    let quoted = |s: &str| -> Option<String> {
        let s = s.trim();
        s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).or_else(|| s.strip_prefix('\'').and_then(|r| r.strip_suffix('\''))).map(str::to_string)
    };
    if let Some(inner) = value.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        for c in inner.chars() {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => {
                    out.push(std::mem::take(&mut cur));
                    quote = None;
                }
                (Some(_), c) => cur.push(c),
                (None, ',' | ' ' | '\t' | '\n' | '\r') => {}
                (None, '#') => return None,
                (None, _) => return None,
            }
        }
        quote.is_none().then_some(out)
    } else {
        quoted(value).map(|s| vec![s])
    }
}

/// A line without its `# comment` (a `#` outside quotes).
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '#') => return line.get(..i).unwrap_or(line),
            _ => {}
        }
    }
    line
}

/// The advisory of one database file: its ```` ```toml ```` front matter and the first `# ` title.
pub fn parse_advisory(text: &str) -> Result<Advisory, String> {
    let body = text.trim_start().strip_prefix("```toml").ok_or("no ```toml front matter")?;
    let (front, rest) = body.split_once("\n```").ok_or("unterminated front matter")?;
    let mut a = Advisory { title: rest.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or("").trim().to_string(), ..Advisory::default() };
    let mut section = String::new();
    let mut lines = front.lines();
    while let Some(line) = lines.next() {
        let line = strip_comment(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && !line.contains('=') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let mut value = value.trim().to_string();
        // A multi-line array: read on to its closing bracket.
        if value.starts_with('[') {
            let mut guard = 0;
            while !value.trim_end().ends_with(']') && guard < 10_000 {
                let Some(next) = lines.next() else { break };
                let next = strip_comment(next);
                value.push(' ');
                value.push_str(next.trim());
                guard += 1;
            }
        }
        let wanted = matches!(
            (section.as_str(), key),
            ("advisory", "id" | "package" | "informational" | "withdrawn") | ("versions", "patched" | "unaffected") | ("affected", "os" | "arch")
        );
        if !wanted {
            continue;
        }
        // A TOML date (`withdrawn = 2021-12-20`) or a string: either way, withdrawn.
        if (section.as_str(), key) == ("advisory", "withdrawn") {
            a.withdrawn = !value.is_empty();
            continue;
        }
        let strings = toml_strings(&value).ok_or_else(|| format!("cannot read `{key}` in [{section}]"))?;
        let first = || strings.first().cloned().unwrap_or_default();
        match (section.as_str(), key) {
            ("advisory", "id") => a.id = first(),
            ("advisory", "package") => a.package = first(),
            ("advisory", "informational") => a.informational = Some(first()),
            ("versions", "patched") => a.patched = strings,
            ("versions", "unaffected") => a.unaffected = strings,
            ("affected", "os") => a.os = strings,
            ("affected", "arch") => a.arch = strings,
            _ => {}
        }
    }
    if a.id.is_empty() || a.package.is_empty() {
        return Err("no `id` or `package` in [advisory]".into());
    }
    Ok(a)
}

/// Whether `version` of the advisory's crate is affected: matched by no `patched` or
/// `unaffected` requirement. A requirement that does not parse is an error, never a pass.
pub fn affects(a: &Advisory, version: &str) -> Result<bool, String> {
    let ver = parse_version(version).ok_or_else(|| format!("{}: cannot parse the version `{version}`", a.package))?;
    for r in a.patched.iter().chain(&a.unaffected) {
        let req = parse_req(r).ok_or_else(|| format!("{}: cannot parse the requirement `{r}`", a.id))?;
        if req.matches(&ver) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the advisory's `[affected] os`/`arch` covers a target triple.
pub fn applies_to(a: &Advisory, target: &str) -> bool {
    let arch = target.split('-').next().unwrap_or("");
    let os = if target.contains("-windows") {
        "windows"
    } else if target.contains("-apple-darwin") {
        "macos"
    } else if target.contains("-linux") {
        "linux"
    } else {
        ""
    };
    (a.os.is_empty() || a.os.iter().any(|o| o == os)) && (a.arch.is_empty() || a.arch.iter().any(|x| x == arch))
}

/// Every advisory under `<db>/crates/`.
pub fn load_db(db: &Path) -> Result<Vec<Advisory>, String> {
    let crates = db.join("crates");
    let dirs = std::fs::read_dir(&crates).map_err(|e| format!("advisories: cannot read {}: {e}", crates.display()))?;
    let mut out = Vec::new();
    let mut files = 0usize;
    for dir in dirs.flatten() {
        let Ok(entries) = std::fs::read_dir(dir.path()) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("RUSTSEC-") && name.ends_with(".md")) {
                continue;
            }
            files += 1;
            if files > MAX_FILES {
                return Err(format!("advisories: more than {MAX_FILES} advisory files in {}", crates.display()));
            }
            if e.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
                return Err(format!("advisories: {} is larger than {MAX_FILE_BYTES} bytes", path.display()));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("advisories: {}: {e}", path.display()))?;
            out.push(parse_advisory(&text).map_err(|e| format!("advisories: {}: {e}", path.display()))?);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Recorded exceptions (`security.md` §8.4).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exception {
    pub id: String,
    pub owner: String,
    pub rationale: String,
    pub control: String,
    /// `YYYY-MM-DD`.
    pub review: String,
}

const FIELDS: &[&str] = &["*Rationale:*", "*Compensating control:*", "*Owner:*", "*Review:*"];

fn field(entry: &str, marker: &str) -> Option<String> {
    let start = entry.find(marker)? + marker.len();
    let rest = entry.get(start..)?;
    let end = FIELDS.iter().filter_map(|m| rest.find(m)).min().unwrap_or(rest.len());
    let value = rest.get(..end)?.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', ',', ';']).to_string();
    (!value.is_empty()).then_some(value)
}

fn first_date(s: &str) -> Option<String> {
    s.char_indices().find_map(|(i, _)| {
        let d = s.get(i..i + 10)?;
        let b = d.as_bytes();
        let ok = b.iter().enumerate().all(|(j, c)| if j == 4 || j == 7 { *c == b'-' } else { c.is_ascii_digit() });
        ok.then(|| d.to_string())
    })
}

/// Every `RUSTSEC-YYYY-NNNN` id in `s`, in order.
fn rustsec_ids(s: &str) -> Vec<String> {
    s.match_indices("RUSTSEC-")
        .filter_map(|(at, _)| {
            let id = s.get(at..at + 17)?;
            let b = id.as_bytes();
            let ok = b.get(8..12).is_some_and(|y| y.iter().all(u8::is_ascii_digit))
                && b.get(12) == Some(&b'-')
                && b.get(13..17).is_some_and(|n| n.iter().all(u8::is_ascii_digit));
            ok.then(|| id.to_string())
        })
        .collect()
}

/// The exceptions §8.4 records: each bullet whose bold head names one or more `RUSTSEC-` ids
/// (one exception per id, sharing the bullet's fields). A recorded entry missing a field, or a `*Review:*` without a date, is an error.
pub fn exceptions(security_md: &str) -> Result<Vec<Exception>, Vec<String>> {
    let section: Vec<&str> = security_md
        .lines()
        .skip_while(|l| !l.starts_with("### 8.4"))
        .skip(1)
        .take_while(|l| !l.starts_with("## ") && !l.starts_with("### "))
        .collect();
    if section.is_empty() {
        return Err(vec!["security.md has no §8.4 (### 8.4)".into()]);
    }
    let mut entries: Vec<String> = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for line in &section {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim_start();
        let starts_entry = t.starts_with("- **RUSTSEC-");
        let ends = t.is_empty() || (t.starts_with("- ") && current.as_ref().is_some_and(|(i, _)| indent <= *i));
        if (ends || starts_entry)
            && let Some((_, text)) = current.take()
        {
            entries.push(text);
        }
        if starts_entry {
            current = Some((indent, t.to_string()));
        } else if let Some((_, text)) = current.as_mut() {
            text.push(' ');
            text.push_str(t);
        }
    }
    if let Some((_, text)) = current {
        entries.push(text);
    }
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for e in entries {
        // The bold head: `- **RUSTSEC-…, RUSTSEC-…, `crate` x**`.
        let head = e.get(4..).and_then(|h| h.split_once("**")).map_or("", |(h, _)| h);
        let ids = rustsec_ids(head);
        let Some(id) = ids.first().cloned() else { continue };
        let get = |m: &str| field(&e, m);
        let review = get("*Review:*").as_deref().and_then(first_date);
        match (get("*Owner:*"), get("*Rationale:*"), get("*Compensating control:*"), review) {
            (Some(owner), Some(rationale), Some(control), Some(review)) => {
                for id in ids {
                    out.push(Exception { id, owner: owner.clone(), rationale: rationale.clone(), control: control.clone(), review: review.clone() });
                }
            }
            (owner, rationale, control, review) => {
                let missing: Vec<&str> = [
                    ("owner", owner.is_none()),
                    ("rationale", rationale.is_none()),
                    ("compensating control", control.is_none()),
                    ("review date", review.is_none()),
                ]
                .into_iter()
                .filter(|(_, m)| *m)
                .map(|(n, _)| n)
                .collect();
                errors.push(format!("security.md §8.4: {id} lacks its {}", missing.join(", ")));
            }
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

// ---------------------------------------------------------------------------------------------
// The check.

/// A shipped crate: name, version and the release targets that build it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipped {
    pub name: String,
    pub version: String,
    pub targets: BTreeSet<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Unrecorded vulnerabilities, and errors reading the database.
    pub failures: Vec<String>,
    /// Informational advisories, stale or overdue exceptions.
    pub warnings: Vec<String>,
    /// Vulnerabilities §8.4 records.
    pub recorded: Vec<String>,
}

/// Matches `db` against `shipped`; `today` is `YYYY-MM-DD`.
pub fn check(shipped: &[Shipped], db: &[Advisory], recorded: &[Exception], today: &str) -> Report {
    let mut r = Report::default();
    let mut used = BTreeSet::new();
    for a in db.iter().filter(|a| !a.withdrawn) {
        for c in shipped.iter().filter(|c| c.name == a.package) {
            let targets: Vec<&str> = c.targets.iter().map(String::as_str).filter(|t| applies_to(a, t)).collect();
            if targets.is_empty() {
                continue;
            }
            match affects(a, &c.version) {
                Err(e) => r.failures.push(e),
                Ok(false) => {}
                Ok(true) => {
                    let what = format!("{} {} {}: {}", a.id, c.name, c.version, a.title);
                    if let Some(kind) = &a.informational {
                        r.warnings.push(format!("{what} (informational: {kind}; review it)"));
                    } else if let Some(e) = recorded.iter().find(|e| e.id == a.id) {
                        used.insert(e.id.clone());
                        if e.review.as_str() < today {
                            r.warnings
                                .push(format!("{}: the review date {} in security.md §8.4 has passed; review it and set the next", e.id, e.review));
                        }
                        r.recorded.push(format!("{what} (recorded in security.md §8.4; owner: {}; review by {})", e.owner, e.review));
                    } else {
                        r.failures.push(format!(
                            "{what} [{}]: not recorded in security.md §8.4 (upgrade the crate, or record it with an owner, a rationale, a compensating control and a review date)",
                            targets.join(", ")
                        ));
                    }
                }
            }
        }
    }
    for e in recorded.iter().filter(|e| !used.contains(&e.id)) {
        r.warnings.push(format!("{}: recorded in security.md §8.4 but no shipped crate is affected any more; remove the entry", e.id));
    }
    r
}

/// Today's UTC date, `YYYY-MM-DD` (civil-from-days on the Unix clock).
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").current_dir(dir).args(args).output().map_err(|e| format!("advisories: cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("advisories: git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Clones the database into `db`, or brings a clone up to date (a shallow fetch of its head).
fn fetch(db: &Path) -> Result<(), String> {
    if db.join(".git").exists() {
        git(db, &["fetch", "-q", "--depth", "1", "origin", "HEAD"])?;
        git(db, &["reset", "-q", "--hard", "FETCH_HEAD"])?;
    } else {
        let parent = db.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| format!("advisories: cannot create {}: {e}", parent.display()))?;
        let target = db.to_string_lossy().into_owned();
        git(parent, &["clone", "-q", "--depth", "1", DB_URL, &target])?;
    }
    Ok(())
}

/// The review gate's step (`cargo xtask ci --full`): the database at `DALICA_ADVISORY_DB` as it is,
/// or `target/advisory-db` fetched first.
pub fn gate(root: &Path) -> Result<(), String> {
    match std::env::var_os(DB_ENV).filter(|v| !v.is_empty()) {
        Some(_) => run(root, &[]),
        None => run(root, &["--fetch".to_string()]),
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: cargo xtask advisories [--db <dir>] [--fetch]";
    let mut db: Option<PathBuf> = None;
    let mut do_fetch = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => db = Some(it.next().map(PathBuf::from).ok_or(USAGE)?),
            "--fetch" => do_fetch = true,
            other => return Err(format!("advisories: unknown argument `{other}` ({USAGE})")),
        }
    }
    let db = db
        .or_else(|| std::env::var_os(DB_ENV).filter(|v| !v.is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| root.join("target").join("advisory-db"));
    let db = if db.is_absolute() { db } else { std::env::current_dir().map_err(|e| e.to_string())?.join(db) };
    if do_fetch {
        println!("advisories: fetching {DB_URL} into {}", db.display());
        fetch(&db)?;
    } else if !db.join("crates").is_dir() {
        return Err(format!(
            "advisories: no advisory database at {} (run `cargo xtask advisories --fetch`, or point --db or {DB_ENV} at a checkout of {DB_URL})",
            db.display()
        ));
    }
    let advisories = load_db(&db)?;
    let revision = git(&db, &["log", "-1", "--format=%h %cs"]).unwrap_or_else(|_| "unknown revision".into());
    let security =
        std::fs::read_to_string(root.join("docs").join("security.md")).map_err(|e| format!("advisories: cannot read docs/security.md: {e}"))?;
    let recorded = exceptions(&security).map_err(|e| e.join("\n"))?;
    let mut shipped: Vec<Shipped> = Vec::new();
    for (_, comps) in crate::sbom::all(root)? {
        for c in comps.into_values() {
            match shipped.iter_mut().find(|s| s.name == c.name && s.version == c.version) {
                Some(s) => s.targets.extend(c.targets),
                None => shipped.push(Shipped { name: c.name, version: c.version, targets: c.targets }),
            }
        }
    }
    let report = check(&shipped, &advisories, &recorded, &today());
    for w in &report.warnings {
        eprintln!("advisories: warning: {w}");
    }
    for r in &report.recorded {
        println!("advisories: {r}");
    }
    for f in &report.failures {
        eprintln!("advisories: {f}");
    }
    let summary = format!(
        "{} advisories (database {revision}) against {} shipped crates: {} unrecorded, {} recorded, {} warnings",
        advisories.len(),
        shipped.len(),
        report.failures.len(),
        report.recorded.len(),
        report.warnings.len()
    );
    if report.failures.is_empty() {
        println!("advisories: ok ({summary})");
        Ok(())
    } else {
        Err(format!("advisories: failed ({summary})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(r: &str, ver: &str) -> bool {
        parse_req(r).unwrap_or_else(|| panic!("req {r}")).matches(&parse_version(ver).unwrap_or_else(|| panic!("version {ver}")))
    }

    #[test]
    fn requirements_follow_cargo_semver() {
        // Comparison operators, with partial versions.
        assert!(req(">= 0.9.5", "0.9.5") && req(">= 0.9.5", "1.2.0") && !req(">= 0.9.5", "0.9.4"));
        assert!(req("> 1.2", "1.3.0") && !req("> 1.2", "1.2.9"));
        assert!(req("< 0.10", "0.9.99") && !req("< 0.10", "0.10.0"));
        assert!(req("<= 1.2", "1.2.7") && !req("<= 1.2", "1.3.0"));
        assert!(req("=1.2", "1.2.5") && !req("=1.2", "1.3.0") && req("=1.2.3", "1.2.3") && !req("=1.2.3", "1.2.4"));
        // Caret (the default), tilde, wildcards, conjunctions.
        assert!(req("^0.8.3", "0.8.9") && !req("^0.8.3", "0.9.0") && !req("^0.8.3", "0.8.2"));
        assert!(req("1.2.3", "1.9.0") && !req("1.2.3", "2.0.0"));
        assert!(req("^0.0.3", "0.0.3") && !req("^0.0.3", "0.0.4"));
        assert!(req("~1.2.3", "1.2.9") && !req("~1.2.3", "1.3.0"));
        assert!(req("1.*", "1.7.0") && !req("1.*", "2.0.0") && req("*", "3.1.4") && req("1.2.*", "1.2.8"));
        assert!(req(">= 0.9.0, < 0.9.4", "0.9.3") && !req(">= 0.9.0, < 0.9.4", "0.9.4"));
        // Pre-releases match only a comparator on the same version with a pre-release.
        assert!(!req(">= 0.0.1", "0.0.2-alpha"));
        assert!(req(">= 0.0.2-alpha", "0.0.2-alpha") && req(">= 0.0.2-alpha", "0.0.2-beta") && !req(">= 0.0.2-beta", "0.0.2-alpha"));
        assert!(req(">= 1.0.0-rc.1", "1.0.0"));
        // Build metadata is ignored; nonsense does not parse.
        assert!(req("^0.25", "0.25.17+spec-1.1.0"));
        for bad in ["", ">= ", "1.2.3.4", "a.b", ">=1.*", "1.2-alpha"] {
            assert!(parse_req(bad).is_none(), "{bad:?}");
        }
    }

    const RSA: &str = "```toml\n[advisory]\nid = \"RUSTSEC-2023-0071\"\npackage = \"rsa\"\ndate = \"2023-11-22\"\nreferences = [\"https://a\",\n  \"https://b\"]\n\n[versions]\npatched = []\n```\n\n# Marvin Attack: potential key recovery through timing sidechannels\n";

    fn plant(dir: &Path, krate: &str, id: &str, body: &str) {
        let d = dir.join("crates").join(krate);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("{id}.md")), body).unwrap();
    }

    fn advisory(id: &str, krate: &str, extra: &str) -> String {
        format!("```toml\n[advisory]\nid = \"{id}\"\npackage = \"{krate}\"\n{extra}```\n\n# Planted {id}\n")
    }

    fn shipped(name: &str, version: &str) -> Shipped {
        Shipped {
            name: name.into(),
            version: version.into(),
            targets: ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin"].map(String::from).into(),
        }
    }

    #[test]
    fn advisories_parse_from_their_front_matter() {
        let a = parse_advisory(RSA).unwrap();
        assert_eq!((a.id.as_str(), a.package.as_str(), a.patched.len()), ("RUSTSEC-2023-0071", "rsa", 0));
        assert!(a.title.starts_with("Marvin Attack"));
        let b = parse_advisory(&advisory(
            "RUSTSEC-2099-0002",
            "foo",
            "informational = \"unmaintained\"\n\n[affected]\nos = [\"windows\"]\n\n[versions]\npatched = [\">= 1.2.0\", \"^0.9.7\"] # two\nunaffected = [\n  \"< 0.5\", # \"old\"\n]\n",
        ))
        .unwrap();
        assert_eq!(b.informational.as_deref(), Some("unmaintained"));
        assert_eq!(b.patched, [">= 1.2.0", "^0.9.7"]);
        assert_eq!(b.unaffected, ["< 0.5"]);
        assert!(applies_to(&b, "x86_64-pc-windows-msvc") && !applies_to(&b, "aarch64-apple-darwin"));
        assert!(affects(&b, "0.9.6").unwrap() && !affects(&b, "0.9.8").unwrap() && !affects(&b, "0.4.0").unwrap() && !affects(&b, "1.3.0").unwrap());
        assert!(parse_advisory("no front matter").is_err());
        assert!(parse_advisory("```toml\n[advisory]\nid = \"X\"\n```\n").is_err());
        let bad = parse_advisory(&advisory("RUSTSEC-2099-0003", "foo", "[versions]\npatched = [\">= banana\"]\n")).unwrap();
        assert!(affects(&bad, "1.0.0").is_err());
    }

    /// The real §8.4 records RUSTSEC-2023-0071 with every field; a fixture database with that
    /// advisory passes, and the same database with a planted unrecorded one fails naming it. Patched,
    /// unaffected, withdrawn, other-platform and informational advisories never fail.
    #[test]
    fn a_recorded_advisory_passes_and_a_planted_one_fails() {
        let security = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/security.md")).unwrap();
        let all = exceptions(&security).unwrap();
        assert!(all.iter().any(|e| e.id == "RUSTSEC-2026-0104"), "{all:?}");
        // The fixture database holds only the rsa advisory of those recorded.
        let recorded: Vec<Exception> = all.into_iter().filter(|e| e.id == "RUSTSEC-2023-0071").collect();
        let rsa = recorded.iter().find(|e| e.id == "RUSTSEC-2023-0071").expect("RUSTSEC-2023-0071 in security.md §8.4");
        assert!(!rsa.owner.is_empty() && !rsa.rationale.is_empty() && !rsa.control.is_empty() && rsa.review.len() == 10, "{rsa:?}");

        let dir = tempfile::tempdir().unwrap();
        plant(dir.path(), "rsa", "RUSTSEC-2023-0071", RSA);
        plant(dir.path(), "smallvec", "RUSTSEC-2099-0010", &advisory("RUSTSEC-2099-0010", "smallvec", "[versions]\npatched = [\">= 1.13.2\"]\n"));
        plant(
            dir.path(),
            "smallvec",
            "RUSTSEC-2099-0011",
            &advisory("RUSTSEC-2099-0011", "smallvec", "[versions]\npatched = []\nunaffected = [\">= 1.0\"]\n"),
        );
        plant(
            dir.path(),
            "smallvec",
            "RUSTSEC-2099-0012",
            &advisory("RUSTSEC-2099-0012", "smallvec", "withdrawn = 2099-01-01\n[versions]\npatched = []\n"),
        );
        plant(
            dir.path(),
            "smallvec",
            "RUSTSEC-2099-0013",
            &advisory("RUSTSEC-2099-0013", "smallvec", "[affected]\nos = [\"freebsd\"]\n[versions]\npatched = []\n"),
        );
        plant(
            dir.path(),
            "paste",
            "RUSTSEC-2099-0014",
            &advisory("RUSTSEC-2099-0014", "paste", "informational = \"unmaintained\"\n[versions]\npatched = []\n"),
        );
        let db = load_db(dir.path()).unwrap();
        assert_eq!(db.len(), 6);
        let graph = [shipped("rsa", "0.9.8"), shipped("smallvec", "1.15.1"), shipped("paste", "1.0.15")];
        let report = check(&graph, &db, &recorded, "2026-10-20");
        assert!(report.failures.is_empty(), "{report:?}");
        assert_eq!(report.recorded.len(), 1, "{report:?}");
        assert_eq!(report.warnings.len(), 1, "{report:?}");
        assert!(report.warnings[0].contains("RUSTSEC-2099-0014") && report.warnings[0].contains("unmaintained"));

        // The planted, unrecorded advisory on a shipped crate fails, naming its id and crate.
        plant(dir.path(), "smallvec", "RUSTSEC-2099-0001", &advisory("RUSTSEC-2099-0001", "smallvec", "[versions]\npatched = [\">= 2.0.0\"]\n"));
        let report = check(&graph, &load_db(dir.path()).unwrap(), &recorded, "2026-10-20");
        assert_eq!(report.failures.len(), 1, "{report:?}");
        assert!(report.failures[0].starts_with("RUSTSEC-2099-0001 smallvec 1.15.1"), "{}", report.failures[0]);

        // Without the record, RUSTSEC-2023-0071 fails too; past its review date it warns.
        assert_eq!(check(&graph, &db, &[], "2026-10-20").failures.len(), 1);
        let overdue = check(&graph, &db, &recorded, "2999-01-01");
        assert!(overdue.failures.is_empty() && overdue.warnings.iter().any(|w| w.contains("has passed")), "{overdue:?}");
        // A record nothing matches any more is a warning to remove it.
        assert!(check(&[shipped("rsa", "0.9.8")], &[], &recorded, "2026-10-20").warnings.iter().any(|w| w.contains("remove the entry")));
    }

    #[test]
    fn an_exception_needs_every_field() {
        let doc = |entry: &str| format!("### 8.4 Advisories\n\n- Known:\n  {entry}\n- Sources: crates.io.\n\n### 8.5 Checks\n");
        let full = "- **RUSTSEC-2024-0001, `x` 1** (why). *Rationale:* a. *Compensating control:* b. *Owner:* the lead.\n    *Review:* 2027-01-31.";
        let got = exceptions(&doc(full)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].review, "2027-01-31");
        assert_eq!(got[0].owner, "the lead");
        // Several ids in one head share its fields.
        let two = exceptions(&doc(&full.replace("RUSTSEC-2024-0001,", "RUSTSEC-2024-0001, RUSTSEC-2024-0002,"))).unwrap();
        assert_eq!(two.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["RUSTSEC-2024-0001", "RUSTSEC-2024-0002"]);
        assert_eq!(two[1].review, "2027-01-31");
        let no_date = full.replace("2027-01-31", "at every review");
        assert!(exceptions(&doc(&no_date)).unwrap_err()[0].contains("review date"));
        let no_owner = full.replace("*Owner:* the lead.", "");
        assert!(exceptions(&doc(&no_owner)).unwrap_err()[0].contains("owner"));
        assert!(exceptions("## 8. nothing").is_err());
    }

    #[test]
    fn today_is_a_date() {
        let t = today();
        assert!(first_date(&t).as_deref() == Some(t.as_str()) && t.as_str() > "2026-01-01", "{t}");
    }
}
