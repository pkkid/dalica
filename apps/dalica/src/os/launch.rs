//! Starting the platform's own programs (`security.md` §5.11): an argument vector, never a shell.
//! Every OS's vectors are built by plain functions compiled on every OS, so the tests check
//! Windows' and macOS's vectors on Linux too; [`Runner`] starts them ([`Spawn`]) or, in tests,
//! records them.
//!
//! Reveal's vectors are ported from LightCraft `apps/lightcraft/src/main.rs` (the `reveal` service,
//! `explorer_select_arg`) at d999e382 (MIT OR Apache-2.0).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Env;

/// The most bytes of a program's output read back ([`Runner::output`]).
pub const MAX_OUTPUT: u64 = 1 << 20;

/// The operating system whose programs a vector names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    MacOs,
}

impl Platform {
    /// The one this build runs on (the BSDs take Linux's freedesktop ways).
    pub fn host() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// A program and its arguments, each one element: nothing in them is ever read by a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Argv {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// Windows only: the arguments go on the command line exactly as they are, joined by spaces
    /// (Explorer's `/select,"<path>"`, which Rust's own quoting would wrap whole).
    pub verbatim: bool,
}

impl Argv {
    pub fn new(program: impl Into<OsString>) -> Argv {
        Argv { program: program.into(), args: Vec::new(), verbatim: false }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Argv {
        self.args.push(arg.into());
        self
    }

    pub fn args<I: IntoIterator<Item = A>, A: Into<OsString>>(mut self, args: I) -> Argv {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// The vector as text, for logs and test messages.
    pub fn describe(&self) -> String {
        let mut out = self.program.to_string_lossy().into_owned();
        for a in &self.args {
            out.push_str(" ‹");
            out.push_str(&a.to_string_lossy());
            out.push('›');
        }
        out
    }
}

/// Starts programs. Every method may block; none may panic.
pub trait Runner: Send + Sync {
    /// Starts `argv` and returns once it has started (it runs on alone).
    fn start(&self, argv: &Argv) -> Result<(), String>;
    /// Runs `argv` to its end and returns what it wrote to its standard output (at most
    /// [`MAX_OUTPUT`] bytes); `Err` when it could not start or did not succeed.
    fn output(&self, argv: &Argv) -> Result<Vec<u8>, String>;
}

/// The real [`Runner`]: `std::process::Command` with the vector as it is, no shell, nothing read
/// from the standard input; a started program is waited for on its own thread so it never lingers
/// as a zombie.
#[derive(Debug, Default)]
pub struct Spawn;

fn command(argv: &Argv) -> Command {
    let mut c = Command::new(&argv.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up for `reg` or a handler started from the window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
        if argv.verbatim {
            for a in &argv.args {
                c.raw_arg(a);
            }
        } else {
            c.args(&argv.args);
        }
    }
    #[cfg(not(windows))]
    c.args(&argv.args);
    c.stdin(Stdio::null());
    c
}

impl Runner for Spawn {
    fn start(&self, argv: &Argv) -> Result<(), String> {
        let mut child = command(argv)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{} could not start: {e}", argv.program.to_string_lossy()))?;
        let reaped = std::thread::Builder::new().name("dalica-os-child".into()).spawn(move || {
            let _ = child.wait();
        });
        if let Err(e) = reaped {
            log::warn!("os: no thread to wait for a started program: {e}");
        }
        Ok(())
    }

    fn output(&self, argv: &Argv) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let mut child = command(argv)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{} could not start: {e}", argv.program.to_string_lossy()))?;
        let mut out = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout.take(MAX_OUTPUT).read_to_end(&mut out).map_err(|e| e.to_string())?;
        }
        // Whatever is past the cap is not waited for: the program is stopped.
        if out.len() as u64 >= MAX_OUTPUT {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() { Ok(out) } else { Err(format!("{} ended with {status}", argv.program.to_string_lossy())) }
    }
}

/// A path as one argument: a path that starts with `-` would read as an option, so it is given as
/// `./-name` (`.\-name` on Windows), the same file; on Windows its separators are backslashes.
pub fn path_arg(path: &Path, platform: Platform) -> OsString {
    let dash = path.as_os_str().as_encoded_bytes().first() == Some(&b'-');
    if platform == Platform::Windows {
        let shown = path.to_string_lossy().replace('/', "\\");
        return OsString::from(if dash { format!(".\\{shown}") } else { shown });
    }
    if dash {
        let mut out = OsString::from("./");
        out.push(path.as_os_str());
        out
    } else {
        path.as_os_str().to_os_string()
    }
}

/// Where `program` is on `PATH` (`program.exe` and the other `PATHEXT` names on Windows): a name
/// with a separator is taken as it is.
pub fn which(program: &str, env: &Env, platform: Platform) -> Option<PathBuf> {
    let has_separator = program.contains('/') || (platform == Platform::Windows && program.contains('\\'));
    if has_separator {
        return Some(PathBuf::from(program));
    }
    if program.is_empty() {
        return None;
    }
    let path = env.get("PATH")?;
    let names: Vec<OsString> = if platform == Platform::Windows {
        let exts = env.get("PATHEXT").map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| ".COM;.EXE".into());
        let mut names = vec![OsString::from(program)];
        names.extend(exts.split(';').filter(|e| !e.is_empty()).map(|e| OsString::from(format!("{program}{}", e.to_ascii_lowercase()))));
        names
    } else {
        vec![OsString::from(program)]
    };
    std::env::split_paths(&path)
        .take(256)
        .filter(|d| !d.as_os_str().is_empty())
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// `program`, found on `PATH` when it is there, else its bare name (the start then fails with the
/// system's own reason).
pub fn program(name: &str, env: &Env, platform: Platform) -> OsString {
    which(name, env, platform).map_or_else(|| OsString::from(name), PathBuf::into_os_string)
}

/// Explorer's `/select,<path>` switch. Explorer parses its own command line and only selects the
/// file when the path after the comma is quoted (`/select,"C:\My Photos\a.jpg"`), spelled with
/// backslashes and free of the verbatim prefix (`\\?\`, `\\?\UNC\`) that `canonicalize` adds;
/// given anything else it opens Documents. Windows file names cannot contain `"`; a path that has
/// them anyway has them dropped, so the command line always holds exactly this one argument.
pub fn explorer_select_arg(path: &OsStr) -> OsString {
    let path = path.to_string_lossy().replace('/', "\\").replace('"', "");
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path.as_str()).to_string(),
    };
    OsString::from(format!("/select,\"{path}\""))
}

/// Reveal: Explorer with the file selected (Windows), Finder with it selected (macOS). Linux has
/// no program for it: the `FileManager1` call does it, else [`open_default`] on the folder.
pub fn reveal(path: &Path, platform: Platform, env: &Env) -> Option<Argv> {
    match platform {
        Platform::Windows => {
            Some(Argv { program: "explorer.exe".into(), args: vec![explorer_select_arg(&path_arg(path, platform))], verbatim: true })
        }
        Platform::MacOs => Some(Argv::new(program("open", env, platform)).arg("-R").arg(path_arg(path, platform))),
        Platform::Linux => None,
    }
}

/// Opens `path` in its default application: `xdg-open` (Linux), `open` (macOS), Explorer given
/// the path as its one argument (Windows; not `cmd /C start`, where cmd.exe would read `&`, `^`,
/// `%` or `|` in a file name as its own syntax).
pub fn open_default(path: &Path, platform: Platform, env: &Env) -> Argv {
    match platform {
        Platform::Windows => Argv::new("explorer.exe").arg(path_arg(path, platform)),
        Platform::MacOs => Argv::new(program("open", env, platform)).arg(path_arg(path, platform)),
        Platform::Linux => Argv::new(program("xdg-open", env, platform)).arg(path_arg(path, platform)),
    }
}

/// A `file://` URI of an absolute path: every byte outside the unreserved set and `/` is
/// percent-encoded (RFC 8089).
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    let raw = path.as_os_str().as_encoded_bytes();
    // A Windows path (`C:\a`) becomes `file:///C:/a`.
    let windows = raw.get(1) == Some(&b':');
    if windows {
        out.push('/');
    }
    for &b in raw {
        let b = if windows && b == b'\\' { b'/' } else { b };
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) || (windows && b == b':') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
