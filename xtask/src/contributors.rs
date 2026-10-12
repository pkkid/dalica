//! `cargo xtask contributors [--check] [--ref <rev>]`: the About window's credits
//! (`docs/contributors.md` → Refreshing).
//!
//! - Without `--check`: regenerates `contributors/contributors.json` from `git log` on `main` and,
//!   through `gh` (read-only), the GitHub username of each commit, the merged pull requests and,
//!   for people who asked for it (`sync_github_name`), their public profile name. Models are
//!   counted from `Co-Authored-By` trailers. Git author names and emails are never written: a
//!   commit is credited to a GitHub username or to nobody.
//! - `--check` (offline, a `cargo xtask ci` step): `contributors/people.toml` parses, has no
//!   unknown keys, and no entry was added by someone other than its owner without a
//!   `consent_source`.
//!
//! The JSON has the shape the Craft apps' `build.rs` reads (schema 1), so the About window can
//! follow PhotoCraft's pattern. Neither mode ever writes `people.toml`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use serde::Serialize;

pub const PEOPLE: &str = "contributors/people.toml";
pub const JSON: &str = "contributors/contributors.json";
const USAGE: &str = "usage: cargo xtask contributors [--check] [--ref <rev>]";

// ---------------------------------------------------------------------------------------------
// people.toml
// ---------------------------------------------------------------------------------------------

/// One `[people.<username>]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub login: String,
    /// 1-based line of its table header.
    pub line: usize,
    pub real_name: Option<String>,
    pub display_name: Option<String>,
    pub sync_github_name: bool,
    pub consent_source: Option<String>,
}

/// A TOML value of the kinds people.toml uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Str(String),
    Bool(bool),
}

/// The keys an entry may have, and whether each is a string (else a boolean).
const KEYS: &[(&str, bool)] = &[("real_name", true), ("display_name", true), ("sync_github_name", false), ("consent_source", true)];

/// A GitHub username: 1–39 ASCII letters, digits and hyphens, not starting or ending with one.
pub fn valid_login(s: &str) -> bool {
    (1..=39).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !s.starts_with('-') && !s.ends_with('-')
}

struct Cursor<'a> {
    s: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a str {
        self.s.get(self.at..).unwrap_or("")
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at = self.at.saturating_add(c.len_utf8());
        Some(c)
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }
    /// Only whitespace or a comment is left.
    fn end(&mut self) -> Result<(), String> {
        self.skip_ws();
        match self.peek() {
            None | Some('#') => Ok(()),
            Some(c) => Err(format!("unexpected `{c}`")),
        }
    }
    fn bare_key(&mut self) -> Option<&'a str> {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            self.bump();
        }
        self.s.get(start..self.at).filter(|k| !k.is_empty())
    }
    /// A basic (`"…"`) or literal (`'…'`) string on one line; the cursor is on its quote.
    fn string(&mut self) -> Result<String, String> {
        let quote = self.bump().unwrap_or('"');
        if self.rest().starts_with(&String::from_iter([quote, quote])) {
            return Err("multi-line strings are not used in people.toml".into());
        }
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err("unterminated string".into()),
                Some(c) if c == quote => return Ok(out),
                Some('\\') if quote == '"' => out.push(self.escape()?),
                Some(c) if c.is_control() && c != '\t' => return Err("control character in a string".into()),
                Some(c) => out.push(c),
            }
        }
    }
    fn escape(&mut self) -> Result<char, String> {
        let hex = |cur: &mut Self, n: usize| -> Result<char, String> {
            let digits: String = (0..n).filter_map(|_| cur.bump()).collect();
            u32::from_str_radix(&digits, 16)
                .ok()
                .filter(|_| digits.len() == n)
                .and_then(char::from_u32)
                .ok_or_else(|| format!("bad unicode escape `{digits}`"))
        };
        match self.bump() {
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('b') => Ok('\u{8}'),
            Some('t') => Ok('\t'),
            Some('n') => Ok('\n'),
            Some('f') => Ok('\u{c}'),
            Some('r') => Ok('\r'),
            Some('e') => Ok('\u{1b}'),
            Some('u') => hex(self, 4),
            Some('U') => hex(self, 8),
            Some(c) => Err(format!("unknown escape `\\{c}`")),
            None => Err("unterminated string".into()),
        }
    }
    /// A table-name segment: a bare key or a quoted one.
    fn segment(&mut self) -> Result<String, String> {
        match self.peek() {
            Some('"' | '\'') => self.string(),
            _ => self.bare_key().map(str::to_string).ok_or_else(|| "expected a name".to_string()),
        }
    }
}

enum Line {
    Blank,
    Table(Vec<String>),
    Pair(String, Value),
}

fn parse_line(line: &str) -> Result<Line, String> {
    let mut c = Cursor { s: line, at: 0 };
    c.skip_ws();
    match c.peek() {
        None | Some('#') => Ok(Line::Blank),
        Some('[') => {
            c.bump();
            if c.peek() == Some('[') {
                return Err("arrays of tables are not used in people.toml".into());
            }
            let mut segments = Vec::new();
            loop {
                c.skip_ws();
                segments.push(c.segment()?);
                c.skip_ws();
                match c.bump() {
                    Some('.') => {}
                    Some(']') => break,
                    _ => return Err("expected `.` or `]` in the table header".into()),
                }
            }
            c.end()?;
            Ok(Line::Table(segments))
        }
        Some(_) => {
            let key = match c.peek() {
                Some('"' | '\'') => return Err("quoted keys are not used in people.toml".into()),
                _ => c.bare_key().ok_or("expected `key = value`")?.to_string(),
            };
            c.skip_ws();
            match c.bump() {
                Some('=') => {}
                Some('.') => return Err("dotted keys are not used in people.toml".into()),
                _ => return Err(format!("expected `=` after `{key}`")),
            }
            c.skip_ws();
            let value = match c.peek() {
                Some('"' | '\'') => Value::Str(c.string()?),
                _ => match c.bare_key() {
                    Some("true") => Value::Bool(true),
                    Some("false") => Value::Bool(false),
                    _ => return Err(format!("`{key}` takes a string or true/false")),
                },
            };
            c.end()?;
            Ok(Line::Pair(key, value))
        }
    }
}

/// Parses people.toml. It is TOML restricted to what the file's template uses: `[people.<username>]`
/// tables of the keys in [`KEYS`], each a one-line string or `true`/`false`, and comments. Anything
/// else is an error with its line, all of them reported at once.
pub fn parse_people(text: &str) -> Result<Vec<Entry>, Vec<String>> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut errors = Vec::new();
    // `None` before any header (top level) or under `[people]`; `Some(i)` inside entries[i];
    // `Err(())` inside a table that was refused (its keys are not reported again).
    let mut current: Result<Option<usize>, ()> = Ok(None);
    let mut seen_keys: HashSet<String> = HashSet::new();
    for (i, line) in text.lines().enumerate() {
        let n = i.saturating_add(1);
        let mut fail = |msg: String| errors.push(format!("{PEOPLE}:{n}: {msg}"));
        match parse_line(line) {
            Err(e) => fail(format!("malformed TOML: {e}")),
            Ok(Line::Blank) => {}
            Ok(Line::Table(segments)) => {
                seen_keys.clear();
                current = Err(());
                match segments.as_slice() {
                    [p] if p == "people" => current = Ok(None),
                    [p, login] if p == "people" => {
                        if !valid_login(login) {
                            fail(format!("`{login}` is not a GitHub username"));
                        } else if let Some(e) = entries.iter().find(|e| e.login.eq_ignore_ascii_case(login)) {
                            fail(format!("[people.{login}] repeats the entry on line {}", e.line));
                        } else {
                            entries.push(Entry { login: login.clone(), line: n, ..Entry::default() });
                            current = Ok(Some(entries.len().saturating_sub(1)));
                        }
                    }
                    _ => fail(format!("unknown table [{}]: entries are [people.<username>]", segments.join("."))),
                }
            }
            Ok(Line::Pair(key, value)) => {
                let index = match current {
                    Err(()) => continue,
                    Ok(None) => {
                        fail(format!("unknown key `{key}` outside a [people.<username>] table"));
                        continue;
                    }
                    Ok(Some(index)) => index,
                };
                let Some(&(_, is_str)) = KEYS.iter().find(|(k, _)| *k == key) else {
                    let known: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
                    fail(format!("unknown key `{key}` (known: {})", known.join(", ")));
                    continue;
                };
                if !seen_keys.insert(key.clone()) {
                    fail(format!("`{key}` is set twice"));
                    continue;
                }
                let Some(entry) = entries.get_mut(index) else { continue };
                match (value, is_str) {
                    (Value::Str(s), true) => {
                        let s = Some(s);
                        match key.as_str() {
                            "real_name" => entry.real_name = s,
                            "display_name" => entry.display_name = s,
                            _ => entry.consent_source = s,
                        }
                    }
                    (Value::Bool(b), false) => entry.sync_github_name = b,
                    (_, true) => fail(format!("`{key}` takes a string")),
                    (_, false) => fail(format!("`{key}` takes true or false")),
                }
            }
        }
    }
    if errors.is_empty() { Ok(entries) } else { Err(errors) }
}

// ---------------------------------------------------------------------------------------------
// Who added each entry
// ---------------------------------------------------------------------------------------------

/// How an entry's ownership check came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// The commit that added it is by its own user.
    Verified,
    /// It has a `consent_source`.
    Consent,
    /// It could not be told here (why); the entry's names stay hidden until it can.
    NotChecked(String),
    /// Added by someone else without a `consent_source`.
    Refused(String),
}

impl Owner {
    pub fn shown(&self) -> bool {
        matches!(self, Owner::Verified | Owner::Consent)
    }
}

/// The GitHub username in a GitHub no-reply address (`123+login@users.noreply.github.com` or
/// `login@users.noreply.github.com`); the address itself is never kept.
pub fn noreply_login(email: &str) -> Option<String> {
    let local = email.trim().strip_suffix("@users.noreply.github.com")?;
    let login = local.split_once('+').map_or(local, |(id, l)| if id.bytes().all(|b| b.is_ascii_digit()) { l } else { local });
    valid_login(login).then(|| login.to_string())
}

/// For each entry in the current people.toml of `rev`'s history: the commit that last added it
/// (it was absent from the file before) and, when known, that commit's GitHub username, from
/// `logins` (sha → username) or else its author's no-reply address.
pub fn entry_adders(root: &Path, rev: &str, logins: &HashMap<String, String>) -> Result<HashMap<String, (String, Option<String>)>, String> {
    let log = git(root, &["log", "--reverse", "--format=%H%x1f%ae", rev, "--", PEOPLE])?;
    let mut present: HashSet<String> = HashSet::new();
    let mut adders: HashMap<String, (String, Option<String>)> = HashMap::new();
    for line in log.lines() {
        let Some((sha, email)) = line.split_once('\x1f') else { continue };
        // A version that does not parse keeps the previous set, so its fix is not taken for an add.
        let text = git(root, &["show", &format!("{sha}:{PEOPLE}")]).unwrap_or_default();
        let Ok(entries) = parse_people(&text) else { continue };
        let now: HashSet<String> = entries.iter().map(|e| e.login.to_ascii_lowercase()).collect();
        for login in now.difference(&present) {
            let by = logins.get(sha).cloned().filter(|l| !l.is_empty()).or_else(|| noreply_login(email));
            adders.insert(login.clone(), (sha.to_string(), by));
        }
        adders.retain(|l, _| now.contains(l));
        present = now;
    }
    Ok(adders)
}

/// Each entry's [`Owner`]. `adders` is [`entry_adders`] (or why it could not be read).
pub fn verify(entries: &[Entry], adders: &Result<HashMap<String, (String, Option<String>)>, String>, rev: &str) -> Vec<Owner> {
    entries
        .iter()
        .map(|e| {
            if e.consent_source.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                return Owner::Consent;
            }
            let adders = match adders {
                Ok(a) => a,
                Err(why) => return Owner::NotChecked(why.clone()),
            };
            match adders.get(&e.login.to_ascii_lowercase()) {
                None => Owner::NotChecked(format!("not committed on {rev} yet")),
                Some((sha, None)) => Owner::NotChecked(format!("commit {} is not mapped to a GitHub username here", short(sha))),
                Some((_, Some(by))) if by.eq_ignore_ascii_case(&e.login) => Owner::Verified,
                Some((sha, Some(by))) => Owner::Refused(format!(
                    "{PEOPLE}:{}: [people.{}] was added by @{by} in {}, not by its owner, and has no consent_source",
                    e.line,
                    e.login,
                    short(sha)
                )),
            }
        })
        .collect()
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// Whether the repository at `root` is a shallow clone (its history cannot say who added what).
fn shallow(root: &Path) -> Result<bool, String> {
    git(root, &["rev-parse", "--is-shallow-repository"]).map(|s| s.trim() == "true")
}

/// `--check`: people.toml, offline.
pub fn check(root: &Path) -> Result<(), String> {
    let path = root.join(PEOPLE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("contributors: cannot read {PEOPLE}: {e}"))?;
    let entries = parse_people(&text).map_err(|errors| {
        for e in &errors {
            eprintln!("contributors: {e}");
        }
        format!("contributors: {PEOPLE}: {} problem(s)", errors.len())
    })?;
    let adders = match shallow(root) {
        Ok(false) => entry_adders(root, "HEAD", &HashMap::new()),
        Ok(true) => Err("shallow clone".to_string()),
        Err(e) => Err(e),
    };
    let owners = verify(&entries, &adders, "HEAD");
    let mut refused = 0usize;
    for (e, o) in entries.iter().zip(&owners) {
        match o {
            Owner::Refused(msg) => {
                eprintln!("contributors: {msg}");
                refused = refused.saturating_add(1);
            }
            Owner::NotChecked(why) => println!("contributors: [people.{}]: owner not checked offline ({why}); the full run checks it", e.login),
            Owner::Verified | Owner::Consent => {}
        }
    }
    if refused > 0 {
        return Err(format!("contributors: {refused} entr{} added by someone else without consent_source", if refused == 1 { "y" } else { "ies" }));
    }
    println!("contributors: {PEOPLE} ok ({} entr{})", entries.len(), if entries.len() == 1 { "y" } else { "ies" });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// git log
// ---------------------------------------------------------------------------------------------

/// One non-merge commit's numbers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitStat {
    pub sha: String,
    /// Committer time, Unix seconds.
    pub time: i64,
    /// `Co-Authored-By` trailer values.
    pub coauthors: Vec<String>,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_added: u64,
    pub binary_deleted: u64,
}

/// The `git log` format [`parse_log`] reads: a record per commit (`\x1e`), its fields (`\x1f`),
/// the trailers joined by `\x1d`, then the `--raw` and `--numstat` lines.
const LOG_FORMAT: &str = "--format=%x1e%H%x1f%ct%x1f%(trailers:key=Co-authored-by,valueonly,separator=%x1d)%x1f";

pub fn parse_log(out: &str) -> Result<Vec<CommitStat>, String> {
    let mut commits = Vec::new();
    for record in out.split('\x1e').filter(|r| !r.trim().is_empty()) {
        let mut fields = record.splitn(4, '\x1f');
        let sha = fields.next().unwrap_or("").trim().to_string();
        let time = fields.next().unwrap_or("").trim();
        let time: i64 = time.parse().map_err(|_| format!("git log: bad commit time `{time}` for {sha}"))?;
        let coauthors = fields.next().unwrap_or("").split(['\x1d', '\n']).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
        let diff = fields.next().unwrap_or("");
        let mut status: HashMap<&str, char> = HashMap::new();
        let mut numstat = Vec::new();
        for line in diff.lines() {
            if let Some(raw) = line.strip_prefix(':') {
                // `:100644 000000 <sha> <sha> D\tpath`
                if let Some((meta, path)) = raw.split_once('\t')
                    && let Some(s) = meta.split_whitespace().nth(4).and_then(|s| s.chars().next())
                {
                    status.insert(path, s);
                }
            } else if let Some((a, rest)) = line.split_once('\t')
                && let Some((d, path)) = rest.split_once('\t')
            {
                numstat.push((a, d, path));
            }
        }
        let mut c = CommitStat { sha, time, coauthors, ..CommitStat::default() };
        for (a, d, path) in numstat {
            if a == "-" && d == "-" {
                match status.get(path) {
                    Some('A') => c.binary_added = c.binary_added.saturating_add(1),
                    Some('D') => c.binary_deleted = c.binary_deleted.saturating_add(1),
                    _ => {}
                }
            } else {
                c.lines_added = c.lines_added.saturating_add(a.parse().unwrap_or(0));
                c.lines_deleted = c.lines_deleted.saturating_add(d.parse().unwrap_or(0));
            }
        }
        commits.push(c);
    }
    Ok(commits)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    // Settings that would change the output are pinned, whatever the user's git config says.
    let pinned = ["-c", "core.quotePath=false", "-c", "log.showSignature=false", "-c", "color.ui=false"];
    let out = Command::new("git").current_dir(root).args(pinned).args(args).output().map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// `ts` (Unix seconds) as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc(ts: i64) -> String {
    // Days to a civil date (H. Hinnant's algorithm); the ranges keep every step far from overflow.
    let ts = ts.clamp(-62_135_596_800, 253_402_300_799); // years 1 to 9999
    let (days, secs) = (ts.div_euclid(86_400), ts.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

// ---------------------------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------------------------

/// The model vendors a `Co-Authored-By` trailer is credited to: the name's first word (or its
/// prefix before a `-`), the address's domain, the company. A trailer that matches none, a person
/// co-author among them, is not counted and not recorded.
const VENDORS: &[(&str, &str, &str)] = &[
    ("Claude", "anthropic.com", "Anthropic"),
    ("Copilot", "users.noreply.github.com", "GitHub"),
    ("Codex", "openai.com", "OpenAI"),
    ("ChatGPT", "openai.com", "OpenAI"),
    ("GPT", "openai.com", "OpenAI"),
    ("Gemini", "google.com", "Google"),
    ("Cursor", "cursor.com", "Cursor"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelId {
    pub id: String,
    pub company: String,
    pub model: String,
    pub version: String,
}

/// `Claude Opus 5.5 <noreply@anthropic.com>` → Anthropic, `Claude Opus`, `5.5`,
/// id `anthropic/claude-opus/5.5`. A trailing `(…)` note is left out.
pub fn model_of(trailer: &str) -> Option<ModelId> {
    let (name, rest) = trailer.split_once('<')?;
    let domain = rest.split_once('>')?.0.rsplit_once('@')?.1.trim().to_ascii_lowercase();
    let mut name = name.trim();
    if name.ends_with(')')
        && let Some((head, _)) = name.rsplit_once(" (")
    {
        name = head.trim();
    }
    if name.is_empty() || name.contains(['@', '<', '>']) {
        return None;
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = words.first()?;
    let (_, _, company) = VENDORS.iter().find(|(prefix, d, _)| {
        let word = first.split_once('-').map_or(*first, |(p, _)| p);
        word.eq_ignore_ascii_case(prefix) && (domain == *d || domain.ends_with(&format!(".{d}")))
    })?;
    let version_like = |w: &&str| w.starts_with(|c: char| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    let (model, version) = match words.split_last() {
        Some((last, head)) if !head.is_empty() && version_like(last) => (head.join(" "), (*last).to_string()),
        _ => (words.join(" "), String::new()),
    };
    let slug = |s: &str| s.to_ascii_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
    let id = if version.is_empty() { format!("{}/{}", slug(company), slug(&model)) } else { format!("{}/{}/{version}", slug(company), slug(&model)) };
    Some(ModelId { id, company: (*company).to_string(), model, version })
}

// ---------------------------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------------------------

/// What the refresh asks GitHub (the real one runs `gh`, read-only; tests pass a fake).
pub trait GitHub {
    /// `owner/name` of the repository.
    fn repo(&self) -> Result<String, String>;
    /// `(sha, username)` for each commit reachable from `head`; the username is empty when GitHub
    /// links the commit to no account.
    fn commit_logins(&self, repo: &str, head: &str) -> Result<Vec<(String, String)>, String>;
    /// `(merge commit sha, author username)` for each pull request merged into `base`.
    fn merged_prs(&self, repo: &str, base: &str) -> Result<Vec<(String, String)>, String>;
    /// The name on a user's public profile, if they set one.
    fn profile_name(&self, login: &str) -> Result<Option<String>, String>;
}

/// [`GitHub`] through the `gh` command. Each query's `--jq` keeps only shas, usernames and the
/// profile name, so no author name or email reaches this process.
pub struct GhCli<'a> {
    pub root: &'a Path,
}

impl GhCli<'_> {
    fn gh(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("gh")
            .current_dir(self.root)
            .args(args)
            .output()
            .map_err(|e| format!("cannot run gh (install the GitHub CLI and run `gh auth login`): {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!("gh {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
        }
    }
}

/// `sha username` lines (the username may be missing).
pub fn parse_pairs(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let (sha, login) = l.trim().split_once(' ').unwrap_or((l.trim(), ""));
            (!sha.is_empty()).then(|| (sha.to_string(), login.trim().to_string()))
        })
        .collect()
}

impl GitHub for GhCli<'_> {
    fn repo(&self) -> Result<String, String> {
        Ok(self.gh(&["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])?.trim().to_string())
    }
    fn commit_logins(&self, repo: &str, head: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("repos/{repo}/commits?sha={head}&per_page=100");
        Ok(parse_pairs(&self.gh(&["api", "--paginate", &url, "--jq", r#".[] | "\(.sha) \(.author.login // "")""#])?))
    }
    fn merged_prs(&self, repo: &str, base: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("repos/{repo}/pulls?state=closed&base={base}&per_page=100");
        let jq = r#".[] | select(.merged_at != null) | "\(.merge_commit_sha // "") \(.user.login // "")""#;
        Ok(parse_pairs(&self.gh(&["api", "--paginate", &url, "--jq", jq])?))
    }
    fn profile_name(&self, login: &str) -> Result<Option<String>, String> {
        let name = self.gh(&["api", &format!("users/{login}"), "--jq", r#".name // """#])?;
        Ok(Some(name.trim().to_string()).filter(|n| !n.is_empty()))
    }
}

// ---------------------------------------------------------------------------------------------
// contributors.json
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub schema: u32,
    pub repo: String,
    pub branch: String,
    pub head: String,
    pub head_date: String,
    pub commits: u64,
    pub unattributed_commits: u64,
    pub contributors: Vec<ContributorRow>,
    pub models: Vec<ModelRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContributorRow {
    pub login: String,
    /// Their people.toml entry passed the ownership check, so its names may be shown.
    pub people_entry: bool,
    /// Their public profile name, fetched only when their verified entry asks for it.
    pub github_name: Option<String>,
    pub prs: u64,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_added: u64,
    pub binary_deleted: u64,
    pub first_commit: Option<String>,
    pub last_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelRow {
    pub id: String,
    pub company: String,
    pub model: String,
    pub version: String,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
}

/// Bots are not people: their commits count as unattributed and their pull requests not at all.
fn person(login: &str) -> Option<&str> {
    Some(login.trim()).filter(|l| !l.is_empty() && !l.ends_with("[bot]") && !l.starts_with("app/"))
}

/// The ref to read: `--ref`, else `origin/main` when it exists (what GitHub knows), else `main`.
fn default_ref(root: &Path) -> String {
    if git(root, &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"]).is_ok() { "origin/main".into() } else { "main".into() }
}

/// Builds the report for `rev` (and the notes to print); fails on a people.toml problem.
pub fn refresh(root: &Path, gh: &dyn GitHub, rev: &str) -> Result<(Report, Vec<String>), String> {
    let text = std::fs::read_to_string(root.join(PEOPLE)).map_err(|e| format!("cannot read {PEOPLE}: {e}"))?;
    let entries = parse_people(&text).map_err(|errors| errors.join("\n"))?;

    let head = git(root, &["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?.trim().to_string();
    let head_time: i64 = git(root, &["log", "-1", "--format=%ct", &head])?.trim().parse().map_err(|_| "git log: bad head time".to_string())?;
    let commits = parse_log(&git(root, &["log", &head, "--no-merges", "--no-renames", "--no-ext-diff", "--raw", "--numstat", LOG_FORMAT])?)?;
    let history: HashSet<String> = git(root, &["rev-list", &head])?.lines().map(str::to_string).collect();

    let repo = gh.repo()?;
    let branch = rev.strip_prefix("origin/").unwrap_or(rev).to_string();
    let logins: HashMap<String, String> = gh.commit_logins(&repo, &head)?.into_iter().collect();
    let prs = gh.merged_prs(&repo, &branch)?;
    let pr_author: HashMap<&str, &str> = prs.iter().map(|(sha, login)| (sha.as_str(), login.as_str())).collect();

    // Who added each people.toml entry.
    let adders = if shallow(root)? { Err("shallow clone".to_string()) } else { entry_adders(root, &head, &logins) };
    let owners = verify(&entries, &adders, &branch);
    let refused: Vec<String> = owners.iter().filter_map(|o| if let Owner::Refused(m) = o { Some(m.clone()) } else { None }).collect();
    if !refused.is_empty() {
        return Err(refused.join("\n"));
    }
    let mut notes = Vec::new();
    for (e, o) in entries.iter().zip(&owners) {
        if let Owner::NotChecked(why) = o {
            notes.push(format!("[people.{}]: names left out until its owner can be checked ({why})", e.login));
        }
    }

    // Per username (case-insensitive; the first spelling seen is kept).
    let mut rows: Vec<ContributorRow> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut row_for = |login: &str, rows: &mut Vec<ContributorRow>| -> usize {
        *index.entry(login.to_ascii_lowercase()).or_insert_with(|| {
            rows.push(ContributorRow {
                login: login.to_string(),
                people_entry: false,
                github_name: None,
                prs: 0,
                commits: 0,
                lines_added: 0,
                lines_deleted: 0,
                binary_added: 0,
                binary_deleted: 0,
                first_commit: None,
                last_commit: None,
            });
            rows.len().saturating_sub(1)
        })
    };
    let mut unattributed = 0u64;
    let mut times: HashMap<usize, (i64, i64)> = HashMap::new();
    let mut models: Vec<ModelRow> = Vec::new();
    for c in &commits {
        let login = logins.get(&c.sha).and_then(|l| person(l)).or_else(|| pr_author.get(c.sha.as_str()).and_then(|l| person(l)));
        match login {
            None => unattributed = unattributed.saturating_add(1),
            Some(login) => {
                let i = row_for(login, &mut rows);
                if let Some(r) = rows.get_mut(i) {
                    r.commits = r.commits.saturating_add(1);
                    r.lines_added = r.lines_added.saturating_add(c.lines_added);
                    r.lines_deleted = r.lines_deleted.saturating_add(c.lines_deleted);
                    r.binary_added = r.binary_added.saturating_add(c.binary_added);
                    r.binary_deleted = r.binary_deleted.saturating_add(c.binary_deleted);
                }
                let t = times.entry(i).or_insert((c.time, c.time));
                *t = (t.0.min(c.time), t.1.max(c.time));
            }
        }
        let mut ids: Vec<ModelId> = c.coauthors.iter().filter_map(|t| model_of(t)).collect();
        ids.sort_by(|a, b| a.id.cmp(&b.id));
        ids.dedup_by(|a, b| a.id == b.id);
        for m in ids {
            let row = match models.iter().position(|r| r.id == m.id) {
                Some(p) => models.get_mut(p),
                None => {
                    models.push(ModelRow {
                        id: m.id,
                        company: m.company,
                        model: m.model,
                        version: m.version,
                        commits: 0,
                        lines_added: 0,
                        lines_deleted: 0,
                    });
                    models.last_mut()
                }
            };
            if let Some(r) = row {
                r.commits = r.commits.saturating_add(1);
                r.lines_added = r.lines_added.saturating_add(c.lines_added);
                r.lines_deleted = r.lines_deleted.saturating_add(c.lines_deleted);
            }
        }
    }
    for (sha, login) in &prs {
        if let Some(login) = person(login)
            && history.contains(sha)
        {
            let i = row_for(login, &mut rows);
            if let Some(r) = rows.get_mut(i) {
                r.prs = r.prs.saturating_add(1);
            }
        }
    }
    for (i, (first, last)) in &times {
        if let Some(r) = rows.get_mut(*i) {
            r.first_commit = Some(utc(*first));
            r.last_commit = Some(utc(*last));
        }
    }
    for (e, o) in entries.iter().zip(&owners) {
        let Some(r) = rows.iter_mut().find(|r| r.login.eq_ignore_ascii_case(&e.login)) else { continue };
        r.people_entry = o.shown();
        if o.shown() && e.sync_github_name {
            r.github_name = gh.profile_name(&r.login)?;
        }
    }
    rows.sort_by(|a, b| {
        b.commits.cmp(&a.commits).then(b.prs.cmp(&a.prs)).then_with(|| a.login.to_ascii_lowercase().cmp(&b.login.to_ascii_lowercase()))
    });
    models.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.id.cmp(&b.id)));
    let report = Report {
        schema: 1,
        repo,
        branch,
        head,
        head_date: utc(head_time),
        commits: u64::try_from(commits.len()).unwrap_or(u64::MAX),
        unattributed_commits: unattributed,
        contributors: rows,
        models,
    };
    Ok((report, notes))
}

pub fn to_json(report: &Report) -> Result<String, String> {
    serde_json::to_string_pretty(report).map(|s| s + "\n").map_err(|e| e.to_string())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut check_only = false;
    let mut rev = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check_only = true,
            "--ref" => rev = Some(it.next().ok_or(USAGE)?.clone()),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("contributors: unknown argument `{other}`\n{USAGE}")),
        }
    }
    if check_only {
        return check(root);
    }
    check(root)?;
    let rev = rev.unwrap_or_else(|| default_ref(root));
    let (report, notes) = refresh(root, &GhCli { root }, &rev).map_err(|e| format!("contributors: {e}"))?;
    for n in &notes {
        println!("contributors: {n}");
    }
    std::fs::write(root.join(JSON), to_json(&report)?).map_err(|e| format!("contributors: cannot write {JSON}: {e}"))?;
    println!(
        "contributors: wrote {JSON}: {} commits on {} at {} ({} unattributed), {} contributor(s), {} model(s)",
        report.commits,
        report.branch,
        short(&report.head),
        report.unattributed_commits,
        report.contributors.len(),
        report.models.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- people.toml ---------------------------------------------------------------------------

    #[test]
    fn the_template_and_full_entries_parse() {
        let text = "# header\n\n[people]\n\n[people.ada-l] # mine\nreal_name = \"Ada \\\"L\\\" \\u00e9\"\ndisplay_name = 'Ada'\nsync_github_name = true\n\n\
                    [people.\"Bob2\"]\nconsent_source = \"https://github.com/o/r/issues/1#issuecomment-2\"\n";
        let e = parse_people(text).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0],
            Entry {
                login: "ada-l".into(),
                line: 5,
                real_name: Some("Ada \"L\" é".into()),
                display_name: Some("Ada".into()),
                sync_github_name: true,
                consent_source: None
            }
        );
        assert_eq!(e[1].login, "Bob2");
        assert_eq!(e[1].consent_source.as_deref(), Some("https://github.com/o/r/issues/1#issuecomment-2"));
    }

    #[test]
    fn the_real_people_toml_passes_the_check() {
        let root = crate::workspace_root();
        let text = std::fs::read_to_string(root.join(PEOPLE)).unwrap();
        assert!(parse_people(&text).is_ok());
        check(&root).unwrap();
    }

    /// Each planted fault is refused, with its line.
    #[test]
    fn check_refuses_each_planted_fault() {
        let cases: &[(&str, &str)] = &[
            ("[people.ada]\nnickname = \"A\"\n", ":2: unknown key `nickname`"),
            ("real_name = \"A\"\n", ":1: unknown key `real_name` outside"),
            ("[people]\nada = \"A\"\n", ":2: unknown key `ada` outside"),
            ("[friends.ada]\n", ":1: unknown table [friends.ada]"),
            ("[people.ada.extra]\n", ":1: unknown table"),
            ("[people.ada]\nreal_name = \"A\n", ":2: malformed TOML: unterminated string"),
            ("[people.ada\n", ":1: malformed TOML"),
            ("[people.ada]\nreal_name \"A\"\n", ":2: malformed TOML: expected `=`"),
            ("[people.ada]\nreal_name = A\n", ":2: malformed TOML"),
            ("[people.ada]\nreal_name = \"A\" x\n", ":2: malformed TOML: unexpected `x`"),
            ("[people.ada]\nreal_name = \"\\q\"\n", ":2: malformed TOML: unknown escape"),
            ("[people.ada]\nreal_name = \"\"\"A\"\"\"\n", ":2: malformed TOML: multi-line"),
            ("[[people.ada]]\n", ":1: malformed TOML: arrays of tables"),
            ("[people.ada]\nname.real = \"A\"\n", ":2: malformed TOML: dotted keys"),
            ("people.ada = { real_name = \"A\" }\n", ":1: malformed TOML: dotted keys"),
            ("[people.ada]\nreal_name = true\n", ":2: `real_name` takes a string"),
            ("[people.ada]\nsync_github_name = \"yes\"\n", ":2: `sync_github_name` takes true or false"),
            ("[people.ada]\nreal_name = \"A\"\nreal_name = \"B\"\n", ":3: `real_name` is set twice"),
            ("[people.ada]\n[people.ADA]\n", ":2: [people.ADA] repeats the entry on line 1"),
            ("[people.-ada]\n", ":1: `-ada` is not a GitHub username"),
            ("[people.\"a b\"]\n", ":1: `a b` is not a GitHub username"),
        ];
        for (text, want) in cases {
            let errors = parse_people(text).expect_err(text);
            assert!(errors.iter().any(|e| e.starts_with(PEOPLE) && e.contains(want)), "{text:?}: {errors:?} lacks {want:?}");
        }
        // All problems at once.
        assert_eq!(parse_people("[people.ada]\nx = \"1\"\ny = \"2\"\n").unwrap_err().len(), 2);
    }

    #[test]
    fn noreply_addresses_give_their_username() {
        assert_eq!(noreply_login("123+ada-l@users.noreply.github.com").as_deref(), Some("ada-l"));
        assert_eq!(noreply_login("ada@users.noreply.github.com").as_deref(), Some("ada"));
        assert_eq!(noreply_login("ada@example.org"), None);
        assert_eq!(noreply_login("a+b+c@users.noreply.github.com"), None);
    }

    // --- parsing helpers --------------------------------------------------------------------

    #[test]
    fn log_records_count_lines_and_binary_files() {
        let out = "\x1eaaa\x1f1791818139\x1f\x1f\n\n:100644 000000 8352675 0000000 D\tb.png\n-\t-\tb.png\n\
                   \x1ebbb\x1f1700000000\x1fClaude Opus 5.5 <noreply@anthropic.com>\x1dHuman <h@e.net>\x1f\n\n\
                   :000000 100644 0000000 422c2b7 A\ta.txt\n:000000 100644 0000000 8352675 A\tb.png\n:100644 100644 1 2 M\tc.png\n\
                   2\t0\ta.txt\n-\t-\tb.png\n-\t-\tc.png\n7\t3\tsub dir/d e.txt\n";
        let c = parse_log(out).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].sha.as_str(), c[0].binary_deleted, c[0].binary_added, c[0].lines_added), ("aaa", 1, 0, 0));
        assert_eq!(c[1].coauthors, ["Claude Opus 5.5 <noreply@anthropic.com>", "Human <h@e.net>"]);
        assert_eq!((c[1].lines_added, c[1].lines_deleted, c[1].binary_added, c[1].binary_deleted), (9, 3, 1, 0));
        assert!(parse_log("\x1eaaa\x1fnot-a-time\x1f\x1f").is_err());
        assert_eq!(parse_log("").unwrap(), []);
    }

    #[test]
    fn models_come_from_known_vendors_only() {
        let m = model_of("Claude Opus 5.5 <noreply@anthropic.com>").unwrap();
        assert_eq!(
            (m.id.as_str(), m.company.as_str(), m.model.as_str(), m.version.as_str()),
            ("anthropic/claude-opus/5.5", "Anthropic", "Claude Opus", "5.5")
        );
        assert_eq!(model_of("Claude <noreply@anthropic.com>").unwrap().id, "anthropic/claude");
        assert_eq!(model_of("Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>").unwrap().id, "anthropic/claude-sonnet/4.6");
        assert_eq!(model_of("Copilot <198982749+Copilot@users.noreply.github.com>").unwrap().id, "github/copilot");
        assert_eq!(model_of("GPT-5 <noreply@openai.com>").unwrap().id, "openai/gpt-5");
        // A person, or a model name at the wrong domain, is not a model.
        assert_eq!(model_of("Claude Monet <claude@example.org>"), None);
        assert_eq!(model_of("Ada Lovelace <ada@anthropic.com>"), None);
        assert_eq!(model_of("Human Helper <helper@example.net>"), None);
        assert_eq!(model_of("Claude a@b <noreply@anthropic.com>"), None);
        assert_eq!(model_of("no address"), None);
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_791_818_139), "2026-10-12T15:15:39Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(-1), "1969-12-31T23:59:59Z");
        assert_eq!(utc(i64::MAX), "9999-12-31T23:59:59Z");
        assert_eq!(utc(i64::MIN), "0001-01-01T00:00:00Z");
    }

    #[test]
    fn gh_pairs_parse() {
        assert_eq!(
            parse_pairs("abc ada\ndef \n\n ghi bob \n"),
            [("abc".into(), "ada".into()), ("def".into(), String::new()), ("ghi".into(), "bob".into())]
        );
    }

    // --- a temporary repository and a fake gh --------------------------------------------------

    struct Repo {
        dir: tempfile::TempDir,
        config: PathBuf,
        clock: i64,
    }

    impl Repo {
        fn new() -> Repo {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("gitconfig");
            std::fs::write(&config, "").unwrap();
            let r = Repo { dir, config, clock: 1_790_000_000 };
            std::fs::create_dir_all(r.path().join("work/contributors")).unwrap();
            r.git(&["-c", "init.defaultBranch=main", "init", "-q"]);
            r
        }
        fn path(&self) -> PathBuf {
            self.dir.path().to_path_buf()
        }
        fn root(&self) -> PathBuf {
            self.path().join("work")
        }
        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .current_dir(self.root())
                .env("GIT_CONFIG_GLOBAL", &self.config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        fn write(&self, rel: &str, bytes: &[u8]) {
            std::fs::write(self.root().join(rel), bytes).unwrap();
        }
        /// Commits everything as `name <email>`; returns the sha.
        fn commit(&mut self, name: &str, email: &str, message: &str) -> String {
            self.clock += 3600;
            let date = format!("@{} +0000", self.clock);
            self.git(&["add", "-A"]);
            let out = Command::new("git")
                .current_dir(self.root())
                .env("GIT_CONFIG_GLOBAL", &self.config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", name)
                .env("GIT_AUTHOR_EMAIL", email)
                .env("GIT_COMMITTER_NAME", name)
                .env("GIT_COMMITTER_EMAIL", email)
                .env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date)
                .args(["-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", message])
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            self.git(&["rev-parse", "HEAD"])
        }
    }

    struct FakeGh {
        logins: Vec<(String, String)>,
        prs: Vec<(String, String)>,
        names: Vec<(String, String)>,
    }

    impl GitHub for FakeGh {
        fn repo(&self) -> Result<String, String> {
            Ok("owner/repo".into())
        }
        fn commit_logins(&self, _repo: &str, _head: &str) -> Result<Vec<(String, String)>, String> {
            Ok(self.logins.clone())
        }
        fn merged_prs(&self, _repo: &str, base: &str) -> Result<Vec<(String, String)>, String> {
            assert_eq!(base, "main");
            Ok(self.prs.clone())
        }
        fn profile_name(&self, login: &str) -> Result<Option<String>, String> {
            Ok(self.names.iter().find(|(l, _)| l == login).map(|(_, n)| n.clone()))
        }
    }

    const CLAUDE_55: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
    const ALICE: (&str, &str) = ("Planted Alice Person", "alice.private@example.org");
    const BOB: (&str, &str) = ("Planted Bob Person", "bob.private@example.org");
    const DAVE: (&str, &str) = ("Planted Dave Person", "dave.private@example.org");

    #[test]
    fn refresh_counts_per_username_and_model_and_records_no_name_or_email() {
        let mut r = Repo::new();
        r.write("a.txt", b"1\n2\n3\n");
        r.write("logo.png", &[0, 1, 2, 3, 0, 255]);
        r.write(PEOPLE, b"# people\n");
        let c1 = r.commit(ALICE.0, ALICE.1, &format!("M1.1: first (#1)\n\nBody.\n\n{CLAUDE_55}\n"));
        r.write("a.txt", b"1\n2\nthree\n");
        std::fs::remove_file(r.root().join("logo.png")).unwrap();
        let c2 = r.commit(
            BOB.0,
            BOB.1,
            "M1.2: second (#2)\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>\nCo-Authored-By: Human Helper <helper.human@example.net>\n",
        );
        r.write(PEOPLE, b"# people\n\n[people.alice]\nreal_name = \"Alice Real\"\nsync_github_name = true\n");
        let c3 = r.commit(ALICE.0, ALICE.1, &format!("M1.3: credit myself (#3)\n\n{CLAUDE_55}\n"));
        r.write("b.txt", b"x\n");
        let c4 = r.commit(DAVE.0, DAVE.1, "unlinked commit\n");
        // A merge commit: not counted itself, its side commit is.
        r.git(&["checkout", "-q", "-b", "side", &c3]);
        r.write("c.txt", b"side\n");
        let c5 = r.commit(BOB.0, BOB.1, "side work\n");
        r.git(&["checkout", "-q", "main"]);
        r.git(&[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Merger",
            "-c",
            "user.email=m@example.org",
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge",
            "side",
        ]);
        let merge = r.git(&["rev-parse", "HEAD"]);

        let gh = FakeGh {
            logins: vec![
                (c1.clone(), "alice".into()),
                (c2.clone(), "Bob".into()),
                (c3.clone(), "alice".into()),
                (c4.clone(), String::new()),
                (c5.clone(), "bob".into()),
                (merge.clone(), "dependabot[bot]".into()),
            ],
            prs: vec![
                (c1.clone(), "alice".into()),
                (c2.clone(), "bob".into()),
                (c3.clone(), "alice".into()),
                ("f".repeat(40), "alice".into()), // merged elsewhere: not on this history
                (merge.clone(), "dependabot[bot]".into()),
            ],
            names: vec![("alice".into(), "Alice Public".into())],
        };
        let (report, notes) = refresh(&r.root(), &gh, "main").unwrap();
        assert_eq!(notes, Vec::<String>::new());
        assert_eq!((report.schema, report.repo.as_str(), report.branch.as_str(), report.head.as_str()), (1, "owner/repo", "main", merge.as_str()));
        assert_eq!((report.commits, report.unattributed_commits), (5, 1));

        let logins: Vec<&str> = report.contributors.iter().map(|c| c.login.as_str()).collect();
        // The first spelling seen (newest commit first) is kept.
        assert_eq!(logins, ["alice", "bob"]);
        let alice = &report.contributors[0];
        assert_eq!((alice.prs, alice.commits, alice.lines_added, alice.lines_deleted, alice.binary_added, alice.binary_deleted), (2, 2, 8, 0, 1, 0));
        assert!(alice.people_entry);
        assert_eq!(alice.github_name.as_deref(), Some("Alice Public"));
        assert_eq!(alice.first_commit.as_deref(), Some(utc(1_790_003_600).as_str()));
        assert_eq!(alice.last_commit.as_deref(), Some(utc(1_790_010_800).as_str()));
        let bob = &report.contributors[1];
        assert_eq!((bob.prs, bob.commits, bob.lines_added, bob.lines_deleted, bob.binary_added, bob.binary_deleted), (1, 2, 2, 1, 0, 1));
        assert!(!bob.people_entry && bob.github_name.is_none());

        let models: Vec<(&str, u64, u64)> = report.models.iter().map(|m| (m.id.as_str(), m.commits, m.lines_added)).collect();
        assert_eq!(models, [("anthropic/claude-opus/5.5", 2, 8), ("anthropic/claude-opus/4.8", 1, 1)]);

        let json = to_json(&report).unwrap();
        for secret in ["Planted", "Person", "private", "example.org", "example.net", "Human", "helper", "anthropic.com", "Merger", "@"] {
            assert!(!json.contains(secret), "{secret:?} leaked into the JSON:\n{json}");
        }
        // The Craft apps' schema: key order and types.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["contributors"][0]["first_commit"], serde_json::json!(utc(1_790_003_600)));
        assert!(json.find("\"schema\"").unwrap() < json.find("\"contributors\"").unwrap());
    }

    #[test]
    fn check_refuses_an_entry_added_by_someone_else_unless_it_has_consent() {
        let mut r = Repo::new();
        // A malformed file fails before any history is read.
        r.write(PEOPLE, b"[people.alice]\nnickname = \"A\"\n");
        assert!(check(&r.root()).unwrap_err().contains("1 problem(s)"));
        r.write(PEOPLE, b"[people.alice]\nreal_name = \"Alice\"\n");
        r.commit("Alice", "1+alice@users.noreply.github.com", "alice credits herself");
        check(&r.root()).unwrap();

        // Bob adds Carol: refused, by --check and by the refresh.
        r.write(PEOPLE, b"[people.alice]\nreal_name = \"Alice\"\n\n[people.carol]\nreal_name = \"Carol\"\n");
        let bad = r.commit("Bob", "2+bob@users.noreply.github.com", "bob adds carol");
        let err = check(&r.root()).unwrap_err();
        assert!(err.contains("1 entry added by someone else"), "{err}");
        let adders = entry_adders(&r.root(), "HEAD", &HashMap::new()).unwrap();
        assert_eq!(adders.get("carol"), Some(&(bad.clone(), Some("bob".to_string()))));
        let gh = FakeGh { logins: vec![], prs: vec![], names: vec![] };
        let err = refresh(&r.root(), &gh, "main").unwrap_err();
        assert!(err.contains("[people.carol] was added by @bob"), "{err}");
        assert!(!r.root().join(JSON).exists());

        // A later edit by someone else does not make an add; with consent_source it passes.
        r.write(
            PEOPLE,
            b"[people.alice]\nreal_name = \"Alice\"\n\n[people.carol]\nreal_name = \"Carol\"\nconsent_source = \"https://example.org/asked\"\n",
        );
        r.commit("Bob", "2+bob@users.noreply.github.com", "consent recorded");
        check(&r.root()).unwrap();

        // An owner that cannot be told offline (an address that is not a no-reply one) is not
        // refused, and its names stay hidden; GitHub's username for the commit then decides.
        let mut r = Repo::new();
        r.write(PEOPLE, b"[people.dave]\nreal_name = \"Dave\"\n");
        let sha = r.commit("Dave", "dave@example.org", "dave credits himself");
        check(&r.root()).unwrap();
        let owners = verify(&parse_people("[people.dave]\n").unwrap(), &entry_adders(&r.root(), "HEAD", &HashMap::new()), "HEAD");
        assert!(matches!(&owners[..], [Owner::NotChecked(_)]), "{owners:?}");
        let mapped = HashMap::from([(sha.clone(), "dave".to_string())]);
        assert_eq!(verify(&parse_people("[people.dave]\n").unwrap(), &entry_adders(&r.root(), "HEAD", &mapped), "HEAD"), [Owner::Verified]);
        let mapped = HashMap::from([(sha, "mallory".to_string())]);
        assert!(matches!(
            &verify(&parse_people("[people.dave]\n").unwrap(), &entry_adders(&r.root(), "HEAD", &mapped), "HEAD")[..],
            [Owner::Refused(_)]
        ));
    }

    #[test]
    fn unknown_arguments_are_refused() {
        let root = crate::workspace_root();
        assert!(run(&root, &["--frobnicate".into()]).is_err());
        assert!(run(&root, &["--ref".into()]).is_err());
    }
}
