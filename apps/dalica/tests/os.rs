//! The app's real OS services (`src/os.rs`, M15.12; `security.md` §5.11): every OS's argument
//! vectors keep a path one argument and never start a shell; the Linux handler lists come from
//! fixture desktop entries and `mimeapps.list` files (hostile ones bounded, no panic) and the
//! Windows ones from `reg query` output; on the OS the test runs on, reveal and open-with start
//! the right program (a stub on `PATH` records its arguments); files and text round-trip through
//! the clipboard where there is a display. No real program is started: the vectors go to a
//! recording runner or to the stubs.

#[allow(dead_code)]
#[path = "../src/os.rs"]
pub mod native;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_engine::os::{Clip, Handler, Os, OsError};
use native::launch::{Argv, Platform, Runner};
use native::{Env, Native, desktop, registry};

/// Keeps every vector it is given; answers `output` from canned text by the vector's second
/// argument (`reg query <key>`), else fails as a missing key does.
#[derive(Default)]
struct Recorder {
    started: Mutex<Vec<Argv>>,
    asked: Mutex<Vec<Argv>>,
    answers: BTreeMap<String, String>,
}

impl Recorder {
    fn answering(answers: &[(&str, &str)]) -> Recorder {
        Recorder { answers: answers.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(), ..Recorder::default() }
    }

    fn take(&self) -> Vec<Argv> {
        std::mem::take(&mut *self.started.lock().unwrap())
    }
}

impl Runner for Recorder {
    fn start(&self, argv: &Argv) -> Result<(), String> {
        self.started.lock().unwrap().push(argv.clone());
        Ok(())
    }

    fn output(&self, argv: &Argv) -> Result<Vec<u8>, String> {
        self.asked.lock().unwrap().push(argv.clone());
        let key = argv.args.get(1).map(|k| k.to_string_lossy().into_owned()).unwrap_or_default();
        self.answers
            .get(&key)
            .map(|t| t.clone().into_bytes())
            .ok_or_else(|| "ERROR: The system was unable to find the specified registry key or value.".into())
    }
}

/// No file manager answers on the bus.
fn no_bus() -> native::ShowItems {
    Box::new(|_| Err("no bus in tests".into()))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A freedesktop tree inside `root`: the user's and a system data folder, the user's config, the
/// MIME database; every XDG variable points inside it, so nothing of this machine's is read.
fn xdg(root: &Path) -> Env {
    Env::from_vars([
        ("HOME", root.join("home").into_os_string()),
        ("XDG_DATA_HOME", root.join("home/data").into_os_string()),
        ("XDG_DATA_DIRS", root.join("sys").into_os_string()),
        ("XDG_CONFIG_HOME", root.join("home/config").into_os_string()),
        ("XDG_CONFIG_DIRS", root.join("etc").into_os_string()),
        ("XDG_CURRENT_DESKTOP", OsString::from("ubuntu:GNOME")),
        ("LANG", OsString::from("fr_FR.UTF-8")),
        ("PATH", root.join("bin").into_os_string()),
        ("ProgramFiles", OsString::from(r"C:\Program Files")),
    ])
}

const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh"];

fn program_name(argv: &Argv) -> String {
    let p = argv.program.to_string_lossy().replace('\\', "/");
    p.rsplit('/').next().unwrap_or_default().to_ascii_lowercase()
}

fn args(argv: &Argv) -> Vec<String> {
    argv.args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// Reveal, open with and the external editor on each OS, with a path with spaces, one starting
/// with `-`, one not in ASCII and one full of shell syntax: no vector starts a shell and each path
/// is exactly one argument (`./-name` for the dash, backslashes on Windows; inside Explorer's
/// quoted `/select,"…"`).
#[test]
fn every_oss_vectors_keep_each_path_one_argument_and_never_start_a_shell() {
    let dir = tempfile::tempdir().unwrap();
    let env = xdg(dir.path());
    write(
        &dir.path().join("home/data/applications/viewer.desktop"),
        "[Desktop Entry]\nType=Application\nName=Viewer\nExec=/opt/viewer/bin/viewer --title \"A \\\"B\\\" C\" %f\nMimeType=image/jpeg;\n",
    );
    let paths: Vec<PathBuf> =
        ["/photos/My Photos/a b.jpg", "-rf.jpg", "/photos/été/日本 写真.jpg", "/p/a;$(rm -rf ~) & b|`c`'d\".jpg"].iter().map(PathBuf::from).collect();
    let reg = [
        (r"HKCR\Pictures.Viewer\shell\open\command", "\r\n    (Default)    REG_EXPAND_SZ    \"%ProgramFiles%\\Viewer\\viewer.exe\" /open \"%1\"\r\n"),
        (r"HKCR\.jpg", "\r\n    (Default)    REG_SZ    Pictures.Viewer\r\n"),
        (r"HKCR\Pictures.Viewer\shell\edit\command", "\r\n    (Default)    REG_SZ    C:\\Paint\\paint.exe %1\r\n"),
    ];
    for platform in [Platform::Linux, Platform::Windows, Platform::MacOs] {
        let runner = Arc::new(Recorder::answering(&reg));
        let os = Native::with(platform, env.clone(), runner.clone(), no_bus());
        for path in &paths {
            os.reveal(path).unwrap();
            let handler = match platform {
                Platform::Linux => "viewer.desktop",
                Platform::Windows => "Pictures.Viewer",
                Platform::MacOs => "/Applications/Preview.app",
            };
            os.open_with(path, handler).unwrap();
            os.open_in_editor(std::slice::from_ref(path), "").unwrap();
            let named = if platform == Platform::Windows { r"C:\Tools\editor.exe" } else { "/opt/editor/editor" };
            os.open_in_editor(std::slice::from_ref(path), named).unwrap();
            let started = runner.take();
            assert_eq!(started.len(), 4, "{platform:?} {path:?}: {started:?}");
            let shown = path.to_string_lossy().into_owned();
            let want = match (platform, shown.starts_with('-')) {
                (Platform::Windows, true) => format!(".\\{shown}"),
                (_, true) => format!("./{shown}"),
                (Platform::Windows, false) => shown.replace('/', "\\"),
                (_, false) => shown.clone(),
            };
            for (i, argv) in started.iter().enumerate() {
                let name = program_name(argv);
                assert!(!SHELLS.contains(&name.as_str()), "{platform:?} started a shell: {}", argv.describe());
                let a = args(argv);
                if argv.verbatim {
                    // Explorer's `/select,"<path>"`: the one raw argument, the path whole inside it.
                    assert_eq!(platform, Platform::Windows);
                    assert_eq!(a, [format!("/select,\"{}\"", want.replace('"', ""))], "{}", argv.describe());
                } else if platform == Platform::Linux && i == 0 {
                    // No file manager answers on the bus: the folder is opened.
                    let folder = Path::new(&shown).parent().map(|f| f.to_string_lossy().into_owned()).filter(|f| !f.is_empty());
                    assert_eq!(a, [folder.unwrap_or_else(|| ".".into())], "{}", argv.describe());
                } else {
                    assert_eq!(a.iter().filter(|x| **x == want).count(), 1, "{platform:?}: {shown} should be one argument of {}", argv.describe());
                }
            }
            let first = &started[0];
            match platform {
                Platform::Linux => assert_eq!(program_name(first), "xdg-open"),
                Platform::Windows => assert_eq!(program_name(first), "explorer.exe"),
                Platform::MacOs => assert_eq!((program_name(first), args(first).first().cloned()), ("open".into(), Some("-R".into()))),
            }
            match platform {
                Platform::Linux => {
                    assert_eq!(started[1].program, OsString::from("/opt/viewer/bin/viewer"));
                    assert_eq!(args(&started[1])[..2], ["--title".to_string(), "A \"B\" C".to_string()]);
                }
                Platform::Windows => {
                    assert_eq!(started[1].program, OsString::from(r"C:\Program Files\Viewer\viewer.exe"));
                    assert_eq!(args(&started[1]), ["/open".to_string(), want.clone()]);
                    assert_eq!(started[2].program, OsString::from(r"C:\Paint\paint.exe"), "the edit verb");
                }
                Platform::MacOs => {
                    assert_eq!(args(&started[1]), ["-a".to_string(), "/Applications/Preview.app".into(), want.clone()]);
                    assert_eq!(args(&started[3])[..2], ["-a".to_string(), "/opt/editor/editor".into()]);
                }
            }
        }
    }
    // Explorer's switch, the verbatim prefix dropped and a quote never ending it early (LightCraft's cases).
    let sel = |p: &str| native::launch::explorer_select_arg(p.as_ref()).to_string_lossy().into_owned();
    assert_eq!(sel(r"C:\Users\Me\My Photos\IMG 1.jpg"), r#"/select,"C:\Users\Me\My Photos\IMG 1.jpg""#);
    assert_eq!(sel("D:/Example/Photos/IMG_0001.CR3"), r#"/select,"D:\Example\Photos\IMG_0001.CR3""#);
    assert_eq!(sel(r"\\?\D:\My Photos\a.jpg"), r#"/select,"D:\My Photos\a.jpg""#);
    assert_eq!(sel(r"\\?\UNC\server\share\a.jpg"), r#"/select,"\\server\share\a.jpg""#);
    assert_eq!(sel(r#"C:\a"b.jpg"#), r#"/select,"C:\ab.jpg""#);
    assert_eq!(native::launch::file_uri(Path::new("/home/me/My Photos/été #1.jpg")), "file:///home/me/My%20Photos/%C3%A9t%C3%A9%20%231.jpg");
}

/// A named editor gets every file in one start; Flatpak's forwarding, a desktop entry by id and
/// an editor that is nowhere are told apart (LightCraft's launcher, Linux's rows added).
#[test]
fn the_external_editor_takes_every_file_in_one_start() {
    let dir = tempfile::tempdir().unwrap();
    let env = xdg(dir.path());
    let paths = [PathBuf::from("/p/a.dng"), PathBuf::from("/p/b c.dng")];
    write(
        &dir.path().join("home/data/applications/org.gimp.GIMP.desktop"),
        "[Desktop Entry]\nType=Application\nName=GIMP\nExec=/usr/bin/flatpak run --file-forwarding org.gimp.GIMP @@u %U @@\n",
    );
    let runner = Arc::new(Recorder::answering(&[("org.example.Editor", "")]));
    let os = Native::with(Platform::Linux, env, runner.clone(), no_bus());
    os.open_in_editor(&paths, "/opt/dxo/photolab").unwrap();
    os.open_in_editor(&paths, "org.gimp.GIMP.desktop").unwrap();
    os.open_in_editor(&paths, "org.example.Editor").unwrap();
    let started = runner.take();
    assert_eq!(args(&started[0]), ["/p/a.dng", "/p/b c.dng"]);
    assert_eq!(args(&started[1]), ["run", "--file-forwarding", "org.gimp.GIMP", "@@u", "/p/a.dng", "/p/b c.dng", "@@"]);
    assert_eq!(program_name(&started[2]), "flatpak");
    assert_eq!(args(&started[2]), ["run", "--file-forwarding", "org.example.Editor", "@@", "/p/a.dng", "/p/b c.dng", "@@"]);
    assert!(matches!(os.open_in_editor(&paths, "no-such-editor"), Err(OsError::Failed(why)) if why.contains("no-such-editor")));
    assert!(os.open_in_editor(&[], "").is_err(), "no files");
    assert!(runner.take().is_empty());
}

/// The handlers of a JPEG from fixture entries: the `mimeapps.list` default first (the
/// desktop's own file before the generic one), then the added association, then the entries
/// listing the type by folder and id; removed, hidden and no-display ones left out; a
/// subfolder's entry named `kde-…`; names in the user's language; a RAW type reaches the
/// handlers of its parent type.
#[test]
fn handler_lists_come_from_desktop_entries_and_mimeapps_lists() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = xdg(root);
    let app =
        |name: &str, mime: &str, extra: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec={name} %U\nMimeType={mime}\n{extra}");
    write(
        &root.join("sys/mime/globs2"),
        "# comment\n50:image/jpeg:*.jpg\n50:image/jpeg:*.jpeg\n80:image/x-canon-cr2:*.cr2\nbroken line\n50:image/png:*.png\n",
    );
    write(&root.join("sys/mime/subclasses"), "image/x-canon-cr2 image/x-dcraw\n");
    write(&root.join("sys/applications/aaa.desktop"), &app("Aaa", "image/jpeg;", ""));
    write(&root.join("sys/applications/viewer.desktop"), &app("Viewer", "image/jpeg;image/png;", "Name[fr]=Visionneuse\n"));
    write(&root.join("sys/applications/editor.desktop"), &app("Editor", "image/png;", ""));
    write(&root.join("sys/applications/gone.desktop"), &app("Gone", "image/jpeg;", "Hidden=true\n"));
    write(&root.join("sys/applications/quiet.desktop"), &app("Quiet", "image/jpeg;", "NoDisplay=true\n"));
    write(&root.join("sys/applications/removed.desktop"), &app("Removed", "image/jpeg;", ""));
    write(&root.join("sys/applications/kde/raw.desktop"), &app("Raw", "image/x-dcraw;", ""));
    write(&root.join("sys/applications/link.desktop"), "[Desktop Entry]\nType=Link\nName=Link\nURL=https://example.org\nMimeType=image/jpeg;\n");
    // The user's own copy shadows the system's.
    write(&root.join("home/data/applications/aaa.desktop"), &app("Aaa (mine)", "image/jpeg;", ""));
    write(&root.join("home/config/gnome-mimeapps.list"), "[Default Applications]\nimage/jpeg=editor.desktop;\n");
    write(
        &root.join("home/config/mimeapps.list"),
        "[Default Applications]\nimage/jpeg=viewer.desktop\n[Added Associations]\nimage/jpeg=quiet.desktop;missing.desktop;\n[Removed Associations]\nimage/jpeg=removed.desktop;\n",
    );
    let os = Native::with(Platform::Linux, env, Arc::new(Recorder::default()), no_bus());
    let ids = |p: &str| os.handlers(Path::new(p)).unwrap().into_iter().map(|h| h.id).collect::<Vec<_>>();
    assert_eq!(ids("/x/IMG_1.JPG"), ["editor.desktop", "viewer.desktop", "quiet.desktop", "aaa.desktop"]);
    let all = os.handlers(Path::new("/x/a.jpg")).unwrap();
    assert_eq!(all[1], Handler::new("viewer.desktop", "Visionneuse"), "the user's language");
    assert_eq!(all[3].name, "Aaa (mine)", "the user's entry shadows the system's");
    assert_eq!(ids("/x/a.png"), ["editor.desktop", "viewer.desktop"]);
    assert_eq!(ids("/x/a.cr2"), ["kde-raw.desktop"], "through the parent type");
    assert_eq!(ids("/x/a.unknown"), Vec::<String>::new());
    // Without a MIME database the built-in table still knows Dalica's types.
    std::fs::remove_file(root.join("sys/mime/globs2")).unwrap();
    assert_eq!(ids("/x/b.jpeg")[0], "editor.desktop");
    // An Exec line's quoting and codes (Desktop Entry Specification).
    assert_eq!(desktop::split_exec(r#"prog "a b" c\ d "e\"f" "" g"#).unwrap(), ["prog", "a b", r"c\", "d", "e\"f", "", "g"]);
    let entry = desktop::parse_entry(
        "v.desktop",
        Path::new("/e/v.desktop"),
        "[Desktop Entry]\nType=Application\nName=V\nIcon=v\nExec=v %i %c %k 100%% --x=%f %d %F\n",
        &[],
    )
    .unwrap();
    let argv = desktop::exec_argv(&entry, &[PathBuf::from("/a.jpg"), PathBuf::from("/b.jpg")], &Env::from_vars([("PATH", "/nowhere")]));
    assert!(argv.is_err(), "a bare program not on PATH is not started");
    let entry = desktop::Entry { exec: entry.exec.replacen("v ", "/bin/v ", 1), ..entry };
    let argv = desktop::exec_argv(&entry, &[PathBuf::from("/a.jpg"), PathBuf::from("/b.jpg")], &Env::default()).unwrap();
    assert_eq!(
        args(&argv),
        ["--icon", "v", "V", "/e/v.desktop", "100%", "--x=", "/a.jpg", "/b.jpg"],
        "a code inside a word is dropped, never filled"
    );
}

/// Hostile files: an entry larger than the cap, unclosed quotes, binary bytes, a walk deeper than
/// the cap, thousands of ids on one line and random bytes through every parser: bounded lists,
/// errors, no panic.
#[test]
fn hostile_entries_and_lists_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let env = xdg(root);
    let apps = root.join("sys/applications");
    write(
        &apps.join("big.desktop"),
        &format!("[Desktop Entry]\nType=Application\nName=Big\nExec=/bin/big\nMimeType=image/jpeg;\n{}", "#".repeat(desktop::MAX_FILE as usize)),
    );
    write(&apps.join("quote.desktop"), "[Desktop Entry]\nType=Application\nName=Quote\nExec=/bin/q \"unclosed %f\nMimeType=image/jpeg;\n");
    std::fs::write(apps.join("binary.desktop"), [0xff, 0xfe, 0x00, 0x5b, 0x0a]).unwrap();
    let deep: PathBuf = (0..desktop::MAX_DEPTH + 2).fold(apps.clone(), |p, i| p.join(format!("d{i}")));
    write(&deep.join("deep.desktop"), "[Desktop Entry]\nType=Application\nName=Deep\nExec=/bin/deep\nMimeType=image/jpeg;\n");
    let many: String = (0..5000).map(|i| format!("a{i}.desktop;")).collect();
    write(&root.join("home/config/mimeapps.list"), &format!("[Default Applications]\nimage/jpeg={many}\n[Added Associations]\nimage/jpeg={many}\n"));
    for i in 0..(desktop::MAX_HANDLERS + 10) {
        write(&apps.join(format!("a{i}.desktop")), &format!("[Desktop Entry]\nType=Application\nName=A{i}\nExec=/bin/a\nMimeType=image/jpeg;\n"));
    }
    let os = Native::with(Platform::Linux, env, Arc::new(Recorder::default()), no_bus());
    let found = os.handlers(Path::new("/x/a.jpg")).unwrap();
    assert_eq!(found.len(), desktop::MAX_HANDLERS);
    assert!(found.iter().all(|h| h.id != "big.desktop" && h.id != "binary.desktop" && !h.id.contains("deep")), "{found:?}");
    assert!(matches!(os.open_with(Path::new("/x/a.jpg"), "quote.desktop"), Err(OsError::Failed(_))), "an unclosed quote is refused");
    assert!(matches!(os.open_with(Path::new("/x/a.jpg"), "big.desktop"), Err(OsError::Failed(_))), "a skipped entry is not installed");
    assert!(os.open_with(Path::new("/x/a.jpg"), "").is_err() && os.reveal(Path::new("")).is_err() && os.reveal(Path::new("/a\0b")).is_err());
    let bad = Clip::Picture { width: u32::MAX, height: u32::MAX, rgba: vec![0; 16] };
    assert!(matches!(os.put_on_clipboard(&bad), Err(OsError::Failed(_))), "a picture of the wrong size is refused before the clipboard");
    assert!(matches!(os.put_on_clipboard(&Clip::Files(vec![PathBuf::from("relative.jpg")])), Err(OsError::Failed(_))));
    // Random bytes through each parser (a fixed seed: the same cases every run).
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let alphabet = b"[]=;%\"\\ \n\r\tabcHKEY_REG_SZ    %1:*.~\x00\xff";
    for _ in 0..if std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1") { 20_000 } else { 2_000 } {
        let len = (seed % 200) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                alphabet[(seed % alphabet.len() as u64) as usize]
            })
            .collect();
        let text = String::from_utf8_lossy(&bytes);
        let _ = desktop::parse_entry("x.desktop", Path::new("/x"), &text, &["fr".into()]);
        let _ = desktop::parse_mimeapps(&text, "image/jpeg");
        let _ = desktop::mime_from_globs2(&text, "a.jpg");
        let _ = desktop::split_exec(&text);
        let _ = registry::Key::read(&text);
        let _ = registry::split_command_line(&text);
        let _ = registry::command_argv(&text, Path::new(r"C:\a.jpg"), &Env::default());
        let _ = registry::expand_env(&text, &Env::from_vars([("A", "b")]));
    }
}

/// Windows' handler list from `reg query` output: the user's choice first, then the type's
/// ProgID, the `OpenWithProgids` and `OpenWithList` entries (by `MRUList`), each with an `open`
/// command and its name; a `DelegateExecute`-only ProgID (no command) is left out.
#[test]
fn windows_handlers_come_from_reg_query_output() {
    let fe = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jpg";
    let reg = [
        (format!(r"{fe}\UserChoice"), "\r\nHKEY_CURRENT_USER\\...\\UserChoice\r\n    Hash    REG_SZ    abc=\r\n    ProgId    REG_SZ    Viewer.Jpeg\r\n".to_string()),
        (r"HKCR\.jpg".into(), "\r\nHKEY_CLASSES_ROOT\\.jpg\r\n    (Default)    REG_SZ    jpegfile\r\n    Content Type    REG_SZ    image/jpeg\r\n\r\nHKEY_CLASSES_ROOT\\.jpg\\OpenWithProgids\r\n".into()),
        (r"HKCR\.jpg\OpenWithProgids".into(), "\r\nHKEY_CLASSES_ROOT\\.jpg\\OpenWithProgids\r\n    AppX43hnxtbyyps62jhe9sqpdzxn1790zetc    REG_SZ    \r\n    jpegfile    REG_NONE    \r\n".into()),
        (format!(r"{fe}\OpenWithList"), "\r\nHKEY_CURRENT_USER\\...\\OpenWithList\r\n    a    REG_SZ    mspaint.exe\r\n    b    REG_SZ    Paint Shop.exe\r\n    MRUList    REG_SZ    ba\r\n".into()),
        (r"HKCR\.jpg\OpenWithList".into(), "\r\nHKEY_CLASSES_ROOT\\.jpg\\OpenWithList\r\n\r\nHKEY_CLASSES_ROOT\\.jpg\\OpenWithList\\gimp.exe\r\n".into()),
        (r"HKCR\Viewer.Jpeg\shell\open\command".into(), "\r\n    (Default)    REG_SZ    \"C:\\Viewer\\viewer.exe\" \"%1\"\r\n".into()),
        (r"HKCR\Viewer.Jpeg\Application".into(), "\r\n    ApplicationName    REG_SZ    Example Viewer\r\n".into()),
        (r"HKCR\jpegfile\shell\open\command".into(), "\r\n    (Default)    REG_EXPAND_SZ    %SystemRoot%\\System32\\rundll32.exe \"%ProgramFiles%\\Windows Photo Viewer\\PhotoViewer.dll\", ImageView_Fullscreen %1\r\n".into()),
        (r"HKCR\Applications\Paint Shop.exe\shell\open\command".into(), "\r\n    (Default)    REG_SZ    \"D:\\PSP\\Paint Shop.exe\" \"%1\"\r\n".into()),
        (r"HKCR\Applications\Paint Shop.exe".into(), "\r\n    FriendlyAppName    REG_SZ    @%ProgramFiles%\\x.dll,-1\r\n".into()),
        (r"HKCR\Applications\mspaint.exe\shell\open\command".into(), "\r\n    (Default)    REG_EXPAND_SZ    %SystemRoot%\\system32\\mspaint.exe \"%1\"\r\n".into()),
        (r"HKCR\Applications\gimp.exe\shell\open\command".into(), "\r\n    (Default)    REG_SZ    \"C:\\GIMP\\gimp.exe\" %*\r\n".into()),
    ];
    let reg: Vec<(&str, &str)> = reg.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let runner = Arc::new(Recorder::answering(&reg));
    let env = Env::from_vars([("SystemRoot", r"C:\Windows"), ("ProgramFiles", r"C:\Program Files")]);
    let os = Native::with(Platform::Windows, env, runner.clone(), no_bus());
    let found = os.handlers(Path::new(r"C:\Pics\IMG 1.JPG")).unwrap();
    let pairs: Vec<(&str, &str)> = found.iter().map(|h| (h.id.as_str(), h.name.as_str())).collect();
    assert_eq!(
        pairs,
        [
            ("Viewer.Jpeg", "Example Viewer"),
            ("jpegfile", "jpegfile"),
            (r"Applications\Paint Shop.exe", "Paint Shop"),
            (r"Applications\mspaint.exe", "mspaint"),
            (r"Applications\gimp.exe", "gimp"),
        ]
    );
    os.open_with(Path::new(r"C:\Pics\IMG 1.JPG"), "jpegfile").unwrap();
    os.open_with(Path::new(r"C:\Pics\IMG 1.JPG"), r"Applications\gimp.exe").unwrap();
    let started = runner.take();
    assert_eq!(started[0].program, OsString::from(r"C:\Windows\System32\rundll32.exe"));
    assert_eq!(args(&started[0]), [r"C:\Program Files\Windows Photo Viewer\PhotoViewer.dll,", "ImageView_Fullscreen", r"C:\Pics\IMG 1.JPG"]);
    assert_eq!(args(&started[1]), [r"C:\Pics\IMG 1.JPG"], "%* dropped, the file added");
    assert!(runner.asked.lock().unwrap().iter().all(|a| program_name(a) == "reg" && args(a)[0] == "query"));
    assert!(matches!(os.open_with(Path::new(r"C:\a.jpg"), "AppX43hnxtbyyps62jhe9sqpdzxn1790zetc"), Err(OsError::Failed(_))));
}

/// Waits for a stub's record of its arguments (it runs on its own).
#[cfg(unix)]
fn recorded(file: &Path) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(file)
            && text.ends_with("<end>\n")
        {
            return text.lines().filter(|l| *l != "<end>").map(str::to_string).collect();
        }
        assert!(Instant::now() < until, "{} was never written", file.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A stub program that writes each argument on a line of `record`.
#[cfg(unix)]
fn stub(path: &Path, record: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{0}.part'; done\nprintf '<end>\\n' >> '{0}.part'\nmv '{0}.part' '{0}'\n",
        record.display()
    );
    write(path, &script);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// On this OS, through the real runner: reveal starts the file manager's program (Linux:
/// `xdg-open` on the folder when no file manager answers on the bus, the `FileManager1` call with
/// the file's URI when one does; macOS: `open -R`), and open-with starts the chosen application
/// with the file, each a stub on `PATH` that records its arguments.
#[test]
fn reveal_and_open_with_start_the_right_program() {
    if cfg!(windows) {
        eprintln!("skipped on Windows: a stub there would be a .cmd, which only cmd.exe starts (the vectors are checked above)");
        return;
    }
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let env = xdg(root);
        let photo = root.join("My Photos/-été 1.jpg");
        write(&photo, "");
        let record = root.join("record");
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        stub(&root.join("bin").join(opener), &record);
        stub(&root.join("bin/viewer"), &root.join("viewer-record"));
        write(
            &root.join("home/data/applications/viewer.desktop"),
            "[Desktop Entry]\nType=Application\nName=Viewer\nExec=viewer --one %f\nMimeType=image/jpeg;\n",
        );
        let host = Platform::host();
        let os = Native::with(host, env.clone(), Arc::new(native::launch::Spawn), no_bus());
        os.reveal(&photo).unwrap();
        let got = recorded(&record);
        if host == Platform::MacOs {
            assert_eq!(got, ["-R".to_string(), photo.to_string_lossy().into_owned()]);
        } else {
            assert_eq!(got, [photo.parent().unwrap().to_string_lossy().into_owned()]);
            let asked = Arc::new(Mutex::new(Vec::new()));
            let keep = asked.clone();
            let os = Native::with(
                host,
                env,
                Arc::new(native::launch::Spawn),
                Box::new(move |uris: &[String]| {
                    keep.lock().unwrap().extend_from_slice(uris);
                    Ok(())
                }),
            );
            os.reveal(&photo).unwrap();
            assert_eq!(*asked.lock().unwrap(), [native::launch::file_uri(&photo)]);
            os.open_with(&photo, "viewer.desktop").unwrap();
            assert_eq!(recorded(&root.join("viewer-record")), ["--one".to_string(), photo.to_string_lossy().into_owned()]);
        }
    }
}

/// Files and text put on the clipboard read back the same, where there is a display (skipped,
/// and said so, otherwise). The text that was there before is put back.
#[test]
fn files_and_text_round_trip_through_the_clipboard() {
    let display = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if cfg!(all(unix, not(target_os = "macos"))) && !display("DISPLAY") && !display("WAYLAND_DISPLAY") {
        eprintln!("skipped: no display (neither DISPLAY nor WAYLAND_DISPLAY), so no clipboard");
        return;
    }
    let Ok(mut reader) = arboard::Clipboard::new() else {
        eprintln!("skipped: the display has no clipboard");
        return;
    };
    let before = reader.get_text().ok();
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = ["a b.jpg", "été.png"].iter().map(|n| dir.path().join(n)).collect();
    for f in &files {
        std::fs::write(f, b"x").unwrap();
    }
    let files: Vec<PathBuf> = files.iter().map(|f| f.canonicalize().unwrap()).collect();
    let os = Native::new();
    os.put_on_clipboard(&Clip::Files(files.clone())).unwrap();
    // A paste may take a moment to see the new owner.
    let until = Instant::now() + Duration::from_secs(5);
    let back = loop {
        let got: Vec<PathBuf> =
            reader.get().file_list().unwrap_or_default().into_iter().map(|p| PathBuf::from(p.to_string_lossy().trim_end_matches('\r'))).collect();
        if got == files || Instant::now() > until {
            break got;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(back, files);
    let text = "/p/a b.jpg\n/p/été.png";
    os.put_on_clipboard(&Clip::Text(text.into())).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let back = loop {
        let got = reader.get_text().unwrap_or_default();
        if got == text || Instant::now() > until {
            break got;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(back, text);
    if let Some(before) = before {
        let _ = os.put_on_clipboard(&Clip::Text(before));
    }
}
