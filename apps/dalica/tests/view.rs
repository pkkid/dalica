//! `dalica --view FILE`, what the file manager's Open with runs (`releasing.md` → Windows): the real
//! binary takes any file (existing, missing, odd characters, a leading `-`) without a usage error
//! and opens its library as usual. Started headless with every folder it could touch inside a
//! temporary directory; the test waits for the library to open, then stops it.

use std::ffi::OsString;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A port nobody listens on now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Kills the app if a test ends while it runs.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Starts `dalica --view FILE` headless on a library in `home` and waits until the library has
/// opened (its folder holds files); panics if the app exits first, with what it printed.
fn opens_with(home: &Path, file: OsString) {
    let library = home.join("library");
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let stderr = home.join("stderr.log");
    let mut c = Command::new(env!("CARGO_BIN_EXE_dalica"));
    c.arg("--view")
        .arg(&file)
        .arg("--library")
        .arg(&library)
        .args(["--control", &free_port().to_string(), "--headless", "--control-token-file"])
        .arg(home.join("control.token"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join("config")),
        ("XDG_CACHE_HOME", home.join("cache")),
        ("DALICA_CACHE_DIR", home.join("dalica-cache")),
        ("XDG_DATA_HOME", home.join("data")),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("APPDATA", home.join("appdata")),
        ("LOCALAPPDATA", home.join("localappdata")),
        ("TMPDIR", tmp.clone()),
        ("TMP", tmp.clone()),
        ("TEMP", tmp),
    ] {
        c.env(k, v);
    }
    for k in ["DALICA_HOME", "DALICA_CONTROL_PORT", "DALICA_CONTROL_TOKEN_FILE", "DALICA_PROFILE"] {
        c.env_remove(k);
    }
    let mut app = App(c.spawn().unwrap());
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        if std::fs::read_dir(&library).is_ok_and(|mut d| d.next().is_some()) {
            break;
        }
        if let Some(status) = app.0.try_wait().unwrap() {
            let said = std::fs::read_to_string(&stderr).unwrap_or_default();
            panic!("dalica --view {file:?} exited with {status} before opening its library:\n{said}");
        }
        assert!(Instant::now() < end, "dalica --view {file:?} did not open its library");
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = std::fs::read_to_string(&stderr).unwrap_or_default();
    assert!(!said.contains("usage:"), "dalica --view {file:?} printed its usage:\n{said}");
}

#[test]
fn view_opens_the_library_for_an_existing_file() {
    let home = tempfile::tempdir().unwrap();
    let photo = home.path().join("photo.jpg");
    std::fs::write(&photo, b"not really a JPEG").unwrap();
    opens_with(home.path(), photo.clone().into_os_string());
    assert_eq!(std::fs::read(&photo).unwrap(), b"not really a JPEG");
}

#[test]
fn view_opens_the_library_for_a_missing_file() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("no such folder").join("gone.cr3");
    opens_with(home.path(), missing.clone().into_os_string());
    assert!(!missing.exists());
    assert!(!missing.parent().unwrap().exists());
}

#[test]
fn view_opens_the_library_for_odd_file_names() {
    // Names Windows forbids (`"`, `*`, `?`, `<`, `>`, `|`, `:`) are only used where they can exist.
    let mut names = vec!["ünïcødé 写真 (1) & $(echo run).jpeg".to_string(), "-x.jpg".to_string(), "--demo".to_string()];
    if !cfg!(windows) {
        names.push("it's \"quoted\" *?<>|;.png".to_string());
    }
    for name in names {
        let home = tempfile::tempdir().unwrap();
        opens_with(home.path(), OsString::from(&name));
        assert!(!home.path().join("library").join(&name).exists());
    }
}
