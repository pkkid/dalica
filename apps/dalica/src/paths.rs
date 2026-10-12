//! Where the desktop app keeps the default library (its settings and index) and the thumbnail
//! cache, and portable mode (`docs/releasing.md` → Windows: the portable zip).
//!
//! Portable mode adapted from PhotoCraft `apps/photocraft/src/app_dirs.rs` (revision bb17dd55);
//! MIT OR Apache-2.0.
//!
//! In order, for the default library:
//! 1. `$DALICA_HOME`, when it is an absolute path (tests, agents, custom setups);
//! 2. **portable mode:** a file named [`PORTABLE_MARKER`] beside the executable keeps everything
//!    in a [`PORTABLE_DATA_DIR`] folder next to it: the library (settings and index) in
//!    `data/library`, the thumbnails in `data/cache/thumbs`, the log in `data/logs`, so a copy on a USB stick leaves
//!    nothing in the user's folders. Sidecars stay in the photo folders, as always. The Windows
//!    portable zip ships the marker; the installers don't. If the data folder cannot be written
//!    (a read-only medium, Program Files), the app says so and uses the platform folders;
//! 3. the platform data folder: Linux `$XDG_DATA_HOME/dalica/library` (default
//!    `~/.local/share/dalica/library`), macOS `~/Library/Application Support/Dalica/Library`,
//!    Windows `%LOCALAPPDATA%\Dalica\Library`.
//!
//! The cache follows the same order with `$DALICA_CACHE_DIR` first
//! (`dalica_engine::thumbs::default_cache_dir`). `--library DIR` names the library itself; its
//! cache still follows portable mode.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The file beside the executable that switches on portable mode (its contents are ignored; a
/// folder of that name does not count).
pub const PORTABLE_MARKER: &str = "portable";
/// The folder beside the executable that holds everything in portable mode.
pub const PORTABLE_DATA_DIR: &str = "data";

/// How [`Dirs`] were chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Beside the executable, in [`PORTABLE_DATA_DIR`].
    Portable,
    /// The platform's per-user folders (or the variables that replace them).
    Platform,
}

/// Where this process keeps its data.
#[derive(Clone, Debug, PartialEq)]
pub struct Dirs {
    pub mode: Mode,
    /// The default library; `None` without `DALICA_HOME`, a portable folder or a home folder.
    pub library: Option<PathBuf>,
    /// The thumbnail cache; `None` likewise (the engine then uses a temporary one).
    pub cache: Option<PathBuf>,
    /// The data folder beside the executable, in portable mode only (the log goes in `logs` there).
    pub data: Option<PathBuf>,
    /// Set when a portable marker was found but its data folder cannot be written.
    pub warning: Option<String>,
}

/// The default library folder on this platform, without portable mode.
pub fn platform_library(var: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    if let Some(home) = abs("DALICA_HOME") {
        return Some(home);
    }
    if cfg!(windows) {
        abs("LOCALAPPDATA").map(|p| p.join("Dalica").join("Library"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Application Support").join("Dalica").join("Library"))
    } else {
        abs("XDG_DATA_HOME").or_else(|| abs("HOME").map(|h| h.join(".local").join("share"))).map(|p| p.join("dalica").join("library"))
    }
}

/// Whether `var(k)` is set to an absolute path.
fn is_set(var: &impl Fn(&str) -> Option<OsString>, k: &str) -> bool {
    var(k).filter(|v| !v.is_empty()).is_some_and(|v| Path::new(&v).is_absolute())
}

/// The portable marker in `exe_dir`, if there is one.
pub fn portable_marker(exe_dir: &Path) -> Option<PathBuf> {
    let marker = exe_dir.join(PORTABLE_MARKER);
    marker.is_file().then_some(marker)
}

/// Creates `dir` and proves something can be made in it (a folder can exist yet be read-only).
fn ensure_writable(dir: &Path) -> Result<(), String> {
    dalica_fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let probe = dalica_fs::TempDir::new_in(dir, ".dalica-write-test").map_err(|e| e.to_string())?;
    // A probe left behind is harmless; it never fails portable mode.
    let _ = probe.close();
    Ok(())
}

/// The folders for the environment `var` and the executable's folder `exe_dir` (see the module
/// docs). Portable mode creates its data folder; nothing else is written.
pub fn resolve(var: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> Dirs {
    let platform = || Dirs {
        mode: Mode::Platform,
        library: platform_library(&var),
        cache: dalica_engine::thumbs::default_cache_dir(&var),
        data: None,
        warning: None,
    };
    let Some((exe_dir, marker)) = exe_dir.and_then(|d| portable_marker(d).map(|m| (d, m))) else {
        return platform();
    };
    let data = exe_dir.join(PORTABLE_DATA_DIR);
    if let Err(e) = ensure_writable(&data) {
        let mut dirs = platform();
        let fallback = dirs.library.as_ref().map_or_else(|| "nowhere (no home folder)".to_string(), |p| p.display().to_string());
        dirs.warning = Some(format!(
            "portable mode ({} found) but {} cannot be written ({e}); the library is kept in {fallback} instead",
            marker.display(),
            data.display()
        ));
        return dirs;
    }
    // The variables still win: they are how tests and agents point Dalica somewhere on purpose.
    let library = if is_set(&var, "DALICA_HOME") { platform_library(&var) } else { Some(data.join("library")) };
    let cache = if is_set(&var, dalica_engine::thumbs::CACHE_DIR_VAR) {
        dalica_engine::thumbs::default_cache_dir(&var)
    } else {
        Some(data.join("cache").join("thumbs"))
    };
    Dirs { mode: Mode::Portable, library, cache, data: Some(data), warning: None }
}

/// [`resolve`] for this process: its environment and the folder of its executable.
pub fn current() -> Dirs {
    let exe = std::env::current_exe().ok();
    let dirs = resolve(|k| std::env::var_os(k), exe.as_deref().and_then(Path::parent));
    if let Some(w) = &dirs.warning {
        eprintln!("dalica: {w}");
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| OsString::from(v))
    }

    fn env_paths(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, OsString)> = pairs.iter().map(|(k, v)| (k.to_string(), v.as_os_str().to_owned())).collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    /// A fake home that every platform's convention resolves under.
    fn fake_home(root: &Path) -> impl Fn(&str) -> Option<OsString> + use<> {
        let (home, local) = (root.join("home"), root.join("local"));
        env_paths(&[("HOME", &home), ("LOCALAPPDATA", &local)])
    }

    #[test]
    fn the_library_is_dalica_home_or_the_platform_data_folder() {
        let lib = if cfg!(windows) { "C:\\lib" } else { "/lib" };
        assert_eq!(platform_library(&env(&[("DALICA_HOME", lib)])), Some(PathBuf::from(lib)));
        // A relative or empty value is ignored.
        assert_eq!(platform_library(&env(&[("DALICA_HOME", "relative")])), platform_library(&env(&[])));
        assert_eq!(platform_library(&env(&[("DALICA_HOME", "")])), None);
        assert_eq!(platform_library(&env(&[])), None);
        if cfg!(windows) {
            let local = "C:\\Users\\a\\AppData\\Local";
            assert_eq!(platform_library(&env(&[("LOCALAPPDATA", local)])), Some(PathBuf::from(local).join("Dalica").join("Library")));
        } else if cfg!(target_os = "macos") {
            assert_eq!(platform_library(&env(&[("HOME", "/Users/a")])), Some(PathBuf::from("/Users/a/Library/Application Support/Dalica/Library")));
        } else {
            assert_eq!(platform_library(&env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.local/share/dalica/library")));
            assert_eq!(platform_library(&env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/data")])), Some(PathBuf::from("/data/dalica/library")));
        }
    }

    #[test]
    fn without_a_marker_the_platform_folders_as_before() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        let var = fake_home(root.path());
        let dirs = resolve(&var, Some(&exe));
        let want = Dirs {
            mode: Mode::Platform,
            library: platform_library(&var),
            cache: dalica_engine::thumbs::default_cache_dir(&var),
            data: None,
            warning: None,
        };
        assert_eq!(dirs, want);
        assert!(dirs.library.unwrap().starts_with(root.path().join(if cfg!(windows) { "local" } else { "home" })));
        assert!(!exe.join(PORTABLE_DATA_DIR).exists(), "no data folder without a marker");
        // No executable folder, no environment: no folders, no panic.
        assert_eq!(resolve(env(&[]), None), Dirs { mode: Mode::Platform, library: None, cache: None, data: None, warning: None });
    }

    #[test]
    fn with_the_marker_everything_is_next_to_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("Dalica");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join(PORTABLE_MARKER), b"").unwrap();
        let dirs = resolve(fake_home(root.path()), Some(&exe));
        let data = exe.join(PORTABLE_DATA_DIR);
        assert_eq!(
            dirs,
            Dirs {
                mode: Mode::Portable,
                library: Some(data.join("library")),
                cache: Some(data.join("cache").join("thumbs")),
                data: Some(data.clone()),
                warning: None
            }
        );
        assert!(data.is_dir(), "the data folder is made");
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0, "the write probe is removed");
        assert!(!root.path().join("home").exists() && !root.path().join("local").exists(), "nothing in the user's folders");
        // Even with no home folder at all.
        assert_eq!(resolve(env(&[]), Some(&exe)).library, Some(data.join("library")));
    }

    #[test]
    fn the_variables_win_over_the_marker() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join(PORTABLE_MARKER), b"").unwrap();
        let (home, cache) = (root.path().join("lib"), root.path().join("cache"));
        let dirs = resolve(env_paths(&[("DALICA_HOME", &home)]), Some(&exe));
        assert_eq!((dirs.mode, dirs.library), (Mode::Portable, Some(home)));
        assert_eq!(dirs.cache, Some(exe.join(PORTABLE_DATA_DIR).join("cache").join("thumbs")));
        let dirs = resolve(env_paths(&[(dalica_engine::thumbs::CACHE_DIR_VAR, &cache)]), Some(&exe));
        assert_eq!(dirs.cache, Some(cache.join("thumbs")));
        assert_eq!(dirs.library, Some(exe.join(PORTABLE_DATA_DIR).join("library")));
    }

    #[test]
    fn a_marker_folder_is_not_a_marker() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin");
        std::fs::create_dir_all(exe.join(PORTABLE_MARKER)).unwrap();
        assert_eq!(resolve(fake_home(root.path()), Some(&exe)).mode, Mode::Platform);
        assert!(!exe.join(PORTABLE_DATA_DIR).exists());
    }

    #[test]
    fn an_unwritable_data_folder_falls_back_with_a_warning() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join(PORTABLE_MARKER), b"").unwrap();
        // A file where the data folder goes: it cannot be made, on every platform.
        std::fs::write(exe.join(PORTABLE_DATA_DIR), b"in the way").unwrap();
        let var = fake_home(root.path());
        let dirs = resolve(&var, Some(&exe));
        assert_eq!((dirs.mode, &dirs.library), (Mode::Platform, &platform_library(&var)));
        let w = dirs.warning.unwrap();
        assert!(w.contains("portable") && w.contains("cannot be written"), "{w}");
        // No home either: a warning, no folder, no panic.
        let dirs = resolve(env(&[]), Some(&exe));
        assert!(dirs.library.is_none() && dirs.warning.is_some_and(|w| w.contains("nowhere")));
    }
}
