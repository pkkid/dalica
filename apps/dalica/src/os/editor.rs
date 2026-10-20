//! Open in External Editor: the program Options names (`files.externalEditor`), else the system's
//! edit verb or default application. Ported from LightCraft `apps/lightcraft/src/editors.rs` at
//! d999e382 (MIT OR Apache-2.0): a named program given every file in one start (an editor that
//! keeps one window loses files that arrive while it starts), `open -a` on macOS, a Flatpak's
//! file forwarding, Explorer for the default on Windows rather than `cmd /C start`. Dalica adds a
//! desktop entry named by its id on Linux and the `edit` verb of the registry on Windows.

use std::path::PathBuf;

use super::Env;
use super::launch::{Argv, Platform, Runner, open_default, path_arg, program, which};

/// How the editor is started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launcher {
    /// The system's edit verb or default application, one start per file.
    Default,
    /// A program file, given every file.
    Program(PathBuf),
    /// macOS: `open -a <app>` (an application's name or bundle path).
    MacApp(String),
    /// A Flatpak application id, the files forwarded into its sandbox.
    Flatpak(String),
    /// Linux: a desktop entry's id (`org.gimp.GIMP.desktop`).
    Entry(String),
}

/// Whether `id` reads as a reverse-DNS application id (`org.gimp.GIMP`), which a Flatpak has.
fn is_app_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

/// How `editor` (Options' text, trimmed; empty: the system's) is started on `platform`: on
/// Linux a `.desktop` id, a path, a program on `PATH`, else an installed Flatpak's id (`flatpak
/// info` asked through `runner`); on Windows a path or a program on `PATH`; on macOS `open -a`.
pub fn launcher(editor: &str, platform: Platform, env: &Env, runner: &dyn Runner) -> Result<Launcher, String> {
    let editor = editor.trim();
    if editor.is_empty() {
        return Ok(Launcher::Default);
    }
    match platform {
        Platform::MacOs => Ok(Launcher::MacApp(editor.to_string())),
        Platform::Windows => which(editor, env, platform).map(Launcher::Program).ok_or_else(|| format!("{editor} was not found")),
        Platform::Linux => {
            if editor.ends_with(".desktop") && !editor.contains('/') {
                return Ok(Launcher::Entry(editor.to_string()));
            }
            if let Some(found) = which(editor, env, platform) {
                return Ok(Launcher::Program(found));
            }
            let flatpak = Argv::new(program("flatpak", env, platform)).arg("info").arg(editor);
            if is_app_id(editor) && runner.output(&flatpak).is_ok() {
                return Ok(Launcher::Flatpak(editor.to_string()));
            }
            Err(format!("{editor} was not found"))
        }
    }
}

/// The one start that opens every file of `paths` in `launcher` (`None` for [`Launcher::Default`]
/// and [`Launcher::Entry`], which the caller starts).
pub fn all_command(launcher: &Launcher, paths: &[PathBuf], platform: Platform, env: &Env) -> Option<Argv> {
    let files = paths.iter().map(|p| path_arg(p, platform));
    match launcher {
        Launcher::Default | Launcher::Entry(_) => None,
        Launcher::MacApp(app) => Some(Argv::new(program("open", env, platform)).arg("-a").arg(app).args(files)),
        Launcher::Program(p) => Some(Argv::new(p.clone()).args(files)),
        Launcher::Flatpak(id) => {
            Some(Argv::new(program("flatpak", env, platform)).args(["run", "--file-forwarding", id.as_str(), "@@"]).args(files).arg("@@"))
        }
    }
}

/// The starts that open `paths` in the system's default application: one per file (Explorer
/// and `xdg-open` take one file), all at once with macOS's `open`.
pub fn default_commands(paths: &[PathBuf], platform: Platform, env: &Env) -> Vec<Argv> {
    match platform {
        Platform::MacOs => vec![Argv::new(program("open", env, platform)).args(paths.iter().map(|p| path_arg(p, platform)))],
        _ => paths.iter().map(|p| open_default(p, platform, env)).collect(),
    }
}
