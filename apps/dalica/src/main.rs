//! Dalica desktop app: the eframe window around `dalica_ui::App` (`docs/architecture.md` §10), its
//! control channel (`docs/control-protocol.md`), and `--headless`, the same app with no window.
//!
//! Usage: see [`USAGE`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control;
mod frame;
mod icon;
mod logging;
mod native_menu;
mod paths;
mod preflight;
mod window;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str =
    "usage: dalica [--library DIR | --demo | --demo-fresh] [--view FILE] [--control [PORT] [CONTROL OPTIONS] [--headless]] [--version | --help]

--demo opens a library of generated photos in the temp folder (made on first use, kept between runs);
--demo-fresh a new one, removed when the window closes. Neither reads or changes your own library or settings.
Otherwise the library is DIR, else $DALICA_HOME, else the `data` folder beside the executable when a file named
`portable` is there (portable mode: the library and caches stay next to the program), else the platform data folder
(Linux ~/.local/share/dalica/library, macOS ~/Library/Application Support/Dalica/Library,
Windows %LOCALAPPDATA%\\Dalica\\Library).

--view FILE is how the file manager's Open with hands Dalica an image: for now it opens the library window as usual
(the standalone viewer comes later). FILE may be missing or have any name; it is never read or run.

On Linux the window first checks for the libraries it loads (Wayland or X11, xkbcommon, Vulkan); a missing one is
named with the package to install and dalica exits with status 1 (DALICA_SKIP_LIB_CHECK=1 skips the check).

--control [PORT] lets agents and scripts drive the app over 127.0.0.1:PORT (default $DALICA_CONTROL_PORT, else 7990),
with a token from a private token file (docs/control-protocol.md). Control options:
  --control-token-file PATH      the token file (made when missing); default <run folder>/control-<PORT>.token
  --automation-read-root DIR     a folder automation may read from (repeatable)
  --automation-write-root DIR    a folder automation may write new files into (repeatable)
  --automation-allow-external    allow commands with effects outside Dalica
  --headless                     no window: the app runs on the CPU renderer and answers only the control channel";

/// What `--version` prints: the version with its commit and build date (`releasing.md`).
fn version_line() -> String {
    format!("dalica {}", dalica_core::build::version_string())
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
    Open(Options),
}

/// What to open and how.
#[derive(Debug, PartialEq)]
struct Options {
    choice: Choice,
    /// `--control` and its options.
    control: Option<control::Flags>,
    headless: bool,
    /// `--view FILE`: the file the file manager's Open with handed over. Taken verbatim (any
    /// bytes, a leading `-`, a missing file) and never read or run; the standalone viewer it will
    /// open (`viewing.md` §3) is later work, so for now Dalica opens its library window as usual.
    view: Option<PathBuf>,
}

/// Reads the arguments (as the OS gave them, so a file name need not be UTF-8); `Err` says what
/// is wrong with them.
fn parse(args: &[OsString]) -> Result<Launch, String> {
    let mut choice = Choice::Default;
    let mut control: Option<control::Flags> = None;
    let mut flags = control::Flags::default();
    let mut needs_control: Option<&str> = None;
    let mut headless = false;
    let mut view = None;
    let mut it = args.iter().peekable();
    while let Some(raw) = it.next() {
        // Every option is ASCII, so a non-UTF-8 argument is never one (it is reported lossily).
        let a = raw.to_string_lossy();
        let mut path = |what: &str| it.next().map(PathBuf::from).ok_or_else(|| format!("{a} needs {what}"));
        let next = match a.as_ref() {
            "--version" | "-V" => return Ok(Launch::Version),
            "--help" | "-h" => return Ok(Launch::Help),
            "--demo" => Choice::Demo,
            "--demo-fresh" => Choice::DemoFresh,
            "--library" => Choice::Library(path("a folder")?),
            "--view" => {
                if view.is_some() {
                    return Err("--view is given twice".into());
                }
                view = Some(path("a file")?);
                continue;
            }
            "--control" => {
                if control.is_some() {
                    return Err("--control is given twice".into());
                }
                let port = match it.peek().map(|p| p.to_string_lossy()) {
                    Some(p) if !p.starts_with('-') => Some(control::parse_port(&p).map_err(|e| format!("--control: {e}"))?),
                    _ => None,
                };
                if port.is_some() {
                    it.next();
                }
                control = Some(control::Flags { port, ..control::Flags::default() });
                continue;
            }
            "--control-token-file" => {
                flags.token_file = Some(path("a file")?);
                needs_control = Some("--control-token-file");
                continue;
            }
            "--automation-read-root" => {
                flags.read_roots.push(path("a folder")?);
                needs_control = Some("--automation-read-root");
                continue;
            }
            "--automation-write-root" => {
                flags.write_roots.push(path("a folder")?);
                needs_control = Some("--automation-write-root");
                continue;
            }
            "--automation-allow-external" => {
                flags.allow_external = true;
                needs_control = Some("--automation-allow-external");
                continue;
            }
            "--headless" => {
                headless = true;
                needs_control = needs_control.or(Some("--headless"));
                continue;
            }
            other => return Err(format!("unknown argument `{other}`")),
        };
        if choice != Choice::Default {
            return Err("choose one of --library, --demo and --demo-fresh".into());
        }
        choice = next;
    }
    let control = match (control, needs_control) {
        (Some(c), _) => Some(control::Flags { port: c.port, ..flags }),
        (None, Some(flag)) => return Err(format!("{flag} needs --control")),
        (None, None) => None,
    };
    Ok(Launch::Open(Options { choice, control, headless, view }))
}

fn main() -> ExitCode {
    // `args_os`: `args` would panic on a file name that is not valid Unicode.
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let options = match parse(&args) {
        Ok(Launch::Version) => {
            println!("{}", version_line());
            return ExitCode::SUCCESS;
        }
        Ok(Launch::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Launch::Open(options)) => options,
        Err(why) => {
            eprintln!("dalica: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Before anything else, the libraries the window loads at run time (Linux; `preflight`): a
    // missing one is named with its package, never a crash inside winit or wgpu.
    if cfg!(target_os = "linux")
        && !options.headless
        && let Err(message) = preflight::check(|k| std::env::var_os(k))
    {
        eprint!("{message}");
        return ExitCode::from(preflight::EXIT_MISSING);
    }
    // The control port first: a taken port exits 2 before anything is made (§1).
    let listening = match &options.control {
        None => None,
        Some(flags) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let resolved = control::resolve(flags.clone(), |k| std::env::var_os(k), &cwd)
                .and_then(|(port, flags)| control::listen(port, flags.token_file.as_deref()).map(|l| (l, flags)));
            match resolved {
                Ok(l) => Some(l),
                Err(why) => {
                    eprintln!("dalica: {why}");
                    return ExitCode::from(2);
                }
            }
        }
    };
    let demo = matches!(options.choice, Choice::Demo | Choice::DemoFresh);
    // The user's library, its caches and the log: the platform folders, or beside the executable
    // in portable mode (`paths`). A demo uses only the log folder.
    let dirs = paths::current();
    let data = dirs.data.clone();
    let dirs = (!demo).then_some(dirs);
    let mut fresh = None;
    let source = match options.choice {
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
        Choice::Default => match dirs.as_ref().and_then(|d| d.library.clone()) {
            Some(dir) => window::Source::Library(dir),
            None => {
                eprintln!("dalica: no library folder: pass --library DIR or set DALICA_HOME");
                return ExitCode::from(2);
            }
        },
    };
    // The user's library keeps its thumbnails in Dalica's cache folder; a demo keeps a temporary
    // cache that goes with it.
    if let Some(dirs) = &dirs {
        dalica_engine::thumbs::set_process_cache_dir(dirs.cache.clone());
    }
    let _log = logging::init(data.as_deref());
    if let Some(dirs) = dirs.as_ref().filter(|d| d.mode == paths::Mode::Portable) {
        log::info!("dalica: portable mode, library {:?}, thumbnails {:?}", dirs.library, dirs.cache);
    }
    if let Some(w) = dirs.as_ref().and_then(|d| d.warning.as_ref()) {
        log::warn!("dalica: {w}");
    }
    // A file name can be private: it goes to the log at debug level only.
    if let Some(file) = &options.view {
        log::debug!("dalica: --view {}: opening the library window (the standalone viewer comes later)", file.display());
    }
    let (code, token_file) = match listening {
        Some((listening, flags)) if options.headless => run_headless(listening, &flags, source, demo),
        Some((listening, flags)) => {
            let control = window::ControlSetup { listening, control: dalica_ui::control::Control::new(flags.config(demo)) };
            run_window(source, Some(control))
        }
        None => run_window(source, None),
    };
    if let Some(t) = token_file {
        t.finish();
    }
    if let Some(dir) = fresh {
        remove_fresh_demo(dir);
    }
    code
}

/// The window, until it closes. Returns the exit code and the token file to remove.
fn run_window(source: window::Source, control: Option<window::ControlSetup>) -> (ExitCode, Option<control::TokenFileGuard>) {
    // The chrome and placement come from the library's settings, read (never changed) before the
    // window and the library open; the session that opens it sets a damaged file aside.
    let settings = dalica_engine::settings::Settings::peek(&source.library_dir());
    let frame = frame::FrameSetup::from_settings(&settings, frame::Os::host());
    let options = eframe::NativeOptions { viewport: window::viewport(&frame), ..Default::default() };
    let token_file = Arc::new(std::sync::Mutex::new(None));
    let keep = token_file.clone();
    let ran = eframe::run_native(
        "Dalica",
        options,
        Box::new(move |cc| {
            let (w, guard) = window::Window::new(&cc.egui_ctx, source, control, frame);
            if let Ok(mut slot) = keep.lock() {
                *slot = guard;
            }
            Ok(Box::new(w))
        }),
    );
    let code = match ran {
        Ok(()) => {
            log::info!("dalica: closed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log::error!("dalica: the window failed: {e}");
            ExitCode::FAILURE
        }
    };
    let guard = token_file.lock().map(|mut g| g.take()).unwrap_or_default();
    (code, guard)
}

/// `--headless`: opens the library, serves the control channel until `app.quit`, flushes.
fn run_headless(
    listening: control::Listening,
    flags: &control::Flags,
    source: window::Source,
    demo: bool,
) -> (ExitCode, Option<control::TokenFileGuard>) {
    let session = match window::open(source) {
        Ok(s) => s,
        Err(e) => {
            log::error!("dalica: cannot open the library: {e}");
            eprintln!("dalica: cannot open the library: {e}");
            return (ExitCode::FAILURE, Some(listening.token_file));
        }
    };
    let port = listening.port;
    let (hub, token_file) = match listening.start(Arc::new(|| {})) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("dalica: {e}");
            return (ExitCode::FAILURE, None);
        }
    };
    eprintln!("dalica: headless, control on 127.0.0.1:{port}, token file {}", token_file.path.display());
    let control = dalica_ui::control::Control::new(flags.config(demo));
    let saved = control::headless::run(&hub, &control, session);
    (if saved { ExitCode::SUCCESS } else { ExitCode::FAILURE }, Some(token_file))
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

    #[test]
    fn version_names_the_build() {
        let v = version_line();
        assert_eq!(v, format!("dalica {}", dalica_core::build::version_string()));
        assert!(v.starts_with(&format!("dalica {} (", env!("CARGO_PKG_VERSION"))), "{v}");
    }

    fn parsed(args: &[&str]) -> Result<Launch, String> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    fn open(choice: Choice) -> Result<Launch, String> {
        Ok(Launch::Open(Options { choice, control: None, headless: false, view: None }))
    }

    #[test]
    fn the_command_line_chooses_one_library() {
        assert_eq!(parsed(&[]), open(Choice::Default));
        assert_eq!(parsed(&["--demo"]), open(Choice::Demo));
        assert_eq!(parsed(&["--demo-fresh"]), open(Choice::DemoFresh));
        assert_eq!(parsed(&["--library", "/lib"]), open(Choice::Library(PathBuf::from("/lib"))));
        assert_eq!(parsed(&["--demo", "--version"]), Ok(Launch::Version));
        assert_eq!(parsed(&["-h"]), Ok(Launch::Help));
        for bad in [&["--library"][..], &["--demo", "--demo-fresh"], &["--demo-fresh", "--library", "/lib"], &["--demo", "--demo"], &["--colour"]] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_control_flags_go_with_control() {
        let flags = |port| control::Flags { port, ..control::Flags::default() };
        let with = |c: control::Flags, headless| Ok(Launch::Open(Options { choice: Choice::Demo, control: Some(c), headless, view: None }));
        assert_eq!(parsed(&["--control", "18040", "--demo"]), with(flags(Some(18040)), false));
        assert_eq!(parsed(&["--demo", "--control"]), with(flags(None), false));
        assert_eq!(parsed(&["--control", "--demo", "--headless"]), with(flags(None), true));
        let full = control::Flags {
            port: Some(18042),
            token_file: Some(PathBuf::from("/t/a.token")),
            read_roots: vec![PathBuf::from("/r1"), PathBuf::from("/r2")],
            write_roots: vec![PathBuf::from("/w")],
            allow_external: true,
        };
        let args = [
            "--automation-read-root",
            "/r1",
            "--demo",
            "--control-token-file",
            "/t/a.token",
            "--control",
            "18042",
            "--automation-read-root",
            "/r2",
            "--automation-write-root",
            "/w",
            "--automation-allow-external",
            "--headless",
        ];
        assert_eq!(parsed(&args), with(full, true));
        for bad in [
            &["--control", "0"][..],
            &["--control", "70000"],
            &["--control", "x"],
            &["--control", "1", "--control", "2"],
            &["--headless"],
            &["--headless", "--demo"],
            &["--control-token-file", "/t"],
            &["--automation-read-root", "/r"],
            &["--automation-write-root", "/w"],
            &["--automation-allow-external"],
            &["--control", "--control-token-file"],
            &["--control", "--automation-read-root"],
        ] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
        assert!(parsed(&["--headless"]).unwrap_err().contains("needs --control"));
    }

    #[test]
    fn view_takes_any_file_verbatim_and_opens_as_usual() {
        let viewing = |choice, file: &str| Ok(Launch::Open(Options { choice, control: None, headless: false, view: Some(PathBuf::from(file)) }));
        // What Open with passes: existing or missing files, spaces, quotes, Unicode, a leading
        // `-` or what looks like an option; none of it is read, run or taken as an option.
        for file in [
            "/photos/IMG_0001.JPG",
            "C:\\Users\\a b\\Pictures\\x.jpg",
            "/no/such/file.cr3",
            "./ünïcødé 写真 (1).jpeg",
            "it's \"quoted\" & $(not run).png",
            "-dash.jpg",
            "--demo",
            "",
        ] {
            assert_eq!(parsed(&["--view", file]), viewing(Choice::Default, file), "{file:?}");
        }
        assert_eq!(parsed(&["--library", "/lib", "--view", "/x.jpg"]), viewing(Choice::Library(PathBuf::from("/lib")), "/x.jpg"));
        for bad in [&["--view"][..], &["--view", "/a.jpg", "--view", "/b.jpg"]] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
    }

    /// A file name that is not valid Unicode (Unix bytes, Windows lone surrogates) is accepted.
    #[test]
    fn view_accepts_a_file_name_that_is_not_unicode() {
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"/photos/\xff\xfe.jpg".to_vec())
        };
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xd800, 0x2e, 0x6a, 0x70, 0x67])
        };
        let got = parse(&[OsString::from("--view"), odd.clone()]);
        let want = Options { choice: Choice::Default, control: None, headless: false, view: Some(PathBuf::from(odd)) };
        assert_eq!(got, Ok(Launch::Open(want)));
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
