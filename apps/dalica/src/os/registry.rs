//! The applications that open a file on Windows, from the registry as `reg query` prints it (the
//! system's own program, started with an argument vector; no registry bindings of our own): the
//! user's choice (`FileExts\.ext\UserChoice`), the type's ProgID, the ProgIDs and programs of
//! `OpenWithProgids` and `OpenWithList` (the user's and the machine's), each kept only when it
//! has an `open` command; then that command made into an argument vector with the file in it.
//! The parsers are plain text functions compiled on every OS, so the tests read fixtures anywhere.

use std::ffi::OsString;
use std::path::Path;

use super::Env;
use super::launch::{Argv, Platform, Runner, path_arg};

/// The most applications in one list.
pub const MAX_HANDLERS: usize = 64;
/// The most arguments a command line gives.
const MAX_ARGS: usize = 256;

/// The per-user file associations.
const FILE_EXTS: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// One row of `reg query` output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A value: its name (`(Default)` for the key's own), its type and its data.
    Value { name: String, kind: String, data: String },
    /// A subkey's full path.
    Key(String),
}

/// Reads `reg query` output: values are indented four spaces as `name    REG_TYPE    data`,
/// subkeys are full key paths on their own lines. Anything else is skipped.
pub fn parse_reg(output: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for line in output.lines().take(4096) {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("    ") {
            let Some(at) = rest.find("    REG_") else { continue };
            let (name, tail) = rest.split_at(at);
            let tail = tail.trim_start();
            let (kind, data) = tail.split_once("    ").unwrap_or((tail, ""));
            out.push(Row::Value { name: name.trim().to_string(), kind: kind.trim().to_string(), data: data.trim().to_string() });
        } else if line.starts_with("HKEY_") {
            out.push(Row::Key(line.trim().to_string()));
        }
    }
    out
}

/// What `reg query` says about one key: its values and its subkeys' last names (the key's own
/// line, which `reg` prints first, is not a subkey).
#[derive(Debug, Default)]
pub struct Key {
    pub values: Vec<(String, String)>,
    pub subkeys: Vec<String>,
}

impl Key {
    pub fn read(output: &str) -> Key {
        let mut key = Key::default();
        let mut own: Option<String> = None;
        for row in parse_reg(output) {
            match row {
                Row::Value { name, data, .. } => key.values.push((name, data)),
                Row::Key(path) => match &own {
                    None => own = Some(path),
                    Some(own) if path.len() > own.len() && path.starts_with(own.as_str()) => {
                        key.subkeys.extend(path.rsplit('\\').next().map(str::to_string));
                    }
                    Some(_) => {}
                },
            }
        }
        key
    }

    /// A value's data by name (`None`: the key's default value).
    pub fn value(&self, name: Option<&str>) -> Option<&str> {
        let name = name.unwrap_or("(Default)");
        self.values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, d)| d.as_str()).filter(|d| !d.is_empty())
    }
}

/// `reg query <key>` through `runner`: `None` when the key is not there.
pub fn query(runner: &dyn Runner, key: &str) -> Option<Key> {
    let out = runner.output(&Argv::new("reg").arg("query").arg(key)).ok()?;
    Some(Key::read(&String::from_utf8_lossy(&out)))
}

/// `%NAME%` replaced by the variable's value (`REG_EXPAND_SZ` data); unknown names and `%1`-style
/// codes stay.
pub fn expand_env(text: &str, env: &Env) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        let after = &after[1..];
        match after.find('%') {
            Some(end)
                if end > 0
                    && after[..end].bytes().all(|b| b.is_ascii_alphanumeric() || b"_()".contains(&b))
                    && !after[..end].bytes().all(|b| b.is_ascii_digit()) =>
            {
                let name = &after[..end];
                match env.get_ignore_case(name) {
                    Some(v) => out.push_str(&v.to_string_lossy()),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A command line split as Windows programs split theirs (`CommandLineToArgvW`): spaces separate,
/// double quotes group, `2n` backslashes before a quote are `n` backslashes and the quote groups,
/// `2n+1` are `n` and a literal quote. `None` for more than [`MAX_ARGS`] arguments.
pub fn split_command_line(line: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut has, mut quoted, mut slashes) = (false, false, 0usize);
    for c in line.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                cur.extend(std::iter::repeat_n('\\', slashes / 2));
                if slashes % 2 == 1 {
                    cur.push('"');
                } else {
                    quoted = !quoted;
                }
                slashes = 0;
                has = true;
            }
            ' ' | '\t' if !quoted => {
                cur.extend(std::iter::repeat_n('\\', slashes));
                slashes = 0;
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.extend(std::iter::repeat_n('\\', slashes));
                slashes = 0;
                cur.push(c);
            }
        }
        if out.len() > MAX_ARGS {
            return None;
        }
    }
    cur.extend(std::iter::repeat_n('\\', slashes));
    if has || !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

/// The argument vector of a verb's command (`"C:\Program Files\X\x.exe" "%1"`) for `path`:
/// environment variables expanded, the line split as Windows does, then `%1` / `%L` (alone or
/// inside an argument such as `"%1"`) the path as one argument and the other codes (`%*`, `%I`,
/// `%2` …) dropped; a command with no `%1` gets the path at the end. The path never goes through
/// cmd.exe.
pub fn command_argv(command: &str, path: &Path, env: &Env) -> Option<Argv> {
    let words = split_command_line(&expand_env(command, env))?;
    let file = path_arg(path, Platform::Windows).to_string_lossy().into_owned();
    let mut args: Vec<OsString> = Vec::new();
    let mut took = false;
    for word in words {
        let mut out = String::with_capacity(word.len());
        let mut chars = word.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('1' | 'L' | 'l') => {
                    out.push_str(&file);
                    took = true;
                }
                Some('%') => out.push('%'),
                _ => {}
            }
        }
        if !out.is_empty() || word.is_empty() {
            args.push(out.into());
        }
    }
    if !took {
        args.push(file.into());
    }
    let mut args = args.into_iter();
    let program = args.next().filter(|p| !p.is_empty())?;
    Some(Argv { program, args: args.collect(), verbatim: false })
}

/// One application that opens a file type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// A ProgID (`jpegfile`) or `Applications\<program>.exe`: the key under `HKCR` with its
    /// `shell\open\command`.
    pub id: String,
    pub name: String,
    /// Its `open` command line.
    pub command: String,
}

/// The ids that open files ending in `ext` (`.jpg`), the default first, each once.
pub fn handler_ids(runner: &dyn Runner, ext: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut push = |id: String| {
        if !id.is_empty() && !ids.iter().any(|i| i.eq_ignore_ascii_case(&id)) && ids.len() < MAX_HANDLERS {
            ids.push(id);
        }
    };
    if let Some(k) = query(runner, &format!(r"{FILE_EXTS}\{ext}\UserChoice")) {
        push(k.value(Some("ProgId")).unwrap_or_default().to_string());
    }
    if let Some(k) = query(runner, &format!(r"HKCR\{ext}")) {
        push(k.value(None).unwrap_or_default().to_string());
    }
    for key in [format!(r"{FILE_EXTS}\{ext}\OpenWithProgids"), format!(r"HKCR\{ext}\OpenWithProgids")] {
        if let Some(k) = query(runner, &key) {
            k.values.iter().filter(|(n, _)| n != "(Default)").for_each(|(n, _)| push(n.clone()));
        }
    }
    if let Some(k) = query(runner, &format!(r"{FILE_EXTS}\{ext}\OpenWithList")) {
        // Values a, b, c … name programs; MRUList is their order.
        let mru = k.value(Some("MRUList")).unwrap_or_default().to_string();
        for letter in mru.chars() {
            if let Some(exe) = k.value(Some(&letter.to_string())) {
                push(format!(r"Applications\{exe}"));
            }
        }
    }
    if let Some(k) = query(runner, &format!(r"HKCR\{ext}\OpenWithList")) {
        k.subkeys.iter().for_each(|exe| push(format!(r"Applications\{exe}")));
    }
    ids
}

/// The name an application shows: its `ApplicationName` or `FriendlyAppName` when it is text (not
/// an `@resource` reference), else its program's name, else the id.
pub fn app_name(id: &str, command: &str, runner: &dyn Runner, env: &Env) -> String {
    let given = [format!(r"HKCR\{id}\Application"), format!(r"HKCR\{id}")].into_iter().find_map(|key| {
        let k = query(runner, &key)?;
        [k.value(Some("ApplicationName")), k.value(Some("FriendlyAppName"))].into_iter().flatten().find(|v| !v.starts_with('@')).map(str::to_string)
    });
    given
        .or_else(|| {
            let program = split_command_line(&expand_env(command, env))?.into_iter().next()?;
            let stem = Path::new(&program.replace('\\', "/")).file_stem()?.to_string_lossy().into_owned();
            Some(stem).filter(|s| !s.eq_ignore_ascii_case("rundll32"))
        })
        .unwrap_or_else(|| id.rsplit('\\').next().unwrap_or(id).to_string())
}

/// `id`'s command for `verb` (`open`, `edit`), if it has one (one that only names a COM handler,
/// `DelegateExecute`, has none we can start).
pub fn verb_command(runner: &dyn Runner, id: &str, verb: &str) -> Option<String> {
    query(runner, &format!(r"HKCR\{id}\shell\{verb}\command")).and_then(|k| k.value(None).map(str::to_string))
}

/// The applications that open `path`, the default first.
pub fn handlers(path: &Path, runner: &dyn Runner, env: &Env) -> Vec<App> {
    let Some(ext) = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())) else { return Vec::new() };
    handler_ids(runner, &ext)
        .into_iter()
        .filter_map(|id| {
            let command = verb_command(runner, &id, "open")?;
            let name = app_name(&id, &command, runner, env);
            Some(App { id, name, command })
        })
        .collect()
}
