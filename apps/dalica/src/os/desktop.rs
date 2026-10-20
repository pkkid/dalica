//! The applications that open a file on Linux and the other freedesktop systems: the file's MIME
//! type from the shared MIME database (`mime/globs2`, a built-in table when there is none), the
//! user's and the system's `mimeapps.list` files (the default, added and removed associations),
//! and the desktop entries whose `MimeType` lists the type; then an entry's `Exec` line made into
//! an argument vector with the files in it (the Desktop Entry Specification's quoting and field
//! codes). Everything read is the user's or another program's file, so every read, list and walk
//! is bounded and a damaged file is skipped, never a panic.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::Env;
use super::launch::{Argv, Platform, path_arg, which};

/// The most bytes read of a desktop entry or a `mimeapps.list` file; a larger one is skipped.
pub const MAX_FILE: u64 = 256 * 1024;
/// The most bytes read of `mime/globs2` and `mime/subclasses`.
pub const MAX_MIME_DB: u64 = 4 * 1024 * 1024;
/// The most desktop entries looked at, across every folder.
pub const MAX_ENTRIES: usize = 8192;
/// How deep the walk goes below an `applications` folder.
pub const MAX_DEPTH: usize = 4;
/// The most applications in one list (and of the ids one `mimeapps.list` key names).
pub const MAX_HANDLERS: usize = 64;
/// The most arguments an `Exec` line gives.
pub const MAX_ARGS: usize = 256;
/// How many parent types are followed (`image/x-canon-cr2` → `image/x-dcraw` → …).
const MAX_PARENTS: usize = 4;

/// One desktop entry that starts an application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its desktop file id (`org.gnome.Loupe.desktop`; `kde4-foo.desktop` for `kde4/foo.desktop`).
    pub id: String,
    /// Its name, in the user's language when the entry has it.
    pub name: String,
    /// Its icon: a theme icon's name or a file.
    pub icon: Option<String>,
    /// The `Exec` value, string escapes undone.
    pub exec: String,
    pub mime_types: Vec<String>,
    /// `Hidden=true`: deleted, as if it were not there.
    pub hidden: bool,
    /// `NoDisplay=true`: not in menus; listed only when an association names it.
    pub no_display: bool,
    /// The file it was read from (`%k`).
    pub path: PathBuf,
}

/// Reads at most `max` bytes of `path` as text: `None` when it is missing, larger or not UTF-8.
pub fn read_bounded(path: &Path, max: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max {
        log::warn!("os: {} is larger than {max} bytes; skipped", path.display());
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The freedesktop base folders (XDG Base Directory Specification).
pub struct Dirs {
    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`, most important first.
    pub data: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME`, then `$XDG_CONFIG_DIRS`.
    pub config: Vec<PathBuf>,
    /// `$XDG_CURRENT_DESKTOP`'s names, lowercased.
    pub desktops: Vec<String>,
    /// The user's language, most specific first (`fr_FR`, `fr`).
    pub languages: Vec<String>,
}

impl Dirs {
    pub fn from_env(env: &Env) -> Dirs {
        let var = |k: &str| env.get(k).map(|v| v.to_string_lossy().into_owned()).filter(|v| !v.is_empty());
        let home = var("HOME").map(PathBuf::from);
        // Relative entries are ignored, as the specification says.
        let absolute = |p: PathBuf| p.is_absolute() || p.starts_with("/");
        // A list of folders in the host's own form (`:`-separated on freedesktop systems, as the
        // specification says), so a `C:\…` folder stays one entry where the host is Windows.
        let list = |v: Option<String>, default: &[&str]| -> Vec<PathBuf> {
            match v {
                Some(v) => std::env::split_paths(&v).take(64).filter(|p| absolute(p.clone())).collect(),
                None => default.iter().map(PathBuf::from).collect(),
            }
        };
        let mut data: Vec<PathBuf> = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| absolute(p.clone()))
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")))
            .into_iter()
            .collect();
        data.extend(list(var("XDG_DATA_DIRS"), &["/usr/local/share", "/usr/share"]));
        let mut config: Vec<PathBuf> = var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| absolute(p.clone()))
            .or_else(|| home.as_ref().map(|h| h.join(".config")))
            .into_iter()
            .collect();
        config.extend(list(var("XDG_CONFIG_DIRS"), &["/etc/xdg"]));
        let desktops = var("XDG_CURRENT_DESKTOP")
            .map(|d| d.split(':').take(8).filter(|s| !s.is_empty()).map(str::to_ascii_lowercase).collect())
            .unwrap_or_default();
        let lang = var("LC_ALL").or_else(|| var("LC_MESSAGES")).or_else(|| var("LANG")).unwrap_or_default();
        let lang = lang.split(['.', '@']).next().unwrap_or_default().to_string();
        let mut languages = Vec::new();
        if !lang.is_empty() && lang != "C" && lang != "POSIX" {
            if let Some((short, _)) = lang.split_once('_') {
                languages.push(lang.clone());
                languages.push(short.to_string());
            } else {
                languages.push(lang);
            }
        }
        Dirs { data, config, desktops, languages }
    }

    /// The `mimeapps.list` files, most important first (MIME Applications Associations
    /// Specification: the desktop's own before the generic one, config before data).
    pub fn mimeapps_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut add = |dir: PathBuf| {
            for d in &self.desktops {
                out.push(dir.join(format!("{d}-mimeapps.list")));
            }
            out.push(dir.join("mimeapps.list"));
        };
        for dir in &self.config {
            add(dir.clone());
        }
        for dir in &self.data {
            add(dir.join("applications"));
        }
        out
    }
}

/// The text of a key's value in an ini-style line, `None` for other keys.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.trim_start();
    rest.strip_prefix('=').map(str::trim)
}

/// A `string`-typed value with its escapes undone (`\s`, `\n`, `\t`, `\r`, `\\`).
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A `;`-separated list value (`MimeType=image/jpeg;image/png;`), at most [`MAX_HANDLERS`] items.
fn list(value: &str) -> Vec<String> {
    value.split(';').map(str::trim).filter(|s| !s.is_empty()).take(MAX_HANDLERS).map(str::to_string).collect()
}

/// Reads a desktop entry's `[Desktop Entry]` group: `None` unless it is an application with an
/// `Exec` line.
pub fn parse_entry(id: &str, path: &Path, text: &str, languages: &[String]) -> Option<Entry> {
    let mut in_group = false;
    let mut name = None;
    let mut localized: BTreeMap<usize, String> = BTreeMap::new();
    let (mut icon, mut exec, mut mime_types, mut kind) = (None, None, Vec::new(), None);
    let (mut hidden, mut no_display) = (false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group || line.starts_with('#') {
            continue;
        }
        if let Some(v) = value(line, "Name") {
            name = Some(unescape(v));
        } else if let Some(rest) = line.strip_prefix("Name[") {
            if let Some((lang, v)) = rest.split_once(']')
                && let (Some(rank), Some(v)) = (languages.iter().position(|l| l == lang), v.trim_start().strip_prefix('='))
            {
                localized.insert(rank, unescape(v.trim()));
            }
        } else if let Some(v) = value(line, "Icon") {
            icon = Some(unescape(v)).filter(|v| !v.is_empty());
        } else if let Some(v) = value(line, "Exec") {
            exec = Some(unescape(v));
        } else if let Some(v) = value(line, "MimeType") {
            mime_types = list(v);
        } else if let Some(v) = value(line, "Type") {
            kind = Some(v.to_string());
        } else if let Some(v) = value(line, "Hidden") {
            hidden = v == "true";
        } else if let Some(v) = value(line, "NoDisplay") {
            no_display = v == "true";
        }
    }
    if kind.as_deref() != Some("Application") {
        return None;
    }
    let exec = exec.filter(|e| !e.trim().is_empty())?;
    let name = localized.into_values().next().or(name).unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());
    Some(Entry { id: id.to_string(), name, icon, exec, mime_types, hidden, no_display, path: path.to_path_buf() })
}

/// What one `mimeapps.list` says about one type.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Associations {
    pub default: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Reads `mime`'s rows of a `mimeapps.list` file.
pub fn parse_mimeapps(text: &str, mime: &str) -> Associations {
    let mut out = Associations::default();
    let mut group = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            group = line;
            continue;
        }
        let Some(ids) = value(line, mime) else { continue };
        let ids = list(ids);
        match group {
            "[Default Applications]" => out.default.extend(ids),
            "[Added Associations]" => out.added.extend(ids),
            "[Removed Associations]" => out.removed.extend(ids),
            _ => {}
        }
    }
    out
}

/// The MIME type of a file named `name` by `globs2`'s `weight:type:*.ext[:flags]` rows: the
/// heaviest match, the longer pattern on a tie. Only suffix patterns are read.
pub fn mime_from_globs2(text: &str, name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let mut best: Option<(u32, usize, String)> = None;
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, ':');
        let (Some(weight), Some(mime), Some(glob)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let case_sensitive = parts.next().is_some_and(|f| f.split(',').any(|f| f == "cs"));
        let Some(suffix) = glob.strip_prefix('*') else { continue };
        if suffix.is_empty() || suffix.contains(['*', '?', '[']) {
            continue;
        }
        let hit = if case_sensitive { name.ends_with(suffix) } else { lower.ends_with(&suffix.to_lowercase()) };
        let Ok(weight) = weight.parse::<u32>() else { continue };
        if hit && best.as_ref().is_none_or(|(w, l, _)| (weight, suffix.len()) > (*w, *l)) {
            best = Some((weight, suffix.len(), mime.to_string()));
        }
    }
    best.map(|(_, _, m)| m)
}

/// The types Dalica shows, when the system has no MIME database.
const BUILT_IN: &[(&str, &str)] = &[
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("jpe", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("bmp", "image/bmp"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("webp", "image/webp"),
    ("heic", "image/heif"),
    ("heif", "image/heif"),
    ("avif", "image/avif"),
    ("jxl", "image/jxl"),
    ("psd", "image/vnd.adobe.photoshop"),
    ("dng", "image/x-adobe-dng"),
    ("cr2", "image/x-canon-cr2"),
    ("cr3", "image/x-canon-cr3"),
    ("nef", "image/x-nikon-nef"),
    ("arw", "image/x-sony-arw"),
    ("orf", "image/x-olympus-orf"),
    ("rw2", "image/x-panasonic-rw2"),
    ("raf", "image/x-fuji-raf"),
    ("pef", "image/x-pentax-pef"),
    ("mp4", "video/mp4"),
    ("m4v", "video/x-m4v"),
    ("mov", "video/quicktime"),
    ("mkv", "video/x-matroska"),
];

/// The MIME type of `path` by its name: the first `mime/globs2` of the data folders that knows
/// it, else the built-in table.
pub fn mime_of(path: &Path, dirs: &Dirs) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if path.is_dir() {
        return Some("inode/directory".into());
    }
    for dir in &dirs.data {
        if let Some(found) = read_bounded(&dir.join("mime/globs2"), MAX_MIME_DB).and_then(|t| mime_from_globs2(&t, &name)) {
            return Some(found);
        }
    }
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    BUILT_IN.iter().find(|(e, _)| *e == ext).map(|(_, m)| (*m).to_string())
}

/// `mime`'s parent types from `mime/subclasses` (`child parent` rows).
pub fn parents_from_subclasses(text: &str, mime: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(child, _)| *child == mime)
        .map(|(_, parent)| parent.trim().to_string())
        .take(MAX_PARENTS)
        .collect()
}

/// `mime` and the types it is a kind of, nearest first.
fn with_parents(mime: &str, dirs: &Dirs) -> Vec<String> {
    let texts: Vec<String> = dirs.data.iter().filter_map(|d| read_bounded(&d.join("mime/subclasses"), MAX_MIME_DB)).collect();
    let mut out = vec![mime.to_string()];
    let mut i = 0;
    while let Some(m) = out.get(i).cloned() {
        for t in &texts {
            for p in parents_from_subclasses(t, &m) {
                if !out.contains(&p) && out.len() <= MAX_PARENTS {
                    out.push(p);
                }
            }
        }
        i += 1;
    }
    out
}

/// Every installed desktop entry by id, the most important folder's first (a later folder's
/// entry of the same id is shadowed). At most [`MAX_ENTRIES`] files, [`MAX_DEPTH`] deep.
pub fn installed(dirs: &Dirs) -> BTreeMap<String, (usize, Entry)> {
    let mut out = BTreeMap::new();
    let mut seen = 0usize;
    for (rank, data) in dirs.data.iter().enumerate() {
        let root = data.join("applications");
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for item in read.flatten() {
                if seen >= MAX_ENTRIES {
                    log::warn!("os: more than {MAX_ENTRIES} desktop entries; the rest are not read");
                    return out;
                }
                seen += 1;
                let path = item.path();
                let Ok(kind) = item.file_type() else { continue };
                if kind.is_dir() {
                    if depth < MAX_DEPTH {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&root) else { continue };
                let id = rel.to_string_lossy().replace(['/', '\\'], "-");
                if out.contains_key(&id) {
                    continue;
                }
                if let Some(entry) = read_bounded(&path, MAX_FILE).and_then(|t| parse_entry(&id, &path, &t, &dirs.languages)) {
                    out.insert(id, (rank, entry));
                }
            }
        }
    }
    out
}

/// The applications that open `path`, the default first: the `mimeapps.list` defaults, then the
/// added associations, then every entry whose `MimeType` lists the type (by folder, then id),
/// each once, the removed ones and hidden entries left out; then the same for its parent types.
pub fn handlers(path: &Path, dirs: &Dirs) -> Vec<Entry> {
    let Some(mime) = mime_of(path, dirs) else { return Vec::new() };
    let entries = installed(dirs);
    let texts: Vec<String> = dirs.mimeapps_files().iter().filter_map(|f| read_bounded(f, MAX_FILE)).collect();
    let mut out: Vec<Entry> = Vec::new();
    let mut taken = BTreeSet::new();
    for mime in with_parents(&mime, dirs) {
        let lists: Vec<Associations> = texts.iter().map(|t| parse_mimeapps(t, &mime)).collect();
        let removed: BTreeSet<&String> = lists.iter().flat_map(|a| &a.removed).collect();
        let named = lists.iter().flat_map(|a| &a.default).chain(lists.iter().flat_map(|a| a.added.iter().filter(|id| !removed.contains(id))));
        let mut listing: Vec<&(usize, Entry)> =
            entries.values().filter(|(_, e)| !e.no_display && e.mime_types.contains(&mime) && !removed.contains(&e.id)).collect();
        listing.sort_by(|(ra, a), (rb, b)| (ra, &a.id).cmp(&(rb, &b.id)));
        let ids = named.cloned().chain(listing.into_iter().map(|(_, e)| e.id.clone()));
        for id in ids {
            if out.len() >= MAX_HANDLERS {
                return out;
            }
            let Some((_, entry)) = entries.get(&id) else { continue };
            if entry.hidden || !taken.insert(id) {
                continue;
            }
            out.push(entry.clone());
        }
    }
    out
}

/// An `Exec` value split into its arguments: separated by spaces, an argument in double quotes
/// keeps its spaces and takes `\"`, `` \` ``, `\$` and `\\` as the characters. `None` when a
/// quote is never closed or there are more than [`MAX_ARGS`] arguments.
pub fn split_exec(exec: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => cur.push(chars.next()?),
            ' ' | '\t' | '\n' if !quoted => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
        if out.len() > MAX_ARGS {
            return None;
        }
    }
    if quoted {
        return None;
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

/// The argument vector that opens `paths` in `entry`: its `Exec` line split, `%f`/`%u` the first
/// file and `%F`/`%U` every file (each one argument, never part of another, so a file name never
/// reaches a shell the entry might start), `%i` its icon, `%c` its name, `%k` its file, `%%` a
/// percent sign; the deprecated codes and a code inside a longer argument are dropped. With no
/// file code the files are added at the end. The program is looked for on `PATH`.
pub fn exec_argv(entry: &Entry, paths: &[PathBuf], env: &Env) -> Result<Argv, String> {
    let words = split_exec(&entry.exec).ok_or_else(|| format!("{} has an Exec line that cannot be read", entry.id))?;
    let files = || paths.iter().map(|p| path_arg(p, Platform::Linux));
    let mut args: Vec<OsString> = Vec::new();
    let mut took_files = false;
    for word in words {
        match word.as_str() {
            "%f" | "%u" => {
                args.extend(files().take(1));
                took_files = true;
            }
            "%F" | "%U" => {
                args.extend(files());
                took_files = true;
            }
            "%i" => {
                if let Some(icon) = &entry.icon {
                    args.push("--icon".into());
                    args.push(icon.into());
                }
            }
            "%c" => args.push(entry.name.clone().into()),
            "%k" => args.push(entry.path.clone().into_os_string()),
            "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            _ => args.push(expand_inline(&word).into()),
        }
    }
    if !took_files {
        args.extend(files());
    }
    let mut args = args.into_iter();
    let program = args.next().ok_or_else(|| format!("{} has an empty Exec line", entry.id))?;
    let name = program.to_string_lossy().into_owned();
    let found = which(&name, env, Platform::Linux).ok_or_else(|| format!("{name} was not found"))?;
    Ok(Argv { program: found.into_os_string(), args: args.collect(), verbatim: false })
}

/// A word with a field code inside it: `%%` is a percent sign, any other code is dropped.
fn expand_inline(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if let Some('%') = chars.next() {
            out.push('%');
        }
    }
    out
}
