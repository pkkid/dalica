//! Dalica as the default viewer (`viewing.md` §3.3): the application that opens a file type now,
//! and making Dalica the one, per OS, only where the OS lets an application do it.
//!
//! - **Linux** (and the other freedesktop systems): the user's `mimeapps.list`
//!   (`$XDG_CONFIG_HOME/mimeapps.list`) gets `<mime>=org.dalica.Dalica.desktop;` in its `[Default
//!   Applications]` group for each MIME type of the types chosen ([`dalica_engine::os::mime_types`]).
//!   The file is another program's: it is read whole (at most [`desktop::MAX_FILE`] bytes; a larger
//!   one is refused, never cut), every other line is kept byte for byte (other groups, keys,
//!   comments, blank lines, the line endings), and it is written through `fs` (atomic, over the
//!   version read, a final link refused). One that is not UTF-8 is damaged: it is kept aside as
//!   `mimeapps.list.corrupt-<time>Z` and a new one holds Dalica's lines. What a key said before
//!   Dalica took it is remembered in Dalica's own `$XDG_CONFIG_HOME/dalica/default-viewer.json`
//!   (written before the list, so a crash between the two never loses it), and a type no longer
//!   chosen ("Don't use Dalica as a viewer": none) gets it back, or loses the key when there was
//!   none; a key the user changed since is left as it is. Dalica's desktop entry must be installed,
//!   or the desktop could not start Dalica for the files: the call answers why instead.
//! - **Windows**: only the user may choose a default (Settings ▸ Default apps; the `UserChoice`
//!   keys are protected). Dalica registers itself for the user with `reg.exe` (the system's own
//!   program, an argument vector, never a shell): its ProgID `Dalica.Image` with its icon and open
//!   command, the types' `OpenWithProgids`, and its Capabilities and `RegisteredApplications` entry
//!   so Default apps lists it; then opens Default apps on Dalica's page and says what to choose.
//!   None chosen takes the Capabilities and the user's Open with entries away again.
//! - **macOS**: setting the role handler needs a binding this build does not have (the safe one
//!   `cargo xtask deps` would accept needs `UTType`, not in the workspace), so nothing is set:
//!   Dalica is registered as a Viewer by its `Info.plist`, and the answer says how the user makes it
//!   the default in Finder. The default handler is not known without a file of the type (`None`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_engine::os::{DefaultSet, Handler, OsError, extensions, mime_types, type_name};
use dalica_engine::settings::VIEWER_TYPES;
use dalica_fs::Expect;
use dalica_scan::FileType;
use serde_json::{Map, Value, json};

use super::launch::{Argv, Platform, Runner};
use super::{Env, Native, desktop, registry};

/// Dalica's desktop entry's id (its application id, `dalica_core::app_id::APP_ID`).
pub fn desktop_id() -> String {
    format!("{}.desktop", dalica_core::app_id::APP_ID)
}

/// The `mimeapps.list` group that names the defaults.
pub const DEFAULTS: &str = "[Default Applications]";
/// Dalica's own file of the handlers it replaced, under the user's config folder.
pub const MEMORY: &str = "dalica/default-viewer.json";
/// The most bytes of [`MEMORY`] read; a larger or unreadable one is kept aside and read as empty.
pub const MAX_MEMORY: u64 = 64 * 1024;
/// The most MIME types [`MEMORY`] remembers, and the longest value it keeps for one.
pub const MAX_REMEMBERED: usize = 256;
pub const MAX_VALUE: usize = 4096;
/// Dalica's ProgID on Windows.
pub const PROG_ID: &str = "Dalica.Image";
/// Where Dalica's Capabilities live on Windows (under `HKCU`).
pub const CAPABILITIES: &str = r"Software\Dalica\Capabilities";
/// The per-user file associations on Windows.
const FILE_EXTS: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";
/// Windows' Default apps, on Dalica's page.
pub const DEFAULT_APPS: &str = "ms-settings:defaultapps?registeredAppUser=Dalica";

/// The application that opens `ty` by default.
pub fn default_handler(os: &Native, ty: FileType) -> Result<Option<Handler>, OsError> {
    match os.platform {
        Platform::Linux => Ok(linux_default(&os.dirs(), ty)),
        Platform::Windows => Ok(windows_default(os.runner.as_ref(), &os.env, ty)),
        Platform::MacOs => Ok(None),
    }
}

/// Makes Dalica the default viewer for `types` and gives every other type it holds back.
pub fn set_default_viewer(os: &Native, types: &[FileType]) -> Result<DefaultSet, OsError> {
    match os.platform {
        Platform::Linux => linux_set(&os.dirs(), types).map(|()| DefaultSet::Set),
        Platform::Windows => windows_set(os, types),
        Platform::MacOs => Ok(DefaultSet::Choose(mac_message(types))),
    }
}

/// The names of `types` for messages ("JPEG, PNG and RAW").
fn names(types: &[FileType]) -> String {
    let names: Vec<&str> = types.iter().map(|t| type_name(*t)).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_string(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

// --- Linux --------------------------------------------------------------------------------------

/// The default of `ty`'s main MIME type: the `mimeapps.list` defaults, then the other associations,
/// as Open With lists them ([`desktop::handlers`]), for a file of the type's main extension.
fn linux_default(dirs: &desktop::Dirs, ty: FileType) -> Option<Handler> {
    let ext = extensions(ty).first()?;
    // Never looked at on disk: only its name is read (`mime_of`).
    let probe = Path::new("/").join(format!("dalica-file-type-probe.{ext}"));
    desktop::handlers(&probe, dirs).into_iter().next().map(|e| Handler { id: e.id, name: e.name, icon: e.icon })
}

/// The user's `mimeapps.list` and Dalica's [`MEMORY`] file, under `dirs`' first config folder.
fn linux_files(dirs: &desktop::Dirs) -> Result<(PathBuf, PathBuf), OsError> {
    let config = dirs.config.first().ok_or_else(|| OsError::Unavailable("There is no home folder to keep the default applications in.".into()))?;
    Ok((config.join("mimeapps.list"), config.join(MEMORY)))
}

/// Linux's [`set_default_viewer`]: the merge described in the module's notes.
pub fn linux_set(dirs: &desktop::Dirs, types: &[FileType]) -> Result<(), OsError> {
    let (list, memory_path) = linux_files(dirs)?;
    let me = desktop_id();
    if !types.is_empty() && !desktop::installed(dirs).get(&me).is_some_and(|(_, e)| !e.hidden) {
        return Err(OsError::Unavailable(format!(
            "Dalica's desktop entry ({me}) is not installed, so the desktop could not open files with Dalica: install Dalica's package first."
        )));
    }
    let (mut memory, memory_expect) = read_memory(&memory_path);
    let (text, expect) = read_list(&list)?;
    let wanted: Vec<&str> = types.iter().flat_map(|t| mime_types(*t).iter().copied()).collect();
    let every: Vec<&str> = VIEWER_TYPES.iter().flat_map(|t| mime_types(*t).iter().copied()).collect();
    let mut next = text.clone();
    let before = memory.clone();
    let mut given_back = Vec::new();
    for mime in every {
        let current = default_of(&next, mime);
        let held = current.as_deref().is_some_and(|v| first_id(v) == Some(me.as_str()));
        if wanted.contains(&mime) {
            if !held {
                if memory.len() >= MAX_REMEMBERED {
                    return Err(OsError::Failed("Dalica remembers too many replaced applications; nothing was changed.".into()));
                }
                memory.insert(mime.to_string(), current.filter(|v| v.len() <= MAX_VALUE));
                next = set_default(&next, mime, Some(&format!("{me};")));
            }
        } else if held {
            let old = memory.get(mime).cloned().flatten();
            next = set_default(&next, mime, old.as_deref());
            given_back.push(mime.to_string());
        } else {
            // The user chose another application since: nothing of Dalica's to give back.
            given_back.push(mime.to_string());
        }
    }
    // What is remembered is written first, with every handler this call replaces, so a crash
    // before the list is written never loses one; the ones given back are forgotten after.
    let memory_expect = if memory != before { write_memory(&memory_path, &memory, memory_expect)? } else { memory_expect };
    if next != text {
        if let Some(parent) = list.parent() {
            dalica_fs::create_dir_all(parent).map_err(|e| OsError::Failed(e.to_string()))?;
        }
        dalica_fs::replace(&list, next.as_bytes(), expect).map_err(|e| OsError::Failed(format!("mimeapps.list was not written: {e}")))?;
    }
    let before = memory.clone();
    given_back.iter().for_each(|m| {
        memory.remove(m);
    });
    if memory != before {
        write_memory(&memory_path, &memory, memory_expect)?;
    }
    Ok(())
}

/// The user's `mimeapps.list` as text, with what a write must find: a missing one is empty; one
/// that is not UTF-8 is kept aside (`.corrupt-<time>Z`) and read as empty; a larger one than
/// [`desktop::MAX_FILE`], a link or one that cannot be read refuses.
fn read_list(path: &Path) -> Result<(String, Expect), OsError> {
    match dalica_fs::read(path, desktop::MAX_FILE) {
        Ok(None) => Ok((String::new(), Expect::Absent)),
        Ok(Some(read)) => match String::from_utf8(read.bytes) {
            Ok(text) if !text.contains('\0') => Ok((text, Expect::Unchanged(read.identity))),
            _ => {
                let aside = dalica_fs::set_aside(path, &corrupt_tag()).map_err(|e| OsError::Failed(e.to_string()))?;
                log::warn!("os: {} is damaged; kept aside as {}", path.display(), aside.display());
                Ok((String::new(), Expect::Absent))
            }
        },
        Err(e) => Err(OsError::Failed(format!("mimeapps.list cannot be changed: {e}"))),
    }
}

/// `corrupt-<time>Z`, the tag of a damaged file kept aside now.
fn corrupt_tag() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let secs = i64::try_from(secs).unwrap_or(0);
    let stamp = dalica_core::DateTime::from_unix_secs(secs).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
    format!("corrupt-{stamp}Z")
}

/// What Dalica remembers: by MIME type, the value the key had before Dalica took it (`None`: there
/// was no key).
pub type Memory = BTreeMap<String, Option<String>>;

/// Reads [`MEMORY`]: missing is empty; damaged (not JSON of the shape [`print_memory`] writes,
/// larger than [`MAX_MEMORY`]) is kept aside and read as empty. Entries past the bounds are left out.
fn read_memory(path: &Path) -> (Memory, Expect) {
    let read = match dalica_fs::read(path, MAX_MEMORY) {
        Ok(None) => return (Memory::new(), Expect::Absent),
        Ok(Some(read)) => read,
        Err(e) => {
            log::warn!("os: {} cannot be read ({e}); kept aside", path.display());
            let _ = dalica_fs::set_aside(path, &corrupt_tag());
            return (Memory::new(), Expect::Absent);
        }
    };
    match parse_memory(&read.bytes) {
        Some(m) => (m, Expect::Unchanged(read.identity)),
        None => {
            log::warn!("os: {} is damaged; kept aside", path.display());
            let _ = dalica_fs::set_aside(path, &corrupt_tag());
            (Memory::new(), Expect::Absent)
        }
    }
}

/// [`MEMORY`]'s content: `{"version": 1, "replaced": {"<mime>": "<value>" | null}}`; `None` when
/// it is not that. At most [`MAX_REMEMBERED`] entries are kept, each a MIME type of at most 255
/// bytes with a value of at most [`MAX_VALUE`].
pub fn parse_memory(bytes: &[u8]) -> Option<Memory> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let replaced = v.get("replaced")?.as_object()?;
    let mut out = Memory::new();
    for (mime, value) in replaced.iter().take(MAX_REMEMBERED) {
        if mime.len() > 255 || !mime.contains('/') || mime.contains(['=', '[', '\n', '\r']) {
            continue;
        }
        match value {
            Value::Null => {
                out.insert(mime.clone(), None);
            }
            Value::String(s) if s.len() <= MAX_VALUE && !s.contains(['\n', '\r']) => {
                out.insert(mime.clone(), Some(s.clone()));
            }
            _ => {}
        }
    }
    Some(out)
}

pub fn print_memory(memory: &Memory) -> Vec<u8> {
    let replaced: Map<String, Value> = memory.iter().map(|(k, v)| (k.clone(), v.as_ref().map_or(Value::Null, |s| json!(s)))).collect();
    let mut out = serde_json::to_vec_pretty(&json!({ "version": 1, "replaced": replaced })).unwrap_or_default();
    out.push(b'\n');
    out
}

fn write_memory(path: &Path, memory: &Memory, expect: Expect) -> Result<Expect, OsError> {
    if let Some(parent) = path.parent() {
        dalica_fs::create_dir_all(parent).map_err(|e| OsError::Failed(e.to_string()))?;
    }
    dalica_fs::replace(path, &print_memory(memory), expect)
        .map(Expect::Unchanged)
        .map_err(|e| OsError::Failed(format!("Dalica could not remember the applications it replaces: {e}")))
}

/// The first desktop entry a `mimeapps.list` value names.
fn first_id(value: &str) -> Option<&str> {
    value.split(';').map(str::trim).find(|s| !s.is_empty())
}

/// The key of an ini line (`image/jpeg` in `image/jpeg = a.desktop;`), if it is one.
fn key_of(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if line.starts_with(['#', '[']) {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

/// Whether `line` starts a group, and whether that group is [`DEFAULTS`].
fn group_of(line: &str) -> Option<bool> {
    let t = line.trim();
    t.starts_with('[').then(|| t == DEFAULTS)
}

/// The value `mime` has in the `[Default Applications]` group of `text` (its first line for it),
/// as written.
pub fn default_of(text: &str, mime: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        if let Some(defaults) = group_of(line) {
            inside = defaults;
        } else if inside && key_of(line) == Some(mime) {
            return line.split_once('=').map(|(_, v)| v.trim().to_string());
        }
    }
    None
}

/// `text` with `mime`'s default set to `value` (`None`: its line taken out): every line of the
/// key in a `[Default Applications]` group is replaced (or removed); a new key goes after the
/// group's last entry, and a new group at the end. Every other line is kept byte for byte.
pub fn set_default(text: &str, mime: &str, value: Option<&str>) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let new_line = value.map(|v| format!("{mime}={v}{eol}"));
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut found = false;
    for line in text.split_inclusive('\n') {
        if let Some(defaults) = group_of(line) {
            inside = defaults;
        } else if inside && key_of(line) == Some(mime) {
            found = true;
            out.extend(new_line.clone());
            continue;
        }
        out.push(line.to_string());
    }
    let Some(line) = new_line.filter(|_| !found) else { return out.concat() };
    // After the last entry of the first `[Default Applications]` group, else a new group at the end.
    let header = out.iter().position(|l| group_of(l) == Some(true));
    let at = header.map(|h| {
        let end = out.iter().skip(h + 1).position(|l| group_of(l).is_some()).map_or(out.len(), |n| h + 1 + n);
        (h + 1..end).rev().find(|&i| out.get(i).is_some_and(|l| key_of(l).is_some())).map_or(h + 1, |i| i + 1)
    });
    let ending = |out: &mut Vec<String>, at: usize| {
        if let Some(prev) = at.checked_sub(1).and_then(|i| out.get_mut(i))
            && !prev.ends_with('\n')
        {
            prev.push_str(eol);
        }
    };
    match at {
        Some(at) => {
            ending(&mut out, at);
            out.insert(at, line);
        }
        None => {
            let n = out.len();
            ending(&mut out, n);
            if !out.is_empty() {
                out.push(eol.to_string());
            }
            out.push(format!("{DEFAULTS}{eol}"));
            out.push(line);
        }
    }
    out.concat()
}

// --- Windows ------------------------------------------------------------------------------------

/// `id`'s icon: its `DefaultIcon` (`C:\path\app.exe,0`), variables expanded.
pub fn windows_icon(runner: &dyn Runner, id: &str, env: &Env) -> Option<String> {
    let key = registry::query(runner, &format!(r"HKCR\{id}\DefaultIcon"))?;
    key.value(None).map(|v| registry::expand_env(v, env))
}

/// The ProgID that opens `ty`'s main extension by default (the user's choice, else the type's
/// own), with its name and icon.
fn windows_default(runner: &dyn Runner, env: &Env, ty: FileType) -> Option<Handler> {
    let ext = extensions(ty).first()?;
    let chosen = registry::query(runner, &format!(r"{FILE_EXTS}\.{ext}\UserChoice")).and_then(|k| k.value(Some("ProgId")).map(str::to_string));
    let id = chosen.or_else(|| registry::query(runner, &format!(r"HKCR\.{ext}")).and_then(|k| k.value(None).map(str::to_string)))?;
    let command = registry::verb_command(runner, &id, "open").unwrap_or_default();
    let name = registry::app_name(&id, &command, runner, env);
    let icon = windows_icon(runner, &id, env);
    Some(Handler { id, name, icon })
}

/// `reg add <key> [/v name | /ve] /t REG_SZ /d data /f`.
fn reg_add(key: &str, name: Option<&str>, data: &str) -> Argv {
    let argv = Argv::new("reg").arg("add").arg(key);
    let argv = match name {
        Some(n) => argv.arg("/v").arg(n),
        None => argv.arg("/ve"),
    };
    argv.arg("/t").arg("REG_SZ").arg("/d").arg(data).arg("/f")
}

/// `reg delete <key> [/v name] /f`.
fn reg_delete(key: &str, name: Option<&str>) -> Argv {
    let argv = Argv::new("reg").arg("delete").arg(key);
    let argv = match name {
        Some(n) => argv.arg("/v").arg(n),
        None => argv,
    };
    argv.arg("/f")
}

/// The `reg` vectors that register Dalica for the user for `types` (and take it away from the
/// other viewer types), `exe` being `dalica.exe`. None chosen takes the Capabilities away.
pub fn windows_registration(exe: &str, types: &[FileType]) -> (Vec<Argv>, Vec<Argv>) {
    let classes = r"HKCU\Software\Classes";
    let caps = format!(r"HKCU\{CAPABILITIES}");
    let mut must = vec![
        reg_add(&format!(r"{classes}\{PROG_ID}"), None, "Image"),
        reg_add(&format!(r"{classes}\{PROG_ID}\DefaultIcon"), None, &format!("\"{exe}\",0")),
        reg_add(&format!(r"{classes}\{PROG_ID}\shell\open\command"), None, &format!("\"{exe}\" --view \"%1\"")),
    ];
    // Deletes of values that may not be there: their failure is not one.
    let mut may = Vec::new();
    if types.is_empty() {
        may.push(reg_delete(r"HKCU\Software\RegisteredApplications", Some("Dalica")));
        may.push(reg_delete(&caps, None));
    } else {
        must.push(reg_add(&caps, Some("ApplicationName"), "Dalica"));
        must.push(reg_add(&caps, Some("ApplicationDescription"), "Organize the photos in your folders and make simple edits"));
        must.push(reg_add(r"HKCU\Software\RegisteredApplications", Some("Dalica"), CAPABILITIES));
    }
    for ty in VIEWER_TYPES {
        for ext in extensions(ty) {
            if types.contains(&ty) {
                must.push(reg_add(&format!(r"{caps}\FileAssociations"), Some(&format!(".{ext}")), PROG_ID));
                must.push(reg_add(&format!(r"{classes}\.{ext}\OpenWithProgids"), Some(PROG_ID), ""));
            } else {
                if !types.is_empty() {
                    may.push(reg_delete(&format!(r"{caps}\FileAssociations"), Some(&format!(".{ext}"))));
                }
                may.push(reg_delete(&format!(r"{classes}\.{ext}\OpenWithProgids"), Some(PROG_ID)));
            }
        }
    }
    (must, may)
}

fn windows_set(os: &Native, types: &[FileType]) -> Result<DefaultSet, OsError> {
    let exe = std::env::current_exe().map_err(|e| OsError::Failed(format!("Dalica cannot find its own program: {e}")))?;
    let exe = exe.to_string_lossy().into_owned();
    if exe.contains('"') {
        return Err(OsError::Failed("Dalica's program has a name Windows cannot register.".into()));
    }
    let (must, may) = windows_registration(&exe, types);
    for argv in &must {
        os.runner.output(argv).map_err(|e| OsError::Failed(format!("Windows did not take Dalica's registration: {e}")))?;
    }
    for argv in &may {
        let _ = os.runner.output(argv);
    }
    os.start(&Argv::new("explorer.exe").arg(DEFAULT_APPS))?;
    Ok(DefaultSet::SettingsOpened(if types.is_empty() {
        "Windows lets only you choose the default apps: in Default apps, choose another app for the file types Dalica opened.".into()
    } else {
        format!("Windows lets only you choose the default apps: in Default apps, under Dalica, choose Dalica for {}.", names(types))
    }))
}

// --- macOS --------------------------------------------------------------------------------------

/// How the user makes Dalica the default on macOS (or another application again).
pub fn mac_message(types: &[FileType]) -> String {
    match types.first() {
        None => "macOS lets only you choose: in Finder, select a file of a type Dalica opens, choose File → Get Info, pick another application under Open with, then click Change All…".into(),
        Some(first) => format!(
            "macOS lets only you choose: in Finder, select a {} file, choose File → Get Info, pick Dalica under Open with, then click Change All…; do the same for {}.",
            type_name(*first),
            names(types)
        ),
    }
}
