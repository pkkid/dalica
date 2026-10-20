//! The real [`dalica_engine::os::Os`] for Linux, Windows and macOS (`organizing.md` §9,
//! `viewing.md` §3.3, `security.md` §5.11): reveal a file in the file manager, the applications
//! that open its type, open it with one of them, the external editor and the clipboard.
//!
//! No `unsafe` and no C library: the system's own programs started with an argument vector
//! ([`launch`]; never a shell, a path starting with `-` passed as `./-name`), pure-Rust crates
//! for the D-Bus call (`zbus`), the clipboard (`arboard`, `wl-clipboard-rs`) and Launch Services
//! (`objc2-app-kit`'s safe methods). What a system offers no such way to do answers
//! [`OsError::Unavailable`] with the reason. Every call may block (a program's start, a D-Bus
//! call, `reg query`, the clipboard thread's answer): the engine's commands make them off the UI
//! thread (M15.17).
//!
//! Origins: reveal from LightCraft `apps/lightcraft/src/main.rs` (the `reveal` service) and the
//! external editor from LightCraft `apps/lightcraft/src/editors.rs`, both at d999e382; the
//! clipboard from PhotoCraft `apps/photocraft/src/services.rs` at 5b087699 and d95fdf31 (all MIT
//! OR Apache-2.0; `NOTICE`).

// The modules name their files, so the tests can include this file by its path (`tests/os.rs`).
#[path = "os/clipboard.rs"]
pub mod clipboard;
#[path = "os/default_viewer.rs"]
pub mod default_viewer;
#[path = "os/desktop.rs"]
pub mod desktop;
#[path = "os/editor.rs"]
pub mod editor;
#[path = "os/launch.rs"]
pub mod launch;
#[cfg(target_os = "macos")]
#[path = "os/macos.rs"]
mod macos;
#[path = "os/registry.rs"]
pub mod registry;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_engine::os::{Clip, DefaultSet, Handler, Os, OsError};
use dalica_scan::FileType;

use launch::{Argv, Platform, Runner, Spawn};

/// The environment the services read (`PATH`, the XDG folders, the language), taken from the
/// process once; the tests give their own.
#[derive(Clone, Debug, Default)]
pub struct Env {
    vars: BTreeMap<String, OsString>,
}

impl Env {
    /// This process's environment.
    pub fn from_process() -> Env {
        Env::from_vars(std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v))))
    }

    /// An environment of exactly these variables.
    pub fn from_vars<K: Into<String>, V: Into<OsString>>(vars: impl IntoIterator<Item = (K, V)>) -> Env {
        Env { vars: vars.into_iter().map(|(k, v)| (k.into(), v.into())).collect() }
    }

    pub fn get(&self, key: &str) -> Option<&OsStr> {
        self.vars.get(key).map(OsString::as_os_str)
    }

    /// A variable by a name in any case, as Windows reads them.
    pub fn get_ignore_case(&self, key: &str) -> Option<&OsStr> {
        self.get(key).or_else(|| self.vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_os_str()))
    }
}

/// Shows files in the file manager by their `file://` URIs (Linux's
/// `org.freedesktop.FileManager1.ShowItems`).
pub type ShowItems = Box<dyn Fn(&[String]) -> Result<(), String> + Send + Sync>;

/// The real OS services.
pub struct Native {
    platform: Platform,
    env: Env,
    runner: Arc<dyn Runner>,
    show_items: ShowItems,
    clipboard: clipboard::Clipboard,
}

impl Native {
    /// The services of the OS this build runs on, with this process's environment.
    pub fn new() -> Native {
        Native::with(Platform::host(), Env::from_process(), Arc::new(Spawn), Box::new(show_items))
    }

    /// The services of `platform` with `env`, starting programs through `runner` and asking the
    /// file manager through `show_items` (the tests: a stub program on `PATH`, a recording runner).
    pub fn with(platform: Platform, env: Env, runner: Arc<dyn Runner>, show_items: ShowItems) -> Native {
        Native { platform, env, runner, show_items, clipboard: clipboard::Clipboard::default() }
    }

    fn start(&self, argv: &Argv) -> Result<(), OsError> {
        log::info!("os: starting {}", argv.describe());
        self.runner.start(argv).map_err(OsError::Failed)
    }

    fn dirs(&self) -> desktop::Dirs {
        desktop::Dirs::from_env(&self.env)
    }

    /// Linux: the installed desktop entry `id`.
    fn entry(&self, id: &str) -> Result<desktop::Entry, OsError> {
        desktop::installed(&self.dirs())
            .remove(id)
            .map(|(_, e)| e)
            .filter(|e| !e.hidden)
            .ok_or_else(|| OsError::Failed(format!("{id} is not installed")))
    }

    /// Windows: the `edit` command of the first application of `path`'s type that has one.
    fn windows_edit(&self, path: &Path) -> Option<Argv> {
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))?;
        registry::handler_ids(self.runner.as_ref(), &ext)
            .iter()
            .find_map(|id| registry::verb_command(self.runner.as_ref(), id, "edit"))
            .and_then(|c| registry::command_argv(&c, path, &self.env))
    }
}

impl Default for Native {
    fn default() -> Native {
        Native::new()
    }
}

/// Linux: `org.freedesktop.FileManager1.ShowItems` on the session bus.
#[cfg(target_os = "linux")]
fn show_items(uris: &[String]) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    bus.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(uris.to_vec(), ""),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn show_items(_uris: &[String]) -> Result<(), String> {
    Err("there is no FileManager1 service on this system".into())
}

/// A path the services take: not empty, no NUL byte (no program could be given it).
fn check_path(path: &Path) -> Result<(), OsError> {
    let raw = path.as_os_str().as_encoded_bytes();
    if raw.is_empty() || raw.contains(&0) {
        return Err(OsError::Failed(format!("{:?} is not a file name", path.as_os_str())));
    }
    Ok(())
}

impl Os for Native {
    fn reveal(&self, path: &Path) -> Result<(), OsError> {
        check_path(path)?;
        if let Some(argv) = launch::reveal(path, self.platform, &self.env) {
            return self.start(&argv);
        }
        // Linux: the file manager selects it, else its folder is opened.
        match (self.show_items)(&[launch::file_uri(path)]) {
            Ok(()) => Ok(()),
            Err(why) => {
                log::info!("os: no file manager selects files here ({why}); opening the folder");
                let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                self.start(&launch::open_default(folder, self.platform, &self.env))
            }
        }
    }

    fn handlers(&self, path: &Path) -> Result<Vec<Handler>, OsError> {
        check_path(path)?;
        match self.platform {
            Platform::Linux => {
                Ok(desktop::handlers(path, &self.dirs()).into_iter().map(|e| Handler { id: e.id, name: e.name, icon: e.icon }).collect())
            }
            Platform::Windows => {
                let runner = self.runner.as_ref();
                let apps = registry::handlers(path, runner, &self.env).into_iter();
                Ok(apps.map(|a| Handler { icon: default_viewer::windows_icon(runner, &a.id, &self.env), id: a.id, name: a.name }).collect())
            }
            Platform::MacOs => mac_handlers(path),
        }
    }

    fn open_with(&self, path: &Path, handler: &str) -> Result<(), OsError> {
        check_path(path)?;
        if handler.trim().is_empty() || handler.contains('\0') {
            return Err(OsError::Failed("no application was named".into()));
        }
        let argv = match self.platform {
            Platform::Linux => desktop::exec_argv(&self.entry(handler)?, &[path.to_path_buf()], &self.env).map_err(OsError::Failed)?,
            Platform::Windows => registry::verb_command(self.runner.as_ref(), handler, "open")
                .and_then(|c| registry::command_argv(&c, path, &self.env))
                .ok_or_else(|| OsError::Failed(format!("{handler} has no command that opens files")))?,
            Platform::MacOs => {
                Argv::new(launch::program("open", &self.env, self.platform)).arg("-a").arg(handler).arg(launch::path_arg(path, self.platform))
            }
        };
        self.start(&argv)
    }

    fn open_in_editor(&self, paths: &[PathBuf], editor: &str) -> Result<(), OsError> {
        if paths.is_empty() {
            return Err(OsError::Failed("there are no files to open".into()));
        }
        paths.iter().try_for_each(|p| check_path(p))?;
        let launcher = editor::launcher(editor, self.platform, &self.env, self.runner.as_ref()).map_err(OsError::Failed)?;
        let starts = match &launcher {
            editor::Launcher::Default if self.platform == Platform::Windows => {
                paths.iter().map(|p| self.windows_edit(p).unwrap_or_else(|| launch::open_default(p, self.platform, &self.env))).collect()
            }
            editor::Launcher::Default => editor::default_commands(paths, self.platform, &self.env),
            editor::Launcher::Entry(id) => vec![desktop::exec_argv(&self.entry(id)?, paths, &self.env).map_err(OsError::Failed)?],
            other => editor::all_command(other, paths, self.platform, &self.env).into_iter().collect::<Vec<_>>(),
        };
        starts.iter().try_for_each(|argv| self.start(argv))
    }

    fn put_on_clipboard(&self, clip: &Clip) -> Result<(), OsError> {
        self.clipboard.put(clip)
    }

    fn default_handler(&self, ty: FileType) -> Result<Option<Handler>, OsError> {
        default_viewer::default_handler(self, ty)
    }

    fn set_default_viewer(&self, types: &[FileType]) -> Result<DefaultSet, OsError> {
        default_viewer::set_default_viewer(self, types)
    }
}

/// macOS: the applications Launch Services offers, named by their bundles.
#[cfg(target_os = "macos")]
fn mac_handlers(path: &Path) -> Result<Vec<Handler>, OsError> {
    let apps = macos::handlers(path, desktop::MAX_HANDLERS).map_err(OsError::Failed)?;
    Ok(apps
        .into_iter()
        .map(|app| {
            let name = app.file_stem().map_or_else(|| app.to_string_lossy().into_owned(), |s| s.to_string_lossy().into_owned());
            Handler { id: app.to_string_lossy().into_owned(), name, icon: None }
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
fn mac_handlers(_path: &Path) -> Result<Vec<Handler>, OsError> {
    Err(OsError::Unavailable("Launch Services is only on macOS".into()))
}
