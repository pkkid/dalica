//! Dalica desktop app: the eframe window around `dalica_ui::App` (`docs/architecture.md` §10), its
//! control channel (`docs/control-protocol.md`), and `--headless`, the same app with no window.
//!
//! Usage: see [`USAGE`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control;
mod drag_out;
mod frame;
mod gpu;
mod icon;
mod instance;
mod logging;
mod native_menu;
mod os;
mod paths;
mod preflight;
mod viewer;
mod window;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str =
    "usage: dalica [--library DIR | --demo | --demo-fresh] [--view FILE | FILE] [--control [PORT] [CONTROL OPTIONS] [--headless]] [--version | --help]

--demo opens a library of generated photos in the temp folder (made on first use, kept between runs);
--demo-fresh a new one, removed when the window closes. Neither reads or changes your own library or settings.
Otherwise the library is DIR, else $DALICA_HOME, else the `data` folder beside the executable when a file named
`portable` is there (portable mode: the library and caches stay next to the program), else the platform data folder
(Linux ~/.local/share/dalica/library, macOS ~/Library/Application Support/Dalica/Library,
Windows %LOCALAPPDATA%\\Dalica\\Library).

--view FILE (or FILE alone, when it does not start with `-`) is how the file manager's Open with hands Dalica an image: it opens the photo viewer on FILE, a borderless
window filling the screen, with the library opened as usual behind it (read-only when another Dalica has it open;
with --headless the viewer is served over the control channel). FILE may be missing or have any name; it is never run.
One Dalica per user: when the user's own Dalica already runs (the default library, without DALICA_HOME or a portable
copy), a later launch hands FILE to it and exits; that Dalica shows it in its viewer, and Esc returns to where it was.

On Linux the window first checks for the libraries it loads (Wayland or X11, xkbcommon, Vulkan); a missing one is
named with the package to install and dalica exits with status 1 (DALICA_SKIP_LIB_CHECK=1 skips the check).

--control [PORT] lets agents and scripts drive the app over 127.0.0.1:PORT (default $DALICA_CONTROL_PORT, else 7990),
with a token from a private token file (docs/control-protocol.md). Control options:
  --control-token-file PATH      the token file (made when missing); default <run folder>/control-<PORT>.token
  --automation-read-root DIR     a folder automation may read from (repeatable)
  --automation-write-root DIR    a folder automation may write new files into (repeatable)
  --automation-allow-external    allow commands with effects outside Dalica
  --automation-allow-people      let people's names and face rectangles reach automation, and its searches match names
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
    /// `--view FILE`: the file the file manager's Open with handed over, for the photo viewer
    /// (`viewing.md` §3, [`viewer`]). Taken verbatim (any bytes, a leading `-`, a missing file) and
    /// never run; only the viewer's worker reads it.
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
            "--automation-allow-people" => {
                flags.allow_people = true;
                needs_control = Some("--automation-allow-people");
                continue;
            }
            "--headless" => {
                headless = true;
                needs_control = needs_control.or(Some("--headless"));
                continue;
            }
            // A file alone is `--view FILE`: what the desktop entry's `Exec=dalica %f` gives (and
            // nothing when it is started from the menu). A file named like an option needs `--view`.
            other if !other.starts_with('-') => {
                if view.is_some() {
                    return Err("--view is given twice".into());
                }
                view = Some(PathBuf::from(raw));
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
    // The user's library, its caches and the log: the platform folders, or beside the executable
    // in portable mode (`paths`). A demo uses only the log folder.
    let dirs = paths::current();
    let cwd = std::env::current_dir().unwrap_or_default();
    let view = options.view.clone().map(|file| viewer::Launch::new(file, &cwd));
    // One Dalica per user (`instance`): a later launch hands its file to the running one and exits,
    // before it binds a port or opens anything.
    let (primary, claimed) = match per_user_library(&options.choice, &dirs) {
        Some(dir) => {
            let files: Vec<PathBuf> = view.iter().map(|v| v.file().to_path_buf()).collect();
            match instance::skip_reason(true, &files) {
                Some(why) => (None, format!("runs on its own: {why}")),
                None => match instance::claim(&dir, &files) {
                    instance::Claim::Forwarded => return ExitCode::SUCCESS,
                    instance::Claim::Primary(p) => (Some(p), "the running Dalica of this user".to_string()),
                    instance::Claim::Standalone(why) => (None, format!("runs on its own: {why}")),
                },
            }
        }
        None => (None, "runs on its own: the settings folder is not the per-user one".to_string()),
    };
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
    log::info!("dalica: {claimed}");
    // A file name can be private: it goes to the log at debug level only.
    if let Some(v) = &view {
        log::debug!("dalica: --view {}: opening the photo viewer", v.file().display());
    }
    let (code, token_file) = match listening {
        Some((listening, flags)) if options.headless => run_headless(listening, &flags, source, demo, view.as_ref(), primary.as_ref()),
        Some((listening, flags)) => {
            let control = window::ControlSetup { listening, control: dalica_ui::control::Control::new(flags.config(demo)) };
            run_window(source, Some(control), view.clone(), primary.as_ref())
        }
        None => run_window(source, None, view.clone(), primary.as_ref()),
    };
    // The app has ended: later launches no longer wait on this one.
    drop(primary);
    if let Some(t) = token_file {
        t.finish();
    }
    if let Some(v) = view {
        v.finish();
    }
    if let Some(dir) = fresh {
        remove_fresh_demo(dir);
    }
    code
}

/// The window, until it closes. Returns the exit code and the token file to remove.
fn run_window(
    source: window::Source,
    control: Option<window::ControlSetup>,
    view: Option<viewer::Launch>,
    primary: Option<&instance::Primary>,
) -> (ExitCode, Option<control::TokenFileGuard>) {
    // The chrome and placement come from the library's settings, read (never changed) before the
    // window and the library open; the session that opens it sets a damaged file aside.
    let settings = dalica_engine::settings::Settings::peek(&source.library_dir());
    let frame = frame::FrameSetup::from_settings(&settings, frame::Os::host());
    // `--view`: the viewer's borderless window filling the screen, transparent when its background
    // is translucent (Options ▸ Photo Viewer).
    let view = view.map(|launch| window::ViewSetup { launch, translucent: settings.viewer().translucent });
    let viewport = match &view {
        Some(v) => viewer::viewport(window::viewport(&frame), v.translucent),
        None => window::viewport(&frame),
    };
    // The GPU backend: DX12 alone on Windows, where Vulkan presentation flickered (`gpu`).
    let options = eframe::NativeOptions { viewport, wgpu_options: gpu::host_options(), ..Default::default() };
    let token_file = Arc::new(std::sync::Mutex::new(None));
    let keep = token_file.clone();
    // Files handed over by later launches (and macOS's open-documents events, whose handlers are
    // registered on this thread before the event loop starts) open in the viewer (`instance`).
    let decorated = frame.chrome != dalica_ui::titlebar::Chrome::Drawn;
    let handoffs = |ctx: &egui::Context| {
        let wake = ctx.clone();
        primary.map(|p| p.connect(Some(Arc::new(move || wake.request_repaint()))))
    };
    #[cfg(target_os = "macos")]
    let apple = instance::apple::AppleEvents::install();
    let ran = eframe::run_native(
        "Dalica",
        options,
        Box::new(move |cc| {
            let receiver = instance::Receiver::new(handoffs(&cc.egui_ctx), decorated);
            #[cfg(target_os = "macos")]
            let receiver = {
                let wake = cc.egui_ctx.clone();
                apple.set_wake(move || wake.request_repaint());
                receiver.with_apple(apple)
            };
            let (w, guard) = window::Window::new(&cc.egui_ctx, source, control, frame, view, Some(receiver));
            if let Ok(mut slot) = keep.lock() {
                *slot = guard;
            }
            // Photos dragged out of the window become the OS's drag of their files (M15.23).
            let drag_out = drag_out::Native::new();
            dalica_ui::drag::out::install(&cc.egui_ctx, drag_out.clone());
            Ok(Box::new(drag_out::Host::new(w, drag_out)))
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

/// `--headless`: opens the library (and the photo viewer with `--view`, its state served over the
/// control channel: `viewer.list`), serves the control channel until `app.quit`, flushes.
fn run_headless(
    listening: control::Listening,
    flags: &control::Flags,
    source: window::Source,
    demo: bool,
    view: Option<&viewer::Launch>,
    primary: Option<&instance::Primary>,
) -> (ExitCode, Option<control::TokenFileGuard>) {
    let session = match window::open(source, view, &|_| {}) {
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
    let receiver = primary.map(|p| instance::Receiver::new(Some(p.connect(None)), false));
    let saved = control::headless::run(&hub, &control, session, receiver);
    (if saved { ExitCode::SUCCESS } else { ExitCode::FAILURE }, Some(token_file))
}

/// The settings folder of a launch on the user's own library, which takes part in the hand-off
/// (`instance`): the platform's per-user library folder, not one named on the command line, by
/// `DALICA_HOME` or by a portable copy (any of which may be shared, or meant as a separate Dalica).
fn per_user_library(choice: &Choice, dirs: &paths::Dirs) -> Option<PathBuf> {
    let home_set = std::env::var_os("DALICA_HOME").is_some_and(|v| !v.is_empty() && std::path::Path::new(&v).is_absolute());
    (*choice == Choice::Default && dirs.mode == paths::Mode::Platform && !home_set).then(|| dirs.library.clone()).flatten()
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
            allow_people: true,
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
            "--automation-allow-people",
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
            &["--automation-allow-people"],
            &["--automation-allow-people", "--demo"],
            &["--control", "--control-token-file"],
            &["--control", "--automation-read-root"],
        ] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
        assert!(parsed(&["--headless"]).unwrap_err().contains("needs --control"));
        assert_eq!(parsed(&["--automation-allow-people"]).unwrap_err(), "--automation-allow-people needs --control");
        let people = parsed(&["--control", "--automation-allow-people"]);
        assert_eq!(
            people,
            Ok(Launch::Open(Options {
                choice: Choice::Default,
                control: Some(control::Flags { allow_people: true, ..flags(None) }),
                headless: false,
                view: None
            }))
        );
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
        // A file alone, as the desktop entry's `Exec=dalica %f` passes it (M15.24).
        assert_eq!(parsed(&["/photos/a b.jpg"]), viewing(Choice::Default, "/photos/a b.jpg"));
        assert_eq!(parsed(&["--library", "/lib", "/x.jpg"]), viewing(Choice::Library(PathBuf::from("/lib")), "/x.jpg"));
        for bad in [&["--view"][..], &["--view", "/a.jpg", "--view", "/b.jpg"], &["/a.jpg", "/b.jpg"], &["--view", "/a.jpg", "/b.jpg"], &["-x.jpg"]] {
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
