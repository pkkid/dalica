//! Dalica desktop app: the eframe window around `dalica_ui::App` (`docs/architecture.md` §10).
//!
//! Usage: `dalica [--library DIR | --demo | --demo-fresh] [--version | --help]`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod logging;
mod window;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: dalica [--library DIR | --demo | --demo-fresh] [--version | --help]

--demo opens a library of generated photos in the temp folder (made on first use, kept between runs);
--demo-fresh a new one, removed when the window closes. Neither reads or changes your own library or settings.
Otherwise the library is DIR, else $DALICA_HOME, else the platform data folder (Linux ~/.local/share/dalica/library,
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

/// Which library the command line chose.
#[derive(Debug, PartialEq)]
enum Choice {
    /// `$DALICA_HOME` or the platform data folder.
    Default,
    Library(PathBuf),
    /// The persistent demo library (`dalica_engine::demo::default_dir`).
    Demo,
    /// A new demo library, removed on exit.
    DemoFresh,
}

/// What the command line asked for.
#[derive(Debug, PartialEq)]
enum Launch {
    Version,
    Help,
    Open(Choice),
}

/// Reads the arguments; `Err` says what is wrong with them.
fn parse(args: &[String]) -> Result<Launch, String> {
    let mut choice = Choice::Default;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let next = match a.as_str() {
            "--version" | "-V" => return Ok(Launch::Version),
            "--help" | "-h" => return Ok(Launch::Help),
            "--demo" => Choice::Demo,
            "--demo-fresh" => Choice::DemoFresh,
            "--library" => Choice::Library(it.next().map(PathBuf::from).ok_or("--library needs a folder")?),
            other => return Err(format!("unknown argument `{other}`")),
        };
        if choice != Choice::Default {
            return Err("choose one of --library, --demo and --demo-fresh".into());
        }
        choice = next;
    }
    Ok(Launch::Open(choice))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let choice = match parse(&args) {
        Ok(Launch::Version) => {
            println!("dalica {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Launch::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Launch::Open(choice)) => choice,
        Err(why) => {
            eprintln!("dalica: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut fresh = None;
    let source = match choice {
        Choice::Demo => window::Source::Demo(dalica_engine::demo::default_dir()),
        Choice::DemoFresh => match dalica_engine::demo::fresh_dir() {
            Ok(dir) => {
                let source = window::Source::Demo(dir.path().to_path_buf());
                fresh = Some(dir);
                source
            }
            Err(e) => {
                eprintln!("dalica: cannot make a fresh demo folder: {e}");
                return ExitCode::FAILURE;
            }
        },
        Choice::Library(dir) => window::Source::Library(dir),
        Choice::Default => match default_library(|k| std::env::var_os(k)) {
            Some(dir) => window::Source::Library(dir),
            None => {
                eprintln!("dalica: no library folder: pass --library DIR or set DALICA_HOME");
                return ExitCode::from(2);
            }
        },
    };
    let _log = logging::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Dalica").with_inner_size(WINDOW_SIZE).with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };
    let code = match eframe::run_native("Dalica", options, Box::new(move |cc| Ok(Box::new(window::Window::new(&cc.egui_ctx, source))))) {
        Ok(()) => {
            log::info!("dalica: closed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log::error!("dalica: the window failed: {e}");
            ExitCode::FAILURE
        }
    };
    if let Some(dir) = fresh {
        remove_fresh_demo(dir);
    }
    code
}

/// Removes a fresh demo's folder once the window, and the session in it, are gone. On its own
/// thread: this one was the UI thread, which the `fs` guard keeps off the disk.
fn remove_fresh_demo(dir: dalica_fs::TempDir) {
    let path = dir.path().to_path_buf();
    let removed = std::thread::Builder::new().name("dalica-cleanup".into()).spawn(move || dir.close()).map(|t| t.join());
    match removed {
        Ok(Ok(Ok(()))) => log::info!("dalica: removed the fresh demo in {}", path.display()),
        Ok(Ok(Err(e))) => log::warn!("dalica: the fresh demo in {} was not removed: {e}", path.display()),
        _ => log::warn!("dalica: the fresh demo in {} was not removed", path.display()),
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

    fn parsed(args: &[&str]) -> Result<Launch, String> {
        parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_command_line_chooses_one_library() {
        assert_eq!(parsed(&[]), Ok(Launch::Open(Choice::Default)));
        assert_eq!(parsed(&["--demo"]), Ok(Launch::Open(Choice::Demo)));
        assert_eq!(parsed(&["--demo-fresh"]), Ok(Launch::Open(Choice::DemoFresh)));
        assert_eq!(parsed(&["--library", "/lib"]), Ok(Launch::Open(Choice::Library(PathBuf::from("/lib")))));
        assert_eq!(parsed(&["--demo", "--version"]), Ok(Launch::Version));
        assert_eq!(parsed(&["-h"]), Ok(Launch::Help));
        for bad in [&["--library"][..], &["--demo", "--demo-fresh"], &["--demo-fresh", "--library", "/lib"], &["--demo", "--demo"], &["--colour"]] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_fresh_demo_is_removed_after_the_window() {
        let dir = dalica_engine::demo::fresh_dir().unwrap();
        let path = dir.path().to_path_buf();
        std::fs::create_dir_all(dalica_engine::demo::library_dir(&path)).unwrap();
        // Called from the thread that was the UI thread: the removal still happens, on another one.
        let _ui = dalica_fs::mark_ui_thread();
        remove_fresh_demo(dir);
        assert!(!path.exists());
    }
}
