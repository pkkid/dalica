//! The Linux start-up check: before the window opens, are the libraries its session loads at
//! run time installed? (`docs/releasing.md` → Linux.)
//!
//! Adapted from PhotoCraft `apps/photocraft/src/linux_libs.rs` @ b3993e89 (MIT OR Apache-2.0):
//! the session rule, the library table, the distro detection and the message. Changed for Dalica:
//! no `ldconfig` process; the check reads where the dynamic loader looks (`LD_LIBRARY_PATH`,
//! `/etc/ld.so.cache`, the directories `/etc/ld.so.conf` and its includes name, the default
//! directories), all under a root that tests plant; Vulkan is the only GPU library (Dalica has no
//! GL backend); a missing library exits with status 1.
//!
//! The windowing libraries (Wayland or X11, xkbcommon) and the GPU loader (Vulkan) are not ELF
//! `NEEDED` entries: winit and wgpu load them when the window opens, and some of those crates
//! panic when one is missing. The AppImage and the tarball cannot declare dependencies, so `dalica`
//! looks for the libraries first and, when one is missing, names the package that installs it and
//! exits with [`EXIT_MISSING`] instead of crashing (`docs/standards/never-crash.md`).
//!
//! No `dlopen` (that needs `unsafe`). A library counts as present when the loader's cache lists
//! it or a file of its name is in one of the loader's directories. Not finding one is conclusive
//! only on a system with a glibc loader configuration (`/etc/ld.so.conf` or `/etc/ld.so.cache`);
//! anywhere else it proves nothing and the window opens as usual.
//!
//! `DALICA_SKIP_LIB_CHECK=1` skips the check. `DALICA_LIB_CHECK_ROOT=DIR` reads every system file
//! under `DIR` instead of `/` (the packaging scripts plant a system there to test the refusal).
//! Every read is bounded: file sizes, include depth, the number of files and directories.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::OsString;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// The exit status when a library is missing.
pub const EXIT_MISSING: u8 = 1;

/// The variable that skips the check.
pub const SKIP_VAR: &str = "DALICA_SKIP_LIB_CHECK";

/// The variable that moves the system root (tests and the packaging scripts).
pub const ROOT_VAR: &str = "DALICA_LIB_CHECK_ROOT";

/// The largest configuration file read (`ld.so.conf`, an include, `os-release`).
const MAX_CONF_BYTES: u64 = 1 << 20;
/// The largest `ld.so.cache` read; a real one is well under a few megabytes.
const MAX_CACHE_BYTES: u64 = 64 << 20;
/// How deep `include` lines nest.
const MAX_INCLUDE_DEPTH: usize = 8;
/// How many configuration files one check reads.
const MAX_CONF_FILES: usize = 256;
/// How many directories one check searches.
const MAX_DIRS: usize = 512;

/// The display server winit connects to, chosen as winit 0.30 does: Wayland when
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty, else X11 when `DISPLAY` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Wayland,
    X11,
    /// Neither: winit reports that itself, as an error.
    None,
}

pub fn session_from_env(var: impl Fn(&str) -> Option<OsString>) -> Session {
    let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET") {
        Session::Wayland
    } else if set("DISPLAY") {
        Session::X11
    } else {
        Session::None
    }
}

/// A library loaded at run time and the package that provides it on each distribution family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lib {
    pub soname: &'static str,
    pub debian: &'static str,
    pub fedora: &'static str,
    pub arch: &'static str,
    pub suse: &'static str,
}

const fn lib(soname: &'static str, debian: &'static str, fedora: &'static str, arch: &'static str, suse: &'static str) -> Lib {
    Lib { soname, debian, fedora, arch, suse }
}

const XKBCOMMON: Lib = lib("libxkbcommon.so.0", "libxkbcommon0", "libxkbcommon", "libxkbcommon", "libxkbcommon0");
const XKBCOMMON_X11: Lib = lib("libxkbcommon-x11.so.0", "libxkbcommon-x11-0", "libxkbcommon-x11", "libxkbcommon-x11", "libxkbcommon-x11-0");
const X11: Lib = lib("libX11.so.6", "libx11-6", "libX11", "libx11", "libX11-6");
const X11_XCB: Lib = lib("libX11-xcb.so.1", "libx11-xcb1", "libX11-xcb", "libx11", "libX11-xcb1");
const XCB: Lib = lib("libxcb.so.1", "libxcb1", "libxcb", "libxcb", "libxcb1");
const XCURSOR: Lib = lib("libXcursor.so.1", "libxcursor1", "libXcursor", "libxcursor", "libXcursor1");
const XI: Lib = lib("libXi.so.6", "libxi6", "libXi", "libxi", "libXi6");
const WAYLAND_CLIENT: Lib = lib("libwayland-client.so.0", "libwayland-client0", "libwayland-client", "wayland", "libwayland-client0");
const VULKAN: Lib = lib("libvulkan.so.1", "libvulkan1", "vulkan-loader", "vulkan-icd-loader", "libvulkan1");

/// What winit 0.30 and wgpu load for a session. X11: x11-dl opens Xlib, Xcursor, Xlib-xcb and
/// XInput2, x11rb opens libxcb, xkbcommon-dl opens xkbcommon and xkbcommon-x11 (RandR goes
/// through x11rb's own protocol code). Wayland: libwayland-client (the cursor code is pure Rust)
/// and xkbcommon. The GPU: the Vulkan loader, wgpu's only backend on Linux here.
pub fn requirements(session: Session) -> Vec<Lib> {
    match session {
        Session::Wayland => vec![WAYLAND_CLIENT, XKBCOMMON, VULKAN],
        Session::X11 => vec![X11, X11_XCB, XCB, XCURSOR, XI, XKBCOMMON, XKBCOMMON_X11, VULKAN],
        Session::None => Vec::new(),
    }
}

/// `path` (absolute) under `root`: `/etc/ld.so.conf` under `/tmp/r` is `/tmp/r/etc/ld.so.conf`.
fn under(root: &Path, path: &Path) -> PathBuf {
    let mut out = root.to_path_buf();
    for c in path.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::ParentDir => out.push(".."),
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
        }
    }
    out
}

/// At most `cap` bytes of a file; `None` when it cannot be read.
fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Does `name` match `pattern`, where `*` stands for any run of characters and `?` for one?
/// (The globs `include` lines use, such as `ld.so.conf.d/*.conf`.)
fn glob_match(pattern: &[u8], name: &[u8]) -> bool {
    // Iterative, with one backtrack point: linear in practice and never recursive.
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == b'?' || name.get(n) == Some(&c) => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    pattern.get(p..).is_some_and(|rest| rest.iter().all(|&c| c == b'*'))
}

/// The files an `include` pattern names (absolute, before the root is applied), sorted as glob
/// sorts them. Wildcards count in the last component only, as in every real configuration.
fn include_files(root: &Path, pattern: &Path) -> Vec<PathBuf> {
    let Some(name) = pattern.file_name().map(|n| n.to_string_lossy().into_owned()) else { return Vec::new() };
    if !name.contains(['*', '?']) {
        return vec![pattern.to_path_buf()];
    }
    let Some(dir) = pattern.parent() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(under(root, dir)) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .take(MAX_CONF_FILES)
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            glob_match(name.as_bytes(), n.as_bytes()).then(|| dir.join(n))
        })
        .collect();
    files.sort();
    files
}

/// The directories `/etc/ld.so.conf` names, following `include` lines, in order.
struct ConfReader<'a> {
    root: &'a Path,
    files_read: usize,
    dirs: Vec<PathBuf>,
}

impl ConfReader<'_> {
    fn read(&mut self, file: &Path, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH || self.files_read >= MAX_CONF_FILES {
            return;
        }
        self.files_read += 1;
        let Some(bytes) = read_capped(&under(self.root, file), MAX_CONF_BYTES) else { return };
        let text = String::from_utf8_lossy(&bytes);
        let base = file.parent().unwrap_or(Path::new("/"));
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut words = line.split_whitespace();
            match words.next() {
                Some("include") => {
                    for pattern in words {
                        // A relative pattern is relative to the including file's folder.
                        let pattern = base.join(pattern);
                        for f in include_files(self.root, &pattern) {
                            self.read(&f, depth + 1);
                        }
                    }
                }
                Some("hwcap") => {}
                _ => {
                    // One folder per line; older files also separate them with `:` or `,`, and
                    // may add `=TYPE`.
                    for dir in line.split(|c: char| c.is_whitespace() || c == ':' || c == ',') {
                        let dir = dir.split('=').next().unwrap_or("");
                        if dir.starts_with('/') && self.dirs.len() < MAX_DIRS {
                            self.dirs.push(PathBuf::from(dir));
                        }
                    }
                }
            }
        }
    }
}

/// The multiarch folder name of this build (`/usr/lib/<triplet>` on Debian and Ubuntu).
fn triplet() -> &'static str {
    if cfg!(target_arch = "aarch64") { "aarch64-linux-gnu" } else { "x86_64-linux-gnu" }
}

/// Where the loader would look on the system under `root`, and what it would find.
pub struct Probe {
    /// The folders searched: `LD_LIBRARY_PATH` first, then `ld.so.conf`'s, then the defaults
    /// (absolute, before the root is applied).
    pub dirs: Vec<PathBuf>,
    /// `/etc/ld.so.cache`, when there is one.
    pub cache: Option<Vec<u8>>,
    /// The system has a glibc loader configuration, so a library not found is really missing.
    pub conclusive: bool,
    root: PathBuf,
}

impl Probe {
    /// Reads the loader's configuration under `root`; `ld_library_path` is `LD_LIBRARY_PATH`.
    pub fn read(root: &Path, ld_library_path: Option<&std::ffi::OsStr>) -> Probe {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(paths) = ld_library_path {
            dirs.extend(std::env::split_paths(paths).filter(|d| d.to_string_lossy().starts_with('/')).take(MAX_DIRS));
        }
        let conf = Path::new("/etc/ld.so.conf");
        let mut reader = ConfReader { root, files_read: 0, dirs: Vec::new() };
        reader.read(conf, 0);
        dirs.extend(reader.dirs);
        for d in ["/lib", "/usr/lib", "/lib64", "/usr/lib64"] {
            dirs.push(PathBuf::from(d));
        }
        dirs.push(Path::new("/lib").join(triplet()));
        dirs.push(Path::new("/usr/lib").join(triplet()));
        let cache_path = under(root, Path::new("/etc/ld.so.cache"));
        let cache = read_capped(&cache_path, MAX_CACHE_BYTES);
        let conclusive = under(root, conf).is_file() || cache.is_some();
        Probe { dirs, cache, conclusive, root: root.to_path_buf() }
    }

    /// Would the loader find `soname`?
    pub fn has(&self, soname: &str) -> bool {
        self.cache.as_deref().is_some_and(|c| cache_lists(c, soname)) || self.dirs.iter().any(|d| under(&self.root, &d.join(soname)).is_file())
    }
}

/// Does an `ld.so.cache` list `soname`? Its string table holds every library's full path, each
/// ending in a NUL byte, so `/<soname>\0` appears exactly when the library is listed.
pub fn cache_lists(cache: &[u8], soname: &str) -> bool {
    let mut needle = Vec::with_capacity(soname.len().saturating_add(2));
    needle.push(b'/');
    needle.extend_from_slice(soname.as_bytes());
    needle.push(0);
    cache.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// The libraries `session` needs that `probe` does not find.
pub fn missing(session: Session, probe: &Probe) -> Vec<Lib> {
    requirements(session).into_iter().filter(|l| !probe.has(l.soname)).collect()
}

/// Distribution family, from `os-release` `ID` and `ID_LIKE`, to name the right packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Debian,
    Fedora,
    Arch,
    Suse,
    Unknown,
}

pub fn distro_from_os_release(text: &str) -> Distro {
    let mut ids: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() == "ID" || key.trim() == "ID_LIKE" {
            ids.extend(value.trim().trim_matches(|c| c == '"' || c == '\'').split_whitespace().map(str::to_ascii_lowercase));
        }
    }
    let any = |names: &[&str]| ids.iter().any(|id| names.contains(&id.as_str()));
    if any(&["debian", "ubuntu"]) {
        Distro::Debian
    } else if any(&["fedora", "rhel", "centos"]) {
        Distro::Fedora
    } else if any(&["arch"]) {
        Distro::Arch
    } else if any(&["suse", "opensuse"]) || ids.iter().any(|id| id.starts_with("opensuse")) {
        Distro::Suse
    } else {
        Distro::Unknown
    }
}

fn package(lib: &Lib, distro: Distro) -> &'static str {
    match distro {
        Distro::Debian | Distro::Unknown => lib.debian,
        Distro::Fedora => lib.fedora,
        Distro::Arch => lib.arch,
        Distro::Suse => lib.suse,
    }
}

/// The packages for `missing`, once each, in order.
fn packages(missing: &[Lib], distro: Distro) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for p in missing.iter().map(|l| package(l, distro)) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// What to print when libraries are missing: which, and the command that installs them.
pub fn missing_message(session: Session, missing: &[Lib], distro: Distro) -> String {
    let session_name = match session {
        Session::Wayland => "Wayland",
        Session::X11 => "X11",
        Session::None => "display",
    };
    let names: Vec<&str> = missing.iter().map(|l| l.soname).collect();
    let list = packages(missing, distro).join(" ");
    let mut msg = format!(
        "dalica: cannot open the window: the {session_name} session needs libraries that are not installed: {}.\nInstall them with:\n",
        names.join(", ")
    );
    match distro {
        Distro::Debian => msg.push_str(&format!("  sudo apt install {list}\n")),
        Distro::Fedora => msg.push_str(&format!("  sudo dnf install {list}\n")),
        Distro::Arch => msg.push_str(&format!("  sudo pacman -S {list}\n")),
        Distro::Suse => msg.push_str(&format!("  sudo zypper install {list}\n")),
        Distro::Unknown => {
            msg.push_str(&format!("  Debian, Ubuntu: sudo apt install {list}\n"));
            msg.push_str(&format!("  Fedora: sudo dnf install {}\n", packages(missing, Distro::Fedora).join(" ")));
        }
    }
    msg.push_str(&format!("(Set {SKIP_VAR}=1 to start anyway.)\n"));
    msg
}

/// The distribution of the system under `root`.
fn distro(root: &Path) -> Distro {
    for f in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Some(bytes) = read_capped(&under(root, Path::new(f)), MAX_CONF_BYTES) {
            return distro_from_os_release(&String::from_utf8_lossy(&bytes));
        }
    }
    Distro::Unknown
}

/// The check, for the session the environment selects: `Err` carries the message to print before
/// exiting with [`EXIT_MISSING`]. `var` reads the environment.
pub fn check(var: impl Fn(&str) -> Option<OsString>) -> Result<(), String> {
    if var(SKIP_VAR).is_some_and(|v| !v.is_empty() && v != "0") {
        return Ok(());
    }
    let session = session_from_env(&var);
    if session == Session::None {
        return Ok(());
    }
    let root = var(ROOT_VAR).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let probe = Probe::read(&root, var("LD_LIBRARY_PATH").as_deref());
    let missing = missing(session, &probe);
    if missing.is_empty() || !probe.conclusive {
        return Ok(());
    }
    Err(missing_message(session, &missing, distro(&root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn the_session_follows_winit() {
        assert_eq!(session_from_env(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")])), Session::Wayland);
        assert_eq!(session_from_env(env(&[("WAYLAND_SOCKET", "3")])), Session::Wayland);
        assert_eq!(session_from_env(env(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", ":99")])), Session::X11);
        assert_eq!(session_from_env(env(&[("DISPLAY", "")])), Session::None);
        assert_eq!(session_from_env(env(&[])), Session::None);
    }

    #[test]
    fn globs_match_like_include_lines() {
        assert!(glob_match(b"*.conf", b"a.conf"));
        assert!(glob_match(b"*.conf", b".conf"));
        assert!(!glob_match(b"*.conf", b"a.conf.bak"));
        assert!(glob_match(b"lib?.conf", b"lib6.conf"));
        assert!(glob_match(b"*", b""));
        assert!(glob_match(b"a*b*c", b"axxbyyc"));
        assert!(!glob_match(b"a*b*c", b"axxbyy"));
        assert!(!glob_match(b"", b"x"));
        // A long hostile name stays fast (no exponential backtracking).
        let long = vec![b'a'; 10_000];
        assert!(!glob_match(b"*a*a*a*a*b", &long));
    }

    #[test]
    fn the_cache_lists_whole_names_only() {
        let cache = b"glibc-ld.so.cache1.1\0\0/usr/lib64/libX11.so.6\0libX11.so.6\0/lib/libxcb.so.1\0";
        assert!(cache_lists(cache, "libX11.so.6"));
        assert!(cache_lists(cache, "libxcb.so.1"));
        assert!(!cache_lists(cache, "libX11.so"));
        assert!(!cache_lists(cache, "libX11-xcb.so.1"));
        assert!(!cache_lists(b"", "libX11.so.6"));
    }

    /// A planted system: `etc/ld.so.conf` with an include, libraries in its folders, and a cache.
    struct Planted {
        dir: tempfile::TempDir,
    }

    impl Planted {
        fn new() -> Planted {
            Planted { dir: tempfile::tempdir().unwrap() }
        }
        fn root(&self) -> &Path {
            self.dir.path()
        }
        fn write(&self, rel: &str, text: &[u8]) {
            let p = self.root().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        fn env<'a>(&'a self, extra: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
            move |k| {
                if k == ROOT_VAR {
                    return Some(self.root().as_os_str().to_owned());
                }
                extra.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
            }
        }
    }

    const X11_ALL: [&str; 8] = [
        "libX11.so.6",
        "libX11-xcb.so.1",
        "libxcb.so.1",
        "libXcursor.so.1",
        "libXi.so.6",
        "libxkbcommon.so.0",
        "libxkbcommon-x11.so.0",
        "libvulkan.so.1",
    ];

    #[test]
    fn a_planted_missing_library_names_its_package() {
        let sys = Planted::new();
        sys.write("etc/os-release", b"NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n");
        sys.write("etc/ld.so.conf", b"# comment\ninclude ld.so.conf.d/*.conf\n");
        sys.write("etc/ld.so.conf.d/gfx.conf", b"/opt/gfx/lib # the GPU vendor's folder\n");
        sys.write("etc/ld.so.conf.d/skip.conf.bak", b"/opt/never\n");
        // Every X11 library but xkbcommon-x11: some in the include's folder, some in a default
        // folder, one only in the cache, and xkbcommon-x11 only in a folder no one searches.
        for name in &X11_ALL[..4] {
            sys.write(&format!("opt/gfx/lib/{name}"), b"");
        }
        sys.write("usr/lib/x86_64-linux-gnu/libXi.so.6", b"");
        sys.write("usr/lib/aarch64-linux-gnu/libXi.so.6", b"");
        sys.write("usr/lib64/libvulkan.so.1", b"");
        sys.write("etc/ld.so.cache", b"glibc-ld.so.cache1.1\0/somewhere/libxkbcommon.so.0\0");
        sys.write("opt/never/libxkbcommon-x11.so.0", b"");

        let refused = check(sys.env(&[("DISPLAY", ":0")]));
        let msg = refused.unwrap_err();
        assert!(msg.contains("the X11 session needs libraries that are not installed: libxkbcommon-x11.so.0."), "{msg}");
        assert!(msg.contains("sudo apt install libxkbcommon-x11-0\n"), "{msg}");
        assert!(msg.contains("DALICA_SKIP_LIB_CHECK=1"), "{msg}");
        assert_eq!(EXIT_MISSING, 1);

        // The skip variable, and no display at all, open the window anyway.
        assert_eq!(check(sys.env(&[("DISPLAY", ":0"), (SKIP_VAR, "1")])), Ok(()));
        assert!(check(sys.env(&[("DISPLAY", ":0"), (SKIP_VAR, "0")])).is_err());
        assert_eq!(check(sys.env(&[])), Ok(()));

        // Planting the library where the configuration points makes the check pass.
        sys.write("opt/gfx/lib/libxkbcommon-x11.so.0", b"");
        assert_eq!(check(sys.env(&[("DISPLAY", ":0")])), Ok(()));
    }

    #[test]
    fn wayland_needs_its_client_xkbcommon_and_vulkan() {
        let sys = Planted::new();
        sys.write("etc/os-release", b"ID=fedora\n");
        sys.write("etc/ld.so.conf", b"");
        let msg = check(sys.env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")])).unwrap_err();
        assert!(
            msg.contains("the Wayland session needs libraries that are not installed: libwayland-client.so.0, libxkbcommon.so.0, libvulkan.so.1."),
            "{msg}"
        );
        assert!(msg.contains("sudo dnf install libwayland-client libxkbcommon vulkan-loader\n"), "{msg}");
        assert!(!msg.contains("libX11"), "{msg}");
        // LD_LIBRARY_PATH counts, folder by folder.
        for name in ["libwayland-client.so.0", "libxkbcommon.so.0", "libvulkan.so.1"] {
            sys.write(&format!("ld/b/{name}"), b"");
        }
        let both = std::env::join_paths(["/ld/a", "/ld/b"]).unwrap();
        let both = both.to_string_lossy().into_owned();
        assert_eq!(check(sys.env(&[("WAYLAND_DISPLAY", "wayland-0"), ("LD_LIBRARY_PATH", both.as_str())])), Ok(()));
    }

    #[test]
    fn without_a_loader_configuration_nothing_is_concluded() {
        // No ld.so.conf and no cache (musl, an unusual system): the window opens as usual.
        let sys = Planted::new();
        sys.write("etc/os-release", b"ID=alpine\n");
        assert_eq!(check(sys.env(&[("DISPLAY", ":0")])), Ok(()));
        let probe = Probe::read(sys.root(), None);
        assert!(!probe.conclusive);
        assert_eq!(missing(Session::X11, &probe).len(), X11_ALL.len());
    }

    #[test]
    fn hostile_configurations_stay_bounded() {
        let sys = Planted::new();
        // An include that includes itself, a huge line, binary junk, a relative folder.
        sys.write("etc/ld.so.conf", b"include /etc/ld.so.conf\ninclude loop.conf\nrelative/lib\n/ok/lib=libc6\n\xff\xfe\0junk\n");
        sys.write("etc/loop.conf", b"include ld.so.conf\n");
        let probe = Probe::read(sys.root(), None);
        assert!(probe.conclusive);
        assert!(probe.dirs.iter().any(|d| d == Path::new("/ok/lib")));
        assert!(!probe.dirs.iter().any(|d| d == Path::new("relative/lib")));
        assert!(probe.dirs.len() <= MAX_DIRS + 6);
        // Many folders on one line are capped.
        let many: String = (0..5000).map(|i| format!("/d{i} ")).collect();
        sys.write("etc/ld.so.conf", many.as_bytes());
        assert!(Probe::read(sys.root(), None).dirs.len() <= MAX_DIRS + 6);
    }

    #[test]
    fn unknown_distributions_get_two_suggestions() {
        let msg = missing_message(Session::X11, &[XKBCOMMON_X11, VULKAN], Distro::Unknown);
        assert!(msg.contains("Debian, Ubuntu: sudo apt install libxkbcommon-x11-0 libvulkan1"), "{msg}");
        assert!(msg.contains("Fedora: sudo dnf install libxkbcommon-x11 vulkan-loader"), "{msg}");
        // One package for two libraries is named once (Arch: libx11 has libX11 and libX11-xcb).
        let msg = missing_message(Session::X11, &[X11, X11_XCB], Distro::Arch);
        assert!(msg.contains("sudo pacman -S libx11\n"), "{msg}");
        let msg = missing_message(Session::Wayland, &[VULKAN], Distro::Suse);
        assert!(msg.contains("sudo zypper install libvulkan1\n"), "{msg}");
    }

    #[test]
    fn distro_detection() {
        assert_eq!(distro_from_os_release("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"), Distro::Debian);
        assert_eq!(distro_from_os_release("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), Distro::Debian);
        assert_eq!(distro_from_os_release("ID=fedora\n"), Distro::Fedora);
        assert_eq!(distro_from_os_release("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"), Distro::Fedora);
        assert_eq!(distro_from_os_release("ID=endeavouros\nID_LIKE=arch\n"), Distro::Arch);
        assert_eq!(distro_from_os_release("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"), Distro::Suse);
        assert_eq!(distro_from_os_release("ID=nixos\n"), Distro::Unknown);
        assert_eq!(distro_from_os_release(""), Distro::Unknown);
        assert_eq!(distro_from_os_release("=\n==\nID\n"), Distro::Unknown);
    }

    #[test]
    fn paths_are_joined_under_the_root() {
        assert_eq!(under(Path::new("/r"), Path::new("/etc/ld.so.conf")), Path::new("/r").join("etc").join("ld.so.conf"));
        assert_eq!(under(Path::new("/"), Path::new("/usr/lib")), Path::new("/usr/lib"));
    }
}
