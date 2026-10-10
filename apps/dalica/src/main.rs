//! Dalica desktop app: the eframe window around `dalica_ui::App` (`docs/architecture.md` §10).
//!
//! Usage: `dalica [--library DIR] [--version | --help]`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod logging;
mod window;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: dalica [--library DIR] [--version | --help]

The library is DIR, else $DALICA_HOME, else the platform data folder (Linux ~/.local/share/dalica/library,
macOS ~/Library/Application Support/Dalica/Library, Windows %LOCALAPPDATA%\\Dalica\\Library).";

/// Default and minimum window sizes (`docs/ui-layout.md` §11).
const WINDOW_SIZE: [f32; 2] = [1400.0, 900.0];
const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];

/// The default library folder (`architecture.md` §4: the platform data directory).
fn default_library(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut library = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("dalica {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--library" => match it.next() {
                Some(dir) => library = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("dalica: --library needs a folder\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("dalica: unknown argument `{other}`\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(library) = library.or_else(|| default_library(|k| std::env::var_os(k))) else {
        eprintln!("dalica: no library folder: pass --library DIR or set DALICA_HOME");
        return ExitCode::from(2);
    };
    let _log = logging::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Dalica").with_inner_size(WINDOW_SIZE).with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };
    match eframe::run_native("Dalica", options, Box::new(move |cc| Ok(Box::new(window::Window::new(&cc.egui_ctx, library))))) {
        Ok(()) => {
            log::info!("dalica: closed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log::error!("dalica: the window failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn the_library_is_dalica_home_or_the_platform_data_folder() {
        let lib = if cfg!(windows) { "C:\\lib" } else { "/lib" };
        assert_eq!(default_library(env(&[("DALICA_HOME", lib)])), Some(PathBuf::from(lib)));
        // A relative or empty value is ignored.
        assert_eq!(default_library(env(&[("DALICA_HOME", "relative")])), default_library(env(&[])));
        assert_eq!(default_library(env(&[])), None);
        if cfg!(windows) {
            let local = "C:\\Users\\a\\AppData\\Local";
            assert_eq!(default_library(env(&[("LOCALAPPDATA", local)])), Some(PathBuf::from(local).join("Dalica").join("Library")));
        } else if cfg!(target_os = "macos") {
            assert_eq!(default_library(env(&[("HOME", "/Users/a")])), Some(PathBuf::from("/Users/a/Library/Application Support/Dalica/Library")));
        } else {
            assert_eq!(default_library(env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.local/share/dalica/library")));
            assert_eq!(default_library(env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/data")])), Some(PathBuf::from("/data/dalica/library")));
        }
    }
}
