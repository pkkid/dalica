//! Dalica as the default viewer on each OS (`src/os/default_viewer.rs`, M15.24, `viewing.md`
//! §3.3), never touching this machine's settings (every XDG folder is a temp one, Windows' `reg`
//! goes to a recording runner):
//!
//! - Linux: a `mimeapps.list` with other groups, comments, an unknown key and CRLF endings keeps
//!   all of them byte for byte when Dalica is chosen for JPEG and PNG, the replaced handler is
//!   remembered, and "Don't use Dalica as a viewer" gives the file back exactly as it was; a key
//!   the user changed since is left alone; a damaged list is kept aside, a too large one or a link
//!   refused, hostile lists never panic; without Dalica's desktop entry it says why.
//! - Windows: the user's registration by `reg add` vectors (never a shell), Default apps opened.
//! - macOS: nothing set, the answer says how to choose in Finder.
//! - The packages declare exactly the types Dalica offers (the desktop entry's `MimeType`, the MSI's
//!   extensions, `Info.plist`'s content types), HEIC and AVIF left out.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::os::native;
use dalica_engine::os::{DefaultSet, Os, OsError, extensions, mime_types};
use dalica_engine::settings::VIEWER_TYPES;
use dalica_scan::FileType;
use native::default_viewer::{self, MEMORY, default_of, desktop_id, set_default};
use native::launch::{Argv, Platform, Runner};
use native::{Env, Native, desktop};

/// Keeps every vector; `reg add`/`reg delete` succeed, `reg query <key>` answers from `answers`.
#[derive(Default)]
struct Reg {
    ran: Mutex<Vec<Argv>>,
    started: Mutex<Vec<Argv>>,
    answers: Vec<(String, String)>,
}

impl Runner for Reg {
    fn start(&self, argv: &Argv) -> Result<(), String> {
        self.started.lock().unwrap().push(argv.clone());
        Ok(())
    }

    fn output(&self, argv: &Argv) -> Result<Vec<u8>, String> {
        self.ran.lock().unwrap().push(argv.clone());
        let a: Vec<String> = argv.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        match a.first().map(String::as_str) {
            Some("add" | "delete") => Ok(Vec::new()),
            _ => a
                .get(1)
                .and_then(|k| self.answers.iter().find(|(key, _)| key == k))
                .map(|(_, v)| v.clone().into_bytes())
                .ok_or_else(|| "ERROR: The system was unable to find the specified registry key or value.".into()),
        }
    }
}

fn no_bus() -> native::ShowItems {
    Box::new(|_| Err("no bus in tests".into()))
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Every XDG folder inside `root`.
fn xdg(root: &Path) -> Env {
    Env::from_vars([
        ("HOME", root.join("home").into_os_string()),
        ("XDG_DATA_HOME", root.join("home/data").into_os_string()),
        ("XDG_DATA_DIRS", root.join("sys").into_os_string()),
        ("XDG_CONFIG_HOME", root.join("home/config").into_os_string()),
        ("XDG_CONFIG_DIRS", root.join("etc").into_os_string()),
        ("PATH", root.join("bin").into_os_string()),
    ])
}

fn packaging(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging").join(rel)).unwrap()
}

/// A Linux tree with Dalica's real desktop entry and an image viewer installed.
fn linux(root: &Path) -> Native {
    write(&root.join("sys/applications").join(desktop_id()), packaging("linux/org.dalica.Dalica.desktop").as_bytes());
    write(
        &root.join("sys/applications/viewer.desktop"),
        b"[Desktop Entry]\nType=Application\nName=Image Viewer\nIcon=viewer\nExec=viewer %U\nMimeType=image/jpeg;image/png;\n",
    );
    write(&root.join("sys/applications/gimp.desktop"), b"[Desktop Entry]\nType=Application\nName=GIMP\nExec=gimp %U\nMimeType=image/jpeg;\n");
    Native::with(Platform::Linux, xdg(root), Arc::new(Reg::default()), no_bus())
}

fn list_path(root: &Path) -> PathBuf {
    root.join("home/config/mimeapps.list")
}

fn read(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

/// The user's list: comments, blank lines, other groups, an unknown key, CRLF line endings.
const USERS: &str = "# my associations\r\n[Added Associations]\r\nimage/png=gimp.desktop;\r\n\r\n[Default Applications]\r\n# viewers\r\nimage/jpeg=viewer.desktop;\r\ntext/plain=org.gnome.TextEditor.desktop;\r\nX-Unknown-Key = kept as it is \r\n\r\n[X-Other Group]\r\nimage/jpeg=not-a-default.desktop\r\n";

#[test]
fn mimeapps_list_keeps_every_other_entry_and_dont_use_gives_it_back_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let os = linux(root);
    write(&list_path(root), USERS.as_bytes());
    let before = os.default_handler(FileType::Jpeg).unwrap().unwrap();
    assert_eq!((before.id.as_str(), before.name.as_str(), before.icon.as_deref()), ("viewer.desktop", "Image Viewer", Some("viewer")));

    assert_eq!(os.set_default_viewer(&[FileType::Jpeg, FileType::Png]).unwrap(), DefaultSet::Set);
    let me = desktop_id();
    let expected = USERS
        .replace("image/jpeg=viewer.desktop;\r\n", &format!("image/jpeg={me};\r\n"))
        .replace("X-Unknown-Key = kept as it is \r\n", &format!("X-Unknown-Key = kept as it is \r\nimage/png={me};\r\n"));
    assert_eq!(read(&list_path(root)), expected, "only Dalica's two keys changed");
    let now = os.default_handler(FileType::Jpeg).unwrap().unwrap();
    assert_eq!((now.id.as_str(), now.name.as_str(), now.icon.as_deref()), (me.as_str(), "Dalica", Some("org.dalica.Dalica")));
    let memory: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("home/config").join(MEMORY)).unwrap()).unwrap();
    assert_eq!(memory["replaced"], serde_json::json!({ "image/jpeg": "viewer.desktop;", "image/png": null }));

    // Choosing again keeps what was replaced first; "Don't use" puts the file back as it was.
    assert_eq!(os.set_default_viewer(&[FileType::Jpeg, FileType::Png]).unwrap(), DefaultSet::Set);
    assert_eq!(os.set_default_viewer(&[]).unwrap(), DefaultSet::Set);
    assert_eq!(read(&list_path(root)), USERS, "byte for byte");
    let memory: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("home/config").join(MEMORY)).unwrap()).unwrap();
    assert_eq!(memory["replaced"], serde_json::json!({}));

    // A key the user changed since is theirs: "Don't use" leaves it.
    os.set_default_viewer(&[FileType::Jpeg]).unwrap();
    let changed = read(&list_path(root)).replace(&format!("image/jpeg={me};"), "image/jpeg=gimp.desktop;");
    write(&list_path(root), changed.as_bytes());
    os.set_default_viewer(&[]).unwrap();
    assert_eq!(read(&list_path(root)), changed);
}

#[test]
fn a_new_list_gets_a_group_and_a_missing_desktop_entry_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let os = linux(root);
    os.set_default_viewer(&[FileType::Gif]).unwrap();
    assert_eq!(read(&list_path(root)), format!("[Default Applications]\nimage/gif={};\n", desktop_id()));
    os.set_default_viewer(&[]).unwrap();
    assert_eq!(read(&list_path(root)), "[Default Applications]\n");

    std::fs::remove_file(root.join("sys/applications").join(desktop_id())).unwrap();
    let r = os.set_default_viewer(&[FileType::Jpeg]);
    assert!(matches!(&r, Err(OsError::Unavailable(why)) if why.contains("not installed")), "{r:?}");
    assert_eq!(read(&list_path(root)), "[Default Applications]\n", "untouched");
}

#[test]
fn a_damaged_list_is_kept_aside_and_hostile_ones_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let os = linux(root);
    let config = root.join("home/config");
    // Not UTF-8: kept aside as it was, a new list holds Dalica's line.
    let damaged = b"[Default Applications]\nimage/jpeg=\xff\xfe.desktop\n".to_vec();
    write(&list_path(root), &damaged);
    os.set_default_viewer(&[FileType::Jpeg]).unwrap();
    let aside: Vec<PathBuf> = std::fs::read_dir(&config)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("mimeapps.list.corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(std::fs::read(&aside[0]).unwrap(), damaged);
    assert_eq!(read(&list_path(root)), format!("[Default Applications]\nimage/jpeg={};\n", desktop_id()));

    // Larger than the bound: refused, never cut.
    let big = format!("[Default Applications]\n{}", "x/y=z.desktop;\n".repeat(desktop::MAX_FILE as usize / 14 + 1));
    write(&list_path(root), big.as_bytes());
    assert!(matches!(os.set_default_viewer(&[FileType::Png]), Err(OsError::Failed(_))));
    assert_eq!(read(&list_path(root)), big);

    // A damaged memory file is kept aside and read as empty.
    std::fs::remove_file(list_path(root)).unwrap();
    write(&config.join(MEMORY), b"{not json");
    os.set_default_viewer(&[FileType::Png]).unwrap();
    assert!(std::fs::read_dir(config.join("dalica")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains(".corrupt-")));

    // A link is never written through.
    #[cfg(unix)]
    {
        let target = root.join("elsewhere.list");
        write(&target, b"[Default Applications]\n");
        std::fs::remove_file(list_path(root)).unwrap();
        std::os::unix::fs::symlink(&target, list_path(root)).unwrap();
        assert!(matches!(os.set_default_viewer(&[FileType::Bmp]), Err(OsError::Failed(_))));
        assert_eq!(read(&target), "[Default Applications]\n");
    }

    // Hostile texts: the merge never panics, sets the key, and keeps every other line in order.
    let parts = [
        "[Default Applications]\n",
        "[Default Applications",
        "[X]\n",
        "image/jpeg=a.desktop;\n",
        "image/jpeg\n",
        "=\n",
        "==;;==\n",
        "# c\n",
        "\n",
        "\r\n",
        " image/jpeg = b;\n",
        "é😀=ü\n",
        "image/jpeg",
        "[",
    ];
    let mut seed: u64 = 0x5eed;
    for _ in 0..300 {
        let mut text = String::new();
        for _ in 0..(seed % 9) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            text.push_str(parts[(seed >> 33) as usize % parts.len()]);
        }
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let set = set_default(&text, "image/jpeg", Some("dalica.desktop;"));
        assert_eq!(default_of(&set, "image/jpeg").as_deref(), Some("dalica.desktop;"), "{text:?} → {set:?}");
        let gone = set_default(&set, "image/jpeg", None);
        assert_eq!(default_of(&gone, "image/jpeg"), None, "{set:?}");
        let kept = |t: &str| {
            t.lines().filter(|l| l.trim_start().split('=').next().map(str::trim) != Some("image/jpeg")).map(str::to_string).collect::<Vec<_>>()
        };
        let (a, b) = (kept(&text), kept(&set));
        assert!(a.iter().all(|l| b.contains(l)), "{text:?} → {set:?}");
    }
    let huge = "x".repeat(100_000);
    assert_eq!(default_of(&set_default(&huge, "image/png", Some("d;")), "image/png").as_deref(), Some("d;"));
    assert!(default_viewer::parse_memory(b"[1,2]").is_none());
    let flood: String = format!("{{\"replaced\":{{{}}}}}", (0..1000).map(|i| format!("\"image/x{i}\":\"v\"")).collect::<Vec<_>>().join(","));
    assert_eq!(default_viewer::parse_memory(flood.as_bytes()).unwrap().len(), default_viewer::MAX_REMEMBERED);
}

#[test]
fn windows_registers_dalica_for_the_user_and_opens_default_apps() {
    let reg = Arc::new(Reg {
        answers: vec![
            (
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jpg\UserChoice".into(),
                "\r\n    ProgId    REG_SZ    Viewer.Jpeg\r\n".into(),
            ),
            (r"HKCR\Viewer.Jpeg\shell\open\command".into(), "\r\n    (Default)    REG_SZ    \"C:\\Viewer\\viewer.exe\" \"%1\"\r\n".into()),
            (r"HKCR\Viewer.Jpeg\DefaultIcon".into(), "\r\n    (Default)    REG_EXPAND_SZ    %SystemRoot%\\viewer.dll,-3\r\n".into()),
        ],
        ..Reg::default()
    });
    let env = Env::from_vars([("SystemRoot", OsString::from(r"C:\Windows"))]);
    let os = Native::with(Platform::Windows, env, reg.clone(), no_bus());
    let jpeg = os.default_handler(FileType::Jpeg).unwrap().unwrap();
    assert_eq!((jpeg.id.as_str(), jpeg.name.as_str(), jpeg.icon.as_deref()), ("Viewer.Jpeg", "viewer", Some(r"C:\Windows\viewer.dll,-3")));
    assert_eq!(os.default_handler(FileType::Png).unwrap(), None);

    reg.ran.lock().unwrap().clear();
    let r = os.set_default_viewer(&[FileType::Jpeg, FileType::Raw]).unwrap();
    assert!(matches!(&r, DefaultSet::SettingsOpened(m) if m.contains("JPEG and RAW")), "{r:?}");
    let ran: Vec<Vec<String>> = reg.ran.lock().unwrap().iter().map(|a| a.args.iter().map(|s| s.to_string_lossy().into_owned()).collect()).collect();
    assert!(reg.ran.lock().unwrap().iter().all(|a| a.program == "reg"), "only reg.exe, never a shell");
    let has = |want: &[&str]| ran.iter().any(|a| a.iter().map(String::as_str).collect::<Vec<_>>() == want);
    assert!(has(&["add", r"HKCU\Software\Dalica\Capabilities\FileAssociations", "/v", ".jpg", "/t", "REG_SZ", "/d", "Dalica.Image", "/f"]));
    assert!(has(&["add", r"HKCU\Software\Dalica\Capabilities\FileAssociations", "/v", ".cr3", "/t", "REG_SZ", "/d", "Dalica.Image", "/f"]));
    assert!(has(&["add", r"HKCU\Software\Classes\.jpeg\OpenWithProgids", "/v", "Dalica.Image", "/t", "REG_SZ", "/d", "", "/f"]));
    assert!(has(&["add", r"HKCU\Software\RegisteredApplications", "/v", "Dalica", "/t", "REG_SZ", "/d", r"Software\Dalica\Capabilities", "/f"]));
    assert!(has(&["delete", r"HKCU\Software\Dalica\Capabilities\FileAssociations", "/v", ".png", "/f"]));
    let open = ran.iter().find(|a| a.get(1).is_some_and(|k| k.ends_with(r"Dalica.Image\shell\open\command"))).unwrap();
    assert!(open[6].ends_with("\" --view \"%1\"") && open[6].starts_with('"'), "{open:?}");
    let started = reg.started.lock().unwrap().clone();
    assert_eq!(started, [Argv::new("explorer.exe").arg(default_viewer::DEFAULT_APPS)]);

    // None: the Capabilities go.
    reg.ran.lock().unwrap().clear();
    assert!(matches!(os.set_default_viewer(&[]).unwrap(), DefaultSet::SettingsOpened(_)));
    let ran: Vec<Vec<String>> = reg.ran.lock().unwrap().iter().map(|a| a.args.iter().map(|s| s.to_string_lossy().into_owned()).collect()).collect();
    assert!(ran.iter().any(|a| a == &["delete", r"HKCU\Software\Dalica\Capabilities", "/f"]));
    assert!(!ran.iter().any(|a| a.first().is_some_and(|c| c == "add") && a.get(1).is_some_and(|k| k.contains("Capabilities"))));
}

#[test]
fn macos_sets_nothing_and_says_how_to_choose() {
    let reg = Arc::new(Reg::default());
    let os = Native::with(Platform::MacOs, Env::default(), reg.clone(), no_bus());
    assert_eq!(os.default_handler(FileType::Jpeg).unwrap(), None);
    let r = os.set_default_viewer(&[FileType::Png]).unwrap();
    assert!(matches!(&r, DefaultSet::Choose(m) if m.contains("Get Info") && m.contains("Change All") && m.contains("PNG")), "{r:?}");
    assert!(reg.ran.lock().unwrap().is_empty() && reg.started.lock().unwrap().is_empty());
}

#[test]
fn the_packages_declare_the_types_dalica_offers() {
    let released: Vec<FileType> = VIEWER_TYPES.into_iter().filter(|t| !matches!(t, FileType::Heif | FileType::Avif)).collect();
    let desktop_entry = packaging("linux/org.dalica.Dalica.desktop");
    let declared: BTreeSet<&str> =
        desktop_entry.lines().find_map(|l| l.strip_prefix("MimeType=")).unwrap().split(';').filter(|s| !s.is_empty()).collect();
    let wanted: BTreeSet<&str> = released.iter().flat_map(|t| mime_types(*t).iter().copied()).collect();
    assert_eq!(declared, wanted, "the desktop entry's MimeType");
    assert!(desktop_entry.contains("\nExec=dalica %f\n"), "a file alone opens the viewer");

    let wxs = packaging("windows/dalica.wxs");
    let line = wxs.lines().find(|l| l.contains("define OpenWithExtensions")).unwrap();
    let exts: BTreeSet<&str> = line.split('=').nth(1).unwrap().trim().trim_end_matches("?>").trim().split(';').collect();
    let wanted: BTreeSet<&str> = released.iter().flat_map(|t| extensions(*t).iter().copied()).collect();
    assert_eq!(exts, wanted, "the MSI's extensions");
    assert!(wxs.contains(r#"Key="Software\RegisteredApplications" Name="Dalica""#), "the MSI lists Dalica in Default apps");

    let plist = packaging("macos/Info.plist.in");
    for uti in [
        "public.jpeg",
        "public.png",
        "public.tiff",
        "org.webmproject.webp",
        "com.compuserve.gif",
        "com.microsoft.bmp",
        "public.jpeg-xl",
        "com.adobe.photoshop-image",
        "public.camera-raw-image",
    ] {
        assert!(plist.contains(&format!("<string>{uti}</string>")), "{uti}");
    }
    assert!(plist.contains("<string>Viewer</string>") && !plist.contains("public.heic") && !plist.contains("public.avif"));
}
