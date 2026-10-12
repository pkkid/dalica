//! Where photos can be: the standard places (Pictures, Documents, Desktop, Home), the mounted
//! volumes, whether a watched root is reachable, and the subfolders of a folder for the Folder
//! Manager (`data-model.md` §7, `organizing.md` §2).
//!
//! Everything here reads the disk (except [`Places::all`] and [`parse_mounts`]) and may wait on a
//! slow or sleeping network volume: call it from a worker thread, never the UI thread. Nothing here
//! writes, and nothing needs a C library: mounted volumes come from `/proc/self/mounts` on Linux,
//! the children of `/Volumes` on macOS and the drive letters on Windows.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use dalica_fs::{Entry, Limits};

use crate::skip::SkipRules;

/// The environment variable that, when set to a folder, makes [`Places::system`] answer the
/// places under that folder instead of the user's own (tests, demos, agents' sandboxes).
pub const PLACES_HOME_ENV: &str = "DALICA_PLACES_HOME";

/// Most subfolders [`children`] lists.
pub const CHILDREN_CAP: usize = 1000;

/// Most bytes of the mount table read.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
const MOUNTS_MAX_BYTES: u64 = 1 << 20;
/// Most mounted volumes listed.
pub const VOLUMES_CAP: usize = 256;

/// A standard place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlaceKind {
    Pictures,
    Documents,
    Desktop,
    Home,
}

/// A standard place and its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub kind: PlaceKind,
    pub path: PathBuf,
}

/// The user's standard places: from the environment the way the OS lays them out, or under one
/// folder ([`Places::under`], and [`PLACES_HOME_ENV`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    pub home: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub desktop: Option<PathBuf>,
}

/// Most bytes of the XDG user-dirs file read.
const USER_DIRS_MAX_BYTES: u64 = 64 * 1024;

impl Places {
    /// This user's places, from the process environment ([`Places::from_env`]).
    pub fn system() -> Places {
        Places::from_env(|k| std::env::var_os(k))
    }

    /// The places the environment (read through `var`) names: everything under
    /// [`PLACES_HOME_ENV`] when that is an absolute folder; otherwise the home (`HOME`, or
    /// `USERPROFILE` on Windows) and its `Pictures`, `Documents` and `Desktop` folders, which on
    /// Linux and the other Unix systems the XDG user-dirs file (`$XDG_CONFIG_HOME/user-dirs.dirs`,
    /// `~/.config` without it) may move. No home: no places.
    pub fn from_env(var: impl Fn(&str) -> Option<OsString>) -> Places {
        let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
        if let Some(home) = abs(PLACES_HOME_ENV) {
            return Places::under(&home);
        }
        let Some(home) = abs(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) else { return Places::default() };
        let mut places = Places::under(&home);
        if !cfg!(windows) && !cfg!(target_os = "macos") {
            let config = abs("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
            if let Some(text) = read_small(&config.join("user-dirs.dirs"), USER_DIRS_MAX_BYTES) {
                for (kind, path) in parse_user_dirs(&text, &home) {
                    match kind {
                        PlaceKind::Pictures => places.pictures = Some(path),
                        PlaceKind::Documents => places.documents = Some(path),
                        PlaceKind::Desktop => places.desktop = Some(path),
                        PlaceKind::Home => {}
                    }
                }
            }
        }
        places
    }

    /// `home` and its `Pictures`, `Documents` and `Desktop` folders.
    pub fn under(home: &Path) -> Places {
        Places {
            home: Some(home.to_path_buf()),
            pictures: Some(home.join("Pictures")),
            documents: Some(home.join("Documents")),
            desktop: Some(home.join("Desktop")),
        }
    }

    /// The places that are known, in the order Pictures, Documents, Desktop, Home. Does not touch
    /// the disk.
    pub fn all(&self) -> Vec<Place> {
        [
            (PlaceKind::Pictures, &self.pictures),
            (PlaceKind::Documents, &self.documents),
            (PlaceKind::Desktop, &self.desktop),
            (PlaceKind::Home, &self.home),
        ]
        .into_iter()
        .filter_map(|(kind, p)| p.as_ref().map(|path| Place { kind, path: path.clone() }))
        .collect()
    }

    /// The known places that are folders on disk, in [`Places::all`]'s order.
    pub fn existing(&self) -> Vec<Place> {
        self.all().into_iter().filter(|p| online(&p.path)).collect()
    }

    /// What the first run offers to watch ("Scan only my Pictures, Documents and Desktop
    /// folders"): those three when they exist, never the home itself (some systems name the home
    /// as an unset Pictures folder), and never a folder inside another one listed.
    pub fn first_run(&self) -> Vec<PathBuf> {
        let candidates: Vec<PathBuf> =
            self.existing().into_iter().filter(|p| p.kind != PlaceKind::Home && self.home.as_ref() != Some(&p.path)).map(|p| p.path).collect();
        let mut out: Vec<PathBuf> = Vec::new();
        for c in &candidates {
            let inside_other = candidates.iter().any(|o| o != c && c.starts_with(o));
            if !inside_other && !out.contains(c) {
                out.push(c.clone());
            }
        }
        out
    }
}

/// The Pictures, Documents and Desktop folders an XDG user-dirs file names
/// (`XDG_PICTURES_DIR="$HOME/Pictures"`): absolute paths or paths under `$HOME`; other lines,
/// other keys and other forms are ignored. Does not touch the disk.
pub fn parse_user_dirs(text: &str, home: &Path) -> Vec<(PlaceKind, PathBuf)> {
    let mut out = Vec::new();
    for line in text.lines().take(256) {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let kind = match key.trim() {
            "XDG_PICTURES_DIR" => PlaceKind::Pictures,
            "XDG_DOCUMENTS_DIR" => PlaceKind::Documents,
            "XDG_DESKTOP_DIR" => PlaceKind::Desktop,
            _ => continue,
        };
        let value = value.trim();
        let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { continue };
        let path = if let Some(rest) = value.strip_prefix("$HOME") {
            match rest.strip_prefix('/') {
                Some(rest) => home.join(rest),
                None if rest.is_empty() => home.to_path_buf(),
                None => continue,
            }
        } else if value.starts_with('/') {
            PathBuf::from(value)
        } else {
            continue;
        };
        out.push((kind, path));
    }
    out
}

/// A small text file, or `None` if it is missing, unreadable, too large or not UTF-8.
fn read_small(path: &Path, max: u64) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    let read = std::fs::File::open(path).ok()?.take(max.saturating_add(1)).read_to_string(&mut text).ok()?;
    (read as u64 <= max).then_some(text)
}

/// What kind of disk a volume is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VolumeKind {
    /// The startup disk (`/`, or the drive Windows runs from).
    System,
    /// Another local disk, card or stick.
    Mounted,
    /// A network share.
    Network,
}

/// A mounted volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// The volume's name: its mount folder's name, `/` for the Unix root, `C:` for a drive.
    pub name: String,
    /// Where it is mounted.
    pub path: PathBuf,
    pub kind: VolumeKind,
}

/// The mounted volumes of this system, the startup disk first, at most [`VOLUMES_CAP`].
pub fn volumes() -> Vec<Volume> {
    let mut out = platform_volumes();
    out.truncate(VOLUMES_CAP);
    out
}

#[cfg(windows)]
fn platform_volumes() -> Vec<Volume> {
    let system = std::env::var("SystemDrive").ok().map(|d| d.to_ascii_uppercase());
    // A: and B: are left out: asking about an empty floppy drive can stall for seconds.
    (b'C'..=b'Z')
        .filter_map(|letter| {
            let name = format!("{}:", char::from(letter));
            let path = PathBuf::from(format!("{name}\\"));
            online(&path).then(|| {
                let kind = if system.as_deref() == Some(name.as_str()) { VolumeKind::System } else { VolumeKind::Mounted };
                Volume { name, path, kind }
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn platform_volumes() -> Vec<Volume> {
    let root = Path::new("/");
    let mut out = vec![Volume { name: "/".into(), path: root.to_path_buf(), kind: VolumeKind::System }];
    // `/Volumes` also holds a link to the startup disk; it is the root already listed.
    out.extend(volumes_in(&[PathBuf::from("/Volumes")]).into_iter().filter(|v| !matches!(std::fs::canonicalize(&v.path), Ok(c) if c == root)));
    out
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_volumes() -> Vec<Volume> {
    use std::io::Read;
    let table = std::fs::File::open("/proc/self/mounts").and_then(|f| {
        let mut text = String::new();
        f.take(MOUNTS_MAX_BYTES).read_to_string(&mut text).map(|_| text)
    });
    match table {
        Ok(text) => parse_mounts(&text),
        // No mount table (a sandbox, a BSD): the root, then the folders desktops mount disks in.
        Err(_) => {
            let mut parents = Vec::new();
            if let Ok(user) = std::env::var("USER") {
                parents.push(PathBuf::from(format!("/media/{user}")));
                parents.push(PathBuf::from(format!("/run/media/{user}")));
            }
            parents.push(PathBuf::from("/mnt"));
            let mut out = vec![Volume { name: "/".into(), path: PathBuf::from("/"), kind: VolumeKind::System }];
            out.extend(volumes_in(&parents));
            out
        }
    }
}

/// File systems that are network shares.
const NETWORK_FS: [&str; 11] = ["nfs", "nfs4", "cifs", "smb3", "smbfs", "afs", "ceph", "glusterfs", "9p", "fuse.sshfs", "fuse.rclone"];

/// Folders under which local disks, cards and sticks are mounted.
const MOUNT_PARENTS: [&str; 3] = ["/media/", "/run/media/", "/mnt/"];

/// The volumes listed in a Linux mount table (the text of `/proc/self/mounts`): the root, disks
/// mounted under `/media`, `/run/media` and `/mnt`, and network shares wherever they are. Pseudo
/// and system file systems are left out. Does not touch the disk.
pub fn parse_mounts(table: &str) -> Vec<Volume> {
    let mut out: Vec<Volume> = Vec::new();
    for line in table.lines().take(VOLUMES_CAP.saturating_mul(16)) {
        let mut fields = line.split_ascii_whitespace();
        let (Some(_device), Some(point), Some(fs)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let point = unescape_mount(point);
        let kind = if point == "/" {
            VolumeKind::System
        } else if NETWORK_FS.contains(&fs) {
            VolumeKind::Network
        } else if MOUNT_PARENTS.iter().any(|p| point.starts_with(p)) && !is_pseudo_fs(fs) {
            VolumeKind::Mounted
        } else {
            continue;
        };
        let path = PathBuf::from(&point);
        if out.iter().any(|v| v.path == path) {
            continue;
        }
        let name = if point == "/" {
            "/".to_string()
        } else {
            // A Linux mount point is `/`-separated text whatever system reads the table (on
            // Windows `Path` would also split at a backslash in the name).
            point.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or(&point).to_string()
        };
        out.push(Volume { name, path, kind });
        if out.len() >= VOLUMES_CAP {
            break;
        }
    }
    // The startup disk first, the rest in table order.
    out.sort_by_key(|v| v.kind != VolumeKind::System);
    out
}

fn is_pseudo_fs(fs: &str) -> bool {
    matches!(
        fs,
        "tmpfs"
            | "devtmpfs"
            | "proc"
            | "sysfs"
            | "cgroup"
            | "cgroup2"
            | "autofs"
            | "devpts"
            | "mqueue"
            | "debugfs"
            | "tracefs"
            | "securityfs"
            | "pstore"
            | "bpf"
            | "configfs"
            | "fusectl"
            | "binfmt_misc"
            | "hugetlbfs"
            | "overlay"
            | "squashfs"
            | "nsfs"
            | "ramfs"
    )
}

/// A mount point as the table spells it, with `\040` (space), `\011` (tab), `\012` (newline) and
/// `\134` (backslash) turned back into their characters.
fn unescape_mount(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let code = rest.get(i + 1..i + 4);
        match code.and_then(|c| u8::from_str_radix(c, 8).ok()) {
            Some(b) if b.is_ascii() => {
                out.push(char::from(b));
                rest = rest.get(i + 4..).unwrap_or_default();
            }
            _ => {
                out.push('\\');
                rest = rest.get(i + 1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// The folders inside `parents` (each a folder whose children are mount points, like
/// `/Volumes`), as mounted volumes, sorted by name within each parent. Missing parents are
/// skipped. The listing used on macOS, and by tests on any system.
pub fn volumes_in(parents: &[PathBuf]) -> Vec<Volume> {
    let mut out = Vec::new();
    for parent in parents {
        let Ok(list) = children_capped(parent, VOLUMES_CAP) else { continue };
        for c in list.folders {
            if out.len() >= VOLUMES_CAP {
                return out;
            }
            if !out.iter().any(|v: &Volume| v.path == c.path) {
                out.push(Volume { name: c.name, path: c.path, kind: VolumeKind::Mounted });
            }
        }
    }
    out
}

/// Whether the watched root `root` can be reached: it is a folder on a mounted, readable volume.
/// A root on an unplugged disk, or renamed or removed, is offline (`data-model.md` §7).
pub fn online(root: &Path) -> bool {
    std::fs::metadata(root).is_ok_and(|m| m.is_dir())
}

/// A subfolder listed by [`children`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Child {
    pub name: String,
    pub path: PathBuf,
    /// Reached through a symbolic link or junction.
    pub is_link: bool,
}

/// The subfolders of a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Children {
    /// Sorted by name (the bytes of the name, as the walk sorts).
    pub folders: Vec<Child>,
    /// More subfolders exist than the cap allowed.
    pub truncated: bool,
}

/// The subfolders of `path`, at most [`CHILDREN_CAP`], for the Folder Manager: the ones a scan
/// would walk (the skip rules apply), links to folders included.
pub fn children(path: &Path) -> dalica_fs::Result<Children> {
    children_capped(path, CHILDREN_CAP)
}

/// [`children`] with another cap.
pub fn children_capped(path: &Path, cap: usize) -> dalica_fs::Result<Children> {
    let rules = SkipRules::system();
    // Files are skipped before they count, so the entry limit counts folders; one more than the
    // cap tells whether there were more.
    let limits = Limits { max_depth: 1, max_entries: cap.saturating_add(1), cancel: None };
    let walk = dalica_fs::walk(path, limits, |e: &Entry| !e.is_dir || rules.skips(e))?;
    let mut folders: Vec<Child> =
        walk.entries.into_iter().filter(|e| e.is_dir).map(|e| Child { name: e.name().into_owned(), path: e.path, is_link: e.is_link }).collect();
    folders.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    let truncated = folders.len() > cap;
    folders.truncate(cap);
    Ok(Children { folders, truncated })
}
